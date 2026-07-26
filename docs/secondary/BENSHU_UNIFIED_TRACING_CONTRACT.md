# BenShu Unified Tracing Contract

> Platform Positioning: `Windows Native` is BenShu's formal product path and primary host platform; `WSL / WSL2 / Linux` routes are development/testing lanes for fast iteration and must not be presented as the default product deployment path.

> 关联文档:
>
> - `docs/secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md`
> - `docs/secondary/BENSHU_HARDNESS_DESIGN_PRINCIPLES.md`
> - `docs/secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md`
> - `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`

---

## 1. 文档目标

这份文档用于统一整个 `BenShu` workspace 的 tracing 语义。

目标不是把所有日志强行做成同一种格式，而是统一以下三件事:

- 运行对象身份
- runtime stage 语义
- execution evidence 的投影关系

一句话:

`BenShu 必须只有一套主 tracing contract，其他日志和局部事件都只能围绕这套 contract 投影。`

---

## 2. 一句话定义

`统一 tracing = 一套稳定的 ID 链 + 一套稳定的 stage 链 + 一套稳定的 trace -> replay -> witness -> scorecard 投影链。`

---

## 3. 必须统一的对象身份

以下字段是 workspace 级主身份，不允许不同 crate 各自重新发明。

### 3.1 主身份字段

- `trace_id`
- `run_id`
- `task_id`
- `session_id`
- `thread_id`
- `parent_task_id`
- `root_task_id`
- `witness_id`

### 3.2 语义要求

- `trace_id`
  表示一条可查询、可回放、可见证的运行主记录。
- `run_id`
  表示一次运行尝试。
  当前主路径里允许与 `trace_id` 等值，但语义上仍保留独立字段。
- `task_id`
  表示任务持久层主键，是 `state` 的 durable join point。
- `session_id`
  表示用户或代理连续会话身份。
- `thread_id`
  表示同一运行上下文中的执行线程/对话线程。
- `parent_task_id / root_task_id`
  表示 delegation / subtask 的因果链。
- `witness_id`
  表示评测/证据工件主键。

### 3.3 禁止事项

- 不要在不同 crate 里再造 `job_id / execution_id / request_id` 来替代上面这些主身份字段
- 不要把一次运行里最关键的 identity 只藏在日志字符串里
- 不要让 UI、gateway、brain、state 各自维护一套互相漂移的 ID 语义

---

## 4. 必须统一的 Runtime Stage

`RuntimeStage` 是整个 AgentOS 的主阶段语义。

当前标准阶段链为:

1. `Ingress`
2. `Governance`
3. `ContextBuild`
4. `Reasoning`
5. `ToolPlanningFiltering`
6. `Execution`
7. `PersistenceMemory`
8. `TraceAudit`
9. `Egress`

### 4.1 规则

- 所有主路径 trace 都应尽量投影到这套 stage
- Hook timing、governance decision、provider failover、memory lifecycle 不能各自维护独立的主阶段体系
- crate 可以保留局部细分阶段，但只能作为 `metadata/detail`，不能替代主 stage

### 4.2 允许扩展

允许补充:

- `detail`
- `metadata`
- 局部子阶段 label

但不允许:

- 在 gateway 单独定义另一套阶段
- 在 panel 再映射一套新的“用户阶段”
- 在 hooks 中脱离 `RuntimeStage` 自己长出一条平行主线

---

## 5. 必须统一的工件投影

统一 tracing 不只关心日志，还关心证据工件如何从运行中长出来。

标准投影链:

`RunTrace -> RunReplay -> WitnessBundle -> Scorecard`

### 5.1 RunTrace

主运行记录。

至少包含:

- 主身份字段
- `TraceStatus`
- stage traces
- tool traces
- artifacts
- degradation notes

### 5.2 RunReplay

由 `RunTrace` 投影出的可回放步骤视图。

要求:

- 保持稳定顺序
- 不伪造不存在的执行步骤
- 缺失回放能力必须显式暴露

### 5.3 WitnessBundle

P3 的执行证据工件。

至少包含:

- `EvalTask`
- `EvalTrial`
- `EvalOutcome`
- `RunReplay`
- `BenchmarkFingerprint`

### 5.4 Scorecard

对一组 witness 的聚合报告。

要求:

- 能累计 trial 数
- 能区分 `pass / warn / fail`
- 能保留 benchmark fingerprint

---

## 6. transcript failure 与 outcome failure

评测不得只给一个粗糙的 `pass/fail`。

至少要区分:

- `transcript failure`
  运行过程本身出了问题
- `outcome failure`
  运行虽然走完，但缺少 replay、task link、session link 或结果结构不完整

### 6.1 transcript failure 典型来源

- stage failed
- tool failed
- cancelled
- timed out
- degraded

### 6.2 outcome failure 典型来源

- missing replay
- missing task link
- missing session link
- empty execution

这条区分是 P3 witness/eval 的基础，不允许再回退成只有一个二元结果。

---

## 7. 各 crate 的最低职责

### 7.1 `telemetry`

- 持有主 trace schema
- 持有 replay / witness / scorecard schema
- 提供最小 registry 与查询读面

### 7.2 `state`

- 作为 `task / run / trace / witness` 的 durable join point
- 不直接发明另一套 trace schema

### 7.3 `brain`

- 在主运行路径产出 `RunTrace`
- 在 runtime / hook / governance / memory 上尽量投影到统一 stage

### 7.4 `gateway`

- 不创造新的 trace contract
- 负责把主路径 runtime outcome 接到 query/read side

### 7.5 `panel`

- 消费统一 trace/witness/scorecard 读面
- 不在 UI 层再解释出另一套主 contract

### 7.6 其他 crate

`security / providers / inference / connectors / builtin-tools / comm / orchestrator / scheduler`

要求:

- 局部事件可保留
- 但如进入主证据链，必须最终投影回统一 tracing contract

---

## 8. 与 Hardness 的关系

统一 tracing 不是纯 observability 工程，它直接支撑:

- truthfulness
- replayability
- witnessability
- governance auditability
- recovery explainability

如果 tracing 不统一:

- `hardness` 会失去统一证据面
- `memory / governance / approval / delegation` 会各自变成黑盒
- `P3-P7` 的许多能力只能停留在“局部存在”

因此:

`Unified Tracing Contract 是 BenShu hardness 的底层证据协议。`

---

## 9. 禁止事项

- 不要把普通日志当成 trace contract
- 不要把 trace 当成 witness 的替代品
- 不要跳过 witness 直接对外声称“已完成评测闭环”
- 不要让 crate 内部的调试字段直接变成长期 API 契约
- 不要让 UI 或 HTTP DTO 反向定义核心 tracing 语义

---

## 10. 当前执行顺序

建议按以下顺序推进:

1. 先统一 ID 与 stage contract
2. 再统一 `RunTrace -> Replay`
3. 再统一 `WitnessBundle -> Scorecard`
4. 再把 artifact / approval / retrieval / delegation 的证据投影补齐
5. 再补第一阶段 durable `run_trace / witness bundle / scorecard / witness log` 存储与 query read side
6. 最后做更完整的 real harness 长期 regression 与 searchable witness-log substrate

---

## 11. 当前状态判断

截至当前:

- `P1` 已完成主运行 trace/replay contract
- `P2` 已完成 governance/provider/hook 的主路径接线
- `P3` 已完成 witness/eval 第一阶段闭环
- `P3` 已完成四批 `real harness` suite，累计 `20` 条真实任务
- `telemetry` 已完成第一阶段 durable `run_trace / witness bundle / scorecard / witness log` 存储，以及最小 query/read side
- `telemetry` 已完成第一阶段 `witness log` 批量刷盘、最大积压约束与本地 retention 清理
- `telemetry` 已完成第一阶段 free-text witness-log 查询与 scorecard 列表读面

后续增强项:

- 更长周期的 regression retention
- 更多真实任务 suite 与更细粒度 witness 检索

---

## 12. 最终结论

`整个 BenShu 项目的 tracing 应该统一，而且必须统一。`

不是因为“日志看起来更整齐”，而是因为:

- 没有统一 tracing，就没有可靠 replay
- 没有可靠 replay，就没有可靠 witness
- 没有可靠 witness，就没有真正的 hardness / audit / recovery 闭环
