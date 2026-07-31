#[cfg(test)]
use super::super::boundary_text_gate::generated_contract_boundary_text_issues;
use super::*;

#[cfg(test)]
pub fn repair_creation_contract_plan_titles(
    draft: &SessionCreationDraftState,
    contract_text: &str,
) -> Option<String> {
    if draft.artifact_kind != "fiction" {
        return None;
    }
    let issues = contract_plan_repair_issues(draft, contract_text);
    let needs_title_repair = issues.iter().any(|issue| {
        issue.contains("章节标题像句子残片")
            || issue.contains("章节标题模板过于重复")
            || issue.contains("章节标题句式过于单一")
    });
    let needs_missing_title_repair = issues
        .iter()
        .any(|issue| issue.contains("逐章规划缺少章节名"));
    let needs_goal_repair = issues
        .iter()
        .any(|issue| issue.contains("逐章规划缺少章节目标"));
    let needs_invalid_title_repair = issues
        .iter()
        .any(|issue| issue.contains("逐章规划包含未编号目标行"));
    if !needs_title_repair
        && !needs_missing_title_repair
        && !needs_goal_repair
        && !needs_invalid_title_repair
    {
        return None;
    }

    let mut changed = false;
    let mut chapter_index = 0usize;
    let mut used_titles = std::collections::BTreeSet::<String>::new();
    let mut repaired_lines = Vec::new();
    for line in contract_text.lines() {
        if needs_invalid_title_repair && line_looks_like_malformed_chapter_plan_goal(line) {
            let next_index = chapter_index + 1;
            if let Some(repaired_line) =
                repair_malformed_chapter_plan_line(next_index, line, needs_goal_repair)
            {
                chapter_index = next_index;
                if let Some(title) = chapter_plan_title_from_line(&repaired_line) {
                    used_titles.insert(normalized_title_key_for_contract(&title));
                }
                repaired_lines.push(repaired_line);
                changed = true;
                continue;
            }
            changed = true;
            continue;
        }
        if line_looks_like_explicit_chapter_plan(line) {
            chapter_index += 1;
            let mut repaired_line = line.to_string();
            if let Some(old_title) = chapter_plan_title_from_line(line) {
                let mut current_title = old_title.clone();
                if needs_title_repair
                    || needs_invalid_title_repair
                        && chapter_plan_title_is_invalid_fragment(&old_title)
                {
                    if let Some(new_title) = replacement_contract_plan_title(
                        chapter_index,
                        line,
                        &old_title,
                        &used_titles,
                    ) {
                        used_titles.insert(normalized_title_key_for_contract(&new_title));
                        if new_title != old_title {
                            changed = true;
                            repaired_line =
                                replace_contract_plan_line_title(line, &old_title, &new_title);
                            current_title = new_title;
                        }
                    }
                } else {
                    used_titles.insert(normalized_title_key_for_contract(&old_title));
                }
                if needs_goal_repair && !chapter_plan_line_has_goal(&repaired_line) {
                    changed = true;
                    repaired_line = append_contract_plan_goal(&repaired_line, &current_title);
                }
            }
            repaired_lines.push(repaired_line);
            continue;
        }
        repaired_lines.push(line.to_string());
    }

    if !changed {
        return None;
    }

    let repaired = repaired_lines.join("\n");
    let repaired_issues = contract_plan_repair_issues(draft, &repaired);
    if repaired_issues.iter().any(|issue| {
        issue.contains("章节标题句式过于单一")
            || issue.contains("章节标题模板过于重复")
            || issue.contains("逐章规划缺少章节名")
            || issue.contains("逐章规划缺少章节目标")
            || issue.contains("章节标题像句子残片")
            || issue.contains("逐章规划包含未编号目标行")
            || issue.contains("书名直接复用了主角名")
            || issue.contains("书名像合同字段")
    }) {
        None
    } else {
        Some(repaired)
    }
}

#[cfg(test)]
fn contract_plan_repair_issues(
    draft: &SessionCreationDraftState,
    contract_text: &str,
) -> Vec<String> {
    let mut issues = generated_contract_boundary_text_issues(draft, contract_text);
    if draft.artifact_kind == "fiction" {
        issues.extend(generated_fiction_contract_planning_issues(
            contract_text,
            false,
        ));
        if let Some(issue) = malformed_goal_like_plan_line_issue(contract_text) {
            issues.push(issue);
        }
        let expected = draft
            .target_units
            .zip(draft.chapter_unit_target)
            .and_then(|(total, per_chapter)| {
                longform_policy::expected_chapter_count(total, per_chapter)
            })
            .unwrap_or_else(|| count_explicit_chapter_plan_lines(contract_text));
        if expected > 0 {
            if let Some(issue) = chapter_plan_missing_title_issue(contract_text, expected) {
                issues.push(issue);
            }
            if let Some(issue) = chapter_plan_missing_goal_issue(contract_text, expected) {
                issues.push(issue);
            }
        }
    }
    issues.sort();
    issues.dedup();
    issues
}

#[cfg(test)]
pub(crate) fn repair_malformed_chapter_plan_line(
    chapter_index: usize,
    line: &str,
    needs_goal_repair: bool,
) -> Option<String> {
    let title = malformed_chapter_plan_title_from_line(line)
        .filter(|title| !chapter_plan_title_is_invalid_fragment(title))?;
    let mut goal = contract_plan_goal_text(line);
    if goal.trim().is_empty() {
        goal = title.clone();
    }
    let mut repaired = format!(
        "第{chapter_index:02}章《{title}》：本章目标：{}",
        goal.trim()
            .trim_start_matches(|ch| matches!(ch, ':' | '：' | ' ' | '\t'))
    );
    if needs_goal_repair && !chapter_plan_line_has_goal(&repaired) {
        repaired = append_contract_plan_goal(&repaired, &title);
    }
    Some(repaired)
}

#[cfg(test)]
pub(crate) fn malformed_chapter_plan_title_from_line(line: &str) -> Option<String> {
    let start = line.find('《')?;
    let tail = &line[start + '《'.len_utf8()..];
    let end = tail.find('》')?;
    let title = tail[..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

#[cfg(test)]
pub(crate) fn normalize_contract_numeric_surface(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, ',' | '，' | '_' | ' ' | '\t'))
        .collect()
}

pub(crate) fn canonicalize_outline_book_title_quotes(
    raw_outline: &str,
    canonical_title: &str,
    allowed_titles: &[String],
) -> Option<String> {
    let canonical = canonical_title.trim();
    if raw_outline.trim().is_empty() || value_missing(canonical) {
        return None;
    }
    let mut repaired = raw_outline.to_string();
    let mut changed = false;
    for quoted in quoted_book_title_like_segments(raw_outline) {
        if quoted == canonical
            || allowed_titles
                .iter()
                .any(|allowed| allowed.trim() == quoted)
        {
            continue;
        }
        if quoted_segment_is_explicit_chapter_title(raw_outline, &quoted) {
            continue;
        }
        let needle = format!("《{quoted}》");
        let replacement = format!("《{canonical}》");
        if repaired.contains(&needle) {
            repaired = repaired.replace(&needle, &replacement);
            changed = true;
        }
    }
    changed.then_some(repaired)
}

pub(crate) fn quoted_book_title_like_segments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('《') {
        let after_start = &rest[start + '《'.len_utf8()..];
        let Some(end) = after_start.find('》') else {
            break;
        };
        let candidate = after_start[..end].trim();
        if quoted_segment_looks_like_title(candidate) && !out.iter().any(|known| known == candidate)
        {
            out.push(candidate.to_string());
        }
        rest = &after_start[end + '》'.len_utf8()..];
    }
    out
}

fn quoted_segment_looks_like_title(candidate: &str) -> bool {
    let len = candidate.chars().count();
    if !(2..=16).contains(&len) {
        return false;
    }
    !candidate.contains("第")
        && !candidate.contains("章")
        && !candidate.contains("卷")
        && candidate
            .chars()
            .all(|ch| contract_plan_char_is_cjk_unified(ch) || ch == '·')
}

fn contract_plan_char_is_cjk_unified(ch: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&ch)
        || ('\u{3400}'..='\u{4DBF}').contains(&ch)
        || ('\u{20000}'..='\u{2A6DF}').contains(&ch)
        || ('\u{2A700}'..='\u{2B73F}').contains(&ch)
        || ('\u{2B740}'..='\u{2B81F}').contains(&ch)
        || ('\u{2B820}'..='\u{2CEAF}').contains(&ch)
}

pub(crate) fn quoted_segment_is_explicit_chapter_title(text: &str, quoted: &str) -> bool {
    let needle = format!("《{quoted}》");
    text.lines().any(|line| {
        let Some(index) = line.find(&needle) else {
            return false;
        };
        let prefix = &line[..index];
        prefix.contains('第') && prefix.contains('章')
    })
}

pub(crate) fn count_explicit_chapter_plan_lines(text: &str) -> usize {
    collect_explicit_chapter_plan_lines(text).len()
}

pub(crate) fn collect_explicit_chapter_plan_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut in_plan_block = false;
    for line in text.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'));
        if trimmed.is_empty() {
            in_plan_block = false;
            continue;
        }
        if line_starts_chapter_plan_block(trimmed) {
            in_plan_block = true;
            if line_looks_like_explicit_chapter_plan(trimmed) {
                lines.push(line);
            }
            continue;
        }
        if in_plan_block && line_looks_like_non_chapter_contract_heading(trimmed) {
            in_plan_block = false;
        }
        if line_looks_like_explicit_chapter_plan(trimmed)
            || (in_plan_block && line_looks_like_numbered_chapter_plan_item(trimmed))
        {
            lines.push(line);
        }
    }
    lines
}

pub(crate) fn line_starts_chapter_plan_block(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    [
        "分卷",
        "卷宗",
        "阶段安排",
        "阶段规划",
        "近期章节包",
        "章节包",
        "逐章规划",
        "章节规划",
        "章节执行包",
        "chapter package",
        "chapter plan",
        "chapter outline",
    ]
    .iter()
    .any(|term| line.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub(crate) fn line_looks_like_non_chapter_contract_heading(line: &str) -> bool {
    if line_looks_like_explicit_chapter_plan(line) || line_starts_chapter_plan_block(line) {
        return false;
    }
    let prefix = line.split(['：', ':']).next().unwrap_or_default().trim();
    if prefix.is_empty() || prefix.chars().count() > 18 {
        return false;
    }
    [
        "角色",
        "人物",
        "主角",
        "世界观",
        "规则",
        "书名",
        "题材",
        "终局",
        "结局",
        "卷宗",
        "分卷",
        "质量",
        "导出",
        "可修改",
        "Protagonist",
        "Character",
        "World",
        "Title",
    ]
    .iter()
    .any(|term| {
        prefix.contains(term)
            || prefix
                .to_ascii_lowercase()
                .contains(&term.to_ascii_lowercase())
    })
}

pub(crate) fn line_looks_like_numbered_chapter_plan_item(line: &str) -> bool {
    let trimmed = line
        .trim()
        .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'));
    let mut chars = trimmed.chars().peekable();
    let mut saw_digit = false;
    while matches!(chars.peek(), Some(ch) if ch.is_ascii_digit()) {
        saw_digit = true;
        let _ = chars.next();
    }
    if !saw_digit || !matches!(chars.next(), Some('.' | '、' | ')' | '）' | ':' | '：')) {
        return false;
    }
    let tail = chars.collect::<String>();
    let title = tail
        .trim()
        .trim_start_matches(|ch: char| matches!(ch, ':' | '：' | '-' | '—' | ' ' | '\t'))
        .split(|ch| {
            matches!(
                ch,
                '，' | ',' | '。' | '\n' | '\r' | ';' | '；' | '-' | '—' | ':' | '：'
            )
        })
        .next()
        .unwrap_or_default()
        .trim();
    !title.is_empty() && !chapter_plan_title_is_goal_marker(title)
}

#[cfg(test)]
pub(crate) fn chapter_plan_missing_goal_issue(text: &str, expected: usize) -> Option<String> {
    let lines = collect_explicit_chapter_plan_lines(text);
    if lines.len() < expected.min(6) {
        return None;
    }
    let missing = lines
        .iter()
        .filter(|line| !chapter_plan_line_has_goal(line))
        .count();
    if missing > 0 {
        Some(format!(
            "逐章规划缺少章节目标：{missing}/{} 行只有章节名；请让每章包含章节号、章节名和本章目标",
            lines.len()
        ))
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) fn malformed_goal_like_plan_line_issue(text: &str) -> Option<String> {
    text.lines()
        .find(|line| line_looks_like_malformed_chapter_plan_goal(line))
        .map(|line| {
            format!(
                "逐章规划包含未编号目标行：{}；请让每一章都使用独立章节号、章节名和本章目标",
                preview_text(line.trim(), 80)
            )
        })
}

pub(crate) fn chapter_plan_invalid_title_issue(text: &str) -> Option<String> {
    for line in collect_explicit_chapter_plan_lines(text) {
        let Some(title) = chapter_plan_title_from_line(line) else {
            continue;
        };
        if chapter_plan_title_is_invalid_fragment(&title) {
            return Some(format!(
                "章节标题像句子残片：`{}`；请让章节名来自本章独特事件、地点、物件、选择或结果",
                preview_text(&title, 40)
            ));
        }
    }
    None
}

#[cfg(test)]
pub(crate) fn chapter_plan_missing_title_issue(text: &str, expected: usize) -> Option<String> {
    let lines = collect_explicit_chapter_plan_lines(text);
    if lines.len() < expected.min(6) {
        return None;
    }
    let missing = lines
        .iter()
        .filter(|line| chapter_plan_title_from_line(line).is_none())
        .count();
    if missing > 0 {
        Some(format!(
            "逐章规划缺少章节名：{missing}/{} 行没有独立章节名；请让每章包含章节号、章节名和本章目标",
            lines.len()
        ))
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) fn chapter_plan_line_has_goal(line: &str) -> bool {
    let line = normalize_chapter_plan_goal_label(line);
    let lowered = line.to_ascii_lowercase();
    line.contains("本章目标")
        || line.contains("章节目标")
        || lowered.contains("chapter goal")
        || lowered.contains("goal:")
        || lowered.contains("objective:")
}

pub(crate) fn chapter_plan_title_diversity_issue(text: &str, expected: usize) -> Option<String> {
    let titles = collect_explicit_chapter_plan_titles(text);
    if titles.len() < expected.min(3) {
        return None;
    }

    let mut template_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut prefix_counts = std::collections::BTreeMap::<String, usize>::new();
    for title in &titles {
        let core = contract_chapter_title_core(title);
        if core.is_empty() {
            continue;
        }
        let template = contract_chapter_title_template(&core);
        if !template.is_empty() {
            *template_counts.entry(template).or_insert(0) += 1;
        }
        let prefix = cjk_prefix(&core, 2);
        if prefix.chars().count() >= 2 {
            *prefix_counts.entry(prefix).or_insert(0) += 1;
        }
    }

    let connector_title_count: usize = template_counts.values().sum();
    let connector_limit = if titles.len() < 6 {
        titles.len() / 2
    } else {
        ((titles.len() * 2) / 5).max(3)
    };
    if connector_title_count > connector_limit {
        return Some(format!(
            "章节标题句式过于单一：含“的/之/中”等连接词的标题出现 {connector_title_count}/{} 次；请混合使用事件名、地点名、选择名、物件名、动作名和结果名",
            titles.len()
        ));
    }

    let template_limit = if titles.len() < 6 {
        1
    } else {
        (titles.len() / 4).max(2)
    };
    if let Some((template, count)) = template_counts
        .iter()
        .find(|(_, count)| **count > template_limit)
    {
        return Some(format!(
            "章节标题模板过于重复：模板 `{template}` 出现 {count} 次；请让每章标题来自本章独特事件、选择、地点或代价"
        ));
    }

    let prefix_limit = if titles.len() < 6 {
        2
    } else {
        (titles.len() / 4).max(2)
    };
    if let Some((prefix, count)) = prefix_counts
        .iter()
        .find(|(_, count)| **count > prefix_limit)
    {
        return Some(format!(
            "章节标题核心字根过于重复：`{prefix}` 开头出现 {count} 次；请按逐章事件重新命名，避免同一意象反复套用"
        ));
    }

    None
}

pub(crate) fn collect_explicit_chapter_plan_titles(text: &str) -> Vec<String> {
    collect_explicit_chapter_plan_lines(text)
        .into_iter()
        .filter_map(chapter_plan_title_from_line)
        .filter(|title| !title.trim().is_empty())
        .collect()
}

pub(crate) fn chapter_plan_title_from_line(line: &str) -> Option<String> {
    if !line_looks_like_explicit_chapter_plan(line)
        && !line_looks_like_numbered_chapter_plan_item(line)
    {
        return None;
    }
    if let Some(start) = line.find('《') {
        if let Some(end) = line[start + '《'.len_utf8()..].find('》') {
            let title = &line[start + '《'.len_utf8()..start + '《'.len_utf8() + end];
            let title = title.trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    let mut value = line
        .trim()
        .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'))
        .to_string();
    if let Some(index) = value.find('第') {
        if index <= 8 {
            value = value[index..].to_string();
        }
    }
    if let Some(index) = value.find('章') {
        let before = value[..index].trim();
        if before.contains('第') && before.chars().count() <= 12 {
            value = value[index + '章'.len_utf8()..].to_string();
        } else if let Some((_, tail)) = value.split_once(|ch| matches!(ch, '.' | '、' | ')' | '）'))
        {
            value = tail.to_string();
        }
    } else if let Some((_, tail)) = value.split_once(|ch| matches!(ch, '.' | '、' | ')' | '）')) {
        value = tail.to_string();
    }
    let value = value
        .trim_start_matches(|ch: char| {
            matches!(ch, ':' | '：' | '-' | '—' | ' ' | '\t' | '、' | '.' | '．')
        })
        .trim();
    let title = value
        .split(|ch| {
            matches!(
                ch,
                '，' | ',' | '。' | '\n' | '\r' | ';' | '；' | '-' | '—' | ':' | '：'
            )
        })
        .next()
        .unwrap_or_default()
        .trim();
    if title.is_empty() || chapter_plan_title_is_goal_marker(title) {
        None
    } else {
        Some(title.to_string())
    }
}

#[cfg(test)]
pub(crate) fn replace_contract_plan_line_title(
    line: &str,
    old_title: &str,
    new_title: &str,
) -> String {
    if let Some(start) = line.find('《') {
        if let Some(end) = line[start + '《'.len_utf8()..].find('》') {
            let absolute_end = start + '《'.len_utf8() + end;
            let mut value = String::new();
            value.push_str(&line[..start + '《'.len_utf8()]);
            value.push_str(new_title);
            value.push_str(&line[absolute_end..]);
            return value;
        }
    }
    line.replacen(old_title, new_title, 1)
}

pub(crate) fn chapter_plan_title_is_goal_marker(title: &str) -> bool {
    let title = title.trim();
    title.is_empty()
        || title == "本章目标"
        || title == "章节目标"
        || title == "目标"
        || title.to_ascii_lowercase() == "goal"
        || title.to_ascii_lowercase() == "objective"
}

#[cfg(test)]
pub(crate) fn append_contract_plan_goal(line: &str, title: &str) -> String {
    let title = title.trim();
    let goal = if title.is_empty() {
        "推进本阶段事件，并形成下一章必须继承的状态变化".to_string()
    } else {
        format!("围绕“{title}”推进本阶段事件，并形成下一章必须继承的状态变化")
    };
    let trimmed = line.trim_end_matches(|ch| matches!(ch, '。' | '.' | ';' | '；'));
    format!("{trimmed}：本章目标：{goal}。")
}

#[cfg(test)]
pub(crate) fn replacement_contract_plan_title(
    chapter_index: usize,
    line: &str,
    old_title: &str,
    used_titles: &std::collections::BTreeSet<String>,
) -> Option<String> {
    let mut candidates = Vec::new();
    let old_core = contract_chapter_title_core(old_title);
    let old_without_connectors = old_core
        .chars()
        .filter(|ch| !contract_title_template_connector(*ch))
        .collect::<String>();
    push_contract_plan_title_candidate(&mut candidates, &old_without_connectors);

    let goal_text = contract_plan_goal_text(line);
    collect_contract_plan_title_candidates(&goal_text, &mut candidates);
    collect_contract_plan_title_candidates(line, &mut candidates);

    candidates.sort_by(|left, right| {
        contract_plan_title_candidate_score(right, chapter_index)
            .cmp(&contract_plan_title_candidate_score(left, chapter_index))
            .then_with(|| left.chars().count().cmp(&right.chars().count()))
    });
    candidates.dedup_by(|left, right| {
        normalized_title_key_for_contract(left) == normalized_title_key_for_contract(right)
    });

    candidates.into_iter().find(|candidate| {
        let key = normalized_title_key_for_contract(candidate);
        !key.is_empty()
            && !used_titles.contains(&key)
            && contract_chapter_title_template(candidate).is_empty()
            && contract_plan_title_candidate_is_useful(candidate)
    })
}

#[cfg(test)]
pub(crate) fn contract_plan_goal_text(line: &str) -> String {
    let normalized = normalize_chapter_plan_goal_label(line);
    for marker in ["本章目标", "章节目标", "goal", "objective"] {
        if let Some(index) = normalized.to_ascii_lowercase().find(marker) {
            return normalized[index + marker.len()..]
                .trim_start_matches(|ch| matches!(ch, ':' | '：' | '-' | '—' | ' ' | '\t'))
                .trim()
                .to_string();
        }
    }
    String::new()
}

#[cfg(test)]
pub(crate) fn collect_contract_plan_title_candidates(text: &str, out: &mut Vec<String>) {
    let mut run = String::new();
    for ch in text.chars() {
        if is_cjk_unified(ch) {
            if contract_title_template_connector(ch) {
                push_contract_plan_title_candidate(out, &run);
                run.clear();
            } else {
                run.push(ch);
            }
        } else {
            push_contract_plan_title_candidate(out, &run);
            run.clear();
        }
    }
    push_contract_plan_title_candidate(out, &run);
}

#[cfg(test)]
pub(crate) fn push_contract_plan_title_candidate(out: &mut Vec<String>, value: &str) {
    let value = value
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '“' | '”' | '《' | '》' | ':' | '：'))
        .to_string();
    let len = value.chars().count();
    if (2..=8).contains(&len) && contract_plan_title_candidate_is_useful(&value) {
        out.push(value);
        return;
    }
}

#[cfg(test)]
pub(crate) fn contract_plan_title_candidate_is_useful(value: &str) -> bool {
    let trimmed = value.trim();
    let len = trimmed.chars().count();
    if !(2..=8).contains(&len) {
        return false;
    }
    if trimmed.chars().any(contract_title_template_connector) {
        return false;
    }
    let hard_noise = [
        "本章",
        "目标",
        "章节",
        "主角",
        "角色",
        "反派",
        "通过",
        "获取",
        "获得",
        "资源",
        "尝试",
        "一次",
        "第一次",
        "第一件",
        "直面",
        "世界规",
        "寻找",
        "意识到",
        "识到",
        "修为",
        "提升",
        "拯救世界",
        "做出",
        "展开",
        "展现",
        "由于",
        "因为",
        "为了",
        "进行",
        "面临",
        "意识",
        "升级",
        "最终",
        "阻拦",
        "解决",
        "结局",
        "定格",
        "关键",
        "现状",
    ];
    if hard_noise.iter().any(|term| trimmed.contains(term)) {
        return false;
    }
    let soft_noise = [
        "故事", "合同", "规划", "推进", "建立", "完成", "继续", "进入", "通过", "进行", "发现",
        "核心", "矛盾", "选择", "代价",
    ];
    if soft_noise
        .iter()
        .any(|term| trimmed == *term || trimmed.contains(term) && len <= 4)
    {
        return false;
    }
    if chapter_plan_title_is_invalid_fragment(trimmed) {
        return false;
    }
    trimmed.chars().all(is_cjk_unified)
}

#[cfg(test)]
pub(crate) fn contract_plan_title_candidate_score(value: &str, chapter_index: usize) -> i32 {
    let len = value.chars().count() as i32;
    let mut score = 20 - (len - 4).abs();
    if value.chars().count() == 2 {
        score -= 4;
    }
    if value.contains(&chapter_index.to_string()) {
        score -= 8;
    }
    score
}

#[cfg(test)]
pub(crate) fn normalized_title_key_for_contract(value: &str) -> String {
    value
        .chars()
        .filter(|ch| is_cjk_unified(*ch) || ch.is_ascii_alphanumeric())
        .collect()
}

pub(crate) fn contract_chapter_title_core(title: &str) -> String {
    naming::chapter_title_core(title)
}

pub(crate) fn contract_chapter_title_template(core: &str) -> String {
    naming::chapter_title_template(core)
}

pub(crate) fn contract_title_template_connector(ch: char) -> bool {
    naming::title_template_connector(ch)
}

pub(crate) fn chapter_plan_title_is_invalid_fragment(title: &str) -> bool {
    let title = contract_chapter_title_core(title);
    let len = title.chars().count();
    if !(2..=16).contains(&len) {
        return true;
    }
    if title.starts_with('角')
        || title.starts_with('但')
        || title.starts_with('在')
        || title.starts_with("被迫")
        || title.starts_with("通过")
        || title.starts_with("尝试")
        || title.starts_with("一次")
        || title.starts_with("由于")
        || title.starts_with("因为")
        || title.starts_with("为了")
        || title.starts_with("进行")
        || title.starts_with("获得")
        || title.starts_with('临') && !title.starts_with("临界")
        || title.starts_with('次')
        || title.starts_with('心')
        || title.starts_with('过')
        || title.starts_with('开')
        || title.starts_with('取')
        || title.starts_with('面')
        || title.starts_with('到')
        || title.contains('在')
    {
        return true;
    }
    if title.contains("本章")
        || title.contains("目标")
        || title.contains("主角")
        || title.contains("角色")
        || title.contains("反派")
        || title.contains("通过")
        || title.contains("获取")
        || title.contains("获得")
        || title.contains("第一件")
        || title.contains("世界规")
        || title.contains("意识到")
        || title.contains("识到")
        || title.contains("修为")
        || title.contains("提升")
        || title.contains("一次")
        || title.contains("第一次")
        || title.contains("展开")
        || title.contains("展现")
        || title.contains("由于")
        || title.contains("因为")
        || title.contains("为了")
        || title.contains("进行")
        || title.contains("面临")
        || title.contains("成长线")
        || title.contains("成长弧线")
        || title.contains("关系线")
        || title.contains("关键转折")
        || title.contains("阶段目标")
        || title.contains("冲突解决")
        || title.contains("情感与")
        || title.contains("力量间")
        || title.contains("拯救世界")
        || title == "逃离界"
        || matches!(
            title.as_str(),
            "做出" | "最终" | "阻拦" | "解决" | "重生在贫" | "意识与力"
        )
    {
        return true;
    }
    if let Some(last) = title.chars().last() {
        if contract_title_template_connector(last)
            || matches!(
                last,
                '而' | '到' | '与' | '在' | '把' | '被' | '为' | '过' | '了'
            )
        {
            return true;
        }
    }
    false
}

pub(crate) fn cjk_prefix(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|ch| is_cjk_unified(*ch))
        .take(limit)
        .collect()
}

pub(crate) fn line_looks_like_explicit_chapter_plan(line: &str) -> bool {
    let trimmed = line
        .trim()
        .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'));
    if let Some(index) = trimmed.find('第') {
        let prefix = trimmed[..index].trim();
        let inline_plan_heading = prefix.contains("章节包")
            || prefix.contains("近期章节")
            || prefix.contains("逐章规划")
            || prefix.to_ascii_lowercase().contains("chapter package")
            || prefix.to_ascii_lowercase().contains("chapter plan");
        if index <= 8 || inline_plan_heading {
            let mut chars = trimmed[index..].chars();
            let _ = chars.next();
            while matches!(chars.clone().next(), Some(' ' | '\t')) {
                let _ = chars.next();
            }
            if matches!(chars.clone().next(), Some('第')) {
                return false;
            }
            let mut saw_number = false;
            for ch in chars {
                if ch.is_whitespace() {
                    continue;
                }
                if is_cjk_number_char(ch) || ch.is_ascii_digit() {
                    saw_number = true;
                    continue;
                }
                return saw_number && ch == '章';
            }
            return false;
        }
    }
    let mut digits = 0usize;
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
            continue;
        }
        if digits > 0 && ch == '章' {
            return true;
        }
        if digits > 0 && matches!(ch, '.' | '、' | ')' | '）') {
            let tail = trimmed
                .get(
                    trimmed
                        .find(ch)
                        .map(|index| index + ch.len_utf8())
                        .unwrap_or(0)..,
                )
                .unwrap_or_default()
                .trim();
            return tail.contains("本章目标")
                || tail.contains("章节目标")
                || tail.starts_with('第')
                || tail.contains("章《")
                || tail.to_ascii_lowercase().contains("chapter goal");
        }
        return false;
    }
    false
}

#[cfg(test)]
pub(crate) fn line_looks_like_malformed_chapter_plan_goal(line: &str) -> bool {
    if line_looks_like_explicit_chapter_plan(line) {
        return false;
    }
    let trimmed = line
        .trim()
        .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'));
    let contains_strict_goal_label = trimmed.contains("本章目标");
    let contains_loose_goal_label = trimmed.contains("章节目标");
    if !contains_strict_goal_label && !contains_loose_goal_label {
        return false;
    }
    let has_chapter_shape = trimmed.contains('《')
        || trimmed.to_ascii_lowercase().contains("chapter")
        || trimmed.find('第').is_some_and(|index| {
            index <= 8 && trimmed[index..].chars().take(12).any(|ch| ch == '章')
        });
    if !has_chapter_shape {
        if contains_strict_goal_label {
            let prefix = trimmed.split(['：', ':']).next().unwrap_or_default().trim();
            let prefix_len = prefix.chars().count();
            let looks_like_contract_note = [
                "机制", "质量", "审稿", "修订", "导出", "合同", "要求", "边界", "规则",
            ]
            .iter()
            .any(|term| prefix.contains(term));
            return (2..=12).contains(&prefix_len) && !looks_like_contract_note;
        }
        return false;
    }
    if contains_strict_goal_label {
        return trimmed.contains('：') || trimmed.contains(':');
    }
    trimmed.contains('：') || trimmed.contains(':')
}
