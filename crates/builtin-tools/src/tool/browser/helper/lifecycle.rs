use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserHelperLifecycleState {
    NotStarted,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHelperLifecycleReceipt {
    pub schema_version: String,
    pub state: BrowserHelperLifecycleState,
    pub owned_processes: usize,
    pub owned_profiles: usize,
}

impl BrowserHelperLifecycleReceipt {
    pub fn new(state: BrowserHelperLifecycleState) -> Self {
        Self {
            schema_version: "benshu.browser.helper_lifecycle.v1".to_string(),
            state,
            owned_processes: 0,
            owned_profiles: 0,
        }
    }
}
