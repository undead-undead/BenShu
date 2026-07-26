use crate::skills::tool::ToolSet;
use tracing::info;

pub(super) fn normalize_query_arguments_for_tool(
    tool_name: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let mut object = match args {
        serde_json::Value::Object(object) => object,
        other => return other,
    };

    object = normalize_colon_embedded_argument_keys(object);

    if matches!(tool_name, "web_search" | "tool_search") && !object.contains_key("query") {
        let query = object
            .get("query")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| {
                object
                    .get("queries")
                    .and_then(|value| value.as_array())
                    .and_then(|queries| queries.iter().find_map(|value| value.as_str()))
                    .map(str::to_string)
            });

        if let Some(query) = query {
            object.insert("query".to_string(), serde_json::Value::String(query));
        }
    }

    if matches!(
        tool_name,
        "read_file"
            | "write_file"
            | "edit_file"
            | "office_parse"
            | "pdf_parse"
            | "text_extract"
            | "document_understand"
            | "transcribe_audio"
    ) && !object.contains_key("path")
    {
        for alias in ["file_path", "filepath", "file", "image_path", "audio_path"] {
            if let Some(value) = object.get(alias).cloned() {
                object.insert("path".to_string(), value);
                break;
            }
        }
    }

    serde_json::Value::Object(object)
}

fn normalize_colon_embedded_argument_keys(
    object: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut normalized = serde_json::Map::new();

    for (key, value) in object {
        if value.is_null() {
            if let Some((argument_key, argument_value)) = split_colon_embedded_argument(&key) {
                normalized.entry(argument_key).or_insert(argument_value);
                continue;
            }
        }
        normalized.insert(key, value);
    }

    normalized
}

fn split_colon_embedded_argument(key: &str) -> Option<(String, serde_json::Value)> {
    let (raw_key, raw_value) = key.split_once(':')?;
    let argument_key = raw_key.trim().trim_matches('"').trim_matches('\'');
    if argument_key.is_empty()
        || !argument_key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return None;
    }

    let decoded_value = decode_pseudo_tool_scalar(raw_value.trim());
    if decoded_value.is_empty() {
        return None;
    }

    Some((
        argument_key.to_string(),
        parse_argument_value(&decoded_value),
    ))
}

fn decode_pseudo_tool_scalar(value: &str) -> String {
    let decoded = value
        .replace("<|\\\"|>", "\"")
        .replace("<|\"|>", "\"")
        .replace("<|'|>", "'");
    decoded
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

pub(super) fn executable_name(tools: &ToolSet, name: &str) -> String {
    if tools.contains(name) {
        return name.to_string();
    }

    if let Some(suffix) = name.rsplit('.').next() {
        if suffix != name && tools.contains(suffix) {
            return suffix.to_string();
        }
    }

    match name {
        "fs.read_file" | "filesystem.read_file" | "read" if tools.contains("read_file") => {
            "read_file".to_string()
        }
        "fs.write_file" | "filesystem.write_file" | "write" if tools.contains("write_file") => {
            "write_file".to_string()
        }
        "fs.list_directory" | "fs.list_dir" | "filesystem.list_directory" | "list_dir"
            if tools.contains("list_directory") =>
        {
            "list_directory".to_string()
        }
        "office.parse" | "office_parse_file" if tools.contains("office_parse") => {
            "office_parse".to_string()
        }
        "pdf.parse" | "pdf_parse_file" if tools.contains("pdf_parse") => "pdf_parse".to_string(),
        "ocr.ocr" | "ocr.extract" | "ocr_extract" if tools.contains("text_extract") => {
            "text_extract".to_string()
        }
        "crypto" | "crypto.hash" | "crypto.hash_text" | "cipher.hash" | "cipher.hash_text"
            if tools.contains("cipher") =>
        {
            "cipher".to_string()
        }
        "data"
        | "data.stats"
        | "data.query"
        | "data.read_csv"
        | "data.write_csv"
        | "data.transform"
        | "data_transform.stats"
        | "data_transform.query"
        | "data_transform.read_csv"
        | "data_transform.write_csv"
        | "data_transform.transform"
            if tools.contains("data_transform") =>
        {
            "data_transform".to_string()
        }
        "runtime"
        | "runtime.catalog"
        | "runtime.inspect"
        | "runtime.ensure"
        | "runtime_surface.catalog"
        | "runtime_surface.inspect"
        | "runtime_surface.ensure"
            if tools.contains("runtime_surface") =>
        {
            "runtime_surface".to_string()
        }
        "document"
        | "document.info"
        | "document.analyze"
        | "document_understand.info"
        | "document_understand.analyze"
            if tools.contains("document_understand") =>
        {
            "document_understand".to_string()
        }
        "ocr" | "ocr.recognize" | "text_extract.recognize" if tools.contains("text_extract") => {
            "text_extract".to_string()
        }
        "scheduler" | "scheduler.list" | "scheduler.schedule" | "scheduler.cancel"
        | "cron.list" | "cron.schedule" | "cron.cancel"
            if tools.contains("cron") =>
        {
            "cron".to_string()
        }
        "desktop"
        | "desktop.list_windows"
        | "desktop.get_active"
        | "desktop_sense.list_windows"
        | "desktop_sense.get_active"
            if tools.contains("desktop_sense") =>
        {
            "desktop_sense".to_string()
        }
        "system" | "system.status" | "system.monitor" if tools.contains("system_monitor") => {
            "system_monitor".to_string()
        }
        _ => name.to_string(),
    }
}

pub(super) fn normalize_local_call(
    tools: &ToolSet,
    name: String,
    args: serde_json::Value,
) -> (String, serde_json::Value) {
    if name == "BENSHU_SPECIALIST_SELECTION" {
        if let Some(selected_tool) = args
            .get("tool")
            .and_then(|value| value.as_str())
            .map(str::to_string)
        {
            if tools.contains(&selected_tool) {
                let normalized_args = normalize_query_arguments_for_tool(&selected_tool, args);
                info!(
                    "Reasoner: normalized specialist-selection pseudo tool call to executable tool '{}'.",
                    selected_tool
                );
                return (selected_tool, normalized_args);
            }
        }
    }

    if !tools.contains(&name) && name == "tool_search" && tools.contains("web_search") {
        let normalized_args = normalize_query_arguments_for_tool("web_search", args);
        info!(
            "Reasoner: normalized unavailable tool_search pseudo call to web_search for specialist execution."
        );
        return ("web_search".to_string(), normalized_args);
    }

    if !tools.contains(&name)
        && matches!(name.as_str(), "google_search" | "browser_search")
        && tools.contains("web_search")
    {
        let normalized_args = normalize_query_arguments_for_tool("web_search", args);
        info!(
            "Reasoner: normalized pseudo browser search tool '{}' to executable tool 'web_search'.",
            name
        );
        return ("web_search".to_string(), normalized_args);
    }

    let executable_name = executable_name(tools, &name);
    let normalized_args = normalize_query_arguments_for_tool(&executable_name, args);
    let normalized_args = ensure_action(&executable_name, &name, normalized_args);
    (executable_name, normalized_args)
}

pub(super) fn ensure_action(
    executable_name: &str,
    source_name: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let Some(action) = action_hint(source_name, executable_name) else {
        return args;
    };

    let serde_json::Value::Object(mut object) = args else {
        return args;
    };

    object
        .entry("action".to_string())
        .or_insert_with(|| serde_json::Value::String(action.to_string()));
    serde_json::Value::Object(object)
}

pub(super) fn extract_inline_calls(text: &str) -> Vec<(String, serde_json::Value)> {
    const OPEN: &str = "<|tool_call>";
    const CLOSE: &str = "<tool_call|>";

    let mut calls = Vec::new();
    let mut remaining = text;

    while let Some(start_idx) = remaining.find(OPEN) {
        let after_open = &remaining[start_idx + OPEN.len()..];
        let Some(end_idx) = after_open.find(CLOSE) else {
            break;
        };

        let body = after_open[..end_idx].trim();
        if let Some((name, args)) = parse_inline_call_body(body) {
            calls.push((name, args));
        }

        remaining = &after_open[end_idx + CLOSE.len()..];
    }

    calls.extend(extract_assistant_tool_request_calls(text));
    calls
}

fn extract_assistant_tool_request_calls(text: &str) -> Vec<(String, serde_json::Value)> {
    let mut calls = Vec::new();
    for line in text.lines() {
        let lowered = line.to_ascii_lowercase();
        let Some(marker_start) = lowered.find("[assistant tool request]") else {
            continue;
        };
        let after_marker = &line[marker_start + "[Assistant tool request]".len()..].trim();
        if let Some(call) = parse_parenthesized_tool_request(after_marker) {
            calls.push(call);
        }
    }
    calls
}

fn parse_parenthesized_tool_request(value: &str) -> Option<(String, serde_json::Value)> {
    let open = value.find('(')?;
    let close = value.rfind(')')?;
    if close <= open {
        return None;
    }
    let name = value[..open].trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-')
    {
        return None;
    }
    let args = parse_parenthesized_args(&value[open..=close])?;
    Some((name.to_string(), args))
}

fn action_hint(name: &str, executable_name: &str) -> Option<&'static str> {
    let suffix = name.rsplit('.').next().unwrap_or(name);
    match (executable_name, suffix) {
        ("cipher", "hash" | "hash_text") => Some("hash_text"),
        ("data_transform", "stats") => Some("stats"),
        ("data_transform", "query") => Some("query"),
        ("data_transform", "read_csv") => Some("read_csv"),
        ("data_transform", "write_csv") => Some("write_csv"),
        ("data_transform", "transform") => Some("transform"),
        ("runtime_surface", "runtime" | "catalog") => Some("catalog"),
        ("runtime_surface", "inspect") => Some("inspect"),
        ("runtime_surface", "ensure") => Some("ensure"),
        ("document_understand", "document" | "info") => Some("info"),
        ("document_understand", "analyze") => Some("analyze"),
        ("text_extract", "ocr" | "recognize") => Some("recognize"),
        ("cron", "scheduler" | "list") => Some("list"),
        ("cron", "schedule") => Some("schedule"),
        ("cron", "cancel") => Some("cancel"),
        ("desktop_sense", "desktop" | "list_windows") => Some("list_windows"),
        ("desktop_sense", "get_active") => Some("get_active"),
        _ => None,
    }
}

fn parse_inline_call_body(body: &str) -> Option<(String, serde_json::Value)> {
    if let Some(parsed) = parse_structured_call_body(body) {
        return Some(parsed);
    }

    let stripped = body.strip_prefix("call:")?.trim();
    let args_start = stripped.find('{').or_else(|| stripped.find('('))?;
    let args_end = if stripped.as_bytes().get(args_start) == Some(&b'(') {
        stripped.rfind(')')?
    } else {
        stripped.len()
    };
    let tool_head = stripped[..args_start].trim().trim_end_matches(':');
    let tool_name = tool_head.rsplit(':').next()?.trim();
    if tool_name.is_empty() {
        return None;
    }

    let args_literal = stripped[args_start..args_end].trim();
    let arguments = if args_literal.starts_with('(') {
        parse_parenthesized_args(args_literal)?
    } else {
        serde_yaml_ng::from_str::<serde_json::Value>(args_literal)
            .or_else(|_| serde_json::from_str::<serde_json::Value>(args_literal))
            .ok()?
    };

    Some((tool_name.to_string(), arguments))
}

fn parse_structured_call_body(body: &str) -> Option<(String, serde_json::Value)> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(args_start) = trimmed.find('{') {
        let head = trimmed[..args_start].trim();
        if !head.is_empty() && !head.contains(':') {
            let tool_name = head
                .split_whitespace()
                .last()
                .map(str::trim)
                .filter(|name| !name.is_empty())?;
            let arguments = serde_yaml_ng::from_str::<serde_json::Value>(&trimmed[args_start..])
                .or_else(|_| serde_json::from_str::<serde_json::Value>(&trimmed[args_start..]))
                .ok()?;
            return Some((tool_name.to_string(), arguments));
        }
    }

    let value = serde_yaml_ng::from_str::<serde_json::Value>(trimmed)
        .or_else(|_| serde_json::from_str::<serde_json::Value>(trimmed))
        .ok()?;
    parse_tool_value(value)
}

fn parse_tool_value(value: serde_json::Value) -> Option<(String, serde_json::Value)> {
    let serde_json::Value::Object(mut object) = value else {
        return None;
    };

    if let Some(function) = object.remove("function") {
        if let serde_json::Value::Object(mut function_object) = function {
            let name = function_object
                .remove("name")
                .and_then(|value| value.as_str().map(str::to_string))?;
            let arguments = function_object
                .remove("arguments")
                .or_else(|| function_object.remove("args"))
                .or_else(|| function_object.remove("parameters"))
                .map(parse_tool_arguments)
                .unwrap_or_else(|| serde_json::Value::Object(function_object));
            return Some((name, arguments));
        }
    }

    let name = object
        .remove("name")
        .or_else(|| object.remove("tool"))
        .or_else(|| object.remove("tool_name"))
        .or_else(|| object.remove("function_name"))
        .and_then(|value| value.as_str().map(str::to_string))?;
    let arguments = object
        .remove("arguments")
        .or_else(|| object.remove("args"))
        .or_else(|| object.remove("parameters"))
        .map(parse_tool_arguments)
        .unwrap_or_else(|| serde_json::Value::Object(object));
    Some((name, arguments))
}

fn parse_tool_arguments(value: serde_json::Value) -> serde_json::Value {
    if let Some(text) = value.as_str() {
        return serde_yaml_ng::from_str::<serde_json::Value>(text)
            .or_else(|_| serde_json::from_str::<serde_json::Value>(text))
            .unwrap_or_else(|_| serde_json::Value::String(text.to_string()));
    }
    value
}

fn parse_parenthesized_args(args_literal: &str) -> Option<serde_json::Value> {
    let inner = args_literal.strip_prefix('(')?.strip_suffix(')')?.trim();
    let mut object = serde_json::Map::new();
    if inner.is_empty() {
        return Some(serde_json::Value::Object(object));
    }

    for pair in split_argument_pairs(inner) {
        let (key, value) = pair.split_once('=')?;
        let key = key.trim().trim_matches('"').trim_matches('\'');
        if key.is_empty() {
            return None;
        }
        object.insert(key.to_string(), parse_argument_value(value.trim()));
    }

    Some(serde_json::Value::Object(object))
}

fn split_argument_pairs(inner: &str) -> Vec<String> {
    let mut pairs = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for ch in inner.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            current.push(ch);
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            current.push(ch);
            if ch == active_quote {
                quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            current.push(ch);
            continue;
        }
        if ch == ',' {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                pairs.push(trimmed.to_string());
            }
            current.clear();
            continue;
        }
        current.push(ch);
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        pairs.push(trimmed.to_string());
    }
    pairs
}

fn parse_argument_value(value: &str) -> serde_json::Value {
    let trimmed = value.trim();
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return parsed;
    }
    if let Ok(parsed) = serde_yaml_ng::from_str::<serde_json::Value>(trimmed) {
        return parsed;
    }
    serde_json::Value::String(trimmed.trim_matches('"').trim_matches('\'').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_query_arguments_recovers_colon_embedded_pseudo_keys() {
        let normalized = normalize_query_arguments_for_tool(
            "novel_studio",
            json!({
                "action:<|\"|>revise_chapter<|\"|>": null,
                "chapter_number:1": null,
                "project_path:<|\"|>/tmp/project<|\"|>": null,
                "feedback:<|\"|>补充摘要和连续性更新<|\"|>": null
            }),
        );

        assert_eq!(normalized["action"], "revise_chapter");
        assert_eq!(normalized["chapter_number"], 1);
        assert_eq!(normalized["project_path"], "/tmp/project");
        assert_eq!(normalized["feedback"], "补充摘要和连续性更新");
    }
}
