pub fn compact_external_error_message<F>(channel: &str, error: &str, is_realtime: F) -> String
where
    F: Fn(&str) -> bool,
{
    if is_realtime(channel) {
        let lowered = error.to_lowercase();
        if lowered.contains("task preempted by new input") {
            return "收到你的新消息了，我已经切到最新这条来继续处理。".to_string();
        }
        if lowered.contains("timed out") {
            return "抱歉，我这次本地推理超时了。请再发一次更短一点的消息，我会用更轻的上下文继续回复。"
                .to_string();
        }
        if lowered.contains("provider error")
            || lowered.contains("inference failed")
            || lowered.contains("decode failed")
        {
            return "抱歉，我刚才本地推理出了点问题。这条错误已经被系统记录，你可以直接再发一次，我会尽量用更轻的路径回复。"
                .to_string();
        }
        return "抱歉，我这次处理消息时遇到内部错误。你可以直接再发一次，我会重新尝试。"
            .to_string();
    }

    format!("I encountered an error: {}", error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_timeout_is_user_friendly() {
        let message = compact_external_error_message("telegram", "timed out", |channel| {
            channel == "telegram"
        });
        assert!(message.contains("超时"));
    }

    #[test]
    fn non_realtime_keeps_error_prefix() {
        let message = compact_external_error_message("api", "boom", |_| false);
        assert_eq!(message, "I encountered an error: boom");
    }
}
