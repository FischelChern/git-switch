//! 文件读写、目录浏览与行内 blame。
//!
//! 只处理仓库内的相对路径；所有路径都会做一次越界检查，
//! **不允许**通过 `../` 逃出仓库根目录。

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use super::exec::{GitError, GitExecutor};

/// 单个文件的大小上限：超过则不读入内存（防止 UI 卡死）。
pub const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

/// 文件操作错误。
#[derive(Debug, Error)]
pub enum FileError {
    /// 路径不合法（绝对路径、含 `..`、为空）。
    #[error("非法路径：{0}")]
    InvalidPath(String),
    /// 文件不存在。
    #[error("文件不存在：{0}")]
    NotFound(String),
    /// 文件过大。
    #[error("文件超过 {limit} 字节上限：{path}")]
    TooLarge { path: String, limit: u64 },
    /// 不是合法 UTF-8 文本。
    #[error("文件不是 UTF-8 文本：{0}")]
    NotText(String),
    /// 读写失败。
    #[error("读写文件失败 {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// blame 解析失败。
    #[error("解析 blame 失败：{0}")]
    Blame(String),
    /// git 命令执行失败（例如仓库没有提交、文件未被跟踪）。
    #[error(transparent)]
    Git(#[from] GitError),
}

/// 校验并拼出仓库内的绝对路径。
///
/// 拒绝绝对路径、`..` 与空路径；`path` 允许用 `/` 分隔。
pub fn resolve_in_repo(repo: &Path, rel: &str) -> Result<PathBuf, FileError> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err(FileError::InvalidPath("路径为空".into()));
    }
    let candidate = Path::new(rel);
    if candidate.is_absolute() {
        return Err(FileError::InvalidPath(format!("不允许绝对路径：{rel}")));
    }
    for comp in candidate.components() {
        match comp {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => {
                return Err(FileError::InvalidPath(format!(
                    "路径不能包含 .. 或前缀：{rel}"
                )));
            }
        }
    }
    Ok(repo.join(candidate))
}

/// 读到的文件内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContent {
    /// 展示用相对路径。
    pub path: String,
    pub text: String,
    /// 行数（末行为空时不计）。
    pub line_count: usize,
    /// 文件字节数。
    pub size: u64,
}

impl FileContent {
    /// 按行切分，供编辑器渲染。
    pub fn lines(&self) -> Vec<&str> {
        self.text.lines().collect()
    }
}

/// 读取仓库内文本文件。
pub fn read_file(repo: &Path, rel: &str) -> Result<FileContent, FileError> {
    let abs = resolve_in_repo(repo, rel)?;
    let meta = std::fs::metadata(&abs).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            FileError::NotFound(rel.to_string())
        } else {
            FileError::Io {
                path: rel.to_string(),
                source: e,
            }
        }
    })?;
    if meta.is_dir() {
        return Err(FileError::InvalidPath(format!("{rel} 是目录")));
    }
    if meta.len() > MAX_READ_BYTES {
        return Err(FileError::TooLarge {
            path: rel.to_string(),
            limit: MAX_READ_BYTES,
        });
    }
    let bytes = std::fs::read(&abs).map_err(|e| FileError::Io {
        path: rel.to_string(),
        source: e,
    })?;
    let text = String::from_utf8(bytes).map_err(|_| FileError::NotText(rel.to_string()))?;
    let line_count = text.lines().count();
    Ok(FileContent {
        path: rel.to_string(),
        text,
        line_count,
        size: meta.len(),
    })
}

/// 写入仓库内文本文件（保留原换行风格由调用方负责）。
pub fn write_file(repo: &Path, rel: &str, text: &str) -> Result<(), FileError> {
    let abs = resolve_in_repo(repo, rel)?;
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| FileError::Io {
            path: rel.to_string(),
            source: e,
        })?;
    }
    std::fs::write(&abs, text).map_err(|e| FileError::Io {
        path: rel.to_string(),
        source: e,
    })
}

/// 目录树里的一个条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// 相对仓库根的路径。
    pub path: String,
    /// 文件名。
    pub name: String,
    /// 嵌套深度，0 为根层。
    pub depth: usize,
    pub is_dir: bool,
}

/// 列出仓库内某个目录的直接子项；`rel` 为空表示根目录。
///
/// 跳过 `.git`。目录排在前、同类按名称排序，保证输出稳定。
pub fn list_dir(repo: &Path, rel: &str) -> Result<Vec<TreeEntry>, FileError> {
    let (abs, depth_base) = if rel.trim().is_empty() {
        (repo.to_path_buf(), 0)
    } else {
        let abs = resolve_in_repo(repo, rel)?;
        let depth = rel.split('/').filter(|s| !s.is_empty()).count();
        (abs, depth)
    };

    let entries = std::fs::read_dir(&abs).map_err(|e| FileError::Io {
        path: rel.to_string(),
        source: e,
    })?;

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let is_dir = entry.path().is_dir();
        let path = if rel.trim().is_empty() {
            name.clone()
        } else {
            format!("{}/{}", rel.trim_end_matches('/'), name)
        };
        out.push(TreeEntry {
            path,
            name,
            depth: depth_base,
            is_dir,
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// 一行 blame 信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    /// 提交短 id。
    pub commit: String,
    pub author: String,
    /// 提交日期（`YYYY-MM-DD`）。
    pub date: String,
    /// 该行在文件中的行号（从 1 开始）。
    pub line_no: usize,
}

/// 解析 `git blame --line-porcelain` 的输出。
///
/// 每个「块」以 `<40 位 sha> <原行号> <结果行号> <行数>` 开头，
/// 随后是若干 `key value` 行，最后一行以 `\t` 开头即源文本。
pub fn parse_blame(input: &str) -> Vec<BlameLine> {
    let mut out = Vec::new();
    let mut cur_sha: Option<String> = None;
    let mut cur_line: Option<usize> = None;
    let mut cur_author = String::new();
    let mut cur_date = String::new();

    for line in input.lines() {
        if cur_sha.is_none() {
            // 头部行：sha 原行 结果行 [行数]
            let mut it = line.split_whitespace();
            let Some(sha) = it.next() else { continue };
            if sha.len() < 7 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let result_line = it.nth(1).and_then(|s| s.parse::<usize>().ok());
            if result_line.is_none() {
                continue;
            }
            cur_sha = Some(sha.to_string());
            cur_line = result_line;
            cur_author.clear();
            cur_date.clear();
            continue;
        }

        if let Some(rest) = line.strip_prefix("author ") {
            cur_author = rest.to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("author-time ") {
            // 只保留时间戳，格式化交给调用方；这里直接存原值
            let _ = rest;
            continue;
        }
        if let Some(rest) = line.strip_prefix("committer-time ") {
            if cur_date.is_empty() {
                cur_date = format_unixtime(rest);
            }
            continue;
        }
        if line.starts_with('\t') {
            if let (Some(sha), Some(line_no)) = (cur_sha.take(), cur_line) {
                out.push(BlameLine {
                    commit: sha.chars().take(7).collect(),
                    author: std::mem::take(&mut cur_author),
                    date: std::mem::take(&mut cur_date),
                    line_no,
                });
            }
        }
    }
    out
}

/// 把 unix 时间戳格式化成 `YYYY-MM-DD`；失败则返回空串。
///
/// 自己实现 UTC 换算，避免为一个日期引入额外依赖。
fn format_unixtime(raw: &str) -> String {
    let Ok(ts) = raw.trim().parse::<i64>() else {
        return String::new();
    };
    if ts < 0 {
        return String::new();
    }
    let days = ts / 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// 由「1970-01-01 起的天数」算出年月日（Howard Hinnant 算法）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 读取某文件的逐行 blame。
pub fn blame(repo: &Path, rel: &str) -> Result<Vec<BlameLine>, FileError> {
    resolve_in_repo(repo, rel)?;
    let out = GitExecutor::new(repo).run(&["blame", "--line-porcelain", "--", rel])?;
    if !out.success {
        return Err(FileError::Blame(out.stderr.trim().to_string()));
    }
    Ok(parse_blame(&out.stdout))
}

/// README 候选文件名，按优先级排序。
pub const README_CANDIDATES: [&str; 6] = [
    "README.md",
    "README.markdown",
    "README.rst",
    "README.txt",
    "README",
    "readme.md",
];

/// 在仓库根目录与常见子目录里找 README，返回相对路径。
pub fn find_readme(repo: &Path) -> Option<String> {
    let mut dirs = vec![String::new()];
    dirs.push("docs".to_string());
    dirs.push("doc".to_string());
    dirs.push(".github".to_string());

    for dir in dirs {
        for name in README_CANDIDATES {
            let rel = if dir.is_empty() {
                name.to_string()
            } else {
                format!("{dir}/{name}")
            };
            let abs = repo.join(&rel);
            if abs.is_file() {
                return Some(rel);
            }
        }
    }
    None
}

/// 读取仓库 README；没有则返回 `None`。
pub fn read_readme(repo: &Path) -> Option<FileContent> {
    let rel = find_readme(repo)?;
    read_file(repo, &rel).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("临时目录");
        std::fs::create_dir_all(dir.path().join("src")).expect("建目录");
        std::fs::write(dir.path().join("README.md"), "# Hi\n\n内容\n").expect("写");
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").expect("写");
        dir
    }

    #[test]
    fn resolve_rejects_absolute_and_parent() {
        let repo = Path::new("/tmp/repo");
        assert!(resolve_in_repo(repo, "/etc/passwd").is_err());
        assert!(resolve_in_repo(repo, "../outside").is_err());
        assert!(resolve_in_repo(repo, "a/../../b").is_err());
        assert!(resolve_in_repo(repo, "").is_err());
        assert!(resolve_in_repo(repo, "a/b.txt").is_ok());
        assert!(resolve_in_repo(repo, "./a.txt").is_ok());
    }

    #[test]
    fn read_and_write_roundtrip() {
        let dir = setup();
        let content = read_file(dir.path(), "README.md").expect("应能读");
        assert_eq!(content.line_count, 3);
        assert!(content.text.contains("# Hi"));

        write_file(dir.path(), "notes/new.txt", "hello\nworld\n").expect("应能写");
        let again = read_file(dir.path(), "notes/new.txt").expect("应能读回");
        assert_eq!(again.text, "hello\nworld\n");
    }

    #[test]
    fn read_missing_file_reports_not_found() {
        let dir = setup();
        let err = read_file(dir.path(), "nope.txt").expect_err("应报错");
        assert!(matches!(err, FileError::NotFound(_)), "实际：{err}");
    }

    #[test]
    fn read_directory_is_rejected() {
        let dir = setup();
        let err = read_file(dir.path(), "src").expect_err("应报错");
        assert!(matches!(err, FileError::InvalidPath(_)));
    }

    #[test]
    fn non_utf8_is_rejected() {
        let dir = tempfile::tempdir().expect("临时目录");
        std::fs::write(dir.path().join("bin.dat"), [0xff, 0xfe, 0x00]).expect("写");
        let err = read_file(dir.path(), "bin.dat").expect_err("应报错");
        assert!(matches!(err, FileError::NotText(_)), "实际：{err}");
    }

    #[test]
    fn list_dir_skips_git_and_sorts_dirs_first() {
        let dir = setup();
        std::fs::create_dir_all(dir.path().join(".git")).expect("建目录");
        let entries = list_dir(dir.path(), "").expect("应能列出");
        assert!(!entries.iter().any(|e| e.name == ".git"), "不应列出 .git");
        assert!(entries[0].is_dir, "目录应排在前：{entries:?}");
        assert_eq!(entries[0].name, "src");
    }

    #[test]
    fn list_dir_supports_nested() {
        let dir = setup();
        let entries = list_dir(dir.path(), "src").expect("应能列出");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "src/main.rs");
        assert_eq!(entries[0].depth, 1);
    }

    #[test]
    fn find_readme_prefers_root() {
        let dir = setup();
        assert_eq!(find_readme(dir.path()).as_deref(), Some("README.md"));
    }

    #[test]
    fn find_readme_falls_back_to_docs() {
        let dir = tempfile::tempdir().expect("临时目录");
        std::fs::create_dir_all(dir.path().join("docs")).expect("建");
        std::fs::write(dir.path().join("docs/README.md"), "# docs\n").expect("写");
        assert_eq!(find_readme(dir.path()).as_deref(), Some("docs/README.md"));
    }

    #[test]
    fn find_readme_returns_none_when_absent() {
        let dir = tempfile::tempdir().expect("临时目录");
        assert!(find_readme(dir.path()).is_none());
    }

    #[test]
    fn read_readme_returns_content() {
        let dir = setup();
        let readme = read_readme(dir.path()).expect("应有 README");
        assert!(readme.text.contains("内容"));
    }

    #[test]
    fn parses_blame_blocks() {
        let sha = "1".repeat(40);
        let sha2 = "2".repeat(40);
        let input = format!(
            "{sha} 1 1 1\nauthor 张鑫宇\nauthor-time 1700000000\ncommitter-time 1700000000\nsummary first\n\tline one\n{sha2} 2 2 1\nauthor 王工\nauthor-time 1700086400\ncommitter-time 1700086400\nsummary second\n\tline two\n"
        );
        let lines = parse_blame(&input);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].commit, "1111111");
        assert_eq!(lines[0].author, "张鑫宇");
        assert_eq!(lines[0].line_no, 1);
        assert!(
            lines[0].date.starts_with("2023-"),
            "日期：{}",
            lines[0].date
        );
        assert_eq!(lines[1].author, "王工");
        assert_eq!(lines[1].line_no, 2);
    }

    #[test]
    fn blame_ignores_garbage() {
        assert!(parse_blame("").is_empty());
        assert!(parse_blame("not a blame").is_empty());
        assert!(parse_blame("zzzz 1 1 1\n\ttext\n").is_empty());
    }

    #[test]
    fn unix_time_formatting() {
        // 1970-01-01
        assert_eq!(format_unixtime("0"), "1970-01-01");
        // 2000-01-01 = 946684800
        assert_eq!(format_unixtime("946684800"), "2000-01-01");
        // 2024-02-29 = 1709164800（闰日）
        assert_eq!(format_unixtime("1709164800"), "2024-02-29");
        assert_eq!(format_unixtime("bad"), "");
        assert_eq!(format_unixtime("-1"), "");
    }
}
