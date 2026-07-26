use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use sha2::{Digest, Sha256};

use crate::agent::message::{Message, Role};
use crate::agent::protocol::ReasonerConfig;
use crate::agent::provider::Provider;
use crate::error::{Error, Result};

use super::reasoner_constants;

pub(super) async fn prepare_messages<P: Provider>(
    provider: &Arc<P>,
    config: &ReasonerConfig,
    distillation_cache: &Cache<Vec<u8>, Message>,
    auxiliary_session_id: Option<String>,
    mut messages: Vec<Message>,
) -> Result<Vec<Message>> {
    messages.retain(|m| !m.content.as_text().trim().is_empty());
    let original_messages = messages.clone();

    if messages.len() <= config.max_history_messages {
        return Ok(ensure_latest_user_turn_retained(
            messages,
            &original_messages,
            config.max_history_messages,
        ));
    }

    if !config.smart_pruning {
        let start = messages.len() - config.max_history_messages;
        return Ok(ensure_latest_user_turn_retained(
            messages[start..].to_vec(),
            &original_messages,
            config.max_history_messages,
        ));
    }

    let keep_recent =
        (config.max_history_messages as f32 * reasoner_constants::RECENT_HISTORY_RATIO) as usize;
    let to_summarize_count = messages.len() - keep_recent;

    if to_summarize_count <= 2 {
        let start = messages.len() - config.max_history_messages;
        return Ok(ensure_latest_user_turn_retained(
            messages[start..].to_vec(),
            &original_messages,
            config.max_history_messages,
        ));
    }

    let (to_summarize, recent_history) = messages.split_at(to_summarize_count);

    let mut hasher = Sha256::new();
    for m in to_summarize {
        hasher.update(m.content.as_text().as_bytes());
    }
    let cache_key = hasher.finalize().to_vec();

    let summary_msg = match distillation_cache.get(&cache_key).await {
        Some(msg) => msg,
        None => {
            let msg = summarize_old_messages(provider, config, auxiliary_session_id, to_summarize)
                .await?;
            distillation_cache.insert(cache_key, msg.clone()).await;
            msg
        }
    };

    let mut new_messages = if !to_summarize.is_empty() && to_summarize[0].role == Role::System {
        vec![to_summarize[0].clone(), summary_msg]
    } else {
        vec![summary_msg]
    };

    new_messages.extend_from_slice(recent_history);

    if new_messages.len() > config.max_history_messages {
        let start = new_messages.len() - config.max_history_messages;
        new_messages = new_messages[start..].to_vec();
        if new_messages.iter().all(|m| m.role != Role::System)
            && !to_summarize.is_empty()
            && to_summarize[0].role == Role::System
        {
            new_messages.insert(0, to_summarize[0].clone());
        }
    }

    Ok(ensure_latest_user_turn_retained(
        new_messages,
        &original_messages,
        config.max_history_messages,
    ))
}

fn ensure_latest_user_turn_retained(
    mut messages: Vec<Message>,
    original_messages: &[Message],
    max_history_messages: usize,
) -> Vec<Message> {
    if messages.iter().any(|message| message.role == Role::User) {
        return messages;
    }

    let Some(latest_user) = original_messages
        .iter()
        .rev()
        .find(|message| message.role == Role::User)
        .cloned()
    else {
        return messages;
    };

    messages.push(latest_user);
    if max_history_messages == 0 {
        return messages;
    }

    while messages.len() > max_history_messages {
        let removable_non_user = messages
            .iter()
            .enumerate()
            .find(|(_, message)| message.role != Role::User)
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        let removable = messages
            .iter()
            .enumerate()
            .find(|(idx, message)| {
                message.role != Role::User && (*idx != 0 || message.role != Role::System)
            })
            .map(|(idx, _)| idx)
            .unwrap_or(removable_non_user)
            .min(messages.len().saturating_sub(1));
        if messages
            .get(removable)
            .is_some_and(|message| message.role == Role::User)
            && messages.iter().any(|message| message.role != Role::User)
        {
            let fallback = messages
                .iter()
                .position(|message| message.role != Role::User)
                .unwrap_or(0);
            messages.remove(fallback);
        } else {
            messages.remove(removable);
        }
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_latest_user_when_recent_slice_is_only_system_recovery() {
        let original = vec![
            Message::system("Current Session ID: test"),
            Message::user("你好，用一句中文回复。"),
            Message::system("### TOOL EXECUTION REQUIRED"),
        ];
        let sliced = vec![Message::system("### TOOL EXECUTION REQUIRED")];

        let retained = ensure_latest_user_turn_retained(sliced, &original, 1);

        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].role, Role::User);
        assert!(retained[0].content.as_text().contains("你好"));
    }

    #[test]
    fn does_not_duplicate_user_when_slice_already_has_one() {
        let original = vec![Message::user("原始问题")];
        let sliced = vec![Message::user("当前问题")];

        let retained = ensure_latest_user_turn_retained(sliced, &original, 1);

        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].content.as_text(), "当前问题");
    }
}

async fn summarize_old_messages<P: Provider>(
    provider: &Arc<P>,
    config: &ReasonerConfig,
    auxiliary_session_id: Option<String>,
    old_messages: &[Message],
) -> Result<Message> {
    let mut history_text = String::new();
    for (i, m) in old_messages.iter().enumerate() {
        if i == 0 && m.role == Role::System {
            continue;
        }

        let role = match m.role {
            Role::System => "SYSTEM",
            Role::User => "USER",
            Role::Assistant => "ASSISTANT",
            Role::Tool => "TOOL_RESULT",
        };

        let owned_text = m.content.as_text();
        let safe_text = if owned_text.len() > 800 {
            format!("{}... [TRUNCATED]", &owned_text[0..800])
        } else {
            owned_text
        };

        history_text.push_str(&format!("[{}] {}: {}\n", i, role, safe_text.trim()));
    }

    if history_text.is_empty() {
        return Ok(Message::system(
            "### CONTEXT DISTILLATION\nInteraction history exhausted.",
        ));
    }

    let prompt = format!(
        "### CONTEXT DISTILLER\nDistill this history into a 'Task Fact Bundle'.\n\n\
         1. GLOBAL GOAL\n2. KEY DISCOVERIES\n3. TOOL OUTCOMES\n4. NEXT STEPS\n\n\
         HISTORY:\n{}",
        history_text
    );

    let request = crate::agent::provider::ChatRequest {
        model: config.model.clone(),
        system_prompt: Some(
            "You are a high-precision context distiller. Output facts only.".to_string(),
        ),
        messages: vec![Message::user(prompt)],
        temperature: Some(0.1),
        max_tokens: Some(512),
        session_id: auxiliary_session_id,
        ..Default::default()
    };

    let stream = tokio::time::timeout(Duration::from_secs(20), provider.stream_completion(request))
        .await
        .map_err(|_| Error::Internal("Distillation timed out".to_string()))??;

    let summary = stream
        .collect_text()
        .await
        .unwrap_or_else(|_| "Distillation failed.".to_string());

    Ok(Message::system(format!(
        "### CONTEXT DISTILLATION (FACT BUNDLE)\n{}\n\n(Archived {} interaction turns)",
        summary.trim(),
        old_messages.len()
    )))
}
