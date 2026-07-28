/// Writing-domain routing policy for governed long-form fiction.
///
/// Delegation may ask whether a worker/task pair should use the governed
/// writing surface, but the intent rules live here with the writing tools.
use benshu_brain::runtime::continuous_task::{ContinuousStepRequest, ContinuousTaskContract};
use benshu_compression::ellipsize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::longform_guard::LongformArtifactGuard;

/// One authority for the selected chapter tier's absolute saved-body ceiling.
/// The workflow currently exposes 2500- and 5000-unit tiers, but keeping this
/// calculation target-based also preserves migrated/custom contracts.
pub(crate) fn chapter_tier_max_units(target: usize) -> usize {
    target.max(1).saturating_mul(2)
}

/// Returns the number of chapters needed to cover a positive total-unit target
/// at a positive per-chapter target. Callers retain responsibility for choosing
/// fallback inputs; the rounding rule lives here so contracts, prompts,
/// planning, persistence, and user-facing summaries cannot disagree.
pub(crate) fn expected_chapter_count(
    target_units: usize,
    chapter_unit_target: usize,
) -> Option<usize> {
    (target_units > 0 && chapter_unit_target > 0)
        .then(|| target_units.div_ceil(chapter_unit_target).max(1))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct GenreGovernanceProfile {
    pub genre_family: String,
    pub control_axes: Vec<GenreControlAxis>,
    pub escalation_rules: Vec<String>,
    pub failure_modes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct GenreControlAxis {
    pub name: String,
    pub current_level: String,
    pub allowed_progression: String,
    pub hard_limits: Vec<String>,
}

pub(crate) fn genre_governance_profile(genre: &str, language: &str) -> GenreGovernanceProfile {
    let family = fiction_genre_profile(genre, Some(genre));
    let chinese = is_chinese_language(language);
    let mut profile = GenreGovernanceProfile {
        genre_family: family.as_str().to_string(),
        ..Default::default()
    };
    match family {
        FictionGenreProfile::Fantasy | FictionGenreProfile::Xianxia => {
            profile.control_axes.push(governance_axis(
                if chinese { "力量阶层" } else { "power scale" },
                if chinese { "初始层级" } else { "initial tier" },
                if chinese {
                    "突破必须有代价、训练、资源、失败或关系后果。"
                } else {
                    "Power increases require cost, training, resources, failure, or relationship consequences."
                },
            ));
            profile.control_axes.push(governance_axis(
                if chinese {
                    "敌我压力"
                } else {
                    "opposition pressure"
                },
                if chinese {
                    "局部可胜，整体仍有压力"
                } else {
                    "local wins, broader pressure remains"
                },
                if chinese {
                    "胜利不能永久清空后续冲突。"
                } else {
                    "Victories must not erase future conflict permanently."
                },
            ));
            profile.escalation_rules.push(if chinese {
                "不得无铺垫秒杀关键敌人；压倒性胜利必须制造新的代价或更高层问题。".to_string()
            } else {
                "Do not resolve key enemies by unseeded instant domination; overwhelming wins must create cost or higher-order problems.".to_string()
            });
        }
        FictionGenreProfile::Romance => {
            profile.control_axes.push(governance_axis(
                if chinese {
                    "关系温度"
                } else {
                    "relationship temperature"
                },
                if chinese {
                    "由误解、信任、选择逐步变化"
                } else {
                    "changes through misunderstanding, trust, and choice"
                },
                if chinese {
                    "亲密推进必须有情绪因果，不能跳步。"
                } else {
                    "Intimacy requires emotional causality; do not skip stages."
                },
            ));
            profile.escalation_rules.push(if chinese {
                "冲突应来自价值、误会、现实压力或人物缺陷，不要靠反复失忆式误会拖延。".to_string()
            } else {
                "Conflict should arise from values, misunderstanding, pressure, or flaws, not repetitive amnesia-like delays.".to_string()
            });
        }
        FictionGenreProfile::ScienceFiction => profile.control_axes.push(governance_axis(
            if chinese {
                "技术边界"
            } else {
                "technology boundary"
            },
            if chinese {
                "技术可改变问题但不能免费解决一切"
            } else {
                "technology changes problems but does not solve everything for free"
            },
            if chinese {
                "新技术必须有约束、副作用或社会代价。"
            } else {
                "New technology needs constraints, side effects, or social cost."
            },
        )),
        FictionGenreProfile::Mystery => profile.control_axes.push(governance_axis(
            if chinese {
                "线索公平性"
            } else {
                "clue fairness"
            },
            if chinese {
                "读者可回看理解"
            } else {
                "reader can understand in hindsight"
            },
            if chinese {
                "关键真相必须提前埋线，不能凭空出现。"
            } else {
                "Core truth must be seeded before revelation."
            },
        )),
        FictionGenreProfile::General => profile.control_axes.push(governance_axis(
            if chinese {
                "叙事因果"
            } else {
                "narrative causality"
            },
            if chinese {
                "选择导致后果"
            } else {
                "choices create consequences"
            },
            if chinese {
                "转折必须来自人物、世界或已埋信息。"
            } else {
                "Turns must come from character, world, or seeded information."
            },
        )),
    }
    profile.failure_modes.push(if chinese {
        "漂移：角色突然违背欲望、恐惧或底线且没有事件原因。".to_string()
    } else {
        "Drift: characters violate desire, fear, or bottom line without causal events.".to_string()
    });
    profile.failure_modes.push(if chinese {
        "膨胀：新能力、资源、关系或真相绕过既有代价。".to_string()
    } else {
        "Inflation: new ability, resource, relationship, or truth bypasses established cost."
            .to_string()
    });
    profile
}

fn governance_axis(name: &str, current_level: &str, allowed_progression: &str) -> GenreControlAxis {
    GenreControlAxis {
        name: name.to_string(),
        current_level: current_level.to_string(),
        allowed_progression: allowed_progression.to_string(),
        hard_limits: vec![
            "Changes must be supported by approved chapter facts.".to_string(),
            "Major jumps require setup, consequence, and ledger update.".to_string(),
        ],
    }
}

fn is_chinese_language(language: &str) -> bool {
    let lowered = language.to_ascii_lowercase();
    lowered.contains("zh") || lowered.contains("chinese") || language.contains('中')
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LongformContinuationSeed {
    pub(crate) title: Option<String>,
    pub(crate) primary_anchor: Option<String>,
    pub(crate) last_next_hook: Option<String>,
    pub(crate) context: Option<String>,
}

impl LongformContinuationSeed {
    pub(crate) fn has_identity(&self) -> bool {
        self.title
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || self
                .primary_anchor
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }
}

pub(crate) fn worker_has_novel_studio_tool(blueprint_tools: &[String]) -> bool {
    blueprint_tools
        .iter()
        .any(|tool| matches!(tool.as_str(), "novel_studio" | "writing"))
}

pub(crate) fn task_requests_governed_fiction_project(
    task: &str,
    requested_text_target_chars: impl Fn(&str) -> Option<usize>,
    longform_step_target_chars: usize,
) -> bool {
    let lowered = task.to_lowercase();
    let fiction_intent = ["novel", "fiction", "story", "multi-chapter", "book-length"]
        .iter()
        .any(|term| lowered.contains(term))
        || [
            "小说",
            "故事",
            "剧情",
            "角色",
            "人物",
            "世界观",
            "修炼体系",
            "伏笔",
            "设定",
        ]
        .iter()
        .any(|term| task.contains(term));

    if !fiction_intent {
        return false;
    }

    let write_intent = [
        "write", "draft", "create", "generate", "compose", "author", "start",
    ]
    .iter()
    .any(|term| lowered.contains(term))
        || ["写", "创作", "生成", "起草", "开始写", "写一部", "写一个"]
            .iter()
            .any(|term| task.contains(term));
    if write_intent {
        return true;
    }

    [
        "chapter",
        "chapters",
        "book-length",
        "long-form",
        "longform",
    ]
    .iter()
    .any(|term| lowered.contains(term))
        || [
            "长篇",
            "章节",
            "上一章",
            "下一章",
            "续写",
            "第",
            "章",
            "完整",
            "连续",
            "不漂移",
            "世界观",
            "主线",
            "伏笔",
            "人物",
            "地点",
            "设定",
            "修炼体系",
        ]
        .iter()
        .any(|term| task.contains(term))
        || lowered.contains("/generated/novels/")
        || lowered.contains("data/generated/novels/")
        || lowered.contains("project_path")
        || requested_text_target_chars(task)
            .is_some_and(|target| target > longform_step_target_chars * 2)
}

pub(crate) fn should_route_writer_fiction_to_novel_studio(
    blueprint_tools: &[String],
    task: &str,
    requested_text_target_chars: impl Fn(&str) -> Option<usize>,
    longform_step_target_chars: usize,
) -> bool {
    worker_has_novel_studio_tool(blueprint_tools)
        && !task.contains("[BENSHU_NOVEL_CONTENT_OPERATION]")
        && !task_requests_fiction_project_readback(task)
        && task_requests_governed_fiction_project(
            task,
            requested_text_target_chars,
            longform_step_target_chars,
        )
}

pub(crate) fn task_requests_fiction_project_readback(task: &str) -> bool {
    let lowered = task.to_lowercase();
    let has_project_ref = lowered.contains("/generated/novels/")
        || lowered.contains("data/generated/novels/")
        || lowered.contains("project_path")
        || lowered.contains("existing artifact/work-in-progress context");
    if !has_project_ref {
        return false;
    }
    let intent_task = user_request_slice_for_readback(task);
    let intent_lowered = intent_task.to_lowercase();
    let read_intent = [
        "summarize",
        "summary",
        "what happened",
        "who is",
        "read",
        "inspect",
        "status",
        "progress",
        "recap",
        "tell me about",
        "what is",
    ]
    .iter()
    .any(|term| intent_lowered.contains(term))
        || [
            "总结",
            "概括",
            "讲了什么",
            "写了什么",
            "主角是谁",
            "是谁",
            "读取",
            "查看",
            "进度",
            "状态",
            "第几章",
            "多少章",
            "内容",
        ]
        .iter()
        .any(|term| intent_task.contains(term));
    let write_intent = [
        "write", "draft", "generate", "create", "continue", "revise", "edit", "export", "save",
    ]
    .iter()
    .any(|term| intent_lowered.contains(term))
        || [
            "写", "生成", "创作", "继续", "续写", "修订", "修改", "导出", "保存",
        ]
        .iter()
        .any(|term| intent_task.contains(term));
    read_intent && !write_intent
}

fn user_request_slice_for_readback(task: &str) -> &str {
    for marker in ["Full user request:", "Original user request:"] {
        let Some((_, tail)) = task.split_once(marker) else {
            continue;
        };
        let tail = tail.trim_start();
        if marker == "Original user request:" {
            if let Some((original, _)) = tail.split_once("\n\nDelegated task:") {
                return original.trim();
            }
        }
        return tail.trim();
    }
    task
}

pub(crate) const NOVEL_CHAPTER_UNIT_BANDS: [usize; 2] = [2500, 5000];

pub(crate) const fn step_target_chars() -> usize {
    NOVEL_CHAPTER_UNIT_BANDS[0]
}

pub(crate) const fn normal_body_range() -> (usize, usize) {
    (
        NOVEL_CHAPTER_UNIT_BANDS[0],
        NOVEL_CHAPTER_UNIT_BANDS[NOVEL_CHAPTER_UNIT_BANDS.len() - 1],
    )
}

pub(crate) const fn long_chapter_unit_range() -> (usize, usize) {
    normal_body_range()
}

pub(crate) const fn novel_chapter_unit_bands() -> [usize; 2] {
    NOVEL_CHAPTER_UNIT_BANDS
}

pub(crate) fn novel_chapter_unit_band_label() -> String {
    NOVEL_CHAPTER_UNIT_BANDS
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(" / ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FictionGenreProfile {
    Fantasy,
    Romance,
    ScienceFiction,
    Xianxia,
    Mystery,
    General,
}

impl FictionGenreProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fantasy => "fantasy",
            Self::Romance => "romance",
            Self::ScienceFiction => "science_fiction",
            Self::Xianxia => "xianxia",
            Self::Mystery => "mystery",
            Self::General => "general",
        }
    }
}

pub(crate) fn fiction_contract_field_requirements(genre: &str) -> BTreeMap<String, String> {
    let mut requirements = BTreeMap::new();
    for key in [
        "emotional_contract",
        "relief_beats",
        "relationship_ledger",
        "scene_type_mix",
        "character_voice_ledger",
        "reader_promise",
        "chapter_ending_rotation",
        "conflict_pressure_curve",
        "motif_ledger",
        "reveal_schedule",
        "relationship_interaction_quotas",
        "chapter_execution_contract",
        "payoff_matrix",
        "narration_contract",
        "time_model",
        "antagonist_pressure",
    ] {
        requirements.insert(key.to_string(), "strong".to_string());
    }
    for key in ["resource_economy", "social_order", "geography_model"] {
        requirements.insert(key.to_string(), "default".to_string());
    }

    let profile = fiction_genre_profile(genre, Some(genre));
    requirements.insert(
        "power_progression".to_string(),
        if matches!(
            profile,
            FictionGenreProfile::Fantasy
                | FictionGenreProfile::Xianxia
                | FictionGenreProfile::ScienceFiction
        ) {
            "genre_strong"
        } else {
            "genre_default"
        }
        .to_string(),
    );
    let lowered = genre.to_ascii_lowercase();
    requirements.insert(
        "artifact_ledger".to_string(),
        if ["悬疑", "推理", "侦探", "探案", "mystery", "detective"]
            .iter()
            .any(|term| genre.contains(term) || lowered.contains(term))
        {
            "genre_strong"
        } else {
            "genre_default"
        }
        .to_string(),
    );
    requirements
}

pub(crate) fn fiction_relief_beat_guidance(genre: &str) -> String {
    match fiction_genre_profile(genre, Some(genre)) {
        FictionGenreProfile::Romance => {
            "用符合人物关系的轻松互动或生活片段缓冲压力，并服务关系推进。".to_string()
        }
        FictionGenreProfile::ScienceFiction => {
            "用符合设定的技术日常、身份反差或队友互动缓冲压力，并服务世界可信度。".to_string()
        }
        FictionGenreProfile::Fantasy | FictionGenreProfile::Xianxia => {
            "用符合世界规则的人物互动、旅途日常或能力反差缓冲压力，并服务设定显影。".to_string()
        }
        FictionGenreProfile::Mystery => {
            "用人物互动、线索误读后的短暂松弛或调查日常缓冲压力，同时保持谜面推进。".to_string()
        }
        FictionGenreProfile::General => {
            "安排符合当前题材和人物气质的轻松、反差或日常片段，让高压情节有呼吸感。".to_string()
        }
    }
}

pub(crate) fn fiction_genre_signal_present(message: &str) -> bool {
    looks_like_fiction_genre_surface(message)
        || ["题材", "类型", "风格", "genre", "fiction", "story"]
            .iter()
            .any(|term| message.contains(term))
}

pub(crate) fn looks_like_fiction_genre_surface(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    fiction_genre_terms()
        .iter()
        .any(|term| value.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub(crate) fn fiction_genre_profile(task: &str, value: Option<&str>) -> FictionGenreProfile {
    let merged = match value {
        Some(value) if !value.trim().is_empty() => format!("{task}\n{value}"),
        _ => task.to_string(),
    };
    let lowered = merged.to_ascii_lowercase();
    if merged.contains("言情")
        || merged.contains("爱情")
        || merged.contains("恋爱")
        || lowered.contains("romance")
    {
        FictionGenreProfile::Romance
    } else if merged.contains("科幻")
        || merged.contains("星际")
        || merged.contains("太空")
        || lowered.contains("sci-fi")
        || lowered.contains("science fiction")
        || lowered.contains("space")
    {
        FictionGenreProfile::ScienceFiction
    } else if merged.contains("仙侠") || merged.contains("修仙") || lowered.contains("xianxia")
    {
        FictionGenreProfile::Xianxia
    } else if merged.contains("悬疑")
        || merged.contains("推理")
        || merged.contains("侦探")
        || lowered.contains("mystery")
        || lowered.contains("detective")
    {
        FictionGenreProfile::Mystery
    } else if merged.contains("玄幻")
        || merged.contains("修炼")
        || merged.contains("灵脉")
        || lowered.contains("fantasy")
        || lowered.contains("xuanhuan")
    {
        FictionGenreProfile::Fantasy
    } else {
        FictionGenreProfile::General
    }
}

fn fiction_genre_terms() -> &'static [&'static str] {
    &[
        "玄幻",
        "科幻",
        "仙侠",
        "修仙",
        "言情",
        "爱情",
        "恋爱",
        "悬疑",
        "都市",
        "历史",
        "奇幻",
        "武侠",
        "校园",
        "fantasy",
        "romance",
        "sci-fi",
        "science fiction",
        "xianxia",
    ]
}

pub(crate) fn nearest_novel_chapter_unit_band(requested: usize) -> usize {
    let bands = novel_chapter_unit_bands();
    bands
        .into_iter()
        .min_by_key(|band| {
            let distance = requested.abs_diff(*band);
            (distance, *band)
        })
        .unwrap_or(step_target_chars())
}

pub(crate) fn normalize_user_chapter_unit_target(requested: Option<usize>) -> Option<usize> {
    requested
        .filter(|value| *value > 0)
        .map(nearest_novel_chapter_unit_band)
}

pub(crate) fn dynamic_chapter_unit_target(target_units: Option<usize>) -> usize {
    let Some(target_units) = target_units.filter(|value| *value > 0) else {
        return step_target_chars();
    };
    let (min_units, max_units) = long_chapter_unit_range();
    let default_chapters = std::env::var("BENSHU_WRITING_DEFAULT_PROJECT_CHAPTERS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(80);
    let natural_target = target_units.div_ceil(default_chapters);
    if target_units <= normal_body_range().1 * 8 {
        natural_target.clamp(normal_body_range().0, max_units)
    } else {
        natural_target.clamp(min_units, max_units)
    }
}

pub(crate) fn normalize_chapter_unit_target(
    requested: Option<usize>,
    target_units: Option<usize>,
) -> Option<usize> {
    if let Some(value) = requested.filter(|value| *value > 0) {
        return Some(nearest_novel_chapter_unit_band(value));
    }

    target_units
        .filter(|value| *value > 0)
        .map(|_| dynamic_chapter_unit_target(target_units))
}

pub(crate) const fn recovery_body_range() -> (usize, usize) {
    (600, 1000)
}

pub(crate) fn previous_error_requests_smaller_step(error: &str) -> bool {
    let lowered = error.to_ascii_lowercase();
    lowered.contains("exceeded")
        || lowered.contains("timeout")
        || lowered.contains("timed out")
        || lowered.contains("too little body")
        || lowered.contains("truncated")
        || lowered.contains("missing continuity note")
        || lowered.contains("next hook")
        || lowered.contains("likely malformed")
        || error.contains("超时")
        || error.contains("截断")
        || error.contains("正文太短")
        || error.contains("输出太短")
        || error.contains("缺少连续性")
        || error.contains("下一步钩子")
        || error.contains("文本异常")
}

pub(crate) fn retry_title_reuse_guidance(error: &str) -> Option<String> {
    if !error.contains("already used by a prior generated artifact")
        && !error.contains("已被之前的产物使用")
    {
        return None;
    }

    let rejected_title = error
        .split_once("title '")
        .and_then(|(_, tail)| tail.split_once('\'').map(|(title, _)| title.trim()))
        .filter(|title| !title.is_empty());
    let forbidden = rejected_title
        .map(|title| format!("；禁止再次使用标题“{title}”"))
        .unwrap_or_default();
    Some(format!(
        "如果原因提到标题已被历史产物使用，本次必须自行创造一个全新的标题{forbidden}，并让文档元数据、正文标题和后续连续性记录全部继承这个新标题；不要复述被拒绝的标题。\n"
    ))
}

pub(crate) fn build_chapter_model_prompt(
    index: usize,
    total: usize,
    seeded_identity: bool,
) -> String {
    let identity_contract = if index == 1 && !seeded_identity {
        "             - 这是产物第一步：必须先输出产物身份块。\n\
         - 如果用户没有指定标题，必须自行命名；标题必须来自本次任务推理，不能复用任何代码示例或历史固定名称。\n\
         - 产物身份块必须包含：# 《标题》、产物类型、主角/主体/核心对象、目标规模、素材来源使用边界、连续性规则、当前进度。\n"
    } else {
        "             - 必须继承契约/前文已经建立的产物标题、主角/主体/核心对象、世界规则/论证框架、语气和主线，不得另起标题，不得更换主角/主体。\n"
    };
    format!(
        "为长文档产物生成第 {index} / {total} 个连续正文步骤。\n\
         必须遵守：\n\
         - 使用与用户请求一致的语言。\n\
         - 输出一个完整正文步骤，而不是大纲。\n\
         - 当前只能输出第 {index} / {total} 个正文步骤；不要提前输出后续步骤、后续章节或后续 section。\n\
{identity_contract}\
         - 如果是小说/剧本/故事，正文步骤必须包含标题、场景、冲突、行动、代价、收束和下一步钩子。\n\
         - 如果是报告/资料/方案，正文步骤必须包含小标题、事实或推理展开、阶段结论和下一步衔接。\n\
         - 从已有文件内容、上一步摘要和当前任务里继承标题、核心规则、语气、主线目标和已建立设定。\n\
         - 必须实际承接并推进上一步“下一步钩子”，不能换词重复同一个悬念或绕开未解决事件。\n\
         - 正文控制在一个可 checkpoint 的短片段内，约 {} 到 {} 个目标语言字符/字词单位；不要为了铺陈牺牲末尾结构。\n\
         - 输出前必须在内部完成一次可读性自检：修正明显错别字、近音错词、生造词、语义不通的短语和重复病句；不要把自检过程写出来。\n\
         - 最后两个非空段落：中文分别以“连续性记录：”和“下一步钩子：”开头；其他语言分别使用“Continuity Notes:”和“Next Hook:”。\n\
         - 不要使用任何代码内置示例名词、固定标题、固定角色、固定世界观或固定章节名。\n\
         - 不复刻任何真实小说的具体情节、角色或专有设定。\n\
         - 如果上一步摘要里有上一章结尾，必须承接上一章。\n\
         建议格式：\n\
         ### 第{index}步/第{index}章 标题\n\n\
         正文……\n\n\
         连续性记录：……\n\n\
         下一步钩子：……",
        normal_body_range().0,
        normal_body_range().1
    )
}

pub(crate) fn continuous_step_output_token_budget(request: &ContinuousStepRequest) -> u64 {
    let recovery_sized_step = request
        .previous_error
        .as_deref()
        .map(previous_error_requests_smaller_step)
        .unwrap_or(false);
    let body_max = if recovery_sized_step {
        recovery_body_range().1
    } else {
        normal_body_range().1
    } as u64;
    let target_chars = request
        .contract
        .as_ref()
        .and_then(|contract| {
            contract
                .anchors
                .iter()
                .find(|anchor| anchor.name == "step_target_chars")
                .and_then(|anchor| anchor.value.parse::<u64>().ok())
        })
        .filter(|value| *value > 0)
        .unwrap_or_else(|| step_target_chars() as u64);
    let structural_overhead = if request.step.index == 1 { 900 } else { 420 };
    let requested = body_max.max(target_chars) + structural_overhead;
    if recovery_sized_step {
        requested.clamp(512, 1_200)
    } else if request.step.index == 1 {
        requested.clamp(1_024, 3_200)
    } else {
        requested.clamp(768, 2_800)
    }
}

pub(crate) fn build_continuous_step_prompt(task: &str, request: &ContinuousStepRequest) -> String {
    let previous = request
        .previous_summary
        .as_deref()
        .filter(|summary| !summary.trim().is_empty())
        .unwrap_or("无。");
    let recent_checkpoints = if request.recent_checkpoint_summaries.is_empty() {
        "无。".to_string()
    } else {
        request.recent_checkpoint_summaries.join("\n")
    };
    let previous_error = request
        .previous_error
        .as_deref()
        .filter(|error| !error.trim().is_empty());
    let recovery_sized_step = previous_error
        .map(previous_error_requests_smaller_step)
        .unwrap_or(false);
    let (body_min, body_max) = if recovery_sized_step {
        recovery_body_range()
    } else {
        normal_body_range()
    };
    let retry_feedback = previous_error
        .map(|error| {
            let recovery_guidance = if previous_error_requests_smaller_step(error) {
                format!(
                    "\n\
                     上一次失败说明当前 chunk 对运行时预算过大或输出被截断。本次必须改为恢复型微步骤：正文控制在约 {body_min} 到 {body_max} 个中文字符，只推进一个最小事件/小节，先保证可落盘、连续性记录和下一步钩子完整。"
                )
            } else {
                String::new()
            };
            let title_guidance = retry_title_reuse_guidance(error).unwrap_or_default();
            format!(
                "这是当前 step 的第 {} 次重试。上一次输出被连续任务校验拒绝，原因：{}{}\n\
                 本次必须修正这个问题，并保持既有目标、身份锚点和最近 checkpoint 一致。\n\
                 如果原因提到未承接上一钩子，必须在正文开头的场景/行动中明确处理最近 checkpoint 的 next_hook，不要换场景绕开，也不要只在连续性记录里提到。\n\
                 {}\
                 如果原因提到缺少连续性记录、下一步钩子、正文太短、超时或疑似截断，本次必须缩短正文，先保证末尾两个收束字段完整出现。\n\
                 这段校验/重试反馈只供内部纠偏；不得把校验、重试、错误原因、漂移、修正等过程诊断写入产物正文、元数据、连续性记录或下一步钩子。",
                request.attempt, error, recovery_guidance, title_guidance
            )
        })
        .unwrap_or_else(|| "无。".to_string());
    let contract = render_continuous_contract(request.contract.as_ref());
    let expected = request
        .step
        .expected_output
        .as_deref()
        .unwrap_or("完成本步骤并返回可落盘的结果。");
    let planned_total_steps = request
        .contract
        .as_ref()
        .and_then(|contract| {
            contract
                .anchors
                .iter()
                .find(|anchor| anchor.name == "planned_total_steps")
                .and_then(|anchor| anchor.value.parse::<usize>().ok())
        })
        .unwrap_or(request.step.index);
    let recovery_output_contract = render_step_recovery_output_contract(
        request,
        body_min,
        body_max,
        planned_total_steps,
        previous_error.is_some(),
    );
    format!(
        "你正在执行一个可恢复的连续任务中的“纯文本产物生成 step”。请只完成当前 step，不要跳到后续步骤。\n\n\
         任务ID：{}\n\
         当前步骤序号：{}\n\
         步骤标签：{}\n\
         步骤说明：{}\n\
         总目标：{}\n\
         连续任务契约：\n{}\n\
         期望输出：{}\n\
         上一步摘要：{}\n\
         最近 checkpoint：\n{}\n\
         校验/重试反馈：{}\n\n\
         当前 step 的具体任务：{}\n\n\
         输出要求：\n\
         {}\
         - 直接返回本 step 的最终内容。\n\
         - 不要只给计划。\n\
         - 不要说“我将会”。\n\
         - 不要调用、模拟、输出任何工具调用或 Tool Result。\n\
         - 如果当前任务提到工具、worker、delegate、落盘、checkpoint，只把它理解为外层执行器已经处理；你只负责生成本 step 的正文产物。\n\
         - 如果是写作/报告/导入摘要，请输出可直接保存的正文。\n\
         - 如果这是当前产物的第一个正文 step，必须先建立产物身份：自拟非硬编码标题、产物类型、主角/主体/核心对象、目标规模、来源使用边界、连续性规则、当前进度；之后再输出正文。\n\
         - 当前只能输出 step {}/{} 对应的正文片段；不要提前输出后续 step、后续章节或后续 section。\n\
         - 如果已有标题或产物身份，必须继承标题、主角/主体/核心对象和核心规则，不要重命名或漂移。\n\
         - 除第一个正文 step 外，不要再次输出 [Document Metadata]、文档元数据、标题、类型、目标规模、当前进度等产物身份块；直接输出本 step 的章节/段落标题和正文。\n\
         - 单个 step 必须是可快速 checkpoint 的 bounded chunk；写足当前步骤，但不要把多个步骤合并成长篇输出。\n\
         - 本 step 使用固定结构：章节/段落标题、正文、连续性记录、下一步钩子。正文只推进一个局部事件或一个论证小节。\n\
         - 为了确保可恢复，正文部分控制在约 {} 到 {} 个中文字符；如果模型输出预算紧张，优先保留结构完整性和尾部字段。\n\
         - 输出前必须在内部完成一次可读性自检：修正明显错别字、近音错词、生造词、语义不通的短语和重复病句；不要把自检过程写出来。\n\
         - 每个 step 末尾必须保留连续性记录和下一步钩子；如果篇幅紧张，优先压缩正文，不得省略这两个收束字段。\n\
         - 最后两个非空段落必须分别以“连续性记录：”和“下一步钩子：”开头。\n\
         - 不要把 retry、校验、guard、错误原因、内部修正说明、执行器状态或 checkpoint 机制写进最终产物内容。",
        request.task_id,
        request.step.index,
        request.step.label,
        request.step.instruction,
        request.objective,
        contract,
        expected,
        previous,
        recent_checkpoints,
        retry_feedback,
        task,
        recovery_output_contract,
        request.step.index,
        planned_total_steps,
        body_min,
        body_max
    )
}

pub(crate) fn build_empty_step_recovery_prompt(
    task: &str,
    request: &ContinuousStepRequest,
) -> String {
    let previous = request
        .previous_summary
        .as_deref()
        .filter(|summary| !summary.trim().is_empty())
        .unwrap_or("无。");
    let expected = request
        .step
        .expected_output
        .as_deref()
        .unwrap_or("完成本步骤并返回可落盘的正文内容。");
    let planned_total_steps = request
        .contract
        .as_ref()
        .and_then(|contract| {
            contract
                .anchors
                .iter()
                .find(|anchor| anchor.name == "planned_total_steps")
                .map(|anchor| anchor.value.as_str())
        })
        .unwrap_or("?");
    let (body_min, body_max) = recovery_body_range();
    let recovery_output_contract =
        render_step_recovery_output_contract(request, body_min, body_max, 0, true);
    format!(
        "上一次连续任务 step 返回了空文本。请执行一次极简恢复：只生成当前 step 的可落盘正文，不解释，不输出计划，不调用工具。\n\n\
         任务ID：{}\n\
         当前步骤：{} / {}\n\
         步骤标签：{}\n\
         步骤说明：{}\n\
         总目标：{}\n\
         上一步摘要：{}\n\
         期望输出：{}\n\
         当前 step 任务：{}\n\n\
         输出要求：\n\
         {}\
         - 直接输出可追加到产物文件的正文。\n\
         - 必须继承已有标题、主角/主体/核心对象和连续性规则。\n\
         - 不能返回空文本、标题-only、元信息-only、计划-only、道歉或无法完成说明。\n\
         - 最后两个非空段落必须分别以“连续性记录：”和“下一步钩子：”开头。",
        request.task_id,
        request.step.index,
        planned_total_steps,
        request.step.label,
        request.step.instruction,
        request.objective,
        previous,
        expected,
        task,
        recovery_output_contract
    )
}

fn render_step_recovery_output_contract(
    request: &ContinuousStepRequest,
    body_min: usize,
    body_max: usize,
    planned_total_steps: usize,
    active: bool,
) -> String {
    if !active {
        return String::new();
    }
    let total = if planned_total_steps == 0 {
        request
            .contract
            .as_ref()
            .and_then(|contract| {
                contract
                    .anchors
                    .iter()
                    .find(|anchor| anchor.name == "planned_total_steps")
                    .and_then(|anchor| anchor.value.parse::<usize>().ok())
            })
            .unwrap_or(request.step.index)
    } else {
        planned_total_steps
    };
    let first_step_identity = if request.step.index == 1 {
        "- 当前是第一个正文 step，必须先建立产物身份块：标题、产物类型、主角/主体/核心对象、目标规模、来源使用边界、连续性规则、当前进度；然后输出正文。\n"
    } else {
        "- 当前不是第一个正文 step，不要重新命名产物，不要重复输出文档元数据；只输出本 step 的标题、正文和尾部字段。\n"
    };
    format!(
        "- 本次是恢复型输出，唯一目标是产生一个可 checkpoint 的最小完整块。\n\
         - 不能只输出标题、目录、元信息、计划、摘要、错误说明或执行器状态。\n\
         {first_step_identity}\
         - 必须至少包含：章节/段落标题、正文段落、连续性记录、下一步钩子。\n\
         - 正文合计控制在约 {body_min} 到 {body_max} 个中文字符；如果空间紧张，压缩正文但保留尾部字段。\n\
         - 当前进度如需声明，必须是 {}/{}，不能声明其它步骤。\n\
         - 如果最近 checkpoint 里有未解决的下一步钩子，正文第一段必须承接并推进它。\n",
        request.step.index, total
    )
}

fn render_continuous_contract(contract: Option<&ContinuousTaskContract>) -> String {
    let Some(contract) = contract else {
        return "- 未提供显式契约；以总目标、步骤说明和最近 checkpoint 为准。".to_string();
    };
    let mut lines = Vec::new();
    if contract.invariants.is_empty()
        && contract.anchors.is_empty()
        && contract.completion_criteria.is_empty()
    {
        return "- 未提供显式契约；以总目标、步骤说明和最近 checkpoint 为准。".to_string();
    }
    if !contract.invariants.is_empty() {
        lines.push("不可漂移约束：".to_string());
        lines.extend(
            contract
                .invariants
                .iter()
                .map(|invariant| format!("- {invariant}")),
        );
    }
    if !contract.anchors.is_empty() {
        lines.push("身份/事实锚点：".to_string());
        lines.extend(
            contract
                .anchors
                .iter()
                .map(|anchor| format!("- {}: {}", anchor.name, anchor.value)),
        );
    }
    if !contract.completion_criteria.is_empty() {
        lines.push("完成条件：".to_string());
        lines.extend(
            contract
                .completion_criteria
                .iter()
                .map(|criterion| format!("- {criterion}")),
        );
    }
    lines.join("\n")
}

pub(crate) fn summarize_step_output(output: &str, fallback_label: &str) -> String {
    let first_line = output
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or(fallback_label);
    let mut parts = vec![ellipsize(first_line, 180)];
    if let Some(continuity) = extract_continuity_record_text(output) {
        parts.push(format!("continuity: {}", ellipsize(&continuity, 260)));
    }
    if let Some(next_hook) = LongformArtifactGuard::extract_next_hook_text(output) {
        parts.push(format!("next_hook: {}", ellipsize(&next_hook, 260)));
    }
    parts.join(" | ")
}

pub(crate) fn extract_continuity_record_text(output: &str) -> Option<String> {
    let mut collecting = false;
    let mut parts = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if collecting {
            if trimmed.starts_with("下一步钩子")
                || trimmed.starts_with("下一章钩子")
                || trimmed.starts_with("后续钩子")
                || trimmed.eq_ignore_ascii_case("next hook")
            {
                break;
            }
            if !trimmed.is_empty() {
                parts.push(trimmed.trim_start_matches(['-', '*', '•', ' ']).to_string());
            }
            continue;
        }
        if trimmed.starts_with("连续性记录")
            || trimmed.starts_with("连续性说明")
            || trimmed.eq_ignore_ascii_case("continuity notes")
        {
            collecting = true;
            if let Some((_, value)) = trimmed.split_once('：').or_else(|| trimmed.split_once(':'))
            {
                let value = value.trim();
                if !value.is_empty() {
                    parts.push(value.to_string());
                }
            }
        }
    }
    let text = parts.join(" ");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_chapter_count_is_the_shared_positive_ceiling_rule() {
        assert_eq!(expected_chapter_count(100_000, 2_500), Some(40));
        assert_eq!(expected_chapter_count(1_000_000, 5_000), Some(200));
        assert_eq!(expected_chapter_count(1_000_001, 5_000), Some(201));
        assert_eq!(expected_chapter_count(1, 5_000), Some(1));
        assert_eq!(expected_chapter_count(0, 2_500), None);
        assert_eq!(expected_chapter_count(100_000, 0), None);
    }

    #[test]
    fn writing_a_novel_is_governed_even_without_explicit_chapter_wording() {
        let tools = vec!["writing".to_string()];

        assert!(should_route_writer_fiction_to_novel_studio(
            &tools,
            "帮我写一个草根逆袭的玄幻小说。",
            |_| None,
            step_target_chars(),
        ));
    }

    #[test]
    fn progress_reply_format_does_not_make_continuation_read_only() {
        let tools = vec!["writing".to_string()];
        let task = "继续《长歌记》写作任务，从当前项目状态继续写到目标约10万字完成。项目路径 data/generated/novels/长歌记。正文保存到项目文件和txt导出，聊天框只返回进度、章节号、字数、文件路径和简短摘要。";

        assert!(!task_requests_fiction_project_readback(task));
        assert!(should_route_writer_fiction_to_novel_studio(
            &tools,
            task,
            |_| Some(100_000),
            step_target_chars(),
        ));
    }
}
