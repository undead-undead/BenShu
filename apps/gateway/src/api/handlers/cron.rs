use crate::api::state::{AppError, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct CronJobDto {
    pub id: String,
    pub name: String,
    pub schedule: serde_json::Value,
    pub payload_kind: String,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub error_count: u32,
}

#[cfg(feature = "cron")]
fn to_cron_dto(job: &benshu_scheduler::CronJob) -> CronJobDto {
    let schedule = serde_json::to_value(&job.schedule).unwrap_or_else(|_| serde_json::json!({}));
    let payload_kind = match &job.payload {
        benshu_scheduler::JobPayload::AgentTurn { .. } => "agentTurn",
        benshu_scheduler::JobPayload::SummarizeDoc { .. } => "summarizeDoc",
        benshu_scheduler::JobPayload::DistillLogs { .. } => "distillLogs",
        benshu_scheduler::JobPayload::ConsolidateMemory { .. } => "consolidateMemory",
    }
    .to_string();

    CronJobDto {
        id: job.id.to_string(),
        name: job.name.clone(),
        schedule,
        payload_kind,
        enabled: job.enabled,
        last_run_at: job.last_run_at.map(|t| t.to_rfc3339()),
        error_count: job.error_count,
    }
}

pub async fn list_cron_jobs(State(state): State<AppState>) -> Json<Vec<CronJobDto>> {
    #[cfg(feature = "cron")]
    if let Some(scheduler) = state.kernel.coordinator().scheduler.get() {
        let jobs = scheduler.list_jobs();
        return Json(jobs.iter().map(to_cron_dto).collect());
    }
    Json(vec![])
}

#[derive(Deserialize)]
pub struct CreateCronJobRequest {
    pub name: String,
    /// "every" | "at" | "cron"
    pub schedule_kind: String,
    /// interval seconds (for "every")
    pub interval_secs: Option<u64>,
    /// cron expression (for "cron")
    pub cron_expr: Option<String>,
    /// ISO8601 timestamp (for "at")
    pub at: Option<String>,
    /// "agentTurn" prompt
    pub prompt: Option<String>,
    /// target role
    pub role: Option<String>,
}

pub async fn create_cron_job(
    State(state): State<AppState>,
    Json(req): Json<CreateCronJobRequest>,
) -> Result<Json<CronJobDto>, AppError> {
    #[cfg(feature = "cron")]
    {
        use benshu_infra::agent::AgentRole;
        use benshu_scheduler::{JobPayload, JobSchedule};

        let schedule = match req.schedule_kind.as_str() {
            "every" => JobSchedule::Every {
                interval_secs: req.interval_secs.unwrap_or(3600),
            },
            "at" => {
                let ts = req.at.as_deref().unwrap_or("");
                let at = chrono::DateTime::parse_from_rfc3339(ts)
                    .map(|t| t.with_timezone(&chrono::Utc))
                    .map_err(|e| anyhow::anyhow!("Invalid timestamp: {}", e))?;
                JobSchedule::At { at }
            }
            _ => JobSchedule::Cron {
                expr: req.cron_expr.unwrap_or_else(|| "0 * * * *".to_string()),
            },
        };

        let prime_role = state.kernel.coordinator().primary_role();
        let role = match req.role.as_deref().unwrap_or("benshu") {
            "benshu" => prime_role,
            "researcher" => AgentRole::Researcher,
            "trader" => AgentRole::Trader,
            "risk_analyst" => AgentRole::RiskAnalyst,
            "strategist" => AgentRole::Strategist,
            custom => AgentRole::Custom(custom.to_string()),
        };

        let payload = JobPayload::AgentTurn {
            role,
            prompt: req
                .prompt
                .unwrap_or_else(|| "Perform a scheduled task.".to_string()),
        };

        let scheduler = state
            .kernel
            .coordinator()
            .scheduler
            .get()
            .ok_or_else(|| anyhow::anyhow!("Scheduler not initialized"))?;

        let id = scheduler
            .add_job(req.name.clone(), schedule, payload)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let jobs = scheduler.list_jobs();
        let job = jobs
            .iter()
            .find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Job not found after creation"))?;

        return Ok(Json(to_cron_dto(job)));
    }
    #[cfg(not(feature = "cron"))]
    Err(anyhow::anyhow!("Cron feature not enabled").into())
}

pub async fn toggle_cron_job(State(_state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    tracing::info!("Toggle cron job {}", id);
    StatusCode::NO_CONTENT
}

pub async fn run_cron_job(State(_state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    tracing::info!("Manual trigger cron job {}", id);
    StatusCode::ACCEPTED
}

pub async fn delete_cron_job(State(state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    #[cfg(feature = "cron")]
    if let Some(scheduler) = state.kernel.coordinator().scheduler.get() {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            match scheduler.remove_job(uuid).await {
                Ok(_) => return StatusCode::OK,
                Err(_) => return StatusCode::NOT_FOUND,
            }
        }
    }
    StatusCode::BAD_REQUEST
}
