//! 身份：一套 `user.name` + `user.email` + 一种认证方式 + 凭据引用。
//!
//! 身份属于**平台**，可以被多个项目引用；身份本身不持有项目。

use serde::{Deserialize, Serialize};

/// 认证方式，决定切换时写哪个 git 配置项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    /// SSH 私钥，写 `core.sshCommand`。
    Ssh,
    /// 访问令牌，凭据交给系统助手，写 `credential.helper`。
    Token,
    /// 账号密码，同上。
    Password,
    /// 凭据文件（`.netrc`）。
    CredentialFile,
}

impl AuthMode {
    /// 展示用名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::Ssh => "SSH 密钥",
            Self::Token => "令牌",
            Self::Password => "账号密码",
            Self::CredentialFile => "凭据文件",
        }
    }

    /// 是否需要把一段密文存进系统钥匙串。
    pub fn needs_secret(self) -> bool {
        matches!(self, Self::Token | Self::Password)
    }

    /// 是否通过 SSH 通道通信。
    pub fn is_ssh(self) -> bool {
        matches!(self, Self::Ssh)
    }
}

/// 一个 git 身份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// 稳定标识，如 `gh-me`。
    pub id: String,
    /// 所属平台 id。
    pub platform_id: String,
    /// 展示名称，如 `张鑫宇 · 个人`。
    pub label: String,
    /// 写入 `user.name` 的值。
    pub user_name: String,
    /// 写入 `user.email` 的值。
    pub user_email: String,
    pub auth: AuthMode,
    /// SSH 私钥路径（`auth == Ssh` 时有效）。只存路径，不存私钥内容。
    #[serde(default)]
    pub ssh_key_path: Option<String>,
}

impl Identity {
    /// 新建身份。
    pub fn new(
        id: impl Into<String>,
        platform_id: impl Into<String>,
        label: impl Into<String>,
        user_name: impl Into<String>,
        user_email: impl Into<String>,
        auth: AuthMode,
    ) -> Self {
        Self {
            id: id.into(),
            platform_id: platform_id.into(),
            label: label.into(),
            user_name: user_name.into(),
            user_email: user_email.into(),
            auth,
            ssh_key_path: None,
        }
    }

    /// 带上 SSH 私钥路径。
    pub fn with_ssh_key(mut self, path: impl Into<String>) -> Self {
        self.ssh_key_path = Some(path.into());
        self
    }

    /// 钥匙串里的引用标识。**配置文件中只存这个，不存密文。**
    pub fn secret_ref(&self) -> String {
        format!("git-switch:identity:{}", self.id)
    }

    /// 首字母，用于 UI 头像。
    pub fn initial(&self) -> String {
        self.label
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_ref_is_stable_and_namespaced() {
        let id = Identity::new(
            "gh-me",
            "github",
            "张鑫宇 · 个人",
            "张鑫宇",
            "me@example.com",
            AuthMode::Ssh,
        );
        assert_eq!(id.secret_ref(), "git-switch:identity:gh-me");
    }

    #[test]
    fn only_token_and_password_need_secret() {
        assert!(AuthMode::Token.needs_secret());
        assert!(AuthMode::Password.needs_secret());
        assert!(!AuthMode::Ssh.needs_secret());
        assert!(!AuthMode::CredentialFile.needs_secret());
    }

    #[test]
    fn initial_is_first_char_of_label() {
        let id = Identity::new("a", "p", "阿张 · 工作", "z", "e", AuthMode::Token);
        assert_eq!(id.initial(), "阿");
    }

    #[test]
    fn initial_is_empty_for_empty_label() {
        let id = Identity::new("a", "p", "", "z", "e", AuthMode::Token);
        assert_eq!(id.initial(), "");
    }
}
