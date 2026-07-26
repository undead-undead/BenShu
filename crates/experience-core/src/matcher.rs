use crate::model::{current_time_ms, ExperienceScope, TaskExperience};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExperienceQuery {
    pub task: String,
    #[serde(default)]
    pub scope: Option<ExperienceScope>,
    #[serde(default)]
    pub worker_role: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub include_expired: bool,
    #[serde(default)]
    pub min_confidence: f32,
    #[serde(default = "current_time_ms")]
    pub now_ms: i64,
}

impl ExperienceQuery {
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            scope: None,
            worker_role: None,
            tool_name: None,
            limit: default_limit(),
            include_expired: false,
            min_confidence: 0.0,
            now_ms: current_time_ms(),
        }
    }
}

fn default_limit() -> usize {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExperienceMatch {
    pub experience: TaskExperience,
    pub score: f32,
    pub reasons: Vec<String>,
}

pub fn rank_experiences(
    experiences: impl IntoIterator<Item = TaskExperience>,
    query: &ExperienceQuery,
) -> Vec<ExperienceMatch> {
    let mut matches = experiences
        .into_iter()
        .filter_map(|experience| score_experience(experience, query))
        .collect::<Vec<_>>();

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.experience.updated_at_ms.cmp(&a.experience.updated_at_ms))
    });
    matches.truncate(query.limit.max(1));
    matches
}

fn score_experience(
    experience: TaskExperience,
    query: &ExperienceQuery,
) -> Option<ExperienceMatch> {
    if !query.include_expired && !experience.is_reusable_at(query.now_ms) {
        return None;
    }
    if experience.confidence < query.min_confidence {
        return None;
    }

    let mut score = experience.confidence.clamp(0.0, 1.0) * 0.55;
    let mut reasons = Vec::new();

    let overlap = text_overlap_score(&query.task, &experience_search_text(&experience));
    if overlap > 0.0 {
        score += overlap * 0.3;
        reasons.push(format!("task_overlap:{overlap:.2}"));
    }

    if let Some(scope) = &query.scope {
        if *scope == experience.scope {
            score += 0.1;
            reasons.push("scope_match".to_string());
        } else if !query.include_expired {
            return None;
        }
    }

    if let Some(worker) = query.worker_role.as_deref() {
        if experience
            .worker_role
            .as_deref()
            .is_some_and(|role| role.eq_ignore_ascii_case(worker))
        {
            score += 0.08;
            reasons.push("worker_match".to_string());
        }
    }

    if let Some(tool_name) = query.tool_name.as_deref() {
        if experience
            .tool_names
            .iter()
            .any(|tool| tool.eq_ignore_ascii_case(tool_name))
        {
            score += 0.08;
            reasons.push("tool_match".to_string());
        }
    }

    if let Some(last_verified_at_ms) = experience.last_verified_at_ms {
        let age_ms = query.now_ms.saturating_sub(last_verified_at_ms).max(0) as f32;
        let age_days = age_ms / 86_400_000.0;
        let freshness = (1.0 / (1.0 + age_days / 14.0)).clamp(0.0, 1.0);
        score += freshness * 0.06;
        reasons.push(format!("freshness:{freshness:.2}"));
    }

    if experience.usage.preflight_fail_count > experience.usage.preflight_pass_count {
        score *= 0.8;
        reasons.push("preflight_failure_penalty".to_string());
    }
    if experience.usage.failure_count > experience.usage.success_count {
        score *= 0.75;
        reasons.push("task_failure_penalty".to_string());
    }

    if score <= 0.0 {
        None
    } else {
        Some(ExperienceMatch {
            experience,
            score: score.clamp(0.0, 1.0),
            reasons,
        })
    }
}

fn experience_search_text(experience: &TaskExperience) -> String {
    let mut parts = vec![
        experience.task_signature.clone(),
        experience.task_summary.clone(),
        experience.scope.as_key(),
    ];
    if let Some(worker) = &experience.worker_role {
        parts.push(worker.clone());
    }
    parts.extend(experience.tool_names.iter().cloned());
    parts.extend(experience.hints.iter().cloned());
    parts.extend(experience.anti_patterns.iter().cloned());
    for step in &experience.successful_steps {
        parts.push(step.label.clone());
        parts.push(step.action.clone());
    }
    parts.join(" ")
}

fn text_overlap_score(query: &str, candidate: &str) -> f32 {
    let query_terms = fragments(query);
    if query_terms.is_empty() {
        return 0.0;
    }
    let candidate_terms = fragments(candidate);
    if candidate_terms.is_empty() {
        return 0.0;
    }
    let hits = query_terms
        .iter()
        .filter(|term| candidate_terms.contains(*term) || candidate.to_lowercase().contains(*term))
        .count();
    (hits as f32 / query_terms.len() as f32).clamp(0.0, 1.0)
}

fn fragments(text: &str) -> BTreeSet<String> {
    let lowered = text.to_lowercase();
    let mut terms = lowered
        .split(|ch: char| !ch.is_alphanumeric())
        .filter_map(|term| {
            let trimmed = term.trim();
            if trimmed.chars().count() >= 2 {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();

    let cjk_chars = lowered.chars().filter(|ch| is_cjk(*ch)).collect::<Vec<_>>();
    for window in cjk_chars.windows(2) {
        terms.insert(window.iter().collect());
    }
    terms
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
    )
}
