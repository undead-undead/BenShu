use benshu_brain::agent::provider::Provider;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use benshu_brain::agent::core::Agent;
use benshu_brain::agent::meta::LlmComplexityEstimator;
use benshu_brain::agent::multi_agent::MultiAgent;
use benshu_brain::agent::protocol::AgentRole;
use benshu_brain::agent::streaming::MockStreamBuilder;
use benshu_brain::testing::{MockSecurityHandler, SequenceMockProvider};
use benshu_brain::{Message, Role};

/// 🧠 TEST CASE 1: The Cerebral Cortex - Max-Steps & Session Validation
#[tokio::test]
async fn test_cerebral_cortex_gating() {
    let complexity_json =
        r#"{"estimated_steps": 5, "risk_score": 0.1, "intent": "test", "rationale": "testing"}"#;

    let responses = vec![
        MockStreamBuilder::new()
            .message(complexity_json)
            .done()
            .build(), // Complexity
        MockStreamBuilder::new()
            .thought("Thinking 1")
            .message("Part 1")
            .done()
            .build(), // Step 1
        MockStreamBuilder::new()
            .thought("Thinking 2")
            .message("Part 2")
            .done()
            .build(), // Step 2
    ];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));

    let agent = Agent::builder(provider.clone())
        .name("stricto")
        .with_default_max_steps(2)
        .with_enable_meta_cognition(true)
        .with_complexity_estimator(Arc::new(LlmComplexityEstimator::new(
            provider,
            "test".to_string(),
        )))
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("Failed to build agent");

    let messages = vec![Message::new(Role::User, "Hello")];
    let res = agent.chat(messages, None).await;

    match res {
        Ok(outcome) => {
            assert!(
                outcome.thoughts.len() <= 2,
                "Took too many steps: {}",
                outcome.thoughts.len()
            );
            println!(
                "✅ Cerebral Cortex: Max-Steps Gating Verified ({})",
                outcome.thoughts.len()
            );
        }
        Err(e) => panic!("Chat failed: {}", e),
    }
}

/// 🧠 TEST CASE 2: No automatic specialist handover in chat mainline
#[tokio::test]
async fn test_chat_mainline_stays_on_current_agent_without_swarm_router() {
    let complexity_json = r#"{"estimated_steps": 2, "risk_score": 0.1, "intent": "coding", "rationale": "coding task"}"#;

    let responses = vec![
        MockStreamBuilder::new()
            .message(complexity_json)
            .done()
            .build(), // 1. Complexity
        MockStreamBuilder::new()
            .message("I can handle this request directly.")
            .done()
            .build(), // 2. Final answer
    ];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));

    let alice = Agent::builder(provider.clone())
        .name("alice")
        .with_role(AgentRole::Researcher)
        .with_enable_meta_cognition(true)
        .with_complexity_estimator(Arc::new(LlmComplexityEstimator::new(
            provider,
            "test".to_string(),
        )))
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("Failed to build Alice");

    let bob_role = AgentRole::Custom("coder".to_string());
    alice.set_all_roles(vec![bob_role.clone()]);

    let messages = vec![Message::new(Role::User, "Write code")];
    let outcome = alice
        .chat_with_cancel(messages, None, CancellationToken::new())
        .await
        .expect("Chat failed");

    assert!(
        outcome.handover.is_none(),
        "Chat mainline should no longer force specialist handover from a removed SwarmRouter path"
    );
    println!("✅ Chat mainline stays on the current agent without SwarmRouter");
}

#[tokio::test]
async fn test_hot_interjection_preemption() {
    let complexity_json =
        r#"{"estimated_steps": 2, "risk_score": 0.1, "intent": "test", "rationale": "test"}"#;
    let responses = vec![
        MockStreamBuilder::new()
            .message(complexity_json)
            .done()
            .build(),
        MockStreamBuilder::new()
            .thought("Deep thought")
            .done()
            .build(),
    ];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
    let agent = Agent::builder(provider.clone())
        .with_enable_meta_cognition(true)
        .with_complexity_estimator(Arc::new(LlmComplexityEstimator::new(
            provider,
            "test".to_string(),
        )))
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .unwrap();
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let agent_clone = agent.clone();

    let h = tokio::spawn(async move {
        agent_clone
            .chat_with_cancel(vec![Message::new(Role::User, "Wait")], None, cancel_clone)
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    cancel.cancel();
    let _ = h.await;
    println!("✅ Hot-Interjection: Preemption flow triggered.");
}

#[tokio::test]
async fn test_metabolic_governance() {
    let complexity_json =
        r#"{"estimated_steps": 2, "risk_score": 0.1, "intent": "test", "rationale": "test"}"#;
    let responses = vec![
        MockStreamBuilder::new()
            .message(complexity_json)
            .done()
            .build(),
        MockStreamBuilder::new().message("OK").done().build(),
    ];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
    let agent = Agent::builder(provider.clone())
        .with_enable_meta_cognition(true)
        .with_complexity_estimator(Arc::new(LlmComplexityEstimator::new(
            provider,
            "test".to_string(),
        )))
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .unwrap();
    let res = agent.chat(vec![Message::new(Role::User, "Hi")], None).await;
    assert!(res.is_ok());
    println!("✅ Metabolic Governance: Flow OK.");
}
