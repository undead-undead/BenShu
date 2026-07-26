use super::*;
use crate::tool::writing::creation_contract::issue::ContractIssueList;

pub(super) fn validate_structured_contract_fields(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
    scope: ContractReadinessScope,
) {
    issues.set_scope(
        "contract.structured_governance",
        crate::tool::writing::creation_contract::issue::ContractIssueKind::Governance,
        "structured",
    );
    let structured = &contract.structured;
    if surface_gate::structured_contract_contains_legal_residue(structured) {
        issues.push(
            "ContractBlocker: 小说合同结构化字段含有法律合同、交付协议或甲乙方条款残片".to_string(),
        );
    }
    let surface_noise_paths = surface_gate::structured_contract_surface_noise_paths(structured);
    if !surface_noise_paths.is_empty() {
        let paths = surface_noise_paths
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("、");
        issues.push(format!(
            "ContractBlocker: 小说合同结构化字段含有异常重复文字噪声，需要重新补齐字段：{paths}"
        ));
    }
    validate_structured_scalar_slot_pollution(contract, issues);
    let authority_names = contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    validate_structured_external_character_references(contract, &authority_names, issues);
    validate_structured_primary_role_references(contract, issues);
    validate_structured_volume_references(contract, issues);
    let emotion = &structured.emotional_contract;
    if field_strength(contract, "emotional_contract").blocks_for_scope(scope, "emotional_contract")
        && value_missing(&emotion.primary_emotion)
        && value_missing(&emotion.emotional_promise)
        && emotion.emotional_beats.is_empty()
    {
        issues.push("ContractBlocker: 小说合同缺少情感承诺或情绪推进线".to_string());
    }
    if field_strength(contract, "emotional_contract").blocks_for_scope(scope, "emotional_contract")
        && value_missing(&emotion.ending_emotional_state)
    {
        issues.push("ContractBlocker: 小说合同缺少终局情绪落点".to_string());
    }
    if field_strength(contract, "relationship_ledger")
        .blocks_for_scope(scope, "relationship_ledger")
        && structured.relationship_ledger.is_empty()
    {
        issues.push("ContractBlocker: 小说合同缺少关系线或关键人物关系账本".to_string());
    } else if field_strength(contract, "relationship_ledger")
        .blocks_for_scope(scope, "relationship_ledger")
        && structured.relationship_ledger.iter().all(|relation| {
            relation.characters.is_empty()
                || (value_missing(&relation.relationship_type)
                    && value_missing(&relation.start_state)
                    && value_missing(&relation.desired_end_state))
        })
    {
        issues.push("ContractBlocker: 小说合同关系线缺少人物、起点或终点".to_string());
    }
    for relation in &structured.relationship_ledger {
        if relationship_ledger_entry_uses_generic_placeholder(relation) {
            issues.push(
                "ContractBlocker: 小说合同关系账本仍使用通用占位关系，必须根据当前故事重写关系类型、阶段和冲突变化"
                    .to_string(),
            );
        }
        if field_strength(contract, "relationship_ledger")
            .blocks_for_scope(scope, "relationship_ledger")
            && relation
                .characters
                .iter()
                .filter(|name| {
                    let name = name.trim();
                    !value_missing(name) && authority_names.iter().any(|known| *known == name)
                })
                .count()
                < 2
        {
            issues.push(
                "ContractBlocker: 小说合同关系线必须包含至少两个角色权威表内角色".to_string(),
            );
        }
        for name in &relation.characters {
            let name = name.trim();
            if !value_missing(name) && !authority_names.iter().any(|known| *known == name) {
                issues.push(format!(
                    "ContractBlocker: 关系线角色 `{name}` 不在角色权威表中"
                ));
            }
        }
    }
    validate_relationship_ledger_roles(contract, issues);
    for entry in &structured.emotional_state_ledger {
        let name = entry.character.trim();
        if !value_missing(name) && !authority_names.iter().any(|known| *known == name) {
            issues.push(format!(
                "ContractBlocker: 情绪状态角色 `{name}` 不在角色权威表中"
            ));
        }
    }
    let payoff_matrix_blocks_when_missing =
        field_strength(contract, "payoff_matrix").blocks_for_scope(scope, "payoff_matrix");
    if payoff_matrix_blocks_when_missing || !structured.payoff_matrix.is_empty() {
        issues.set_scope(
            "contract.payoff_matrix",
            crate::tool::writing::creation_contract::issue::ContractIssueKind::Plot,
            "payoff_matrix",
        );
        if structured.payoff_matrix.is_empty() {
            if payoff_matrix_blocks_when_missing {
                issues.push("ContractBlocker: 小说合同缺少伏笔/承诺兑现矩阵".to_string());
            }
        } else {
            let entry_value_missing = |value: &str| {
                if payoff_matrix_blocks_when_missing {
                    value_missing(value)
                } else {
                    value.trim().is_empty()
                }
            };
            for (index, entry) in structured.payoff_matrix.iter().enumerate() {
                if entry_value_missing(&entry.promise) {
                    issues.push(format!(
                        "ContractBlocker: 小说合同兑现矩阵第{}项缺少具体承诺或伏笔",
                        index + 1
                    ));
                }
                if entry_value_missing(&entry.payoff_target) {
                    issues.push(format!(
                        "ContractBlocker: 小说合同兑现矩阵第{}项缺少具体兑现目标",
                        index + 1
                    ));
                }
                if entry_value_missing(&entry.status) {
                    issues.push(format!(
                        "ContractBlocker: 小说合同兑现矩阵第{}项缺少生命周期状态",
                        index + 1
                    ));
                }
            }
        }
        issues.set_scope(
            "contract.structured_governance",
            crate::tool::writing::creation_contract::issue::ContractIssueKind::Governance,
            "structured",
        );
    }
    if field_strength(contract, "power_progression").blocks_for_scope(scope, "power_progression")
        && value_missing(&structured.power_progression.system_name)
        && structured.power_progression.levels.is_empty()
    {
        issues
            .push("ContractBlocker: 小说合同缺少当前题材要求的成长/权限/能力进阶约束".to_string());
    } else if field_strength(contract, "power_progression")
        .blocks_for_scope(scope, "power_progression")
        && contract_power_progression_is_generic(contract)
    {
        issues.push(
            "ContractBlocker: 小说合同成长体系只是重复世界观意象，缺少等级、代价、瓶颈或失控后果"
                .to_string(),
        );
    }
    if field_strength(contract, "resource_economy").blocks_for_scope(scope, "resource_economy")
        && value_missing(&structured.resource_economy.currency)
        && value_missing(&structured.resource_economy.value_scale)
        && structured.resource_economy.resource_types.is_empty()
    {
        issues.push("ContractBlocker: 小说合同缺少当前题材要求的资源/货币/能源约束".to_string());
    } else if field_strength(contract, "resource_economy")
        .blocks_for_scope(scope, "resource_economy")
        && contract_resource_economy_is_generic(contract)
    {
        issues.push(
            "ContractBlocker: 小说合同资源体系只是重复世界观意象，缺少资源类型、消耗、稀缺或交易规则"
                .to_string(),
        );
    }
    if field_strength(contract, "social_order").blocks_for_scope(scope, "social_order")
        && structured.social_order.institutions.is_empty()
        && value_missing(&structured.social_order.rank_system)
    {
        issues.push("ContractBlocker: 小说合同缺少当前题材要求的社会秩序或机构压力".to_string());
    } else if field_strength(contract, "social_order").blocks_for_scope(scope, "social_order")
        && contract_social_order_is_generic(contract)
    {
        issues.push(
            "ContractBlocker: 小说合同社会秩序只是重复世界观意象，缺少机构、阶层、晋升或权力冲突"
                .to_string(),
        );
    }
    if field_strength(contract, "time_model").blocks_for_scope(scope, "time_model")
        && value_missing(&structured.time_model.story_start_time)
        && structured.time_model.deadline_events.is_empty()
    {
        issues.push("ContractBlocker: 小说合同缺少当前题材要求的时间模型或期限事件".to_string());
    }
    let narration = &structured.narration_contract;
    if field_strength(contract, "narration_contract").blocks_for_scope(scope, "narration_contract")
        && value_missing(&narration.pov)
        && value_missing(&narration.narrative_distance)
        && value_missing(&narration.dialogue_style)
    {
        issues.push("ContractBlocker: 小说合同缺少叙事视角或语气合同".to_string());
    }
    validate_aesthetic_contract_fields(contract, issues, scope);
    if contract.language.to_ascii_lowercase().starts_with("zh")
        || contract.language.contains("中文")
        || contract
            .story_basis_text()
            .chars()
            .any(surface_gate::is_cjk_unified)
    {
        for (label, value) in [
            ("叙事视角", narration.pov.as_str()),
            ("叙事距离", narration.narrative_distance.as_str()),
            ("对白风格", narration.dialogue_style.as_str()),
        ] {
            if surface_gate::contains_latin_word(value) {
                issues.push(format!("ContractBlocker: 中文小说合同{label}混入英文残片"));
            }
        }
    }
}

fn validate_relationship_ledger_roles(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    for relation in &contract.structured.relationship_ledger {
        if !relationship_ledger_has_explicit_intimate_destination(relation) {
            continue;
        }
        for name in &relation.characters {
            let name = name.trim();
            let Some(character) = contract
                .characters
                .iter()
                .find(|character| character.canonical_name.trim() == name)
            else {
                continue;
            };
            if character.role_looks_primary()
                || character_role_supports_intimate_relationship(&character.role)
            {
                continue;
            }
            issues.push(format!(
                "ContractBlocker: 小说合同关系账本把 `{name}` 的终局明确写成恋爱/伴侣关系，但角色权威表定位是 `{}`；必须统一角色权威与关系终局",
                character.role.trim()
            ));
        }
    }
}

fn relationship_ledger_has_explicit_intimate_destination(
    relation: &super::super::novel_contract_v2::RelationshipLedgerEntry,
) -> bool {
    let joined = [
        relation.next_expected_stage.as_str(),
        relation.desired_end_state.as_str(),
    ]
    .join(" ");
    let lowered = joined.to_ascii_lowercase();
    [
        "恋人", "爱人", "伴侣", "相爱", "恋爱", "婚姻", "订婚", "结婚", "夫妻",
    ]
    .iter()
    .any(|marker| joined.contains(marker))
        || ["romance", "romantic", "lover", "spouse"]
            .iter()
            .any(|marker| lowered.contains(marker))
}

fn character_role_supports_intimate_relationship(role: &str) -> bool {
    let lowered = role.to_ascii_lowercase();
    [
        "关系对象",
        "情感对象",
        "恋人",
        "爱人",
        "伴侣",
        "男主",
        "女主",
    ]
    .iter()
    .any(|marker| role.contains(marker))
        || ["love interest", "romantic", "lover", "spouse"]
            .iter()
            .any(|marker| lowered.contains(marker))
}

fn relationship_ledger_entry_uses_generic_placeholder(
    relation: &super::super::novel_contract_v2::RelationshipLedgerEntry,
) -> bool {
    let joined = [
        relation.arc_type.as_str(),
        relation.relationship_type.as_str(),
        relation.stage.as_str(),
        relation.next_expected_stage.as_str(),
        relation.current_state.as_str(),
        relation.desired_end_state.as_str(),
    ]
    .join(" ");
    let compact = joined.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    let generic_hits = [
        "relationship",
        "主角核心关系",
        "主角与关键压力源",
        "protagonistcorerelationship",
        "protagonistandkeypressuresource",
        "建立关系",
        "产生变化",
        "承受考验",
        "完成兑现",
    ]
    .iter()
    .filter(|marker| compact.contains(**marker) || joined.to_ascii_lowercase().contains(**marker))
    .count();
    generic_hits >= 2
        && relation.characters.len() <= 1
        && relation.conflicts.is_empty()
        && relation.turning_points.is_empty()
        && value_missing(&relation.desired_end_state)
}

fn validate_aesthetic_contract_fields(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
    scope: ContractReadinessScope,
) {
    let structured = &contract.structured;
    if field_strength(contract, "scene_type_mix").blocks_for_scope(scope, "scene_type_mix")
        && scene_type_mix_is_generic_or_incomplete(contract)
    {
        issues.push(
            "ContractBlocker: 小说合同缺少具体场景类型配比，不能只用通用轮换说明".to_string(),
        );
    }
    if field_strength(contract, "character_voice_ledger")
        .blocks_for_scope(scope, "character_voice_ledger")
        && character_voice_ledger_is_incomplete(contract)
    {
        issues.push("ContractBlocker: 小说合同缺少角色声音表".to_string());
    }
    if contract
        .structured
        .character_voice_ledger
        .iter()
        .any(character_voice_profile_contains_unresolved_placeholder)
    {
        issues.push(
            "ContractBlocker: 小说合同角色声音表含有未补齐的角色欲望、恐惧或底线占位".to_string(),
        );
    }
    if field_strength(contract, "reader_promise").blocks_for_scope(scope, "reader_promise")
        && reader_promise_is_generic_or_incomplete(contract)
    {
        issues.push("ContractBlocker: 小说合同缺少读者期待/爽点合同".to_string());
    }
    if field_strength(contract, "conflict_pressure_curve")
        .blocks_for_scope(scope, "conflict_pressure_curve")
        && structured.conflict_pressure_curve.global_curve.is_empty()
        && value_missing(&structured.conflict_pressure_curve.release_strategy)
        && value_missing(&structured.conflict_pressure_curve.peak_policy)
    {
        issues.push("ContractBlocker: 小说合同缺少冲突升降压曲线".to_string());
    }
    if field_strength(contract, "motif_ledger").blocks_for_scope(scope, "motif_ledger")
        && structured.motif_ledger.is_empty()
    {
        issues.push("ContractBlocker: 小说合同缺少主题母题账本".to_string());
    }
    if field_strength(contract, "reveal_schedule").blocks_for_scope(scope, "reveal_schedule")
        && structured.reveal_schedule.is_empty()
    {
        issues.push("ContractBlocker: 小说合同缺少信息揭示节奏表".to_string());
    }
    if field_strength(contract, "relationship_interaction_quotas")
        .blocks_for_scope(scope, "relationship_interaction_quotas")
        && relationship_interaction_quotas_are_incomplete(contract)
    {
        issues.push("ContractBlocker: 小说合同缺少角色关系互动配额".to_string());
    }
    validate_antagonist_pressure_roles(contract, issues);
}

fn validate_antagonist_pressure_roles(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    for antagonist in &contract.structured.antagonist_pressure.antagonists {
        let name = antagonist.name.trim();
        if value_missing(name) {
            continue;
        }
        let Some(character) = contract
            .characters
            .iter()
            .find(|character| character.canonical_name.trim() == name)
        else {
            issues.push(format!(
                "ContractBlocker: 小说合同对手压力引用了角色权威表外角色 `{name}`"
            ));
            continue;
        };
        if antagonist_pressure_uses_hostile_action(antagonist)
            && character_role_is_explicitly_supportive(&character.role)
        {
            issues.push(format!(
                "ContractBlocker: 小说合同对手压力把 `{name}`（{}）这个明确正向角色写成敌对行动者，必须改成真实压力源，或调整该角色在角色权威表中的功能定位",
                character.role.trim()
            ));
        }
    }
}

fn antagonist_pressure_uses_hostile_action(
    antagonist: &super::super::novel_contract_v2::AntagonistRecord,
) -> bool {
    let joined = [
        antagonist.goal.as_str(),
        antagonist.knowledge_state.as_str(),
        antagonist.current_move.as_str(),
        antagonist.defeat_condition.as_str(),
        &antagonist.escalation_plan.join(" "),
    ]
    .join(" ");
    let hostile_markers = [
        "吞噬", "封锁", "掌控", "毁灭", "夺取", "压制", "追杀", "篡改", "抹除", "操控", "垄断",
        "献祭", "背叛", "陷害", "清算", "威胁", "勒索", "剥削", "封口", "反派", "敌对", "夺权",
    ];
    hostile_markers.iter().any(|marker| joined.contains(marker))
}

fn character_role_is_explicitly_supportive(role: &str) -> bool {
    let role = role.trim();
    if value_missing(role) {
        return false;
    }
    let lowered = role.to_ascii_lowercase();
    [
        "主角", "同伴", "伙伴", "盟友", "朋友", "挚友", "恋人", "爱人", "亲人", "家人", "女主",
        "男主", "助手",
    ]
    .iter()
    .any(|marker| role.contains(marker))
        || lowered.contains("protagonist")
        || lowered.contains("companion")
        || lowered.contains("ally")
        || lowered.contains("friend")
        || lowered.contains("lover")
}

fn scene_type_mix_is_generic_or_incomplete(contract: &NovelCreationContract) -> bool {
    let mix = &contract.structured.scene_type_mix;
    let specific_slots = [
        mix.action.as_str(),
        mix.dialogue.as_str(),
        mix.everyday.as_str(),
        mix.reveal.as_str(),
        mix.emotional.as_str(),
        mix.turning_point.as_str(),
    ]
    .into_iter()
    .filter(|value| !value_missing(value))
    .count();
    specific_slots < 2 || text_is_generic_default(&mix.balance_rule)
}

fn character_voice_ledger_is_incomplete(contract: &NovelCreationContract) -> bool {
    let authority_names = contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    if authority_names.is_empty() {
        return true;
    }
    let valid_profiles = contract
        .structured
        .character_voice_ledger
        .iter()
        .filter(|voice| {
            let character = voice.character.trim();
            !value_missing(character)
                && authority_names.iter().any(|known| *known == character)
                && !character_voice_profile_contains_unresolved_placeholder(voice)
                && (!value_missing(&voice.voice_style) || !voice.dialogue_rules.is_empty())
        })
        .count();
    valid_profiles == 0
}

fn character_voice_profile_contains_unresolved_placeholder(
    voice: &super::super::novel_contract_v2::CharacterVoiceProfile,
) -> bool {
    value_missing(&voice.character)
        || value_missing(&voice.voice_style)
        || voice.catchphrases.iter().any(|value| value_missing(value))
        || voice
            .forbidden_expressions
            .iter()
            .any(|value| value_missing(value))
        || voice
            .dialogue_rules
            .iter()
            .any(|value| value_missing(value))
        || text_contains_unresolved_anchor_placeholder(&voice.voice_style)
        || voice
            .dialogue_rules
            .iter()
            .any(|value| text_contains_unresolved_anchor_placeholder(value))
}

fn reader_promise_is_generic_or_incomplete(contract: &NovelCreationContract) -> bool {
    let promise = &contract.structured.reader_promise;
    value_missing(&promise.core_hook)
        || text_is_generic_default(&promise.core_hook)
        || (promise.pleasure_points.is_empty()
            && value_missing(&promise.curiosity_engine)
            && value_missing(&promise.payoff_style))
}

fn relationship_interaction_quotas_are_incomplete(contract: &NovelCreationContract) -> bool {
    let authority_names = contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    contract
        .structured
        .relationship_interaction_quotas
        .iter()
        .all(|quota| {
            let known_characters = quota
                .characters
                .iter()
                .filter(|name| authority_names.iter().any(|known| *known == name.trim()))
                .count();
            known_characters < 2
                || value_missing(&quota.cadence)
                || value_missing(&quota.required_interaction)
        })
}

fn validate_structured_scalar_slot_pollution(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    for (label, value) in [
        (
            "资源尺度",
            contract.structured.resource_economy.value_scale.as_str(),
        ),
        (
            "成长体系名",
            contract.structured.power_progression.system_name.as_str(),
        ),
        (
            "社会等级/秩序",
            contract.structured.social_order.rank_system.as_str(),
        ),
        (
            "主要对手压力",
            contract
                .structured
                .antagonist_pressure
                .primary_pressure
                .as_str(),
        ),
    ] {
        if crate::tool::writing::surface_sanitizer::contains_creation_request_control_residue(value)
        {
            issues.push(format!(
                "ContractBlocker: 小说合同结构化字段 `{label}` 混入用户请求参数或流程说明，不能作为创作内容"
            ));
        }
        if structured_scalar_slot_contains_story_summary(value, contract) {
            issues.push(format!(
                "ContractBlocker: 小说合同结构化字段 `{label}` 像把剧情摘要、主线因果或世界观长句塞进了短字段槽位"
            ));
        }
        if let Some(reference) = authority_name_glued_to_person_fragment(value, contract) {
            issues.push(format!(
                "ContractBlocker: 小说合同结构化字段 `{label}` 含有角色名拼接污染 `{reference}`，必须重新生成干净字段"
            ));
        }
    }

    for relation in &contract.structured.relationship_ledger {
        if structured_scalar_slot_contains_story_summary(&relation.relationship_type, contract) {
            issues.push(
                "ContractBlocker: 小说合同关系类型像把剧情摘要或主线因果塞进了关系字段槽位"
                    .to_string(),
            );
        }
    }
}

fn validate_structured_external_character_references(
    contract: &NovelCreationContract,
    authority_names: &[&str],
    issues: &mut ContractIssueList,
) {
    if authority_names.is_empty() {
        return;
    }
    let non_character_terms = non_character_contract_terms(contract);
    let mut fields = Vec::<(&'static str, &str)>::new();
    collect_structured_reference_fields(contract, &mut fields);
    for (label, text) in fields {
        if value_missing(text) {
            continue;
        }
        character_gate::validate_text_character_references(
            label,
            text,
            authority_names,
            &non_character_terms,
            issues,
        );
    }
}

fn validate_structured_primary_role_references(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    let primary_names = contract
        .characters
        .iter()
        .filter(|character| character.role_looks_primary())
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    if primary_names.is_empty() {
        return;
    }
    let mut fields = Vec::<(&'static str, &str)>::new();
    collect_structured_reference_fields(contract, &mut fields);
    for (label, text) in fields {
        if value_missing(text) {
            continue;
        }
        for reference in character_gate::primary_role_person_references(text) {
            if primary_names
                .iter()
                .any(|primary| character_gate::authority_name_prefix_matches(&reference, primary))
                || character_gate::reference_matches_authority_name_in_text(
                    &reference,
                    text,
                    &primary_names,
                )
            {
                continue;
            }
            issues.push(format!(
                "ContractBlocker: 小说合同{label}把 `{reference}` 标成主角，但角色权威表主角是 `{}`",
                primary_names.join(" / ")
            ));
        }
    }
}

fn validate_structured_volume_references(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    let volume_count = contract.outline.volumes.len();
    let mut fields = Vec::<(&'static str, &str)>::new();
    collect_structured_reference_fields(contract, &mut fields);
    for (label, text) in fields {
        let Some(volume) =
            character_gate::first_volume_reference_outside_contract(text, volume_count)
        else {
            continue;
        };
        issues.push(format!(
            "ContractBlocker: 小说合同{label}引用第{volume}卷，但合同只有{volume_count}卷；必须按实际分卷重写结构化计划"
        ));
    }
}

fn collect_structured_reference_fields<'a>(
    contract: &'a NovelCreationContract,
    fields: &mut Vec<(&'static str, &'a str)>,
) {
    let structured = &contract.structured;
    fields.extend([
        (
            "情感承诺",
            structured.emotional_contract.primary_emotion.as_str(),
        ),
        (
            "情感承诺",
            structured.emotional_contract.emotional_promise.as_str(),
        ),
        (
            "情感终局",
            structured
                .emotional_contract
                .ending_emotional_state
                .as_str(),
        ),
        (
            "叙事视角",
            structured.narration_contract.dialogue_style.as_str(),
        ),
    ]);
    for value in &structured.emotional_contract.relief_beats {
        fields.push(("缓冲节拍", value));
    }
    for value in &structured.emotional_contract.payoff_requirements {
        fields.push(("情感兑现", value));
    }
    for entry in &structured.emotional_state_ledger {
        fields.extend([
            ("情绪状态", entry.current_emotion.as_str()),
            ("情绪压力", entry.pressure.as_str()),
            ("情绪欲望", entry.desire.as_str()),
            ("情绪恐惧", entry.fear.as_str()),
            ("情绪变化", entry.expected_next_shift.as_str()),
            ("情绪兑现", entry.payoff_target.as_str()),
        ]);
    }
    for relation in &structured.relationship_ledger {
        fields.extend([
            ("关系类型", relation.arc_type.as_str()),
            ("关系类型", relation.relationship_type.as_str()),
            ("关系阶段", relation.stage.as_str()),
            ("关系阶段", relation.next_expected_stage.as_str()),
            ("关系状态", relation.start_state.as_str()),
            ("关系状态", relation.current_state.as_str()),
            ("关系目标", relation.desired_end_state.as_str()),
            ("关系证据", relation.evidence.as_str()),
        ]);
        for value in &relation.conflicts {
            fields.push(("关系冲突", value));
        }
        for value in &relation.secrets {
            fields.push(("关系秘密", value));
        }
        for value in &relation.turning_points {
            fields.push(("关系转折", value));
        }
    }
    for entry in &structured.payoff_matrix {
        fields.extend([
            ("兑现矩阵", entry.promise.as_str()),
            ("兑现矩阵", entry.payoff_target.as_str()),
            ("兑现矩阵", entry.status.as_str()),
        ]);
        for value in &entry.evidence {
            fields.push(("兑现证据", value));
        }
    }
    for voice in &structured.character_voice_ledger {
        fields.push(("角色声音", voice.voice_style.as_str()));
        for value in &voice.catchphrases {
            fields.push(("角色口癖", value));
        }
        for value in &voice.forbidden_expressions {
            fields.push(("角色表达禁忌", value));
        }
        for value in &voice.dialogue_rules {
            fields.push(("角色对白规则", value));
        }
    }
    // primary_pressure is allowed to name events, systems, institutions, or
    // disasters such as "王陵崩塌"; concrete named actors are validated through
    // antagonist entries below.
    // relationship_interaction_quotas owns explicit character references through
    // its `characters` field, which is validated by
    // relationship_interaction_quotas_are_incomplete. Cadence and required
    // interaction prose often contains scheduling fragments like "每3章推进";
    // running the generic person-name scanner over those fields creates false
    // positives because some ordinary CJK words start with surname characters.
    for antagonist in &structured.antagonist_pressure.antagonists {
        fields.extend([
            ("对手目标", antagonist.goal.as_str()),
            ("对手认知", antagonist.knowledge_state.as_str()),
            ("对手失败条件", antagonist.defeat_condition.as_str()),
        ]);
        for value in &antagonist.escalation_plan {
            fields.push(("对手升级计划", value));
        }
    }
    // Emotional beats, antagonist moves, conflict curves, motif, and reveal
    // prose often name abstract concepts, symbols, locations, facts, and world
    // mechanisms. Explicit character-bearing slots above already validate
    // named actors, so do not run the generic person scanner over these fields.
}

pub(super) fn non_character_contract_terms(contract: &NovelCreationContract) -> Vec<String> {
    let mut terms = Vec::new();
    push_quoted_non_character_terms(&mut terms, &contract.premise);
    push_quoted_non_character_terms(&mut terms, &contract.main_causal_spine);
    push_quoted_non_character_terms(&mut terms, &contract.protagonist_arc);
    push_quoted_non_character_terms(&mut terms, &contract.world_imagery);
    push_quoted_non_character_terms(&mut terms, &contract.ending.desired_resolution);
    push_quoted_non_character_terms(&mut terms, &contract.ending.final_state);
    for value in &contract.world_rules {
        push_quoted_non_character_terms(&mut terms, value);
    }
    for value in &contract.themes {
        push_exact_non_character_term(&mut terms, value);
    }
    let structured = &contract.structured;
    push_exact_non_character_term(&mut terms, &structured.power_progression.system_name);
    push_exact_non_character_term(&mut terms, &structured.resource_economy.currency);
    for value in &structured.resource_economy.resource_types {
        push_exact_non_character_term(&mut terms, value);
    }
    for value in &structured.reader_promise.pleasure_points {
        push_exact_non_character_term(&mut terms, value);
    }
    for value in &structured.social_order.institutions {
        push_exact_non_character_term(&mut terms, value);
    }
    for value in &structured.geography_model.regions {
        push_exact_non_character_term(&mut terms, value);
    }
    for location in &structured.geography_model.important_locations {
        push_exact_non_character_term(&mut terms, &location.name);
    }
    for artifact in &structured.artifact_ledger {
        push_exact_non_character_term(&mut terms, &artifact.name);
    }
    for motif in &structured.motif_ledger {
        push_exact_non_character_term(&mut terms, &motif.motif);
    }
    terms.sort();
    terms.dedup();
    terms
}

fn push_exact_non_character_term(out: &mut Vec<String>, value: &str) {
    let trimmed = value.trim();
    let len = trimmed.chars().count();
    if (2..=12).contains(&len)
        && !trimmed.contains(char::is_whitespace)
        && !trimmed.contains('，')
        && !trimmed.contains('。')
        && !trimmed.contains('；')
        && !trimmed.contains('：')
        && !trimmed.contains(',')
        && !trimmed.contains(';')
        && !trimmed.contains(':')
    {
        out.push(trimmed.to_string());
    }
}

fn push_quoted_non_character_terms(out: &mut Vec<String>, value: &str) {
    for (open, close) in [
        ('《', '》'),
        ('「', '」'),
        ('『', '』'),
        ('“', '”'),
        ('‘', '’'),
    ] {
        let mut rest = value;
        while let Some(start) = rest.find(open) {
            let after_start = &rest[start + open.len_utf8()..];
            let Some(end) = after_start.find(close) else {
                break;
            };
            push_exact_non_character_term(out, &after_start[..end]);
            rest = &after_start[end + close.len_utf8()..];
        }
    }
}

#[cfg(test)]
mod local_term_tests {
    use super::*;
    use crate::tool::writing::novel_contract_v2::{ArtifactLedgerEntry, PayoffMatrixEntry};

    #[test]
    fn non_character_terms_keep_quoted_artifacts_without_free_text_windows() {
        let mut contract = NovelCreationContract::default();
        contract.premise = "主角林烬得到《混沌经》，遇见白阙砺。".to_string();
        contract
            .structured
            .artifact_ledger
            .push(ArtifactLedgerEntry {
                name: "断碑".to_string(),
                ..Default::default()
            });

        let terms = non_character_contract_terms(&contract);

        assert!(terms.iter().any(|term| term == "混沌经"), "{terms:?}");
        assert!(terms.iter().any(|term| term == "断碑"), "{terms:?}");
        assert!(
            !terms.iter().any(|term| term == "林烬" || term == "白阙砺"),
            "free-text character-like names must not become non-character exemptions: {terms:?}"
        );
    }

    #[test]
    fn structured_plan_cannot_reference_a_volume_outside_the_outline() {
        let mut contract = NovelCreationContract::default();
        contract.outline.volumes = (1..=4)
            .map(
                |index| super::super::super::creation_contract_model::VolumeContract {
                    title: format!("第{index}卷"),
                    objective: format!("推进第{index}阶段"),
                    ending_change: format!("形成第{index}阶段变化"),
                },
            )
            .collect();
        contract.structured.payoff_matrix = vec![PayoffMatrixEntry {
            promise: "旧芯片保存企业审计记录".to_string(),
            payoff_target: "第五卷终局公开全部记录".to_string(),
            status: "planned".to_string(),
            ..Default::default()
        }];

        let mut issues = ContractIssueList::default();
        validate_structured_volume_references(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("兑现矩阵引用第5卷") && issue.contains("只有4卷")),
            "structured references must stay inside the actual volume plan: {issues:?}"
        );
    }

    #[test]
    fn structured_plan_cannot_reference_volumes_without_a_volume_outline() {
        let mut contract = NovelCreationContract::default();
        contract.structured.payoff_matrix = vec![PayoffMatrixEntry {
            promise: "记忆碎片保存设计师证据".to_string(),
            payoff_target: "卷三揭示机制，卷五完成终局兑现".to_string(),
            status: "planned".to_string(),
            ..Default::default()
        }];

        let mut issues = ContractIssueList::default();
        validate_structured_volume_references(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("兑现矩阵引用第3卷") && issue.contains("只有0卷")),
            "structured volume promises require an actual volume plan: {issues:?}"
        );
    }

    #[test]
    fn structured_plan_may_reference_the_actual_final_volume() {
        let mut contract = NovelCreationContract::default();
        contract.outline.volumes = (1..=4)
            .map(
                |index| super::super::super::creation_contract_model::VolumeContract {
                    title: format!("第{index}卷"),
                    objective: format!("推进第{index}阶段"),
                    ending_change: format!("形成第{index}阶段变化"),
                },
            )
            .collect();
        contract.structured.payoff_matrix = vec![PayoffMatrixEntry {
            promise: "旧芯片保存企业审计记录".to_string(),
            payoff_target: "第四卷终局公开全部记录".to_string(),
            status: "planned".to_string(),
            ..Default::default()
        }];

        let mut issues = ContractIssueList::default();
        validate_structured_volume_references(&contract, &mut issues);

        assert!(issues.is_empty(), "{issues:?}");
    }
}

fn structured_scalar_slot_contains_story_summary(
    value: &str,
    contract: &NovelCreationContract,
) -> bool {
    let normalized_value = super::normalized_contract_text(value);
    if normalized_value.chars().count() < 18 {
        return false;
    }
    let looks_like_sentence_or_chain = value.contains('：')
        || value.contains(':')
        || value.contains('→')
        || value.contains('。')
        || value.contains('；')
        || value.contains('，');
    let story_anchor = [
        contract.premise.as_str(),
        contract.main_causal_spine.as_str(),
        contract.protagonist_arc.as_str(),
        contract.world_imagery.as_str(),
        contract.ending.desired_resolution.as_str(),
        contract.ending.final_state.as_str(),
    ]
    .iter()
    .filter(|anchor| !value_missing(anchor))
    .map(|anchor| super::normalized_contract_text(anchor))
    .any(|anchor| {
        let anchor_len = anchor.chars().count();
        anchor_len >= 12
            && (normalized_value == anchor
                || normalized_value.contains(&anchor)
                || (looks_like_sentence_or_chain
                    && anchor.contains(&normalized_value)
                    && normalized_value.chars().count() >= 24))
    });

    story_anchor && (looks_like_sentence_or_chain || normalized_value.chars().count() >= 28)
}

fn text_is_generic_default(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    compact.is_empty()
        || compact.contains("根据题材")
        || compact.contains("当前题材")
        || compact.contains("形成持续阅读期待")
        || compact.contains("避免连续章节形态单一")
}

fn text_contains_unresolved_anchor_placeholder(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    [
        "未明欲望",
        "未明恐惧",
        "未明底线",
        "`目标`",
        "`顾虑`",
        "`底线`",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
}

fn authority_name_glued_to_person_fragment(
    value: &str,
    contract: &NovelCreationContract,
) -> Option<String> {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return None;
    }
    for known in contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
    {
        let known_chars = known.chars().collect::<Vec<_>>();
        if known_chars.is_empty() || chars.len() <= known_chars.len() {
            continue;
        }
        for index in 0..=chars.len() - known_chars.len() {
            if chars[index..index + known_chars.len()] != known_chars[..] {
                continue;
            }
            let tail_start = index + known_chars.len();
            let tail = chars[tail_start..]
                .iter()
                .take_while(|ch| surface_gate::is_cjk_unified(**ch))
                .take(3)
                .collect::<String>();
            if tail.chars().count() >= 2
                && character_gate::role_reference_candidate_looks_like_person(&tail)
            {
                return Some(format!("{known}{tail}"));
            }
        }
    }
    None
}

fn contract_power_progression_is_generic(contract: &NovelCreationContract) -> bool {
    let power = &contract.structured.power_progression;
    !value_missing(&power.system_name)
        && text_repeats_story_anchor(&power.system_name, contract)
        && power.levels.is_empty()
        && power.advancement_costs.is_empty()
        && power.bottlenecks.is_empty()
        && power.failure_consequences.is_empty()
        && power.anti_power_creep_rules.is_empty()
}

fn contract_resource_economy_is_generic(contract: &NovelCreationContract) -> bool {
    let resource = &contract.structured.resource_economy;
    !value_missing(&resource.value_scale)
        && text_repeats_story_anchor(&resource.value_scale, contract)
        && value_missing(&resource.currency)
        && resource.resource_types.is_empty()
        && resource.income_sources.is_empty()
        && resource.cost_examples.is_empty()
        && resource.scarcity_rules.is_empty()
        && resource.trade_rules.is_empty()
}

fn contract_social_order_is_generic(contract: &NovelCreationContract) -> bool {
    let order = &contract.structured.social_order;
    !value_missing(&order.rank_system)
        && text_repeats_story_anchor(&order.rank_system, contract)
        && order.institutions.is_empty()
        && order.exam_or_promotion_rules.is_empty()
        && order.laws.is_empty()
        && value_missing(&order.class_structure)
        && order.authority_conflicts.is_empty()
}

fn text_repeats_story_anchor(value: &str, contract: &NovelCreationContract) -> bool {
    let normalized = super::normalized_contract_text(value);
    !normalized.is_empty()
        && [
            contract.world_imagery.as_str(),
            contract.premise.as_str(),
            contract.main_causal_spine.as_str(),
        ]
        .iter()
        .filter(|anchor| !value_missing(anchor))
        .any(|anchor| normalized == super::normalized_contract_text(anchor))
}

fn field_strength(contract: &NovelCreationContract, key: &str) -> PatchFieldStrength {
    contract
        .structured
        .field_requirements
        .get(key)
        .map(|value| PatchFieldStrength::from_policy_value(value))
        .or_else(|| {
            crate::tool::writing::longform_policy::fiction_contract_field_requirements(
                &contract.genre,
            )
            .get(key)
            .map(|value| PatchFieldStrength::from_policy_value(value))
        })
        .unwrap_or(PatchFieldStrength::Default)
}
