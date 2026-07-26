use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHelperHealth {
    pub running: bool,
    pub provider_origin: String,
    pub active_sessions: usize,
    pub active_pages: usize,
    pub last_error: Option<String>,
}

impl BrowserHelperHealth {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            running: false,
            provider_origin: "unavailable".to_string(),
            active_sessions: 0,
            active_pages: 0,
            last_error: Some(reason.into()),
        }
    }
}
