use benshu_brain::runtime::task_runner::TaskRunner;
use benshu_infra::traits::background::BackgroundTaskManager;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_task_runner_concurrency_limit() {
    // 1. Initialize TaskRunner with capacity 2
    let runner = TaskRunner::new(2);
    let counter = Arc::new(AtomicUsize::new(0));

    // 2. Spawn 3 tasks (Capacity is 2, so the 3rd should be dropped)
    for _ in 0..3 {
        let counter_inner = Arc::clone(&counter);
        runner.spawn(Box::pin(async move {
            counter_inner.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(100)).await;
        }));
    }

    // Give it a moment to start
    sleep(Duration::from_millis(20)).await;

    // 3. Verify active tasks is 2
    assert_eq!(runner.active_tasks(), 2);

    // 4. Wait for them to finish
    sleep(Duration::from_millis(200)).await;

    // 5. Check counter (Only 2 should have ever run)
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_task_runner_graceful_shutdown() {
    let runner = TaskRunner::new(5);
    let finished_count = Arc::new(AtomicUsize::new(0));

    // Spawn 5 long tasks
    for _ in 0..5 {
        let finished_inner = Arc::clone(&finished_count);
        runner.spawn(Box::pin(async move {
            sleep(Duration::from_millis(200)).await;
            finished_inner.fetch_add(1, Ordering::SeqCst);
        }));
    }

    sleep(Duration::from_millis(50)).await;
    assert_eq!(runner.active_tasks(), 5);

    // Trigger shutdown and wait
    runner.shutdown().await;

    // After shutdown returns, all 5 should be finished
    assert_eq!(finished_count.load(Ordering::SeqCst), 5);
    assert_eq!(runner.active_tasks(), 0);
}

#[tokio::test]
async fn test_task_runner_abort_all() {
    let runner = TaskRunner::new(10);
    let counter = Arc::new(AtomicUsize::new(0));

    for _ in 0..5 {
        let counter_inner = Arc::clone(&counter);
        runner.spawn(Box::pin(async move {
            sleep(Duration::from_secs(10)).await; // Should never finish
            counter_inner.fetch_add(1, Ordering::SeqCst);
        }));
    }

    sleep(Duration::from_millis(50)).await;
    assert_eq!(runner.active_tasks(), 5);

    runner.abort_all();

    // Abort is immediate, but handles might need a tick to reflect status
    sleep(Duration::from_millis(50)).await;
    assert_eq!(runner.active_tasks(), 0);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}
