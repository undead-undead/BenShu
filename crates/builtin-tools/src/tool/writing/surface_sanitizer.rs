//! Text-surface cleanup shared by writing contract and artifact gates.
//!
//! These helpers remove UI/prompt residue from tool-facing contracts without
//! deciding story content.

pub(crate) fn strip_generation_markup_noise(text: &str) -> String {
    text.replace("\\rightarrow", "")
        .replace("\\leftarrow", "")
        .replace("rightarrow", "")
        .replace("leftarrow", "")
        .replace("**", "")
}

pub(crate) fn contains_legal_contract_residue(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let strong_markers = [
        "本合同",
        "合同签订",
        "创作周期",
        "交付方式",
        "交付时间",
        "版权与署名",
        "著作权归",
        "按照甲方",
        "甲方审阅",
        "修改意见",
        "修改次数",
        "交付日期",
        "违约责任",
        "签字/盖章",
        "具体天数",
        "具体次数",
        "最终稿确认",
        "平台或渠道发布",
        "将本作品",
        "copyright ownership",
        "contract period",
        "breach of contract",
    ];
    strong_markers
        .iter()
        .any(|marker| text.contains(marker) || lowered.contains(&marker.to_ascii_lowercase()))
        || (text.contains("甲方") && text.contains("乙方"))
        || (lowered.contains("party a") && lowered.contains("party b"))
}

pub(crate) fn cjk_action_object_part_boundary_fragments(content: &str) -> Vec<String> {
    let mut fragments = Vec::new();
    for line in content.lines() {
        let chars = line.chars().collect::<Vec<_>>();
        for index in 0..chars.len() {
            if cjk_object_part_boundary_should_insert(&chars, index) {
                let start = index.saturating_sub(8);
                let end = (index + 8).min(chars.len());
                fragments.push(chars[start..end].iter().collect::<String>());
            }
        }
    }
    fragments.sort();
    fragments.dedup();
    fragments
}

pub(crate) fn repair_cjk_action_object_part_boundaries(content: &str) -> String {
    content
        .lines()
        .map(repair_cjk_action_object_part_boundary_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn repair_cjk_action_object_part_boundary_line(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    if chars.len() < 4 {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut changed = false;
    for index in 0..chars.len() {
        out.push(chars[index]);
        if cjk_object_part_boundary_should_insert(&chars, index) {
            out.push('，');
            out.push(chars[index]);
            changed = true;
        }
    }
    if changed {
        out
    } else {
        line.to_string()
    }
}

/// Detects author-facing chapter planning commentary accidentally appended to
/// finished prose. Requiring current/future chapter references plus an
/// authorial planning signal keeps ordinary narrative sentences intact.
pub(crate) fn line_looks_like_story_planning_meta(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 220 {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    let current_chapter = ["本章", "这一章", "本节", "this chapter", "this section"]
        .iter()
        .any(|marker| trimmed.contains(marker) || lowered.contains(marker));
    let future_chapter = [
        "下一章",
        "下章",
        "后续章节",
        "后文",
        "next chapter",
        "following chapter",
        "later chapter",
    ]
    .iter()
    .any(|marker| trimmed.contains(marker) || lowered.contains(marker));
    let planning_signal = [
        "埋下伏笔",
        "留下伏笔",
        "作铺垫",
        "悬念落在",
        "将在",
        "展开",
        "揭晓",
        "推进",
        "foreshadow",
        "sets up",
        "will reveal",
        "will unfold",
    ]
    .iter()
    .any(|marker| trimmed.contains(marker) || lowered.contains(marker));

    current_chapter && future_chapter && planning_signal
}

/// Removes short, bracketed author-facing beat labels that a model may place
/// inside otherwise valid prose. This extends the existing planning-meta
/// cleanup to streamed bodies where an entire chapter can arrive on one line.
pub(crate) fn strip_inline_story_planning_labels(text: &str) -> String {
    if !text.contains('【') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('【') {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + '【'.len_utf8()..];
        let Some(close) = after_open.find('】') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inner = &after_open[..close];
        let author_annotation_boundary = out
            .trim_end()
            .chars()
            .next_back()
            .is_none_or(|ch| matches!(ch, '。' | '！' | '？' | '；' | '\n' | '\r'));
        if author_annotation_boundary && inline_story_planning_label_inner(inner) {
            rest = &after_open[close + '】'.len_utf8()..];
        } else {
            out.push('【');
            out.push_str(inner);
            out.push('】');
            rest = &after_open[close + '】'.len_utf8()..];
        }
    }
    out.push_str(rest);
    out
}

fn inline_story_planning_label_inner(inner: &str) -> bool {
    let trimmed = inner.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 48 {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    [
        "抉择时刻",
        "转折时刻",
        "冲突升级",
        "情绪转折",
        "伏笔设置",
        "悬念设置",
        "场景目标",
        "叙事节点",
        "章节功能",
        "镜头切换",
        "choice moment",
        "plot beat",
        "scene goal",
        "chapter function",
    ]
    .iter()
    .any(|marker| trimmed.contains(marker) || lowered.contains(marker))
}

fn cjk_object_part_boundary_should_insert(chars: &[char], object_index: usize) -> bool {
    let Some(next) = chars.get(object_index + 1).copied() else {
        return false;
    };
    if !cjk_object_part_boundary_object(chars[object_index])
        || !cjk_object_part_boundary_part(next)
        || chars
            .get(object_index.wrapping_sub(1))
            .is_some_and(|ch| cjk_sentence_or_clause_boundary(*ch))
    {
        return false;
    }
    if object_index + 2 < chars.len()
        && matches!(next, '身' | '头' | '面')
        && chars[object_index + 2] == '体'
    {
        return false;
    }
    cjk_object_part_boundary_has_action_context(chars, object_index)
        && cjk_object_part_boundary_has_predicate_after_part(chars, object_index + 1)
}

fn cjk_object_part_boundary_has_action_context(chars: &[char], object_index: usize) -> bool {
    let start = object_index.saturating_sub(12);
    let context = chars[start..object_index].iter().collect::<String>();
    [
        "收", "收回", "握紧", "握住", "握着", "拿起", "提起", "举起", "拔出", "拔", "按住", "攥紧",
        "抓住", "持",
    ]
    .iter()
    .any(|marker| context.contains(marker))
}

fn cjk_object_part_boundary_has_predicate_after_part(chars: &[char], part_index: usize) -> bool {
    let end = (part_index + 8).min(chars.len());
    let after = chars[part_index + 1..end].iter().collect::<String>();
    [
        "滴落",
        "落下",
        "坠下",
        "亮起",
        "亮了",
        "发亮",
        "发出",
        "泛起",
        "浮现",
        "裂开",
        "震颤",
        "颤动",
        "镶嵌",
        "刻着",
        "刻有",
        "纹路",
        "黑色纹路",
    ]
    .iter()
    .any(|marker| after.contains(marker))
}

fn cjk_object_part_boundary_object(ch: char) -> bool {
    matches!(
        ch,
        '剑' | '刀'
            | '杖'
            | '枪'
            | '戟'
            | '矛'
            | '弓'
            | '笔'
            | '书'
            | '符'
            | '玉'
            | '珠'
            | '镜'
            | '令'
            | '牌'
            | '柱'
            | '门'
            | '钟'
            | '灯'
            | '盒'
            | '卷'
    )
}

fn cjk_object_part_boundary_part(ch: char) -> bool {
    matches!(
        ch,
        '尖' | '刃' | '柄' | '身' | '头' | '面' | '背' | '锋' | '纹'
    )
}

fn cjk_sentence_or_clause_boundary(ch: char) -> bool {
    matches!(
        ch,
        '。' | '！'
            | '？'
            | '，'
            | '、'
            | '；'
            | '：'
            | ','
            | ';'
            | ':'
            | '“'
            | '”'
            | '「'
            | '」'
            | '『'
            | '』'
            | '（'
            | '）'
            | '('
            | ')'
    )
}

pub(crate) fn sanitize_contract_surface_text(text: &str) -> String {
    let cleaned = strip_inline_cjk_markup_noise(&strip_generation_markup_noise(text))
        .trim()
        .to_string();
    let cleaned = close_trailing_unbalanced_cjk_delimiters(&cleaned);
    if contains_legal_contract_residue(&cleaned)
        || contains_excessive_repeated_cjk_surface_noise(&cleaned)
    {
        String::new()
    } else {
        cleaned
    }
}

/// Completes a short generated contract scalar when it was truncated after an
/// opening CJK quote or title bracket. Mismatched closing delimiters are left
/// untouched so the quality gate can still reject ambiguous corruption.
pub(crate) fn close_trailing_unbalanced_cjk_delimiters(text: &str) -> String {
    let mut expected_closers = Vec::new();
    for ch in text.chars() {
        let closer = match ch {
            '“' => Some('”'),
            '‘' => Some('’'),
            '「' => Some('」'),
            '『' => Some('』'),
            '《' => Some('》'),
            _ => None,
        };
        if let Some(closer) = closer {
            expected_closers.push(closer);
            continue;
        }
        if matches!(ch, '”' | '’' | '」' | '』' | '》') {
            if expected_closers.pop() != Some(ch) {
                return text.to_string();
            }
        }
    }
    if expected_closers.is_empty() || expected_closers.len() > 2 {
        return text.to_string();
    }
    let mut repaired = text.to_string();
    repaired.extend(expected_closers.into_iter().rev());
    repaired
}

pub(crate) fn contains_creation_request_control_residue(text: &str) -> bool {
    let compact = text.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    let lowered = compact.to_ascii_lowercase();
    [
        "target_units",
        "chapter_unit_target",
        "expected_chapters",
        "max_chapters_per_turn",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
        || (["每章", "总字数", "目标字数", "字数", "章节数"]
            .iter()
            .any(|marker| compact.contains(marker))
            && contains_creation_size_parameter(&compact))
        || ((compact.contains("至少")
            || compact.contains("起步")
            || compact.contains("一共")
            || compact.contains("总共")
            || compact.contains("全文")
            || compact.contains("全书")
            || compact.contains("整部"))
            && compact.chars().any(|ch| ch.is_ascii_digit())
            && (compact.contains('万') || compact.contains("word") || compact.contains("字")))
}

fn contains_creation_size_parameter(compact: &str) -> bool {
    compact.chars().any(|ch| ch.is_ascii_digit())
        && (compact.contains('字')
            || compact.contains('万')
            || compact.contains('千')
            || compact.contains("章")
            || compact.to_ascii_lowercase().contains("word"))
}

pub(crate) fn contains_generic_contract_placeholder_residue(text: &str) -> bool {
    let compact = text.replace(char::is_whitespace, "");
    compact.is_empty()
        || compact.contains("根据题材")
        || compact.contains("当前题材")
        || compact.contains("用当前题材")
        || compact.contains("形成持续阅读期待")
        || compact.contains("避免连续章节形态单一")
        || compact.contains("字段完整度")
        || contains_lettered_role_placeholder(text)
}

fn contains_lettered_role_placeholder(text: &str) -> bool {
    [
        "嫌疑人",
        "主角",
        "反派",
        "对手",
        "同伴",
        "导师",
        "盟友",
        "角色",
        "人物",
        "suspect",
        "character",
        "protagonist",
        "antagonist",
    ]
    .iter()
    .any(|marker| {
        let lowered = text.to_ascii_lowercase();
        let marker = marker.to_ascii_lowercase();
        let mut rest = lowered.as_str();
        while let Some(index) = rest.find(&marker) {
            let after = rest[index + marker.len()..].trim_start_matches(|ch: char| {
                ch.is_whitespace() || matches!(ch, ':' | '：' | '-' | '_' | '(' | '（')
            });
            let mut chars = after.chars();
            let slot = chars.next();
            let following = chars.next();
            if slot.is_some_and(|ch| ch.is_ascii_alphabetic())
                && !following
                    .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            {
                return true;
            }
            rest = &rest[index + marker.len()..];
        }
        false
    })
}

pub(crate) fn contains_excessive_repeated_cjk_surface_noise(text: &str) -> bool {
    let mut previous = None;
    let mut run_len = 0usize;
    for ch in text.chars() {
        if Some(ch) == previous {
            run_len += 1;
        } else {
            previous = Some(ch);
            run_len = 1;
        }
        if is_cjk_unified(ch) && run_len > 3 {
            return true;
        }
    }
    false
}

/// Detects only surface corruption that can be established without interpreting
/// prose semantics. Domain vocabulary, names, and stylistic choices deliberately
/// do not belong here.
pub(crate) fn high_confidence_surface_issue(text: &str) -> Option<String> {
    if text.contains('\u{fffd}') {
        return Some("replacement character U+FFFD in generated text".to_string());
    }
    if text
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Some("unexpected control character in generated text".to_string());
    }
    for (open, close, label) in [
        ('“', '”', "unbalanced Chinese double quotes"),
        ('‘', '’', "unbalanced Chinese single quotes"),
        ('「', '」', "unbalanced corner quotes"),
        ('《', '》', "unbalanced Chinese title brackets"),
    ] {
        let openings = text.chars().filter(|ch| *ch == open).count();
        let closings = text.chars().filter(|ch| *ch == close).count();
        if openings != closings {
            return Some(format!(
                "{label}: expected matching {open}{close} pairs, found {openings} openings and {closings} closings"
            ));
        }
    }
    if contains_excessive_repeated_cjk_surface_noise(text) {
        return Some("excessive repeated CJK character run".to_string());
    }
    None
}

pub(crate) fn strip_inline_cjk_markup_noise(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if is_inline_markup_noise(ch) {
            let start = index;
            while index < chars.len() && is_inline_markup_noise(chars[index]) {
                index += 1;
            }
            let prev = previous_non_noise_char(&chars, start);
            let next = chars.get(index).copied();
            if prev.is_some_and(is_cjk_unified) && next.is_some_and(is_cjk_unified) {
                if prev == next {
                    index += 1;
                }
                continue;
            }
            out.extend(chars[start..index].iter());
            continue;
        }
        out.push(ch);
        index += 1;
    }
    out
}

pub(crate) fn strip_cjk_markup_residue_lines(content: &str) -> String {
    content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .filter_map(clean_cjk_markup_residue_line)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn clean_cjk_markup_residue_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if line_is_standalone_markup_residue(trimmed) {
        return None;
    }
    let mut cleaned = strip_leading_short_escape_residue_before_cjk(line);
    cleaned = strip_leading_markup_wrapper_residue_before_cjk(&cleaned);
    cleaned = strip_short_escape_residue_near_cjk_line(&cleaned);
    cleaned = strip_latex_arrow_residue_from_cjk_line(&cleaned);
    let cleaned = cleaned.trim_end().to_string();
    (!cleaned.trim().is_empty()).then_some(cleaned)
}

pub(crate) fn line_is_standalone_markup_residue(line: &str) -> bool {
    let compact = line
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.is_empty() || compact.chars().count() > 16 {
        return false;
    }
    let lowered = compact.to_ascii_lowercase();
    let has_markup = compact.contains('\\')
        || compact.contains('$')
        || lowered.contains("rightarrow")
        || lowered.contains("ightarrow");
    has_markup
        && compact.chars().all(|ch| {
            ch.is_ascii_alphabetic() || matches!(ch, '\\' | '$' | '_' | '^' | '{' | '}' | '-')
        })
}

pub(crate) fn strip_short_escape_residue_near_cjk(content: &str) -> String {
    content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(strip_short_escape_residue_near_cjk_line)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn strip_short_escape_residue_near_cjk_line(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '\\' {
            out.push(chars[index]);
            index += 1;
            continue;
        }

        let residue_start = index;
        let prev = previous_non_whitespace_char(&chars, residue_start);
        index += 1;
        while chars.get(index).is_some_and(|ch| ch.is_whitespace()) {
            index += 1;
        }
        let letters_start = index;
        while chars.get(index).is_some_and(|ch| ch.is_ascii_alphabetic()) {
            index += 1;
        }
        let letter_count = index.saturating_sub(letters_start);
        while chars.get(index).is_some_and(|ch| ch.is_whitespace()) {
            index += 1;
        }
        let next = chars.get(index).copied();
        if (letter_count == 0 || (1..=3).contains(&letter_count))
            && is_cjk_noise_context(prev, next)
        {
            continue;
        }

        out.push('\\');
        out.extend(chars[residue_start + 1..index].iter());
    }
    out
}

pub(crate) fn strip_latex_arrow_residue_from_cjk_line(line: &str) -> String {
    if !line.chars().any(is_cjk_unified) {
        return line.to_string();
    }
    let mut cleaned = line.trim_start().to_string();
    let leading_ws = line.len().saturating_sub(cleaned.len());
    for _ in 0..4 {
        let before = cleaned.clone();
        let lowered = cleaned.to_ascii_lowercase();
        for marker in [
            "$\\rightarrow$",
            "$\\\\rightarrow$",
            "\\rightarrow",
            "\\\\rightarrow",
            "rightarrow$",
            "ightarrow$",
            "rightarrow",
            "ightarrow",
            "→",
        ] {
            if lowered.starts_with(&marker.to_ascii_lowercase()) {
                cleaned = cleaned[marker.len()..]
                    .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '$')
                    .to_string();
                break;
            }
        }
        if cleaned == before {
            break;
        }
    }
    if cleaned.contains("rightarrow") || cleaned.contains("ightarrow") || cleaned.contains('$') {
        cleaned = cleaned.replace("\\rightarrow", "");
        cleaned = cleaned.replace("\\\\rightarrow", "");
        cleaned = cleaned.replace("rightarrow", "");
        cleaned = cleaned.replace("ightarrow", "");
        cleaned = cleaned.replace('$', "");
    }
    format!("{}{}", " ".repeat(leading_ws), cleaned.trim_start())
}

pub(crate) fn strip_leading_short_escape_residue_before_cjk(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    while chars.get(index).is_some_and(|ch| ch.is_whitespace()) {
        index += 1;
    }
    if chars.get(index) != Some(&'\\') {
        return line.to_string();
    }
    while chars
        .get(index)
        .is_some_and(|ch| *ch == '\\' || ch.is_whitespace())
    {
        index += 1;
    }
    let letters_start = index;
    while chars.get(index).is_some_and(|ch| ch.is_ascii_alphabetic()) {
        index += 1;
    }
    let letter_count = index.saturating_sub(letters_start);
    while chars.get(index).is_some_and(|ch| ch.is_whitespace()) {
        index += 1;
    }
    if (letter_count == 0 || (1..=3).contains(&letter_count))
        && chars
            .get(index)
            .is_some_and(|ch| is_cjk_unified(*ch) || is_cjk_noise_boundary(*ch))
    {
        return chars[index..].iter().collect();
    }
    line.to_string()
}

pub(crate) fn strip_leading_markup_wrapper_residue_before_cjk(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    while chars.get(index).is_some_and(|ch| ch.is_whitespace()) {
        index += 1;
    }
    let start = index;
    while chars.get(index).is_some_and(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                '\\' | '$' | '^' | '{' | '}' | '_' | '[' | ']' | '(' | ')'
            )
    }) {
        index += 1;
    }
    if index > start
        && chars
            .get(index)
            .is_some_and(|ch| is_cjk_unified(*ch) || is_cjk_noise_boundary(*ch))
    {
        return chars[index..].iter().collect();
    }
    line.to_string()
}

pub(crate) fn is_cjk_noise_context(prev: Option<char>, next: Option<char>) -> bool {
    (prev.is_some_and(is_cjk_noise_boundary)
        && next.is_none_or(|ch| is_cjk_unified(ch) || is_cjk_noise_boundary(ch)))
        || (prev.is_none_or(is_cjk_noise_boundary)
            && next.is_some_and(|ch| is_cjk_unified(ch) || is_cjk_noise_boundary(ch)))
}

pub(crate) fn is_cjk_noise_boundary(ch: char) -> bool {
    is_cjk_unified(ch)
        || ch.is_ascii_digit()
        || matches!(
            ch,
            '。' | '，'
                | '、'
                | '；'
                | '：'
                | '！'
                | '？'
                | '”'
                | '“'
                | '’'
                | '‘'
                | '）'
                | '（'
                | '》'
                | '《'
                | '」'
                | '「'
                | '.'
                | ','
                | ';'
                | ':'
                | '!'
                | '?'
                | '('
                | ')'
                | '['
                | ']'
                | '\\'
        )
}

pub(crate) fn collapse_adjacent_repeated_cjk_phrases(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < chars.len() {
        if let Some((phrase_len, consumed)) = repeated_parenthetical_cjk_phrase_at(&chars, index) {
            out.extend(chars[index..index + phrase_len].iter());
            index += consumed;
            continue;
        }
        let mut matched_len = None;
        for len in (2..=6).rev() {
            if index + len * 2 > chars.len() {
                continue;
            }
            let first = &chars[index..index + len];
            let second = &chars[index + len..index + len * 2];
            if first != second || !first.iter().copied().all(is_cjk_unified) {
                continue;
            }
            if first.windows(2).all(|pair| pair[0] == pair[1]) {
                continue;
            }
            matched_len = Some(len);
            break;
        }
        if let Some(len) = matched_len {
            out.extend(chars[index..index + len].iter());
            index += len * 2;
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn repeated_parenthetical_cjk_phrase_at(chars: &[char], index: usize) -> Option<(usize, usize)> {
    for phrase_len in (2..=8).rev() {
        let opening_index = index + phrase_len;
        let closing_index = opening_index + phrase_len + 1;
        if closing_index >= chars.len()
            || !chars[index..opening_index]
                .iter()
                .copied()
                .all(is_cjk_unified)
        {
            continue;
        }
        let (opening, closing) = match chars[opening_index] {
            '（' => ('（', '）'),
            '(' => ('(', ')'),
            _ => continue,
        };
        if chars[opening_index] == opening
            && chars[closing_index] == closing
            && chars[index..opening_index] == chars[opening_index + 1..closing_index]
        {
            return Some((phrase_len, phrase_len * 2 + 2));
        }
    }
    None
}

fn previous_non_whitespace_char(chars: &[char], before: usize) -> Option<char> {
    chars
        .get(..before)?
        .iter()
        .rev()
        .copied()
        .find(|ch| !ch.is_whitespace())
}

pub(crate) fn strip_json_string_line_wrappers(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(strip_json_string_line_wrapper)
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_json_string_line_wrapper(line: &str) -> String {
    let leading_ws = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<String>();
    let mut body = line.trim().to_string();
    if body.is_empty() {
        return String::new();
    }

    let had_trailing_json_comma = body.ends_with("\",") || body.ends_with("',");
    if had_trailing_json_comma {
        body.truncate(body.len().saturating_sub(2));
        body = body.trim_end().to_string();
    }

    let starts_json_quoted =
        (body.starts_with('"') || body.starts_with('\'')) && body.chars().any(is_cjk_unified);
    if starts_json_quoted && (had_trailing_json_comma || !has_balanced_outer_ascii_quote(&body)) {
        body = body
            .trim_start_matches(|ch| matches!(ch, '"' | '\'' | ',' | '，'))
            .trim_start()
            .to_string();
    }

    if had_trailing_json_comma {
        body = body
            .trim_end_matches(|ch| matches!(ch, '"' | '\'' | ',' | '，'))
            .trim_end()
            .to_string();
    }

    format!("{leading_ws}{body}")
}

fn has_balanced_outer_ascii_quote(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    if !matches!(first, '"' | '\'') {
        return true;
    }
    value.chars().skip(1).any(|ch| ch == first)
}

pub(crate) fn strip_markdown_frontmatter(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    if let Some(rest) = normalized.strip_prefix("---\n") {
        if let Some((_, body)) = rest.split_once("\n---\n") {
            return body.trim_start_matches('\n').to_string();
        }
    }
    normalized
}

pub(crate) fn line_is_assistant_surface_noise(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    let surface_markers = [
        "已确认合同摘要",
        "当前标准小说合同草案",
        "当前草案",
        "待确认的小说创作合同草案",
        "待确认的写作文档合同草案",
        "我还没有开始写正文",
        "我还没有开始写完整正文",
        "可修改合同项",
        "可修改说明",
        "下一步：",
        "如果合同还不满意",
        "如果已经可以",
        "若合同通过",
        "如果这个草案已经可以",
        "请回复“开始写",
        "请回复\"开始写",
        "回复“开始写",
        "回复\"开始写",
        "由于您尚未提供",
        "尚未提供具体",
        "请先提供",
        "请补充或选择",
        "不算正文",
        "不要声称文件已生成",
    ];
    if surface_markers
        .iter()
        .any(|marker| trimmed.contains(marker) || lowered.contains(&marker.to_ascii_lowercase()))
    {
        return true;
    }

    let cleaned = clean_surface_line(trimmed);
    cleaned.starts_with("你可以直接说")
        || cleaned.starts_with("你可以继续")
        || cleaned.starts_with("接下来请")
        || cleaned.starts_with("下面是")
        || cleaned.starts_with("当前大纲素材")
}

fn clean_surface_line(line: &str) -> String {
    line.trim()
        .trim_start_matches(|ch| {
            matches!(
                ch,
                '-' | '*' | '+' | '#' | '>' | ' ' | '\t' | '。' | ':' | '：'
            )
        })
        .trim()
        .to_string()
}

fn is_inline_markup_noise(ch: char) -> bool {
    matches!(ch, '\\' | '^' | '_' | '{' | '}' | '$')
}

fn previous_non_noise_char(chars: &[char], start: usize) -> Option<char> {
    let mut index = start;
    while index > 0 {
        index -= 1;
        let ch = chars[index];
        if !is_inline_markup_noise(ch) && !ch.is_whitespace() {
            return Some(ch);
        }
    }
    None
}

fn is_cjk_unified(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_request_control_residue_requires_size_parameter_for_chapter_wording() {
        assert!(!contains_creation_request_control_residue(
            "每章设置关键转折点，推动职场权力结构变化"
        ));
        assert!(contains_creation_request_control_residue(
            "每章2500字，至少5万字起"
        ));
        assert!(contains_creation_request_control_residue(
            "总字数50000，章节数20"
        ));
        assert!(contains_creation_request_control_residue(
            "发现更大的阴谋。target_units=100000chapter_unit_target=2500expected_chapters=40"
        ));
    }

    #[test]
    fn excessive_repeated_cjk_surface_noise_is_detected() {
        assert!(contains_excessive_repeated_cjk_surface_noise(
            "对白必须体现欲望：季季季季谢谢谢谢梁梁梁梁"
        ));
        assert!(!contains_excessive_repeated_cjk_surface_noise(
            "她轻轻推开门，说谢谢你。"
        ));
        assert!(!contains_excessive_repeated_cjk_surface_noise(
            "他摆摆手说：行行行，我马上去。"
        ));
    }

    #[test]
    fn contract_surface_sanitizer_drops_repeated_cjk_noise_fields() {
        assert_eq!(
            sanitize_contract_surface_text("钟钟钟钟钟钟钟钟钟钟钟钟"),
            ""
        );
        assert_eq!(
            sanitize_contract_surface_text("对白要短促克制，避免解释设定。"),
            "对白要短促克制，避免解释设定。"
        );
    }

    #[test]
    fn contract_placeholder_gate_detects_lettered_role_slots() {
        assert!(contains_generic_contract_placeholder_residue(
            "锁定嫌疑人A后发现对方只是替罪羊"
        ));
        assert!(contains_generic_contract_placeholder_residue(
            "character B hides the evidence"
        ));
        assert!(!contains_generic_contract_placeholder_residue(
            "锁定嫌疑人孟泊声后发现对方只是替罪羊"
        ));
        assert!(!contains_generic_contract_placeholder_residue(
            "副站长协助主角，对手K-7是自动化清洗协议"
        ));
        assert!(!contains_generic_contract_placeholder_residue(
            "对手Alpha启动角色R_2的备用协议"
        ));
    }

    #[test]
    fn generation_markup_cleanup_preserves_paths_and_currency() {
        assert_eq!(
            strip_generation_markup_noise(r"预算为 $100，路径是 C:\drafts\book。"),
            r"预算为 $100，路径是 C:\drafts\book。"
        );
    }

    #[test]
    fn repairs_action_object_part_boundary_before_quality_gate() {
        let raw = "他握着一柄水蓝色的长剑尖滴落着寒潭的冷水。";
        let repaired = repair_cjk_action_object_part_boundaries(raw);
        assert_eq!(repaired, "他握着一柄水蓝色的长剑，剑尖滴落着寒潭的冷水。");
        assert!(cjk_action_object_part_boundary_fragments(&repaired).is_empty());
    }

    #[test]
    fn collapses_repeated_parenthetical_cjk_label() {
        assert_eq!(
            collapse_adjacent_repeated_cjk_phrases("揭示梁知弦（梁知弦）的真实意图"),
            "揭示梁知弦的真实意图"
        );
        assert_eq!(
            collapse_adjacent_repeated_cjk_phrases("导师秦承弦(秦承弦)留下密钥"),
            "导师秦承弦留下密钥"
        );
    }

    #[test]
    fn preserves_normal_object_part_compound_without_action_boundary() {
        let raw = "陶照白拄着一根看似普通的竹杖头却镶嵌着一块幽蓝色的晶石。";
        let repaired = repair_cjk_action_object_part_boundaries(raw);
        assert_eq!(repaired, raw);
        assert!(cjk_action_object_part_boundary_fragments(raw).is_empty());
    }

    #[test]
    fn detects_chapter_planning_commentary_without_matching_narrative_prose() {
        assert!(line_looks_like_story_planning_meta(
            "本章以林汐踏入静默区为结，悬念落在未知声音的呼唤上，为下一章的身份揭秘埋下伏笔。"
        ));
        assert!(!line_looks_like_story_planning_meta(
            "林汐踏入静默区，身后的潮声忽然消失了。"
        ));
    }

    #[test]
    fn strips_inline_bracketed_planning_label_but_preserves_story_ui() {
        assert_eq!(
            strip_inline_story_planning_labels(
                "她按住齿轮。【抉择时刻：记忆的代价】指针开始倒转。"
            ),
            "她按住齿轮。指针开始倒转。"
        );
        assert_eq!(
            strip_inline_story_planning_labels("终端弹出【任务提示：离开站台】。"),
            "终端弹出【任务提示：离开站台】。"
        );
        assert_eq!(
            strip_inline_story_planning_labels("终端弹出【场景目标：突破封锁】。"),
            "终端弹出【场景目标：突破封锁】。"
        );
    }
}
