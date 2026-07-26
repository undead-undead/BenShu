use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskComplexity {
    pub estimated_steps: usize,
    pub risk_score: f32,
    pub rationale: String,
    pub max_steps_override: Option<usize>,
    pub intent: String,
}

pub fn sanitize_task_complexity(mut complexity: TaskComplexity) -> TaskComplexity {
    if complexity.intent.is_empty() {
        complexity.intent = "general_query".to_string();
    }
    complexity.estimated_steps = complexity.estimated_steps.clamp(1, 100);
    complexity.risk_score = complexity.risk_score.clamp(0.0, 1.0);
    if let Some(ref mut max_steps_override) = complexity.max_steps_override {
        *max_steps_override = (*max_steps_override).clamp(1, 200);
    }
    complexity
}
