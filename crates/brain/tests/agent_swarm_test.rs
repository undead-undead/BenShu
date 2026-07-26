use benshu_brain::agent::core::Agent;
use benshu_brain::agent::multi_agent::MultiAgent;
use benshu_brain::agent::protocol::Address;
use benshu_brain::agent::AgentLiaison;
use benshu_brain::testing::{CommTestEnv, MockSecurityHandler, SequenceMockProvider};
use std::sync::Arc;

#[tokio::test]
async fn test_agent_to_agent_direct_communication() {
    // 1. Setup global comm environment
    let env = CommTestEnv::new();

    // 2. Create Alice (Sender)
    let provider_alice = SequenceMockProvider::new(vec![]);
    let alice_client = env.create_client("alice");
    let alice = Agent::builder(provider_alice)
        .name("alice")
        .with_comm_client(alice_client)
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("Failed to build Alice");

    // 3. Create Bob (Receiver)
    let provider_bob = SequenceMockProvider::new(vec![]);
    let bob_client = env.create_client("bob");
    let bob = Agent::builder(provider_bob)
        .name("bob")
        .with_comm_client(bob_client)
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("Failed to build Bob");

    // Give a bit of time for registration to settle in the hub
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 4. Alice sends a message to Bob
    let payload = b"Hello from Alice!".to_vec();
    let target = Address::Agent("bob".to_string());

    let alice_comm = alice
        .comm_client()
        .expect("Alice should have a comm client");
    alice_comm
        .send_msg(target.clone(), payload.clone())
        .await
        .expect("Alice failed to send message");

    // 5. Bob receives the message
    let bob_comm = bob.comm_client().expect("Bob should have a comm client");

    // We poll Bob's client directly to verify the transport works inside the agent context
    let received = tokio::time::timeout(std::time::Duration::from_secs(1), bob_comm.receive_next())
        .await
        .expect("Timeout waiting for Bob to receive message");

    let envelope = received
        .expect("Transport error")
        .expect("Bob received no envelope");

    // 6. Assertions
    assert_eq!(envelope.meta.source.to_string(), "agent://alice");
    assert_eq!(envelope.payload, payload);

    println!("✅ Agent-to-Agent Communication Loop Verified: Alice -> MemoryHub -> Bob");
}

#[tokio::test]
async fn test_agent_multi_tenancy_throttling() {
    let env = CommTestEnv::new();
    let alice_client = env.create_client("alice");
    let bob_client = env.create_client("bob");

    // 1. Setup throttle for Tenant X on Alice's side (outbound throttle)
    alice_client.set_tenant_limit("tenant_x", 0).await; // Block tenant X

    // 2. Alice tries to send to Bob as Tenant X
    let res = alice_client
        .send_with_tenant(
            benshu_comm::protocol::Address::Agent("bob".to_string()),
            b"Fail".to_vec(),
            Some("tenant_x".to_string()),
        )
        .await;

    // 3. Assert failure
    assert!(res.is_err(), "Should be throttled for tenant_x");
    let err_msg = format!("{:?}", res.err());
    assert!(
        err_msg.contains("tenant:tenant_x"),
        "Error should mention tenant_x"
    );

    // 4. Try sending as Tenant Y (should pass)
    let res = alice_client
        .send_with_tenant(
            benshu_comm::protocol::Address::Agent("bob".to_string()),
            b"Pass".to_vec(),
            Some("tenant_y".to_string()),
        )
        .await;
    assert!(res.is_ok(), "Should pass for tenant_y");

    println!("✅ Multi-tenancy Throttling Verified");
}

#[tokio::test]
async fn test_metadata_security_verification() {
    use benshu_comm::protocol::Address;
    use benshu_comm::protocol::Metadata;

    let source = Address::Agent("sender".to_string());
    let mut meta = Metadata::new(source.clone());
    let key = b"super-secret-key-32-chars-long!!!";

    // 1. Sign
    meta.sign(key, source.clone()).expect("Signing failed");
    assert!(meta.signature.is_some());

    // 2. Verify Success
    assert!(meta.verify(key), "Verification failed on valid signature");

    // 3. Verify Failure (Wrong Key)
    let wrong_key = b"wrong-key-32-chars-long-!!!!!!!!";
    assert!(
        !meta.verify(wrong_key),
        "Verification should fail with wrong key"
    );

    // 4. Verify Failure (Tampered Payload - represented here by changing timestamp)
    meta.timestamp = std::time::Duration::from_secs(meta.timestamp.as_secs() + 1);
    assert!(
        !meta.verify(key),
        "Verification should fail if metadata is tampered"
    );

    println!("✅ Metadata Security (HMAC-SHA256) Verified");
}
