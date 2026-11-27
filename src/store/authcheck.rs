//! 「测试认证」：把身份配置与远程地址做自洽性检查。
//!
//! 明确的能力边界：**本模块不发网络请求**。真实的连通性探测（`ssh -T`、
//! `git ls-remote`）需要网络与凭据，属于 [`crate::git`] 的职责，
//! 且必须由用户显式触发。这里只做本地可判定的检查，并如实标注未探测。
//!
//! 结论三档：`Pass` / `Warn` / `Fail`，取最差的一条。

use crate::model::identity::{AuthMode, Identity};
use crate::model::remote::RemoteProtocol;
use crate::store::credential::CredStatus;

/// 单项检查结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// 确定没问题。
    Pass,
    /// 本地无法判定，需要实机确认。
    Warn,
    /// 确定有问题，会导致鉴权失败。
    Fail,
}

impl Level {
    /// 前缀符号。
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Pass => "✓",
            Self::Warn => "!",
            Self::Fail => "✗",
        }
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "通过",
            Self::Warn => "需人工确认",
            Self::Fail => "失败",
        }
    }
}

/// 一条检查项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub level: Level,
    pub message: String,
}

impl Check {
    fn pass(message: impl Into<String>) -> Self {
        Self {
            level: Level::Pass,
            message: message.into(),
        }
    }

    fn warn(message: impl Into<String>) -> Self {
        Self {
            level: Level::Warn,
            message: message.into(),
        }
    }

    fn fail(message: impl Into<String>) -> Self {
        Self {
            level: Level::Fail,
            message: message.into(),
        }
    }
}

/// 一次测试的完整结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    /// 最差的一档；没有检查项时视为 `Warn`（不能凭空判通过）。
    pub fn level(&self) -> Level {
        self.checks
            .iter()
            .map(|c| c.level)
            .max()
            .unwrap_or(Level::Warn)
    }

    /// 结论文字。
    pub fn headline(&self) -> String {
        format!("结论：{}", self.level().label())
    }

    /// 是否全部通过。
    pub fn is_pass(&self) -> bool {
        self.level() == Level::Pass
    }

    /// 终端回显用的逐行文本。
    pub fn lines(&self) -> Vec<(Level, String)> {
        self.checks
            .iter()
            .map(|c| (c.level, format!("{} {}", c.level.glyph(), c.message)))
            .chain(std::iter::once((self.level(), self.headline())))
            .collect()
    }
}

/// 需要记录到钥匙串的账号键。
pub fn secret_account(identity: &Identity) -> String {
    identity.secret_ref()
}

/// 执行检查。`remote` 为空表示该仓库还没配置远程地址。
pub fn run(
    identity: &Identity,
    cred: CredStatus,
    remote: &str,
    protocol: Option<RemoteProtocol>,
) -> Report {
    let mut checks = Vec::new();

    // 1. 远程地址与认证方式是否自洽
    match protocol {
        None => checks.push(Check::warn(
            "项目未配置远程地址，无法校验协议与认证方式是否匹配",
        )),
        Some(p) => {
            let ssh_remote = matches!(p, RemoteProtocol::Ssh | RemoteProtocol::File);
            match (identity.auth.is_ssh(), ssh_remote) {
                (true, true) => checks.push(Check::pass(format!(
                    "认证方式 {} 与远程协议 {} 一致",
                    identity.auth.label(),
                    p.as_str()
                ))),
                (false, false) => checks.push(Check::pass(format!(
                    "认证方式 {} 与远程协议 {} 一致",
                    identity.auth.label(),
                    p.as_str()
                ))),
                (true, false) => checks.push(Check::fail(format!(
                    "身份用 {} 但远程地址是 {}（{remote}），推送会被拒绝",
                    identity.auth.label(),
                    p.as_str()
                ))),
                (false, true) => checks.push(Check::fail(format!(
                    "远程地址是 {}（{remote}）但身份用 {}，无法通过 SSH 鉴权",
                    p.as_str(),
                    identity.auth.label()
                ))),
            }
        }
    }

    // 2. 该认证方式需要的凭据
    match cred {
        CredStatus::Saved => checks.push(Check::pass("凭据已保存到系统钥匙串（内容不回显）")),
        CredStatus::Missing => checks.push(Check::fail(format!(
            "凭据未保存：请重新填写 {} 并保存",
            identity.auth.label()
        ))),
        CredStatus::Unavailable => {
            checks.push(Check::warn("无法访问系统钥匙串，无法确认凭据是否保存"))
        }
        CredStatus::NotRequired => match identity.auth {
            AuthMode::Ssh => match identity.ssh_key_path.as_deref().map(str::trim) {
                Some(p) if !p.is_empty() => checks.push(Check::warn(format!(
                    "私钥路径 {p} 需在实机确认存在且权限为 600"
                ))),
                _ => checks.push(Check::warn("未填私钥路径，将回退到全局 ssh 配置")),
            },
            AuthMode::CredentialFile => {
                checks.push(Check::warn("凭据文件方式：需确认 .netrc 权限为 600"))
            }
            // needs_secret() 为 true 时不会走到这里
            AuthMode::Token | AuthMode::Password => {}
        },
    }

    // 3. 明确说明未做的检查，避免被误读成「已经连过」
    checks.push(Check::warn(format!(
        "未做真实网络探测（{}）；实际连通性请用「获取」或 `ssh -T` 自行确认",
        match protocol {
            Some(p) => format!("{} 上的 {}", p.as_str(), host_of(remote)),
            None => "远程主机".to_string(),
        }
    )));

    Report { checks }
}

/// 从远程地址里取 host，仅用于提示文案。
fn host_of(remote: &str) -> String {
    crate::model::remote::RemoteUrl::parse(remote)
        .map(|u| u.host)
        .unwrap_or_else(|_| remote.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh_ident() -> Identity {
        Identity::new(
            "gh-me",
            "github",
            "张鑫宇",
            "张鑫宇",
            "me@example.com",
            AuthMode::Ssh,
        )
        .with_ssh_key("~/.ssh/id_ed25519")
    }

    fn token_ident() -> Identity {
        Identity::new(
            "gh-alt",
            "github",
            "小号",
            "zx",
            "alt@example.com",
            AuthMode::Token,
        )
    }

    #[test]
    fn ssh_identity_with_ssh_remote_passes_protocol_check() {
        let r = run(
            &ssh_ident(),
            CredStatus::NotRequired,
            "git@github.com:o/r.git",
            Some(RemoteProtocol::Ssh),
        );
        assert!(!r.is_pass(), "未做网络探测不应判全通过");
        assert!(r.level() == Level::Warn);
        assert!(
            r.checks.iter().any(|c| c.message.contains("一致")),
            "{:?}",
            r.checks
        );
    }

    #[test]
    fn ssh_identity_with_https_remote_fails() {
        let r = run(
            &ssh_ident(),
            CredStatus::NotRequired,
            "https://gitee.com/o/r.git",
            Some(RemoteProtocol::Https),
        );
        assert_eq!(r.level(), Level::Fail);
        assert!(r.checks.iter().any(|c| c.message.contains("推送会被拒绝")));
    }

    #[test]
    fn token_identity_with_ssh_remote_fails() {
        let r = run(
            &token_ident(),
            CredStatus::Saved,
            "git@github.com:o/r.git",
            Some(RemoteProtocol::Ssh),
        );
        assert_eq!(r.level(), Level::Fail);
        assert!(
            r.checks
                .iter()
                .any(|c| c.message.contains("无法通过 SSH 鉴权"))
        );
    }

    #[test]
    fn token_identity_without_secret_fails() {
        let r = run(
            &token_ident(),
            CredStatus::Missing,
            "https://github.com/o/r.git",
            Some(RemoteProtocol::Https),
        );
        assert_eq!(r.level(), Level::Fail);
        assert!(r.checks.iter().any(|c| c.message.contains("凭据未保存")));
    }

    #[test]
    fn token_identity_with_secret_only_warns_about_network() {
        let r = run(
            &token_ident(),
            CredStatus::Saved,
            "https://github.com/o/r.git",
            Some(RemoteProtocol::Https),
        );
        assert_eq!(r.level(), Level::Warn, "只应因未做网络探测而 warn");
        assert_eq!(
            r.checks.iter().filter(|c| c.level == Level::Warn).count(),
            1
        );
    }

    #[test]
    fn unavailable_keychain_warns_instead_of_failing() {
        let r = run(
            &token_ident(),
            CredStatus::Unavailable,
            "https://github.com/o/r.git",
            Some(RemoteProtocol::Https),
        );
        assert_eq!(r.level(), Level::Warn, "{:?}", r.checks);
        assert!(
            r.checks
                .iter()
                .any(|c| c.message.contains("无法访问系统钥匙串"))
        );
    }

    #[test]
    fn missing_remote_is_flagged() {
        let r = run(&ssh_ident(), CredStatus::NotRequired, "", None);
        assert!(
            r.checks
                .iter()
                .any(|c| c.message.contains("未配置远程地址"))
        );
    }

    #[test]
    fn ssh_without_key_warns_about_falling_back() {
        let id = Identity::new("a", "p", "l", "n", "e", AuthMode::Ssh);
        let r = run(
            &id,
            CredStatus::NotRequired,
            "git@h:o/r.git",
            Some(RemoteProtocol::Ssh),
        );
        assert!(
            r.checks.iter().any(|c| c.message.contains("回退到全局")),
            "{:?}",
            r.checks
        );
    }

    #[test]
    fn empty_checks_never_pass() {
        let r = Report { checks: Vec::new() };
        assert_eq!(r.level(), Level::Warn, "没有检查项不能判通过");
    }

    #[test]
    fn lines_include_headline_last() {
        let r = run(
            &token_ident(),
            CredStatus::Saved,
            "https://github.com/o/r.git",
            Some(RemoteProtocol::Https),
        );
        let lines = r.lines();
        assert!(lines.last().expect("应有结论").1.starts_with("结论："));
    }

    #[test]
    fn level_ordering_is_pass_warn_fail() {
        assert!(Level::Pass < Level::Warn);
        assert!(Level::Warn < Level::Fail);
    }

    #[test]
    fn secret_account_matches_identity_ref() {
        let id = token_ident();
        assert_eq!(secret_account(&id), "git-switch:identity:gh-alt");
    }
}
