#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderHealthIssue {
    ServiceDisconnected,
    TurnTimedOut,
    StreamStalled,
}

pub fn classify_provider_health_issue(message: &str) -> Option<ProviderHealthIssue> {
    let lowered = message.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return None;
    }

    let disconnect_markers = [
        "error sending request",
        "error decoding response body",
        "failed to read response body",
        "connection error",
        "connection refused",
        "connection reset",
        "connection closed before message completed",
        "connection aborted",
        "failed to connect",
        "tcp connect error",
        "broken pipe",
        "operation timed out",
        "request timed out",
        "deadline has elapsed",
        "stream interrupted",
        "unexpected eof",
        "incomplete message",
        "server disconnected",
        "service unavailable",
        "provider_service_unavailable",
        "provider service unavailable",
        "temporarily unavailable",
    ];

    if disconnect_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return Some(ProviderHealthIssue::ServiceDisconnected);
    }

    let turn_timeout_markers = [
        "llm_turn_timeout",
        "llm step timeout",
        "model produced no usable output",
        "model call returned no executable tool call or deliverable content",
        "runtime stopped this turn to avoid a silent background wait",
        "本轮模型调用在",
        "系统已停止这一轮等待",
        "避免后台空转",
    ];

    if turn_timeout_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return Some(ProviderHealthIssue::TurnTimedOut);
    }

    let stream_stall_markers = [
        "stream timeout after",
        "stream timed out after",
        "stream idle timeout",
        "stream produced no chunk",
        "continuous text stream timeout",
    ];

    if stream_stall_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return Some(ProviderHealthIssue::StreamStalled);
    }

    None
}

pub fn provider_service_pause_reason(message: &str) -> String {
    let preview = preview_text(message.trim(), 320);
    match classify_provider_health_issue(message) {
        Some(ProviderHealthIssue::TurnTimedOut) => format!(
            "model turn timed out before producing usable output; task paused at the latest checkpoint and can be resumed with the same task context. provider_error_preview: {preview}"
        ),
        Some(ProviderHealthIssue::StreamStalled) => format!(
            "model text stream stalled before producing a complete artifact; task paused at the latest checkpoint and can be resumed with the same task context. provider_error_preview: {preview}"
        ),
        _ => format!(
            "model provider service disconnected; task paused at the latest checkpoint and can be resumed after the runtime host restarts. provider_error_preview: {preview}"
        ),
    }
}

pub fn is_recoverable_provider_disconnect(message: &str) -> bool {
    matches!(
        classify_provider_health_issue(message),
        Some(
            ProviderHealthIssue::ServiceDisconnected
                | ProviderHealthIssue::TurnTimedOut
                | ProviderHealthIssue::StreamStalled
        )
    )
}

pub fn provider_health_issue_should_restart_runtime_host(message: &str) -> bool {
    matches!(
        classify_provider_health_issue(message),
        Some(ProviderHealthIssue::ServiceDisconnected)
    )
}

fn preview_text(message: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in message.chars().take(max_chars) {
        out.push(ch);
    }
    if message.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_local_provider_disconnects() {
        for message in [
            "Internal error: error sending request for url (http://127.0.0.1/v1/chat/completions)",
            "status: paused\nerror_kind: provider_service_unavailable\nblockers: local provider disconnected",
            "Internal error: error decoding response body",
            "Provider API error: Failed to read response body: connection closed before message completed",
            "Connection refused (os error 111)",
        ] {
            assert_eq!(
                classify_provider_health_issue(message),
                Some(ProviderHealthIssue::ServiceDisconnected)
            );
        }
    }

    #[test]
    fn classifies_recoverable_turn_timeouts() {
        for message in [
            "status: blocked\nerror_kind: llm_turn_timeout\nblockers: 本轮模型调用在 240 秒内没有返回可执行工具调用或可交付内容。",
            "LLM STEP TIMEOUT: model produced no usable output within 240s",
            "The model call returned no executable tool call or deliverable content within 240 seconds.",
        ] {
            assert_eq!(
                classify_provider_health_issue(message),
                Some(ProviderHealthIssue::TurnTimedOut)
            );
            assert!(is_recoverable_provider_disconnect(message));
        }
    }

    #[test]
    fn classifies_recoverable_stream_stalls() {
        for message in [
            "Stream timeout after 74s",
            "continuous step 1 attempt 1 failed: Stream timeout after 74s",
            "continuous text stream timeout while waiting for local model output",
        ] {
            assert_eq!(
                classify_provider_health_issue(message),
                Some(ProviderHealthIssue::StreamStalled)
            );
            assert!(is_recoverable_provider_disconnect(message));
            assert!(!provider_health_issue_should_restart_runtime_host(message));
        }
    }

    #[test]
    fn only_service_disconnect_restarts_runtime_host() {
        assert!(provider_health_issue_should_restart_runtime_host(
            "error sending request for url (http://127.0.0.1/v1/chat/completions)"
        ));
        assert!(!provider_health_issue_should_restart_runtime_host(
            "Stream timeout after 180s"
        ));
        assert!(!provider_health_issue_should_restart_runtime_host(
            "model produced no usable output"
        ));
    }

    #[test]
    fn does_not_classify_contract_errors_as_disconnects() {
        assert_eq!(
            classify_provider_health_issue("chapter still needs revision before approval"),
            None
        );
    }
}
