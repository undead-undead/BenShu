use super::*;

pub(crate) fn dedup_compact_contract_values(
    values: Vec<String>,
    max_items: usize,
    max_chars_each: usize,
) -> Vec<String> {
    let mut out = Vec::<String>::new();
    for value in values {
        let cleaned = sanitize_generated_contract_scalar(&value);
        if cleaned.is_empty() || out.iter().any(|existing| existing == &cleaned) {
            continue;
        }
        out.push(compact_creation_text(&cleaned, max_chars_each));
        if out.len() >= max_items {
            break;
        }
    }
    out
}

pub(crate) fn generated_contract_field(text: &str, labels: &[&str]) -> Option<String> {
    for line in text.lines() {
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
            if !value.is_empty() {
                let value = trim_generated_contract_inline_field_tail(value);
                if let Some(quoted) = leading_cjk_quote_value(&value) {
                    return Some(quoted);
                }
                return Some(value);
            }
        }
    }
    None
}

pub(crate) fn split_contract_field_line<'a>(
    line: &'a str,
    label: &str,
) -> Option<(&'a str, &'a str)> {
    let index = line.find(label)?;
    let prefix = line[..index].trim();
    let tail = line[index + label.len()..].trim_start();
    let tail = if let Some(tail) = tail.strip_prefix('：').or_else(|| tail.strip_prefix(':')) {
        tail.trim_start()
    } else if tail.starts_with('《') {
        tail
    } else {
        return None;
    };
    Some((prefix, tail))
}

pub(crate) fn first_cjk_quote_value(value: &str) -> Option<String> {
    let start = value.find('《')?;
    let rest = &value[start + '《'.len_utf8()..];
    let end = rest.find('》')?;
    let title = rest[..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

pub(crate) fn leading_cjk_quote_value(value: &str) -> Option<String> {
    let trimmed = value.trim_start();
    if !trimmed.starts_with('《') {
        return None;
    }
    first_cjk_quote_value(trimmed)
}

pub(crate) fn contract_field_prefix_allowed(prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    let compact = prefix.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return true;
    }
    if compact
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '、' | ')' | '）' | '-' | '：' | ':'))
    {
        return true;
    }
    let section_prefix = [
        "命名依据合同",
        "角色权威表",
        "基本参数",
        "故事合同",
        "结构合同",
        "近期章节包",
        "质量合同",
    ]
    .iter()
    .any(|section| compact.contains(section));
    if section_prefix {
        return true;
    }
    [
        "唯一",
        "项目",
        "小说",
        "作品",
        "文档",
        "当前",
        "标准小说合同草案",
        "标准合同草案",
        "基本参数",
        "基本参数：",
        "1.基本参数",
        "1.基本参数：",
        "命名依据合同",
        "命名依据合同：",
        "2.命名依据合同",
        "2.命名依据合同：",
        "角色权威表",
        "角色权威表：",
        "3.角色权威表",
        "3.角色权威表：",
    ]
    .iter()
    .any(|allowed| compact.ends_with(allowed))
}

pub(crate) fn sanitize_generated_contract_scalar(value: &str) -> String {
    let trimmed = trim_generated_contract_inline_field_tail(value);
    let scalar = trimmed
        .split(|ch| matches!(ch, '\n' | '\r'))
        .next()
        .unwrap_or_default()
        .trim();
    strip_matching_outer_quotes(scalar).trim().to_string()
}

fn strip_matching_outer_quotes(value: &str) -> &str {
    let Some(first) = value.chars().next() else {
        return value;
    };
    let Some(last) = value.chars().next_back() else {
        return value;
    };
    if !matches!(
        (first, last),
        ('"', '"') | ('\'', '\'') | ('“', '”') | ('‘', '’')
    ) {
        return value;
    }
    let start = first.len_utf8();
    let end = value.len().saturating_sub(last.len_utf8());
    if start > end {
        value
    } else {
        &value[start..end]
    }
}

pub(crate) fn trim_generated_contract_inline_field_tail(value: &str) -> String {
    let mut end = value.len();
    for marker in GENERATED_CONTRACT_INLINE_FIELD_BOUNDARIES {
        if let Some(index) = value.find(marker) {
            if index > 0 {
                end = end.min(index);
            }
        }
    }
    value[..end]
        .trim()
        .trim_end_matches(|ch| matches!(ch, '。' | '；' | ';' | '，' | ','))
        .trim()
        .to_string()
}

/// Restores line boundaries when a local model emits an otherwise valid
/// contract field pack as one long line. This extends the existing field-pack
/// boundary authority; callers should parse the returned text with the normal
/// field/character/outline parsers instead of maintaining another parser.
pub(crate) fn normalize_generated_contract_field_pack_lines(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 64);
    for (index, ch) in value.char_indices() {
        let tail = &value[index..];
        let static_boundary = GENERATED_CONTRACT_INLINE_FIELD_BOUNDARIES
            .iter()
            .any(|marker| tail.starts_with(marker));
        let character_entry_boundary =
            generated_contract_character_entry_marker(tail, out.chars().next_back());
        let plan_boundary = generated_contract_plan_marker(tail);
        if (static_boundary || character_entry_boundary || plan_boundary)
            && !out.trim_end().is_empty()
        {
            let previous = out.chars().next_back();
            if !matches!(previous, Some('\n' | '\r')) {
                out.push('\n');
            }
        }
        out.push(ch);
    }
    out
}

fn generated_contract_character_entry_marker(value: &str, previous: Option<char>) -> bool {
    let starts_entry = ["姓名：", "姓名:", "Name:", "name:"]
        .iter()
        .any(|marker| value.starts_with(marker));
    starts_entry
        && previous.is_none_or(|ch| {
            ch.is_whitespace() || matches!(ch, '：' | ':' | '。' | '；' | ';' | '|' | '｜')
        })
}

fn generated_contract_plan_marker(value: &str) -> bool {
    let mut chars = value.chars();
    if chars.next() != Some('第') {
        return false;
    }
    let mut saw_number = false;
    for ch in chars.by_ref().take(8) {
        if generated_contract_plan_number_char(ch) {
            saw_number = true;
            continue;
        }
        if saw_number && matches!(ch, '卷' | '章') {
            let suffix = chars.as_str();
            let Some(first) = suffix.chars().next() else {
                return false;
            };
            if matches!(first, '：' | ':' | '《' | '〈' | '“' | '"' | '-' | '—') {
                return true;
            }
            if !first.is_whitespace() {
                return false;
            }
            let suffix = suffix.trim_start();
            return [
                "本卷目标",
                "卷名：",
                "卷名:",
                "阶段目标：",
                "阶段目标:",
                "本章目标",
                "章节目标",
                "章目标",
                "目标：",
                "目标:",
            ]
            .iter()
            .any(|prefix| suffix.starts_with(prefix));
        }
        return false;
    }
    false
}

fn generated_contract_plan_number_char(ch: char) -> bool {
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

pub(super) const GENERATED_CONTRACT_INLINE_FIELD_BOUNDARIES: &[&str] = &[
    "题材：",
    "类型：",
    "简述：",
    "语言：",
    "总字数：",
    "目标字数：",
    "每章档位：",
    "每章字数：",
    "故事前提：",
    "终局方向：",
    "结局方向：",
    "终局状态：",
    "结局状态：",
    "主角弧线：",
    "成长线：",
    "世界观意象：",
    "世界意象：",
    "核心意象：",
    "总主线因果链：",
    "主线因果链：",
    "书名：",
    "标题：",
    "书名候选：",
    "标题候选：",
    "命名理由：",
    "书名理由：",
    "标题理由：",
    "角色权威表：",
    "核心主题：",
    "世界规则：",
    "叙事风格：",
    "必须避免：",
    "全书大纲：",
    "故事大纲：",
    "分卷规划：",
    "阶段规划：",
    "近期章节包：",
    "章节规划：",
    "伏笔/承诺兑现矩阵：",
    "伏笔矩阵：",
    "结构合同：",
    "质量合同：",
    "Genre:",
    "Brief:",
    "Language:",
    "Title:",
    "Title Candidates:",
    "Premise:",
    "Ending Direction:",
    "Final State:",
    "Protagonist Arc:",
    "World Imagery:",
    "Main Causal Spine:",
    "Title Rationale:",
    "Theme:",
    "Themes:",
    "World:",
    "Rules:",
    "Style:",
    "Must Avoid:",
    "Outline:",
];

#[cfg(test)]
mod tests {
    use super::{
        normalize_generated_contract_field_pack_lines, sanitize_generated_contract_scalar,
    };

    #[test]
    fn scalar_sanitizer_only_removes_matching_outer_quotes() {
        assert_eq!(sanitize_generated_contract_scalar("“完整字段”"), "完整字段");
        assert_eq!(
            sanitize_generated_contract_scalar("\"complete field\""),
            "complete field"
        );
        assert_eq!(
            sanitize_generated_contract_scalar("顾望衡暗示“书是特意留给懂行的人”"),
            "顾望衡暗示“书是特意留给懂行的人”"
        );
        assert_eq!(
            sanitize_generated_contract_scalar("未闭合“引语"),
            "未闭合“引语"
        );
    }

    #[test]
    fn one_line_field_pack_restores_fields_and_plan_entries() {
        let normalized = normalize_generated_contract_field_pack_lines(
            "必须避免：不要漂移；不要改名全书大纲：查明断枢真相分卷规划：第一卷：入城；目标：查账；卷尾变化：发现伪账；第二卷：断枢；目标：公开证据；卷尾变化：旧制瓦解近期章节包：第1章 本章目标：取得账册；预期转折：账册被换第2章 本章目标：追查换册人；预期转折：同伴留下暗号",
        );

        assert!(normalized.contains("不要改名\n全书大纲："));
        assert!(normalized.contains("分卷规划：\n第一卷："));
        assert!(normalized.contains("卷尾变化：发现伪账；\n第二卷："));
        assert!(normalized.contains("近期章节包：\n第1章 "));
        assert!(normalized.contains("账册被换\n第2章 "));
    }

    #[test]
    fn one_line_initial_batch_restores_scalar_and_character_entries() {
        let normalized = normalize_generated_contract_field_pack_lines(
            "题材：赛博朋克简述：拾荒者发现零号记忆终局方向：主角公开证据终局状态：城市记忆交易被永久废止书名：零号雨书名候选：零号雨；天枢裂隙；记忆拾荒者角色权威表：姓名：顾望衡，角色：主角，欲望：查明真相。姓名：秦照野，角色：关键同伴，欲望：守住证据。",
        );

        assert!(normalized.contains("题材：赛博朋克\n简述："));
        assert!(normalized.contains("终局方向：主角公开证据\n终局状态："));
        assert!(normalized.contains("书名：零号雨\n书名候选："));
        assert!(normalized.contains("角色权威表：\n姓名：顾望衡"));
        assert!(normalized.contains("顾望衡，角色：主角，欲望：查明真相。\n姓名：秦照野"));
    }

    #[test]
    fn prose_volume_references_do_not_gain_plan_boundaries() {
        let normalized = normalize_generated_contract_field_pack_lines(
            "全书大纲：第1卷中，主角取得第一份证据；第2卷 中他继续追查。分卷规划：第1卷《起势》：本卷目标：取得证据。",
        );

        assert!(normalized.contains("第1卷中"));
        assert!(normalized.contains("第2卷 中"));
        assert!(normalized.contains("分卷规划：\n第1卷《起势》"));
    }
}
