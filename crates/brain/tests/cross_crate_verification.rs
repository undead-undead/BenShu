use benshu_brain::agent::memory::{InMemoryMemory, Memory};
use benshu_brain::security::SecurityHandler;
use benshu_brain::skills::runtime::SkillRuntime;
use benshu_brain::skills::{SkillExecutionConfig, SkillMetadata};
use benshu_runtimes::QuickJSRuntime;
use benshu_security::{SecurityConfig, SecurityManager};
use std::sync::Arc;

#[tokio::test]
async fn test_cross_crate_linkage() {
    // 1. Verify Security crate implements Brain's SecurityHandler trait
    let security_config = SecurityConfig::default();
    let security_manager = SecurityManager::new(security_config, None);

    // Call trait method
    let (redacted, _) = security_manager.check_output("sk-123456789012345678901234");
    assert!(!redacted.contains("sk-123456789012345678901234"));
    println!("✅ Security Trait Linkage: Verified");

    // 2. Verify Runtimes crate implements Brain's SkillRuntime trait
    let runtime = QuickJSRuntime::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let scripts_dir = temp_dir.path().join("scripts");
    std::fs::create_dir_all(&scripts_dir).unwrap();

    let script_name = "test.js";
    std::fs::write(
        scripts_dir.join(script_name),
        "console.log('linkage check'); 1+1",
    )
    .unwrap();

    let metadata = SkillMetadata {
        name: "linkage_test".to_string(),
        description: "check".to_string(),
        homepage: None,
        parameters: None,
        interface: None,
        script: Some(script_name.to_string()),
        runtime: Some("quickjs".to_string()),
        metadata: serde_json::Value::Null,
        kind: "tool".to_string(),
        usage_guidelines: None,
        dependencies: vec![],
        use_browser: false,
        models: vec![],
        source_fallback: None,
        safety_audit: None,
        permissions: Default::default(),
        resources: Default::default(),
        wasm: None,
    };

    let output = runtime
        .execute(
            &metadata,
            "{}",
            temp_dir.path(),
            &SkillExecutionConfig::default(),
            None,
        )
        .await
        .unwrap();
    assert!(output.status.success());
    println!("✅ Runtime Trait Linkage: Verified");

    // 3. Verify Memory Trait (internal to brain but core abstraction)
    let memory: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    use benshu_brain::agent::message::Message;

    memory
        .store("user1", Some("agent1"), Message::user("test_val"))
        .await
        .unwrap();
    let msgs = memory.retrieve("user1", Some("agent1"), 10).await;
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].text().contains("test_val"));
    println!("✅ Memory Trait Linkage: Verified");
}
