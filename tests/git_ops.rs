//! 集成测试：真实仓库上验证 diff、分支、提交与扫描。

use std::path::Path;
use std::process::Command;

use git_switch::git::{branch, diff, discover, ops, status::read_status};

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

fn init(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
}

fn write(dir: &Path, name: &str, content: &str) {
    if let Some(parent) = dir.join(name).parent() {
        std::fs::create_dir_all(parent).expect("建目录");
    }
    std::fs::write(dir.join(name), content).expect("写文件");
}

fn commit_all(dir: &Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// 建一个含多文件、多次提交的仓库。
fn seed(dir: &Path) {
    init(dir);
    write(dir, "a.txt", "one\ntwo\nthree\n");
    write(dir, "b.txt", "bee\n");
    commit_all(dir, "first");
}

#[test]
fn worktree_diff_reports_real_changes() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());

    write(dir.path(), "a.txt", "one\nTWO\nthree\nadded\n");

    let files = diff::read_worktree_diff(dir.path()).expect("应能读取 diff");
    assert_eq!(files.len(), 1, "只改了一个文件");
    let f = &files[0];
    assert_eq!(f.new_path, "a.txt");
    assert_eq!(f.additions, 2, "两行新增（TWO 与 added）");
    assert_eq!(f.deletions, 1, "一行删除（two）");

    // 有新增行与删除行
    let kinds: Vec<_> = f.rows.iter().map(|r| r.kind()).collect();
    assert!(kinds.contains(&git_switch::git::RowKind::Added));
    assert!(kinds.contains(&git_switch::git::RowKind::Removed));
    assert!(kinds.contains(&git_switch::git::RowKind::Context));
}

#[test]
fn worktree_diff_is_empty_for_clean_repo() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    let files = diff::read_worktree_diff(dir.path()).expect("应能读取");
    assert!(files.is_empty(), "干净仓库不应有 diff");
}

#[test]
fn file_diff_for_tracked_file() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    write(dir.path(), "a.txt", "changed\n");

    let f = diff::read_file_diff(dir.path(), "a.txt")
        .expect("应能读取")
        .expect("应有 diff");
    assert_eq!(f.new_path, "a.txt");
    assert!(f.deletions >= 1);
}

#[test]
fn file_diff_for_untracked_file() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    write(dir.path(), "new.txt", "brand new\n");

    let result = diff::read_file_diff(dir.path(), "new.txt").expect("应能读取");
    // 未跟踪文件走 --no-index，是否产出 diff 取决于 git 版本；至少不能报错
    if let Some(f) = result {
        assert!(f.additions >= 1 || f.binary);
    }
}

#[test]
fn side_by_side_conversion_on_real_diff() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    write(dir.path(), "a.txt", "one\nTWO\nthree\n");

    let files = diff::read_worktree_diff(dir.path()).expect("读取");
    let side = diff::to_side_by_side(&files[0].rows);
    assert!(!side.is_empty());
    // 至少有一行左右都有内容（来自上下文或配对）
    assert!(side.iter().any(|r| r.left.is_some() && r.right.is_some()));
}

#[test]
fn branch_create_list_rename_delete() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());

    branch::create_branch(dir.path(), "feat/x", None, false).expect("应能新建");
    let branches = branch::list_branches(dir.path()).expect("应能列出");
    assert!(
        branches.iter().any(|b| b.name == "feat/x"),
        "应包含 feat/x：{branches:?}"
    );
    assert!(
        branches.iter().any(|b| b.name == "main" && b.current),
        "main 应为当前分支"
    );

    branch::rename_branch(dir.path(), "feat/x", "feat/y").expect("应能重命名");
    let branches = branch::list_branches(dir.path()).expect("列出");
    assert!(branches.iter().any(|b| b.name == "feat/y"));
    assert!(!branches.iter().any(|b| b.name == "feat/x"));

    branch::delete_branch(dir.path(), "feat/y", false).expect("应能删除");
    let branches = branch::list_branches(dir.path()).expect("列出");
    assert!(!branches.iter().any(|b| b.name == "feat/y"));
}

#[test]
fn branch_checkout_switches_current() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    branch::create_branch(dir.path(), "dev", None, false).expect("新建");

    branch::checkout_branch(dir.path(), "dev").expect("应能检出");
    let branches = branch::list_branches(dir.path()).expect("列出");
    let current = branches.iter().find(|b| b.current).expect("应有当前分支");
    assert_eq!(current.name, "dev");
}

#[test]
fn invalid_branch_name_is_rejected_before_running_git() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    let err = branch::create_branch(dir.path(), "bad name", None, false).expect_err("应拒绝");
    assert!(
        err.to_string().contains("空格"),
        "错误信息应说明原因：{err}"
    );
}

#[test]
fn duplicate_branch_reports_git_error() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    branch::create_branch(dir.path(), "dup", None, false).expect("第一次应成功");
    let err = branch::create_branch(dir.path(), "dup", None, false).expect_err("第二次应失败");
    assert!(!err.to_string().is_empty());
}

#[test]
fn tags_create_list_delete() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());

    branch::create_tag(dir.path(), "v1.0.0", None).expect("应能打轻量标签");
    branch::create_tag(dir.path(), "v1.1.0", Some("带说明的标签")).expect("应能打附注标签");

    let tags = branch::list_tags(dir.path()).expect("列出");
    assert!(tags.contains(&"v1.0.0".to_string()), "tags: {tags:?}");
    assert!(tags.contains(&"v1.1.0".to_string()));

    branch::delete_tag(dir.path(), "v1.0.0").expect("应能删除");
    let tags = branch::list_tags(dir.path()).expect("列出");
    assert!(!tags.contains(&"v1.0.0".to_string()));
}

#[test]
fn commit_writes_history_and_rejects_empty_message() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    write(dir.path(), "c.txt", "cee\n");

    // 空信息必须先被拦下，且不产生提交
    let err = ops::commit(dir.path(), "   ", false).expect_err("空信息应被拒绝");
    assert!(err.to_string().contains("不能为空"));
    let log = git(dir.path(), &["log", "--oneline"]);
    assert_eq!(log.lines().count(), 1, "不应产生新提交");

    // 新增文件不被 add -u 覆盖，必须先整体暂存
    ops::stage_all(dir.path()).expect("暂存");
    let result = ops::commit(dir.path(), "feat: 新增 c.txt", false).expect("应能提交");
    assert!(!result.display().is_empty());

    let log = git(dir.path(), &["log", "--oneline"]);
    assert_eq!(log.lines().count(), 2);
}

#[test]
fn commit_with_only_untracked_files_reports_real_reason() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    write(dir.path(), "only_new.txt", "x\n");

    // stage_tracked=true 只暂存已跟踪文件，新文件未被暂存 → 提交应失败
    let err = ops::commit(dir.path(), "try", true).expect_err("没有暂存内容应失败");
    let text = err.to_string();
    // 关键：错误信息不能是空壳，必须带上 git 的真实输出
    assert!(
        text.to_lowercase().contains("nothing")
            || text.contains("未暂存")
            || text.contains("no changes"),
        "错误信息应说明真实原因，实际：{text}"
    );
}

#[test]
fn stage_all_picks_up_untracked() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    write(dir.path(), "new.txt", "n\n");

    ops::stage_all(dir.path()).expect("应能暂存");
    let out = git(dir.path(), &["diff", "--cached", "--name-only"]);
    assert!(out.contains("new.txt"), "暂存区应包含 new.txt：{out}");
}

#[test]
fn stash_and_pop_roundtrip() {
    let dir = tempfile::tempdir().expect("临时目录");
    seed(dir.path());
    write(dir.path(), "a.txt", "dirty\n");

    ops::stash(dir.path()).expect("应能贮藏");
    let status = read_status(dir.path()).expect("读状态");
    assert!(!status.is_dirty(), "贮藏后应干净：{:?}", status.files);

    ops::stash_pop(dir.path()).expect("应能弹出");
    let status = read_status(dir.path()).expect("读状态");
    assert!(status.is_dirty(), "弹出后应重新出现改动");
}

#[test]
fn merge_conflict_is_detected() {
    let dir = tempfile::tempdir().expect("临时目录");
    init(dir.path());
    write(dir.path(), "f.txt", "base\n");
    commit_all(dir.path(), "base");

    branch::create_branch(dir.path(), "other", None, true).expect("建分支");
    write(dir.path(), "f.txt", "other side\n");
    commit_all(dir.path(), "other change");

    branch::checkout_branch(dir.path(), "main").expect("切回 main");
    write(dir.path(), "f.txt", "main side\n");
    commit_all(dir.path(), "main change");

    // 合并应冲突
    let err = ops::merge(dir.path(), "other").expect_err("应发生冲突");
    assert!(!err.to_string().is_empty(), "应带 git 的 stderr");

    assert!(ops::has_conflicts(dir.path()), "应检测到冲突");
    let files = ops::conflicted_files(dir.path());
    assert_eq!(files, vec!["f.txt".to_string()]);
}

#[test]
fn scan_finds_repos_and_matches_platform() {
    let root = tempfile::tempdir().expect("临时目录");

    let a = root.path().join("github-project");
    std::fs::create_dir_all(&a).expect("建目录");
    init(&a);
    git(&a, &["remote", "add", "origin", "git@github.com:o/r.git"]);

    let b = root.path().join("plain-project");
    std::fs::create_dir_all(&b).expect("建目录");
    init(&b);

    let found = discover::scan(root.path(), &discover::ScanOptions::default());
    assert_eq!(found.len(), 2, "应发现两个仓库：{found:?}");

    let platforms = git_switch::model::platform::Platform::presets();
    let gh = found
        .iter()
        .find(|f| f.dir_name() == "github-project")
        .expect("应找到 github-project");
    let matched = gh.platform(&platforms).expect("应匹配 GitHub");
    assert_eq!(matched.id, "github");

    let plain = found
        .iter()
        .find(|f| f.dir_name() == "plain-project")
        .expect("应找到 plain-project");
    assert!(
        plain.platform(&platforms).is_none(),
        "无 remote 不应匹配平台"
    );
}

#[test]
fn scan_converts_to_repository_with_stable_id() {
    let root = tempfile::tempdir().expect("临时目录");
    let a = root.path().join("proj");
    std::fs::create_dir_all(&a).expect("建目录");
    init(&a);
    git(
        &a,
        &["remote", "add", "origin", "https://github.com/o/r.git"],
    );

    let found = discover::scan(root.path(), &discover::ScanOptions::default());
    let repo = discover::to_repository(&found[0], "github");
    assert_eq!(repo.platform_id, "github");
    assert_eq!(repo.path, a);
    assert_eq!(repo.remote_url, "https://github.com/o/r.git");
    assert!(repo.identity_id.is_none(), "新发现的项目应未绑定身份");
    assert_eq!(repo.id, a.to_string_lossy(), "id 用路径保证稳定");
}
