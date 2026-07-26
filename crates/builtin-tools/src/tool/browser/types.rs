use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserActionKind {
    Search,
    Navigate,
    Click,
    Fill,
    Snapshot,
    ExtractLinks,
    Evaluate,
    Hover,
    Scroll,
    SaveSession,
    LoadSession,
    Diff,
    Screenshot,
    VisualAnalyze,
    Unknown,
}

impl BrowserActionKind {
    pub fn from_tool_action(action: &str) -> Self {
        match action.trim().to_ascii_lowercase().as_str() {
            "search" => Self::Search,
            "navigate" => Self::Navigate,
            "click" => Self::Click,
            "fill" => Self::Fill,
            "snapshot" => Self::Snapshot,
            "extract_links" => Self::ExtractLinks,
            "evaluate" => Self::Evaluate,
            "hover" => Self::Hover,
            "scroll" => Self::Scroll,
            "save_session" => Self::SaveSession,
            "load_session" => Self::LoadSession,
            "diff" => Self::Diff,
            "screenshot" => Self::Screenshot,
            "visual_analyze" => Self::VisualAnalyze,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserControlMode {
    Anonymous,
    WaitingForUser,
    GuardedReadOnly,
    ApprovedInteraction,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserActionRisk {
    Observe,
    Navigate,
    Interact,
    Submit,
    Destructive,
    Credential,
}

impl BrowserActionRisk {
    pub fn for_action(action: BrowserActionKind) -> Self {
        match action {
            BrowserActionKind::Search
            | BrowserActionKind::Snapshot
            | BrowserActionKind::ExtractLinks
            | BrowserActionKind::Evaluate
            | BrowserActionKind::Diff
            | BrowserActionKind::Screenshot
            | BrowserActionKind::VisualAnalyze
            | BrowserActionKind::LoadSession
            | BrowserActionKind::SaveSession => Self::Observe,
            BrowserActionKind::Navigate | BrowserActionKind::Scroll => Self::Navigate,
            BrowserActionKind::Click | BrowserActionKind::Fill | BrowserActionKind::Hover => {
                Self::Interact
            }
            BrowserActionKind::Unknown => Self::Interact,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSessionGuardReceipt {
    pub schema_version: String,
    pub session_mode: String,
    pub control_mode: BrowserControlMode,
    pub action_risk: BrowserActionRisk,
    pub allowed_action_level: BrowserActionRisk,
    pub approval_required: bool,
    pub approval_id: Option<String>,
    pub user_takeover: BrowserUserTakeoverReceipt,
    pub untrusted_page_content: bool,
    pub sensitive_element_flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserUserTakeoverReceipt {
    pub required: bool,
    pub reason: Option<String>,
    pub session_id: Option<String>,
    pub page_id: Option<String>,
    pub continue_token: Option<String>,
}

impl BrowserSessionGuardReceipt {
    pub fn guarded_for_action(action: BrowserActionKind) -> Self {
        let action_risk = BrowserActionRisk::for_action(action);
        let approval_required = matches!(
            action_risk,
            BrowserActionRisk::Submit
                | BrowserActionRisk::Destructive
                | BrowserActionRisk::Credential
        );
        let control_mode = match action_risk {
            BrowserActionRisk::Observe | BrowserActionRisk::Navigate => {
                BrowserControlMode::GuardedReadOnly
            }
            BrowserActionRisk::Interact => BrowserControlMode::ApprovedInteraction,
            BrowserActionRisk::Submit
            | BrowserActionRisk::Destructive
            | BrowserActionRisk::Credential => BrowserControlMode::WaitingForUser,
        };
        let allowed_action_level = if approval_required {
            BrowserActionRisk::Observe
        } else {
            action_risk
        };
        Self {
            schema_version: "benshu.browser.session_guard.v1".to_string(),
            session_mode: "anonymous_or_unknown".to_string(),
            control_mode,
            action_risk,
            allowed_action_level,
            approval_required,
            approval_id: None,
            user_takeover: BrowserUserTakeoverReceipt {
                required: false,
                reason: None,
                session_id: None,
                page_id: None,
                continue_token: None,
            },
            untrusted_page_content: true,
            sensitive_element_flags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHtmlInputReceipt {
    pub schema_version: String,
    pub html_source: String,
    pub html_chars: usize,
    pub inline_chars: usize,
    pub artifact_ref: Option<String>,
    pub untrusted_page_content: bool,
    pub authenticated_session: bool,
    pub user_takeover: bool,
    pub content_visibility: String,
}

impl BrowserHtmlInputReceipt {
    pub fn public_snapshot(html_chars: usize, inline_chars: usize) -> Self {
        Self {
            schema_version: "benshu.browser.html_input.v1".to_string(),
            html_source: "browser_observation".to_string(),
            html_chars,
            inline_chars,
            artifact_ref: None,
            untrusted_page_content: true,
            authenticated_session: false,
            user_takeover: false,
            content_visibility: "unknown".to_string(),
        }
    }
}
