//! 分支与标签的读写。

use std::path::Path;

use thiserror::Error;

use super::exec::{GitError, GitExecutor};

/// 一个分支。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// 分支名（本地不含 `refs/heads/`，远程以 `origin/` 开头）。
    pub name: String,
    /// 是否当前检出。
    pub current: bool,
    /// 是否远程分支。
    pub remote: bool,
}

/// 分支操作错误。
#[derive(Debug, Error)]
pub enum BranchError {
    /// git 调用失败。
    #[error(transparent)]
    Git(#[from] GitError),
    /// 分支名非法或已存在。
    #[error("{0}")]
    Invalid(String),
}

/// 校验分支名：git 自身规则里最容易踩的几条。
pub fn validate_branch_name(name: &str) -> Result<(), BranchError> {
    let n = name.trim();
    if n.is_empty() {
        return Err(BranchError::Invalid("分支名不能为空".into()));
    }
    if n.starts_with('-') || n.starts_with('/') || n.ends_with('/') {
        return Err(BranchError::Invalid(
            "分支名不能以 - 或 / 开头，也不能以 / 结尾".into(),
        ));
    }
    if n.ends_with(".lock") {
        return Err(BranchError::Invalid("分支名不能以 .lock 结尾".into()));
    }
    if n.contains("..")
        || n.contains(' ')
        || n.contains('\t')
        || n.contains('~')
        || n.contains('^')
        || n.contains(':')
        || n.contains('?')
        || n.contains('*')
        || n.contains('[')
        || n.contains('\\')
    {
        return Err(BranchError::Invalid(
            "分支名不能包含空格、..、~、^、:、?、*、[ 或反斜杠".into(),
        ));
    }
    if n.contains("//") || n.contains("@{") {
        return Err(BranchError::Invalid("分支名格式非法".into()));
    }
    Ok(())
}

/// 解析 `git branch -a --format=%(refname:short)%09%(HEAD)` 的输出。
pub fn parse_branches(input: &str) -> Vec<Branch> {
    let mut out = Vec::new();
    for line in input.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let Some(name) = parts.next().map(str::trim) else {
            continue;
        };
        if name.is_empty() || name == "HEAD" || name.ends_with("/HEAD") {
            continue;
        }
        let head = parts.next().unwrap_or("").trim();
        let current = head == "*";
        // `git branch -a` 的远程分支形如 remotes/origin/main
        let (name, remote) = if let Some(rest) = name.strip_prefix("remotes/") {
            (rest.to_string(), true)
        } else {
            (name.to_string(), false)
        };
        out.push(Branch {
            name,
            current,
            remote,
        });
    }
    out
}

/// 列出全部分支（本地 + 远程）。
pub fn list_branches(repo: &Path) -> Result<Vec<Branch>, BranchError> {
    let out =
        GitExecutor::new(repo).run(&["branch", "-a", "--format=%(refname:short)\t%(HEAD)"])?;
    if !out.success {
        return Err(BranchError::Git(GitError::CommandFailed {
            command: out.command,
            stderr: out.stderr,
        }));
    }
    Ok(parse_branches(&out.stdout))
}

/// 新建分支。
///
/// `checkout` 为真时新建并立即检出。
pub fn create_branch(
    repo: &Path,
    name: &str,
    start_point: Option<&str>,
    checkout: bool,
) -> Result<(), BranchError> {
    validate_branch_name(name)?;
    let name = name.trim();

    let mut args: Vec<&str> = vec![if checkout { "checkout" } else { "branch" }];
    if checkout {
        args.push("-b");
    }
    args.push(name);
    if let Some(sp) = start_point {
        let sp = sp.trim();
        if !sp.is_empty() {
            args.push(sp);
        }
    }

    let out = GitExecutor::new(repo).run(&args)?;
    if !out.success {
        return Err(BranchError::Invalid(out.stderr.trim().to_string()));
    }
    Ok(())
}

/// 删除分支。`force` 对应 `-D`。
pub fn delete_branch(repo: &Path, name: &str, force: bool) -> Result<(), BranchError> {
    validate_branch_name(name)?;
    let out =
        GitExecutor::new(repo).run(&["branch", if force { "-D" } else { "-d" }, name.trim()])?;
    if !out.success {
        return Err(BranchError::Invalid(out.stderr.trim().to_string()));
    }
    Ok(())
}

/// 重命名分支。
pub fn rename_branch(repo: &Path, old: &str, new: &str) -> Result<(), BranchError> {
    validate_branch_name(old)?;
    validate_branch_name(new)?;
    let out = GitExecutor::new(repo).run(&["branch", "-m", old.trim(), new.trim()])?;
    if !out.success {
        return Err(BranchError::Invalid(out.stderr.trim().to_string()));
    }
    Ok(())
}

/// 检出分支。
pub fn checkout_branch(repo: &Path, name: &str) -> Result<(), BranchError> {
    validate_branch_name(name)?;
    let out = GitExecutor::new(repo).run(&["checkout", name.trim()])?;
    if !out.success {
        return Err(BranchError::Invalid(out.stderr.trim().to_string()));
    }
    Ok(())
}

/// 列出标签。
pub fn list_tags(repo: &Path) -> Result<Vec<String>, BranchError> {
    let out = GitExecutor::new(repo).run(&["tag", "--list", "--sort=-creatordate"])?;
    if !out.success {
        return Err(BranchError::Git(GitError::CommandFailed {
            command: out.command,
            stderr: out.stderr,
        }));
    }
    Ok(out
        .lines()
        .into_iter()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .collect())
}

/// 创建标签。
pub fn create_tag(repo: &Path, name: &str, message: Option<&str>) -> Result<(), BranchError> {
    validate_branch_name(name)?;
    let name = name.trim();
    let out = match message.map(str::trim).filter(|m| !m.is_empty()) {
        Some(msg) => GitExecutor::new(repo).run(&["tag", "-a", name, "-m", msg])?,
        None => GitExecutor::new(repo).run(&["tag", name])?,
    };
    if !out.success {
        return Err(BranchError::Invalid(out.stderr.trim().to_string()));
    }
    Ok(())
}

/// 删除标签。
pub fn delete_tag(repo: &Path, name: &str) -> Result<(), BranchError> {
    validate_branch_name(name)?;
    let out = GitExecutor::new(repo).run(&["tag", "-d", name.trim()])?;
    if !out.success {
        return Err(BranchError::Invalid(out.stderr.trim().to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_and_remote_branches() {
        let input = "main\t*\nfeature\t\nremotes/origin/main\t\nremotes/origin/dev\t\n";
        let branches = parse_branches(input);
        assert_eq!(branches.len(), 4);
        assert_eq!(branches[0].name, "main");
        assert!(branches[0].current);
        assert!(!branches[0].remote);
        assert_eq!(branches[1].name, "feature");
        assert!(!branches[1].current);
        assert_eq!(branches[2].name, "origin/main");
        assert!(branches[2].remote);
    }

    #[test]
    fn ignores_head_pseudo_refs() {
        let input = "HEAD\t\nremotes/origin/HEAD\t\nmain\t*\n";
        let branches = parse_branches(input);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "main");
    }

    #[test]
    fn empty_input_yields_no_branches() {
        assert!(parse_branches("").is_empty());
        assert!(parse_branches("\n\n").is_empty());
    }

    #[test]
    fn valid_branch_names_accepted() {
        for name in ["main", "feat/x", "release/2.4", "fix-123", "a_b.c"] {
            assert!(validate_branch_name(name).is_ok(), "{name} 应合法");
        }
    }

    #[test]
    fn invalid_branch_names_rejected() {
        for name in [
            "",
            "  ",
            "-bad",
            "/bad",
            "bad/",
            "has space",
            "a..b",
            "a~b",
            "a^b",
            "a:b",
            "a?b",
            "a*b",
            "a[b",
            "a\\b",
            "a@{b",
            "x.lock",
        ] {
            assert!(validate_branch_name(name).is_err(), "{name:?} 应被拒绝");
        }
    }

    #[test]
    fn slash_in_middle_is_allowed() {
        assert!(validate_branch_name("feat/my-thing").is_ok());
    }
}
