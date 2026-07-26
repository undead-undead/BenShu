# BenShu 开发准则

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 测试链口径: 当前与后续开发测试链默认遵循 `GPU 优先原则`。凡是涉及本地主脑/小模型真实性能、上下文体积、工具调用时延、背景压缩成本的测试，默认必须优先在可用 GPU 路线上验证；`CPU` 路径仅作为回退、诊断、兼容性与极端降级验证，不应被当成默认性能测试结论来源。

> 状态更新: 2026-03-22 | 适用范围: 全 Workspace (`apps/*` + `crates/*`) | 定位: AgentOS 级工程规范

## 0. 文档目标

本规范用于统一 `BenShu` 全仓库的工程标准，适用于以下模块族：

- 认知与记忆: `brain`、`engram`、`knowledge`、`state`
- 执行与安全: `kernel`、`security`、`runtimes`、`auth`
- 推理与模型: `providers`、`inference`、`mcp`
- 通信与互联: `comm`、`connectors`
- 调度与编排: `scheduler`、`orchestrator`
- 感官与基础设施: `sensory`、`builtin-tools`、`infra`、`telemetry`
- 应用层: `apps/gateway`、`apps/panel`

本规范不是“代码风格建议”，而是面向 AgentOS 运行时的生产级约束。所有新功能、重构和修复默认都应满足本规范；如需例外，必须在代码与 PR/提交说明中明确写出原因。

配套执行文档：

- `secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md`
- `secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md`
- `secondary/BENSHU_BACKGROUND_COMPRESSION_MAINLINE_PLAN_ZH.md`

## 0.1 文档分工

为避免文档体系继续膨胀，BenShu 的工程文档按“核心文档”与“次级专题文档”分层维护。

核心文档：

- `DEVELOPMENT_STANDARDS_AGENTOS.md`
  - 唯一总规范
  - 回答“什么是必须长期成立的工程约束”
- `secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md`
  - 唯一执行蓝图
  - 回答“当前按什么顺序施工、什么算阶段完成”
- `secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md`
  - 唯一 tracing 契约
  - 回答“trace / replay / witness / scorecard 的主语义是什么”
- `secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md`
  - 唯一前台产品架构立场文档
  - 回答“为什么 BenShu 是单一前台主代理，而不是平级 swarm 产品”

次级专题文档：

- `secondary/BENSHU_HARDNESS_DESIGN_PRINCIPLES.md`
- `BRAIN_CAPABILITY_PRIORITY_AND_INTEGRATION_AUDIT.md`
- `secondary/BENSHU_PERSONAL_JARVIS_ROADMAP_ZH.md`
- `secondary/BENSHU_BACKGROUND_COMPRESSION_MAINLINE_PLAN_ZH.md`

这些文档用于保留专题判断、设计背景和阶段性审计结论，但不再作为长期主约束来源。

补充说明：

- `secondary/BENSHU_BACKGROUND_COMPRESSION_MAINLINE_PLAN_ZH.md`
  - 当前作为 Agent 背景信息窗压缩方向下的正式专题文档存在
  - 回答“如何在不重写 `ContextManager / MemoryManager / EngramMemory / SLM tactical pre-pass` 的前提下，把背景信息窗压缩做成产品级主线”
  - 它不是底层 `KV compression` 文档，而是面向 Agent backend context window 的连续性、稳定性和背景治理专题

维护规则：

- 若专题结论已经进入主路径并稳定成立，应提炼进核心文档，而不是继续在专题文档中平行维护大段重复说明。
- 若核心文档与次级专题文档冲突，以核心文档为准。
- 若“专项能力实施计划”与“结构收口 / 代码拆分计划”并行存在：
  - 专项能力计划负责回答“能力目标是什么”
  - 结构收口 / 代码拆分计划负责回答“代码应落在哪、模块如何归位、旧代码何时删除”
  - 一旦两者冲突，代码落点、模块归位与旧代码回收顺序应优先服从结构收口 / 代码拆分计划
- 次级专题文档应优先保留：
  - 为什么这样设计
  - 当时的审计结论
  - 后续深化方向
- 次级专题文档不应继续承担：
  - 最终规范
  - 最终执行顺序
  - 最终完成口径

---

## 1. 总体原则

### 1.1 生产优先，而非演示优先
- 设计必须优先考虑可回收、可审计、可降级、可恢复。
- 不接受“功能先跑起来，生命周期后补”的实现方式。
- 对外宣称“已实现”的能力，必须在主路径中真实接线，而不是只存在于孤立模块或实验入口。

### 1.2 显式边界，高于隐式魔法
- 安全、审批、配置、生命周期、背压、运行模式必须显式建模。
- 禁止把关键系统语义只藏在环境变量、`task_local!`、默认值或隐式全局单例里。
- “能工作”不是边界，“明确知道为何工作、何时失效、如何回收”才是边界。

### 1.3 单一职责，但允许高内聚
- crate 之间按领域分层，不允许基础设施反向吸收业务逻辑。
- 高层模块可以组合多个低层能力，但不得绕过中间抽象直接穿透依赖边界。
- 为了“省一次封装”破坏领域边界，长期成本通常高于短期收益。

### 1.4 主路径优先于旁路能力
- 新能力若未接入主工厂路径、主 API 路径或主 UI 路径，不得标记为完成。
- 后台 worker、治理链路、审计链路都必须覆盖主路径。
- 当主线路径已进入结构收口阶段时，新增能力或继续补齐专项计划时不得无视当前模块归位方案继续把实现堆回历史热点文件；必须优先按正在执行的结构收口计划落入目标模块，再进入主路径。

### 1.5 对硬编码保持极高警惕
- 硬编码必须慎重、慎重、再慎重。
- 凡是可能演化为配置、策略、协议、能力声明、平台差异或用户可变行为的内容，默认不应写死在代码里。
- 如确需硬编码，必须同时说明：
  - 为什么当前不能配置化
  - 影响范围是什么
  - 未来如何回收
  - 如何测试它不会成为隐性主路径约束

### 1.6 产品北极星是“个人真正的贾维斯”
- BenShu 的长期战略目标不是做一个单点能力很强的演示系统，而是成为用户可长期信任、可持续协作、可跨任务连续工作的个人 Agent 系统，也就是“个人真正的贾维斯”。
- 这意味着系统必须优先建设：
  - 长期记忆
  - 稳定执行
  - 多工具协作
  - 可解释治理
  - 审批与安全边界
  - 跨任务连续性
- 任何专项能力，包括代码能力、文档能力、知识能力、多模态能力，都应服务于这条北极星，而不应反过来绑架整体架构。
- 不以“做市场上最强的通用单项 Agent”作为首要目标；首要目标始终是把 BenShu 建成一个用户愿意长期托付任务、上下文和工作流的个人 Agent 操作系统。

### 1.7 所有机制都必须服务于用户，而不是管控用户
- BenShu 的所有策略、治理、保护、审批、背压与安全机制，第一目标都是为用户服务，而不是限制、训诫、驯化或管控用户。
- 系统允许做的“限制”只应指向两类目标：
  - 最大限度保护用户避免数据、资产、隐私、执行结果与工作流损失
  - 最大限度防止系统进入失控、风暴、死循环、资源耗尽或危险写操作状态
- 禁止引入任何以“约束用户行为”本身为目的的产品机制，尤其禁止把平台风控、增长管制、惩罚式封禁、敌对式限流语义带入个人 Agent 场景。
- 当确需对某条路径做节流、降级、延迟、确认、回退或中止时，默认语义必须是“保护用户与保护系统稳定性”，而不是“怀疑用户、惩罚用户或剥夺用户控制权”。
- 所有用户保护机制都应尽量遵循：
  - 优先帮助用户完成目标
  - 无法完整完成时优先安全降级
  - 必要时明确说明原因、影响与下一步恢复方式
  - 不得把用户整体当作风险对象，只能把具体失控行为、危险操作或资源风暴当作治理对象

---

## 2. 架构级准则

## 2.1 分层原则

### A. 基础设施层
适用: `infra`、`telemetry`

- 只提供通用能力，不承载业务决策。
- 允许平台差异封装，但禁止引入认知层、策略层、业务层判断。
- 所有跨平台分支必须有明确的 OS 能力探测与失败回退。

### B. 领域核心层
适用: `brain`、`engram`、`knowledge`、`state`、`comm`

- 表达核心领域模型与规则。
- 不得直接依赖 UI 细节、HTTP 细节、桌面状态。
- 不得通过“偷读环境变量”完成关键配置。

### C. 安全与执行层
适用: `security`、`runtimes`、`auth`

- 默认拒绝，显式授权。
- 运行时隔离、凭证处理、审计记录必须独立于业务 happy path。
- 安全失败应可见、可日志化、可追踪，不能静默降级为全开放。

### D. 编排与接入层
适用: `kernel`、`scheduler`、`orchestrator`、`providers`、`mcp`、`connectors`

- 负责装配与协调，不直接篡改下层内部状态。
- 负责把配置、依赖和上下文显式传递给领域对象。

### E. 应用层
适用: `apps/gateway`、`apps/panel`

- 负责入口、模式切换、会话、API 契约、用户操作反馈。
- 不允许把核心业务规则写死在 UI 或路由层。
- 不允许让应用层和核心层各自维护一套互相漂移的契约。

---

## 3. Rust 语言级准则

## 3.1 内存安全

- 严格遵循所有权、借用、生命周期，不为图省事牺牲可验证性。
- `unsafe` 默认禁止，只有在以下场景才可考虑：
  - 零拷贝热点路径且已证明收益明显
  - FFI 边界
  - 平台底层系统调用封装
- 每个 `unsafe` 块必须带 `SAFETY:` 注释，说明：
  - 内存托管方
  - 生命周期保证来源
  - 线程安全前提
  - 为什么安全
- 核心认知逻辑、治理逻辑、安全决策逻辑中禁止新增 `unsafe`。

## 3.2 错误处理

- crate 内部优先使用 `thiserror` 建立稳定错误类型。
- 应用层和装配层可使用 `anyhow::Result`，但必须补 `.context(...)`。
- 禁止直接吞错。
- 禁止把安全失败、权限失败、审计失败伪装成普通空结果。
- 错误信息不得泄露：
  - 密钥
  - 完整凭证
  - 用户敏感内容
  - 内部不安全状态细节

## 3.3 抽象成本控制

- 热点路径优先静态分发与零成本抽象。
- 边界层、插件层、协议适配层允许使用 `dyn Trait` 换取可扩展性。
- 不要把“禁止动态分发”写成宗教规则，应按热点与边界区分。
- 序列化必须使用静态 schema 驱动的方式，避免运行时反射式设计。

## 3.4 显式配置

- 所有关键配置必须能通过显式对象传递，例如 `Config` / `Options` / `Context`。
- 允许在应用装配层从 env、文件、CLI 读取配置，但读取后必须转换为明确配置对象再下传。
- 禁止核心域对象在深层逻辑中自行读取环境变量决定行为。

## 3.5 日志与可观测性

- 默认使用结构化日志。
- 关键异步链路必须带 trace/span 关联。
- 主路径 tracing 必须服从 `secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md`，不得在不同 crate 中各自发明平行的 `trace / run / witness / scorecard` 语义。
- 任何日志中禁止打印：
  - API key
  - session token
  - internal key
  - 完整 OAuth 凭证
- 对用户可见的错误信息与内部日志要分层处理。

---

## 4. 并发与生命周期准则

## 4.1 生命周期分层

- `cancel current task` 与 `shutdown service/agent` 必须是两套语义。
- 长生命周期对象必须区分：
  - 生命周期 token
  - 当前任务 token
  - 资源关闭句柄
- 禁止把“打断当前任务”直接映射为“取消根生命周期”。

## 4.2 后台任务管理

- 所有长期 worker 必须具备：
  - 显式启动点
  - 显式 shutdown 信号
  - 可等待的 `JoinHandle` / `JoinSet`
  - 明确的退出条件
- 禁止依赖“sender 被 drop 了所以 worker 迟早会停”这种间接退出模型。
- 所有后台任务必须接入主 runtime 管理，不允许无主裸 `tokio::spawn` 长期悬挂。

## 4.3 并发共享原则

- 只有真实跨线程共享的核心类型才要求 `Send + Sync`。
- 不要为了满足 `Send + Sync` 机械性套 `Arc<Mutex<_>>`。
- 优先选择：
  - 不共享
  - 消息传递
  - 所有权转移
  - 只读共享
  - 最后才是可变共享

## 4.4 背压与队列语义

- 队列任务不得静默丢弃。
- 当系统进入高压感或节流状态时，任务只能：
  - 延迟
  - 重排队
  - 显式失败并记录原因
- 禁止在 `recv()` 之后因 `continue`、早退、覆盖状态而悄悄吞任务。

---

## 5. 治理与安全准则

## 5.1 显式治理上下文

- 风险分数、审批策略、工具策略、可信工作区、运行模式必须显式建模为 context/config。
- `task_local!` 可以作为便捷访问层，但不能承载安全真义。
- 跨 `spawn`、跨 agent、跨 worker 的治理继承必须显式发生。

## 5.2 默认最小授权

- 安全策略默认拒绝，不默认全开。
- 本地模式、嵌入模式、独立网关模式必须使用不同的安全基线。
- 临时绕过、开发后门、localhost 免鉴权都必须显式开关，并限制在开发环境。

## 5.3 用户保护优先于用户管控

- 治理策略的目标是保护用户，而不是管理用户。
- 对个人用户产品，默认不设计惩罚式封禁、敌对式风控或“把用户挡在系统外面”的机制。
- 对查询、工具、写操作、资源预算、节流与恢复策略，应优先采用：
  - 提醒
  - 降级
  - 延迟
  - 合并重复请求
  - 暂停失控链路
  - 显式确认高风险操作
- 只有在继续执行会明显伤害用户、破坏数据、泄露隐私、扩大损失或导致系统失控时，才允许中止当前具体路径；该中止应被表述为保护性中止，而不是对用户的惩罚。
- 所有这类策略都必须保持用户可解释、可恢复、可继续，不得把平台治理语义偷换成用户惩戒语义。

## 5.3 凭证与秘密管理

- 所有秘密仅允许出现在：
  - 密钥库
  - 加密存储
  - 短生命周期内存对象
- 禁止写入：
  - 日志
  - snapshot API
  - panic 文本
  - 调试输出

## 5.4 审计要求

- 高风险工具调用必须具备审计记录。
- 安全拒绝必须可追溯。
- 审计记录应以元数据为主，避免记录敏感 payload 原文。

---

## 6. AgentOS 专项准则

## 6.1 `brain`

- 不允许把“运行时能力是否存在”建立在偶然启动顺序上。
- 子 Agent 派生必须显式继承治理上下文。
- 不允许多个 Agent 共享可变运行态对象来表示“学习能力”或“调度能力”。
- 记忆、进化、治理、执行必须有清晰责任边界。

## 6.2 `kernel`

- 负责服务装配、生命周期注册和统一启动闭环。
- 不得把领域逻辑偷偷塞回工厂函数。
- 对外提供的是稳定装配语义，不是隐式魔法。

## 6.3 `gateway`

- API 契约必须与 panel/client 保持一致。
- 路由存在即语义存在，禁止前端调用未注册接口。
- 运行模式必须显式化，例如 `Embedded` 与 `Standalone`。

## 6.4 `panel`

- 面板只负责控制与展示，不复制核心业务判断。
- 所有管理请求必须走统一鉴权路径。
- UI 文案不得掩盖后端失败。

## 6.5 `security` 与 `runtimes`

- 沙箱、审计、权限、隔离是硬边界，不接受“失败后直接裸跑”的回退。
- 外部工具链探测、修复、重装都必须是可观测、可重试、可日志化的。

## 6.6 `comm` 与 `connectors`

- 通信协议只管传输与元数据，不吞业务错误。
- 多租户、签名、寻址属于一等设计，不是附加属性。
- 所有跨域消息必须可追踪来源、去向和消息 ID。

### 6.6.1 通讯软件与渠道观测要求

Telegram、飞书、Discord、邮件、Webhook、企业 IM 或其他通讯软件接入后，必须被视为正式运行通道，而不是“只负责收发消息的边缘适配器”。

最低要求如下：

- 每条入站消息必须具备稳定的 channel message id，并能关联到 `channel / session / user / thread / trace / task / run`
- 每条出站消息必须记录发送时间、目标 channel、发送结果、失败原因与重试状态
- channel 适配层必须保留 `inbound -> routing -> task/run -> outbound` 的因果链
- connector 不得只产生日志文本，必须产出结构化事件，至少包括：
  - `channel_name`
  - `channel_session_id`
  - `channel_user_id`
  - `message_id`
  - `direction`
  - `timestamp`
  - `delivery_status`
  - `retry_count`
  - `trace_id`
  - `task_id`
  - `run_id`
- 通讯渠道的失败必须区分：
  - 拉取失败
  - 路由失败
  - Agent 执行失败
  - 发送失败
  - 外部平台限流/拒绝
- 所有渠道事件必须能进入 trace，并可在必要时进入 witness 的 evidence 引用，而不是散落在普通日志中
- connector 可采集的观测字段必须遵循最小必要原则，不得默认采集无关隐私内容
- 端到端加密内容、第三方客户端内部 UI 状态、用户本地软件内部实现，不得被当作默认观测目标；主路径观测对象应是通过 BenShu 接入的消息、事件与执行链路
- 通讯软件观测默认采用被动、接入点驱动模式：以 webhook、bot、mention、已授权 inbox、显式订阅事件为边界，不以主动全量抓取历史消息或平台全量监听为默认能力
- 如需更主动的增量同步、线程追踪或邮箱拉取，必须建立明确授权范围、速率限制、失败回退与审计记录，不得把这类能力伪装成普通 channel observability

系统必须明确区分：

- `channel observability`
  目标是看清消息输入输出、路由、失败、重试和执行关联
- `client instrumentation`
  这是更高风险、非默认、需要额外授权的能力，不得与正常 channel observability 混为一谈

## 6.7 `engram` / `knowledge`

- 数据层不允许全量扫描替代索引设计。
- 长期存储默认考虑 OOM、磁盘膨胀、事务一致性。
- 记忆淘汰、事实晋升、冲突解决必须可解释、可审计。

---

## 7. 测试准则

## 7.1 测试分层

- 单元测试: 验证纯逻辑、边界条件、错误分支
- 集成测试: 验证 crate 间装配与协议契约
- 端到端测试: 验证 gateway/panel/agent 主路径闭环

## 7.2 必测场景

以下场景新增功能时必须覆盖：

- 生命周期测试
  - 当前任务取消不会杀死长期 worker
  - shutdown 能回收后台任务
- 并发测试
  - 多 worker 并发不出现共享状态踩踏
- 治理测试
  - 子 Agent 正确继承审批、风险、工作区边界
- 安全测试
  - 无凭证请求被拒绝
  - 凭证不会出现在日志与快照
- 背压测试
  - 高压力下任务延迟但不静默丢失
- 契约测试
  - panel/client 调用的 API 在 gateway 侧真实存在

## 7.3 测试真实性

- 禁止用大量 mock 掩盖运行时问题。
- 涉及生命周期、并发、队列、节流、安全边界的测试，优先跑真实 runtime。
- 如确实需要 mock，只允许 mock 外部系统，不允许 mock 自己的核心边界。
- 涉及本地主脑、本地小模型、本地多模态、工具调用、背景压缩、上下文体积与时延的测试，默认以 `apps/panel -> apps/gateway -> brain -> provider/inference` 的真实链路为最终验收标准。
- 面板真实链路测试回答的是“系统能不能真实工作”，优先级高于任意底层 smoke、孤立 provider 调用或单 crate 推理测试。

## 7.4 模型测试口径

- 以后所有模型测试，默认分为两层：
  - 产品验收层：必须走面板聊天接口，经 `gateway` 接主脑与运行时主路径完成真实测试。
  - 底层自检层：允许使用 `smoke` / 最小推理测试快速判断模型、GPU、`llama.cpp`、`mmproj`、本地 provider 是否“通电”。
- `smoke` 的意义仅限于：
  - 快速确认模型能否加载
  - GPU 是否真正启用
  - 最小推理链是否活着
  - 在真实链路失败时帮助定位问题是在底层还是在运行时编排层
- `smoke` 不得被当成以下问题的完成证明：
  - 面板是否接通
  - gateway 路由是否正确
  - fast/full 通道是否正确
  - hardness 是否生效
  - 工具调用是否闭环
  - OCR-first / multimodal-fallback 是否按系统策略运行
  - trace / persistence / memory / background 是否完整
- 文档、提交说明与测试结论中，若结果仅来自 smoke，必须显式标注为：
  - `底层自检通过`
  - 不得伪装成 `系统真实链路通过`

---

## 8. 文档与对外承诺准则

- 文档中“已完成”必须等于：
  - 已实现
  - 已接线
  - 已测试
- “设计目标”与“已落地能力”必须分开写。
- 任何 README 中涉及运行时、治理、安全、进化的描述，都要能被代码路径和测试证明。
- 每次开发完成后，必须同步更新对应 crate 的 `README.md`。
- crate README 的更新不是可选收尾动作，而是交付的一部分；如果 crate 的行为、能力边界、依赖方式、主路径接线、限制条件或完成状态发生变化，README 必须同步反映。

---

## 9. 代码审查口径

代码评审至少检查以下问题：

- 生命周期是否清晰
- 后台任务是否可回收
- 安全边界是否显式
- 配置是否从入口显式传入
- 日志是否泄露秘密
- 队列是否会吞任务
- 子系统间契约是否一致
- 文档宣称是否与主路径一致

如以上任一问题答案不明确，则不得视为生产级完成。

---

## 10. BenShu 系统级重构计划

## 10.1 计划目标

这份重构计划的目标不是“把代码整理漂亮”，而是把 BenShu 从“主干很强但闭环未尽”的状态，推进到“主路径稳定、运行时清晰、治理显式、可评测、可追踪、可恢复”的 AgentOS 工程状态。

这份计划默认遵守本文件前九章的全部开发准则，尤其强调以下四件事：

- 主路径优先
- 生命周期优先
- 治理显式化
- 文档承诺必须可被代码与测试证明

---

## 11. 重构北极星

### 11.1 目标状态

BenShu 完成重构后的理想状态应满足：

1. 核心 Agent runtime 具备稳定且可解释的执行阶段
2. 关键能力全部接入主路径，而不是停留在孤立 crate 或备用实现
3. 长任务、多 Agent、工具执行、记忆写回、追踪、评测形成闭环
4. panel / gateway / kernel / brain 对外呈现一致语义
5. 安全、审批、审计、风险、工作区边界在任意异步链路中都能显式传递
6. 整个系统最终服务于“个人真正的贾维斯”这一产品目标，而不是为了局部 benchmark 或单项演示能力牺牲整体一致性

### 11.2 不做什么

本轮重构不是为了：

- 推倒 crate 版图重来
- 把系统重写成单体应用
- 追求抽象层数更多
- 用新框架替换现有主干
- 优先美化 UI 或增加演示能力
- 把研发重心绑定到某个单项专项能力竞赛上

本轮重构只做一件事：

> 把现有正确的方向收束成真正生产级主路径。

### 11.3 Windows 原生环境兼容约束

BenShu 的重构计划必须保持跨平台成立，同时把 `Windows 原生环境` 视为一等兼容目标之一，而不是“功能完成后再适配”的附属平台。

这意味着：

- 任何主路径设计都不得默认依赖 Unix 专属语义，例如 `/tmp`、`/bin/bash`、POSIX 权限位、符号链接必然可用、fork 模型、冒号分隔 PATH、`kill -9` 风格进程回收。
- 任何“线程工作区 / artifact / uploads / outputs”设计，必须先抽象为平台无关语义，再映射到 Windows、Linux、macOS 的具体路径实现。
- 任何 runtime/tooling 方案都必须把 Windows 原生工具链视为正式支持路径，包括：
  - Job Objects
  - PowerShell / cmd / MinGit Bash
  - 便携工具链
  - Windows 路径与句柄模型
- 不允许把“先在 Linux/macOS 跑通，再看 Windows”当作默认路线。
- 文档中任何“原生可用、零依赖、开箱即用”的说法，都必须在 Windows 原生环境上不冲突、可验证，同时保持对 Linux 与 macOS 的兼容。

因此，这份重构计划的适用性结论是：

> 方向上是跨平台适用的，但执行上必须显式加入 Windows 原生兼容约束，否则会在落地阶段悄悄滑回 Unix 默认假设。

### 11.4 GPU 优先测试约束

BenShu 当前与后续的开发测试链，应明确采用 `GPU 优先` 原则。

- 所有涉及以下主题的测试，默认必须优先在 GPU 路线上完成：
  - 本地主脑回复时延
  - 完整通道 / 快通道延迟
  - 长上下文 prefill 成本
  - 工具调用完整路径成本
  - 背景压缩 / 记忆召回对时延的影响
  - 本地模型栈 role-binding 的真实性能
- `CPU` 路径保留为：
  - fallback 验证
  - 故障诊断
  - 极端降级路径
  - 无 GPU 宿主下的最低可用性检查
- 文档与测试结论中，若结果来自 CPU 路径，必须显式标注，不得伪装成默认本地性能结论。
- `WSL / WSL2` 作为开发测试链可以继续存在，但若其承担性能结论验证职责，则必须优先接入可用 GPU，而不是默认走 CPU。

---

## 12. 当前问题定义

结合现状，BenShu 当前最关键的问题不是“没有能力”，而是“能力存在但未全部闭环”。

### 12.1 已经明确强的部分

- `brain` 的 runtime / session 治理
- `infra` 的 tool contract
- `brain` + `engram` 的 memory 基础设施
- `security` 的 prompt injection / sandbox 方向
- `multi_agent` / `fission` 的多 Agent 骨架
- `kernel` 的系统装配角色

### 12.2 当前最真实的缺口

- 缺少系统化 `eval / harness`
- `telemetry` 未形成强 trace 闭环
- `provider failover` 有实现但不是默认主路径
- `hooks` 有引擎但不是主控制面
- `state` 有持久化底座但长任务主路径不足
- `comm` / `multi-agent` 距离协议化完全体仍有差距
- 文件工作区、artifact、upload、output 尚未统一成系统语义

### 12.3 根本矛盾

根本矛盾可以归纳为一句话：

> BenShu 的系统分层已经接近完整，但主路径闭环、运行时收束和治理传递还没有达到同等完成度。

---

## 13. 重构总原则

### 13.1 保留 crate 版图，重做主路径

不重画大架构，不推翻现有 crate 分层。
重构重点放在：

- 主路径整合
- 阶段划分
- 生命周期
- 显式上下文
- 可观测与可评测

### 13.2 优先重构“系统连接处”

优先改：

- `brain <-> kernel`
- `brain <-> state`
- `brain <-> telemetry`
- `brain <-> security`
- `brain <-> builtin-tools`
- `comm <-> state <-> telemetry`
- `gateway/panel <-> kernel`

不要先去做大规模内部细节抛光。

### 13.3 所有新增能力必须以主路径接线为完成标准

任何能力只有满足以下条件才算完成：

- 在主工厂被装配
- 在主 API 路径或主 runtime 路径可达
- 有 trace
- 有测试
- 有失败语义
- 有回收语义

---

## 14. 目标运行时模型

重构后的 BenShu 应当建立清晰的 agent runtime stage pipeline。

该模型必须满足跨平台约束：

- stage 间传递的是平台无关语义对象，不是原始 OS 特定细节
- OS 差异只能在 `security`、`runtimes`、`infra`、`builtin-tools` 等平台边界层落地
- `brain` / `state` / `telemetry` / `kernel` 中不得出现依赖 Unix 默认行为的主路径假设
- 路径、进程、shell、退出码、文件锁、编码与权限的差异必须在边界层被统一

建议的统一阶段如下：

1. `Ingress`
   - 接收输入
   - 绑定 session / thread / request metadata
   - 生成 trace root

2. `Governance`
   - 风险策略
   - 审批策略
   - 工作区边界
   - 运行模式
   - agent passport / delegation policy

3. `Context Build`
   - system prompt
   - static prefix
   - context injectors
   - memory injection
   - artifact / upload / thread workspace context

4. `Reasoning`
   - provider 请求
   - model 选择 / failover
   - token accounting

5. `Tool Planning & Filtering`
   - tool registry
   - deferred tool discovery
   - per-turn tool shaping
   - loop detection

6. `Execution`
   - tool execution
   - sandbox / runtime
   - subagent / fission / background task

7. `Persistence & Memory`
   - session persistence
   - task state persistence
   - memory writeback
   - artifacts registration

8. `Trace & Audit`
   - transcript
   - tool trace
   - policy decisions
   - audit events

9. `Egress`
   - user-visible result
   - UI / channel events
   - status / progress updates

这九段不是为了形式化，而是为了彻底解决当前主路径分散的问题。

---

## 15. 重点工作流 1：Eval / Harness 体系补全

这是第一优先级。

### 15.1 目标

建立 BenShu 自己的评测与执行验证闭环，而不是仅依赖单元测试与偶发人工验证。

### 15.2 需要落地的能力

- `Task`：标准化任务定义
- `Trial`：单次运行记录
- `Transcript`：完整执行过程
- `Outcome`：最终环境结果
- `Grader`：评分器
- `Suite`：任务集
- `Regression`：回归测试入口

### 15.3 crate 建议落点

- `telemetry`：transcript / run trace schema
- `state`：trial / run persistence
- `brain`：runtime event emission
- `kernel`：eval harness 装配
- 可新增独立 `eval` crate，若新增则必须只承载评测，不承载业务逻辑

### 15.4 完成标准

- 至少 20 个真实主路径任务形成标准 task suite
- 每次重构可跑回归
- 结果能区分 transcript failure 与 outcome failure
- 引入 pass/fail 之外的 failure reason 分类

### 15.5 必须补齐的评测工件

`eval / harness` 不能只停留在 task + grader 的最小结构，还必须补齐可审计工件层。

必须新增以下对象：

- `Witness`：单次任务执行证据单元，至少包含
  - spec
  - 计划摘要
  - tool trace
  - diff 摘要
  - test log 摘要
  - policy decision
  - cost / latency / token 统计
- `Replay Unit`：可用于重放一次任务执行的最小描述对象
- `Scorecard`：按 suite / model / provider / runtime profile 聚合结果的标准对象
- `Benchmark Fingerprint`：把评测配置、模型版本、工具集、grader 配置固化成稳定指纹

### 15.6 评测形态要求

- 评测必须支持 `ablation`，至少能回答：
  - 有无 retrieval 的差异
  - 有无 memory 注入的差异
  - 有无 hooks/middleware 的差异
  - 有无 failover / degradation 的差异
- 评测结果必须能沉淀成长期回归基线，而不是一次性输出
- 评测运行必须显式记录：
  - runtime profile
  - provider 组合
  - tool shaping 策略
  - artifact / workspace 语义配置

### 15.7 Real Harness 与 Simulation Harness 必须分层

评测体系允许存在 simulation harness，但不得把 simulation 结果冒充主路径能力证明。

要求：

- `Simulation Harness`
  - 用于快速回归、参数扫描、故障注入、grader 调试
- `Real Harness`
  - 直接驱动真实 runtime、真实 provider 组合、真实工具路径或等价高保真替身
- 所有对外或对主线决策有影响的能力结论，必须至少有一轮 `Real Harness` 支撑
- 文档、报告、scorecard 中必须明确标注本次结果来自：
  - simulation
  - real
  - mixed
- 若 simulation 与 real 出现显著偏差，必须优先修复 harness 差异，不得直接修改 agent 主逻辑掩盖问题

---

## 16. 重点工作流 2：Tracing / Observability 闭环

这是第二优先级，且必须和 eval 同步推进。

### 16.1 目标

让 BenShu 的每次运行都具备完整追踪能力，而不是只有零散日志。

### 16.2 必须记录的对象

- request / thread / session / task / agent / parent-agent / child-agent ID
- system prompt 摘要与 context 构成
- model 请求与 provider 选择
- tool call 参数与结果摘要
- guardrail / approval / policy decision
- task state 变化
- memory writeback 结果
- user-facing output

### 16.3 结构要求

trace 必须是结构化对象，而不是只依赖 `tracing::info!`。

建议输出至少分三层：

- `Span`：生命周期边界
- `Event`：关键节点
- `Artifact`：大对象引用，如 transcript、tool payload、generated output

### 16.4 完成标准

- 能按 trace_id 完整回放一次运行
- 能回答“为什么这次工具没执行 / 为什么被拒绝 / 为什么降级”
- 多 Agent 运行可以串起 parent-child 关系

### 16.5 Witness 级追踪要求

除了普通 trace，还应建立 witness 级执行证据。

区别如下：

- `trace`：面向调试与运行可观测性
- `witness`：面向审计、回放、评测归档与能力证明

因此，主路径应支持：

- 每次任务结束后生成 witness 摘要对象
- witness 与 trace 通过 `trace_id / task_id / run_id` 互相引用
- 允许将 transcript、tool payload、大对象输出以 artifact 引用方式挂入 witness，而不是把大对象直接塞进日志
- 失败任务也必须产出 witness，至少包含 failure reason 与最后有效阶段

### 16.6 Trace 工件分层

建议将 trace 工件固定为三层：

- `Run Trace`
  - 一次完整运行的阶段流
- `Tool Trace`
  - 单次工具调用的输入、结果、失败与降级
- `Witness Summary`
  - 面向审计与评测的压缩执行证据

三层对象必须可独立存储，也必须能按主键重新串联。

### 16.7 Witness Log 与语义审计索引

除了常规 trace 与 witness summary，系统还应维护可搜索的 witness log。

该对象至少应支持记录：

- route / provider / model decision
- context artifact 引用
- tool path 与关键失败原因
- quality / latency / degradation 指标
- 关联 task_id / trace_id / run_id / suite_id

要求：

- witness log 必须支持后台批量写入与背压处理
- witness log 应能被结构化查询，而不是只做文本 grep
- 若未来引入语义索引，索引对象必须是 witness log 的派生产物，而不是替代原始 witness
- witness log 不得丢失关键治理字段，例如 policy decision、fallback reason、budget exhaustion

---

## 17. 重点工作流 3：Provider 与 Model 主路径收束

### 17.1 目标

把 provider failover、model capability、context window、cache control 从“分散能力”收束成 runtime 主路径能力。

### 17.2 重点动作

- 默认优先复用同一套 `runtime / context / governance / tracing` 机制，同时适配本地模型与云 provider；只有当 capability 差异无法在统一 contract 中表达时，才允许在 provider adapter 层分叉
- 让 `ResilientProvider` 成为正式可配置主路径
- 显式区分：
  - primary model
  - fallback model
  - local model
  - tactical/cheap model
- 让 `ContextManager`、provider metadata、tool shaping 共用同一个 capability 视图

### 17.3 完成标准

- provider 切换可追踪
- fallback 可测试
- context budgeting 不依赖猜测
- thinking / vision / tool / long-context 能力有统一声明

---

## 18. 重点工作流 4：Hooks / Middleware / Stage Control 面升级

### 18.1 目标

把 `HookEngine` 从“存在的基础设施”升级成 runtime 主控制面的一部分。

### 18.2 重点动作

- 将 hook timing 与 runtime stage 对齐
- 明确 pre-context / pre-provider / pre-tool / post-tool / post-response / post-persist 等阶段
- 把当前散落在 core 中的部分 cross-cutting 逻辑迁入 stage/hook

### 18.3 适合优先 hook 化的能力

- clarification gating
- loop detection
- upload / artifact context 注入
- tool error degradation
- memory writeback 过滤
- trace injection
- post-run evaluation tap

### 18.4 完成标准

- hook 在主路径真实生效
- 无 hook 时零成本
- hook 修改/拒绝行为可被 trace 和测试证明

### 18.5 Hooks 外部控制面

除了 runtime 内部 hook engine，还应建设外部控制面能力。

目标不是复制内部逻辑，而是让以下流程可被显式安装、配置、审计：

- session start / restore
- pre-tool / post-tool
- pre-compact
- user prompt submit
- notification / stop

这层能力必须满足：

- 可以独立配置
- 可以按平台选择脚本入口
- 可以注入有限上下文而不是任意篡改主流程
- 可以挂接 memory recall、route recommendation、post-run evaluation tap

### 18.6 Hook 数据后端

如果 hook 开始承担学习与推荐职责，就不能只靠临时内存。

建议为 hook 控制面提供可替换的数据后端，至少支持：

- pattern store
- memory recall store
- error pattern store
- file / tool sequence store
- session stats store

该后端可以落在现有持久层上，但接口必须独立，不得把 hook 逻辑直接散落进 `brain` 核心路径。

### 18.7 Hook 设计边界

- hook 可以建议，不应默认拥有无限决策权
- hook 可以拒绝高风险操作，但拒绝必须产出结构化原因
- hook 可以修改上下文注入内容，但修改必须可 trace
- hook 的学习结果不得绕过主治理链路直接影响高风险写操作

---

## 19. 重点工作流 5：Task State 与长任务主路径化

### 19.1 目标

让 `TaskState` 从“持久化能力”升级为“长任务一等控制对象”。

### 19.2 重点动作

- 明确 thread/session 与 task 的关系
- 每个长任务必须有显式 task_id
- 建立：
  - `pending`
  - `running`
  - `blocked`
  - `awaiting_approval`
  - `completed`
  - `failed`
  - `cancelled`
- 将 task 进度与 panel / gateway / telemetry 打通

### 19.3 禁止事项

- 禁止长任务只存在于内存消息里
- 禁止 UI 自己维护一套假的 task 状态
- 禁止子任务结束后不回写主 task state

### 19.4 完成标准

- 长任务中断后可恢复
- 可区分 session 状态与 task 状态
- task 的进展、阻塞原因、恢复点可观测

---

## 20. 重点工作流 6：Multi-Agent 与 Comm 协议化收束

### 20.1 目标

保持 BenShu 的多 Agent 上限，同时把它从“骨架存在”推进到“协议化、可追踪、可恢复”的系统能力。

### 20.2 重点动作

- 在 `comm` 中标准化消息 envelope
- 标准化：
  - message_id
  - parent_task_id
  - owner
  - role
  - target
  - trace_id
  - causality metadata
- 建立 task ownership 与 agent ownership 的一致规则
- 明确 subagent / fission / handover / delegation 的状态机

### 20.3 非目标

- 不追求一开始就做分布式集群
- 不追求先做复杂拓扑
- 先把单机多 Agent 语义做扎实

### 20.4 完成标准

- 每次 delegation 都可追踪
- 子 Agent 失败不会无声吞掉
- 所有交接都能落到 task state 和 trace 上

---

## 21. 重点工作流 7：Tooling、Workspace 与 Artifact 统一语义

### 21.1 目标

把工具执行、上传文件、工作空间、输出文件、生成物注册统一为系统语义，而不是每个 tool 各玩各的。

### 21.2 统一模型

建议建立 thread-scoped workspace 语义：

- `uploads`
- `workspace`
- `outputs`
- `artifacts`

这些语义应同时被以下层使用：

- `builtin-tools`
- `brain`
- `state`
- `gateway`
- `panel`
- `telemetry`

这些语义必须满足 Windows 原生要求：

- 不把 `/mnt/...`、`/tmp/...`、`~/.cache/...` 之类路径写死进核心模型
- 不默认依赖 POSIX 路径分隔符与大小写敏感行为
- 不默认依赖符号链接、硬链接、Unix 权限位和 inode 语义
- 必须考虑 Windows 文件锁、路径长度、驱动器前缀、PowerShell/cmd 与 bash 差异
- 用户可见路径与内部实际路径应允许分离，必要时通过虚拟路径语义统一展示

### 21.3 重点动作

- 把 artifact 注册从工具内部散点实现，收束成统一服务
- 上传文件和临时文件不自动进入长期记忆
- 生成物必须能追踪来源 task / tool / agent
- 工具失败后的残留文件要有回收策略

### 21.4 完成标准

- 用户可见文件语义一致
- tool 输出不再各自定义路径规则
- artifact 有统一索引与清理策略

### 21.5 Retrieval Safety Net 与退化报告

凡是带检索、召回、候选筛选的系统路径，都必须补一层 runtime 级 safety net。

必须支持：

- 当主召回路径候选不足时，触发受预算约束的补扫
- 补扫至少支持：
  - 邻域扩展
  - 热缓存/近期对象窗口
  - 附加候选重排
- 补扫不能无限放大资源消耗，必须受下列预算约束：
  - scan time
  - candidates scanned
  - distance ops / rerank ops

### 21.6 Degradation Report

任何 safety net、fallback、budget exhaustion 都必须产出结构化退化报告。

退化报告至少包含：

- fallback path
- degradation reason
- scanned / total
- budget type
- guarantee lost

这份报告必须同时进入：

- trace
- witness
- eval outcome

### 21.7 查询与检索路径的 DoS Hardening

检索主路径必须显式防御恶意高成本查询。

建议补齐以下机制：

- token bucket / budget bucket
- degenerate query negative cache
- 可选 proof-of-work 或等价计算挑战
- query signature 与异常模式识别

要求：

- 这类保护不得只停留在网关层
- runtime 内部也必须知道“为什么这次 safety net 被禁用 / 为什么预算被锁死”

---

## 22. 重点工作流 8：Gateway / Panel 与核心契约统一

### 22.1 目标

让 `apps/gateway` 与 `apps/panel` 不再只是系统外壳，而成为主路径契约的真实载体。

### 22.2 重点动作

- 所有 agent/task/artifact/memory/approval 状态以统一 DTO 对外暴露
- panel 不再自行推导核心状态
- gateway 不再容忍“路由有了但后端语义不完整”
- 运行模式显式化：`Embedded` / `Standalone` / future modes

### 22.3 完成标准

- panel 看到的任务状态与 brain/state 一致
- 不再出现“前端以为有这个能力，后端没真正接线”的漂移

---

## 23. 分阶段实施路线

## Phase 1：运行时收束期

目标：

- 建立 runtime stage 模型
- 补 trace schema
- 接入基础 hook/middleware

完成标志：

- 关键路径可 trace
- clarifcation / loop detection / tool degradation 可 stage 化
- 至少有一条 Windows 原生运行路径被纳入持续验证

## Phase 2：闭环补全期

目标：

- 补 eval/harness
- 让 provider failover、task state、artifact 进入主路径

完成标志：

- 有 task suite
- 有 transcript/outcome 评分
- 长任务可以恢复
- Windows 原生环境下至少覆盖一条 tool + workspace + artifact + task 状态主路径

## Phase 3：协议化强化期

目标：

- 收束 multi-agent / comm / ownership / handover
- 打通 panel/gateway 的 task 与 trace 可视化

完成标志：

- delegation 端到端可追踪
- parent-child task 图可视化

## Phase 4：一致性清算期

目标：

- 清理旁路实现
- 清理重复语义
- 清理未接线但被宣称的能力

完成标志：

- 文档承诺与主路径一致
- 非主路径残留能力显式标注实验性

---

## 24. crate 级工作分配建议

### `brain`

- runtime stage pipeline
- hook 接入
- provider failover 主路径化
- tool planning / loop detection / clarification gate

### `telemetry`

- trace schema
- transcript storage
- event bus binding
- evaluation tap

### `state`

- task state 主路径化
- thread/task/session 关系建模
- artifact/task 映射

### `kernel`

- 全局装配收束
- 主路径依赖注入
- runtime mode 显式传递

### `comm`

- message envelope
- causality metadata
- ownership / handover semantics

### `builtin-tools`

- workspace / artifact 统一语义接入
- tool degradation 标准化
- 大 schema 工具按需发现

### `security`

- governance context 接线强化
- policy decision trace
- 审计事件结构化
- Windows Job Objects / 路径边界 / 进程回收语义收束为正式主路径能力
- action permission / deny / defer 语义进入正式治理链路
- authority / budget / coherence 违规都必须能结构化落盘

### `runtimes`

- Windows 原生 runtime/toolchain 作为正式一等支持路径
- 统一 PowerShell / cmd / MinGit Bash / portable toolchain 的能力抽象
- 消除核心路径中对 Unix shell 语义的隐式依赖
- 将环境探测、下载、恢复、失败回退接入 trace 与 task state

### `apps/gateway`

- 标准 DTO
- artifact / task / approval / trace / replay / witness API

### `apps/panel`

- 主路径状态展示
- task progress
- artifact/trace/replay/witness/approval 可视化

---

## 25. 验收标准

重构完成不是靠“感觉更清晰”，而是靠验收。

以下条件全部满足，才可认为本轮重构达标：

1. 至少 3 条主路径端到端可回放
2. 至少 20 个标准任务进入 eval suite
3. provider failover 可被自动化测试证明
4. 长任务可恢复可取消
5. 子 Agent delegation 可追踪
6. uploads / workspace / outputs / artifacts 语义统一
7. panel / gateway / brain / state 对 task 状态达成一致
8. 文档中所有“已完成”能力都能找到主路径代码与测试
9. 至少有一条 Windows 原生端到端主路径通过自动化验证
10. 不存在依赖 Unix 默认路径、shell 或权限语义才能工作的核心主路径
11. 至少一条运行可以产出 trace + witness + outcome 三联工件
12. 至少一类检索路径具备 safety net 与 degradation report
13. 至少一类高风险操作走通 permit / defer / deny 治理链路
14. hooks 的外部控制面至少覆盖 session、tool、compact 三类事件

---

## 27. 运行时治理补充要求

### 27.1 Authority / Budget / Coherence 三轴模型

后续所有治理能力，建议收束为三条独立但可组合的运行时轴：

- `Authority`
  - 当前运行最多能做什么
- `Budget`
  - 当前运行最多能消耗多少资源
- `Coherence`
  - 当前系统是否处于允许继续自主推进的稳定状态

这三条轴不得混写成一团。

### 27.2 Authority

建议显式定义至少四级 authority：

- `ReadOnly`
- `WriteMemory`
- `ExecuteTools`
- `WriteExternal`

要求：

- 所有 tool / runtime action 都必须能映射到 authority requirement
- authority 检查结果必须进入 policy decision trace
- 子 Agent 继承 authority 时必须支持降级，默认不得升级

### 27.3 Budget

建议对以下资源建立统一 budget tracker：

- wall clock time
- tokens
- cost
- tool calls
- external writes

要求：

- budget 超限必须返回结构化错误，而不是隐式失败
- budget exhaustion 必须能进入 witness / eval / trace
- budget 应支持 profile 化，而不是全局只有一套固定阈值

### 27.4 Coherence

建议把下面几类信号收束成统一 coherence monitor：

- contradiction rate
- rollback ratio
- runtime health score
- degradation frequency

并据此形成最少四种状态：

- `Healthy`
- `SkillFreeze`
- `RepairMode`
- `Halted`

这层状态的作用不是“看起来高级”，而是统一回答：

- 现在还能不能继续自动推进
- 还能不能写 memory
- 还能不能提升 skill / policy
- 是否必须切到人工审批

### 27.5 Action Permission Service

高风险写操作不应只靠散落的 policy if/else 决定。

建议建设独立的 action permission 服务层，输出固定三态：

- `Permit`
- `Defer`
- `Deny`

并要求：

- `Defer` 必须带 escalation 原因
- `Deny` 必须带 policy reason
- 每次 permission decision 都可回放、可审计、可关联到 trace/witness/task
- 每次 decision 都应产生稳定 receipt，可供后续查询
- permission 服务至少支持：
  - receipt retrieval
  - decision replay
  - policy basis lookup
- 如引入链式完整性校验，必须作为增强能力，不得阻塞最小可用主路径

### 27.6 Profiler 与可复现性

任何 benchmark、eval、ablation、性能结论，如果不能复现，就不能进入长期工程决策。

因此建议新增 profiler 级约束：

- benchmark config 必须有稳定 fingerprint
- latency / memory / energy 或其等价资源指标必须可独立导出
- 输出格式必须稳定，方便跨机对比与回归
- 性能工件必须能与 run_id / trace_id / eval suite 关联

---

## 附录 A. 基于当前仓库的重构北极星审查意见

这一节不是重新写计划，而是对照当前仓库代码，判断：

- 这项能力现在是否已经存在
- 它是主路径能力，还是只是地基
- 它下一步是“保留”，还是“必须补主路径”

以下判断以当前仓库源码为依据，而不是以 README 或愿景描述为依据。

### A.1 Eval / Harness

我的判断：

- `应该继续做，但已不再是“主路径缺失”`
- 当前仓库已经形成第一版 `eval / harness / witness / scorecard` 主路径，后续重点应转向质量、覆盖面与长期产品化增强

代码依据：

- `telemetry` 现在已经形成 `RunTrace / ToolTrace / WitnessSummary / scorecard / profiler artifact` 主路径
- `TelemetryManager::capture_evaluation_tap(...)` 已把 `post-run evaluation tap` 收敛成统一入口，不再要求 gateway 手工拼 witness/trace
- gateway / panel 已有 `trace / witness / profiler` 第一版读面；execution plan 与 runtime 归档文档也已把这条主线标记为完成

结论：

- 这部分现在已经不是“真实缺失的主干”
- 后续仍值得继续加强，但应理解为：
  - 提高 suite 覆盖面
  - 提高 replay / witness 质量
  - 提高跨环境可复现性
  - 而不是继续按“主路径尚未建立”来判断优先级

### A.2 Tracing / Observability

我的判断：

- `主路径已形成，后续仍需继续增强`
- 当前已不再是“轻量日志 + 点状 trace”阶段，而是已经具备结构化运行证据系统的第一版主路径

代码依据：

- `crates/telemetry/src/trace.rs`
  - 现在已经有 `RuntimeStage / ToolTrace / RunTrace / WitnessSummary`
  - 旧的 `TraceResult / AgentTracer::record()` 兼容路径也已移除
- `crates/brain/src/agent/session.rs`
  - session 状态机是扎实的
  - 同时 trace / witness / stage metadata 也已经独立成链，不再只依赖 session 状态

结论：

- `16. Tracing / Observability` 仍应保持高优先级
- 但现在的重点应是：
  - witness log 的长期产品化
  - artifact 引用与查询体验继续增强
  - 更大规模 replay / audit / eval 体系收口
  - 而不是继续按“主干尚未存在”来判断

### A.3 Provider 与 Model 主路径收束

我的判断：

- `部分有，而且地基不错`
- 但还没有证据表明它已经完全成为默认主路径

代码依据：

- `crates/brain/src/agent/provider/resilient.rs`
  - `ResilientProvider` 已实现 circuit breaker、timeout、fallback provider
- `crates/brain/src/agent/context.rs`
  - `ContextManager` 已经在做 context budget、history pruning、injector、local vs remote 分流

结论：

- 这块不是从零开始
- 正确方向不是“重写 provider 层”
- 而是把已有能力接成统一 capability 视图，并让 failover 成为可测试、可追踪的正式主路径

### A.4 Hooks / Middleware / Stage Control

我的判断：

- `部分有，但现在更像基础设施，不像主控制面`

代码依据：

- `crates/brain/src/hooks/engine.rs`
  - `HookEngine` 已经支持 register / fire / modify / abort / skip
  - 空 hook 路径也是零成本返回
- 但目前还看不出它已经和 `runtime stage pipeline` 完全对齐
- 也还没有看到外部 hooks 控制面已经成为产品化能力

结论：

- `18. Hooks / Middleware / Stage Control` 不是空想
- 但当前确实还没完成“从底层引擎升级成主路径控制面”这一步
- 所以这一章应该做，而且要避免只停留在“hook engine 已经存在”的自我安慰

### A.5 Task State 与长任务主路径化

我的判断：

- `有 durable task state`
- `但还没有完整长任务主路径闭环`

代码依据：

- `crates/state/src/task.rs`
  - 已经有 `TaskStatus`、`TaskState`、`TaskManager`
  - 支持 `save / load / list`
- 但当前状态模型仍偏简：
  - 有 `Pending / Running / Completed / Failed / Cancelled / Paused`
  - 还没有计划里强调的 `blocked / awaiting_approval`
- 也还看不出 task 与 panel/gateway/telemetry 已经端到端打通

结论：

- 这一块不是缺存储
- 是缺“task 真正成为 runtime 一等控制对象”
- 因此 `19. Task State 与长任务主路径化` 是应该做的，而且不该再只停留在 state crate 里

### A.6 Multi-Agent 与 Comm 协议化

我的判断：

- `已有，而且这是 BenShu 的强项之一`
- 但仍然值得做“协议收口”，不是因为没有，而是因为已经值得标准化

代码依据：

- `crates/brain/src/agent/multi_agent.rs`
  - 已有 `Coordinator`
  - 有 session persistence、scheduler、approval handler、memory 链接
- `crates/brain/src/agent/fission.rs`
  - 已有递归深度限制、capability stripping、governance context 继承、token budget 继承
- `crates/comm/src/protocol/a2a.rs`
  - 已有 `A2AMessage`、`DelegationEnvelope`、ownership 相关字段

结论：

- `20. Multi-Agent 与 Comm 协议化收束` 应该做
- 但不是因为 BenShu 没有多 Agent
- 而是因为 BenShu 已经有多 Agent 主干，正适合进入“message envelope / ownership / causality / task graph”收口阶段

### A.7 Tooling / Workspace / Artifact

我的判断：

- `workspace 很强`
- `artifact 统一语义明显还没有`

代码依据：

- `crates/builtin-tools/src/tool/filesystem.rs`
  - 已有严格 workspace path validation
  - 还兼容动态 trusted workspaces
- `crates/security/src/sandbox.rs`
  - 已经明确处理 Windows 路径边界与 workspace 外逃逸问题
- `apps/gateway/src/api/handlers/workspace.rs` 与 `apps/panel/src/api.rs`
  - workspace 已有前后端管理接口
- 但用 `rg "artifact"` 看 `brain / state / gateway / panel`
  - 目前几乎没有统一 artifact 服务主路径

结论：

- `21. Tooling / Workspace / Artifact` 里，workspace 语义属于“已有基础、要继续保留”
- artifact 注册、生命周期、索引、清理策略属于“确实应该补”

### A.8 Retrieval Safety Net / Degradation / DoS Hardening

我的判断：

- `应该做`
- 目前仓库里有 retrieval 与 retrieval telemetry，但还不是 safety-net runtime

代码依据：

- `crates/engram/src/vector_store.rs`
  - 已有 `last_execution_mode`、latency sample、snapshot manifest 等治理基础
- `crates/knowledge/src/retrieval/tiered.rs`
  - 已有分层检索入口
- 但没有看到：
  - 主召回不足时的受预算补扫
  - `degradation report`
  - 查询级 `negative cache / token bucket / proof-of-work`

结论：

- `21.5 - 21.7` 不是过度设计
- 而是当前检索链路确实还没有的高可靠层

### A.9 Authority / Budget / Coherence / Permission

我的判断：

- `部分有 authority / budget 散点`
- `统一三轴治理模型还没有`

代码依据：

- `crates/brain/src/approval/policy.rs`
  - 已有 `AutoApprove / Deny / RequireConfirmation`
- `crates/brain/src/agent/builder.rs`
  - 已有 token budget、trusted workspace、governance context 继承
- `crates/brain/src/agent/fission.rs`
  - 已有子 agent authority / budget 继承与 capability stripping
- 但当前没有看到：
  - 独立 `Action Permission Service`
  - 稳定 `Permit / Defer / Deny`
  - decision receipt / replay
  - 统一 `CoherenceMonitor`

结论：

- 计划里这部分应该做
- 但要注意不要误判成“我们完全没有治理”
- BenShu 现在的问题是治理语义分散，而不是没有治理

### A.10 Windows 不冲突的跨平台约束

我的判断：

- `这部分必须保留，而且已经有真实代码基础`
- 不是文档虚构出来的要求

代码依据：

- `crates/security/src/sandbox.rs`
  - 明确有 Windows path boundary check
- `crates/builtin-tools/src/tool/filesystem.rs`
  - 路径校验对 Windows 绝对路径有显式处理
- `apps/gateway` / `apps/panel`
  - 已经有 trusted workspace 管理链路

结论：

- “跨平台成立，Windows 原生不冲突” 这一条完全应该留在北极星里
- 它不是额外负担，而是 BenShu 现有工程优势之一

### A.11 总体判断

对照当前仓库代码，我的最终看法是：

- `应该保留的`
  - Eval / Harness 第一优先级
  - Trace / Witness / Witness Log
  - Hook 控制面升级
  - Task 主路径化
  - Retrieval safety net
  - Authority / Budget / Coherence
  - Artifact 统一语义
- `已经有地基，不应推倒重写的`
  - Provider failover 地基
  - Context management
  - Multi-agent / comm 主干
  - Workspace 边界与 Windows 跨平台约束
  - Approval / governance 的部分现有机制
- `不应该做错方向的`
  - 不要因为缺 witness 就重写 session
  - 不要因为缺 eval 就推翻 telemetry 现有基础
  - 不要因为要做 protocolization 就否定当前多 Agent 主干
  - 不要为了 artifact 统一语义去破坏已有 workspace 安全边界

一句话收束：

> 这份“重构北极星”计划与当前 BenShu 仓库是相容的。它不是脱离代码现实的理想图，而是“在已有强主干上补主路径闭环”的正确方向。

---

## 26. 最终判断

这份重构计划的核心立场是：

> BenShu 不需要推倒重来，也不需要重新发明架构；它真正需要的是，把已经正确的系统版图，收束成真正生产级闭环。

所以这不是一份“技术债清单”，而是一份系统升级计划。

如果执行得当，重构完成后的 BenShu 应当具备下面三个特征：

- 从“功能很多”走向“主路径稳定”
- 从“能力存在”走向“能力可证明”
- 从“AgentOS 雏形”走向“生产级 AgentOS”

---

## 10. 例外机制

- 允许例外，但不允许无记录例外。
- 所有偏离本规范的实现必须至少说明：
  - 为什么必须偏离
  - 风险是什么
  - 何时回收该例外
  - 如何测试该例外不会扩大伤害面

---

## 11. 最终原则

BenShu 不是一个单体脚本集合，而是一个带有长期运行、跨模块协作、动态派生、安全执行和持续演化能力的 AgentOS。

因此，本仓库的开发准则必须围绕 5 个核心问题持续自检：

1. 这段代码是否有明确边界，而不是靠隐式状态碰巧工作？
2. 这段代码是否可回收、可关闭、可取消？
3. 这段代码是否会在压力、安全或异常情况下静默失真？
4. 这段代码是否会让文档承诺与真实主路径脱节？
5. 这段代码是否能在半年后仍被别人安全维护？

如果答案不能明确为“是”，就不应把它当作最终形态合入主线。

---

*文档更新日志: 2026-03-22 - 新建面向全 BenShu Workspace 的开发准则文档，覆盖架构分层、生命周期、治理、安全、并发、测试与文档承诺标准。*
