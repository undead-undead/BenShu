use super::*;

#[derive(Debug, Clone, Default)]
pub(super) struct ContractTermAuthorityView {
    pub(super) character_names: BTreeSet<String>,
    pub(super) character_identity_markers: BTreeMap<String, BTreeSet<String>>,
    pub(super) world_terms: BTreeSet<String>,
    pub(super) organizations_or_places: BTreeSet<String>,
}

impl ContractTermAuthorityView {
    pub(super) fn non_character_terms(&self) -> BTreeSet<String> {
        self.world_terms
            .iter()
            .chain(self.organizations_or_places.iter())
            .cloned()
            .collect()
    }

    pub(super) fn is_non_character_term(&self, value: &str) -> bool {
        let value = value.trim();
        !value.is_empty()
            && (self.world_terms.contains(value) || self.organizations_or_places.contains(value))
    }
}

pub(super) fn contract_term_authority_view(
    manifest: &NovelProjectManifest,
) -> ContractTermAuthorityView {
    let mut view = ContractTermAuthorityView::default();

    if let Some(contract) = &manifest.contract {
        for value in &contract.characters {
            if let Some(name) = character_name_from_authority_text(value) {
                insert_authority_name(&mut view.character_names, &name);
            }
        }
        for value in contract
            .world_rules
            .iter()
            .chain(contract.themes.iter())
            .chain(contract.style_rules.iter())
        {
            insert_world_terms_from_text(&mut view.world_terms, value);
        }
        insert_world_terms_from_text(&mut view.world_terms, &contract.premise);
        insert_world_terms_from_text(&mut view.world_terms, &contract.outline);
        collect_structured_contract_terms(&mut view, &contract.structured_contract_v2);
    }

    for character in &manifest.character_ledger {
        insert_authority_name(&mut view.character_names, &character.canonical_name);
        let markers = character
            .identity_markers
            .iter()
            .map(|marker| marker.trim())
            .filter(|marker| !marker.is_empty())
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>();
        if !markers.is_empty() {
            view.character_identity_markers
                .insert(character.canonical_name.clone(), markers);
        }
        for alias in &character.aliases {
            insert_authority_name(&mut view.character_names, alias);
        }
    }

    collect_structured_contract_terms(&mut view, &manifest.structured_contract_v2);

    if let Some(bible) = &manifest.story_bible {
        for character in &bible.character_ledger {
            insert_authority_name(&mut view.character_names, &character.name);
        }
        for rule in &bible.world_database.rules {
            insert_world_terms_from_text(&mut view.world_terms, &rule.rule);
            insert_world_terms_from_text(&mut view.world_terms, &rule.cost_or_limit);
            insert_world_terms_from_text(&mut view.world_terms, &rule.narrative_effect);
        }
        for entity in bible
            .world_database
            .locations
            .iter()
            .chain(bible.world_database.factions.iter())
            .chain(bible.world_database.resources.iter())
        {
            insert_authority_name(&mut view.organizations_or_places, &entity.name);
            insert_world_terms_from_text(&mut view.world_terms, &entity.role);
            for fact in &entity.known_facts {
                insert_world_terms_from_text(&mut view.world_terms, fact);
            }
        }
        for value in &bible.world_database.constraints {
            insert_world_terms_from_text(&mut view.world_terms, value);
        }
        for arc in &bible.narrative_graph.volume_arcs {
            insert_authority_name(&mut view.organizations_or_places, &arc.title);
            insert_world_terms_from_text(&mut view.world_terms, &arc.goal);
            insert_world_terms_from_text(&mut view.world_terms, &arc.resolves_toward);
        }
    }

    for volume in &manifest.volumes {
        insert_authority_name(&mut view.organizations_or_places, &volume.title);
        insert_world_terms_from_text(&mut view.world_terms, &volume.objective);
        insert_world_terms_from_text(&mut view.world_terms, &volume.ending_change);
        for value in volume
            .key_results
            .iter()
            .chain(volume.must_open.iter())
            .chain(volume.must_payoff.iter())
        {
            insert_world_terms_from_text(&mut view.world_terms, value);
        }
    }

    view.world_terms
        .retain(|term| !view.character_names.contains(term));
    view.organizations_or_places
        .retain(|term| !view.character_names.contains(term));
    view
}

pub(super) fn chapter_character_candidates(chapter: &ChapterRecord) -> BTreeSet<String> {
    let metadata = std::iter::once(chapter.title.as_str())
        .chain(std::iter::once(chapter.summary.as_str()))
        .chain(chapter.key_facts.iter().map(String::as_str))
        .chain(chapter.continuity_updates.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");

    metadata
        .lines()
        .flat_map(declared_character_names_in_line)
        .collect()
}

fn declared_character_names_in_line(line: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for marker in [
        "新增角色",
        "关键人物",
        "新角色",
        "负责人",
        "角色",
        "人物",
        "反派",
        "对手",
        "导师",
        "同伴",
        "盟友",
        "首席",
    ] {
        let Some((before, after)) = line.split_once(marker) else {
            continue;
        };
        if let Some(name) = leading_declared_name(after) {
            names.insert(name);
        }
        if let Some(name) = trailing_declared_name(before) {
            names.insert(name);
        }
        break;
    }
    names
}

fn leading_declared_name(value: &str) -> Option<String> {
    let value = value.trim_start_matches([
        ' ', '\t', ':', '：', '-', '—', '=', '是', '为', '叫', '名', '由',
    ]);
    if value
        .chars()
        .next()
        .is_some_and(|ch| matches!(ch, '的' | '之' | '与' | '和' | '或' | '及' | '、'))
    {
        return None;
    }
    cjk_name_like_candidates(value)
        .into_iter()
        .filter(|candidate| value.starts_with(candidate))
        .max_by_key(|candidate| candidate.chars().count())
}

fn trailing_declared_name(value: &str) -> Option<String> {
    let value = value.trim_end();
    let value = ["担任", "作为", "是", "为"]
        .iter()
        .find_map(|marker| value.strip_suffix(marker))?
        .trim_end_matches([' ', '\t', ':', '：', '-', '—', '=']);
    cjk_name_like_candidates(value)
        .into_iter()
        .filter(|candidate| value.ends_with(candidate))
        .max_by_key(|candidate| candidate.chars().count())
}

#[cfg(test)]
mod tests {
    use super::declared_character_names_in_line;

    #[test]
    fn role_word_followed_by_a_predicate_is_not_a_declared_character() {
        let names = declared_character_names_in_line("秦予安转过身，看到导师正站在不远处。");

        assert!(
            names.is_empty(),
            "predicate text must not become a name: {names:?}"
        );
    }

    #[test]
    fn explicit_role_declaration_still_extracts_the_person_name() {
        let names = declared_character_names_in_line("关键人物：林婉儿，负责核对实验记录。");

        assert!(
            names.contains("林婉儿"),
            "explicit declared name was lost: {names:?}"
        );
    }
}

fn collect_structured_contract_terms(
    view: &mut ContractTermAuthorityView,
    contract: &NovelContractV2,
) {
    for relation in &contract.relationship_ledger {
        for name in &relation.characters {
            insert_authority_name(&mut view.character_names, name);
        }
        for value in [
            relation.arc_type.as_str(),
            relation.relationship_type.as_str(),
            relation.stage.as_str(),
            relation.next_expected_stage.as_str(),
            relation.start_state.as_str(),
            relation.current_state.as_str(),
            relation.desired_end_state.as_str(),
            relation.evidence.as_str(),
        ] {
            insert_world_terms_from_text(&mut view.world_terms, value);
        }
        for value in relation
            .conflicts
            .iter()
            .chain(relation.secrets.iter())
            .chain(relation.turning_points.iter())
        {
            insert_world_terms_from_text(&mut view.world_terms, value);
        }
    }

    for state in &contract.emotional_state_ledger {
        insert_authority_name(&mut view.character_names, &state.character);
    }
    for value in [
        contract.resource_economy.currency.as_str(),
        contract.resource_economy.value_scale.as_str(),
        contract.resource_economy.class_impact.as_str(),
        contract.power_progression.system_name.as_str(),
        contract.social_order.rank_system.as_str(),
        contract.social_order.class_structure.as_str(),
    ] {
        insert_world_terms_from_text(&mut view.world_terms, value);
    }
    for value in contract
        .resource_economy
        .resource_types
        .iter()
        .chain(contract.resource_economy.income_sources.iter())
        .chain(contract.resource_economy.cost_examples.iter())
        .chain(contract.resource_economy.scarcity_rules.iter())
        .chain(contract.resource_economy.trade_rules.iter())
        .chain(contract.power_progression.levels.iter())
        .chain(contract.power_progression.advancement_costs.iter())
        .chain(contract.power_progression.bottlenecks.iter())
        .chain(contract.power_progression.failure_consequences.iter())
        .chain(contract.power_progression.anti_power_creep_rules.iter())
        .chain(contract.social_order.institutions.iter())
        .chain(contract.social_order.exam_or_promotion_rules.iter())
        .chain(contract.social_order.laws.iter())
        .chain(contract.social_order.authority_conflicts.iter())
        .chain(contract.geography_model.regions.iter())
        .chain(contract.geography_model.distance_rules.iter())
        .chain(contract.geography_model.travel_constraints.iter())
        .chain(contract.geography_model.location_changes.iter())
        .chain(contract.time_model.deadline_events.iter())
        .chain(contract.time_model.time_skip_rules.iter())
    {
        insert_world_terms_from_text(&mut view.world_terms, value);
    }
    for location in &contract.geography_model.important_locations {
        insert_authority_name(&mut view.organizations_or_places, &location.name);
        insert_world_terms_from_text(&mut view.world_terms, &location.role);
        for fact in &location.known_facts {
            insert_world_terms_from_text(&mut view.world_terms, fact);
        }
    }
    for artifact in &contract.artifact_ledger {
        insert_authority_name(&mut view.world_terms, &artifact.name);
        for value in [
            artifact.owner.as_str(),
            artifact.origin.as_str(),
            artifact.ability.as_str(),
            artifact.cost_or_limit.as_str(),
            artifact.status.as_str(),
        ] {
            insert_world_terms_from_text(&mut view.world_terms, value);
        }
    }
    for antagonist in &contract.antagonist_pressure.antagonists {
        insert_authority_name(&mut view.character_names, &antagonist.name);
        for value in antagonist
            .resources
            .iter()
            .chain(antagonist.escalation_plan.iter())
        {
            insert_world_terms_from_text(&mut view.world_terms, value);
        }
    }
}

fn character_name_from_authority_text(value: &str) -> Option<String> {
    for label in [
        "canonical_name",
        "name",
        "姓名",
        "名字",
        "主角姓名",
        "角色名",
    ] {
        if let Some(name) = labeled_field_value(value, label) {
            return Some(name);
        }
    }
    value
        .split([';', '；', ',', '，', '\n'])
        .next()
        .map(str::trim)
        .filter(|value| value.chars().count() <= 8)
        .map(ToString::to_string)
}

fn labeled_field_value(value: &str, label: &str) -> Option<String> {
    let index = value.find(label)?;
    let after_label = &value[index + label.len()..];
    let after_separator = after_label
        .trim_start()
        .strip_prefix([':', '：'])
        .unwrap_or(after_label)
        .trim_start();
    let end = after_separator
        .find([';', '；', ',', '，', '\n'])
        .unwrap_or(after_separator.len());
    let candidate = after_separator[..end].trim();
    (!candidate.is_empty()).then(|| candidate.to_string())
}

fn insert_authority_name(target: &mut BTreeSet<String>, value: &str) {
    let value = value.trim();
    let len = value.chars().count();
    if (2..=12).contains(&len) && value.chars().any(is_cjk_unified) {
        target.insert(value.to_string());
    }
}

fn insert_world_terms_from_text(target: &mut BTreeSet<String>, value: &str) {
    for term in cjk_term_candidates(value) {
        target.insert(term);
    }
}

fn cjk_term_candidates(value: &str) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    let mut run = Vec::new();
    for ch in value.chars() {
        if is_cjk_unified(ch) {
            run.push(ch);
            continue;
        }
        insert_cjk_run_terms(&mut terms, &run);
        run.clear();
    }
    insert_cjk_run_terms(&mut terms, &run);
    terms
}

fn insert_cjk_run_terms(target: &mut BTreeSet<String>, run: &[char]) {
    if run.len() < 2 {
        return;
    }
    if run.len() <= 8 {
        target.insert(run.iter().collect());
    }
    for window in 2..=4 {
        if run.len() < window {
            continue;
        }
        for start in 0..=run.len() - window {
            target.insert(run[start..start + window].iter().collect());
        }
    }
}

fn is_cjk_unified(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}
