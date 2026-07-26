use std::sync::Arc;

use benshu_brain::agent::evolution::auditor::Auditor;
use benshu_brain::agent::evolution::evolution_manager::EvolutionManager;
use benshu_brain::agent::memory::{InMemoryMemory, Memory};
use benshu_brain::agent::provider::Provider;
use benshu_brain::agent::streaming::MockStreamBuilder;
use benshu_brain::testing::SequenceMockProvider;
use benshu_brain::Message;
use benshu_experience_core::ExperienceStore;
use tempfile::tempdir;

fn successful_outcome() -> benshu_brain::agent::protocol::ChatOutcome {
    benshu_brain::agent::protocol::ChatOutcome {
        response: "Executed successfully".to_string(),
        thoughts: Vec::new(),
        tool_calls: Vec::new(),
        metabolic_stats: None,
        ownership: benshu_brain::agent::protocol::TaskOwnership::direct(
            benshu_infra::agent::AgentRole::Custom("benshu".to_string()),
            None,
        ),
        delegation: None,
        handover: None,
        runtime_task: None,
        run_trace: None,
    }
}

#[tokio::test]
async fn evolution_learning_persists_task_experience_record() {
    let exp_json = r#"{
        "problem_description": "Data processing is slow",
        "successful_path": ["step_A", "step_B"],
        "key_parameters": [],
        "lessons_learned": [],
        "anti_patterns": [],
        "timestamp": "2026-03-20T10:00:00Z"
    }"#;

    let provider: Arc<dyn Provider> =
        Arc::new(SequenceMockProvider::new(vec![MockStreamBuilder::new()
            .message(exp_json)
            .done()
            .build()]));
    let base_dir = tempdir().unwrap().path().to_path_buf();
    let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
    let manager = EvolutionManager::new(auditor, base_dir.clone());
    let experience_store =
        Arc::new(ExperienceStore::open(base_dir.join("experience.redb")).unwrap());
    manager.set_experience_store(experience_store.clone());

    let memory = Arc::new(InMemoryMemory::new());
    manager.set_memory(Arc::clone(&memory) as Arc<dyn Memory>);

    let count = manager
        .learn_from_experience(&[Message::user("optimize parser")], &successful_outcome())
        .await
        .expect("learning should complete");

    assert_eq!(count, 2);
    assert_eq!(experience_store.list().unwrap().len(), 1);
}

#[tokio::test]
async fn evolution_rewards_used_experience_without_skill_hardening() {
    let exp_json = r#"{
        "problem_description": "Data processing is slow",
        "successful_path": ["step_A"],
        "key_parameters": [],
        "lessons_learned": [],
        "anti_patterns": [],
        "timestamp": "2026-03-20T10:00:00Z"
    }"#;

    let provider: Arc<dyn Provider> =
        Arc::new(SequenceMockProvider::new(vec![MockStreamBuilder::new()
            .message(exp_json)
            .done()
            .build()]));
    let base_dir = tempdir().unwrap().path().to_path_buf();
    let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
    let manager = EvolutionManager::new(auditor, base_dir);

    let memory = Arc::new(InMemoryMemory::new());
    manager.set_memory(Arc::clone(&memory) as Arc<dyn Memory>);

    let exp_id = "exp_golden_123";
    let mut experience = serde_json::json!({
        "problem_description": "Data processing is slow",
        "successful_path": ["step_A", "step_B"],
        "task_query": "Optimized data processing",
        "utility_score": 5.1,
        "last_updated_at": 0
    });
    experience["id"] = serde_json::json!(exp_id);
    memory.store_experience(experience).await.unwrap();

    let mut msg = Message::user("Do it again");
    msg.used_experience_ids = vec![exp_id.to_string()];

    let count = manager
        .learn_from_experience(&[msg], &successful_outcome())
        .await
        .expect("learning should complete");

    assert_eq!(count, 1);
    let updated = memory
        .get_experience(exp_id)
        .await
        .expect("experience lookup should succeed")
        .expect("experience should still exist");
    assert!(
        updated["utility_score"].as_f64().unwrap_or_default() >= 5.1,
        "existing experience should still be rewarded"
    );
}
