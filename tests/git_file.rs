//! 集成测试：真实仓库上验证文件读写、blame、README 定位、路径越界拒绝与克隆。
//!
//! 所有临时仓库都建在 `tempfile` 目录里，绝不触碰用户真实仓库。

use std::path::Path;
use std::process::Command;

use git_switch::git::clone::{CloneRequest, clone_repo, is_local_source};
use git_switch::git::config::read_local;
use git_switch::git::file::{
    FileError, blame, find_readme, list_dir, read_file, read_readme, write_file,
};
use git_switch::model::highlight::{Language, diagnose, highlight};
use git_switch::model::identity::{AuthMode, Identity};
use git_switch::model::repository::{EffectiveIdentity, ValueSource};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()
        .expect("无法执行 git，请确认已安装");
    assert!(
        out.status.success(),
        "git {args:?} 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(dir: &Path, name: &str, content: &str) {
    if let Some(parent) = dir.join(name).parent() {
        std::fs::create_dir_all(parent).expect("建目录");
    }
    std::fs::write(dir.join(name), content).expect("写文件");
}

/// 建一个带 README、源码与两次提交的仓库。
fn sample_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("临时目录");
    let path = dir.path();
    git(path, &["init", "-q", "-b", "main"]);
    write(
        path,
        "README.md",
        "# 示例项目\n\n用于集成测试。\n\n## 用法\n\n执行 `git switch` 即可。\n",
    );
    write(
        path,
        "src/main.rs",
        "fn main() {\n    println!(\"hello\");\n}\n",
    );
    write(path, "docs/README.md", "# 子文档\n");
    git(path, &["add", "-A"]);
    git(path, &["commit", "-q", "-m", "初始提交"]);

    write(
        path,
        "src/main.rs",
        "fn main() {\n    println!(\"hello\");\n    println!(\"again\");\n}\n",
    );
    git(path, &["add", "-A"]);
    git(path, &["commit", "-q", "-m", "追加一行"]);
    dir
}

fn identity() -> Identity {
    Identity {
        id: "gh-me".into(),
        platform_id: "github".into(),
        label: "张鑫宇 · 个人".into(),
        user_name: "张鑫宇".into(),
        user_email: "me@example.com".into(),
        auth: AuthMode::Ssh,
        ssh_key_path: None,
    }
}

fn effective() -> EffectiveIdentity {
    EffectiveIdentity {
        user_name: "张鑫宇".into(),
        user_email: "me@example.com".into(),
        name_source: ValueSource::Identity,
        email_source: ValueSource::Identity,
    }
}

#[test]
fn reads_real_file_and_counts_lines() {
    let repo = sample_repo();
    let content = read_file(repo.path(), "README.md").expect("应能读");
    assert!(content.text.starts_with("# 示例项目"));
    assert_eq!(content.line_count, 7, "实际：{:?}", content.lines());
    assert_eq!(content.path, "README.md");
    assert!(content.size > 0);
}

#[test]
fn rejects_path_escape() {
    let repo = sample_repo();
    for bad in ["../Cargo.toml", "src/../../etc/passwd", "/etc/passwd"] {
        let err = read_file(repo.path(), bad).expect_err("必须拒绝越界路径");
        assert!(
            matches!(err, FileError::InvalidPath(_)),
            "{bad} 应报 InvalidPath，实际：{err}"
        );
    }
    // 越界写入同样必须拒绝，且不能真的写出文件
    let err = write_file(repo.path(), "../escape.txt", "x").expect_err("必须拒绝越界写入");
    assert!(matches!(err, FileError::InvalidPath(_)));
    assert!(
        !repo
            .path()
            .parent()
            .expect("父目录")
            .join("escape.txt")
            .exists(),
        "不允许在仓库外写出文件"
    );
}

#[test]
fn write_then_read_back_and_status_sees_it() {
    let repo = sample_repo();
    write_file(repo.path(), "notes/todo.md", "- 第一项\n").expect("应能写");
    let back = read_file(repo.path(), "notes/todo.md").expect("应能读回");
    assert_eq!(back.text, "- 第一项\n");

    let status = git_switch::git::read_status(repo.path()).expect("应能读状态");
    assert!(
        status.files.iter().any(|c| c.path == "notes/todo.md"),
        "改动列表应包含新文件：{:?}",
        status.files
    );
}

#[test]
fn lists_tree_skipping_git_dir() {
    let repo = sample_repo();
    let root = list_dir(repo.path(), "").expect("应能列出根目录");
    let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"src"), "{names:?}");
    assert!(names.contains(&"README.md"), "{names:?}");
    assert!(!names.contains(&".git"), "不应列出 .git");
    assert!(root.iter().any(|e| e.is_dir && e.name == "src"));

    let src = list_dir(repo.path(), "src").expect("应能列出子目录");
    assert_eq!(src.len(), 1);
    assert_eq!(src[0].path, "src/main.rs");
    assert_eq!(src[0].depth, 1);
}

#[test]
fn finds_readme_in_root_first() {
    let repo = sample_repo();
    assert_eq!(find_readme(repo.path()).as_deref(), Some("README.md"));
    let readme = read_readme(repo.path()).expect("应能读到 README");
    assert!(readme.text.contains("示例项目"));
}

#[test]
fn missing_readme_is_none() {
    let dir = tempfile::tempdir().expect("临时目录");
    git(dir.path(), &["init", "-q", "-b", "main"]);
    assert!(find_readme(dir.path()).is_none());
    assert!(read_readme(dir.path()).is_none());
}

#[test]
fn blame_attributes_lines_to_real_commits() {
    let repo = sample_repo();
    let lines = blame(repo.path(), "src/main.rs").expect("应能 blame");
    // 文件共 4 行：fn main() { / hello / again / }
    assert_eq!(lines.len(), 4, "实际：{lines:?}");
    for (idx, line) in lines.iter().enumerate() {
        assert_eq!(line.line_no, idx + 1, "行号应连续：{lines:?}");
    }

    // 第 2 行（hello）来自首个提交，第 3 行（again）来自第二个提交
    assert_ne!(
        lines[1].commit, lines[2].commit,
        "不同提交的行应有不同 commit：{lines:?}"
    );
    // 每次提交的短 id 都是 7 位十六进制
    for line in &lines {
        assert_eq!(line.commit.len(), 7, "短 id 应为 7 位：{line:?}");
        assert!(line.commit.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!line.author.is_empty(), "应带作者：{line:?}");
        assert_eq!(line.date.len(), 10, "日期应为 YYYY-MM-DD：{line:?}");
        assert_eq!(line.date.chars().nth(4), Some('-'));
        assert_eq!(line.date.chars().nth(7), Some('-'));
    }
}

#[test]
fn blame_of_untracked_file_fails_without_faking() {
    let repo = sample_repo();
    write(repo.path(), "fresh.txt", "新文件\n");
    let err = blame(repo.path(), "fresh.txt").expect_err("未跟踪文件不能 blame 成功");
    assert!(!err.to_string().is_empty(), "必须给出真实原因");
}

#[test]
fn highlight_and_diagnose_real_rust_file() {
    let repo = sample_repo();
    let content = read_file(repo.path(), "src/main.rs").expect("应能读");
    let lang = Language::from_path(Path::new(&content.path));
    assert_eq!(lang, Language::Rust);

    let toks = highlight(&content.text, lang);
    assert_eq!(toks.len(), content.line_count, "每行都要有 token 列表");
    assert!(
        toks.iter().flatten().any(|_| true),
        "至少应识别出若干 token"
    );
    assert!(
        diagnose(&content.text, lang).is_empty(),
        "正常的 Rust 文件不应有诊断：{:?}",
        diagnose(&content.text, lang)
    );
}

#[test]
fn diagnose_flags_broken_file() {
    let broken = "fn main() {\n    let x = 1;\n";
    let diags = diagnose(broken, Language::Rust);
    assert!(!diags.is_empty(), "缺右花括号必须报出来");
    assert!(diags.iter().any(|d| d.message.contains('{')), "{diags:?}");
}

#[test]
fn clone_into_temp_dir_applies_identity_and_leaves_source_untouched() {
    let src = sample_repo();
    let before = std::fs::read_to_string(src.path().join(".git/config")).expect("读源配置");

    let dst_root = tempfile::tempdir().expect("目标目录");
    let req = CloneRequest::new(src.path().display().to_string(), dst_root.path());
    let outcome = clone_repo(&req, Some((&identity(), &effective()))).expect("应能克隆");

    // 克隆结果真实存在
    assert!(outcome.path.join("README.md").is_file());
    assert!(outcome.path.join(".git").is_dir());
    assert!(!outcome.output.is_empty(), "应带回 git 的真实输出");

    // 身份只写进新仓库
    let local = read_local(&outcome.path).expect("读新仓库配置");
    assert_eq!(local.user_name.as_deref(), Some("张鑫宇"));
    assert_eq!(local.user_email.as_deref(), Some("me@example.com"));

    // 源仓库配置一个字节都没变
    let after = std::fs::read_to_string(src.path().join(".git/config")).expect("读源配置");
    assert_eq!(before, after, "克隆不得改动源仓库的 .git/config");
}

#[test]
fn clone_into_non_empty_dir_is_refused_without_damage() {
    let src = sample_repo();
    let dst_root = tempfile::tempdir().expect("目标目录");
    let name = "占用目录";
    let occupied = dst_root.path().join(name);
    std::fs::create_dir_all(&occupied).expect("建目录");
    write(&occupied, "keep.txt", "重要内容\n");

    let req =
        CloneRequest::new(src.path().display().to_string(), dst_root.path()).with_dir_name(name);
    let err = clone_repo(&req, None).expect_err("非空目录必须拒绝克隆");
    assert!(err.to_string().contains("已存在"), "实际：{err}");
    assert_eq!(
        std::fs::read_to_string(occupied.join("keep.txt")).expect("读"),
        "重要内容\n",
        "原有内容不得被改动"
    );
}

#[test]
fn local_source_is_recognized_and_remote_is_not() {
    assert!(is_local_source("/Users/mac/WorkSpace/x"));
    assert!(!is_local_source(
        "git@github.com:FischelChern/git-switch.git"
    ));
    assert!(!is_local_source("https://gitee.com/o/r.git"));
}
