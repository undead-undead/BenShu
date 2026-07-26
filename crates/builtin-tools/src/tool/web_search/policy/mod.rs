use chrono::Datelike;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::tool::browser_site_policy::{policy_for_host, SiteFetchMode};
use crate::tool::web_search::orchestrator::SourceCapability;
use crate::tool::web_search::policy::artifact_policy::{
    normalize_artifact_policy_value, policy_handle_matches_task,
};
use crate::tool::web_search::policy::policy_bundle::{
    PolicyPhase, RuntimePolicyResolver, TaskPolicyInput,
};

mod artifact_policy;
mod policy_bundle;
mod quality_contract;
pub(crate) mod source;

/// Search intent extracted from user language before source selection.
///
/// This is deliberately deterministic. The LLM can understand the request, but
/// the runtime still needs stable engineering hints so repeated tool calls choose
/// comparable sources instead of drifting into social pages, portals, or shells.
#[derive(Debug, Clone, Default)]
pub(crate) struct LookupIntent {
    pub(crate) base_terms: Vec<String>,
    pub(crate) site_hints: Vec<String>,
    pub(crate) artifact_hints: Vec<String>,
    pub(crate) evidence_hints: Vec<String>,
    pub(crate) freshness_hints: Vec<String>,
    pub(crate) direct_record_hints: Vec<String>,
}

/// Deterministic search/source policy used by delegation.
///
/// The policy layer is intentionally separate from the delegate tool executor:
/// adding a new artifact or public source should not require editing the
/// multi-agent orchestration flow.
pub(crate) struct SearchPolicy;
pub(crate) type BrowserEvidencePolicy = SearchPolicy;

impl SearchPolicy {
    pub(crate) fn push_unique(target: &mut Vec<String>, value: impl Into<String>) {
        let value = value.into();
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

    pub(crate) fn build_lookup_intent(task: &str) -> LookupIntent {
        let trimmed = task.trim();
        if trimmed.is_empty() {
            return LookupIntent::default();
        }

        let lowered = trimmed.to_ascii_lowercase();
        let mut intent = LookupIntent {
            base_terms: Self::task_tokens(trimmed),
            ..LookupIntent::default()
        };

        Self::apply_site_hints(&mut intent, trimmed, &lowered);
        Self::apply_artifact_hints(&mut intent, trimmed, &lowered);
        Self::apply_evidence_hints(&mut intent, trimmed, &lowered);
        Self::apply_freshness_hints(&mut intent, trimmed, &lowered);
        Self::apply_direct_record_hints(&mut intent, trimmed, &lowered);
        Self::apply_runtime_artifact_policies(&mut intent, trimmed, &lowered);
        Self::expand_lookup_intent_with_site_policy(&mut intent);
        intent
    }

    pub(crate) fn browser_search_query_with_task_context(
        query: &str,
        task_context: Option<&str>,
    ) -> String {
        let query = query.trim();
        let Some(task_context) = task_context
            .map(str::trim)
            .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case(query))
        else {
            return query.to_string();
        };

        let lookup_surface = Self::lookup_surface_from_task_context(task_context);
        let query_lowered = query.to_ascii_lowercase();
        let intent = Self::build_lookup_intent(&lookup_surface);
        let mut additions = Vec::new();

        for group in [
            intent.site_hints,
            intent.artifact_hints,
            intent.evidence_hints,
            intent.freshness_hints,
            intent.base_terms,
        ] {
            for term in group {
                let trimmed = term.trim();
                if trimmed.is_empty()
                    || Self::is_lookup_noise_term(trimmed)
                    || Self::looks_like_internal_query_identifier(trimmed)
                    || query.contains(trimmed)
                    || query_lowered.contains(&trimmed.to_ascii_lowercase())
                {
                    continue;
                }
                Self::push_unique(&mut additions, trimmed.to_string());
                if additions.len() >= 10 {
                    break;
                }
            }
            if additions.len() >= 10 {
                break;
            }
        }

        if additions.is_empty() {
            query.to_string()
        } else if query.is_empty() {
            additions.join(" ")
        } else {
            format!("{query} {}", additions.join(" "))
        }
    }

    pub(crate) fn lookup_surface_from_task_context(task_context: &str) -> String {
        let mut surface = task_context.trim();
        for marker in [
            "Original user request:",
            "完整用户请求",
            "User task:",
            "用户请求：",
            "用户请求:",
        ] {
            if let Some(index) = surface.rfind(marker) {
                surface = surface[index + marker.len()..].trim();
                break;
            }
        }

        for marker in [
            "\n\nOriginal delegated task:",
            "\n\nDelegated task:",
            "\nDelegated task:",
            "\n\nRuntime policy bundle:",
            "\n\nRules:",
            "\n\n###",
            "\n###",
        ] {
            if let Some((head, _)) = surface.split_once(marker) {
                surface = head.trim();
            }
        }

        let phase_surface = Self::lookup_phase_surface(surface);
        if phase_surface.trim().is_empty() {
            surface.to_string()
        } else {
            phase_surface
        }
    }

    fn lookup_phase_surface(surface: &str) -> String {
        let mut trimmed = surface.trim().to_string();
        if trimmed.is_empty() {
            return trimmed;
        }

        for marker in [
            "然后",
            "之后",
            "随后",
            "接着",
            "再根据",
            "根据知识库",
            "基于知识库",
            " then ",
            " after that ",
            " afterward ",
            " afterwards ",
        ] {
            if let Some(index) = trimmed
                .to_ascii_lowercase()
                .find(&marker.to_ascii_lowercase())
            {
                let before = trimmed[..index].trim();
                if Self::text_has_lookup_or_source_signal(before) {
                    trimmed = before.to_string();
                    break;
                }
            }
        }

        let mut kept = Vec::new();
        let mut saw_lookup = false;
        for sentence in trimmed
            .split_inclusive(['。', '；', ';', '.', '!', '?', '！', '？', '\n'])
            .map(str::trim)
            .filter(|sentence| !sentence.is_empty())
        {
            if Self::text_has_lookup_or_source_signal(sentence) {
                saw_lookup = true;
                kept.push(sentence.trim_matches(['。', '；', ';']).to_string());
                continue;
            }
            if saw_lookup && Self::text_has_artifact_generation_signal(sentence) {
                break;
            }
            if saw_lookup && Self::text_has_storage_signal(sentence) {
                kept.push(sentence.trim_matches(['。', '；', ';']).to_string());
            }
        }

        if kept.is_empty() {
            trimmed
        } else {
            kept.join(" ")
        }
    }

    fn text_has_lookup_or_source_signal(text: &str) -> bool {
        let lowered = text.to_ascii_lowercase();
        [
            "search", "find", "lookup", "browse", "fetch", "download", "source", "material",
            "content", "text", "搜索", "查找", "检索", "浏览", "抓取", "下载", "来源", "素材",
            "正文", "内容", "网页", "公网",
        ]
        .iter()
        .any(|term| lowered.contains(term) || text.contains(term))
    }

    fn text_has_storage_signal(text: &str) -> bool {
        let lowered = text.to_ascii_lowercase();
        [
            "store",
            "save",
            "import",
            "ingest",
            "archive",
            "knowledge",
            "database",
            "存",
            "保存",
            "导入",
            "收进",
            "收录",
            "入库",
            "知识库",
            "数据库",
        ]
        .iter()
        .any(|term| lowered.contains(term) || text.contains(term))
    }

    fn text_has_artifact_generation_signal(text: &str) -> bool {
        let lowered = text.to_ascii_lowercase();
        [
            "write",
            "draft",
            "create",
            "compose",
            "revise",
            "audit",
            "export",
            "artifact",
            "file",
            "txt",
            "pdf",
            "写",
            "创作",
            "生成",
            "修订",
            "审查",
            "导出",
            "保存成",
            "文件",
        ]
        .iter()
        .any(|term| lowered.contains(term) || text.contains(term))
    }

    fn apply_site_hints(intent: &mut LookupIntent, trimmed: &str, lowered: &str) {
        for rule in SITE_HINT_RULES {
            if Self::hint_rule_matches(trimmed, lowered, rule) {
                Self::push_unique(&mut intent.site_hints, rule.hint);
            }
        }
    }

    fn apply_artifact_hints(intent: &mut LookupIntent, trimmed: &str, lowered: &str) {
        for rule in ARTIFACT_HINT_RULES {
            if Self::hint_rule_matches(trimmed, lowered, rule) {
                Self::push_unique(&mut intent.artifact_hints, rule.hint);
            }
        }

        if Self::task_requests_collection_or_ranking(trimmed) {
            Self::push_unique(&mut intent.artifact_hints, "collection");
            Self::push_unique(&mut intent.artifact_hints, "ranking");
            Self::push_unique(&mut intent.artifact_hints, "list");
        }

        if Self::task_requests_data_or_records(trimmed) {
            Self::push_unique(&mut intent.artifact_hints, "data");
            Self::push_unique(&mut intent.artifact_hints, "records");
        }
    }

    fn apply_evidence_hints(intent: &mut LookupIntent, trimmed: &str, lowered: &str) {
        for rule in EVIDENCE_HINT_RULES {
            if Self::hint_rule_matches(trimmed, lowered, rule) {
                Self::push_unique(&mut intent.evidence_hints, rule.hint);
            }
        }

        if Self::task_requests_data_or_records(trimmed) {
            Self::push_unique(&mut intent.evidence_hints, "official");
            Self::push_unique(&mut intent.evidence_hints, "record");
            Self::push_unique(&mut intent.direct_record_hints, "record");
        }
    }

    fn apply_freshness_hints(intent: &mut LookupIntent, trimmed: &str, lowered: &str) {
        if lowered.contains("latest")
            || lowered.contains("recent")
            || lowered.contains("newest")
            || trimmed.contains("最新")
            || trimmed.contains("最近")
        {
            Self::push_unique(&mut intent.freshness_hints, "2025");
            Self::push_unique(&mut intent.freshness_hints, "2026");
            Self::push_unique(&mut intent.freshness_hints, "latest");
        }
    }

    fn apply_direct_record_hints(intent: &mut LookupIntent, trimmed: &str, lowered: &str) {
        for rule in DIRECT_RECORD_HINT_RULES {
            if Self::hint_rule_matches(trimmed, lowered, rule) {
                Self::push_unique(&mut intent.direct_record_hints, rule.hint);
            }
        }
    }

    fn hint_rule_matches(original: &str, lowered: &str, rule: &HintRule) -> bool {
        if rule.needle.chars().any(|ch| !ch.is_ascii()) {
            return original.contains(rule.needle);
        }
        if rule.needle.contains(' ') {
            return lowered.contains(rule.needle);
        }
        lowered
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
            .any(|token| token == rule.needle)
    }

    pub(crate) fn site_hint_host(site_hint: &str) -> Option<&str> {
        site_hint
            .strip_prefix("site:")
            .or_else(|| site_hint.strip_prefix("SITE:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn preferred_lookup_hosts_for_task(task: &str) -> Vec<String> {
        let intent = Self::build_lookup_intent(task);
        let mut hosts = Vec::new();

        for hint in &intent.site_hints {
            if let Some(host) = Self::site_hint_host(hint) {
                let policy = policy_for_host(host);
                for preferred_host in policy.preferred_lookup_hosts {
                    Self::push_unique(&mut hosts, *preferred_host);
                }
            }
        }

        hosts
    }

    fn expand_lookup_intent_with_site_policy(intent: &mut LookupIntent) {
        let site_hosts = intent
            .site_hints
            .iter()
            .filter_map(|hint| Self::site_hint_host(hint).map(str::to_string))
            .collect::<Vec<_>>();

        for host in site_hosts {
            let policy = policy_for_host(&host);
            for preferred_host in policy.preferred_lookup_hosts {
                Self::push_unique(&mut intent.site_hints, format!("site:{preferred_host}"));
            }

            if policy.challenge_prone {
                Self::push_unique(&mut intent.evidence_hints, "open access");
                Self::push_unique(&mut intent.evidence_hints, "record");
                Self::push_unique(&mut intent.direct_record_hints, "doi");
            }
        }
    }

    pub(crate) fn official_data_lookup_query_hints(task: &str) -> Vec<String> {
        let mut hints = Vec::new();
        let current_year = chrono::Utc::now().year().to_string();
        if task.contains("中国福利彩票") || task.contains("福利彩票") || task.contains("福彩")
        {
            Self::push_unique(
                &mut hints,
                format!("中国福利彩票 开奖公告 开奖号码 {current_year}"),
            );
            Self::push_unique(
                &mut hints,
                format!("中国福利彩票 双色球 开奖公告 开奖号码 {current_year}"),
            );
            Self::push_unique(
                &mut hints,
                format!("site:cwl.gov.cn 中国福利彩票 开奖公告 开奖号码 {current_year}"),
            );
        }
        hints
    }

    pub(crate) fn compact_data_lookup_terms(base_terms: &[String]) -> Vec<String> {
        let mut subject_terms = Vec::new();
        let mut data_terms = Vec::new();
        let mut fallback = Vec::new();
        for term in base_terms {
            if Self::is_data_lookup_instruction_term(term) {
                continue;
            }
            if Self::is_data_lookup_subject_term(term) {
                Self::push_unique(&mut subject_terms, term.clone());
            } else if Self::is_data_lookup_strong_term(term) {
                Self::push_unique(&mut data_terms, term.clone());
            } else {
                Self::push_unique(&mut fallback, term.clone());
            }
        }

        let mut strong = subject_terms;
        for term in data_terms {
            if strong.len() >= 8 {
                break;
            }
            Self::push_unique(&mut strong, term);
        }
        if strong.is_empty() {
            for term in fallback {
                if strong.len() >= 8 {
                    break;
                }
                Self::push_unique(&mut strong, term);
            }
        }
        strong.truncate(8);
        strong
    }

    pub(crate) fn public_data_record_urls_for_task(task: &str) -> Vec<String> {
        let mut urls = PUBLIC_DATA_SOURCE_POLICIES
            .iter()
            .find(|policy| (policy.matches)(task))
            .map(|policy| policy.urls.iter().map(|url| (*url).to_string()).collect())
            .unwrap_or_default();
        let lowered = task.to_ascii_lowercase();
        for policy in Self::runtime_artifact_policies() {
            Self::append_policy_urls_for_task(&mut urls, &policy, task, &lowered);
        }
        urls
    }

    pub(crate) fn source_adapter_names_for_task(task: &str) -> Vec<String> {
        let mut sources = Vec::new();
        let lowered = task.to_ascii_lowercase();
        for policy in Self::runtime_artifact_policies() {
            Self::append_policy_sources_for_task(&mut sources, &policy, task, &lowered);
        }
        sources
    }

    pub(crate) fn source_adapter_overrides_for_task(
        task: &str,
    ) -> Vec<RuntimeSourceAdapterOverride> {
        let lowered = task.to_ascii_lowercase();
        let mut overrides = Vec::new();
        for policy in Self::runtime_artifact_policies() {
            Self::append_policy_source_adapter_overrides(&mut overrides, &policy, task, &lowered);
        }
        overrides
    }

    pub(crate) fn source_policy_diagnostics_for_task(task: &str) -> Vec<String> {
        let lowered = task.to_ascii_lowercase();
        let mut diagnostics = Vec::new();
        for policy in Self::runtime_artifact_policies() {
            Self::append_policy_source_diagnostics(&mut diagnostics, &policy, task, &lowered);
        }
        diagnostics
    }

    pub(crate) fn collection_intent_facets_for_task(
        task: &str,
        item_level: bool,
    ) -> Vec<PolicyIntentFacet> {
        let lowered = task.to_ascii_lowercase();
        let mut facets = Vec::new();
        for policy in Self::runtime_artifact_policies() {
            Self::append_policy_collection_facets(&mut facets, &policy, task, &lowered, item_level);
        }
        facets
    }

    pub(crate) fn collection_title_noise_terms_for_task(task: &str) -> Vec<String> {
        Self::policy_terms_for_task(
            task,
            &[
                "collection_title_noise_terms",
                "collection_block_title_noise_terms",
                "browser_structural_noise_terms",
            ],
        )
    }

    pub(crate) fn filter_navigation_text_terms_for_task(task: &str) -> Vec<String> {
        Self::policy_terms_for_task(
            task,
            &[
                "filter_navigation_text_terms",
                "browser_filter_text_terms",
                "navigation_filter_terms",
            ],
        )
    }

    pub(crate) fn filter_navigation_path_prefixes_for_task(task: &str) -> Vec<String> {
        Self::policy_terms_for_task(
            task,
            &[
                "filter_navigation_path_prefixes",
                "browser_filter_path_prefixes",
                "navigation_filter_path_prefixes",
            ],
        )
    }

    pub(crate) fn navigation_noise_text_terms_for_task(task: &str) -> Vec<String> {
        Self::policy_terms_for_task(
            task,
            &[
                "navigation_noise_text_terms",
                "browser_navigation_noise_text_terms",
                "site_chrome_text_terms",
            ],
        )
    }

    pub(crate) fn non_content_path_prefixes_for_task(task: &str) -> Vec<String> {
        Self::policy_terms_for_task(
            task,
            &[
                "non_content_path_prefixes",
                "browser_non_content_path_prefixes",
                "site_chrome_path_prefixes",
            ],
        )
    }

    pub(crate) fn collection_index_path_terms_for_task(task: &str) -> Vec<String> {
        Self::policy_terms_for_task(
            task,
            &[
                "collection_index_path_terms",
                "browser_collection_index_paths",
                "listing_index_path_terms",
            ],
        )
    }

    pub(crate) fn browser_direct_site_max_pages_for_task(task: &str) -> usize {
        Self::policy_usize_for_task(
            task,
            &[
                "browser_direct_site_max_pages",
                "direct_site_max_pages",
                "browser_collection_max_pages",
            ],
        )
        .unwrap_or(16)
        .clamp(1, 64)
    }

    pub(crate) fn browser_direct_site_budget_secs_for_task(task: &str) -> u64 {
        Self::policy_u64_for_task(
            task,
            &[
                "browser_direct_site_budget_secs",
                "direct_site_budget_secs",
                "browser_collection_budget_secs",
            ],
        )
        .unwrap_or(90)
        .clamp(15, 600)
    }

    pub(crate) fn browser_direct_site_attempt_timeout_secs_for_task(task: &str) -> u64 {
        Self::policy_u64_for_task(
            task,
            &[
                "browser_direct_site_attempt_timeout_secs",
                "direct_site_attempt_timeout_secs",
                "browser_collection_attempt_timeout_secs",
            ],
        )
        .unwrap_or(24)
        .clamp(5, 120)
    }

    pub(crate) fn delegate_fast_path_budget_secs_for_task(task: &str) -> u64 {
        let configured = Self::policy_u64_for_task(
            task,
            &[
                "delegate_fast_path_budget_secs",
                "worker_direct_execution_budget_secs",
                "direct_execution_budget_secs",
            ],
        )
        .unwrap_or(90);
        let direct_site_budget = if Self::browser_site_seed_urls_for_task(task).is_empty() {
            0
        } else {
            Self::browser_direct_site_budget_secs_for_task(task).saturating_add(15)
        };
        configured.max(direct_site_budget).clamp(30, 900)
    }

    pub(crate) fn browser_site_seed_urls_for_task(task: &str) -> Vec<String> {
        let intent = Self::build_lookup_intent(task);
        let mut urls = Vec::new();
        let index_paths = Self::browser_site_seed_index_paths_for_task(task);
        for hint in intent.site_hints {
            let Some(host) = Self::site_hint_host(&hint) else {
                continue;
            };
            let host = host.trim().trim_start_matches("www.");
            if host.is_empty() {
                continue;
            }
            if matches!(policy_for_host(host).mode, SiteFetchMode::StaticOnly) {
                continue;
            }
            let mobile_host = format!("m.{host}");
            for path in &index_paths {
                Self::push_unique(&mut urls, format!("https://{mobile_host}/{path}/"));
                Self::push_unique(&mut urls, format!("https://{host}/{path}/"));
                Self::push_unique(&mut urls, format!("https://www.{host}/{path}/"));
            }
            Self::push_unique(&mut urls, format!("https://{mobile_host}/"));
            Self::push_unique(&mut urls, format!("https://{host}/"));
            Self::push_unique(&mut urls, format!("https://www.{host}/"));
        }
        urls
    }

    pub(crate) fn browser_site_seed_index_paths_for_task(task: &str) -> Vec<String> {
        let mut paths = Self::collection_index_path_terms_for_task(task)
            .into_iter()
            .filter_map(|term| {
                let segment = term.trim().trim_matches('/');
                if segment.is_empty()
                    || segment.len() > 48
                    || segment
                        .chars()
                        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
                {
                    return None;
                }
                Some(segment.to_ascii_lowercase())
            })
            .fold(Vec::new(), |mut paths, path| {
                Self::push_unique(&mut paths, path);
                paths
            });
        paths.sort_by(|left, right| {
            Self::browser_site_seed_path_score(task, right)
                .cmp(&Self::browser_site_seed_path_score(task, left))
                .then_with(|| left.cmp(right))
        });
        paths
    }

    fn browser_site_seed_path_score(task: &str, path: &str) -> i32 {
        let lowered_task = task.to_ascii_lowercase();
        let lowered_path = path.to_ascii_lowercase();
        let mut score = 0;
        if lowered_path.len() >= 3 && lowered_task.contains(&lowered_path) {
            score += 30;
        }
        for facet in Self::collection_intent_facets_for_task(task, true) {
            if !Self::text_contains_any_owned(task, &facet.requested_by) {
                continue;
            }
            if facet.evidence_terms.iter().any(|term| {
                let term = term.trim().trim_matches('/').to_ascii_lowercase();
                term.len() >= 3 && (term == lowered_path || lowered_path.contains(&term))
            }) {
                score += 20;
            }
        }
        score
    }

    fn text_contains_any_owned(text: &str, terms: &[String]) -> bool {
        terms.iter().any(|term| Self::text_contains(text, term))
    }

    fn text_contains(text: &str, term: &str) -> bool {
        let term = term.trim();
        if term.is_empty() {
            return false;
        }
        if term.is_ascii() {
            text.to_ascii_lowercase()
                .contains(&term.to_ascii_lowercase())
        } else {
            text.contains(term)
        }
    }

    pub(crate) fn task_requests_china_welfare_lottery_records(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        Self::task_requests_data_or_records(task)
            && (task.contains("中国福利彩票")
                || task.contains("福利彩票")
                || task.contains("福彩")
                || lowered.contains("china welfare lottery"))
    }

    pub(crate) fn task_requests_data_or_records(task: &str) -> bool {
        if Self::task_requests_collection_or_ranking(task)
            && !Self::task_contains_hard_record_signal(task)
        {
            return false;
        }

        let lowered = task.to_ascii_lowercase();
        (lowered.contains("data") && !lowered.contains("metadata"))
            || lowered.contains("record")
            || lowered.contains("records")
            || lowered.contains("number")
            || lowered.contains("numbers")
            || (lowered.contains("result") && Self::task_contains_hard_record_signal(task))
            || (lowered.contains("results") && Self::task_contains_hard_record_signal(task))
            || task.contains("数据")
            || task.contains("记录")
            || (task.contains("结果") && Self::task_contains_hard_record_signal(task))
            || task.contains("号码")
            || task.contains("开奖")
            || task.contains("每期")
    }

    pub(crate) fn task_requests_collection_or_ranking(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        let has_requested_prefix_count =
            Regex::new(r"前\s*(?:\d{1,2}|[一二三四五六七八九十百]{1,4})")
                .expect("valid requested collection prefix regex")
                .is_match(task);
        lowered.contains("top ")
            || lowered.contains("top-")
            || lowered.contains("top10")
            || lowered.contains("top 10")
            || lowered.contains("rank")
            || lowered.contains("ranking")
            || lowered.contains("leaderboard")
            || lowered.contains("recommend")
            || lowered.contains("recommended")
            || lowered.contains("catalog")
            || lowered.contains("directory")
            || lowered.contains("collection")
            || lowered.contains("curated")
            || has_requested_prefix_count
            || task.contains("前10")
            || task.contains("前十")
            || task.contains("排名")
            || task.contains("排行")
            || task.contains("排行榜")
            || task.contains("榜单")
            || task.contains("推荐")
            || task.contains("书单")
            || task.contains("目录")
            || task.contains("清单")
            || (task.contains("列表") && !Self::task_contains_hard_record_signal(task))
    }

    fn task_contains_hard_record_signal(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        (lowered.contains("data") && !lowered.contains("metadata"))
            || lowered.contains("record")
            || lowered.contains("records")
            || lowered.contains("dataset")
            || lowered.contains("table")
            || lowered.contains("number")
            || lowered.contains("numbers")
            || lowered.contains("lottery")
            || lowered.contains("draw")
            || (task.contains("数据") && !task.contains("元数据"))
            || task.contains("记录")
            || task.contains("号码")
            || task.contains("开奖")
            || task.contains("彩票")
            || task.contains("期号")
            || task.contains("每期")
    }

    pub(crate) fn task_requests_record_collection(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        lowered.contains("past ")
            || lowered.contains("last ")
            || lowered.contains("history")
            || lowered.contains("historical")
            || lowered.contains("records")
            || (lowered.contains("list") && Self::task_contains_hard_record_signal(task))
            || task.contains("每期")
            || task.contains("往期")
            || task.contains("历史")
            || task.contains("最近")
            || task.contains("近")
            || task.contains("个月")
            || (task.contains("列表") && Self::task_contains_hard_record_signal(task))
    }

    pub(crate) fn task_requests_recent_material(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        lowered.contains("latest")
            || lowered.contains("recent")
            || lowered.contains("current")
            || lowered.contains("past ")
            || lowered.contains("last ")
            || task.contains("最近")
            || task.contains("最新")
            || task.contains("近")
            || task.contains("个月")
            || task.contains("本月")
            || task.contains("今年")
    }

    pub(crate) fn cjk_lookup_phrases(task: &str) -> Vec<String> {
        let mut phrases = Vec::new();
        for keyword in [
            "彩票", "开奖", "号码", "数据", "记录", "结果", "日期", "期号", "论文", "研究", "治疗",
            "仓库", "项目", "源码", "视频", "字幕", "价格", "走势", "公告", "小说", "科幻", "星际",
            "玄幻", "奇幻", "仙侠", "太空", "宇宙", "起点", "免费", "下载", "开放", "排名", "排行",
            "榜单", "推荐", "作者", "简介",
        ] {
            for phrase in Self::cjk_phrases_around_keyword(task, keyword, 6, 4) {
                if !Self::is_cjk_lookup_phrase_noise(&phrase) {
                    Self::push_unique(&mut phrases, phrase);
                }
                if phrases.len() >= 16 {
                    return phrases;
                }
            }
        }
        phrases
    }

    fn task_tokens(task: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        for token in task
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        ',' | '.'
                            | ';'
                            | ':'
                            | '!'
                            | '?'
                            | '"'
                            | '\''
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
                            | '！'
                            | '？'
                            | '（'
                            | '）'
                            | '【'
                            | '】'
                    )
            })
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .filter(|token| token.chars().count() > 1)
        {
            if Self::contains_cjk(token) && token.chars().count() > 10 {
                let extracted = Self::cjk_lookup_phrases(token);
                if extracted.is_empty() {
                    Self::push_unique(&mut tokens, token.to_string());
                } else {
                    for phrase in extracted {
                        Self::push_unique(&mut tokens, phrase);
                    }
                }
            } else {
                Self::push_unique(&mut tokens, token.to_string());
            }
            if tokens.len() >= 18 {
                break;
            }
        }
        tokens
    }

    pub(crate) fn is_data_lookup_subject_term(term: &str) -> bool {
        let lowered = term.to_ascii_lowercase();
        if Self::is_data_lookup_generic_term(term) || Self::is_lookup_noise_term(term) {
            return false;
        }
        term.contains("中国")
            || term.contains("福利彩票")
            || term.contains("福彩")
            || term.contains("双色球")
            || term.contains("快乐")
            || term.contains("七乐彩")
            || lowered.contains("china")
            || lowered.contains("welfare")
            || lowered.contains("lottery")
            || lowered.contains("lancet")
            || lowered.contains("github")
            || lowered.contains("pubmed")
            || lowered.contains("youtube")
            || lowered.contains("blockbeats")
    }

    fn is_data_lookup_strong_term(term: &str) -> bool {
        let lowered = term.to_ascii_lowercase();
        !Self::is_data_lookup_generic_stopword(&lowered)
            && (lowered.contains("data")
                || lowered.contains("record")
                || lowered.contains("number")
                || lowered.contains("result")
                || term.contains("数据")
                || term.contains("记录")
                || term.contains("结果")
                || term.contains("号码")
                || term.contains("开奖")
                || term.contains("期号")
                || term.contains("日期")
                || term.contains("彩票")
                || term.contains("福彩")
                || term.contains("双色球")
                || term.contains("快乐"))
    }

    fn is_data_lookup_instruction_term(term: &str) -> bool {
        let lowered = term.to_ascii_lowercase();
        Self::is_data_lookup_generic_stopword(&lowered)
            || lowered.contains("knowledge")
            || lowered.contains("predict")
            || lowered.contains("format")
            || term.contains("知识库")
            || term.contains("预测")
            || term.contains("结构化")
            || term.contains("后续")
            || term.contains("整理")
            || term.contains("保存")
            || term.contains("存入")
    }

    fn is_data_lookup_generic_stopword(lowered: &str) -> bool {
        matches!(
            lowered,
            "search"
                | "find"
                | "lookup"
                | "check"
                | "for"
                | "the"
                | "of"
                | "and"
                | "or"
                | "to"
                | "from"
                | "past"
                | "last"
                | "latest"
                | "recent"
                | "month"
                | "months"
                | "official"
                | "source"
                | "sources"
        )
    }

    pub(crate) fn is_data_lookup_generic_term(term: &str) -> bool {
        let lowered = term.to_ascii_lowercase();
        matches!(
            lowered.as_str(),
            "data"
                | "record"
                | "records"
                | "result"
                | "results"
                | "number"
                | "numbers"
                | "list"
                | "winning"
                | "draw"
                | "draws"
        ) || matches!(
            term,
            "数据" | "记录" | "结果" | "列表" | "号码" | "开奖" | "日期" | "期号" | "每期"
        )
    }

    fn is_lookup_noise_term(token: &str) -> bool {
        let lowered = token.trim().to_ascii_lowercase();
        if Self::looks_like_internal_query_identifier(&lowered) {
            return true;
        }
        matches!(
            lowered.as_str(),
            "search"
                | "lookup"
                | "find"
                | "latest"
                | "recent"
                | "newest"
                | "research"
                | "related"
                | "browse"
                | "fetch"
                | "recall"
                | "access"
                | "read"
                | "extract"
                | "project"
                | "projects"
                | "high-starred"
                | "starred"
                | "real"
                | "url"
                | "urls"
                | "please"
                | "also"
                | "specify"
                | "used"
                | "use"
                | "using"
                | "process"
                | "import"
                | "ingest"
                | "ingestion"
                | "tool"
                | "tools"
                | "browser"
                | "web_search"
                | "paper"
                | "papers"
                | "write"
                | "writing"
                | "draft"
                | "drafts"
                | "revise"
                | "revision"
                | "plan"
                | "compose"
                | "composer"
                | "architect"
                | "audit"
                | "auditor"
                | "export"
                | "study"
                | "studies"
                | "article"
                | "articles"
                | "titles"
                | "title"
                | "summary"
                | "summaries"
                | "source"
                | "sources"
                | "link"
                | "links"
                | "save"
                | "saved"
                | "knowledge"
                | "base"
                | "report"
                | "results"
                | "final"
                | "user"
                | "chinese"
                | "requested"
                | "specific"
                | "latest,"
        ) || matches!(
            token.trim(),
            "搜索"
                | "查找"
                | "检索"
                | "最新"
                | "最近"
                | "论文"
                | "研究"
                | "文章"
                | "标题"
                | "摘要"
                | "来源"
                | "链接"
                | "网址"
                | "保存"
                | "知识库"
                | "结果"
                | "汇总"
                | "中文"
                | "用户"
        )
    }

    fn looks_like_internal_query_identifier(token: &str) -> bool {
        let lowered = token.trim().to_ascii_lowercase();
        lowered.contains('_')
            && lowered
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    }

    fn contains_cjk(input: &str) -> bool {
        input
            .chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
    }

    fn cjk_phrases_around_keyword(
        input: &str,
        keyword: &str,
        before: usize,
        after: usize,
    ) -> Vec<String> {
        let chars = input.chars().collect::<Vec<_>>();
        let keyword_chars = keyword.chars().collect::<Vec<_>>();
        if keyword_chars.is_empty() || chars.len() < keyword_chars.len() {
            return Vec::new();
        }

        let mut phrases = Vec::new();
        for index in 0..=chars.len() - keyword_chars.len() {
            if chars[index..index + keyword_chars.len()] != keyword_chars[..] {
                continue;
            }
            let mut start = index.saturating_sub(before);
            while start < index && !Self::is_cjk_context_char(chars[start]) {
                start += 1;
            }
            let mut end = (index + keyword_chars.len() + after).min(chars.len());
            while end > index + keyword_chars.len() && !Self::is_cjk_context_char(chars[end - 1]) {
                end -= 1;
            }
            let phrase = chars[start..end].iter().collect::<String>();
            let phrase = Self::trim_cjk_lookup_phrase(&phrase);
            if phrase.chars().count() >= keyword_chars.len() {
                Self::push_unique(&mut phrases, phrase);
            }
        }
        phrases
    }

    fn is_cjk_context_char(ch: char) -> bool {
        ('\u{4e00}'..='\u{9fff}').contains(&ch)
            || ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '/')
    }

    fn trim_cjk_lookup_phrase(input: &str) -> String {
        let mut phrase = input
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    '查' | '找'
                        | '搜'
                        | '索'
                        | '请'
                        | '将'
                        | '把'
                        | '月'
                        | '个'
                        | '过'
                        | '去'
                        | '所'
                        | '有'
                        | '的'
                        | '和'
                        | '或'
                        | '如'
                        | '等'
                        | '内'
                        | '近'
                        | '最'
                        | '新'
                )
            })
            .to_string();
        let workflow_separators = [
            "之后", "然后", "接着", "并且", "以及", "同时", "把", "将", "可以", "需要", "要求",
            "尝试", "进行", "根据", "保存", "放到", "存到",
        ];
        let mut changed = true;
        while changed {
            changed = false;
            for separator in workflow_separators {
                let Some(index) = phrase.find(separator) else {
                    continue;
                };
                let before = phrase[..index].trim();
                let after = phrase[index + separator.len()..].trim();
                phrase = if before.chars().count() >= 2 {
                    before.to_string()
                } else {
                    after.to_string()
                };
                changed = true;
                break;
            }
        }
        for prefix in [
            "载的",
            "下载的",
            "可下载的",
            "可下载",
            "取到的",
            "读取到的",
            "可以读取到的",
            "到的",
        ] {
            while phrase.starts_with(prefix) {
                phrase = phrase[prefix.len()..].trim().to_string();
            }
        }
        phrase
            .trim_matches(|ch: char| matches!(ch, '的' | '了' | '和' | '与' | '及'))
            .to_string()
    }

    fn is_cjk_lookup_phrase_noise(term: &str) -> bool {
        term.chars().count() < 2
            || matches!(
                term,
                "数据" | "记录" | "结果" | "日期" | "号码" | "开奖" | "公告" | "项目"
            )
    }

    fn apply_runtime_artifact_policies(intent: &mut LookupIntent, original: &str, lowered: &str) {
        let policies = Self::runtime_artifact_policies();
        let bundle = RuntimePolicyResolver::resolve(
            TaskPolicyInput::new(original).with_phase(PolicyPhase::TaskEntry),
            &policies,
        );
        for hint in bundle.artifact_hints {
            Self::push_unique(&mut intent.artifact_hints, hint);
        }
        for hint in bundle.evidence_hints {
            Self::push_unique(&mut intent.evidence_hints, hint);
        }
        for hint in bundle.freshness_hints {
            Self::push_unique(&mut intent.freshness_hints, hint);
        }
        for hint in bundle.direct_record_hints {
            Self::push_unique(&mut intent.direct_record_hints, hint);
        }
        for hint in bundle.site_hints {
            let hint = hint.trim();
            if hint.is_empty() {
                continue;
            }
            if hint.to_ascii_lowercase().starts_with("site:") {
                Self::push_unique(&mut intent.site_hints, hint.to_string());
            } else {
                Self::push_unique(&mut intent.site_hints, format!("site:{hint}"));
            }
        }

        for policy in policies {
            Self::apply_artifact_policy_value(intent, &policy, original, lowered);
        }
    }

    fn apply_artifact_policy_value(
        intent: &mut LookupIntent,
        policy: &Value,
        original: &str,
        lowered: &str,
    ) {
        let Some(handles) = policy.get("handles").and_then(|value| value.as_array()) else {
            return;
        };
        for handle in handles {
            if !Self::policy_handle_matches_task(handle, original, lowered) {
                continue;
            }
            Self::push_policy_string_field(handle, "artifact", &mut intent.artifact_hints);
            Self::push_policy_string_array(handle, "intents", &mut intent.artifact_hints);
            Self::push_policy_string_array(handle, "artifact_hints", &mut intent.artifact_hints);
            Self::push_policy_string_array(handle, "evidence_hints", &mut intent.evidence_hints);
            Self::push_policy_string_array(handle, "freshness_hints", &mut intent.freshness_hints);
            Self::push_policy_string_array(
                handle,
                "direct_record_hints",
                &mut intent.direct_record_hints,
            );
            Self::push_policy_sites(handle, "sites", &mut intent.site_hints);
            Self::push_policy_sites(handle, "site_hints", &mut intent.site_hints);
            Self::push_policy_sites(handle, "domains", &mut intent.site_hints);
            Self::push_policy_sites(handle, "preferred_hosts", &mut intent.site_hints);
        }
    }

    fn append_policy_urls_for_task(
        urls: &mut Vec<String>,
        policy: &Value,
        original: &str,
        lowered: &str,
    ) {
        let Some(handles) = policy.get("handles").and_then(|value| value.as_array()) else {
            return;
        };
        for handle in handles {
            if !Self::policy_handle_matches_task(handle, original, lowered) {
                continue;
            }
            Self::push_policy_string_array(handle, "urls", urls);
            Self::push_policy_string_array(handle, "seed_urls", urls);
            Self::push_policy_string_array(handle, "record_urls", urls);
            Self::push_policy_string_array(handle, "public_urls", urls);
        }
    }

    fn append_policy_sources_for_task(
        sources: &mut Vec<String>,
        policy: &Value,
        original: &str,
        lowered: &str,
    ) {
        let Some(handles) = policy.get("handles").and_then(|value| value.as_array()) else {
            return;
        };
        for handle in handles {
            if !Self::policy_handle_matches_task(handle, original, lowered) {
                continue;
            }
            Self::push_policy_string_array(handle, "sources", sources);
            Self::push_policy_string_array(handle, "source_adapters", sources);
            Self::push_policy_string_array(handle, "source_hints", sources);
        }
    }

    fn append_policy_source_adapter_overrides(
        overrides: &mut Vec<RuntimeSourceAdapterOverride>,
        policy: &Value,
        original: &str,
        lowered: &str,
    ) {
        let Some(handles) = policy.get("handles").and_then(|value| value.as_array()) else {
            return;
        };
        for handle in handles {
            if !Self::policy_handle_matches_task(handle, original, lowered) {
                continue;
            }
            for value in Self::source_adapter_values(handle) {
                if let Some(override_spec) = Self::parse_source_adapter_override(value) {
                    Self::push_source_adapter_override(overrides, override_spec);
                }
            }
        }
    }

    fn append_policy_source_diagnostics(
        diagnostics: &mut Vec<String>,
        policy: &Value,
        original: &str,
        lowered: &str,
    ) {
        let Some(handles) = policy.get("handles").and_then(|value| value.as_array()) else {
            return;
        };
        for handle in handles {
            if !Self::policy_handle_matches_task(handle, original, lowered) {
                continue;
            }
            for value in Self::source_adapter_values(handle) {
                if let Some(message) = Self::source_adapter_policy_error(value) {
                    Self::push_unique(diagnostics, message);
                }
            }
        }
    }

    fn append_policy_collection_facets(
        facets: &mut Vec<PolicyIntentFacet>,
        policy: &Value,
        original: &str,
        lowered: &str,
        item_level: bool,
    ) {
        let Some(handles) = policy.get("handles").and_then(|value| value.as_array()) else {
            return;
        };
        let keys: &[&str] = if item_level {
            &["collection_item_facets", "item_facets"]
        } else {
            &["collection_facets", "ranking_facets"]
        };
        for handle in handles {
            if !Self::policy_handle_matches_task(handle, original, lowered) {
                continue;
            }
            for key in keys {
                let Some(values) = handle.get(*key) else {
                    continue;
                };
                match values {
                    Value::Array(items) => {
                        for item in items {
                            if let Some(facet) = Self::parse_policy_intent_facet(item) {
                                Self::push_policy_intent_facet(facets, facet);
                            }
                        }
                    }
                    Value::Object(_) => {
                        if let Some(facet) = Self::parse_policy_intent_facet(values) {
                            Self::push_policy_intent_facet(facets, facet);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn policy_terms_for_task(task: &str, keys: &[&str]) -> Vec<String> {
        let lowered = task.to_ascii_lowercase();
        let mut terms = Vec::new();
        for policy in Self::runtime_artifact_policies() {
            Self::append_policy_terms_for_task(&mut terms, &policy, task, &lowered, keys);
        }
        terms
    }

    fn policy_usize_for_task(task: &str, keys: &[&str]) -> Option<usize> {
        Self::policy_u64_for_task(task, keys).and_then(|value| usize::try_from(value).ok())
    }

    fn policy_u64_for_task(task: &str, keys: &[&str]) -> Option<u64> {
        let lowered = task.to_ascii_lowercase();
        for policy in Self::runtime_artifact_policies() {
            if let Some(value) = Self::policy_u64_value_for_task(&policy, task, &lowered, keys) {
                return Some(value);
            }
        }
        None
    }

    pub(crate) fn policy_u64_value_for_task(
        policy: &Value,
        original: &str,
        lowered: &str,
        keys: &[&str],
    ) -> Option<u64> {
        let handles = policy.get("handles").and_then(|value| value.as_array())?;
        for handle in handles {
            if !Self::policy_handle_matches_task(handle, original, lowered) {
                continue;
            }
            for key in keys {
                if let Some(value) = handle.get(*key).and_then(Value::as_u64) {
                    return Some(value);
                }
            }
        }
        None
    }

    fn append_policy_terms_for_task(
        terms: &mut Vec<String>,
        policy: &Value,
        original: &str,
        lowered: &str,
        keys: &[&str],
    ) {
        let Some(handles) = policy.get("handles").and_then(|value| value.as_array()) else {
            return;
        };
        for handle in handles {
            if !Self::policy_handle_matches_task(handle, original, lowered) {
                continue;
            }
            for key in keys {
                Self::push_policy_string_array(handle, key, terms);
            }
        }
    }

    fn parse_policy_intent_facet(value: &Value) -> Option<PolicyIntentFacet> {
        let map = value.as_object()?;
        let name = map
            .get("name")
            .or_else(|| map.get("id"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())?
            .to_string();
        let requested_by = Self::string_values_from_object(map, "requested_by")
            .or_else(|| Self::string_values_from_object(map, "triggers"))
            .unwrap_or_default();
        let evidence_terms = Self::string_values_from_object(map, "evidence_terms")
            .or_else(|| Self::string_values_from_object(map, "matches"))
            .unwrap_or_default();
        let conflicting_terms = Self::string_values_from_object(map, "conflicting_terms")
            .or_else(|| Self::string_values_from_object(map, "conflicts"))
            .unwrap_or_default();
        let requires_evidence = map
            .get("requires_evidence")
            .or_else(|| map.get("required"))
            .or_else(|| map.get("require_item_evidence"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if requested_by.is_empty() || evidence_terms.is_empty() {
            return None;
        }
        Some(PolicyIntentFacet {
            name,
            requested_by,
            evidence_terms,
            conflicting_terms,
            requires_evidence,
        })
    }

    fn push_policy_intent_facet(facets: &mut Vec<PolicyIntentFacet>, facet: PolicyIntentFacet) {
        if facets.iter().any(|existing| existing.name == facet.name) {
            return;
        }
        facets.push(facet);
    }

    fn source_adapter_values(handle: &Value) -> Vec<&Value> {
        let mut values = Vec::new();
        for key in ["source_adapters", "sources", "source_hints"] {
            match handle.get(key) {
                Some(Value::Array(items)) => values.extend(items.iter()),
                Some(value @ Value::Object(_)) | Some(value @ Value::String(_)) => {
                    values.push(value)
                }
                _ => {}
            }
        }
        values
    }

    fn parse_source_adapter_override(value: &Value) -> Option<RuntimeSourceAdapterOverride> {
        match value {
            Value::String(name) => {
                let name = name.trim();
                (!name.is_empty()).then(|| RuntimeSourceAdapterOverride {
                    name: name.to_string(),
                    ..RuntimeSourceAdapterOverride::default()
                })
            }
            Value::Object(map) => {
                let name = map
                    .get("name")
                    .or_else(|| map.get("id"))
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                Some(RuntimeSourceAdapterOverride {
                    name: name.to_string(),
                    capability: map
                        .get("capability")
                        .and_then(|value| value.as_str())
                        .and_then(parse_source_capability),
                    requires_browser: map
                        .get("requires_browser")
                        .and_then(|value| value.as_bool()),
                    requires_auth: map.get("requires_auth").and_then(|value| value.as_bool()),
                    challenge_prone: map.get("challenge_prone").and_then(|value| value.as_bool()),
                    domains: Self::string_values_from_object(map, "domains"),
                    fallback_sources: Self::string_values_from_object(map, "fallback_sources"),
                    weight: map
                        .get("weight")
                        .and_then(|value| value.as_u64())
                        .and_then(|value| u32::try_from(value).ok()),
                })
            }
            _ => None,
        }
    }

    fn source_adapter_policy_error(value: &Value) -> Option<String> {
        match value {
            Value::Object(map) => {
                let name = map
                    .get("name")
                    .or_else(|| map.get("id"))
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .trim();
                if name.is_empty() {
                    return Some("source_adapter missing required name/id".to_string());
                }
                if let Some(capability) = map.get("capability").and_then(|value| value.as_str()) {
                    if parse_source_capability(capability).is_none() {
                        return Some(format!(
                            "source_adapter '{name}' has unsupported capability '{capability}'"
                        ));
                    }
                }
                if let Some(weight) = map.get("weight").and_then(|value| value.as_u64()) {
                    if u32::try_from(weight).is_err() {
                        return Some(format!(
                            "source_adapter '{name}' weight is outside u32 range"
                        ));
                    }
                }
                None
            }
            Value::String(_) => None,
            _ => Some("source_adapter must be a string or object".to_string()),
        }
    }

    fn push_source_adapter_override(
        overrides: &mut Vec<RuntimeSourceAdapterOverride>,
        override_spec: RuntimeSourceAdapterOverride,
    ) {
        if let Some(existing) = overrides
            .iter_mut()
            .find(|existing| existing.name.eq_ignore_ascii_case(&override_spec.name))
        {
            existing.merge(override_spec);
        } else {
            overrides.push(override_spec);
        }
    }

    fn string_values_from_object(
        map: &serde_json::Map<String, Value>,
        key: &str,
    ) -> Option<Vec<String>> {
        let value = map.get(key)?;
        let mut values = Vec::new();
        match value {
            Value::String(value) => Self::push_unique(&mut values, value),
            Value::Array(items) => {
                for item in items {
                    if let Some(value) = item.as_str() {
                        Self::push_unique(&mut values, value);
                    }
                }
            }
            _ => {}
        }
        Some(values)
    }

    fn policy_handle_matches_task(handle: &Value, original: &str, _lowered: &str) -> bool {
        policy_handle_matches_task(handle, original)
    }

    fn push_policy_string_field(handle: &Value, key: &str, target: &mut Vec<String>) {
        if let Some(value) = handle.get(key).and_then(|value| value.as_str()) {
            Self::push_unique(target, value.to_string());
        }
    }

    fn push_policy_string_array(handle: &Value, key: &str, target: &mut Vec<String>) {
        let Some(values) = handle.get(key) else {
            return;
        };
        match values {
            Value::String(value) => Self::push_unique(target, value.to_string()),
            Value::Array(values) => {
                for value in values {
                    if let Some(value) = value.as_str() {
                        Self::push_unique(target, value.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    fn push_policy_sites(handle: &Value, key: &str, target: &mut Vec<String>) {
        let mut values = Vec::new();
        Self::push_policy_string_array(handle, key, &mut values);
        for value in values {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            if value.to_ascii_lowercase().starts_with("site:") {
                Self::push_unique(target, value.to_string());
            } else {
                Self::push_unique(target, format!("site:{value}"));
            }
        }
    }

    fn runtime_artifact_policies() -> Vec<Value> {
        let mut policies = Vec::new();
        for dir in Self::runtime_agent_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let root = entry.path();
                let standalone_policy =
                    Self::read_artifact_policy_yaml(&root.join("artifact_policy.yaml"));
                if let Some(policy) = standalone_policy {
                    policies.push(policy);
                } else if let Some(policy) =
                    Self::read_agent_artifact_policy(&root.join("AGENT.md"))
                {
                    policies.push(policy);
                }
            }
        }
        for dir in Self::runtime_skill_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let root = entry.path();
                let standalone_policy =
                    Self::read_artifact_policy_yaml(&root.join("artifact_policy.yaml"));
                if let Some(policy) = standalone_policy {
                    policies.push(policy);
                } else if let Some(policy) =
                    Self::read_agent_artifact_policy(&root.join("SKILL.md"))
                {
                    policies.push(policy);
                }
            }
        }
        policies
    }

    fn runtime_agent_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Ok(path) = std::env::var("BENSHU_AGENT_PATH") {
            let path = PathBuf::from(path);
            Self::push_unique_path(&mut dirs, path);
        }
        if let Ok(path) = std::env::var("BENSHU_DATA_DIR") {
            Self::push_unique_path(&mut dirs, PathBuf::from(path).join("agents"));
        }
        if let Ok(cwd) = std::env::current_dir() {
            Self::push_unique_path(&mut dirs, cwd.join("data").join("agents"));
            Self::push_unique_path(&mut dirs, cwd.join("..").join("data").join("agents"));
        }
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for ancestor in manifest_dir.ancestors() {
            Self::push_unique_path(&mut dirs, ancestor.join("data").join("agents"));
        }
        dirs
    }

    fn runtime_skill_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Ok(path) = std::env::var("BENSHU_SKILL_PATH") {
            Self::push_unique_path(&mut dirs, PathBuf::from(path));
        }
        if let Ok(path) = std::env::var("BENSHU_DATA_DIR") {
            Self::push_unique_path(&mut dirs, PathBuf::from(path).join("skills"));
        }
        if let Ok(cwd) = std::env::current_dir() {
            Self::push_unique_path(&mut dirs, cwd.join("data").join("skills"));
            Self::push_unique_path(&mut dirs, cwd.join("..").join("data").join("skills"));
        }
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for ancestor in manifest_dir.ancestors() {
            Self::push_unique_path(&mut dirs, ancestor.join("data").join("skills"));
        }
        dirs
    }

    fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
        if paths.iter().any(|existing| existing == &path) {
            return;
        }
        paths.push(path);
    }

    fn read_agent_artifact_policy(path: &Path) -> Option<Value> {
        let content = std::fs::read_to_string(path).ok()?;
        let yaml = split_frontmatter_yaml(&content)?;
        #[derive(Deserialize)]
        struct AgentPolicyFrontmatter {
            artifact_policy: Option<Value>,
        }
        serde_yaml_ng::from_str::<AgentPolicyFrontmatter>(yaml)
            .ok()?
            .artifact_policy
            .and_then(Self::normalize_artifact_policy_value)
    }

    fn read_artifact_policy_yaml(path: &Path) -> Option<Value> {
        let content = std::fs::read_to_string(path).ok()?;
        let value = serde_yaml_ng::from_str::<Value>(&content).ok()?;
        Self::normalize_artifact_policy_value(value)
    }

    fn normalize_artifact_policy_value(value: Value) -> Option<Value> {
        normalize_artifact_policy_value(value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RuntimeSourceAdapterOverride {
    pub(crate) name: String,
    pub(crate) capability: Option<SourceCapability>,
    pub(crate) requires_browser: Option<bool>,
    pub(crate) requires_auth: Option<bool>,
    pub(crate) challenge_prone: Option<bool>,
    pub(crate) domains: Option<Vec<String>>,
    pub(crate) fallback_sources: Option<Vec<String>>,
    pub(crate) weight: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PolicyIntentFacet {
    pub(crate) name: String,
    pub(crate) requested_by: Vec<String>,
    pub(crate) evidence_terms: Vec<String>,
    pub(crate) conflicting_terms: Vec<String>,
    pub(crate) requires_evidence: bool,
}

impl RuntimeSourceAdapterOverride {
    fn merge(&mut self, other: RuntimeSourceAdapterOverride) {
        self.capability = other.capability.or(self.capability.take());
        self.requires_browser = other.requires_browser.or(self.requires_browser);
        self.requires_auth = other.requires_auth.or(self.requires_auth);
        self.challenge_prone = other.challenge_prone.or(self.challenge_prone);
        self.domains = merge_optional_vec(self.domains.take(), other.domains);
        self.fallback_sources =
            merge_optional_vec(self.fallback_sources.take(), other.fallback_sources);
        self.weight = other.weight.or(self.weight);
    }
}

fn merge_optional_vec(
    left: Option<Vec<String>>,
    right: Option<Vec<String>>,
) -> Option<Vec<String>> {
    match (left, right) {
        (None, None) => None,
        (Some(values), None) | (None, Some(values)) => Some(values),
        (Some(mut left), Some(right)) => {
            for value in right {
                SearchPolicy::push_unique(&mut left, value);
            }
            Some(left)
        }
    }
}

fn parse_source_capability(value: &str) -> Option<SourceCapability> {
    match value.trim().to_ascii_lowercase().as_str() {
        "public" => Some(SourceCapability::Public),
        "api" => Some(SourceCapability::Public),
        "rss" | "feed" => Some(SourceCapability::Rss),
        "search_engine" | "search-engine" | "searchengine" => Some(SourceCapability::SearchEngine),
        "browser" => Some(SourceCapability::Browser),
        "cookie" => Some(SourceCapability::Cookie),
        "header" => Some(SourceCapability::Header),
        "intercept" => Some(SourceCapability::Intercept),
        "ui" => Some(SourceCapability::Ui),
        _ => None,
    }
}

fn split_frontmatter_yaml(content: &str) -> Option<&str> {
    let rest = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

struct HintRule {
    needle: &'static str,
    hint: &'static str,
}

const SITE_HINT_RULES: &[HintRule] = &[
    HintRule {
        needle: "github",
        hint: "site:github.com",
    },
    HintRule {
        needle: "gitlab",
        hint: "site:gitlab.com",
    },
    HintRule {
        needle: "youtube",
        hint: "site:youtube.com",
    },
    HintRule {
        needle: "youtu.be",
        hint: "site:youtube.com",
    },
    HintRule {
        needle: "bilibili",
        hint: "site:bilibili.com",
    },
    HintRule {
        needle: "qidian",
        hint: "site:qidian.com",
    },
    HintRule {
        needle: "起点",
        hint: "site:qidian.com",
    },
    HintRule {
        needle: "起点中文网",
        hint: "site:qidian.com",
    },
    HintRule {
        needle: "thelancet",
        hint: "site:thelancet.com",
    },
    HintRule {
        needle: "lancet",
        hint: "site:thelancet.com",
    },
    HintRule {
        needle: "柳叶刀",
        hint: "site:thelancet.com",
    },
    HintRule {
        needle: "pubmed",
        hint: "site:pubmed.ncbi.nlm.nih.gov",
    },
    HintRule {
        needle: "pmc",
        hint: "site:pmc.ncbi.nlm.nih.gov",
    },
    HintRule {
        needle: "doi",
        hint: "site:doi.org",
    },
    HintRule {
        needle: "crossref",
        hint: "site:crossref.org",
    },
];

const ARTIFACT_HINT_RULES: &[HintRule] = &[
    HintRule {
        needle: "doi",
        hint: "doi",
    },
    HintRule {
        needle: "pubmed",
        hint: "pubmed",
    },
    HintRule {
        needle: "pmc",
        hint: "pmc",
    },
    HintRule {
        needle: "crossref",
        hint: "crossref",
    },
    HintRule {
        needle: "readme",
        hint: "readme",
    },
    HintRule {
        needle: "repository",
        hint: "repository",
    },
    HintRule {
        needle: "repo",
        hint: "repo",
    },
    HintRule {
        needle: "issue",
        hint: "issue",
    },
    HintRule {
        needle: "pull request",
        hint: "pull request",
    },
    HintRule {
        needle: "commit",
        hint: "commit",
    },
    HintRule {
        needle: "video",
        hint: "video",
    },
    HintRule {
        needle: "transcript",
        hint: "transcript",
    },
    HintRule {
        needle: "caption",
        hint: "caption",
    },
    HintRule {
        needle: "subtitle",
        hint: "subtitle",
    },
    HintRule {
        needle: "full text",
        hint: "full text",
    },
    HintRule {
        needle: "abstract",
        hint: "abstract",
    },
    HintRule {
        needle: "paper",
        hint: "paper",
    },
    HintRule {
        needle: "study",
        hint: "study",
    },
    HintRule {
        needle: "article",
        hint: "article",
    },
    HintRule {
        needle: "journal",
        hint: "journal",
    },
    HintRule {
        needle: "论文",
        hint: "paper",
    },
    HintRule {
        needle: "研究",
        hint: "study",
    },
    HintRule {
        needle: "期刊",
        hint: "journal",
    },
    HintRule {
        needle: "摘要",
        hint: "abstract",
    },
    HintRule {
        needle: "全文",
        hint: "full text",
    },
    HintRule {
        needle: "开放全文",
        hint: "open access",
    },
    HintRule {
        needle: "仓库",
        hint: "repository",
    },
    HintRule {
        needle: "源码",
        hint: "source code",
    },
    HintRule {
        needle: "代码",
        hint: "source code",
    },
    HintRule {
        needle: "提交",
        hint: "commit",
    },
    HintRule {
        needle: "字幕",
        hint: "caption",
    },
    HintRule {
        needle: "视频",
        hint: "video",
    },
    HintRule {
        needle: "讲解",
        hint: "transcript",
    },
    HintRule {
        needle: "开奖",
        hint: "draw results",
    },
    HintRule {
        needle: "开奖号码",
        hint: "winning numbers",
    },
    HintRule {
        needle: "每期",
        hint: "records",
    },
    HintRule {
        needle: "数据",
        hint: "data",
    },
    HintRule {
        needle: "链接",
        hint: "link",
    },
    HintRule {
        needle: "网址",
        hint: "url",
    },
    HintRule {
        needle: "winning number",
        hint: "winning numbers",
    },
    HintRule {
        needle: "winning numbers",
        hint: "winning numbers",
    },
    HintRule {
        needle: "draw result",
        hint: "draw results",
    },
    HintRule {
        needle: "draw results",
        hint: "draw results",
    },
    HintRule {
        needle: "records",
        hint: "records",
    },
    HintRule {
        needle: "record",
        hint: "record",
    },
    HintRule {
        needle: "data",
        hint: "data",
    },
    HintRule {
        needle: "novel",
        hint: "novel",
    },
    HintRule {
        needle: "fiction",
        hint: "fiction",
    },
    HintRule {
        needle: "fantasy",
        hint: "fantasy",
    },
    HintRule {
        needle: "science fiction",
        hint: "science fiction",
    },
    HintRule {
        needle: "sci-fi",
        hint: "science fiction",
    },
    HintRule {
        needle: "scifi",
        hint: "science fiction",
    },
    HintRule {
        needle: "interstellar",
        hint: "interstellar",
    },
    HintRule {
        needle: "space opera",
        hint: "space opera",
    },
    HintRule {
        needle: "ranking",
        hint: "ranking",
    },
    HintRule {
        needle: "rank",
        hint: "ranking",
    },
    HintRule {
        needle: "recommend",
        hint: "recommendation",
    },
    HintRule {
        needle: "小说",
        hint: "novel",
    },
    HintRule {
        needle: "玄幻",
        hint: "fantasy",
    },
    HintRule {
        needle: "科幻",
        hint: "science fiction",
    },
    HintRule {
        needle: "星际",
        hint: "interstellar",
    },
    HintRule {
        needle: "太空",
        hint: "space",
    },
    HintRule {
        needle: "宇宙",
        hint: "space",
    },
    HintRule {
        needle: "排行",
        hint: "ranking",
    },
    HintRule {
        needle: "排名",
        hint: "ranking",
    },
    HintRule {
        needle: "榜单",
        hint: "ranking",
    },
    HintRule {
        needle: "推荐",
        hint: "recommendation",
    },
    HintRule {
        needle: "免费",
        hint: "free",
    },
    HintRule {
        needle: "下载",
        hint: "download",
    },
    HintRule {
        needle: "download",
        hint: "download",
    },
];

const EVIDENCE_HINT_RULES: &[HintRule] = &[
    HintRule {
        needle: "link",
        hint: "link",
    },
    HintRule {
        needle: "url",
        hint: "url",
    },
    HintRule {
        needle: "source",
        hint: "source",
    },
    HintRule {
        needle: "headline",
        hint: "headline",
    },
    HintRule {
        needle: "official",
        hint: "official",
    },
    HintRule {
        needle: "record",
        hint: "record",
    },
    HintRule {
        needle: "链接",
        hint: "link",
    },
    HintRule {
        needle: "网址",
        hint: "url",
    },
    HintRule {
        needle: "来源",
        hint: "source",
    },
    HintRule {
        needle: "原文",
        hint: "full text",
    },
    HintRule {
        needle: "官方",
        hint: "official",
    },
    HintRule {
        needle: "记录",
        hint: "record",
    },
];

#[cfg(test)]
mod tests {
    use super::SearchPolicy;
    use crate::tool::web_search::orchestrator::SourceCapability;
    use serde_json::json;

    #[test]
    fn lookup_intent_maps_qidian_short_name_to_official_site() {
        let intent = SearchPolicy::build_lookup_intent("搜索起点前10免费小说");
        assert!(intent
            .site_hints
            .iter()
            .any(|hint| hint == "site:qidian.com"));
    }

    #[test]
    fn artifact_policy_value_can_add_site_and_artifact_hints() {
        let policy = json!({
            "handles": [{
                "artifact": "custom_public_record",
                "triggers": ["测试开奖记录"],
                "sites": ["records.example.com"],
                "evidence_hints": ["official"],
                "direct_record_hints": ["record"]
            }]
        });
        let mut intent = SearchPolicy::build_lookup_intent("测试开奖记录");
        SearchPolicy::apply_artifact_policy_value(
            &mut intent,
            &policy,
            "测试开奖记录",
            "测试开奖记录",
        );

        assert!(intent
            .site_hints
            .iter()
            .any(|hint| hint == "site:records.example.com"));
        assert!(intent
            .artifact_hints
            .iter()
            .any(|hint| hint == "custom_public_record"));
        assert!(intent.evidence_hints.iter().any(|hint| hint == "official"));
    }

    #[test]
    fn artifact_policy_value_can_add_seed_urls() {
        let policy = json!({
            "handles": [{
                "artifact": "custom_public_record",
                "triggers": ["测试开奖记录"],
                "seed_urls": ["https://records.example.com/latest"]
            }]
        });
        let mut urls = Vec::new();
        SearchPolicy::append_policy_urls_for_task(
            &mut urls,
            &policy,
            "测试开奖记录",
            "测试开奖记录",
        );

        assert_eq!(urls, vec!["https://records.example.com/latest"]);
    }

    #[test]
    fn browser_seed_urls_use_site_hints_and_policy_paths() {
        let urls = SearchPolicy::browser_site_seed_urls_for_task("搜索起点前十免费玄幻小说");
        assert!(urls.iter().any(|url| url.contains("qidian.com")));
        assert!(urls
            .iter()
            .any(|url| url.contains("/free/") || url.contains("/xuanhuan/")));
    }

    #[test]
    fn delegate_fast_path_budget_covers_direct_site_budget() {
        let direct =
            SearchPolicy::browser_direct_site_budget_secs_for_task("搜索起点前十免费玄幻小说");
        let delegate =
            SearchPolicy::delegate_fast_path_budget_secs_for_task("搜索起点前十免费玄幻小说");
        assert!(delegate > direct);
    }

    #[test]
    fn artifact_policy_value_can_add_source_adapters() {
        let policy = json!({
            "handles": [{
                "artifact": "custom_search",
                "triggers": ["测试搜索源"],
                "sources": ["custom_source"]
            }]
        });
        let mut sources = Vec::new();
        SearchPolicy::append_policy_sources_for_task(
            &mut sources,
            &policy,
            "测试搜索源",
            "测试搜索源",
        );

        assert_eq!(sources, vec!["custom_source"]);
    }

    #[test]
    fn artifact_policy_value_can_add_collection_facets() {
        let policy = json!({
            "handles": [{
                "artifact": "ranked_collection",
                "triggers": ["测试榜单"],
                "collection_facets": [{
                    "name": "free_collection",
                    "requested_by": ["免费"],
                    "evidence_terms": ["免费"],
                    "conflicting_terms": ["推荐"]
                }],
                "collection_item_facets": [{
                    "name": "custom_genre",
                    "requested_by": ["类型A"],
                    "evidence_terms": ["类型A"],
                    "conflicting_terms": ["类型B"]
                }]
            }]
        });
        let mut collection_facets = Vec::new();
        SearchPolicy::append_policy_collection_facets(
            &mut collection_facets,
            &policy,
            "测试榜单 免费",
            "测试榜单 免费",
            false,
        );
        assert_eq!(collection_facets[0].name, "free_collection");

        let mut item_facets = Vec::new();
        SearchPolicy::append_policy_collection_facets(
            &mut item_facets,
            &policy,
            "测试榜单 类型A",
            "测试榜单 类型a",
            true,
        );
        assert_eq!(item_facets[0].conflicting_terms, vec!["类型B"]);
    }

    #[test]
    fn artifact_policy_yaml_file_can_be_loaded_directly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("artifact_policy.yaml");
        std::fs::write(
            &path,
            "handles:\n  - artifact: custom_record\n    triggers: [测试记录]\n",
        )
        .expect("write policy");

        let policy = SearchPolicy::read_artifact_policy_yaml(&path).expect("policy");
        assert_eq!(
            policy["handles"][0]["artifact"].as_str(),
            Some("custom_record")
        );
    }

    #[test]
    fn artifact_policy_yaml_compiles_term_references() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("artifact_policy.yaml");
        std::fs::write(
            &path,
            "terms:\n  paper_query:\n    aliases: [论文, paper]\n  evidence:\n    aliases: [official, record]\nhandles:\n  - artifact: academic_paper\n    triggers_from: [paper_query]\n    evidence_hints_from: [evidence]\n",
        )
        .expect("write policy");

        let policy = SearchPolicy::read_artifact_policy_yaml(&path).expect("policy");
        assert_eq!(
            policy["handles"][0]["triggers"].as_array().unwrap()[0].as_str(),
            Some("论文")
        );
        assert!(policy["handles"][0]["evidence_hints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some("record")));
    }

    #[test]
    fn artifact_policy_value_can_override_source_adapter_shape() {
        let policy = json!({
            "handles": [{
                "artifact": "custom_search",
                "triggers": ["测试搜索源"],
                "source_adapters": [{
                    "name": "custom_source",
                    "capability": "public",
                    "requires_browser": false,
                    "requires_auth": true,
                    "challenge_prone": false,
                    "domains": ["api.example.com"],
                    "fallback_sources": ["browser"],
                    "weight": 77
                }]
            }]
        });
        let mut overrides = Vec::new();
        SearchPolicy::append_policy_source_adapter_overrides(
            &mut overrides,
            &policy,
            "测试搜索源",
            "测试搜索源",
        );

        assert_eq!(overrides.len(), 1);
        let override_spec = &overrides[0];
        assert_eq!(override_spec.name, "custom_source");
        assert_eq!(override_spec.capability, Some(SourceCapability::Public));
        assert_eq!(override_spec.requires_auth, Some(true));
        assert_eq!(
            override_spec.domains.as_deref(),
            Some(&["api.example.com".to_string()][..])
        );
        assert_eq!(override_spec.weight, Some(77));
    }

    #[test]
    fn artifact_policy_value_reports_invalid_source_adapter() {
        let policy = json!({
            "handles": [{
                "artifact": "custom_search",
                "triggers": ["测试搜索源"],
                "source_adapters": [{
                    "name": "custom_source",
                    "capability": "telepathy"
                }]
            }]
        });
        let mut diagnostics = Vec::new();
        SearchPolicy::append_policy_source_diagnostics(
            &mut diagnostics,
            &policy,
            "测试搜索源",
            "测试搜索源",
        );

        assert_eq!(
            diagnostics,
            vec!["source_adapter 'custom_source' has unsupported capability 'telepathy'"]
        );
    }
}

const DIRECT_RECORD_HINT_RULES: &[HintRule] = &[
    HintRule {
        needle: "doi",
        hint: "doi",
    },
    HintRule {
        needle: "pubmed",
        hint: "pubmed",
    },
    HintRule {
        needle: "pmc",
        hint: "pmc",
    },
    HintRule {
        needle: "crossref",
        hint: "crossref",
    },
    HintRule {
        needle: "full text",
        hint: "full text",
    },
    HintRule {
        needle: "abstract",
        hint: "abstract",
    },
    HintRule {
        needle: "开放全文",
        hint: "open access",
    },
    HintRule {
        needle: "全文",
        hint: "full text",
    },
    HintRule {
        needle: "摘要",
        hint: "abstract",
    },
];

struct PublicDataSourcePolicy {
    matches: fn(&str) -> bool,
    urls: &'static [&'static str],
}

const PUBLIC_DATA_SOURCE_POLICIES: &[PublicDataSourcePolicy] = &[PublicDataSourcePolicy {
    matches: SearchPolicy::task_requests_china_welfare_lottery_records,
    urls: &[
        "https://www.cwl.gov.cn/ygkj/wqkjgg/ssq/",
        "https://cp.ip138.com/shuangseqiu/",
        "https://caipiao.ip138.com/shuangseqiu/",
        "https://cp.ip138.com/quanguo/",
        "https://caipiao.eastmoney.com/pub/Result/History/ssq",
        "https://kaijiang.500.com/index_fc.shtml",
        "https://kaijiang.78500.cn/ssq/",
    ],
}];
