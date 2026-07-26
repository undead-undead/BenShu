use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CdpRequest {
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CdpResponse {
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<Value>,
}
