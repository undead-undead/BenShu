use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::artifact::{
    policy_handle_matches_task, push_policy_string_array, push_policy_string_field, push_unique,
};
use super::quality::{quality_contract_from_policy, QualityContract};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PolicyPhase {
    #[default]
    TaskEntry,
    Delegation,
    ToolCall,
    ArtifactValidation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskPolicyInput {
    pub task: String,
    pub full_user_request: Option<String>,
    pub requested_role: Option<String>,
    pub worker_role: Option<String>,
    pub worker_tools: Vec<String>,
    pub phase: PolicyPhase,
}

impl TaskPolicyInput {
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            ..Self::default()
        }
    }

    pub fn with_full_user_request(mut self, full_user_request: Option<&str>) -> Self {
        self.full_user_request = full_user_request
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        self
    }

    pub fn with_requested_role(mut self, requested_role: impl Into<String>) -> Self {
        self.requested_role = Some(requested_role.into());
        self
    }

    pub fn with_worker(mut self, worker_role: impl Into<String>, tools: &[String]) -> Self {
        self.worker_role = Some(worker_role.into());
        self.worker_tools = tools.to_vec();
        self
    }

    pub fn with_phase(mut self, phase: PolicyPhase) -> Self {
        self.phase = phase;
        self
    }

    pub fn match_surface(&self) -> String {
        let mut parts = vec![self.task.trim().to_string()];
        if let Some(full) = self.full_user_request.as_deref() {
            parts.push(full.trim().to_string());
        }
        if let Some(role) = self.requested_role.as_deref() {
            parts.push(role.trim().to_string());
        }
        if let Some(role) = self.worker_role.as_deref() {
            parts.push(role.trim().to_string());
        }
        parts
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimePolicyBundle {
    pub phase: PolicyPhase,
    pub matched_policy_count: usize,
    pub matched_artifacts: Vec<String>,
    pub matched_intents: Vec<String>,
    pub artifact_hints: Vec<String>,
    pub evidence_hints: Vec<String>,
    pub freshness_hints: Vec<String>,
    pub direct_record_hints: Vec<String>,
    pub site_hints: Vec<String>,
    pub source_hints: Vec<String>,
    pub worker_tools: Vec<String>,
    pub quality_contract: Option<QualityContract>,
    pub match_reasons: Vec<String>,
}

impl RuntimePolicyBundle {
    pub fn is_empty(&self) -> bool {
        self.matched_policy_count == 0
            && self.quality_contract.is_none()
            && self.worker_tools.is_empty()
    }

    pub fn compact_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("phase={:?}", self.phase));
        if !self.matched_artifacts.is_empty() {
            lines.push(format!("artifacts={}", self.matched_artifacts.join(", ")));
        }
        if !self.matched_intents.is_empty() {
            lines.push(format!("intents={}", self.matched_intents.join(", ")));
        }
        if !self.evidence_hints.is_empty() {
            lines.push(format!("evidence_hints={}", self.evidence_hints.join(", ")));
        }
        if !self.site_hints.is_empty() {
            lines.push(format!("site_hints={}", self.site_hints.join(", ")));
        }
        if let Some(contract) = &self.quality_contract {
            lines.push(format!(
                "quality_contract={}",
                contract.summary_lines().join("; ")
            ));
        }
        lines.join("\n")
    }
}

pub struct RuntimePolicyResolver;

impl RuntimePolicyResolver {
    pub fn resolve(input: TaskPolicyInput, policies: &[Value]) -> RuntimePolicyBundle {
        let surface = input.match_surface();
        let mut bundle = RuntimePolicyBundle {
            phase: input.phase,
            worker_tools: input.worker_tools,
            ..RuntimePolicyBundle::default()
        };

        for policy in policies {
            Self::apply_policy(&surface, policy, &mut bundle);
        }

        bundle
    }

    fn apply_policy(surface: &str, policy: &Value, bundle: &mut RuntimePolicyBundle) {
        if let Some(contract) = quality_contract_from_policy(surface, policy) {
            bundle.quality_contract = Some(contract);
        }
        let Some(handles) = policy.get("handles").and_then(Value::as_array) else {
            return;
        };
        for handle in handles {
            if !policy_handle_matches_task(handle, surface) {
                continue;
            }
            bundle.matched_policy_count += 1;
            if let Some(artifact) = handle.get("artifact").and_then(Value::as_str) {
                push_unique(&mut bundle.matched_artifacts, artifact.to_string());
                push_unique(&mut bundle.artifact_hints, artifact.to_string());
                bundle
                    .match_reasons
                    .push(format!("artifact_policy_handle:{artifact}"));
            }
            push_policy_string_array(handle, "intents", &mut bundle.matched_intents);
            push_policy_string_array(handle, "intents", &mut bundle.artifact_hints);
            push_policy_string_array(handle, "artifact_hints", &mut bundle.artifact_hints);
            push_policy_string_array(handle, "evidence_hints", &mut bundle.evidence_hints);
            push_policy_string_array(handle, "freshness_hints", &mut bundle.freshness_hints);
            push_policy_string_array(
                handle,
                "direct_record_hints",
                &mut bundle.direct_record_hints,
            );
            push_policy_string_array(handle, "sources", &mut bundle.source_hints);
            push_policy_string_array(handle, "source_hints", &mut bundle.source_hints);
            push_policy_string_field(handle, "site", &mut bundle.site_hints);
            push_policy_string_array(handle, "sites", &mut bundle.site_hints);
            push_policy_string_array(handle, "site_hints", &mut bundle.site_hints);
            push_policy_string_array(handle, "domains", &mut bundle.site_hints);
            push_policy_string_array(handle, "preferred_hosts", &mut bundle.site_hints);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolver_builds_one_bundle_for_task_phase() {
        let policy = json!({
            "handles": [{
                "artifact": "written_document",
                "triggers": ["报告"],
                "intents": ["draft", "export"],
                "tools": ["writing"],
                "quality_contract": {
                    "min_chars": 1000,
                    "require_title": true
                }
            }]
        });
        let bundle = RuntimePolicyResolver::resolve(
            TaskPolicyInput::new("写一份报告").with_phase(PolicyPhase::Delegation),
            &[policy],
        );

        assert_eq!(bundle.phase, PolicyPhase::Delegation);
        assert_eq!(bundle.matched_artifacts, vec!["written_document"]);
        assert!(bundle.quality_contract.is_some());
    }
}
