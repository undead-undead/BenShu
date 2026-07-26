pub(super) fn strip_markdown_heading(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            if line.starts_with("# ") {
                line.trim_start_matches("# ").to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn normalize_chapter_body_for_record(content: &str, title: &str) -> String {
    let normalized = normalize_literal_escaped_newlines(content);
    strip_redundant_leading_chapter_heading(&normalized, title)
}

fn normalize_literal_escaped_newlines(content: &str) -> String {
    content
        .replace("\\r\\n", "\n")
        .replace("\\n", "\n")
        .replace("\\r", "\n")
}

pub(super) fn strip_redundant_leading_chapter_heading(content: &str, title: &str) -> String {
    let mut lines = content.lines().collect::<Vec<_>>();
    while let Some(first) = lines.first().copied() {
        let trimmed = first.trim();
        if trimmed.is_empty() {
            lines.remove(0);
            continue;
        }
        if leading_line_looks_like_same_chapter_heading(trimmed, title)
            || leading_line_looks_like_generated_chapter_heading(trimmed)
            || leading_line_looks_like_metadata_title_heading(trimmed)
        {
            lines.remove(0);
            continue;
        }
        break;
    }
    lines.join("\n").trim().to_string()
}

fn leading_line_looks_like_metadata_title_heading(line: &str) -> bool {
    let Some(heading) = markdown_heading_text(line) else {
        return false;
    };
    let trimmed = heading.trim().trim_matches(['"', '\'', '“', '”']);
    !trimmed.is_empty() && line.trim_start().starts_with("# ") && trimmed.chars().count() <= 40
}

pub(super) fn leading_line_looks_like_same_chapter_heading(line: &str, title: &str) -> bool {
    if let Some(heading) = markdown_heading_text(line) {
        return heading_looks_like_same_chapter_heading(heading, title);
    }
    plain_line_looks_like_same_chapter_heading(line, title)
}

fn plain_line_looks_like_same_chapter_heading(line: &str, title: &str) -> bool {
    let trimmed = line.trim().trim_matches(['"', '\'', '“', '”']);
    if trimmed.chars().count() > 80 {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    let title_lowered = title.to_ascii_lowercase();
    let has_chapter_marker = trimmed.contains('章')
        || lowered.contains("chapter")
        || title.contains('章')
        || title_lowered.contains("chapter");
    has_chapter_marker && heading_looks_like_same_chapter_heading(trimmed, title)
}

fn leading_line_looks_like_generated_chapter_heading(line: &str) -> bool {
    let Some(heading) = markdown_heading_text(line) else {
        return false;
    };
    let trimmed = heading.trim().trim_matches(['"', '\'', '“', '”']);
    if trimmed.chars().count() > 80 {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    trimmed.starts_with('第')
        || lowered.starts_with("chapter ")
        || lowered.starts_with("chapter:")
        || chapter_ordinal_token(trimmed).is_some()
}

pub(super) fn markdown_heading_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = trimmed.get(hashes..)?.trim_start();
    (!rest.is_empty()).then_some(rest.trim())
}

fn heading_looks_like_same_chapter_heading(heading: &str, title: &str) -> bool {
    let normalized_heading = normalize_heading_text(heading);
    let normalized_title = normalize_heading_text(title);
    if normalized_heading.is_empty() || normalized_title.is_empty() {
        return false;
    }
    if normalized_heading == normalized_title {
        return true;
    }
    match (
        chapter_ordinal_token(&normalized_heading),
        chapter_ordinal_token(&normalized_title),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn chapter_ordinal_token(value: &str) -> Option<String> {
    let chars = value.chars().collect::<Vec<_>>();
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '第' {
            continue;
        }
        let mut end = index + 1;
        while end < chars.len() {
            let current = chars[end];
            if current == '章' || current == '回' || current == '节' {
                if end > index + 1 {
                    return Some(chars[index..=end].iter().collect());
                }
                break;
            }
            if !(current.is_ascii_digit() || is_cjk_numeral(current)) {
                break;
            }
            end += 1;
        }
    }
    None
}

fn is_cjk_numeral(ch: char) -> bool {
    matches!(
        ch,
        '零' | '〇'
            | '一'
            | '二'
            | '两'
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
    )
}

fn normalize_heading_text(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '#' | ' ' | '\t' | ':' | '：' | '-' | '—' | '《' | '》' | '"' | '\''
            )
        })
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}
