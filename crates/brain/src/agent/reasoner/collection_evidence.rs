use regex::Regex;

use super::knowledge_delivery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CollectionEvidenceGap {
    pub requested: usize,
    pub observed: usize,
}

pub(super) fn explicit_requested_item_count(query: &str) -> Option<usize> {
    let lowered = query.to_ascii_lowercase();
    let numeric_patterns = [
        r"(?:top|first|up\s+to|at\s+most|no\s+more\s+than)\s*(\d{1,4})",
        r"(\d{1,4})\s*(?:items?|records?|sources?|documents?|docs?|papers?|articles?|books?|novels?|stories?|texts?|entries?|results?)",
        r"(?:前|最多|至多|不超过|不少于|至少|找出|找到|查找|搜索)\s*(\d{1,4})\s*(?:部|个|条|篇|本|项|份|则)?",
        r"(\d{1,4})\s*(?:部|个|条|篇|本|项|份|则)",
    ];
    for pattern in numeric_patterns {
        let regex = Regex::new(pattern).expect("valid collection count regex");
        if let Some(count) = regex
            .captures(&lowered)
            .and_then(|captures| captures.get(1))
            .and_then(|value| value.as_str().parse::<usize>().ok())
            .filter(|count| (2..=1000).contains(count))
        {
            return Some(count);
        }
    }

    let english_words = [
        ("one", 1),
        ("two", 2),
        ("three", 3),
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
        ("ten", 10),
        ("eleven", 11),
        ("twelve", 12),
        ("thirteen", 13),
        ("fourteen", 14),
        ("fifteen", 15),
        ("sixteen", 16),
        ("seventeen", 17),
        ("eighteen", 18),
        ("nineteen", 19),
        ("twenty", 20),
    ];
    for (word, count) in english_words {
        let prefixed = [
            format!("top {word}"),
            format!("first {word}"),
            format!("up to {word}"),
            format!("at most {word}"),
            format!("no more than {word}"),
        ];
        let item_word = Regex::new(&format!(
            r"\b{word}\s+(?:items?|records?|sources?|documents?|docs?|papers?|articles?|books?|novels?|stories?|texts?|entries?|results?)\b"
        ))
        .expect("valid English word item regex");
        if prefixed.iter().any(|needle| lowered.contains(needle)) || item_word.is_match(&lowered) {
            return (count > 1).then_some(count);
        }
    }

    for marker in ["前", "最多", "至多", "不少于", "至少"] {
        if let Some(count) = chinese_count_after_marker(query, marker) {
            return (count > 1).then_some(count);
        }
    }
    chinese_count_before_unit(query).and_then(|count| (count > 1).then_some(count))
}

pub(super) fn requested_item_count_or_default(query: &str, default: usize) -> usize {
    explicit_requested_item_count(query).unwrap_or_else(|| {
        if singular_item_request(query) {
            1
        } else {
            default
        }
    })
}

fn singular_item_request(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    let english = Regex::new(
        r"\b(?:one|a|an|single)\s+(?:item|record|source|document|doc|paper|article|book|novel|story|text|entry|result)\b",
    )
    .expect("valid singular item regex");
    english.is_match(&lowered)
        || [
            "一部", "一个", "一条", "一篇", "一本", "一项", "一份", "一则", "单个", "任一",
        ]
        .iter()
        .any(|marker| query.contains(marker))
}

pub(super) fn observed_item_count(result: &str) -> usize {
    let mut observed = numeric_field_max(result);
    observed = observed.max(knowledge_delivery::ranked_metadata_items_from_result(result).len());
    observed.max(generic_ranked_result_line_count(result))
}

pub(super) fn evidence_gap(query: &str, result: &str) -> Option<CollectionEvidenceGap> {
    let requested = explicit_requested_item_count(query)?;
    if requested <= 1 {
        return None;
    }
    let observed = observed_item_count(result);
    (observed < requested).then_some(CollectionEvidenceGap {
        requested,
        observed,
    })
}

pub(super) fn format_gap_blocker(
    query: &str,
    gap: CollectionEvidenceGap,
    evidence_preview: &str,
) -> String {
    let preview: String = evidence_preview.chars().take(2_400).collect();
    if query_prefers_chinese(query) {
        format!(
            "已暂停后续产物生成：原始任务要求先取得 {requested} 条条目级来源证据，但当前只确认了 {observed} 条。为了避免把单个列表页、目录页或不完整检索结果当成完整资料集，我没有继续写作/生成文件。\n\n下一步应继续检索、打开条目详情、或由 worker 明确报告只能找到多少条以及卡在哪里。\n\n当前证据摘要：\n{preview}",
            requested = gap.requested,
            observed = gap.observed
        )
    } else {
        format!(
            "I paused downstream artifact generation: the original task requires {requested} item-level source records first, but the current evidence confirms only {observed}. I will not treat a single listing/index page or incomplete lookup as the full source set.\n\nNext, continue lookup/detail-page observation, or have the worker report how many records were actually found and where it got blocked.\n\nCurrent evidence preview:\n{preview}",
            requested = gap.requested,
            observed = gap.observed
        )
    }
}

pub(super) fn recovery_instruction(
    query: &str,
    gap: CollectionEvidenceGap,
    result: &str,
) -> String {
    let preview: String = result.lines().take(80).collect::<Vec<_>>().join("\n");
    format!(
        "BENSHU_COLLECTION_EVIDENCE_GATE\n\
         The original user task asks for a multi-item source set before knowledge import or downstream artifact generation.\n\
         Requested item-level records: {requested}\n\
         Observed item-level records: {observed}\n\
         Do not import a single index/listing/search page as if it satisfied the requested collection.\n\
         Continue gathering item-level evidence with the available lookup/browser tools. If real attempts cannot find enough records, return a clear blocker with observed/requested counts and the actual obstacle. Keep this generic across Chinese and English tasks.\n\
         Original user request: {query}\n\n\
         Current evidence preview:\n{preview}",
        requested = gap.requested,
        observed = gap.observed
    )
}

fn numeric_field_max(result: &str) -> usize {
    let regex = Regex::new(r"(?im)^\s*(?:observed_item_records|item_count|items_found|record_count|source_count)\s*[:=]\s*(\d{1,6})\s*$")
        .expect("valid observed item count regex");
    regex
        .captures_iter(result)
        .filter_map(|captures| captures.get(1))
        .filter_map(|value| value.as_str().parse::<usize>().ok())
        .max()
        .unwrap_or(0)
}

fn generic_ranked_result_line_count(result: &str) -> usize {
    let ranked = Regex::new(r"^\s*-?\s*\d{1,4}[.、)]\s+").expect("valid ranked line regex");
    result
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if !ranked.is_match(trimmed) {
                return false;
            }
            let lowered = trimmed.to_ascii_lowercase();
            lowered.contains("source:")
                || lowered.contains("url:")
                || lowered.contains("http://")
                || lowered.contains("https://")
                || lowered.contains("metadata:")
                || lowered.contains("public metadata:")
        })
        .count()
}

fn chinese_count_after_marker(query: &str, marker: &str) -> Option<usize> {
    let (_, tail) = query.split_once(marker)?;
    let digits = tail
        .chars()
        .take_while(|ch| is_chinese_number_char(*ch))
        .collect::<String>();
    chinese_number_to_usize(&digits).filter(|count| (2..=1000).contains(count))
}

fn chinese_count_before_unit(query: &str) -> Option<usize> {
    let units = ["部", "个", "条", "篇", "本", "项", "份", "则"];
    for unit in units {
        let Some((head, _)) = query.split_once(unit) else {
            continue;
        };
        let digits = head
            .chars()
            .rev()
            .take_while(|ch| is_chinese_number_char(*ch))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        if let Some(count) =
            chinese_number_to_usize(&digits).filter(|count| (2..=1000).contains(count))
        {
            return Some(count);
        }
    }
    None
}

fn is_chinese_number_char(ch: char) -> bool {
    matches!(
        ch,
        '零' | '〇'
            | '一'
            | '二'
            | '两'
            | '三'
            | '四'
            | '五'
            | '六'
            | '七'
            | '八'
            | '九'
            | '十'
            | '百'
    )
}

fn chinese_digit(ch: char) -> Option<usize> {
    match ch {
        '零' | '〇' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

fn chinese_number_to_usize(text: &str) -> Option<usize> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text.chars().all(|ch| chinese_digit(ch).is_some()) {
        return text.chars().try_fold(0usize, |acc, ch| {
            chinese_digit(ch).map(|digit| acc * 10 + digit)
        });
    }

    let mut total = 0usize;
    let mut current = 0usize;
    for ch in text.chars() {
        match ch {
            '百' => {
                let value = if current == 0 { 1 } else { current };
                total += value * 100;
                current = 0;
            }
            '十' => {
                let value = if current == 0 { 1 } else { current };
                total += value * 10;
                current = 0;
            }
            _ => current = chinese_digit(ch)?,
        }
    }
    Some(total + current)
}

fn query_prefers_chinese(query: &str) -> bool {
    query.chars().any(|ch| {
        ('\u{4e00}'..='\u{9fff}').contains(&ch)
            || ('\u{3400}'..='\u{4dbf}').contains(&ch)
            || ('\u{f900}'..='\u{faff}').contains(&ch)
    })
}
