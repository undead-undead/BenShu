use super::health::BrowserHelperHealth;

#[derive(Debug, Clone)]
pub struct BrowserHelperClient {
    base_url: String,
}

impl BrowserHelperClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn offline_health(&self, reason: impl Into<String>) -> BrowserHelperHealth {
        BrowserHelperHealth::unavailable(reason)
    }
}
