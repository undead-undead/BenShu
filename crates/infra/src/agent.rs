use serde::{Deserialize, Serialize};

/// Role of an agent in a multi-agent system
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentRole {
    Researcher,
    Trader,
    RiskAnalyst,
    Strategist,
    Custom(String),
}

impl std::str::FromStr for AgentRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "researcher" => Ok(Self::Researcher),
            "trader" => Ok(Self::Trader),
            "risk_analyst" => Ok(Self::RiskAnalyst),
            "strategist" => Ok(Self::Strategist),
            "benshu" => Ok(Self::Custom("benshu".to_string())),
            _ => Ok(Self::Custom(s.to_string())),
        }
    }
}

impl AgentRole {
    pub fn name(&self) -> &str {
        match self {
            Self::Researcher => "researcher",
            Self::Trader => "trader",
            Self::RiskAnalyst => "risk_analyst",
            Self::Strategist => "strategist",
            Self::Custom(name) => name,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Researcher => "Specialized in deep research, web search, and data analysis.",
            Self::Trader => "Specialized in executing trades and managing orders.",
            Self::RiskAnalyst => "Specialized in assessing risks and providing safety scores.",
            Self::Strategist => "Specialized in high-level planning and decision making.",
            Self::Custom(name) if name == "benshu" => {
                "Prime agent responsible for user interaction, coordination, governance, and delivery."
            }
            Self::Custom(_) => "Specialized agent for custom tasks.",
        }
    }

    pub fn capabilities(&self) -> Vec<&str> {
        match self {
            Self::Researcher => vec!["search", "analyze", "synthesize"],
            Self::Trader => vec!["trade", "order_management", "balance_check"],
            Self::RiskAnalyst => vec!["risk_assessment", "safety_audit", "metrics"],
            Self::Strategist => vec!["planning", "orchestration", "optimization"],
            Self::Custom(name) if name == "benshu" => {
                vec!["interaction", "coordination", "governance", "delivery"]
            }
            Self::Custom(_) => vec!["custom_task"],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Request,
    Response,
    Info,
    Approval,
    Denial,
    Handover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub from: AgentRole,
    pub to: Option<AgentRole>,
    pub content: String,
    pub msg_type: MessageType,
}

/// Phase 12-D: Safety levels for tool execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SafetyLevel {
    /// Safe to execute without restriction
    #[default]
    Green,
    /// Potentially sensitive, may need monitoring
    Yellow,
    /// High-risk tool, definitely needs approval or strict gating
    Red,
}

/// Token usage statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Metabolic Stats for Performance Arbitrage
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetabolicStats {
    pub cpu_usage: f32,
    pub vram_pressure: f32,
    pub mem_pressure: f32,
    pub token_usage: Option<TokenUsage>,
    pub is_throttled: bool,
}

/// Call data for tool execution (for history and metrics)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallData {
    pub name: String,
    pub args: String,
    pub result: Option<String>,
    pub duration_ms: u64,
    pub timestamp: u64,
    pub caller_id: Option<String>,
    pub safety_level: SafetyLevel,
    #[serde(default)]
    pub cpu_pressure: Option<f32>,
    #[serde(default)]
    pub vram_pressure: Option<f32>,
}

/// Events emitted by the Agent during execution
#[derive(Debug, Clone, Serialize)]
pub struct AgentEvent {
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub data: AgentEventData,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AgentEventData {
    Thinking {
        prompt: String,
    },
    StepStart {
        step: usize,
    },
    ToolCall {
        tool: String,
        input: String,
        safety: SafetyLevel,
    },
    ToolExecutionStart {
        tool: String,
        input: String,
        safety: SafetyLevel,
    },
    ToolExecutionEnd {
        tool: String,
        output_preview: String,
        duration_ms: u64,
        success: bool,
    },
    ApprovalPending {
        tool: String,
        input: String,
        safety: SafetyLevel,
    },
    Thought {
        content: String,
    },
    PartialResponse {
        content: String,
    },
    ToolResult {
        tool: String,
        output: String,
    },
    Response {
        content: String,
        usage: Option<TokenUsage>,
    },
    TokenUsage {
        usage: TokenUsage,
    },
    LatencyTTFT {
        duration_ms: u64,
    },
    Error {
        message: String,
    },
    Cancelled {
        reason: String,
    },
    Intervention {
        typ: String,
        reason: String,
        metadata: serde_json::Value,
    },
    ForgePreview {
        name: String,
        script: String,
        runtime: String,
        complexity: f32,
    },
    CommSent {
        target: String,
        size: usize,
        success: bool,
    },
    CommReceived {
        source: String,
        size: usize,
    },
    RuntimeStage {
        stage: String,
        status: String,
        run_id: Option<String>,
        task_id: Option<String>,
        thread_id: Option<String>,
        detail: Option<String>,
    },
    GovernanceDecision {
        scope: String,
        subject: Option<String>,
        authority: String,
        policy: Option<String>,
        approved: Option<bool>,
        risk_score: Option<f32>,
        detail: Option<String>,
    },
    GovernanceBudget {
        budget_kind: String,
        limit: Option<u32>,
        used: u32,
        remaining: Option<u32>,
        exceeded: bool,
        detail: Option<String>,
    },
    ProviderFailover {
        route: String,
        state: String,
        provider: String,
        fallback_provider: Option<String>,
        reason: String,
    },
    A2aEnvelopeProcessed {
        kind: String,
        message_id: Option<String>,
        request_id: Option<String>,
        runtime_profile: String,
        source: Option<String>,
        target: Option<String>,
        session_id: Option<String>,
        trace_id: Option<String>,
        task_id: Option<String>,
        parent_task_id: Option<String>,
        root_task_id: Option<String>,
        visible_owner: Option<String>,
        memory_owner: Option<String>,
        approval_owner: Option<String>,
        delegated_by: Option<String>,
        delegated_to: Option<String>,
        final_response_owner: Option<String>,
        return_mode: Option<String>,
        delegation_state: Option<String>,
    },
}
