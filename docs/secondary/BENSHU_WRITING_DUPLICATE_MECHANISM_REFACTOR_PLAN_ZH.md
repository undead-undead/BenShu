# BenShu 写作工具重复机制收口重构计划

> 状态: 草案  
> 日期: 2026-06-27  
> 范围: `crates/builtin-tools/src/tool/writing` 与必要的 `apps/gateway` 展示边界  
> 目标: 删除重复机制、统一职责边界、保留已有能力底座，避免继续通过堆判断修复真实测试问题。

## 0. 当前执行状态

- [x] Phase 0: 基线确认
  - `cargo fmt --check`: 通过
  - `cargo check -p benshu-builtin-tools`: 通过
  - `cargo test -p benshu-builtin-tools chapter_title --lib`: 通过，23 个测试
  - `cargo test -p benshu-builtin-tools creation_contract --lib`: 重构前既有失败 26 个，作为后续合同收口基线
- [x] Phase 1: 统一 CJK / markup 清理内核
  - 通用行级 CJK markup 清理已收口到 `surface_sanitizer.rs`
  - `novel_studio/prose_sanitizer.rs` 与 `novel_workflow_driver/output_cleanup.rs` 保留为场景适配层
- [x] Phase 2: 书名 authority 收口
  - 书名候选选择与理由生成已统一委托 `naming/title.rs`
  - `creation_contract_model/core.rs` 只保留合同字段同步 wrapper，不再内置第二套书名评分
  - `contract_text.rs` 已降级为合同 surface noise 判断，不再承担书名审美质量门
  - 验证: `cargo fmt` 通过；`cargo check -p benshu-builtin-tools` 通过；`cargo test -p benshu-builtin-tools chapter_title --lib` 通过，23 个测试
- [x] Phase 3: 合同状态机单一化
  - 候选提交路径已收口到 `contract_readiness_issues_for_candidate_draft`
  - 合同 ready / needs-repair / blocked 的最终判断只从 draft readiness -> typed gate 进入
  - `creation_contract_model::validate(_for_scope)` 保持 typed gate 委托，不维护第二套 issue 分类
  - `metadata_repair` 保留为修复路由/patch 应用器，应用后仍回到候选提交路径重新跑 typed gate
  - 验证: `cargo fmt` 通过；`cargo check -p benshu-builtin-tools` 通过
- [x] Phase 4: 章节标题 authority 收口
  - 章节标题证据判断已由 `naming::evaluate_chapter_title_candidate` 输出 decision
  - `novel_studio` 只把 decision 映射为 metadata warning/repair 文案
  - 生产路径删除未使用的 `title_needs_post_body_repair` studio wrapper；测试适配保留在 `#[cfg(test)]`
  - 验证: `cargo fmt` 通过；`cargo check -p benshu-builtin-tools` 通过；`cargo test -p benshu-builtin-tools chapter_title --lib` 通过，23 个测试
- [x] Phase 5: gateway 展示 DTO 化
  - 合同 panel payload 已由 `writing/session_surface` 统一生成
  - 合同质量 blocker 的 metadata key 与用户可见 blocker 摘要已收口到 `session_surface`
  - gateway / host 只负责 HTTP task/result 装配，不再拼写作合同展示 DTO 细节
  - 验证: `cargo fmt` 通过；`cargo check -p benshu-gateway` 通过；`cargo test -p benshu-builtin-tools chapter_title --lib` 通过，23 个测试
- [x] Phase 6: 测试归位
  - 合同 plan title repair / forbidden name surface 等测试辅助函数已限制在 `#[cfg(test)]`
  - 章节标题 post-body repair helper 不再作为生产 re-export 暴露
  - 写作测试目录已检查，不再以具体模型名绑定测试语义
  - 验证: `cargo fmt --check` 通过；`cargo check -p benshu-builtin-tools` 通过；`cargo test -p benshu-builtin-tools chapter_title --lib` 通过，23 个测试

## 1. 背景

最近两天写作工具围绕合同、书名、章节名、正文清理、质量门、真实面板回归做了大量修复。当前能力底座已经明显增强，但代码里出现了几类重复机制：

- 同一类 CJK / markup 噪声清理存在三套实现。
- 书名质量判断、书名修复、书名证据解释分散在多个模块。
- 合同 ready / blocked / repair 的判断分散在合同生命周期、typed gate、metadata repair、model core 中。
- 章节标题核心已迁到 `naming/chapter_title.rs`，但 `novel_studio` 仍保留部分遗留判断包装。
- gateway 仍知道写作合同内部状态，应用层和工具层边界不够干净。

这些重复不会必然导致编译问题，但会导致真实测试中出现“修了但没生效”的问题：修复只命中某条路径，另一条路径仍使用旧判断。

## 2. 总原则

### 2.1 不新增平行机制

本次重构不允许再新增一套同类规则。所有修复必须遵循：

- 有现成模块的，迁入现成模块。
- 有现成函数的，复用现成函数。
- 需要新增函数时，必须先删除或替换旧职责。
- 不能因为某个模型、某个题材、某次测试失败而新增特例分支。

### 2.2 工具策略仍归工具，不进入 runtime-policy-core

写作工具的命名、合同、章节、正文质量策略属于 writing 工具内部治理，不迁入 `runtime-policy-core`。

### 2.3 gateway 只做展示，不做写作业务判断

gateway 可以显示任务状态、路径、摘要、按钮，但不应解析“书名是否合格”“合同是否 ready”“写作 blocker 是什么类型”。这些语义应由 writing 模块输出稳定 surface。

### 2.4 正文是最高价值产物

标题、摘要、metadata 问题不应重写正文。正文清理、标题修复、合同修复、审批状态应分层处理。

## 3. 当前重复机制清单

### 3.1 CJK / markup / prose 清理重复

现有位置：

- `crates/builtin-tools/src/tool/writing/surface_sanitizer.rs`
  - `collapse_adjacent_repeated_cjk_phrases`
  - `strip_inline_cjk_markup_noise`
  - `line_is_assistant_surface_noise`
  - 合同 surface residue 判断
- `crates/builtin-tools/src/tool/writing/novel_studio/prose_sanitizer.rs`
  - `strip_chinese_markup_residue_lines`
  - `clean_chinese_markup_residue_line`
  - `line_is_standalone_markup_residue`
  - `strip_latex_arrow_residue_from_chinese_line`
  - `strip_short_escape_residue_near_chinese_line`
  - `is_chinese_noise_boundary`
- `crates/builtin-tools/src/tool/writing/novel_workflow_driver/output_cleanup.rs`
  - `clean_chinese_markup_residue_line`
  - `line_is_standalone_markup_residue`
  - `strip_latex_arrow_residue_from_chinese_line`
  - `strip_short_escape_residue_near_cjk_line`
  - `is_chinese_noise_boundary`

问题：

- `prose_sanitizer.rs` 和 `output_cleanup.rs` 有近似重复实现。
- `surface_sanitizer.rs` 已经有共享入口，但只承接了一部分通用清理。
- 不同路径可能对同一段正文清出不同结果。

保留：

- `surface_sanitizer.rs` 作为通用 surface 清理内核。
- `prose_sanitizer.rs` 保留为 novel_studio 保存/审批路径适配层。
- `output_cleanup.rs` 保留为 workflow driver 输出路径适配层。

删除/替换：

- 将以下通用函数迁入 `surface_sanitizer.rs`：
  - `clean_cjk_markup_residue_line`
  - `line_is_standalone_markup_residue`
  - `strip_latex_arrow_residue_from_cjk_line`
  - `strip_short_escape_residue_near_cjk_line`
  - `is_cjk_noise_boundary`
- 删除 `prose_sanitizer.rs` 与 `output_cleanup.rs` 中重复实现。
- 两个调用方只保留“何时调用、是否保留空行、是否做正文段落策略”的适配逻辑。

验收：

- `rg -n "clean_chinese_markup_residue_line|strip_latex_arrow_residue_from_chinese_line|strip_latex_arrow_residue_from_cjk_line|is_chinese_noise_boundary" crates/builtin-tools/src/tool/writing`
  - 生产实现只应在 `surface_sanitizer.rs` 有一份。
  - 其他模块只能调用，不再实现。
- `cargo test -p benshu-builtin-tools chapter_title --lib`
- `cargo test -p benshu-builtin-tools creation_contract --lib`

### 3.2 书名质量与修复重复

现有位置：

- `crates/builtin-tools/src/tool/writing/naming/title.rs`
  - `select_book_title_candidate_decision`
  - `local_book_title_candidates_from_story_evidence`
  - `title_is_bare_abstract_concept_stack`
- `crates/builtin-tools/src/tool/writing/naming/title_policy.rs`
  - 大量书名弱模板、抽象拼接、泛化标题、候选评分规则。
- `crates/builtin-tools/src/tool/writing/creation_contract_model/core.rs`
  - `repair_canonical_title_from_candidates`
  - `select_repair_title_candidate`
  - `title_candidate_repair_rationale`
- `crates/builtin-tools/src/tool/writing/creation_contract/patch.rs`
  - 使用 naming 决策，但也承担部分候选修复/选择。
- `crates/builtin-tools/src/tool/writing/typed_contract_gate.rs`
  - 调用 `select_book_title_candidate_decision`，并把结果转换成 blocker。
- `crates/builtin-tools/src/tool/writing/creation_contract/contract_text.rs`
  - `generated_title_is_contract_noise`
  - `contract_book_title_has_useful_segment`
  - `generated_title_reuses_protagonist_name`

问题：

- “书名是否有吸引力”与“书名是否是合同脏输出”混在一起。
- model core 会尝试 repair，typed gate 会判定 blocker，contract_text 又有文本噪声判断。
- 容易出现：候选被本地修复后，又被另一层拒绝；或者坏书名被某一层通过。

保留：

- `naming/title.rs` + `naming/title_policy.rs` 是唯一书名质量权威。
- `typed_contract_gate.rs` 只做编排：调用 naming 决策并映射为 ready/blocker/warning。
- `creation_contract_model/core.rs` 只负责 typed contract normalization 与字段同步，不内置书名策略。
- `contract_text.rs` 只负责“LLM 输出表面是否是合同残留/脏文本”，不负责审美质量。

删除/替换：

- 将 `creation_contract_model/core.rs` 中书名候选选择和 rationale 逻辑收口为调用 `naming::select_book_title_candidate_decision`。
- 删除或降级 `contract_text.rs` 中对“书名质量”的判断，只保留明显 surface noise：
  - 字段名误当标题
  - 合同法务残留误当标题
  - 空标题/占位标题
- `creation_contract/patch.rs` 不再独立解释书名好坏，只负责把字段补丁交给 naming gate。

验收：

- `rg -n "repair_canonical_title_from_candidates|title_candidate_repair_rationale|contract_book_title_has_useful_segment|generated_title_is_contract_noise" crates/builtin-tools/src/tool/writing`
  - 书名质量评分和候选选择只能从 `naming` 进入。
- 书名相关测试集中到 `naming` 或 typed gate 映射测试。
- 不允许 gateway、workflow driver、novel_studio 直接判断书名审美。

### 3.3 合同 ready / blocked / repair 重复

现有位置：

- `crates/builtin-tools/src/tool/writing/creation_contract/repair_coordinator.rs`
  - `submit_session_creation_contract_candidate`
  - `try_repair_creation_contract_title_metadata`
  - `repair_session_contract_metadata_locally`
- `crates/builtin-tools/src/tool/writing/creation_contract/contract_candidate/metadata_repair.rs`
  - `creation_contract_issues_are_title_metadata_only`
  - `creation_contract_issues_are_contract_metadata_only`
  - `submit_pending_contract_title_metadata_repair`
  - `submit_pending_contract_metadata_repair`
- `crates/builtin-tools/src/tool/writing/creation_contract_model/core.rs`
  - `validate`
  - `validate_for_scope`
  - 默认 seed / normalize 逻辑
- `crates/builtin-tools/src/tool/writing/typed_contract_gate.rs`
  - typed contract readiness 主门。
- `crates/builtin-tools/src/tool/writing/typed_contract_gate/*`
  - `structured_gate.rs`
  - `outline_gate.rs`
  - `surface_gate.rs`
  - `character_gate.rs`

问题：

- repair_coordinator、metadata_repair、model core、typed gate 都在不同程度上“决定合同状态”。
- `Blocked / NeedsRepair / Ready` 容易在不同层出现不同解释。
- 这会导致真实面板看到“任务 completed，但合同未 ready”或“按这个开始写第一章又回到 planning”。

保留边界：

- `creation_contract`：draft lifecycle 和候选记录。
- `creation_contract_model`：typed struct、normalize、字段默认化，但不决定生命周期状态。
- `typed_contract_gate`：唯一 read-only readiness 判定，不修改 draft。
- `repair_coordinator`：唯一 repair 路由器，根据 typed gate issue 选择修复方式。
- `metadata_repair`：唯一 patch 应用器，应用后必须重新跑 typed gate。

删除/替换：

- 删除 `metadata_repair.rs` 中独立推导“ready”的逻辑，只返回 patch result。
- `repair_coordinator.rs` 不直接构造 ready 结论，所有结论来自 typed gate。
- `creation_contract_model/core.rs` 的 `validate` 可以保留，但内部必须委托 typed gate；不能维护第二套 issue 分类。
- `creation_contract_issues_are_*` 这类 issue 分类要么迁成 typed gate issue enum，要么只作为 repair 路由辅助，不得决定最终状态。

验收：

- 每次 pending/current 合同更新路径都满足：
  1. normalize
  2. typed gate
  3. gate result 写入 draft lifecycle
  4. surface 展示读取 lifecycle
- `rg -n "ContractGateStatus|CreationContractGate|ready|blocked" crates/builtin-tools/src/tool/writing/creation_contract crates/builtin-tools/src/tool/writing/creation_contract_model crates/builtin-tools/src/tool/writing/typed_contract_gate*`
  - 确认最终状态只由 typed gate result 推导。

### 3.4 章节标题判断包装层仍偏多

现有位置：

- `crates/builtin-tools/src/tool/writing/naming/chapter_title.rs`
  - `evaluate_chapter_title_candidate`
  - `select_final_chapter_title_from_body`
  - `chapter_title_needs_post_body_repair`
  - 章节标题疲劳、证据、默认标题、残句检查。
- `crates/builtin-tools/src/tool/writing/novel_studio/quality_gate.rs`
  - `chapter_title_blocking_issues`
  - `chapter_title_formality_metadata_issues`
  - `chapter_title_registry_issues`
  - `chapter_title_fatigue_issues`
  - `chapter_title_completion_issues`
- `crates/builtin-tools/src/tool/writing/novel_studio/project_governance.rs`
  - `final_chapter_title_from_body`
  - `title_needs_post_body_repair`
  - `chapter_title_is_generic_stage_label`

问题：

- 当前多数 wrapper 已委托到 naming，但仍保留部分本地判定。
- 未来很容易再次把章节标题规则写回 quality_gate/project_governance。

保留：

- `naming/chapter_title.rs` 是唯一章节标题策略权威。
- `novel_studio/quality_gate.rs` 只负责把 naming decision 映射为：
  - blocker
  - metadata repair
  - warning
- `project_governance.rs` 只负责项目状态更新，不直接判断标题好坏。

删除/替换：

- 将 `chapter_title_blocking_issues`、`chapter_title_formality_metadata_issues` 中剩余的标题策略迁回 `naming/chapter_title.rs`。
- `quality_gate.rs` 改成调用一个统一的 `ChapterTitleDecision`。
- `project_governance.rs` 的标题函数若只是转发，可以保留极薄 wrapper；若含策略，则迁移。

验收：

- `rg -n "chapter_title_.*issues|title_needs_post_body_repair|generic_stage_label|stale_connector" crates/builtin-tools/src/tool/writing/novel_studio crates/builtin-tools/src/tool/writing/novel_workflow_driver`
  - 应只看到调用 naming decision 的适配逻辑。

### 3.5 gateway 写作合同展示边界过深

现有位置：

- `apps/gateway/src/api/handlers/chat.rs`
  - `is_creation_contract_task`
  - `creation_contract_task_status_from_session_draft`
  - `creation_contract_lifecycle_status_from_session_draft`
  - `creation_contract_quality_blocker_from_outcome`
  - 多处直接拼 `result["creation_contract"]`

问题：

- gateway 仍知道 creation contract 的内部 lifecycle 细节。
- 如果 writing 模块内部状态变了，gateway 需要同步修。
- 这容易造成应用层和工具层各自维护一套状态解释。

保留：

- gateway 负责 HTTP / SSE / task result / panel response。
- writing 工具负责合同状态、surface 文本、可确认状态、文件路径。

删除/替换：

- 在 writing 模块提供稳定 DTO，例如：

```rust
pub struct WritingTaskPresentation {
    pub status: WritingTaskStatus,
    pub user_visible_text: String,
    pub artifacts: Vec<WritingArtifactLink>,
    pub contract: Option<CreationContractPresentation>,
    pub can_confirm: bool,
    pub next_actions: Vec<String>,
}
```

- gateway 只调用 writing 提供的 presentation builder。
- `chat.rs` 不再读取 `creation_contract_quality_blocked` 等内部 metadata。

验收：

- `rg -n "creation_contract" apps/gateway/src/api/handlers/chat.rs`
  - 只允许出现 DTO 类型、调用 presentation builder、测试展示契约。
  - 不允许出现合同 quality/blocker 业务判断。

## 4. 分阶段执行方案

### Phase 0: 基线确认

目标：

- 不改功能，只确认当前重复点和测试基线。

操作：

- `git status --short`
- `cargo fmt --check`
- `cargo check -p benshu-builtin-tools`
- `cargo test -p benshu-builtin-tools chapter_title --lib`
- `cargo test -p benshu-builtin-tools creation_contract --lib`

注意：

- 当前已知 `creation_contract` 测试仍有失败，重构前要记录失败数量和失败类型。
- 不碰 `data/benshu.yaml`、`data/cron.redb`。

完成标准：

- 有清晰基线，知道哪些失败是重构前就存在。

### Phase 1: 统一 CJK / markup 清理内核

目标：

- 消除 `surface_sanitizer`、`prose_sanitizer`、`output_cleanup` 三套重复清理。

步骤：

1. 在 `surface_sanitizer.rs` 新增或公开通用 helper。
2. 替换 `prose_sanitizer.rs` 重复实现。
3. 替换 `output_cleanup.rs` 重复实现。
4. 删除本地重复函数。
5. 保留调用方适配逻辑。

风险：

- 清理顺序变化可能影响正文格式。
- 应避免一次性改动“是否删除整行”的业务策略。

验收：

- 针对 `\l`、`rightarrow`、孤立 `$`、CJK 邻接噪声的测试仍通过。

### Phase 2: 书名 authority 收口

目标：

- 所有“书名好不好、候选怎么选、为什么拒绝”只从 `naming/title*` 出来。

步骤：

1. 审查 `creation_contract_model/core.rs` 里所有 title repair 函数。
2. 将候选选择和 rationale 委托到 `naming/title.rs`。
3. `contract_text.rs` 只保留 surface-noise 判断。
4. `typed_contract_gate.rs` 只保留 gate adapter。
5. 删除或降级重复 title 函数。

风险：

- 书名修复链路短期可能更严格，导致合同补齐暴露更多 blocker。
- 不应放宽质量门，而应保证 repair coordinator 能拿到 actionable issue。

验收：

- 抽象拼接标题仍会被拒绝。
- 从剧情、结局、爽点生成候选仍可通过。
- 用户指定标题优先，但仍做 surface safety。

### Phase 3: 合同状态机单一化

目标：

- `Ready / NeedsRepair / Blocked` 只有一个来源。

步骤：

1. 定义 typed gate result 是最终状态来源。
2. `creation_contract_model::validate` 内部委托 typed gate。
3. `metadata_repair` 不返回最终 ready，只返回 patch 应用结果。
4. `repair_coordinator` 每次 patch 后强制重新跑 typed gate。
5. draft lifecycle 只存 gate result，不自行推断。

风险：

- 这一步会触及真实面板“按这个开始写第一章”的关键路径。
- 需要保留旧项目迁移兼容，不允许已有草案直接崩。

验收：

- 合同未 ready 时，“开始写第一章”明确返回合同缺口，不重新 planning。
- 合同 ready 后，“开始写第一章”只能进入 writer/novel_studio，不回到 creation planning。

### Phase 4: 章节标题 authority 收口

目标：

- 章节标题策略只在 `naming/chapter_title.rs`。

步骤：

1. 新增或确认统一 `ChapterTitleDecision`。
2. `quality_gate.rs` 只做 decision 到 quality issue 的映射。
3. `project_governance.rs` 只做 project state / metadata 更新。
4. 删除本地标题策略判断。

风险：

- 不能让标题 metadata 问题重新阻塞正文。

验收：

- 标题为空、默认标题、重复标题、正文残句为硬问题。
- 标题意象弱为 warning 或 metadata repair，不重写正文。
- 写后标题可更新 manifest/export/frontmatter。

### Phase 5: gateway 展示 DTO 化

目标：

- gateway 不再解释写作合同内部字段。

步骤：

1. 在 writing/session_surface 或 creation_contract/surface 增加 presentation DTO。
2. gateway 只调用 presentation builder。
3. 移除 `chat.rs` 中合同 quality/blocker 解析。
4. 保留 HTTP task status 和 artifacts 链接输出。

风险：

- 面板展示字段可能短期变化。
- 需要确认合同草案仍完整展示，不被 1200 字符截断。

验收：

- 聊天框能看到合同草案、缺口、下一步动作。
- 后台任务 completed/blocked 和内部合同 ready 状态一致。

### Phase 6: 测试归位

目标：

- 测试按职责归属，不让测试文件变成另一个重复机制入口。

步骤：

1. title 测试放 `naming`。
2. contract readiness 测试放 `typed_contract_gate` / creation_contract lifecycle。
3. sanitizer 测试放 `surface_sanitizer`。
4. workflow 测试只测端到端行为，不重复底层规则。

验收：

- 测试名不绑定具体模型名。
- 测试固定输入固定断言，但测试样例不得进入生产逻辑。

## 5. 删除清单

优先删除或替换以下重复内容：

- `prose_sanitizer.rs` 中可迁入 `surface_sanitizer.rs` 的 CJK 噪声函数。
- `output_cleanup.rs` 中可迁入 `surface_sanitizer.rs` 的 CJK 噪声函数。
- `contract_text.rs` 中不属于 surface noise 的书名质量判断。
- `creation_contract_model/core.rs` 中独立书名候选选择/审美判断。
- `metadata_repair.rs` 中独立判断合同最终 ready 的逻辑。
- `quality_gate.rs` / `project_governance.rs` 中不委托 `naming/chapter_title.rs` 的章节标题策略。
- `chat.rs` 中解析 writing 内部 blocker 的业务逻辑。

## 6. 保留清单

以下不是垃圾代码，不应删除：

- `creation_contract` 的 draft lifecycle。
- `typed_contract_gate` 的 read-only 合同 readiness。
- `naming/title_policy.rs` 的书名质量策略。
- `naming/chapter_title.rs` 的章节标题策略。
- `novel_studio` 的 manifest、chapter IO、approval、truth、export。
- `novel_workflow_driver` 的章节执行流。
- 测试中的固定小说名、章节名、角色名样例，只要它们位于 `#[cfg(test)]` 或测试模块中。

## 7. 性能与复杂度要求

### 7.1 不引入更高复杂度

- 标题候选评分应保持候选数有限，避免正文全量 O(n*m) 多轮扫描。
- 正文清理应按行线性处理。
- 合同 gate 应基于 typed struct 字段，不重新遍历全部 raw prompt。

### 7.2 减少 clone

重构时优先：

- `&str` / `Cow<'_, str>` 处理清理结果。
- 只有实际修改时分配新 `String`。
- 大文本不要在多个 gate 之间重复 clone。

### 7.3 避免多次 LLM 修复

本次是代码边界重构，不通过增加 LLM 修复轮数解决问题。

## 8. 风险

### 8.1 短期暴露更多真实 blocker

重复机制被删除后，原来被某条路径“兜过去”的坏合同可能会暴露出来。这是健康现象，不应立刻放宽质量门。

### 8.2 测试需要同步改口径

原来测试可能依赖旧的分散判断。重构后应按新职责修改测试，而不是为了旧断言保留重复代码。

### 8.3 面板展示可能需要同步

gateway DTO 化后，面板字段路径可能变化。必须保证用户能看到：

- 合同草案
- 是否可确认
- 缺少什么
- 生成文件路径
- 写作进度

## 9. 最终验收矩阵

代码层：

- `cargo fmt`
- `cargo check -p benshu-builtin-tools`
- `cargo test -p benshu-builtin-tools chapter_title --lib`
- `cargo test -p benshu-builtin-tools creation_contract --lib`
- `git diff --check`

搜索层：

- `rg -n "clean_chinese_markup_residue_line|strip_latex_arrow_residue_from_chinese_line|is_chinese_noise_boundary" crates/builtin-tools/src/tool/writing`
- `rg -n "generated_title_is_contract_noise|contract_book_title_has_useful_segment|repair_canonical_title_from_candidates|title_candidate_repair_rationale" crates/builtin-tools/src/tool/writing`
- `rg -n "creation_contract_quality_blocker_from_outcome|creation_contract_task_status_from_session_draft" apps/gateway/src/api/handlers/chat.rs`

真实面板层：

1. 泛化开场：
   - 用户: `帮我写小说`
   - 预期: 轻量追问，不启动后台重任务。
2. 具体需求：
   - 用户: `写都市玄幻小说，每章2500字，至少5万字`
   - 预期: 自动生成完整可确认合同草案。
3. 开始写作：
   - 用户: `按这个开始，写第一章`
   - 预期: 不回到 planning，生成章节文件。
4. 标题/metadata 修复：
   - 只修 metadata，不重写正文。
5. 书名质量：
   - 书名必须能从剧情、终局、爽点、世界意象解释出来，并像作品名。
6. 聊天框反馈：
   - 显示进度、章节号、字数、文件路径、摘要、审查状态，不塞全文正文。

## 10. 建议执行顺序

推荐顺序：

1. Phase 0 基线确认。
2. Phase 1 统一 CJK / markup 清理。
3. Phase 2 书名 authority 收口。
4. Phase 3 合同状态机单一化。
5. Phase 4 章节标题 authority 收口。
6. Phase 5 gateway 展示 DTO 化。
7. Phase 6 测试归位。

不要先做真实长篇回归。先把重复机制收口，否则真实测试失败时仍然难以判断到底是哪条路径生效。
