# BenShu 与 Codex 风格 Hardness / Reflexion 对比说明

## 1. 文档目的

这份文档用于回答一个很具体的问题：

- “Codex 风格的 `hardness + Reflexion` 大致是怎么做的？”
- “它和 BenShu 当前文档里的 `hardness` 设计，有什么本质差异？”

这里的 `Codex` 不是指某个外部黑盒实现细节复刻，而是指一种更稳定、工程上更常见的参考模型：

- `hardness` 负责任务分级
- `Reflexion` 负责有限、自控的结果复查
- 两者都不应成为“任何任务都被拉进重处理链”的默认借口

---

## 2. Codex 风格的参考模型

## 2.1 一句话定义

`Codex-style hardness = 任务分级与执行门控；Reflexion = 仅在有充分理由时才启动的一次性或有限次复查机制。`

---

## 2.2 参考式 Hardness

在参考模型里，`hardness` 本身不是单一分数崇拜，而是一个多维判断层。

它通常综合这些信号：

- 任务是否简单直答
- 是否需要工具
- 是否需要外部验证
- 是否涉及高风险动作
- 是否涉及多步执行
- 是否涉及长上下文依赖
- 是否存在歧义、冲突目标或失败恢复成本

输出通常不是“难 / 不难”二元值，而是近似下面几档：

- `simple_direct`
- `medium_reasoning`
- `complex_planning`
- `high_risk_execution`
- `multimodal_understanding`

这些分级会影响：

- 是否走轻量上下文
- 是否启用深推理
- 是否先规划再执行
- 是否要先验证
- 是否允许进入更重的治理链

---

## 2.3 参考式 Reflexion

在参考模型里，`Reflexion` 不是默认行为，而是一个受门控的后处理阶段。

它的大致流程是：

1. 先完成第一版回答或执行结果
2. 检查是否存在明确问题
3. 只有问题足够具体时，才进入一次修订

典型触发条件：

- 高风险回答
- 复杂多步任务
- 最终交付明显没答到点上
- 工具结果没有被整合
- 自信度低且用户代价高

典型不应触发场景：

- 简单闲聊
- 简单工具成功返回
- 简单图片描述
- 一句话能结束的前台短问答

推荐约束：

- `Reflexion` 最多 1 次或 2 次
- 必须有明确退出条件
- 不能把“上轮不完美”自动等同于“继续复盘”

---

## 2.4 参考式失败补救

失败补救在参考模型里应独立于 `Reflexion`。

更合理的拆法是：

- `execution_gate`
  - 这轮是否真的要调用工具
- `review_gate`
  - 是否要对结果做复查
- `recovery_gate`
  - 失败后是重试、换路、解释原因，还是复盘一次

换句话说：

- 工具失败 ≠ 必须 Reflexion
- 模型输出一般 ≠ 必须 Reflexion
- 上轮出错 ≠ 下一轮自动升级重策略

---

## 3. BenShu 文档里的 Hardness

根据现有文档，BenShu 的 `hardness` 更偏产品原则与系统治理，而不只是运行时难度分级。

主要依据：

- [BENSHU_HARDNESS_DESIGN_PRINCIPLES.md](/home/biubiuboy/BenShu/docs/secondary/BENSHU_HARDNESS_DESIGN_PRINCIPLES.md)
- [BENSHU_LOCAL_FAST_CHAT_REFACTOR_PLAN_ZH.md](/home/biubiuboy/BenShu/docs/BENSHU_LOCAL_FAST_CHAT_REFACTOR_PLAN_ZH.md)
- [BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md](/home/biubiuboy/BenShu/docs/secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md)

---

## 3.1 你们文档里的核心定义

在 [BENSHU_HARDNESS_DESIGN_PRINCIPLES.md](/home/biubiuboy/BenShu/docs/secondary/BENSHU_HARDNESS_DESIGN_PRINCIPLES.md) 里，`hardness` 的一句话定义是：

`认知主权 + 可验证执行 + 非破坏性默认 + 风险显式化 + 可恢复治理`

这说明 BenShu 文档里的 `hardness`，重点在：

- truth first
- verification first
- non-destructive by default
- explicit risk
- recovery first

也就是说，它首先是：

- 产品治理原则
- 执行真实性原则
- 恢复与证据原则

而不是单纯“复杂度高低分流器”。

---

## 3.2 你们文档里的执行面含义

在 [BENSHU_LOCAL_FAST_CHAT_REFACTOR_PLAN_ZH.md](/home/biubiuboy/BenShu/docs/BENSHU_LOCAL_FAST_CHAT_REFACTOR_PLAN_ZH.md) 中，`hardness` 被进一步落成：

- `fast/full` 前置 gate
- `lite hardness`
- `full hardness`

核心立场是：

- 快通道不是绕过 hardness
- 而是 hardness 通过后才允许进入轻量执行面

这其实是一个很好的方向，说明你们文档已经意识到：

- `hardness` 不该总是重
- 轻任务也要有 gate
- 但轻任务不该被重治理链污染

---

## 3.3 你们文档里的运行时解释

在当前 AgentOS 执行计划和运行时代码中，运行时口径已经比较接近现实：

- `hardness` 被拆进 pre-flight 风险和复杂度治理
- 轻请求默认跳过重 pre-flight
- 复杂 / 执行 / 媒体请求按需打开

这表明文档主张本身已经比较合理：

- 轻请求不该走重链
- 媒体请求要按需打开治理
- `hardness` 不再是“每轮都做满”

---

## 4. 两者的核心差异

## 4.1 定位不同

`Codex` 参考模型里的 `hardness` 更偏：

- 运行时任务分级
- 策略选择
- 代价控制

而 BenShu 文档里的 `hardness` 更偏：

- 产品治理
- 证据链
- 风险显示
- 恢复与主权

一句话：

- `Codex-style hardness` 更像“推理与执行门控”
- `BenShu hardness` 更像“全系统治理原则”

---

## 4.2 Reflexion 的地位不同

在参考模型里：

- `Reflexion` 只是一个可选后处理器
- 它从属于 `hardness` 之后的策略决策
- 它必须是有限、受控、带退出条件的

而 BenShu 当前运行时实际表现里：

- `Reflexion` 容易被失败补救链放大
- `retry_count > 0`
- `last_error != None`
- `complexity.score > 0.8`

这些条件叠加后，会让用户体感接近：

- 很多不该重处理的任务，也被拖进了 Reflexion

这不是文档原则的问题，而是当前实现门槛太宽。

---

## 4.3 失败补救耦合度不同

在参考模型里：

- 工具失败
- 回答不完整
- 风险高

这三类问题应分别进入不同 gate。

而 BenShu 当前代码链里更像：

- 失败补救
- 复杂度升级
- Reflexion
- intervention

相互耦合较深。

结果就是：

- 简单任务一旦失败一次
- 或者出现一次格式异常
- 就容易被整体推入更重的链路

---

## 5. 对比表

| 维度 | Codex 风格参考模型 | BenShu 文档中的 hardness |
|---|---|---|
| 核心定位 | 运行时任务分级与执行门控 | 产品治理、真值、恢复、证据主权 |
| 主要目标 | 选对策略、控制代价、减少误用重推理 | 保证真实执行、风险显式化、可恢复治理 |
| 是否强调证据链 | 有，但不是第一核心 | 是第一核心之一 |
| 是否强调轻重分流 | 强 | 强，且已有 `lite/full hardness` 口径 |
| Reflexion 地位 | 可选的有限复查器 | 文档中不是硬核心，但运行时实现已深度介入 |
| 失败后默认动作 | 优先重试 / 降级 / 报错解释，必要时再复查 | 当前实现里容易直接进入 Reflexion / intervention 组合链 |
| 多模态简单问答 | 应优先直答 | 文档原则支持直答，但实现仍未完全收干净 |

---

## 6. 当前最值得吸收的 Codex 参考点

如果 BenShu 想借鉴“Codex 风格”的部分，最应该吸收的不是某个神秘算法，而是以下几个边界原则：

### 6.1 把 `hardness` 和 `Reflexion` 拆开

不要让：

- 复杂度判断
- 失败补救
- 自我复查

在实现层里变成同一个漏斗。

应该显式拆成：

- `difficulty_gate`
- `execution_gate`
- `review_gate`
- `recovery_gate`

---

### 6.2 Reflexion 只能是“有限复查”，不能是“默认修正通道”

建议规则：

- 简单问答禁用
- 简单多模态直答禁用
- 工具成功后禁用
- 只有复杂任务、高风险结果或明显未答到点时才启用
- 并且默认最多 1 次

---

### 6.3 “失败过一次”不应自动等于“升级 Reflexion”

当前运行时最容易放大误触发的点之一，就是：

- `retry_count > 0`

一旦失败重试就被视作更复杂，从而升级到更重策略。

参考模型里更稳的做法是：

- `retry_count` 只是恢复信号
- 不应直接作为 `Reflexion` 的充分条件

---

## 7. 反过来，Codex 参考模型也该向 BenShu 学什么

BenShu 文档里的优势也很明显，尤其是这几条：

- `Truth First`
- `Verification First`
- `Recovery First`
- `Non-Destructive By Default`

这些是很多单纯“任务分级系统”容易做弱的地方。

也就是说：

- Codex 风格参考模型更适合做运行时调度
- BenShu 的 hardness 原则更适合做长期产品治理

最理想的结合方式不是二选一，而是：

- 用 `Codex-style gating` 管执行面
- 用 `BenShu hardness principles` 管系统边界

---

## 8. 一句话结论

`Codex 风格 hardness + Reflexion` 更像“任务难度门控 + 有限结果复查”；`BenShu 文档里的 hardness` 更像“真值、风险、恢复、主权”的全系统治理原则。`

两者并不冲突。

真正的问题不是 BenShu 的文档原则错了，而是当前运行时把：

- complexity
- retry
- last_error
- Reflexion
- intervention

耦合得过宽，导致简单任务也容易掉进重链。

如果后续要继续优化，最优解不是放弃 hardness，而是：

`保留 BenShu 的 hardness 原则层，按 Codex 风格把运行时 gate、Reflexion 与 recovery 拆开。`

---

## 9. 基于当前测试结果的修复计划

这一节不是抽象建议，而是根据最近几轮真实 `/api/chat`、多模态、工具交付测试里已经暴露出来的问题制定的执行计划。

当前已确认的现象包括：

- 简单图片理解会被复杂度链和 intervention 干扰
- 多模态直答曾出现伪 `<|tool_call>` 泄漏
- 多模态直答曾出现英文“我将调用工具”的程序性占位话术
- `retry_count > 0` 会把简单任务也推向 `Reflexion`
- `last_error` 会把失败补救和 `Reflexion` 直接耦合
- 工具成功之后，后处理层仍可能把交付重新拖回重链

所以修复目标不是“关掉 hardness”，而是把 `hardness / Reflexion / recovery` 重新拆回边界清晰的三层。

---

## 9.1 总体目标

目标分三条：

1. 简单任务稳定留在轻链路
2. 复杂任务保留重治理能力
3. `Reflexion` 从“泛化补救器”收回“有限复查器”

一句话就是：

`轻任务不被重链污染，复杂任务仍保留治理与恢复能力。`

---

## 9.2 Phase 1：先止血，收紧 Reflexion 触发边界

这是优先级最高的一阶段，因为它直接影响现在的用户体感。

### 目标

不再让以下场景轻易进入 `Reflexion`：

- 简单前台问答
- 简单多模态描述/看看/读图
- 工具成功且结果明确的请求
- 仅因上轮失败一次就被升级的请求

### 建议改动

1. 去掉 `retry_count > 0 => Reflexion` 的直接升级逻辑  
当前它只是恢复信号，不该成为升级为 `Reflexion` 的充分条件。

2. 给 `Reflexion` 增加显式禁止条件  
至少包括：
- `simple_direct`
- `multimodal_direct`
- `tool_success_already_observed`

3. `last_error` 不再直接等于注入 `Reflexion` intervention  
应先判断：
- 错误是不是可重试
- 错误是不是格式层
- 错误是不是工具层
- 错误是不是用户真正会感知到的质量问题

### 验收标准

- 简单文本问答不会进入 Reflexion
- 简单图片描述不会进入 Reflexion
- 工具成功后不会再因为收尾不理想被强制复盘

---

## 9.3 Phase 2：把 hardness gate 拆成独立的 4 个 gate

这是结构性修复。

### 目标

不再让一个 `complexity` 信号同时决定：

- 是否重推理
- 是否重治理
- 是否复查
- 是否失败补救

### 建议拆成 4 个 gate

1. `difficulty_gate`
- 判断任务是简单、中等、复杂

2. `execution_gate`
- 判断是否需要工具 / specialist / 外部验证

3. `review_gate`
- 判断是否需要 `Reflexion`

4. `recovery_gate`
- 判断失败后是重试、换路、解释失败，还是复查一次

### 建议原则

- `difficulty_gate` 不直接调用 `Reflexion`
- `execution_gate` 不等于 `review_gate`
- `recovery_gate` 不得默认跳到 `Reflexion`

### 验收标准

- 代码中能明确看到这四层边界
- 运行 trace 中能看出“为什么走 execution / review / recovery”
- 用户侧不再感受到“所有异常最后都进 Reflexion”

---

## 9.4 Phase 3：为多模态和简单工具请求建立硬豁免区

这是这次测试最直接暴露出来的主战场。

### 目标

给以下场景建立“硬豁免区”：

- 单图前台描述
- 简单 OCR
- 简单图文问答
- 已成功返回结果的单工具请求

### 建议规则

如果同时满足：

- 单轮前台请求
- 输入短
- 媒体数少
- 非高风险
- 无显式执行型目标

则：

- 禁止 `FractalMeltdown`
- 禁止 `SwarmAdvisory`
- 禁止 `StatusRecap`
- 禁止 `Reflexion`

直接进入：

- `multimodal_direct`
或
- `tool_success_direct_finalize`

### 验收标准

- “请描述这张图里有什么”稳定自然语言交付
- “请读出这张图里的字”稳定自然语言交付
- 不再出现内部委派说明、伪工具标记、程序性占位话术

---

## 10. 当前 Crate 边界与宿主责任

这一节用于说明：在最近一轮重构之后，哪些 hardness 规则已经正式进入 `benshu-hardness`，哪些能力仍然应该留在 `brain` 或其他宿主 crate 中。

这不是“理想图”，而是当前代码已经落地后的真实边界。

---

### 10.1 已进入 `benshu-hardness` 的内容

当前 [crates/hardness/src](/home/biubiuboy/BenShu/crates/hardness/src) 已经承担以下规则面：

- `complexity`
  - 运行时复杂度分数模型
  - 语义复杂度分析接口

- `task_complexity`
  - 旧 `TaskComplexity` 数据契约
  - `sanitize_task_complexity(...)`

- `preflight`
  - extended pre-flight 分级
  - complexity estimator / jit distillation / auto-stepdown gate

- `media`
  - `simple_media_understanding`
  - `frontstage_single_image_turn`
  - frontstage 媒体注入清理

- `strategy`
  - 初始 `ReasoningStrategy` 决策
  - 显式文生图首轮判定
  - 失败后是否追加 reflexion 恢复提示

- `reflexion`
  - Reflexion 升级 gate
  - Reflexion review gate
  - critique 文本协议解析

- `recovery`
  - `tool-first recovery` gate

- `execution`
  - 这轮是否必须要求执行型工具回复

- `failure`
  - 工具失败后是否应入队 failure analysis

- `intervention`
  - `error_reflexion`
  - `fractal_meltdown`
  - `swarm_advisory`
  - `recursive_handover`
  - `status_recap`
  - internal complexity probe 识别

一句话：

`凡是“运行时治理门槛”的东西，当前已经尽量收回到了 hardness crate。`

---

### 10.2 仍应留在 `brain` 的内容

仍留在 `brain` 的，不再是“忘记抽离的规则”，而主要是以下两类宿主责任。

#### 1. Provider / runtime 绑定实现

例如：

- [meta.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/meta.rs) 中的 `LlmComplexityEstimator`
- [reasoner.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/reasoner.rs) 中的 provider 调用
- [executor.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/executor.rs) 中的工具执行与 hook

这些代码直接依赖：

- `Provider`
- `ToolDefinition`
- runtime event bus
- hook engine
- security / governance context

如果把它们硬搬进 `hardness`，会造成依赖方向反转，让 `hardness` 从治理 crate 退化成新的运行时中心。

所以这部分应该保留在 `brain`。

#### 2. Prompt 文案与运行时编排

例如：

- intervention 注入的具体 prompt 文本
- reasoner 中 critique 请求如何发起
- executor 中错误如何展示给用户
- foreground runtime 如何组装 execution seed / trace / task

这些属于“执行与交付的宿主行为”，不是 hardness 规则本身。

换句话说：

- `hardness` 决定“该不该”
- `brain` 决定“怎么做”

---

### 10.3 当前推荐边界

现在比较健康的分层应该是：

- `benshu-hardness`
  - 只负责规则、分类、门控、契约、sanitize

- `benshu-brain`
  - 负责 provider 调用、工具执行、prompt 注入、事件发射、结果交付

- `gateway / panel / telemetry`
  - 负责配置输入、运行观测、面板展示、trace 解释

这意味着后续不应该继续做两种错误方向：

1. 把 provider / tool / runtime 编排继续塞进 `hardness`
2. 再把新的 gate 逻辑写回 `reasoner / executor / intervention`

---

### 10.4 当前阶段性结论

截至这轮重构，可以比较负责任地说：

- `hardness` 相关的核心规则面，已经基本独立成 crate
- `brain` 中残留的部分，主要是宿主责任，不再是大量遗漏的 hardness 规则
- 后续优化重点应从“继续盲目抽离”转向“维护边界稳定 + 补充 trace / diagnostics / 文档”

所以接下来更值得做的，不再是无限拆模块，而是：

- 补清晰的 crate 边界说明
- 给运行 trace 增加 hardness evidence
- 用真实聊天回归验证这些 gate 是否真的改善前台体验

---

## 11. 文档与代码对齐状态

这一节用于回答一个更现实的问题：

`上面这些“修复计划”，现在代码到底做到了哪一步？`

这里按三档标记：

- `已完成`：代码里已经有明确实现，且本轮重构中已验证编译/测试通过
- `部分完成`：已经有结构或局部实现，但离文档目标还有明显缺口
- `未完成`：当前仍主要停留在文档建议，没有形成对应运行时能力

---

### 11.1 Phase 1：收紧 Reflexion 触发边界

#### 1. 去掉 `retry_count > 0 => Reflexion` 的直接升级逻辑

状态：`已完成`

已经完成的部分：

- 当前 [strategy.rs](/home/biubiuboy/BenShu/crates/hardness/src/strategy.rs) 已去掉初始策略里的 `retry_count > 0 => Reflexion`
- 当前 [reflexion.rs](/home/biubiuboy/BenShu/crates/hardness/src/reflexion.rs) 不再只因为 `retry_count > 0` 就升级，而是要求 `retry_recovery_eligible`
- 当前 [failure.rs](/home/biubiuboy/BenShu/crates/hardness/src/failure.rs) 已引入失败分类，只有被识别为 `quality_error` 的重试才允许触发 `RetryRecovery`

这说明：

- `retry_count` 已经从“直接升级条件”收回成“恢复信号”
- 现在是否升级，还要经过失败分类 gate

#### 2. 给 Reflexion 增加显式禁止条件

状态：`部分完成`

已经完成的部分：

- 多媒体输入会阻止 `Reflexion` 升级
- `simple_media_understanding` 会阻止 `Reflexion`
- `should_run_reflexion_review(...)` 已有媒体与简单媒体豁免

还没完成的部分：

- 文档里提到的 `tool_success_already_observed` 还没有被做成显式 gate
- 简单文本问答虽然已有部分轻链路保护，但还没有一个统一的 `simple_direct` review 禁止开关

#### 3. `last_error` 不再直接等于 Reflexion intervention

状态：`部分完成`

已经完成的部分：

- 当前 [intervention.rs](/home/biubiuboy/BenShu/crates/hardness/src/intervention.rs) 已不再使用 `has_last_error`
- 当前 [failure.rs](/home/biubiuboy/BenShu/crates/hardness/src/failure.rs) 已新增 `FailureClass`
- 现在只有 `quality_error_detected` 才会触发 `error_reflexion`

还没完成的部分：

- 当前错误分类还是启发式文本分类，不是完整的运行时错误 taxonomy
- `quality / transport / resource / execution` 还没有贯穿到全部 recovery / finalization 决策

#### 这一阶段的总体判断

状态：`部分完成`

结论：

- 结构上已经把 `Reflexion` 从散乱逻辑收成了显式 policy
- 但文档里最强的行为主张，还没有完全落实

---

### 11.2 Phase 2：拆出独立的 gate

状态：`大体完成`

当前已经存在的 gate 面：

- `complexity`
- `preflight`
- `strategy`
- `reflexion`
- `recovery`
- `execution`
- `failure`
- `intervention`
- `media`

这说明文档里提出的：

- `difficulty_gate`
- `execution_gate`
- `review_gate`
- `recovery_gate`

虽然不完全按这个名字落地，但在结构上已经基本拆开。

还没完成的部分：

- 运行 trace 里还没有形成一套完整的 hardness evidence 输出
- 目前更多是“代码边界已拆”，不是“诊断面完全可见”

#### 这一阶段的总体判断

状态：`已完成（结构层） / 部分完成（观测层）`

---

### 11.3 Phase 3：为多模态和简单工具请求建立硬豁免区

状态：`部分完成`

已经完成的部分：

- `simple_media_understanding`
- `frontstage_single_image_turn`
- 多媒体请求不再轻易升级到 `Reflexion`
- 简单媒体场景不会再触发 `FractalMeltdown / StatusRecap`
- 执行型工具回复 requirement 已对简单多模态文档理解做了豁免

还没完成的部分：

- 文档要求的“直接进入 `multimodal_direct` / `tool_success_direct_finalize`”仍未完全被保证
- 真实多模态前台交付是否稳定自然语言结束，这条还需要再做真实聊天回归确认
- 简单 OCR / 图文问答虽然方向已对，但前台最终交付层并没有被正式独立出来

#### 这一阶段的总体判断

状态：`部分完成`

---

### 11.4 Phase 4：失败补救先分类，再恢复

状态：`部分完成`

已经完成的部分：

- failure analysis 是否入队，已经有显式 gate
- tool-first recovery 是否触发，已经有显式 gate
- reflexion critique 是否有效，已经有显式协议解析
- 当前 [failure.rs](/home/biubiuboy/BenShu/crates/hardness/src/failure.rs) 已新增初步 `FailureClass`
- 当前 `retry recovery` 只会对 `quality_error` 生效
- 当前 `error_reflexion` intervention 只会对 `quality_error` 生效
- 当前工具执行错误后的 `Reflexion recovery prompt` 也只会对 `quality_error` 生效

但还没完成的关键目标是：

- 还没有正式且更细的 `transport_error / format_error / tool_error / quality_error` taxonomy
- 还没有完整的“错误类型 -> 恢复动作 -> finalization”统一映射
- provider / runtime / finalization 还没有统一消费同一份失败分类证据

所以这一阶段当前应视为：`部分完成`

---

### 11.5 Phase 5：独立 finalization 层

状态：`部分完成`

当前虽然已经做过一些局部止血，例如：

- 伪 `tool_call` 文本的局部拦截
- 多模态直答里的程序性占位话术压制
- 某些工具成功交付的合成兜底
- 当前 [finalization.rs](/home/biubiuboy/BenShu/crates/hardness/src/finalization.rs) 已新增显式 finalization fallback 决策入口
- 当前 [reasoner.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/reasoner.rs) 已开始在“LLM 无自然语言交付”出口消费该入口

这说明我们已经不再完全停留在散点修复，但离文档里要求的“独立 finalization layer”还差一段距离。

仍未完成的目标包括：

- 统一拦截内部运行语义泄漏
- 统一把工具结果 / 视觉摘要 / 文档摘要转成最终用户交付
- 统一定义 finalization 优先级
- 让 foreground runtime / reasoner / executor / provider failure surface 共用同一套 finalization policy

所以这一阶段当前应视为：`部分完成`

---

### 11.6 当前总表

| 阶段 | 状态 | 说明 |
|---|---|---|
| Phase 1：收紧 Reflexion 触发边界 | 大体完成 | 最宽的两条直连已切掉，但更完整的 success/finalization gate 还没补完 |
| Phase 2：拆出独立 gate | 已完成（结构） / 部分完成（观测） | crate 边界已经成形，但 trace 证据面还没补足 |
| Phase 3：多模态 / 简单工具硬豁免区 | 部分完成 | gate 已有，最终交付仍需真实回归确认 |
| Phase 4：失败补救先分类再恢复 | 部分完成 | 已有初步失败分类，但还不是贯穿全链路的正式治理层 |
| Phase 5：独立 finalization 层 | 部分完成 | 已有显式入口，但还没形成全链路统一 finalization policy |

---

### 11.7 这意味着什么

当前最准确的描述不是：

- “文档已经全部实现了”

也不是：

- “只是把代码搬成了一个 crate”

而是：

`我们已经把 hardness 相关规则面成功抽成了独立 crate，并完成了大部分结构性收口；同时，Reflexion 触发边界、失败分类、以及 finalization 入口都已经开始落成。但文档里要求的“全链路统一治理层”还没有完全闭环。`

---

### 11.8 收口结论

如果把这篇文档当成“当前阶段是否已经可以收口”的判断依据，那么更准确的结论是：

- 可以说：`结构重构已经收口`
- 可以说：`最宽的 Reflexion 误触发已经明显收紧`
- 可以说：`失败分类与 finalization 已经从 0 变成显式治理入口`
- 不能说：`整套 hardness / recovery / finalization 已经彻底完成`

当前最适合的收口表述是：

`BenShu 现在已经完成了 hardness 治理层的主体拆分，并且把最关键的行为误触发点压下去了；后续剩余工作主要集中在“把失败分类和 finalization 从局部入口，继续推进成全链路统一政策”。`

---

## 9.5 Phase 4：把失败补救策略改成“先分类，再恢复”

当前失败补救最大的问题不是存在，而是太混。

### 目标

失败发生后，系统先做错误分类，再决定怎么恢复。

### 建议错误分类

1. `transport_error`
- 网络、provider timeout、媒体路径不可读

2. `format_error`
- 伪工具输出、模板标记泄漏、程序性占位话术

3. `tool_error`
- 工具执行失败、工具未配置、工具返回异常

4. `quality_error`
- 回答未答到点、总结不自然、漏掉关键结果

### 对应恢复动作

- `transport_error`
  - 优先重试或换输入形式
- `format_error`
  - 优先清洗 / 重写最终交付
- `tool_error`
  - 优先解释失败原因或换降级路径
- `quality_error`
  - 必要时才启用一次 Reflexion

### 验收标准

- trace 中能看到 error type
- 恢复动作和错误类型有稳定映射
- `Reflexion` 只出现在 `quality_error` 或高风险复杂任务里

---

## 9.6 Phase 5：把最终交付层独立出来

这是为了防止“前面都做对了，最后又被收尾逻辑毁掉”。

### 目标

建立单独的 finalization 层，优先确保用户看到的是最终结果，而不是内部运行语义。

### 必须拦截的内容

- 伪工具标记
- “我将调用某工具”
- “我会把任务交给 specialist”
- 纯程序性占位文本
- 内部术语泄漏

### 建议优先级

1. 有明确工具成功结果
  - 直接 synthesize 成最终交付
2. 有本地视觉/附件摘要
  - 直接用摘要生成用户可读答案
3. 上述都没有
  - 再回兜底失败说明

### 验收标准

- 用户永远看不到内部 tool call 模板
- 用户永远看不到内部 specialist 调度话术
- 多模态前台请求能自然结束

---

## 9.7 推荐实施顺序

建议严格按下面顺序推进：

1. 收紧 `Reflexion` 触发边界  
先止血，避免继续污染简单任务。

2. 为多模态和简单工具场景建立硬豁免区  
先把最影响体验的链路打通。

3. 拆分 `difficulty / execution / review / recovery` 四个 gate  
把结构重新摆正。

4. 重写失败补救分类  
把“错误类型”和“恢复动作”对齐。

5. 最后独立 finalization 层  
确保用户只看到结果，不看到内部运行噪音。

---

## 9.8 本文对应的现实判断

基于目前测试结果，BenShu 当前最需要修的不是：

- 模型更强一点
- prompt 更漂亮一点
- 多加几个 specialist

而是：

`把简单任务从错误的重治理链里救出来。`

只要这一步没做好：

- 工具越多，误触发越多
- 多模态越强，越容易被后处理打断
- 复杂系统能力越多，前台体验越容易变差

所以当前最关键的工程方向不是“继续加能力”，而是：

`先把 hardness / Reflexion / recovery 的边界收干净。`
