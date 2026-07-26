//! Book-title naming policy implementation.
//!
//! This module owns title surface and story-evidence checks. Callers should go
//! through `writing::naming`, not `writing::policy`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleQualityIssueKind {
    Surface,
    ContractBasis,
}

#[cfg(test)]
mod evidence_policy_tests {
    use super::{title_contract_basis_issue, title_formality_issue};

    #[test]
    fn story_backed_book_title_does_not_require_marketing_hook() {
        let evidence = "都市玄幻。霓虹是城市灵能的载体，终局关闭城市核心后只剩余烬。";
        assert_eq!(
            title_contract_basis_issue(
                "霓虹余烬",
                "书名",
                "霓虹来自城市灵能载体，余烬对应结局中关闭城市核心后的残留景象。",
                evidence,
            ),
            None
        );
    }

    #[test]
    fn story_backed_system_label_is_allowed() {
        let evidence = "古代商战。盐铁令控制食材定价，终局公开账册并废除盐铁令。";
        assert_eq!(
            title_contract_basis_issue(
                "盐铁令",
                "书名",
                "盐铁令是主线里的定价制度，也是终局被公开废除的规则。",
                evidence,
            ),
            None
        );
    }

    #[test]
    fn story_backed_ledger_title_is_not_mistaken_for_clipped_prose() {
        let evidence = "秦望澜获得能预知物价的逆流账本，凭借逆流账本挽救商号并重塑商业秩序。";
        assert_eq!(
            title_contract_basis_issue(
                "逆流账本",
                "书名",
                "核心道具“逆流账本”贯穿全书，且书名直接指向主角利用信息差翻盘的轨迹。",
                evidence,
            ),
            None
        );
    }

    #[test]
    fn clipped_contract_phrase_is_rejected() {
        let evidence = "主角推进证据链，终局公开账册并打破垄断。";
        assert!(title_contract_basis_issue(
            "进证据破局",
            "书名",
            "进证据破局来自推进证据链并打破垄断。",
            evidence,
        )
        .is_some());
    }

    #[test]
    fn title_without_contract_evidence_is_rejected() {
        assert!(title_contract_basis_issue(
            "霓虹余烬",
            "书名",
            "霓虹余烬代表城市故事的最终状态。",
            "古代航海。主角终局修复星盘并返回港口。",
        )
        .is_some());
    }

    #[test]
    fn surface_noise_remains_blocking() {
        assert!(title_formality_issue("第2章：A/path", "章节标题").is_some());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TitleQualityIssue {
    kind: TitleQualityIssueKind,
    message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TitleQualityReport {
    issues: Vec<TitleQualityIssue>,
}

impl TitleQualityReport {
    fn blocking_issue(&self) -> Option<String> {
        self.issues.first().map(|issue| issue.message.clone())
    }

    fn push_unique(&mut self, kind: TitleQualityIssueKind, message: Option<String>) {
        let Some(message) = message else {
            return;
        };
        if self.issues.iter().any(|issue| issue.message == message) {
            return;
        }
        self.issues.push(TitleQualityIssue { kind, message });
    }
}

pub(crate) fn title_formality_issue(title: &str, target: &str) -> Option<String> {
    if title_has_malformed_book_delimiters_or_ordinal(title) {
        return Some(format!("{target}混入卷/章标签、序号或书名括号残片"));
    }
    let core = normalized_title_core(title);
    if title_has_non_title_surface_noise(&core) {
        return Some(format!("{target}包含符号残片、外文残片或结构噪声"));
    }
    if title_has_degenerate_repetition(&core) {
        return Some(format!("{target}含有重复字根或退化重复"));
    }
    if title_looks_like_user_instruction_fragment(&core) {
        return Some(format!("{target}混入用户确认、修改或流程指令残片"));
    }
    if title_looks_like_creation_request_object_fragment(&core) {
        return Some(format!("{target}混入创作请求对象残片"));
    }
    if title_contains_artifact_or_workflow_label(&core) {
        return Some(format!("{target}混入计划、方案或工作流标签"));
    }
    if title_contains_internal_contract_slot_label(&core) {
        return Some(format!("{target}混入合同字段名或内部规划槽位"));
    }
    if title_looks_like_unfinished_phrase(&core)
        || title_contains_narrative_connector_fragment(&core)
        || title_starts_with_sentence_tail_connector_fragment(&core)
        || (title_target_is_book(target) && title_starts_with_clause_function_word(&core))
        || title_is_short_setting_relation_tail_fragment(&core)
        || title_is_short_action_plus_generic_outcome(&core)
        || title_is_clipped_verb_payoff_chain(&core)
        || title_looks_like_mechanical_action_chain(&core)
        || title_is_clipped_evidence_or_fact_label(&core)
        || title_is_compacted_contract_keyword_fragment(&core)
        || title_is_action_prefix_plus_narrative_verb_fragment(&core)
        || title_is_action_prefixed_function_word_fragment(&core)
        || title_is_action_prefixed_coordinating_connector_fragment(&core)
        || title_is_action_prefixed_truncated_noun_phrase(&core)
        || title_is_mechanical_compacted_action_compound(&core)
    {
        return Some(format!("{target}像从剧情或合同中截出的残句，文字不完整"));
    }
    if title_target_is_book(target) && title_looks_like_planning_volume_label(&core) {
        return Some(format!("{target}混入卷、篇或章节规划标签"));
    }
    None
}

fn title_has_non_title_surface_noise(core: &str) -> bool {
    if core.trim().is_empty() {
        return false;
    }
    if core.contains("->")
        || core.contains("=>")
        || core.contains("→")
        || core.contains("←")
        || core.contains("⇒")
        || core.contains("⇐")
        || core.contains('/')
        || core.contains('\\')
        || core.contains('|')
    {
        return true;
    }
    if title_contains_invalid_subtitle_separator(core) {
        return true;
    }
    let mut ascii_run = String::new();
    for ch in core.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            ascii_run.push(ch);
            continue;
        }
        if !ascii_run.is_empty() {
            if title_ascii_run_is_surface_noise(&ascii_run) {
                return true;
            }
            ascii_run.clear();
        }
        if matches!(
            ch,
            '+' | '=' | '*' | '#' | '@' | '$' | '%' | '^' | '&' | '~'
        ) {
            return true;
        }
    }
    false
}

fn title_contains_invalid_subtitle_separator(core: &str) -> bool {
    let colon_count = core.chars().filter(|ch| matches!(ch, '：' | ':')).count();
    if colon_count == 0 {
        return false;
    }
    if colon_count > 1 {
        return true;
    }
    let Some((left, right)) = core.split_once(['：', ':']) else {
        return true;
    };
    title_subtitle_side_is_invalid(left) || title_subtitle_side_is_invalid(right)
}

fn title_subtitle_side_is_invalid(side: &str) -> bool {
    let side = side.trim();
    let len = side.chars().count();
    if !(2..=12).contains(&len) {
        return true;
    }
    let cjk_count = side.chars().filter(|ch| ('一'..='龥').contains(ch)).count();
    cjk_count * 2 < len
}

fn title_ascii_run_is_surface_noise(run: &str) -> bool {
    let token = run.trim_matches(|ch: char| ch == '-' || ch == '_');
    if token.is_empty() {
        return false;
    }
    let has_letter = token.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    if !has_letter {
        return false;
    }
    if !has_digit {
        return true;
    }
    let letter_count = token.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
    let token_len = token.chars().count();
    let code_like_separator = token.contains('-') || token.contains('_');
    token_len > 8 || letter_count > 3 || !code_like_separator
}

fn title_looks_like_planning_volume_label(core: &str) -> bool {
    if [
        "古卷", "秘卷", "残卷", "画卷", "手卷", "书卷", "经卷", "卷宗",
    ]
    .iter()
    .any(|tail| core.ends_with(tail))
    {
        return false;
    }
    let Some(stem) = core.strip_suffix(['卷', '篇', '章']) else {
        return false;
    };
    let stem = stem.trim();
    let stem_chars = stem.chars().count();
    if stem_chars < 3 {
        return false;
    }
    if matches!(
        stem,
        "古" | "秘" | "残" | "画" | "手" | "书" | "山河" | "星河"
    ) {
        return false;
    }

    let has_action_or_outcome = [
        "破", "夺", "守", "救", "逆", "翻", "打破", "垄断", "破局", "翻盘", "归来", "觉醒", "重建",
        "崛起", "逆袭", "胜利", "真相", "终局",
    ]
    .iter()
    .any(|term| stem.contains(term));
    let has_summary_object = [
        "公司", "集团", "系统", "秩序", "规则", "世界", "人生", "命运", "资源", "权力", "垄断",
        "合同", "计划",
    ]
    .iter()
    .any(|term| stem.contains(term));

    stem_chars >= 5 || (has_action_or_outcome && has_summary_object)
}

fn title_target_is_book(target: &str) -> bool {
    target.contains("书名")
        || target.contains("作品名")
        || target.to_ascii_lowercase().contains("book title")
}

fn title_contains_narrative_connector_fragment(core: &str) -> bool {
    narrative_connector_fragments()
        .iter()
        .any(|term| core.contains(*term))
}

fn title_is_short_setting_relation_tail_fragment(core: &str) -> bool {
    let chars = core.chars().count();
    if !(3..=6).contains(&chars) {
        return false;
    }
    let relation_terms = ["并存", "共存", "交错", "交织", "碰撞", "并行", "对照"];
    let Some(term) = relation_terms.iter().find(|term| core.ends_with(**term)) else {
        return false;
    };
    let prefix = core.trim_end_matches(*term);
    let prefix_len = prefix.chars().count();
    (1..=2).contains(&prefix_len)
}

fn narrative_connector_fragments() -> &'static [&'static str] {
    &[
        "凭借", "依靠", "通过", "逐步", "最终", "历经", "获得", "踏上", "成为", "确认", "围绕",
        "遭遇", "面对", "选择", "改变", "推进", "完成", "实现", "揭开", "解开", "展开", "较量",
    ]
}

fn title_starts_with_sentence_tail_connector_fragment(core: &str) -> bool {
    let compact = core.trim();
    if compact.chars().count() < 3 {
        return false;
    }
    let Some(prefix) = ["并", "且", "而", "再", "又"]
        .iter()
        .find(|prefix| compact.starts_with(**prefix))
    else {
        return false;
    };
    let rest = compact.trim_start_matches(*prefix);
    if rest.chars().count() < 2 {
        return false;
    }
    sentence_tail_action_terms()
        .iter()
        .any(|term| rest.starts_with(*term) || rest.contains(*term))
}

fn title_starts_with_clause_function_word(core: &str) -> bool {
    let compact = core.trim();
    let len = compact.chars().count();
    if !(3..=12).contains(&len) {
        return false;
    }
    [
        "此", "这", "那", "该", "其", "是", "为", "把", "将", "让", "被", "在", "从", "向", "由",
        "因", "以", "用", "靠", "通过",
    ]
    .iter()
    .any(|prefix| compact.starts_with(*prefix))
}

fn sentence_tail_action_terms() -> &'static [&'static str] {
    &[
        "拯救", "揭开", "解开", "完成", "实现", "改变", "守住", "夺回", "修复", "打破", "推翻",
        "重塑", "重构", "建立", "证明", "成为", "获得", "赢得", "击败", "战胜", "公开",
    ]
}

fn title_is_compacted_contract_keyword_fragment(core: &str) -> bool {
    let char_count = core.chars().count();
    if !(3..=8).contains(&char_count) {
        return false;
    }
    if title_has_reader_hook_surface(core) || title_has_plot_action_hook(core) {
        return false;
    }
    let contains_contract_keyword = [
        "承诺", "因果", "终局", "结局", "规则", "法则", "代价", "契约", "命运", "主线", "弧线",
        "主题", "意象", "形态", "秩序",
    ]
    .iter()
    .any(|term| core.contains(*term));
    if !contains_contract_keyword {
        return false;
    }
    let useful_tokens = title_specific_tokens(core);
    useful_tokens.is_empty()
        || useful_tokens.iter().all(|token| {
            [
                "承诺", "因果", "终局", "结局", "规则", "法则", "代价", "契约", "命运", "主线",
                "弧线", "主题", "意象", "形态", "秩序",
            ]
            .iter()
            .any(|term| token.contains(*term))
        })
}

fn title_has_malformed_book_delimiters_or_ordinal(title: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return false;
    }
    let open_count = trimmed.matches('《').count();
    let close_count = trimmed.matches('》').count();
    if open_count != close_count {
        return true;
    }
    let core = trimmed
        .trim_matches(|ch| matches!(ch, '《' | '》' | '"' | '\'' | '“' | '”' | '`'))
        .trim();
    if core.contains("卷《") || core.contains("章《") {
        return true;
    }
    let compact = core.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    let lowered = compact.to_ascii_lowercase();
    if lowered.starts_with("volume") || lowered.starts_with("chapter") {
        return true;
    }
    let mut chars = compact.chars();
    let first = chars.next().unwrap_or_default();
    let second = chars.next();
    if (first.is_ascii_digit()
        || matches!(
            first,
            '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
        ))
        && matches!(second, Some('卷' | '章'))
    {
        return true;
    }
    compact.starts_with('第') && compact.chars().take(5).any(|ch| matches!(ch, '卷' | '章'))
}

fn title_has_degenerate_repetition(core: &str) -> bool {
    let chars = core.chars().collect::<Vec<_>>();
    if chars.windows(2).any(|window| window[0] == window[1]) {
        return true;
    }
    if chars.len() < 6 {
        return false;
    }
    for len in [2usize, 3] {
        for left in 0..=chars.len().saturating_sub(len * 2) {
            let right = left + len;
            if chars[left..right] == chars[right..right + len] {
                return true;
            }
        }
    }
    false
}

fn title_looks_like_unfinished_phrase(core: &str) -> bool {
    if core.trim().is_empty() {
        return false;
    }
    if core.chars().last().is_some_and(|ch| {
        matches!(
            ch,
            '的' | '之' | '与' | '和' | '及' | '或' | '并' | '在' | '把' | '将' | '背'
        )
    }) {
        return true;
    }
    let chars = core.chars().collect::<Vec<_>>();
    if chars.len() < 5 {
        return false;
    }
    let tail = chars[chars.len().saturating_sub(3)..]
        .iter()
        .collect::<String>();
    if matches!(
        tail.as_str(),
        "中的" | "里的" | "下的" | "上的" | "之后" | "之前"
    ) {
        return true;
    }
    let connector_count = ["的", "之", "中", "里", "下", "上"]
        .iter()
        .filter(|connector| core.contains(**connector))
        .count();
    connector_count >= 3
}

fn title_looks_like_user_instruction_fragment(core: &str) -> bool {
    let compact = core.trim();
    if compact.is_empty() {
        return false;
    }
    let has_dialogue_actor = ["我", "你", "用户", "读者"]
        .iter()
        .any(|term| compact.contains(term));
    let has_control_surface = [
        "确认",
        "修改",
        "开始",
        "继续",
        "停止",
        "暂停",
        "回复",
        "告诉",
        "需要",
        "不要",
        "可以",
        "后再",
        "再开",
        "再写",
        "来定",
        "按这个",
        "这个",
    ]
    .iter()
    .any(|term| compact.contains(term));
    if has_dialogue_actor && has_control_surface {
        return true;
    }
    let chars = compact.chars().collect::<Vec<_>>();
    if chars.len() <= 6
        && ["后再", "再开", "再写", "开始", "确认", "回复", "这个"]
            .iter()
            .any(|term| compact.contains(term))
        && !title_has_strong_reader_hook_surface(compact)
    {
        return true;
    }
    false
}

fn title_looks_like_creation_request_object_fragment(core: &str) -> bool {
    let compact = core.trim();
    if compact.is_empty() {
        return false;
    }
    let Some(prefix) = creation_request_object_fragments()
        .iter()
        .find(|prefix| compact.starts_with(**prefix))
    else {
        return false;
    };
    let rest = compact.trim_start_matches(*prefix);
    rest.chars().count() >= 2
        && (title_has_plot_action_hook(rest)
            || title_has_reader_hook_surface(rest)
            || title_mood_or_process_terms()
                .iter()
                .any(|term| rest.contains(*term)))
}

fn creation_request_object_fragments() -> &'static [&'static str] {
    &["一部", "一本", "这部", "这本", "本书", "小说"]
}

fn title_contains_artifact_or_workflow_label(core: &str) -> bool {
    [
        "计划",
        "方案",
        "项目",
        "工程",
        "报告",
        "合同",
        "草案",
        "框架",
        "规划",
        "设定集",
        "大纲",
        "摘要",
        "总结",
        "说明",
        "手册",
        "指南",
    ]
    .iter()
    .any(|term| core.contains(term))
}

fn title_contains_internal_contract_slot_label(core: &str) -> bool {
    internal_contract_slot_title_terms()
        .iter()
        .any(|term| core.contains(*term))
}

fn internal_contract_slot_title_terms() -> &'static [&'static str] {
    &[
        "卷尾变化",
        "卷尾转折",
        "不可逆变化",
        "预期转折",
        "读者期待",
        "读者承诺",
        "书名理由",
        "标题理由",
        "命名理由",
        "字段完整度",
        "结构化合同",
        "合同字段",
        "角色权威表",
        "近期章节",
        "章节目标",
        "故事骨架",
        "终局方向",
        "终局状态",
        "主角弧线",
        "因果链",
    ]
}

fn title_is_mechanical_compacted_action_compound(core: &str) -> bool {
    let chars = core.chars().count();
    if !(4..=6).contains(&chars) {
        return false;
    }
    for action in ["突破", "打破", "改写", "重塑", "重建", "觉醒", "建立"] {
        let Some((head, tail)) = core.split_once(action) else {
            continue;
        };
        if head.is_empty() || tail.is_empty() {
            continue;
        }
        if head.chars().count() <= 2
            && tail.chars().count() <= 2
            && (mechanical_title_side_is_generic(head) || mechanical_title_side_is_generic(tail))
        {
            return true;
        }
    }
    false
}

fn title_is_action_prefix_plus_narrative_verb_fragment(core: &str) -> bool {
    let chars = core.chars().count();
    if !(4..=8).contains(&chars) {
        return false;
    }
    let Some(action) = [
        "破", "夺", "斩", "杀", "救", "守", "炼", "试", "醒", "封", "改", "建", "开",
    ]
    .iter()
    .find(|action| core.starts_with(**action)) else {
        return false;
    };
    let rest = core.trim_start_matches(*action);
    [
        "揭开", "解开", "发现", "确认", "获得", "踏上", "进入", "寻找", "面对", "改变", "完成",
        "实现", "选择",
    ]
    .iter()
    .any(|verb| {
        let Some(tail) = rest.strip_prefix(*verb) else {
            return false;
        };
        tail.chars().count() <= 2
    })
}

fn title_is_action_prefixed_function_word_fragment(core: &str) -> bool {
    let chars = core.chars().collect::<Vec<_>>();
    if !(4..=8).contains(&chars.len()) {
        return false;
    }
    let Some(action) = [
        "破", "夺", "斩", "杀", "救", "守", "炼", "试", "醒", "封", "改", "建", "开",
    ]
    .iter()
    .find(|action| core.starts_with(**action)) else {
        return false;
    };
    let rest = core.trim_start_matches(*action);
    if rest.chars().count() < 3 || title_has_strong_reader_hook_surface(rest) {
        return false;
    }
    let has_function_word = ["在", "被", "让", "把", "将", "为", "向", "由", "因", "因而"]
        .iter()
        .any(|term| rest.contains(*term));
    has_function_word
}

fn title_is_action_prefixed_coordinating_connector_fragment(core: &str) -> bool {
    let chars = core.chars().collect::<Vec<_>>();
    if !(4..=10).contains(&chars.len()) {
        return false;
    }
    if !matches!(
        chars.first().copied(),
        Some(
            '破' | '夺'
                | '斩'
                | '杀'
                | '救'
                | '守'
                | '炼'
                | '试'
                | '醒'
                | '封'
                | '改'
                | '建'
                | '开'
                | '借'
        )
    ) {
        return false;
    }
    if !matches!(
        chars.get(1).copied(),
        Some('与' | '和' | '及' | '并' | '或' | '、')
    ) {
        return false;
    }
    let rest = chars.iter().skip(2).collect::<String>();
    rest.chars().count() >= 2
        && (title_has_story_entry_surface(&rest, core)
            || title_specific_tokens(&rest).iter().any(|token| {
                story_object_suffixes()
                    .iter()
                    .any(|suffix| token.ends_with(*suffix))
            }))
}

fn title_is_action_prefixed_truncated_noun_phrase(core: &str) -> bool {
    let chars = core.chars().collect::<Vec<_>>();
    if !(4..=12).contains(&chars.len()) {
        return false;
    }
    let Some(action) = [
        "公开", "揭开", "解开", "发现", "追踪", "潜入", "重置", "实现", "破", "夺", "斩", "杀",
        "救", "守", "炼", "试", "醒", "封", "改", "建", "开",
    ]
    .iter()
    .find(|action| core.starts_with(**action)) else {
        return false;
    };
    let rest = core.trim_start_matches(*action);
    if rest.chars().count() < 3 {
        return false;
    }
    if title_action_rest_is_clause_object_fragment(rest) {
        return true;
    }
    let last = rest.chars().last().unwrap_or_default();
    let truncated_quantifier_tail = matches!(
        last,
        '千' | '万'
            | '百'
            | '亿'
            | '诸'
            | '众'
            | '群'
            | '满'
            | '半'
            | '背'
            | '后'
            | '前'
            | '里'
            | '中'
            | '下'
            | '上'
    );
    if truncated_quantifier_tail {
        return true;
    }
    let generic_class_tail = [
        "豪门", "世家", "公司", "集团", "学院", "宗门", "门派", "王朝", "帝国", "城市", "世界",
    ]
    .iter()
    .any(|term| rest.ends_with(*term));
    if generic_class_tail && !title_has_strong_reader_hook_surface(rest) {
        return true;
    }

    let event_clause_tail = [
        "崩塌", "坍塌", "倒灌", "失控", "爆炸", "毁灭", "沦陷", "停摆", "消亡", "失踪",
    ]
    .iter()
    .any(|predicate| {
        rest.ends_with(*predicate) && rest.trim_end_matches(*predicate).chars().count() >= 2
    });
    if event_clause_tail {
        return true;
    }

    let investigation_summary_tail = [
        "命案", "旧案", "案件", "悬案", "血案", "谜案", "奇案", "冤案", "真相", "黑幕", "阴谋",
        "危机",
    ]
    .iter()
    .any(|term| rest.ends_with(*term));
    if !investigation_summary_tail {
        return false;
    }
    let scope_or_generic_modifier = [
        "京城", "全城", "天下", "朝堂", "宫廷", "豪门", "世家", "连环", "系列", "全部", "所有",
        "权贵", "行业", "城市", "学院", "宗门",
    ]
    .iter()
    .any(|term| rest.contains(*term));
    scope_or_generic_modifier || rest.chars().count() >= 5
}

fn title_action_rest_is_clause_object_fragment(rest: &str) -> bool {
    let rest = rest.trim();
    let len = rest.chars().count();
    if !(3..=8).contains(&len) {
        return false;
    }
    let starts_with_measure_phrase = [
        "一个", "一场", "一份", "一次", "一种", "一段", "一条", "这场", "这份", "这个", "那场",
        "那份", "那个", "某个", "某场",
    ]
    .iter()
    .any(|prefix| rest.starts_with(*prefix));
    if !starts_with_measure_phrase {
        return false;
    }
    [
        "针对", "关于", "面向", "属于", "指向", "围绕", "有关", "对于", "对抗", "连接", "通往",
    ]
    .iter()
    .any(|term| rest.contains(*term))
}

fn mechanical_title_side_is_generic(value: &str) -> bool {
    [
        "界", "天", "道", "门", "宗", "院", "城", "规则", "法则", "秩序", "体系", "世界", "命运",
        "桎梏", "枷锁", "灵脉", "气运", "仙门", "宗门", "天阶", "血脉", "命格",
    ]
    .iter()
    .any(|term| value.contains(term))
}

fn title_is_short_action_plus_generic_outcome(core: &str) -> bool {
    let char_count = core.chars().count();
    if !(3..=5).contains(&char_count) {
        return false;
    }
    if title_is_story_anchored_full_object(core, core) {
        return false;
    }
    let Some(action) = [
        "得", "获", "取", "求", "证", "赢", "成", "争", "夺", "控", "掌",
    ]
    .iter()
    .find(|action| core.starts_with(**action)) else {
        return false;
    };
    let rest = core.trim_start_matches(*action);
    if rest.chars().count() < 2 {
        return false;
    }
    let generic_outcomes = [
        "转机", "机会", "机遇", "成功", "胜利", "逆袭", "崛起", "人生", "未来", "命运", "答案",
        "真相", "信任", "认可", "资本", "资源", "权力", "格局", "局面", "局势", "承诺", "证明",
        "成长",
    ];
    generic_outcomes
        .iter()
        .any(|outcome| rest == *outcome || rest.ends_with(*outcome))
}

fn title_is_clipped_verb_payoff_chain(core: &str) -> bool {
    let char_count = core.chars().count();
    if !(5..=9).contains(&char_count) {
        return false;
    }
    if title_has_story_entry_surface(core, core) || title_is_story_anchored_full_object(core, core)
    {
        return false;
    }

    let clipped_prefix_followed_by_narrative_verb = [
        "反杀", "逆袭", "翻盘", "破局", "破产", "夺权", "夺回", "清算", "登顶", "崛起", "掌控",
        "改写",
    ]
    .iter()
    .any(|verb| {
        let Some(prefix) = core.split(*verb).next() else {
            return false;
        };
        let prefix_chars = prefix.chars().count();
        prefix_chars == 1 && !title_single_char_prefix_is_natural_anchor(prefix)
    });
    if !clipped_prefix_followed_by_narrative_verb {
        return false;
    }

    let has_clause_connector = ["并", "再", "后", "而", "以", "将", "让", "把", "从"]
        .iter()
        .any(|connector| core.contains(*connector));
    let generic_payoff_tail = [
        "破局", "翻盘", "逆袭", "登顶", "崛起", "清算", "夺回", "夺权", "改写", "掌控",
    ]
    .iter()
    .any(|term| core.ends_with(*term));

    has_clause_connector && generic_payoff_tail
}

fn title_single_char_prefix_is_natural_anchor(prefix: &str) -> bool {
    matches!(
        prefix.chars().next().unwrap_or_default(),
        '门' | '城'
            | '山'
            | '海'
            | '塔'
            | '桥'
            | '院'
            | '街'
            | '灯'
            | '井'
            | '碑'
            | '钟'
            | '镜'
            | '剑'
            | '刀'
            | '火'
            | '雨'
            | '雪'
            | '风'
            | '月'
            | '星'
            | '云'
            | '河'
    )
}

fn title_is_clipped_evidence_or_fact_label(core: &str) -> bool {
    let chars = core.chars().count();
    if !(3..=6).contains(&chars) {
        return false;
    }
    let fact_terms = [
        "证据", "真相", "线索", "文件", "合同", "账目", "流水", "录音", "视频", "照片", "报告",
    ];
    let Some(term) = fact_terms
        .iter()
        .find(|term| core == **term || core.ends_with(**term))
    else {
        return false;
    };
    if core == *term {
        return true;
    }
    let prefix = core.trim_end_matches(*term);
    prefix.chars().count() <= 2
        && [
            "现", "见", "看", "查", "找", "得", "获", "拿", "揭", "曝", "呈", "交", "旧", "新",
            "真", "假",
        ]
        .iter()
        .any(|fragment| prefix == *fragment || prefix.ends_with(*fragment))
}

fn title_looks_like_clipped_hook_noun_compound(core: &str, story_evidence: &str) -> bool {
    let story_evidence = story_evidence.trim();
    if story_evidence.is_empty() {
        return false;
    }
    let chars = core.chars().count();
    if !(3..=6).contains(&chars) {
        return false;
    }
    let generic_hook_nouns = [
        "契约", "合同", "证据", "真相", "线索", "档案", "账册", "账目", "命脉", "规则", "关系",
        "选择", "救赎", "觉醒", "余烬", "余温", "裂痕", "承诺",
    ];
    let Some(noun) = generic_hook_nouns
        .iter()
        .find(|noun| core.ends_with(**noun) && core != **noun)
    else {
        return false;
    };
    let prefix = core.trim_end_matches(*noun);
    let prefix_len = prefix.chars().count();
    if !(1..=2).contains(&prefix_len) {
        return false;
    }
    if story_evidence.contains(prefix) || story_evidence.contains(core) {
        return false;
    }
    let noun_is_generic_contract_or_outcome = abstract_title_concept_terms()
        .iter()
        .any(|term| noun.contains(*term))
        || [
            "契约", "合同", "证据", "真相", "线索", "档案", "账册", "账目", "关系", "规则",
        ]
        .iter()
        .any(|term| noun == term);
    noun_is_generic_contract_or_outcome
}

fn title_cjk_char(ch: char) -> bool {
    matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{20000}'..='\u{2A6DF}')
}

fn title_looks_like_truncated_story_phrase(core: &str, story_evidence: &str) -> bool {
    let chars = core.chars().count();
    if !(3..=12).contains(&chars) || story_evidence.trim().is_empty() {
        return false;
    }
    if title_has_strong_reader_hook_surface(core)
        && title_has_story_entry_surface(core, story_evidence)
        && title_story_specific_overlap_count(core, story_evidence) > 0
    {
        return false;
    }
    if title_is_story_anchored_full_object(core, story_evidence) {
        return false;
    }
    let core_chars = core.chars().collect::<Vec<_>>();
    let evidence_chars = story_evidence.chars().collect::<Vec<_>>();
    if core_chars.is_empty() || evidence_chars.len() <= core_chars.len() {
        return false;
    }
    for start in 0..=evidence_chars.len().saturating_sub(core_chars.len()) {
        if evidence_chars[start..start + core_chars.len()] != core_chars {
            continue;
        }
        let before_inside_word = start
            .checked_sub(1)
            .and_then(|idx| evidence_chars.get(idx))
            .is_some_and(|ch| title_cjk_char(*ch));
        let after_inside_word = evidence_chars
            .get(start + core_chars.len())
            .is_some_and(|ch| title_cjk_char(*ch));
        if after_inside_word || (before_inside_word && !title_has_reader_hook_surface(core)) {
            return true;
        }
    }
    false
}

pub(crate) fn title_contract_basis_issue(
    title: &str,
    target: &str,
    rationale: &str,
    story_evidence: &str,
) -> Option<String> {
    title_quality_report(title, target, rationale, story_evidence).blocking_issue()
}

fn title_quality_report(
    title: &str,
    target: &str,
    rationale: &str,
    story_evidence: &str,
) -> TitleQualityReport {
    let mut report = TitleQualityReport::default();
    report.push_unique(
        TitleQualityIssueKind::Surface,
        title_formality_issue(title, target),
    );
    report.push_unique(
        TitleQualityIssueKind::ContractBasis,
        title_contract_blocking_issue(title, target, rationale, story_evidence),
    );
    report
}

pub(crate) fn title_contract_blocking_issue(
    title: &str,
    target: &str,
    rationale: &str,
    story_evidence: &str,
) -> Option<String> {
    let title = title.trim();
    let rationale = rationale.trim();
    let story_evidence = story_evidence.trim();
    if title.is_empty() {
        return Some(format!("{target}为空，不能锁定为作品权威标题"));
    }
    if let Some(issue) = title_formality_issue(title, target) {
        return Some(issue);
    }
    let core = normalized_title_core(title);
    if title_contains_clipped_narrative_connector_tail(&core, story_evidence)
        || title_looks_like_clipped_hook_noun_compound(&core, story_evidence)
        || title_looks_like_truncated_story_phrase(&core, story_evidence)
    {
        return Some(format!(
            "{target}像从合同或剧情证据里截出的半截短语，文字不完整"
        ));
    }
    if title_target_is_chapter(target) && title_looks_like_character_name_with_weak_suffix(&core) {
        return Some(format!("{target}像人物名后拼接的正文残字，文字不完整"));
    }
    if let Some(other_title) = rationale_quoted_title_mismatch(&core, rationale) {
        return Some(format!(
            "{target}命名理由解释的是《{other_title}》，和当前书名《{core}》不一致"
        ));
    }
    if rationale.chars().count() < 12 {
        return Some(format!(
            "{target}命名理由过短，必须说明它如何来自结局、大纲、核心冲突或世界规则"
        ));
    }
    if title_basis_rationale_is_generic(rationale) {
        return Some(format!(
            "{target}命名理由过于泛化，必须写清楚具体情节、结局兑现或世界规则，而不是只说体现主题"
        ));
    }

    let title_tokens = title_specific_tokens(&core);
    if title_tokens.is_empty() {
        return Some(format!(
            "{target}缺少可验证的故事锚点，不能只由题材词、抽象词或通用气质组成"
        ));
    }
    if let Some(missing_anchor) = title_rationale_missing_required_anchor(&core, rationale) {
        return Some(format!(
            "{target}命名理由没有解释标题中的关键锚点 `{missing_anchor}`，不能证明整部书名来自当前合同"
        ));
    }

    let external_story_evidence = story_evidence_without_rationale(story_evidence, rationale);
    let rationale_mentions_title = title_tokens
        .iter()
        .any(|token| rationale.contains(token.as_str()));
    let evidence_supports_title = title_tokens
        .iter()
        .any(|token| external_story_evidence.contains(token.as_str()));
    let rationale_story_overlap =
        story_specific_overlap_count(rationale, &external_story_evidence, &title_tokens);
    if !rationale_mentions_title {
        return Some(format!(
            "{target}命名理由没有解释标题中的关键字，不能证明书名来自当前合同"
        ));
    }
    if rationale_mentions_title
        && title_rationale_has_concrete_payoff(rationale)
        && (evidence_supports_title || rationale_story_overlap >= 2)
    {
        return None;
    }
    if !evidence_supports_title && rationale_story_overlap < 2 {
        return Some(format!(
            "{target}没有被合同里的剧情、终局、世界观或主线因果链支撑"
        ));
    }

    None
}

fn rationale_quoted_title_mismatch(title: &str, rationale: &str) -> Option<String> {
    let title_core = normalized_title_core(title);
    if let Some(mismatch) = rationale
        .split('《')
        .skip(1)
        .filter_map(|tail| tail.split_once('》').map(|(value, _)| value.trim()))
        .filter(|value| !value.is_empty())
        .find(|value| normalized_title_core(value) != title_core)
        .map(str::to_string)
    {
        return Some(mismatch);
    }

    for (marker, close) in [
        ("书名'", "'"),
        ("标题'", "'"),
        ("作品名'", "'"),
        ("书名\"", "\""),
        ("标题\"", "\""),
        ("作品名\"", "\""),
        ("书名“", "”"),
        ("标题“", "”"),
        ("作品名“", "”"),
        ("书名‘", "’"),
        ("标题‘", "’"),
        ("作品名‘", "’"),
    ] {
        let mut rest = rationale;
        while let Some(start) = rest.find(marker) {
            let after = &rest[start + marker.len()..];
            let Some(end) = after.find(close) else {
                break;
            };
            let quoted = after[..end].trim();
            if rationale_quoted_title_segment_looks_like_title(quoted)
                && normalized_title_core(quoted) != title_core
            {
                return Some(quoted.to_string());
            }
            rest = &after[end + close.len()..];
        }
    }

    if let Some(mismatch) = rationale_bare_quoted_title_mismatch(&title_core, rationale) {
        return Some(mismatch);
    }

    if let Some(mismatch) = rationale_unquoted_title_mismatch(&title_core, rationale) {
        return Some(mismatch);
    }

    None
}

fn rationale_unquoted_title_mismatch(title_core: &str, rationale: &str) -> Option<String> {
    for marker in ["书名", "标题", "作品名"] {
        let mut rest = rationale;
        while let Some(start) = rest.find(marker) {
            let after = rest[start + marker.len()..].trim_start_matches(['：', ':', ' ', '\t']);
            let end = [
                "浓缩", "源自", "来自", "取自", "对应", "呼应", "指向", "聚焦", "概括", "体现",
                "融合", "围绕", "承载", "象征", "直指",
            ]
            .iter()
            .filter_map(|predicate| after.find(predicate))
            .min();
            if let Some(end) = end {
                let candidate = after[..end]
                    .trim()
                    .trim_matches(['《', '》', '“', '”', '‘', '’', '\'', '"']);
                let candidate_core = normalized_title_core(candidate);
                let candidate_chars = candidate.chars().count();
                let candidate_is_plain_cjk_title = (2..=12).contains(&candidate_chars)
                    && candidate
                        .chars()
                        .all(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
                    && ![
                        "直接", "本身", "同时", "最终", "明确", "主要", "核心", "实际", "能够",
                        "可以", "正好",
                    ]
                    .iter()
                    .any(|modifier| candidate == *modifier);
                if candidate_is_plain_cjk_title
                    && candidate_core != title_core
                    && !title_core.contains(&candidate_core)
                    && !candidate_core.contains(title_core)
                {
                    return Some(candidate.to_string());
                }
            }
            rest = &after[after.chars().next().map(char::len_utf8).unwrap_or(0)..];
        }
    }
    None
}

fn rationale_bare_quoted_title_mismatch(title_core: &str, rationale: &str) -> Option<String> {
    for (open, close) in [("“", "”"), ("‘", "’"), ("\"", "\""), ("'", "'")] {
        let mut rest = rationale;
        while let Some(start) = rest.find(open) {
            let before = &rest[..start];
            let after_open = &rest[start + open.len()..];
            let Some(end) = after_open.find(close) else {
                break;
            };
            let quoted = after_open[..end].trim();
            let after = &after_open[end + close.len()..];
            let quoted_core = normalized_title_core(quoted);
            if rationale_quoted_title_segment_looks_like_title(quoted)
                && quoted_core != title_core
                && !title_core.contains(&quoted_core)
                && !quoted_core.contains(title_core)
                && bare_quoted_title_context_looks_like_title_reference(before, after)
            {
                return Some(quoted.to_string());
            }
            rest = after;
        }
    }
    None
}

fn bare_quoted_title_context_looks_like_title_reference(before: &str, after: &str) -> bool {
    let before_tail = before.chars().rev().take(12).collect::<String>();
    let before_tail = before_tail.chars().rev().collect::<String>();
    let after_head = after.chars().take(16).collect::<String>();
    let explicit_before = [
        "书名",
        "标题",
        "作品名",
        "命名",
        "名为",
        "题为",
        "叫作",
        "叫做",
        "取名",
    ]
    .iter()
    .any(|marker| before_tail.contains(marker));
    if explicit_before {
        return true;
    }
    let leading_quote = before.trim().is_empty()
        || before
            .trim_end()
            .chars()
            .last()
            .is_some_and(|ch| matches!(ch, '：' | ':' | '。' | '；' | ';' | '\n'));
    leading_quote
        && [
            "来自",
            "对应",
            "取自",
            "融合",
            "精准融合",
            "体现",
            "指向",
            "中的",
            "命名",
        ]
        .iter()
        .any(|marker| after_head.contains(marker))
}

fn rationale_quoted_title_segment_looks_like_title(value: &str) -> bool {
    let value = normalized_title_core(value);
    let len = value.chars().count();
    (2..=14).contains(&len) && value.chars().any(is_title_policy_cjk_unified)
}

fn is_title_policy_cjk_unified(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
            | 0x30000..=0x3134F
    )
}

fn story_evidence_without_rationale(story_evidence: &str, rationale: &str) -> String {
    let rationale = rationale.trim();
    if rationale.is_empty() {
        return story_evidence.to_string();
    }
    story_evidence
        .lines()
        .filter(|line| line.trim() != rationale)
        .collect::<Vec<_>>()
        .join("\n")
        .replace(rationale, "")
}

fn title_rationale_has_concrete_payoff(rationale: &str) -> bool {
    let concrete = [
        "封印", "关闭", "打开", "公开", "坍塌", "重建", "支付", "偿还", "献祭", "牺牲", "失去",
        "夺回", "打破", "突破", "推翻", "反转", "改写", "守住", "救下", "归还", "修补",
    ];
    concrete.iter().any(|term| rationale.contains(term))
}

fn title_rationale_missing_required_anchor(core: &str, rationale: &str) -> Option<String> {
    let core = normalized_title_core(core);
    if core.chars().count() < 4 || rationale.contains(&core) {
        return None;
    }
    let chars = core.chars().collect::<Vec<_>>();
    let suffix = chars[chars.len().saturating_sub(2)..]
        .iter()
        .collect::<String>();
    if title_suffix_anchor_can_be_implicit(&suffix) {
        return None;
    }
    let explained_tokens = title_specific_tokens(&core)
        .into_iter()
        .filter(|token| token != &suffix && rationale.contains(token))
        .count();
    if explained_tokens > 0 && title_rationale_has_concrete_payoff(rationale) {
        return None;
    }
    if title_specific_token_is_useful(&suffix) && !rationale.contains(&suffix) {
        return Some(suffix);
    }
    None
}

fn title_suffix_anchor_can_be_implicit(suffix: &str) -> bool {
    [
        "开始", "开局", "破局", "逆袭", "翻盘", "崛起", "登顶", "觉醒", "升级", "重构", "重塑",
        "掌控", "称王", "巅峰", "反击", "清算", "胜利",
    ]
    .iter()
    .any(|term| suffix == *term)
}

pub(crate) fn title_anchor_tokens(title: &str) -> Vec<String> {
    title_specific_tokens(&normalized_title_core(title))
}

fn token_is_clipped_from_narrative_connector_tail(token: &str, chunk: &str) -> bool {
    let token = token.trim();
    let token_len = token.chars().count();
    if !(2..=5).contains(&token_len) {
        return false;
    }
    let Some(first) = token.chars().next() else {
        return false;
    };
    let rest = token.chars().skip(1).collect::<String>();
    if rest.is_empty() {
        return false;
    }
    let clipped_after_connector = narrative_connector_fragments()
        .iter()
        .filter(|connector| connector.chars().count() >= 2)
        .any(|connector| {
            connector.chars().last() == Some(first) && chunk.contains(&format!("{connector}{rest}"))
        });
    if !clipped_after_connector {
        return false;
    }
    let rest_is_generic_plot_surface = [
        "证据", "资源", "关系", "代价", "真相", "危机", "冲突", "线索", "秘密", "身份", "规则",
        "秩序", "势力", "困局", "结果", "核心", "目标",
    ]
    .iter()
    .any(|term| rest.contains(term));
    rest_is_generic_plot_surface || title_contains_narrative_connector_fragment(&rest)
}

fn title_contains_clipped_narrative_connector_tail(core: &str, story_evidence: &str) -> bool {
    if story_evidence.trim().is_empty() {
        return false;
    }
    title_anchor_tokens(core)
        .iter()
        .any(|token| token_is_clipped_from_narrative_connector_tail(token, story_evidence))
}

pub(crate) fn title_rationale_is_concrete(rationale: &str, title: &str) -> bool {
    let rationale = rationale.trim();
    if rationale.chars().count() < 12 {
        return false;
    }
    let title_core = normalized_title_core(title);
    if rationale_quoted_title_mismatch(&title_core, rationale).is_some() {
        return false;
    }
    if title_basis_rationale_is_generic(rationale) {
        return false;
    }
    if title_rationale_missing_required_anchor(&title_core, rationale).is_some() {
        return false;
    }
    let mentions_title_token = title_anchor_tokens(title)
        .iter()
        .any(|token| rationale.contains(token));
    let mentions_story_basis = title_basis_markers()
        .iter()
        .any(|term| rationale.contains(term));
    mentions_title_token && mentions_story_basis
}

fn normalized_title_core(title: &str) -> String {
    let mut core = title
        .trim()
        .trim_matches(|ch| matches!(ch, '《' | '》' | '"' | '\'' | '“' | '”' | '`'))
        .trim()
        .to_string();
    if let Some((prefix, rest)) = core.split_once(['：', ':']) {
        let prefix_key = prefix
            .chars()
            .filter(|ch| ch.is_ascii_digit() || ('一'..='龥').contains(ch))
            .collect::<String>();
        if prefix_key.contains('章') || prefix_key.contains("chapter") {
            core = rest.trim().to_string();
        }
    }
    core.chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '，' | ',' | '。' | '.' | '、'))
        .collect()
}

fn title_has_reader_hook_surface(core: &str) -> bool {
    reader_hook_terms().iter().any(|term| core.contains(*term))
}

fn title_has_plot_action_hook(core: &str) -> bool {
    plot_action_hook_terms()
        .iter()
        .any(|term| core.contains(*term))
}

fn title_has_story_entry_surface(core: &str, story_evidence: &str) -> bool {
    let evidence = story_evidence.trim();
    story_entry_terms()
        .iter()
        .chain(story_object_suffixes().iter())
        .any(|term| core.contains(*term) && evidence.contains(*term))
}

fn title_is_story_anchored_full_object(core: &str, story_evidence: &str) -> bool {
    let evidence = story_evidence.trim();
    if core.chars().count() < 3 || evidence.is_empty() || !evidence.contains(core) {
        return false;
    }
    story_object_suffixes().iter().any(|suffix| {
        core.ends_with(*suffix) && core.trim_end_matches(*suffix).chars().count() >= 2
    })
}

fn story_object_suffixes() -> &'static [&'static str] {
    &[
        "借灵证",
        "准考证",
        "通行证",
        "许可证",
        "账册",
        "账本",
        "账目",
        "名册",
        "名单",
        "档案",
        "卷宗",
        "契约",
        "婚契",
        "灵契",
        "禁令",
        "剥骨令",
        "灵轨",
        "轨道",
        "证",
        "令",
        "账",
        "碑",
        "塔",
        "桥",
        "城",
        "院",
        "校",
        "楼",
        "街",
        "巷",
        "门",
        "钟",
        "镜",
        "卷",
        "册",
        "簿",
        "书",
        "契",
        "案",
    ]
}

fn title_has_strong_reader_hook_surface(core: &str) -> bool {
    strong_reader_hook_terms()
        .iter()
        .any(|term| core.contains(*term))
}

fn title_looks_like_character_name_with_weak_suffix(core: &str) -> bool {
    let chars = core.chars().collect::<Vec<_>>();
    if !(4..=5).contains(&chars.len()) {
        return false;
    }
    for name_len in [3usize, 2] {
        if chars.len() <= name_len {
            continue;
        }
        let name = chars[..name_len].iter().collect::<String>();
        let suffix = chars[name_len..].iter().collect::<String>();
        if cjk_token_looks_like_person_name(&name)
            && weak_character_title_suffixes()
                .iter()
                .any(|term| suffix == *term)
        {
            return true;
        }
    }
    false
}

fn title_target_is_chapter(target: &str) -> bool {
    let lowered = target.to_ascii_lowercase();
    target.contains("章") || lowered.contains("chapter")
}

fn weak_character_title_suffixes() -> &'static [&'static str] {
    &[
        "心", "身", "影", "眼", "眸", "手", "嘴", "血", "骨", "魂", "梦", "意", "念", "路", "门",
        "局", "劫",
    ]
}

fn cjk_token_looks_like_person_name(token: &str) -> bool {
    let chars = token.chars().collect::<Vec<_>>();
    if chars.len() != 3
        || !chars
            .iter()
            .all(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
    {
        return false;
    }
    let surname = chars[0];
    if !common_cjk_surnames().contains(&surname) {
        return false;
    }
    let name_tail = chars[1..].iter().collect::<String>();
    !title_has_reader_hook_surface(&name_tail)
        && !title_has_plot_action_hook(&name_tail)
        && !title_mood_or_process_terms()
            .iter()
            .any(|term| name_tail.contains(*term))
}

fn common_cjk_surnames() -> &'static [char] {
    &[
        '赵', '钱', '孙', '李', '周', '吴', '郑', '王', '冯', '陈', '褚', '卫', '蒋', '沈', '韩',
        '杨', '朱', '秦', '尤', '许', '何', '吕', '施', '张', '孔', '曹', '严', '华', '金', '魏',
        '陶', '姜', '戚', '谢', '邹', '喻', '柏', '水', '窦', '章', '云', '苏', '潘', '葛', '奚',
        '范', '彭', '郎', '鲁', '韦', '昌', '马', '苗', '凤', '花', '方', '俞', '任', '袁', '柳',
        '鲍', '史', '唐', '费', '廉', '岑', '薛', '雷', '贺', '倪', '汤', '滕', '殷', '罗', '毕',
        '郝', '邬', '安', '常', '乐', '于', '时', '傅', '皮', '卞', '齐', '康', '伍', '余', '元',
        '卜', '顾', '孟', '平', '黄', '和', '穆', '萧', '尹', '姚', '邵', '湛', '汪', '祁', '毛',
        '禹', '狄', '米', '贝', '明', '臧', '计', '伏', '成', '戴', '谈', '宋', '庞', '熊', '纪',
        '舒', '屈', '项', '祝', '董', '梁', '杜', '阮', '蓝', '闵', '席', '季', '麻', '强', '贾',
        '路', '娄', '危', '江', '童', '颜', '郭', '梅', '盛', '林', '刁', '钟', '徐', '邱', '骆',
        '高', '夏', '蔡', '田', '胡', '凌', '霍', '虞', '万', '支', '柯', '管', '卢', '莫', '经',
        '房', '裘', '缪', '干', '解', '应', '宗', '丁', '宣', '邓', '郁', '单', '杭', '洪', '包',
        '诸', '左', '石', '崔', '吉', '龚', '程', '嵇', '邢', '滑', '裴', '陆', '荣', '翁', '荀',
        '羊', '於', '惠', '甄', '曲', '家', '封', '芮', '羿', '储', '靳', '汲', '邴', '糜', '松',
        '井', '段', '富', '巫', '乌', '焦', '巴', '弓', '牧', '隗', '山', '谷', '车', '侯', '宓',
        '蓬', '全', '郗', '班', '仰', '秋', '仲', '伊', '宫', '宁', '仇', '栾', '暴', '甘', '斜',
        '厉', '戎', '祖', '武', '符', '刘', '景', '詹', '束', '龙', '叶', '幸', '司', '韶', '郜',
        '黎', '蓟', '薄', '印', '宿', '白', '怀', '蒲', '邰', '从', '鄂', '索', '咸', '赖', '卓',
        '蔺', '屠', '蒙', '池', '乔', '阴', '鬱', '胥', '能', '苍', '双', '闻', '莘', '党', '翟',
        '谭', '贡', '劳', '逄', '姬', '申', '扶', '堵', '冉', '宰', '郦', '雍', '璩', '桑', '桂',
        '濮', '牛', '寿', '通', '边', '扈', '燕', '冀', '浦', '尚', '农', '温', '别', '庄', '晏',
        '柴', '瞿', '阎', '充', '慕', '连', '茹', '习', '宦', '艾', '鱼', '容', '向', '古', '易',
        '慎', '戈', '廖', '庾', '终', '暨', '居', '衡', '步', '都', '耿', '满', '弘', '匡', '国',
        '文', '寇', '广', '禄', '阙', '东', '欧', '殳', '沃', '利', '蔚', '越', '夔', '隆', '师',
        '巩', '厍', '聂', '晁', '勾', '敖', '融', '冷', '訾', '辛', '阚', '那', '简', '饶', '空',
        '曾', '毋', '沙', '乜', '养', '鞠', '须', '丰', '巢', '关', '蒯', '相', '查', '后', '荆',
        '红', '游', '竺', '权', '逯', '盖', '益', '桓', '公',
    ]
}

fn title_story_specific_overlap_count(core: &str, story_evidence: &str) -> usize {
    let evidence = story_evidence.trim();
    if evidence.is_empty() {
        return 0;
    }
    title_specific_tokens(core)
        .into_iter()
        .filter(|token| {
            evidence.contains(token.as_str()) && !title_token_is_mood_or_process(token.as_str())
        })
        .count()
}

fn title_specific_tokens(core: &str) -> Vec<String> {
    let chars = core.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    for len in [4usize, 3, 2] {
        if chars.len() < len {
            continue;
        }
        for window in chars.windows(len) {
            let token = window.iter().collect::<String>();
            if title_specific_token_is_useful(&token) && !tokens.iter().any(|known| known == &token)
            {
                tokens.push(token);
            }
        }
    }
    tokens
}

fn title_specific_token_is_useful(token: &str) -> bool {
    if token.chars().count() < 2 {
        return false;
    }
    if title_token_has_fragment_edge(token) {
        return false;
    }
    if token
        .chars()
        .any(|ch| matches!(ch, '的' | '之' | '中' | '下' | '上' | '与' | '和'))
    {
        return false;
    }
    if title_has_malformed_book_delimiters_or_ordinal(token) {
        return false;
    }
    if title_contains_narrative_connector_fragment(token) {
        return false;
    }
    if creation_request_object_fragments()
        .iter()
        .any(|fragment| token.contains(*fragment))
    {
        return false;
    }
    if super::title_lexicon::generic_fiction_chapter_title_terms()
        .iter()
        .any(|term| token == *term)
    {
        return false;
    }
    !abstract_title_concept_terms()
        .iter()
        .any(|term| token.contains(*term))
        && ![
            "小说", "故事", "主角", "角色", "章节", "世界", "结局", "命运", "成长", "主题", "都市",
            "玄幻", "科幻", "言情", "异界", "重生", "逆袭", "第一", "第二",
        ]
        .iter()
        .any(|term| token.contains(term))
}

fn title_token_has_fragment_edge(token: &str) -> bool {
    let compact = token.trim();
    if compact.is_empty() {
        return true;
    }
    let mut chars = compact.chars();
    let first = chars.next().unwrap_or_default();
    let last = compact.chars().last().unwrap_or_default();
    let leading_fragments = [
        '得', '被', '让', '把', '将', '从', '在', '以', '用', '为', '向', '对', '角',
    ];
    let trailing_fragments = [
        '得', '了', '着', '为', '成', '于', '向', '对', '被', '将', '把',
    ];
    leading_fragments.contains(&first) || trailing_fragments.contains(&last)
}

fn title_looks_like_mechanical_action_chain(core: &str) -> bool {
    let char_count = core.chars().count();
    if !(4..=8).contains(&char_count) {
        return false;
    }
    if title_has_story_entry_surface(core, core)
        || title_is_story_anchored_full_object(core, core)
        || (title_has_strong_reader_hook_surface(core)
            && story_object_suffixes()
                .iter()
                .any(|suffix| core.contains(*suffix)))
    {
        return false;
    }
    let action_char_count = core
        .chars()
        .filter(|ch| {
            matches!(
                ch,
                '借' | '换' | '破' | '修' | '证' | '夺' | '封' | '守' | '炼'
            )
        })
        .count();
    let has_generic_payoff = ["破局", "破案", "守城", "封门"]
        .iter()
        .any(|term| core.ends_with(*term));
    let payoff_prefix = ["破局", "破案", "守城", "封门"]
        .iter()
        .find_map(|term| core.strip_suffix(*term))
        .unwrap_or_default();
    let prefix_is_process_or_action_summary = !payoff_prefix.is_empty()
        && (title_mood_or_process_terms()
            .iter()
            .any(|term| payoff_prefix.contains(*term))
            || [
                "开辟", "建立", "制定", "改写", "重塑", "重建", "打破", "推翻", "完成", "实现",
            ]
            .iter()
            .any(|term| payoff_prefix.contains(*term)));
    let has_abstract_or_process = title_mood_or_process_terms()
        .iter()
        .any(|term| core.contains(*term))
        || ["修为", "法则", "因果", "命运", "力量", "潜力"]
            .iter()
            .any(|term| core.contains(*term));
    (has_generic_payoff && (action_char_count >= 2 || prefix_is_process_or_action_summary))
        || (has_abstract_or_process && action_char_count >= 2)
}

fn title_token_is_mood_or_process(token: &str) -> bool {
    title_mood_or_process_terms()
        .iter()
        .any(|term| token.contains(*term))
}

fn title_mood_or_process_terms() -> &'static [&'static str] {
    &[
        "霓虹", "余烬", "裂痕", "暗流", "迷雾", "风暴", "微光", "余温", "回响", "余响", "静默",
        "沉默", "感知", "频率", "阈值", "裂纹", "过载", "秩序", "混沌", "规则", "法则", "代价",
        "律法", "抉择", "真相", "觉醒", "坍塌", "重构", "重塑", "边界", "命运", "记忆", "灵魂",
        "断裂", "破碎", "故障", "失效", "扭曲", "墨染", "尘封", "常规", "流程", "步骤", "阶段",
        "方案", "计划",
    ]
}

fn title_basis_rationale_is_generic(rationale: &str) -> bool {
    let compact = rationale.trim();
    let lowered = compact.to_ascii_lowercase();
    if title_rationale_contains_internal_contract_slot_surface(compact) {
        return true;
    }
    let generic_relation = ["体现", "代表", "象征", "符合"]
        .iter()
        .any(|term| compact.contains(term))
        && ["题材", "风格", "气质", "主题"]
            .iter()
            .any(|term| compact.contains(term))
        && !title_rationale_has_concrete_payoff(compact);
    if generic_relation {
        return true;
    }
    [
        "取自当前故事的关键地点",
        "当前故事的关键地点、物件、制度、事件或终局选择",
        "取自当前合同里的关键地点",
        "当前合同里的关键地点",
        "关键地点、物件、制度漏洞、爽点行动或结局反转",
        "关键地点、物件、制度、事件、行动或结局反转",
        "连接到主线/终局",
        "需要连接主线",
        "需要连接到主线",
        "需要连接主线推进",
        "需要连接主线推进、终局兑现或读者爽点",
        "需要连接主线、终局或读者爽点",
        "需要连接主线推进、终局兑现",
        "需要连接终局兑现",
        "需要连接读者爽点",
        "体现主角成长",
        "体现成长",
        "体现故事主题",
        "体现主题",
        "符合题材",
        "符合风格",
        "象征命运",
        "象征成长",
        "故事气质",
        "作品气质",
        "来自本书已确认的具体终局",
        "来自本书已确认的具体终局、主线线索或世界规则",
        "来自本书已确认的具体终局、主线或世界规则",
        "本书已确认的具体终局、主线线索或世界规则",
        "对应本书已确认的核心意象或关键行动",
        "对应当前故事会兑现的具体转折",
        "终局/主线会兑现",
        "当前合同的终局、主线和世界规则",
        "当前合同的终局、主线或世界规则",
        "当前合同里的具体故事锚点",
        "以一部作为",
        "以一本作为",
        "以这部作为",
        "以这本作为",
        "work title",
        "story theme",
    ]
    .iter()
    .any(|term| compact.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn title_rationale_contains_internal_contract_slot_surface(rationale: &str) -> bool {
    let compact = rationale.replace(char::is_whitespace, "");
    internal_contract_slot_title_terms().iter().any(|term| {
        if *term == "角色权威表"
            && rationale_uses_authority_table_as_concrete_character_basis(&compact)
        {
            return false;
        }
        compact.contains(term)
    }) || [
        "书名候选",
        "候选书名",
        "hook_type",
        "canonical_title",
        "field_requirements",
        "contract_ready",
    ]
    .iter()
    .any(|term| compact.contains(term))
}

fn rationale_uses_authority_table_as_concrete_character_basis(rationale: &str) -> bool {
    rationale.contains("角色权威表")
        && title_rationale_has_concrete_payoff(rationale)
        && title_specific_tokens(rationale)
            .iter()
            .any(|token| cjk_token_looks_like_person_name(token))
}

fn title_basis_markers() -> &'static [&'static str] {
    &[
        "终局",
        "结局",
        "最终",
        "大纲",
        "情节",
        "剧情",
        "主线",
        "因果",
        "冲突",
        "反派",
        "对手",
        "代价",
        "选择",
        "不可逆",
        "兑现",
        "爽点",
        "卖点",
        "读者期待",
        "读者承诺",
        "世界观",
        "规则",
        "制度",
        "力量体系",
        "晋级",
        "考试",
        "关系",
        "情感",
        "欲望",
        "恐惧",
        "底线",
        "地点",
        "物件",
        "事件",
        "守护",
        "承担",
        "跨过",
        "重立",
        "公开",
    ]
}

fn story_specific_overlap_count(
    rationale: &str,
    story_evidence: &str,
    title_tokens: &[String],
) -> usize {
    if story_evidence.trim().is_empty() {
        return 0;
    }
    let mut seen = Vec::new();
    for token in title_tokens {
        if story_evidence.contains(token) && !seen.iter().any(|known| known == token) {
            seen.push(token.clone());
        }
    }
    for token in title_specific_tokens(rationale) {
        if story_evidence.contains(&token) && !seen.iter().any(|known| known == &token) {
            seen.push(token);
        }
    }
    seen.len()
}

fn reader_hook_terms() -> &'static [&'static str] {
    &[
        "我", "你", "他", "她", "城", "校", "院", "街", "巷", "塔", "门", "桥", "灯", "车", "刀",
        "剑", "火", "雨", "雪", "海", "星", "月", "神", "鬼", "妖", "魔", "龙", "狐", "灵", "符",
        "丹", "机", "芯", "网", "局", "案", "契", "卷", "碑", "钟", "镜", "骨", "血", "镇", "守",
        "斩", "杀", "破", "夺", "救", "赌", "逃", "追", "葬", "炼", "偷", "换", "买", "卖", "考",
        "审", "借", "证", "令", "债", "榜", "试", "祭", "坠", "焚", "醒", "开", "天阶", "涨停",
        "跌停", "盘口", "K线", "股市",
    ]
}

fn strong_reader_hook_terms() -> &'static [&'static str] {
    &[
        "城", "校", "院", "街", "巷", "塔", "门", "桥", "灯", "车", "刀", "剑", "火", "雨", "雪",
        "海", "星", "月", "神", "鬼", "妖", "魔", "龙", "狐", "灵", "符", "丹", "机", "芯", "网",
        "局", "案", "契", "卷", "证", "碑", "钟", "镜", "骨", "血", "镇", "守", "斩", "杀", "破",
        "夺", "救", "赌", "逃", "追", "葬", "炼", "偷", "换", "买", "卖", "考", "审", "借", "令",
        "债", "试", "祭", "坠", "焚", "醒", "开", "天阶", "涨停", "跌停", "盘口", "K线",
    ]
}

fn plot_action_hook_terms() -> &'static [&'static str] {
    &[
        "借",
        "偷",
        "换",
        "买",
        "卖",
        "赌",
        "逃",
        "追",
        "夺",
        "救",
        "斩",
        "杀",
        "破",
        "炼",
        "葬",
        "封",
        "祭",
        "考",
        "审",
        "证",
        "令",
        "债",
        "开",
        "破局",
        "入场",
        "补考",
        "试炼",
        "通关",
        "逆袭",
        "反杀",
        "开道",
        "开门",
        "开天门",
        "拆解",
        "公开",
        "归还",
        "夺回",
        "重开",
        "改写",
    ]
}

fn story_entry_terms() -> &'static [&'static str] {
    &[
        "学校",
        "学院",
        "考场",
        "试炼",
        "榜单",
        "证件",
        "入场券",
        "债契",
        "灵脉",
        "天阶",
        "地下城",
        "涨停板",
        "跌停板",
        "盘口",
        "K线",
        "股市",
        "金融街",
        "矿井",
        "禁区",
        "祭坛",
        "符文",
        "塔",
        "门",
        "桥",
        "街",
        "巷",
        "城",
        "校",
        "院",
        "证",
        "令",
        "契",
        "井",
        "榜",
    ]
}

fn abstract_title_concept_terms() -> &'static [&'static str] {
    &[
        "感知", "静默", "频率", "过载", "觉醒", "代价", "抉择", "真相", "裂痕", "裂纹", "余烬",
        "回响", "余响", "边界", "重构", "坍塌", "秩序", "混沌", "规则", "法则", "律令", "制度",
        "维度", "噪音", "阈值", "终局", "命运", "成长", "记忆", "梦境", "灵魂", "意识", "暗流",
        "迷雾", "风暴", "危机", "答案", "选择", "考验", "回归", "失衡", "平衡", "缄默", "刻度",
        "逻辑", "余温", "尺度", "阈限", "沉默", "静止", "回路", "灵能", "故障", "失效", "断裂",
        "破碎", "城市", "都市", "墨染", "尘封", "灵感", "证道", "长生", "飞升", "成仙", "大道",
        "控局", "布局", "掌控", "控制", "局面", "局势", "格局", "层级", "阶层", "资源", "资本",
        "权力", "承诺", "财富", "机遇", "商业", "版图",
    ]
}
