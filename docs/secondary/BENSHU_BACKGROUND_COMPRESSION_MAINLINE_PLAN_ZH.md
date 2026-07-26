# BenShu Agent 背景信息窗压缩主线状态与开发方案（中文）

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 关联核心文档: `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`
>
> 关联产品路线: `docs/secondary/BENSHU_PERSONAL_JARVIS_ROADMAP_ZH.md`
>
> 关联前台架构: `docs/secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md`
>
> 文档定位: 本文定义的是 `BenShu` 现有 Agent 主线的 `产品级背景信息窗压缩` 方案，不是 KV cache 压缩，不替代现有记忆系统，也不重造新的上下文框架。

---

## 0. 这份文档要解决什么

### 0.1 状态标记

- `[x]` 已完成
- `[~]` 部分完成
- `[ ]` 未完成

这份文档回答的问题不是：

- 要不要做底层 `KV compression`
- 要不要造新的 memory system
- 要不要重写 `ContextManager`

而是：

**在 BenShu 现有 `Prime Agent + MemoryManager + ContextManager + Engram` 架构上，如何做一套真正产品级的“背景信息窗压缩”机制，让 Agent 在单一 session 下长期连续存在，而不是因为背景窗耗尽被迫频繁重开 session。**

本文的“背景信息窗压缩”特指：

- 对当前 Agent backend background window 进行结构化提炼、分层保留与动态更新
- 覆盖对话历史、工具结果、文档/网页/截图/多模态结果、任务状态、workspace focus、memory recall 等背景输入
- 让重要人格、关系、近期状态、进行中任务持续保留
- 在上下文预算受限时，把“原始历史”转成“可持续背景层”

本文明确不指：

- `llama.cpp KV cache` 压缩
- page/block 级推理缓存压缩
- 模型权重量化

### 0.2 最终目标强度

这里必须单独写清楚：

本文目标不是做一个“普通 history summary 机制”，而是最终逼近一种更强的产品形态：

- Agent 在长会话下拥有稳定、持续、自更新的背景层
- 即使原始历史被裁剪，用户仍明显感受到“还是同一个 Agent”
- 前台人格、关系状态、近期主题、当前任务不会因为上下文窗逼近上限而频繁断裂
- 背景层具备 revision、evidence、晋升与回滚语义，而不是一次性摘要文本

一句话：

**本文的目标强度应理解为“持续背景人格层（Persistent Background Persona Layer）”，而不是“上下文摘要优化”。**

### 0.3 当前实现收口

截至当前代码状态，这条主线已经不是纯规划文档，而是进入了“主路径已接通、产品效果仍在继续验收”的阶段。

当前已经落地的骨架包括：

- 正式 `BackgroundEnvelope / revision / evidence refs / quality signal` 对象模型
- `ContextManager` 中的背景层装配
- 无 `SLM` 也可工作的规则型背景 verdict
- `Foreground Runtime` 的背景刷新、checkpoint、resume
- `MemoryManager` authority path 下的 session background 持久化与 durable review
- trace / witness / panel 的背景元数据读面
- `engram` 对 archived/recovered session 背景快照的回填与 retention 保护

当前需要特别收口的一点是：

- 当前背景压缩主线仍以最近消息窗口为主要驱动输入
- `workspace_focus`、`source_path`、`tool_name`、`media_preprocess_source_ref` 等后端信号已经接入
- `web / artifact / task / tool / multimodal / memory recall` 这些 typed backend objects 已经接入
- 但更大范围真实产品日志下的多源 backend 输入，还没有全部收成统一背景 object 家族与统一产品回归集

当前仍在继续推进的重点包括：

- 多源 backend 背景输入的一等建模
- 更大范围真实产品日志/真实用户任务集回归
- 背景写回 hallucination 产品数据集

### 0.4 当前完成度口径

截至当前版本，这条主线更适合按下面的口径理解：

- `B1 ~ B5` 的主路径骨架已经接通
- `B6` 已经进入“前台工作区场景有效，但多源 backend 输入仍待扩展”的阶段
- `B7` 已经进入“核心产品回归成立，但更大范围真实任务集仍待补完”的阶段

换句话说，当前状态更准确的结论是：

- 背景信息窗压缩主线已经进入“可用且已被回归验证”的阶段
- 但还不能说“所有尾项都已完成”
- 剩余工作主要集中在：
  - 更大范围真实产品日志/真实用户任务集
  - 多源 backend object 家族的进一步统一
  - 更大范围 hallucination 产品数据集

也就是说，本文不再是“早期从零开工的开发计划”，而是：

**一条已经进入主代码、正在继续收产品验收和真实任务集的 Agent 背景信息窗压缩主线。**

---

## 1. 为什么现在必须做

对于普通工具型 Agent，上下文裁剪通常只是体验优化。

对于长会话 Agent，背景信息窗裁剪会直接变成产品生死线。

原因很简单：

1. 长会话的背景输入天然更长
2. 单一人格连续性比单次回答质量更重要
3. 用户最在意的是“你还记得我、记得我们刚刚在聊什么、记得你自己是谁”
4. 如果每次上下文窗接近上限就强制换 session，用户感知上会像“换了一个人”

所以背景压缩的直接产品目标不是“减少 token”，而是：

- 尽量不因为上下文窗而被迫开新 session
- 保持同一前台人格连续存在
- 降低长会话下的人格漂移和关系遗失
- 让最近状态、重要偏好、长期关系在单一 session 里可持续携带

一句话：

**背景压缩的目标是让 session 活下去，而不是让 token 数字好看。**

### 1.1 对长会话 Agent 的最终产品口径

如果这条线成功，用户最终应感受到的是：

- 不需要因为上下文窗而频繁手动或被动开启新 session
- 长对话后前台 Agent 仍保有稳定人格与关系记忆
- 最近在聊什么、正在做什么、彼此关系处于什么状态，不会轻易断线
- 背景层会像“持续存在的内部自我状态”那样工作，而不是像临时 prompt 技巧

也就是说，这条线的目的不是“压缩成功”，而是：

**让 Agent 在单一 session 下尽量像同一个持续存在的对话主体。**

---

## 2. 当前代码里的真实基础

BenShu 现在并不是没有背景压缩地基，而是已经有了几块关键底座，只是还没有产品化收口。

### 2.1 上下文构建底座已经存在

位置：

- `crates/brain/src/agent/context.rs`

当前已经具备：

- `ContextManager`
- `system prompt` 注入
- `ContextInjector` 机制
- `history budget` 计算
- 超长单消息 `soft_trim`
- 超预算历史裁剪
- `smart_pruning` 下的 `Historical Context Summary (Pruned)`

也就是说，系统已经具备：

- `history pruning`
- `summary bridge`
- `context budgeting`

但当前它更像“上下文装配层的裁剪规则”，还不是“背景压缩产品层”。

### 2.2 记忆分层已经存在

位置：

- `crates/brain/src/agent/memory/mod.rs`
- `crates/brain/src/agent/memory/facade.rs`
- `crates/brain/src/agent/memory/episodic.rs`
- `crates/engram/src/agent_memory.rs`

当前已经具备：

- `ShortTermMemory` 作为热记忆
- `MemoryManager` 作为 `hot + engram` 治理层
- `EngramMemory` 作为长期持久层适配器
- `Fact / Session / Document / MultimodalMemory` 的正式契约

也就是说，系统已经有：

- 热态会话历史
- 长期事实/文档/会话恢复
- 关系型 fact memory

但当前“记忆”和“上下文背景层”的中间桥还不够明确。

### 2.3 SLM 战术层已经存在

位置：

- `crates/brain/src/agent/builder.rs`
- `crates/brain/src/agent/tactical.rs`
- `crates/brain/src/agent/reasoner.rs`

当前已经具备：

- `with_slm(...)`
- `GlobalTacticalOrchestrator`
- 在主脑 tool plan 前做 `derive_tactics(...)`
- `Proceed / Pivot / Halt` 三类 verdict

这意味着系统已经有了一个天然适合承接“背景压缩前置判断”的小模型入口。

这很重要，因为：

- 长会话 Agent 的背景压缩不应该每次都让主脑自己总结自己
- 用 SLM 做前置提炼和压缩判断，能降低成本，也能减少主 LLM 在长 session 下自我污染

---

## 3. 这条主线不重造什么

本文方案有一个硬约束：

**背景压缩必须建立在现有架构之上，而不是平行再造一个“第二上下文系统”或“第二记忆系统”。**

因此，明确不重造：

1. 不重写 `ContextManager`
2. 不绕开 `MemoryManager`
3. 不让应用层直接越过 `brain` 去写 `engram`
4. 不新造“后台人格”来维护前台背景
5. 不把压缩做成隐式黑箱

本文方案只允许新增：

- 背景压缩的正式对象模型
- 背景压缩策略层
- SLM 前置压缩器
- 背景层的 trace / witness / panel 读面

---

## 4. 产品级背景压缩的目标语义

产品级背景压缩不是“把旧对话记录缩成一句话”，而是维护一个稳定、分层、可持续更新的背景层。

### 4.1 最终需要形成的 4 层背景

#### Layer A: Core Persona Layer

负责保存不会频繁变化的 Agent 身份基线：

- Agent 的身份设定
- 说话风格和边界
- 用户与 Agent 的关系基线
- 长期安全与治理提示

这层更新频率最低，必须非常稳定。

#### Layer B: User Relationship Layer

负责保存用户相关的稳定关系信息：

- 用户偏好
- 称呼方式
- 长期主题
- 生活习惯
- 持续性的情绪/关系线索

这层来自：

- `Fact memory`
- 审核后的关系事实
- 历史高价值会话提炼

#### Layer C: Ongoing Session Layer

负责保存当前 session 的动态背景：

- 最近持续主题
- 当前进行中任务
- 近几轮未完成目标
- 最近情绪或关系状态变化
- 当前桌面环境/文档环境中的关键上下文

这层是本文最核心的背景压缩对象。

#### Layer D: Raw Recent Window

负责保存最近原始背景窗口：

- 最近 `N` 轮用户/助手消息
- 最近工具结果
- 最近文档/网页/截图/多模态观察

这层不做强摘要，而是作为原始短窗口保真层。

### 4.2 设计原则

最终的 prompt 背景应是：

`稳定人格层 + 稳定关系层 + 当前 session 压缩层 + 最近原始窗口`

而不是：

`所有历史原文尽量往里塞`

### 4.3 本文要达到的效果上限

本文主线如果真正做成，效果上限不应只停留在：

- 旧历史被总结成一段文本

而应逐步逼近：

- 稳定的自我背景层
- 稳定的用户关系层
- 稳定的当前会话状态层
- 在裁剪原始历史后仍保持连续人格和连续关系的前台体验

因此本文默认要达成的是：

- `持续背景层`
- `低漂移`
- `低幻觉写回`
- `可持续单 session`

而不是：

- `单次总结质量还不错`

---

## 5. 为什么 SLM 应该参与

长会话 Agent 下，背景压缩如果完全由主 LLM 每轮自己做，会有三个风险：

1. 成本高
2. 漂移大
3. 容易把当前 hallucination 再写回背景

而现有系统已经有 `SLM tactical pre-pass`，所以更合理的方式是：

### 5.1 SLM 的角色

SLM 不负责最终回答用户。

SLM 负责：

- 判断是否触发背景压缩
- 对最近原始历史做低成本结构提炼
- 生成候选背景更新草案
- 给主 LLM 或治理层提供 `Proceed / Pivot / Halt` 风格建议

### 5.2 为什么这能降低 hallucination

因为产品级背景压缩的风险不只是“总结得不够好”，而是：

- 把临时猜测写成稳定背景
- 把当轮幻觉升级成长期事实
- 把助手的错误理解永久化

SLM 作为前置压缩器的价值不在“更聪明”，而在：

- 更便宜地做预筛
- 更稳定地做结构化抽取
- 限制主脑在长 session 中反复自我扩写

所以这里的原则是：

**SLM 先做候选压缩，主 LLM 不直接统治背景写回。**

### 5.3 SLM 在本方案中的正式地位

这里必须单独收口：

- `SLM` 很有价值
- 但 `SLM` 不能成为背景压缩主线成立的前提

原因是：

- 并不是每个 Agent 部署环境都会具备本地 `SLM`
- 如果背景压缩只有“有 SLM 才能工作”，它就会变成高成本、低普及率的附属功能
- 这不符合本文“产品级主线”的目标

因此本方案正式采用两层口径：

1. `默认主线`
   - 无 `SLM` 也必须成立
   - 使用规则、阈值、session 生命周期与 evidence policy 产出基础 `BackgroundCompressionDecision`
2. `增强主线`
   - 有 `SLM` 时复用 `SLM tactical pre-pass`
   - 进一步降低 hallucination 写回
   - 改善关系层和高风险 durable promotion 的判断质量

所以更准确地说：

- `SLM` 是背景层质量控制的关键增强件
- 但不是背景压缩能力存在与否的硬前提

这也意味着：

- 没有 `SLM` 时，系统仍必须能：
  - `Skip`
  - `RefreshSessionLayer`
  - `RejectCandidate`
- 有 `SLM` 时，再把：
  - `PromoteRelationshipFact`
  - `RewriteWholeEnvelope`
  - 更细的高风险拦截
  做得更稳

---

## 6. 现有系统上的正式挂接点

### 6.1 挂接点一：`ContextManager`

位置：

- `crates/brain/src/agent/context.rs`

新增职责：

- 接收正式的 `BackgroundEnvelope`
- 在 `build_context(...)` 时不只处理 `history`
- 还要处理：
  - `persona background`
  - `relationship background`
  - `session background`
  - `recent raw window`

换句话说：

- `ContextManager` 仍然负责最终 prompt 装配
- 但不再只装配“历史裁剪结果”
- 而是装配“背景层 + 最近窗口”

### 6.2 挂接点二：`MemoryManager`

位置：

- `crates/brain/src/agent/memory/facade.rs`

新增职责：

- 负责背景压缩对象的 authority
- 区分：
  - `inflight session background`
  - `durable promoted background`
- 决定哪些背景更新只属于当前 session
- 决定哪些可以写入长期事实/文档/多模态记忆

也就是说，背景压缩不能直接越过 `MemoryManager` 去写 durable backend。

### 6.3 挂接点三：`EngramMemory`

位置：

- `crates/engram/src/agent_memory.rs`

新增职责：

- 为背景对象提供 durable metadata contract
- 支持：
  - session background snapshots
  - relationship summaries
  - background revision provenance
  - source span / evidence refs

但 `engram` 不负责发明“该压什么”，只负责 durable authority。

### 6.4 挂接点四：`SLM Tactical Orchestrator`

位置：

- `crates/brain/src/agent/tactical.rs`
- `crates/brain/src/agent/reasoner.rs`
- `crates/brain/src/agent/builder.rs`

新增职责：

- 在主回答前或会话 checkpoint 后，判断是否需要背景压缩
- 产出：
  - `Proceed`: 当前不压缩
  - `Pivot`: 推荐更新某个背景层
  - `Halt`: 当前候选内容不应进入背景

这里不建议新造一个平行的 `BackgroundCompressorAgent`。

更好的方式是：

- 复用现有 tactical SLM 入口
- 让背景压缩成为 Tactical Orchestrator 的正式子能力之一

同时必须明确：

- 没有真实 `SLM` backend 时
- 也必须有规则驱动的 `background verdict` 逻辑
- 不能让 `SLM` 成为单点前提

### 6.5 挂接点五：Foreground Runtime Checkpoint

位置：

- `crates/brain/src/agent/foreground_runtime.rs`

当前已有：

- session restore
- session persistence
- checkpoint 机制

新增职责：

- 在每轮完成、任务完成、长会话阈值触发时
- 触发背景压缩评估
- 把当前 session 中的高价值变化写入 `BackgroundEnvelope`

---

## 7. 正式对象模型

建议新增正式对象：

### 7.1 `BackgroundEnvelope`

最顶层背景对象，至少包含：

- `persona_layer`
- `relationship_layer`
- `session_layer`
- `recent_window_summary`
- `revision`
- `source_refs`
- `quality_signal`
- `compression_reason`
- `updated_at`

### 7.2 `SessionBackgroundState`

负责当前 session 的动态背景，至少包含：

- `active_topics`
- `open_loops`
- `recent_emotional_state`
- `ongoing_goals`
- `workspace_focus`
- `pending_followups`

### 7.3 `BackgroundCompressionDecision`

显式建模压缩决策：

- `Skip`
- `RefreshSessionLayer`
- `PromoteRelationshipFact`
- `RewriteWholeEnvelope`
- `RejectCandidate`

### 7.4 `BackgroundEvidenceRef`

明确背景压缩不是拍脑袋：

- 来源消息 ID
- 来源工具结果 ID
- 来源文档/记忆 ID
- 来源时间范围
- 可信度

---

## 8. 背景压缩的触发条件

必须显式化，不允许靠隐式 heuristics 藏在代码里。

推荐触发条件分 4 类：

### 8.1 容量触发

- `ContextManager` 发现：
  - 历史裁剪率连续升高
  - `pruned_messages` 已明显增多
  - 最近多轮都在靠 summary bridge 活着

### 8.2 生命周期触发

- 一轮任务完成
- 长对话阶段结束
- 用户切换大主题前
- 会话即将 archive / suspend

### 8.3 关系变化触发

- 出现新的稳定偏好
- 出现明确的长期关系信号
- 出现明显的情绪/态度转折

### 8.4 工作区与多源上下文触发

对前台工作区和多源上下文场景来说，还要支持：

- 当前窗口主题明显切换
- 新文档/项目进入主工作集
- 当前工作上下文阶段发生变化

---

## 9. 背景压缩的执行流程

### 9.1 主流程

1. `Foreground Runtime` 完成一轮对话或阶段性 checkpoint
2. 收集：
   - 最近原始消息窗口
   - 当前 session state
   - recent tool results
   - recent memory writes
3. 把候选输入交给 `SLM tactical pre-pass`
4. 产出 `BackgroundCompressionDecision`
5. 若为 `Skip`
   - 不更新背景层
6. 若为 `RefreshSessionLayer`
   - 只更新当前 session background
7. 若为 `PromoteRelationshipFact`
   - 通过 `MemoryManager` 走正式 fact write path
8. 若为 `RewriteWholeEnvelope`
   - 重写当前背景总对象，但必须保留 revision/evidence
9. 更新后的 `BackgroundEnvelope` 回到 `ContextManager`，参与后续 prompt 构建

### 9.2 一条硬约束

**任何进入 durable layer 的背景内容，都必须带 evidence refs 和 revision 语义。**

否则它只是“模型觉得大概如此”，不能成为长期背景。

### 9.3 回退与降级矩阵

根据 `DEVELOPMENT_STANDARDS_AGENTOS.md`，这条主线不能只有“成功路径”，必须显式定义：

- fallback
- rollback
- recover
- structured degradation

因此背景压缩的正式回退矩阵至少包括：

#### A. 无 `SLM` 回退

- 条件：
  - 未配置 `SLM`
  - `SLM` backend 不可用
  - tactical timeout / error
- 处理：
  - 自动退回规则驱动 verdict
  - 允许：
    - `Skip`
    - `RefreshSessionLayer`
    - `RejectCandidate`
  - 默认不做激进 durable promotion

#### B. 候选背景不可信回退

- 条件：
  - evidence 不足
  - 与现有 relationship/persona layer 冲突
  - 高风险推断无法自证
- 处理：
  - `RejectCandidate`
  - 保留现有 `BackgroundEnvelope`
  - 产出可观测拒写原因

#### C. 背景重写失败回退

- 条件：
  - 重写后的 envelope 为空、冲突或质量过低
  - 结构化字段缺失
- 处理：
  - 回退到上一个 `BackgroundRevision`
  - 当前轮只保留 `recent raw window`
  - 不污染 durable layer

#### D. Durable promotion 失败回退

- 条件：
  - `MemoryManager -> Engram` 写入失败
  - durable metadata 不完整
- 处理：
  - 只保留 session-local background
  - durable 背景维持旧 revision
  - 记录 warning / rollback 事件

#### E. 预算耗尽回退

- 条件：
  - 背景层本身过大
  - 当轮 injectors + recent window 已逼近上限
- 处理：
  - 优先保留：
    - `persona layer`
    - `relationship layer`
    - 最近原始窗口
  - 缩减：
    - `session background layer`
    - `recent window summary`
  - 再不够时退回保守 session summary，而不是强行塞满 prompt

#### F. Session 恢复失败回退

- 条件：
  - archived session 背景快照损坏
  - evidence refs 丢失
- 处理：
  - 退回：
    - durable relationship/persona
    - 最近原始消息窗口
  - 允许当前 session 在较弱背景层下继续工作
  - 不因单个恢复失败强制开新 session

一句话：

**背景压缩的正确行为不是“必须成功”，而是“失败时仍保证人格层、关系层和当前 session 可继续、可解释、可恢复”。**

---

## 10. Prompt 组装顺序

最终 prompt 不建议继续只按“system + injectors + history”理解。

对长会话 Agent，建议正式改写为：

1. `Core persona layer`
2. `Relationship layer`
3. `Session background layer`
4. `Dynamic injectors`
5. `Recent raw window`
6. `当前用户输入`

其中：

- `1-3` 属于背景层
- `4` 属于动态环境层
- `5-6` 属于当轮交互层

这能保证：

- 人格稳定
- 关系稳定
- 当前状态连续
- 最近细节仍保真

---

## 11. 产品读面与可观测性

产品级背景压缩必须进入读面，而不是只做内部策略。

至少要能在 trace / witness / panel 中看到：

- 本轮是否触发背景压缩
- 触发原因
- 更新了哪一层
- 保留了哪些核心槽位
- 哪些旧历史被压入摘要
- 哪些内容被拒绝写入长期背景
- 当前背景 revision
- 当前背景质量信号

建议新增读面字段：

- `background_compression_triggered`
- `background_compression_reason`
- `background_compression_decision`
- `background_revision`
- `background_layer_updated`
- `background_evidence_count`
- `background_quality_signal`
- `background_rejected_candidate_count`

这条要求与现有 tracing / witness 主线一致，不应另起一套 panel 语义。

---

## 11.1 产品级验收标准

如果这条线要被认为“达到本文目标”，验收不能只看：

- 压缩率
- 节省了多少 token
- summary 文本看起来是否顺

而必须优先看下面这些产品结果：

### A. Session 连续性

- `[~]` 长会话下不因上下文窗机械触发新 session
- `[~]` 即使历史被裁剪，当前 session 仍保留连续身份感

### B. 人格稳定性

- `[~]` 长会话后核心人格提示不明显漂移
- `[~]` 说话风格、称呼方式、关系基线不明显断裂

### C. 关系连续性

- `[~]` 最近关系状态在多轮压缩后仍可保持
- `[~]` 用户偏好不会因为多轮摘要被明显冲淡或误写

### D. 低幻觉写回

- `[x]` 候选背景与 durable 背景严格区分
- `[x]` 高风险关系信息不会因单轮推断被永久化
- `[~]` 关系事实/长期偏好写回必须可追溯到 evidence refs

### E. 可恢复性

- `[x]` session 恢复后仍能拿到最近背景层
- `[x]` 背景 revision 和来源在 trace / witness 中可解释
- `[x]` `SLM` 不可用时规则主线仍可工作
- `[~]` 背景重写失败时能回退到上一个 revision
- `[x]` durable promotion 失败时不会污染长期背景

一句话：

**这条线的真正验收标准是“背景连续性和 Agent 一致性是否成立”，不是“上下文裁剪是否触发”。**

---

## 12. 降低 hallucination 的正式策略

用户刚才提到一个关键点：

**有了 SLM 模块，同一个 session 下背景压缩不应该再那么容易发生高幻觉。**

这个判断方向是对的，但要把它写成明确策略：

### 12.1 禁止直接把模型自由总结写成长期背景

必须区分：

- `candidate background`
- `verified/promoted background`

### 12.2 背景写回必须走双阶段

1. `SLM` 做候选提炼
2. `MemoryManager` / fact policy 决定是否晋升 durable

### 12.3 长期关系信息必须偏保守

对于：

- 用户偏好
- 情感状态
- 人格关系
- 长期承诺

必须更保守地写入，而不是只因一轮对话就永久化。

### 12.4 近期状态层可以更积极

对于：

- 当前任务
- 最近主题
- 当前文档/桌面上下文

可以更积极地更新，因为它们本来就是 session 级背景。

---

## 13. 分阶段实施方案

### Phase B0：显式产品收口 `状态: [x]`

目标：

- 明确“背景压缩 != KV 压缩”
- 把它从上下文裁剪专题升级为产品主线专题

交付：

- `[x]` 本文档
- `[x]` `DEVELOPMENT_STANDARDS_AGENTOS.md` 正式关联
- `[x]` `README_ZH.md` 正式入口
- `[x]` 与 `BENSHU_PERSONAL_JARVIS_ROADMAP_ZH.md` 做显式交叉引用

### Phase B1：对象模型落地 `状态: [x]`

目标：

- 落地 `BackgroundEnvelope`
- 落地 `SessionBackgroundState`
- 落地 `BackgroundCompressionDecision`
- 落地 `BackgroundEvidenceRef`

交付位置：

- `crates/brain/src/agent/memory/*`
- `crates/brain/src/agent/context.rs`

交付口径：

- `[x]` 有正式类型，不再靠临时 message summary 伪装
- `[x]` 背景层对象可单测
- `[x]` revision / evidence 字段进入正式 contract

### Phase B2：在 `ContextManager` 中接入背景层 `状态: [x]`

目标：

- 把当前 `smart_pruning` 升级为“背景层 + 最近窗口”结构

交付：

- `[x]` `build_context(...)` 支持背景层装配
- `[x]` 历史裁剪不再只是临时 log
- `[~]` prompt 组装顺序已形成“背景层 + recent history/recent raw window”的主线，但 `recent raw window` 仍主要由现有 history 预算与裁剪逻辑承接

### Phase B3：复用 SLM tactical pre-pass `状态: [x]`

目标：

- 建立“规则主线 + 可选 SLM 增强”双轨 verdict
- 让 SLM 在存在时增强背景压缩候选判断

交付位置：

- `tactical.rs`
- `builder.rs`
- `reasoner.rs`

交付口径：

- `[x]` 无 `SLM` 时规则 verdict 可独立成立
- `[x]` `BackgroundCompressionDecision` 已形成规则主线 + 可选 `SLM` 增强 verdict
- `[x]` 候选背景更新不再直接依赖主 LLM 自我总结
- `[x]` `RejectCandidate` 已可拦住高风险背景写回

### Phase B4：接入 `MemoryManager` `状态: [~]`

目标：

- 背景层写回遵守现有 memory authority
- 不新造平行 durable path

交付：

- `[x]` session background vs durable promotion 的显式治理
- `[x]` 候选背景 / durable 背景双阶段模型成立
- `[~]` durable promotion 带 evidence refs

当前已落地：

- session background 已经通过 `MemoryManager.persist_background_envelope(...)` 进入正式 authority path
- `PromoteRelationshipFact` 已经不再直接“变成真相”，而是以 `Protected + PendingReview` 的 durable candidate 进入现有 review 流程
- 无 `MemoryManager` 或 durable 写入失败时，会保守退回 `session-local`，不会污染长期背景
- `archive_session / recover_session` 已开始把背景快照生命周期写回 `background_envelope.metadata`
  - 当前已能看到：
    - `background_session_lifecycle_state=archived|recovered`
    - `background_session_archive_reason`
    - `background_session_retention_until_ms`
    - `background_session_recovered_from`
    - `background_session_last_recovered_at_ms`

### Phase B5：观测与产品读面 `状态: [x]`

目标：

- trace / witness / panel 能解释背景压缩发生了什么

交付：

- `[x]` 背景层 revision 可见
- `[x]` 背景触发原因可见
- `[x]` 背景质量信号可见
- `[x]` 背景拒写计数可见

当前已落地：

- `run trace / stage trace / telemetry / panel` 已能看到：
  - `background_revision`
  - `background_quality_signal`
  - `background_decision`
  - `background_total_attempts`
  - `background_skip_count`
  - `background_reject_count`
  - `background_refresh_session_count`
  - `background_promote_relationship_count`
  - `background_rewrite_count`
  - `background_session_persistence_status`
  - `background_durable_promotion_status`
  - `background_review_reason`
  - `background_review_source`
- `BackgroundEnvelope` 已开始执行预算裁剪：
  - persona / relationship / session layer 的列表与文本长度有硬上限
  - `recent_window_summary` 与 `source_refs` 已有有限窗口
  - runtime metadata 已能看到：
    - `background_budget_compaction_applied`
    - `background_source_ref_count_pre_cap`
    - `background_source_ref_count`
- `engram` 已补入 background-aware session audit 读面：
  - archived / recovered session 的 `session_audit` 现在会带：
    - `session_background_present`
    - `session_background_lifecycle_state`
    - `session_background_revision`
  - `EngramMemory.retrieve_session(...)` 已会从最新 `session_audit` 回填背景生命周期与 revision
  - `HybridSearchStats / runtime metadata` 已能看到：
    - `engram.session.background_archive_count`
    - `engram.session.background_recovery_count`

### Phase B6：前台工作区场景优化 `状态: [~]`

目标：

- 对前台工作区场景引入：
  - 当前窗口主题
  - 当前工作模式
  - 最近用户情绪与关系状态
  - 活动中的工作主题 / 交互主题
- 把当前主线从“主要由消息窗口驱动”继续推进到“多源 backend 背景输入共同形成 active background”
- 对标更强背景系统的能力上限：
  - 不只看对话文本
  - 而是让工具结果、文档/网页/截图理解结果、任务状态、memory recall、workspace focus 一起参与背景形成

交付：

- `[x]` 已接入 `workspace/source-level backend signals`
  - 当前已通过 `tactical` 规则主线，从现有 `Message.metadata / source_path / source_collection / tool_name / media_preprocess_source_ref`
    中提炼 `workspace_focus`
  - 当前已覆盖：
    - 活动窗口标题 / 前台应用
    - source path / source collection / tool name / media preprocess source ref
- `[~]` 工作主题与关系状态能持续跨轮保留
  - 当前 `session layer` 已开始在旧背景之上做保守继承，不再每轮都完全覆写
  - 当前 `tactical` 已会补充并渲染：
    - `working_mode`
    - `interaction_theme`
  - 当前 `session layer` 已增加活跃背景衰减语义：
    - 旧 `workspace_focus / working_mode / interaction_theme`
      会在长期不再相关时退出 `active background`
    - 短期 follow-up 则仍会保留最近桌面主题
  - 当前 `relationship layer` 已会与既有 `user_preferences / long_term_topics` 做保守合并
  - 当前 `Foreground Runtime` 已有跨轮回归，验证：
    - `workspace_focus` 不会因下一轮缺少显式 backend 信号而被冲掉
    - 关系偏好线索不会因 session background 刷新而丢失
    - 最近桌面主题在短期 follow-up 中能够持续
    - 旧桌面主题在多轮无关新主线后会自然淡出
  - 但前台长会话下“同一个 Agent 的工作主题 / 关系状态持续稳定”仍需继续做产品回归
- `[~]` 文档/网页/截图/多模态结果、任务状态、memory recall 已开始进入统一背景对象
  - 当前 `session layer.backend_contexts` 已开始正式承接：
    - `Web context`
    - `Artifact context`
    - `Collection context`
    - `Multimodal context`
    - `Multimodal route`
    - `Task state`
    - `Memory recall`
  - 当前这些来源已经不只是附属 metadata，而是会被写进 `BackgroundEnvelope.session_layer`
  - 当前 `session layer.backend_context_records` 也已开始作为 typed backend records 落地：
    - `kind`
    - `value`
    - `source`
  - 当前 `session layer` 也已开始承接更明确的 typed backend objects：
    - `retrieved_memory_objects`
    - `web_session_objects`
    - `artifact_session_objects`
    - `task_session_objects`
    - `tool_session_objects`
    - `multimodal_session_objects`
  - 当前 `BackgroundEvidenceRef` 也已开始从 `source_url / source_path / media_preprocess_source_ref / retrieved_from / tool_name`
    中自动提炼更稳定的 provenance
  - 当前主线已不再只靠文本化 `backend_contexts`，但仍缺更大范围真实日志下的多源 backend object 家族收口与统一任务集
- `[ ]` 前台 Agent 在长对话下仍表现为“同一个持续存在的 Agent”

### Phase B7：产品级稳定性验收 `状态: [~]`

目标：

- 把本文目标从“设计成立”推进到“长会话产品效果成立”
- 对标更强长会话 Agent 的稳定性标准：
  - 在真实复杂任务里，背景层不是“勉强不坏”
  - 而是长期保持像“同一个 Agent 在持续工作”
  - 即使经过多轮压缩、任务切换、工具调用和会话恢复，仍能维持人格、关系、称呼、任务主线和背景边界

交付：

- `[~]` 长 session 压测下人格连续性通过
- `[~]` 多轮关系状态保持测试通过
- `[x]` session 恢复后背景层回放通过
- `[~]` 背景写回 hallucination 回归测试通过
- `[~]` 长会话背景窗体验测试中，不因上下文窗频繁重开 session

当前已落地：

- 已有 `rewrite` 路径回归，验证多轮背景重写后 `persona layer` 的核心锚点不会被冲掉
- 已有 `reject` 路径回归，验证高风险候选会被拒写，不污染现有背景和 durable facts
- 已有长会话回归，验证背景层刷新不会机械触发新 session
- 已有 `100+` 轮长会话回归，验证 `persona / relationship` 基线在持续背景刷新下仍然保持
- 已有多轮关系/工作区回归，验证 `workspace_focus` 与用户偏好不会因下一轮缺少显式 backend 信号而丢失
- 已有任务切换回归，验证 session background 能从旧工作区正确转移到新 backend focus，同时保留既有关系偏好
- 已有用户偏好多轮压缩回归，验证稳定 `relationship preference` 不会因多轮背景刷新被冲掉
- 已有长期关系状态回归，验证在多次任务切换后 `relationship_summary / user_preferences / long_term_topics` 仍不断线
- 已有最近桌面主题回归，验证短期 follow-up 下 `working_mode / interaction_theme` 会持续
- 已有旧桌面主题衰减回归，验证在多轮无关新主线后旧 `focused_review` 会退出 `active background`
- 已有人格风格/关系框架回归，验证在多主题、多工作区切换后 `speaking_style / relationship_frame` 仍保持稳定
- 已有称呼偏好回归，验证“以后叫我 xxx”这类稳定称呼偏好在长会话与任务切换后仍会保留
- 已有多源 backend context 回归，验证 `memory recall / web / collection / multimodal / task state` 在短期 follow-up 中可持续保留，并会在长期无关新主线后自然淡出
- 已有 typed backend record 回归，验证多源 backend 输入会同时进入 `backend_contexts` 与 `backend_context_records`
- 已有 typed backend object 回归，验证 `retrieved_memory_objects / web_session_objects / artifact_session_objects / task_session_objects / tool_session_objects / multimodal_session_objects`
  会与 `backend_context_records` 一起进入 `session layer`
- 已有多源 backend evidence refs 回归，验证 `source_url / source_path / media_preprocess_source_ref / retrieved_from`
  会进入更稳定的背景来源证据
- 已有规则拒写回归，验证“也许 / 先别记住 / 先不要写进长期偏好”这类临时想法不会覆盖现有稳定称呼偏好，也不会污染 durable facts
- 已有 hallucination 敏感写回固定任务包，验证多种中英混合的“临时想法 / 先别记住 / 不要写进长期记忆”表达都会稳定触发 `RejectCandidate`
- 已有多源 backend 任务包回归，验证在浏览器结果、记忆召回、桌面截图混合输入下，背景层仍能保住既有关系偏好并拦住临时称呼写回
- 已有更大范围 hallucination 产品任务包回归，验证在多轮 recall / screenshot 混合输入下，多种临时称呼/临时背景覆盖尝试仍会被拒写，不污染 durable facts
- 已有多源 backend 污染包回归，验证浏览器结果 / 文档解析 / memory recall / browser snapshot 等不同后端来源里的“临时称呼/临时长期偏好”都会被拒写
- 已有混合 `100+` 轮产品回归，验证在浏览器/文档/协作讨论/纯衰减尾段混合切换下，`persona / relationship / 称呼偏好` 仍稳定，且旧桌面审查主题会自然淡出
- 已有多源切换真实日志回归，验证在浏览器/截图/文档/recall 切换与恢复后，背景对象和来源证据仍能保留
- 已有长链路多次恢复回归，验证在浏览器 / 文档 / recall / 截图混合主线中经历多次 archive / recover 后，
  `persona / relationship / 称呼偏好 / 多源来源证据` 仍保持稳定

当前仍未完全收口：

- 更大范围真实产品日志/真实用户任务集下的长期稳定性回归
- 更大范围的 background write/reject hallucination 产品数据集
- 更高强度的“多源输入 + 多主题切换 + 长期恢复”混合任务集

---

## 14. 当前最推荐的下一步

如果以当前代码状态继续推进，推荐顺序已经收口为：

1. 继续补 `B6` 的 backend background object 家族统一建模，重点转向更大范围真实日志下的 object 收口与统一消费
2. 继续补 `B7` 的真实产品日志/真实用户任务集回归，尤其是多次恢复和更长任务链
3. 把 hallucination 产品任务包继续扩成更大范围真实日志数据集，覆盖更多后端来源与表达变体

当前硬约束仍保持不变：

1. 不允许把“summary 文本生成成功”误判为“持续背景层已经成立”
2. 任何阶段都必须先定义 fallback / rollback / recover 语义，再允许进入主路径

---

## 14.1 文件级开发落点

这一节回答的不是“原则是什么”，而是：

**这条主线当前主要落在哪些文件里，以及后续扩展优先应继续落到哪些文件里。**

### A. `brain` 层

#### 1. `crates/brain/src/agent/memory/mod.rs`

职责：

- 新增背景层正式对象模型
- 统一导出背景压缩相关 trait / enum / DTO

建议新增：

- `BackgroundEnvelope`
- `PersonaBackgroundLayer`
- `RelationshipBackgroundLayer`
- `SessionBackgroundState`
- `RecentWindowSummary`
- `BackgroundCompressionDecision`
- `BackgroundEvidenceRef`
- `BackgroundQualitySignal`
- `BackgroundRevision`

要求：

- 这些类型必须是正式 contract，不允许只在 `context.rs` 里临时拼装
- 至少支持 `serde` 序列化，方便 trace / witness / durable metadata

#### 2. `crates/brain/src/agent/context.rs`

职责：

- 把现有 `ContextManager` 从“history pruning”升级为“背景层 + 最近窗口”的正式装配器

建议改动：

- 给 `ContextManager` 增加背景对象输入
- 新增独立的背景装配步骤：
  - `assemble_persona_layer(...)`
  - `assemble_relationship_layer(...)`
  - `assemble_session_background(...)`
  - `assemble_recent_window(...)`
- 把当前 `smart_pruning` 输出从临时 log 升级为 `RecentWindowSummary`

要求：

- 不破坏现有 `ContextInjector` 契约
- `build_context(...)` 最终顺序要稳定可预测
- 本地/远端 provider 的预算策略继续保留，但背景层不应被当成普通历史消息处理

#### 3. `crates/brain/src/agent/memory/facade.rs`

职责：

- 让 `MemoryManager` 成为背景层 authority 的正式路由器

建议新增：

- `store_session_background(...)`
- `retrieve_session_background(...)`
- `promote_background_fact(...)`
- `rewrite_background_envelope(...)`
- `reject_background_candidate(...)`

要求：

- 明确区分：
  - `session-local background`
  - `durable promoted background`
- durable 写入必须仍通过 `MemoryManager`
- 一致性失败时必须保留 rollback / warning 语义

#### 4. `crates/brain/src/agent/tactical.rs`

职责：

- 复用现有 `TacticalOrchestrator` 做背景压缩候选判断

建议新增：

- `BackgroundCompressionInput`
- `BackgroundCompressionVerdict`
- `derive_background_tactics(...)`
- `derive_background_tactics_rule_based(...)`

建议 verdict：

- `Skip`
- `RefreshSessionLayer`
- `PromoteFact`
- `RewriteEnvelope`
- `Reject`

要求：

- 不新造第二套 SLM runtime
- 继续复用当前 tactical SLM backend
- 背景压缩 verdict 和 tool-plan verdict 可共存，但语义要明确分开
- 没有 `SLM` backend 时，规则 verdict 必须可独立工作

#### 5. `crates/brain/src/agent/reasoner.rs`

职责：

- 把背景压缩判断接进主前台推理循环

建议新增：

- 在主回答完成后或关键阶段点调用背景压缩评估
- 对高风险候选写回插入明确的 `Thought / Trace` 事件

要求：

- 不让背景压缩阻塞主回答过久
- 必须允许 `Skip`
- 主脑不应直接把自由文本总结永久写回背景

#### 6. `crates/brain/src/agent/foreground_runtime.rs`

职责：

- 把背景压缩挂到 session checkpoint 和 foreground 生命周期上

建议新增：

- `maybe_refresh_background(...)`
- `checkpoint_background_state(...)`
- `emit_background_compression_stage(...)`

建议触发点：

- 每轮回答完成后
- 长会话阈值触发时
- 任务收束时
- session archive 前

要求：

- 背景压缩不能破坏现有 session persistence
- 触发逻辑要可 trace、可关闭、可降级
- 背景刷新失败时必须保留旧 revision 并继续前台对话

#### 7. `crates/brain/src/agent/background_runtime.rs`

职责：

- 承接低优先级背景重写或二次整理

建议用途：

- 对已经产生的 session background 做后台整理
- 对低优先级背景 revision 做延迟优化

要求：

- 不抢前台主路径
- 遵守现有 background throttle / hygiene / sleep consolidator 节奏

### B. `engram` 层

#### 8. `crates/engram/src/agent_memory.rs`

职责：

- 给背景层提供 durable metadata contract 和 durable authority 落点

建议新增：

- background envelope 的 metadata 序列化
- background evidence refs 的 durable 记录
- background revision provenance
- session background snapshot 存储

要求：

- `engram` 不发明背景含义
- 只负责 durable storage / retrieval / auditability
- durable 写入失败必须回传明确 rollback / warning 语义

#### 9. `crates/engram/src/store.rs`

职责：

- 为背景对象提供显式持久化 collection / lifecycle 支持

建议新增：

- session background 的 retention/pruning policy
- archived session 对应背景快照的生命周期规则

要求：

- 背景层不能变成“神秘 dump”
- 必须继续遵守 retention / archive / recovery 规则
- session background 必须有容量上限，不能因长期不关 session 而无限膨胀

#### 10. `crates/engram/src/retriever.rs`

职责：

- 在需要时把 durable background 拉回主路径

建议用途：

- archived session 恢复时回填背景层
- 长时间中断后拉回 relationship/session background

要求：

- 不把 background retrieval 和普通 RAG 检索混成一类
- 应优先把它视为“背景恢复”，而不是“搜索结果”

### C. 可观测性与产品读面

#### 11. `crates/brain/src/agent/*trace*` 与 runtime trace 相关落点

职责：

- 记录背景压缩决策和背景 revision

建议新增 metadata：

- `background_compression_triggered`
- `background_compression_reason`
- `background_compression_decision`
- `background_revision`
- `background_quality_signal`

#### 12. `apps/gateway` / `apps/panel`

职责：

- 暴露用户可理解的背景状态，而不是只暴露内部字段

建议读面：

- 当前背景层是否刚更新
- 当前 session 是否进入背景压缩保护
- 当前人物关系/最近状态是否已稳定

要求：

- 不要把它做成开发者专用面板
- 最终产品需要“可解释但不过载”的用户读面

---

## 14.2 测试与验收落点

背景压缩这条线不应该只靠手工聊天验证，必须补正式测试。

### A. 单元测试

建议新增到：

- `crates/brain/src/agent/context.rs`
- `crates/brain/src/agent/memory/*`
- `crates/brain/src/agent/tactical.rs`

至少覆盖：

- `[x]` 背景层装配顺序稳定
- `[~]` `RecentWindowSummary` 不会覆盖核心 persona layer
- `[ ]` `BackgroundCompressionDecision` 序列化/反序列化稳定
- `[~]` evidence refs 不会丢
- `[x]` 高风险候选会被 `Reject/Halt`

### B. MemoryManager 集成测试

建议新增到：

- `crates/brain` 现有 memory facade 测试区域

至少覆盖：

- `[x]` session background 只写 hot 层时的行为
- `[~]` durable promotion 成功时 hot + engram 一致
- `[~]` durable promotion 失败时 rollback 成立
- `[x]` session recover 后 background 能回填
- `[x]` 无 `SLM` 时规则 verdict 仍可驱动 session background 刷新

### C. Engram 集成测试

建议新增到：

- `crates/engram/src/agent_memory.rs` 的测试区

至少覆盖：

- `[x]` background envelope metadata round-trip
- `[x]` evidence refs round-trip
- `[x]` archived session 的背景快照可恢复
- `[x]` retention/pruning 不会误删仍活跃的 session background

### D. Foreground Runtime / 长会话测试

建议新增到：

- `crates/brain/src/agent/foreground_runtime.rs` 或 harness 测试

至少覆盖：

- `[x]` 长会话下背景层会刷新但不强制新 session
- `[x]` 背景刷新后最近原始窗口仍保真
- `[x]` 插话/中断后背景层不乱写
- `[x]` 回答完成后的 checkpoint 不会阻塞主路径超时
- `[x]` tactical / SLM 失败时前台仍可继续，且沿规则主线退化

### E. 产品验收任务集

建议单独形成一组真实任务集：

- `[x]` 长会话闲聊/协作跨 100+ 轮后人格是否稳定
- `[x]` 用户偏好在多轮压缩后是否持续
- `[x]` 长期关系状态是否不断线
- `[x]` 当前任务切换后 session background 是否正确转移
- `[x]` 前台工作区场景下，最近桌面主题是否持续

---

## 14.3 主施工顺序（已基本走完）

这条主线当前的主施工顺序已经基本按下面路径走完，后续扩展仍应遵守同样边界：

1. 先改 `memory/mod.rs`
   - 定义正式对象模型
2. 再改 `context.rs`
   - 让上下文能接收背景层
3. 再改 `tactical.rs`
   - 先让规则主线产出背景压缩 verdict，再接 SLM 增强
4. 再改 `foreground_runtime.rs`
   - 让主路径开始触发背景刷新
5. 再改 `memory/facade.rs`
   - 接上 authority 与写回治理
6. 再改 `engram/agent_memory.rs + store.rs`
   - 补 durable layer
7. 最后接 trace / witness / panel

一句话：

**先把“背景对象 + 上下文装配 + SLM/规则判断”立起来，再把 durable、读面和产品回归补齐。**

---

## 15. 一句话总结

**BenShu 需要的不是新的 memory system，而是一套挂接在 `ContextManager + MemoryManager + EngramMemory + SLM tactical pre-pass` 之上的产品级 Agent 背景信息窗压缩主线。它的目标不是省 token 本身，而是让 Agent 在同一个 session 下持续保持人格、关系、任务状态、工具结果上下文和最近工作主题，尽量像同一个持续存在的个体，而不因为背景窗逼近上限被迫频繁“重新做人”。**
