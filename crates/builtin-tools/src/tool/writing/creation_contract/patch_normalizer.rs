use super::*;
use crate::tool::writing::creation_contract_normalizer;
use crate::tool::writing::novel_contract_v2::{
    ChapterEndingRotation, CharacterVoiceProfile, ConflictPressureCurve, MotifLedgerEntry,
    PressureBeat, ReaderPromise, RelationshipInteractionQuota, RevealScheduleEntry, SceneTypeMix,
};
use serde_json::Value;

const MAX_NEAR_CHAPTERS: usize = 8;

pub(crate) fn normalize_creation_contract_patch_boundary(
    draft: &SessionCreationDraftState,
    raw: &str,
) -> Option<CreationContractPatch> {
    normalize_patch_json(raw).or_else(|| normalize_patch_field_pack(draft, raw))
}

fn normalize_patch_json(raw: &str) -> Option<CreationContractPatch> {
    let value = raw_json_value(raw)?;
    let object = value.as_object()?;
    let mut patches = Vec::new();
    if let Some(title) = object.get("title_patch") {
        push_patch(
            &mut patches,
            title_patch_from_value(title).map(CreationContractPatch::Title),
        );
    }
    if let Some(skeleton) = object.get("skeleton_patch") {
        push_patch(
            &mut patches,
            skeleton_patch_from_value(skeleton).map(CreationContractPatch::Skeleton),
        );
    }
    if let Some(characters) = object
        .get("character_patch")
        .or_else(|| object.get("characters_patch"))
    {
        push_patch(
            &mut patches,
            character_patch_from_value(characters).map(CreationContractPatch::Characters),
        );
    }
    if let Some(plot) = object.get("plot_patch") {
        push_patch(
            &mut patches,
            plot_patch_from_value(plot).map(CreationContractPatch::Plot),
        );
    }
    if let Some(governance) = object.get("governance_patch") {
        push_patch(
            &mut patches,
            governance_patch_from_value(governance).map(CreationContractPatch::Governance),
        );
    }
    if let Some(metadata) = object.get("metadata_patch") {
        push_patch(
            &mut patches,
            metadata_patch_from_value(metadata).map(CreationContractPatch::Metadata),
        );
    }
    if !patches.is_empty() {
        return Some(collapse_patch_batch(patches));
    }

    let patch_type = object_get_alias(object, &["patch_type", "patchtype", "patchType", "type"])
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace(['_', '-', ' '], "");
    match patch_type.as_str() {
        "titlepatch" | "title" => title_patch_from_value(&value).map(CreationContractPatch::Title),
        "skeletonpatch" | "skeleton" => skeleton_patch_from_value(&value)
            .map(CreationContractPatch::Skeleton)
            .map(|patch| with_optional_title_patch(&value, patch)),
        "characterpatch" | "characterspatch" | "characters" => {
            character_patch_from_value(&value).map(CreationContractPatch::Characters)
        }
        "plotpatch" | "plot" | "outlinepatch" => {
            plot_patch_from_value(&value).map(CreationContractPatch::Plot)
        }
        "governancepatch" | "governance" => {
            governance_patch_from_value(&value).map(CreationContractPatch::Governance)
        }
        "metadatapatch" | "metadata" => {
            metadata_patch_from_value(&value).map(CreationContractPatch::Metadata)
        }
        _ => {
            let has_title_metadata = object.contains_key("title")
                || object.contains_key("title_metadata")
                || object.contains_key("titleMetadata")
                || object.contains_key("canonical_title")
                || object.contains_key("canonicalTitle")
                || object.contains_key("book_title")
                || object.contains_key("bookTitle")
                || object.contains_key("work_title")
                || object.contains_key("workTitle");
            let has_outline_metadata = object.contains_key("outline")
                || object.contains_key("near_chapters")
                || object.contains_key("nearChapters")
                || object.contains_key("chapter_plan")
                || object.contains_key("chapterPlan")
                || object.contains_key("chapters")
                || object.contains_key("volumes")
                || object.contains_key("volume_arcs")
                || object.contains_key("volumeArcs")
                || object.contains_key("分卷")
                || object.contains_key("卷宗")
                || object.contains_key("近期章节")
                || object.contains_key("章节规划");
            let has_governance_metadata = object_contains_governance_metadata(object);
            if has_title_metadata && (has_outline_metadata || has_governance_metadata) {
                metadata_patch_from_value(&value).map(CreationContractPatch::Metadata)
            } else if has_title_metadata {
                title_patch_from_value(&value).map(CreationContractPatch::Title)
            } else if object.contains_key("characters") {
                character_patch_from_value(&value).map(CreationContractPatch::Characters)
            } else if has_outline_metadata {
                plot_patch_from_value(&value).map(CreationContractPatch::Plot)
            } else if has_governance_metadata {
                governance_patch_from_value(&value).map(CreationContractPatch::Governance)
            } else if object.contains_key("premise")
                || object.contains_key("ending")
                || object.contains_key("main_causal_spine")
            {
                skeleton_patch_from_value(&value)
                    .map(CreationContractPatch::Skeleton)
                    .map(|patch| with_optional_title_patch(&value, patch))
            } else {
                None
            }
        }
    }
}

fn object_contains_governance_metadata(object: &serde_json::Map<String, Value>) -> bool {
    object.contains_key("themes")
        || object.contains_key("world_rules")
        || object.contains_key("worldRules")
        || object.contains_key("世界规则")
        || object.contains_key("规则")
        || object.contains_key("emotional_contract")
        || object.contains_key("structured")
        || object.contains_key("scene_type_mix")
        || object.contains_key("character_voice_ledger")
        || object.contains_key("reader_promise")
        || object.contains_key("chapter_ending_rotation")
        || object.contains_key("conflict_pressure_curve")
        || object.contains_key("motif_ledger")
        || object.contains_key("reveal_schedule")
        || object.contains_key("relationship_interaction_quotas")
        || object.contains_key("场景类型配比")
        || object.contains_key("角色声音表")
        || object.contains_key("读者期待")
        || object.contains_key("爽点合同")
        || object.contains_key("章节收尾轮换")
        || object.contains_key("冲突升降压曲线")
        || object.contains_key("主题母题")
        || object.contains_key("信息揭示节奏")
        || object.contains_key("角色关系互动配额")
}

fn with_optional_title_patch(value: &Value, patch: CreationContractPatch) -> CreationContractPatch {
    let Some(title_patch) = title_patch_from_value(value).map(CreationContractPatch::Title) else {
        return patch;
    };
    CreationContractPatch::Batch(vec![patch, title_patch])
}

fn normalize_patch_field_pack(
    _draft: &SessionCreationDraftState,
    raw: &str,
) -> Option<CreationContractPatch> {
    let normalized_field_pack = normalize_generated_contract_field_pack_lines(raw);
    let raw = normalized_field_pack.as_str();
    let mut patches = Vec::new();
    push_patch(
        &mut patches,
        title_patch_from_field_pack(raw).map(CreationContractPatch::Title),
    );
    push_patch(
        &mut patches,
        skeleton_patch_from_field_pack(raw).map(CreationContractPatch::Skeleton),
    );
    push_patch(
        &mut patches,
        character_patch_from_field_pack(raw).map(CreationContractPatch::Characters),
    );
    push_patch(
        &mut patches,
        plot_patch_from_field_pack(raw).map(CreationContractPatch::Plot),
    );
    push_patch(
        &mut patches,
        governance_patch_from_field_pack(raw).map(CreationContractPatch::Governance),
    );
    (!patches.is_empty()).then(|| collapse_patch_batch(patches))
}

fn push_patch(target: &mut Vec<CreationContractPatch>, patch: Option<CreationContractPatch>) {
    if let Some(patch) = patch {
        target.push(patch);
    }
}

fn collapse_patch_batch(mut patches: Vec<CreationContractPatch>) -> CreationContractPatch {
    if patches.len() == 1 {
        patches.remove(0)
    } else {
        CreationContractPatch::Batch(patches)
    }
}

fn raw_json_value(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    if let Some(normalized) =
        creation_contract_normalizer::normalize_creation_contract_boundary(trimmed)
    {
        return Some(normalized.value);
    }
    let fenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim);
    if let Some(fenced) = fenced {
        if let Ok(value) = serde_json::from_str::<Value>(fenced) {
            return Some(value);
        }
        if let Some(normalized) =
            creation_contract_normalizer::normalize_creation_contract_boundary(fenced)
        {
            return Some(normalized.value);
        }
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }
    let candidate = &trimmed[start..=end];
    serde_json::from_str::<Value>(candidate).ok().or_else(|| {
        creation_contract_normalizer::normalize_creation_contract_boundary(candidate)
            .map(|normalized| normalized.value)
    })
}

fn title_patch_from_value(value: &Value) -> Option<TitlePatch> {
    let object = value.as_object()?;
    let title_object = object
        .get("title")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let mut canonical_title = string_aliases(
        title_object,
        &[
            "canonical_title",
            "title",
            "book_title",
            "work_title",
            "书名",
            "标题",
            "作品名",
        ],
    );
    let mut rationale = string_aliases(
        title_object,
        &[
            "rationale",
            "title_rationale",
            "reason",
            "basis",
            "书名理由",
            "命名理由",
            "标题理由",
        ],
    );
    if value_missing(&rationale) {
        rationale = string_aliases(
            object,
            &[
                "rationale",
                "title_rationale",
                "reason",
                "basis",
                "书名理由",
                "命名理由",
                "标题理由",
            ],
        );
    }
    let (mut candidates, candidate_rationales, candidate_hook_types) =
        title_candidate_metadata_aliases(
            title_object,
            &["candidates", "title_candidates", "书名候选", "标题候选"],
        );
    if value_missing(&canonical_title) {
        canonical_title = candidates.first().cloned().unwrap_or_default();
    }
    if value_missing(&canonical_title) {
        canonical_title = infer_book_title_from_rationale_text(&rationale);
    }
    if !value_missing(&canonical_title) && candidates.is_empty() {
        candidates.push(canonical_title.clone());
    }
    if value_missing(&rationale) && !value_missing(&canonical_title) {
        rationale = candidate_rationales
            .get(canonical_title.trim())
            .cloned()
            .or_else(|| {
                candidate_rationales
                    .values()
                    .find(|value| !value_missing(value))
                    .cloned()
            })
            .unwrap_or_default();
    }
    if value_missing(&canonical_title) && value_missing(&rationale) && candidates.is_empty() {
        return None;
    }
    Some(TitlePatch {
        canonical_title,
        candidates,
        candidate_rationales,
        candidate_hook_types,
        rationale,
        source: TitleSource::LlmContract,
    })
}

fn skeleton_patch_from_value(value: &Value) -> Option<SkeletonPatch> {
    let object = value.as_object()?;
    let ending_object = object.get("ending").and_then(Value::as_object);
    let patch = SkeletonPatch {
        genre: string_aliases(object, &["genre", "题材", "类型"]),
        brief: string_aliases(object, &["brief", "summary", "简述", "创作简述"]),
        target_units: usize_aliases(object, &["target_units", "total_units", "总字数"]),
        chapter_unit_target: longform_policy::normalize_user_chapter_unit_target(usize_aliases(
            object,
            &[
                "chapter_unit_target",
                "chapter_units",
                "每章字数",
                "每章档位",
            ],
        )),
        max_chapters_per_turn: usize_aliases(object, &["max_chapters_per_turn", "每轮最多章节"]),
        premise: string_aliases(object, &["premise", "story_premise", "故事前提", "前提"]),
        ending_desired_resolution: ending_object
            .map(|ending| {
                string_aliases(
                    ending,
                    &["desired_resolution", "ending_direction", "终局方向"],
                )
            })
            .unwrap_or_else(|| {
                string_aliases(
                    object,
                    &["ending_direction", "desired_resolution", "终局方向"],
                )
            }),
        ending_final_state: ending_object
            .map(|ending| string_aliases(ending, &["final_state", "终局状态"]))
            .unwrap_or_else(|| string_aliases(object, &["final_state", "终局状态"])),
        protagonist_arc: string_aliases(
            object,
            &["protagonist_arc", "主角弧线", "成长线", "主角弧光"],
        ),
        world_imagery: string_aliases(
            object,
            &[
                "world_imagery",
                "worldImagery",
                "world_imaginery",
                "worldImaginery",
                "世界观意象",
                "世界观核心意象",
                "世界意象",
                "核心意象",
            ],
        ),
        main_causal_spine: string_aliases(
            object,
            &[
                "main_causal_spine",
                "main_spine",
                "总主线因果链",
                "主线因果链",
            ],
        ),
    };
    (!skeleton_patch_empty(&patch)).then_some(patch)
}

fn character_patch_from_value(value: &Value) -> Option<CharacterPatch> {
    if !value.is_object() {
        let characters = characters_from_value(value);
        return (!characters.is_empty()).then_some(CharacterPatch {
            characters,
            relationship_ledger: Vec::new(),
            emotional_state_ledger: Vec::new(),
        });
    }
    let object = value.as_object()?;
    let characters = object
        .get("characters")
        .or_else(|| object.get("character_authority"))
        .or_else(|| object.get("character_ledger"))
        .or_else(|| object.get("characterLedger"))
        .or_else(|| object.get("角色权威表"))
        .or_else(|| object.get("人物权威表"))
        .or_else(|| object.get("角色表"))
        .or_else(|| object.get("人物表"))
        .or_else(|| object.get("主要人物"))
        .or_else(|| object.get("角色"))
        .map(characters_from_value)
        .unwrap_or_default();
    if characters.is_empty() {
        return None;
    }
    let relationship_ledger = object
        .get("relationship_ledger")
        .or_else(|| object.get("relationshipLedger"))
        .or_else(|| object.get("关系线"))
        .or_else(|| object.get("关系账本"))
        .or_else(|| object.get("人物关系"))
        .or_else(|| {
            object
                .get("structured")
                .and_then(Value::as_object)
                .and_then(|structured| {
                    object_get_alias(
                        structured,
                        &[
                            "relationship_ledger",
                            "relationshipLedger",
                            "关系线",
                            "关系账本",
                        ],
                    )
                })
        })
        .map(relationship_ledger_from_value)
        .unwrap_or_default();
    let emotional_state_ledger = object
        .get("emotional_state_ledger")
        .or_else(|| object.get("emotionalStateLedger"))
        .or_else(|| object.get("情绪状态"))
        .or_else(|| object.get("情绪账本"))
        .or_else(|| {
            object
                .get("structured")
                .and_then(Value::as_object)
                .and_then(|structured| {
                    object_get_alias(
                        structured,
                        &[
                            "emotional_state_ledger",
                            "emotionalStateLedger",
                            "情绪状态",
                            "情绪账本",
                        ],
                    )
                })
        })
        .map(emotional_state_ledger_from_value)
        .unwrap_or_default();
    Some(CharacterPatch {
        characters,
        relationship_ledger,
        emotional_state_ledger,
    })
}

fn plot_patch_from_value(value: &Value) -> Option<PlotPatch> {
    let object = value.as_object()?;
    let outline_value =
        object_get_alias(object, &["outline", "outline_patch", "outlinePatch"]).unwrap_or(value);
    let outline_object = outline_value.as_object().unwrap_or(object);
    let raw_outline = string_aliases(
        outline_object,
        &[
            "raw_outline",
            "rawoutline",
            "outline",
            "summary",
            "大纲",
            "全书大纲",
            "故事大纲",
            "结构合同",
        ],
    );
    let derived_outline = derive_plot_contract_from_outline_text(&raw_outline);
    let mut volumes = outline_object
        .get("volumes")
        .or_else(|| object_get_alias(outline_object, &["volume_arcs", "volumeArcs"]))
        .or_else(|| outline_object.get("分卷"))
        .or_else(|| outline_object.get("卷宗"))
        .or_else(|| outline_object.get("卷规划"))
        .or_else(|| object_get_alias(object, &["volumes", "volume_arcs", "volumeArcs"]))
        .or_else(|| object.get("分卷"))
        .or_else(|| object.get("卷宗"))
        .or_else(|| object.get("卷规划"))
        .cloned()
        .map(|value| volume_contracts_from_value(&value))
        .unwrap_or_default();
    if volumes.is_empty() {
        volumes = derived_outline.volumes.clone();
    }
    let mut near_chapters = outline_object
        .get("near_chapters")
        .or_else(|| {
            object_get_alias(
                outline_object,
                &["nearChapters", "chapter_plan", "chapterPlan"],
            )
        })
        .or_else(|| outline_object.get("近期章节包"))
        .or_else(|| outline_object.get("近期章节"))
        .or_else(|| outline_object.get("章节规划"))
        .or_else(|| {
            object_get_alias(
                object,
                &[
                    "near_chapters",
                    "nearChapters",
                    "chapter_plan",
                    "chapterPlan",
                ],
            )
        })
        .or_else(|| object.get("近期章节包"))
        .or_else(|| object.get("近期章节"))
        .or_else(|| object.get("章节规划"))
        .or_else(|| object.get("chapters"))
        .cloned()
        .map(|value| chapter_seed_contracts_from_value(&value))
        .unwrap_or_default();
    if near_chapters.is_empty() {
        near_chapters = derived_outline.near_chapters.clone();
    }
    let mut payoff_matrix = object
        .get("payoff_matrix")
        .or_else(|| object_get_alias(object, &["payoffMatrix", "promises", "hooks"]))
        .or_else(|| object.get("伏笔矩阵"))
        .or_else(|| object.get("承诺兑现矩阵"))
        .or_else(|| object.get("伏笔"))
        .or_else(|| {
            object
                .get("structured")
                .and_then(Value::as_object)
                .and_then(|structured| structured.get("payoff_matrix"))
        })
        .cloned()
        .map(|value| payoff_matrix_from_value(&value))
        .unwrap_or_default();
    if payoff_matrix.is_empty() {
        payoff_matrix = derived_outline.payoff_matrix.clone();
    }
    deduplicate_volumes(&mut volumes);
    normalize_near_chapter_window(&mut near_chapters);
    let patch = PlotPatch {
        volumes,
        near_chapters,
        raw_outline,
        payoff_matrix,
    };
    (!patch.volumes.is_empty()
        || !patch.near_chapters.is_empty()
        || !value_missing(&patch.raw_outline)
        || !patch.payoff_matrix.is_empty())
    .then_some(patch)
}

fn governance_patch_from_value(value: &Value) -> Option<GovernancePatch> {
    let object = value.as_object()?;
    let mut structured = object_get_alias(object, &["structured", "contract_v2", "contractV2"])
        .cloned()
        .and_then(|value| serde_json::from_value::<NovelContractV2>(value).ok())
        .unwrap_or_default();
    // The payoff matrix is owned by PlotPatch. A governance response may echo
    // the stable matrix, but it must never acquire authority to replace it.
    structured.payoff_matrix.clear();
    if let Some(value) = object_get_alias(
        object,
        &["scene_type_mix", "sceneTypeMix", "场景类型配比", "场景配比"],
    )
    .cloned()
    .and_then(|value| serde_json::from_value::<SceneTypeMix>(value).ok())
    {
        structured.scene_type_mix = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &[
            "character_voice_ledger",
            "characterVoiceLedger",
            "角色声音表",
            "角色对白表",
        ],
    )
    .cloned()
    .and_then(|value| serde_json::from_value::<Vec<CharacterVoiceProfile>>(value).ok())
    {
        structured.character_voice_ledger = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &["reader_promise", "readerPromise", "读者期待", "爽点合同"],
    )
    .cloned()
    .and_then(|value| serde_json::from_value::<ReaderPromise>(value).ok())
    {
        structured.reader_promise = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &[
            "chapter_ending_rotation",
            "chapterEndingRotation",
            "章节收尾轮换",
            "章尾轮换",
        ],
    )
    .cloned()
    .and_then(|value| serde_json::from_value::<ChapterEndingRotation>(value).ok())
    {
        structured.chapter_ending_rotation = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &[
            "conflict_pressure_curve",
            "conflictPressureCurve",
            "冲突升降压曲线",
            "冲突曲线",
        ],
    )
    .cloned()
    .and_then(|value| serde_json::from_value::<ConflictPressureCurve>(value).ok())
    {
        structured.conflict_pressure_curve = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &["motif_ledger", "motifLedger", "主题母题", "母题账本"],
    )
    .cloned()
    .and_then(|value| serde_json::from_value::<Vec<MotifLedgerEntry>>(value).ok())
    {
        structured.motif_ledger = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &[
            "reveal_schedule",
            "revealSchedule",
            "信息揭示节奏",
            "揭示时间表",
        ],
    )
    .cloned()
    .and_then(|value| serde_json::from_value::<Vec<RevealScheduleEntry>>(value).ok())
    {
        structured.reveal_schedule = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &[
            "relationship_interaction_quotas",
            "relationshipInteractionQuotas",
            "角色关系互动配额",
            "关系互动配额",
        ],
    )
    .cloned()
    .and_then(|value| serde_json::from_value::<Vec<RelationshipInteractionQuota>>(value).ok())
    {
        structured.relationship_interaction_quotas = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &[
            "resource_economy",
            "resourceEconomy",
            "资源经济",
            "资源体系",
        ],
    )
    .cloned()
    .and_then(|value| {
        serde_json::from_value::<crate::tool::writing::novel_contract_v2::ResourceEconomy>(value)
            .ok()
    }) {
        structured.resource_economy = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &[
            "power_progression",
            "powerProgression",
            "成长体系",
            "力量体系",
            "修炼体系",
        ],
    )
    .cloned()
    .and_then(|value| {
        serde_json::from_value::<crate::tool::writing::novel_contract_v2::PowerProgression>(value)
            .ok()
    }) {
        structured.power_progression = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &["social_order", "socialOrder", "社会秩序", "等级秩序"],
    )
    .cloned()
    .and_then(|value| {
        serde_json::from_value::<crate::tool::writing::novel_contract_v2::SocialOrder>(value).ok()
    }) {
        structured.social_order = value;
    }
    if let Some(value) =
        object_get_alias(object, &["time_model", "timeModel", "时间模型", "时间规则"])
            .cloned()
            .and_then(|value| {
                serde_json::from_value::<crate::tool::writing::novel_contract_v2::TimeModel>(value)
                    .ok()
            })
    {
        structured.time_model = value;
    }
    if let Some(value) = object_get_alias(
        object,
        &["geography_model", "geographyModel", "地理模型", "空间模型"],
    )
    .cloned()
    .and_then(|value| {
        serde_json::from_value::<crate::tool::writing::novel_contract_v2::GeographyModel>(value)
            .ok()
    }) {
        structured.geography_model = value;
    }
    let patch = GovernancePatch {
        themes: list_aliases(object, &["themes", "主题", "核心主题", "主题承诺"]),
        world_rules: list_aliases(object, &["world_rules", "worldRules", "世界规则", "规则"]),
        style_rules: list_aliases(
            object,
            &["style_rules", "styleRules", "叙事风格", "文风", "风格"],
        ),
        must_avoid: list_aliases(
            object,
            &[
                "must_avoid",
                "mustAvoid",
                "avoid",
                "必须避免",
                "禁忌",
                "禁区",
            ],
        ),
        emotional_contract: object_get_alias(object, &["emotional_contract", "emotionalContract"])
            .or_else(|| object.get("情感合同"))
            .or_else(|| object.get("情感线"))
            .cloned()
            .and_then(|value| serde_json::from_value::<EmotionalContract>(value).ok())
            .unwrap_or_default(),
        relationship_ledger: object_get_alias(
            object,
            &["relationship_ledger", "relationshipLedger"],
        )
        .or_else(|| {
            object_get_alias(object, &["structured", "contract_v2", "contractV2"])
                .and_then(Value::as_object)
                .and_then(|structured| {
                    object_get_alias(structured, &["relationship_ledger", "relationshipLedger"])
                })
        })
        .map(relationship_ledger_from_value)
        .unwrap_or_default(),
        antagonist_pressure: object_get_alias(
            object,
            &["antagonist_pressure", "antagonistPressure"],
        )
        .or_else(|| object.get("对手压力"))
        .or_else(|| object.get("反派压力"))
        .and_then(antagonist_pressure_from_value)
        .unwrap_or_default(),
        narration_contract: object_get_alias(object, &["narration_contract", "narrationContract"])
            .or_else(|| object.get("叙事合同"))
            .cloned()
            .and_then(|value| serde_json::from_value::<NarrationContract>(value).ok())
            .unwrap_or_default(),
        structured,
    };
    let has_structured = serde_json::to_value(&patch.structured)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .map(|object| {
            object
                .values()
                .any(|value| !value.is_null() && value != &Value::Array(Vec::new()))
        })
        .unwrap_or(false);
    (!patch.themes.is_empty()
        || !patch.world_rules.is_empty()
        || !patch.style_rules.is_empty()
        || !patch.must_avoid.is_empty()
        || !value_missing(&patch.emotional_contract.primary_emotion)
        || !patch.relationship_ledger.is_empty()
        || !value_missing(&patch.antagonist_pressure.primary_pressure)
        || !patch.antagonist_pressure.antagonists.is_empty()
        || !value_missing(&patch.narration_contract.pov)
        || has_structured)
        .then_some(patch)
}

fn antagonist_pressure_from_value(value: &Value) -> Option<AntagonistPressure> {
    let mut pressure = serde_json::from_value::<AntagonistPressure>(value.clone()).ok()?;
    let object = value.as_object()?;
    if value_missing(&pressure.primary_pressure) {
        pressure.primary_pressure = string_aliases(
            object,
            &[
                "primary_pressure",
                "primary_antagonist",
                "主要对手",
                "压力源",
            ],
        );
    }
    if pressure.antagonists.is_empty() {
        pressure.antagonists = object
            .get("antagonists")
            .or_else(|| object.get("对手列表"))
            .cloned()
            .and_then(|value| {
                serde_json::from_value::<
                    Vec<crate::tool::writing::novel_contract_v2::AntagonistRecord>,
                >(value)
                .ok()
            })
            .unwrap_or_default();
    }
    Some(pressure)
}

fn metadata_patch_from_value(value: &Value) -> Option<MetadataPatch> {
    let object = value.as_object()?;
    let title = title_patch_from_value(value).unwrap_or_default();
    let outline_object = object
        .get("outline")
        .or_else(|| object.get("outline_patch"))
        .or_else(|| object.get("outlinePatch"))
        .and_then(Value::as_object);
    let volumes = outline_object
        .and_then(|outline| {
            outline
                .get("volumes")
                .or_else(|| object_get_alias(outline, &["volume_arcs", "volumeArcs"]))
                .or_else(|| outline.get("分卷"))
                .or_else(|| outline.get("卷宗"))
                .or_else(|| outline.get("卷规划"))
        })
        .or_else(|| {
            object
                .get("volumes")
                .or_else(|| object_get_alias(object, &["volume_arcs", "volumeArcs"]))
                .or_else(|| object.get("分卷"))
                .or_else(|| object.get("卷宗"))
                .or_else(|| object.get("卷规划"))
        })
        .cloned()
        .map(|value| volume_contracts_from_value(&value))
        .unwrap_or_default();
    let mut near_chapters = outline_object
        .and_then(|outline| {
            outline
                .get("near_chapters")
                .or_else(|| {
                    object_get_alias(outline, &["nearChapters", "chapter_plan", "chapterPlan"])
                })
                .or_else(|| outline.get("近期章节包"))
                .or_else(|| outline.get("近期章节"))
                .or_else(|| outline.get("章节规划"))
                .or_else(|| outline.get("chapters"))
        })
        .or_else(|| {
            object
                .get("near_chapters")
                .or_else(|| {
                    object_get_alias(object, &["nearChapters", "chapter_plan", "chapterPlan"])
                })
                .or_else(|| object.get("近期章节包"))
                .or_else(|| object.get("近期章节"))
                .or_else(|| object.get("章节规划"))
                .or_else(|| object.get("chapters"))
        })
        .cloned()
        .map(|value| chapter_seed_contracts_from_value(&value))
        .unwrap_or_default();
    let world_rules = list_aliases(
        object,
        &[
            "world_rules",
            "worldRules",
            "世界规则",
            "规则",
            "世界观规则",
            "world_governance_rules",
        ],
    );
    normalize_near_chapter_window(&mut near_chapters);
    let patch = MetadataPatch {
        title,
        world_rules,
        volumes,
        near_chapters,
    };
    (!value_missing(&patch.title.canonical_title)
        || !value_missing(&patch.title.rationale)
        || !patch.world_rules.is_empty()
        || !patch.volumes.is_empty()
        || !patch.near_chapters.is_empty())
    .then_some(patch)
}

fn title_patch_from_field_pack(raw: &str) -> Option<TitlePatch> {
    let rationale =
        field_string_preserve_sentence(raw, &["书名理由", "命名理由", "标题理由", "rationale"])
            .unwrap_or_default();
    let mut candidates =
        field_list(raw, &["书名候选", "标题候选", "title_candidates"]).unwrap_or_default();
    candidates.extend(title_candidates_from_numbered_field_pack(raw));
    candidates = dedup_compact_contract_values(candidates, 5, 18);
    let inferred_title = infer_book_title_from_rationale_text(&rationale);
    let canonical_title = field_string(raw, &["书名", "标题", "canonical_title", "title"])
        .or_else(|| (!value_missing(&inferred_title)).then_some(inferred_title))
        .or_else(|| candidates.first().cloned())?;
    let candidates = if candidates.is_empty() {
        vec![canonical_title.clone()]
    } else {
        candidates
    };
    Some(TitlePatch {
        canonical_title,
        candidates,
        candidate_rationales: BTreeMap::new(),
        candidate_hook_types: BTreeMap::new(),
        rationale,
        source: TitleSource::LlmContract,
    })
}

fn title_candidates_from_numbered_field_pack(raw: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    for line in raw.lines() {
        let cleaned = line
            .trim()
            .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'))
            .trim();
        let lowered = cleaned.to_ascii_lowercase();
        let looks_like_candidate_line = cleaned.starts_with("候选")
            || cleaned.starts_with("书名候选")
            || cleaned.starts_with("标题候选")
            || lowered.starts_with("candidate");
        if !looks_like_candidate_line {
            continue;
        }
        let value = cleaned
            .split_once('：')
            .or_else(|| cleaned.split_once(':'))
            .map(|(_, value)| value)
            .unwrap_or(cleaned)
            .trim();
        if let Some(quoted) = first_cjk_quote_value(value) {
            candidates.push(quoted);
            continue;
        }
        let value = value
            .split(['-', '—', '，', ',', '；', ';'])
            .next()
            .map(str::trim)
            .unwrap_or_default();
        if !value_missing(value) {
            candidates.push(sanitize_generated_contract_scalar(value));
        }
    }
    dedup_compact_contract_values(candidates, 5, 18)
}

pub(crate) fn infer_book_title_from_rationale_text(rationale: &str) -> String {
    let rationale = rationale.trim();
    if rationale.is_empty() {
        return String::new();
    }
    for (left, right) in [
        ('《', '》'),
        ('“', '”'),
        ('‘', '’'),
        ('"', '"'),
        ('「', '」'),
    ] {
        let mut rest = rationale;
        while let Some((_, after_left)) = rest.split_once(left) {
            let Some((candidate, after_right)) = after_left.split_once(right) else {
                break;
            };
            if title_candidate_from_rationale_looks_usable(candidate) {
                return sanitize_generated_contract_scalar(candidate);
            }
            rest = after_right;
        }
    }

    let markers = [
        "最终书名",
        "正式书名",
        "作品名",
        "书名",
        "标题",
        "canonical_title",
        "title",
    ];
    for marker in markers {
        let Some((_, after_marker)) = rationale.split_once(marker) else {
            continue;
        };
        let after_marker = after_marker.trim_start();
        let explains_title_origin = ["来自", "源自", "取自", "出自", "来自于", "源于"]
            .iter()
            .any(|prefix| after_marker.starts_with(prefix));
        if explains_title_origin {
            continue;
        }
        let candidate = after_marker
            .trim_start_matches(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '为' | '是' | '叫' | '定' | '取' | '名' | '：' | ':' | '=' | '-' | '—'
                    )
            })
            .trim();
        let candidate = trim_inferred_title_before_explanation(candidate);
        if title_candidate_from_rationale_looks_usable(candidate) {
            return sanitize_generated_contract_scalar(candidate);
        }
    }
    String::new()
}

fn trim_inferred_title_before_explanation(value: &str) -> &str {
    let mut end = value.len();
    for delimiter in ['，', '。', '；', ';', ',', '.', '\n', '\r'] {
        if let Some(index) = value.find(delimiter) {
            end = end.min(index);
        }
    }
    for marker in [
        "直接对应",
        "对应",
        "来自",
        "源自",
        "源于",
        "取自",
        "体现",
        "暗合",
        "呼应",
        "代表",
        "指向",
    ] {
        if let Some(index) = value.find(marker) {
            end = end.min(index);
        }
    }
    value[..end].trim()
}

fn title_candidate_from_rationale_looks_usable(candidate: &str) -> bool {
    let candidate = sanitize_generated_contract_scalar(candidate);
    let len = candidate.chars().count();
    (3..=14).contains(&len)
        && candidate
            .chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
        && !candidate.contains("书名")
        && !candidate.contains("标题")
        && !candidate.contains("理由")
        && !candidate.contains("候选")
        && !candidate.contains("字段")
        && !candidate.contains("JSON")
        && !candidate.contains("json")
}

fn title_candidate_metadata_aliases(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> (
    Vec<String>,
    BTreeMap<String, String>,
    BTreeMap<String, String>,
) {
    let Some(value) = object_get_alias(object, keys) else {
        return (Vec::new(), BTreeMap::new(), BTreeMap::new());
    };
    let Some(items) = value.as_array() else {
        return (list_aliases(object, keys), BTreeMap::new(), BTreeMap::new());
    };

    let mut candidates = Vec::new();
    let mut rationales = BTreeMap::new();
    let mut hook_types = BTreeMap::new();
    for item in items.iter().take(12) {
        if let Some(title) = item.as_str().map(sanitize_generated_contract_scalar) {
            if !value_missing(&title) && !candidates.iter().any(|known| known == &title) {
                candidates.push(title);
            }
            continue;
        }
        let Some(candidate) = item.as_object() else {
            continue;
        };
        let title = string_aliases(
            candidate,
            &[
                "title",
                "canonical_title",
                "book_title",
                "书名",
                "标题",
                "作品名",
            ],
        );
        if value_missing(&title) {
            continue;
        }
        if !candidates.iter().any(|known| known == &title) {
            candidates.push(title.clone());
        }
        let rationale = string_aliases(
            candidate,
            &[
                "rationale",
                "ration",
                "reason",
                "basis",
                "书名理由",
                "命名理由",
                "标题理由",
            ],
        );
        if !value_missing(&rationale) {
            rationales.insert(title.clone(), rationale);
        }
        let hook_type = string_aliases(
            candidate,
            &["hook_type", "hookType", "type", "钩子类型", "命名类型"],
        );
        if !value_missing(&hook_type) {
            hook_types.insert(title, hook_type);
        }
    }
    (candidates, rationales, hook_types)
}

fn skeleton_patch_from_field_pack(raw: &str) -> Option<SkeletonPatch> {
    let patch = SkeletonPatch {
        genre: field_string(raw, &["题材", "类型", "genre"]).unwrap_or_default(),
        brief: field_string(raw, &["简述", "创作简述", "brief"]).unwrap_or_default(),
        target_units: field_usize(raw, &["总字数", "目标字数", "target_units"]),
        chapter_unit_target: longform_policy::normalize_user_chapter_unit_target(field_usize(
            raw,
            &["每章字数", "每章档位", "chapter_unit_target"],
        )),
        max_chapters_per_turn: field_usize(raw, &["每轮最多章节", "max_chapters_per_turn"]),
        premise: field_string(raw, &["故事前提", "前提", "premise"]).unwrap_or_default(),
        ending_desired_resolution: field_string(raw, &["终局方向", "结局方向", "ending_direction"])
            .unwrap_or_default(),
        ending_final_state: field_string(raw, &["终局状态", "final_state"]).unwrap_or_default(),
        protagonist_arc: field_string(raw, &["主角弧线", "成长线", "protagonist_arc"])
            .unwrap_or_default(),
        world_imagery: field_string(
            raw,
            &[
                "世界观意象",
                "世界观核心意象",
                "世界意象",
                "核心意象",
                "world_imagery",
                "world_imaginery",
            ],
        )
        .unwrap_or_default(),
        main_causal_spine: field_string(raw, &["总主线因果链", "主线因果链", "main_causal_spine"])
            .unwrap_or_default(),
    };
    (!skeleton_patch_empty(&patch)).then_some(patch)
}

fn character_patch_from_field_pack(raw: &str) -> Option<CharacterPatch> {
    if !raw_character_field_pack_has_explicit_contract_fields(raw) {
        return None;
    }
    let characters = generated_fiction_character_lines(raw)
        .iter()
        .filter(|line| character_field_pack_line_is_explicit_authority_entry(line))
        .map(|line| super::draft_character_line_to_contract(line))
        .filter(|character| character_contract_has_patchable_anchor(character))
        .collect::<Vec<_>>();
    (!characters.is_empty()).then_some(CharacterPatch {
        relationship_ledger: relationship_ledger_from_field_pack(raw, &characters),
        emotional_state_ledger: emotional_state_ledger_from_field_pack(raw, &characters),
        characters,
    })
}

fn raw_character_field_pack_has_explicit_contract_fields(raw: &str) -> bool {
    let lowered = raw.to_ascii_lowercase();
    let has_name = [
        "姓名：",
        "姓名:",
        "名字：",
        "名字:",
        "canonical_name",
        "canonicalname",
        "name:",
        "name：",
        "主角姓名",
        "主角名",
        "对手姓名",
        "反派姓名",
    ]
    .iter()
    .any(|marker| raw.contains(marker) || lowered.contains(&marker.to_ascii_lowercase()));
    let has_role = [
        "角色：",
        "角色:",
        "身份：",
        "身份:",
        "定位：",
        "定位:",
        "role:",
        "role：",
        "主角",
        "对手",
        "反派",
        "导师",
        "同伴",
        "关键关系对象",
    ]
    .iter()
    .any(|marker| raw.contains(marker) || lowered.contains(&marker.to_ascii_lowercase()));
    let has_anchor = [
        "欲望：",
        "欲望:",
        "恐惧：",
        "恐惧:",
        "底线：",
        "底线:",
        "弧线起点",
        "弧线终点",
        "desire:",
        "fear:",
        "bottom_line",
        "bottomline",
        "arc_start",
        "arc_end",
        "arcstart",
        "arcend",
    ]
    .iter()
    .any(|marker| raw.contains(marker) || lowered.contains(&marker.to_ascii_lowercase()));
    has_name && has_role && has_anchor
}

fn relationship_ledger_from_field_pack(
    raw: &str,
    characters: &[CharacterContract],
) -> Vec<RelationshipLedgerEntry> {
    let Some(lines) = field_list(
        raw,
        &[
            "relationship_ledger",
            "relationshipLedger",
            "关系线",
            "关系账本",
            "人物关系",
            "关键人物关系",
        ],
    ) else {
        return Vec::new();
    };
    lines
        .into_iter()
        .filter_map(|line| {
            let names = relationship_names_from_line(&line, characters);
            (names.len() >= 2).then(|| RelationshipLedgerEntry {
                characters: names,
                relationship_type: line.clone(),
                start_state: line.clone(),
                current_state: line.clone(),
                desired_end_state: line,
                ..Default::default()
            })
        })
        .take(8)
        .collect()
}

fn emotional_state_ledger_from_field_pack(
    raw: &str,
    characters: &[CharacterContract],
) -> Vec<EmotionalStateLedgerEntry> {
    let Some(lines) = field_list(
        raw,
        &[
            "emotional_state_ledger",
            "emotionalStateLedger",
            "情绪状态",
            "情绪账本",
            "情感线",
            "情绪推进",
        ],
    ) else {
        return Vec::new();
    };
    let primary = characters
        .iter()
        .find(|character| character.role_looks_primary());
    lines
        .into_iter()
        .filter_map(|line| {
            let character = characters
                .iter()
                .find(|character| {
                    let name = character.canonical_name.trim();
                    !value_missing(name) && line.contains(name)
                })
                .or(primary)?;
            Some(EmotionalStateLedgerEntry {
                character: character.canonical_name.clone(),
                current_emotion: line.clone(),
                pressure: line.clone(),
                desire: character.desire.clone(),
                fear: character.fear.clone(),
                expected_next_shift: line.clone(),
                payoff_target: character.arc_end.clone(),
                ..Default::default()
            })
        })
        .take(8)
        .collect()
}

fn relationship_names_from_line(line: &str, characters: &[CharacterContract]) -> Vec<String> {
    let mut names = characters
        .iter()
        .filter_map(|character| {
            let name = character.canonical_name.trim();
            (!value_missing(name) && line.contains(name)).then(|| name.to_string())
        })
        .collect::<Vec<_>>();
    if names.len() >= 2 {
        return names;
    }
    let Some(primary) = characters
        .iter()
        .find(|character| character.role_looks_primary())
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
    else {
        return names;
    };
    if !names.iter().any(|name| name == primary) {
        names.insert(0, primary.to_string());
    }
    if names.len() < 2 {
        if let Some(other) = characters
            .iter()
            .find(|character| {
                !character.role_looks_primary() && !value_missing(&character.canonical_name)
            })
            .map(|character| character.canonical_name.trim())
        {
            if !names.iter().any(|name| name == other) {
                names.push(other.to_string());
            }
        }
    }
    names
}

fn plot_patch_from_field_pack(raw: &str) -> Option<PlotPatch> {
    let raw_outline = field_string(
        raw,
        &[
            "全书大纲",
            "大纲",
            "故事大纲",
            "阶段规划",
            "分卷/阶段安排",
            "近期章节包",
            "章节规划",
            "outline",
        ],
    )
    .unwrap_or_default();
    let derived_outline = derive_plot_contract_from_outline_text(raw);
    let mut near_chapters = field_list(raw, &["近期章节", "章节目标", "章节规划", "near_chapters"])
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, item)| ChapterSeedContract {
            number: Some(index + 1),
            goal: item.clone(),
            expected_turn: item,
        })
        .take(12)
        .collect::<Vec<_>>();
    if near_chapters.is_empty() {
        near_chapters = derived_outline.near_chapters.clone();
    }
    let mut volumes = field_list(raw, &["分卷", "卷宗", "卷规划", "volumes"])
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, item)| VolumeContract {
            title: format!("第{}卷", index + 1),
            objective: item.clone(),
            ending_change: item,
        })
        .take(8)
        .collect::<Vec<_>>();
    if volumes.is_empty() {
        volumes = derived_outline.volumes.clone();
    }
    let mut payoff_matrix = field_list(
        raw,
        &[
            "伏笔/承诺兑现矩阵",
            "伏笔矩阵",
            "承诺兑现矩阵",
            "伏笔",
            "payoff_matrix",
        ],
    )
    .unwrap_or_default()
    .into_iter()
    .map(|item| PayoffMatrixEntry {
        promise: item.clone(),
        payoff_target: item,
        status: "planned".to_string(),
        ..Default::default()
    })
    .take(12)
    .collect::<Vec<_>>();
    if payoff_matrix.is_empty() {
        payoff_matrix = derived_outline.payoff_matrix.clone();
    }
    deduplicate_volumes(&mut volumes);
    normalize_near_chapter_window(&mut near_chapters);
    let patch = PlotPatch {
        volumes,
        near_chapters,
        raw_outline,
        payoff_matrix,
    };
    (!patch.volumes.is_empty()
        || !patch.near_chapters.is_empty()
        || !value_missing(&patch.raw_outline)
        || !patch.payoff_matrix.is_empty())
    .then_some(patch)
}

pub(crate) fn derive_plot_contract_from_outline_text(raw_outline: &str) -> PlotPatch {
    let mut patch = PlotPatch::default();
    if value_missing(raw_outline) {
        return patch;
    }
    for segment in split_chinese_plan_segments(raw_outline) {
        match leading_plan_marker(&segment) {
            Some('卷') => {
                if let Some(volume) = volume_contract_from_plan_segment(&segment) {
                    patch.volumes.push(volume);
                }
            }
            Some('章') => {
                if let Some(chapter) = chapter_seed_from_plan_segment(&segment) {
                    patch.near_chapters.push(chapter);
                }
            }
            _ => {}
        }
    }
    if patch.payoff_matrix.is_empty() {
        if let Some(last_volume) = patch.volumes.last() {
            if !value_missing(&last_volume.ending_change) {
                patch.payoff_matrix.push(PayoffMatrixEntry {
                    promise: last_volume.objective.clone(),
                    payoff_target: last_volume.ending_change.clone(),
                    status: "planned".to_string(),
                    ..Default::default()
                });
            }
        }
    }
    deduplicate_volumes(&mut patch.volumes);
    normalize_near_chapter_window(&mut patch.near_chapters);
    patch.payoff_matrix.truncate(16);
    patch
}

fn normalize_near_chapter_window(near_chapters: &mut Vec<ChapterSeedContract>) {
    near_chapters.truncate(MAX_NEAR_CHAPTERS);
}

fn deduplicate_volumes(volumes: &mut Vec<VolumeContract>) {
    let mut seen = Vec::<(String, String, String)>::new();
    volumes.retain(|volume| {
        let identity = (
            volume.title.trim().to_string(),
            volume.objective.trim().to_string(),
            volume.ending_change.trim().to_string(),
        );
        if seen.contains(&identity) {
            false
        } else {
            seen.push(identity);
            true
        }
    });
}

pub(crate) fn strip_plot_control_segments_from_outline_text(raw_outline: &str) -> String {
    if value_missing(raw_outline) {
        return String::new();
    }
    let Some(first_start) = chinese_plan_segment_starts(raw_outline).first().copied() else {
        return raw_outline.trim().to_string();
    };
    raw_outline[..first_start]
        .trim()
        .trim_end_matches(['；', ';', '。', '.', '，', ','])
        .trim()
        .to_string()
}

fn split_chinese_plan_segments(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let normalized = text.replace('\n', "；");
    let starts = chinese_plan_segment_starts(&normalized);
    for (position, start) in starts.iter().enumerate() {
        let end = starts
            .get(position + 1)
            .copied()
            .unwrap_or(normalized.len());
        let segment = normalized[*start..end].trim();
        if !segment.is_empty() {
            segments.push(segment.to_string());
        }
    }
    segments
}

fn chinese_plan_segment_starts(text: &str) -> Vec<usize> {
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut starts = Vec::new();
    for (index, (byte_index, ch)) in chars.iter().enumerate() {
        if *ch != '第' {
            continue;
        }
        let mut marker = None;
        let mut marker_index = None;
        for (next_index, (_, next)) in chars.iter().enumerate().skip(index + 1).take(8) {
            if *next == '卷' || *next == '章' {
                marker = Some(*next);
                marker_index = Some(next_index);
                break;
            }
            if !plan_number_char(*next) {
                break;
            }
        }
        let follows_explicit_plan_header = marker
            .zip(marker_index)
            .and_then(|(marker, marker_index)| {
                chars
                    .get(marker_index + 1)
                    .map(|(suffix_start, next)| (marker, *suffix_start, *next))
            })
            .is_some_and(|(marker, suffix_start, next)| {
                if matches!(
                    next,
                    '《' | '〈' | '“' | '"' | '：' | ':' | '-' | '—' | '【' | '['
                ) {
                    return true;
                }
                if !next.is_whitespace() {
                    return false;
                }
                let suffix = text[suffix_start..].trim_start();
                match marker {
                    '卷' => ["本卷目标", "卷名：", "卷名:", "阶段目标：", "阶段目标:"]
                        .iter()
                        .any(|prefix| suffix.starts_with(prefix)),
                    '章' => ["本章目标", "章节目标", "章目标", "目标：", "目标:"]
                        .iter()
                        .any(|prefix| suffix.starts_with(prefix)),
                    _ => false,
                }
            });
        if follows_explicit_plan_header {
            starts.push(*byte_index);
        }
    }
    starts
}

fn plan_number_char(ch: char) -> bool {
    ch.is_ascii_digit()
        || matches!(
            ch,
            '一' | '二'
                | '三'
                | '四'
                | '五'
                | '六'
                | '七'
                | '八'
                | '九'
                | '十'
                | '百'
                | '零'
                | '〇'
        )
}

fn leading_plan_marker(text: &str) -> Option<char> {
    let chars = text.trim_start().chars().collect::<Vec<_>>();
    if chars.first().copied()? != '第' {
        return None;
    }
    for ch in chars.into_iter().skip(1).take(8) {
        if ch == '卷' || ch == '章' {
            return Some(ch);
        }
        if !plan_number_char(ch) {
            return None;
        }
    }
    None
}

fn volume_contract_from_plan_segment(segment: &str) -> Option<VolumeContract> {
    let text = sanitize_generated_contract_scalar(segment);
    if value_missing(&text) {
        return None;
    }
    let raw_title = quoted_text(&text)
        .or_else(|| unquoted_volume_title_from_plan_segment(&text))
        .unwrap_or_else(|| {
            text.split(['：', ':', '；', ';'])
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        });
    let title = clean_volume_contract_title(&raw_title);
    let objective = volume_objective_from_plan_segment(&text, &raw_title, &title);
    let ending_change = text_after_any(
        &text,
        &["卷尾变化：", "卷尾变化:", "不可逆变化：", "不可逆变化:"],
    )
    .unwrap_or_else(|| objective.clone());
    if volume_title_looks_like_chapter_fragment(&title) {
        return None;
    }
    let mut volume = VolumeContract {
        title,
        objective,
        ending_change,
    };
    normalize_volume_contract_surface(&mut volume);
    Some(volume)
}

fn volume_objective_from_plan_segment(text: &str, raw_title: &str, title: &str) -> String {
    let after_title = quoted_text(text)
        .and_then(|quoted| {
            text.find(&format!("《{quoted}》"))
                .map(|index| &text[index + quoted.len() + "《》".len()..])
        })
        .map(|tail| tail.trim_start_matches(['：', ':', '-', ' ', '；', ';']))
        .filter(|tail| !tail.is_empty())
        .map(str::to_string);
    let objective = after_title
        .or_else(|| {
            text_between_after_any(
                text,
                &["：", ":"],
                &["；卷尾变化", ";卷尾变化", "；不可逆变化", ";不可逆变化"],
            )
        })
        .unwrap_or_else(|| text.to_string());
    clean_volume_contract_objective(&objective, raw_title, title)
}

fn clean_volume_contract_title(value: &str) -> String {
    let mut title = sanitize_generated_contract_scalar(value)
        .trim_matches(['《', '》', '"', '“', '”'])
        .trim()
        .to_string();
    if let Some(stripped) = strip_volume_ordinal_prefix(&title) {
        title = stripped;
    }
    title
        .trim_matches(['《', '》', '"', '“', '”'])
        .trim()
        .to_string()
}

fn strip_volume_ordinal_prefix(value: &str) -> Option<String> {
    let text = value.trim();
    if let Some(rest) = text.strip_prefix('第') {
        let mut chars = rest.char_indices();
        let mut volume_byte = None;
        for (index, ch) in &mut chars {
            if ch == '卷' {
                volume_byte = Some(index);
                break;
            }
            if !plan_number_char(ch) {
                return None;
            }
        }
        let volume_byte = volume_byte?;
        let tail = rest[volume_byte + '卷'.len_utf8()..]
            .trim_start_matches(['：', ':', '-', ' '])
            .trim();
        return (!value_missing(tail)).then(|| tail.to_string());
    }
    if let Some(rest) = text.strip_prefix('卷') {
        let mut chars = rest.char_indices();
        let mut sep_byte = None;
        for (index, ch) in &mut chars {
            if matches!(ch, '：' | ':' | '-' | ' ') {
                sep_byte = Some(index);
                break;
            }
            if !plan_number_char(ch) {
                return None;
            }
        }
        let sep_byte = sep_byte?;
        let tail = rest[sep_byte..]
            .trim_start_matches(['：', ':', '-', ' '])
            .trim();
        return (!value_missing(tail)).then(|| tail.to_string());
    }
    None
}

fn clean_volume_contract_objective(value: &str, raw_title: &str, title: &str) -> String {
    let mut objective =
        strip_contract_section_heading_residue(&sanitize_generated_contract_scalar(value));
    for marker in [
        "；卷尾变化",
        ";卷尾变化",
        "；卷尾转折",
        ";卷尾转折",
        "；不可逆变化",
        ";不可逆变化",
        "；预期转折",
        ";预期转折",
        "；本章目标",
        ";本章目标",
        "；章节目标",
        ";章节目标",
    ] {
        if let Some(index) = objective.find(marker) {
            objective.truncate(index);
        }
    }
    let mut objective = objective
        .trim_matches(['《', '》', '"', '“', '”'])
        .trim()
        .to_string();
    for _ in 0..4 {
        let before = objective.clone();
        objective = objective
            .trim_start_matches(['：', ':', '-', ' ', '；', ';', '》'])
            .trim()
            .to_string();
        for prefix in [raw_title, title] {
            let prefix = prefix.trim();
            if prefix.is_empty() {
                continue;
            }
            if let Some(tail) = objective.strip_prefix(prefix) {
                objective = tail
                    .trim_start_matches(['：', ':', '-', ' ', '；', ';', '》'])
                    .trim()
                    .to_string();
            }
        }
        if objective == before {
            break;
        }
    }
    objective = strip_leading_contract_field_labels(
        &objective,
        &[
            "本卷目标：",
            "本卷目标:",
            "阶段目标：",
            "阶段目标:",
            "目标：",
            "目标:",
            "卷尾变化：",
            "卷尾变化:",
            "卷尾转折：",
            "卷尾转折:",
            "不可逆变化：",
            "不可逆变化:",
        ],
    );
    objective
}

pub(crate) fn normalize_volume_contract_surface(volume: &mut VolumeContract) {
    let raw_title = volume.title.clone();
    let title = clean_volume_contract_title(&raw_title);
    volume.objective = clean_volume_contract_objective(&volume.objective, &raw_title, &title);
    volume.ending_change =
        clean_volume_contract_objective(&volume.ending_change, &raw_title, &title);
    volume.title = title;
}

fn unquoted_volume_title_from_plan_segment(text: &str) -> Option<String> {
    let marker = leading_plan_marker(text)?;
    if marker != '卷' {
        return None;
    }
    let (_, tail) = text.split_once('卷')?;
    let title = tail
        .trim_start_matches(['：', ':', '-', ' '])
        .split(['；', ';', '。', '\n'])
        .next()
        .unwrap_or_default()
        .trim();
    (!value_missing(title) && !title.contains('：') && !title.contains(':'))
        .then(|| title.to_string())
}

fn volume_title_looks_like_chapter_fragment(title: &str) -> bool {
    let compact = title.trim().replace(char::is_whitespace, "");
    compact.contains('章')
        || compact.contains("本章目标")
        || compact == "卷尾变化"
        || compact == "卷尾转折"
        || compact == "不可逆变化"
        || compact == "预期转折"
        || compact.matches('《').count() != compact.matches('》').count()
}

fn chapter_seed_from_plan_segment(segment: &str) -> Option<ChapterSeedContract> {
    let text = sanitize_generated_contract_scalar(segment);
    if value_missing(&text) {
        return None;
    }
    let number = leading_plan_number(&text, '章');
    let goal = text_between_after_any(
        &text,
        &["本章目标：", "本章目标:", "目标：", "目标:"],
        &["；预期转折", ";预期转折", "；不可逆变化", ";不可逆变化"],
    )
    .or_else(|| {
        text.split(['：', ':'])
            .nth(1)
            .map(sanitize_generated_contract_scalar)
    })
    .unwrap_or_else(|| text.clone());
    let expected_turn = text_after_any(
        &text,
        &["预期转折：", "预期转折:", "不可逆变化：", "不可逆变化:"],
    )
    .unwrap_or_else(|| goal.clone());
    Some(ChapterSeedContract {
        number,
        goal,
        expected_turn,
    })
}

fn quoted_text(text: &str) -> Option<String> {
    let start = text.find('《')?;
    let end = text[start + '《'.len_utf8()..].find('》')? + start + '《'.len_utf8();
    let value = sanitize_generated_contract_scalar(&text[start + '《'.len_utf8()..end]);
    (!value_missing(&value)).then_some(value)
}

fn leading_plan_number(text: &str, marker: char) -> Option<usize> {
    let after_di = text.strip_prefix('第')?;
    let number_text = after_di.split(marker).next()?.trim();
    number_text.parse::<usize>().ok()
}

fn text_after_any(text: &str, starts: &[&str]) -> Option<String> {
    starts.iter().find_map(|start| {
        let index = text.find(start)? + start.len();
        let value = sanitize_generated_contract_scalar(&text[index..]);
        (!value_missing(&value)).then_some(value)
    })
}

fn text_between_after_any(text: &str, starts: &[&str], ends: &[&str]) -> Option<String> {
    starts.iter().find_map(|start| {
        let start_index = text.find(start)? + start.len();
        let tail = &text[start_index..];
        let end_index = ends
            .iter()
            .filter_map(|end| tail.find(end))
            .min()
            .unwrap_or(tail.len());
        let value = sanitize_generated_contract_scalar(&tail[..end_index]);
        (!value_missing(&value)).then_some(value)
    })
}

fn governance_patch_from_field_pack(raw: &str) -> Option<GovernancePatch> {
    let structured = NovelContractV2 {
        scene_type_mix: SceneTypeMix {
            action: field_string(raw, &["动作戏比例", "动作", "action"]).unwrap_or_default(),
            dialogue: field_string(raw, &["对话戏比例", "对话", "dialogue"]).unwrap_or_default(),
            everyday: field_string(raw, &["日常戏比例", "日常", "everyday"]).unwrap_or_default(),
            reveal: field_string(raw, &["信息揭示比例", "揭示", "reveal"]).unwrap_or_default(),
            emotional: field_string(raw, &["情感戏比例", "情感戏", "emotional"])
                .unwrap_or_default(),
            turning_point: field_string(raw, &["转折戏比例", "转折", "turning_point"])
                .unwrap_or_default(),
            balance_rule: field_string(raw, &["场景类型配比", "场景配比", "balance_rule"])
                .unwrap_or_default(),
        },
        character_voice_ledger: character_voice_ledger_from_field_pack(raw),
        reader_promise: ReaderPromise {
            core_hook: field_string(raw, &["读者期待", "爽点合同", "核心爽点", "core_hook"])
                .unwrap_or_default(),
            pleasure_points: field_list(raw, &["爽点", "pleasure_points"]).unwrap_or_default(),
            curiosity_engine: field_string(raw, &["好奇引擎", "悬念引擎", "curiosity_engine"])
                .unwrap_or_default(),
            payoff_style: field_string(raw, &["兑现方式", "payoff_style"]).unwrap_or_default(),
        },
        chapter_ending_rotation: ChapterEndingRotation {
            planned_rotation: field_list(raw, &["章节收尾轮换", "章尾轮换", "planned_rotation"])
                .unwrap_or_default(),
            avoid_repetition_rule: field_string(
                raw,
                &["章尾避免重复", "收尾避免重复", "avoid_repetition_rule"],
            )
            .unwrap_or_default(),
        },
        conflict_pressure_curve: ConflictPressureCurve {
            global_curve: pressure_curve_from_field_pack(raw),
            release_strategy: field_string(raw, &["降压策略", "缓冲策略", "release_strategy"])
                .unwrap_or_default(),
            peak_policy: field_string(raw, &["爆发策略", "峰值策略", "peak_policy"])
                .unwrap_or_default(),
        },
        motif_ledger: motif_ledger_from_field_pack(raw),
        reveal_schedule: reveal_schedule_from_field_pack(raw),
        relationship_interaction_quotas: relationship_interaction_quotas_from_field_pack(raw),
        ..Default::default()
    };
    let patch = GovernancePatch {
        themes: field_list(raw, &["核心主题", "主题", "themes"]).unwrap_or_default(),
        world_rules: field_list(raw, &["世界规则", "规则", "world_rules"])
            .or_else(|| numbered_world_rule_lines(raw))
            .unwrap_or_default(),
        style_rules: field_list(raw, &["叙事风格", "文风", "style_rules"]).unwrap_or_default(),
        must_avoid: field_list(raw, &["必须避免", "禁忌", "must_avoid"]).unwrap_or_default(),
        emotional_contract: EmotionalContract {
            primary_emotion: field_string(raw, &["主情绪", "核心情绪", "primary_emotion"])
                .unwrap_or_default(),
            emotional_promise: field_string(raw, &["情感承诺", "emotional_promise"])
                .unwrap_or_default(),
            emotional_beats: field_list(raw, &["情感节拍", "情感线", "emotional_beats"])
                .unwrap_or_default(),
            relief_beats: field_list(
                raw,
                &[
                    "节奏缓冲",
                    "轻松缓冲",
                    "幽默缓冲",
                    "呼吸点",
                    "relief_beats",
                    "reliefBeats",
                ],
            )
            .unwrap_or_default(),
            payoff_requirements: field_list(raw, &["情感兑现", "payoff_requirements"])
                .unwrap_or_default(),
            ending_emotional_state: field_string(
                raw,
                &["终局情绪落点", "结局情绪", "ending_emotional_state"],
            )
            .unwrap_or_default(),
        },
        relationship_ledger: Vec::new(),
        antagonist_pressure: AntagonistPressure {
            primary_pressure: field_string(raw, &["对手压力", "主要压力", "反派压力"])
                .unwrap_or_default(),
            antagonists: Vec::new(),
        },
        narration_contract: NarrationContract {
            pov: field_string(raw, &["视角", "叙事视角", "pov"]).unwrap_or_default(),
            dialogue_style: field_string(raw, &["对白风格", "dialogue_style"]).unwrap_or_default(),
            description_density: field_string(raw, &["描写密度", "description_density"])
                .unwrap_or_default(),
            chapter_pacing: field_string(raw, &["章节节奏", "chapter_pacing"]).unwrap_or_default(),
            ..Default::default()
        },
        structured,
    };
    (!patch.themes.is_empty()
        || !patch.world_rules.is_empty()
        || !patch.style_rules.is_empty()
        || !patch.must_avoid.is_empty()
        || !value_missing(&patch.emotional_contract.primary_emotion)
        || !value_missing(&patch.emotional_contract.emotional_promise)
        || !patch.emotional_contract.emotional_beats.is_empty()
        || !patch.emotional_contract.relief_beats.is_empty()
        || !value_missing(&patch.emotional_contract.ending_emotional_state)
        || !value_missing(&patch.antagonist_pressure.primary_pressure)
        || !value_missing(&patch.narration_contract.pov)
        || governance_structured_has_content(&patch.structured))
    .then_some(patch)
}

fn numbered_world_rule_lines(raw: &str) -> Option<Vec<String>> {
    let values = raw
        .lines()
        .filter_map(numbered_world_rule_line)
        .take(12)
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values)
}

fn numbered_world_rule_line(line: &str) -> Option<String> {
    let trimmed = line
        .trim()
        .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'));
    let tail = trimmed
        .strip_prefix("规则")
        .and_then(|value| {
            let digit_len = value
                .chars()
                .take_while(|ch| {
                    ch.is_ascii_digit() || matches!(ch, '一' | '二' | '三' | '四' | '五')
                })
                .map(char::len_utf8)
                .sum::<usize>();
            (digit_len > 0).then_some(&value[digit_len..])
        })
        .or_else(|| {
            let digit_len = trimmed
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .map(char::len_utf8)
                .sum::<usize>();
            (digit_len > 0).then_some(&trimmed[digit_len..])
        })?
        .trim_start_matches(|ch| matches!(ch, ':' | '：' | '.' | '、' | ')' | '）' | ' ' | '\t'))
        .trim();
    let value = sanitize_generated_contract_scalar(tail);
    (!value_missing(&value)
        && !crate::tool::writing::typed_contract_gate::world_rule_looks_truncated_or_not_actionable(
            &value,
        ))
    .then_some(value)
}

fn governance_structured_has_content(value: &NovelContractV2) -> bool {
    !scene_type_mix_empty(&value.scene_type_mix)
        || !value.character_voice_ledger.is_empty()
        || !reader_promise_empty(&value.reader_promise)
        || !value.chapter_ending_rotation.planned_rotation.is_empty()
        || !value
            .chapter_ending_rotation
            .avoid_repetition_rule
            .trim()
            .is_empty()
        || !value.conflict_pressure_curve.global_curve.is_empty()
        || !value
            .conflict_pressure_curve
            .release_strategy
            .trim()
            .is_empty()
        || !value.conflict_pressure_curve.peak_policy.trim().is_empty()
        || !value.motif_ledger.is_empty()
        || !value.reveal_schedule.is_empty()
        || !value.relationship_interaction_quotas.is_empty()
}

fn scene_type_mix_empty(value: &SceneTypeMix) -> bool {
    value.action.trim().is_empty()
        && value.dialogue.trim().is_empty()
        && value.everyday.trim().is_empty()
        && value.reveal.trim().is_empty()
        && value.emotional.trim().is_empty()
        && value.turning_point.trim().is_empty()
        && value.balance_rule.trim().is_empty()
}

fn reader_promise_empty(value: &ReaderPromise) -> bool {
    value.core_hook.trim().is_empty()
        && value.pleasure_points.is_empty()
        && value.curiosity_engine.trim().is_empty()
        && value.payoff_style.trim().is_empty()
}

fn character_voice_ledger_from_field_pack(raw: &str) -> Vec<CharacterVoiceProfile> {
    let Some(lines) = field_list(
        raw,
        &[
            "character_voice_ledger",
            "characterVoiceLedger",
            "角色声音表",
            "角色对白表",
            "角色口吻",
        ],
    ) else {
        return Vec::new();
    };
    lines
        .into_iter()
        .map(|line| CharacterVoiceProfile {
            character: field_fragment_after_any(&line, &["角色：", "角色:", "character:"])
                .unwrap_or_default(),
            voice_style: line,
            ..Default::default()
        })
        .take(12)
        .collect()
}

fn pressure_curve_from_field_pack(raw: &str) -> Vec<PressureBeat> {
    let Some(lines) = field_list(
        raw,
        &[
            "conflict_pressure_curve",
            "conflictPressureCurve",
            "冲突升降压曲线",
            "冲突曲线",
            "压力曲线",
        ],
    ) else {
        return Vec::new();
    };
    lines
        .into_iter()
        .map(|line| PressureBeat {
            range: field_fragment_after_any(&line, &["范围：", "范围:", "range:"])
                .unwrap_or_default(),
            pressure_level: field_fragment_after_any(&line, &["压力：", "压力:", "pressure:"])
                .unwrap_or_else(|| line.clone()),
            function: field_fragment_after_any(&line, &["作用：", "作用:", "function:"])
                .unwrap_or(line),
        })
        .take(12)
        .collect()
}

fn motif_ledger_from_field_pack(raw: &str) -> Vec<MotifLedgerEntry> {
    let Some(lines) = field_list(
        raw,
        &[
            "motif_ledger",
            "motifLedger",
            "主题母题",
            "母题账本",
            "场景意象",
        ],
    ) else {
        return Vec::new();
    };
    lines
        .into_iter()
        .map(|line| MotifLedgerEntry {
            motif: field_fragment_after_any(&line, &["母题：", "母题:", "motif:"])
                .unwrap_or_else(|| line.clone()),
            meaning: field_fragment_after_any(&line, &["含义：", "含义:", "meaning:"])
                .unwrap_or_else(|| line.clone()),
            evolution: field_fragment_after_any(&line, &["变化：", "变化:", "evolution:"])
                .map(|value| vec![value])
                .unwrap_or_default(),
            payoff_target: field_fragment_after_any(&line, &["兑现：", "兑现:", "payoff:"])
                .unwrap_or_default(),
        })
        .take(12)
        .collect()
}

fn reveal_schedule_from_field_pack(raw: &str) -> Vec<RevealScheduleEntry> {
    let Some(lines) = field_list(
        raw,
        &[
            "reveal_schedule",
            "revealSchedule",
            "信息揭示节奏",
            "揭示时间表",
            "秘密揭示",
        ],
    ) else {
        return Vec::new();
    };
    lines
        .into_iter()
        .map(|line| RevealScheduleEntry {
            secret: field_fragment_after_any(&line, &["秘密：", "秘密:", "secret:"])
                .unwrap_or_else(|| line.clone()),
            reader_knows: field_fragment_after_any(&line, &["读者知道：", "读者知道:", "reader:"])
                .unwrap_or_default(),
            protagonist_knows: field_fragment_after_any(
                &line,
                &["主角知道：", "主角知道:", "protagonist:"],
            )
            .unwrap_or_default(),
            antagonist_knows: field_fragment_after_any(
                &line,
                &["对手知道：", "反派知道：", "antagonist:"],
            )
            .unwrap_or_default(),
            reveal_window: field_fragment_after_any(&line, &["窗口：", "窗口:", "window:"])
                .unwrap_or_default(),
            status: field_fragment_after_any(&line, &["状态：", "状态:", "status:"])
                .unwrap_or_else(|| "planned".to_string()),
        })
        .take(12)
        .collect()
}

fn relationship_interaction_quotas_from_field_pack(raw: &str) -> Vec<RelationshipInteractionQuota> {
    let Some(lines) = field_list(
        raw,
        &[
            "relationship_interaction_quotas",
            "relationshipInteractionQuotas",
            "角色关系互动配额",
            "关系互动配额",
            "互动配额",
        ],
    ) else {
        return Vec::new();
    };
    lines
        .into_iter()
        .map(|line| RelationshipInteractionQuota {
            relationship: field_fragment_after_any(&line, &["关系：", "关系:", "relationship:"])
                .unwrap_or_else(|| line.clone()),
            characters: field_fragment_after_any(&line, &["角色：", "角色:", "characters:"])
                .map(|value| {
                    value
                        .split(['、', ',', '，', '/', '和'])
                        .map(str::trim)
                        .filter(|value| !value_missing(value))
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            cadence: field_fragment_after_any(&line, &["频率：", "节奏：", "cadence:"])
                .unwrap_or_else(|| line.clone()),
            next_due: field_fragment_after_any(&line, &["下次：", "下次推进:", "next_due:"])
                .unwrap_or_default(),
            required_interaction: field_fragment_after_any(
                &line,
                &["互动：", "必须互动:", "required:"],
            )
            .unwrap_or(line),
        })
        .take(12)
        .collect()
}

fn field_fragment_after_any(text: &str, markers: &[&str]) -> Option<String> {
    markers.iter().find_map(|marker| {
        let index = text.find(marker)? + marker.len();
        let value = text[index..]
            .split(['；', ';', '，', ',', '\n'])
            .next()
            .unwrap_or_default()
            .trim();
        (!value_missing(value)).then(|| value.to_string())
    })
}

fn character_from_value(value: &Value) -> Option<CharacterContract> {
    let object = value.as_object()?;
    let character = CharacterContract {
        canonical_name: sanitize_character_name(&string_aliases(
            object,
            &[
                "canonical_name",
                "canonicalName",
                "name",
                "character_name",
                "characterName",
                "姓名",
                "名字",
                "角色名",
                "人物名",
            ],
        )),
        aliases: list_aliases(object, &["aliases", "别名"]),
        role: string_aliases(object, &["role", "角色", "身份", "定位", "职责"]),
        desire: string_aliases(object, &["desire", "欲望"]),
        fear: string_aliases(object, &["fear", "恐惧"]),
        bottom_line: string_aliases(object, &["bottom_line", "bottom line", "底线"]),
        arc_start: string_aliases(object, &["arc_start", "弧线起点", "起点"]),
        arc_end: string_aliases(object, &["arc_end", "弧线终点", "终点"]),
        planned_entry: string_aliases(object, &["planned_entry", "计划登场"]),
        planned_exit: string_aliases(object, &["planned_exit", "计划离场"]),
        ..Default::default()
    };
    (!value_missing(&character.canonical_name)).then_some(character)
}

fn characters_from_value(value: &Value) -> Vec<CharacterContract> {
    let mut characters = match value {
        Value::Array(items) => items
            .iter()
            .flat_map(|item| {
                character_from_value(item).into_iter().chain(
                    item.as_str()
                        .into_iter()
                        .map(draft_character_line_to_contract),
                )
            })
            .filter(|character| !value_missing(&character.canonical_name))
            .collect(),
        Value::String(text) => generated_fiction_character_lines(text)
            .iter()
            .map(|line| draft_character_line_to_contract(line))
            .filter(|character| !value_missing(&character.canonical_name))
            .collect(),
        _ => Vec::new(),
    };
    normalize_character_patch_candidates(&mut characters);
    characters
}

fn relationship_ledger_from_value(value: &Value) -> Vec<RelationshipLedgerEntry> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(relationship_entry_from_value)
            .filter(|entry| {
                entry
                    .characters
                    .iter()
                    .filter(|name| !value_missing(name))
                    .count()
                    >= 2
                    || !value_missing(&entry.relationship_type)
                    || !value_missing(&entry.desired_end_state)
            })
            .take(16)
            .collect(),
        _ => Vec::new(),
    }
}

fn relationship_entry_from_value(value: &Value) -> Option<RelationshipLedgerEntry> {
    let object = value.as_object()?;
    Some(RelationshipLedgerEntry {
        character_ids: Vec::new(),
        characters: list_aliases(object, &["characters", "角色", "人物", "相关角色"]),
        arc_type: string_aliases(object, &["arc_type", "arcType", "arctype", "关系弧线类型"]),
        relationship_type: string_aliases(
            object,
            &[
                "relationship_type",
                "relationshipType",
                "relationshiptype",
                "关系类型",
            ],
        ),
        stage: string_aliases(object, &["stage", "阶段", "当前阶段"]),
        next_expected_stage: string_aliases(
            object,
            &[
                "next_expected_stage",
                "nextExpectedStage",
                "nextexpectedstage",
                "下一阶段",
            ],
        ),
        start_state: string_aliases(
            object,
            &[
                "start_state",
                "startState",
                "startstate",
                "起始关系",
                "起始状态",
            ],
        ),
        current_state: string_aliases(
            object,
            &[
                "current_state",
                "currentState",
                "currentstate",
                "当前关系",
                "当前状态",
            ],
        ),
        desired_end_state: string_aliases(
            object,
            &[
                "desired_end_state",
                "desiredEndState",
                "desiredendstate",
                "终局关系",
                "目标关系",
            ],
        ),
        evidence: string_aliases(object, &["evidence", "证据"]),
        conflicts: list_aliases(object, &["conflicts", "冲突"]),
        secrets: list_aliases(object, &["secrets", "秘密"]),
        turning_points: list_aliases(
            object,
            &["turning_points", "turningPoints", "turningpoints", "转折点"],
        ),
        transition_history: Vec::new(),
        last_changed_chapter: usize_aliases(
            object,
            &[
                "last_changed_chapter",
                "lastChangedChapter",
                "lastchangedchapter",
                "最近变化章节",
            ],
        ),
    })
}

fn emotional_state_ledger_from_value(value: &Value) -> Vec<EmotionalStateLedgerEntry> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(emotional_state_entry_from_value)
            .filter(|entry| {
                !value_missing(&entry.character)
                    || !value_missing(&entry.current_emotion)
                    || !value_missing(&entry.expected_next_shift)
            })
            .take(16)
            .collect(),
        _ => Vec::new(),
    }
}

fn emotional_state_entry_from_value(value: &Value) -> Option<EmotionalStateLedgerEntry> {
    let object = value.as_object()?;
    Some(EmotionalStateLedgerEntry {
        character: string_aliases(object, &["character", "角色", "人物", "姓名"]),
        current_emotion: string_aliases(
            object,
            &[
                "current_emotion",
                "currentEmotion",
                "currentemotion",
                "当前情绪",
            ],
        ),
        pressure: string_aliases(object, &["pressure", "压力"]),
        desire: string_aliases(object, &["desire", "欲望"]),
        fear: string_aliases(object, &["fear", "恐惧"]),
        expected_next_shift: string_aliases(
            object,
            &[
                "expected_next_shift",
                "expectedNextShift",
                "expectednextshift",
                "下一情绪变化",
            ],
        ),
        payoff_target: string_aliases(
            object,
            &["payoff_target", "payoffTarget", "payofftarget", "兑现目标"],
        ),
        last_changed_chapter: usize_aliases(
            object,
            &[
                "last_changed_chapter",
                "lastChangedChapter",
                "lastchangedchapter",
                "最近变化章节",
            ],
        ),
        transition_history: Vec::new(),
    })
}

fn normalize_character_patch_candidates(characters: &mut Vec<CharacterContract>) {
    characters.retain(character_contract_has_patchable_anchor);
    let mut seen_names = std::collections::BTreeSet::new();
    characters.retain(|character| seen_names.insert(character.canonical_name.trim().to_string()));
    for character in characters.iter_mut() {
        if value_missing(&character.arc_end) && !value_missing(&character.planned_exit) {
            character.arc_end = character.planned_exit.clone();
        }
    }
    crate::tool::writing::creation_contract_model::normalize_character_contract_roles(
        characters, false,
    );
}

fn sanitize_character_name(value: &str) -> String {
    sanitize_generated_contract_scalar(value)
        .trim_matches(['"', '\'', '`', ':', '：', '\\', '/', ',', '，', '。', ' '])
        .trim()
        .to_string()
}

fn volume_contracts_from_value(value: &Value) -> Vec<VolumeContract> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(volume_contract_from_value)
            .filter(|volume| {
                !value_missing(&volume.title)
                    || !value_missing(&volume.objective)
                    || !value_missing(&volume.ending_change)
            })
            .take(12)
            .collect(),
        Value::String(text) => text
            .lines()
            .filter_map(|line| {
                let line = sanitize_generated_contract_scalar(line);
                (!value_missing(&line)).then_some(VolumeContract {
                    title: String::new(),
                    objective: line.clone(),
                    ending_change: line,
                })
            })
            .take(12)
            .collect(),
        _ => Vec::new(),
    }
}

fn volume_contract_from_value(value: &Value) -> Option<VolumeContract> {
    let object = value.as_object()?;
    let mut volume = VolumeContract {
        title: string_aliases(object, &["title", "volume_title", "卷名", "标题"]),
        objective: string_aliases(object, &["objective", "goal", "阶段目标", "目标"]),
        ending_change: string_aliases(
            object,
            &[
                "ending_change",
                "endingchange",
                "endingChange",
                "irreversible_change",
                "卷尾变化",
                "不可逆变化",
            ],
        ),
    };
    normalize_volume_contract_surface(&mut volume);
    Some(volume)
}

fn chapter_seed_contracts_from_value(value: &Value) -> Vec<ChapterSeedContract> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(chapter_seed_contract_from_value)
            .filter(|chapter| {
                !value_missing(&chapter.goal) || !value_missing(&chapter.expected_turn)
            })
            .take(16)
            .collect(),
        Value::String(text) => text
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let line = sanitize_generated_contract_scalar(line);
                (!value_missing(&line)).then_some(ChapterSeedContract {
                    number: Some(index + 1),
                    goal: line.clone(),
                    expected_turn: line,
                })
            })
            .take(16)
            .collect(),
        _ => Vec::new(),
    }
}

fn chapter_seed_contract_from_value(value: &Value) -> Option<ChapterSeedContract> {
    let object = value.as_object()?;
    let number = usize_aliases(
        object,
        &["number", "chapter", "chapter_number", "章节", "章"],
    );
    let chapter = ChapterSeedContract {
        number,
        goal: clean_chapter_goal_field(&string_aliases(
            object,
            &["goal", "objective", "目标", "本章目标", "事件目标"],
        )),
        expected_turn: clean_chapter_expected_turn_field(&string_aliases(
            object,
            &[
                "expected_turn",
                "expectedturn",
                "expectedTurn",
                "turn",
                "预期转折",
                "不可逆变化",
                "本章不可逆变化",
            ],
        )),
    };
    Some(chapter)
}

fn clean_chapter_goal_field(value: &str) -> String {
    strip_leading_contract_field_labels(
        value,
        &[
            "本章目标：",
            "本章目标:",
            "章节目标：",
            "章节目标:",
            "事件目标：",
            "事件目标:",
            "目标：",
            "目标:",
        ],
    )
}

fn clean_chapter_expected_turn_field(value: &str) -> String {
    strip_leading_contract_field_labels(
        value,
        &[
            "预期转折：",
            "预期转折:",
            "不可逆变化：",
            "不可逆变化:",
            "本章不可逆变化：",
            "本章不可逆变化:",
        ],
    )
}

fn strip_leading_contract_field_labels(value: &str, labels: &[&str]) -> String {
    let mut text = value.trim().to_string();
    for _ in 0..4 {
        let before = text.clone();
        text = text
            .trim_start_matches(['：', ':', '-', ' ', '；', ';'])
            .trim()
            .to_string();
        for label in labels {
            if let Some(tail) = text.strip_prefix(label) {
                text = tail
                    .trim_start_matches(['：', ':', '-', ' ', '；', ';'])
                    .trim()
                    .to_string();
            }
        }
        if text == before {
            break;
        }
    }
    text
}

fn payoff_matrix_from_value(value: &Value) -> Vec<PayoffMatrixEntry> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(payoff_matrix_entry_from_value)
            .filter(|entry| !value_missing(&entry.promise) || !value_missing(&entry.payoff_target))
            .take(16)
            .collect(),
        Value::String(text) => text
            .lines()
            .filter_map(|line| {
                let line = sanitize_generated_contract_scalar(line);
                (!value_missing(&line)).then_some(PayoffMatrixEntry {
                    promise: line.clone(),
                    payoff_target: line,
                    status: "planned".to_string(),
                    ..Default::default()
                })
            })
            .take(16)
            .collect(),
        _ => Vec::new(),
    }
}

fn payoff_matrix_entry_from_value(value: &Value) -> Option<PayoffMatrixEntry> {
    let object = value.as_object()?;
    Some(PayoffMatrixEntry {
        promise: string_aliases(object, &["promise", "hook", "承诺", "伏笔"]),
        introduced_chapter: usize_aliases(
            object,
            &[
                "introduced_chapter",
                "introducedchapter",
                "introducedChapter",
                "出现章节",
            ],
        ),
        payoff_target: string_aliases(
            object,
            &[
                "payoff_target",
                "payofftarget",
                "payoffTarget",
                "target",
                "兑现目标",
                "回收目标",
            ],
        ),
        payoff_chapter: usize_aliases(
            object,
            &[
                "payoff_chapter",
                "payoffchapter",
                "payoffChapter",
                "回收章节",
            ],
        ),
        status: string_aliases(object, &["status", "状态"]),
        evidence: list_aliases(object, &["evidence", "证据"]),
    })
}

fn skeleton_patch_empty(patch: &SkeletonPatch) -> bool {
    value_missing(&patch.genre)
        && value_missing(&patch.brief)
        && patch.target_units.is_none()
        && patch.chapter_unit_target.is_none()
        && patch.max_chapters_per_turn.is_none()
        && value_missing(&patch.premise)
        && value_missing(&patch.ending_desired_resolution)
        && value_missing(&patch.ending_final_state)
        && value_missing(&patch.protagonist_arc)
        && value_missing(&patch.world_imagery)
        && value_missing(&patch.main_causal_spine)
}

fn string_aliases(object: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    object_get_alias(object, keys)
        .and_then(Value::as_str)
        .map(sanitize_generated_contract_scalar)
        .unwrap_or_default()
}

fn list_aliases(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Vec<String> {
    object_get_alias(object, keys)
        .map(list_values_from_value)
        .unwrap_or_default()
}

fn list_values_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(sanitize_generated_contract_scalar)
            .filter(|value| !value_missing(value))
            .take(12)
            .collect(),
        Value::String(text) => split_contract_list_scalar(text)
            .into_iter()
            .filter(|value| !value_missing(value))
            .take(12)
            .collect(),
        _ => Vec::new(),
    }
}

fn split_contract_list_scalar(value: &str) -> Vec<String> {
    value
        .lines()
        .flat_map(|line| line.split(['；', ';']))
        .map(|item| {
            sanitize_generated_contract_scalar(
                item.trim()
                    .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t')),
            )
        })
        .filter(|item| !value_missing(item))
        .collect()
}

fn usize_aliases(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<usize> {
    object_get_alias(object, keys).and_then(|value| {
        value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
    })
}

fn object_get_alias<'a>(
    object: &'a serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<&'a Value> {
    for key in keys {
        if let Some(value) = object.get(*key) {
            return Some(value);
        }
    }
    let normalized_keys = keys
        .iter()
        .map(|key| normalize_schema_key(key))
        .collect::<Vec<_>>();
    object.iter().find_map(|(key, value)| {
        let normalized = normalize_schema_key(key);
        normalized_keys
            .iter()
            .any(|candidate| candidate == &normalized)
            .then_some(value)
    })
}

fn normalize_schema_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '_' | '-' | ' ' | '\t'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn field_string(raw: &str, labels: &[&str]) -> Option<String> {
    generated_contract_field(raw, labels)
        .map(|value| sanitize_generated_contract_scalar(&value))
        .filter(|value| !value_missing(value))
}

fn field_string_preserve_sentence(raw: &str, labels: &[&str]) -> Option<String> {
    for line in raw.lines() {
        let cleaned = line
            .trim()
            .trim_start_matches(|ch| matches!(ch, '*' | '-' | '+' | '#' | ' ' | '\t'))
            .replace("**", "")
            .replace("__", "")
            .replace('`', "");
        for label in labels {
            let Some((prefix, tail)) = split_contract_field_line(&cleaned, label) else {
                continue;
            };
            if !contract_field_prefix_allowed(prefix) {
                continue;
            }
            let value = tail.trim();
            if value.is_empty() {
                continue;
            }
            let value = trim_generated_contract_inline_field_tail(value);
            if !value_missing(&value) {
                return Some(value);
            }
        }
    }
    None
}

fn field_list(raw: &str, labels: &[&str]) -> Option<Vec<String>> {
    let value = field_string(raw, labels)?;
    let values = value
        .lines()
        .flat_map(|line| line.split(['；', ';', '，', ',', '、']))
        .map(|item| item.trim().trim_start_matches(['-', '*', '+', ' ']).trim())
        .filter(|item| !value_missing(item))
        .map(ToOwned::to_owned)
        .take(12)
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values)
}

fn field_usize(raw: &str, labels: &[&str]) -> Option<usize> {
    field_string(raw, labels)
        .and_then(|value| requested_total_unit_target(&value).or_else(|| value.parse().ok()))
}

fn character_field_pack_line_is_explicit_authority_entry(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    let has_name_field = [
        "name:",
        "name：",
        "canonical_name",
        "姓名:",
        "姓名：",
        "名字:",
        "名字：",
    ]
    .iter()
    .any(|marker| line.contains(marker) || lowered.contains(marker));
    let has_role_field = [
        "role:",
        "role：",
        "角色:",
        "角色：",
        "身份:",
        "身份：",
        "定位:",
        "定位：",
    ]
    .iter()
    .any(|marker| line.contains(marker) || lowered.contains(marker));
    has_name_field && has_role_field
}

fn character_contract_has_patchable_anchor(character: &CharacterContract) -> bool {
    let name = character.canonical_name.trim();
    !value_missing(&character.canonical_name)
        && !character_name_is_field_label_or_abstract_term(name)
        && (!value_missing(&character.role)
            || !value_missing(&character.desire)
            || !value_missing(&character.fear)
            || !value_missing(&character.bottom_line)
            || !value_missing(&character.arc_start)
            || !value_missing(&character.arc_end))
}

fn character_name_is_field_label_or_abstract_term(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    [
        "角色权威表",
        "人物权威表",
        "角色档案",
        "人物档案",
        "关系线",
        "情感线",
        "感知",
        "灵石",
        "符文",
        "觉醒传承",
        "传承",
        "系统",
        "灵脉",
        "命运",
        "破解符文阵法",
        "character ledger",
        "characters",
        "relationship",
    ]
    .iter()
    .any(|term| name == *term || lowered == term.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_json_patch_uses_terminal_state_as_missing_arc_end() {
        let value = serde_json::json!({
            "characters": [{
                "canonical_name": "对手槽位",
                "role": "关键对手",
                "desire": "保住账目垄断",
                "fear": "原始证据被公开",
                "bottom_line": "绝不主动交出原始账册",
                "arc_start": "控制全部查账入口",
                "planned_exit": "证据公开后失去账目控制权"
            }]
        });

        let patch = character_patch_from_value(&value).expect("character patch");

        assert_eq!(patch.characters[0].arc_end, "证据公开后失去账目控制权");
    }

    #[test]
    fn character_json_patch_normalizes_common_model_role_enums_once() {
        let value = serde_json::json!({
            "characters": [
                {
                    "name": "林浅",
                    "role": "female_lead",
                    "desire": "保住社区剧场",
                    "fear": "剧场被拆除",
                    "bottom_line": "绝不牺牲演出安全换取续租",
                    "arc_start": "独自控制全部细节",
                    "arc_end": "学会信任团队"
                },
                {
                    "name": "陈叙",
                    "role": "male_lead",
                    "desire": "完成兼顾社区记忆的改造",
                    "fear": "设计抹去剧场历史",
                    "bottom_line": "绝不掩盖结构安全问题",
                    "arc_start": "只相信数据",
                    "arc_end": "愿意共同承担"
                },
                {
                    "name": "赵建国",
                    "role": "antagonist",
                    "desire": "按期完成商业改造",
                    "fear": "项目延期导致资金链断裂",
                    "bottom_line": "绝不接受没有量化价值的延期",
                    "arc_start": "坚持强制清退",
                    "arc_end": "接受长期续租"
                }
            ]
        });

        let patch = character_patch_from_value(&value).expect("character patch");

        assert_eq!(
            patch
                .characters
                .iter()
                .filter(|character| character.role_looks_primary())
                .count(),
            1
        );
        assert_eq!(patch.characters[0].role, "女主");
        assert_eq!(patch.characters[1].role, "关键关系对象");
        assert_eq!(patch.characters[2].role, "关键对手");
    }

    #[test]
    fn character_patch_wrapper_accepts_direct_character_array() {
        let value = serde_json::json!({
            "character_patch": [
                {
                    "canonical_name": "唐照棠",
                    "role": "女主",
                    "desire": "查清异常检验编号背后的数据造假链条",
                    "fear": "无辜患者因证据被覆盖而继续受害",
                    "bottom_line": "绝不篡改或销毁患者的原始病历",
                    "arc_start": "只关注临床救治",
                    "arc_end": "敢于挑战制度性造假",
                    "planned_entry": "第一章",
                    "planned_exit": "案件完成清算后继续守护急诊科"
                },
                {
                    "canonical_name": "商承衡",
                    "role": "男主",
                    "desire": "还原火灾与骗赔案之间的证据链",
                    "fear": "火场证据被利益集团彻底销毁",
                    "bottom_line": "绝不提交未经复核的火灾结论",
                    "arc_start": "习惯独自调查",
                    "arc_end": "学会与医疗专业人员共享证据",
                    "planned_entry": "第二章",
                    "planned_exit": "案件结束后回归火灾调查岗位"
                },
                {
                    "canonical_name": "赵宏图",
                    "role": "关键对手",
                    "desire": "维持医疗数据造假带来的骗赔收益",
                    "fear": "原始数据公开后失去职位与利益网络",
                    "bottom_line": "绝不亲自签署伪造的病历记录",
                    "arc_start": "控制医院数据入口",
                    "arc_end": "证据公开后失去控制权",
                    "planned_entry": "第一卷",
                    "planned_exit": "终局接受调查"
                }
            ]
        });

        let patch = normalize_patch_json(&value.to_string()).expect("character patch");
        let CreationContractPatch::Characters(patch) = patch else {
            panic!("expected character patch");
        };

        assert_eq!(patch.characters.len(), 3);
        assert_eq!(
            patch
                .characters
                .iter()
                .filter(|character| character.role_looks_primary())
                .count(),
            1
        );
        assert_eq!(patch.characters[0].role, "女主");
        assert_eq!(patch.characters[1].role, "关键关系对象");
    }

    #[test]
    fn plot_json_patch_preserves_the_full_volume_plan_and_bounds_only_near_chapters() {
        let volumes = (1..=7)
            .map(|number| {
                serde_json::json!({
                    "title": format!("第{number}卷"),
                    "objective": format!("推进阶段目标{number}"),
                    "ending_change": format!("形成不可逆变化{number}")
                })
            })
            .collect::<Vec<_>>();
        let near_chapters = (1..=10)
            .map(|number| {
                serde_json::json!({
                    "number": number,
                    "goal": format!("完成事件目标{number}"),
                    "expected_turn": format!("发生事件变化{number}")
                })
            })
            .collect::<Vec<_>>();
        let value = serde_json::json!({
            "outline": {
                "volumes": volumes,
                "near_chapters": near_chapters
            }
        });

        let patch = plot_patch_from_value(&value).expect("plot patch");

        assert_eq!(patch.volumes.len(), 7);
        assert_eq!(patch.near_chapters.len(), MAX_NEAR_CHAPTERS);
        assert_eq!(patch.volumes.last().expect("volume").title, "第7卷");
        assert_eq!(patch.near_chapters.last().expect("chapter").number, Some(8));
    }

    #[test]
    fn metadata_json_patch_preserves_the_full_volume_plan_and_bounds_only_near_chapters() {
        let volumes = (1..=7)
            .map(|number| {
                serde_json::json!({
                    "title": format!("第{number}卷"),
                    "objective": format!("推进阶段目标{number}"),
                    "ending_change": format!("形成不可逆变化{number}")
                })
            })
            .collect::<Vec<_>>();
        let near_chapters = (1..=10)
            .map(|number| {
                serde_json::json!({
                    "number": number,
                    "goal": format!("完成事件目标{number}"),
                    "expected_turn": format!("发生事件变化{number}")
                })
            })
            .collect::<Vec<_>>();
        let value = serde_json::json!({
            "outline": {
                "volumes": volumes,
                "near_chapters": near_chapters
            }
        });

        let patch = metadata_patch_from_value(&value).expect("metadata patch");

        assert_eq!(patch.volumes.len(), 7);
        assert_eq!(patch.near_chapters.len(), MAX_NEAR_CHAPTERS);
    }

    #[test]
    fn creation_plot_window_preserves_model_chapter_numbers_for_validation() {
        let value = serde_json::json!({
            "outline": {
                "near_chapters": [
                    {"number": 1, "goal": "入住公寓", "expected_turn": "听见异常噪音"},
                    {"number": 2, "goal": "询问邻居", "expected_turn": "取得第一份录音"},
                    {"number": 3, "goal": "分析录音", "expected_turn": "发现频率密码"},
                    {"number": 5, "goal": "追踪信号", "expected_turn": "锁定地下通道"}
                ]
            }
        });

        let patch = plot_patch_from_value(&value).expect("plot patch");
        let numbers = patch
            .near_chapters
            .iter()
            .map(|chapter| chapter.number)
            .collect::<Vec<_>>();

        assert_eq!(numbers, vec![Some(1), Some(2), Some(3), Some(5)]);
    }

    #[test]
    fn character_json_patch_preserves_targeted_anchor_repair_without_role() {
        let raw = r#"{
            "patch_type": "character_patch",
            "characters": [
                {
                    "canonical_name": "闻予野",
                    "bottom_line": "绝不拿住客安全换取民宿口碑"
                }
            ]
        }"#;
        let draft = build_initial_creation_draft(
            "session",
            "fiction",
            "写一部当代山村民宿创业轻喜剧，每章2500字，一共5万字。",
        )
        .expect("draft");

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");

        let CreationContractPatch::Characters(characters) = patch else {
            panic!("expected character patch");
        };
        assert_eq!(characters.characters.len(), 1);
        assert_eq!(characters.characters[0].canonical_name, "闻予野");
        assert_eq!(
            characters.characters[0].bottom_line,
            "绝不拿住客安全换取民宿口碑"
        );
        assert!(characters.characters[0].role.is_empty());
    }

    #[test]
    fn character_field_pack_accepts_entries_below_an_empty_section_heading() {
        let raw = "角色权威表：\n\
姓名：沈知衡，角色：主角，欲望：查清玉牒真相，恐惧：证据被毁，底线：绝不牺牲无辜者，弧线起点：只想自保，弧线终点：公开证据改写制度。\n\
姓名：程望舟，角色：关键关系对象，欲望：守住史馆底稿，恐惧：家族被连坐，底线：必须守住史官原稿，弧线起点：拒绝表态，弧线终点：主动作证。\n\
姓名：赵崇序，角色：关键对手，欲望：控制朝堂，恐惧：密诏公开，底线：绝不交出禁军调令权，弧线起点：垄断权力，弧线终点：失去控制权。";
        let draft = build_initial_creation_draft(
            "session",
            "fiction",
            "写一部历史权谋小说，每章2500字，一共10万字。",
        )
        .expect("draft");

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");
        let CreationContractPatch::Characters(characters) = patch else {
            panic!("expected character patch");
        };

        assert_eq!(characters.characters.len(), 3);
        assert!(characters.characters[0].role_looks_primary());
        assert_eq!(characters.characters[1].role, "关键关系对象");
        assert_eq!(characters.characters[2].role, "对手");
    }

    #[test]
    fn one_line_contract_batch_projects_typed_volume_and_chapter_plans() {
        let raw = "patch_type: contract_batch全书大纲：工匠查清断枢制度并公开证据。分卷规划：第一卷《入城查枢》：本卷目标：取得伪账；卷尾变化：确认账册被换；第二卷《断枢公开》：本卷目标：公开完整证据；卷尾变化：旧制度被不可逆改写。近期章节包：第1章《盲刻》：本章目标：取得第一张残页；预期转折：残页指向内城。第2章《换册》：本章目标：核对残页编号；预期转折：同伴承认换册。第3章《夜枢》：本章目标：进入枢房取证；预期转折：守卫封锁出口并留下后续债务。";
        let draft = build_initial_creation_draft(
            "session",
            "fiction",
            "写一部东方奇幻小说，每章2500字，一共10万字。",
        )
        .expect("draft");

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");
        let CreationContractPatch::Plot(plot) = patch else {
            panic!("expected plot patch");
        };

        assert_eq!(plot.volumes.len(), 2, "{plot:?}");
        assert_eq!(plot.near_chapters.len(), 3, "{plot:?}");
        assert_eq!(plot.near_chapters[0].number, Some(1));
        assert_ne!(
            plot.near_chapters[0].goal,
            plot.near_chapters[0].expected_turn
        );
    }

    #[test]
    fn one_line_initial_contract_batch_keeps_all_existing_typed_owners() {
        let raw = "patch_type: contract_batch书名：零号雨书名候选：零号雨（关键物件，来自零号记忆）；天枢裂隙（关键地点，来自核心裂隙）；全息雨（世界意象，来自城市酸雨）书名理由：书名来自主线中的关键记忆题材：赛博朋克简述：拾荒者发现足以改写城市秩序的零号记忆总字数：100000每章字数：2500故事前提：底层拾荒者取得被企业封锁的记忆证据终局方向：主角公开证据并关闭记忆垄断终局状态：城市的记忆交易许可被永久废止主角弧线：从只求自保到承担公共责任世界观意象：全息雨覆盖锈蚀街区总主线因果链：取得记忆引来追杀，追查真相直到公开证据角色权威表：姓名：顾望衡，角色：主角，欲望：查清零号记忆来源，恐惧：失去自身记忆，底线：绝不出售无辜者的原始记忆，弧线起点：只求自保，弧线终点：承担公共责任。姓名：秦照野，角色：关键同伴，欲望：守住地下档案，恐惧：证据被销毁，底线：必须守住原始账本，弧线起点：拒绝合作，弧线终点：主动作证。姓名：许闻枢，角色：关键对手，欲望：维持企业垄断，恐惧：旧案公开，底线：绝不交出核心控制权，弧线起点：控制全城，弧线终点：失去制度权力。核心主题：记忆与自我；技术与阶级世界规则：提取记忆会损伤情感；义体过载会灼伤神经；核心断电会永久删除备份叙事风格：冷硬克制必须避免：角色改名；无代价升级；提前终局全书大纲：主角从拾得证据到公开垄断真相。第1卷《雨城追索》：本卷目标：取得伪账；卷尾变化：确认账册被换。第2卷《天枢公开》：本卷目标：公开完整证据；卷尾变化：旧制度被不可逆改写。第1章《盲刻》：本章目标：取得第一张残页；预期转折：残页指向内城。第2章《换册》：本章目标：核对残页编号；预期转折：同伴承认换册。第3章《夜枢》：本章目标：进入枢房取证；预期转折：守卫封锁出口并留下后续债务。";
        let draft = build_initial_creation_draft(
            "session",
            "fiction",
            "请从零创建并自动写一部赛博朋克长篇小说，总字数10万字，使用2500字每章档。",
        )
        .expect("draft");

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");
        let CreationContractPatch::Batch(items) = patch else {
            panic!("expected complete typed batch");
        };

        assert!(items
            .iter()
            .any(|item| matches!(item, CreationContractPatch::Skeleton(_))));
        assert!(items
            .iter()
            .any(|item| matches!(item, CreationContractPatch::Characters(value) if value.characters.len() == 3)));
        assert!(items
            .iter()
            .any(|item| matches!(item, CreationContractPatch::Governance(_))));
        assert!(items.iter().any(
            |item| matches!(item, CreationContractPatch::Plot(value) if value.volumes.len() == 2 && value.near_chapters.len() == 3)
        ));
    }

    #[test]
    fn title_field_pack_accepts_numbered_candidate_lines() {
        let raw = "候选1：《旧城翻盘局》 - 来自主角在旧城商业局中反杀对手\n\
候选2：《三千万退婚局》 - 来自退婚债务和第一桶金爽点\n\
候选3：《雨夜开盘》 - 来自暴雨夜重生和资本开局\n\
书名理由：《旧城翻盘局》把旧城资产、破产翻盘和资本设局三个故事锚点绑定在一起。";

        let patch = title_patch_from_field_pack(raw).expect("title patch");

        assert_eq!(patch.canonical_title, "旧城翻盘局");
        assert!(patch.candidates.iter().any(|item| item == "三千万退婚局"));
        assert!(patch.rationale.contains("旧城资产"));
    }

    #[test]
    fn title_json_patch_infers_missing_title_from_rationale() {
        let raw = r#"{
            "title_patch": {
                "rationale": "书名沉岛倒计时直接对应三天内查明沉没原因并托起孤岛的终局压力。"
            }
        }"#;
        let draft = build_initial_creation_draft(
            "session",
            "fiction",
            "帮我写一部近未来海岛灾难悬疑小说，每章2500字，一共5万字。",
        )
        .expect("draft");

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");

        let CreationContractPatch::Title(title) = patch else {
            panic!("expected title patch");
        };
        assert_eq!(title.canonical_title, "沉岛倒计时");
    }

    #[test]
    fn title_json_patch_infers_title_inside_cjk_single_quotes() {
        let raw = r#"{
            "title_patch": {
                "rationale": "书名‘冰渊回声’源自终局中主角听见古老生物共鸣的关键选择。"
            }
        }"#;
        let draft = build_initial_creation_draft(
            "session",
            "fiction",
            "帮我写一部极地科考冒险小说，每章2500字，一共5万字。",
        )
        .expect("draft");

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");

        let CreationContractPatch::Title(title) = patch else {
            panic!("expected title patch");
        };
        assert_eq!(title.canonical_title, "冰渊回声");
    }

    #[test]
    fn governance_json_patch_accepts_string_world_rules() {
        let raw = r#"{
            "world_rules": "规则1：蒸汽核心每次超压都会消耗操作者记忆；规则2：机械义眼只能在雾中看见被篡改的齿轮痕迹；规则3：守门人契约一旦签下就会把城市债务转移到签约者身上"
        }"#;
        let draft = build_initial_creation_draft(
            "session",
            "fiction",
            "帮我写一部蒸汽朋克悬疑小说，每章2500字，一共5万字。",
        )
        .expect("draft");

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");

        let CreationContractPatch::Governance(governance) = patch else {
            panic!("expected governance patch");
        };
        assert_eq!(
            governance.world_rules.len(),
            3,
            "{:?}",
            governance.world_rules
        );
        assert!(governance
            .world_rules
            .iter()
            .any(|rule| rule.contains("蒸汽核心")));
    }

    #[test]
    fn governance_patch_cannot_replace_plot_owned_payoff_matrix() {
        let mut draft = build_initial_creation_draft(
            "governance-cannot-own-payoff",
            "fiction",
            "帮我写一部校园悬疑小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        draft.payoff_matrix = vec![PayoffMatrixEntry {
            promise: "旧磁带藏有被剪掉的校庆广播".to_string(),
            payoff_target: "终局由主角公开原始广播并证明事故真相".to_string(),
            status: "planned".to_string(),
            ..Default::default()
        }];
        let raw = r#"{
            "patch_type":"governance_patch",
            "world_rules":["播放原始声纹会消耗听者一段近期记忆。"],
            "payoff_matrix":[{"promise":"","payoff_target":"错误覆盖","status":"planned"}],
            "structured":{"payoff_matrix":[{"promise":"错误承诺","payoff_target":"错误覆盖","status":"planned"}]}
        }"#;

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");
        patch.apply_to_draft(&mut draft);

        assert_eq!(draft.payoff_matrix.len(), 1);
        assert_eq!(draft.payoff_matrix[0].promise, "旧磁带藏有被剪掉的校庆广播");
        assert_eq!(
            draft.payoff_matrix[0].payoff_target,
            "终局由主角公开原始广播并证明事故真相"
        );
    }

    #[test]
    fn governance_patch_accepts_numbered_world_rule_lines() {
        let raw = "规则1：每次接近岛心灯塔都必须消耗淡水配额，否则会触发雾潮迷航。\n\
规则2：任何人隐瞒沉船账册线索都会留下可追踪的潮盐印记。\n\
规则3：救援船只能在涨潮窗口靠岸，错过窗口就会失去下一章行动机会。";
        let draft = build_initial_creation_draft(
            "session",
            "fiction",
            "帮我写一部海岛悬疑冒险小说，每章2500字，一共5万字。",
        )
        .expect("draft");

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");

        let CreationContractPatch::Governance(governance) = patch else {
            panic!("expected governance patch");
        };
        assert_eq!(
            governance.world_rules.len(),
            3,
            "{:?}",
            governance.world_rules
        );
        assert!(governance
            .world_rules
            .iter()
            .any(|rule| rule.contains("淡水配额")));
    }

    #[test]
    fn governance_json_patch_keeps_array_world_rule_sentences_intact() {
        let raw = r#"{
            "patch_type": "governance_patch",
            "world_rules": [
                "听觉共鸣代价：每次深度解读海浪低语或操控潮汐波动，需消耗自身体内的水分；若连续高强度使用导致身体脱水，将陷入昏迷甚至骨骼碎裂。",
                "深渊压力限制：任何携带陆地空气或淡水进入深渊核心的容器，其耐压极限仅为30分钟，超时则容器破碎。",
                "潮汐契约排斥：除非媒介建立连接，否则海兽无法理解人类语言，人类武器也无法精准命中精神护盾。"
            ]
        }"#;
        let draft = build_initial_creation_draft(
            "session",
            "fiction",
            "帮我写一部海洋灾难奇幻小说，每章2500字，一共5万字。",
        )
        .expect("draft");

        let patch = normalize_creation_contract_patch_boundary(&draft, raw).expect("patch");

        let CreationContractPatch::Governance(governance) = patch else {
            panic!("expected governance patch");
        };
        assert_eq!(
            governance.world_rules.len(),
            3,
            "{:?}",
            governance.world_rules
        );
        assert!(governance.world_rules[0].contains("需消耗自身体内的水分"));
        assert!(governance.world_rules[0].contains("陷入昏迷甚至骨骼碎裂"));
    }

    #[test]
    fn volume_field_cleanup_is_idempotent_across_repair_rounds() {
        let polluted = "关键证据暴露，冲突升级到公开对抗；卷尾变化：主角完成终局选择；预期转折：主角确认入口；预期转折：主角确认入口";

        let once = clean_volume_contract_objective(polluted, "", "");
        let twice = clean_volume_contract_objective(&once, "", "");

        assert_eq!(once, "关键证据暴露，冲突升级到公开对抗");
        assert_eq!(twice, once);
    }

    #[test]
    fn full_contract_volume_uses_the_same_cleanup_as_plot_patches() {
        let mut volume = VolumeContract {
            title: "第3卷《连锁震荡》".to_string(),
            objective: "阶段目标：验证决策模型".to_string(),
            ending_change:
                "团队验证决策模型，系统稳定性提升；卷尾变化：新制度确立；预期转折：终局完成"
                    .to_string(),
        };

        normalize_volume_contract_surface(&mut volume);
        let once = volume.clone();
        normalize_volume_contract_surface(&mut volume);

        assert_eq!(volume.title, "连锁震荡");
        assert_eq!(volume.objective, "验证决策模型");
        assert_eq!(volume.ending_change, "团队验证决策模型，系统稳定性提升");
        assert_eq!(volume, once);
    }

    #[test]
    fn volume_cleanup_removes_field_labels_and_following_section_heading_residue() {
        let mut volume = VolumeContract {
            title: "第2卷《余烬》".to_string(),
            objective: "本卷目标：阮启岚策划爆炸，陆承言与唐星原合作".to_string(),
            ending_change: "爆炸证据被公开，唐星原找到哥哥。近期章节包".to_string(),
        };

        normalize_volume_contract_surface(&mut volume);

        assert_eq!(volume.title, "余烬");
        assert_eq!(volume.objective, "阮启岚策划爆炸，陆承言与唐星原合作");
        assert_eq!(volume.ending_change, "爆炸证据被公开，唐星原找到哥哥。");
    }

    #[test]
    fn prose_reference_to_a_volume_does_not_create_an_extra_volume() {
        let outline = "第1卷《风眼初现》：发现风脉异常；卷尾变化：主角成为通缉犯\n\
第2卷《重塑秩序》：进入世界核心；卷尾变化：主角重启风脉\n\
第6章 本章目标：准备对抗神族清洗；预期转折：故事进入第一卷高潮前奏";

        let patch = derive_plot_contract_from_outline_text(outline);

        assert_eq!(patch.volumes.len(), 2, "{:?}", patch.volumes);
        assert_eq!(patch.volumes[0].title, "风眼初现");
        assert_eq!(patch.volumes[1].title, "重塑秩序");
        assert_eq!(patch.near_chapters.len(), 1);
        assert!(patch.near_chapters[0]
            .expected_turn
            .contains("进入第一卷高潮前奏"));
    }

    #[test]
    fn terminal_volume_reference_at_segment_end_does_not_create_an_extra_volume() {
        let outline = "第1卷《旧账重开》：主角重新取得谈判入口；卷尾变化：对手被迫公开第一份账目\n\
第2卷《逆价收网》：主角建立新的供应链；卷尾变化：主角持续推进到第4卷\n\
第3卷《董事暗潮》：盟友取得关键表决权；卷尾变化：终局冲突被迫提前\n\
第4卷《新秩序》：完成最终商业对决；卷尾变化：主角在第4卷完成制度重建";

        let patch = derive_plot_contract_from_outline_text(outline);

        assert_eq!(patch.volumes.len(), 4, "{:?}", patch.volumes);
        assert_eq!(patch.volumes[3].title, "新秩序");
    }

    #[test]
    fn prose_volume_reference_followed_by_whitespace_is_not_a_plan_header() {
        let outline =
            "第1卷《入山》：发现宗门账册；卷尾变化：主角决定追查到第4卷 终局才公开的旧案\n\
第2卷《问剑》：取得第一份证词；卷尾变化：证人被迫离山\n\
第3卷《旧盟》：重建破裂的同盟；卷尾变化：幕后人现身\n\
第4卷《归宗》：公开旧案；卷尾变化：主角完成最终选择";

        let patch = derive_plot_contract_from_outline_text(outline);

        assert_eq!(patch.volumes.len(), 4, "{:?}", patch.volumes);
        assert_eq!(patch.volumes[3].title, "归宗");
    }

    #[test]
    fn repeated_prose_volume_sentences_cannot_expand_the_typed_volume_plan() {
        let repeated = (0..70)
            .map(|_| "第1卷中，主角取得证据并继续追查")
            .collect::<Vec<_>>()
            .join("；");
        let outline = format!(
            "第1卷《潜流》：查明第一条数据异常；卷尾变化：获得证据但留下主线债务\n{repeated}\n第2卷《逆流》：公开完整证据；卷尾变化：旧制度被不可逆改写"
        );

        let patch = derive_plot_contract_from_outline_text(&outline);

        assert_eq!(patch.volumes.len(), 2, "{:?}", patch.volumes);
        assert_eq!(patch.volumes[0].title, "潜流");
        assert_eq!(patch.volumes[1].title, "逆流");
    }

    #[test]
    fn full_contract_preserves_more_than_eight_distinct_volumes() {
        let outline = (1..=12)
            .map(|number| {
                format!(
                    "第{number}卷《阶段{number}》：本卷目标：完成第{number}阶段调查；卷尾变化：第{number}阶段证据被永久公开"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let patch = derive_plot_contract_from_outline_text(&outline);

        assert_eq!(patch.volumes.len(), 12);
        assert_eq!(patch.volumes.last().expect("last volume").title, "阶段12");
    }

    #[test]
    fn compact_chapter_header_with_explicit_goal_remains_parseable() {
        let outline = "故事总纲第1章 本章目标：主角取得第一份证据；预期转折：证人失踪";

        let patch = derive_plot_contract_from_outline_text(outline);

        assert_eq!(patch.near_chapters.len(), 1, "{:?}", patch.near_chapters);
        assert_eq!(patch.near_chapters[0].number, Some(1));
    }
}
