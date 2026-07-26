use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::artifact::{policy_handle_matches_task, push_unique};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityContract {
    pub artifact_type: String,
    pub min_chars: usize,
    pub max_chars: Option<usize>,
    pub min_citations: usize,
    pub required_sections: Vec<String>,
    pub required_section_label: String,
    pub require_title: bool,
    pub require_self_review: bool,
    pub require_stable_ledger_for_multi_step: bool,
    pub require_audit_before_export_for_multi_step: bool,
}

impl Default for QualityContract {
    fn default() -> Self {
        Self {
            artifact_type: "document".to_string(),
            min_chars: 800,
            max_chars: None,
            min_citations: 0,
            required_sections: vec!["标题".to_string(), "正文".to_string()],
            required_section_label: "document_sections".to_string(),
            require_title: true,
            require_self_review: false,
            require_stable_ledger_for_multi_step: false,
            require_audit_before_export_for_multi_step: false,
        }
    }
}

impl QualityContract {
    pub fn from_policy_value(task: &str, value: &Value) -> Self {
        let mut contract = Self::default();
        contract.apply_policy_value(task, value);
        contract
    }

    pub fn apply_policy_value(&mut self, _task: &str, value: &Value) {
        if let Some(artifact_type) = value
            .get("artifact_type")
            .or_else(|| value.get("artifact"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.artifact_type = artifact_type.to_string();
        }
        if let Some(min_chars) = value
            .get("min_chars")
            .or_else(|| value.get("minimum_chars"))
            .or_else(|| value.get("min_length_chars"))
            .and_then(Value::as_u64)
        {
            self.min_chars = min_chars as usize;
        }
        if let Some(max_chars) = value
            .get("max_chars")
            .or_else(|| value.get("maximum_chars"))
            .or_else(|| value.get("max_length_chars"))
            .and_then(Value::as_u64)
        {
            self.max_chars = Some(max_chars as usize);
        }
        if let Some(min_citations) = value
            .get("min_citations")
            .or_else(|| value.get("minimum_citations"))
            .or_else(|| value.get("min_sources"))
            .and_then(Value::as_u64)
        {
            self.min_citations = min_citations as usize;
        }
        if let Some(required_sections) = value
            .get("required_sections")
            .or_else(|| value.get("sections"))
            .and_then(Value::as_array)
        {
            let sections = required_sections
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            if !sections.is_empty() {
                self.required_sections = sections;
            }
        }
        if let Some(label) = value
            .get("required_section_label")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.required_section_label = label.to_string();
        }
        set_bool(value, "require_title", &mut self.require_title);
        set_bool(value, "require_self_review", &mut self.require_self_review);
        set_bool(value, "self_review_required", &mut self.require_self_review);
        set_bool(
            value,
            "require_stable_ledger_for_multi_step",
            &mut self.require_stable_ledger_for_multi_step,
        );
        set_bool(
            value,
            "require_audit_before_export_for_multi_step",
            &mut self.require_audit_before_export_for_multi_step,
        );
    }

    pub fn merge_prefer_policy(&mut self, policy_contract: &QualityContract) {
        self.artifact_type = policy_contract.artifact_type.clone();
        self.min_chars = policy_contract.min_chars;
        self.max_chars = policy_contract.max_chars;
        self.min_citations = policy_contract.min_citations;
        self.required_sections = policy_contract.required_sections.clone();
        self.required_section_label = policy_contract.required_section_label.clone();
        self.require_title = policy_contract.require_title;
        self.require_self_review = policy_contract.require_self_review;
        self.require_stable_ledger_for_multi_step =
            policy_contract.require_stable_ledger_for_multi_step;
        self.require_audit_before_export_for_multi_step =
            policy_contract.require_audit_before_export_for_multi_step;
    }

    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("artifact_type={}", self.artifact_type));
        lines.push(format!("min_chars={}", self.min_chars));
        if let Some(max_chars) = self.max_chars {
            lines.push(format!("max_chars={max_chars}"));
        }
        lines.push(format!("min_citations={}", self.min_citations));
        if !self.required_sections.is_empty() {
            lines.push(format!(
                "required_sections={}",
                self.required_sections.join(", ")
            ));
        }
        if self.require_title {
            lines.push("require_title=true".to_string());
        }
        if self.require_self_review {
            lines.push("require_self_review=true".to_string());
        }
        if self.require_stable_ledger_for_multi_step {
            lines.push("require_stable_ledger_for_multi_step=true".to_string());
        }
        if self.require_audit_before_export_for_multi_step {
            lines.push("require_audit_before_export_for_multi_step=true".to_string());
        }
        lines
    }
}

pub fn quality_contract_from_policy(task: &str, policy: &Value) -> Option<QualityContract> {
    for key in ["artifact_quality_contracts", "quality_contracts"] {
        if let Some(items) = policy.get(key).and_then(Value::as_array) {
            for item in items {
                if quality_contract_candidate_matches(task, item) {
                    return Some(QualityContract::from_policy_value(task, item));
                }
            }
        }
    }
    for key in ["artifact_quality_contract", "quality_contract"] {
        if let Some(item) = policy.get(key) {
            if quality_contract_candidate_matches(task, item) {
                return Some(QualityContract::from_policy_value(task, item));
            }
        }
    }

    let handles = policy.get("handles").and_then(Value::as_array)?;
    for handle in handles {
        if !policy_handle_matches_task(handle, task)
            && !quality_contract_candidate_matches(task, handle)
        {
            continue;
        }
        if let Some(contract) = handle
            .get("quality_contract")
            .or_else(|| handle.get("artifact_quality_contract"))
        {
            let mut quality = QualityContract::from_policy_value(task, handle);
            quality.apply_policy_value(task, contract);
            return Some(quality);
        }
    }
    None
}

pub fn collect_quality_terms(value: &Value, out: &mut Vec<String>) {
    for key in ["triggers", "aliases", "intents"] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            for item in items.iter().filter_map(Value::as_str) {
                push_unique(out, item.to_string());
            }
        }
    }
    for key in ["artifact", "artifact_type", "name"] {
        if let Some(item) = value.get(key).and_then(Value::as_str) {
            push_unique(out, item.to_string());
        }
    }
}

fn quality_contract_candidate_matches(task: &str, value: &Value) -> bool {
    if value
        .get("default")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }

    let lowered = task.to_ascii_lowercase();
    let mut terms = Vec::new();
    collect_quality_terms(value, &mut terms);
    terms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .any(|term| {
            if term.chars().any(|ch| !ch.is_ascii()) {
                task.contains(term)
            } else {
                lowered.contains(&term.to_ascii_lowercase())
            }
        })
}

fn set_bool(value: &Value, key: &str, target: &mut bool) {
    if let Some(flag) = value.get(key).and_then(Value::as_bool) {
        *target = flag;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quality_contract_reads_handle_level_policy() {
        let policy = json!({
            "handles": [{
                "artifact": "written_document",
                "triggers": ["报告"],
                    "quality_contract": {
                        "min_chars": 1200,
                        "max_chars": 2400,
                        "required_sections": ["标题", "分析", "结论"],
                        "require_self_review": true
                }
            }]
        });

        let contract = quality_contract_from_policy("写一份报告", &policy).expect("contract");
        assert_eq!(contract.artifact_type, "written_document");
        assert_eq!(contract.min_chars, 1200);
        assert_eq!(contract.max_chars, Some(2400));
        assert!(contract.require_self_review);
        assert_eq!(contract.required_sections, vec!["标题", "分析", "结论"]);
    }
}
