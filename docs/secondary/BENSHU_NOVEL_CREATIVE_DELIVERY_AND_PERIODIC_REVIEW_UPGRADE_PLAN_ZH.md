# BenShu 小说创作交付层与五章阶段审查升级计划

> 状态：Phase 0～5 已按决策实施并通过写作主链回归；Phase 6 按计划暂缓；真实聊天简测已完成合同与 2 个连续批准章节，第 3 章按用户要求在生成中暂停；简测发现第 2 章提前消费第 3 章边界事件，内容连续性验收未通过，跨 5/10 章完整验收尚未执行
>
> 核对日期：2026-08-02
>
> 核对基线：当前工作区实际调用代码；实施前后均按唯一 owner、调用链、持久化边界和重复机制清单核对
>
> 参考来源：[Tomato Novelist：番茄高数据小说创作助手](https://skillhub.cn/skills/user_634bbcdc/g113593)
>
> 适用范围：`crates/builtin-tools/src/tool/writing`
>
> 核心目标：只吸收参考 Skill 中 BenShu 确实缺少的创作交付能力；复用现有合同、权威包、审稿、状态结算和伏笔机制，不安装 Skill，不复制其文件工作流，不新增平行权威或主观硬门。

## 0. 最终决策摘要

本计划确认以下产品流程不变：

```text
用户自然语言提出题材、总字数和 2500/5000 章节档位
-> 模型在内部补齐创作合同
-> 面板展示一份完整、可读的合同摘要
-> 用户可以用自然语言修改任意相关合同字段
-> 系统通过现有 typed patch 更新合同并重新校验
-> 用户一次确认合同
-> 系统按确认后的合同自动写作
```

参考 Skill 的“六问”只是外部方案的问卷设计，不是 BenShu 的内部合同维度，也不在本计划中取得特殊地位。BenShu 只要求用户明确提供小说题材、小说总字数和 2500/5000 章节档位；其余全部内容由现有完整多维合同生成链自动创作，用户看完合同后再通过自然语言修改。

代码核对后，BenShu 真正需要处理的是五项，其中第 2 项主要是强化现有生成要求，其余四项是明确的接线或能力缺口：

1. 在写作合同入口增加小说专属的三项输入前置检查：题材、用户指定总字数和用户明确选择的 2500/5000 档位。该检查归属写作工具，不改变通用创建 intake。
2. 保留现有完整多维合同和字段强度/readiness 规则，只强化模型对主角反差、读者承诺等已有字段的协同生成；不把合同裁剪成参考 Skill 的六维合同，也不把所有可选字段升级为确认硬门。
3. 把已有角色声音、情绪、关系、力量/年龄状态以及当前物件、对手、兑现/揭示项精准投影到当前章节，再落实场景配比、读者承诺和章尾轮换；不能只把完整结构化合同交给模型后依赖其自行寻找或通用前 N 项截断。
4. 为第一章和普通章节补充不同的开篇与正文交付原则；第一章尽快兑现读者承诺，普通章节优先承接已批准状态和上一章钩子。该原则是创作指导，不是硬阻断。
5. 把现有每五章的单章审稿触发语义迁移为批准后的五章窗口审查；结果只能作为下一阶段的非阻断创作建议，并使用与单章批准凭证隔离的最小记录。

以下内容明确不引入：

- 强制六问表单。
- 每章完成后询问用户是否继续。
- 固定 10/30/50/100/200 章。
- 固定 2200～2800 字。
- 单独的 `00-大纲.md`、`01-人物档案.md`、`02-情绪曲线.md`、`03-黄金开篇.md` 平行权威文件。
- 三个开篇版本等待用户选择的必经流程。
- 主观质量分大于 60 或任意分数阈值才能批准。
- 每章必须用悬念结尾。
- 每三章强制执行同一种“压—扬—压—爆”公式。
- 强制插入“评论区金句”。
- 根据模型猜测的热度自动给配角加戏、改主线或改合同。
- 参考 Skill 的字数脚本和固定平台模板。

### 0.1 不降级保证

本计划是增量优化，不是把 BenShu 改造成参考 Skill：

- 不删除、合并或弱化 `NovelCreationContract` / `NovelContractV2` 的现有合同维度。
- 不要求每个题材都填满所有结构化数组；继续服从现有 `field_requirements`、`ContractReadinessScope` 和 `PatchFieldStrength`，允许不适用或滚动生成的字段保持可选。
- 不降低人物身份冲突、章节编号、未来事件越界、合同不可满足、最终正文状态污染等确定性硬门。
- 不缩减持久化的完整合同、Story Bible、truth、settlement 或 sealed authority；相关性过滤只作用于当前章节的工作投影。
- 不把自动写作改成逐章询问，不新增六问流程，也不降低总字数可任意指定、章节档位仅 2500/5000 的现有能力。
- 不把平台化写法、主角反差、开篇吸引力、对白风格或模型评分变成合同/章节硬门。
- 五章审查最多替换现有“第 5 倍数章额外审当前单章”的模型调用，不与旧周期调用并行，避免增加重复审稿和资源负担。

## 1. 实施总准则

### 1.1 每一步实施前必须先核对旧机制

每个 Phase 开始前必须完成以下检查，并把结果写入对应提交说明或审查记录：

1. 使用 `rg` 查找相同字段、函数、调度条件和持久化记录。
2. 确认当前调用链实际使用的 owner，不能只看文件名或测试 helper。
3. 确认新需求属于：
   - 现有能力已完整存在：保持不动。
   - 现有能力存在但未接线：接通现有 owner。
   - 现有能力职责错误：原位替换，并删除旧实现。
   - 现有能力确实不存在：才允许在最接近的 owner 中补齐。
4. 检查同一语义是否已经存在于合同、Story Bible、manifest、review 或 workflow 中。
5. 如果必须扩展结构，只扩展现有结构；不得创建第二套合同、人物账本、伏笔账本、节奏账本或审稿状态机。
6. 新实现接线并通过回归后，删除被替换的旧函数、旧 prompt 分支、无生产调用的 helper 和过时测试夹具。

### 1.2 数据权威不变

本计划不改变现有权威顺序：

```text
用户明确输入
> 用户确认后的结构化合同
> 当前章节封存权威包
> 已批准最终正文的 typed settlement
> 派生摘要、审稿建议和展示信息
```

创作建议、开篇建议、五章节奏报告都不属于故事事实权威，不能覆盖人物、世界规则、终局、已批准状态或伏笔生命周期。

### 1.3 主观质量永远不能成为新的硬门

以下内容只能成为 `advisories`：

- 开篇不够抓人。
- 情绪波动偏平。
- 对话不够有个性。
- 句式或段落节奏单一。
- 修饰语偏多。
- 场景类型偏科。
- 章尾形式重复。
- 模型主观评分偏低。

它们不得：

- 把章节改成 `needs_revision`。
- 消耗正文语义修订预算。
- 触发多轮自动重写。
- 阻止批准、状态结算或进入下一章。

合同、连续性、状态污染和正文完整性等确定错误仍按现有 typed hard gate 处理，本计划不削弱它们。

### 1.4 用户确认边界

用户确认发生在完整合同层，而不是六个问题或每章层：

- 合同未确认时：用户可以自然语言修改书名、题材、情绪、主角、人物关系、主线、世界规则、结局、总字数和章节档位等字段。
- 每次修改必须经过现有 typed patch、名字同步、合同 normalization 和 typed gate，再展示更新后的合同摘要。
- 未被用户修改的稳定字段必须保留，不能因为局部修改重新生成整份合同。
- 用户确认后才进入正文写作。
- 已经存在批准章节后，如用户要求改变故事前提、主角身份、核心冲突、终局或世界硬规则，应按现有合同重新开启与下游失效规则处理，不能把重大换故事请求当作普通创作建议静默注入后续章节。
- 用户修改合同不需要填写 JSON，不展示内部路径或 typed patch。

## 2. 当前代码机制核对

### 2.1 自然语言入口与一次确认

现有 owner：

- `creation_contract/chat_flow.rs`
- `creation_contract/intent.rs`
- `creation_contract/intent/field_extract.rs`
- `creation_contract/draft_lifecycle.rs`
- `creation_contract/surface/user_view.rs`

当前已有：

- 判断用户是在新建、修改、确认、继续还是读取状态。
- 合同未完成时展示缺口，合同可确认时展示用户可读摘要。
- 用户可以通过自然语言修改草案。
- 短确认继承已经保存的全书执行范围。
- 确认前不会直接把内部 JSON/patch 展示给用户。

决策：保持现有自然语言入口和一次确认，不新增六问 UI、不新增选项状态机。

当前确实存在的严格性缺口：

- `build_initial_creation_draft` 已能解析题材、总字数和章节档位，但三个值在初始 draft 中仍可能为空。
- `target_units` 和 `chapter_unit_target` 仍是可选值；当前部分路径会在用户没有选择章节档位时，根据总字数动态推导默认档位。
- 题材可以从用户故事描述中解析，但当前没有统一入口保证“缺少可用题材时先追问，不能交给合同模型自行决定”。
- 通用 `runtime-policy-core/src/intake.rs::evaluate_creation_intake` 当前允许小说请求使用“你来定”，其职责是识别创建类任务并给出通用缺口提示，不具备 BenShu 小说合同三项用户权威的完整语义。
- 因而“三项必须由用户提供”目前是测试约定和常用请求格式，尚未成为统一的创建前置条件。

Phase 1 必须在 `creation_contract/chat_flow.rs` 的小说新建/草案入口补齐写作专属前置条件：三项中缺哪一项，就用一次自然语言提示只询问缺失项；三项齐全后才进入完整合同生成。不得用模型默认值替代用户对题材、总字数或章节档位的选择，也不得把小说专属规则下沉到通用 runtime policy。

### 2.2 合同结构

现有 owner：

- `creation_contract_model/core.rs::NovelCreationContract`
- `creation_contract_model/core.rs::CharacterContract`
- `novel_contract_v2/core.rs::NovelContractV2`

当前合同已经包含：

- 书名、语言、题材、简介、目标总字数和章节档位。
- 故事前提、终局、主角弧线、世界意象和主因果线。
- 角色姓名、定位、欲望、恐惧、底线、弧线、计划登场和离场。
- 情绪合同和情绪状态账本。
- 人物关系账本。
- 资源、力量、制度、地理、时间、物件和对手压力。
- 伏笔兑现矩阵。
- 叙事合同、场景配比、人物声音、读者承诺、章尾轮换和冲突压力曲线。
- 母题、揭示节奏和关系互动配额。

参考 Skill 问卷涉及的情绪、题材、主角、核心冲突和章节规模都只是 BenShu 完整合同的一小部分，并且已经有表达位置。唯一没有独立字段的是“主角核心反差”，但它可以并且应该由现有字段共同表达，不能为此新建一份平行人物设定。

完整多维不等于每个数组都必须非空。当前系统已经通过 `field_requirements`、`ContractReadinessScope` 和 `PatchFieldStrength` 区分确认必需、强字段、可选字段及滚动补充字段。阶段性大纲、尚未适用的关系配额或题材不需要的治理维度不能因为本计划被升级为 blocker。

决策：保留结构和既有字段强度，不新增 `emotion_tag`、`protagonist_contrast`、`core_conflict` 或 `tomato_profile` 等重复字段，也不扩大确认 readiness 的硬门集合。

### 2.3 合同生成、修复与自然语言修改

现有 owner：

- `creation_contract/patch_prompt.rs`
- `creation_contract/patch.rs`
- `creation_contract/patch_normalizer.rs`
- `creation_contract/repair_coordinator.rs`
- `typed_contract_gate.rs` 及其子模块

当前已有：

- typed patch。
- 用户明确字段保护。
- 人物姓名本地治理和跨字段同步。
- 合同局部修复、候选比较和最佳候选保留。
- readiness scope 和字段强度。
- 用户修改后重新进入统一 typed gate。

决策：只在现有合同生成 prompt 中把“主角反差”作为内部创作检查维度，要求其落实到已有字段；不新建 patch 类型，不建立单独反差 gate，不把反差缺失设成合同 blocker。

### 2.4 总字数、章节档位和预计章数

现有 owner：

- `longform_policy.rs`
- `novel_studio/context_packaging.rs::narrative_progress_contract`
- `novel_studio/chapter_state.rs`

当前规则：

```text
expected_chapters = ceil(target_units / chapter_unit_target)
```

- 用户总字数允许任意正整数。
- 新建合同的章节档位只有 2500 和 5000。
- 10 万字/2500 档通常得到约 40 章，但 40 不是系统常量。
- 100 万字/5000 档通常得到约 200 章。
- 完成进度以磁盘连续 approved 正文为权威。

当前严格性缺口：`normalize_chapter_unit_target` 在用户没有明确选择档位、但已有总字数时可以动态推导档位。该行为与本计划确定的“三项必须由用户提供”不一致。

决策：保持总字数、预计章数公式、磁盘进度权威和 2500/5000 两档；在新小说创建入口停止用动态默认值代替用户选择。动态推导只允许服务历史项目兼容或明确的迁移/恢复路径，不能满足新建合同的用户输入前置条件。不采用参考 Skill 的固定章节选项和 2200～2800 字脚本。

### 2.5 章节上下文与当前人物子图

现有 owner：

- `novel_studio/context_packaging.rs::build_context_payload`
- `novel_studio/context_packaging.rs::relevant_character_subgraph`
- `novel_studio/context_packaging.rs::relevant_contract_view`
- `novel_studio/context_packaging.rs::relevant_story_bible_view`
- `novel_studio/context_packaging.rs::build_prompt_context_payload`

当前已有：

- 受保护上下文和可压缩上下文分层。
- 当前章节合同、下一章边界和滚动大纲窗口。
- 最近三章已批准最终正文结算。
- 长篇 archive、truth files 和来源材料。
- 当前相关角色子图。
- prompt context 指纹和预算 telemetry。
- `SealedChapterAuthority` 已把规范合同、truth、工作上下文、章节计划/合同/架构和登记请求绑定为同一只读章节权威，并为 Writer、Auditor、Reviser、Observer 生成可校验角色投影。

当前确实存在的缺口：

- `relevant_contract_view` 会过滤角色和关系账本，但没有同步按当前角色子图过滤 `character_voice_ledger`、`emotional_state_ledger` 和 `relationship_interaction_quotas`。
- 同一缺口还影响 `power_progression.character_current_levels` 和 `time_model.age_progression` 等人物索引状态；只修前三张表仍会让后排当前人物的力量或年龄状态被通用截断。
- `relevant_story_bible_view` 当前只过滤人物、关系及部分 hook/payoff 窗口，没有与合同工作视图共享上述人物规则。
- `artifact_ledger`、`antagonist_pressure.antagonists`、`payoff_matrix`、`motif_ledger` 和 `reveal_schedule` 等实体/时序数组也会受统一前 N 项截断影响；长篇后段若当前物件、对手或到期兑现项位于后排，仍可能丢失。
- 后续通用数组压缩会对结构化数组统一截断；角色多时，当前实际出场人物的创作规则可能被无关人物挤出 prompt。
- `scene_type_mix`、`reader_promise`、`chapter_ending_rotation` 虽然存在于合同中，但执行 prompt 只笼统要求继承节奏和伏笔，没有明确把它们转换为本章交付任务。

决策：只升级封存前的工作上下文相关性投影，并让合同视图与 Story Bible 视图复用一个相关性选择结果；不重建权威包，也不暗示当前 Writer/Auditor/Reviser 已经使用不同故事权威。不新建 `dialogue_profile`、`emotion_file` 或第二套章节上下文。

### 2.6 章节执行包与写作 prompt

现有 owner：

- `novel_runner/core/model.rs::ChapterExecutionPackage`
- `novel_runner/core/model.rs::ChapterMemo`
- `novel_runner/core/prompts.rs::chapter_execution_prompt`
- `novel_runner/core/prompts.rs::writer_prompt`
- `novel_runner/core/prompts.rs::reviser_prompt`

当前已有：

- 章节目标、冲突、选择、代价、揭示和情绪节拍。
- 章末不可逆事件和可验证新状态。
- 世界、人物、关系、力量、资源和伏笔变化上限。
- 恰好五个场景节点，每个场景描述功能、行动、继承事实和结果状态。
- 标题依据、滚动未来章节和新人物请求。
- Writer、Reviser 使用相同只读章节权威。
- 2500/5000 档初稿长度预留。

当前确实存在的缺口：

- 没有第一章专用的开篇交付原则。
- 没有明确要求第一个场景节点承接 `reader_promise`。
- 没有明确区分“第一章建立读者问题”和“普通章节承接上一批准状态”。
- 人物声音虽在合同上下文中，但 writer prompt 没有针对当前出场人物明确强调对话目的、潜台词和声音区别。

决策：扩展现有执行包生成指令和 writer 指令，不增加新的开篇模型调用、开篇文件或章节状态。

### 2.7 单章审稿与主观质量

现有 owner：

- `novel_workflow_driver/quality.rs`
- `novel_workflow_driver/audit.rs`
- `chapter_quality.rs`
- `novel_studio/quality_gate.rs`
- `novel_studio/review_approval.rs`
- `novel_studio/model.rs::ReviewReceipt`

当前已有：

- 第 1、2 章和每 5 章触发 LLM 质量审稿。
- 确定性 finding 与 LLM advisory 分离。
- LLM 权威冲突只有通过本地证据验证才可能成为 finding。
- 标题、措辞、节奏、审美和 score 只进入 advisory/telemetry。
- score 不决定正文重写。
- 自由文本 issue 不能伪造 hard blocker。

当前确实存在的缺口：

- 每 5 章触发时仍主要审查当前这一章，没有形成最近五章整体表现报告。
- 审稿 prompt 没有明确观察第一章前两三段是否兑现读者承诺。
- 对话目的、角色声音同质化、抽象陈述替代场景和句段节奏只能被“通顺/节奏”笼统覆盖。

决策：单章审稿继续扩展现有 `ReviewReceipt.advisories`，不新增主观质量 gate；跨五章报告必须与单章批准凭证隔离，具体见 Phase 5。

### 2.8 最终正文结算、状态和伏笔

现有 owner：

- `novel_studio/state_truth.rs`
- `novel_studio/settlement.rs`
- `novel_studio/runtime_records.rs`
- `novel_bible`
- `novel_studio/review_approval.rs`

当前已有：

- 状态从最终正文结算，不信任生成阶段声明。
- typed state changes 绑定实体、权威路径和正文证据。
- 结算失败时不允许污染后续 truth。
- 已批准 settlement、approval receipt 和事务 journal。
- 伏笔 seed、advance、pay off、defer 和 overdue 生命周期。
- 章节摘要、当前状态和 pending hooks 来自最终正文观察。

决策：完全保持。五章阶段审查只能读取这些结果，不能建立第二套人物状态、情绪状态或伏笔状态。

### 2.9 逐项必要性复核结论

| 计划项 | 当前代码事实 | 最终结论 | 为什么不是降级/重复 |
| --- | --- | --- | --- |
| 小说三项用户输入 | 解析器和用户权威标志已有，但新建前置条件不统一，通用 intake 仍允许“你来定” | 必须补写作专属 preflight | 复用现有提取器，不修改通用 intake，不改变旧项目兼容 |
| 完整多维合同 | 合同模型、typed patch、readiness 和多维字段均已存在 | 保持主体，只细化生成协同和回归 | 不减字段、不建六维合同、不扩大硬门 |
| 主角反差 | 无独立字段，但主角弧线、人物状态、冲突与读者承诺可完整表达 | 只补 prompt 创作检查 | 不建重复字段，不强制套路 |
| 当前章节相关投影 | 人物/关系已有部分过滤，但多个人物与实体数组仍依赖前 N 项截断，合同与 Story Bible 选择不完整 | 必须原位扩展同一投影 | 完整权威不变，只让本章稳定看到正确子集 |
| 第一章/普通章开篇 | 五场景执行包已有，缺少 reader promise 驱动的首场景规则 | 必须补 writer guidance | 不加模型调用、文件、状态或硬门 |
| 对白/展示/节奏 | 单章 LLM advisory 已有，但 prompt 只做笼统通顺/节奏检查 | 原位细化 | 继续写入 advisory，不进入修订 blocker |
| 五章整体表现 | `% 5` 调度已有，但仅审当前单章；没有批准后窗口、隔离记录和恢复幂等 | 必须迁移调度并补最小非权威记录 | 删除旧周期单章分支；不混用 approval review，不建第二状态机 |
| 外部读者数据 | 当前无真实数据源 | Phase 6 暂缓 | 不为虚假数据或非前置功能增加复杂度 |

因此，真正新增的持久化类型只有五章 delivery advisory window；其余升级均为现有 owner 的前置检查、相关性接线或 prompt 原位细化。任何实施 diff 若出现第二套合同、人物/伏笔账本、章节审稿 verdict 或大量平台题材特例，都与本复核结论冲突，应停止并回到 Phase 0。

## 3. 目标工作流

### 3.1 合同创建与修改

```text
用户：写一本 10 万字修仙小说，每章 2500 字
-> 自然语言 intake 锁定题材、总字数和章节档位
-> 合同生成器自动创建完整多维合同
-> typed normalize / gate
-> 展示完整合同摘要
-> 用户：主角改成女主，性格外冷内热，结局不要飞升
-> 现有 modification intent + typed patch
-> 只修改受影响字段并同步相关故事字段
-> typed normalize / gate
-> 展示更新合同
-> 用户：按这个开始
-> 锁定合同并进入写作
```

三项用户必填输入的现有字段映射：

| 用户输入 | 现有权威字段 | 处理方式 |
| --- | --- | --- |
| 小说题材 | `genre`，以及用户故事核心权威 | 必须来自用户自然语言；允许自由题材描述，不限固定选项 |
| 小说总字数 | `target_units`、`target_units_user_specified` | 必须由用户提供任意正整数，不能由模型决定 |
| 章节字数档位 | `chapter_unit_target`、`chapter_unit_target_user_specified`、`chapter_unit_target_user_authority` | 必须由用户选择 2500 或 5000；新建合同不能用动态默认替代 |

三项齐全后，模型自动生成的完整合同至少覆盖以下现有分组：

| 合同分组 | 现有内容 |
| --- | --- |
| 故事骨架 | 书名、简介、前提、主因果线、主题、终局、主角弧线 |
| 人物权威 | 姓名、定位、欲望、恐惧、底线、弧线、登场/离场 |
| 情绪与读者体验 | 情绪合同、情绪状态、读者承诺、冲突压力曲线 |
| 关系治理 | 关系账本、关系转折、互动配额 |
| 世界治理 | 世界规则、资源经济、力量成长、社会秩序、地理、时间、物件 |
| 叙事交付 | 叙事合同、场景配比、人物声音、章尾轮换、母题和揭示计划 |
| 长篇结构 | 分卷、近期章节窗口、滚动大纲、伏笔兑现矩阵 |

“覆盖”表示合同生成链继续认识并按适用范围生成这些现有分组，不表示所有字段、数组在首次确认时都必须非空。是否构成确认缺口继续由现有字段要求、readiness scope 和字段强度决定。参考 Skill 的六个问卷问题不会限制、裁剪或重新定义上述合同结构。

### 3.2 逐章写作

```text
读取确认合同和已批准状态
-> 选择当前章节相关人物、实体和时序子图
-> 精准投影人物声音/情绪/关系/力量状态与当前物件、兑现和揭示规则
-> 生成当前章节执行包和 5 场景架构
-> 第 1 章：第一场景落实读者承诺和开篇问题
-> 普通章节：第一场景承接已批准状态或待推进钩子
-> Writer 生成完整正文
-> 确定性质量门 + 非阻断创作审稿
-> 有确定错误时进入现有有限修订
-> 从最终正文结算 typed state
-> 批准并提交 truth
-> 自动进入下一章
```

### 3.3 每五章阶段审查

```text
磁盘连续 approved 端点到达 5、10、15……章
-> 读取最近五章批准正文的受控窗口
-> 读取五章 approved settlement、review 和 hook debt
-> 模型生成跨章创作表现 advisories
-> 本地确认报告只包含非权威创作建议
-> 写入独立于单章批准凭证的 delivery advisory window 记录
-> 下一阶段执行包读取精简后的有效建议
-> 不修改合同、不重写旧章、不等待用户，继续下一章
```

## 4. 分阶段升级方案

## Phase 0：冻结基线与重复机制审查

目标：在修改任何代码前，确认现有 owner 和调用链，避免再次重复造轮子。

实施：

1. 记录当前提交、工作树状态和测试基线。
2. 搜索三项用户必填输入、合同自动生成字段和对应写入点。
3. 搜索当前章节人物、实体、时序相关性投影，以及合同/Story Bible 的 prompt 压缩路径。
4. 搜索所有开篇、钩子、对话、节奏和周期审稿逻辑。
5. 搜索所有 review/advisory 持久化路径。
6. 形成“保留、原位升级、替换删除、不引入”清单后才能开始 Phase 1。

禁止：

- 新建 `tomato_*` 生产模块。
- 新建第二套合同文件模板。
- 在 `chat.rs`、gateway 或 runtime policy 中加入小说创作策略。

说明：`runtime-policy-core` 继续负责通用创建任务识别；它当前允许“你来定”不代表小说合同三项权威已经满足。小说专属前置检查必须在写作工具内覆盖这一语义，而不是修改所有创建任务的通用行为。

验收：

- 每项改动都有唯一 owner。
- 没有先写新实现再寻找旧实现的情况。

## Phase 1：锁定三项用户必填输入并强化完整合同自动生成与自然语言修改

目标：新小说创建前只向用户收集题材、总字数和章节档位；三项齐全后由模型生成完整多维合同，并允许用户查看合同后自然修改。

保留：

- `creation_contract/chat_flow.rs` 的 intake、modify、approve 状态流。
- `runtime-policy-core` 的通用创建任务识别，不在其中增加小说字段规则。
- 现有 typed patch、用户字段保护、名字治理和 typed gate。
- 一份可见合同摘要和一次用户确认。
- 现有成人内容年龄确认与其他安全前置流程；三项检查不得绕过或替代它们。

升级：

1. 在 `creation_contract/chat_flow.rs` 的新小说/草案路径、合同模型调用前，复用现有字段提取器验证三个用户输入：可用题材、用户指定的正整数总字数、用户明确选择的 2500/5000 档位。
2. 多项缺失时合并成一次简短追问，只询问缺失项；不按顺序发起固定问卷。
3. 三项未齐全时保存 draft，但不允许模型用默认题材、默认总字数或动态章节档位代替用户决定。
4. 三项齐全后，在现有合同生成/修复 prompt 中要求模型自主生成完整合同，包括主情绪、主角结构、人物、关系、主角反差、核心冲突、世界治理、长篇结构和终局。
5. “主角反差”必须落到已有字段，至少在主角弧线或人物起始状态中形成可执行矛盾；不得只生成平台标签。
6. 用户自然语言修改合同内容时，只 patch 受影响字段，并使用现有同步机制更新相关故事字段。
7. 合同摘要应让用户看见完整故事合同的关键内容，但不展示内部 JSON。

不升级为硬门：

- 不要求每本小说必须具有“隐藏大佬”“废柴逆袭”等强套路反差。
- 现实、群像、文学向故事允许使用更细微的人格、身份或价值反差。
- 反差表达不够商业化不能阻止合同确认。

替换/删除：

- 如果实施时发现 prompt 中存在要求用户逐项补合同内部字段的固定追问，应删除；只允许针对题材、总字数和章节档位中的实际缺失项追问。
- 新建小说路径中用动态章节档位替代用户选择的行为必须退出；动态推导 helper 可保留给历史兼容和迁移，但不得被新建 intake 调用作为已满足输入的依据。
- 新建小说中，非 2500/5000 的原始章节字数不能静默取最近档位；应提示用户明确选择两档之一。现有 nearest normalization 只保留给旧数据规范化、迁移或恢复。
- 题材必须能从当前用户自然语言或已保存的用户草案权威中提取；不得把合同模型后来补出的 `genre` 反向冒充为用户输入。除非现有调用链无法区分来源，否则不为此新增一套题材 provenance 字段。
- 接线完成后删除被替换的固定问卷分支或无生产调用 helper；如果当前不存在，则不为了形式新增再删除。

回归：

- 仅给题材、总字数和章节档位就能生成完整可确认合同。
- 缺任意一项时只追问缺失项，三项未齐全前不启动完整合同生成。
- 用户未明确选择章节档位时，动态推导结果不能伪装成用户选择。
- 用户输入 3000/4000 等非档位值时不会静默归一为 2500/5000，而是请求明确选择。
- 通用创建 intake 的其他文档类型行为不变，成人内容年龄确认不回退。
- 不出现六连问。
- 用户一句话修改书名、主角性别、情绪方向或结局时，只更新相关字段。
- 用户未修改的姓名、世界规则和总字数保持不变。

## Phase 2：按当前章节相关子图精准投影现有创作合同

目标：解决“合同已有正确人物/实体/时序规则，但本章模型因通用前 N 项截断看不到正确条目”的接线缺口。

唯一修改 owner：

- `novel_studio/context_packaging.rs::relevant_contract_view`
- `novel_studio/context_packaging.rs::relevant_story_bible_view`
- 两个工作视图必须复用同一个本地 relevance selection/predicate 集合，不能分别复制一套筛选规则。

升级：

1. 在选出 `relevant_names` 和 `relevant_ids` 后，对合同与 Story Bible 两个工作视图同步过滤人物索引数组：
   - `character_voice_ledger`
   - `emotional_state_ledger`
   - `relationship_interaction_quotas`
   - `power_progression.character_current_levels`
   - `time_model.age_progression`
2. 保留当前主角、明确出场人物、当前章节关系事件涉及人物和按计划本章登场人物；不得把尚未计划登场的未来人物规则带入本章。
3. 对实体/时序数组先复用已有 chapter plan、chapter contract、当前分卷、approved settlement、pending hook/payoff 和明确 schedule/status 选出当前物件、对手、兑现项、母题和揭示项，再执行预算压缩；不能仅凭模糊文本相似度扩大上下文。
4. `artifact_ledger`、`antagonist_pressure.antagonists`、`payoff_matrix`、`motif_ledger`、`reveal_schedule` 实施前逐项确认已有筛选；已有正确窗口规则（例如 Story Bible 的 hook/payoff 章号过滤）必须复用，不能覆盖或另写第二份。
5. 先完成确定性相关性选择和稳定排序，再执行现有数组数量和字符预算压缩；主角、当前明确实体和到期项优先级高于仅作为背景的全局条目。
6. 在 context trace 中保留各类条目的包含/排除原因，使测试能证明当前规则没有被压缩掉，同时能发现误纳入未来项。

保持不动：

- `character_ledger` 权威。
- `relationship_ledger` 已有过滤。
- protected/compressible 分层。
- 36,000 字符 prompt context 总预算。
- 完整上下文磁盘记录和 fingerprint。
- `SealedChapterAuthority` 的规范合同、truth、章节计划、章节合同、架构及四种角色投影校验。
- 持久化的完整合同和 Story Bible；过滤后的 JSON 只能是本章工作视图，绝不能回写或裁剪规范权威。

替换/删除：

- 对上述实体型数组，使用“先相关性选择和稳定排序、后通用截断”替换“直接依赖通用前 N 项截断”。
- 不删除通用 `compact_json_prompt_view`，它继续作为最终预算保护。
- 不新增 `current_character_voice.json`、人物声音缓存或第二份人物账本。
- 不新增另一套 Writer/Auditor/Reviser 上下文构造器；四个角色继续消费同一封存根下的只读投影。

回归：

- 合同存在 12 名以上人物时，第 10 名但本章明确出场的角色声音仍进入 prompt。
- 第 10 名当前人物对应的情绪、力量和年龄状态仍进入 prompt。
- 未出场角色不会因出现在完整合同中被提前带入正文。
- 超过 8 个物件、对手、payoff 或 reveal 条目时，本章明确引用/到期的后排条目仍进入 prompt，未来未到期条目不被误纳入。
- 合同工作视图与 Story Bible 工作视图对同一人物/实体的选择一致，不存在一份保留、一份截断的冲突。
- 当前人物的姓名、声音、情绪和关系规则一致。
- prompt 预算没有无界增长。
- 过滤后磁盘规范合同的字段数量和指纹不发生变化，sealed authority 的角色投影验证继续通过。

## Phase 3：开篇交付规则接入现有章节执行包

目标：借鉴“黄金开篇”的创作原则，但不复制三个版本、独立文件和用户选择流程。

唯一修改 owner：

- `novel_runner/core/prompts.rs::chapter_execution_prompt`
- `novel_runner/core/prompts.rs::writer_prompt`
- 如需本地验证，仅扩展现有 prompt/执行包测试。

第一章规则：

1. 五场景架构的第一个节点必须源自：
   - `reader_promise.core_hook`
   - 第一章 `goal/expected_turn`
   - 主角起始状态或核心反差
2. 前两三段应尽快建立至少一个具体阅读问题：异常、冲突、代价、危险、选择或关系张力。
3. 不允许只用世界观说明、天气、醒来、照镜子、人物履历或抽象感慨占满开头。
4. 不要求固定 50 字、固定句式或固定百分比，也不生成三个完整候选。

普通章节规则：

1. 优先承接上一批准状态、当前章节目标或仍需推进的已有钩子。
2. 不得为了“开头抓人”凭空制造新主线、新物件或新人物。
3. 不得把下一章边界中的未来事件提前搬到开头。
4. 允许缓冲章、情绪章和日常章采用非冲突开头，只要它完成合同规定的场景功能。

性质：

- 开篇规则属于 writer guidance。
- 开篇任务从已确认 `reader_promise`、当前章节目标和 sealed authority 推导，只细化现有五场景架构的第一个节点，不替代完整章节合同或其余四个场景节点。
- 本地只检查明显结构错误、污染和合同漂移。
- “不够吸引人”只能作为 advisory。
- 不能因为开篇建议触发整章重写。

替换/删除：

- 如果现有 prompt 中存在与上述规则重复的笼统开头句，应原位改写为一份统一说明，不能再追加第二段相同规则。
- 不增加 `opening_strategy` 持久化权威字段；第一个 `architecture` 场景节点和现有 memo 已能承载该任务。

回归：

- 第一章 prompt 明确使用 reader promise 和第一章目标。
- 第二章以后不再收到“黄金第一章”规则。
- 开篇建议不会进入 `ChapterFinding::hard_blocking()`。
- 开篇不能提前消费下一章事件。

## Phase 4：对白与正文表现指导接入现有 Writer/Auditor

目标：补足参考 Skill 中有价值的通用写作原则，同时避免番茄模板化和主观阻断。

Writer 指导：

- 每段对话至少承担行动推进、信息揭示、关系变化、冲突加压或人物暴露中的一种功能。
- 当前人物遵守投影进来的 `character_voice_ledger`。
- 允许潜台词和动作反应，不要求人物把动机全部说明。
- 用具体行动、感官和选择承载关键变化，避免连续抽象总结代替场景。
- 长短句和段落密度应服务当前场景，不规定固定比例。
- 不强制删除某些形容词、四字词或文体特征。
- 不强制插入可摘录金句。

Auditor advisory：

- 前两三段是否建立当前章节问题。
- 主要人物对白是否逐渐同质化。
- 对话是否主要用于解释设定而不推动事件。
- 是否用总结替代了关键行动、代价或关系变化。
- 修饰语是否遮蔽了主体行动。
- 句段节奏是否在整章中长期单一。

唯一修改 owner：

- 写作指导：`novel_runner/core/prompts.rs`
- 审稿建议：`novel_workflow_driver/quality.rs::llm_quality_audit_prompt`
- 结果持久化：现有 `ReviewReceipt.advisories`

保持不动：

- `ReviewReceipt.findings` 的 hard blocker 证据规则。
- `score` 仅作为 telemetry。
- 有限修订、净提升、最佳版本和回滚机制。
- 现有确定性章节质量门和最终正文结算；新增文风建议不能覆盖这些结论。

替换/删除：

- 将现有“中文是否通顺/节奏”笼统段落原位细化，避免在 prompt 尾部再附加一份重复清单。
- 如果同一建议已经由本地确定性质量检查承担，则 LLM 只做补充观察，不再生成第二个 hard 判定。

回归：

- 模型返回“节奏偏平”“对白不够鲜明”时章节仍可批准。
- 同一意见不会进入语义修订循环。
- score 低但无 hard finding 时仍为 passed。
- 确定的人物身份冲突仍然阻断。

## Phase 5：把现有每五章调度升级为跨五章阶段审查

目标：每完成五章，由模型自动检查最近五章的整体创作表现，不询问用户，不改变故事权威。

代码事实与迁移边界：

- `novel_workflow_driver/quality.rs::chapter_requires_periodic_full_audit`
- 当前条件为 `chapter_number > 0 && chapter_number % 5 == 0`，但它现在影响的是批准前的当前单章审稿，不是真正的五章窗口审查。
- 第 1、2 章的 LLM 单章审稿有独立冷启动价值，必须保留。
- 第 5 倍数章当前存在的额外单章 LLM 审稿应迁移为批准后的五章窗口调用；每章的确定性审查照常执行。
- 当前 LLM 审稿里的 authority conflict 只有得到本地 hard finding 的逐项证据验证才会阻断；因此迁移第 5 倍数章调用不会移除任何独立 hard authority。第 5 章有确定错误时仍会在 approval 前由现有本地质量门/状态门阻断。
- `ReviewReceipt` 是单章批准事务依赖：`latest_passing_review` 取同章最后一条记录，`ApprovalReceipt.review_fingerprint` 与其绑定，恢复校验和 `review_pass_rate` 也消费 `manifest.reviews`。窗口报告写入该数组会污染批准凭证和统计，因此禁止复用。
- `ReviewCycleRecord` 管理单章修订迭代和 `next_action`，也不能承载批准后的窗口建议。
- `LongformArchiveRecord` 会被当作长篇连续性材料投影，`StyleProfileRecord` 会进入 truth/style，`VolumeSummaryRecord` 是分卷事实摘要；三者都不能伪装成临时创作建议。

唯一职责分配：

- 周期判断、窗口 prompt、typed 输出解析：`novel_workflow_driver/quality.rs`，复用现有质量模型调用基础设施。
- approval 成功后的有界调用与恢复补做：`novel_workflow_driver/chapter.rs`，不得让 workflow driver 直接改 manifest 文件。
- 记录类型、原子 artifact/manifest upsert：`novel_studio/model.rs` 与现有 `novel_studio/runtime_records.rs`/manifest 保存路径，通过内部 studio action 提交。
- 下一阶段建议选择与预算：`novel_studio/context_packaging.rs`。
- 不创建第二个 workflow、scheduler、review cycle 或对外可手工驱动的平行小说工具。

调度决策：

- 复用 `chapter_number % 5 == 0` 这一周期语义，但将 helper 改名或原位重定义为“完成五章审查窗口”，避免继续用 `full_audit` 表示批准前单章审稿。
- 不新增第二个计数器、定时器或 scheduler job。
- 触发依据必须是磁盘连续 approved 端点，而不是模型声称“已经完成五章”。
- 只有第 5、10、15……章的 approval receipt 已提交后，才能生成对应窗口报告。
- 在现有章节循环的 approval commit 成功后接入唯一 post-approval hook；旧的第 5 倍数章批准前 LLM 分支迁移完成后删除，不能双跑。
- 下一章规划前同步执行一次有界窗口审查，成功建议才进入下一阶段；失败/降级立即放行。启动恢复时若发现连续 approved 端点已跨过窗口但缺少对应成功或 terminal-degraded 记录，调用同一幂等 helper 补做，避免 approval 后崩溃永久漏审。

五章输入窗口：

对 `N-4..N` 的每章读取：

- 章号和最终标题。
- approved body fingerprint。
- authority fingerprint。
- approved settlement 的 `chapter_summary`、`current_state`、`pending_hooks`、typed state changes 和 resolved hooks。
- 正文开头受控摘录，用于判断开篇重复和读者承诺交付。
- 正文中段的少量均匀采样，用于观察场景类型、对话功能和情绪推进；不得为此把五章全文无界塞入 prompt。
- 正文结尾受控摘录，用于判断章尾重复和钩子类型。
- 已有 ReviewReceipt 的 advisories。
- 已有 hook debt 报告。

不得读取：

- 未批准草稿。
- stale settlement。
- 后续章节执行包。
- 下一阶段未来正文或模型内部推测。
- 与该五章无关的完整项目正文。

模型检查维度：

1. 五章因果和地点/任务衔接是否自然。
2. 开头方式是否连续重复。
3. 章尾动作、悬念句或情绪落点是否重复。
4. 冲突形式和解决方式是否单一。
5. 情绪压力是否长期没有起伏或突然无因跳变。
6. 场景类型是否严重偏科。
7. 主要人物对白是否同质化。
8. 关系推进是否停滞或无证据跳跃。
9. 伏笔是否只新增、不推进，或连续数章被遗忘。
10. `reader_promise` 和当前分卷目标是否仍在被兑现。

输出边界：

- 只输出结构化 advisories 和可选 score telemetry。
- advisory category 只允许开篇、章尾、对白、场景配比、句段节奏和读者承诺交付等表达/交付维度，不提供新增剧情事实、改人物身份、改终局或改伏笔状态的入口。
- 不输出 hard finding。
- 不决定 `verdict`。
- 不触发旧章修订。
- 不修改合同、大纲、人物、Story Bible、truth 或 hook ledger。
- 报告解析失败、模型超时或服务重启时记录 degraded telemetry，并继续下一章。
- 复用现有有界推理/传输重试，不建立阶段报告的语义修订循环；达到现有上限后把该窗口记为 terminal degraded。

持久化决策：

- 不复用 `ReviewReceipt`、`ReviewCycleRecord`、`LongformArchiveRecord`、`StyleProfileRecord` 或 `VolumeSummaryRecord`。
- 现有结构中没有既不参与批准、又不进入故事 truth/continuity 的窗口建议记录，因此允许在 `novel_studio/model.rs::NovelProjectManifest` 中增加一个最小、向后兼容的 `delivery_advisory_windows` 数组及 `DeliveryAdvisoryWindowRecord`。这是补齐缺失的非权威持久化槽位，不是第二套审稿状态机、节奏账本或故事权威。
- 最小记录只保存窗口起止章、五章 approval/body/authority 聚合指纹、结构化 advisories、可选 score、artifact path、生成状态/降级原因和时间戳；不包含 `verdict`、hard findings、修订轮数或 `next_action`。
- 通过现有 manifest 原子保存路径写入；以“窗口范围 + 聚合指纹”执行 upsert，不建立独立 manifest 或后台 scheduler。
- 报告必须绑定五章 approval receipt/body fingerprints，正文变化后旧报告自动失效。
- 相同五章指纹组合只生成一次报告，服务重启不能重复调用模型。
- 窗口报告不得计入 `review_pass_rate`，也不得参与 `latest_passing_review`、`ApprovalReceipt.review_fingerprint`、批准恢复或章节 verdict。

供下一阶段使用：

- `context_packaging` 只选择最近一份仍有效的窗口 advisories。
- 最多投影少量、可执行的交付建议，例如“下一阶段避免连续使用发现异常式开头”。
- 这些建议放在带有 `authority=false`、`scope=delivery` 标记的工作上下文区域，不进入规范合同、truth、Story Bible 或 required protected coverage。
- 为了复现一次写作调用，该非权威工作建议可以随当前工作上下文一起被 sealed root 指纹绑定；“被封存以保证只读复现”不等于升级为故事事实权威。
- 到下一次五章报告生成时自动替换旧建议，不无限累积。
- 建议只能改变表达、场景配比、开篇和章尾形式；不能改变故事事实和终局方向。

替换/删除：

- 将“第 5 倍数章只因为章号整除而进行一次更重的当前章 LLM 审稿”替换为“当前章确定性审稿 + 批准后五章窗口诊断”。第 1、2 章的 LLM 单章审稿保持不动。
- 复用同一周期判断语义；不得保留两个 `% 5 == 0` 触发 owner。
- 删除迁移后不再使用的旧 periodic 单章 full-audit 分支和过时测试，不得让两次模型审稿并行存在。
- 不复用单章 ReviewReceipt 的方案属于已排除设计，实施时不得重新引入 `scope/window_start/window_end` 来混用它。

回归：

- 只有 1～4 章 approved 时不触发窗口审查。
- 第 5 章未批准或 receipt 未提交时不触发。
- 第 5 章批准后窗口严格为 1～5。
- 第 10 章批准后窗口严格为 6～10，不混入 1～5 的过期建议。
- 重启后相同 fingerprints 不重复生成报告。
- 报告超时或 JSON 解析失败不阻断第 6/11 章。
- 报告中的节奏建议不能成为 hard finding。
- 报告不能修改合同或已批准状态。
- 第 5、10 章不会同时执行“周期单章 LLM 审稿”和“五章窗口 LLM 审稿”。
- 生成窗口报告前后，`manifest.reviews`、`review_cycles`、`review_pass_rate`、同章 `latest_passing_review` 结果和已提交 approval receipt 校验结果完全不变。
- `delivery_advisory_windows` 旧项目反序列化默认空数组；相同范围和聚合指纹只保留一条有效记录。

## Phase 6：可选的真实读者数据入口，暂缓实施

参考 Skill 支持完读率、追读率和评论关键词，但 BenShu 当前没有平台数据连接。这不是完成小说的前置能力，本计划默认不实施。

以后如实施，必须满足：

- 只有用户主动提供真实数据时才处理。
- 明确标记数据来源和适用章节范围。
- 模型先生成建议，不自动改合同。
- 用户明确要求调整后，才通过现有自然语言合同/滚动计划修改入口处理。
- 已批准正文和已结算 truth 不反向改写。
- 不根据模型预测的“评论区争论点”制造虚假数据。

## 5. 机制保留、升级、替换与拒绝总表

| 能力 | 当前 owner | 决策 | 是否删除旧代码 |
| --- | --- | --- | --- |
| 通用创建 intake | `runtime-policy-core` | 保持通用识别，不承载小说三项权威 | 否 |
| 小说三项输入前置检查 | `creation_contract/chat_flow.rs` + 现有字段提取器 | 在合同模型前补齐 | 删除新建路径对动态默认的错误依赖 |
| 一次合同确认 | draft lifecycle/user view | 保持 | 否 |
| 用户自然语言修改合同 | intent + typed patch | 保持并补回归 | 只删除发现的旁路 |
| 完整多维合同自动生成 | 现有 `NovelCreationContract` / `NovelContractV2` | 三项齐全后强化生成 prompt，保留现有字段强度/readiness | 不新增精简合同、六维合同或新 blocker |
| 主角反差 | 主角弧线/人物字段/读者承诺 | 原位表达 | 不新增反差账本 |
| 章节数量 | `longform_policy` | 保持按总字数计算 | 拒绝固定章数 |
| 场景规划 | ChapterExecutionPackage | 保持五场景 | 拒绝第二套 3～5 场景规划器 |
| 当前人物/实体/时序规则 | context packaging 的合同与 Story Bible 工作视图 | 复用同一相关性选择后再压缩 | 替换直接依赖前 N 项截断的路径 |
| 第一章开篇 | execution/writer prompt | 原位补充 | 删除重复 prompt 说明 |
| 三个开篇候选 | 无 | 不引入 | 不创建 |
| 对白/展示/节奏 | writer + LLM advisory | 原位细化 | 删除重复审稿文案 |
| 单章连续性 | typed quality/state truth | 保持 | 否 |
| 单章字数 | longform/quality gate | 保持 | 拒绝外部固定字数脚本 |
| 伏笔生命周期 | novel bible/settlement | 保持 | 拒绝第二套钩子表 |
| 每五章调度 | 现有 `% 5 == 0` helper 语义 | 迁移为 approval 后唯一窗口触发并准确改名 | 删除旧 periodic 单章 LLM 分支和重复条件 |
| 五章整体报告 | `NovelProjectManifest.delivery_advisory_windows`（新增最小非权威槽位） | 与单章 review/approval 隔离 | 不建新 manifest、状态机或故事账本 |
| 主观评分 | ReviewReceipt telemetry | 保持非阻断 | 拒绝分数 gate |
| 每章询问继续 | 无 | 不引入 | 不创建 |
| 外部读者数据 | 无 | 延后、用户主动 | 当前不实施 |

## 6. 风险点与控制措施

### 风险 1：把创作建议重新变成硬阻断

表现：开篇不够抓人、节奏偏平或对白相似导致章节反复重写。

控制：

- 所有新增观察只进入 `advisories`。
- 不增加新的 `ChapterFindingDisposition` hard 类型。
- 测试必须证明低分和软建议不改变 passed verdict。

### 风险 2：五章报告成为第二份故事权威

表现：模型根据节奏建议修改角色身份、终局、世界规则或伏笔事实。

控制：

- 报告明确标记为 delivery advisory。
- 不写入 contract、Story Bible、truth 或 settlement。
- 下一阶段 prompt 把它放在非权威区域。

### 风险 3：窗口报告污染单章批准凭证

表现：把窗口报告追加到 `manifest.reviews` 后，`latest_passing_review` 选中错误记录，导致 approval receipt 指纹、恢复校验和通过率改变。

控制：

- 使用独立的 `delivery_advisory_windows` 非权威数组，不复用 `ReviewReceipt` 或 `ReviewCycleRecord`。
- 窗口记录不含 verdict/hard findings/next_action，不进入单章批准或修订调用链。
- 回归固定比较生成报告前后的 latest passing review、approval receipt 校验和 review pass rate。

### 风险 4：当前章节投影遗漏必要人物或实体

表现：过滤后丢失本章确实需要的人物声音、状态、物件、对手或到期兑现/揭示项。

控制：

- 相关性来源同时使用当前章节种子、章节合同、计划、架构和计划登场信息。
- 主角始终保留。
- 实体/时序项优先使用明确名称、已批准状态、pending hook、payoff/reveal window 和当前分卷，不用模糊相似度猜测。
- 合同与 Story Bible 复用同一选择器；规范权威保持完整。
- trace 记录包含和排除原因。
- 增加 12 名以上角色的回归夹具。

### 风险 5：开篇规则导致所有章节公式化

表现：每章都以事故、追杀、倒计时或惊醒开头。

控制：

- 第一章和普通章节使用不同规则。
- 不规定固定 50 字、固定 20% 或固定钩子类型。
- 普通章节允许日常、缓冲和情绪场景。
- 五章报告只提醒重复，不指定唯一替代模板。

### 风险 6：为了开篇抓人提前消费未来事件

表现：把下一章人物、揭示或关键物件提前写进本章。

控制：

- 开篇仍服从 sealed authority 和 next chapter boundary。
- 只能从当前章节目标和既有钩子选材。
- 已有未来事件提前消费检查保持 hard blocker。

### 风险 7：prompt 膨胀导致受保护上下文被挤压

表现：新增创作提示和五章建议后，人物、合同或状态被截断。

控制：

- 当前人物相关性过滤应先减少无关内容。
- 开篇和对白规则保持短小，不复制 Skill 的长参考文档。
- 五章窗口只携带 settlement、首尾摘录和受控中段采样，不默认携带五章全文。
- 五章建议数量和字符数设上限，只保留最近有效窗口。
- protected authority 的预算优先级不变。

### 风险 8：五章报告在修订或重启后过期

表现：报告审查的是旧正文，却继续影响下一章；重启后重复调用模型。

控制：

- 报告绑定五章 approval/body fingerprints。
- 任一 fingerprint 改变即视为 stale。
- 以 fingerprint 组合作为幂等键。
- 只有 approval receipt 提交后生成。

### 风险 9：同一个本地模型自写自审产生主观偏差

表现：模型长期偏好同一种风格，阶段报告误以为正常或提出波动建议。

控制：

- 阶段报告只做 advisory。
- 尽量提供可比较的五章首尾摘录和 settlement，而不是让模型凭记忆判断。
- 不因报告结论修改权威或重写旧章。
- 独立审稿模型仍属于后续选项，不是本计划前置条件。

### 风险 10：自然语言修改合同造成字段不同步

表现：用户只改主角或结局，旧姓名、旧终局仍残留在大纲、声音表或兑现矩阵。

控制：

- 必须继续走现有 typed patch 和跨字段 canonicalization。
- 修改后重新运行 typed gate。
- 受影响字段同步改写，未受影响字段受保护。
- 不允许聊天层直接改 manifest 文本。

### 风险 11：实施过程中再次出现重复机制

表现：新建开篇模块、节奏 ledger、问卷状态机或独立周期审稿器。

控制：

- 每个 Phase 开始前执行 Phase 0 检查。
- 每个提交必须列出复用的 owner 和删除的旧路径。
- diff 审查搜索相同调度条件、相同 prompt 规则和相同持久化字段。
- 若发现旧机制职责错误，先确定迁移和删除路径，再接入新实现。

### 风险 12：小说三项前置检查误伤通用创建或旧项目恢复

表现：把小说专属规则写入 runtime policy 后，其他文档创建也被强制追问；或旧项目因缺少新 provenance 而无法恢复。

控制：

- 三项检查只进入写作工具的新小说/未确认草案路径。
- 已确认项目、迁移、恢复和其他 artifact kind 保持现有兼容策略。
- 不修改 `runtime-policy-core` 的通用自治语义；由 writing owner 在进入合同模型前判定小说三项是否满足。
- 新建路径只接受用户原始 2500/5000 选择，旧数据仍可通过现有 normalization 恢复。

## 7. 验证计划

### 7.1 单元回归

合同交互：

- 题材、总字数和章节档位齐全的最小自然语言请求不会触发额外问卷。
- 缺少三项中的任一项时，只返回一次针对缺失项的简短追问。
- 三项齐全后，合同生成链覆盖完整现有多维结构，而不只生成参考 Skill 的六项；不适用/滚动字段仍按现有 field requirements 和 readiness 保持可选。
- 输入非 2500/5000 的章节字数时要求用户明确选择，不静默取最近档位。
- `runtime-policy-core` 的其他 artifact 创建行为不变，旧项目恢复不受新建前置检查阻断。
- 用户可以自然语言修改字段。
- 修改只影响相关字段。
- 用户看不到 JSON、工具参数或内部路径。

上下文投影：

- 当前人物声音、情绪、关系配额、力量和年龄状态必定进入本章 prompt。
- 当前明确物件、对手、到期 payoff/reveal 位于数组第 8 项以后时仍能进入 prompt。
- 无关未来人物被排除。
- 数组超过 8 项时仍优先保留当前人物。
- 合同工作视图与 Story Bible 工作视图使用同一选择结果。
- 相关性过滤后仍服从上下文预算。
- 规范合同、Story Bible 和 sealed authority 的完整数据未被工作视图过滤回写。

开篇与正文指导：

- 第一章包含第一章专用开篇指导。
- 后续章节使用承接式开篇指导。
- 开篇指导不进入 hard finding。
- 对话和节奏建议只能进入 advisories。

五章阶段审查：

- 1～4 章不触发。
- 5、10、15 章批准后分别生成正确窗口。
- 未批准章节不会进入窗口。
- stale settlement 不会进入窗口。
- 相同 fingerprints 重启后不重复审查。
- 模型超时、输出损坏或 score 低不阻断下一章。
- 阶段报告不修改合同、truth、hook ledger 或 approved body。
- 阶段报告只写 `delivery_advisory_windows`，不追加 `reviews` 或 `review_cycles`。
- 生成阶段报告前后，review pass rate、latest passing review、approval receipt 指纹与恢复校验结果不变。
- 第 5 倍数章只有一次周期 LLM 调用，不保留旧 periodic 单章审稿与窗口审稿双调用。

现有核心回归：

- 2500/5000 档位不变。
- 任意总字数的预计章数计算不变。
- 人名权威和跨字段同步不回退。
- 下一章边界和未来事件禁止提前消费不回退。
- 最终正文 settlement 和状态污染门不回退。
- 有限修订、净提升和最佳版本回滚不回退。

### 7.2 真实聊天验收

真实测试必须使用新 session、新项目和普通用户语言，不能把测试约束写进用户消息。

示例用户消息：

```text
写一本10万字的都市小说，每章2500字。
```

合同展示后再用自然语言修改，例如：

```text
主角改成女性，职业换成夜班急诊医生，结局不要离开这座城市。
```

验收流程：

1. 系统不进行六问，自动生成完整合同。
2. 用户自然语言修改成功，其他稳定字段不被重写。
3. 用户一次确认后自动写作。
4. 连续批准第 1～5 章。
5. 第 5 章批准后自动生成 1～5 章阶段报告，不询问用户。
6. 系统自动进入第 6 章。
7. 连续批准至第 10 章。
8. 第 10 章批准后自动生成 6～10 章阶段报告。
9. 检查两份报告是否只影响交付方式，没有修改故事权威。
10. 检查章节编号、人物姓名、关系、世界规则、伏笔、状态和大纲窗口连续性。

任何生产代码修改都会使该次真实测试失去“无人干预连续运行”证明力；修复后必须从新 session、新题材和第 0 章重新测试。

## 8. 完成标准

只有同时满足以下条件，计划才算实施完成：

- 用户只需自然语言提出题材、总字数和章节档位。
- 系统只要求用户提供题材、总字数和章节档位，其余完整多维合同由模型生成。
- 现有合同字段、字段强度/readiness、任意总字数、2500/5000 档、自动连续写作和确定性硬门没有被削弱。
- 用户看完合同后可以用自然语言修改相关字段。
- 修改后合同仍由同一 typed gate 校验，人物和故事字段保持同步。
- 当前出场人物的声音、情绪、关系、力量/年龄状态，以及当前物件、对手和到期兑现/揭示项稳定进入章节 prompt；未来无关项不提前泄漏。
- 第一章具有由读者承诺和当前合同推导的开篇任务。
- 普通章节承接已批准状态，不为了抓人提前消费未来事件。
- 对白、展示和节奏检查全部保持 advisory。
- 每五个连续 approved 章节自动生成一次跨章表现报告。
- 阶段报告失败不阻断写作，成功报告不修改故事权威。
- 阶段报告与 `ReviewReceipt`、review cycle、approval receipt 和 review pass rate 完全隔离。
- 没有新增第二套合同、上下文、伏笔、人物、审稿或状态机制。
- 唯一新增的数据结构只是缺失的非权威 `delivery_advisory_windows` 持久化槽位，不具有 verdict、修订或故事事实职责。
- 被替换的旧代码、重复 prompt 和死分支已经删除。
- 单元回归通过。
- 新 session 真实聊天能越过第 5 章和第 10 章，且中间没有代码干预。

## 9. 明确的实施顺序

必须严格按以下顺序实施：

1. Phase 0：冻结基线并核对重复机制。
2. Phase 1：锁定三项用户必填输入并强化完整合同自动生成与自然语言修改回归。
3. Phase 2：修复当前章节人物、实体和时序规则的统一相关性投影。
4. Phase 3：把开篇交付规则接入现有章节执行包。
5. Phase 4：把对白和正文表现检查接入现有 Writer/Auditor advisory。
6. Phase 5：升级现有每五章调度为批准后的跨章阶段审查。
7. 完成单元、集成、格式和静态检查。
8. 审查完整 diff，确认无重复机制和遗留旧代码。
9. 经用户同意后再进行真实聊天测试。
10. Phase 6 外部读者数据入口保持暂缓，除非用户另行决定。

每完成一个 Phase 都必须重新核对本文对应细则，不能把后续 Phase 的功能提前塞入当前改动，也不能以真实测试中的某个题材失败为理由增加题材特例。

## 10. 实施核对记录（2026-08-02）

- Phase 0：完成旧机制与生产调用链核对。未新增 `tomato_*`、问卷状态机、第二套合同/人物/伏笔/状态账本，也未把小说规则写入 gateway、`chat.rs` 或通用 runtime policy。
- Phase 1：在现有创建草案入口接入题材、任意正整数总字数、2500/5000 精确档位三项前置检查；新建路径不再把非档位值静默归一成用户权威，历史兼容 normalization 保留。主角反差只映射到既有人物弧线、欲望、恐惧、底线与读者承诺。
- Phase 2：合同工作视图和 Story Bible 工作视图复用同一 `ChapterRelevanceSelection`，先选择当前人物、实体和时序项，再进入既有预算压缩；规范合同、Story Bible 和 sealed authority 未被裁剪回写。
- Phase 3：第一章与普通章节开篇规则原位接入现有执行包和 Writer prompt，没有新增开篇文件、模型调用或硬门。
- Phase 4：对白、展示与句段节奏只进入现有 Writer guidance 和 `ReviewReceipt.advisories`；删除了把近章章尾形式重复直接要求重写的旧主观规则。
- Phase 5：保留第 1、2 章单章 LLM 审稿，将唯一 `% 5` 周期语义迁移为批准后的五章窗口建议；新增的 `delivery_advisory_windows` 与单章 review、review cycle、approval receipt 和故事 truth 隔离，并按批准回执、正文、权威及 settlement 指纹幂等校验。
- 回归：`creation_contract`、`novel_studio`、`novel_workflow_driver` 三组测试全部通过；格式化和 `cargo check -p benshu-builtin-tools` 通过。
- 真实聊天简测：全新 session 仅输入“写一本10万字的都市悬疑小说，每章2500字。”，系统正确解析题材、总字数、档位和约 40 章规模，自动生成可确认合同；用户仅回复“就按这个写。”后，第 1、2 章分别以 2756、2946 字通过审稿、最终正文状态结算并批准保存。第 3 章已进入正文生成，但按用户要求取消；本次不计为跨 5/10 章完整验收。
- 简测未通过内容连续性验收：第 2 章封存权威明确把“发现无法闭合的致命缺口”列为下一章禁区，但第 2 章最终正文、标题/摘要和 truth validation 已写入并接受“未闭合断裂”，提前消费了第 3 章核心事件。说明权威投影已到达 Writer，但最终正文未来边界检测没有正确识别语义等价事件；后续应先审查现有 future-boundary hard gate 的证据匹配与最终正文接线，不能另建平行机制。
