use std::collections::BTreeMap;

use axum::{
    extract::{Path, Query, State},
    Json,
};
use benshu_experience_core::{
    EvidenceRefs, ExperienceIndexProjection, ExperienceMatch, ExperienceQuery, ExperienceScope,
    ExperienceStep, FailureSignature, PreflightCheck, TaskExperience,
};
use serde::{Deserialize, Serialize};

use crate::api::state::{AppError, AppState};

#[derive(Debug, Deserialize)]
pub struct ListExperienceParams {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct CreateExperienceRequest {
    pub task_signature: String,
    pub task_summary: String,
    pub scope: ExperienceScope,
    #[serde(default)]
    pub worker_role: Option<String>,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub successful_steps: Vec<ExperienceStep>,
    #[serde(default)]
    pub required_preflight: Vec<PreflightCheck>,
    #[serde(default)]
    pub failure_signatures: Vec<FailureSignature>,
    #[serde(default)]
    pub evidence_refs: EvidenceRefs,
    #[serde(default)]
    pub hints: Vec<String>,
    #[serde(default)]
    pub anti_patterns: Vec<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub ttl_seconds: Option<i64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct ExperienceResultRequest {
    pub passed: Option<bool>,
    pub succeeded: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ExperienceStatsResponse {
    pub path: String,
    pub total_experiences: u64,
    pub index_scope_task_entries: u64,
    pub index_worker_entries: u64,
    pub index_tool_entries: u64,
    pub index_status_entries: u64,
}

impl From<benshu_experience_core::ExperienceStoreStats> for ExperienceStatsResponse {
    fn from(stats: benshu_experience_core::ExperienceStoreStats) -> Self {
        Self {
            path: stats.path.display().to_string(),
            total_experiences: stats.total_experiences,
            index_scope_task_entries: stats.index_scope_task_entries,
            index_worker_entries: stats.index_worker_entries,
            index_tool_entries: stats.index_tool_entries,
            index_status_entries: stats.index_status_entries,
        }
    }
}

pub async fn stats(
    State(state): State<AppState>,
) -> Result<Json<ExperienceStatsResponse>, AppError> {
    let stats = state.kernel.experience_store().stats()?;
    Ok(Json(stats.into()))
}

pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<ListExperienceParams>,
) -> Result<Json<Vec<TaskExperience>>, AppError> {
    let mut records = state.kernel.experience_store().list()?;
    records.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    if let Some(limit) = params.limit {
        records.truncate(limit.max(1));
    }
    Ok(Json(records))
}

pub async fn query(
    State(state): State<AppState>,
    Json(mut query): Json<ExperienceQuery>,
) -> Result<Json<Vec<ExperienceMatch>>, AppError> {
    query.limit = query.limit.clamp(1, 20);
    let matches = state.kernel.experience_store().query(&query)?;
    Ok(Json(matches))
}

pub async fn create(
    State(state): State<AppState>,
    Json(request): Json<CreateExperienceRequest>,
) -> Result<Json<TaskExperience>, AppError> {
    let mut experience =
        TaskExperience::new(request.task_signature, request.task_summary, request.scope);
    experience.worker_role = request.worker_role;
    experience.tool_names = request.tool_names;
    experience.successful_steps = request.successful_steps;
    experience.required_preflight = request.required_preflight;
    experience.failure_signatures = request.failure_signatures;
    experience.evidence_refs = request.evidence_refs;
    experience.hints = request.hints;
    experience.anti_patterns = request.anti_patterns;
    if let Some(confidence) = request.confidence {
        experience.confidence = confidence;
    }
    experience.ttl_seconds = request.ttl_seconds;
    experience.metadata = request.metadata;

    let stored = state.kernel.experience_store().upsert(experience)?;
    Ok(Json(stored))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Option<TaskExperience>>, AppError> {
    Ok(Json(state.kernel.experience_store().get(&id)?))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = state.kernel.experience_store().delete(&id)?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

pub async fn mark_selected(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Option<TaskExperience>>, AppError> {
    Ok(Json(state.kernel.experience_store().mark_selected(&id)?))
}

pub async fn record_preflight(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ExperienceResultRequest>,
) -> Result<Json<Option<TaskExperience>>, AppError> {
    let passed = request.passed.unwrap_or(false);
    Ok(Json(
        state
            .kernel
            .experience_store()
            .record_preflight_result(&id, passed)?,
    ))
}

pub async fn record_task_result(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ExperienceResultRequest>,
) -> Result<Json<Option<TaskExperience>>, AppError> {
    let succeeded = request.succeeded.unwrap_or(false);
    Ok(Json(
        state
            .kernel
            .experience_store()
            .record_task_result(&id, succeeded)?,
    ))
}

pub async fn projection(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Option<ExperienceIndexProjection>>, AppError> {
    let projection = state
        .kernel
        .experience_store()
        .get(&id)?
        .map(|experience| ExperienceIndexProjection::from_experience(&experience));
    Ok(Json(projection))
}
