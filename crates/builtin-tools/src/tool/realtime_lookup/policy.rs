use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RealtimeLookupPolicy {
    #[serde(default)]
    latest_info: LatestInfoPolicy,
    #[serde(default)]
    market_quotes: MarketQuotePolicy,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LatestInfoPolicy {
    #[serde(default = "default_latest_info_min_sources")]
    min_sources: usize,
    #[serde(default = "default_latest_info_generic_min_sources")]
    generic_min_sources: usize,
    #[serde(default = "default_request_scaffold")]
    request_scaffold: Vec<String>,
    #[serde(default)]
    additional_request_scaffold: Vec<String>,
    #[serde(default = "default_ascii_generic_terms")]
    ascii_generic_terms: Vec<String>,
    #[serde(default)]
    additional_ascii_generic_terms: Vec<String>,
    #[serde(default = "default_non_ascii_generic_terms")]
    non_ascii_generic_terms: Vec<String>,
    #[serde(default)]
    additional_non_ascii_generic_terms: Vec<String>,
    #[serde(default = "default_ascii_filter_noise_terms")]
    ascii_filter_noise_terms: Vec<String>,
    #[serde(default)]
    additional_ascii_filter_noise_terms: Vec<String>,
    #[serde(default = "default_non_ascii_filter_noise_terms")]
    non_ascii_filter_noise_terms: Vec<String>,
    #[serde(default)]
    additional_non_ascii_filter_noise_terms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MarketQuotePolicy {
    #[serde(default = "default_market_quote_sources")]
    sources: Vec<MarketQuoteSourcePolicy>,
    #[serde(default)]
    additional_sources: Vec<MarketQuoteSourcePolicy>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MarketQuoteSourcePolicy {
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) endpoints: Vec<MarketQuoteEndpointPolicy>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MarketQuoteEndpointPolicy {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) parser: PublicMarketQuoteParser,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PublicMarketQuoteParser {
    StooqCsvClose,
    GoogleFinanceLastPrice,
}

impl Default for RealtimeLookupPolicy {
    fn default() -> Self {
        Self {
            latest_info: LatestInfoPolicy::default(),
            market_quotes: MarketQuotePolicy::default(),
        }
    }
}

impl Default for LatestInfoPolicy {
    fn default() -> Self {
        Self {
            request_scaffold: default_request_scaffold(),
            min_sources: default_latest_info_min_sources(),
            generic_min_sources: default_latest_info_generic_min_sources(),
            additional_request_scaffold: Vec::new(),
            ascii_generic_terms: default_ascii_generic_terms(),
            additional_ascii_generic_terms: Vec::new(),
            non_ascii_generic_terms: default_non_ascii_generic_terms(),
            additional_non_ascii_generic_terms: Vec::new(),
            ascii_filter_noise_terms: default_ascii_filter_noise_terms(),
            additional_ascii_filter_noise_terms: Vec::new(),
            non_ascii_filter_noise_terms: default_non_ascii_filter_noise_terms(),
            additional_non_ascii_filter_noise_terms: Vec::new(),
        }
    }
}

impl Default for MarketQuotePolicy {
    fn default() -> Self {
        Self {
            sources: default_market_quote_sources(),
            additional_sources: Vec::new(),
        }
    }
}

impl RealtimeLookupPolicy {
    pub(crate) fn load() -> Self {
        let mut policy = Self::default();
        if let Some(runtime_policy) = read_runtime_policy() {
            policy.merge(runtime_policy);
        }
        policy
    }

    pub(crate) fn strip_latest_info_request_scaffold(&self, topic: &str) -> String {
        let mut text = topic.to_string();
        for phrase in self.latest_info.request_scaffold_terms() {
            text = text.replace(phrase, " ");
        }
        text
    }

    pub(crate) fn latest_info_topic_is_generic_news(&self, topic: &str) -> bool {
        let stripped = self.strip_latest_info_request_scaffold(topic);
        let lowered = stripped.to_ascii_lowercase();
        let ascii_generic_terms = self.latest_info.ascii_generic_terms();
        let non_ascii_generic_terms = self.latest_info.non_ascii_generic_terms();
        let has_news_marker = ascii_generic_terms
            .iter()
            .any(|term| lowered.contains(term))
            || non_ascii_generic_terms
                .iter()
                .any(|term| stripped.contains(term));
        if !has_news_marker {
            return false;
        }

        let mut ascii_residual = lowered;
        for generic in ascii_generic_terms {
            ascii_residual = ascii_residual.replace(generic, "");
        }

        let mut original_residual = stripped;
        for generic in non_ascii_generic_terms {
            original_residual = original_residual.replace(generic, "");
        }

        let ascii_specific = ascii_residual
            .chars()
            .filter(|ch| !ch.is_ascii_digit())
            .filter(|ch| !ch.is_ascii_whitespace())
            .filter(|ch| !matches!(ch, '-' | '_' | '/' | '\\' | '.' | ',' | ':' | ';' | '\''))
            .collect::<String>();
        let original_specific = original_residual
            .chars()
            .filter(|ch| !ch.is_ascii_digit())
            .filter(|ch| !ch.is_whitespace())
            .filter(|ch| {
                !matches!(
                    ch,
                    '-' | '_'
                        | '/'
                        | '\\'
                        | '.'
                        | ','
                        | ':'
                        | ';'
                        | '，'
                        | '。'
                        | '：'
                        | '；'
                        | '（'
                        | '）'
                )
            })
            .collect::<String>();
        ascii_specific.is_empty() || original_specific.is_empty()
    }

    pub(crate) fn latest_info_filter_terms(&self, topic: &str) -> Vec<String> {
        if self.latest_info_topic_is_generic_news(topic) {
            return Vec::new();
        }

        let topic = self.strip_latest_info_request_scaffold(topic);
        let ascii_noise = self.latest_info.ascii_filter_noise_terms();
        let non_ascii_noise = self.latest_info.non_ascii_filter_noise_terms();
        topic
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        ',' | '.'
                            | ';'
                            | ':'
                            | '/'
                            | '\\'
                            | '|'
                            | '('
                            | ')'
                            | '['
                            | ']'
                            | '{'
                            | '}'
                            | '，'
                            | '。'
                            | '；'
                            | '：'
                            | '（'
                            | '）'
                    )
            })
            .map(str::trim)
            .filter(|term| term.chars().count() >= 2)
            .filter(|term| {
                let lowered = term.to_ascii_lowercase();
                !ascii_noise.iter().any(|noise| lowered == *noise)
                    && !lowered.chars().all(|ch| ch.is_ascii_digit() || ch == '-')
                    && !non_ascii_noise.iter().any(|noise| *term == *noise)
            })
            .map(|term| term.to_ascii_lowercase())
            .collect()
    }

    pub(crate) fn normalized_latest_info_query(
        &self,
        topic: &str,
        query: Option<&str>,
        date: &str,
    ) -> String {
        if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
            return query.to_string();
        }

        if self.latest_info_topic_is_generic_news(topic) {
            return format!("latest news {date}");
        }

        let cleaned_topic = self
            .strip_latest_info_request_scaffold(topic)
            .trim()
            .trim_matches(['，', '。', ',', '.', ':', '：'])
            .trim()
            .to_string();
        if cleaned_topic.is_empty() {
            format!("latest news {date}")
        } else {
            format!("latest {cleaned_topic} {date}")
        }
    }

    pub(crate) fn feed_item_matches_topic(&self, block: &str, topic: &str) -> bool {
        let terms = self.latest_info_filter_terms(topic);
        if terms.is_empty() {
            return true;
        }
        let haystack = block.to_ascii_lowercase();
        terms.iter().any(|term| haystack.contains(term))
    }

    pub(crate) fn latest_info_min_sources(&self, topic: &str) -> usize {
        if self.latest_info_topic_is_generic_news(topic) {
            self.latest_info.generic_min_sources.max(1)
        } else {
            self.latest_info.min_sources.max(1)
        }
    }

    pub(crate) fn market_quote_source_for_symbol(
        &self,
        symbol: &str,
    ) -> Option<MarketQuoteSourcePolicy> {
        let normalized = normalize_market_quote_key(symbol);
        if normalized.is_empty() {
            return None;
        }
        self.market_quotes.sources().into_iter().find(|source| {
            source.aliases.iter().any(|alias| {
                let alias = normalize_market_quote_key(alias);
                !alias.is_empty() && (normalized == alias || normalized.contains(&alias))
            })
        })
    }

    fn merge(&mut self, other: Self) {
        self.latest_info.merge(other.latest_info);
        self.market_quotes.merge(other.market_quotes);
    }

    #[cfg(test)]
    pub(crate) fn from_yaml(yaml: &str) -> Result<Self, String> {
        let mut policy = Self::default();
        let parsed = serde_yaml_ng::from_str::<Self>(yaml).map_err(|error| error.to_string())?;
        policy.merge(parsed);
        Ok(policy)
    }
}

impl LatestInfoPolicy {
    fn request_scaffold_terms(&self) -> Vec<&str> {
        merged_terms(&self.request_scaffold, &self.additional_request_scaffold)
    }

    fn ascii_generic_terms(&self) -> Vec<&str> {
        merged_terms(
            &self.ascii_generic_terms,
            &self.additional_ascii_generic_terms,
        )
    }

    fn non_ascii_generic_terms(&self) -> Vec<&str> {
        merged_terms(
            &self.non_ascii_generic_terms,
            &self.additional_non_ascii_generic_terms,
        )
    }

    fn ascii_filter_noise_terms(&self) -> Vec<String> {
        merged_terms(
            &self.ascii_filter_noise_terms,
            &self.additional_ascii_filter_noise_terms,
        )
        .into_iter()
        .map(str::to_ascii_lowercase)
        .collect()
    }

    fn non_ascii_filter_noise_terms(&self) -> Vec<&str> {
        merged_terms(
            &self.non_ascii_filter_noise_terms,
            &self.additional_non_ascii_filter_noise_terms,
        )
    }

    fn merge(&mut self, other: Self) {
        if other.min_sources != default_latest_info_min_sources() {
            self.min_sources = other.min_sources;
        }
        if other.generic_min_sources != default_latest_info_generic_min_sources() {
            self.generic_min_sources = other.generic_min_sources;
        }
        if !other.request_scaffold.is_empty() {
            self.request_scaffold = other.request_scaffold;
        }
        append_unique(
            &mut self.additional_request_scaffold,
            other.additional_request_scaffold,
        );

        if !other.ascii_generic_terms.is_empty() {
            self.ascii_generic_terms = other.ascii_generic_terms;
        }
        append_unique(
            &mut self.additional_ascii_generic_terms,
            other.additional_ascii_generic_terms,
        );

        if !other.non_ascii_generic_terms.is_empty() {
            self.non_ascii_generic_terms = other.non_ascii_generic_terms;
        }
        append_unique(
            &mut self.additional_non_ascii_generic_terms,
            other.additional_non_ascii_generic_terms,
        );

        if !other.ascii_filter_noise_terms.is_empty() {
            self.ascii_filter_noise_terms = other.ascii_filter_noise_terms;
        }
        append_unique(
            &mut self.additional_ascii_filter_noise_terms,
            other.additional_ascii_filter_noise_terms,
        );

        if !other.non_ascii_filter_noise_terms.is_empty() {
            self.non_ascii_filter_noise_terms = other.non_ascii_filter_noise_terms;
        }
        append_unique(
            &mut self.additional_non_ascii_filter_noise_terms,
            other.additional_non_ascii_filter_noise_terms,
        );
    }
}

impl MarketQuotePolicy {
    fn sources(&self) -> Vec<MarketQuoteSourcePolicy> {
        let mut sources = self.sources.clone();
        for source in &self.additional_sources {
            if source.aliases.is_empty() || source.endpoints.is_empty() {
                continue;
            }
            let duplicate = sources.iter().any(|existing| {
                existing.aliases.iter().any(|left| {
                    source.aliases.iter().any(|right| {
                        normalize_market_quote_key(left) == normalize_market_quote_key(right)
                    })
                })
            });
            if !duplicate {
                sources.push(source.clone());
            }
        }
        sources
    }

    fn merge(&mut self, other: Self) {
        if !other.sources.is_empty() {
            self.sources = other.sources;
        }
        self.additional_sources.extend(other.additional_sources);
    }
}

fn normalize_market_quote_key(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| {
            !ch.is_whitespace()
                && !matches!(
                    ch,
                    '?' | '？'
                        | '。'
                        | '，'
                        | ','
                        | '.'
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
        .collect()
}

fn read_runtime_policy() -> Option<RealtimeLookupPolicy> {
    for path in runtime_policy_candidate_paths() {
        if let Some(policy) = read_policy_file(&path) {
            return Some(policy);
        }
    }
    None
}

fn runtime_policy_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = std::env::var("BENSHU_REALTIME_LOOKUP_POLICY") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            paths.push(PathBuf::from(trimmed));
        }
    }
    paths.push(PathBuf::from("data/policies/tools/realtime_lookup.yaml"));
    paths.push(PathBuf::from("policies/tools/realtime_lookup.yaml"));
    paths
}

fn read_policy_file(path: &Path) -> Option<RealtimeLookupPolicy> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_yaml_ng::from_str::<RealtimeLookupPolicy>(&content).ok()
}

fn merged_terms<'a>(base: &'a [String], additional: &'a [String]) -> Vec<&'a str> {
    let mut terms = Vec::new();
    for term in base.iter().chain(additional.iter()) {
        let trimmed = term.trim();
        if trimmed.is_empty()
            || terms
                .iter()
                .any(|existing: &&str| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        terms.push(trimmed);
    }
    terms
}

fn append_unique(target: &mut Vec<String>, source: Vec<String>) {
    for value in source {
        let trimmed = value.trim();
        if trimmed.is_empty()
            || target
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        target.push(trimmed.to_string());
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn default_market_quote_sources() -> Vec<MarketQuoteSourcePolicy> {
    vec![
        MarketQuoteSourcePolicy {
            aliases: strings(&[
                "纳斯达克",
                "纳指",
                "纳斯达克综合",
                "纳斯达克综合指数",
                "nasdaq",
                "nasdaq composite",
                "ixic",
                "^ixic",
            ]),
            title: "NASDAQ Composite current index quote".to_string(),
            endpoints: vec![
                MarketQuoteEndpointPolicy {
                    title: "Stooq NASDAQ Composite quote".to_string(),
                    url: "https://stooq.com/q/l/?s=^ndq&f=sd2t2ohlcv&h&e=csv".to_string(),
                    parser: PublicMarketQuoteParser::StooqCsvClose,
                },
                MarketQuoteEndpointPolicy {
                    title: "Google Finance NASDAQ Composite quote".to_string(),
                    url: "https://www.google.com/finance/quote/.IXIC:INDEXNASDAQ".to_string(),
                    parser: PublicMarketQuoteParser::GoogleFinanceLastPrice,
                },
            ],
        },
        MarketQuoteSourcePolicy {
            aliases: strings(&[
                "道琼斯",
                "道指",
                "道琼斯指数",
                "道琼斯工业平均指数",
                "dow",
                "dow jones",
                "dow jones industrial average",
                "djia",
                "^dji",
            ]),
            title: "Dow Jones Industrial Average current index quote".to_string(),
            endpoints: vec![
                MarketQuoteEndpointPolicy {
                    title: "Stooq Dow Jones Industrial Average quote".to_string(),
                    url: "https://stooq.com/q/l/?s=^dji&f=sd2t2ohlcv&h&e=csv".to_string(),
                    parser: PublicMarketQuoteParser::StooqCsvClose,
                },
                MarketQuoteEndpointPolicy {
                    title: "Google Finance Dow Jones Industrial Average quote".to_string(),
                    url: "https://www.google.com/finance/quote/.DJI:INDEXDJX".to_string(),
                    parser: PublicMarketQuoteParser::GoogleFinanceLastPrice,
                },
            ],
        },
        MarketQuoteSourcePolicy {
            aliases: strings(&[
                "标普500",
                "标普 500",
                "标准普尔500",
                "标准普尔500指数",
                "s&p 500",
                "sp500",
                "snp 500",
                "spx",
                "^spx",
            ]),
            title: "S&P 500 current index quote".to_string(),
            endpoints: vec![MarketQuoteEndpointPolicy {
                title: "Stooq S&P 500 quote".to_string(),
                url: "https://stooq.com/q/l/?s=^spx&f=sd2t2ohlcv&h&e=csv".to_string(),
                parser: PublicMarketQuoteParser::StooqCsvClose,
            }],
        },
    ]
}

fn default_request_scaffold() -> Vec<String> {
    strings(&[
        "帮我查一下",
        "帮我查",
        "查一下",
        "查询",
        "请查",
        "用中文简要列出并给出来源",
        "用中文简要列出",
        "用中文回答并给出来源",
        "用中文回答",
        "简要列出并给出来源",
        "给出来源",
        "列出来源",
        "请",
        "please",
        "can you",
        "could you",
        "answer in chinese",
        "in chinese",
        "briefly",
        "with sources",
        "sources",
    ])
}

fn default_latest_info_min_sources() -> usize {
    1
}

fn default_latest_info_generic_min_sources() -> usize {
    2
}

fn default_ascii_generic_terms() -> Vec<String> {
    strings(&[
        "today's",
        "todays",
        "latest",
        "news",
        "today",
        "recent",
        "current",
        "headline",
        "headlines",
        "update",
        "updates",
    ])
}

fn default_non_ascii_generic_terms() -> Vec<String> {
    strings(&["最新", "新闻", "时事", "今天", "今日", "最近", "快讯"])
}

fn default_ascii_filter_noise_terms() -> Vec<String> {
    strings(&[
        "latest",
        "news",
        "today",
        "recent",
        "current",
        "update",
        "updates",
        "headline",
        "headlines",
        "source",
        "sources",
        "answer",
        "brief",
        "list",
    ])
}

fn default_non_ascii_filter_noise_terms() -> Vec<String> {
    strings(&[
        "最新",
        "新闻",
        "时事",
        "今天",
        "今日",
        "最近",
        "帮我",
        "查一下",
        "查询",
        "用中文",
        "中文",
        "简要",
        "列出",
        "给出",
        "来源",
        "回答",
    ])
}
