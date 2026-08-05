use std::path::Path;

use benshu_brain::runtime::continuous_task::ContinuousTaskStatus;
use benshu_compression::preview_text;
use serde_json::{json, Value};

use super::{NovelWorkflowConfig, ProjectCompletionGateDecision};
use crate::tool::writing::novel_pipeline;

pub(super) fn format_interrupted_novel_workflow_result(
    config: &NovelWorkflowConfig,
    project_path: &str,
    completed_steps: usize,
    total_steps: usize,
    status: &ContinuousTaskStatus,
    final_summary: &str,
) -> String {
    let (status_label, blocker_label, reason) = match status {
        ContinuousTaskStatus::Paused { reason } => {
            ("paused", "provider_or_stream_stall", reason.as_str())
        }
        ContinuousTaskStatus::Blocked { reason } => {
            ("blocked", "workflow_blocked", reason.as_str())
        }
        ContinuousTaskStatus::Failed { reason } => ("failed", "workflow_failed", reason.as_str()),
        ContinuousTaskStatus::Completed => ("completed", "none", ""),
    };
    format!(
        "status: {status_label}\nworker: {}\nexecuted_tool: novel_studio\nworkflow_driver: {}\nproject_path: {project_path}\nexport_path: \noutput_path: \nformat: txt\nmedia_type: text/plain\nruntime_effects: artifact.checkpointed\ncompletion_scope: partial\nproject_complete: false\nturn_complete: false\nunit_count: 0\ntotal_units: 0\nchapters_completed: {completed_steps}\nchapters_planned: {total_steps}\nblocker_kind: {blocker_label}\nblockers: {}\nstate: {{}}\nresult: {}",
        config.worker_label,
        novel_pipeline::novel_workflow_descriptor().id,
        preview_text(reason, 500),
        preview_text(final_summary, 500)
    )
}

pub(super) fn format_novel_workflow_result(
    config: &NovelWorkflowConfig,
    project_path: &str,
    status_packet: &Value,
    project_complete: bool,
    requested_turn_complete: bool,
    completed_steps: usize,
    total_steps: usize,
    completion_gate: Option<&ProjectCompletionGateDecision>,
    final_summary: &str,
) -> anyhow::Result<String> {
    let narrative_completion_blocked =
        completion_gate.is_some_and(|gate| gate.target_reached && !gate.complete);
    let revision_blocked = workflow_summary_reports_unapproved_revision(final_summary);
    let state_reports_unapproved = status_packet_reports_unapproved_chapters(status_packet);
    let requested_turn_complete = requested_turn_complete && !revision_blocked;
    let complete = !revision_blocked
        && !state_reports_unapproved
        && (project_complete || (requested_turn_complete && !narrative_completion_blocked));
    let status = if complete { "completed" } else { "blocked" };
    let status_export_path = status_packet
        .pointer("/export/artifact_path")
        .or_else(|| status_packet.pointer("/export/output_path"))
        .or_else(|| status_packet.get("artifact_path"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let readable_txt_path = Path::new(project_path).join("exports").join("current.txt");
    let export_path = if !status_export_path.trim().is_empty() {
        status_export_path.to_string()
    } else if readable_txt_path.exists() {
        readable_txt_path.to_string_lossy().to_string()
    } else {
        String::new()
    };
    let state = status_packet
        .get("state")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let blockers = if status == "completed" {
        String::new()
    } else if revision_blocked {
        "\nblockers: latest chapter artifact requires revision before it can count as approved completion".to_string()
    } else if state_reports_unapproved {
        "\nblockers: novel project state still contains unapproved chapter artifacts; the outer task cannot report completed until the internal chapter state is approved".to_string()
    } else if let Some(gate) = completion_gate.filter(|gate| gate.target_reached && !gate.complete)
    {
        format!(
            "\nblockers: target units reached but narrative completion gate still needs closure: {}",
            gate.reason
        )
    } else {
        "\nblockers: governed writing workflow did not complete the requested chapter checkpoint batch".to_string()
    };

    let runtime_effects = if project_complete {
        "artifact.written, artifact.exported, artifact.txt, artifact.verified"
    } else if !export_path.is_empty() {
        "artifact.written, artifact.txt"
    } else {
        "artifact.checkpointed"
    };
    let total_units = state
        .get("approved_units")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let turn_units = receipt_number(final_summary, "unit_count").unwrap_or(total_units);
    let completion_gate_summary = completion_gate
        .map(|gate| {
            format!(
                "\ncompletion_gate: target_reached={}, narrative_closed={}, needs_finale={}, reason={}",
                gate.target_reached, gate.narrative_closed, gate.needs_finale, gate.reason
            )
        })
        .unwrap_or_default();

    Ok(format!(
        "status: {status}\nworker: {}\nexecuted_tool: novel_studio\nworkflow_driver: {}\nproject_path: {project_path}\nexport_path: {export_path}\noutput_path: {export_path}\nformat: txt\nmedia_type: text/plain\nruntime_effects: {runtime_effects}\ncompletion_scope: {}\nproject_complete: {}\nturn_complete: {}\nunit_count: {turn_units}\ntotal_units: {total_units}\nchapters_completed: {}\nchapters_planned: {}{}{}\nstate: {}\nresult: {}",
        config.worker_label,
        novel_pipeline::novel_workflow_descriptor().id,
        if project_complete {
            "project"
        } else if narrative_completion_blocked {
            "partial"
        } else if requested_turn_complete {
            "requested_turn"
        } else {
            "partial"
        },
        project_complete,
        requested_turn_complete,
        completed_steps,
        total_steps,
        blockers,
        completion_gate_summary,
        state,
        preview_text(final_summary, 500)
    ))
}

pub(super) fn format_completed_project_result(
    worker_label: &str,
    project_path: &str,
    status_packet: &Value,
    result: &str,
) -> String {
    let export_path = status_packet
        .pointer("/export/artifact_path")
        .or_else(|| status_packet.pointer("/export/output_path"))
        .or_else(|| status_packet.get("artifact_path"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let state = status_packet
        .get("state")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let reported_units = state
        .get("approved_units")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    format!(
        "status: completed\nworker: {worker_label}\nexecuted_tool: novel_studio\nworkflow_driver: {}\nproject_path: {project_path}\nexport_path: {export_path}\noutput_path: {export_path}\nformat: txt\nmedia_type: text/plain\nruntime_effects: artifact.written, artifact.exported, artifact.txt, artifact.verified\ncompletion_scope: project\nproject_complete: true\nturn_complete: true\nunit_count: {reported_units}\ntotal_units: {reported_units}\nchapters_completed: 0\nchapters_planned: 0\nstate: {state}\nresult: {result}",
        novel_pipeline::novel_workflow_descriptor().id
    )
}

fn workflow_summary_reports_unapproved_revision(summary: &str) -> bool {
    let lowered = summary.to_ascii_lowercase();
    lowered.contains("artifact.needs_revision")
        || lowered.contains("runtime_effect: artifact.needs_revision")
        || lowered.contains("\"runtime_effect\":\"artifact.needs_revision\"")
        || lowered.contains("status: blocked")
        || lowered.contains("\"status\":\"blocked\"")
}

pub(super) fn chapter_step_blocker_reason(output: &str) -> Option<String> {
    if !workflow_summary_reports_unapproved_revision(output) {
        return None;
    }
    Some(
        output
            .lines()
            .find_map(|line| line.strip_prefix("blockers:"))
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .unwrap_or("chapter did not reach an approved artifact state")
            .to_string(),
    )
}

pub(super) fn format_state_repair_blocker_result(
    project_path: &str,
    chapter_number: usize,
    settlement: &Value,
) -> String {
    let warnings = settlement
        .pointer("/validation/warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|warning| !warning.is_empty())
        .collect::<Vec<_>>();
    let observer_error = settlement
        .get("observer_error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|error| !error.is_empty());
    let detail = observer_error
        .map(str::to_string)
        .or_else(|| (!warnings.is_empty()).then(|| warnings.join("; ")))
        .unwrap_or_else(|| "final body state could not be validated".to_string());
    format!(
        "status: blocked\nworker: observer\nexecuted_tool: novel_studio\noperation: settle_chapter_state\nproject_path: {project_path}\nchapter_number: {chapter_number}\nruntime_effect: artifact.state_degraded\nchapter_status: state_repair_required\nprose_status: preserved_audit_passed\ntruth_status: not_committed\nblocker_kind: state_repair_required\nblockers: {detail}"
    )
}

pub(super) fn format_metadata_repair_blocker_result(
    project_path: &str,
    chapter_number: usize,
    write_result: &Value,
) -> String {
    let artifact_path = write_result
        .get("artifact_path")
        .and_then(Value::as_str)
        .or_else(|| {
            write_result
                .pointer("/chapter/path")
                .and_then(Value::as_str)
        })
        .unwrap_or("");
    let issues = super::metadata_issue_summary(write_result);
    format!(
        "status: blocked\nworker: editor\nexecuted_tool: novel_studio\noperation: repair_chapter_metadata\nproject_path: {project_path}\nchapter_number: {chapter_number}\nruntime_effect: artifact.needs_metadata_revision\ndraft_status: preserved_metadata_revision_required\nprose_status: preserved_audit_passed\nartifact_path: {artifact_path}\nblocker_kind: metadata_repair_exhausted\nblockers: chapter prose is preserved, but chapter title or metadata did not converge within bounded attempts\nmetadata_issues:\n{issues}"
    )
}

pub(super) fn format_chapter_approval_result(
    project_path: &str,
    requested_chapter: Option<usize>,
    approved: &Value,
) -> String {
    let success = approved
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let chapter_number = approved
        .get("chapter_number")
        .and_then(Value::as_u64)
        .or_else(|| approved.pointer("/chapter/number").and_then(Value::as_u64))
        .or_else(|| requested_chapter.map(|number| number as u64))
        .unwrap_or(0);
    if success {
        let state = approved.get("state").cloned().unwrap_or_else(|| json!({}));
        let approved_units = state
            .get("approved_units")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        return format!(
            "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: approve_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nruntime_effect: artifact.approved\nunit_count: {approved_units}\nsummary: 第 {chapter_number} 章已批准保存。"
        );
    }
    let error_kind = approved
        .get("error_kind")
        .and_then(Value::as_str)
        .unwrap_or("approval_not_ready");
    let error = approved
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("chapter is not ready for approval");
    format!(
        "status: blocked\nworker: writer\nexecuted_tool: novel_studio\noperation: approve_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nruntime_effect: artifact.approval_blocked\nblockers: {error_kind}: {error}"
    )
}

fn receipt_number(text: &str, key: &str) -> Option<u64> {
    for token in text.split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | ',')) {
        let Some((candidate_key, value)) = token.split_once('=') else {
            continue;
        };
        if candidate_key.trim() != key {
            continue;
        }
        let digits = value
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if let Ok(number) = digits.parse::<u64>() {
            return Some(number);
        }
    }
    None
}

pub(super) fn status_packet_reports_unapproved_chapters(status_packet: &Value) -> bool {
    let state = status_packet.get("state").unwrap_or(status_packet);
    if state
        .get("first_unapproved_chapter")
        .is_some_and(|value| !value.is_null())
    {
        return true;
    }
    let chapters = state.get("chapters").and_then(Value::as_u64);
    let approved = state.get("approved_chapters").and_then(Value::as_u64);
    matches!((chapters, approved), (Some(total), Some(done)) if done < total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_result_keeps_turn_units_separate_from_total_units() {
        let config = NovelWorkflowConfig {
            workspace: std::path::PathBuf::from("/tmp"),
            worker_label: "writer".to_string(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2500),
            chapter_count: 1,
            requested_start_chapter: None,
            existing_project_path: None,
            creation_draft_path: None,
            runtime: Default::default(),
        };
        let status_packet = json!({
            "state": {
                "approved_units": 14420,
                "chapters": 5,
                "approved_chapters": 5
            },
            "export": {
                "artifact_path": "/tmp/novel/exports/current.txt"
            }
        });

        let result = format_novel_workflow_result(
            &config,
            "/tmp/novel",
            &status_packet,
            false,
            true,
            1,
            1,
            None,
            "chapter 5 reused; path=/tmp/novel/exports/current.txt; unit_count=3290; total_units=14420; audit=passed",
        )
        .expect("format result");

        assert!(result.contains("unit_count: 3290"), "{result}");
        assert!(result.contains("total_units: 14420"), "{result}");
    }

    #[test]
    fn approval_blocker_preserves_the_actual_transaction_reason() {
        let result = format_chapter_approval_result(
            "/tmp/novel",
            Some(4),
            &json!({
                "success": false,
                "error_kind": "approval_requires_quality_gate",
                "error": "the final body still has deterministic hard blockers"
            }),
        );

        assert!(result.contains("operation: approve_chapter"), "{result}");
        assert!(
            result.contains("approval_requires_quality_gate"),
            "{result}"
        );
        assert!(
            !result.contains("revision did not converge"),
            "approval rejection must not be mislabeled as revision exhaustion: {result}"
        );
    }

    #[test]
    fn metadata_exhaustion_reports_metadata_operation_and_issues() {
        let result = format_metadata_repair_blocker_result(
            "/tmp/novel",
            17,
            &json!({
                "artifact_path": "/tmp/novel/chapters/0017.md",
                "metadata_gate": {
                    "repairable": ["chapter title duplicates a previous title"]
                }
            }),
        );

        assert!(
            result.contains("operation: repair_chapter_metadata"),
            "{result}"
        );
        assert!(result.contains("chapter_number: 17"), "{result}");
        assert!(result.contains("chapter title duplicates"), "{result}");
        assert!(!result.contains("operation: revise_draft"), "{result}");
    }
}
