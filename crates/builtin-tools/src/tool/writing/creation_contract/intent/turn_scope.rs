use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreationDraftTurnScope {
    FirstUnit,
    ExplicitUnits(usize),
    AllRemaining,
    Configured,
}

pub const CREATION_EXECUTION_SCOPE_NOTE_PREFIX: &str = "__creation_execution_scope:";
pub const FICTION_EXPLICIT_TURN_UNITS_MAX: usize = 20;

pub fn creation_draft_turn_scope(message: &str, artifact_kind: &str) -> CreationDraftTurnScope {
    let all_remaining = creation_draft_requests_all_remaining(message, artifact_kind);
    if all_remaining && message_requests_incremental_full_run(message, artifact_kind) {
        CreationDraftTurnScope::AllRemaining
    } else if let Some(units) = creation_draft_requested_turn_units(message, artifact_kind) {
        if units <= 1 {
            CreationDraftTurnScope::FirstUnit
        } else {
            CreationDraftTurnScope::ExplicitUnits(units)
        }
    } else if approval_requests_first_writing_unit(message, artifact_kind) {
        CreationDraftTurnScope::FirstUnit
    } else if all_remaining {
        CreationDraftTurnScope::AllRemaining
    } else {
        CreationDraftTurnScope::Configured
    }
}

fn message_requests_incremental_full_run(message: &str, artifact_kind: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    let incremental_terms = if artifact_kind == "fiction" {
        &[
            "每次一章",
            "每次只写一章",
            "每轮一章",
            "每轮只写一章",
            "一次一章",
            "一次只写一章",
            "逐章",
            "一章一章",
            "one chapter at a time",
            "chapter by chapter",
        ][..]
    } else {
        &[
            "每次一节",
            "每次只写一节",
            "每轮一节",
            "逐节",
            "一节一节",
            "one section at a time",
            "section by section",
        ][..]
    };
    if incremental_terms
        .iter()
        .any(|term| message.contains(term) || lowered.contains(term))
    {
        return true;
    }
    if artifact_kind != "fiction" {
        return false;
    }
    let starts_from_first_chapter = [
        "从第一章起",
        "从第1章起",
        "从第一章开始",
        "从第1章开始",
        "starting from chapter one",
        "starting with chapter one",
        "from chapter one",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(term));
    let requests_continuous_execution = [
        "持续写",
        "持续自动",
        "连续写",
        "连续自动",
        "自动连续",
        "一直写",
        "continue writing",
        "write continuously",
        "continuously write",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(term));
    starts_from_first_chapter && requests_continuous_execution
}

pub fn creation_execution_scope_note(message: &str, artifact_kind: &str) -> Option<String> {
    match creation_draft_turn_scope(message, artifact_kind) {
        CreationDraftTurnScope::FirstUnit => {
            Some(format!("{CREATION_EXECUTION_SCOPE_NOTE_PREFIX}first_unit"))
        }
        CreationDraftTurnScope::ExplicitUnits(units) => Some(format!(
            "{CREATION_EXECUTION_SCOPE_NOTE_PREFIX}explicit_units={units}"
        )),
        CreationDraftTurnScope::AllRemaining => Some(format!(
            "{CREATION_EXECUTION_SCOPE_NOTE_PREFIX}all_remaining"
        )),
        CreationDraftTurnScope::Configured => None,
    }
}

pub fn persisted_creation_execution_scope(notes: &[String]) -> Option<CreationDraftTurnScope> {
    notes.iter().rev().find_map(|note| {
        let value = note.strip_prefix(CREATION_EXECUTION_SCOPE_NOTE_PREFIX)?;
        if value == "first_unit" {
            Some(CreationDraftTurnScope::FirstUnit)
        } else if value == "all_remaining" {
            Some(CreationDraftTurnScope::AllRemaining)
        } else {
            value
                .strip_prefix("explicit_units=")
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .map(CreationDraftTurnScope::ExplicitUnits)
        }
    })
}

pub fn creation_draft_requested_turn_units(message: &str, artifact_kind: &str) -> Option<usize> {
    let max_reasonable_turn_units = if artifact_kind == "fiction" {
        FICTION_EXPLICIT_TURN_UNITS_MAX
    } else {
        12
    };
    let mut candidates = Vec::new();
    if artifact_kind == "fiction" {
        collect_unit_count_before_markers(message, &["章"], &mut candidates);
        collect_english_unit_count(message, &["chapter", "chapters"], &mut candidates);
    } else {
        collect_unit_count_before_markers(message, &["节", "部分"], &mut candidates);
        collect_english_unit_count(
            message,
            &["section", "sections", "part", "parts"],
            &mut candidates,
        );
    }
    candidates
        .into_iter()
        .filter(|value| (1..=max_reasonable_turn_units).contains(value))
        .max()
}

pub fn collect_unit_count_before_markers(message: &str, markers: &[&str], out: &mut Vec<usize>) {
    for marker in markers {
        let mut search_start = 0usize;
        while let Some(relative) = message[search_start..].find(marker) {
            let marker_start = search_start + relative;
            let prefix = &message[..marker_start];
            if let Some(value) = trailing_unit_count(prefix) {
                if prefix_has_generation_unit_action(prefix)
                    && !prefix_negates_requested_unit_count(prefix)
                    && !prefix_looks_like_existing_unit_reference(prefix)
                {
                    out.push(value);
                }
            }
            search_start = marker_start + marker.len();
        }
    }
}

fn prefix_negates_requested_unit_count(prefix: &str) -> bool {
    let clause = prefix
        .rsplit(|ch| matches!(ch, '，' | ',' | '。' | ';' | '；' | '\n'))
        .next()
        .unwrap_or(prefix);
    let compact = clause
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let lowered = compact.to_ascii_lowercase();
    [
        "不要只写",
        "别只写",
        "不能只写",
        "不应只写",
        "不是只写",
        "并非只写",
        "不要仅写",
        "别仅写",
        "不能仅写",
        "不止写",
        "不仅写",
        "notjustwrite",
        "notonlywrite",
        "donotjustwrite",
        "don'tjustwrite",
    ]
    .iter()
    .any(|term| compact.contains(term) || lowered.contains(term))
        || clause_negates_limited_unit_generation(&compact, &lowered)
}

fn clause_negates_limited_unit_generation(compact: &str, lowered: &str) -> bool {
    let limited_markers = ["只写", "仅写", "只生成", "仅生成", "只创作", "仅创作"];
    let negation_markers = [
        "不要", "别", "不能", "不应", "不是", "并非", "不许", "不可", "禁止",
    ];
    let chinese_negated = limited_markers.iter().any(|limited| {
        compact.find(limited).is_some_and(|limited_index| {
            negation_markers.iter().any(|negation| {
                compact
                    .find(negation)
                    .is_some_and(|negation_index| negation_index < limited_index)
            })
        })
    });
    if chinese_negated {
        return true;
    }
    let english_limited = ["justwrite", "onlywrite", "justgenerate", "onlygenerate"];
    let english_negation = ["not", "donot", "don't", "cannot", "cant", "never"];
    english_limited.iter().any(|limited| {
        lowered.find(limited).is_some_and(|limited_index| {
            english_negation.iter().any(|negation| {
                lowered
                    .find(negation)
                    .is_some_and(|negation_index| negation_index < limited_index)
            })
        })
    })
}

fn prefix_has_generation_unit_action(prefix: &str) -> bool {
    let tail = prefix
        .rsplit(|ch| matches!(ch, '，' | ',' | '。' | ';' | '；' | '\n'))
        .next()
        .unwrap_or(prefix)
        .chars()
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let lowered = tail.to_ascii_lowercase();
    [
        "写", "生成", "创作", "产出", "完成", "跑", "先", "再", "开始", "continue", "write",
        "draft", "generate",
    ]
    .iter()
    .any(|term| tail.contains(term) || lowered.contains(term))
}

fn prefix_looks_like_existing_unit_reference(prefix: &str) -> bool {
    let clause = prefix
        .rsplit(|ch| matches!(ch, '，' | ',' | '。' | ';' | '；' | '\n'))
        .next()
        .unwrap_or(prefix);
    [
        "已批准前",
        "已通过前",
        "已完成前",
        "已生成前",
        "已写前",
        "已有前",
        "不要重写前",
        "不要再写前",
        "不能重写前",
        "别重写前",
        "不重写前",
        "前文",
        "前面",
        "前序",
        "previous",
        "approved previous",
    ]
    .iter()
    .any(|term| clause.contains(term) || clause.to_ascii_lowercase().contains(term))
}

pub fn trailing_unit_count(prefix: &str) -> Option<usize> {
    let trimmed = prefix.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let chars = trimmed.chars().collect::<Vec<_>>();
    let mut start = chars.len();
    while start > 0 && (chars[start - 1].is_ascii_digit() || is_cjk_number_char(chars[start - 1])) {
        start -= 1;
    }
    if start == chars.len() {
        return None;
    }
    if start > 0 && chars[start - 1] == '第' {
        return None;
    }
    let number_text = chars[start..].iter().collect::<String>();
    if number_text.chars().all(|ch| ch.is_ascii_digit()) {
        number_text.parse::<usize>().ok()
    } else {
        parse_cjk_cardinal(&number_text)
    }
}

pub fn collect_english_unit_count(message: &str, markers: &[&str], out: &mut Vec<usize>) {
    let tokens = message
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect::<Vec<_>>();
    for window in tokens.windows(2) {
        let [count, unit] = window else {
            continue;
        };
        if !markers.iter().any(|marker| unit == marker) {
            continue;
        }
        if let Some(value) = parse_english_small_count(count) {
            out.push(value);
        }
    }
}

pub fn parse_english_small_count(value: &str) -> Option<usize> {
    match value {
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        _ => value.parse::<usize>().ok(),
    }
}

pub fn approval_requests_first_writing_unit(message: &str, artifact_kind: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    if artifact_kind == "fiction" {
        return [
            "第一章",
            "第1章",
            "先写一章",
            "写一章",
            "一章",
            "本章",
            "first chapter",
            "chapter one",
        ]
        .iter()
        .any(|term| message.contains(term) || lowered.contains(term));
    }
    [
        "第一节",
        "第1节",
        "第一部分",
        "第1部分",
        "先写一节",
        "先写一部分",
        "本节",
        "first section",
        "first part",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(term))
}

pub fn creation_draft_requests_all_remaining(message: &str, artifact_kind: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    let generic_terms = [
        "直接生成完",
        "直接写完",
        "全部生成",
        "全部写完",
        "一口气写完",
        "生成完整",
        "写完整",
        "完成全文",
        "完成全书",
        "写完全书",
        "继续完成",
        "完成当前",
        "完成这本",
        "完整结局",
        "完整结尾",
        "真正结尾",
        "写到结尾",
        "直到结尾",
        "自然结尾",
        "剩下的都写完",
        "后面都写完",
        "complete ending",
        "finish current",
        "finish this",
        "finish all",
        "finish the rest",
        "complete the rest",
        "complete it",
    ];
    if generic_terms
        .iter()
        .any(|term| message_contains_positive_operation_term(message, &lowered, term))
    {
        return true;
    }
    if requests_complete_whole_artifact(message, artifact_kind) {
        return true;
    }
    if artifact_kind == "fiction" {
        ["全书", "整本", "全文", "完结"]
            .iter()
            .any(|term| message_contains_positive_operation_term(message, &lowered, term))
    } else {
        ["全文", "整篇", "完整文档"]
            .iter()
            .any(|term| message_contains_positive_operation_term(message, &lowered, term))
    }
}

fn requests_complete_whole_artifact(message: &str, artifact_kind: &str) -> bool {
    let artifact_terms: &[&str] = if artifact_kind == "fiction" {
        &[
            "小说", "故事", "作品", "正文", "书", "novel", "story", "book",
        ]
    } else {
        &[
            "文章", "论文", "报告", "文档", "正文", "作品", "article", "paper", "report",
            "document",
        ]
    };
    message
        .split(|ch| {
            matches!(
                ch,
                '，' | ',' | '。' | '.' | '！' | '!' | '？' | '?' | '；' | ';' | '\n' | '\r'
            )
        })
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .any(|clause| {
            let clause_lowered = clause.to_ascii_lowercase();
            let completion_operation = [
                "完成",
                "写完",
                "生成完",
                "创作完",
                "finish",
                "complete",
                "write all",
            ]
            .iter()
            .any(|term| message_contains_positive_operation_term(clause, &clause_lowered, term));
            let whole_scope = [
                "整部", "整篇", "整本", "全部", "所有", "剩余", "完整", "entire", "whole", "all",
                "rest",
            ]
            .iter()
            .any(|term| clause.contains(term) || clause_lowered.contains(term));
            completion_operation
                && whole_scope
                && artifact_terms
                    .iter()
                    .any(|term| clause.contains(term) || clause_lowered.contains(term))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_chapter_cadence_does_not_cancel_explicit_full_book_scope() {
        let message = "总字数10万字，每章2500字，每次只写一章，待我确认后自动连续写完整本并保存。";

        assert_eq!(
            creation_draft_turn_scope(message, "fiction"),
            CreationDraftTurnScope::AllRemaining
        );
        let note = creation_execution_scope_note(message, "fiction").expect("scope note");
        assert_eq!(
            persisted_creation_execution_scope(&[note]),
            Some(CreationDraftTurnScope::AllRemaining)
        );
    }

    #[test]
    fn chapter_completion_status_does_not_combine_with_negated_whole_book_scope() {
        let message = "本轮范围：用户本轮只要求先写第一章；不要因为总目标字数存在而连续生成全书，完成本章后返回进度。";

        assert!(!creation_draft_requests_all_remaining(message, "fiction"));
        assert_eq!(
            creation_draft_turn_scope(message, "fiction"),
            CreationDraftTurnScope::FirstUnit
        );
    }

    #[test]
    fn negated_do_not_convert_task_to_first_three_chapters_keeps_full_book_scope() {
        let message = "请从零创建并完整写完一本中文重生都市题材长篇小说。总字数10万字，选择每章2500字档；先生成完整合同让我确认，确认后请自动连续写作、审稿、保存并导出整本书，不要在前三章停止，也不要把任务改成只写前三章。";

        assert_eq!(
            creation_draft_turn_scope(message, "fiction"),
            CreationDraftTurnScope::AllRemaining
        );
        let note = creation_execution_scope_note(message, "fiction").expect("scope note");
        assert_eq!(
            persisted_creation_execution_scope(&[note]),
            Some(CreationDraftTurnScope::AllRemaining)
        );
    }

    #[test]
    fn continuous_full_book_request_starting_at_chapter_one_is_not_reduced_to_one_chapter() {
        let message = "按这个合同开始写。请从第一章起持续自动写作、审稿并保存，直到完成整本10万字小说；不要把任务截成前十章。";

        assert_eq!(
            creation_draft_turn_scope(message, "fiction"),
            CreationDraftTurnScope::AllRemaining
        );
    }
}
