# BenShu Gateway (benshu-gateway)

The `gateway` is the central HTTP routing, orchestration, and security entry point for the BenShu system. It exposes the underlying `brain` (cognitive core), `engram` (memory), and `providers` (LLMs) as a standardized API.

## Core Responsibilities

*   **API Standardization**: Provides an OpenAI-compatible interface (`/v1/chat/completions`) and custom management endpoints.
*   **Universal Model Loading (Phase 21.6)**: The `/model/load` endpoint supports universal model registration and routes models to the local/runtime substrate or cloud/provider path based on declared capability.
*   **Fleet Management**: Manages the lifecycle of multiple independent agents simultaneously, with hardware-aware resource isolation.
*   **Security Barrier**: Implements `ApiGuard` middleware for `X-API-Key` validation and granular permission control.
*   **Production-Grade Perception**: Exposes local Whisper (STT) and Piper (TTS) via the `UnifiedAudioPlugin`, featuring automatic resampling and audio processing.
*   **MCP Infrastructure**: Implements the Model Context Protocol (MCP) to expose BenShu tools (e.g., Code Search, Browser, Shell) to external IDEs.
*   **Runtime Task Surface**: The foreground chat path now persists the emitted `runtime_task` into the durable state layer and returns `task_id`, `run_id`, and `trace_id` in `/api/chat` responses when those runtime references are available.
*   **Session Task Query**: The gateway now exposes `/api/sessions/{id}/tasks` as a stable read-only task view for a session. The DTO now includes `parent_task_id`, `root_task_id`, and `witness_id` so higher layers can correlate a persisted task with both runtime hierarchy and witness references.
*   **Run Trace Query**: The gateway now exposes `/api/traces/{id}` and reads the structured `RunTrace` object from the telemetry layer.
*   **Replay Query**: The gateway now also exposes `/api/traces/{id}/replay`, so `trace_id -> replay` is available as a stable read path instead of remaining telemetry-internal only.
*   **Witness Query Surface**: The gateway now exposes `/api/witnesses/{id}`, `/api/witnesses/{id}/bundle`, and `/api/witnesses/{id}/log`, so higher layers can read the summary, bundle, and structured witness log from telemetry.
*   **Profiler & Scorecard Query**: The gateway now also exposes `/api/profilers`, `/api/profilers/export`, `/api/scorecards`, and `/api/scorecards/{id}` as stable read paths for runtime diagnostics and regression summaries.
*   **Models & A2A Read Surface**: The gateway now exposes `/api/system/local-model-stack`, `/api/a2a/summary`, and `/api/a2a/throttle` so panel can align the `Models` tab and the A2A diagnostics surface with the current runtime mainline.
*   **Conversational Control Bridge**: Any inbound channel that publishes `InboundMessage` objects into the gateway bus can now reuse the same lightweight conversational control path for `stop / pause / reprioritize / interject`, so Telegram-style bots or IM adapters do not need their own interruption logic.
*   **Connector Bootstrap**: Configured Telegram connectors now bootstrap with the gateway server and immediately enter the shared polling/webhook loop, so real bot traffic can reach the BenShu message bus without a separate sidecar process.
*   **Session-Scoped Stop**: The gateway now exposes `/api/sessions/{id}/cancel` so UI/chat surfaces can stop only the current session task instead of relying on a global abort path.
*   **Stage A Smoke Coverage**: `scripts/run_stage_a_agent_smoke.sh` now includes gateway/connector regressions that validate `session tasks -> replay / witness / session-stop`, bot-style conversational control, and Telegram inbound parsing as one stable Stage A communication-aware read/write surface.

## Technology Stack

*   **Framework**: `axum` (Migrated to 0.8) utilizing low-overhead async handlers and `debug_handler` optimized state extraction.
*   **Runtime**: `tokio` for high-performance concurrent task management.
*   **State Management**: `AppState` powered by `dashmap` and `parking_lot` for thread-safe session and provider tracking.
*   **Hardware Awareness**: Integrated with `HardwareStatus` to provide real-time VRAM/RAM telemetry for the UI dashboard.

## Directory Overview

*   `src/api/`: Axum routers for Chat, Tools, Vault, Sessions, and Knowledge.
*   `src/blueprints/`: Configuration-driven agent templates (Coder, Trader, etc.).
*   `src/onboard.ps1`: Automated first-run setup for fresh environments.
*   `src/mcp.rs`: Model Context Protocol server implementation.

## Current AgentOS Notes

*   The gateway is now on the main `task / run / trace / replay / witness / profiler / scorecard` read path.
*   Task persistence, trace lookup, replay lookup, witness bundle/log lookup, profiler export/query, scorecard lookup, local-model-stack lookup, and A2A summary/throttle are all exposed as stable main-path reads.
