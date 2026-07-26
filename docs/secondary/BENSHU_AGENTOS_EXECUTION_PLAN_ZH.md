# BenShu AgentOS 重构执行计划

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 测试链口径: 执行蓝图中的性能、时延与主路径体验验收默认按 `GPU 优先测试链` 解释；`CPU` 测试只用于 fallback/诊断，不直接代表 BenShu 本地主路径体验。

> 关联主文档: `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`
>
> 关联补充文档: `docs/secondary/BENSHU_HARDNESS_DESIGN_PRINCIPLES.md`
>
> 关联 tracing 契约: `docs/secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md`
>
> 文档定位: 这是“如何执行”的文档，不是“为什么这样设计”的文档。
>
> 使用方式: 所有重构任务、阶段排期、crate 分工、验收推进，默认以本文件为执行蓝本，以主标准文档为最终约束来源。

---

## 0. 文档目标

这份文档用于把 `DEVELOPMENT_STANDARDS_AGENTOS.md` 里的重构北极星，拆成真正可以推进的开发步骤、待完成清单和执行顺序。

这份文档回答 4 个问题：

1. 先做什么，后做什么。
2. 哪些能力是“必须补闭环”，哪些能力是“已有地基，先收束接线”。
3. 每个阶段具体改哪些 crate。
4. 什么叫做“这一阶段真的完成了”。

与 memory 专项计划的关系:

- 原 memory completion plan 已完成阶段性使命并移出 brain 主目录
- 当前 memory 系统主说明以 `crates/memory-core/README.md` 为准
- 本文件始终是上位执行计划，决定总顺序、总闸门、总排期
- memory 后续增强不得绕开本文件单独改写总体优先级
- memory 后续增强应作为本文件相关阶段内的细化施工单执行

本文件默认适用于：

- `crates/*`
- `apps/gateway`
- `apps/panel`

并默认遵守：

- 跨平台成立
- Windows 原生环境不冲突
- 不以推倒重写为目标
- 一切以主路径闭环为完成标准

---

## 1. 执行总原则

### 1.1 先补主路径闭环，再做能力扩展

执行顺序必须优先：

- trace / witness / task / governance 这类主路径支撑
- 再做 hooks 外部控制面、artifact 统一语义、retrieval safety net
- 最后做更高层的 protocolization、panel 深可视化、profiler 独立化

禁止倒序：

- 不能先做漂亮 UI 再补 task 主路径
- 不能先做复杂多 Agent 协议再补 trace
- 不能先堆工具再补 artifact 生命周期

### 1.2 优先改“连接处”，不优先改“内部小细节”

最优先的连接处：

- `brain <-> telemetry`
- `brain <-> state`
- `brain <-> security`
- `brain <-> builtin-tools`
- `kernel <-> brain`
- `gateway/panel <-> kernel`
- `comm <-> state`

### 1.3 每一阶段都必须留可回退点

每一阶段结束前必须保证：

- 编译通过
- 现有主路径不被破坏
- 新增 schema 有版本字段或兼容策略
- 不通过删除旧实现来掩盖新实现未完成

### 1.4 统一“完成”的定义

一项工作只有同时满足下面条件，才算完成：

- 有主路径接线
- 有错误语义
- 有 trace
- 有测试
- 有最小文档
- 有回收/清理语义

### 1.5 Hardness 作为主线约束，不作为独立岔路

`hardness` 不单独形成一条脱离主线的新排期，而是作为执行计划中的统一约束注入各阶段。

具体原则见:

- `docs/secondary/BENSHU_HARDNESS_DESIGN_PRINCIPLES.md`

执行含义:

- `P1` 负责把 hardness 的 `trace / truth / replay` 地基补齐
- `P2` 负责把 hardness 的 `authority / budget / governance decision` 正式化
- `P3-P5` 负责把 hardness 延伸到 witness、artifact、delegation 与协议边界
- `P6-P7` 负责把 hardness 延伸到 reproducibility 与 recovery

---

## 2. 总体执行顺序

建议按下面顺序推进：

1. `P0 基线冻结与盘点`
2. `P1 Runtime Stage / Trace / Task 主路径收束`
3. `P2 Governance / Provider / Hook 内核升级`
4. `P3 Eval / Harness / Witness 闭环`
5. `P4 Workspace / Artifact / Retrieval Hardening`
6. `P5 Multi-Agent / Gateway / Panel 协议收束`
7. `P6 Profiler / Reproducibility / 清算期`
8. `P7 Sealed Memory Backup / Restore 收尾期`

这个顺序的原因是：

- 没有 trace/task，后面所有能力都很难验证
- 没有 governance/provider/hook 内核，eval/harness 会测到一套不稳定主路径
- 没有 artifact / retrieval hardening，工具层会持续漂移
- 没有 protocolization，multi-agent 和应用层状态会长期不一致
- 记忆备份属于恢复保障层，必须建立在前面 memory/task/trace/governance 已收束的前提上，不能提前打断主线

memory 专项计划挂载规则:

- memory `P1-P4` 第一阶段已完成，不再作为当前主线阻塞项；后续只在 `P2-P4` 相关阶段按需继续补增强项
- memory `P5 Multimodal Memory Writeback` 第一阶段已完成，后续 provider/tool 自动接线与治理细化，应继续挂载在主线的 retrieval / artifact / sensory 阶段推进
- memory `P6 Sealed Memory Backup / Restore` 第一阶段后端能力已落地，但 system-side 用户闭环、restore policy 与 UI 收尾仍严格服从本文件 `P7`
- 因此从当前阶段开始，memory 专项计划默认转入“伴随式维护”，不再单独改写总体优先级

---

## 3. P0 基线冻结与盘点

状态: `第一轮已完成，后续滚动更新`

### 3.1 目标

为后续重构建立不会漂移的基线。

### 3.2 需要完成的事

- 冻结当前主路径清单
- 冻结当前对外承诺能力清单
- 建立现有 runtime 主路径图
- 记录当前已存在地基能力
- 标出“已实现但未主路径化”的能力

### 3.3 具体步骤

1. 整理当前主路径入口：
   - `apps/gateway`
   - `apps/panel`
   - `kernel` 装配入口
2. 形成当前主路径矩阵：
   - session
   - provider
   - tool execution
   - approval
   - multi-agent delegation
   - workspace access
3. 标记现有地基能力：
   - `ResilientProvider`
   - `HookEngine`
   - `TaskState`
   - `A2A / delegation envelope`
   - `workspace sandbox`
4. 建立“非主路径能力登记表”：
   - 有代码
   - 但未默认接线
   - 不能对外标记完成
5. 生成第一版重构任务看板

### 3.4 待完成清单

- [x] 当前主路径入口清单完成
- [x] 当前主路径状态流完成
- [x] 当前已存在地基能力列表完成
- [x] 当前未主路径化能力列表完成
- [x] 当前对外承诺与真实实现的差距列表完成

### 3.5 涉及 crate

- `brain`
- `kernel`
- `telemetry`
- `state`
- `security`
- `builtin-tools`
- `comm`
- `apps/gateway`
- `apps/panel`

### 3.6 阶段完成标准

- 后续每一项重构都能明确挂到某个已盘点主路径上
- 不再出现“感觉这里应该有一个能力”这种模糊推进方式

---

## 4. P1 Runtime Stage / Trace / Task 主路径收束

### 4.1 目标

把当前分散的运行时逻辑，先收束成可追踪、可持久化、可恢复的主路径。

### 4.2 这一阶段必须先做的原因

如果不先做：

- 之后的 hook 只会接到模糊阶段
- eval/harness 没有稳定 transcript / outcome 锚点
- panel/gateway 无法看到真实 task 状态
- 多 Agent 的交接无法被稳定回放

### 4.3 具体步骤

1. 固化 runtime stage schema：
   - `Ingress`
   - `Governance`
   - `Context Build`
   - `Reasoning`
   - `Tool Planning & Filtering`
   - `Execution`
   - `Persistence & Memory`
   - `Trace & Audit`
   - `Egress`
2. 在 `brain` 中补统一 stage event emission
3. 在 `telemetry` 中定义：
   - `Run Trace`
   - `Tool Trace`
   - `Artifact Ref`
4. 在 `state` 中补：
   - `run_id`
   - `task_id`
   - `thread_id`
   - `session_id`
   的稳定关联
5. 扩展 `TaskState` 状态模型：
   - `blocked`
   - `awaiting_approval`
   - `failed(reason)`
   - `cancelled`
6. 打通 session 与 task 的关系：
   - 一个 session 可关联零或一个主 task
   - 子 agent / delegation 可产生子 task
7. 打通 gateway/panel 的 task 读取路径

### 4.4 待完成清单

- [x] runtime stage 枚举与事件 schema 定义完成
- [x] trace 主对象落地到 `telemetry`
- [x] `TaskState` 扩展完成
- [x] session/task/run/thread ID 关系稳定
- [x] gateway 有 task 查询接口
- [x] gateway 有 trace 查询接口
- [x] panel 能显示真实 task 状态
- [x] panel 有只读 trace 展示
- [x] 至少一条主路径可按 trace_id 回放

当前已审过并可确认的最小完成项：

- `telemetry` 已具备 `RunTrace / ToolTrace / ArtifactRef / WitnessSummary` 结构化主对象
- `state` 已具备扩展后的 `TaskState` 与 `list_by_session(session_id)` 查询能力
- `brain` 单 agent 前台主路径会产出 `runtime_task + run_trace`
- `telemetry` 已具备最小 `save/get/list_session` run trace 注册能力
- `brain` 已具备正式 `RuntimeStage` 枚举、stage event emission 与 stage-based `RunTrace` contract
- `state::TaskState` 与 `RunTrace` 已稳定关联 `session_id / thread_id / run_id / trace_id / task_id`
- `gateway` 会持久化 `runtime_task` 和 `run_trace`，`/api/chat` 返回 `task_id / run_id / trace_id`，并提供 `/api/sessions/{id}/tasks`、`/api/traces/{id}` 与 `/api/traces/{id}/replay`
- `gateway` 现已将 delegation 记录投影为子 `TaskState`，并稳定保留 `parent_task_id / root_task_id` 关系，session 任务视图能够看到多 agent delegation 的父子链
- `panel` 已能从当前会话任务列表里手动加载并只读展示 `RunTrace` 摘要

### 4.5 涉及 crate

- `brain`
- `telemetry`
- `state`
- `kernel`
- `apps/gateway`
- `apps/panel`

### 4.6 本阶段禁止事项

- 不要在这阶段急着做 witness bundle 全量功能
- 不要先做 UI 美化
- 不要先做新的 tool family

### 4.7 阶段完成标准

- 至少 1 条单 agent 主路径具备完整 `trace + task`
- 至少 1 条多 agent delegation 具备父子关联
- Windows 原生至少有 1 条主路径通过

当前状态补充：

- 三条阶段标准现已全部满足
- Windows CI lane 已真实跑通 `P1 Windows Runtime Smoke`，期间顺带清除了 `protoc`、DXGI、sandbox handle 与默认 TensorRT 链接等 Windows 兼容缺口

---

## 5. P2 Governance / Provider / Hook 内核升级

### 5.1 目标

把现有散点治理能力收束成 runtime 核心控制层。

这是 `secondary/BENSHU_HARDNESS_DESIGN_PRINCIPLES.md` 中 `Authority Axis`、`Budget Axis` 与 `Truth Axis` 的首个正式落地点。

### 5.2 具体步骤

1. 收束 authority 语义：
   - `ReadOnly`
   - `WriteMemory`
   - `ExecuteTools`
   - `WriteExternal`
2. 收束 budget 语义：
   - tokens
   - tool calls
   - wall clock
   - external writes
3. 为 governance decision 建立结构化 trace
4. 让 `ResilientProvider` 成为正式可配置主路径
5. 统一 provider capability 视图：
   - context
   - vision
   - tool use
   - local/remote
   - fallback
   - 默认优先共用同一套 runtime/context/governance/tracing 机制；只有 capability 差异不可抹平时，才在 provider adapter 层分叉
6. 将 `HookEngine` 与 stage 对齐：
   - pre-context
   - pre-provider
   - pre-tool
   - post-tool
   - post-persist
   - post-response
7. 把以下 cross-cutting 逻辑迁入 hook/stage：
   - clarification gate
   - loop detection
   - tool degradation
   - trace injection
   - post-run evaluation tap

### 5.3 待完成清单

- [x] authority requirement 映射表完成
- [x] budget tracker 原型完成
- [x] governance decision trace 完成
- [x] `ResilientProvider` 可通过配置启用
- [x] provider fallback 可测试
- [x] `HookEngine` 与 runtime stage 对齐
- [x] 至少 3 类 cross-cutting 逻辑转入 hook/stage

当前已审过并可确认的第一阶段完成项：

- `brain::governance` 已具备正式 `AuthorityRequirement / GovernanceBudgetSnapshot / GovernanceDecision` contract
- `brain::governance` 已补齐 `ReadOnly / WriteMemory / ExecuteTools / WriteExternal` 的正式 scope 语义
- `reasoner` 已把 session token budget 接入 runtime 主路径，超预算会显式中止并发出治理预算事件
- `executor` 已为工具执行发出结构化 governance decision 与 tool-call budget 事件
- `ResilientProvider` 已具备 failover decision observer 与回归测试
- `kernel::factory` 已支持通过 app config 启用 `ResilientProvider(primary + fallback + circuit breaker)`
- provider schema 已具备统一 `capability_view` 投影，至少覆盖 `context_window_tokens / vision / tool use / streaming / local/remote / fallback`
- `HookTiming` 已有 `RuntimeStage -> HookTiming` 的正式映射，不再完全脱节于 runtime stage
- runtime hook 主路径已接入 `Agent + ActionExecutor`，至少完成：
  - `loop detection -> before_tool_call hook guard`
  - `tool degradation -> after_tool/on_error hook surface`
  - `trace injection -> hook metadata runtime refs`
  - `post-run evaluation tap -> before_response hook`
- `run_trace` 已会回写 hook capture 元数据与 degradation notes，并有回归测试覆盖

### 5.4 涉及 crate

- `brain`
- `security`
- `providers`
- `telemetry`
- `kernel`

### 5.5 本阶段禁止事项

- 不要把 hook 直接变成“万能脚本逃生口”
- 不要让治理语义继续散落在 UI、tool 和 provider 各层里
- 不要引入只在 Unix 下成立的 shell 假设

### 5.6 阶段完成标准

- 至少一条 provider failover 主路径具备 trace
- 至少一类高风险工具调用有 authority + budget 记录
- hook 的 modify/abort/skip 能被 trace 与测试证明

当前状态补充：

- 当前三条阶段标准已满足
- `P2` 可以按第一阶段核心闭环完成处理，后续若继续推进属于增强与扩展，而不是主路径缺口

---

## 6. P3 Eval / Harness / Witness 闭环

### 6.1 目标

建立 BenShu 自己的评测与执行证据闭环。

### 6.2 具体步骤

1. 定义评测核心对象：
   - `Task`
   - `Trial`
   - `Transcript`
   - `Outcome`
   - `Grader`
   - `Suite`
   - `Regression`
2. 定义 witness 工件：
   - `Witness`
   - `Replay Unit`
   - `Scorecard`
   - `Benchmark Fingerprint`
3. 先做 `Simulation Harness`
4. 再做 `Real Harness`
5. 明确 transcript failure vs outcome failure
6. 建立最小 suite：
   - 单 agent 工具执行
   - provider failover
   - approval flow
   - delegation flow
   - retrieval flow
7. 把 witness 接到 trace 与 state：
   - `trace_id`
   - `run_id`
   - `task_id`
8. 产出 scorecard 聚合报告

### 6.3 待完成清单

- [x] eval schema 完成
- [x] witness schema 完成
- [x] simulation harness 完成
- [x] real harness 完成
- [x] scorecard 原型完成
- [x] benchmark fingerprint 完成
- [x] 至少 20 个真实任务进入 suite
- [x] transcript/outcome split grader 跑通

当前状态：
- `telemetry` 已新增 `EvalTask / EvalTrial / EvalOutcome / WitnessBundle / Scorecard / BenchmarkFingerprint`
- gateway 前台 chat 主路径现在会把 `RunTrace` 通过 `SimulationHarness` 材料化为 `witness + outcome + scorecard`
- `telemetry::RealHarness` 已可执行真实前台 runtime case，并把真实 `RunTrace` 投影成 `witness + outcome + scorecard`
- 第一批 `real harness` runtime batch 已落地 `5` 条代表任务：单 agent 前台 chat、loop guard、tool degradation、provider failover、preemptive foreground merge
- 第二批 `real harness` context/memory batch 已落地 `5` 条代表任务：clean tool execution、retrieval signal injection、retrieval low-signal skip、stable session/thread refs、failing context injector non-fatal
- 第三批 `real harness` governance/memory batch 已落地 `5` 条代表任务：approval guard、prime delegation ownership、comm inbox owner rollup、pending review persistence、archive+prune lifecycle
- 第四批 `real harness` hardening batch 已落地 `5` 条代表任务：token budget exhaustion、relation depth hard cap、multimodal writeback contract、cancel marker persistence、pending-review resolution persistence
- state 侧既有 `witness_id` 回填继续作为 `task / run / trace / witness` 的 durable join point
- `TelemetryManager` 现已把 `run_trace / witness bundle / scorecard` 作为本地 durable 工件落盘，并支持重启后回读
- `TelemetryManager` 现已新增第一阶段 `witness log` 与结构化查询读面，gateway/panel 也已暴露 `get/query witness log` 与 `list/get scorecard` 接口
- `TelemetryManager` 的 `witness log` 现已具备第一阶段批量刷盘、最大积压约束与本地 retention 清理语义
- `TelemetryManager` 现已补齐第一阶段 free-text witness-log 查询与 scorecard 列表读面，`P3` 阶段标准已满足；后续保留项转为更长周期 regression retention 与 suite 持续扩展

### 6.4 涉及 crate

- 新增 `eval` crate 或在 `kernel + telemetry + state` 中先做最小闭环
- `brain`
- `telemetry`
- `state`
- `kernel`

P3 tracing 约束:

- 本阶段所有 `trace / replay / witness / scorecard` 工件，默认服从 `docs/secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md`
- gateway、panel、brain 不得再各自定义平行主 tracing 语义

### 6.5 本阶段禁止事项

- 不要把 simulation 结果写成主路径能力结论
- 不要只统计 pass/fail 而不记录 failure reason
- 不要跳过 witness，直接拿 trace 冒充评测工件

### 6.6 阶段完成标准

- 至少 20 个任务可回归
- 至少 1 个 scorecard 可长期保存
- 至少 1 条运行产出 `trace + witness + outcome`

当前判断：

- `P3` 第一阶段已完成
- 后续仅保留 witness/eval 长周期增强项，不再阻塞主线推进

---

## 7. P4 Workspace / Artifact / Retrieval Hardening

### 7.1 目标

把工具执行环境、文件语义、artifact 生命周期和检索硬化补到生产级。

### 7.2 具体步骤

1. 定义 thread-scoped runtime 文件语义：
   - `uploads`
   - `workspace`
   - `outputs`
   - `artifacts`
2. 建立统一 artifact 注册服务：
   - 来源 task
   - 来源 tool
   - 来源 agent
   - 路径/虚拟路径
   - 生命周期
3. 建立 artifact cleanup 策略
4. 让 gateway/panel 能读取 artifact 索引
5. 为 retrieval 链路补 safety net：
   - candidate 不足补扫
   - rerank 补候选
   - 热窗口补候选
6. 为 retrieval 链路补 degradation report
7. 为 retrieval 链路补 DoS hardening：
   - token bucket
   - negative cache
   - query signature
   - signature cooldown

### 7.3 待完成清单

- [x] artifact schema 完成
- [x] artifact 注册服务完成
- [x] artifact 清理策略完成
- [x] gateway artifact API 完成
- [x] panel artifact 展示完成
- [x] retrieval safety net 原型完成
- [x] degradation report 接入 trace/witness/eval
- [x] 查询路径 DoS hardening 原型完成

当前状态：

- `state::ArtifactManager` 已落地，artifact 现在有正式 durable schema：`scope(upload/workspace/output/artifacts)`、`lifecycle(ephemeral/session/durable)`、`task/run/trace/thread/session` 关联、`source_kind` 与 metadata
- `kernel` 已暴露 `state_artifact()`，artifact 不再只能散落在 `RunTrace.artifacts` 和 `TaskState.artifacts` 里
- gateway 前台 chat 主路径现在会把 `RunTrace.artifacts` 与 `TaskState.artifacts` 投影进 artifact registry，形成第一条正式注册链
- `ArtifactManager` 现已新增 lifecycle-aware cleanup contract：`ArtifactCleanupPolicy / ArtifactCleanupReport`，支持 `ephemeral/session/durable` 分级保留、`scope/source_kind` 过滤、`dry-run`、`max_delete`、`orphan_only`、`prune_missing_local_files` 与 `delete_local_files`
- artifact cleanup 第二阶段已具备更深的垃圾治理：可以专门命中“失去 `session/thread/task/run/trace` 关联的孤儿记录”，可以修剪 registry 中指向本地缺失文件的陈旧记录，也可以在受控条件下删除本地磁盘文件并回填删除报告
- gateway 已新增 `GET /api/artifacts`、`GET /api/artifacts/{id}` 与 `POST /api/artifacts`，支持 artifact 查询与 cleanup policy 执行
- panel `System -> Artifacts` 已具备第一阶段 artifact console：支持筛选、列表查看、详情检查，以及 lifecycle-aware cleanup 的 `dry-run / execute` 最小交互
- `engram::HierarchicalRetriever` 已新增 `search_recursive_with_report(...)`，第一阶段 retrieval safety net 现已具备 `candidate top-up + candidate-pool backfill + degraded route report`
- `knowledge_search` 工具与 gateway knowledge SSE 现会暴露 retrieval route/degradation report；当检索链路降级时，`executor` 会把 report 投影成 `RunTrace.degradation_notes`，因此 `trace / witness / eval` 已能吃到 retrieval degradation
- retrieval DoS hardening 第二阶段已落地：在 `query signature + token bucket + negative cache` 之外，又新增了 `signature cooldown`。当同一类高成本查询刚刚被节流后，后续重复命中会在 cooldown 窗口内快速短路，避免 retriever 被重复烧预算
- 本阶段最小完成标准“artifact 可索引、可追踪、可清理”与“至少 1 条 retrieval 主路径具备 safety net + degradation report”均已满足；`cleanup/DoS` 收尾项也已补到第一阶段闭环，P4 后续只保留长期增强项

### 7.4 涉及 crate

- `builtin-tools`
- `brain`
- `state`
- `telemetry`
- `knowledge`
- `engram`
- `security`
- `apps/gateway`
- `apps/panel`

### 7.5 本阶段禁止事项

- 不要把 artifact 继续散落在工具内部各自维护
- 不要把上传文件默认当成长时记忆
- 不要让 retrieval fallback 无限烧预算

### 7.6 阶段完成标准

- 用户可见文件语义统一
- artifact 可索引、可追踪、可清理
- 至少 1 条 retrieval 主路径具备 safety net + degradation report

---

## 8. P5 Multi-Agent / Gateway / Panel 协议收束

### 8.1 目标

在不推翻当前多 Agent 主干的前提下，收束协议、ownership 和应用层契约。

### 8.2 具体步骤

1. 收束 `comm` 消息 envelope：
   - `message_id`
   - `trace_id`
   - `task_id`
   - `parent_task_id`
   - `owner`
   - `role`
   - `target`
   - `causality metadata`
2. 收束 delegation 状态机：
   - created
   - accepted
   - running
   - failed
   - returned
   - transferred
3. 让所有 delegation 都落到 task state
4. 让 gateway 暴露统一 DTO：
   - task
   - trace
   - artifact
   - approval
5. 为 `connectors` 建立通讯软件观测主路径：
   - inbound / outbound message id
   - channel / session / user / thread 映射
   - delivery / retry / failure 分类
   - `trace_id / task_id / run_id` 关联
6. 让 panel 不再自行推导核心状态
7. 建立 parent-child task graph 展示

### 8.3 待完成清单

- [x] message envelope 第一阶段已携带 delegation causality / owner 视图
- [x] delegation 状态机第一阶段已接入 task state / gateway DTO / panel 读面
- [x] 通讯软件观测字段第一阶段已收束
- [x] 至少 1 条 connector 主路径具备结构化 channel observability
- [x] parent-child task graph 完成
- [x] gateway 标准 DTO 第一阶段完成
- [x] panel 不再维护核心 runtime 假状态（第一阶段）
- [x] delegation 端到端 trace 第一阶段可见

第一阶段现状:

- `comm` / `brain` / `state` / `gateway` 已统一保留 delegation 因果字段：`trace_id / task_id / parent_task_id / root_task_id`
- A2A processed event / session delegation inbox 已能携带 `visible_owner / memory_owner / approval_owner / final_response_owner`
- gateway 已暴露 session-level delegation evidence：`/api/sessions/{id}/delegation`
- panel 已能查看 runtime task graph、delegation state、owner rollup 与 recent A2A inbox
- connector/channel observability 已能显示 inbound/outbound、最近 session/chat/thread、失败分类与最后观测时间
- gateway 已对 `artifact / approval / runtime mode / sealed restore` 暴露稳定 DTO；panel 已通过 `/api/system/runtime-mode` 覆盖核心 runtime status，而不再长期依赖本地默认值

### 8.4 涉及 crate

- `brain`
- `comm`
- `connectors`
- `telemetry`
- `gateway`
- `state`
- `telemetry`
- `apps/gateway`
- `apps/panel`

### 8.5 本阶段禁止事项

- 不要为了协议化去重写现有 Coordinator/Fission 主干
- 不要先做复杂分布式拓扑
- 不要让 panel 继续维护独立状态逻辑

### 8.6 阶段完成标准

- 至少 1 条 delegation 链路可从 panel 一路追到 trace 与 task
- 子 agent 失败不会无声吞掉
- owner / handover / return mode 语义一致

---

## 9. P6 Profiler / Reproducibility / 清算期

### 9.1 目标

把所有前面完成的能力，纳入可复现、可对比、可长期维护的工程闭环。

### 9.2 具体步骤

1. 建立 profiler 工件：
   - latency
   - memory
   - energy 或等价资源指标
2. 建立 benchmark fingerprint
3. 让 profiler 与：
   - `run_id`
   - `trace_id`
   - `suite_id`
   关联
4. 统一导出格式
5. 清理旧旁路能力
6. 清理重复 DTO / 重复状态 / 旧工具旁路实现
7. 把所有“未主路径接线能力”改成：
   - experimental
   - deprecated
   - hidden

### 9.3 待完成清单

- [x] profiler 原型完成
- [x] benchmark fingerprint 主路径化
- [x] 导出格式稳定
- [x] 旧旁路能力清单清算完成
- [x] 文档承诺与主路径对齐完成

### 9.4 涉及 crate

- `telemetry`
- `kernel`
- `brain`
- `state`
- `infra`
- `apps/gateway`
- `apps/panel`

### 9.5 阶段完成标准

- 性能结论可复现
- 文档承诺与主路径一致
- 旧旁路能力不再混淆用户和开发者

当前状态：
- `telemetry` 已新增第一阶段 `ProfilerArtifact / ProfilerArtifactQuery / ProfilerExport`，覆盖 `latency / memory / energy-or-equivalent`
- profiler 工件已在前台 `RunTrace -> WitnessBundle` 主路径自动材料化，并关联 `run_id / trace_id / witness_id / suite_id / benchmark_fingerprint`
- `TelemetryManager::capture_evaluation_tap(...)` 已把 `post-run evaluation tap` 收敛为单一主路径入口，不再要求 gateway 手工拼接 `attach_simulation_witness + save_run_trace`
- gateway/panel 现已暴露 `get run profiler / query profilers / export profilers` 第一阶段读面
- profiler 导出格式已固定为稳定 schema version，支持跨机对比所需的可序列化基线
- 旧的 logging-only `TraceResult / AgentTracer::record()` 兼容路径已移除，主证据链只保留 `RunTrace + witness / scorecard`
- `P6` 现已完成，后续仅保留更高阶长期增强项，不再阻塞主线推进

---

## 10. P7 Sealed Memory Backup / Restore 收尾期

状态: `已完成，restore-only system-side 备份恢复闭环正式收口`

### 10.1 目标

在不允许“导出 BenShu 本体记忆明文”的前提下，补齐`可恢复、不可随意读出、受治理约束`的记忆备份能力。

这项能力不是：

- 可浏览的 memory export
- agent 本体记忆 JSON / vessel / 明文打包
- 面向外部系统的任意记忆下载接口

这项能力只应是：

- `restore-only` 语义的 sealed backup
- 对 `STM / Engram / 必要 state durable 工件` 的恢复型快照
- 受密钥保护、可校验、可审计的整库恢复能力

### 10.2 为什么必须放到最后

- 如果 memory lifecycle / retention / archive 语义尚未稳定，先做备份只会固化错误边界
- 如果先做 export/backup，很容易把“恢复快照”误做成“本体记忆导出”
- 只有主路径稳定后，才能定义哪些 durable 工件必须进入备份、哪些只能留在本机临时层

因此执行约束是：

- `P7` 不得早于 `P6`
- 在 `P7` 完成前，不得对外宣称“system-side 记忆备份恢复闭环已全部完成”

### 10.3 具体步骤

1. 定义 sealed backup contract：
   - `backup_id`
   - `created_at`
   - `agent_identity_fingerprint`
   - `contract_version`
   - `data_scope`
   - `encryption_mode`
   - `restore_policy`
2. 明确备份覆盖范围：
   - `STM` durable redb
   - `engram` durable db / retention metadata
   - 必要 `state` durable 工件
   - 可选 audit / lifecycle 索引
3. 明确排除范围：
   - 不导出明文 memory listing
   - 不提供任意 fact/message 浏览接口
   - 不把“备份文件”当作 agent export 成果物
4. 建立备份执行流程：
   - pause background memory writes
   - flush / commit
   - 复制 durable 文件到 staging
   - 生成 manifest
   - 加密封装
   - 恢复主路径写入
5. 建立恢复流程：
   - 校验 backup manifest
   - 校验 identity / contract version / fingerprint
   - dry-run 验证
   - 显式 restore
   - 生成 restore receipt 与 audit event
6. 建立密钥策略：
   - vault 托管
   - 或用户 passphrase
   - 默认不落明文密钥
7. 建立保留策略：
   - 最近 N 个恢复点
   - 自动清理旧恢复点
   - restore receipt 可查询
8. 在 gateway/panel 中只暴露：
   - 创建恢复点
   - 查看恢复点元数据
   - 执行恢复
   - 查看恢复审计结果

### 10.4 已落地第一阶段

- `security` 已具备 sealed backup contract、manifest、receipt、fingerprint 校验与 audit event
- gateway 已具备 restore-only API：
  - create restore point
  - list restore points
  - inspect restore metadata
  - execute restore
- 至少 1 条 `STM + Engram` 备份恢复自动化验证已经通过
- per-agent `.vessel` portability 已与 system-side backup 明确分离，不再混用语义

当前新增进展：

- panel 现已可创建 restore point、查看恢复点元数据、执行 dry-run、查看 policy basis、查看 receipt、执行显式 restore
- panel 现已要求先通过当前 restore point 的 dry-run，`Execute Restore` 才会解锁
- panel 现已直接展示 manifest 元数据、受保护文件清单摘要、receipt 关键字段，方便做 restore 治理核对
- `security` 已补上 policy deny-path 自动化验证，损坏 sealed payload 时会返回 `deny + dry_run_invalid`
- `security` 现已具备最近 N 个 restore point 自动保留策略，创建新恢复点时会清理超出上限的旧恢复点
- panel 现已展示 dry-run 检查时间、contract/fingerprint 与 delete 报告字节规模，便于恢复前治理确认

结论：

- `P7` 现已完成
- system-side sealed backup / restore 用户闭环已成立
- 系统仍保持 `restore-only` 语义，不提供本体记忆明文导出
- 后续只保留长期增强项，不再阻塞主线推进

### 10.5 待完成清单

- [x] sealed backup schema 完成
- [x] restore manifest 完成
- [x] backup fingerprint 完成
- [x] memory backup service 完成
- [x] restore dry-run 校验完成
- [x] restore receipt / audit event 完成
- [x] gateway 备份/恢复 API 完成
- [x] panel 恢复点只读展示完成
- [x] panel 恢复操作流完成
- [x] 至少 1 条 `STM + Engram` 备份恢复自动化验证通过

### 10.6 涉及 crate

- `brain`
- `engram`
- `state`
- `security`
- `telemetry`
- `apps/gateway`
- `apps/panel`

### 10.7 本阶段禁止事项

- 不要提供本体记忆明文导出
- 不要把 backup API 设计成“任意下载全部记忆内容”
- 不要跳过加密，直接复制数据库文件给 UI 下载
- 不要把 restore 做成静默覆盖，必须有显式 receipt 与审计
- 不要让 `brain` 承担完整备份封装逻辑，`brain` 只保留薄协调层

### 10.7 阶段完成标准

- 用户可以创建受密钥保护的恢复点
- 用户可以在不读取明文记忆的前提下恢复 durable memory
- 备份与恢复全程有 audit / trace / receipt
- 系统仍然不提供“导出本体记忆”能力
- 记忆备份能力不会破坏前面已经收束的主路径语义

---

## 11. crate 级待完成清单

当前新增进展：

- `state` 已新增独立 `run` durable record，主路径会持久化 `run_id / trace_id / session_id / task_id / witness_id / profiler_id / artifact_ids`
- `gateway` 主路径现已把 `run_trace` 产生的 artifact 反向同步回 `TaskState.artifacts`，形成稳定的 artifact/task 双向映射
- `kernel` 现已提供统一 `persist_runtime_mainline(...)` 装配入口，把 witness/trace/task/artifact/run 的主路径持久化收拢到单点依赖注入，gateway 不再手工拼接旁路落盘
- `kernel` 现已直接暴露 `run_real_harness_case/suite(...)`，把真实 harness 的 `trace/witness/scorecard/profiler/run-record` 装配成统一入口，不再要求上层自己拼 telemetry 持久化
- `security` 现已新增第一阶段 `query protection` 统一接口，采用 `allow / degrade / pause_current_path` 的个人用户保护语义；`gateway knowledge` 主路径会先做查询保护判定，再决定是否暂停深检索路径，并把保护结果显式返回前端
- `knowledge_search` 工具主路径现已消费同一套 `query protection` 接口；当深检索需要降级或暂停当前路径时，工具会自动回退到轻量检索，而不是封死用户查询
- `builtin-tools` 现已新增统一 `ToolDegradation` 标准对象，schema version 固定为 `benshu.builtin_tools.degradation.v1`；`cipher/mailer` 的 capability/info 路径已开始按统一结构返回 `active / kind / reason / user_message / fallback_path / retryable`
- `builtin-tools` 现已开始落统一 `ToolCleanup` 标准对象，schema version 固定为 `benshu.builtin_tools.cleanup.v1`；`chart` 的 `info/generate` 路径现已显式返回“临时脚本已自动清理”与“默认输出仍位于 OS temp 目录”的 cleanup 语义
- `builtin-tools` 现已新增共享 artifact registration helper，并先把 `chart` 输出接入 state artifact registry；工具返回里会显式附带 `artifact_registration`，后续新工具可直接复用这套 helper
- `gateway bridge` 现已把自然语言会话控制提升为轻量 `ConversationalControlIntent` 分类器，统一区分 `stop / pause / reprioritize / interject`；所有走 `InboundMessage` 的 bot/IM 适配层都可直接复用这条主路径
- `panel/gateway` 现已开始把聊天区 `STOP` 收敛为 session-scoped 前台停止；可见 UI 不再依赖右上角全局 `ABORT ALL`，而是为当前聊天会话调用独立停止接口

### 11.1 `brain`

- [x] runtime stage pipeline
- [x] provider capability 统一视图
- [x] failover 主路径化
- [x] hook 对齐 stage
- [x] clarification / loop detection / degradation hook 化
- [x] authority / budget / coherence 接线

### 11.2 `telemetry`

- [x] run trace schema
- [x] tool trace schema
- [x] artifact ref schema
- [x] witness summary schema
- [x] witness log
- [x] evaluation tap
- [x] profiler 工件关联

### 11.3 `state`

- [x] task 状态扩展
- [x] run/session/thread/task 关系建模
- [x] trial/run persistence
- [x] artifact/task 映射
- [x] parent-child task graph persistence

### 11.4 `security`

- [x] authority requirement 接线
- [x] action permission service（第一阶段）
- [x] `Permit / Defer / Deny`（第一阶段）
- [x] receipt / replay / policy basis lookup（第一阶段读面）
- [x] Windows 原生 sandbox 语义继续收口（第一阶段）
- [x] DoS hardening 与 query protection 接口
- [x] sealed backup encryption / restore receipt 能力

### 11.5 `builtin-tools`

- 进展：共享 `artifact registration` helper 已落地到 `chart` 与 `data_transform`，工具输出会回传稳定 `artifact_registration` 对象，并写入 state artifact registry，后续新工具可直接复用这套接线。
- 进展：`voice` 的 `text_to_speech` 已接入同一套 artifact registration helper，音频输出会回传 `artifact_registration` 与稳定 `cleanup` 读面。
- 进展：`document_understand` 已补统一 `ToolCleanup` 语义，远程 URL 输入会显式返回临时缓存自动清理信息，继续把 output/temp 生命周期从工具实现细节提升为稳定读面。
- 进展：`brain` 首次工具注入已支持大 schema 紧凑摘要，超大 `TypeScript / JSON Schema` 在 first-use 时只注入 compact summary，避免工具上下文被巨型 schema 挤爆。

- [x] tool degradation 标准对象
- [x] artifact 注册统一接入
- [x] output/temporary file cleanup
- [x] 大 schema 工具按需发现

### 11.6 `knowledge` / `engram`

- 进展：`engram::HierarchicalRetriever` 的 retrieval safety net、degradation report、query signature、negative cache、candidate/rerank budget 已在主路径稳定化；本轮补上 `gateway system restore -> durable memory metadata` 同步，sealed backup manifest / restore receipt 会写入 `engram.recovery.sealed_restore.*`，让 durable restore 协同不再只停留在 security 私有读面。

- [x] retrieval safety net
- [x] degradation report
- [x] query signature
- [x] negative cache
- [x] candidate budget / rerank budget
- [x] sealed backup manifest / durable restore 协同

### 11.7 `kernel`

- [x] eval/harness 装配
- [x] runtime mode 显式传递
- [x] witness/trace/task 主路径依赖注入

### 11.8 `comm`

- 进展：`comm::protocol` 已新增标准 causality metadata，并把 `session/trace/task lineage + parent/root message` 写进统一 `Metadata`；`CommEnvelope` / `CommClient` 现在有稳定的 `new_with_source / with_causality / link_to_parent / send_a2a_with_context` 入口。`DelegationOwnership / DelegationEnvelope` 也补了标准 helper，handover / return-mode / owner 语义不再依赖各调用点手拼字段。

- [x] message envelope 收束
- [x] ownership 语义
- [x] handover / return mode 主路径化
- [x] causality metadata

### 11.9 `apps/gateway`

- [x] task DTO
- [x] trace DTO
- [x] witness DTO
- [x] artifact DTO
- [x] approval DTO
- [x] 运行模式 DTO
- [x] sealed backup / restore DTO

### 11.10 `apps/panel`

- [x] task 主状态展示
- [x] trace 展示
- [x] witness 只读查询展示
- [x] artifact 展示
- [x] approval 展示
- [x] parent-child task graph 展示
- [x] 恢复点只读展示与 restore 操作流

---

## 12. 建议周序

如果按连续推进节奏执行，建议周序如下：

### 第 1 周

- P0 全部完成
- P1 schema 设计完成

### 第 2 周

- P1 主路径接线完成一半
- task / trace / gateway 查询先跑通

### 第 3 周

- P1 完成
- P2 开始：provider / governance / hook

### 第 4 周

- P2 完成
- 至少 1 条高风险工具调用具备 authority + budget + trace

### 第 5 周

- P3 开始
- 先 simulation harness，再 real harness

### 第 6 周

- P3 完成最小闭环
- 20 个任务进入 suite

### 第 7 周

- P4 开始
- 统一 artifact 与 retrieval hardening

### 第 8 周

- P4 完成最小闭环
- gateway/panel 能看 artifact

### 第 9 周

- P5 开始
- multi-agent / comm / DTO 收束

### 第 10 周

- P5 完成
- delegation 端到端可见

### 第 11-12 周

- P6 清算
- profiler / reproducibility / 文档对齐

### 第 13 周

- P7 开始并完成最小闭环
- 收口 system-side restore UI / restore dry-run / policy
- 完成 restore-only 用户闭环并保持无明文本体记忆导出

---

## 13. 执行闸门

每阶段开始前都必须通过上一阶段闸门。

### Gate 1: 进入 P2 前

- [x] 至少 1 条主路径有 task + trace
- [x] panel 能看见真实 task 状态

### Gate 2: 进入 P3 前

- [x] provider failover 有主路径 trace
- [x] hook 已接到 stage

### Gate 3: 进入 P4 前

- [x] 至少 1 条 harness 真实跑通
- [x] witness schema 已冻结第一版

### Gate 4: 进入 P5 前

- [x] artifact 统一语义已落地
- [x] retrieval degradation report 已接 trace

### Gate 5: 宣告总体完成前

- [x] 20+ eval suite
- [x] 3 条主路径端到端回放
- [x] 1 条 Windows 原生端到端自动化通过
- [x] 文档承诺与主路径一致

### Gate 6: 进入 P7 前

- [x] memory lifecycle / archive / retention 语义已稳定
- [x] `STM + Engram + state` durable 工件范围已冻结
- [x] 系统仍无“本体记忆明文导出”旁路
- [x] gateway/panel 的备份能力定义为 restore-only 而非 export

---

## 14. 最终说明

这份执行计划的核心思想只有一句话：

> BenShu 现在最需要的，不是更多“能力名词”，而是把已有强主干收束成可追踪、可评测、可治理、可恢复的生产级主路径。

因此推进时应始终记住：

- 不推倒重写
- 不做概念工程
- 不把已有强项毁掉
- 先补闭环，再谈上限

当这份文档与 `DEVELOPMENT_STANDARDS_AGENTOS.md` 冲突时，以主标准文档为准；当执行层遇到现实阻碍时，应更新本文件，而不是悄悄跳过执行顺序。

---

## 附录 A. brain 防臃肿守则

`brain` 是主代理 runtime 核心，但它不能演化成“超级杂物间”。

判断标准不是：

- `brain` 功能多不多

而是：

- `brain` 是否还主要负责 runtime orchestration
- 还是已经开始吞 durable storage、artifact 管理、grader、索引细节、UI 契约

### A.1 `brain` 应该承载的内容

这些能力应继续保留在 `brain`：

- runtime stage pipeline
- context build 与 provider 请求编排
- tool planning / tool filtering / loop detection
- governance 上下文接线
- hook/stage control
- delegation orchestration
- session 级推理状态
- 对 `state / telemetry / security / engram / comm` 的协调调用

### A.2 不应继续塞进 `brain` 的内容

以下内容默认不应新增到 `brain` 内部：

- durable storage schema 细节
- artifact registry 与 artifact cleanup
- eval grader / scorecard / suite 执行器
- profiler 导出与 benchmark 结果格式
- retrieval safety net 的底层扫描实现
- negative cache / token bucket / query DoS 细节
- 应用层 DTO
- panel 专用展示状态
- 网关路由专用协议整形逻辑

这些内容的推荐落点分别是：

- `state`
- `telemetry`
- `security`
- `engram`
- `knowledge`
- `apps/gateway`
- `apps/panel`

### A.3 允许放在 `brain` 的“薄协调层”

下面这些可以存在于 `brain`，但必须保持薄：

- trait / interface
- facade
- runtime decision
- policy routing
- capability selection
- stage emission

要求：

- `brain` 只决定“何时调用谁”
- 不实现“被调用模块的全部内部逻辑”

### A.4 出现以下信号时，说明 `brain` 开始臃肿

- 一个新能力在 `brain` 中新增了大量持久化表结构
- 一个新能力在 `brain` 中实现了完整导出/报告格式
- 一个新能力在 `brain` 中维护独立 artifact 生命周期
- 一个新能力在 `brain` 中实现了底层索引/扫描/缓存细节
- 一个新能力为了 UI 方便，直接把展示契约写进 `brain`

一旦出现这些信号，应立即暂停继续往 `brain` 塞逻辑，先判断是否应该下沉到：

- `state`
- `telemetry`
- `security`
- `engram`
- `knowledge`
- `apps/gateway`

### A.5 执行约束

今后任何涉及 `brain` 的大改动，在进入实现前都要先回答两个问题：

1. 这段逻辑是不是 runtime orchestration 本身？
2. 如果不是，它为什么不应该放到更合适的 crate？

如果第二个问题答不清，就不应默认放进 `brain`。
