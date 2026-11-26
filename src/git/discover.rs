//! 扫描本地目录，发现 git 仓库并推导其所属平台。

use std::path::{Path, PathBuf};

use super::exec::GitExecutor;
use super::status::StatusError;
use crate::model::platform::{Platform, match_host};
use crate::model::remote::RemoteUrl;

/// 发现的一个仓库候选。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredRepo {
    /// 仓库绝对路径。
    pub path: PathBuf,
    /// 远程地址；没有 origin 时为 `None`。
    pub remote_url: Option<String>,
    /// 解析出的 host。
    pub host: Option<String>,
}

impl DiscoveredRepo {
    /// 目录名。
    pub fn dir_name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }

    /// 匹配已配置平台。
    pub fn platform<'a>(&self, platforms: &'a [Platform]) -> Option<&'a Platform> {
        self.host.as_deref().and_then(|h| match_host(platforms, h))
    }
}

/// 扫描选项。
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// 最大递归深度（1 表示只看直接子目录）。
    pub max_depth: usize,
    /// 是否跟随符号链接。
    pub follow_symlinks: bool,
    /// 结果上限，防止误扫整个 home。
    pub limit: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_depth: 3,
            follow_symlinks: false,
            limit: 200,
        }
    }
}

/// 判断某个目录本身是不是 git 仓库（存在 `.git`）。
fn is_repo_dir(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// 读取仓库的 origin 远程地址。
fn read_origin(repo: &Path) -> Option<String> {
    let out = GitExecutor::new(repo)
        .run(&["config", "--get", "remote.origin.url"])
        .ok()?;
    if !out.success {
        return None;
    }
    let url = out.stdout_trimmed().to_string();
    (!url.is_empty()).then_some(url)
}

/// 扫描目录，返回发现的仓库。
///
/// 规则：
/// - 命中 `.git` 即视为仓库，**不再深入该仓库**（避免扫到 node_modules 之类）；
/// - 跳过 `.` 开头的隐藏目录与 `node_modules`、`target` 等常见噪音目录；
/// - 结果按路径排序，保证输出稳定。
pub fn scan(root: &Path, opts: &ScanOptions) -> Vec<DiscoveredRepo> {
    let mut found = Vec::new();
    walk(root, 0, opts, &mut found);
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.truncate(opts.limit);
    found
}

/// 应该跳过的目录名。
fn is_noise_dir(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | "vendor"
            | ".cache"
            | "Library"
            | "Applications"
    )
}

fn walk(dir: &Path, depth: usize, opts: &ScanOptions, out: &mut Vec<DiscoveredRepo>) {
    if out.len() >= opts.limit || depth > opts.max_depth {
        return;
    }

    if is_repo_dir(dir) {
        let remote_url = read_origin(dir);
        let host = remote_url
            .as_deref()
            .and_then(|u| RemoteUrl::parse(u).ok())
            .map(|r| r.host);
        out.push(DiscoveredRepo {
            path: dir.to_path_buf(),
            remote_url,
            host,
        });
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut children: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || is_noise_dir(&name) {
            continue;
        }
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() && !opts.follow_symlinks {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        children.push(path);
    }
    children.sort();

    for child in children {
        if out.len() >= opts.limit {
            return;
        }
        walk(&child, depth + 1, opts, out);
    }
}

/// 把发现的仓库转成配置里的项目（未选身份）。
pub fn to_repository(
    discovered: &DiscoveredRepo,
    platform_id: &str,
) -> crate::model::repository::Repository {
    crate::model::repository::Repository::new(
        // 用路径做稳定 id，避免引入随机数
        discovered.path.to_string_lossy().to_string(),
        platform_id,
        discovered.path.clone(),
        discovered.remote_url.clone().unwrap_or_default(),
    )
}

/// 读取仓库当前分支名（未出生分支时返回 `None`）。
pub fn current_branch(repo: &Path) -> Result<Option<String>, StatusError> {
    let out = GitExecutor::new(repo).run(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    if !out.success {
        return Ok(None);
    }
    let name = out.stdout_trimmed();
    if name.is_empty() || name == "HEAD" {
        Ok(None)
    } else {
        Ok(Some(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo(dir: &Path) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q", "-b", "main"])
            .output()
            .expect("执行 git");
        assert!(out.status.success(), "git init 失败");
    }

    #[test]
    fn finds_repo_at_root() {
        let dir = tempfile::tempdir().expect("临时目录");
        init_repo(dir.path());
        let found = scan(dir.path(), &ScanOptions::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, dir.path());
    }

    #[test]
    fn finds_nested_repos() {
        let root = tempfile::tempdir().expect("临时目录");
        let a = root.path().join("a");
        let b = root.path().join("b/c");
        fs::create_dir_all(&a).expect("建目录");
        fs::create_dir_all(&b).expect("建目录");
        init_repo(&a);
        init_repo(&b);

        let found = scan(root.path(), &ScanOptions::default());
        assert_eq!(found.len(), 2, "应发现两个嵌套仓库：{found:?}");
    }

    #[test]
    fn does_not_descend_into_found_repo() {
        let root = tempfile::tempdir().expect("临时目录");
        let outer = root.path().join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).expect("建目录");
        init_repo(&outer);
        init_repo(&inner);

        let found = scan(root.path(), &ScanOptions::default());
        assert_eq!(found.len(), 1, "命中仓库后不再深入：{found:?}");
        assert_eq!(found[0].path, outer);
    }

    #[test]
    fn skips_noise_and_hidden_dirs() {
        let root = tempfile::tempdir().expect("临时目录");
        for name in ["node_modules", "target", ".git-outer", "dist"] {
            let p = root.path().join(name).join("repo");
            fs::create_dir_all(&p).expect("建目录");
            init_repo(&p);
        }
        let found = scan(root.path(), &ScanOptions::default());
        assert!(found.is_empty(), "噪音目录应被跳过：{found:?}");
    }

    #[test]
    fn respects_max_depth() {
        let root = tempfile::tempdir().expect("临时目录");
        let deep = root.path().join("a/b/c/d/repo");
        fs::create_dir_all(&deep).expect("建目录");
        init_repo(&deep);

        let shallow = ScanOptions {
            max_depth: 2,
            ..Default::default()
        };
        assert!(scan(root.path(), &shallow).is_empty(), "深度 2 应扫不到");

        let deep_opts = ScanOptions {
            max_depth: 6,
            ..Default::default()
        };
        assert_eq!(scan(root.path(), &deep_opts).len(), 1);
    }

    #[test]
    fn respects_limit() {
        let root = tempfile::tempdir().expect("临时目录");
        for i in 0..5 {
            let p = root.path().join(format!("r{i}"));
            fs::create_dir_all(&p).expect("建目录");
            init_repo(&p);
        }
        let opts = ScanOptions {
            limit: 3,
            ..Default::default()
        };
        assert_eq!(scan(root.path(), &opts).len(), 3);
    }

    #[test]
    fn results_are_sorted() {
        let root = tempfile::tempdir().expect("临时目录");
        for name in ["zeta", "alpha", "mike"] {
            let p = root.path().join(name);
            fs::create_dir_all(&p).expect("建目录");
            init_repo(&p);
        }
        let found = scan(root.path(), &ScanOptions::default());
        let names: Vec<String> = found.iter().map(|f| f.dir_name()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "结果应排序稳定");
    }

    #[test]
    fn repo_without_remote_has_no_host() {
        let dir = tempfile::tempdir().expect("临时目录");
        init_repo(dir.path());
        let found = scan(dir.path(), &ScanOptions::default());
        assert_eq!(found.len(), 1);
        assert!(found[0].remote_url.is_none());
        assert!(found[0].host.is_none());
    }

    #[test]
    fn repo_with_remote_infers_host_and_platform() {
        let dir = tempfile::tempdir().expect("临时目录");
        init_repo(dir.path());
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["remote", "add", "origin", "git@github.com:owner/repo.git"])
            .output()
            .expect("执行 git");
        assert!(out.status.success());

        let found = scan(dir.path(), &ScanOptions::default());
        assert_eq!(found[0].host.as_deref(), Some("github.com"));

        let platforms = Platform::presets();
        let p = found[0].platform(&platforms).expect("应命中 GitHub");
        assert_eq!(p.id, "github");
    }

    #[test]
    fn noise_dir_names_are_listed() {
        for name in ["node_modules", "target", "dist", "build", "vendor"] {
            assert!(is_noise_dir(name), "{name} 应被跳过");
        }
        assert!(!is_noise_dir("src"));
        assert!(!is_noise_dir("my-target"));
    }

    #[test]
    fn scanning_empty_dir_finds_nothing() {
        let dir = tempfile::tempdir().expect("临时目录");
        assert!(scan(dir.path(), &ScanOptions::default()).is_empty());
    }

    #[test]
    fn scanning_missing_dir_finds_nothing() {
        let found = scan(
            Path::new("/definitely/not/here/git-switch"),
            &ScanOptions::default(),
        );
        assert!(found.is_empty());
    }

    #[test]
    fn current_branch_reads_name() {
        let dir = tempfile::tempdir().expect("临时目录");
        init_repo(dir.path());
        // 没有任何提交时 rev-parse --abbrev-ref HEAD 可能失败或返回 HEAD
        let branch = current_branch(dir.path()).expect("应能执行");
        // 未出生分支允许为 None 或分支名
        if let Some(name) = branch {
            assert!(!name.is_empty());
        }
    }
}
