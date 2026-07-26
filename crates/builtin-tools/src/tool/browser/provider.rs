use serde::{Deserialize, Serialize};

use super::runtime::{BrowserFamily, BrowserRuntime, BrowserRuntimeOrigin};

pub mod probe;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserProviderKind {
    Edge,
    Chrome,
    EmbeddedLight,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserProviderOrigin {
    WindowsNative,
    WslTestBridge,
    UnixFallback,
    EnvOverride,
    EmbeddedFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserProviderCapabilities {
    pub dump_dom: bool,
    pub managed_profile: bool,
    pub devtools_session: bool,
    pub windows_first: bool,
    pub lifecycle_wait: bool,
    pub structured_dom_read: bool,
    pub readonly_evaluate: bool,
    pub network_summary: bool,
    pub request_interception_policy: bool,
    pub page_session_pool: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserProviderDescriptor {
    pub kind: BrowserProviderKind,
    pub origin: BrowserProviderOrigin,
    pub executable_path: String,
    pub capabilities: BrowserProviderCapabilities,
}

impl BrowserProviderDescriptor {
    pub fn from_runtime(runtime: &BrowserRuntime) -> Self {
        Self::from_runtime_with_interactive_cdp(runtime, !runtime.is_windows_executable())
    }

    pub fn from_runtime_with_interactive_cdp(
        runtime: &BrowserRuntime,
        interactive_cdp: bool,
    ) -> Self {
        let windows_first = matches!(
            runtime.origin,
            BrowserRuntimeOrigin::WindowsNative | BrowserRuntimeOrigin::WslTestBridge
        );
        let devtools_session = interactive_cdp;
        Self {
            kind: runtime.family.into(),
            origin: runtime.origin.into(),
            executable_path: runtime.executable_path.display().to_string(),
            capabilities: BrowserProviderCapabilities {
                dump_dom: true,
                managed_profile: true,
                devtools_session,
                windows_first,
                lifecycle_wait: true,
                structured_dom_read: true,
                readonly_evaluate: devtools_session,
                network_summary: true,
                request_interception_policy: true,
                page_session_pool: devtools_session,
            },
        }
    }

    pub fn semantic_layer(&self) -> &'static str {
        "browser_browse"
    }

    pub fn diagnostic_summary(&self) -> String {
        format!(
            "provider={} origin={} path={} semantic_layer={} cdp_domains={}",
            self.kind.label(),
            self.origin.label(),
            self.executable_path,
            self.semantic_layer(),
            self.cdp_inspired_domains().join("|")
        )
    }

    pub fn cdp_inspired_domains(&self) -> &'static [&'static str] {
        &["Page", "Runtime", "DOM", "Network", "Fetch", "Input"]
    }
}

impl BrowserProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Edge => "edge",
            Self::Chrome => "chrome",
            Self::EmbeddedLight => "embedded_light",
            Self::Unknown => "unknown",
        }
    }
}

impl BrowserProviderOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::WindowsNative => "windows_native",
            Self::WslTestBridge => "wsl_test_bridge",
            Self::UnixFallback => "unix_fallback",
            Self::EnvOverride => "env_override",
            Self::EmbeddedFallback => "embedded_fallback",
        }
    }
}

impl From<BrowserFamily> for BrowserProviderKind {
    fn from(value: BrowserFamily) -> Self {
        match value {
            BrowserFamily::Edge => Self::Edge,
            BrowserFamily::Chrome => Self::Chrome,
            BrowserFamily::Unknown => Self::Unknown,
        }
    }
}

impl From<BrowserRuntimeOrigin> for BrowserProviderOrigin {
    fn from(value: BrowserRuntimeOrigin) -> Self {
        match value {
            BrowserRuntimeOrigin::WindowsNative => Self::WindowsNative,
            BrowserRuntimeOrigin::WslTestBridge => Self::WslTestBridge,
            BrowserRuntimeOrigin::UnixFallback => Self::UnixFallback,
            BrowserRuntimeOrigin::EnvOverride => Self::EnvOverride,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn provider_descriptor_exposes_cdp_inspired_capabilities() {
        let runtime = BrowserRuntime {
            executable_path: PathBuf::from("/usr/bin/google-chrome"),
            family: BrowserFamily::Chrome,
            origin: BrowserRuntimeOrigin::UnixFallback,
        };
        let descriptor = BrowserProviderDescriptor::from_runtime(&runtime);
        assert_eq!(descriptor.semantic_layer(), "browser_browse");
        assert!(descriptor.capabilities.lifecycle_wait);
        assert!(descriptor.capabilities.structured_dom_read);
        assert!(descriptor.capabilities.readonly_evaluate);
        assert!(descriptor.cdp_inspired_domains().contains(&"Runtime"));
        assert!(descriptor.diagnostic_summary().contains("cdp_domains="));
    }

    #[test]
    fn wsl_windows_runtime_can_report_interactive_cdp_when_enabled() {
        let runtime = BrowserRuntime {
            executable_path: PathBuf::from(
                "/mnt/c/Program Files/Google/Chrome/Application/chrome.exe",
            ),
            family: BrowserFamily::Chrome,
            origin: BrowserRuntimeOrigin::WslTestBridge,
        };
        let descriptor =
            BrowserProviderDescriptor::from_runtime_with_interactive_cdp(&runtime, true);
        assert!(descriptor.capabilities.dump_dom);
        assert!(descriptor.capabilities.devtools_session);
        assert!(descriptor.capabilities.page_session_pool);
        assert!(descriptor.capabilities.readonly_evaluate);
    }

    #[test]
    fn wsl_windows_runtime_reports_static_only_when_cdp_disabled() {
        let runtime = BrowserRuntime {
            executable_path: PathBuf::from(
                "/mnt/c/Program Files/Google/Chrome/Application/chrome.exe",
            ),
            family: BrowserFamily::Chrome,
            origin: BrowserRuntimeOrigin::WslTestBridge,
        };
        let descriptor =
            BrowserProviderDescriptor::from_runtime_with_interactive_cdp(&runtime, false);
        assert!(descriptor.capabilities.dump_dom);
        assert!(!descriptor.capabilities.devtools_session);
        assert!(!descriptor.capabilities.page_session_pool);
    }
}
