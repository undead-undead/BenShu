//! Chapter-title naming authority helpers.
//!
//! Chapter title callers should use this module for title surface parsing and
//! template detection. Metadata repair may still live in the studio/workflow
//! layers, but title-shape rules belong here.

use super::title_lexicon;

#[derive(Debug, Clone)]
pub(crate) struct ChapterTitleEvidence {
    pub(crate) language: String,
    pub(crate) summary: String,
    pub(crate) key_facts: Vec<String>,
    pub(crate) continuity_updates: Vec<String>,
    pub(crate) content: String,
}

impl ChapterTitleEvidence {
    pub(crate) fn new(
        language: impl Into<String>,
        summary: impl Into<String>,
        key_facts: Vec<String>,
        continuity_updates: Vec<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            language: language.into(),
            summary: summary.into(),
            key_facts,
            continuity_updates,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ChapterTitleContext {
    pub(crate) language: String,
    pub(crate) project_title: String,
    pub(crate) volume_titles: Vec<String>,
    pub(crate) other_chapter_titles: Vec<(usize, String)>,
    pub(crate) character_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChapterTitleCandidate {
    pub(crate) title: String,
}

impl ChapterTitleCandidate {
    pub(crate) fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ChapterTitleDecision {
    pub(crate) accepted: bool,
    pub(crate) repairable: bool,
    pub(crate) selected: Option<ChapterTitleCandidate>,
    pub(crate) reasons: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChapterTitleSurfaceIssue {
    Empty,
    LanguageMismatch,
    DefaultHeading,
    WeakSignal,
    ProjectOrVolumeDuplicate,
    ChapterDuplicate,
    ProseFragment,
    PredicateFragment,
}

fn chapter_title_surface_issue(
    context: &ChapterTitleContext,
    number: usize,
    title: &str,
) -> Option<ChapterTitleSurfaceIssue> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Some(ChapterTitleSurfaceIssue::Empty);
    }
    if is_chinese_language(&context.language) && !trimmed.chars().any(is_cjk_unified) {
        return Some(ChapterTitleSurfaceIssue::LanguageMismatch);
    }
    if title_is_default_chapter_heading(trimmed, number, &context.language) {
        return Some(ChapterTitleSurfaceIssue::DefaultHeading);
    }
    if !title_has_enough_signal(trimmed) {
        return Some(ChapterTitleSurfaceIssue::WeakSignal);
    }
    if title_matches_project_or_volume(context, trimmed) {
        return Some(ChapterTitleSurfaceIssue::ProjectOrVolumeDuplicate);
    }
    if title_matches_other_chapter(context, number, trimmed)
        || title_is_too_similar_to_other_chapter(context, number, trimmed)
    {
        return Some(ChapterTitleSurfaceIssue::ChapterDuplicate);
    }
    if internal_process_title_label(trimmed) || incomplete_character_clause(context, trimmed) {
        return Some(ChapterTitleSurfaceIssue::ProseFragment);
    }
    if title_looks_like_body_nominal_fragment(trimmed)
        || title_looks_like_body_adverbial_predicate_fragment(trimmed)
        || title_looks_like_place_fragment(trimmed)
        || title_looks_like_temporal_prose_fragment(trimmed)
        || title_looks_like_causal_clause_fragment(trimmed)
        || title_looks_like_quantity_statement_fragment(trimmed)
        || sentence_fragment_edge(trimmed)
        || prose_grammar_fragment(&chapter_title_core(trimmed))
    {
        return Some(ChapterTitleSurfaceIssue::ProseFragment);
    }
    if local_chapter_title_candidate_is_predicate_fragment(trimmed) {
        return Some(ChapterTitleSurfaceIssue::PredicateFragment);
    }
    None
}

pub(crate) fn chapter_title_core(title: &str) -> String {
    let mut value = title.trim().trim_matches('"').trim().to_string();
    if let Some(rest) = value.strip_prefix('#') {
        value = rest.trim().to_string();
    }
    value = strip_structural_title_prefix(&value, '卷');
    value = strip_english_volume_prefix(&value);
    if let Some((index, ch)) = value.char_indices().find(|(_, ch)| *ch == '章') {
        let end = index + ch.len_utf8();
        let prefix_len = value[..end].chars().count();
        if prefix_len <= 8 {
            value = value[end..].to_string();
        }
    }
    value
        .trim_start_matches(|ch: char| {
            matches!(ch, ':' | '：' | '-' | '—' | ' ' | '\t' | '、' | '.')
        })
        .trim()
        .to_string()
}

fn strip_structural_title_prefix(value: &str, marker: char) -> String {
    let trimmed = value.trim();
    let Some((index, ch)) = trimmed.char_indices().find(|(_, ch)| *ch == marker) else {
        return trimmed.to_string();
    };
    let end = index + ch.len_utf8();
    let prefix = &trimmed[..end];
    let prefix_len = prefix.chars().count();
    if prefix_len <= 8 && prefix.starts_with('第') {
        return trimmed[end..]
            .trim_start_matches(|ch: char| {
                matches!(ch, ':' | '：' | '-' | '—' | ' ' | '\t' | '、' | '.')
            })
            .trim()
            .to_string();
    }
    trimmed.to_string()
}

fn strip_english_volume_prefix(value: &str) -> String {
    let trimmed = value.trim();
    let lowered = trimmed.to_ascii_lowercase();
    for prefix in ["volume ", "vol. "] {
        let Some(rest) = lowered.strip_prefix(prefix) else {
            continue;
        };
        let consumed = trimmed.len() - rest.len();
        let original_rest = &trimmed[consumed..];
        let Some((split_at, split_len)) = original_rest
            .char_indices()
            .find_map(|(idx, ch)| matches!(ch, ':' | '-' | '—').then_some((idx, ch.len_utf8())))
        else {
            continue;
        };
        let number_part = original_rest[..split_at].trim();
        if !number_part.is_empty()
            && number_part
                .chars()
                .all(|ch| ch.is_ascii_digit() || ch.is_ascii_alphabetic() || ch == ' ')
        {
            return original_rest[split_at + split_len..].trim().to_string();
        }
    }
    trimmed.to_string()
}

pub(crate) fn chapter_title_template(core: &str) -> String {
    let mut template = String::new();
    let mut in_cjk_run = false;
    for ch in core.chars() {
        if is_cjk_unified(ch) {
            if title_template_connector(ch) {
                if in_cjk_run {
                    template.push('X');
                    in_cjk_run = false;
                }
                template.push(ch);
            } else {
                in_cjk_run = true;
            }
        } else if in_cjk_run {
            template.push('X');
            in_cjk_run = false;
        }
    }
    if in_cjk_run {
        template.push('X');
    }
    let signal_connectors = template
        .chars()
        .filter(|ch| title_template_connector(*ch))
        .count();
    if signal_connectors == 0 {
        String::new()
    } else {
        template
    }
}

pub(crate) fn title_template_connector(ch: char) -> bool {
    matches!(
        ch,
        '的' | '之' | '中' | '下' | '上' | '里' | '间' | '边' | '后' | '前'
    )
}

pub(crate) fn generic_stage_label(title: &str) -> bool {
    let core = chapter_title_core(title);
    if core.is_empty() {
        return true;
    }
    let compact = core
        .chars()
        .filter(|ch| !matches!(ch, '第' | '章' | '：' | ':' | ' ' | '\t'))
        .collect::<String>();
    if compact.chars().count() <= 2 {
        return true;
    }
    title_lexicon::generic_fiction_chapter_title_terms()
        .iter()
        .any(|term| compact == *term)
}

pub(crate) fn sentence_fragment_edge(candidate: &str) -> bool {
    let mut chars = candidate.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    let last = chars.last().unwrap_or(first);
    matches!(
        first,
        '着' | '了'
            | '在'
            | '与'
            | '和'
            | '及'
            | '而'
            | '并'
            | '或'
            | '但'
            | '却'
            | '又'
            | '将'
            | '被'
            | '把'
            | '为'
            | '以'
            | '从'
            | '向'
            | '对'
            | '于'
            | '出'
            | '入'
            | '进'
            | '回'
            | '到'
            | '让'
            | '使'
            | '给'
            | '住'
    ) || matches!(
        last,
        '着' | '了'
            | '的'
            | '之'
            | '顺'
            | '沿'
            | '没'
            | '不'
            | '无'
            | '是'
            | '有'
            | '会'
            | '要'
            | '能'
            | '已'
            | '就'
            | '完'
            | '共'
            | '在'
            | '与'
            | '和'
            | '及'
            | '而'
            | '并'
            | '或'
            | '将'
            | '把'
            | '被'
            | '为'
            | '以'
            | '从'
            | '向'
            | '对'
            | '于'
            | '似'
            | '像'
            | '如'
            | '若'
            | '般'
    )
}

pub(crate) fn prose_grammar_fragment(core: &str) -> bool {
    let chars = core.chars().collect::<Vec<_>>();
    if chars.len() < 3 {
        return false;
    }
    let trimmed = core.trim();
    let comparative_openers = [
        "仿佛", "好像", "似乎", "犹如", "宛如", "仿若", "恍若", "如同", "像是",
    ];
    if comparative_openers
        .iter()
        .any(|opener| trimmed.starts_with(opener))
    {
        return true;
    }
    let comparative_markers = [
        "仿佛", "好像", "似乎", "犹如", "宛如", "仿若", "恍若", "如同", "像是", "比",
    ];
    if comparative_markers.iter().any(|marker| {
        trimmed
            .find(marker)
            .is_some_and(|index| index > 0 && trimmed[index + marker.len()..].chars().count() <= 3)
    }) {
        return true;
    }
    if comparative_quantity_tail_fragment(trimmed) {
        return true;
    }
    if incomplete_comparative_or_state_fragment(trimmed) {
        return true;
    }
    if connective_clause_fragment(trimmed) {
        return true;
    }
    if bare_measure_noun_fragment(trimmed) {
        return true;
    }
    if bare_body_action_phrase(&chars) {
        return true;
    }
    if pronoun_body_action_phrase(&chars) {
        return true;
    }
    if object_body_state_clause_fragment(&chars) {
        return true;
    }
    if narrative_predicate_phrase_fragment(trimmed) {
        return true;
    }
    if short_subject_predicate_fragment(trimmed) {
        return true;
    }
    if short_copula_or_degree_prefix_fragment(trimmed) {
        return true;
    }
    if short_temporal_action_tail_fragment(trimmed) {
        return true;
    }
    if short_adjective_connector_fragment(trimmed) {
        return true;
    }
    if aspect_particle_body_fragment(&chars) {
        return true;
    }
    let predicate_fragment_tails = [
        "来得", "显得", "变得", "显现", "出现", "开始", "继续", "正在", "仍在", "即将", "已经",
        "终于", "依旧", "仍旧", "并未", "并非", "尚未", "仍未", "还未", "未能", "没有", "不再",
        "不能", "不会",
    ];
    if predicate_fragment_tails
        .iter()
        .any(|tail| trimmed.ends_with(tail))
    {
        return true;
    }
    let predicate_fragment_markers = [
        "总是", "往往", "常常", "一直", "仍然", "依然", "依旧", "尚且", "正在", "已经",
    ];
    if predicate_fragment_markers.iter().any(|marker| {
        trimmed.find(marker).is_some_and(|index| {
            let left_len = trimmed[..index].chars().count();
            let right_len = trimmed[index + marker.len()..].chars().count();
            left_len > 0 && right_len <= 3 && chars.len() <= 7
        })
    }) {
        return true;
    }
    let grammar_markers = [
        '在', '把', '被', '将', '让', '使', '给', '向', '对', '从', '为',
    ];
    for (index, ch) in chars.iter().enumerate() {
        if grammar_markers.contains(ch) && index > 0 {
            let right_len = chars.len().saturating_sub(index + 1);
            return right_len <= 3 || chars.len() <= 6;
        }
    }
    false
}

fn incomplete_comparative_or_state_fragment(text: &str) -> bool {
    let len = text.chars().count();
    if !(3..=8).contains(&len) || !text.chars().all(is_cjk_unified) {
        return false;
    }
    if let Some(index) = text.find('比') {
        let left_len = text[..index].chars().count();
        let right = text[index + '比'.len_utf8()..].trim();
        let right_len = right.chars().count();
        let temporal_or_degree_tail = [
            "昨", "昨日", "昨天", "今", "今日", "今天", "前", "从前", "以往", "往昔", "更", "更加",
            "还", "仍", "仍旧", "依旧",
        ];
        if left_len > 0
            && (right_len <= 2 || temporal_or_degree_tail.iter().any(|tail| right == *tail))
        {
            return true;
        }
    }
    false
}

fn comparative_quantity_tail_fragment(text: &str) -> bool {
    let Some(index) = text.find('像') else {
        return false;
    };
    if index == 0 {
        return false;
    }
    let tail = text[index + '像'.len_utf8()..].trim();
    if tail.is_empty() || tail.chars().count() > 4 {
        return false;
    }
    let quantity_tails = [
        "无数", "千万", "万千", "许多", "很多", "一道", "一条", "一把", "一阵", "一片", "一座",
        "一张", "一根", "一束", "一团", "一层", "几道", "几条", "几座",
    ];
    quantity_tails.iter().any(|prefix| tail.starts_with(prefix))
}

fn connective_clause_fragment(text: &str) -> bool {
    let len = text.chars().count();
    if len > 8 || !text.chars().all(is_cjk_unified) {
        return false;
    }
    let clause_openers = [
        "由于", "因为", "如果", "虽然", "但是", "只是", "所以", "因此", "于是", "然后", "随后",
        "同时", "为了", "通过", "关于", "对于", "面对", "当他", "当她", "当它", "当那",
    ];
    clause_openers
        .iter()
        .any(|opener| text.starts_with(opener) && text[opener.len()..].chars().count() <= 5)
}

fn bare_measure_noun_fragment(text: &str) -> bool {
    let len = text.chars().count();
    if len > 6 || !text.chars().all(is_cjk_unified) {
        return false;
    }
    let measure_prefixes = [
        '个', '位', '名', '枚', '块', '张', '道', '条', '层', '片', '缕', '声', '股', '阵', '场',
        '座', '扇', '间',
    ];
    if text
        .chars()
        .next()
        .is_some_and(|first| measure_prefixes.contains(&first))
    {
        return true;
    }
    let fragments = [
        "座城",
        "座城市",
        "座楼",
        "座山",
        "座桥",
        "座塔",
        "个城市",
        "个秘密",
        "个选择",
        "条街",
        "条路",
        "扇门",
        "间房",
    ];
    fragments.iter().any(|fragment| text == *fragment)
}

fn object_body_state_clause_fragment(chars: &[char]) -> bool {
    if chars.len() > 8 {
        return false;
    }
    chars.iter().enumerate().any(|(index, ch)| {
        *ch == '身'
            && index > 0
            && index + 1 < chars.len()
            && matches!(
                chars[index + 1],
                '泛' | '闪'
                    | '亮'
                    | '发'
                    | '散'
                    | '涌'
                    | '浮'
                    | '覆'
                    | '罩'
                    | '裹'
                    | '燃'
                    | '震'
                    | '颤'
            )
    })
}

fn narrative_predicate_phrase_fragment(text: &str) -> bool {
    if text.chars().count() > 6 {
        return false;
    }
    let markers = [
        "看见", "听见", "望见", "发现", "意识", "确认", "记住", "明白", "知道", "想到", "触碰",
        "踏入", "走进", "回到", "看向",
    ];
    markers.iter().any(|marker| {
        let Some(index) = text.find(marker) else {
            return false;
        };
        let left_len = text[..index].chars().count();
        let right_len = text[index + marker.len()..].chars().count();
        (left_len > 0 && left_len <= 2 && right_len <= 3)
            || (left_len == 0 && (1..=2).contains(&right_len))
    })
}

fn short_subject_predicate_fragment(text: &str) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    if !(3..=8).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    for marker in ['是', '成', '为'] {
        let Some(index) = chars.iter().position(|ch| *ch == marker) else {
            continue;
        };
        if index == 0 || index + 1 >= chars.len() {
            continue;
        }
        let left_len = index;
        let right_len = chars.len().saturating_sub(index + 1);
        if left_len <= 2 && right_len <= 4 && !title_has_story_action_or_clue_surface(text) {
            return true;
        }
    }
    false
}

fn short_copula_or_degree_prefix_fragment(text: &str) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    if !(3..=8).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    if title_has_story_action_or_clue_surface(text) {
        return false;
    }
    let copula_prefixes = ["是", "并非", "不是", "没有", "仍是", "仍然是"];
    if copula_prefixes
        .iter()
        .any(|prefix| text.starts_with(prefix))
    {
        return true;
    }
    let degree_prefixes = ["最", "更", "更加", "极其", "非常", "格外"];
    if degree_prefixes
        .iter()
        .any(|prefix| text.starts_with(prefix))
    {
        return true;
    }
    let degree_markers = ['最', '更'];
    chars
        .iter()
        .enumerate()
        .any(|(index, ch)| index > 0 && degree_markers.contains(ch) && chars.len() <= index + 3)
}

fn short_temporal_action_tail_fragment(text: &str) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    if !(3..=8).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    let temporal_tail = ["后", "之后", "以后", "前", "之前", "以前", "时", "时候"];
    if !temporal_tail.iter().any(|tail| text.ends_with(tail)) {
        return false;
    }
    let action_markers = [
        "离开", "回来", "返回", "走出", "进入", "抵达", "醒来", "发现", "看见", "听见", "确认",
        "知道", "明白", "说完", "放下", "转身",
    ];
    action_markers.iter().any(|marker| text.contains(marker))
}

fn short_adjective_connector_fragment(text: &str) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    if !(3..=8).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    let Some(index) = chars.iter().position(|ch| *ch == '而') else {
        return false;
    };
    if index == 0 || index + 1 >= chars.len() || title_has_story_action_or_clue_surface(text) {
        return false;
    }
    let left = chars[..index].iter().collect::<String>();
    let right = chars[index + 1..].iter().collect::<String>();
    let adjective_surfaces = [
        "沉", "重", "稠", "冷", "热", "深", "浅", "暗", "亮", "静", "急", "慢", "轻", "硬", "软",
        "空", "满", "密", "远", "近", "旧", "新",
    ];
    !left.is_empty()
        && !right.is_empty()
        && left
            .chars()
            .all(|ch| adjective_surfaces.contains(&ch.to_string().as_str()))
        && right
            .chars()
            .all(|ch| adjective_surfaces.contains(&ch.to_string().as_str()))
}

fn aspect_particle_body_fragment(chars: &[char]) -> bool {
    if chars.len() > 8 {
        return false;
    }
    chars.iter().enumerate().any(|(index, ch)| {
        matches!(ch, '着' | '了' | '过')
            && index > 0
            && index + 1 < chars.len()
            && chars[index + 1..].len() <= 4
    })
}

pub(crate) fn registry_issues(
    current_number: usize,
    current_title: &str,
    other_titles: impl IntoIterator<Item = (usize, String)>,
) -> Vec<String> {
    let mut issues = Vec::new();
    let title_key = normalized_title_key(current_title);
    if title_key.is_empty() || !title_has_enough_signal(current_title) {
        return issues;
    }
    let current_lookup = normalize_title_lookup_key(current_title);
    for (other_number, other_title) in other_titles {
        if other_number == current_number {
            continue;
        }
        let other_key = normalized_title_key(&other_title);
        if other_key.is_empty() {
            continue;
        }
        if title_key == other_key {
            issues.push(format!(
                "chapter title duplicates chapter {other_number}: {other_title}"
            ));
        } else {
            let score =
                title_similarity(&current_lookup, &normalize_title_lookup_key(&other_title));
            if score >= 0.86 || short_titles_share_long_core_fragment(&current_lookup, &other_key) {
                issues.push(format!(
                    "chapter title is too similar to chapter {other_number}: {other_title}"
                ));
            }
        }
    }
    issues
}

pub(crate) fn fatigue_issues(
    language: &str,
    current_number: usize,
    current_title: &str,
    recent_prior_titles: impl IntoIterator<Item = (usize, String)>,
) -> Vec<String> {
    if !is_chinese_language(language) || !title_has_enough_signal(current_title) {
        return Vec::new();
    }
    let current_core = chapter_title_core(current_title);
    let current_template = chapter_title_template(&current_core);
    let mut prior = recent_prior_titles
        .into_iter()
        .filter(|(number, title)| *number < current_number && title_has_enough_signal(title))
        .collect::<Vec<_>>();
    prior.reverse();
    let recent_titles = prior.into_iter().take(4).collect::<Vec<_>>();
    let recent = recent_titles
        .iter()
        .take(4)
        .filter_map(|(number, title)| {
            let core = chapter_title_core(&title);
            let template = chapter_title_template(&core);
            (!template.is_empty()).then_some((*number, title.clone(), template))
        })
        .collect::<Vec<_>>();

    if !current_template.is_empty() {
        let same_template = recent
            .iter()
            .filter(|(_, _, template)| *template == current_template)
            .collect::<Vec<_>>();
        if !same_template.is_empty() {
            let examples = same_template
                .iter()
                .take(3)
                .map(|(number, title, _)| format!("chapter {number}: {title}"))
                .collect::<Vec<_>>()
                .join("; ");
            return vec![format!(
                "chapter title repeats the recent syntactic template `{current_template}` too often; derive this title from the chapter's unique event instead of reusing the same wording shape ({examples})"
            )];
        }
    }

    let current_rhythm = chapter_title_rhythm_signature(&current_core);
    let current_tokens = story_tokens(&current_core);
    let current_polarity = chapter_title_polarity_cadence(&current_core);
    if !current_rhythm.is_empty() {
        let repeated_rhythm = recent
            .iter()
            .filter_map(|(number, title, _)| {
                let core = chapter_title_core(title);
                let rhythm = chapter_title_rhythm_signature(&core);
                if rhythm != current_rhythm {
                    return None;
                }
                let prior_tokens = story_tokens(&core);
                let shared = current_tokens
                    .iter()
                    .filter(|token| prior_tokens.iter().any(|prior| prior == *token))
                    .cloned()
                    .collect::<Vec<_>>();
                let same_polarity = same_title_polarity_cadence(&current_polarity, &core);
                (!shared.is_empty() || same_polarity).then_some((*number, title.clone(), shared))
            })
            .collect::<Vec<_>>();
        let repeated_rhythm = if repeated_rhythm.is_empty() {
            recent_titles
                .iter()
                .filter_map(|(number, title)| {
                    let core = chapter_title_core(title);
                    let rhythm = chapter_title_rhythm_signature(&core);
                    if rhythm != current_rhythm {
                        return None;
                    }
                    let prior_tokens = story_tokens(&core);
                    let shared = current_tokens
                        .iter()
                        .filter(|token| prior_tokens.iter().any(|prior| prior == *token))
                        .cloned()
                        .collect::<Vec<_>>();
                    let same_polarity = same_title_polarity_cadence(&current_polarity, &core);
                    (!shared.is_empty() || same_polarity).then_some((
                        *number,
                        title.clone(),
                        shared,
                    ))
                })
                .collect::<Vec<_>>()
        } else {
            repeated_rhythm
        };
        if !repeated_rhythm.is_empty() {
            let examples = repeated_rhythm
                .iter()
                .take(3)
                .map(|(number, title, shared)| {
                    if shared.is_empty() {
                        format!("chapter {number}: {title} (cadence match)")
                    } else {
                        format!("chapter {number}: {title} (shared: {})", shared.join("/"))
                    }
                })
                .collect::<Vec<_>>()
                .join("; ");
            return vec![format!(
                "chapter title repeats a recent punctuation rhythm and story phrase; derive this title from the chapter's unique event instead of reusing the same cadence ({examples})"
            )];
        }
    }

    Vec::new()
}

fn same_title_polarity_cadence(current_polarity: &str, prior_core: &str) -> bool {
    !current_polarity.is_empty()
        && title_cadence_has_polarity_signal(current_polarity)
        && current_polarity == chapter_title_polarity_cadence(prior_core)
}

fn chapter_title_polarity_cadence(core: &str) -> String {
    let parts = core
        .split(|ch| matches!(ch, '，' | ',' | '；' | ';' | '：' | ':' | '、' | '/' | '／'))
        .map(|part| {
            if part
                .chars()
                .any(|ch| matches!(ch, '无' | '未' | '不' | '没' | '非'))
            {
                "neg"
            } else if part
                .chars()
                .any(|ch| matches!(ch, '有' | '成' | '启' | '开' | '破' | '归'))
            {
                "pos"
            } else {
                "plain"
            }
        })
        .collect::<Vec<_>>();
    if parts.len() >= 2 {
        parts.join("/")
    } else {
        String::new()
    }
}

fn title_cadence_has_polarity_signal(signature: &str) -> bool {
    signature.contains("neg") || signature.contains("pos")
}

fn chapter_title_rhythm_signature(core: &str) -> String {
    let mut signature = String::new();
    let mut cjk_run = 0usize;
    for ch in core.chars() {
        if is_cjk_unified(ch) {
            cjk_run += 1;
            continue;
        }
        if cjk_run > 0 {
            signature.push_str(&cjk_run.to_string());
            cjk_run = 0;
        }
        if matches!(ch, '，' | ',' | '；' | ';' | '：' | ':' | '、' | '/' | '／') {
            signature.push(ch);
        }
    }
    if cjk_run > 0 {
        signature.push_str(&cjk_run.to_string());
    }
    let has_separator = signature
        .chars()
        .any(|ch| matches!(ch, '，' | ',' | '；' | ';' | '：' | ':' | '、' | '/' | '／'));
    if has_separator {
        signature
    } else {
        String::new()
    }
}

pub(crate) fn has_story_evidence(
    language: &str,
    title: &str,
    summary: &str,
    key_facts: &[String],
    continuity_updates: &[String],
    content: &str,
) -> bool {
    if !is_chinese_language(language) {
        return title_has_enough_signal(title);
    }
    let core = chapter_title_core(title);
    if core.is_empty() || sentence_fragment_edge(&core) || prose_grammar_fragment(&core) {
        return false;
    }
    let tokens = story_tokens(&core);
    if tokens.is_empty() {
        return false;
    }
    let evidence = format!(
        "{}\n{}\n{}\n{}",
        summary,
        key_facts.join("\n"),
        continuity_updates.join("\n"),
        preview_chars(content, 2600)
    );
    let anchored_segments = core
        .split(|ch| matches!(ch, '的' | '之' | '与' | '和' | '及' | '、' | '：' | ':'))
        .map(str::trim)
        .filter(|segment| segment.chars().count() >= 2)
        .collect::<Vec<_>>();
    if anchored_segments.len() >= 2 {
        return anchored_segments.iter().all(|segment| {
            let segment_tokens = story_tokens(segment);
            !segment_tokens.is_empty()
                && segment_tokens.iter().any(|token| evidence.contains(token))
        });
    }
    tokens.iter().any(|token| evidence.contains(token))
}

pub(crate) fn evaluate_chapter_title_candidate(
    candidate: ChapterTitleCandidate,
    evidence: &ChapterTitleEvidence,
) -> ChapterTitleDecision {
    let title = candidate.title.trim();
    if title.is_empty() {
        return ChapterTitleDecision {
            accepted: false,
            repairable: true,
            selected: None,
            reasons: vec!["chapter title is empty".to_string()],
            warnings: Vec::new(),
        };
    }
    if generic_stage_label(title) {
        return ChapterTitleDecision {
            accepted: false,
            repairable: true,
            selected: None,
            reasons: vec!["chapter title is a generic stage label".to_string()],
            warnings: Vec::new(),
        };
    }
    if !has_story_evidence(
        &evidence.language,
        title,
        &evidence.summary,
        &evidence.key_facts,
        &evidence.continuity_updates,
        &evidence.content,
    ) {
        return ChapterTitleDecision {
            accepted: false,
            repairable: true,
            selected: None,
            reasons: vec!["chapter title is not grounded in chapter evidence".to_string()],
            warnings: Vec::new(),
        };
    }
    ChapterTitleDecision {
        accepted: true,
        repairable: false,
        selected: Some(ChapterTitleCandidate {
            title: title.to_string(),
        }),
        reasons: Vec::new(),
        warnings: Vec::new(),
    }
}

pub(crate) fn select_final_chapter_title_from_body(
    context: &ChapterTitleContext,
    number: usize,
    requested_title: &str,
    _summary: &str,
    content: &str,
) -> ChapterTitleDecision {
    let normalized_requested = chapter_title_core(requested_title);
    let requested = normalized_requested.trim();
    let body_evidence = content_without_leading_markdown_heading(content);
    if !chapter_title_needs_post_body_repair(context, number, requested)
        && title_body_fragment_issue(&context.language, requested, &body_evidence).is_none()
    {
        return ChapterTitleDecision {
            accepted: true,
            repairable: false,
            selected: Some(ChapterTitleCandidate::new(requested)),
            reasons: Vec::new(),
            warnings: Vec::new(),
        };
    }

    let placeholder = if is_chinese_language(&context.language) {
        format!("第{number}章")
    } else {
        format!("Chapter {number}")
    };
    ChapterTitleDecision {
        accepted: true,
        repairable: true,
        selected: Some(ChapterTitleCandidate::new(placeholder)),
        reasons: vec![
            "chapter title requires metadata-only model repair; body text must remain unchanged"
                .to_string(),
        ],
        warnings: Vec::new(),
    }
}

pub(crate) fn title_body_fragment_issue(
    language: &str,
    title: &str,
    content: &str,
) -> Option<String> {
    let core = chapter_title_core(title);
    if !is_chinese_language(language)
        || core.chars().count() < 3
        || !core.chars().all(is_cjk_unified)
        || content.trim().is_empty()
    {
        return None;
    }
    let body = content_without_leading_markdown_heading(content);
    if candidate_is_prefix_of_longer_cjk_compound(&core, &body)
        || candidate_is_embedded_in_longer_cjk_run(&core, &body)
        || candidate_has_detached_numeric_classifier_prefix(&core, &body)
    {
        return Some(
            "chapter title is a prose fragment embedded in the chapter body; repair title from the chapter's completed event, object, place, or irreversible change"
                .to_string(),
        );
    }
    None
}

fn content_without_leading_markdown_heading(content: &str) -> String {
    let mut lines = content.lines().peekable();
    while lines.peek().is_some_and(|line| line.trim().is_empty()) {
        lines.next();
    }
    if lines
        .peek()
        .is_some_and(|line| line.trim_start().starts_with('#'))
    {
        lines.next();
        while lines.peek().is_some_and(|line| line.trim().is_empty()) {
            lines.next();
        }
    }
    lines.collect::<Vec<_>>().join("\n")
}

pub(crate) fn chapter_title_needs_post_body_repair(
    context: &ChapterTitleContext,
    number: usize,
    title: &str,
) -> bool {
    chapter_title_surface_issue(context, number, title).is_some()
}

fn title_looks_like_quantity_statement_fragment(title: &str) -> bool {
    let core = chapter_title_core(title);
    let chars = core.chars().collect::<Vec<_>>();
    if !(3..=8).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    let quantifiers = [
        '个', '位', '名', '枚', '块', '张', '道', '条', '层', '重', '次', '年', '月', '日', '章',
        '卷', '步', '招', '场',
    ];
    let Some(prefix_index) = chars
        .iter()
        .position(|ch| matches!(ch, '共' | '约' | '逾' | '满' | '近'))
    else {
        return false;
    };
    let suffix = &chars[prefix_index + 1..];
    let Some(classifier_index) = suffix.iter().position(|ch| quantifiers.contains(ch)) else {
        return false;
    };
    if !suffix[..classifier_index]
        .iter()
        .any(|ch| cjk_numeric_char(*ch))
    {
        return false;
    }
    suffix[classifier_index + 1..].len() <= 1
}

fn title_looks_like_temporal_prose_fragment(title: &str) -> bool {
    let core = chapter_title_core(title);
    if core.chars().count() < 4 {
        return false;
    }
    [
        "的那一刻",
        "的这一刻",
        "的瞬间",
        "那一刻",
        "这一刻",
        "一瞬间",
        "片刻后",
        "下一刻",
    ]
    .iter()
    .any(|fragment| core.contains(fragment) || core.ends_with(fragment))
}

fn title_looks_like_causal_clause_fragment(title: &str) -> bool {
    let core = chapter_title_core(title);
    let chars = core.chars().collect::<Vec<_>>();
    if !(4..=8).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '因' || index == 0 || index + 1 >= chars.len() {
            continue;
        }
        let suffix = chars[index + 1..].iter().collect::<String>();
        if suffix.chars().count() <= 4 && causal_fragment_suffix_looks_unresolved(&suffix) {
            return true;
        }
    }
    false
}

fn causal_fragment_suffix_looks_unresolved(suffix: &str) -> bool {
    let action_chars = [
        '破', '夺', '斩', '救', '逃', '战', '契', '启', '封', '裂', '赌', '换', '炼', '证', '拒',
        '买', '卖', '赚', '赢', '败', '醒', '归', '守',
    ];
    if suffix.chars().any(|ch| action_chars.contains(&ch)) {
        return false;
    }
    let unresolved_heads = [
        "资金",
        "债务",
        "压力",
        "危机",
        "合同",
        "账目",
        "线索",
        "误会",
        "身份",
        "命令",
        "交易",
        "回购",
        "价格",
        "估值",
        "资金链",
    ];
    unresolved_heads
        .iter()
        .any(|head| suffix.starts_with(head) || suffix == *head)
}

fn cjk_numeric_char(ch: char) -> bool {
    ch.is_ascii_digit()
        || matches!(
            ch,
            '零' | '一'
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
                | '万'
                | '亿'
                | '两'
        )
}

fn local_chapter_title_candidate_is_predicate_fragment(candidate: &str) -> bool {
    let core = chapter_title_core(candidate);
    if core.chars().count() < 3 || !core.chars().all(is_cjk_unified) {
        return false;
    }
    let modifier_tails = [
        "完全", "不同", "真正", "已经", "正在", "仍然", "只是", "终于", "开始", "继续", "再次",
        "没有", "无法", "不能", "不会", "需要", "必须", "显得", "变得", "成为", "仍在", "即将",
        "依旧", "仍旧", "并未", "并非", "尚未", "仍未", "还未", "未能", "不再",
    ];
    modifier_tails.iter().any(|tail| core.ends_with(tail))
        || cjk_prose_predicate_fragment(&core)
        || cjk_short_clause_fragment(&core)
}

fn cjk_prose_predicate_fragment(core: &str) -> bool {
    let predicate_markers = [
        "传来", "传出", "响起", "浮现", "涌出", "泛起", "落下", "掠过", "裂开", "震颤", "升起",
        "垂下", "滑落", "逼近", "裂出", "浮出", "发现", "发出", "看见", "听见", "感到", "变成",
        "成为",
    ];
    if predicate_markers.iter().any(|marker| core.contains(marker)) {
        return true;
    }
    let adverb_prefixes = [
        "突然", "忽然", "猛地", "骤然", "顿时", "随即", "很快", "终于", "立刻", "转眼",
    ];
    let predicate_chars = [
        '来', '出', '起', '落', '开', '动', '响', '裂', '涌', '泛', '逼',
    ];
    if adverb_prefixes
        .iter()
        .any(|prefix| core.starts_with(prefix))
        && core.chars().any(|ch| predicate_chars.contains(&ch))
    {
        return true;
    }
    if cjk_modal_predicate_clause_fragment(core) {
        return true;
    }
    if cjk_adverbial_predicate_fragment(core) {
        return true;
    }
    let quantity_fragments = [
        "一阵", "一道", "一股", "一声", "一缕", "一层", "一点", "一抹", "一丝", "一片",
    ];
    quantity_fragments
        .iter()
        .any(|fragment| core.ends_with(fragment))
}

fn cjk_short_clause_fragment(core: &str) -> bool {
    let chars = core.chars().collect::<Vec<_>>();
    if !(3..=8).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    let compact = chars.iter().collect::<String>();
    let incomplete_aspect_prefixes = [
        "还没",
        "还没有",
        "尚未",
        "仍未",
        "还未",
        "未曾",
        "正在",
        "仍在",
        "已经",
        "变得",
        "显得",
        "成为",
        "变为",
    ];
    if incomplete_aspect_prefixes.iter().any(|prefix| {
        compact.starts_with(prefix) && compact.chars().count() > prefix.chars().count()
    }) {
        return true;
    }
    if [
        "那是", "这是", "它是", "他是", "她是", "他们", "她们", "它们",
    ]
    .iter()
    .any(|prefix| compact.starts_with(prefix))
    {
        return true;
    }
    if compact.starts_with('是')
        && !["是非", "是夜"]
            .iter()
            .any(|lexical_head| compact.starts_with(lexical_head))
    {
        return true;
    }
    if ["以来", "之后", "之前", "以后", "当中"]
        .iter()
        .any(|suffix| compact.ends_with(suffix))
    {
        return true;
    }
    let adverbial_prefixes = [
        "微微", "缓缓", "慢慢", "轻轻", "隐隐", "渐渐", "悄悄", "默默", "忽忽",
    ];
    if adverbial_prefixes
        .iter()
        .any(|prefix| compact.starts_with(prefix))
        && chars.iter().skip(2).any(|ch| {
            matches!(
                ch,
                '动' | '响' | '流' | '落' | '升' | '沉' | '亮' | '暗' | '开' | '出'
            )
        })
    {
        return true;
    }
    false
}

fn cjk_adverbial_predicate_fragment(core: &str) -> bool {
    let chars = core.chars().collect::<Vec<_>>();
    if !(3..=8).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    let Some(index) = chars.iter().position(|ch| *ch == '地') else {
        return false;
    };
    if index == 0 || index + 1 >= chars.len() {
        return false;
    }
    let prefix = &chars[..index];
    let suffix = &chars[index + 1..];
    if prefix.len() == 2 && prefix[0] == prefix[1] && (1..=3).contains(&suffix.len()) {
        return true;
    }
    let predicate_tails = [
        '扎', '刺', '落', '响', '亮', '沉', '升', '开', '裂', '涌', '泛', '逼', '动', '颤', '燃',
        '塌', '坠', '滑', '扑', '砸', '撞', '压',
    ];
    chars[index + 1..]
        .iter()
        .any(|ch| predicate_tails.contains(ch))
}

fn cjk_modal_predicate_clause_fragment(core: &str) -> bool {
    let chars = core.chars().collect::<Vec<_>>();
    if chars.len() < 4 || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    let predicate_heads = [
        '吞', '噬', '撕', '裂', '失', '夺', '杀', '死', '伤', '救', '改', '变', '燃', '爆', '坠',
        '落', '响', '亮', '逼', '毁', '断', '塌', '醒',
    ];
    chars.windows(2).any(|window| {
        matches!(
            window[0],
            '会' | '将' | '要' | '能' | '被' | '把' | '让' | '使'
        ) && predicate_heads.contains(&window[1])
    })
}

fn title_looks_like_body_nominal_fragment(title: &str) -> bool {
    let core = chapter_title_core(title);
    let chars = core.chars().collect::<Vec<_>>();
    if !(3..=8).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    let action_chars = [
        '破', '夺', '斩', '救', '逃', '战', '契', '启', '封', '裂', '护',
    ];
    if chars.iter().any(|ch| action_chars.contains(ch)) {
        return false;
    }
    for (index, ch) in chars.iter().enumerate() {
        if !matches!(ch, '人' | '者' | '客') || index == 0 || index + 1 >= chars.len() {
            continue;
        }
        let tail = chars[index + 1..].iter().collect::<String>();
        if cjk_title_object_tail(&tail) {
            return true;
        }
    }
    false
}

fn title_looks_like_body_adverbial_predicate_fragment(title: &str) -> bool {
    let core = chapter_title_core(title);
    let chars = core.chars().collect::<Vec<_>>();
    if !(3..=6).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    let compact = chars.iter().collect::<String>();
    let predicate_tails = [
        "而精准",
        "而缓慢",
        "而从容",
        "而坚定",
        "而沉默",
        "而清晰",
        "而冰冷",
        "却坚定",
        "却沉默",
        "却清晰",
    ];
    predicate_tails
        .iter()
        .any(|tail| compact.ends_with(tail) && compact.chars().count() <= tail.chars().count() + 2)
}

fn title_looks_like_place_fragment(title: &str) -> bool {
    let core = chapter_title_core(title);
    let generic_location_tails = [
        "入口", "边缘", "深处", "角落", "尽头", "附近", "面前", "背后",
    ];
    if generic_location_tails
        .iter()
        .any(|tail| core.ends_with(tail) && core.chars().count() <= tail.chars().count() + 4)
    {
        return true;
    }
    let chars = core.chars().collect::<Vec<_>>();
    if !(3..=6).contains(&chars.len()) || !chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return false;
    }
    if chars.iter().any(|ch| {
        matches!(
            ch,
            '破' | '夺'
                | '斩'
                | '救'
                | '逃'
                | '战'
                | '契'
                | '启'
                | '封'
                | '裂'
                | '赌'
                | '换'
                | '炼'
                | '证'
                | '拜'
                | '入'
                | '出'
                | '归'
                | '守'
        )
    }) {
        return false;
    }
    let setting_tails = [
        '地', '区', '处', '边', '口', '里', '中', '下', '前', '后', '间',
    ];
    let Some(last) = chars.last().copied() else {
        return false;
    };
    if !setting_tails.contains(&last) {
        return false;
    }
    let place_markers = [
        '巷', '城', '镇', '村', '坊', '阁', '楼', '院', '宗', '门', '山', '谷', '峰', '街', '桥',
        '井', '港', '站', '厂', '矿', '市',
    ];
    chars[..chars.len() - 1]
        .iter()
        .any(|ch| place_markers.contains(ch))
}

fn cjk_title_object_tail(value: &str) -> bool {
    matches!(
        value,
        "剑" | "刀"
            | "枪"
            | "令"
            | "符"
            | "阵"
            | "印"
            | "书"
            | "卷"
            | "牌"
            | "门"
            | "影"
            | "局"
            | "网"
            | "账"
    )
}

fn title_has_story_action_or_clue_surface(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch,
            '破' | '夺'
                | '斩'
                | '救'
                | '逃'
                | '战'
                | '契'
                | '启'
                | '封'
                | '裂'
                | '赌'
                | '换'
                | '炼'
                | '证'
                | '查'
                | '揭'
                | '守'
                | '案'
                | '债'
        )
    })
}

fn candidate_is_embedded_in_longer_cjk_run(candidate: &str, evidence: &str) -> bool {
    let core = chapter_title_core(candidate);
    if core.chars().count() < 3 || !core.chars().all(is_cjk_unified) || evidence.is_empty() {
        return false;
    }
    let chars = evidence.chars().collect::<Vec<_>>();
    let needle = core.chars().collect::<Vec<_>>();
    chars.windows(needle.len() + 2).any(|window| {
        is_cjk_unified(window[0])
            && window[1..window.len() - 1] == needle
            && is_cjk_unified(window[window.len() - 1])
    })
}

fn candidate_is_prefix_of_longer_cjk_compound(candidate: &str, evidence: &str) -> bool {
    let core = chapter_title_core(candidate);
    if core.chars().count() < 3 || !core.chars().all(is_cjk_unified) || evidence.is_empty() {
        return false;
    }
    let chars = evidence.chars().collect::<Vec<_>>();
    let needle = core.chars().collect::<Vec<_>>();
    chars.windows(needle.len() + 1).any(|window| {
        window[..needle.len()] == needle
            && (window[needle.len()] == '的'
                || cjk_predicate_prefix_continues(&core, window[needle.len()]))
    })
}

fn cjk_predicate_prefix_continues(core: &str, next: char) -> bool {
    let Some(last) = core.chars().last() else {
        return false;
    };
    matches!(
        (last, next),
        ('发', '出')
            | ('发', '声')
            | ('发', '光')
            | ('化', '作')
            | ('变', '成')
            | ('亮', '起')
            | ('响', '起')
            | ('浮', '现')
            | ('浮', '出')
            | ('涌', '出')
            | ('泛', '起')
            | ('落', '下')
            | ('滑', '落')
            | ('裂', '开')
            | ('逼', '近')
            | ('燃', '起')
            | ('显', '现')
            | ('出', '现')
    )
}

fn candidate_has_detached_numeric_classifier_prefix(candidate: &str, evidence: &str) -> bool {
    let core = chapter_title_core(candidate);
    if !core.starts_with('号') || evidence.is_empty() {
        return false;
    }
    let chars = evidence.chars().collect::<Vec<_>>();
    chars
        .windows(2)
        .any(|window| window[0].is_ascii_digit() && window[1] == '号')
}

fn internal_process_title_label(title: &str) -> bool {
    matches!(
        normalized_title_key(title).as_str(),
        "核心体现"
            | "核心目标"
            | "核心变化"
            | "阶段目标"
            | "阶段成果"
            | "主要矛盾"
            | "关键变化"
            | "关键转折"
            | "符合预期"
            | "符合要求"
            | "通过审查"
            | "审稿通过"
    )
}

fn incomplete_character_clause(context: &ChapterTitleContext, title: &str) -> bool {
    let key = normalized_title_key(title);
    context.character_names.iter().any(|name| {
        let name = normalized_title_key(name);
        let Some(suffix) = key.strip_prefix(&name) else {
            return false;
        };
        let suffix = suffix.trim();
        if suffix.is_empty() {
            return false;
        }
        if suffix.chars().count() == 1 {
            return true;
        }
        [
            "没有", "并未", "尚未", "未曾", "不会", "不能", "通过", "获得", "完成", "发现", "确认",
            "决定", "选择", "成为", "开始", "继续", "抵达", "进入", "走进", "回到",
        ]
        .iter()
        .any(|predicate| {
            suffix.starts_with(predicate) && suffix.chars().count() <= predicate.chars().count() + 2
        })
    })
}

pub(crate) fn title_matches_project_or_volume(context: &ChapterTitleContext, title: &str) -> bool {
    let key = normalized_title_key(title);
    if key.is_empty() {
        return false;
    }
    key == normalized_title_key(&context.project_title)
        || context
            .volume_titles
            .iter()
            .any(|volume_title| key == normalized_title_key(volume_title))
}

fn title_matches_other_chapter(context: &ChapterTitleContext, number: usize, title: &str) -> bool {
    let key = normalized_title_key(title);
    !key.is_empty()
        && context
            .other_chapter_titles
            .iter()
            .filter(|(chapter_number, _)| *chapter_number != number)
            .any(|(_, chapter_title)| key == normalized_title_key(chapter_title))
}

fn title_is_too_similar_to_other_chapter(
    context: &ChapterTitleContext,
    number: usize,
    title: &str,
) -> bool {
    let current = normalize_title_lookup_key(title);
    if current.is_empty() || !title_has_enough_signal(title) {
        return false;
    }
    context
        .other_chapter_titles
        .iter()
        .filter(|(chapter_number, _)| *chapter_number != number)
        .any(|(_, chapter_title)| {
            let other = normalize_title_lookup_key(chapter_title);
            if other.is_empty() {
                return false;
            }
            title_similarity(&current, &other) >= 0.86
                || short_titles_share_long_core_fragment(&current, &other)
        })
}

pub(crate) fn title_is_default_chapter_heading(title: &str, number: usize, language: &str) -> bool {
    let key = normalized_title_key(title);
    let default = if is_chinese_language(language) {
        format!("第{number}章")
    } else {
        format!("Chapter {number}")
    };
    key == normalized_title_key(&default)
        || key == normalized_title_key(&format!("第{number}章"))
        || key == normalized_title_key(&format!("Chapter {number}"))
}

pub(crate) fn title_has_enough_signal(value: &str) -> bool {
    let key = normalized_title_key(value);
    if key.chars().count() < 2 {
        return false;
    }
    let lowered = value.trim().to_ascii_lowercase();
    let generic = [
        "untitled", "chapter", "正文", "章节", "本章", "故事", "小说",
    ];
    !generic
        .iter()
        .any(|term| key == normalized_title_key(term) || lowered == *term)
}

pub(crate) fn normalized_title_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            !matches!(
                ch,
                ' ' | '\t'
                    | '\n'
                    | '\r'
                    | '#'
                    | '*'
                    | '`'
                    | '"'
                    | '\''
                    | '“'
                    | '”'
                    | '《'
                    | '》'
                    | ':'
                    | '：'
                    | '-'
                    | '—'
                    | '_'
                    | '.'
                    | '。'
                    | ','
                    | '，'
            )
        })
        .collect::<String>()
        .to_ascii_lowercase()
}

fn story_tokens(core: &str) -> Vec<String> {
    let chars = core.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    for len in [4usize, 3, 2] {
        if chars.len() < len {
            continue;
        }
        for window in chars.windows(len) {
            let token = window.iter().collect::<String>();
            if !story_token_is_useful(&token) {
                continue;
            }
            if !tokens.iter().any(|known| known == &token) {
                tokens.push(token);
            }
        }
    }
    tokens
}

fn story_token_is_useful(token: &str) -> bool {
    if token.chars().count() < 2 {
        return false;
    }
    if !token.chars().all(is_cjk_unified) {
        return false;
    }
    if token.chars().any(title_template_connector) {
        return false;
    }
    if sentence_fragment_edge(token) || prose_grammar_fragment(token) {
        return false;
    }
    if title_lexicon::generic_fiction_chapter_title_terms()
        .iter()
        .any(|generic| normalized_title_key(generic) == normalized_title_key(token))
    {
        return false;
    }
    let generic = [
        "本章", "章节", "故事", "主角", "角色", "人物", "世界", "异界", "玄幻", "都市", "科幻",
        "言情", "重生", "逆袭", "开始", "最终", "过程", "剧情", "情节",
    ];
    !generic.iter().any(|item| token.contains(item))
}

fn normalize_title_lookup_key(value: &str) -> String {
    normalized_title_key(value)
}

fn title_similarity(left: &str, right: &str) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left == right {
        return 1.0;
    }
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    let common = left_chars
        .iter()
        .filter(|ch| right_chars.contains(ch))
        .count();
    let denom = left_chars.len().max(right_chars.len()).max(1);
    common as f64 / denom as f64
}

fn short_titles_share_long_core_fragment(left: &str, right: &str) -> bool {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    if left_chars.len() > 8 || right_chars.len() > 8 {
        return false;
    }
    for len in (3usize..=left_chars.len().min(right_chars.len())).rev() {
        for window in left_chars.windows(len) {
            if window.iter().any(|ch| !is_cjk_unified(*ch)) {
                continue;
            }
            if right_chars.windows(len).any(|other| other == window) {
                return true;
            }
        }
    }
    false
}

fn preview_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn is_chinese_language(language: &str) -> bool {
    let lowered = language.trim().to_ascii_lowercase();
    lowered.starts_with("zh") || lowered.contains("chinese") || lowered.contains("中文")
}

fn bare_body_action_phrase(chars: &[char]) -> bool {
    if !(3..=4).contains(&chars.len()) {
        return false;
    }
    let starts_with_plain_action = matches!(
        chars[0],
        '站' | '坐'
            | '转'
            | '抬'
            | '低'
            | '伸'
            | '缩'
            | '退'
            | '走'
            | '跑'
            | '跪'
            | '倒'
            | '醒'
            | '看'
            | '望'
            | '听'
            | '喊'
            | '问'
            | '答'
            | '笑'
            | '哭'
    );
    let has_motion_complement = chars[1..chars.len() - 1]
        .iter()
        .any(|ch| matches!(ch, '起' | '下' | '过' | '回' | '转' | '开' | '住'));
    let ends_with_body_or_plain_action_object = chars
        .last()
        .is_some_and(|ch| matches!(ch, '身' | '头' | '手' | '脚' | '眼' | '口' | '声'));
    starts_with_plain_action && has_motion_complement && ends_with_body_or_plain_action_object
}

fn pronoun_body_action_phrase(chars: &[char]) -> bool {
    if !(4..=5).contains(&chars.len()) || !matches!(chars[0], '他' | '她' | '它' | '牠') {
        return false;
    }
    bare_body_action_phrase(&chars[1..])
}

fn is_cjk_unified(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        || ('\u{20000}'..='\u{2a6df}').contains(&ch)
        || ('\u{2a700}'..='\u{2b73f}').contains(&ch)
        || ('\u{2b740}'..='\u{2b81f}').contains(&ch)
        || ('\u{2b820}'..='\u{2ceaf}').contains(&ch)
        || ('\u{f900}'..='\u{faff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_flags_short_titles_sharing_core_fragment() {
        let issues = registry_issues(3, "璃幕墙前", [(2, "玻璃幕墙".to_string())]);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("too similar to chapter 2")),
            "short chapter titles sharing a long core fragment should be rejected: {issues:?}"
        );
    }

    #[test]
    fn chapter_title_core_strips_volume_prefix() {
        assert_eq!(chapter_title_core("第二卷：观星台的代价"), "观星台的代价");
        assert_eq!(
            chapter_title_core("Volume 2: The Observatory Price"),
            "The Observatory Price"
        );
    }

    #[test]
    fn title_body_fragment_rejects_cjk_predicate_prefix() {
        let content = "梁澈川身形暴起，手中长剑化作一道流光，直劈晏照珩头顶。";

        let issue = title_body_fragment_issue("zh-CN", "手中长剑化", content);

        assert!(
            issue.is_some(),
            "a prefix sliced out of a longer prose predicate should be repaired as metadata"
        );
    }

    #[test]
    fn final_title_selection_rejects_adjectival_prefix_of_body_phrase() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "灵根掠夺者".to_string(),
            volume_titles: vec!["灰土求生".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["姜闻棠".to_string(), "赵无涯".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "一阵轻微",
            "姜闻棠在黑市用残根换取灵液，并被赵无涯发现吞噬灵液的异常。",
            "就在他转身准备离开时，一阵轻微的脚步声从巷口传来。赵无涯发现姜闻棠能吞噬灵液，提出让他做试药人。姜闻棠没有立刻屈服，而是借此获得继续活下去的机会。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("一阵轻微")
        );
    }

    #[test]
    fn final_title_selection_preserves_model_title_after_structural_prefix_normalization() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "虹桥暗翼".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["白望遥".to_string(), "段澈舟".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "第一章 残骸与静默",
            "白望遥发现失踪侦察机残骸，并在无线电静默窗口确认异常信标。",
            "白望遥飞越荒野时发现德制侦察机残骸。段澈舟告诉他，当晚正处于无线电静默窗口期。",
        );

        assert_eq!(chapter_title_surface_issue(&context, 1, "残骸与静默"), None);
        assert!(has_story_evidence(
            "zh-CN",
            "残骸与静默",
            "白望遥发现失踪侦察机残骸，并在无线电静默窗口确认异常信标。",
            &[],
            &[],
            "白望遥飞越荒野时发现德制侦察机残骸。段澈舟告诉他，当晚正处于无线电静默窗口期。"
        ));
        assert!(decision.accepted, "{decision:?}");
        assert_eq!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("残骸与静默")
        );
    }

    #[test]
    fn compound_title_is_not_grounded_when_its_future_action_has_not_happened() {
        let summary = "陆泊宁在校验室发现第7区记忆批次异常，并保留原始记忆碎片。";
        let body = "陆泊宁在校验室导出记忆碎片。她决定下班后前往地下酒吧，但尚未与任何人交涉。";

        assert!(!has_story_evidence(
            "zh-CN",
            "地下酒吧的拦截",
            summary,
            &[],
            &[],
            body
        ));
        assert!(has_story_evidence(
            "zh-CN",
            "校验室的碎片",
            summary,
            &[],
            &[],
            body
        ));
    }

    #[test]
    fn final_title_selection_accepts_grounded_event_metaphor() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "逆风开球".to_string(),
            volume_titles: vec!["入局见证".to_string()],
            other_chapter_titles: vec![
                (1, "铁网后的第一脚".to_string()),
                (2, "泥地上的阵型".to_string()),
            ],
            character_names: vec!["段知遥".to_string(), "韩澈川".to_string()],
        };
        let summary = "韩澈川认可段知遥呐喊中难以被数据模拟的野性，段知遥随后接受队友的烧烤邀请。";
        let body = "段知遥对着空旷球场无声呐喊。韩澈川说，这种野性是数据模型模拟不出的回响。队友随后邀请段知遥去吃烧烤，她第一次放松下来。";

        assert_eq!(chapter_title_surface_issue(&context, 9, "野性回响"), None);
        let decision = select_final_chapter_title_from_body(&context, 9, "野性回响", summary, body);
        assert_eq!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("野性回响"),
            "{decision:?}"
        );
    }

    #[test]
    fn final_title_selection_keeps_story_title_with_embedded_ordinal_object() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "青岩回响".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["许望棠".to_string(), "宁澈川".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "晨雾与第一张档案",
            "许望棠在晨雾中的教室翻开第一张学生档案，并确认乡村教育规则的现实代价。",
            "青岩村晨雾未散，许望棠翻开学生档案，第一次看见点名册背后的辍学危机。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_eq!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("晨雾与第一张档案")
        );
    }

    #[test]
    fn final_title_selection_repairs_short_prose_clause_fragments() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "潮汐契约".to_string(),
            volume_titles: vec!["黑礁来信".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["梁庭安".to_string(), "程庭禾".to_string()],
        };
        let body = "梁庭安在旧木屋里发现暗红色契约书。书页的符文随着潮声微微流动，像有一条海沟藏在纸背后。程庭禾带他听见灯塔底部的回声，守岛人的旧债第一次露出形状。";

        for bad_title in ["微微流动", "那是沉星", "底层以来", "影兽发出"] {
            let decision = select_final_chapter_title_from_body(
                &context,
                1,
                bad_title,
                "梁庭安发现契约书，程庭禾确认灯塔底部藏着守岛旧债。",
                body,
            );

            assert!(decision.accepted, "{bad_title}: {decision:?}");
            assert_ne!(
                decision
                    .selected
                    .as_ref()
                    .map(|candidate| candidate.title.as_str()),
                Some(bad_title),
                "{bad_title} should be treated as prose-clause metadata, not final title"
            );
        }
    }

    #[test]
    fn final_title_selection_repairs_real_world_short_body_fragments() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "灯塔破局".to_string(),
            volume_titles: vec!["潮汐账本".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["季予棠".to_string(), "陈伯".to_string()],
        };
        let summary =
            "季予棠在潮眼裂缝旁找到旧账本，确认灯塔债务并非传闻，陈伯要求她做出守岛选择。";
        let body = "海风比昨天更冷，季予棠站在渡口边，看见潮眼裂缝在礁石下泛出蓝光。陈伯离开后，旧账本仍压在灯塔桌面。那面镜子像是某种审判，屋里是核心账页，空气稠而沉重。她最终翻到账本背面的血色签名，确认赵海生当年隐瞒了第一次沉船的真相。";

        for bad_title in ["海风比昨", "生离开后", "像是某种", "里是核心", "稠而沉重"]
        {
            let decision =
                select_final_chapter_title_from_body(&context, 4, bad_title, summary, body);

            assert!(decision.accepted, "{bad_title}: {decision:?}");
            assert_ne!(
                decision
                    .selected
                    .as_ref()
                    .map(|candidate| candidate.title.as_str()),
                Some(bad_title),
                "{bad_title} should be treated as a clipped prose fragment, not final title"
            );
        }
    }

    #[test]
    fn final_title_selection_repairs_copula_degree_prefix_from_body() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "神经探针下的旧城".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["阮栖棠".to_string(), "赵无极".to_string()],
        };
        let summary = "阮栖棠在赵无极手术中发现童年记忆吻合，确认植入物来源指向旧城区。";
        let body = "阮栖棠是江城最年轻的神经外科主任医师，以绝对理性著称。手术中，神经探针从赵无极脑内植入物里提取出红色蝴蝶伞记忆。那段记忆与阮栖棠七岁前的空白童年吻合，他决定追查旧城区的记忆来源。";

        let decision = select_final_chapter_title_from_body(&context, 1, "是江城最", summary, body);

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("是江城最"),
            "copula/degree prefixes cut from prose must not become final chapter titles"
        );
    }

    #[test]
    fn final_title_selection_repairs_copula_clause_prefix_from_body() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "残卷诡局".to_string(),
            volume_titles: vec!["旧案回潮".to_string()],
            other_chapter_titles: vec![(2, "单证定罪".to_string())],
            character_names: vec!["南知声".to_string(), "段照安".to_string()],
        };
        let summary = "残卷记载了镇北侯当年的行军路线，是证明镇北侯清白的关键证据。";
        let body = "段照安展开残卷，指出其中的行军路线与旧案证词相悖。南知声确认这份路线图是证明镇北侯清白的关键证据，并决定追查被篡改的坐标。";

        let decision = select_final_chapter_title_from_body(&context, 3, "是证明镇", summary, body);

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("是证明镇"),
            "copula clause fragments cut from prose must not become final chapter titles"
        );
    }

    #[test]
    fn final_title_selection_repairs_classifier_fragment_detached_from_number() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "灰度晋升".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: Vec::new(),
            character_names: vec!["温知遥".to_string(), "阮望川".to_string()],
        };
        let summary = "温知遥确认第74号异常数据并非随机噪声，决定追查黑箱日志。";
        let body = "温知遥重新核验第74号异常数据，发现周期峰值与核心产品的活跃度同步。阮望川交给她一枚旧芯片，两人决定沿着异常数据追查被隐藏的利益链。";

        let decision = select_final_chapter_title_from_body(&context, 1, "号异常数", summary, body);

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("号异常数"),
            "a classifier fragment detached from its number must not become a chapter title"
        );
    }

    #[test]
    fn final_title_selection_repairs_character_action_truncated_object_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "神经接口下的记忆交易".to_string(),
            volume_titles: vec!["旧日当铺".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["温曜遥".to_string(), "段烁川".to_string()],
        };
        let body = "温曜遥抵达星穹学院时，操场上空的神经接口钟已经开始倒计时。段烁川带他穿过旧日当铺的侧门，解释记忆水晶的价格会随考试排名浮动。温曜遥没有立刻卖掉母亲的临终记忆，而是用初级微积分记忆换来第一张入场券。";
        let decision = select_final_chapter_title_from_body(
            &context,
            2,
            "温曜遥抵达星",
            "温曜遥抵达星穹学院，第一次理解旧日当铺的记忆交易规则，并保住母亲的临终记忆。",
            body,
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("温曜遥抵达星"),
            "character action fragments with an unresolved object tail should be repaired"
        );
    }

    #[test]
    fn final_title_selection_repairs_ambient_setting_only_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "灵枢通胀时代的底层猎杀".to_string(),
            volume_titles: vec!["废材堆里的黄金".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["白栖舟".to_string(), "韩知禾".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "霓虹灯",
            "白栖舟在垃圾堆中发现疑似灵枢原核的黑石头，经古法温养后确认为高纯度灵材。",
            "第七区贫民窟的霓虹灯管滋滋作响。白栖舟在废料传送带边缘夹起一块黑色石头，灵枢图谱显示里面藏着完整的灵枢原核。他用引灵草灰温养黑石，黑壳裂开后露出纯净灵光。他在记账本上写下第一桶金的估值，决定赌上全部身家收购浊灵石渣。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("霓虹灯")
        );
    }

    #[test]
    fn final_title_selection_repairs_causal_clause_fragment_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "资本教父：从捡漏开始".to_string(),
            volume_titles: vec!["古玩街初显锋芒".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["裴晴川".to_string(), "陶闻序".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            2,
            "赵德柱因资金",
            "裴晴川在慧眼预知失效前把样钱变现，又利用赵德柱的贪婪完成十万元回流。",
            "赵德柱因资金链断裂急着找货。裴晴川把光绪通宝样钱先以两千元卖给他，三天后又看准赵德柱想用镇店之宝撑场面的心理，把回购价抬到十万元。裴晴川确认落袋为安比等待估值更可靠。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("赵德柱因资金")
        );
    }

    #[test]
    fn final_title_selection_rejects_summary_echo_of_embedded_place_fragment() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "残方证道：师门逆袭录".to_string(),
            volume_titles: vec!["入局见证".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["梁阙舟".to_string(), "洛衡息".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "门杂役区",
            "门杂役区 青岚宗的外门杂役区里，梁阙舟在弃物阁发现铜钱里的残方。",
            "青岚宗的外门杂役区，终年笼罩在一层洗不净的灰雾里。梁阙舟跪在弃物阁冰冷的石板上，从废弃杂物里发现一枚藏着敛息残方的铜钱。他把铜钱从赵管事手中保住，确认这不是偶然，而是自己踏入修行缝隙的入口。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("门杂役区")
        );
    }

    #[test]
    fn final_title_selection_repairs_character_name_action_fragment_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卡片来源与城破局".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["岑桥晚".to_string(), "段望白".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "岑桥晚皱",
            "岑桥晚被赶出豪门后继承顶层游戏卡，用卡片指引买入宏远商贸股票并发现有人在压盘。",
            "雨夜里，岑桥晚在出租屋发现父亲遗留的顶层游戏卡。卡片提示他买入宏远商贸股票，他借贷加仓，又收到陌生短信警告有人在等他入局。黑色卡片泛起暗红纹路，云鼎大厦像一只眼睛注视着他。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("岑桥晚皱")
        );
    }

    #[test]
    fn final_title_selection_repairs_character_name_with_unresolved_tail() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "破局者游戏".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(1, "入场券".to_string())],
            character_names: vec![
                "秦桥禾".to_string(),
                "段澈白".to_string(),
                "景栖禾".to_string(),
            ],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            2,
            "秦桥禾深",
            "秦桥禾进入地下档案区，找到初代测试员离职档案，并确认系统任务会扣除视力与判断力。",
            "秦桥禾深吸一口气。100点，这是他目前的全部家当。输了，就是半瞎加大脑宕机；赢了，就能获得更深层的权限。他打开地下二层的档案柜，抽出林默的离职档案，并发现宏达集团早在数年前就把破局者系统嵌入职场考核。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("秦桥禾深")
        );
    }

    #[test]
    fn final_title_selection_repairs_pronoun_body_action_fragment_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "灵核契约".to_string(),
            volume_titles: vec!["拾荒者的灵核".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["孟晴棠".to_string(), "洛望禾".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "她站起身",
            "孟晴棠激活旧基站里的守望者协议，并被运输舰锁定。",
            "孟晴棠把金属盒插入旧基站，蓝光沿着断裂天线冲上云层。守望者协议启动，运输舰的机械爪从雨幕里落下。她站起身，知道自己已经被整座城市看见。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("她站起身")
        );
    }

    #[test]
    fn final_title_selection_repairs_body_prefix_predicate_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(1, "海城青年".to_string())],
            character_names: vec!["顾栖川".to_string(), "程予棠".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            2,
            "夜雨来得",
            "顾栖川击败雷刚及其手下，但古玉开始抽取其精血作为代价。",
            "海城夜雨里，顾栖川在巷口被雷刚拦截。他借古玉力量反击，却发现古玉开始抽取精血。程予棠的黑色轿车停在巷口，提出让他成为自己的影子。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("夜雨来得")
        );
    }

    #[test]
    fn final_title_selection_repairs_predicate_prefix_cut_from_body() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "古老契".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["顾棠安".to_string(), "祝棠晚".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "雨刮器发",
            "顾棠安在雨夜交通冲突中发现折叠券，并第一次用十秒时间差反制赵德发。",
            "雨刮器发出干涩的吱呀声，顾棠安在雨夜堵车时被赵德发逼迫赔钱。手机里的折叠券亮起，他第一次加速十秒，抓住公文包漏洞完成反制，也看见都市资本规则背后的入口。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("雨刮器发")
        );
    }

    #[test]
    fn final_title_selection_repairs_unfinished_negation_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(1, "海城青年".to_string()), (2, "巷口影约".to_string())],
            character_names: vec![
                "顾栖川".to_string(),
                "程予棠".to_string(),
                "洛栖舟".to_string(),
            ],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            3,
            "夜雨并未",
            "顾栖川前往洛家老宅，拿到雷刚交出的监控存储卡，并接受三天后护送玄铁令的委托。",
            "海城的夜雨并未停歇。顾栖川前往洛家老宅，见到洛栖舟和雷刚，拿到云图武馆地下三层的监控存储卡。雷刚承认败北背后牵动洛家规矩，洛栖舟提出三天后护住玄铁令的交易。顾栖川接受影子身份，决定正式卷入拍卖会风暴。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("夜雨并未")
        );
    }

    #[test]
    fn final_title_selection_repairs_unfinished_negative_predicate_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(7, "断水门".to_string())],
            character_names: vec!["顾栖川".to_string(), "苏清婉".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            8,
            "灯光并非",
            "苏清婉揭示断水令共九枚，顾栖川得知自己是承接断水之力的容器候选。",
            "后巷的灯光并非突然熄灭，而是被一道无形的水线生生切断。苏清婉揭示断水令共九枚，第九枚归零用于筛选容器。顾栖川握紧玄铁令，决定登上海城码头的货船，追查程予棠的暗桩。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("灯光并非")
        );
    }

    #[test]
    fn final_title_selection_repairs_unfinished_count_phrase_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(7, "断水门".to_string())],
            character_names: vec!["顾栖川".to_string(), "苏清婉".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            8,
            "断水令共",
            "苏清婉揭示断水令共九枚，顾栖川得知自己是承接断水之力的容器候选。",
            "苏清婉低声道，断水令共九枚，前八枚各有属性，第九枚归零用于筛选容器。顾栖川握紧玄铁令，决定登上海城码头的货船。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("断水令共")
        );
    }

    #[test]
    fn final_title_selection_rejects_embedded_count_phrase_fragment() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(7, "断水门".to_string())],
            character_names: vec!["顾栖川".to_string(), "苏清婉".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            8,
            "水令共九",
            "苏清婉揭示断水令共九枚，顾栖川得知自己是承接断水之力的容器候选。",
            "苏清婉低声道，断水令共九枚，前八枚各有属性，第九枚归零用于筛选容器。顾栖川握紧玄铁令，决定登上海城码头的货船。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("水令共九")
        );
    }

    #[test]
    fn final_title_selection_does_not_let_existing_heading_self_validate() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(7, "断水门".to_string())],
            character_names: vec!["顾栖川".to_string(), "苏清婉".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            8,
            "水令共九",
            "苏清婉揭示断水令共九枚，顾栖川得知自己是承接断水之力的容器候选。",
            "# 水令共九\n\n苏清婉低声道，断水令共九枚，前八枚各有属性，第九枚归零用于筛选容器。顾栖川握紧玄铁令，决定登上海城码头的货船。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("水令共九")
        );
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("令共九枚")
        );
    }

    #[test]
    fn final_title_selection_rejects_embedded_direction_fragment() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(7, "断水门".to_string())],
            character_names: vec!["顾栖川".to_string(), "苏清婉".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            10,
            "断水门面",
            "苏清婉提醒顾栖川在断水门面前平衡可控性，影七协助他布置钟楼考验。",
            "苏清婉指出顾栖川必须在断水门面前表现出可控性。影七接过信号弹，顾栖川前往听雨轩，从老茶师口中明白钟楼考验同时考验武力、权谋和心境。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("断水门面")
        );
    }

    #[test]
    fn final_title_selection_repairs_adverb_predicate_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(1, "海城青年".to_string()), (4, "令牌暗流".to_string())],
            character_names: vec!["顾栖川".to_string(), "程予棠".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            5,
            "雨总是带",
            "顾栖川在博览中心接下玄铁令，神秘人突袭，程予棠出手挡住赵铁柱。",
            "海城的雨总是带着潮湿腥味。顾栖川在博览中心包厢接下玄铁令，神秘人短刃突袭，程予棠以竹杖挡住赵铁柱的拳锋。洛天雄宣布游戏开始，玄铁令争夺正式升级。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("雨总是带")
        );
    }

    #[test]
    fn final_title_selection_repairs_event_predicate_quantity_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "禁灵破界".to_string(),
            volume_titles: vec!["入局见证".to_string()],
            other_chapter_titles: vec![],
            character_names: vec![
                "段照野".to_string(),
                "洛栖遥".to_string(),
                "梁闻野".to_string(),
            ],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "突然传来一阵",
            "段照野在断龙崖发现吞天鼎，第一次触碰凡骨之外的破局力量。",
            "段照野站在断龙崖边缘，灰黄色雾霾压低天色。他在遗迹碎片中找到吞天鼎，体内干涸的气海突然传来一阵剧痛，禁灵结界的秘密也露出第一道裂缝。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("突然传来一阵")
        );
    }

    #[test]
    fn final_title_selection_repairs_nominal_compound_fragment_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(5, "玄铁令".to_string())],
            character_names: vec![
                "顾栖川".to_string(),
                "洛栖舟".to_string(),
                "程予棠".to_string(),
            ],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            6,
            "神秘人剑",
            "洛栖舟指出神秘人剑法疑似北方断水门绝学，顾栖川在楼梯间发现高处指印，确认神秘人暗中监视。",
            "洛栖舟指出神秘人的剑法来自断水门。顾栖川在楼梯间发现水渍和高处指印，意识到神秘人一直潜伏在博览中心，玄铁令的争夺已经转入暗处。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("神秘人剑")
        );
    }

    #[test]
    fn final_title_selection_repairs_body_suffix_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(5, "玄铁令".to_string())],
            character_names: vec!["顾栖川".to_string(), "洛栖舟".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            6,
            "住玄铁令",
            "顾栖川护住玄铁令并发现神秘人的断水门痕迹。",
            "顾栖川在VIP走廊护住玄铁令，随后在楼梯间发现断水门留下的水渍和高处指印。他意识到神秘人已经把拍卖会变成暗处猎场。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("住玄铁令")
        );
    }

    #[test]
    fn final_title_selection_repairs_body_prefix_verb_fragment_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "借卷入豪门".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(5, "玄铁令".to_string())],
            character_names: vec!["顾栖川".to_string(), "洛栖舟".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            6,
            "出玄铁令",
            "洛栖舟要求顾栖川逼神秘人交出玄铁令，顾栖川发现断水门留下的高处指印。",
            "洛栖舟让顾栖川查明神秘人身份，并在拍卖会上逼对方交出玄铁令。顾栖川在楼梯间发现水渍与高处指印，意识到断水门已经把他当成猎物。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("出玄铁令")
        );
    }

    #[test]
    fn final_title_selection_repairs_connective_clause_fragment_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "旧城新主".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["沈砚安".to_string(), "司桥安".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "由于账户",
            "沈砚安收到异常冻结通知，回到旧城档案室查清第一笔责任账。",
            "沈砚安在雨夜收到异常账户冻结通知，被迫回到旧城档案室。他发现那笔被隐藏的责任账连接着旧城改造和司桥安的失踪线索，于是决定先追查地下二层的账册。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("由于账户")
        );
    }

    #[test]
    fn final_title_selection_repairs_bare_measure_noun_fragment_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "晚掌控城".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["景砚澜".to_string(), "孟砚安".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "座城市",
            "景砚澜确认老公寓存在空间折叠，并接到孟砚安相关的警告。",
            "景砚澜带客户看老公寓时觉醒折叠空间的能力，促成交易后收到陌生语音提醒他小心孟砚安。他意识到这座城市的空间秘密正向自己打开。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("座城市")
        );
    }

    #[test]
    fn final_title_selection_repairs_window_cut_measure_fragment_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "逆袭从送外卖开始".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["宋晴安".to_string(), "祝望澜".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "块黑石头",
            "宋晴安在旧书摊发现高价值黑石，获得进入财富世界的第一张证明。",
            "宋晴安在旧书摊角落发现一块不起眼的黑色石头。它被破字典压住，却在价值视野里泛出刺眼金光。鉴定室里，专家确认这块石头可能切出羊脂白玉，宋晴安因此拿到第一张财富证明。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("块黑石头")
        );
    }

    #[test]
    fn final_title_selection_repairs_review_verdict_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "断剑吞灵".to_string(),
            volume_titles: vec!["入局见证".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["祝朔澜".to_string(), "谢澈川".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "符合预期",
            "祝朔澜在断龙崖截获天枢星阵溢出的废灵流，断剑首次吞噬本源金芒。",
            "祝朔澜在断龙崖握住锈迹斑斑的断剑，让裂口吞下天枢星阵溢出的灰蓝废灵流。谢澈川现身夺剑，风刃被断剑反吸，祝朔澜意识到这柄断剑正是自己踏入修仙秩序的入口。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("符合预期")
        );
    }

    #[test]
    fn final_title_selection_repairs_internal_stage_label_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "天穹暗战：识人破局".to_string(),
            volume_titles: vec!["潜龙在渊".to_string()],
            other_chapter_titles: vec![],
            character_names: vec!["唐庭澜".to_string(), "姜棠宁".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "核心体现",
            "唐庭澜入职天穹集团销售部，凭借观察客户微表情签下第一份合约。",
            "唐庭澜在天穹集团销售部面对刁钻的陈总，没有照本宣科介绍系统，而是看出物流调度痛点，拿出成本分析表和 VIP 通道承诺，签下两年合约，也第一次引起主管李曼的注意。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("核心体现")
        );
    }

    #[test]
    fn final_title_selection_avoids_recent_near_duplicate_object_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "无灵根：以凡骨证道".to_string(),
            volume_titles: vec!["入局见证".to_string()],
            other_chapter_titles: vec![(1, "黑色石头".to_string())],
            character_names: vec!["梁闻珩".to_string(), "许朔野".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            2,
            "那块黑色石",
            "梁闻珩接下灰雾秘境试炼，用凡骨代价换取清心丹机会。",
            "梁闻珩在坊市被梁闻澜逼迫接下灰雾秘境试炼。他买下止血散和解毒丹，明白那块黑色石已经把自己推入宗门倾轧。凡骨纹路在手臂上加深，清心丹成了他必须夺回的第一份生路。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("那块黑色石")
        );
    }

    #[test]
    fn final_title_selection_repairs_place_tail_fragment_heading() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "无灵根：以凡骨证道".to_string(),
            volume_titles: vec!["入局见证".to_string()],
            other_chapter_titles: vec![(1, "黑色石头".to_string()), (2, "秘境试炼".to_string())],
            character_names: vec!["梁闻珩".to_string(), "许朔野".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            3,
            "枯骨巷地",
            "梁闻珩与许朔野发现枯骨巷暗脉，确认梁家封脉镇天梯的旧秘密。",
            "梁闻珩与许朔野进入枯骨巷旧矿坑，移开刻有古老符文的封石，发现暗脉灵泉和封脉石碑。碑文揭开梁氏千年前封此脉以镇天梯的秘密，梁闻珩意识到自己的凡骨传承与苍云界底层生路相连。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("枯骨巷地")
        );
    }

    #[test]
    fn final_title_selection_repairs_character_negative_predicate_fragment() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "盲眼灯塔".to_string(),
            volume_titles: vec!["开局卷".to_string()],
            other_chapter_titles: vec![(1, "交出锁链".to_string())],
            character_names: vec!["钟澈白".to_string(), "司岚阙".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            2,
            "钟澈白没有回",
            "钟澈白在第二层回音室击碎月光珍珠，支付一年的听觉代价并进入静默回廊。",
            "钟澈白没有回头，他把铜管贴近地面，用共振震散听风者。月光珍珠在贝壳石台上裂开，第二节点被激活，他支付一年的听觉代价。司岚阙带他走向静默回廊，提醒第三层真正考验的是分辨唯一不和谐的声音。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("钟澈白没有回"),
            "character-name negative predicate fragments must not become final chapter titles"
        );
    }

    #[test]
    fn final_title_selection_repairs_character_incomplete_transitive_predicate() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "修补旧日".to_string(),
            volume_titles: vec!["旧店重开".to_string()],
            other_chapter_titles: vec![(1, "不收现金".to_string())],
            character_names: vec!["陶砚川".to_string(), "程予澜".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            2,
            "陶砚川通过",
            "陶砚川修复旧怀表，确认店铺会让承载承诺的旧物显现代价。",
            "陶砚川拆开旧怀表，固定松动表冠并复位游丝。怀表恢复走时后，表主留下了关于承诺的故事，店里的旧钟随之重新响起。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("陶砚川通过"),
            "a character name followed by a transitive predicate without its object is prose, not a title"
        );
    }

    #[test]
    fn final_title_selection_repairs_adverbial_predicate_body_fragment() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "深夜味蕾审判".to_string(),
            volume_titles: vec!["开局卷".to_string()],
            other_chapter_titles: Vec::new(),
            character_names: vec!["温庭宁".to_string(), "南晴遥".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "缓而精准",
            "温庭宁接手归味食堂，发现客人能用味觉记忆碎片支付餐费，并从第一枚碎片里看到妹妹失踪旧案的线索。",
            "温庭宁接手归味食堂，在午夜为第一位食客煮出忘忧菌汤。食客以一枚悲伤味觉记忆碎片支付餐费，碎片中浮现出妹妹失踪旧案相关的雨夜影像。温庭宁意识到食堂不是普通餐馆，而是通往真相的入口。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("缓而精准"),
            "adverbial predicate prose fragments must not become final chapter titles"
        );
    }

    #[test]
    fn final_title_selection_repairs_reduplicated_adverbial_body_fragment() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "铜雨账本".to_string(),
            volume_titles: vec!["旧市开局".to_string()],
            other_chapter_titles: Vec::new(),
            character_names: vec!["沈知衡".to_string(), "程泊澜".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            1,
            "紧紧地锁",
            "沈知衡发现盐引账册被程家封存在旧仓，决定借交割日查验暗账。",
            "程家把旧仓门紧紧地锁住。沈知衡趁交割日核对盐引，在破损账册里找到重复火耗记录，并决定从运货脚夫追查暗账来源。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("紧紧地锁"),
            "reduplicated adverbial prose fragments must not become final chapter titles"
        );
    }

    #[test]
    fn final_title_selection_repairs_incomplete_aspect_clause() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "村口那个金算盘".to_string(),
            volume_titles: vec!["入局见证".to_string()],
            other_chapter_titles: vec![(1, "槐树下的金算盘".to_string())],
            character_names: vec!["孟衡棠".to_string(), "梁闻安".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            2,
            "还没散尽",
            "孟衡棠建立苹果分级制度，并用风味果扭转村民对次果的看法。",
            "梧桐村的清晨，雾气还没散尽。孟衡棠建立苹果分级制度，用风味果扭转村民对次果的看法，并迎来景望声的新一轮压价。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("还没散尽"),
            "an incomplete aspect clause from prose must not become a chapter title"
        );
    }

    #[test]
    fn final_title_selection_repairs_linking_predicate_clause() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "绿噬最后的种子".to_string(),
            volume_titles: vec!["代价追索".to_string()],
            other_chapter_titles: vec![(12, "倒计时".to_string())],
            character_names: vec!["裴知声".to_string(), "陶砚禾".to_string()],
        };
        let decision = select_final_chapter_title_from_body(
            &context,
            13,
            "变得密集",
            "核心室外的变异生物包围逐渐收紧，裴知声守住幼苗。",
            "核心室外的摩擦声变得密集。裴知声借白藤屏障守住幼苗，并确认包围圈正在收紧。",
        );

        assert!(decision.accepted, "{decision:?}");
        assert_ne!(
            decision
                .selected
                .as_ref()
                .map(|candidate| candidate.title.as_str()),
            Some("变得密集"),
            "a linking-predicate prose clause must not become a chapter title"
        );
    }

    #[test]
    fn chapter_title_surface_gate_routes_invalid_titles_to_metadata_repair() {
        let context = ChapterTitleContext {
            language: "zh-CN".to_string(),
            project_title: "铜雨账本".to_string(),
            volume_titles: vec!["旧城卷".to_string()],
            other_chapter_titles: vec![(1, "雨夜开账".to_string())],
            character_names: vec!["沈知衡".to_string()],
        };

        for title in ["雨夜开账", "缓而精准"] {
            assert!(
                chapter_title_needs_post_body_repair(&context, 2, title),
                "{title} should require metadata repair"
            );
        }
    }
}
