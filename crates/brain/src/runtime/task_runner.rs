use async_trait::async_trait;
use benshu_infra::traits::background::{BackgroundTaskManager, BoxedTask};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Phase 5.4: TaskRunner for decoupling background work from the Agent
/// Standard 6: OOM prevention via Semaphore-based concurrency limiting
pub struct TaskRunner {
    /// Maximum concurrent background tasks allowed
    capacity: usize,
    /// Semaphore for governing concurrency
    semaphore: Arc<Semaphore>,
    /// Active task handles for monitoring (L1 awareness)
    handles: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl TaskRunner {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            semaphore: Arc::new(Semaphore::new(capacity)),
            handles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Clean up finished handles (leak prevention)
    /// This is safe because it only affects the map, not the running tasks
    fn cleanup_finished(handles: &Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>) {
        let mut guard = handles.write();
        guard.retain(|_, handle| !handle.is_finished());
    }
}

#[async_trait]
impl BackgroundTaskManager for TaskRunner {
    fn spawn(&self, task: BoxedTask) {
        Self::cleanup_finished(&self.handles);

        // Standard 6: Check capacity and reserve permit before spawning
        let permit = match self.semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!(
                    "TaskRunner at full capacity ({}); dropping non-critical background task",
                    self.capacity
                );
                return;
            }
        };

        let handles_clone = self.handles.clone();
        let task_id = uuid::Uuid::new_v4().to_string();
        let task_id_for_spawn = task_id.clone();

        let handle = tokio::spawn(async move {
            // Keep permit for the duration of the task
            let _permit = permit;
            task.await;

            // Clean up from map once done
            handles_clone.write().remove(&task_id_for_spawn);
        });

        self.handles.write().insert(task_id, handle);
    }

    fn active_tasks(&self) -> usize {
        Self::cleanup_finished(&self.handles);
        self.handles.read().len()
    }

    fn shutdown(&self) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>> {
        let handles_clone = self.handles.clone();
        Box::pin(async move {
            tracing::info!("TaskRunner initiated shutdown");
            let worker_handles: Vec<_> = {
                let mut guard = handles_clone.write();
                guard.drain().map(|(_, h)| h).collect()
            };
            for handle in worker_handles {
                let _ = handle.await;
            }
            tracing::info!("TaskRunner shutdown complete.");
        })
    }

    fn capacity(&self) -> usize {
        self.capacity
    }

    fn abort_all(&self) {
        let mut handles = self.handles.write();
        for (_, handle) in handles.drain() {
            handle.abort();
        }
    }
}
