//! 系统钥匙串封装。
//!
//! 令牌、密码、私钥口令**只存这里**，配置文件里只出现 [`crate::model::identity::Identity::secret_ref`] 的引用。
//!
//! - macOS：Keychain
//! - Windows：Credential Manager
//! - Linux：Secret Service（无桌面环境时会失败，返回可读错误）

use keyring::Entry;
use thiserror::Error;

/// 钥匙串里的服务名，所有条目共用。
pub const SERVICE: &str = "git-switch";

/// 钥匙串操作错误。
#[derive(Debug, Error)]
pub enum SecretError {
    /// 无法访问钥匙串（未授权、无桌面环境等）。
    #[error("无法访问系统钥匙串：{0}")]
    Unavailable(String),
    /// 写入失败。
    #[error("保存到钥匙串失败：{0}")]
    Store(String),
    /// 读取失败。
    #[error("从钥匙串读取失败：{0}")]
    Read(String),
    /// 删除失败。
    #[error("从钥匙串删除失败：{0}")]
    Delete(String),
}

fn entry(account: &str) -> Result<Entry, SecretError> {
    Entry::new(SERVICE, account).map_err(|e| SecretError::Unavailable(e.to_string()))
}

/// 写入（或覆盖）一条密文。
pub fn set_secret(account: &str, secret: &str) -> Result<(), SecretError> {
    entry(account)?
        .set_password(secret)
        .map_err(|e| SecretError::Store(e.to_string()))
}

/// 读取密文；不存在时返回 `Ok(None)`。
pub fn get_secret(account: &str) -> Result<Option<String>, SecretError> {
    match entry(account)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(SecretError::Read(e.to_string())),
    }
}

/// 删除密文；不存在时视为成功。
pub fn delete_secret(account: &str) -> Result<(), SecretError> {
    match entry(account)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(SecretError::Delete(e.to_string())),
    }
}

/// 判断钥匙串是否可用：对一个不存在的条目做一次读取。
pub fn is_available() -> bool {
    match entry("__git-switch-probe__") {
        Err(_) => false,
        Ok(e) => matches!(e.get_password(), Ok(_) | Err(keyring::Error::NoEntry)),
    }
}

/// 该账号是否已保存密文。
pub fn has_secret(account: &str) -> bool {
    matches!(get_secret(account), Ok(Some(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_is_stable() {
        // 改名会让所有已保存的凭据失效，锁定防止误改
        assert_eq!(SERVICE, "git-switch");
    }

    #[test]
    fn missing_entry_returns_none() {
        let account = format!("__git-switch-test-missing-{}", std::process::id());
        match get_secret(&account) {
            // 钥匙串可用：不存在应返回 None
            Ok(v) => assert!(v.is_none(), "不应读到内容"),
            // 无钥匙串的环境（CI / 无桌面 Linux）：跳过
            Err(SecretError::Unavailable(_)) => {}
            Err(e) => panic!("意外错误：{e}"),
        }
    }

    #[test]
    fn delete_missing_entry_is_ok() {
        let account = format!("__git-switch-test-delete-{}", std::process::id());
        match delete_secret(&account) {
            Ok(()) => {}
            Err(SecretError::Unavailable(_)) => {}
            Err(e) => panic!("删除不存在的条目应成功，实际：{e}"),
        }
    }

    #[test]
    #[ignore = "需要真实钥匙串，手动运行：cargo test -- --ignored"]
    fn full_roundtrip_against_real_keychain() {
        let account = format!("__git-switch-test-roundtrip-{}", std::process::id());
        set_secret(&account, "s3cret-token").expect("应能写入");
        assert_eq!(
            get_secret(&account).expect("应能读取").as_deref(),
            Some("s3cret-token")
        );
        assert!(has_secret(&account));
        delete_secret(&account).expect("应能删除");
        assert_eq!(get_secret(&account).expect("应能读取"), None);
    }
}
