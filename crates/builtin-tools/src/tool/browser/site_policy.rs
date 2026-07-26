use reqwest::Url;

use crate::tool::web_search::policy::source::{self, BrowserSourceFetchMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteFetchMode {
    StaticOnly,
    StaticThenBrowser,
    BrowserOnly,
    BrowserThenStatic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePolicy {
    pub mode: SiteFetchMode,
    pub policy_name: &'static str,
    pub reason: &'static str,
    pub preferred_lookup_hosts: &'static [&'static str],
    pub challenge_prone: bool,
}

impl SitePolicy {
    pub fn default_static_then_browser() -> Self {
        Self {
            mode: SiteFetchMode::StaticThenBrowser,
            policy_name: "default_static_then_browser",
            reason:
                "unknown site; attempt static fetch first, then escalate to browser on blockers",
            preferred_lookup_hosts: &[],
            challenge_prone: false,
        }
    }
}

pub fn policy_for_url(raw_url: &str) -> SitePolicy {
    let Ok(url) = Url::parse(raw_url) else {
        return SitePolicy::default_static_then_browser();
    };
    policy_for_host(url.host_str().unwrap_or_default())
}

pub fn policy_for_host(host: &str) -> SitePolicy {
    let lowered = host.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return SitePolicy::default_static_then_browser();
    }

    if let Some(policy) = source::policy_for_host(&lowered) {
        return SitePolicy {
            mode: match policy.fetch_mode {
                BrowserSourceFetchMode::StaticOnly => SiteFetchMode::StaticOnly,
                BrowserSourceFetchMode::StaticThenBrowser => SiteFetchMode::StaticThenBrowser,
                BrowserSourceFetchMode::BrowserOnly => SiteFetchMode::BrowserOnly,
                BrowserSourceFetchMode::BrowserThenStatic => SiteFetchMode::BrowserThenStatic,
            },
            policy_name: policy.policy_name,
            reason: policy.reason,
            preferred_lookup_hosts: policy.preferred_lookup_hosts,
            challenge_prone: policy.challenge_prone,
        };
    }

    SitePolicy::default_static_then_browser()
}

#[cfg(test)]
mod tests {
    use super::{policy_for_host, SiteFetchMode};

    #[test]
    fn github_prefers_static_first() {
        let policy = policy_for_host("github.com");
        assert_eq!(policy.mode, SiteFetchMode::StaticThenBrowser);
    }

    #[test]
    fn pubmed_is_static_only() {
        let policy = policy_for_host("pubmed.ncbi.nlm.nih.gov");
        assert_eq!(policy.mode, SiteFetchMode::StaticOnly);
    }

    #[test]
    fn academic_record_mirrors_are_static_only() {
        let policy = policy_for_host("pmc.ncbi.nlm.nih.gov");
        assert_eq!(policy.mode, SiteFetchMode::StaticOnly);

        let policy = policy_for_host("doi.org");
        assert_eq!(policy.mode, SiteFetchMode::StaticOnly);
    }

    #[test]
    fn etsy_is_browser_only() {
        let policy = policy_for_host("www.etsy.com");
        assert_eq!(policy.mode, SiteFetchMode::BrowserOnly);
    }

    #[test]
    fn lancet_policy_prefers_open_structured_alternatives() {
        let policy = policy_for_host("www.thelancet.com");
        assert_eq!(policy.mode, SiteFetchMode::BrowserThenStatic);
        assert!(policy.challenge_prone);
        assert!(policy
            .preferred_lookup_hosts
            .contains(&"pubmed.ncbi.nlm.nih.gov"));
        assert!(policy.preferred_lookup_hosts.contains(&"api.crossref.org"));
    }

    #[test]
    fn youtube_prefers_static_fetch() {
        let policy = policy_for_host("www.youtube.com");
        assert_eq!(policy.mode, SiteFetchMode::StaticOnly);
        assert!(!policy.challenge_prone);
    }

    #[test]
    fn social_login_heavy_sites_prefer_authenticated_browser_session() {
        let policy = policy_for_host("x.com");
        assert_eq!(policy.mode, SiteFetchMode::BrowserOnly);
        assert_eq!(policy.policy_name, "authenticated_browser_session_site");
        assert!(policy.challenge_prone);
    }

    #[test]
    fn public_developer_sources_are_static_only() {
        let policy = policy_for_host("stackoverflow.com");
        assert_eq!(policy.mode, SiteFetchMode::StaticOnly);
        assert!(!policy.challenge_prone);
    }

    #[test]
    fn finance_and_login_heavy_sources_require_browser_session() {
        let policy = policy_for_host("www.barchart.com");
        assert_eq!(policy.mode, SiteFetchMode::BrowserOnly);
        assert_eq!(policy.policy_name, "authenticated_browser_session_site");
        assert!(policy.challenge_prone);
    }

    #[test]
    fn feed_sources_stay_static_only() {
        let policy = policy_for_host("feeds.bbci.co.uk");
        assert_eq!(policy.mode, SiteFetchMode::StaticOnly);
        assert!(!policy.challenge_prone);
    }

    #[test]
    fn google_news_is_challenge_prone_browser_then_static() {
        let policy = policy_for_host("news.google.com");
        assert_eq!(policy.mode, SiteFetchMode::BrowserThenStatic);
        assert!(policy.challenge_prone);
    }

    #[test]
    fn representative_site_matrix_stays_classified() {
        let cases = [
            ("api.github.com", SiteFetchMode::StaticOnly, false),
            ("hn.algolia.com", SiteFetchMode::StaticOnly, false),
            ("huggingface.co", SiteFetchMode::StaticOnly, false),
            ("www.xiaoyuzhoufm.com", SiteFetchMode::StaticOnly, false),
            ("github.com", SiteFetchMode::StaticThenBrowser, false),
            ("www.v2ex.com", SiteFetchMode::StaticThenBrowser, false),
            ("www.bilibili.com", SiteFetchMode::StaticThenBrowser, true),
            ("www.zhihu.com", SiteFetchMode::StaticThenBrowser, true),
            ("book.douban.com", SiteFetchMode::StaticThenBrowser, true),
            ("x.com", SiteFetchMode::BrowserOnly, true),
            ("www.instagram.com", SiteFetchMode::BrowserOnly, true),
            ("www.linkedin.com", SiteFetchMode::BrowserOnly, true),
            ("mp.weixin.qq.com", SiteFetchMode::BrowserOnly, true),
            ("finance.yahoo.com", SiteFetchMode::BrowserOnly, true),
            ("www.reuters.com", SiteFetchMode::BrowserThenStatic, true),
            ("www.bloomberg.com", SiteFetchMode::BrowserThenStatic, true),
        ];

        for (host, expected_mode, expected_challenge) in cases {
            let policy = policy_for_host(host);
            assert_eq!(policy.mode, expected_mode, "{host}");
            assert_eq!(policy.challenge_prone, expected_challenge, "{host}");
        }
    }
}
