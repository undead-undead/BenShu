use crate::agent::message::{Message, Role};

use super::{collection_evidence, knowledge_delivery, source_selection, turn_state};

pub(super) fn query_requests_post_import_delivery(query: &str) -> bool {
    knowledge_delivery::query_requests_post_import_delivery(query)
}

pub(super) fn query_requests_prediction(query: &str) -> bool {
    knowledge_delivery::query_requests_prediction(query)
}

pub(super) fn query_requests_creative_synthesis(query: &str) -> bool {
    knowledge_delivery::query_requests_creative_synthesis(query)
}

pub(super) fn query_requests_file_artifact(query: &str) -> bool {
    knowledge_delivery::query_requests_file_artifact(query)
}

pub(super) fn synthesize_post_import_delivery(
    query: &str,
    messages: &[Message],
    prefers_chinese: bool,
) -> Option<String> {
    if !query_requests_post_import_delivery(query) {
        return None;
    }
    if query_requests_file_artifact(query) {
        return None;
    }

    let import_result = latest_successful_delegate_result_containing(
        messages,
        &["worker: knowledge", "executed_tool: knowledge_import_url"],
    );
    let researcher_result = latest_successful_lookup_delegate_result(messages)?;
    if let Some(gap) = collection_evidence::evidence_gap(query, &researcher_result) {
        return Some(collection_evidence::format_gap_blocker(
            query,
            gap,
            &researcher_result,
        ));
    }

    let import_summary = import_result
        .as_deref()
        .and_then(extract_knowledge_import_summary);
    let source_url = source_selection::explicit_source_url_in_result(&researcher_result)
        .or_else(|| source_selection::best_lookup_source_url_for_query(query, &researcher_result));
    let rows = knowledge_delivery::numeric_record_rows_from_text(&researcher_result);

    if query_requests_prediction(query) {
        return Some(synthesize_prediction_delivery(
            query,
            prefers_chinese,
            import_summary.as_deref(),
            source_url.as_deref(),
            &rows,
        ));
    }

    if query_requests_creative_synthesis(query) {
        let items = knowledge_delivery::ranked_metadata_items_from_result(&researcher_result);
        if !items.is_empty() {
            return Some(synthesize_creative_delivery(
                prefers_chinese,
                import_summary.as_deref(),
                source_url.as_deref(),
                &items,
            ));
        }
    }

    import_summary.map(|summary| {
        if prefers_chinese {
            format!("已完成知识库写入：{summary}。我也拿到了可用于后续分析的检索结果。")
        } else {
            format!(
                "The knowledge-base import completed successfully: {summary}. The retrieved result is available for follow-up analysis."
            )
        }
    })
}

pub(super) fn extract_knowledge_import_summary(result: &str) -> Option<String> {
    let collection = result
        .split("Imported web knowledge into collection '")
        .nth(1)?
        .split('\'')
        .next()?
        .trim();
    let path = result
        .split(" at path '")
        .nth(1)?
        .split('\'')
        .next()?
        .trim();
    let source_url = result.split("Source URL: ").nth(1)?.trim();

    if collection.is_empty() || path.is_empty() || source_url.is_empty() {
        return None;
    }

    Some(format!(
        "collection `{}`，path `{}`，source `{}`",
        collection, path, source_url
    ))
}

fn latest_successful_delegate_result_containing(
    messages: &[Message],
    required_markers: &[&str],
) -> Option<String> {
    turn_state::current_turn_messages(messages)
        .iter()
        .rev()
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }
            if !message
                .metadata
                .get("tool_name")
                .is_some_and(|name| name == "delegate")
            {
                return None;
            }
            if message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                return None;
            }

            let text = message.text();
            let lowered = text.to_ascii_lowercase();
            if required_markers
                .iter()
                .all(|marker| lowered.contains(&marker.to_ascii_lowercase()))
                && !tool_result_content_is_runtime_error(&text)
            {
                Some(text)
            } else {
                None
            }
        })
}

fn latest_successful_lookup_delegate_result(messages: &[Message]) -> Option<String> {
    turn_state::current_turn_messages(messages)
        .iter()
        .rev()
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }
            if !message
                .metadata
                .get("tool_name")
                .is_some_and(|name| name == "delegate")
            {
                return None;
            }
            if message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                return None;
            }

            let text = message.text();
            if tool_result_content_is_runtime_error(&text)
                || turn_state::tool_result_is_blocked(&text)
            {
                return None;
            }
            let lowered = text.to_ascii_lowercase();
            let lookup_worker =
                lowered.contains("worker: researcher") || lowered.contains("worker: browser");
            let lookup_tool = lowered.contains("executed_tool: web_search")
                || lowered.contains("executed_tool: web_fetch")
                || lowered.contains("executed_tool: browser_browse");
            (lookup_worker && lookup_tool).then_some(text)
        })
}

fn tool_result_content_is_runtime_error(content: &str) -> bool {
    let lowered = content.to_ascii_lowercase();
    if structured_tool_observation_not_found(&lowered) {
        return false;
    }
    lowered.contains("error executing tool")
        || lowered.contains("runtime tool error")
        || lowered.contains("tool execution error")
        || lowered.contains("execution timed out before a usable result")
        || lowered.contains("provider error")
        || lowered.contains("http error")
        || lowered.contains("\"success\":false")
        || lowered.contains("\"success\": false")
        || lowered.contains("missing_required")
        || lowered.contains("missing required")
        || lowered.contains(" is required")
        || lowered.contains(" required for ")
        || (lowered.contains("next_step_hint") && lowered.contains("example_shape"))
}

fn structured_tool_observation_not_found(lowered: &str) -> bool {
    (lowered.contains("\"error_kind\":\"not_found\"")
        || lowered.contains("\"error_kind\": \"not_found\"")
        || lowered.contains("\"error_kind\":\"chapter_not_found\"")
        || lowered.contains("\"error_kind\": \"chapter_not_found\""))
        && !lowered.contains("missing_required")
        && !lowered.contains("missing required")
        && !lowered.contains(" is required")
        && !lowered.contains(" required for ")
}

fn synthesize_prediction_delivery(
    _query: &str,
    prefers_chinese: bool,
    import_summary: Option<&str>,
    source_url: Option<&str>,
    rows: &[(String, String, Vec<u8>)],
) -> String {
    if rows.len() >= 5 {
        let first = rows.last();
        let latest = rows.first();
        let date_range = match (first, latest) {
            (Some((start_date, _, _)), Some((end_date, _, _))) => {
                format!("{start_date} 至 {end_date}")
            }
            _ => "已抓取区间".to_string(),
        };
        let source_line = source_url
            .map(|url| format!("\n来源：{url}"))
            .unwrap_or_default();
        let import_line = import_summary
            .map(|summary| format!("\n知识库：已写入 {summary}"))
            .unwrap_or_else(|| "\n知识库：已完成写入。".to_string());

        return if prefers_chinese {
            format!(
                "已完成抓取、入库和初步推断。{import_line}{source_line}\n\n我从已抓取内容中识别到 {count} 条结构化数字记录，覆盖 {date_range}。这些记录可以支持后续统计、趋势对比或假设分析，但 reasoner 不在这里内置任何特定领域的预测模板。若要继续生成结论，应由具备相应工具和质量合同的 worker 基于知识库记录继续推理。",
                count = rows.len()
            )
        } else {
            format!(
                "The lookup, knowledge import, and initial inference are complete.{import_line}{source_line}\n\nI identified {count} structured numeric records covering {date_range}. These records can support follow-up statistics, trend comparison, or hypothesis analysis, but the reasoner does not embed a domain-specific prediction template here. A worker with the right tools and quality contract should continue from the knowledge-base records.",
                count = rows.len()
            )
        };
    }

    if prefers_chinese {
        let source_line = source_url
            .map(|url| format!("来源：{url}\n"))
            .unwrap_or_default();
        let import_line = import_summary
            .map(|summary| format!("知识库：已写入 {summary}\n"))
            .unwrap_or_else(|| "知识库：已完成写入。\n".to_string());
        format!(
            "{import_line}{source_line}但我没有从已抓取结果中识别到足够稳定的结构化记录，所以不能可靠完成请求的预测/推断。下一步应补充更完整的原始记录，或让合适的 worker 先把来源转换为结构化数据后再继续。"
        )
    } else {
        let source_line = source_url
            .map(|url| format!("Source: {url}\n"))
            .unwrap_or_default();
        let import_line = import_summary
            .map(|summary| format!("Knowledge base: imported {summary}\n"))
            .unwrap_or_else(|| "Knowledge base: import completed.\n".to_string());
        format!(
            "{import_line}{source_line}I could not identify enough stable structured numeric records in the fetched result, so I cannot safely produce the requested prediction yet."
        )
    }
}

fn synthesize_creative_delivery(
    prefers_chinese: bool,
    import_summary: Option<&str>,
    source_url: Option<&str>,
    items: &[(String, String, String)],
) -> String {
    let import_line = import_summary
        .map(|summary| format!("知识库：已写入 {summary}"))
        .unwrap_or_else(|| "知识库：已完成写入。".to_string());
    let source_line = source_url
        .map(|url| format!("来源：{url}"))
        .unwrap_or_else(|| "来源：已来自 researcher 的公开检索结果。".to_string());
    let item_lines = items
        .iter()
        .enumerate()
        .map(|(index, (title, metadata, source))| {
            let source = if source.is_empty() {
                source_line.as_str()
            } else {
                source.as_str()
            };
            format!("{}. {}：{} | {}", index + 1, title, metadata, source)
        })
        .collect::<Vec<_>>()
        .join("\n");

    if prefers_chinese {
        return format!(
            "已完成搜索和知识库写入。\n{import_line}\n{source_line}\n\n已确认的来源样本：\n{item_lines}\n\n下一步应由合适的产物 worker 基于这些材料继续生成用户请求的原创或分析产物；reasoner 不内置标题、角色、结构模板或领域示例，避免把固定样例当成真实推理结果。"
        );
    }

    format!(
        "The lookup and knowledge import are complete.\n{import_line}\n{source_line}\n\nConfirmed source samples:\n{item_lines}\n\nNext, an appropriate artifact worker should generate the requested original or analytical artifact from these materials. The reasoner does not embed fixed titles, characters, structure templates, or domain examples."
    )
}
