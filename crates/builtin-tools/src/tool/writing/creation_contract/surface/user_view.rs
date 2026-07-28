use super::*;
use crate::tool::writing::novel_contract_v2::{
    ChapterEndingRotation, CharacterVoiceProfile, ConflictPressureCurve, MotifLedgerEntry,
    ReaderPromise, RelationshipInteractionQuota, RevealScheduleEntry, SceneTypeMix,
};

pub const CREATION_CONTRACT_QUALITY_BLOCKED_METADATA_KEY: &str =
    "creation_contract_quality_blocked";

pub fn stabilize_creation_contract_user_response(
    draft: &SessionCreationDraftState,
    response: &str,
) -> String {
    if draft.artifact_kind == "fiction" {
        let surface = CreationContractSurfaceState::from_draft(draft);
        if !surface.confirmable {
            let mut text = render_creation_draft_compact_status(draft);
            if !surface.issues.is_empty() {
                text.push_str("\n- 当前缺口：");
                text.push_str(&creation_contract_issue_summary(&surface.issues));
            }
            return format!(
                "写作需求摘要还在补齐为正式合同，我还没有开始写正文。\n\n当前需求摘要（不可确认）：\n{text}\n\n只有可展示合同通过质量门后，才会显示为“可确认合同”。你可以继续用自然语言补充或修改。"
            );
        }
        if surface.lifecycle == CreationDraftLifecycleStatus::ContractReady {
            let contract = creation_draft_planning_response_text(draft, "完整合同");
            return format!("可确认合同：\n\n{contract}");
        }
        return creation_draft_planning_response_text(draft, "完整合同");
    }

    let sanitized = sanitize_generated_contract_surface(draft, response);
    let trimmed = sanitized.trim();
    let mut sections = vec![
        "下面是待确认的写作文档合同草案。我还没有开始写完整正文；你可以继续修改，确认后再写。"
            .to_string(),
    ];
    if trimmed.is_empty() {
        sections.push(creation_draft_planning_response_text(
            draft,
            "显示当前合同草案",
        ));
    } else {
        sections.push(trimmed.to_string());
    }
    if !trimmed.contains("开始写")
        && !trimmed.contains("按这个开始")
        && !trimmed.to_ascii_lowercase().contains("start")
    {
        sections.push(format!(
            "下一步：如果合同还不满意，可以直接说要改哪里；{}",
            creation_draft_next_action_text(draft, true)
        ));
    }
    sections.join("\n\n")
}

pub fn creation_contract_quality_blocked_response(issues: &[String]) -> String {
    let summary = creation_contract_issue_summary(issues);
    format!(
        "合同草案还没有补齐到可确认状态，暂时不能开始写正文。\n\n还需要补齐：{summary}。\n\n我没有把这版草案当作可确认合同保存；你可以继续用自然语言补充要求，或者让我重新补齐合同。书名、角色名、卷名会继续按大纲、情节链、终局和世界观意象来生成。"
    )
}

pub fn creation_draft_planning_response_text(
    draft: &SessionCreationDraftState,
    latest_user: &str,
) -> String {
    let status = render_creation_draft_compact_status(draft);
    let ready_to_start = draft.artifact_kind == "fiction"
        && matches!(
            draft.lifecycle_status(),
            CreationDraftLifecycleStatus::ContractReady
                | CreationDraftLifecycleStatus::Approved
                | CreationDraftLifecycleStatus::Writing
        )
        && creation_draft_contract_blocking_issues_for_scope(
            draft,
            ContractReadinessScope::LockedAuthorityContract,
        )
        .is_empty();
    let asks_full_view =
        draft.artifact_kind == "fiction" || intent_requests_creation_contract_view(latest_user);
    let full_contract_requested = draft.artifact_kind == "fiction"
        || intent_requests_full_creation_contract_view(latest_user);
    let outline_payload = if asks_full_view {
        let outline = creation_planning_outline_payload(draft);
        let contract_view = render_creation_draft_contract_view(draft, full_contract_requested);
        if full_contract_requested {
            if contract_view.trim().is_empty() {
                String::new()
            } else {
                format!("\n\n{contract_view}")
            }
        } else if outline.trim().is_empty() {
            if contract_view.trim().is_empty() {
                String::new()
            } else {
                format!("\n\n{contract_view}")
            }
        } else {
            format!("\n\n{contract_view}\n\n当前大纲素材：\n{outline}")
        }
    } else {
        String::new()
    };

    if draft.artifact_kind == "fiction" {
        let next_action = creation_draft_next_action_text(draft, ready_to_start);
        format!(
            "可以，我还没有开始写正文；我先把小说方向自动整理成待确认、可修改草案。这个草案不算正文，后续你可以继续修改。\n\n当前草案：\n{status}{outline_payload}\n\n我会自动补齐书名、角色、结局、分卷/阶段和近期章节包。你不需要填写字段，只要像聊天一样说“更热血一点”“主角换成女性”“每章改成5000档”就行。小说每章字数支持 {}，非档位会自动归一到最近档位。\n\n{next_action}",
            longform_policy::novel_chapter_unit_band_label()
        )
    } else {
        format!(
            "可以，我先不写完整正文，先把写作文档合同定下来。\n\n当前草案：\n{status}\n\n接下来请补充或选择这 3 件事：\n1. 文档类型和主题：论文、报告、文章、综述等。\n2. 读者和用途：给谁看，要解决什么问题。\n3. 结构和证据：需要哪些章节、引用、材料或知识库内容。\n\n如果这个草案已经可以，就回复“开始写”或“按这个开始”，我再进入正式写作并把正文保存到文件。"
        )
    }
}

pub fn creation_draft_next_action_text(
    draft: &SessionCreationDraftState,
    ready_to_start: bool,
) -> &'static str {
    if draft.artifact_kind == "fiction" {
        if ready_to_start {
            "如果方向已经可以，就回复“开始写”或“按这个开始”，我再进入正式写作并把正文保存到文件。"
        } else {
            "我会先自动补齐合同缺口；补齐并通过质量门后，你再回复“开始写”或“按这个开始”，我才会进入正式写作。"
        }
    } else {
        "如果这个草案已经可以，就回复“开始写”或“按这个开始”，我再进入正式写作并把正文保存到文件。"
    }
}

pub fn render_creation_draft_compact_status(draft: &SessionCreationDraftState) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "- 类型：{}",
        creation_kind_label(&draft.artifact_kind)
    ));
    if !draft.title.trim().is_empty() {
        lines.push(format!(
            "- 标题：{}",
            visible_creation_title_or_pending(&draft.title)
        ));
    }
    lines.push(format!(
        "- 语言：{}",
        empty_display(&draft.language, "未指定")
    ));
    if draft.artifact_kind == "fiction" {
        lines.push(format!("- 题材：{}", empty_display(&draft.genre, "未指定")));
        lines.push(format!(
            "- 简述：{}",
            if draft.brief.trim().is_empty() {
                "未指定".to_string()
            } else {
                compact_creation_text(&draft.brief, 240)
            }
        ));
        if let Some(target) = draft.target_units {
            lines.push(format!("- 总目标字数：{target}"));
        }
        if let Some(target) = draft.chapter_unit_target {
            lines.push(format!(
                "- 每章目标档位：{target}（小说每章字数仅支持 {}）",
                longform_policy::novel_chapter_unit_band_label()
            ));
        } else if draft.artifact_kind == "fiction" {
            lines.push(format!(
                "- 每章目标档位：未指定（小说每章字数可选 {}）",
                longform_policy::novel_chapter_unit_band_label()
            ));
        }
        if let Some(turns) = draft.max_chapters_per_turn {
            lines.push(format!("- 每轮章节数：{turns}"));
        }
        if let Some(expected) =
            draft
                .target_units
                .zip(draft.chapter_unit_target)
                .and_then(|(total, per_chapter)| {
                    longform_policy::expected_chapter_count(total, per_chapter)
                })
        {
            lines.push(format!("- 预计章节数：约 {expected} 章"));
        }
        let structured_status = render_structured_contract_v2_status(draft, false);
        if !structured_status.trim().is_empty() {
            lines.push(format!("- 结构化合同摘要：{structured_status}"));
        }
    } else {
        lines.push(format!(
            "- 文档类型：{}",
            empty_display(&draft.document_type, "未指定")
        ));
        lines.push(format!(
            "- 受众：{}",
            empty_display(&draft.audience, "未指定")
        ));
        lines.push(format!(
            "- 目的：{}",
            empty_display(&draft.purpose, "未指定")
        ));
        lines.push(format!(
            "- 论点/前提：{}",
            empty_display(&draft.thesis_or_premise, "未指定")
        ));
        if let Some(target) = draft.target_units {
            lines.push(format!("- 目标字数：{target}"));
        }
    }
    let planning_notes = stable_creation_planning_notes(draft);
    if !planning_notes.is_empty() {
        lines.push(format!(
            "- 已记录设定：{}",
            planning_notes
                .iter()
                .take(5)
                .map(|note| compact_creation_text(note, 90))
                .collect::<Vec<_>>()
                .join("；")
        ));
    }
    lines.push(format!("- 导出格式：{}", draft.export_format));
    lines.join("\n")
}

pub fn render_creation_draft_contract_view(
    draft: &SessionCreationDraftState,
    full: bool,
) -> String {
    if draft.artifact_kind != "fiction" {
        return String::new();
    }
    if full {
        return render_full_fiction_contract_view(draft);
    }
    let body = render_structured_contract_v2_status(draft, full);
    if body.trim().is_empty() {
        return "结构化合同摘要：完整合同仍在补齐中。".to_string();
    }
    format!("结构化合同摘要：{body}")
}

pub(crate) fn render_full_fiction_contract_view(draft: &SessionCreationDraftState) -> String {
    let mut lines = Vec::new();
    let locked_issues = creation_draft_contract_blocking_issues_for_scope(
        draft,
        ContractReadinessScope::LockedAuthorityContract,
    );
    if locked_issues.is_empty() {
        lines.push("标准小说合同草案（确认前可自然语言修改）：".to_string());
    } else {
        lines.push("小说合同草案（补齐中，不可确认）：".to_string());
        let mut known = Vec::new();
        if !draft.title.trim().is_empty() {
            known.push(format!(
                "- 标题：{}",
                visible_creation_title_or_pending(&draft.title)
            ));
        }
        if !draft.genre.trim().is_empty() {
            known.push(format!("- 题材：{}", contract_view_text(&draft.genre, 120)));
        }
        if !draft.brief.trim().is_empty() {
            known.push(format!("- 简述：{}", contract_view_text(&draft.brief, 180)));
        }
        if let Some(target) = draft.target_units {
            known.push(format!("- 总目标字数：{target}"));
        }
        if let Some(target) = draft.chapter_unit_target {
            known.push(format!("- 每章目标档位：{target}"));
        }
        if !known.is_empty() {
            lines.push("当前已知信息：".to_string());
            lines.extend(known);
        }
        lines.push("仍需补齐并通过质量门：".to_string());
        for issue in locked_issues.iter().take(8) {
            lines.push(format!(
                "- {}",
                issue.trim_start_matches("ContractBlocker: ").trim()
            ));
        }
        return lines.join("\n");
    }
    push_contract_line(&mut lines, "书名", &draft.title);
    push_contract_line(&mut lines, "题材", &draft.genre);
    push_contract_line(&mut lines, "语言", &draft.language);
    push_contract_line(&mut lines, "简述", &draft.brief);
    if let Some(target) = draft.target_units {
        lines.push(format!("- 总目标字数：{target}"));
    } else {
        lines.push("- 总目标字数：未指定".to_string());
    }
    if let Some(target) = draft.chapter_unit_target {
        lines.push(format!(
            "- 每章目标档位：{target}（小说每章字数仅支持 {}）",
            longform_policy::novel_chapter_unit_band_label()
        ));
    } else {
        lines.push(format!(
            "- 每章目标档位：未指定（小说每章字数可选 {}）",
            longform_policy::novel_chapter_unit_band_label()
        ));
    }
    if let Some(turns) = draft.max_chapters_per_turn {
        lines.push(format!("- 每轮章节数：{turns}"));
    } else {
        lines.push("- 每轮章节数：未指定".to_string());
    }
    if let Some(expected) =
        draft
            .target_units
            .zip(draft.chapter_unit_target)
            .and_then(|(total, per_chapter)| {
                longform_policy::expected_chapter_count(total, per_chapter)
            })
    {
        lines.push(format!("- 预计章节数：约 {expected} 章"));
    }

    push_contract_line(&mut lines, "故事前提", &draft.fiction_premise);
    push_contract_line(&mut lines, "终局方向", &draft.fiction_ending_direction);
    push_contract_line(&mut lines, "主角弧线", &draft.fiction_protagonist_arc);
    push_contract_line(&mut lines, "世界观意象", &draft.fiction_world_imagery);
    push_contract_line(&mut lines, "总主线因果链", &draft.fiction_main_causal_spine);
    push_contract_line(&mut lines, "书名/标题理由", &draft.fiction_title_rationale);

    push_contract_list(
        &mut lines,
        "角色权威表",
        governed_contract_characters_for_view(draft),
    );
    push_contract_list(&mut lines, "核心主题", draft.fiction_themes.clone());
    push_contract_list(&mut lines, "世界规则", draft.fiction_world_rules.clone());
    push_contract_list(&mut lines, "叙事风格", draft.fiction_style_rules.clone());
    push_contract_list(&mut lines, "必须避免", draft.fiction_must_avoid.clone());
    push_contract_block(&mut lines, "大纲/阶段规划", &draft.fiction_outline);
    push_typed_outline_contract_blocks(&mut lines, draft);

    let structured = render_structured_contract_v2_status(draft, true);
    if structured.trim().is_empty() {
        lines.push("- 结构化合同：仍在补齐中".to_string());
    } else {
        lines.push("结构化合同完整视图：".to_string());
        for line in structured.lines() {
            lines.push(format!("- {}", line.trim()));
        }
    }

    lines.join("\n")
}

fn visible_creation_title_or_pending(title: &str) -> String {
    if fiction_title_is_temporary_placeholder(title) {
        "待定".to_string()
    } else {
        title.trim().to_string()
    }
}

pub(crate) fn push_contract_line(lines: &mut Vec<String>, label: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        lines.push(format!("- {label}：未指定"));
    } else {
        lines.push(format!("- {label}：{}", contract_view_text(value, 360)));
    }
}

pub(crate) fn push_contract_block(lines: &mut Vec<String>, label: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        lines.push(format!("- {label}：未指定"));
        return;
    }
    lines.push(format!("- {label}："));
    for line in value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !contract_view_line_is_structural_noise(line))
        .take(16)
    {
        lines.push(format!("  - {}", contract_view_text(line, 220)));
    }
}

pub(crate) fn push_contract_list(lines: &mut Vec<String>, label: &str, values: Vec<String>) {
    let values = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        lines.push(format!("- {label}：未指定"));
        return;
    }
    lines.push(format!("- {label}："));
    for value in values.into_iter().take(12) {
        lines.push(format!("  - {}", contract_view_text(&value, 260)));
    }
}

fn push_typed_outline_contract_blocks(lines: &mut Vec<String>, draft: &SessionCreationDraftState) {
    let Some(contract) = current_or_pending_typed_contract_for_view(draft) else {
        return;
    };
    let volume_lines = contract
        .outline
        .volumes
        .iter()
        .enumerate()
        .filter_map(|(index, volume)| {
            let title = volume.title.trim();
            let objective = volume.objective.trim();
            let ending = volume.ending_change.trim();
            if title.is_empty() && objective.is_empty() && ending.is_empty() {
                return None;
            }
            let mut line = format!("第{}卷", index + 1);
            if !title.is_empty() {
                line.push_str(&format!("《{title}》"));
            }
            if !objective.is_empty() {
                line.push_str(&format!("：{objective}"));
            }
            if !ending.is_empty() {
                line.push_str(&format!("；卷尾变化：{ending}"));
            }
            Some(line)
        })
        .collect::<Vec<_>>();
    push_contract_list(lines, "分卷规划", volume_lines);

    let chapter_lines = contract
        .outline
        .near_chapters
        .iter()
        .filter_map(|chapter| {
            let goal = chapter.goal.trim();
            let turn = chapter.expected_turn.trim();
            if goal.is_empty() && turn.is_empty() {
                return None;
            }
            let number = chapter
                .number
                .map(|number| number.to_string())
                .unwrap_or_else(|| "?".to_string());
            let mut line = format!("第{number}章");
            if !goal.is_empty() {
                line.push_str(&format!(" 本章目标：{goal}"));
            }
            if !turn.is_empty() {
                line.push_str(&format!("；预期转折：{turn}"));
            }
            Some(line)
        })
        .collect::<Vec<_>>();
    push_contract_list(lines, "近期章节包", chapter_lines);
}

fn current_or_pending_typed_contract_for_view(
    draft: &SessionCreationDraftState,
) -> Option<NovelCreationContract> {
    let value = draft.current_contract.as_ref()?;
    let text = serde_json::to_string(value).ok()?;
    NovelCreationContract::parse_json_boundary(&text)
}

pub(crate) fn governed_contract_characters_for_view(
    draft: &SessionCreationDraftState,
) -> Vec<String> {
    draft
        .fiction_characters
        .iter()
        .filter_map(|line| user_facing_character_contract_line(line))
        .collect()
}

pub(crate) fn canonical_character_authority_for_prompt(
    draft: &SessionCreationDraftState,
    approved: &Value,
    project_path: &str,
) -> String {
    let approved_characters = approved
        .get("draft")
        .and_then(|draft| draft.get("characters"))
        .and_then(value_string_array)
        .unwrap_or_default();
    if !approved_characters.is_empty() {
        let summary = approved_characters
            .iter()
            .filter_map(|line| user_facing_character_contract_line(line))
            .collect::<Vec<_>>()
            .join("；");
        if project_path.trim().is_empty() {
            return summary;
        }
        return format!("最终以 project_path 内 manifest/story_bible/character_ledger 为准；合同摘要：{summary}");
    }
    if !project_path.trim().is_empty() {
        return "最终以 project_path 内 manifest/story_bible/character_ledger 为准；不要使用聊天历史里的旧角色名。"
            .to_string();
    }
    governed_contract_characters_for_view(draft).join("；")
}

pub(crate) fn value_string_array(value: &Value) -> Option<Vec<String>> {
    let values = value
        .as_array()?
        .iter()
        .filter_map(|item| item.as_str().map(str::trim))
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values)
}

pub(crate) fn user_facing_character_contract_line(line: &str) -> Option<String> {
    let sanitized = sanitize_contract_view_text(line);
    if sanitized.trim().is_empty() {
        return None;
    }
    let name = contract_line_field_value(line, "name")
        .or_else(|| contract_line_field_value(line, "姓名"))
        .unwrap_or_default();
    let role = contract_line_field_value(line, "role")
        .or_else(|| contract_line_field_value(line, "角色"))
        .unwrap_or_default();
    let desire = contract_line_field_value(line, "desire")
        .or_else(|| contract_line_field_value(line, "欲望"))
        .unwrap_or_default();
    let fear = contract_line_field_value(line, "fear")
        .or_else(|| contract_line_field_value(line, "恐惧"))
        .unwrap_or_default();
    let bottom_line = contract_line_field_value(line, "bottom_line")
        .or_else(|| contract_line_field_value(line, "底线"))
        .unwrap_or_default();
    if !name.is_empty() || !role.is_empty() {
        let mut parts = Vec::new();
        if !name.is_empty() {
            parts.push(format!("姓名：{name}"));
        }
        if !role.is_empty() {
            parts.push(format!("角色：{role}"));
        }
        if !desire.is_empty() {
            parts.push(format!("欲望：{desire}"));
        }
        if !fear.is_empty() {
            parts.push(format!("恐惧：{fear}"));
        }
        if !bottom_line.is_empty() {
            parts.push(format!("底线：{bottom_line}"));
        }
        return Some(contract_view_text(&parts.join("，"), 260));
    }
    Some(contract_view_text(&sanitized, 260))
}

pub(crate) fn contract_line_field_value(line: &str, key: &str) -> Option<String> {
    let parts = if line.contains([';', '；']) {
        line.split([';', '；']).collect::<Vec<_>>()
    } else {
        line.split([',', '，']).collect::<Vec<_>>()
    };
    parts
        .into_iter()
        .filter_map(|part| {
            part.split_once(':')
                .or_else(|| part.split_once('：'))
                .map(|(left, right)| (left.trim(), right.trim()))
        })
        .find_map(|(left, right)| {
            (left == key).then(|| strip_balanced_contract_field_quotes(right.trim()).to_string())
        })
        .filter(|value| !value.is_empty())
}

fn strip_balanced_contract_field_quotes(value: &str) -> &str {
    [('"', '"'), ('\'', '\''), ('“', '”'), ('‘', '’')]
        .into_iter()
        .find_map(|(opening, closing)| {
            value
                .strip_prefix(opening)
                .and_then(|inner| inner.strip_suffix(closing))
        })
        .unwrap_or(value)
        .trim()
}

pub(crate) fn contract_view_text(value: &str, max_chars: usize) -> String {
    compact_creation_text(&sanitize_contract_view_text(value), max_chars)
}

pub(crate) fn sanitize_contract_view_text(value: &str) -> String {
    let without_machine_fields = value
        .split([';', '；'])
        .map(str::trim)
        .filter(|part| {
            let lowered = part.to_ascii_lowercase();
            !lowered.starts_with("name_source:")
                && !lowered.starts_with("name_source：")
                && !lowered.starts_with("source:")
                && !lowered.starts_with("source：")
        })
        .collect::<Vec<_>>()
        .join("；");
    let naturalized = without_machine_fields
        .replace(" -> ", "，然后")
        .replace("->", "，然后")
        .replace(" => ", "，因此")
        .replace("=>", "，因此")
        .replace('→', "，然后");
    collapse_adjacent_repeated_cjk_words(&naturalized)
        .trim()
        .to_string()
}

pub(crate) fn contract_view_line_is_structural_noise(line: &str) -> bool {
    let compact = line
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.is_empty() {
        return true;
    }
    matches!(
        compact.as_str(),
        "{" | "}" | "[" | "]" | "," | "}," | "]," | "{{" | "}}"
    )
}

pub(crate) fn collapse_adjacent_repeated_cjk_words(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < chars.len() {
        let mut collapsed = false;
        for len in (2..=4).rev() {
            if index + len * 2 > chars.len() {
                continue;
            }
            if chars[index..index + len]
                .iter()
                .all(|ch| is_cjk_unified(*ch))
                && chars[index..index + len] == chars[index + len..index + len * 2]
            {
                out.extend(chars[index..index + len].iter());
                index += len * 2;
                collapsed = true;
                break;
            }
        }
        if !collapsed {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

pub(crate) fn render_structured_contract_v2_status(
    draft: &SessionCreationDraftState,
    full: bool,
) -> String {
    let contract = draft.contract_v2();
    let mut parts = Vec::new();
    let summary_lines = novel_contract_v2::summary_lines(&contract);
    if !summary_lines.is_empty() {
        parts.push(summary_lines.join("；"));
    }
    if !contract.field_requirements.is_empty() {
        parts.push(format!(
            "字段完整度：已补齐 {} 项",
            contract.field_requirements.len()
        ));
    }
    if full {
        push_contract_detail(
            &mut parts,
            "资源体系",
            &contract.resource_economy.value_scale,
        );
        push_contract_detail(
            &mut parts,
            "资源类型",
            &contract.resource_economy.resource_types.join("、"),
        );
        push_contract_detail(
            &mut parts,
            "情绪路径",
            &contract.emotional_contract.emotional_beats.join("；"),
        );
        push_contract_detail(
            &mut parts,
            "节奏缓冲",
            &contract.emotional_contract.relief_beats.join("；"),
        );
        push_contract_detail(
            &mut parts,
            "关系账本",
            &format_relationship_ledger(&contract.relationship_ledger),
        );
        push_contract_detail(
            &mut parts,
            "成长体系",
            &contract.power_progression.system_name,
        );
        push_contract_detail(
            &mut parts,
            "成长阶段",
            &contract.power_progression.levels.join("、"),
        );
        push_contract_detail(&mut parts, "社会秩序", &contract.social_order.rank_system);
        push_contract_detail(
            &mut parts,
            "地理模型",
            &contract.geography_model.regions.join("、"),
        );
        push_contract_detail(&mut parts, "时间模型", &contract.time_model.calendar);
        push_contract_detail(
            &mut parts,
            "物品/线索账本",
            &contract
                .artifact_ledger
                .iter()
                .map(|item| item.name.trim())
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>()
                .join("、"),
        );
        push_contract_detail(
            &mut parts,
            "对手压力",
            &contract.antagonist_pressure.primary_pressure,
        );
        push_contract_detail(
            &mut parts,
            "兑现矩阵",
            &contract
                .payoff_matrix
                .iter()
                .filter(|item| {
                    crate::tool::writing::typed_contract_gate::payoff_matrix_entry_is_complete(item)
                })
                .map(|item| {
                    first_non_empty_string(&[item.promise.as_str(), item.payoff_target.as_str()])
                })
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join("；"),
        );
        push_contract_detail(&mut parts, "叙事口径", &contract.narration_contract.pov);
        push_contract_detail(
            &mut parts,
            "场景类型配比",
            &format_scene_type_mix(&contract.scene_type_mix),
        );
        push_contract_detail(
            &mut parts,
            "角色声音表",
            &format_character_voice_ledger(&contract.character_voice_ledger),
        );
        push_contract_detail(
            &mut parts,
            "读者期待/爽点合同",
            &format_reader_promise(&contract.reader_promise),
        );
        push_contract_detail(
            &mut parts,
            "章尾轮换",
            &format_chapter_ending_rotation(&contract.chapter_ending_rotation),
        );
        push_contract_detail(
            &mut parts,
            "冲突升降压曲线",
            &format_conflict_pressure_curve(&contract.conflict_pressure_curve),
        );
        push_contract_detail(
            &mut parts,
            "主题母题账本",
            &format_motif_ledger(&contract.motif_ledger),
        );
        push_contract_detail(
            &mut parts,
            "信息揭示节奏",
            &format_reveal_schedule(&contract.reveal_schedule),
        );
        push_contract_detail(
            &mut parts,
            "关系互动配额",
            &format_relationship_interaction_quotas(&contract.relationship_interaction_quotas),
        );
    }
    parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .map(|part| contract_view_text(&part, if full { 480 } else { 160 }))
        .collect::<Vec<_>>()
        .join(if full { "\n" } else { "；" })
}

pub(crate) fn push_contract_detail(parts: &mut Vec<String>, label: &str, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        parts.push(format!("{label}：{}", sanitize_contract_view_text(value)));
    }
}

pub(crate) fn format_relationship_ledger(entries: &[RelationshipLedgerEntry]) -> String {
    entries
        .iter()
        .filter_map(|entry| {
            let names = entry
                .characters
                .iter()
                .map(|name| name.trim())
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>();
            if names.is_empty() {
                return None;
            }
            let relation = first_non_empty_user_visible_relationship_value(&[
                entry.relationship_type.as_str(),
                entry.stage.as_str(),
                entry.current_state.as_str(),
                entry.next_expected_stage.as_str(),
                entry.desired_end_state.as_str(),
                entry.arc_type.as_str(),
            ]);
            Some(if relation.is_empty() {
                names.join("/")
            } else {
                format!("{}：{}", names.join("/"), relation)
            })
        })
        .collect::<Vec<_>>()
        .join("；")
}

pub(crate) fn format_scene_type_mix(value: &SceneTypeMix) -> String {
    [
        ("动作", value.action.as_str()),
        ("对话", value.dialogue.as_str()),
        ("日常", value.everyday.as_str()),
        ("揭示", value.reveal.as_str()),
        ("情感", value.emotional.as_str()),
        ("转折", value.turning_point.as_str()),
        ("轮换", value.balance_rule.as_str()),
    ]
    .into_iter()
    .filter(|(_, value)| !value.trim().is_empty())
    .map(|(label, value)| format!("{label}：{}", value.trim()))
    .collect::<Vec<_>>()
    .join("；")
}

pub(crate) fn format_character_voice_ledger(entries: &[CharacterVoiceProfile]) -> String {
    entries
        .iter()
        .filter_map(|entry| {
            let name = entry.character.trim();
            let voice = entry.voice_style.trim();
            if name.is_empty() && voice.is_empty() && entry.dialogue_rules.is_empty() {
                return None;
            }
            let mut parts = Vec::new();
            if !voice.is_empty() {
                parts.push(voice.to_string());
            }
            if !entry.dialogue_rules.is_empty() {
                parts.push(entry.dialogue_rules.join("、"));
            }
            Some(format!(
                "{}：{}",
                if name.is_empty() { "角色" } else { name },
                parts.join("；")
            ))
        })
        .collect::<Vec<_>>()
        .join("；")
}

pub(crate) fn format_reader_promise(value: &ReaderPromise) -> String {
    [
        value.core_hook.trim().to_string(),
        (!value.pleasure_points.is_empty())
            .then(|| value.pleasure_points.join("、"))
            .unwrap_or_default(),
        value.curiosity_engine.trim().to_string(),
        value.payoff_style.trim().to_string(),
    ]
    .into_iter()
    .filter(|value| !value.trim().is_empty())
    .collect::<Vec<_>>()
    .join("；")
}

pub(crate) fn format_chapter_ending_rotation(value: &ChapterEndingRotation) -> String {
    [
        (!value.planned_rotation.is_empty())
            .then(|| value.planned_rotation.join("、"))
            .unwrap_or_default(),
        value.avoid_repetition_rule.trim().to_string(),
    ]
    .into_iter()
    .filter(|value| !value.trim().is_empty())
    .collect::<Vec<_>>()
    .join("；")
}

pub(crate) fn format_conflict_pressure_curve(value: &ConflictPressureCurve) -> String {
    let mut parts = value
        .global_curve
        .iter()
        .filter_map(|beat| {
            let text = [
                beat.range.trim(),
                beat.pressure_level.trim(),
                beat.function.trim(),
            ]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("/");
            (!text.is_empty()).then_some(text)
        })
        .collect::<Vec<_>>();
    if !value.release_strategy.trim().is_empty() {
        parts.push(format!("缓冲：{}", value.release_strategy.trim()));
    }
    if !value.peak_policy.trim().is_empty() {
        parts.push(format!("爆发：{}", value.peak_policy.trim()));
    }
    parts.join("；")
}

pub(crate) fn format_motif_ledger(entries: &[MotifLedgerEntry]) -> String {
    entries
        .iter()
        .filter_map(|entry| {
            let text = [
                entry.motif.trim(),
                entry.meaning.trim(),
                entry.payoff_target.trim(),
            ]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("/");
            (!text.is_empty()).then_some(text)
        })
        .collect::<Vec<_>>()
        .join("；")
}

pub(crate) fn format_reveal_schedule(entries: &[RevealScheduleEntry]) -> String {
    entries
        .iter()
        .filter_map(|entry| {
            let text = [
                entry.secret.trim(),
                entry.reader_knows.trim(),
                entry.protagonist_knows.trim(),
                entry.reveal_window.trim(),
            ]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("/");
            (!text.is_empty()).then_some(text)
        })
        .collect::<Vec<_>>()
        .join("；")
}

pub(crate) fn format_relationship_interaction_quotas(
    entries: &[RelationshipInteractionQuota],
) -> String {
    entries
        .iter()
        .filter_map(|entry| {
            let text = [
                entry.relationship.trim().to_string(),
                (!entry.characters.is_empty())
                    .then(|| entry.characters.join("、"))
                    .unwrap_or_default(),
                entry.cadence.trim().to_string(),
                entry.required_interaction.trim().to_string(),
            ]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join("/");
            (!text.is_empty()).then_some(text)
        })
        .collect::<Vec<_>>()
        .join("；")
}

pub(crate) fn first_non_empty_string(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn first_non_empty_user_visible_relationship_value(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| {
            !value.is_empty()
                && !matches!(
                    value.to_ascii_lowercase().as_str(),
                    "relationship"
                        | "love"
                        | "family"
                        | "friendship"
                        | "mentor"
                        | "rivalry"
                        | "alliance"
                )
        })
        .unwrap_or("")
        .to_string()
}

pub fn intent_requests_creation_contract_view(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    let known_surface = [
        "展示当前",
        "当前小说创作合同",
        "完整合同",
        "当前合同",
        "现在合同",
        "查看合同",
        "显示合同",
        "合同是什么",
        "合同内容",
        "全部合同",
        "合同摘要",
        "结构化合同",
        "合同草案",
        "分卷大纲",
        "人物弧线",
        "预计章节",
        "show current",
        "full contract",
        "contract summary",
        "current outline",
        "contract draft",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(term));
    if known_surface {
        return true;
    }

    let has_read_action = [
        "展示", "显示", "查看", "看看", "读取", "核对", "show", "view", "display", "inspect",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(term));
    let has_contract_surface = ["合同", "草案", "contract", "draft"]
        .iter()
        .any(|term| message.contains(term) || lowered.contains(term));
    has_read_action && has_contract_surface
}

pub fn intent_requests_full_creation_contract_view(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    let known_surface = [
        "完整合同",
        "全部合同",
        "完整创作合同",
        "完整小说合同",
        "full contract",
        "entire contract",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(term));
    known_surface
        || ((message.contains("完整") || message.contains("全部") || lowered.contains("full"))
            && (message.contains("合同")
                || message.contains("草案")
                || lowered.contains("contract")
                || lowered.contains("draft")))
}

pub fn creation_draft_view_only_requested(message: &str) -> bool {
    if !intent_requests_creation_contract_view(message) {
        return false;
    }
    if creation_draft_approval_requested(message) {
        return false;
    }
    let mutating_terms = [
        "重新生成",
        "重新输出",
        "重写",
        "修复",
        "修订",
        "改成",
        "改为",
        "修改",
        "设置",
        "设为",
        "设定",
        "补充",
        "补齐",
        "完善",
        "增加",
        "加入",
        "换成",
        "主角是",
        "女主是",
        "男主是",
        "反派是",
        "题材是",
        "类型是",
        "每章",
        "一章",
        "单章",
        "总字数",
        "一共",
        "总共",
        "全文",
        "全书",
        "整部",
        "target",
        "total",
        "change",
        "set",
        "update",
        "add",
    ];
    !text_has_any(message, &mutating_terms)
}

pub fn creation_draft_framework_requested(message: &str, artifact_kind: &str) -> bool {
    if !creation_draft_planning_dialogue_requested(message)
        && (creation_draft_approval_requested(message)
            || creation_draft_execution_requested(message, artifact_kind))
    {
        return false;
    }
    let lowered = message.to_ascii_lowercase();
    let common_terms = [
        "框架",
        "大纲",
        "章节规划",
        "章节大纲",
        "人物关系",
        "人物弧线",
        "核心矛盾",
        "创作合同",
        "合同草案",
        "重新生成合同",
        "重新生成草案",
        "重新输出合同",
        "重新输出草案",
        "重写合同",
        "重写草案",
        "修订合同",
        "修订草案",
        "修复合同",
        "补齐合同",
        "质量门",
        "未通过",
        "修复刚才提示",
        "修复上面的问题",
        "修复这些问题",
        "结构安排",
        "先规划",
        "先定",
        "outline",
        "framework",
        "structure",
        "plan first",
    ];
    if creation_draft_planning_dialogue_requested(message) {
        return true;
    }
    if artifact_kind == "fiction" {
        let fiction_terms = ["书名", "主角名字", "角色名字", "结尾承诺", "情感弧线"];
        return common_terms
            .iter()
            .chain(fiction_terms.iter())
            .any(|term| message.contains(term) || lowered.contains(term));
    }
    common_terms
        .iter()
        .any(|term| message.contains(term) || lowered.contains(term))
}

#[cfg(test)]
mod contract_view_intent_tests {
    use super::*;

    #[test]
    fn modified_full_contract_read_request_stays_view_only() {
        let message = "请展示现在完整的可确认创作合同。我需要先核对书名、主要人物、世界规则、分卷与近期章节规划；在我明确确认前不要写正文。";

        assert!(intent_requests_creation_contract_view(message));
        assert!(intent_requests_full_creation_contract_view(message));
        assert!(creation_draft_view_only_requested(message));
    }

    #[test]
    fn contract_read_request_with_a_real_mutation_is_not_view_only() {
        assert!(!creation_draft_view_only_requested(
            "请展示当前合同，并把主角改成女性。"
        ));
    }

    #[test]
    fn character_field_parser_preserves_internal_closing_quote() {
        let line = "name: 苏青; fear: 被清洗协议遗漏成为“空白人”; bottom_line: 不伪造记录";

        assert_eq!(
            contract_line_field_value(line, "fear").as_deref(),
            Some("被清洗协议遗漏成为“空白人”")
        );
        assert_eq!(
            contract_line_field_value("fear: “害怕失去记忆”", "fear").as_deref(),
            Some("害怕失去记忆")
        );
    }

    #[test]
    fn character_field_parser_preserves_commas_inside_semicolon_delimited_values() {
        let line = "name: 季砚岚; role: 同伴; bottom_line: 无论祝承原变成何种形态，誓死守住他的神经接口安全; name_source: generated_by_writing_tool_policy";

        assert_eq!(
            contract_line_field_value(line, "bottom_line").as_deref(),
            Some("无论祝承原变成何种形态，誓死守住他的神经接口安全")
        );
    }
}
