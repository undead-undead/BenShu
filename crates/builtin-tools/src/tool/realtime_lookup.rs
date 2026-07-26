//! Specialized realtime lookup tools.
//!
//! These tools provide structured execution surfaces for weather, FX, price,
//! and latest-info queries so the runtime does not need to rely solely on
//! generic web search prompts.

use async_trait::async_trait;
use chrono::Datelike;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};
use benshu_routing::{
    build_source_observed_followup_plan, build_verification_followup_plan,
    build_verified_verification_result_envelope, route_reason_for_plan, QueryVerificationPlan,
    VerificationDomain, VerificationFollowupPlan, VerificationMode, VerificationOutcome,
    VerificationRequirement, VerificationSource, WebVerificationOrchestrator,
};

use super::{web_fetch::WebFetchTool, web_search::WebSearchTool};

mod policy;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_FETCHED_RESULTS: usize = 2;
const PRICE_MAX_FETCHED_RESULTS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchResultRecord {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RealtimeSourceRecord {
    title: String,
    url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    snippet: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    fetched_excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    published_at: Option<String>,
}

fn is_search_engine_host(host: &str) -> bool {
    matches!(
        host,
        "google.com"
            | "www.google.com"
            | "bing.com"
            | "www.bing.com"
            | "duckduckgo.com"
            | "www.duckduckgo.com"
            | "so.com"
            | "www.so.com"
            | "news.so.com"
            | "sogou.com"
            | "www.sogou.com"
            | "search.yahoo.com"
            | "www.yahoo.com"
    )
}

fn is_search_result_like_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let path = parsed.path().to_ascii_lowercase();
    let search_host = is_search_engine_host(&host);
    if !search_host {
        return false;
    }

    let has_search_query = parsed.query_pairs().any(|(key, _)| {
        matches!(
            key.as_ref().to_ascii_lowercase().as_str(),
            "q" | "query" | "wd" | "word" | "s" | "p"
        )
    });
    has_search_query
        || matches!(
            path.as_str(),
            "/search" | "/web" | "/ns" | "/html/" | "/html"
        )
}

fn source_record_is_usable(record: &RealtimeSourceRecord) -> bool {
    let url = record.url.trim();
    if url.is_empty() {
        return false;
    }

    if is_search_result_like_url(url) {
        return false;
    }

    if let Ok(parsed) = reqwest::Url::parse(url) {
        if parsed
            .host_str()
            .map(|host| is_search_engine_host(&host.to_ascii_lowercase()))
            .unwrap_or(false)
        {
            return false;
        }
    }

    let title = record.title.trim().to_lowercase();
    if title.is_empty() || title == "google" || title == "bing" || title == "yahoo" {
        return false;
    }

    !record.snippet.trim().is_empty() || !record.fetched_excerpt.trim().is_empty()
}

fn price_source_record_matches_symbol(symbol: &str, record: &RealtimeSourceRecord) -> bool {
    let aliases = price_symbol_aliases(symbol);
    if aliases.is_empty() {
        return false;
    }

    let title = record.title.to_ascii_lowercase();
    let url = record.url.to_ascii_lowercase();
    aliases
        .iter()
        .any(|alias| title.contains(alias) || url.contains(alias))
}

fn price_symbol_aliases(symbol: &str) -> Vec<String> {
    let normalized = symbol.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut aliases = vec![normalized.clone()];
    match normalized.as_str() {
        "btc" | "xbt" | "bitcoin" | "比特币" => {
            aliases.extend(["bitcoin".to_string(), "btc".to_string(), "xbt".to_string()]);
        }
        "eth" | "ethereum" | "以太坊" => {
            aliases.extend(["ethereum".to_string(), "eth".to_string()]);
        }
        _ => {}
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn price_source_mentions_stale_year(context: &str) -> bool {
    let current_year = chrono::Utc::now().year();
    for year in 1990..current_year {
        let compact = year.to_string();
        if context.contains(&compact)
            || context.contains(&format!("{year}年"))
            || context.contains(&format!("{year}-"))
            || context.contains(&format!("{year}/"))
        {
            return true;
        }
    }
    false
}

fn price_context_looks_like_current_quote(context: &str) -> bool {
    let lowered = context.to_ascii_lowercase();
    let live_quote_terms = [
        "price",
        "quote",
        "current",
        "latest",
        "live",
        "trading at",
        "trades at",
        "last price",
        "spot",
        "现价",
        "当前",
        "最新",
        "实时",
        "报价",
    ];
    let stale_or_accounting_terms = [
        "average purchase price",
        "aggregate purchase price",
        "purchase price",
        "acquired at",
        "bought at",
        "held approximately",
        "holdings",
        "treasury",
        "cost basis",
        "平均买入",
        "买入价",
        "购入价",
        "持有",
        "历史",
        "趋势",
        "回顾",
        "可视化",
        "historical",
        "history",
    ];

    live_quote_terms.iter().any(|term| lowered.contains(term))
        && !stale_or_accounting_terms
            .iter()
            .any(|term| lowered.contains(term))
        && !price_source_mentions_stale_year(context)
}

fn extract_current_price_from_source(
    symbol: &str,
    source: &RealtimeSourceRecord,
) -> Option<String> {
    if !price_source_record_matches_symbol(symbol, source) {
        return None;
    }
    let surface = format!(
        "{} {} {}",
        source.title, source.snippet, source.fetched_excerpt
    );
    if !price_context_looks_like_current_quote(&surface) {
        return None;
    }
    extract_observed_price_text(&surface)
}

fn extract_current_weather_from_source(
    location: &str,
    source: &RealtimeSourceRecord,
) -> Option<String> {
    let surface = format!(
        "{} {} {}",
        source.title, source.snippet, source.fetched_excerpt
    );
    if !weather_context_looks_like_current_weather(location, &surface) {
        return None;
    }
    extract_observed_weather_text(&surface)
}

fn weather_context_looks_like_current_weather(location: &str, context: &str) -> bool {
    let lowered = context.to_ascii_lowercase();
    let location_lowered = location.to_ascii_lowercase();
    let has_weather_marker = [
        "weather",
        "temperature",
        "forecast",
        "current",
        "today",
        "humidity",
        "wind",
        "rain",
        "snow",
        "天气",
        "气温",
        "温度",
        "预报",
        "当前",
        "今天",
        "湿度",
        "风",
        "雨",
        "雪",
    ]
    .iter()
    .any(|term| lowered.contains(term) || context.contains(term));
    let has_location = location.trim().is_empty()
        || lowered.contains(&location_lowered)
        || context.contains(location);
    has_weather_marker && has_location
}

fn extract_observed_weather_text(text: &str) -> Option<String> {
    let regex = Regex::new(
        r"(?xi)
        -?[0-9]{1,2}(?:\.[0-9]+)?\s*
        (?:
            °\s*[CF]
            | ℃
            | ℉
            | degrees?\s*(?:celsius|fahrenheit|c|f)?
            | 度
        )
    ",
    )
    .ok()?;
    let observed = regex
        .find_iter(text)
        .map(|matched| {
            normalize_excerpt(
                &text_context_window(text, matched.start(), matched.end(), 120),
                260,
            )
        })
        .find(|candidate| {
            let lowered = candidate.to_ascii_lowercase();
            [
                "weather",
                "temperature",
                "current",
                "forecast",
                "气温",
                "温度",
                "天气",
                "当前",
                "预报",
            ]
            .iter()
            .any(|term| lowered.contains(term) || candidate.contains(term))
        });
    observed
}

fn normalize_excerpt(text: &str, max_chars: usize) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect::<String>()
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn verification_source(
    kind: &str,
    title: impl Into<String>,
    uri: impl Into<String>,
    observed_at: Option<String>,
) -> VerificationSource {
    VerificationSource {
        kind: kind.to_string(),
        title: title.into(),
        uri: uri.into(),
        observed_at,
    }
}

fn realtime_source_timestamp(source: &RealtimeSourceRecord) -> Option<String> {
    source
        .published_at
        .clone()
        .or_else(|| source.observed_at.clone())
}

fn realtime_receipt(
    kind: &str,
    query: &str,
    status: &str,
    sources: &[RealtimeSourceRecord],
    value: Option<serde_json::Value>,
    blockers: Vec<String>,
) -> serde_json::Value {
    let source_receipts = sources
        .iter()
        .map(|source| {
            json!({
                "title": source.title,
                "url": source.url,
                "observed_at": source.observed_at,
                "published_at": source.published_at,
            })
        })
        .collect::<Vec<_>>();
    let freshness_ok = sources
        .iter()
        .all(|source| realtime_source_timestamp(source).is_some());
    json!({
        "kind": kind,
        "query": query,
        "status": status,
        "queried_at": now_rfc3339(),
        "freshness": {
            "required": true,
            "ok": freshness_ok,
        },
        "value": value,
        "sources": source_receipts,
        "blockers": blockers,
    })
}

fn ensure_realtime_sources_are_fresh(
    kind: &str,
    sources: &[RealtimeSourceRecord],
) -> anyhow::Result<()> {
    if sources.is_empty() {
        anyhow::bail!("{kind} did not return any usable realtime source");
    }
    if sources
        .iter()
        .any(|source| realtime_source_timestamp(source).is_none())
    {
        anyhow::bail!("{kind} returned source evidence without a timestamp");
    }
    Ok(())
}

fn realtime_lookup_plan() -> QueryVerificationPlan {
    QueryVerificationPlan {
        domain: VerificationDomain::KnowledgeFact,
        requirement: VerificationRequirement::Required,
        mode: VerificationMode::RealtimeLookup,
        route_hint: None,
    }
}

fn structured_lookup_followup_plan() -> VerificationFollowupPlan {
    build_verification_followup_plan(
        VerificationMode::RealtimeLookup,
        VerificationOutcome::VerificationSucceeded,
    )
}

fn realtime_lookup_orchestration_payload(
    plan: QueryVerificationPlan,
    preview: &serde_json::Value,
    followup: &serde_json::Value,
) -> anyhow::Result<(String, serde_json::Value)> {
    let route_reason = route_reason_for_plan(Some(&plan)).as_str().to_string();
    let preview = serde_json::from_value(preview.clone())?;
    let followup = serde_json::from_value(followup.clone())?;
    let decision =
        WebVerificationOrchestrator::new().decide(Some(&plan), Some(&preview), Some(&followup));
    Ok((route_reason, serde_json::to_value(decision)?))
}

fn web_backed_lookup_plan() -> QueryVerificationPlan {
    QueryVerificationPlan {
        domain: VerificationDomain::KnowledgeFact,
        requirement: VerificationRequirement::Required,
        mode: VerificationMode::WebSearchFetch,
        route_hint: None,
    }
}

fn preferred_query_date(date: Option<&str>) -> String {
    date.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string())
}

fn extract_observed_price_text(text: &str) -> Option<String> {
    let regex = Regex::new(
        r"(?xi)
        (?:
            (?P<prefix>\b(?:USD|US\$|EUR|GBP|CNY|JPY|HKD|AUD|CAD|CHF|SGD|KRW|RMB)\b|[¥\$€£])\s*
        )?
        (?P<number>(?:[0-9]{1,3}(?:,[0-9]{3})+|[0-9]+)(?:\.[0-9]+)?)
        (?:
            \s*(?P<suffix>\b(?:USD|USDT|USDC|EUR|GBP|CNY|JPY|HKD|AUD|CAD|CHF|SGD|KRW|RMB)\b)
        )?
    ",
    )
    .ok()?;

    regex
        .captures_iter(text)
        .filter_map(|capture| {
            let matched = capture.get(0)?;
            let number = capture.name("number")?.as_str();
            let value = number.replace(',', "").parse::<f64>().ok()?;
            let observed = matched.as_str().trim().to_string();
            if observed.len() > 32 {
                return None;
            }

            let has_currency = capture.name("prefix").is_some() || capture.name("suffix").is_some();
            let context = text_context_window(text, matched.start(), matched.end(), 80);
            let score = price_candidate_score(
                &observed,
                number,
                value,
                has_currency,
                number_is_followed_by_percent(text, matched.end()),
                &text_before_window(text, matched.start(), 48),
                &text_after_window(text, matched.end(), 48),
                &context,
            );
            Some((score, matched.start(), observed))
        })
        .filter(|(score, _, _)| *score >= 30)
        .max_by_key(|(score, start, _)| (*score, std::cmp::Reverse(*start)))
        .map(|(_, _, observed)| observed)
}

fn text_context_window(text: &str, start: usize, end: usize, radius_chars: usize) -> String {
    let prefix = text[..start]
        .chars()
        .rev()
        .take(radius_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let suffix = text[end..].chars().take(radius_chars).collect::<String>();
    format!("{}{}{}", prefix, &text[start..end], suffix)
}

fn text_before_window(text: &str, start: usize, radius_chars: usize) -> String {
    text[..start]
        .chars()
        .rev()
        .take(radius_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>()
}

fn text_after_window(text: &str, end: usize, radius_chars: usize) -> String {
    text[end..].chars().take(radius_chars).collect::<String>()
}

fn price_candidate_score(
    observed: &str,
    number: &str,
    value: f64,
    has_currency: bool,
    followed_by_percent: bool,
    before_context: &str,
    after_context: &str,
    context: &str,
) -> i32 {
    let lowered = context.to_lowercase();
    let before_lowered = before_context.to_lowercase();
    let after_lowered = after_context.to_lowercase();
    let mut score = 0;

    if has_currency {
        score += 70;
    }
    if number.contains(',') {
        score += 15;
    }
    if number.contains('.') {
        score += 10;
    }
    if value >= 100.0 {
        score += 10;
    }

    let price_terms = [
        "price",
        "quote",
        "last",
        "latest",
        "current",
        "trades at",
        "trading at",
        "market",
        "spot",
        "close",
        "bid",
        "ask",
        "价格",
        "报价",
        "现价",
        "当前",
        "最新",
        "交易",
        "美元",
        "人民币",
    ];
    if price_terms.iter().any(|term| lowered.contains(term)) {
        score += 35;
    }

    let direct_quote_before_terms = [
        "trades at",
        "trading at",
        "is trading at",
        "price is",
        "current price",
        "latest price",
        "quoted at",
        "报价",
        "现价",
        "当前价格",
        "最新价格",
        "交易于",
    ];
    if direct_quote_before_terms
        .iter()
        .any(|term| before_lowered.contains(term))
    {
        score += 120;
    }

    let non_quote_after_terms = [
        "resistance",
        "support",
        "target",
        "market cap",
        "volume",
        "high",
        "low",
        "阻力",
        "支撑",
        "目标",
        "市值",
        "成交量",
        "高点",
        "低点",
    ];
    if non_quote_after_terms
        .iter()
        .any(|term| after_lowered.contains(term))
    {
        score -= 120;
    }

    let date_terms = [
        "jan",
        "feb",
        "mar",
        "apr",
        "may",
        "jun",
        "jul",
        "aug",
        "sep",
        "oct",
        "nov",
        "dec",
        "updated",
        "date",
        "posted",
        "published",
        "time",
        "年",
        "月",
        "日",
    ];
    let looks_like_year = (1900.0..=2100.0).contains(&value) && number.len() == 4;
    let looks_like_small_calendar_number = (1.0..=31.0).contains(&value) && !number.contains('.');
    let near_date_language = date_terms.iter().any(|term| lowered.contains(term));
    if !has_currency
        && (looks_like_year || (looks_like_small_calendar_number && near_date_language))
    {
        score -= 90;
    }

    if observed.len() <= 2 && !has_currency {
        score -= 35;
    }
    if followed_by_percent || lowered.contains("percent") {
        score -= 60;
    }

    score
}

fn number_is_followed_by_percent(text: &str, end: usize) -> bool {
    text[end..]
        .chars()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| ch == '%')
}

fn parse_search_results(payload: &str) -> anyhow::Result<Vec<SearchResultRecord>> {
    match serde_json::from_str::<serde_json::Value>(payload) {
        Ok(serde_json::Value::Array(results)) => results
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<Vec<SearchResultRecord>, _>>()
            .map_err(|e| anyhow::anyhow!("Invalid search result record: {e}")),
        Ok(serde_json::Value::Object(mut object)) => {
            let results = object
                .remove("results")
                .ok_or_else(|| anyhow::anyhow!("Search result payload missing results field"))?;
            serde_json::from_value(results)
                .map_err(|e| anyhow::anyhow!("Invalid structured search results: {e}"))
        }
        Ok(_) => anyhow::bail!("Search result payload is not an object or array"),
        Err(_) if payload.trim_start().starts_with("status: blocked") => {
            anyhow::bail!("{}", normalize_excerpt(payload, 1000))
        }
        Err(e) => anyhow::bail!("Invalid search result payload: {e}"),
    }
}

async fn run_structured_search_fetch(
    search_tool: &WebSearchTool,
    fetch_tool: &WebFetchTool,
    query: &str,
    max_results: usize,
) -> anyhow::Result<Vec<RealtimeSourceRecord>> {
    let raw = search_tool
        .call(&json!({ "query": query, "structured": true }).to_string())
        .await?;
    let results = parse_search_results(&raw)?;
    let mut sources: Vec<RealtimeSourceRecord> = Vec::new();

    for result in results.into_iter().take(max_results) {
        let fetched_excerpt = match fetch_tool
            .call(&json!({ "url": result.url }).to_string())
            .await
        {
            Ok(content) => normalize_excerpt(&content, 800),
            Err(_) => String::new(),
        };

        let source = RealtimeSourceRecord {
            title: result.title,
            url: result.url,
            snippet: result.snippet,
            fetched_excerpt,
            observed_at: Some(now_rfc3339()),
            published_at: None,
        };

        if source_record_is_usable(&source) {
            sources.push(source);
        }
    }

    if sources.is_empty() {
        anyhow::bail!("Structured realtime lookup did not yield a usable source page");
    }

    Ok(sources)
}

pub struct WeatherLookupTool {
    client: Client,
    search_tool: WebSearchTool,
    fetch_tool: WebFetchTool,
}

impl WeatherLookupTool {
    pub fn new() -> Result<Self, Error> {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .user_agent("Mozilla/5.0 (compatible; BenShu-Bot/1.0)")
            .build()
            .map_err(|e| Error::Internal(format!("HTTP client error: {e}")))?;
        Ok(Self {
            client,
            search_tool: WebSearchTool::from_env()?,
            fetch_tool: WebFetchTool::with_defaults()?,
        })
    }

    fn geocode_language_order(location: &str) -> &'static [&'static str] {
        if location.chars().any(|ch| {
            ('\u{4E00}'..='\u{9FFF}').contains(&ch)
                || ('\u{3040}'..='\u{30FF}').contains(&ch)
                || ('\u{AC00}'..='\u{D7AF}').contains(&ch)
        }) {
            &["zh", "en"]
        } else {
            &["en"]
        }
    }

    async fn geocode_location(&self, location: &str) -> anyhow::Result<OpenMeteoGeocodeResult> {
        if location.chars().any(|ch| !ch.is_ascii()) {
            if let Some(place) = self.geocode_location_with_nominatim(location).await? {
                return Ok(place);
            }
        }

        let mut last_error: Option<anyhow::Error> = None;
        for language in Self::geocode_language_order(location) {
            let response = self
                .client
                .get("https://geocoding-api.open-meteo.com/v1/search")
                .query(&[
                    ("name", location),
                    ("count", "5"),
                    ("language", *language),
                    ("format", "json"),
                ])
                .send()
                .await
                .and_then(|response| response.error_for_status());

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    last_error = Some(error.into());
                    continue;
                }
            };

            let geocode = match response.json::<OpenMeteoGeocodeResponse>().await {
                Ok(geocode) => geocode,
                Err(error) => {
                    last_error = Some(error.into());
                    continue;
                }
            };

            if let Some(place) = geocode.results.into_iter().next() {
                return Ok(place);
            }
        }

        if let Some(place) = self.geocode_location_with_nominatim(location).await? {
            return Ok(place);
        }

        if let Some(error) = last_error {
            Err(error.context(format!("No weather location match found for '{location}'")))
        } else {
            anyhow::bail!("No weather location match found for '{}'", location);
        }
    }

    async fn geocode_location_with_nominatim(
        &self,
        location: &str,
    ) -> anyhow::Result<Option<OpenMeteoGeocodeResult>> {
        let response = self
            .client
            .get("https://nominatim.openstreetmap.org/search")
            .query(&[
                ("q", location),
                ("format", "jsonv2"),
                ("limit", "1"),
                ("accept-language", "zh,en"),
            ])
            .send()
            .await
            .and_then(|response| response.error_for_status());
        let response = match response {
            Ok(response) => response,
            Err(_) => return Ok(None),
        };
        let results = response
            .json::<Vec<NominatimGeocodeResult>>()
            .await
            .unwrap_or_default();
        let Some(result) = results.into_iter().next() else {
            return Ok(None);
        };
        let Ok(latitude) = result.lat.parse::<f64>() else {
            return Ok(None);
        };
        let Ok(longitude) = result.lon.parse::<f64>() else {
            return Ok(None);
        };
        Ok(Some(OpenMeteoGeocodeResult {
            name: normalize_excerpt(&result.display_name, 120),
            country: None,
            latitude,
            longitude,
            timezone: None,
        }))
    }

    fn render_display(
        requested_location: &str,
        resolved_location: &str,
        timezone: Option<&str>,
        source_name: &str,
        forecast: &OpenMeteoForecastResponse,
    ) -> serde_json::Value {
        let zh = Self::render_display_for_locale(
            true,
            requested_location,
            resolved_location,
            timezone,
            source_name,
            forecast,
        );
        let en = Self::render_display_for_locale(
            false,
            requested_location,
            resolved_location,
            timezone,
            source_name,
            forecast,
        );
        json!({
            "zh": zh,
            "en": en,
        })
    }

    fn render_display_for_locale(
        prefers_chinese: bool,
        requested_location: &str,
        resolved_location: &str,
        timezone: Option<&str>,
        source_name: &str,
        forecast: &OpenMeteoForecastResponse,
    ) -> String {
        let location = if resolved_location.trim().is_empty() {
            requested_location
        } else {
            resolved_location
        };
        let current = forecast.current.as_ref();
        let temperature = current
            .and_then(|current| current.get("temperature_2m"))
            .and_then(|value| value.as_f64())
            .map(|value| format!("{value:.1}°C"));
        let humidity = current
            .and_then(|current| current.get("relative_humidity_2m"))
            .and_then(|value| value.as_u64())
            .map(|value| format!("{value}%"));
        let precipitation = current
            .and_then(|current| current.get("precipitation"))
            .and_then(|value| value.as_f64())
            .map(|value| format!("{value:.1} mm"));
        let wind = current
            .and_then(|current| current.get("wind_speed_10m"))
            .and_then(|value| value.as_f64())
            .map(|value| format!("{value:.1} km/h"));
        let observed_time =
            current.and_then(|current| current.get("time").and_then(|value| value.as_str()));
        let condition = current
            .and_then(|current| current.get("weather_code"))
            .and_then(|value| value.as_i64())
            .map(|code| Self::weather_code_label(code, prefers_chinese));

        let today = forecast.daily.as_ref().and_then(|daily| {
            let date = daily
                .get("time")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.as_str())?;
            let max = daily
                .get("temperature_2m_max")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.as_f64());
            let min = daily
                .get("temperature_2m_min")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.as_f64());
            let rain = daily
                .get("precipitation_sum")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.as_f64());
            Some((date, max, min, rain))
        });

        if prefers_chinese {
            let mut parts = vec![format!("{}当前天气", location)];
            if let Some(condition) = condition {
                parts.push(condition.to_string());
            }
            if let Some(temperature) = temperature {
                parts.push(format!("气温 {}", temperature));
            }
            if let Some(humidity) = humidity {
                parts.push(format!("湿度 {}", humidity));
            }
            if let Some(precipitation) = precipitation {
                parts.push(format!("当前降水 {}", precipitation));
            }
            if let Some(wind) = wind {
                parts.push(format!("风速 {}", wind));
            }
            let mut summary = format!("{}。", parts.join("，"));
            if let Some((date, max, min, rain)) = today {
                summary.push_str(&format!(
                    "\n今天（{}）预报：最高温{}，最低温{}，累计降水{}。",
                    date,
                    max.map(|value| format!("{value:.1}°C"))
                        .unwrap_or_else(|| "未知".to_string()),
                    min.map(|value| format!("{value:.1}°C"))
                        .unwrap_or_else(|| "未知".to_string()),
                    rain.map(|value| format!("{value:.1} mm"))
                        .unwrap_or_else(|| "未知".to_string())
                ));
            }
            if let Some(observed_time) = observed_time {
                summary.push_str(&format!(
                    "\n观测时间：{}{}。",
                    observed_time,
                    timezone
                        .map(|value| format!(" {}", value))
                        .unwrap_or_default()
                ));
            }
            summary.push_str(&format!(" 来源：{}。", source_name));
            return summary;
        }

        let mut parts = vec![format!("Current weather for {}", location)];
        if let Some(condition) = condition {
            parts.push(condition.to_string());
        }
        if let Some(temperature) = temperature {
            parts.push(format!("temperature {}", temperature));
        }
        if let Some(humidity) = humidity {
            parts.push(format!("humidity {}", humidity));
        }
        if let Some(precipitation) = precipitation {
            parts.push(format!("current precipitation {}", precipitation));
        }
        if let Some(wind) = wind {
            parts.push(format!("wind {}", wind));
        }
        let mut summary = format!("{}.", parts.join(", "));
        if let Some((date, max, min, rain)) = today {
            summary.push_str(&format!(
                "\nForecast for {}: high {}, low {}, total precipitation {}.",
                date,
                max.map(|value| format!("{value:.1}°C"))
                    .unwrap_or_else(|| "unknown".to_string()),
                min.map(|value| format!("{value:.1}°C"))
                    .unwrap_or_else(|| "unknown".to_string()),
                rain.map(|value| format!("{value:.1} mm"))
                    .unwrap_or_else(|| "unknown".to_string())
            ));
        }
        if let Some(observed_time) = observed_time {
            summary.push_str(&format!(
                "\nObserved at {}{}.",
                observed_time,
                timezone
                    .map(|value| format!(" {}", value))
                    .unwrap_or_default()
            ));
        }
        summary.push_str(&format!(" Source: {}.", source_name));
        summary
    }

    fn weather_code_label(code: i64, prefers_chinese: bool) -> &'static str {
        match (prefers_chinese, code) {
            (true, 0) => "晴",
            (true, 1) => "基本晴",
            (true, 2) => "局部多云",
            (true, 3) => "阴",
            (true, 45 | 48) => "有雾",
            (true, 51 | 53 | 55) => "毛毛雨",
            (true, 56 | 57) => "冻毛毛雨",
            (true, 61 | 63 | 65) => "下雨",
            (true, 66 | 67) => "冻雨",
            (true, 71 | 73 | 75) => "下雪",
            (true, 77) => "雪粒",
            (true, 80 | 81 | 82) => "阵雨",
            (true, 85 | 86) => "阵雪",
            (true, 95) => "雷暴",
            (true, 96 | 99) => "雷暴伴冰雹",
            (true, _) => "天气状态未知",
            (false, 0) => "clear sky",
            (false, 1) => "mainly clear",
            (false, 2) => "partly cloudy",
            (false, 3) => "overcast",
            (false, 45 | 48) => "fog",
            (false, 51 | 53 | 55) => "drizzle",
            (false, 56 | 57) => "freezing drizzle",
            (false, 61 | 63 | 65) => "rain",
            (false, 66 | 67) => "freezing rain",
            (false, 71 | 73 | 75) => "snow",
            (false, 77) => "snow grains",
            (false, 80 | 81 | 82) => "rain showers",
            (false, 85 | 86) => "snow showers",
            (false, 95) => "thunderstorm",
            (false, 96 | 99) => "thunderstorm with hail",
            (false, _) => "unknown conditions",
        }
    }

    async fn lookup_weather_via_search_fallback(
        &self,
        location: &str,
        date: Option<&str>,
        provider_error: anyhow::Error,
    ) -> anyhow::Result<String> {
        let query = match date.map(str::trim).filter(|value| !value.is_empty()) {
            Some(date) => format!("{location} weather forecast {date} current temperature"),
            None => format!("{location} current weather today temperature forecast"),
        };
        let sources =
            run_structured_search_fetch(&self.search_tool, &self.fetch_tool, &query, 3).await?;
        let Some((source, observed_weather_text)) = sources.iter().find_map(|source| {
            extract_current_weather_from_source(location, source).map(|text| (source, text))
        }) else {
            anyhow::bail!(
                "weather_lookup provider failed ({provider_error}); search fallback found sources but no explicit current weather observation"
            );
        };
        let verified_sources = vec![source.clone()];
        ensure_realtime_sources_are_fresh("weather_lookup", &verified_sources)?;
        let verification_preview =
            serde_json::to_value(build_verified_verification_result_envelope(
                VerificationDomain::KnowledgeFact,
                VerificationMode::WebSearchFetch,
                verified_sources
                    .iter()
                    .map(|source| {
                        verification_source(
                            "weather_web_source",
                            source.title.clone(),
                            source.url.clone(),
                            realtime_source_timestamp(source),
                        )
                    })
                    .collect(),
                "weather lookup completed from search fallback and source fetch",
            ))?;
        let verification_followup =
            serde_json::to_value(build_source_observed_followup_plan(true))?;
        let (route_reason, orchestration_decision) = realtime_lookup_orchestration_payload(
            web_backed_lookup_plan(),
            &verification_preview,
            &verification_followup,
        )?;
        let realtime_receipt = realtime_receipt(
            "weather_lookup",
            &query,
            "verified",
            &verified_sources,
            Some(json!({
                "location": location,
                "observed_weather_text": observed_weather_text.clone(),
                "fallback": "web_search_fetch",
            })),
            Vec::new(),
        );
        let zh = format!(
            "{location}天气：{observed_weather_text}\n来源：{}（{}）。",
            source.title, source.url
        );
        let en = format!(
            "Weather for {location}: {observed_weather_text}\nSource: {} ({}).",
            source.title, source.url
        );
        let payload = json!({
            "kind": "weather_lookup",
            "display": {
                "zh": zh,
                "en": en,
            },
            "location": {
                "requested": location,
                "resolved": location,
            },
            "requested_date": date.map(str::trim).filter(|value| !value.is_empty()),
            "queried_at": now_rfc3339(),
            "source": {
                "name": "web_search_fetch",
                "provider_error": provider_error.to_string(),
            },
            "realtime_receipt": realtime_receipt,
            "verification_preview": verification_preview,
            "verification_followup": verification_followup,
            "route_reason": route_reason,
            "orchestration_decision": orchestration_decision,
            "sources": verified_sources,
        });

        Ok(serde_json::to_string_pretty(&payload)?)
    }
}

#[derive(Debug, Deserialize)]
struct WeatherArgs {
    location: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    days: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoGeocodeResponse {
    #[serde(default)]
    results: Vec<OpenMeteoGeocodeResult>,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoGeocodeResult {
    name: String,
    #[serde(default)]
    country: Option<String>,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    timezone: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NominatimGeocodeResult {
    display_name: String,
    lat: String,
    lon: String,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoForecastResponse {
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    current: Option<serde_json::Value>,
    #[serde(default)]
    daily: Option<serde_json::Value>,
}

#[async_trait]
impl Tool for WeatherLookupTool {
    fn name(&self) -> String {
        "weather_lookup".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "weather_lookup".to_string(),
            description: "Lookup structured current weather or forecast information for a location using a dedicated weather execution surface.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string", "description": "City or location to look up."},
                    "date": {"type": "string", "description": "Optional date (YYYY-MM-DD) for forecast queries."},
                    "days": {"type": "integer", "description": "Optional forecast window in days (1-7)."}
                },
                "required": ["location"]
            }),
            parameters_ts: Some("interface WeatherLookup { location: string; date?: string; days?: number; }".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use for weather, forecast, temperature, rain, and condition questions. Prefer this over generic web search when location is known.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: WeatherArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Invalid arguments: {e}"))?;
        let location = args.location.trim();
        if location.is_empty() {
            anyhow::bail!("location is required");
        }

        let place = match self.geocode_location(location).await {
            Ok(place) => place,
            Err(error) => {
                return self
                    .lookup_weather_via_search_fallback(location, args.date.as_deref(), error)
                    .await;
            }
        };

        let forecast_days = args.days.unwrap_or(3).clamp(1, 7).to_string();
        let forecast = match self
            .client
            .get("https://api.open-meteo.com/v1/forecast")
            .query(&[
                ("latitude", place.latitude.to_string()),
                ("longitude", place.longitude.to_string()),
                (
                    "current",
                    "temperature_2m,relative_humidity_2m,precipitation,weather_code,wind_speed_10m"
                        .to_string(),
                ),
                (
                    "daily",
                    "weather_code,temperature_2m_max,temperature_2m_min,precipitation_sum"
                        .to_string(),
                ),
                ("timezone", "auto".to_string()),
                ("forecast_days", forecast_days),
            ])
            .send()
            .await
            .and_then(|response| response.error_for_status())
        {
            Ok(response) => match response.json::<OpenMeteoForecastResponse>().await {
                Ok(forecast) => forecast,
                Err(error) => {
                    return self
                        .lookup_weather_via_search_fallback(
                            location,
                            args.date.as_deref(),
                            error.into(),
                        )
                        .await;
                }
            },
            Err(error) => {
                return self
                    .lookup_weather_via_search_fallback(
                        location,
                        args.date.as_deref(),
                        error.into(),
                    )
                    .await;
            }
        };

        let verification_preview =
            serde_json::to_value(build_verified_verification_result_envelope(
                VerificationDomain::KnowledgeFact,
                VerificationMode::RealtimeLookup,
                vec![
                    verification_source(
                        "weather_geocoding",
                        "Open-Meteo Geocoding",
                        "https://geocoding-api.open-meteo.com/v1/search",
                        None,
                    ),
                    verification_source(
                        "weather_forecast",
                        "Open-Meteo Forecast",
                        "https://api.open-meteo.com/v1/forecast",
                        Some(now_rfc3339()),
                    ),
                ],
                "structured weather lookup completed",
            ))?;
        let verification_followup = serde_json::to_value(structured_lookup_followup_plan())?;
        let (route_reason, orchestration_decision) = realtime_lookup_orchestration_payload(
            realtime_lookup_plan(),
            &verification_preview,
            &verification_followup,
        )?;

        let timezone = forecast.timezone.clone().or_else(|| place.timezone.clone());
        let display = Self::render_display(
            location,
            &place.name,
            timezone.as_deref(),
            "open-meteo",
            &forecast,
        );
        let weather_sources = vec![RealtimeSourceRecord {
            title: "Open-Meteo Forecast".to_string(),
            url: "https://api.open-meteo.com/v1/forecast".to_string(),
            snippet: "Structured weather forecast response observed by realtime lookup."
                .to_string(),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: None,
        }];
        ensure_realtime_sources_are_fresh("weather_lookup", &weather_sources)?;
        let realtime_receipt = realtime_receipt(
            "weather_lookup",
            location,
            "verified",
            &weather_sources,
            Some(json!({
                "location": place.name,
                "current": forecast.current.clone(),
                "daily": forecast.daily.clone(),
            })),
            Vec::new(),
        );

        let payload = json!({
            "kind": "weather_lookup",
            "display": display,
            "location": {
                "requested": location,
                "resolved": place.name,
                "country": place.country,
                "latitude": place.latitude,
                "longitude": place.longitude,
                "timezone": timezone,
            },
            "requested_date": args.date.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()),
            "queried_at": now_rfc3339(),
            "source": {
                "name": "open-meteo",
                "geocoding_url": "https://geocoding-api.open-meteo.com",
                "forecast_url": "https://api.open-meteo.com"
            },
            "realtime_receipt": realtime_receipt,
            "verification_preview": verification_preview,
            "verification_followup": verification_followup,
            "route_reason": route_reason,
            "orchestration_decision": orchestration_decision,
            "result": {
                "current": forecast.current,
                "daily": forecast.daily,
            }
        });

        Ok(serde_json::to_string_pretty(&payload)?)
    }
}

#[derive(Debug, Clone)]
pub struct FxLookupTool {
    client: Client,
}

impl FxLookupTool {
    pub fn new() -> Result<Self, Error> {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .user_agent("Mozilla/5.0 (compatible; BenShu-Bot/1.0)")
            .build()
            .map_err(|e| Error::Internal(format!("HTTP client error: {e}")))?;
        Ok(Self { client })
    }
}

#[derive(Debug, Deserialize)]
struct FxArgs {
    base_currency: String,
    quote_currency: String,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FrankfurterResponse {
    amount: f64,
    base: String,
    date: String,
    rates: HashMap<String, f64>,
}

fn format_fx_display_zh(
    base: &str,
    quote: &str,
    amount: f64,
    rate: Option<f64>,
    date: &str,
) -> String {
    match rate {
        Some(rate) => format!(
            "{} {} 约等于 {} {}，参考汇率为 1 {} = {} {}。来源：Frankfurter FX API（https://www.frankfurter.app），参考日期：{}。",
            format_decimal(amount),
            base,
            format_decimal(amount * rate),
            quote,
            base,
            format_decimal(rate),
            quote,
            date
        ),
        None => format!(
            "暂时没有查到 {base}/{quote} 的可用汇率。来源：Frankfurter FX API（https://www.frankfurter.app），参考日期：{date}。"
        ),
    }
}

fn format_fx_display_en(
    base: &str,
    quote: &str,
    amount: f64,
    rate: Option<f64>,
    date: &str,
) -> String {
    match rate {
        Some(rate) => format!(
            "{} {} is about {} {}; reference rate is 1 {} = {} {}. Source: Frankfurter FX API (https://www.frankfurter.app), reference date: {}.",
            format_decimal(amount),
            base,
            format_decimal(amount * rate),
            quote,
            base,
            format_decimal(rate),
            quote,
            date
        ),
        None => format!(
            "No usable {base}/{quote} FX rate was found. Source: Frankfurter FX API (https://www.frankfurter.app), reference date: {date}."
        ),
    }
}

fn format_decimal(value: f64) -> String {
    let mut text = format!("{value:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[async_trait]
impl Tool for FxLookupTool {
    fn name(&self) -> String {
        "fx_lookup".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "fx_lookup".to_string(),
            description: "Lookup a structured foreign exchange rate using a dedicated FX execution surface.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "base_currency": {"type": "string"},
                    "quote_currency": {"type": "string"},
                    "date": {"type": "string", "description": "Optional date (YYYY-MM-DD)."}
                },
                "required": ["base_currency", "quote_currency"]
            }),
            parameters_ts: Some("interface FxLookup { base_currency: string; quote_currency: string; date?: string; }".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use for exchange rate questions. Prefer this over generic web search once base and quote currencies are known.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: FxArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Invalid arguments: {e}"))?;
        let base = args.base_currency.trim().to_uppercase();
        let quote = args.quote_currency.trim().to_uppercase();
        if base.len() != 3 || quote.len() != 3 {
            anyhow::bail!("base_currency and quote_currency must be 3-letter ISO codes");
        }

        let date = preferred_query_date(args.date.as_deref());
        let endpoint = if args
            .date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
        {
            format!("https://api.frankfurter.app/{date}")
        } else {
            "https://api.frankfurter.app/latest".to_string()
        };

        let response = self
            .client
            .get(endpoint)
            .query(&[("from", base.as_str()), ("to", quote.as_str())])
            .send()
            .await?
            .error_for_status()?
            .json::<FrankfurterResponse>()
            .await?;

        let rate = response.rates.get(&quote).copied();
        let fx_sources = vec![RealtimeSourceRecord {
            title: "Frankfurter FX API".to_string(),
            url: "https://api.frankfurter.app/latest".to_string(),
            snippet: format!(
                "Structured FX rate for {}/{} observed.",
                response.base, quote
            ),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: Some(response.date.clone()),
        }];
        ensure_realtime_sources_are_fresh("fx_lookup", &fx_sources)?;
        let realtime_receipt = realtime_receipt(
            "fx_lookup",
            &format!("{}/{}", response.base, quote),
            if rate.is_some() { "verified" } else { "failed" },
            &fx_sources,
            rate.map(|value| {
                json!({
                    "base_currency": response.base.clone(),
                    "quote_currency": quote.clone(),
                    "exchange_rate": value,
                    "reference_date": response.date.clone(),
                })
            }),
            if rate.is_some() {
                Vec::new()
            } else {
                vec![format!("No {} rate was returned by the FX provider", quote)]
            },
        );
        if rate.is_none() {
            anyhow::bail!(
                "Structured FX lookup did not yield a rate for {}/{}",
                response.base,
                quote
            );
        }
        let verification_preview =
            serde_json::to_value(build_verified_verification_result_envelope(
                VerificationDomain::KnowledgeFact,
                VerificationMode::RealtimeLookup,
                vec![verification_source(
                    "fx_api",
                    "Frankfurter FX API",
                    "https://api.frankfurter.app/latest",
                    Some(now_rfc3339()),
                )],
                "structured FX lookup completed",
            ))?;
        let verification_followup = serde_json::to_value(structured_lookup_followup_plan())?;
        let (route_reason, orchestration_decision) = realtime_lookup_orchestration_payload(
            realtime_lookup_plan(),
            &verification_preview,
            &verification_followup,
        )?;

        let payload = json!({
            "kind": "fx_lookup",
            "base_currency": response.base,
            "quote_currency": quote,
            "exchange_rate": rate,
            "query_time": now_rfc3339(),
            "reference_date": response.date,
            "display": {
                "zh": format_fx_display_zh(&response.base, &quote, response.amount, rate, &response.date),
                "en": format_fx_display_en(&response.base, &quote, response.amount, rate, &response.date)
            },
            "source": {
                "name": "frankfurter",
                "url": "https://www.frankfurter.app"
            },
            "realtime_receipt": realtime_receipt,
            "verification_preview": verification_preview,
            "verification_followup": verification_followup,
            "route_reason": route_reason,
            "orchestration_decision": orchestration_decision,
            "amount": response.amount
        });

        Ok(serde_json::to_string_pretty(&payload)?)
    }
}

pub struct LatestInfoLookupTool {
    client: Client,
    search_tool: WebSearchTool,
    fetch_tool: WebFetchTool,
}

impl LatestInfoLookupTool {
    pub fn new() -> Result<Self, Error> {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .user_agent("Mozilla/5.0 (compatible; BenShu-Bot/1.0)")
            .build()
            .map_err(|e| Error::Internal(format!("HTTP client error: {e}")))?;
        Ok(Self {
            client,
            search_tool: WebSearchTool::from_env()?,
            fetch_tool: WebFetchTool::with_defaults()?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct LatestInfoArgs {
    topic: String,
    #[serde(default)]
    query: Option<String>,
}

fn public_news_feed_urls(topic: &str) -> Vec<&'static str> {
    let lowered = topic.to_ascii_lowercase();
    let mut urls: Vec<&'static str> = Vec::new();
    let mut push = |url: &'static str| {
        if !urls.contains(&url) {
            urls.push(url);
        }
    };

    if topic.contains("科技")
        || topic.contains("人工智能")
        || lowered.contains("technology")
        || lowered.contains("tech")
        || lowered.contains("ai")
    {
        push("https://feeds.bbci.co.uk/news/technology/rss.xml");
        push("https://www.theguardian.com/technology/rss");
    }
    if topic.contains("财经")
        || topic.contains("金融")
        || lowered.contains("business")
        || lowered.contains("finance")
        || lowered.contains("market")
    {
        push("https://feeds.bbci.co.uk/news/business/rss.xml");
        push("https://www.theguardian.com/business/rss");
    }
    if topic.contains("中国")
        || topic.contains("国内")
        || lowered.contains("china")
        || lowered.contains("chinese")
    {
        push("https://feeds.bbci.co.uk/zhongwen/simp/rss.xml");
        push("https://feeds.bbci.co.uk/news/world/asia/rss.xml");
    }

    for url in [
        "https://feeds.bbci.co.uk/news/world/rss.xml",
        "https://feeds.bbci.co.uk/news/rss.xml",
        "https://rss.nytimes.com/services/xml/rss/nyt/World.xml",
        "https://www.theguardian.com/world/rss",
        "https://www.aljazeera.com/xml/rss/all.xml",
    ] {
        push(url);
    }
    urls
}

fn latest_info_topic_is_broad_feed_request(topic: &str) -> bool {
    let lowered = topic.to_ascii_lowercase();
    [
        "国际",
        "世界",
        "中国",
        "国内",
        "科技",
        "人工智能",
        "财经",
        "金融",
        "重要",
    ]
    .iter()
    .any(|term| topic.contains(term))
        || [
            "world",
            "international",
            "china",
            "technology",
            "tech",
            "ai",
            "business",
            "finance",
            "market",
            "important",
        ]
        .iter()
        .any(|term| lowered.contains(term))
}

fn latest_info_source_mentions_stale_year(source: &RealtimeSourceRecord) -> bool {
    let current_year = chrono::Utc::now().year();
    let context = format!(
        "{}\n{}\n{}",
        source.title, source.snippet, source.fetched_excerpt
    );
    for year in 1990..current_year {
        if context.contains(&year.to_string())
            || context.contains(&format!("{year}年"))
            || context.contains(&format!("{year}-"))
            || context.contains(&format!("{year}/"))
        {
            return true;
        }
    }
    false
}

fn filter_recent_latest_info_sources(
    sources: Vec<RealtimeSourceRecord>,
    max_results: usize,
) -> Vec<RealtimeSourceRecord> {
    let mut filtered = Vec::new();
    for source in sources {
        if latest_info_source_mentions_stale_year(&source) {
            continue;
        }
        filtered.push(source);
        if filtered.len() >= max_results {
            break;
        }
    }
    filtered
}

fn xml_text(block: &str, tag: &str) -> Option<String> {
    let pattern = format!(r"(?is)<(?:\w+:)?{tag}[^>]*>(.*?)</(?:\w+:)?{tag}>");
    let regex = Regex::new(&pattern).ok()?;
    regex
        .captures(block)
        .and_then(|captures| captures.get(1))
        .map(|value| decode_xml_text(value.as_str()))
        .filter(|value| !value.trim().is_empty())
}

fn decode_xml_text(value: &str) -> String {
    let value = value
        .replace("<![CDATA[", "")
        .replace("]]>", "")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'");
    let tags = Regex::new(r"(?is)<[^>]+>").ok();
    let value = tags
        .map(|regex| regex.replace_all(&value, " ").to_string())
        .unwrap_or(value);
    normalize_excerpt(&value, 500)
}

fn feed_item_blocks(body: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    if let Ok(item_regex) = Regex::new(r"(?is)<item\b[^>]*>.*?</item>") {
        blocks.extend(
            item_regex
                .find_iter(body)
                .map(|matched| matched.as_str().to_string()),
        );
    }
    if blocks.is_empty() {
        if let Ok(entry_regex) = Regex::new(r"(?is)<entry\b[^>]*>.*?</entry>") {
            blocks.extend(
                entry_regex
                    .find_iter(body)
                    .map(|matched| matched.as_str().to_string()),
            );
        }
    }
    blocks
}

fn link_from_feed_block(block: &str) -> Option<String> {
    if let Some(link) = xml_text(block, "link") {
        if link.starts_with("http://") || link.starts_with("https://") {
            return Some(link);
        }
    }
    let href_regex = Regex::new(r#"(?is)<(?:\w+:)?link[^>]+href=["']([^"']+)["']"#).ok()?;
    href_regex
        .captures(block)
        .and_then(|captures| captures.get(1))
        .map(|value| decode_xml_text(value.as_str()))
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
}

fn format_latest_info_display_zh(sources: &[RealtimeSourceRecord]) -> String {
    if sources.is_empty() {
        return "没有找到可用的近期公开来源。".to_string();
    }
    let mut lines = vec!["已找到近期公开来源：".to_string()];
    for (index, source) in sources.iter().enumerate() {
        lines.push(format!("{}. {} - {}", index + 1, source.title, source.url));
    }
    lines.join("\n")
}

fn format_latest_info_display_en(sources: &[RealtimeSourceRecord]) -> String {
    if sources.is_empty() {
        return "No usable recent public sources were found.".to_string();
    }
    let mut lines = vec!["Recent public sources found:".to_string()];
    for (index, source) in sources.iter().enumerate() {
        lines.push(format!("{}. {} - {}", index + 1, source.title, source.url));
    }
    lines.join("\n")
}

async fn lookup_recent_public_feed_sources(
    client: &Client,
    topic: &str,
    max_results: usize,
) -> Vec<RealtimeSourceRecord> {
    let policy = policy::RealtimeLookupPolicy::load();
    let broad_feed_request = policy.latest_info_topic_is_generic_news(topic)
        || latest_info_topic_is_broad_feed_request(topic);
    let mut sources: Vec<RealtimeSourceRecord> = Vec::new();
    for feed_url in public_news_feed_urls(topic) {
        let Ok(response) = client
            .get(feed_url)
            .header(
                "Accept",
                "application/rss+xml,application/atom+xml,application/xml,text/xml,*/*",
            )
            .send()
            .await
        else {
            continue;
        };
        let Ok(body) = response.error_for_status() else {
            continue;
        };
        let Ok(body) = body.text().await else {
            continue;
        };
        for block in feed_item_blocks(&body) {
            if !broad_feed_request && !policy.feed_item_matches_topic(&block, topic) {
                continue;
            }
            let Some(title) = xml_text(&block, "title") else {
                continue;
            };
            let Some(url) = link_from_feed_block(&block) else {
                continue;
            };
            if sources.iter().any(|source| source.url == url) {
                continue;
            }
            let published = xml_text(&block, "pubDate")
                .or_else(|| xml_text(&block, "updated"))
                .or_else(|| xml_text(&block, "published"));
            let description = xml_text(&block, "description")
                .or_else(|| xml_text(&block, "summary"))
                .unwrap_or_default();
            let snippet = match published {
                Some(ref published) if !description.is_empty() => {
                    format!("{description} Published: {published}")
                }
                Some(ref published) => format!("Published: {published}"),
                None => description,
            };
            let source = RealtimeSourceRecord {
                title,
                url,
                snippet,
                fetched_excerpt: String::new(),
                observed_at: Some(now_rfc3339()),
                published_at: published,
            };
            if source_record_is_usable(&source) && !latest_info_source_mentions_stale_year(&source)
            {
                sources.push(source);
            }
            if sources.len() >= max_results {
                return sources;
            }
        }
    }
    sources
}

#[async_trait]
impl Tool for LatestInfoLookupTool {
    fn name(&self) -> String {
        "latest_info_lookup".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "latest_info_lookup".to_string(),
            description: "Lookup recent public information for a topic and return a structured source list.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "topic": {"type": "string"},
                    "query": {"type": "string", "description": "Optional fully specified latest-info query."}
                },
                "required": ["topic"]
            }),
            parameters_ts: Some("interface LatestInfoLookup { topic: string; query?: string; }".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use for latest developments, current events, recent updates, or \"what's new\" queries about a topic.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: LatestInfoArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Invalid arguments: {e}"))?;
        let topic = args.topic.trim();
        if topic.is_empty() {
            anyhow::bail!("topic is required");
        }

        let lookup_policy = policy::RealtimeLookupPolicy::load();
        let preferred_date = preferred_query_date(None);
        let normalized_query = lookup_policy.normalized_latest_info_query(
            topic,
            args.query.as_deref(),
            &preferred_date,
        );

        let sources = if lookup_policy.latest_info_topic_is_generic_news(topic)
            || lookup_policy.latest_info_topic_is_generic_news(&normalized_query)
            || latest_info_topic_is_broad_feed_request(topic)
            || latest_info_topic_is_broad_feed_request(&normalized_query)
        {
            let feed_sources =
                lookup_recent_public_feed_sources(&self.client, &normalized_query, 5).await;
            if feed_sources.is_empty() {
                let sources = run_structured_search_fetch(
                    &self.search_tool,
                    &self.fetch_tool,
                    &normalized_query,
                    MAX_FETCHED_RESULTS,
                )
                .await?;
                filter_recent_latest_info_sources(sources, MAX_FETCHED_RESULTS)
            } else {
                feed_sources
            }
        } else {
            match run_structured_search_fetch(
                &self.search_tool,
                &self.fetch_tool,
                &normalized_query,
                MAX_FETCHED_RESULTS,
            )
            .await
            {
                Ok(sources) => filter_recent_latest_info_sources(sources, MAX_FETCHED_RESULTS),
                Err(search_error) => {
                    let feed_sources =
                        lookup_recent_public_feed_sources(&self.client, &normalized_query, 5).await;
                    if feed_sources.is_empty() {
                        return Err(search_error);
                    }
                    feed_sources
                }
            }
        };
        let min_sources = lookup_policy
            .latest_info_min_sources(topic)
            .max(lookup_policy.latest_info_min_sources(&normalized_query));
        if sources.len() < min_sources {
            anyhow::bail!(
                "latest_info_lookup found {} usable source(s), but {} timestamped source(s) are required for a verified realtime answer",
                sources.len(),
                min_sources
            );
        }
        ensure_realtime_sources_are_fresh("latest_info_lookup", &sources)?;

        let verification_preview =
            serde_json::to_value(build_verified_verification_result_envelope(
                VerificationDomain::KnowledgeFact,
                VerificationMode::WebSearchFetch,
                sources
                    .iter()
                    .map(|source| {
                        verification_source(
                            "web_source",
                            source.title.clone(),
                            source.url.clone(),
                            realtime_source_timestamp(source),
                        )
                    })
                    .collect(),
                "latest-info lookup completed from structured search and fetch",
            ))?;
        let verification_followup =
            serde_json::to_value(build_source_observed_followup_plan(true))?;
        let (route_reason, orchestration_decision) = realtime_lookup_orchestration_payload(
            web_backed_lookup_plan(),
            &verification_preview,
            &verification_followup,
        )?;
        let realtime_receipt = realtime_receipt(
            "latest_info_lookup",
            &normalized_query,
            "verified",
            &sources,
            Some(json!({
                "results_summary": sources.iter().map(|source| source.title.clone()).collect::<Vec<_>>(),
                "source_count": sources.len(),
            })),
            Vec::new(),
        );

        let payload = json!({
            "kind": "latest_info_lookup",
            "topic": topic,
            "normalized_query": normalized_query,
            "queried_at": now_rfc3339(),
            "display": {
                "zh": format_latest_info_display_zh(&sources),
                "en": format_latest_info_display_en(&sources)
            },
            "results_summary": sources.iter().map(|source| source.title.clone()).collect::<Vec<_>>(),
            "realtime_receipt": realtime_receipt,
            "verification_preview": verification_preview,
            "verification_followup": verification_followup,
            "route_reason": route_reason,
            "orchestration_decision": orchestration_decision,
            "sources": sources
        });

        Ok(serde_json::to_string_pretty(&payload)?)
    }
}

pub struct PriceLookupTool {
    client: Client,
    search_tool: WebSearchTool,
    fetch_tool: WebFetchTool,
}

impl PriceLookupTool {
    pub fn new() -> Result<Self, Error> {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .user_agent("Mozilla/5.0 (compatible; BenShu-Bot/1.0)")
            .build()
            .map_err(|e| Error::Internal(format!("HTTP client error: {e}")))?;
        Ok(Self {
            client,
            search_tool: WebSearchTool::from_env()?,
            fetch_tool: WebFetchTool::with_defaults()?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct PriceArgs {
    symbol: String,
    #[serde(default)]
    market: Option<String>,
    #[serde(default)]
    quote_currency: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CoinGeckoSearchResponse {
    #[serde(default)]
    coins: Vec<CoinGeckoCoin>,
}

#[derive(Debug, Deserialize)]
struct CoinGeckoCoin {
    id: String,
    name: String,
    symbol: String,
}

async fn lookup_public_market_quote(
    client: &Client,
    symbol: &str,
) -> anyhow::Result<Option<(RealtimeSourceRecord, String)>> {
    let lookup_policy = policy::RealtimeLookupPolicy::load();
    let Some(source_policy) = lookup_policy.market_quote_source_for_symbol(symbol) else {
        return Ok(None);
    };
    for endpoint in source_policy.endpoints {
        let Ok(response) = client.get(&endpoint.url).send().await else {
            continue;
        };
        let Ok(body) = response.error_for_status() else {
            continue;
        };
        let Ok(body) = body.text().await else {
            continue;
        };
        let Some(observed) = extract_public_market_quote_text(endpoint.parser, &body) else {
            continue;
        };
        let source = RealtimeSourceRecord {
            title: endpoint.title,
            url: endpoint.url,
            snippet: format!(
                "{} is {}. Observed at {}.",
                source_policy.title,
                observed,
                now_rfc3339()
            ),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: None,
        };
        return Ok(Some((source, observed)));
    }
    Ok(None)
}

async fn lookup_public_ticker_quote(
    client: &Client,
    symbol: &str,
    market: Option<&str>,
) -> anyhow::Result<Option<(RealtimeSourceRecord, String)>> {
    let Some(ticker) = normalize_public_ticker_symbol(symbol) else {
        return Ok(None);
    };

    let candidates = public_ticker_source_candidates(&ticker, market);
    for (source_symbol, url) in candidates {
        let Ok(response) = client.get(&url).send().await else {
            continue;
        };
        let Ok(body) = response.error_for_status() else {
            continue;
        };
        let Ok(body) = body.text().await else {
            continue;
        };
        let Some(observed) = extract_stooq_csv_close_quote_text(&body) else {
            continue;
        };
        let source = RealtimeSourceRecord {
            title: format!("{source_symbol} current market quote"),
            url,
            snippet: format!(
                "{source_symbol} latest public market close quote is {observed}. Observed at {}.",
                now_rfc3339()
            ),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: None,
        };
        return Ok(Some((source, observed)));
    }

    Ok(None)
}

fn normalize_public_ticker_symbol(symbol: &str) -> Option<String> {
    let trimmed = symbol
        .trim()
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '?' | '？'
                        | '。'
                        | '，'
                        | ','
                        | ':'
                        | '：'
                        | ';'
                        | '；'
                        | '('
                        | ')'
                        | '（'
                        | '）'
                )
        })
        .trim_end_matches("股票")
        .trim_end_matches("stock")
        .trim();
    if trimmed.is_empty() || trimmed.chars().count() > 12 {
        return None;
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '^'))
    {
        return None;
    }
    Some(trimmed.to_ascii_lowercase().replace(['_', '-'], "."))
}

fn public_ticker_source_candidates(ticker: &str, market: Option<&str>) -> Vec<(String, String)> {
    let mut symbols = Vec::new();
    let normalized = ticker.trim().trim_start_matches('^').to_ascii_lowercase();
    if normalized.is_empty() {
        return Vec::new();
    }

    if normalized.contains('.') {
        symbols.push(normalized.clone());
    } else {
        let market = market.unwrap_or("").trim().to_ascii_lowercase();
        let suffix = if market.contains("hong kong") || market == "hk" || market.contains("港") {
            "hk"
        } else if market.contains("london") || market == "uk" || market.contains("英") {
            "uk"
        } else if market.contains("canada") || market == "ca" || market.contains("加") {
            "ca"
        } else {
            "us"
        };
        symbols.push(format!("{normalized}.{suffix}"));
        symbols.push(normalized.clone());
    }

    symbols
        .into_iter()
        .map(|source_symbol| {
            let url = format!("https://stooq.com/q/l/?s={source_symbol}&f=sd2t2ohlcv&h&e=csv");
            (source_symbol.to_ascii_uppercase(), url)
        })
        .collect()
}

fn extract_public_market_quote_text(
    parser: policy::PublicMarketQuoteParser,
    body: &str,
) -> Option<String> {
    match parser {
        policy::PublicMarketQuoteParser::StooqCsvClose => extract_stooq_csv_close_quote_text(body),
        policy::PublicMarketQuoteParser::GoogleFinanceLastPrice => {
            extract_google_finance_last_price_text(body)
        }
    }
}

fn extract_stooq_csv_close_quote_text(body: &str) -> Option<String> {
    let mut rows = body.lines().map(str::trim).filter(|line| !line.is_empty());
    let header = rows.next()?;
    let close_index = header
        .split(',')
        .position(|name| name.trim().eq_ignore_ascii_case("close"))?;
    let row = rows.next()?;
    let close = row.split(',').nth(close_index)?.trim();
    if close.eq_ignore_ascii_case("N/D") || close.is_empty() {
        return None;
    }
    let value = close.parse::<f64>().ok()?;
    Some(format_market_quote_value(value))
}

fn extract_google_finance_last_price_text(body: &str) -> Option<String> {
    let regex = Regex::new(r#"data-last-price="(?P<price>[0-9]+(?:\.[0-9]+)?)""#).ok()?;
    let price = regex
        .captures(body)
        .and_then(|capture| capture.name("price"))
        .and_then(|value| value.as_str().parse::<f64>().ok())?;
    Some(format_market_quote_value(price))
}

fn format_market_quote_value(value: f64) -> String {
    let raw = format!("{value:.2}");
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw.as_str(), ""));
    let mut reversed = String::new();
    for (index, ch) in whole.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            reversed.push(',');
        }
        reversed.push(ch);
    }
    let whole = reversed.chars().rev().collect::<String>();
    if fraction.is_empty() {
        whole
    } else {
        format!("{whole}.{fraction}")
    }
}

async fn lookup_public_crypto_price(
    client: &Client,
    symbol: &str,
    quote_currency: Option<&str>,
) -> anyhow::Result<Option<(RealtimeSourceRecord, String)>> {
    let symbol = symbol.trim();
    if symbol.is_empty() {
        return Ok(None);
    }
    let quote_currency = quote_currency
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("USD")
        .to_ascii_lowercase();

    let aliases = price_symbol_aliases(symbol);
    let search_terms = if aliases.is_empty() {
        vec![symbol.to_string()]
    } else {
        aliases.clone()
    };
    let mut matched_coin: Option<CoinGeckoCoin> = None;
    for search_term in search_terms {
        let search = client
            .get("https://api.coingecko.com/api/v3/search")
            .query(&[("query", search_term.as_str())])
            .send()
            .await?
            .error_for_status()?
            .json::<CoinGeckoSearchResponse>()
            .await?;
        if let Some(coin) = search.coins.into_iter().find(|coin| {
            coin.symbol.eq_ignore_ascii_case(symbol)
                || coin.name.eq_ignore_ascii_case(symbol)
                || coin.id.eq_ignore_ascii_case(symbol)
                || aliases.iter().any(|alias| {
                    coin.id.eq_ignore_ascii_case(alias)
                        || coin.symbol.eq_ignore_ascii_case(alias)
                        || coin.name.eq_ignore_ascii_case(alias)
                })
        }) {
            matched_coin = Some(coin);
            break;
        }
    }
    let Some(coin) = matched_coin else {
        return Ok(None);
    };
    let normalized_symbol = coin.symbol.to_ascii_lowercase();

    let price_payload = client
        .get("https://api.coingecko.com/api/v3/simple/price")
        .query(&[
            ("ids", coin.id.as_str()),
            ("vs_currencies", quote_currency.as_str()),
            ("include_last_updated_at", "true"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;
    let Some(price) = price_payload
        .get(&coin.id)
        .and_then(|entry| entry.get(&quote_currency))
        .and_then(serde_json::Value::as_f64)
    else {
        return Ok(None);
    };

    let observed = format!("{} {:.8}", quote_currency.to_ascii_uppercase(), price)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string();
    let last_updated = price_payload
        .get(&coin.id)
        .and_then(|entry| entry.get("last_updated_at"))
        .and_then(serde_json::Value::as_i64)
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_else(now_rfc3339);
    let source = RealtimeSourceRecord {
        title: format!("{} ({}) current price", coin.name, coin.symbol),
        url: format!("https://www.coingecko.com/en/coins/{}", coin.id),
        snippet: format!(
            "{} current live price quote is {}. Observed at {}.",
            normalized_symbol.to_ascii_uppercase(),
            observed,
            last_updated
        ),
        fetched_excerpt: String::new(),
        observed_at: Some(last_updated),
        published_at: None,
    };

    Ok(Some((source, observed)))
}

fn format_price_display_zh(
    symbol: &str,
    observed_price_text: Option<&str>,
    sources: &[RealtimeSourceRecord],
) -> String {
    let price = observed_price_text.unwrap_or("未识别到当前报价");
    let source = sources
        .first()
        .map(|source| format!("来源：{}（{}）", source.title, source.url))
        .unwrap_or_else(|| "来源：无可用来源".to_string());
    format!("{symbol} 当前价格约为 {price}。{source}。")
}

fn format_price_display_en(
    symbol: &str,
    observed_price_text: Option<&str>,
    sources: &[RealtimeSourceRecord],
) -> String {
    let price = observed_price_text.unwrap_or("no current quote found");
    let source = sources
        .first()
        .map(|source| format!("Source: {} ({})", source.title, source.url))
        .unwrap_or_else(|| "Source: no usable source".to_string());
    format!("{symbol} current price is about {price}. {source}.")
}

#[async_trait]
impl Tool for PriceLookupTool {
    fn name(&self) -> String {
        "price_lookup".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "price_lookup".to_string(),
            description: "Lookup recent market price information for a symbol or asset and return structured source evidence.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "symbol": {"type": "string"},
                    "market": {"type": "string"},
                    "quote_currency": {"type": "string"},
                    "query": {"type": "string", "description": "Optional fully specified price query."}
                },
                "required": ["symbol"]
            }),
            parameters_ts: Some("interface PriceLookup { symbol: string; market?: string; quote_currency?: string; query?: string; }".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use for latest price and quote questions once the target symbol or asset is known. Prefer this over generic web search.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: PriceArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Invalid arguments: {e}"))?;
        let symbol = args.symbol.trim();
        if symbol.is_empty() {
            anyhow::bail!("symbol is required");
        }

        let normalized_query = args
            .query
            .map(|q| q.trim().to_string())
            .filter(|q| !q.is_empty())
            .unwrap_or_else(|| {
                let mut pieces = vec![symbol.to_string()];
                if let Some(market) = args
                    .market
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    pieces.push(market.to_string());
                }
                if let Some(currency) = args
                    .quote_currency
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    pieces.push(currency.to_string());
                }
                pieces.extend(
                    ["current", "price", "live", "quote"]
                        .into_iter()
                        .map(str::to_string),
                );
                pieces.join(" ")
            });

        let structured_quote = match lookup_public_market_quote(&self.client, symbol)
            .await
            .ok()
            .flatten()
        {
            Some(quote) => Some(quote),
            None => match lookup_public_ticker_quote(&self.client, symbol, args.market.as_deref())
                .await
                .ok()
                .flatten()
            {
                Some(quote) => Some(quote),
                None => {
                    lookup_public_crypto_price(&self.client, symbol, args.quote_currency.as_deref())
                        .await
                        .ok()
                        .flatten()
                }
            },
        };
        let (sources, observed_price_text) = if let Some((source, observed)) = structured_quote {
            (vec![source], Some(observed))
        } else {
            let sources = run_structured_search_fetch(
                &self.search_tool,
                &self.fetch_tool,
                &normalized_query,
                PRICE_MAX_FETCHED_RESULTS,
            )
            .await?;
            let observed_price_text = sources
                .iter()
                .find_map(|source| extract_current_price_from_source(symbol, source));
            (sources, observed_price_text)
        };
        if observed_price_text.is_none() {
            anyhow::bail!(
                "Structured price lookup did not yield a current market quote for '{}'",
                symbol
            );
        }
        ensure_realtime_sources_are_fresh("price_lookup", &sources)?;
        let realtime_receipt = realtime_receipt(
            "price_lookup",
            &normalized_query,
            "verified",
            &sources,
            observed_price_text.as_ref().map(|observed| {
                json!({
                    "symbol": symbol,
                    "observed_price_text": observed,
                    "quote_currency": args.quote_currency.as_deref().map(str::trim).filter(|value| !value.is_empty()),
                })
            }),
            Vec::new(),
        );

        let verification_preview =
            serde_json::to_value(build_verified_verification_result_envelope(
                VerificationDomain::KnowledgeFact,
                VerificationMode::RealtimeLookup,
                sources
                    .iter()
                    .map(|source| {
                        verification_source(
                            "price_source",
                            source.title.clone(),
                            source.url.clone(),
                            realtime_source_timestamp(source),
                        )
                    })
                    .collect(),
                "price lookup completed from structured search and fetch",
            ))?;
        let verification_followup = serde_json::to_value(structured_lookup_followup_plan())?;
        let (route_reason, orchestration_decision) = realtime_lookup_orchestration_payload(
            realtime_lookup_plan(),
            &verification_preview,
            &verification_followup,
        )?;

        let payload = json!({
            "kind": "price_lookup",
            "symbol": symbol,
            "market": args.market.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()),
            "quote_currency": args.quote_currency.map(|v| v.trim().to_uppercase()).filter(|v| !v.is_empty()),
            "normalized_query": normalized_query,
            "query_time": now_rfc3339(),
            "observed_price_text": observed_price_text.clone(),
            "display": {
                "zh": format_price_display_zh(symbol, observed_price_text.as_deref(), &sources),
                "en": format_price_display_en(symbol, observed_price_text.as_deref(), &sources)
            },
            "realtime_receipt": realtime_receipt,
            "verification_preview": verification_preview,
            "verification_followup": verification_followup,
            "route_reason": route_reason,
            "orchestration_decision": orchestration_decision,
            "sources": sources
        });

        Ok(serde_json::to_string_pretty(&payload)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn extract_observed_price_text_finds_first_candidate() {
        let value =
            extract_observed_price_text("AAPL price now $198.42 after hours").expect("price");
        assert!(value.contains("198.42"));
    }

    #[test]
    fn extract_observed_price_text_ignores_dates_near_market_text() {
        let value = extract_observed_price_text(
            "Bitcoin BTC·79,630.45 -1.84% # Bitcoin price today. \
             14 May 2026. BTC trades at $79,314 on major markets.",
        )
        .expect("price");

        assert!(value.contains("79"));
        assert_ne!(value, "14");
    }

    #[test]
    fn extract_observed_price_text_rejects_plain_dates() {
        let value =
            extract_observed_price_text("Bitcoin daily analysis, updated 14 May 2026 at 09:30.");
        assert!(value.is_none());
    }

    #[test]
    fn extract_observed_price_text_accepts_currency_code() {
        let value =
            extract_observed_price_text("Gold current price USD 2,391.20 today").expect("price");
        assert!(value.contains("2,391.20"));
    }

    #[test]
    fn extract_observed_price_text_prefers_trading_quote_over_level() {
        let value = extract_observed_price_text(
            "Quick Answer: Bitcoin has broken above the $78,932 resistance level \
             and trades at $79,948 on Binance (May 4, 2026).",
        )
        .expect("price");

        assert_eq!(value, "$79,948");
    }

    #[test]
    fn current_price_extraction_rejects_unrelated_historical_purchase_price() {
        let source = RealtimeSourceRecord {
            title: "MicroStrategy".to_string(),
            url: "https://en.wikipedia.org/?curid=1449636".to_string(),
            snippet: "subsidiaries held approximately 130,000 BTC, acquired at an aggregate purchase price of $3.98 billion at an average purchase price of $30,639 per bitcoin.".to_string(),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: None,
        };

        assert_eq!(extract_current_price_from_source("BTC", &source), None);
    }

    #[test]
    fn price_aliases_cover_common_localized_asset_names() {
        let aliases = price_symbol_aliases("比特币");
        assert!(aliases.iter().any(|alias| alias == "bitcoin"));
        assert!(aliases.iter().any(|alias| alias == "btc"));

        let eth_aliases = price_symbol_aliases("以太坊");
        assert!(eth_aliases.iter().any(|alias| alias == "ethereum"));
        assert!(eth_aliases.iter().any(|alias| alias == "eth"));
    }

    #[test]
    fn public_market_quote_source_matches_localized_index_name() {
        let policy = policy::RealtimeLookupPolicy::default();
        let source = policy
            .market_quote_source_for_symbol("纳斯达克点数多少")
            .expect("market quote source");

        assert!(source.title.contains("NASDAQ"));
    }

    #[test]
    fn public_market_quote_source_matches_dow_jones_index_name() {
        let policy = policy::RealtimeLookupPolicy::default();
        let source = policy
            .market_quote_source_for_symbol("道琼斯指数多少")
            .expect("market quote source");

        assert!(source.title.contains("Dow Jones"));
    }

    #[test]
    fn extract_public_market_quote_reads_google_finance_price_attr() {
        let value = extract_public_market_quote_text(
            policy::PublicMarketQuoteParser::GoogleFinanceLastPrice,
            r#"<div data-last-price="26402.344"></div>"#,
        )
        .expect("quote");

        assert_eq!(value, "26,402.34");
    }

    #[test]
    fn extract_public_market_quote_reads_stooq_csv_close() {
        let value = extract_public_market_quote_text(
            policy::PublicMarketQuoteParser::StooqCsvClose,
            "Symbol,Date,Time,Open,High,Low,Close,Volume\n^NDQ,2026-05-13,23:00:00,26147.65,26474.18,25990.16,26402.34,5909022692\n",
        )
        .expect("quote");

        assert_eq!(value, "26,402.34");
    }

    #[test]
    fn public_ticker_source_candidates_default_to_public_us_quote_source() {
        let candidates = public_ticker_source_candidates("aapl", None);

        assert!(candidates
            .iter()
            .any(|(symbol, url)| symbol == "AAPL.US" && url.contains("stooq.com")));
    }

    #[test]
    fn normalize_public_ticker_symbol_accepts_ascii_market_symbols_only() {
        assert_eq!(
            normalize_public_ticker_symbol("AAPL 股票").as_deref(),
            Some("aapl")
        );
        assert_eq!(normalize_public_ticker_symbol("比特币"), None);
    }

    #[test]
    fn current_price_extraction_rejects_stale_history_articles() {
        let source = RealtimeSourceRecord {
            title: "比特币一年翻6倍?用Python动态可视化比特币价格变动趋势".to_string(),
            url: "https://example.com/bitcoin-history".to_string(),
            snippet: "2021年3月16日，本文回顾比特币价格趋势，示例数据 300。".to_string(),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: None,
        };

        assert_eq!(extract_current_price_from_source("比特币", &source), None);
    }

    #[test]
    fn current_price_extraction_accepts_symbol_aligned_live_quote() {
        let source = RealtimeSourceRecord {
            title: "Bitcoin BTC Price Today".to_string(),
            url: "https://example.com/price/bitcoin".to_string(),
            snippet: "Bitcoin current price is $79,948 live quote in USD.".to_string(),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: None,
        };

        assert_eq!(
            extract_current_price_from_source("BTC", &source).as_deref(),
            Some("$79,948")
        );
    }

    #[test]
    fn parse_search_results_accepts_structured_payload() {
        let payload = json!({
            "kind": "web_search",
            "results": [
                {
                    "title": "Bitcoin Price Today",
                    "url": "https://example.com/btc",
                    "snippet": "Bitcoin current live quote"
                }
            ]
        });
        let results = parse_search_results(&payload.to_string()).expect("results");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/btc");
    }

    #[test]
    fn parse_search_results_reports_blocked_payload_without_json_noise() {
        let error = parse_search_results(
            "status: blocked\nexecuted_tool: web_search\nresults: []\nblockers: no candidates",
        )
        .expect_err("blocked");

        assert!(error.to_string().contains("status: blocked"));
        assert!(!error.to_string().contains("expected value"));
    }

    #[test]
    fn feed_parser_extracts_rss_items() {
        let body = r#"
            <rss><channel><item>
              <title><![CDATA[Example headline]]></title>
              <link>https://example.com/news/1</link>
              <description>Example summary</description>
              <pubDate>Thu, 14 May 2026 12:00:00 GMT</pubDate>
            </item></channel></rss>
        "#;
        let blocks = feed_item_blocks(body);
        let title = xml_text(&blocks[0], "title").expect("title");
        let link = link_from_feed_block(&blocks[0]).expect("link");

        assert_eq!(title, "Example headline");
        assert_eq!(link, "https://example.com/news/1");
    }

    #[test]
    fn generic_latest_news_query_does_not_over_filter_feed_items() {
        let policy = policy::RealtimeLookupPolicy::default();
        assert!(policy
            .latest_info_filter_terms("latest news 2026-05-14")
            .is_empty());
        assert!(policy.feed_item_matches_topic(
            "<item><title>Any current headline</title></item>",
            "latest news 2026-05-14"
        ));
        assert!(policy.latest_info_topic_is_generic_news("latest news 2026-05-14"));
        assert!(policy.latest_info_topic_is_generic_news("today's latest news 2026-05-14"));
        assert!(policy.latest_info_topic_is_generic_news("今天最新时事新闻"));
        assert!(policy
            .latest_info_filter_terms("帮我查一下今天最新时事新闻，用中文简要列出并给出来源。")
            .is_empty());
        assert_eq!(
            policy.normalized_latest_info_query(
                "帮我查一下今天最新时事新闻，用中文简要列出并给出来源。",
                None,
                &preferred_query_date(None),
            ),
            format!("latest news {}", preferred_query_date(None))
        );
        assert!(!policy.latest_info_topic_is_generic_news("latest OpenAI safety policy news"));
    }

    #[test]
    fn realtime_lookup_policy_yaml_can_extend_latest_news_terms() {
        let policy = policy::RealtimeLookupPolicy::from_yaml(
            r#"
latest_info:
  additional_request_scaffold:
    - 麻烦看看
  additional_non_ascii_generic_terms:
    - 要闻
"#,
        )
        .expect("policy");

        assert!(policy.latest_info_topic_is_generic_news("麻烦看看今日要闻"));
        assert_eq!(
            policy.normalized_latest_info_query(
                "麻烦看看今日要闻",
                None,
                &preferred_query_date(None)
            ),
            format!("latest news {}", preferred_query_date(None))
        );
    }

    #[test]
    fn source_record_filters_search_result_pages() {
        let record = RealtimeSourceRecord {
            title: "新闻搜索".to_string(),
            url: "https://news.so.com/ns?q=%E6%96%B0%E9%97%BB".to_string(),
            snippet: "搜索结果页".to_string(),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: None,
        };

        assert!(!source_record_is_usable(&record));
    }

    #[test]
    fn source_record_filters_search_engine_homepages() {
        let record = RealtimeSourceRecord {
            title: "Google".to_string(),
            url: "https://www.google.com/".to_string(),
            snippet: String::new(),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: None,
        };

        assert!(!source_record_is_usable(&record));
    }

    #[test]
    fn source_record_accepts_real_article_pages() {
        let record = RealtimeSourceRecord {
            title: "Heart disease treatment study".to_string(),
            url: "https://www.thelancet.com/journals/lancet/article/example".to_string(),
            snippet: "A randomized trial of a new treatment".to_string(),
            fetched_excerpt: String::new(),
            observed_at: Some(now_rfc3339()),
            published_at: None,
        };

        assert!(source_record_is_usable(&record));
    }

    #[tokio::test]
    async fn weather_lookup_definition_is_structured() {
        let tool = WeatherLookupTool::new().expect("tool");
        let definition = tool.definition().await;
        assert_eq!(definition.name, "weather_lookup");
        assert!(definition
            .parameters_ts
            .as_deref()
            .unwrap_or_default()
            .contains("location: string"));
    }

    #[tokio::test]
    async fn fx_lookup_definition_is_structured() {
        let tool = FxLookupTool::new().expect("tool");
        let definition = tool.definition().await;
        assert_eq!(definition.name, "fx_lookup");
        assert!(definition
            .parameters_ts
            .as_deref()
            .unwrap_or_default()
            .contains("base_currency"));
    }

    #[test]
    fn realtime_lookup_orchestration_can_finalize_structured_lookup() {
        let preview = serde_json::to_value(build_verified_verification_result_envelope(
            VerificationDomain::KnowledgeFact,
            VerificationMode::RealtimeLookup,
            vec![verification_source(
                "fx_api",
                "Frankfurter FX API",
                "https://api.frankfurter.app/latest",
                Some(now_rfc3339()),
            )],
            "structured FX lookup completed",
        ))
        .expect("preview");
        let followup = serde_json::to_value(structured_lookup_followup_plan()).expect("followup");
        let (route_reason, decision) =
            realtime_lookup_orchestration_payload(realtime_lookup_plan(), &preview, &followup)
                .expect("decision");
        let decision: Value = decision;

        assert_eq!(route_reason, "structured_lookup_can_answer_directly");
        assert_eq!(
            decision["termination"],
            Value::String("FinalizeStructuredLookup".to_string())
        );
        assert_eq!(decision["can_finalize_answer"], Value::Bool(true));
    }

    #[test]
    fn web_backed_lookup_orchestration_finalizes_with_sources() {
        let preview = serde_json::to_value(build_verified_verification_result_envelope(
            VerificationDomain::KnowledgeFact,
            VerificationMode::WebSearchFetch,
            vec![verification_source(
                "web_source",
                "Example source",
                "https://example.com/article",
                Some(now_rfc3339()),
            )],
            "latest-info lookup completed from structured search and fetch",
        ))
        .expect("preview");
        let followup =
            serde_json::to_value(build_source_observed_followup_plan(true)).expect("followup");
        let (route_reason, decision) =
            realtime_lookup_orchestration_payload(web_backed_lookup_plan(), &preview, &followup)
                .expect("decision");
        let decision: Value = decision;

        assert_eq!(
            route_reason,
            "external_fact_requires_search_then_source_read"
        );
        assert_eq!(
            decision["termination"],
            Value::String("FinalizeWithSources".to_string())
        );
        assert_eq!(decision["can_finalize_answer"], Value::Bool(true));
    }
}
