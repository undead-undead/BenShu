use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub enum LeakAction {
    Block,
    Redact,
    Warn,
}

#[derive(Debug, Clone)]
pub struct LeakDetection {
    pub pattern_name: String,
    pub redacted_value: String,
    pub action: LeakAction,
}

struct SecretPattern {
    name: &'static str,
    regex: Regex,
    action: LeakAction,
}

pub struct LeakDetector {
    patterns: Vec<SecretPattern>,
}

impl LeakDetector {
    pub fn new() -> Self {
        let pattern_defs: Vec<(&str, &str, LeakAction)> = vec![
            // --- API keys (Redact) ---
            (
                "anthropic_api_key",
                r"sk-ant-api[a-zA-Z0-9\-]{20,}",
                LeakAction::Redact,
            ),
            ("openai_api_key", r"sk-[a-zA-Z0-9]{20,}", LeakAction::Redact),
            ("aws_access_key", r"AKIA[A-Z0-9]{16}", LeakAction::Redact),
            (
                "github_pat",
                r"github_pat_[a-zA-Z0-9_]{22,}",
                LeakAction::Redact,
            ),
            ("github_token", r"ghp_[a-zA-Z0-9]{36}", LeakAction::Redact),
            (
                "stripe_live_key",
                r"sk_live_[a-zA-Z0-9]{24,}",
                LeakAction::Redact,
            ),
            (
                "stripe_test_key",
                r"sk_test_[a-zA-Z0-9]{24,}",
                LeakAction::Redact,
            ),
            (
                "google_api_key",
                r"AIza[a-zA-Z0-9_\-]{35}",
                LeakAction::Redact,
            ),
            (
                "slack_bot_token",
                r"xoxb-[a-zA-Z0-9\-]+",
                LeakAction::Redact,
            ),
            (
                "slack_user_token",
                r"xoxp-[a-zA-Z0-9\-]+",
                LeakAction::Redact,
            ),
            (
                "bearer_token",
                r"Bearer [a-zA-Z0-9._\-]{20,}",
                LeakAction::Redact,
            ),
            (
                "minimax_api_key",
                r"mm-[a-zA-Z0-9]{20,}",
                LeakAction::Redact,
            ), // Added for Minimax
            // --- Block ---
            (
                "pem_private_key",
                r"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
                LeakAction::Block,
            ),
            // --- Warn ---
            (
                "authorization_header",
                r"Authorization:\s*[a-zA-Z0-9._\-]{20,}",
                LeakAction::Warn,
            ),
            (
                "generic_jwt",
                r"eyJ[a-zA-Z0-9_\-]{10,}\.[a-zA-Z0-9_\-]{10,}\.[a-zA-Z0-9_\-]{10,}",
                LeakAction::Warn,
            ),
        ];

        let patterns = pattern_defs
            .into_iter()
            .map(|(name, pattern, action)| SecretPattern {
                name,
                regex: Regex::new(pattern).expect("Invalid regex pattern"),
                action,
            })
            .collect();

        Self { patterns }
    }

    pub fn redact(&self, input: &str) -> (String, Vec<LeakDetection>) {
        let mut result = input.to_string();
        let mut detections = Vec::new();

        for pattern in &self.patterns {
            let matches: Vec<(usize, usize, String)> = pattern
                .regex
                .find_iter(&result)
                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                .collect();

            for (start, end, matched) in matches.iter().rev() {
                detections.push(LeakDetection {
                    pattern_name: pattern.name.to_string(),
                    redacted_value: matched.clone(),
                    action: pattern.action.clone(),
                });

                if pattern.action == LeakAction::Redact {
                    let redacted = redact_string(&matched);
                    result.replace_range(start..end, &redacted);
                }
            }
        }

        (result, detections)
    }
}

fn redact_string(s: &str) -> String {
    let char_count = s.chars().count();
    if char_count <= 8 {
        return "***".to_string();
    }

    let prefix: String = s.chars().take(4).collect();
    let suffix: String = s
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{prefix}***{suffix}")
}

impl Default for LeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leak_detector_redact_openai_key() {
        let detector = LeakDetector::new();
        let input = "Here is my key: sk-abc123DEF456ghi789jkl012";
        let (sanitized, detections) = detector.redact(input);

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].pattern_name, "openai_api_key");
        assert_eq!(detections[0].action, LeakAction::Redact);

        // Redaction keeps first 4 and last 4
        assert!(sanitized.contains("sk-a***l012"));
        assert!(!sanitized.contains("abc123DEF456ghi789jkl012"));
    }

    #[test]
    fn test_leak_detector_block_private_key() {
        let detector = LeakDetector::new();
        let input = "Some text\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----\n";
        let (sanitized, detections) = detector.redact(input);

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].pattern_name, "pem_private_key");
        assert_eq!(detections[0].action, LeakAction::Block);

        // Block action currently doesn't mutate in redact(), it just creates a detection
        // with Action = Block for higher-level handler to deal with.
        // Wait, the logic in redact() currently says: `if pattern.action == LeakAction::Redact { replace... }`
        // So `sanitized` should be identical for Block actions.
        assert_eq!(sanitized, input);
    }

    #[test]
    fn test_redact_string() {
        assert_eq!(redact_string("short"), "***");
        assert_eq!(redact_string("exactly8"), "***");
        assert_eq!(redact_string("thisislongenough"), "this***ough");
    }
}
