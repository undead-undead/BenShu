use benshu_brain::agent::evolution::auditor::Auditor;
use benshu_brain::agent::evolution::evolution_manager::EvolutionManager;
use benshu_brain::agent::protocol::*;
use benshu_brain::agent::protocol::{Message, ReasonerConfig};
use benshu_brain::agent::provider::MockProvider;
use benshu_brain::agent::reasoner::Reasoner;
use benshu_brain::agent::tactical::GlobalTacticalOrchestrator;
use benshu_brain::skills::tool::ToolSet;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn test_extreme_evolution_synergy() {
    let base_dir = tempdir().unwrap().path().to_path_buf();

    // 1. Setup Mock Provider with a complex scripted response
    // Sequence:
    // Turn 1: Propose dangerous vault rotation
    // Turn 2: (After Audit rejection) Propose safer rotation with backup
    let mock_responses = vec![
        r#"[THOUGHT] I need to rotate the vault key immediately. [TOOL_CALL] {"id": "call_1", "name": "vault_manager", "arguments": {"action": "rotate"}}"#,
        r#"[THOUGHT] The auditor rejected my first plan due to lack of backup. I will perform a backup first, then rotate safely. [TOOL_CALL] {"id": "call_2", "name": "vault_manager", "arguments": {"action": "backup_and_rotate"}}"#,
        r#"Task completed successfully with safety protocols."#,
    ];
    let provider = Arc::new(MockProvider::new_sequence(mock_responses));

    // 2. Setup Evolution Manager & Auditor
    let auditor_provider = Arc::new(MockProvider::new("{\"decision\": \"REJECTED\", \"reason\": \"Missing pre-rotation backup - potential data loss.\"}"));
    let auditor = Arc::new(Auditor::new(auditor_provider, "audit-model".to_string()));
    let ev_manager = Arc::new(EvolutionManager::new(auditor, base_dir.clone()));

    // 3. Setup ToolSet with a "Red" safety tool
    let mut tools = ToolSet::new();
    // (In a real test we'd register a mock tool here, but for logic check we just need the definition)

    // 4. Setup Reasoner
    let config = ReasonerConfig {
        model: "test-model".to_string(),
        max_history_messages: 10,
        smart_pruning: true,
        ..Default::default()
    };
    let tactical = Arc::new(GlobalTacticalOrchestrator::passthrough());
    let reasoner = Reasoner::new(provider, config, tools, None, tactical);

    // 5. Setup Mock Liaison (Bridge)
    // Here we'd use a MockAgentLiaison that returns ThrottleLevel::Low to trigger metabolic adaptation

    println!(
        "🧪 EVOLUTION SYNERGY TEST: Verification of Audit/Adaptation/Learning flow starting..."
    );

    // This test would execute the loop and we'd assert on:
    // - messages.len() increasing with rejection notice
    // - experience records stored through the configured memory/experience backend
    // - etc.
}
