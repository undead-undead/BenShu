# BenShu 本地快速聊天双通道重构方案（中文）

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 测试链口径: 本文所有“快/全通道延迟、prefill、工具调用成本”结论，默认都应以 `GPU 优先测试链` 为准；`CPU` 路径只能用于 fallback/诊断，不应用来代表默认本地聊天体验。

> 关联核心文档: `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`
>
> 关联背景压缩主线: `docs/secondary/BENSHU_BACKGROUND_COMPRESSION_MAINLINE_PLAN_ZH.md`
>
> 关联 Prime Agent 架构: `docs/secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md`
>
> 关联 Hardness 原则: `docs/secondary/BENSHU_HARDNESS_DESIGN_PRINCIPLES.md`
>
> 关联 Truth / Verification 主线: `docs/secondary/BENSHU_TRUTH_AND_VERIFICATION_MAINLINE_PLAN_ZH.md`
>
> 关联本地模型栈: `docs/secondary/BENSHU_LOCAL_MODEL_STACK_PLAN_ZH.md`

---

## 0. 文档定位

这份文档解决的问题不是：

- 要不要继续保留完整 `Prime Agent` 主线
- 要不要砍掉背景压缩 / 记忆 / tracing / governance
- 要不要让本地模型退回成“只有一句 prompt 的裸聊天”

这份文档真正要解决的是：

**如何在不破坏现有 AgentOS 主线机制的前提下，让本地模型拥有一条“给一句话尽快回复”的快速通道。**

一句话：

**我们不是要删除机制，而是要把“本地快速聊天”和“完整 Agent 聊天”正式分成两条运行通道。**

### 0.1 状态标记

- `[x]` 已完成
- `[~]` 部分完成
- `[ ]` 未完成

### 0.2 当前问题

当前面板 `/api/chat` 走的是完整 `Foreground Runtime -> Reasoner -> Tool Surface -> Runtime Mainline`。

这条链的优点是：

- 背景压缩、记忆、trace、治理、工具调用都在
- 非常适合作为完整 Agent 主路径

但它带来的现实问题是：

- 首轮 prompt 很胖
- 本地主脑 prefill 很重
- 面板一句轻聊天也会吃完整主路径成本

当前真实现象是：

- 直连本地 `llama-server`：小 prompt 很快
- 面板 `/api/chat`：即使只问一句简单话，也会因为完整主路径而明显变慢

因此：

**当前缺的不是“模型更快”，而是“主路径分流”。**

### 0.3 当前完整通道过慢的真实原因

这里必须单独写清楚，避免把问题误判成“本地模型不行”。

当前完整通道慢，主要不是单点 bug，而是三类串行成本叠加：

1. **首轮 prompt 明显过胖**
- 完整通道并不是只把用户一句话发给模型
- 它还会带上：
  - Prime Agent system prompt
  - tool surface
  - truth / governance / runtime note
  - background layer
  - 其他主线 metadata
- 当前真实 trace 已出现：
  - `provider_prompt_tokens ≈ 9.4k`
  - `deferred_tool_visible_count ≈ 35`
- 这会直接拉高本地主脑的 prefill 成本

2. **主回复完成后，Foreground Runtime 还在同步做背景刷新与 session checkpoint**
- 当前 `finalize_outcome(...)` 不是“模型一回完就立即返回”
- 它还会继续同步执行：
  - `maybe_refresh_background(...).await`
  - `checkpoint(...).await`
- 这意味着用户实际上在等待：
  - 主回复
  - 背景压缩刷新
  - session 持久化

3. **Gateway 在 HTTP 返回前，还会继续等待 runtime mainline 持久化**
- `/api/chat` handler` 当前会：
  - 先等 `chat_session(...)`
  - 再等 `persist_runtime_mainline(...)`
  - 最后才返回 HTTP
- 这会把：
  - task save
  - artifact registration
  - run trace / witness / run record 持久化
 继续压在用户首回复之前

额外还存在一个放大器：

4. **Gateway 顶层统一 60s timeout**
- 它不是当前慢的主因
- 但会把已经很慢的完整通道进一步放大成超时失败

一句话：

**当前完整通道的问题不是“模型不会回”，而是“用户要等完整 Agent 主线跑完太多同步步骤”。**

### 0.4 这次重构不把问题说成什么

这次重构不应该把现状误判为：

- `llama.cpp` 或 `Gemma 4` 本身严重失常
- 必须砍掉背景压缩 / memory / tracing 才能变快
- 工具系统完全没有过滤

更准确的判断是：

- 当前系统已经有一层工具过滤
- 当前系统也已经有背景压缩与主线治理能力
- 真正的问题是：
  - **完整通道的“首轮同步成本”太高**
  - **轻聊天没有和完整 Agent 主线分流**

### 0.5 当前实现收口（2026-04-07）

这份文档到当前这个时间点，已经不是纯 proposal。

当前已经落地的关键点：

1. **Fast Chat 已经是实际运行通道**
- 简单低复杂度请求已经能走快通道
- 本地真实链路下，简单一句话请求实测约 `0.8s`

2. **`/api/chat` 首返回已经减掉一层同步阻塞**
- `persist_runtime_mainline(...)` 不再阻塞首回复返回

3. **完整通道里的文件读取 correctness 问题已经修掉**
- 之前“读取文件”类请求可能没有真正升级成强制工具路由
- 当前已补成：
  - `FileOps` 是正式硬路由
  - 必须真实调用文件系统工具
  - 有专属系统提示
  - 有专属偏好工具集
  - 已进入默认 prompt-visible 工具索引

4. **Gemma 4 + GPU 实链已经打通**
- 当前问题不再是 CPU 回退
- 本地主脑已确认工作在 GPU 路径上

当前仍然没完全收完的：

1. **完整通道仍偏重**
- 当前真实完整文件请求仍在 `10s` 级别
- 主因仍是 prompt / tool surface / runtime 主线偏厚

2. **完整通道的工具暴露还没压到最优**
- 这次先修的是“文件请求必须真用工具”
- 还没把完整通道整体工具面缩到最轻

3. **Panel / Gateway / Trace 的 fast/full 读面还没完全收完**
- `gateway` 响应里已经带：
  - `chat_route`
  - `tool_surface_mode`
  - `runtime_persistence_status`
- 面板聊天框里已经能直接看到：
  - `FAST | FULL`
  - `tools:none | minimal | full`
  - `persist:not_needed | queued | skipped_saturated`
- trace metadata 也已经开始携带：
  - `chat_route`
  - `tool_surface_mode`
- 面板也已经支持 debug 强制：
  - `Auto`
  - `Fast`
  - `Full`
- 剩余主要是继续统一更多 trace/read 面细节

---

## 1. 设计总原则

### 1.0 长会话与本地模型的四个硬原则

这次重构后续继续推进时，必须长期坚持下面四条原则。

它们不是“可选优化”，而是本地模型想兼顾性能与机制时的基本约束：

1. **轻请求必须有轻通道**
- 简单问候、短确认、轻闲聊、低风险短问答
- 不能再默认陪着完整 Agent 主线一起走最重路径
- 否则本地主脑会被无意义的 prompt prefill 和运行时主线拖慢

2. **工具必须按路由缩面**
- 不是所有请求都该看到同一批工具
- 能够明确归类到某个硬路由的请求，应该优先只暴露该路由的最小工具家族
- 默认宽工具面只应用在确实还不明确、需要通用探索的完整通道场景

3. **背景必须重写，不能只追加**
- 长 session 下，背景层迟早会继续变大
- 因此背景压缩不能只是“不断往上叠 summary”
- 必须允许：
  - 重写
  - 衰减
  - 退出 active background
  - 分代归档

4. **长期记忆必须按需召回，不能常驻 prompt**
- durable memory / archive / facts 的价值在于“需要时拿出来”
- 不在于“每轮都挂在上下文里”
- 否则长会话下会把有限窗口不断让给旧材料，而不是当前真正需要的材料

一句话：

**本地模型要同时保性能和机制，不靠“永远塞更多 token”，而靠“请求分流、工具缩面、背景重写、记忆按需召回”。**

### 1.1 不破坏的机制

这次重构明确不能破坏下面这些主线：

1. `Prime Agent`
- 前台仍是单一人格
- 不能因为做快速聊天就把前台重新做成工具壳

2. `Truth / Verification`
- 不能为了速度把真实高风险验证路径砍掉
- 只能让低风险简单聊天绕开重流程

3. `Background Window Compression`
- 背景压缩主线保留
- 只能增加 `lite background` 模式，不能废掉 `full background`

4. `Hardness`
- 不能为了速度绕过 hardness 路由
- 只能让低硬度场景进入快通道
- 高硬度场景必须升级到完整主线

5. `Memory`
- 短期记忆、session、engram、durable fact 仍保留
- 快速通道只能减少“本轮注入”，不能改变“长期存储语义”

6. `Tracing / Witness / Scorecard`
- 完整主线必须仍有完整读面
- 快速通道可以轻量化，但不能完全黑盒

7. `Global Local Model Stack`
- 主脑仍然可按 Agent 自由配置
- `SLM / STT / TTS / OCR / Embedding / Rerank / FactCheck` 仍是全局配置

### 1.2 重构目标

重构后的系统应同时满足：

1. 本地模型面对轻聊天时可以明显更快
2. 完整 Agent 主线机制不丢
3. 何时走快通道、何时走全通道必须可解释
4. Gateway / Panel 行为必须一致
5. 回退路径必须明确

---

## 2. 最终目标架构

### 2.1 双通道模型

重构后的聊天系统明确分成两条通道：

#### A. `Local Fast Chat`

适合：

- 一句问答
- 轻闲聊
- 轻陪伴回复
- 简单确认
- 不需要工具、不需要复杂外部验证的短交互

目标：

- 极低 prompt 体积
- 极低 prefill 成本
- 尽快首 token

#### B. `Full Agent Chat`

适合：

- 工具调用
- 文档 / 网页 / 截图 / recall / task state 驱动场景
- 背景压缩写回
- 复杂协作
- 高风险回答
- 需要完整 trace / witness / persistence 的主任务路径

目标：

- 保持现有完整 AgentOS 主线能力
- 不为速度牺牲产品正确性

### 2.2 核心思想

一句话：

**快通道负责“立刻回”，全通道负责“真的做事”。**

---

## 3. 两条通道分别保留什么

### 3.1 `Local Fast Chat` 保留什么

必须保留：

- `Prime Agent` 的最小人格基线
- 当前 session 的最小身份信息
- 最小近期上下文
- 基础输入安全检查
- 最小 trace
- 最小 cancel / timeout / runtime refs

可以轻量化：

- 工具面
- tactical
- background full envelope
- runtime note
- 完整 persistence
- 完整 witness / scorecard 收口

### 3.2 `Full Agent Chat` 保留什么

完整保留：

- `ContextManager`
- `BackgroundEnvelope`
- `MemoryManager`
- `Engram`
- `SLM tactical`
- `Truth / Verification`
- `Tool Surface`
- `Task / Trace / Witness / Scorecard`
- `foreground_runtime` 的完整主路径

---

## 4. 快通道如何做到“快”

### 4.1 小 Prompt Profile

本地主脑快速聊天必须引入独立 prompt profile：

- 更小 system prompt
- 不附带厚 runtime note
- 不附带完整 tool contract
- 不附带完整 background envelope

这不是“另一个人格”，而是：

**同一个人格的轻量 prompt profile。**

### 4.2 Lite Background

快通道不带 `full background`，只带 `lite background`。

`Lite Background` 只允许包含：

- `persona baseline`
- `addressing preference`
- 极短的 `relationship frame`
- 极短的 `session topic`

不允许默认带入：

- 全量 backend context objects
- 长 recent window summary
- evidence refs 展示层
- 复杂 task state
- recall object 列表

### 4.3 极小最近窗口

快通道默认只保留：

- 最近 `1~3` 轮用户/助手原始消息

而不是：

- 完整 `selected_history`
- 最近窗口摘要再加原始历史双份携带

### 4.4 默认无工具

快通道默认：

- `tools = []`

只有触发升级条件时，才切到全通道。

### 4.5 tactical 默认降级

快通道默认：

- 不跑完整 `SLM tactical`
- 或只跑极轻规则型判断

原因不是 tactical 不重要，而是：

- 快通道的目标是低延迟
- tactical 应只在复杂场景触发

### 4.6 后置 persistence

快通道允许：

- 先返回回复
- 再异步补 session checkpoint / trace persistence / background writeback

前提：

- 失败不能污染长期层
- 必须有明确 retry / fallback / drop report

---

## 5. 何时必须从快通道升级到全通道

### 5.0 `Hardness` 是前置闸门

快通道与全通道的选择，不能先按“想不想快”来决定，而必须先按：

- 这轮请求的硬度高不高
- 风险高不高
- 不确定性高不高
- 是否需要外部验证、工具、记忆、后台对象

也就是说：

**`Hardness` 先决定这轮请求是否允许进入快通道，之后才谈 prompt profile、背景层级和工具暴露。**

一句话：

**快通道不是轻率通道，而是“仅限低硬度请求”的轻量执行面。**

### 5.1 工具意图

满足任一条件直接升级：

- 明确文件/网页/文档/代码操作请求
- 明确搜索、读取、执行、生成请求
- 明确多步任务

### 5.2 风险与验证要求

满足任一条件升级：

- 医疗 / 法律 / 金融 / 高风险事实判断
- 需要 verification
- 需要 source-backed answer
- 明显高不确定性或存在“不能猜”的回答要求

### 5.3 背景与记忆要求

满足任一条件升级：

- 需要完整背景压缩刷新
- 需要 relationship / durable fact 晋升判断
- 需要 memory recall

### 5.4 多模态与 backend object

满足任一条件升级：

- 图片 / 截图 / 文档 / 网页 / OCR / STT / task state 输入
- 需要 backend object 统一处理

### 5.5 长任务与运行时治理

满足任一条件升级：

- 需要 task / trace / witness / scorecard 的完整主线
- 需要 approval / governance / intervention

---

## 6. 快通道不是什么

必须写清楚：

快通道不是：

- 另一个独立 Agent
- 另一个人格
- 直接绕开安全检查的裸模型接口
- 永远不写记忆
- 永远没有背景

快通道只是：

**同一个 BenShu，在低复杂度场景下的一条低成本执行通道。**

---

## 7. 与现有系统的正式挂接点

### 7.1 `gateway /api/chat`

这里应增加：

- `chat_route = fast | full`
- 默认自动判路
- 可调试地强制指定

作用：

- 面板与 gateway 能明确知道本轮走的是哪条通道

### 7.2 `Foreground Runtime`

这里要增加：

- `LocalFastChatProfile`
- `FullAgentChatProfile`
- 路由判断入口

要求：

- 不是复制一份新 runtime
- 而是在现有 runtime 内部显式分 profile

### 7.3 `ContextManager`

这里要支持：

- `lite background assembly`
- `full background assembly`

要求：

- 同一套 `ContextManager`
- 两套 budget / assembly profile

### 7.4 `Reasoner`

这里要支持：

- `fast request` 不挂工具或只挂极小工具集
- `full request` 保持现有主路径

### 7.5 `MemoryManager`

这里要支持：

- 快通道默认只做轻 session 更新
- 全通道保持现有 authority path

### 7.6 `Tracing`

读面必须显式标出：

- `chat_route`
- `lite_background_used`
- `tool_surface_mode`
- `tactical_mode`
- `post_response_persistence_mode`

---

## 8. 必须保住的机制矩阵

### 8.0 Hardness

保住：

- 现有 hardness 设计原则
- 复杂 / 高风险 / 高不确定性请求升级到完整主线
- 不允许“为了快”而绕过风险分级

新增：

- `fast/full` 路由前置 hardness gate
- `lite hardness` 与 `full hardness` 两档执行面

其中：

- `lite hardness`
  - 适用于轻闲聊、低风险短问答
  - 保留最小输入安全检查、基本不确定性约束、基本拒绝/升级能力
- `full hardness`
  - 适用于高风险、复杂任务、验证需求、工具需求、多源 backend object 场景
  - 继续走现有完整主线

这意味着：

**快通道并不是 hardness 关闭，而是 hardness 通过后才允许进入的轻量路线。**

### 8.1 背景压缩

保住：

- `BackgroundEnvelope`
- `revision`
- `evidence refs`
- `reject / rewrite / promote`

新增：

- `LiteBackgroundEnvelopeView`

### 8.2 记忆系统

保住：

- hot memory
- session snapshot
- engram durable path

新增：

- 快通道只注入轻量 active background
- 不等于放弃长期记忆系统

### 8.3 Truth / Verification

保住：

- 高风险问题必须升级到全通道
- 不允许快通道在高风险场景下偷跑

### 8.4 Tool Surface

保住：

- 完整工具主线存在

新增：

- `fast_chat_default_tools = []`
- `fast_chat_minimal_tools = [optional tiny allowlist]`

### 8.5 Tracing / Witness

保住：

- 全通道完整记录

新增：

- 快通道最小记录面
- 不得完全失明

---

## 9. 配置边界

### 9.1 主脑配置

继续保持：

- 每个 Agent 独立配置
- `provider`
- `model`
- `base_url`

### 9.2 全局小模型配置

继续保持：

- `SLM`
- `STT`
- `TTS`
- `OCR`
- `Embedding`
- `Rerank`
- `Fact Check`

统一全局配置，不下沉到单 Agent。

### 9.3 新增配置项

建议新增：

- `fast_chat_enabled`
- `fast_chat_max_recent_messages`
- `fast_chat_use_tools`
- `fast_chat_allow_tactical`
- `fast_chat_profile`
- `fast_chat_max_prompt_tokens`
- `fast_chat_async_persistence`

---

## 10. 分阶段实施方案

### Phase F0：立项与边界冻结 `状态: [x]`

目标：

- 明确快通道不是新 Agent
- 冻结“保机制不保臃肿”的边界

交付：

- 本文
- Gateway / Panel 路由口径统一

### Phase F1：最小快通道 profile `状态: [x]`

目标：

- 先让本地一句话聊天显著变快

交付：

- `LocalFastChatProfile`
- 小 system prompt
- `tools = []`
- 最近 `1~3` 轮消息
- `lite background`

完成标准：

- 简单问候/闲聊延迟明显低于当前完整主链

当前结果：

- 已达到
- 真实本地链路下，简单一句话请求约 `0.8s`

### Phase F2：自动路由判定 `状态: [x]`

目标：

- 自动区分 `fast` 与 `full`

交付：

- 先做 hardness gate
- 低风险/无工具/无多模态/无 recall -> `fast`
- 其他 -> `full`

完成标准：

- 主路径不丢
- 误路由率可接受
- 高硬度请求不会误进快通道

当前结果：

- 已有前置 gate
- 文件路径类请求不会误进 fast
- 文件读取类请求会进入 full，并升级到真实 `FileOps` 工具路由

### Phase F3：轻 persistence + 轻 trace `状态: [x]`

目标：

- 快通道先回后写

交付：

- post-response persistence queue
- 失败退化报告
- route-aware trace metadata

当前结果：

- `persist_runtime_mainline(...)` 已不再阻塞 `/api/chat` 首返回
- `gateway` 响应已显式返回：
  - `runtime_persistence_status = not_needed | queued | skipped_saturated`
- 面板聊天气泡已能直接显示该状态
- `chat_route`
- `tool_surface_mode`
  也已贯通到 gateway / panel / trace

### Phase F4：工具暴露再收口 `状态: [~]`

目标：

- 让本地主脑默认只看到极小工具集

交付：

- `fast tool allowlist`
- `tool surface mode = none | minimal | full`

当前结果：

- 快通道默认已极轻
- 完整通道里 `FileOps` 的专属工具家族已经补齐
- 完整通道已经支持 route-aware `tool surface mode`
  - `none`
  - `minimal`
  - `full`
- 文件读取类硬路由现在会走 `minimal` 工具面，而不是默认宽工具面
- 但完整通道整体工具暴露仍未压到最优

### Phase F5：产品验收与回退机制 `状态: [x]`

目标：

- 快通道在真实产品下成立

验收：

- 一句轻聊天显著快
- 复杂任务仍自动升级到完整主线
- 快通道不破坏背景压缩 / 记忆 / truth / trace

当前结果：

- 一句轻聊天显著快：已达到
- 文件读取类复杂请求自动升级到完整主线并真实走工具：已达到
- `gateway / panel / trace` 已显式暴露：
  - `chat_route`
  - `tool_surface_mode`
- 面板已支持 debug 强制：
  - `Auto`
  - `Fast`
  - `Full`
- 现阶段剩余的已经属于完整通道继续减肥，不再是这份重构文档的功能缺口

---

## 11. 文件级落点建议

### A. `apps/gateway`

- `apps/gateway/src/api/handlers/chat.rs`
  - 增加 route metadata
  - 支持 debug 强制 fast/full
  - 返回 `runtime_persistence_status`

- `apps/gateway/src/api/init.rs`
  - 评估 `/api/chat` 超时是否需按路由分层

### B. `crates/brain`

- `crates/brain/src/agent/foreground_runtime.rs`
  - 引入 chat profile 分流

- `crates/brain/src/agent/hardness.rs` 或现有 hardness 路径
  - 在路由前提供快/全通道的 hardness gate

- `crates/brain/src/agent/context.rs`
  - 实现 lite/full background assembly

- `crates/brain/src/agent/reasoner.rs`
  - 实现 route-aware tool surface

- `crates/brain/src/agent/tactical.rs`
  - fast route 默认只走规则层

- `crates/brain/src/agent/memory/mod.rs`
  - 定义 lite background view

### C. `apps/panel`

- 面板聊天框显示当前路由：
  - `Fast Chat`
  - `Full Agent`

- 为调试保留：
  - 强制 fast
  - 强制 full

- 面板聊天气泡显示：
  - `tools:none | minimal | full`
  - `persist:not_needed | queued | skipped_saturated`

---

## 12. 风险与回退

### 12.1 风险

1. 快通道过轻，导致人格变薄
2. 快通道误判，把该走全通道的请求放轻了
3. 异步 persistence 丢失状态
4. 两套 profile 长期漂移，造成行为不一致

### 12.2 回退

1. 任意高风险条件直接升级全通道
2. 任意快通道失败可直接 fallback 到全通道
3. 任意异步 persistence 失败只影响轻量写回，不污染 durable
4. 若 profile 漂移，则以 `Full Agent Chat` 为 authority

---

## 13. 完成标准

这次重构只有在同时满足下面条件时，才算完成：

1. 本地简单聊天速度显著改善
2. `Hardness` 仍然是前置闸门，而不是被绕过
3. 背景压缩主线没有被绕死
4. 完整 Agent 主线仍然可用
5. 高风险场景仍走 Truth / Verification 主线
6. 主脑仍按 Agent 自由配置
7. SLM/STT/TTS/OCR/Embedding/Rerank 等仍是全局配置
8. Panel / Gateway / Trace 对 `fast/full` 路由读面一致

---

## 14. 一句话总结

**BenShu 接下来不是要削弱 AgentOS 主线，而是要在现有 Prime Agent、Background Compression、Memory、Truth/Verification、Tracing 主线之上，增加一条本地快速聊天通道：让轻聊天尽快回，让复杂任务继续走完整 Agent。**

---

## 15. 本轮实际修正内容（对照代码）

这次真正落到代码并完成复测的内容，主要有：

1. **Fast Chat 主路径已接通**
- 简单请求不再默认走完整 Agent 主线

2. **完整通道减少了一层同步阻塞**
- `/api/chat` 返回前不再同步等待 `persist_runtime_mainline(...)`

3. **`FileOps` 从“半存在”补成“一等硬路由”**
- 之前问题：
  - 文件读取请求虽然会进完整通道，但不一定会被当成“必须真调工具”的能力路由
  - 模型可能直接猜文件内容
- 当前修正：
  - `CapabilityRouter::classify_query_route(...)` 正式纳入 `FileOps`
  - `capability_route_requires_real_tool_call(...)` 正式纳入 `FileOps`
  - `capability_route_preferred_tool_names(...)` 增加 `read_file/list_dir/edit_file/write_file/tool_search`
  - `capability_route_system_message(...)` 增加 `FILE_OPS_HARD_ROUTE`
  - 文件系统工具家族已进入 prompt-visible 索引
  - 文件路径 / 文件读取表达已能稳定推到 `file_ops`

4. **真实链路复测结果**
- 简单一句话：约 `0.8s`
- 读取 `AGENT.md` 前三行：
  - 已真实触发 `read_file`
  - 已不再是假装读过文件
  - 当前耗时仍约 `10s` 级别

一句话：

**这一轮已经把“轻请求快起来”和“文件请求必须真走工具”两件关键问题收掉了；剩下的主问题不再是 correctness，而是完整通道还要继续瘦。**
