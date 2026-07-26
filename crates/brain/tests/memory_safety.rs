use benshu_brain::agent::memory::{Fact, FactStatus, Memory, ShortTermMemory};
use benshu_brain::agent::message::{Message, Role};
use tempfile::tempdir;

#[tokio::test]
async fn test_hem_memory_tiering_and_persistence() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test_memory.redb");

    // 1. Initialize HEM with small L1 capacity (5 messages) to force tiering
    let memory = ShortTermMemory::new(5, 10, db_path.clone()).await;
    let user_id = "test_user";
    let agent_id = Some("test_agent");

    // 2. Push 10 messages (L1 should overflow, but L2 should keep all)
    for i in 0..10 {
        let msg = Message::new(Role::User, format!("Message {}", i));
        memory
            .store(user_id, agent_id, msg)
            .await
            .expect("Failed to store message");
    }

    // 3. Verify L1 only has 5 messages (Latest 5-9)
    let l1_msgs = memory.retrieve(user_id, agent_id, 10).await;
    assert_eq!(l1_msgs.len(), 5);
    assert_eq!(l1_msgs[0].text(), "Message 5");
    assert_eq!(l1_msgs[4].text(), "Message 9");

    // 4. Verify L2 Persistence (Simulate L1 cache miss/clear)
    drop(memory);

    let recovered_memory = ShortTermMemory::new(100, 10, db_path).await;

    // We can verify that L1 of the new instance is empty
    let empty_msgs = recovered_memory.retrieve(user_id, agent_id, 10).await;
    assert_eq!(empty_msgs.len(), 0);

    // But retrieve_full_history should fetch all 10 from Redb
    let full_history = recovered_memory
        .retrieve_full_history(user_id, agent_id)
        .await
        .expect("Failed to get full history");
    assert_eq!(full_history.len(), 10);
    assert_eq!(full_history[0].text(), "Message 0");
    assert_eq!(full_history[9].text(), "Message 9");
}

#[tokio::test]
async fn test_fact_lifecycle_and_updates() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("fact_test.redb");
    let memory = ShortTermMemory::new(50, 10, db_path).await;
    let user_id = "test_user";

    // 1. Store a Fact
    let mut fact1 = Fact::new("User lives in Beijing", "Personal");
    fact1.id = "fact_001".to_string();
    fact1.status = FactStatus::Verified;

    memory
        .store_fact(user_id, None, fact1)
        .await
        .expect("Store fact failed");

    // 2. Retrieve and verify
    let facts = memory
        .retrieve_facts(user_id, None)
        .await
        .expect("Retrieve facts failed");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].content, "User lives in Beijing");

    // 3. Update Fact (Simulate moving)
    let mut fact2 = facts[0].clone();
    fact2.content = "User moved to Shanghai".to_string();
    memory
        .store_fact(user_id, None, fact2)
        .await
        .expect("Update fact failed");

    let updated_facts = memory
        .retrieve_facts(user_id, None)
        .await
        .expect("Retrieve failed");
    assert_eq!(updated_facts.len(), 1);
    assert_eq!(updated_facts[0].content, "User moved to Shanghai");
}
