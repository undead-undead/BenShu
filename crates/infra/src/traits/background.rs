use std::future::Future;
use std::pin::Pin;

/// Standard 6: OOM/Leak prevention - Background Task Governance
pub trait BackgroundTaskManager: Send + Sync {
    /// Spawn a background task with governance
    fn spawn(&self, task: Pin<Box<dyn Future<Output = ()> + Send + 'static>>);

    /// Count currently active tasks
    fn active_tasks(&self) -> usize;

    /// Wait for all tasks to complete (for graceful shutdown)
    fn shutdown(&self) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

    /// Get the maximum allowed concurrent tasks
    fn capacity(&self) -> usize;

    /// Abort all active tasks immediately
    fn abort_all(&self);
}

/// Helper for building tasks that can be spawned easily
pub type BoxedTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
