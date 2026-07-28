use super::*;
use crate::tool::writing::typed_contract_gate;

pub(super) fn canonical_project_contract_projection(
    manifest: &NovelProjectManifest,
) -> serde_json::Value {
    if let Some(authority) = manifest
        .contract
        .as_ref()
        .and_then(|contract| contract.authority_contract.as_ref())
    {
        return serde_json::to_value(authority).unwrap_or(serde_json::Value::Null);
    }
    let Some(contract) = manifest.contract.as_ref() else {
        return serde_json::Value::Null;
    };
    json!({
        "title": manifest.title,
        "language": manifest.language,
        "genre": manifest.genre,
        "brief": manifest.brief,
        "target_units": manifest.target_units,
        "chapter_unit_target": manifest.chapter_unit_target,
        "premise": contract.premise,
        "themes": contract.themes,
        "characters": contract.characters,
        "world_rules": contract.world_rules,
        "style_rules": contract.style_rules,
        "must_avoid": contract.must_avoid,
        "outline": contract.outline,
        "structured": manifest.structured_contract_v2
    })
}

pub(super) fn ensure_story_bible_from_manifest(manifest: &mut NovelProjectManifest) {
    if manifest.story_bible.is_none() {
        rebuild_story_bible_from_manifest(manifest);
    }
}

pub(super) fn rebuild_story_bible_from_manifest(manifest: &mut NovelProjectManifest) {
    let Some(contract) = manifest.contract.clone() else {
        return;
    };
    let approved = manifest
        .chapters
        .iter()
        .filter(|chapter| chapter_is_approved(chapter))
        .map(|chapter| {
            let character_registrations = manifest
                .chapter_contracts
                .iter()
                .find(|record| record.number == chapter.number)
                .map(|record| record.character_registrations.clone())
                .unwrap_or_default();
            novel_bible::ApprovedChapterDelta {
                number: chapter.number,
                title: chapter.title.clone(),
                summary: chapter.summary.clone(),
                unit_count: chapter.unit_count,
                key_facts: chapter.key_facts.clone(),
                continuity_updates: chapter.continuity_updates.clone(),
                character_registrations,
                state_changes: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    let mut bible = novel_bible::rebuild_story_bible(
        &manifest.title,
        &manifest.language,
        &manifest.genre,
        &manifest.brief,
        &contract,
        &approved,
        now_iso(),
    );
    for chapter in &manifest.chapter_contracts {
        if approved
            .iter()
            .any(|approved| approved.number == chapter.number)
        {
            continue;
        }
        novel_bible::upsert_planned_chapter_goal(
            &mut bible,
            chapter.number,
            &chapter.goal,
            &chapter.reveal,
            &chapter.payoff_target,
        );
    }
    manifest.story_bible = Some(bible);
}

pub(super) fn rebuild_story_bible_from_contract_only(manifest: &mut NovelProjectManifest) {
    let Some(contract) = manifest.contract.clone() else {
        manifest.story_bible = None;
        return;
    };
    manifest.story_bible = Some(novel_bible::rebuild_story_bible(
        &manifest.title,
        &manifest.language,
        &manifest.genre,
        &manifest.brief,
        &contract,
        &[],
        now_iso(),
    ));
}

pub(super) fn invalidate_story_bible_planning_after(
    bible: Option<&mut novel_bible::StoryBible>,
    authority_cutoff_chapter: usize,
) {
    let Some(bible) = bible else {
        return;
    };
    bible
        .narrative_graph
        .chapter_goals
        .retain(|goal| goal.chapter_number <= authority_cutoff_chapter);
}

pub(super) fn ensure_project_governance(manifest: &mut NovelProjectManifest) {
    ensure_contract_character_identity_fields(manifest);
    ensure_structured_contract_v2(manifest);
    ensure_title_state(manifest);
    if manifest.contract.is_some() {
        ensure_story_bible_from_manifest(manifest);
    }
    ensure_story_bible_character_anchor_cores(manifest);
    ensure_volume_records_from_story_bible(manifest);
    ensure_character_authority_ledger(manifest);
    ensure_chapter_volume_assignments(manifest);
    ensure_volume_summaries(manifest);
    ensure_title_state(manifest);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidating_descendants_removes_only_future_planning_nodes() {
        let mut bible = novel_bible::StoryBible::default();
        bible.narrative_graph.chapter_goals = vec![
            novel_bible::ChapterGoal {
                chapter_number: 3,
                goal: "保留已批准边界".to_string(),
                ..Default::default()
            },
            novel_bible::ChapterGoal {
                chapter_number: 4,
                goal: "删除失效滚动目标".to_string(),
                ..Default::default()
            },
        ];
        invalidate_story_bible_planning_after(Some(&mut bible), 3);

        let goals = &bible.narrative_graph.chapter_goals;
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].chapter_number, 3);
    }
}

fn ensure_contract_character_identity_fields(manifest: &mut NovelProjectManifest) {
    let Some(contract) = manifest.contract.as_mut() else {
        return;
    };
    let mut ids_by_name = BTreeMap::new();
    for line in &mut contract.characters {
        let mut character = super::super::creation_contract::draft_character_line_to_contract(line);
        let name = character.canonical_name.trim();
        if name.is_empty() {
            continue;
        }
        if character.character_id.trim().is_empty() {
            let fingerprint =
                governance::authority_fingerprint(&json!([manifest.title.as_str(), name]));
            character.character_id = format!("character-legacy-{}", &fingerprint[..16]);
        }
        if character.name_source.trim().is_empty() {
            character.name_source = "legacy_contract".to_string();
        }
        ids_by_name.insert(name.to_string(), character.character_id.clone());
        *line = character.to_draft_line();
    }
    for relation in &mut contract.structured_contract_v2.relationship_ledger {
        resolve_legacy_relationship_ids(relation, &ids_by_name);
    }
    for relation in &mut manifest.structured_contract_v2.relationship_ledger {
        resolve_legacy_relationship_ids(relation, &ids_by_name);
    }
}

fn resolve_legacy_relationship_ids(
    relationship: &mut RelationshipLedgerEntry,
    ids_by_name: &BTreeMap<String, String>,
) {
    let resolved = relationship
        .characters
        .iter()
        .filter_map(|name| ids_by_name.get(name.trim()).cloned())
        .collect::<Vec<_>>();
    if resolved.len() == relationship.characters.len() {
        relationship.character_ids = resolved;
    }
}

pub(super) fn ensure_structured_contract_v2(manifest: &mut NovelProjectManifest) {
    manifest.structured_contract_v2.normalize();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.structured_contract_v2.normalize();
    }
    if let Some(bible) = manifest.story_bible.as_mut() {
        bible.structured_contract_v2.normalize();
    }

    // The confirmed typed creation contract is authoritative. StoryContract and
    // the manifest field are compatibility mirrors; the story-bible copy is a
    // derived runtime projection and must never win merely because its reducer
    // bumped a revision counter.
    let canonical = manifest
        .contract
        .as_ref()
        .and_then(|contract| {
            contract
                .authority_contract
                .as_ref()
                .map(|authority| authority.structured.clone())
                .filter(NovelContractV2::has_authored_content)
                .or_else(|| {
                    contract
                        .structured_contract_v2
                        .has_authored_content()
                        .then(|| contract.structured_contract_v2.clone())
                })
        })
        .filter(NovelContractV2::has_authored_content)
        .or_else(|| {
            manifest
                .structured_contract_v2
                .has_authored_content()
                .then(|| manifest.structured_contract_v2.clone())
        })
        .or_else(|| {
            manifest
                .story_bible
                .as_ref()
                .map(|bible| bible.structured_contract_v2.clone())
                .filter(NovelContractV2::has_authored_content)
        })
        .unwrap_or_default();

    manifest.structured_contract_v2 = canonical.clone();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.structured_contract_v2 = canonical.clone();
    }
    if let Some(bible) = manifest.story_bible.as_mut() {
        bible.structured_contract_v2 = canonical;
    }
}

pub(super) fn ensure_title_state(manifest: &mut NovelProjectManifest) {
    let now = now_iso();
    if manifest.title.trim().is_empty() {
        manifest.title = first_non_empty(&[
            manifest.title_state.canonical_title.as_str(),
            manifest.title_state.provisional_title.as_str(),
            "untitled",
        ])
        .to_string();
    }
    if manifest.title_state.provisional_title.trim().is_empty() {
        manifest.title_state.provisional_title = manifest.title.clone();
    }
    if manifest.title_state.canonical_title.trim().is_empty()
        || manifest.title_state.canonical_title.trim() != manifest.title.trim()
    {
        manifest.title_state.canonical_title = manifest.title.clone();
    }
    if manifest.title_state.source.trim().is_empty() {
        manifest.title_state.source = if manifest.contract.is_some() {
            "llm_contract".to_string()
        } else {
            "initial_title".to_string()
        };
    }
    if manifest.title_state.rationale.trim().is_empty() {
        let ending = manifest
            .story_bible
            .as_ref()
            .map(|bible| bible.ending_contract.final_state.as_str())
            .unwrap_or("");
        manifest.title_state.rationale = first_non_empty(&[
            ending,
            manifest.brief.as_str(),
            "Derived from the project contract, ending direction, protagonist arc, and world imagery.",
        ])
        .to_string();
    }
    if manifest.contract.is_some() {
        manifest.title_state.locked = true;
    }
    if manifest.title_state.updated_at.trim().is_empty() {
        manifest.title_state.updated_at = now;
    }
}

fn ensure_story_bible_character_anchor_cores(manifest: &mut NovelProjectManifest) {
    let Some(bible) = manifest.story_bible.as_mut() else {
        return;
    };
    for character in &mut bible.character_ledger {
        if !character_anchor_value_is_meaningful(&character.desire) {
            character.desire.clear();
        }
        if !character_anchor_value_is_meaningful(&character.fear) {
            character.fear.clear();
        }
        if !character_anchor_value_is_meaningful(&character.bottom_line) {
            character.bottom_line.clear();
        }
    }
}

pub(super) fn ensure_character_authority_ledger(manifest: &mut NovelProjectManifest) {
    let mut existing = manifest
        .character_ledger
        .iter()
        .map(|record| (record.canonical_name.clone(), record.clone()))
        .collect::<BTreeMap<_, _>>();
    let now = now_iso();
    let mut records = Vec::new();
    if let Some(contract) = manifest.contract.as_ref() {
        for (index, line) in contract.characters.iter().enumerate() {
            let character = super::super::creation_contract::draft_character_line_to_contract(line);
            let name = character.canonical_name.trim();
            if name.is_empty() {
                continue;
            }
            let previous = existing.remove(name);
            let forbidden_renames = previous
                .as_ref()
                .map(|record| record.forbidden_renames.iter().cloned())
                .into_iter()
                .flatten()
                .chain(character.previous_names.iter().cloned())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty() && value != name)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let identity_markers = character_identity_markers_for_role(
                previous
                    .as_ref()
                    .map(|record| record.identity_markers.clone())
                    .unwrap_or_default(),
                &character.role,
            );
            records.push(CharacterAuthorityRecord {
                id: first_non_empty(&[
                    character.character_id.as_str(),
                    previous
                        .as_ref()
                        .map(|record| record.id.as_str())
                        .unwrap_or(""),
                    &format!("character-{:04}", index + 1),
                ])
                .to_string(),
                canonical_name: name.to_string(),
                name_source: first_non_empty(&[
                    character.name_source.as_str(),
                    previous
                        .as_ref()
                        .map(|record| record.name_source.as_str())
                        .unwrap_or(""),
                    "contract_authority",
                ])
                .to_string(),
                aliases: previous
                    .as_ref()
                    .map(|record| record.aliases.iter().cloned())
                    .into_iter()
                    .flatten()
                    .chain(character.aliases.into_iter())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                identity_markers,
                role: character.role,
                desire: meaningful_character_anchor_or_empty(&[character.desire.as_str()]),
                fear: meaningful_character_anchor_or_empty(&[character.fear.as_str()]),
                bottom_line: meaningful_character_anchor_or_empty(&[character
                    .bottom_line
                    .as_str()]),
                arc_start: meaningful_character_anchor_or_empty(&[character.arc_start.as_str()]),
                arc_end: meaningful_character_anchor_or_empty(&[character.arc_end.as_str()]),
                planned_entry: character.planned_entry,
                planned_exit: character.planned_exit,
                forbidden_renames,
                status: previous
                    .as_ref()
                    .map(|record| record.status.clone())
                    .unwrap_or_else(|| "planned".to_string()),
                updated_at: now.clone(),
            });
        }
    }
    if let Some(bible) = manifest.story_bible.as_ref() {
        for (index, character) in bible.character_ledger.iter().enumerate() {
            let name = character.name.trim();
            if name.is_empty() || records.iter().any(|record| record.canonical_name == name) {
                continue;
            }
            let previous = existing.remove(name);
            let role = first_non_empty(&[
                character.role.as_str(),
                previous
                    .as_ref()
                    .map(|record| record.role.as_str())
                    .unwrap_or(""),
            ])
            .to_string();
            let identity_markers = character_identity_markers_for_role(
                previous
                    .as_ref()
                    .map(|record| record.identity_markers.clone())
                    .unwrap_or_default(),
                &role,
            );
            records.push(CharacterAuthorityRecord {
                id: previous
                    .as_ref()
                    .map(|record| record.id.clone())
                    .unwrap_or_else(|| {
                        first_non_empty(&[
                            character.id.as_str(),
                            &format!("character-{:04}", index + 1),
                        ])
                        .to_string()
                    }),
                canonical_name: name.to_string(),
                name_source: previous
                    .as_ref()
                    .map(|record| record.name_source.clone())
                    .unwrap_or_else(|| "approved_chapter".to_string()),
                aliases: previous
                    .as_ref()
                    .map(|record| record.aliases.clone())
                    .unwrap_or_default(),
                identity_markers,
                role,
                desire: meaningful_character_anchor_or_empty(&[
                    character.desire.as_str(),
                    previous
                        .as_ref()
                        .map(|record| record.desire.as_str())
                        .unwrap_or(""),
                ]),
                fear: meaningful_character_anchor_or_empty(&[
                    character.fear.as_str(),
                    previous
                        .as_ref()
                        .map(|record| record.fear.as_str())
                        .unwrap_or(""),
                ]),
                bottom_line: meaningful_character_anchor_or_empty(&[
                    character.bottom_line.as_str(),
                    previous
                        .as_ref()
                        .map(|record| record.bottom_line.as_str())
                        .unwrap_or(""),
                ]),
                arc_start: meaningful_character_anchor_or_empty(&[previous
                    .as_ref()
                    .map(|record| record.arc_start.as_str())
                    .unwrap_or("")]),
                arc_end: meaningful_character_anchor_or_empty(&[previous
                    .as_ref()
                    .map(|record| record.arc_end.as_str())
                    .unwrap_or("")]),
                planned_entry: previous
                    .as_ref()
                    .map(|record| record.planned_entry.clone())
                    .unwrap_or_default(),
                planned_exit: previous
                    .as_ref()
                    .map(|record| record.planned_exit.clone())
                    .unwrap_or_default(),
                forbidden_renames: previous
                    .as_ref()
                    .map(|record| record.forbidden_renames.clone())
                    .unwrap_or_default(),
                status: previous
                    .as_ref()
                    .map(|record| record.status.clone())
                    .unwrap_or_else(|| "active".to_string()),
                updated_at: now.clone(),
            });
        }
    }
    for record in existing.into_values() {
        if !record.canonical_name.trim().is_empty() {
            records.push(record);
        }
    }
    records.sort_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
    records.dedup_by(|left, right| left.canonical_name == right.canonical_name);
    manifest.character_ledger = records;
}

fn character_identity_markers_for_role(mut markers: Vec<String>, role: &str) -> Vec<String> {
    let explicit_profile = if role.contains("女主") {
        Some("pronoun_profile:feminine")
    } else if role.contains("男主") {
        Some("pronoun_profile:masculine")
    } else {
        None
    };
    if let Some(profile) = explicit_profile {
        markers.retain(|marker| {
            !marker.starts_with("pronoun_profile:")
                && !marker.starts_with("inferred_pronoun_profile:")
        });
        markers.push(profile.to_string());
    }
    markers.sort();
    markers.dedup();
    markers
}

pub(super) fn promote_approved_chapter_character_identity_markers(
    manifest: &mut NovelProjectManifest,
    final_body: &str,
) {
    if !is_chinese_language(&manifest.language) {
        return;
    }
    let character_names = manifest
        .character_ledger
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    for character in &mut manifest.character_ledger {
        if character.identity_markers.iter().any(|marker| {
            marker.starts_with("pronoun_profile:")
                || marker.starts_with("inferred_pronoun_profile:")
        }) {
            continue;
        }
        let name = character.canonical_name.trim();
        if name.is_empty() {
            continue;
        }
        let mut other_character_names = character_names.clone();
        other_character_names.remove(name);
        let Some(profile) = super::contract_stable_character_pronoun_profile_in_text(
            final_body,
            name,
            &other_character_names,
        ) else {
            continue;
        };
        character
            .identity_markers
            .push(format!("inferred_pronoun_profile:{profile}"));
        character.identity_markers.sort();
        character.identity_markers.dedup();
        character.updated_at = now_iso();
    }
}

pub(super) fn register_chapter_character_requests(
    manifest: &mut NovelProjectManifest,
    chapter_number: usize,
    requests: &[ChapterCharacterRequest],
) -> Vec<ChapterCharacterRegistration> {
    if requests.is_empty() || manifest.contract.is_none() {
        return Vec::new();
    }
    ensure_character_authority_ledger(manifest);
    let mut used_names = contract_term_authority_view(manifest).character_names;
    used_names.extend(manifest_character_anchors(manifest));
    let mut registrations = Vec::new();
    let mut seen_requests = BTreeSet::new();

    for (index, request) in requests.iter().enumerate() {
        let request_id =
            normalized_character_request_id(&request.request_id, chapter_number, index);
        let importance = normalized_character_importance(&request.importance);
        if !seen_requests.insert(request_id.clone())
            || !chapter_character_request_is_complete(request, importance)
        {
            continue;
        }
        let request_marker = format!("request_id:{request_id}");
        if let Some(existing) = manifest.character_ledger.iter().find(|record| {
            record
                .identity_markers
                .iter()
                .any(|marker| marker == &request_marker)
        }) {
            registrations.push(character_registration_from_record(
                existing,
                request,
                &request_id,
            ));
            used_names.insert(existing.canonical_name.clone());
            continue;
        }
        let Some(canonical_name) = naming::allocate_character_name(
            &manifest.title,
            &request_id,
            &request.role,
            &manifest.language,
            &used_names,
        ) else {
            continue;
        };
        used_names.insert(canonical_name.clone());
        let character_id = stable_chapter_character_id(manifest, chapter_number, &request_id);
        let planned_entry = first_non_empty(&[
            request.planned_entry.as_str(),
            &format!("第{chapter_number}章"),
        ])
        .to_string();
        let mut identity_markers = vec![
            request_marker,
            format!("introduced_in_chapter:{chapter_number}"),
            format!("importance:{importance}"),
            format!("narrative_purpose:{}", request.narrative_purpose.trim()),
        ];
        if !request.relationship_to_existing.trim().is_empty() {
            identity_markers.push(format!(
                "relationship_to_existing:{}",
                request.relationship_to_existing.trim()
            ));
        }
        if !request.voice_style.trim().is_empty() {
            identity_markers.push(format!("voice_style:{}", request.voice_style.trim()));
        }
        let record = CharacterAuthorityRecord {
            id: character_id.clone(),
            canonical_name: canonical_name.clone(),
            name_source: "local_character_allocator".to_string(),
            aliases: Vec::new(),
            identity_markers,
            role: request.role.trim().to_string(),
            desire: request.desire.trim().to_string(),
            fear: request.fear.trim().to_string(),
            bottom_line: request.bottom_line.trim().to_string(),
            arc_start: request.arc_start.trim().to_string(),
            arc_end: request.arc_end.trim().to_string(),
            planned_entry: planned_entry.clone(),
            planned_exit: request.planned_exit.trim().to_string(),
            forbidden_renames: Vec::new(),
            status: format!("pending:chapter-{chapter_number}"),
            updated_at: now_iso(),
        };
        registrations.push(character_registration_from_record(
            &record,
            request,
            &request_id,
        ));
        manifest.character_ledger.push(record);
    }
    manifest
        .character_ledger
        .sort_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
    manifest
        .character_ledger
        .dedup_by(|left, right| left.canonical_name == right.canonical_name);
    registrations
}

fn stable_chapter_character_id(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
    request_id: &str,
) -> String {
    let fingerprint = governance::authority_fingerprint(&json!([
        manifest.title.as_str(),
        chapter_number,
        request_id
    ]));
    format!("character-chapter-{}", &fingerprint[..16])
}

fn normalized_character_request_id(value: &str, chapter_number: usize, index: usize) -> String {
    let normalized = value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        .take(64)
        .collect::<String>();
    if normalized.is_empty() {
        format!("chapter-{chapter_number}-character-{}", index + 1)
    } else {
        normalized
    }
}

fn normalized_character_importance(value: &str) -> &'static str {
    match value.trim() {
        "project_core" => "project_core",
        "volume_recurring" => "volume_recurring",
        _ => "chapter_temporary",
    }
}

fn chapter_character_request_is_complete(
    request: &ChapterCharacterRequest,
    importance: &str,
) -> bool {
    if request.role.trim().is_empty() || request.narrative_purpose.trim().is_empty() {
        return false;
    }
    if importance == "chapter_temporary" {
        return !request.planned_exit.trim().is_empty();
    }
    [
        request.relationship_to_existing.as_str(),
        request.desire.as_str(),
        request.fear.as_str(),
        request.bottom_line.as_str(),
        request.arc_start.as_str(),
        request.arc_end.as_str(),
        request.voice_style.as_str(),
    ]
    .iter()
    .all(|value| !value.trim().is_empty())
}

fn character_registration_from_record(
    record: &CharacterAuthorityRecord,
    request: &ChapterCharacterRequest,
    request_id: &str,
) -> ChapterCharacterRegistration {
    ChapterCharacterRegistration {
        request_id: request_id.to_string(),
        character_id: record.id.clone(),
        canonical_name: record.canonical_name.clone(),
        role: record.role.clone(),
        importance: normalized_character_importance(&request.importance).to_string(),
        narrative_purpose: request.narrative_purpose.trim().to_string(),
        planned_entry: record.planned_entry.clone(),
        planned_exit: record.planned_exit.clone(),
        relationship_to_existing: request.relationship_to_existing.trim().to_string(),
        desire: record.desire.clone(),
        fear: record.fear.clone(),
        bottom_line: record.bottom_line.clone(),
        arc_start: record.arc_start.clone(),
        arc_end: record.arc_end.clone(),
        voice_style: request.voice_style.trim().to_string(),
    }
}

pub(super) fn promote_chapter_character_registrations(
    manifest: &mut NovelProjectManifest,
    chapter_number: usize,
) {
    let pending_status = format!("pending:chapter-{chapter_number}");
    let pending = manifest
        .character_ledger
        .iter()
        .filter(|character| character.status == pending_status)
        .cloned()
        .collect::<Vec<_>>();
    let now = now_iso();
    for character in &mut manifest.character_ledger {
        if character.status == pending_status {
            character.status = if character
                .identity_markers
                .iter()
                .any(|marker| marker == "importance:chapter_temporary")
            {
                format!("chapter_local:chapter-{chapter_number}")
            } else {
                "active".to_string()
            };
            character.updated_at = now.clone();
        }
    }
    for character in pending {
        promote_character_voice_profile(manifest, &character);
        promote_character_relationships(manifest, &character, chapter_number);
    }
}

fn promote_character_voice_profile(
    manifest: &mut NovelProjectManifest,
    character: &CharacterAuthorityRecord,
) {
    let Some(voice_style) = character
        .identity_markers
        .iter()
        .find_map(|marker| marker.strip_prefix("voice_style:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let profile = CharacterVoiceProfile {
        character: character.canonical_name.clone(),
        voice_style: voice_style.to_string(),
        ..CharacterVoiceProfile::default()
    };
    upsert_character_voice_profile(
        &mut manifest.structured_contract_v2.character_voice_ledger,
        &profile,
    );
    if let Some(contract) = manifest.contract.as_mut() {
        upsert_character_voice_profile(
            &mut contract.structured_contract_v2.character_voice_ledger,
            &profile,
        );
    }
    if let Some(bible) = manifest.story_bible.as_mut() {
        upsert_character_voice_profile(
            &mut bible.structured_contract_v2.character_voice_ledger,
            &profile,
        );
    }
}

fn upsert_character_voice_profile(
    ledger: &mut Vec<CharacterVoiceProfile>,
    profile: &CharacterVoiceProfile,
) {
    if let Some(existing) = ledger
        .iter_mut()
        .find(|existing| existing.character == profile.character)
    {
        existing.voice_style = profile.voice_style.clone();
    } else {
        ledger.push(profile.clone());
    }
}

fn promote_character_relationships(
    manifest: &mut NovelProjectManifest,
    character: &CharacterAuthorityRecord,
    chapter_number: usize,
) {
    let Some(relationship) = character
        .identity_markers
        .iter()
        .find_map(|marker| marker.strip_prefix("relationship_to_existing:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let counterparts = manifest
        .character_ledger
        .iter()
        .filter(|existing| existing.id != character.id)
        .filter(|existing| relationship.contains(existing.canonical_name.trim()))
        .cloned()
        .collect::<Vec<_>>();
    for counterpart in counterparts {
        let entry = RelationshipLedgerEntry {
            character_ids: vec![counterpart.id.clone(), character.id.clone()],
            characters: vec![
                counterpart.canonical_name.clone(),
                character.canonical_name.clone(),
            ],
            arc_type: character
                .identity_markers
                .iter()
                .find_map(|marker| marker.strip_prefix("importance:"))
                .unwrap_or("chapter_temporary")
                .to_string(),
            relationship_type: relationship.to_string(),
            stage: "introduced".to_string(),
            next_expected_stage: character.arc_end.clone(),
            start_state: relationship.to_string(),
            current_state: relationship.to_string(),
            desired_end_state: character.arc_end.clone(),
            evidence: format!("chapter {chapter_number} execution contract"),
            last_changed_chapter: Some(chapter_number),
            ..RelationshipLedgerEntry::default()
        };
        upsert_relationship_entry(
            &mut manifest.structured_contract_v2.relationship_ledger,
            &entry,
        );
        if let Some(contract) = manifest.contract.as_mut() {
            upsert_relationship_entry(
                &mut contract.structured_contract_v2.relationship_ledger,
                &entry,
            );
        }
    }
}

fn upsert_relationship_entry(
    ledger: &mut Vec<RelationshipLedgerEntry>,
    entry: &RelationshipLedgerEntry,
) {
    let ids = entry.character_ids.iter().cloned().collect::<BTreeSet<_>>();
    if let Some(existing) = ledger.iter_mut().find(|existing| {
        existing
            .character_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            == ids
    }) {
        *existing = entry.clone();
    } else {
        ledger.push(entry.clone());
    }
}

pub(super) fn discard_chapter_character_registrations(
    manifest: &mut NovelProjectManifest,
    chapter_number: usize,
) {
    let pending_status = format!("pending:chapter-{chapter_number}");
    manifest
        .character_ledger
        .retain(|character| character.status != pending_status);
}

pub(super) fn ensure_volume_records_from_story_bible(manifest: &mut NovelProjectManifest) {
    let now = now_iso();
    let arcs = manifest
        .story_bible
        .as_ref()
        .map(|bible| bible.narrative_graph.volume_arcs.clone())
        .unwrap_or_default();
    if arcs.is_empty() {
        if manifest.volumes.is_empty() {
            manifest.volumes.push(default_volume_record(manifest, now));
        }
        if let Some(expected) = expected_project_chapter_count(manifest) {
            if manifest.volumes.len() == 1 && manifest.volumes[0].end_chapter.is_none() {
                manifest.volumes[0].end_chapter = Some(expected);
            }
        }
        seed_volume_contract_debts(&mut manifest.volumes, &manifest.structured_contract_v2);
        refresh_volume_statuses(manifest);
        return;
    }

    let existing = manifest
        .volumes
        .iter()
        .map(|volume| (volume.id.clone(), volume.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut volumes = Vec::new();
    for (index, arc) in arcs.into_iter().enumerate() {
        let fallback_id = format!("volume-{:04}", index + 1);
        let id = first_non_empty(&[arc.id.as_str(), fallback_id.as_str()]).to_string();
        let previous = existing.get(&id);
        let title = volume_title_from_arc(manifest, &arc, index + 1);
        let start_chapter = arc.start_chapter.unwrap_or_else(|| {
            previous
                .map(|volume| volume.start_chapter)
                .unwrap_or(index.saturating_add(1))
                .max(1)
        });
        volumes.push(VolumeRecord {
            id,
            title,
            start_chapter,
            end_chapter: arc
                .end_chapter
                .or_else(|| previous.and_then(|volume| volume.end_chapter)),
            objective: first_non_empty(&[
                arc.goal.as_str(),
                previous
                    .map(|volume| volume.objective.as_str())
                    .unwrap_or(""),
                manifest.brief.as_str(),
            ])
            .to_string(),
            key_results: previous
                .map(|volume| volume.key_results.clone())
                .unwrap_or_default(),
            emotional_curve: previous
                .map(|volume| volume.emotional_curve.clone())
                .unwrap_or_default(),
            must_open: previous
                .map(|volume| volume.must_open.clone())
                .unwrap_or_default(),
            must_payoff: previous
                .map(|volume| volume.must_payoff.clone())
                .unwrap_or_default(),
            ending_change: first_non_empty(&[
                arc.resolves_toward.as_str(),
                previous
                    .map(|volume| volume.ending_change.as_str())
                    .unwrap_or(""),
            ])
            .to_string(),
            status: previous
                .map(|volume| volume.status.clone())
                .unwrap_or_else(|| "planned".to_string()),
            summary: previous.and_then(|volume| volume.summary.clone()),
            created_at: previous
                .map(|volume| volume.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now.clone(),
        });
    }
    let expected_chapters = expected_project_chapter_count(manifest);
    normalize_volume_ranges(&mut volumes, expected_chapters);
    seed_volume_contract_debts(&mut volumes, &manifest.structured_contract_v2);
    manifest.volumes = volumes;
    refresh_volume_statuses(manifest);
}

fn seed_volume_contract_debts(volumes: &mut [VolumeRecord], contract: &NovelContractV2) {
    if volumes.is_empty() {
        return;
    }
    for payoff in &contract.payoff_matrix {
        let promise = payoff.promise.trim();
        let target = payoff.payoff_target.trim();
        if promise.is_empty() && target.is_empty() {
            continue;
        }
        let opening = first_non_empty(&[promise, target]).to_string();
        let payoff_text = first_non_empty(&[target, promise]).to_string();
        let opening_index = payoff
            .introduced_chapter
            .and_then(|chapter| {
                volumes
                    .iter()
                    .position(|volume| chapter_number_in_volume(chapter, volume))
            })
            .unwrap_or(0);
        let payoff_index = payoff
            .payoff_chapter
            .and_then(|chapter| {
                volumes
                    .iter()
                    .position(|volume| chapter_number_in_volume(chapter, volume))
            })
            .unwrap_or(volumes.len() - 1);
        if !opening.is_empty() && !volumes[opening_index].must_open.contains(&opening) {
            volumes[opening_index].must_open.push(opening);
        }
        if !payoff_text.is_empty() && !volumes[payoff_index].must_payoff.contains(&payoff_text) {
            volumes[payoff_index].must_payoff.push(payoff_text);
        }
    }
}

pub(super) fn ensure_chapter_volume_assignments(manifest: &mut NovelProjectManifest) {
    let volume_by_chapter = manifest
        .chapters
        .iter()
        .map(|chapter| {
            (
                chapter.number,
                volume_for_chapter(manifest, chapter.number)
                    .map(|volume| (volume.id.clone(), volume.title.clone()))
                    .unwrap_or_default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for chapter in &mut manifest.chapters {
        if let Some((id, title)) = volume_by_chapter.get(&chapter.number) {
            chapter.volume_id = id.clone();
            chapter.volume_title = title.clone();
        }
    }
}

pub(super) fn ensure_volume_summaries(manifest: &mut NovelProjectManifest) {
    let character_anchors = manifest_character_anchors(manifest)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut existing = manifest
        .volume_summaries
        .iter()
        .map(|summary| (summary.volume_id.clone(), summary.clone()))
        .collect::<BTreeMap<_, _>>();
    let now = now_iso();
    let mut summaries = Vec::new();
    for volume in &manifest.volumes {
        let approved = manifest
            .chapters
            .iter()
            .filter(|chapter| chapter_is_approved(chapter))
            .filter(|chapter| {
                chapter.volume_id == volume.id || chapter_number_in_volume(chapter.number, volume)
            })
            .collect::<Vec<_>>();
        if approved.is_empty() {
            if let Some(existing) = existing.remove(&volume.id) {
                summaries.push(existing);
            }
            continue;
        }
        let existing_record = existing.remove(&volume.id);
        let summary = first_non_empty(&[
            existing_record
                .as_ref()
                .map(|record| record.summary.as_str())
                .unwrap_or(""),
            volume.summary.as_deref().unwrap_or(""),
            &approved
                .iter()
                .map(|chapter| {
                    first_non_empty(&[chapter.summary.as_str(), chapter.title.as_str()]).to_string()
                })
                .collect::<Vec<_>>()
                .join("；"),
        ])
        .to_string();
        let resolved_hooks = approved
            .iter()
            .flat_map(|chapter| chapter.continuity_updates.iter())
            .filter(|item| {
                item.contains("resolved") || item.contains("收束") || item.contains("兑现")
            })
            .cloned()
            .take(12)
            .collect::<Vec<_>>();
        let new_hooks = approved
            .iter()
            .flat_map(|chapter| chapter.continuity_updates.iter())
            .filter(|item| item.contains("hook") || item.contains("伏笔") || item.contains("悬念"))
            .cloned()
            .take(12)
            .collect::<Vec<_>>();
        summaries.push(VolumeSummaryRecord {
            volume_id: volume.id.clone(),
            summary: compact_chapter_summary(&summary, &manifest.language),
            resolved_hooks,
            new_hooks,
            character_changes: approved
                .iter()
                .flat_map(|chapter| chapter.key_facts.iter())
                .filter(|item| character_anchors.iter().any(|name| item.contains(name)))
                .cloned()
                .take(12)
                .collect(),
            world_changes: approved
                .iter()
                .flat_map(|chapter| chapter.key_facts.iter())
                .filter(|item| !character_anchors.iter().any(|name| item.contains(name)))
                .cloned()
                .take(12)
                .collect(),
            next_volume_pressure: first_non_empty(&[
                volume.ending_change.as_str(),
                volume.objective.as_str(),
                "Continue causal pressure into the next volume without rereading full prose.",
            ])
            .to_string(),
            updated_at: existing_record
                .as_ref()
                .map(|record| record.updated_at.clone())
                .unwrap_or_else(|| now.clone()),
        });
    }
    summaries.sort_by(|left, right| left.volume_id.cmp(&right.volume_id));
    manifest.volume_summaries = summaries;
}

pub(super) fn chapter_number_in_volume(number: usize, volume: &VolumeRecord) -> bool {
    number >= volume.start_chapter && volume.end_chapter.is_none_or(|end| number <= end)
}

pub(super) fn default_volume_record(manifest: &NovelProjectManifest, now: String) -> VolumeRecord {
    VolumeRecord {
        id: "volume-0001".to_string(),
        title: if is_chinese_language(&manifest.language) {
            "第一卷".to_string()
        } else {
            "Volume 1".to_string()
        },
        start_chapter: 1,
        end_chapter: None,
        objective: first_non_empty(&[manifest.brief.as_str(), manifest.title.as_str()]).to_string(),
        key_results: Vec::new(),
        emotional_curve: String::new(),
        must_open: Vec::new(),
        must_payoff: Vec::new(),
        ending_change: manifest
            .contract
            .as_ref()
            .map(|contract| contract.outline.trim().to_string())
            .unwrap_or_default(),
        status: "active".to_string(),
        summary: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

pub(super) fn canonical_project_title(manifest: &NovelProjectManifest) -> &str {
    first_non_empty(&[
        manifest.title_state.canonical_title.as_str(),
        manifest.title.as_str(),
        manifest.title_state.provisional_title.as_str(),
    ])
}

pub(super) fn volume_title_from_arc(
    manifest: &NovelProjectManifest,
    arc: &novel_bible::NarrativeArc,
    index: usize,
) -> String {
    let title = arc.title.trim();
    if !title.is_empty() && !title.eq_ignore_ascii_case("opening movement") {
        return clean_manifest_volume_title(title, index, &manifest.language);
    }
    if is_chinese_language(&manifest.language) {
        format!("第{index}卷")
    } else {
        format!("Volume {index}")
    }
}

pub(super) fn clean_manifest_volume_title(title: &str, index: usize, language: &str) -> String {
    let trimmed = title.trim();
    if let Some(start) = trimmed.find('《') {
        if let Some(end) = trimmed[start + '《'.len_utf8()..].find('》') {
            let title = &trimmed[start + '《'.len_utf8()..start + '《'.len_utf8() + end];
            if !title.trim().is_empty() {
                return title.trim().to_string();
            }
        }
    }
    let mut title = trimmed
        .split(['：', ':', '(', '（'])
        .next()
        .unwrap_or(trimmed)
        .trim()
        .to_string();
    if title.chars().count() > 12 && is_chinese_language(language) {
        title = format!("第{index}卷");
    }
    title
}

fn expected_project_chapter_count(manifest: &NovelProjectManifest) -> Option<usize> {
    if let Some(expected) = manifest
        .target_units
        .zip(manifest.chapter_unit_target)
        .and_then(|(target, chapter_target)| {
            longform_policy::expected_chapter_count(target, chapter_target)
        })
    {
        return Some(expected);
    }
    manifest
        .chapter_plans
        .iter()
        .map(|plan| plan.number)
        .chain(manifest.chapters.iter().map(|chapter| chapter.number))
        .max()
        .filter(|value| *value > 0)
}

pub(super) fn normalize_volume_ranges(
    volumes: &mut [VolumeRecord],
    expected_chapters: Option<usize>,
) {
    volumes.sort_by_key(|volume| volume.start_chapter);
    if let Some(expected) = expected_chapters.filter(|value| *value > volumes.len()) {
        let generated_one_chapter_ranges = volumes.iter().enumerate().all(|(index, volume)| {
            volume.start_chapter == index + 1
                && (volume.end_chapter.is_none()
                    || (index == 0 && volume.end_chapter == Some(expected)))
        });
        if generated_one_chapter_ranges {
            let span = expected.div_ceil(volumes.len()).max(1);
            let volume_count = volumes.len();
            for (index, volume) in volumes.iter_mut().enumerate() {
                let start = index.saturating_mul(span).saturating_add(1);
                volume.start_chapter = start.min(expected).max(1);
                volume.end_chapter = if index + 1 == volume_count {
                    Some(expected)
                } else {
                    Some(((index + 1).saturating_mul(span)).min(expected))
                };
            }
            return;
        }
    }
    for index in 0..volumes.len() {
        if volumes[index].start_chapter == 0 {
            volumes[index].start_chapter = 1;
        }
        if index > 0 {
            let previous_end = volumes[index - 1]
                .end_chapter
                .unwrap_or(volumes[index - 1].start_chapter);
            if volumes[index].start_chapter <= previous_end {
                volumes[index].start_chapter = previous_end.saturating_add(1);
            }
        }
        if let Some(end) = volumes[index].end_chapter {
            if end < volumes[index].start_chapter {
                volumes[index].end_chapter = None;
            }
        }
        if volumes[index].end_chapter.is_none() {
            if let Some(next) = volumes.get(index + 1) {
                if next.start_chapter > volumes[index].start_chapter {
                    volumes[index].end_chapter = Some(next.start_chapter - 1);
                }
            }
        }
    }
    if let Some(expected) = expected_chapters {
        for volume in volumes {
            if volume.start_chapter > expected {
                volume.start_chapter = expected.max(1);
                volume.end_chapter = Some(expected.max(1));
                continue;
            }
            if let Some(end) = volume.end_chapter {
                volume.end_chapter = Some(end.min(expected).max(volume.start_chapter));
            }
        }
    }
}

pub(super) fn refresh_volume_statuses(manifest: &mut NovelProjectManifest) {
    let latest_chapter = manifest
        .chapters
        .iter()
        .filter(|chapter| chapter_is_approved(chapter))
        .map(|chapter| chapter.number)
        .max()
        .unwrap_or(0);
    for volume in &mut manifest.volumes {
        let end = volume.end_chapter.unwrap_or(usize::MAX);
        volume.status = if latest_chapter < volume.start_chapter {
            "planned".to_string()
        } else if latest_chapter > end {
            "completed".to_string()
        } else {
            "active".to_string()
        };
    }
}

pub(super) fn volume_for_chapter(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> Option<&VolumeRecord> {
    manifest
        .volumes
        .iter()
        .filter(|volume| {
            chapter_number >= volume.start_chapter
                && volume.end_chapter.is_none_or(|end| chapter_number <= end)
        })
        .max_by_key(|volume| volume.start_chapter)
        .or_else(|| {
            manifest
                .volumes
                .iter()
                .filter(|volume| volume.start_chapter <= chapter_number)
                .max_by_key(|volume| volume.start_chapter)
        })
}

pub(super) fn chapter_volume_pair(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> (String, String) {
    volume_for_chapter(manifest, chapter_number)
        .map(|volume| (volume.id.clone(), volume.title.clone()))
        .unwrap_or_default()
}

#[cfg(test)]
pub(super) fn final_chapter_title_from_body(
    manifest: &NovelProjectManifest,
    number: usize,
    requested_title: &str,
    summary: &str,
    content: &str,
) -> String {
    final_chapter_title_from_body_with_metadata(
        manifest,
        number,
        requested_title,
        summary,
        &[],
        &[],
        content,
    )
}

pub(super) fn final_chapter_title_from_body_with_metadata(
    manifest: &NovelProjectManifest,
    number: usize,
    requested_title: &str,
    summary: &str,
    key_facts: &[String],
    continuity_updates: &[String],
    content: &str,
) -> String {
    let context = chapter_title_context(manifest);
    let metadata_evidence = [
        summary.trim().to_string(),
        key_facts.join("\n"),
        continuity_updates.join("\n"),
    ]
    .into_iter()
    .filter(|value| !value.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n");
    let selected = naming::select_final_chapter_title_from_body(
        &context,
        number,
        requested_title,
        &metadata_evidence,
        content,
    )
    .selected
    .map(|candidate| candidate.title);
    selected
        .as_deref()
        .map(naming::chapter_title_core)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| default_chapter_title(&manifest.language, number))
}

fn chapter_title_context(manifest: &NovelProjectManifest) -> naming::ChapterTitleContext {
    naming::ChapterTitleContext {
        language: manifest.language.clone(),
        project_title: manifest.title.clone(),
        volume_titles: manifest
            .volumes
            .iter()
            .map(|volume| volume.title.clone())
            .collect(),
        other_chapter_titles: manifest
            .chapters
            .iter()
            .map(|chapter| (chapter.number, chapter.title.clone()))
            .collect(),
        character_names: manifest
            .character_ledger
            .iter()
            .map(|character| character.canonical_name.clone())
            .collect(),
    }
}

#[cfg(test)]
pub(super) fn title_needs_post_body_repair(
    manifest: &NovelProjectManifest,
    number: usize,
    title: &str,
) -> bool {
    let context = chapter_title_context(manifest);
    naming::chapter_title_needs_post_body_repair(&context, number, title)
        || chinese_title_language_issues(title)
            .is_some_and(|_| is_chinese_language(&manifest.language))
}

pub(super) fn title_matches_project_or_volume(
    manifest: &NovelProjectManifest,
    title: &str,
) -> bool {
    let context = chapter_title_context(manifest);
    naming::title_matches_project_or_volume(&context, title)
}

pub(super) fn title_is_default_chapter_heading(title: &str, number: usize, language: &str) -> bool {
    naming::title_is_default_chapter_heading(title, number, language)
}

pub(super) fn chapter_title_is_generic_stage_label(title: &str) -> bool {
    naming::generic_chapter_stage_label(title)
}

pub(super) fn cjk_title_candidate_has_sentence_fragment_edge(candidate: &str) -> bool {
    naming::chapter_title_sentence_fragment_edge(candidate)
}

pub(super) fn cjk_title_core_has_prose_grammar_fragment(core: &str) -> bool {
    naming::chapter_title_prose_grammar_fragment(core)
}

fn meaningful_character_anchor_or_empty(items: &[&str]) -> String {
    items
        .iter()
        .map(|item| item.trim())
        .find(|item| character_anchor_value_is_meaningful(item))
        .unwrap_or("")
        .to_string()
}

fn character_anchor_value_is_meaningful(value: &str) -> bool {
    let text = value.trim();
    if text.is_empty() {
        return false;
    }
    if typed_contract_gate::character_anchor_uses_generic_placeholder(text)
        || character_anchor_looks_like_project_outline(text)
    {
        return false;
    }
    let lowered = text.to_ascii_lowercase();
    !(text.contains("未明示")
        || text.contains("后续")
        || text.contains("待补")
        || text.contains("关键关系")
        || text.contains("推动终局的关系")
        || text.contains("关系对象")
        || text.contains("Established by project contract")
        || text.contains("Move toward the ending contract")
        || lowered.contains("not specified")
        || lowered.contains("not explicit")
        || lowered.contains("not fully explicit"))
}

fn character_anchor_looks_like_project_outline(value: &str) -> bool {
    let text = value.trim();
    let char_count = text.chars().count();
    char_count > 96
        || text.contains("->")
        || text.contains("→")
        || (text.contains('第') && text.contains('章'))
        || (text.contains("前提") && text.contains("主线"))
        || (text.contains("主线") && text.contains("结局"))
        || (text.contains("起势阶段") && text.contains("终局阶段"))
}
