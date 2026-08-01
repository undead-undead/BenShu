use super::*;

pub fn creation_draft_tool_args(action: &str, draft: &SessionCreationDraftState) -> Value {
    if draft.tool_name == "novel_studio" {
        let authority_contract = super::strong_novel_contract_from_creation_draft(draft);
        let brief = sanitize_creation_brief_value(&draft.brief);
        let premise = authority_contract.premise.clone();
        let outline = super::strong_contract_outline_text(&authority_contract);
        let themes = authority_contract.themes.clone();
        let characters = authority_contract
            .characters
            .iter()
            .map(|character| character.to_draft_line())
            .collect::<Vec<_>>();
        let world_rules = authority_contract.world_rules.clone();
        let style_rules = authority_contract.style_rules.clone();
        let must_avoid = authority_contract.must_avoid.clone();
        let contract_v2 = authority_contract.structured.clone();
        let chapter_unit_target = draft
            .chapter_unit_target_user_authority
            .or(draft.chapter_unit_target);
        let tool_action = match action {
            "draft" => "draft_project",
            "update" => "update_draft",
            "approve" => "approve_draft",
            "discard" => "discard_draft",
            other => other,
        };
        let mut payload = serde_json::json!({
            "action": tool_action,
            "draft_path": draft.draft_path,
            "title": draft.title,
            "language": draft.language,
            "genre": draft.genre,
            "brief": brief,
            "target_units": draft.target_units,
            "chapter_unit_target": chapter_unit_target,
            "max_chapters_per_turn": draft.max_chapters_per_turn,
            "format": draft.export_format,
            "export_when_complete": draft.export_when_complete,
            "approved_only": draft.approved_only,
            "premise": premise,
            "ending_direction": authority_contract.ending.desired_resolution,
            "authority_contract": authority_contract,
            "protagonist_arc": authority_contract.protagonist_arc,
            "world_imagery": authority_contract.world_imagery,
            "main_causal_spine": authority_contract.main_causal_spine,
            "title_rationale": authority_contract.title.rationale,
            "themes": themes,
            "characters": characters,
            "world_rules": world_rules,
            "style_rules": style_rules,
            "must_avoid": must_avoid,
            "outline": outline,
            "field_requirements": contract_v2.field_requirements,
            "resource_economy": contract_v2.resource_economy,
            "emotional_contract": contract_v2.emotional_contract,
            "emotional_state_ledger": contract_v2.emotional_state_ledger,
            "relationship_ledger": contract_v2.relationship_ledger,
            "power_progression": contract_v2.power_progression,
            "social_order": contract_v2.social_order,
            "geography_model": contract_v2.geography_model,
            "time_model": contract_v2.time_model,
            "artifact_ledger": contract_v2.artifact_ledger,
            "antagonist_pressure": contract_v2.antagonist_pressure,
            "payoff_matrix": contract_v2.payoff_matrix,
            "narration_contract": contract_v2.narration_contract
        });
        let object = payload
            .as_object_mut()
            .expect("creation draft payload must be an object");
        object.insert(
            "scene_type_mix".to_string(),
            serde_json::to_value(contract_v2.scene_type_mix).unwrap_or_default(),
        );
        object.insert(
            "character_voice_ledger".to_string(),
            serde_json::to_value(contract_v2.character_voice_ledger).unwrap_or_default(),
        );
        object.insert(
            "reader_promise".to_string(),
            serde_json::to_value(contract_v2.reader_promise).unwrap_or_default(),
        );
        object.insert(
            "chapter_ending_rotation".to_string(),
            serde_json::to_value(contract_v2.chapter_ending_rotation).unwrap_or_default(),
        );
        object.insert(
            "conflict_pressure_curve".to_string(),
            serde_json::to_value(contract_v2.conflict_pressure_curve).unwrap_or_default(),
        );
        object.insert(
            "motif_ledger".to_string(),
            serde_json::to_value(contract_v2.motif_ledger).unwrap_or_default(),
        );
        object.insert(
            "reveal_schedule".to_string(),
            serde_json::to_value(contract_v2.reveal_schedule).unwrap_or_default(),
        );
        object.insert(
            "relationship_interaction_quotas".to_string(),
            serde_json::to_value(contract_v2.relationship_interaction_quotas).unwrap_or_default(),
        );
        payload
    } else {
        let brief = sanitize_creation_brief_value(&draft.brief);
        let tool_action = match action {
            "draft" => "draft_document",
            "update" => "update_draft",
            "approve" => "approve_draft",
            "discard" => "discard_draft",
            other => other,
        };
        serde_json::json!({
            "action": tool_action,
            "draft_path": draft.draft_path,
            "title": draft.title,
            "document_type": draft.document_type,
            "language": draft.language,
            "audience": draft.audience,
            "purpose": draft.purpose,
            "brief": brief,
            "target_units": draft.target_units,
            "section_unit_target": draft.section_unit_target,
            "format": draft.export_format,
            "export_when_complete": draft.export_when_complete,
            "approved_only": draft.approved_only,
            "thesis_or_premise": non_empty_or(&draft.thesis_or_premise, &brief),
            "required_structure": draft.required_structure,
            "style_rules": draft.style_rules,
            "evidence_rules": draft.evidence_rules,
            "revision_policy": "先完成结构化草稿，再按质量合同审查并必要时修订。"
        })
    }
}
