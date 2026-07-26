//! Web Fetch Tool
//!
//! Fetches content from URLs and converts HTML to clean Markdown.
//! Handles: HTML pages, JSON APIs, plain text.
//!
//! Feature-gated behind `http`.

use async_trait::async_trait;
use encoding_rs::{Encoding, GB18030, UTF_8};
use regex::Regex;
use reqwest::{Client, Url};
use serde::Deserialize;
use std::fmt;
use std::time::Duration;
use tokio::net::lookup_host;
use tracing::{debug, warn};

use crate::net_safety;
use crate::tool::browser_site_policy::{policy_for_url, SiteFetchMode};
use benshu_compression::json::compact_known_json_api_response;
use benshu_compression::{head_with_notice, TruncationNotice};
use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};
use benshu_routing::{
    build_pending_verification_followup_plan, build_pending_verification_result_envelope,
    build_source_observed_followup_plan, build_verified_verification_result_envelope,
    route_reason_for_plan, QueryVerificationPlan, VerificationDomain, VerificationMode,
    VerificationRequirement, VerificationSource, WebVerificationOrchestrator,
};

/// Maximum response body size (2 MB).
const MAX_BODY_SIZE: usize = 2 * 1024 * 1024;
/// Maximum output length returned to the Agent (to save tokens).
const MAX_OUTPUT_CHARS: usize = 12_000;
/// Default request timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// Maximum static fetch retries before falling through.
const DEFAULT_MAX_RETRIES: u8 = 1;
/// Initial retry backoff for transient failures.
const DEFAULT_RETRY_BACKOFF: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
struct WebFetchObservation {
    url: String,
    content_type: String,
    content: String,
    backend: String,
    site_policy: String,
    links: Vec<WebFetchLink>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct WebFetchLink {
    text: String,
    url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserFallbackReason {
    AccessBlocked,
    RateLimited,
}

#[derive(Debug, Clone)]
struct StaticFetchError {
    message: String,
    retryable: bool,
    browser_fallback: Option<BrowserFallbackReason>,
}

impl fmt::Display for StaticFetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StaticFetchError {}

/// Web fetch tool configuration.
#[derive(Debug, Clone)]
pub struct WebFetchConfig {
    /// Request timeout
    pub timeout: Duration,
    /// Maximum response body size in bytes
    pub max_body_size: usize,
    /// Maximum output characters
    pub max_output_chars: usize,
    /// Maximum retries for transient failures
    pub max_retries: u8,
    /// Base retry backoff
    pub retry_backoff: Duration,
    /// Blocked URL patterns (security)
    pub blocked_patterns: Vec<String>,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            max_body_size: MAX_BODY_SIZE,
            max_output_chars: MAX_OUTPUT_CHARS,
            max_retries: DEFAULT_MAX_RETRIES,
            retry_backoff: DEFAULT_RETRY_BACKOFF,
            blocked_patterns: vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "0.0.0.0".to_string(),
                "[::1]".to_string(),
                "169.254.".to_string(), // Link-local
                "10.".to_string(),      // Private
                "192.168.".to_string(), // Private
                "172.16.".to_string(),  // Private
            ],
        }
    }
}

/// Web fetch tool — retrieves content from a URL and returns clean text.
pub struct WebFetchTool {
    config: WebFetchConfig,
    client: Client,
}

#[derive(Debug, Deserialize)]
struct WebFetchArgs {
    url: String,
    #[serde(default)]
    structured: bool,
}

impl WebFetchTool {
    /// Create a new web fetch tool.
    pub fn new(config: WebFetchConfig) -> Result<Self, Error> {
        let client = Client::builder()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::limited(5))
            .cookie_store(true)
            .user_agent("Mozilla/5.0 (compatible; BenShu-Bot/1.0)")
            .build()
            .map_err(|e| Error::Internal(format!("HTTP client error: {}", e)))?;

        Ok(Self { config, client })
    }

    /// Create with defaults.
    pub fn with_defaults() -> Result<Self, Error> {
        Self::new(WebFetchConfig::default())
    }

    fn ip_targets_internal_resource(ip: std::net::IpAddr) -> bool {
        net_safety::ip_targets_internal_resource(ip)
    }

    fn ip_is_fake_proxy_address(ip: std::net::IpAddr) -> bool {
        net_safety::ip_is_fake_proxy_address(ip)
    }

    fn host_looks_public(host: &str) -> bool {
        net_safety::host_looks_public(host)
    }

    fn resolved_addresses_require_block(
        host: &str,
        resolved_ips: &[std::net::IpAddr],
    ) -> Option<std::net::IpAddr> {
        let first_internal = resolved_ips
            .iter()
            .copied()
            .find(|ip| Self::ip_targets_internal_resource(*ip));

        let Some(blocked_ip) = first_internal else {
            return None;
        };

        let looks_like_fake_proxy = !resolved_ips.is_empty()
            && resolved_ips
                .iter()
                .all(|ip| Self::ip_is_fake_proxy_address(*ip));

        if looks_like_fake_proxy && Self::host_looks_public(host) {
            return None;
        }

        Some(blocked_ip)
    }

    /// Security check: validate URL isn't targeting internal resources.
    async fn validate_url(&self, url: &str) -> anyhow::Result<()> {
        let lower = url.to_lowercase();

        let public_url = net_safety::validate_public_http_url(url).map_err(anyhow::Error::msg)?;

        // Parse URL to extract host for robust SSRF checking
        let parsed = reqwest::Url::parse(url).map_err(|e| anyhow::anyhow!("Invalid URL: {}", e))?;
        let host = public_url.host.as_str();

        // Check if host resolves to a private/loopback IP (covers hex/octal tricks)
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            if Self::ip_targets_internal_resource(ip) {
                anyhow::bail!("URL blocked: targets internal/private IP address {}", ip);
            }
        }

        // Check blocked patterns (SSRF protection — defense in depth)
        for pattern in &self.config.blocked_patterns {
            if lower.contains(pattern) {
                anyhow::bail!("URL blocked by security policy: contains '{}'", pattern);
            }
        }

        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| anyhow::anyhow!("Unable to determine port for URL"))?;
        if let Ok(resolved) = lookup_host((host, port)).await {
            let resolved_ips: Vec<std::net::IpAddr> =
                resolved.map(|socket_addr| socket_addr.ip()).collect();
            if let Some(ip) = Self::resolved_addresses_require_block(host, &resolved_ips) {
                anyhow::bail!(
                    "URL blocked: host resolves to internal/private IP address {}",
                    ip
                );
            }
            if !resolved_ips.is_empty()
                && resolved_ips
                    .iter()
                    .all(|ip| Self::ip_is_fake_proxy_address(*ip))
                && Self::host_looks_public(host)
            {
                warn!(
                    host = host,
                    resolved = ?resolved_ips,
                    "Allowing public hostname resolved through fake-ip proxy addresses"
                );
            }
        }

        Ok(())
    }

    fn classify_http_status(status: reqwest::StatusCode) -> StaticFetchError {
        let retryable = matches!(
            status,
            reqwest::StatusCode::REQUEST_TIMEOUT
                | reqwest::StatusCode::TOO_EARLY
                | reqwest::StatusCode::TOO_MANY_REQUESTS
                | reqwest::StatusCode::INTERNAL_SERVER_ERROR
                | reqwest::StatusCode::BAD_GATEWAY
                | reqwest::StatusCode::SERVICE_UNAVAILABLE
                | reqwest::StatusCode::GATEWAY_TIMEOUT
        );
        let browser_fallback = match status {
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                Some(BrowserFallbackReason::AccessBlocked)
            }
            reqwest::StatusCode::TOO_MANY_REQUESTS => Some(BrowserFallbackReason::RateLimited),
            _ => None,
        };
        StaticFetchError {
            message: format!(
                "HTTP {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            ),
            retryable,
            browser_fallback,
        }
    }

    fn classify_request_error(&self, error: reqwest::Error, url: &str) -> StaticFetchError {
        if error.is_timeout() {
            return StaticFetchError {
                message: format!(
                    "Request timed out after {:?} for {}",
                    self.config.timeout, url
                ),
                retryable: true,
                browser_fallback: None,
            };
        }
        if error.is_connect() {
            return StaticFetchError {
                message: format!("Failed to connect to {}", url),
                retryable: true,
                browser_fallback: None,
            };
        }
        StaticFetchError {
            message: format!("Request failed: {}", error),
            retryable: false,
            browser_fallback: None,
        }
    }

    fn retry_delay(&self, attempt: u8) -> Duration {
        let multiplier = 1u32 << attempt.saturating_sub(1);
        self.config.retry_backoff.saturating_mul(multiplier)
    }

    #[cfg(feature = "browser")]
    async fn fetch_browser_snapshot_observation(
        &self,
        url: &str,
        reason: BrowserFallbackReason,
    ) -> anyhow::Result<WebFetchObservation> {
        let site_policy = policy_for_url(url);
        let content = crate::tool::browser::BrowserTool::snapshot_once(url, true, false)
            .await
            .map_err(|e| anyhow::anyhow!("Browser fallback failed: {}", e))?;
        Ok(WebFetchObservation {
            url: url.to_string(),
            content_type: "application/x-browser-snapshot".to_string(),
            content,
            backend: match reason {
                BrowserFallbackReason::AccessBlocked => "browser_snapshot_fallback_blocked",
                BrowserFallbackReason::RateLimited => "browser_snapshot_fallback_rate_limited",
            }
            .to_string(),
            site_policy: site_policy.policy_name.to_string(),
            links: Vec::new(),
        })
    }

    #[cfg(feature = "browser")]
    async fn fetch_browser_direct_observation(
        &self,
        url: &str,
        backend: &'static str,
    ) -> anyhow::Result<WebFetchObservation> {
        let site_policy = policy_for_url(url);
        let content = crate::tool::browser::BrowserTool::snapshot_once(url, true, false)
            .await
            .map_err(|e| anyhow::anyhow!("Browser fetch failed: {}", e))?;
        Ok(WebFetchObservation {
            url: url.to_string(),
            content_type: "application/x-browser-snapshot".to_string(),
            content,
            backend: backend.to_string(),
            site_policy: site_policy.policy_name.to_string(),
            links: Vec::new(),
        })
    }

    async fn fetch_static_url_observation(
        &self,
        url: &str,
    ) -> Result<WebFetchObservation, StaticFetchError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| self.classify_request_error(e, url))?;

        let status = response.status();
        if !status.is_success() {
            return Err(Self::classify_http_status(status));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        // Check content length
        if let Some(len) = response.content_length() {
            if len > self.config.max_body_size as u64 {
                return Err(StaticFetchError {
                    message: format!(
                        "Response too large ({} bytes, max {})",
                        len, self.config.max_body_size
                    ),
                    retryable: false,
                    browser_fallback: None,
                });
            }
        }

        let body = response.bytes().await.map_err(|e| StaticFetchError {
            message: format!("Failed to read body: {}", e),
            retryable: e.is_timeout(),
            browser_fallback: None,
        })?;

        if body.len() > self.config.max_body_size {
            return Err(StaticFetchError {
                message: format!(
                    "Response body too large ({} bytes, max {})",
                    body.len(),
                    self.config.max_body_size
                ),
                retryable: false,
                browser_fallback: None,
            });
        }

        let text = decode_body_text(&body, &content_type);

        // Process based on content type
        let mut links = Vec::new();
        let processed = if content_type.contains("json") {
            if let Some(compacted) = compact_known_json_api_response(url, &text, 5) {
                compacted
            // Pretty-print JSON
            } else {
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(v) => serde_json::to_string_pretty(&v).unwrap_or(text),
                    Err(_) => text,
                }
            }
        } else if content_type.contains("html") {
            links = extract_html_links(&text, url, 40);
            // Convert HTML to readable text
            html_to_text(&text)
        } else {
            // Plain text / other
            text
        };

        // Truncate if needed (char-safe to avoid panic on multi-byte characters)
        let content = head_with_notice(
            &processed,
            self.config.max_output_chars,
            TruncationNotice::Generic,
        )
        .content;

        Ok(WebFetchObservation {
            url: url.to_string(),
            content_type,
            content,
            backend: "http_fetch".to_string(),
            site_policy: policy_for_url(url).policy_name.to_string(),
            links,
        })
    }

    /// Fetch URL and return processed content.
    async fn fetch_url_observation(&self, url: &str) -> anyhow::Result<WebFetchObservation> {
        self.validate_url(url).await?;
        let site_policy = policy_for_url(url);

        #[cfg(feature = "browser")]
        match site_policy.mode {
            SiteFetchMode::BrowserOnly => {
                warn!(
                    url = url,
                    policy = site_policy.policy_name,
                    "Site policy requires browser-only fetch"
                );
                return self
                    .fetch_browser_direct_observation(url, "browser_snapshot_policy_browser_only")
                    .await;
            }
            SiteFetchMode::BrowserThenStatic => {
                warn!(
                    url = url,
                    policy = site_policy.policy_name,
                    "Site policy prefers browser fetch before static fallback"
                );
                match self
                    .fetch_browser_direct_observation(
                        url,
                        "browser_snapshot_policy_browser_then_static",
                    )
                    .await
                {
                    Ok(observation) => return Ok(observation),
                    Err(error) => {
                        warn!(
                            url = url,
                            error = %error,
                            "Browser-first policy fetch failed; falling back to static fetch"
                        );
                    }
                }
            }
            SiteFetchMode::StaticOnly | SiteFetchMode::StaticThenBrowser => {}
        }

        let mut attempt = 0u8;
        loop {
            match self.fetch_static_url_observation(url).await {
                Ok(observation) => {
                    #[cfg(feature = "browser")]
                    if matches!(site_policy.mode, SiteFetchMode::StaticThenBrowser) {
                        let quality =
                            web_fetch_content_quality(&observation.url, &observation.content);
                        if matches!(
                            quality,
                            WebFetchContentQuality::Empty
                                | WebFetchContentQuality::TooShort
                                | WebFetchContentQuality::BoilerplateOnly
                                | WebFetchContentQuality::ChallengeOrBlocked
                        ) {
                            warn!(
                                url = url,
                                quality = quality.as_str(),
                                "Static web fetch produced low-quality content; escalating to browser snapshot"
                            );
                            match self
                                .fetch_browser_direct_observation(
                                    url,
                                    "browser_snapshot_fallback_low_quality_static",
                                )
                                .await
                            {
                                Ok(browser_observation) => return Ok(browser_observation),
                                Err(error) => {
                                    warn!(
                                        url = url,
                                        error = %error,
                                        "Browser fallback for low-quality static fetch failed; returning static observation"
                                    );
                                }
                            }
                        }
                    }

                    return Ok(observation);
                }
                Err(error) => {
                    if matches!(site_policy.mode, SiteFetchMode::StaticOnly) {
                        return Err(anyhow::anyhow!(
                            "{} (site policy: {} requires static/API fetch only)",
                            error,
                            site_policy.policy_name
                        ));
                    }
                    if error.retryable && attempt < self.config.max_retries {
                        attempt += 1;
                        let delay = self.retry_delay(attempt);
                        warn!(
                            url = url,
                            attempt = attempt,
                            delay_ms = delay.as_millis(),
                            error = %error,
                            "Static web fetch failed; retrying"
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }

                    #[cfg(feature = "browser")]
                    if let Some(reason) = error.browser_fallback {
                        warn!(
                            url = url,
                            error = %error,
                            "Static web fetch encountered anti-bot or rate limiting; escalating to browser snapshot"
                        );
                        return self.fetch_browser_snapshot_observation(url, reason).await;
                    }

                    return Err(anyhow::anyhow!(error));
                }
            }
        }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> String {
        "web_fetch".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_fetch".to_string(),
            description: "Fetch content from a URL. Returns the page content as clean text. Supports HTML (converted to readable text), JSON (pretty-printed), and plain text.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch (must be http:// or https://)"
                    },
                    "structured": {
                        "type": "boolean",
                        "description": "When true, return a structured payload with verification preview instead of only page content."
                    }
                },
                "required": ["url"]
            }),
            parameters_ts: Some("interface WebFetch {\n  url: string; // The URL to fetch (http/https only)\n  structured?: boolean; // Return verification-aware structured payload\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use to read the content of a specific public web page or feed URL. Prefer `web_search` to find URLs first, then `web_fetch` to read the content.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: WebFetchArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Invalid arguments: {}", e))?;

        let url = args.url.trim();
        if url.is_empty() {
            anyhow::bail!("URL cannot be empty");
        }

        debug!(url = url, "Fetching URL");
        let observation = self.fetch_url_observation(url).await?;
        if args.structured {
            render_structured_web_fetch_payload(observation)
        } else {
            Ok(render_web_fetch_receipt(observation))
        }
    }
}

fn render_web_fetch_receipt(observation: WebFetchObservation) -> String {
    format!(
        "status: completed\n\
         executed_tool: web_fetch\n\
         source_url: {}\n\
         content_type: {}\n\
         backend: {}\n\
         site_policy: {}\n\
         result:\n{}",
        observation.url,
        observation.content_type,
        observation.backend,
        observation.site_policy,
        observation.content.trim()
    )
}

fn render_structured_web_fetch_payload(observation: WebFetchObservation) -> anyhow::Result<String> {
    let verification_plan = QueryVerificationPlan {
        domain: VerificationDomain::KnowledgeFact,
        requirement: VerificationRequirement::Required,
        mode: VerificationMode::WebSearchFetch,
        route_hint: None,
    };
    let content_quality = web_fetch_content_quality(&observation.url, &observation.content);
    let content_is_actionable = matches!(content_quality, WebFetchContentQuality::Actionable);
    let verification_preview = if content_is_actionable {
        build_verified_verification_result_envelope(
            VerificationDomain::KnowledgeFact,
            VerificationMode::WebSearchFetch,
            vec![VerificationSource {
                kind: "web_page".to_string(),
                title: "Fetched URL".to_string(),
                uri: observation.url.clone(),
                observed_at: Some(chrono::Utc::now().to_rfc3339()),
            }],
            "web fetch completed for a specific source",
        )
    } else {
        build_pending_verification_result_envelope(
            verification_plan,
            true,
            format!(
                "web fetch returned low-information or challenge-like content: {}",
                content_quality.as_str()
            ),
        )
    };
    let verification_followup = if content_is_actionable {
        build_source_observed_followup_plan(true)
    } else {
        build_pending_verification_followup_plan(VerificationMode::WebSearchFetch)
    };
    let orchestration_decision = WebVerificationOrchestrator::new().decide(
        Some(&verification_plan),
        Some(&verification_preview),
        Some(&verification_followup),
    );
    let payload = serde_json::json!({
        "kind": "web_fetch",
        "url": observation.url,
        "fetched_at": chrono::Utc::now().to_rfc3339(),
        "content_type": observation.content_type,
        "backend": observation.backend,
        "site_policy": observation.site_policy,
        "content_quality": content_quality.as_str(),
        "links": observation.links,
        "route_reason": route_reason_for_plan(Some(&verification_plan)).as_str(),
        "verification_preview": verification_preview,
        "verification_followup": verification_followup,
        "orchestration_decision": orchestration_decision,
        "content": observation.content,
    });
    Ok(serde_json::to_string_pretty(&payload)?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebFetchContentQuality {
    Actionable,
    Empty,
    TooShort,
    BoilerplateOnly,
    ChallengeOrBlocked,
}

impl WebFetchContentQuality {
    fn as_str(self) -> &'static str {
        match self {
            Self::Actionable => "actionable",
            Self::Empty => "empty",
            Self::TooShort => "too_short",
            Self::BoilerplateOnly => "boilerplate_only",
            Self::ChallengeOrBlocked => "challenge_or_blocked",
        }
    }
}

fn web_fetch_content_quality(url: &str, content: &str) -> WebFetchContentQuality {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return WebFetchContentQuality::Empty;
    }

    let replacement_count = trimmed.chars().filter(|ch| *ch == '\u{FFFD}').count();
    if replacement_count >= 8 && replacement_count * 20 > trimmed.chars().count() {
        return WebFetchContentQuality::BoilerplateOnly;
    }

    let lowered = trimmed.to_ascii_lowercase();
    if lowered.contains("cloudflare")
        || lowered.contains("cloudfront")
        || lowered.contains("403 error")
        || lowered.contains("request blocked")
        || lowered.contains("enable javascript and cookies to continue")
        || lowered.contains("security verification")
        || lowered.contains("anti-bot")
        || lowered.contains("challenge page")
    {
        return WebFetchContentQuality::ChallengeOrBlocked;
    }
    if lowered.contains("probe.js")
        || lowered.contains("securitycheck")
        || lowered.contains("window._cf_chl_opt")
        || (lowered.contains("<body></body>") && lowered.contains("<script"))
    {
        return WebFetchContentQuality::ChallengeOrBlocked;
    }

    let word_count = trimmed.split_whitespace().count();
    let lowered_url = url.to_ascii_lowercase();
    let looks_like_youtube_shell = lowered_url.contains("youtube.com")
        && lowered.contains("aboutpresscopyrightcontact")
        && lowered.contains("how youtube works")
        && !lowered.contains("watch?v=");
    if looks_like_youtube_shell {
        return WebFetchContentQuality::BoilerplateOnly;
    }

    if word_count < 8 && trimmed.len() < 120 {
        return WebFetchContentQuality::TooShort;
    }

    WebFetchContentQuality::Actionable
}

fn decode_body_text(body: &[u8], content_type: &str) -> String {
    if let Some(label) = charset_label_from_content_type(content_type) {
        if let Some(encoding) = Encoding::for_label(label.as_bytes()) {
            return decode_with_encoding(body, encoding);
        }
    }

    let utf8 = decode_with_encoding(body, UTF_8);
    if text_has_too_many_replacement_chars(&utf8) {
        let gb18030 = decode_with_encoding(body, GB18030);
        if !text_has_too_many_replacement_chars(&gb18030) {
            return gb18030;
        }
    }
    utf8
}

fn decode_with_encoding(body: &[u8], encoding: &'static Encoding) -> String {
    let (text, _, _) = encoding.decode(body);
    text.into_owned()
}

fn charset_label_from_content_type(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| {
            if key.trim().eq_ignore_ascii_case("charset") {
                let value = value.trim().trim_matches(['"', '\'']).to_string();
                (!value.is_empty()).then_some(value)
            } else {
                None
            }
        })
}

fn text_has_too_many_replacement_chars(text: &str) -> bool {
    let char_count = text.chars().count();
    if char_count == 0 {
        return false;
    }
    let replacement_count = text.chars().filter(|ch| *ch == '\u{FFFD}').count();
    replacement_count >= 8 && replacement_count * 20 > char_count
}

// ─── HTML to Text Converter ────────────────────────────────────────────

fn extract_html_links(html: &str, base_url: &str, limit: usize) -> Vec<WebFetchLink> {
    let Ok(base) = Url::parse(base_url) else {
        return Vec::new();
    };
    let anchor_re =
        Regex::new(r#"(?is)<a\b[^>]*\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')[^>]*>(.*?)</a>"#)
            .expect("valid anchor regex");

    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();
    for captures in anchor_re.captures_iter(html) {
        let Some(raw_href) = captures
            .get(1)
            .or_else(|| captures.get(2))
            .map(|value| value.as_str().trim())
        else {
            continue;
        };
        if raw_href.is_empty()
            || raw_href.starts_with('#')
            || raw_href.starts_with("javascript:")
            || raw_href.starts_with("mailto:")
            || raw_href.starts_with("tel:")
        {
            continue;
        }

        let Ok(resolved) = base.join(raw_href) else {
            continue;
        };
        if !matches!(resolved.scheme(), "http" | "https") {
            continue;
        }

        let text = captures
            .get(3)
            .map(|value| html_to_text(value.as_str()))
            .unwrap_or_default();
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }

        let url = resolved.to_string();
        if !seen.insert(url.clone()) {
            continue;
        }

        links.push(WebFetchLink { text, url });
        if links.len() >= limit {
            break;
        }
    }

    links
}

/// Convert HTML to readable plain text.
/// This is a lightweight HTML-to-text converter that handles common cases.
fn html_to_text(html: &str) -> String {
    let mut result = String::with_capacity(html.len() / 3);
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut tag_name = String::new();
    let mut last_was_space = false;

    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        if ch == '<' {
            in_tag = true;
            tag_name.clear();
            i += 1;
            continue;
        }

        if in_tag {
            if ch == '>' {
                in_tag = false;
                let tag_lower = tag_name.to_lowercase();

                // Track script/style blocks
                if tag_lower.starts_with("script") {
                    in_script = true;
                } else if tag_lower.starts_with("/script") {
                    in_script = false;
                } else if tag_lower.starts_with("style") {
                    in_style = true;
                } else if tag_lower.starts_with("/style") {
                    in_style = false;
                }

                // Block elements → newline
                let block_tags = [
                    "p",
                    "/p",
                    "div",
                    "/div",
                    "br",
                    "br/",
                    "br /",
                    "h1",
                    "/h1",
                    "h2",
                    "/h2",
                    "h3",
                    "/h3",
                    "h4",
                    "/h4",
                    "h5",
                    "/h5",
                    "h6",
                    "/h6",
                    "li",
                    "/li",
                    "tr",
                    "/tr",
                    "blockquote",
                    "/blockquote",
                    "hr",
                    "hr/",
                ];
                let clean_tag = tag_lower.split_whitespace().next().unwrap_or("");
                if block_tags.contains(&clean_tag) {
                    if !result.ends_with('\n') {
                        result.push('\n');
                    }
                    last_was_space = true;

                    // Add markdown-like prefix for headings
                    if clean_tag.starts_with('h') && clean_tag.len() == 2 {
                        if let Some(level) = clean_tag.chars().nth(1).and_then(|c| c.to_digit(10)) {
                            for _ in 0..level {
                                result.push('#');
                            }
                            result.push(' ');
                        }
                    }

                    // List items
                    if clean_tag == "li" {
                        result.push_str("• ");
                    }

                    // Horizontal rule
                    if clean_tag == "hr" || clean_tag == "hr/" {
                        result.push_str("---\n");
                    }
                }
            } else {
                tag_name.push(ch);
            }
            i += 1;
            continue;
        }

        // Skip script/style content
        if in_script || in_style {
            i += 1;
            continue;
        }

        // Handle HTML entities
        if ch == '&' {
            let rest: String = chars[i..].iter().take(10).collect();
            if rest.starts_with("&amp;") {
                result.push('&');
                i += 5;
                last_was_space = false;
                continue;
            } else if rest.starts_with("&lt;") {
                result.push('<');
                i += 4;
                last_was_space = false;
                continue;
            } else if rest.starts_with("&gt;") {
                result.push('>');
                i += 4;
                last_was_space = false;
                continue;
            } else if rest.starts_with("&quot;") {
                result.push('"');
                i += 6;
                last_was_space = false;
                continue;
            } else if rest.starts_with("&#39;") || rest.starts_with("&apos;") {
                result.push('\'');
                i += if rest.starts_with("&#39;") { 5 } else { 6 };
                last_was_space = false;
                continue;
            } else if rest.starts_with("&nbsp;") {
                result.push(' ');
                i += 6;
                last_was_space = true;
                continue;
            }
        }

        // Collapse whitespace
        if ch.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(ch);
            last_was_space = false;
        }

        i += 1;
    }

    // Clean up excessive blank lines
    let mut cleaned = String::with_capacity(result.len());
    let mut consecutive_newlines = 0;
    for ch in result.chars() {
        if ch == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                cleaned.push(ch);
            }
        } else {
            consecutive_newlines = 0;
            cleaned.push(ch);
        }
    }

    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fetch_budget_is_chat_fast() {
        let config = WebFetchConfig::default();
        assert_eq!(config.timeout, Duration::from_secs(10));
        assert_eq!(config.max_retries, 1);
    }

    #[test]
    fn test_html_to_text_basic() {
        assert_eq!(html_to_text("<p>Hello <b>world</b></p>"), "Hello world");
    }

    #[test]
    fn test_html_to_text_headings() {
        let html = "<h1>Title</h1><p>Content</p>";
        let text = html_to_text(html);
        assert!(text.contains("# Title"));
        assert!(text.contains("Content"));
    }

    #[test]
    fn test_html_to_text_scripts_removed() {
        let html = "<p>Before</p><script>alert('xss')</script><p>After</p>";
        let text = html_to_text(html);
        assert!(!text.contains("alert"));
        assert!(text.contains("Before"));
        assert!(text.contains("After"));
    }

    #[test]
    fn test_html_to_text_entities() {
        assert_eq!(html_to_text("a &amp; b &lt; c"), "a & b < c");
    }

    #[test]
    fn test_html_to_text_list() {
        let html = "<ul><li>One</li><li>Two</li></ul>";
        let text = html_to_text(html);
        assert!(text.contains("• One"));
        assert!(text.contains("• Two"));
    }

    #[test]
    fn test_extract_html_links_resolves_relative_links() {
        let html = r#"
            <a href="/">Home</a>
            <a href="/data/records/">Official data records</a>
            <a href="javascript:void(0)">Ignore</a>
        "#;
        let links = extract_html_links(html, "https://example.com/base/page.html", 10);

        assert_eq!(links.len(), 2);
        assert_eq!(links[0].url, "https://example.com/");
        assert_eq!(links[1].url, "https://example.com/data/records/");
        assert_eq!(links[1].text, "Official data records");
    }

    #[test]
    fn decode_body_text_respects_legacy_chinese_charset() {
        let (bytes, _, _) = GB18030.encode("免费小说在线阅读");
        let decoded = decode_body_text(bytes.as_ref(), "text/html; charset=gb2312");
        assert!(decoded.contains("免费小说"));
    }

    #[test]
    fn content_quality_rejects_replacement_garble() {
        let garble = "����������������������������������������";
        assert_eq!(
            web_fetch_content_quality("https://example.com/page", garble),
            WebFetchContentQuality::BoilerplateOnly
        );
    }

    #[tokio::test]
    async fn test_validate_url_blocks_internal() {
        let tool = WebFetchTool::with_defaults().unwrap();
        assert!(tool.validate_url("http://localhost:8080").await.is_err());
        assert!(tool.validate_url("http://127.0.0.1").await.is_err());
        assert!(tool.validate_url("http://192.168.1.1").await.is_err());
        assert!(tool.validate_url("ftp://example.com").await.is_err());
    }

    #[test]
    fn test_ip_targets_internal_resource() {
        assert!(WebFetchTool::ip_targets_internal_resource(
            "127.0.0.1".parse().unwrap()
        ));
        assert!(WebFetchTool::ip_targets_internal_resource(
            "172.20.1.10".parse().unwrap()
        ));
        assert!(WebFetchTool::ip_targets_internal_resource(
            "::1".parse().unwrap()
        ));
        assert!(!WebFetchTool::ip_targets_internal_resource(
            "8.8.8.8".parse().unwrap()
        ));
    }

    #[test]
    fn public_hostname_with_fake_proxy_addresses_is_allowed() {
        let resolved = vec![
            "198.19.235.130".parse().unwrap(),
            "fc00::19d:b573:eb82".parse().unwrap(),
        ];

        assert_eq!(
            WebFetchTool::resolved_addresses_require_block("www.thelancet.com", &resolved),
            None
        );
    }

    #[test]
    fn internal_hostname_with_private_resolution_still_blocks() {
        let resolved = vec!["fc00::1".parse().unwrap()];

        assert_eq!(
            WebFetchTool::resolved_addresses_require_block("vault.internal", &resolved),
            Some("fc00::1".parse().unwrap())
        );
    }

    #[test]
    fn structured_web_fetch_payload_contains_verification_preview() {
        let rendered = render_structured_web_fetch_payload(WebFetchObservation {
            url: "https://example.com".to_string(),
            content_type: "text/html".to_string(),
            content: "Example content with enough source detail to summarize safely.".to_string(),
            backend: "http_fetch".to_string(),
            site_policy: "default_static_then_browser".to_string(),
            links: Vec::new(),
        })
        .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(payload["kind"], "web_fetch");
        assert_eq!(payload["url"], "https://example.com");
        assert_eq!(payload["backend"], "http_fetch");
        assert_eq!(payload["site_policy"], "default_static_then_browser");
        assert_eq!(
            payload["verification_preview"]["outcome"],
            "VerificationSucceeded"
        );
        assert_eq!(
            payload["verification_followup"]["answer_readiness"],
            "source_content_observed"
        );
        assert_eq!(
            payload["route_reason"],
            "external_fact_requires_search_then_source_read"
        );
        assert_eq!(
            payload["orchestration_decision"]["termination"],
            "FinalizeWithSources"
        );
        assert_eq!(
            payload["content"],
            "Example content with enough source detail to summarize safely."
        );
    }

    #[test]
    fn plain_web_fetch_payload_contains_machine_readable_receipt() {
        let rendered = render_web_fetch_receipt(WebFetchObservation {
            url: "https://example.com/novel".to_string(),
            content_type: "text/html".to_string(),
            content: "Readable source body.".to_string(),
            backend: "http_fetch".to_string(),
            site_policy: "default_static_then_browser".to_string(),
            links: Vec::new(),
        });

        assert!(rendered.contains("status: completed"));
        assert!(rendered.contains("executed_tool: web_fetch"));
        assert!(rendered.contains("source_url: https://example.com/novel"));
        assert!(rendered.contains("result:\nReadable source body."));
    }

    #[test]
    fn structured_web_fetch_marks_youtube_shell_as_pending() {
        let rendered = render_structured_web_fetch_payload(WebFetchObservation {
            url: "https://www.youtube.com/results?search_query=agent+browser".to_string(),
            content_type: "text/html".to_string(),
            content: "AboutPressCopyrightContact usCreatorsAdvertiseDevelopersTermsPrivacyPolicy & SafetyHow YouTube worksTest new featuresNFL Sunday Ticket".to_string(),
            backend: "http_fetch".to_string(),
            site_policy: "static_first_video_source".to_string(),
            links: Vec::new(),
        })
        .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(payload["content_quality"], "boilerplate_only");
        assert_eq!(
            payload["verification_followup"]["answer_readiness"],
            "verification_pending"
        );
        assert_eq!(payload["orchestration_decision"]["termination"], "NotReady");
    }

    #[test]
    fn compact_known_json_api_response_keeps_github_search_actionable() {
        let raw = serde_json::json!({
            "total_count": 2,
            "incomplete_results": false,
            "items": [
                {
                    "full_name": "cline/cline",
                    "html_url": "https://github.com/cline/cline",
                    "stargazers_count": 61086,
                    "description": "Autonomous coding agent right in your IDE.",
                    "language": "TypeScript",
                    "updated_at": "2026-04-28T08:44:18Z",
                    "archive_url": "https://api.github.com/repos/cline/cline/{archive_format}{/ref}"
                }
            ]
        })
        .to_string();

        let compact = compact_known_json_api_response(
            "https://api.github.com/search/repositories?q=agent-browser&per_page=5",
            &raw,
            5,
        )
        .unwrap();

        assert!(compact.contains("https://github.com/cline/cline"));
        assert!(compact.contains("stargazers_count"));
        assert!(!compact.contains("archive_url"));
    }

    #[test]
    fn classify_http_status_marks_retryable_and_browser_fallbacks() {
        let too_many = WebFetchTool::classify_http_status(reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert!(too_many.retryable);
        assert_eq!(
            too_many.browser_fallback,
            Some(BrowserFallbackReason::RateLimited)
        );

        let forbidden = WebFetchTool::classify_http_status(reqwest::StatusCode::FORBIDDEN);
        assert!(!forbidden.retryable);
        assert_eq!(
            forbidden.browser_fallback,
            Some(BrowserFallbackReason::AccessBlocked)
        );

        let internal =
            WebFetchTool::classify_http_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(internal.retryable);
        assert_eq!(internal.browser_fallback, None);
    }
}
