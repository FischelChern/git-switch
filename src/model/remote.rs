//! 远程地址解析：把 `git remote get-url` 的输出解析成 host / owner / repo。
//!
//! 支持的写法（纯字符串处理，不启动任何进程）：
//!
//! ```text
//! git@github.com:owner/repo.git           scp 形式
//! ssh://git@gitlab.company.com/erp/x.git  ssh:// 形式
//! https://gitee.com/owner/repo.git        https 形式
//! https://github.com/owner/repo           无 .git 后缀
//! ```

use std::fmt;

/// 远程地址使用的协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteProtocol {
    Ssh,
    Https,
    Http,
    Git,
    File,
}

impl RemoteProtocol {
    /// 是否为 SSH（决定写 `core.sshCommand` 还是 `credential.helper`）。
    pub fn is_ssh(self) -> bool {
        matches!(self, Self::Ssh)
    }

    /// 展示用短名称。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ssh => "SSH",
            Self::Https => "HTTPS",
            Self::Http => "HTTP",
            Self::Git => "GIT",
            Self::File => "FILE",
        }
    }
}

/// 解析后的远程地址。`host` 一律小写，不含端口与用户信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteUrl {
    pub host: String,
    pub port: Option<u16>,
    pub owner: String,
    pub repo: String,
    pub protocol: RemoteProtocol,
}

/// 解析失败原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteParseError {
    /// 输入为空。
    Empty,
    /// 无法识别的格式。
    Unrecognized,
    /// 缺少 host。
    MissingHost,
    /// 缺少仓库名。
    MissingRepo,
    /// 不支持的协议。
    UnsupportedScheme(String),
}

impl fmt::Display for RemoteParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "仓库地址为空"),
            Self::Unrecognized => write!(f, "无法识别的仓库地址格式"),
            Self::MissingHost => write!(f, "仓库地址缺少主机名"),
            Self::MissingRepo => write!(f, "仓库地址缺少仓库名"),
            Self::UnsupportedScheme(s) => write!(f, "不支持的协议：{s}"),
        }
    }
}

impl std::error::Error for RemoteParseError {}

impl RemoteUrl {
    /// 解析远程地址。
    pub fn parse(input: &str) -> Result<Self, RemoteParseError> {
        let s = input.trim();
        if s.is_empty() {
            return Err(RemoteParseError::Empty);
        }

        if let Some(idx) = s.find("://") {
            Self::parse_scheme(&s[..idx], &s[idx + 3..])
        } else {
            Self::parse_scp(s)
        }
    }

    /// `scheme://[user@]host[:port]/path`
    fn parse_scheme(scheme: &str, rest: &str) -> Result<Self, RemoteParseError> {
        let protocol = match scheme.to_ascii_lowercase().as_str() {
            "ssh" => RemoteProtocol::Ssh,
            "https" => RemoteProtocol::Https,
            "http" => RemoteProtocol::Http,
            "git" => RemoteProtocol::Git,
            "file" => RemoteProtocol::File,
            other => return Err(RemoteParseError::UnsupportedScheme(other.to_string())),
        };

        let (authority, path) = rest.split_once('/').ok_or(RemoteParseError::MissingRepo)?;
        let host_port = authority.rsplit('@').next().unwrap_or(authority);
        let (host, port) = split_host_port(host_port);

        Self::build(protocol, host, port, path)
    }

    /// `[user@]host:path`
    fn parse_scp(s: &str) -> Result<Self, RemoteParseError> {
        let (left, path) = s.split_once(':').ok_or(RemoteParseError::Unrecognized)?;
        // 反斜杠形式（Windows 本地路径）不是远程地址
        if path.contains('\\') || left.contains('\\') {
            return Err(RemoteParseError::Unrecognized);
        }
        let host = left.rsplit('@').next().unwrap_or(left);
        Self::build(RemoteProtocol::Ssh, host, None, path)
    }

    fn build(
        protocol: RemoteProtocol,
        host: &str,
        port: Option<u16>,
        raw_path: &str,
    ) -> Result<Self, RemoteParseError> {
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() {
            return Err(RemoteParseError::MissingHost);
        }

        // 先去掉首尾斜杠，再剥 `.git`，最后再清一次尾斜杠：
        // `https://host/o/r.git/` 这种写法要求这个顺序
        let path = raw_path.trim().trim_start_matches('/');
        let path = path.trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let path = path.trim_end_matches('/');
        if path.is_empty() {
            return Err(RemoteParseError::MissingRepo);
        }

        let (owner, repo) = match path.rsplit_once('/') {
            Some((owner, repo)) => (owner.to_string(), repo.to_string()),
            None => (String::new(), path.to_string()),
        };

        if repo.is_empty() {
            return Err(RemoteParseError::MissingRepo);
        }

        Ok(Self {
            host,
            port,
            owner,
            repo,
            protocol,
        })
    }

    /// `owner/repo`；owner 为空时只返回 repo。
    pub fn full_name(&self) -> String {
        if self.owner.is_empty() {
            self.repo.clone()
        } else {
            format!("{}/{}", self.owner, self.repo)
        }
    }
}

/// 拆分 `host:port`。端口解析失败时整体视为 host。
fn split_host_port(input: &str) -> (&str, Option<u16>) {
    match input.rsplit_once(':') {
        Some((host, port)) => match port.parse::<u16>() {
            Ok(p) => (host, Some(p)),
            Err(_) => (input, None),
        },
        None => (input, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> RemoteUrl {
        match RemoteUrl::parse(input) {
            Ok(v) => v,
            Err(e) => panic!("应解析成功：{input} → {e}"),
        }
    }

    #[test]
    fn scp_form_with_git_suffix() {
        let r = parse("git@github.com:zhangxinyu/iot-gateway.git");
        assert_eq!(r.host, "github.com");
        assert_eq!(r.owner, "zhangxinyu");
        assert_eq!(r.repo, "iot-gateway");
        assert_eq!(r.protocol, RemoteProtocol::Ssh);
        assert!(r.protocol.is_ssh());
    }

    #[test]
    fn https_form() {
        let r = parse("https://github.com/zhangxinyu/iot-gateway.git");
        assert_eq!(r.host, "github.com");
        assert_eq!(r.full_name(), "zhangxinyu/iot-gateway");
        assert_eq!(r.protocol, RemoteProtocol::Https);
        assert!(!r.protocol.is_ssh());
    }

    #[test]
    fn ssh_scheme_form_with_custom_host() {
        let r = parse("ssh://git@gitlab.company.com/erp/backend.git");
        assert_eq!(r.host, "gitlab.company.com");
        assert_eq!(r.owner, "erp");
        assert_eq!(r.repo, "backend");
        assert!(r.protocol.is_ssh());
    }

    #[test]
    fn https_without_git_suffix() {
        let r = parse("https://gitee.com/3237138707/mqtt-bridge");
        assert_eq!(r.host, "gitee.com");
        assert_eq!(r.repo, "mqtt-bridge");
    }

    #[test]
    fn scp_with_custom_host() {
        let r = parse("git@gitlab.company.com:erp/report.git");
        assert_eq!(r.host, "gitlab.company.com");
        assert_eq!(r.full_name(), "erp/report");
    }

    #[test]
    fn nested_owner_is_kept() {
        let r = parse("git@gitlab.company.com:group/sub/project.git");
        assert_eq!(r.owner, "group/sub");
        assert_eq!(r.repo, "project");
        assert_eq!(r.full_name(), "group/sub/project");
    }

    #[test]
    fn trailing_and_leading_slash() {
        let r = parse("https://github.com/o/r.git/");
        assert_eq!(r.repo, "r");
    }

    #[test]
    fn port_is_parsed() {
        let r = parse("ssh://git@gitlab.company.com:2222/erp/backend.git");
        assert_eq!(r.host, "gitlab.company.com");
        assert_eq!(r.port, Some(2222));
        assert_eq!(r.repo, "backend");
    }

    #[test]
    fn host_is_lowercased() {
        let r = parse("https://GitHub.COM/o/r.git");
        assert_eq!(r.host, "github.com");
    }

    #[test]
    fn unknown_host_still_parses() {
        let r = parse("https://unknown.host/foo/bar.git");
        assert_eq!(r.host, "unknown.host");
        assert_eq!(r.full_name(), "foo/bar");
    }

    #[test]
    fn repo_without_owner() {
        let r = parse("https://github.com/single.git");
        assert_eq!(r.owner, "");
        assert_eq!(r.repo, "single");
        assert_eq!(r.full_name(), "single");
    }

    #[test]
    fn rejects_empty_and_garbage() {
        assert_eq!(RemoteUrl::parse(""), Err(RemoteParseError::Empty));
        assert_eq!(RemoteUrl::parse("   "), Err(RemoteParseError::Empty));
        assert_eq!(
            RemoteUrl::parse("not-a-url"),
            Err(RemoteParseError::Unrecognized)
        );
    }

    #[test]
    fn rejects_unsupported_scheme() {
        assert!(matches!(
            RemoteUrl::parse("ftp://github.com/o/r.git"),
            Err(RemoteParseError::UnsupportedScheme(_))
        ));
    }

    #[test]
    fn rejects_missing_repo() {
        assert_eq!(
            RemoteUrl::parse("git@github.com:"),
            Err(RemoteParseError::MissingRepo)
        );
        assert_eq!(
            RemoteUrl::parse("https://github.com/"),
            Err(RemoteParseError::MissingRepo)
        );
    }

    #[test]
    fn windows_local_path_is_not_remote() {
        assert!(RemoteUrl::parse(r"C:\Users\me\repo").is_err());
    }
}
