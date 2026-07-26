# BenShu Panel (benshu-panel)

The `panel` is the official Graphical User Interface (GUI) mission-control center for BenShu. It is a cross-platform desktop application built for high performance and sleek aesthetics.

## Key Features

*   **Agent Mission Control**: Visually configure the Agent's instructions, models, and skills.
*   **Worker Policy Editing**: Worker artifact policies are edited in the Agent Identity Editor and saved through the gateway policy endpoint into `data/agents/<worker>/artifact_policy.yaml`, keeping `AGENT.md` focused on identity and equipped tools.
*   **Knowledge Hub**: Manage the `engram` vector database—upload files and manage indexed knowledge via a GUI.
*   **Prime Chat Center**: The default chat surface is centered on the main `benshu` agent, while specialist delegation and execution feedback are surfaced through runtime tasks and A2A diagnostics instead of per-agent chat windows.
*   **Observability**: Real-time metrics for token usage, latency, and background cron statuses.
*   **Models Workspace**: The panel now exposes a dedicated `Models` tab split into `Cloud` and `Local`, aligning cloud providers and the local model stack under one product surface instead of older system-only model controls.
*   **Runtime Host Control**: The panel API can restart configured runtime hosts through `/api/system/runtime-hosts/{role}/restart`, using only `runtime_host_control` settings from panel/runtime configuration.
*   **Skill Marketplace**: Browse and install Python/Wasm/Native skills from the community repository.
*   **AgentOS Runtime Task View**: The panel now consumes runtime-grade `task_id / run_id / trace_id` references from chat responses, can query per-session runtime tasks, and renders a stable read-side runtime console inside the Tasks page.
*   **Trace / Witness / Profiler Read Path**: The Tasks page can load `RunTrace`, `Replay`, `WitnessSummary`, `WitnessBundle`, `WitnessLog`, `ProfilerArtifact`, profiler query/export results, and embedded scorecard data.
*   **Approval Queue**: The Tasks page exposes the live high-risk approval queue, including challenge code visibility and stateful approve/reject actions that flow back through the panel state instead of ad-hoc UI calls.
*   **Task Hierarchy View**: The runtime task surface now renders a first read-only parent-child task graph derived from durable `parent_task_id / root_task_id` relationships.
*   **Governance & Budget Diagnostics**: Runtime task details now include `Runtime Governance` and `Context & Budgeting` cards for approval, owner, clarification, recovery, artifact lifecycle, and budget/fallback evidence.

## Design Concept: "Pro-Visual"
The Panel uses an **immediate-mode GUI** (`egui`) for maximum responsiveness (0ms UI lag) and a "Safety-First" visual language. It supports both Dark and Light modes with custom premium HSL palettes.

## Technology Stack

*   **GUI Framework**: `eframe` (`egui`) for OpenGL/WGPU accelerated rendering.
*   **Internationalization**: Native support for English and Chinese (`HarmonyOS Sans SC` bundled).
*   **Async Core**: `tokio` handles non-blocking background API polling and downloads.
*   **Client**: `reqwest` for robust communication with the `gateway`.

## Directory Overview

*   `src/app_state.rs`: The unified source of truth for the entire application state.
*   `src/app.rs`: The primary drawing loop and navigation logic.
*   `src/api.rs`: Async service layer for Gateway interactions.
*   `src/i18n/`: Translation dictionaries for multi-language support.

## Current AgentOS Notes

*   The panel now has a stable read-side runtime console for `task / trace / replay / witness / profiler / governance / budgeting`.
*   Stage A smoke now also locks the panel-side runtime state machine for `trace -> witness` projection, runtime selection retention/cleanup, and session-stop promise completion through `scripts/run_stage_a_agent_smoke.sh`.
*   The `Models` tab now aligns cloud providers and the local model stack under `Cloud / Local` instead of the older system-page model controls.
*   Runtime host restart is a control-plane action, separate from the shared local model pool unload/prune/clear actions. The restart path does not hardcode a bridge or model; it executes the configured `runtime_host_control.<role>` command or Windows service binding.
*   Agent policy saving is split: `PUT /api/system/agent/detail` updates identity/tools/runtime fields, while `PUT /api/system/agent/artifact-policy` updates the worker policy YAML. Policy-only edits call only the policy endpoint.
*   A2A is exposed as a runtime diagnostics/read surface rather than a second user-facing chat front.
*   Artifact support is still a read-only list sourced from `RunTrace.artifacts`, not a dedicated artifact console.
*   Task graph support is still a minimal hierarchy renderer rather than the final interactive graph console.
