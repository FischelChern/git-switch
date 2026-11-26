//! 克隆：远程地址校验、目标路径推导、执行 `git clone` 并落实身份。
//!
//! 不变量：
//! - 目标目录**必须不存在或为空**，绝不覆盖已有内容。
//! - 克隆成功后立即把身份写进新仓库的 `.git/config`，不需要用户再点一次切换。
//! - 失败时把 stderr 原样带出，不伪造成功。

use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

use super::config::apply_identity;
use super::exec::{GitError, GitExecutor};
use crate::model::identity::Identity;
use crate::model::remote::{RemoteParseError, RemoteUrl};
use crate::model::repository::EffectiveIdentity;

/// 克隆超时：仓库可能很大，给 30 分钟。
pub const CLONE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// 克隆失败原因。
#[derive(Debug, Error)]
pub enum CloneError {
    /// 远程地址无法解析。
    #[error("远程地址无法解析：{0}")]
    BadUrl(#[from] RemoteParseError),
    /// 目标目录已存在且非空。
    #[error("目标目录已存在且非空：{0}")]
    TargetNotEmpty(String),
    /// 目标目录没有父目录可写。
    #[error("无法确定克隆位置：{0}")]
    NoParent(String),
    /// 克隆成功但落身份失败（仓库已在磁盘上，不能当作整体失败当作没发生）。
    #[error("克隆成功，但写入身份失败：{0}\n仓库已存在于 {1}，可手动重试切换身份")]
    ApplyFailed(String, String),
    /// git clone 失败。
    #[error(transparent)]
    Git(#[from] GitError),
}

/// 一次克隆请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneRequest {
    /// 远程地址原文，如 `git@github.com:owner/repo.git`。
    pub url: String,
    /// 克隆到哪个**父目录**。
    pub parent_dir: PathBuf,
    /// 目录名；为空时按远程仓库名推导。
    pub dir_name: String,
}

impl CloneRequest {
    /// 新建请求。
    pub fn new(url: impl Into<String>, parent_dir: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into(),
            parent_dir: parent_dir.into(),
            dir_name: String::new(),
        }
    }

    /// 指定目录名。
    pub fn with_dir_name(mut self, name: impl Into<String>) -> Self {
        self.dir_name = name.into();
        self
    }

    /// 解析远程地址；本地路径没有 host，会被视为无法解析。
    pub fn parsed(&self) -> Result<RemoteUrl, CloneError> {
        Ok(RemoteUrl::parse(&self.url)?)
    }

    /// 实际目录名：用户填了就用用户的，否则按地址推导。
    ///
    /// 远程地址用仓库名；本地路径用最后一段目录名。
    pub fn effective_dir_name(&self) -> Result<String, CloneError> {
        let typed = self.dir_name.trim();
        if !typed.is_empty() {
            return Ok(typed.to_string());
        }
        if is_local_source(&self.url) {
            let last = self
                .url
                .trim()
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .find(|s| !s.is_empty())
                .unwrap_or("");
            return dir_name_from_repo(last);
        }
        let parsed = self.parsed()?;
        dir_name_from_repo(&parsed.repo)
    }

    /// 最终克隆路径。
    pub fn target_path(&self) -> Result<PathBuf, CloneError> {
        let name = self.effective_dir_name()?;
        Ok(self.parent_dir.join(name))
    }
}

/// 是否是本地文件系统路径（本地仓库克隆、测试用）。
///
/// 这类地址没有 host，[`RemoteUrl::parse`] 无法解析，需要单独处理。
pub fn is_local_source(url: &str) -> bool {
    let s = url.trim();
    s.starts_with('/')
        || s.starts_with("./")
        || s.starts_with("../")
        || s.starts_with("file://")
        || s.starts_with('~')
        // Windows 盘符
        || (s.len() > 2 && s.as_bytes()[1] == b':' && s.as_bytes()[0].is_ascii_alphabetic())
}

/// 由仓库名推导目录名：去掉 `.git`、空格换成 `-`、剔除路径分隔符。
///
/// 结果为空（例如仓库名只有 `.git`）时返回错误，由调用方给出提示。
pub fn dir_name_from_repo(repo: &str) -> Result<String, CloneError> {
    let trimmed = repo.trim().trim_end_matches(".git");
    let cleaned: String = trimmed
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c if c.is_whitespace() => '-',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_matches('-').to_string();
    if cleaned.is_empty() {
        return Err(CloneError::BadUrl(RemoteParseError::Unrecognized));
    }
    Ok(cleaned)
}

/// 克隆结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneOutcome {
    /// 新仓库的绝对路径。
    pub path: PathBuf,
    /// `git clone` 的原始回显（stderr 里是 `Cloning into ...` 之类的进度）。
    pub output: String,
}

/// 检查目标目录是否可写：不存在，或存在但为空。
fn ensure_target_free(target: &Path) -> Result<(), CloneError> {
    if !target.exists() {
        return Ok(());
    }
    if !target.is_dir() {
        return Err(CloneError::TargetNotEmpty(target.display().to_string()));
    }
    let mut entries = std::fs::read_dir(target)
        .map_err(|_| CloneError::TargetNotEmpty(target.display().to_string()))?;
    if entries.next().is_some() {
        return Err(CloneError::TargetNotEmpty(target.display().to_string()));
    }
    Ok(())
}

/// 执行克隆；给了身份就顺手写进新仓库。
///
/// `identity` 与 `effective` 必须同时给或同时不给。
/// 注意：`GitExecutor` 绑定的是父目录，这里只借它的 `-C` 与超时控制，
/// 不用 `run_ok`——因为 clone 的进度信息写在 stderr，非空 stderr 不代表失败。
pub fn clone_repo(
    req: &CloneRequest,
    identity: Option<(&Identity, &EffectiveIdentity)>,
) -> Result<CloneOutcome, CloneError> {
    let target = req.target_path()?;
    if req.parent_dir.as_os_str().is_empty() {
        return Err(CloneError::NoParent(target.display().to_string()));
    }
    ensure_target_free(&target)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| CloneError::NoParent(parent.display().to_string()))?;
    }

    let dir_name = req.effective_dir_name()?;

    let executor = GitExecutor::new(&req.parent_dir).with_timeout(CLONE_TIMEOUT);
    let out = executor.run(&["clone", "--", &req.url, &dir_name])?;

    if !out.success {
        // 失败时清理可能被 git 建了一半的空目录，避免下次重名
        let _ = std::fs::remove_dir(&target);
        return Err(CloneError::Git(GitError::CommandFailed {
            command: out.command,
            stderr: pick_message(&out.stdout, &out.stderr),
        }));
    }

    let output = pick_message(&out.stdout, &out.stderr);

    if let Some((identity, effective)) = identity {
        apply_identity(&target, identity, effective)
            .map_err(|e| CloneError::ApplyFailed(e.to_string(), target.display().to_string()))?;
    }

    Ok(CloneOutcome {
        path: target,
        output,
    })
}

/// 合并 stdout / stderr 作为回显文本；两者都空时给出通用文案。
///
/// clone 的进度在 stderr，因此不能只看 stdout。
fn pick_message(stdout: &str, stderr: &str) -> String {
    let mut parts = Vec::new();
    let s = stdout.trim();
    let e = stderr.trim();
    if !s.is_empty() {
        parts.push(s.to_string());
    }
    if !e.is_empty() {
        parts.push(e.to_string());
    }
    if parts.is_empty() {
        "克隆完成（git 未输出内容）".to_string()
    } else {
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident() -> Identity {
        Identity {
            id: "gh-me".into(),
            platform_id: "github".into(),
            label: "测试身份".into(),
            user_name: "张鑫宇".into(),
            user_email: "me@example.com".into(),
            auth: crate::model::identity::AuthMode::Ssh,
            ssh_key_path: None,
        }
    }

    fn effective() -> EffectiveIdentity {
        EffectiveIdentity {
            user_name: "张鑫宇".into(),
            user_email: "me@example.com".into(),
            name_source: crate::model::repository::ValueSource::Identity,
            email_source: crate::model::repository::ValueSource::Identity,
        }
    }

    #[test]
    fn dir_name_strips_git_suffix() {
        assert_eq!(dir_name_from_repo("iot-gateway").unwrap(), "iot-gateway");
        assert_eq!(
            dir_name_from_repo("iot-gateway.git").unwrap(),
            "iot-gateway"
        );
    }

    #[test]
    fn dir_name_replaces_separators() {
        assert_eq!(dir_name_from_repo("a/b\\c").unwrap(), "a-b-c");
        assert_eq!(dir_name_from_repo("my repo").unwrap(), "my-repo");
    }

    #[test]
    fn dir_name_rejects_empty() {
        assert!(dir_name_from_repo("").is_err());
        assert!(dir_name_from_repo(".git").is_err());
    }

    #[test]
    fn target_path_uses_remote_name_by_default() {
        let req = CloneRequest::new("git@github.com:owner/iot-gateway.git", "/tmp/work");
        assert_eq!(
            req.target_path().unwrap(),
            PathBuf::from("/tmp/work/iot-gateway")
        );
    }

    #[test]
    fn target_path_prefers_typed_name() {
        let req =
            CloneRequest::new("https://gitee.com/x/y.git", "/tmp/work").with_dir_name("自定义目录");
        assert_eq!(
            req.target_path().unwrap(),
            PathBuf::from("/tmp/work/自定义目录")
        );
    }

    #[test]
    fn local_source_detection() {
        assert!(is_local_source("/tmp/repo"));
        assert!(is_local_source("./repo"));
        assert!(is_local_source("../repo"));
        assert!(is_local_source("file:///tmp/repo"));
        assert!(is_local_source("C:/WorkSpace/repo"));
        assert!(!is_local_source("git@github.com:o/r.git"));
        assert!(!is_local_source("https://gitee.com/o/r.git"));
    }

    #[test]
    fn target_path_derives_from_local_path() {
        let req = CloneRequest::new("/tmp/src/my-repo.git", "/tmp/work");
        assert_eq!(
            req.target_path().unwrap(),
            PathBuf::from("/tmp/work/my-repo")
        );
    }

    #[test]
    fn bad_url_is_reported() {
        let req = CloneRequest::new("不是地址", "/tmp/work");
        assert!(matches!(req.parsed(), Err(CloneError::BadUrl(_))));
        assert!(req.target_path().is_err());
    }

    #[test]
    fn existing_non_empty_target_is_rejected() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("repo");
        std::fs::create_dir_all(&target).expect("建目录");
        std::fs::write(target.join("keep.txt"), "x").expect("写");
        let err = ensure_target_free(&target).expect_err("应拒绝");
        assert!(matches!(err, CloneError::TargetNotEmpty(_)), "实际：{err}");
        // 原有内容不能被破坏
        assert!(target.join("keep.txt").exists());
    }

    #[test]
    fn empty_existing_target_is_allowed() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("repo");
        std::fs::create_dir_all(&target).expect("建目录");
        assert!(ensure_target_free(&target).is_ok());
    }

    #[test]
    fn clone_and_apply_identity_writes_only_target_repo() {
        // 源仓库
        let src_root = tempfile::tempdir().expect("源目录");
        let src = src_root.path().join("src-repo");
        std::fs::create_dir_all(&src).expect("建目录");
        let g = GitExecutor::new(&src);
        g.run_ok(&["init", "-q", "-b", "main"]).expect("init");
        g.run_ok(&["config", "user.name", "初始"]).expect("config");
        g.run_ok(&["config", "user.email", "init@example.com"])
            .expect("config");
        std::fs::write(src.join("a.txt"), "hello\n").expect("写");
        g.run_ok(&["add", "a.txt"]).expect("add");
        g.run_ok(&["commit", "-q", "-m", "init"]).expect("commit");

        // 目标父目录
        let dst_root = tempfile::tempdir().expect("目标目录");
        let req = CloneRequest::new(src.display().to_string(), dst_root.path());
        let outcome = clone_repo(&req, Some((&ident(), &effective()))).expect("应能克隆");

        assert!(outcome.path.join("a.txt").exists(), "文件应存在");
        assert!(
            outcome.path.join(".git").is_dir(),
            "应是真实 git 仓库：{}",
            outcome.output
        );

        let local = super::super::config::read_local(&outcome.path).expect("读本地配置");
        assert_eq!(local.user_name.as_deref(), Some("张鑫宇"));
        assert_eq!(local.user_email.as_deref(), Some("me@example.com"));
    }

    #[test]
    fn clone_without_identity_leaves_config_alone() {
        let src_root = tempfile::tempdir().expect("源目录");
        let src = src_root.path().join("plain");
        std::fs::create_dir_all(&src).expect("建目录");
        let g = GitExecutor::new(&src);
        g.run_ok(&["init", "-q", "-b", "main"]).expect("init");
        std::fs::write(src.join("b.txt"), "x\n").expect("写");

        let dst_root = tempfile::tempdir().expect("目标目录");
        let req = CloneRequest::new(src.display().to_string(), dst_root.path());
        let outcome = clone_repo(&req, None).expect("应能克隆");
        let local = super::super::config::read_local(&outcome.path).expect("读本地配置");
        assert!(local.is_empty(), "不应写入身份：{local:?}");
    }

    #[test]
    fn clone_failure_reports_real_reason_and_cleans_up() {
        let dst_root = tempfile::tempdir().expect("目标目录");
        let missing = dst_root.path().join("does-not-exist");
        let req = CloneRequest::new(missing.display().to_string(), dst_root.path())
            .with_dir_name("target");
        let err = clone_repo(&req, None).expect_err("应失败");
        let msg = err.to_string();
        assert!(
            msg.contains("does-not-exist") || msg.contains("repository"),
            "错误信息应包含真实原因：{msg}"
        );
        assert!(
            !dst_root.path().join("target").exists(),
            "失败后不应留下半成品目录"
        );
    }
}
