//! 集成测试：在真实仓库上构造分支与合并，验证提交图与工作区状态解析。

use std::path::Path;
use std::process::Command;

use git_switch::git::{ChangeStatus, read_graph, read_status};

/// 在指定目录执行 git，失败即 panic（测试里允许）。
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
    std::fs::write(dir.join(name), content).expect("写文件");
}

/// 建一个带提交、分支、合并的仓库。
fn build_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    write(dir, "a.txt", "one\n");
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-q", "-m", "first"]);

    write(dir, "a.txt", "one\ntwo\n");
    git(dir, &["commit", "-q", "-am", "second"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    write(dir, "b.txt", "feature\n");
    git(dir, &["add", "b.txt"]);
    git(dir, &["commit", "-q", "-m", "feature work"]);

    git(dir, &["checkout", "-q", "main"]);
    write(dir, "c.txt", "main side\n");
    git(dir, &["add", "c.txt"]);
    git(dir, &["commit", "-q", "-m", "main side work"]);

    git(
        dir,
        &[
            "merge",
            "-q",
            "--no-ff",
            "feature",
            "-m",
            "Merge branch 'feature'",
        ],
    );
}

#[test]
fn graph_reads_commits_with_merge_and_refs() {
    let dir = tempfile::tempdir().expect("临时目录");
    build_repo(dir.path());

    let graph = read_graph(dir.path(), 50).expect("应能读取提交图");

    assert!(graph.commits.len() >= 5, "提交数：{}", graph.commits.len());

    let merges: Vec<_> = graph.commits.iter().filter(|c| c.is_merge()).collect();
    assert_eq!(merges.len(), 1, "应恰好有一个合并提交");
    assert_eq!(merges[0].summary, "Merge branch 'feature'");
    assert_eq!(merges[0].parents.len(), 2, "合并提交应有两个父提交");

    // 引用：至少 main 与 feature 出现在某些提交上
    let all_refs: Vec<String> = graph.commits.iter().flat_map(|c| c.refs.clone()).collect();
    assert!(
        all_refs.iter().any(|r| r == "main"),
        "应包含 main 引用：{all_refs:?}"
    );
    assert!(
        all_refs.iter().any(|r| r == "feature"),
        "应包含 feature 引用：{all_refs:?}"
    );

    // 布局：合并历史至少两条泳道
    assert!(
        graph.layout.lane_count >= 2,
        "合并历史应至少两条泳道，实际 {}",
        graph.layout.lane_count
    );
    assert_eq!(graph.layout.rows.len(), graph.commits.len());
}

#[test]
fn graph_complete_first_row_is_the_merge() {
    let dir = tempfile::tempdir().expect("临时目录");
    build_repo(dir.path());
    let graph = read_graph(dir.path(), 50).expect("读取");
    let first = &graph.commits[0];
    assert!(
        first.is_merge(),
        "最新提交应是合并提交，实际：{} {}",
        first.short_id(),
        first.summary
    );
    // 合并提交的两个父提交落在不同泳道
    let row = &graph.layout.rows[0];
    assert_eq!(row.parent_lanes.len(), 2);
    assert_ne!(row.parent_lanes[0], row.parent_lanes[1]);
}

#[test]
fn graph_respects_limit() {
    let dir = tempfile::tempdir().expect("临时目录");
    build_repo(dir.path());
    let graph = read_graph(dir.path(), 2).expect("读取");
    assert_eq!(graph.commits.len(), 2, "应被 -n 限制为 2 条");
}

#[test]
fn status_reports_branch_and_clean_tree() {
    let dir = tempfile::tempdir().expect("临时目录");
    build_repo(dir.path());

    let status = read_status(dir.path()).expect("应能读取状态");
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(!status.is_dirty(), "刚提交完应是干净的：{:?}", status.files);
    // 本地无上游，ahead/behind 均为 0
    assert_eq!((status.ahead, status.behind), (0, 0));
}

#[test]
fn status_detects_modified_and_untracked() {
    let dir = tempfile::tempdir().expect("临时目录");
    build_repo(dir.path());

    write(dir.path(), "a.txt", "changed\n");
    write(dir.path(), "untracked.txt", "new file\n");

    let status = read_status(dir.path()).expect("读取状态");
    assert!(status.is_dirty());

    let modified = status
        .files
        .iter()
        .find(|f| f.path == "a.txt")
        .expect("应包含 a.txt");
    assert_eq!(modified.status, ChangeStatus::Modified);

    let untracked = status
        .files
        .iter()
        .find(|f| f.path == "untracked.txt")
        .expect("应包含未跟踪文件");
    assert_eq!(untracked.status, ChangeStatus::Untracked);
}

#[test]
fn status_detects_rename_with_original_path() {
    let dir = tempfile::tempdir().expect("临时目录");
    build_repo(dir.path());

    git(dir.path(), &["mv", "c.txt", "renamed.txt"]);

    let status = read_status(dir.path()).expect("读取状态");
    let renamed = status
        .files
        .iter()
        .find(|f| f.status == ChangeStatus::Renamed)
        .expect("应识别出重命名");
    assert_eq!(renamed.path, "renamed.txt");
    assert_eq!(renamed.original_path.as_deref(), Some("c.txt"));
}

#[test]
fn status_detects_deleted_file() {
    let dir = tempfile::tempdir().expect("临时目录");
    build_repo(dir.path());

    std::fs::remove_file(dir.path().join("b.txt")).expect("删文件");

    let status = read_status(dir.path()).expect("读取状态");
    let deleted = status
        .files
        .iter()
        .find(|f| f.path == "b.txt")
        .expect("应包含被删除文件");
    assert_eq!(deleted.status, ChangeStatus::Deleted);
}

#[test]
fn status_on_non_repo_returns_error() {
    let dir = tempfile::tempdir().expect("临时目录");
    let result = read_status(dir.path());
    assert!(result.is_err(), "非仓库目录应返回错误");
}

#[test]
fn graph_on_non_repo_returns_error() {
    let dir = tempfile::tempdir().expect("临时目录");
    assert!(read_graph(dir.path(), 10).is_err());
}

#[test]
fn empty_repo_yields_empty_graph() {
    let dir = tempfile::tempdir().expect("临时目录");
    git(dir.path(), &["init", "-q", "-b", "main"]);

    // 没有任何提交时，git log --all 返回空输出，应得到空图而不是报错
    let graph = read_graph(dir.path(), 10).expect("空仓库应返回空图");
    assert!(graph.commits.is_empty(), "空仓库不应有提交");
    assert_eq!(graph.layout.lane_count, 1, "空图也应有一条泳道");
    assert!(graph.layout.rows.is_empty());
}
