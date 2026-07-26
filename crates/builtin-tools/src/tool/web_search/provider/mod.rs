//! Search provider dispatch for `web_search`.
//!
//! This module keeps provider selection separate from search planning. The
//! user-visible tool remains `web_search`; providers are an internal execution
//! detail.

use async_trait::async_trait;

use super::orchestrator::{SearchCandidate, SearchOrchestrator, SourceAdapterSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SearchProviderKind {
    Bing,
    DuckDuckGo,
    Sogou,
    So,
    Rss,
    Gutendex,
    Github,
    HackerNews,
    Arxiv,
    Wikipedia,
    BrowserSerp,
}

impl SearchProviderKind {
    pub(crate) fn from_source_name(source_name: &str) -> Self {
        match source_name {
            "bing" => Self::Bing,
            "duckduckgo" => Self::DuckDuckGo,
            "sogou" => Self::Sogou,
            "so" => Self::So,
            "rss" | "feed" | "feed_discovery" => Self::Rss,
            "gutendex" => Self::Gutendex,
            "github" => Self::Github,
            "hackernews" => Self::HackerNews,
            "arxiv" => Self::Arxiv,
            "wikipedia" => Self::Wikipedia,
            "browser" => Self::BrowserSerp,
            _ => Self::BrowserSerp,
        }
    }
}

#[async_trait]
pub(crate) trait SearchProvider {
    fn provider_name(&self) -> &'static str;

    async fn search(
        &self,
        orchestrator: &SearchOrchestrator,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>>;
}

#[async_trait]
impl SearchProvider for SearchProviderKind {
    fn provider_name(&self) -> &'static str {
        match self {
            Self::Bing => "bing",
            Self::DuckDuckGo => "duckduckgo",
            Self::Sogou => "sogou",
            Self::So => "so",
            Self::Rss => "rss",
            Self::Gutendex => "gutendex",
            Self::Github => "github",
            Self::HackerNews => "hackernews",
            Self::Arxiv => "arxiv",
            Self::Wikipedia => "wikipedia",
            Self::BrowserSerp => "browser_serp",
        }
    }

    async fn search(
        &self,
        orchestrator: &SearchOrchestrator,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        match self {
            Self::Bing => orchestrator.search_bing(source, query).await,
            Self::DuckDuckGo => orchestrator.search_duckduckgo(source, query).await,
            Self::Sogou => orchestrator.search_sogou(source, query).await,
            Self::So => orchestrator.search_so(source, query).await,
            Self::Rss => orchestrator.search_rss(source, query).await,
            Self::Gutendex => orchestrator.search_gutendex(source, query).await,
            Self::Github => orchestrator.search_github(source, query).await,
            Self::HackerNews => orchestrator.search_hackernews(source, query).await,
            Self::Arxiv => orchestrator.search_arxiv(source, query).await,
            Self::Wikipedia => orchestrator.search_wikipedia(source, query).await,
            Self::BrowserSerp => {
                super::orchestrator::search_browser(
                    source,
                    query,
                    orchestrator.config().max_results,
                )
                .await
            }
        }
    }
}
