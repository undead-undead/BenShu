use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStage {
    Ingress,
    Governance,
    ContextBuild,
    Reasoning,
    ToolPlanningFiltering,
    Execution,
    PersistenceMemory,
    TraceAudit,
    Egress,
}

impl RuntimeStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ingress => "Ingress",
            Self::Governance => "Governance",
            Self::ContextBuild => "Context Build",
            Self::Reasoning => "Reasoning",
            Self::ToolPlanningFiltering => "Tool Planning & Filtering",
            Self::Execution => "Execution",
            Self::PersistenceMemory => "Persistence & Memory",
            Self::TraceAudit => "Trace & Audit",
            Self::Egress => "Egress",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TraceStatus {
    Started,
    Succeeded,
    Failed,
    Cancelled,
    Degraded,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolTrace {
    pub call_id: String,
    pub tool_name: String,
    pub status: TraceStatus,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeStageTrace {
    pub stage: RuntimeStage,
    pub status: TraceStatus,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WitnessSummary {
    pub witness_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorecard: Option<serde_json::Value>,
    #[serde(default)]
    pub replayable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunTrace {
    pub run_id: Uuid,
    pub session_id: Uuid,
    pub agent_id: String,
    pub status: TraceStatus,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<RuntimeStageTrace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolTrace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degradation_notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness: Option<WitnessSummary>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunReplayStep {
    pub ordinal: usize,
    pub label: String,
    pub status: TraceStatus,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunReplay {
    pub trace_id: Uuid,
    pub run_id: Uuid,
    pub session_id: Uuid,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub replayable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<RunReplayStep>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl RunTrace {
    pub fn to_replay(&self) -> RunReplay {
        let mut steps = Vec::new();
        for (ordinal, stage) in self.stages.iter().enumerate() {
            steps.push(RunReplayStep {
                ordinal,
                label: stage.stage.label().to_string(),
                status: stage.status.clone(),
                started_at: stage.started_at,
                finished_at: stage.finished_at,
                detail: stage.detail.clone(),
            });
        }

        let tool_offset = steps.len();
        for (idx, tool) in self.tools.iter().enumerate() {
            steps.push(RunReplayStep {
                ordinal: tool_offset + idx,
                label: format!("Tool: {}", tool.tool_name),
                status: tool.status.clone(),
                started_at: tool.started_at,
                finished_at: tool.finished_at,
                detail: tool
                    .duration_ms
                    .map(|duration| format!("duration_ms={duration}"))
                    .or_else(|| tool.error.clone()),
            });
        }

        RunReplay {
            trace_id: self.run_id,
            run_id: self.run_id,
            session_id: self.session_id,
            agent_id: self.agent_id.clone(),
            task_id: self.task_id,
            thread_id: self.thread_id.clone(),
            replayable: !steps.is_empty(),
            steps,
            metadata: self.metadata.clone(),
        }
    }
}

/// Agent-centric execution tracer (Phase 22.4)
#[derive(Debug, Clone)]
pub struct AgentTracer {
    pub session_id: Uuid,
    pub agent_id: String,
}

impl AgentTracer {
    pub fn new(session_id: Uuid, agent_id: &str) -> Self {
        Self {
            session_id,
            agent_id: agent_id.to_string(),
        }
    }

    pub fn start_run_trace(&self) -> RunTrace {
        RunTrace {
            run_id: Uuid::new_v4(),
            session_id: self.session_id,
            agent_id: self.agent_id.clone(),
            status: TraceStatus::Started,
            started_at: Utc::now(),
            finished_at: None,
            task_id: None,
            thread_id: None,
            provider: None,
            model: None,
            prompt_tokens: None,
            completion_tokens: None,
            stages: Vec::new(),
            tools: Vec::new(),
            artifacts: Vec::new(),
            degradation_notes: Vec::new(),
            witness: None,
            metadata: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn run_trace_round_trip_preserves_tools_and_witness() {
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let mut run = tracer.start_run_trace();
        run.status = TraceStatus::Degraded;
        run.provider = Some("anthropic".to_string());
        run.model = Some("claude".to_string());
        run.thread_id = Some("thread-1".to_string());
        run.prompt_tokens = Some(123);
        run.completion_tokens = Some(456);
        run.stages.push(RuntimeStageTrace {
            stage: RuntimeStage::Ingress,
            status: TraceStatus::Succeeded,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            detail: Some("input sanitized".to_string()),
            metadata: HashMap::new(),
        });
        run.tools.push(ToolTrace {
            call_id: "call-1".to_string(),
            tool_name: "pdf_parse".to_string(),
            status: TraceStatus::Succeeded,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            duration_ms: Some(88),
            input: Some(json!({"path": "demo.pdf"})),
            output: Some(json!({"ok": true})),
            error: None,
            degraded: false,
        });
        run.artifacts.push(ArtifactRef {
            artifact_id: "artifact-1".to_string(),
            kind: "witness_bundle".to_string(),
            uri: "artifacts://runs/run-1/witness.json".to_string(),
            media_type: Some("application/json".to_string()),
        });
        run.witness = Some(WitnessSummary {
            witness_id: Uuid::new_v4(),
            run_id: Some(run.run_id),
            verdict: "pass".to_string(),
            scorecard: Some(json!({"accuracy": 0.9})),
            replayable: true,
            benchmark_fingerprint: Some("bench:v1:model:claude".to_string()),
            notes: vec!["native fallback used".to_string()],
        });
        run.metadata
            .insert("route".to_string(), "native_structured".to_string());

        let encoded = serde_json::to_value(&run).expect("serialize run trace");
        let decoded: RunTrace = serde_json::from_value(encoded).expect("deserialize run trace");

        assert_eq!(decoded.agent_id, "agent-main");
        assert_eq!(decoded.stages.len(), 1);
        assert_eq!(decoded.tools.len(), 1);
        assert_eq!(decoded.artifacts.len(), 1);
        assert!(decoded.witness.is_some());
        assert_eq!(
            decoded.metadata.get("route").map(String::as_str),
            Some("native_structured")
        );
    }

    #[test]
    fn run_trace_can_be_projected_into_replay_steps() {
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let mut run = tracer.start_run_trace();
        run.stages.push(RuntimeStageTrace {
            stage: RuntimeStage::Ingress,
            status: TraceStatus::Succeeded,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            detail: Some("accepted request".to_string()),
            metadata: HashMap::new(),
        });
        run.tools.push(ToolTrace {
            call_id: "call-1".to_string(),
            tool_name: "memory_manage".to_string(),
            status: TraceStatus::Succeeded,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            duration_ms: Some(12),
            input: None,
            output: None,
            error: None,
            degraded: false,
        });

        let replay = run.to_replay();
        assert!(replay.replayable);
        assert_eq!(replay.trace_id, run.run_id);
        assert_eq!(replay.steps.len(), 2);
        assert_eq!(replay.steps[0].label, "Ingress");
        assert_eq!(replay.steps[1].label, "Tool: memory_manage");
    }
}
