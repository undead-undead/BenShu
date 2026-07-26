use super::*;

pub(in crate::tool::writing::novel_studio) fn pre_sanitized_content_issues(
    manifest: &NovelProjectManifest,
    content: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    issues.extend(pre_sanitized_surface_contamination_issues(content));
    issues.extend(placeholder_or_omission_issues(content));
    if is_chinese_language(&manifest.language) && contains_separated_unexpected_script(content) {
        issues.push("Chinese chapter body contains unexpected non-CJK script segment".to_string());
    }
    if is_chinese_language(&manifest.language) {
        if content.contains("\\n") {
            issues.push(
                "Chinese-language chapter contains literal escaped newline marker: \\n".to_string(),
            );
        } else if let Some(fragment) = literal_newline_escape_residue_in_cjk(content) {
            issues.push(format!(
                "Chinese-language chapter contains likely escaped newline residue: {fragment}"
            ));
        }
    }
    issues.sort();
    issues.dedup();
    issues
}

pub(in crate::tool::writing::novel_studio) fn pre_sanitized_surface_contamination_issues(
    content: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    if contains_provider_protocol_marker(content) {
        issues.push("chapter body contains provider/internal protocol marker".to_string());
    }
    if contains_generation_meta_disclaimer(content) {
        issues.push("chapter body contains model/output-limit meta commentary".to_string());
    }
    if content
        .lines()
        .any(crate::tool::writing::surface_sanitizer::line_looks_like_story_planning_meta)
    {
        issues.push("chapter body contains story-planning meta commentary".to_string());
    }
    if content
        .lines()
        .any(|line| line_looks_like_artifact_receipt_surface(line.trim()))
    {
        issues.push("chapter body contains artifact receipt/progress surface text".to_string());
    }
    if content.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("```") || trimmed.starts_with("~~~")
    }) {
        issues.push("chapter body contains code fence/control block marker".to_string());
    }
    if content
        .lines()
        .any(|line| line_looks_like_embedded_model_chapter_heading(line.trim()))
    {
        issues.push(
            "chapter body contains embedded model-generated chapter heading residue".to_string(),
        );
    }
    if content.lines().any(line_looks_like_json_field_surface) {
        issues.push("chapter body contains JSON field/control surface residue".to_string());
    }
    if contains_markup_math_residue(content) {
        issues.push("chapter body contains markup/math residue in prose".to_string());
    }
    issues
}

pub(in crate::tool::writing::novel_studio) fn pre_sanitized_issue_survives_cleanup(
    manifest: &NovelProjectManifest,
    issue: &str,
    cleaned_content: &str,
) -> bool {
    if issue.contains("JSON field/control surface") {
        return cleaned_content
            .lines()
            .any(line_looks_like_json_field_surface);
    }
    if issue.contains("code fence/control block") {
        return cleaned_content.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("```") || trimmed.starts_with("~~~")
        });
    }
    if issue.contains("provider/internal protocol marker") {
        return contains_provider_protocol_marker(cleaned_content);
    }
    if issue.contains("model/output-limit meta commentary") {
        return contains_generation_meta_disclaimer(cleaned_content);
    }
    if issue.contains("story-planning meta commentary") {
        return cleaned_content
            .lines()
            .any(crate::tool::writing::surface_sanitizer::line_looks_like_story_planning_meta);
    }
    if issue.contains("artifact receipt/progress surface") {
        return cleaned_content
            .lines()
            .any(|line| line_looks_like_artifact_receipt_surface(line.trim()));
    }
    if issue.contains("markup/math residue") {
        return contains_markup_math_residue(cleaned_content);
    }
    if issue.contains("literal escaped newline marker") {
        return cleaned_content.contains("\\n");
    }
    if issue.contains("escaped newline residue") {
        return true;
    }
    if issue.contains("unexpected non-CJK script segment")
        && is_chinese_language(&manifest.language)
    {
        return true;
    }
    true
}

pub(in crate::tool::writing::novel_studio) fn contains_separated_unexpected_script(
    content: &str,
) -> bool {
    content.split_whitespace().any(|token| {
        let has_unexpected = token.chars().any(|ch| {
            matches!(
                ch as u32,
                0x0370..=0x03ff | 0x0400..=0x052f | 0x0590..=0x05ff | 0x0600..=0x06ff
            )
        });
        let has_cjk = token.chars().any(is_cjk_unified);
        has_unexpected && !has_cjk
    })
}

pub(in crate::tool::writing::novel_studio) fn narrative_substance_issues(
    manifest: &NovelProjectManifest,
    content: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    let line_count = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let control_like_lines = content
        .lines()
        .filter(|line| line_looks_like_outline_or_analysis(line.trim()))
        .count();
    if line_count >= 8 && control_like_lines >= 4 && control_like_lines * 2 >= line_count {
        issues.push(
            "chapter body looks like outline/analysis prose instead of finished scenes".to_string(),
        );
    }
    if is_chinese_language(&manifest.language) {
        let cjk_chars = content.chars().filter(|ch| is_cjk_unified(*ch)).count();
        let scene_punctuation = content
            .chars()
            .filter(|ch| matches!(ch, '。' | '！' | '？' | '”' | '」'))
            .count();
        if cjk_chars >= 1200 && scene_punctuation < 12 {
            issues.push(
                "Chinese chapter body has too little sentence/dialogue punctuation for finished prose"
                    .to_string(),
            );
        }
        if let Some(fragment) = repeated_cjk_scene_fragment(content) {
            issues.push(format!(
                "Chinese chapter body repeats the same scene fragment too many times: {fragment}"
            ));
        }
        if let Some(fragment) = repeated_cjk_scene_block(content) {
            issues.push(format!(
                "Chinese chapter body repeats the same scene block instead of advancing the chapter: {fragment}"
            ));
        }
        if let Some(fragment) = repeated_cjk_paragraph_opening(content) {
            issues.push(format!(
                "Chinese chapter body repeats the same paragraph opening instead of advancing the scene: {fragment}"
            ));
        }
        if let Some(term) = overused_cjk_story_term(manifest, content) {
            issues.push(format!(
                "Chinese chapter body overuses the same story term without enough concrete progression: {term}"
            ));
        }
        if let Some(term) = overused_cjk_rhetorical_marker(content) {
            issues.push(format!(
                "Chinese chapter body overuses the same rhetorical marker instead of varying prose movement: {term}"
            ));
        }
        if let Some(term) = overused_cjk_named_concept(manifest, content) {
            issues.push(format!(
                "Chinese chapter body overuses the same named concept without enough concrete scene progression: {term}"
            ));
        }
    }
    issues
}

pub(in crate::tool::writing::novel_studio) fn repeated_cjk_paragraph_opening(
    content: &str,
) -> Option<String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if line.starts_with('#') {
            continue;
        }
        let opening = line
            .chars()
            .filter(|ch| is_cjk_unified(*ch))
            .take(18)
            .collect::<String>();
        let len = opening.chars().count();
        if !(10..=18).contains(&len) {
            continue;
        }
        *counts.entry(opening).or_default() += 1;
    }
    counts
        .into_iter()
        .find(|(opening, count)| {
            let required = if opening.chars().count() >= 18 { 2 } else { 3 };
            *count >= required
        })
        .map(|(opening, count)| {
            format!("`{}` repeated {} times", preview_chars(&opening, 60), count)
        })
}

fn overused_cjk_story_term(manifest: &NovelProjectManifest, content: &str) -> Option<String> {
    let cjk_chars = content.chars().filter(|ch| is_cjk_unified(*ch)).count();
    if cjk_chars < 1600 {
        return None;
    }
    let authority = contract_term_authority_view(manifest);
    let meaningful_sentences = content
        .split(['。', '！', '？', '\n'])
        .map(str::trim)
        .filter(|part| part.chars().filter(|ch| is_cjk_unified(*ch)).count() >= 12)
        .count()
        .max(1);
    let compact = content
        .chars()
        .filter(|ch| is_cjk_unified(*ch))
        .collect::<String>();
    let chars = compact.chars().collect::<Vec<_>>();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for window in chars.windows(4) {
        let term = window.iter().collect::<String>();
        if generic_cjk_story_ngram(&term)
            || term_overlaps_authority_character_name(&term, &authority.character_names)
            || authority.is_non_character_term(&term)
        {
            continue;
        }
        *counts.entry(term).or_default() += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count >= 8 && count.saturating_mul(100) / meaningful_sentences >= 15)
        .max_by_key(|(_, count)| *count)
        .map(|(term, count)| format!("`{}` appears {} times", term, count))
}

fn term_overlaps_authority_character_name(term: &str, character_names: &BTreeSet<String>) -> bool {
    character_names.iter().any(|name| {
        let name_len = name.chars().count();
        if name_len < 2 {
            return false;
        }
        if name.contains(term) || term.contains(name) {
            return true;
        }
        let min_overlap = name_len.min(3);
        cjk_substrings(name, min_overlap).any(|part| term.contains(&part))
    })
}

fn cjk_substrings(value: &str, len: usize) -> impl Iterator<Item = String> + '_ {
    let chars = value.chars().collect::<Vec<_>>();
    let end = chars.len().saturating_sub(len);
    (0..=end).filter_map(move |start| {
        (start + len <= chars.len()).then(|| chars[start..start + len].iter().collect())
    })
}

fn overused_cjk_rhetorical_marker(content: &str) -> Option<String> {
    let sentence_count = content
        .split(['。', '！', '？', '\n'])
        .filter(|part| part.chars().filter(|ch| is_cjk_unified(*ch)).count() >= 10)
        .count()
        .max(1);
    ["仿佛", "似乎", "好像", "宛如", "像是"]
        .into_iter()
        .filter_map(|term| {
            let count = content.matches(term).count();
            (count >= 16 && count.saturating_mul(100) / sentence_count >= 16)
                .then_some((term, count))
        })
        .max_by_key(|(_, count)| *count)
        .map(|(term, count)| format!("`{term}` appears {count} times"))
}

pub(in crate::tool::writing::novel_studio) fn overused_cjk_named_concept(
    manifest: &NovelProjectManifest,
    content: &str,
) -> Option<String> {
    let cjk_chars = content.chars().filter(|ch| is_cjk_unified(*ch)).count();
    if cjk_chars < 1800 {
        return None;
    }
    let sentence_count = content
        .split(['。', '！', '？', '\n'])
        .filter(|part| part.chars().filter(|ch| is_cjk_unified(*ch)).count() >= 10)
        .count()
        .max(1);
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    collect_quoted_cjk_terms(content, &mut counts);
    collect_frequent_cjk_concept_terms(content, &mut counts);
    collect_frequent_cjk_surface_terms(content, manifest, &mut counts);
    counts
        .into_iter()
        .filter(|(term, count)| {
            let len = term.chars().count();
            (1..=6).contains(&len)
                && *count >= 18
                && count.saturating_mul(100) / sentence_count >= 35
                && !generic_named_concept_term(term)
        })
        .max_by_key(|(_, count)| *count)
        .map(|(term, count)| format!("`{}` appears {} times", term, count))
}

fn collect_quoted_cjk_terms(content: &str, counts: &mut std::collections::BTreeMap<String, usize>) {
    let mut current = String::new();
    let mut in_quote = false;
    for ch in content.chars() {
        match ch {
            '‘' | '“' | '《' => {
                current.clear();
                in_quote = true;
            }
            '’' | '”' | '》' if in_quote => {
                let term = current
                    .chars()
                    .filter(|ch| is_cjk_unified(*ch))
                    .collect::<String>();
                if (1..=6).contains(&term.chars().count()) {
                    *counts.entry(term).or_default() += 1;
                }
                current.clear();
                in_quote = false;
            }
            _ if in_quote => current.push(ch),
            _ => {}
        }
    }
}

fn collect_frequent_cjk_surface_terms(
    content: &str,
    manifest: &NovelProjectManifest,
    counts: &mut std::collections::BTreeMap<String, usize>,
) {
    let compact = content
        .chars()
        .filter(|ch| is_cjk_unified(*ch))
        .collect::<String>();
    let chars = compact.chars().collect::<Vec<_>>();
    let authority = contract_term_authority_view(manifest);
    for window in 2..=3 {
        if chars.len() < window {
            continue;
        }
        for start in 0..=chars.len() - window {
            let term = chars[start..start + window].iter().collect::<String>();
            if generic_cjk_surface_term(&term, &authority.character_names) {
                continue;
            }
            *counts.entry(term).or_default() += 1;
        }
    }
}

fn generic_cjk_surface_term(term: &str, character_names: &BTreeSet<String>) -> bool {
    if cjk_story_ngram_has_structural_edge(term) {
        return true;
    }
    if character_names
        .iter()
        .any(|name| name.contains(term) || term.contains(name))
    {
        return true;
    }
    matches!(
        term,
        "然而"
            | "但是"
            | "只是"
            | "已经"
            | "无法"
            | "自己"
            | "这种"
            | "这个"
            | "那个"
            | "一个"
            | "一种"
            | "不是"
            | "没有"
            | "什么"
            | "可以"
            | "不能"
            | "必须"
            | "开始"
            | "继续"
            | "终于"
            | "突然"
            | "再次"
            | "仍然"
    )
}

fn collect_frequent_cjk_concept_terms(
    content: &str,
    counts: &mut std::collections::BTreeMap<String, usize>,
) {
    for term in ["法则", "规则", "力量", "真相", "本质", "代价", "世界"] {
        let count = content.matches(term).count();
        if count > 0 {
            *counts.entry(term.to_string()).or_default() += count;
        }
    }
}

fn generic_named_concept_term(term: &str) -> bool {
    matches!(term, "什么" | "为何" | "怎么" | "他说" | "她说" | "你说")
}

fn generic_cjk_story_ngram(term: &str) -> bool {
    if cjk_story_ngram_has_structural_edge(term) {
        return true;
    }
    [
        "就在这",
        "的时候",
        "这一刻",
        "他知道",
        "没有理",
        "然而就",
        "仿佛有",
        "这不可能",
        "他的心",
    ]
    .iter()
    .any(|generic| term.contains(generic))
}

fn cjk_story_ngram_has_structural_edge(term: &str) -> bool {
    let chars = term.chars().collect::<Vec<_>>();
    let Some(first) = chars.first().copied() else {
        return true;
    };
    let Some(last) = chars.last().copied() else {
        return true;
    };
    let structural = |ch| {
        matches!(
            ch,
            '的' | '了'
                | '着'
                | '过'
                | '在'
                | '与'
                | '和'
                | '把'
                | '被'
                | '将'
                | '就'
                | '都'
                | '也'
                | '却'
                | '而'
                | '并'
                | '向'
                | '从'
                | '对'
                | '给'
                | '到'
                | '为'
        )
    };
    structural(first) || structural(last)
}

fn repeated_cjk_scene_fragment(content: &str) -> Option<String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for sentence in content.split(['。', '！', '？', '\n']) {
        let normalized = normalize_repetition_sentence(sentence);
        let len = normalized.chars().count();
        if !(18..=120).contains(&len) {
            continue;
        }
        let count = counts.entry(normalized).or_default();
        *count += 1;
    }
    counts
        .into_iter()
        .find(|(_, count)| *count >= 3)
        .map(|(fragment, count)| {
            format!(
                "`{}` repeated {} times",
                preview_chars(&fragment, 80),
                count
            )
        })
}

fn repeated_cjk_scene_block(content: &str) -> Option<String> {
    let sentences = content
        .split(['。', '！', '？', '\n'])
        .map(normalize_repetition_sentence)
        .filter(|sentence| sentence.chars().count() >= 12)
        .collect::<Vec<_>>();
    if sentences.len() < 6 {
        return None;
    }
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for width in (3usize..=5).rev() {
        if sentences.len() < width * 2 {
            continue;
        }
        counts.clear();
        for window in sentences.windows(width) {
            let joined = window.join("。");
            if joined.chars().count() < 90 {
                continue;
            }
            *counts.entry(joined).or_default() += 1;
        }
        if let Some((fragment, count)) = counts.iter().find(|(_, count)| **count >= 2) {
            return Some(format!(
                "`{}` repeated {} times",
                preview_chars(fragment, 100),
                count
            ));
        }
    }
    None
}

fn normalize_repetition_sentence(value: &str) -> String {
    value
        .chars()
        .filter(|ch| is_cjk_unified(*ch) || matches!(ch, '“' | '”' | '《' | '》'))
        .collect::<String>()
}

pub(in crate::tool::writing::novel_studio) fn line_looks_like_outline_or_analysis(
    line: &str,
) -> bool {
    if line.is_empty() {
        return false;
    }
    let lowered = line.to_ascii_lowercase();
    let has_label = [
        "本章目标",
        "章节目标",
        "剧情推进",
        "人物弧",
        "伏笔",
        "世界观",
        "设定",
        "总结",
        "分析",
        "场景功能",
        "人物行动",
        "状态变化",
        "chapter goal",
        "plot progression",
        "character arc",
        "hook",
        "worldbuilding",
        "analysis",
        "summary",
        "scene function",
        "state change",
    ]
    .iter()
    .any(|marker| line.contains(marker) || lowered.contains(marker));
    has_label && (line.contains(':') || line.contains('：') || line.starts_with('-'))
}

pub(in crate::tool::writing::novel_studio) fn prose_surface_contamination_issues(
    content: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    if contains_provider_protocol_marker(content) {
        issues.push("chapter body contains provider/internal protocol marker".to_string());
    }
    if contains_generation_meta_disclaimer(content) {
        issues.push("chapter body contains model/output-limit meta commentary".to_string());
    }
    if content
        .lines()
        .any(crate::tool::writing::surface_sanitizer::line_looks_like_story_planning_meta)
    {
        issues.push("chapter body contains story-planning meta commentary".to_string());
    }
    if content
        .lines()
        .any(|line| line_looks_like_artifact_receipt_surface(line.trim()))
    {
        issues.push("chapter body contains artifact receipt/progress surface text".to_string());
    }
    if content.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("```") || trimmed.starts_with("~~~")
    }) {
        issues.push("chapter body contains code fence/control block marker".to_string());
    }
    if content
        .lines()
        .any(|line| line_looks_like_embedded_model_chapter_heading(line.trim()))
    {
        issues.push(
            "chapter body contains embedded model-generated chapter heading residue".to_string(),
        );
    }
    if content.lines().any(line_looks_like_json_field_surface) {
        issues.push("chapter body contains JSON field/control surface residue".to_string());
    }
    if contains_markup_math_residue(content) {
        issues.push("chapter body contains markup/math residue in prose".to_string());
    }
    if let Some(reason) = high_confidence_cjk_text_noise_issue(content) {
        issues.push(format!(
            "chapter body contains likely malformed CJK prose: {reason}"
        ));
    }
    issues.extend(cjk_malformed_phrase_issues(content));
    issues.extend(prose_ending_completeness_issues(content));
    issues
}

fn line_looks_like_embedded_model_chapter_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(after_hash) = trimmed.strip_prefix('#') else {
        return false;
    };
    let after_hash = after_hash.trim_start();
    let Some(after_di) = after_hash.strip_prefix('第') else {
        return false;
    };
    let mut saw_number = false;
    for ch in after_di.chars() {
        if ch.is_ascii_digit()
            || matches!(
                ch,
                '零' | '〇'
                    | '一'
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
                    | '两'
            )
        {
            saw_number = true;
            continue;
        }
        return saw_number && matches!(ch, '章' | '回' | '节');
    }
    false
}

pub(in crate::tool::writing::novel_studio) fn cjk_malformed_phrase_issues(
    content: &str,
) -> Vec<String> {
    if !content.chars().any(is_cjk_unified) {
        return Vec::new();
    }
    let mut issues = Vec::new();
    for marker in [
        "什的",
        "什地",
        "什都",
        "什东西",
        "正静地",
        "正静的",
        "悄蔓延",
        "悄扩散",
        "悄靠近",
        "突直跳",
        "喃自语",
        "地回头",
        "地甩头",
        "为什",
    ] {
        if cjk_missing_fragment_marker_present(content, marker) {
            issues.push(format!(
                "chapter body contains likely missing-character fragment: {marker}"
            ));
        }
    }
    issues.extend(cjk_standalone_shen_missing_suffix_issues(content));
    issues.extend(cjk_action_object_part_boundary_issues(content));
    issues
}

fn cjk_action_object_part_boundary_issues(content: &str) -> Vec<String> {
    let repaired =
        crate::tool::writing::surface_sanitizer::repair_cjk_action_object_part_boundaries(content);
    crate::tool::writing::surface_sanitizer::cjk_action_object_part_boundary_fragments(&repaired)
        .into_iter()
        .map(|fragment| {
            format!(
                "chapter body contains likely malformed CJK action-object-part boundary; missing punctuation or duplicated object near: {fragment}"
            )
        })
        .collect()
}

fn cjk_missing_fragment_marker_present(content: &str, marker: &str) -> bool {
    let chars = content.chars().collect::<Vec<_>>();
    let marker_chars = marker.chars().collect::<Vec<_>>();
    if marker_chars.is_empty() || chars.len() < marker_chars.len() {
        return false;
    }
    for index in 0..=chars.len() - marker_chars.len() {
        if chars[index..index + marker_chars.len()] != marker_chars {
            continue;
        }
        let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
        let next = chars.get(index + marker_chars.len()).copied();
        if cjk_missing_fragment_marker_is_normal_word(marker, prev, next) {
            continue;
        }
        return true;
    }
    false
}

fn cjk_missing_fragment_marker_is_normal_word(
    marker: &str,
    previous: Option<char>,
    next: Option<char>,
) -> bool {
    match marker {
        "为什" => next == Some('么'),
        "悄蔓延" | "悄扩散" | "悄靠近" => previous == Some('悄'),
        "突直跳" => previous == Some('突'),
        "喃自语" => previous == Some('喃'),
        "地回头" | "地甩头" => previous == Some('猛'),
        _ => false,
    }
}

fn cjk_standalone_shen_missing_suffix_issues(content: &str) -> Vec<String> {
    let chars = content.chars().collect::<Vec<_>>();
    let mut issues = Vec::new();
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '什' {
            continue;
        }
        let Some(next) = chars.get(index + 1).copied() else {
            continue;
        };
        if matches!(next, '么' | '錦' | '锦') || !is_cjk_unified(next) {
            continue;
        }
        let previous = index
            .checked_sub(1)
            .and_then(|prev| chars.get(prev).copied());
        if previous.is_some_and(|prev| matches!(prev, '什' | '么')) {
            continue;
        }
        let end = chars[index + 1..]
            .iter()
            .position(|candidate| !is_cjk_unified(*candidate))
            .map(|offset| index + 1 + offset)
            .unwrap_or(chars.len())
            .min(index + 5);
        if end <= index + 1 {
            continue;
        }
        let marker = chars[index..end].iter().collect::<String>();
        issues.push(format!(
            "chapter body contains likely missing-character fragment: {marker}"
        ));
    }
    chapter_quality::finalize_issues(issues)
}

pub(in crate::tool::writing::novel_studio) fn prose_ending_completeness_issues(
    content: &str,
) -> Vec<String> {
    let body = strip_markdown_heading(&strip_frontmatter(content));
    let last = body
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if last.is_empty() {
        return Vec::new();
    }

    let mut issues = Vec::new();
    if final_line_ends_with_non_terminal_punctuation(last) {
        issues.push(format!(
            "chapter body appears unfinished: final line ends with non-terminal punctuation near `{}`",
            preview_tail(last, 40)
        ));
    }
    if final_line_ends_without_terminal_punctuation(last) {
        issues.push(format!(
            "chapter body appears unfinished: final line has no terminal punctuation near `{}`",
            preview_tail(last, 40)
        ));
    }
    if final_line_has_unclosed_dialogue_quote(last) {
        issues.push(format!(
            "chapter body appears unfinished: final line has an unclosed dialogue quote near `{}`",
            preview_tail(last, 40)
        ));
    }
    if final_line_looks_like_incomplete_speech_or_transition(last) {
        issues.push(format!(
            "chapter body appears unfinished: final line stops before completing a speech or transition near `{}`",
            preview_tail(last, 40)
        ));
    }
    chapter_quality::finalize_issues(issues)
}

fn final_line_ends_with_non_terminal_punctuation(line: &str) -> bool {
    line.chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| matches!(ch, '，' | ',' | '：' | ':' | '、' | '；' | ';' | '—'))
}

fn final_line_ends_without_terminal_punctuation(line: &str) -> bool {
    let trimmed = line
        .trim()
        .trim_end_matches(['"', '\'', '”', '’', '」', '』', '）', ')', '】', ']', '》'])
        .trim_end();
    if trimmed.chars().count() < 6 || !trimmed.chars().any(is_cjk_unified) {
        return false;
    }
    trimmed
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| !matches!(ch, '。' | '！' | '？' | '!' | '?' | '.'))
}

fn final_line_has_unclosed_dialogue_quote(line: &str) -> bool {
    let open = line.matches('「').count() + line.matches('“').count();
    let close = line.matches('」').count() + line.matches('”').count();
    open > close
}

fn final_line_looks_like_incomplete_speech_or_transition(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.chars().count() < 6 {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    [
        "说道",
        "说",
        "回答",
        "问道",
        "问",
        "低声说",
        "缓缓说道",
        "开口",
        "告诉",
        "解释",
        "想到",
        "意识到",
        "明白",
        "看见",
        "听见",
        "因为",
        "但是",
        "然而",
        "于是",
        "如果",
        "when",
        "because",
        "but",
        "and then",
    ]
    .iter()
    .any(|suffix| trimmed.ends_with(suffix) || lowered.ends_with(suffix))
}

fn preview_tail(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

pub(in crate::tool::writing::novel_studio) fn high_confidence_cjk_text_noise_issue(
    content: &str,
) -> Option<String> {
    crate::tool::writing::surface_sanitizer::high_confidence_surface_issue(content)
}

pub(in crate::tool::writing::novel_studio) fn contains_markup_math_residue(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        if line_is_standalone_markup_residue(trimmed) {
            return true;
        }
        if !trimmed.chars().any(is_cjk_unified) {
            return false;
        }
        let lowered = trimmed.to_ascii_lowercase();
        lowered.contains("\\rightarrow")
            || lowered.contains("rightarrow$")
            || lowered.contains("ightarrow$")
            || lowered.starts_with("$\\rightarrow")
            || lowered.starts_with("$\\\\rightarrow")
            || trimmed.contains("$ $")
            || line_starts_with_short_escape_residue_before_chinese_text(trimmed)
            || line_contains_short_escape_residue_near_chinese_text(trimmed)
            || trimmed.starts_with("\\ l")
    })
}

pub(in crate::tool::writing::novel_studio) fn line_starts_with_short_escape_residue_before_chinese_text(
    line: &str,
) -> bool {
    let chars = line.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    while chars.get(index).is_some_and(|ch| ch.is_whitespace()) {
        index += 1;
    }
    if chars.get(index) != Some(&'\\') {
        return false;
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
    (letter_count == 0 || (1..=3).contains(&letter_count))
        && chars
            .get(index)
            .is_some_and(|ch| is_cjk_unified(*ch) || is_chinese_noise_boundary(*ch))
}

pub(in crate::tool::writing::novel_studio) fn line_contains_short_escape_residue_near_chinese_text(
    line: &str,
) -> bool {
    strip_short_escape_residue_near_chinese_line(line) != line
}

pub(in crate::tool::writing::novel_studio) fn contains_provider_protocol_marker(
    content: &str,
) -> bool {
    const MARKERS: &[&str] = &[
        "<|channel>",
        "<|channel|>",
        "<channel|>",
        "<|/channel|>",
        "<|eot_id|>",
        "<|start_header_id|>",
        "<|end_header_id|>",
        "<|im_start|>",
        "<|im_end|>",
        "<|end|>",
    ];
    MARKERS.iter().any(|marker| content.contains(marker))
}

pub(in crate::tool::writing::novel_studio) fn line_looks_like_json_field_surface(
    line: &str,
) -> bool {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with('"') || trimmed.starts_with('{') || trimmed.starts_with(',')) {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    [
        "\"add\"",
        "\"addition\"",
        "\"content\"",
        "\"summary\"",
        "\"summary_delta\"",
        "\"key_facts\"",
        "\"continuity_updates\"",
        "\"revision_notes\"",
        "\"title\"",
    ]
    .iter()
    .any(|field| lowered.contains(field))
}

pub(in crate::tool::writing::novel_studio) fn line_looks_like_artifact_receipt_surface(
    line: &str,
) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    let has_path = trimmed.contains("文件路径")
        || trimmed.contains("路径")
        || lowered.contains("artifact_path")
        || lowered.contains("project_path")
        || lowered.contains("txt_artifact_path");
    let has_receipt = trimmed.contains("修改摘要")
        || trimmed.contains("修改状态")
        || trimmed.contains("审查状态")
        || trimmed.contains("同步")
        || trimmed.contains("导出")
        || lowered.contains("runtime_effect")
        || lowered.contains("quality_gate")
        || lowered.contains("audit_status");
    let has_units = trimmed.contains("字数")
        || trimmed.contains("单位")
        || lowered.contains("unit_count")
        || lowered.contains("word count");
    let has_chapter_prefix = trimmed.starts_with("第") && trimmed.contains('章');

    (has_path && (has_receipt || has_units))
        || (has_chapter_prefix && has_units && has_receipt)
        || (has_path && lowered.contains("status:"))
}

pub(in crate::tool::writing::novel_studio) fn placeholder_or_omission_issues(
    content: &str,
) -> Vec<String> {
    let markers = [
        "此处省略",
        "此处应为",
        "省略后续",
        "以下省略",
        "略去",
        "待补充",
        "占位",
        "具体正文",
        "恢复阶段",
        "生成符合",
        "后续剧情",
        "未完待续",
        "omitted",
        "placeholder",
        "todo",
        "to be continued",
        "specific body text",
        "current recovery stage",
        "should contain the actual",
        "due to character limit",
        "due to the character limit",
        "character limit constraints",
        "content ends at",
        "full body is truncated",
        "truncated here",
        "not shown in full",
        "cannot provide the full",
        "in a production environment",
        "would continue to complete",
        "完整内容受限",
        "篇幅限制",
        "无法完整展示",
        "受输出限制",
        "由于字数限制",
        "由于篇幅限制",
    ];
    markers
        .iter()
        .filter(|marker| {
            content.contains(**marker)
                || content
                    .to_ascii_lowercase()
                    .contains(&marker.to_ascii_lowercase())
        })
        .map(|marker| format!("chapter body contains placeholder or omission marker: {marker}"))
        .collect()
}

pub(in crate::tool::writing::novel_studio) fn line_contains_placeholder_or_omission_marker(
    line: &str,
) -> bool {
    let lowered = line.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "此处省略",
        "此处应为",
        "省略后续",
        "以下省略",
        "略去",
        "待补充",
        "占位",
        "具体正文",
        "后续剧情",
        "未完待续",
        "omitted",
        "placeholder",
        "todo",
        "to be continued",
        "specific body text",
        "current recovery stage",
        "should contain the actual",
        "due to character limit",
        "due to the character limit",
        "character limit constraints",
        "content ends at",
        "full body is truncated",
        "truncated here",
        "not shown in full",
        "cannot provide the full",
        "in a production environment",
        "would continue to complete",
        "完整内容受限",
        "篇幅限制",
        "无法完整展示",
        "受输出限制",
        "由于字数限制",
        "由于篇幅限制",
    ];
    MARKERS
        .iter()
        .any(|marker| line.contains(marker) || lowered.contains(&marker.to_ascii_lowercase()))
}

pub(in crate::tool::writing::novel_studio) fn contains_generation_meta_disclaimer(
    content: &str,
) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        let lowered = trimmed.to_ascii_lowercase();
        let looks_like_note_prefix = lowered.starts_with("[note:")
            || lowered.starts_with("note:")
            || lowered.starts_with("注:")
            || lowered.starts_with("说明:");
        let output_limit_context = lowered.contains("character limit")
            || lowered.contains("output limit")
            || lowered.contains("token limit")
            || lowered.contains("production environment")
            || lowered.contains("would continue")
            || trimmed.contains("输出限制")
            || trimmed.contains("篇幅限制")
            || trimmed.contains("字数限制");
        looks_like_note_prefix && output_limit_context
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_surface_contamination_detects_embedded_model_chapter_heading() {
        let issues = prose_surface_contamination_issues(
            "# 耐热合金\n\n#第2章逆熵的余温清晨的底层区弥漫着酸涩气味。",
        );

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("embedded model-generated chapter heading")),
            "{issues:?}"
        );
    }
}
