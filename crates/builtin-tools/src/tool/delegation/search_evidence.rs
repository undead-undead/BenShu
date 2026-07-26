use super::policy::{PolicyPhase, QualityContract, RuntimePolicyResolver, TaskPolicyInput};
use super::*;
use crate::tool::writing::artifact_contract::{ArtifactQualityContract, ArtifactQualityReport};

fn artifact_quality_contract_from_policy(contract: QualityContract) -> ArtifactQualityContract {
    ArtifactQualityContract::new(
        contract.artifact_type,
        None,
        contract.min_chars,
        contract.max_chars,
        contract.min_citations,
        contract.required_sections,
        contract.required_section_label,
        contract.require_title,
        contract.require_self_review,
    )
}

impl DelegateTool {
    pub(crate) fn build_lookup_intent(task: &str) -> LookupIntent {
        SearchPolicy::build_lookup_intent(task)
    }

    pub(crate) fn build_lookup_query_intent(task: &str) -> LookupIntent {
        let lookup_text = Self::lookup_surface_text(task);
        let mut intent = SearchPolicy::build_lookup_intent(&lookup_text);
        intent.base_terms = Self::filter_lookup_query_terms(intent.base_terms);
        intent.artifact_hints = Self::filter_lookup_query_terms(intent.artifact_hints);
        intent.evidence_hints = Self::filter_lookup_query_terms(intent.evidence_hints);
        intent.freshness_hints = Self::filter_lookup_query_terms(intent.freshness_hints);
        intent.direct_record_hints = Self::filter_lookup_query_terms(intent.direct_record_hints);
        intent
    }

    pub(crate) fn lookup_surface_text(task: &str) -> String {
        let task = task
            .split("\n---\n### NOTICE:")
            .next()
            .unwrap_or(task)
            .trim();
        let markers = [
            "Original user request:",
            "完整用户请求（必须保留查找之后的后续阶段，不能只完成查找片段）：",
            "完整用户请求:",
            "User task:",
        ];

        let mut parts = Vec::new();
        let earliest_marker = markers
            .into_iter()
            .filter_map(|marker| task.find(marker).map(|index| (index, marker)))
            .min_by_key(|(index, _)| *index);

        if let Some((index, marker)) = earliest_marker {
            let prefix = task[..index].trim();
            let tail = task[index + marker.len()..]
                .split("\n\nDelegated task:")
                .next()
                .unwrap_or_default()
                .split("\nDelegated task:")
                .next()
                .unwrap_or_default()
                .split("\n\nOriginal delegated task:")
                .next()
                .unwrap_or_default()
                .split("\n\nVerified researcher evidence:")
                .next()
                .unwrap_or_default()
                .split("\n\nKnowledge import receipt:")
                .next()
                .unwrap_or_default()
                .split("\n\nArtifact contract:")
                .next()
                .unwrap_or_default()
                .trim();
            if !tail.is_empty() {
                parts.push(tail);
            } else if !prefix.is_empty() && marker != "User task:" {
                parts.push(prefix);
            }
        } else {
            parts.push(task);
        }

        let joined = parts.join("\n");
        let joined = if joined.trim().is_empty() {
            task.to_string()
        } else {
            joined
        };
        let phase_surface = SearchPolicy::lookup_surface_from_task_context(&joined);
        if phase_surface.trim().is_empty() {
            joined
        } else {
            phase_surface
        }
    }

    pub(crate) fn filter_lookup_query_terms(terms: Vec<String>) -> Vec<String> {
        let mut filtered = Vec::new();
        for term in terms {
            if Self::is_lookup_query_noise_term(&term) {
                continue;
            }
            Self::push_unique(&mut filtered, term);
        }
        Self::prune_redundant_cjk_lookup_terms(&mut filtered);
        filtered
    }

    pub(crate) fn is_lookup_query_noise_term(term: &str) -> bool {
        let trimmed = term.trim();
        if trimmed.is_empty() || Self::is_lookup_noise_term(trimmed) {
            return true;
        }
        let contains_cjk = trimmed
            .chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
        if contains_cjk
            && (Self::is_cjk_relevance_noise(trimmed)
                || trimmed.starts_with(['以', '对', '让'])
                || (trimmed.contains('的') && trimmed.chars().count() > 4))
        {
            return true;
        }
        if contains_cjk
            && trimmed.chars().any(|ch| ch.is_ascii_digit())
            && trimmed.chars().count() > 6
        {
            return true;
        }
        let lowered = trimmed.to_ascii_lowercase();
        if Self::looks_like_internal_query_identifier(&lowered) {
            return true;
        }
        if lowered.starts_with("call:")
            || lowered.starts_with("tool:")
            || lowered.ends_with("_lookup")
            || lowered.ends_with("_search")
            || lowered.ends_with("_management")
            || lowered.ends_with("_document")
            || lowered.ends_with("_paper")
            || lowered.starts_with("fetch_")
            || lowered.starts_with("read_")
            || lowered.starts_with("summarize_")
            || lowered.starts_with("extract_")
            || lowered.starts_with("find_")
        {
            return true;
        }
        matches!(
            lowered.as_str(),
            "knowledge_lookup"
                | "read_saved_knowledge"
                | "knowledge_management"
                | "web_research"
                | "source_research"
                | "summarize_sources"
                | "academic_paper"
                | "find_papers"
                | "fetch_abstract"
                | "source_summary"
                | "import"
                | "ingest"
                | "ingestion"
                | "pdf_document"
                | "browser_browse"
                | "tiered_search"
                | "web_fetch"
                | "web_page"
                | "code_repository"
                | "inspect_project"
                | "inspect"
                | "browse"
                | "recall"
                | "fetch"
                | "this"
                | "task"
                | "step"
                | "not"
                | "update"
                | "delete"
                | "parse"
                | "summarize"
                | "open"
                | "access"
                | "open access"
                | "record"
                | "prior"
                | "could"
                | "cannot"
                | "enough"
                | "verified"
                | "page"
                | "evidence"
                | "scrape"
                | "copyrighted"
                | "brief"
                | "authors"
                | "news"
                | "draft"
                | "drafts"
                | "revise"
                | "revision"
                | "revisions"
                | "plan"
                | "compose"
                | "composer"
                | "architect"
                | "audit"
                | "auditor"
        ) || matches!(
            trimmed,
            "完整用户请求"
                | "必须保留"
                | "查找之后"
                | "后续阶段"
                | "不能只完成"
                | "片段"
                | "任务包含"
                | "请先执行"
                | "存储步骤"
                | "撰写"
                | "转换"
                | "文件"
                | "写一篇"
                | "拿到结果后"
                | "根据这些知识"
                | "将这些知识"
                | "导出"
                | "格式"
                | "关键信息"
                | "相关"
        )
    }

    fn looks_like_internal_query_identifier(lowered: &str) -> bool {
        lowered.contains('_')
            && lowered
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    }

    pub(crate) fn site_hint_host(site_hint: &str) -> Option<&str> {
        SearchPolicy::site_hint_host(site_hint)
    }

    pub(crate) fn compose_lookup_query(parts: &[Vec<String>]) -> String {
        let mut seen = std::collections::HashSet::new();
        let mut query = Vec::new();
        for group in parts {
            for value in group {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let key = trimmed.to_ascii_lowercase();
                if seen.insert(key) {
                    query.push(trimmed.to_string());
                }
            }
        }
        query.join(" ")
    }

    pub(crate) fn compact_lookup_query(task: &str) -> String {
        let lookup_text = Self::lookup_surface_text(task);
        let trimmed = lookup_text.trim();
        if trimmed.is_empty() {
            return trimmed.to_string();
        }
        let intent = Self::build_lookup_query_intent(task);
        let query = Self::compose_lookup_query(&[
            intent.site_hints.clone(),
            intent.base_terms.clone(),
            intent.artifact_hints.clone(),
            intent.evidence_hints.clone(),
            intent.freshness_hints.clone(),
        ]);
        if query.is_empty() {
            trimmed
                .split_whitespace()
                .take(18)
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            query
        }
    }

    pub(crate) fn task_prefers_academic_sources(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        lowered.contains("paper")
            || lowered.contains("study")
            || lowered.contains("journal")
            || lowered.contains("doi")
            || lowered.contains("pubmed")
            || lowered.contains("crossref")
            || lowered.contains("pmc")
            || task.contains("论文")
            || task.contains("研究")
            || task.contains("期刊")
            || task.contains("学术")
    }

    pub(crate) fn task_prefers_structured_sources(task: &str) -> bool {
        !Self::preferred_lookup_hosts_for_task(task).is_empty()
    }

    pub(crate) fn task_requires_structured_followup(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        Self::task_prefers_academic_sources(task)
            || lowered.contains("doi")
            || lowered.contains("pubmed")
            || lowered.contains("pmc")
            || lowered.contains("crossref")
            || task.contains("论文")
            || task.contains("期刊")
            || task.contains("学术")
    }

    pub(crate) fn lookup_query_variants(task: &str) -> Vec<String> {
        let lookup_text = Self::lookup_surface_text(task);
        let intent = Self::build_lookup_query_intent(task);
        let primary = Self::compact_lookup_query(task);
        let mut variants = Vec::new();
        for quoted in Self::extract_quoted_terms(&lookup_text) {
            let quoted = quoted.trim();
            if quoted.chars().count() > 2
                && !quoted.contains("://")
                && !Self::is_lookup_query_noise_term(quoted)
            {
                variants.push(quoted.to_string());
            }
        }
        if !Self::task_prefers_academic_sources(&lookup_text) {
            for query in Self::cjk_priority_lookup_queries(&lookup_text, &intent) {
                variants.push(query);
            }
        }
        if Self::task_requests_data_or_records(task) {
            let data_base_terms = Self::compact_data_lookup_terms(&intent.base_terms);
            for query in Self::official_data_lookup_query_hints(task) {
                variants.push(query);
            }
            variants.push(Self::compose_lookup_query(&[
                data_base_terms.clone(),
                vec![
                    "official".to_string(),
                    "results".to_string(),
                    "records".to_string(),
                    "data".to_string(),
                ],
                intent.freshness_hints.clone(),
            ]));
            variants.push(Self::compose_lookup_query(&[
                data_base_terms,
                vec![
                    "官方".to_string(),
                    "开奖结果".to_string(),
                    "开奖记录".to_string(),
                    "数据".to_string(),
                ],
                intent.freshness_hints.clone(),
            ]));
        }
        if !intent.direct_record_hints.is_empty() {
            variants.push(Self::compose_lookup_query(&[
                intent.direct_record_hints.clone(),
                intent.base_terms.clone(),
                intent.site_hints.clone(),
                intent.freshness_hints.clone(),
            ]));
        }
        if !intent.artifact_hints.is_empty() {
            variants.push(Self::compose_lookup_query(&[
                intent.base_terms.clone(),
                intent.artifact_hints.clone(),
                intent.site_hints.clone(),
                intent.evidence_hints.clone(),
                intent.freshness_hints.clone(),
            ]));
        }
        if !intent.site_hints.is_empty() {
            variants.push(Self::compose_lookup_query(&[
                intent.site_hints.clone(),
                intent.base_terms.clone(),
                intent.evidence_hints.clone(),
                intent.freshness_hints.clone(),
            ]));
        }
        variants.push(primary);

        let mut seen = std::collections::HashSet::new();
        variants
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .filter(|value| seen.insert(value.clone()))
            .collect()
    }

    pub(crate) fn cjk_priority_lookup_queries(task: &str, intent: &LookupIntent) -> Vec<String> {
        if !task
            .chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
        {
            return Vec::new();
        }

        let mut terms = Vec::new();
        for term in Self::cjk_lookup_subject_terms(task, &intent.base_terms) {
            Self::push_unique(&mut terms, term);
        }
        if terms.is_empty() {
            for term in &intent.base_terms {
                if Self::is_compact_cjk_lookup_term(term)
                    && !Self::is_lookup_noise_term(term)
                    && !Self::is_cjk_relevance_noise(term)
                {
                    Self::push_unique(&mut terms, term.clone());
                } else if term
                    .chars()
                    .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
                {
                    for compact in Self::cjk_relevance_terms(term) {
                        if Self::is_compact_cjk_lookup_term(&compact)
                            && !Self::is_lookup_noise_term(&compact)
                            && !Self::is_cjk_relevance_noise(&compact)
                        {
                            Self::push_unique(&mut terms, compact);
                        }
                    }
                }
            }
            for compact in Self::cjk_relevance_terms(task) {
                if Self::is_compact_cjk_lookup_term(&compact)
                    && !Self::is_lookup_noise_term(&compact)
                    && !Self::is_cjk_relevance_noise(&compact)
                {
                    Self::push_unique(&mut terms, compact);
                }
            }
        }
        Self::prune_redundant_cjk_lookup_terms(&mut terms);
        Self::prioritize_cjk_lookup_terms(&mut terms);
        if terms.is_empty() {
            return Vec::new();
        }

        let mut cjk_artifacts = Vec::new();
        if Self::task_requests_collection_or_ranking(task) {
            for value in ["排行", "榜单", "推荐", "列表"] {
                Self::push_unique(&mut cjk_artifacts, value);
            }
        }
        if task.contains("免费") {
            Self::push_unique(&mut cjk_artifacts, "免费");
        }
        if task.contains("下载") {
            Self::push_unique(&mut cjk_artifacts, "下载");
        }
        for hint in &intent.artifact_hints {
            match hint.as_str() {
                "free" => Self::push_unique(&mut cjk_artifacts, "免费"),
                "download" | "downloadable" => Self::push_unique(&mut cjk_artifacts, "下载"),
                "open access" => Self::push_unique(&mut cjk_artifacts, "开放获取"),
                _ => {}
            }
        }
        if Self::task_requests_data_or_records(task) {
            for value in ["官方", "数据", "记录"] {
                Self::push_unique(&mut cjk_artifacts, value);
            }
        }

        let concise = Self::compose_lookup_query(&[
            intent.site_hints.clone(),
            terms.clone(),
            cjk_artifacts.clone(),
        ]);
        let broad = Self::compose_lookup_query(&[terms, cjk_artifacts]);

        let mut queries = Vec::new();
        if !broad.is_empty() {
            queries.push(broad);
        }
        if !concise.is_empty() {
            queries.push(concise);
        }
        queries
    }

    fn cjk_lookup_subject_terms(task: &str, base_terms: &[String]) -> Vec<String> {
        let mut terms = Vec::new();
        let surface = Self::cjk_lookup_subject_surface(task).unwrap_or_else(|| task.to_string());
        for phrase in SearchPolicy::cjk_lookup_phrases(&surface) {
            for cleaned in Self::split_clean_cjk_subject_terms(&phrase) {
                Self::push_unique(&mut terms, cleaned);
            }
        }
        if surface == task {
            for term in base_terms {
                if !term
                    .chars()
                    .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
                {
                    continue;
                }
                for cleaned in Self::split_clean_cjk_subject_terms(term) {
                    Self::push_unique(&mut terms, cleaned);
                }
            }
        }
        terms.retain(|term| {
            Self::is_compact_cjk_lookup_term(term)
                && !Self::is_lookup_noise_term(term)
                && !Self::is_cjk_relevance_noise(term)
                && !Self::is_cjk_lookup_subject_stage_noise(term)
        });
        Self::prune_redundant_cjk_lookup_terms(&mut terms);
        terms.truncate(6);
        terms
    }

    fn cjk_lookup_subject_surface(task: &str) -> Option<String> {
        let mut best = None;
        for raw in task
            .split(['\n', '\r', '。', '.', ';', '；'])
            .map(str::trim)
            .filter(|part| {
                part.chars()
                    .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
            })
        {
            let raw = Self::strip_lookup_surface_prefix(raw).trim();
            let mut normalized = raw.to_string();
            for separator in ["然后", "之后", "接着", "并且", "同时", "根据"] {
                normalized = normalized.replace(separator, "\n");
            }
            for segment in normalized.split(['\n', '，', ',']).map(str::trim) {
                if segment.chars().count() < 2 {
                    continue;
                }
                let lookupish = [
                    "搜索",
                    "查找",
                    "检索",
                    "寻找",
                    "找",
                    "下载",
                    "可下载",
                    "免费",
                    "公网",
                    "公开",
                    "读取",
                    "抓取",
                    "采集",
                    "素材",
                    "正文",
                    "全文",
                ]
                .iter()
                .any(|marker| segment.contains(marker));
                let artifactish = [
                    "写",
                    "创作",
                    "生成",
                    "全新",
                    "不能",
                    "复制",
                    "角色",
                    "漂移",
                    "保存成",
                ]
                .iter()
                .any(|marker| segment.contains(marker));
                if lookupish && !artifactish {
                    let candidate = segment.split(['把', '将']).next().unwrap_or(segment).trim();
                    if candidate.chars().count() >= 2 {
                        best = Some(candidate.to_string());
                        break;
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        best
    }

    fn strip_lookup_surface_prefix(raw: &str) -> &str {
        let trimmed = raw.trim();
        for marker in [
            "Original user request:",
            "original user request:",
            "User task:",
            "user task:",
            "完整用户请求:",
            "完整用户请求：",
        ] {
            if let Some((_, tail)) = trimmed.split_once(marker) {
                return tail;
            }
        }
        if let Some((label, tail)) = trimmed.split_once([':', '：']) {
            let label = label.trim();
            let label_lowered = label.to_ascii_lowercase();
            let label_looks_structural = label.chars().count() <= 48
                && (label_lowered.contains("task")
                    || label_lowered.contains("request")
                    || label_lowered.contains("query")
                    || label.contains("任务")
                    || label.contains("请求")
                    || label.contains("查询"));
            if label_looks_structural {
                return tail;
            }
        }
        trimmed
    }

    fn split_clean_cjk_subject_terms(input: &str) -> Vec<String> {
        let mut terms = Vec::new();
        for segment in input
            .split(|ch: char| {
                matches!(
                    ch,
                    ',' | '.'
                        | ';'
                        | ':'
                        | '!'
                        | '?'
                        | '，'
                        | '。'
                        | '；'
                        | '：'
                        | '！'
                        | '？'
                        | '、'
                )
            })
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
        {
            let cleaned = Self::clean_cjk_subject_term(segment);
            if cleaned.chars().count() >= 2 {
                Self::push_unique(&mut terms, cleaned);
            }
        }
        terms
    }

    fn clean_cjk_subject_term(input: &str) -> String {
        let mut text = input.trim().to_string();
        for separator in [
            "之后", "然后", "接着", "并且", "以及", "同时", "要求", "根据", "保存", "存到", "放到",
            "导入", "入库",
        ] {
            if let Some(index) = text.find(separator) {
                text.truncate(index);
            }
        }
        for prefix in [
            "搜索", "查找", "检索", "寻找", "找", "获取", "下载", "抓取", "采集", "浏览", "读取",
            "请", "帮我", "一部", "一个", "一篇", "一份",
        ] {
            while text.starts_with(prefix) {
                text = text[prefix.len()..].trim().to_string();
            }
        }
        for removable in [
            "公网可下载",
            "公网",
            "可下载",
            "可以下载",
            "下载的",
            "下载",
            "可读取",
            "可以读取",
            "公开",
            "免费的",
            "免费",
        ] {
            text = text.replace(removable, "");
        }
        if let Some((head, tail)) = text.split_once('的') {
            if head.chars().count() <= 1 && tail.chars().count() >= 2 {
                text = tail.to_string();
            }
        }
        text.trim_matches(|ch: char| matches!(ch, '的' | '了' | '和' | '与' | '及' | '、'))
            .to_string()
    }

    fn is_cjk_lookup_subject_stage_noise(term: &str) -> bool {
        [
            "不能", "简单", "复制", "素材", "全新", "创作", "生成", "角色", "漂移", "要求", "保存",
            "文件", "文档", "任务", "问题",
        ]
        .iter()
        .any(|marker| term.contains(marker))
    }

    fn prioritize_cjk_lookup_terms(terms: &mut [String]) {
        terms.sort_by(|left, right| {
            Self::cjk_lookup_term_score(right)
                .cmp(&Self::cjk_lookup_term_score(left))
                .then_with(|| left.chars().count().cmp(&right.chars().count()))
        });
    }

    fn cjk_lookup_term_score(term: &str) -> i32 {
        let len = term.chars().count() as i32;
        let mut score = len.min(8);
        if len >= 3 {
            score += 4;
        } else {
            score -= 3;
        }
        if term.contains('的') || term.contains('和') || term.contains('与') {
            score -= 5;
        }
        score
    }

    pub(crate) fn is_compact_cjk_lookup_term(term: &str) -> bool {
        let len = term.chars().count();
        (2..=8).contains(&len)
            && term
                .chars()
                .all(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
    }

    fn prune_redundant_cjk_lookup_terms(terms: &mut Vec<String>) {
        let original = terms.clone();
        terms.retain(|term| {
            let len = term.chars().count();
            !original.iter().any(|other| {
                other != term
                    && other.chars().count() > len
                    && other.contains(term)
                    && other
                        .chars()
                        .all(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
            })
        });
    }

    pub(crate) fn official_data_lookup_query_hints(task: &str) -> Vec<String> {
        SearchPolicy::official_data_lookup_query_hints(task)
    }

    pub(crate) fn compact_data_lookup_terms(base_terms: &[String]) -> Vec<String> {
        SearchPolicy::compact_data_lookup_terms(base_terms)
    }

    pub(crate) fn search_output_requires_followup(result: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(result) else {
            return false;
        };

        payload
            .get("verification_followup")
            .and_then(|value| value.get("next_tools"))
            .and_then(|value| value.as_array())
            .is_some_and(|tools| tools.iter().any(|tool| tool.as_str() == Some("web_fetch")))
            || payload
                .get("orchestration_decision")
                .and_then(|value| value.get("requires_followup"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
    }

    pub(crate) fn looks_like_worker_status_block(value: &str) -> bool {
        let trimmed = value.trim_start();
        trimmed.starts_with("status: completed\nworker:")
            || trimmed.starts_with("status: blocked\nworker:")
            || trimmed.starts_with("status: failed\nworker:")
    }

    pub(crate) fn looks_like_worker_blocker_status(value: &str) -> bool {
        let trimmed = value.trim_start();
        trimmed.starts_with("status: blocked\nworker:")
            || trimmed.starts_with("status: failed\nworker:")
    }

    pub(crate) fn search_output_has_usable_candidates(task: &str, result: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(result) else {
            return false;
        };
        let Some(results) = payload.get("results").and_then(|value| value.as_array()) else {
            return false;
        };

        results.iter().any(|entry| {
            let url = entry
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let title = entry
                .get("title")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let snippet = entry
                .get("snippet")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            Self::candidate_score(task, url, title, snippet) > 0
        })
    }

    pub(crate) fn search_output_has_any_url_candidates(result: &str) -> bool {
        serde_json::from_str::<serde_json::Value>(result)
            .ok()
            .and_then(|payload| {
                payload
                    .get("results")
                    .and_then(|value| value.as_array())
                    .cloned()
            })
            .is_some_and(|results| {
                results.iter().any(|entry| {
                    entry
                        .get("url")
                        .and_then(|value| value.as_str())
                        .is_some_and(|url| !url.trim().is_empty())
                })
            })
    }

    pub(crate) fn url_is_specific_academic_record(url: &str) -> bool {
        Self::url_is_academic_source(url) && Self::url_has_stable_record_identifier(url)
    }

    pub(crate) fn url_is_academic_source(url: &str) -> bool {
        let Ok(parsed) = Url::parse(url) else {
            return false;
        };
        let host = parsed
            .host_str()
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        host.contains("pubmed.")
            || host.contains("ncbi.nlm.nih.gov")
            || host.contains("doi.org")
            || host.contains("crossref.org")
            || host.contains("openalex.org")
            || host.contains("semanticscholar.org")
            || host.contains("arxiv.org")
            || host.contains("thelancet.com")
            || host.contains("elsevier.com")
            || host.contains("sciencedirect.com")
            || host.contains("springer.com")
            || host.contains("nature.com")
            || host.contains("wiley.com")
            || host.contains("bmj.com")
            || host.contains("nejm.org")
            || host.contains("jamanetwork.com")
    }

    pub(crate) fn url_has_stable_record_identifier(url: &str) -> bool {
        let Ok(parsed) = Url::parse(url) else {
            return false;
        };

        let path = parsed.path().trim_matches('/').to_ascii_lowercase();
        if path.is_empty() || path == "home" || path == "search" || path.ends_with("/search") {
            return false;
        }

        let path_segments = path
            .split('/')
            .filter(|segment| !segment.trim().is_empty())
            .collect::<Vec<_>>();
        let query_pairs = parsed
            .query_pairs()
            .map(|(key, value)| (key.to_ascii_lowercase(), value.to_string()))
            .collect::<Vec<_>>();

        if query_pairs.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "id" | "ids" | "doi" | "pmid" | "pmcid" | "uid" | "record" | "record_id"
            ) && Self::looks_like_record_identifier(value)
        }) {
            return true;
        }

        if query_pairs.iter().any(|(key, value)| {
            matches!(key.as_str(), "term" | "query" | "q" | "search")
                && !Self::looks_like_record_identifier(value)
        }) {
            return false;
        }

        path_segments.iter().any(|segment| {
            let segment = segment.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '.' | ',' | ';' | ':' | '"' | '\'' | '(' | ')' | '[' | ']'
                )
            });
            Self::looks_like_record_identifier(segment)
        }) || (path_segments.len() >= 3
            && path_segments
                .iter()
                .any(|segment| matches!(*segment, "article" | "articles" | "works" | "record")))
    }

    pub(crate) fn looks_like_record_identifier(value: &str) -> bool {
        let value = value
            .trim()
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
                )
            })
            .to_ascii_lowercase();
        if value.is_empty() {
            return false;
        }
        if value.starts_with("10.") && value.contains('/') {
            return true;
        }
        if value.starts_with("pmc") && value.chars().any(|ch| ch.is_ascii_digit()) {
            return true;
        }
        let has_digit = value.chars().any(|ch| ch.is_ascii_digit());
        let has_alpha = value.chars().any(|ch| ch.is_ascii_alphabetic());
        let recordish_len = value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .count();
        if has_digit && recordish_len >= 5 {
            return true;
        }
        has_digit
            && has_alpha
            && value.chars().any(|ch| matches!(ch, '-' | '_' | ':' | '.'))
            && recordish_len >= 4
    }

    pub(crate) fn search_output_has_preferred_academic_candidates(result: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(result) else {
            return false;
        };
        let Some(results) = payload.get("results").and_then(|value| value.as_array()) else {
            return false;
        };

        results.iter().any(|entry| {
            let url = entry
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            Self::url_is_specific_academic_record(url)
                || url.contains("api.crossref.org")
                || url.contains("api.openalex.org")
        })
    }

    pub(crate) fn search_output_has_direct_url_candidates(result: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(result) else {
            return false;
        };
        let Some(results) = payload.get("results").and_then(|value| value.as_array()) else {
            return false;
        };

        results.iter().any(|entry| {
            entry
                .get("source")
                .and_then(|value| value.as_str())
                .is_some_and(|source| source == "direct_url")
        })
    }

    pub(crate) fn preferred_lookup_hosts_for_task(task: &str) -> Vec<String> {
        SearchPolicy::preferred_lookup_hosts_for_task(task)
    }

    pub(crate) fn is_lookup_noise_term(token: &str) -> bool {
        let lowered = token.trim().to_ascii_lowercase();
        matches!(
            lowered.as_str(),
            "search"
                | "for"
                | "the"
                | "of"
                | "in"
                | "on"
                | "or"
                | "and"
                | "to"
                | "into"
                | "then"
                | "after"
                | "before"
                | "once"
                | "from"
                | "lookup"
                | "find"
                | "found"
                | "finding"
                | "latest"
                | "recent"
                | "newest"
                | "research"
                | "related"
                | "regarding"
                | "about"
                | "relevant"
                | "key"
                | "findings"
                | "information"
                | "provide"
                | "include"
                | "answer"
                | "respond"
                | "return"
                | "select"
                | "pick"
                | "choose"
                | "important"
                | "item"
                | "items"
                | "list"
                | "must"
                | "published"
                | "publish"
                | "publishing"
                | "write"
                | "written"
                | "writing"
                | "new"
                | "based"
                | "export"
                | "pdf"
                | "them"
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
                | "tool"
                | "tools"
                | "browser"
                | "web_search"
                | "paper"
                | "papers"
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
                | "abstract"
                | "abstracts"
                | "metadata"
                | "populate"
                | "focus"
                | "focused"
                | "obtain"
                | "extract"
                | "extracted"
                | "prepare"
                | "prepared"
                | "core"
                | "conclusion"
                | "conclusions"
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
                | "概括"
                | "回答"
                | "挑"
                | "选择"
                | "重要"
                | "中文"
                | "用户"
                | "获取"
                | "核心"
                | "结论"
                | "提取"
                | "准备"
                | "后续"
        )
    }

    pub(crate) fn is_lookup_temporal_hint_term(token: &str) -> bool {
        let trimmed = token.trim().trim_matches(|ch: char| {
            matches!(ch, ',' | '.' | ';' | ':' | '，' | '。' | '；' | '：')
        });
        let lowered = trimmed.to_ascii_lowercase();
        if matches!(
            lowered.as_str(),
            "latest" | "recent" | "newest" | "current" | "最近" | "最新"
        ) {
            return true;
        }
        let normalized = lowered.replace(['/', '_'], "-");
        let parts = normalized.split('-').collect::<Vec<_>>();
        if parts
            .iter()
            .all(|part| part.len() == 4 && part.chars().all(|ch| ch.is_ascii_digit()))
            && !parts.is_empty()
        {
            return true;
        }
        normalized.len() == 4 && normalized.chars().all(|ch| ch.is_ascii_digit())
    }

    pub(crate) fn structured_lookup_query(task: &str) -> String {
        Self::structured_lookup_queries(task)
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    pub(crate) fn structured_lookup_queries(task: &str) -> Vec<String> {
        let lookup_text = Self::lookup_surface_text(task);
        let intent = Self::build_lookup_query_intent(task);
        let source_terms = Self::structured_source_terms_from_site_hints(&intent);
        if let Some(quoted) = Self::extract_quoted_text(&lookup_text) {
            let quoted = quoted.trim();
            if quoted.chars().count() > 2
                && !quoted.contains("://")
                && !Self::is_lookup_query_noise_term(quoted)
            {
                return vec![quoted.to_string()];
            }
        }

        let filtered_base_terms = Self::lookup_alignment_terms(&lookup_text);

        let ascii_terms = filtered_base_terms
            .iter()
            .filter(|term| term.is_ascii())
            .cloned()
            .collect::<Vec<_>>();

        let subject_terms = if ascii_terms.len() >= 3 {
            ascii_terms.clone()
        } else if !filtered_base_terms.is_empty() {
            filtered_base_terms.clone()
        } else {
            intent.base_terms.clone()
        };

        let mut trimmed_subject_terms = subject_terms.into_iter().take(8).collect::<Vec<_>>();
        if trimmed_subject_terms.is_empty() {
            trimmed_subject_terms = intent.base_terms.into_iter().take(8).collect::<Vec<_>>();
        }

        let mut queries = Vec::new();
        Self::push_unique(
            &mut queries,
            Self::compose_lookup_query(&[
                source_terms.clone(),
                trimmed_subject_terms.clone(),
                intent.evidence_hints.clone(),
                intent.freshness_hints.clone(),
            ]),
        );

        if !ascii_terms.is_empty() {
            Self::push_unique(
                &mut queries,
                Self::compose_lookup_query(&[
                    source_terms.clone(),
                    ascii_terms.into_iter().take(6).collect::<Vec<_>>(),
                ]),
            );
        }

        if !filtered_base_terms.is_empty() {
            Self::push_unique(
                &mut queries,
                Self::compose_lookup_query(&[
                    source_terms.clone(),
                    filtered_base_terms
                        .iter()
                        .take(4)
                        .cloned()
                        .collect::<Vec<_>>(),
                ]),
            );
        }

        if SearchPolicy::task_requests_recent_material(&lookup_text) && !source_terms.is_empty() {
            Self::push_unique(&mut queries, Self::compose_lookup_query(&[source_terms]));
        }

        queries
    }

    pub(crate) fn structured_source_terms_from_site_hints(intent: &LookupIntent) -> Vec<String> {
        let mut terms = Vec::new();
        for hint in &intent.site_hints {
            let Some(host) = SearchPolicy::site_hint_host(hint) else {
                continue;
            };
            let policy = policy_for_host(host);
            if !policy.challenge_prone || policy.preferred_lookup_hosts.is_empty() {
                continue;
            }
            for term in Self::host_lookup_label_terms(host) {
                Self::push_unique(&mut terms, term);
            }
        }
        terms
    }

    pub(crate) fn host_lookup_label_terms(host: &str) -> Vec<String> {
        let mut terms = Vec::new();
        for label in host
            .trim()
            .trim_end_matches('.')
            .split('.')
            .map(|part| part.trim().to_ascii_lowercase())
        {
            if label.len() < 4
                || matches!(
                    label.as_str(),
                    "www"
                        | "m"
                        | "api"
                        | "com"
                        | "org"
                        | "net"
                        | "gov"
                        | "edu"
                        | "co"
                        | "cn"
                        | "ncbi"
                        | "nlm"
                        | "nih"
                )
            {
                continue;
            }
            if let Some(stripped) = label
                .strip_prefix("the")
                .filter(|value| value.chars().count() >= 4)
            {
                Self::push_unique(&mut terms, stripped.to_string());
                continue;
            }
            Self::push_unique(&mut terms, label.clone());
        }
        terms
    }

    pub(crate) fn structured_lookup_date_bounds(task: &str) -> Option<(String, String)> {
        if !SearchPolicy::task_requests_recent_material(task) {
            return None;
        }
        let today = chrono::Utc::now().date_naive();
        let start_year = today.year().saturating_sub(2);
        Some((format!("{start_year:04}-01-01"), today.to_string()))
    }

    pub(crate) fn structured_lookup_urls(task: &str) -> Vec<String> {
        let queries = Self::structured_lookup_queries(task);
        if queries.is_empty() {
            return Vec::new();
        }

        let mut urls = Vec::new();
        let date_bounds = Self::structured_lookup_date_bounds(task);
        for url in Self::public_data_record_urls_for_task(task) {
            Self::push_unique(&mut urls, url);
        }
        for query in queries {
            for host in Self::preferred_lookup_hosts_for_task(task) {
                match host.as_str() {
                    "pubmed.ncbi.nlm.nih.gov" | "eutils.ncbi.nlm.nih.gov" => {
                        let base_params = vec![
                            ("db".to_string(), "pubmed".to_string()),
                            ("term".to_string(), query.clone()),
                            ("retmax".to_string(), "10".to_string()),
                            ("retmode".to_string(), "json".to_string()),
                        ];
                        let mut dated_params = base_params.clone();
                        let mut relevance_params = base_params;
                        if let Some((from, until)) = date_bounds.as_ref() {
                            let mindate = from.replace('-', "/");
                            let maxdate = until.replace('-', "/");
                            relevance_params.push(("mindate".to_string(), mindate.clone()));
                            relevance_params.push(("maxdate".to_string(), maxdate.clone()));
                            relevance_params.push(("datetype".to_string(), "pdat".to_string()));
                            dated_params.push(("mindate".to_string(), mindate));
                            dated_params.push(("maxdate".to_string(), maxdate));
                            dated_params.push(("datetype".to_string(), "pdat".to_string()));
                        }
                        dated_params.push(("sort".to_string(), "pub+date".to_string()));
                        for params in [relevance_params, dated_params] {
                            if let Ok(url) = Url::parse_with_params(
                                "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi",
                                params
                                    .iter()
                                    .map(|(key, value)| (key.as_str(), value.as_str())),
                            ) {
                                Self::push_unique(&mut urls, url.to_string());
                            }
                        }
                    }
                    "api.crossref.org" => {
                        let mut params = vec![
                            ("query", query.as_str()),
                            ("rows", "5"),
                            ("mailto", Self::STRUCTURED_LOOKUP_MAILTO),
                        ];
                        let mut filter = None;
                        if let Some((from, until)) = date_bounds.as_ref() {
                            filter = Some(format!("from-pub-date:{from},until-pub-date:{until}"));
                        }
                        if let Some(filter) = filter.as_deref() {
                            params.push(("filter", filter));
                        }
                        if let Ok(url) =
                            Url::parse_with_params("https://api.crossref.org/works", &params)
                        {
                            Self::push_unique(&mut urls, url.to_string());
                        }
                    }
                    "api.openalex.org" => {
                        let mut params = vec![
                            ("search", query.as_str()),
                            ("per-page", "5"),
                            ("mailto", Self::STRUCTURED_LOOKUP_MAILTO),
                        ];
                        let mut filter = None;
                        if let Some((from, until)) = date_bounds.as_ref() {
                            filter = Some(format!(
                                "from_publication_date:{from},to_publication_date:{until}"
                            ));
                        }
                        if let Some(filter) = filter.as_deref() {
                            params.push(("filter", filter));
                        }
                        if let Ok(url) =
                            Url::parse_with_params("https://api.openalex.org/works", &params)
                        {
                            Self::push_unique(&mut urls, url.to_string());
                        }
                    }
                    "api.github.com" => {
                        if let Ok(url) = Url::parse_with_params(
                            "https://api.github.com/search/repositories",
                            &[("q", query.as_str()), ("per_page", "5"), ("sort", "stars")],
                        ) {
                            Self::push_unique(&mut urls, url.to_string());
                        }
                    }
                    "www.youtube.com" | "youtube.com" | "youtu.be" => {
                        if let Ok(url) = Url::parse_with_params(
                            "https://www.youtube.com/results",
                            &[("search_query", query.as_str())],
                        ) {
                            Self::push_unique(&mut urls, url.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        urls
    }

    pub(crate) fn public_data_record_urls_for_task(task: &str) -> Vec<String> {
        SearchPolicy::public_data_record_urls_for_task(task)
    }

    pub(crate) fn candidate_score(task: &str, url: &str, title: &str, snippet: &str) -> i32 {
        let intent = Self::build_lookup_intent(task);
        let preferred_lookup_hosts = Self::preferred_lookup_hosts_for_task(task);
        let task_lower = task.to_ascii_lowercase();
        let url_lower = url.to_ascii_lowercase();
        let title_lower = title.to_ascii_lowercase();
        let snippet_lower = snippet.to_ascii_lowercase();

        let text = format!("{title_lower}\n{snippet_lower}");
        let alignment_evidence = format!("{url}\n{title}\n{snippet}");
        if Self::url_looks_like_non_content_navigation(task, url) {
            return 0;
        }
        if Self::collection_intent_alignment_blocker_for_evidence(task, &alignment_evidence)
            .is_some()
        {
            return 0;
        }
        let mut score = 0i32;

        for site_hint in &intent.site_hints {
            let host_hint = site_hint
                .strip_prefix("site:")
                .unwrap_or(site_hint.as_str())
                .to_ascii_lowercase();
            if !host_hint.is_empty() && url_lower.contains(&host_hint) {
                score += 28;
            }
        }

        for preferred_host in &preferred_lookup_hosts {
            if url_lower.contains(&preferred_host.to_ascii_lowercase()) {
                score += 34;
            }
        }

        if Self::url_is_specific_academic_record(&url_lower)
            && url_lower.contains("pubmed.ncbi.nlm.nih.gov")
        {
            score += 22;
        }
        if Self::url_is_specific_academic_record(&url_lower)
            && url_lower.contains("pmc.ncbi.nlm.nih.gov")
        {
            score += 26;
        }
        if Self::url_is_specific_academic_record(&url_lower) && url_lower.contains("doi.org") {
            score += 24;
        }
        if Self::url_is_specific_academic_record(&url_lower) && url_lower.contains("crossref.org") {
            score += 18;
        }

        score += Self::policy_evidence_term_score(&intent, &text, title, snippet);

        if task_lower.contains("paper")
            || task_lower.contains("study")
            || task.contains("论文")
            || task.contains("研究")
        {
            if text.contains("study")
                || text.contains("trial")
                || text.contains("article")
                || url_lower.contains("/article/")
                || url_lower.contains("/onlinefirst")
                || url_lower.contains("/issue/current")
                || Self::url_is_specific_academic_record(&url_lower)
            {
                score += 15;
            }
        }

        if url_lower.contains("/article/")
            || url_lower.contains("/onlinefirst")
            || url_lower.contains("/issue/current")
        {
            score += 18;
        }

        for hint in &intent.artifact_hints {
            let lowered_hint = hint.to_ascii_lowercase();
            if text.contains(&lowered_hint) || url_lower.contains(&lowered_hint.replace(' ', "")) {
                score += 14;
            }
        }

        for hint in &intent.direct_record_hints {
            let lowered_hint = hint.to_ascii_lowercase();
            if text.contains(&lowered_hint) || url_lower.contains(&lowered_hint.replace(' ', "")) {
                score += 18;
            }
        }

        let haystack_original = format!("{url}\n{title}\n{snippet}");
        let mut relevance_hits = 0;
        let mut scoring_terms = intent
            .base_terms
            .iter()
            .filter(|term| !Self::is_lookup_noise_term(term))
            .filter(|term| term.chars().count() > 1)
            .cloned()
            .collect::<Vec<_>>();
        for term in Self::cjk_relevance_terms(task) {
            Self::push_unique(&mut scoring_terms, term);
        }

        for term in scoring_terms {
            if term.is_ascii() {
                let lowered_term = term.to_ascii_lowercase();
                if lowered_term.len() >= 4
                    && (text.contains(&lowered_term) || url_lower.contains(&lowered_term))
                {
                    relevance_hits += 1;
                }
            } else if haystack_original.contains(&term) {
                relevance_hits += 1;
            }
        }
        score += relevance_hits.min(4) * 8;

        if Self::task_requests_data_or_records(task) && relevance_hits == 0 {
            score -= 42;
        }

        let data_or_records_task = Self::task_requests_data_or_records(task);
        let low_trust_data_host =
            data_or_records_task && Self::candidate_host_is_low_trust_for_data(url);

        if data_or_records_task {
            if low_trust_data_host {
                score -= 90;
            }
            let mut subject_hits = 0;
            let mut subject_terms = intent
                .base_terms
                .iter()
                .filter(|term| SearchPolicy::is_data_lookup_subject_term(term))
                .cloned()
                .collect::<Vec<_>>();
            for term in Self::cjk_relevance_terms(task) {
                if !SearchPolicy::is_data_lookup_generic_term(&term)
                    && !Self::is_cjk_relevance_noise(&term)
                {
                    Self::push_unique(&mut subject_terms, term);
                }
            }
            for term in subject_terms {
                if term.is_ascii() {
                    let lowered_term = term.to_ascii_lowercase();
                    if lowered_term.len() >= 4
                        && (text.contains(&lowered_term) || url_lower.contains(&lowered_term))
                    {
                        subject_hits += 1;
                    }
                } else if haystack_original.contains(&term) {
                    subject_hits += 1;
                }
            }
            if subject_hits == 0 {
                score -= 70;
            } else {
                score += (subject_hits.min(3) * 16) as i32;
            }

            if Self::candidate_mentions_data_or_records(url, title, snippet) {
                score += 12;
            } else {
                score -= 36;
            }
            if Self::task_requests_record_collection(task) {
                if Self::candidate_mentions_record_collection(url, title, snippet) {
                    score += 34;
                } else {
                    score -= 28;
                }
                if Self::candidate_looks_like_news_or_portal_index(url, title, snippet) {
                    score -= 140;
                }
            }
        }

        if Self::task_requests_recent_material(task) {
            let years = Self::years_mentioned_in_candidate(url, title, snippet);
            let current_year = chrono::Utc::now().year();
            if years.iter().any(|year| *year == current_year) {
                score += 12;
            }
            if years.iter().any(|year| *year < current_year - 1) {
                score -= 48;
            }
        }

        if url_lower.contains("/blob/")
            || url_lower.contains("/tree/")
            || url_lower.contains("/issues/")
            || url_lower.contains("/pull/")
            || url_lower.contains("/watch")
            || url_lower.contains("/video/")
        {
            score += 18;
        }

        for hint in &intent.site_hints {
            if let Some(host) = Self::site_hint_host(hint) {
                let policy = policy_for_host(host);
                if policy.challenge_prone
                    && url_lower.contains(&host.to_ascii_lowercase())
                    && !preferred_lookup_hosts
                        .iter()
                        .any(|preferred| url_lower.contains(&preferred.to_ascii_lowercase()))
                    && !Self::url_is_specific_academic_record(&url_lower)
                {
                    score -= 18;
                }
            }
        }

        if url_lower == "https://www.thelancet.com/"
            || url_lower == "https://pubmed.ncbi.nlm.nih.gov/"
            || url_lower == "https://pmc.ncbi.nlm.nih.gov/"
            || url_lower == "https://www.crossref.org/"
            || url_lower == "https://doi.org/"
        {
            score -= 35;
        }
        if url_lower.ends_with("/journals/lancet/home") {
            score -= 25;
        }
        if url_lower.ends_with("/journals/lancet/issues") || url_lower.ends_with("/issues") {
            score -= 10;
        }
        if data_or_records_task && Self::url_looks_like_homepage(url) {
            score -= 220;
        }
        if data_or_records_task
            && Self::candidate_host_is_app_store(url)
            && !Self::task_mentions_app_store_subject(task)
        {
            score -= 140;
        }
        if low_trust_data_host {
            score = score.min(0);
        }

        score
    }

    pub(crate) fn policy_evidence_term_score(
        intent: &LookupIntent,
        lowered_text: &str,
        title: &str,
        snippet: &str,
    ) -> i32 {
        let original_text = format!("{title}\n{snippet}");
        let mut hits = 0;
        for term in &intent.evidence_hints {
            let term = term.trim();
            if term.len() < 2
                || matches!(
                    term.to_ascii_lowercase().as_str(),
                    "official" | "record" | "open access" | "source" | "item_records"
                )
            {
                continue;
            }
            if term.is_ascii() {
                if lowered_text.contains(&term.to_ascii_lowercase()) {
                    hits += 1;
                }
            } else if original_text.contains(term) {
                hits += 1;
            }
        }
        hits.min(6) * 8
    }

    pub(crate) fn candidate_host_is_low_trust_for_data(url: &str) -> bool {
        Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
            .is_some_and(|host| {
                host.contains("zhihu.com")
                    || host.contains("mp.weixin.qq.com")
                    || host.contains("weixin.qq.com")
                    || host.contains("reddit.com")
                    || host.contains("x.com")
                    || host.contains("twitter.com")
                    || host.contains("instagram.com")
                    || host.contains("tiktok.com")
            })
    }

    pub(crate) fn candidate_host_is_app_store(url: &str) -> bool {
        let lowered = url.to_ascii_lowercase();
        if lowered.contains("microsoft.com/store") || lowered.contains("/store/apps/") {
            return true;
        }
        Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
            .is_some_and(|host| {
                host == "play.google.com"
                    || host.ends_with(".play.google.com")
                    || host == "apps.apple.com"
                    || host.ends_with(".apps.apple.com")
            })
    }

    pub(crate) fn task_mentions_app_store_subject(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        lowered.contains("app")
            || lowered.contains("android")
            || lowered.contains("ios")
            || lowered.contains("mobile")
            || lowered.contains("google play")
            || lowered.contains("app store")
            || task.contains("应用")
            || task.contains("安卓")
            || task.contains("手机")
    }

    pub(crate) fn url_looks_like_homepage(url: &str) -> bool {
        Url::parse(url).ok().is_some_and(|parsed| {
            let path = parsed.path().trim_matches('/');
            path.is_empty() || path.eq_ignore_ascii_case("home")
        })
    }

    pub(crate) fn task_requests_data_or_records(task: &str) -> bool {
        SearchPolicy::task_requests_data_or_records(task)
    }

    pub(crate) fn task_requests_collection_or_ranking(task: &str) -> bool {
        SearchPolicy::task_requests_collection_or_ranking(task)
    }

    pub(crate) fn task_requests_record_collection(task: &str) -> bool {
        SearchPolicy::task_requests_record_collection(task)
    }

    pub(crate) fn task_requests_recent_material(task: &str) -> bool {
        SearchPolicy::task_requests_recent_material(task)
    }

    pub(crate) fn years_mentioned_in_candidate(url: &str, title: &str, snippet: &str) -> Vec<i32> {
        let text = format!("{url}\n{title}\n{snippet}");
        let chars = text.chars().collect::<Vec<_>>();
        let mut years = Vec::new();
        for window in chars.windows(4) {
            if !window.iter().all(|ch| ch.is_ascii_digit()) {
                continue;
            }
            let year_text = window.iter().collect::<String>();
            let Ok(year) = year_text.parse::<i32>() else {
                continue;
            };
            if (1990..=2100).contains(&year) && !years.contains(&year) {
                years.push(year);
            }
        }
        years
    }

    pub(crate) fn cjk_relevance_terms(task: &str) -> Vec<String> {
        let mut terms = Vec::new();
        for phrase in SearchPolicy::cjk_lookup_phrases(task) {
            Self::push_unique(&mut terms, phrase);
        }
        for run in task
            .split(|ch: char| !('\u{4e00}'..='\u{9fff}').contains(&ch))
            .map(str::trim)
            .filter(|run| run.chars().count() >= 2)
        {
            for size in [2usize, 3, 4] {
                let chars = run.chars().collect::<Vec<_>>();
                if chars.len() < size {
                    continue;
                }
                for window in chars.windows(size) {
                    let term = window.iter().collect::<String>();
                    if !Self::is_cjk_relevance_noise(&term) {
                        Self::push_unique(&mut terms, term);
                    }
                    if terms.len() >= 32 {
                        return terms;
                    }
                }
            }
        }
        terms
    }

    pub(crate) fn is_cjk_relevance_noise(term: &str) -> bool {
        if term.contains("之后")
            || term.contains("然后")
            || term.contains("进行")
            || term.contains("尝试")
            || term.contains("要求")
            || term.contains("可以")
            || term.contains("把")
            || term.contains("将")
            || term.contains("知识库")
            || term.contains("推理")
            || term.contains("情节")
            || term.contains("角色")
            || term.contains("漂移")
            || term.contains("问题")
            || term.contains("解决")
            || term.contains("任务")
            || term.contains("不要")
        {
            return true;
        }
        matches!(
            term,
            "查找"
                | "搜索"
                | "公开"
                | "来源"
                | "抓取"
                | "包括"
                | "保存"
                | "知识"
                | "识库"
                | "放进"
                | "预测"
                | "下一"
                | "一期"
                | "要求"
                | "整理"
                | "最后"
                | "基于"
                | "明确"
                | "说明"
                | "只能"
                | "作为"
                | "参考"
        )
    }

    pub(crate) fn candidate_mentions_data_or_records(
        url: &str,
        title: &str,
        snippet: &str,
    ) -> bool {
        let lowered = format!("{url}\n{title}\n{snippet}").to_ascii_lowercase();
        lowered.contains("data")
            || lowered.contains("record")
            || lowered.contains("result")
            || lowered.contains("number")
            || lowered.contains("list")
            || title.contains("数据")
            || title.contains("记录")
            || title.contains("结果")
            || title.contains("开奖")
            || title.contains("号码")
            || snippet.contains("数据")
            || snippet.contains("记录")
            || snippet.contains("结果")
            || snippet.contains("开奖")
            || snippet.contains("号码")
    }

    pub(crate) fn candidate_mentions_record_collection(
        url: &str,
        title: &str,
        snippet: &str,
    ) -> bool {
        let lowered = format!("{url}\n{title}\n{snippet}").to_ascii_lowercase();
        lowered.contains("history")
            || lowered.contains("historical")
            || lowered.contains("archive")
            || lowered.contains("past")
            || lowered.contains("records")
            || lowered.contains("query")
            || lowered.contains("search")
            || lowered.contains("/history/")
            || lowered.contains("/result/")
            || title.contains("历史")
            || title.contains("往期")
            || title.contains("查询")
            || title.contains("记录")
            || title.contains("开奖公告")
            || title.contains("开奖结果")
            || title.contains("开奖信息")
            || snippet.contains("历史")
            || snippet.contains("往期")
            || snippet.contains("查询")
            || snippet.contains("记录")
            || snippet.contains("开奖公告")
            || snippet.contains("开奖结果")
            || snippet.contains("开奖信息")
    }

    pub(crate) fn candidate_looks_like_news_or_portal_index(
        url: &str,
        title: &str,
        snippet: &str,
    ) -> bool {
        let lowered = format!("{url}\n{title}\n{snippet}").to_ascii_lowercase();
        let strong_index_path = lowered.contains("/index.")
            || lowered.ends_with("/index")
            || lowered.contains("/sy/tt/");
        let looks_like_index = lowered.contains("/index.")
            || lowered.ends_with("/index")
            || lowered.contains("/news/")
            || lowered.contains("/sy/tt/")
            || lowered.contains("portal")
            || title.contains("门户")
            || title.contains("资讯")
            || title.contains("新闻")
            || title.contains("头条");
        strong_index_path
            || (looks_like_index
                && !Self::candidate_mentions_record_collection(url, title, snippet))
    }

    pub(crate) fn url_is_suitable_for_static_fetch_candidate(task: &str, url: &str) -> bool {
        let Ok(parsed) = Url::parse(url) else {
            return false;
        };
        let host = parsed.host_str().unwrap_or_default();
        let policy = policy_for_host(host);
        if matches!(
            policy.mode,
            SiteFetchMode::BrowserOnly | SiteFetchMode::BrowserThenStatic
        ) {
            return false;
        }
        if policy.challenge_prone
            && Self::task_requires_verified_fetch_result(task)
            && !policy.preferred_lookup_hosts.is_empty()
        {
            return false;
        }
        true
    }

    pub(crate) fn best_followup_fetch_urls(task: &str, result: &str, limit: usize) -> Vec<String> {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(result) else {
            return Vec::new();
        };
        let Some(results) = payload.get("results").and_then(|value| value.as_array()) else {
            return Vec::new();
        };

        let mut ranked = results
            .iter()
            .filter_map(|entry| {
                let url = entry.get("url")?.as_str()?.trim();
                let title = entry
                    .get("title")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                let snippet = entry
                    .get("snippet")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                if !Self::url_is_suitable_for_static_fetch_candidate(task, url) {
                    return None;
                }
                if Self::task_requests_data_or_records(task) && Self::url_looks_like_homepage(url) {
                    return None;
                }
                if Self::url_looks_like_non_content_navigation(task, url) {
                    return None;
                }
                let alignment_evidence = format!("{url}\n{title}\n{snippet}");
                if Self::collection_intent_alignment_blocker_for_evidence(task, &alignment_evidence)
                    .is_some()
                {
                    return None;
                }
                let score = Self::candidate_score(task, url, title, snippet);
                let score = if score <= 0
                    && Self::task_requests_collection_or_ranking(task)
                    && Self::candidate_has_minimal_subject_overlap(task, url, title, snippet)
                {
                    1
                } else {
                    score
                };
                Some((score, url.to_string()))
            })
            .collect::<Vec<_>>();

        ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        ranked.dedup_by(|left, right| left.1 == right.1);
        let positive = ranked
            .iter()
            .filter(|(score, _)| *score > 0)
            .cloned()
            .collect::<Vec<_>>();
        if !positive.is_empty() {
            return Self::select_diverse_followup_urls(task, positive, limit);
        }
        Vec::new()
    }

    pub(crate) fn candidate_has_minimal_subject_overlap(
        task: &str,
        url: &str,
        title: &str,
        snippet: &str,
    ) -> bool {
        let haystack = format!("{url}\n{title}\n{snippet}");
        let haystack_lower = haystack.to_ascii_lowercase();
        let intent = Self::build_lookup_intent(task);
        let mut terms = intent
            .base_terms
            .into_iter()
            .filter(|term| !Self::is_lookup_noise_term(term))
            .filter(|term| !Self::is_cjk_relevance_noise(term))
            .filter(|term| term.chars().count() > 1)
            .collect::<Vec<_>>();
        for term in Self::cjk_relevance_terms(task) {
            if !Self::is_lookup_noise_term(&term) && !Self::is_cjk_relevance_noise(&term) {
                Self::push_unique(&mut terms, term);
            }
        }

        terms.into_iter().any(|term| {
            if term.is_ascii() {
                let lowered = term.to_ascii_lowercase();
                lowered.len() >= 4 && haystack_lower.contains(&lowered)
            } else {
                haystack.contains(&term)
            }
        })
    }

    pub(crate) fn select_diverse_followup_urls(
        task: &str,
        ranked: Vec<(i32, String)>,
        limit: usize,
    ) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }

        let prefer_host_diversity = Self::task_requests_collection_or_ranking(task)
            || Self::task_requests_recent_material(task)
            || Self::task_requires_structured_followup(task);
        if !prefer_host_diversity {
            return ranked.into_iter().take(limit).map(|(_, url)| url).collect();
        }

        let mut selected = Vec::new();
        let mut selected_urls = std::collections::HashSet::new();
        let mut host_counts = std::collections::HashMap::<String, usize>::new();

        for (_, url) in &ranked {
            let host = Self::url_host_key(url);
            let count = host_counts.get(&host).copied().unwrap_or_default();
            if count > 0 {
                continue;
            }
            selected.push(url.clone());
            selected_urls.insert(url.clone());
            host_counts.insert(host, count + 1);
            if selected.len() >= limit {
                return selected;
            }
        }

        for (_, url) in ranked {
            if selected_urls.insert(url.clone()) {
                selected.push(url);
                if selected.len() >= limit {
                    break;
                }
            }
        }

        selected
    }

    pub(crate) fn url_host_key(url: &str) -> String {
        Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
            .unwrap_or_else(|| url.to_ascii_lowercase())
    }

    pub(crate) fn followup_fetch_limit_for_task(task: &str) -> usize {
        if Self::task_requests_collection_or_ranking(task) {
            6
        } else if Self::task_requests_recent_material(task)
            || Self::task_requires_structured_followup(task)
        {
            4
        } else {
            2
        }
    }

    pub(crate) fn best_discovery_fetch_urls(task: &str, result: &str, limit: usize) -> Vec<String> {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(result) else {
            return Vec::new();
        };
        let Some(results) = payload.get("results").and_then(|value| value.as_array()) else {
            return Vec::new();
        };

        let mut ranked = results
            .iter()
            .filter_map(|entry| {
                let url = entry.get("url")?.as_str()?.trim();
                let title = entry
                    .get("title")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                let snippet = entry
                    .get("snippet")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                if !Self::url_is_suitable_for_static_fetch_candidate(task, url) {
                    return None;
                }
                Some((
                    Self::candidate_discovery_score(task, url, title, snippet),
                    url.to_string(),
                ))
            })
            .filter(|(score, _)| *score > 0)
            .collect::<Vec<_>>();

        ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        ranked.dedup_by(|left, right| left.1 == right.1);
        ranked.into_iter().take(limit).map(|(_, url)| url).collect()
    }

    pub(crate) fn candidate_discovery_score(
        task: &str,
        url: &str,
        title: &str,
        snippet: &str,
    ) -> i32 {
        if Self::task_requests_data_or_records(task)
            && Self::candidate_host_is_low_trust_for_data(url)
        {
            return 0;
        }
        let mut score = Self::candidate_score(task, url, title, snippet);
        if Self::task_requests_data_or_records(task) && Self::url_looks_like_homepage(url) {
            score += 220;
        }
        if Self::candidate_mentions_data_or_records(url, title, snippet) {
            score += 10;
        }
        score
    }

    pub(crate) fn fetched_result_followup_urls(
        task: &str,
        fetch_payload: &str,
        limit: usize,
    ) -> Vec<String> {
        let mut urls = Self::web_fetch_link_followup_urls(task, fetch_payload, limit);
        for url in Self::structured_lookup_followup_urls(fetch_payload, limit) {
            Self::push_unique(&mut urls, url);
            if urls.len() >= limit {
                break;
            }
        }
        urls.truncate(limit);
        urls
    }

    pub(crate) fn web_fetch_link_followup_urls(
        task: &str,
        fetch_payload: &str,
        limit: usize,
    ) -> Vec<String> {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(fetch_payload) else {
            return Vec::new();
        };
        let Some(links) = payload.get("links").and_then(|value| value.as_array()) else {
            return Vec::new();
        };

        let mut ranked = links
            .iter()
            .filter_map(|link| {
                let url = link.get("url")?.as_str()?.trim();
                if Self::task_requests_data_or_records(task) && Self::url_looks_like_homepage(url) {
                    return None;
                }
                let text = link
                    .get("text")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                Some((
                    Self::candidate_score(task, url, text, text),
                    url.to_string(),
                ))
            })
            .filter(|(score, _)| *score > 0)
            .collect::<Vec<_>>();

        ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        ranked.dedup_by(|left, right| left.1 == right.1);
        ranked.into_iter().take(limit).map(|(_, url)| url).collect()
    }

    pub(crate) fn fetch_result_contains_verification_challenge(content: &str) -> bool {
        let lowered = content.to_ascii_lowercase();
        lowered.contains("正在进行安全验证")
            || lowered.contains("请稍候")
            || lowered.contains("cloudflare")
            || lowered.contains("cloudfront")
            || lowered.contains("403 error")
            || lowered.contains("request blocked")
            || lowered.contains("enable javascript and cookies to continue")
            || lowered.contains("security verification")
            || lowered.contains("anti-bot")
            || lowered.contains("challenge page")
    }

    pub(crate) fn fetch_result_is_low_information(url: &str, content: &str) -> bool {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return true;
        }

        let lowered = trimmed.to_ascii_lowercase();
        let lowered_url = url.to_ascii_lowercase();
        let youtube_shell = lowered_url.contains("youtube.com")
            && lowered.contains("aboutpresscopyrightcontact")
            && lowered.contains("how youtube works")
            && !lowered.contains("watch?v=");
        if youtube_shell {
            return true;
        }

        trimmed.split_whitespace().count() < 8 && trimmed.len() < 120
    }

    pub(crate) fn structured_lookup_payload_has_results(url: &str, content: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(content) else {
            return true;
        };

        let lowered_url = url.to_ascii_lowercase();

        if lowered_url.contains("/entrez/eutils/esearch.fcgi") {
            let count = payload
                .get("esearchresult")
                .and_then(|value| value.get("count"))
                .and_then(|value| value.as_str())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let id_count = payload
                .get("esearchresult")
                .and_then(|value| value.get("idlist"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            return count > 0 || id_count > 0;
        }

        if lowered_url.contains("/entrez/eutils/esummary.fcgi") {
            let uid_count = payload
                .get("result")
                .and_then(|value| value.get("uids"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            let record_count = payload
                .get("result")
                .and_then(|value| value.as_object())
                .map(|items| items.keys().filter(|key| key.as_str() != "uids").count())
                .unwrap_or(0);
            return uid_count > 0 || record_count > 0;
        }

        if lowered_url.contains("api.crossref.org/works") {
            let total = payload
                .get("message")
                .and_then(|value| value.get("total-results"))
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let item_count = payload
                .get("message")
                .and_then(|value| value.get("items"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            return total > 0 || item_count > 0;
        }

        if lowered_url.contains("api.openalex.org/works") {
            let total = payload
                .get("meta")
                .and_then(|value| value.get("count"))
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let item_count = payload
                .get("results")
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            return total > 0 || item_count > 0;
        }

        if lowered_url.contains("api.github.com/search/") {
            let total = payload
                .get("total_count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let item_count = payload
                .get("items")
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            return total > 0 || item_count > 0;
        }

        true
    }

    pub(crate) fn structured_lookup_followup_urls(
        fetch_payload: &str,
        limit: usize,
    ) -> Vec<String> {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(fetch_payload) else {
            return Vec::new();
        };
        let url = payload
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let content = payload
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let Ok(body) = serde_json::from_str::<serde_json::Value>(content) else {
            return Vec::new();
        };

        let mut urls = Vec::new();

        if url.contains("/entrez/eutils/esearch.fcgi") {
            if let Some(ids) = body
                .get("esearchresult")
                .and_then(|value| value.get("idlist"))
                .and_then(|value| value.as_array())
            {
                let ids = ids
                    .iter()
                    .filter_map(|value| value.as_str())
                    .take(limit)
                    .collect::<Vec<_>>();
                if ids.len() > 1 {
                    if let Ok(summary_url) = Url::parse_with_params(
                        "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi",
                        &[
                            ("db", "pubmed"),
                            ("id", &ids.join(",")),
                            ("retmode", "json"),
                        ],
                    ) {
                        Self::push_unique(&mut urls, summary_url.to_string());
                    }
                }
                for id in ids {
                    if let Ok(summary_url) = Url::parse_with_params(
                        "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi",
                        &[("db", "pubmed"), ("id", id), ("retmode", "json")],
                    ) {
                        Self::push_unique(&mut urls, summary_url.to_string());
                    }
                    Self::push_unique(&mut urls, format!("https://pubmed.ncbi.nlm.nih.gov/{id}/"));
                }
            }
            return urls;
        }

        if url.contains("/entrez/eutils/esummary.fcgi") {
            if let Some(records) = Self::pubmed_esummary_records(&body) {
                for record in records.into_iter().take(limit) {
                    if let Some(uid) = record.get("uid").and_then(|value| value.as_str()) {
                        Self::push_unique(
                            &mut urls,
                            format!("https://pubmed.ncbi.nlm.nih.gov/{uid}/"),
                        );
                    }
                    if let Some(doi) = Self::pubmed_esummary_doi(record) {
                        Self::push_unique(&mut urls, format!("https://doi.org/{doi}"));
                    }
                }
            }
            return urls;
        }

        if url.contains("api.crossref.org/works") {
            if let Some(items) = body
                .get("message")
                .and_then(|value| value.get("items"))
                .and_then(|value| value.as_array())
            {
                for item in items.iter().take(limit) {
                    if let Some(doi) = item.get("DOI").and_then(|value| value.as_str()) {
                        let encoded = urlencoding::encode(doi);
                        Self::push_unique(
                            &mut urls,
                            format!("https://api.crossref.org/works/{encoded}"),
                        );
                    }
                    if let Some(resource_url) = item.get("URL").and_then(|value| value.as_str()) {
                        Self::push_unique(&mut urls, resource_url);
                    }
                    if let Some(doi) = item.get("DOI").and_then(|value| value.as_str()) {
                        Self::push_unique(&mut urls, format!("https://doi.org/{doi}"));
                    }
                }
            }
            return urls;
        }

        if url.contains("api.openalex.org/works") {
            if let Some(items) = body.get("results").and_then(|value| value.as_array()) {
                for item in items.iter().take(limit) {
                    if let Some(id) = item.get("id").and_then(|value| value.as_str()) {
                        let api_id = id
                            .replace("https://openalex.org/", "https://api.openalex.org/works/")
                            .replace(
                                "https://api.openalex.org/works/works/",
                                "https://api.openalex.org/works/",
                            );
                        Self::push_unique(&mut urls, api_id);
                    }
                    if let Some(resource_url) = item
                        .get("primary_location")
                        .and_then(|value| value.get("landing_page_url"))
                        .and_then(|value| value.as_str())
                    {
                        Self::push_unique(&mut urls, resource_url);
                    }
                    if let Some(resource_url) = item
                        .get("primary_location")
                        .and_then(|value| value.get("pdf_url"))
                        .and_then(|value| value.as_str())
                    {
                        Self::push_unique(&mut urls, resource_url);
                    }
                    if let Some(doi) = item.get("doi").and_then(|value| value.as_str()) {
                        Self::push_unique(&mut urls, doi);
                    }
                }
            }
            return urls;
        }

        if url.contains("api.github.com/search/") {
            if let Some(items) = body.get("items").and_then(|value| value.as_array()) {
                for item in items.iter().take(limit) {
                    if let Some(resource_url) = item.get("url").and_then(|value| value.as_str()) {
                        Self::push_unique(&mut urls, resource_url);
                    }
                    if let Some(resource_url) =
                        item.get("html_url").and_then(|value| value.as_str())
                    {
                        Self::push_unique(&mut urls, resource_url);
                    }
                }
            }
            return urls;
        }

        urls
    }

    pub(crate) fn is_structured_discovery_url(url: &str) -> bool {
        let lowered = url.to_ascii_lowercase();
        lowered.contains("/entrez/eutils/esearch.fcgi")
            || lowered.contains("api.crossref.org/works")
            || lowered.contains("api.openalex.org/works")
            || lowered.contains("api.github.com/search/")
            || lowered.contains("youtube.com/results")
    }

    pub(crate) fn fetched_result_looks_usable(fetch_payload: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(fetch_payload) else {
            let trimmed = fetch_payload.trim();
            return !trimmed.is_empty()
                && !Self::fetch_result_contains_verification_challenge(trimmed);
        };

        let url = payload
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let content = payload
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or(fetch_payload);

        !content.trim().is_empty()
            && !Self::fetch_result_contains_verification_challenge(content)
            && !Self::fetch_result_is_low_information(url, content)
            && Self::structured_lookup_payload_has_results(url, content)
    }

    pub(crate) fn fetched_result_looks_usable_for_task(task: &str, fetch_payload: &str) -> bool {
        if !Self::fetched_result_looks_usable(fetch_payload) {
            return false;
        }
        if Self::collection_intent_alignment_blocker(task, "", fetch_payload).is_some() {
            return false;
        }
        if Self::task_requests_data_or_records(task) {
            return Self::fetched_result_has_record_values(fetch_payload);
        }
        if Self::task_requests_collection_or_ranking(task) {
            if !Self::fetch_payload_matches_lookup_intent(task, fetch_payload) {
                return false;
            }
            let requested = Self::requested_collection_item_count(task);
            return Self::compact_collection_fetch_summary(task, "", fetch_payload)
                .map(|summary| Self::ranked_summary_item_count(&summary) >= requested)
                .unwrap_or(false);
        }
        if !Self::fetch_payload_matches_lookup_intent(task, fetch_payload) {
            return false;
        }
        if !Self::fetch_payload_matches_requested_material_type(task, fetch_payload) {
            return false;
        }
        true
    }

    pub(crate) fn fetch_payload_matches_requested_material_type(
        task: &str,
        fetch_payload: &str,
    ) -> bool {
        if !Self::task_requests_narrative_source_material(task) {
            return true;
        }

        let evidence = serde_json::from_str::<serde_json::Value>(fetch_payload)
            .ok()
            .and_then(|payload| {
                let url = payload
                    .get("url")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let content = payload
                    .get("content")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                Some(format!("{url}\n{content}"))
            })
            .unwrap_or_else(|| fetch_payload.to_string());
        let lowered = evidence.to_lowercase();

        if !Self::fetch_payload_matches_cjk_narrative_material_terms(task, &evidence) {
            return false;
        }

        let non_narrative_markers = [
            "history and criticism",
            "literary criticism",
            "criticism",
            "bibliography",
            "encyclopedia",
            "dictionary",
            "catalogue",
            "catalog",
            "index of",
            "study of",
            "studies in",
            "themes in",
            "drug themes",
            "essay",
            "essays",
            "commentary",
            "analysis",
            "论文",
            "研究",
            "评论",
            "评析",
            "赏析",
            "主题研究",
            "目录",
            "索引",
        ];
        if non_narrative_markers
            .iter()
            .any(|marker| lowered.contains(marker))
            && !Self::narrative_source_signal_is_strong(&lowered)
        {
            return false;
        }

        Self::narrative_source_signal_is_present(&lowered)
            && Self::narrative_genre_signal_matches_request(task, &lowered)
    }

    pub(crate) fn fetch_payload_matches_cjk_narrative_material_terms(
        task: &str,
        evidence: &str,
    ) -> bool {
        if !Self::task_requests_narrative_source_material(task) {
            return true;
        }
        let required_terms = Self::cjk_relevance_terms(task)
            .into_iter()
            .filter(|term| term.chars().count() >= 2)
            .filter(|term| !Self::is_cjk_narrative_material_noise(term))
            .collect::<Vec<_>>();
        if required_terms.is_empty() {
            return true;
        }
        required_terms
            .iter()
            .any(|term| evidence.contains(term.as_str()))
    }

    fn is_cjk_narrative_material_noise(term: &str) -> bool {
        if Self::is_cjk_relevance_noise(term) {
            return true;
        }
        term.contains("小说")
            || term.contains("故事")
            || term.contains("正文")
            || term.contains("素材")
            || term.contains("热门")
            || term.contains("公网")
            || term.contains("下载")
            || term.contains("可下")
            || term.contains("读取")
            || term.contains("可读")
            || term.contains("免费")
            || term.contains("公开")
            || term.contains("联网")
    }

    pub(crate) fn task_requests_narrative_source_material(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        task.contains("小说")
            || task.contains("故事")
            || task.contains("长篇")
            || lowered.contains("novel")
            || lowered.contains("novels")
            || lowered.contains("story")
            || lowered.contains("stories")
            || lowered.contains("fiction")
    }

    fn narrative_source_signal_is_present(evidence_lower: &str) -> bool {
        [
            "novel",
            "story",
            "stories",
            "tale",
            "tales",
            "fiction",
            "chapter i",
            "chapter 1",
            "第一章",
            "楔子",
            "正文",
        ]
        .iter()
        .any(|marker| evidence_lower.contains(marker))
    }

    fn narrative_source_signal_is_strong(evidence_lower: &str) -> bool {
        [
            "a novel",
            "this novel",
            "science fiction novel",
            "fiction stories",
            "short stories",
            "chapter i",
            "chapter 1",
            "第一章",
            "正文",
        ]
        .iter()
        .any(|marker| evidence_lower.contains(marker))
    }

    fn narrative_genre_signal_matches_request(task: &str, evidence_lower: &str) -> bool {
        let lowered_task = task.to_ascii_lowercase();
        let mut requested_groups: Vec<(&[&str], &[&str])> = Vec::new();
        let requests_xuanhuan_specific = task.contains("玄幻")
            || lowered_task.contains("xuanhuan")
            || lowered_task.contains("eastern fantasy")
            || lowered_task.contains("chinese fantasy");
        if requests_xuanhuan_specific {
            requested_groups.push((
                &["玄幻", "xuanhuan", "eastern fantasy", "chinese fantasy"],
                &[],
            ));
        }
        if task.contains("奇幻")
            || (!requests_xuanhuan_specific && lowered_task.contains("fantasy"))
        {
            requested_groups.push((&["奇幻"], &["fantasy"]));
        }
        if task.contains("科幻")
            || task.contains("星际")
            || lowered_task.contains("science fiction")
            || lowered_task.contains("sci-fi")
            || lowered_task.contains("space opera")
        {
            requested_groups.push((
                &["科幻", "星际", "science fiction", "sci-fi", "space opera"],
                &[],
            ));
        }
        if task.contains("仙侠") || task.contains("修仙") || lowered_task.contains("xianxia") {
            requested_groups.push((&["仙侠", "修仙", "xianxia", "cultivation"], &[]));
        }
        if task.contains("言情") || lowered_task.contains("romance") {
            requested_groups.push((&["言情", "romance"], &[]));
        }
        if task.contains("悬疑")
            || task.contains("推理")
            || lowered_task.contains("mystery")
            || lowered_task.contains("thriller")
        {
            requested_groups.push((&["悬疑", "推理", "mystery", "thriller"], &[]));
        }
        if requested_groups.is_empty() {
            return true;
        }

        requested_groups.iter().any(|(exact_terms, broad_terms)| {
            let task_used_exact_term = exact_terms.iter().any(|term| {
                if term.is_ascii() {
                    lowered_task.contains(term)
                } else {
                    task.contains(term)
                }
            });
            if task_used_exact_term {
                exact_terms
                    .iter()
                    .any(|term| evidence_lower.contains(&term.to_lowercase()))
            } else {
                exact_terms
                    .iter()
                    .chain(broad_terms.iter())
                    .any(|term| evidence_lower.contains(&term.to_lowercase()))
            }
        })
    }

    pub(crate) fn fetch_payload_matches_lookup_intent(task: &str, fetch_payload: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(fetch_payload) else {
            return true;
        };
        let url = payload
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let content = payload
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let url_lower = url.to_ascii_lowercase();

        if url_lower.contains("/entrez/eutils/esummary.fcgi") {
            let Ok(body) = serde_json::from_str::<serde_json::Value>(content) else {
                return false;
            };
            let Some(records) = Self::pubmed_esummary_records(&body) else {
                return false;
            };
            return records
                .into_iter()
                .any(|record| Self::pubmed_esummary_record_matches_lookup_intent(task, record));
        }

        let evidence_lower = format!("{url}\n{content}").to_lowercase();
        Self::lookup_evidence_text_matches_intent(task, url, &evidence_lower, true)
    }

    pub(crate) fn lookup_evidence_text_matches_intent(
        task: &str,
        url: &str,
        evidence_lower: &str,
        allow_single_academic_record_match: bool,
    ) -> bool {
        let cjk_matches = Self::cjk_relevance_terms(task)
            .into_iter()
            .filter(|term| !Self::is_lookup_alignment_noise_term(term))
            .filter(|term| term.chars().count() >= 2)
            .any(|term| evidence_lower.contains(&term));
        if cjk_matches {
            return true;
        }

        let terms = Self::lookup_alignment_terms(task);
        if terms.is_empty() {
            return true;
        }

        let matched = terms
            .iter()
            .filter(|term| Self::lookup_alignment_term_matches(&evidence_lower, term))
            .count();
        let required = if terms.len() <= 2 { 1 } else { 2 };

        matched >= required
            || (allow_single_academic_record_match
                && Self::task_prefers_academic_sources(task)
                && Self::url_is_specific_academic_record(url)
                && matched >= 1)
    }

    pub(crate) fn lookup_alignment_terms(task: &str) -> Vec<String> {
        let intent = Self::build_lookup_query_intent(task);
        let source_terms = Self::structured_source_terms_from_site_hints(&intent)
            .into_iter()
            .map(|term| term.to_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let mut terms = Vec::new();

        let alignment_sources = [
            intent.base_terms,
            intent.artifact_hints,
            intent.evidence_hints,
            intent.direct_record_hints,
        ];
        for term in alignment_sources.into_iter().flatten() {
            let term = term
                .trim()
                .trim_matches(|ch: char| {
                    matches!(
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
                .to_lowercase();
            if term.is_empty()
                || term.starts_with("site:")
                || source_terms.contains(&term)
                || Self::is_lookup_noise_term(&term)
                || Self::is_lookup_alignment_noise_term(&term)
                || Self::is_lookup_temporal_hint_term(&term)
            {
                continue;
            }

            let char_count = term.chars().count();
            if (term.is_ascii() && char_count < 3) || (!term.is_ascii() && char_count < 2) {
                continue;
            }
            Self::push_unique(&mut terms, term);
            if terms.len() >= 10 {
                break;
            }
        }

        terms
    }

    pub(crate) fn is_lookup_alignment_noise_term(token: &str) -> bool {
        let lowered = token.trim().to_ascii_lowercase();
        matches!(
            lowered.as_str(),
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
        ) || matches!(
            token.trim(),
            "免费" | "全文" | "正文" | "公网" | "公开" | "可下载" | "下载" | "内容" | "入库"
        )
    }

    pub(crate) fn structured_discovery_result_can_seed_followup(
        url: &str,
        fetch_payload: &str,
    ) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(fetch_payload) else {
            return false;
        };
        let source_url = payload
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or(url);
        let content = payload
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        Self::is_structured_discovery_url(source_url)
            && !Self::fetch_result_contains_verification_challenge(content)
            && Self::structured_lookup_payload_has_results(source_url, content)
            && !Self::structured_lookup_followup_urls(fetch_payload, 1).is_empty()
    }

    pub(crate) fn lookup_alignment_term_matches(evidence_lower: &str, term: &str) -> bool {
        if evidence_lower.contains(term) {
            return true;
        }
        if term.is_ascii() {
            let stem = term
                .trim_end_matches("ies")
                .trim_end_matches("ing")
                .trim_end_matches("ed")
                .trim_end_matches('s');
            return stem.len() >= 4 && evidence_lower.contains(stem);
        }
        false
    }

    pub(crate) fn fetched_result_requires_more_evidence(fetch_payload: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(fetch_payload) else {
            return false;
        };
        let content_quality = payload
            .get("content_quality")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if matches!(content_quality, "empty" | "low_information" | "challenge") {
            return true;
        }
        if payload
            .get("orchestration_decision")
            .and_then(|value| value.get("can_finalize_answer"))
            .and_then(|value| value.as_bool())
            .is_some_and(|can_finalize| !can_finalize)
        {
            return true;
        }

        payload
            .get("verification_followup")
            .and_then(|value| value.get("answer_readiness"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_ascii_lowercase())
            .is_some_and(|value| {
                matches!(
                    value.as_str(),
                    "blocked"
                        | "challenge"
                        | "empty"
                        | "low_information"
                        | "not_ready"
                        | "needs_followup"
                        | "needs_more_evidence"
                        | "requires_followup"
                        | "verification_pending"
                )
            })
    }

    pub(crate) fn task_requires_verified_fetch_result(task: &str) -> bool {
        Self::task_requests_data_or_records(task)
            || Self::task_requests_collection_or_ranking(task)
            || Self::task_requires_structured_followup(task)
            || Self::task_requires_intent_aligned_source_material(task)
            || Self::task_requests_time_sensitive_lookup(task)
    }

    pub(crate) fn task_requests_time_sensitive_lookup(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        let ascii_markers = [
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
        ];
        if ascii_markers.iter().any(|marker| lowered.contains(marker)) {
            return true;
        }

        [
            "今天", "今日", "当前", "现在", "实时", "最新", "天气", "预报", "气温", "温度", "降雨",
            "下雨",
        ]
        .iter()
        .any(|marker| task.contains(marker))
    }

    pub(crate) fn task_requires_intent_aligned_source_material(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        let requests_durable_ingest = lowered.contains("knowledge base")
            || lowered.contains("knowledge-base")
            || lowered.contains("ingest")
            || lowered.contains("import")
            || lowered.contains("save")
            || task.contains("知识库")
            || task.contains("入库")
            || task.contains("导入")
            || task.contains("保存");
        let requests_downstream_artifact = lowered.contains("based on")
            || lowered.contains("from this")
            || lowered.contains("use this")
            || lowered.contains("write")
            || lowered.contains("create")
            || lowered.contains("generate")
            || task.contains("根据")
            || task.contains("基于")
            || task.contains("写")
            || task.contains("创作")
            || task.contains("生成");
        requests_durable_ingest || requests_downstream_artifact
    }

    pub(crate) fn knowledge_import_source_alignment_blocker(task: &str) -> Option<String> {
        if !Self::knowledge_import_requires_source_alignment_evidence(task) {
            return None;
        }

        let Some(evidence) = Self::knowledge_import_source_alignment_evidence(task) else {
            return Some(
                "knowledge import needs fetched source/body evidence aligned with the original material request before durable ingestion"
                    .to_string(),
            );
        };

        if Self::fetched_result_looks_usable_for_task(task, &evidence) {
            return None;
        }

        Some(
            Self::collection_intent_alignment_blocker(task, "", &evidence).unwrap_or_else(|| {
                "provided source evidence does not match the requested material well enough for durable ingestion"
                    .to_string()
            }),
        )
    }

    pub(crate) fn knowledge_import_requires_source_alignment_evidence(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        let carries_original_request =
            lowered.contains("original user request") || task.contains("完整用户请求");
        if !carries_original_request || !Self::task_requires_intent_aligned_source_material(task) {
            return false;
        }

        let requests_material = Self::task_requests_narrative_source_material(task)
            || lowered.contains("source material")
            || lowered.contains("full text")
            || lowered.contains("body text")
            || lowered.contains("downloadable")
            || task.contains("正文")
            || task.contains("素材")
            || task.contains("全文")
            || task.contains("可读取")
            || task.contains("可下载")
            || task.contains("下载");
        let uses_material_downstream = lowered.contains("based on")
            || lowered.contains("use this")
            || lowered.contains("write")
            || lowered.contains("create")
            || lowered.contains("generate")
            || task.contains("根据")
            || task.contains("基于")
            || task.contains("写")
            || task.contains("创作")
            || task.contains("生成");

        requests_material && uses_material_downstream
    }

    pub(crate) fn knowledge_import_source_alignment_evidence(task: &str) -> Option<String> {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(task) {
            for key in [
                "fetched_result",
                "source_body",
                "source_content",
                "body_evidence",
                "content",
                "result",
            ] {
                if let Some(value) = payload.get(key) {
                    let evidence = if let Some(text) = value.as_str() {
                        text.to_string()
                    } else {
                        value.to_string()
                    };
                    if !evidence.trim().is_empty() {
                        return Some(evidence);
                    }
                }
            }
        }

        let markers = [
            "fetched_result:",
            "source_body:",
            "source_content:",
            "body_evidence:",
            "content_evidence:",
        ];
        let lowered = task.to_ascii_lowercase();
        for marker in markers {
            if let Some(index) = lowered.find(marker) {
                let start = index + marker.len();
                let evidence = Self::trim_knowledge_import_evidence_tail(&task[start..]);
                if !evidence.trim().is_empty() {
                    return Some(evidence);
                }
            }
        }

        None
    }

    fn trim_knowledge_import_evidence_tail(evidence: &str) -> String {
        let terminators = [
            "\n\nsearch_result_preview:",
            "\n\nOriginal user request",
            "\n\n完整用户请求",
            "\n\n用户请求",
            "\n\nfull_user_request",
        ];
        let lower = evidence.to_ascii_lowercase();
        let mut end = evidence.len();
        for terminator in terminators {
            if let Some(index) = lower.find(&terminator.to_ascii_lowercase()) {
                end = end.min(index);
            }
        }
        evidence[..end].trim().to_string()
    }

    pub(crate) fn requested_collection_item_count(task: &str) -> usize {
        let lowered = task.to_ascii_lowercase();
        let patterns = [
            r"(?:前|top\s*|排名\s*前\s*|推荐\s*前\s*)(\d{1,2})",
            r"(\d{1,2})\s*(?:部|个|条|篇|本|项)",
        ];
        for pattern in patterns {
            let regex = Regex::new(pattern).expect("valid collection count regex");
            if let Some(count) = regex
                .captures(&lowered)
                .and_then(|captures| captures.get(1))
                .and_then(|value| value.as_str().parse::<usize>().ok())
                .filter(|count| (1..=50).contains(count))
            {
                return count;
            }
        }
        if task.contains("前十") || lowered.contains("top ten") {
            return 10;
        }
        3
    }

    pub(crate) fn fetched_result_collection_item_count(task: &str, fetch_payload: &str) -> usize {
        let payload = serde_json::from_str::<serde_json::Value>(fetch_payload).ok();
        let content = payload
            .as_ref()
            .and_then(|value| value.get("content"))
            .and_then(|value| value.as_str())
            .unwrap_or(fetch_payload);

        let mut item_keys = std::collections::HashSet::new();
        if let Some(links) = payload
            .as_ref()
            .and_then(|value| value.get("links"))
            .and_then(|value| value.as_array())
        {
            for link in links {
                let url = link
                    .get("url")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let text = link
                    .get("text")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                if Self::link_looks_like_collection_item(task, url, text) {
                    item_keys.insert(format!("link:{url}"));
                }
            }
        }

        let title_regexes = [
            r"《[^》]{2,60}》",
            r"(?m)^\s*(?:\d{1,2}[.、)]|第[一二三四五六七八九十0-9]{1,3}[名部本项条篇])\s*[^\n]{2,120}$",
        ];
        for pattern in title_regexes {
            let regex = Regex::new(pattern).expect("valid collection item regex");
            for matched in regex.find_iter(content) {
                let item = matched.as_str().trim();
                if !item.is_empty() {
                    item_keys.insert(format!("content:{item}"));
                }
            }
        }
        Self::collect_numbered_collection_items(task, content, &mut item_keys);

        item_keys.len()
    }

    pub(crate) fn compact_collection_fetch_summary(
        task: &str,
        source_url: &str,
        fetch_payload: &str,
    ) -> Option<String> {
        if !Self::task_requests_collection_or_ranking(task) {
            return None;
        }
        if Self::collection_intent_alignment_blocker(task, source_url, fetch_payload).is_some() {
            return None;
        }

        let payload = serde_json::from_str::<serde_json::Value>(fetch_payload).ok();
        let content = payload
            .as_ref()
            .and_then(|value| value.get("content"))
            .and_then(|value| value.as_str())
            .unwrap_or(fetch_payload);
        let requested = Self::requested_collection_item_count(task).max(1);
        let mut items = Self::extract_ranked_collection_items(task, source_url, content, requested);
        if items.is_empty() {
            items = Self::extract_collection_items_from_metadata_blocks(
                task, source_url, content, requested,
            );
        }
        if items.is_empty() {
            if let Some(links) = payload
                .as_ref()
                .and_then(|value| value.get("links"))
                .and_then(|value| value.as_array())
            {
                let mut seen = std::collections::HashSet::new();
                for link in links {
                    let url = link
                        .get("url")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let text = link
                        .get("text")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .trim();
                    if Self::link_looks_like_collection_item(task, url, text)
                        && Self::ranked_item_intent_blocker(task, url, text, None).is_none()
                        && seen.insert(text.to_string())
                    {
                        items.push((text.to_string(), None));
                        if items.len() >= requested {
                            break;
                        }
                    }
                }
            }
        }
        if items.is_empty() {
            return None;
        }

        let lines = items
            .into_iter()
            .enumerate()
            .map(|(index, (title, metadata))| {
                let metadata = metadata
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "metadata not visible in fetched source".to_string());
                format!(
                    "- {}. {} | public metadata: {} | source: {}",
                    index + 1,
                    title,
                    metadata,
                    source_url
                )
            })
            .collect::<Vec<_>>();

        let summary = lines.join("\n");
        if !Self::collection_summary_satisfies_task(task, &summary) {
            return None;
        }

        Some(summary)
    }

    pub(crate) fn collection_summary_satisfies_task(task: &str, summary: &str) -> bool {
        if Self::task_requests_collection_item_details(task)
            && Self::verified_ranked_metadata_count(summary)
                < Self::requested_collection_item_count(task).max(1)
        {
            return false;
        }
        true
    }

    pub(crate) fn collection_summary_ready_for_completion(task: &str, summary: &str) -> bool {
        if summary.trim().is_empty() {
            return false;
        }
        Self::ranked_summary_item_count(summary)
            >= Self::requested_collection_item_count(task).max(1)
            && Self::collection_summary_satisfies_task(task, summary)
    }

    pub(crate) fn task_requests_collection_item_details(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        lowered.contains("summary")
            || lowered.contains("summaries")
            || lowered.contains("metadata")
            || lowered.contains("plot")
            || lowered.contains("details")
            || task.contains("摘要")
            || task.contains("简介")
            || task.contains("梗概")
            || task.contains("情节")
            || task.contains("剧情")
            || task.contains("元数据")
            || task.contains("详情")
            || task.contains("内容")
    }

    pub(crate) fn ranked_summary_item_count(summary: &str) -> usize {
        summary
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with("- ") && trimmed.contains(" | public metadata: ")
            })
            .count()
    }

    pub(crate) fn extract_ranked_collection_items(
        task: &str,
        source_url: &str,
        content: &str,
        limit: usize,
    ) -> Vec<(String, Option<String>)> {
        let lines = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let rank_marker = Regex::new(
            r"^(?:[•·\-\s]*)?(?:(?:no\.?|NO\.?)\s*)?(?:\d{1,2}|第[一二三四五六七八九十0-9]{1,3})(?:[.、)]|名)?$",
        )
        .expect("valid numbered collection marker regex");
        let numeric_noise =
            Regex::new(r"^[0-9]+(?:月票|票|收藏|推荐|点击|阅读)?$").expect("valid metric regex");

        let mut items = Vec::<(String, Option<String>)>::new();
        let mut seen = std::collections::HashSet::<String>::new();

        for (index, line) in lines.iter().enumerate() {
            if !rank_marker.is_match(line) {
                continue;
            }

            let Some((title_offset, title)) =
                lines.iter().enumerate().skip(index + 1).take(5).find_map(
                    |(candidate_index, candidate)| {
                        Self::collection_title_candidate_looks_valid(task, candidate)
                            .then_some((candidate_index, (*candidate).to_string()))
                    },
                )
            else {
                continue;
            };

            if !seen.insert(title.clone()) {
                continue;
            }

            let metadata = lines
                .iter()
                .skip(title_offset + 1)
                .take(4)
                .find(|candidate| {
                    !rank_marker.is_match(candidate)
                        && !numeric_noise.is_match(candidate)
                        && !matches!(**candidate, "•" | "·" | "")
                        && candidate.chars().count() >= 2
                        && candidate.chars().count() <= 120
                        && Self::collection_metadata_candidate_looks_valid(candidate)
                })
                .map(|value| (*value).to_string());

            if Self::ranked_item_intent_blocker(task, source_url, &title, metadata.as_deref())
                .is_some()
            {
                continue;
            }

            items.push((title, metadata));
            if items.len() >= limit {
                break;
            }
        }

        items
    }

    pub(crate) fn extract_collection_items_from_metadata_blocks(
        task: &str,
        source_url: &str,
        content: &str,
        limit: usize,
    ) -> Vec<(String, Option<String>)> {
        let lines = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let mut items = Vec::<(String, Option<String>)>::new();
        let mut seen = std::collections::HashSet::<String>::new();

        for (index, title) in lines.iter().enumerate() {
            if !Self::collection_block_title_candidate_looks_valid(task, title) {
                continue;
            }

            let Some(metadata) = lines
                .get(index + 1)
                .filter(|candidate| Self::collection_metadata_candidate_looks_valid(candidate))
                .map(|value| (*value).to_string())
            else {
                continue;
            };

            if !seen.insert((*title).to_string()) {
                continue;
            }

            let metadata = Self::augment_collection_item_metadata_from_nearby_evidence(
                task,
                &lines[index..lines.len().min(index + 14)],
                &metadata,
            );
            if Self::ranked_item_intent_blocker(task, source_url, title, Some(&metadata)).is_some()
            {
                continue;
            }

            items.push(((*title).to_string(), Some(metadata)));
            if items.len() >= limit {
                break;
            }
        }

        items
    }

    pub(crate) fn collection_block_title_candidate_looks_valid(
        task: &str,
        candidate: &str,
    ) -> bool {
        let text = candidate.trim();
        if !Self::collection_title_candidate_looks_valid(task, text) {
            return false;
        }
        let lowered = text.to_ascii_lowercase();
        let structural_noise = SearchPolicy::collection_title_noise_terms_for_task(task);
        !structural_noise
            .iter()
            .any(|value| text.contains(value) || lowered.contains(&value.to_ascii_lowercase()))
    }

    pub(crate) fn augment_collection_item_metadata_from_nearby_evidence(
        task: &str,
        nearby_lines: &[&str],
        metadata: &str,
    ) -> String {
        let mut parts = vec![metadata.trim().to_string()];
        let mut joined_evidence = metadata.to_string();
        for facet in SearchPolicy::collection_intent_facets_for_task(task, true) {
            let already_present = facet.evidence_terms.iter().any(|term| {
                Self::text_contains(&joined_evidence, term)
                    || joined_evidence
                        .to_ascii_lowercase()
                        .contains(&term.to_ascii_lowercase())
            });
            if already_present {
                continue;
            }
            if let Some(line) = nearby_lines.iter().find(|line| {
                facet.evidence_terms.iter().any(|term| {
                    Self::text_contains(line, term)
                        || line
                            .to_ascii_lowercase()
                            .contains(&term.to_ascii_lowercase())
                })
            }) {
                let evidence = preview_text(line.trim(), 80);
                if !parts.iter().any(|part| part == &evidence) {
                    parts.push(evidence.clone());
                    joined_evidence.push('\n');
                    joined_evidence.push_str(&evidence);
                }
            }
        }

        preview_text(&parts.join(" / "), 260)
    }

    pub(crate) fn ranked_item_intent_blocker(
        task: &str,
        source_url: &str,
        title: &str,
        metadata: Option<&str>,
    ) -> Option<String> {
        let item_evidence = format!(
            "{}\n{}\n{}",
            source_url,
            title,
            metadata.unwrap_or_default()
        );
        for facet in SearchPolicy::collection_intent_facets_for_task(task, true) {
            if !Self::text_contains_any_owned(task, &facet.requested_by) {
                continue;
            }
            let lowered_item = item_evidence.to_ascii_lowercase();
            let has_matching_evidence =
                Self::text_contains_any_owned(&item_evidence, &facet.evidence_terms)
                    || Self::ascii_text_contains_any_owned(&lowered_item, &facet.evidence_terms);
            if facet.requires_evidence && !has_matching_evidence {
                return Some(format!(
                    "requested '{}' but ranked item evidence is missing",
                    facet.name
                ));
            }
            let conflicts = facet
                .conflicting_terms
                .iter()
                .filter(|term| {
                    Self::text_contains(&item_evidence, term)
                        || lowered_item.contains(&term.to_ascii_lowercase())
                })
                .cloned()
                .collect::<Vec<_>>();
            if !conflicts.is_empty() && !has_matching_evidence {
                return Some(format!(
                    "requested '{}' but ranked item evidence indicates '{}'",
                    facet.name,
                    conflicts.join("/")
                ));
            }
        }
        None
    }

    pub(crate) fn collection_metadata_candidate_looks_valid(candidate: &str) -> bool {
        candidate.contains('·')
            || candidate.contains('|')
            || candidate.contains("作者")
            || candidate.contains("分类")
            || candidate.contains("月票")
            || candidate.contains("推荐")
            || candidate.contains("收藏")
            || candidate.contains("销量")
            || candidate.to_ascii_lowercase().contains("author")
            || candidate.to_ascii_lowercase().contains("category")
    }

    pub(crate) fn collect_numbered_collection_items(
        task: &str,
        content: &str,
        item_keys: &mut std::collections::HashSet<String>,
    ) {
        let lines = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let rank_marker = Regex::new(
            r"^(?:[•·\-\s]*)?(?:(?:no\.?|NO\.?)\s*)?(?:\d{1,2}|第[一二三四五六七八九十0-9]{1,3})(?:[.、)]|名)?$",
        )
        .expect("valid numbered collection marker regex");

        for (index, line) in lines.iter().enumerate() {
            if !rank_marker.is_match(line) {
                continue;
            }
            for candidate in lines.iter().skip(index + 1).take(4) {
                if Self::collection_title_candidate_looks_valid(task, candidate) {
                    item_keys.insert(format!("ranked:{candidate}"));
                    break;
                }
            }
        }
    }

    pub(crate) fn collection_title_candidate_looks_valid(task: &str, candidate: &str) -> bool {
        let text = candidate.trim();
        let len = text.chars().count();
        if !(2..=80).contains(&len) {
            return false;
        }
        if Self::link_text_looks_like_navigation_noise(task, text)
            || Self::link_text_looks_like_filter_navigation(task, text)
        {
            return false;
        }
        if text.contains('|') || text.contains("://") {
            return false;
        }
        if Self::task_requests_book_collection(task) {
            return !text.ends_with("榜")
                && !text.ends_with("排行")
                && !text.ends_with("分类")
                && !text.contains("作品")
                && !text.contains("小说网");
        }
        true
    }

    pub(crate) fn link_looks_like_collection_item(task: &str, url: &str, text: &str) -> bool {
        let text = text.trim();
        if text.chars().count() < 2 || text.chars().count() > 100 {
            return false;
        }
        if Self::link_text_looks_like_filter_navigation(task, text) {
            return false;
        }
        if Self::link_text_looks_like_navigation_noise(task, text) {
            return false;
        }

        let Ok(parsed) = Url::parse(url) else {
            return false;
        };
        let path = parsed.path().trim_matches('/');
        let index_paths = SearchPolicy::collection_index_path_terms_for_task(task);
        if path.is_empty()
            || index_paths
                .iter()
                .any(|index_path| path.eq_ignore_ascii_case(index_path))
            || Self::url_path_looks_like_filter_navigation(task, path)
            || Self::url_path_looks_like_non_content_navigation(task, path)
        {
            return false;
        }
        let path_depth = path.split('/').filter(|part| !part.is_empty()).count();
        if Self::task_requests_book_collection(task) {
            let lowered_url = url.to_ascii_lowercase();
            return lowered_url.contains("/book/")
                || lowered_url.contains("/novel/")
                || text.contains('《');
        }
        if path_depth >= 2 {
            return true;
        }

        Self::candidate_score(task, url, text, text) > 0
            && !Self::candidate_looks_like_news_or_portal_index(url, text, text)
    }

    pub(crate) fn link_text_looks_like_navigation_noise(task: &str, text: &str) -> bool {
        let trimmed = text.trim();
        let lowered = trimmed.to_ascii_lowercase();
        SearchPolicy::navigation_noise_text_terms_for_task(task)
            .iter()
            .any(|term| {
                let term_lower = term.to_ascii_lowercase();
                trimmed == term
                    || lowered == term_lower
                    || (!term.trim().is_empty() && trimmed.starts_with(term))
            })
    }

    pub(crate) fn link_text_looks_like_filter_navigation(task: &str, text: &str) -> bool {
        let trimmed = text.trim();
        let lowered = trimmed.to_ascii_lowercase();
        SearchPolicy::filter_navigation_text_terms_for_task(task)
            .iter()
            .any(|term| trimmed == term || lowered == term.to_ascii_lowercase())
    }

    pub(crate) fn url_path_looks_like_filter_navigation(task: &str, path: &str) -> bool {
        let prefixes = SearchPolicy::filter_navigation_path_prefixes_for_task(task);
        path.split('/')
            .flat_map(|part| part.split('-'))
            .filter(|part| !part.is_empty())
            .any(|part| {
                let lowered = part.to_ascii_lowercase();
                prefixes
                    .iter()
                    .any(|prefix| lowered.starts_with(&prefix.to_ascii_lowercase()))
            })
    }

    pub(crate) fn url_looks_like_non_content_navigation(task: &str, url: &str) -> bool {
        Url::parse(url)
            .ok()
            .map(|parsed| {
                Self::url_path_looks_like_non_content_navigation(
                    task,
                    parsed.path().trim_matches('/'),
                )
            })
            .unwrap_or(false)
    }

    pub(crate) fn url_path_looks_like_non_content_navigation(task: &str, path: &str) -> bool {
        let prefixes = SearchPolicy::non_content_path_prefixes_for_task(task);
        path.split('/')
            .flat_map(|part| part.split('-'))
            .filter(|part| !part.is_empty())
            .any(|part| {
                let lowered = part.to_ascii_lowercase();
                prefixes
                    .iter()
                    .any(|prefix| lowered.starts_with(&prefix.to_ascii_lowercase()))
            })
    }

    pub(crate) fn link_is_filter_navigation_without_task_overlap(
        task: &str,
        text: &str,
        url: &str,
    ) -> bool {
        let Ok(parsed) = Url::parse(url) else {
            return false;
        };
        if !Self::url_path_looks_like_filter_navigation(task, parsed.path().trim_matches('/')) {
            return false;
        }
        let evidence = format!("{text}\n{url}");
        !Self::cjk_relevance_terms(task)
            .into_iter()
            .filter(|term| term.chars().count() >= 2)
            .any(|term| Self::text_contains(&evidence, &term))
    }

    pub(crate) fn task_requests_book_collection(task: &str) -> bool {
        !SearchPolicy::collection_intent_facets_for_task(task, true).is_empty()
    }

    pub(crate) fn fetched_result_has_record_values(fetch_payload: &str) -> bool {
        let content = serde_json::from_str::<serde_json::Value>(fetch_payload)
            .ok()
            .and_then(|payload| {
                payload
                    .get("content")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| fetch_payload.to_string());
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return false;
        }

        let issue_count = Regex::new(r"(?:第\s*)?20\d{4,}\s*期|\b20\d{4,}\b")
            .expect("valid issue regex")
            .find_iter(trimmed)
            .count();
        let date_count = Regex::new(r"20\d{2}[-年/.]\d{1,2}[-月/.]\d{1,2}")
            .expect("valid date regex")
            .find_iter(trimmed)
            .count();
        let number_row_count = Regex::new(r"(?:\b\d{1,2}\b[\s,，、+|]+){4,}\b\d{1,2}\b")
            .expect("valid number row regex")
            .find_iter(trimmed)
            .count();
        let structured_key_hits = [
            "\"issue\"",
            "\"drawdate\"",
            "\"drawDate\"",
            "\"draw_num\"",
            "\"red\"",
            "\"blue\"",
            "开奖日期",
            "开奖号码",
            "红球",
            "蓝球",
        ]
        .iter()
        .filter(|needle| trimmed.contains(**needle))
        .count();

        (issue_count >= 2 || date_count >= 2) && (number_row_count >= 1 || structured_key_hits >= 3)
    }

    pub(crate) fn fetched_result_blocker(fetch_payload: &str) -> Option<String> {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(fetch_payload) else {
            let trimmed = fetch_payload.trim();
            if trimmed.is_empty() {
                return Some("source returned empty content".to_string());
            }
            if Self::fetch_result_contains_verification_challenge(trimmed) {
                return Some("source returned a verification/challenge page".to_string());
            }
            return None;
        };

        let url = payload
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let content = payload
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or(fetch_payload);
        let content_quality = payload
            .get("content_quality")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        if Self::fetch_result_contains_verification_challenge(content) {
            return Some(format!(
                "source returned verification/challenge content: {}",
                url
            ));
        }
        if Self::fetch_result_is_low_information(url, content) {
            return Some(format!(
                "source returned low-information content (quality={}): {}",
                if content_quality.is_empty() {
                    "unknown"
                } else {
                    content_quality
                },
                url
            ));
        }
        if !Self::structured_lookup_payload_has_results(url, content) {
            return Some(format!(
                "structured source returned zero usable records: {}",
                url
            ));
        }

        None
    }

    pub(crate) fn collection_intent_alignment_blocker(
        task: &str,
        source_url: &str,
        fetch_payload: &str,
    ) -> Option<String> {
        if !Self::task_requests_collection_or_ranking(task) {
            return None;
        }
        let evidence = Self::collection_alignment_evidence(source_url, fetch_payload);
        Self::collection_intent_alignment_blocker_for_evidence(task, &evidence)
    }

    pub(crate) fn collection_intent_alignment_blocker_for_evidence(
        task: &str,
        evidence: &str,
    ) -> Option<String> {
        if !Self::task_requests_collection_or_ranking(task) {
            return None;
        }
        let lowered_evidence = evidence.to_ascii_lowercase();
        let mut mismatches = Vec::new();
        for facet in SearchPolicy::collection_intent_facets_for_task(task, false) {
            if !Self::text_contains_any_owned(task, &facet.requested_by) {
                continue;
            }
            let has_matching_evidence =
                Self::text_contains_any_owned(evidence, &facet.evidence_terms)
                    || Self::ascii_text_contains_any_owned(
                        &lowered_evidence,
                        &facet.evidence_terms,
                    );
            let conflicts = facet
                .conflicting_terms
                .iter()
                .filter(|term| {
                    Self::text_contains(evidence, term)
                        || lowered_evidence.contains(&term.to_ascii_lowercase())
                })
                .cloned()
                .collect::<Vec<_>>();
            if !conflicts.is_empty() && !has_matching_evidence {
                mismatches.push(format!(
                    "requested '{}' but source evidence indicates '{}'",
                    facet.name,
                    conflicts.join("/")
                ));
            }
        }
        (!mismatches.is_empty())
            .then(|| format!("source intent mismatch: {}", mismatches.join("; ")))
    }

    pub(crate) fn collection_alignment_evidence(source_url: &str, fetch_payload: &str) -> String {
        let mut evidence = String::new();
        if !source_url.trim().is_empty() {
            evidence.push_str(source_url);
            evidence.push('\n');
        }
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(fetch_payload) {
            Self::push_json_strings_for_alignment(&payload, &mut evidence);
        } else {
            evidence.push_str(fetch_payload);
        }
        evidence
    }

    pub(crate) fn push_json_strings_for_alignment(value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::String(text) => {
                out.push_str(text);
                out.push('\n');
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    Self::push_json_strings_for_alignment(value, out);
                }
            }
            serde_json::Value::Object(map) => {
                for value in map.values() {
                    Self::push_json_strings_for_alignment(value, out);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn text_contains_any_owned(text: &str, terms: &[String]) -> bool {
        terms.iter().any(|term| Self::text_contains(text, term))
    }

    pub(crate) fn ascii_text_contains_any_owned(lowered_text: &str, terms: &[String]) -> bool {
        terms
            .iter()
            .filter(|term| term.is_ascii())
            .any(|term| lowered_text.contains(&term.to_ascii_lowercase()))
    }

    pub(crate) fn text_contains_any(text: &str, terms: &[&str]) -> bool {
        terms.iter().any(|term| Self::text_contains(text, term))
    }

    pub(crate) fn ascii_text_contains_any(lowered_text: &str, terms: &[&str]) -> bool {
        terms
            .iter()
            .filter(|term| term.is_ascii())
            .any(|term| lowered_text.contains(&term.to_ascii_lowercase()))
    }

    pub(crate) fn text_contains(text: &str, term: &str) -> bool {
        if term.is_ascii() {
            text.to_ascii_lowercase()
                .contains(&term.to_ascii_lowercase())
        } else {
            text.contains(term)
        }
    }

    pub(crate) fn compact_search_result_for_tool_output(search_result: &str) -> String {
        preview_text(search_result, 1_200)
    }

    pub(crate) fn format_evidence_gap_blocker(
        worker: &str,
        executed_tool: &str,
        lookup_strategy: &str,
        query_or_source: Option<&str>,
        observed: usize,
        requested: usize,
        evidence_scope: &str,
        result_summary: Option<&str>,
        diagnostic_preview: Option<&str>,
    ) -> String {
        let mut output = format!(
            "status: blocked\nworker: {worker}\nexecuted_tool: {executed_tool}\nlookup_strategy: {lookup_strategy}\nblocker_contract: goal_not_satisfied\nobserved_item_records: {observed}\nrequested_item_records: {requested}\nmissing_item_records: {}\nevidence_scope: {evidence_scope}\nnext_action_policy: infer_from_original_goal_and_observed_evidence\n",
            requested.saturating_sub(observed)
        );
        if let Some(value) = query_or_source.filter(|value| !value.trim().is_empty()) {
            output.push_str(&format!("evidence_locator: {}\n", value.trim()));
        }
        if let Some(summary) = result_summary.filter(|summary| !summary.trim().is_empty()) {
            output.push_str("result_summary:\n");
            output.push_str(summary.trim());
            output.push_str("\n\n");
        }
        if let Some(preview) = diagnostic_preview.filter(|preview| !preview.trim().is_empty()) {
            output.push_str("diagnostic_preview:\n");
            output.push_str(preview.trim());
        }
        output
    }

    pub(crate) fn task_requests_full_source_content(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        lowered.contains("full text")
            || lowered.contains("entire article")
            || lowered.contains("complete content")
            || task.contains("全文")
            || task.contains("完整内容")
            || task.contains("正文内容")
            || task.contains("小说内容")
            || task.contains("章节内容")
    }

    pub(crate) fn task_allows_public_metadata_surrogate(task: &str) -> bool {
        if !Self::task_requests_full_source_content(task) {
            return false;
        }
        let lowered = task.to_ascii_lowercase();
        lowered.contains("write")
            || lowered.contains("generate")
            || lowered.contains("create")
            || lowered.contains("synthesize")
            || lowered.contains("reason")
            || lowered.contains("analyze")
            || lowered.contains("novel")
            || task.contains("写")
            || task.contains("创作")
            || task.contains("创造")
            || task.contains("生成")
            || task.contains("推理")
            || task.contains("分析")
    }

    pub(crate) fn format_search_index_collection_completion(
        task: &str,
        search_query: Option<&str>,
        search_result: &str,
    ) -> Option<String> {
        if !Self::task_requests_collection_or_ranking(task) {
            return None;
        }
        let (observed, summary) = Self::search_index_collection_summary(task, search_result)?;
        let requested = Self::requested_collection_item_count(task);
        let query_line = search_query
            .filter(|query| !query.trim().is_empty())
            .map(|query| format!("search_query: {query}\n"))
            .unwrap_or_default();

        if Self::task_requests_full_source_content(task) {
            if Self::task_allows_public_metadata_surrogate(task) && observed >= requested {
                return Some(format!(
                    "status: completed\nworker: researcher\nexecuted_tool: web_search\nlookup_strategy: search_index_evidence_fallback\n{query_line}observed_item_records: {observed}\nrequested_item_records: {requested}\nevidence_scope: public_metadata_surrogate_not_full_source_content\ncontent_policy_note: full source content was not imported; use public metadata/summaries only for downstream transformative reasoning\nresult_summary:\n{summary}\n\nsearch_result_preview:\n{}",
                    Self::compact_search_result_for_tool_output(search_result)
                ));
            }
            let mut blocked = Self::format_evidence_gap_blocker(
                "researcher",
                "web_search",
                "search_index_evidence_fallback",
                search_query,
                observed,
                requested,
                "search_index_metadata_not_page_content",
                Some(&summary),
                Some(&Self::compact_search_result_for_tool_output(search_result)),
            );
            blocked.push_str(
                "\ncontent_policy_note: search index evidence only provides public metadata; not importable full content",
            );
            return Some(blocked);
        }

        let status = if observed >= requested {
            "completed"
        } else {
            "blocked"
        };
        if status == "blocked" {
            return Some(Self::format_evidence_gap_blocker(
                "researcher",
                "web_search",
                "search_index_evidence_fallback",
                search_query,
                observed,
                requested,
                "search_index_metadata_not_page_content",
                Some(&summary),
                Some(&Self::compact_search_result_for_tool_output(search_result)),
            ));
        }
        Some(format!(
            "status: {status}\nworker: researcher\nexecuted_tool: web_search\nlookup_strategy: search_index_evidence_fallback\n{query_line}observed_item_records: {observed}\nrequested_item_records: {requested}\nevidence_scope: search_index_metadata_not_page_content\nresult_summary:\n{summary}\n\nsearch_result_preview:\n{}",
            Self::compact_search_result_for_tool_output(search_result)
        ))
    }

    pub(crate) fn search_index_collection_summary(
        task: &str,
        search_result: &str,
    ) -> Option<(usize, String)> {
        let payload = serde_json::from_str::<serde_json::Value>(search_result).ok()?;
        let results = payload.get("results").and_then(|value| value.as_array())?;
        let requested = Self::requested_collection_item_count(task).max(1);
        let mut seen = std::collections::HashSet::new();
        let mut lines = Vec::new();

        for entry in results {
            let url = entry
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            let title = entry
                .get("title")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            let snippet = entry
                .get("snippet")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            if url.is_empty() && title.is_empty() && snippet.is_empty() {
                continue;
            }
            let evidence = format!("{url}\n{title}\n{snippet}");
            if Self::collection_intent_alignment_blocker_for_evidence(task, &evidence).is_some() {
                continue;
            }

            for (item_title, metadata) in
                Self::extract_ranked_collection_items(task, url, &evidence, requested)
            {
                let key = item_title.to_ascii_lowercase();
                if seen.insert(key) {
                    lines.push(Self::format_search_index_item_line(
                        lines.len() + 1,
                        &item_title,
                        metadata.as_deref().unwrap_or(snippet),
                        url,
                    ));
                }
                if lines.len() >= requested {
                    break;
                }
            }
            if lines.len() >= requested {
                break;
            }

            if Self::search_result_entry_looks_like_collection_item(task, url, title, snippet) {
                let key = if !url.is_empty() {
                    url.to_ascii_lowercase()
                } else {
                    title.to_ascii_lowercase()
                };
                if seen.insert(key) {
                    lines.push(Self::format_search_index_item_line(
                        lines.len() + 1,
                        title,
                        snippet,
                        url,
                    ));
                }
            }
            if lines.len() >= requested {
                break;
            }
        }

        (!lines.is_empty()).then(|| (lines.len(), lines.join("\n")))
    }

    pub(crate) fn format_search_index_item_line(
        index: usize,
        title: &str,
        metadata: &str,
        url: &str,
    ) -> String {
        let metadata = metadata
            .trim()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let metadata = if metadata.is_empty() {
            "metadata not visible in search index".to_string()
        } else {
            ellipsize(&metadata, 260)
        };
        format!(
            "- {index}. {title} | public metadata: {metadata} | source: {url} | provenance: search_index"
        )
    }

    pub(crate) fn search_result_entry_looks_like_collection_item(
        task: &str,
        url: &str,
        title: &str,
        snippet: &str,
    ) -> bool {
        if title.trim().is_empty() {
            return false;
        }
        if Self::search_result_title_looks_like_collection_page(title) {
            return false;
        }
        if !Self::link_looks_like_collection_item(task, url, title) {
            return false;
        }
        let score = Self::candidate_score(task, url, title, snippet);
        score > 0 || Self::candidate_has_minimal_subject_overlap(task, url, title, snippet)
    }

    pub(crate) fn search_result_title_looks_like_collection_page(title: &str) -> bool {
        let lowered = title.to_ascii_lowercase();
        [
            "排行榜",
            "排行",
            "榜单",
            "大全",
            "列表",
            "推荐",
            "分类",
            "搜索",
            "rank",
            "ranking",
            "list",
            "directory",
            "search",
            "category",
        ]
        .iter()
        .any(|term| {
            if term.is_ascii() {
                lowered.contains(term)
            } else {
                title.contains(term)
            }
        })
    }

    pub(crate) fn format_research_fetch_completion(
        task: &str,
        source_url: &str,
        search_query: Option<&str>,
        lookup_strategy: Option<&str>,
        search_result: &str,
        fetched_result: &str,
    ) -> String {
        let fetched_result_is_intent_aligned =
            Self::fetched_result_looks_usable_for_task(task, fetched_result);
        let narrative_material_mismatch = Self::task_requests_narrative_source_material(task)
            && !Self::fetch_payload_matches_requested_material_type(task, fetched_result);
        let status = if Self::fetched_result_requires_more_evidence(fetched_result)
            || (Self::task_requires_verified_fetch_result(task)
                && !fetched_result_is_intent_aligned)
            || narrative_material_mismatch
        {
            "blocked"
        } else {
            "completed"
        };
        let mut header = format!(
            "status: {status}\nworker: researcher\nexecuted_tool: web_fetch\nsource_url: {source_url}\n"
        );
        if let Some(query) = search_query.filter(|query| !query.trim().is_empty()) {
            header.push_str(&format!("search_query: {query}\n"));
        }
        if let Some(strategy) = lookup_strategy.filter(|strategy| !strategy.trim().is_empty()) {
            header.push_str(&format!("lookup_strategy: {strategy}\n"));
        }
        if status == "blocked" {
            let blocker =
                Self::collection_intent_alignment_blocker(task, source_url, fetched_result)
                    .unwrap_or_else(|| {
                        Self::fetched_result_blocker(fetched_result).unwrap_or_else(|| {
                            "fetched source did not provide enough verified evidence to answer"
                                .to_string()
                        })
                    });
            header.push_str(&format!("blockers: {blocker}\n"));
        }
        let result_summary =
            Self::compact_collection_fetch_summary(task, source_url, fetched_result)
                .map(|summary| format!("result_summary:\n{summary}\n\n"))
                .unwrap_or_default();
        format!(
            "{header}{result_summary}fetched_result:\n{}\n\nsearch_result_preview:\n{}",
            fetched_result,
            Self::compact_search_result_for_tool_output(search_result)
        )
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_search_result_payload(
        query: &str,
        engine: &str,
        search_url: &str,
        results: &[BrowserSearchResult],
    ) -> String {
        let results = results
            .iter()
            .map(|result| {
                json!({
                    "title": result.title.clone(),
                    "url": result.url.clone(),
                    "snippet": result.snippet.clone(),
                    "source": result.source.clone(),
                    "position": result.position,
                })
            })
            .collect::<Vec<_>>();

        json!({
            "query": query,
            "engine": engine,
            "search_url": search_url,
            "results": results,
        })
        .to_string()
    }

    #[cfg(feature = "browser")]
    pub(crate) fn retag_browser_collection_completion(completion: String) -> String {
        completion
            .replacen("worker: researcher", "worker: browser", 1)
            .replacen(
                "executed_tool: web_search",
                "executed_tool: browser_browse",
                1,
            )
    }

    #[cfg(feature = "browser")]
    pub(crate) fn format_browser_snapshot_completion(
        task: &str,
        source_url: &str,
        search_query: &str,
        search_result: &str,
        snapshot: &str,
    ) -> String {
        let snapshot_payload = serde_json::json!({
            "url": source_url,
            "content": snapshot,
            "content_quality": "actionable",
            "orchestration_decision": {
                "can_finalize_answer": true
            },
            "verification_followup": {
                "answer_readiness": "source_content_observed"
            }
        })
        .to_string();
        let result_summary =
            Self::compact_collection_fetch_summary(task, source_url, &snapshot_payload)
                .map(|summary| format!("result_summary:\n{summary}\n\n"))
                .unwrap_or_default();
        let status = if Self::task_requires_verified_fetch_result(task)
            && !Self::collection_summary_ready_for_completion(task, &result_summary)
        {
            "blocked"
        } else {
            "completed"
        };
        let blocker = if status == "blocked" {
            let reason =
                Self::collection_intent_alignment_blocker(task, source_url, &snapshot_payload)
                    .unwrap_or_else(|| {
                        "browser page did not expose enough item-level public metadata".to_string()
                    });
            format!("blockers: {reason}\n")
        } else {
            String::new()
        };

        format!(
            "status: {status}\nworker: browser\nexecuted_tool: browser_browse\nlookup_strategy: browser_search_snapshot_followup\nsource_url: {source_url}\nsearch_query: {search_query}\n{blocker}{result_summary}result:\n{snapshot}\n\nsearch_result_preview:\n{}",
            Self::compact_search_result_for_tool_output(search_result)
        )
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_site_seed_urls(task: &str) -> Vec<String> {
        SearchPolicy::browser_site_seed_urls_for_task(task)
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_site_seed_index_paths(task: &str) -> Vec<String> {
        SearchPolicy::browser_site_seed_index_paths_for_task(task)
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_site_seed_path_score(task: &str, path: &str) -> i32 {
        let lowered_task = task.to_ascii_lowercase();
        let lowered_path = path.to_ascii_lowercase();
        let mut score = 0;
        if lowered_path.len() >= 3 && lowered_task.contains(&lowered_path) {
            score += 30;
        }
        for facet in SearchPolicy::collection_intent_facets_for_task(task, true) {
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

    #[cfg(feature = "browser")]
    pub(crate) fn browser_lookup_query_variants(task: &str) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut queries = Vec::new();
        queries.extend(Self::lookup_query_variants(task));
        let browser_focused = BrowserTool::search_query_with_task_context("", Some(task));
        if !browser_focused.trim().is_empty() {
            queries.push(browser_focused);
        }
        queries
            .into_iter()
            .map(|query| Self::strip_static_only_site_filters_for_browser(&query))
            .map(|query| Self::clean_browser_lookup_query(&query))
            .map(|query| query.trim().to_string())
            .filter(|query| !query.is_empty())
            .filter(|query| seen.insert(query.to_ascii_lowercase()))
            .collect()
    }

    #[cfg(feature = "browser")]
    pub(crate) fn clean_browser_lookup_query(query: &str) -> String {
        let mut terms = Vec::new();
        for token in query.split_whitespace() {
            let mut token = token.trim().to_string();
            if token.is_empty() || Self::is_lookup_query_noise_term(&token) {
                continue;
            }
            if token
                .chars()
                .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
            {
                if token.chars().count() > 4 {
                    token = Self::clean_cjk_subject_term(&token);
                }
                if token.is_empty()
                    || Self::is_lookup_query_noise_term(&token)
                    || Self::is_cjk_lookup_subject_stage_noise(&token)
                {
                    continue;
                }
            }
            Self::push_unique(&mut terms, token);
        }
        terms.join(" ")
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_blocker_query(task: &str) -> String {
        Self::browser_lookup_query_variants(task)
            .into_iter()
            .next()
            .unwrap_or_else(|| {
                Self::strip_static_only_site_filters_for_browser(&Self::compact_lookup_query(task))
            })
    }

    #[cfg(feature = "browser")]
    pub(crate) fn strip_static_only_site_filters_for_browser(query: &str) -> String {
        let mut kept_site_filter = false;
        query
            .split_whitespace()
            .filter(|token| {
                let Some(host) = token
                    .strip_prefix("site:")
                    .or_else(|| token.strip_prefix("SITE:"))
                else {
                    return true;
                };
                if matches!(policy_for_host(host).mode, SiteFetchMode::StaticOnly) {
                    return false;
                }
                if kept_site_filter {
                    return false;
                }
                kept_site_filter = true;
                true
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_seed_depth(url: &str) -> usize {
        Url::parse(url)
            .ok()
            .map(|parsed| {
                if parsed.path() == "/" || parsed.path().trim().is_empty() {
                    0
                } else {
                    1
                }
            })
            .unwrap_or(0)
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_site_link_score(
        task: &str,
        current_url: &str,
        text: &str,
        url: &str,
    ) -> i32 {
        let evidence = format!("{current_url}\n{url}\n{text}");
        let link_is_item = Self::link_looks_like_collection_item(task, url, text);
        if !link_is_item && !Self::link_has_task_relevance_overlap(task, &evidence) {
            return 0;
        }

        let mut score = Self::candidate_score(task, url, text, text);
        if link_is_item {
            score += 24;
        }

        for term in Self::cjk_relevance_terms(task) {
            if term.chars().count() >= 2 && evidence.contains(&term) {
                score += 12;
            }
        }

        score
    }

    #[cfg(feature = "browser")]
    pub(crate) fn link_has_task_relevance_overlap(task: &str, evidence: &str) -> bool {
        let lowered_evidence = evidence.to_ascii_lowercase();
        let intent = Self::build_lookup_intent(task);
        let mut terms = intent
            .base_terms
            .iter()
            .filter(|term| !Self::is_lookup_noise_term(term))
            .filter(|term| term.chars().count() > 1)
            .cloned()
            .collect::<Vec<_>>();
        for term in Self::cjk_relevance_terms(task) {
            Self::push_unique(&mut terms, term);
        }
        terms.into_iter().any(|term| {
            if term.is_ascii() {
                let lowered = term.to_ascii_lowercase();
                lowered.len() >= 4 && lowered_evidence.contains(&lowered)
            } else {
                evidence.contains(&term)
            }
        })
    }

    #[cfg(feature = "browser")]
    pub(crate) async fn try_browser_direct_site_exploration(
        task: &str,
    ) -> anyhow::Result<Option<String>> {
        let mut queue = Self::browser_site_seed_urls(task)
            .into_iter()
            .map(|url| {
                let depth = Self::browser_seed_depth(&url);
                (url, depth)
            })
            .collect::<std::collections::VecDeque<_>>();
        if queue.is_empty() {
            return Ok(None);
        }

        let browser = BrowserTool::new(None, None, Self::empty_sensory_hub());
        let mut visited = std::collections::HashSet::new();
        let mut last_blocker = None;
        let mut inspected = 0usize;
        let started_at = std::time::Instant::now();
        let max_pages = SearchPolicy::browser_direct_site_max_pages_for_task(task);
        let observation_budget = std::time::Duration::from_secs(
            SearchPolicy::browser_direct_site_budget_secs_for_task(task),
        );
        let per_attempt_timeout = std::time::Duration::from_secs(
            SearchPolicy::browser_direct_site_attempt_timeout_secs_for_task(task),
        );
        let deadline = started_at + observation_budget;

        while let Some((url, depth)) = queue.pop_front() {
            if inspected >= max_pages || depth > 3 || !visited.insert(url.clone()) {
                continue;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining < std::time::Duration::from_millis(500) {
                last_blocker = Some(format!(
                    "browser direct site exploration reached its per-attempt observation budget after inspecting {inspected} pages"
                ));
                break;
            }
            inspected += 1;
            let attempt_timeout = remaining.min(per_attempt_timeout);
            let links_payload = match tokio::time::timeout(
                attempt_timeout,
                browser.call(
                    &json!({
                        "action": "extract_links",
                        "url": url,
                        "structured": true,
                        "compact": true,
                        "wait_until": "domcontentloaded"
                    })
                    .to_string(),
                ),
            )
            .await
            {
                Ok(Ok(payload)) => payload,
                Ok(Err(error)) => {
                    last_blocker =
                        Some(format!("browser link extraction failed for {url}: {error}"));
                    continue;
                }
                Err(_) => {
                    last_blocker = Some(format!("browser link extraction timed out for {url}"));
                    continue;
                }
            };

            let parsed = serde_json::from_str::<serde_json::Value>(&links_payload).ok();
            if let Some(completion) =
                Self::try_browser_payload_record_collection(task, &url, parsed.as_ref())
            {
                return Ok(Some(completion));
            }
            if let Some(completion) =
                Self::try_browser_payload_link_collection(task, &url, parsed.as_ref())
            {
                return Ok(Some(completion));
            }
            let links = Self::browser_payload_links(parsed.as_ref());
            if let Some(completion) =
                Self::try_browser_item_detail_collection(task, &url, &links, Some(deadline)).await?
            {
                return Ok(Some(completion));
            }
            let mut ranked_links = links
                .into_iter()
                .filter_map(|link| {
                    let text = link.get("text")?.as_str()?.trim().to_string();
                    let next_url = link.get("url")?.as_str()?.trim().to_string();
                    if text.is_empty() || next_url.is_empty() {
                        return None;
                    }
                    if Self::link_is_filter_navigation_without_task_overlap(task, &text, &next_url)
                        || Self::url_looks_like_non_content_navigation(task, &next_url)
                    {
                        return None;
                    }
                    let score = Self::browser_site_link_score(task, &url, &text, &next_url);
                    (score > 0).then_some((score, next_url))
                })
                .collect::<Vec<_>>();
            ranked_links
                .sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

            let mut next_urls = Vec::new();
            for (_, next_url) in ranked_links.into_iter().take(12) {
                Self::push_unique(&mut next_urls, next_url);
            }
            for next_url in next_urls.into_iter().take(max_pages).rev() {
                if !visited.contains(&next_url) {
                    queue.push_front((next_url, depth + 1));
                }
            }
        }

        Ok(last_blocker.map(|blocker| {
            format!(
                "status: blocked\nworker: browser\nexecuted_tool: browser_browse\nlookup_strategy: direct_site_navigation\nblockers: {blocker}\nquery: {}",
                Self::browser_blocker_query(task)
            )
        }))
    }

    #[cfg(feature = "browser")]
    pub(crate) async fn try_browser_lookup(task: &str) -> anyhow::Result<Option<String>> {
        let direct_site_blocker = match Self::try_browser_direct_site_exploration(task).await {
            Ok(Some(result)) => {
                if !Self::looks_like_worker_blocker_status(&result) {
                    return Ok(Some(result));
                }
                Some(result)
            }
            Ok(None) => None,
            Err(error) => Some(format!("browser direct site exploration failed: {error}")),
        };

        let queries = Self::browser_lookup_query_variants(task);
        let max_results = if Self::task_requests_collection_or_ranking(task) {
            Self::requested_collection_item_count(task).clamp(5, 10)
        } else {
            5
        };
        let mut last_blocker = None;

        for query in queries.into_iter().take(3) {
            let (engine, search_url, results) = match tokio::time::timeout(
                std::time::Duration::from_secs(45),
                BrowserTool::search_once(&query, Some("auto"), max_results),
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => {
                    last_blocker = Some(format!("browser search failed for {query}: {error}"));
                    continue;
                }
                Err(_) => {
                    last_blocker = Some(format!("browser search timed out for {query}"));
                    continue;
                }
            };
            let search_result =
                Self::browser_search_result_payload(&query, &engine, &search_url, &results);

            for url in Self::best_followup_fetch_urls(
                task,
                &search_result,
                Self::followup_fetch_limit_for_task(task).clamp(1, 3),
            ) {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(45),
                    BrowserTool::snapshot_once(&url, false, true),
                )
                .await
                {
                    Ok(Ok(snapshot)) => {
                        let completion = Self::format_browser_snapshot_completion(
                            task,
                            &url,
                            &query,
                            &search_result,
                            &snapshot,
                        );
                        if !Self::looks_like_worker_blocker_status(&completion) {
                            return Ok(Some(completion));
                        }
                        last_blocker = Some(completion);
                    }
                    Ok(Err(error)) => {
                        last_blocker = Some(format!("browser snapshot failed for {url}: {error}"));
                    }
                    Err(_) => {
                        last_blocker = Some(format!("browser snapshot timed out for {url}"));
                    }
                }
            }

            if let Some(completion) =
                Self::format_search_index_collection_completion(task, Some(&query), &search_result)
            {
                return Ok(Some(Self::retag_browser_collection_completion(completion)));
            }

            if !results.is_empty() {
                if Self::task_requests_collection_or_ranking(task) {
                    last_blocker = Some(Self::format_evidence_gap_blocker(
                        "browser",
                        "browser_browse",
                        "browser_search",
                        Some(&query),
                        0,
                        Self::requested_collection_item_count(task),
                        "search_results_without_item_level_records",
                        None,
                        Some(&Self::compact_search_result_for_tool_output(&search_result)),
                    ));
                    continue;
                }
                return Ok(Some(format!(
                    "status: completed\nworker: browser\nexecuted_tool: browser_browse\nlookup_strategy: browser_search\nsource_url: {search_url}\nsearch_query: {query}\nresult:\n{search_result}"
                )));
            }
        }

        Ok(Some(format!(
            "status: blocked\nworker: browser\nexecuted_tool: browser_browse\nblockers: {}{}{}\nquery: {}",
            last_blocker.unwrap_or_else(|| {
                "browser search did not return usable observable results".to_string()
            }),
            if direct_site_blocker.is_some() {
                "\ndirect_site_attempt:\n"
            } else {
                ""
            },
            direct_site_blocker.unwrap_or_default(),
            Self::browser_blocker_query(task)
        )))
    }

    #[cfg(feature = "browser")]
    pub(crate) async fn try_browser_lookup_for_worker(
        task: &str,
        worker_name: &str,
    ) -> anyhow::Result<Option<String>> {
        let Some(result) = Self::try_browser_lookup(task).await? else {
            return Ok(None);
        };
        if worker_name == "browser" {
            return Ok(Some(result));
        }
        Ok(Some(result.replacen(
            "worker: browser",
            &format!("worker: {worker_name}"),
            1,
        )))
    }

    #[cfg(feature = "browser")]
    pub(crate) fn try_browser_payload_link_collection(
        task: &str,
        source_url: &str,
        payload: Option<&serde_json::Value>,
    ) -> Option<String> {
        if !Self::task_requests_collection_or_ranking(task) {
            return None;
        }
        let requested = Self::requested_collection_item_count(task).clamp(1, 10);
        let links = Self::browser_payload_links(payload);
        if links.is_empty() {
            return None;
        }

        let page_metadata = Self::browser_collection_page_metadata(task, source_url, payload);
        let mut candidates = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for link in links {
            let text = link
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            let url = link
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            if text.is_empty() || url.is_empty() {
                continue;
            }
            if !Self::link_looks_like_collection_item(task, url, text)
                || Self::url_looks_like_non_content_navigation(task, url)
                || !seen.insert(url.to_string())
            {
                continue;
            }

            let metadata = format!("{}; item listed on observed collection page", page_metadata);
            if Self::ranked_item_intent_blocker(task, url, text, Some(&metadata)).is_some() {
                continue;
            }
            let score = Self::browser_site_link_score(task, source_url, text, url);
            candidates.push((score, text.to_string(), url.to_string(), metadata));
        }
        if candidates.len() < requested {
            return None;
        }
        candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.2.cmp(&right.2)));

        let rows = candidates
            .into_iter()
            .take(requested)
            .enumerate()
            .map(|(index, (_, title, url, metadata))| {
                format!(
                    "- {}. {} | public metadata: {} | source: {}",
                    index + 1,
                    title,
                    metadata,
                    url
                )
            })
            .collect::<Vec<_>>();

        let summary = rows.join("\n");
        if !Self::collection_summary_satisfies_task(task, &summary) {
            return None;
        }

        Some(format!(
            "status: completed\nworker: browser\nexecuted_tool: browser_browse\nlookup_strategy: direct_site_link_collection\nsource_url: {source_url}\nresult_summary:\n{summary}\n\nevidence_scope: public_metadata_surrogate_not_full_source_content\ncontent_policy_note: full source content was not imported; downstream creative work must use public metadata and summaries only\nresult:\nObserved item-level links on a real browser collection page and preserved public metadata only; full copyrighted text was not scraped."
        ))
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_collection_page_metadata(
        task: &str,
        source_url: &str,
        payload: Option<&serde_json::Value>,
    ) -> String {
        let mut parts = vec![format!("observed collection page: {source_url}")];
        if let Some(content) = payload
            .and_then(|value| value.get("content"))
            .and_then(|value| value.as_str())
        {
            for line in content
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                if line.chars().count() > 120 {
                    continue;
                }
                let relevant = Self::cjk_relevance_terms(task)
                    .iter()
                    .any(|term| term.chars().count() >= 2 && line.contains(term))
                    || SearchPolicy::collection_intent_facets_for_task(task, true)
                        .into_iter()
                        .flat_map(|facet| facet.evidence_terms)
                        .any(|term| {
                            Self::text_contains(line, &term)
                                || line
                                    .to_ascii_lowercase()
                                    .contains(&term.to_ascii_lowercase())
                        });
                if relevant && !parts.iter().any(|part| part == line) {
                    parts.push(line.to_string());
                }
                if parts.len() >= 4 {
                    break;
                }
            }
        }
        preview_text(&parts.join(" / "), 260)
    }

    #[cfg(feature = "browser")]
    pub(crate) fn try_browser_payload_record_collection(
        task: &str,
        source_url: &str,
        payload: Option<&serde_json::Value>,
    ) -> Option<String> {
        if !Self::task_requests_collection_or_ranking(task) {
            return None;
        }
        let requested = Self::requested_collection_item_count(task).clamp(1, 10);
        let records = Self::browser_payload_record_values(payload);
        if records.is_empty() {
            return None;
        }
        let mut rows = Vec::new();
        let mut detail_sources = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for record in records {
            let title = record
                .get("title")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            let url = record
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or(source_url)
                .trim();
            let metadata = record
                .get("metadata")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            if title.is_empty() || metadata.is_empty() || !seen.insert(title.to_string()) {
                continue;
            }
            if Self::ranked_item_intent_blocker(task, url, title, Some(metadata)).is_some() {
                continue;
            }
            rows.push(format!(
                "- {}. {} | public metadata: {} | source: {}",
                rows.len() + 1,
                title,
                metadata,
                url
            ));
            detail_sources.push(json!({
                "title": title,
                "url": url,
                "metadata": metadata
            }));
            if rows.len() >= requested {
                break;
            }
        }

        let summary = rows.join("\n");
        if rows.len() < requested || !Self::collection_summary_satisfies_task(task, &summary) {
            return None;
        }

        Some(format!(
            "status: completed\nworker: browser\nexecuted_tool: browser_browse\nlookup_strategy: direct_site_embedded_record_collection\nsource_url: {source_url}\nresult_summary:\n{summary}\n\nevidence_scope: public_metadata_surrogate_not_full_source_content\nresult:\nObserved structured public metadata embedded in the page snapshot; full copyrighted text was not scraped.\ndetail_sources:\n{}",
            serde_json::to_string_pretty(&detail_sources).unwrap_or_else(|_| "[]".to_string())
        ))
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_payload_record_values<'a>(
        payload: Option<&'a serde_json::Value>,
    ) -> Vec<&'a serde_json::Value> {
        let mut records = Vec::new();
        if let Some(payload) = payload {
            Self::collect_browser_payload_record_values(payload, &mut records);
        }
        records
    }

    #[cfg(feature = "browser")]
    pub(crate) fn browser_payload_links(
        payload: Option<&serde_json::Value>,
    ) -> Vec<serde_json::Value> {
        let mut links = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if let Some(payload) = payload {
            Self::collect_browser_payload_links(payload, &mut links, &mut seen);
        }
        links
    }

    #[cfg(feature = "browser")]
    fn collect_browser_payload_links(
        value: &serde_json::Value,
        links: &mut Vec<serde_json::Value>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        if links.len() >= 300 {
            return;
        }
        match value {
            serde_json::Value::Object(map) => {
                if let Some(items) = map.get("links").and_then(|value| value.as_array()) {
                    for item in items {
                        let Some(url) = item.get("url").and_then(|value| value.as_str()) else {
                            continue;
                        };
                        if seen.insert(url.to_string()) {
                            links.push(item.clone());
                            if links.len() >= 300 {
                                return;
                            }
                        }
                    }
                }
                for child in map.values() {
                    Self::collect_browser_payload_links(child, links, seen);
                    if links.len() >= 300 {
                        return;
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::collect_browser_payload_links(item, links, seen);
                    if links.len() >= 300 {
                        return;
                    }
                }
            }
            _ => {}
        }
    }

    #[cfg(feature = "browser")]
    pub(crate) fn collect_browser_payload_record_values<'a>(
        value: &'a serde_json::Value,
        records: &mut Vec<&'a serde_json::Value>,
    ) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(items) = map.get("records").and_then(|value| value.as_array()) {
                    records.extend(items.iter());
                }
                for nested in map.values() {
                    Self::collect_browser_payload_record_values(nested, records);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::collect_browser_payload_record_values(item, records);
                }
            }
            _ => {}
        }
    }

    #[cfg(feature = "browser")]
    pub(crate) async fn try_browser_item_detail_collection(
        task: &str,
        source_url: &str,
        links: &[serde_json::Value],
        deadline: Option<std::time::Instant>,
    ) -> anyhow::Result<Option<String>> {
        if !Self::task_requests_collection_or_ranking(task) {
            return Ok(None);
        }

        let requested = Self::requested_collection_item_count(task).clamp(1, 10);
        let mut seen = std::collections::HashSet::new();
        let mut candidates = Vec::new();
        for link in links {
            let text = link
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            let url = link
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            if !Self::link_looks_like_collection_item(task, url, text) {
                continue;
            }
            if seen.insert(url.to_string()) {
                let score = Self::browser_site_link_score(task, source_url, text, url);
                candidates.push((score, text.to_string(), url.to_string()));
            }
        }
        if candidates.len() < requested {
            return Ok(None);
        }
        candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.2.cmp(&right.2)));

        let mut rows = Vec::new();
        let mut detail_sources = Vec::new();
        for (_, title, url) in candidates.into_iter().take(requested * 2) {
            if rows.len() >= requested {
                break;
            }
            let remaining = deadline
                .map(|deadline| deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap_or_else(|| std::time::Duration::from_secs(24));
            if remaining < std::time::Duration::from_millis(500) {
                break;
            }
            let attempt_timeout = remaining.min(std::time::Duration::from_secs(24));
            let snapshot = match tokio::time::timeout(
                attempt_timeout,
                BrowserTool::snapshot_once(&url, false, true),
            )
            .await
            {
                Ok(Ok(snapshot)) => snapshot,
                Ok(Err(_)) | Err(_) => continue,
            };
            let metadata = Self::browser_detail_public_metadata(task, &snapshot);
            if !Self::browser_detail_metadata_satisfies_item(task, &metadata) {
                continue;
            }
            if Self::ranked_item_intent_blocker(task, &url, &title, Some(&metadata)).is_some() {
                continue;
            }
            rows.push(format!(
                "- {}. {} | public metadata: {} | source: {}",
                rows.len() + 1,
                title,
                metadata,
                url
            ));
            detail_sources.push(json!({
                "title": title,
                "url": url,
                "metadata": metadata
            }));
        }

        let summary = rows.join("\n");
        if !Self::collection_summary_satisfies_task(task, &summary) {
            return Ok(None);
        }

        Ok(Some(format!(
            "status: completed\nworker: browser\nexecuted_tool: browser_browse\nlookup_strategy: direct_site_detail_collection\nsource_url: {source_url}\nresult_summary:\n{summary}\n\nresult:\nObserved item detail pages in a real browser session and extracted public metadata only; full copyrighted text was not scraped.\ndetail_sources:\n{}",
            serde_json::to_string_pretty(&detail_sources).unwrap_or_else(|_| "[]".to_string())
        )))
    }

    pub(crate) fn browser_detail_metadata_satisfies_item(task: &str, metadata: &str) -> bool {
        let metadata = metadata.trim();
        if metadata.is_empty() || metadata == "metadata not visible in fetched source" {
            return false;
        }
        if !Self::task_requests_book_collection(task) {
            return true;
        }
        let lowered = metadata.to_ascii_lowercase();
        metadata.contains("作者")
            || metadata.contains("简介")
            || metadata.contains("书籍")
            || lowered.contains("author")
            || lowered.contains("summary")
            || lowered.contains("intro")
            || lowered.contains("book")
    }

    pub(crate) fn browser_detail_public_metadata(task: &str, snapshot: &str) -> String {
        let mut selected = Vec::new();
        for line in snapshot
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            if line.chars().count() > 160 {
                continue;
            }
            let lowered = line.to_ascii_lowercase();
            let relevant = line.contains("作者")
                || line.contains("分类")
                || line.contains("简介")
                || line.contains("类型")
                || line.contains("玄幻")
                || line.contains("奇幻")
                || line.contains("仙侠")
                || lowered.contains("author")
                || lowered.contains("category")
                || lowered.contains("summary")
                || lowered.contains("intro")
                || Self::cjk_relevance_terms(task)
                    .iter()
                    .any(|term| term.chars().count() >= 2 && line.contains(term));
            if relevant && !selected.iter().any(|item| item == line) {
                selected.push(line.to_string());
            }
            if selected.len() >= 4 {
                break;
            }
        }

        if selected.is_empty() {
            "metadata not visible in fetched source".to_string()
        } else {
            let joined = selected.join(" / ");
            preview_text(&joined, 260)
        }
    }

    pub(crate) fn compact_structured_fetch_result(
        task: &str,
        fetch_payload: &str,
        limit: usize,
    ) -> Option<String> {
        let payload = serde_json::from_str::<serde_json::Value>(fetch_payload).ok()?;
        let url = payload.get("url")?.as_str()?.to_string();
        let content = payload.get("content")?.as_str()?;

        if let Some(summary) = summarize_github_search_items(&url, content, limit) {
            return Some(format!(
                "status: completed\nworker: researcher\nexecuted_tool: web_fetch\nlookup_strategy: structured_source_first\nsource_url: {url}\nresult_summary:\n{}",
                summary.content
            ));
        }

        if let Some(summary) = Self::summarize_academic_structured_items(task, &url, content, limit)
        {
            return Some(format!(
                "status: completed\nworker: researcher\nexecuted_tool: web_fetch\nlookup_strategy: structured_source_first\nsource_url: {url}\nresult_summary:\n{summary}"
            ));
        }

        None
    }

    pub(crate) fn summarize_academic_structured_items(
        task: &str,
        url: &str,
        content: &str,
        limit: usize,
    ) -> Option<String> {
        let body = serde_json::from_str::<serde_json::Value>(content).ok()?;
        let lowered_url = url.to_ascii_lowercase();
        let mut rows = Vec::new();

        if lowered_url.contains("api.crossref.org/works") {
            let items = body
                .get("message")
                .and_then(|value| value.get("items"))
                .and_then(|value| value.as_array())?;
            for item in items {
                if let Some(row) = Self::crossref_item_summary_row(task, item) {
                    rows.push(row);
                    if rows.len() >= limit {
                        break;
                    }
                }
            }
        } else if lowered_url.contains("api.openalex.org/works") {
            let items = body.get("results").and_then(|value| value.as_array())?;
            for item in items {
                if let Some(row) = Self::openalex_item_summary_row(task, item) {
                    rows.push(row);
                    if rows.len() >= limit {
                        break;
                    }
                }
            }
        } else if lowered_url.contains("/entrez/eutils/esummary.fcgi") {
            let records = Self::pubmed_esummary_records(&body)?;
            for item in records {
                if let Some(row) = Self::pubmed_esummary_item_summary_row(task, item) {
                    rows.push(row);
                    if rows.len() >= limit {
                        break;
                    }
                }
            }
        }

        (!rows.is_empty()).then(|| rows.join("\n"))
    }

    fn pubmed_esummary_records(body: &serde_json::Value) -> Option<Vec<&serde_json::Value>> {
        let result = body.get("result")?;
        let uids = result
            .get("uids")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut records = Vec::new();
        for uid in uids {
            if let Some(record) = result.get(uid) {
                records.push(record);
            }
        }
        if records.is_empty() {
            if let Some(object) = result.as_object() {
                records.extend(
                    object
                        .iter()
                        .filter(|(key, _)| key.as_str() != "uids")
                        .map(|(_, value)| value),
                );
            }
        }
        (!records.is_empty()).then_some(records)
    }

    fn pubmed_esummary_doi(record: &serde_json::Value) -> Option<&str> {
        record
            .get("articleids")
            .and_then(|value| value.as_array())
            .and_then(|items| {
                items.iter().find_map(|item| {
                    let id_type = item
                        .get("idtype")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    (id_type == "doi")
                        .then(|| item.get("value").and_then(|value| value.as_str()))
                        .flatten()
                })
            })
    }

    fn pubmed_esummary_record_matches_lookup_intent(task: &str, item: &serde_json::Value) -> bool {
        let title = item
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let uid = item
            .get("uid")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let doi = Self::pubmed_esummary_doi(item).unwrap_or_default();
        let pubtypes = item
            .get("pubtype")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let landing_url = if uid.is_empty() {
            String::new()
        } else {
            format!("https://pubmed.ncbi.nlm.nih.gov/{uid}/")
        };
        let evidence_lower = format!("{title}\n{doi}\n{pubtypes}\n{landing_url}").to_lowercase();
        Self::lookup_evidence_text_matches_intent(task, &landing_url, &evidence_lower, false)
    }

    pub(crate) fn pubmed_esummary_item_summary_row(
        task: &str,
        item: &serde_json::Value,
    ) -> Option<String> {
        let title = item
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or("Untitled");
        let uid = item
            .get("uid")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let source = item
            .get("fulljournalname")
            .or_else(|| item.get("source"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let published = item
            .get("pubdate")
            .or_else(|| item.get("sortpubdate"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let doi = Self::pubmed_esummary_doi(item).unwrap_or("");
        let landing_url = if uid.is_empty() {
            String::new()
        } else {
            format!("https://pubmed.ncbi.nlm.nih.gov/{uid}/")
        };
        if !Self::pubmed_esummary_record_matches_lookup_intent(task, item) {
            return None;
        }
        Some(format!(
            "- title: {title}; source: {source}; published: {published}; doi: {doi}; pmid: {uid}; url: {landing_url}"
        ))
    }

    pub(crate) fn crossref_item_summary_row(
        task: &str,
        item: &serde_json::Value,
    ) -> Option<String> {
        let title = item
            .get("title")
            .and_then(|value| value.as_array())
            .and_then(|values| values.first())
            .and_then(|value| value.as_str())
            .unwrap_or("Untitled");
        let doi = item
            .get("DOI")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let source = item
            .get("container-title")
            .and_then(|value| value.as_array())
            .and_then(|values| values.first())
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let published = item
            .get("published-print")
            .or_else(|| item.get("published-online"))
            .or_else(|| item.get("created"))
            .and_then(|value| value.get("date-parts"))
            .and_then(|value| value.as_array())
            .and_then(|values| values.first())
            .and_then(|value| value.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|value| value.as_u64())
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join("-")
            })
            .unwrap_or_default();
        let landing_url = item
            .get("URL")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let evidence = format!("{title}\n{source}\n{doi}\n{published}\n{landing_url}\n{item}");
        let pseudo_payload = json!({
            "url": landing_url,
            "content": evidence
        })
        .to_string();
        if !Self::fetch_payload_matches_lookup_intent(task, &pseudo_payload) {
            return None;
        }
        Some(format!(
            "- title: {title}; source: {source}; published: {published}; doi: {doi}; url: {landing_url}"
        ))
    }

    pub(crate) fn openalex_item_summary_row(
        task: &str,
        item: &serde_json::Value,
    ) -> Option<String> {
        let title = item
            .get("display_name")
            .or_else(|| item.get("title"))
            .and_then(|value| value.as_str())
            .unwrap_or("Untitled");
        let doi = item
            .get("doi")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let source = item
            .get("primary_location")
            .and_then(|value| value.get("source"))
            .and_then(|value| value.get("display_name"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let published = item
            .get("publication_date")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let landing_url = item
            .get("primary_location")
            .and_then(|value| value.get("landing_page_url"))
            .and_then(|value| value.as_str())
            .or_else(|| item.get("id").and_then(|value| value.as_str()))
            .unwrap_or("");
        let evidence = format!("{title}\n{source}\n{doi}\n{published}\n{landing_url}\n{item}");
        let pseudo_payload = json!({
            "url": landing_url,
            "content": evidence
        })
        .to_string();
        if !Self::fetch_payload_matches_lookup_intent(task, &pseudo_payload) {
            return None;
        }
        Some(format!(
            "- title: {title}; source: {source}; published: {published}; doi: {doi}; url: {landing_url}"
        ))
    }

    pub(crate) fn first_url(text: &str) -> Option<String> {
        let start = text.find("https://").or_else(|| text.find("http://"))?;
        let rest = &text[start..];
        let end = rest
            .char_indices()
            .find_map(|(idx, ch)| {
                if ch.is_whitespace()
                    || matches!(
                        ch,
                        '"' | '\'' | ',' | ')' | ']' | '}' | '>' | '。' | '，' | '）' | '】'
                    )
                {
                    Some(idx)
                } else {
                    None
                }
            })
            .unwrap_or(rest.len());
        Some(rest[..end].trim_end_matches('.').to_string())
    }

    pub(crate) fn task_requests_file_read(task: &str) -> bool {
        let lowered = task.to_lowercase();
        (lowered.contains("read")
            || lowered.contains("读取")
            || lowered.contains("读出")
            || lowered.contains("查看")
            || lowered.contains("返回"))
            && (lowered.contains("file")
                || lowered.contains("文件")
                || lowered.contains(".txt")
                || lowered.contains(".md")
                || lowered.contains(".json")
                || lowered.contains(".rs")
                || lowered.contains(".toml")
                || lowered.contains(".yaml")
                || lowered.contains(".yml"))
    }

    pub(crate) fn task_requests_local_file_continuation(task: &str) -> bool {
        let lowered = task.to_lowercase();
        let asks_continuation = lowered.contains("continue")
            || lowered.contains("continuation")
            || lowered.contains("append")
            || lowered.contains("extend")
            || lowered.contains("checkpointed")
            || lowered.contains("续写")
            || lowered.contains("继续")
            || lowered.contains("追加")
            || lowered.contains("扩写")
            || lowered.contains("补写");
        let mentions_local_artifact = lowered.contains(".txt")
            || lowered.contains(".md")
            || lowered.contains("local file")
            || lowered.contains("text artifact")
            || lowered.contains("已保存")
            || task.contains("文档")
            || task.contains("文件");
        if asks_continuation
            && mentions_local_artifact
            && Self::task_requests_existing_artifact_revision(task)
            && !Self::task_requests_checkpointed_text_artifact(task)
        {
            return false;
        }
        (asks_continuation && mentions_local_artifact)
            || Self::task_requests_checkpointed_text_artifact(task)
    }

    pub(crate) fn task_requests_existing_artifact_revision(task: &str) -> bool {
        crate::tool::writing::policy::task_requests_existing_artifact_revision(task)
    }

    pub(crate) fn writer_fast_path_should_defer_existing_revision(task: &str) -> bool {
        Self::task_requests_existing_artifact_revision(task)
            && !Self::task_requests_checkpointed_text_artifact(task)
    }

    pub(crate) fn task_requests_checkpointed_text_artifact(task: &str) -> bool {
        let intent = Self::artifact_intent_surface(task);
        let lowered = intent.to_lowercase();
        let full_lowered = task.to_lowercase();
        let asks_text_artifact = Self::task_requests_file_write(task)
            && !lowered.contains(".pdf")
            && !lowered.contains("pdf")
            && !Self::extract_write_target_path(task)
                .as_deref()
                .is_some_and(|path| path.to_ascii_lowercase().ends_with(".pdf"));
        if !asks_text_artifact {
            return false;
        }

        Self::requested_text_target_chars(task)
            .is_some_and(|target| target > Self::longform_step_target_chars())
            || full_lowered.contains("checkpointed continuation")
            || full_lowered.contains("too large for one model response")
            || full_lowered.contains("oversized artifact")
            || full_lowered.contains("large document")
            || full_lowered.contains("book-length")
            || intent.contains("超长")
            || intent.contains("长文")
            || intent.contains("长篇")
    }

    pub(crate) fn should_route_local_continuation_to_writer(
        requested_role: &str,
        task: &str,
    ) -> bool {
        (Self::task_requests_local_file_continuation(task)
            || Self::task_requests_local_writing_context(task))
            && matches!(
                Self::normalize_role_label(requested_role).as_str(),
                "researcher" | "web_research" | "source_research" | "web_lookup" | "latest_lookup"
            )
    }

    pub(crate) fn should_rewrite_requested_role_for_local_continuation(
        auto_role_requested: bool,
        requested_role: &str,
        task: &str,
    ) -> bool {
        auto_role_requested && Self::should_route_local_continuation_to_writer(requested_role, task)
    }

    pub(crate) fn task_requests_local_writing_context(task: &str) -> bool {
        crate::tool::writing::policy::task_requests_local_writing_context(task)
    }

    pub(crate) fn task_requests_file_write(task: &str) -> bool {
        let lowered = task.to_lowercase();
        (lowered.contains("write_file")
            || lowered.contains("write to")
            || lowered.contains("create")
            || lowered.contains("save")
            || task.contains("写入")
            || task.contains("写成")
            || task.contains("保存成")
            || task.contains("保存为")
            || task.contains("保存到"))
            && (lowered.contains(".txt")
                || lowered.contains("txt")
                || lowered.contains(" txt")
                || lowered.contains("txt ")
                || lowered.contains("txt文")
                || lowered.contains("文本文件")
                || lowered.contains("text file")
                || lowered.contains(".md")
                || lowered.contains(" markdown")
                || task.contains("Markdown")
                || lowered.contains(".pdf")
                || lowered.contains("pdf")
                || lowered.contains(".json")
                || lowered.contains("text artifact")
                || task.contains("文档")
                || task.contains("文件"))
    }

    pub(crate) fn task_requests_local_git(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        (lowered.contains("git_ops")
            || lowered.contains("git status")
            || lowered.contains("local_status")
            || task.contains("仓库状态")
            || task.contains("当前仓库")
            || task.contains("本地仓库"))
            && !lowered.contains("github.com/search")
    }

    pub(crate) fn extract_local_path(task: &str) -> Option<PathBuf> {
        task.split(|ch: char| {
            ch.is_whitespace() || ch == '，' || ch == '。' || ch == ',' || ch == '；' || ch == ';'
        })
        .map(|part| {
            part.trim_matches(|ch: char| {
                ch == '"' || ch == '\'' || ch == '`' || ch == ':' || ch == '：'
            })
            .trim_end_matches(|ch: char| {
                matches!(
                    ch,
                    '.' | '。' | ',' | '，' | ';' | '；' | ')' | '）' | ']' | '】'
                )
            })
        })
        .find(|part| {
            part.starts_with('/') || part.contains(":\\") || {
                let lowered = part.to_ascii_lowercase();
                !Self::looks_like_bare_file_extension(part)
                    && (lowered.ends_with(".txt")
                        || lowered.ends_with(".md")
                        || lowered.ends_with(".pdf")
                        || lowered.ends_with(".json")
                        || lowered.ends_with(".rs")
                        || lowered.ends_with(".toml")
                        || lowered.ends_with(".yaml")
                        || lowered.ends_with(".yml"))
            }
        })
        .map(PathBuf::from)
    }

    pub(crate) fn extract_write_target_path(task: &str) -> Option<String> {
        for marker in [
            "Write to `",
            "write to `",
            "path `",
            "path: `",
            "at `",
            "to `",
            "保存到 `",
            "保存成 `",
            "保存为 `",
            "写入 `",
            "写成 `",
        ] {
            if let Some(start) = task.find(marker) {
                let rest = &task[start + marker.len()..];
                if let Some(end) = rest.find('`') {
                    let path = rest[..end].trim();
                    if !path.is_empty() {
                        return Some(path.to_string());
                    }
                }
            }
        }

        task.split(|ch: char| {
            ch.is_whitespace() || matches!(ch, '，' | '。' | ',' | ';' | '；' | ')' | '）')
        })
        .map(|part| {
            part.trim_matches(|ch| {
                matches!(ch, '`' | '"' | '\'' | ':' | '：' | '[' | ']' | '【' | '】')
            })
        })
        .find(|part| {
            let lowered = part.to_ascii_lowercase();
            !Self::looks_like_bare_file_extension(part)
                && (lowered.ends_with(".txt")
                    || lowered.ends_with(".md")
                    || lowered.ends_with(".pdf")
                    || lowered.ends_with(".json")
                    || lowered.ends_with(".yaml")
                    || lowered.ends_with(".yml"))
        })
        .map(str::to_string)
    }

    pub(crate) fn looks_like_bare_file_extension(value: &str) -> bool {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            ".txt"
                | "txt"
                | ".md"
                | "md"
                | ".pdf"
                | "pdf"
                | ".json"
                | "json"
                | ".yaml"
                | "yaml"
                | ".yml"
                | "yml"
        )
    }

    pub(crate) fn default_generated_artifact_path(task: &str) -> Option<String> {
        if !Self::task_requests_file_write(task) {
            return None;
        }
        let intent = Self::artifact_intent_surface(task);
        let lowered = intent.to_lowercase();
        let artifact_ext = if lowered.contains("pdf") || lowered.contains(".pdf") {
            "pdf"
        } else if lowered.contains("markdown") || lowered.contains(".md") {
            "md"
        } else if lowered.contains(".json") {
            "json"
        } else {
            "txt"
        };
        Some(format!(
            "data/generated/tasks/{}/agent-artifact-1.{artifact_ext}",
            uuid::Uuid::new_v4()
        ))
    }

    pub(crate) fn artifact_intent_surface(task: &str) -> String {
        let mut surface = task.trim();
        if let Some((_, tail)) = surface.split_once("Original user request:") {
            surface = tail.trim();
        }
        for marker in [
            "Verified researcher evidence:",
            "Knowledge import receipt:",
            "已验证公开证据摘要：",
            "已验证证据摘要：",
            "知识库写入回执：",
            "[Tool Result:",
            "Tool Result:",
            "blockers:",
            "当前具体卡点：",
        ] {
            if let Some((head, _)) = surface.split_once(marker) {
                surface = head.trim();
            }
        }
        surface.to_string()
    }

    pub(crate) fn build_delegated_file_artifact_prompt_with_contract(
        task: &str,
        path: &str,
        quality_contract: &ArtifactQualityContract,
    ) -> String {
        let original_request = Self::artifact_intent_surface(task);
        let evidence = Self::artifact_evidence_surface(task);
        crate::tool::writing::artifact_contract::build_file_artifact_prompt(
            path,
            &original_request,
            &evidence,
            quality_contract,
        )
    }

    pub(crate) fn build_delegated_file_artifact_revision_prompt(
        task: &str,
        path: &str,
        previous: &str,
        quality: &ArtifactQualityReport,
        attempt: usize,
    ) -> String {
        let contract = Self::artifact_quality_contract(task);
        crate::tool::writing::artifact_contract::build_file_artifact_revision_prompt(
            task, path, previous, quality, &contract, attempt,
        )
    }

    pub(crate) fn sanitize_generated_file_artifact(output: &str, task: &str) -> String {
        crate::tool::writing::artifact_contract::sanitize_generated_file_artifact(output, task)
    }

    pub(crate) fn artifact_quality_report_with_contract(
        task: &str,
        content: &str,
        contract: &ArtifactQualityContract,
    ) -> ArtifactQualityReport {
        let evidence = Self::artifact_evidence_surface(task);
        crate::tool::writing::artifact_contract::quality_report_with_evidence(
            content, contract, &evidence,
        )
    }

    pub(crate) fn artifact_quality_contract(task: &str) -> ArtifactQualityContract {
        let intent = Self::artifact_intent_surface(task);
        let requested_chars = Self::requested_text_target_chars(&intent)
            .or_else(|| Self::requested_text_target_chars(task));
        let max_chars = Self::requested_text_max_chars(&intent)
            .or_else(|| Self::requested_text_max_chars(task));
        crate::tool::writing::artifact_contract::infer_quality_contract(
            &intent,
            requested_chars,
            max_chars,
        )
    }

    pub(crate) fn artifact_quality_contract_for_coordinator(
        coordinator: &Coordinator,
        task: &str,
    ) -> ArtifactQualityContract {
        let policies = coordinator
            .worker_blueprints()
            .into_iter()
            .filter_map(|blueprint| blueprint.artifact_policy)
            .collect::<Vec<_>>();
        let bundle = RuntimePolicyResolver::resolve(
            TaskPolicyInput::new(Self::artifact_intent_surface(task))
                .with_phase(PolicyPhase::ArtifactValidation),
            &policies,
        );
        if let Some(contract) = bundle.quality_contract {
            let mut policy_contract = artifact_quality_contract_from_policy(contract);
            let inferred = Self::artifact_quality_contract(task);
            policy_contract.delivery_scope = inferred.delivery_scope;
            policy_contract.final_target_chars = inferred.final_target_chars;
            if let Some(max_chars) = inferred.max_chars {
                policy_contract.max_chars = Some(max_chars);
                policy_contract.min_chars = policy_contract.min_chars.min(max_chars);
            }
            if inferred.delivery_scope
                == crate::tool::writing::artifact_contract::ArtifactDeliveryScope::Stage
            {
                policy_contract.min_chars = inferred.min_chars;
            } else if let Some(target) = inferred.final_target_chars {
                policy_contract.min_chars = target;
            }
            return policy_contract;
        }
        Self::artifact_quality_contract(task)
    }

    fn artifact_evidence_surface(task: &str) -> String {
        for start_marker in [
            "Verified researcher evidence:",
            "已验证公开证据摘要：",
            "已验证证据摘要：",
        ] {
            let Some((_, tail)) = task.split_once(start_marker) else {
                continue;
            };
            let end = [
                "Knowledge import receipt:",
                "知识库写入回执：",
                "blockers:",
                "当前具体卡点：",
            ]
            .iter()
            .filter_map(|marker| tail.find(marker))
            .min()
            .unwrap_or(tail.len());
            let evidence = tail[..end].trim();
            if !evidence.is_empty() {
                return evidence.to_string();
            }
        }
        String::new()
    }

    pub(crate) fn write_pdf_text_artifact(path: &Path, content: &str) -> anyhow::Result<u64> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = Self::render_simple_pdf(content);
        std::fs::write(path, &bytes)?;
        Ok(bytes.len() as u64)
    }

    pub(crate) fn render_simple_pdf(content: &str) -> Vec<u8> {
        let lines = Self::pdf_wrapped_lines(content, 42);
        let pages = lines
            .chunks(44)
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>();
        let pages = if pages.is_empty() {
            vec![vec!["".to_string()]]
        } else {
            pages
        };
        let page_count = pages.len();
        let font_obj = 3 + page_count * 2;
        let descendant_font_obj = font_obj + 1;
        let descriptor_obj = font_obj + 2;
        let mut objects = Vec::new();
        objects.push(format!("<< /Type /Catalog /Pages 2 0 R >>"));
        let kids = (0..page_count)
            .map(|idx| format!("{} 0 R", 3 + idx))
            .collect::<Vec<_>>()
            .join(" ");
        objects.push(format!(
            "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
        ));
        for idx in 0..page_count {
            let page_obj = 3 + idx;
            let stream_obj = 3 + page_count + idx;
            let _ = page_obj;
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 {font_obj} 0 R >> >> /Contents {stream_obj} 0 R >>"
            ));
        }
        for page_lines in &pages {
            let stream = Self::pdf_page_content_stream(page_lines);
            objects.push(format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                stream.as_bytes().len(),
                stream
            ));
        }
        objects.push(format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /STSong-Light /Encoding /UniGB-UCS2-H /DescendantFonts [{descendant_font_obj} 0 R] >>"
        ));
        objects.push(format!(
            "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /STSong-Light /CIDSystemInfo << /Registry (Adobe) /Ordering (GB1) /Supplement 2 >> /FontDescriptor {descriptor_obj} 0 R /DW 1000 >>"
        ));
        objects.push(
            "<< /Type /FontDescriptor /FontName /STSong-Light /Flags 6 /FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 880 /Descent -120 /CapHeight 700 /StemV 80 >>"
                .to_string(),
        );

        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::with_capacity(objects.len());
        for (idx, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", idx + 1, object).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    pub(crate) fn pdf_wrapped_lines(content: &str, max_chars: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for raw_line in content.lines() {
            let trimmed = raw_line.trim_end();
            if trimmed.is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut current = String::new();
            for ch in trimmed.chars() {
                current.push(ch);
                if current.chars().count() >= max_chars {
                    lines.push(current);
                    current = String::new();
                }
            }
            if !current.is_empty() {
                lines.push(current);
            }
        }
        lines
    }

    pub(crate) fn pdf_page_content_stream(lines: &[String]) -> String {
        let mut stream = String::from("BT\n/F1 11 Tf\n50 790 Td\n15 TL\n");
        for line in lines {
            stream.push('<');
            stream.push_str(&Self::pdf_utf16_hex(line));
            stream.push_str("> Tj\nT*\n");
        }
        stream.push_str("ET");
        stream
    }

    pub(crate) fn pdf_utf16_hex(text: &str) -> String {
        text.encode_utf16()
            .flat_map(|unit| [(unit >> 8) as u8, (unit & 0xff) as u8])
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>()
    }

    pub(crate) fn evidence_quality_blocker_for_file_artifact(task: &str) -> Option<String> {
        if !task.contains("Verified researcher evidence:") {
            return None;
        }
        let source_goal = Self::artifact_source_goal_text(task);
        if !Self::task_requests_collection_or_ranking(source_goal) {
            return None;
        }
        let requested = Self::requested_collection_item_count(source_goal);
        if requested <= 1 {
            return None;
        }
        let observed = Self::verified_ranked_metadata_count(task);
        if observed >= requested {
            return None;
        }
        Some(Self::format_evidence_gap_blocker(
            "writer",
            "delegate",
            "source_derived_file_artifact_guard",
            None,
            observed,
            requested,
            "verified_research_metadata_required",
            None,
            Some("file artifact generation paused before source-derived writing because the verified evidence set does not satisfy the original collection requirement"),
        ))
    }

    pub(crate) fn artifact_source_goal_text(task: &str) -> &str {
        let Some(after_marker) = task
            .split_once("Original user request:")
            .map(|(_, tail)| tail)
        else {
            return task;
        };
        let end = after_marker
            .find("\n\nVerified researcher evidence:")
            .or_else(|| after_marker.find("\n\nKnowledge import receipt:"))
            .or_else(|| after_marker.find("\n\nArtifact contract:"))
            .unwrap_or(after_marker.len());
        let goal = after_marker[..end].trim();
        if goal.is_empty() {
            task
        } else {
            goal
        }
    }

    pub(crate) fn verified_ranked_metadata_count(text: &str) -> usize {
        let ranked = Regex::new(r"^\s*-\s*\d{1,3}[.、)]\s+").expect("valid ranked metadata regex");
        text.lines()
            .filter(|line| {
                let trimmed = line.trim();
                ranked.is_match(trimmed)
                    && trimmed.contains("public metadata:")
                    && !trimmed.contains("metadata not visible")
            })
            .count()
    }
}
