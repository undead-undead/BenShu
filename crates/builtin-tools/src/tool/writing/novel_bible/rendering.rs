use serde_json::json;

use super::model::StoryBible;
use crate::tool::writing::novel_contract_v2;

const CURRENT_STATE_ARRAY_HEAD_ITEMS: usize = 24;
const CURRENT_STATE_ARRAY_TAIL_ITEMS: usize = 40;
const CURRENT_STATE_STRING_MAX_CHARS: usize = 1_200;

pub(crate) fn render_story_bible_markdown(bible: &StoryBible) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Story Bible: {}\n\n", bible.title));
    out.push_str("## Ending Contract\n\n");
    out.push_str(&format!(
        "- Desired resolution: {}\n- Final state: {}\n- Must resolve: {}\n- Open questions allowed: {}\n\n",
        empty_label(&bible.ending_contract.desired_resolution),
        empty_label(&bible.ending_contract.final_state),
        render_inline_list(&bible.ending_contract.must_resolve),
        render_inline_list(&bible.ending_contract.open_questions_allowed)
    ));
    out.push_str("## Narrative Graph\n\n");
    out.push_str(&format!(
        "- Global spine: {}\n- Reverse design: {}\n\n",
        empty_label(&bible.narrative_graph.global_spine),
        render_inline_list(&bible.narrative_graph.reverse_design_notes)
    ));
    out.push_str("### Volume Arcs\n\n");
    for volume in &bible.narrative_graph.volume_arcs {
        out.push_str(&format!(
            "- {}: chapters {:?}-{:?}; goal={}; resolves_toward={}\n",
            volume.title,
            volume.start_chapter,
            volume.end_chapter,
            empty_label(&volume.goal),
            empty_label(&volume.resolves_toward)
        ));
    }
    out.push_str("\n## Characters\n\n");
    for character in &bible.character_ledger {
        out.push_str(&format!(
            "- {}: role={}, desire={}, fear={}, bottom_line={}, state={}\n",
            character.name,
            empty_label(&character.role),
            empty_label(&character.desire),
            empty_label(&character.fear),
            empty_label(&character.bottom_line),
            empty_label(&character.current_state)
        ));
    }
    out.push_str("\n## World Database\n\n");
    for rule in &bible.world_database.rules {
        out.push_str(&format!(
            "- {}: cost/limit={}, effect={}\n",
            rule.rule,
            empty_label(&rule.cost_or_limit),
            empty_label(&rule.narrative_effect)
        ));
    }
    out.push_str("\n## Genre Governance\n\n");
    out.push_str(&format!(
        "- Family: {}\n",
        bible.genre_governance.genre_family
    ));
    for axis in &bible.genre_governance.control_axes {
        out.push_str(&format!(
            "- {}: current={}, allowed={}, limits={}\n",
            axis.name,
            empty_label(&axis.current_level),
            empty_label(&axis.allowed_progression),
            render_inline_list(&axis.hard_limits)
        ));
    }
    let structured_summary = novel_contract_v2::summary_lines(&bible.structured_contract_v2);
    if !structured_summary.is_empty() {
        out.push_str("\n## Structured Contract v2\n\n");
        for line in structured_summary {
            out.push_str(&format!("- {line}\n"));
        }
    }
    out.push_str("\n## Hook Ledger\n\n");
    for hook in &bible.hook_ledger {
        out.push_str(&format!(
            "- {}: status={:?}, introduced={:?}, knowers={}, payoff_window={}, planned_payoff={:?}, actual_payoff={:?}, last_advanced={:?}, deferred_until={:?}, emotional_effect={}\n",
            hook.title,
            hook.status,
            hook.introduced_chapter,
            render_inline_list(&hook.knowers),
            empty_label(&hook.planned_payoff_window),
            hook.planned_payoff_chapter,
            hook.payoff_chapter,
            hook.last_advanced_chapter,
            hook.deferred_until_chapter,
            empty_label(&hook.emotional_effect)
        ));
    }
    out.push_str("\n## Timeline\n\n");
    for item in bible.timeline.iter().rev().take(60).rev() {
        out.push_str(&format!(
            "- {:?} {}: {} -> {}\n",
            item.chapter_number,
            item.label,
            empty_label(&item.event),
            empty_label(&item.causal_link)
        ));
    }
    out.push_str("\n## Chapter Summaries\n\n");
    for chapter in bible.chapter_summaries.iter().rev().take(40).rev() {
        out.push_str(&format!(
            "- Chapter {} {}: {} ({})\n",
            chapter.chapter_number,
            chapter.title,
            empty_label(&chapter.summary),
            chapter.unit_count
        ));
    }
    out
}

pub(crate) fn story_bible_prompt_view(bible: &StoryBible) -> serde_json::Value {
    json!({
        "schema_version": bible.schema_version,
        "ending_contract": bible.ending_contract,
        "narrative_graph": {
            "global_spine": bible.narrative_graph.global_spine,
            "reverse_design_notes": bible.narrative_graph.reverse_design_notes,
            "volume_arcs": bible.narrative_graph.volume_arcs.iter().rev().take(6).collect::<Vec<_>>(),
            "chapter_goals": bible.narrative_graph.chapter_goals.iter().rev().take(12).collect::<Vec<_>>()
        },
        "world_database": bible.world_database,
        "character_ledger": bible.character_ledger,
        "hook_ledger": bible.hook_ledger.iter().rev().take(40).collect::<Vec<_>>(),
        "genre_governance": bible.genre_governance,
        "structured_contract_v2": {
            "field_requirements": &bible.structured_contract_v2.field_requirements,
            "summary": novel_contract_v2::summary_lines(&bible.structured_contract_v2),
            "resource_economy": &bible.structured_contract_v2.resource_economy,
            "emotional_contract": &bible.structured_contract_v2.emotional_contract,
            "emotional_state_ledger": &bible.structured_contract_v2.emotional_state_ledger,
            "relationship_ledger": &bible.structured_contract_v2.relationship_ledger,
            "power_progression": &bible.structured_contract_v2.power_progression,
            "social_order": &bible.structured_contract_v2.social_order,
            "geography_model": &bible.structured_contract_v2.geography_model,
            "time_model": &bible.structured_contract_v2.time_model,
            "artifact_ledger": &bible.structured_contract_v2.artifact_ledger,
            "antagonist_pressure": &bible.structured_contract_v2.antagonist_pressure,
            "payoff_matrix": &bible.structured_contract_v2.payoff_matrix,
            "narration_contract": &bible.structured_contract_v2.narration_contract,
        },
        "theme_ledger": bible.theme_ledger,
        "timeline": bible.timeline.iter().rev().take(40).collect::<Vec<_>>(),
        "chapter_summaries": bible.chapter_summaries.iter().rev().take(20).collect::<Vec<_>>()
    })
}

/// Renders the durable current-state projection exclusively from the approved
/// typed reducers. Writer/observer display summaries are intentionally absent.
pub(crate) fn approved_state_truth(bible: &StoryBible) -> String {
    let mut projection = json!({
        "source": "approved_typed_state_changes",
        "last_approved_chapter": bible.last_rebuilt_chapter,
        "characters": bible.character_ledger.iter().map(|character| json!({
            "id": character.id,
            "name": character.name,
            "current_state": character.current_state,
        })).collect::<Vec<_>>(),
        "relationships": bible.structured_contract_v2.relationship_ledger,
        "world": bible.world_database,
        "power": bible.structured_contract_v2.power_progression,
        "resources": bible.structured_contract_v2.resource_economy,
        "artifacts": bible.structured_contract_v2.artifact_ledger,
        "hooks": bible.hook_ledger,
    });
    let mut stats = CurrentStateProjectionStats::default();
    bound_current_state_value(&mut projection, &mut stats);
    if let Some(object) = projection.as_object_mut() {
        object.insert(
            "projection".to_string(),
            json!({
                "complete_authority": "story_bible",
                "array_item_limit": CURRENT_STATE_ARRAY_HEAD_ITEMS
                    + CURRENT_STATE_ARRAY_TAIL_ITEMS,
                "string_char_limit": CURRENT_STATE_STRING_MAX_CHARS,
                "arrays_truncated": stats.arrays_truncated,
                "array_items_omitted": stats.array_items_omitted,
                "strings_truncated": stats.strings_truncated,
            }),
        );
    }
    serde_json::to_string_pretty(&projection).unwrap_or_else(|_| "{}".to_string())
}

#[derive(Default)]
struct CurrentStateProjectionStats {
    arrays_truncated: usize,
    array_items_omitted: usize,
    strings_truncated: usize,
}

fn bound_current_state_value(
    value: &mut serde_json::Value,
    stats: &mut CurrentStateProjectionStats,
) {
    match value {
        serde_json::Value::Array(items) => {
            let limit = CURRENT_STATE_ARRAY_HEAD_ITEMS + CURRENT_STATE_ARRAY_TAIL_ITEMS;
            if items.len() > limit {
                let original_len = items.len();
                let tail = items.split_off(original_len - CURRENT_STATE_ARRAY_TAIL_ITEMS);
                items.truncate(CURRENT_STATE_ARRAY_HEAD_ITEMS);
                items.extend(tail);
                stats.arrays_truncated += 1;
                stats.array_items_omitted += original_len - limit;
            }
            for item in items {
                bound_current_state_value(item, stats);
            }
        }
        serde_json::Value::Object(object) => {
            for item in object.values_mut() {
                bound_current_state_value(item, stats);
            }
        }
        serde_json::Value::String(text) => {
            if text.chars().count() > CURRENT_STATE_STRING_MAX_CHARS {
                let mut bounded = text
                    .chars()
                    .take(CURRENT_STATE_STRING_MAX_CHARS)
                    .collect::<String>();
                bounded.push('…');
                *text = bounded;
                stats.strings_truncated += 1;
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn render_inline_list(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        items
            .iter()
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::writing::novel_bible::model::CharacterAnchor;

    #[test]
    fn approved_state_truth_is_a_bounded_valid_projection_of_full_authority() {
        let mut bible = StoryBible::default();
        bible.character_ledger = (0..100)
            .map(|index| CharacterAnchor {
                id: format!("character-{index:04}"),
                name: format!("角色{index}"),
                current_state: if index == 99 {
                    "状态".repeat(CURRENT_STATE_STRING_MAX_CHARS)
                } else {
                    format!("第{index}章状态")
                },
                ..CharacterAnchor::default()
            })
            .collect();

        let rendered = approved_state_truth(&bible);
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("projection must remain valid JSON");
        let characters = parsed["characters"].as_array().unwrap();

        assert_eq!(
            characters.len(),
            CURRENT_STATE_ARRAY_HEAD_ITEMS + CURRENT_STATE_ARRAY_TAIL_ITEMS
        );
        assert_eq!(characters.first().unwrap()["id"], "character-0000");
        assert_eq!(characters.last().unwrap()["id"], "character-0099");
        assert_eq!(parsed["projection"]["array_items_omitted"], 36);
        assert_eq!(parsed["projection"]["strings_truncated"], 1);
        assert_eq!(bible.character_ledger.len(), 100);
        assert!(
            bible.character_ledger[99].current_state.chars().count()
                > CURRENT_STATE_STRING_MAX_CHARS
        );
    }
}

fn empty_label(value: &str) -> &str {
    if value.trim().is_empty() {
        "(missing)"
    } else {
        value.trim()
    }
}
