use crate::net_safety::validate_public_http_url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserSafetyDecision {
    Allow { notes: Vec<String> },
    Block { reason: String },
    UserTakeoverRequired { reason: String },
}

impl BrowserSafetyDecision {
    pub fn into_result(self) -> anyhow::Result<Vec<String>> {
        match self {
            Self::Allow { notes } => Ok(notes),
            Self::Block { reason } => {
                anyhow::bail!("browser safety boundary blocked action: {reason}")
            }
            Self::UserTakeoverRequired { reason } => {
                anyhow::bail!(
                    "browser action requires user takeover or explicit approval: {reason}"
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BrowserSafetyRequest<'a> {
    pub action: &'a str,
    pub url: Option<&'a str>,
    pub selector: Option<&'a str>,
    pub text: Option<&'a str>,
    pub current_url: Option<&'a str>,
    pub readonly: Option<bool>,
}

pub struct BrowserSafetyGate;

impl BrowserSafetyGate {
    pub fn preflight(request: &BrowserSafetyRequest<'_>) -> BrowserSafetyDecision {
        let action = request.action.trim().to_ascii_lowercase();
        let mut notes = Vec::new();

        if let Some(url) = request.url.or(request.current_url) {
            if let Err(reason) = Self::validate_public_web_url(url) {
                return BrowserSafetyDecision::Block { reason };
            }
            notes.push("url_boundary=public_http_or_https".to_string());
        }

        match action.as_str() {
            "search" => {
                if let Some(query) = request.text {
                    if query.chars().count() > 4096 {
                        return BrowserSafetyDecision::Block {
                            reason: "search query is too large for browser execution".to_string(),
                        };
                    }
                }
            }
            "evaluate" => {
                if request.readonly == Some(false) {
                    return BrowserSafetyDecision::Block {
                        reason: "Runtime.evaluate must be readonly".to_string(),
                    };
                }
                notes.push("runtime_evaluate=readonly".to_string());
            }
            "click" => {
                if let Some(selector) = request.selector {
                    if Self::looks_like_account_mutation_selector(selector) {
                        return BrowserSafetyDecision::UserTakeoverRequired {
                            reason: "selector appears to trigger account, payment, publishing, upload, or destructive state change".to_string(),
                        };
                    }
                }
            }
            "fill" => {
                if request
                    .selector
                    .is_some_and(Self::looks_like_sensitive_input_selector)
                    || request
                        .text
                        .is_some_and(Self::looks_like_secret_or_payment_text)
                {
                    return BrowserSafetyDecision::UserTakeoverRequired {
                        reason: "fill target or value appears to contain credentials, payment data, OTP, or a secret".to_string(),
                    };
                }
            }
            "save_session" | "load_session" => {
                let Some(key) = request
                    .text
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return BrowserSafetyDecision::UserTakeoverRequired {
                        reason: "browser session persistence requires an explicit session key"
                            .to_string(),
                    };
                };
                if !Self::session_key_is_safe(key) {
                    return BrowserSafetyDecision::Block {
                        reason: "browser session key contains path separators, control characters, or is too long".to_string(),
                    };
                }
                notes.push("session_cookie_access=explicit_key".to_string());
            }
            _ => {}
        }

        BrowserSafetyDecision::Allow { notes }
    }

    pub fn enforce(request: &BrowserSafetyRequest<'_>) -> anyhow::Result<Vec<String>> {
        Self::preflight(request).into_result()
    }

    pub fn validate_public_web_url(raw_url: &str) -> Result<(), String> {
        validate_public_http_url(raw_url).map(|_| ())
    }

    fn looks_like_sensitive_input_selector(selector: &str) -> bool {
        let lowered = selector.to_ascii_lowercase();
        [
            "password",
            "passwd",
            "pwd",
            "otp",
            "2fa",
            "mfa",
            "totp",
            "verification-code",
            "verification_code",
            "credit-card",
            "credit_card",
            "cardnumber",
            "card-number",
            "cvv",
            "cvc",
            "api-key",
            "api_key",
            "secret",
            "token",
            "private-key",
            "private_key",
        ]
        .iter()
        .any(|term| lowered.contains(term))
    }

    fn looks_like_secret_or_payment_text(text: &str) -> bool {
        let trimmed = text.trim();
        let lowered = trimmed.to_ascii_lowercase();
        if lowered.starts_with("sk-")
            || lowered.starts_with("ghp_")
            || lowered.starts_with("xoxb-")
            || lowered.contains("-----begin private key-----")
        {
            return true;
        }
        let digits = trimmed.chars().filter(|ch| ch.is_ascii_digit()).count();
        digits >= 13
            && digits <= 19
            && trimmed.chars().filter(|ch| !ch.is_whitespace()).count() <= 24
    }

    fn looks_like_account_mutation_selector(selector: &str) -> bool {
        let lowered = selector.to_ascii_lowercase();
        [
            "delete",
            "remove",
            "destroy",
            "checkout",
            "purchase",
            "buy",
            "pay",
            "payment",
            "order",
            "transfer",
            "withdraw",
            "deposit",
            "publish",
            "post",
            "tweet",
            "send-message",
            "send_message",
            "follow",
            "unfollow",
            "subscribe",
            "unsubscribe",
            "upload",
            "confirm-delete",
            "confirm_delete",
        ]
        .iter()
        .any(|term| lowered.contains(term))
    }

    fn session_key_is_safe(key: &str) -> bool {
        !key.is_empty()
            && key.chars().count() <= 96
            && !key.contains('/')
            && !key.contains('\\')
            && !key.contains("..")
            && !key.chars().any(char::is_control)
    }
}

#[cfg(test)]
mod tests {
    use super::{BrowserSafetyDecision, BrowserSafetyGate, BrowserSafetyRequest};

    fn request<'a>(action: &'a str, url: Option<&'a str>) -> BrowserSafetyRequest<'a> {
        BrowserSafetyRequest {
            action,
            url,
            selector: None,
            text: None,
            current_url: None,
            readonly: None,
        }
    }

    #[test]
    fn browser_safety_allows_public_https() {
        assert!(matches!(
            BrowserSafetyGate::preflight(&request("navigate", Some("https://example.com/path"))),
            BrowserSafetyDecision::Allow { .. }
        ));
    }

    #[test]
    fn browser_safety_blocks_local_and_private_targets() {
        for url in [
            "http://localhost:3000",
            "http://127.0.0.1:9222/json/version",
            "http://192.168.1.1/",
            "http://10.0.0.5/",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "file:///etc/passwd",
        ] {
            assert!(
                matches!(
                    BrowserSafetyGate::preflight(&request("navigate", Some(url))),
                    BrowserSafetyDecision::Block { .. }
                ),
                "{url}"
            );
        }
    }

    #[test]
    fn browser_safety_allows_normal_fill_but_requires_takeover_for_secret_fill() {
        let normal = BrowserSafetyRequest {
            action: "fill",
            url: None,
            selector: Some("#searchInput"),
            text: Some("玄幻小说"),
            current_url: None,
            readonly: None,
        };
        assert!(matches!(
            BrowserSafetyGate::preflight(&normal),
            BrowserSafetyDecision::Allow { .. }
        ));

        let secret = BrowserSafetyRequest {
            selector: Some("input[type=password]"),
            text: Some("hunter2"),
            ..normal
        };
        assert!(matches!(
            BrowserSafetyGate::preflight(&secret),
            BrowserSafetyDecision::UserTakeoverRequired { .. }
        ));
    }

    #[test]
    fn browser_safety_requires_explicit_safe_session_key() {
        let missing = BrowserSafetyRequest {
            action: "save_session",
            url: None,
            selector: None,
            text: None,
            current_url: None,
            readonly: None,
        };
        assert!(matches!(
            BrowserSafetyGate::preflight(&missing),
            BrowserSafetyDecision::UserTakeoverRequired { .. }
        ));

        let safe = BrowserSafetyRequest {
            text: Some("qidian-user-profile"),
            ..missing
        };
        assert!(matches!(
            BrowserSafetyGate::preflight(&safe),
            BrowserSafetyDecision::Allow { .. }
        ));
    }
}
