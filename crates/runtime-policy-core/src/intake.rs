use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreationIntakeDecision {
    pub action: CreationIntakeAction,
    pub artifact_kind: Option<String>,
    pub missing_slots: Vec<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreationIntakeAction {
    Proceed,
    Clarify,
}

impl CreationIntakeDecision {
    pub fn proceed() -> Self {
        Self {
            action: CreationIntakeAction::Proceed,
            artifact_kind: None,
            missing_slots: Vec::new(),
            prompt: None,
        }
    }

    pub fn should_clarify(&self) -> bool {
        self.action == CreationIntakeAction::Clarify
    }
}

#[derive(Debug, Clone)]
struct CreationArtifactProfile {
    kind: &'static str,
    surfaces: &'static [&'static str],
    minimum_slots: usize,
    questions_zh: &'static [&'static str],
    questions_en: &'static [&'static str],
}

const CREATION_VERBS: &[&str] = &[
    "写",
    "创作",
    "生成",
    "创建",
    "做",
    "制作",
    "起草",
    "设计",
    "开发",
    "实现",
    "产出",
    "整理",
    "write",
    "generate",
    "create",
    "make",
    "draft",
    "design",
    "build",
    "implement",
    "produce",
];

const AUTONOMY_SURFACES: &[&str] = &[
    "你自己定",
    "你来定",
    "你决定",
    "自行定夺",
    "自行决定",
    "自由发挥",
    "按你的判断",
    "帮我定",
    "自动补齐",
    "自己定",
    "自己决定",
    "you decide",
    "your call",
    "use your judgment",
    "surprise me",
    "fill in the details",
];

const PLANNING_DIALOGUE_SURFACES: &[&str] = &[
    "先不要写正文",
    "不要写正文",
    "先别写正文",
    "先不写正文",
    "先通过多轮对话",
    "先和我多轮对话",
    "多轮自然语言对话",
    "多轮对话定",
    "多轮对话确定",
    "多轮对话把",
    "定制小说大纲",
    "定制大纲",
    "定制创作大纲",
    "规划清楚",
    "先定大纲",
    "定下大纲",
    "先定框架",
    "定下框架",
    "把框架定下来",
    "框架定下来",
    "先把框架",
    "先定创作框架",
    "创作合同",
    "先理解需求",
    "问我还需要确认",
    "需要确认哪些",
    "先确认",
    "先讨论",
    "先聊清楚",
    "不要立刻开始",
    "don't write yet",
    "do not write yet",
    "do not start writing",
    "before writing",
    "clarify first",
    "outline first",
    "plan first",
    "ask me",
];

const SCALE_SURFACES: &[&str] = &[
    "字",
    "页",
    "章",
    "分钟",
    "小时",
    "万字",
    "千字",
    "短篇",
    "中篇",
    "长篇",
    "完整",
    "一章",
    "第一章",
    "word",
    "words",
    "page",
    "pages",
    "chapter",
    "chapters",
    "short",
    "long",
    "complete",
    "full",
];

const FORMAT_SURFACES: &[&str] = &[
    "txt", "pdf", "md", "markdown", "html", "docx", "文件", "文档", "导出", "保存",
];

const AUDIENCE_SURFACES: &[&str] = &[
    "给",
    "面向",
    "读者",
    "老师",
    "学生",
    "客户",
    "老板",
    "投资人",
    "用户",
    "audience",
    "reader",
    "for ",
];

const SOURCE_SURFACES: &[&str] = &[
    "根据",
    "基于",
    "参考",
    "材料",
    "素材",
    "知识库",
    "数据库",
    "论文",
    "网页",
    "文档",
    "according to",
    "based on",
    "source",
    "material",
    "reference",
];

const ADULT_CREATION_SURFACES: &[&str] = &[
    "色情",
    "情色",
    "成人",
    "成人向",
    "露骨",
    "性爱",
    "性描写",
    "18禁",
    "r18",
    "nsfw",
    "adult fiction",
    "adult novel",
    "adult content",
    "erotic",
    "explicit sex",
    "sexual",
    "porn",
];

const GRAPHIC_VIOLENCE_SURFACES: &[&str] = &[
    "血腥",
    "重口",
    "残虐",
    "虐杀",
    "肢解",
    "内脏",
    "暴力血腥",
    "graphic violence",
    "gore",
    "gory",
    "splatter",
];

const ADULT_AGE_CONFIRMATION_SURFACES: &[&str] = &[
    "我已满18",
    "我已满十八",
    "我已年满18",
    "我已年满十八",
    "本人已满18",
    "本人已满十八",
    "年满18周岁",
    "年满十八周岁",
    "确认已满18",
    "确认已满十八",
    "我是成年人",
    "成年人",
    "已成年",
    "已滿18",
    "已滿十八",
    "i am 18",
    "i'm 18",
    "i am over 18",
    "i'm over 18",
    "i am an adult",
];

const PROFILES: &[CreationArtifactProfile] = &[
    CreationArtifactProfile {
        kind: "fiction",
        surfaces: &[
            "小说",
            "故事",
            "章节",
            "正文",
            "novel",
            "fiction",
            "story",
        ],
        minimum_slots: 1,
        questions_zh: &[
            "想写什么题材的小说？比如都市玄幻、异界玄幻、科幻、言情；也可以说“你来定”。",
            "每章大概多少字？小说目前支持 2500 或 5000 两档。",
            "总字数大概多少？比如 5 万字、10 万字、50 万字；也可以让我按题材建议。",
        ],
        questions_en: &[
            "What fiction genre should it use, or should I decide?",
            "How long should each chapter be? Fiction currently supports 2500 or 5000 words/characters as configured bands.",
            "What total length should the project target, or should I propose one for the genre?",
        ],
    },
    CreationArtifactProfile {
        kind: "paper",
        surfaces: &["论文", "paper", "thesis", "article"],
        minimum_slots: 1,
        questions_zh: &[
            "论文主题、研究问题或领域是什么？也可以说“你来定”。",
            "需要学术论文、综述、课程论文，还是投稿风格？",
            "是否需要引用已有资料、知识库或外部检索证据？",
        ],
        questions_en: &[
            "What topic, research question, or field should the paper cover?",
            "Should it be a research paper, review, class essay, or publication-style draft?",
            "Should it cite supplied material, knowledge-base items, or external evidence?",
        ],
    },
    CreationArtifactProfile {
        kind: "report",
        surfaces: &["报告", "方案", "总结", "report", "proposal", "brief"],
        minimum_slots: 1,
        questions_zh: &[
            "报告主题和使用场景是什么？也可以说“你来定”。",
            "读者是谁，偏正式、商业、技术还是汇报风格？",
            "需要多长、什么格式、是否要引用材料？",
        ],
        questions_en: &[
            "What is the report topic and use case?",
            "Who is the audience, and should the tone be formal, business, technical, or briefing-style?",
            "How long and what output format should it use, and should it cite sources?",
        ],
    },
];

pub fn evaluate_creation_intake(request: &str) -> CreationIntakeDecision {
    let request = request.trim();
    if request.is_empty() {
        return CreationIntakeDecision::proceed();
    }

    let Some(profile) = creation_artifact_profile(request) else {
        return CreationIntakeDecision::proceed();
    };

    if creation_request_needs_adult_age_confirmation(request) {
        let chinese = has_cjk(request);
        let missing_slots = vec![label(chinese, "年龄确认", "adult age confirmation")];
        let prompt = render_adult_age_confirmation_prompt(chinese);
        return CreationIntakeDecision {
            action: CreationIntakeAction::Clarify,
            artifact_kind: Some(profile.kind.to_string()),
            missing_slots,
            prompt: Some(prompt),
        };
    }

    if contains_any(request, PLANNING_DIALOGUE_SURFACES) {
        if contains_any(request, AUTONOMY_SURFACES)
            || creation_slot_count(request, profile.kind) >= profile.minimum_slots
        {
            return CreationIntakeDecision::proceed();
        }

        let chinese = has_cjk(request);
        let missing_slots = missing_slot_labels(request, profile.kind, chinese);
        let prompt = render_creation_intake_prompt(profile, &missing_slots, chinese);
        return CreationIntakeDecision {
            action: CreationIntakeAction::Clarify,
            artifact_kind: Some(profile.kind.to_string()),
            missing_slots,
            prompt: Some(prompt),
        };
    }

    if !contains_any(request, CREATION_VERBS) {
        return CreationIntakeDecision::proceed();
    }
    if contains_any(request, AUTONOMY_SURFACES) {
        return CreationIntakeDecision::proceed();
    }

    if !has_concrete_topic_or_modifier(request, profile.kind) {
        let chinese = has_cjk(request);
        let missing_slots = missing_slot_labels(request, profile.kind, chinese);
        let prompt = render_creation_intake_prompt(profile, &missing_slots, chinese);
        return CreationIntakeDecision {
            action: CreationIntakeAction::Clarify,
            artifact_kind: Some(profile.kind.to_string()),
            missing_slots,
            prompt: Some(prompt),
        };
    }

    let slot_count = creation_slot_count(request, profile.kind);
    if slot_count >= profile.minimum_slots {
        return CreationIntakeDecision::proceed();
    }

    let chinese = has_cjk(request);
    let missing_slots = missing_slot_labels(request, profile.kind, chinese);
    let prompt = render_creation_intake_prompt(profile, &missing_slots, chinese);
    CreationIntakeDecision {
        action: CreationIntakeAction::Clarify,
        artifact_kind: Some(profile.kind.to_string()),
        missing_slots,
        prompt: Some(prompt),
    }
}

pub fn detect_creation_artifact_kind(request: &str) -> Option<String> {
    let request = request.trim();
    if request.is_empty() || !contains_any(request, CREATION_VERBS) {
        return None;
    }
    creation_artifact_profile(request).map(|profile| profile.kind.to_string())
}

fn creation_artifact_profile(request: &str) -> Option<&'static CreationArtifactProfile> {
    PROFILES
        .iter()
        .find(|profile| contains_any(request, profile.surfaces))
        .or_else(|| {
            fiction_book_request_has_genre_evidence(request)
                .then(|| PROFILES.iter().find(|profile| profile.kind == "fiction"))
                .flatten()
        })
}

fn fiction_book_request_has_genre_evidence(request: &str) -> bool {
    contains_any(
        request,
        &[
            "写一本",
            "创作一本",
            "生成一本",
            "创建一本",
            "写一部",
            "创作一部",
        ],
    ) && contains_any(
        request,
        &[
            "悬疑",
            "玄幻",
            "仙侠",
            "武侠",
            "科幻",
            "奇幻",
            "言情",
            "爱情",
            "恐怖",
            "惊悚",
            "推理",
            "冒险",
            "群像",
            "现实主义",
            "novel",
            "fiction",
        ],
    )
}

pub fn creation_request_needs_adult_age_confirmation(request: &str) -> bool {
    let request = request.trim();
    if request.is_empty() || adult_age_confirmation_present(request) {
        return false;
    }
    contains_any(request, ADULT_CREATION_SURFACES)
        || contains_any(request, GRAPHIC_VIOLENCE_SURFACES)
}

pub fn adult_age_confirmation_present(request: &str) -> bool {
    contains_any(request, ADULT_AGE_CONFIRMATION_SURFACES)
}

fn creation_slot_count(request: &str, kind: &str) -> usize {
    let mut count = 0;
    if has_concrete_topic_or_modifier(request, kind) {
        count += 1;
    }
    if contains_any(request, SCALE_SURFACES) || contains_digit(request) {
        count += 1;
    }
    if contains_any(request, FORMAT_SURFACES) {
        count += 1;
    }
    if contains_any(request, AUDIENCE_SURFACES) {
        count += 1;
    }
    if contains_any(request, SOURCE_SURFACES) {
        count += 1;
    }
    count
}

fn missing_slot_labels(request: &str, kind: &str, chinese: bool) -> Vec<String> {
    let mut slots = Vec::new();
    if !has_concrete_topic_or_modifier(request, kind) {
        slots.push(label(chinese, "主题/题材", "topic or premise"));
    }
    if !contains_any(request, SCALE_SURFACES) && !contains_digit(request) {
        slots.push(label(chinese, "规模/范围", "scope or length"));
    }
    if kind != "fiction" && !contains_any(request, FORMAT_SURFACES) {
        slots.push(label(chinese, "输出格式", "output format"));
    }
    slots
}

fn render_creation_intake_prompt(
    profile: &CreationArtifactProfile,
    missing_slots: &[String],
    chinese: bool,
) -> String {
    let missing = if missing_slots.is_empty() {
        if chinese {
            "关键信息".to_string()
        } else {
            "key details".to_string()
        }
    } else {
        missing_slots.join(if chinese { "、" } else { ", " })
    };
    let questions = if chinese {
        profile.questions_zh
    } else {
        profile.questions_en
    }
    .iter()
    .map(|question| format!("- {question}"))
    .collect::<Vec<_>>()
    .join("\n");

    if chinese {
        if profile.kind == "fiction" {
            format!(
                "可以。你想写什么题材的小说？每章字数请选择 2500 或 5000。总字数可以告诉我，比如 5万、10万、50万；如果不想定，也可以说“你来定”。\n\n\
当前还缺：{missing}。\n\n\
你可以这样回答：\n{questions}"
            )
        } else {
            format!(
                "可以。你只要继续用自然语言补一句方向就行，我会自动补齐创作合同，不需要你填写系统字段。\n\n\
当前最好再说明：{missing}。\n\n\
你可以任选一种说法：\n{questions}\n- 或者直接说“你来定”，我会自行生成主题、结构和写作参数。"
            )
        }
    } else {
        format!(
            "Yes. Add one natural-language direction and I will fill the creation contract automatically; you do not need to fill system fields.\n\n\
It would help to clarify: {missing}.\n\n\
You can answer in any of these ways:\n{questions}\n- Or just say \"you decide\" and I will generate the premise, outline, ending, and writing parameters."
        )
    }
}

fn render_adult_age_confirmation_prompt(chinese: bool) -> String {
    if chinese {
        "这类创作可能包含成人向、强烈暴力或血腥内容。开始生成合同或正文前，请先确认你已年满十八周岁。\n\n请用自然语言回复确认，例如：“我已年满十八周岁，继续按这个方向写。”确认前我不会开始生成这类内容。".to_string()
    } else {
        "This request may involve adult, explicit, graphic, or violent content. Before I generate a contract or prose for it, please confirm that you are at least 18 years old.\n\nYou can reply naturally, for example: \"I am over 18; continue with this direction.\" I will not start generating this content before that confirmation.".to_string()
    }
}

fn has_concrete_topic_or_modifier(request: &str, kind: &str) -> bool {
    let compact = request
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let normalized_compact = compact.to_ascii_lowercase();
    let topic_markers = [
        "关于",
        "主题",
        "题材",
        "类型",
        "风格",
        "玄幻",
        "仙侠",
        "科幻",
        "言情",
        "悬疑",
        "草根",
        "逆袭",
        "心脏病",
        "治疗",
        "北京",
        "广州",
        "比特币",
        "topic",
        "about",
        "genre",
        "style",
    ];
    if contains_any(&normalized_compact, &topic_markers) {
        return true;
    }
    let surfaces = PROFILES
        .iter()
        .find(|profile| profile.kind == kind)
        .map(|profile| profile.surfaces)
        .unwrap_or(&[]);
    let without_verbs = strip_known_terms(&normalized_compact, CREATION_VERBS);
    let without_artifact = strip_known_terms(&without_verbs, surfaces);
    let without_scale = strip_known_terms(&without_artifact, SCALE_SURFACES);
    let without_fillers = strip_topic_fillers(&without_scale);
    without_fillers.chars().count() >= if has_cjk(request) { 4 } else { 12 }
}

fn strip_known_terms(value: &str, terms: &[&str]) -> String {
    let mut out = value.to_string();
    for term in terms {
        out = out.replace(term, "");
    }
    out
}

fn strip_topic_fillers(value: &str) -> String {
    let mut out = value.to_string();
    for filler in [
        "帮我",
        "给我",
        "请",
        "麻烦",
        "一下",
        "一个",
        "一份",
        "一篇",
        "先",
        "和我",
        "跟我",
        "多轮",
        "对话",
        "自然语言",
        "正文",
        "框架",
        "大纲",
        "合同",
        "help me",
        "please",
        "clarify",
        "outline",
        "framework",
        "contract",
    ] {
        out = out.replace(filler, "");
    }
    out.chars()
        .filter(|ch| {
            !ch.is_ascii_digit()
                && !matches!(
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
                        | '几'
                        | '个'
                        | '篇'
                        | '份'
                        | '页'
                        | '部'
                        | '本'
                        | '段'
                        | '条'
                )
        })
        .collect()
}

fn label(chinese: bool, zh: &str, en: &str) -> String {
    if chinese {
        zh.to_string()
    } else {
        en.to_string()
    }
}

fn contains_digit(value: &str) -> bool {
    value.chars().any(|ch| ch.is_ascii_digit())
}

fn contains_any(value: &str, terms: &[&str]) -> bool {
    let lowered = value.to_ascii_lowercase();
    terms.iter().any(|term| {
        let term_lowered = term.to_ascii_lowercase();
        value.contains(term) || lowered.contains(&term_lowered)
    })
}

fn has_cjk(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thin_creation_requests_are_clarified() {
        let decision = evaluate_creation_intake("帮我写小说");
        assert!(decision.should_clarify());
        assert_eq!(decision.artifact_kind.as_deref(), Some("fiction"));
        let prompt = decision.prompt.unwrap();
        assert!(prompt.contains("写什么题材的小说"), "{prompt}");
        assert!(prompt.contains("每章字数请选择 2500 或 5000"), "{prompt}");
        assert!(prompt.contains("总字数"), "{prompt}");
        assert!(prompt.contains("你来定"), "{prompt}");

        let paper = evaluate_creation_intake("帮我写论文");
        assert!(paper.should_clarify());
        assert_eq!(paper.artifact_kind.as_deref(), Some("paper"));
        assert!(evaluate_creation_intake("帮我写一篇10页论文").should_clarify());
    }

    #[test]
    fn specified_or_autonomous_creation_requests_proceed() {
        assert!(!evaluate_creation_intake("帮我写一个草根逆袭的玄幻小说").should_clarify());
        assert!(!evaluate_creation_intake("帮我写小说，你来定").should_clarify());
        assert!(
            !evaluate_creation_intake("请创作一本10万字的都市言情长篇小说，每章2500字")
                .should_clarify()
        );
        assert_eq!(
            detect_creation_artifact_kind("请创作一本10万字的都市言情长篇小说，每章2500字")
                .as_deref(),
            Some("fiction")
        );
        assert!(!evaluate_creation_intake("写一个 React 登录页面代码").should_clarify());
        assert!(!evaluate_creation_intake("帮我做PPT").should_clarify());
    }

    #[test]
    fn adult_or_graphic_fiction_requires_age_confirmation_before_contract() {
        let erotic = evaluate_creation_intake("帮我写一部都市色情小说，每章2500字");
        assert!(erotic.should_clarify());
        assert_eq!(erotic.artifact_kind.as_deref(), Some("fiction"));
        let prompt = erotic.prompt.unwrap();
        assert!(prompt.contains("年满十八周岁"), "{prompt}");

        let graphic = evaluate_creation_intake("写一部暴力血腥小说，每章2500字，5万字");
        assert!(graphic.should_clarify());
        assert!(graphic
            .missing_slots
            .iter()
            .any(|slot| slot.contains("年龄")));
    }

    #[test]
    fn adult_or_graphic_fiction_proceeds_after_age_confirmation() {
        let decision =
            evaluate_creation_intake("我已年满十八周岁，写一部成人向悬疑小说，每章2500字，5万字");
        assert!(!decision.should_clarify());

        let english = evaluate_creation_intake("I am over 18; write an erotic noir story.");
        assert!(!english.should_clarify());
    }

    #[test]
    fn creation_artifact_kind_detection_survives_proceeding_requests() {
        assert_eq!(
            detect_creation_artifact_kind("帮我写一个草根逆袭的玄幻小说").as_deref(),
            Some("fiction")
        );
        assert_eq!(
            detect_creation_artifact_kind("帮我写小说，你来定").as_deref(),
            Some("fiction")
        );
        assert_eq!(
            detect_creation_artifact_kind("写一个 React 登录页面代码"),
            None
        );
        assert_eq!(
            detect_creation_artifact_kind("请写一本 Rust 异步编程入门教程"),
            None
        );
        assert_eq!(
            detect_creation_artifact_kind(
                "请写一本发生在老玻璃厂的工业悬疑长篇，总字数10万字。先建立创作合同，确认后再写完整本书。"
            )
            .as_deref(),
            Some("fiction")
        );
    }

    #[test]
    fn thin_planning_dialogue_requests_are_clarified_before_writing() {
        let decision = evaluate_creation_intake("先和我多轮对话，帮我写小说");

        assert!(decision.should_clarify());
        assert_eq!(decision.artifact_kind.as_deref(), Some("fiction"));
        assert!(decision
            .missing_slots
            .iter()
            .any(|slot| slot.contains("主题") || slot.contains("规模")));
    }

    #[test]
    fn specified_planning_dialogue_requests_proceed_to_contract_generation() {
        let decision = evaluate_creation_intake(
            "我们先不要写正文，先通过多轮对话定下一部草根逆袭科幻玄幻小说的大纲和创作合同，每章3000字，一共50万字",
        );

        assert!(!decision.should_clarify());

        let demand_style = evaluate_creation_intake(
            "我要一部草根逆袭的科幻玄幻小说，每章3000字，一共50万字，先通过多轮对话定下大纲和创作合同",
        );
        assert!(!demand_style.should_clarify());

        let framework_style = evaluate_creation_intake(
            "写一部5万字的短篇爱情小说，每章2500字。先和我多轮对话把框架定下来，情感要细腻，有完整结尾。",
        );
        assert!(!framework_style.should_clarify());

        let custom_outline_style = evaluate_creation_intake(
            "跟我多轮自然语言对话，定制小说大纲，写一篇异世界重生玄幻小说，2500字每章，写5万字，规划清楚结局后再写完整。",
        );
        assert!(!custom_outline_style.should_clarify());
    }
}
