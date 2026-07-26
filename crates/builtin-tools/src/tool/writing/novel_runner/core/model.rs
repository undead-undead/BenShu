use serde::{Deserialize, Serialize};

use crate::tool::writing::novel_bible::ChapterStateChange;
use crate::tool::writing::novel_contract_v2::ChapterCharacterRequest;

pub(super) const ZH_MEMO_SECTIONS: &[&str] = &[
    "当前任务",
    "本章目标",
    "该兑现",
    "暂不掀",
    "日常过渡功能",
    "关键抉择三连问",
    "章尾必须发生的改变",
    "不要做",
];

pub(super) const EN_MEMO_SECTIONS: &[&str] = &[
    "Current Task",
    "Chapter Goal",
    "Pay Off",
    "Do Not Reveal Yet",
    "Everyday Transition Function",
    "Decision Checks",
    "Required End-State Change",
    "Do Not",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChapterMemo {
    pub goal: String,
    pub body: String,
    pub sections: Vec<MemoSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChapterExecutionPackage {
    pub memo: ChapterMemo,
    pub architecture: String,
    #[serde(default)]
    pub scene_goal: String,
    #[serde(default)]
    pub conflict: String,
    #[serde(default)]
    pub choice: String,
    #[serde(default)]
    pub cost: String,
    #[serde(default)]
    pub reveal: String,
    #[serde(default)]
    pub emotional_beat: String,
    #[serde(default)]
    pub chapter_function: String,
    #[serde(default)]
    pub irreversible_event: String,
    #[serde(default)]
    pub new_state_after_chapter: String,
    #[serde(default)]
    pub character_change: String,
    #[serde(default)]
    pub relationship_change: String,
    #[serde(default)]
    pub power_delta: String,
    #[serde(default)]
    pub resource_delta: String,
    #[serde(default)]
    pub hook_opened: Vec<String>,
    #[serde(default)]
    pub hook_paid_off: Vec<String>,
    #[serde(default)]
    pub title_basis: String,
    #[serde(default)]
    pub new_character_requests: Vec<ChapterCharacterRequest>,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub degraded_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MemoSection {
    pub heading: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DraftOutput {
    pub title: String,
    pub content: String,
    pub summary: String,
    #[serde(default)]
    pub key_facts: Vec<String>,
    #[serde(default)]
    pub continuity_updates: Vec<String>,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub degraded_reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct FinalChapterObservation {
    #[serde(default)]
    pub current_state: String,
    #[serde(default)]
    pub pending_hooks: String,
    #[serde(default)]
    pub chapter_summary: String,
    #[serde(default)]
    pub continuity_updates: Vec<String>,
    #[serde(default)]
    pub resolved_hooks: Vec<String>,
    #[serde(default)]
    pub state_changes: Vec<ChapterStateChange>,
}

pub(crate) fn is_chinese_language(language: &str) -> bool {
    let lowered = language.to_ascii_lowercase();
    lowered.contains("zh") || lowered.contains("chinese") || language.contains('中')
}

pub(crate) fn required_memo_sections(language: &str) -> &'static [&'static str] {
    if is_chinese_language(language) {
        ZH_MEMO_SECTIONS
    } else {
        EN_MEMO_SECTIONS
    }
}
