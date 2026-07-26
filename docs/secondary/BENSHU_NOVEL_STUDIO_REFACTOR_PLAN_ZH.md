# BenShu Novel Studio 子系统重构审查与实施计划

更新时间：2026-06-22

范围：

- `crates/builtin-tools/src/tool/writing/novel_studio.rs`
- `crates/builtin-tools/src/tool/writing/novel_studio/`
- `crates/builtin-tools/src/tool/writing/novel_studio_tests/`
- 与 `novel_studio` 直接交互的 `novel_workflow_driver` 调用点

不属于本文档范围：

- `creation_contract` 合同生成状态机的整体重构。
- `writing/naming` 全局命名治理的继续扩展。
- gateway / panel 聊天入口改造。
- 模型 provider、llama.cpp、本地模型参数调优。
- 运行时数据清理或测试产物删除。

本文档是 `WRITING_TOOL_BOUNDARY_REFACTOR_PLAN_ZH.md` 的子计划。总边界仍以写作工具职责边界文档为准；本文只回答一个问题：`novel_studio` 内部如何继续降低复杂度、消除重复、收紧 action/质量门/truth/export 这几条真实写作主路径。

## 1. 当前审查结论

### 1.1 当前代码状态

`novel_studio` 已经不是普通工具文件，而是一个长篇小说项目运行时子系统。它承担：

- 项目 manifest 读写。
- 草案与项目生命周期。
- 章节计划、执行包、上下文包。
- 正文写入、章节记录、metadata 修复。
- 审稿、修订、批准。
- pending settlement、truth、story bible 更新。
- 分卷、摘要、伏笔、角色权威表。
- 快照、恢复、导出。
- 状态、审计、analytics。

这类能力确实需要较多代码，不应简单按行数判断为“臃肿”。当前最需要治理的不是“继续删功能”，而是保证：

- 公开 action、内部兼容 action、错误引导 action 不漂移。
- 正文质量门、metadata 门、合同泄露门、角色漂移门互不越界。
- 章节未批准时绝不污染 truth / summary / hooks / story bible。
- 命名治理只通过 `writing/naming` 入口调用，不回到散落判断。
- 大文本扫描、正文清理、导出尽量复用扫描结果，避免重复 O(n)。

### 1.2 已经做得比较好的部分

1. `novel_studio.rs` 主文件已经降为薄入口。
   - 主文件主要保留工具定义、参数归一、action dispatch、少量共享 helper。
   - 项目生命周期、配置、章节 IO、状态 truth、审稿批准、导出状态已经拆到子模块。

2. 审稿批准链路相对安全。
   - `approve_chapter` 前会重新运行正文质量门。
   - 会重新运行 metadata gate。
   - 会校验 pending settlement 是否与正文匹配。
   - 只有 approved 后才应用 pending settlement、更新 story bible、压缩长篇状态、同步 txt 导出。

3. 下一章上下文包不直接塞全文正文。
   - `context_packaging` 读取 approved prior chapters 的摘要、key facts、continuity updates。
   - sources / archives / truth files 有截断和预算。

4. 命名逻辑开始从 `project_governance` 迁回 `writing/naming`。
   - 章节标题选择、默认标题判断、书名/卷名重复判断已经通过 `naming` 入口。
   - 这是正确方向，应继续保持。

5. `contract_terms` 是正确方向。
   - 角色名、世界术语、组织/地点开始有权威视图。
   - 这比“从正文里粗扫 2-4 字 CJK 候选名”更通用。

### 1.3 已发现的主要风险

1. `input.rs` 的错误引导 action 列表和真实公开 schema 不一致。
   - schema 中有 `update_project`、`clone_project`、`import_chapters`、`repair_chapter_metadata`、`review_chapter`、`reject_chapter`、`snapshot`、`restore_snapshot` 等。
   - `missing_novel_action_result` / `wrong_novel_studio_action_result` 返回的 `available_actions` 是手写旧列表。
   - 影响：模型调错 action 后，系统给出的纠错菜单会把它带回旧流程。

2. 内部兼容 action 与公开 action 的边界容易被误解。
   - `plan_chapter`、`compose_chapter`、`architect_chapter`、`add_chapter_plan`、`repair_latest_chapter_metadata` 是测试明确要求隐藏的内部兼容面。
   - 但 dispatcher 仍支持它们。
   - 影响：如果未来有人按 dispatcher 补 schema，可能把内部兼容面重新暴露给 LLM，破坏工具面收口。

3. `quality_checks.rs` 仍是最大维护热点。
   - 文件约 3800 行，函数约 143 个。
   - 混合了剧情推进、合同泄露、角色漂移、CJK 噪声、重复句式、占位符、结尾完整性、标题辅助判断。
   - 影响：真实测试中一旦出现“质量门误伤”，很难快速判断是哪一类门出了问题。

4. 合同泄露检测增强后有误伤风险。
   - 当前已把 `premise` 和 `outline` 纳入检测。
   - 它能防止合同/提纲文本直接混入正文，但如果正文自然复述设定，也可能被判为泄露。
   - 影响：第一章容易因为设定介绍被误判，而不是因为正文真的脏。

5. 命名迁移还有 warning 尾巴。
   - 编译测试暴露部分 `naming` 和 `novel_studio` 的未使用 import / 未使用 public struct。
   - 影响不大，但说明迁移后旧 API 还没完全收口。

6. 大文本性能仍有继续优化空间。
   - 质量门、metadata 门、settlement、导出、重复检查都可能读取正文。
   - 已经有 `TextScanReport`，但不是所有路径都完全复用。
   - 长篇项目下，这些重复扫描会累计成明显延迟。

## 2. 重构目标

### 2.1 产品目标

用户自然语言触发写作后，`novel_studio` 必须稳定承担“项目真实状态”的职责：

```text
合同 ready
-> 创建/恢复项目
-> 构建章节执行上下文
-> 写入正文 artifact
-> 本地硬门槛检查
-> metadata-only 修复
-> 审稿/修订
-> approve
-> truth/story_bible/hooks/summary/export 更新
-> 下一章继续
```

其中：

- 不允许未批准草稿污染后续上下文。
- 不允许 metadata 问题触发正文重写。
- 不允许错误引导把模型带到旧 action。
- 不允许章节标题、书名、角色名在 `novel_studio` 内重新发明一套命名规则。
- 不允许大文本在同一阶段被无意义重复扫描。

### 2.2 工程目标

1. action surface 单一来源。
2. quality gate 分域拆分。
3. contract leakage gate 独立化，并降低误伤。
4. character drift 只依赖合同术语权威视图。
5. approval/truth 链路保持强边界。
6. export 和 chapter IO 性能稳定。
7. 测试按职责拆分，覆盖真实失败模式。
8. 文档和代码同步，避免“文档说完成，代码仍有旧路径”。

## 3. 重构硬约束

1. 不新增平行机制。
   - 新 helper 前必须先 `rg` 是否已有等价逻辑。
   - 如已有，应迁移、替换或薄转发，不应并存。

2. 不把内部兼容 action 暴露给 LLM。
   - 公开 action 只来自 schema。
   - 内部兼容 action 必须带注释和测试说明。

3. 不在 gateway / brain 增加小说业务规则。
   - gateway 只能展示工具结果、artifact、任务状态。
   - 小说项目状态、章节状态、质量门都留在 writing 工具。

4. 不因为 metadata 问题重写正文。
   - title / summary / key_facts / continuity_updates 问题只走 metadata repair。

5. 不用题材词表修通用 bug。
   - 玄幻、修仙、都市、言情、科幻的具体词汇应来自 genre profile / contract / story bible。
   - 通用质量门只判断结构、污染、重复、漂移、可读性。

6. 不提交运行时数据。
   - `data/benshu.yaml`
   - `data/cron.redb`
   - `data/generated`
   - 模型文件

7. 每个 Phase 完成后必须：
   - `cargo fmt`
   - 聚焦 `cargo test -p benshu-builtin-tools ... --lib`
   - 必要时 `cargo check -p benshu-builtin-tools`
   - `git diff --check`

## 4. 目标模块边界

### 4.1 `novel_studio.rs`

职责：

- 工具定义。
- action dispatch。
- action lifecycle wrapper。
- 最小共享 helper。

不应承担：

- 质量门细节。
- 命名判断。
- 章节正文清理。
- 合同补齐。
- 复杂导出。

### 4.2 `tool_schema.rs`

职责：

- 公开 tool schema。
- 公开 action enum。
- 对用户/LLM 可见的参数说明。

改造目标：

- 提供 `PUBLIC_ACTIONS` 常量。
- schema、错误提示、测试共用该常量。
- 显式声明 `INTERNAL_COMPAT_ACTIONS`，但不放进 schema。

### 4.3 `input.rs`

职责：

- 参数形状归一。
- missing action / wrong action 的可恢复错误。

改造目标：

- `available_actions` 不再手写。
- `wrong_novel_studio_action_result` 返回：
  - attempted action
  - public actions
  - 如果命中内部兼容 action，则提示使用对应 canonical public action。
  - 如果命中其他工具名，则提示调用独立工具。

### 4.4 `quality_gate.rs`

职责：

- 编排正文硬门槛、metadata 门、状态裁判。
- 不包含大量具体检查算法。

目标：

```text
chapter_quality_gate
-> body_quality::hard_issues
-> body_quality::repairable_issues
-> body_quality::warnings
-> character_drift::issues
-> contract_leakage::issues
-> progression::issues
-> completion::issues
```

### 4.5 `quality_checks/`

目标拆分目录：

```text
novel_studio/quality_checks/
  mod.rs
  surface_noise.rs
  contract_leakage.rs
  character_drift.rs
  progression.rs
  completion.rs
  repetition.rs
  cjk_text.rs
  narrative_substance.rs
```

拆分原则：

- 只移动代码，不改变行为。
- 每次拆分后跑对应测试。
- 禁止一边拆一边改规则阈值。

### 4.6 `contract_terms.rs`

职责：

- 从 manifest、contract、story_bible、structured_contract_v2、world rules、relationship ledger 中构建合同术语权威视图。

目标结构：

```rust
ContractTermAuthorityView {
    character_names,
    world_terms,
    organizations_or_places,
    relationship_terms,
    artifact_terms,
}
```

使用规则：

- 角色漂移只看 `character_names`。
- 命中 `world_terms` / `organizations_or_places` 的候选不得升级为角色漂移 blocker。
- 不确定候选最多 warning，不直接 blocker。

### 4.7 `project_governance.rs`

职责：

- project manifest 的治理补全。
- title state、story bible、volume、character authority ledger 的一致性维护。

不应承担：

- 本地章节标题生成算法。
- 书名吸引力评分。
- 正文质量门。

目标：

- 保持调用 `writing/naming`。
- 清掉迁移后的旧命名 import / helper。
- 对新项目和旧项目迁移区分处理：
  - 旧项目可 warning 迁移。
  - 新合同缺关键角色锚点不应被默认值静默补满。

### 4.8 `review_approval.rs`

职责：

- 审稿、修订、批准。
- approve 前的最终安全检查。

目标：

- 保持当前强边界。
- 增加测试确认：
  - metadata-only 修复不会改正文。
  - pending settlement 只有 approve 后进入 truth。
  - failed/needs_revision 不会进入 next chapter context。

### 4.9 `context_packaging.rs`

职责：

- 构建给下一章的上下文包。
- 控制上下文预算。

目标：

- 明确分区预算 telemetry：
  - contract
  - structured_contract_v2
  - story_bible
  - truth_files
  - recent_chapters
  - volume_summary
  - hooks
  - relationship/emotion ledger
  - sources / archives
- 不读取未批准章节正文。
- 长篇 100 章、500 万字项目仍然只增长摘要与状态，不增长全文上下文。

### 4.10 `status_export.rs`

职责：

- 状态输出。
- txt / md 导出。
- readable txt 同步。

目标：

- 导出只相信 manifest 权威标题、卷名、章节标题。
- 不从正文 heading 反推权威字段。
- 导出使用流式写入。
- 大项目导出避免一次性拼接全文字符串。

## 5. Phase 计划

## Phase 0：冻结审查基线

目标：

- 明确当前 dirty diff。
- 明确当前 `novel_studio` 行数、模块、测试状态。
- 不做功能改动。

操作：

1. `git status --short`
2. `rg --files crates/builtin-tools/src/tool/writing/novel_studio`
3. `find ... -name '*.rs' -exec wc -l`
4. `cargo test -p benshu-builtin-tools novel_studio_definition_schema_exposes_curated_public_actions --lib`
5. `cargo test -p benshu-builtin-tools contract_premise_must_not_leak_as_prose_clause --lib`

验收：

- [x] 文档记录当前基线。
- [x] 确认未触碰 runtime dirty files。

风险：

- 当前仓库可能已有大量 writing dirty diff；重构前必须区分本次改动与历史未提交改动。

## Phase 1：Action Surface 单一来源

目标：

- 公开 action 列表只维护一处。
- schema、missing action、wrong action、测试都引用同一来源。

建议实现：

1. 在 `tool_schema.rs` 新增：

```rust
pub(super) const PUBLIC_ACTIONS: &[&str] = &[...];
pub(super) const INTERNAL_COMPAT_ACTIONS: &[&str] = &[...];
```

2. `novel_studio_parameters()` 用 `PUBLIC_ACTIONS` 生成 enum，不再手写 raw JSON enum。
3. `input.rs` 的 `available_actions` 使用 `tool_schema::PUBLIC_ACTIONS`。
4. `wrong_novel_studio_action_result` 增加：
   - `internal_compat_action: bool`
   - `canonical_action_hint`
5. 测试：
   - schema exposes public actions。
   - internal compat actions hidden。
   - missing/wrong action available_actions 与 schema 一致。

验收：

- [x] `tool_schema.rs` 不再有第二份 action enum。
- [x] `input.rs` 不再手写 action 列表。
- [x] 内部兼容 action 仍可由 dispatcher 接收，但不会出现在 schema。

风险：

- raw JSON schema 改为动态生成时可能影响字段顺序，但不应影响工具调用。

## Phase 2：内部兼容 Action 注释与防漂移测试

目标：

- 保留内部兼容 action，但防止以后误暴露。

内部兼容 action 当前包括：

- `plan_chapter`
- `compose_chapter`
- `architect_chapter`
- `add_chapter_plan`
- `repair_latest_chapter_metadata`

处理原则：

- 如果 workflow 内部仍用，保留。
- 如果只有旧测试用，考虑迁移测试到 canonical public action。
- 不在 schema 暴露。

建议实现：

1. 在 dispatcher match 前增加注释，说明这组 action 是 legacy/internal compatibility surface。
2. 增加 `is_internal_compat_action(action)` helper。
3. wrong action 如果命中 internal compat action，不说 unknown，而是提示 canonical action。
4. 对 `compose_chapter` 的双态行为加注释：
   - 有 content -> write_draft
   - 无 content -> compose_context
   - 长期目标是由 canonical public action 替代。

验收：

- [x] 内部兼容 action 不再被误判为普通 unknown。
- [x] 测试明确保护“隐藏但兼容”。

风险：

- 如果 LLM 看到 internal compat hint，可能继续尝试旧 action。提示文案应鼓励 canonical action，而不是推荐 legacy。

## Phase 3：Quality Checks 只移动拆分

目标：

- 拆 `quality_checks.rs`，但不改判断逻辑。
- 先降低维护风险，再讨论阈值。

建议拆分顺序：

1. `surface_noise.rs`
   - `pre_sanitized_surface_contamination_issues`
   - `line_looks_like_json_field_surface`
   - `line_looks_like_artifact_receipt_surface`
   - provider/protocol/markup residue 检查。

2. `contract_leakage.rs`
   - `contract_governance_leakage_issues`
   - `contract_governance_clauses`
   - `contract_clause_leaks_into_sentence`
   - `cjk_probe_terms`

3. `character_drift.rs`
   - `contract_character_anchor_issues`
   - `contract_character_drift_issues`
   - 与 `contract_terms` 的连接。

4. `progression.rs`
   - `chapter_progression_contract_issues`
   - state change / hook / closure signal。

5. `completion.rs`
   - `chapter_completion_mode_issues`
   - `completion_obligation_issues`
   - ending / target reached / closure。

6. `repetition.rs`
   - shingle similarity。
   - repeated paragraph opening。
   - overused story term / concept。

验收：

- [x] `quality_checks.rs` 降为轻量 re-export + 剩余正文/进度检查承载文件；重型检查已拆入子模块。
- [x] 每次拆分后行为测试不变。
- [x] 没有新增阈值。

风险：

- 移动函数可能引起可见性膨胀。优先用 `pub(super)`，不要随意 `pub(crate)`。

## Phase 4：合同泄露 Gate 降误伤

目标：

- 防止合同/提纲文本直接混入正文。
- 但不要把正常设定介绍误判为 blocking。

当前风险：

- `premise`、`outline` 加入检测后，第一章自然介绍世界观时可能命中多个 4 字窗口。

建议实现：

1. 将泄露检测分层：

```text
Exact / near-exact governance clause leak -> blocker
Outline/premise semantic overlap only -> warning
Contains meta prose markers -> blocker
```

2. 增加“说明文/合同文气质”判断：
   - 包含“本章/本文/故事/主角弧线/终局/世界观/必须避免/大纲/合同/设定为”等元文本 marker。
   - 或句子明显是条款式表达。

3. `premise` / `outline` 默认先作为 warning 来源，除非同时有元文本 marker 或高度近似。

验收：

- [x] 合同条款原文直接出现在正文仍 blocking。
- [x] 正文自然戏剧化呈现 premise 不 blocking。
- [x] 增加测试：
  - contract premise leak as prose clause。
  - dramatized premise should not block。
  - outline meta sentence should block。

风险：

- 降低误伤可能放过少量合同残留。需要把明显 marker 保持 blocking。

## Phase 5：角色漂移只依赖合同术语权威视图

目标：

- 不再从正文粗扫所有 2-4 字 CJK 词作为角色名。
- 防止“符文、阵法、网络、灵脉、部门、公司”等世界术语被当成人名漂移。

建议实现：

1. 强化 `contract_terms.rs`：

```rust
ContractTermAuthorityView {
    character_names,
    world_terms,
    organizations_or_places,
    relationship_terms,
    artifact_terms,
}
```

2. `contract_character_drift_issues` 只做：
   - 已知角色名是否被改写。
   - 已知 forbidden_renames 是否出现。
   - 未知疑似重要角色只 warning，不 blocker。

3. 命中 world/org/place/artifact terms 的候选直接跳过。

4. 删除或弱化 `stable_character_anchor_name` 中的硬排除词表。

验收：

- [x] 世界术语不触发角色漂移 blocker。
- [x] 旧主角名/其他项目主角名污染仍能被抓住。
- [x] 合同角色名缺失时阻止合同 ready，而不是到正文阶段靠漂移门补救。

风险：

- 如果模型凭空引入新重要角色，可能只 warning。解决方式应在合同/章节执行包中要求“新增重要角色必须进入角色表”，不是正文质量门粗猜。

## Phase 6：Metadata Gate 与正文 Gate 再收紧边界

目标：

- 彻底保证标题、摘要、key facts、continuity_updates 只触发 metadata repair。
- 正文 gate 只管正文硬错误。

正文 blocker 应包括：

- 正文缺失。
- 明显乱码/协议污染/JSON 字段残留。
- 语言错。
- 主角/权威角色漂移。
- 大段重复。
- 占位符/省略声明/未完成免责声明。
- 字数低于档位。
- truth validation 不通过。

metadata repair 应包括：

- 标题为空、默认标题、像正文残句、和书名/卷名重复。
- 摘要不支撑正文。
- key facts 缺失或不被正文支持。
- continuity_updates 缺失或不被正文支持。

验收：

- [x] metadata-only repair 不改正文 hash。
- [x] 标题问题不进入 revise_chapter。
- [x] metadata 两轮不收敛时返回 blocker，但保留正文。

风险：

- 如果 metadata gate 太宽，export 会出现不佳标题/摘要。解决方式是 metadata blocker，不是重写正文。

## Phase 7：Approval / Truth / Story Bible 不回退

目标：

- 保持并加强当前最重要的安全边界。

必须保持：

- `settle_chapter_state` 只写 pending settlement。
- `approve_chapter` 前重新校验正文、metadata、truth、settlement。
- approved 后才：
  - apply pending settlement
  - update story bible
  - compact longform state
  - write snapshot
  - sync readable txt export

建议测试：

1. needs_revision chapter 不进入 context。
2. rejected chapter 不进入 context。
3. audit_passed 但未 approved 不进入 truth。
4. pending settlement validation failed 时 approve 失败。
5. approve_all 只批准 ready chapters。

验收：

- [x] 上述测试覆盖。
- [x] `context_packaging` 只读取 approved prior chapters。

风险：

- 自动 approve 太激进会污染长篇；approve_all 必须保守。

## Phase 8：Context Packaging 预算治理

目标：

- 支持 50 万字、500 万字长篇时上下文不爆炸。

建议实现：

1. `build_context_payload` 输出完整结构。
2. `build_prompt_context_payload` 输出压缩后的 prompt 结构。
3. 增加 telemetry：

```json
{
  "context_budget": {
    "full_chars": 12345,
    "prompt_chars": 6789,
    "sections": {
      "contract": 1000,
      "story_bible": 1500,
      "truth_files": 2000,
      "recent_chapters": 1200,
      "archives": 900,
      "sources": 800
    }
  }
}
```

4. 保证 prompt 里：
   - 最近章节只取 approved summary。
   - 长篇历史优先卷总结、arc summary、hook ledger。
   - 不塞未批准正文。

验收：

- [x] 100 章模拟 manifest 下 prompt context 不线性增长全文。
- [x] 降低 ctx_size 后 prompt context 能按新预算裁剪。

风险：

- 过度压缩会导致漂移。需要保留角色权威表、当前卷目标、当前章目标、伏笔债务、最近 2-3 章摘要。

## Phase 9：Export 与 Chapter IO 性能治理

目标：

- 大项目导出稳定，Windows 用户能直接打开 txt。

建议实现：

1. `status_export.rs` 导出继续使用 manifest 权威状态。
2. txt/md 导出使用 streaming writer。
3. 每章完成后同步：
   - `exports/current.txt`
   - `exports/章节合集.txt`
4. 导出时不从正文 heading 反推标题。
5. 导出返回 artifact path，用于面板点击打开。

验收：

- [x] 章节标题修改后 export 使用新标题。
- [x] 正文 heading 错误不会污染 manifest。
- [x] 大项目导出不一次性拼接全文字符串。

风险：

- 每章同步全集 txt 可能在超长篇变慢。可在项目达到阈值后改成增量 index + 最终导出。

## Phase 10：测试矩阵重建

目标：

- 用测试保护这次重构，不再靠真实测试反复踩同一个坑。

测试分组：

```text
novel_studio_tests/
  action_surface.rs
  context_packaging.rs
  quality_gate_body.rs
  quality_gate_metadata.rs
  contract_leakage.rs
  character_drift.rs
  approval_truth.rs
  export.rs
  migration.rs
```

必须覆盖：

1. action schema 和 error guidance 一致。
2. internal compat actions hidden。
3. metadata-only repair keeps body unchanged。
4. contract premise direct leak blocks。
5. dramatized premise does not block。
6. world term not treated as character drift。
7. unknown important role warning, not blocker。
8. rejected/needs_revision chapters excluded from context。
9. approve applies settlement once。
10. export uses manifest title/chapter title.

验收：

- [x] 聚焦测试稳定通过。
- [x] 测试名不包含模型特例名。
- [x] 测试 fixture 不写死旧项目书名/角色名。

## Phase 11：真实面板回归矩阵

目标：

- 代码层测试后，必须用真实面板/真实 gateway 验证。

2026-06-22 真实 gateway 回归尝试：

- 已用 `BENSHU_DATA_DIR=/home/biubiuboy/BenShu/data` 启动 standalone gateway。
- `/health` 成功返回。
- `/api/system/local-model-stack` 显示主模型配置为本地 Qwen GGUF，model pool 当前未加载。
- 轻聊天 `/api/chat` 真实请求失败：gateway 仍请求旧 runtime host `http://172.18.176.1:28013/v1/chat/completions`，当前 bridge / llama-server 未启动或 resolver 未刷新。
- 因此本 Phase 只能记录为真实环境阻塞，不能标记为完成。继续回归前需要先通过面板/运行时控制闭环完成：选择模型 -> 启动 llama.cpp -> 获取实际 base_url -> 刷新 resolver -> reload BenShu/worker。

测试顺序：

1. 轻聊天。
2. 天气/价格/新闻短实时任务。
3. 写作泛化开场：
   - 用户：帮我写小说
   - 期望：轻量追问，不启动长任务。
4. 具体写作需求：
   - 用户：写异界修仙小说，每章2500字，至少5万字
   - 期望：生成可确认合同，不写正文。
5. 合同确认：
   - 用户：按这个开始，先写第一章
   - 期望：进入 write flow，不回 creation planning。
6. 第一章：
   - 期望：正文 artifact 生成，聊天只显示摘要、路径、审查状态。
7. 前三章：
   - 期望：角色名稳定，章节名合理，truth 只从 approved 章节更新。
8. 十章连续：
   - 一次性请求 10 章。
   - 分 10 轮每轮 1 章。
   - 比较两种路径是否同样稳定。

验收：

- [ ] 能写出第一章。（待真实面板/本地模型回归）
- [ ] 能写出前三章。（待真实面板/本地模型回归）
- [ ] 能写到十章。（待真实面板/本地模型回归）
- [ ] 用户能在聊天框看到自然进度。（待真实面板/本地模型回归）
- [ ] 正文不塞聊天历史。（待真实面板/本地模型回归）
- [ ] 导出路径可点击打开。（待真实面板/本地模型回归）

## 6. 建议实施顺序

优先级从高到低：

1. Phase 1：Action Surface 单一来源。
2. Phase 2：内部兼容 action 防漂移。
3. Phase 3：拆 `quality_checks.rs`，只移动不改行为。
4. Phase 4：合同泄露 gate 降误伤。
5. Phase 5：角色漂移接 `contract_terms` 权威视图。
6. Phase 6：metadata/body gate 边界测试补齐。
7. Phase 7：approval/truth 回归测试补齐。
8. Phase 8：context budget telemetry。
9. Phase 9：export 性能与 Windows artifact 路径。
10. Phase 10：测试矩阵拆分。
11. Phase 11：真实面板回归。

为什么这个顺序：

- 先修 action 和错误引导，是因为这会直接影响 LLM 自恢复。
- 再拆 quality checks，是因为继续加规则前必须先让质量门可维护。
- 再调合同泄露/角色漂移，是因为这两类最容易误伤第一章。
- 最后做性能、导出和面板真实回归，避免在不稳定结构上反复测。

## 7. 不建议做的事情

1. 不建议继续在 `quality_checks.rs` 里直接加更多词表。
2. 不建议把 `plan_chapter` 等内部兼容 action 加回公开 schema。
3. 不建议在 gateway 里判断小说章节、合同字段、书名质量。
4. 不建议为某个模型新增专属修复路径。
5. 不建议因为第一章失败就放宽所有质量门。
6. 不建议把合同泄露检测直接删掉。
7. 不建议把正文塞回聊天历史来提高“记忆”。

## 8. 完成定义

本计划完成时，必须同时满足：

1. `novel_studio` action surface 单一来源。
2. `input.rs` 错误引导不再漂移。
3. `quality_checks.rs` 不再是 3000+ 行单体热点。
4. 合同泄露检测不误伤正常设定叙事。
5. 世界术语不会被当作角色漂移。
6. metadata 问题不会重写正文。
7. 未批准章节不会污染 truth / context。
8. 大项目上下文不随全文线性增长。
9. txt 导出路径稳定可打开。
10. 聚焦测试通过。
11. 真实面板可完成至少前三章写作回归。

## 9. 当前待办清单

- [x] Phase 0：冻结审查基线。
- [x] Phase 1：Action Surface 单一来源。
- [x] Phase 2：内部兼容 Action 注释与防漂移测试。
- [x] Phase 3：Quality Checks 只移动拆分。
- [x] Phase 4：合同泄露 Gate 降误伤。
- [x] Phase 5：角色漂移只依赖合同术语权威视图。
- [x] Phase 6：Metadata Gate 与正文 Gate 再收紧边界。
- [x] Phase 7：Approval / Truth / Story Bible 不回退。
- [x] Phase 8：Context Packaging 预算治理。
- [x] Phase 9：Export 与 Chapter IO 性能治理。
- [x] Phase 10：测试矩阵重建。
- [ ] Phase 11：真实面板回归矩阵。（代码重构已就绪，需另起真实面板/本地模型回归执行）
