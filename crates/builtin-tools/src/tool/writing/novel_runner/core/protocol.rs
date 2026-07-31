use std::sync::Arc;

use benshu_brain::agent::multi_agent::{
    MultiAgent, TextGenerationLimits, TextGenerationProgressSink,
};

use super::{
    extract_json, parse_chapter_execution_package, parse_draft_output, parse_draft_stream_protocol,
    ChapterExecutionPackage, DraftOutput, FinalChapterObservation,
};
use crate::tool::writing::text_sanitizer::{sanitize_common_surface_report, WritingSanitizeStage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseProvenance {
    StreamProtocol,
    TruncatedStream,
    ExactJson,
    RecoveredJson,
    Freeform,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedOutput<T> {
    pub(crate) value: T,
    pub(crate) provenance: ParseProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevisionMode {
    LocalRepair,
    FullRewrite,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CharacterAuthority {
    pub(crate) protagonist: Option<String>,
    pub(crate) canonical_names: Vec<String>,
}

impl CharacterAuthority {
    pub(crate) fn from_names(protagonist: Option<String>, canonical_names: Vec<String>) -> Self {
        let protagonist = protagonist.and_then(|name| canonical_character_name(&name));
        let mut ordered_names = Vec::new();
        if let Some(name) = protagonist.as_ref() {
            ordered_names.push(name.clone());
        }
        for name in canonical_names
            .into_iter()
            .filter_map(|name| canonical_character_name(&name))
        {
            if !ordered_names.contains(&name) {
                ordered_names.push(name);
            }
        }
        Self {
            protagonist,
            canonical_names: ordered_names,
        }
    }

    pub(crate) fn from_context(context: &serde_json::Value) -> Self {
        let primary = context
            .pointer("/continuity_anchors/primary_characters")
            .or_else(|| {
                context.pointer("/authority/working_context/continuity_anchors/primary_characters")
            })
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .find_map(canonical_character_name);
        let names = context
            .pointer("/continuity_anchors/characters")
            .or_else(|| context.pointer("/authority/working_context/continuity_anchors/characters"))
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .filter_map(canonical_character_name)
            .collect::<Vec<_>>();
        Self::from_names(primary, names)
    }
}

impl ParseProvenance {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::StreamProtocol => "stream_protocol",
            Self::TruncatedStream => "truncated_stream",
            Self::ExactJson => "exact_json",
            Self::RecoveredJson => "recovered_json",
            Self::Freeform => "freeform",
        }
    }
}

fn canonical_character_name(value: &str) -> Option<String> {
    let first_field = value
        .trim()
        .split([';', '；'])
        .next()
        .unwrap_or_default()
        .trim();
    let candidate = ["name:", "Name:", "name：", "Name：", "姓名:", "姓名："]
        .iter()
        .find_map(|prefix| first_field.strip_prefix(prefix))
        .unwrap_or(first_field)
        .trim()
        .trim_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, '-' | '—' | '，' | ',' | '。' | '.')
        });
    (!candidate.is_empty()).then(|| candidate.to_string())
}

pub(crate) async fn generate_draft(
    agent: &Arc<dyn MultiAgent>,
    prompt: &str,
    limits: TextGenerationLimits,
    progress: Option<TextGenerationProgressSink>,
    chapter_number: usize,
    language: &str,
) -> anyhow::Result<ParsedOutput<DraftOutput>> {
    let raw = agent
        .generate_text_only_with_limits(prompt, limits, progress)
        .await?;
    let raw = sanitize_common_surface_report(&raw, WritingSanitizeStage::ModelOutput).text;
    let value = parse_draft_output(&raw, chapter_number, language);
    let provenance = if parse_draft_stream_protocol(&raw).is_some() {
        if value.degraded {
            ParseProvenance::TruncatedStream
        } else {
            ParseProvenance::StreamProtocol
        }
    } else {
        parse_provenance(&raw, value.degraded)
    };
    Ok(ParsedOutput { value, provenance })
}

pub(crate) async fn generate_execution_package(
    agent: &Arc<dyn MultiAgent>,
    prompt: &str,
    max_tokens: Option<u64>,
    language: &str,
) -> anyhow::Result<ParsedOutput<ChapterExecutionPackage>> {
    let raw = agent
        .generate_text_only_with_max_tokens(prompt, max_tokens)
        .await?;
    let raw = sanitize_common_surface_report(&raw, WritingSanitizeStage::ModelOutput).text;
    let value = parse_chapter_execution_package(&raw, language).map_err(anyhow::Error::msg)?;
    let provenance = parse_provenance(&raw, value.degraded);
    Ok(ParsedOutput { value, provenance })
}

pub(crate) fn parse_final_chapter_observation(
    raw: &str,
    content: &str,
) -> anyhow::Result<FinalChapterObservation> {
    let json = extract_json(raw)
        .ok_or_else(|| anyhow::anyhow!("final chapter observer did not return a JSON object"))?;
    let mut value = serde_json::from_str::<serde_json::Value>(&json)
        .map_err(|error| anyhow::anyhow!("invalid final chapter observation: {error}"))?;
    normalize_final_observation_shape(&mut value, content);
    let mut observation = serde_json::from_value::<FinalChapterObservation>(value)
        .map_err(|error| anyhow::anyhow!("invalid final chapter observation: {error}"))?;
    observation.current_state = observation.current_state.trim().to_string();
    observation.pending_hooks = observation.pending_hooks.trim().to_string();
    observation.chapter_summary = observation.chapter_summary.trim().to_string();
    observation.future_boundary_evidence = observation.future_boundary_evidence.trim().to_string();
    observation.continuity_updates = clean_observation_items(observation.continuity_updates);
    observation.resolved_hooks = clean_observation_items(observation.resolved_hooks);
    observation.state_changes.retain(|change| {
        !change.entity_id.trim().is_empty()
            && !change.value.trim().is_empty()
            && !change.evidence.excerpt.trim().is_empty()
    });
    if observation.current_state.is_empty() || observation.chapter_summary.is_empty() {
        anyhow::bail!("final chapter observation is missing current_state or chapter_summary");
    }
    Ok(observation)
}

fn normalize_final_observation_shape(value: &mut serde_json::Value, content: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for field in [
        "current_state",
        "pending_hooks",
        "chapter_summary",
        "future_boundary_evidence",
    ] {
        if let Some(field_value) = object.get_mut(field) {
            if !field_value.is_string() {
                *field_value = serde_json::Value::String(observation_text_from_value(field_value));
            }
        }
    }
    for field in ["continuity_updates", "resolved_hooks"] {
        let Some(field_value) = object.get_mut(field) else {
            continue;
        };
        let items = match &*field_value {
            serde_json::Value::Array(values) => values
                .iter()
                .map(observation_text_from_value)
                .filter(|value| !value.is_empty())
                .map(serde_json::Value::String)
                .collect(),
            serde_json::Value::Null => Vec::new(),
            value => {
                let text = observation_text_from_value(value);
                (!text.is_empty())
                    .then(|| vec![serde_json::Value::String(text)])
                    .unwrap_or_default()
            }
        };
        *field_value = serde_json::Value::Array(items);
    }

    let future_boundary_ids = object
        .get("future_boundary_sentence_ids")
        .map(observation_sentence_ids)
        .unwrap_or_default();
    object.insert(
        "future_boundary_evidence".to_string(),
        serde_json::Value::String(
            resolve_observation_sentence_ids(content, &future_boundary_ids).unwrap_or_default(),
        ),
    );
    object.remove("future_boundary_sentence_ids");

    if object
        .get("state_changes")
        .is_some_and(|value| value.is_object())
    {
        let update = object.remove("state_changes").unwrap_or_default();
        object.insert(
            "state_changes".to_string(),
            serde_json::Value::Array(vec![update]),
        );
    }
    let Some(updates) = object
        .get_mut("state_changes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for update in updates {
        let ids = update
            .get("evidence_sentence_ids")
            .or_else(|| update.pointer("/evidence/sentence_ids"))
            .map(observation_sentence_ids)
            .unwrap_or_default();
        let excerpt = resolve_observation_sentence_ids(content, &ids).unwrap_or_default();
        update["value"] = serde_json::Value::String(excerpt.clone());
        update["evidence"] = serde_json::json!({"excerpt": excerpt});
        update["authority_excerpt"] = serde_json::Value::String(String::new());
        if let Some(number) = update
            .get("defer_until_chapter")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|number| !number.is_empty())
            .and_then(|number| number.parse::<u64>().ok())
        {
            update["defer_until_chapter"] = serde_json::json!(number);
        }
        update
            .as_object_mut()
            .map(|fields| fields.remove("evidence_sentence_ids"));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IndexedBodySentence {
    id: usize,
    paragraph: usize,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FinalBodyEvidenceSpan {
    pub(crate) start_char: usize,
    pub(crate) end_char: usize,
    pub(crate) excerpt: String,
}

pub(crate) fn render_final_body_sentence_index(content: &str) -> String {
    let mut rendered = String::new();
    let mut previous_paragraph = None;
    for sentence in indexed_body_sentences(content) {
        if !rendered.is_empty() {
            rendered.push('\n');
            if previous_paragraph.is_some_and(|paragraph| paragraph != sentence.paragraph) {
                rendered.push('\n');
            }
        }
        rendered.push_str(&format!(
            "[S{:04}] {}",
            sentence.id,
            &content[sentence.start..sentence.end]
        ));
        previous_paragraph = Some(sentence.paragraph);
    }
    rendered
}

/// Returns the same bounded sentence windows exposed to the final-body
/// observer. Deterministic settlement recovery must consume these indexed
/// windows instead of maintaining a second sentence splitter.
pub(crate) fn final_body_evidence_spans(content: &str) -> Vec<FinalBodyEvidenceSpan> {
    let sentences = indexed_body_sentences(content);
    let mut spans = Vec::new();
    for (start_index, first) in sentences.iter().enumerate() {
        for width in 1..=3 {
            let Some(last) = sentences.get(start_index + width - 1) else {
                break;
            };
            if last.paragraph != first.paragraph {
                break;
            }
            let excerpt = content[first.start..last.end].trim();
            if excerpt.is_empty() || excerpt.chars().count() > 320 {
                break;
            }
            let mut matches = content.match_indices(excerpt);
            if matches.next().is_none() || matches.next().is_some() {
                continue;
            }
            spans.push(FinalBodyEvidenceSpan {
                start_char: content[..first.start].chars().count(),
                end_char: content[..last.end].chars().count(),
                excerpt: excerpt.to_string(),
            });
        }
    }
    spans
}

fn indexed_body_sentences(content: &str) -> Vec<IndexedBodySentence> {
    let mut sentences = Vec::new();
    let mut start = 0;
    let mut paragraph = 0;
    for (index, ch) in content.char_indices() {
        let end = index + ch.len_utf8();
        if body_char_ends_sentence(content, index, ch) {
            push_indexed_body_sentence(content, start, end, paragraph, &mut sentences);
            start = end;
        } else if ch == '\n' {
            push_indexed_body_sentence(content, start, index, paragraph, &mut sentences);
            start = end;
            paragraph = paragraph.saturating_add(1);
        }
    }
    push_indexed_body_sentence(content, start, content.len(), paragraph, &mut sentences);
    sentences
}

fn body_char_ends_sentence(content: &str, index: usize, ch: char) -> bool {
    if matches!(ch, '。' | '！' | '？' | '!' | '?') {
        return true;
    }
    if ch != '.' {
        return false;
    }
    let previous = content[..index].chars().next_back();
    let next = content[index + ch.len_utf8()..].chars().next();
    !(previous.is_some_and(|value| value.is_ascii_digit())
        && next.is_some_and(|value| value.is_ascii_digit()))
}

fn push_indexed_body_sentence(
    content: &str,
    start: usize,
    end: usize,
    paragraph: usize,
    sentences: &mut Vec<IndexedBodySentence>,
) {
    if start >= end || end > content.len() {
        return;
    }
    let slice = &content[start..end];
    let Some(first) = slice.find(|ch: char| !ch.is_whitespace()) else {
        return;
    };
    let Some(last) = slice.rfind(|ch: char| !ch.is_whitespace()) else {
        return;
    };
    let absolute_start = start + first;
    let absolute_end = start
        + last
        + slice[last..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0);
    if absolute_start >= absolute_end {
        return;
    }
    sentences.push(IndexedBodySentence {
        id: sentences.len() + 1,
        paragraph,
        start: absolute_start,
        end: absolute_end,
    });
}

fn observation_sentence_ids(value: &serde_json::Value) -> Vec<usize> {
    let values = match value {
        serde_json::Value::Array(values) => values.iter().collect::<Vec<_>>(),
        value => vec![value],
    };
    values
        .into_iter()
        .filter_map(|value| match value {
            serde_json::Value::Number(number) => number
                .as_u64()
                .and_then(|number| usize::try_from(number).ok()),
            serde_json::Value::String(value) => value
                .trim()
                .trim_start_matches(['S', 's'])
                .parse::<usize>()
                .ok(),
            _ => None,
        })
        .collect()
}

fn resolve_observation_sentence_ids(content: &str, ids: &[usize]) -> Option<String> {
    if ids.is_empty() || ids.len() > 3 {
        return None;
    }
    let sentences = indexed_body_sentences(content);
    let selected = ids
        .iter()
        .map(|id| sentences.get(id.checked_sub(1)?).copied())
        .collect::<Option<Vec<_>>>()?;
    let first = *selected.first()?;
    if selected.iter().enumerate().any(|(offset, sentence)| {
        sentence.id != first.id + offset || sentence.paragraph != first.paragraph
    }) {
        return None;
    }
    let last = *selected.last()?;
    let excerpt = content[first.start..last.end].trim();
    if excerpt.is_empty() || excerpt.chars().count() > 320 {
        return None;
    }
    let mut matches = content.match_indices(excerpt);
    matches.next()?;
    matches.next().is_none().then(|| excerpt.to_string())
}

fn observation_text_from_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.trim().to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Array(values) => values
            .iter()
            .map(observation_text_from_value)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("；"),
        serde_json::Value::Object(values) => values
            .iter()
            .filter_map(|(key, value)| {
                let value = observation_text_from_value(value);
                (!value.is_empty()).then(|| format!("{}：{}", key.trim(), value))
            })
            .collect::<Vec<_>>()
            .join("；"),
        serde_json::Value::Null => String::new(),
    }
}

fn clean_observation_items(items: Vec<String>) -> Vec<String> {
    let mut cleaned = items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    cleaned.sort();
    cleaned.dedup();
    cleaned
}

fn parse_provenance(raw: &str, freeform: bool) -> ParseProvenance {
    if freeform {
        return ParseProvenance::Freeform;
    }
    if extract_json(raw)
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .is_some()
    {
        ParseProvenance::ExactJson
    } else {
        ParseProvenance::RecoveredJson
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_protagonist_first_in_character_authority() {
        let authority = CharacterAuthority::from_names(
            Some("姓名：沈砚；身份：主角".to_string()),
            vec!["楚辞尘".to_string(), "沈砚".to_string()],
        );

        assert_eq!(authority.protagonist.as_deref(), Some("沈砚"));
        assert_eq!(authority.canonical_names, ["沈砚", "楚辞尘"]);
    }

    #[test]
    fn sealed_projection_exposes_registered_chapter_characters() {
        let authority = CharacterAuthority::from_context(&serde_json::json!({
            "authority": {
                "working_context": {
                    "continuity_anchors": {
                        "primary_characters": ["沈砚"],
                        "characters": ["沈砚", "楚辞尘"]
                    }
                }
            }
        }));

        assert_eq!(authority.protagonist.as_deref(), Some("沈砚"));
        assert_eq!(authority.canonical_names, ["沈砚", "楚辞尘"]);
    }

    #[test]
    fn distinguishes_exact_recovered_and_freeform_parse_provenance() {
        assert_eq!(
            parse_provenance(r#"{"title":"第一章"}"#, false),
            ParseProvenance::ExactJson
        );
        assert_eq!(
            parse_provenance(r#"{"title":"第一章""#, false),
            ParseProvenance::RecoveredJson
        );
        assert_eq!(
            parse_provenance("第一章正文", true),
            ParseProvenance::Freeform
        );
    }

    #[test]
    fn stream_protocol_provenance_is_not_json_recovery() {
        let raw = "TITLE: 雨夜旧站\n---BODY---\n闻庭安推开旧站的门。\n---END BODY---";
        let draft = parse_draft_output(raw, 1, "zh");
        let provenance = if parse_draft_stream_protocol(raw).is_some() {
            if draft.degraded {
                ParseProvenance::TruncatedStream
            } else {
                ParseProvenance::StreamProtocol
            }
        } else {
            parse_provenance(raw, draft.degraded)
        };

        assert_eq!(provenance, ParseProvenance::StreamProtocol);
    }
}
