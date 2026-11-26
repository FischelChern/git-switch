//! 平台：一个 git 托管服务的域名。
//!
//! 平台**只由远程地址的 host 推导**，UI 不允许手动切换。

use serde::{Deserialize, Serialize};

use super::remote::RemoteUrl;

/// 平台的类型，决定 API 形态与默认凭据助手。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlatformKind {
    GitHub,
    GitLab,
    Gitee,
    Bitbucket,
    /// 自建或未识别的服务，按 GitLab 风格处理。
    Custom,
}

impl PlatformKind {
    /// 展示用名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
            Self::Gitee => "Gitee",
            Self::Bitbucket => "Bitbucket",
            Self::Custom => "自定义",
        }
    }
}

/// 一个平台。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Platform {
    /// 稳定标识，如 `github`、`gitlab-company`。
    pub id: String,
    /// 展示名称，如 `GitHub`、`公司 GitLab`。
    pub name: String,
    /// 小写域名，如 `github.com`、`gitlab.company.com`。
    pub host: String,
    pub kind: PlatformKind,
}

impl Platform {
    /// 创建平台。`host` 会被转成小写。
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        host: impl Into<String>,
        kind: PlatformKind,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            host: host.into().trim().to_ascii_lowercase(),
            kind,
        }
    }

    /// 内置平台模板。
    pub fn presets() -> Vec<Self> {
        vec![
            Self::new("github", "GitHub", "github.com", PlatformKind::GitHub),
            Self::new("gitlab", "GitLab", "gitlab.com", PlatformKind::GitLab),
            Self::new("gitee", "Gitee", "gitee.com", PlatformKind::Gitee),
            Self::new(
                "bitbucket",
                "Bitbucket",
                "bitbucket.org",
                PlatformKind::Bitbucket,
            ),
        ]
    }

    /// `host` 是否命中本平台：完全相等，或是本平台的子域。
    ///
    /// 子域规则让 `api.github.com`、`gist.github.com` 也归到 `github.com`。
    pub fn matches_host(&self, host: &str) -> bool {
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() || self.host.is_empty() {
            return false;
        }
        host == self.host || host.ends_with(&format!(".{}", self.host))
    }
}

/// 按 host 找到平台。
///
/// 存在歧义时（多个平台命中同一个 host，例如 `gitlab.com` 与自建子域）
/// 返回**列表中最靠前**的一个。消歧是 backlog `P2-5`。
pub fn match_host<'a>(platforms: &'a [Platform], host: &str) -> Option<&'a Platform> {
    platforms.iter().find(|p| p.matches_host(host))
}

/// 由远程地址推导平台。
pub fn resolve<'a>(platforms: &'a [Platform], remote: &RemoteUrl) -> Option<&'a Platform> {
    match_host(platforms, &remote.host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_host_matches() {
        let p = Platform::new("github", "GitHub", "github.com", PlatformKind::GitHub);
        assert!(p.matches_host("github.com"));
        assert!(p.matches_host("GitHub.COM"));
    }

    #[test]
    fn subdomain_matches() {
        let p = Platform::new("github", "GitHub", "github.com", PlatformKind::GitHub);
        assert!(p.matches_host("api.github.com"));
        assert!(p.matches_host("gist.github.com"));
    }

    #[test]
    fn unrelated_host_does_not_match() {
        let p = Platform::new("github", "GitHub", "github.com", PlatformKind::GitHub);
        assert!(!p.matches_host("github.com.evil.example"));
        assert!(!p.matches_host("notgithub.com"));
        assert!(!p.matches_host(""));
    }

    #[test]
    fn custom_host_matches_itself() {
        let p = Platform::new(
            "corp",
            "公司 GitLab",
            "gitlab.company.com",
            PlatformKind::GitLab,
        );
        assert!(p.matches_host("gitlab.company.com"));
        assert!(!p.matches_host("gitlab.com"));
    }

    #[test]
    fn resolve_picks_matching_platform() {
        let platforms = Platform::presets();
        let remote = RemoteUrl::parse("git@github.com:o/r.git").expect("应解析成功");
        let p = resolve(&platforms, &remote).expect("应命中 GitHub");
        assert_eq!(p.id, "github");
    }

    #[test]
    fn resolve_returns_none_for_unknown_host() {
        let platforms = Platform::presets();
        let remote = RemoteUrl::parse("https://unknown.host/foo/bar.git").expect("应解析成功");
        assert!(resolve(&platforms, &remote).is_none());
    }

    #[test]
    fn presets_cover_the_big_three() {
        let hosts: Vec<String> = Platform::presets().into_iter().map(|p| p.host).collect();
        assert!(hosts.contains(&"github.com".to_string()));
        assert!(hosts.contains(&"gitlab.com".to_string()));
        assert!(hosts.contains(&"gitee.com".to_string()));
    }

    #[test]
    fn same_host_first_wins_documented_behaviour() {
        // 这是已知的歧义行为（backlog P2-5），此处固化现状防止误改
        let platforms = vec![
            Platform::new("a", "A", "gitlab.com", PlatformKind::GitLab),
            Platform::new("b", "B", "gitlab.com", PlatformKind::GitLab),
        ];
        let hit = match_host(&platforms, "gitlab.com").expect("应命中");
        assert_eq!(hit.id, "a");
    }
}
