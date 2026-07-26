# BenShu Documentation Hub

> Platform Positioning: `Windows Native` is BenShu's formal product path and primary host platform; `WSL / WSL2 / Linux` routes are development/testing lanes for fast iteration and must not be presented as the default product deployment path.

> Testing Positioning: current and future development/testing lanes follow a `GPU-first` principle by default; `CPU` remains a fallback, diagnostic, and compatibility path rather than the default source of performance conclusions.

Times have changed, and technology is reshaping us. We must embrace AI, maximizing our partnership with it. BenShu inherits the spirit of **benshu**, specifically addressing the pain points of OpenClaw: complex configuration, security concerns, token waste, and poor memory—especially on Windows. We provide an out-of-the-box, secure, and local-model-friendly experience for all Windows users, ensuring everyone can maximize the utility of AI and face the future.

## Architecture Overview: The Crates Ecosystem

The heart of BenShu resides in the `crates/` directory. Each crate is a specialized module designed for high performance and strict security.

### Core Intelligence & Storage
*   [**benshu-brain**](../crates/brain/README.md): **Cognitive Core.** Powers ReAct autonomous loops and meta-cognitive strategies, featuring an integrated **HEM (Hierarchical Episodic Memory)**. See [**Memory Core**](../crates/memory-core/README.md) for technical specs.
*   [**benshu-engram**](../crates/engram/README.md): **Memory Engine.** High-performance ACID-compliant storage based on Redb, supporting vector retrieval and hot-context backfilling.
*   [**benshu-knowledge**](../crates/knowledge/README.md): **Semantic Intelligence.** Distills conversations into atomic facts and builds knowledge graphs with a dedicated **Fact Promotion Gate**.
*   [**benshu-experience-core**](../crates/experience-core/README.md): **System Experience Store.** Stores task-execution experience, preflight checks, TTL, confidence, and evidence references in a separate `experience.redb`; it can project into an isolated engram namespace without entering the user knowledge base, and it is not automatically injected into the foreground chat prompt by default.

### Sensory & Inference
*   [**benshu-inference**](../crates/inference/README.md): **Inference Engine.** Native support for local LLMs and **SDXL Diffusion**. Implements **Dual-Encoder (CLIP-L/G)** and **CFG** guidance enhancement.
*   [**benshu-sensory**](../crates/sensory/README.md): **Sensory Bus.** Unified management for STT (Whisper), TTS (Piper), and Vision (LLaVA). Integrates the **Sovereign Memory Bus** for real-time cognitive tracing.
*   [**benshu-orchestrator**](../crates/orchestrator/README.md): **VRAM Orchestrator.** Manages metabolic pressure and GPU memory arbitration, supporting adaptive loading for SDXL (6GB) and Flux (16GB).

### Execution & Security
*   [**benshu-kernel**](../crates/kernel/README.md): **OS Kernel.** The central nervous system of the project, responsible for booting, service registration, and global bus management.
*   [**benshu-security**](../crates/security/README.md): The Fortress. Provides a 4-layer defense stack including shell firewalls, kernel-level sandboxing (Job Objects, bwrap, Seatbelt), and programmable WASM auditing.
*   [**benshu-runtimes**](../crates/runtimes/README.md): The Execution Engine. A multi-tiered system providing "Zero-Dependency" environments for Python, JavaScript (QuickJS/Bun/Node), and C/C++ (Smart GCC).
*   [**benshu-auth**](../crates/auth/README.md): The Vault. Handles AES-256 encrypted secret storage with OS Keyring integration and manages outbound OAuth2 connectivity.

### Integration & Connectivity
*   [**benshu-providers**](../crates/providers/README.md): LLM Bridge. Standardized adapters for OpenAI, Anthropic, Gemini, DeepSeek, and other LLM providers with built-in metadata management.
*   [**benshu-connectors**](../crates/connectors/README.md): IM Gateways. Bi-directional bridges for Telegram, Discord, Slack, and other messaging platforms.
*   [**benshu-mcp**](../crates/mcp/README.md): Protocol Hub. Implements the Model Context Protocol (MCP) to expose tools to external agents and IDEs.

### Sensory & Inference
*   [**benshu-sensory**](../crates/sensory/README.md): **Sensory Bus.** Unified management for STT, TTS, and Vision plugins.
*   [**benshu-inference**](../crates/inference/README.md): **Inference Engine.** Handles the execution of local LLMs (GGUF/Safetensors) with GPU acceleration and optimized runtime memory management.
*   [**benshu-orchestrator**](../crates/orchestrator/README.md): **VRAM Orchestrator.** Manages metabolic pressure and GPU memory arbitration for multi-model workflows.

### Infrastructure & Tools
*   [**benshu-builtin-tools**](../crates/builtin-tools/README.md): Standard Skills. A curated set of high-performance tools for filesystem access, web searching, and system interaction.
*   [**benshu-skill**](../crates/skill/README.md): **Dynamic Skills.** Implements the hot-loading and autonomous Forge protocols for on-the-fly capability expansion.
*   [**benshu-scheduler**](../crates/scheduler/README.md): **Task Scheduler.** Handles Cron tasks, deferred jobs, and long-term planning.
*   [**benshu-telemetry**](../crates/telemetry/README.md): **Telemetry Hub.** Unified management for Prometheus metrics, distributed tracing, and structured logging.
*   [**benshu-infra**](../crates/infra/README.md): Platform Foundation. Manages the portable toolchain (infra/bin), system utilities, and cross-platform abstracts.

---

## Applications

*   [**Gateway**](../apps/gateway/README.md): The high-performance API server. It coordinates agent lifecycles, provides OpenAI-compatible endpoints, and acts as the fleet orchestrator.
*   [**Panel**](../apps/panel/README.md): The premium Desktop GUI (egui-based). A visual mission-control center for managing agents, monitoring logs, and interacting with agents.

---

## Documentation Strategy
We follow a **"Crate-Native"** documentation approach. Detailed technical specifications, architecture diagrams, and development guides are located directly within their respective modules to ensure they stay in sync with the code. Use the links above to dive deep into any specific component.
