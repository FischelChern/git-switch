//! 工作区状态：变更文件列表、当前分支、领先/落后提交数。

use std::path::Path;

use thiserror::Error;

use super::exec::{GitError, GitExecutor};

/// 单个文件的变更类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Conflicted,
    TypeChanged,
}

impl ChangeStatus {
    /// 单字母标识，用于 UI 展示。
    pub fn letter(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::Untracked => "U",
            Self::Conflicted => "!",
            Self::TypeChanged => "T",
        }
    }

    /// 中文说明。
    pub fn label(self) -> &'static str {
        match self {
            Self::Added => "新增",
            Self::Modified => "修改",
            Self::Deleted => "删除",
            Self::Renamed => "重命名",
            Self::Copied => "复制",
            Self::Untracked => "未跟踪",
            Self::Conflicted => "冲突",
            Self::TypeChanged => "类型变更",
        }
    }
}

/// 一个变更文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// 当前路径。
    pub path: String,
    /// 重命名 / 复制前的路径。
    pub original_path: Option<String>,
    pub status: ChangeStatus,
}

/// 工作区整体状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkStatus {
    /// 当前分支名；分离头指针时为 `None`。
    pub branch: Option<String>,
    /// 领先上游的提交数。
    pub ahead: u32,
    /// 落后上游的提交数。
    pub behind: u32,
    /// 变更文件。
    pub files: Vec<FileChange>,
}

impl WorkStatus {
    /// 是否有未提交改动。
    pub fn is_dirty(&self) -> bool {
        !self.files.is_empty()
    }
}

/// 状态读取错误。
#[derive(Debug, Error)]
pub enum StatusError {
    /// git 调用失败。
    #[error(transparent)]
    Git(#[from] GitError),
}

/// 解析 `git status --porcelain=v1 -z` 的输出。
///
/// 格式：每条 `XY <path>\0`；重命名与复制会紧跟一条旧路径。
pub fn parse_porcelain(input: &str) -> Vec<FileChange> {
    let mut out = Vec::new();
    let mut parts = input.split('\0').filter(|s| !s.is_empty());

    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let bytes = entry.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        // "XY path"：第 3 个字符是空格
        let path = entry[3..].to_string();
        let status = classify(x, y);

        let original_path = if matches!(status, ChangeStatus::Renamed | ChangeStatus::Copied) {
            parts.next().map(|s| s.to_string())
        } else {
            None
        };

        out.push(FileChange {
            path,
            original_path,
            status,
        });
    }
    out
}

/// 由 porcelain 的两个状态字符判断类型。
fn classify(x: char, y: char) -> ChangeStatus {
    use ChangeStatus::*;
    if x == '?' && y == '?' {
        return Untracked;
    }
    if x == '!' && y == '!' {
        return Untracked;
    }
    // 冲突：任一列为 U，或 AA / DD
    if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
        return Conflicted;
    }
    if x == 'R' || y == 'R' {
        return Renamed;
    }
    if x == 'C' || y == 'C' {
        return Copied;
    }
    if x == 'A' || y == 'A' {
        return Added;
    }
    if x == 'D' || y == 'D' {
        return Deleted;
    }
    if x == 'T' || y == 'T' {
        return TypeChanged;
    }
    Modified
}

/// 读取仓库当前状态。
pub fn read_status(repo: &Path) -> Result<WorkStatus, StatusError> {
    let ex = GitExecutor::new(repo);

    let out = ex.run(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    if !out.success {
        return Err(StatusError::Git(GitError::CommandFailed {
            command: out.command,
            stderr: out.stderr,
        }));
    }
    let files = parse_porcelain(&out.stdout);

    let branch = match ex.run(&["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(o) if o.success => {
            let name = o.stdout_trimmed().to_string();
            if name == "HEAD" { None } else { Some(name) }
        }
        _ => None,
    };

    let (behind, ahead) = ahead_behind(repo);

    Ok(WorkStatus {
        branch,
        ahead,
        behind,
        files,
    })
}

/// 读取领先/落后数。没有上游时返回 (0, 0)。
fn ahead_behind(repo: &Path) -> (u32, u32) {
    let out = match GitExecutor::new(repo).run(&[
        "rev-list",
        "--count",
        "--left-right",
        "@{upstream}...HEAD",
    ]) {
        Ok(o) if o.success => o,
        _ => return (0, 0),
    };
    let text = out.stdout_trimmed();
    let mut it = text.split_whitespace();
    let left = it.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let right = it.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    // left = 上游独有 = behind；right = 本地独有 = ahead
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_modified() {
        let changes = parse_porcelain(" M src/main.rs\0");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "src/main.rs");
        assert_eq!(changes[0].status, ChangeStatus::Modified);
        assert!(changes[0].original_path.is_none());
    }

    #[test]
    fn parses_staged_add_and_deleted() {
        let changes = parse_porcelain("A  new.rs\0 D old.rs\0");
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].status, ChangeStatus::Added);
        assert_eq!(changes[1].status, ChangeStatus::Deleted);
    }

    #[test]
    fn parses_untracked() {
        let changes = parse_porcelain("?? notes.tmp\0");
        assert_eq!(changes[0].status, ChangeStatus::Untracked);
        assert_eq!(changes[0].path, "notes.tmp");
    }

    #[test]
    fn parses_rename_with_original_path() {
        // -z 模式下重命名的旧路径是单独一条
        let changes = parse_porcelain("R  new.rs\0old.rs\0");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].status, ChangeStatus::Renamed);
        assert_eq!(changes[0].path, "new.rs");
        assert_eq!(changes[0].original_path.as_deref(), Some("old.rs"));
    }

    #[test]
    fn parses_conflict() {
        let changes = parse_porcelain("UU both.rs\0");
        assert_eq!(changes[0].status, ChangeStatus::Conflicted);
    }

    #[test]
    fn parses_both_deleted_conflict() {
        let changes = parse_porcelain("DD both.rs\0");
        assert_eq!(changes[0].status, ChangeStatus::Conflicted);
    }

    #[test]
    fn parses_path_with_spaces() {
        let changes = parse_porcelain(" M dir/a b c.rs\0");
        assert_eq!(changes[0].path, "dir/a b c.rs");
    }

    #[test]
    fn empty_output_yields_no_changes() {
        assert!(parse_porcelain("").is_empty());
        assert!(parse_porcelain("\0\0").is_empty());
    }

    #[test]
    fn multiple_entries_are_all_parsed() {
        let changes = parse_porcelain("M  a\0?? b\0D  c\0");
        assert_eq!(changes.len(), 3);
    }

    #[test]
    fn status_letters_are_distinct() {
        let letters: Vec<&str> = [
            ChangeStatus::Added,
            ChangeStatus::Modified,
            ChangeStatus::Deleted,
            ChangeStatus::Renamed,
            ChangeStatus::Copied,
            ChangeStatus::Untracked,
            ChangeStatus::Conflicted,
            ChangeStatus::TypeChanged,
        ]
        .iter()
        .map(|s| s.letter())
        .collect();
        let unique: std::collections::HashSet<&str> = letters.iter().copied().collect();
        assert_eq!(unique.len(), letters.len(), "标识应互不相同：{letters:?}");
    }

    #[test]
    fn dirty_detection() {
        assert!(!WorkStatus::default().is_dirty());
        let s = WorkStatus {
            files: vec![FileChange {
                path: "a".into(),
                original_path: None,
                status: ChangeStatus::Modified,
            }],
            ..Default::default()
        };
        assert!(s.is_dirty());
    }
}
