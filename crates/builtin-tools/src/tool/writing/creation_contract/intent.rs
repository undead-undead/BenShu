use super::*;

mod content_ops;
mod field_extract;
mod intake;
mod turn_scope;

pub use content_ops::*;
pub use field_extract::*;
pub use intake::*;
pub use turn_scope::{
    creation_draft_requested_turn_units, creation_draft_requests_all_remaining,
    creation_draft_turn_scope, creation_execution_scope_note, persisted_creation_execution_scope,
    CreationDraftTurnScope, CREATION_EXECUTION_SCOPE_NOTE_PREFIX, FICTION_EXPLICIT_TURN_UNITS_MAX,
};

pub fn creation_draft_execution_requested(message: &str, artifact_kind: &str) -> bool {
    creation_draft_execution_requested_for_intent(
        message,
        artifact_kind,
        classify_creation_draft_turn_intent(message),
    )
}

pub(super) fn creation_draft_execution_requested_for_intent(
    message: &str,
    artifact_kind: &str,
    turn_intent: CreationDraftTurnIntent,
) -> bool {
    if matches!(turn_intent, CreationDraftTurnIntent::DeferStart) {
        return false;
    }
    if matches!(
        turn_intent,
        CreationDraftTurnIntent::ClarifyOrPlan | CreationDraftTurnIntent::UpdateContract
    ) {
        return false;
    }
    if matches!(turn_intent, CreationDraftTurnIntent::ApproveAndStart)
        || creation_draft_requests_all_remaining(message, artifact_kind)
    {
        return true;
    }
    let lowered = message.to_ascii_lowercase();
    let generic_terms = [
        "继续写",
        "接着写",
        "继续生成",
        "接着生成",
        "继续推进",
        "下一章",
        "下章",
        "下一节",
        "下一部分",
        "再写",
        "continue",
        "next chapter",
        "next section",
        "keep writing",
    ];
    generic_terms
        .iter()
        .any(|term| message_contains_positive_operation_term(message, &lowered, term))
}

pub fn creation_draft_modification_requested(message: &str) -> bool {
    creation_contract_repair_only_message(message)
        || requested_title(message).is_some()
        || creation_draft_requests_generated_title_revision(message)
        || requested_total_unit_target(message).is_some()
        || requested_chapter_unit_target(message).is_some()
        || requested_section_unit_target(message).is_some()
        || requested_max_chapters_per_turn(message).is_some()
        || requested_export_format(message).is_some()
        || [
            "改成",
            "调整",
            "设置",
            "改为",
            "换成",
            "改一下",
            "改下",
            "变成",
            "更新",
            "修改",
            "修订",
            "修正",
            "纠正",
            "更正",
            "补充",
            "完善",
        ]
        .iter()
        .any(|term| message.contains(term))
}

pub fn creation_draft_approval_succeeded(approved: &Value) -> bool {
    approved
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

pub fn creation_draft_approval_title_conflicted(approved: &Value) -> bool {
    approved
        .get("error")
        .and_then(|value| value.as_str())
        .is_some_and(|error| error == "title_conflict")
}

pub fn creation_draft_approval_failure_response(approved: &Value) -> String {
    if approved
        .get("error")
        .and_then(|value| value.as_str())
        .is_some_and(|error| error == "title_conflict")
    {
        let title = approved
            .get("title")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("当前标题");
        return format!(
            "标题《{title}》已经存在，所以我没有新建同名写作项目。\n\n如果要继续这个已有项目，请回复“继续已有项目”；如果要新建一本不同的书，可以回复新的书名，也可以让我根据当前合同重新取一个不同的新书名。"
        );
    }

    let issue_count = approved
        .get("issues")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if issue_count > 0 {
        format!(
            "创作草案暂时没有批准成功：合同还有 {issue_count} 项质量门未通过。本轮没有创建项目，也没有开始正文。"
        )
    } else {
        "创作草案暂时没有批准成功。本轮没有创建项目，也没有开始正文。".to_string()
    }
}

pub fn project_path_from_approved_creation_draft(approved: &Value) -> Option<String> {
    for pointer in [
        "/project_path",
        "/init/project_path",
        "/contract/project_path",
    ] {
        if let Some(path) = approved.pointer(pointer).and_then(|value| value.as_str()) {
            let path = path.trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

pub fn sync_creation_draft_from_approval(
    draft: &mut SessionCreationDraftState,
    approved: &Value,
) -> bool {
    let Some(approved_draft) = approved.get("draft") else {
        return false;
    };
    let mut authoritative_contract = approved_draft
        .get("authority_contract")
        .and_then(|value| serde_json::from_value::<NovelCreationContract>(value.clone()).ok());
    let previous_characters = draft.fiction_characters.clone();
    let previous_current_contract_characters =
        character_lines_from_current_contract(draft.current_contract.as_ref());
    let user_chapter_unit_target = draft.user_chapter_unit_target();
    let mut changed = false;
    changed |= sync_string_field(approved_draft, "title", &mut draft.title);
    changed |= sync_string_field(approved_draft, "language", &mut draft.language);
    changed |= sync_string_field(approved_draft, "genre", &mut draft.genre);
    changed |= sync_string_field(approved_draft, "brief", &mut draft.brief);
    changed |= sync_string_field(approved_draft, "export_format", &mut draft.export_format);
    changed |= sync_option_usize_field(approved_draft, "target_units", &mut draft.target_units);
    changed |= sync_option_usize_field(
        approved_draft,
        "chapter_unit_target",
        &mut draft.chapter_unit_target,
    );
    if let Some(target) = user_chapter_unit_target {
        changed |= draft.chapter_unit_target != Some(target);
        draft.chapter_unit_target = Some(target);
        draft.chapter_unit_target_user_authority = Some(target);
    }
    changed |= sync_option_usize_field(
        approved_draft,
        "max_chapters_per_turn",
        &mut draft.max_chapters_per_turn,
    );
    changed |= sync_bool_field(
        approved_draft,
        "export_when_complete",
        &mut draft.export_when_complete,
    );
    changed |= sync_bool_field(approved_draft, "approved_only", &mut draft.approved_only);
    changed |= sync_string_field(approved_draft, "premise", &mut draft.fiction_premise);
    changed |= sync_string_field(
        approved_draft,
        "ending_direction",
        &mut draft.fiction_ending_direction,
    );
    changed |= sync_string_field(
        approved_draft,
        "protagonist_arc",
        &mut draft.fiction_protagonist_arc,
    );
    changed |= sync_string_field(
        approved_draft,
        "world_imagery",
        &mut draft.fiction_world_imagery,
    );
    changed |= sync_string_field(
        approved_draft,
        "main_causal_spine",
        &mut draft.fiction_main_causal_spine,
    );
    changed |= sync_string_field(
        approved_draft,
        "title_rationale",
        &mut draft.fiction_title_rationale,
    );
    changed |= sync_string_vec_field(approved_draft, "themes", &mut draft.fiction_themes);
    changed |= sync_string_vec_field(approved_draft, "characters", &mut draft.fiction_characters);
    changed |= sync_string_vec_field(
        approved_draft,
        "world_rules",
        &mut draft.fiction_world_rules,
    );
    changed |= sync_string_vec_field(
        approved_draft,
        "style_rules",
        &mut draft.fiction_style_rules,
    );
    changed |= sync_string_vec_field(approved_draft, "must_avoid", &mut draft.fiction_must_avoid);
    changed |= sync_string_field(approved_draft, "outline", &mut draft.fiction_outline);
    if let Some(value) = approved_draft.get("structured_contract_v2") {
        if let Ok(contract) = serde_json::from_value::<NovelContractV2>(value.clone()) {
            let before = serde_json::to_value(draft.contract_v2()).ok();
            draft.set_contract_v2(contract);
            changed |= before != serde_json::to_value(draft.contract_v2()).ok();
        }
    }
    let mut restored_authority_contract = false;
    if let Some(contract) = authoritative_contract.as_mut() {
        let before = serde_json::to_value(&*draft).ok();
        apply_strong_novel_contract_to_creation_draft(draft, contract);
        contract.normalize();
        if let Ok(value) = serde_json::to_value(contract) {
            draft.current_contract = Some(value);
            restored_authority_contract = true;
        }
        changed |= before != serde_json::to_value(&*draft).ok();
    }
    if changed || restored_authority_contract {
        if !previous_characters.is_empty() && previous_characters != draft.fiction_characters {
            let governed_characters = draft.fiction_characters.clone();
            align_fiction_contract_text_to_governed_characters(
                draft,
                &previous_characters,
                &governed_characters,
            );
        }
        if !previous_current_contract_characters.is_empty()
            && previous_current_contract_characters != draft.fiction_characters
        {
            let governed_characters = draft.fiction_characters.clone();
            align_fiction_contract_text_to_governed_characters(
                draft,
                &previous_current_contract_characters,
                &governed_characters,
            );
        }
        normalize_fiction_creation_draft_after_contract_change(draft);
        sanitize_creation_draft_control_noise(draft);
        if !restored_authority_contract {
            changed |= rebuild_current_contract_from_visible_draft(draft);
        }
        draft.refresh_contract_status_from_validation();
        draft.updated_at = chrono::Utc::now().to_rfc3339();
    }
    changed
}

fn character_lines_from_current_contract(contract: Option<&Value>) -> Vec<String> {
    let Some(characters) = contract
        .and_then(|value| value.get("characters"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    characters
        .iter()
        .filter_map(|character| {
            let name = character
                .get("canonical_name")
                .or_else(|| character.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            let role = character
                .get("role")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("角色");
            Some(format!("name: {name}; role: {role}"))
        })
        .collect()
}

pub(crate) fn rebuild_current_contract_from_visible_draft(
    draft: &mut SessionCreationDraftState,
) -> bool {
    if draft.artifact_kind != "fiction" {
        return false;
    }
    let mut contract = strong_novel_contract_from_visible_creation_draft(draft);
    contract.normalize();
    let Ok(value) = serde_json::to_value(contract) else {
        return false;
    };
    if draft.current_contract.as_ref() == Some(&value) {
        return false;
    }
    draft.current_contract = Some(value);
    true
}

fn sync_string_field(source: &Value, key: &str, target: &mut String) -> bool {
    let Some(value) = source
        .get(key)
        .and_then(Value::as_str)
        .map(sanitize_generated_contract_scalar)
        .filter(|value| !value.trim().is_empty())
    else {
        return false;
    };
    if *target == value {
        return false;
    }
    *target = value;
    true
}

fn sync_string_vec_field(source: &Value, key: &str, target: &mut Vec<String>) -> bool {
    let Some(items) = source.get(key).and_then(Value::as_array) else {
        return false;
    };
    let next = items
        .iter()
        .filter_map(Value::as_str)
        .map(sanitize_generated_contract_scalar)
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>();
    if next.is_empty() || *target == next {
        return false;
    }
    *target = next;
    true
}

fn sync_option_usize_field(source: &Value, key: &str, target: &mut Option<usize>) -> bool {
    let Some(value) = source
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if *target == Some(value) {
        return false;
    }
    *target = Some(value);
    true
}

fn sync_bool_field(source: &Value, key: &str, target: &mut bool) -> bool {
    let Some(value) = source.get(key).and_then(Value::as_bool) else {
        return false;
    };
    if *target == value {
        return false;
    }
    *target = value;
    true
}

pub fn text_has_any(value: &str, terms: &[&str]) -> bool {
    let lowered = value.to_ascii_lowercase();
    terms
        .iter()
        .any(|term| value.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub fn merge_short_field(existing: &str, incoming: &str) -> String {
    let existing = existing.trim();
    let incoming = incoming.trim();
    if incoming.is_empty() || existing == incoming {
        existing.to_string()
    } else if existing.is_empty() {
        incoming.to_string()
    } else if existing.contains(incoming) {
        existing.to_string()
    } else if incoming.contains(existing) {
        incoming.to_string()
    } else {
        format!("{existing}；{incoming}")
    }
}

pub fn merge_list(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut merged = existing.to_vec();
    for item in incoming {
        if !merged.iter().any(|existing| existing == item) {
            merged.push(item.clone());
        }
    }
    merged
}

pub fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

pub fn empty_display<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    non_empty_or(value, fallback)
}

pub fn creation_kind_label(kind: &str) -> &'static str {
    match kind {
        "fiction" => "小说",
        "paper" => "论文",
        "report" => "报告",
        _ => "写作产物",
    }
}

fn quoted_segments(content: &str) -> Vec<String> {
    let mut values = Vec::new();
    let pairs = [
        ('《', '》'),
        ('「', '」'),
        ('“', '”'),
        ('"', '"'),
        ('\'', '\''),
    ];
    for (left, right) in pairs {
        let mut rest = content;
        while let Some((_, after_left)) = rest.split_once(left) {
            let Some((value, after_right)) = after_left.split_once(right) else {
                break;
            };
            let value = value.trim();
            if !value.is_empty() {
                values.push(value.to_string());
            }
            rest = after_right;
        }
        if !values.is_empty() {
            break;
        }
    }
    values
}

pub(crate) fn intent_requests_read_only_existing_artifact_answer(intent: &str) -> bool {
    let lowered = intent.to_ascii_lowercase();
    if creation_draft_message_requests_continuation_generation(intent, &lowered) {
        return false;
    }
    let segment_followup = intent_requests_existing_artifact_segment_answer(intent, &lowered);
    let read_surface = [
        "检查",
        "总结",
        "概括",
        "说明",
        "告诉我",
        "看一下",
        "查看",
        "读取",
        "主角",
        "内容",
        "路径",
        "在哪",
        "哪里",
        "什么",
        "谁",
        "讲",
        "大概",
        "是否完成",
        "完成了吗",
        "完成了没",
        "完成没",
        "写好了吗",
        "写好了没",
        "写好没",
        "已完成",
        "完成到",
        "第几章",
        "进度",
        "状态",
        "总字数",
        "章节数",
        "最后一章",
        "导出路径",
        "summarize",
        "summary",
        "tell me",
        "what is",
        "who is",
        "where",
        "path",
        "read",
        "inspect",
        "status",
        "progress",
        "done",
        "complete",
    ];
    let existing_surface = [
        "当前",
        "这本",
        "刚才",
        "刚刚",
        "上次",
        "上一轮",
        "之前",
        "前面",
        "已经",
        "已完成",
        "完成到",
        "已生成",
        "生成的",
        "保存的",
        "导出的",
        "这个",
        "那个",
        "current",
        "previous",
        "last",
        "already",
        "existing",
        "generated",
        "saved",
        "exported",
    ];
    let has_read_surface = read_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term));
    let has_existing_surface = existing_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term));
    if !has_read_surface || (!has_existing_surface && !segment_followup) {
        return false;
    }

    let mutation_check_surface = intent_with_negated_mutations_removed(intent);
    let mutation_check_lowered = mutation_check_surface.to_ascii_lowercase();
    let new_mutation_surface = [
        "重新写",
        "重新生成",
        "重新创建",
        "再写",
        "再生成",
        "继续写",
        "继续处理",
        "继续这本",
        "接着写",
        "续写",
        "写到",
        "写完",
        "完整结尾",
        "收束",
        "补足",
        "补全",
        "另写",
        "新写",
        "只修",
        "修一下",
        "修好",
        "修正",
        "修复",
        "修改",
        "修订",
        "改写",
        "润色",
        "扩写",
        "调整",
        "更名",
        "改名",
        "重命名",
        "保存成",
        "导出为",
        "做成",
        "rewrite",
        "regenerate",
        "write another",
        "continue writing",
        "revise",
        "edit",
        "polish",
        "expand",
        "export as",
        "save as",
    ];
    !new_mutation_surface
        .iter()
        .any(|term| mutation_check_surface.contains(term) || mutation_check_lowered.contains(term))
}

fn intent_with_negated_mutations_removed(intent: &str) -> String {
    let mut normalized = intent.to_string();
    for phrase in [
        "不要继续写新章节",
        "不要继续写下一章",
        "不要继续写",
        "不用继续写",
        "无需继续写",
        "别继续写",
        "不要再写",
        "不用再写",
        "无需再写",
        "别再写",
        "不要生成新章节",
        "不用生成新章节",
        "无需生成新章节",
        "别生成新章节",
        "do not continue writing",
        "don't continue writing",
        "do not write another",
        "don't write another",
        "do not generate a new",
        "don't generate a new",
    ] {
        normalized = normalized.replace(phrase, " ");
    }
    normalized
}

fn intent_requests_existing_artifact_segment_answer(intent: &str, lowered: &str) -> bool {
    if referenced_artifact_segment_numbers(intent).is_empty() {
        return false;
    }
    let segment_surface = [
        "章", "章节", "节", "部分", "段", "chapter", "section", "part", "segment",
    ];
    segment_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term))
}

pub(crate) fn referenced_artifact_segment_numbers(intent: &str) -> Vec<usize> {
    let mut numbers = Vec::new();
    collect_arabic_segment_numbers(intent, &mut numbers);
    collect_cjk_segment_numbers(intent, &mut numbers);
    numbers.retain(|value| (1..=500).contains(value));
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

fn collect_arabic_segment_numbers(intent: &str, numbers: &mut Vec<usize>) {
    let chars = intent.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
        let value = chars[start..index].iter().collect::<String>();
        let Ok(number) = value.parse::<usize>() else {
            continue;
        };
        let next_non_space = chars[index..]
            .iter()
            .copied()
            .find(|ch| !ch.is_whitespace());
        let previous_text = chars[..start]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        let previous_word_is_segment = previous_text.trim_end().ends_with("chapter")
            || previous_text.trim_end().ends_with("section")
            || previous_text.trim_end().ends_with("part");
        if matches!(next_non_space, Some('章' | '节' | '段')) || previous_word_is_segment {
            numbers.push(number);
        }
    }
}

fn collect_cjk_segment_numbers(intent: &str, numbers: &mut Vec<usize>) {
    let chars = intent.chars().collect::<Vec<_>>();
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '第' {
            continue;
        }
        let mut end = index + 1;
        while end < chars.len() && is_cjk_number_char(chars[end]) {
            end += 1;
        }
        if end == index + 1 {
            continue;
        }
        let marker = chars.get(end).copied();
        if !matches!(marker, Some('章' | '节' | '段')) {
            continue;
        }
        let value = chars[index + 1..end].iter().collect::<String>();
        if let Some(number) = parse_cjk_cardinal(&value) {
            numbers.push(number);
        }
    }
}

pub(super) fn is_cjk_number_char(ch: char) -> bool {
    matches!(
        ch,
        '零' | '〇' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
    )
}

fn parse_cjk_cardinal(value: &str) -> Option<usize> {
    if value.is_empty() {
        return None;
    }
    if let Some((left, right)) = value.split_once('十') {
        let tens = if left.is_empty() {
            1
        } else {
            cjk_digit_value(left.chars().next()?)?
        };
        let ones = if right.is_empty() {
            0
        } else {
            cjk_digit_value(right.chars().next()?)?
        };
        return Some(tens * 10 + ones);
    }
    let mut number = 0usize;
    for ch in value.chars() {
        number = number
            .saturating_mul(10)
            .saturating_add(cjk_digit_value(ch)?);
    }
    Some(number)
}

fn cjk_digit_value(ch: char) -> Option<usize> {
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

pub(super) fn is_cjk_unified(ch: char) -> bool {
    matches!(
        ch,
        '\u{4e00}'..='\u{9fff}'
            | '\u{3400}'..='\u{4dbf}'
            | '\u{20000}'..='\u{2a6df}'
            | '\u{2a700}'..='\u{2b73f}'
            | '\u{2b740}'..='\u{2b81f}'
            | '\u{2b820}'..='\u{2ceaf}'
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn genre_sanitizer_removes_list_marker_and_size_tail() {
        assert_eq!(
            super::sanitize_creation_genre_value("异界修仙小说3.作品字数：至少5万字，每章约2500字"),
            "异界修仙小说"
        );
    }

    #[test]
    fn read_only_progress_question_does_not_continue_next_chapter() {
        let message =
            "任务进度。只告诉我已完成到第几章、能否继续下一章；不要展示JSON、内部路径或工具参数。";
        let lowered = message.to_ascii_lowercase();

        assert!(!super::creation_draft_message_requests_continuation_generation(message, &lowered));
    }

    #[test]
    fn explicit_next_chapter_request_still_continues_generation() {
        let message = "继续写下一章。保持角色名和伏笔连续。";
        let lowered = message.to_ascii_lowercase();

        assert!(super::creation_draft_message_requests_continuation_generation(message, &lowered));
    }
}
