//! 解析 unified diff，输出左右 / 上下两种视图所需的行数据。

use std::path::Path;

use thiserror::Error;

use super::exec::{GitError, GitExecutor};

/// 一侧的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// 行号；新增侧对删除行为 `None`，删除侧对新增行为 `None`。
    pub line_no: Option<usize>,
    pub text: String,
}

/// 一个文件块的 diff。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// `a/` 侧路径。
    pub old_path: String,
    /// `b/` 侧路径。
    pub new_path: String,
    /// 是否二进制文件（无逐行内容）。
    pub binary: bool,
    /// 上下文 / 删除 / 新增三种行，按原始顺序。
    pub rows: Vec<DiffRow>,
    /// 新增行数。
    pub additions: usize,
    /// 删除行数。
    pub deletions: usize,
}

impl FileDiff {
    /// 展示用路径：改了名显示 `old → new`。
    pub fn display_path(&self) -> String {
        if self.old_path == self.new_path {
            self.new_path.clone()
        } else {
            format!("{} → {}", self.old_path, self.new_path)
        }
    }
}

/// diff 中的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    /// 未改动的上下文行。
    Context { old: DiffLine, new: DiffLine },
    /// 被删除的行。
    Removed(DiffLine),
    /// 新增的行。
    Added(DiffLine),
}

impl DiffRow {
    /// 是否为增删行（用于着色）。
    pub fn kind(&self) -> RowKind {
        match self {
            Self::Context { .. } => RowKind::Context,
            Self::Removed(_) => RowKind::Removed,
            Self::Added(_) => RowKind::Added,
        }
    }
}

/// 行种类，UI 用来选颜色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Context,
    Removed,
    Added,
}

/// 左右并排视图的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideRow {
    pub left: Option<DiffLine>,
    pub right: Option<DiffLine>,
}

/// 把逐行 diff 转成左右并排的行。
///
/// 规则：连续的删除与新增配对成同一行，多余的一侧留空。
pub fn to_side_by_side(rows: &[DiffRow]) -> Vec<SideRow> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        match &rows[i] {
            DiffRow::Context { old, new } => {
                out.push(SideRow {
                    left: Some(old.clone()),
                    right: Some(new.clone()),
                });
                i += 1;
            }
            DiffRow::Removed(_) | DiffRow::Added(_) => {
                // 收集连续的一段删除与新增
                let mut removed = Vec::new();
                let mut added = Vec::new();
                while i < rows.len() {
                    match &rows[i] {
                        DiffRow::Removed(l) => removed.push(l.clone()),
                        DiffRow::Added(l) => added.push(l.clone()),
                        DiffRow::Context { .. } => break,
                    }
                    i += 1;
                }
                let n = removed.len().max(added.len());
                for k in 0..n {
                    out.push(SideRow {
                        left: removed.get(k).cloned(),
                        right: added.get(k).cloned(),
                    });
                }
            }
        }
    }
    out
}

/// diff 读取错误。
#[derive(Debug, Error)]
pub enum DiffError {
    #[error(transparent)]
    Git(#[from] GitError),
}

/// 解析 `git diff` 的 unified 输出（`--no-color`）。
pub fn parse_unified(input: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cur: Option<FileDiff> = None;
    // 当前 hunk 的游标
    let mut old_no = 0usize;
    let mut new_no = 0usize;

    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(f) = cur.take() {
                files.push(f);
            }
            let (old_path, new_path) = split_git_paths(rest);
            cur = Some(FileDiff {
                old_path,
                new_path,
                binary: false,
                rows: Vec::new(),
                additions: 0,
                deletions: 0,
            });
            continue;
        }

        let Some(file) = cur.as_mut() else {
            continue;
        };

        if line.starts_with("new file mode") {
            // 新增文件：旧侧路径按惯例显示为 /dev/null 的语义，这里保留 b/ 侧名字
            if file.old_path.is_empty() {
                file.old_path = file.new_path.clone();
            }
            continue;
        }
        if line.starts_with("deleted file mode") {
            if file.new_path.is_empty() {
                file.new_path = file.old_path.clone();
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("rename from ") {
            file.old_path = rest.to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("rename to ") {
            file.new_path = rest.to_string();
            continue;
        }
        if line.starts_with("Binary files ") || line == "GIT binary patch" {
            file.binary = true;
            continue;
        }
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("index ") {
            continue;
        }

        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some((o, n)) = parse_hunk_header(rest) {
                old_no = o;
                new_no = n;
            }
            continue;
        }

        // hunk 内容
        if let Some(text) = line.strip_prefix('+') {
            file.rows.push(DiffRow::Added(DiffLine {
                line_no: Some(new_no),
                text: text.to_string(),
            }));
            file.additions += 1;
            new_no += 1;
        } else if let Some(text) = line.strip_prefix('-') {
            file.rows.push(DiffRow::Removed(DiffLine {
                line_no: Some(old_no),
                text: text.to_string(),
            }));
            file.deletions += 1;
            old_no += 1;
        } else if let Some(text) = line.strip_prefix(' ') {
            file.rows.push(DiffRow::Context {
                old: DiffLine {
                    line_no: Some(old_no),
                    text: text.to_string(),
                },
                new: DiffLine {
                    line_no: Some(new_no),
                    text: text.to_string(),
                },
            });
            old_no += 1;
            new_no += 1;
        }
        // 其它行（\ No newline at end of file 等）忽略
    }

    if let Some(f) = cur.take() {
        files.push(f);
    }
    files
}

/// 从 `diff --git a/x b/y` 里拆出两个路径。
fn split_git_paths(rest: &str) -> (String, String) {
    let rest = rest.trim();
    if let Some(idx) = rest.find(" b/") {
        let a = &rest[..idx];
        let b = &rest[idx + 1..];
        (strip_prefix_ab(a), strip_prefix_ab(b))
    } else {
        let p = strip_prefix_ab(rest);
        (p.clone(), p)
    }
}

fn strip_prefix_ab(s: &str) -> String {
    s.trim()
        .strip_prefix("a/")
        .or_else(|| s.trim().strip_prefix("b/"))
        .unwrap_or(s.trim())
        .to_string()
}

/// 解析 `@@ -1,5 +1,6 @@ title` → (旧起始, 新起始)。
fn parse_hunk_header(rest: &str) -> Option<(usize, usize)> {
    let mut it = rest.split_whitespace();
    let old = it.next()?.strip_prefix('-')?;
    let new = it.next()?.strip_prefix('+')?;
    let old_start = old.split(',').next()?.parse::<usize>().ok()?;
    let new_start = new.split(',').next()?.parse::<usize>().ok()?;
    Some((old_start, new_start))
}

/// 读取工作区相对 HEAD 的 diff。
pub fn read_worktree_diff(repo: &Path) -> Result<Vec<FileDiff>, DiffError> {
    let out = GitExecutor::new(repo).run(&["diff", "--no-color", "--no-ext-diff", "-M", "HEAD"])?;
    if !out.success {
        return Err(DiffError::Git(GitError::CommandFailed {
            command: out.command,
            stderr: out.stderr,
        }));
    }
    Ok(parse_unified(&out.stdout))
}

/// 读取单个文件的 diff。
pub fn read_file_diff(repo: &Path, path: &str) -> Result<Option<FileDiff>, DiffError> {
    // 未跟踪文件用 --no-index 与空文件比较
    let tracked = GitExecutor::new(repo).run(&["ls-files", "--error-unmatch", "--", path]);
    let is_tracked = matches!(&tracked, Ok(o) if o.success);

    if !is_tracked {
        let out = GitExecutor::new(repo).run(&[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-index",
            "--",
            "/dev/null",
            path,
        ])?;
        // --no-index 在有差异时返回 1，不算错误
        let files = parse_unified(&out.stdout);
        return Ok(files.into_iter().next());
    }

    let out = GitExecutor::new(repo).run(&[
        "diff",
        "--no-color",
        "--no-ext-diff",
        "-M",
        "HEAD",
        "--",
        path,
    ])?;
    if !out.success {
        return Err(DiffError::Git(GitError::CommandFailed {
            command: out.command,
            stderr: out.stderr,
        }));
    }
    Ok(parse_unified(&out.stdout).into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,4 @@ fn main
 line one
-old line
+new line
+extra line
 line three
";

    #[test]
    fn parses_single_file_with_counts() {
        let files = parse_unified(SAMPLE);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.old_path, "src/a.rs");
        assert_eq!(f.new_path, "src/a.rs");
        assert_eq!(f.additions, 2);
        assert_eq!(f.deletions, 1);
        assert!(!f.binary);
        assert_eq!(f.display_path(), "src/a.rs");
    }

    #[test]
    fn row_kinds_and_line_numbers() {
        let files = parse_unified(SAMPLE);
        let rows = &files[0].rows;
        assert_eq!(rows[0].kind(), RowKind::Context);
        assert_eq!(rows[1].kind(), RowKind::Removed);
        assert_eq!(rows[2].kind(), RowKind::Added);
        assert_eq!(rows[3].kind(), RowKind::Added);
        assert_eq!(rows[4].kind(), RowKind::Context);

        match &rows[0] {
            DiffRow::Context { old, new } => {
                assert_eq!(old.line_no, Some(1));
                assert_eq!(new.line_no, Some(1));
            }
            _ => panic!("第 0 行应是上下文"),
        }
        match &rows[2] {
            DiffRow::Added(l) => {
                assert_eq!(l.line_no, Some(2));
                assert_eq!(l.text, "new line");
            }
            _ => panic!("第 2 行应是新增"),
        }
    }

    #[test]
    fn side_by_side_pairs_removed_and_added() {
        let files = parse_unified(SAMPLE);
        let side = to_side_by_side(&files[0].rows);
        // 1 context + 2（1 删 + 2 增，配对成 2 行）+ 1 context
        assert_eq!(side.len(), 4);
        // 第一对：删 old line ↔ 增 new line
        assert_eq!(
            side[1].left.as_ref().map(|l| l.text.clone()),
            Some("old line".to_string())
        );
        assert_eq!(
            side[1].right.as_ref().map(|l| l.text.clone()),
            Some("new line".to_string())
        );
        // 第二对：左空，右有 extra line
        assert!(side[2].left.is_none());
        assert_eq!(
            side[2].right.as_ref().map(|l| l.text.clone()),
            Some("extra line".to_string())
        );
    }

    #[test]
    fn parses_multiple_files() {
        let input = format!(
            "{SAMPLE}{}",
            "\
diff --git a/b.txt b/b.txt
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-x
+y
"
        );
        let files = parse_unified(&input);
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].new_path, "b.txt");
    }

    #[test]
    fn detects_binary_file() {
        let input = "\
diff --git a/img.png b/img.png
index 111..222 100644
Binary files a/img.png and b/img.png differ
";
        let files = parse_unified(input);
        assert_eq!(files.len(), 1);
        assert!(files[0].binary);
    }

    #[test]
    fn detects_rename() {
        let input = "\
diff --git a/old.rs b/new.rs
similarity index 95%
rename from old.rs
rename to new.rs
";
        let files = parse_unified(input);
        assert_eq!(files[0].old_path, "old.rs");
        assert_eq!(files[0].new_path, "new.rs");
        assert_eq!(files[0].display_path(), "old.rs → new.rs");
    }

    #[test]
    fn parses_hunk_header_variants() {
        assert_eq!(parse_hunk_header("-1,3 +1,4 @@ fn main"), Some((1, 1)));
        assert_eq!(parse_hunk_header("-10 +20 @@"), Some((10, 20)));
        assert_eq!(parse_hunk_header("garbage"), None);
    }

    #[test]
    fn empty_input_yields_no_files() {
        assert!(parse_unified("").is_empty());
        assert!(parse_unified("\n\n").is_empty());
    }

    #[test]
    fn no_newline_marker_is_ignored() {
        let input = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-x
+y
\\ No newline at end of file
";
        let files = parse_unified(input);
        assert_eq!(files[0].additions, 1);
        assert_eq!(files[0].deletions, 1);
        assert_eq!(files[0].rows.len(), 2);
    }

    #[test]
    fn side_by_side_keeps_context_aligned() {
        let rows = vec![
            DiffRow::Context {
                old: DiffLine {
                    line_no: Some(1),
                    text: "a".into(),
                },
                new: DiffLine {
                    line_no: Some(1),
                    text: "a".into(),
                },
            },
            DiffRow::Added(DiffLine {
                line_no: Some(2),
                text: "b".into(),
            }),
        ];
        let side = to_side_by_side(&rows);
        assert_eq!(side.len(), 2);
        assert!(side[1].left.is_none());
        assert!(side[1].right.is_some());
    }
}
