pub(crate) fn extract_json(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed.to_string());
    }
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after
            .strip_prefix("json")
            .or_else(|| after.strip_prefix("JSON"))
            .unwrap_or(after)
            .trim_start();
        if let Some(end) = after.find("```") {
            let candidate = after[..end].trim();
            if candidate.starts_with('{') {
                return Some(candidate.to_string());
            }
        }
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    (end > start).then(|| trimmed[start..=end].to_string())
}

pub(crate) fn jsonish_string_field(raw: &str, key: &str, next_keys: &[&str]) -> Option<String> {
    let key_pos = raw.find(&format!("\"{key}\""))?;
    let after_key = &raw[key_pos + key.len() + 2..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    let mut chars = after_colon.char_indices();
    let (_, first) = chars.next()?;
    if first != '"' {
        return None;
    }
    let body_start = 1;
    let mut escaped = false;
    for (idx, ch) in after_colon[body_start..].char_indices() {
        let absolute = body_start + idx;
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch != '"' {
            continue;
        }
        let rest = after_colon[absolute + ch.len_utf8()..].trim_start();
        if rest.starts_with('}') || rest.starts_with(']') || rest.is_empty() {
            return Some(unescape_jsonish_string(&after_colon[body_start..absolute]));
        }
        let Some(after_comma) = rest.strip_prefix(',') else {
            continue;
        };
        let after_comma = after_comma.trim_start();
        if looks_like_jsonish_field_boundary(after_comma, next_keys) || after_comma.starts_with('}')
        {
            return Some(unescape_jsonish_string(&after_colon[body_start..absolute]));
        }
    }
    if next_keys.is_empty() || key == "content" {
        let recovered = recover_unclosed_jsonish_string(&after_colon[body_start..]);
        if !recovered.trim().is_empty() {
            return Some(recovered);
        }
    }
    None
}

fn recover_unclosed_jsonish_string(value: &str) -> String {
    let mut body = value.trim();
    body = body.trim_end_matches('`').trim_end();
    if body.ends_with('}') {
        body = body[..body.len() - 1].trim_end();
    }
    if body.ends_with('"') {
        body = body[..body.len() - 1].trim_end();
    }
    unescape_jsonish_string(body)
}

fn looks_like_jsonish_field_boundary(value: &str, next_keys: &[&str]) -> bool {
    if next_keys.iter().any(|next_key| {
        value.starts_with(&format!("\"{next_key}\""))
            || value.starts_with(&format!("{next_key}:"))
            || value.starts_with(&format!("_{next_key}:"))
    }) {
        return true;
    }
    let field_head = value
        .split(['\n', '\r'])
        .next()
        .unwrap_or_default()
        .trim_start();
    let Some(colon_index) = field_head.find(':') else {
        return false;
    };
    let candidate = field_head[..colon_index]
        .trim()
        .trim_matches('"')
        .trim_start_matches('_');
    !candidate.is_empty()
        && candidate
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub(crate) fn jsonish_string_array_field(raw: &str, key: &str) -> Vec<String> {
    let Some(key_pos) = raw.find(&format!("\"{key}\"")) else {
        return Vec::new();
    };
    let after_key = &raw[key_pos + key.len() + 2..];
    let Some(colon_pos) = after_key.find(':') else {
        return Vec::new();
    };
    let after_colon = after_key[colon_pos + 1..].trim_start();
    let Some(array_start) = after_colon.find('[') else {
        return Vec::new();
    };
    let after_array_start = &after_colon[array_start + 1..];
    let Some(array_end) = after_array_start.find(']') else {
        return Vec::new();
    };
    let array = &after_array_start[..array_end];
    let object_items = jsonish_object_array_items(array);
    if !object_items.is_empty() {
        return object_items;
    }
    jsonish_string_items(array)
}

fn jsonish_string_items(array: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in array.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if in_string && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            if in_string {
                let item = current.trim();
                if !item.is_empty() {
                    items.push(item.to_string());
                }
                current.clear();
                in_string = false;
            } else {
                in_string = true;
            }
            continue;
        }
        if in_string {
            current.push(ch);
        }
    }
    items
}

fn jsonish_object_array_items(array: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in array.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_string && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(idx);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = start.take() {
                        let object = &array[start..=idx];
                        if let Some(rendered) = render_jsonish_object(object) {
                            items.push(rendered);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    items
}

fn render_jsonish_object(object: &str) -> Option<String> {
    let mut pairs = Vec::new();
    let mut rest = object;
    while let Some(key_start) = rest.find('"') {
        let after_key_start = &rest[key_start + 1..];
        let Some(key_end) = after_key_start.find('"') else {
            break;
        };
        let key = after_key_start[..key_end].trim();
        let after_key = &after_key_start[key_end + 1..];
        let Some(colon) = after_key.find(':') else {
            break;
        };
        let after_colon = after_key[colon + 1..].trim_start();
        if let Some(value_body) = after_colon.strip_prefix('"') {
            let Some(value_end) = value_body.find('"') else {
                break;
            };
            let value = value_body[..value_end].trim();
            if !key.is_empty() && !value.is_empty() {
                pairs.push(format!("{key}: {value}"));
            }
            rest = &value_body[value_end + 1..];
        } else {
            rest = after_colon;
        }
    }
    (!pairs.is_empty()).then(|| pairs.join("; "))
}

fn unescape_jsonish_string(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
        .trim()
        .to_string()
}
