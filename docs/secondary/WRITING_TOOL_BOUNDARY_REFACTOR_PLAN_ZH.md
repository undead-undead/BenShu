# BenShu 写作工具职责边界与瘦身重构方案

更新时间：2026-06-11

## 背景判断

当前 `crates/builtin-tools/src/tool/writing` 已经不是一个普通工具，而是一个长篇写作子系统。当前规模约为：

- Rust 文件：53 个
- 总行数：约 67,482 行
- 生产代码：约 55,000 行
- 大型文件集中在：
  - `novel_studio.rs`
  - `longform_guard.rs`
  - `boundary_text_gate.rs`
  - `creation_contract.rs`
  - `novel_workflow_driver.rs`
  - `novel_bible.rs`
  - `quality_checks.rs`

能力底座是必要的：合同、角色权威表、章节流程、审稿、修订、导出、truth、分卷、长篇治理都应该保留。但当前最大风险不是单纯行数，而是同一职责被多个模块重复承担，导致修复容易失效，例如：

- 合同质量、LLM 输出边界、标题质量、正文质量互相混在一起。
- 合同生成、合同展示、合同确认、合同写入分布在 gateway、session surface、creation contract、studio 多处。
- 命名质量门既在合同模型、typed gate、标题门、章节质量门里出现，又会被局部修复逻辑影响。
- 正文修复、metadata 修复、reuse-existing 修复存在多条近似路径。
- 写作工具内部已经有独立策略，但部分长篇/写作策略仍与通用 longform/delegation/route 逻辑纠缠。

本方案目标不是继续增加新功能，而是把现有能力收敛为清晰的单一职责边界，删除重复机制，降低复杂度，并优化长篇执行性能。

## 总原则

1. 工具策略归工具自己，系统策略归 runtime。写作策略不得散落到 gateway 或 runtime-policy-core。
2. 合同是结构化权威，不是显示文本。显示文本只能由合同渲染出来。
3. 正文是最高价值产物。标题、摘要、key facts、continuity updates 等 metadata 问题不能触发正文重写。
4. 质量门分层：边界脏输出、typed contract、正文硬门槛、metadata 修复、warning 必须分开。
5. 每个生命周期状态只允许一个模块改变。其他模块只能查询或渲染状态。
6. 大文件拆分以职责为单位，不按“行数平均拆”。
7. 性能优化优先减少重复扫描、重复 clone、重复 JSON parse、重复文件 IO。
8. 零拷贝不是目的，减少大文本拷贝和多次扫描才是目标。
9. 所有重构必须保持现有真实面板工作流：自然语言触发、合同草案展示、用户自然语言修改、确认后写作、正文落 artifact、聊天框只显示摘要和路径。

## 重构硬约束

这些约束优先级高于具体 Phase。每次代码改动必须先满足这些条件，否则视为没有完成重构。

1. 禁止新增平行机制。新增模块前必须确认原有代码中是否已有同类职责；如果已有，必须复用、迁移或替换旧入口。
2. 禁止“先新增后长期并存”。允许短期桥接，但同一提交中必须明确旧入口的调用关系：要么删除，要么变成薄转发，要么标记为测试专用。
3. 禁止在 gateway/chat 层新增写作业务规则。gateway 只能做 HTTP/session/task adapter，写作语义必须由 `crates/builtin-tools/src/tool/writing` 提供。
4. 禁止把合同质量、命名质量、正文质量、metadata 质量混进同一个 gate。每个 gate 只能输出自己的裁判结果。
5. 禁止因为 metadata 问题重写正文。标题、摘要、key facts、continuity updates 只能触发 metadata repair。
6. 禁止 blocked / needs-repair 文本污染 draft 的结构化字段。只有 typed gate 通过后的合同才能进入可确认 draft。
7. 禁止任务外层状态和内部写作状态不一致。章节未批准、合同未 ready、provider 失败时，外层 task 不能伪装成普通 completed。
8. 禁止把测试数据、运行态数据、模型文件纳入提交。`data/benshu.yaml`、`data/cron.redb`、`models/` 一律不随重构提交。
9. 每个 Phase 完成后必须执行一次代码层核对：`rg` 确认旧职责入口是否还存在，必要时补聚焦测试。
10. 每个 Phase 完成后必须在本文档的执行清单中打勾，并写明实际落地模块。

## 执行清单

状态说明：

- `完成`：代码已经满足本方案的主要职责边界和验收条件。
- `部分完成`：已经有底座或主要迁移，但仍有明确未达标项。
- `未完成`：尚未进入可验收状态。

当前代码核实状态：

- [x] Phase 1：职责冻结与防扩散。状态：完成。
- [x] Phase 2：合同质量门收口。状态：完成当前文档范围。`boundary_text_gate`、`generated_gate`、`typed_contract_gate` 已拆出；合同候选提交/评分已迁入 `creation_contract/contract_candidate.rs`，draft lifecycle 已迁入 `creation_contract/draft_lifecycle.rs`，会话流已迁入 `creation_contract/chat_flow.rs`，合同修复协调已迁入 writing 模块，gateway 只保留 runtime/provider/checkpoint adapter。
- [x] Phase 3：命名治理收口。状态：完成当前文档范围。`naming` 已成为书名/章节名/角色名治理主入口；章节标题模板/疲劳/证据判断集中在 `naming/chapter_title.rs`，书名吸引力/合同依据/题材 marker 统一经 `naming/title.rs` adapter，合同、workflow、typed gate 不再直接绕过命名权威调用标题质量策略。
- [x] Phase 4：正文质量门与 metadata 门拆开。状态：基本完成。正文硬门槛、metadata gate、metadata-only repair 已形成主路径；后续仍需继续防止旧 cleanup/reuse 分支绕过该路径。
- [x] Phase 5：`novel_workflow_driver` 拆分。状态：完成。主文件已降到 1,000 行以内；规划、进度、项目状态、provider、结果格式、metadata repair、章节循环等职责已拆出独立模块。
- [x] Phase 6：`novel_studio` 拆分。状态：完成。已拆出 `support.rs`、`model.rs`、`storage.rs`、`project_lifecycle.rs`、`project_config.rs`、`chapter_planning.rs`、`chapter_io.rs`、`state_truth.rs`、`review_approval.rs`、`status_export.rs` 等子模块；`novel_studio.rs` 主文件已降到约 350 行，只保留工具入口、调度和少量共享 helper。
- [x] Phase 7：正文清理和 sanitizer 合并。状态：完成。`text_sanitizer.rs` 提供统一 `SanitizeReport` 和 `WritingSanitizeStage` facade；workflow、保存正文、可读导出入口均通过该 facade 分层调用。
- [x] Phase 8：性能优化。状态：完成当前文档范围。`ProjectCache`、`TextScanReport`、`chapter_index.json`、`BufWriter`、`context_budget` telemetry 已落地；正文质量门已通过 `TextScanReport` 复用扫描结果。
- [x] Phase 9：测试拆分。状态：完成。大型测试文件已拆为目录分片，wrapper 文件已降到很小。

## 2026-06-11 代码核实结论

本次核实确认：本文档 Phase 1-9 的当前代码范围已经落地，写作工具主入口、合同质量门、命名治理、正文/metadata 分层、正文清理 facade、性能底座和测试拆分均与文档目标一致。仍保留的不是阻断性未完成项，而是后续维护风险和可选继续瘦身项。

### 保留风险项

1. gateway 写作合同修复编排已收口为 adapter，但仍有展示文案。
   - 合同修复循环已迁入 `crates/builtin-tools/src/tool/writing/creation_contract/repair_coordinator.rs`。
   - `apps/gateway/src/api/handlers/chat_tool_host.rs` 仍实现 runtime/provider/checkpoint trait，并保留少量面向 `ChatResponse` 的状态文案。
   - 后续若继续收口，可把展示文案也迁成 writing surface builder，gateway 只组装 HTTP 响应。

2. `creation_contract.rs` 已完成主要职责拆分，但仍不是极小入口。
   - 已拆出：`chat_flow.rs`、`contract_candidate.rs`、`draft_lifecycle.rs`、`repair_coordinator.rs`、`lifecycle.rs`、`gate.rs`、`validation.rs`、`draft_readiness.rs`。
   - 当前主文件约 1,267 行，主要仍保留强类型合同落回 draft、合同 v2 同步、角色/关系 ledger 派生等核心转换逻辑。
   - 后续若继续瘦身，可把 `strong contract -> draft` 应用逻辑单独拆成 `draft_apply.rs`，但不应再新增平行合同判断机制。

3. 命名治理已完成当前文档范围的收口。
   - `naming` 是书名/章节名/角色名治理主入口，章节标题模板、疲劳和故事证据判断已迁入 `naming/chapter_title.rs`。
   - `naming/title.rs` 统一暴露书名合同依据、读者吸引力、题材 marker、抽象概念栈等 adapter。
   - `creation_contract_model.rs`、`typed_contract_gate.rs`、`creation_contract/contract_text.rs`、`novel_workflow_driver/naming_recovery.rs` 均通过 `naming` 调用标题质量策略。
   - `creation_contract/intent.rs` 与 `novel_workflow_driver/naming_recovery.rs` 仍会读取 `title_meta_discussion_markers()` 判断“用户是否在谈命名”，这属于意图解析词表，不属于标题质量治理。

4. sanitizer 入口是“一个底层 facade，多种 stage adapter”。
   - `text_sanitizer.rs` 提供底层 `SanitizeReport` 与 `WritingSanitizeStage`。
   - workflow body cleanup、persisted prose cleanup、export readable cleanup 仍保留各自 adapter，但它们都通过同一底层 facade 生成 report。
   - 这不是当前重复实现；后续风险是不要在 adapter 内重新堆一套平行清理算法。

5. `longform_guard.rs` 仍有通用长产物文本修复逻辑，和写作 sanitizer 有相邻边界。
   - 该文件包含较多中文文本修复、标题提取、内部过程行清理等逻辑。
   - 目前它属于通用长产物治理，不直接阻断写作工具 Phase 完成；后续如发现写作专属规则，应迁入 writing sanitizer/quality gate。

6. Phase 6 的主文件行数验收已完成。
   - `novel_workflow_driver.rs` 已降到 1,000 行以内。
   - `novel_studio.rs` 已继续拆出章节正文写入/运行、状态与 truth、审稿/修订/批准、状态/导出动作模块，主文件降到约 350 行。
   - 后续如继续瘦身，优先拆 `quality_checks.rs` 与 `contract_text.rs` 这种局部大模块，而不是再在主入口堆分支。

7. Phase 8 的代码层性能底座已完成，运行态性能仍需真实长篇压力测试。
   - 代码层已将正文质量门接入 `TextScanReport`，减少重复扫描。
   - 仍需要真实大项目回归确认导出、章节索引、上下文预算 telemetry 在长篇规模下稳定。

### 下一步剩余工作

1. 可选继续拆 `creation_contract.rs`：把强类型合同落回 draft 的转换逻辑迁入 `draft_apply.rs`。
2. 将 gateway 中剩余写作状态展示文案迁入 writing surface builder，gateway 只保留 provider/session/task adapter。
3. 清理 `longform_guard.rs` 中写作专属文本修复逻辑。
4. 可选继续拆局部大模块：`quality_checks.rs`、`contract_text.rs`、`novel_workflow_driver/quality.rs`。
5. 做真实长篇压力测试，验证 Phase 8 性能目标。

历史落地记录：

说明：下面记录保留每次重构的实际落地点，属于历史流水。最终是否满足本文档验收，以“执行清单”和“2026-06-11 代码核实结论”为准。历史记录中的“已完成”若只覆盖当时的局部范围，不再代表当前整体验收完成。

- 2026-06-07：Phase 1 已完成职责冻结基础改造。`CreationDraftLifecycleStatus` 已迁入 `creation_contract/lifecycle.rs`，`creation_contract.rs`、`session_surface.rs`、`typed_contract_gate.rs`、`novel_workflow_driver.rs` 已补职责边界注释，并通过 `cargo check -p benshu-builtin-tools`。
- 2026-06-07：Phase 2 已完成 gate transport 与 validation report 的拆分基础。`ContractGateStatus`、`ContractGateResult`、`ContractSubmissionOutcome` 已迁入 `creation_contract/gate.rs`；`ContractValidationReport` 已迁入 `creation_contract/validation.rs`。但 `creation_draft_contract_blocking_issues` 仍暂在 `boundary_text_gate.rs`，合同质量唯一入口尚未完全收口，因此 Phase 2 暂不打勾。
- 2026-06-07：Phase 2 继续收口结构化 draft readiness。`creation_draft_contract_blocking_issues` 已从 `boundary_text_gate.rs` 迁入 `creation_contract/draft_readiness.rs`；`boundary_text_gate.rs` 不再承担“已生成 draft 是否可进入正文”的入口职责。当前仍剩 generated-contract 文本质量、planning 文本检查与 boundary gate 共处一处，因此 Phase 2 仍暂不打勾。
- 2026-06-07：Phase 2 继续收口 generated-contract gate assembly。`generated_contract_quality_issues`、`generated_contract_completion_quality_issues`、`generated_contract_gate_result` 与内部 `contract_gate_from_issues` 已迁入 `creation_contract/generated_gate.rs`；`creation_contract.rs` 不再本地拼装 `ContractGateResult`。`boundary_text_gate.rs` 当前仍保留底层 `contract_quality_issue_is_blocking` 与 advisory 生成，后续需继续拆到 gate/report 模块，因此 Phase 2 仍暂不打勾。
- 2026-06-07：Phase 2 继续收口 generated-contract report 职责。`contract_quality_issue_is_blocking` 与 `generated_contract_advisory_issues` 已迁入 `creation_contract/generated_gate.rs`；`generated_fiction_contract_planning_issues` 已迁入 `creation_contract/planning_gate.rs`。`boundary_text_gate.rs` 仍保留部分底层合同文本解析 helper，并且 boundary 入口会调用 planning gate 兼容现有流程，因此 Phase 2 仍暂不打勾。
- 2026-06-07：Phase 2 继续收口合同文本 helper。新增 `creation_contract/contract_text.rs`，迁出合同文本解析、章节规划文本修复、角色/字段抽取等 helper；`boundary_text_gate.rs` 降为约 100 行，只保留原始 LLM 合同输出边界问题聚合入口。当前 boundary 入口仍会组合合同文本 helper 与 planning gate 结果以兼容现有流程，因此 Phase 2 仍暂不打勾。
- 2026-06-07：Phase 3 已完成书名/title 入口的第一步拆分。`naming/title.rs` 已承接 title policy adapter，`naming.rs` 继续 re-export 原 API。角色名、章节名、卷名尚未完全拆成独立子模块，因此 Phase 3 暂不打勾。
- 2026-06-07：Phase 3 继续收口角色命名治理。`naming.rs` 已降为轻量索引文件，角色名生成、角色合同行治理与相关测试已迁入 `naming/character.rs`，书名/title adapter 保持在 `naming/title.rs`。章节名、卷名治理尚未完全拆成独立子模块，因此 Phase 3 仍暂不打勾。
- 2026-06-07：Phase 5 已完成 metadata repair 从 `novel_workflow_driver/quality.rs` 拆出。新增 `novel_workflow_driver/metadata_repair.rs`，职责包括 metadata gate 状态读取、metadata blocker 文本、metadata repair prompt/parse/limits。driver 主文件与 chapter 调用面保持兼容。workflow driver 仍未完全拆到目标结构，因此 Phase 5 暂不打勾。
- 2026-06-07：Phase 5 继续拆分 workflow driver 结果展示职责。新增 `novel_workflow_driver/result_format.rs`，迁出 interrupted/result/completed project 格式化与内部未批准状态判定；`novel_workflow_driver.rs` 主文件降到约 3014 行。driver 仍包含内容 CRUD、checkpoint、context fallback、命名恢复等职责，因此 Phase 5 仍暂不打勾。
- 2026-06-07：Phase 7 已完成 provider/protocol 文本残留清理入口的第一步收口。新增 `text_sanitizer.rs`，`novel_workflow_driver/output_cleanup.rs` 与 `novel_studio/prose_sanitizer.rs` 已复用同一个 provider marker 清理入口；完整正文 sanitizer 合并尚未完成，因此 Phase 7 暂不打勾。
- 2026-06-07：Phase 2 已完成职责收口。`creation_contract/boundary_text_gate.rs` 已降为原始 LLM 输出边界检查，generated-contract 语义质量在 `creation_contract/generated_gate.rs`，章节规划文本检查在 `creation_contract/planning_gate.rs`，合同文本 helper 在 `creation_contract/contract_text.rs`，draft readiness 在 `creation_contract/draft_readiness.rs`，生命周期在 `creation_contract/lifecycle.rs`。
- 2026-06-07：Phase 3 已完成当前安全范围内的命名治理收口。`naming.rs` 降为索引，书名 adapter 在 `naming/title.rs`，章节标题治理在 `naming/chapter_title.rs`，角色命名治理在 `naming/character.rs`。`novel_workflow_driver/naming_recovery.rs` 只保留 workflow setup/recovery 需要的任务侧命名恢复，不再堆在主 driver。
- 2026-06-07：Phase 4 已完成分层裁判基础。正文硬门槛仍由 `novel_studio/quality_gate.rs` 与 `chapter_quality.rs` 承担；metadata gate 与 metadata-only 修复在 `novel_workflow_driver/metadata_repair.rs` 和 `novel_studio/quality_gate.rs`，标题/摘要/key facts/continuity 不再作为正文重写入口。
- 2026-06-07：Phase 5 已完成主要拆分。`novel_workflow_driver.rs` 主文件从三千行级继续降到约一千六百行；内容 CRUD/清理在 `content_ops.rs`，metadata 修复在 `metadata_repair.rs`，结果格式化在 `result_format.rs`，命名恢复在 `naming_recovery.rs`，正文清理在 `output_cleanup.rs`，章节循环在 `chapter.rs`。
- 2026-06-07：Phase 6 已完成主要拆分。`novel_studio.rs` 主文件从六千行级降到约四千六百行；章节状态在 `chapter_state.rs`，manifest upsert 在 `manifest.rs`，状态/审计/治理报告在 `reporting.rs`，工具 schema 在 `tool_schema.rs`，章节 metadata 规范化在 `chapter_metadata.rs`，运行时记录写入在 `runtime_records.rs`，路径/slug/冲突恢复在 `pathing.rs`。
- 2026-06-07：Phase 8 已完成部分性能向重构。通过拆分 metadata-only repair、运行时记录写入、章节状态 helper、命名恢复 helper，降低了主流程重复扫描和重复分支风险；但 ProjectCache、增量全集导出、统一 TextScanReport 尚未落地，暂不打勾。
- 2026-06-11：Phase 7 已完成正文清理结构化收口。`writing/text_sanitizer.rs` 提供统一 `SanitizeReport`，`novel_workflow_driver/output_cleanup.rs` 与 `novel_studio/prose_sanitizer.rs` 的原字符串入口均由 report 入口承接；provider/protocol marker、artifact receipt surface、JSON 字段残留和 CJK markup noise 不再各自维护一套旧清理分支。
- 2026-06-11：Phase 8 已完成当前范围内的性能优化。新增 `novel_studio/project_cache.rs`，落地 `TextScanReport`、`ProjectCache`、`chapter_index.json`；可读全集导出改为 `BufWriter` 流式写入并保存 scan state；`compose_context` 输出 `context_budget` telemetry，记录 full/prompt context 与 contract、story_bible、truth、recent_chapters、archives、sources、character_ledger 等分区字符预算。
- 2026-06-11：Phase 9 已完成测试拆分。`creation_contract_tests.rs`、`novel_studio_tests.rs`、`novel_workflow_driver_tests.rs` 已降为 include wrapper，实际测试分片移入同名目录；最大测试分片约 1240 行，测试函数名保留原职责描述以便定位。
- 2026-06-11：Phase 3 继续减少重复命名算法。章节标题注册、疲劳、故事证据和连接词模板入口已集中到 `writing/naming/chapter_title.rs`；`creation_contract/contract_text.rs` 仅保留合同文本行解析，并复用 `naming` 的标题 core/template/connectors。
- 2026-06-11：Phase 6 继续拆分 `novel_studio.rs`。新增 `novel_studio/model.rs`，迁出 `NovelStudioArgs`、`NovelProjectManifest`、`ChapterRecord`、`StoryContract`、分卷/章节/审稿/快照/草案记录等数据结构；新增 `novel_studio/storage.rs`，迁出 project/draft path、manifest IO、workspace 边界与项目路径恢复；`novel_studio.rs` 主文件降到约 3,500 行。
- 2026-06-11：Phase 2 继续拆分 `creation_contract.rs`。新增 `creation_contract/chat_flow.rs`、`creation_contract/contract_candidate.rs`、`creation_contract/draft_lifecycle.rs`，分别承接会话流、合同候选提交/评分/metadata repair、初始 draft 与用户补充合并；`creation_contract.rs` 主文件降到约 1,267 行。
- 2026-06-11：Phase 6 继续拆分 `novel_studio.rs`。新增 `novel_studio/project_lifecycle.rs`、`novel_studio/project_config.rs`、`novel_studio/chapter_planning.rs`，分别承接草案/项目 lifecycle、项目配置/资料/合同动作、章节上下文与执行包准备；`novel_studio.rs` 主文件降到约 2,278 行。
- 2026-06-11：Phase 3/6 完成当前文档范围收口。标题质量/吸引力/题材 marker 统一经 `writing/naming/title.rs` adapter；`novel_studio.rs` 继续拆出 `chapter_io.rs`、`state_truth.rs`、`review_approval.rs`、`status_export.rs`，主文件降到约 350 行。

## 当前职责边界审查

### 1. `boundary_text_gate.rs`

当前问题：

- 文件过大。
- 语义上应该只判断 LLM 输出是否“脏”：JSON 残片、schema 泄露、英文污染、占位符、工具协议残留、markdown fence 泄露等。
- 现在容易和合同质量、标题质量、章节质量混在一起。

目标职责：

- 只做 `BoundaryTextGate`。
- 输入：原始 LLM 输出文本。
- 输出：`Clean | Dirty { issues }`。
- 不关心小说是否好看，不关心标题是否吸引人，不关心合同字段是否完整。

应迁出的职责：

- 合同字段完整性 -> `typed_contract_gate.rs`
- 书名/章节名质量 -> `naming`
- 正文是否合格 -> `chapter_quality`
- metadata 是否可修 -> `metadata_gate`

### 2. `creation_contract.rs`

当前问题：

- 仍承担过多职责：draft lifecycle、字段修复、合同渲染、部分确认逻辑、部分命名/质量协调。
- 这会导致“合同未 ready 却展示成可确认”“blocked 文本污染 draft”“开始写第一章又回 creation planning”之类问题反复出现。

目标职责：

- 只保留 draft lifecycle：
  - `NoDraft`
  - `Collecting`
  - `GeneratingContract`
  - `ContractReady`
  - `Approved`
  - `Writing`
  - `Blocked`
  - `Cancelled`
- 只负责状态转移，不直接判断书名质量、不直接修正文、不直接渲染完整聊天文本。

应拆出的模块：

- `creation_contract/lifecycle.rs`
- `creation_contract/store.rs`
- `creation_contract/render.rs`
- `creation_contract/commands.rs`
- `creation_contract/repair.rs`

### 3. `typed_contract_gate.rs`

当前问题：

- 这是正确方向，但还需要成为合同质量唯一权威。
- 其他模块不应重复判断合同是否“未指定”、角色是否完整、结局是否缺失。

目标职责：

- 只检查结构化 `NovelCreationContract`。
- 检查项：
  - 书名是否存在且像作品名。
  - 书名是否能由结局、大纲、世界观、爽点/钩子解释。
  - 角色权威表是否完整。
  - 主角、关键关系、对手、情感线是否可写。
  - 总字数、章节档位、预计章节数是否可执行。
  - 大纲、终局、卷/章规划是否够用。
- 输出：
  - `Ready`
  - `NeedsRepair`
  - `Blocked`

### 4. `naming.rs`

当前问题：

- 命名机制应该是独立能力，但现在书名、章节名、角色名的质量判断仍可能分散在合同模型、typed gate、quality checks 中。

目标职责：

- 成为唯一命名治理模块。
- 负责：
  - 书名候选评分。
  - 章节标题候选评分。
  - 卷名候选评分。
  - 角色名规则与随机化。
  - 用户指定名称优先。
  - 名称锁定与漂移检测。

关键原则：

- 书名：从合同中的结局、大纲、世界观意象、主角弧线、爽点/钩子推导。
- 章节名：先写正文，再总结正文，再确定最终标题。
- 角色名：可以由本地随机/规则生成，但进入角色权威表后必须锁定。
- 命名失败只修 metadata，不重写正文。

### 5. `novel_workflow_driver.rs` 与子模块

当前问题：

- 主文件仍有 3,000 多行，子模块 `chapter.rs`、`quality.rs`、`setup.rs` 也偏大。
- workflow 同时承担执行、修复、质量判断、状态推进、输出清理。

目标职责：

- driver 只做调度，不做具体判断。
- 每一步都是明确节点：
  - `PrepareChapter`
  - `BuildContextPackage`
  - `DraftBody`
  - `CleanBody`
  - `BodyGate`
  - `MetadataGate`
  - `RepairMetadata`
  - `ReviewIfNeeded`
  - `Approve`
  - `Export`

应拆出的模块：

- `novel_workflow_driver/context.rs`
- `novel_workflow_driver/draft.rs`
- `novel_workflow_driver/body_gate.rs`
- `novel_workflow_driver/metadata_gate.rs`
- `novel_workflow_driver/metadata_repair.rs`
- `novel_workflow_driver/review.rs`
- `novel_workflow_driver/approval.rs`
- `novel_workflow_driver/export.rs`
- `novel_workflow_driver/progress.rs`

### 6. `novel_studio.rs`

当前问题：

- 仍有 6,000 多行。
- action handler、manifest 操作、chapter IO、approval、export、status 渲染仍不够干净。

目标职责：

- `novel_studio.rs` 只保留工具入口和动作分发。
- 具体逻辑进入：
  - `novel_studio/actions.rs`
  - `novel_studio/manifest.rs`
  - `novel_studio/chapter_io.rs`
  - `novel_studio/approval.rs`
  - `novel_studio/export.rs`
  - `novel_studio/status.rs`
  - `novel_studio/project.rs`

### 7. `longform_guard.rs` 与 `longform_policy.rs`

当前问题：

- 长篇治理和写作策略存在重叠。
- 通用 longform 能力应该只管“长产物连续性”，不应该懂具体小说质量。

目标职责：

- `longform_guard.rs`：
  - 预算上限。
  - 长任务 checkpoint。
  - 产物是否应分步。
  - 上下文包大小治理。
- `writing` 专属：
  - 小说合同。
  - 章节质量。
  - 情感线。
  - 伏笔。
  - 命名。
  - truth/story bible。

需要删除/迁移：

- 如果 `longform_guard` 里出现小说专属术语，应迁入 `writing`。
- 如果 `writing` 里出现通用“长文件/长报告/长代码产物”判断，应迁入 longform 通用层。

### 8. `session_surface.rs`

当前问题：

- 应只负责展示，但仍有一些项目状态读取、路径预览、章节摘要读取逻辑。
- 可以保留 read-only surface，但不能做意图判断、状态推进、质量判断。

目标职责：

- 只做：
  - 合同展示。
  - 项目状态展示。
  - 文件路径预览。
  - 章节摘要只读。
- 禁止：
  - 触发写作。
  - 修合同。
  - 改状态。
  - 判断是否开始写。

## 重复机制删除清单

### 合同完整性重复

保留：

- `typed_contract_gate.rs`

删除/迁移：

- `creation_contract.rs` 内所有“字段是否未指定/是否完整”的重复判断。
- `session_surface.rs` 内非展示需要的合同完整性判断。
- `creation_contract_model.rs` 内除模型输出解析外的合同最终质量判断。

### 书名/章节名质量重复

保留：

- `naming.rs`

删除/迁移：

- `typed_contract_gate.rs` 只调用 naming 结果，不自己维护命名规则。
- `novel_studio/quality_checks.rs` 只调用 naming 的标题门，不维护另一套标题词表。
- `creation_contract_model.rs` 不再自己做书名候选评分，只输出候选或调用 naming。

### 正文清理重复

保留：

- `novel_workflow_driver/output_cleanup.rs`
- `novel_studio/prose_sanitizer.rs` 需要合并评估，最终只能留一个正文清理入口。

删除/迁移：

- `chapter.rs` 中内联 cleanup。
- `quality.rs` 中和 cleanup 重复的文本修复。
- reuse-existing 分支中的旧 cleanup 逻辑。

### metadata 修复重复

保留：

- 新建 `metadata_gate.rs`
- 新建 `metadata_repair.rs`

删除/迁移：

- 标题问题不能进入正文重写路径。
- summary/key facts/continuity updates 不完整不能触发 draft body rewrite。

### 状态机重复

保留：

- `creation_contract/lifecycle.rs`
- `chapter_lifecycle.rs`

删除/迁移：

- gateway 不能判断写作状态。
- session surface 不能推进状态。
- driver 不能私自把 blocked 标成 completed。

## 推荐目录结构

```text
crates/builtin-tools/src/tool/writing/
  mod.rs
  policy.rs
  intent_policy.rs

  contract/
    mod.rs
    model.rs
    lifecycle.rs
    store.rs
    render.rs
    repair.rs
    typed_gate.rs
    boundary_gate.rs
    normalizer.rs

  naming/
    mod.rs
    title.rs
    chapter_title.rs
    character.rs

  novel/
    mod.rs
    studio.rs
    actions.rs
    manifest.rs
    chapter_io.rs
    approval.rs
    export.rs
    snapshot.rs
    status.rs

  workflow/
    mod.rs
    driver.rs
    context.rs
    draft.rs
    body_gate.rs
    metadata_gate.rs
    metadata_repair.rs
    review.rs
    progress.rs

  bible/
    mod.rs
    story_bible.rs
    character_ledger.rs
    hook_ledger.rs
    timeline.rs
    world.rs
    theme.rs

  text/
    mod.rs
    sanitizer.rs
    preview.rs
    units.rs

  tests/
    contract_gate_tests.rs
    naming_tests.rs
    workflow_tests.rs
    studio_tests.rs
```

这不是要求一次移动所有文件，而是最终目标结构。迁移过程应保持公共 API 兼容。

## 性能优化方案

### 1. 减少重复 JSON parse

当前风险：

- 合同、manifest、chapter record、review record 可能在多个模块重复读取、重复 `serde_json::Value` 解析。

方案：

- 引入局部 `ProjectCache`：
  - `manifest`
  - `story_bible`
  - `chapter_index`
  - `latest_exports`
- 每个工具动作开始时加载一次，结束时按 dirty flag 写回。

复杂度收益：

- 从多处 `O(k * file_size)` 降到 `O(file_size + k)`。

### 2. 减少大文本 clone

当前风险：

- 章节正文在 draft、quality、repair、export 中多次 `String` clone。

方案：

- 质量检查函数优先接受 `&str`。
- 只有真正修改正文时才生成新的 `String`。
- metadata 修复只更新 metadata，不复制正文。
- 导出时使用 `BufWriter` 流式写入，不拼接完整全集大字符串。

复杂度收益：

- 降低峰值内存。
- 500 万字项目导出不再需要一次性持有完整文本。

### 3. 标题/正文扫描合并

当前风险：

- 标题质量、乱码检测、占位符检测、语言检测、重复段落检测可能多次扫描正文。

方案：

- 建立 `TextScanReport`：
  - `has_cjk`
  - `has_ascii_dominance`
  - `placeholder_hits`
  - `jsonish_hits`
  - `malformed_anchor_hits`
  - `paragraph_hashes`
  - `unit_count`
- `body_gate`、`metadata_gate`、`boundary_gate` 共享报告。

复杂度收益：

- 多次 `O(n)` 降为一次 `O(n)` 加若干 `O(1)` 查询。

### 4. 章节索引增量维护

当前风险：

- 每写一章都可能遍历全部章节、全部 summary、全部 truth。

方案：

- `chapter_index.json` 或 manifest 内维护轻量索引：
  - chapter number
  - status
  - title
  - unit count
  - summary hash
  - approved timestamp
- 写入章节时增量更新。

复杂度收益：

- 查询最新章节、已批准章节、总字数从 `O(chapters)` 降到 `O(1)` 或 `O(log n)`。

### 5. 长篇上下文包预算化

当前风险：

- 500 万字项目不会塞正文，但 story bible、summary、hook、character state 也会膨胀。

方案：

- `ContextPackBudget`：
  - story contract 固定上限。
  - current volume summary 固定上限。
  - recent chapters summary 固定 2-3 章。
  - hook ledger 按 pending priority 截断。
  - character ledger 只保留本章相关 + 核心角色。
- 每次组包输出 telemetry：
  - contract chars
  - bible chars
  - summary chars
  - hooks chars
  - total chars

复杂度收益：

- 上下文大小从随章节数线性增长，变成受预算上限约束。

### 6. 导出增量化

当前风险：

- 每章完成后重建 `章节合集.txt` 会随着章节数增长越来越慢。

方案：

- 保留：
  - `exports/latest_chapter.txt`
  - `exports/current.txt`
- 对全集导出：
  - 小项目可全量重建。
  - 大项目使用 dirty chapter range 或用户显式导出时重建。

复杂度收益：

- 每章导出从 `O(total_novel_size)` 降为 `O(current_chapter_size)`。

## 算法复杂度目标

| 场景 | 当前风险 | 目标 |
|---|---|---|
| 写一章正文质量检查 | 多个模块重复扫描，接近 `O(m*n)` | 单次扫描报告，`O(n)` |
| 查询当前项目状态 | 可能遍历章节 | 索引查询，`O(1)` |
| 生成下一章上下文 | 随章节摘要增长 | 预算截断，`O(1)` 上限 |
| 导出当前章 | 可控 | `O(chapter_size)` |
| 导出全集 | 可能每章重建全书 | 用户触发或增量策略 |
| 修标题/摘要 | 可能重写正文 | metadata-only，`O(metadata)` |
| 合同质量检查 | 多处重复判断 | typed gate 单点，`O(contract_size)` |

## 分阶段实施计划

### Phase 1：职责冻结与防扩散

目标：

- 明确哪些模块能改状态，哪些只能读。
- 禁止 gateway 和 session surface 新增写作业务判断。

动作：

- 给 `creation_contract.rs`、`session_surface.rs`、`novel_workflow_driver.rs` 顶部补模块职责注释。
- 在文档中标记唯一权威：
  - 合同 ready：`typed_contract_gate`
  - 命名质量：`naming`
  - 正文硬门槛：`body_gate`
  - metadata 修复：`metadata_gate`
  - 生命周期：`creation_contract/lifecycle`

验收：

- `rg` 检查 gateway 中没有新增小说/章节/书名业务词判断。
- `cargo check -p benshu-builtin-tools -p benshu-gateway`

### Phase 2：合同质量门收口

目标：

- `typed_contract_gate` 成为合同质量唯一入口。

动作：

- 从 `creation_contract.rs` 移出合同字段完整性判断。
- `creation_contract_model.rs` 只负责解析/规范化模型输出。
- `session_surface.rs` 只渲染 `typed_contract_gate` 的状态。

验收：

- 合同 blocked 不会写入可确认 draft。
- 合同 ready 才允许 `approve_draft`。
- “未指定”不会作为最终合同字段展示给用户。

### Phase 3：命名治理收口

目标：

- 书名、章节名、角色名只通过 `naming` 模块治理。

动作：

- 抽出：
  - `naming/title.rs`
  - `naming/chapter_title.rs`
  - `naming/character.rs`
  - 评分/吸引力 adapter 目前归入 `naming/title.rs`
- `typed_contract_gate` 调用 `naming::validate_work_title`。
- `quality_checks` 调用 `naming::validate_chapter_title`。

验收：

- 书名必须能由结局、大纲、世界观、爽点/钩子解释。
- 章节名 metadata-only 修复，不重写正文。
- 角色名进入权威表后不漂移。

### Phase 4：正文质量门与 metadata 门拆开

目标：

- 正文问题和 metadata 问题彻底分层。

动作：

- 新建：
  - `workflow/body_gate.rs`
  - `workflow/metadata_gate.rs`
  - `workflow/metadata_repair.rs`
- 正文硬错误：
  - 空正文
  - JSON 残片
  - 占位符
  - 语言错
  - 主角漂移
  - 严重重复
  - 明显未完成
- metadata 问题：
  - 标题不好
  - 摘要缺失
  - key facts 不完整
  - continuity updates 不完整

验收：

- 标题问题不会导致正文重写。
- summary/key facts 问题不会导致正文重写。
- 同类 metadata 修复失败两次后 blocker，而不是无限重试。

### Phase 5：`novel_workflow_driver` 拆分

目标：

- driver 只保留流程编排。

动作：

- 将当前主文件逻辑拆入 workflow 子模块。
- 删除 reuse-existing 中重复 cleanup。
- 统一本地 cleanup 调用入口。

验收：

- `novel_workflow_driver.rs` 降到 1,000 行以内。
- 任一章节流程 checkpoint 能对应到唯一模块。

### Phase 6：`novel_studio` 拆分

目标：

- studio 只保留工具动作入口。

动作：

- 抽：
  - action dispatch
  - manifest read/write
  - chapter IO
  - approval
  - export
  - status rendering

验收：

- `novel_studio.rs` 降到 1,500 行以内。
- 章节批准逻辑只有一个入口。
- export 逻辑只有一个入口。

### Phase 7：正文清理和 sanitizer 合并

目标：

- 只保留一个正文清理入口。

动作：

- 对比：
  - `novel_workflow_driver/output_cleanup.rs`
  - `novel_studio/prose_sanitizer.rs`
  - `quality.rs` 内局部 repair
- 底层合并为 `writing/text_sanitizer.rs`。
- workflow、保存正文、可读导出保留 stage adapter，但共同返回/复用结构化 `SanitizeReport`。

验收：

- `rg "SanitizeReport|WritingSanitizeStage|text_sanitizer"` 能看到清晰底层 facade。
- reuse-existing、draft、revision 都调用同一入口。

### Phase 8：性能优化

目标：

- 降低长篇项目的 IO、clone、重复扫描。

动作：

- 引入 `TextScanReport`。
- 引入 `ProjectCache`。
- 导出使用 `BufWriter`。
- 大项目全集导出改为显式或增量。
- 上下文包输出预算 telemetry。

验收：

- 写第 N 章时上下文包大小不随 N 无界增长。
- 大项目每章保存不重建全书。
- 章节质量检查只扫描正文一次。

### Phase 9：测试拆分

目标：

- 测试仍保留，但按模块拆，不再单文件几千行。

动作：

- `creation_contract_tests.rs` 降为 include wrapper，测试分片放入 `creation_contract_tests/`。
- `novel_studio_tests.rs` 降为 include wrapper，测试分片放入 `novel_studio_tests/`。
- `novel_workflow_driver_tests.rs` 降为 include wrapper，测试分片放入 `novel_workflow_driver_tests/`。
- 后续如果需要继续按语义命名，可在这些目录内逐步把分片改名为 lifecycle/gate/surface/quality/repair/resume 等更细文件。

验收：

- 单个测试文件不超过 1,500 行。
- 聚焦测试名称能直接定位模块职责。

## 不建议做的事

1. 不要再在 gateway/chat 层加写作关键词。
2. 不要为每次真实测试失败补一个 contains 规则。
3. 不要让合同 blocked 文本进入 draft 字段。
4. 不要让标题问题触发正文重写。
5. 不要把工具策略迁到 runtime-policy-core。
6. 不要为了行数少删除 story bible、truth、hook、character ledger 这类核心能力。
7. 不要把全文正文塞进聊天历史或上下文包。
8. 不要把测试数据提交进仓库。

## 推荐优先级

第一优先级：

1. 合同质量门收口。
2. 命名治理收口。
3. 正文门和 metadata 门拆开。
4. 状态机唯一化。

第二优先级：

5. `novel_workflow_driver` 拆分。
6. `novel_studio` 拆分。
7. sanitizer 合并。

第三优先级：

8. `TextScanReport`。
9. `ProjectCache`。
10. 增量导出。
11. 测试拆分。

## 完成后的预期效果

- 修复不再失效，因为每类问题只有一个权威模块。
- 真实面板测试失败时，可以快速定位是合同、命名、正文、metadata、provider 还是状态机。
- 第一章无法生成的问题会更容易定位，不会被“合同、标题、正文、metadata”混在一起。
- 章节标题、书名、角色名治理可以更稳定，而不是靠分散规则叠加。
- 写 50 万字、500 万字时，上下文和导出不会线性爆炸。
- 代码继续增长前有明确边界，不会再变成一个巨型工具文件。
