use super::*;

#[cfg(test)]
pub(crate) fn generated_contract_character_names(contract_text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(name) = generated_contract_field(contract_text, &["主角", "Protagonist"]) {
        let name = clean_generated_character_name(&name);
        if !name.is_empty() {
            out.push(name);
        }
    }
    for line in contract_text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if !(trimmed.contains("主角")
            || trimmed.contains("人物")
            || trimmed.contains("角色")
            || lower.contains("character")
            || lower.contains("protagonist"))
        {
            continue;
        }
        for marker in ["name:", "name：", "名字：", "姓名：", "主角：", "主角:"] {
            let Some((_, tail)) = trimmed.split_once(marker) else {
                continue;
            };
            let name = clean_generated_character_name(tail);
            if !name.is_empty() {
                out.push(name.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
pub fn generated_contract_forbidden_name_surfaces(contract_text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in contract_text.lines() {
        let cleaned = clean_contract_line(line);
        if !(cleaned.contains("书名") || cleaned.contains("标题")) {
            continue;
        }
        collect_quoted_cjk_names(&cleaned, &mut names);
        if let Some(value) = generated_contract_field(&cleaned, &["书名", "标题", "Title"]) {
            push_forbidden_name_surface(&mut names, &sanitize_generated_contract_scalar(&value));
        }
    }
    dedup_compact_contract_values(names, 16, 32)
}

#[cfg(test)]
pub(crate) fn collect_quoted_cjk_names(text: &str, out: &mut Vec<String>) {
    let mut rest = text;
    while let Some(start) = rest.find('《') {
        let after_start = &rest[start + '《'.len_utf8()..];
        let Some(end) = after_start.find('》') else {
            break;
        };
        push_forbidden_name_surface(out, &after_start[..end]);
        rest = &after_start[end + '》'.len_utf8()..];
    }
}

#[cfg(test)]
pub(crate) fn push_forbidden_name_surface(out: &mut Vec<String>, value: &str) {
    let candidate = value
        .trim()
        .trim_matches(|ch| matches!(ch, '《' | '》' | '"' | '\'' | '“' | '”'))
        .split(|ch| {
            matches!(
                ch,
                '，' | ',' | '；' | ';' | '。' | '.' | '：' | ':' | '、' | ' '
            )
        })
        .next()
        .unwrap_or_default()
        .trim();
    let len = candidate.chars().count();
    if (2..=8).contains(&len)
        && candidate.chars().any(is_cjk_unified)
        && ![
            "书名",
            "标题",
            "主角",
            "角色",
            "对手",
            "反派",
            "未命名",
            "待定",
        ]
        .iter()
        .any(|noise| candidate.contains(noise))
    {
        out.push(candidate.to_string());
    }
}

#[cfg(test)]
pub(crate) fn clean_generated_character_name(value: &str) -> String {
    let name = value
        .split(|ch| matches!(ch, ';' | '；' | ',' | '，' | '、' | ':' | '：' | ' ' | '\t'))
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches(|ch| matches!(ch, '《' | '》' | '"' | '\'' | '“' | '”' | '*' | '-'));
    if name.chars().count() >= 2 && name.chars().count() <= 6 {
        name.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
pub(crate) fn normalize_cjk_identity(value: &str) -> String {
    value
        .chars()
        .filter(|ch| is_cjk_unified(*ch) || ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

pub(crate) fn malformed_numeric_fragment(text: &str) -> Option<String> {
    let chars = text.chars().collect::<Vec<_>>();
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '_' {
            continue;
        }
        let prev_digit = index
            .checked_sub(1)
            .and_then(|idx| chars.get(idx))
            .is_some_and(|ch| ch.is_ascii_digit());
        let next_digit = chars.get(index + 1).is_some_and(|ch| ch.is_ascii_digit());
        if prev_digit && next_digit {
            let start = index.saturating_sub(12);
            let end = (index + 13).min(chars.len());
            return Some(chars[start..end].iter().collect::<String>());
        }
    }
    for (index, ch) in chars.iter().enumerate() {
        if !ch.is_ascii_digit() {
            continue;
        }
        let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
        let next = chars.get(index + 1).copied();
        let prev_cjk = prev.is_some_and(is_cjk_unified);
        let next_cjk = next.is_some_and(is_cjk_unified);
        if !prev_cjk || !next_cjk {
            continue;
        }
        let prev_allowed = prev.is_some_and(|value| matches!(value, '第' | '约'));
        let next_allowed = next.is_some_and(|value| {
            matches!(
                value,
                '章' | '卷'
                    | '字'
                    | '万'
                    | '千'
                    | '百'
                    | '个'
                    | '部'
                    | '年'
                    | '月'
                    | '日'
                    | '次'
                    | '轮'
                    | '节'
                    | '幕'
                    | '页'
                    | '元'
                    | '点'
                    | '级'
                    | '层'
                    | '倍'
                    | '分'
                    | '秒'
            )
        });
        let normal_zero_start_phrase = *ch == '0'
            && prev.is_some_and(|value| matches!(value, '从' | '由'))
            && next.is_some_and(|value| matches!(value, '开' | '起'));
        if prev_allowed || next_allowed || normal_zero_start_phrase {
            continue;
        }
        if !looks_like_contract_number_join(prev, next) {
            continue;
        }
        let start = index.saturating_sub(8);
        let end = (index + 9).min(chars.len());
        return Some(chars[start..end].iter().collect::<String>());
    }
    None
}

fn looks_like_contract_number_join(prev: Option<char>, next: Option<char>) -> bool {
    let Some(prev) = prev else {
        return false;
    };
    let Some(next) = next else {
        return false;
    };
    let previous_field_boundary = matches!(
        prev,
        '字' | '章' | '卷' | '节' | '幕' | '部' | '项' | '条' | '点' | '次' | '共' | '计'
    );
    let next_field_heading = matches!(
        next,
        '每' | '总'
            | '作'
            | '品'
            | '章'
            | '节'
            | '卷'
            | '书'
            | '名'
            | '主'
            | '角'
            | '题'
            | '材'
            | '结'
            | '局'
            | '世'
            | '界'
            | '大'
            | '纲'
            | '分'
            | '交'
            | '付'
            | '乙'
            | '甲'
            | '需'
            | '在'
            | '按'
            | '频'
    );
    previous_field_boundary && next_field_heading
}

pub(crate) fn generated_fiction_character_lines(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    for label in [
        "主角/重要角色",
        "主角/主要角色",
        "主角与重要角色",
        "主角和重要角色",
        "主角名",
        "主角姓名",
        "主角名称",
        "主人公名",
        "主人公姓名",
        "主人公名称",
        "男主角",
        "女主角",
        "男主姓名",
        "女主姓名",
        "男主",
        "女主",
        "核心角色",
        "角色锚点",
        "人物锚点",
        "角色权威表",
        "人物权威表",
        "角色档案",
        "人物档案",
        "主要角色",
        "角色",
        "反派",
        "对手",
        "同伴",
        "导师",
        "Protagonist",
        "Antagonist",
    ] {
        if let Some(value) = generated_contract_field(text, &[label]) {
            let raw_entry = format!("{label}: {value}");
            for entry in split_fiction_character_contract_entries(&raw_entry) {
                if let Some(normalized) = normalize_fiction_character_contract_line(&entry) {
                    values.push(normalized);
                }
            }
        }
    }
    let mut capture = false;
    for line in text.lines() {
        let cleaned = clean_contract_line(line);
        if cleaned.is_empty() {
            continue;
        }
        if capture && line_looks_like_section_boundary(&cleaned) {
            capture = false;
        }
        if capture || fiction_character_line_has_role_signal(&cleaned) {
            if let Some(normalized) = normalize_fiction_character_contract_line(&cleaned) {
                values.push(normalized);
                continue;
            }
        }
        if line_looks_like_contract_heading(&cleaned) {
            capture = line_starts_character_contract_block(&cleaned);
            if capture {
                if let Some((_, tail)) =
                    cleaned.split_once('：').or_else(|| cleaned.split_once(':'))
                {
                    for entry in split_fiction_character_contract_entries(tail) {
                        if let Some(normalized) = normalize_fiction_character_contract_line(&entry)
                        {
                            values.push(normalized);
                        }
                    }
                }
            }
            continue;
        }
    }
    dedup_compact_contract_values(values, 12, 260)
}

pub(crate) fn line_starts_character_contract_block(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    [
        "角色权威表",
        "人物权威表",
        "角色档案",
        "人物档案",
        "主要角色",
        "核心角色",
        "角色锚点",
        "人物锚点",
        "character ledger",
        "characters",
        "character anchors",
    ]
    .iter()
    .any(|label| line.contains(label) || lowered.contains(&label.to_ascii_lowercase()))
}

pub(crate) fn split_fiction_character_contract_entries(value: &str) -> Vec<String> {
    let mut normalized = value
        .replace("；姓名", "\n姓名")
        .replace(";姓名", "\n姓名")
        .replace("；名字", "\n名字")
        .replace(";名字", "\n名字")
        .replace("；name", "\nname")
        .replace(";name", "\nname");
    for marker in [
        "主角姓名",
        "主角名字",
        "主人公姓名",
        "男主姓名",
        "女主姓名",
        "对手姓名",
        "反派姓名",
        "关键配角姓名",
        "配角姓名",
        "导师姓名",
        "同伴姓名",
        "canonical_name",
    ] {
        normalized = normalized
            .replace(&format!("；{marker}"), &format!("\n{marker}"))
            .replace(&format!(";{marker}"), &format!("\n{marker}"));
    }
    normalized
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn generated_fiction_outline(text: &str) -> String {
    let mut lines = Vec::new();
    let mut capture = false;
    for line in text.lines() {
        let cleaned = clean_contract_line(line);
        if cleaned.is_empty() {
            continue;
        }
        if line_looks_like_explicit_chapter_plan(&cleaned) {
            lines.push(cleaned);
            continue;
        }
        if line_looks_like_contract_heading(&cleaned) {
            capture = line_starts_fiction_outline_block(&cleaned);
            if capture && line_looks_like_explicit_chapter_plan(&cleaned) {
                lines.push(cleaned);
            } else if capture {
                if let Some((_, tail)) =
                    cleaned.split_once('：').or_else(|| cleaned.split_once(':'))
                {
                    let tail = tail.trim();
                    if !tail.is_empty() {
                        lines.push(tail.to_string());
                    }
                }
            }
            continue;
        }
        if capture {
            lines.push(cleaned);
        }
    }
    dedup_compact_contract_values(lines, 80, 260).join("\n")
}

pub(crate) fn line_starts_fiction_outline_block(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    line_starts_chapter_plan_block(line)
        || [
            "大纲",
            "阶段规划",
            "分卷规划",
            "卷宗规划",
            "剧情规划",
            "情节规划",
            "结构合同",
            "故事结构",
            "outline",
            "plot outline",
            "story structure",
            "volume plan",
        ]
        .iter()
        .any(|term| line.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub(crate) fn clean_contract_line(line: &str) -> String {
    line.trim()
        .trim_start_matches(|ch| matches!(ch, '*' | '-' | '+' | '#' | ' ' | '\t'))
        .replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .trim()
        .to_string()
}

pub(crate) fn line_looks_like_contract_heading(cleaned: &str) -> bool {
    line_is_bare_contract_section_heading(cleaned)
        || cleaned.ends_with('：')
        || cleaned.ends_with(':')
        || cleaned.starts_with("##")
        || cleaned.contains("：")
        || cleaned.contains(':')
}

pub(crate) fn line_is_bare_contract_section_heading(line: &str) -> bool {
    let cleaned = clean_contract_line(line);
    let heading = cleaned
        .trim_end_matches(['：', ':'])
        .trim()
        .trim_start_matches(|ch: char| {
            ch.is_ascii_digit() || matches!(ch, '.' | '、' | ')' | '）' | '-' | ' ' | '\t')
        })
        .trim();
    super::field_pack::GENERATED_CONTRACT_INLINE_FIELD_BOUNDARIES
        .iter()
        .map(|label| label.trim_end_matches(['：', ':']).trim())
        .any(|label| heading.eq_ignore_ascii_case(label))
}

pub(crate) fn strip_contract_section_heading_residue(value: &str) -> String {
    value
        .lines()
        .filter_map(|line| {
            let mut cleaned = line.trim().to_string();
            if cleaned.is_empty() || line_is_bare_contract_section_heading(&cleaned) {
                return None;
            }
            for label in super::field_pack::GENERATED_CONTRACT_INLINE_FIELD_BOUNDARIES
                .iter()
                .map(|label| label.trim_end_matches(['：', ':']).trim())
            {
                let comparable = cleaned.trim_end_matches(['：', ':']).trim_end();
                let Some(prefix) = comparable.strip_suffix(label) else {
                    continue;
                };
                let separated = prefix.is_empty()
                    || prefix.chars().next_back().is_some_and(|ch| {
                        ch.is_whitespace() || matches!(ch, '。' | '.' | '；' | ';')
                    })
                    || fused_section_heading_residue(label);
                if separated {
                    cleaned = prefix.trim_end().to_string();
                    break;
                }
            }
            (!cleaned.is_empty()).then_some(cleaned)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn fused_section_heading_residue(label: &str) -> bool {
    matches!(label, "近期章节包" | "伏笔矩阵" | "结构合同" | "质量合同")
}

pub(crate) fn contract_text_contains_section_heading_residue(value: &str) -> bool {
    value.lines().any(|line| {
        let cleaned = line.trim();
        if line_is_bare_contract_section_heading(cleaned) {
            return true;
        }
        super::field_pack::GENERATED_CONTRACT_INLINE_FIELD_BOUNDARIES
            .iter()
            .map(|label| label.trim_end_matches(['：', ':']).trim())
            .any(|label| {
                let comparable = cleaned.trim_end_matches(['：', ':']).trim_end();
                comparable.strip_suffix(label).is_some_and(|prefix| {
                    prefix.chars().next_back().is_some_and(|ch| {
                        ch.is_whitespace() || matches!(ch, '。' | '.' | '；' | ';')
                    }) || fused_section_heading_residue(label)
                })
            })
    })
}

pub(crate) fn line_looks_like_section_boundary(cleaned: &str) -> bool {
    if line_looks_like_character_identity_entry(cleaned) {
        return false;
    }
    let known_section = [
        "标准小说合同",
        "故事合同",
        "结构合同",
        "质量合同",
        "可修改说明",
        "书名",
        "题材",
        "主题",
        "角色",
        "世界",
        "意象",
        "弧线",
        "主线",
        "因果",
        "力量",
        "风格",
        "章节",
        "卷宗",
        "结局",
        "Title",
        "Genre",
        "Theme",
        "Character",
        "World",
        "Style",
        "Chapter",
        "Ending",
    ]
    .iter()
    .any(|label| cleaned.contains(label));
    known_section
        && (line_looks_like_contract_heading(cleaned)
            || cleaned.ends_with("合同")
            || cleaned.ends_with("说明"))
}

fn line_looks_like_character_identity_entry(cleaned: &str) -> bool {
    let trimmed = cleaned.trim();
    let lowered = trimmed.to_ascii_lowercase();
    (trimmed.starts_with("姓名：")
        || trimmed.starts_with("姓名:")
        || trimmed.starts_with("名字：")
        || trimmed.starts_with("名字:")
        || lowered.starts_with("name:"))
        && (trimmed.contains("角色：")
            || trimmed.contains("角色:")
            || lowered.contains("role:")
            || trimmed.contains("主角")
            || trimmed.contains("对手")
            || trimmed.contains("反派")
            || trimmed.contains("同伴")
            || trimmed.contains("导师"))
}

pub(crate) fn fiction_character_line_has_role_signal(cleaned: &str) -> bool {
    let trimmed = cleaned
        .trim()
        .trim_start_matches(|ch: char| {
            ch.is_ascii_digit() || matches!(ch, '.' | ')' | '、' | '-' | '*' | '•' | ' ' | '\t')
        })
        .trim();
    [
        "主角",
        "主角名",
        "主角姓名",
        "主人公",
        "主人公名",
        "主人公姓名",
        "男主",
        "男主角",
        "女主",
        "女主角",
        "反派",
        "对手",
        "同伴",
        "导师",
        "姓名",
        "名字",
    ]
    .iter()
    .any(|label| {
        trimmed.starts_with(&format!("{label}："))
            || trimmed.starts_with(&format!("{label}:"))
            || trimmed.starts_with(&format!("{label}姓名："))
            || trimmed.starts_with(&format!("{label}姓名:"))
            || trimmed.starts_with(&format!("{label}名字："))
            || trimmed.starts_with(&format!("{label}名字:"))
    })
}

pub(crate) fn normalize_fiction_character_contract_line(cleaned: &str) -> Option<String> {
    let line = cleaned.trim();
    if line.is_empty() || line.contains("合同") || line.contains("说明") {
        return None;
    }
    if fiction_character_line_is_non_identity_contract_field(line) {
        return None;
    }
    let explicit_role =
        contract_line_detail_value(line, &["role", "角色", "角色定位", "身份", "定位"]);
    let role_basis = if explicit_role.trim().is_empty() {
        line
    } else {
        explicit_role.as_str()
    };
    let role = draft_character_role_from_basis(role_basis, &line.to_ascii_lowercase());
    let name = character_name_from_contract_line(line)?;
    if name == role
        || name.contains("合同")
        || name.contains("说明")
        || fiction_character_name_candidate_is_noise(&name)
    {
        return None;
    }
    if line.contains("desire")
        || line.contains("fear")
        || line.contains("bottom")
        || line.contains("欲望")
        || line.contains("恐惧")
        || line.contains("底线")
    {
        let core = compact_fiction_character_core_details(line);
        if !core.is_empty() {
            return Some(format!("name: {name}; role: {role}; {core}"));
        }
    }
    Some(format!(
        "name: {name}; role: {role}; desire: 完成本次故事合同中的核心目标; fear: 失去选择权或被既有秩序重新吞没; bottom_line: 不用无解释的背叛或牺牲无辜来换取胜利"
    ))
}

pub(crate) fn fiction_character_line_is_non_identity_contract_field(line: &str) -> bool {
    let prefix = line
        .split_once('：')
        .or_else(|| line.split_once(':'))
        .map(|(prefix, _)| prefix.trim())
        .unwrap_or_default();
    if prefix.is_empty() {
        return false;
    }
    [
        "终局方向",
        "结局方向",
        "结尾承诺",
        "终局承诺",
        "主角弧线",
        "主角弧光",
        "成长线",
        "世界观意象",
        "世界意象",
        "核心意象",
        "关键意象",
        "总主线因果链",
        "主线因果链",
        "主线因果",
        "核心矛盾",
        "故事前提",
        "创作前提",
        "书名",
        "标题",
        "命名理由",
        "书名理由",
        "标题理由",
    ]
    .iter()
    .any(|field| prefix.contains(field))
}

pub(crate) fn compact_fiction_character_core_details(line: &str) -> String {
    line.split(['，', ',', '；', ';'])
        .map(str::trim)
        .filter(|part| {
            [
                "命名依据",
                "欲望",
                "恐惧",
                "底线",
                "弧线起点",
                "弧线终点",
                "arc_start",
                "arc_end",
                "desire",
                "fear",
                "bottom",
                "bottom_line",
            ]
            .iter()
            .any(|marker| part.contains(marker))
        })
        .map(|part| {
            part.trim_start_matches(|ch: char| {
                ch.is_ascii_digit() || matches!(ch, '.' | ')' | '、' | '-' | '*' | '•' | ' ' | '\t')
            })
            .trim()
            .to_string()
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn character_name_from_contract_line(line: &str) -> Option<String> {
    for field in line.split(['；', ';', '，', ',', '\n', '\r']) {
        let field = field.trim();
        for label in [
            "canonical_name",
            "name",
            "姓名",
            "名字",
            "名称",
            "主角名",
            "主人公名",
            "角色名",
        ] {
            for separator in [":", "：", "="] {
                let prefix = format!("{label}{separator}");
                if let Some(rest) = field.strip_prefix(&prefix) {
                    if let Some(name) = usable_labeled_character_name_candidate(rest) {
                        return Some(name);
                    }
                }
            }
        }
    }

    let first = line
        .split(['；', ';', '，', ',', '\n', '\r'])
        .next()
        .unwrap_or(line)
        .trim();
    if !first.contains(':') && !first.contains('：') && !first.contains('=') {
        if let Some(name) = usable_cjk_character_name_candidate(first) {
            return Some(name);
        }
    }
    extract_cjk_character_name_near_role_signal(line)
}

fn usable_labeled_character_name_candidate(value: &str) -> Option<String> {
    if let Some(candidate) = usable_cjk_character_name_candidate(value) {
        return Some(candidate);
    }
    let candidate = value
        .trim()
        .trim_matches(|ch| matches!(ch, '《' | '》' | '"' | '\'' | '“' | '”'));
    let len = candidate.chars().count();
    if !(1..=80).contains(&len)
        || !candidate.chars().any(char::is_alphabetic)
        || !candidate.chars().all(|ch| {
            ch.is_alphanumeric() || ch.is_whitespace() || matches!(ch, '-' | '\'' | '.' | '·')
        })
    {
        return None;
    }
    Some(candidate.to_string())
}

fn usable_cjk_character_name_candidate(value: &str) -> Option<String> {
    let candidate = value
        .trim()
        .trim_matches(|ch| matches!(ch, '《' | '》' | '"' | '\'' | '“' | '”'))
        .split(['。', '.', '（', '(', ' '])
        .next()
        .unwrap_or(value)
        .trim()
        .to_string();
    let len = candidate.chars().count();
    if (2..=6).contains(&len) && candidate.chars().any(is_cjk_unified) {
        Some(candidate)
    } else {
        None
    }
}

pub(crate) fn extract_cjk_character_name_near_role_signal(line: &str) -> Option<String> {
    let role_markers = [
        "主角",
        "主人公",
        "男主角",
        "女主角",
        "男主",
        "女主",
        "反派",
        "对手",
        "同伴",
        "导师",
        "姓名",
        "名字",
        "名称",
        "canonical_name",
        "name",
    ];
    let start = role_markers
        .iter()
        .filter_map(|marker| line.find(marker).map(|idx| idx + marker.len()))
        .min()
        .unwrap_or(0);
    let slice = &line[start..];
    let mut current = String::new();
    let mut candidates = Vec::new();
    for ch in slice.chars() {
        if is_cjk_unified(ch) {
            current.push(ch);
            continue;
        }
        push_character_name_candidate(&mut candidates, &mut current);
    }
    push_character_name_candidate(&mut candidates, &mut current);
    candidates
        .into_iter()
        .find(|candidate| !fiction_character_name_candidate_is_noise(candidate))
}

pub(crate) fn push_character_name_candidate(candidates: &mut Vec<String>, current: &mut String) {
    let len = current.chars().count();
    if (2..=6).contains(&len) {
        candidates.push(current.clone());
    }
    current.clear();
}

pub(crate) fn fiction_character_name_candidate_is_noise(candidate: &str) -> bool {
    matches!(
        candidate,
        "主角"
            | "主人公"
            | "男主"
            | "女主"
            | "男主角"
            | "女主角"
            | "反派"
            | "对手"
            | "同伴"
            | "导师"
            | "姓名"
            | "名字"
            | "名称"
            | "角色"
            | "人物"
            | "欲望"
            | "恐惧"
            | "底线"
            | "世界观"
            | "核心矛盾"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        contract_text_contains_section_heading_residue, generated_fiction_outline,
        strip_contract_section_heading_residue,
    };

    #[test]
    fn bare_contract_section_headings_do_not_enter_creative_outline_fields() {
        let outline = generated_fiction_outline(
            "全书大纲：陆承言发现墙内爆炸计划。\n\
分卷规划\n\
第1卷《墙影》：本卷目标：取得第一份证据；卷尾变化：监视名单被公开\n\
近期章节包\n\
第1章 本章目标：核对名单；预期转折：名单指向旧哨塔",
        );

        assert!(!outline.contains("分卷规划"), "{outline}");
        assert!(!outline.contains("近期章节包"), "{outline}");
        assert!(outline.contains("第1章 本章目标"), "{outline}");
    }

    #[test]
    fn section_heading_residue_cleanup_is_boundary_aware() {
        assert_eq!(
            strip_contract_section_heading_residue(
                "主角公开监视名单。分卷规划\n近期章节包\n主角重新设计分卷规划"
            ),
            "主角公开监视名单。\n主角重新设计分卷规划"
        );
        assert!(contract_text_contains_section_heading_residue(
            "主角公开监视名单。近期章节包"
        ));
        assert!(!contract_text_contains_section_heading_residue(
            "主角重新设计分卷规划"
        ));
        assert_eq!(
            strip_contract_section_heading_residue("对手突破防线，灯塔近在咫尺近期章节包"),
            "对手突破防线，灯塔近在咫尺"
        );
        assert!(contract_text_contains_section_heading_residue(
            "对手突破防线，灯塔近在咫尺近期章节包"
        ));
    }
}
