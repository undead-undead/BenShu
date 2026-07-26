use crate::model::{ExperienceScope, TaskExperience, DEFAULT_EXPERIENCE_NAMESPACE};
use serde::{Deserialize, Serialize};

pub const EXPERIENCE_INDEX_NAMESPACE: &str = "system_experience";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExperienceIndexProjection {
    pub namespace: String,
    pub experience_id: String,
    pub scope: ExperienceScope,
    pub title: String,
    pub text: String,
    pub confidence: f32,
    pub updated_at_ms: i64,
    pub expires_at_ms: Option<i64>,
}

impl ExperienceIndexProjection {
    pub fn from_experience(exp: &TaskExperience) -> Self {
        let mut parts = Vec::new();
        parts.push(exp.task_signature.clone());
        parts.push(exp.task_summary.clone());
        parts.extend(exp.hints.iter().cloned());
        parts.extend(
            exp.anti_patterns
                .iter()
                .map(|value| format!("Avoid: {value}")),
        );
        for step in &exp.successful_steps {
            parts.push(format!("{}: {}", step.label, step.action));
        }

        Self {
            namespace: exp.namespace.clone(),
            experience_id: exp.id.clone(),
            scope: exp.scope.clone(),
            title: exp.task_summary.clone(),
            text: parts
                .into_iter()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            confidence: exp.confidence,
            updated_at_ms: exp.updated_at_ms,
            expires_at_ms: exp.expires_at_ms,
        }
    }

    pub fn engram_collection(&self) -> &str {
        EXPERIENCE_INDEX_NAMESPACE
    }

    pub fn engram_path(&self) -> String {
        format!("{}/{}", self.scope_key(), self.experience_id)
    }

    pub fn scope_key(&self) -> String {
        self.scope.as_key()
    }
}

pub fn is_experience_namespace(namespace: &str) -> bool {
    let namespace = namespace.trim();
    namespace == EXPERIENCE_INDEX_NAMESPACE || namespace == DEFAULT_EXPERIENCE_NAMESPACE
}
