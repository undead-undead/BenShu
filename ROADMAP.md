# BenShu: AgentOS 宏伟蓝图 (v3.2 - Final Truth)

本文件定义了 BenShu 的演化路径。我们的核心竞争优势是：**零配置、零 Docker、嵌入式沙箱、内置 QuickJS、硬件感知、以及个人主权。**

---

## 🧠 智能哲学：高度拟人化的智能体
架构遵循 **Forge-Loop 2.0**，旨在创建一个具备以下特征的数字实体：
*   **元认知 (Meta-Cognition)**：能够评估任务复杂性与风险，动态调整策略。
*   **自主演化 (Self-Forging)**：当现有工具失效时，自主编写、编译并热加载新技能。
*   **代谢感知 (Metabolic Adapting)**：根据宿主负载自主调整执行深度（“劳逸结合”）。

---

## 🏛️ 架构基石：九大核心域 (The 9 Pillars)

| 领域 | 核心模块 (Crates) | 状态 |
| :--- | :--- | :--- |
| **1. 核心认知层** | `benshu-brain` | ✅ 已就绪 |
| **2. 记忆与知识层** | `engram`, `knowledge` | ✅ 已就绪 |
| **3. 推理与模型层** | `inference`, `providers` | ✅ 已就绪 |
| **4. 执行与运行时** | `runtimes`, `skill` | ✅ 已就绪 |
| **5. 安全与沙箱层** | `security`, `auth` | ✅ 已就绪 |
| **6. 感官交互层** | `sensory`, `connectors` | ✅ 已完成 |
| **7. 调度与编排层** | `scheduler`, `orchestra` | ✅ 已就绪 |
| **8. 运行时基础设施** | `state`, `infra` | ✅ 已完成 |
| **9. 可观测性层** | `telemetry` | ✅ 已完成 |

---

## 🟢 Phase 1: Windows Native & Zero-Admin Foundation (Completed)
- [x] **Agnostic Identity**: Map local OS sessions as identity.
- [x] **Pixi/UV/QuickJS Hybrid**: Pixi for isolation, `uv` for 10x speed, QuickJS for lightweight JS execution.
- [x] **15MB Mini Git Bash**: Curated subset for bash compatibility on Windows.
- [x] **Self-Healing Runtime**: Panel UI supports "One-Click Repair" for toolchains.

## 🟢 Phase 2: Multi-Platform Security & Sandboxing (Completed)
- [x] **OS-Native Sandboxing**: Windows Job Objects, Linux Bubblewrap, macOS Seatbelt.
- [x] **Wasm Policy Guard**: Pre-flight validation for tool parameters.
- [x] **PID-Bound Handshake**: Internal IPC lockdown via random tokens.
- [x] **Immutable Audit Logs**: Append-only execution records in redb.

## 🟢 Phase 3: Distributed Architecture & Modularity (Completed)
- [x] **Crate Decoupling**: Standalone crates for connectors, security, runtimes, skills, knowledge.
- [x] **Filesystem Hygiene**: All artifacts moved to standardized `/data` paths.

## 🟢 Phase 4: High-Performance Knowledge (Engram V2) (Completed)
- [x] **Hybrid Search**: BM25 + Vector + RRF fusion.
- [x] **Quantization Tiering**: FP32 -> U8 -> INT4 -> Ternary.
- [x] **Hardware Acceleration**: AVX-512/Neon SIMD integration.

## 🟡 Phase 5: Professional UI & Local Multimedia (Ongoing)
- [x] **Offline Voice**: Whisper (STT) and Piper (TTS) native integration.
- [x] **Model Management**: UI for hot-swapping multi-language models.
- [x] **Unified Header (Control Center)**: Real-time system health and resource monitoring.
- [x] **In-Chat Execution Tracing**: Thought and tool blocks directly in the message stream.
- [ ] **Global Hotkeys**: `Alt + Space` quick input bar (Pending).
- [ ] **Startup Optimization**: Background service auto-launch.

## 🟢 Phase 6 - 9: Explainable Governance & Swarm Intelligence (Completed ✅)
- [x] **Color-Coded Execution Trace**: Green/Yellow/Red risk levels for tools.
- [x] **Omni-Channel Authorization**: Approval flow via Telegram/Discord/Local UI.
- [x] **Shadow Backup & Rollback**: Automatic pre-action backup (`ShadowBak`) and two-way undo.
- [x] **Visual Job Builder**: Persistent Cron jobs with redb-backed history.
- [x] **Dynamic Swarm Dispatcher**: Automated task decomposition and role delegation.
- [x] **Consensus Safety Audit**: Multi-agent cross-verification for red-level tools.

## 🟢 Phase 11: Multi-modal Native Perception (Sensory Hub) (Completed ✅)
- [x] **Sensory Hub Integration**: Unified OCR, STT, and TTS across all modules.
- [x] **Native Multimodal Ops**: CLIP and LLaVA architectures via Candle.
- [x] **Hardware-Aware Fallback**: Atomic selection of CUDA/Metal/SIMD kernels.
- [x] **Automated SOM (Set-of-Mark)**: Visual UID tags for precise UI grounding.

## 🟢 Phase 12 - 14: Fractal Intelligence & Deep Evolution (Completed ✅)
- [x] **Fractal Agent Lifecycle**: Cellular fission (Split) and hierarchical distillation (Merge).
- [x] **Metabolic Adaptation**: Resource-aware reasoning strategy (ToT -> ReAct scaling).
- [x] **Autonomous Experience Mining**: Persistent RAG for "searching its own past."
- [x] **Self-Distillation**: Ephemeral scripts promoted to permanent Atomic Tools.
- [x] **Reflexion 2.0**: Anti-pattern library in redb to prevent repetitive failures.

## 🟡 Phase 16: Triple-Engine Architecture (Ongoing)
- [x] **System 2 Reflection (Tactical SLM)**: Global orchestrator using qwen2.5-0.5b for reflexive reasoning.
- [x] **Entropy Monitor**: SLM-based detection for reasoning recursion loops.
- [x] **Speculative Orchestration**: Parallel validation of reasoning steps.
- [x] **Unified Backend Arbitrage**: Support for GGUF (Dual-File) and Safetensors.

## 🟡 Phase 17/18: Cognitive Core Hardening (Ongoing)
- [x] **HNSW O(log N)**: Replace linear vector scanning with efficient indexing.
- [x] **Bitwise SIMD Search**: Direct comparison of Ternary quant-codes.
- [x] **Redb Triple Indexing**: Dedicated (SPO/OPS/POS) tables for 5ms fact retrieval.
- [x] **Transparent Encryption**: AES-GCM-256 integrated into the memory loop.
- [ ] **NVMe Direct-to-VRAM**: Windows DirectStorage integration for massive vector indexes.

## 🟢 Phase 19: Jarvis Vision — Genetic & Cognitive Synthesis (Completed ✅)
- [x] **Poincaré Distance Metric**: Hyperbolic geometry for non-Euclidean hierarchical indexing.
- [x] **Windows SIMD Acceleration**: Fused AVX-512 kernels for hyperbolic d(u,v).
- [x] **N-Ary Hyperedges**: KG upgraded from triples to multi-entity events (Meetings, Projects).
- [x] **Behavioral Persistence Bridge**: ACID-safe hyperedge commits to Redb.
- [x] **Event Traversal API**: Natural language event recall (e.g. "Meeting about X with person Y").
- [x] **Behavioral Autopilot**: speculative pre-warming of memory tiers based on intent prediction.
- [x] **Cognitive Tension Management**: Automated reasoning-depth adjustment (System 1/2 switch).

---

## 🔍 技术亮点对比 (Competitive Advantage)

| 特性 | 优势 | 状态 |
| :--- | :--- | :--- |
| **零权限沙箱** | 无需 UAC 限制文件/网络访问 | ✅ |
| **自愈环境** | 自动修复 Python/Node 依赖损坏 | ✅ |
| **全便携运行** | 内置 Bun, QuickJS, Git, Mini Bash, GCC | ✅ |
| **影子备份** | 所有文件修改动作均可无损 Undo | ✅ |
| **硬件共鸣** | AVX-512 / TensorCore / Metal 自动优化 | ✅ |
| **主权记忆** | 本地 AES-256 加密的事实化存储 | ✅ |

---
**标记**: 此文档为 BenShu 开发的最高真值来源。
**更新日期**: 2026-03-18 (Phase 19 核心功能全量验收版)