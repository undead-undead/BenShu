use crate::eval::{BenchmarkFingerprint, WitnessBundle};
use crate::trace::RunTrace;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub const PROFILER_EXPORT_SCHEMA_VERSION: &str = "benshu.telemetry.profiler.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LatencyBreakdownEntry {
    pub label: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfilerLatencyArtifact {
    pub wall_time_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_breakdown: Vec<LatencyBreakdownEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_breakdown: Vec<LatencyBreakdownEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProfilerMemoryArtifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rss_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_rss_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_memory_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_ram_used_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceMetricKind {
    TokenEquivalent,
    CpuTime,
    EnergyEstimate,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfilerResourceArtifact {
    pub metric_kind: ResourceMetricKind,
    pub value: f64,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_energy_mah: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfilerArtifact {
    pub profiler_id: String,
    pub run_id: Uuid,
    pub trace_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_fingerprint: Option<BenchmarkFingerprint>,
    pub generated_at: DateTime<Utc>,
    pub export_schema_version: String,
    pub latency: ProfilerLatencyArtifact,
    pub memory: ProfilerMemoryArtifact,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ProfilerResourceArtifact>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfilerArtifactQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfilerExport {
    pub schema_version: String,
    pub exported_at: DateTime<Utc>,
    pub artifacts: Vec<ProfilerArtifact>,
}

impl ProfilerArtifact {
    pub fn from_run_trace(run_trace: &RunTrace, witness: Option<&WitnessBundle>) -> Self {
        let trace_id = witness
            .map(|bundle| bundle.trace_id)
            .unwrap_or(run_trace.run_id);
        let latency = build_latency_artifact(run_trace);
        let memory = build_memory_artifact(run_trace);
        let resource = build_resource_artifact(run_trace);
        let suite_id = witness.map(|bundle| bundle.task.suite_id.clone());
        let benchmark_fingerprint = witness.map(|bundle| bundle.benchmark_fingerprint.clone());
        let witness_id = witness.map(|bundle| bundle.witness_id);
        let generated_at = witness
            .map(|bundle| bundle.generated_at)
            .or(run_trace.finished_at)
            .unwrap_or(run_trace.started_at);

        Self {
            profiler_id: profiler_id_for_run(&run_trace.run_id),
            run_id: run_trace.run_id,
            trace_id,
            witness_id,
            suite_id,
            benchmark_fingerprint,
            generated_at,
            export_schema_version: PROFILER_EXPORT_SCHEMA_VERSION.to_string(),
            latency,
            memory,
            resource,
            metadata: build_profiler_metadata(run_trace),
        }
    }
}

impl ProfilerExport {
    pub fn from_artifacts(mut artifacts: Vec<ProfilerArtifact>) -> Self {
        artifacts.sort_by(|left, right| {
            left.suite_id
                .cmp(&right.suite_id)
                .then_with(|| left.run_id.cmp(&right.run_id))
                .then_with(|| left.profiler_id.cmp(&right.profiler_id))
        });

        Self {
            schema_version: PROFILER_EXPORT_SCHEMA_VERSION.to_string(),
            exported_at: Utc::now(),
            artifacts,
        }
    }
}

pub fn profiler_id_for_run(run_id: &Uuid) -> String {
    format!("profiler-{run_id}")
}

fn build_latency_artifact(run_trace: &RunTrace) -> ProfilerLatencyArtifact {
    let wall_time_ms = run_trace
        .finished_at
        .map(|finished_at| safe_duration_ms(run_trace.started_at, finished_at))
        .unwrap_or_default();

    let stage_breakdown = run_trace
        .stages
        .iter()
        .filter_map(|stage| {
            stage.finished_at.map(|finished_at| LatencyBreakdownEntry {
                label: stage.stage.label().to_string(),
                duration_ms: safe_duration_ms(stage.started_at, finished_at),
            })
        })
        .collect();

    let tool_breakdown = run_trace
        .tools
        .iter()
        .filter_map(|tool| {
            tool.duration_ms.map(|duration_ms| LatencyBreakdownEntry {
                label: tool.tool_name.clone(),
                duration_ms,
            })
        })
        .collect();

    ProfilerLatencyArtifact {
        wall_time_ms,
        stage_breakdown,
        tool_breakdown,
    }
}

fn build_memory_artifact(run_trace: &RunTrace) -> ProfilerMemoryArtifact {
    ProfilerMemoryArtifact {
        rss_bytes: parse_metadata_u64(&run_trace.metadata, "profiler.memory.rss_bytes"),
        peak_rss_bytes: parse_metadata_u64(&run_trace.metadata, "profiler.memory.peak_rss_bytes"),
        virtual_memory_bytes: parse_metadata_u64(
            &run_trace.metadata,
            "profiler.memory.virtual_memory_bytes",
        ),
        system_ram_used_bytes: parse_metadata_u64(
            &run_trace.metadata,
            "profiler.memory.system_ram_used_bytes",
        ),
    }
}

fn build_resource_artifact(run_trace: &RunTrace) -> Option<ProfilerResourceArtifact> {
    let custom_kind = run_trace.metadata.get("profiler.resource.kind").cloned();
    let custom_value = parse_metadata_f64(&run_trace.metadata, "profiler.resource.value");
    let custom_unit = run_trace.metadata.get("profiler.resource.unit").cloned();
    let estimated_energy_mah = parse_metadata_f64(
        &run_trace.metadata,
        "profiler.resource.estimated_energy_mah",
    );

    if let Some(value) = custom_value {
        let metric_kind = match custom_kind.as_deref() {
            Some("cpu_time") => ResourceMetricKind::CpuTime,
            Some("energy_estimate") => ResourceMetricKind::EnergyEstimate,
            Some("token_equivalent") => ResourceMetricKind::TokenEquivalent,
            Some(_) | None => ResourceMetricKind::Custom,
        };
        return Some(ProfilerResourceArtifact {
            metric_kind,
            value,
            unit: custom_unit.unwrap_or_else(|| "units".to_string()),
            estimated_energy_mah,
        });
    }

    let total_tokens = run_trace.prompt_tokens.unwrap_or_default()
        + run_trace.completion_tokens.unwrap_or_default();
    if total_tokens == 0 {
        return None;
    }

    Some(ProfilerResourceArtifact {
        metric_kind: ResourceMetricKind::TokenEquivalent,
        value: total_tokens as f64,
        unit: "tokens".to_string(),
        estimated_energy_mah,
    })
}

fn build_profiler_metadata(run_trace: &RunTrace) -> HashMap<String, String> {
    let mut metadata = HashMap::new();

    if let Some(provider) = &run_trace.provider {
        metadata.insert("provider".to_string(), provider.clone());
    }
    if let Some(model) = &run_trace.model {
        metadata.insert("model".to_string(), model.clone());
    }
    if let Some(thread_id) = &run_trace.thread_id {
        metadata.insert("thread_id".to_string(), thread_id.clone());
    }
    if let Some(task_id) = run_trace.task_id {
        metadata.insert("task_id".to_string(), task_id.to_string());
    }
    if let Some(route) = run_trace.metadata.get("route") {
        metadata.insert("route".to_string(), route.clone());
    }

    metadata
}

fn parse_metadata_u64(metadata: &HashMap<String, String>, key: &str) -> Option<u64> {
    metadata
        .get(key)
        .and_then(|value| value.parse::<u64>().ok())
}

fn parse_metadata_f64(metadata: &HashMap<String, String>, key: &str) -> Option<f64> {
    metadata
        .get(key)
        .and_then(|value| value.parse::<f64>().ok())
}

fn safe_duration_ms(started_at: DateTime<Utc>, finished_at: DateTime<Utc>) -> u64 {
    finished_at
        .signed_duration_since(started_at)
        .num_milliseconds()
        .max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::SimulationHarness;
    use crate::trace::{AgentTracer, RuntimeStage, RuntimeStageTrace, ToolTrace, TraceStatus};
    use chrono::Duration;
    use std::collections::HashMap;

    #[test]
    fn profiler_artifact_builds_from_run_trace_and_witness() {
        let tracer = AgentTracer::new(Uuid::new_v4(), "agent-main");
        let mut run_trace = tracer.start_run_trace();
        run_trace.task_id = Some(Uuid::new_v4());
        run_trace.thread_id = Some("thread-main".to_string());
        run_trace.provider = Some("openai".to_string());
        run_trace.model = Some("gpt-test".to_string());
        run_trace.prompt_tokens = Some(120);
        run_trace.completion_tokens = Some(80);
        run_trace.finished_at = Some(run_trace.started_at + Duration::milliseconds(220));
        run_trace.stages.push(RuntimeStageTrace {
            stage: RuntimeStage::Ingress,
            status: TraceStatus::Succeeded,
            started_at: run_trace.started_at,
            finished_at: Some(run_trace.started_at + Duration::milliseconds(20)),
            detail: None,
            metadata: HashMap::new(),
        });
        run_trace.tools.push(ToolTrace {
            call_id: "call-1".to_string(),
            tool_name: "fs".to_string(),
            status: TraceStatus::Succeeded,
            started_at: run_trace.started_at + Duration::milliseconds(50),
            finished_at: Some(run_trace.started_at + Duration::milliseconds(100)),
            duration_ms: Some(50),
            input: None,
            output: None,
            error: None,
            degraded: false,
        });
        run_trace.metadata.insert(
            "profiler.memory.peak_rss_bytes".to_string(),
            "8192".to_string(),
        );

        let witness = SimulationHarness::build_witness_bundle(&run_trace, "runtime_main_path");
        let artifact = ProfilerArtifact::from_run_trace(&run_trace, Some(&witness));

        assert_eq!(artifact.run_id, run_trace.run_id);
        assert_eq!(artifact.trace_id, witness.trace_id);
        assert_eq!(artifact.witness_id, Some(witness.witness_id));
        assert_eq!(artifact.suite_id.as_deref(), Some("runtime_main_path"));
        assert_eq!(artifact.latency.wall_time_ms, 220);
        assert_eq!(artifact.latency.stage_breakdown[0].duration_ms, 20);
        assert_eq!(artifact.latency.tool_breakdown[0].duration_ms, 50);
        assert_eq!(artifact.memory.peak_rss_bytes, Some(8192));
        assert_eq!(
            artifact.resource,
            Some(ProfilerResourceArtifact {
                metric_kind: ResourceMetricKind::TokenEquivalent,
                value: 200.0,
                unit: "tokens".to_string(),
                estimated_energy_mah: None,
            })
        );
        assert_eq!(
            artifact.export_schema_version,
            PROFILER_EXPORT_SCHEMA_VERSION.to_string()
        );
    }
}
