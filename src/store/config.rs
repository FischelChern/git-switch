//! 配置文件读写。
//!
//! 位置：`<config_dir>/git-switch/config.json`
//! （macOS `~/Library/Application Support`、Linux `~/.config`、Windows `%APPDATA%`）
//!
//! 可用环境变量 `GIT_SWITCH_CONFIG_DIR` 覆盖整个目录，便于测试与便携模式。
//!
//! **写入必须原子**：先写同目录下的 `.tmp`，再 `rename` 覆盖。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::identity::Identity;
use crate::model::platform::Platform;
use crate::model::repository::Repository;

/// 配置文件名。
pub const CONFIG_FILE: &str = "config.json";
/// 应用子目录名。
pub const APP_DIR: &str = "git-switch";
/// 覆盖配置目录的环境变量。
pub const ENV_CONFIG_DIR: &str = "GIT_SWITCH_CONFIG_DIR";
/// 当前配置格式版本。
pub const CURRENT_VERSION: u32 = 1;

/// 配置读写错误。
#[derive(Debug, Error)]
pub enum ConfigError {
    /// 无法确定配置目录（`dirs` 返回空且未设置环境变量）。
    #[error("无法确定配置目录，请设置 {ENV_CONFIG_DIR}")]
    NoConfigDir,
    /// 读取失败。
    #[error("读取配置失败 {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// 解析失败。
    #[error("解析配置失败 {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// 写入失败。
    #[error("写入配置失败 {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// 序列化失败。
    #[error("序列化配置失败: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// 整个应用的配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// 格式版本，便于将来迁移。
    #[serde(default = "default_version")]
    pub version: u32,
    /// 平台列表。
    #[serde(default)]
    pub platforms: Vec<Platform>,
    /// 身份列表（跨平台共用一个池，用 `platform_id` 归属）。
    #[serde(default)]
    pub identities: Vec<Identity>,
    /// 项目列表。
    #[serde(default)]
    pub repositories: Vec<Repository>,
}

fn default_version() -> u32 {
    CURRENT_VERSION
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            platforms: Platform::presets(),
            identities: Vec::new(),
            repositories: Vec::new(),
        }
    }
}

impl Config {
    /// 从默认位置读取。文件不存在时返回包含预设平台的空配置（**不写盘**）。
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(&config_dir()?)
    }

    /// 从指定目录读取。
    pub fn load_from(dir: &Path) -> Result<Self, ConfigError> {
        let path = dir.join(CONFIG_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(&text).map_err(|source| ConfigError::Parse { path, source })
    }

    /// 写到默认位置。
    pub fn save(&self) -> Result<(), ConfigError> {
        self.save_to(&config_dir()?)
    }

    /// 写到指定目录（原子写）。
    pub fn save_to(&self, dir: &Path) -> Result<(), ConfigError> {
        let path = dir.join(CONFIG_FILE);
        let text = serde_json::to_string_pretty(self)?;
        atomic_write(&path, text.as_bytes()).map_err(|source| ConfigError::Write { path, source })
    }

    // ---- 查询辅助 ----

    /// 按 id 找平台。
    pub fn platform(&self, id: &str) -> Option<&Platform> {
        self.platforms.iter().find(|p| p.id == id)
    }

    /// 按 id 找身份。
    pub fn identity(&self, id: &str) -> Option<&Identity> {
        self.identities.iter().find(|i| i.id == id)
    }

    /// 按 id 找项目。
    pub fn repository(&self, id: &str) -> Option<&Repository> {
        self.repositories.iter().find(|r| r.id == id)
    }

    /// 某平台下的全部身份。
    pub fn identities_of(&self, platform_id: &str) -> Vec<&Identity> {
        self.identities
            .iter()
            .filter(|i| i.platform_id == platform_id)
            .collect()
    }

    /// 某平台下的全部项目。**与当前身份无关**，切换身份不会改变它。
    pub fn repositories_of(&self, platform_id: &str) -> Vec<&Repository> {
        self.repositories
            .iter()
            .filter(|r| r.platform_id == platform_id)
            .collect()
    }

    /// 引用了某身份的项目数量。
    pub fn repositories_using(&self, identity_id: &str) -> Vec<&Repository> {
        self.repositories
            .iter()
            .filter(|r| r.identity_id.as_deref() == Some(identity_id))
            .collect()
    }
}

/// 配置目录：环境变量优先，否则 `<config_dir>/git-switch`。
pub fn config_dir() -> Result<PathBuf, ConfigError> {
    if let Some(dir) = std::env::var_os(ENV_CONFIG_DIR) {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let base = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
    Ok(base.join(APP_DIR))
}

/// 配置文件的完整路径。
pub fn config_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join(CONFIG_FILE))
}

/// 原子写：写 `.tmp` 再 `rename`，避免中途崩溃留下半个文件。
fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| CONFIG_FILE.to_string());
    let tmp = path.with_file_name(format!("{file_name}.tmp"));
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::identity::AuthMode;

    fn sample_config() -> Config {
        let mut cfg = Config::default();
        cfg.identities.push(
            Identity::new(
                "gh-me",
                "github",
                "张鑫宇 · 个人",
                "张鑫宇",
                "429092941@qq.com",
                AuthMode::Ssh,
            )
            .with_ssh_key("~/.ssh/id_ed25519_github"),
        );
        let mut repo = Repository::new(
            "r1",
            "github",
            "/tmp/iot-gateway",
            "git@github.com:zhangxinyu/iot-gateway.git",
        );
        repo.set_identity("gh-me");
        cfg.repositories.push(repo);
        cfg
    }

    #[test]
    fn default_has_preset_platforms_and_no_entries() {
        let cfg = Config::default();
        assert_eq!(cfg.version, CURRENT_VERSION);
        assert!(!cfg.platforms.is_empty());
        assert!(cfg.identities.is_empty());
        assert!(cfg.repositories.is_empty());
    }

    #[test]
    fn missing_file_yields_default_without_creating_it() {
        let dir = tempfile::tempdir().expect("临时目录");
        let cfg = Config::load_from(dir.path()).expect("应返回默认配置");
        assert!(cfg.identities.is_empty());
        assert!(!dir.path().join(CONFIG_FILE).exists(), "读操作不应写盘");
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempfile::tempdir().expect("临时目录");
        let cfg = sample_config();
        cfg.save_to(dir.path()).expect("应能写入");
        let loaded = Config::load_from(dir.path()).expect("应能读回");
        assert_eq!(cfg, loaded);
    }

    #[test]
    fn save_is_atomic_and_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().expect("临时目录");
        sample_config().save_to(dir.path()).expect("应能写入");
        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .expect("可列目录")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件：{leftovers:?}");
    }

    #[test]
    fn empty_file_falls_back_to_default() {
        let dir = tempfile::tempdir().expect("临时目录");
        fs::write(dir.path().join(CONFIG_FILE), "   ").expect("写空文件");
        let cfg = Config::load_from(dir.path()).expect("应回落到默认");
        assert!(!cfg.platforms.is_empty());
    }

    #[test]
    fn broken_json_reports_parse_error() {
        let dir = tempfile::tempdir().expect("临时目录");
        fs::write(dir.path().join(CONFIG_FILE), "{ not json").expect("写坏文件");
        let err = Config::load_from(dir.path()).expect_err("应报错");
        assert!(matches!(err, ConfigError::Parse { .. }), "实际：{err}");
    }

    #[test]
    fn nested_directory_is_created_on_save() {
        let dir = tempfile::tempdir().expect("临时目录");
        let nested = dir.path().join("a/b/c");
        sample_config().save_to(&nested).expect("应自动建目录");
        assert!(nested.join(CONFIG_FILE).exists());
    }

    #[test]
    fn lookups_by_id_work() {
        let cfg = sample_config();
        assert_eq!(
            cfg.platform("github").map(|p| p.id.as_str()),
            Some("github")
        );
        assert!(cfg.platform("nope").is_none());
        assert_eq!(cfg.identity("gh-me").map(|i| i.id.as_str()), Some("gh-me"));
        assert_eq!(cfg.repository("r1").map(|r| r.id.as_str()), Some("r1"));
    }

    #[test]
    fn identities_and_repositories_by_platform() {
        let cfg = sample_config();
        assert_eq!(cfg.identities_of("github").len(), 1);
        assert_eq!(cfg.identities_of("gitee").len(), 0);
        assert_eq!(cfg.repositories_of("github").len(), 1);
        assert_eq!(cfg.repositories_of("gitee").len(), 0);
    }

    #[test]
    fn repositories_using_identity() {
        let cfg = sample_config();
        assert_eq!(cfg.repositories_using("gh-me").len(), 1);
        assert_eq!(cfg.repositories_using("other").len(), 0);
    }

    #[test]
    fn env_var_overrides_config_dir() {
        // 该测试会改动进程级环境变量，使用独立变量名避免影响其它测试
        let dir = tempfile::tempdir().expect("临时目录");
        // SAFETY: 单测内串行使用，且仅设置本进程的环境变量
        unsafe { std::env::set_var(ENV_CONFIG_DIR, dir.path()) };
        let got = config_dir().expect("应返回环境变量指定的目录");
        unsafe { std::env::remove_var(ENV_CONFIG_DIR) };
        assert_eq!(got, dir.path());
    }

    #[test]
    fn config_path_appends_file_name() {
        assert!(config_path().is_ok());
    }
}
