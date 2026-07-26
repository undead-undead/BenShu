//! Boundary normalizer for model-emitted writing creation contracts.
//!
//! This module only repairs structural JSON boundary issues. It must not
//! generate story content, names, titles, or plot details.

use serde_json::{Map, Value};

use super::novel_runner;

#[derive(Debug, Clone)]
pub(crate) struct NormalizedCreationContract {
    pub(crate) value: Value,
    pub(crate) json: String,
}

pub(crate) fn normalize_creation_contract_boundary(
    raw: &str,
) -> Option<NormalizedCreationContract> {
    for json in creation_contract_json_candidates(raw) {
        let Some(mut value) = parse_contract_json_value(&json) else {
            continue;
        };
        normalize_common_key_aliases(&mut value, ROOT_CONTRACT_SCHEMA_KEYS);
        unwrap_contract_container(&mut value);
        normalize_common_key_aliases(&mut value, ROOT_CONTRACT_SCHEMA_KEYS);
        normalize_contract_object(&mut value);
        let json = serde_json::to_string(&value).ok()?;
        return Some(NormalizedCreationContract { value, json });
    }
    None
}

fn creation_contract_json_candidates(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    push_unique_json_candidate(&mut out, novel_runner::extract_json(raw));
    for block in fenced_code_blocks(raw) {
        push_balanced_json_candidates(&mut out, block);
    }
    push_balanced_json_candidates(&mut out, raw);
    out
}

fn push_unique_json_candidate(out: &mut Vec<String>, candidate: Option<String>) {
    let Some(candidate) = candidate else {
        return;
    };
    let candidate = candidate.trim();
    if candidate.starts_with('{') && candidate.ends_with('}') && !out.iter().any(|v| v == candidate)
    {
        out.push(candidate.to_string());
    }
}

fn fenced_code_blocks(raw: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = raw;
    while let Some(start) = rest.find("```") {
        rest = &rest[start + 3..];
        if let Some(end) = rest.find("```") {
            let mut block = rest[..end].trim_start();
            block = block
                .strip_prefix("json")
                .or_else(|| block.strip_prefix("JSON"))
                .unwrap_or(block)
                .trim_start();
            blocks.push(block);
            rest = &rest[end + 3..];
        } else {
            break;
        }
    }
    blocks
}

fn push_balanced_json_candidates(out: &mut Vec<String>, raw: &str) {
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut start = None::<usize>;
    for (idx, ch) in raw.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        if ch == '{' {
            if depth == 0 {
                start = Some(idx);
            }
            depth += 1;
            continue;
        }
        if ch != '}' || depth == 0 {
            continue;
        }
        depth -= 1;
        if depth == 0 {
            if let Some(start_idx) = start.take() {
                let candidate = raw[start_idx..idx + ch.len_utf8()].trim();
                push_unique_json_candidate(out, Some(candidate.to_string()));
            }
        }
    }
}

fn parse_contract_json_value(json: &str) -> Option<Value> {
    serde_json::from_str::<Value>(json).ok().or_else(|| {
        repair_common_contract_json_drift(json).and_then(|fixed| serde_json::from_str(&fixed).ok())
    })
}

fn repair_common_contract_json_drift(json: &str) -> Option<String> {
    let mut repaired = json.to_string();
    repaired = repaired.replace("\n_avoid\"", "\n\"must_avoid\"");
    repaired = repaired.replace(",_avoid\"", ",\"must_avoid\"");
    repaired = repaired.replace("\"prem새\"", "\"premise\"");
    repaired = repair_chinese_colon_before_json_key_value(&repaired);
    repaired = repair_stray_prefix_before_json_key(&repaired);
    repaired = repair_missing_opening_quote_before_json_key(&repaired);
    repaired = drop_orphan_contract_json_lines(&repaired);
    (repaired != json).then_some(repaired)
}

fn repair_chinese_colon_before_json_key_value(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut cursor = 0usize;

    while let Some(relative_quote) = json[cursor..].find('"') {
        let start = cursor + relative_quote;
        let previous = json[..start].chars().rev().find(|ch| !ch.is_whitespace());
        if !matches!(previous, Some('{') | Some(',')) {
            out.push_str(&json[cursor..start + 1]);
            cursor = start + 1;
            continue;
        }

        let mut colon = None;
        for (offset, ch) in json[start + 1..].char_indices() {
            let index = start + 1 + offset;
            if index.saturating_sub(start) > 96 || ch == '"' {
                break;
            }
            if ch == '：' {
                colon = Some(index);
                break;
            }
        }

        let Some(colon) = colon else {
            out.push_str(&json[cursor..start + 1]);
            cursor = start + 1;
            continue;
        };
        let key = json[start + 1..colon].trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric() || is_cjk_key_char(ch))
        {
            out.push_str(&json[cursor..start + 1]);
            cursor = start + 1;
            continue;
        }

        let mut value_start = colon + '：'.len_utf8();
        while let Some(ch) = json[value_start..].chars().next() {
            if !ch.is_whitespace() {
                break;
            }
            value_start += ch.len_utf8();
        }

        let Some(skip) = json[value_start..]
            .strip_prefix("\\\"")
            .map(|_| 2)
            .or_else(|| json[value_start..].strip_prefix('"').map(|_| 1))
        else {
            out.push_str(&json[cursor..start + 1]);
            cursor = start + 1;
            continue;
        };

        out.push_str(&json[cursor..start]);
        out.push('"');
        out.push_str(key);
        out.push_str("\":\"");
        cursor = value_start + skip;
    }

    out.push_str(&json[cursor..]);
    out
}

fn is_cjk_key_char(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x9FFF)
}

fn repair_stray_prefix_before_json_key(json: &str) -> String {
    json.lines()
        .map(|line| {
            let leading_len = line.len() - line.trim_start().len();
            let (leading, rest) = line.split_at(leading_len);
            let trimmed = rest.trim_start();
            let Some(after_marker) = trimmed.strip_prefix('_') else {
                return line.to_string();
            };
            let candidate = after_marker.trim_start();
            if candidate.starts_with('"') && candidate.contains(':') {
                format!("{leading}{candidate}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn repair_missing_opening_quote_before_json_key(json: &str) -> String {
    json.lines()
        .map(|line| {
            let leading_len = line.len() - line.trim_start().len();
            let (leading, rest) = line.split_at(leading_len);
            let trimmed = rest.trim_start();
            if trimmed.starts_with('"') {
                return line.to_string();
            }
            let Some(quote_pos) = trimmed.find("\":") else {
                return line.to_string();
            };
            let key = &trimmed[..quote_pos];
            if key.is_empty()
                || !key
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                return line.to_string();
            }
            format!("{leading}\"{trimmed}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn drop_orphan_contract_json_lines(json: &str) -> String {
    json.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                return true;
            }
            let first = trimmed.chars().next().unwrap_or_default();
            if matches!(first, '{' | '}' | '[' | ']' | '"' | ',') {
                return true;
            }
            !(trimmed.contains('"') && !trimmed.contains(':'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_common_key_aliases(value: &mut Value, near_schema_keys: &'static [&'static str]) {
    match value {
        Value::Object(object) => {
            let keys = object.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let Some(canonical) = canonical_contract_key_alias(&key, near_schema_keys) else {
                    continue;
                };
                if key == canonical {
                    continue;
                }
                if object.contains_key(canonical) {
                    object.remove(&key);
                    continue;
                }
                if let Some(value) = object.remove(&key) {
                    object.insert(canonical.to_string(), value);
                }
            }
            for (key, value) in object.iter_mut() {
                normalize_common_key_aliases(value, nested_contract_schema_keys(key));
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_common_key_aliases(item, near_schema_keys);
            }
        }
        _ => {}
    }
}

fn canonical_contract_key_alias(
    key: &str,
    near_schema_keys: &'static [&'static str],
) -> Option<&'static str> {
    if let Some(canonical) = match key {
        "书名" | "标题" | "作品名" => Some("title"),
        "书名理由" | "命名理由" | "标题理由" => Some("title_rationale"),
        "书名候选" | "标题候选" => Some("title_candidates"),
        "语言" => Some("language"),
        "题材" | "类型" => Some("genre"),
        "简述" | "创作简述" => Some("brief"),
        "总字数" | "目标字数" | "总目标字数" => Some("target_units"),
        "每章字数" | "每章目标字数" | "每章档位" => Some("chapter_unit_target"),
        "每轮最多章节" => Some("max_chapters_per_turn"),
        "故事前提" | "前提" => Some("premise"),
        "终局方向" | "结局方向" | "结尾承诺" => Some("ending_direction"),
        "终局状态" | "最终状态" => Some("final_state"),
        "主角弧线" | "主角弧光" | "成长线" => Some("protagonist_arc"),
        "世界观意象" | "世界意象" | "核心意象" => Some("world_imagery"),
        "总主线因果链" | "主线因果链" | "主线因果" => Some("main_causal_spine"),
        "角色权威表" | "人物权威表" | "角色表" => Some("characters"),
        "核心主题" | "主题" | "主题承诺" => Some("themes"),
        "世界规则" | "规则" => Some("world_rules"),
        "叙事风格" | "文风" | "风格" => Some("style_rules"),
        "必须避免" | "禁忌" | "禁区" => Some("must_avoid"),
        "全书大纲" | "故事大纲" | "大纲" | "结构合同" => Some("outline"),
        "近期章节包" | "近期章节" | "章节规划" => Some("near_chapters"),
        "分卷" | "卷宗" | "卷规划" | "分卷规划" => Some("volumes"),
        "姓名" | "名字" => Some("canonical_name"),
        "角色" | "身份" | "定位" => Some("role"),
        "欲望" => Some("desire"),
        "恐惧" => Some("fear"),
        "底线" => Some("bottom_line"),
        "弧线起点" | "起点" => Some("arc_start"),
        "弧线终点" | "终点" => Some("arc_end"),
        "卷名" => Some("title"),
        "阶段目标" | "卷目标" => Some("objective"),
        "卷尾变化" | "卷尾转折" => Some("ending_change"),
        "本章目标" | "章节目标" => Some("goal"),
        "预期转折" | "不可逆变化" => Some("expected_turn"),
        "canonicaltitle" | "canonicalTitle" => Some("canonical_title"),
        "targetunits" | "targetUnits" => Some("target_units"),
        "chapterunittarget" | "chapterUnitTarget" => Some("chapter_unit_target"),
        "maxchaptersperturn" | "maxChaptersPerTurn" => Some("max_chapters_per_turn"),
        "desiredresolution" | "desiredResolution" => Some("desired_resolution"),
        "finalstate" | "finalState" => Some("final_state"),
        "mustresolve" | "mustResolve" => Some("must_resolve"),
        "allowedopenquestions" | "allowedOpenQuestions" => Some("allowed_open_questions"),
        "protagonistarc" | "protagonistArc" => Some("protagonist_arc"),
        "worldimagery" | "worldImagery" => Some("world_imagery"),
        "maincausalspine" | "mainCausalSpine" => Some("main_causal_spine"),
        "rawoutline" | "rawOutline" => Some("raw_outline"),
        "nearchapters" | "nearChapters" => Some("near_chapters"),
        "expectedturn" | "expectedTurn" => Some("expected_turn"),
        "canonicalname" | "canonicalName" => Some("canonical_name"),
        "bottomline" | "bottomLine" => Some("bottom_line"),
        "arcstart" | "arcStart" => Some("arc_start"),
        "arcend" | "arcEnd" => Some("arc_end"),
        _ => None,
    } {
        return Some(canonical);
    }

    canonical_contract_key_by_near_schema_match(key, near_schema_keys)
}

fn canonical_contract_key_by_near_schema_match(
    key: &str,
    schema_keys: &[&'static str],
) -> Option<&'static str> {
    let normalized = normalize_contract_key_surface(key);
    if normalized.len() < 5 {
        return canonical_contract_key_by_schema_prefix_with_noise(key, &normalized, schema_keys);
    }
    let mut best = None::<(&'static str, usize)>;
    for canonical in schema_keys {
        let target = normalize_contract_key_surface(canonical);
        if normalized == target {
            return Some(canonical);
        }
        let Some(distance) = bounded_levenshtein(&normalized, &target, 2) else {
            continue;
        };
        if distance <= 2
            && normalized.len().abs_diff(target.len()) <= 2
            && best.map(|(_, current)| distance < current).unwrap_or(true)
        {
            best = Some((canonical, distance));
        }
    }
    best.map(|(canonical, _)| canonical)
}

fn canonical_contract_key_by_schema_prefix_with_noise(
    original: &str,
    normalized: &str,
    schema_keys: &[&'static str],
) -> Option<&'static str> {
    if normalized.is_empty()
        || original
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    let mut best = None::<(&'static str, usize)>;
    for canonical in schema_keys {
        let target = normalize_contract_key_surface(canonical);
        if target.len() < 5 || !target.starts_with(normalized) {
            continue;
        }
        let missing = target.len() - normalized.len();
        if missing <= 3 && best.map(|(_, current)| missing < current).unwrap_or(true) {
            best = Some((canonical, missing));
        }
    }
    best.map(|(canonical, _)| canonical)
}

const ROOT_CONTRACT_SCHEMA_KEYS: &[&str] = &[
    "canonical_title",
    "target_units",
    "chapter_unit_target",
    "max_chapters_per_turn",
    "protagonist_arc",
    "world_imagery",
    "main_causal_spine",
    "premise",
    "world_rules",
    "style_rules",
    "must_avoid",
];

const TITLE_CONTRACT_SCHEMA_KEYS: &[&str] = &["canonical_title"];
const ENDING_CONTRACT_SCHEMA_KEYS: &[&str] = &[
    "desired_resolution",
    "final_state",
    "must_resolve",
    "allowed_open_questions",
];
const CHARACTER_CONTRACT_SCHEMA_KEYS: &[&str] =
    &["canonical_name", "bottom_line", "arc_start", "arc_end"];
const OUTLINE_CONTRACT_SCHEMA_KEYS: &[&str] = &["raw_outline", "near_chapters"];
const VOLUME_CONTRACT_SCHEMA_KEYS: &[&str] = &["title", "objective", "ending_change"];
const CHAPTER_CONTRACT_SCHEMA_KEYS: &[&str] = &["number", "goal", "expected_turn"];

fn nested_contract_schema_keys(key: &str) -> &'static [&'static str] {
    match key {
        "title" => TITLE_CONTRACT_SCHEMA_KEYS,
        "ending" => ENDING_CONTRACT_SCHEMA_KEYS,
        "characters" => CHARACTER_CONTRACT_SCHEMA_KEYS,
        "outline" => OUTLINE_CONTRACT_SCHEMA_KEYS,
        "volumes" => VOLUME_CONTRACT_SCHEMA_KEYS,
        "near_chapters" => CHAPTER_CONTRACT_SCHEMA_KEYS,
        _ => &[],
    }
}

fn normalize_contract_key_surface(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn bounded_levenshtein(left: &str, right: &str, max_distance: usize) -> Option<usize> {
    if left.len().abs_diff(right.len()) > max_distance {
        return None;
    }
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0; right_chars.len() + 1];
    for (left_index, left_ch) in left.chars().enumerate() {
        current[0] = left_index + 1;
        let mut row_min = current[0];
        for (right_index, right_ch) in right_chars.iter().enumerate() {
            let insert_cost = current[right_index] + 1;
            let delete_cost = previous[right_index + 1] + 1;
            let replace_cost = previous[right_index] + usize::from(left_ch != *right_ch);
            let value = insert_cost.min(delete_cost).min(replace_cost);
            current[right_index + 1] = value;
            row_min = row_min.min(value);
        }
        if row_min > max_distance {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let distance = previous[right_chars.len()];
    (distance <= max_distance).then_some(distance)
}

fn unwrap_contract_container(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for key in [
        "contract",
        "creation_contract",
        "novel_contract",
        "fiction_contract",
        "draft",
    ] {
        let Some(inner) = object.get(key).cloned() else {
            continue;
        };
        if inner.is_object() {
            *value = inner;
            return;
        }
    }
}

fn normalize_contract_object(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    normalize_title(object);
    normalize_ending(object);
    normalize_outline(object);
    rename_alias(object, "_world_rules", "world_rules");
    rename_alias(object, "rules", "world_rules");
    rename_alias(object, "must_avoid_rules", "must_avoid");
    rename_alias(object, "structured_contract_v2", "structured");

    for key in [
        "title_candidates",
        "themes",
        "world_rules",
        "style_rules",
        "must_avoid",
        "characters",
    ] {
        coerce_string_or_object_list(object, key);
    }
}

fn normalize_title(object: &mut Map<String, Value>) {
    let mut title_object = match object.remove("title") {
        Some(Value::Object(map)) => map,
        Some(Value::String(value)) => {
            let mut map = Map::new();
            map.insert("canonical_title".to_string(), Value::String(value));
            map
        }
        Some(other) => {
            let mut map = Map::new();
            map.insert("canonical_title".to_string(), other);
            map
        }
        None => Map::new(),
    };

    move_scalar(
        object,
        &mut title_object,
        "canonical_title",
        "canonical_title",
    );
    move_scalar(object, &mut title_object, "book_title", "canonical_title");
    move_scalar(object, &mut title_object, "work_title", "canonical_title");
    move_scalar(object, &mut title_object, "title_rationale", "rationale");
    move_scalar(object, &mut title_object, "rationale", "rationale");
    move_scalar(object, &mut title_object, "title_source", "source");
    move_array(object, &mut title_object, "title_candidates", "candidates");
    if let Some(value) = title_object.get_mut("candidates") {
        coerce_value_to_array(value);
    }

    if !title_object.is_empty() {
        object.insert("title".to_string(), Value::Object(title_object));
    }
}

fn normalize_ending(object: &mut Map<String, Value>) {
    let mut ending_object = match object.remove("ending") {
        Some(Value::Object(map)) => map,
        Some(Value::String(value)) => {
            let mut map = Map::new();
            map.insert("desired_resolution".to_string(), Value::String(value));
            map
        }
        Some(other) => {
            let mut map = Map::new();
            map.insert("desired_resolution".to_string(), other);
            map
        }
        None => Map::new(),
    };

    move_scalar(
        object,
        &mut ending_object,
        "ending_direction",
        "desired_resolution",
    );
    move_scalar(
        object,
        &mut ending_object,
        "desired_resolution",
        "desired_resolution",
    );
    move_scalar(object, &mut ending_object, "final_state", "final_state");
    move_array(object, &mut ending_object, "must_resolve", "must_resolve");
    move_array(
        object,
        &mut ending_object,
        "allowed_open_questions",
        "allowed_open_questions",
    );
    for key in ["must_resolve", "allowed_open_questions"] {
        if let Some(value) = ending_object.get_mut(key) {
            coerce_value_to_array(value);
        }
    }

    if !ending_object.is_empty() {
        object.insert("ending".to_string(), Value::Object(ending_object));
    }
}

fn normalize_outline(object: &mut Map<String, Value>) {
    let mut outline_object = match object.remove("outline") {
        Some(Value::Object(map)) => map,
        Some(Value::String(value)) => {
            let mut map = Map::new();
            map.insert("raw_outline".to_string(), Value::String(value));
            map
        }
        Some(Value::Array(items)) => {
            let mut map = Map::new();
            let raw = items
                .into_iter()
                .filter_map(|item| item.as_str().map(str::trim).map(ToString::to_string))
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            map.insert("raw_outline".to_string(), Value::String(raw));
            map
        }
        Some(other) => {
            let mut map = Map::new();
            map.insert("raw_outline".to_string(), other);
            map
        }
        None => Map::new(),
    };

    move_scalar(object, &mut outline_object, "raw_outline", "raw_outline");
    move_array(object, &mut outline_object, "volumes", "volumes");
    move_array(
        object,
        &mut outline_object,
        "near_chapters",
        "near_chapters",
    );
    for key in ["volumes", "near_chapters"] {
        if let Some(value) = outline_object.get_mut(key) {
            coerce_value_to_array(value);
        }
    }

    if !outline_object.is_empty() {
        object.insert("outline".to_string(), Value::Object(outline_object));
    }
}

fn rename_alias(object: &mut Map<String, Value>, alias: &str, canonical: &str) {
    if object.contains_key(canonical) {
        object.remove(alias);
        return;
    }
    if let Some(value) = object.remove(alias) {
        object.insert(canonical.to_string(), value);
    }
}

fn move_scalar(
    from: &mut Map<String, Value>,
    to: &mut Map<String, Value>,
    source: &str,
    target: &str,
) {
    if to.contains_key(target) {
        from.remove(source);
        return;
    }
    if let Some(value) = from.remove(source) {
        to.insert(target.to_string(), value);
    }
}

fn move_array(
    from: &mut Map<String, Value>,
    to: &mut Map<String, Value>,
    source: &str,
    target: &str,
) {
    if to.contains_key(target) {
        from.remove(source);
        return;
    }
    if let Some(mut value) = from.remove(source) {
        coerce_value_to_array(&mut value);
        to.insert(target.to_string(), value);
    }
}

fn coerce_string_or_object_list(object: &mut Map<String, Value>, key: &str) {
    if let Some(value) = object.get_mut(key) {
        coerce_value_to_array(value);
    }
}

fn coerce_value_to_array(value: &mut Value) {
    match value {
        Value::Array(_) => {}
        Value::String(text) => {
            let items = split_loose_list(text)
                .into_iter()
                .map(Value::String)
                .collect::<Vec<_>>();
            *value = Value::Array(items);
        }
        Value::Null => {
            *value = Value::Array(Vec::new());
        }
        _ => {
            let item = std::mem::take(value);
            *value = Value::Array(vec![item]);
        }
    }
}

fn split_loose_list(text: &str) -> Vec<String> {
    text.split(['\n', '；', ';'])
        .flat_map(|part| part.split("、"))
        .map(|part| {
            part.trim()
                .trim_start_matches(|ch| matches!(ch, '-' | '*' | '•' | ' ' | '\t'))
                .trim()
        })
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_truncated_must_avoid_key_from_model_output() {
        let raw = r#"```json
{
  "title": {"canonical_title": "霓虹下的余烬", "rationale": "霓虹对应都市，余烬对应终局代价。"},
  "ending": {"desired_resolution": "主角牺牲力量修复城市。"},
  "protagonist_arc": "从追求力量到守住城市。",
  "world_imagery": "霓虹裂痕。",
  "main_causal_spine": "觉醒引发代价，代价逼近终局。",
  "characters": [{"canonical_name":"沈墨","role":"主角","desire":"找回记忆","fear":"失去现实","bottom_line":"不伤害无辜","arc_start":"追求力量","arc_end":"守住城市"}],
  "world_rules": "能力会消耗现实稳定性。",
  "style_rules": "冷峻紧凑。",
_avoid": ["传统升级体系"],
  "outline": {"near_chapters": [{"number":1,"goal":"发现裂痕","expected_turn":1}]}
}
```"#;

        let normalized = normalize_creation_contract_boundary(raw).expect("normalized");
        assert!(normalized.value.get("must_avoid").is_some());
    }

    #[test]
    fn repairs_corrupt_premise_key_and_drops_orphan_line() {
        let raw = r#"```json
{
  "title": {"canonical_title": "霓虹余烬", "rationale": "霓虹对应城市，余烬对应终局代价。"},
  "prem새": "主角通过频率感知现实边界。",
  "ending": {"desired_resolution": "主角献祭感知修复秩序。"},
  "protagonist_arc": "从寻亲者到守望者。",
  "world_imagery": "霓虹、信号塔。",
  "main_causal_spine": "发现异常，追查真相，献祭感知。",
  "characters": [{
    "canonical_name":"陆沉",
    "role":"主角",
    "desire":"寻找亲人",
    "fear":"秩序崩塌",
并保持现状",
    "bottom_line":"不伤害无辜",
    "arc_start":"孤立调查员",
    "arc_end":"城市守望者"
  }],
  "world_rules": "频率驱动现实。",
  "outline": {"near_chapters": [{"number":1,"goal":"信号干扰","expected_turn":1}]}
}
```"#;

        let normalized = normalize_creation_contract_boundary(raw).expect("normalized");
        assert_eq!(
            normalized.value.get("premise").and_then(Value::as_str),
            Some("主角通过频率感知现实边界。")
        );
    }

    #[test]
    fn repairs_near_miss_schema_keys_without_generating_story_content() {
        let raw = r#"```json
{
  "title": {"canonicaltitle": "烬余重塑", "rationale": "烬余来自世界余烬，重塑对应终局选择。"},
  "premile": "世界秩序由于灵力枯竭而支离破碎。",
  "ending": {"desiredresolution": "主角献出自身力量重塑世界规则。"},
  "characters": [{
    "canonicalname": "陆离",
    "role": "主角",
    "desire": "修复破碎世界",
    "fear": "失去最后的感知",
    "bottomtsline": "绝不为了生存而背叛秩序",
    "arcstart": "流浪者",
    "arcend": "秩序重塑者"
  }]
}
```"#;

        let normalized = normalize_creation_contract_boundary(raw).expect("normalized");
        assert_eq!(
            normalized.value.get("premise").and_then(Value::as_str),
            Some("世界秩序由于灵力枯竭而支离破碎。")
        );
        let character = normalized
            .value
            .get("characters")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_object)
            .expect("character");
        assert_eq!(
            character.get("bottom_line").and_then(Value::as_str),
            Some("绝不为了生存而背叛秩序")
        );
    }

    #[test]
    fn near_schema_repair_does_not_rename_nested_structured_fields() {
        let raw = r#"{
  "title": {"canonical_title": "潮汐回声"},
  "premise": "潜航员追查失踪信标。",
  "structured": {
    "payoff_matrix": [{
      "promise": "失踪信标仍在重复遇难者最后的呼叫",
      "payoff_target": "终局定位信标并揭开沉船真相",
      "status": "planned"
    }]
  }
}"#;

        let normalized = normalize_creation_contract_boundary(raw).expect("normalized");

        assert_eq!(
            normalized
                .value
                .pointer("/structured/payoff_matrix/0/promise")
                .and_then(Value::as_str),
            Some("失踪信标仍在重复遇难者最后的呼叫")
        );
        assert!(normalized
            .value
            .pointer("/structured/payoff_matrix/0/premise")
            .is_none());
    }

    #[test]
    fn repairs_schema_key_prefix_with_foreign_noise_without_generating_story_content() {
        let raw = r#"```json
{
  "title": {"canonicaltitle": "碎骨鸣钟", "rationale": "碎骨来自终局代价，鸣钟来自重塑秩序的动作。"},
  "premية": "世界秩序被虚无之潮侵蚀，修行者必须敲响古钟稳定现实。",
  "worldimagery": "破碎浮空岛、法则结晶、古老钟塔。",
worldrules": "法力是向世界规则借贷，过度使用会导致空间坍塌。",
  "characters": [{
    "canonicalname": "陆离",
    "role": "主角",
    "desire": "修复破碎世界",
    "fear": "失去最后的感知",
    "bottomline": "不伤害无辜凡人",
    "arcstart": "流浪者",
    "arcend": "秩序重塑者"
  }]
}
```"#;

        let normalized = normalize_creation_contract_boundary(raw).expect("normalized");
        assert_eq!(
            normalized.value.get("premise").and_then(Value::as_str),
            Some("世界秩序被虚无之潮侵蚀，修行者必须敲响古钟稳定现实。")
        );
        assert_eq!(
            normalized
                .value
                .get("world_rules")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(Value::as_str),
            Some("法力是向世界规则借贷，过度使用会导致空间坍塌。")
        );
    }

    #[test]
    fn repairs_stray_marker_before_characters_key() {
        let raw = r#"```json
{
  "title": {"canonical_title": "霓虹余烬", "rationale": "霓虹来自都市意象，余烬来自终局代价。"},
  "language": "zh-CN",
  "genre": "都市玄幻",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "premise": "灵能城市正在吞掉居民感官。",
  "ending": {"desired_resolution": "主角放弃力量修复城市。", "final_state": "城市回归平凡秩序。"},
  "protagonist_arc": "从追求力量到守护秩序。",
  "world_imagery": "霓虹、符文、机械义肢。",
  "main_causal_spine": "力量带来感官退化，主角最终选择归还力量。",
_ "characters": [{"canonical_name":"陆沉","role":"主角","desire":"找回记忆","fear":"失去现实","bottom_line":"不伤害无辜","arc_start":"追求力量","arc_end":"守护平凡"}],
  "world_rules": "灵能使用会消耗感官。",
  "outline": {"near_chapters": [{"number":1,"goal":"能力觉醒","expected_turn":1}]}
}
```"#;

        let normalized = normalize_creation_contract_boundary(raw).expect("normalized");
        let characters = normalized
            .value
            .get("characters")
            .and_then(Value::as_array)
            .expect("characters");
        assert_eq!(characters.len(), 1);
    }

    #[test]
    fn repairs_jsonish_contract_with_compact_keys_and_missing_key_quote() {
        let raw = r#"<|channel>thought
<channel|>```json
{
  "title": {
    "canonicaltitle": "霓虹灯下的敛火者",
    "candidates": "霓虹灯下的敛火者；余烬城守卫者",
    "rationale": "敛火来自能量收容制度，霓虹来自都市环境。",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "都市玄幻",
  "brief": "城市清理员卷入流火制度。",
  "targetunits": 50000,
  "chapterunittarget": 2500,
  "maxchaptersperturn": 1,
  "premise": "主角能感知并吸收流火残余。",
  "ending": {
    "desiredresolution": "主角重塑城市能量分配制度。",
    "finalstate": "城市秩序稳定。",
    "mustresolve": "流火核心是否被收容，主角是否摆脱控制。",
    "allowedopenquestions": "新制度是否会带来新冲突。"
  },
  "protagonistarc": "从底层清理员成长为规则改写者。",
  "worldimagery": "雨后街道、能量裂缝、回收装置。",
  "maincausalspine": "发现异常能量 -> 卷入收容冲突 -> 重塑规则。",
  "characters": [
    {
      "canonicalname": "陆沉",
      "role": "主角",
      "desire": "寻找失踪的父亲",
      "fear": "失去生活掌控",
      "bottomline": "不伤害无辜",
arcstart": "消极避世的清理员",
      "arcend": "掌控秩序的领袖"
    }
  ],
  "world_rules": "流火失控会破坏城市。",
  "outline": {
    "nearchapters": [
      {"number": 1, "goal": "发现流火异常", "expectedturn": "确认制度漏洞"}
    ]
  }
}
```"#;

        let normalized = normalize_creation_contract_boundary(raw).expect("normalized");
        assert_eq!(
            normalized
                .value
                .pointer("/title/canonical_title")
                .and_then(Value::as_str),
            Some("霓虹灯下的敛火者")
        );
        assert_eq!(
            normalized.value.get("target_units").and_then(Value::as_u64),
            Some(50000)
        );
        assert!(normalized
            .value
            .pointer("/ending/must_resolve")
            .and_then(Value::as_array)
            .is_some());
        assert_eq!(
            normalized
                .value
                .pointer("/characters/0/arc_start")
                .and_then(Value::as_str),
            Some("消极避世的清理员")
        );
        assert_eq!(
            normalized
                .value
                .pointer("/outline/near_chapters/0/expected_turn")
                .and_then(Value::as_str),
            Some("确认制度漏洞")
        );
    }

    #[test]
    fn extracts_later_balanced_contract_when_first_fence_is_not_json() {
        let raw = r#"这里先解释一下，不是合同：
```text
请看下面 JSON
```
现在给出合同：
```json
{
  "title": {"canonical_title": "剥骨令", "rationale": "剥骨令来自世界规则里的剥骨晋阶制度，终局主角公开再生法门废除此令。"},
  "language": "zh-CN",
  "genre": "异界玄幻",
  "brief": "剥骨晋阶世界。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "premise": "修士以剥骨换取力量。",
  "ending": {"desired_resolution": "主角废除剥骨令。"},
  "protagonist_arc": "从被剥夺者到新规则制定者。",
  "world_imagery": "骨令、灰塔、剥骨台。",
  "main_causal_spine": "被迫剥骨，发现制度真相，公开再生法门，废除剥骨令。",
  "characters": [{"canonical_name":"程砺舟","role":"主角","desire":"保住身体与尊严","fear":"被剥成空壳","bottom_line":"不牺牲无辜","arc_start":"被迫服从","arc_end":"废除旧令"}],
  "world_rules": ["晋阶必须剥骨"],
  "outline": {"near_chapters": [{"number":1,"goal":"主角第一次被迫剥骨","expected_turn":"主角发现剥骨台会吞掉记忆"}]}
}
```
附注：以上就是合同。"#;

        let normalized = normalize_creation_contract_boundary(raw).expect("normalized");
        assert_eq!(
            normalized
                .value
                .pointer("/title/canonical_title")
                .and_then(Value::as_str),
            Some("剥骨令")
        );
    }
}
