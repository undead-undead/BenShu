use super::*;

pub(super) fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub(super) fn normalize_language(language: &str) -> String {
    let trimmed = language.trim();
    if trimmed.is_empty() {
        return "zh".to_string();
    }
    trimmed.to_ascii_lowercase()
}

pub(super) fn default_chapter_title(language: &str, number: usize) -> String {
    if is_chinese_language(language) {
        format!("第{number}章")
    } else {
        format!("Chapter {number}")
    }
}

pub(super) fn fallback_chapter_plan_from_manifest(
    manifest: &NovelProjectManifest,
    number: usize,
) -> Option<String> {
    let has_contract = manifest.contract.is_some();
    let has_manifest_brief = !manifest.brief.trim().is_empty();
    if !has_contract && !has_manifest_brief {
        return None;
    }
    let title = default_chapter_title(&manifest.language, number);
    let chapter_target = manifest
        .chapter_unit_target
        .map(|target| target.to_string())
        .unwrap_or_else(|| "unspecified".to_string());
    let project_target = manifest
        .target_units
        .map(|target| target.to_string())
        .unwrap_or_else(|| "unspecified".to_string());
    if is_chinese_language(&manifest.language) {
        let premise = manifest
            .contract
            .as_ref()
            .map(|contract| contract.premise.trim())
            .filter(|premise| !premise.is_empty())
            .unwrap_or_else(|| manifest.brief.trim());
        let outline = manifest
            .contract
            .as_ref()
            .map(|contract| contract.outline.trim())
            .filter(|outline| !outline.is_empty())
            .unwrap_or("延续既定主线，完成本章阶段推进。");
        let volume_context = volume_for_chapter(manifest, number)
            .map(|volume| {
                format!(
                    "当前卷：{}；卷目标：{}；卷收束方向：{}",
                    volume.title,
                    first_non_empty(&[volume.objective.as_str(), "未指定"]),
                    first_non_empty(&[volume.ending_change.as_str(), "未指定"])
                )
            })
            .unwrap_or_default();
        return Some(format!(
            "章节：{title}\n项目：{}\n目标规模：总字数约 {project_target}，本章参考 {chapter_target}\n{}\n本章目标：承接故事合同、当前卷目标与既有连续性，推进一个明确冲突或转折。\n核心前提：{}\n长期主线：{}\n连续性要求：保持已建立的人名、关系、地名、能力边界和未解决伏笔；不得引入与合同冲突的事实。\n产出要求：先 compose_context，再生成章节执行包并持久化为计划/架构，再写正文草稿；写后 audit_chapter，必要时 revise_chapter。",
            manifest.title,
            volume_context,
            premise,
            outline
        ));
    }
    let premise = manifest
        .contract
        .as_ref()
        .map(|contract| contract.premise.trim())
        .filter(|premise| !premise.is_empty())
        .unwrap_or_else(|| manifest.brief.trim());
    let outline = manifest
        .contract
        .as_ref()
        .map(|contract| contract.outline.trim())
        .filter(|outline| !outline.is_empty())
        .unwrap_or("Continue the established main line and advance one concrete conflict or turn.");
    let volume_context = volume_for_chapter(manifest, number)
        .map(|volume| {
            format!(
                "Current volume: {}; objective: {}; ending movement: {}",
                volume.title,
                first_non_empty(&[volume.objective.as_str(), "unspecified"]),
                first_non_empty(&[volume.ending_change.as_str(), "unspecified"])
            )
        })
        .unwrap_or_default();
    Some(format!(
        "Chapter: {title}\nProject: {}\nTarget scale: total units {project_target}, chapter reference {chapter_target}\n{}\nChapter goal: follow the story contract, current volume objective, and continuity, advancing one clear conflict or turn.\nPremise: {}\nLong line: {}\nContinuity requirements: preserve established names, relationships, places, ability boundaries, and unresolved hooks; do not introduce facts that conflict with the contract.\nOutput workflow: compose_context, generate and persist one chapter execution package as plan/architecture, write the draft, audit_chapter, then revise_chapter if needed.",
        manifest.title, volume_context, premise, outline
    ))
}

pub(super) fn first_non_empty<'a>(values: &[&'a str]) -> &'a str {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("untitled")
}

pub(super) fn policy_packet_chapter_body_reference(chapter: Option<&ChapterRecord>) -> String {
    let Some(chapter) = chapter else {
        return "<chapter prose not created yet>".to_string();
    };
    let mut parts = vec![
        format!(
            "<chapter prose stored in artifact {}; not embedded in policy packet>",
            chapter.path
        ),
        format!("unit_count={}", chapter.unit_count),
        format!("status={}", first_non_empty(&[chapter.status.as_str()])),
    ];
    if !chapter.summary.trim().is_empty() {
        parts.push(format!(
            "summary={}",
            truncate_compact_text(&chapter.summary, CHAPTER_SUMMARY_MAX_CHARS)
        ));
    }
    if !chapter.key_facts.is_empty() {
        parts.push(format!(
            "key_facts={}",
            chapter
                .key_facts
                .iter()
                .take(CHAPTER_FACT_LIMIT)
                .map(|fact| truncate_compact_text(fact, CHAPTER_FACT_MAX_CHARS))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    parts.join("; ")
}

pub(super) fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(super) fn contract_v2_from_args(args: &NovelStudioArgs) -> NovelContractV2 {
    let mut contract = NovelContractV2 {
        schema_version: String::new(),
        revision: 0,
        field_requirements: args.field_requirements.clone(),
        resource_economy: args.resource_economy.clone(),
        emotional_contract: args.emotional_contract.clone(),
        emotional_state_ledger: args.emotional_state_ledger.clone(),
        relationship_ledger: args.relationship_ledger.clone(),
        power_progression: args.power_progression.clone(),
        social_order: args.social_order.clone(),
        geography_model: args.geography_model.clone(),
        time_model: args.time_model.clone(),
        artifact_ledger: args.artifact_ledger.clone(),
        antagonist_pressure: args.antagonist_pressure.clone(),
        payoff_matrix: args.payoff_matrix.clone(),
        narration_contract: args.narration_contract.clone(),
        scene_type_mix: args.scene_type_mix.clone(),
        character_voice_ledger: args.character_voice_ledger.clone(),
        reader_promise: args.reader_promise.clone(),
        chapter_ending_rotation: args.chapter_ending_rotation.clone(),
        conflict_pressure_curve: args.conflict_pressure_curve.clone(),
        motif_ledger: args.motif_ledger.clone(),
        reveal_schedule: args.reveal_schedule.clone(),
        relationship_interaction_quotas: args.relationship_interaction_quotas.clone(),
    };
    contract.normalize();
    contract
}

pub(super) fn apply_contract_v2_to_args(args: &mut NovelStudioArgs, contract: &NovelContractV2) {
    args.field_requirements = contract.field_requirements.clone();
    args.resource_economy = contract.resource_economy.clone();
    args.emotional_contract = contract.emotional_contract.clone();
    args.emotional_state_ledger = contract.emotional_state_ledger.clone();
    args.relationship_ledger = contract.relationship_ledger.clone();
    args.power_progression = contract.power_progression.clone();
    args.social_order = contract.social_order.clone();
    args.geography_model = contract.geography_model.clone();
    args.time_model = contract.time_model.clone();
    args.artifact_ledger = contract.artifact_ledger.clone();
    args.antagonist_pressure = contract.antagonist_pressure.clone();
    args.payoff_matrix = contract.payoff_matrix.clone();
    args.narration_contract = contract.narration_contract.clone();
    args.scene_type_mix = contract.scene_type_mix.clone();
    args.character_voice_ledger = contract.character_voice_ledger.clone();
    args.reader_promise = contract.reader_promise.clone();
    args.chapter_ending_rotation = contract.chapter_ending_rotation.clone();
    args.conflict_pressure_curve = contract.conflict_pressure_curve.clone();
    args.motif_ledger = contract.motif_ledger.clone();
    args.reveal_schedule = contract.reveal_schedule.clone();
    args.relationship_interaction_quotas = contract.relationship_interaction_quotas.clone();
}

pub(super) fn contract_v2_has_explicit_input(args: &NovelStudioArgs) -> bool {
    !args.field_requirements.is_empty()
        || !args.resource_economy.currency.trim().is_empty()
        || !args.resource_economy.value_scale.trim().is_empty()
        || !args.resource_economy.resource_types.is_empty()
        || !args.emotional_contract.primary_emotion.trim().is_empty()
        || !args.emotional_contract.emotional_promise.trim().is_empty()
        || !args.emotional_contract.emotional_beats.is_empty()
        || !args.emotional_contract.relief_beats.is_empty()
        || !args.emotional_state_ledger.is_empty()
        || !args.relationship_ledger.is_empty()
        || !args.power_progression.system_name.trim().is_empty()
        || !args.power_progression.levels.is_empty()
        || !args.social_order.institutions.is_empty()
        || !args.social_order.rank_system.trim().is_empty()
        || !args.geography_model.regions.is_empty()
        || !args.geography_model.important_locations.is_empty()
        || !args.time_model.calendar.trim().is_empty()
        || !args.time_model.story_start_time.trim().is_empty()
        || !args.artifact_ledger.is_empty()
        || !args.antagonist_pressure.primary_pressure.trim().is_empty()
        || !args.antagonist_pressure.antagonists.is_empty()
        || !args.payoff_matrix.is_empty()
        || !args.narration_contract.pov.trim().is_empty()
        || !args.narration_contract.chapter_pacing.trim().is_empty()
}

pub(super) fn chapter_execution_contract_v2_from_args(
    args: &NovelStudioArgs,
) -> ChapterExecutionContractV2 {
    ChapterExecutionContractV2 {
        scene_goal: args.scene_goal.trim().to_string(),
        conflict: args.conflict.trim().to_string(),
        choice: args.choice.trim().to_string(),
        cost: args.cost.trim().to_string(),
        reveal: args.reveal.trim().to_string(),
        emotional_beat: args.emotional_beat.trim().to_string(),
        new_state_after_chapter: args.new_state_after_chapter.trim().to_string(),
        relationship_delta: args.relationship_delta.trim().to_string(),
        power_delta: args.power_delta.trim().to_string(),
        resource_delta: args.resource_delta.trim().to_string(),
        hook_opened: clean_list(&args.hook_opened),
        hook_paid_off: clean_list(&args.hook_paid_off),
        character_change: args.character_change.trim().to_string(),
        world_change: args.world_change.trim().to_string(),
        payoff_target: args.payoff_target.trim().to_string(),
        new_character_requests: args.new_character_requests.clone(),
        character_registrations: Vec::new(),
    }
}

pub(super) fn clean_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn clean_contract_list(values: &[String]) -> Vec<String> {
    clean_list(values)
        .into_iter()
        .map(|value| sanitize_contract_text(&value))
        .filter(|value| !value.trim().is_empty())
        .collect()
}

pub(super) fn sanitize_contract_text(value: &str) -> String {
    surface_sanitizer::sanitize_contract_surface_text(&normalize_cjk_separator_punctuation(
        &strip_cjk_separator_noise(value),
    ))
    .replace("世界观意意象", "世界观意象")
    .trim()
    .to_string()
}

fn strip_cjk_separator_noise(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut out = String::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        if matches!(ch, '_' | '^')
            && index > 0
            && index + 1 < chars.len()
            && is_cjk_unified(chars[index - 1])
            && is_cjk_unified(chars[index + 1])
        {
            continue;
        }
        out.push(ch);
    }
    out
}

fn normalize_cjk_separator_punctuation(value: &str) -> String {
    let mut normalized = value.to_string();
    for (from, to) in [
        ("。；", "。"),
        ("。;", "。"),
        ("；。", "。"),
        (";.", "."),
        ("；；", "；"),
        (";;", ";"),
        ("。。", "。"),
        ("..", "."),
    ] {
        while normalized.contains(from) {
            normalized = normalized.replace(from, to);
        }
    }
    normalized
}

pub(super) fn ensure_text_size(text: &str, field: &str) -> anyhow::Result<()> {
    if text.len() > MAX_SINGLE_TEXT_BYTES {
        anyhow::bail!("{field} exceeds the 8MB single-call safety limit");
    }
    Ok(())
}

pub(super) fn replace_list_if_present(target: &mut Vec<String>, values: &[String]) {
    if !values.is_empty() {
        *target = clean_list(values);
    }
}

pub(super) fn inferred_chapter_unit_target(target_units: Option<usize>) -> Option<usize> {
    let target = target_units.filter(|target| *target > 0)?;
    Some(longform_policy::dynamic_chapter_unit_target(Some(target)))
}

pub(super) fn with_stage(
    mut value: serde_json::Value,
    stage: &str,
    next_action: &str,
) -> serde_json::Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("stage".to_string(), json!(stage));
        object.insert("next_action".to_string(), json!(next_action));
        object.insert(
            "writing_policy".to_string(),
            policy::fiction_stage_policy(stage, next_action),
        );
    }
    value
}
