#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CARGO_BIN="${CARGO_BIN:-cargo}"

cd "${REPO_ROOT}"

run_case() {
  local test_name="$1"
  echo "[Stage A smoke] running: ${test_name}"
  "${CARGO_BIN}" test -p benshu-brain "${test_name}" --test runtime_governance_regression -- --nocapture
}

run_panel_case() {
  local test_name="$1"
  echo "[Stage A smoke] running: ${test_name}"
  "${CARGO_BIN}" test -p benshu-panel "${test_name}" -- --nocapture
}

run_case foreground_chat_emits_runtime_stage_trace_and_replay_contract
run_case real_harness_can_execute_foreground_runtime_case
run_case explicit_risk_context_upgrades_auto_tools_to_approval
run_case prime_session_keeps_visible_ownership_when_specialist_is_recommended
run_case preemptive_chat_cancels_prior_foreground_task_and_merges_context

echo "[Stage A smoke] running: gateway_runtime_read_paths_cover_task_replay_witness_and_session_stop"
"${CARGO_BIN}" test -p benshu-gateway gateway_runtime_read_paths_cover_task_replay_witness_and_session_stop -- --nocapture

echo "[Stage A smoke] running: bot_channel_commands_cover_stop_pause_reprioritize_and_interject"
"${CARGO_BIN}" test -p benshu-gateway bot_channel_commands_cover_stop_pause_reprioritize_and_interject -- --nocapture

echo "[Stage A smoke] running: telegram_update_parsing_covers_text_and_callback_semantics"
"${CARGO_BIN}" test -p benshu-connectors telegram_update_parsing_covers_text_and_callback_semantics -- --nocapture

run_panel_case poll_run_trace_promise_populates_witness_projection
run_panel_case poll_session_runtime_tasks_promise_clears_stale_trace_and_witness_selection
run_panel_case poll_session_runtime_tasks_promise_preserves_visible_trace_selection
run_panel_case poll_cancel_promise_clears_pending_and_sets_status

echo "[Stage A smoke] passed"
