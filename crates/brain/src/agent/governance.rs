use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::agent::protocol::{ApprovalHandler, RiskyToolPolicy, TokenUsage, ToolPolicy};
use crate::security::SecurityHandler;
use crate::skills::tool::SafetyLevel;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityRequirement {
    Automatic,
    ExplicitApproval,
    Blocked,
}

impl AuthorityRequirement {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::ExplicitApproval => "explicit_approval",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceScope {
    ReadOnly,
    WriteMemory,
    ExecuteTools,
    WriteExternal,
}

impl GovernanceScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::WriteMemory => "write_memory",
            Self::ExecuteTools => "execute_tools",
            Self::WriteExternal => "write_external",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GovernanceBudgetSnapshot {
    pub limit: Option<u32>,
    pub used: u32,
    pub remaining: Option<u32>,
    pub exceeded: bool,
    pub tool_calls: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GovernanceDecision {
    pub scope: GovernanceScope,
    pub subject: String,
    pub policy: ToolPolicy,
    pub authority: AuthorityRequirement,
    pub safety_level: SafetyLevel,
    pub approved: Option<bool>,
    pub risk_score: f32,
    pub detail: Option<String>,
    pub budget: GovernanceBudgetSnapshot,
}

/// Explicit governance boundary for an agent runtime.
///
/// This object carries the durable policies that must survive task-local
/// boundaries and child-agent spawning.
pub struct GovernanceContext {
    tool_policy: RiskyToolPolicy,
    approval_handler: Arc<dyn ApprovalHandler>,
    trusted_workspaces: Vec<PathBuf>,
    security_handler: Arc<dyn SecurityHandler>,
    risk_score: parking_lot::RwLock<f32>,
    token_budget: Option<u32>,
    consumed_tokens: AtomicU32,
    tool_calls: AtomicU32,
}

impl GovernanceContext {
    pub fn new(
        tool_policy: RiskyToolPolicy,
        approval_handler: Arc<dyn ApprovalHandler>,
        trusted_workspaces: Vec<PathBuf>,
        security_handler: Arc<dyn SecurityHandler>,
        risk_score: f32,
        token_budget: Option<u32>,
    ) -> Self {
        Self {
            tool_policy,
            approval_handler,
            trusted_workspaces,
            security_handler,
            risk_score: parking_lot::RwLock::new(risk_score),
            token_budget,
            consumed_tokens: AtomicU32::new(0),
            tool_calls: AtomicU32::new(0),
        }
    }

    pub fn tool_policy(&self) -> &RiskyToolPolicy {
        &self.tool_policy
    }

    pub fn approval_handler(&self) -> Arc<dyn ApprovalHandler> {
        self.approval_handler.clone()
    }

    pub fn trusted_workspaces(&self) -> &[PathBuf] {
        &self.trusted_workspaces
    }

    pub fn security_handler(&self) -> Arc<dyn SecurityHandler> {
        self.security_handler.clone()
    }

    pub fn risk_score(&self) -> f32 {
        *self.risk_score.read()
    }

    pub fn set_risk_score(&self, risk_score: f32) {
        *self.risk_score.write() = risk_score;
    }

    pub fn token_budget(&self) -> Option<u32> {
        self.token_budget
    }

    pub fn budget_snapshot(&self) -> GovernanceBudgetSnapshot {
        let used = self.consumed_tokens.load(Ordering::Relaxed);
        let limit = self.token_budget;
        GovernanceBudgetSnapshot {
            limit,
            used,
            remaining: limit.map(|value| value.saturating_sub(used)),
            exceeded: limit.is_some_and(|value| used > value),
            tool_calls: self.tool_calls.load(Ordering::Relaxed),
        }
    }

    pub fn register_token_usage(&self, usage: &TokenUsage) -> GovernanceBudgetSnapshot {
        self.consumed_tokens
            .fetch_add(usage.total_tokens, Ordering::Relaxed);
        self.budget_snapshot()
    }

    pub fn register_tool_call(&self) -> GovernanceBudgetSnapshot {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
        self.budget_snapshot()
    }

    pub fn authority_for_policy(&self, policy: &ToolPolicy) -> AuthorityRequirement {
        match policy {
            ToolPolicy::Auto => AuthorityRequirement::Automatic,
            ToolPolicy::RequiresApproval => AuthorityRequirement::ExplicitApproval,
            ToolPolicy::Disabled => AuthorityRequirement::Blocked,
        }
    }

    pub fn build_tool_decision(
        &self,
        scope: GovernanceScope,
        subject: impl Into<String>,
        policy: ToolPolicy,
        safety_level: SafetyLevel,
        approved: Option<bool>,
        detail: Option<String>,
    ) -> GovernanceDecision {
        GovernanceDecision {
            scope,
            subject: subject.into(),
            authority: self.authority_for_policy(&policy),
            policy,
            safety_level,
            approved,
            risk_score: self.risk_score(),
            detail,
            budget: self.budget_snapshot(),
        }
    }

    fn clone_counters_from(&self, other: &Self) {
        self.consumed_tokens.store(
            other.consumed_tokens.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.tool_calls
            .store(other.tool_calls.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    pub fn with_tool_policy(&self, tool_policy: RiskyToolPolicy) -> Self {
        let next = Self {
            tool_policy,
            approval_handler: self.approval_handler(),
            trusted_workspaces: self.trusted_workspaces.clone(),
            security_handler: self.security_handler(),
            risk_score: parking_lot::RwLock::new(self.risk_score()),
            token_budget: self.token_budget,
            consumed_tokens: AtomicU32::new(0),
            tool_calls: AtomicU32::new(0),
        };
        next.clone_counters_from(self);
        next
    }

    pub fn with_approval_handler(&self, approval_handler: Arc<dyn ApprovalHandler>) -> Self {
        let next = Self {
            tool_policy: self.tool_policy.clone(),
            approval_handler,
            trusted_workspaces: self.trusted_workspaces.clone(),
            security_handler: self.security_handler(),
            risk_score: parking_lot::RwLock::new(self.risk_score()),
            token_budget: self.token_budget,
            consumed_tokens: AtomicU32::new(0),
            tool_calls: AtomicU32::new(0),
        };
        next.clone_counters_from(self);
        next
    }

    pub fn with_trusted_workspaces(&self, trusted_workspaces: Vec<PathBuf>) -> Self {
        let next = Self {
            tool_policy: self.tool_policy.clone(),
            approval_handler: self.approval_handler(),
            trusted_workspaces,
            security_handler: self.security_handler(),
            risk_score: parking_lot::RwLock::new(self.risk_score()),
            token_budget: self.token_budget,
            consumed_tokens: AtomicU32::new(0),
            tool_calls: AtomicU32::new(0),
        };
        next.clone_counters_from(self);
        next
    }

    pub fn with_security_handler(&self, security_handler: Arc<dyn SecurityHandler>) -> Self {
        let next = Self {
            tool_policy: self.tool_policy.clone(),
            approval_handler: self.approval_handler(),
            trusted_workspaces: self.trusted_workspaces.clone(),
            security_handler,
            risk_score: parking_lot::RwLock::new(self.risk_score()),
            token_budget: self.token_budget,
            consumed_tokens: AtomicU32::new(0),
            tool_calls: AtomicU32::new(0),
        };
        next.clone_counters_from(self);
        next
    }

    pub fn with_risk_score(&self, risk_score: f32) -> Self {
        let next = Self {
            tool_policy: self.tool_policy.clone(),
            approval_handler: self.approval_handler(),
            trusted_workspaces: self.trusted_workspaces.clone(),
            security_handler: self.security_handler(),
            risk_score: parking_lot::RwLock::new(risk_score),
            token_budget: self.token_budget,
            consumed_tokens: AtomicU32::new(0),
            tool_calls: AtomicU32::new(0),
        };
        next.clone_counters_from(self);
        next
    }

    pub fn with_token_budget(&self, token_budget: Option<u32>) -> Self {
        let next = Self {
            tool_policy: self.tool_policy.clone(),
            approval_handler: self.approval_handler(),
            trusted_workspaces: self.trusted_workspaces.clone(),
            security_handler: self.security_handler(),
            risk_score: parking_lot::RwLock::new(self.risk_score()),
            token_budget,
            consumed_tokens: AtomicU32::new(0),
            tool_calls: AtomicU32::new(0),
        };
        next.clone_counters_from(self);
        next
    }

    pub fn inherit_full(&self) -> Arc<Self> {
        Arc::new(self.with_risk_score(self.risk_score()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopSecurity;

    #[async_trait::async_trait]
    impl crate::security::SecurityHandler for NoopSecurity {
        fn check_input(&self, text: &str) -> benshu_infra::traits::security::SanitizedOutput {
            benshu_infra::traits::security::SanitizedOutput {
                content: text.to_string(),
                warnings: Vec::new(),
                was_modified: false,
            }
        }

        fn check_output(
            &self,
            text: &str,
        ) -> (String, Vec<benshu_infra::traits::security::LeakDetection>) {
            (text.to_string(), Vec::new())
        }

        fn log_action(
            &self,
            _session_key: Option<&str>,
            _tool_name: &str,
            _arguments: &str,
            _success: bool,
            _output_preview: &str,
            _backup: Option<benshu_infra::skill::BackupInfo>,
        ) {
        }

        async fn retrieve_audit_logs(
            &self,
            _limit: usize,
        ) -> anyhow::Result<Vec<benshu_infra::traits::security::AuditLogRecord>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct NoopApproval;

    #[async_trait::async_trait]
    impl benshu_infra::traits::agent::ApprovalHandler for NoopApproval {
        async fn approve(
            &self,
            _tool_name: &str,
            _input: &str,
            _safety: SafetyLevel,
        ) -> anyhow::Result<bool> {
            Ok(true)
        }

        async fn approve_with_timeout(
            &self,
            _tool_name: &str,
            _input: &str,
            _safety: SafetyLevel,
            _timeout: std::time::Duration,
        ) -> anyhow::Result<bool> {
            Ok(true)
        }
    }

    fn make_context(limit: Option<u32>) -> GovernanceContext {
        GovernanceContext::new(
            RiskyToolPolicy::default(),
            Arc::new(NoopApproval),
            Vec::new(),
            Arc::new(NoopSecurity),
            0.4,
            limit,
        )
    }

    #[test]
    fn token_budget_snapshot_tracks_usage_and_exceeded() {
        let ctx = make_context(Some(100));
        let snapshot = ctx.register_token_usage(&TokenUsage {
            prompt_tokens: 30,
            completion_tokens: 80,
            total_tokens: 110,
        });

        assert_eq!(snapshot.used, 110);
        assert!(snapshot.exceeded);
        assert_eq!(snapshot.remaining, Some(0));
    }

    #[test]
    fn inherited_governance_preserves_counters() {
        let ctx = make_context(Some(200));
        ctx.register_token_usage(&TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        });
        ctx.register_tool_call();

        let inherited = ctx.inherit_full();
        let snapshot = inherited.budget_snapshot();
        assert_eq!(snapshot.used, 30);
        assert_eq!(snapshot.tool_calls, 1);
    }

    #[test]
    fn tool_decision_carries_execute_tools_scope() {
        let ctx = make_context(None);
        let decision = ctx.build_tool_decision(
            GovernanceScope::ExecuteTools,
            "shell.exec",
            ToolPolicy::RequiresApproval,
            SafetyLevel::Red,
            None,
            Some("needs operator approval".to_string()),
        );

        assert_eq!(decision.scope, GovernanceScope::ExecuteTools);
        assert_eq!(decision.authority, AuthorityRequirement::ExplicitApproval);
    }
}
