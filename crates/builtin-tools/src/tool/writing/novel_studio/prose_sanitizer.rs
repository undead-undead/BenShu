use super::super::surface_sanitizer;
pub(super) use super::super::surface_sanitizer::{
    is_cjk_noise_boundary as is_chinese_noise_boundary, line_is_standalone_markup_residue,
    strip_short_escape_residue_near_cjk_line as strip_short_escape_residue_near_chinese_line,
};
use super::super::text_sanitizer;
use super::{
    is_chinese_language, is_cjk_or_chinese_text_compatible, is_cjk_unified,
    is_unexpected_script_for_chinese, line_looks_like_artifact_receipt_surface,
    line_looks_like_json_field_surface, looks_like_preserved_ascii_acronym, NovelProjectManifest,
};

pub(super) fn sanitize_saved_prose(content: &str) -> String {
    sanitize_saved_prose_report(content).text
}

pub(super) fn sanitize_saved_prose_report(content: &str) -> text_sanitizer::SanitizeReport {
    let provider_report = text_sanitizer::sanitize_common_surface_report(
        content,
        text_sanitizer::WritingSanitizeStage::SavedProse,
    );
    let mut removed_lines = 0usize;
    let provider_text = surface_sanitizer::strip_json_string_line_wrappers(&provider_report.text);
    let cleaned = provider_text
        .lines()
        .filter_map(|line| {
            let trim = line.trim();
            if line_looks_like_artifact_receipt_surface(trim)
                || line_looks_like_json_field_surface(trim)
            {
                removed_lines += 1;
                None
            } else {
                Some(line.trim_end())
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cleaned = surface_sanitizer::collapse_adjacent_repeated_cjk_phrases(
        &surface_sanitizer::strip_inline_cjk_markup_noise(&cleaned),
    )
    .trim()
    .to_string();
    text_sanitizer::SanitizeReport::from_text(content, cleaned)
        .merge(provider_report)
        .with_removed_lines(removed_lines)
}

#[allow(dead_code)]
pub(super) fn sanitize_chinese_script_noise_report(
    manifest: &NovelProjectManifest,
    content: &str,
) -> text_sanitizer::SanitizeReport {
    let cleaned = sanitize_chinese_script_noise(manifest, content);
    let note = if is_chinese_language(&manifest.language) {
        "chinese_script_noise"
    } else {
        "script_noise_skipped_non_chinese"
    };
    text_sanitizer::SanitizeReport::from_text(content, cleaned).note(note)
}

pub(super) fn sanitize_chinese_script_noise(
    manifest: &NovelProjectManifest,
    content: &str,
) -> String {
    if !is_chinese_language(&manifest.language) {
        return content.to_string();
    }
    let content = normalize_chinese_surface_punctuation(content);
    let content = surface_sanitizer::strip_inline_cjk_markup_noise(&content);
    let content = strip_embedded_structured_field_residue_from_chinese_prose(&content);
    let content = collapse_excessive_repeated_cjk_chars(&content);
    let content = strip_adjacent_foreign_alpha_runs_from_chinese_text(&content);
    let content = strip_spurious_escape_markers_near_chinese_text(&content);
    let content = strip_short_escape_residue_near_chinese_text(&content);
    let content = strip_isolated_unexpected_scripts_from_chinese_text(&content);
    let content = strip_remaining_unexpected_scripts_from_chinese_text(&content);
    let content = collapse_unexpected_cjk_internal_whitespace(&content);
    strip_chinese_markup_residue_lines(&content)
}

pub(super) fn normalize_chinese_surface_punctuation(content: &str) -> String {
    let mut cleaned = content.replace('`', "");
    cleaned = normalize_unbalanced_quote_pair(cleaned, '‘', '’');
    cleaned = normalize_unbalanced_quote_pair(cleaned, '“', '”');
    cleaned
}

pub(super) fn strip_embedded_structured_field_residue_from_chinese_prose(content: &str) -> String {
    let markers = [
        "\n\"summary\"",
        "\n    \"summary\"",
        "\n  \"summary\"",
        "\n\"key_facts\"",
        "\n    \"key_facts\"",
        "\n\"continuity_updates\"",
        "\n    \"continuity_updates\"",
        "\nsummary:",
        "\nkey_facts:",
        "\ncontinuity_updates:",
    ];
    let Some(index) = markers
        .iter()
        .filter_map(|marker| content.find(marker))
        .min()
    else {
        return content.to_string();
    };
    if content[..index]
        .chars()
        .filter(|ch| is_cjk_unified(*ch))
        .count()
        < 120
    {
        return content.to_string();
    }
    content[..index].trim_end().to_string()
}

pub(super) fn collapse_excessive_repeated_cjk_chars(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut prev: Option<char> = None;
    let mut run_len = 0usize;
    for ch in content.chars() {
        if Some(ch) == prev {
            run_len += 1;
        } else {
            prev = Some(ch);
            run_len = 1;
        }
        if is_cjk_unified(ch) && run_len > 3 {
            continue;
        }
        out.push(ch);
    }
    out
}

fn normalize_unbalanced_quote_pair(content: String, open: char, close: char) -> String {
    let open_count = content.chars().filter(|ch| *ch == open).count();
    let close_count = content.chars().filter(|ch| *ch == close).count();
    if open_count == close_count {
        return content;
    }
    let mut remaining_closes = close_count;
    let mut pending_opens = 0usize;
    let mut out = String::with_capacity(content.len());
    for ch in content.chars() {
        if ch == open {
            if remaining_closes > pending_opens {
                pending_opens += 1;
                out.push(ch);
            }
            continue;
        }
        if ch == close {
            remaining_closes = remaining_closes.saturating_sub(1);
            if pending_opens > 0 {
                pending_opens -= 1;
                out.push(ch);
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn collapse_unexpected_cjk_internal_whitespace(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if !ch.is_whitespace() || ch == '\n' || ch == '\r' {
            out.push(ch);
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len()
            && chars[index].is_whitespace()
            && chars[index] != '\n'
            && chars[index] != '\r'
        {
            index += 1;
        }
        let prev = previous_non_whitespace_char(&chars, start);
        let next = next_non_whitespace_char(&chars, index);
        if prev.is_some_and(is_cjk_unified) && next.is_some_and(is_cjk_unified) {
            continue;
        }
        out.extend(chars[start..index].iter());
    }
    out
}

pub(super) fn strip_chinese_markup_residue_lines(content: &str) -> String {
    surface_sanitizer::strip_cjk_markup_residue_lines(content)
}

pub(super) fn strip_adjacent_foreign_alpha_runs_from_chinese_text(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if !is_foreign_alpha_for_chinese_text(ch) {
            out.push(ch);
            index += 1;
            continue;
        }

        let start = index;
        while index < chars.len() && is_foreign_alpha_for_chinese_text(chars[index]) {
            index += 1;
        }
        let run = chars[start..index].iter().collect::<String>();
        let prev = previous_non_whitespace_char(&chars, start);
        let next = next_non_whitespace_char(&chars, index);
        if is_chinese_foreign_noise_context(prev, next) && !looks_like_preserved_ascii_acronym(&run)
        {
            continue;
        }
        out.push_str(&run);
    }
    out
}

fn strip_spurious_escape_markers_near_chinese_text(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = String::new();
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '\\' {
            let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
            let next = chars.get(index + 1).copied();
            if prev.is_none_or(is_chinese_noise_boundary)
                && next.is_some_and(|next| is_cjk_unified(next) || is_chinese_noise_boundary(next))
            {
                continue;
            }
        }
        out.push(*ch);
    }
    out
}

fn strip_short_escape_residue_near_chinese_text(content: &str) -> String {
    surface_sanitizer::strip_short_escape_residue_near_cjk(content)
}

pub(super) fn strip_isolated_unexpected_scripts_from_chinese_text(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if !is_unexpected_script_noise_for_chinese(ch) {
            out.push(ch);
            index += 1;
            continue;
        }

        let start = index;
        while index < chars.len() && is_unexpected_script_noise_for_chinese(chars[index]) {
            index += 1;
        }
        let prev = previous_non_whitespace_char(&chars, start);
        let next = next_non_whitespace_char(&chars, index);
        if is_chinese_foreign_noise_context(prev, next) {
            continue;
        }
        out.extend(chars[start..index].iter());
    }
    out
}

fn strip_remaining_unexpected_scripts_from_chinese_text(content: &str) -> String {
    content
        .chars()
        .filter(|ch| !is_unexpected_script_for_chinese(*ch))
        .collect()
}

fn previous_non_whitespace_char(chars: &[char], before: usize) -> Option<char> {
    chars
        .get(..before)?
        .iter()
        .rev()
        .copied()
        .find(|ch| !ch.is_whitespace())
}

fn next_non_whitespace_char(chars: &[char], from: usize) -> Option<char> {
    chars
        .get(from..)?
        .iter()
        .copied()
        .find(|ch| !ch.is_whitespace())
}

fn is_unexpected_script_noise_for_chinese(ch: char) -> bool {
    if ch.is_ascii() || is_cjk_or_chinese_text_compatible(ch) {
        return false;
    }
    ch.is_alphabetic() || is_unicode_mark(ch)
}

fn is_unicode_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036f
            | 0x0591..=0x05bd
            | 0x05bf
            | 0x05c1..=0x05c2
            | 0x05c4..=0x05c5
            | 0x05c7
            | 0x0610..=0x061a
            | 0x064b..=0x065f
            | 0x0670
            | 0x06d6..=0x06dc
            | 0x06df..=0x06e4
            | 0x06e7..=0x06e8
            | 0x06ea..=0x06ed
            | 0x0711
            | 0x0730..=0x074a
            | 0x07a6..=0x07b0
            | 0x07eb..=0x07f3
            | 0x0816..=0x0819
            | 0x081b..=0x0823
            | 0x0825..=0x0827
            | 0x0829..=0x082d
            | 0x0859..=0x085b
            | 0x08d3..=0x08e1
            | 0x08e3..=0x0903
            | 0x093a..=0x093c
            | 0x0941..=0x0948
            | 0x094d
            | 0x0951..=0x0957
            | 0x0962..=0x0963
            | 0x0981
            | 0x09bc
            | 0x09c1..=0x09c4
            | 0x09cd
            | 0x09e2..=0x09e3
            | 0x0a01..=0x0a02
            | 0x0a3c
            | 0x0a41..=0x0a42
            | 0x0a47..=0x0a48
            | 0x0a4b..=0x0a4d
            | 0x0a51
            | 0x0a70..=0x0a71
            | 0x0a75
            | 0x0a81..=0x0a82
            | 0x0abc
            | 0x0ac1..=0x0ac5
            | 0x0ac7..=0x0ac8
            | 0x0acd
            | 0x0ae2..=0x0ae3
            | 0x0b01
            | 0x0b3c
            | 0x0b3f
            | 0x0b41..=0x0b44
            | 0x0b4d
            | 0x0b56
            | 0x0b62..=0x0b63
            | 0x0b82
            | 0x0bc0
            | 0x0bcd
            | 0x0c00
            | 0x0c04
            | 0x0c3e..=0x0c40
            | 0x0c46..=0x0c48
            | 0x0c4a..=0x0c4d
            | 0x0c55..=0x0c56
            | 0x0c62..=0x0c63
            | 0x0c81
            | 0x0cbc
            | 0x0cbf
            | 0x0cc6
            | 0x0ccc..=0x0ccd
            | 0x0ce2..=0x0ce3
            | 0x0d00..=0x0d01
            | 0x0d3b..=0x0d3c
            | 0x0d41..=0x0d44
            | 0x0d4d
            | 0x0d62..=0x0d63
            | 0x0dca
            | 0x0dd2..=0x0dd4
            | 0x0dd6
            | 0x0e31
            | 0x0e34..=0x0e3a
            | 0x0e47..=0x0e4e
            | 0x0eb1
            | 0x0eb4..=0x0ebc
            | 0x0ec8..=0x0ecd
            | 0x0f18..=0x0f19
            | 0x0f35
            | 0x0f37
            | 0x0f39
            | 0x0f71..=0x0f7e
            | 0x0f80..=0x0f84
            | 0x0f86..=0x0f87
            | 0x0f8d..=0x0f97
            | 0x0f99..=0x0fbc
            | 0x0fc6
            | 0x102d..=0x1030
            | 0x1032..=0x1037
            | 0x1039..=0x103a
            | 0x103d..=0x103e
            | 0x1058..=0x1059
            | 0x105e..=0x1060
            | 0x1071..=0x1074
            | 0x1082
            | 0x1085..=0x1086
            | 0x108d
            | 0x109d
            | 0x135d..=0x135f
            | 0x1712..=0x1714
            | 0x1732..=0x1734
            | 0x1752..=0x1753
            | 0x1772..=0x1773
            | 0x17b4..=0x17b5
            | 0x17b7..=0x17bd
            | 0x17c6
            | 0x17c9..=0x17d3
            | 0x17dd
            | 0x180b..=0x180d
            | 0x1885..=0x1886
            | 0x18a9
            | 0x1920..=0x1922
            | 0x1927..=0x1928
            | 0x1932
            | 0x1939..=0x193b
            | 0x1a17..=0x1a18
            | 0x1a1b
            | 0x1a56
            | 0x1a58..=0x1a5e
            | 0x1a60
            | 0x1a62
            | 0x1a65..=0x1a6c
            | 0x1a73..=0x1a7c
            | 0x1a7f
            | 0x1ab0..=0x1aff
            | 0x1dc0..=0x1dff
            | 0x20d0..=0x20ff
            | 0xfe20..=0xfe2f
    )
}

fn is_foreign_alpha_for_chinese_text(ch: char) -> bool {
    ch.is_alphabetic() && !is_cjk_or_chinese_text_compatible(ch)
}

fn is_chinese_foreign_noise_context(prev: Option<char>, next: Option<char>) -> bool {
    match (prev, next) {
        (Some(left), Some(right)) => {
            (is_cjk_unified(left) && is_chinese_noise_boundary(right))
                || (is_chinese_noise_boundary(left) && is_cjk_unified(right))
        }
        _ => false,
    }
}
