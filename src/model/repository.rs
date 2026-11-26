//! 项目：磁盘上的一个 git 仓库。
//!
//! 项目属于**平台**，只持有 `identity_id` 引用当前使用的身份；
//! 项目可以**覆盖**身份的用户名/邮箱，优先级最高。
//!
//! 有效值优先级：`项目覆盖` > `身份默认值`。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::identity::Identity;
use super::remote::RemoteUrl;

/// 项目级姓名/邮箱覆盖。
///
/// `enabled == false` 时整块忽略；即使 `enabled == true`，
/// 空字符串也视为「不覆盖」，回落到身份默认值。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityOverride {
    pub enabled: bool,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub user_email: Option<String>,
}

impl IdentityOverride {
    /// 未启用覆盖。
    pub fn none() -> Self {
        Self::default()
    }

    /// 启用并指定值；`None` 表示该项不覆盖。
    pub fn enabled_with(
        user_name: Option<impl Into<String>>,
        user_email: Option<impl Into<String>>,
    ) -> Self {
        Self {
            enabled: true,
            user_name: user_name.map(Into::into),
            user_email: user_email.map(Into::into),
        }
    }

    /// 有效姓名覆盖值（过滤掉空白）。
    pub fn name_value(&self) -> Option<&str> {
        self.enabled
            .then_some(self.user_name.as_deref())
            .flatten()
            .filter(|s| !s.trim().is_empty())
    }

    /// 有效邮箱覆盖值（过滤掉空白）。
    pub fn email_value(&self) -> Option<&str> {
        self.enabled
            .then_some(self.user_email.as_deref())
            .flatten()
            .filter(|s| !s.trim().is_empty())
    }

    /// 是否有任意一项实际生效。
    pub fn is_active(&self) -> bool {
        self.name_value().is_some() || self.email_value().is_some()
    }
}

/// 某个字段最终取自哪里，供 UI 展示来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    /// 来自项目覆盖。
    Override,
    /// 来自身份默认值。
    Identity,
}

impl ValueSource {
    /// 展示用名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::Override => "项目覆盖",
            Self::Identity => "身份默认",
        }
    }
}

/// 解析后的有效身份值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveIdentity {
    pub user_name: String,
    pub user_email: String,
    pub name_source: ValueSource,
    pub email_source: ValueSource,
}

/// 一个项目（磁盘上的 git 仓库）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    /// 稳定标识。
    pub id: String,
    /// 所属平台 id。**由远程地址推导，不由用户指定。**
    pub platform_id: String,
    /// 当前使用的身份 id；`None` 表示跟随全局配置。
    #[serde(default)]
    pub identity_id: Option<String>,
    /// 仓库绝对路径。
    pub path: PathBuf,
    /// 最近一次读取到的远程地址，用于展示与一致性校验。
    #[serde(default)]
    pub remote_url: String,
    /// 项目级覆盖。
    #[serde(default, rename = "override")]
    pub identity_override: IdentityOverride,
}

impl Repository {
    /// 新建项目。
    pub fn new(
        id: impl Into<String>,
        platform_id: impl Into<String>,
        path: impl Into<PathBuf>,
        remote_url: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            platform_id: platform_id.into(),
            identity_id: None,
            path: path.into(),
            remote_url: remote_url.into(),
            identity_override: IdentityOverride::none(),
        }
    }

    /// 目录名，用于列表展示。
    pub fn dir_name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }

    /// 解析当前远程地址。
    pub fn remote(&self) -> Option<RemoteUrl> {
        RemoteUrl::parse(&self.remote_url).ok()
    }

    /// 计算最终要写入 `.git/config` 的 `user.name` / `user.email`。
    pub fn effective_identity(&self, identity: &Identity) -> EffectiveIdentity {
        let (user_name, name_source) = match self.identity_override.name_value() {
            Some(v) => (v.to_string(), ValueSource::Override),
            None => (identity.user_name.clone(), ValueSource::Identity),
        };
        let (user_email, email_source) = match self.identity_override.email_value() {
            Some(v) => (v.to_string(), ValueSource::Override),
            None => (identity.user_email.clone(), ValueSource::Identity),
        };
        EffectiveIdentity {
            user_name,
            user_email,
            name_source,
            email_source,
        }
    }

    /// 设置当前身份。
    pub fn set_identity(&mut self, identity_id: impl Into<String>) {
        self.identity_id = Some(identity_id.into());
    }

    /// 远程地址与所属平台是否一致（用户在外部改过 remote 时检测用）。
    pub fn platform_matches_remote(&self, platform_host: &str) -> Option<bool> {
        self.remote()
            .map(|r| r.host == platform_host || r.host.ends_with(&format!(".{platform_host}")))
    }

    /// 路径是否可用（仅做字符串级判断，不触碰文件系统）。
    pub fn has_path(&self) -> bool {
        !self.path.as_os_str().is_empty()
    }

    /// 供 UI 展示的路径。
    pub fn display_path(&self) -> &Path {
        self.path.as_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::identity::AuthMode;

    fn identity() -> Identity {
        Identity::new(
            "gh-me",
            "github",
            "张鑫宇 · 个人",
            "张鑫宇",
            "429092941@qq.com",
            AuthMode::Ssh,
        )
    }

    fn repo() -> Repository {
        Repository::new(
            "r1",
            "github",
            "/tmp/iot-gateway",
            "git@github.com:zhangxinyu/iot-gateway.git",
        )
    }

    #[test]
    fn no_override_uses_identity_values() {
        let r = repo();
        let e = r.effective_identity(&identity());
        assert_eq!(e.user_name, "张鑫宇");
        assert_eq!(e.user_email, "429092941@qq.com");
        assert_eq!(e.name_source, ValueSource::Identity);
        assert_eq!(e.email_source, ValueSource::Identity);
    }

    #[test]
    fn override_both_fields() {
        let mut r = repo();
        r.identity_override = IdentityOverride::enabled_with(Some("阿张"), Some("zhang@corp.com"));
        let e = r.effective_identity(&identity());
        assert_eq!(e.user_name, "阿张");
        assert_eq!(e.user_email, "zhang@corp.com");
        assert_eq!(e.name_source, ValueSource::Override);
        assert_eq!(e.email_source, ValueSource::Override);
    }

    #[test]
    fn override_only_email_keeps_identity_name() {
        let mut r = repo();
        r.identity_override =
            IdentityOverride::enabled_with(None::<String>, Some("zhang@corp.com"));
        let e = r.effective_identity(&identity());
        assert_eq!(e.user_name, "张鑫宇");
        assert_eq!(e.user_email, "zhang@corp.com");
        assert_eq!(e.name_source, ValueSource::Identity);
        assert_eq!(e.email_source, ValueSource::Override);
    }

    #[test]
    fn override_only_name_keeps_identity_email() {
        let mut r = repo();
        r.identity_override = IdentityOverride::enabled_with(Some("阿张"), None::<String>);
        let e = r.effective_identity(&identity());
        assert_eq!(e.user_name, "阿张");
        assert_eq!(e.user_email, "429092941@qq.com");
        assert_eq!(e.name_source, ValueSource::Override);
        assert_eq!(e.email_source, ValueSource::Identity);
    }

    #[test]
    fn disabled_override_is_ignored() {
        let mut r = repo();
        r.identity_override = IdentityOverride {
            enabled: false,
            user_name: Some("阿张".into()),
            user_email: Some("zhang@corp.com".into()),
        };
        let e = r.effective_identity(&identity());
        assert_eq!(e.user_name, "张鑫宇");
        assert_eq!(e.user_email, "429092941@qq.com");
        assert_eq!(e.name_source, ValueSource::Identity);
    }

    #[test]
    fn blank_override_values_fall_back() {
        let mut r = repo();
        r.identity_override = IdentityOverride {
            enabled: true,
            user_name: Some("   ".into()),
            user_email: Some("".into()),
        };
        let e = r.effective_identity(&identity());
        assert_eq!(e.user_name, "张鑫宇");
        assert_eq!(e.user_email, "429092941@qq.com");
        assert!(!r.identity_override.is_active());
    }

    #[test]
    fn dir_name_and_path() {
        let r = repo();
        assert_eq!(r.dir_name(), "iot-gateway");
        assert!(r.has_path());
        assert_eq!(r.display_path(), Path::new("/tmp/iot-gateway"));
    }

    #[test]
    fn platform_matches_remote_detection() {
        let r = repo();
        assert_eq!(r.platform_matches_remote("github.com"), Some(true));
        assert_eq!(r.platform_matches_remote("gitlab.company.com"), Some(false));

        let broken = Repository::new("r2", "github", "/tmp/x", "not-a-url");
        assert_eq!(broken.platform_matches_remote("github.com"), None);
    }

    #[test]
    fn remote_is_parsed_from_field() {
        let r = repo();
        let remote = r.remote().expect("应能解析");
        assert_eq!(remote.host, "github.com");
        assert_eq!(remote.full_name(), "zhangxinyu/iot-gateway");
    }

    #[test]
    fn set_identity_updates_reference() {
        let mut r = repo();
        assert!(r.identity_id.is_none());
        r.set_identity("gh-alt");
        assert_eq!(r.identity_id.as_deref(), Some("gh-alt"));
    }
}
