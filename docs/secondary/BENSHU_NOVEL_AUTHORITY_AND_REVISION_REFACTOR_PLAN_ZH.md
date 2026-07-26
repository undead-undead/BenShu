# BenShu 小说权威、质量门与有限修订重构计划

> 状态：第三次端到端代码核对修订稿，待实施
>
> 日期：2026-07-20
>
> 范围：`crates/builtin-tools/src/tool/writing` 内的小说章节执行、审稿、修订、状态结算与恢复流程
>
> 核心目标：删除主观质量造成的假阻断，保留并加强有证据的内容漂移阻断；统一章节权威和修订入口，避免“能推进但漂移”或“无漂移却反复卡住”。

## 0. 文档定位

本文是以下既有文档的后续专项计划，不替代其中已经稳定的命名、合同和工具边界结论：

- `BENSHU_WRITING_DUPLICATE_MECHANISM_REFACTOR_PLAN_ZH.md`
- `BENSHU_NOVEL_STUDIO_REFACTOR_PLAN_ZH.md`
- `BENSHU_NOVEL_STRUCTURED_CONTRACT_V2_PLAN_ZH.md`
- `WRITING_TOOL_BOUNDARY_REFACTOR_PLAN_ZH.md`

本文以六个核心问题为主体：

1. 只有可确定的合同、连续性、状态和正文完整性错误才能阻断写作。
2. 主观质量分不能触发多轮正文修订。
3. 全部正文修订路径复用同一个净提升、最佳版本和回滚机制。
4. Writer、Auditor、Reviser、State Observer 消费同一个只读章节权威包。
5. 人物、世界观和伏笔从最终正文结算；失败时保留旧状态并阻断污染。
6. 前五项稳定后，才考虑给审稿器配置另一模型。

第三次代码核对确认：仅处理上述六项仍不足以解决“小说根本写不出来”。因此本文同时
纳入两个不新增正式 Phase 的端到端前置/收尾条件：

- **合同收敛前置条件**：合同必须在有限、可证明有进展的尝试内进入用户可确认状态，
  不能在正文开始前进行最多 30 轮同模型自修自审。
- **全书持续完成条件**：章节通过后必须以可恢复的小批次自动推进到目标和结构化终局，
  不能依赖一次存活数小时/数天的 40～200 章内存任务，也不能由结尾关键词决定完结。

本文不会把“更容易写完”置于“内容不漂移”之上。正确目标是：

> 删除假阻断，但绝不自动放行仍然存在的合同、连续性和状态漂移。

---

## 1. 当前代码审查结论

### 1.1 六项能力现状

| 项目 | 当前状态 | 主要问题 |
| --- | --- | --- |
| 确定错误才阻断 | 部分具备 | 本地启发式和 LLM 自由文本仍可能成为硬阻断 |
| 主观分不触发多轮修订 | 未具备 | `score < 85` 仍可导致 `needs_revision`，语义修订预算为 10 |
| 净提升与最佳版本回滚 | 部分具备 | 只覆盖主修订链，旧草稿恢复链没有完整复用 |
| 同一只读章节权威包 | 未具备 | ContextPackage 在执行包生成前保存，阶段间权威组合不一致 |
| 最终正文结算与失败隔离 | 基本具备 | 元数据可反向覆盖 pending settlement，fallback 仍可能复用写作元数据 |
| 独立审稿模型 | 未具备 | Writer、Auditor、Reviser、Observer 共用 `self.agent` |
| 合同有限收敛 | 部分具备 | 有进展判断和阶段轮换，但总预算仍为 30，字符串分流与同模型 `Uncertain` 可反复阻断 |
| 全书无人干预完成 | 部分具备 | 能按字数估算 40/200 章，但一次创建全部 steps；完结仍依赖 LLM 与表面关键词 |

### 1.2 当前应保留的机制

以下机制方向正确，本次不得重新实现第二套：

- `ContextPackage`、`RuleStack`、`ChapterTrace`
- `build_prompt_context_payload` 已有的“受保护上下文/可压缩上下文”分层
- `prompt_context_fingerprint` 已有的 SHA-256 上下文指纹
- `ChapterExecutionContractV2`
- `ChapterContractRecord`、`ChapterArchitectureRecord`
- `ensure_structured_contract_v2` 已有的合同权威优先级和兼容镜像同步
- creation contract 已有的 typed patch、阶段轮换、pending best candidate 和
  `ContractRepairProgress` 无进展检测
- 本地人物命名、登记和权威锁定
- `ChapterQualityGate`、`ChapterMetadataGate`
- `ChapterLoopDecision`
- 现有净提升比较和非提升回滚
- `persist_stream_snapshot`
- `ReviewCycleRecord`
- `final_chapter_observer_prompt`
- `validated_settlement_from_final_body`
- pending settlement 与批准后 truth commit
- `state_repair_required`
- 磁盘连续已批准章节作为进度权威
- 项目写作锁、心跳、原子文件写入和批准后 Story Bible 重建
- `HookStatus` 及 seed、advance、pay_off、defer、overdue 伏笔生命周期
- staging/backup 形式的项目快照切换
- `ContinuousTaskExecutor`、磁盘批准进度和逐章 dependency
- `init_project` 已有的临时目录初始化与 rename

### 1.3 当前存在的直接冲突

#### 冲突 A：字符串质量问题同时承担“诊断”和“阻断”

`ChapterQualityGate` 当前主要存储字符串：

```rust
pub struct ChapterQualityGate {
    pub passed: bool,
    pub issues: Vec<String>,
    pub repairable: Vec<String>,
    pub warnings: Vec<String>,
}
```

`audit_issue_is_actionable` 再根据“略显、严重、建议、重复、截断”等自然语言措辞猜测是否阻断。未命中特例的文本可能默认成为 actionable。

结果：

- 同一意见换一种措辞就可能改变阻断结果。
- 修复容易变成不断增加中文短语特例。
- LLM 主观意见可能越权成为正文硬门。

#### 冲突 B：LLM score 仍是批准条件

当前 LLM 审稿通过条件包含：

```rust
passed && score >= 85 && issues.is_empty()
```

因此即使没有可验证的合同、连续性和正文错误，低分仍可能触发修订。

#### 冲突 C：一个“10轮”常量控制多类循环

当前存在：

```rust
MAX_CHAPTER_REVISION_ATTEMPTS: usize = 10
```

它同时影响：

- 章节语义修订
- review cycle 的 blocked 判定
- metadata repair 上限
- 恢复路径的修订预算

此外还有单独的补尾、扩写、清洗和外层 step retry。用户看到的总尝试次数可能明显超过一次有限语义修订。

#### 冲突 D：新章节和恢复旧草稿有两条语义修订链

主章节链已经使用 `revision_quality_score` 和非提升回滚。

旧草稿恢复链使用：

- `revise_reusable_existing_chapter_once`
- 独立的 `while body_revision_required_after_audit`
- 独立 fingerprint 集合
- 独立失败后重新生成策略

两条路径的候选选择、停止条件和恢复结果不完全一致。

#### 冲突 E：ContextPackage 保存时还没有最终执行包

当前顺序是：

```text
compose_context
-> 保存 context/rules/trace
-> 生成 ChapterExecutionPackage
-> 本地登记人物
-> persist_execution_package
-> 写正文
```

Writer 使用内存中的 ContextPackage 加 ChapterExecutionPackage；Auditor 和 State Observer 读取已保存的 ContextPackage。这个已保存包生成于最终执行包和人物登记之前。

结果：

- Writer 能看到的新人物和章节变化，Auditor 未必从相同结构看到。
- 审稿只能通过重新读取 manifest 间接补齐部分权威。
- 恢复旧草稿时还可能重新生成一个不同的执行包。

#### 冲突 F：最终正文 settlement 仍可能被元数据覆盖

`sync_pending_settlement_metadata` 会把：

- `chapter.summary`
- `chapter.continuity_updates`

重新写入 pending settlement。

`default_pending_settlement_from_chapter` 也会优先复用章节 continuity metadata。

这与“写作阶段 metadata 和执行包声明不是事实来源”的 final observer 规则冲突。

#### 冲突 G：当前复合指纹会被批准流程自己改写失效

`chapter_revision_fingerprint` 同时包含正文、标题、摘要、关键事实和
`continuity_updates`。批准流程先用该指纹验证 review、truth validation 和
pending settlement，随后又把 settlement 的摘要和连续性结果写回章节元数据。

结果：

- 通过验证时的指纹可能不再等于最终已批准记录的指纹。
- 只修元数据也会迫使正文语义审稿重新运行。
- “正文事实没变”和“展示元数据变了”无法区分。

同时代码中还存在基于 `DefaultHasher` 的正文 fingerprint 和多个 SHA-256
fingerprint，它们的输入、稳定性和用途并不相同，不能继续统称为一个
“章节指纹”。

#### 冲突 H：底层公开动作可以绕过规范审批链

当前 schema/dispatch 仍公开 `add_chapter`、`import_chapters`、`revise_chapter`、
`review_chapter`、`approve_chapter`、`update_truth` 等低层动作。部分动作接受
调用者传入的字符串状态：

- `add_chapter`、`revise_chapter`、`import_chapters` 可以收到 `approved`。
- `review_chapter` 可以收到调用者提供的 verdict/issue。
- `update_truth` 可以直接写 durable truth。

如果只重构 workflow driver，而不在 `novel_studio` 存储边界强制状态不变量，
模型或其他调用方仍可绕过 sealed authority、typed review、settlement 和正式批准。
这是原计划遗漏的最高风险项之一。

#### 冲突 I：当前 settlement 没有承载完整的 typed 状态变化

`SettlementOutput` 和 `FinalChapterObservation` 主要仍是字符串：

- `current_state`
- `pending_hooks`
- `chapter_summary`
- `continuity_updates`
- `hook_updates`

它们没有完整表达人物、关系、地点、世界规则、能力、资源等带实体 ID 和正文
证据位置的 typed delta。批准后 `novel_bible/core.rs` 和
`novel_bible/contract_settlement.rs` 仍会从 summary、key facts 和 continuity
文本推导世界与人物状态。

因此仅修改 `settlement.rs` 不足以完成“最终正文结算”；下游 reducer 也必须改为
消费已经验证的 typed settlement delta。

#### 冲突 J：状态证据验证存在重复且证据强度不足

`novel_governance.rs` 与 `settlement.rs` 各自存在正文支持判断。当前部分判断依赖
词项或 bigram 命中：

- 两套实现可能对同一状态得出不同结论。
- 常见双字命中只能说明文字相关，不能证明“关系已改变”或“伏笔已回收”。
- observer 幻觉仍可能通过弱字符串支持进入 truth。

状态证据需要统一到一个 evidence validator，并使用实体 ID、事件类型和正文精确
span，而不是再新增第三套关键词检查。

#### 冲突 K：合同已有唯一权威，但 prompt 仍可能重复携带兼容镜像

`ensure_structured_contract_v2` 已规定：

- 已确认的 `authority_contract.structured` 是合同权威。
- `StoryContract` 和 manifest 字段是兼容镜像。
- Story Bible 是派生视图，不能反向成为合同权威。

如果 `SealedChapterAuthority` 直接封存现有 `project_context`，可能同时带入上述
多份镜像。它们即使内容大致相同，也会增加冲突指令、上下文膨胀和模型误选权威的
概率。sealed package 必须封存“规范化权威投影”，而不是把所有存储镜像原样打包。

另外，`set_contract` 当前主要清理 context packages，未完整失效执行包、草稿候选、
审稿、settlement 等全部后代产物，需要建立依赖失效链。

#### 冲突 L：同一根指纹不代表各角色实际看到了同样内容

现有 audit/observer prompt 会对 authority 文本做约 12,000 字符 preview；完整
prompt context 还有总预算和相关人物数量上限。当前 degraded/minimal context 路径
也允许以缩减上下文继续。

因此：

- 四个阶段记录相同 root fingerprint，并不能证明实际消费的 role projection 相同。
- 权威包如果在封存前已经漏掉必要人物、规则或历史，封存只会把遗漏冻结。
- protected context overflow 如果只是 telemetry，不会阻止带残缺权威继续写作。

Phase 1 必须同时解决“同源、完整性、角色投影可追踪”三个问题。

#### 冲突 M：章节状态仍由大量原始字符串和历史别名共同决定

`novel_pipeline/lifecycle.rs` 已经有章节生命周期解析器，但生产路径仍大量直接比较
或写入 `drafted`、`revised`、`needs_revision`、`audit_passed`、
`reviewed_passed`、`approved` 等字符串。

这会导致：

- 恢复链和首次写作链对同一状态理解不同。
- 新增状态时继续堆别名。
- `approved` 可能绕开唯一状态转移入口。

必须复用并收口到 typed lifecycle，不得新增第二套状态机。

#### 冲突 N：当前“最佳恢复版本”并不等于权威上最好的版本

主修订链已有 blocker/非提升比较，但 durable snapshot 的恢复选择仍包含完成度和
正文长度倾向；snapshot 主要保存 body 文本，并不完整保存：

- 对应 authority fingerprint
- 章节 metadata
- typed findings
- quality vector
- 候选来源和被接受原因

此外，外层 step retry、重新生成、final cleanup、补尾和最终字数 top-up 仍可能在
统一语义修订循环之外改变正文。若 Phase 3 只合并两个显眼的 revision loop，重复
机制仍然存在。

#### 冲突 O：合同阶段仍有另一套“字符串分类 + 30 轮修复”

`creation_contract/repair_coordinator.rs` 已有值得保留的 staged patch、best pending
candidate 和无进展检测，但当前上限仍是：

```rust
MAX_CREATION_CONTRACT_AUTO_REPAIR_ATTEMPTS = 30
```

每个循环还可能依次执行 semantic repair、本地 repair、title/metadata repair 和 staged
LLM repair。`ContractIssueKind::from_text` 又通过“书名、角色、大纲、世界规则”等
中文/英文短语判断 issue 应进入哪个阶段。

结果：

- 合同还没展示给用户，就可能耗费大量同模型调用。
- issue 只换措辞就可能被送到错误 patch owner。
- “字段更多”会被 `filled_score` 视为进展，但不保证旧的用户权威和已正确字段没有
  回退。
- 章节侧有限修订完成后，系统仍可能永远到不了第一章。

这与 Phase 2/3 要删除的章节字符串分类和多轮修订属于同一种架构问题，不能只修章节
而保留合同版本。

#### 冲突 P：合同同模型语义裁判把 `Uncertain` 作为硬阻断

合同结构通过后，系统还调用同一个模型判断：

- 用户故事核心是否被保留
- 大纲/兑现矩阵是否服从人物、世界和终局
- 两种终局表达是否等价

在 `user_story_authority_semantic_issue` 和
`outline_character_authority_semantic_issue` 中，模型无输出、解析失败或
`Uncertain` 都可重新打开 DraftingContract 并继续修复。

这会把“裁判无法证明”等同于“合同确定冲突”。尤其 Writer 和 semantic reviewer 是
同一模型时，输出波动会形成有限但昂贵的自我否定循环。用户已经看到并确认的完整
合同，也不应被一个无证据的 `Uncertain` 推翻。

#### 冲突 Q：长正文仍被要求装进 JSON，展示 metadata 又与最终状态重复

Writer 当前被要求一次返回：

```text
title, content, summary, key_facts, continuity_updates
```

其中数千字正文位于 JSON string 中，模型需要正确转义引号和换行；输出截断时，
summary/key facts/continuity 往往位于正文之后而丢失。现有 jsonish/freeform recovery
虽然提高了容错，却形成多种解析 provenance 和 degraded 分支。

而 Phase 4 已确定 writer metadata 不能成为事实来源。继续强制 Writer 在长正文输出
中生成这些字段，既增加结构化失败概率，也重复 State Observer 的职责。

#### 冲突 R：全书运行把 40～200 章放入一次长寿命任务

10 万字/2500 字约 40 章，100 万字/5000 字约 200 章。当前
`existing_project_turn_chapter_count` 会在“写完整本”请求中计算全部剩余章节，并一次
创建对应的 `ContinuousTaskPlan` steps。

逐章磁盘批准和 dependency 是正确的，但一次调用长期持有 workflow lease 并连续执行
全部 steps，仍受以下外部条件影响：

- 会话、worker、模型服务或进程重启
- 单步多次 retry 带来的总耗时膨胀
- 长任务取消和平台请求生命周期

暂停后可以再次从磁盘继续，不等于“不需要用户干预就能自动写完整本”。需要复用现有
ContinuousTaskExecutor 做 durable rolling batch，而不是新增另一套章节执行器。

#### 冲突 S：完结门仍把 LLM 结论和表面关键词当作完成权威

达到目标字数后，`completion.rs`：

- 调用 LLM 判断 narrative closure
- 用“终章、尾声、尘埃落定、新阶段、入口、下一章”等词语进行确定性覆盖
- 默认最多再追加 3 章，可配置上限 8 章，然后停止

正常结尾没有这些关键词可能被判“未完结”；正文为回顾而提到“新阶段”也可能被判中段。
反过来，只出现“尾声”也不能证明主冲突、人物弧线和必须回收伏笔已经完成。

这会造成两种失败：

- 小说已经写完但系统永远不承认完成。
- 小说实际没完成，却因为表面完结词被导出。

完结必须由合同终局义务和批准后的 typed state/hook lifecycle 结算，不应继续维护
结尾关键词特例。

#### 冲突 T：15 秒本地工具预算与长篇派生状态全量重建会逐章放大

`read_manifest` 和 `write_manifest` 都调用 `ensure_project_governance`，其中会从全部已
批准章节 metadata 重建 Story Bible；`write_manifest` 还写 Story Bible artifacts。
workflow 对多项本地 stage 默认使用 15 秒 timeout。

40 章可能暂时看不出问题，200 章项目会不断放大 manifest 克隆、派生状态重建和多文件
写入成本。更危险的是，Phase 1 如果把 persistence 失败改成硬停止，却不先保证事务
幂等和合理时限，可能比当前更容易卡住。

Story Bible 仍应是派生视图，但正常批准应增量消费一个 approved typed delta；全量
rebuild 保留给审计/修复。超时重试必须通过 transaction ID/receipt 识别已完成状态，
不能创建第二份执行包或重复登记人物。

#### 冲突 U：批准涉及多文件更新，但还没有单章原子 commit

当前批准会依次修改章节记录、review/truth/settlement、truth files、Story Bible、
manifest、导出和快照。单文件写入是原子的，整个批准事务却不是一个原子状态转移。

进程在中间崩溃时，可能出现：

- chapter 已写 approved，但 truth 尚未提交
- truth 已更新，但 approval receipt/manifest 尚未一致
- Story Bible artifact 和 project.json 处于不同 revision

项目 snapshot 的 staging/backup 应继续复用，但需要增加轻量的 per-chapter approval
journal/commit marker；不能为此再造第二套项目锁。

---

## 2. 修正后的核心设计原则

### 2.1 不再把“主观质量”和“故事权威”放在同一个门里

质量必须分成两类。

#### 作品质量 Advisory

包括：

- 节奏偏慢
- 描写不够丰富
- 情绪感染力不足
- 对话不够生动
- 语言不够优美
- 人物塑造可以更深
- 综合评分低于 85

处理方式：

- 记录到 review report
- 可供人工检查
- 不阻断批准
- 不触发正文语义修订

#### 故事权威 Hard Gate

包括：

- 人物身份、姓名、代词、能力或资源冲突
- 关系状态冲突
- 世界规则冲突
- 时间线、地点和任务连续性冲突
- 未经允许提前消费未来章节事件
- 未经允许开启新主线
- 伏笔提前回收、错误回收或无证据回收
- 本章最终状态超出章节执行合同允许的变化
- 正文污染、截断、缺失、硬字数越界

处理方式：

- 必须修复
- 未修复不能批准
- 未批准不能提交 truth
- 未提交 truth 不能进入下一章

### 2.2 LLM 不能仅凭分数或结论制造硬阻断

LLM 可以发现语义漂移，但必须返回可验证证据：

```rust
struct AuthorityConflictFinding {
    conflict_kind: AuthorityConflictKind,
    authority_path: String,
    authority_excerpt: String,
    body_excerpt: String,
    explanation: String,
    affected_entities: Vec<String>,
    confidence: f32,
}
```

finding 必须先分证据等级：

```rust
enum FindingEvidenceGrade {
    DeterministicInvariant,
    EvidenceBackedSemantic,
    Advisory,
}
```

- `DeterministicInvariant`：正文缺失、截断、污染、确定字数越界、指纹不匹配等可以
  由本地直接证明的错误。
- `EvidenceBackedSemantic`：人物、关系、世界、时间线、伏笔和状态冲突。LLM 只能
  提交候选 finding，必须通过对应类型的本地结构验证。
- `Advisory`：无法由结构和正文证据确定真假的语义意见，以及全部审美意见。

本地必须验证：

1. `authority_path` 存在于本章封存权威包。
2. `authority_excerpt` 来自该路径。
3. `body_excerpt` 存在于当前正文。
4. 涉及的角色、世界规则、伏笔 ID 或章节节点真实存在。
5. `conflict_kind` 属于允许硬阻断的类型。
6. finding 绑定当前 `authority_fingerprint` 和 `body_fingerprint`。
7. 对应 conflict kind 的关系确实成立；不能因为两段文字都存在就认定它们矛盾。

没有证据的“我认为不合理”只能成为 Advisory。

注意：路径存在、引用存在、置信度高，只能证明 finding 引用了真实输入，不能证明
矛盾成立。例如“林舟来到城门”和“林舟上一章在客栈”同时存在，并不自动构成地点
连续性冲突；还必须验证时间顺序、移动许可和本章执行目标。

### 2.3 修订预算耗尽不等于自动通过

统一行为：

- 没有 hard blocker：允许批准。
- 有 hard blocker 且修订消除：允许批准。
- 修订没有净提升：回滚最佳版本。
- 回滚后仍有 hard blocker：保留正文，状态为 `needs_revision`。
- 绝不能因为尝试次数用完而自动批准。

### 2.4 最终状态必须同时满足两种证据

每一项新状态都要同时满足：

1. 正文证据：变化确实发生在最终正文中。
2. 权威许可：变化没有超出封存章节执行合同允许的范围。

例如正文确实写了“主角成为帝国主教”，但执行合同只允许“主角取得铜钥匙并离开矿区”，该状态仍然属于漂移，不能提交 truth。

### 2.5 封存权威必须同源、完整、按角色可证明

一个 root fingerprint 只证明“大家声称来自同一个根”，不能证明 prompt 没被截断。
因此每章必须同时保存：

- `authority_root_fingerprint`
- 规范化权威根
- Writer、Auditor、Reviser、Observer 各自的只读 projection
- 每个 projection 的 fingerprint、包含/排除路径和截断记录
- protected authority coverage 检查结果

四个角色可以拥有不同格式的 projection，但不得重新读取可变 manifest 来补权威。
其中合同、人物身份、世界硬规则、本章执行合同、截至本章的 truth、伏笔约束等
protected authority core 必须逐字段完全一致；projection 只能在任务指令、输出
schema 和可压缩背景的呈现方式上不同。任何受保护字段缺失、截断或超预算时，不得
进入正文生成。近期正文等可压缩信息仍然复用现有分层机制。

未来章节信息只能进入明确的“禁止提前消费边界”，不能作为正向剧情材料混入 Writer
上下文，避免为了防剧透反而把未来事件提示给模型。

### 2.6 不变量必须在存储边界强制执行

workflow driver 负责正常编排，但安全性不能依赖“大家都只调用正确入口”：

- 只有 canonical `approve_chapter` 状态转移可以写 `approved`。
- add、revise、import 不接受调用方直接写 `approved`。
- review receipt 必须绑定 body 和 authority，调用方自由文本不能伪造通过。
- durable truth 只能来自已批准 settlement；人工覆盖必须是显式管理操作，并记录
  provenance、失效全部下游产物。
- 导入章节先进入 `imported_unverified`，完成迁移验证后才可能批准。

这些限制应放在 `novel_studio` 写入边界；不能只放在聊天 prompt 或 runner 中。

### 2.7 不同语义的指纹必须拆开

统一使用带 domain/schema tag 的 canonical JSON + SHA-256：

```rust
BodyFingerprint
MetadataFingerprint
AuthorityFingerprint
ApprovalReceiptFingerprint
```

- semantic audit 和 settlement 绑定 body + authority。
- metadata gate 绑定 body + metadata。
- 最终 approval receipt 在 settlement 元数据投影完成后一次性生成。
- durable identity 不再使用 `DefaultHasher`。

元数据变化不能让已经验证且正文未变的 settlement 失效；正文变化必须让旧 audit、
settlement 和 approval receipt 全部失效。

### 2.8 状态只通过 typed delta 进入派生视图

summary、key facts、continuity updates 是展示/检索元数据，不是 durable truth 写入
接口。人物、关系、地点、世界规则、能力、资源和伏笔都必须由带以下字段的 typed
delta 表达：

- entity ID
- delta kind
- old/new value 或 transition
- final body 精确 evidence span
- execution authority allowance
- body/authority fingerprint

Story Bible、world database、character state 和 hook lifecycle 只消费批准后的 typed
delta。不得再从自然语言 metadata 猜测事实并回写状态。

### 2.9 减少 blocker 不能以删除剧情为代价

“hard blocker 数量减少”只是必要条件，不是候选成为 best 的充分条件。修订候选还必须
满足：

- 本章 required outcome 没有丢失。
- 已出现的受保护人物、关系、事实和伏笔推进没有被无依据删除。
- 没有通过大段删文规避冲突。
- 未引入更高优先级 blocker。
- 仍满足正文完整性和档位上限。

否则统一修订控制器可能通过删掉复杂内容降低 blocker 数量，却制造更隐蔽的情节漂移。

### 2.10 合同“可确认”与“可持续丰富”必须分层

开始第一章前真正必须锁定的是：

- 用户题材、目标字数和 2500/5000 档位
- canonical title 及命名依据
- 故事前提、主冲突、终局方向和不可违反条件
- 主角及首阶段必要人物的核心身份/欲望/恐惧/底线
- 世界硬规则和本题材真正需要的成长/资源/关系约束
- 全书阶段/分卷骨架，以及当前 3～8 章可执行窗口

角色声音配额、母题账本、细粒度揭示表等 rolling enrichment 不应阻止用户确认和第一章
开始；它们可以在 sealed authority 中标明 `unresolved_optional`，并在进入相关阶段前
补齐。现有 `ContractReadinessScope`、`PatchFieldStrength` 和
`field_is_rolling_longform_enrichment` 应作为唯一分层机制，不新增另一套“精简合同”。

### 2.11 合同修复与章节修订使用相同的候选治理原则

不要求共享同一个数据结构，但必须共享原则：

- typed issue owner，不按 issue 文本猜 patch stage
- 用户明确输入和已确认字段是 protected authority
- candidate 只有减少 blocker 且不回退 protected fields 才可成为 best
- 本地确定修复和 LLM semantic patch 分开计数
- LLM semantic patch 默认 1 次，明确净提升才允许第 2 次
- `Uncertain` 不等于 `Conflict`
- 预算耗尽保存 best pending candidate 并展示具体剩余问题，不继续 30 轮盲修

用户确认可以锁定结构完整且无确定冲突的合同；无证据的 semantic uncertainty 只作为
提示。用户确认不能覆盖缺失主角、目标档位、终局或明确自相矛盾等确定 blocker。

### 2.12 正文输出协议必须为长流式文本设计

Writer 的必需输出收缩为：

- 章节标题
- 完整正文

summary、key facts、continuity updates 从最终正文 observer/settlement 产生，不再要求
Writer 在长 JSON 末尾重复生成。协议可以继续支持旧 JSON 输入，但新主路径应使用
stream-safe envelope 或明确 title header + prose body：

- 正文中的引号和换行不需要 JSON 转义。
- 截断时能保存最长完整正文候选和明确 `truncated` 状态。
- parser recovery 只负责 legacy/provider 兼容，不能悄悄把 degraded 当 exact。
- metadata 不存在不再迫使正文重写。

### 2.13 “写完整本”必须是 durable 目标，不是单次超长调用

复用现有磁盘进度、ContinuousTaskExecutor、workflow lease 和逐章依赖，增加项目级
rolling batch coordinator：

```text
读取磁盘批准进度
-> 执行有限批次（例如 1～3 章）
-> 原子提交 batch checkpoint
-> 释放/续租 workflow lease
-> 从磁盘重新规划下一批
-> 直到 typed completion gate 通过
```

批次大小只是运行可靠性边界，不改变全书目标，也不把每批当成用户需要再次确认的任务。
进程重启后由同一 durable goal 自动恢复，而不是等待用户重新说“继续”。

---

## 3. 目标模块职责

### 3.1 `novel_governance.rs`

保留职责：

- ContextPackage
- RuleStack
- ChapterTrace
- ChapterControlContract
- review/truth governance DTO

新增职责：

- 封存章节权威包的数据结构
- authority fingerprint 计算
- 权威包完整性验证

不得承担：

- 正文修订循环
- LLM 审美评分
- Story Bible 状态提交

不得重复实现：

- `project_governance.rs::ensure_structured_contract_v2` 已有的合同优先级
- `context_packaging.rs` 已有的受保护/可压缩上下文选择
- `settlement.rs` 的正文证据验证

### 3.2 `novel_studio/context_packaging.rs`

唯一职责：

- 从合同、已批准 truth、Story Bible、近期章节和当前计划中选择基础上下文
- 生成可压缩/受保护上下文与 trace
- 从 `ensure_structured_contract_v2` 的结果生成唯一规范化合同投影
- 生成各角色 projection、coverage 和 exclusion trace

不得在正文生成后重新解释本章合同。
不得把兼容合同镜像、未来剧情正文或未批准状态当成第二权威。

### 3.3 `novel_studio/chapter_planning.rs`

唯一职责：

- 保存章节计划和架构
- 登记执行包中的新人物
- 将最终执行合同写入项目
- 封存本章唯一权威包

封存动作必须在现有 workflow lock/heartbeat 租约内完成；不新增第二套写作锁。

### 3.4 `chapter_quality.rs` 与 `novel_studio/quality_gate.rs`

唯一职责：

- 定义 typed finding
- 运行确定性质量检查
- 根据 finding disposition 输出批准、修订、metadata repair 或 warning

### 3.5 `novel_workflow_driver/audit.rs`

唯一职责：

- 请求 LLM 提供结构化语义 finding
- 校验 finding 证据
- 将通过验证的权威冲突交给 typed quality gate
- 将主观意见保存为 advisory

不得：

- 根据 score 独立决定 `needs_revision`
- 根据自由文本措辞决定 hard/soft

### 3.6 `novel_workflow_driver/chapter_loop.rs`

唯一职责：

- 决定本地清洗、补尾、补字数、metadata repair、语义修订或停止
- 维护唯一修订预算
- 比较候选净提升
- 维护最佳版本和回滚

新章节和恢复章节必须进入同一个入口。

### 3.7 `novel_studio/settlement.rs` 与 `state_truth.rs`

唯一职责：

- 从最终正文生成 pending settlement
- 验证正文证据和权威许可
- 失败时进入 `state_repair_required`
- 批准后提交 truth

不得根据 writer metadata 反向改写 settlement。

其中：

- `settlement.rs` 负责唯一的 evidence validator 和 typed pending delta。
- `state_truth.rs` 负责验证、提交和从批准 receipt 重建派生状态。
- `repair_project_state` 不得静默改写已批准章节正文或元数据，只能重建派生投影；真正
  的历史更正必须走显式用户纠错/迁移。

### 3.8 `novel_workflow_driver/chapter.rs`

降级为编排层：

- 获取封存权威
- 调用 Writer
- 调用统一质量门
- 调用统一修订控制器
- 调用 settlement
- 调用批准

不得保留第二套质量判定和修订循环。

### 3.9 `novel_studio/runtime_records.rs`

唯一职责：

- 提供 canonical SHA-256 fingerprint 类型与实现
- 保存 `RevisionSessionRecord`、完整候选记录和 `ApprovalReceipt`
- 验证 body、metadata、authority 和 receipt 之间的依赖关系

不得使用 `DefaultHasher` 作为 durable fingerprint，不得用同一字符串表示不同输入
语义。

### 3.10 `novel_bible/core.rs` 与 `novel_bible/contract_settlement.rs`

唯一职责：

- 从已批准 typed delta 重建人物、世界、关系、资源和伏笔派生视图
- 保留现有 HookStatus 生命周期和逾期规则

不得从 summary、key facts、continuity updates 的自然语言文本创建 durable 世界规则
或人物状态。

### 3.11 `novel_pipeline/lifecycle.rs`

作为唯一章节状态机：

- 定义合法状态和合法转移
- 读取旧字符串别名，但生产写入只使用 canonical typed state
- 拒绝 add/revise/import 直接转为 approved

不得在 workflow、storage 或恢复代码中再建另一份状态字符串判断表。

### 3.12 `novel_studio/tool_schema.rs`、`novel_studio.rs` 与各写入 action

负责在工具/存储边界执行：

- mutation action 权限和状态转移约束
- review/settlement/approval receipt 完整性
- `update_truth` 显式管理覆盖隔离
- import migration 状态

正常自然语言写作仍以 `run_next_chapter`/`run_project` 为主；低层 action 即使保留给
内部测试，也不能绕过上述不变量。

### 3.13 `creation_contract/repair_coordinator.rs`、`issue.rs` 与 typed gate

保留：

- typed patch
- staged repair
- pending best candidate
- `ContractRepairProgress`
- `ContractReadinessScope`/`PatchFieldStrength`

整改：

- issue 在产生处携带 typed owner/code，不再由 `ContractIssueKind::from_text` 猜 stage
- 合同 semantic finding 复用 evidence-grade 原则，`Uncertain` 不 hard block
- 合同候选比较保护用户权威和已正确字段
- 删除 30 轮总循环，改为有限 local repair + 最多 1～2 次 LLM semantic patch

### 3.14 `novel_runner/core/protocol.rs`、`draft.rs` 与 Writer prompt

负责 stream-safe 正文协议和 parse provenance：

- 新主路径只要求 title + body
- 明确区分 exact、recovered、truncated
- 允许旧 JSON 作为兼容输入
- 不再要求 Writer 产生 durable summary/facts/continuity

不得通过无结构 fallback 静默把截断正文标记为完整。

### 3.15 `novel_workflow_driver/completion.rs` 与项目级 coordinator

`completion.rs` 只负责：

- 目标字数/章节进度
- 合同 must-resolve 和 ending obligation
- typed state、hook/payoff 生命周期
- allowed open questions

LLM 可以给结尾建议，但不能凭主观结论或关键词决定 complete。项目级 coordinator
复用 ContinuousTaskExecutor 执行 durable rolling batches，不新建章节 runner。

### 3.16 `novel_studio/storage.rs`、批准流程与 Story Bible reducer

负责：

- per-chapter approval journal/transaction ID
- 幂等 commit 和崩溃恢复
- final receipt 作为提交标记
- 正常批准增量应用一个 typed delta
- 显式 audit/repair 时全量重建派生视图

继续复用现有 project lock、atomic file 和 snapshot staging；不得建立平行数据库或第二
份 truth。

---

## 4. 六阶段实施计划

### 实施总准则：每一个改动项都必须执行

以下不是建议，而是前置 A、Phase 1～6 每一个实现项的强制准入条件。未完成核对记录，
不得开始写新机制。

#### 准则 1：实施前必须先做现有代码机制清单

每个改动项开始前必须用 `rg`、调用链和测试反向核对，至少记录：

- 当前唯一或候选 owner 模块
- 现有数据结构、常量、helper、prompt 和状态字段
- 所有生产调用者与直接/间接写入者
- 首次写作、恢复、导入、修复和低层 action 是否存在旁路
- 现有单元测试、集成测试和真实运行证据
- 与目标机制语义相同、相近或冲突的旧实现
- 需要保留的旧项目兼容字段

不得根据文件名或昨日记忆判断“系统没有这个机制”；必须以当前工作树的生产调用链为
准。

每项实现前形成最小映射表：

| 字段 | 必填内容 |
| --- | --- |
| 目标不变量 | 本项最终只允许哪一个事实来源/状态转移 |
| 当前 owner | 当前真正执行该职责的模块和函数 |
| 重复候选 | 语义相同或相近的其他函数/循环/prompt |
| 冲突路径 | 会产生不同结论或绕过 owner 的路径 |
| 复用决定 | 直接复用、修正现有实现，还是确有缺口才新增 |
| 删除清单 | 新路径接通后必须删除的代码、测试、常量和 prompt |
| 兼容清单 | 暂时保留但不得再参与生产决策的 serde/legacy 字段 |
| 验证证据 | 聚焦测试、故障注入、`rg` 和真实运行检查 |

#### 准则 2：决策顺序固定为“复用 → 修正 → 替换 → 新增”

1. 现有机制正确：直接复用，不包装出第二个 owner。
2. 现有机制方向正确但实现有缺陷：在原 owner 内修正。
3. 现有机制职责错误：新实现接管后删除旧生产路径。
4. 只有调用链证明不存在相应能力时，才允许新增机制。

“现有函数不好接”“新写更快”“测试里暂时方便”都不能作为新增平行机制的理由。

如果必须新增数据结构或 controller，提交说明必须写清：

- 现有结构为什么不能安全扩展
- 新结构接管的唯一职责
- 哪些旧结构/调用会在同一 Phase 被删除
- 如何证明不会形成两个权威

#### 准则 3：不能用新 wrapper 掩盖旧重复机制

禁止以下伪重构：

- 新建统一 controller，但内部仍调用两套旧循环。
- 新增 typed finding，同时继续用字符串函数决定 hard/soft。
- 新增 sealed authority，但 Auditor/Observer 仍可重新 compose context。
- 新增 typed settlement，但 Story Bible 仍从 summary/key facts 推导事实。
- 新增 lifecycle，同时生产代码继续直接写状态字符串。
- 新增 SHA-256 receipt，但旧 `DefaultHasher` 仍参与 durable identity。
- 新增 rolling batch，但内部另建一套章节 runner。

验收必须检查最终生产调用链，而不仅检查新类型是否已经存在。

#### 准则 4：替换必须完成“接通新路径 + 删除旧路径”

每项替换在同一 Phase 内按以下顺序完成：

```text
给现有行为增加保护性测试
-> 接通新唯一 owner
-> 迁移所有生产调用者
-> 验证新路径
-> 删除旧调用和旧实现
-> 删除只服务旧实现的测试、常量、prompt、DTO 和 helper
-> rg 确认无生产引用
```

不得长期保留“新旧都能工作”的双轨状态。短暂过渡若不可避免，必须：

- 有明确 feature/migration 边界
- 默认只走新路径
- 记录删除提交和最迟删除 Phase
- 不允许两个路径同时写同一 durable state

#### 准则 5：弃用代码必须删除，但兼容数据要区分处理

必须删除：

- 已被替换且无生产调用的函数、循环、classifier 和 fallback
- 只为旧机制存在的常量、prompt、DTO、测试和 re-export
- 被新 owner 接管后仍可能被误调用的 public action
- 针对某个题材、某个人名或某条错误措辞添加的特例判断
- 永久关闭但仍留在生产代码中的死 feature branch

不能直接删除：

- 读取旧项目所必需的 serde 字段
- 明确用于一次性迁移的 legacy parser
- 用户数据中仍可能存在的历史状态别名

兼容代码必须满足：

- 只读，不再成为新写入或决策权威
- 带 `legacy`/migration 注释和删除条件
- 有旧项目读取测试
- 转换后立即进入 canonical 新结构

“为了保险先留着”不能成为保留旧生产机制的理由。

#### 准则 6：不得用题材、人名和错误文案特例修复架构问题

修复必须针对：

- typed field
- entity ID
- lifecycle transition
- evidence relationship
- authority dependency
- schema invariant

不得针对：

- 玄幻、言情、赛博朋克等单一题材单独放行
- “林默”“主教”等具体测试名字
- 某一模型的一句固定错误输出
- “略显、严重、建议”等字符串措辞继续补关键词

题材差异只能通过已有 genre profile/field strength 数据表达，不得产生另一套 workflow。

#### 准则 7：每个 Phase 都要限制 diff 和职责扩散

开始前记录基线：

```text
git status --short
git diff --stat
相关模块生产引用数量
相关旧机制符号清单
```

完成后再次记录。若单项实现出现以下情况，必须暂停并重新审查：

- diff 明显超过预估
- 一个修复扩散到无关聊天、通用 agent 或 UI 层
- 为通过测试连续新增多个特例
- 同一职责同时落入三个以上 owner
- 旧代码没有减少，新增代码却持续增长

写作领域逻辑应留在 writing/novel 模块；不得再次把小说合同、命名、审稿或状态治理
塞入通用 `chat.rs`。

#### 准则 8：测试必须证明“旧机制已不能生效”

每项至少包含：

1. 保护性测试：修改前复现当前问题。
2. 正向测试：新唯一 owner 得到正确结果。
3. 反向测试：旧旁路、旧状态、伪造 receipt 或字符串特例不能再影响结果。
4. 恢复测试：暂停/崩溃/重启后仍走相同 owner。
5. 兼容测试：旧项目能读，但不会重新启用旧权威。
6. `rg` 删除检查：弃用符号无生产引用。

只新增测试验证新 helper，不足以证明重复机制已经消失。

#### 准则 9：每个提交必须是可编译、可恢复、可回滚的单一职责提交

每个 Phase 可以拆成多个小提交，但每个提交必须：

- 只改变一个明确不变量
- 不把“新增新路径”和“以后再删除旧路径”跨越多个长期提交
- 通过格式、编译和相关聚焦测试
- 不包含运行数据库、生成小说、日志和无关用户改动
- 在提交说明中列出复用、删除、兼容和验证结果

建议提交说明附带：

```text
Reused:
Replaced:
Deleted:
Legacy compatibility:
Invariant tests:
Remaining follow-up:
```

#### 准则 10：实现完成后才能进行真实模型测试

真实模型测试不是用来代替架构核对。顺序必须是：

```text
代码清单和冲突核对
-> 聚焦实现
-> 删除旧机制
-> 静态/单元/故障注入验证
-> 新 session、新项目、新题材真实测试
```

真实测试发现问题后，下一次修改仍必须重新执行准则 1～9；不能直接依据一轮输出增加
特例。

实施前先增加保护性回归测试，并完成下面的合同收敛前置整改；它不改变原六项需求，
也不单独计为一个 Phase。

### 实施前置 A：让合同有限收敛并可确认

#### 复用

- `ContractReadinessScope`
- `PatchFieldStrength`
- typed creation contract/patch
- staged repair
- pending best candidate
- `ContractRepairProgress`
- 本地命名治理

#### 改动

1. 将 contract gate issue 改成 typed code + owner + evidence，逐步删除
   `ContractIssueKind::from_text` 的生产分流。
2. 明确 `LockedAuthorityContract` 的最小硬字段，rolling enrichment 不阻止第一章。
3. 合同 candidate 比较增加：

   - user authority preserved
   - confirmed fields preserved
   - blocker count/vector
   - cross-field contradiction count
   - protected-field regression

4. 本地 metadata/格式修复每种最多一次；LLM semantic patch 默认一次，只有净提升才
   允许第二次。
5. 删除 `MAX_CREATION_CONTRACT_AUTO_REPAIR_ATTEMPTS = 30`。
6. semantic reviewer：

   - `Conflict` 必须带用户权威字段和候选字段的精确引用
   - `Uncertain` 记录 advisory，不重开完整合同
   - provider unavailable/JSON parse failure 是 runtime 状态，不是合同冲突

7. 用户确认时，结构完整且无确定矛盾即可锁定；之后任何机器修复都不能改变用户已确认
   字段。
8. `approve_draft` 使用临时项目目录完成 init + set_contract + title/character
   authority 验证，全部成功后一次 rename 到正式项目路径；不能先暴露空项目再失败删除。

#### 验收

- 任意新题材合同的 LLM semantic patch 绝对不超过 2 次。
- 同一个 issue 不因中文措辞变化切换 patch owner。
- semantic reviewer 返回 `Uncertain` 不会单独阻止用户确认。
- 2500/5000 档位和任意总字数在合同批准后保持原值。
- 合同批准中途崩溃不会留下可被续写的半初始化项目。

六个正式 Phase 如下。

## Phase 1：封存唯一章节权威包

### 目标

让 Writer、Auditor、Reviser、State Observer 在同一章内永远读取同一个只读权威版本。

### 复用

- `ContextPackage`
- `RuleStack`
- `ChapterTrace`
- `ChapterExecutionContractV2`
- `ChapterContractRecord`
- `ChapterArchitectureRecord`
- `ContextPackageRecord`
- 本地人物登记
- 原子文件写入

### 改动

1. 在 `novel_governance.rs` 增加 `SealedChapterAuthority`：

   ```rust
   struct SealedChapterAuthority {
       schema_version: String,
       chapter_number: usize,
       canonical_contract: Value,
       truth_as_of_chapter: Value,
       truth_cutoff_chapter: usize,
       context_package: ContextPackage,
       rule_stack: RuleStack,
       trace: ChapterTrace,
       chapter_contract: ChapterContractRecord,
       chapter_architecture: ChapterArchitectureRecord,
       character_registrations: Vec<ChapterCharacterRegistration>,
       role_projections: BTreeMap<AuthorityRole, AuthorityProjectionRecord>,
       authority_root_fingerprint: AuthorityFingerprint,
       protected_coverage: AuthorityCoverage,
       sealed_at: String,
   }
   ```

   `canonical_contract` 必须来自现有 `ensure_structured_contract_v2` 权威结果；不得把
   StoryContract、manifest mirror 和 Story Bible 合同摘要重复封存。

2. 扩展 `ContextPackageRecord`，或新增独立的 authority record：

   - `authority_root_fingerprint`
   - `sealed`
   - `sealed_at`
   - `chapter_contract_fingerprint`
   - `truth_fingerprint`
   - `truth_cutoff_chapter`
   - `role_projection_fingerprints`
   - `protected_coverage`
   - `excluded_future_paths`

   新字段全部提供 serde default，避免破坏旧项目读取。

3. `compose_context` 继续生成基础上下文，但标记为 `sealed=false`。

4. `persist_execution_package` 完成以下步骤后才能封存：

   - 保存计划
   - 保存架构
   - 写入 `ChapterExecutionContractV2`
   - 完成本地人物命名和登记
   - 将 request ID 全部替换为权威姓名
   - 合并 ContextPackage、RuleStack、执行合同、架构和人物登记
   - 只选择截至本章开始时可见的已批准 truth 和历史
   - 构造 Writer/Auditor/Reviser/Observer role projection
   - 验证所有受保护字段均未被截断
   - 计算 root 和各 projection fingerprint
   - 原子写回现有章节 context artifact
   - 标记 `sealed=true`

5. `NovelChapterRunner` 在现有 workflow lock 内加载一次
   `Arc<SealedChapterAuthority>`。

6. Writer、Auditor、Reviser、Observer prompt 全部接收从该对象生成的只读 role
   projection。禁止阶段内重新 `compose_context`，禁止用 12,000 字符的无结构文本
   截断代替 projection。

7. 所有 draft、review、revision、settlement 记录 authority root 和实际 role
   projection fingerprint。

8. protected coverage 不完整、执行包无法持久化或只能生成 degraded/minimal context
   时，本章不得生成/批准正文。minimal context 仅可用于明确的诊断和迁移，不可成为
   无提示降级的批准通道。

9. 建立依赖失效链：

   ```text
   canonical contract/truth revision
   -> sealed authority
   -> execution package
   -> draft candidates
   -> review findings
   -> settlement
   -> approval receipt
   ```

   上游改变后，全部未批准后代标记 stale。已有批准内容不能被新合同静默重新解释；
   若用户要整体换故事或改变已批准历史的根合同，应新建项目或执行显式迁移。

10. 同一 Phase 先加存储边界保护，避免后续阶段的新结构仍可被旧旁路绕开：

    - add/revise/import 不能直接写 approved
    - 只有 canonical approve action 可以执行批准状态转移
    - 普通自动写作不能直接写 durable truth

    Phase 5 再完成 schema 收口、旧项目迁移和旧旁路删除。

### 删除/替换

- 删除 `authoritative_chapter_audit_context_json` 的独立组装职责。
- 将 `authoritative_chapter_context_json` 改为只读已封存权威。
- 删除旧草稿恢复时重新生成执行包。
- 删除“执行包持久化失败但继续用内存包写正文”的路径；持久化失败就停止，不把
  未记录的内存权威交给 Writer。
- 删除批准路径的 degraded minimal-context 旁路。
- 删除 prompt 中重复的合同兼容镜像；存储兼容字段继续由现有治理代码维护。

### 验收

- 四个阶段的 fingerprint 完全一致。
- 四个阶段的 root fingerprint 一致，各 projection 的差异均有明确 trace，关键受
  保护路径 coverage 为 100%，protected authority core 内容完全一致。
- 人物登记后的权威姓名在四个阶段均可见。
- 正文生成后修改 manifest 不会静默改变本章权威。
- 合同中途变化时返回 `authority_stale`，不得用新合同审旧正文。
- 旧项目重建第 N 章权威时不能读到 N 章之后的 truth 或正文。

## Phase 2：建立证据化故事权威硬门

### 目标

保留真实漂移阻断，删除主观质量和自然语言措辞造成的假阻断。

### 复用

- `ChapterQualityGate`
- `ChapterMetadataGate`
- `chapter_quality_decision`
- `novel_studio/quality_checks/*`
- `truth_validation`
- `contract_terms`
- 现有人物身份、合同泄漏、正文污染和连续性检查

### 改动

1. 在 `chapter_quality.rs` 定义：

   ```rust
   enum ChapterFindingClass {
       Contract,
       Continuity,
       State,
       BodyIntegrity,
       Length,
       Metadata,
       Advisory,
   }

   enum ChapterFindingDisposition {
       HardBlock,
       DeterministicRepair,
       Warning,
   }

   struct ChapterFinding {
       code: String,
       class: ChapterFindingClass,
       disposition: ChapterFindingDisposition,
       evidence_grade: FindingEvidenceGrade,
       source: String,
       message: String,
       authority_evidence: Vec<AuthorityEvidenceRef>,
       body_evidence: Vec<BodyEvidenceSpan>,
       authority_fingerprint: String,
       body_fingerprint: String,
   }
   ```

2. 将确定性检查逐步改为直接返回 typed finding，不再先生成字符串、再根据字符串分类。

3. 明确 hard blocker code 白名单，但白名单只定义“可能成为 hard”的类型，不代表
   任意同名 LLM finding 自动 hard：

   - `character_identity_conflict`
   - `character_name_replacement`
   - `unregistered_character`
   - `character_pronoun_conflict`
   - `relationship_state_conflict`
   - `world_rule_conflict`
   - `timeline_conflict`
   - `location_continuity_conflict`
   - `ability_or_resource_conflict`
   - `future_chapter_consumed`
   - `chapter_goal_replaced`
   - `unplanned_main_branch`
   - `premature_hook_payoff`
   - `unsupported_hook_resolution`
   - `state_change_outside_execution_contract`
   - `body_truncated`
   - `body_missing`
   - `body_surface_contamination`
   - `length_below_minimum`
   - `length_above_tier_maximum`
   - `authority_fingerprint_mismatch`
   - `state_validation_failed`

4. LLM audit 改为输出：

   - `authority_conflicts`
   - `advisories`
   - `score`

5. 本地按 conflict kind 验证 authority conflict。通用检查只验证引用真实性；人物
   身份、地点时间线、关系转移、世界规则、伏笔回收等必须分别验证其结构关系。通过
   对应 validator 后才映射为 hard finding。

6. score 只写 telemetry，不进入 verdict。

7. `body_revision_required_after_audit` 只检查 typed hard blockers。

8. 对现有 `quality_checks` 做一次逐项迁移表，不允许整体把旧字符串检查包装成
   `HardBlock`：

   - body missing、污染、截断、确定字数越界：`DeterministicInvariant`
   - 明确姓名替换且旧名/权威名映射可证明：`DeterministicInvariant`
   - 人物身份、关系、世界、时间线、伏笔：`EvidenceBackedSemantic`
   - 通用叙事力度、重复感、段落观感、结尾模式、进展不足：默认 `Advisory`
   - “缺少稳定锚点”等启发式只有绑定合同 required outcome 后才可能升级

9. review 结果保存为 typed `ReviewReceipt`，绑定 body + authority。批准逻辑只接受
   由本地 validator 产生的 receipt，不接受调用者传入字符串 verdict 代替。

### 删除

- `audit_issue_is_actionable`
- `audit_issue_has_hard_blocking_marker`
- `audit_has_only_non_actionable_issues`
- 所有依靠“略显、建议、严重、重复”等词语判断 hard/soft 的特例函数
- `score >= 85` 的阻断条件
- `passed && issues.is_empty()` 对自由文本 issues 的批准依赖
- `novel_governance.rs` 与 `settlement.rs` 中重复的弱正文支持算法；合并到唯一
  evidence validator

### 验收

- 0 分但无 hard finding 的正文不会被重写。
- “节奏慢、描写少、语言一般”只进入 advisory。
- 带有效权威证据和正文证据的人物、世界观、时间线或伏笔冲突仍然阻断。
- finding 缺少有效引用时不能升级为 hard blocker。
- 引用真实但逻辑上不构成冲突的 finding 不能升级为 hard blocker。
- 相同 typed finding 在首次写作、恢复、直接 review action 中得到同一 disposition。

## Phase 3：统一有限修订、净提升和最佳版本

### 目标

所有正文路径使用同一个修订入口；减少无意义重写，但绝不自动放行未解决漂移。

### 复用

- `ChapterLoopDecision`
- `chapter_loop.rs`
- 现有 `revision_quality_score` 的确定性部分
- 非提升回滚逻辑
- `persist_stream_snapshot`
- `ReviewCycleRecord`
- 正文 fingerprint

### 改动

1. 在现有 `chapter_loop.rs` 建立唯一入口：

   ```rust
   run_bounded_revision_cycle(
       authority: &SealedChapterAuthority,
       initial_draft: DraftOutput,
       initial_findings: &[ChapterFinding],
       persisted_state: RevisionState,
   )
   ```

2. 新章节、暂停恢复、旧草稿复用、崩溃恢复全部进入该函数。

   外层 step retry、整章重新生成、final cleanup、tail recovery 和最终字数 top-up
   只负责把候选提交给该控制器，不得直接覆盖正式 draft。整章 regenerate 也算一次
   semantic candidate，不是隐藏的额外十轮。

3. 拆分预算：

   - 本地确定性清洗：每个 body fingerprint 最多 1 次
   - 字数补写：最多 1 次
   - 结尾补全：最多 1 次
   - LLM metadata repair：最多 1 次
   - LLM 语义修订：默认 1 次
   - 只有首次修订明确减少 hard blocker 且未引入新 blocker 时，允许第 2 次
   - 语义修订绝对不能进入第 3 次

4. 将 `revision_quality_score` 替换为确定性质量向量：

   ```rust
   struct RevisionQualityVector {
       hard_blockers: usize,
       authority_conflicts: usize,
       state_conflicts: usize,
       required_outcomes_missing: usize,
       protected_facts_lost: usize,
       new_high_priority_blockers: usize,
       material_deletion_ratio: u16,
       incomplete_body: bool,
       contaminated_body: bool,
       degenerate_repetition: bool,
       length_violation: usize,
       deterministic_repairs: usize,
   }
   ```

5. 候选只有满足以下条件才成为 best：

   - hard blocker 严格减少
   - 或从有 blocker 变为无 blocker
   - 且没有引入新的更高优先级 blocker
   - required outcome、已出现人物/关系事实和必要伏笔推进没有回退
   - 没有靠大段删除正文规避问题
   - 仍符合完整性和档位上限

6. LLM score、文风 advisory 数量不参与 best 比较。

7. 新增或扩展 durable 记录：

   ```rust
   struct DraftCandidateRecord {
       candidate_id: String,
       parent_candidate_id: Option<String>,
       authority_fingerprint: AuthorityFingerprint,
       body_fingerprint: BodyFingerprint,
       metadata_fingerprint: MetadataFingerprint,
       draft: DraftOutput,
       findings: Vec<ChapterFinding>,
       quality_vector: RevisionQualityVector,
       provenance: CandidateProvenance,
       accepted_as_best: bool,
   }
   ```

   `ReviewCycleRecord` 增加：

   - `attempt_kind`
   - `candidate_fingerprint`
   - `quality_vector`
   - `accepted_as_best`
   - `best_candidate_path`

8. stream snapshot 使用 iteration/fingerprint 命名，不再覆盖同一个
   `revise.stream.txt`。snapshot 必须关联完整 `DraftCandidateRecord`，不能只有 body
   文本。

9. 正式草稿只在候选成为 best 后替换。崩溃恢复读取最后一个 `accepted_as_best=true` 的候选。

10. 预算耗尽后：

    - 无 hard blocker：继续 settlement。
    - 有 hard blocker：回滚 best，保留 `needs_revision`，阻止批准。

11. 复用 `novel_pipeline/lifecycle.rs` 收口状态转移。所有新写入使用 typed canonical
    state；旧字符串仅在反序列化时兼容。只有批准 action 可以执行
    `ReviewPassed/StateReady -> Approved`。

12. 已接受候选记录、sealed authority 和 approval receipt 必须进入可恢复的 durable
    project snapshot。现有轻量批准快照若排除 runtime 目录，就不能把这些权威记录只
    放在会被排除的临时 runtime 路径。

13. 将 Writer/Reviser 主路径切换为 stream-safe title + body 协议。`DraftOutput` 可以
    在迁移期保留 summary/key facts/continuity 字段，但它们不再是必填，也不进入
    durable truth。正文输出被截断时保存 `truncated` candidate，先进行一次有界补尾，
    不能把 jsonish recovery 当作完整正文重新走十轮。

14. body generation、length top-up、tail completion 都必须遵守同一档位上限：

    - 2500 档正文可超过目标，但不得超过 5000
    - 5000 档正文可超过目标，但不得超过 10000

    `hard_max_chars`、quality gate 和保存边界使用同一 tier policy，不能各自计算不同
    上限。

### 删除

- `MAX_CHAPTER_REVISION_ATTEMPTS = 10`
- `revise_reusable_existing_chapter_once`
- 恢复旧草稿的独立语义修订 while 循环
- 根据 `next_action.contains("revis")` 猜语义修订次数
- 独立的主链/恢复链 stalled 计数
- 旧标量 `revision_quality_score`
- 以正文长度/完成度为主选择“最佳恢复正文”的旧逻辑
- 任何可以在 unified controller 之外替换正式正文的 retry/regenerate/top-up 路径
- 生产路径中的原始字符串状态写入和重复状态判断表
- 新主路径中要求数千字正文完整嵌入 JSON string 的 Writer 输出合同
- Writer metadata 缺失导致整章正文重写的路径

### 验收

- 每章语义修订默认不超过 1 次，绝对不超过 2 次。
- 新章节与恢复章节使用同一入口、同一停止条件和同一 best 选择器。
- 非净提升候选不能覆盖当前 best。
- 删除主要剧情或 required outcome 的候选，即使 blocker 变少也不能成为 best。
- 进程在候选写入后崩溃，重启仍恢复最后 best。
- hard blocker 未消除时绝不自动批准。
- add/revise/import 传入 `approved` 时被存储边界拒绝。
- 5000 字正文即使 JSON 尾部缺失，也能以明确 truncated/recovered provenance 保存
  body candidate，而不是丢弃整章或伪装 exact。

## Phase 4：收口最终正文状态结算

### 目标

人物、世界观、关系和伏笔状态只能由最终正文和本章权威许可共同决定。

### 复用

- `final_chapter_observer_prompt`
- `FinalChapterObservation`
- `SettlementOutput`
- `validated_settlement_from_final_body`
- `deterministic_state_validation`
- `validate_settlement_for_chapter`
- pending settlement
- `state_repair_required`
- `apply_pending_settlement_to_truth`
- Story Bible 重建

### 改动

1. Settlement 增加：

   - `body_fingerprint`
   - `authority_fingerprint`
   - typed `state_changes`
   - 每项 state change 的正文精确 span、实体 ID 和事件类型
   - 每项 state change 的允许范围匹配结果

   需要同步修改：

   - `novel_runner/core/model.rs::FinalChapterObservation`
   - `novel_studio/model.rs::SettlementOutput`
   - `novel_bible` 使用的 `ApprovedChapterDelta`
   - pending settlement 和 approval receipt 序列化结构

2. 将状态验证拆成两个连续步骤：

   ```text
   final body evidence validation
   -> chapter authority allowance validation
   ```

3. 状态变化许可来源：

   - `new_state_after_chapter`
   - `character_change`
   - `relationship_delta`
   - `world_change`
   - `power_delta`
   - `resource_delta`
   - `hook_opened`
   - `hook_paid_off`
   - 本章允许的 bounded incidental state change

4. incidental state change 必须满足：

   - 不改变角色身份、核心能力和底线
   - 不改变世界硬规则
   - 不提前回收未来伏笔
   - 不开启新主线
   - 有最终正文证据

5. 新项目中删除隐式 `default_pending_settlement_from_chapter` 批准 fallback。observer
   失败时：

   - 保留旧 truth
   - 记录明确的 degraded reason
   - 进入 `state_repair_required`

   旧项目迁移如必须 fallback，也只能生成“无状态变化”的 degraded settlement，
   不能复用 writer continuity metadata，且不能静默放行含状态变化的章节。

6. metadata 改变时：

   - 正文没变：保留 settlement，不改写其中事实
   - 正文变了：旧 settlement 标记 stale，重新从最终正文结算

7. 批准前同时验证 body fingerprint 和 authority fingerprint。

8. 只有批准后的 settlement 才能进入：

   - current state truth
   - chapter summaries
   - character state
   - world database
   - hook lifecycle
   - Story Bible

9. 改造 `novel_bible/core.rs` 与 `novel_bible/contract_settlement.rs`：

   - 从 approved typed delta 更新人物、世界、关系、能力、资源和伏笔。
   - summary/key facts/continuity 只用于展示和检索。
   - 不再根据“看起来像世界规则”的文本自动创建 durable world rule。
   - 保留现有 HookStatus 生命周期，但新 seed 也必须经过本章 authority allowance。

10. 修正批准顺序和指纹：

   ```text
   validate review(body + authority)
   -> validate settlement(body + authority)
   -> derive display metadata from validated settlement
   -> run metadata gate(body + metadata)
   -> freeze final chapter record
   -> create approval receipt once
   -> commit truth
   ```

   settlement 后不得再次修改指纹覆盖范围内的 final chapter。metadata-only repair 只
   重跑 metadata gate，不重跑正文 semantic audit。

11. 改造 `repair_project_state`：只能从批准 receipt/settlement 重建 derived truth，
    不得用弱默认 settlement 静默重写已批准章节的摘要或连续性字段。

12. 将 `update_truth` 从普通模型写作动作中隔离。只有显式用户/管理修正可以调用，
    必须保存 provenance，并使受影响章节后的 sealed authority、draft、review、
    settlement 全部 stale。

13. 将单章批准改为幂等事务：

    ```text
    write approval journal(prepared, transaction_id)
    -> stage final chapter/settlement/truth/bible/manifest
    -> validate staged dependency fingerprints
    -> atomic install or recoverable commit sequence
    -> write ApprovalReceipt + journal(committed)
    ```

    同一 transaction ID 重试只能返回已提交 receipt，不能重复应用人物登记、伏笔更新
    或 truth delta。

14. Story Bible 正常热路径增量应用本章 approved typed delta；保留
    `rebuild_story_bible_from_manifest` 作为审计、迁移和显式 repair 使用，不再在每次
   普通 manifest read/write 时无条件全量重建。

### 删除

- `sync_pending_settlement_metadata`
- settlement 对 writer summary/key facts/continuity 的反向信任
- metadata repair 后直接修改 settlement 事实内容的路径
- 从批准章节自然语言 metadata 猜测 durable 人物/世界状态的 reducer
- 新流程中“observer 失败则复用 writer metadata”的默认 settlement
- 每次普通 manifest read/write 都无条件全量重建 Story Bible 的热路径

### 验收

- writer metadata 声称发生、但正文没有发生的状态不会进入 truth。
- 正文发生、但执行合同不允许的重大变化会触发 `state_repair_required`。
- settlement 失败后旧 truth 文件内容和哈希保持不变。
- 伏笔 seed、advance、pay_off、defer 都必须有正文证据。
- 未批准章节不更新 Story Bible。
- settlement 写回展示 metadata 后，最终 approval receipt 的 body/metadata/authority
  指纹仍全部匹配。
- 常见词或 bigram 命中不能单独授权 durable 状态变化。
- 在批准事务的任一文件写入后崩溃，重启只能恢复为完整未提交或完整已提交状态。
- 同一批准 transaction 重放不会重复推进伏笔、资源、关系或人物状态。

## Phase 5：旧项目兼容、删除旧机制与集成验证

### 目标

安全迁移旧项目，彻底删除已经被新主线替换的旧判断和循环。

### 旧项目策略

#### 已批准章节

- 不修改正文。
- 不重新审稿。
- 不重新生成执行包。
- 只在内存反序列化时补默认字段。
- Story Bible 继续以已批准记录为权威。
- 生成 legacy approval receipt 时只证明“这是历史批准记录”，不得伪造当时并不存在的
  typed audit/settlement。
- 若需要从历史正文重建派生状态，必须按该章当时可见的历史做 as-of 重建，不能读取
  后续章节反向补前章权威。

#### 未批准章节

如果缺少 sealed authority：

1. 从现有 ContextPackage、ChapterContractRecord、ArchitectureRecord 重建一次。
2. 生成 authority fingerprint。
3. 将现有正文登记为 legacy candidate。
4. 只运行 typed hard gate。
5. 没有 hard blocker 时继续 settlement。
6. 不因为历史低分重写正文。
7. 有证据化漂移时进入统一有限修订。

#### 导入章节

- 不接受调用方用 `status=approved` 直接成为项目权威。
- 默认状态为 `imported_unverified`。
- 只有完成 authority reconstruction、typed hard gate、final-body settlement 和正式
  approval receipt 后才能进入 approved history。

#### 旧 settlement

- 按最终正文重新验证。
- 旧 fingerprint 不兼容时重新结算。
- 在新 settlement 批准前不修改旧 truth。

### 存储边界收口

- `add_chapter`、`revise_chapter`、`import_chapters` 忽略或拒绝 `approved` 输入。
- `review_chapter` 的调用方 verdict 只作为候选输入，不能替代 typed review receipt。
- `approve_chapter` 验证 lifecycle、sealed authority、body、review、metadata、
  settlement 全依赖链。
- `update_truth` 改成显式管理覆盖，不出现在普通自动写作的可选 mutation 路径中。
- 现有项目锁/心跳继续作为唯一写作 lease；不新增第二套锁。
- staging/backup 快照继续复用，但必须包含 sealed authority、best candidate 和
  approval receipt；恢复后逐项校验依赖指纹。

### 全书持续执行与完成门

1. `run_project` 写入 durable project goal：

   - target units
   - chapter tier
   - contract/authority fingerprint
   - next approved chapter
   - run status
   - cancellation/explicit pause state

2. 复用 `ContinuousTaskExecutor` 每次执行有限 rolling batch。batch 完成后从磁盘重新
   读取连续批准进度再规划下一批；不一次把 40～200 章全部放入一个内存 plan。
3. provider/runtime 暂停时保留 active durable goal；服务恢复后从最后 committed
   approval receipt 自动续跑。只有用户明确暂停/取消、确定 hard blocker 或达到 typed
   completion 才停止。
4. completion gate 改为验证：

   - approved units 达到合同目标
   - ending desired resolution/final state 有 approved typed state evidence
   - must-resolve/payoff obligations 已完成
   - 未解决 hooks 均属于 allowed open questions，且没有 overdue hard debt
   - 最后一章 approval receipt 与当前 truth 匹配

5. LLM completion review 和文风建议只作 advisory/finale brief，不决定 complete。
6. 删除 `text_has_closure_signal`、`text_has_midstory_expansion_signal` 的 hard gate
   职责，以及“最多追加 3/8 章后停止但仍称完整”的行为。若 typed obligation 仍未完成，
   由章节执行合同明确消费剩余债务；无净进展时返回具体 blocker，不盲目无限追加。

### 删除确认

使用 `rg` 确认生产路径中不再存在：

- `MAX_CHAPTER_REVISION_ATTEMPTS`
- `audit_issue_is_actionable`
- `audit_issue_has_hard_blocking_marker`
- `audit_has_only_non_actionable_issues`
- `revise_reusable_existing_chapter_once`
- `sync_pending_settlement_metadata`
- 恢复路径中的执行包重新生成
- 多个语义修订循环
- 可直接写 `approved` 的 add/revise/import 路径
- 普通自动写作可直接调用的 durable `update_truth`
- 重复的 fingerprint 工具和 `DefaultHasher` durable identity
- 从自然语言 metadata 写人物/世界 durable state 的旧 reducer
- 生产路径中的多份章节状态字符串判断
- completion 中按“终章/尾声/新阶段/入口”等词语决定 complete 的 hard gate
- 一次为全部剩余 40～200 章创建单个长寿命 plan 的路径

### 集成测试

先进行故障注入：

- LLM 给低分但无 hard finding
- LLM 只给主观建议
- LLM 审稿 JSON 解析失败
- 审稿 finding 伪造 authority path
- 修订稿变差
- 修订后进程崩溃
- 合同在本章中途改变
- settlement 声明正文不存在的状态
- settlement 声明超出执行合同的状态
- metadata repair 发生在 settlement 之后
- 暂停、重启、恢复未批准章节
- add/revise/import 尝试直接写 `approved`
- 调用方伪造 passed review
- 普通写作尝试直接 `update_truth`
- protected authority 超预算或 role projection 被截断
- 修订稿通过大段删除剧情降低 blocker
- 旧项目第 N 章重建时意外读取 N+1 章以后事实
- 批准快照恢复时缺少 sealed authority 或 approval receipt
- 40 章与 200 章目标在 batch 边界暂停/重启
- provider 在第 N 章后不可用，恢复后从 N+1 章继续且不需要用户再次确认
- 正常终局没有“终章/尘埃落定”等关键词
- 正文出现“回顾过去的新阶段”但 typed ending obligations 已完成
- 仅写出“尾声”字样但主冲突和 must-resolve 仍未完成

再进行真实聊天测试：

- 每次全新 session
- 每次全新项目
- 每次不同大题材
- 2500 字档、10 万字合同
- 5000 字档、100 万字合同
- 暂停、重启、恢复、导出
- 持续写到完整合同结束

### 验收

- 主观分数不能单独触发修订。
- hard drift 不能因预算耗尽而自动通过。
- 每章只有一个 authority fingerprint。
- 每章只有一个语义修订入口。
- 每章只有一个 best 版本选择器。
- 恢复流程与首次写作流程行为一致。
- state settlement 失败不能污染后续章节。
- 所有低层 action 都不能绕过同一批准不变量。
- 快照恢复后的权威、best draft、approval receipt 和 truth 依赖指纹一致。
- 全书任务跨 batch、进程和模型服务恢复后仍自动推进，且已批准章节不重写。
- complete 由合同终局和 approved typed state 证明，不由 LLM 分数或结尾关键词决定。
- 页面/聊天结果不泄漏内部 JSON、内部路径和伪成功状态。

## Phase 6：可选独立审稿模型

### 前置条件

只有 Phase 1～5 全部通过，且真实测试仍表明同模型自审存在系统性漏检，才实施本阶段。

### 改动

将：

```rust
NovelChapterRunner {
    agent: Arc<dyn MultiAgent>,
}
```

演进为：

```rust
struct NovelWorkflowAgents {
    writer: Arc<dyn MultiAgent>,
    auditor: Arc<dyn MultiAgent>,
    state_observer: Arc<dyn MultiAgent>,
}
```

兼容规则：

- 未配置 auditor 时复用 writer。
- 未配置 state observer 时复用 auditor。
- Reviser 默认复用 writer，防止风格突变。
- 不要求立即修改面板 UI。
- 模型路由只放在 workflow 构造边界，不进入 `novel_studio` 存储模块。

独立审稿模型仍然不能：

- 仅凭 score 阻断
- 绕过 evidence validation
- 修改 sealed authority
- 直接提交 truth

独立模型返回的 semantic finding 仍只是候选证据。不同模型可能带来更高召回率，也会
带来更多意见分歧和 false positive；因此它不能改变本地 evidence grade、修订预算或
批准状态机。若独立 auditor 不稳定，应退化为 advisory，而不是扩大 hard blocker。

### 验收

- 同一正文分别由 writer-model 和 auditor-model 审查时，hard finding 格式一致。
- 更换 auditor 不改变本地 hard gate 结果。
- auditor 不可用时不重写正文；按明确策略重试一次或返回 audit unavailable blocker。
- 不因模型路由引入第二套章节状态机。

---

## 5. 对内容漂移的影响评估

### 5.1 总体判断

第三次代码核对后的结论是：

> 原计划方向正确，但不能原样实施。补齐存储边界、指纹拆分、role projection
> coverage、typed state delta、下游 reducer 和候选保真约束后，预计会明显降低小说
> 的章节内漂移与跨章节累积漂移；若不补这些遗漏，系统可能只是减少修订循环，却继续
> 产生状态污染，甚至把不完整权威永久封存。

它不是通过“降低质量门”来换取完成率，而是把阻断权收窄到可以证明的错误，同时让
真实的合同、连续性和状态漂移更难绕过。

### 5.2 对漂移有直接帮助的部分

| 改动 | 对漂移的帮助 |
| --- | --- |
| 规范化 sealed authority + as-of truth | 防止 Writer、Auditor、Reviser 使用不同合同或未来事实 |
| role projection coverage/trace | 防止同指纹但 prompt 被截断、遗漏人物或规则 |
| typed evidence hard gate | 保留可证明的身份、关系、世界、时间线和伏笔冲突阻断 |
| 1～2 次有限修订 + best rollback | 减少多轮重写逐步改坏人物和剧情的风险 |
| 候选 required outcome/受保护事实保真 | 防止通过删剧情“修好”冲突 |
| final-body typed settlement | 阻止 writer metadata 或 observer 幻觉污染后续章节 |
| settlement 失败保留旧 truth | 防止单章状态失败演变为全书累积漂移 |
| 唯一批准入口和 typed lifecycle | 防止低层动作把未审章节直接变成权威 |
| 合同/状态依赖失效链 | 防止旧草稿、旧审稿和新合同混用 |
| 合同候选 protected-field regression | 防止自动补全合同时改掉用户故事、书名和角色权威 |
| durable rolling batch | 长任务重启后从批准权威继续，不依赖旧内存上下文 |
| typed completion obligation | 防止为了满足表面“完结词”提前收尾或无限追加 |

### 5.3 可能的弊端与控制方式

#### 弊端 A：完整性门更严格，短期完成率可能下降

受保护权威缺失、observer 失败或 settlement 无法验证时，系统会明确停在
`needs_revision`/`state_repair_required`，而不是猜一个状态继续。这可能让早期测试
看起来“比以前更容易停”，但属于防污染的必要代价。

控制方式：错误必须明确指出缺少的 authority path、evidence 或 receipt；修复系统
原因，不放宽成自动通过。

#### 弊端 B：封存错误输入会冻结错误

sealed authority 只能防止阶段间变化，不能自动让输入变正确。如果封存的是冲突合同、
残缺上下文或错误人物名，四个角色会一致地犯错。

控制方式：封存前运行 canonical contract、人物登记、protected coverage、truth cutoff
和执行包完整性验证；失败不得封存。

#### 弊端 C：证据化 hard gate 可能漏掉真正但难以结构证明的漂移

复杂情绪关系、隐含动机和长期主题漂移不一定能由本地结构确定。将这些全部降为
advisory 会提高 false negative。

控制方式：把关键关系、状态、伏笔和章节 required outcome 逐步结构化；不要用自由
文本短语特例冒充结构验证。无法确定的 finding 保留在报告中供真实长篇测试评估，再
决定是否提升结构表达能力。

#### 弊端 D：错误的语义 validator 会制造假阻断

仅验证引用存在、关键词/bigram 命中并不够。过度简化的地点、关系或伏笔 validator
会把正常剧情推进判断为冲突。

控制方式：每种 hard semantic finding 都要有独立的关系验证和反例测试；validator
无法确定时必须返回 advisory/unknown，不能默认 hard。

#### 弊端 E：修订次数减少可能保留文风瑕疵

节奏、语言和感染力问题不会再自动触发十轮重写，单章的主观精致度可能不如激进修订。

控制方式：将 advisory 提供给 Writer 的首次生成和人工报告；完整小说自动化阶段优先
保证权威连续性。未来若需要润色，使用正文完成后的独立、可回滚润色流程，不和故事
状态审批混在一起。

#### 弊端 F：bounded incidental state 过宽或过窄都会有问题

过宽会允许模型借“小变化”改变关系和世界；过窄会让正常行动都要求写入合同。

控制方式：用 typed delta 明确允许的变化级别。身份、核心能力、世界硬规则、主线和
伏笔回收永远不属于 incidental；位置移动、短期情绪和无持久影响的局部动作可在证据
充分时允许。

#### 弊端 G：独立审稿模型可能增加分歧、成本和假阳性

独立模型不是 Phase 1～5 的修复替代品。它只能提高候选 finding 的多样性，不能提高
finding 的本地证据等级。

#### 弊端 H：rolling batch 会增加边界读取和事务开销

每 1～3 章重新读取磁盘状态，比一次内存跑完多一些 I/O。但对 40～200 章任务，它换来
的是可恢复性、权威刷新和资源释放。

控制方式：复用增量 Story Bible、轻量 status 和 approval receipt；批次边界不重新
全量审查已批准正文。

#### 弊端 I：合同可确认门如果裁得过薄，会把规划债务推到正文阶段

rolling enrichment 不能变成“什么都以后再说”。终局、主冲突、人物核心和世界硬规则
缺失时，第一章仍然容易漂移。

控制方式：只把审美配额和远期细节延后；用户权威、终局、主角、硬规则和当前执行窗口
始终属于开始前 blocker。

### 5.4 实施决策

建议实施，但必须先完成合同收敛前置 A，再按修订后的 Phase 1～5 顺序，不建议把原
计划直接编码：

1. 先让合同在最多 1～2 次 semantic patch 内进入可确认状态。
2. 再封存规范化且完整的权威，并堵住存储旁路。
3. 再迁移 typed evidence hard gate。
4. 再统一全部候选、正文协议、重试、恢复和状态机。
5. 再切换到 final-body typed settlement、原子批准及下游 reducer。
6. 最后迁移旧项目、建立 durable rolling batch/typed completion 并删除旧机制。

在 Phase 1/2 尚未完成时先减少修订次数，可能把未检出的漂移直接放行；在 Phase 4
下游 reducer 尚未改完时先删除旧状态机制，可能造成状态丢失。顺序本身是安全条件。

### 5.5 对“以前写不出小说”的改善判断

#### 只实施原六项的结果

会显著改善第一章生成后的反复修订和审批卡死，但**不能保证改善合同前卡死，也不能
保证 40～200 章无人干预完成**。最多只能判断为“局部明显改善”。

#### 实施本次补齐后的完整计划

预计会直接改善以下失败点：

| 以前的卡点 | 重构后的变化 |
| --- | --- |
| 合同最多 30 轮自修自审 | typed owner + 1～2 次 semantic patch + best pending |
| 同模型 `Uncertain` 重开合同 | 无证据 uncertainty 降为 advisory，用户可确认 |
| 合同 rolling 字段过多导致第一章无法开始 | 复用 readiness scope，只锁开始所需权威 |
| 长正文 JSON 截断/metadata 缺失 | stream-safe body；metadata 由最终正文结算 |
| 主观 score/自由文本 issue 触发十轮重写 | 仅 typed hard blocker 触发最多 1～2 次修订 |
| step retry 换成 minimal context 后重新写坏 | retry 使用同一 sealed authority 和 best candidate |
| settlement/metadata 指纹相互失效 | 指纹拆分和一次性 approval receipt |
| 单章批准中途崩溃留下半状态 | 幂等 approval journal/transaction |
| 40/200 章依赖一次长期请求 | durable rolling batch 自动恢复 |
| 已写完却因没有“终章”关键词不完成 | typed ending/must-resolve/hook completion |

因此完成全部前置 A + Phase 1～5 后，预期从“经常连第一章或前两章都无法稳定批准”
提升为“合同和单章失败都能在有限次数内得到明确结果，已批准进度可持续推进”。这会
大幅提高写出完整小说的概率。

但不能在没有真实长篇验证前承诺“一定能写完”：

- 模型仍可能连续两次无法生成满足 hard contract 的正文。
- provider、硬件或进程长期不可用不是本地治理能消除的问题。
- semantic validator 自身若有误仍会正确地停住而不是污染后续。

最终成功标准应是：新 session、新项目、新题材下，合同有限收敛；每章最多两次语义
修订；中断可自动恢复；至少完整跑通一本 10 万字/2500 档小说，再验证一本
100 万字/5000 档合同的 200 章调度、快照和恢复，不要求一次测试实际生成 100 万字后
才证明合同容量。

---

## 6. 测试矩阵

### 6.0 合同有限收敛

| 场景 | 预期 |
| --- | --- |
| semantic reviewer 返回 Uncertain | 记录 advisory，不单独重开合同 |
| provider 未返回 semantic JSON | runtime unavailable，不伪造合同冲突 |
| issue 文本换一种措辞但 code/owner 相同 | 仍路由到同一 typed patch |
| 第二次 semantic patch 仍无净提升 | 停止并保留 best pending，不进入第 3～30 次 |
| candidate 补齐字段但改掉用户核心故事 | 拒绝该 candidate |
| rolling aesthetic ledger 未补齐 | 不阻止确认；进入相关阶段前再补 |
| 主角、终局、世界硬规则缺失 | 仍然阻止确认 |
| approve_draft 中途失败 | 不留下正式半初始化项目 |

### 6.1 权威一致性

| 场景 | 预期 |
| --- | --- |
| 新人物在执行包中申请并登记 | 四个阶段均看到同一权威姓名 |
| 审稿时 manifest 被外部修改 | 本章仍使用 sealed authority |
| 合同中途被用户明确修改 | 当前草稿标记 `authority_stale`，不能静默审稿 |
| 暂停后恢复 | authority fingerprint 不变 |
| Auditor projection 超过预算 | 受保护路径不截断；无法满足时停止而不是降级批准 |
| 合同同时存在 structured authority 和兼容镜像 | prompt 只包含 canonical contract |
| 重建第 N 章 | 不包含 N+1 章及之后产生的 truth |
| future boundary | 能阻止提前消费，但不把未来正文当作正向生成材料 |

### 6.2 质量门

| 场景 | 预期 |
| --- | --- |
| score=0，无 hard finding | 记录 advisory，允许进入 settlement |
| “节奏略慢” | warning，不修正文 |
| 人物身份与合同冲突且证据有效 | hard block |
| finding 引用不存在的 authority path | finding 无效，不能 hard block |
| authority/body 引用都存在但不构成矛盾 | advisory/unknown，不 hard block |
| 正文有 JSON/工具回执 | 本地 hard block |
| 直接调用 review 并传入 passed | 没有本地 typed receipt 时不能批准 |

### 6.3 修订

| 场景 | 预期 |
| --- | --- |
| 第一次修订消除全部 hard blocker | 接受并停止 |
| 第一次修订没有净提升 | 回滚并停止 |
| 第一次减少 blocker，仍剩另一 blocker | 允许第二次 |
| 第二次仍失败 | 回滚 best，保持 needs_revision |
| 恢复旧草稿 | 使用同一修订入口 |
| 修订删除 required outcome 或大段剧情 | 即使 blocker 减少也不能成为 best |
| 外层 step retry 重新生成正文 | 作为同一 revision session 候选计入预算 |
| final top-up 改变正文 | 旧 audit/settlement 失效并重新验证 |
| 只有 metadata 改变 | 不重跑正文 semantic audit，只重跑 metadata gate |

### 6.4 状态结算

| 场景 | 预期 |
| --- | --- |
| writer metadata 编造状态 | 不进入 truth |
| 最终正文明确改变关系且合同允许 | 更新关系状态 |
| 最终正文写出合同不允许的身份跃迁 | state repair required |
| 伏笔无正文证据却标记 pay_off | validation failed |
| settlement 失败 | 旧 truth、Story Bible 不变 |
| summary 声称新增世界规则但 typed delta 不存在 | 不更新 world database |
| observer 只找到常见 bigram | 不能授权 durable state change |
| settlement 元数据投影完成 | 最终 approval receipt 的全部指纹仍匹配 |
| 直接调用 update_truth | 普通自动写作被拒绝；显式管理覆盖记录 provenance 并失效下游 |

### 6.5 存储边界、生命周期与恢复

| 场景 | 预期 |
| --- | --- |
| add/revise/import 传入 `status=approved` | 拒绝或降为非批准 canonical 状态 |
| imported chapter | 进入 `imported_unverified`，不能直接成为进度权威 |
| 非法状态转移 | 由唯一 lifecycle 拒绝 |
| 进程在 candidate 持久化后崩溃 | 恢复完整 best candidate，不按最长 body 猜测 |
| 轻量批准快照恢复 | sealed authority、approval receipt、truth 全部存在且匹配 |
| contract/truth revision 改变 | 所有未批准后代产物标记 stale |

### 6.6 正文协议、全书推进与完成

| 场景 | 预期 |
| --- | --- |
| 5000 字正文包含大量引号和换行 | 不依赖 JSON string 转义，正文完整保存 |
| provider 在正文尾部截断 | 保存 truncated candidate，只执行一次有界补尾 |
| Writer 未提供 summary/key facts | 不重写正文，由 final observer 结算 |
| 10 万字/2500 档 | 约 40 章通过 rolling batches 自动推进 |
| 100 万字/5000 档 | 正确计算约 200 章并验证 batch/checkpoint/恢复容量 |
| batch 结束后进程重启 | 从磁盘最后 committed approval receipt 自动继续 |
| 最后一章无“终章/尾声”词但终局义务完成 | complete |
| 最后一章写“尾声”但 must-resolve 未完成 | 不 complete |
| typed completion 连续无净进展 | 返回具体 blocker，不无限追加终局章 |

---

## 7. 提交顺序

建议每个 Phase 单独提交：

0. `refactor(writing): bound creation contract convergence`
1. `refactor(writing): seal chapter authority packages`
2. `refactor(writing): type evidence-backed chapter blockers`
3. `refactor(writing): unify bounded chapter revision and prose protocol`
4. `refactor(writing): atomically settle truth from final prose authority`
5. `refactor(writing): add durable full-book continuation and migrate legacy chapters`
6. `feat(writing): support optional auditor model routing`

每个 Phase 完成后必须：

```text
cargo fmt --all -- --check
cargo check -p benshu-builtin-tools
对应模块聚焦测试
git diff --check
rg 检查旧机制是否仍有生产调用
```

同时必须把“实施总准则”的机制映射表随提交或审查记录保存；如果 `Deleted` 为空，必须
明确证明本项是纯新增缺口而不是覆盖已有机制。

Phase 1～5 未全部通过前，不进行完整小说压力测试；只做短章节和故障注入，避免在错误架构上继续消耗模型调用。

前置 A 未通过前不做第一章真实模型压力测试；否则合同 30 轮修复会继续消耗模型调用，
并掩盖章节重构本身的结果。

---

## 8. 最终完成定义

本重构只有同时满足以下条件才算完成：

1. 合同 semantic patch 默认最多 1 次、绝对最多 2 次；不再存在 30 轮自动修复。
2. semantic `Uncertain` 不能单独阻止结构完整合同的用户确认。
3. 合同候选不得回退用户故事权威、已正确字段、人物名和目标档位。
4. 主观 score 永远不能单独触发正文修订。
5. 有证据的合同、连续性和状态漂移仍然硬阻断。
6. hard blocker 未解决时，预算耗尽也不能自动批准。
7. Writer、Auditor、Reviser、Observer 使用同一个 sealed authority fingerprint。
8. 所有 role projection 均有 coverage、exclusion trace 和自己的 fingerprint，受保护
   权威不能被静默截断。
9. sealed contract 是 canonical authority projection，不重复携带兼容镜像。
10. 新章节、恢复章节、外层 retry 和整章 regenerate 使用同一个修订控制器。
11. 每章只保留一个携带完整 DraftOutput、typed findings 和依赖指纹的 best 版本权威。
12. best 版本不能通过删除 required outcome、关键事实或大段剧情取得“净提升”。
13. Writer 长正文不再依赖完整 JSON envelope，也不因 metadata 缺失重写正文。
14. 人物、世界观、关系和伏笔只从最终正文 typed delta 结算。
15. 最终正文中的重大状态变化必须同时获得章节执行合同许可。
16. settlement 失败时旧 truth 不改变，下一章不能开始。
17. final approval receipt 在所有 settlement/metadata 投影完成后生成且不会自我失效。
18. 单章批准事务崩溃/重放不会留下半提交状态或重复应用 delta。
19. add/revise/import/review/update_truth 等低层动作不能绕过 canonical approval。
20. 所有已被替换的字符串分类、重复修订循环、重复 evidence/fingerprint、元数据状态
    reducer 和反向覆盖代码已经删除。
21. 2500/5000 字档位、任意总字数合同不受本次重构破坏。
22. 暂停、崩溃和重启后恢复相同 authority、best draft、approval receipt 和 truth 状态。
23. 旧项目 as-of 重建不会把未来章节事实泄漏到早期章节权威中。
24. 10 万字/2500 档项目可跨 rolling batch 自动推进，不依赖用户重复发送“继续”。
25. 100 万字/5000 档合同能正确规划约 200 章并通过 checkpoint/recovery 容量测试。
26. complete 只由目标规模、终局义务、typed state 和 hook/payoff 生命周期证明，不由
    LLM 分数或“终章/尾声”等关键词决定。

最终衡量标准不是“系统不再阻断”，而是：

> 只阻断真正会破坏小说权威和后续连续性的错误；对主观审美波动保持克制；对真实内容漂移保持严格。
