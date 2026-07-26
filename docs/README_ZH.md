# BenShu 文档中心

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 测试链口径: 当前与后续开发测试链默认遵循 `GPU 优先原则`；`CPU` 路径仅作为回退、诊断与兼容性验证，不作为默认性能结论来源。

时代已经变迁，科技正在改变我们，我们要拥抱 AI，最大化地将 AI 作为伙伴。本项目传承 [**BenShu**](https://benshu.xyz) 精神，旨在解决 OpenClaw 等工具在 Windows 上配置繁琐、安全性欠缺、Token 浪费及记忆力差等痛点，并弥补那些超小型实现通常只能借助宿主机器运行时环境、并非真正原生的遗憾。我们为所有 Windows 用户提供开箱即用、安全且支持本地大模型的体验，让每个人都能最大化利用 AI，直面未来。

## 架构概览：Crates 生态系统

BenShu 的核心位于 `crates/` 目录。每个 crate 都是一个专为高性能和严格安全而设计的专业模块。

### 核心智能与存储 (Cognitive & Storage)
*   [**benshu-brain**](../crates/brain/README.md): **认知大脑。** 驱动 ReAct 自主循环，集成 **HEM (阶梯记忆系统)**。详见 [**Memory Core**](../crates/memory-core/README.md) 技术规范。
*   [**benshu-engram**](../crates/engram/README.md): **记忆引擎。** 基于 Redb (ACID) 的高性能持久化存储，支持向量检索与热点记忆回填。
*   [**benshu-knowledge**](../crates/knowledge/README.md): **语义智能。** 提炼对话为原子化事实，构建知识图谱。支持事实确认门控 (Fact Promotion Gate)。
*   [**benshu-experience-core**](../crates/experience-core/README.md): **系统经验库。** 使用独立 `experience.redb` 保存任务执行经验、preflight、TTL、置信度和证据引用；可投影到 engram 的隔离 namespace，但不进入用户知识库，也不会默认自动注入前台聊天 prompt。
*   [**benshu-state**](../crates/state/README.md): **状态机。** 管理智能体生命周期状态、任务快照与持久化上下文。

### 执行与安全
*   [**benshu-kernel**](../crates/kernel/README.md): **OS 内核。** 系统的生命中枢，负责引导（Boot）、服务注册与全局总线管理。
*   [**benshu-security**](../crates/security/README.md): **安全要塞。** 提供 4 层防御栈，包括 Shell 防火墙、内核级沙箱（Job Objects, bwrap, Seatbelt）以及可编程的 WASM 审计。
*   [**benshu-runtimes**](../crates/runtimes/README.md): **执行引擎。** 多级回退系统，为 Python、JavaScript (QuickJS/Bun/Node) 和 C/C++ (Smart GCC) 提供“零依赖”运行环境。
*   [**benshu-auth**](../crates/auth/README.md): **加密金库。** 处理 AES-256 加密的事实存储，集成操作系统密钥链（Keyring），并管理外向 OAuth2 连接。

### 集成与互联
*   [**benshu-providers**](../crates/providers/README.md): **LLM 桥接。** 为 OpenAI、Anthropic、Gemini、DeepSeek 等主流模型提供标准适配器及内置元数据管理。
*   [**benshu-connectors**](../crates/connectors/README.md): **即时通讯网关。** 为 Telegram、Discord、Slack 等平台提供双向通讯桥梁。
*   [**benshu-mcp**](../crates/mcp/README.md): **协议中心。** 实现模型上下文协议 (MCP)，将内部工具暴露给外部智能体和 IDE。

### 感官与推理
*   [**benshu-sensory**](../crates/sensory/README.md): **感官总线。** 统一管理 STT (语音转文字)、TTS (文字转语音) 和视觉分析插件。
*   [**benshu-inference**](../crates/inference/README.md): **推理引擎。** 负责本地大模型 (GGUF/Safetensors) 的执行，提供 GPU 加速适配与高性能 KV 缓存管理。
*   [**benshu-orchestrator**](../crates/orchestrator/README.md): **显存调度。** 负责多模型并行的显存仲裁与代谢压力感知，确保硬件不爆屏。

### 基础设施与工具
*   [**benshu-builtin-tools**](../crates/builtin-tools/README.md): **标准技能。** 经过精选的高性能工具集，涵盖文件系统访问、网页搜索和系统交互。
*   [**benshu-skill**](../crates/skill/README.md): **动态技能。** 实现技能（Skill）的热加载与自主锻造（Forge）协议。
*   [**benshu-scheduler**](../crates/scheduler/README.md): **任务调度。** 处理 Cron 任务、延迟作业与长期规划。
*   [**benshu-telemetry**](../crates/telemetry/README.md): **遥测中心。** 统一管理 Prometheus 指标、分布式追踪（Tracing）与结构化日志。
*   [**benshu-infra**](../crates/infra/README.md): **平台底座。** 管理便携式工具链 (`infra/bin`)、系统实用程序和跨平台抽象层。

---

## 应用程序 (Applications)

*   [**Gateway**](../apps/gateway/README.md): 高性能 API 服务端。负责协调智能体生命周期，提供兼容 OpenAI 的接口，并充当整个集群的编排器。
*   [**Panel**](../apps/panel/README.md): 基于 egui 的高级桌面 GUI。为管理 Agent、监控日志和与智能体交互提供直观的任务控制中心。

---

## 文档策略
我们遵循 **"Crate-Native" (组件原生)** 的文档模式。详细的技术规格、架构图和开发指南直接存放于各自的模块目录中，以确保文档与代码同步。请使用上方链接深入了解任何特定组件。

当前与整体产品方向最相关的核心工程文档包括：

- [开发准则](./DEVELOPMENT_STANDARDS_AGENTOS.md)
- [Agent 背景信息窗压缩主线开发方案](./secondary/BENSHU_BACKGROUND_COMPRESSION_MAINLINE_PLAN_ZH.md)
- [Truth / Verification 主线计划](./secondary/BENSHU_TRUTH_AND_VERIFICATION_MAINLINE_PLAN_ZH.md)
- [AgentOS 重构执行计划](./secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md)
- [统一 Tracing 契约](./secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md)
- [Prime Agent 架构立场](./secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md)
- [个人 Jarvis 落地路线图](./secondary/BENSHU_PERSONAL_JARVIS_ROADMAP_ZH.md)

当前与后续本地压缩优化方向相关的专题文档包括：

- [本地模型栈与媒体预处理统一计划](./secondary/BENSHU_LOCAL_MODEL_STACK_PLAN_ZH.md)
