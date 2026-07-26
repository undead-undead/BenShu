use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTakeoverState {
    NotRequired,
    WaitingForUser,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserTakeoverRequest {
    pub state: BrowserTakeoverState,
    pub reason: String,
    pub session_id: Option<String>,
    pub page_id: Option<String>,
    pub continue_token: Option<String>,
}

impl BrowserTakeoverRequest {
    pub fn waiting(reason: impl Into<String>) -> Self {
        Self {
            state: BrowserTakeoverState::WaitingForUser,
            reason: reason.into(),
            session_id: None,
            page_id: None,
            continue_token: None,
        }
    }
}
