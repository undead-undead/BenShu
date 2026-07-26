use super::*;

pub(super) fn project_state_summary(manifest: &NovelProjectManifest) -> serde_json::Value {
    let units = project_total_units(manifest);
    let approved_units: usize = manifest
        .chapters
        .iter()
        .filter(|chapter| chapter_is_approved(chapter))
        .map(|chapter| chapter.unit_count)
        .sum();
    let progress = manifest
        .target_units
        .filter(|target| *target > 0)
        .map(|target| approved_units as f64 / target as f64);
    json!({
        "title": manifest.title,
        "title_state": manifest.title_state,
        "language": manifest.language,
        "genre": manifest.genre,
        "sources": manifest.sources.len(),
        "chapter_plans": manifest.chapter_plans.len(),
        "chapter_contracts": manifest.chapter_contracts.len(),
        "context_packages": manifest.context_packages.len(),
        "chapter_architectures": manifest.chapter_architectures.len(),
        "chapters": manifest.chapters.len(),
        "manifest_approved_chapters": manifest.chapters.iter().filter(|chapter| chapter_is_approved(chapter)).count(),
        "units": units,
        "manifest_approved_units": approved_units,
        "target_units": manifest.target_units,
        "chapter_unit_target": manifest.chapter_unit_target,
        "max_chapters_per_turn": manifest.max_chapters_per_turn,
        "export_format": manifest.export_format,
        "export_when_complete": manifest.export_when_complete,
        "approved_only": manifest.approved_only,
        "manifest_progress_ratio_estimate": progress,
        "has_contract": manifest.contract.is_some(),
        "has_story_bible": manifest.story_bible.is_some(),
        "volumes": manifest.volumes.len(),
        "volume_summaries": manifest.volume_summaries.len(),
        "character_ledger": manifest.character_ledger.len(),
        "structured_contract_v2": {
            "field_requirements": &manifest.structured_contract_v2.field_requirements,
            "summary": novel_contract_v2::summary_lines(&manifest.structured_contract_v2),
            "relationship_count": manifest.structured_contract_v2.relationship_ledger.len(),
            "payoff_count": manifest.structured_contract_v2.payoff_matrix.len(),
            "artifact_count": manifest.structured_contract_v2.artifact_ledger.len()
        },
        "active_volume": manifest
            .chapters
            .iter()
            .map(|chapter| chapter.number)
            .max()
            .and_then(|number| volume_for_chapter(manifest, number))
            .map(|volume| json!({
                "id": volume.id,
                "title": volume.title,
                "start_chapter": volume.start_chapter,
                "end_chapter": volume.end_chapter,
                "status": volume.status
            })),
        "writing_governance": writing_governance_report(manifest),
        "story_bible": manifest.story_bible.as_ref().map(|bible| json!({
            "characters": bible.character_ledger.len(),
            "world_rules": bible.world_database.rules.len(),
            "hooks": bible.hook_ledger.len(),
            "timeline_entries": bible.timeline.len(),
            "chapter_summaries": bible.chapter_summaries.len(),
            "genre_family": bible.genre_governance.genre_family,
            "ending_contract": !bible.ending_contract.desired_resolution.trim().is_empty()
        })),
        "truth_files": manifest.truth_files.len(),
        "archives": manifest.archives.len(),
        "reviews": manifest.reviews.len(),
        "review_cycles": manifest.review_cycles.len(),
        "truth_validations": manifest.truth_validations.len(),
        "hook_debt_reports": manifest.hook_debt_reports.len(),
        "snapshots": manifest.snapshots.len(),
        "style_profiles": manifest.style_profiles.len(),
        "progress_authority": "requires_contiguous_approved_chapter_bodies_on_disk"
    })
}

pub(super) fn project_state_summary_light(manifest: &NovelProjectManifest) -> serde_json::Value {
    let units = project_total_units(manifest);
    let approved_units: usize = manifest
        .chapters
        .iter()
        .filter(|chapter| chapter_is_approved(chapter))
        .map(|chapter| chapter.unit_count)
        .sum();
    let progress = manifest
        .target_units
        .filter(|target| *target > 0)
        .map(|target| approved_units as f64 / target as f64);
    json!({
        "title": manifest.title,
        "title_state": manifest.title_state,
        "language": manifest.language,
        "genre": manifest.genre,
        "sources": manifest.sources.len(),
        "chapter_plans": manifest.chapter_plans.len(),
        "chapter_contracts": manifest.chapter_contracts.len(),
        "context_packages": manifest.context_packages.len(),
        "chapter_architectures": manifest.chapter_architectures.len(),
        "chapters": manifest.chapters.len(),
        "manifest_approved_chapters": manifest.chapters.iter().filter(|chapter| chapter_is_approved(chapter)).count(),
        "units": units,
        "manifest_approved_units": approved_units,
        "target_units": manifest.target_units,
        "chapter_unit_target": manifest.chapter_unit_target,
        "max_chapters_per_turn": manifest.max_chapters_per_turn,
        "export_format": manifest.export_format,
        "export_when_complete": manifest.export_when_complete,
        "approved_only": manifest.approved_only,
        "manifest_progress_ratio_estimate": progress,
        "has_contract": manifest.contract.is_some(),
        "has_story_bible": manifest.story_bible.is_some(),
        "volumes": manifest.volumes.len(),
        "volume_summaries": manifest.volume_summaries.len(),
        "character_ledger": manifest.character_ledger.len(),
        "structured_contract_v2": {
            "summary": novel_contract_v2::summary_lines(&manifest.structured_contract_v2),
            "relationship_count": manifest.structured_contract_v2.relationship_ledger.len(),
            "payoff_count": manifest.structured_contract_v2.payoff_matrix.len(),
            "artifact_count": manifest.structured_contract_v2.artifact_ledger.len()
        },
        "truth_files": manifest.truth_files.len(),
        "archives": manifest.archives.len(),
        "reviews": manifest.reviews.len(),
        "review_cycles": manifest.review_cycles.len(),
        "truth_validations": manifest.truth_validations.len(),
        "hook_debt_reports": manifest.hook_debt_reports.len(),
        "snapshots": manifest.snapshots.len(),
        "style_profiles": manifest.style_profiles.len(),
        "progress_authority": "requires_contiguous_approved_chapter_bodies_on_disk"
    })
}

pub(super) fn light_status_audit_manifest(manifest: &NovelProjectManifest) -> serde_json::Value {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    if manifest.contract.is_none() {
        blockers.push("Story contract is missing.".to_string());
    }
    if manifest.story_bible.is_none() {
        warnings.push("Story bible is missing.".to_string());
    }
    if first_unapproved_chapter_number(manifest).is_some() {
        warnings.push("One or more chapters are not approved.".to_string());
    }
    json!({
        "passed": blockers.is_empty(),
        "blockers": blockers,
        "warnings": warnings,
        "mode": "light_status"
    })
}

pub(super) fn novel_draft_summary(draft: &NovelCreationDraft) -> serde_json::Value {
    json!({
        "schema_version": draft.schema_version,
        "title": draft.title,
        "language": draft.language,
        "genre": draft.genre,
        "brief": draft.brief,
        "target_units": draft.target_units,
        "chapter_unit_target": draft.chapter_unit_target,
        "max_chapters_per_turn": draft.max_chapters_per_turn,
        "export_format": draft.export_format,
        "export_when_complete": draft.export_when_complete,
        "approved_only": draft.approved_only,
        "premise": draft.premise,
        "ending_direction": draft.ending_direction,
        "authority_contract": draft.authority_contract,
        "protagonist_arc": draft.protagonist_arc,
        "world_imagery": draft.world_imagery,
        "main_causal_spine": draft.main_causal_spine,
        "title_rationale": draft.title_rationale,
        "themes": draft.themes,
        "characters": draft.characters,
        "world_rules": draft.world_rules,
        "style_rules": draft.style_rules,
        "must_avoid": draft.must_avoid,
        "outline": draft.outline,
        "structured_contract_v2": {
            "field_requirements": &draft.structured_contract_v2.field_requirements,
            "summary": novel_contract_v2::summary_lines(&draft.structured_contract_v2),
            "resource_economy": &draft.structured_contract_v2.resource_economy,
            "emotional_contract": &draft.structured_contract_v2.emotional_contract,
            "relationship_ledger": &draft.structured_contract_v2.relationship_ledger,
            "power_progression": &draft.structured_contract_v2.power_progression,
            "social_order": &draft.structured_contract_v2.social_order,
            "geography_model": &draft.structured_contract_v2.geography_model,
            "time_model": &draft.structured_contract_v2.time_model,
            "artifact_ledger": &draft.structured_contract_v2.artifact_ledger,
            "antagonist_pressure": &draft.structured_contract_v2.antagonist_pressure,
            "payoff_matrix": &draft.structured_contract_v2.payoff_matrix,
            "narration_contract": &draft.structured_contract_v2.narration_contract,
        },
        "updated_at": draft.updated_at
    })
}

pub(super) fn approved_novel_creation_draft_from_manifest(
    draft: &NovelCreationDraft,
    manifest: &NovelProjectManifest,
) -> NovelCreationDraft {
    let canonical = novel_creation_draft_from_manifest(manifest);
    let mut approved = draft.clone();
    approved.schema_version = canonical.schema_version;
    approved.title = canonical.title;
    approved.language = canonical.language;
    approved.genre = canonical.genre;
    approved.brief = canonical.brief;
    approved.target_units = canonical.target_units;
    approved.chapter_unit_target = canonical.chapter_unit_target;
    approved.max_chapters_per_turn = canonical.max_chapters_per_turn;
    approved.export_format = canonical.export_format;
    approved.export_when_complete = canonical.export_when_complete;
    approved.approved_only = canonical.approved_only;
    approved.premise = canonical.premise;
    approved.ending_direction = canonical.ending_direction;
    approved.authority_contract = canonical.authority_contract;
    approved.protagonist_arc = canonical.protagonist_arc;
    approved.world_imagery = canonical.world_imagery;
    approved.main_causal_spine = canonical.main_causal_spine;
    approved.title_rationale = canonical.title_rationale;
    approved.themes = canonical.themes;
    approved.characters = canonical.characters;
    approved.world_rules = canonical.world_rules;
    approved.style_rules = canonical.style_rules;
    approved.must_avoid = canonical.must_avoid;
    approved.outline = canonical.outline;
    approved.structured_contract_v2 = canonical.structured_contract_v2;
    approved.updated_at = canonical.updated_at;
    approved
}

pub(super) fn novel_creation_draft_from_manifest(
    manifest: &NovelProjectManifest,
) -> NovelCreationDraft {
    let mut draft = NovelCreationDraft {
        schema_version: "benshu.novel_creation_draft.v1".to_string(),
        title: manifest.title.clone(),
        language: manifest.language.clone(),
        genre: manifest.genre.clone(),
        brief: manifest.brief.clone(),
        target_units: manifest.target_units,
        chapter_unit_target: manifest.chapter_unit_target,
        max_chapters_per_turn: manifest.max_chapters_per_turn,
        export_format: manifest
            .export_format
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "txt".to_string()),
        export_when_complete: manifest.export_when_complete,
        approved_only: manifest.approved_only,
        premise: String::new(),
        ending_direction: String::new(),
        authority_contract: None,
        protagonist_arc: String::new(),
        world_imagery: String::new(),
        main_causal_spine: String::new(),
        title_rationale: manifest.title_state.rationale.clone(),
        themes: Vec::new(),
        characters: Vec::new(),
        world_rules: Vec::new(),
        style_rules: Vec::new(),
        must_avoid: Vec::new(),
        outline: String::new(),
        structured_contract_v2: manifest.structured_contract_v2.clone(),
        created_at: manifest.created_at.clone(),
        updated_at: manifest.updated_at.clone(),
    };
    if let Some(contract) = manifest.contract.as_ref() {
        draft.authority_contract = contract.authority_contract.clone();
        draft.premise = contract.premise.clone();
        draft.themes = contract.themes.clone();
        draft.characters = contract.characters.clone();
        draft.world_rules = contract.world_rules.clone();
        draft.style_rules = contract.style_rules.clone();
        draft.must_avoid = contract.must_avoid.clone();
        draft.outline = contract.outline.clone();
        draft.structured_contract_v2 = contract.structured_contract_v2.clone();
    }
    if let Some(value) = draft
        .structured_contract_v2
        .field_requirements
        .get("ending_direction")
    {
        draft.ending_direction = value.clone();
    }
    if let Some(value) = draft
        .structured_contract_v2
        .field_requirements
        .get("protagonist_arc")
    {
        draft.protagonist_arc = value.clone();
    }
    if let Some(value) = draft
        .structured_contract_v2
        .field_requirements
        .get("world_imagery")
    {
        draft.world_imagery = value.clone();
    }
    if let Some(value) = draft
        .structured_contract_v2
        .field_requirements
        .get("main_causal_spine")
    {
        draft.main_causal_spine = value.clone();
    }
    if let Some(value) = draft
        .structured_contract_v2
        .field_requirements
        .get("title_rationale")
        .filter(|value| !value.trim().is_empty())
    {
        draft.title_rationale = value.clone();
    }
    draft
}

pub(super) fn apply_novel_draft_updates(draft: &mut NovelCreationDraft, args: &NovelStudioArgs) {
    let explicit_title_update = !args.title.trim().is_empty();
    let previous_title = draft.title.clone();
    if !args.title.trim().is_empty() {
        draft.title = args.title.trim().to_string();
    }
    if !args.language.trim().is_empty() {
        draft.language = normalize_language(&args.language);
    }
    if !args.genre.trim().is_empty() {
        draft.genre = args.genre.trim().to_string();
    }
    if !args.brief.trim().is_empty() {
        draft.brief = args.brief.trim().to_string();
    }
    if args.target_units.is_some() {
        draft.target_units = args.target_units;
        draft.chapter_unit_target = longform_policy::normalize_chapter_unit_target(
            draft.chapter_unit_target,
            args.target_units,
        );
    }
    if args.chapter_unit_target.is_some() {
        draft.chapter_unit_target = longform_policy::normalize_chapter_unit_target(
            args.chapter_unit_target,
            draft.target_units,
        );
    }
    if args.max_chapters_per_turn.is_some() {
        draft.max_chapters_per_turn = args.max_chapters_per_turn.filter(|value| *value > 0);
    }
    if !args.format.trim().is_empty() {
        draft.export_format = export::normalize_export_format(args.format.trim());
    }
    if args.export_when_complete {
        draft.export_when_complete = true;
    }
    if args.approved_only {
        draft.approved_only = true;
    }
    if !args.premise.trim().is_empty() {
        draft.premise = args.premise.trim().to_string();
    }
    if !args.ending_direction.trim().is_empty() {
        draft.ending_direction = args.ending_direction.trim().to_string();
    }
    if args.authority_contract.is_some() {
        draft.authority_contract = args.authority_contract.clone();
    }
    if !args.protagonist_arc.trim().is_empty() {
        draft.protagonist_arc = args.protagonist_arc.trim().to_string();
    }
    if !args.world_imagery.trim().is_empty() {
        draft.world_imagery = args.world_imagery.trim().to_string();
    }
    if !args.main_causal_spine.trim().is_empty() {
        draft.main_causal_spine = args.main_causal_spine.trim().to_string();
    }
    if !args.title_rationale.trim().is_empty() {
        draft.title_rationale = args.title_rationale.trim().to_string();
    }
    replace_list_if_present(&mut draft.themes, &args.themes);
    replace_list_if_present(&mut draft.characters, &args.characters);
    replace_list_if_present(&mut draft.world_rules, &args.world_rules);
    replace_list_if_present(&mut draft.style_rules, &args.style_rules);
    replace_list_if_present(&mut draft.must_avoid, &args.must_avoid);
    if !args.outline.trim().is_empty() {
        draft.outline = args.outline.trim().to_string();
    }
    let incoming_contract_v2 = contract_v2_from_args(args);
    if contract_v2_has_explicit_input(args) {
        draft.structured_contract_v2 = incoming_contract_v2;
    } else {
        draft.structured_contract_v2.normalize();
    }
    if !explicit_title_update && novel_draft_title_should_refresh(&previous_title, draft, args) {
        draft.title = temporary_novel_title(&draft.language);
    }
}

pub(super) fn novel_draft_title_from_args(args: &NovelStudioArgs, language: &str) -> String {
    if !args.title.trim().is_empty() {
        return args.title.trim().to_string();
    }
    temporary_novel_title(language)
}

pub(super) fn novel_draft_readiness_issues(draft: &NovelCreationDraft) -> Vec<String> {
    if let Some(authority_contract) = draft.authority_contract.as_ref() {
        let mut authority_contract = authority_contract.clone();
        authority_contract.normalize();
        return authority_contract
            .validate_for_scope(ContractReadinessScope::LockedAuthorityContract)
            .issues
            .messages();
    }

    let mut issues = Vec::new();
    if draft.title.trim().is_empty() || project_title_is_temporary_placeholder(&draft.title) {
        issues.push(
            "title must be generated by the fiction contract, not left as a temporary placeholder"
                .to_string(),
        );
    }
    for (field, value) in [
        ("premise", draft.premise.trim()),
        ("ending_direction", draft.ending_direction.trim()),
        ("protagonist_arc", draft.protagonist_arc.trim()),
        ("world_imagery", draft.world_imagery.trim()),
        ("main_causal_spine", draft.main_causal_spine.trim()),
        ("title_rationale", draft.title_rationale.trim()),
    ] {
        if novel_draft_contract_value_missing(value) {
            issues.push(format!(
                "{field} is required before approving a fiction draft"
            ));
        }
    }
    if draft.characters.is_empty()
        || draft
            .characters
            .iter()
            .any(|line| novel_draft_character_line_has_placeholder_name(line))
    {
        issues.push(
            "characters must include stable names generated by the fiction contract".to_string(),
        );
    }
    if draft
        .characters
        .iter()
        .filter_map(|line| stable_anchor_token(line).map(ToString::to_string))
        .filter(|name| stable_character_anchor_name(name).is_some())
        .count()
        == 0
    {
        issues.push(
            "characters must contain at least one semantically valid character name anchor"
                .to_string(),
        );
    }
    let parsed_characters = draft
        .characters
        .iter()
        .map(|line| crate::tool::writing::creation_contract::draft_character_line_to_contract(line))
        .collect::<Vec<_>>();
    let mut character_ids = BTreeSet::new();
    for character in &parsed_characters {
        if character.character_id.trim().is_empty() {
            issues.push(format!(
                "character `{}` must have a stable character_id before approval",
                character.canonical_name
            ));
        } else if !character_ids.insert(character.character_id.trim().to_string()) {
            issues.push(format!(
                "character_id `{}` is assigned more than once",
                character.character_id
            ));
        }
        if character.name_source.trim().is_empty() {
            issues.push(format!(
                "character `{}` must record its naming authority before approval",
                character.canonical_name
            ));
        }
    }
    for relationship in &draft.structured_contract_v2.relationship_ledger {
        if !relationship.characters.is_empty()
            && relationship.character_ids.len() != relationship.characters.len()
        {
            issues.push(
                "relationship entries must resolve every character name to character_id before approval"
                    .to_string(),
            );
        }
    }
    if novel_draft_outline_contains_workflow_surface(&draft.outline) {
        issues
            .push("outline contains naming, review, export, or workflow surface text".to_string());
    }
    issues
}

pub(super) fn novel_draft_contract_value_missing(value: &str) -> bool {
    let compact = value.trim();
    if compact.is_empty() {
        return true;
    }
    let lowered = compact.to_ascii_lowercase();
    [
        "未指定",
        "待定",
        "暂无",
        "not specified",
        "unspecified",
        "placeholder",
        "(not specified yet)",
    ]
    .iter()
    .any(|marker| compact.contains(marker) || lowered.contains(marker))
}

pub(super) fn novel_draft_outline_contains_workflow_surface(outline: &str) -> bool {
    outline.lines().any(|line| {
        let trimmed = line.trim();
        [
            "命名理由",
            "命名依据",
            "质量合同",
            "审稿",
            "修订",
            "导出格式",
            "字段强度",
            "结构化合同",
        ]
        .iter()
        .any(|marker| trimmed.contains(marker))
    })
}

pub(super) fn temporary_novel_title(language: &str) -> String {
    let suffix = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect::<String>();
    if is_chinese_language(language) {
        format!("未命名小说-{suffix}")
    } else {
        format!("Untitled-Fiction-{suffix}")
    }
}

pub(super) fn project_title_is_temporary_placeholder(title: &str) -> bool {
    let lowered = title.trim().to_ascii_lowercase();
    lowered.starts_with("未命名小说-")
        || title.trim().contains("未命名")
        || lowered.starts_with("untitled-fiction-")
        || lowered == "untitled"
        || lowered.contains("placeholder")
}

pub(super) fn novel_draft_character_line_has_placeholder_name(line: &str) -> bool {
    let text = line.trim();
    let lowered = text.to_ascii_lowercase();
    text.contains("未命名")
        || text.contains("待命名")
        || lowered.contains("unnamed")
        || lowered.contains("placeholder")
}

pub(super) fn draft_premise_with_naming_basis(draft: &NovelCreationDraft) -> String {
    draft.premise.trim().to_string()
}

pub(super) fn draft_outline_with_naming_basis(draft: &NovelCreationDraft) -> String {
    draft.outline.trim().to_string()
}

pub(super) fn novel_draft_title_violates_language(title: &str, language: &str) -> bool {
    is_chinese_language(language) && chinese_title_language_issues(title).is_some()
}

pub(super) fn novel_draft_title_should_refresh(
    previous_title: &str,
    draft: &NovelCreationDraft,
    args: &NovelStudioArgs,
) -> bool {
    if previous_title.trim().is_empty() {
        return true;
    }
    if novel_draft_title_violates_language(&draft.title, &draft.language) {
        return true;
    }
    let user_added_story_direction = !args.genre.trim().is_empty()
        || !args.brief.trim().is_empty()
        || !args.premise.trim().is_empty()
        || !args.outline.trim().is_empty()
        || args.target_units.is_some();
    if !user_added_story_direction {
        return false;
    }
    let task = [
        draft.genre.trim(),
        draft.brief.trim(),
        draft.premise.trim(),
        draft.outline.trim(),
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>()
    .join("；");
    if task.trim().is_empty() {
        return false;
    }
    naming::generated_project_title_looks_stale_for_task(&task, &draft.title)
}

pub(super) fn init_project_title_conflicted(value: &serde_json::Value) -> bool {
    value
        .get("error")
        .and_then(|error| error.as_str())
        .is_some_and(|error| error == "title_conflict")
        || value
            .get("error_kind")
            .and_then(|error| error.as_str())
            .is_some_and(|error| error == "title_conflict")
}
