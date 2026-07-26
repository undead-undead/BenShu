use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

// Brain Core dependencies
use benshu_brain::agent::core::Agent;
use benshu_brain::agent::message::Message;
use benshu_brain::agent::protocol::{ChatOutcome, ToolCallData};
use benshu_brain::agent::provider::{ChatRequest, Provider, ProviderMetadata};
use benshu_brain::agent::streaming::{MockStreamBuilder, StreamingResponse};
use benshu_brain::error::Result;
use benshu_brain::security::{AuditLogRecord, LeakDetection, SanitizedOutput, SecurityHandler};
use benshu_brain::skills::tool::{SafetyLevel, Tool, ToolDefinition, ToolSet};

// --- Mocking Setup ---

struct MockSecurityHandler;
#[async_trait]
impl SecurityHandler for MockSecurityHandler {
    fn check_input(&self, text: &str) -> SanitizedOutput {
        SanitizedOutput {
            content: text.to_string(),
            warnings: vec![],
            was_modified: false,
        }
    }
    fn check_output(&self, text: &str) -> (String, Vec<LeakDetection>) {
        (text.to_string(), vec![])
    }
    fn log_action(
        &self,
        _s: Option<&str>,
        _t: &str,
        _a: &str,
        _succ: bool,
        _o: &str,
        _b: Option<benshu_brain::skills::BackupInfo>,
    ) {
    }
    async fn retrieve_audit_logs(&self, _l: usize) -> anyhow::Result<Vec<AuditLogRecord>> {
        Ok(vec![])
    }
}

/// Provider that returns pre-configured responses in sequence
struct SequenceMockProvider {
    responses: Mutex<Vec<StreamingResponse>>,
}

impl SequenceMockProvider {
    fn new(responses: Vec<StreamingResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl Provider for SequenceMockProvider {
    async fn stream_completion(
        &self,
        _request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        let mut resps = self.responses.lock().await;
        if resps.is_empty() {
            return Err(benshu_infra::error::Error::Internal(
                "MockProvider: No more responses configured".to_string(),
            ));
        }
        Ok(resps.remove(0))
    }

    fn name(&self) -> &'static str {
        "sequence-mock"
    }

    // Correctly implemented as an associated function per the trait definition
    fn metadata() -> ProviderMetadata
    where
        Self: Sized,
    {
        ProviderMetadata {
            id: "mock".to_string(),
            name: "Mock Provider".to_string(),
            description: "Sequence-based mock provider for testing".to_string(),
            icon: "".to_string(),
            fields: vec![],
            capabilities: vec![],
            preferred_models: vec![],
        }
    }
}

/// A simple tool for multiplying numbers
pub struct MultiplyTool;

#[async_trait]
impl Tool for MultiplyTool {
    fn name(&self) -> String {
        "multiply".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Multiplies two numbers".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                },
                "required": ["a", "b"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: SafetyLevel::Green,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let a = args["a"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("Missing parameter 'a'"))?;
        let b = args["b"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("Missing parameter 'b'"))?;

        Ok((a * b).to_string())
    }
}

// --- The Test ---

#[tokio::test]
async fn test_full_reasoning_execution_loop() {
    // 1. Prepare Mock responses (sequential)
    let resp1 = MockStreamBuilder::new()
        .thought("I need to multiply 7 and 6.")
        .tool_call("call_1", "multiply", serde_json::json!({"a": 7, "b": 6}))
        .done()
        .build();

    let resp2 = MockStreamBuilder::new()
        .message("The result of 7 * 6 is 42.")
        .done()
        .build();

    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(vec![resp1, resp2]));

    // 2. Setup ToolSet (Using interior mutability)
    let toolset = ToolSet::new();
    toolset.add(MultiplyTool);

    // 3. Build the Agent
    let agent = Agent::builder(provider)
        .name("test-agent")
        .with_tools(toolset)
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("Failed to build agent");

    // 4. Run the reasoning loop
    let outcome: ChatOutcome = agent
        .chat(vec![Message::user("What is 7 times 6?")], None)
        .await
        .expect("Chat execution failed");

    // 5. Assertions
    assert!(
        outcome.response.contains("42"),
        "Final response should contain 42, got: {}",
        outcome.response
    );

    assert!(
        outcome
            .thoughts
            .iter()
            .any(|t| t.contains("multiply 7 and 6")),
        "Expected thinking trace not found in: {:?}",
        outcome.thoughts
    );

    assert_eq!(outcome.tool_calls.len(), 1, "Expected exactly 1 tool call");
    let tool_call: &ToolCallData = &outcome.tool_calls[0];
    assert_eq!(tool_call.name, "multiply");

    // Check tool result (may contain First Use Injection notice)
    let result = tool_call
        .result
        .as_ref()
        .expect("Tool result should be present");
    assert!(
        result.starts_with("42"),
        "Tool output mismatch (expected to start with 42), got: {}",
        result
    );

    println!("✅ Full Reasoning Loop Verified: Thought -> Tool Call -> Result -> Final Answer");
}
