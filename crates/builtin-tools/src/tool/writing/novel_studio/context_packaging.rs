use super::project_cache::TextScanReport;
use super::*;
use crate::tool::writing::creation_contract_model::ChapterSeedContract;
use sha2::Digest;

const SOURCE_CONTEXT_EXCERPT_CHARS: usize = 3_000;
const SOURCE_CONTEXT_PAYLOAD_CHARS: usize = 1_600;
const PROMPT_CONTEXT_TOTAL_CHARS: usize = 36_000;

pub(super) fn protected_prompt_context_char_limit() -> usize {
    PROMPT_CONTEXT_TOTAL_CHARS
}

fn source_context_entry(
    title: &str,
    notes: Option<&str>,
    source_url: Option<&str>,
    excerpt: &str,
) -> String {
    format!(
        "title: {}\nnotes: {}\nsource_url: {}\nexcerpt:\n{}",
        title.trim(),
        notes.unwrap_or_default().trim(),
        source_url.unwrap_or_default().trim(),
        excerpt.trim()
    )
}

pub(super) async fn source_material_excerpt(
    project_dir: &Path,
    source: &SourceRecord,
    max_chars: usize,
) -> anyhow::Result<String> {
    let path = project_dir.join(&source.path);
    let content = tokio::fs::read_to_string(path).await.unwrap_or_default();
    let body = strip_frontmatter(&content);
    Ok(preview_chars(&body, max_chars))
}

pub(super) async fn build_context_payload(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> anyhow::Result<serde_json::Value> {
    let plan = manifest
        .chapter_plans
        .iter()
        .find(|plan| plan.number == chapter_number)
        .cloned();
    let mut truth_files = Vec::new();
    for truth in &manifest.truth_files {
        let raw = tokio::fs::read_to_string(project_dir.join(&truth.path))
            .await
            .unwrap_or_default();
        let content =
            repair_contract_character_name_typos(manifest, &truth_file_body(&truth.section, &raw));
        truth_files.push(json!({
            "section": truth.section,
            "path": truth.path,
            "unit_count": truth.unit_count,
            "content": content
        }));
    }
    let recent_chapters = approved_prior_chapters(manifest, chapter_number)
        .rev()
        .take(CONTEXT_RECENT_CHAPTER_LIMIT)
        .map(|chapter| {
            json!({
                "number": chapter.number,
                "title": chapter.title,
                "summary": repair_contract_character_name_typos(manifest, &chapter.summary),
                "status": chapter.status,
                "key_facts": clean_contract_character_name_typos(
                    manifest,
                    chapter.key_facts.clone()
                ),
                "continuity_updates": clean_contract_character_name_typos(
                    manifest,
                    chapter.continuity_updates.clone()
                ),
                "unit_count": chapter.unit_count
            })
        })
        .collect::<Vec<_>>();
    let mut sources = Vec::new();
    for source in manifest.sources.iter().rev().take(CONTEXT_SOURCE_LIMIT) {
        sources.push(json!({
            "id": source.id,
            "title": source.title,
            "path": source.path,
            "notes": source.notes,
            "source_url": source.source_url,
            "unit_count": source.unit_count,
            "excerpt": source_material_excerpt(project_dir, source, SOURCE_CONTEXT_EXCERPT_CHARS).await?
        }));
    }
    let mut archives = Vec::new();
    for archive in context_archives(manifest, chapter_number)
        .into_iter()
        .take(CONTEXT_ARCHIVE_LIMIT)
    {
        let content = tokio::fs::read_to_string(project_dir.join(&archive.path))
            .await
            .unwrap_or_default();
        archives.push(json!({
            "kind": archive.kind,
            "range_start": archive.range_start,
            "range_end": archive.range_end,
            "path": archive.path,
            "unit_count": archive.unit_count,
            "excerpt": preview_chars(&content, ARCHIVE_EXCERPT_CHARS)
        }));
    }
    let relevant_characters = relevant_character_subgraph(manifest, chapter_number, plan.as_ref());
    let relevant_names = relevant_characters
        .iter()
        .map(|character| character.canonical_name.clone())
        .collect::<BTreeSet<_>>();
    let relevant_ids = relevant_characters
        .iter()
        .map(|character| character.id.clone())
        .collect::<BTreeSet<_>>();
    let relevant_anchor_names = relevant_characters
        .iter()
        .map(|character| character.canonical_name.clone())
        .collect::<Vec<_>>();
    let relevant_primary_names = relevant_characters
        .iter()
        .filter(|character| character_role_is_primary(&character.role))
        .map(|character| character.canonical_name.clone())
        .collect::<Vec<_>>();
    let relevant_contract =
        relevant_contract_view(manifest, chapter_number, &relevant_names, &relevant_ids);
    let relevant_story_bible = manifest
        .story_bible
        .as_ref()
        .map(novel_bible::story_bible_prompt_view)
        .map(|value| {
            relevant_story_bible_view(value, chapter_number, &relevant_names, &relevant_ids)
        })
        .map(|value| sanitize_prompt_json_text_fields(value, manifest));
    let narrative_progress = narrative_progress_contract(manifest, chapter_number);
    Ok(json!({
        "project": {
            "title": manifest.title,
            "title_state": manifest.title_state,
            "language": manifest.language,
            "genre": manifest.genre,
            "brief": manifest.brief,
            "target_units": manifest.target_units,
            "chapter_unit_target": manifest.chapter_unit_target,
            "current_volume": volume_for_chapter(manifest, chapter_number).map(|volume| json!({
                "id": volume.id,
                "title": volume.title,
                "start_chapter": volume.start_chapter,
                "end_chapter": volume.end_chapter,
                "objective": volume.objective,
                "ending_change": volume.ending_change,
                "status": volume.status
            })),
            "volumes": &manifest.volumes,
            "volume_summaries": &manifest.volume_summaries
        },
        "chapter_number": chapter_number,
        "contract": relevant_contract,
        "character_ledger": relevant_characters,
        "continuity_anchors": {
            "primary_characters": relevant_primary_names,
            "characters": relevant_anchor_names,
            "source": "story_contract_or_approved_chapters",
            "rule": "Preserve these authoritative identities only if the current chapter requires them; do not introduce a listed character merely because the name is present."
        },
        "story_bible": relevant_story_bible,
        "narrative_progress": narrative_progress,
        "next_chapter_boundary": next_chapter_boundary_view(manifest, chapter_number),
        "plan": plan.as_ref().map(current_chapter_plan_view),
        "architecture": manifest
            .chapter_architectures
            .iter()
            .find(|item| item.number == chapter_number)
            .map(current_chapter_architecture_view),
        "truth_files": truth_files,
        "recent_chapters": recent_chapters,
        "archives": archives,
        "sources": sources,
        "style_profiles": manifest.style_profiles,
        "audit": light_status_audit_manifest(manifest)
    }))
}

pub(super) fn build_minimal_context_payload(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> serde_json::Value {
    let plan = manifest
        .chapter_plans
        .iter()
        .find(|plan| plan.number == chapter_number);
    let relevant_characters = relevant_character_subgraph(manifest, chapter_number, plan);
    let relevant_names = relevant_characters
        .iter()
        .map(|character| character.canonical_name.clone())
        .collect::<BTreeSet<_>>();
    let relevant_ids = relevant_characters
        .iter()
        .map(|character| character.id.clone())
        .collect::<BTreeSet<_>>();
    let relevant_anchor_names = relevant_characters
        .iter()
        .map(|character| character.canonical_name.clone())
        .collect::<Vec<_>>();
    let relevant_primary_names = relevant_characters
        .iter()
        .filter(|character| character_role_is_primary(&character.role))
        .map(|character| character.canonical_name.clone())
        .collect::<Vec<_>>();
    let recent_chapters = approved_prior_chapters(manifest, chapter_number)
        .rev()
        .take(3)
        .map(|chapter| {
            json!({
                "number": chapter.number,
                "title": chapter.title,
                "summary": repair_contract_character_name_typos(manifest, &chapter.summary),
                "key_facts": clean_contract_character_name_typos(manifest, chapter.key_facts.clone()),
                "continuity_updates": clean_contract_character_name_typos(
                    manifest,
                    chapter.continuity_updates.clone()
                )
            })
        })
        .collect::<Vec<_>>();
    json!({
        "project": {
            "title": manifest.title,
            "title_state": manifest.title_state,
            "language": manifest.language,
            "genre": manifest.genre,
            "brief": manifest.brief,
            "target_units": manifest.target_units,
            "chapter_unit_target": manifest.chapter_unit_target,
            "current_volume": volume_for_chapter(manifest, chapter_number)
        },
        "chapter_number": chapter_number,
        "contract": relevant_contract_view(
            manifest,
            chapter_number,
            &relevant_names,
            &relevant_ids,
        ),
        "character_ledger": relevant_characters,
        "continuity_anchors": {
            "primary_characters": relevant_primary_names,
            "characters": relevant_anchor_names,
            "source": "typed_project_authority",
            "rule": "Preserve these authoritative identities only if the current chapter requires them; do not introduce a listed character merely because the name is present."
        },
        "story_bible": manifest
            .story_bible
            .as_ref()
            .map(novel_bible::story_bible_prompt_view)
            .map(|value| {
                relevant_story_bible_view(value, chapter_number, &relevant_names, &relevant_ids)
            })
            .map(|value| sanitize_prompt_json_text_fields(value, manifest)),
        "narrative_progress": narrative_progress_contract(manifest, chapter_number),
        "next_chapter_boundary": next_chapter_boundary_view(manifest, chapter_number),
        "plan": plan.map(current_chapter_plan_view),
        "recent_chapters": recent_chapters,
        "governance": {
            "mode": "minimal_authoritative_context",
            "approved_only": true,
            "do_not_import_unapproved_drafts": true
        }
    })
}

pub(super) fn narrative_progress_contract(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> serde_json::Value {
    let approved_units = manifest
        .chapters
        .iter()
        .filter(|chapter| chapter_is_approved(chapter))
        .map(|chapter| chapter.unit_count)
        .sum::<usize>();
    let target_units = manifest.target_units.filter(|value| *value > 0);
    let expected_chapters = target_units
        .zip(manifest.chapter_unit_target.filter(|value| *value > 0))
        .map(|(target, per_chapter)| target.div_ceil(per_chapter).max(1));
    let unit_progress_percent = target_units
        .map(|target| approved_units.saturating_mul(100).saturating_div(target))
        .unwrap_or(0)
        .min(100);
    let chapter_progress_percent = expected_chapters
        .map(|expected| {
            chapter_number
                .saturating_sub(1)
                .saturating_mul(100)
                .saturating_div(expected)
        })
        .unwrap_or(0)
        .min(100);
    let progress_percent = unit_progress_percent.max(chapter_progress_percent);
    let current_volume = volume_for_chapter(manifest, chapter_number);
    let current_volume_is_last = current_volume.is_some_and(|current| {
        manifest
            .volumes
            .last()
            .is_some_and(|last| last.id == current.id)
    });
    let phase = if progress_percent >= 100 {
        "finale"
    } else if progress_percent >= 75 || current_volume_is_last && progress_percent >= 60 {
        "convergence"
    } else if chapter_number <= 1 {
        "establishment"
    } else {
        "development"
    };
    let expansion_policy = match phase {
        "finale" => "closed",
        "convergence" => "restricted",
        "development" => "bounded",
        _ => "open",
    };
    json!({
        "phase": phase,
        "progress_percent": progress_percent,
        "approved_units": approved_units,
        "target_units": target_units,
        "expected_chapters": expected_chapters,
        "remaining_units": target_units.map(|target| target.saturating_sub(approved_units)),
        "expansion_policy": expansion_policy,
        "current_volume_is_last": current_volume_is_last,
        "rule": "Use the current contract, volume objective, approved continuity, and unresolved debts to decide concrete events. In convergence, new major entities or main branches must directly resolve an existing contract debt. In finale, do not open a new main branch."
    })
}

fn next_chapter_boundary_view(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> Vec<ChapterSeedContract> {
    let Some(next_number) = chapter_number.checked_add(1) else {
        return Vec::new();
    };
    manifest
        .contract
        .as_ref()
        .and_then(|contract| contract.authority_contract.as_ref())
        .and_then(|authority| {
            authority
                .outline
                .near_chapters
                .iter()
                .find(|chapter| chapter.number == Some(next_number))
        })
        .cloned()
        .into_iter()
        .collect()
}

pub(super) fn relevant_character_subgraph(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
    plan: Option<&ChapterPlanRecord>,
) -> Vec<CharacterAuthorityRecord> {
    let evidence = [
        plan.map(|value| current_chapter_authority_text(&value.plan))
            .unwrap_or_default(),
        manifest
            .chapter_contracts
            .iter()
            .find(|item| item.number == chapter_number)
            .map(|value| {
                [
                    value.goal.as_str(),
                    value.scene_goal.as_str(),
                    value.conflict.as_str(),
                    value.choice.as_str(),
                    value.cost.as_str(),
                    value.reveal.as_str(),
                    value.emotional_beat.as_str(),
                    value.relationship_delta.as_str(),
                    value.character_change.as_str(),
                ]
                .join("\n")
            })
            .unwrap_or_default(),
        manifest
            .chapter_architectures
            .iter()
            .find(|item| item.number == chapter_number)
            .map(|value| current_chapter_authority_text(&value.architecture))
            .unwrap_or_default(),
    ]
    .join("\n");

    let mut selected_names = manifest
        .character_ledger
        .iter()
        .filter(|record| {
            character_role_is_primary(&record.role)
                || evidence.contains(&record.canonical_name)
                || planned_character_entry_matches(record, manifest, chapter_number)
        })
        .map(|record| record.canonical_name.clone())
        .collect::<BTreeSet<_>>();
    if selected_names.is_empty() {
        selected_names.extend(
            manifest
                .character_ledger
                .iter()
                .take(1)
                .map(|record| record.canonical_name.clone()),
        );
    }
    let mut records = manifest
        .character_ledger
        .iter()
        .filter(|record| selected_names.contains(&record.canonical_name))
        .cloned()
        .collect::<Vec<_>>();
    for record in &mut records {
        record.forbidden_renames.clear();
    }
    records.sort_by_key(|record| {
        if character_role_is_primary(&record.role) {
            0
        } else if evidence.contains(&record.canonical_name) {
            1
        } else {
            2
        }
    });
    records.truncate(12);
    records
}

fn current_chapter_authority_text(value: &str) -> String {
    let lowered = value.to_ascii_lowercase();
    let boundaries = [
        value.find("## 下一章边界"),
        value.find("下一章边界："),
        lowered.find("## next chapter boundary"),
        lowered.find("next chapter boundary:"),
    ];
    let end = boundaries
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(value.len());
    value[..end].to_string()
}

fn character_role_is_primary(role: &str) -> bool {
    let lowered = role.to_ascii_lowercase();
    role.contains("主角")
        || role.contains("主人公")
        || role.contains("女主")
        || role.contains("男主")
        || lowered.contains("protagonist")
        || lowered.contains("main character")
}

fn current_chapter_plan_view(value: &ChapterPlanRecord) -> ChapterPlanRecord {
    let mut current = value.clone();
    current.plan = current_chapter_authority_text(&current.plan);
    current
}

fn current_chapter_architecture_view(
    value: &ChapterArchitectureRecord,
) -> ChapterArchitectureRecord {
    let mut current = value.clone();
    current.architecture = current_chapter_authority_text(&current.architecture);
    current
}

fn planned_character_entry_matches(
    record: &CharacterAuthorityRecord,
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> bool {
    let entry = record.planned_entry.trim();
    if entry.is_empty() {
        return false;
    }
    let chapter_markers = [
        format!("第{chapter_number}章"),
        format!("chapter {chapter_number}"),
    ];
    if chapter_markers.iter().any(|marker| entry.contains(marker)) {
        return true;
    }
    volume_for_chapter(manifest, chapter_number)
        .is_some_and(|volume| entry.contains(&volume.id) || entry.contains(&volume.title))
}

fn relevant_contract_view(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
    relevant_names: &BTreeSet<String>,
    relevant_ids: &BTreeSet<String>,
) -> Option<StoryContract> {
    let mut contract = manifest.contract.clone()?;
    contract.characters = contract
        .characters
        .into_iter()
        .filter_map(|line| {
            let mut character =
                super::super::creation_contract::draft_character_line_to_contract(&line);
            if !relevant_names.contains(&character.canonical_name)
                && !relevant_ids.contains(&character.character_id)
            {
                return None;
            }
            character.previous_names.clear();
            Some(character.to_draft_line())
        })
        .collect();
    if let Some(authority) = contract.authority_contract.as_mut() {
        authority.characters.retain(|character| {
            relevant_names.contains(&character.canonical_name)
                || relevant_ids.contains(&character.character_id)
        });
        for character in &mut authority.characters {
            character.previous_names.clear();
        }
        authority
            .outline
            .near_chapters
            .retain(|chapter| chapter.number == Some(chapter_number));
        authority.outline.raw_outline.clear();
    }
    contract.outline = manifest
        .chapter_contracts
        .iter()
        .find(|chapter| chapter.number == chapter_number)
        .map(|chapter| {
            [
                chapter.goal.as_str(),
                chapter.scene_goal.as_str(),
                chapter.conflict.as_str(),
                chapter.choice.as_str(),
                chapter.cost.as_str(),
                chapter.reveal.as_str(),
            ]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
        })
        .unwrap_or_default();
    contract
        .structured_contract_v2
        .relationship_ledger
        .retain(|relation| {
            relationship_intersects_subgraph(relation, relevant_names, relevant_ids)
        });
    Some(contract)
}

pub(super) fn relevant_story_bible_view(
    mut value: serde_json::Value,
    chapter_number: usize,
    relevant_names: &BTreeSet<String>,
    relevant_ids: &BTreeSet<String>,
) -> serde_json::Value {
    if let Some(characters) = value
        .get_mut("character_ledger")
        .and_then(serde_json::Value::as_array_mut)
    {
        characters.retain(|character| {
            character
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| relevant_names.contains(name))
                || character
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| relevant_ids.contains(id))
        });
    }
    if let Some(relations) = value
        .pointer_mut("/structured_contract_v2/relationship_ledger")
        .and_then(serde_json::Value::as_array_mut)
    {
        relations.retain(|relation| {
            serde_json::from_value::<RelationshipLedgerEntry>(relation.clone())
                .ok()
                .is_some_and(|relation| {
                    relationship_intersects_subgraph(&relation, relevant_names, relevant_ids)
                })
        });
    }
    for pointer in [
        "/narrative_graph/chapter_goals",
        "/structured_contract_v2/narrative_graph/chapter_goals",
    ] {
        if let Some(goals) = value
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_array_mut)
        {
            goals.retain(|goal| {
                goal.get("chapter_number")
                    .or_else(|| goal.get("number"))
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|number| number == chapter_number as u64)
            });
        }
    }
    for pointer in ["/hook_ledger", "/structured_contract_v2/payoff_matrix"] {
        if let Some(items) = value
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_array_mut)
        {
            items.retain(|item| {
                let introduced = item
                    .get("introduced_chapter")
                    .and_then(serde_json::Value::as_u64);
                let payoff = item
                    .get("payoff_chapter")
                    .and_then(serde_json::Value::as_u64);
                introduced.is_some_and(|number| number <= chapter_number as u64)
                    || payoff == Some(chapter_number as u64)
            });
        }
    }
    for pointer in [
        "/narrative_graph/volume_arcs",
        "/structured_contract_v2/narrative_graph/volume_arcs",
    ] {
        if let Some(object) = value.pointer_mut(pointer) {
            *object = serde_json::Value::Array(Vec::new());
        }
    }
    value
}

fn relationship_intersects_subgraph(
    relationship: &RelationshipLedgerEntry,
    relevant_names: &BTreeSet<String>,
    relevant_ids: &BTreeSet<String>,
) -> bool {
    relationship
        .characters
        .iter()
        .any(|name| relevant_names.contains(name))
        || relationship
            .character_ids
            .iter()
            .any(|id| relevant_ids.contains(id))
}

pub(super) fn sanitize_prompt_json_text_fields(
    mut value: serde_json::Value,
    manifest: &NovelProjectManifest,
) -> serde_json::Value {
    match &mut value {
        serde_json::Value::String(text) => {
            *text = repair_contract_character_name_typos(manifest, text);
        }
        serde_json::Value::Array(items) => {
            for item in items {
                let cleaned = sanitize_prompt_json_text_fields(std::mem::take(item), manifest);
                *item = cleaned;
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                let cleaned = sanitize_prompt_json_text_fields(std::mem::take(item), manifest);
                *item = cleaned;
            }
        }
        _ => {}
    }
    value
}

pub(super) fn build_prompt_context_payload(context: &serde_json::Value) -> serde_json::Value {
    let mut prompt_context = context.clone();

    if let Some(contract) = prompt_context.get_mut("contract") {
        compact_contract_prompt_view(contract);
        if let Some(outline) = contract.pointer_mut("/authority_contract/outline") {
            if let Some(object) = outline.as_object_mut() {
                object.remove("volumes");
            }
        }
    }

    if let Some(story_bible) = prompt_context.get_mut("story_bible") {
        compact_story_bible_prompt_view(story_bible);
    }

    if let Some(character_ledger) = prompt_context.get_mut("character_ledger") {
        compact_json_prompt_view(
            character_ledger,
            PROMPT_CONTRACT_ITEM_CHARS,
            PROMPT_CONTRACT_ARRAY_ITEMS,
        );
    }

    if let Some(project) = prompt_context
        .get_mut("project")
        .and_then(serde_json::Value::as_object_mut)
    {
        project.remove("volumes");
        project.remove("volume_summaries");
        if let Some(current_volume) = project
            .get_mut("current_volume")
            .and_then(serde_json::Value::as_object_mut)
        {
            for key in [
                "ending_change",
                "must_open",
                "must_payoff",
                "key_results",
                "summary",
                "emotional_curve",
            ] {
                current_volume.remove(key);
            }
        }
    }

    if let Some(truth_files) = prompt_context
        .get_mut("truth_files")
        .and_then(serde_json::Value::as_array_mut)
    {
        let mut remaining = PROMPT_TRUTH_TOTAL_CHARS;
        for truth in truth_files {
            let limit = PROMPT_TRUTH_FILE_CHARS.min(remaining);
            truncate_json_string_field_exact(truth, "content", limit);
            remaining = remaining.saturating_sub(
                truth
                    .get("content")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| value.chars().count())
                    .unwrap_or_default(),
            );
        }
    }

    if let Some(sources) = prompt_context
        .get_mut("sources")
        .and_then(serde_json::Value::as_array_mut)
    {
        for source in sources {
            truncate_json_string_field(source, "excerpt", PROMPT_SOURCE_EXCERPT_CHARS);
        }
    }

    shrink_compressible_context_to_budget(&mut prompt_context, PROMPT_CONTEXT_TOTAL_CHARS);
    let protected_paths = [
        "/project",
        "/contract",
        "/character_ledger",
        "/continuity_anchors",
        "/story_bible",
        "/narrative_progress",
        "/next_chapter_boundary",
        "/plan",
        "/chapter_contract",
        "/chapter_architecture",
        "/truth_files",
    ];
    let compressible_paths = ["/recent_chapters", "/archives", "/sources"];
    let protected_chars = context_paths_chars(&prompt_context, &protected_paths);
    let compressible_chars = context_paths_chars(&prompt_context, &compressible_paths);
    let total_chars = serde_json::to_string(&prompt_context)
        .unwrap_or_default()
        .chars()
        .count();

    if let Some(object) = prompt_context.as_object_mut() {
        object.insert(
            "context_layers".to_string(),
            json!({
                "schema": "benshu.context_layers.v1",
                "protected": {
                    "paths": protected_paths,
                    "chars": protected_chars,
                    "policy": "reserved authority; never removed to admit reference material"
                },
                "compressible": {
                    "paths": compressible_paths,
                    "chars": compressible_chars,
                    "policy": "progressively compacted when the total prompt budget is exceeded"
                },
                "total_chars_before_layer_metadata": total_chars,
                "total_budget_chars": PROMPT_CONTEXT_TOTAL_CHARS,
                "protected_overflow": total_chars > PROMPT_CONTEXT_TOTAL_CHARS
            }),
        );
        object.insert(
            "prompt_packaging".to_string(),
            json!({
                "schema": "compact_prompt_context.v1",
                "truth_file_content_chars": PROMPT_TRUTH_FILE_CHARS,
                "truth_total_chars": PROMPT_TRUTH_TOTAL_CHARS,
                "source_excerpt_chars": PROMPT_SOURCE_EXCERPT_CHARS,
                "contract_text_chars": PROMPT_CONTRACT_TEXT_CHARS,
                "contract_item_chars": PROMPT_CONTRACT_ITEM_CHARS,
                "contract_array_items": PROMPT_CONTRACT_ARRAY_ITEMS,
                "story_bible_text_chars": PROMPT_STORY_BIBLE_TEXT_CHARS,
                "story_bible_array_items": PROMPT_STORY_BIBLE_ARRAY_ITEMS,
                "recent_chapter_limit": CONTEXT_RECENT_CHAPTER_LIMIT,
                "archive_limit": CONTEXT_ARCHIVE_LIMIT,
                "total_context_chars": PROMPT_CONTEXT_TOTAL_CHARS,
                "format": "compact_json",
                "full_context": "persisted in runtime/chapter-XXXX.context.json"
            }),
        );
    }

    prompt_context
}

fn shrink_compressible_context_to_budget(
    prompt_context: &mut serde_json::Value,
    total_budget_chars: usize,
) {
    for max_string_chars in [600usize, 320, 160] {
        let current_chars = serde_json::to_string(prompt_context)
            .unwrap_or_default()
            .chars()
            .count();
        if current_chars <= total_budget_chars {
            break;
        }
        for key in ["sources", "archives", "recent_chapters"] {
            if let Some(value) = prompt_context.get_mut(key) {
                compact_json_prompt_view(value, max_string_chars, CONTEXT_RECENT_CHAPTER_LIMIT);
            }
        }
    }
}

fn context_paths_chars(value: &serde_json::Value, paths: &[&str]) -> usize {
    paths
        .iter()
        .filter_map(|path| value.pointer(path))
        .map(|section| {
            serde_json::to_string(section)
                .unwrap_or_default()
                .chars()
                .count()
        })
        .sum()
}

pub(super) fn prompt_context_fingerprint(prompt_context: &serde_json::Value) -> String {
    let raw = serde_json::to_vec(prompt_context).unwrap_or_default();
    hex::encode(sha2::Sha256::digest(raw))
}

fn compact_contract_prompt_view(contract: &mut serde_json::Value) {
    for key in [
        "premise",
        "outline",
        "ending_direction",
        "protagonist_arc",
        "main_causal_spine",
    ] {
        truncate_json_string_field(contract, key, PROMPT_CONTRACT_TEXT_CHARS);
    }

    for key in [
        "themes",
        "characters",
        "world_rules",
        "style_rules",
        "must_avoid",
    ] {
        truncate_json_string_array_field(
            contract,
            key,
            PROMPT_CONTRACT_ARRAY_ITEMS,
            PROMPT_CONTRACT_ITEM_CHARS,
        );
    }

    if let Some(structured) = contract.get_mut("structured_contract_v2") {
        compact_json_prompt_view(
            structured,
            PROMPT_CONTRACT_ITEM_CHARS,
            PROMPT_CONTRACT_ARRAY_ITEMS,
        );
    }
}

fn compact_story_bible_prompt_view(story_bible: &mut serde_json::Value) {
    compact_json_prompt_view(
        story_bible,
        PROMPT_STORY_BIBLE_TEXT_CHARS,
        PROMPT_STORY_BIBLE_ARRAY_ITEMS,
    );
}

fn compact_json_prompt_view(
    value: &mut serde_json::Value,
    max_string_chars: usize,
    max_array_items: usize,
) {
    match value {
        serde_json::Value::String(text) => {
            truncate_string_to_char_limit(text, max_string_chars);
        }
        serde_json::Value::Array(items) => {
            if items.len() > max_array_items {
                items.truncate(max_array_items);
            }
            for item in items {
                compact_json_prompt_view(item, max_string_chars, max_array_items);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                compact_json_prompt_view(item, max_string_chars, max_array_items);
            }
        }
        _ => {}
    }
}

fn truncate_string_to_char_limit(text: &mut String, max_chars: usize) {
    if max_chars == 0 {
        text.clear();
        return;
    }
    if text.chars().count() <= max_chars {
        return;
    }
    *text = preview_chars(text, max_chars);
}

fn truncate_json_string_array_field(
    value: &mut serde_json::Value,
    key: &str,
    max_items: usize,
    max_item_chars: usize,
) {
    let Some(items) = value.get_mut(key).and_then(serde_json::Value::as_array_mut) else {
        return;
    };
    if items.len() > max_items {
        items.truncate(max_items);
    }
    for item in items {
        match item {
            serde_json::Value::String(text) => truncate_string_to_char_limit(text, max_item_chars),
            other => compact_json_prompt_view(other, max_item_chars, max_items),
        }
    }
}

pub(super) fn build_context_budget_telemetry(
    context: &serde_json::Value,
    prompt_context: &serde_json::Value,
    language: &str,
) -> serde_json::Value {
    let section_scan = |key: &str, value: &serde_json::Value| {
        let section = value
            .get(key)
            .map(|section| serde_json::to_string(section).unwrap_or_default())
            .unwrap_or_default();
        serde_json::to_value(TextScanReport::scan(&section, language)).unwrap_or_else(|_| json!({}))
    };
    let full_context = serde_json::to_string(context).unwrap_or_default();
    let prompt = serde_json::to_string(prompt_context).unwrap_or_default();
    json!({
        "schema": "benshu.context_budget.v1",
        "full_context": TextScanReport::scan(&full_context, language),
        "prompt_context": TextScanReport::scan(&prompt, language),
        "sections": {
            "contract": section_scan("contract", prompt_context),
            "story_bible": section_scan("story_bible", prompt_context),
            "truth_files": section_scan("truth_files", prompt_context),
            "recent_chapters": section_scan("recent_chapters", prompt_context),
            "archives": section_scan("archives", prompt_context),
            "sources": section_scan("sources", prompt_context),
            "character_ledger": section_scan("character_ledger", prompt_context)
        },
        "limits": prompt_context.get("prompt_packaging").cloned().unwrap_or_else(|| json!({}))
    })
}

pub(super) fn truncate_json_string_field_exact(
    value: &mut serde_json::Value,
    field: &str,
    max_chars: usize,
) {
    let Some(content) = value.get(field).and_then(serde_json::Value::as_str) else {
        return;
    };
    if content.chars().count() <= max_chars {
        return;
    }
    let truncated = content.chars().take(max_chars).collect::<String>();
    if let Some(object) = value.as_object_mut() {
        object.insert(field.to_string(), serde_json::Value::String(truncated));
        object.insert(format!("{field}_truncated"), serde_json::Value::Bool(true));
    }
}

pub(super) fn truncate_json_string_field(
    value: &mut serde_json::Value,
    field: &str,
    max_chars: usize,
) {
    let Some(content) = value.get(field).and_then(serde_json::Value::as_str) else {
        return;
    };
    if max_chars == 0 {
        if let Some(object) = value.as_object_mut() {
            object.insert(field.to_string(), serde_json::Value::String(String::new()));
            object.insert(format!("{field}_truncated"), serde_json::Value::Bool(true));
        }
        return;
    }
    if content.chars().count() <= max_chars {
        return;
    }
    let truncated = preview_chars(content, max_chars);
    if let Some(object) = value.as_object_mut() {
        object.insert(field.to_string(), serde_json::Value::String(truncated));
        object.insert(format!("{field}_truncated"), serde_json::Value::Bool(true));
    }
}

pub(super) async fn build_context_governance(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> anyhow::Result<(
    governance::ContextPackage,
    governance::RuleStack,
    governance::ChapterTrace,
)> {
    let mut selected_context = Vec::new();
    let mut planner_inputs = Vec::new();
    let mut composer_inputs = Vec::new();

    if let Some(contract) = &manifest.contract {
        selected_context.push(governance::context_source(
            "contract.md",
            "Preserve the stable story contract.",
            Some(render_contract(contract)),
        ));
        planner_inputs.push("contract.md".to_string());
    }

    if let Some(bible) = &manifest.story_bible {
        let bible_prompt_view =
            sanitize_prompt_json_text_fields(novel_bible::story_bible_prompt_view(bible), manifest);
        let bible_prompt_text =
            serde_json::to_string(&bible_prompt_view).unwrap_or_else(|_| "{}".to_string());
        selected_context.push(governance::context_source(
            "story_bible.prompt.json",
            "Preserve the compact story bible view: ending-first design, world database, character anchors, hook ledger, genre controls, themes, and timeline.",
            Some(bible_prompt_text),
        ));
        planner_inputs.push("story_bible.prompt.json".to_string());
        composer_inputs.push("story_bible.prompt.json".to_string());
    }

    if let Some(chapter_contract) = manifest
        .chapter_contracts
        .iter()
        .find(|record| record.number == chapter_number)
    {
        let content = tokio::fs::read_to_string(project_dir.join(&chapter_contract.markdown_path))
            .await
            .unwrap_or_default();
        selected_context.push(governance::context_source(
            &chapter_contract.markdown_path,
            "Carry the current chapter control contract into drafting.",
            Some(content),
        ));
        planner_inputs.push(chapter_contract.markdown_path.clone());
    }

    if let Some(plan) = manifest
        .chapter_plans
        .iter()
        .find(|record| record.number == chapter_number)
    {
        selected_context.push(governance::context_source(
            &plan.path,
            "Anchor the current chapter plan.",
            Some(plan.plan.clone()),
        ));
        planner_inputs.push(plan.path.clone());
    }

    if let Some(architecture) = manifest
        .chapter_architectures
        .iter()
        .find(|record| record.number == chapter_number)
    {
        selected_context.push(governance::context_source(
            &architecture.path,
            "Carry the chapter scene architecture into drafting.",
            Some(architecture.architecture.clone()),
        ));
        composer_inputs.push(architecture.path.clone());
    }

    for truth in &manifest.truth_files {
        let raw = tokio::fs::read_to_string(project_dir.join(&truth.path))
            .await
            .unwrap_or_default();
        let content =
            repair_contract_character_name_typos(manifest, &truth_file_body(&truth.section, &raw));
        selected_context.push(governance::context_source(
            &truth.path,
            "Preserve durable truth and continuity facts.",
            Some(content),
        ));
        composer_inputs.push(truth.path.clone());
    }

    for archive in context_archives(manifest, chapter_number)
        .into_iter()
        .take(CONTEXT_ARCHIVE_LIMIT)
    {
        let content = tokio::fs::read_to_string(project_dir.join(&archive.path))
            .await
            .unwrap_or_default();
        selected_context.push(governance::context_source(
            &archive.path,
            "Use archived arc/volume continuity as a compact long-project memory layer.",
            Some(preview_chars(&content, ARCHIVE_EXCERPT_CHARS)),
        ));
        composer_inputs.push(archive.path.clone());
    }

    for chapter in approved_prior_chapters(manifest, chapter_number)
        .rev()
        .take(CONTEXT_RECENT_CHAPTER_LIMIT)
    {
        selected_context.push(governance::context_source(
            &chapter.path,
            "Use recent chapter continuity, not the full prose, to avoid drift.",
            Some(format!(
                "title: {}\nsummary: {}\nkey_facts: {}\ncontinuity_updates: {}",
                chapter.title,
                repair_contract_character_name_typos(manifest, &chapter.summary),
                clean_contract_character_name_typos(manifest, chapter.key_facts.clone()).join("; "),
                clean_contract_character_name_typos(manifest, chapter.continuity_updates.clone())
                    .join("; ")
            )),
        ));
        composer_inputs.push(chapter.path.clone());
    }

    for source in manifest.sources.iter().rev().take(CONTEXT_SOURCE_LIMIT) {
        let excerpt = source_material_excerpt(project_dir, source, SOURCE_CONTEXT_PAYLOAD_CHARS)
            .await
            .unwrap_or_default();
        selected_context.push(governance::context_source(
            &source.path,
            "Use source material as reference evidence, not copy text.",
            Some(source_context_entry(
                &source.title,
                source.notes.as_deref(),
                source.source_url.as_deref(),
                &excerpt,
            )),
        ));
        composer_inputs.push(source.path.clone());
    }

    for source in &mut selected_context {
        source.layer = if context_source_is_protected(&source.source) {
            "protected".to_string()
        } else {
            "compressible".to_string()
        };
    }
    let context_package = governance::build_context_package(chapter_number, selected_context);
    let rule_stack = governance::build_rule_stack(
        chapter_number,
        manifest.contract.is_some(),
        manifest
            .chapter_contracts
            .iter()
            .any(|record| record.number == chapter_number),
        manifest.truth_files.len(),
        manifest.sources.len(),
        manifest
            .chapter_architectures
            .iter()
            .any(|record| record.number == chapter_number),
    );
    let selected_sources = context_package
        .selected_context
        .iter()
        .map(|entry| entry.source.clone())
        .collect::<Vec<_>>();
    let mut trace = governance::build_trace(
        chapter_number,
        planner_inputs,
        composer_inputs,
        selected_sources,
    );
    trace.selection_decisions = context_package
        .selected_context
        .iter()
        .map(|source| governance::ContextSelectionDecision {
            source: source.source.clone(),
            layer: source.layer.clone(),
            reason: source.reason.clone(),
            original_chars: source.original_chars,
            selected_chars: source.selected_chars,
            truncated: source.truncated,
        })
        .collect();
    Ok((context_package, rule_stack, trace))
}

fn context_source_is_protected(source: &str) -> bool {
    source == "contract.md"
        || source == "story_bible.prompt.json"
        || source.starts_with("truth/")
        || source.starts_with("plans/")
        || source.contains(".contract.")
        || source.contains(".architecture.")
}

pub(super) fn approved_prior_chapters(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> impl DoubleEndedIterator<Item = &ChapterRecord> {
    manifest
        .chapters
        .iter()
        .filter(move |chapter| chapter.number < chapter_number && chapter_is_approved(chapter))
}

pub(super) fn context_archives(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> Vec<&LongformArchiveRecord> {
    let mut archives = manifest
        .archives
        .iter()
        .filter(|archive| archive.range_end < chapter_number)
        .collect::<Vec<_>>();
    archives.sort_by(|left, right| {
        let kind_rank = |kind: &str| {
            if kind.eq_ignore_ascii_case("volume") {
                0
            } else {
                1
            }
        };
        right
            .range_end
            .cmp(&left.range_end)
            .then_with(|| kind_rank(&left.kind).cmp(&kind_rank(&right.kind)))
    });
    archives
}
