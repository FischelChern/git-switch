//! 读写仓库本地配置，并保证**失败可回滚**。
//!
//! 只碰 `<repo>/.git/config` 里的四个键：
//!
//! | 键 | 何时写入 |
//! |---|---|
//! | `user.name` / `user.email` | 始终写（值是「项目覆盖 > 身份默认」的结果） |
//! | `core.sshCommand` | 认证方式为 SSH 且配置了私钥路径 |
//! | `credential.helper` | 认证方式为令牌 / 密码 / 凭据文件 |
//!
//! **绝不修改 `~/.gitconfig`。**

use std::path::Path;

use crate::model::identity::{AuthMode, Identity};
use crate::model::repository::EffectiveIdentity;

use super::exec::{GitError, GitExecutor};

/// `user.name`。
pub const KEY_USER_NAME: &str = "user.name";
/// `user.email`。
pub const KEY_USER_EMAIL: &str = "user.email";
/// `core.sshCommand`。
pub const KEY_SSH_COMMAND: &str = "core.sshCommand";
/// `credential.helper`。
pub const KEY_CREDENTIAL_HELPER: &str = "credential.helper";

/// 与身份相关的全部键，用于快照与回滚。
pub const IDENTITY_KEYS: [&str; 4] = [
    KEY_USER_NAME,
    KEY_USER_EMAIL,
    KEY_SSH_COMMAND,
    KEY_CREDENTIAL_HELPER,
];

/// 当前平台对应的凭据助手。
pub fn credential_helper() -> &'static str {
    if cfg!(target_os = "macos") {
        "osxkeychain"
    } else if cfg!(target_os = "windows") {
        "manager"
    } else {
        "libsecret"
    }
}

/// 拼出 `core.sshCommand` 的值。
///
/// 路径含空格时必须加引号，否则 git 调起 ssh 时会被拆成两个参数。
pub fn ssh_command(key_path: &str) -> String {
    let quoted = if key_path.contains(' ') || key_path.contains('\t') {
        format!("\"{key_path}\"")
    } else {
        key_path.to_string()
    };
    format!("ssh -i {quoted} -o IdentitiesOnly=yes")
}

/// 仓库本地配置里的身份相关字段。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalIdentity {
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub ssh_command: Option<String>,
    pub credential_helper: Option<String>,
}

impl LocalIdentity {
    /// 是否没有任何本地身份配置。
    pub fn is_empty(&self) -> bool {
        self.user_name.is_none()
            && self.user_email.is_none()
            && self.ssh_command.is_none()
            && self.credential_helper.is_none()
    }
}

/// 读取单个本地配置项；不存在时返回 `Ok(None)`。
pub fn get_local(repo: &Path, key: &str) -> Result<Option<String>, GitError> {
    let out = GitExecutor::new(repo).run(&["config", "--local", "--get", key])?;
    if out.success {
        let value = out.stdout.trim_end().to_string();
        Ok(Some(value))
    } else {
        // 退出码 1：键不存在，不是错误
        Ok(None)
    }
}

/// 写入单个本地配置项。
pub fn set_local(repo: &Path, key: &str, value: &str) -> Result<(), GitError> {
    GitExecutor::new(repo).run_ok(&["config", "--local", key, value])?;
    Ok(())
}

/// 删除单个本地配置项；不存在时视为成功。
pub fn unset_local(repo: &Path, key: &str) -> Result<(), GitError> {
    let out = GitExecutor::new(repo).run(&["config", "--local", "--unset", key])?;
    // git 在键不存在时返回 5
    if out.success || out.code == Some(5) {
        Ok(())
    } else {
        Err(GitError::CommandFailed {
            command: out.command,
            stderr: out.stderr,
        })
    }
}

/// 读取全部身份相关字段。
pub fn read_local(repo: &Path) -> Result<LocalIdentity, GitError> {
    Ok(LocalIdentity {
        user_name: get_local(repo, KEY_USER_NAME)?,
        user_email: get_local(repo, KEY_USER_EMAIL)?,
        ssh_command: get_local(repo, KEY_SSH_COMMAND)?,
        credential_helper: get_local(repo, KEY_CREDENTIAL_HELPER)?,
    })
}

/// 写入前的配置快照，用于失败回滚。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigSnapshot {
    entries: Vec<(String, Option<String>)>,
}

impl ConfigSnapshot {
    /// 记录四个键的当前值。
    pub fn capture(repo: &Path) -> Result<Self, GitError> {
        let mut entries = Vec::with_capacity(IDENTITY_KEYS.len());
        for key in IDENTITY_KEYS {
            entries.push((key.to_string(), get_local(repo, key)?));
        }
        Ok(Self { entries })
    }

    /// 恢复快照：原来有值的写回，原来没有的删除。
    pub fn restore(&self, repo: &Path) -> Result<(), GitError> {
        for (key, value) in &self.entries {
            match value {
                Some(v) => set_local(repo, key, v)?,
                None => unset_local(repo, key)?,
            }
        }
        Ok(())
    }

    /// 快照内容，供测试与日志使用。
    pub fn entries(&self) -> &[(String, Option<String>)] {
        &self.entries
    }
}

/// 把身份（含项目级覆盖）写进仓库本地配置。
///
/// 任何一步失败都会把仓库恢复到调用前的状态。
pub fn apply_identity(
    repo: &Path,
    identity: &Identity,
    effective: &EffectiveIdentity,
) -> Result<(), GitError> {
    let snapshot = ConfigSnapshot::capture(repo)?;
    match apply_inner(repo, identity, effective) {
        Ok(()) => Ok(()),
        Err(err) => {
            // 回滚失败也不能掩盖原始错误，原始错误优先返回
            let _ = snapshot.restore(repo);
            Err(err)
        }
    }
}

fn apply_inner(
    repo: &Path,
    identity: &Identity,
    effective: &EffectiveIdentity,
) -> Result<(), GitError> {
    set_local(repo, KEY_USER_NAME, &effective.user_name)?;
    set_local(repo, KEY_USER_EMAIL, &effective.user_email)?;

    match identity.auth {
        AuthMode::Ssh => {
            match identity.ssh_key_path.as_deref() {
                Some(key) if !key.trim().is_empty() => {
                    set_local(repo, KEY_SSH_COMMAND, &ssh_command(key.trim()))?;
                }
                _ => {
                    // 没配私钥：清掉，跟随全局 ssh 配置
                    unset_local(repo, KEY_SSH_COMMAND)?;
                }
            }
        }
        AuthMode::Token | AuthMode::Password => {
            unset_local(repo, KEY_SSH_COMMAND)?;
            set_local(repo, KEY_CREDENTIAL_HELPER, credential_helper())?;
        }
        AuthMode::CredentialFile => {
            unset_local(repo, KEY_SSH_COMMAND)?;
            set_local(repo, KEY_CREDENTIAL_HELPER, "store")?;
        }
    }
    Ok(())
}

/// 清除仓库本地的身份配置，回到「跟随全局」。
pub fn clear_identity(repo: &Path) -> Result<(), GitError> {
    for key in [KEY_USER_NAME, KEY_USER_EMAIL, KEY_SSH_COMMAND] {
        unset_local(repo, key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_helper_is_platform_specific() {
        let helper = credential_helper();
        assert!(
            matches!(helper, "osxkeychain" | "manager" | "libsecret"),
            "意外的凭据助手：{helper}"
        );
    }

    #[test]
    fn ssh_command_includes_key_and_identities_only() {
        let cmd = ssh_command("~/.ssh/id_ed25519_github");
        assert!(cmd.contains("-i ~/.ssh/id_ed25519_github"));
        assert!(cmd.contains("-o IdentitiesOnly=yes"));
        assert!(cmd.starts_with("ssh "));
    }

    #[test]
    fn ssh_command_quotes_path_with_space() {
        let cmd = ssh_command("/Users/me/My Keys/id_ed25519");
        assert!(
            cmd.contains("\"/Users/me/My Keys/id_ed25519\""),
            "实际：{cmd}"
        );
    }

    #[test]
    fn ssh_command_quotes_path_with_tab() {
        let cmd = ssh_command("/tmp/a\tb");
        assert!(cmd.contains("\"/tmp/a\tb\""), "实际：{cmd}");
    }

    #[test]
    fn empty_local_identity_detection() {
        assert!(LocalIdentity::default().is_empty());
        let with_name = LocalIdentity {
            user_name: Some("x".into()),
            ..Default::default()
        };
        assert!(!with_name.is_empty());
    }

    #[test]
    fn snapshot_entries_cover_all_identity_keys() {
        let snap = ConfigSnapshot {
            entries: IDENTITY_KEYS
                .iter()
                .map(|k| (k.to_string(), None))
                .collect(),
        };
        assert_eq!(snap.entries().len(), 4);
    }
}
