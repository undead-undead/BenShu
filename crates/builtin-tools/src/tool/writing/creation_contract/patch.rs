use super::*;
#[cfg(test)]
use crate::tool::writing::novel_contract_v2::{
    AgeProgressionState, CharacterProgressionState, LocationRecord, RelationshipTransition,
};
use crate::tool::writing::novel_contract_v2::{
    ArtifactLedgerEntry, GeographyModel, PowerProgression, ResourceEconomy, SocialOrder, TimeModel,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreationContractPatchType {
    Title,
    Skeleton,
    Characters,
    Plot,
    Governance,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatchFieldStrength {
    Required,
    Strong,
    Default,
    Optional,
    Disabled,
}

impl PatchFieldStrength {
    pub(crate) fn from_policy_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "required" | "genre_required" => Self::Required,
            "strong" | "genre_strong" => Self::Strong,
            "optional" | "genre_optional" => Self::Optional,
            "disabled" | "genre_disabled" => Self::Disabled,
            _ => Self::Default,
        }
    }

    pub(crate) fn as_prompt_label(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Strong => "strong",
            Self::Default => "default",
            Self::Optional => "optional",
            Self::Disabled => "disabled",
        }
    }

    pub(crate) fn blocks_for_scope(self, scope: ContractReadinessScope, field_key: &str) -> bool {
        match scope {
            ContractReadinessScope::DisplayContract => {
                let _ = field_key;
                matches!(self, Self::Required)
            }
            ContractReadinessScope::LockedAuthorityContract => {
                matches!(self, Self::Required) && !field_is_rolling_longform_enrichment(field_key)
            }
            #[cfg(test)]
            ContractReadinessScope::FullLongformContract => {
                matches!(self, Self::Required | Self::Strong)
            }
        }
    }
}

fn field_is_rolling_longform_enrichment(field_key: &str) -> bool {
    matches!(
        field_key,
        "scene_type_mix"
            | "character_voice_ledger"
            | "reader_promise"
            | "chapter_ending_rotation"
            | "conflict_pressure_curve"
            | "motif_ledger"
            | "reveal_schedule"
            | "relationship_interaction_quotas"
            | "payoff_matrix"
            | "emotional_contract"
            | "narration_contract"
            | "time_model"
    )
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PatchValidationReport {
    pub(crate) issues: Vec<String>,
}

impl PatchValidationReport {
    pub(crate) fn ready(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) enum CreationContractPatch {
    Title(TitlePatch),
    Skeleton(SkeletonPatch),
    Characters(CharacterPatch),
    Plot(PlotPatch),
    Governance(GovernancePatch),
    Metadata(MetadataPatch),
    Batch(Vec<CreationContractPatch>),
}

impl CreationContractPatch {
    pub(crate) fn is_multi_scope_batch(&self) -> bool {
        match self {
            Self::Batch(items) => {
                let mut scopes = items.iter().map(Self::patch_type).collect::<Vec<_>>();
                scopes.sort_by_key(|scope| *scope as u8);
                scopes.dedup();
                scopes.len() > 1
            }
            _ => false,
        }
    }

    pub(crate) fn patch_type(&self) -> CreationContractPatchType {
        match self {
            Self::Title(_) => CreationContractPatchType::Title,
            Self::Skeleton(_) => CreationContractPatchType::Skeleton,
            Self::Characters(_) => CreationContractPatchType::Characters,
            Self::Plot(_) => CreationContractPatchType::Plot,
            Self::Governance(_) => CreationContractPatchType::Governance,
            Self::Metadata(_) => CreationContractPatchType::Metadata,
            Self::Batch(items) => items
                .first()
                .map(Self::patch_type)
                .unwrap_or(CreationContractPatchType::Skeleton),
        }
    }

    pub(crate) fn validate_scope(
        &self,
        draft: &SessionCreationDraftState,
    ) -> PatchValidationReport {
        let mut issues = Vec::new();
        match self {
            Self::Batch(items) => {
                for item in items {
                    issues.extend(item.validate_scope(draft).issues);
                }
            }
            Self::Title(patch) => {
                if value_missing(&patch.canonical_title) {
                    issues.push("title_patch 缺少 canonical_title".to_string());
                }
                if value_missing(&patch.rationale)
                    && patch
                        .candidate_rationales
                        .values()
                        .all(|value| value_missing(value))
                {
                    issues.push("title_patch 缺少书名理由".to_string());
                }
            }
            Self::Skeleton(patch) => {
                if value_missing(&patch.premise)
                    && value_missing(&patch.ending_desired_resolution)
                    && value_missing(&patch.protagonist_arc)
                    && value_missing(&patch.world_imagery)
                    && value_missing(&patch.main_causal_spine)
                    && value_missing(&patch.genre)
                    && value_missing(&patch.brief)
                    && patch.target_units.is_none()
                    && patch.chapter_unit_target.is_none()
                    && patch.max_chapters_per_turn.is_none()
                {
                    issues.push("skeleton_patch 没有可合并的合同骨架字段".to_string());
                }
            }
            Self::Characters(patch) => {
                validate_character_patch_scope(patch, draft, &mut issues);
            }
            Self::Plot(patch) => {
                if patch.volumes.is_empty()
                    && patch.near_chapters.is_empty()
                    && value_missing(&patch.raw_outline)
                    && patch.payoff_matrix.is_empty()
                {
                    issues.push("plot_patch 没有可合并的分卷、章节目标或伏笔字段".to_string());
                }
                for chapter in &patch.near_chapters {
                    if value_missing(&chapter.goal) || value_missing(&chapter.expected_turn) {
                        issues.push("plot_patch 近期章节缺少具体事件目标或不可逆变化".to_string());
                    }
                }
                for volume in &patch.volumes {
                    if value_missing(&volume.title)
                        || value_missing(&volume.objective)
                        || value_missing(&volume.ending_change)
                    {
                        issues
                            .push("plot_patch 分卷缺少卷名、阶段目标或卷尾不可逆变化".to_string());
                    }
                }
            }
            Self::Governance(patch) => {
                if patch.themes.is_empty()
                    && patch.world_rules.is_empty()
                    && patch.style_rules.is_empty()
                    && patch.must_avoid.is_empty()
                    && patch.relationship_ledger.is_empty()
                    && patch.emotional_contract.primary_emotion.trim().is_empty()
                    && patch.antagonist_pressure.primary_pressure.trim().is_empty()
                    && patch.antagonist_pressure.antagonists.is_empty()
                {
                    issues.push("governance_patch 没有可合并的治理字段".to_string());
                }
            }
            Self::Metadata(patch) => {
                if value_missing(&patch.title.canonical_title)
                    && value_missing(&patch.title.rationale)
                    && patch.world_rules.is_empty()
                    && patch.near_chapters.is_empty()
                    && patch.volumes.is_empty()
                {
                    issues.push("metadata_patch 没有可合并的元数据字段".to_string());
                }
            }
        }
        issues.sort();
        issues.dedup();
        PatchValidationReport { issues }
    }

    pub(crate) fn apply_to_draft(&self, draft: &mut SessionCreationDraftState) {
        self.apply_to_draft_with_role_repair_policy(draft, false);
    }

    pub(crate) fn apply_to_draft_with_role_repair_policy(
        &self,
        draft: &mut SessionCreationDraftState,
        allow_character_role_authority_repair: bool,
    ) {
        self.apply_values_to_draft(draft, allow_character_role_authority_repair);
        draft.structured_contract_revision = draft.structured_contract_revision.saturating_add(1);
    }

    fn apply_values_to_draft(
        &self,
        draft: &mut SessionCreationDraftState,
        allow_character_role_authority_repair: bool,
    ) {
        match self {
            Self::Title(patch) => patch.apply_to_draft(draft),
            Self::Skeleton(patch) => patch.apply_to_draft(draft),
            Self::Characters(patch) => patch.apply_to_draft_with_role_repair_policy(
                draft,
                allow_character_role_authority_repair,
            ),
            Self::Plot(patch) => patch.apply_to_draft(draft),
            Self::Governance(patch) => patch.apply_to_draft(draft),
            Self::Metadata(patch) => patch.apply_to_draft(draft),
            Self::Batch(items) => {
                for item in items {
                    item.apply_values_to_draft(draft, allow_character_role_authority_repair);
                }
            }
        }
    }

    pub(crate) fn merge_applied_scope_into_contract(
        &self,
        base: &mut NovelCreationContract,
        applied: &NovelCreationContract,
    ) {
        self.merge_applied_scope_into_contract_with_role_repair_policy(base, applied, false);
    }

    pub(crate) fn merge_applied_scope_into_contract_with_role_repair_policy(
        &self,
        base: &mut NovelCreationContract,
        applied: &NovelCreationContract,
        allow_character_role_authority_repair: bool,
    ) {
        match self {
            Self::Title(_) => base.title = applied.title.clone(),
            Self::Skeleton(patch) => {
                base.genre = applied.genre.clone();
                base.brief = applied.brief.clone();
                base.target_units = applied.target_units;
                base.chapter_unit_target = applied.chapter_unit_target;
                base.max_chapters_per_turn = applied.max_chapters_per_turn;
                base.premise = applied.premise.clone();
                if !value_missing(&patch.ending_desired_resolution) {
                    base.ending.desired_resolution = applied.ending.desired_resolution.clone();
                } else if value_missing(&base.ending.desired_resolution) {
                    base.ending.desired_resolution = applied.ending.desired_resolution.clone();
                }
                if !value_missing(&patch.ending_final_state) {
                    base.ending.final_state = patch.ending_final_state.clone();
                }
                base.protagonist_arc = applied.protagonist_arc.clone();
                base.world_imagery = applied.world_imagery.clone();
                base.main_causal_spine = applied.main_causal_spine.clone();
            }
            Self::Characters(patch) => {
                let replacements =
                    character_patch_authority_replacements(&patch.characters, &applied.characters);
                rewrite_novel_contract_names(base, &replacements);
                if !patch.characters.is_empty() {
                    let volume_count = base.outline.volumes.len();
                    merge_character_patch_scope(
                        &mut base.characters,
                        &applied.characters,
                        volume_count,
                        allow_character_role_authority_repair,
                    );
                    record_superseded_character_names(&mut base.characters, &replacements);
                }
                if !patch.relationship_ledger.is_empty() {
                    base.structured.relationship_ledger =
                        applied.structured.relationship_ledger.clone();
                }
                if !patch.emotional_state_ledger.is_empty() {
                    base.structured.emotional_state_ledger =
                        applied.structured.emotional_state_ledger.clone();
                }
            }
            Self::Plot(patch) => {
                if !value_missing(&patch.raw_outline) {
                    base.outline.raw_outline = applied.outline.raw_outline.clone();
                }
                if !patch.volumes.is_empty() {
                    base.outline.volumes = applied.outline.volumes.clone();
                }
                if !patch.near_chapters.is_empty() {
                    base.outline.near_chapters = applied.outline.near_chapters.clone();
                }
                if !patch.payoff_matrix.is_empty() {
                    base.structured.payoff_matrix = applied.structured.payoff_matrix.clone();
                }
            }
            Self::Governance(patch) => {
                if !patch.themes.is_empty() {
                    base.themes = applied.themes.clone();
                }
                if !patch.world_rules.is_empty() {
                    base.world_rules = applied.world_rules.clone();
                }
                if !patch.style_rules.is_empty() {
                    base.style_rules = applied.style_rules.clone();
                }
                if !patch.must_avoid.is_empty() {
                    base.must_avoid = applied.must_avoid.clone();
                }
                merge_non_empty_contract_v2(&mut base.structured, &applied.structured);
            }
            Self::Metadata(patch) => {
                if !value_missing(&patch.title.canonical_title)
                    || !value_missing(&patch.title.rationale)
                {
                    base.title = applied.title.clone();
                }
                if !patch.world_rules.is_empty() {
                    base.world_rules = applied.world_rules.clone();
                }
                if !patch.volumes.is_empty() {
                    base.outline.volumes = applied.outline.volumes.clone();
                }
                if !patch.near_chapters.is_empty() {
                    base.outline.near_chapters = applied.outline.near_chapters.clone();
                }
            }
            Self::Batch(items) => {
                for item in items {
                    item.merge_applied_scope_into_contract_with_role_repair_policy(
                        base,
                        applied,
                        allow_character_role_authority_repair,
                    );
                }
            }
        }
        canonicalize_novel_contract_to_character_authority(base);
        base.normalize();
    }

    pub(crate) fn apply_title_repair_to_draft(&self, draft: &mut SessionCreationDraftState) {
        match self {
            Self::Title(patch) => patch.apply_repair_to_draft(draft),
            Self::Metadata(patch) => patch.title.apply_repair_to_draft(draft),
            Self::Batch(items) => {
                for item in items {
                    item.apply_title_repair_to_draft(draft);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn has_repairable_title_for_draft(&self, draft: &SessionCreationDraftState) -> bool {
        match self {
            Self::Title(patch) => patch.best_title_candidate_for_draft(draft).is_some(),
            Self::Metadata(patch) => patch.title.best_title_candidate_for_draft(draft).is_some(),
            Self::Batch(items) => items
                .iter()
                .any(|item| item.has_repairable_title_for_draft(draft)),
            _ => true,
        }
    }

    pub(crate) fn title_repair_failure_reasons_for_draft(
        &self,
        draft: &SessionCreationDraftState,
    ) -> Vec<String> {
        match self {
            Self::Title(patch) => patch.title_repair_failure_reasons_for_draft(draft),
            Self::Metadata(patch) => patch.title.title_repair_failure_reasons_for_draft(draft),
            Self::Batch(items) => {
                let mut reasons = items
                    .iter()
                    .flat_map(|item| item.title_repair_failure_reasons_for_draft(draft))
                    .collect::<Vec<_>>();
                reasons.sort();
                reasons.dedup();
                reasons
            }
            _ => Vec::new(),
        }
    }
}

fn merge_character_patch_scope(
    authority: &mut Vec<CharacterContract>,
    incoming: &[CharacterContract],
    volume_count: usize,
    allow_role_authority_repair: bool,
) {
    if authority.is_empty() {
        authority.extend_from_slice(incoming);
        return;
    }
    let authority_names = authority
        .iter()
        .map(|character| character.canonical_name.trim().to_string())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    let repair_complete_role_table = allow_role_authority_repair
        && complete_canonical_character_role_repair(authority, incoming);
    for candidate in incoming {
        let existing_index = authority
            .iter()
            .position(|known| {
                !value_missing(&known.character_id)
                    && known.character_id.trim() == candidate.character_id.trim()
            })
            .or_else(|| {
                authority.iter().position(|known| {
                    known.canonical_name.trim() == candidate.canonical_name.trim()
                })
            })
            .or_else(|| {
                authority
                    .iter()
                    .position(|known| character_contract_roles_match(known, candidate))
            });
        if let Some(existing) = existing_index.and_then(|index| authority.get_mut(index)) {
            if repair_complete_role_table
                && existing.canonical_name.trim() == candidate.canonical_name.trim()
            {
                replace_character_role_authority_fields(existing, candidate);
            }
            merge_missing_character_contract_fields(
                existing,
                candidate,
                &authority_names,
                volume_count,
            );
        }
    }
}

fn character_patch_authority_replacements(
    source: &[CharacterContract],
    authority: &[CharacterContract],
) -> BTreeMap<String, String> {
    let mut replacements = BTreeMap::new();
    let mut claimed_authority_names = Vec::new();
    for source_character in source {
        let old_name = source_character.canonical_name.trim();
        if value_missing(old_name) {
            continue;
        }
        let target = authority
            .iter()
            .find(|candidate| candidate.canonical_name.trim() == old_name)
            .or_else(|| {
                authority.iter().find(|candidate| {
                    candidate
                        .aliases
                        .iter()
                        .chain(candidate.previous_names.iter())
                        .any(|name| name.trim() == old_name)
                })
            })
            .or_else(|| {
                authority.iter().find(|candidate| {
                    !claimed_authority_names
                        .iter()
                        .any(|known| known == candidate.canonical_name.trim())
                        && character_contract_roles_match(source_character, candidate)
                })
            });
        let Some(target) = target else {
            continue;
        };
        let new_name = target.canonical_name.trim();
        claimed_authority_names.push(new_name.to_string());
        if old_name != new_name && !value_missing(new_name) {
            replacements.insert(old_name.to_string(), new_name.to_string());
        }
    }
    replacements
}

pub(crate) fn rewrite_novel_contract_names(
    contract: &mut NovelCreationContract,
    replacements: &BTreeMap<String, String>,
) {
    if replacements.is_empty() {
        return;
    }
    for value in [
        &mut contract.brief,
        &mut contract.premise,
        &mut contract.ending.desired_resolution,
        &mut contract.ending.final_state,
        &mut contract.protagonist_arc,
        &mut contract.world_imagery,
        &mut contract.main_causal_spine,
        &mut contract.title.rationale,
        &mut contract.outline.raw_outline,
    ] {
        rewrite_structured_character_references(value, replacements);
    }
    for value in contract
        .title
        .candidates
        .iter_mut()
        .chain(contract.ending.must_resolve.iter_mut())
        .chain(contract.ending.allowed_open_questions.iter_mut())
        .chain(contract.themes.iter_mut())
        .chain(contract.world_rules.iter_mut())
        .chain(contract.style_rules.iter_mut())
        .chain(contract.must_avoid.iter_mut())
    {
        rewrite_structured_character_references(value, replacements);
    }
    for volume in &mut contract.outline.volumes {
        for value in [
            &mut volume.title,
            &mut volume.objective,
            &mut volume.ending_change,
        ] {
            rewrite_structured_character_references(value, replacements);
        }
    }
    for chapter in &mut contract.outline.near_chapters {
        for value in [&mut chapter.goal, &mut chapter.expected_turn] {
            rewrite_structured_character_references(value, replacements);
        }
    }
    for character in &mut contract.characters {
        for value in [
            &mut character.desire,
            &mut character.fear,
            &mut character.bottom_line,
            &mut character.arc_start,
            &mut character.arc_end,
            &mut character.planned_entry,
            &mut character.planned_exit,
        ] {
            rewrite_structured_character_references(value, replacements);
        }
    }
    rewrite_contract_v2_names(&mut contract.structured, replacements);
}

fn record_superseded_character_names(
    characters: &mut [CharacterContract],
    replacements: &BTreeMap<String, String>,
) {
    for (old_name, new_name) in replacements {
        let Some(character) = characters.iter_mut().find(|character| {
            character.canonical_name.trim() == new_name.trim()
                && character.name_source.trim() == "generated_by_writing_tool_policy"
        }) else {
            continue;
        };
        if !character
            .previous_names
            .iter()
            .any(|name| name.trim() == old_name.trim())
        {
            character.previous_names.push(old_name.clone());
        }
    }
}

pub(crate) fn canonicalize_novel_contract_to_character_authority(
    contract: &mut NovelCreationContract,
) {
    if contract.characters.is_empty() {
        return;
    }
    let identity_metadata = contract
        .characters
        .iter()
        .map(|character| {
            (
                character.canonical_name.trim().to_string(),
                (character.aliases.clone(), character.previous_names.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut character_lines = contract
        .characters
        .iter()
        .map(CharacterContract::to_draft_line)
        .collect::<Vec<_>>();
    let authority = CharacterAuthority::from_lines(&character_lines);
    if authority.default_character().is_none() {
        return;
    }

    let replacements = stale_character_arc_subject_replacements(contract, &authority);
    if !replacements.is_empty() {
        rewrite_novel_contract_names(contract, &replacements);
        character_lines = contract
            .characters
            .iter()
            .map(CharacterContract::to_draft_line)
            .collect();
    }

    canonicalize_character_anchor_lines_to_authority(&mut character_lines, &authority);
    contract.characters = character_lines
        .iter()
        .map(|line| super::draft_character_line_to_contract(line))
        .collect();
    for character in &mut contract.characters {
        if let Some((aliases, previous_names)) =
            identity_metadata.get(character.canonical_name.trim())
        {
            character.aliases = aliases.clone();
            character.previous_names = previous_names.clone();
        }
    }

    for value in [
        &mut contract.brief,
        &mut contract.premise,
        &mut contract.ending.desired_resolution,
        &mut contract.ending.final_state,
        &mut contract.protagonist_arc,
        &mut contract.world_imagery,
        &mut contract.main_causal_spine,
        &mut contract.title.rationale,
        &mut contract.outline.raw_outline,
    ] {
        rewrite_external_character_references_to_authority(value, &authority);
    }
    for value in contract
        .ending
        .must_resolve
        .iter_mut()
        .chain(contract.ending.allowed_open_questions.iter_mut())
        .chain(contract.themes.iter_mut())
        .chain(contract.world_rules.iter_mut())
        .chain(contract.style_rules.iter_mut())
        .chain(contract.must_avoid.iter_mut())
    {
        rewrite_external_character_references_to_authority(value, &authority);
    }
    for volume in &mut contract.outline.volumes {
        for value in [
            &mut volume.title,
            &mut volume.objective,
            &mut volume.ending_change,
        ] {
            rewrite_external_character_references_to_authority(value, &authority);
        }
    }
    for chapter in &mut contract.outline.near_chapters {
        for value in [&mut chapter.goal, &mut chapter.expected_turn] {
            rewrite_external_character_references_to_authority(value, &authority);
        }
    }
    canonicalize_contract_v2_to_authority(&mut contract.structured, &authority);
}

fn stale_character_arc_subject_replacements(
    contract: &NovelCreationContract,
    authority: &CharacterAuthority,
) -> BTreeMap<String, String> {
    let mut replacements = BTreeMap::new();
    for character in &contract.characters {
        let owner = character.canonical_name.trim();
        if value_missing(owner) || !authority.contains(owner) {
            continue;
        }
        for value in [&character.arc_start, &character.arc_end] {
            let Some(reference) =
                crate::tool::writing::typed_contract_gate::leading_character_arc_subject(value)
            else {
                continue;
            };
            if !authority.contains(&reference) && reference != owner {
                replacements.insert(reference, owner.to_string());
            }
        }
    }
    if let Some(primary) = authority.primary.as_deref() {
        if let Some(reference) =
            crate::tool::writing::typed_contract_gate::leading_character_arc_subject(
                &contract.protagonist_arc,
            )
        {
            if !authority.contains(&reference) && reference != primary {
                replacements.insert(reference, primary.to_string());
            }
        }
    }
    replacements
}

fn validate_character_patch_scope(
    patch: &CharacterPatch,
    draft: &SessionCreationDraftState,
    issues: &mut Vec<String>,
) {
    let existing = draft
        .fiction_characters
        .iter()
        .map(|line| super::draft_character_line_to_contract(line))
        .filter(|character| !value_missing(&character.canonical_name))
        .collect::<Vec<_>>();
    if existing.is_empty() {
        let primary_count = patch
            .characters
            .iter()
            .filter(|character| character.role_looks_primary())
            .count();
        if primary_count != 1 {
            issues.push(format!(
                "character_patch 必须恰好 1 个主角槽位，当前为 {primary_count}"
            ));
        }
        if patch
            .characters
            .iter()
            .filter(|character| !character.role_looks_primary())
            .count()
            < 1
        {
            issues.push("character_patch 至少需要 1 个非主角关键角色".to_string());
        }
        for character in &patch.characters {
            if value_missing(&character.canonical_name) {
                issues.push("character_patch 角色缺少 canonical_name".to_string());
            }
            if value_missing(&character.role)
                || value_missing(&character.desire)
                || value_missing(&character.fear)
                || value_missing(&character.bottom_line)
                || value_missing(&character.arc_start)
                || value_missing(&character.arc_end)
            {
                issues.push(format!(
                    "character_patch 角色 {} 缺少欲望/恐惧/底线/弧线字段",
                    empty_display(&character.canonical_name, "未命名角色")
                ));
            }
        }
        return;
    }

    if patch.characters.is_empty() {
        issues.push("character_patch 没有可合并的角色字段".to_string());
        return;
    }
    let mut slot_coverage = CharacterRoleSlotCoverage::from_characters(&existing);
    for character in &patch.characters {
        let name = character.canonical_name.trim();
        if value_missing(name) {
            issues.push("character_patch 局部修复缺少 canonical_name".to_string());
            continue;
        }
        if !existing
            .iter()
            .any(|known| known.canonical_name.trim() == name)
        {
            let fills_missing_support = !slot_coverage.has_supporting
                && !character.role_looks_primary()
                && character.role_family().is_some();
            let fills_missing_pressure =
                !slot_coverage.has_pressure && character.role_looks_like_pressure_source();
            if fills_missing_support && fills_missing_pressure {
                issues.push(format!(
                    "character_patch 新增角色 `{name}` 不能同时占用关系角色和压力角色两个互斥槽位"
                ));
                continue;
            }
            if !fills_missing_support && !fills_missing_pressure {
                issues.push(format!(
                    "character_patch 局部修复引用了角色权威表外姓名 `{name}`"
                ));
                continue;
            }
            if value_missing(&character.role)
                || value_missing(&character.desire)
                || value_missing(&character.fear)
                || value_missing(&character.bottom_line)
                || value_missing(&character.arc_start)
                || value_missing(&character.arc_end)
                || value_missing(&character.planned_entry)
                || value_missing(&character.planned_exit)
            {
                issues.push(format!(
                    "character_patch 新增的缺失角色槽位 `{name}` 必须提供完整欲望、恐惧、底线和弧线字段"
                ));
                continue;
            }
            slot_coverage.has_supporting |= fills_missing_support;
            slot_coverage.has_pressure |= fills_missing_pressure;
            continue;
        }
        if [
            character.desire.as_str(),
            character.fear.as_str(),
            character.bottom_line.as_str(),
            character.arc_start.as_str(),
            character.arc_end.as_str(),
        ]
        .iter()
        .all(|value| value_missing(value))
        {
            issues.push(format!(
                "character_patch 局部修复没有提供角色 `{name}` 的可修复锚点"
            ));
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TitlePatch {
    pub(crate) canonical_title: String,
    pub(crate) candidates: Vec<String>,
    pub(crate) candidate_rationales: BTreeMap<String, String>,
    pub(crate) candidate_hook_types: BTreeMap<String, String>,
    pub(crate) rationale: String,
    pub(crate) source: TitleSource,
}

impl TitlePatch {
    fn apply_to_draft(&self, draft: &mut SessionCreationDraftState) {
        let title = self.best_title_for_draft(draft);
        let rationale = self.rationale_for_title(draft, &title);
        if matches!(self.source, TitleSource::User) {
            draft.title = title.to_string();
        } else if title_patch_should_replace_existing_title(draft, &title, &self.rationale) {
            draft.title = title.to_string();
        } else {
            merge_missing_aware_string(&mut draft.title, &title);
        }
        if !value_missing(&rationale)
            && !naming::title_rationale_is_concrete(&draft.fiction_title_rationale, &draft.title)
        {
            draft.fiction_title_rationale = rationale;
        } else {
            merge_missing_aware_string(&mut draft.fiction_title_rationale, &rationale);
        }
    }

    pub(crate) fn apply_repair_to_draft(&self, draft: &mut SessionCreationDraftState) {
        if let Some(candidate) = self.best_title_candidate_for_draft(draft) {
            draft.title = candidate.title;
            draft.fiction_title_rationale = candidate.rationale;
        }
    }

    fn apply_metadata_repair_to_draft(&self, draft: &mut SessionCreationDraftState) {
        let evidence = draft_title_evidence(draft, &self.rationale);
        let mut canonical_candidate = Vec::new();
        push_unique_title_candidate(&mut canonical_candidate, &self.canonical_title);
        let candidate = self
            .best_title_candidate_from_candidates(draft, &canonical_candidate, &evidence)
            .or_else(|| self.best_title_candidate_for_draft(draft));
        if let Some(candidate) = candidate {
            draft.title = candidate.title;
            draft.fiction_title_rationale = candidate.rationale;
        }
    }

    pub(crate) fn has_valid_provided_title_for_draft(
        &self,
        draft: &SessionCreationDraftState,
    ) -> bool {
        let evidence = draft_title_evidence(draft, &self.rationale);
        let mut provided_candidates = Vec::new();
        push_unique_title_candidate(&mut provided_candidates, &self.canonical_title);
        for candidate in &self.candidates {
            push_unique_title_candidate(&mut provided_candidates, candidate);
        }
        self.best_title_candidate_from_candidates(draft, &provided_candidates, &evidence)
            .is_some()
    }

    pub(crate) fn title_repair_failure_reasons_for_draft(
        &self,
        draft: &SessionCreationDraftState,
    ) -> Vec<String> {
        let evidence = draft_title_evidence(draft, &self.rationale);
        let evidence_model = naming::BookTitleEvidence::new("书名", &evidence);
        let mut provided_candidates = Vec::new();
        push_unique_title_candidate(&mut provided_candidates, &self.canonical_title);
        for candidate in &self.candidates {
            push_unique_title_candidate(&mut provided_candidates, candidate);
        }
        let mut reasons = naming::select_book_title_candidate_decision(
            provided_candidates.iter().map(|candidate| {
                naming::BookTitleCandidate::new(
                    candidate.as_str(),
                    self.rationale_for_title(draft, candidate),
                )
            }),
            &evidence_model,
        )
        .reasons;

        let declared_decision = naming::select_book_title_candidate_decision(
            naming::declared_book_title_candidates_from_contract_evidence(&evidence),
            &evidence_model,
        );
        reasons.extend(declared_decision.reasons);
        reasons.sort();
        reasons.dedup();
        if reasons.is_empty() {
            reasons.push(
                "书名修复没有形成文字完整且有合同证据支撑的候选；需要从当前合同的终局、主线、世界规则、关键物件、地点或事件重新取名"
                    .to_string(),
            );
        }
        reasons
    }

    fn best_title_for_draft(&self, draft: &SessionCreationDraftState) -> String {
        self.best_title_candidate_for_draft(draft)
            .map(|candidate| candidate.title)
            .unwrap_or_default()
    }

    pub(crate) fn best_title_candidate_for_draft(
        &self,
        draft: &SessionCreationDraftState,
    ) -> Option<naming::BookTitleCandidate> {
        let evidence = draft_title_evidence(draft, &self.rationale);
        let mut provided_candidates = Vec::new();
        push_unique_title_candidate(&mut provided_candidates, &self.canonical_title);
        for candidate in &self.candidates {
            push_unique_title_candidate(&mut provided_candidates, candidate);
        }
        if let Some(candidate) =
            self.best_title_candidate_from_candidates(draft, &provided_candidates, &evidence)
        {
            return Some(candidate);
        }
        if !provided_candidates.is_empty() {
            return None;
        }

        let declared_candidates =
            naming::declared_book_title_candidates_from_contract_evidence(&evidence);
        let evidence_model = naming::BookTitleEvidence::new("书名", &evidence);
        let decision =
            naming::select_book_title_candidate_decision(declared_candidates, &evidence_model);
        if decision.selected.is_some() {
            return decision.selected;
        }

        None
    }

    fn best_title_candidate_from_candidates(
        &self,
        draft: &SessionCreationDraftState,
        candidates: &[String],
        evidence: &str,
    ) -> Option<naming::BookTitleCandidate> {
        let evidence_model = naming::BookTitleEvidence::new("书名", evidence);
        let decision = naming::select_book_title_candidate_decision(
            candidates.iter().map(|candidate| {
                naming::BookTitleCandidate::new(
                    candidate.as_str(),
                    self.rationale_for_title(draft, candidate),
                )
            }),
            &evidence_model,
        );
        decision.selected
    }

    fn rationale_for_title(&self, draft: &SessionCreationDraftState, title: &str) -> String {
        if let Some(rationale) = self
            .candidate_rationales
            .get(title.trim())
            .map(String::as_str)
            .filter(|value| !value_missing(value))
        {
            let rationale = self.rationale_with_candidate_hook_type(title, rationale);
            let evidence = draft_title_evidence(draft, "");
            if naming::title_contract_basis_issue(title, "书名", &rationale, &evidence).is_none()
                || naming::title_rationale_is_concrete(&rationale, title)
            {
                return rationale;
            }
        }
        let rationale = self.rationale.trim();
        let evidence = draft_title_evidence(draft, "");
        if !value_missing(rationale)
            && !title_patch_rationale_has_candidate_residue(rationale)
            && (naming::title_contract_basis_issue(title, "书名", rationale, &evidence).is_none()
                || naming::title_rationale_is_concrete(rationale, title))
        {
            return self.rationale_with_candidate_hook_type(title, rationale);
        }
        if !title_has_story_anchor_support(title, &evidence) {
            return self.rationale_with_candidate_hook_type(title, rationale);
        }
        self.rationale_with_candidate_hook_type(
            title,
            &contract_evidence_title_rationale_from_draft(title, draft),
        )
    }

    fn rationale_with_candidate_hook_type(&self, title: &str, rationale: &str) -> String {
        let Some(hook_type) = self
            .candidate_hook_types
            .get(title.trim())
            .map(String::as_str)
            .filter(|value| !value_missing(value))
        else {
            return rationale.trim().to_string();
        };
        let rationale = rationale.trim();
        if rationale.contains(hook_type) {
            rationale.to_string()
        } else {
            format!("{rationale} 命名入口：{hook_type}。")
        }
    }
}

fn title_patch_rationale_has_candidate_residue(rationale: &str) -> bool {
    [
        "其他候选",
        "候选分别",
        "候选包括",
        "备选",
        "另一个候选",
        "也可以叫",
        "candidate",
        "alternatives",
    ]
    .iter()
    .any(|term| rationale.contains(term))
}

fn title_has_story_anchor_support(title: &str, evidence: &str) -> bool {
    naming::title_anchor_tokens(title)
        .iter()
        .any(|token| evidence.contains(token))
}

fn contract_evidence_title_rationale_from_draft(
    title: &str,
    draft: &SessionCreationDraftState,
) -> String {
    let basis = [
        draft.fiction_ending_direction.as_str(),
        draft.fiction_main_causal_spine.as_str(),
        draft.fiction_premise.as_str(),
        draft.fiction_world_imagery.as_str(),
        draft.brief.as_str(),
    ]
    .into_iter()
    .find(|value| !value_missing(value))
    .unwrap_or("当前合同的终局、主线和世界规则");
    naming::book_title_candidate_rationale_from_story_evidence(title, basis)
}

fn push_unique_title_candidate(out: &mut Vec<String>, candidate: &str) {
    let candidate = candidate.trim();
    if value_missing(candidate) || out.iter().any(|existing| existing == candidate) {
        return;
    }
    out.push(candidate.to_string());
}

fn title_patch_should_replace_existing_title(
    draft: &SessionCreationDraftState,
    incoming_title: &str,
    incoming_rationale: &str,
) -> bool {
    if value_missing(incoming_title) || draft.title.trim().is_empty() {
        return false;
    }
    if draft.lifecycle_status() == CreationDraftLifecycleStatus::ContractReady {
        return false;
    }
    let evidence = draft_title_evidence(draft, incoming_rationale);
    let existing_has_issue = title_is_unlocked_contract_placeholder(draft)
        || naming::title_formality_issue(&draft.title, "书名").is_some()
        || naming::title_contract_basis_issue(
            &draft.title,
            "书名",
            &draft.fiction_title_rationale,
            &evidence,
        )
        .is_some();
    if existing_has_issue && draft.lifecycle_status() != CreationDraftLifecycleStatus::ContractReady
    {
        return true;
    }
    let incoming_has_issue =
        naming::title_contract_basis_issue(incoming_title, "书名", incoming_rationale, &evidence)
            .is_some();
    if existing_has_issue && !incoming_has_issue {
        return true;
    }
    existing_has_issue && !incoming_has_issue
}

pub(crate) fn draft_title_evidence(
    draft: &SessionCreationDraftState,
    incoming_rationale: &str,
) -> String {
    let mut evidence = Vec::new();
    if let Some(basis) = draft_typed_contract_story_basis(draft) {
        evidence.push(basis);
    }
    evidence.extend(
        [
            draft.fiction_premise.as_str(),
            draft.fiction_ending_direction.as_str(),
            draft.fiction_protagonist_arc.as_str(),
            draft.fiction_world_imagery.as_str(),
            draft.fiction_main_causal_spine.as_str(),
            draft.fiction_outline.as_str(),
            incoming_rationale,
        ]
        .into_iter()
        .map(ToOwned::to_owned),
    );
    evidence
        .into_iter()
        .filter(|value| !value_missing(value))
        .collect::<Vec<_>>()
        .join("\n")
}

fn draft_typed_contract_story_basis(draft: &SessionCreationDraftState) -> Option<String> {
    let value = draft.current_contract.as_ref().or_else(|| {
        draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|candidate| candidate.get("normalized"))
    })?;
    let text = serde_json::to_string(value).ok()?;
    let contract = NovelCreationContract::parse_json_boundary(&text)?;
    let basis = contract.story_basis_text();
    (!value_missing(&basis)).then_some(basis)
}

fn title_is_unlocked_contract_placeholder(draft: &SessionCreationDraftState) -> bool {
    let title = draft.title.trim();
    if title.is_empty() {
        return true;
    }
    let compact_title = title.replace(char::is_whitespace, "");
    let compact_genre = draft.genre.trim().replace(char::is_whitespace, "");
    if !compact_genre.is_empty() && compact_title == compact_genre {
        return true;
    }
    let lowered = title.to_ascii_lowercase();
    matches!(
        lowered.as_str(),
        "小说" | "故事" | "fiction" | "story" | "untitled"
    ) || title.starts_with("未命名")
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SkeletonPatch {
    pub(crate) genre: String,
    pub(crate) brief: String,
    pub(crate) target_units: Option<usize>,
    pub(crate) chapter_unit_target: Option<usize>,
    pub(crate) max_chapters_per_turn: Option<usize>,
    pub(crate) premise: String,
    pub(crate) ending_desired_resolution: String,
    pub(crate) ending_final_state: String,
    pub(crate) protagonist_arc: String,
    pub(crate) world_imagery: String,
    pub(crate) main_causal_spine: String,
}

impl SkeletonPatch {
    fn apply_to_draft(&self, draft: &mut SessionCreationDraftState) {
        replace_non_missing_string(&mut draft.genre, &self.genre);
        let clean_brief = intent::sanitize_creation_brief_value(&self.brief);
        replace_non_missing_string(&mut draft.brief, &clean_brief);
        if draft.target_units.is_none() {
            draft.target_units = self.target_units;
        }
        if draft.chapter_unit_target.is_none() {
            draft.chapter_unit_target = self.chapter_unit_target;
        }
        if draft.max_chapters_per_turn.is_none() {
            draft.max_chapters_per_turn = self.max_chapters_per_turn;
        }
        replace_non_missing_string(&mut draft.fiction_premise, &self.premise);
        replace_non_missing_string(
            &mut draft.fiction_ending_direction,
            &first_non_empty_string(&[
                self.ending_desired_resolution.as_str(),
                self.ending_final_state.as_str(),
            ]),
        );
        replace_non_missing_string(&mut draft.fiction_protagonist_arc, &self.protagonist_arc);
        replace_non_missing_string(&mut draft.fiction_world_imagery, &self.world_imagery);
        replace_non_missing_string(
            &mut draft.fiction_main_causal_spine,
            &self.main_causal_spine,
        );
    }
}

fn replace_non_missing_string(target: &mut String, incoming: &str) {
    if !value_missing(incoming) {
        *target = incoming.trim().to_string();
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CharacterPatch {
    pub(crate) characters: Vec<CharacterContract>,
    pub(crate) relationship_ledger: Vec<RelationshipLedgerEntry>,
    pub(crate) emotional_state_ledger: Vec<EmotionalStateLedgerEntry>,
}

impl CharacterPatch {
    #[cfg(test)]
    fn apply_to_draft(&self, draft: &mut SessionCreationDraftState) {
        self.apply_to_draft_with_role_repair_policy(draft, false);
    }

    fn apply_to_draft_with_role_repair_policy(
        &self,
        draft: &mut SessionCreationDraftState,
        allow_role_authority_repair: bool,
    ) {
        if !self.characters.is_empty() {
            if draft_has_trusted_character_name_authority(draft) {
                self.apply_as_authority_field_repair(draft, allow_role_authority_repair);
                return;
            }
            let mut governed_contracts = self.characters.clone();
            governed_contracts.retain(|character| {
                !super::planning_notes_explicitly_exclude_character(
                    &draft.planning_notes,
                    &character.canonical_name,
                )
            });
            super::complete_minimum_character_slots(&mut governed_contracts, draft);
            let previous_untrusted_candidates = draft
                .fiction_characters
                .iter()
                .map(|line| super::draft_character_line_to_contract(line))
                .filter(|character| !value_missing(&character.canonical_name))
                .collect::<Vec<_>>();
            let mut governance = govern_initial_character_names(&mut governed_contracts, draft);
            reconcile_untrusted_character_candidates_to_governed_authority(
                &previous_untrusted_candidates,
                &mut governed_contracts,
                &mut governance,
            );
            remove_character_plan_references_to_missing_outline(
                &mut governed_contracts,
                super::strong_novel_contract_from_creation_draft(draft)
                    .outline
                    .volumes
                    .len(),
            );
            let mut governed_lines = governed_contracts
                .iter()
                .map(CharacterContract::to_draft_line)
                .collect::<Vec<_>>();
            rewrite_draft_story_surface_names(draft, governance.replacements());
            let authority = CharacterAuthority::from_lines(&governed_lines);
            canonicalize_character_anchor_lines_to_authority(&mut governed_lines, &authority);
            canonicalize_draft_story_surface_to_authority(draft, &authority);
            let mut contract_v2 = draft.contract_v2();
            rewrite_contract_v2_names(&mut contract_v2, governance.replacements());
            canonicalize_contract_v2_to_authority(&mut contract_v2, &authority);
            draft.set_contract_v2(contract_v2);
            if !self.relationship_ledger.is_empty() {
                let mut ledger = self.relationship_ledger.clone();
                rewrite_relationship_ledger_names(&mut ledger, governance.replacements());
                canonicalize_relationship_ledger_to_authority(&mut ledger, &authority);
                draft.relationship_ledger = ledger;
            }
            if !self.emotional_state_ledger.is_empty() {
                let mut ledger = self.emotional_state_ledger.clone();
                rewrite_emotional_state_ledger_names(&mut ledger, governance.replacements());
                canonicalize_emotional_state_ledger_to_authority(&mut ledger, &authority);
                draft.emotional_state_ledger = ledger;
            }
            let mut governed_contracts = governed_lines
                .iter()
                .map(|line| super::draft_character_line_to_contract(line))
                .collect::<Vec<_>>();
            governance.lock_authority(&mut governed_contracts);
            draft.fiction_characters = governed_contracts
                .iter()
                .map(CharacterContract::to_draft_line)
                .collect();
        }
        if self.characters.is_empty() && !self.relationship_ledger.is_empty() {
            draft.relationship_ledger = self.relationship_ledger.clone();
        }
        if self.characters.is_empty() && !self.emotional_state_ledger.is_empty() {
            draft.emotional_state_ledger = self.emotional_state_ledger.clone();
        }
    }

    fn apply_as_authority_field_repair(
        &self,
        draft: &mut SessionCreationDraftState,
        allow_role_authority_repair: bool,
    ) {
        let volume_count = super::strong_novel_contract_from_creation_draft(draft)
            .outline
            .volumes
            .len();
        let trusted_lines = draft
            .fiction_characters
            .iter()
            .filter(|line| draft_character_line_has_trusted_name_authority(draft, line))
            .cloned()
            .collect::<Vec<_>>();
        let previous_authority = CharacterAuthority::from_lines(&trusted_lines);
        let name_sources = trusted_lines
            .iter()
            .filter_map(|line| {
                let character = super::draft_character_line_to_contract(line);
                super::character_line_name_source(line)
                    .map(|source| (character.canonical_name.trim().to_string(), source))
            })
            .collect::<BTreeMap<_, _>>();
        let mut existing = trusted_lines
            .iter()
            .map(|line| super::draft_character_line_to_contract(line))
            .filter(|character| !value_missing(&character.canonical_name))
            .collect::<Vec<_>>();
        if existing.is_empty() {
            return;
        }

        let incoming = self.characters.clone();
        let repair_complete_role_table = allow_role_authority_repair
            && complete_canonical_character_role_repair(&existing, &incoming);
        let allow_role_alignment = incoming.len() >= existing.len();
        let mut replacements = BTreeMap::new();
        let mut new_characters = Vec::new();

        for incoming_character in incoming {
            if value_missing(&incoming_character.canonical_name) {
                continue;
            }
            let incoming_name = incoming_character.canonical_name.trim();
            let canonical_match = existing
                .iter()
                .position(|known| known.canonical_name.trim() == incoming_name);
            let historical_identity_match = existing.iter().position(|known| {
                known
                    .aliases
                    .iter()
                    .chain(known.previous_names.iter())
                    .any(|name| name.trim() == incoming_name)
            });
            let matched_index = if let Some(index) = canonical_match {
                Some(index)
            } else if let Some(index) = historical_identity_match {
                if character_contract_roles_match(&existing[index], &incoming_character) {
                    Some(index)
                } else {
                    // A locked alias or historical name cannot be reassigned to
                    // another role or promoted into a new character identity.
                    continue;
                }
            } else if allow_role_alignment {
                existing
                    .iter()
                    .position(|known| character_contract_roles_match(known, &incoming_character))
            } else {
                None
            };

            if let Some(index) = matched_index {
                let preserved_name = existing[index].canonical_name.trim().to_string();
                if incoming_name != preserved_name {
                    replacements.insert(incoming_name.to_string(), preserved_name);
                }
                if repair_complete_role_table && canonical_match == Some(index) {
                    replace_character_role_authority_fields(
                        &mut existing[index],
                        &incoming_character,
                    );
                }
                let authority_names = existing
                    .iter()
                    .map(|character| character.canonical_name.trim().to_string())
                    .filter(|name| !value_missing(name))
                    .collect::<Vec<_>>();
                merge_missing_character_contract_fields(
                    &mut existing[index],
                    &incoming_character,
                    &authority_names,
                    volume_count,
                );
            } else {
                new_characters.push(incoming_character);
            }
        }

        if !new_characters.is_empty() {
            let used_names = existing
                .iter()
                .map(|character| character.canonical_name.trim().to_string())
                .filter(|name| !value_missing(name))
                .collect::<BTreeSet<_>>();
            let governance = govern_character_name_candidates(
                &mut new_characters,
                draft,
                used_names,
                "incremental-character-slot",
            );
            replacements.extend(governance.replacements().clone());
            governance.lock_authority(&mut new_characters);
            existing.extend(new_characters);
        }
        remove_character_plan_references_to_missing_outline(&mut existing, volume_count);

        let original_lines = existing
            .iter()
            .map(|character| {
                let mut character = character.clone();
                character.name_source = name_sources
                    .get(character.canonical_name.trim())
                    .map(String::as_str)
                    .or_else(|| {
                        (character.name_source.trim() == "generated_by_writing_tool_policy")
                            .then_some("generated_by_writing_tool_policy")
                    })
                    .or_else(|| {
                        previous_authority
                            .contains(character.canonical_name.trim())
                            .then_some("contract_authority")
                    })
                    .unwrap_or("contract_authority")
                    .to_string();
                character.to_draft_line()
            })
            .collect::<Vec<_>>();
        let mut governed_lines = original_lines;
        let authority = CharacterAuthority::from_lines(&governed_lines);
        canonicalize_character_anchor_lines_to_authority(&mut governed_lines, &authority);
        draft.fiction_characters = governed_lines;
        rewrite_draft_story_surface_names(draft, &replacements);
        canonicalize_draft_story_surface_to_authority(draft, &authority);

        let mut contract_v2 = draft.contract_v2();
        if !self.relationship_ledger.is_empty() {
            let mut ledger = self.relationship_ledger.clone();
            rewrite_relationship_ledger_names(&mut ledger, &replacements);
            canonicalize_relationship_ledger_to_authority(&mut ledger, &authority);
            draft.relationship_ledger = ledger;
        }
        if !self.emotional_state_ledger.is_empty() {
            let mut ledger = self.emotional_state_ledger.clone();
            rewrite_emotional_state_ledger_names(&mut ledger, &replacements);
            canonicalize_emotional_state_ledger_to_authority(&mut ledger, &authority);
            draft.emotional_state_ledger = ledger;
        }
        rewrite_contract_v2_names(&mut contract_v2, &replacements);
        canonicalize_contract_v2_to_authority(&mut contract_v2, &authority);
        draft.set_contract_v2(contract_v2);
    }
}

fn complete_canonical_character_role_repair(
    existing: &[CharacterContract],
    incoming: &[CharacterContract],
) -> bool {
    existing.len() >= 2
        && existing.len() == incoming.len()
        && incoming.iter().all(|candidate| {
            !value_missing(&candidate.canonical_name)
                && !value_missing(&candidate.role)
                && !value_missing(&candidate.desire)
                && !value_missing(&candidate.fear)
                && !value_missing(&candidate.bottom_line)
                && !value_missing(&candidate.arc_start)
                && !value_missing(&candidate.arc_end)
                && existing
                    .iter()
                    .any(|known| known.canonical_name.trim() == candidate.canonical_name.trim())
        })
        && existing.iter().all(|known| {
            incoming
                .iter()
                .filter(|candidate| candidate.canonical_name.trim() == known.canonical_name.trim())
                .count()
                == 1
        })
        && incoming
            .iter()
            .filter(|candidate| candidate.role_looks_primary())
            .count()
            == 1
        && incoming.iter().any(|candidate| {
            existing.iter().any(|known| {
                known.canonical_name.trim() == candidate.canonical_name.trim()
                    && known.role.trim() != candidate.role.trim()
            })
        })
}

fn replace_character_role_authority_fields(
    target: &mut CharacterContract,
    incoming: &CharacterContract,
) {
    for (target, incoming) in [
        (&mut target.role, &incoming.role),
        (&mut target.desire, &incoming.desire),
        (&mut target.fear, &incoming.fear),
        (&mut target.bottom_line, &incoming.bottom_line),
        (&mut target.arc_start, &incoming.arc_start),
        (&mut target.arc_end, &incoming.arc_end),
    ] {
        *target = incoming.trim().to_string();
    }
    if !value_missing(&incoming.planned_entry) {
        target.planned_entry = incoming.planned_entry.trim().to_string();
    }
    if !value_missing(&incoming.planned_exit) {
        target.planned_exit = incoming.planned_exit.trim().to_string();
    }
}

fn remove_character_plan_references_to_missing_outline(
    characters: &mut [CharacterContract],
    volume_count: usize,
) {
    if volume_count != 0 {
        return;
    }
    for character in characters {
        for value in [&mut character.planned_entry, &mut character.planned_exit] {
            if crate::tool::writing::typed_contract_gate::first_volume_reference_outside_contract(
                value,
                volume_count,
            )
            .is_some()
            {
                value.clear();
            }
        }
    }
}

fn reconcile_untrusted_character_candidates_to_governed_authority(
    previous: &[CharacterContract],
    governed: &mut [CharacterContract],
    governance: &mut InitialCharacterNameGovernance,
) {
    for old in previous {
        let old_name = old.canonical_name.trim();
        if value_missing(old_name) || !super::fiction_contract_character_name_is_valid(old_name) {
            continue;
        }
        let matching_slots = governed
            .iter()
            .enumerate()
            .filter(|(_, candidate)| character_contract_roles_match(old, candidate))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        // Role families can contain several opponents or companions.  Only a
        // unique slot is strong enough evidence to migrate an untrusted model
        // candidate without inventing an identity correspondence.
        let [index] = matching_slots.as_slice() else {
            continue;
        };
        let target = &mut governed[*index];
        let canonical = target.canonical_name.trim();
        if value_missing(canonical) || canonical == old_name {
            continue;
        }
        governance
            .replacements
            .insert(old_name.to_string(), canonical.to_string());
        if !target
            .previous_names
            .iter()
            .any(|name| name.trim() == old_name)
        {
            target.previous_names.push(old_name.to_string());
        }
    }
}

fn draft_has_trusted_character_name_authority(draft: &SessionCreationDraftState) -> bool {
    draft
        .fiction_characters
        .iter()
        .any(|line| draft_character_line_has_trusted_name_authority(draft, line))
}

fn draft_character_line_has_trusted_name_authority(
    draft: &SessionCreationDraftState,
    line: &str,
) -> bool {
    let source = super::character_line_name_source(line).unwrap_or_default();
    matches!(source.trim(), "user" | "generated_by_writing_tool_policy")
        || (!draft.project_path.trim().is_empty() && source.trim() == "contract_authority")
}

#[derive(Debug, Clone, Default)]
pub(crate) struct InitialCharacterNameGovernance {
    replacements: BTreeMap<String, String>,
    authority_sources: BTreeMap<String, String>,
}

impl InitialCharacterNameGovernance {
    pub(crate) fn replacements(&self) -> &BTreeMap<String, String> {
        &self.replacements
    }

    pub(crate) fn lock_authority(&self, characters: &mut [CharacterContract]) {
        for character in characters {
            character.name_source = self
                .authority_sources
                .get(character.canonical_name.trim())
                .cloned()
                .unwrap_or_default();
        }
    }
}

pub(crate) fn govern_initial_character_names(
    characters: &mut [CharacterContract],
    draft: &SessionCreationDraftState,
) -> InitialCharacterNameGovernance {
    let forbidden_names = forbidden_naming_authority(draft)
        .character_names
        .into_iter()
        .collect::<BTreeSet<_>>();
    let used_names = characters
        .iter()
        .filter(|character| draft_explicitly_names_character(draft, &character.canonical_name))
        .map(|character| character.canonical_name.trim().to_string())
        .filter(|name| !name.is_empty() && !forbidden_names.contains(name))
        .chain(forbidden_names.iter().cloned())
        .collect::<BTreeSet<_>>();
    govern_character_name_candidates(characters, draft, used_names, "initial-character-slot")
}

pub(crate) fn govern_character_name_candidates(
    characters: &mut [CharacterContract],
    draft: &SessionCreationDraftState,
    mut used_names: BTreeSet<String>,
    request_scope: &str,
) -> InitialCharacterNameGovernance {
    let forbidden_names = forbidden_naming_authority(draft)
        .character_names
        .into_iter()
        .collect::<BTreeSet<_>>();
    used_names.extend(forbidden_names.iter().cloned());
    let project_key = format!(
        "{}\n{}\n{}",
        draft.title.trim(),
        draft.genre.trim(),
        draft.brief.trim()
    );
    let mut governance = InitialCharacterNameGovernance::default();
    let source_name_counts = characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .fold(BTreeMap::<String, usize>::new(), |mut counts, name| {
            *counts.entry(name.to_string()).or_default() += 1;
            counts
        });

    for (index, character) in characters.iter_mut().enumerate() {
        let old_name = character.canonical_name.trim().to_string();
        let replacement_source_name = contextual_character_name_source(&old_name, draft, character)
            .unwrap_or_else(|| old_name.clone());
        let source_name_is_unambiguous = source_name_counts
            .get(&old_name)
            .copied()
            .unwrap_or_default()
            == 1;
        let user_named = draft_explicitly_names_character(draft, &old_name)
            && !forbidden_names.contains(&old_name);
        // A fresh model candidate cannot establish its own provenance. Only the
        // user request or this local allocator may create initial name authority.
        character.name_source.clear();
        if user_named {
            governance
                .authority_sources
                .insert(old_name, "user".to_string());
            continue;
        }
        let request_id = format!("{request_scope}-{index}");
        let Some(local_name) = naming::allocate_character_name(
            &project_key,
            &request_id,
            &character.role,
            &draft.language,
            &used_names,
        ) else {
            if source_name_is_unambiguous
                && !replacement_source_name.is_empty()
                && !character
                    .previous_names
                    .iter()
                    .any(|name| name.trim() == replacement_source_name)
            {
                character.previous_names.push(replacement_source_name);
            }
            character.canonical_name.clear();
            continue;
        };
        used_names.insert(local_name.clone());
        if source_name_is_unambiguous
            && !replacement_source_name.is_empty()
            && replacement_source_name != local_name
        {
            governance
                .replacements
                .insert(replacement_source_name.clone(), local_name.clone());
            if !character
                .previous_names
                .iter()
                .any(|name| name.trim() == replacement_source_name)
            {
                character.previous_names.push(replacement_source_name);
            }
        }
        character.canonical_name = local_name;
        governance.authority_sources.insert(
            character.canonical_name.clone(),
            "generated_by_writing_tool_policy".to_string(),
        );
    }

    for character in characters {
        for value in [
            &mut character.desire,
            &mut character.fear,
            &mut character.bottom_line,
            &mut character.arc_start,
            &mut character.arc_end,
            &mut character.planned_entry,
            &mut character.planned_exit,
        ] {
            rewrite_structured_character_references(value, &governance.replacements);
        }
    }
    governance
}

fn contextual_character_name_source(
    candidate: &str,
    draft: &SessionCreationDraftState,
    character: &CharacterContract,
) -> Option<String> {
    let candidate = candidate.trim();
    let count = candidate.chars().count();
    if !(3..=4).contains(&count) || !candidate.chars().all(is_cjk_unified) {
        return None;
    }
    let story_fields = [
        draft.brief.as_str(),
        draft.fiction_premise.as_str(),
        draft.fiction_ending_direction.as_str(),
        draft.fiction_protagonist_arc.as_str(),
        draft.fiction_world_imagery.as_str(),
        draft.fiction_main_causal_spine.as_str(),
        draft.fiction_outline.as_str(),
        character.desire.as_str(),
        character.fear.as_str(),
        character.bottom_line.as_str(),
        character.arc_start.as_str(),
        character.arc_end.as_str(),
    ];
    for prefix_len in (2..count).rev() {
        let prefix = candidate.chars().take(prefix_len).collect::<String>();
        if !super::fiction_contract_character_name_is_replaceable_source(&prefix) {
            continue;
        }
        if story_fields.iter().any(|text| {
            crate::tool::writing::typed_contract_gate::character_reference_extends_name_with_action(
                candidate, &prefix, text,
            )
        }) {
            return Some(prefix);
        }
    }
    None
}

pub(crate) fn draft_explicitly_names_character(
    draft: &SessionCreationDraftState,
    name: &str,
) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let marker = format!("明确指定角色姓名：{name}");
    draft
        .planning_notes
        .iter()
        .any(|note| note.trim() == marker)
}

fn merge_missing_character_contract_fields(
    target: &mut CharacterContract,
    incoming: &CharacterContract,
    authority_names: &[String],
    volume_count: usize,
) {
    fill_missing_character_field(&mut target.character_id, &incoming.character_id);
    fill_missing_character_field(&mut target.name_source, &incoming.name_source);
    target.aliases = merge_unique_strings(&target.aliases, &incoming.aliases);
    target.previous_names = merge_unique_strings(&target.previous_names, &incoming.previous_names);
    fill_missing_character_field(&mut target.role, &incoming.role);
    let locked_role = target.role.clone();
    fill_repairable_character_anchor_field(
        &mut target.desire,
        &incoming.desire,
        authority_names,
        &locked_role,
    );
    fill_repairable_character_fear_field(
        &mut target.fear,
        &incoming.fear,
        authority_names,
        &locked_role,
    );
    fill_repairable_character_bottom_line_field(
        &mut target.bottom_line,
        &incoming.bottom_line,
        authority_names,
        &locked_role,
    );
    fill_repairable_character_anchor_field(
        &mut target.arc_start,
        &incoming.arc_start,
        authority_names,
        &locked_role,
    );
    fill_repairable_character_anchor_field(
        &mut target.arc_end,
        &incoming.arc_end,
        authority_names,
        &locked_role,
    );
    let primary = target.role_looks_primary();
    fill_repairable_character_plan_field(
        &mut target.planned_entry,
        &incoming.planned_entry,
        volume_count,
        primary,
        false,
    );
    fill_repairable_character_plan_field(
        &mut target.planned_exit,
        &incoming.planned_exit,
        volume_count,
        primary,
        true,
    );
}

fn fill_missing_character_field(target: &mut String, incoming: &str) {
    let incoming = incoming.trim();
    if value_missing(target) && !value_missing(incoming) {
        *target = incoming.to_string();
    }
}

fn fill_repairable_character_plan_field(
    target: &mut String,
    incoming: &str,
    volume_count: usize,
    primary: bool,
    planned_exit: bool,
) {
    let incoming = incoming.trim();
    if value_missing(incoming) {
        return;
    }
    if value_missing(target) {
        *target = incoming.to_string();
        return;
    }
    if volume_count == 0
        || !crate::tool::writing::typed_contract_gate::character_plan_anchor_needs_repair(
            target,
            volume_count,
            primary,
            planned_exit,
        )
        || crate::tool::writing::typed_contract_gate::character_plan_anchor_needs_repair(
            incoming,
            volume_count,
            primary,
            planned_exit,
        )
    {
        return;
    }
    *target = incoming.to_string();
}

fn fill_repairable_character_anchor_field(
    target: &mut String,
    incoming: &str,
    authority_names: &[String],
    role: &str,
) {
    let incoming = incoming.trim();
    if value_missing(incoming) {
        return;
    }
    if value_missing(target) || character_anchor_field_needs_repair(target, authority_names, role) {
        *target = incoming.to_string();
    }
}

fn fill_repairable_character_fear_field(
    target: &mut String,
    incoming: &str,
    authority_names: &[String],
    role: &str,
) {
    let incoming = incoming.trim();
    if value_missing(incoming) {
        return;
    }
    if value_missing(target)
        || character_anchor_field_needs_repair(target, authority_names, role)
        || crate::tool::writing::typed_contract_gate::character_fear_ends_with_dangling_temporal_clause(
            target,
        )
    {
        *target = incoming.to_string();
    }
}

fn fill_repairable_character_bottom_line_field(
    target: &mut String,
    incoming: &str,
    authority_names: &[String],
    role: &str,
) {
    let incoming = incoming.trim();
    if value_missing(incoming) {
        return;
    }
    if value_missing(target)
        || character_anchor_field_needs_repair(target, authority_names, role)
        || crate::tool::writing::typed_contract_gate::character_bottom_line_lacks_boundary_action(
            target,
        )
    {
        *target = incoming.to_string();
    }
}

fn character_anchor_field_needs_repair(
    value: &str,
    authority_names: &[String],
    role: &str,
) -> bool {
    crate::tool::writing::typed_contract_gate::character_anchor_uses_generic_placeholder(value)
        || crate::tool::writing::typed_contract_gate::character_anchor_looks_like_storyline_or_truncated_surface(value)
        || crate::tool::writing::typed_contract_gate::character_anchor_person_references(value)
            .iter()
            .any(|reference| {
                !authority_names
                    .iter()
                    .any(|known| known.trim() == reference.trim())
            })
        || character_anchor_identity_conflicts_with_role(value, role)
}

fn character_anchor_identity_conflicts_with_role(value: &str, role: &str) -> bool {
    let expected = if role.contains("女主") {
        Some("feminine")
    } else if role.contains("男主") {
        Some("masculine")
    } else {
        None
    };
    expected.is_some_and(|expected| {
        crate::tool::writing::novel_studio::contract_explicit_identity_profile_in_character_anchor(
            value,
        )
        .is_some_and(|observed| observed != expected)
    })
}

pub(crate) fn character_contract_roles_match(
    left: &CharacterContract,
    right: &CharacterContract,
) -> bool {
    if !value_missing(&left.character_id)
        && !value_missing(&right.character_id)
        && left.character_id.trim() == right.character_id.trim()
    {
        return true;
    }
    let left_is_primary = left.role_looks_primary();
    let right_is_primary = right.role_looks_primary();
    if left_is_primary || right_is_primary {
        return left_is_primary && right_is_primary;
    }
    if left.role_looks_like_pressure_source() && right.role_looks_like_pressure_source() {
        return true;
    }
    if let (Some(left_family), Some(right_family)) = (left.role_family(), right.role_family()) {
        if left_family == right_family {
            return true;
        }
    }
    let left_role = compact_role_label(&left.role);
    let right_role = compact_role_label(&right.role);
    !left_role.is_empty() && left_role == right_role
}

fn compact_role_label(role: &str) -> String {
    role.chars()
        .filter(|ch| {
            !ch.is_whitespace() && !matches!(ch, '/' | '／' | '-' | '_' | '，' | ',' | '、')
        })
        .collect::<String>()
        .to_ascii_lowercase()
}

fn rewrite_draft_story_surface_names(
    draft: &mut SessionCreationDraftState,
    replacements: &BTreeMap<String, String>,
) {
    if replacements.is_empty() {
        return;
    }
    for value in [
        &mut draft.brief,
        &mut draft.fiction_premise,
        &mut draft.fiction_ending_direction,
        &mut draft.fiction_protagonist_arc,
        &mut draft.fiction_world_imagery,
        &mut draft.fiction_main_causal_spine,
        &mut draft.fiction_title_rationale,
        &mut draft.fiction_outline,
    ] {
        rewrite_structured_character_references(value, replacements);
    }
    for value in draft
        .fiction_themes
        .iter_mut()
        .chain(draft.fiction_world_rules.iter_mut())
        .chain(draft.fiction_style_rules.iter_mut())
        .chain(draft.fiction_must_avoid.iter_mut())
    {
        rewrite_structured_character_references(value, replacements);
    }
    rewrite_contract_state_names(&mut draft.current_contract, replacements);
    if let Some(candidate) = draft.pending_contract_candidate.as_mut() {
        if let Some(normalized) = candidate.get_mut("normalized") {
            rewrite_json_story_character_references(normalized, replacements, None);
        }
        if let Some(raw_preview) = candidate
            .get_mut("raw_preview")
            .and_then(|value| value.as_str())
        {
            let mut rewritten = raw_preview.to_string();
            rewrite_structured_character_references(&mut rewritten, replacements);
            candidate["raw_preview"] = Value::String(rewritten);
        }
    }
}

fn rewrite_contract_state_names(
    contract: &mut Option<Value>,
    replacements: &BTreeMap<String, String>,
) {
    if let Some(value) = contract.as_mut() {
        rewrite_json_story_character_references(value, replacements, None);
    }
}

fn rewrite_json_story_character_references(
    value: &mut Value,
    replacements: &BTreeMap<String, String>,
    field_name: Option<&str>,
) {
    match value {
        Value::String(text) => {
            if !matches!(
                field_name,
                Some("previous_names" | "previous names" | "历史姓名" | "旧名")
            ) {
                rewrite_structured_character_references(text, replacements);
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_json_story_character_references(item, replacements, field_name);
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                rewrite_json_story_character_references(item, replacements, Some(key));
            }
        }
        _ => {}
    }
}

fn rewrite_relationship_ledger_names(
    ledger: &mut [RelationshipLedgerEntry],
    replacements: &BTreeMap<String, String>,
) {
    if replacements.is_empty() {
        return;
    }
    for entry in ledger {
        for name in &mut entry.characters {
            if let Some(new_name) = replacements.get(name.trim()) {
                *name = new_name.clone();
            }
        }
        rewrite_text_with_name_replacements(&mut entry.arc_type, replacements);
        rewrite_text_with_name_replacements(&mut entry.relationship_type, replacements);
        rewrite_text_with_name_replacements(&mut entry.stage, replacements);
        rewrite_text_with_name_replacements(&mut entry.next_expected_stage, replacements);
        rewrite_text_with_name_replacements(&mut entry.start_state, replacements);
        rewrite_text_with_name_replacements(&mut entry.current_state, replacements);
        rewrite_text_with_name_replacements(&mut entry.desired_end_state, replacements);
        rewrite_text_with_name_replacements(&mut entry.evidence, replacements);
        for value in entry.conflicts.iter_mut().chain(entry.secrets.iter_mut()) {
            rewrite_text_with_name_replacements(value, replacements);
        }
        for value in &mut entry.turning_points {
            rewrite_text_with_name_replacements(value, replacements);
        }
        for transition in &mut entry.transition_history {
            rewrite_text_with_name_replacements(&mut transition.from_state, replacements);
            rewrite_text_with_name_replacements(&mut transition.to_state, replacements);
            rewrite_text_with_name_replacements(&mut transition.from_stage, replacements);
            rewrite_text_with_name_replacements(&mut transition.to_stage, replacements);
            rewrite_text_with_name_replacements(&mut transition.event, replacements);
            rewrite_text_with_name_replacements(&mut transition.evidence, replacements);
            rewrite_text_with_name_replacements(&mut transition.relationship_delta, replacements);
        }
    }
}

fn rewrite_emotional_state_ledger_names(
    ledger: &mut [EmotionalStateLedgerEntry],
    replacements: &BTreeMap<String, String>,
) {
    if replacements.is_empty() {
        return;
    }
    for entry in ledger {
        if let Some(new_name) = replacements.get(entry.character.trim()) {
            entry.character = new_name.clone();
        }
        rewrite_text_with_name_replacements(&mut entry.current_emotion, replacements);
        rewrite_text_with_name_replacements(&mut entry.pressure, replacements);
        rewrite_text_with_name_replacements(&mut entry.desire, replacements);
        rewrite_text_with_name_replacements(&mut entry.fear, replacements);
        rewrite_text_with_name_replacements(&mut entry.expected_next_shift, replacements);
        rewrite_text_with_name_replacements(&mut entry.payoff_target, replacements);
        for transition in &mut entry.transition_history {
            rewrite_text_with_name_replacements(&mut transition.from_emotion, replacements);
            rewrite_text_with_name_replacements(&mut transition.to_emotion, replacements);
            rewrite_text_with_name_replacements(&mut transition.trigger_event, replacements);
            rewrite_text_with_name_replacements(&mut transition.relationship_effect, replacements);
            rewrite_text_with_name_replacements(&mut transition.evidence, replacements);
        }
    }
}

pub(crate) fn rewrite_contract_v2_names(
    contract: &mut NovelContractV2,
    replacements: &BTreeMap<String, String>,
) {
    if replacements.is_empty() {
        return;
    }
    rewrite_resource_economy_names(&mut contract.resource_economy, replacements);
    rewrite_emotional_contract_names(&mut contract.emotional_contract, replacements);
    rewrite_relationship_ledger_names(&mut contract.relationship_ledger, replacements);
    rewrite_emotional_state_ledger_names(&mut contract.emotional_state_ledger, replacements);
    rewrite_power_progression_names(&mut contract.power_progression, replacements);
    rewrite_social_order_names(&mut contract.social_order, replacements);
    rewrite_geography_model_names(&mut contract.geography_model, replacements);
    rewrite_time_model_names(&mut contract.time_model, replacements);
    rewrite_artifact_ledger_names(&mut contract.artifact_ledger, replacements);
    rewrite_character_voice_ledger_names(&mut contract.character_voice_ledger, replacements);
    rewrite_relationship_quota_names(&mut contract.relationship_interaction_quotas, replacements);
    rewrite_antagonist_pressure_names(&mut contract.antagonist_pressure, replacements);
    rewrite_payoff_matrix_names(&mut contract.payoff_matrix, replacements);
    rewrite_reveal_schedule_names(&mut contract.reveal_schedule, replacements);
    rewrite_motif_ledger_names(&mut contract.motif_ledger, replacements);
    rewrite_conflict_pressure_curve_names(&mut contract.conflict_pressure_curve, replacements);
    rewrite_scene_type_mix_names(&mut contract.scene_type_mix, replacements);
    rewrite_narration_contract_names(&mut contract.narration_contract, replacements);
    rewrite_reader_promise_names(&mut contract.reader_promise, replacements);
    rewrite_chapter_ending_rotation_names(&mut contract.chapter_ending_rotation, replacements);
}

fn rewrite_resource_economy_names(
    economy: &mut ResourceEconomy,
    replacements: &BTreeMap<String, String>,
) {
    for value in [
        &mut economy.currency,
        &mut economy.value_scale,
        &mut economy.class_impact,
    ] {
        rewrite_text_with_name_replacements(value, replacements);
    }
    for value in economy
        .resource_types
        .iter_mut()
        .chain(economy.income_sources.iter_mut())
        .chain(economy.cost_examples.iter_mut())
        .chain(economy.scarcity_rules.iter_mut())
        .chain(economy.trade_rules.iter_mut())
    {
        rewrite_text_with_name_replacements(value, replacements);
    }
}

fn rewrite_emotional_contract_names(
    contract: &mut EmotionalContract,
    replacements: &BTreeMap<String, String>,
) {
    rewrite_text_with_name_replacements(&mut contract.primary_emotion, replacements);
    rewrite_text_with_name_replacements(&mut contract.emotional_promise, replacements);
    rewrite_text_with_name_replacements(&mut contract.ending_emotional_state, replacements);
    for value in contract
        .emotional_beats
        .iter_mut()
        .chain(contract.payoff_requirements.iter_mut())
        .chain(contract.relief_beats.iter_mut())
    {
        rewrite_text_with_name_replacements(value, replacements);
    }
}

fn rewrite_character_voice_ledger_names(
    ledger: &mut [CharacterVoiceProfile],
    replacements: &BTreeMap<String, String>,
) {
    for entry in ledger {
        if let Some(new_name) = replacements.get(entry.character.trim()) {
            entry.character = new_name.clone();
        }
        rewrite_text_with_name_replacements(&mut entry.voice_style, replacements);
        for value in entry
            .catchphrases
            .iter_mut()
            .chain(entry.forbidden_expressions.iter_mut())
            .chain(entry.dialogue_rules.iter_mut())
        {
            rewrite_text_with_name_replacements(value, replacements);
        }
    }
}

fn rewrite_power_progression_names(
    progression: &mut PowerProgression,
    replacements: &BTreeMap<String, String>,
) {
    rewrite_text_with_name_replacements(&mut progression.system_name, replacements);
    for value in progression
        .levels
        .iter_mut()
        .chain(progression.advancement_costs.iter_mut())
        .chain(progression.bottlenecks.iter_mut())
        .chain(progression.failure_consequences.iter_mut())
        .chain(progression.anti_power_creep_rules.iter_mut())
    {
        rewrite_text_with_name_replacements(value, replacements);
    }
    for state in &mut progression.character_current_levels {
        rewrite_text_with_name_replacements(&mut state.character, replacements);
        rewrite_text_with_name_replacements(&mut state.level, replacements);
        rewrite_text_with_name_replacements(&mut state.evidence, replacements);
    }
}

fn rewrite_social_order_names(order: &mut SocialOrder, replacements: &BTreeMap<String, String>) {
    for value in [&mut order.rank_system, &mut order.class_structure] {
        rewrite_text_with_name_replacements(value, replacements);
    }
    for value in order
        .institutions
        .iter_mut()
        .chain(order.exam_or_promotion_rules.iter_mut())
        .chain(order.laws.iter_mut())
        .chain(order.authority_conflicts.iter_mut())
    {
        rewrite_text_with_name_replacements(value, replacements);
    }
}

fn rewrite_geography_model_names(
    geography: &mut GeographyModel,
    replacements: &BTreeMap<String, String>,
) {
    for value in geography
        .regions
        .iter_mut()
        .chain(geography.distance_rules.iter_mut())
        .chain(geography.travel_constraints.iter_mut())
        .chain(geography.location_changes.iter_mut())
    {
        rewrite_text_with_name_replacements(value, replacements);
    }
    for location in &mut geography.important_locations {
        rewrite_text_with_name_replacements(&mut location.name, replacements);
        rewrite_text_with_name_replacements(&mut location.role, replacements);
        for value in &mut location.known_facts {
            rewrite_text_with_name_replacements(value, replacements);
        }
    }
}

fn rewrite_time_model_names(time: &mut TimeModel, replacements: &BTreeMap<String, String>) {
    for value in [
        &mut time.calendar,
        &mut time.story_start_time,
        &mut time.elapsed_time,
    ] {
        rewrite_text_with_name_replacements(value, replacements);
    }
    for value in time
        .deadline_events
        .iter_mut()
        .chain(time.time_skip_rules.iter_mut())
    {
        rewrite_text_with_name_replacements(value, replacements);
    }
    for state in &mut time.age_progression {
        rewrite_text_with_name_replacements(&mut state.character, replacements);
        rewrite_text_with_name_replacements(&mut state.start_age, replacements);
        rewrite_text_with_name_replacements(&mut state.current_age, replacements);
    }
}

fn rewrite_artifact_ledger_names(
    ledger: &mut [ArtifactLedgerEntry],
    replacements: &BTreeMap<String, String>,
) {
    for artifact in ledger {
        for value in [
            &mut artifact.name,
            &mut artifact.owner,
            &mut artifact.origin,
            &mut artifact.ability,
            &mut artifact.cost_or_limit,
            &mut artifact.status,
        ] {
            rewrite_text_with_name_replacements(value, replacements);
        }
    }
}

fn rewrite_relationship_quota_names(
    quotas: &mut [RelationshipInteractionQuota],
    replacements: &BTreeMap<String, String>,
) {
    for quota in quotas {
        for name in &mut quota.characters {
            if let Some(new_name) = replacements.get(name.trim()) {
                *name = new_name.clone();
            }
        }
        rewrite_text_with_name_replacements(&mut quota.relationship, replacements);
        rewrite_text_with_name_replacements(&mut quota.cadence, replacements);
        rewrite_text_with_name_replacements(&mut quota.required_interaction, replacements);
        rewrite_text_with_name_replacements(&mut quota.next_due, replacements);
    }
}

fn rewrite_antagonist_pressure_names(
    pressure: &mut AntagonistPressure,
    replacements: &BTreeMap<String, String>,
) {
    rewrite_text_with_name_replacements(&mut pressure.primary_pressure, replacements);
    for antagonist in &mut pressure.antagonists {
        rewrite_text_with_name_replacements(&mut antagonist.name, replacements);
        rewrite_text_with_name_replacements(&mut antagonist.goal, replacements);
        for value in &mut antagonist.resources {
            rewrite_text_with_name_replacements(value, replacements);
        }
        rewrite_text_with_name_replacements(&mut antagonist.current_move, replacements);
        rewrite_text_with_name_replacements(&mut antagonist.knowledge_state, replacements);
        rewrite_text_with_name_replacements(&mut antagonist.defeat_condition, replacements);
        for value in &mut antagonist.escalation_plan {
            rewrite_text_with_name_replacements(value, replacements);
        }
    }
}

fn rewrite_payoff_matrix_names(
    matrix: &mut [PayoffMatrixEntry],
    replacements: &BTreeMap<String, String>,
) {
    for entry in matrix {
        rewrite_text_with_name_replacements(&mut entry.promise, replacements);
        rewrite_text_with_name_replacements(&mut entry.payoff_target, replacements);
        rewrite_text_with_name_replacements(&mut entry.status, replacements);
        for value in &mut entry.evidence {
            rewrite_text_with_name_replacements(value, replacements);
        }
    }
}

fn rewrite_reveal_schedule_names(
    schedule: &mut [RevealScheduleEntry],
    replacements: &BTreeMap<String, String>,
) {
    for entry in schedule {
        rewrite_text_with_name_replacements(&mut entry.secret, replacements);
        rewrite_text_with_name_replacements(&mut entry.reader_knows, replacements);
        rewrite_text_with_name_replacements(&mut entry.protagonist_knows, replacements);
        rewrite_text_with_name_replacements(&mut entry.antagonist_knows, replacements);
        rewrite_text_with_name_replacements(&mut entry.reveal_window, replacements);
        rewrite_text_with_name_replacements(&mut entry.status, replacements);
    }
}

fn rewrite_motif_ledger_names(
    ledger: &mut [MotifLedgerEntry],
    replacements: &BTreeMap<String, String>,
) {
    for entry in ledger {
        rewrite_text_with_name_replacements(&mut entry.motif, replacements);
        rewrite_text_with_name_replacements(&mut entry.meaning, replacements);
        rewrite_text_with_name_replacements(&mut entry.payoff_target, replacements);
        for value in &mut entry.evolution {
            rewrite_text_with_name_replacements(value, replacements);
        }
    }
}

fn rewrite_conflict_pressure_curve_names(
    curve: &mut ConflictPressureCurve,
    replacements: &BTreeMap<String, String>,
) {
    rewrite_text_with_name_replacements(&mut curve.peak_policy, replacements);
    rewrite_text_with_name_replacements(&mut curve.release_strategy, replacements);
    for beat in &mut curve.global_curve {
        rewrite_text_with_name_replacements(&mut beat.range, replacements);
        rewrite_text_with_name_replacements(&mut beat.pressure_level, replacements);
        rewrite_text_with_name_replacements(&mut beat.function, replacements);
    }
}

fn rewrite_scene_type_mix_names(scene: &mut SceneTypeMix, replacements: &BTreeMap<String, String>) {
    rewrite_text_with_name_replacements(&mut scene.action, replacements);
    rewrite_text_with_name_replacements(&mut scene.dialogue, replacements);
    rewrite_text_with_name_replacements(&mut scene.everyday, replacements);
    rewrite_text_with_name_replacements(&mut scene.reveal, replacements);
    rewrite_text_with_name_replacements(&mut scene.emotional, replacements);
    rewrite_text_with_name_replacements(&mut scene.turning_point, replacements);
    rewrite_text_with_name_replacements(&mut scene.balance_rule, replacements);
}

fn rewrite_narration_contract_names(
    contract: &mut NarrationContract,
    replacements: &BTreeMap<String, String>,
) {
    rewrite_text_with_name_replacements(&mut contract.pov, replacements);
    rewrite_text_with_name_replacements(&mut contract.narrative_distance, replacements);
    rewrite_text_with_name_replacements(&mut contract.dialogue_style, replacements);
    rewrite_text_with_name_replacements(&mut contract.chapter_pacing, replacements);
    rewrite_text_with_name_replacements(&mut contract.description_density, replacements);
    for value in &mut contract.forbidden_style_drift {
        rewrite_text_with_name_replacements(value, replacements);
    }
}

fn rewrite_reader_promise_names(
    promise: &mut ReaderPromise,
    replacements: &BTreeMap<String, String>,
) {
    rewrite_text_with_name_replacements(&mut promise.core_hook, replacements);
    rewrite_text_with_name_replacements(&mut promise.curiosity_engine, replacements);
    rewrite_text_with_name_replacements(&mut promise.payoff_style, replacements);
    for value in &mut promise.pleasure_points {
        rewrite_text_with_name_replacements(value, replacements);
    }
}

fn rewrite_chapter_ending_rotation_names(
    rotation: &mut ChapterEndingRotation,
    replacements: &BTreeMap<String, String>,
) {
    rewrite_text_with_name_replacements(&mut rotation.avoid_repetition_rule, replacements);
    for value in &mut rotation.planned_rotation {
        rewrite_text_with_name_replacements(value, replacements);
    }
}

fn rewrite_text_with_name_replacements(
    value: &mut String,
    replacements: &BTreeMap<String, String>,
) {
    rewrite_structured_character_references(value, replacements);
}

fn rewrite_structured_character_references(
    value: &mut String,
    replacements: &BTreeMap<String, String>,
) {
    let mut ordered_replacements = replacements.iter().collect::<Vec<_>>();
    ordered_replacements.sort_by(|(left, _), (right, _)| {
        right
            .chars()
            .count()
            .cmp(&left.chars().count())
            .then_with(|| left.cmp(right))
    });
    for (old_name, new_name) in ordered_replacements {
        if value.contains(old_name) {
            let mut rewritten =
                crate::tool::writing::typed_contract_gate::replace_character_anchor_reference(
                    value, old_name, new_name,
                );
            if rewritten != *value {
                rewrite_co_referential_family_name(&mut rewritten, old_name, new_name);
                *value = rewritten;
            }
        }
    }
}

fn rewrite_co_referential_family_name(value: &mut String, old_name: &str, new_name: &str) {
    let Some(old_surname) = naming::cjk_character_surname(old_name) else {
        return;
    };
    let Some(new_surname) = naming::cjk_character_surname(new_name) else {
        return;
    };
    if old_surname == new_surname {
        return;
    }
    for suffix in ["家", "府", "父", "母", "氏"] {
        let old_reference = format!("{old_surname}{suffix}");
        if value.contains(&old_reference) {
            *value = value.replace(&old_reference, &format!("{new_surname}{suffix}"));
        }
    }
}

fn rewrite_unambiguous_household_references_to_authority(
    value: &mut String,
    authority: &CharacterAuthority,
) {
    let Some(primary) = authority.primary.as_deref() else {
        return;
    };
    let mut targets_by_superseded_surname = BTreeMap::<String, BTreeSet<String>>::new();
    for (old_name, new_name) in &authority.superseded_names {
        let (Some(old_surname), Some(new_surname)) = (
            naming::cjk_character_surname(old_name),
            naming::cjk_character_surname(new_name),
        ) else {
            continue;
        };
        if old_surname != new_surname {
            targets_by_superseded_surname
                .entry(old_surname.to_string())
                .or_default()
                .insert(new_name.to_string());
        }
    }
    for (old_surname, targets) in targets_by_superseded_surname {
        let target = if targets.len() == 1 {
            targets.iter().next().map(String::as_str)
        } else if targets.contains(primary) {
            // A bare household anchor has no local character subject. When a
            // superseded surname belonged to several model candidates, the
            // contract's primary-character authority is the only deterministic
            // owner. Explicit "old full name + household" references have
            // already been handled by rewrite_co_referential_family_name above.
            Some(primary)
        } else {
            None
        };
        let Some(new_surname) = target.and_then(naming::cjk_character_surname) else {
            continue;
        };
        for suffix in ["家", "府", "父", "母", "氏"] {
            let old_reference = format!("{old_surname}{suffix}");
            if value.contains(&old_reference) {
                *value = value.replace(&old_reference, &format!("{new_surname}{suffix}"));
            }
        }
    }
}

fn canonicalize_draft_story_surface_to_authority(
    draft: &mut SessionCreationDraftState,
    authority: &CharacterAuthority,
) {
    if authority.default_character().is_none() {
        return;
    }
    for value in [
        &mut draft.brief,
        &mut draft.fiction_premise,
        &mut draft.fiction_ending_direction,
        &mut draft.fiction_protagonist_arc,
        &mut draft.fiction_title_rationale,
        &mut draft.fiction_main_causal_spine,
        &mut draft.fiction_outline,
    ] {
        rewrite_external_character_references_to_authority(value, authority);
    }
}

pub(crate) fn canonicalize_draft_story_surfaces_to_character_lines(
    draft: &mut SessionCreationDraftState,
    character_lines: &[String],
) {
    let authority = CharacterAuthority::from_lines(character_lines);
    if authority.names.is_empty() {
        return;
    }
    canonicalize_draft_story_surface_to_authority(draft, &authority);
    let mut contract_v2 = draft.contract_v2();
    canonicalize_contract_v2_to_authority(&mut contract_v2, &authority);
    draft.set_contract_v2(contract_v2);
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PlotPatch {
    pub(crate) volumes: Vec<VolumeContract>,
    pub(crate) near_chapters: Vec<ChapterSeedContract>,
    pub(crate) raw_outline: String,
    pub(crate) payoff_matrix: Vec<PayoffMatrixEntry>,
}

impl PlotPatch {
    fn apply_to_draft(&self, draft: &mut SessionCreationDraftState) {
        let mut outline = Vec::new();
        let raw_summary =
            patch_normalizer::strip_plot_control_segments_from_outline_text(&self.raw_outline);
        if !value_missing(&raw_summary) {
            outline.push(raw_summary);
        }
        for (index, volume) in self.volumes.iter().enumerate() {
            let mut line = format!("第{}卷", index + 1);
            if !value_missing(&volume.title) {
                line.push_str(&format!("《{}》", volume.title.trim()));
            }
            if !value_missing(&volume.objective) {
                line.push_str(&format!("：{}", volume.objective.trim()));
            }
            if !value_missing(&volume.ending_change) {
                line.push_str(&format!("；卷尾变化：{}", volume.ending_change.trim()));
            }
            outline.push(line);
        }
        for chapter in &self.near_chapters {
            let number = chapter.number.unwrap_or_else(|| outline.len() + 1);
            let mut line = format!("第{number}章");
            if !value_missing(&chapter.goal) {
                line.push_str(&format!(" 本章目标：{}", chapter.goal.trim()));
            }
            if !value_missing(&chapter.expected_turn) {
                line.push_str(&format!("；预期转折：{}", chapter.expected_turn.trim()));
            }
            outline.push(line);
        }
        if !outline.is_empty() {
            draft.fiction_outline = outline.join("\n");
        }
        if !self.payoff_matrix.is_empty() {
            draft.payoff_matrix = self.payoff_matrix.clone();
        }
        canonicalize_plot_patch_to_character_authority(draft);
    }
}

fn canonicalize_plot_patch_to_character_authority(draft: &mut SessionCreationDraftState) {
    let authority = CharacterAuthority::from_lines(&draft.fiction_characters);
    if authority.names.is_empty() {
        return;
    }
    canonicalize_draft_story_surface_to_authority(draft, &authority);
    canonicalize_payoff_matrix_to_authority(&mut draft.payoff_matrix, &authority);
    let mut contract = draft.contract_v2();
    canonicalize_contract_v2_to_authority(&mut contract, &authority);
    draft.set_contract_v2(contract);
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GovernancePatch {
    pub(crate) themes: Vec<String>,
    pub(crate) world_rules: Vec<String>,
    pub(crate) style_rules: Vec<String>,
    pub(crate) must_avoid: Vec<String>,
    pub(crate) emotional_contract: EmotionalContract,
    pub(crate) relationship_ledger: Vec<RelationshipLedgerEntry>,
    pub(crate) antagonist_pressure: AntagonistPressure,
    pub(crate) narration_contract: NarrationContract,
    pub(crate) structured: NovelContractV2,
}

impl GovernancePatch {
    fn apply_to_draft(&self, draft: &mut SessionCreationDraftState) {
        let visible = visible_governance_fields_from_patch(self);
        if !self.themes.is_empty() {
            draft.fiction_themes = self.themes.clone();
        } else if !visible.themes.is_empty() {
            draft.fiction_themes = visible.themes.clone();
        }
        if !self.world_rules.is_empty() {
            draft.fiction_world_rules = self.world_rules.clone();
        } else if !visible.world_rules.is_empty() {
            draft.fiction_world_rules = visible.world_rules.clone();
        }
        if !self.style_rules.is_empty() {
            draft.fiction_style_rules = self.style_rules.clone();
        } else if !visible.style_rules.is_empty() {
            draft.fiction_style_rules = visible.style_rules.clone();
        }
        if !self.must_avoid.is_empty() {
            draft.fiction_must_avoid = self.must_avoid.clone();
        } else if !visible.must_avoid.is_empty() {
            draft.fiction_must_avoid = visible.must_avoid.clone();
        }
        let mut contract = draft.contract_v2();
        if !value_missing(&self.emotional_contract.primary_emotion)
            || !value_missing(&self.emotional_contract.emotional_promise)
            || !self.emotional_contract.emotional_beats.is_empty()
            || !self.emotional_contract.relief_beats.is_empty()
        {
            contract.emotional_contract = self.emotional_contract.clone();
        }
        if !self.relationship_ledger.is_empty() {
            contract.relationship_ledger = self.relationship_ledger.clone();
        }
        if !value_missing(&self.antagonist_pressure.primary_pressure)
            || !self.antagonist_pressure.antagonists.is_empty()
        {
            contract.antagonist_pressure = self.antagonist_pressure.clone();
        }
        if !value_missing(&self.narration_contract.pov)
            || !value_missing(&self.narration_contract.dialogue_style)
        {
            contract.narration_contract = self.narration_contract.clone();
        }
        merge_non_empty_contract_v2(&mut contract, &self.structured);
        let authority = CharacterAuthority::from_lines(&draft.fiction_characters);
        canonicalize_contract_v2_to_authority(&mut contract, &authority);
        draft.set_contract_v2(contract);
        let normalized_visible = visible_governance_fields_from_contract_v2(&draft.contract_v2());
        if draft.fiction_world_rules.is_empty() && !normalized_visible.world_rules.is_empty() {
            draft.fiction_world_rules = normalized_visible.world_rules;
        }
    }
}

#[derive(Default)]
pub(crate) struct VisibleGovernanceFields {
    pub(crate) themes: Vec<String>,
    pub(crate) world_rules: Vec<String>,
    pub(crate) style_rules: Vec<String>,
    pub(crate) must_avoid: Vec<String>,
}

fn visible_governance_fields_from_patch(patch: &GovernancePatch) -> VisibleGovernanceFields {
    let mut visible = VisibleGovernanceFields::default();
    visible.themes = merge_unique_strings(&visible.themes, &patch.themes);
    visible.world_rules = merge_unique_strings(&visible.world_rules, &patch.world_rules);
    visible.style_rules = merge_unique_strings(&visible.style_rules, &patch.style_rules);
    visible.must_avoid = merge_unique_strings(&visible.must_avoid, &patch.must_avoid);

    visible.themes = merge_unique_strings(
        &visible.themes,
        &non_empty_list_from_value(&patch.emotional_contract.emotional_promise),
    );
    visible.themes =
        merge_unique_strings(&visible.themes, &patch.emotional_contract.emotional_beats);
    visible.style_rules =
        merge_unique_strings(&visible.style_rules, &patch.emotional_contract.relief_beats);
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &non_empty_list_from_value(&patch.narration_contract.pov),
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &non_empty_list_from_value(&patch.narration_contract.dialogue_style),
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &non_empty_list_from_value(&patch.narration_contract.chapter_pacing),
    );

    visible = merge_visible_governance_fields(
        visible,
        visible_governance_fields_from_contract_v2(&patch.structured),
    );
    visible
}

pub(crate) fn visible_governance_fields_from_contract_v2(
    structured: &NovelContractV2,
) -> VisibleGovernanceFields {
    let mut visible = VisibleGovernanceFields::default();
    visible.themes = merge_unique_strings(
        &visible.themes,
        &non_empty_list_from_value(&structured.emotional_contract.emotional_promise),
    );
    visible.themes = merge_unique_strings(
        &visible.themes,
        &structured.emotional_contract.emotional_beats,
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &structured.emotional_contract.relief_beats,
    );
    visible.themes = merge_unique_strings(
        &visible.themes,
        &non_empty_list_from_value(&structured.reader_promise.core_hook),
    );
    visible.themes =
        merge_unique_strings(&visible.themes, &structured.reader_promise.pleasure_points);
    visible.themes = merge_unique_strings(
        &visible.themes,
        &non_empty_list_from_value(&structured.reader_promise.curiosity_engine),
    );
    visible.themes = merge_unique_strings(
        &visible.themes,
        &structured
            .motif_ledger
            .iter()
            .flat_map(|motif| {
                [
                    motif.motif.trim().to_string(),
                    motif.meaning.trim().to_string(),
                    motif.payoff_target.trim().to_string(),
                ]
            })
            .collect::<Vec<_>>(),
    );
    visible.world_rules = merge_unique_strings(
        &visible.world_rules,
        &structured.power_progression.advancement_costs,
    );
    visible.world_rules = merge_unique_strings(
        &visible.world_rules,
        &structured.power_progression.bottlenecks,
    );
    visible.world_rules = merge_unique_strings(
        &visible.world_rules,
        &structured.power_progression.failure_consequences,
    );
    visible.world_rules = merge_unique_strings(
        &visible.world_rules,
        &structured.power_progression.anti_power_creep_rules,
    );
    visible.world_rules =
        merge_unique_strings(&visible.world_rules, &structured.social_order.institutions);
    visible.world_rules = merge_unique_strings(&visible.world_rules, &structured.social_order.laws);
    visible.world_rules = merge_unique_strings(
        &visible.world_rules,
        &structured.resource_economy.cost_examples,
    );
    visible.world_rules = merge_unique_strings(
        &visible.world_rules,
        &structured.resource_economy.scarcity_rules,
    );
    visible.world_rules = merge_unique_strings(
        &visible.world_rules,
        &structured.resource_economy.trade_rules,
    );
    visible.world_rules = merge_unique_strings(
        &visible.world_rules,
        &structured.geography_model.distance_rules,
    );
    visible.world_rules = merge_unique_strings(
        &visible.world_rules,
        &structured.geography_model.travel_constraints,
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &non_empty_list_from_value(&structured.narration_contract.pov),
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &non_empty_list_from_value(&structured.narration_contract.dialogue_style),
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &non_empty_list_from_value(&structured.narration_contract.chapter_pacing),
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &non_empty_list_from_value(&structured.scene_type_mix.balance_rule),
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &structured.chapter_ending_rotation.planned_rotation,
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &non_empty_list_from_value(&structured.chapter_ending_rotation.avoid_repetition_rule),
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &structured
            .character_voice_ledger
            .iter()
            .flat_map(|voice| {
                let mut items = Vec::new();
                if !value_missing(&voice.voice_style) {
                    items.push(format!(
                        "{}：{}",
                        if value_missing(&voice.character) {
                            "角色声音"
                        } else {
                            voice.character.trim()
                        },
                        voice.voice_style.trim()
                    ));
                }
                items.extend(voice.dialogue_rules.iter().cloned());
                items
            })
            .collect::<Vec<_>>(),
    );
    visible.style_rules = merge_unique_strings(
        &visible.style_rules,
        &structured
            .relationship_interaction_quotas
            .iter()
            .map(|quota| {
                first_non_empty_string(&[
                    quota.required_interaction.as_str(),
                    quota.cadence.as_str(),
                    quota.relationship.as_str(),
                ])
            })
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>(),
    );
    visible.must_avoid = merge_unique_strings(
        &visible.must_avoid,
        &structured.narration_contract.forbidden_style_drift,
    );
    visible.must_avoid = merge_unique_strings(
        &visible.must_avoid,
        &structured.power_progression.anti_power_creep_rules,
    );
    visible
}

fn merge_visible_governance_fields(
    mut base: VisibleGovernanceFields,
    incoming: VisibleGovernanceFields,
) -> VisibleGovernanceFields {
    base.themes = merge_unique_strings(&base.themes, &incoming.themes);
    base.world_rules = merge_unique_strings(&base.world_rules, &incoming.world_rules);
    base.style_rules = merge_unique_strings(&base.style_rules, &incoming.style_rules);
    base.must_avoid = merge_unique_strings(&base.must_avoid, &incoming.must_avoid);
    base
}

fn merge_unique_strings(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut out = existing
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value_missing(value))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    for value in incoming {
        let value = value.trim();
        if value_missing(value) {
            continue;
        }
        if !out.iter().any(|known| known == value) {
            out.push(value.to_string());
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MetadataPatch {
    pub(crate) title: TitlePatch,
    pub(crate) world_rules: Vec<String>,
    pub(crate) volumes: Vec<VolumeContract>,
    pub(crate) near_chapters: Vec<ChapterSeedContract>,
}

impl MetadataPatch {
    fn apply_to_draft(&self, draft: &mut SessionCreationDraftState) {
        if self.title.has_valid_provided_title_for_draft(draft) {
            self.title.apply_metadata_repair_to_draft(draft);
        }
        if !self.world_rules.is_empty() {
            draft.fiction_world_rules = self.world_rules.clone();
        }
        if !self.volumes.is_empty() || !self.near_chapters.is_empty() {
            PlotPatch {
                volumes: self.volumes.clone(),
                near_chapters: self.near_chapters.clone(),
                raw_outline: String::new(),
                payoff_matrix: Vec::new(),
            }
            .apply_to_draft(draft);
        }
    }
}

fn merge_non_empty_contract_v2(target: &mut NovelContractV2, incoming: &NovelContractV2) {
    if !incoming.field_requirements.is_empty() {
        target
            .field_requirements
            .extend(incoming.field_requirements.clone());
    }
    if !value_missing(&incoming.resource_economy.currency)
        || !incoming.resource_economy.resource_types.is_empty()
    {
        target.resource_economy = incoming.resource_economy.clone();
    }
    if !incoming.emotional_state_ledger.is_empty() {
        target.emotional_state_ledger = incoming.emotional_state_ledger.clone();
    }
    if !incoming.power_progression.levels.is_empty()
        || !value_missing(&incoming.power_progression.system_name)
    {
        target.power_progression = incoming.power_progression.clone();
    }
    if !incoming.social_order.institutions.is_empty()
        || !value_missing(&incoming.social_order.rank_system)
    {
        target.social_order = incoming.social_order.clone();
    }
    if !incoming.geography_model.important_locations.is_empty() {
        target.geography_model = incoming.geography_model.clone();
    }
    if !incoming.time_model.deadline_events.is_empty()
        || !value_missing(&incoming.time_model.story_start_time)
    {
        target.time_model = incoming.time_model.clone();
    }
    if !incoming.artifact_ledger.is_empty() {
        target.artifact_ledger = incoming.artifact_ledger.clone();
    }
    if emotional_contract_has_content(&incoming.emotional_contract) {
        target.emotional_contract = incoming.emotional_contract.clone();
    }
    if !incoming.relationship_ledger.is_empty() {
        target.relationship_ledger = incoming.relationship_ledger.clone();
    }
    if antagonist_pressure_has_content(&incoming.antagonist_pressure) {
        target.antagonist_pressure = incoming.antagonist_pressure.clone();
    }
    if !incoming.payoff_matrix.is_empty() {
        target.payoff_matrix = incoming.payoff_matrix.clone();
    }
    if narration_contract_has_content(&incoming.narration_contract) {
        target.narration_contract = incoming.narration_contract.clone();
    }
    if scene_type_mix_has_content(&incoming.scene_type_mix) {
        target.scene_type_mix = incoming.scene_type_mix.clone();
    }
    if !incoming.character_voice_ledger.is_empty() {
        target.character_voice_ledger = incoming.character_voice_ledger.clone();
    }
    if reader_promise_has_content(&incoming.reader_promise) {
        target.reader_promise = incoming.reader_promise.clone();
    }
    if chapter_ending_rotation_has_content(&incoming.chapter_ending_rotation) {
        target.chapter_ending_rotation = incoming.chapter_ending_rotation.clone();
    }
    if conflict_pressure_curve_has_content(&incoming.conflict_pressure_curve) {
        target.conflict_pressure_curve = incoming.conflict_pressure_curve.clone();
    }
    if !incoming.motif_ledger.is_empty() {
        target.motif_ledger = incoming.motif_ledger.clone();
    }
    if !incoming.reveal_schedule.is_empty() {
        target.reveal_schedule = incoming.reveal_schedule.clone();
    }
    if !incoming.relationship_interaction_quotas.is_empty() {
        target.relationship_interaction_quotas = incoming.relationship_interaction_quotas.clone();
    }
}

#[derive(Debug, Clone, Default)]
struct CharacterAuthority {
    names: Vec<String>,
    superseded_names: BTreeMap<String, String>,
    primary: Option<String>,
    male_primary: Option<String>,
    female_primary: Option<String>,
    secondary: Option<String>,
    pressure_source: Option<String>,
}

impl CharacterAuthority {
    fn from_lines(lines: &[String]) -> Self {
        let characters = lines
            .iter()
            .map(|line| super::draft_character_line_to_contract(line))
            .filter(|character| !value_missing(&character.canonical_name))
            .collect::<Vec<_>>();
        let names = characters
            .iter()
            .map(|character| character.canonical_name.trim().to_string())
            .collect::<Vec<_>>();
        let superseded_name_counts = characters
            .iter()
            .flat_map(|character| character.previous_names.iter())
            .map(|name| name.trim())
            .filter(|name| !value_missing(name))
            .fold(BTreeMap::<String, usize>::new(), |mut counts, name| {
                *counts.entry(name.to_string()).or_default() += 1;
                counts
            });
        let superseded_names = characters
            .iter()
            .flat_map(|character| {
                character.previous_names.iter().filter_map(|previous_name| {
                    let previous_name = previous_name.trim();
                    let canonical_name = character.canonical_name.trim();
                    (!value_missing(previous_name)
                        && previous_name != canonical_name
                        && superseded_name_counts
                            .get(previous_name)
                            .copied()
                            .unwrap_or_default()
                            == 1)
                        .then(|| (previous_name.to_string(), canonical_name.to_string()))
                })
            })
            .collect::<BTreeMap<_, _>>();
        let primary = characters
            .iter()
            .find(|character| character.role_looks_primary())
            .map(|character| character.canonical_name.trim().to_string());
        let male_primary = characters
            .iter()
            .find(|character| character.role.contains("男主"))
            .map(|character| character.canonical_name.trim().to_string());
        let female_primary = characters
            .iter()
            .find(|character| character.role.contains("女主"))
            .map(|character| character.canonical_name.trim().to_string());
        let secondary = characters
            .iter()
            .find(|character| !character.role_looks_primary())
            .map(|character| character.canonical_name.trim().to_string())
            .or_else(|| {
                characters
                    .iter()
                    .find(|character| Some(character.canonical_name.trim()) != primary.as_deref())
                    .map(|character| character.canonical_name.trim().to_string())
            });
        let pressure_source = characters
            .iter()
            .find(|character| character.role_looks_like_pressure_source())
            .map(|character| character.canonical_name.trim().to_string())
            .or_else(|| {
                characters
                    .iter()
                    .find(|character| {
                        !character.role_looks_primary()
                            && Some(character.canonical_name.trim()) != secondary.as_deref()
                    })
                    .map(|character| character.canonical_name.trim().to_string())
            });
        Self {
            names,
            superseded_names,
            primary,
            male_primary,
            female_primary,
            secondary,
            pressure_source,
        }
    }

    fn contains(&self, name: &str) -> bool {
        let name = name.trim();
        !value_missing(name) && self.names.iter().any(|known| known == name)
    }

    fn relationship_pair(&self) -> Vec<String> {
        let mut pair = Vec::new();
        if let Some(primary) = &self.primary {
            pair.push(primary.clone());
        }
        if let Some(secondary) = &self.secondary {
            if !pair.iter().any(|existing| existing == secondary) {
                pair.push(secondary.clone());
            }
        }
        for name in &self.names {
            if pair.len() >= 2 {
                break;
            }
            if !pair.iter().any(|existing| existing == name) {
                pair.push(name.clone());
            }
        }
        pair
    }

    fn default_character(&self) -> Option<String> {
        self.primary.clone().or_else(|| self.names.first().cloned())
    }

    fn pressure_source_character(&self) -> Option<String> {
        self.pressure_source
            .clone()
            .or_else(|| self.secondary.clone())
            .or_else(|| {
                self.names
                    .iter()
                    .find(|name| Some(*name) != self.primary.as_ref())
                    .cloned()
            })
            .or_else(|| self.default_character())
    }

    fn reference_matches_primary(&self, reference: &str) -> bool {
        let Some(primary) = self.primary.as_deref() else {
            return false;
        };
        crate::tool::writing::typed_contract_gate::character_anchor_person_references(reference)
            .iter()
            .any(|candidate| candidate == primary)
            || reference.trim() == primary
            || reference.trim().strip_prefix(primary).is_some_and(|tail| {
                let tail_len = tail.chars().count();
                (1..=2).contains(&tail_len)
                    && tail
                        .chars()
                        .all(external_character_reference_trailing_action_noise)
            })
    }
}

fn canonicalize_character_anchor_lines_to_authority(
    lines: &mut [String],
    authority: &CharacterAuthority,
) {
    if authority.names.is_empty() {
        return;
    }
    for line in lines {
        let mut character = super::draft_character_line_to_contract(line);
        if value_missing(&character.canonical_name) {
            continue;
        }
        rewrite_known_stale_character_references_to_authority(&mut character.desire, authority);
        rewrite_known_stale_character_references_to_authority(&mut character.fear, authority);
        rewrite_known_stale_character_references_to_authority(
            &mut character.bottom_line,
            authority,
        );
        rewrite_known_stale_character_references_to_authority(&mut character.arc_start, authority);
        rewrite_known_stale_character_references_to_authority(&mut character.arc_end, authority);
        rewrite_known_stale_character_references_to_authority(
            &mut character.planned_entry,
            authority,
        );
        rewrite_known_stale_character_references_to_authority(
            &mut character.planned_exit,
            authority,
        );
        *line = character.to_draft_line();
    }
}

fn canonicalize_relationship_ledger_to_authority(
    ledger: &mut [RelationshipLedgerEntry],
    authority: &CharacterAuthority,
) {
    if authority.names.is_empty() {
        return;
    }
    let pair = authority.relationship_pair();
    for entry in ledger {
        if entry.characters.is_empty() {
            entry.characters = pair.clone();
        } else {
            let mut normalized = Vec::new();
            for name in &entry.characters {
                let name = relationship_participant_authority_name(name, authority)
                    .unwrap_or_else(|| name.trim().to_string());
                if !value_missing(&name) && !normalized.iter().any(|existing| existing == &name) {
                    normalized.push(name);
                }
            }
            entry.characters = normalized;
        }
        for value in [
            &mut entry.arc_type,
            &mut entry.relationship_type,
            &mut entry.stage,
            &mut entry.next_expected_stage,
            &mut entry.start_state,
            &mut entry.current_state,
            &mut entry.desired_end_state,
            &mut entry.evidence,
        ] {
            rewrite_contract_state_character_references_to_authority(value, authority);
        }
        for value in entry.conflicts.iter_mut().chain(entry.secrets.iter_mut()) {
            rewrite_contract_state_character_references_to_authority(value, authority);
        }
        for point in &mut entry.turning_points {
            rewrite_contract_state_character_references_to_authority(point, authority);
        }
        for transition in &mut entry.transition_history {
            rewrite_contract_state_character_references_to_authority(
                &mut transition.from_state,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.to_state,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.from_stage,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.to_stage,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.event,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.relationship_delta,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.evidence,
                authority,
            );
        }
    }
}

fn canonicalize_emotional_state_ledger_to_authority(
    ledger: &mut [EmotionalStateLedgerEntry],
    authority: &CharacterAuthority,
) {
    let Some(default_character) = authority.default_character() else {
        return;
    };
    for entry in ledger {
        if !authority.contains(&entry.character) {
            entry.character = default_character.clone();
        }
        for value in [
            &mut entry.current_emotion,
            &mut entry.pressure,
            &mut entry.desire,
            &mut entry.fear,
            &mut entry.expected_next_shift,
            &mut entry.payoff_target,
        ] {
            rewrite_contract_state_character_references_to_authority(value, authority);
        }
        for transition in &mut entry.transition_history {
            rewrite_contract_state_character_references_to_authority(
                &mut transition.from_emotion,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.to_emotion,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.trigger_event,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.relationship_effect,
                authority,
            );
            rewrite_contract_state_character_references_to_authority(
                &mut transition.evidence,
                authority,
            );
        }
    }
}

fn canonicalize_character_voice_ledger_to_authority(
    ledger: &mut [CharacterVoiceProfile],
    authority: &CharacterAuthority,
) {
    let Some(default_character) = authority.default_character() else {
        return;
    };
    for entry in ledger {
        if !authority.contains(&entry.character) {
            entry.character = character_voice_entry_owner_from_authority(entry, authority)
                .unwrap_or_else(|| default_character.clone());
        }
        rewrite_external_character_references_to_authority(&mut entry.voice_style, authority);
        for value in entry
            .catchphrases
            .iter_mut()
            .chain(entry.forbidden_expressions.iter_mut())
            .chain(entry.dialogue_rules.iter_mut())
        {
            rewrite_external_character_references_to_authority(value, authority);
        }
    }
}

fn relationship_participant_authority_name(
    name: &str,
    authority: &CharacterAuthority,
) -> Option<String> {
    let name = name.trim();
    if value_missing(name) {
        return None;
    }
    if authority.contains(name) {
        return Some(name.to_string());
    }
    authority.names.iter().find_map(|known| {
        let known = known.trim();
        let tail = name.strip_prefix(known)?;
        let tail_len = tail.chars().count();
        (1..=2).contains(&tail_len).then(|| known.to_string())
    })
}

fn character_voice_entry_owner_from_authority(
    entry: &CharacterVoiceProfile,
    authority: &CharacterAuthority,
) -> Option<String> {
    let text = std::iter::once(entry.voice_style.as_str())
        .chain(entry.catchphrases.iter().map(String::as_str))
        .chain(entry.forbidden_expressions.iter().map(String::as_str))
        .chain(entry.dialogue_rules.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    authority
        .names
        .iter()
        .find(|name| !name.trim().is_empty() && text.contains(name.as_str()))
        .cloned()
}

fn rewrite_external_character_references_to_authority(
    value: &mut String,
    authority: &CharacterAuthority,
) {
    if value_missing(value) {
        return;
    }
    rewrite_structured_character_references(value, &authority.superseded_names);
    rewrite_unambiguous_household_references_to_authority(value, authority);
    rewrite_primary_role_references_to_authority(value, authority);
}

fn rewrite_contract_state_character_references_to_authority(
    value: &mut String,
    authority: &CharacterAuthority,
) {
    rewrite_known_stale_character_references_to_authority(value, authority);
}

fn rewrite_known_stale_character_references_to_authority(
    value: &mut String,
    authority: &CharacterAuthority,
) {
    if value_missing(value) {
        return;
    }
    rewrite_structured_character_references(value, &authority.superseded_names);
    rewrite_unambiguous_household_references_to_authority(value, authority);
    rewrite_primary_role_references_to_authority(value, authority);
}

fn rewrite_primary_role_references_to_authority(
    value: &mut String,
    authority: &CharacterAuthority,
) {
    let Some(primary) = authority.primary.as_deref() else {
        return;
    };
    if value_missing(value) || value_missing(primary) {
        return;
    }
    rewrite_specific_primary_role_marker(
        value,
        "男主",
        authority.male_primary.as_deref(),
        authority,
    );
    rewrite_specific_primary_role_marker(
        value,
        "女主",
        authority.female_primary.as_deref(),
        authority,
    );
    rewrite_primary_role_marker_known_authority_names(value, authority, primary);
    let references =
        crate::tool::writing::typed_contract_gate::primary_role_person_references(value);
    for reference in references {
        let surface = external_character_reference_rewrite_surface(&reference);
        if authority.contains(&surface)
            || authority.reference_matches_primary(&surface)
            || crate::tool::writing::typed_contract_gate::reference_looks_like_collective_or_organization(
                &surface,
            )
            || !value.contains(&surface)
        {
            continue;
        }
        *value = crate::tool::writing::typed_contract_gate::replace_character_anchor_reference(
            value, &surface, primary,
        );
    }
}

fn rewrite_specific_primary_role_marker(
    value: &mut String,
    marker: &str,
    target: Option<&str>,
    authority: &CharacterAuthority,
) {
    let Some(target) = target else {
        return;
    };
    for reference in
        crate::tool::writing::typed_contract_gate::marked_primary_role_person_references(
            value, marker,
        )
    {
        let surface = external_character_reference_rewrite_surface(&reference);
        if surface == target
            || crate::tool::writing::typed_contract_gate::reference_looks_like_collective_or_organization(
                &surface,
            )
            || !value.contains(&surface)
        {
            continue;
        }
        *value = crate::tool::writing::typed_contract_gate::replace_character_anchor_reference(
            value, &surface, target,
        );
    }
    // Keep the generic primary pass from remapping a correctly resolved
    // male/female lead back to the first primary slot.
    debug_assert!(authority.contains(target));
}

fn rewrite_primary_role_marker_known_authority_names(
    value: &mut String,
    authority: &CharacterAuthority,
    primary: &str,
) {
    for marker in ["主角", "主人公"] {
        for name in &authority.names {
            let name = name.trim();
            if name.is_empty() || name == primary {
                continue;
            }
            for connector in ["", "：", ":", "是", "的"] {
                let surface = format!("{marker}{connector}{name}");
                if value.contains(&surface) {
                    *value = value.replace(&surface, &format!("{marker}{connector}{primary}"));
                }
            }
        }
    }
}

fn external_character_reference_rewrite_surface(reference: &str) -> String {
    let mut surface = reference.trim().to_string();
    while surface.chars().count() > 2
        && surface
            .chars()
            .last()
            .is_some_and(external_character_reference_trailing_action_noise)
    {
        surface.pop();
    }
    surface
}

fn external_character_reference_trailing_action_noise(ch: char) -> bool {
    matches!(
        ch,
        '公' | '开'
            | '崛'
            | '起'
            | '成'
            | '为'
            | '突'
            | '破'
            | '建'
            | '立'
            | '维'
            | '护'
            | '改'
            | '写'
            | '打'
            | '败'
            | '夺'
            | '回'
            | '守'
            | '住'
            | '揭'
            | '露'
            | '反'
            | '击'
            | '连'
            | '贫'
            | '追'
            | '查'
            | '从'
            | '每'
            | '身'
            | '只'
            | '仍'
            | '因'
            | '向'
            | '给'
            | '对'
            | '中'
            | '时'
            | '次'
            | '上'
            | '位'
            | '置'
            | '知'
            | '道'
            | '后'
    )
}

fn canonicalize_relationship_quotas_to_authority(
    quotas: &mut [RelationshipInteractionQuota],
    authority: &CharacterAuthority,
) {
    if authority.names.is_empty() {
        return;
    }
    let pair = authority.relationship_pair();
    if pair.is_empty() {
        return;
    }
    for quota in quotas {
        let mut normalized = Vec::new();
        for name in &quota.characters {
            let trimmed = name.trim();
            if authority.contains(trimmed)
                && !normalized
                    .iter()
                    .any(|existing: &String| existing == trimmed)
            {
                normalized.push(trimmed.to_string());
            }
        }
        for name in &pair {
            if normalized.len() >= 2 {
                break;
            }
            if !normalized.iter().any(|existing| existing == name) {
                normalized.push(name.clone());
            }
        }
        quota.characters = normalized;
        for value in [
            &mut quota.relationship,
            &mut quota.next_due,
            &mut quota.required_interaction,
        ] {
            rewrite_contract_state_character_references_to_authority(value, authority);
        }
    }
}

fn rewrite_external_character_references_in_list_to_authority(
    values: &mut [String],
    authority: &CharacterAuthority,
) {
    for value in values {
        rewrite_contract_state_character_references_to_authority(value, authority);
    }
}

fn canonicalize_emotional_contract_to_authority(
    contract: &mut EmotionalContract,
    authority: &CharacterAuthority,
) {
    rewrite_contract_state_character_references_to_authority(
        &mut contract.primary_emotion,
        authority,
    );
    rewrite_contract_state_character_references_to_authority(
        &mut contract.emotional_promise,
        authority,
    );
    rewrite_contract_state_character_references_to_authority(
        &mut contract.ending_emotional_state,
        authority,
    );
    rewrite_external_character_references_in_list_to_authority(
        &mut contract.emotional_beats,
        authority,
    );
    rewrite_external_character_references_in_list_to_authority(
        &mut contract.relief_beats,
        authority,
    );
    rewrite_external_character_references_in_list_to_authority(
        &mut contract.payoff_requirements,
        authority,
    );
}

fn canonicalize_reader_promise_to_authority(
    promise: &mut ReaderPromise,
    authority: &CharacterAuthority,
) {
    rewrite_contract_state_character_references_to_authority(&mut promise.core_hook, authority);
    rewrite_contract_state_character_references_to_authority(
        &mut promise.curiosity_engine,
        authority,
    );
    rewrite_contract_state_character_references_to_authority(&mut promise.payoff_style, authority);
    rewrite_external_character_references_in_list_to_authority(
        &mut promise.pleasure_points,
        authority,
    );
}

fn canonicalize_antagonist_pressure_to_authority(
    pressure: &mut AntagonistPressure,
    authority: &CharacterAuthority,
) {
    let Some(replacement) = authority.pressure_source_character() else {
        return;
    };
    rewrite_contract_state_character_references_to_authority(
        &mut pressure.primary_pressure,
        authority,
    );
    for antagonist in &mut pressure.antagonists {
        if !authority.contains(&antagonist.name)
            || authority
                .primary
                .as_deref()
                .is_some_and(|primary| antagonist.name.trim() == primary)
        {
            antagonist.name = replacement.clone();
        }
        rewrite_contract_state_character_references_to_authority(&mut antagonist.goal, authority);
        rewrite_external_character_references_in_list_to_authority(
            &mut antagonist.resources,
            authority,
        );
        rewrite_contract_state_character_references_to_authority(
            &mut antagonist.knowledge_state,
            authority,
        );
        rewrite_contract_state_character_references_to_authority(
            &mut antagonist.current_move,
            authority,
        );
        rewrite_external_character_references_in_list_to_authority(
            &mut antagonist.escalation_plan,
            authority,
        );
        rewrite_contract_state_character_references_to_authority(
            &mut antagonist.defeat_condition,
            authority,
        );
    }
}

fn canonicalize_payoff_matrix_to_authority(
    matrix: &mut [PayoffMatrixEntry],
    authority: &CharacterAuthority,
) {
    for entry in matrix {
        rewrite_contract_state_character_references_to_authority(&mut entry.promise, authority);
        rewrite_contract_state_character_references_to_authority(
            &mut entry.payoff_target,
            authority,
        );
        rewrite_external_character_references_in_list_to_authority(&mut entry.evidence, authority);
    }
}

fn canonicalize_reveal_schedule_to_authority(
    schedule: &mut [RevealScheduleEntry],
    authority: &CharacterAuthority,
) {
    for entry in schedule {
        rewrite_contract_state_character_references_to_authority(&mut entry.secret, authority);
        rewrite_contract_state_character_references_to_authority(
            &mut entry.reader_knows,
            authority,
        );
        rewrite_contract_state_character_references_to_authority(
            &mut entry.protagonist_knows,
            authority,
        );
        rewrite_contract_state_character_references_to_authority(
            &mut entry.antagonist_knows,
            authority,
        );
        rewrite_contract_state_character_references_to_authority(
            &mut entry.reveal_window,
            authority,
        );
        rewrite_contract_state_character_references_to_authority(&mut entry.status, authority);
    }
}

fn canonicalize_motif_ledger_to_authority(
    motifs: &mut [MotifLedgerEntry],
    authority: &CharacterAuthority,
) {
    for entry in motifs {
        rewrite_contract_state_character_references_to_authority(&mut entry.motif, authority);
        rewrite_contract_state_character_references_to_authority(&mut entry.meaning, authority);
        rewrite_external_character_references_in_list_to_authority(&mut entry.evolution, authority);
        rewrite_contract_state_character_references_to_authority(
            &mut entry.payoff_target,
            authority,
        );
    }
}

fn canonicalize_conflict_pressure_curve_to_authority(
    curve: &mut ConflictPressureCurve,
    authority: &CharacterAuthority,
) {
    rewrite_contract_state_character_references_to_authority(
        &mut curve.release_strategy,
        authority,
    );
    rewrite_contract_state_character_references_to_authority(&mut curve.peak_policy, authority);
    for beat in &mut curve.global_curve {
        rewrite_contract_state_character_references_to_authority(&mut beat.range, authority);
        rewrite_contract_state_character_references_to_authority(
            &mut beat.pressure_level,
            authority,
        );
        rewrite_contract_state_character_references_to_authority(&mut beat.function, authority);
    }
}

fn canonicalize_contract_v2_to_authority(
    contract: &mut NovelContractV2,
    authority: &CharacterAuthority,
) {
    canonicalize_emotional_contract_to_authority(&mut contract.emotional_contract, authority);
    canonicalize_relationship_ledger_to_authority(&mut contract.relationship_ledger, authority);
    canonicalize_emotional_state_ledger_to_authority(
        &mut contract.emotional_state_ledger,
        authority,
    );
    canonicalize_character_voice_ledger_to_authority(
        &mut contract.character_voice_ledger,
        authority,
    );
    canonicalize_relationship_quotas_to_authority(
        &mut contract.relationship_interaction_quotas,
        authority,
    );
    canonicalize_reader_promise_to_authority(&mut contract.reader_promise, authority);
    canonicalize_antagonist_pressure_to_authority(&mut contract.antagonist_pressure, authority);
    canonicalize_payoff_matrix_to_authority(&mut contract.payoff_matrix, authority);
    canonicalize_reveal_schedule_to_authority(&mut contract.reveal_schedule, authority);
    canonicalize_motif_ledger_to_authority(&mut contract.motif_ledger, authority);
    canonicalize_conflict_pressure_curve_to_authority(
        &mut contract.conflict_pressure_curve,
        authority,
    );
}

pub(crate) fn canonicalize_contract_v2_to_character_lines(
    contract: &mut NovelContractV2,
    character_lines: &[String],
) {
    let authority = CharacterAuthority::from_lines(character_lines);
    canonicalize_contract_v2_to_authority(contract, &authority);
}

fn emotional_contract_has_content(value: &EmotionalContract) -> bool {
    !value_missing(&value.primary_emotion)
        || !value_missing(&value.emotional_promise)
        || !value.emotional_beats.is_empty()
        || !value.relief_beats.is_empty()
        || !value.payoff_requirements.is_empty()
        || !value_missing(&value.ending_emotional_state)
}

fn antagonist_pressure_has_content(value: &AntagonistPressure) -> bool {
    !value_missing(&value.primary_pressure) || !value.antagonists.is_empty()
}

fn narration_contract_has_content(value: &NarrationContract) -> bool {
    !value_missing(&value.pov)
        || !value_missing(&value.tense)
        || !value_missing(&value.narrative_distance)
        || !value_missing(&value.dialogue_style)
        || !value_missing(&value.description_density)
        || !value_missing(&value.chapter_pacing)
        || !value.forbidden_style_drift.is_empty()
}

fn scene_type_mix_has_content(value: &SceneTypeMix) -> bool {
    !value_missing(&value.action)
        || !value_missing(&value.dialogue)
        || !value_missing(&value.everyday)
        || !value_missing(&value.reveal)
        || !value_missing(&value.emotional)
        || !value_missing(&value.turning_point)
        || !value_missing(&value.balance_rule)
}

fn reader_promise_has_content(value: &ReaderPromise) -> bool {
    !value_missing(&value.core_hook)
        || !value.pleasure_points.is_empty()
        || !value_missing(&value.curiosity_engine)
        || !value_missing(&value.payoff_style)
}

fn chapter_ending_rotation_has_content(value: &ChapterEndingRotation) -> bool {
    !value.planned_rotation.is_empty() || !value_missing(&value.avoid_repetition_rule)
}

fn conflict_pressure_curve_has_content(value: &ConflictPressureCurve) -> bool {
    !value.global_curve.is_empty()
        || !value_missing(&value.release_strategy)
        || !value_missing(&value.peak_policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_authority_rewrites_stale_primary_subject_across_story_fields() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![CharacterContract {
            canonical_name: "白澈棠".to_string(),
            role: "主角".to_string(),
            arc_start: "林远从依赖技术炫技的独奏者".to_string(),
            arc_end: "成长为能够倾听乐团的艺术领袖".to_string(),
            ..Default::default()
        }];
        contract.premise = "林远空降交响乐团后遇到断弦事件".to_string();
        contract.protagonist_arc = "林远从孤傲的独奏者成长为乐团领袖".to_string();
        contract.main_causal_spine = "林远追查断弦、失窃总谱与指挥家死因".to_string();

        canonicalize_novel_contract_to_character_authority(&mut contract);

        let rendered = serde_json::to_string(&contract).expect("contract json");
        assert!(!rendered.contains("林远"), "{rendered}");
        assert!(rendered.contains("白澈棠"), "{rendered}");
        assert!(contract.premise.starts_with("白澈棠"), "{rendered}");
        assert!(contract.protagonist_arc.starts_with("白澈棠"), "{rendered}");
        assert!(
            contract.main_causal_spine.starts_with("白澈棠"),
            "{rendered}"
        );
    }

    #[test]
    fn character_authority_rewrites_quoted_incomplete_primary_name() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![CharacterContract {
            canonical_name: "陆晟衡".to_string(),
            role: "主角".to_string(),
            ..Default::default()
        }];
        contract.premise = "主角“默”是一名底层记忆修复师".to_string();

        canonicalize_novel_contract_to_character_authority(&mut contract);

        assert_eq!(contract.premise, "主角“陆晟衡”是一名底层记忆修复师");
    }
    use crate::tool::writing::novel_contract_v2::AntagonistRecord;

    #[test]
    fn primary_role_never_matches_a_relationship_slot() {
        let primary = CharacterContract {
            canonical_name: "顾怀声".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        };
        let collaborator = CharacterContract {
            canonical_name: "程谨岚".to_string(),
            role: "关键关系对象".to_string(),
            ..Default::default()
        };

        assert!(!character_contract_roles_match(&primary, &collaborator));
        assert!(!character_contract_roles_match(&collaborator, &primary));
    }

    #[test]
    fn rejected_title_repair_keeps_existing_title_and_rationale_atomic() {
        let mut draft = super::super::build_initial_creation_draft(
            "session",
            "fiction",
            "写一部近未来医疗伦理悬疑小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        draft.title = "脑波审计员".to_string();
        draft.fiction_title_rationale =
            "脑波审计员来自主角审查记忆数据的职业身份和终局选择。".to_string();
        let patch = TitlePatch {
            canonical_title: "标题".to_string(),
            rationale: "这是另一个未通过质量门的候选解释。".to_string(),
            ..Default::default()
        };

        patch.apply_repair_to_draft(&mut draft);

        assert_eq!(draft.title, "脑波审计员");
        assert_eq!(
            draft.fiction_title_rationale,
            "脑波审计员来自主角审查记忆数据的职业身份和终局选择。"
        );
    }

    #[test]
    fn metadata_patch_selects_a_valid_candidate_instead_of_invalid_canonical_title() {
        let mut draft = super::super::build_initial_creation_draft(
            "metadata-title-selection",
            "fiction",
            "写一部近未来医疗伦理悬疑小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        draft.fiction_premise =
            "地铁站灵能袭击暴露血瞳符文，底层青年被迫追查夜枭集团。".to_string();
        draft.fiction_ending_direction =
            "主角公开夜枭集团的瞳术账册并改写城市力量秩序。".to_string();
        draft.fiction_world_imagery = "霓虹地铁、血瞳符文、地下灵能账册。".to_string();
        draft.fiction_main_causal_spine =
            "地铁袭击引出血瞳账册，追查夜枭垄断，终局公开账册重写秩序。".to_string();
        let valid_title = "夺血瞳账册".to_string();
        let mut rationales = BTreeMap::new();
        rationales.insert(
            valid_title.clone(),
            "夺血瞳账册来自地铁袭击后的关键物件和终局公开账册改写城市力量秩序的爽点。".to_string(),
        );
        let patch = MetadataPatch {
            title: TitlePatch {
                canonical_title: "标题".to_string(),
                candidates: vec![valid_title.clone()],
                candidate_rationales: rationales,
                rationale:
                    "夺血瞳账册来自地铁袭击后的关键物件和终局公开账册改写城市力量秩序的爽点。"
                        .to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        patch.apply_to_draft(&mut draft);

        assert_eq!(draft.title, valid_title);
        assert!(draft.fiction_title_rationale.contains("城市力量秩序"));
    }

    #[test]
    fn authority_rewrite_does_not_touch_ordinary_story_terms() {
        let authority = CharacterAuthority {
            names: vec!["季桥晚".to_string(), "晏岑禾".to_string()],
            superseded_names: BTreeMap::new(),
            primary: Some("季桥晚".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("晏岑禾".to_string()),
            pressure_source: Some("晏岑禾".to_string()),
        };
        let mut value = "主角掌控都市核心灵脉，重建普通人的修行资格。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert_eq!(value, "主角掌控都市核心灵脉，重建普通人的修行资格。");
    }

    #[test]
    fn authority_rewrite_does_not_replace_overlap_inside_trust_phrase() {
        let authority = CharacterAuthority {
            names: vec!["程庭宁".to_string(), "阮澈澜".to_string()],
            superseded_names: BTreeMap::new(),
            primary: Some("程庭宁".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("阮澈澜".to_string()),
            pressure_source: Some("阮澈澜".to_string()),
        };
        let mut value = "从独自求生到敢于在绝境中信任他人。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert_eq!(value, "从独自求生到敢于在绝境中信任他人。");
    }

    #[test]
    fn authority_rewrite_replaces_longest_superseded_name_before_its_prefix() {
        let mut replacements = BTreeMap::new();
        replacements.insert("顾廷".to_string(), "温昭衡".to_string());
        replacements.insert("顾廷深".to_string(), "温昭衡".to_string());
        let mut value = "顾廷深掌控集团，顾廷最终选择放手。".to_string();

        rewrite_structured_character_references(&mut value, &replacements);

        assert_eq!(value, "温昭衡掌控集团，温昭衡最终选择放手。");
        assert!(!value.contains("温昭衡深"));
    }

    #[test]
    fn authority_rewrite_updates_a_family_term_co_referential_with_the_replaced_name() {
        let replacements = BTreeMap::from([("阮昭言".to_string(), "顾屿野".to_string())]);
        let mut value = "阮昭言决定取回阮家祖传信物。".to_string();

        rewrite_structured_character_references(&mut value, &replacements);

        assert_eq!(value, "顾屿野决定取回顾家祖传信物。");
    }

    #[test]
    fn authority_rewrite_preserves_an_unrelated_family_without_the_replaced_name() {
        let replacements = BTreeMap::from([("阮昭言".to_string(), "顾屿野".to_string())]);
        let mut value = "顾屿野决定调查阮家失踪案。".to_string();

        rewrite_structured_character_references(&mut value, &replacements);

        assert_eq!(value, "顾屿野决定调查阮家失踪案。");
    }

    #[test]
    fn authority_rewrite_updates_compound_surname_family_terms() {
        let replacements = BTreeMap::from([("欧阳知白".to_string(), "司马望宁".to_string())]);
        let mut value = "欧阳知白拒绝继承欧阳家旧约。".to_string();

        rewrite_structured_character_references(&mut value, &replacements);

        assert_eq!(value, "司马望宁拒绝继承司马家旧约。");
    }

    #[test]
    fn authority_rewrite_updates_bare_household_anchors_to_primary_authority() {
        let authority = CharacterAuthority {
            names: vec!["顾维澜".to_string(), "裴予川".to_string()],
            superseded_names: BTreeMap::from([
                ("沈长风".to_string(), "顾维澜".to_string()),
                ("沈清婉".to_string(), "裴予川".to_string()),
                ("祝承言".to_string(), "顾维澜".to_string()),
            ]),
            primary: Some("顾维澜".to_string()),
            secondary: Some("裴予川".to_string()),
            ..Default::default()
        };
        let mut value = "重振沈家；从沈府救出沈父；重夺祝家兵权。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert_eq!(value, "重振顾家；从顾府救出顾父；重夺顾家兵权。");
    }

    #[test]
    fn explicit_non_primary_household_reference_wins_before_bare_primary_fallback() {
        let authority = CharacterAuthority {
            names: vec!["顾维澜".to_string(), "裴予川".to_string()],
            superseded_names: BTreeMap::from([
                ("沈长风".to_string(), "顾维澜".to_string()),
                ("沈清婉".to_string(), "裴予川".to_string()),
            ]),
            primary: Some("顾维澜".to_string()),
            secondary: Some("裴予川".to_string()),
            ..Default::default()
        };
        let mut value = "沈清婉返回沈府处理自己的族产。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert_eq!(value, "裴予川返回裴府处理自己的族产。");
    }

    #[test]
    fn contract_canonicalization_rewrites_stale_household_anchors_in_every_story_scope() {
        let mut contract = NovelCreationContract {
            characters: vec![
                CharacterContract {
                    canonical_name: "顾维澜".to_string(),
                    name_source: "generated_by_writing_tool_policy".to_string(),
                    previous_names: vec!["沈长风".to_string(), "祝承言".to_string()],
                    role: "主角".to_string(),
                    desire: "重振沈家".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "裴予川".to_string(),
                    name_source: "generated_by_writing_tool_policy".to_string(),
                    previous_names: vec!["沈清婉".to_string()],
                    role: "关键关系对象".to_string(),
                    bottom_line: "守住沈家最后的血脉".to_string(),
                    ..Default::default()
                },
            ],
            outline: OutlineContract {
                raw_outline: "顾维澜在沈府醒来并重夺祝家兵权".to_string(),
                volumes: vec![VolumeContract {
                    objective: "重夺祝家兵权".to_string(),
                    ..Default::default()
                }],
                near_chapters: vec![ChapterSeedContract {
                    number: Some(1),
                    goal: "从沈府救出沈父".to_string(),
                    ..Default::default()
                }],
            },
            ..Default::default()
        };

        canonicalize_novel_contract_to_character_authority(&mut contract);

        let serialized = serde_json::to_string(&contract).expect("contract json");
        assert!(!serialized.contains("沈家"), "{serialized}");
        assert!(!serialized.contains("沈府"), "{serialized}");
        assert!(!serialized.contains("沈父"), "{serialized}");
        assert!(!serialized.contains("祝家"), "{serialized}");
        assert!(serialized.contains("顾家"), "{serialized}");
        assert!(serialized.contains("顾府"), "{serialized}");
        assert!(serialized.contains("顾父"), "{serialized}");
    }

    #[test]
    fn authority_rewrite_replaces_standalone_single_character_codename_everywhere() {
        let replacements = BTreeMap::from([("K".to_string(), "姜承舟".to_string())]);
        let mut contract = NovelCreationContract {
            premise: "K追捕主角，但OK协议与K7芯片不是角色名。".to_string(),
            outline: OutlineContract {
                raw_outline: "主角躲避K追杀并定位K总部。".to_string(),
                ..Default::default()
            },
            structured: NovelContractV2 {
                antagonist_pressure: AntagonistPressure {
                    primary_pressure: "K持续封锁底层区。".to_string(),
                    antagonists: vec![crate::tool::writing::novel_contract_v2::AntagonistRecord {
                        name: "K".to_string(),
                        resources: vec!["AI副手K".to_string()],
                        ..Default::default()
                    }],
                },
                reveal_schedule: vec![RevealScheduleEntry {
                    secret: "K的真实身份".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };

        rewrite_novel_contract_names(&mut contract, &replacements);

        let rewritten = serde_json::to_string(&contract).expect("serialize contract");
        assert!(!rewritten.contains("K追捕"), "{rewritten}");
        assert!(!rewritten.contains("K总部"), "{rewritten}");
        assert!(!rewritten.contains("AI副手K"), "{rewritten}");
        assert!(!rewritten.contains("K的真实身份"), "{rewritten}");
        assert!(rewritten.contains("OK协议"), "{rewritten}");
        assert!(rewritten.contains("K7芯片"), "{rewritten}");
    }

    #[test]
    fn authority_rewrite_replaces_stale_name_in_other_character_fields() {
        let mut contract = NovelCreationContract {
            characters: vec![
                CharacterContract {
                    canonical_name: "秦照棠".to_string(),
                    name_source: "generated_by_writing_tool_policy".to_string(),
                    previous_names: vec!["林默".to_string()],
                    role: "主角".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "许维禾".to_string(),
                    role: "关键关系对象".to_string(),
                    bottom_line: "必须守住林默的记忆核心".to_string(),
                    planned_entry: "第1卷与林默建立生存同盟".to_string(),
                    planned_exit: "陪伴林默至终局".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        canonicalize_novel_contract_to_character_authority(&mut contract);

        let relation = &contract.characters[1];
        assert_eq!(relation.bottom_line, "必须守住秦照棠的记忆核心");
        assert_eq!(relation.planned_entry, "第1卷与秦照棠建立生存同盟");
        assert_eq!(relation.planned_exit, "陪伴秦照棠至终局");
        assert_eq!(contract.characters[0].previous_names, ["林默"]);
    }

    #[test]
    fn authority_rewrite_repairs_explicit_role_slot_only() {
        let authority = CharacterAuthority {
            names: vec!["季桥晚".to_string(), "晏岑禾".to_string()],
            superseded_names: BTreeMap::new(),
            primary: Some("季桥晚".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("晏岑禾".to_string()),
            pressure_source: Some("晏岑禾".to_string()),
        };
        let mut value = "主角林凡公开旧楼账本并重建晋升规则。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert_eq!(value, "主角季桥晚公开旧楼账本并重建晋升规则。");
    }

    #[test]
    fn authority_rewrite_keeps_male_and_female_lead_slots_distinct() {
        let authority = CharacterAuthority {
            names: vec!["岑怀言".to_string(), "程照声".to_string()],
            superseded_names: BTreeMap::new(),
            primary: Some("岑怀言".to_string()),
            male_primary: Some("岑怀言".to_string()),
            female_primary: Some("程照声".to_string()),
            secondary: Some("程照声".to_string()),
            pressure_source: Some("程照声".to_string()),
        };
        let mut value = "男主林深因实验事故失忆，女主苏念为保护事务所与他签订契约。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert_eq!(
            value,
            "男主岑怀言因实验事故失忆，女主程照声为保护事务所与他签订契约。"
        );
    }

    #[test]
    fn character_plan_references_wait_for_an_actual_outline() {
        let mut characters = vec![CharacterContract {
            canonical_name: "岑怀言".to_string(),
            planned_entry: "第1卷进入主线".to_string(),
            planned_exit: "持续至第4卷终局".to_string(),
            ..Default::default()
        }];

        remove_character_plan_references_to_missing_outline(&mut characters, 0);

        assert!(characters[0].planned_entry.is_empty());
        assert!(characters[0].planned_exit.is_empty());
    }

    #[test]
    fn authority_rewrite_preserves_unqualified_external_name_for_typed_gate() {
        let authority = CharacterAuthority {
            names: vec!["孟闻白".to_string(), "闻予晚".to_string()],
            superseded_names: BTreeMap::new(),
            primary: Some("孟闻白".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("闻予晚".to_string()),
            pressure_source: Some("闻予晚".to_string()),
        };
        let mut value = "对白必须体现孟闻白害怕林渊突破后的代价。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert_eq!(value, "对白必须体现孟闻白害怕林渊突破后的代价。");
    }

    #[test]
    fn authority_rewrite_never_turns_story_object_into_character() {
        let authority = CharacterAuthority {
            names: vec!["沈栖安".to_string(), "阮庭澜".to_string()],
            superseded_names: BTreeMap::new(),
            primary: Some("沈栖安".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("阮庭澜".to_string()),
            pressure_source: Some("阮庭澜".to_string()),
        };
        let mut value = "主角延缓城市下沉，但第七区永久沉入水中，成为水下遗迹。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert!(value.contains("水下遗迹"), "{value}");
        assert!(!value.ends_with("成为沈栖安。"), "{value}");
    }

    #[test]
    fn authority_rewrite_keeps_collective_terms() {
        let authority = CharacterAuthority {
            names: vec!["孟闻白".to_string(), "闻予晚".to_string()],
            superseded_names: BTreeMap::new(),
            primary: Some("孟闻白".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("闻予晚".to_string()),
            pressure_source: Some("闻予晚".to_string()),
        };
        let mut value = "苏家试图垄断旧城灵脉。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert_eq!(value, "苏家试图垄断旧城灵脉。");
    }

    #[test]
    fn authority_rewrite_repairs_primary_marker_with_non_primary_authority_name() {
        let authority = CharacterAuthority {
            names: vec![
                "姜夙白".to_string(),
                "司栖澜".to_string(),
                "洛澈棠".to_string(),
            ],
            superseded_names: BTreeMap::new(),
            primary: Some("姜夙白".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("司栖澜".to_string()),
            pressure_source: Some("洛澈棠".to_string()),
        };
        let mut value = "主角司栖澜在灵脉崩塌时承担最终代价。".to_string();

        rewrite_external_character_references_to_authority(&mut value, &authority);

        assert_eq!(value, "主角姜夙白在灵脉崩塌时承担最终代价。");
    }

    #[test]
    fn antagonist_pressure_does_not_fallback_to_protagonist() {
        let authority = CharacterAuthority {
            names: vec![
                "姜夙白".to_string(),
                "司栖澜".to_string(),
                "洛澈棠".to_string(),
            ],
            superseded_names: BTreeMap::new(),
            primary: Some("姜夙白".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("司栖澜".to_string()),
            pressure_source: Some("洛澈棠".to_string()),
        };
        let mut pressure = AntagonistPressure {
            primary_pressure: "世家继续围猎主角司栖澜。".to_string(),
            antagonists: vec![AntagonistRecord {
                name: "姜夙白".to_string(),
                goal: "压制主角司栖澜".to_string(),
                ..Default::default()
            }],
        };

        canonicalize_antagonist_pressure_to_authority(&mut pressure, &authority);

        assert_eq!(pressure.antagonists[0].name, "洛澈棠");
        assert_eq!(pressure.primary_pressure, "世家继续围猎主角姜夙白。");
        assert_eq!(pressure.antagonists[0].goal, "压制主角姜夙白");
    }

    #[test]
    fn character_anchor_lines_preserve_unqualified_names_for_typed_repair() {
        let mut lines = vec![
            "name: 司砚棠; role: 主角; desire: 找到被夺走的灵脉; fear: 林烬还在燃烧; bottom_line: 不牺牲阮阙白换取飞升; arc_start: 被宗门放逐; arc_end: 守住新秩序".to_string(),
            "name: 白阙砺; role: 关键对手; desire: 维持旧宗门秩序; fear: 司砚棠公开灵契; bottom_line: 不承认低阶修士改写天规; arc_start: 执法长老; arc_end: 被迫面对新天规".to_string(),
        ];
        let authority = CharacterAuthority::from_lines(&lines);

        canonicalize_character_anchor_lines_to_authority(&mut lines, &authority);

        let references =
            crate::tool::writing::typed_contract_gate::character_anchor_person_references(
                &lines.join("\n"),
            );
        let joined = lines.join("\n");
        assert!(
            joined.contains("阮阙白"),
            "unqualified fresh character-like names should remain visible instead of being silently rewritten: {lines:#?}; refs={references:?}"
        );
        assert!(joined.contains("林烬"));
    }

    #[test]
    fn character_voice_owner_uses_authority_name_from_voice_text() {
        let authority = CharacterAuthority {
            names: vec![
                "韩栖序".to_string(),
                "洛予宁".to_string(),
                "温棠白".to_string(),
            ],
            superseded_names: BTreeMap::new(),
            primary: Some("韩栖序".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("洛予宁".to_string()),
            pressure_source: Some("温棠白".to_string()),
        };
        let mut ledger = vec![CharacterVoiceProfile {
            character: "角色".to_string(),
            voice_style: "围绕洛予宁的门规压力展开，语气克制。".to_string(),
            dialogue_rules: vec!["对白必须体现洛予宁坚守门规。".to_string()],
            ..Default::default()
        }];

        canonicalize_character_voice_ledger_to_authority(&mut ledger, &authority);

        assert_eq!(ledger[0].character, "洛予宁");
    }

    #[test]
    fn character_voice_rules_preserve_unqualified_actor_for_typed_gate() {
        let authority = CharacterAuthority {
            names: vec!["司庭序".to_string(), "温泊安".to_string()],
            superseded_names: BTreeMap::new(),
            primary: Some("司庭序".to_string()),
            male_primary: None,
            female_primary: None,
            secondary: Some("温泊安".to_string()),
            pressure_source: Some("温泊安".to_string()),
        };
        let mut ledger = vec![CharacterVoiceProfile {
            character: "司庭序".to_string(),
            voice_style: "面对压力时语气克制。".to_string(),
            dialogue_rules: vec!["害怕角色沈知行识破药方时仍不说流程说明。".to_string()],
            ..Default::default()
        }];

        canonicalize_character_voice_ledger_to_authority(&mut ledger, &authority);

        let serialized = serde_json::to_string(&ledger).expect("voice ledger");
        assert!(serialized.contains("沈知行"), "{serialized}");
        assert!(serialized.contains("司庭序"), "{serialized}");
    }

    #[test]
    fn character_patch_replaces_repairable_nonempty_anchor_fields() {
        let mut target = CharacterContract {
            canonical_name: "洛衡遥".to_string(),
            role: "导师".to_string(),
            desire: "守住旧校史".to_string(),
            fear: "学生再被系统抹名".to_string(),
            bottom_line: "守灯".to_string(),
            arc_start: "沉默的档案管理员".to_string(),
            arc_end: "公开校史证据的人".to_string(),
            ..Default::default()
        };
        let incoming = CharacterContract {
            canonical_name: "洛衡遥".to_string(),
            role: "导师".to_string(),
            desire: "守住旧校史".to_string(),
            fear: "学生再被系统抹名".to_string(),
            bottom_line: "不伪造证据换取安全".to_string(),
            arc_start: "沉默的档案管理员".to_string(),
            arc_end: "公开校史证据的人".to_string(),
            ..Default::default()
        };

        merge_missing_character_contract_fields(&mut target, &incoming, &["洛衡遥".to_string()], 0);

        assert_eq!(target.bottom_line, "不伪造证据换取安全");
    }

    #[test]
    fn character_patch_replaces_anchor_that_references_external_actor() {
        let mut target = CharacterContract {
            canonical_name: "司庭序".to_string(),
            role: "主角".to_string(),
            fear: "害怕角色沈知行销毁医馆账册".to_string(),
            arc_start: "沈知行原本替他判断每一张药方".to_string(),
            ..Default::default()
        };
        let incoming = CharacterContract {
            canonical_name: "司庭序".to_string(),
            role: "主角".to_string(),
            fear: "害怕唯一账册在公开前被销毁".to_string(),
            arc_start: "只敢按旧规复核药方的年轻医者".to_string(),
            ..Default::default()
        };

        merge_missing_character_contract_fields(
            &mut target,
            &incoming,
            &["司庭序".to_string(), "温泊安".to_string()],
            0,
        );

        assert_eq!(target.fear, incoming.fear);
        assert_eq!(target.arc_start, incoming.arc_start);
    }

    #[test]
    fn character_patch_accepts_targeted_anchor_repair_after_authority_exists() {
        let mut draft = super::build_initial_creation_draft(
            "session-character-partial-repair",
            "fiction",
            "写一部职场悬疑小说，每章2500字，一共5万字",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 岑庭白; role: 主角; desire: 查清档案篡改; fear: 自己也是伪造记录; bottom_line: 不牺牲无辜者换取真相; arc_start: 服从规则; arc_end: 公开真相".to_string(),
            "name: 温望声; role: 导师; desire: 找回旧证据; fear: 密钥失效; bottom_line: 守护芯片; arc_start: 隐瞒过往; arc_end: 交出证据".to_string(),
            "name: 孟岑宁; role: 对手; desire: 维持档案秩序; fear: 系统崩溃; bottom_line: 清除异常数据; arc_start: 控制局面; arc_end: 面对真相".to_string(),
        ];
        let patch = CreationContractPatch::Characters(CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "温望声".to_string(),
                bottom_line: "绝不拿无辜档案员测试创始人密钥".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let report = patch.validate_scope(&draft);

        assert!(report.ready(), "unexpected issues: {:?}", report.issues);
    }

    #[test]
    fn character_patch_can_fill_a_missing_support_slot_after_authority_exists() {
        let mut draft = super::build_initial_creation_draft(
            "session-character-missing-support",
            "fiction",
            "写一部都市悬疑小说，每章2500字，一共10万字",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 阮砚澜; role: 女主; desire: 公开并购证据; fear: 证据链被销毁; bottom_line: 不牺牲无辜员工; arc_start: 独自审计; arc_end: 公开追责; name_source: generated_by_writing_tool_policy".to_string(),
            "name: 南照舟; role: 关键对手; desire: 掩盖并购黑幕; fear: 私账曝光; bottom_line: 不放弃董事会控制; arc_start: 幕后操盘; arc_end: 接受审判; name_source: generated_by_writing_tool_policy".to_string(),
        ];
        let patch = CreationContractPatch::Characters(CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "顾临川".to_string(),
                role: "关键关系对象".to_string(),
                desire: "完成独立法律尽调".to_string(),
                fear: "关键证人被收买".to_string(),
                bottom_line: "不销毁原始律师底稿".to_string(),
                arc_start: "只信书面程序".to_string(),
                arc_end: "愿意与阮砚澜共同公开证据".to_string(),
                planned_entry: "第一卷".to_string(),
                planned_exit: "终局共同作证".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let report = patch.validate_scope(&draft);

        assert!(report.ready(), "unexpected issues: {:?}", report.issues);
        patch.apply_to_draft(&mut draft);
        let characters = draft
            .fiction_characters
            .iter()
            .map(|line| super::draft_character_line_to_contract(line))
            .collect::<Vec<_>>();
        assert_eq!(characters.len(), 3);
        assert!(characters.iter().any(|character| {
            character.role_family().is_some() && !character.role_looks_primary()
        }));
    }

    #[test]
    fn character_patch_still_rejects_an_unrequested_extra_name_when_slots_are_complete() {
        let mut draft = super::build_initial_creation_draft(
            "session-character-complete-slots",
            "fiction",
            "写一部都市悬疑小说，每章2500字，一共10万字",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 阮砚澜; role: 女主; desire: 公开并购证据; fear: 证据链被销毁; bottom_line: 不牺牲无辜员工; arc_start: 独自审计; arc_end: 公开追责".to_string(),
            "name: 顾临川; role: 关键关系对象; desire: 完成法律尽调; fear: 证人被收买; bottom_line: 不销毁底稿; arc_start: 只信程序; arc_end: 共同作证".to_string(),
            "name: 南照舟; role: 关键对手; desire: 掩盖并购黑幕; fear: 私账曝光; bottom_line: 不放弃控制; arc_start: 幕后操盘; arc_end: 接受审判".to_string(),
        ];
        let patch = CreationContractPatch::Characters(CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "林默".to_string(),
                role: "关键关系对象".to_string(),
                desire: "加入调查".to_string(),
                fear: "调查失败".to_string(),
                bottom_line: "不伪造证据".to_string(),
                arc_start: "局外人".to_string(),
                arc_end: "证人".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let report = patch.validate_scope(&draft);

        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("角色权威表外姓名 `林默`")),
            "unexpected issues: {:?}",
            report.issues
        );
    }

    #[test]
    fn character_patch_preserves_valid_nonempty_anchor_fields() {
        let mut target = CharacterContract {
            canonical_name: "洛衡遥".to_string(),
            role: "导师".to_string(),
            desire: "守住旧校史".to_string(),
            fear: "学生再被系统抹名".to_string(),
            bottom_line: "不伪造证据换取安全".to_string(),
            arc_start: "沉默的档案管理员".to_string(),
            arc_end: "公开校史证据的人".to_string(),
            ..Default::default()
        };
        let incoming = CharacterContract {
            canonical_name: "洛衡遥".to_string(),
            role: "导师".to_string(),
            desire: "守住旧校史".to_string(),
            fear: "学生再被系统抹名".to_string(),
            bottom_line: "不牺牲学生换取校盟承认".to_string(),
            arc_start: "沉默的档案管理员".to_string(),
            arc_end: "公开校史证据的人".to_string(),
            ..Default::default()
        };

        merge_missing_character_contract_fields(&mut target, &incoming, &["洛衡遥".to_string()], 0);

        assert_eq!(target.bottom_line, "不伪造证据换取安全");
    }

    #[test]
    fn character_patch_repairs_identity_conflicting_anchor_without_changing_locked_role() {
        let mut target = CharacterContract {
            canonical_name: "钟星岚".to_string(),
            role: "女主".to_string(),
            arc_start: "初出茅庐的寒门士子".to_string(),
            ..Default::default()
        };
        let incoming = CharacterContract {
            canonical_name: "钟星岚".to_string(),
            role: "寒门官员".to_string(),
            arc_start: "初出茅庐的寒门女官".to_string(),
            ..Default::default()
        };

        merge_missing_character_contract_fields(&mut target, &incoming, &["钟星岚".to_string()], 0);

        assert_eq!(target.role, "女主");
        assert_eq!(target.arc_start, "初出茅庐的寒门女官");
    }

    #[test]
    fn targeted_character_patch_keeps_known_role_authority_even_if_model_repeats_story_role() {
        let mut draft = super::build_initial_creation_draft(
            "session-character-role-lock",
            "fiction",
            "写一部历史小说，每章2500字，一共10万字",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 钟星岚; role: 女主; desire: 查清旧案; fear: 权臣灭口; bottom_line: 不牺牲百姓; arc_start: 寒门士子; arc_end: 主动辞官".to_string(),
            "name: 唐晏白; role: 同伴; desire: 稳固皇位; fear: 沦为傀儡; bottom_line: 不出卖国本; arc_start: 幼主; arc_end: 独立君主".to_string(),
        ];
        let patch = CreationContractPatch::Characters(CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "唐晏白".to_string(),
                role: "幼主".to_string(),
                arc_start: "尚未亲政的皇子".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let report = patch.validate_scope(&draft);

        assert!(report.ready(), "unexpected issues: {:?}", report.issues);
    }

    #[test]
    fn complete_canonical_character_patch_repairs_wrong_roles_without_renaming() {
        let mut draft = super::build_initial_creation_draft(
            "complete-character-role-repair",
            "fiction",
            "写一部古代言情小说，每章2500字，一共10万字",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 叶望真; role: 主角; desire: 保住香药铺; fear: 家业被夺; bottom_line: 不以假香害人; arc_start: 独自守店; arc_end: 重建商号; name_source: generated_by_writing_tool_policy".to_string(),
            "name: 顾云朔; role: 关键关系对象; desire: 垄断香药行会; fear: 私账曝光; bottom_line: 不失去行会控制; arc_start: 隐身幕后的会首; arc_end: 因私账败露; name_source: generated_by_writing_tool_policy".to_string(),
            "name: 陶泊衡; role: 对手; desire: 查清贡香账册; fear: 证据被毁; bottom_line: 不以无辜者顶罪; arc_start: 独自查案的官员; arc_end: 与叶望真共同追查真相; name_source: generated_by_writing_tool_policy".to_string(),
        ];

        let role_repair = CharacterPatch {
            characters: vec![
                CharacterContract {
                    canonical_name: "叶望真".to_string(),
                    role: "主角".to_string(),
                    desire: "保住香药铺并查明账册真相".to_string(),
                    fear: "家业与信誉一同被夺".to_string(),
                    bottom_line: "不以假香害人".to_string(),
                    arc_start: "独自承担店铺债务".to_string(),
                    arc_end: "能与可信之人共同承担责任".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "顾云朔".to_string(),
                    role: "关键对手".to_string(),
                    desire: "垄断香药行会并掩盖私账".to_string(),
                    fear: "贡香私账曝光".to_string(),
                    bottom_line: "不失去行会控制".to_string(),
                    arc_start: "隐身幕后的会首".to_string(),
                    arc_end: "因私账败露而失势".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "陶泊衡".to_string(),
                    role: "关键关系对象".to_string(),
                    desire: "与叶望真共同查清贡香账册".to_string(),
                    fear: "证据被毁且连累叶望真".to_string(),
                    bottom_line: "不以无辜者顶罪".to_string(),
                    arc_start: "只相信卷宗的年轻官员".to_string(),
                    arc_end: "学会信任叶望真的判断".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut ordinary_completion_draft = draft.clone();
        role_repair.apply_to_draft(&mut ordinary_completion_draft);
        let ordinary_characters = ordinary_completion_draft
            .fiction_characters
            .iter()
            .map(|line| super::draft_character_line_to_contract(line))
            .collect::<Vec<_>>();
        assert_eq!(ordinary_characters[1].role, "关键关系对象");
        assert_eq!(ordinary_characters[2].role, "对手");

        role_repair.apply_to_draft_with_role_repair_policy(&mut draft, true);

        let characters = draft
            .fiction_characters
            .iter()
            .map(|line| super::draft_character_line_to_contract(line))
            .collect::<Vec<_>>();
        assert_eq!(
            characters
                .iter()
                .map(|character| character.canonical_name.as_str())
                .collect::<Vec<_>>(),
            vec!["叶望真", "顾云朔", "陶泊衡"]
        );
        assert_eq!(characters[0].role, "主角");
        assert_eq!(characters[1].role, "对手");
        assert_eq!(characters[2].role, "关键关系对象");
        assert_eq!(characters[2].desire, "与叶望真共同查清贡香账册");
        assert!(characters
            .iter()
            .all(|character| { character.name_source == "generated_by_writing_tool_policy" }));
    }

    #[test]
    fn character_patch_replaces_nonempty_plan_anchor_outside_actual_volumes() {
        let mut target = CharacterContract {
            canonical_name: "叶谨声".to_string(),
            role: "女主".to_string(),
            planned_entry: "第一卷".to_string(),
            planned_exit: "第十六卷".to_string(),
            ..Default::default()
        };
        let incoming = CharacterContract {
            canonical_name: "叶谨声".to_string(),
            role: "女主".to_string(),
            planned_entry: "第一卷".to_string(),
            planned_exit: "第五卷终局成为能源节点".to_string(),
            ..Default::default()
        };

        merge_missing_character_contract_fields(&mut target, &incoming, &["叶谨声".to_string()], 5);

        assert_eq!(target.planned_entry, "第一卷");
        assert_eq!(target.planned_exit, "第五卷终局成为能源节点");
    }

    #[test]
    fn character_patch_does_not_replace_invalid_plan_anchor_with_another_invalid_anchor() {
        let mut target = CharacterContract {
            canonical_name: "叶谨声".to_string(),
            role: "女主".to_string(),
            planned_exit: "第十六卷".to_string(),
            ..Default::default()
        };
        let incoming = CharacterContract {
            canonical_name: "叶谨声".to_string(),
            role: "女主".to_string(),
            planned_exit: "第十五卷".to_string(),
            ..Default::default()
        };

        merge_missing_character_contract_fields(&mut target, &incoming, &["叶谨声".to_string()], 5);

        assert_eq!(target.planned_exit, "第十六卷");
    }

    #[test]
    fn repeated_character_repairs_preserve_locked_names_and_role_slots() {
        let mut draft = super::build_initial_creation_draft(
            "character-authority-repair",
            "fiction",
            "写一部现实主义都市职场小说，每章2500字，一共5万字",
        )
        .expect("fiction creation draft");
        draft.fiction_characters = vec![
            "name: 陶照声; role: 主角; desire: 公开真实账目; fear: 同伴再次被迫背锅; bottom_line: 不伪造证据换取晋升; arc_start: 沉默的审计员; arc_end: 建立透明规则; name_source: generated_by_writing_tool_policy".to_string(),
            "name: 阮予宁; role: 导师; desire: 保护年轻同事; fear: 团队再次被清洗; bottom_line: 守灯; arc_start: 隐忍的部门主管; arc_end: 公开支持改革; name_source: generated_by_writing_tool_policy".to_string(),
        ];

        for incoming_name in ["谢庭舟", "顾桥白"] {
            CharacterPatch {
                characters: vec![
                    CharacterContract {
                        canonical_name: "林默".to_string(),
                        role: "主角".to_string(),
                        desire: "公开真实账目".to_string(),
                        fear: "同伴再次被迫背锅".to_string(),
                        bottom_line: "不伪造证据换取晋升".to_string(),
                        arc_start: "沉默的审计员".to_string(),
                        arc_end: "建立透明规则".to_string(),
                        ..Default::default()
                    },
                    CharacterContract {
                        canonical_name: incoming_name.to_string(),
                        role: "导师".to_string(),
                        desire: "保护年轻同事".to_string(),
                        fear: "团队再次被清洗".to_string(),
                        bottom_line: "不牺牲下属换取职位安全".to_string(),
                        arc_start: "隐忍的部门主管".to_string(),
                        arc_end: "公开支持改革".to_string(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }
            .apply_to_draft(&mut draft);
        }

        assert_eq!(
            draft.fiction_characters.len(),
            2,
            "{:#?}",
            draft.fiction_characters
        );
        assert!(
            draft.fiction_characters.iter().any(|line| {
                line.contains("陶照声")
                    && line.contains("name_source: generated_by_writing_tool_policy")
            }),
            "{:#?}",
            draft.fiction_characters
        );
        assert!(
            draft.fiction_characters.iter().any(|line| {
                line.contains("阮予宁")
                    && line.contains("不牺牲下属换取职位安全")
                    && line.contains("name_source: generated_by_writing_tool_policy")
            }),
            "{:#?}",
            draft.fiction_characters
        );
        assert!(!draft
            .fiction_characters
            .iter()
            .any(|line| line.contains("谢庭舟")
                || line.contains("顾桥白")
                || line.contains("林默")));
    }

    #[test]
    fn initial_governance_does_not_trust_model_claimed_user_name_source() {
        let draft = super::build_initial_creation_draft(
            "initial-model-name-provenance",
            "fiction",
            "写一部深海悬疑小说，每章2500字，一共10万字；角色姓名请自然且互不混淆。",
        )
        .expect("fiction creation draft");
        let mut characters = vec![
            CharacterContract {
                canonical_name: "秦承弦".to_string(),
                name_source: "user".to_string(),
                role: "导师".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "梁知弦".to_string(),
                name_source: "contract_authority".to_string(),
                role: "对手".to_string(),
                ..Default::default()
            },
        ];

        let governance = govern_initial_character_names(&mut characters, &draft);
        assert!(characters
            .iter()
            .all(|character| character.name_source.is_empty()));
        governance.lock_authority(&mut characters);

        assert!(characters
            .iter()
            .all(|character| character.name_source == "generated_by_writing_tool_policy"));
        assert!(characters
            .iter()
            .all(|character| !matches!(character.canonical_name.as_str(), "秦承弦" | "梁知弦")));
        assert_ne!(
            characters[0].canonical_name.chars().last(),
            characters[1].canonical_name.chars().last()
        );
    }

    #[test]
    fn forbidden_character_name_is_reallocated_before_authority_lock() {
        let mut draft = super::build_initial_creation_draft(
            "initial-forbidden-name-governance",
            "fiction",
            "写一部都市悬疑小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        super::apply_message_to_creation_draft(&mut draft, "主角不要叫林默。");
        let mut characters = vec![CharacterContract {
            canonical_name: "林默".to_string(),
            name_source: "contract_authority".to_string(),
            role: "主角".to_string(),
            ..Default::default()
        }];

        let governance = govern_initial_character_names(&mut characters, &draft);
        governance.lock_authority(&mut characters);

        assert_ne!(characters[0].canonical_name, "林默");
        assert_eq!(
            characters[0].name_source,
            "generated_by_writing_tool_policy"
        );
    }

    #[test]
    fn name_rewrite_covers_all_structured_story_scopes_before_authority_lock() {
        let mut contract = NovelContractV2 {
            resource_economy: ResourceEconomy {
                class_impact: "林默无法购买深潜许可".to_string(),
                ..Default::default()
            },
            relationship_ledger: vec![RelationshipLedgerEntry {
                turning_points: vec!["林默公开事故日志".to_string()],
                transition_history: vec![RelationshipTransition {
                    event: "林默拒绝隐瞒".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            power_progression: PowerProgression {
                character_current_levels: vec![CharacterProgressionState {
                    character: "林默".to_string(),
                    evidence: "林默完成第一次深潜".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            social_order: SocialOrder {
                laws: vec!["林默不得读取封存日志".to_string()],
                ..Default::default()
            },
            geography_model: GeographyModel {
                important_locations: vec![LocationRecord {
                    role: "林默的事故调查入口".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            time_model: TimeModel {
                age_progression: vec![AgeProgressionState {
                    character: "林默".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            artifact_ledger: vec![ArtifactLedgerEntry {
                owner: "林默".to_string(),
                origin: "林默从事故艇带回".to_string(),
                ..Default::default()
            }],
            chapter_ending_rotation: ChapterEndingRotation {
                planned_rotation: vec!["林默发现新证据".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };

        rewrite_contract_v2_names(
            &mut contract,
            &BTreeMap::from([("林默".to_string(), "阮知白".to_string())]),
        );

        let rewritten = serde_json::to_string(&contract).expect("serialize contract");
        assert!(!rewritten.contains("林默"), "{rewritten}");
        assert!(rewritten.contains("阮知白"), "{rewritten}");
    }

    #[test]
    fn initial_character_patch_filters_non_character_before_name_rewrite() {
        let mut draft = super::super::build_initial_creation_draft(
            "initial-character-non-character-filter",
            "fiction",
            "写近未来海洋悬疑小说；K-7是一份协议编号，明确不是人物姓名或角色；每章2500字，总字数10万字。",
        )
        .expect("draft");
        draft.fiction_premise = "林远追查K-7协议造成的日志空白。".to_string();
        let patch = CharacterPatch {
            characters: vec![
                CharacterContract {
                    canonical_name: "林远".to_string(),
                    role: "主角".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "陈默".to_string(),
                    role: "同伴".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "K-7协议".to_string(),
                    role: "对手".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        patch.apply_to_draft(&mut draft);

        assert_eq!(draft.fiction_characters.len(), 2);
        assert!(draft
            .fiction_characters
            .iter()
            .all(|line| !line.contains("K-7") && !line.contains("previous_names: K-7")));
        assert!(draft.fiction_premise.contains("K-7协议"));
    }

    #[test]
    fn source_less_model_names_cannot_promote_themselves_to_contract_authority() {
        let mut draft = super::build_initial_creation_draft(
            "pending-character-authority-repair",
            "fiction",
            "写一部历史商战群像小说，每章2500字，一共5万字",
        )
        .expect("fiction creation draft");
        draft.fiction_characters = vec![
            "name: 景砚安; role: 主角; desire: 公开盐铁账目; fear: 商号被夺; bottom_line: 不伪造账册换取胜利; arc_start: 边缘账房; arc_end: 新商约建立者".to_string(),
            "name: 陶庭野; role: 同伴; desire: 守住家业; fear: 同伴再被牺牲; bottom_line: 守灯; arc_start: 沉默伙计; arc_end: 公开作证者".to_string(),
        ];
        let authority = CharacterAuthority::from_lines(&draft.fiction_characters);
        assert!(authority.contains("景砚安"), "{authority:#?}");
        assert!(authority.contains("陶庭野"), "{authority:#?}");

        CharacterPatch {
            characters: vec![
                CharacterContract {
                    canonical_name: "林默".to_string(),
                    role: "主角".to_string(),
                    desire: "公开盐铁账目".to_string(),
                    fear: "商号被夺".to_string(),
                    bottom_line: "不伪造账册换取胜利".to_string(),
                    arc_start: "边缘账房".to_string(),
                    arc_end: "新商约建立者".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "顾桥白".to_string(),
                    role: "同伴".to_string(),
                    desire: "守住家业".to_string(),
                    fear: "同伴再被牺牲".to_string(),
                    bottom_line: "不牺牲同伴换取商号安全".to_string(),
                    arc_start: "沉默伙计".to_string(),
                    arc_end: "公开作证者".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
        .apply_to_draft(&mut draft);

        let visible = draft.fiction_characters.join("\n");
        let canonical_names = draft
            .fiction_characters
            .iter()
            .filter_map(|line| super::character_name_from_contract_line(line))
            .collect::<BTreeSet<_>>();
        assert!(!canonical_names.contains("景砚安"), "{visible}");
        assert!(!canonical_names.contains("陶庭野"), "{visible}");
        assert!(
            visible.contains("name_source: generated_by_writing_tool_policy")
                && visible.contains("景砚安")
                && visible.contains("陶庭野"),
            "{visible}"
        );
    }

    #[test]
    fn mixed_trusted_and_source_less_names_only_preserve_trusted_authority() {
        let mut draft = super::build_initial_creation_draft(
            "mixed-character-authority-repair",
            "fiction",
            "写一部现实主义调查小说，每章2500字，一共5万字",
        )
        .expect("fiction creation draft");
        draft.fiction_characters = vec![
            "name: 陶照声; role: 主角; desire: 公开原始记录; fear: 证据被销毁; bottom_line: 不伪造证据; name_source: generated_by_writing_tool_policy".to_string(),
            "name: 陈默; role: 对手; desire: 掩盖旧事故; fear: 原始记录公开; bottom_line: 不交出原始台账".to_string(),
        ];
        draft.fiction_premise = "陈默试图销毁陶照声找到的原始台账。".to_string();

        CharacterPatch {
            characters: vec![
                CharacterContract {
                    canonical_name: "林远".to_string(),
                    role: "主角".to_string(),
                    arc_end: "公开完整证据链".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "陈默".to_string(),
                    role: "对手".to_string(),
                    desire: "掩盖旧事故".to_string(),
                    fear: "原始记录公开".to_string(),
                    bottom_line: "不交出原始台账".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
        .apply_to_draft(&mut draft);

        let characters = draft
            .fiction_characters
            .iter()
            .map(|line| super::draft_character_line_to_contract(line))
            .collect::<Vec<_>>();
        assert!(characters.iter().any(|character| {
            character.canonical_name == "陶照声"
                && character.name_source == "generated_by_writing_tool_policy"
        }));
        let opponent = characters
            .iter()
            .find(|character| character.role.contains("对手"))
            .expect("governed opponent");
        assert_ne!(opponent.canonical_name, "陈默");
        assert_eq!(opponent.name_source, "generated_by_writing_tool_policy");
        assert!(!draft.fiction_premise.contains("陈默"));
        assert!(draft.fiction_premise.contains(&opponent.canonical_name));
    }

    #[test]
    fn persisted_project_contract_authority_remains_trusted_during_field_repair() {
        let mut draft = super::build_initial_creation_draft(
            "persisted-character-authority-repair",
            "fiction",
            "写一部现实主义小说，每章2500字，一共5万字",
        )
        .expect("fiction creation draft");
        draft.project_path = "data/generated/novels/existing-project".to_string();
        draft.fiction_characters = vec![
            "name: 景砚安; role: 主角; desire: 公开原始记录; fear: 项目记录被删除; bottom_line: 不伪造证据; name_source: contract_authority".to_string(),
        ];

        CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "林默".to_string(),
                role: "主角".to_string(),
                arc_end: "建立公开档案制度".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }
        .apply_to_draft(&mut draft);

        let visible = draft.fiction_characters.join("\n");
        assert!(visible.contains("景砚安"), "{visible}");
        assert!(
            visible.contains("name_source: contract_authority"),
            "{visible}"
        );
        assert!(!visible.contains("name: 林默"), "{visible}");
    }

    #[test]
    fn governance_patch_preserves_typed_plot_authority() {
        let mut base = NovelCreationContract::default();
        base.outline.volumes = vec![VolumeContract {
            title: "潮痕遗嘱".to_string(),
            objective: "查清灯塔熄灭与家族失踪案的联系".to_string(),
            ending_change: "主角确认家族契约正在吞噬守塔人记忆".to_string(),
        }];
        let patch = CreationContractPatch::Governance(GovernancePatch {
            world_rules: vec![
                "灯塔每次熄灭都会让守塔人失去一段与海岛有关的记忆。".to_string(),
                "海雾越过警戒线后，岛上航道会交换真实方位与虚假方位。".to_string(),
                "只有付出一段家族秘密，铜镜才会显示失踪者留下的航迹。".to_string(),
            ],
            ..Default::default()
        });
        let mut applied = base.clone();
        applied.world_rules = match &patch {
            CreationContractPatch::Governance(value) => value.world_rules.clone(),
            _ => unreachable!(),
        };

        patch.merge_applied_scope_into_contract(&mut base, &applied);

        assert_eq!(base.world_rules.len(), 3);
        assert_eq!(base.outline.volumes[0].title, "潮痕遗嘱");
    }

    #[test]
    fn plot_patch_preserves_typed_world_rule_authority() {
        let mut base = NovelCreationContract::default();
        base.world_rules = vec![
            "灯塔每次熄灭都会让守塔人失去一段与海岛有关的记忆。".to_string(),
            "海雾越过警戒线后，岛上航道会交换真实方位与虚假方位。".to_string(),
            "只有付出一段家族秘密，铜镜才会显示失踪者留下的航迹。".to_string(),
        ];
        let volumes = vec![VolumeContract {
            title: "潮痕遗嘱".to_string(),
            objective: "查清灯塔熄灭与家族失踪案的联系".to_string(),
            ending_change: "主角确认家族契约正在吞噬守塔人记忆".to_string(),
        }];
        let patch = CreationContractPatch::Plot(PlotPatch {
            volumes: volumes.clone(),
            ..Default::default()
        });
        let mut applied = base.clone();
        applied.outline.volumes = volumes;

        patch.merge_applied_scope_into_contract(&mut base, &applied);

        assert_eq!(base.outline.volumes[0].title, "潮痕遗嘱");
        assert_eq!(base.world_rules.len(), 3);
    }

    #[test]
    fn plot_patch_does_not_duplicate_typed_volumes_from_raw_outline() {
        let mut draft = super::build_initial_creation_draft(
            "session-plot-raw-outline-round-trip",
            "fiction",
            "从零创作一本玄幻长篇小说，总字数10万字，每章2500字。",
        )
        .expect("draft");
        let volumes = vec![
            VolumeContract {
                title: "断剑初鸣".to_string(),
                objective: "驿卒递送断剑并发现第一段被篡改的战史".to_string(),
                ending_change: "驿卒取得能证明屠村真相的第一件证物".to_string(),
            },
            VolumeContract {
                title: "真相回响".to_string(),
                objective: "驿卒公开完整证据并阻止边境战争重演".to_string(),
                ending_change: "朝廷承认被篡改的战史并重建边境档案".to_string(),
            },
        ];
        let patch = PlotPatch {
            raw_outline: "主线围绕断剑记忆与被篡改的边境战史推进。\n第1卷《断剑初鸣》：驿卒递送断剑并发现第一段被篡改的战史；卷尾变化：驿卒取得能证明屠村真相的第一件证物\n第2卷《真相回响》：驿卒公开完整证据并阻止边境战争重演；卷尾变化：朝廷承认被篡改的战史并重建边境档案".to_string(),
            volumes,
            near_chapters: vec![
                ChapterSeedContract {
                    number: Some(1),
                    goal: "驿卒接收无名断剑".to_string(),
                    expected_turn: "断剑第一次传出战场记忆".to_string(),
                },
                ChapterSeedContract {
                    number: Some(2),
                    goal: "驿卒核对沿途战碑".to_string(),
                    expected_turn: "战碑记载与断剑记忆发生冲突".to_string(),
                },
                ChapterSeedContract {
                    number: Some(3),
                    goal: "驿卒寻找幸存证人".to_string(),
                    expected_turn: "证人交出未被改写的军令".to_string(),
                },
            ],
            ..Default::default()
        };

        patch.apply_to_draft(&mut draft);
        let rebuilt = super::strong_novel_contract_from_visible_creation_draft(&draft);

        assert_eq!(rebuilt.outline.volumes.len(), 2);
        assert_eq!(rebuilt.outline.volumes[0].title, "断剑初鸣");
        assert_eq!(rebuilt.outline.volumes[1].title, "真相回响");
        assert_eq!(
            rebuilt.outline.raw_outline,
            "主线围绕断剑记忆与被篡改的边境战史推进"
        );
    }

    #[test]
    fn decorated_volume_titles_keep_objective_distinct_from_ending_after_draft_round_trip() {
        let mut draft = super::build_initial_creation_draft(
            "session-decorated-volume-round-trip",
            "fiction",
            "从零创作一本修仙长篇小说，总字数10万字，每章2500字。",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 叶昭安; role: 男主; desire: 斩断灵脉; fear: 再次成为宗门耗材; bottom_line: 绝不吞食活人血肉; arc_start: 宗门杂役; arc_end: 凡俗秩序守护者; previous_names: 李尘|钟砚白".to_string(),
            "name: 秦景棠; role: 导师; desire: 重铸上古神兵; fear: 技艺失传; bottom_line: 绝不交付未开锋剑胚; arc_start: 落魄铸剑师; arc_end: 以坐化守住传承".to_string(),
            "name: 梁栖澜; role: 对手; desire: 巩固天才地位; fear: 被无灵根者超越; bottom_line: 绝不向凡人低头; arc_start: 宗门天才; arc_end: 跌落凡尘".to_string(),
        ];
        let raw = r#"{
          "patch_type":"plot_patch",
          "outline":{
            "volumes":[{
              "title":"第一卷《铁骨初成》",
              "objective":"叶昭安在宗门底层通过吞噬废铁淬炼肉身，于外门大比中击败梁栖澜。",
              "ending_change":"叶昭安救下秦景棠并正式脱离杂役籍，踏入修行核心圈。"
            }],
            "near_chapters":[
              {"number":1,"goal":"叶昭安在杂役院发现废铁异变。","expected_turn":"叶昭安确认凡铁能够强化肉身。"},
              {"number":2,"goal":"叶昭安进入矿渣堆寻找更多废铁。","expected_turn":"梁栖澜发现叶昭安力量异常。"},
              {"number":3,"goal":"叶昭安参加外门大比。","expected_turn":"叶昭安取得进入内门矿脉的资格。"}
            ]
          }
        }"#;
        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("plot patch");

        patch.apply_to_draft(&mut draft);
        super::normalize_fiction_creation_draft_after_contract_change(&mut draft);
        super::sanitize_creation_draft_control_noise(&mut draft);
        let rebuilt = super::strong_novel_contract_from_visible_creation_draft(&draft);

        assert_eq!(rebuilt.outline.volumes.len(), 1);
        assert_eq!(rebuilt.outline.volumes[0].title, "铁骨初成");
        assert_eq!(
            rebuilt.outline.volumes[0].objective,
            "叶昭安在宗门底层通过吞噬废铁淬炼肉身，于外门大比中击败梁栖澜"
        );
        assert_eq!(
            rebuilt.outline.volumes[0].ending_change,
            "叶昭安救下秦景棠并正式脱离杂役籍，踏入修行核心圈"
        );
    }

    #[test]
    fn character_patch_preserves_authority_without_guessing_story_references() {
        let mut base = NovelCreationContract {
            premise: "主角林默接手修复古城钟楼。".to_string(),
            characters: vec![
                CharacterContract {
                    canonical_name: "秦砚野".to_string(),
                    role: "主角".to_string(),
                    desire: "查清钟楼真相".to_string(),
                    fear: "重演父亲的失败".to_string(),
                    bottom_line: "不伪造修复记录".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "景予川".to_string(),
                    role: "关键同伴".to_string(),
                    desire: "守住旧工坊".to_string(),
                    fear: "钟楼被拆除".to_string(),
                    bottom_line: "守护未完成的旧物".to_string(),
                    ..Default::default()
                },
            ],
            outline: OutlineContract {
                raw_outline: "第一阶段由林默接手钟楼修复。".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let applied = NovelCreationContract {
            characters: vec![CharacterContract {
                canonical_name: "闻庭川".to_string(),
                role: "关键同伴".to_string(),
                bottom_line: "绝不销毁任何尚未核验的修复记录".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let patch = CreationContractPatch::Characters(CharacterPatch {
            characters: applied.characters.clone(),
            ..Default::default()
        });

        patch.merge_applied_scope_into_contract(&mut base, &applied);

        assert_eq!(base.characters[1].canonical_name, "景予川");
        assert_eq!(base.characters[1].bottom_line, "守护未完成的旧物");
        assert!(base.premise.contains("秦砚野"), "{}", base.premise);
        assert!(!base.premise.contains("林默"), "{}", base.premise);
        assert!(
            base.outline.raw_outline.contains("林默"),
            "{}",
            base.outline.raw_outline
        );
    }

    #[test]
    fn character_patch_rewrites_source_names_across_preserved_contract_scopes() {
        let mut base = NovelCreationContract {
            premise: "港口调度员林远发现货运系统在夜班伪造集装箱去向。".to_string(),
            main_causal_spine: "林远追查系统漏洞，并在卷尾公开完整证据链。".to_string(),
            outline: OutlineContract {
                raw_outline: "第一卷由林远从异常吊装记录追到幕后调度网络。".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let source_character = CharacterContract {
            canonical_name: "林远".to_string(),
            role: "主角".to_string(),
            desire: "查清异常货运记录".to_string(),
            fear: "证据链被彻底销毁".to_string(),
            bottom_line: "不伪造证据换取胜利".to_string(),
            arc_start: "谨慎自保的基层调度员".to_string(),
            arc_end: "敢于公开系统真相的调查者".to_string(),
            ..Default::default()
        };
        let applied = NovelCreationContract {
            characters: vec![CharacterContract {
                canonical_name: "阮知白".to_string(),
                name_source: "generated_by_writing_tool_policy".to_string(),
                ..source_character.clone()
            }],
            ..Default::default()
        };
        let patch = CreationContractPatch::Characters(CharacterPatch {
            characters: vec![source_character],
            ..Default::default()
        });

        let replacements = character_patch_authority_replacements(
            match &patch {
                CreationContractPatch::Characters(patch) => &patch.characters,
                _ => unreachable!(),
            },
            &applied.characters,
        );
        assert_eq!(replacements.get("林远").map(String::as_str), Some("阮知白"));

        patch.merge_applied_scope_into_contract(&mut base, &applied);

        assert_eq!(base.characters[0].canonical_name, "阮知白");
        assert!(
            base.characters[0]
                .previous_names
                .iter()
                .any(|name| name == "林远"),
            "{:?}",
            base.characters[0]
        );
        assert!(base.premise.contains("阮知白"), "{}", base.premise);
        assert!(!base.premise.contains("林远"), "{}", base.premise);
        assert!(base.main_causal_spine.contains("阮知白"));
        assert!(base.outline.raw_outline.contains("阮知白"));
        assert!(base.characters[0]
            .previous_names
            .iter()
            .any(|name| name == "林远"));
    }

    #[test]
    fn character_patch_rewrites_governed_previous_name_when_role_label_is_normalized() {
        let mut base = NovelCreationContract {
            premise: "材料实验员韩清朔复检催化芯。".to_string(),
            outline: OutlineContract {
                raw_outline: "韩清朔在公开听证会上提交复检报告。".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let source = CharacterContract {
            canonical_name: "韩清朔".to_string(),
            role: "关键盟友".to_string(),
            desire: "完成材料复检".to_string(),
            fear: "样本被销毁".to_string(),
            bottom_line: "不伪造光谱数据".to_string(),
            arc_start: "独自核验样本".to_string(),
            arc_end: "公开完整复检报告".to_string(),
            ..Default::default()
        };
        let authority = CharacterContract {
            canonical_name: "沈星岚".to_string(),
            previous_names: vec!["韩清朔".to_string()],
            name_source: "generated_by_writing_tool_policy".to_string(),
            role: "同伴".to_string(),
            ..source.clone()
        };
        let patch = CreationContractPatch::Characters(CharacterPatch {
            characters: vec![source],
            ..Default::default()
        });
        let applied = NovelCreationContract {
            characters: vec![authority],
            ..Default::default()
        };

        patch.merge_applied_scope_into_contract(&mut base, &applied);

        assert!(base.premise.contains("沈星岚"), "{}", base.premise);
        assert!(!base.premise.contains("韩清朔"), "{}", base.premise);
        assert!(base.outline.raw_outline.contains("沈星岚"));
        assert!(!base.outline.raw_outline.contains("韩清朔"));
    }

    #[test]
    fn initial_character_patch_uses_local_names_and_rewrites_story_references() {
        let mut draft = build_initial_creation_draft(
            "session-local-initial-names",
            "fiction",
            "写一部近未来深海考古悬疑小说，每章2500字，共10万字",
        )
        .expect("draft");
        draft.title = "深渊回声".to_string();
        draft.fiction_premise = "林默在海沟遗迹中发现被篡改的航行记录。".to_string();
        let patch = CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "林默".to_string(),
                role: "主角".to_string(),
                desire: "林默要查明航行记录被篡改的原因".to_string(),
                fear: "调查会让潜航队再次失踪".to_string(),
                bottom_line: "不牺牲队员换取遗迹数据".to_string(),
                arc_start: "只相信仪器记录的工程师".to_string(),
                arc_end: "愿意承担公开真相代价的见证者".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        patch.apply_to_draft(&mut draft);

        let character = draft_character_line_to_contract(&draft.fiction_characters[0]);
        assert_ne!(character.canonical_name, "林默");
        assert_eq!(character.name_source, "generated_by_writing_tool_policy");
        assert!(character.previous_names.iter().any(|name| name == "林默"));
        assert!(!draft.fiction_premise.contains("林默"));
        assert!(draft.fiction_premise.contains(&character.canonical_name));
        assert!(!character.desire.contains("林默"));
        assert!(character.desire.contains(&character.canonical_name));
    }

    #[test]
    fn initial_character_name_rewrite_preserves_following_action_phrase() {
        let mut draft = build_initial_creation_draft(
            "session-name-followed-by-action",
            "fiction",
            "写一部科幻小说，每章2500字，共10万字",
        )
        .expect("draft");
        draft.title = "深空灯塔".to_string();
        draft.fiction_ending_direction = "林远手动校准深空灯塔并切断超光速信道".to_string();
        let patch = CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "林远手".to_string(),
                role: "女主".to_string(),
                desire: "修复深空灯塔".to_string(),
                fear: "恒星失控".to_string(),
                bottom_line: "不牺牲地下城平民".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        patch.apply_to_draft(&mut draft);

        let character = draft_character_line_to_contract(&draft.fiction_characters[0]);
        assert_eq!(
            draft.fiction_ending_direction,
            format!(
                "{}手动校准深空灯塔并切断超光速信道",
                character.canonical_name
            )
        );

        let mut legitimate_draft = build_initial_creation_draft(
            "session-legitimate-name-before-action",
            "fiction",
            "写一部都市小说，每章2500字，共10万字",
        )
        .expect("draft");
        legitimate_draft.fiction_ending_direction = "沈知行动身调查旧城档案".to_string();
        let legitimate_character = CharacterContract {
            canonical_name: "沈知行".to_string(),
            role: "男主".to_string(),
            ..Default::default()
        };
        assert_eq!(
            contextual_character_name_source(
                &legitimate_character.canonical_name,
                &legitimate_draft,
                &legitimate_character,
            ),
            None,
            "a legitimate three-character name before an ordinary verb must remain intact"
        );

        legitimate_draft.fiction_premise = "沈无妄因一把断刃卷入江湖纷争。".to_string();
        let legitimate_name_ending_with_modal = CharacterContract {
            canonical_name: "沈无妄".to_string(),
            role: "主角".to_string(),
            ..Default::default()
        };
        assert_eq!(
            contextual_character_name_source(
                &legitimate_name_ending_with_modal.canonical_name,
                &legitimate_draft,
                &legitimate_name_ending_with_modal,
            ),
            None,
            "a complete candidate name must not be shortened merely because the following clause begins with a predicate"
        );
    }

    #[test]
    fn duplicate_model_source_names_do_not_create_one_to_many_replacements() {
        let draft = build_initial_creation_draft(
            "session-ambiguous-source-name",
            "fiction",
            "写一部城市工程悬疑小说，每章2500字，共10万字",
        )
        .expect("draft");
        let mut characters = vec![
            CharacterContract {
                canonical_name: "陈默".to_string(),
                role: "同伴".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "陈默".to_string(),
                role: "对手".to_string(),
                ..Default::default()
            },
        ];

        let governance = govern_character_name_candidates(
            &mut characters,
            &draft,
            BTreeSet::new(),
            "ambiguous-source-slot",
        );

        assert_ne!(characters[0].canonical_name, characters[1].canonical_name);
        assert!(!governance.replacements().contains_key("陈默"));
        assert!(characters
            .iter()
            .all(|character| !character.previous_names.iter().any(|name| name == "陈默")));
    }

    #[test]
    fn established_authority_does_not_reassign_another_characters_previous_name() {
        let mut draft = build_initial_creation_draft(
            "session-locked-previous-name",
            "fiction",
            "写一部城市工程悬疑小说，每章2500字，共10万字",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            CharacterContract {
                canonical_name: "韩知朔".to_string(),
                previous_names: vec!["陈默".to_string()],
                name_source: "generated_by_writing_tool_policy".to_string(),
                role: "同伴".to_string(),
                ..Default::default()
            }
            .to_draft_line(),
            CharacterContract {
                canonical_name: "闻望言".to_string(),
                name_source: "generated_by_writing_tool_policy".to_string(),
                role: "对手".to_string(),
                ..Default::default()
            }
            .to_draft_line(),
        ];
        draft.fiction_premise = "陈默被困在封闭支洞中。".to_string();
        let patch = CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "陈默".to_string(),
                role: "对手".to_string(),
                desire: "掩盖旧工程事故".to_string(),
                fear: "旧名册被公开".to_string(),
                bottom_line: "绝不交出原始施工记录".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        patch.apply_to_draft(&mut draft);

        assert!(
            draft.fiction_premise.contains("韩知朔"),
            "premise={}, characters={:#?}",
            draft.fiction_premise,
            draft.fiction_characters
        );
        assert!(!draft.fiction_premise.contains("闻望言被困"));
        let characters = draft
            .fiction_characters
            .iter()
            .map(|line| draft_character_line_to_contract(line))
            .collect::<Vec<_>>();
        assert!(characters
            .iter()
            .find(|character| character.canonical_name == "闻望言")
            .is_some_and(|character| !character.previous_names.iter().any(|name| name == "陈默")));
    }

    #[test]
    fn initial_character_patch_preserves_name_explicitly_supplied_by_user() {
        let mut draft = build_initial_creation_draft(
            "session-explicit-initial-name",
            "fiction",
            "写一部近未来悬疑小说，主角叫林默，每章2500字，共10万字",
        )
        .expect("draft");
        let patch = CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "林默".to_string(),
                role: "主角".to_string(),
                desire: "查明失踪档案的真相".to_string(),
                fear: "唯一证人再次消失".to_string(),
                bottom_line: "不伪造证据换取结案".to_string(),
                arc_start: "回避旧案的调查员".to_string(),
                arc_end: "公开完整证据链的见证者".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        assert!(
            draft
                .planning_notes
                .iter()
                .any(|note| note == "明确指定角色姓名：林默"),
            "{:#?}",
            draft.planning_notes
        );

        patch.apply_to_draft(&mut draft);

        let character = draft_character_line_to_contract(&draft.fiction_characters[0]);
        assert_eq!(character.canonical_name, "林默");
        assert_eq!(character.name_source, "user");
        assert!(character.previous_names.is_empty());
    }

    #[test]
    fn generated_story_mentions_do_not_claim_user_name_authority() {
        let mut draft = build_initial_creation_draft(
            "session-generated-name-mention",
            "fiction",
            "写一部工业悬疑小说，每章2500字，共10万字，人物姓名由系统生成",
        )
        .expect("draft");
        draft.brief = "林默在废弃水电站追查被篡改的检修记录。".to_string();
        let patch = CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "林默".to_string(),
                role: "主角".to_string(),
                desire: "查清检修记录被篡改的原因".to_string(),
                fear: "旧事故再次发生".to_string(),
                bottom_line: "不伪造数据换取复工".to_string(),
                arc_start: "只相信仪表的工程师".to_string(),
                arc_end: "敢公开完整证据链的人".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        patch.apply_to_draft(&mut draft);

        let character = draft_character_line_to_contract(&draft.fiction_characters[0]);
        assert_ne!(character.canonical_name, "林默");
        assert_eq!(character.name_source, "generated_by_writing_tool_policy");
        assert!(!draft.brief.contains("林默"));
        assert!(draft.brief.contains(&character.canonical_name));
    }

    #[test]
    fn incremental_character_candidates_use_local_governance_and_sync_story_fields() {
        let mut draft = build_initial_creation_draft(
            "session-incremental-local-name",
            "fiction",
            "写一部港口缉私悬疑小说，每章2500字，共10万字，人物姓名由系统生成",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 陶照声; role: 主角; desire: 查清夜班货单; fear: 同伴失踪; bottom_line: 不伪造证据; arc_start: 谨慎的调度员; arc_end: 公开证据的人; name_source: generated_by_writing_tool_policy".to_string(),
            "name: 阮予宁; role: 导师; desire: 守住旧账册; fear: 证人再失踪; bottom_line: 不销毁原始货单; arc_start: 沉默的稽核员; arc_end: 出庭作证的人; name_source: generated_by_writing_tool_policy".to_string(),
        ];
        draft.fiction_premise = "白澈白封锁了零号泊位的夜班货单。".to_string();
        let patch = CharacterPatch {
            characters: vec![CharacterContract {
                canonical_name: "白澈白".to_string(),
                role: "码头承包商".to_string(),
                desire: "掩盖走私货单".to_string(),
                fear: "零号泊位日志公开".to_string(),
                bottom_line: "不让原始铅封进入法庭".to_string(),
                arc_start: "控制夜班装卸的人".to_string(),
                arc_end: "失去港口控制权的人".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        patch.apply_to_draft(&mut draft);

        assert_eq!(draft.fiction_characters.len(), 3);
        let added = draft
            .fiction_characters
            .iter()
            .map(|line| draft_character_line_to_contract(line))
            .find(|character| character.previous_names.iter().any(|name| name == "白澈白"))
            .unwrap_or_else(|| panic!("incremental character: {:#?}", draft.fiction_characters));
        assert_ne!(added.canonical_name, "白澈白");
        assert_eq!(added.name_source, "generated_by_writing_tool_policy");
        assert!(added.previous_names.iter().any(|name| name == "白澈白"));
        assert!(!draft.fiction_premise.contains("白澈白"));
        assert!(draft.fiction_premise.contains(&added.canonical_name));
    }
}
