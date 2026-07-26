# BenShu 写作工具书名权威链路升级计划

## 背景

真实写作回归中反复出现过两类问题：

- 质量门能拦住明显不合格的书名，但不一定能稳定修出好书名。
- 书名判断散落在文本边界、合同补丁、typed contract gate、命名策略里，容易出现“修了但没生效”。

本计划不新增第二套命名系统，而是把现有能力收敛成一条权威链路：

```text
用户需求 / 合同草案 / typed patch
  -> 当前故事证据归一化
  -> BookTitleCandidate 候选池
  -> BookTitleDecision 评分和拒绝原因
  -> TitlePatch 写回合同
  -> typed_contract_gate 最终确认
  -> 面板展示可确认合同
```

## 不做什么

- 不在 gateway/chat 写小说命名规则。
- 不按 Qwen、Gemma 或某个模型写专属修复。
- 不为某个题材写固定书名模板。
- 不为了测试固定某个书名。
- 不让文本 boundary gate 扩张成语义审美门。

## Phase 0：基线审查

状态：已完成。

当前生产代码中，书名候选和质量判断集中在：

- `crates/builtin-tools/src/tool/writing/naming/title.rs`
- `crates/builtin-tools/src/tool/writing/naming/title_policy.rs`
- `crates/builtin-tools/src/tool/writing/creation_contract/patch.rs`
- `crates/builtin-tools/src/tool/writing/typed_contract_gate.rs`

旧的 `select_best_book_title_candidate` 已从生产代码移除。

## Phase 1：TitlePatch 成为合同书名修复入口

状态：已完成。

`metadata_repair.rs` 不再直接决定最终书名，而是构造 `TitlePatch` 并委托：

- `TitlePatch::best_title_candidate_for_draft`
- `select_book_title_candidate_decision`
- `local_book_title_candidates_from_story_evidence`

这样 LLM 给出的泛泛候选不会直接通过；如果当前合同证据足够，系统可以从当前故事证据里修出更具体的候选。

## Phase 2：文本 boundary gate 只管脏输出

状态：已完成。

`generated_gate.rs` 不再负责书名审美、剧情依据、读者钩子判断。它只保留：

- 输出污染
- 格式残片
- 法律/交付合同误识别
- 数字档位丢失
- 章节计划结构异常

书名质量统一交给 `naming/title_policy.rs` 和 `typed_contract_gate.rs`。

## Phase 3：统一 BookTitleDecision 反馈

状态：已完成。

`typed_contract_gate.rs` 使用 `BookTitleDecision.reasons` 作为书名失败的主要诊断来源。

metadata title repair 失败时也复用同一套候选选择链路，避免出现多套 blocker 文案。

## Phase 4：增强本地候选生成，但不写固定模板

状态：已完成。

`local_book_title_candidates_from_story_evidence` 只基于当前合同证据生成候选，不依赖历史项目，也不使用题材固定模板。

候选来源优先使用：

- 当前故事关键地点
- 当前故事关键物件
- 制度漏洞
- 公开事件
- 主角爽点行动
- 终局兑现

并且补了通用残片过滤，避免把“主角追查证据”截成“角追查证”这类句中残片。

## Phase 5：章节标题和书名分权

状态：已完成。

书名来自全书合同、终局、大纲、世界观意象和读者爽点。

章节标题继续由章节写后总结/章节标题流程负责，不反向调用书名候选器。

## Phase 6：生命周期接入

状态：已完成代码侧收束。

坏书名由 typed contract gate 统一阻断。metadata repair 能走本地候选修复；无法形成合格候选时返回 NeedsRepair。

真实面板展示与体验验证留到 Phase 8。

## Phase 7：测试治理

状态：已完成。

测试只验证机制属性，不固定唯一书名：

- 泛泛候选不会被直接采纳。
- 当前故事证据能生成更具体候选。
- 候选必须包含故事锚点、行动或 payoff。
- 句中残片不会成为书名候选。

## Phase 8：真实面板回归

状态：未执行。

用户要求本轮先不做真实面板回归，因此 Phase 8 暂停。

待测重点：

- 合同草案里的书名是否来自剧情、大纲和结局。
- 坏书名是否能进入可解释的 repair，而不是裸露内部 blocker。
- 用户确认合同后，是否稳定进入第一章写作。
