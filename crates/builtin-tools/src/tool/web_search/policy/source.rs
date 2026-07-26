#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserSourceFetchMode {
    StaticOnly,
    StaticThenBrowser,
    BrowserOnly,
    BrowserThenStatic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserSourceCapability {
    Public,
    Structured,
    Browser,
    Cookie,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSourcePolicy {
    pub id: &'static str,
    pub domains: &'static [&'static str],
    pub capability: BrowserSourceCapability,
    pub fetch_mode: BrowserSourceFetchMode,
    pub requires_browser: bool,
    pub requires_auth: bool,
    pub challenge_prone: bool,
    pub preferred_lookup_hosts: &'static [&'static str],
    pub fallback_sources: &'static [&'static str],
    pub policy_name: &'static str,
    pub reason: &'static str,
}

impl BrowserSourcePolicy {
    pub fn matches_host(&self, host: &str) -> bool {
        self.domains
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    }
}

pub fn policy_for_host(host: &str) -> Option<BrowserSourcePolicy> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    builtin_source_policies()
        .into_iter()
        .find(|policy| policy.matches_host(&host))
}

pub fn builtin_source_policies() -> Vec<BrowserSourcePolicy> {
    let mut policies = Vec::new();
    policies.extend(structured_static_sources());
    policies.extend(static_then_browser_sources());
    policies.extend(authenticated_browser_sources());
    policies.extend(semi_public_browser_sources());
    policies.extend(browser_only_sources());
    policies.extend(browser_then_static_sources());
    policies
}

fn structured_static_sources() -> Vec<BrowserSourcePolicy> {
    vec![
        BrowserSourcePolicy {
            id: "structured_static",
            domains: &[
                "api.github.com",
                "raw.githubusercontent.com",
                "pubmed.ncbi.nlm.nih.gov",
                "pmc.ncbi.nlm.nih.gov",
                "eutils.ncbi.nlm.nih.gov",
                "openalex.org",
                "api.openalex.org",
                "api.crossref.org",
                "crossref.org",
                "doi.org",
                "arxiv.org",
                "export.arxiv.org",
                "en.wikipedia.org",
                "wikipedia.org",
                "hn.algolia.com",
                "dev.to",
                "stackoverflow.com",
                "api.stackexchange.com",
                "huggingface.co",
                "itunes.apple.com",
                "rss.marketingtools.apple.com",
                "feeds.bbci.co.uk",
                "feeds.bloomberg.com",
                "lobste.rs",
                "store.steampowered.com",
                "app.cj.sina.com.cn",
                "suggestqueries.google.com",
                "trends.google.com",
                "www.xiaoyuzhoufm.com",
            ],
            capability: BrowserSourceCapability::Structured,
            fetch_mode: BrowserSourceFetchMode::StaticOnly,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            preferred_lookup_hosts: &[],
            fallback_sources: &["browser"],
            policy_name: "structured_or_static_source",
            reason: "this domain exposes structured or static content reliably without a browser",
        },
        BrowserSourcePolicy {
            id: "youtube_metadata",
            domains: &["youtube.com", "www.youtube.com", "m.youtube.com", "youtu.be"],
            capability: BrowserSourceCapability::Public,
            fetch_mode: BrowserSourceFetchMode::StaticOnly,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            preferred_lookup_hosts: &["www.youtube.com", "youtube.com", "youtu.be"],
            fallback_sources: &["browser"],
            policy_name: "static_first_video_source",
            reason: "public metadata, watch pages, search pages, and oEmbed can be fetched statically before browser escalation",
        },
    ]
}

fn static_then_browser_sources() -> Vec<BrowserSourcePolicy> {
    vec![BrowserSourcePolicy {
        id: "static_preferred_public_site",
        domains: &[
            "github.com",
            "news.ycombinator.com",
            "reddit.com",
            "www.reddit.com",
            "substack.com",
            "v2ex.com",
            "www.v2ex.com",
            "linux.do",
            "www.linux.do",
        ],
        capability: BrowserSourceCapability::Public,
        fetch_mode: BrowserSourceFetchMode::StaticThenBrowser,
        requires_browser: false,
        requires_auth: false,
        challenge_prone: false,
        preferred_lookup_hosts: &["api.github.com", "raw.githubusercontent.com"],
        fallback_sources: &["browser"],
        policy_name: "static_preferred_public_site",
        reason: "public content is often reachable with static fetch; browser is only a fallback",
    }]
}

fn authenticated_browser_sources() -> Vec<BrowserSourcePolicy> {
    vec![BrowserSourcePolicy {
        id: "authenticated_browser_session_site",
        domains: &[
            "x.com",
            "twitter.com",
            "www.twitter.com",
            "instagram.com",
            "www.instagram.com",
            "tiktok.com",
            "www.tiktok.com",
            "facebook.com",
            "www.facebook.com",
            "linkedin.com",
            "www.linkedin.com",
            "xiaohongshu.com",
            "www.xiaohongshu.com",
            "weibo.com",
            "www.weibo.com",
            "www.zhipin.com",
            "zhipin.com",
            "mooc2-ans.chaoxing.com",
            "www.barchart.com",
            "barchart.com",
            "www.coupang.com",
            "coupang.com",
            "www.ctrip.com",
            "ctrip.com",
            "grok.com",
            "m.okjike.com",
            "web.okjike.com",
            "jimeng.jianying.com",
            "mp.weixin.qq.com",
            "weread.qq.com",
            "xueqiu.com",
            "finance.yahoo.com",
            "yollomi.com",
        ],
        capability: BrowserSourceCapability::Cookie,
        fetch_mode: BrowserSourceFetchMode::BrowserOnly,
        requires_browser: true,
        requires_auth: true,
        challenge_prone: true,
        preferred_lookup_hosts: &[],
        fallback_sources: &["browser"],
        policy_name: "authenticated_browser_session_site",
        reason: "this source commonly needs a real user browser session, cookies, or login-gated UI before content is reliable",
    }]
}

fn semi_public_browser_sources() -> Vec<BrowserSourcePolicy> {
    vec![BrowserSourcePolicy {
        id: "public_but_session_improves_reliability",
        domains: &[
            "bilibili.com",
            "www.bilibili.com",
            "zhihu.com",
            "www.zhihu.com",
            "douban.com",
            "www.douban.com",
            "book.douban.com",
            "movie.douban.com",
            "search.douban.com",
            "medium.com",
            "www.medium.com",
            "smzdm.com",
            "www.smzdm.com",
            "blog.sina.com.cn",
        ],
        capability: BrowserSourceCapability::Browser,
        fetch_mode: BrowserSourceFetchMode::StaticThenBrowser,
        requires_browser: true,
        requires_auth: false,
        challenge_prone: true,
        preferred_lookup_hosts: &[],
        fallback_sources: &["browser"],
        policy_name: "public_but_session_improves_reliability",
        reason: "some public pages are reachable anonymously, but a real browser session is more reliable when static fetch hits bot checks or login walls",
    }]
}

fn browser_only_sources() -> Vec<BrowserSourcePolicy> {
    vec![BrowserSourcePolicy {
        id: "browser_only_protected_site",
        domains: &["www.etsy.com", "etsy.com", "www.booking.com", "booking.com"],
        capability: BrowserSourceCapability::Browser,
        fetch_mode: BrowserSourceFetchMode::BrowserOnly,
        requires_browser: true,
        requires_auth: false,
        challenge_prone: true,
        preferred_lookup_hosts: &[],
        fallback_sources: &["browser"],
        policy_name: "browser_only_protected_site",
        reason: "this site is commonly protected by browser-enforced anti-bot checks",
    }]
}

fn browser_then_static_sources() -> Vec<BrowserSourcePolicy> {
    vec![BrowserSourcePolicy {
        id: "browser_preferred_challenge_prone_site",
        domains: &[
            "thelancet.com",
            "www.thelancet.com",
            "reuters.com",
            "www.reuters.com",
            "bloomberg.com",
            "www.bloomberg.com",
            "google.com",
            "www.google.com",
            "news.google.com",
        ],
        capability: BrowserSourceCapability::Browser,
        fetch_mode: BrowserSourceFetchMode::BrowserThenStatic,
        requires_browser: true,
        requires_auth: false,
        challenge_prone: true,
        preferred_lookup_hosts: &[
            "pubmed.ncbi.nlm.nih.gov",
            "pmc.ncbi.nlm.nih.gov",
            "api.crossref.org",
            "api.openalex.org",
            "doi.org",
        ],
        fallback_sources: &["browser"],
        policy_name: "browser_preferred_challenge_prone_site",
        reason:
            "this site often needs a real browser context before any static fallback is meaningful",
    }]
}

#[cfg(test)]
mod tests {
    use super::{policy_for_host, BrowserSourceFetchMode};

    #[test]
    fn source_policy_matches_subdomains() {
        let policy = policy_for_host("substack.com").expect("substack source policy");
        assert_eq!(policy.fetch_mode, BrowserSourceFetchMode::StaticThenBrowser);

        let subdomain = policy_for_host("writer.substack.com").expect("substack subdomain policy");
        assert_eq!(subdomain.id, policy.id);
    }

    #[test]
    fn source_policy_marks_authenticated_browser_sources() {
        let policy = policy_for_host("x.com").expect("x source policy");
        assert_eq!(policy.fetch_mode, BrowserSourceFetchMode::BrowserOnly);
        assert!(policy.requires_browser);
        assert!(policy.requires_auth);
        assert!(policy.challenge_prone);
    }

    #[test]
    fn source_policy_keeps_academic_record_fallbacks_together() {
        let policy = policy_for_host("www.thelancet.com").expect("lancet source policy");
        assert_eq!(policy.fetch_mode, BrowserSourceFetchMode::BrowserThenStatic);
        assert!(policy.requires_browser);
        assert!(policy.challenge_prone);
        assert!(policy
            .preferred_lookup_hosts
            .contains(&"pubmed.ncbi.nlm.nih.gov"));
        assert!(policy.preferred_lookup_hosts.contains(&"doi.org"));
    }
}
