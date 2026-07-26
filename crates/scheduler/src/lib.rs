use async_trait::async_trait;
use benshu_infra::{AgentRole, HealthCheck, HealthStatus};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Weak};
use tokio_cron_scheduler::JobScheduler;
use tracing::{error, info};
use uuid::Uuid;

#[cfg(feature = "persistence")]
use redb::{Database, ReadableTable, TableDefinition};

// Re-exports/Constants
#[cfg(feature = "persistence")]
const CRON_JOBS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("cron_jobs");
#[cfg(feature = "persistence")]
const CRON_EXECUTIONS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("cron_executions");

// --- Errors ---
#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Execution failed: {0}")]
    Execution(String),
    #[error("Handler dropped")]
    HandlerDropped,
}

pub type Result<T> = std::result::Result<T, SchedulerError>;

// --- Traits ---

#[async_trait]
pub trait JobHandler: Send + Sync {
    async fn execute(&self, name: &str, payload: &JobPayload) -> Result<Option<String>>;
}

#[cfg(feature = "persistence")]
#[async_trait]
pub trait CronStore: Send + Sync {
    async fn save_job(&self, job: &CronJob) -> Result<()>;
    async fn remove_job(&self, id: Uuid) -> Result<()>;
    async fn load_all_jobs(&self) -> Result<Vec<CronJob>>;
    async fn save_execution_record(&self, record: &JobExecutionRecord) -> Result<()>;
    async fn load_execution_records(
        &self,
        job_id: Uuid,
        limit: usize,
    ) -> Result<Vec<JobExecutionRecord>>;
}

// --- Data Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobExecutionRecord {
    pub job_id: Uuid,
    pub job_name: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub success: bool,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JobSchedule {
    #[serde(rename_all = "camelCase")]
    At {
        #[serde(with = "chrono::serde::ts_seconds")]
        at: DateTime<Utc>,
    },
    #[serde(rename_all = "camelCase")]
    Every { interval_secs: u64 },
    #[serde(rename_all = "camelCase")]
    Cron { expr: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JobPayload {
    #[serde(rename_all = "camelCase")]
    AgentTurn { role: AgentRole, prompt: String },
    #[serde(rename_all = "camelCase")]
    SummarizeDoc {
        collection: String,
        path: String,
        content: String,
    },
    #[serde(rename_all = "camelCase")]
    DistillLogs { limit: usize },
    #[serde(rename_all = "camelCase")]
    ConsolidateMemory {
        limit: usize,
        agent_id: Option<String>,
        global_context: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: Uuid,
    pub name: String,
    pub schedule: JobSchedule,
    pub payload: JobPayload,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub error_count: u32,
    pub max_retries: u32,
    pub priority: u8,
}

// --- Persistence Implementation ---

#[cfg(feature = "persistence")]
pub struct RedbCronStore {
    db: Arc<Database>,
}

#[cfg(feature = "persistence")]
impl RedbCronStore {
    pub fn new(path: &str) -> Result<Self> {
        let db = Database::create(path)
            .map_err(|e| SchedulerError::Internal(format!("Failed to create Redb: {}", e)))?;

        let write_txn = db
            .begin_write()
            .map_err(|e| SchedulerError::Internal(format!("Failed to begin write txn: {}", e)))?;
        {
            let _ = write_txn.open_table(CRON_JOBS_TABLE).map_err(|e| {
                SchedulerError::Internal(format!("Failed to open cron table: {}", e))
            })?;
            let _ = write_txn.open_table(CRON_EXECUTIONS_TABLE).map_err(|e| {
                SchedulerError::Internal(format!("Failed to open executions table: {}", e))
            })?;
        }
        write_txn
            .commit()
            .map_err(|e| SchedulerError::Internal(format!("Failed to commit init txn: {}", e)))?;

        Ok(Self { db: Arc::new(db) })
    }
}

#[cfg(feature = "persistence")]
#[async_trait]
impl CronStore for RedbCronStore {
    async fn save_job(&self, job: &CronJob) -> Result<()> {
        let id_str = job.id.to_string();
        let data = serde_json::to_vec(job)
            .map_err(|e| SchedulerError::Internal(format!("Failed to serialize job: {}", e)))?;

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| SchedulerError::Internal(format!("Failed to begin write txn: {}", e)))?;
        {
            let mut table = write_txn.open_table(CRON_JOBS_TABLE).map_err(|e| {
                SchedulerError::Internal(format!("Failed to open cron table: {}", e))
            })?;
            table
                .insert(id_str.as_str(), data.as_slice())
                .map_err(|e| SchedulerError::Internal(format!("Failed to insert job: {}", e)))?;
        }
        write_txn
            .commit()
            .map_err(|e| SchedulerError::Internal(format!("Failed to commit save txn: {}", e)))?;

        Ok(())
    }

    async fn remove_job(&self, id: Uuid) -> Result<()> {
        let id_str = id.to_string();
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| SchedulerError::Internal(format!("Failed to begin write txn: {}", e)))?;
        {
            let mut table = write_txn.open_table(CRON_JOBS_TABLE).map_err(|e| {
                SchedulerError::Internal(format!("Failed to open cron table: {}", e))
            })?;
            table
                .remove(id_str.as_str())
                .map_err(|e| SchedulerError::Internal(format!("Failed to remove job: {}", e)))?;
        }
        write_txn
            .commit()
            .map_err(|e| SchedulerError::Internal(format!("Failed to commit remove txn: {}", e)))?;
        Ok(())
    }

    async fn load_all_jobs(&self) -> Result<Vec<CronJob>> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| SchedulerError::Internal(format!("Failed to begin read txn: {}", e)))?;
        let table = read_txn
            .open_table(CRON_JOBS_TABLE)
            .map_err(|e| SchedulerError::Internal(format!("Failed to open cron table: {}", e)))?;

        let mut jobs = Vec::new();
        for entry in table
            .iter()
            .map_err(|e| SchedulerError::Internal(format!("Failed to iter table: {}", e)))?
        {
            let (_, value) = entry
                .map_err(|e| SchedulerError::Internal(format!("Failed to get entry: {}", e)))?;
            let job: CronJob = serde_json::from_slice(value.value()).map_err(|e| {
                SchedulerError::Internal(format!("Failed to deserialize job: {}", e))
            })?;
            jobs.push(job);
        }
        Ok(jobs)
    }

    async fn save_execution_record(&self, record: &JobExecutionRecord) -> Result<()> {
        let key = format!(
            "{}:{}",
            record.job_id,
            record.start_time.timestamp_nanos_opt().unwrap_or(0)
        );
        let data = serde_json::to_vec(record)
            .map_err(|e| SchedulerError::Internal(format!("Failed to serialize record: {}", e)))?;

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| SchedulerError::Internal(format!("Failed to begin write txn: {}", e)))?;
        {
            let mut table = write_txn.open_table(CRON_EXECUTIONS_TABLE).map_err(|e| {
                SchedulerError::Internal(format!("Failed to open executions table: {}", e))
            })?;
            table
                .insert(key.as_str(), data.as_slice())
                .map_err(|e| SchedulerError::Internal(format!("Failed to insert record: {}", e)))?;
        }
        write_txn
            .commit()
            .map_err(|e| SchedulerError::Internal(format!("Failed to commit record txn: {}", e)))?;
        Ok(())
    }

    async fn load_execution_records(
        &self,
        job_id: Uuid,
        limit: usize,
    ) -> Result<Vec<JobExecutionRecord>> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| SchedulerError::Internal(format!("Failed to begin read txn: {}", e)))?;
        let table = read_txn.open_table(CRON_EXECUTIONS_TABLE).map_err(|e| {
            SchedulerError::Internal(format!("Failed to open executions table: {}", e))
        })?;

        let prefix = job_id.to_string();
        let mut records = Vec::new();

        for entry in table
            .range(prefix.as_str()..)
            .map_err(|e| SchedulerError::Internal(format!("Failed to range table: {}", e)))?
        {
            let (key, value) = entry
                .map_err(|e| SchedulerError::Internal(format!("Failed to get entry: {}", e)))?;
            if !key.value().starts_with(&prefix) {
                break;
            }
            let record: JobExecutionRecord =
                serde_json::from_slice(value.value()).map_err(|e| {
                    SchedulerError::Internal(format!("Failed to deserialize record: {}", e))
                })?;
            records.push(record);
            if records.len() >= limit {
                break;
            }
        }
        Ok(records)
    }
}

// --- Scheduler Core ---

pub struct Scheduler {
    jobs: DashMap<Uuid, CronJob>,
    scheduler: tokio::sync::Mutex<JobScheduler>,
    handler: Weak<dyn JobHandler>,
    store: Option<Box<dyn CronStore>>,
    sys: tokio::sync::Mutex<sysinfo::System>,
    active_jobs: Arc<tokio::sync::Semaphore>,
}

impl Scheduler {
    pub async fn new(
        handler: Weak<dyn JobHandler>,
        store: Option<Box<dyn CronStore>>,
    ) -> Arc<Self> {
        let scheduler = JobScheduler::new()
            .await
            .expect("Failed to initialize JobScheduler");
        Arc::new(Self {
            jobs: DashMap::new(),
            scheduler: tokio::sync::Mutex::new(scheduler),
            handler,
            store,
            sys: tokio::sync::Mutex::new(sysinfo::System::new_all()),
            active_jobs: Arc::new(tokio::sync::Semaphore::new(3)),
        })
    }

    pub async fn load_jobs(self: &Arc<Self>) -> Result<()> {
        if let Some(store) = &self.store {
            let jobs = store.load_all_jobs().await?;
            info!("Loading {} jobs from cron store", jobs.len());
            for cron_job in jobs {
                self.clone().register_job_runtime(cron_job).await?;
            }
        }
        Ok(())
    }

    fn register_job_runtime(
        self: Arc<Self>,
        mut cron_job: CronJob,
    ) -> BoxFuture<'static, Result<Uuid>> {
        Box::pin(async move {
            let handler_weak = self.handler.clone();
            let scheduler_weak = Arc::downgrade(&self);
            let payload_clone = cron_job.payload.clone();
            let name_clone = cron_job.name.clone();
            let id_original = cron_job.id;

            let job = match &cron_job.schedule {
                JobSchedule::At { at } => {
                    let now = Utc::now();
                    if *at <= now {
                        return Ok(id_original);
                    }
                    let duration = at.signed_duration_since(now).to_std().unwrap_or_default();

                    tokio_cron_scheduler::Job::new_one_shot_async(duration, move |uuid, _l| {
                        let handler_weak = handler_weak.clone();
                        let scheduler_weak = scheduler_weak.clone();
                        let payload = payload_clone.clone();
                        let name = name_clone.clone();
                        Box::pin(async move {
                            let success = Self::execute_payload(&handler_weak, &name, &payload)
                                .await
                                .is_ok();
                            if let Some(s) = scheduler_weak.upgrade() {
                                let _ = s.update_job_status(uuid, success).await;
                            }
                        })
                    })
                    .map_err(|e| {
                        SchedulerError::Internal(format!("Failed to create one-shot job: {}", e))
                    })?
                }
                JobSchedule::Every { interval_secs } => {
                    let duration = std::time::Duration::from_secs(*interval_secs);
                    tokio_cron_scheduler::Job::new_repeated_async(duration, move |uuid, _l| {
                        let handler_weak = handler_weak.clone();
                        let scheduler_weak = scheduler_weak.clone();
                        let payload = payload_clone.clone();
                        let name = name_clone.clone();
                        Box::pin(async move {
                            let success = Self::execute_payload(&handler_weak, &name, &payload)
                                .await
                                .is_ok();
                            if let Some(s) = scheduler_weak.upgrade() {
                                let _ = s.update_job_status(uuid, success).await;
                            }
                        })
                    })
                    .map_err(|e| {
                        SchedulerError::Internal(format!("Failed to create repeated job: {}", e))
                    })?
                }
                JobSchedule::Cron { expr } => {
                    tokio_cron_scheduler::Job::new_async(expr.as_str(), move |uuid, _l| {
                        let handler_weak = handler_weak.clone();
                        let scheduler_weak = scheduler_weak.clone();
                        let payload = payload_clone.clone();
                        let name = name_clone.clone();
                        Box::pin(async move {
                            let success = Self::execute_payload(&handler_weak, &name, &payload)
                                .await
                                .is_ok();
                            if let Some(s) = scheduler_weak.upgrade() {
                                let _ = s.update_job_status(uuid, success).await;
                            }
                        })
                    })
                    .map_err(|e| {
                        SchedulerError::Internal(format!("Failed to create cron job: {}", e))
                    })?
                }
            };

            let sched = self.scheduler.lock().await;
            let registered_id = sched
                .add(job)
                .await
                .map_err(|e| SchedulerError::Internal(format!("Failed to add job: {}", e)))?;

            cron_job.id = registered_id;
            self.jobs.insert(registered_id, cron_job.clone());

            if registered_id != id_original {
                self.jobs.remove(&id_original);
                if let Some(store) = &self.store {
                    let _ = store.remove_job(id_original).await;
                }
            }

            if let Some(store) = &self.store {
                store.save_job(&cron_job).await?;
            }

            Ok(registered_id)
        })
    }

    pub async fn add_job(
        self: &Arc<Self>,
        name: String,
        schedule: JobSchedule,
        payload: JobPayload,
    ) -> Result<Uuid> {
        let cron_job = CronJob {
            id: Uuid::new_v4(),
            name,
            schedule,
            payload,
            enabled: true,
            last_run_at: None,
            error_count: 0,
            max_retries: 3,
            priority: 5,
        };
        self.clone().register_job_runtime(cron_job).await
    }

    pub async fn update_job_status(self: &Arc<Self>, id: Uuid, success: bool) -> Result<()> {
        let mut retry_needed = false;
        let mut job_to_retry = None;

        if let Some(mut job) = self.jobs.get_mut(&id) {
            job.last_run_at = Some(Utc::now());
            if success {
                job.error_count = 0;
            } else {
                job.error_count += 1;
                if job.error_count > job.max_retries {
                    job.enabled = false;
                    let sched = self.scheduler.lock().await;
                    let _ = sched.remove(&id).await;
                } else if let JobSchedule::At { .. } = job.schedule {
                    retry_needed = true;
                    job_to_retry = Some(job.clone());
                }
            }

            if let Some(store) = &self.store {
                store.save_job(&job).await?;
            }
        }

        if retry_needed {
            if let Some(mut job) = job_to_retry {
                job.schedule = JobSchedule::At {
                    at: Utc::now() + chrono::Duration::seconds(30),
                };
                let _ = self.clone().register_job_runtime(job).await;
            }
        }
        Ok(())
    }

    pub fn list_jobs(&self) -> Vec<CronJob> {
        self.jobs.iter().map(|r| r.value().clone()).collect()
    }

    pub async fn remove_job(&self, id: Uuid) -> Result<bool> {
        let sched = self.scheduler.lock().await;
        let _ = sched.remove(&id).await;
        if let Some(store) = &self.store {
            let _ = store.remove_job(id).await;
        }
        Ok(self.jobs.remove(&id).is_some())
    }

    async fn execute_payload(
        handler_weak: &Weak<dyn JobHandler>,
        name: &str,
        payload: &JobPayload,
    ) -> Result<()> {
        let handler = handler_weak
            .upgrade()
            .ok_or(SchedulerError::HandlerDropped)?;
        match tokio::time::timeout(
            std::time::Duration::from_secs(300),
            handler.execute(name, payload),
        )
        .await
        {
            Ok(res) => res.map(|_| ()),
            Err(_) => Err(SchedulerError::Execution("Timeout".to_string())),
        }
    }

    /// Convenience method for triggering memory consolidation (Roadmap Phase 14)
    pub async fn trigger_consolidation(
        self: &Arc<Self>,
        agent_id: Option<String>,
        limit: usize,
    ) -> Result<Uuid> {
        let payload = JobPayload::ConsolidateMemory {
            limit,
            agent_id,
            global_context: None,
        };
        self.add_job(
            "manual_consolidation".to_string(),
            JobSchedule::At { at: Utc::now() },
            payload,
        )
        .await
    }

    pub async fn run(self: Arc<Self>) {
        let sched = self.scheduler.lock().await;
        if let Err(e) = sched.start().await {
            error!("Failed to start scheduler: {}", e);
        }
    }
}

#[async_trait]
impl HealthCheck for Scheduler {
    async fn check_health(&self) -> HealthStatus {
        let is_running = {
            let sched = self.scheduler.lock().await;
            sched.start().await.is_ok() // Basic check
        };

        if is_running {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy("Scheduler failed to start or is not responding".to_string())
        }
    }

    fn module_name(&self) -> &'static str {
        "benshu-scheduler"
    }
}
