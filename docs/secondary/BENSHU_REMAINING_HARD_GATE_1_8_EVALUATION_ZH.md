# BenShu 剩余硬门 1～8 现状评估与整合决策

> 状态：第 1～8 项已按本文顺序实施；真实聊天测试按用户要求暂不执行
>
> 核对日期：2026-08-01
>
> 冻结基线：`d691ada fix(writing): stabilize chapter continuity and settlement`
>
> 核对范围：`crates/builtin-tools/src/tool/writing`
>
> 目标：先确认能力是否已经存在，再决定保持、原位替换、补齐或删除，禁止建立重复机制。

## 复核说明

本文已针对同一冻结基线完成第二轮“声明—调用链—持久化 owner”核对。第二轮纠正了初稿
中四个过度结论，并把三个遗漏纳入实施范围：

- 章节 hard policy 的类型核心已经有，但 workflow raw JSON 扫描和 legacy issues 仍能旁路
  `ChapterFinding::hard_blocking()`，所以不能写成已经统一。
- 章节长度 gate 当前取 `chapter.unit_count.max(scan.units)`，最终正文扫描值尚未成为唯一
  当前真值。
- `new_state_after_chapter` 是唯一 required outcome；HookSeed/HookPayOff/HookDefer 当前不是
  required recovery 清单。
- observer 的 high-risk 布尔值来自模型且默认 false，当前没有本地派生，不能直接宣称状态
  污染门已经完整。
- proposed delta 没有正文证据、不能绑定密封权威或非法延后 hook 时，复用结算层已有的
  `DependencyMismatch` 阻断 owner；变更标记为 `Rejected`，不写入后续状态，章节进入
  `state-repair-required`。不再另建 `StatePollution` 枚举或第二套状态污染门。
- durable progress 只保存循环中最后一章的 receipt 验证结果，approved prefix 的历史
  receipt 失败可能被后章覆盖。
- 跨模块 `Repairable/Degraded` 是决策语义，不要求在章节侧再建一套枚举；章节侧复用现有
  `DeterministicRepair/Warning`。

以下各节保留冻结基线上的问题描述与实施决策，便于核对改动来源。生产代码按该顺序
原位修复；验证结果记录在下一节。

## 实施结果（2026-08-01）

- 第 1～7 项均在本文指定的既有 owner 中原位升级，没有建立第二套合同 gate、章节 gate、
  状态机、修订器、元数据存储或进度 ledger。
- 第 8 项已删除被 typed policy 接管的字符串判断、无生产者 hard code、Character 专用恢复、
  observer 自报高风险布尔值路径以及元数据 terminal blocker。
- `FullLongformContract` 已退出生产路径；测试侧只保留它作为历史强字段校验夹具，不参与
  创建、修复、确认或写作运行时决策。
- 章节档位/总字数混合解析、空批准载荷、用户书名与角色姓名权威保留、越权 typed delta
  均有定向回归覆盖；定向测试通过。
- 完整回归结果必须以当前工作树最后一次运行的实际数字为准；本文件不再提前宣称
  `0 failed`。`cargo fmt --all -- --check` 与 `git diff --check` 均通过。
- 第 15.2 节真实聊天测试未运行。

## 0. 先纠正两个项目约束

### 0.1 小说章数不是固定 40 章

当前正确规则已经存在于 `longform_policy::expected_chapter_count`：

```text
expected_chapters = ceil(target_units / chapter_unit_target)
```

- `target_units` 是用户在建书时指定的任意正整数。
- 新建/用户合同的 `chapter_unit_target` 只允许 2500 和 5000 两档；通用计算 helper
  仍可读取历史或迁移中的其他正值，因此生产路径必须先完成档位归一化，不能把 helper
  的通用性误当成第三个用户档位。
- 10 万字、2500 档会得到 40 章，但 40 只是该合同的计算结果，不是系统常量。
- 100 万字、5000 档会得到 200 章。
- 其他总字数按同一公式计算，不得写死成 40、80、200 或任何固定章数。

项目完成也不是“写到某个固定章号”，而是：

```text
磁盘连续 approved 章节正文的累计有效字数 >= target_units
```

这一点由 `novel_studio/chapter_state.rs` 的 durable progress 负责，必须保持。

### 0.2 “硬阻断”和“有限恢复”必须分开

本文统一使用四类结果：

| 类型 | 含义 | 是否可进入下一章 |
| --- | --- | --- |
| `HardBlock` | 已有确定证据证明合同不可满足、正文污染后续状态或破坏连续性 | 否 |
| `Repairable` | 有限次数内可通过本地修复、补写、重新解析或同正文重试恢复 | 修复完成后可以 |
| `Degraded` | 模型格式或展示元数据失败，但正文与旧状态未被证明错误 | 可以在安全回退成功后继续 |
| `Advisory` | 主观分数、审美意见、轻微格式偏好和非确定性推断 | 是 |

不能再把“模型没有按 JSON schema 回答”直接等同于“小说状态已经污染”。

上表是跨模块的决策语义，不等同于要求新增同名枚举。合同侧可以使用计划中的
`ContractIssueDisposition::Repairable`；章节侧应复用现有
`ChapterFindingDisposition::DeterministicRepair` 和 `Warning`，workflow 只需记录
有限恢复是否耗尽，不能再造一套平行的 `Repairable/DegradedAccepted` finding 类型。

---

## 1. 总体结论

8 项中没有一项需要新建平行系统。现有代码已经具备主要基础设施：

- 合同有 typed issue、field strength、readiness scope、有限 patch budget 和净提升判断。
- 章节有 typed finding、typed disposition、证据等级和 `hard_blocking()` 核心；但 workflow
  仍存在绕过 typed evidence 的 JSON 字符串扫描和 legacy issues 兜底，尚未真正统一。
- 章节有有限扩写、有限语义修订、最佳候选持久化和非提升回滚。
- 状态结算有 sealed authority、最终正文证据、typed delta、allowance 和 pending settlement。
- 元数据有独立修复链，不需要改写正文。
- 批准有事务日志、before image、receipt、指纹链和崩溃恢复。
- 进度有磁盘连续 approved 正文权威；但 receipt 完整性目前只保留循环中最后一章的
  检查结果，尚未验证整个 approved prefix 的 receipt 链。

真正缺少的是：

1. 合同 issue 没有 typed disposition。
2. 章节 hard policy 在 typed finding 之外又维护了一份字符串代码表。
3. 轻微字数不足仍被定义成 `HardBlock`。
4. observer 格式失败、展示元数据失败和真实状态污染共用失败出口。
5. required end-state 的本地恢复只覆盖 Character。
6. 元数据五轮未收敛后仍终止整章。
7. 磁盘进度虽按连续正文计数，但历史 approved 章节的 receipt 缺失或正文不匹配可能被
   后续正常 receipt 覆盖。

因此总体决策不是“大量新增”，而是：

```text
保留现有 owner
→ 扩展现有类型表达
→ 把错误调用路径迁回唯一 owner
→ 删除被替换的字符串判断、终止出口和无生产者代码
```

## 2. 1～8 决策总表

| # | 目标 | 当前是否已有 | 决策 |
| --- | --- | --- | --- |
| 1 | 合同 issue 使用 typed disposition，并只保留一个合同 readiness owner | 部分已有 | 保留 typed issue/evidence/scope；扩展现有 `ContractIssue`；替换字符串严重度与重复 readiness 判定 |
| 2 | 章节硬门只由一个 typed policy 决定 | 核心已有、调用边界未统一 | 保持 `ChapterFinding::hard_blocking()`；删除 `HARD_BLOCKER_CODES`、raw JSON hard 扫描和 legacy 无证据兜底 |
| 3 | 字数走有限补写、轻微不足回滚 best、档位上限硬阻断 | 大部分已有 | 保留扩写和 best candidate；替换低于目标即硬阻断的定义；统一三套阈值 |
| 4 | 区分 observer 格式失败与真实状态污染 | 部分已有 | 保留五轮重试、pending settlement 和旧 truth 隔离；扩展现有 validation outcome；替换统一 `state_repair_required` 出口 |
| 5 | required state recovery 不再固定为 Character | 基础已有 | 只泛化 `new_state_after_chapter` 的唯一可解析类型；其余字段仍是允许上限，不建第二套恢复器或把全部字段升级为必需项 |
| 6 | 元数据五轮后选择 best 或确定性回退，不终止正文 | 大部分已有 | 保留独立 metadata repair；接通现有 best/local projection；删除 metadata terminal blocker |
| 7 | 批准事务、指纹、章节连续性和磁盘进度保持原 owner | 核心已有，approved-prefix receipt 链不完整 | 不重构 owner；在 `chapter_state.rs` 原位补齐逐章 receipt 验证并补回归测试 |
| 8 | 删除替换后的旧判断、死分支和无生产者代码 | 明确存在清理对象 | 随 1～7 迁移后逐项删除，不能先删后接线 |

---

## 3. 第 1 项：合同 issue typed disposition 与唯一合同 gate

### 3.1 当前已经有什么

现有 owner：

- `creation_contract/issue.rs`
  - `ContractIssue`
  - `ContractIssueKind`
  - `ContractIssueEvidence`
  - `ContractIssueList`
  - `ContractIssueSet`
- `typed_contract_gate.rs`
  - 声明自己是结构化小说合同 readiness 的唯一 owner。
- `creation_contract/patch.rs`
  - `PatchFieldStrength`
  - `ContractReadinessScope`
  - `blocks_for_scope`
- `creation_contract/repair_coordinator.rs`
  - typed issue 驱动的 patch 路由。
  - `ContractRepairProgressSnapshot` 净提升判断。
  - 当前模型 patch 绝对上限为 5 次。

这些能力方向正确，必须保留。

### 3.2 当前缺口与冲突

`ContractIssue` 当前只有：

```rust
code
kind
evidence
text
```

没有 disposition。`ContractIssueSet::actionable()` 只能用：

```text
非 Diagnostic = actionable
```

这无法表达：

- 确定不可满足的合同冲突。
- 可以有限补齐的缺失字段。
- 只影响展示质量的建议。
- 仅用于排障的诊断。

同时 `creation_contract/generated_gate.rs` 仍有：

```rust
contract_quality_issue_is_blocking(issue: &str)
```

它通过中文字符串判断“混入面板说明”“连续重复退化”“异常下划线”等是否 blocking。
这意味着同一事实只要换一种文案，就可能改变 gate 结果。

此外 readiness 仍分散在：

- `typed_contract_gate`
- `creation_draft_visible_approval_readiness_issues`
- `novel_bible::story_contract_blockers`
- `novel_bible::story_bible_audit`
- `reporting::governed_project_readiness_blockers`

这些层并非全部错误，但它们重复判断“主题、角色、世界规则、风格、终局、角色锚点是否
齐全”，使创建阶段已通过的合同在运行阶段可能被兼容镜像再次否决。

当前合同 gate 还把下列字段广泛当作 readiness 条件：

- 书名理由和读者钩子质量。
- 非主角关键角色。
- 每个角色的欲望、恐惧、底线和弧线。
- 主题、风格、must_avoid、世界观意象。
- 分卷、近期章节、结构化治理和部分审美字段。

其中有些是防漂移所需权威，有些只是可以滚动补齐的丰富度，不应全部成为终止性硬门。

### 3.3 决策

**保留：**

- `ContractIssue`、`ContractIssueKind`、`ContractIssueEvidence`。
- `ContractIssueList` 的排序、去重和证据字段。
- `typed_contract_gate` 作为唯一 readiness owner。
- `PatchFieldStrength` 和 `ContractReadinessScope`。
- 现有 repair coordinator、patch owner 和净提升判断。

**扩展现有类型，不新增第二套 issue：**

给 `ContractIssue` 增加 typed disposition，例如：

```rust
enum ContractIssueDisposition {
    HardBlock,
    Repairable,
    Advisory,
    Diagnostic,
}
```

生产者在创建 issue 时明确 disposition，不能由 `text` 推断。

注意现有 `ContractIssueKind::Diagnostic` 把“问题归属”与“处理方式”混在同一轴。增加
disposition 后，`ContractIssueKind` 应只表达 Skeleton/Characters/Plot/Governance/Other
等 owner/domain；现有 Diagnostic kind 迁移成实际 domain 加 `Diagnostic` disposition，
所有调用迁完后删除该 kind variant。否则会出现“kind 是 Diagnostic、disposition 又是
HardBlock/Repairable”的冲突组合，形成新的重复机制。

**原位替换：**

- `ContractIssueSet::actionable()` 改为只读取 disposition。
- `ContractGateResult` 从 typed issue 生成 transport 字段。
- `contract_quality_issue_is_blocking` 删除。
- `creation_draft_visible_approval_readiness_issues` 不再建立第二套最终 readiness；
  只保留用户展示投影，来源必须是 typed gate 的结果。
- `story_contract_blockers` 和 `story_bible_audit` 在运行阶段只检查存储完整性、必要
  权威存在及镜像一致性，不再重新评价创建合同的丰富度。

### 3.4 disposition 应如何分配

#### `HardBlock`

只允许确定性合同错误：

- 用户明确数值权威互相矛盾或被改写。
- `chapter_unit_target` 不是 2500/5000。
- 总字数不是正整数。
- 角色同一稳定 ID 同时绑定两个互斥身份/姓名。
- 合同存在无法同时满足的终局、角色身份或世界硬规则冲突。
- 合同正文被 JSON、工具回执、控制面文本或结构污染破坏到无法解析。
- 近期章节编号跳号、重号或不从 1 开始，且不能确定性修复。

#### `Repairable`

- 缺书名、书名理由、主角锚点、终局方向、世界规则或首批章节窗口。
- 缺少当前题材要求的结构化字段。
- 角色欲望、恐惧、底线不完整。
- 关系、伏笔、分卷或审美字段不够丰富。
- 可通过 typed patch 或本地 normalizer 确定性补齐的问题。

`Repairable` 可以阻止“立即锁定合同”，但不能伪装成系统终止错误；只能进入有限修复。

#### `Advisory`

- 书名读者钩子不够强。
- 风格、场景配比、母题、角色声音表等审美丰富度不足。
- 非确定性重复、泛化或主观质量问题。

#### `Diagnostic`

- parser/runtime 失败。
- patch scope 噪声。
- 调试用候选边界说明。

### 3.5 最小可执行合同与滚动丰富度

`LockedAuthorityContract` 应只要求能安全开始第一批章节的最小权威：

- 任意正整数总字数。
- 2500/5000 章节档位。
- 唯一项目/合同 ID 与版本。
- 书名或可确定生成书名的故事依据。
- 明确主角身份和稳定姓名。
- 故事前提、终局方向、主线因果。
- 必要世界硬规则。
- 从第 1 章开始的连续近期章节窗口。

其余结构化字段按照 `PatchFieldStrength` 在滚动阶段补齐。不能为了启动写作，要求模型
一次生成整本书全部审美细节。

### 3.6 应删除什么

在 typed disposition 全部接线并通过测试后删除：

- `contract_quality_issue_is_blocking`
- 依靠 `ContractIssueKind != Diagnostic` 推导所有 actionable 的逻辑
- disposition 接管后语义重复的 `ContractIssueKind::Diagnostic` variant
- 用户展示层重复计算最终 readiness 的路径
- StoryContract/StoryBible 对已锁定 canonical contract 的重复丰富度否决
- 仅供已弃用 `FullLongformContract` 路径使用且无生产调用的分支

### 3.7 验收

- 同一 issue 改写中文文案不改变 disposition。
- advisory-only 合同可以进入用户确认。
- repairable 合同最多走现有有限预算，不进入无限/长轮次自修。
- 用户总字数和章节档位任何时候都不能被 patch 改写。
- 已锁定 canonical contract 不因兼容镜像少一个非必要字段被运行阶段否决。

---

## 4. 第 2 项：统一章节 typed hard policy

### 4.1 当前已经有什么

`chapter_quality.rs` 已经提供正确核心：

- `ChapterFindingClass`
- `ChapterFindingDisposition`
- `FindingEvidenceGrade`
- `AuthorityEvidenceRef`
- `BodyEvidenceSpan`
- `ChapterFinding::hard_blocking()`

其中语义 finding 只有同时具备：

- 非空 authority fingerprint。
- 非空 body fingerprint。
- authority evidence。
- body evidence。

才允许 hard block。这个机制必须保持，是当前防内容漂移的核心。

`novel_studio/quality_gate.rs` 也已经把大部分本地检查转为 typed finding：

- 正文污染、占位和截断。
- 人物身份、未登记人物、人物代词。
- 世界硬规则。
- 字数。
- 元数据。
- 跨章完全重复。

### 4.2 当前重复机制

`novel_workflow_driver/quality.rs` 又维护：

```rust
const HARD_BLOCKER_CODES: &[&str]
```

并在 `validate_llm_authority_conflict` 中再次：

- 判断代码是否在字符串白名单。
- 根据字符串 code 重新决定 finding class。
- 重新构造一份 hard finding。

这与 `ChapterFinding::hard_blocking()`、本地 finding producer 和已有 class/disposition
重复。

更严重的是，当前白名单中有多项没有生产 finding 的本地 owner，例如：

- `character_name_replacement`
- `relationship_state_conflict`
- `timeline_conflict`
- `location_continuity_conflict`
- `ability_or_resource_conflict`
- `chapter_goal_replaced`
- `unplanned_main_branch`
- `premature_hook_payoff`
- `unsupported_hook_resolution`
- `state_change_outside_execution_contract`
- `authority_fingerprint_mismatch`
- `state_validation_failed`

这些代码目前只存在于 allowlist、revision prompt、质量向量或测试分支，不能被
`locally_confirmed_codes` 真正确认，属于悬空策略或死路由。

此外还有两条绕过 typed hard policy 的生产路径：

- `ChapterQualityGate::hard_blocking()` 在 `findings.is_empty() && !issues.is_empty()` 时
  直接 hard block。它是 legacy 兼容兜底，但生产数据一旦只带旧 `issues` 文本，就绕过
  disposition、evidence grade 和双侧证据。
- `novel_workflow_driver/quality/issue_classification.rs::value_has_hard_findings` 只扫描 JSON
  中是否出现 `"disposition":"hard_block"`，没有反序列化 `ChapterFinding`，也没有调用
  `ChapterFinding::hard_blocking()`。因此一个缺少 evidence 的 semantic finding 仍可能在
  workflow 边界被当成 hard。

还有一项现有 producer 的证据等级过强：`contract_must_avoid_issues` 只要正文包含
`must_avoid` 的原始字符串，就把 `world_rule_conflict` 标成
`DeterministicInvariant/HardBlock`，但没有 authority/body evidence ref。明确的控制词、
禁用标记可以做确定性字符串阻断；自然语言世界禁令属于语义冲突，必须满足双侧证据，
不能仅靠 substring 自动升级为确定性 hard。

### 4.3 决策

**保持不动：**

- `ChapterFinding` 的类型和证据结构。
- `ChapterFinding::hard_blocking()` 作为唯一 hard 判定。
- `novel_studio/quality_gate.rs` 作为本地 finding producer owner。
- LLM audit 只能提供建议或补充证据，不能独立创造 hard code。

**原位替换：**

`validate_llm_authority_conflict` 不再读取 `HARD_BLOCKER_CODES`，而是：

1. 从当前本地 typed findings 找到同一 code/class 的 finding。
2. 验证 LLM 提供的 authority path、authority excerpt 和 body excerpt。
3. 只为同一 typed finding 补充证据或说明。
4. disposition 和 class 继承本地 finding，不能由字符串 `match code` 重建。

如果本地没有 finding，LLM 输出只能进入 advisory。

workflow 边界必须反序列化现有 `ChapterFinding` 并调用
`ChapterFinding::hard_blocking()`；不能只检查 disposition 字符串。legacy `issues` 仅用于
只读展示/迁移：迁移器能够重建 typed finding 时才进入 typed policy，无法重建证据时只能
降为 advisory，不能保留无证据 hard 旁路。

`must_avoid` 应在同一 producer 内按证据性质分层：

- 精确控制标记、禁止占位符等本地可判定项使用 `DeterministicInvariant`。
- 人物/世界/情节类自然语言禁令使用 `EvidenceBackedSemantic`，补齐 authority path 与
  body span 后才能 hard。
- 模糊命中或只存在关键词时是 `Warning`，交给有限修订或用户可见审查，不得硬阻断。

### 4.4 应删除什么

- `HARD_BLOCKER_CODES`
- `validate_llm_authority_conflict` 内按 code 重建 class 的大 `match`
- 无本地 producer 的 revision prompt 分支和质量向量分支
- 通过 JSON 中任意 `"disposition":"hard_block"` 字符串直接决定策略的重复入口；
  workflow 边界应反序列化现有 typed finding，legacy 数据只读兼容
- `ChapterQualityGate::hard_blocking()` 中“没有 typed findings 但任意 legacy issues 即 hard”
  的生产兜底；若保留解析，只能放在显式 legacy migration 层

不能删除仍有生产者的代码，例如：

- `character_identity_conflict`
- `unregistered_character`
- `character_pronoun_conflict`
- `world_rule_conflict`
- `future_chapter_consumed`
- `body_truncated`
- `body_surface_contamination`
- `length_above_tier_maximum`
- `cross_chapter_exact_duplicate`

### 4.5 验收

- 任意 free-text audit、score 或未知 code 不能 hard block。
- 缺少 typed evidence 的 legacy issue 或 raw JSON hard disposition 不能 hard block。
- 同一 typed finding 在 writer、auditor、reviser 和 approval 中 disposition 一致。
- 删除无生产者 code 后，revision prompt 不再出现永远无法生成的专用分支。
- 有双侧证据的人物身份冲突、未来章节提前消费和世界硬规则冲突仍然阻断。
- 自然语言 `must_avoid` 的单纯 substring 命中不能伪装成确定性世界规则冲突。

---

## 5. 第 3 项：统一字数策略

### 5.1 当前已经有什么

现有能力应复用：

- `expand_short_chapter_if_needed`
  - 有界扩写。
  - 最多 5 次模型调用尝试；代码会拒绝重复或无增量片段，但不能保证模型五次输出都
    彼此不同。
  - 拒绝重复、过短、未增加正文或破坏尾部的片段。
- `chapter_expansion_round_budget`
  - 2500 档最多 2 个有效扩写 round。
  - 5000 档最多 3 个有效扩写 round。
- `BoundedRevisionCycle`
- `DraftCandidateRecord`
- `RevisionQualityVector`
- `candidate_is_strict_improvement`
- `persist_best_draft_candidate`
- 非提升候选回滚。
- 档位最大值：
  - 2500 档最大 5000。
  - 5000 档最大 10000。

因此不需要新建扩写器、长度修订器或 best selector。

### 5.2 当前冲突

当前存在三套口径：

1. `novel_studio/quality_gate.rs`
   - 少于 `chapter_unit_target` 1 个单位也产生 `length_below_minimum: HardBlock`。
2. `novel_workflow_driver::minimum_chapter_units`
   - 80% 用于判断旧草稿是否可复用。
3. `novel_studio/reporting.rs`
   - 低于目标 1/3 才报告“far below” warning。

最终 gate 使用 100%，所以即使正文 2499/2500，有限扩写失败后仍会终止章节。

此外 `only_small_length_shortfall` 会先执行有限补写，但补写后只要仍低于 100%，最终
`body_revision_required_after_audit` 仍会返回 blocker。现有 best candidate 只能保存
版本，不能批准轻微不足版本。

长度真值还有一处不一致：`chapter_length_findings` 当前使用：

```rust
measured_units = chapter.unit_count.max(scan.units)
```

如果记录中的 `chapter.unit_count` 是旧值或错误地偏大，它会掩盖最终正文扫描出的实际
短缺。批准前的当前真值必须是最终正文 `scan.units`，并用它回写记录；不能用缓存字段和
正文扫描值取较大者。磁盘 durable progress 已经重新读取正文计数，这一原则应前移到
quality gate。

### 5.3 决策

**保持：**

- 两档目标值和两档 hard max。
- 当前有界扩写函数、attempt budget、片段去重和尾部完整性检查。
- 当前 best candidate、净提升和回滚。
- 总字数按 approved 正文实际字数累计，不伪造 unit_count。

**替换当前长度 finding 语义：**

建议在现有 `chapter_length_findings` 内统一为：

| 条件 | disposition |
| --- | --- |
| 正文为空、明显截断或低于可用下限 | `HardBlock`，但 code 属于 body integrity，不再伪装成普通轻微不足 |
| 低于目标但达到可用下限 | 现有 `ChapterFindingDisposition::DeterministicRepair`，触发现有有界 top-up |
| top-up 后仍轻微不足，且正文完整、无其他 hard finding | 现有 `Warning` 加 typed workflow recovery outcome，选择现有 best 后允许继续 |
| 超过 2500 档 5000 或 5000 档 10000 | `HardBlock` |

“可用下限”应由一个函数统一，不能继续保留 100%/80%/33% 三套标准。建议保留并重命名
现有 `minimum_chapter_units(target)` 为唯一底线 owner；具体比例在实施时用跨题材样本
校准，但不得新增题材特例。

推荐初始策略：

```text
target = 2500/5000
soft target = target
usable floor = 80% * target
hard max = 2 * target
```

80% 不是自动批准条件，还必须同时满足：

- 正文不是摘要、占位或截断。
- 没有 JSON、工具回执或控制文本。
- 没有合同、身份、连续性或状态 hard finding。
- 没有跨章完全重复。
- 现有 best candidate 确实是净提升或不劣版本。

### 5.4 应删除什么

- `length_below_minimum` 作为一律 `HardBlock` 的旧定义。
- `chapter.unit_count.max(scan.units)` 的 gate 计量方式；改为最终正文扫描值并同步记录。
- `reporting.rs` 独立的 `target / 3` 口径。
- `draft_output_fallback_body_is_usable` 内再次手写 80% 的计算。
- `length_shortfall_node` 对英文错误文案的生产策略判断；legacy JSON 可只读兼容。
- 任何按题材单独设置不足比例的特例。

### 5.5 验收

- 2490/2500 的完整正文不会因补写器格式失败而终止。
- 低于统一 usable floor 的正文仍不能批准。
- 2500 档超过 5000、5000 档超过 10000 始终 hard block。
- 轻微不足版本必须经过现有 best selector，不能直接取最后一次输出。
- gate、approved `unit_count` 和磁盘进度都使用同一最终正文实际计数，整书仍写到累计
  目标字数。

---

## 6. 第 4 项：区分 observer 格式失败与状态污染

### 6.1 当前已经有什么

现有正确机制：

- `MAX_FINAL_STATE_OBSERVER_ATTEMPTS = 5`
- 每次都读取同一最终正文和 sealed observer projection。
- `parse_final_chapter_observation` 做 JSON shape normalizer。
- `legacy_zero_change_degraded_settlement` 会构造零变化 degraded settlement。
- settlement 先写 pending，不会在批准前提交 truth。
- `validate_final_body_evidence` 验证正文精确 span。
- `authority_allowance` 验证状态变化是否被章节权威允许。
- 未通过的 typed delta 被丢弃，不会污染 truth。
- approval transaction 只提交通过验证的 settlement。

这些已经满足“失败时保留旧状态、防止污染”的大部分要求。

### 6.2 当前错误

目前以下情况全部会让 `StateValidationOutput.passed = false`：

- observer JSON 解析失败。
- `current_state` 为空。
- `chapter_summary` 为空。
- degraded reason 非空。
- required end-state delta 缺失。
- body/authority fingerprint 不一致。
- 真实越权或无证据状态变化。

随后 `state_truth.rs` 把章节统一标成：

```text
state_repair_required
```

因此：

- 模型少一个展示摘要字段，等同于状态污染。
- observer 第 5 次仍输出坏 JSON，正文即使完全正确也无法批准。
- 用户无法区分“模型格式失败”和“状态证据冲突”。

这里还需要纠正两个更细的代码事实：

1. `validate_final_body_evidence` 或 `authority_allowance` 失败的 proposed delta 当前会被丢弃
   并只记 advisory，本身不会必然阻断。这一行为适合“observer 幻觉且正文没有证据”的
   情况，不能把所有被丢弃 delta 都升级为阻断。
2. 当前高风险判断 `state_change_claims_forbidden_transition` 依赖 observer 输出的
   `changes_identity`、`changes_core_ability`、`changes_world_hard_rule`、
   `pays_future_hook_early` 等布尔值。它们 serde 默认是 `false`，observer prompt 的严格
   输出形状也没有要求这些字段，`bind_contract_authority` 不会从权威路径或正文证据推导
   它们。因此模型漏报布尔值时高风险变化可能绕过该检查；模型误报 `true` 时又会在正文
   证据验证之前直接 hard block。

### 6.3 决策

**保持：**

- 五次有限 observer 重试。
- sealed authority 与最终正文固定不变。
- pending settlement。
- 旧 truth 不提交、不覆盖。
- body/authority fingerprint mismatch 始终 hard。
- 越权 typed delta、伪造证据和明确缺失 required outcome 仍 hard。

**扩展现有 validation，而不是新建状态机：**

在现有 `StateValidationOutput` 或同一模块内增加 typed outcome，例如：

```rust
enum StateSettlementDisposition {
    Ready,
    ObserverFormatDegraded,
    DisplayMetadataDegraded,
    RequiredStateMissing,
    DependencyMismatch,
}
```

分类原则：

- `ObserverFormatDegraded`
  - JSON/schema/字段类型失败。
  - 不代表正文错误。
  - 五次后进入本地安全回退。
- `DisplayMetadataDegraded`
  - `current_state`、`chapter_summary` 或 continuity display 缺失/不佳。
  - 由本地最终正文投影补齐。
- `RequiredStateMissing`
  - sealed contract 明确要求章末变化，但最终正文没有可验证证据。
  - hard block。
- `DependencyMismatch`
  - settlement 绑定的正文或 authority fingerprint 不一致。
  - hard block。

### 6.4 安全回退

observer 格式失败五次后：

1. 不使用生成阶段的状态声明。
2. 不提交任何未经证据验证的 typed delta。
3. 从旧 approved truth 复制 as-of state 作为基线。
4. 用最终正文的本地投影生成展示摘要；展示失败只记 warning。
5. 若章节合同没有 required state change，零 delta settlement 可以通过。
6. 若合同存在 required state change，调用第 5 项的通用证据恢复。
7. 恢复成功则通过；恢复失败才标记 `state_repair_required`。

若 observer 只是声称了越权变化，但最终正文无证据，则丢弃该 delta 并记 advisory，不应
把 observer 幻觉升级成正文污染。反过来，正文与权威双侧证据证明存在越权变化时，即使
observer 漏填所有风险布尔值也必须由本地规则识别并阻断。

observer 五次格式失败也意味着部分语义审查覆盖不可用，例如 future-boundary 的模型证据
可能缺失。系统应记录 typed degraded coverage，不能伪称完成了完整语义审查；但“没有拿到
observer 证据”本身也不能证明正文提前消费未来章节。本地已确认 finding 仍按原规则 hard，
未获得证据的语义项保持未知/advisory。

### 6.5 应删除什么

- `degraded_reason` 非空就自动令 validation fail 的规则。
- `current_state`/`chapter_summary` 缺失就自动等同状态污染的规则。
- observer error 和 state pollution 共用同一个终止文案的出口。
- 对 optional typed delta 缺失的硬要求。
- 直接信任 observer `changes_*`/`pays_*`/`opens_*` 布尔值决定 high-risk hard 的路径；
  改为在现有 settlement validator 中本地派生。

`state_repair_required` 状态本身不能删除；它只用于真实 required state 缺失或依赖指纹不匹配。
最终正文中有权威证据的越权身份、能力、世界硬规则或未来伏笔变化由现有章节质量门阻断，
不在 settlement 中新增第二个状态污染出口。

### 6.6 验收

- observer 连续 5 次坏 JSON，但章节无 required state change 时，可以以零 delta、
  旧 truth 和本地展示投影批准。
- observer 提议且最终正文/权威双侧证据确认越权人物身份变化时，不提交 delta，且仍阻断。
- observer 无证据地声称越权时只丢弃；最终正文有双侧证据的越权变化即使模型风险布尔值
  全为 false 仍阻断。
- settlement body/authority fingerprint 不匹配时仍阻断。
- parser error 不再显示成“正文状态污染”。
- degraded semantic coverage 被明确记录，不得作为“已通过完整观察”的伪成功状态。
- 下一章只能读取 approved truth，不能读取失败 observer 输出。

---

## 7. 第 5 项：`new_state_after_chapter` 的通用 required state recovery

### 7.1 当前已经有什么

`novel_studio/settlement.rs` 已有可复用基础：

- `authority_values`
  - 已覆盖 Character、Relationship、World、Power、Resource、HookSeed、
    HookAdvance、HookDefer、HookPayOff。
- `authority_entity_resolution`
- `validate_final_body_evidence`
- `authority_allowance`
- `contract_change_supported_by_final_evidence`
- `final_body_evidence_spans`
- `dedupe_required_end_state_changes`

因此不应新增第二套人物/世界/伏笔证据匹配器。

但“`authority_values` 覆盖某个 event type”只表示该类型可以被章节合同授权，不表示它是
本章必须产生的状态变化。当前 writer/observer prompt 已明确：

- `chapter_contract.new_state_after_chapter` 是唯一 required outcome。
- 其他非空 typed 字段只是允许上限，不是强制 checklist。

其中 `new_state_after_chapter` 目前只被 `authority_values` 暴露给 Character、Relationship、
World、Power、Resource 和 HookAdvance；HookSeed、HookPayOff、HookDefer 分别由
`hook_opened/*`、`hook_paid_off/*`、`payoff_target` 授权，不能把它们误写成当前 required
recovery 的覆盖范围。

### 7.2 当前缺口

observer 尝试耗尽后的本地恢复只有：

```rust
recover_explicit_required_character_state_change
```

它把 `chapter_contract.new_state_after_chapter` 固定恢复成：

```rust
ChapterStateEventType::Character
```

这会导致同一个 required end-state 如果实际描述的是：

- 关系变化。
- 世界规则/地点状态变化。
- 能力阶段变化。
- 资源持有变化。
- 唯一既有伏笔的推进。

则无法使用已经存在的 `authority_values` 和 allowance 完成恢复。

更准确地说，当前专用函数一定写成 Character；但现有合同又没有单独的
`required_event_type` 字段，`authority_event_for_path` 对
`chapter_contract.new_state_after_chapter` 也返回 `None`。因此泛化后不能只靠 path 直接
判断它是关系、世界、能力、资源或 HookAdvance；必须从现有权威实体、配套 typed 字段和
正文证据中得到唯一解，不能用关键词猜类型。

### 7.3 决策

**保持：**

- 所有现有 evidence span、entity resolution、allowance 和去重机制。
- `new_state_after_chapter` 是 required outcome。
- 其他非空 typed contract 字段默认是允许上限，不应全部自动变成硬性 checklist。

**原位泛化：**

把 `recover_explicit_required_character_state_change` 替换为同模块内的：

```text
recover_explicit_required_state_change
```

它按以下顺序工作：

1. 读取 required authority path/value。
2. 只枚举当前确实允许 `new_state_after_chapter` 的 event type：Character、Relationship、
   World、Power、Resource、HookAdvance。
3. 使用现有 entity resolution 找唯一实体。
4. 结合现有配套 typed 字段和 canonical entity domain 消除 event type 歧义；不能根据
   “关系”“升级”“获得”等词直接分类。
5. 使用现有 final-body evidence spans 找唯一、正文可见、语义支持的 span。
6. 调用现有 `validate_final_body_evidence`。
7. 调用现有 `authority_allowance`。
8. 只有 event type、实体和证据均唯一时恢复 typed delta。
9. 多义、无实体或无证据时不猜测，返回 `RequiredStateMissing`。

如果跨题材样本证明现有字段无法稳定得到唯一 event type，允许的最小结构变更是在现有
`ChapterExecutionContractV2` 中增加一个 typed required-event 描述，并让 sealed authority、
writer、observer、settlement 共用它；这属于补齐现有合同，不是新增平行恢复机制。没有该
证据前不得预先增加字段，也不能默认为 Character。

其他 `character_change`、`relationship_delta`、`world_change`、`power_delta`、
`resource_delta`、`hook_opened`、`hook_paid_off` 和 `payoff_target` 仍是 optional allowed
maxima。系统可以在正文证据和权威解析均唯一时确定性恢复这些 optional delta，但缺失时
不得触发 `RequiredStateMissing`，除非未来合同 schema 显式把某一项标为 required。

Hook 事件继续使用现有 hook ID/path，不允许用常见词或 bigram 猜测回收。

### 7.4 不能做什么

- 不能为玄幻、言情、赛博朋克等题材分别写 event type 特例。
- 不能仅凭 `new_state_after_chapter` 中出现“关系”“升级”“获得”等关键词直接提交。
- 不能把 observer 生成的 `value` 当事实。
- 不能从 summary、key_facts 或 continuity metadata 结算 durable truth。
- 不能在恢复失败时自动制造一个 Incidental delta。
- 不能把 `hook_opened`、`hook_paid_off` 或 `payoff_target` 的存在自动升级成 required
  HookSeed/HookPayOff/HookDefer。

### 7.5 应删除什么

- Character 专用 required recovery 函数。
- 只为 Character 写的专用测试，改为 required path 参数化测试与 optional path 独立测试。
- 与通用 `authority_values` 重复的字段枚举。

### 7.6 验收

required `new_state_after_chapter` 参数化覆盖：

- Character
- Relationship
- World
- Power
- Resource
- HookAdvance

每类都验证：

- 唯一证据可以恢复。
- 多义证据不能恢复。
- 权威未许可不能恢复。
- 正文不含实体不能恢复。
- future hook 不能提前回收。

另行覆盖 HookSeed、HookPayOff、HookDefer 的 optional 授权与证据验证，确认它们在未被
schema 标为 required 时，observer 漏报不会把章节错误升级成 hard block。

---

## 8. 第 6 项：元数据有限修复后的 best/fallback

### 8.1 当前已经有什么

现有机制已经较完整：

- `MAX_METADATA_REPAIR_ATTEMPTS = 5`
- 元数据修复与正文修订分开。
- `metadata_repair_prompt` 明确禁止重写正文。
- title candidates 会去重和排除已拒标题。
- candidate-only metadata gate 会选择 title issue 更少的候选。
- metadata candidate 会进入现有 `reconcile_submitted_candidate`。
- `DraftCandidateRecord` 已同时保存正文、元数据和 fingerprint。
- approval 侧已有 `settlement_display_metadata_or_body_validated_best`，会比较现有元数据和
  settlement 投影。
- `repair_latest_chapter_metadata` 已有本地标题、摘要和连续性修复函数。

因此不能新建新的 metadata candidate store 或新的元数据质量门。

### 8.2 当前错误

如果五次仍未通过，当前会调用：

```text
format_metadata_blocker_result
```

并终止整章。

另外 approval 在 settlement display metadata 仍有 issue 时会返回：

```text
approval_requires_metadata_repair
```

这使“标题不够好、摘要缺失或模型 JSON 格式波动”仍可阻断已经通过正文、审稿和状态
证据的章节。

### 8.3 决策

**保持：**

- 5 次上限。
- 正文不可变。
- candidate-only 校验。
- 现有 best candidate 和 metadata fingerprint。
- 现有本地 title/summary/continuity 投影函数。

**接通现有机制：**

第五次后：

1. 从本轮 metadata candidates 选择 typed issue 最少的 best。
2. 如果 best 仍只有审美/展示问题，使用 best。
3. 如果缺少持久化必需字段，调用现有本地投影生成：
   - 非空、安全、无控制文本的标题。
   - 基于最终正文的简短摘要。
   - 只含正文可见事实的 continuity display。
4. 再跑一次现有 metadata gate。
5. 仍存在主观/审美问题时降为 advisory。
6. 只有元数据会破坏存储、路径、正文指纹或泄漏控制文本时才 hard block。

### 8.4 应删除什么

- 五次耗尽后直接 `format_metadata_blocker_result` 的生产出口。
- approval 对纯展示问题的 terminal `approval_requires_metadata_repair`。
- 与 `settlement_display_metadata_or_body_validated_best` 重复的 best 选择。
- 任何为了修标题而重新生成正文的路径。

`format_metadata_blocker_result` 如果无其他生产调用，应在迁移后删除；不能保留为默认
兜底，以免未来重新接回。

### 8.5 验收

- metadata 模型连续五次坏 JSON，正文 fingerprint 保持不变并可用本地 fallback。
- 标题、摘要和 continuity display 不泄漏 JSON、路径或工具回执。
- 元数据修复不能改变 state_changes、body fingerprint 或 authority fingerprint。
- 只有 metadata 审美 warning 的章节可以批准。
- 存储破坏或控制文本污染仍阻断。

---

## 9. 第 7 项：批准事务、指纹、连续性和磁盘进度保护区

### 9.1 当前机制是否已有

核心机制已经有，并且是当前最完整的部分，但不能写成“已完整存在”。

`novel_studio/approval_transaction.rs` 已有：

- before image。
- prepared/committed journal。
- transaction ID。
- interrupted approval rollback。
- receipt 已写但 journal 未收口时的恢复。
- approved settlement。
- review、body、authority、metadata、settlement、truth 指纹。
- accepted best candidate 依赖验证。
- approval idempotent replay。

`novel_studio/chapter_state.rs` 已有：

- 只接受从第 1 章开始的连续 approved prefix。
- 检查重复章号。
- 检查缺章。
- 检查危险路径。
- 读取磁盘正文并重新计数。
- 读取 approval receipt。
- 累加磁盘正文实际 units。
- 以累计 units 与任意 `target_units` 判断目标是否达到。

当前仍有一个确定缺口：`durable_chapter_progress` 虽逐章读取 approval receipt，却把结果
反复写入 `latest_receipt_*` 四个字段。循环完成后只剩最后一章的 receipt 状态，且
`durable_project_completion_blockers` 也只检查这四个 latest 字段。因此：

```text
第 1 章 receipt 缺失/正文指纹不匹配
→ 第 2 章 receipt 正常
→ 循环结束只看到第 2 章正常
```

这种情况下磁盘正文仍会被计入 approved prefix，历史 receipt 破坏可能被后章覆盖。
所以“磁盘正文连续性 owner 已有”成立，“整个 approved prefix 的 receipt 链已经验证”不
成立。

### 9.2 决策

**保持 owner，不建立第二套进度或 receipt 系统。**

1～6 的修改只能改变：

- finding disposition。
- recovery 结果。
- metadata fallback。

不能绕过或弱化：

- accepted best dependency。
- pending settlement。
- passing typed review。
- truth validation。
- approval transaction。
- receipt 和 fingerprint。
- 磁盘连续 approved 进度。

在同一 `chapter_state.rs::durable_chapter_progress` 内原位补齐：

1. 对 approved prefix 中每章验证 receipt 是否存在、非 legacy、body fingerprint 是否与
   该章磁盘最终正文一致。
2. 在首次缺失或 mismatch 处记录 blocker，并停止把后续章节计入可信 approved prefix；
   不能让后章正常 receipt 覆盖前章失败。
3. truth fingerprint 不能简单拿每个历史 receipt 与“当前全书 truth”比较，因为后续章节
   会合法改变当前 truth。最后一章继续执行当前 truth 一致性检查；历史章只有存在可靠的
   chapter-cutoff truth/settlement snapshot 时才做对应比较，不能制造必然失败的比较。
4. 复用现有 `read_approval_receipt`、body fingerprint 和 blockers，不增加平行 ledger。

### 9.3 允许的改动

除此项已确认的原位修正外，只允许：

- 给新的 degraded/repairable 结果补充 receipt 可验证字段。
- 增加回归测试。
- 确保 metadata fallback 后重新计算 metadata fingerprint。
- 确保 zero-delta settlement 仍绑定最终 body/authority fingerprint。

### 9.4 明确禁止

- 不能因为 observer 格式失败而跳过 settlement。
- 不能直接把章节 status 写成 approved。
- 不能从内存计数覆盖磁盘进度。
- 不能用计划章数代替 approved units。
- 不能为了“更容易写完”放松 receipt/body/truth 一致性。

### 9.5 验收

- 任意一步崩溃后只能恢复成完整未提交或完整已提交。
- metadata fallback 后旧 receipt 失效，新 receipt 精确匹配。
- state degraded zero-delta settlement 不能污染旧 truth。
- approved prefix 任一章跳号、重号、缺正文、缺 receipt、legacy receipt 或 body receipt
  mismatch 都在该章阻断，不能被后章正常 receipt 覆盖。
- latest receipt 仍必须匹配当前 truth；历史 truth 只有存在对应 chapter-cutoff 快照时才比较。
- 任意总字数都按磁盘 approved units 完成，不按固定章数完成。

---

## 10. 第 8 项：旧机制与死代码删除清单

第 8 项不是先做的大删除，而是 1～7 每项接通后的收尾。删除前必须先证明新路径已经
接管所有调用。

### 10.1 合同侧

迁移后删除：

- `contract_quality_issue_is_blocking`
- 以 issue 中文文案判断 hard/repairable 的代码
- 用户展示层重复生成最终 readiness 的逻辑
- StoryContract/StoryBible 对 canonical contract 丰富度的重复 gate
- 无生产调用的 `FullLongformContract` 分支

保留：

- legacy 文本展示。
- legacy 数据只读迁移。
- typed gate、patch scope 和 field strength。

### 10.2 章节 hard policy

迁移后删除：

- `HARD_BLOCKER_CODES`
- 按 code 字符串重建 finding class/disposition 的 `match`
- 无生产 finding owner 的 code 分支
- 对 typed finding JSON 进行多处重复字符串扫描的生产策略
- typed findings 为空时把任意 legacy `issues` 文本直接当 hard 的生产兜底

保留：

- legacy receipt/finding 只读 parser。
- `ChapterFinding::hard_blocking()`。
- 本地 evidence-backed producers。

### 10.3 字数

迁移后删除：

- 低于 target 一律 hard 的 `length_below_minimum` 定义
- reporting 的 `target / 3` 独立标准
- fallback 内手写 80%
- 通过错误文案搜索长度不足的生产路由

保留：

- 唯一 usable floor 函数。
- finite top-up。
- hard max。
- 实际 units 累加。

### 10.4 状态与元数据

迁移后删除：

- Character 专用 required state recovery。
- 信任 observer 自报高风险布尔值、而不从本地 authority/evidence 派生风险的路径。
- observer format error 共用的 `state_repair_required` 终止出口。
- degraded reason 自动令 validation fail。
- metadata 五轮后的 terminal blocker。
- approval 对 advisory-only metadata 的重复阻断。

保留：

- `state_repair_required` 生命周期状态，用于真实 required state 缺失或依赖指纹不匹配；正文
  状态污染由章节质量门的确定性 finding 阻断。
- pending/approved settlement。
- typed delta evidence、allowance 和 fingerprint。
- metadata gate 作为修复/告警 producer。

### 10.5 删除验证

实施完成必须使用 `rg` 证明旧生产符号无调用，并检查：

```text
contract_quality_issue_is_blocking
HARD_BLOCKER_CODES
recover_explicit_required_character_state_change
format_metadata_blocker_result
target / 3 的章节长度标准
错误文案驱动的 length_shortfall_node 生产路径
无 producer 的 hard code 分支
```

兼容 parser 如果必须保留，要标注 `legacy read-only`，且不能参与新写入策略。

---

## 11. 模块归属与禁止新增位置

| 能力 | 唯一 owner | 应修改位置 | 禁止新增位置 |
| --- | --- | --- | --- |
| 合同 issue/disposition | `creation_contract/issue.rs` | 扩展现有 issue | `chat.rs`、gateway、session surface |
| 合同 readiness | `typed_contract_gate.rs` | 统一 producer 与 scope | StoryBible、reporting 再建质量门 |
| 合同有限修复 | `creation_contract/repair_coordinator.rs` | 消费 typed disposition | 新建第二个 repair runner |
| 章节 finding/hard policy | `chapter_quality.rs` | 保持唯一 hard 判定 | workflow driver 字符串白名单 |
| 本地章节 finding producer | `novel_studio/quality_gate.rs` | 调整长度 disposition | auditor prompt 中创造 hard |
| 有限修订和 best | `novel_workflow_driver/chapter_loop.rs` | 复用现有 controller | metadata/state 各建 candidate store |
| observer 调用 | `novel_workflow_driver/audit.rs` | 返回 typed outcome | session surface 判断格式错误 |
| 状态证据与恢复 | `novel_studio/settlement.rs` | 泛化现有 recovery | novel bible 再解析正文 |
| 状态生命周期 | `novel_studio/state_truth.rs` | 区分 degraded/hard | workflow 字符串猜 status |
| 元数据有限恢复 | `novel_workflow_driver/metadata_repair.rs` | 接通 best/fallback | 正文 reviser 修标题 |
| 批准事务 | `novel_studio/approval_transaction.rs` | 保护，只接收新 outcome | workflow 直接写 approved |
| 磁盘进度与 receipt prefix 验证 | `novel_studio/chapter_state.rs` | 保持 owner，补逐章 receipt/body 验证 | 内存 task 自报完成或新建第二套 receipt ledger |

---

## 12. 推荐实施顺序

必须按依赖顺序实施，不能同时大面积改写。

### Step 1：合同 typed disposition

先扩展现有 `ContractIssue`，迁移 producer 和 gate，再删除字符串严重度。

原因：正文开始前的大量假阻断主要来自合同问题无法区分 hard、repairable 和 advisory。

### Step 2：章节唯一 hard policy

让 workflow 直接消费本地 typed finding，删除 `HARD_BLOCKER_CODES` 和悬空 code。

原因：后续长度、observer 和 metadata 的 soft/degraded 结果必须依赖唯一 disposition。

### Step 3：统一字数标准

复用现有 top-up/best，修改 finding disposition，删除 100%/80%/33% 冲突。

原因：这是当前章节常见假阻断，但不能在 typed hard policy 统一前单独打补丁。

### Step 4：observer outcome 分层

区分 format degraded、display degraded、required state missing、pollution 和 dependency mismatch。

### Step 5：泛化 required state recovery

复用现有 `authority_values`、evidence 和 allowance，替换 Character 专用函数。

### Step 6：metadata best/fallback

五次后接通现有 best 和本地投影，删除 terminal blocker。

### Step 7：补齐 approved-prefix receipt 验证并做保护区回归

在现有 durable progress owner 内修复历史 receipt 被 latest 状态覆盖的问题，再逐一验证
approval transaction、fingerprint、连续章节和磁盘进度没有被弱化。

### Step 8：统一删除

按第 10 节 `rg` 清单删除旧生产路径、死代码和被替换出口。

---

## 13. 每一步实施前的强制核对准则

每个 Step 开始前都必须完成：

1. 用 `rg` 找出目标类型、函数、常量和所有调用者。
2. 找到当前唯一持久化 owner。
3. 判断现有机制是正确、职责错误还是确实缺失。
4. 能扩展现有类型时，不增加平行类型。
5. 能修改现有 controller 时，不增加平行循环。
6. 写出本步将删除的旧符号。
7. 先增加/修改不变量测试，再迁移调用。
8. 所有调用切换完成后才删除旧路径。
9. `git diff --check`、格式化、编译和专项测试通过后才进入下一步。
10. 若实施过程中发现文档判断与真实调用链不符，先更新本文，再继续编码。

题材特例禁止进入 hard policy。测试题材只用于覆盖，不得成为代码分支。

---

## 14. 对“写不出小说”和“内容漂移”的影响

### 14.1 对写不出小说的改善

预计会有直接改善，原因不是放弃治理，而是删除假阻断：

- 合同缺少非关键丰富度不再被当成终止错误。
- 轻微字数不足在有限补写后可以选择完整 best。
- observer 坏 JSON 不再等同于状态污染。
- 标题/摘要五次未收敛不再终止正文。
- 无生产者 hard code 和字符串策略不再产生错误路由。

### 14.2 对内容漂移的影响

按本文全部实施并通过跨题材回归后，预期漂移风险不会因“软化假阻断”而上升；这不是在
实施前可以保证的结论。尤其第 4 项的本地 high-risk 派生和第 7 项的 approved-prefix
receipt 验证必须同时完成，否则只放松 observer/metadata 出口会形成新的漏检窗口。

完整实施后仍保留的防漂移条件包括：

- 合同、人物身份、世界硬规则仍由 sealed authority 约束。
- 语义 hard finding 仍需要 authority/body 双侧证据。
- future chapter consumption 仍阻断。
- 状态只能从最终正文的 typed delta 结算。
- 无证据 observer delta 仍被丢弃；最终正文有双侧证据的越权变化仍 hard block 且不提交
  truth。
- required end-state 缺少正文证据仍阻断。
- 下一章只消费 approved truth。
- approval receipt 和 fingerprint 仍验证同一正文与权威，且补齐后不会只检查最后一章。

实际还会降低一种隐性漂移：当前模型为了通过重复元数据、长度和 observer 格式门而多次
重写正文，重写次数越多，人物和事件漂移概率越高。复用 best 和有限降级后，这类无必要
正文重写会减少。

### 14.3 仍需警惕的风险

- 轻微不足不能变成无限放宽；必须保留统一 usable floor。
- degraded observer 不能自动提交零 delta 后忽略明确 required outcome。
- high-risk 状态分类不能依赖模型自报布尔值，必须由本地 authority/path/evidence 推导。
- metadata fallback 不能从未经批准的生成阶段声明构造 truth。
- contract advisory 化不能移除主角、终局、主线、世界硬规则和首批章节窗口。
- LLM audit advisory 化不能影响本地 evidence-backed hard finding。

---

## 15. 验证计划

### 15.1 单元与组件回归

至少覆盖：

1. ContractIssue disposition 与文案无关。
2. advisory-only 合同 ready。
3. repairable 合同有限收敛。
4. 用户任意总字数和 2500/5000 档不被修改。
5. 未知 LLM hard code 不能升级。
6. evidence-backed 身份/世界/未来章节冲突仍 hard。
7. 轻微字数不足 top-up 后选择 best。
8. 低于 usable floor 仍阻断。
9. 两档 hard max。
10. observer 五次坏 JSON 的安全 zero-delta fallback。
11. required `new_state_after_chapter` 在可唯一解析的多 event type 中恢复；多义时不猜测。
12. optional HookSeed/HookPayOff/HookDefer 漏报不被自动升级成 required hard。
13. observer 无证据越权声明只丢弃；正文有双侧证据的 pollution 不提交 truth 并阻断。
14. observer 风险布尔值全 false 也不能隐藏本地可确定的高风险变化。
15. metadata 五次失败后的 deterministic fallback。
16. metadata fallback 不改变正文和 settlement state delta。
17. approval 崩溃恢复与幂等重放。
18. 磁盘跳号、重号、缺正文以及 approved prefix 任一章 receipt/body mismatch。
19. 历史 receipt 失败不能被后章正常 receipt 覆盖。
20. 任意 target_units 的完成判断。

### 15.2 真实聊天测试

代码实施完成且所有组件回归通过后，再恢复真实面板模拟测试：

- 每次必须新 session、新项目、新大题材，从 0 开始。
- 任何代码修改后，之前章节数作废，必须从 0 重测。
- 用户请求完整小说，不能人为改成“只写前 10 章”。
- 后台只观察能否自然越过 10 章。
- 先验证不干预连续 10 章，再验证完整 10 万字/2500 档。
- 另开新 session 验证 100 万字/5000 档合同生成、确认且不截断。
- 重点检查角色姓名、身份、章节编号、伏笔生命周期、世界状态和连续性。

---

## 16. 最终实施判定

这 8 项的正确动作可以压缩为：

```text
1. 合同：扩展现有 issue，替换字符串严重度。
2. 章节：保持现有 typed hard owner，删除第二份 code policy。
3. 字数：保持 top-up/best，替换轻微不足 hard block。
4. Observer：保持有限重试和状态隔离，补 outcome 分层。
5. 状态恢复：只对 required `new_state_after_chapter` 泛化现有 Character 恢复，复用现有
   证据/allowance；optional 字段不升级为必需项。
6. 元数据：保持五轮和 candidate，接通 best/local fallback。
7. 批准与进度：保持现有 owner，补齐 approved-prefix 逐章 receipt/body 验证，其余作为
   不可弱化保护区。
8. 清理：接线完成后删除旧字符串判断、终止出口和无生产者代码。
```

结论：BenShu 不是缺少整个治理系统，而是已有系统在合同严重度、轻微字数、observer
格式和元数据出口处仍把“可恢复失败”错误升级为“终止性阻断”。本计划应以原位整合和
删除冲突路径为主，不应再增加第二套合同 gate、章节 gate、修订器、状态机或元数据
candidate 机制。
