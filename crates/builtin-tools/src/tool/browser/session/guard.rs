use super::super::types::{BrowserActionKind, BrowserSessionGuardReceipt};

pub fn guard_for_tool_action(action: &str) -> BrowserSessionGuardReceipt {
    BrowserSessionGuardReceipt::guarded_for_action(BrowserActionKind::from_tool_action(action))
}
