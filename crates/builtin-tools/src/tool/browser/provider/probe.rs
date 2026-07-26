use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserProviderProbe {
    pub executable_found: bool,
    pub cdp_http_available: bool,
    pub cdp_websocket_available: bool,
    pub page_domain_available: bool,
    pub runtime_domain_available: bool,
    pub dom_domain_available: bool,
    pub network_domain_available: bool,
    pub input_domain_available: bool,
    pub screenshot_available: bool,
}

impl BrowserProviderProbe {
    pub fn unavailable() -> Self {
        Self {
            executable_found: false,
            cdp_http_available: false,
            cdp_websocket_available: false,
            page_domain_available: false,
            runtime_domain_available: false,
            dom_domain_available: false,
            network_domain_available: false,
            input_domain_available: false,
            screenshot_available: false,
        }
    }
}
