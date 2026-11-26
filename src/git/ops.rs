//! 提交与常规写操作（暂存、提交、拉取、推送等）。

use std::path::Path;

use thiserror::Error;

use super::exec::{GitError, GitExecutor};

/// 写操作错误。
#[derive(Debug, Error)]
pub enum OpError {
    /// git 调用失败。
    #[error(transparent)]
    Git(#[from] GitError),
    /// 输入不合法（例如空提交信息）。
    #[error("{0}")]
    Invalid(String),
}

/// 校验提交信息：不允许纯空白。
pub fn validate_commit_message(message: &str) -> Result<String, OpError> {
    let m = message.trim();
    if m.is_empty() {
        return Err(OpError::Invalid("提交信息不能为空".into()));
    }
    // 首行过长通常是误粘贴，给出提示而不是拒绝
    Ok(m.to_string())
}

/// 一次操作的结果：命令 + 输出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpResult {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
}

impl OpResult {
    /// 是否适合直接展示给用户（stdout 为空时退到 stderr）。
    pub fn display(&self) -> &str {
        if self.stdout.trim().is_empty() {
            self.stderr.trim()
        } else {
            self.stdout.trim()
        }
    }
}

fn run_ok(repo: &Path, args: &[&str]) -> Result<OpResult, OpError> {
    let out = GitExecutor::new(repo).run(args)?;
    if !out.success {
        // git 有些失败信息写在 stdout（例如 "nothing to commit"），
        // 所以 stderr 为空时要退到 stdout，不能吞掉真实原因。
        let detail = if !out.stderr.trim().is_empty() {
            out.stderr.trim().to_string()
        } else if !out.stdout.trim().is_empty() {
            out.stdout.trim().to_string()
        } else {
            format!("git {} 失败（退出码 {:?}）", args.join(" "), out.code)
        };
        return Err(OpError::Invalid(detail));
    }
    Ok(OpResult {
        command: out.command,
        stdout: out.stdout,
        stderr: out.stderr,
    })
}

/// 暂存全部改动。
pub fn stage_all(repo: &Path) -> Result<OpResult, OpError> {
    run_ok(repo, &["add", "-A"])
}

/// 暂存单个文件。
pub fn stage_file(repo: &Path, path: &str) -> Result<OpResult, OpError> {
    if path.trim().is_empty() {
        return Err(OpError::Invalid("文件路径为空".into()));
    }
    run_ok(repo, &["add", "--", path])
}

/// 提交。必须先校验信息非空。
pub fn commit(repo: &Path, message: &str, stage_tracked: bool) -> Result<OpResult, OpError> {
    let msg = validate_commit_message(message)?;
    if stage_tracked {
        run_ok(repo, &["add", "-u"])?;
    }
    run_ok(repo, &["commit", "-m", &msg])
}

/// 获取。
pub fn fetch(repo: &Path) -> Result<OpResult, OpError> {
    run_ok(repo, &["fetch", "--all", "--prune"])
}

/// 拉取（快进优先，保留本地提交时由 git 决定是否需要 merge）。
pub fn pull(repo: &Path) -> Result<OpResult, OpError> {
    run_ok(repo, &["pull", "--no-rebase"])
}

/// 推送当前分支。
pub fn push(repo: &Path) -> Result<OpResult, OpError> {
    run_ok(repo, &["push"])
}

/// 贮藏（含未跟踪）。
pub fn stash(repo: &Path) -> Result<OpResult, OpError> {
    run_ok(repo, &["stash", "push", "-u"])
}

/// 弹出最近一次贮藏。
pub fn stash_pop(repo: &Path) -> Result<OpResult, OpError> {
    run_ok(repo, &["stash", "pop"])
}

/// 合并指定分支。
pub fn merge(repo: &Path, branch: &str) -> Result<OpResult, OpError> {
    if branch.trim().is_empty() {
        return Err(OpError::Invalid("目标分支为空".into()));
    }
    run_ok(repo, &["merge", "--no-ff", branch.trim()])
}

/// 是否处于合并冲突状态。
pub fn has_conflicts(repo: &Path) -> bool {
    match GitExecutor::new(repo).run(&["diff", "--name-only", "--diff-filter=U"]) {
        Ok(o) if o.success => !o.stdout_trimmed().is_empty(),
        _ => false,
    }
}

/// 列出冲突文件。
pub fn conflicted_files(repo: &Path) -> Vec<String> {
    match GitExecutor::new(repo).run(&["diff", "--name-only", "--diff-filter=U"]) {
        Ok(o) if o.success => o.lines().into_iter().map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_message_is_rejected() {
        assert!(validate_commit_message("").is_err());
        assert!(validate_commit_message("   ").is_err());
        assert!(validate_commit_message("\n\t\n").is_err());
    }

    #[test]
    fn message_is_trimmed() {
        let m = validate_commit_message("  fix: bug  \n").expect("应通过");
        assert_eq!(m, "fix: bug");
    }

    #[test]
    fn multiline_message_is_kept() {
        let m = validate_commit_message("feat: x\n\nbody line").expect("应通过");
        assert!(m.contains("body line"));
    }

    #[test]
    fn display_prefers_stdout_then_stderr() {
        let a = OpResult {
            command: "git x".into(),
            stdout: "out".into(),
            stderr: "err".into(),
        };
        assert_eq!(a.display(), "out");
        let b = OpResult {
            command: "git x".into(),
            stdout: "  ".into(),
            stderr: " err ".into(),
        };
        assert_eq!(b.display(), "err");
    }

    #[test]
    fn empty_path_is_rejected() {
        let dir = tempfile::tempdir().expect("临时目录");
        assert!(stage_file(dir.path(), "  ").is_err());
    }

    #[test]
    fn empty_merge_target_is_rejected() {
        let dir = tempfile::tempdir().expect("临时目录");
        assert!(merge(dir.path(), " ").is_err());
    }
}
