//! Search orchestration for web_search.
//!
//! This layer turns a single query into a small, source-aware search plan,
//! fans out to compatible sources, then returns a unified evidence bundle.

use chrono::Datelike;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::time::timeout;

#[cfg(feature = "browser")]
use crate::tool::browser::BrowserTool;
use crate::tool::web_search::policy::source::{policy_for_host, BrowserSourceFetchMode};
use crate::tool::web_search::policy::{RuntimeSourceAdapterOverride, SearchPolicy};
use crate::tool::web_search::provider::{SearchProvider, SearchProviderKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchIntent {
    Academic,
    Code,
    News,
    Video,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceCapability {
    Public,
    Rss,
    SearchEngine,
    Browser,
    Cookie,
    Header,
    Intercept,
    Ui,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceAdapterSpec {
    pub name: String,
    pub capability: SourceCapability,
    pub requires_browser: bool,
    pub requires_auth: bool,
    pub challenge_prone: bool,
    pub domains: Vec<String>,
    pub fallback_sources: Vec<String>,
    pub weight: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchSubquery {
    pub label: String,
    pub query: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchPlan {
    pub intent: SearchIntent,
    pub freshness: String,
    pub requires_browser: bool,
    pub candidate_sources: Vec<SourceAdapterSpec>,
    pub subqueries: Vec<SearchSubquery>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchCandidate {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String,
    pub capability: SourceCapability,
    pub rank: usize,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceDiagnostic {
    pub source: String,
    pub capability: SourceCapability,
    pub status: String,
    pub message: String,
    pub retry_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceBundle {
    pub kind: String,
    pub query: String,
    pub plan: SearchPlan,
    pub diagnostics: Vec<SourceDiagnostic>,
    pub candidates: Vec<SearchCandidate>,
}

#[derive(Debug, Clone)]
pub struct SearchOrchestratorConfig {
    pub max_results: usize,
    pub source_timeout: Duration,
}

impl Default for SearchOrchestratorConfig {
    fn default() -> Self {
        Self {
            max_results: 5,
            source_timeout: Duration::from_secs(4),
        }
    }
}

pub struct SearchOrchestrator {
    client: reqwest::Client,
    config: SearchOrchestratorConfig,
}

impl SearchOrchestrator {
    pub fn new(config: SearchOrchestratorConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(config.source_timeout.max(Duration::from_secs(8)))
            .user_agent("BenShu/1.0 search-orchestrator")
            .build()?;
        Ok(Self { client, config })
    }

    pub(crate) fn config(&self) -> &SearchOrchestratorConfig {
        &self.config
    }

    pub async fn search(&self, query: &str) -> EvidenceBundle {
        let query = normalize_common_search_typos(query);
        let query = query.as_ref();
        let plan = build_search_plan(query);
        let mut diagnostics = Vec::new();
        for message in SearchPolicy::source_policy_diagnostics_for_task(query) {
            diagnostics.push(SourceDiagnostic {
                source: "artifact_policy".to_string(),
                capability: SourceCapability::Public,
                status: "policy_warning".to_string(),
                message,
                retry_hint: "Fix the worker artifact_policy source_adapters entry in the panel; built-in fallback sources remain active.".to_string(),
            });
        }
        let mut ranked_lists = Vec::new();
        let direct_urls = direct_url_candidates(query);
        if !direct_urls.is_empty() {
            diagnostics.push(SourceDiagnostic {
                source: "direct_url".to_string(),
                capability: SourceCapability::Public,
                status: "ok".to_string(),
                message: format!("{} direct URL candidates", direct_urls.len()),
                retry_hint:
                    "Fetch or browse the explicit URL before falling back to search results."
                        .to_string(),
            });
            ranked_lists.push(direct_urls);
        }

        for subquery in &plan.subqueries {
            let source_futures = subquery.sources.iter().filter_map(|source_name| {
                let source = plan
                    .candidate_sources
                    .iter()
                    .find(|source| source.name == *source_name)?
                    .clone();
                let query = subquery.query.clone();
                Some(async move {
                    let source_timeout =
                        source_timeout_for_query(&source, &query, self.config.source_timeout);
                    let result = timeout(source_timeout, self.search_source(&source, &query))
                        .await
                        .map_err(|_| {
                            anyhow::anyhow!(
                                "{} search timed out after {}s",
                                source.name,
                                source_timeout.as_secs()
                            )
                        })
                        .and_then(|result| result);
                    (source, result)
                })
            });

            for (source, result) in futures::future::join_all(source_futures).await {
                match result {
                    Ok(candidates) => {
                        diagnostics.push(SourceDiagnostic {
                            source: source.name.clone(),
                            capability: source.capability.clone(),
                            status: "ok".to_string(),
                            message: format!("{} candidates", candidates.len()),
                            retry_hint: "none".to_string(),
                        });
                        if !candidates.is_empty() {
                            ranked_lists.push(candidates);
                        }
                    }
                    Err(error) => diagnostics.push(source_failure_diagnostic(&source, &error)),
                }
            }
        }

        if query_requests_feed_discovery(query) {
            self.augment_with_discovered_feeds(query, &plan, &mut diagnostics, &mut ranked_lists)
                .await;
        }

        let mut candidates = fuse_ranked_lists(ranked_lists, &plan, query);
        candidates = filter_candidates_by_source_access_fit(candidates, query, &plan);
        candidates.truncate(self.config.max_results);

        EvidenceBundle {
            kind: "search_evidence_bundle".to_string(),
            query: query.to_string(),
            plan,
            diagnostics,
            candidates,
        }
    }

    async fn search_source(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let provider = SearchProviderKind::from_source_name(source.name.as_str());
        let result = provider.search(self, source, query).await;
        if source.name == "browser"
            || source.name == provider.provider_name()
            || provider != SearchProviderKind::BrowserSerp
        {
            result
        } else {
            result.map_err(|error| {
                anyhow::anyhow!(
                    "source adapter '{}' used browser fallback and failed: {}",
                    source.name,
                    error
                )
            })
        }
    }

    pub(super) async fn search_sogou(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let url = format!(
            "https://www.sogou.com/web?query={}",
            urlencoding::encode(query)
        );
        let html = self
            .client
            .get(url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        Ok(parse_sogou_results(
            &html,
            source,
            self.config.max_results.min(10),
        ))
    }

    pub(super) async fn search_so(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let url = format!("https://www.so.com/s?q={}", urlencoding::encode(query));
        let html = self
            .client
            .get(url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        Ok(parse_so_results(
            &html,
            source,
            self.config.max_results.min(10),
        ))
    }

    pub(super) async fn search_bing(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let url = format!(
            "https://www.bing.com/search?q={}",
            urlencoding::encode(query)
        );
        let html = self
            .client
            .get(url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        #[cfg(feature = "browser")]
        {
            let parsed =
                BrowserTool::parse_bing_search_results(&html, self.config.max_results.min(10));
            let results = BrowserTool::filter_results_for_query(query, parsed.clone());
            let relevant = results
                .into_iter()
                .filter(|result| {
                    search_candidate_matches_query_terms(
                        query,
                        &result.title,
                        &result.url,
                        &result.snippet,
                    )
                })
                .enumerate()
                .map(|(index, result)| SearchCandidate {
                    title: result.title,
                    url: result.url,
                    snippet: result.snippet,
                    source: source.name.clone(),
                    capability: source.capability.clone(),
                    rank: index + 1,
                    score: 0.0,
                })
                .collect::<Vec<_>>();
            if relevant.is_empty() {
                return self.search_bing_rss(source, query).await;
            }
            return Ok(relevant);
        }

        #[cfg(not(feature = "browser"))]
        {
            let parsed = parse_basic_bing_results(&html, source, self.config.max_results.min(10));
            if parsed.is_empty() {
                return self.search_bing_rss(source, query).await;
            }
            Ok(parsed)
        }
    }

    async fn search_bing_rss(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let url = format!(
            "https://www.bing.com/search?q={}&format=rss",
            urlencoding::encode(query)
        );
        let body = self
            .client
            .get(url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let mut candidates = parse_feed_candidates(
            &body,
            "https://www.bing.com/search?format=rss",
            source,
            query,
            self.config.max_results.min(10),
        );
        if candidates.is_empty() {
            candidates = parse_feed_candidates(
                &body,
                "https://www.bing.com/search?format=rss",
                source,
                "",
                self.config.max_results.min(10),
            );
            let identity_terms = mixed_query_ascii_identity_terms(query);
            if !identity_terms.is_empty() {
                candidates = candidates
                    .into_iter()
                    .filter(|candidate| {
                        candidate_matches_any_ascii_identity(candidate, &identity_terms)
                    })
                    .collect();
            }
        }
        let filtered = filter_candidates_by_query(candidates.clone(), query);
        if filtered.is_empty() {
            if query_has_explicit_match_terms(query) {
                return Ok(Vec::new());
            }
            Ok(candidates)
        } else {
            Ok(filtered)
        }
    }

    pub(super) async fn search_duckduckgo(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let url = format!(
            "https://duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );
        let html = self
            .client
            .get(url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        Ok(parse_duckduckgo_results(
            &html,
            source,
            self.config.max_results.min(10),
        ))
    }

    pub(super) async fn search_rss(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let mut feed_urls = Vec::new();
        for url in direct_urls(query) {
            push_unique(&mut feed_urls, url);
        }
        for host in site_filter_hosts(query) {
            for candidate in common_feed_urls_for_host(&host) {
                push_unique(&mut feed_urls, candidate);
            }
        }

        let mut results = Vec::new();
        for feed_url in feed_urls.into_iter().take(8) {
            let Ok(mut candidates) = self
                .fetch_feed_candidates(&feed_url, source, query, self.config.max_results.min(10))
                .await
            else {
                continue;
            };
            results.append(&mut candidates);
            if results.len() >= self.config.max_results {
                break;
            }
        }
        Ok(results)
    }

    pub(super) async fn search_gutendex(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        if !query_allows_cross_language_public_text_catalog(query) {
            return Ok(Vec::new());
        }

        let api_query = public_text_api_query(query);
        let target_count = self.config.max_results.min(10);
        let mut merged = Vec::new();
        let mut seen_urls = HashSet::new();

        for catalog_query in public_text_catalog_queries(&api_query) {
            let url = format!(
                "https://gutendex.com/books/?search={}",
                urlencoding::encode(&catalog_query)
            );
            let body = self
                .client
                .get(url)
                .header("Accept", "application/json")
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;

            let candidates = parse_gutendex_results(&body, source, target_count)?;
            for candidate in candidates {
                if seen_urls.insert(candidate.url.clone()) {
                    merged.push(candidate);
                }
                if merged.len() >= target_count {
                    break;
                }
            }
            if merged.len() >= target_count {
                break;
            }
        }

        Ok(merged)
    }

    pub(super) async fn search_github(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        #[derive(Deserialize)]
        struct GithubOwner {
            login: String,
        }
        #[derive(Deserialize)]
        struct GithubRepo {
            full_name: String,
            html_url: String,
            description: Option<String>,
            stargazers_count: Option<u64>,
            owner: GithubOwner,
        }
        #[derive(Deserialize)]
        struct GithubResponse {
            items: Vec<GithubRepo>,
        }

        let raw_query = query;
        let query = source_api_query(query);
        let url = format!(
            "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page={}",
            urlencoding::encode(&query),
            self.config.max_results.min(10)
        );
        let response: GithubResponse = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let candidates = response
            .items
            .into_iter()
            .enumerate()
            .map(|(index, repo)| SearchCandidate {
                title: repo.full_name,
                url: repo.html_url,
                snippet: format!(
                    "{} stars. Owner: {}. {}",
                    repo.stargazers_count.unwrap_or_default(),
                    repo.owner.login,
                    repo.description.unwrap_or_default()
                ),
                source: source.name.clone(),
                capability: source.capability.clone(),
                rank: index + 1,
                score: 0.0,
            })
            .collect::<Vec<_>>();

        Ok(filter_candidates_by_query(candidates, raw_query))
    }

    pub(super) async fn search_hackernews(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        #[derive(Deserialize)]
        struct HnHit {
            title: Option<String>,
            url: Option<String>,
            #[serde(rename = "objectID")]
            object_id: String,
            points: Option<i64>,
            num_comments: Option<i64>,
        }
        #[derive(Deserialize)]
        struct HnResponse {
            hits: Vec<HnHit>,
        }

        let raw_query = query;
        let query = source_api_query(query);
        let url = format!(
            "https://hn.algolia.com/api/v1/search?query={}&tags=story&hitsPerPage={}",
            urlencoding::encode(&query),
            self.config.max_results.min(10)
        );
        let response: HnResponse = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let candidates = response
            .hits
            .into_iter()
            .enumerate()
            .map(|(index, hit)| {
                let title = hit.title.unwrap_or_else(|| "Hacker News item".to_string());
                let item_url = hit.url.unwrap_or_else(|| {
                    format!("https://news.ycombinator.com/item?id={}", hit.object_id)
                });
                SearchCandidate {
                    title,
                    url: item_url,
                    snippet: format!(
                        "{} points, {} comments",
                        hit.points.unwrap_or_default(),
                        hit.num_comments.unwrap_or_default()
                    ),
                    source: source.name.clone(),
                    capability: source.capability.clone(),
                    rank: index + 1,
                    score: 0.0,
                }
            })
            .collect::<Vec<_>>();

        Ok(filter_candidates_by_project_token(candidates, raw_query))
    }

    pub(super) async fn search_arxiv(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let query = academic_api_query(query);
        let search_query = arxiv_search_query(&query);
        let url = format!(
            "https://export.arxiv.org/api/query?search_query={}&start=0&max_results={}&sortBy=relevance&sortOrder=descending",
            urlencoding::encode(&search_query),
            self.config.max_results.min(10)
        );
        let body = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(parse_arxiv_entries(&body, source))
    }

    pub(super) async fn search_wikipedia(
        &self,
        source: &SourceAdapterSpec,
        query: &str,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        #[derive(Deserialize)]
        struct WikiItem {
            title: String,
            snippet: Option<String>,
            pageid: u64,
        }
        #[derive(Deserialize)]
        struct WikiQuery {
            search: Vec<WikiItem>,
        }
        #[derive(Deserialize)]
        struct WikiResponse {
            query: WikiQuery,
        }

        let url = format!(
            "https://en.wikipedia.org/w/api.php?action=query&list=search&format=json&srlimit={}&srsearch={}",
            self.config.max_results.min(10),
            urlencoding::encode(query)
        );
        let response: WikiResponse = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(response
            .query
            .search
            .into_iter()
            .enumerate()
            .map(|(index, item)| SearchCandidate {
                title: item.title,
                url: format!("https://en.wikipedia.org/?curid={}", item.pageid),
                snippet: strip_html(&item.snippet.unwrap_or_default()),
                source: source.name.clone(),
                capability: source.capability.clone(),
                rank: index + 1,
                score: 0.0,
            })
            .collect())
    }

    async fn augment_with_discovered_feeds(
        &self,
        query: &str,
        plan: &SearchPlan,
        diagnostics: &mut Vec<SourceDiagnostic>,
        ranked_lists: &mut Vec<Vec<SearchCandidate>>,
    ) {
        let rss_source = plan
            .candidate_sources
            .iter()
            .find(|source| source.name == "rss")
            .cloned()
            .unwrap_or_else(|| source("rss", 100));

        let seed_urls = ranked_lists
            .iter()
            .flat_map(|list| list.iter())
            .filter(|candidate| !candidate.url.trim().is_empty())
            .take(8)
            .map(|candidate| candidate.url.clone())
            .collect::<Vec<_>>();

        if seed_urls.is_empty() {
            return;
        }

        let mut discovered_feeds = Vec::new();
        let mut seen_hosts = HashSet::new();
        for seed_url in seed_urls {
            let Ok(parsed) = Url::parse(&seed_url) else {
                continue;
            };
            let Some(host) = parsed.host_str().map(|host| host.to_ascii_lowercase()) else {
                continue;
            };
            if !seen_hosts.insert(host.clone()) || seen_hosts.len() > 4 {
                continue;
            }

            for common in common_feed_urls_for_host(&host) {
                push_unique(&mut discovered_feeds, common);
            }

            if let Ok(response) = timeout(
                Duration::from_secs(4),
                self.client
                    .get(parsed.clone())
                    .header("Accept", "text/html,application/xhtml+xml,*/*")
                    .send(),
            )
            .await
            {
                if let Ok(response) = response.and_then(|response| response.error_for_status()) {
                    if let Ok(html) = timeout(Duration::from_secs(4), response.text()).await {
                        if let Ok(html) = html {
                            for feed_url in discover_feed_urls_from_html(&parsed, &html) {
                                push_unique(&mut discovered_feeds, feed_url);
                            }
                        }
                    }
                }
            }

            if discovered_feeds.len() >= 12 {
                break;
            }
        }

        let mut feed_candidates = Vec::new();
        for feed_url in discovered_feeds.into_iter().take(12) {
            let Ok(mut candidates) = self
                .fetch_feed_candidates(
                    &feed_url,
                    &rss_source,
                    query,
                    self.config.max_results.min(10),
                )
                .await
            else {
                continue;
            };
            feed_candidates.append(&mut candidates);
            if feed_candidates.len() >= self.config.max_results {
                break;
            }
        }

        diagnostics.push(SourceDiagnostic {
            source: "rss".to_string(),
            capability: SourceCapability::Rss,
            status: if feed_candidates.is_empty() {
                "no_feed_candidates".to_string()
            } else {
                "ok".to_string()
            },
            message: format!("{} feed candidates from auto-discovered public feeds", feed_candidates.len()),
            retry_hint: if feed_candidates.is_empty() {
                "Continue with search engine and browser candidates; no user RSS configuration is required.".to_string()
            } else {
                "Use feed candidates as stable recent-source hints, then fetch the concrete article URL when needed.".to_string()
            },
        });

        if !feed_candidates.is_empty() {
            ranked_lists.push(feed_candidates);
        }
    }

    async fn fetch_feed_candidates(
        &self,
        feed_url: &str,
        source: &SourceAdapterSpec,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let response = timeout(
            Duration::from_secs(5),
            self.client
                .get(feed_url)
                .header(
                    "Accept",
                    "application/rss+xml,application/atom+xml,application/xml,text/xml,*/*",
                )
                .send(),
        )
        .await??;
        let body = timeout(Duration::from_secs(5), response.error_for_status()?.text()).await??;
        let mut candidates = parse_feed_candidates(&body, feed_url, source, query, limit);
        candidates = filter_candidates_by_query(candidates, query);
        Ok(candidates)
    }
}

pub fn build_search_plan(query: &str) -> SearchPlan {
    let intent = infer_intent(query);
    let freshness = infer_freshness(query);
    let mut candidate_sources = candidate_sources_for_intent(&intent);
    for override_spec in SearchPolicy::source_adapter_overrides_for_task(query) {
        apply_source_adapter_override(&mut candidate_sources, override_spec);
    }
    for source_name in SearchPolicy::source_adapter_names_for_task(query) {
        if !candidate_sources
            .iter()
            .any(|source| source.name.eq_ignore_ascii_case(&source_name))
        {
            candidate_sources.push(source(&source_name, 90));
        }
    }
    if query_requests_public_text_catalog(query)
        && query_allows_cross_language_public_text_catalog(query)
        && !candidate_sources
            .iter()
            .any(|source| source.name == "gutendex")
    {
        candidate_sources.insert(0, source("gutendex", 140));
    }
    if contains_cjk(query)
        && !candidate_sources
            .iter()
            .any(|source| source.name == "sogou")
    {
        let insert_at = candidate_sources
            .iter()
            .position(|source| source.name == "bing")
            .map(|index| index + 1)
            .unwrap_or(0);
        candidate_sources.insert(insert_at, source("sogou", 95));
    }
    if contains_cjk(query) && !candidate_sources.iter().any(|source| source.name == "so") {
        let insert_at = candidate_sources
            .iter()
            .position(|source| source.name == "sogou")
            .map(|index| index + 1)
            .or_else(|| {
                candidate_sources
                    .iter()
                    .position(|source| source.name == "bing")
                    .map(|index| index + 1)
            })
            .unwrap_or(0);
        candidate_sources.insert(insert_at, source("so", 92));
    }
    if query_requests_time_sensitive_lookup(query) {
        remove_static_background_sources_for_live_lookup(&mut candidate_sources);
        upsert_source(&mut candidate_sources, source("rss", 108));
    }
    if query_requests_browser_search_surface(query) {
        upsert_source(&mut candidate_sources, source("browser", 115));
    } else {
        // Keep the browser-backed SERP provider as a normal fallback. Search
        // engine HTML/API shapes are volatile, especially for CJK and recent
        // news queries; the provider timeout keeps this bounded for chat turns.
        upsert_source(&mut candidate_sources, source("browser", 70));
    }
    let requires_browser = candidate_sources
        .iter()
        .any(|source| source.requires_browser);
    let primary_query = normalize_public_search_query(query);
    let mut subqueries = vec![SearchSubquery {
        label: "primary".to_string(),
        query: primary_query.clone(),
        sources: candidate_sources
            .iter()
            .map(|source| source.name.clone())
            .collect(),
    }];
    if let Some(english_query) = public_text_english_search_query(query) {
        if english_query != primary_query {
            subqueries.push(SearchSubquery {
                label: "public_text_bilingual".to_string(),
                query: english_query,
                sources: candidate_sources
                    .iter()
                    .filter(|source| !source.requires_auth)
                    .map(|source| source.name.clone())
                    .collect(),
            });
        }
    }
    if let Some(document_query) = document_english_search_query(query) {
        if document_query != primary_query
            && !subqueries
                .iter()
                .any(|subquery| subquery.query.eq_ignore_ascii_case(&document_query))
        {
            subqueries.push(SearchSubquery {
                label: "document_bilingual".to_string(),
                query: document_query,
                sources: candidate_sources
                    .iter()
                    .filter(|source| !source.requires_auth)
                    .map(|source| source.name.clone())
                    .collect(),
            });
        }
    }

    SearchPlan {
        intent,
        freshness,
        requires_browser,
        candidate_sources,
        subqueries,
    }
}

fn source_timeout_for_query(
    source: &SourceAdapterSpec,
    query: &str,
    default_timeout: Duration,
) -> Duration {
    if source.name == "gutendex" && query_requests_public_text_catalog(query) {
        return default_timeout.max(Duration::from_secs(20));
    }
    default_timeout
}

fn query_requests_browser_search_surface(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    contains_any(
        &lowered,
        &[
            "browser",
            "chrome",
            "edge",
            "rendered page",
            "dynamic page",
            "javascript page",
            "login session",
            "interactive page",
        ],
    ) || contains_any(
        query,
        &[
            "浏览器",
            "真实浏览器",
            "打开网页",
            "动态页面",
            "渲染页面",
            "需要登录",
            "登录态",
            "交互页面",
        ],
    )
}

fn apply_source_adapter_override(
    candidate_sources: &mut Vec<SourceAdapterSpec>,
    override_spec: RuntimeSourceAdapterOverride,
) {
    if override_spec.name.trim().is_empty() {
        return;
    }
    let position = candidate_sources
        .iter()
        .position(|source| source.name.eq_ignore_ascii_case(&override_spec.name));
    let mut source = position
        .and_then(|index| candidate_sources.get(index).cloned())
        .unwrap_or_else(|| source(&override_spec.name, override_spec.weight.unwrap_or(90)));

    if let Some(capability) = override_spec.capability {
        source.capability = capability;
    }
    if let Some(requires_browser) = override_spec.requires_browser {
        source.requires_browser = requires_browser;
    }
    if let Some(requires_auth) = override_spec.requires_auth {
        source.requires_auth = requires_auth;
    }
    if let Some(challenge_prone) = override_spec.challenge_prone {
        source.challenge_prone = challenge_prone;
    }
    if let Some(domains) = override_spec.domains {
        source.domains = domains;
    }
    if let Some(fallback_sources) = override_spec.fallback_sources {
        source.fallback_sources = fallback_sources;
    }
    if let Some(weight) = override_spec.weight {
        source.weight = weight;
    }

    if let Some(index) = position {
        candidate_sources[index] = source;
    } else {
        candidate_sources.push(source);
    }
}

fn remove_static_background_sources_for_live_lookup(
    candidate_sources: &mut Vec<SourceAdapterSpec>,
) {
    candidate_sources.retain(|source| {
        !matches!(
            source.name.as_str(),
            "wikipedia" | "baidu_baike" | "wikidata"
        )
    });
}

fn upsert_source(candidate_sources: &mut Vec<SourceAdapterSpec>, source: SourceAdapterSpec) {
    if let Some(index) = candidate_sources
        .iter()
        .position(|candidate| candidate.name.eq_ignore_ascii_case(&source.name))
    {
        candidate_sources[index] = source;
    } else {
        candidate_sources.push(source);
    }
}

fn infer_intent(query: &str) -> SearchIntent {
    let lowered = query.to_ascii_lowercase();
    if contains_any(&lowered, &["youtube", "video", "bilibili", "视频"]) {
        SearchIntent::Video
    } else if contains_any(
        &lowered,
        &[
            "pubmed", "arxiv", "paper", "doi", "clinical", "trial", "lancet", "论文",
        ],
    ) {
        SearchIntent::Academic
    } else if contains_any(
        &lowered,
        &["github", "crate", "repo", "repository", "代码", "开源"],
    ) {
        SearchIntent::Code
    } else if contains_any(&lowered, &["news", "headline", "headlines", "讨论"])
        || contains_any(query, &["新闻", "资讯", "讨论"])
    {
        SearchIntent::News
    } else if looks_like_code_or_project_query(&lowered) {
        SearchIntent::Code
    } else {
        SearchIntent::General
    }
}

fn looks_like_code_or_project_query(query: &str) -> bool {
    let has_project_marker = contains_any(
        query,
        &[
            "library",
            "package",
            "sdk",
            "cli",
            "tool",
            "framework",
            "plugin",
            "extension",
            "agent",
            "项目",
            "库",
            "工具",
        ],
    );
    if !has_project_marker {
        return false;
    }

    query
        .split_whitespace()
        .any(|token| is_ascii_project_token(token))
}

fn is_ascii_project_token(token: &str) -> bool {
    let token =
        token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_');
    token.len() >= 4
        && token
            .chars()
            .any(|ch| ch == '-' || ch == '_' || ch.is_ascii_digit())
        && token.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn infer_freshness(query: &str) -> String {
    let lowered = query.to_ascii_lowercase();
    if contains_any(
        &lowered,
        &[
            "latest",
            "today",
            "current",
            "now",
            "live",
            "forecast",
            "this week",
            "最近",
            "最新",
            "今天",
            "当前",
            "现在",
            "实时",
            "天气",
            "预报",
        ],
    ) {
        "strict_recent".to_string()
    } else {
        "balanced".to_string()
    }
}

fn query_requests_time_sensitive_lookup(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    contains_any(
        &lowered,
        &[
            "current",
            "today",
            "now",
            "live",
            "real-time",
            "realtime",
            "latest",
            "forecast",
            "weather",
            "temperature",
        ],
    ) || contains_any(
        query,
        &[
            "今天", "今日", "当前", "现在", "实时", "最新", "天气", "预报", "气温", "温度", "降雨",
            "下雨",
        ],
    )
}

fn query_requests_public_text_catalog(query: &str) -> bool {
    if query.contains("白皮书") {
        return false;
    }
    let lowered = query.to_ascii_lowercase();
    contains_any(
        &lowered,
        &[
            "book",
            "books",
            "ebook",
            "ebooks",
            "fiction",
            "novel",
            "novels",
            "full text",
            "plain text",
            "download",
            "downloadable",
            "public domain",
            "gutenberg",
        ],
    ) || contains_any(
        query,
        &[
            "书",
            "书籍",
            "小说",
            "网文",
            "电子书",
            "全文",
            "正文",
            "下载",
            "可下载",
        ],
    )
}

fn normalize_common_search_typos(query: &str) -> Cow<'_, str> {
    let mut changed = false;
    let mut normalized = Vec::new();
    for token in query.split_whitespace() {
        let trimmed = token.trim_matches(|ch: char| {
            matches!(
                ch,
                ',' | '.' | ';' | ':' | '，' | '。' | '；' | '：' | '!' | '！' | '?' | '？'
            )
        });
        if trimmed.eq_ignore_ascii_case("pdt") {
            normalized.push(token.replacen(trimmed, "pdf", 1));
            changed = true;
        } else if let Some(prefix) = trimmed
            .strip_suffix("pdt")
            .filter(|prefix| !prefix.is_empty() && !prefix.ends_with("http"))
        {
            normalized.push(token.replacen(trimmed, &format!("{prefix} pdf"), 1));
            changed = true;
        } else {
            normalized.push(token.to_string());
        }
    }

    if changed {
        Cow::Owned(normalized.join(" "))
    } else {
        Cow::Borrowed(query)
    }
}

fn query_allows_cross_language_public_text_catalog(query: &str) -> bool {
    if !contains_cjk(query) {
        return true;
    }

    let lowered = query.to_ascii_lowercase();
    if contains_any(
        &lowered,
        &["english", "public domain", "gutenberg", "project gutenberg"],
    ) || contains_any(query, &["英文", "英语", "外文", "公版", "古腾堡"])
    {
        return true;
    }

    meaningful_ascii_query_terms(query).into_iter().any(|term| {
        matches!(
            term.as_str(),
            "fantasy"
                | "xuanhuan"
                | "wuxia"
                | "xianxia"
                | "romance"
                | "mystery"
                | "science"
                | "fiction"
                | "sci-fi"
        )
    })
}

fn candidate_sources_for_intent(intent: &SearchIntent) -> Vec<SourceAdapterSpec> {
    match intent {
        SearchIntent::Academic => vec![
            source("arxiv", 100),
            source("bing", 70),
            source("wikipedia", 40),
            source("browser", 60),
        ],
        SearchIntent::Code => vec![
            source("github", 100),
            source("hackernews", 50),
            source("bing", 55),
            source("browser", 60),
        ],
        SearchIntent::News => vec![
            source("bing", 100),
            source("duckduckgo", 90),
            source("rss", 85),
            source("browser", 80),
        ],
        SearchIntent::Video => vec![
            source("bing", 100),
            source("duckduckgo", 90),
            source("browser", 80),
            source("wikipedia", 20),
        ],
        SearchIntent::General => vec![
            source("bing", 100),
            source("duckduckgo", 90),
            source("wikipedia", 35),
            source("browser", 70),
        ],
    }
}

pub fn registered_source_adapters() -> Vec<SourceAdapterSpec> {
    vec![
        source("bing", 100),
        source("duckduckgo", 100),
        source("sogou", 100),
        source("so", 100),
        source("rss", 100),
        source("gutendex", 100),
        source("github", 100),
        source("hackernews", 100),
        source("arxiv", 100),
        source("wikipedia", 100),
        source("browser", 100),
        source("reddit", 100),
        source("youtube", 100),
        source("twitter", 100),
        source("instagram", 100),
        source("tiktok", 100),
        source("xiaohongshu", 100),
        source("apple_podcasts", 100),
        source("bbc", 100),
        source("bloomberg_feeds", 100),
        source("devto", 100),
        source("stackoverflow", 100),
        source("huggingface", 100),
        source("lobsters", 100),
        source("v2ex", 100),
        source("bilibili", 100),
        source("zhihu", 100),
        source("douban", 100),
        source("medium", 100),
        source("substack", 100),
        source("reuters", 100),
        source("bloomberg", 100),
        source("google", 100),
        source("weibo", 100),
        source("barchart", 100),
        source("boss", 100),
        source("ctrip", 100),
        source("coupang", 100),
        source("linkedin", 100),
        source("weixin", 100),
        source("xueqiu", 100),
        source("sinafinance", 100),
        source("steam", 100),
        source("xiaoyuzhou", 100),
        source("google_suggest", 100),
        source("google_trends", 100),
        source("linux_do", 100),
        source("sinablog", 100),
        source("chaoxing", 100),
        source("grok", 100),
        source("jike", 100),
        source("jimeng", 100),
        source("smzdm", 100),
        source("weread", 100),
        source("yahoo_finance", 100),
        source("yollomi", 100),
    ]
}

fn source(name: &str, weight: u32) -> SourceAdapterSpec {
    let mut spec = source_profile(name);
    spec.weight = weight;
    spec
}

fn source_profile(name: &str) -> SourceAdapterSpec {
    match name {
        "bing" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::SearchEngine,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: strings(&["www.bing.com", "bing.com"]),
            fallback_sources: strings(&["browser"]),
            weight: 100,
        },
        "duckduckgo" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::SearchEngine,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: strings(&["duckduckgo.com", "html.duckduckgo.com"]),
            fallback_sources: strings(&["bing", "browser"]),
            weight: 100,
        },
        "sogou" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::SearchEngine,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: strings(&["www.sogou.com", "sogou.com"]),
            fallback_sources: strings(&["browser"]),
            weight: 100,
        },
        "so" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::SearchEngine,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: strings(&["www.so.com", "so.com"]),
            fallback_sources: strings(&["sogou", "bing", "browser"]),
            weight: 100,
        },
        "gutendex" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::Public,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: strings(&["gutendex.com", "www.gutenberg.org"]),
            fallback_sources: strings(&["wikipedia", "browser"]),
            weight: 100,
        },
        "github" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::Public,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: strings(&["api.github.com", "github.com"]),
            fallback_sources: strings(&["browser"]),
            weight: 100,
        },
        "hackernews" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::Public,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: strings(&["hn.algolia.com", "news.ycombinator.com"]),
            fallback_sources: strings(&["browser"]),
            weight: 100,
        },
        "arxiv" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::Public,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: strings(&["export.arxiv.org", "arxiv.org"]),
            fallback_sources: strings(&["browser"]),
            weight: 100,
        },
        "wikipedia" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::Public,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: strings(&["en.wikipedia.org", "wikipedia.org"]),
            fallback_sources: strings(&["browser"]),
            weight: 100,
        },
        "rss" | "feed" | "feed_discovery" => SourceAdapterSpec {
            name: "rss".to_string(),
            capability: SourceCapability::Rss,
            requires_browser: false,
            requires_auth: false,
            challenge_prone: false,
            domains: vec![],
            fallback_sources: strings(&["bing", "duckduckgo", "browser"]),
            weight: 100,
        },
        "apple_podcasts" => public_profile(
            name,
            &["itunes.apple.com", "rss.marketingtools.apple.com"],
            &["browser"],
        ),
        "bbc" => public_profile(name, &["feeds.bbci.co.uk"], &["browser"]),
        "bloomberg_feeds" => public_profile(name, &["feeds.bloomberg.com"], &["browser"]),
        "devto" => public_profile(name, &["dev.to"], &["browser"]),
        "stackoverflow" => public_profile(
            name,
            &["stackoverflow.com", "api.stackexchange.com"],
            &["browser"],
        ),
        "huggingface" => public_profile(name, &["huggingface.co"], &["browser"]),
        "lobsters" => public_profile(name, &["lobste.rs"], &["browser"]),
        "v2ex" => semi_public_profile(name, &["www.v2ex.com"], &["browser"]),
        "reddit" => auth_browser_profile(name, &["reddit.com", "www.reddit.com"]),
        "youtube" => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::Browser,
            requires_browser: true,
            requires_auth: false,
            challenge_prone: true,
            domains: strings(&["youtube.com", "www.youtube.com", "youtu.be"]),
            fallback_sources: strings(&["browser"]),
            weight: 100,
        },
        "twitter" => auth_browser_profile(name, &["x.com", "twitter.com"]),
        "instagram" => auth_browser_profile(name, &["instagram.com", "www.instagram.com"]),
        "tiktok" => auth_browser_profile(name, &["tiktok.com", "www.tiktok.com"]),
        "xiaohongshu" => auth_browser_profile(name, &["xiaohongshu.com", "www.xiaohongshu.com"]),
        "bilibili" => {
            semi_public_profile(name, &["bilibili.com", "www.bilibili.com"], &["browser"])
        }
        "zhihu" => semi_public_profile(name, &["zhihu.com", "www.zhihu.com"], &["browser"]),
        "douban" => semi_public_profile(
            name,
            &[
                "douban.com",
                "book.douban.com",
                "movie.douban.com",
                "search.douban.com",
            ],
            &["browser"],
        ),
        "medium" => semi_public_profile(name, &["medium.com", "www.medium.com"], &["browser"]),
        "substack" => semi_public_profile(name, &["substack.com"], &["browser"]),
        "reuters" => challenge_browser_profile(name, &["reuters.com", "www.reuters.com"]),
        "bloomberg" => challenge_browser_profile(name, &["bloomberg.com", "www.bloomberg.com"]),
        "google" => {
            challenge_browser_profile(name, &["google.com", "www.google.com", "news.google.com"])
        }
        "weibo" => auth_browser_profile(name, &["weibo.com", "www.weibo.com"]),
        "barchart" => auth_browser_profile(name, &["barchart.com", "www.barchart.com"]),
        "boss" => auth_browser_profile(name, &["zhipin.com", "www.zhipin.com"]),
        "ctrip" => auth_browser_profile(name, &["ctrip.com", "www.ctrip.com"]),
        "coupang" => auth_browser_profile(name, &["coupang.com", "www.coupang.com"]),
        "linkedin" => auth_browser_profile(name, &["linkedin.com", "www.linkedin.com"]),
        "weixin" => auth_browser_profile(name, &["mp.weixin.qq.com"]),
        "xueqiu" => auth_browser_profile(name, &["xueqiu.com"]),
        "sinafinance" => public_profile(name, &["app.cj.sina.com.cn"], &["browser"]),
        "steam" => public_profile(name, &["store.steampowered.com"], &["browser"]),
        "xiaoyuzhou" => public_profile(name, &["www.xiaoyuzhoufm.com"], &["browser"]),
        "google_suggest" => public_profile(name, &["suggestqueries.google.com"], &["browser"]),
        "google_trends" => public_profile(name, &["trends.google.com"], &["browser"]),
        "linux_do" => semi_public_profile(name, &["linux.do", "www.linux.do"], &["browser"]),
        "sinablog" => semi_public_profile(name, &["blog.sina.com.cn"], &["browser"]),
        "chaoxing" => auth_browser_profile(name, &["mooc2-ans.chaoxing.com"]),
        "grok" => auth_browser_profile(name, &["grok.com"]),
        "jike" => auth_browser_profile(name, &["m.okjike.com", "web.okjike.com"]),
        "jimeng" => auth_browser_profile(name, &["jimeng.jianying.com"]),
        "smzdm" => semi_public_profile(name, &["smzdm.com", "www.smzdm.com"], &["browser"]),
        "weread" => auth_browser_profile(name, &["weread.qq.com"]),
        "yahoo_finance" => auth_browser_profile(name, &["finance.yahoo.com"]),
        "yollomi" => auth_browser_profile(name, &["yollomi.com"]),
        "browser" | _ => SourceAdapterSpec {
            name: name.to_string(),
            capability: SourceCapability::Browser,
            requires_browser: true,
            requires_auth: false,
            challenge_prone: true,
            domains: vec![],
            fallback_sources: vec![],
            weight: 100,
        },
    }
}

fn public_profile(name: &str, domains: &[&str], fallback_sources: &[&str]) -> SourceAdapterSpec {
    SourceAdapterSpec {
        name: name.to_string(),
        capability: SourceCapability::Public,
        requires_browser: false,
        requires_auth: false,
        challenge_prone: false,
        domains: strings(domains),
        fallback_sources: strings(fallback_sources),
        weight: 100,
    }
}

fn semi_public_profile(
    name: &str,
    domains: &[&str],
    fallback_sources: &[&str],
) -> SourceAdapterSpec {
    SourceAdapterSpec {
        name: name.to_string(),
        capability: SourceCapability::Browser,
        requires_browser: false,
        requires_auth: false,
        challenge_prone: true,
        domains: strings(domains),
        fallback_sources: strings(fallback_sources),
        weight: 100,
    }
}

fn auth_browser_profile(name: &str, domains: &[&str]) -> SourceAdapterSpec {
    SourceAdapterSpec {
        name: name.to_string(),
        capability: SourceCapability::Cookie,
        requires_browser: true,
        requires_auth: true,
        challenge_prone: true,
        domains: strings(domains),
        fallback_sources: strings(&["browser"]),
        weight: 100,
    }
}

fn challenge_browser_profile(name: &str, domains: &[&str]) -> SourceAdapterSpec {
    SourceAdapterSpec {
        name: name.to_string(),
        capability: SourceCapability::Browser,
        requires_browser: true,
        requires_auth: false,
        challenge_prone: true,
        domains: strings(domains),
        fallback_sources: strings(&["browser"]),
        weight: 100,
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn source_failure_diagnostic(
    source: &SourceAdapterSpec,
    error: &anyhow::Error,
) -> SourceDiagnostic {
    let message = error.to_string();
    let lowered = message.to_ascii_lowercase();
    let (status, retry_hint) = if lowered.contains("challenge")
        || lowered.contains("anti-bot")
        || lowered.contains("verify you are human")
        || lowered.contains("unusual traffic")
    {
        (
            "challenge",
            "Use a real browser session, try a structured public source first, or ask the user to complete verification in Edge/Chrome.",
        )
    } else if lowered.contains("login")
        || lowered.contains("sign in")
        || lowered.contains("authentication")
        || source.requires_auth
    {
        (
            "login_required",
            "Use the user's authenticated Edge/Chrome profile or configure an approved session for this source.",
        )
    } else if lowered.contains("no supported browser")
        || lowered.contains("browser")
            && (lowered.contains("not found") || lowered.contains("missing"))
    {
        (
            "browser_unavailable",
            "Install Microsoft Edge or Google Chrome, or set BENSHU_BROWSER_PATH.",
        )
    } else {
        (
            "failed",
            "Source failed; continue with remaining sources and preserve diagnostics for the researcher.",
        )
    };

    SourceDiagnostic {
        source: source.name.clone(),
        capability: source.capability.clone(),
        status: status.to_string(),
        message,
        retry_hint: retry_hint.to_string(),
    }
}

fn fuse_ranked_lists(
    ranked_lists: Vec<Vec<SearchCandidate>>,
    plan: &SearchPlan,
    query: &str,
) -> Vec<SearchCandidate> {
    let source_weights: HashMap<&str, f32> = plan
        .candidate_sources
        .iter()
        .map(|source| (source.name.as_str(), source.weight as f32 / 100.0))
        .collect();
    let mut by_key: HashMap<String, SearchCandidate> = HashMap::new();
    let mut seen_by_source: HashSet<(String, String)> = HashSet::new();

    for list in ranked_lists {
        for candidate in list {
            let key = candidate_key(&candidate);
            let source_key = (candidate.source.clone(), key.clone());
            if !seen_by_source.insert(source_key) {
                continue;
            }
            let weight = source_weights
                .get(candidate.source.as_str())
                .copied()
                .unwrap_or(1.0);
            let rrf = weight / (60.0 + candidate.rank as f32);
            by_key
                .entry(key)
                .and_modify(|existing| existing.score += rrf)
                .or_insert_with(|| SearchCandidate {
                    score: rrf,
                    ..candidate
                });
        }
    }

    let mut candidates: Vec<SearchCandidate> = by_key.into_values().collect();
    apply_candidate_quality_adjustments(&mut candidates, query);
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.rank.cmp(&b.rank))
    });
    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate.rank = index + 1;
    }
    candidates
}

fn apply_candidate_quality_adjustments(candidates: &mut [SearchCandidate], query: &str) {
    let lowered_query = query.to_ascii_lowercase();
    let time_sensitive = query_mentions_recent_or_current(query, &lowered_query);
    let asks_download = query_requests_download_or_install(query, &lowered_query);
    let document_lookup = query_requests_document_lookup(query);
    let asks_wechat = query.contains("微信")
        || query.contains("公众号")
        || lowered_query.contains("wechat")
        || lowered_query.contains("weixin");
    let asks_official = query.contains("官网")
        || query.contains("官方")
        || lowered_query.contains("official")
        || lowered_query.contains("primary source");
    let asks_programming_language = query.contains("编程")
        || query.contains("语言")
        || lowered_query.contains("programming")
        || lowered_query.contains("language");
    let mut ascii_terms = meaningful_ascii_query_terms(query);
    for term in document_bilingual_terms(query) {
        if term.is_ascii() {
            push_unique(&mut ascii_terms, term);
        }
    }
    let cjk_terms = meaningful_cjk_query_terms(query);
    let current_year = chrono::Utc::now().date_naive().year();

    for candidate in candidates {
        let host = Url::parse(&candidate.url)
            .ok()
            .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
            .unwrap_or_default();
        let url_l = candidate.url.to_ascii_lowercase();
        let title_l = candidate.title.to_ascii_lowercase();
        let snippet_l = candidate.snippet.to_ascii_lowercase();
        let haystack_l = format!("{host} {url_l} {title_l} {snippet_l}");
        let haystack = format!(
            "{} {} {}",
            candidate.url, candidate.title, candidate.snippet
        );

        if !asks_wechat && (host == "mp.weixin.qq.com" || host.ends_with(".weixin.qq.com")) {
            candidate.score *= 0.18;
        } else if !asks_wechat && (host.ends_with(".qq.com") || host == "qq.com") {
            candidate.score *= 0.55;
        }
        if document_lookup
            && !asks_wechat
            && (host == "mp.weixin.qq.com"
                || host.ends_with(".weixin.qq.com")
                || host.ends_with(".qq.com")
                || host == "qq.com")
        {
            candidate.score *= 0.2;
        }

        if !asks_download
            && [
                "download",
                "apk",
                "crack",
                "onlinedown",
                "soft",
                "软件下载",
                "免费下载",
                "破解版",
                "绿色版",
            ]
            .iter()
            .any(|term| haystack_l.contains(term))
        {
            candidate.score *= 0.35;
        }
        if asks_programming_language
            && ["steam", "facepunch", "game", "survive", "survival", "游戏"]
                .iter()
                .any(|term| haystack_l.contains(term))
        {
            candidate.score *= 0.18;
        }
        if !query_asks_for_search_engine(query)
            && matches!(
                host.as_str(),
                "www.baidu.com"
                    | "baidu.com"
                    | "www.google.com"
                    | "google.com"
                    | "www.google.com.hk"
                    | "google.com.hk"
                    | "www.bing.com"
                    | "bing.com"
            )
        {
            candidate.score *= 0.05;
        }

        if time_sensitive && candidate_mentions_stale_year(&haystack_l, current_year) {
            candidate.score *= 0.32;
        }

        let ascii_hits = ascii_terms
            .iter()
            .filter(|term| haystack_l.contains(term.as_str()))
            .count();
        let cjk_hits = cjk_terms
            .iter()
            .filter(|term| haystack.contains(term.as_str()))
            .count();
        if ascii_hits > 0 || cjk_hits > 0 {
            candidate.score *= 1.0 + ((ascii_hits + cjk_hits) as f32 * 0.18).min(0.9);
        }

        let host_identity_hits = ascii_terms
            .iter()
            .filter(|term| host.contains(term.as_str()))
            .count();
        if host_identity_hits > 0 {
            candidate.score *= 1.8;
        }
        if asks_official && (host_identity_hits > 0 || cjk_hits > 0) {
            candidate.score *= 1.7;
        }
        if document_lookup {
            let asks_pdf = lowered_query.contains("pdf") || lowered_query.contains(".pdf");
            if asks_pdf && (url_l.contains(".pdf") || title_l.contains("pdf")) {
                candidate.score *= 2.2;
            }
            if query.contains("白皮书")
                || lowered_query.contains("whitepaper")
                || lowered_query.contains("white paper")
            {
                if haystack_l.contains("whitepaper")
                    || haystack_l.contains("white paper")
                    || haystack.contains("白皮书")
                {
                    candidate.score *= 1.8;
                }
            }
        }
        if host.ends_with(".edu.cn")
            || host.ends_with(".edu")
            || host.ends_with(".gov")
            || host.ends_with(".gov.cn")
        {
            candidate.score *= 1.35;
        }
    }
}

fn query_mentions_recent_or_current(query: &str, lowered_query: &str) -> bool {
    query.contains("最近")
        || query.contains("最新")
        || query.contains("今天")
        || query.contains("当前")
        || query.contains("现在")
        || lowered_query.contains("latest")
        || lowered_query.contains("recent")
        || lowered_query.contains("today")
        || lowered_query.contains("current")
        || lowered_query.contains("stable version")
        || lowered_query.contains("release version")
}

fn query_requests_download_or_install(query: &str, lowered_query: &str) -> bool {
    query.contains("下载")
        || query.contains("安装")
        || lowered_query.contains("download")
        || lowered_query.contains("install")
}

fn candidate_mentions_stale_year(haystack: &str, current_year: i32) -> bool {
    if haystack.contains(&current_year.to_string()) {
        return false;
    }
    let start = (current_year - 8).max(1990);
    (start..current_year).any(|year| {
        let year = year.to_string();
        haystack.contains(&year)
    })
}

fn filter_candidates_by_source_access_fit(
    candidates: Vec<SearchCandidate>,
    query: &str,
    _plan: &SearchPlan,
) -> Vec<SearchCandidate> {
    if candidates.is_empty() || !query_requests_source_body_or_material(query) {
        return candidates;
    }

    let mut directly_readable = Vec::new();
    let mut session_bound = Vec::new();
    for mut candidate in candidates {
        if candidate_requires_user_browser_session(&candidate) {
            candidate.score *= 0.35;
            session_bound.push(candidate);
        } else {
            directly_readable.push(candidate);
        }
    }

    if directly_readable.is_empty() {
        return Vec::new();
    }

    directly_readable.extend(session_bound);
    for (index, candidate) in directly_readable.iter_mut().enumerate() {
        candidate.rank = index + 1;
    }
    directly_readable
}

fn candidate_requires_user_browser_session(candidate: &SearchCandidate) -> bool {
    if candidate.requires_session_like_source() {
        return true;
    }
    let Some(host) = Url::parse(&candidate.url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
    else {
        return false;
    };
    policy_for_host(&host).is_some_and(|policy| {
        policy.requires_auth
            || policy.challenge_prone
            || matches!(policy.fetch_mode, BrowserSourceFetchMode::BrowserOnly)
    })
}

impl SearchCandidate {
    fn requires_session_like_source(&self) -> bool {
        self.capability == SourceCapability::Cookie
            || self.capability == SourceCapability::Ui
            || self.source.eq_ignore_ascii_case("browser")
    }
}

fn query_requests_source_body_or_material(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    query_requests_public_text_catalog(query)
        || lowered.contains("full text")
        || lowered.contains("source material")
        || lowered.contains("readable")
        || lowered.contains("download")
        || lowered.contains("plain text")
        || query.contains("正文")
        || query.contains("全文")
        || query.contains("素材")
        || query.contains("可下载")
        || query.contains("下载")
        || query.contains("读取")
        || query.contains("入库")
        || query.contains("知识库")
}

fn candidate_key(candidate: &SearchCandidate) -> String {
    let url = candidate
        .url
        .split('?')
        .next()
        .unwrap_or(&candidate.url)
        .trim_end_matches('/')
        .to_ascii_lowercase();
    if !url.is_empty() {
        return url;
    }
    candidate.title.to_ascii_lowercase()
}

fn filter_candidates_by_query(
    candidates: Vec<SearchCandidate>,
    query: &str,
) -> Vec<SearchCandidate> {
    if query_requests_time_sensitive_lookup(query) {
        let mut filtered = candidates
            .into_iter()
            .filter(|candidate| time_sensitive_candidate_matches_query(query, candidate))
            .collect::<Vec<_>>();
        for (index, candidate) in filtered.iter_mut().enumerate() {
            candidate.rank = index + 1;
        }
        return filtered;
    }

    if contains_cjk(query) && !meaningful_cjk_query_terms(query).is_empty() {
        let mut filtered = candidates
            .into_iter()
            .filter(|candidate| {
                search_candidate_matches_query_terms(
                    query,
                    &candidate.title,
                    &candidate.url,
                    &candidate.snippet,
                )
            })
            .collect::<Vec<_>>();
        for (index, candidate) in filtered.iter_mut().enumerate() {
            candidate.rank = index + 1;
        }
        return filtered;
    }

    let terms = query_relevance_terms(query);
    if terms.len() < 2 {
        return candidates;
    }

    let min_hits = terms.len().min(2);
    let mut filtered = candidates
        .iter()
        .filter(|candidate| candidate_query_hits(candidate, &terms) >= min_hits)
        .cloned()
        .collect::<Vec<_>>();

    if filtered.is_empty() {
        return Vec::new();
    }

    for (index, candidate) in filtered.iter_mut().enumerate() {
        candidate.rank = index + 1;
    }
    filtered
}

fn time_sensitive_candidate_matches_query(query: &str, candidate: &SearchCandidate) -> bool {
    if candidate_is_static_background_result(candidate) {
        return false;
    }

    let haystack = format!(
        "{} {} {}",
        candidate.title, candidate.url, candidate.snippet
    );
    if !time_sensitive_subject_matches(query, &haystack) {
        return false;
    }

    if query_requires_live_measurement_evidence(query) {
        return time_sensitive_evidence_signal_matches(&haystack);
    }

    true
}

fn candidate_is_static_background_result(candidate: &SearchCandidate) -> bool {
    let lowered = format!(
        "{} {} {} {}",
        candidate.source, candidate.title, candidate.url, candidate.snippet
    )
    .to_ascii_lowercase();
    contains_any(
        &lowered,
        &[
            "wikipedia.org",
            "wikimedia.org",
            "wikidata.org",
            "encyclopedia",
            "百科",
        ],
    )
}

fn time_sensitive_subject_matches(query: &str, haystack: &str) -> bool {
    let ascii_terms = time_sensitive_ascii_subject_terms(query);
    let cjk_terms = time_sensitive_cjk_subject_terms(query);
    if ascii_terms.is_empty() && cjk_terms.is_empty() {
        return true;
    }

    let lowered = haystack.to_ascii_lowercase();
    ascii_terms
        .iter()
        .any(|term| lowered.contains(term.as_str()))
        || cjk_terms
            .iter()
            .any(|term| haystack.contains(term.as_str()))
}

fn time_sensitive_ascii_subject_terms(query: &str) -> Vec<String> {
    meaningful_ascii_query_terms(query)
        .into_iter()
        .filter(|term| !is_time_sensitive_query_modifier(term))
        .collect()
}

fn time_sensitive_cjk_subject_terms(query: &str) -> Vec<String> {
    let mut normalized = query.to_string();
    for marker in [
        "搜索一下",
        "查一下",
        "查找",
        "搜索",
        "今天",
        "今日",
        "当前",
        "现在",
        "实时",
        "最新",
        "天气",
        "预报",
        "气温",
        "温度",
        "降雨",
        "下雨",
        "怎样",
        "怎么样",
        "如何",
        "一下",
    ] {
        normalized = normalized.replace(marker, " ");
    }

    meaningful_cjk_query_terms(&normalized)
        .into_iter()
        .filter(|term| term.chars().count() >= 2)
        .collect()
}

fn is_time_sensitive_query_modifier(term: &str) -> bool {
    matches!(
        term,
        "current"
            | "today"
            | "now"
            | "live"
            | "latest"
            | "forecast"
            | "weather"
            | "temperature"
            | "realtime"
            | "real-time"
            | "lookup"
            | "search"
            | "find"
    )
}

fn query_requires_live_measurement_evidence(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    contains_any(
        &lowered,
        &[
            "forecast",
            "weather",
            "temperature",
            "temp",
            "price",
            "quote",
            "stock",
            "crypto",
            "bitcoin",
            "ethereum",
        ],
    ) || contains_any(
        query,
        &[
            "天气",
            "预报",
            "气温",
            "温度",
            "价格",
            "股价",
            "行情",
            "指数",
            "比特币",
            "以太坊",
        ],
    )
}

fn time_sensitive_evidence_signal_matches(haystack: &str) -> bool {
    let lowered = haystack.to_ascii_lowercase();
    lowered.contains('°')
        || lowered.contains("℃")
        || contains_any(
            &lowered,
            &[
                "forecast",
                "temperature",
                "hourly",
                "daily",
                "today",
                "current",
                "updated",
                "as of",
                "humidity",
                "precipitation",
                "wind",
                "radar",
                "weather.com",
                "accuweather",
                "metoffice",
            ],
        )
        || contains_any(
            haystack,
            &[
                "天气",
                "预报",
                "气温",
                "温度",
                "降雨",
                "湿度",
                "风力",
                "空气质量",
                "今日",
                "今天",
                "实时",
                "更新",
            ],
        )
}

fn filter_candidates_by_project_token(
    candidates: Vec<SearchCandidate>,
    query: &str,
) -> Vec<SearchCandidate> {
    let Some(project_token) = first_project_token(query)
        .or_else(|| recover_project_token_from_phrase(query))
        .map(|token| normalize_project_token(&token))
    else {
        return filter_candidates_by_query(candidates, query);
    };
    let mut filtered = candidates
        .into_iter()
        .filter(|candidate| {
            let normalized = normalize_project_token(&format!(
                "{} {} {}",
                candidate.title, candidate.url, candidate.snippet
            ));
            normalized_project_contains(&normalized, &project_token)
        })
        .collect::<Vec<_>>();
    for (index, candidate) in filtered.iter_mut().enumerate() {
        candidate.rank = index + 1;
    }
    filtered
}

fn recover_project_token_from_phrase(query: &str) -> Option<String> {
    let tokens = query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|token| token.to_ascii_lowercase())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    for window in tokens.windows(2) {
        if window == ["browser", "use"] && tokens.iter().any(|token| token == "agent") {
            return Some("browser-use".to_string());
        }
    }
    None
}

fn normalize_project_token(input: &str) -> String {
    input
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn normalized_project_contains(normalized_haystack: &str, normalized_needle: &str) -> bool {
    format!("-{}-", normalized_haystack).contains(&format!("-{}-", normalized_needle))
}

fn candidate_query_hits(candidate: &SearchCandidate, terms: &[String]) -> usize {
    let haystack = format!("{} {}", candidate.title, candidate.snippet).to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count()
}

fn candidate_matches_any_ascii_identity(candidate: &SearchCandidate, terms: &[String]) -> bool {
    let haystack = format!(
        "{} {} {}",
        candidate.title, candidate.url, candidate.snippet
    )
    .to_ascii_lowercase();
    terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .any(|term| haystack.contains(&term))
}

fn query_relevance_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for token in query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 4)
        .filter(|token| !is_query_relevance_noise(token))
    {
        if seen.insert(token.clone()) {
            terms.push(token);
        }
    }
    terms
}

fn is_query_relevance_noise(token: &str) -> bool {
    matches!(
        token,
        "search"
            | "find"
            | "lookup"
            | "information"
            | "regarding"
            | "about"
            | "related"
            | "provide"
            | "candidate"
            | "candidates"
            | "source"
            | "sources"
            | "links"
            | "official"
            | "document"
            | "documents"
            | "documentation"
            | "recent"
            | "news"
            | "with"
            | "their"
            | "this"
            | "that"
            | "from"
    )
}

fn normalize_query(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_public_search_query(query: &str) -> String {
    let normalized = normalize_query(query);
    if !contains_cjk(&normalized) {
        return normalized;
    }

    let mut terms = Vec::new();
    for site in site_filters(&normalized) {
        push_unique(&mut terms, site);
    }
    for term in mixed_query_ascii_identity_terms(&normalized) {
        push_unique(&mut terms, term);
    }
    for term in cjk_public_search_terms(&normalized) {
        push_unique(&mut terms, term);
    }
    for term in cjk_collection_terms(&normalized) {
        push_unique(&mut terms, term);
    }

    if terms.len() >= 3 {
        terms.join(" ")
    } else {
        normalized
    }
}

fn site_filters(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|token| {
            token
                .trim()
                .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | '“' | '”' | ',' | '，'))
        })
        .filter(|token| token.to_ascii_lowercase().starts_with("site:"))
        .map(str::to_string)
        .collect()
}

fn cjk_public_search_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();

    for term in ["免费", "下载", "公开", "公网", "可下载", "可访问"] {
        if query.contains(term) {
            push_unique(&mut terms, term.to_string());
        }
    }

    for term in cjk_query_core_segments(query) {
        push_unique(&mut terms, term);
        if terms.len() >= 10 {
            return terms;
        }
    }

    for run in query
        .split(|ch: char| !('\u{4e00}'..='\u{9fff}').contains(&ch))
        .map(str::trim)
        .filter(|run| run.chars().count() >= 2)
    {
        if run.chars().count() <= 12 && !is_cjk_search_noise(run) {
            push_unique(&mut terms, run.to_string());
            if terms.len() >= 10 {
                return terms;
            }
            continue;
        }

        let chars = run.chars().collect::<Vec<_>>();
        for size in [2usize, 3, 4] {
            if chars.len() < size {
                continue;
            }
            for window in chars.windows(size) {
                let term = window.iter().collect::<String>();
                if is_cjk_search_noise(&term) {
                    continue;
                }
                push_unique(&mut terms, term);
                if terms.len() >= 10 {
                    return terms;
                }
            }
        }
    }
    terms
}

fn cjk_query_core_segments(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for segment in query
        .split(|ch: char| {
            !('\u{4e00}'..='\u{9fff}').contains(&ch)
                || matches!(ch, '的' | '是' | '吗' | '？' | '，' | '。')
        })
        .map(str::trim)
        .filter(|segment| segment.chars().count() >= 2)
        .filter(|segment| !is_cjk_search_noise(segment))
    {
        if let Some(prefix) = segment.strip_suffix("官方网站") {
            if prefix.chars().count() >= 2 && !is_cjk_search_noise(prefix) {
                push_unique(&mut terms, prefix.to_string());
            }
            push_unique(&mut terms, "官方网站".to_string());
            continue;
        }
        if let Some(prefix) = segment.strip_suffix("官网") {
            if prefix.chars().count() >= 2 && !is_cjk_search_noise(prefix) {
                push_unique(&mut terms, prefix.to_string());
            }
            push_unique(&mut terms, "官网".to_string());
            continue;
        }
        push_unique(&mut terms, segment.to_string());
    }
    terms
}

fn mixed_query_ascii_identity_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    let site_hosts = site_filter_hosts(query);
    for raw in query.split_whitespace() {
        let token = raw
            .trim()
            .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_');
        let lowered = token.to_ascii_lowercase();
        if token.len() < 3
            || token.starts_with("site:")
            || is_search_scope_token(token)
            || is_search_instruction_noise(token)
            || is_common_search_stopword(&lowered)
            || is_search_access_modifier(&lowered)
            || site_hosts.iter().any(|host| host.contains(&lowered))
        {
            continue;
        }

        let has_identity_shape = token.chars().any(|ch| ch.is_ascii_uppercase())
            || token
                .chars()
                .any(|ch| ch == '-' || ch == '_' || ch.is_ascii_digit())
            || token.eq_ignore_ascii_case("readme");
        if has_identity_shape && seen.insert(lowered) {
            terms.push(token.to_string());
        }
    }
    terms
}

fn cjk_collection_terms(query: &str) -> Vec<String> {
    if !(query.contains("前10")
        || query.contains("前十")
        || query.contains("排行")
        || query.contains("排名")
        || query.contains("榜单")
        || query.contains("推荐")
        || query.contains("列表")
        || query.to_ascii_lowercase().contains("top"))
    {
        return Vec::new();
    }

    let mut terms = Vec::new();
    for term in ["排行", "榜单", "推荐", "列表"] {
        push_unique(&mut terms, term.to_string());
    }
    terms
}

fn search_candidate_matches_query_terms(
    query: &str,
    title: &str,
    url: &str,
    snippet: &str,
) -> bool {
    let haystack = format!("{title} {url} {snippet}");
    let ascii_terms = meaningful_ascii_query_terms(query);
    let ascii_identity_terms = mixed_query_ascii_identity_terms(query)
        .into_iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let cjk_terms = meaningful_cjk_query_terms(query);
    if !cjk_terms.is_empty() {
        let matches = cjk_terms
            .iter()
            .filter(|term| haystack.contains(term.as_str()))
            .count();
        let required = if cjk_terms.len() <= 2 { 1 } else { 2 };
        if matches < required {
            let translated_terms = public_text_candidate_match_terms(query);
            if query_requests_public_text_catalog(query)
                && query_allows_cross_language_public_text_catalog(query)
                && !translated_terms.is_empty()
                && ascii_terms_match_haystack(&translated_terms, &haystack)
            {
                return true;
            }
            if !ascii_identity_terms.is_empty()
                && ascii_terms_match_haystack(&ascii_identity_terms, &haystack)
            {
                return true;
            }
            return false;
        }
        return true;
    }

    let terms = ascii_terms;
    if terms.is_empty() {
        return true;
    }
    let haystack = format!("{title} {url} {snippet}").to_ascii_lowercase();
    let matches = terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count();
    if terms.len() <= 2 {
        return matches >= 1;
    }
    matches >= 2 || matches >= terms.len().saturating_sub(1)
}

fn ascii_terms_match_haystack(terms: &[String], haystack: &str) -> bool {
    if terms.is_empty() {
        return false;
    }
    let lowered = haystack.to_ascii_lowercase();
    let matches = terms
        .iter()
        .filter(|term| lowered.contains(term.as_str()))
        .count();
    if terms.len() <= 2 {
        matches >= 1
    } else {
        matches >= 2 || matches >= terms.len().saturating_sub(1)
    }
}

fn meaningful_ascii_query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for raw in query.split_whitespace() {
        let token = raw
            .trim()
            .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
            .to_ascii_lowercase();
        if token.len() < 3
            || token.starts_with("site:")
            || is_common_search_stopword(&token)
            || is_search_access_modifier(&token)
            || token.chars().all(|ch| ch.is_ascii_digit())
        {
            continue;
        }
        if seen.insert(token.clone()) {
            terms.push(token);
        }
    }
    terms
}

fn meaningful_cjk_query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();

    for term in cjk_query_core_segments(query) {
        if seen.insert(term.clone()) {
            terms.push(term);
        }
        if terms.len() >= 12 {
            return terms;
        }
    }

    for run in query
        .split(|ch: char| !('\u{4e00}'..='\u{9fff}').contains(&ch))
        .map(str::trim)
        .filter(|run| run.chars().count() >= 2)
    {
        if run.chars().count() <= 12
            && !cjk_run_contains_question_glue(run)
            && !is_cjk_search_noise(run)
        {
            if seen.insert(run.to_string()) {
                terms.push(run.to_string());
            }
            if terms.len() >= 12 {
                return terms;
            }
            continue;
        }

        let chars = run.chars().collect::<Vec<_>>();
        for size in [2usize, 3, 4] {
            if chars.len() < size {
                continue;
            }
            for window in chars.windows(size) {
                let term = window.iter().collect::<String>();
                if is_cjk_search_noise(&term) {
                    continue;
                }
                if seen.insert(term.clone()) {
                    terms.push(term);
                }
                if terms.len() >= 12 {
                    return terms;
                }
            }
        }
    }
    terms
}

fn query_has_explicit_match_terms(query: &str) -> bool {
    !mixed_query_ascii_identity_terms(query).is_empty()
        || !meaningful_ascii_query_terms(query).is_empty()
        || !cjk_query_core_segments(query)
            .into_iter()
            .filter(|term| !matches!(term.as_str(), "官网" | "官方网站"))
            .collect::<Vec<_>>()
            .is_empty()
}

fn is_cjk_search_noise(term: &str) -> bool {
    if term.contains("热门")
        || term.contains("免费")
        || term.contains("下载")
        || term.contains("完整")
        || term.contains("内容")
        || term.contains("公网")
        || term.contains("公开")
        || term.contains("可入")
        || term.contains("入库")
        || term.contains("查找")
        || term.contains("搜索")
        || term.contains("保存")
        || term.contains("网站")
        || term.contains("网页")
        || term.contains("页面")
        || term.contains("链接")
        || term.contains("资源")
        || term.contains("列表")
        || term.contains("排行")
        || term.contains("榜单")
        || term.contains("来源")
        || term.contains("给出")
        || term.contains("什么")
        || term.contains('的')
        || term.contains('是')
    {
        return true;
    }
    matches!(
        term,
        "热门"
            | "免费"
            | "下载"
            | "完整"
            | "内容"
            | "公网"
            | "公开"
            | "查找"
            | "搜索"
            | "保存"
            | "网站"
            | "网页"
            | "页面"
            | "链接"
            | "资源"
            | "列表"
            | "排行"
            | "榜单"
    )
}

fn cjk_run_contains_question_glue(run: &str) -> bool {
    run.contains("什么")
        || run.contains("是什么")
        || run.contains('的')
        || run.contains('是')
        || run.contains("给出")
        || run.contains("来源")
}

fn query_asks_for_search_engine(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    contains_any(
        &lowered,
        &[
            "search engine",
            "google",
            "bing",
            "baidu",
            "duckduckgo",
            "sogou",
        ],
    ) || contains_any(query, &["搜索引擎", "百度", "谷歌", "必应", "搜狗"])
}

fn is_common_search_stopword(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "with"
            | "from"
            | "into"
            | "onto"
            | "about"
            | "what"
            | "when"
            | "where"
            | "which"
            | "that"
            | "this"
            | "list"
            | "lists"
            | "top"
            | "best"
            | "popular"
            | "representative"
            | "reading"
            | "reference"
            | "source"
            | "sources"
            | "result"
            | "results"
            | "most"
            | "recommended"
            | "recommendation"
            | "currently"
            | "available"
            | "collection"
    )
}

fn is_search_access_modifier(token: &str) -> bool {
    matches!(
        token,
        "free"
            | "full"
            | "text"
            | "public"
            | "web"
            | "online"
            | "available"
            | "availability"
            | "download"
            | "downloadable"
            | "downloaded"
            | "downloads"
            | "ebook"
            | "ebooks"
            | "content"
            | "contents"
    )
}

fn push_unique(target: &mut Vec<String>, value: String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    if !target
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(trimmed))
    {
        target.push(trimmed.to_string());
    }
}

fn direct_url_candidates(query: &str) -> Vec<SearchCandidate> {
    direct_urls(query)
        .into_iter()
        .enumerate()
        .map(|(index, url)| SearchCandidate {
            title: url.clone(),
            url,
            snippet: "Explicit URL mentioned by the user; fetch this page directly before using search results."
                .to_string(),
            source: "direct_url".to_string(),
            capability: SourceCapability::Public,
            rank: index + 1,
            score: 0.0,
        })
        .collect()
}

fn direct_urls(query: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut seen = HashSet::new();
    for token in query.split_whitespace() {
        if let Some(url) = normalize_direct_url_token(token) {
            if seen.insert(url.clone()) {
                urls.push(url);
            }
        }
    }
    urls
}

fn normalize_direct_url_token(token: &str) -> Option<String> {
    let trimmed = token.trim().trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\''
                | '`'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '<'
                | '>'
                | '('
                | ')'
                | '（'
                | '）'
                | '['
                | ']'
                | '【'
                | '】'
                | ','
                | '，'
                | '。'
                | ';'
                | '；'
        )
    });
    let trimmed = trimmed.trim_end_matches(|ch: char| matches!(ch, '.' | ':' | '：' | '!' | '?'));
    if trimmed.is_empty() || trimmed.to_ascii_lowercase().starts_with("site:") {
        return None;
    }

    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else if looks_like_public_domain(trimmed) {
        format!("https://{trimmed}")
    } else {
        return None;
    };

    let parsed = reqwest::Url::parse(&with_scheme).ok()?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = parsed.host_str()?;
    if !looks_like_public_domain(host) {
        return None;
    }
    Some(parsed.to_string())
}

fn looks_like_public_domain(value: &str) -> bool {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty()
        || value.contains('@')
        || value.contains(' ')
        || value.starts_with('.')
        || value.ends_with('.')
        || value.eq_ignore_ascii_case("localhost")
    {
        return false;
    }
    let labels = value.split('.').collect::<Vec<_>>();
    if labels.len() < 2 {
        return false;
    }
    let Some(tld) = labels.last() else {
        return false;
    };
    if tld.len() < 2 || tld.len() > 24 || !tld.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    labels.iter().all(|label| {
        !label.is_empty()
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

fn source_api_query(query: &str) -> String {
    if let Some(quoted) = first_quoted_text(query) {
        return quoted;
    }
    if let Some(project_token) = first_project_token(query) {
        return project_token;
    }

    let tokens = query
        .split(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ',' | '，'
                        | '。'
                        | ';'
                        | '；'
                        | ':'
                        | '：'
                        | '('
                        | ')'
                        | '（'
                        | '）'
                        | '['
                        | ']'
                        | '【'
                        | '】'
                )
        })
        .map(|token| token.trim().trim_matches(['"', '\'', '`']))
        .filter(|token| !token.is_empty())
        .filter(|token| token.is_ascii())
        .filter(|token| !token.starts_with("site:"))
        .filter(|token| !token.eq_ignore_ascii_case("site"))
        .filter(|token| !is_search_scope_token(token))
        .filter(|token| !is_search_instruction_noise(token))
        .take(6)
        .map(str::to_string)
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        normalize_query(query)
    } else {
        tokens.join(" ")
    }
}

fn public_text_api_query(query: &str) -> String {
    let core_terms = public_text_bilingual_terms(query);
    if !core_terms.is_empty() {
        return core_terms.join(" ");
    }

    let mut terms = Vec::new();
    for token in query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 3)
        .filter(|token| !is_search_instruction_noise(token))
        .filter(|token| !is_search_access_modifier(token))
        .filter(|token| !is_public_text_catalog_noise(token))
        .filter(|token| !matches!(token.as_str(), "book" | "books" | "ebook" | "ebooks"))
    {
        push_unique(&mut terms, token);
        if terms.len() >= 3 {
            break;
        }
    }

    if terms.is_empty() {
        let intent = SearchPolicy::build_lookup_intent(query);
        for group in [
            intent.artifact_hints,
            intent.evidence_hints,
            intent.base_terms,
            intent.freshness_hints,
        ] {
            for hint in group {
                for token in hint
                    .split(|ch: char| !ch.is_ascii_alphanumeric())
                    .map(|token| token.trim().to_ascii_lowercase())
                    .filter(|token| token.len() >= 3)
                    .filter(|token| !is_search_access_modifier(token))
                    .filter(|token| !is_search_instruction_noise(token))
                    .filter(|token| !is_public_text_catalog_noise(token))
                    .filter(|token| {
                        !matches!(token.as_str(), "book" | "books" | "ebook" | "ebooks")
                    })
                {
                    push_unique(&mut terms, token);
                    if terms.len() >= 3 {
                        break;
                    }
                }
                if terms.len() >= 3 {
                    break;
                }
            }
            if terms.len() >= 3 {
                break;
            }
        }
    }

    if terms.is_empty() {
        source_api_query(query)
    } else {
        terms.join(" ")
    }
}

fn public_text_english_search_query(query: &str) -> Option<String> {
    if !contains_cjk(query)
        || !query_requests_public_text_catalog(query)
        || !query_allows_cross_language_public_text_catalog(query)
    {
        return None;
    }

    let mut terms = Vec::new();
    for term in public_text_bilingual_terms(query) {
        push_unique(&mut terms, term);
    }

    if terms.is_empty() {
        return None;
    }

    if query.contains("小说")
        || query.contains("书")
        || query.contains("正文")
        || query.contains("全文")
        || query.contains("下载")
    {
        push_unique(&mut terms, "fiction".to_string());
        push_unique(&mut terms, "plain text".to_string());
    }
    if query.contains("热门")
        || query.contains("排行")
        || query.contains("排名")
        || query.contains("前十")
        || query.contains("前10")
    {
        push_unique(&mut terms, "popular".to_string());
    }

    Some(terms.join(" "))
}

fn query_requests_document_lookup(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    lowered.contains("pdf")
        || lowered.contains(".pdf")
        || lowered.contains("whitepaper")
        || lowered.contains("white paper")
        || lowered.contains("document")
        || query.contains("白皮书")
        || query.contains("文档")
        || query.contains("论文")
}

fn document_english_search_query(query: &str) -> Option<String> {
    if !contains_cjk(query) || !query_requests_document_lookup(query) {
        return None;
    }

    let mut terms = document_bilingual_terms(query);
    for token in meaningful_ascii_query_terms(query) {
        push_unique(&mut terms, token);
    }
    if terms.is_empty() {
        return None;
    }
    Some(terms.join(" "))
}

fn document_bilingual_terms(query: &str) -> Vec<String> {
    let lowered = query.to_ascii_lowercase();
    let mut terms = Vec::new();

    if query.contains("比特币") || lowered.contains("bitcoin") || lowered.contains("btc") {
        push_unique(&mut terms, "bitcoin".to_string());
    }
    if query.contains("以太坊") || lowered.contains("ethereum") || lowered.contains("eth") {
        push_unique(&mut terms, "ethereum".to_string());
    }
    if query.contains("白皮书") || lowered.contains("whitepaper") || lowered.contains("white paper")
    {
        push_unique(&mut terms, "whitepaper".to_string());
    }
    if lowered.contains("pdf") || lowered.contains(".pdf") {
        push_unique(&mut terms, "pdf".to_string());
    }

    terms
}

fn public_text_catalog_queries(api_query: &str) -> Vec<String> {
    let mut queries = Vec::new();
    let normalized = api_query.trim();
    if !normalized.is_empty() {
        push_unique(&mut queries, normalized.to_string());
        let tokens = normalized.split_whitespace().collect::<Vec<_>>();
        if tokens.len() > 2 {
            push_unique(&mut queries, tokens[..2].join(" "));
        }
    }

    let relaxed = normalized
        .split_whitespace()
        .filter(|token| {
            !matches!(
                *token,
                "novel"
                    | "novels"
                    | "fiction"
                    | "book"
                    | "books"
                    | "ebook"
                    | "ebooks"
                    | "online"
                    | "download"
                    | "downloadable"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    if !relaxed.is_empty() {
        push_unique(&mut queries, relaxed);
    }

    queries
}

fn public_text_bilingual_terms(query: &str) -> Vec<String> {
    let lowered = query.to_ascii_lowercase();
    let mut terms = Vec::new();

    if query.contains("玄幻")
        || query.contains("奇幻")
        || query.contains("魔法")
        || lowered.contains("fantasy")
    {
        push_unique(&mut terms, "fantasy".to_string());
    }
    if query.contains("科幻") || lowered.contains("science fiction") || lowered.contains("sci-fi")
    {
        push_unique(&mut terms, "science fiction".to_string());
    }
    if query.contains("仙侠") || query.contains("修仙") || query.contains("武侠") {
        push_unique(&mut terms, "fantasy".to_string());
    }
    if query.contains("推理") || query.contains("侦探") || lowered.contains("mystery") {
        push_unique(&mut terms, "mystery".to_string());
    }
    if query.contains("历史") || lowered.contains("history") || lowered.contains("historical") {
        push_unique(&mut terms, "history".to_string());
    }
    if terms.is_empty() && query_requests_public_text_catalog(query) {
        push_unique(&mut terms, "fiction".to_string());
    }

    terms
}

fn public_text_candidate_match_terms(query: &str) -> Vec<String> {
    let mut terms = public_text_bilingual_terms(query);
    let lowered = query.to_ascii_lowercase();
    if query.contains("小说")
        || query.contains("书")
        || query.contains("网文")
        || lowered.contains("novel")
        || lowered.contains("book")
        || lowered.contains("fiction")
    {
        push_unique(&mut terms, "novel".to_string());
        push_unique(&mut terms, "fiction".to_string());
        push_unique(&mut terms, "book".to_string());
    }
    terms
}

fn is_public_text_catalog_noise(token: &str) -> bool {
    matches!(
        token,
        "popular"
            | "hot"
            | "top"
            | "ranking"
            | "ranked"
            | "catalog"
            | "collection"
            | "complete"
            | "contents"
            | "content"
    )
}

fn academic_api_query(query: &str) -> String {
    let tokens = query
        .split(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ',' | '，'
                        | '。'
                        | ';'
                        | '；'
                        | ':'
                        | '：'
                        | '('
                        | ')'
                        | '（'
                        | '）'
                        | '['
                        | ']'
                        | '【'
                        | '】'
                )
        })
        .map(|token| token.trim().trim_matches(['"', '\'', '`']))
        .filter(|token| !token.is_empty())
        .filter(|token| token.is_ascii())
        .filter(|token| !token.starts_with("site:"))
        .filter(|token| !is_search_scope_token(token))
        .filter(|token| !is_academic_instruction_noise(token))
        .take(8)
        .map(str::to_string)
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        source_api_query(query)
    } else {
        tokens.join(" ")
    }
}

fn arxiv_search_query(query: &str) -> String {
    let tokens = query
        .split_whitespace()
        .filter(|token| !is_academic_instruction_noise(token))
        .take(5)
        .map(|token| format!("all:{}", token))
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        format!("all:{}", query)
    } else {
        tokens.join(" AND ")
    }
}

fn first_project_token(query: &str) -> Option<String> {
    query
        .split(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ',' | '，'
                        | '。'
                        | ';'
                        | '；'
                        | ':'
                        | '：'
                        | '('
                        | ')'
                        | '（'
                        | '）'
                        | '['
                        | ']'
                        | '【'
                        | '】'
                )
        })
        .map(|token| token.trim().trim_matches(['"', '\'', '`']))
        .filter(|token| token.is_ascii())
        .filter(|token| !token.starts_with("site:"))
        .filter(|token| !is_search_scope_token(token))
        .filter(|token| !is_search_instruction_noise(token))
        .find(|token| is_ascii_project_token(token))
        .map(str::to_string)
}

fn first_quoted_text(query: &str) -> Option<String> {
    for quote in ['"', '\'', '“', '”'] {
        let mut parts = query.split(quote);
        let _before = parts.next();
        if let Some(text) = parts.next() {
            let trimmed = text.trim();
            if trimmed.chars().count() > 2 && !trimmed.contains("://") {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn is_academic_instruction_noise(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "arxiv"
            | "pubmed"
            | "paper"
            | "papers"
            | "论文"
            | "search"
            | "find"
            | "lookup"
            | "related"
            | "regarding"
            | "about"
            | "provide"
            | "candidate"
            | "candidates"
            | "title"
            | "titles"
            | "source"
            | "sources"
            | "link"
            | "links"
            | "this"
            | "url"
            | "urls"
            | "with"
            | "their"
            | "the"
            | "for"
            | "and"
            | "or"
            | "on"
            | "of"
            | "to"
    )
}

fn is_search_scope_token(token: &str) -> bool {
    let lowered = token.to_ascii_lowercase();
    lowered.ends_with(".com")
        || lowered.ends_with(".org")
        || lowered.ends_with(".net")
        || lowered.ends_with(".io")
        || lowered.ends_with(".dev")
}

fn is_search_instruction_noise(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "github"
            | "gitlab"
            | "search"
            | "find"
            | "lookup"
            | "project"
            | "projects"
            | "repo"
            | "repos"
            | "repository"
            | "repositories"
            | "high-starred"
            | "starred"
            | "related"
            | "return"
            | "real"
            | "url"
            | "urls"
            | "source"
            | "sources"
            | "confirm"
            | "confirmed"
            | "using"
            | "used"
            | "use"
            | "tool"
            | "tools"
            | "browser"
            | "web_search"
            | "chinese"
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn contains_cjk(input: &str) -> bool {
    input.chars().any(|ch| {
        matches!(
            ch as u32,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
        )
    })
}

fn strip_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn query_requests_feed_discovery(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    contains_any(
        &lowered,
        &["rss", "atom", "feed", "feeds", "subscription feed"],
    ) || contains_any(query, &["订阅源", "信息流", "RSS", "Atom"])
}

fn site_filter_hosts(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter_map(|token| token.trim().strip_prefix("site:"))
        .filter_map(|host| {
            let host = host
                .trim()
                .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | ',' | '，'))
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .trim_matches('/');
            looks_like_public_domain(host).then(|| host.to_ascii_lowercase())
        })
        .collect()
}

fn common_feed_urls_for_host(host: &str) -> Vec<String> {
    let host = host.trim().trim_matches('/').to_ascii_lowercase();
    if !looks_like_public_domain(&host) {
        return Vec::new();
    }
    ["/feed", "/rss", "/rss.xml", "/feed.xml", "/atom.xml"]
        .into_iter()
        .map(|path| format!("https://{host}{path}"))
        .collect()
}

fn discover_feed_urls_from_html(base_url: &Url, html: &str) -> Vec<String> {
    let mut feeds = Vec::new();
    let Ok(link_re) = regex::Regex::new(r#"(?is)<link\b[^>]*>"#) else {
        return feeds;
    };
    let Ok(href_re) = regex::Regex::new(r#"(?is)\bhref\s*=\s*["']([^"']+)["']"#) else {
        return feeds;
    };

    for link in link_re.find_iter(html).take(80) {
        let tag = link.as_str();
        let lowered = tag.to_ascii_lowercase();
        let is_feed = lowered.contains("alternate")
            && (lowered.contains("rss")
                || lowered.contains("atom")
                || lowered.contains("application/xml")
                || lowered.contains("application/feed+json"));
        if !is_feed {
            continue;
        }
        let Some(href) = href_re
            .captures(tag)
            .and_then(|capture| capture.get(1))
            .map(|value| value.as_str().trim())
        else {
            continue;
        };
        if let Ok(url) = base_url.join(href) {
            push_unique(&mut feeds, url.to_string());
        }
    }
    feeds
}

fn parse_feed_candidates(
    body: &str,
    feed_url: &str,
    source: &SourceAdapterSpec,
    query: &str,
    limit: usize,
) -> Vec<SearchCandidate> {
    let item_blocks = feed_blocks(body);
    let terms = feed_match_terms(query);
    let mut candidates = Vec::new();

    for block in item_blocks.into_iter().take(80) {
        let title = first_xml_text(&block, &["title"]).unwrap_or_default();
        let link = first_xml_link(&block).unwrap_or_default();
        let summary =
            first_xml_text(&block, &["description", "summary", "content"]).unwrap_or_default();
        let published =
            first_xml_text(&block, &["pubDate", "published", "updated"]).unwrap_or_default();
        let haystack = format!("{title} {summary} {published} {link}");
        if !terms.is_empty() && !feed_terms_match(&terms, &haystack) {
            continue;
        }
        let url = if link.is_empty() {
            feed_url.to_string()
        } else {
            link
        };
        candidates.push(SearchCandidate {
            title: if title.is_empty() {
                "Untitled feed item".to_string()
            } else {
                title
            },
            url,
            snippet: [published, summary]
                .into_iter()
                .filter(|value| !value.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" — ")
                .chars()
                .take(500)
                .collect(),
            source: source.name.clone(),
            capability: SourceCapability::Rss,
            rank: candidates.len() + 1,
            score: 0.0,
        });
        if candidates.len() >= limit {
            break;
        }
    }
    candidates
}

fn feed_blocks(body: &str) -> Vec<String> {
    let mut blocks = regex_blocks(body, "item");
    if blocks.is_empty() {
        blocks = regex_blocks(body, "entry");
    }
    blocks
}

fn regex_blocks(body: &str, tag: &str) -> Vec<String> {
    let pattern = format!(r"(?is)<{tag}\b[^>]*>(.*?)</{tag}>");
    let Ok(re) = regex::Regex::new(&pattern) else {
        return Vec::new();
    };
    re.captures_iter(body)
        .filter_map(|capture| capture.get(1).map(|value| value.as_str().to_string()))
        .collect()
}

fn first_xml_text(block: &str, tags: &[&str]) -> Option<String> {
    for tag in tags {
        let pattern = format!(r"(?is)<(?:[\w.-]+:)?{tag}\b[^>]*>(.*?)</(?:[\w.-]+:)?{tag}>");
        let Ok(re) = regex::Regex::new(&pattern) else {
            continue;
        };
        if let Some(value) = re
            .captures(block)
            .and_then(|capture| capture.get(1))
            .map(|value| clean_xml_text(value.as_str()))
            .filter(|value| !value.trim().is_empty())
        {
            return Some(value);
        }
    }
    None
}

fn first_xml_link(block: &str) -> Option<String> {
    if let Some(link) = first_xml_text(block, &["link"]).filter(|value| value.starts_with("http")) {
        return Some(link);
    }
    let Ok(href_re) =
        regex::Regex::new(r#"(?is)<(?:[\w.-]+:)?link\b[^>]*\bhref\s*=\s*["']([^"']+)["'][^>]*/?>"#)
    else {
        return None;
    };
    href_re
        .captures(block)
        .and_then(|capture| capture.get(1))
        .map(|value| clean_xml_text(value.as_str()))
        .filter(|value| value.starts_with("http"))
}

fn clean_xml_text(value: &str) -> String {
    let without_cdata = value
        .replace("<![CDATA[", "")
        .replace("]]>", "")
        .replace('\r', " ")
        .replace('\n', " ");
    strip_html(&without_cdata)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn feed_match_terms(query: &str) -> Vec<String> {
    let mut terms = meaningful_ascii_query_terms(query);
    for term in meaningful_cjk_query_terms(query) {
        push_unique(&mut terms, term);
    }
    terms
        .into_iter()
        .filter(|term| !is_time_sensitive_query_modifier(term))
        .filter(|term| !matches!(term.as_str(), "news" | "latest" | "recent" | "today"))
        .take(8)
        .collect()
}

fn feed_terms_match(terms: &[String], haystack: &str) -> bool {
    let lowered = haystack.to_ascii_lowercase();
    let hits = terms
        .iter()
        .filter(|term| lowered.contains(&term.to_ascii_lowercase()) || haystack.contains(*term))
        .count();
    hits >= terms.len().min(2)
}

fn parse_gutendex_results(
    body: &str,
    source: &SourceAdapterSpec,
    max_results: usize,
) -> anyhow::Result<Vec<SearchCandidate>> {
    #[derive(Deserialize)]
    struct GutendexPerson {
        name: String,
    }

    #[derive(Deserialize)]
    struct GutendexBook {
        id: u64,
        title: String,
        #[serde(default)]
        authors: Vec<GutendexPerson>,
        #[serde(default)]
        summaries: Vec<String>,
        #[serde(default)]
        subjects: Vec<String>,
        #[serde(default)]
        bookshelves: Vec<String>,
        #[serde(default)]
        formats: HashMap<String, String>,
        #[serde(default)]
        download_count: Option<u64>,
    }

    #[derive(Deserialize)]
    struct GutendexResponse {
        results: Vec<GutendexBook>,
    }

    let response: GutendexResponse = serde_json::from_str(body)?;
    Ok(response
        .results
        .into_iter()
        .take(max_results)
        .enumerate()
        .map(|(index, book)| {
            let text_url = gutendex_plain_text_url(&book.formats);
            let url = text_url
                .clone()
                .unwrap_or_else(|| format!("https://www.gutenberg.org/ebooks/{}", book.id));
            let authors = book
                .authors
                .iter()
                .map(|author| author.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let subjects = book
                .subjects
                .iter()
                .chain(book.bookshelves.iter())
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
            let summary = book
                .summaries
                .first()
                .map(|value| value.as_str())
                .unwrap_or_default();
            let mut snippet = format!(
                "Public downloadable text catalog record. Authors: {}. Subjects: {}. Downloads: {}. {}",
                authors,
                subjects,
                book.download_count.unwrap_or_default(),
                summary
            );
            if let Some(text_url) = text_url {
                snippet.push_str(&format!(" Plain text: {text_url}"));
            }
            SearchCandidate {
                title: book.title,
                url,
                snippet: normalize_query(&snippet),
                source: source.name.clone(),
                capability: source.capability.clone(),
                rank: index + 1,
                score: 0.0,
            }
        })
        .collect())
}

fn gutendex_plain_text_url(formats: &HashMap<String, String>) -> Option<String> {
    formats
        .iter()
        .filter(|(kind, url)| {
            kind.to_ascii_lowercase().starts_with("text/plain") && url.starts_with("http")
        })
        .min_by_key(|(kind, _)| {
            if kind.eq_ignore_ascii_case("text/plain; charset=utf-8") {
                0
            } else {
                1
            }
        })
        .map(|(_, url)| url.clone())
}

fn parse_duckduckgo_results(
    html: &str,
    source: &SourceAdapterSpec,
    max_results: usize,
) -> Vec<SearchCandidate> {
    html.split("result__body")
        .skip(1)
        .filter_map(|block| {
            let title_anchor = block.find("result__a").and_then(|class_at| {
                let anchor_start = block[..class_at].rfind("<a").unwrap_or(class_at);
                let anchor_end = block[class_at..].find("</a>")? + class_at + "</a>".len();
                Some(&block[anchor_start..anchor_end])
            })?;
            let title = extract_anchor_text(title_anchor)
                .map(|text| normalize_query(&strip_html(&text)))
                .unwrap_or_default();
            if title.is_empty() {
                return None;
            }

            let raw_url = extract_attr_value(title_anchor, "href")?;
            let url = normalize_duckduckgo_url(&raw_url)?;
            let snippet = extract_duckduckgo_snippet(block);

            Some(SearchCandidate {
                title,
                url,
                snippet,
                source: source.name.clone(),
                capability: source.capability.clone(),
                rank: 0,
                score: 0.0,
            })
        })
        .enumerate()
        .take(max_results)
        .map(|(index, mut candidate)| {
            candidate.rank = index + 1;
            candidate
        })
        .collect()
}

fn normalize_duckduckgo_url(raw_url: &str) -> Option<String> {
    let decoded_once = raw_url.replace("&amp;", "&");
    let candidate = if let Some(uddg_at) = decoded_once.find("uddg=") {
        let value_start = uddg_at + "uddg=".len();
        let value_end = decoded_once[value_start..]
            .find('&')
            .map(|offset| value_start + offset)
            .unwrap_or(decoded_once.len());
        urlencoding::decode(&decoded_once[value_start..value_end])
            .ok()
            .map(|value| value.into_owned())?
    } else {
        decoded_once
    };
    normalize_result_url(candidate)
}

fn extract_duckduckgo_snippet(block: &str) -> String {
    if let Some(start) = block.find("result__snippet") {
        let tag_start = block[..start].rfind('<').unwrap_or(start);
        let slice = &block[tag_start..];
        let end = slice
            .find("</a>")
            .or_else(|| slice.find("</div>"))
            .unwrap_or_else(|| previous_char_boundary(slice, slice.len().min(700)));
        return normalize_query(&strip_html(&slice[..end]));
    }
    String::new()
}

fn parse_sogou_results(
    html: &str,
    source: &SourceAdapterSpec,
    max_results: usize,
) -> Vec<SearchCandidate> {
    html.split("<h3")
        .skip(1)
        .filter_map(|block| {
            let h3_end = block.find("</h3>")?;
            let h3 = &block[..h3_end];
            let title = extract_anchor_text(h3)
                .map(|text| normalize_query(&strip_html(&text)))
                .unwrap_or_else(|| normalize_query(&strip_html(h3)));
            if title.is_empty() {
                return None;
            }

            let primary_url = extract_attr_value(h3, "href");
            let cite_url = extract_sogou_cite_url(block);
            let url = normalize_result_url(cite_url.or(primary_url)?)?;
            let snippet = extract_sogou_snippet(&block[h3_end..]);

            Some(SearchCandidate {
                title,
                url,
                snippet,
                source: source.name.clone(),
                capability: source.capability.clone(),
                rank: 0,
                score: 0.0,
            })
        })
        .enumerate()
        .take(max_results)
        .map(|(index, mut candidate)| {
            candidate.rank = index + 1;
            candidate
        })
        .collect()
}

fn parse_so_results(
    html: &str,
    source: &SourceAdapterSpec,
    max_results: usize,
) -> Vec<SearchCandidate> {
    html.split("res-list")
        .skip(1)
        .filter_map(|block| {
            let h3_start = block.find("<h3")?;
            let h3_end = block[h3_start..].find("</h3>")? + h3_start + "</h3>".len();
            let h3 = &block[h3_start..h3_end];
            let title = extract_anchor_text(h3)
                .map(|text| normalize_query(&strip_html(&text)))
                .unwrap_or_else(|| normalize_query(&strip_html(h3)));
            if title.is_empty() {
                return None;
            }

            let raw_url =
                extract_attr_value(h3, "data-mdurl").or_else(|| extract_attr_value(h3, "href"))?;
            let url = normalize_so_result_url(&raw_url)?;
            let snippet = extract_so_snippet(block);

            Some(SearchCandidate {
                title,
                url,
                snippet,
                source: source.name.clone(),
                capability: source.capability.clone(),
                rank: 0,
                score: 0.0,
            })
        })
        .enumerate()
        .take(max_results)
        .map(|(index, mut candidate)| {
            candidate.rank = index + 1;
            candidate
        })
        .collect()
}

fn normalize_so_result_url(raw_url: &str) -> Option<String> {
    normalize_result_url(raw_url.trim().to_string())
}

fn extract_so_snippet(block: &str) -> String {
    if let Some(start) = block.find("res-desc") {
        let tag_start = block[..start].rfind('<').unwrap_or(start);
        let slice = &block[tag_start..];
        let end = slice
            .find("</p>")
            .or_else(|| slice.find("</div>"))
            .unwrap_or_else(|| previous_char_boundary(slice, slice.len().min(900)));
        let snippet = normalize_query(&strip_html(&slice[..end]));
        if !snippet.is_empty() {
            return snippet;
        }
    }
    String::new()
}

fn extract_sogou_cite_url(block: &str) -> Option<String> {
    let class_index = block.find("citeLinkClass")?;
    let anchor_start = block[..class_index].rfind("<a").unwrap_or(class_index);
    let window_end = previous_char_boundary(block, (class_index + 1200).min(block.len()));
    extract_attr_value(&block[anchor_start..window_end], "href")
}

fn extract_sogou_snippet(block: &str) -> String {
    for marker in ["class=\"ft\"", "class=\"str_info\"", "<p"] {
        if let Some(start) = block.find(marker) {
            let tag_start = block[..start].rfind('<').unwrap_or(start);
            let slice = &block[tag_start..];
            let end = slice
                .find("</div>")
                .or_else(|| slice.find("</p>"))
                .unwrap_or_else(|| previous_char_boundary(slice, slice.len().min(700)));
            let snippet = normalize_query(&strip_html(&slice[..end]));
            if !snippet.is_empty() {
                return snippet;
            }
        }
    }
    String::new()
}

fn extract_anchor_text(input: &str) -> Option<String> {
    let anchor_start = input.find("<a")?;
    let text_start = input[anchor_start..].find('>')? + anchor_start + 1;
    let text_end = input[text_start..].find("</a>")? + text_start;
    Some(input[text_start..text_end].to_string())
}

fn extract_attr_value(input: &str, attr: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let marker = format!("{attr}={quote}");
        if let Some(start) = input.find(&marker) {
            let value_start = start + marker.len();
            let value_end = input[value_start..].find(quote)? + value_start;
            let value = strip_html(&input[value_start..value_end]);
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn previous_char_boundary(input: &str, mut index: usize) -> usize {
    index = index.min(input.len());
    while index > 0 && !input.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn normalize_result_url(url: String) -> Option<String> {
    let url = url.trim().to_string();
    if url.starts_with("http://") || url.starts_with("https://") {
        return Some(url);
    }
    if url.starts_with("//") {
        return Some(format!("https:{url}"));
    }
    None
}

#[cfg(not(feature = "browser"))]
fn parse_basic_bing_results(
    html: &str,
    source: &SourceAdapterSpec,
    max_results: usize,
) -> Vec<SearchCandidate> {
    html.split("<li class=\"b_algo\"")
        .skip(1)
        .filter_map(|block| {
            let href_marker = "<a href=\"";
            let href_start = block.find(href_marker)? + href_marker.len();
            let href_end = block[href_start..].find('"')? + href_start;
            let url = strip_html(&block[href_start..href_end]);
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return None;
            }

            let title_start = block[href_end..].find('>')? + href_end + 1;
            let title_end = block[title_start..].find("</a>")? + title_start;
            let title = normalize_query(&strip_html(&block[title_start..title_end]));

            let snippet = block
                .find("<p>")
                .and_then(|start| {
                    let start = start + "<p>".len();
                    let end = block[start..].find("</p>")? + start;
                    Some(normalize_query(&strip_html(&block[start..end])))
                })
                .unwrap_or_default();

            Some(SearchCandidate {
                title,
                url,
                snippet,
                source: source.name.clone(),
                capability: source.capability.clone(),
                rank: 0,
                score: 0.0,
            })
        })
        .enumerate()
        .take(max_results)
        .map(|(index, mut candidate)| {
            candidate.rank = index + 1;
            candidate
        })
        .collect()
}

fn parse_arxiv_entries(body: &str, source: &SourceAdapterSpec) -> Vec<SearchCandidate> {
    body.split("<entry>")
        .skip(1)
        .enumerate()
        .filter_map(|(index, entry)| {
            let title = extract_xml_text(entry, "title")?;
            let id = extract_xml_text(entry, "id")?;
            let summary = extract_xml_text(entry, "summary").unwrap_or_default();
            Some(SearchCandidate {
                title: normalize_query(&title),
                url: id,
                snippet: normalize_query(&summary),
                source: source.name.clone(),
                capability: source.capability.clone(),
                rank: index + 1,
                score: 0.0,
            })
        })
        .collect()
}

fn extract_xml_text(entry: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = entry.find(&open)? + open.len();
    let end = entry[start..].find(&close)? + start;
    Some(strip_html(&entry[start..end]))
}

#[cfg(feature = "browser")]
pub(super) async fn search_browser(
    source: &SourceAdapterSpec,
    query: &str,
    max_results: usize,
) -> anyhow::Result<Vec<SearchCandidate>> {
    let (_, _, results) = BrowserTool::search_once(query, None, max_results).await?;
    Ok(results
        .into_iter()
        .enumerate()
        .map(|(index, result)| SearchCandidate {
            title: result.title,
            url: result.url,
            snippet: result.snippet,
            source: source.name.clone(),
            capability: source.capability.clone(),
            rank: index + 1,
            score: 0.0,
        })
        .collect())
}

#[cfg(not(feature = "browser"))]
pub(super) async fn search_browser(
    _source: &SourceAdapterSpec,
    _query: &str,
    _max_results: usize,
) -> anyhow::Result<Vec<SearchCandidate>> {
    anyhow::bail!(
        "Browser source requires Microsoft Edge or Google Chrome, or the browser feature must be enabled"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_source_timeout_is_chat_fast() {
        let config = SearchOrchestratorConfig::default();
        assert_eq!(config.source_timeout, Duration::from_secs(4));
    }

    #[test]
    fn code_query_prefers_github() {
        let plan = build_search_plan("github rust agent browser");
        assert_eq!(plan.intent, SearchIntent::Code);
        assert_eq!(plan.candidate_sources[0].name, "github");
        assert_eq!(
            plan.candidate_sources[0].capability,
            SourceCapability::Public
        );
        assert!(!plan.candidate_sources[0].requires_auth);
    }

    #[test]
    fn general_search_includes_public_html_fallback_source() {
        let plan = build_search_plan("popular fantasy novels download");
        assert_eq!(plan.intent, SearchIntent::General);
        assert!(plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "duckduckgo"
                && source.capability == SourceCapability::SearchEngine
                && !source.requires_browser));
    }

    #[test]
    fn public_text_request_adds_structured_downloadable_text_source() {
        let plan = build_search_plan("popular fantasy novels download full text");

        assert!(plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "gutendex"
                && source.capability == SourceCapability::Public
                && !source.requires_browser));
        assert_eq!(
            plan.candidate_sources
                .first()
                .map(|source| source.name.as_str()),
            Some("gutendex")
        );
    }

    #[test]
    fn cjk_public_text_request_adds_bilingual_subquery() {
        let plan = build_search_plan("热门免费玄幻奇幻小说下载 列表");
        assert!(!plan
            .subqueries
            .iter()
            .any(|query| query.label == "public_text_bilingual"));
        assert!(plan
            .subqueries
            .iter()
            .any(|query| query.query.contains("玄幻") || query.query.contains("奇幻")));
        assert!(!plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "gutendex"));
    }

    #[test]
    fn cjk_public_text_request_allows_explicit_cross_language_catalog() {
        let plan = build_search_plan("热门免费英文 fantasy 小说下载 列表");
        assert!(plan
            .subqueries
            .iter()
            .any(|query| query.label == "public_text_bilingual"));
        assert!(plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "gutendex"));
    }

    #[test]
    fn public_text_sources_get_slow_source_budget() {
        let gutendex = source("gutendex", 100);
        assert!(
            source_timeout_for_query(&gutendex, "fantasy fiction", Duration::from_secs(8))
                >= Duration::from_secs(20)
        );
        let bing = source("bing", 100);
        assert_eq!(
            source_timeout_for_query(&bing, "fantasy fiction", Duration::from_secs(4)),
            Duration::from_secs(4)
        );
    }

    #[test]
    fn whitepaper_query_is_not_book_catalog_and_normalizes_pdt_typo() {
        assert!(!query_requests_public_text_catalog(
            "尝试搜索一个比特币白皮书pdt"
        ));
        assert_eq!(
            normalize_common_search_typos("尝试搜索一个比特币白皮书pdt").as_ref(),
            "尝试搜索一个比特币白皮书 pdf"
        );
        let plan = build_search_plan("尝试搜索一个比特币白皮书 pdf");
        assert!(plan
            .subqueries
            .iter()
            .any(|subquery| subquery.label == "document_bilingual"
                && subquery.query == "bitcoin whitepaper pdf"));
    }

    #[test]
    fn public_text_api_query_uses_meaningful_bilingual_terms() {
        assert_eq!(
            public_text_api_query("popular downloadable free fantasy novels online"),
            "fantasy"
        );
        let query = public_text_api_query("热门免费玄幻小说下载 完整内容");
        assert_eq!(query, "fantasy");
        assert_eq!(
            public_text_api_query(
                "classic high-quality interstellar sci-fi novels for world-building"
            ),
            "science fiction"
        );
    }

    #[test]
    fn public_text_catalog_queries_relax_artifact_terms() {
        let queries = public_text_catalog_queries("fantasy novels online");
        assert_eq!(
            queries.first().map(String::as_str),
            Some("fantasy novels online")
        );
        assert!(queries.iter().any(|query| query == "fantasy novels"));
        assert!(queries.iter().any(|query| query == "fantasy"));
    }

    #[test]
    fn gutendex_parser_extracts_plain_text_download_url() {
        let source = source("gutendex", 90);
        let body = r#"{
          "results": [{
            "id": 123,
            "title": "Example Fantasy",
            "authors": [{"name": "Example Author"}],
            "summaries": ["A public-domain fantasy story."],
            "subjects": ["Fantasy fiction"],
            "bookshelves": ["Fantasy"],
            "formats": {
              "text/plain; charset=utf-8": "https://www.gutenberg.org/files/123/123-0.txt",
              "text/html": "https://www.gutenberg.org/ebooks/123.html.images"
            },
            "download_count": 42
          }]
        }"#;

        let results = parse_gutendex_results(body, &source, 5).expect("gutendex");

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].url,
            "https://www.gutenberg.org/files/123/123-0.txt"
        );
        assert!(results[0].snippet.contains("Plain text"));
    }

    #[test]
    fn gutendex_structured_results_survive_explicit_cross_language_query() {
        let source = source("gutendex", 90);
        let body = r#"{
          "results": [{
            "id": 34219,
            "title": "The Enchanted Castle",
            "authors": [{"name": "Nesbit, E."}],
            "summaries": ["A fantasy story with a magic castle."],
            "subjects": ["Fantasy fiction"],
            "bookshelves": ["Children's Fiction"],
            "formats": {
              "text/plain; charset=utf-8": "https://www.gutenberg.org/files/34219/34219-0.txt"
            },
            "download_count": 99
          }]
        }"#;

        let api_query = public_text_api_query("热门免费英文 fantasy 小说下载 列表");
        let results = parse_gutendex_results(body, &source, 5).expect("gutendex");

        assert_eq!(api_query, "fantasy");
        assert_eq!(results.len(), 1);
        assert!(results[0].snippet.contains("Plain text"));
    }

    #[test]
    fn source_adapter_override_can_replace_domains_capability_and_weight() {
        let mut sources = vec![source("browser", 70)];
        apply_source_adapter_override(
            &mut sources,
            RuntimeSourceAdapterOverride {
                name: "browser".to_string(),
                capability: Some(SourceCapability::Public),
                requires_browser: Some(false),
                requires_auth: Some(true),
                challenge_prone: Some(false),
                domains: Some(vec!["api.example.com".to_string()]),
                fallback_sources: Some(vec!["bing".to_string()]),
                weight: Some(42),
            },
        );

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].capability, SourceCapability::Public);
        assert_eq!(sources[0].requires_browser, false);
        assert_eq!(sources[0].requires_auth, true);
        assert_eq!(sources[0].challenge_prone, false);
        assert_eq!(sources[0].domains, vec!["api.example.com"]);
        assert_eq!(sources[0].fallback_sources, vec!["bing"]);
        assert_eq!(sources[0].weight, 42);
    }

    #[test]
    fn duckduckgo_parser_extracts_result_urls_and_snippets() {
        let source = source("duckduckgo", 90);
        let html = r#"
        <div class="result__body">
          <a rel="nofollow" class="result__a" href="/l/?uddg=https%3A%2F%2Fexample.com%2Fbook&amp;rut=abc">Example Book</a>
          <a class="result__snippet">A useful fantasy novel result.</a>
        </div>
        "#;

        let results = parse_duckduckgo_results(html, &source, 5);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example Book");
        assert_eq!(results[0].url, "https://example.com/book");
        assert!(results[0].snippet.contains("fantasy novel"));
    }

    #[test]
    fn project_like_query_prefers_code_sources_without_saying_github() {
        let plan = build_search_plan("browser-use agent browser related resources");
        assert_eq!(plan.intent, SearchIntent::Code);
        assert_eq!(plan.candidate_sources[0].name, "github");
    }

    #[test]
    fn explicit_recent_discussion_query_prefers_news_over_project_shape() {
        let plan = build_search_plan("搜索最近 browser-use agent browser 的相关新闻或讨论");
        assert_eq!(plan.intent, SearchIntent::News);
        assert_eq!(plan.candidate_sources[0].name, "bing");
        assert!(!plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "github"));
    }

    #[test]
    fn recent_data_lookup_stays_general_not_news_vertical() {
        let plan = build_search_plan("查找最近2个月中国福利彩票每期开奖数据");
        assert_eq!(plan.intent, SearchIntent::General);
        assert_eq!(plan.candidate_sources[0].name, "bing");
        assert_eq!(plan.candidate_sources[1].name, "sogou");
        assert_eq!(plan.candidate_sources[2].name, "so");
        assert!(!plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "hackernews"));
    }

    #[test]
    fn live_lookup_plan_keeps_bounded_browser_fallback_without_static_background_sources() {
        let plan = build_search_plan("搜索一下今天北京天气怎样");

        assert_eq!(plan.freshness, "strict_recent");
        assert!(plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "browser"));
        assert!(plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "rss"));
        assert!(!plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "wikipedia"));
    }

    #[test]
    fn live_lookup_filter_rejects_encyclopedia_weather_noise() {
        let candidates = vec![
            SearchCandidate {
                title: "Weather - Wikipedia".to_string(),
                url: "https://en.wikipedia.org/wiki/Weather".to_string(),
                snippet: "Weather is the state of the atmosphere. Beijing hosted events in 2008."
                    .to_string(),
                source: "wikipedia".to_string(),
                capability: SourceCapability::Public,
                rank: 1,
                score: 0.0,
            },
            SearchCandidate {
                title: "Beijing Weather Forecast".to_string(),
                url: "https://example.com/weather/beijing".to_string(),
                snippet: "Today hourly temperature, wind and precipitation forecast for Beijing."
                    .to_string(),
                source: "bing".to_string(),
                capability: SourceCapability::Public,
                rank: 2,
                score: 0.0,
            },
        ];

        let filtered = filter_candidates_by_query(candidates, "today Beijing weather");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Beijing Weather Forecast");
    }

    #[test]
    fn qidian_novel_ranking_query_stays_general_not_code_vertical() {
        let plan = build_search_plan(
            "Search for the top 10 most recommended free fantasy 玄幻 novels currently available on Qidian site:qidian.com novel ranking recommendation collection list source",
        );

        assert_eq!(plan.intent, SearchIntent::General);
        assert_eq!(
            plan.subqueries[0].query,
            "site:qidian.com 玄幻 排行 榜单 推荐 列表"
        );
        assert!(plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "sogou"));
        assert!(plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "so"));
        assert!(!plan
            .candidate_sources
            .iter()
            .any(|source| source.name == "github" || source.name == "hackernews"));
    }

    #[test]
    fn scoped_novel_ranking_query_does_not_add_site_specific_directory_seeds() {
        let urls = direct_urls(
            "site:qidian.com Search top 10 free fantasy 玄幻 novels Qidian ranking recommendation",
        );

        assert!(urls.is_empty());
    }

    #[test]
    fn generic_search_filter_rejects_unrelated_lexical_results() {
        let query = "list of popular downloadable fantasy novels for writing reference";

        assert!(!search_candidate_matches_query_terms(
            query,
            "What is the difference between list [1] and list [1:] in Python?",
            "https://stackoverflow.com/questions/123",
            "Python slicing question"
        ));
        assert!(search_candidate_matches_query_terms(
            query,
            "Public domain fantasy novels available as ebooks",
            "https://example.org/public-domain-fantasy",
            "A catalog of fantasy novels with downloadable text"
        ));
    }

    #[test]
    fn chinese_search_filter_rejects_results_matching_only_access_modifiers() {
        let query = "热门免费玄幻奇幻小说下载 完整内容";

        assert!(!search_candidate_matches_query_terms(
            query,
            "免费 在线游戏 - 在 Playhop.com 上 免费 玩",
            "https://playhop.com/zh",
            "免费在线小游戏，立即开始游玩"
        ));
        assert!(search_candidate_matches_query_terms(
            query,
            "热门玄幻小说免费下载合集",
            "https://example.com/fantasy",
            "收录奇幻与玄幻小说条目"
        ));
    }

    #[test]
    fn mixed_chinese_english_query_filter_accepts_chinese_core_match() {
        let query = "热门免费玄幻小说下载 downloadable fantasy novels";

        assert!(search_candidate_matches_query_terms(
            query,
            "玄幻小说排行榜：热门免费小说合集",
            "https://example.com/rank",
            "包含多部玄幻小说条目和正文页面"
        ));
        assert!(!search_candidate_matches_query_terms(
            query,
            "Free online arcade games",
            "https://example.com/games",
            "downloadable browser games and fantasy sports"
        ));
    }

    #[test]
    fn chinese_public_text_query_filter_rejects_unrequested_translated_catalog_match() {
        assert!(!search_candidate_matches_query_terms(
            "热门免费玄幻奇幻小说下载 列表",
            "The Enchanted Castle",
            "https://www.gutenberg.org/files/34219/34219-0.txt",
            "Public downloadable text catalog record. Subjects: Fantasy fiction. Plain text: https://www.gutenberg.org/files/34219/34219-0.txt"
        ));
    }

    #[test]
    fn explicit_cross_language_public_text_query_accepts_translated_catalog_match() {
        assert!(search_candidate_matches_query_terms(
            "热门免费英文 fantasy 小说下载 列表",
            "The Enchanted Castle",
            "https://www.gutenberg.org/files/34219/34219-0.txt",
            "Public downloadable text catalog record. Subjects: Fantasy fiction. Plain text: https://www.gutenberg.org/files/34219/34219-0.txt"
        ));
    }

    #[test]
    fn chinese_search_filter_rejects_generic_website_results() {
        let query = "热门免费玄幻小说排行榜 列表 网站";

        assert!(!search_candidate_matches_query_terms(
            query,
            "有没有可以一直免费看剧看电影的网站，不用充会员的?",
            "https://www.zhihu.com/question/1931",
            "整理一些免费网站入口和在线资源"
        ));
        assert!(search_candidate_matches_query_terms(
            query,
            "玄幻小说排行榜：热门免费小说合集",
            "https://example.com/rank",
            "包含多部玄幻小说条目和下载页面"
        ));
    }

    #[test]
    fn chinese_entity_query_keeps_school_identity_terms() {
        let query = "搜索 北京大学官网的校训是什么，并给出来源。";

        assert!(!search_candidate_matches_query_terms(
            query,
            "Beijing - 北京市人民政府门户网站",
            "https://www.beijing.gov.cn/index.html",
            "北京市政府的网上服务窗口，发布政策文件和政务服务信息"
        ));
        assert!(search_candidate_matches_query_terms(
            query,
            "北京大学校训",
            "https://www.pku.edu.cn/about.html",
            "北京大学官方网站介绍校训、校徽和学校概况"
        ));
    }

    #[test]
    fn natural_language_query_does_not_add_site_specific_directory_seeds() {
        let urls = direct_urls("起点中文网 免费 玄幻 小说 推荐榜 前十");

        assert!(urls.is_empty());
    }

    #[test]
    fn cjk_query_adds_chinese_public_search_adapter() {
        let plan = build_search_plan("中国福利彩票 开奖结果 最近两个月");
        let source_names = plan
            .candidate_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>();
        assert!(source_names.contains(&"sogou"));
        assert!(source_names.contains(&"so"));
        assert!(source_names
            .windows(2)
            .any(|window| window == ["bing", "sogou"]));
        assert!(source_names
            .windows(2)
            .any(|window| window == ["sogou", "so"]));
    }

    #[test]
    fn parses_sogou_result_with_cite_url() {
        let source = source("sogou", 100);
        let html = r#"
            <h3 class="vr-title"><a href="/link?url=abc">中国福彩网_公益福彩_中国福利彩票官方网站</a></h3>
            <p class="str_info">开奖公告 双色球 福彩3D 七乐彩</p>
            <a class="citeLinkClass" href="http://www.cwl.gov.cn/">www.cwl.gov.cn</a>
        "#;
        let results = parse_sogou_results(html, &source, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "http://www.cwl.gov.cn/");
        assert_eq!(results[0].title, "中国福彩网_公益福彩_中国福利彩票官方网站");
        assert!(results[0].snippet.contains("开奖公告"));
        assert!(!results[0].title.contains("class="));
        assert!(!results[0].snippet.contains("class="));
    }

    #[test]
    fn parses_sogou_result_without_panicking_on_multibyte_window() {
        let source = source("sogou", 100);
        let prefix = "能".repeat(587);
        let html = format!(
            r#"
            <h3 class="vr-title"><a href="/link?url=abc">中国福利彩票 开奖数据</a></h3>
            <p class="str_info">{prefix}</p>
            <a class="citeLinkClass" href="http://www.example.cn/kjxx/">www.example.cn</a>
        "#
        );
        let results = parse_sogou_results(&html, &source, 5);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "http://www.example.cn/kjxx/");
    }

    #[test]
    fn parses_so_result_with_mdurl_and_snippet() {
        let source = source("so", 100);
        let html = r#"
            <ul class="result"><li class="res-list">
              <h3 class="res-title">
                <a href="https://www.so.com/link?m=abc" data-mdurl="https://www.example.com/category/fantasy" target="_blank">
                  <em>Fantasy</em> rankings - free novels
                </a>
              </h3>
              <p class="res-desc">Popular free fantasy novels with online reading and download pages.</p>
            </li></ul>
        "#;

        let results = parse_so_results(html, &source, 5);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.example.com/category/fantasy");
        assert_eq!(results[0].title, "Fantasy rankings - free novels");
        assert!(results[0].snippet.contains("download pages"));
    }

    #[test]
    fn fuses_and_deduplicates_candidates() {
        let plan = build_search_plan("example");
        let source = SourceCapability::Public;
        let a = SearchCandidate {
            title: "A".to_string(),
            url: "https://example.com/a?utm=1".to_string(),
            snippet: "first".to_string(),
            source: "wikipedia".to_string(),
            capability: source.clone(),
            rank: 1,
            score: 0.0,
        };
        let b = SearchCandidate {
            title: "A duplicate".to_string(),
            url: "https://example.com/a".to_string(),
            snippet: "second".to_string(),
            source: "browser".to_string(),
            capability: source,
            rank: 1,
            score: 0.0,
        };
        let fused = fuse_ranked_lists(vec![vec![a], vec![b]], &plan, "example");
        assert_eq!(fused.len(), 1);
        assert!(fused[0].score > 0.0);
    }

    #[test]
    fn quality_adjustment_prefers_official_sources_over_social_mirrors() {
        let plan = build_search_plan("北京大学官网 校训");
        let source = SourceCapability::Public;
        let mirror = SearchCandidate {
            title: "【今日分享】北京大学校训".to_string(),
            url: "http://mp.weixin.qq.com/s?src=11&timestamp=1".to_string(),
            snippet: "转载内容".to_string(),
            source: "sogou".to_string(),
            capability: source.clone(),
            rank: 1,
            score: 0.0,
        };
        let official = SearchCandidate {
            title: "北京大学".to_string(),
            url: "https://www.pku.edu.cn/".to_string(),
            snippet: "北京大学官网".to_string(),
            source: "bing".to_string(),
            capability: source,
            rank: 2,
            score: 0.0,
        };

        let fused = fuse_ranked_lists(vec![vec![mirror, official]], &plan, "北京大学官网 校训");

        assert_eq!(fused[0].url, "https://www.pku.edu.cn/");
    }

    #[test]
    fn document_lookup_prefers_direct_pdf_over_social_mirror() {
        let plan = build_search_plan("尝试搜索一个比特币白皮书 pdf");
        let source = SourceCapability::Public;
        let mirror = SearchCandidate {
            title: "比特币白皮书(中文版)".to_string(),
            url: "http://mp.weixin.qq.com/s?src=11&timestamp=1".to_string(),
            snippet: "转载的比特币白皮书内容".to_string(),
            source: "sogou".to_string(),
            capability: source.clone(),
            rank: 1,
            score: 0.0,
        };
        let direct_pdf = SearchCandidate {
            title: "Bitcoin: A Peer-to-Peer Electronic Cash System PDF".to_string(),
            url: "https://bitcoin.org/bitcoin.pdf".to_string(),
            snippet: "Satoshi Nakamoto whitepaper".to_string(),
            source: "bing".to_string(),
            capability: source,
            rank: 2,
            score: 0.0,
        };

        let fused = fuse_ranked_lists(
            vec![vec![mirror, direct_pdf]],
            &plan,
            "尝试搜索一个比特币白皮书 pdf",
        );

        assert_eq!(fused[0].url, "https://bitcoin.org/bitcoin.pdf");
    }

    #[test]
    fn direct_url_candidates_keep_explicit_urls_ahead_of_search_noise() {
        let candidates = direct_url_candidates(
            "Find out the purpose of website example.com and summarize it in Chinese.",
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source, "direct_url");
        assert_eq!(candidates[0].url, "https://example.com/");
    }

    #[test]
    fn direct_url_detection_ignores_site_scope_hints() {
        let urls = direct_urls("site:github.com search browser-use");

        assert!(urls.is_empty());
    }

    #[test]
    fn registry_marks_social_sources_as_session_or_challenge_prone() {
        let registry = registered_source_adapters();
        let twitter = registry
            .iter()
            .find(|source| source.name == "twitter")
            .expect("twitter profile");
        assert!(twitter.requires_browser);
        assert!(twitter.requires_auth);
        assert!(twitter.challenge_prone);
    }

    #[test]
    fn registry_contains_public_semi_public_and_authenticated_profiles() {
        let registry = registered_source_adapters();
        let bbc = registry
            .iter()
            .find(|source| source.name == "bbc")
            .expect("bbc profile");
        assert_eq!(bbc.capability, SourceCapability::Public);
        assert!(!bbc.requires_browser);
        assert!(!bbc.challenge_prone);

        let bilibili = registry
            .iter()
            .find(|source| source.name == "bilibili")
            .expect("bilibili profile");
        assert_eq!(bilibili.capability, SourceCapability::Browser);
        assert!(!bilibili.requires_auth);
        assert!(bilibili.challenge_prone);
        assert!(bilibili.fallback_sources.contains(&"browser".to_string()));

        let barchart = registry
            .iter()
            .find(|source| source.name == "barchart")
            .expect("barchart profile");
        assert_eq!(barchart.capability, SourceCapability::Cookie);
        assert!(barchart.requires_browser);
        assert!(barchart.requires_auth);
    }

    #[test]
    fn registry_classifies_remaining_site_profiles() {
        let registry = registered_source_adapters();
        let find = |name: &str| {
            registry
                .iter()
                .find(|source| source.name == name)
                .unwrap_or_else(|| panic!("{name} profile"))
        };

        for name in [
            "apple_podcasts",
            "devto",
            "stackoverflow",
            "huggingface",
            "sinafinance",
            "steam",
            "xiaoyuzhou",
        ] {
            let source = find(name);
            assert_eq!(source.capability, SourceCapability::Public, "{name}");
            assert!(!source.requires_browser, "{name}");
            assert!(!source.requires_auth, "{name}");
        }

        for name in [
            "v2ex", "douban", "medium", "substack", "linux_do", "sinablog", "smzdm",
        ] {
            let source = find(name);
            assert_eq!(source.capability, SourceCapability::Browser, "{name}");
            assert!(!source.requires_auth, "{name}");
            assert!(source.challenge_prone, "{name}");
        }

        for name in [
            "weibo",
            "boss",
            "ctrip",
            "coupang",
            "linkedin",
            "weixin",
            "xueqiu",
            "chaoxing",
            "grok",
            "jike",
            "jimeng",
            "weread",
            "yahoo_finance",
            "yollomi",
        ] {
            let source = find(name);
            assert_eq!(source.capability, SourceCapability::Cookie, "{name}");
            assert!(source.requires_browser, "{name}");
            assert!(source.requires_auth, "{name}");
            assert!(source.challenge_prone, "{name}");
        }
    }

    #[test]
    fn source_body_queries_drop_session_bound_candidates_when_no_readable_source_exists() {
        let candidates = vec![SearchCandidate {
            title: "小说合集丨135本天花板灵异玄幻文".to_string(),
            url: "http://mp.weixin.qq.com/s?src=11&timestamp=1".to_string(),
            snippet: "热门玄幻小说 正文 下载 资源".to_string(),
            source: "sogou".to_string(),
            capability: SourceCapability::Public,
            rank: 1,
            score: 1.0,
        }];
        let plan = build_search_plan("热门玄幻小说 正文 下载 资源");

        let filtered = filter_candidates_by_source_access_fit(
            candidates,
            "热门玄幻小说 正文 下载 资源",
            &plan,
        );

        assert!(filtered.is_empty(), "{filtered:?}");
    }

    #[test]
    fn source_body_queries_keep_readable_candidates_before_session_bound_candidates() {
        let candidates = vec![
            SearchCandidate {
                title: "小说合集丨135本天花板灵异玄幻文".to_string(),
                url: "http://mp.weixin.qq.com/s?src=11&timestamp=1".to_string(),
                snippet: "热门玄幻小说 正文 下载 资源".to_string(),
                source: "sogou".to_string(),
                capability: SourceCapability::Public,
                rank: 1,
                score: 1.0,
            },
            SearchCandidate {
                title: "示例玄幻小说正文下载".to_string(),
                url: "https://example.org/xuanhuan/fulltext.txt".to_string(),
                snippet: "玄幻小说 正文 下载 TXT".to_string(),
                source: "bing".to_string(),
                capability: SourceCapability::Public,
                rank: 2,
                score: 0.5,
            },
        ];
        let plan = build_search_plan("热门玄幻小说 正文 下载 资源");

        let filtered = filter_candidates_by_source_access_fit(
            candidates,
            "热门玄幻小说 正文 下载 资源",
            &plan,
        );

        assert_eq!(filtered[0].url, "https://example.org/xuanhuan/fulltext.txt");
        assert_eq!(filtered[0].rank, 1);
    }

    #[test]
    fn source_api_query_keeps_core_code_terms() {
        let query = source_api_query(
            "site:github.com site:api.github.com 搜索 GitHub agent-browser 相关的高星项目 请返回 URL 并确认 web_search browser",
        );

        assert_eq!(query, "agent-browser");
    }

    #[test]
    fn source_api_query_prefers_project_token_from_natural_language() {
        let query = source_api_query(
            "Search for information regarding browser-use agent browser and provide candidates with their source links",
        );

        assert_eq!(query, "browser-use");
    }

    #[test]
    fn github_relevance_filter_drops_broad_high_star_noise() {
        let candidates = vec![
            SearchCandidate {
                title: "browser-use/browser-use".to_string(),
                url: "https://github.com/browser-use/browser-use".to_string(),
                snippet: "Make websites accessible for AI agents".to_string(),
                source: "github".to_string(),
                capability: SourceCapability::Public,
                rank: 1,
                score: 0.0,
            },
            SearchCandidate {
                title: "animate-css/animate.css".to_string(),
                url: "https://github.com/animate-css/animate.css".to_string(),
                snippet: "A cross-browser animation library that is easy to use".to_string(),
                source: "github".to_string(),
                capability: SourceCapability::Public,
                rank: 2,
                score: 0.0,
            },
        ];

        let filtered =
            filter_candidates_by_query(candidates, "browser-use agent browser official docs");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "browser-use/browser-use");
    }

    #[test]
    fn relevance_filter_drops_cross_domain_raw_search_noise() {
        assert!(!search_candidate_matches_query_terms(
            "热门免费玄幻奇幻小说下载 资源",
            "有没有可以一直免费看剧看电影的网站，不用充会员的? - 知乎",
            "https://www.zhihu.com/question/1931",
            "免费电影、电视剧、会员、视频资源讨论"
        ));
        assert!(search_candidate_matches_query_terms(
            "热门免费玄幻奇幻小说下载 资源",
            "热门免费玄幻小说全集下载",
            "https://example.com/xuanhuan-novel",
            "玄幻小说下载与在线阅读资源列表"
        ));
    }

    #[test]
    fn query_filter_returns_empty_when_no_candidate_matches_intent_terms() {
        let candidates = vec![SearchCandidate {
            title: "Free movie streaming sites".to_string(),
            url: "https://example.com/movies".to_string(),
            snippet: "Watch drama and movies online".to_string(),
            source: "bing".to_string(),
            capability: SourceCapability::Public,
            rank: 1,
            score: 0.0,
        }];

        let filtered = filter_candidates_by_query(candidates, "free fantasy novel download");

        assert!(filtered.is_empty());
    }

    #[test]
    fn project_token_filter_drops_split_token_discussion_noise() {
        let candidates = vec![
            SearchCandidate {
                title: "Launch HN: Browser Use (YC W25) - open-source web agents".to_string(),
                url: "https://github.com/browser-use/browser-use".to_string(),
                snippet: "259 points, 100 comments".to_string(),
                source: "hackernews".to_string(),
                capability: SourceCapability::Public,
                rank: 1,
                score: 0.0,
            },
            SearchCandidate {
                title: "History of the browser user-agent string".to_string(),
                url: "https://example.com/user-agent".to_string(),
                snippet: "A discussion about browser user-agent strings".to_string(),
                source: "hackernews".to_string(),
                capability: SourceCapability::Public,
                rank: 2,
                score: 0.0,
            },
        ];

        let filtered = filter_candidates_by_project_token(
            candidates,
            "搜索最近 browser-use agent browser 的相关新闻或讨论",
        );

        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].title.contains("Browser Use"));
    }
}
