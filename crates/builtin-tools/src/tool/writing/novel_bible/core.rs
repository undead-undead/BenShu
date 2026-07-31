use super::model::*;
use crate::tool::writing::creation_contract::{
    derive_plot_contract_from_outline_text, draft_character_line_to_contract,
    strip_plot_control_segments_from_outline_text,
};
use crate::tool::writing::creation_contract_model::ChapterSeedContract;
use crate::tool::writing::novel_contract_v2::NovelContractV2;

const STORY_BIBLE_VERSION: &str = "benshu.story_bible.v2";

fn authoritative_structured_contract(contract: &StoryContract) -> &NovelContractV2 {
    contract
        .authority_contract
        .as_ref()
        .filter(|authority| authority.structured.has_authored_content())
        .map(|authority| &authority.structured)
        .unwrap_or(&contract.structured_contract_v2)
}

pub(crate) fn build_story_bible(
    title: &str,
    language: &str,
    genre: &str,
    brief: &str,
    contract: &StoryContract,
    now: String,
) -> StoryBible {
    let title = sanitize_contract_scalar(title);
    let language = sanitize_contract_scalar(language);
    let genre = sanitize_contract_scalar(genre);
    let brief = sanitize_contract_multiline(brief);
    let structured_contract_v2 = authoritative_structured_contract(contract).clone();
    let mut bible = StoryBible {
        schema_version: STORY_BIBLE_VERSION.to_string(),
        title,
        language: language.clone(),
        genre: genre.clone(),
        brief,
        ending_contract: ending_contract_from(contract),
        narrative_graph: narrative_graph_from(contract),
        world_database: world_database_from(contract),
        character_ledger: character_ledger_from(contract),
        hook_ledger: hook_ledger_from_contract(contract),
        genre_governance: genre_governance_profile(&genre, &language),
        theme_ledger: theme_ledger_from(contract),
        timeline: timeline_from(contract),
        source_contract_revision: structured_contract_v2.revision,
        structured_contract_v2,
        chapter_summaries: Vec::new(),
        last_rebuilt_chapter: None,
        updated_at: now,
    };
    ensure_bible_defaults(&mut bible);
    bible
}

pub(crate) fn rebuild_story_bible(
    title: &str,
    language: &str,
    genre: &str,
    brief: &str,
    contract: &StoryContract,
    approved_chapters: &[ApprovedChapterDelta],
    now: String,
) -> StoryBible {
    let mut rebuilt = build_story_bible(title, language, genre, brief, contract, now.clone());
    let last_approved = approved_chapters.iter().map(|chapter| chapter.number).max();

    let mut ordered = approved_chapters.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|chapter| chapter.number);
    for chapter in ordered {
        apply_approved_chapter_delta(&mut rebuilt, chapter, now.clone());
    }
    rebuilt.last_rebuilt_chapter = last_approved;
    rebuilt.updated_at = now;
    rebuilt
}

pub(crate) fn apply_approved_chapter_delta(
    bible: &mut StoryBible,
    chapter: &ApprovedChapterDelta,
    now: String,
) {
    sanitize_character_ledger(&mut bible.character_ledger);
    upsert_chapter_summary(
        &mut bible.chapter_summaries,
        ChapterContinuitySummary {
            chapter_number: chapter.number,
            title: chapter.title.clone(),
            summary: chapter.summary.clone(),
            key_facts: chapter.key_facts.clone(),
            continuity_updates: chapter.continuity_updates.clone(),
            unit_count: chapter.unit_count,
        },
    );
    ensure_character_anchors_from_chapter(bible, chapter);
    apply_typed_state_changes(bible, chapter);
    super::contract_settlement::apply_approved_chapter(
        &mut bible.structured_contract_v2,
        &bible.character_ledger,
        chapter,
    );
    sanitize_character_ledger(&mut bible.character_ledger);
    bible.updated_at = now;
}

fn apply_typed_state_changes(bible: &mut StoryBible, chapter: &ApprovedChapterDelta) {
    use ChapterStateEventType as Event;
    for change in &chapter.state_changes {
        if !matches!(
            change.allowance,
            StateChangeAllowance::Contract | StateChangeAllowance::BoundedIncidental
        ) {
            continue;
        }
        match change.event_type {
            Event::Character | Event::Incidental => {
                if let Some(character) = bible.character_ledger.iter_mut().find(|character| {
                    character.id == change.entity_id || character.name == change.entity_id
                }) {
                    character.current_state = durable_character_state_value(change).to_string();
                }
            }
            Event::Relationship => {}
            Event::World => {
                if let Some(rule) = bible
                    .world_database
                    .rules
                    .iter_mut()
                    .find(|rule| rule.id == change.entity_id)
                {
                    rule.narrative_effect = change.value.clone();
                } else if let Some(entity) = bible
                    .world_database
                    .locations
                    .iter_mut()
                    .chain(bible.world_database.factions.iter_mut())
                    .chain(bible.world_database.resources.iter_mut())
                    .find(|entity| entity.id == change.entity_id || entity.name == change.entity_id)
                {
                    append_capped_unique(&mut entity.known_facts, &change.value, 24);
                }
            }
            Event::Power => {}
            Event::Resource => {
                if let Some(resource) = bible.world_database.resources.iter_mut().find(|resource| {
                    resource.id == change.entity_id || resource.name == change.entity_id
                }) {
                    append_capped_unique(&mut resource.known_facts, &change.value, 24);
                }
            }
            Event::HookSeed | Event::HookAdvance | Event::HookPayOff | Event::HookDefer => {
                apply_typed_hook_change(bible, chapter.number, change);
            }
        }
    }
    for hook in &mut bible.hook_ledger {
        if matches!(hook.status, HookStatus::PaidOff | HookStatus::Dropped) {
            continue;
        }
        if hook
            .deferred_until_chapter
            .or(hook.planned_payoff_chapter)
            .is_some_and(|due| chapter.number > due)
        {
            hook.status = HookStatus::Overdue;
        }
    }
}

fn durable_character_state_value(change: &ChapterStateChange) -> &str {
    if change.allowance == StateChangeAllowance::Contract
        && change.authority_path.trim() == "chapter_contract.new_state_after_chapter"
        && !change.authority_excerpt.trim().is_empty()
    {
        change.authority_excerpt.trim()
    } else {
        change.value.trim()
    }
}

fn apply_typed_hook_change(
    bible: &mut StoryBible,
    chapter_number: usize,
    change: &ChapterStateChange,
) {
    let existing = bible
        .hook_ledger
        .iter_mut()
        .find(|hook| hook.id == change.entity_id || hook.title == change.entity_id);
    match change.event_type {
        ChapterStateEventType::HookSeed => {
            if existing.is_none() {
                bible.hook_ledger.push(HookLedgerEntry {
                    id: change.entity_id.clone(),
                    title: change.value.clone(),
                    introduced_chapter: Some(chapter_number),
                    introduced_when: format!(
                        "chapter {chapter_number} chars {}..{}",
                        change.evidence.start_char, change.evidence.end_char
                    ),
                    status: HookStatus::Seeded,
                    ..Default::default()
                });
            }
        }
        ChapterStateEventType::HookAdvance => {
            if let Some(hook) = existing {
                hook.last_advanced_chapter = Some(chapter_number);
                hook.status = HookStatus::Advancing;
            }
        }
        ChapterStateEventType::HookPayOff => {
            if let Some(hook) = existing {
                hook.payoff_chapter = Some(chapter_number);
                hook.status = HookStatus::PaidOff;
            }
        }
        ChapterStateEventType::HookDefer => {
            if let Some(hook) = existing {
                hook.last_advanced_chapter = Some(chapter_number);
                hook.deferred_until_chapter = change.defer_until_chapter;
                hook.status = HookStatus::Deferred;
            }
        }
        _ => {}
    }
}

fn append_capped_unique(values: &mut Vec<String>, value: &str, max_len: usize) {
    let value = value.trim();
    if value.is_empty() || values.iter().any(|existing| existing == value) {
        return;
    }
    values.push(value.to_string());
    if values.len() > max_len {
        values.drain(0..values.len() - max_len);
    }
}

pub(crate) fn upsert_planned_chapter_goal(
    bible: &mut StoryBible,
    chapter_number: usize,
    goal: &str,
    irreversible_event: &str,
    chapter_function: &str,
) {
    let goal = sanitize_contract_multiline(goal);
    if chapter_number == 0 || goal.trim().is_empty() {
        return;
    }
    let moves_toward_ending = sanitize_contract_multiline(first_non_empty(&[
        irreversible_event,
        chapter_function,
        bible.ending_contract.desired_resolution.as_str(),
        bible.narrative_graph.global_spine.as_str(),
    ]));
    let planned = ChapterGoal {
        chapter_number,
        goal,
        depends_on: if chapter_number > 1 {
            vec![chapter_number - 1]
        } else {
            Vec::new()
        },
        moves_toward_ending,
    };
    if let Some(existing) = bible
        .narrative_graph
        .chapter_goals
        .iter_mut()
        .find(|existing| existing.chapter_number == chapter_number)
    {
        // The project contract/story bible owns the chapter goal. A generated
        // execution package may fill a missing goal, but retries must not wrap
        // and overwrite the same authority text on every attempt.
        if existing.goal.trim().is_empty() {
            existing.goal = planned.goal;
        }
        if existing.moves_toward_ending.trim().is_empty() {
            existing.moves_toward_ending = planned.moves_toward_ending;
        }
        if existing.depends_on.is_empty() {
            existing.depends_on = planned.depends_on;
        }
    } else {
        bible.narrative_graph.chapter_goals.push(planned);
    }
    bible
        .narrative_graph
        .chapter_goals
        .sort_by_key(|entry| entry.chapter_number);
}

pub(crate) fn story_bible_audit(bible: Option<&StoryBible>) -> (Vec<String>, Vec<String>) {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    let Some(bible) = bible else {
        blockers.push(
            "Story bible is missing. Create it from the story contract before long-form drafting."
                .to_string(),
        );
        return (blockers, warnings);
    };
    if bible.ending_contract.desired_resolution.trim().is_empty() {
        blockers.push("Story bible has no ending contract desired_resolution.".to_string());
    }
    if bible.ending_contract.final_state.trim().is_empty() {
        blockers.push("Story bible has no ending contract final_state.".to_string());
    }
    if bible.character_ledger.is_empty() {
        blockers.push("Story bible has no character anchors.".to_string());
    } else if bible
        .character_ledger
        .iter()
        .all(character_anchor_core_is_missing)
    {
        blockers.push(
            "Story bible character anchors lack explicit desire/fear/bottom-line core.".to_string(),
        );
    }
    if bible.world_database.rules.is_empty() {
        blockers.push("Story bible has no world rules database.".to_string());
    }
    if bible.narrative_graph.global_spine.trim().is_empty() {
        blockers.push("Story bible has no global narrative spine.".to_string());
    }
    if bible.genre_governance.control_axes.is_empty() {
        blockers.push("Story bible has no genre governance axes.".to_string());
    }
    let structured_warnings = structured_contract_warnings(&bible.structured_contract_v2);
    warnings.extend(structured_warnings);
    let open_hooks = bible
        .hook_ledger
        .iter()
        .filter(|hook| !matches!(hook.status, HookStatus::PaidOff | HookStatus::Dropped))
        .count();
    if open_hooks > 80 {
        warnings.push(format!(
            "Story bible has {open_hooks} open hooks; consider payoff, archive, or consolidation."
        ));
    }
    (blockers, warnings)
}

pub(crate) fn story_contract_blockers(contract: &StoryContract) -> Vec<String> {
    let mut blockers = Vec::new();
    if contract.premise.trim().is_empty() {
        blockers.push("Story contract premise is missing.".to_string());
    }
    if contract.outline.trim().is_empty() {
        blockers.push("Story contract outline/finale direction is missing.".to_string());
    }
    if contract.themes.iter().all(|item| item.trim().is_empty()) {
        blockers.push("Story contract themes are missing.".to_string());
    }
    if contract
        .characters
        .iter()
        .all(|item| item.trim().is_empty())
    {
        blockers.push("Story contract characters are missing.".to_string());
    } else {
        let named = contract
            .characters
            .iter()
            .filter_map(|item| character_name(item))
            .filter(|name| stable_character_name(name))
            .count();
        if named == 0 {
            blockers.push("Story contract has no stable named character anchor.".to_string());
        }
        if !contract
            .characters
            .iter()
            .any(|item| character_contract_item_has_core_anchor(item))
        {
            blockers.push(
                "Story contract characters need at least one explicit desire/fear/bottom-line anchor."
                    .to_string(),
            );
        }
    }
    if contract
        .world_rules
        .iter()
        .all(|item| item.trim().is_empty())
    {
        blockers.push("Story contract world_rules are missing.".to_string());
    }
    if contract
        .style_rules
        .iter()
        .all(|item| item.trim().is_empty())
    {
        blockers.push("Story contract style_rules are missing.".to_string());
    }
    blockers
}

fn structured_contract_warnings(contract: &NovelContractV2) -> Vec<String> {
    let mut warnings = Vec::new();
    if contract
        .emotional_contract
        .emotional_promise
        .trim()
        .is_empty()
    {
        warnings.push("Structured contract v2 has no emotional promise.".to_string());
    }
    if contract.relationship_ledger.is_empty() {
        warnings.push("Structured contract v2 has no relationship ledger.".to_string());
    }
    if contract.payoff_matrix.is_empty() {
        warnings.push("Structured contract v2 has no payoff matrix.".to_string());
    }
    if contract.narration_contract.pov.trim().is_empty()
        && contract.narration_contract.chapter_pacing.trim().is_empty()
    {
        warnings.push("Structured contract v2 has no narration contract.".to_string());
    }
    if contract.time_model.calendar.trim().is_empty()
        && contract.time_model.story_start_time.trim().is_empty()
    {
        warnings.push("Structured contract v2 has no time model.".to_string());
    }
    if contract
        .antagonist_pressure
        .primary_pressure
        .trim()
        .is_empty()
        && contract.antagonist_pressure.antagonists.is_empty()
    {
        warnings.push("Structured contract v2 has no antagonist/external pressure.".to_string());
    }
    warnings
}

pub(crate) fn story_bible_completion_blockers(bible: Option<&StoryBible>) -> Vec<String> {
    let Some(bible) = bible else {
        return vec!["Story bible is missing before project completion.".to_string()];
    };
    let mut blockers = Vec::new();
    if bible.ending_contract.final_state.trim().is_empty()
        || bible.ending_contract.desired_resolution.trim().is_empty()
    {
        blockers.push("Ending contract is incomplete.".to_string());
    }
    if bible.chapter_summaries.is_empty() {
        blockers.push("No approved chapter summaries are recorded in story bible.".to_string());
    }
    if bible.character_ledger.is_empty() {
        blockers.push("Character ledger is empty at completion gate.".to_string());
    }
    if bible.world_database.rules.is_empty() {
        blockers.push("World database is empty at completion gate.".to_string());
    }
    let unresolved = story_bible_completion_debts(Some(bible))
        .into_iter()
        .map(|debt| preview(&debt.title, 80))
        .collect::<Vec<_>>();
    if !unresolved.is_empty() {
        blockers.push(format!(
            "Key hook ledger still has unresolved debts: {}",
            unresolved.join("; ")
        ));
    }
    blockers
}

pub(crate) fn story_bible_completion_debts(bible: Option<&StoryBible>) -> Vec<CompletionDebt> {
    let Some(bible) = bible else {
        return Vec::new();
    };
    bible
        .hook_ledger
        .iter()
        .filter(|hook| !matches!(hook.status, HookStatus::PaidOff | HookStatus::Dropped))
        .filter(|hook| hook_is_completion_relevant(bible, hook))
        .map(|hook| CompletionDebt {
            id: hook.id.clone(),
            title: preview(&hook.title, 80),
        })
        .take(12)
        .collect()
}

fn ending_contract_from(contract: &StoryContract) -> EndingContract {
    if let Some(authority) = contract.authority_contract.as_ref() {
        return EndingContract {
            desired_resolution: sanitize_contract_multiline(&authority.ending.desired_resolution),
            final_state: sanitize_contract_multiline(&authority.ending.final_state),
            open_questions_allowed: authority
                .ending
                .allowed_open_questions
                .iter()
                .filter_map(|value| sanitize_contract_item(value))
                .collect(),
            must_resolve: authority
                .ending
                .must_resolve
                .iter()
                .filter_map(|value| sanitize_contract_item(value))
                .collect(),
        };
    }
    let premise = sanitize_contract_multiline(&contract.premise);
    let desired_resolution = contract_ending_resolution(contract);
    let final_state = first_non_empty(&[desired_resolution.as_str(), premise.as_str()])
        .trim()
        .to_string();
    EndingContract {
        desired_resolution,
        final_state,
        open_questions_allowed: Vec::new(),
        must_resolve: contract
            .themes
            .iter()
            .filter_map(|theme| sanitize_contract_item(theme))
            .map(|theme| format!("Resolve the thematic promise: {}", theme.trim()))
            .collect(),
    }
}

fn narrative_graph_from(contract: &StoryContract) -> NarrativeGraph {
    NarrativeGraph {
        global_spine: contract_global_spine(contract),
        reverse_design_notes: reverse_design_notes(contract),
        volume_arcs: volume_arcs_from_contract(contract),
        chapter_goals: chapter_goals_from_contract(contract),
    }
}

fn chapter_goals_from_contract(contract: &StoryContract) -> Vec<ChapterGoal> {
    let near_chapters = contract
        .authority_contract
        .as_ref()
        .filter(|authority| !authority.outline.near_chapters.is_empty())
        .map(|authority| authority.outline.near_chapters.clone())
        .unwrap_or_else(|| derive_plot_contract_from_outline_text(&contract.outline).near_chapters);
    let mut goals = Vec::new();
    for seed in near_chapters {
        if let Some(goal) = chapter_goal_from_seed(seed, contract) {
            if !goals
                .iter()
                .any(|existing: &ChapterGoal| existing.chapter_number == goal.chapter_number)
            {
                goals.push(goal);
            }
        }
    }
    goals.sort_by_key(|goal| goal.chapter_number);
    goals
}

fn chapter_goal_from_seed(
    seed: ChapterSeedContract,
    contract: &StoryContract,
) -> Option<ChapterGoal> {
    let chapter_number = seed.number?;
    let goal = sanitize_contract_multiline(&seed.goal);
    if goal.trim().is_empty() || contract_line_is_surface_noise(&goal) {
        return None;
    }
    let ending_resolution = contract_ending_resolution(contract);
    let global_spine = contract_global_spine(contract);
    let moves_toward_ending = sanitize_contract_multiline(first_non_empty(&[
        seed.expected_turn.as_str(),
        ending_resolution.as_str(),
        global_spine.as_str(),
        contract.premise.as_str(),
    ]));
    Some(ChapterGoal {
        chapter_number,
        goal,
        depends_on: if chapter_number > 1 {
            vec![chapter_number - 1]
        } else {
            Vec::new()
        },
        moves_toward_ending,
    })
}

pub(crate) fn volume_arcs_from_contract(contract: &StoryContract) -> Vec<NarrativeArc> {
    if let Some(authority) = contract.authority_contract.as_ref() {
        if !authority.outline.volumes.is_empty() {
            let legacy_ranges = volume_arcs_from_outline_text(contract);
            return authority
                .outline
                .volumes
                .iter()
                .enumerate()
                .map(|(index, volume)| {
                    let range = legacy_ranges.get(index);
                    NarrativeArc {
                        id: format!("volume-{:04}", index + 1),
                        title: sanitize_contract_scalar(&volume.title),
                        goal: sanitize_contract_multiline(&volume.objective),
                        start_chapter: range.and_then(|arc| arc.start_chapter),
                        end_chapter: range.and_then(|arc| arc.end_chapter),
                        resolves_toward: sanitize_contract_multiline(&volume.ending_change),
                    }
                })
                .collect();
        }
    }
    volume_arcs_from_outline_text(contract)
}

fn volume_arcs_from_outline_text(contract: &StoryContract) -> Vec<NarrativeArc> {
    let mut arcs = Vec::new();
    let mut current: Option<VolumeArcDraft> = None;

    for line in contract.outline.lines() {
        let Some(header) = volume_header_from_line(line) else {
            if let Some(current) = current.as_mut() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && volume_goal_line_belongs_to_current_arc(trimmed) {
                    if !current.goal.is_empty() {
                        current.goal.push(' ');
                    }
                    current.goal.push_str(trimmed);
                }
            }
            continue;
        };

        if let Some(previous) = current.take() {
            arcs.push(previous.into_arc(arcs.len() + 1, contract));
        }
        current = Some(header);
    }

    if let Some(last) = current.take() {
        arcs.push(last.into_arc(arcs.len() + 1, contract));
    }

    if arcs.is_empty() {
        arcs.push(default_volume_arc(contract));
    }
    normalize_volume_arc_ranges(&mut arcs);
    arcs
}

#[derive(Debug, Clone)]
struct VolumeArcDraft {
    title: String,
    goal: String,
    start_chapter: Option<usize>,
    end_chapter: Option<usize>,
}

impl VolumeArcDraft {
    fn into_arc(self, index: usize, contract: &StoryContract) -> NarrativeArc {
        let premise = sanitize_contract_multiline(&contract.premise);
        let spine = contract_global_spine(contract);
        let fallback_goal =
            first_non_empty(&[self.goal.as_str(), spine.as_str(), premise.as_str()]);
        let clean_title = sanitize_contract_scalar(&self.title);
        NarrativeArc {
            id: format!("volume-{index:04}"),
            title: first_non_empty(&[clean_title.as_str(), default_volume_title(contract)])
                .to_string(),
            goal: sanitize_contract_multiline(fallback_goal),
            start_chapter: self.start_chapter,
            end_chapter: self.end_chapter,
            resolves_toward: sanitize_contract_multiline(first_non_empty(&[
                self.goal.as_str(),
                spine.as_str(),
                premise.as_str(),
            ])),
        }
    }
}

fn default_volume_arc(contract: &StoryContract) -> NarrativeArc {
    let premise = sanitize_contract_multiline(&contract.premise);
    let spine = contract_global_spine(contract);
    NarrativeArc {
        id: "volume-0001".to_string(),
        title: default_volume_title(contract).to_string(),
        goal: first_non_empty(&[spine.as_str(), premise.as_str()]).to_string(),
        start_chapter: Some(1),
        end_chapter: None,
        resolves_toward: first_non_empty(&[spine.as_str(), premise.as_str()]).to_string(),
    }
}

fn contract_ending_resolution(contract: &StoryContract) -> String {
    if let Some(authority) = contract.authority_contract.as_ref() {
        return first_non_empty(&[
            authority.ending.desired_resolution.as_str(),
            authority.ending.final_state.as_str(),
        ])
        .trim()
        .to_string();
    }
    let labeled = labeled_contract_value(
        &contract.outline,
        &[
            "终局方向",
            "终局",
            "结局方向",
            "结局",
            "最终状态",
            "ending",
            "finale",
            "final state",
            "desired resolution",
        ],
    );
    let tail = first_clean_non_plan_line_from_tail(&contract.outline);
    let premise = sanitize_contract_multiline(&contract.premise);
    first_non_empty(&[labeled.as_str(), tail.as_str(), premise.as_str()]).to_string()
}

fn contract_global_spine(contract: &StoryContract) -> String {
    if let Some(authority) = contract.authority_contract.as_ref() {
        let spine = authority.main_causal_spine.trim();
        if !spine.is_empty() {
            return spine.to_string();
        }
    }
    let labeled = labeled_contract_value(
        &contract.outline,
        &[
            "总主线因果链",
            "主线因果链",
            "主线",
            "全书主线",
            "因果链",
            "global spine",
            "causal spine",
            "main spine",
        ],
    );
    let premise = sanitize_contract_multiline(&contract.premise);
    first_non_empty(&[labeled.as_str(), premise.as_str()]).to_string()
}

fn labeled_contract_value(text: &str, labels: &[&str]) -> String {
    for raw_line in text.replace('\r', "\n").lines() {
        let line = normalize_outline_line(raw_line);
        if line.is_empty() {
            continue;
        }
        for label in labels {
            if let Some(value) = line_value_after_label(&line, label) {
                let cleaned = sanitize_contract_multiline(&value);
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
        }
    }
    String::new()
}

fn line_value_after_label(line: &str, label: &str) -> Option<String> {
    let lowered = line.to_ascii_lowercase();
    let label_lowered = label.to_ascii_lowercase();
    let index = lowered.find(&label_lowered)?;
    let after = line[index + label.len()..]
        .trim()
        .trim_start_matches(['：', ':', '-', '—', ' '])
        .trim();
    if after.is_empty() {
        return None;
    }
    Some(
        after
            .split(['；', ';', '\n'])
            .next()
            .unwrap_or(after)
            .trim()
            .trim_end_matches(['。', '.', '，', ','])
            .trim()
            .to_string(),
    )
}

fn first_clean_non_plan_line_from_tail(text: &str) -> String {
    strip_plot_control_segments_from_outline_text(text)
        .lines()
        .rev()
        .map(normalize_outline_line)
        .find(|line| {
            !line.is_empty()
                && !contract_line_is_surface_noise(line)
                && !line_looks_like_explicit_chapter_plan_for_volume_parser(line)
        })
        .unwrap_or_default()
}

fn default_volume_title(contract: &StoryContract) -> &'static str {
    if text_looks_cjk(&format!(
        "{}\n{}\n{}",
        contract.premise, contract.outline, contract.updated_at
    )) {
        "开局卷"
    } else {
        "Opening arc"
    }
}

fn text_looks_cjk(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

fn volume_header_from_line(line: &str) -> Option<VolumeArcDraft> {
    let normalized = normalize_outline_line(line);
    if normalized.is_empty()
        || normalized.contains("分卷大纲")
        || normalized.contains("volume outline")
    {
        return None;
    }
    if !line_looks_like_volume_header(&normalized) {
        return None;
    }
    let (head, tail) = split_volume_header_goal(&normalized);
    let (start_chapter, end_chapter) = chapter_range_from_text(&normalized);
    let title = volume_title_from_header_parts(head, tail);
    Some(VolumeArcDraft {
        title,
        goal: tail.trim().to_string(),
        start_chapter,
        end_chapter,
    })
}

fn volume_goal_line_belongs_to_current_arc(line: &str) -> bool {
    let normalized = normalize_outline_line(line);
    if normalized.is_empty() {
        return false;
    }
    if contract_line_is_surface_noise(&normalized) {
        return false;
    }
    if line_looks_like_explicit_chapter_plan_for_volume_parser(&normalized) {
        return false;
    }
    let lowered = normalized.to_ascii_lowercase();
    let section_markers = [
        "逐章规划",
        "章节规划",
        "章节大纲",
        "质量合同",
        "导出规范",
        "可修改说明",
        "近期章节包",
        "章节审稿",
        "审稿",
        "伏笔债务",
        "上下文包",
        "执行包",
        "导出",
        "全书推进依据",
        "书名",
        "书名理由",
        "终局方向",
        "主角弧线",
        "世界观意象",
        "总主线因果链",
        "chapter outline",
        "chapter plan",
        "quality contract",
        "export",
        "recent chapter",
        "audit",
        "hook debt",
        "context package",
        "execution package",
    ];
    !section_markers
        .iter()
        .any(|marker| normalized.contains(marker) || lowered.contains(marker))
}

fn normalize_outline_line(line: &str) -> String {
    line.trim()
        .trim_start_matches('#')
        .trim_start_matches(['-', '*', '+'])
        .trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '.' || ch == ')')
        .trim()
        .trim_matches('`')
        .trim()
        .to_string()
}

fn line_looks_like_volume_header(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    if lowered.starts_with("volume ") || lowered.starts_with("book ") || line.starts_with('卷') {
        return true;
    }
    if line.contains("卷名") || line.contains("卷：") || line.contains("卷:") {
        return true;
    }
    chinese_ordinal_volume_header_prefix(line).is_some()
}

fn split_volume_header_goal(line: &str) -> (&str, &str) {
    for delimiter in ['：', ':', '；', ';'] {
        if let Some((head, tail)) = line.split_once(delimiter) {
            return (head.trim(), tail.trim());
        }
    }
    (line.trim(), "")
}

fn volume_title_from_header_parts(head: &str, tail: &str) -> String {
    let head_title = clean_volume_title(head);
    if volume_header_title_is_generic(&head_title) {
        let tail_title = clean_volume_title(
            tail.split(['（', '(', '：', ':', '；', ';'])
                .next()
                .unwrap_or(tail),
        );
        if !tail_title.trim().is_empty() {
            return tail_title;
        }
    }
    head_title
}

fn volume_header_title_is_generic(title: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lowered = trimmed.to_ascii_lowercase();
    lowered.starts_with("volume ")
        || lowered.starts_with("book ")
        || trimmed == "卷"
        || chinese_ordinal_volume_header_prefix(trimmed)
            .is_some_and(|prefix| prefix.trim() == trimmed)
}

fn chinese_ordinal_volume_header_prefix(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with('第') {
        return None;
    }
    let mut end = 0usize;
    let mut saw_number = false;
    for (index, ch) in trimmed.char_indices() {
        if index == 0 {
            continue;
        }
        if is_cjk_number_char(ch) || ch.is_ascii_digit() {
            saw_number = true;
            continue;
        }
        if saw_number && ch == '卷' {
            end = index + ch.len_utf8();
            break;
        }
        return None;
    }
    if end == 0 {
        return None;
    }
    Some(&trimmed[..end])
}

fn is_cjk_number_char(ch: char) -> bool {
    matches!(
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
}

fn line_looks_like_explicit_chapter_plan_for_volume_parser(line: &str) -> bool {
    let trimmed = line
        .trim()
        .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'));
    if let Some(index) = trimmed.find('第') {
        if index <= 8 {
            let mut chars = trimmed[index..].chars();
            let _ = chars.next();
            let mut saw_number = false;
            for ch in chars {
                if ch.is_whitespace() {
                    continue;
                }
                if is_cjk_number_char(ch) || ch.is_ascii_digit() {
                    saw_number = true;
                    continue;
                }
                return saw_number && ch == '章';
            }
        }
    }
    false
}

fn clean_volume_title(value: &str) -> String {
    let mut value = value
        .trim()
        .trim_matches(['《', '》', '"', '\'', '“', '”'])
        .trim()
        .to_string();
    if let Some(prefix) = chinese_ordinal_volume_header_prefix(&value) {
        value = value[prefix.len()..]
            .trim()
            .trim_matches(['《', '》', '"', '\'', '“', '”'])
            .trim()
            .to_string();
    }
    for marker in ["》", "（", "(", "阶段目标", "目标：", "目标:"] {
        if let Some(index) = value.find(marker) {
            value = value[..index].trim().to_string();
        }
    }
    value = value
        .trim()
        .trim_matches(['《', '》', '"', '\'', '“', '”'])
        .trim()
        .to_string();
    value
}

fn chapter_range_from_text(text: &str) -> (Option<usize>, Option<usize>) {
    if !text.contains('章') && !text.to_ascii_lowercase().contains("chapter") {
        return (None, None);
    }
    let numbers = extract_numbers(text);
    match numbers.as_slice() {
        [] => (None, None),
        [start] => (Some(*start), None),
        [start, end, ..] => {
            if end >= start {
                (Some(*start), Some(*end))
            } else {
                (Some(*start), None)
            }
        }
    }
}

fn extract_numbers(text: &str) -> Vec<usize> {
    let mut numbers = Vec::new();
    let mut run = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            run.push(ch);
            continue;
        }
        if !run.is_empty() {
            if let Ok(number) = run.parse::<usize>() {
                numbers.push(number);
            }
            run.clear();
        }
    }
    if !run.is_empty() {
        if let Ok(number) = run.parse::<usize>() {
            numbers.push(number);
        }
    }
    numbers
}

fn normalize_volume_arc_ranges(arcs: &mut [NarrativeArc]) {
    let mut next_start = 1usize;
    for arc in arcs {
        let start = arc.start_chapter.unwrap_or(next_start).max(1);
        arc.start_chapter = Some(start);
        if let Some(end) = arc.end_chapter {
            if end < start {
                arc.end_chapter = None;
            } else {
                next_start = end.saturating_add(1);
            }
        } else {
            next_start = start.saturating_add(1);
        }
    }
}

fn reverse_design_notes(contract: &StoryContract) -> Vec<String> {
    let premise = sanitize_contract_multiline(&contract.premise);
    let ending_resolution = contract_ending_resolution(contract);
    let ending = first_non_empty(&[ending_resolution.as_str(), premise.as_str()]);
    vec![
        "Start from the intended ending before selecting chapter events.".to_string(),
        format!("Every volume and chapter should move causally toward: {ending}"),
        "Do not introduce a major promise unless the hook ledger can track its payoff.".to_string(),
    ]
}

fn world_database_from(contract: &StoryContract) -> WorldDatabase {
    let world_rules = contract
        .authority_contract
        .as_ref()
        .map(|authority| authority.world_rules.as_slice())
        .unwrap_or(contract.world_rules.as_slice());
    let constraints = contract
        .authority_contract
        .as_ref()
        .map(|authority| authority.must_avoid.as_slice())
        .unwrap_or(contract.must_avoid.as_slice());
    let rules = world_rules
        .iter()
        .filter_map(|rule| sanitize_contract_item(rule))
        .enumerate()
        .map(|(index, rule)| WorldRule {
            id: format!("world-rule-{:04}", index + 1),
            rule,
            cost_or_limit: "Must remain consistent; changes require an explicit story event."
                .to_string(),
            narrative_effect: "Controls available choices, conflict cost, and credible escalation."
                .to_string(),
        })
        .collect::<Vec<_>>();
    WorldDatabase {
        rules,
        locations: Vec::new(),
        factions: Vec::new(),
        resources: Vec::new(),
        constraints: constraints
            .iter()
            .filter_map(|item| sanitize_contract_item(item))
            .collect(),
    }
}

fn character_ledger_from(contract: &StoryContract) -> Vec<CharacterAnchor> {
    if let Some(authority) = contract.authority_contract.as_ref() {
        let anchors = authority
            .characters
            .iter()
            .filter(|character| stable_character_name(&character.canonical_name))
            .map(|character| CharacterAnchor {
                id: character.character_id.clone(),
                name: character.canonical_name.clone(),
                role: character.role.clone(),
                desire: character.desire.clone(),
                fear: character.fear.clone(),
                bottom_line: character.bottom_line.clone(),
                wound_or_flaw: String::new(),
                current_state:
                    "Established by project contract; update only from approved chapters."
                        .to_string(),
                relationship_anchors: Vec::new(),
            })
            .collect::<Vec<_>>();
        if !anchors.is_empty() {
            return anchors;
        }
    }
    contract
        .characters
        .iter()
        .filter_map(|character| sanitize_contract_item(character))
        .filter(|character| contract_character_item_looks_like_anchor(character))
        .filter_map(|character| {
            let typed = draft_character_line_to_contract(&character);
            let name = if typed.canonical_name.trim().is_empty() {
                character_name(&character)?
            } else {
                typed.canonical_name.clone()
            };
            if !stable_character_name(&name) {
                return None;
            }
            Some(CharacterAnchor {
                id: typed.character_id,
                name,
                role: first_non_empty(&[typed.role.as_str(), "角色"]).to_string(),
                desire: typed.desire,
                fear: typed.fear,
                bottom_line: typed.bottom_line,
                wound_or_flaw: anchor_field_or_default(
                    &character,
                    &["flaw:", "weakness:", "弱点：", "创伤："],
                    "",
                ),
                current_state:
                    "Established by project contract; update only from approved chapters."
                        .to_string(),
                relationship_anchors: Vec::new(),
            })
        })
        .enumerate()
        .map(|(index, mut anchor)| {
            if anchor.id.trim().is_empty() {
                anchor.id = format!("character-{:04}", index + 1);
            }
            anchor
        })
        .collect()
}

fn hook_ledger_from_contract(contract: &StoryContract) -> Vec<HookLedgerEntry> {
    let mut hooks = contract
        .outline
        .lines()
        .filter_map(sanitize_contract_item)
        .filter(|line| hook_like_text(line))
        .enumerate()
        .map(|(index, line)| HookLedgerEntry {
            id: format!("hook-{:04}", index + 1),
            title: preview(&line, 80),
            introduced_chapter: None,
            introduced_when: "project_setup".to_string(),
            knowers: Vec::new(),
            reader_knows: preview(&line, 180),
            planned_payoff_window: "Before the natural ending; refine during chapter planning."
                .to_string(),
            planned_payoff_chapter: None,
            payoff_chapter: None,
            last_advanced_chapter: None,
            deferred_until_chapter: None,
            emotional_effect: "Curiosity, tension, or delayed satisfaction.".to_string(),
            status: HookStatus::Open,
            evidence: vec![line.trim().to_string()],
        })
        .collect::<Vec<_>>();
    for payoff in &authoritative_structured_contract(contract).payoff_matrix {
        let promise = sanitize_contract_multiline(&payoff.promise);
        let target = sanitize_contract_multiline(&payoff.payoff_target);
        if promise.trim().is_empty() && target.trim().is_empty() {
            continue;
        }
        if hooks.iter().any(|hook| {
            hook.title == promise
                || hook.evidence.iter().any(|evidence| evidence == &promise)
                || (!target.trim().is_empty()
                    && hook.evidence.iter().any(|evidence| evidence == &target))
        }) {
            continue;
        }
        let title = first_non_empty(&[promise.as_str(), target.as_str()]).to_string();
        let mut evidence = Vec::new();
        if !promise.trim().is_empty() {
            evidence.push(promise);
        }
        if !target.trim().is_empty() && !evidence.contains(&target) {
            evidence.push(target.clone());
        }
        hooks.push(HookLedgerEntry {
            id: format!("hook-{:04}", hooks.len() + 1),
            title: preview(&title, 80),
            introduced_chapter: payoff.introduced_chapter,
            introduced_when: "project_contract".to_string(),
            knowers: Vec::new(),
            reader_knows: preview(&title, 180),
            planned_payoff_window: if let Some(chapter) = payoff.payoff_chapter {
                format!("chapter-{chapter:04}")
            } else {
                "Before the natural ending; refine during chapter planning.".to_string()
            },
            planned_payoff_chapter: payoff.payoff_chapter,
            payoff_chapter: payoff
                .status
                .eq_ignore_ascii_case("paid_off")
                .then_some(payoff.payoff_chapter)
                .flatten(),
            last_advanced_chapter: None,
            deferred_until_chapter: None,
            emotional_effect: "Fulfil the contract promise with a visible consequence.".to_string(),
            status: if payoff.status.eq_ignore_ascii_case("paid_off") {
                HookStatus::PaidOff
            } else {
                HookStatus::Open
            },
            evidence,
        });
    }
    let ending = ending_contract_from(contract);
    let ending_obligations = std::iter::once((
        "ending-desired-resolution".to_string(),
        ending.desired_resolution,
    ))
    .chain(std::iter::once((
        "ending-final-state".to_string(),
        ending.final_state,
    )))
    .chain(
        ending
            .must_resolve
            .into_iter()
            .enumerate()
            .map(|(index, obligation)| {
                (format!("ending-must-resolve-{:04}", index + 1), obligation)
            }),
    );
    for (id, obligation) in ending_obligations {
        let obligation = sanitize_contract_multiline(&obligation);
        if obligation.trim().is_empty() || hooks.iter().any(|hook| hook.id == id) {
            continue;
        }
        hooks.push(HookLedgerEntry {
            id,
            title: preview(&obligation, 80),
            introduced_when: "ending_contract".to_string(),
            reader_knows: preview(&obligation, 180),
            planned_payoff_window: "typed ending obligation".to_string(),
            emotional_effect: "Required final-body evidence before completion.".to_string(),
            status: HookStatus::Open,
            evidence: vec![obligation],
            ..Default::default()
        });
    }
    hooks
}

fn theme_ledger_from(contract: &StoryContract) -> Vec<ThemeLedgerEntry> {
    let themes = contract
        .authority_contract
        .as_ref()
        .map(|authority| authority.themes.as_slice())
        .unwrap_or(contract.themes.as_slice());
    themes
        .iter()
        .filter_map(|theme| sanitize_contract_item(theme))
        .map(|theme| ThemeLedgerEntry {
            theme,
            function: "Guide character choices and consequences; do not remain decorative."
                .to_string(),
            recurrence_rule:
                "Touch through action, cost, relationship, or reversal across the project."
                    .to_string(),
            last_touched_chapter: None,
        })
        .collect()
}

fn timeline_from(contract: &StoryContract) -> Vec<TimelineEntry> {
    let premise = sanitize_contract_multiline(&contract.premise);
    if premise.trim().is_empty() {
        return Vec::new();
    }
    vec![TimelineEntry {
        chapter_number: None,
        label: "premise".to_string(),
        event: premise,
        causal_link: "Initial cause of the story spine.".to_string(),
    }]
}

fn genre_governance_profile(
    genre: &str,
    language: &str,
) -> crate::tool::writing::longform_policy::GenreGovernanceProfile {
    crate::tool::writing::longform_policy::genre_governance_profile(genre, language)
}

fn ensure_bible_defaults(bible: &mut StoryBible) {
    if bible.schema_version != STORY_BIBLE_VERSION {
        bible.schema_version = STORY_BIBLE_VERSION.to_string();
    }
    if bible.narrative_graph.reverse_design_notes.is_empty() {
        bible.narrative_graph.reverse_design_notes =
            vec!["Design backward from the ending before drafting forward.".to_string()];
    }
    bible.structured_contract_v2.normalize();
    if bible.source_contract_revision == 0 {
        bible.source_contract_revision = bible.structured_contract_v2.revision;
    }
}

pub(crate) fn pending_hook_truth(bible: &StoryBible) -> String {
    bible
        .hook_ledger
        .iter()
        .filter(|hook| !matches!(hook.status, HookStatus::PaidOff | HookStatus::Dropped))
        .map(|hook| {
            format!(
                "{} | {} | status={:?} | due={} | last_advanced={}",
                hook.id,
                hook.title,
                hook.status,
                hook.deferred_until_chapter
                    .or(hook.planned_payoff_chapter)
                    .map(|chapter| chapter.to_string())
                    .unwrap_or_else(|| "unplanned".to_string()),
                hook.last_advanced_chapter
                    .map(|chapter| chapter.to_string())
                    .unwrap_or_else(|| "never".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ensure_character_anchors_from_chapter(bible: &mut StoryBible, chapter: &ApprovedChapterDelta) {
    for registration in &chapter.character_registrations {
        let name = registration.canonical_name.trim();
        if name.is_empty() {
            continue;
        }
        if bible
            .character_ledger
            .iter()
            .any(|character| character.name == name)
        {
            continue;
        }
        bible.character_ledger.push(CharacterAnchor {
            id: registration.character_id.clone(),
            name: name.to_string(),
            role: registration.role.clone(),
            desire: registration.desire.clone(),
            fear: registration.fear.clone(),
            bottom_line: registration.bottom_line.clone(),
            wound_or_flaw: registration.arc_start.clone(),
            current_state: String::new(),
            relationship_anchors: sanitize_contract_item(&registration.relationship_to_existing)
                .into_iter()
                .collect(),
        });
    }
}

fn stable_character_name(name: &str) -> bool {
    let trimmed = name.trim();
    let len = trimmed.chars().count();
    if !(2..=32).contains(&len) {
        return false;
    }
    if trimmed.chars().any(|ch| {
        ch.is_ascii_digit()
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
                    | '\n'
                    | '\r'
                    | '\t'
                    | '"'
                    | '\''
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '《'
                    | '》'
                    | '('
                    | ')'
                    | '（'
                    | '）'
                    | '['
                    | ']'
                    | '【'
                    | '】'
            )
    }) {
        return false;
    }
    let has_cjk = trimmed
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
    if has_cjk && !(2..=6).contains(&len) {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    !matches!(
        lowered.as_str(),
        "protagonist" | "antagonist" | "hero" | "villain" | "character" | "主角" | "反派" | "角色"
    )
}

fn character_contract_item_has_core_anchor(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    let has_desire =
        value.contains("欲望") || value.contains("目标") || lowered.contains("desire:");
    let has_fear = value.contains("恐惧") || value.contains("害怕") || lowered.contains("fear:");
    let has_bottom_line = value.contains("底线")
        || lowered.contains("bottom_line:")
        || lowered.contains("bottom line:");
    has_desire && has_fear && has_bottom_line
}

fn character_anchor_core_is_missing(anchor: &CharacterAnchor) -> bool {
    anchor_value_is_missing(&anchor.desire)
        || anchor_value_is_missing(&anchor.fear)
        || anchor_value_is_missing(&anchor.bottom_line)
}

fn anchor_value_is_missing(value: &str) -> bool {
    let text = value.trim();
    if text.is_empty() {
        return true;
    }
    let lowered = text.to_ascii_lowercase();
    text.contains("未明示")
        || text.contains("not fully explicit")
        || text.contains("后续")
        || lowered.contains("not specified")
        || lowered.contains("not explicit")
}

fn hook_is_completion_relevant(bible: &StoryBible, hook: &HookLedgerEntry) -> bool {
    if hook.title.trim().is_empty() {
        return false;
    }
    if hook.status == HookStatus::Overdue {
        return true;
    }
    if bible
        .ending_contract
        .open_questions_allowed
        .iter()
        .any(|allowed| {
            allowed == &hook.title || hook.evidence.iter().any(|evidence| evidence == allowed)
        })
    {
        return false;
    }
    hook.id.starts_with("ending-")
        || hook.planned_payoff_chapter.is_some()
        || bible
            .structured_contract_v2
            .payoff_matrix
            .iter()
            .any(|payoff| {
                payoff.promise == hook.title
                    || payoff.payoff_target == hook.title
                    || hook.evidence.iter().any(|evidence| {
                        evidence == &payoff.promise || evidence == &payoff.payoff_target
                    })
            })
}

fn sanitize_contract_item(value: impl AsRef<str>) -> Option<String> {
    let cleaned = sanitize_contract_scalar(value.as_ref());
    (!cleaned.is_empty() && !contract_line_is_surface_noise(&cleaned)).then_some(cleaned)
}

fn sanitize_contract_scalar(value: &str) -> String {
    let cleaned = value
        .trim()
        .trim_matches(['"', '\'', '“', '”', '‘', '’'])
        .trim()
        .to_string();
    if contract_line_is_surface_noise(&cleaned) {
        String::new()
    } else {
        cleaned
    }
}

fn sanitize_contract_multiline(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .filter_map(sanitize_contract_item)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn contract_line_is_surface_noise(value: &str) -> bool {
    let text = value.trim();
    if text.is_empty() {
        return true;
    }
    let lowered = text.to_ascii_lowercase();
    [
        "可修改说明",
        "请回复",
        "回复开始",
        "开始写第一章",
        "按这个开始",
        "如果不满意",
        "如果满意",
        "用户可以",
        "你可以修改",
        "质量合同",
        "导出规范",
        "工具调用",
        "下一步",
        "操作说明",
        "确认后",
        "全书推进依据",
        "书名理由",
        "终局方向",
        "主角弧线",
        "世界观意象",
        "总主线因果链",
        "contract draft",
        "quality contract",
        "export format",
        "reply",
        "please confirm",
        "next step",
    ]
    .iter()
    .any(|marker| text.contains(marker) || lowered.contains(marker))
}

fn sanitize_character_ledger(ledger: &mut Vec<CharacterAnchor>) {
    let mut seen = Vec::<String>::new();
    ledger.retain(|character| {
        let name = character.name.trim();
        if !stable_character_name(name) || seen.iter().any(|existing| existing == name) {
            return false;
        }
        seen.push(name.to_string());
        true
    });
}

fn upsert_chapter_summary(
    summaries: &mut Vec<ChapterContinuitySummary>,
    summary: ChapterContinuitySummary,
) {
    summaries.retain(|item| item.chapter_number != summary.chapter_number);
    summaries.push(summary);
    summaries.sort_by_key(|item| item.chapter_number);
}

fn hook_like_text(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "伏笔", "悬念", "线索", "秘密", "未解", "谜", "hook", "clue", "secret", "mystery",
    ]
    .iter()
    .any(|term| text.contains(term) || lowered.contains(term))
}

fn character_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    for label in ["name:", "Name:", "name：", "Name：", "姓名：", "姓名:"] {
        if let Some(rest) = trimmed.strip_prefix(label) {
            let candidate = rest
                .split(['；', ';', '，', ',', '\n', '\r'])
                .next()
                .unwrap_or(rest)
                .trim();
            if !candidate.is_empty() {
                return Some(candidate.to_string());
            }
        }
    }
    if !contract_character_item_has_profile_marker(trimmed)
        && !character_contract_item_has_core_anchor(trimmed)
    {
        return None;
    }
    let first = trimmed
        .split(['；', ';', '，', ',', '：', ':', '-', '—'])
        .next()
        .unwrap_or(trimmed)
        .trim();
    if first.is_empty() {
        None
    } else {
        Some(first.to_string())
    }
}

fn contract_character_item_looks_like_anchor(value: &str) -> bool {
    let Some(name) = character_name(value) else {
        return false;
    };
    stable_character_name(&name)
        && (character_contract_item_has_core_anchor(value)
            || contract_character_item_has_profile_marker(value))
}

fn contract_character_item_has_profile_marker(value: &str) -> bool {
    let text = value.trim();
    let lowered = text.to_ascii_lowercase();
    [
        "role:",
        "canonical_name:",
        "name:",
        "姓名",
        "名字",
        "角色名",
        "主角",
        "主人公",
        "男主",
        "女主",
        "反派",
        "对手",
        "同伴",
        "导师",
        "protagonist",
        "antagonist",
        "character",
        "mentor",
        "ally",
        "opponent",
    ]
    .iter()
    .any(|marker| text.contains(marker) || lowered.contains(marker))
}

fn anchor_field_or_default(value: &str, keys: &[&str], default: &str) -> String {
    for key in keys {
        if let Some((_, tail)) = value.split_once(key) {
            let candidate = tail
                .split(['；', ';', '。', '\n'])
                .next()
                .unwrap_or(tail)
                .trim();
            if !candidate.is_empty() {
                return candidate.to_string();
            }
        }
    }
    default.to_string()
}

fn first_non_empty<'a>(items: &[&'a str]) -> &'a str {
    items
        .iter()
        .map(|item| item.trim())
        .find(|item| !item.is_empty())
        .unwrap_or("")
}

fn preview(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests;
