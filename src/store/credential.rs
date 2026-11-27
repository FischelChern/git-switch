//! 凭据状态查询。
//!
//! 把「该身份的密文是否已经在系统钥匙串里」这一判断集中到一处，
//! 供侧栏徽标与「测试认证」共用。
//!
//! **只读**：本模块绝不读取密文内容，只判断存在性。

use crate::model::identity::Identity;
use crate::store::secret;

/// 单条凭据的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredStatus {
    /// 密文已保存在系统钥匙串。
    Saved,
    /// 该认证方式需要密文，但钥匙串里没有。
    Missing,
    /// 该认证方式本就不需要密文（SSH 私钥、凭据文件）。
    NotRequired,
    /// 无法访问系统钥匙串（无桌面环境、未授权）。
    Unavailable,
}

impl CredStatus {
    /// 徽标字符。
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Saved => "✓",
            Self::Missing => "!",
            Self::NotRequired => "—",
            Self::Unavailable => "?",
        }
    }

    /// 中文说明，用于悬停提示与终端回显。
    pub fn label(self) -> &'static str {
        match self {
            Self::Saved => "凭据已保存到钥匙串",
            Self::Missing => "凭据未保存",
            Self::NotRequired => "该认证方式无需钥匙串密文",
            Self::Unavailable => "无法访问系统钥匙串",
        }
    }

    /// 是否算「可用」。
    pub fn is_ok(self) -> bool {
        matches!(self, Self::Saved | Self::NotRequired)
    }
}

/// 查询某身份的凭据状态。
pub fn status(identity: &Identity) -> CredStatus {
    if !identity.auth.needs_secret() {
        return CredStatus::NotRequired;
    }
    // 钥匙串不可用与「条目不存在」要能区分开，否则会给出误导性结论
    match secret::get_secret(&identity.secret_ref()) {
        Ok(Some(_)) => CredStatus::Saved,
        Ok(None) => CredStatus::Missing,
        Err(secret::SecretError::Unavailable(_)) => CredStatus::Unavailable,
        // 其它错误（读失败、删除失败）统一按不可用处理，不假装成功
        Err(_) => CredStatus::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::identity::AuthMode;

    fn ident(auth: AuthMode) -> Identity {
        Identity::new("t", "p", "标签", "n", "e@x.com", auth)
    }

    #[test]
    fn ssh_and_file_never_require_secret() {
        assert_eq!(status(&ident(AuthMode::Ssh)), CredStatus::NotRequired);
        assert_eq!(
            status(&ident(AuthMode::CredentialFile)),
            CredStatus::NotRequired
        );
    }

    #[test]
    fn token_without_stored_secret_is_missing_or_unavailable() {
        let id = ident(AuthMode::Token);
        match status(&id) {
            // 钥匙串可用但没存过 → Missing
            CredStatus::Missing => {}
            // 无钥匙串的环境（CI / 无桌面 Linux）→ Unavailable
            CredStatus::Unavailable => {}
            other => panic!("不应为 {other:?}"),
        }
    }

    #[test]
    fn glyph_and_label_are_distinct() {
        let all = [
            CredStatus::Saved,
            CredStatus::Missing,
            CredStatus::NotRequired,
            CredStatus::Unavailable,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a.glyph(), b.glyph(), "{a:?} 与 {b:?} 徽标重复");
                assert_ne!(a.label(), b.label());
            }
        }
    }

    #[test]
    fn is_ok_only_for_saved_and_not_required() {
        assert!(CredStatus::Saved.is_ok());
        assert!(CredStatus::NotRequired.is_ok());
        assert!(!CredStatus::Missing.is_ok());
        assert!(!CredStatus::Unavailable.is_ok());
    }
}
