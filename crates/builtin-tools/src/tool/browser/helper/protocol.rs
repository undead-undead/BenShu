use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHelperRequest {
    pub request_id: String,
    pub action: String,
    pub session_id: Option<String>,
    pub page_id: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHelperResponse {
    pub request_id: String,
    pub ok: bool,
    pub session_id: Option<String>,
    pub page_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHelperShutdownRequest {
    pub reason: String,
}
