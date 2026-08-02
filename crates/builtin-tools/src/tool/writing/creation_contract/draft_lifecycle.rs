use super::*;

pub(super) const PENDING_EXPLICIT_CONTRACT_REVISION_PREFIX: &str = "待应用合同字段修订：";
const PENDING_EXPLICIT_CONTRACT_REVISION_SCOPE_SEPARATOR: char = '|';
pub(crate) const LEGACY_FORBIDDEN_NAMING_PREFIX: &str = "失败合同禁用命名：";
pub(crate) const FORBIDDEN_TITLE_NAMING_PREFIX: &str = "失败合同禁用书名：";
pub(crate) const FORBIDDEN_CHARACTER_NAMING_PREFIX: &str = "失败合同禁用角色名：";
pub(crate) const CONTRACT_QUALITY_BLOCKER_DIAGNOSTIC_PREFIX: &str = "合同草案未通过质量门：";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ForbiddenNamingAuthority {
    pub(crate) titles: Vec<String>,
    pub(crate) character_names: Vec<String>,
}

impl ForbiddenNamingAuthority {
    fn normalize(&mut self) {
        self.titles.sort();
        self.titles.dedup();
        self.character_names.sort();
        self.character_names.dedup();
    }

    fn is_empty(&self) -> bool {
        self.titles.is_empty() && self.character_names.is_empty()
    }
}

fn initial_structured_contract(genre: &str) -> NovelContractV2 {
    let mut contract = NovelContractV2 {
        field_requirements: longform_policy::fiction_contract_field_requirements(genre),
        ..Default::default()
    };
    contract.emotional_contract.relief_beats =
        vec![longform_policy::fiction_relief_beat_guidance(genre)];
    contract.normalize();
    contract
}

pub fn build_initial_creation_draft(
    session_id: &str,
    artifact_kind: &str,
    message: &str,
) -> Option<SessionCreationDraftState> {
    if !matches!(artifact_kind, "fiction" | "paper" | "report") {
        return None;
    }
    let now = chrono::Utc::now().to_rfc3339();
    let language = resolve_language_contract(message).artifact_language;
    let target_units = requested_total_unit_target(message);
    let raw_chapter_unit_target = requested_raw_chapter_unit_target(message);
    let chapter_unit_target =
        raw_chapter_unit_target.and_then(longform_policy::exact_novel_chapter_unit_band);
    let section_unit_target = requested_section_unit_target(message);
    let export_format = requested_export_format(message).unwrap_or_else(|| "txt".to_string());
    let title = requested_title(message).unwrap_or_default();
    let brief = creation_brief(message, artifact_kind);
    let genre = if artifact_kind == "fiction" {
        infer_fiction_genre(message).unwrap_or_default()
    } else {
        String::new()
    };
    let contract_v2 = initial_structured_contract(&genre);
    let mut planning_notes = merge_list(
        &creation_planning_notes(message, artifact_kind),
        &explicit_user_character_name_notes(message),
    );
    if let Some(scope_note) = creation_execution_scope_note(message, artifact_kind) {
        planning_notes = merge_list(&planning_notes, &[scope_note]);
    }
    if artifact_kind == "fiction" && !brief.trim().is_empty() {
        planning_notes = merge_list(
            &planning_notes,
            &[format!("用户故事核心权威：{}", brief.trim())],
        );
    }
    if !title.trim().is_empty() {
        planning_notes = merge_list(
            &planning_notes,
            &[format!("书名权威（用户）：{}", title.trim())],
        );
    }
    let mut draft = SessionCreationDraftState {
        schema_version: "benshu.writing.creation_draft.v1".to_string(),
        session_id: session_id.to_string(),
        artifact_kind: artifact_kind.to_string(),
        tool_name: if artifact_kind == "fiction" {
            "novel_studio".to_string()
        } else {
            "writing_studio".to_string()
        },
        draft_path: String::new(),
        project_path: String::new(),
        title,
        language,
        genre,
        brief,
        document_type: match artifact_kind {
            "paper" => "paper",
            "report" => "report",
            _ => "",
        }
        .to_string(),
        audience: requested_after_marker(message, &["面向", "读者是", "audience"])
            .unwrap_or_default(),
        purpose: requested_after_marker(message, &["用途是", "目的", "purpose"])
            .unwrap_or_default(),
        thesis_or_premise: requested_after_marker(
            message,
            &["主题是", "关于", "研究问题", "topic"],
        )
        .unwrap_or_default(),
        target_units,
        target_units_user_specified: target_units.is_some(),
        chapter_unit_target,
        chapter_unit_target_user_specified: chapter_unit_target.is_some(),
        chapter_unit_target_user_authority: chapter_unit_target,
        section_unit_target,
        max_chapters_per_turn: requested_max_chapters_per_turn(message),
        export_format,
        export_when_complete: true,
        approved_only: true,
        required_structure: requested_structure_items(message),
        evidence_rules: requested_evidence_rules(message),
        style_rules: requested_style_rules(message),
        planning_notes,
        diagnostics: Vec::new(),
        current_contract: None,
        pending_contract_candidate: None,
        fiction_premise: String::new(),
        fiction_themes: Vec::new(),
        fiction_characters: Vec::new(),
        fiction_world_rules: Vec::new(),
        fiction_style_rules: Vec::new(),
        fiction_must_avoid: Vec::new(),
        fiction_outline: String::new(),
        fiction_ending_direction: String::new(),
        fiction_protagonist_arc: String::new(),
        fiction_world_imagery: String::new(),
        fiction_main_causal_spine: String::new(),
        fiction_title_rationale: String::new(),
        field_requirements: contract_v2.field_requirements,
        structured_contract_schema_version: contract_v2.schema_version,
        structured_contract_revision: contract_v2.revision,
        resource_economy: contract_v2.resource_economy,
        emotional_contract: contract_v2.emotional_contract,
        emotional_state_ledger: contract_v2.emotional_state_ledger,
        relationship_ledger: contract_v2.relationship_ledger,
        power_progression: contract_v2.power_progression,
        social_order: contract_v2.social_order,
        geography_model: contract_v2.geography_model,
        time_model: contract_v2.time_model,
        artifact_ledger: contract_v2.artifact_ledger,
        antagonist_pressure: contract_v2.antagonist_pressure,
        payoff_matrix: contract_v2.payoff_matrix,
        narration_contract: contract_v2.narration_contract,
        scene_type_mix: contract_v2.scene_type_mix,
        character_voice_ledger: contract_v2.character_voice_ledger,
        reader_promise: contract_v2.reader_promise,
        chapter_ending_rotation: contract_v2.chapter_ending_rotation,
        conflict_pressure_curve: contract_v2.conflict_pressure_curve,
        motif_ledger: contract_v2.motif_ledger,
        reveal_schedule: contract_v2.reveal_schedule,
        relationship_interaction_quotas: contract_v2.relationship_interaction_quotas,
        created_at: now.clone(),
        updated_at: now,
        status: CreationDraftLifecycleStatus::DraftingContract
            .as_str()
            .to_string(),
    };
    sanitize_creation_draft_control_noise(&mut draft);
    Some(draft)
}

pub fn apply_message_to_creation_draft(draft: &mut SessionCreationDraftState, message: &str) {
    sanitize_creation_draft_control_noise(draft);
    if creation_draft_execution_requested(message, &draft.artifact_kind) {
        if let Some(scope_note) = creation_execution_scope_note(message, &draft.artifact_kind) {
            draft
                .planning_notes
                .retain(|note| !note.starts_with(CREATION_EXECUTION_SCOPE_NOTE_PREFIX));
            draft.planning_notes.push(scope_note);
        }
    }
    let had_locked_contract = draft.current_contract.is_some();
    let repair_only_message = creation_contract_repair_only_message(message);
    let language = resolve_language_contract(message).artifact_language;
    if !language.trim().is_empty() {
        draft.language = language;
    }
    let requested_title_value = requested_title(message);
    let generated_title_revision = creation_draft_requests_generated_title_revision(message);
    let replace_fiction_concept = draft.artifact_kind == "fiction"
        && !generated_title_revision
        && fiction_concept_replacement_requested(message);
    let fiction_content_message = if replace_fiction_concept {
        fiction_concept_replacement_payload(message).unwrap_or(message)
    } else {
        message
    };
    let mut forbidden = explicitly_rejected_naming_authority_from_message(message);
    if generated_title_revision {
        if !draft.title.trim().is_empty() {
            forbidden.titles.push(draft.title.trim().to_string());
        }
    }
    forbidden.normalize();
    if let Some(title) = requested_title_value.as_ref() {
        draft.title = title.clone();
        draft.planning_notes = merge_list(
            &draft.planning_notes,
            &[format!("书名权威（用户）：{}", title.trim())],
        );
    }
    if let Some(target) = requested_total_unit_target(message) {
        draft.target_units = Some(target);
        draft.target_units_user_specified = true;
    }
    let raw_chapter_unit_target = requested_raw_chapter_unit_target(message);
    if let Some(target) =
        raw_chapter_unit_target.and_then(longform_policy::exact_novel_chapter_unit_band)
    {
        draft.chapter_unit_target = Some(target);
        draft.chapter_unit_target_user_specified = true;
        draft.chapter_unit_target_user_authority = Some(target);
    }
    if let Some(target) = requested_section_unit_target(message) {
        draft.section_unit_target = Some(target);
    }
    if let Some(count) = requested_max_chapters_per_turn(message) {
        draft.max_chapters_per_turn = Some(count);
    }
    if let Some(format) = requested_export_format(message) {
        draft.export_format = format;
    }
    if draft.artifact_kind == "fiction" {
        let inferred_genre = infer_fiction_genre(fiction_content_message)
            .or_else(|| infer_followup_fiction_genre(fiction_content_message));
        if replace_fiction_concept {
            draft.genre = inferred_genre.unwrap_or_default();
            if requested_title_value.is_none() {
                draft.title.clear();
            }
            clear_fiction_contract_fields(draft);
        } else if let Some(genre) = inferred_genre {
            if !generated_title_revision {
                draft.genre = merge_short_field(&draft.genre, &genre);
            }
        }
        let brief = creation_brief(fiction_content_message, "fiction");
        if !brief.is_empty()
            && !creation_draft_approval_requested(message)
            && !repair_only_message
            && !generated_title_revision
        {
            if replace_fiction_concept {
                draft.brief = brief;
            } else {
                draft.brief = merge_short_field(&draft.brief, &brief);
            }
        }
        let planning_notes = creation_planning_notes(fiction_content_message, "fiction");
        if !repair_only_message && !generated_title_revision {
            draft.planning_notes = merge_list(&draft.planning_notes, &planning_notes);
            if !replace_fiction_concept {
                let user_story_authorities = planning_notes
                    .iter()
                    .filter(|note| {
                        !forbidden
                            .character_names
                            .iter()
                            .any(|name| note.contains(name))
                    })
                    .map(|note| format!("用户故事核心权威：{note}"))
                    .collect::<Vec<_>>();
                draft.planning_notes = merge_list(&draft.planning_notes, &user_story_authorities);
            }
            if had_locked_contract
                && creation_draft_modification_requested(message)
                && !replace_fiction_concept
            {
                let patch_type = explicit_contract_revision_patch_type(message);
                let pending_revisions = planning_notes
                    .iter()
                    .map(|note| pending_explicit_contract_revision_note(patch_type, note))
                    .collect::<Vec<_>>();
                draft.planning_notes = merge_list(&draft.planning_notes, &pending_revisions);
            }
        }
        if replace_fiction_concept && !draft.brief.trim().is_empty() {
            draft.planning_notes = merge_list(
                &draft.planning_notes,
                &[format!("用户故事核心权威：{}", draft.brief.trim())],
            );
        }
        draft.planning_notes = merge_list(
            &draft.planning_notes,
            &explicit_user_character_name_notes(message),
        );
        record_forbidden_naming_authority(draft, &forbidden);
    } else {
        if let Some(topic) =
            requested_after_marker(message, &["主题是", "关于", "研究问题", "topic"])
        {
            draft.thesis_or_premise = merge_short_field(&draft.thesis_or_premise, &topic);
        }
        if let Some(audience) = requested_after_marker(message, &["面向", "读者是", "audience"])
        {
            draft.audience = merge_short_field(&draft.audience, &audience);
        }
        if let Some(purpose) = requested_after_marker(message, &["用途是", "目的", "purpose"])
        {
            draft.purpose = merge_short_field(&draft.purpose, &purpose);
        }
        let structure = requested_structure_items(message);
        if !structure.is_empty() {
            draft.required_structure = merge_list(&draft.required_structure, &structure);
        }
        let evidence_rules = requested_evidence_rules(message);
        if !evidence_rules.is_empty() {
            draft.evidence_rules = merge_list(&draft.evidence_rules, &evidence_rules);
        }
        let style_rules = requested_style_rules(message);
        if !style_rules.is_empty() {
            draft.style_rules = merge_list(&draft.style_rules, &style_rules);
        }
        let brief = creation_brief(message, &draft.artifact_kind);
        if !brief.is_empty() && !creation_draft_approval_requested(message) && !repair_only_message
        {
            draft.brief = merge_short_field(&draft.brief, &brief);
        }
        if !repair_only_message {
            draft.planning_notes = merge_list(
                &draft.planning_notes,
                &creation_planning_notes(message, &draft.artifact_kind),
            );
        }
    }
    sanitize_creation_draft_control_noise(draft);
    if !draft.is_approved() {
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
    }
}

pub(crate) fn record_contract_quality_blocker_diagnostic(
    draft: &mut SessionCreationDraftState,
    issues: &[String],
) {
    clear_contract_quality_blocker_diagnostic(draft);
    if !issues.is_empty() {
        draft.diagnostics.push(format!(
            "{CONTRACT_QUALITY_BLOCKER_DIAGNOSTIC_PREFIX}{}",
            issues.join("；")
        ));
    }
}

pub(crate) fn clear_contract_quality_blocker_diagnostic(draft: &mut SessionCreationDraftState) {
    draft
        .diagnostics
        .retain(|item| !item.starts_with(CONTRACT_QUALITY_BLOCKER_DIAGNOSTIC_PREFIX));
}

#[cfg(test)]
pub(crate) fn pending_explicit_contract_revision_issue(
    draft: &SessionCreationDraftState,
) -> Option<String> {
    let findings = pending_explicit_contract_revision_findings(draft);
    (!findings.is_empty()).then(|| findings.join("；"))
}

pub(crate) fn pending_explicit_contract_revision_findings(
    draft: &SessionCreationDraftState,
) -> super::issue::ContractIssueList {
    let mut findings = super::issue::ContractIssueList::default();
    for (patch_type, revision) in draft
        .planning_notes
        .iter()
        .filter_map(|note| note.strip_prefix(PENDING_EXPLICIT_CONTRACT_REVISION_PREFIX))
        .filter_map(parse_pending_explicit_contract_revision)
    {
        let (kind, field) = match patch_type {
            CreationContractPatchType::Title => {
                (super::issue::ContractIssueKind::Skeleton, "title")
            }
            CreationContractPatchType::Skeleton | CreationContractPatchType::Metadata => {
                (super::issue::ContractIssueKind::Skeleton, "story_authority")
            }
            CreationContractPatchType::Characters => {
                (super::issue::ContractIssueKind::Characters, "characters")
            }
            CreationContractPatchType::Plot => (super::issue::ContractIssueKind::Plot, "outline"),
            CreationContractPatchType::Governance => {
                (super::issue::ContractIssueKind::Governance, "governance")
            }
        };
        findings.push_issue(super::issue::ContractIssue::new(
            "contract.explicit_revision",
            kind,
            super::issue::ContractIssueDisposition::Repairable,
            super::issue::ContractIssueEvidence::new(field, revision),
            format!(
                "ContractBlocker[contract.explicit_revision]: 用户明确合同修订尚未经过对应 typed patch 实际写入：{revision}"
            ),
        ));
    }
    findings.sort_dedup();
    findings
}

pub(crate) fn clear_applied_explicit_contract_revisions(
    draft: &mut SessionCreationDraftState,
    patch_type: CreationContractPatchType,
) {
    draft.planning_notes.retain(|note| {
        let Some(revision) = note.strip_prefix(PENDING_EXPLICIT_CONTRACT_REVISION_PREFIX) else {
            return true;
        };
        let Some((revision_patch_type, _)) = parse_pending_explicit_contract_revision(revision)
        else {
            return true;
        };
        revision_patch_type != patch_type
    });
}

pub(super) fn pending_explicit_contract_revision_note(
    patch_type: CreationContractPatchType,
    revision: &str,
) -> String {
    format!(
        "{PENDING_EXPLICIT_CONTRACT_REVISION_PREFIX}{}{PENDING_EXPLICIT_CONTRACT_REVISION_SCOPE_SEPARATOR}{}",
        contract_patch_scope_label(patch_type),
        revision.trim()
    )
}

pub(super) fn parse_pending_explicit_contract_revision(
    revision: &str,
) -> Option<(CreationContractPatchType, &str)> {
    let revision = revision.trim();
    if let Some((scope, text)) =
        revision.split_once(PENDING_EXPLICIT_CONTRACT_REVISION_SCOPE_SEPARATOR)
    {
        let patch_type = match scope.trim() {
            "skeleton" => CreationContractPatchType::Skeleton,
            "characters" => CreationContractPatchType::Characters,
            "plot" => CreationContractPatchType::Plot,
            "governance" => CreationContractPatchType::Governance,
            "title" => CreationContractPatchType::Title,
            "metadata" => CreationContractPatchType::Metadata,
            _ => return None,
        };
        let text = text.trim();
        return (!text.is_empty()).then_some((patch_type, text));
    }
    // Legacy sessions stored only the revision text. Those entries originated
    // from planning notes, whose canonical owner is the story skeleton. Read
    // them once as Skeleton; new writes always persist an explicit scope.
    (!revision.is_empty()).then_some((CreationContractPatchType::Skeleton, revision))
}

fn contract_patch_scope_label(patch_type: CreationContractPatchType) -> &'static str {
    match patch_type {
        CreationContractPatchType::Title => "title",
        CreationContractPatchType::Skeleton => "skeleton",
        CreationContractPatchType::Characters => "characters",
        CreationContractPatchType::Plot => "plot",
        CreationContractPatchType::Governance => "governance",
        CreationContractPatchType::Metadata => "metadata",
    }
}

fn explicit_contract_revision_patch_type(message: &str) -> CreationContractPatchType {
    let lower = message.to_ascii_lowercase();
    if crate::tool::writing::session_route::extract_requested_chapter_number_from_text(message)
        .is_some()
        || message_contains_indexed_volume_target(message)
        || [
            "大纲", "分卷", "章节", "伏笔", "outline", "chapter", "volume", "payoff",
        ]
        .iter()
        .any(|marker| message.contains(marker) || lower.contains(marker))
    {
        return CreationContractPatchType::Plot;
    }
    if [
        "角色",
        "人物",
        "主角",
        "对手",
        "姓名",
        "欲望",
        "恐惧",
        "底线",
        "character",
        "protagonist",
    ]
    .iter()
    .any(|marker| message.contains(marker) || lower.contains(marker))
    {
        return CreationContractPatchType::Characters;
    }
    if [
        "世界规则",
        "叙事风格",
        "必须避免",
        "主题",
        "关系线",
        "情感线",
        "world_rules",
        "style_rules",
        "must_avoid",
        "relationship",
        "emotional",
    ]
    .iter()
    .any(|marker| message.contains(marker) || lower.contains(marker))
    {
        return CreationContractPatchType::Governance;
    }
    CreationContractPatchType::Skeleton
}

fn message_contains_indexed_volume_target(message: &str) -> bool {
    let ordinal = |ch: char| {
        ch.is_ascii_digit()
            || matches!(
                ch,
                '零' | '〇'
                    | '一'
                    | '二'
                    | '三'
                    | '四'
                    | '五'
                    | '六'
                    | '七'
                    | '八'
                    | '九'
                    | '十'
                    | '百'
                    | '千'
                    | '两'
            )
    };
    let chars = message.chars().collect::<Vec<_>>();
    chars.iter().enumerate().any(|(start, ch)| {
        if *ch != '第' {
            return false;
        }
        let tail = &chars[start + 1..];
        let Some(end) = tail.iter().take(8).position(|ch| *ch == '卷') else {
            return false;
        };
        end > 0 && tail[..end].iter().all(|ch| ordinal(*ch))
    })
}

fn explicitly_rejected_naming_authority_from_message(message: &str) -> ForbiddenNamingAuthority {
    let mut authority = ForbiddenNamingAuthority::default();
    let names = explicitly_rejected_names_from_message(message);
    let title_context = ["书名", "标题", "小说名", "作品名"]
        .iter()
        .any(|marker| message.contains(marker));
    let character_context = [
        "角色名",
        "人物名",
        "角色",
        "人物",
        "姓名",
        "主角",
        "男主",
        "女主",
        "对手",
        "反派",
        "同伴",
        "导师",
    ]
    .iter()
    .any(|marker| message.contains(marker));
    for name in names {
        if title_context && !character_context {
            authority.titles.push(name);
        } else {
            authority.character_names.push(name);
        }
    }
    authority.normalize();
    authority
}

pub(crate) fn explicitly_rejected_names_from_message(message: &str) -> Vec<String> {
    let markers = [
        "不要叫",
        "别叫",
        "不叫",
        "不要用",
        "别用",
        "不要复用",
        "不再使用",
        "禁用",
    ];
    let mut names = Vec::new();
    for marker in markers {
        let Some((_, tail)) = message.split_once(marker) else {
            continue;
        };
        let mut candidate = String::new();
        for ch in tail.chars() {
            if ch.is_whitespace()
                || matches!(
                    ch,
                    '，' | ','
                        | '。'
                        | '.'
                        | '；'
                        | ';'
                        | '：'
                        | ':'
                        | '、'
                        | '！'
                        | '!'
                        | '？'
                        | '?'
                        | '（'
                        | '('
                        | '）'
                        | ')'
                )
            {
                break;
            }
            candidate.push(ch);
            if candidate.chars().count() >= 8 {
                break;
            }
        }
        let candidate = candidate.trim_matches(['《', '》', '"', '\'', '“', '”', '‘', '’']);
        let describes_name_category = [
            "书名", "标题", "角色", "人物", "名字", "姓名", "命名", "旧名", "测试", "任何", "此前",
        ]
        .iter()
        .any(|noise| candidate.contains(noise));
        if candidate.chars().count() >= 2 && !describes_name_category && !value_missing(candidate) {
            names.push(candidate.to_string());
        }
    }
    let rejection_context = [
        "不要",
        "别",
        "不再",
        "禁用",
        "拒绝",
        "复用",
        "旧名",
        "旧名字",
        "更换",
        "替换",
        "没有生效",
        "仍然是",
        "仍复用",
    ]
    .iter()
    .any(|marker| message.contains(marker));
    if rejection_context {
        collect_quoted_rejected_names(message, &mut names);
    }
    names.retain(|name| !line_is_bare_contract_section_heading(name));
    names.sort();
    names.dedup();
    names
}

fn collect_quoted_rejected_names(message: &str, names: &mut Vec<String>) {
    for (open, close) in [
        ('“', '”'),
        ('‘', '’'),
        ('「', '」'),
        ('『', '』'),
        ('《', '》'),
        ('"', '"'),
        ('\'', '\''),
    ] {
        let mut rest = message;
        while let Some(start) = rest.find(open) {
            let after = &rest[start + open.len_utf8()..];
            let Some(end) = after.find(close) else {
                break;
            };
            let candidate = after[..end].trim();
            let len = candidate.chars().count();
            if (2..=8).contains(&len)
                && candidate.chars().all(|ch| {
                    ('\u{4e00}'..='\u{9fff}').contains(&ch)
                        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
                        || ch == '·'
                })
                && !value_missing(candidate)
            {
                names.push(candidate.to_string());
            }
            rest = &after[end + close.len_utf8()..];
        }
    }
}

fn record_forbidden_naming_authority(
    draft: &mut SessionCreationDraftState,
    authority: &ForbiddenNamingAuthority,
) {
    if authority.is_empty() {
        return;
    }
    let mut notes = Vec::new();
    if !authority.titles.is_empty() {
        notes.push(format!(
            "{FORBIDDEN_TITLE_NAMING_PREFIX}{}",
            authority.titles.join("、")
        ));
    }
    if !authority.character_names.is_empty() {
        notes.push(format!(
            "{FORBIDDEN_CHARACTER_NAMING_PREFIX}{}",
            authority.character_names.join("、")
        ));
    }
    draft.planning_notes = merge_list(&draft.planning_notes, &notes);
}

pub(crate) fn forbidden_naming_authority(
    draft: &SessionCreationDraftState,
) -> ForbiddenNamingAuthority {
    let mut authority = ForbiddenNamingAuthority::default();
    for note in &draft.planning_notes {
        let (target, value) = if let Some(value) = note.strip_prefix(FORBIDDEN_TITLE_NAMING_PREFIX)
        {
            (&mut authority.titles, value)
        } else if let Some(value) = note.strip_prefix(FORBIDDEN_CHARACTER_NAMING_PREFIX) {
            (&mut authority.character_names, value)
        } else if let Some(value) = note.strip_prefix(LEGACY_FORBIDDEN_NAMING_PREFIX) {
            let mut legacy_values = value
                .split('、')
                .map(str::trim)
                .filter(|name| !value_missing(name));
            if let Some(previous_title) = legacy_values.next() {
                authority.titles.push(previous_title.to_string());
            }
            authority
                .character_names
                .extend(legacy_values.map(ToString::to_string));
            continue;
        } else {
            continue;
        };
        target.extend(
            value
                .split('、')
                .map(str::trim)
                .filter(|name| !value_missing(name))
                .map(ToString::to_string),
        );
    }
    authority.normalize();
    authority
}

pub(crate) fn creation_contract_repair_only_message(message: &str) -> bool {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    let contract_surface = [
        "合同",
        "草案",
        "创作蓝图",
        "质量门",
        "候选",
        "story contract",
        "contract",
        "draft",
    ]
    .iter()
    .any(|term| trimmed.contains(term) || lowered.contains(&term.to_ascii_lowercase()));
    let repair_action = [
        "重新生成",
        "重新输出",
        "重写",
        "修订",
        "修复",
        "修正",
        "纠正",
        "补齐",
        "补全",
        "自检",
        "检查",
        "复核",
        "处理",
        "regenerate",
        "repair",
        "fix",
        "review",
    ]
    .iter()
    .any(|term| trimmed.contains(term) || lowered.contains(&term.to_ascii_lowercase()));
    if !contract_surface || !repair_action {
        return false;
    }
    !message_contains_creation_contract_content_update_surface(trimmed, &lowered)
}

pub(crate) fn fiction_concept_replacement_requested(message: &str) -> bool {
    if text_has_any(
        message,
        &[
            "不要这个",
            "推倒重来",
            "从头重写",
            "从零重写",
            "全新题材",
            "全新故事",
            "全新世界观",
        ],
    ) {
        return true;
    }

    if fiction_replacement_action_targets_whole_concept(message) {
        return true;
    }

    if text_has_any(
        message,
        &["不是这个题材", "不是这个故事", "题材错误", "故事错误"],
    ) {
        return true;
    }

    text_has_any(message, &["删除", "清除", "移除", "去掉"])
        && text_has_any(
            message,
            &["旧设定", "原设定", "无关设定", "旧合同", "原合同"],
        )
}

fn fiction_replacement_action_targets_whole_concept(message: &str) -> bool {
    let action_markers = [
        "改成",
        "换成",
        "改为",
        "换为",
        "重写成",
        "更正为",
        "更正成",
        "纠正为",
        "替换为",
        "重做为",
        "重新设定",
        "重新定",
    ];
    action_markers.iter().any(|marker| {
        message.match_indices(marker).any(|(index, _)| {
            let start = message[..index]
                .char_indices()
                .rev()
                .find(|(_, ch)| matches!(ch, '，' | ',' | '。' | '.' | '；' | ';' | '\n'))
                .map(|(position, ch)| position + ch.len_utf8())
                .unwrap_or(0);
            let marker_end = index + marker.len();
            let end = message[marker_end..]
                .find(|ch| matches!(ch, '，' | ',' | '。' | '.' | '；' | ';' | '\n'))
                .map(|position| marker_end + position)
                .unwrap_or(message.len());
            let clause = &message[start..end];
            let lowered = clause.to_ascii_lowercase();
            let explicitly_targets_concept = text_has_any(
                clause,
                &[
                    "题材",
                    "世界观",
                    "时代背景",
                    "故事背景",
                    "故事前提",
                    "故事核心",
                    "整个故事",
                    "整套故事",
                    "完整更正",
                ],
            ) || lowered.contains("story")
                || lowered.contains("premise")
                || lowered.contains("genre");
            if explicitly_targets_concept {
                return true;
            }
            if text_has_any(clause, &["书名", "标题", "作品名", "小说名"]) {
                return false;
            }
            let payload = &message[marker_end..end];
            infer_fiction_genre(payload)
                .or_else(|| infer_followup_fiction_genre(payload))
                .is_some()
        })
    })
}

pub(crate) fn fiction_concept_replacement_payload(message: &str) -> Option<&str> {
    for marker in [
        "完整更正为",
        "整体更正为",
        "全部更正为",
        "重新设定为",
        "重新定为",
        "从头重写为",
        "从零重写为",
        "更正为",
        "更正成",
        "纠正为",
        "替换为",
        "重写成",
        "重做为",
        "改成",
        "换成",
        "改为",
        "换为",
    ] {
        let Some((_, tail)) = message.split_once(marker) else {
            continue;
        };
        let tail = tail.trim_start_matches(|ch| matches!(ch, '：' | ':' | '，' | ',' | ' '));
        if !tail.is_empty() {
            return Some(tail);
        }
    }
    None
}

pub(crate) fn clear_fiction_contract_fields(draft: &mut SessionCreationDraftState) {
    draft.current_contract = None;
    draft.pending_contract_candidate = None;
    draft.fiction_premise.clear();
    draft.fiction_themes.clear();
    draft.fiction_characters.clear();
    draft.fiction_world_rules.clear();
    draft.fiction_style_rules.clear();
    draft.fiction_must_avoid.clear();
    draft.fiction_outline.clear();
    draft.fiction_ending_direction.clear();
    draft.fiction_protagonist_arc.clear();
    draft.fiction_world_imagery.clear();
    draft.fiction_main_causal_spine.clear();
    draft.fiction_title_rationale.clear();
    draft.planning_notes.clear();
    draft.diagnostics.clear();
    draft.set_contract_v2(initial_structured_contract(&draft.genre));
}

/// Return user-owned planning notes that must survive replacement of the
/// generated contract projection.  These notes are the existing authority
/// source consumed by the title/character governance path; they are not a
/// second authority store.
pub(crate) fn user_authority_planning_notes(draft: &SessionCreationDraftState) -> Vec<String> {
    const AUTHORITY_PREFIXES: &[&str] = &[
        "用户故事核心权威：",
        "书名权威（用户）：",
        "角色姓名权威（用户）：",
        "明确指定角色姓名：",
    ];
    draft
        .planning_notes
        .iter()
        .filter(|note| {
            AUTHORITY_PREFIXES
                .iter()
                .any(|prefix| note.starts_with(prefix))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_draft_preserves_compound_relationship_name_authority() {
        let draft = build_initial_creation_draft(
            "compound-relationship-name-authority",
            "fiction",
            "请从零创建玄幻小说《星墟回响》，总字数10万字，每章2500字，主角姓名为顾星河，关键关系对象剑修姓名为苏晚棠，对手姓名为谢无尘。",
        )
        .expect("draft");

        assert!(draft
            .planning_notes
            .iter()
            .any(|note| note == "明确指定角色姓名：苏晚棠"));
        assert!(draft
            .planning_notes
            .iter()
            .any(|note| note == "角色姓名权威（用户）：关键关系对象=苏晚棠"));
    }

    #[test]
    fn full_book_execution_scope_survives_short_confirmation_turn() {
        let mut draft = build_initial_creation_draft(
            "scope-survives-confirmation",
            "fiction",
            "写一部10万字小说，每章2500字，每次只写一章，确认后自动连续写完整本。",
        )
        .expect("draft");

        assert_eq!(
            persisted_creation_execution_scope(&draft.planning_notes),
            Some(CreationDraftTurnScope::AllRemaining)
        );
        apply_message_to_creation_draft(&mut draft, "确认合同，开始写作。");
        assert_eq!(
            persisted_creation_execution_scope(&draft.planning_notes),
            Some(CreationDraftTurnScope::AllRemaining)
        );
    }

    #[test]
    fn repair_request_with_preserved_contract_dimensions_does_not_reopen_story_authority() {
        let mut draft = build_initial_creation_draft(
            "repair-preserves-existing-authority",
            "fiction",
            "写一部都市言情小说，总字数10万字，每章2500字",
        )
        .expect("draft");
        draft.brief = "建筑师调查旧温室改造争议，并与画廊主重建信任。".to_string();
        draft.planning_notes = vec!["用户故事核心权威：旧温室改造争议".to_string()];
        draft.current_contract = Some(serde_json::json!({"title":{"canonical_title":"温室回声"}}));
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::ContractReady);
        let before_brief = draft.brief.clone();
        let before_notes = draft.planning_notes.clone();
        let before_contract = draft.current_contract.clone();

        let message =
            "请自动重新检查并修复当前合同，保持都市言情、总字数10万和每章2500字不变，不要写正文。";
        assert!(creation_contract_repair_only_message(message));
        apply_message_to_creation_draft(&mut draft, message);

        assert_eq!(draft.brief, before_brief);
        assert_eq!(draft.planning_notes, before_notes);
        assert_eq!(draft.current_contract, before_contract);
        assert_eq!(
            draft.lifecycle_status(),
            CreationDraftLifecycleStatus::DraftingContract
        );
        assert!(pending_explicit_contract_revision_issue(&draft).is_none());
    }

    #[test]
    fn repair_request_with_an_actual_genre_change_is_not_repair_only() {
        assert!(!creation_contract_repair_only_message(
            "修复合同，并把题材改成科幻"
        ));
    }

    #[test]
    fn generic_contract_repair_control_message_cannot_pollute_story_authority() {
        let mut draft = build_initial_creation_draft(
            "generic-repair-control",
            "fiction",
            "写一部修仙小说，总字数10万字，每章2500字",
        )
        .expect("draft");
        draft.brief = "剑修调查山门旧案。".to_string();
        draft.planning_notes = vec!["用户故事核心权威：山门旧案".to_string()];
        draft.current_contract = Some(serde_json::json!({"title":{"canonical_title":"旧剑山门"}}));
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::ContractReady);
        let before_brief = draft.brief.clone();
        let before_notes = draft.planning_notes.clone();
        let before_contract = draft.current_contract.clone();
        let message = "继续让合同自动修复器处理当前剩余缺口，保留当前最佳候选，不修改题材、总字数和章节档位；通过前不写正文。";

        assert!(creation_contract_repair_only_message(message));
        apply_message_to_creation_draft(&mut draft, message);

        assert_eq!(draft.brief, before_brief);
        assert_eq!(draft.planning_notes, before_notes);
        assert_eq!(draft.current_contract, before_contract);
        assert_eq!(
            draft.lifecycle_status(),
            CreationDraftLifecycleStatus::DraftingContract
        );
    }

    #[test]
    fn payoff_integrity_repair_instruction_cannot_pollute_story_authority() {
        let mut draft = build_initial_creation_draft(
            "payoff-integrity-repair-control",
            "fiction",
            "写一部校园悬疑小说，总字数10万字，每章2500字",
        )
        .expect("draft");
        draft.title = "星澜声纹".to_string();
        draft.brief = "失语学生调查图书馆被篡改的声音档案。".to_string();
        draft.fiction_title_rationale =
            "《星澜声纹》对应学院声纹档案与终局恢复真实声音。".to_string();
        draft.current_contract = Some(serde_json::json!({"title":{"canonical_title":"星澜声纹"}}));
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::ContractReady);
        let before_title = draft.title.clone();
        let before_rationale = draft.fiction_title_rationale.clone();
        let before_brief = draft.brief.clone();
        let before_contract = draft.current_contract.clone();
        let message = "请继续修复当前合同的完整性：保持书名《星澜声纹》、角色、主线、世界规则、总字数和章节档位不变；每条伏笔兑现记录都必须有非空承诺、具体兑现目标和生命周期状态。通过质量门后给我可确认合同，不要写正文。";

        assert!(creation_planning_note_is_quality_feedback(message));
        assert!(creation_contract_repair_only_message(message));
        apply_message_to_creation_draft(&mut draft, message);

        assert_eq!(draft.title, before_title);
        assert_eq!(draft.fiction_title_rationale, before_rationale);
        assert_eq!(draft.brief, before_brief);
        assert_eq!(draft.current_contract, before_contract);
        assert!(pending_explicit_contract_revision_issue(&draft).is_none());
    }

    #[test]
    fn semantic_contract_review_request_cannot_become_story_content() {
        let mut draft = build_initial_creation_draft(
            "semantic-review-control",
            "fiction",
            "写一部赛博朋克小说，总字数10万字，每章2500字",
        )
        .expect("draft");
        draft.brief = "清道夫发现主脑正在清除底层居民的记忆。".to_string();
        draft.planning_notes = vec!["用户故事核心权威：清道夫必须阻止记忆清除".to_string()];
        draft.current_contract =
            Some(serde_json::json!({"title":{"canonical_title":"格式化黎明"}}));
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::ContractReady);
        let before_brief = draft.brief.clone();
        let before_notes = draft.planning_notes.clone();
        let before_contract = draft.current_contract.clone();
        let message = "请继续使用现有自动流程进行合同复核和修复。重点核对所有具名机制在故事前提、世界规则、主线因果、终局方向和兑现矩阵中的既定作用是否一致；如果最终用途与既定效果不同，合同必须明确可验证的改写、反转、重定向或代价因果，修复完整后再给我可确认合同，不要写正文。";

        assert!(creation_planning_note_is_quality_feedback(message));
        assert!(creation_contract_repair_only_message(message));
        apply_message_to_creation_draft(&mut draft, message);

        assert_eq!(draft.brief, before_brief);
        assert_eq!(draft.planning_notes, before_notes);
        assert_eq!(draft.current_contract, before_contract);
        assert!(pending_explicit_contract_revision_issue(&draft).is_none());
    }

    #[test]
    fn automated_terminal_repair_instruction_cannot_become_story_authority() {
        let mut draft = build_initial_creation_draft(
            "terminal-repair-control",
            "fiction",
            "写一部都市言情小说，总字数10万字，每章2500字",
        )
        .expect("draft");
        draft.brief = "建筑师与审计师共同揭开旧城改造黑幕。".to_string();
        draft.planning_notes = vec!["用户故事核心权威：两人共同保住旧街区".to_string()];
        draft.current_contract = Some(serde_json::json!({"title":{"canonical_title":"旧街有晴"}}));
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::ContractReady);
        let before_brief = draft.brief.clone();
        let before_notes = draft.planning_notes.clone();
        let before_contract = draft.current_contract.clone();
        let message = "继续使用现有自动流程修复当前合同，保持已有都市言情故事方向与角色设定不变。请让实际末卷完整执行权威终局的核心行动、结果和不可逆关系变化，并同步分卷、近期章节与伏笔兑现；通过全部质量门后只给我可确认合同，不要写正文。";

        assert!(creation_planning_note_is_quality_feedback(message));
        assert!(creation_contract_repair_only_message(message));
        apply_message_to_creation_draft(&mut draft, message);

        assert_eq!(draft.brief, before_brief);
        assert_eq!(draft.planning_notes, before_notes);
        assert_eq!(draft.current_contract, before_contract);
        assert!(pending_explicit_contract_revision_issue(&draft).is_none());
    }

    #[test]
    fn explicit_contract_value_change_is_not_misclassified_as_quality_feedback() {
        let message = "复核当前合同，并把终局改为主角销毁主脑后带妹妹离城。";

        assert!(!creation_contract_repair_only_message(message));
        assert!(message_contains_creation_contract_content_update_surface(
            message,
            &message.to_ascii_lowercase()
        ));
    }

    #[test]
    fn indexed_volume_revision_is_owned_by_plot_even_when_roles_are_mentioned() {
        assert_eq!(
            explicit_contract_revision_patch_type(
                "修改第2卷，删除角色成为自己表妹的自指关系，保留现有角色权威"
            ),
            CreationContractPatchType::Plot
        );
        assert_eq!(
            explicit_contract_revision_patch_type("重写第二卷卷尾变化，不要改人物姓名"),
            CreationContractPatchType::Plot
        );
    }

    #[test]
    fn rejected_quoted_character_name_uses_existing_forbidden_naming_authority() {
        let message = "合同第二章仍复用了此前测试中的旧名“林默”，请自动更换并同步所有故事字段";

        assert_eq!(
            explicitly_rejected_names_from_message(message),
            vec!["林默".to_string()]
        );

        let mut draft = build_initial_creation_draft(
            "session-rejected-character-name",
            "fiction",
            "写一部都市小说，每章2500字，共10万字",
        )
        .expect("draft");
        apply_message_to_creation_draft(&mut draft, message);

        assert_eq!(
            forbidden_naming_authority(&draft).character_names,
            vec!["林默"]
        );
        assert!(stable_creation_planning_notes(&draft)
            .iter()
            .all(|note| !note.contains("林默")));
    }

    #[test]
    fn generic_do_not_reuse_request_does_not_invent_a_forbidden_name() {
        assert!(
            explicitly_rejected_names_from_message("不要复用此前测试的书名或角色名").is_empty()
        );
    }

    #[test]
    fn rejected_contract_section_headings_are_not_recorded_as_character_names() {
        assert!(explicitly_rejected_names_from_message(
            "合同字段不能混入“分卷规划”“近期章节包”等栏目标题"
        )
        .is_empty());
    }

    #[test]
    fn concept_replacement_preserves_newly_rejected_name_authority() {
        let mut draft = build_initial_creation_draft(
            "session-replace-concept-and-name",
            "fiction",
            "写一部都市小说，每章2500字，共10万字",
        )
        .expect("draft");

        apply_message_to_creation_draft(&mut draft, "把整个故事改成修仙题材，主角不要叫林默。");

        assert_eq!(draft.genre, "修仙");
        assert_eq!(
            forbidden_naming_authority(&draft).character_names,
            vec!["林默".to_string()]
        );
    }

    #[test]
    fn legacy_combined_naming_note_migrates_title_and_character_scopes() {
        let mut draft = build_initial_creation_draft(
            "session-legacy-forbidden-naming",
            "fiction",
            "写一部都市小说，每章2500字，共10万字",
        )
        .expect("draft");
        draft
            .planning_notes
            .push("失败合同禁用命名：旧城余烬、林默、陆离".to_string());

        let authority = forbidden_naming_authority(&draft);

        assert_eq!(authority.titles, vec!["旧城余烬".to_string()]);
        assert_eq!(
            authority.character_names,
            vec!["林默".to_string(), "陆离".to_string()]
        );
    }
}
