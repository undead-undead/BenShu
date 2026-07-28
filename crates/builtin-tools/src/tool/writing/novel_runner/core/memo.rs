use super::jsonish::extract_json;
use super::model::{
    is_chinese_language, required_memo_sections, ChapterExecutionPackage, ChapterMemo, MemoSection,
};
use serde::Deserialize;

pub(crate) fn parse_memo(markdown: &str, language: &str) -> Result<ChapterMemo, String> {
    let body = markdown.trim();
    if body.is_empty() {
        return Err("memo is empty".to_string());
    }
    let goal = parse_memo_goal(body).unwrap_or_else(|| fallback_memo_goal(body, language));

    let sections = normalize_memo_sections(body, language)?;
    let body = render_normalized_memo_body(&goal, &sections, language);

    Ok(ChapterMemo {
        goal,
        body,
        sections,
    })
}

#[derive(Default, Deserialize)]
struct RawChapterExecutionPackage {
    memo_markdown: Option<serde_json::Value>,
    architecture: Option<serde_json::Value>,
    scene_goal: Option<serde_json::Value>,
    conflict: Option<serde_json::Value>,
    choice: Option<serde_json::Value>,
    cost: Option<serde_json::Value>,
    reveal: Option<serde_json::Value>,
    emotional_beat: Option<serde_json::Value>,
    chapter_function: Option<serde_json::Value>,
    irreversible_event: Option<serde_json::Value>,
    new_state_after_chapter: Option<serde_json::Value>,
    character_change: Option<serde_json::Value>,
    relationship_change: Option<serde_json::Value>,
    power_delta: Option<serde_json::Value>,
    resource_delta: Option<serde_json::Value>,
    hook_opened: Option<serde_json::Value>,
    hook_paid_off: Option<serde_json::Value>,
    title_basis: Option<serde_json::Value>,
    future_chapters: Option<serde_json::Value>,
    new_character_requests: Option<serde_json::Value>,
}

pub(crate) fn parse_chapter_execution_package(
    raw: &str,
    language: &str,
) -> Result<ChapterExecutionPackage, String> {
    let looks_like_json_package = raw.trim_start().starts_with('{')
        || raw.contains("```json")
        || raw.contains("\"memo_markdown\"")
        || raw.contains("\"architecture\"");
    if let Some(json) = extract_json(raw) {
        match serde_json::from_str::<serde_json::Value>(&json).and_then(|value| {
            validate_execution_package_required_fields(&value)
                .map_err(|message| serde_json::Error::io(std::io::Error::other(message)))?;
            serde_json::from_value::<RawChapterExecutionPackage>(value)
        }) {
            Ok(parsed) => {
                let memo_markdown = parsed
                    .memo_markdown
                    .as_ref()
                    .map(|value| execution_package_value_to_text(value, "memo"))
                    .unwrap_or_default();
                let architecture = parsed
                    .architecture
                    .as_ref()
                    .map(|value| execution_package_value_to_text(value, "architecture"))
                    .unwrap_or_default();
                if !memo_markdown.trim().is_empty() && !architecture.trim().is_empty() {
                    let memo = parse_memo(&memo_markdown, language)?;
                    if memo_goal_is_control_fragment(&memo.goal)
                        || architecture_looks_like_embedded_package(&architecture)
                    {
                        return Err(
                            "chapter execution package contains malformed memo/architecture fields"
                                .to_string(),
                        );
                    }
                    return Ok(ChapterExecutionPackage {
                        memo,
                        architecture: render_execution_contract_header(&parsed)
                            + architecture.trim(),
                        scene_goal: execution_package_optional_text(&parsed.scene_goal),
                        conflict: execution_package_optional_text(&parsed.conflict),
                        choice: execution_package_optional_text(&parsed.choice),
                        cost: execution_package_optional_text(&parsed.cost),
                        reveal: execution_package_optional_text(&parsed.reveal),
                        emotional_beat: execution_package_optional_text(&parsed.emotional_beat),
                        chapter_function: parsed
                            .chapter_function
                            .as_ref()
                            .map(|value| execution_package_value_to_text(value, "meta"))
                            .unwrap_or_default(),
                        irreversible_event: parsed
                            .irreversible_event
                            .as_ref()
                            .map(|value| execution_package_value_to_text(value, "meta"))
                            .unwrap_or_default(),
                        new_state_after_chapter: parsed
                            .new_state_after_chapter
                            .as_ref()
                            .map(|value| execution_package_value_to_text(value, "meta"))
                            .unwrap_or_default(),
                        character_change: parsed
                            .character_change
                            .as_ref()
                            .map(|value| execution_package_value_to_text(value, "meta"))
                            .unwrap_or_default(),
                        relationship_change: parsed
                            .relationship_change
                            .as_ref()
                            .map(|value| execution_package_value_to_text(value, "meta"))
                            .unwrap_or_default(),
                        power_delta: execution_package_optional_text(&parsed.power_delta),
                        resource_delta: execution_package_optional_text(&parsed.resource_delta),
                        hook_opened: execution_package_string_array(parsed.hook_opened.as_ref()),
                        hook_paid_off: execution_package_hook_paid_off(&parsed),
                        title_basis: parsed
                            .title_basis
                            .as_ref()
                            .map(|value| execution_package_value_to_text(value, "meta"))
                            .unwrap_or_default(),
                        future_chapters: execution_package_future_chapters(
                            parsed.future_chapters.as_ref(),
                        ),
                        new_character_requests: execution_package_character_requests(
                            parsed.new_character_requests.as_ref(),
                        ),
                        degraded: false,
                        degraded_reason: String::new(),
                    });
                }
                if looks_like_json_package {
                    return Err(
                        "chapter execution package JSON is missing memo_markdown or architecture"
                            .to_string(),
                    );
                }
            }
            Err(error) if looks_like_json_package => {
                return Err(format!(
                    "chapter execution package JSON parse failed: {error}"
                ));
            }
            Err(_) => {}
        }
    }
    if looks_like_json_package {
        return Err(
            "chapter execution package looked like JSON but could not be parsed".to_string(),
        );
    }

    let memo = parse_memo(raw, language)?;
    if memo_goal_is_control_fragment(&memo.goal) {
        return Err("chapter execution package memo goal is malformed".to_string());
    }
    let architecture = compact_memo_excerpt(raw);
    Ok(ChapterExecutionPackage {
        memo,
        architecture,
        scene_goal: String::new(),
        conflict: String::new(),
        choice: String::new(),
        cost: String::new(),
        reveal: String::new(),
        emotional_beat: String::new(),
        chapter_function: String::new(),
        irreversible_event: String::new(),
        new_state_after_chapter: String::new(),
        character_change: String::new(),
        relationship_change: String::new(),
        power_delta: String::new(),
        resource_delta: String::new(),
        hook_opened: Vec::new(),
        hook_paid_off: Vec::new(),
        title_basis: String::new(),
        future_chapters: Vec::new(),
        new_character_requests: Vec::new(),
        degraded: true,
        degraded_reason: "execution package was recovered from freeform text".to_string(),
    })
}

fn validate_execution_package_required_fields(value: &serde_json::Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "chapter execution package must be a JSON object".to_string())?;
    const STRING_FIELDS: &[&str] = &[
        "scene_goal",
        "conflict",
        "choice",
        "cost",
        "reveal",
        "emotional_beat",
        "chapter_function",
        "irreversible_event",
        "new_state_after_chapter",
        "character_change",
        "relationship_change",
        "power_delta",
        "resource_delta",
        "title_basis",
    ];
    let mut missing = Vec::new();
    for field in ["memo_markdown", "architecture"] {
        if object.get(field).is_none_or(serde_json::Value::is_null) {
            missing.push(field);
        }
    }
    for field in STRING_FIELDS.iter().copied().chain([
        "hook_opened",
        "hook_paid_off",
        "future_chapters",
        "new_character_requests",
    ]) {
        if !object.contains_key(field) {
            missing.push(field);
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "chapter execution package is missing required fields: {}",
            missing.join(", ")
        ))
    }
}

fn execution_package_future_chapters(
    value: Option<&serde_json::Value>,
) -> Vec<crate::tool::writing::creation_contract_model::ChapterSeedContract> {
    let Some(serde_json::Value::Array(values)) = value else {
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

fn execution_package_character_requests(
    value: Option<&serde_json::Value>,
) -> Vec<crate::tool::writing::novel_contract_v2::ChapterCharacterRequest> {
    let values = match value {
        Some(serde_json::Value::Array(values)) => values.as_slice(),
        Some(value @ serde_json::Value::Object(_)) => std::slice::from_ref(value),
        _ => return Vec::new(),
    };
    values
        .iter()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

fn render_execution_contract_header(parsed: &RawChapterExecutionPackage) -> String {
    let scene_goal = execution_package_optional_text(&parsed.scene_goal);
    let conflict = execution_package_optional_text(&parsed.conflict);
    let choice = execution_package_optional_text(&parsed.choice);
    let cost = execution_package_optional_text(&parsed.cost);
    let reveal = execution_package_optional_text(&parsed.reveal);
    let emotional_beat = execution_package_optional_text(&parsed.emotional_beat);
    let chapter_function = parsed
        .chapter_function
        .as_ref()
        .map(|value| execution_package_value_to_text(value, "meta"))
        .unwrap_or_default();
    let irreversible_event = parsed
        .irreversible_event
        .as_ref()
        .map(|value| execution_package_value_to_text(value, "meta"))
        .unwrap_or_default();
    let new_state_after_chapter = parsed
        .new_state_after_chapter
        .as_ref()
        .map(|value| execution_package_value_to_text(value, "meta"))
        .unwrap_or_default();
    let character_change = parsed
        .character_change
        .as_ref()
        .map(|value| execution_package_value_to_text(value, "meta"))
        .unwrap_or_default();
    let relationship_change = parsed
        .relationship_change
        .as_ref()
        .map(|value| execution_package_value_to_text(value, "meta"))
        .unwrap_or_default();
    let power_delta = execution_package_optional_text(&parsed.power_delta);
    let resource_delta = execution_package_optional_text(&parsed.resource_delta);
    let hook_opened = execution_package_string_array(parsed.hook_opened.as_ref()).join("; ");
    let hook_paid_off = execution_package_hook_paid_off(parsed).join("; ");
    let title_basis = parsed
        .title_basis
        .as_ref()
        .map(|value| execution_package_value_to_text(value, "meta"))
        .unwrap_or_default();
    let lines = [
        ("scene_goal", scene_goal),
        ("conflict", conflict),
        ("choice", choice),
        ("cost", cost),
        ("reveal", reveal),
        ("emotional_beat", emotional_beat),
        ("chapter_function", chapter_function),
        ("irreversible_event", irreversible_event),
        ("new_state_after_chapter", new_state_after_chapter),
        ("character_change", character_change),
        ("relationship_change", relationship_change),
        ("power_delta", power_delta),
        ("resource_delta", resource_delta),
        ("hook_opened", hook_opened),
        ("hook_paid_off", hook_paid_off),
        ("title_basis", title_basis),
    ]
    .into_iter()
    .filter(|(_, value)| !value.trim().is_empty())
    .map(|(key, value)| format!("{key}: {}", value.trim()))
    .collect::<Vec<_>>();
    if lines.is_empty() {
        String::new()
    } else {
        format!("Execution contract:\n{}\n\n", lines.join("\n"))
    }
}

fn execution_package_hook_paid_off(parsed: &RawChapterExecutionPackage) -> Vec<String> {
    let mut values = execution_package_string_array(parsed.hook_paid_off.as_ref());
    values.retain(|value| !value.trim().is_empty());
    values.sort();
    values.dedup();
    values
}

fn execution_package_optional_text(value: &Option<serde_json::Value>) -> String {
    value
        .as_ref()
        .map(|value| execution_package_value_to_text(value, "meta"))
        .unwrap_or_default()
}

fn execution_package_string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| execution_package_value_to_text(item, "meta"))
            .filter(|item| !item.trim().is_empty())
            .collect(),
        Some(value) => {
            let rendered = execution_package_value_to_text(value, "meta");
            if rendered.trim().is_empty() {
                Vec::new()
            } else {
                vec![rendered]
            }
        }
        None => Vec::new(),
    }
}

fn execution_package_value_to_text(value: &serde_json::Value, field_name: &str) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => text.trim().to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let rendered = execution_package_value_to_text(item, field_name);
                (!rendered.trim().is_empty()).then_some(rendered)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(map) => {
            if field_name == "memo" {
                if let Some(rendered) = render_structured_memo_object(map) {
                    return rendered;
                }
            }
            render_structured_text_object(map)
        }
    }
}

fn render_structured_memo_object(
    map: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let goal = map
        .get("goal")
        .or_else(|| map.get("目标"))
        .or_else(|| map.get("chapter_goal"))
        .or_else(|| map.get("objective"))
        .map(|value| execution_package_value_to_text(value, "memo"))
        .filter(|value| !value.trim().is_empty())?;
    let mut out = format!("目标：{}\n", goal.trim());
    if let Some(sections) = map
        .get("sections")
        .or_else(|| map.get("memo_sections"))
        .or_else(|| map.get("章节备忘"))
        .or_else(|| map.get("beats"))
    {
        out.push('\n');
        match sections {
            serde_json::Value::Array(items) => {
                for item in items {
                    append_memo_section_value(&mut out, item);
                }
            }
            serde_json::Value::Object(sections) => {
                for (heading, value) in sections {
                    append_titled_text_section(&mut out, heading, value);
                }
            }
            other => {
                let rendered = execution_package_value_to_text(other, "memo");
                if !rendered.trim().is_empty() {
                    out.push_str(rendered.trim());
                    out.push('\n');
                }
            }
        }
    }
    Some(out)
}

fn append_memo_section_value(out: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            let heading = map
                .get("heading")
                .or_else(|| map.get("title"))
                .or_else(|| map.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Section");
            let content = map
                .get("content")
                .or_else(|| map.get("body"))
                .or_else(|| map.get("details"))
                .or_else(|| map.get("items"))
                .unwrap_or(value);
            append_titled_text_section(out, heading, content);
        }
        other => {
            let rendered = execution_package_value_to_text(other, "memo");
            if !rendered.trim().is_empty() {
                out.push_str("## Section\n");
                out.push_str(rendered.trim());
                out.push_str("\n\n");
            }
        }
    }
}

fn append_titled_text_section(out: &mut String, heading: &str, value: &serde_json::Value) {
    let content = execution_package_value_to_text(value, "section");
    if content.trim().is_empty() {
        return;
    }
    out.push_str("## ");
    out.push_str(heading.trim());
    out.push('\n');
    out.push_str(content.trim());
    out.push_str("\n\n");
}

fn render_structured_text_object(map: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut out = String::new();
    for (key, value) in map {
        let rendered = execution_package_value_to_text(value, key);
        if rendered.trim().is_empty() {
            continue;
        }
        out.push_str("## ");
        out.push_str(key.trim());
        out.push('\n');
        out.push_str(rendered.trim());
        out.push_str("\n\n");
    }
    out.trim().to_string()
}

fn memo_goal_is_control_fragment(goal: &str) -> bool {
    let trimmed = goal.trim();
    trimmed.is_empty()
        || matches!(trimmed, "{" | "}" | "[" | "]")
        || trimmed.starts_with('{')
        || trimmed.starts_with('[')
}

fn architecture_looks_like_embedded_package(architecture: &str) -> bool {
    let trimmed = architecture.trim_start();
    trimmed.starts_with("{")
        && (trimmed.contains("\"memo_markdown\"")
            || trimmed.contains("\"architecture\"")
            || trimmed.contains("memo_markdown"))
}

fn parse_memo_goal(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("goal:")
                .or_else(|| trimmed.strip_prefix("Goal:"))
                .or_else(|| trimmed.strip_prefix("目标："))
                .or_else(|| trimmed.strip_prefix("目标:"))
                .map(|value| value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn fallback_memo_goal(body: &str, language: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with("```")
                && !line.eq_ignore_ascii_case("markdown")
        })
        .map(|line| line.trim_start_matches('-').trim().to_string())
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| {
            if is_chinese_language(language) {
                "推进本章的明确场景变化".to_string()
            } else {
                "Advance this chapter through a concrete scene change".to_string()
            }
        })
}

fn normalize_memo_sections(body: &str, language: &str) -> Result<Vec<MemoSection>, String> {
    let parsed = parse_markdown_sections(body);
    let missing = required_memo_sections(language)
        .iter()
        .filter(|required| {
            !parsed.iter().any(|section| {
                normalize_heading(&section.heading) == normalize_heading(required)
                    && !section.body.trim().is_empty()
            })
        })
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "chapter memo is missing required sections: {}",
            missing.join(", ")
        ));
    }
    Ok(required_memo_sections(language)
        .iter()
        .filter_map(|required| {
            parsed
                .iter()
                .find(|section| {
                    normalize_heading(&section.heading) == normalize_heading(required)
                        && !section.body.trim().is_empty()
                })
                .cloned()
        })
        .map(|section| sanitize_memo_section(section, language))
        .collect())
}

fn sanitize_memo_section(section: MemoSection, language: &str) -> MemoSection {
    let normalized = normalize_heading(&section.heading);
    let is_decision_check = normalized == normalize_heading("关键抉择三连问")
        || normalized == normalize_heading("Decision Checks");
    if !is_decision_check {
        return section;
    }
    let body = section.body.trim();
    let compact = body
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let contains_visible_control_questions = compact.contains("为什么做")
        || compact.contains("是否符合人设")
        || compact.contains("读者是否突兀")
        || compact
            .to_ascii_lowercase()
            .contains("whetheritfitsthecharacter");
    if body.is_empty() || contains_visible_control_questions {
        return MemoSection {
            heading: section.heading,
            body: decision_check_instruction(language),
        };
    }
    section
}

fn decision_check_instruction(language: &str) -> String {
    if is_chinese_language(language) {
        "作者内部检查：关键选择必须由正文中的动机、行动和代价支撑；不得把检查问题或作者评语写进正文。".to_string()
    } else {
        "Internal author check: the prose must support each key choice through motive, action, and cost; never emit the checklist or author commentary as prose.".to_string()
    }
}

fn compact_memo_excerpt(body: &str) -> String {
    let excerpt = body
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with("```")
                && !line.eq_ignore_ascii_case("markdown")
                && !line.starts_with("goal:")
                && !line.starts_with("Goal:")
                && !line.starts_with("目标：")
                && !line.starts_with("目标:")
                && !memo_line_contains_visible_control_question(line)
        })
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    if excerpt.is_empty() {
        "Use the chapter context to create a concrete, continuous scene plan.".to_string()
    } else {
        excerpt.chars().take(240).collect()
    }
}

fn memo_line_contains_visible_control_question(line: &str) -> bool {
    let compact = line
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    compact.contains("为什么做")
        || compact.contains("是否符合人设")
        || compact.contains("读者是否突兀")
        || compact
            .to_ascii_lowercase()
            .contains("whetheritfitsthecharacter")
}

fn render_normalized_memo_body(goal: &str, sections: &[MemoSection], language: &str) -> String {
    let mut rendered = if is_chinese_language(language) {
        format!("目标：{goal}")
    } else {
        format!("goal: {goal}")
    };
    for section in sections {
        rendered.push_str("\n\n## ");
        rendered.push_str(section.heading.trim());
        rendered.push('\n');
        rendered.push_str(section.body.trim());
    }
    rendered
}

fn parse_markdown_sections(markdown: &str) -> Vec<MemoSection> {
    let mut sections = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_body = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            if let Some(previous) = current_heading.replace(heading.trim().to_string()) {
                sections.push(MemoSection {
                    heading: previous,
                    body: current_body.trim().to_string(),
                });
                current_body.clear();
            }
        } else if current_heading.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(heading) = current_heading {
        sections.push(MemoSection {
            heading,
            body: current_body.trim().to_string(),
        });
    }
    sections
}

pub(super) fn normalize_heading(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch: char| ch == '#' || ch == ':' || ch == '：' || ch.is_whitespace())
        .to_lowercase()
        .replace([' ', '-', '_', '/', '／'], "")
}
