# BenShu 小说结构化合同 v2 升级计划

> 状态: Phase 1-7 已完成代码闭环；Phase 8 待真实面板长篇回归
> 适用范围: `crates/builtin-tools/src/tool/writing/*`，以及必要的 gateway / panel 展示接线
> 原则: 工具策略归工具治理；本计划只属于 `writing/novel_studio`，不进入 `runtime-policy-core`

## 0. 当前代码核对结论

截至当前代码状态：

- 已存在基础长篇底座：`SessionCreationDraftState`、`NovelProjectManifest`、`StoryContract`、`StoryBible`、`VolumeRecord`、`ChapterContractRecord`、章节计划、上下文包、审稿、truth validation、hook debt、快照和导出。
- 已存在权威状态的一部分：`title_state`、`character_ledger`、`story_bible`、`volumes`、`volume_summaries`、`chapter_summaries`、`hook_ledger`、`timeline`、`genre_governance`。
- 已完成入口级修复：合同阶段不再因为章节标题句式/残片问题阻断；“按这个合同开始 / 按这个创作合同开始”应进入正式写作路径，而不是重新进入 creation planning。
- 已实现 v2 的 12 类结构化字段，代码落点为 `novel_contract_v2.rs`，项目/草案/story bible 中以 `structured_contract_v2` 保存，工具调用面仍暴露同名结构化字段。
- 已实现 v2 每章执行合同细化字段：`scene_goal`、`conflict`、`choice`、`cost`、`reveal`、`emotional_beat`、`relationship_delta`、`power_delta`、`resource_delta`、`hook_opened`、`hook_paid_off`、`character_change`、`world_change`、`payoff_target`。
- 本文档仍保留后续真实回归项；实现必须继续复用现有底座，不另起一套写作系统。

## 1. 背景

当前小说写作工具已经具备基础长篇治理能力，包括：

- 项目草案 `SessionCreationDraftState`
- 小说项目 `NovelProjectManifest`
- 故事合同 `StoryContract`
- 故事圣经 `StoryBible`
- 分卷 `VolumeRecord`
- 章节计划、章节合同、上下文包、审稿、truth validation、hook debt、快照和导出

但多轮真实面板测试暴露出一个核心问题：系统已经能“写下去”，但长期质量依赖大量散落文本字段，导致经济、情感、关系、力量、制度、时间、物品和对手压力等关键小说状态不能被稳定治理。

现在很多信息被混进：

- `world_rules`
- `outline`
- `characters`
- `planning_notes`
- `chapter.summary`
- `continuity_updates`

这些字段能保存内容，但不够结构化，难以做稳定检查、续写、回滚、导出和长篇压缩。

## 2. 目标

升级目标是把小说写作从“LLM 每章临场发挥”改成：

```text
用户自然语言需求
-> 可见合同摘要
-> 内部结构化故事合同 v2
-> 项目 manifest / story bible 权威状态
-> 每章执行合同
-> 正文生成
-> 分层质量门
-> approve 后更新权威状态
-> 导出与续写
```

最终要达到：

- 任意类型小说都能使用同一套通用合同框架。
- 不把玄幻字段硬套给言情、现实、悬疑等类型。
- 字段全部结构化保存，但面板只展示摘要。
- 长篇写作按需读取合同片段，不把完整合同或正文塞进上下文。
- 未批准草稿不能污染 story bible / truth / continuity。

## 3. 非目标

本计划不做以下事情：

- 不把小说策略放进 `runtime-policy-core`。
- 不让工具自己嵌套调用 LLM；LLM 调用仍由主 agent / worker 编排完成。
- 不把所有字段完整 JSON 展示给用户。
- 不把合同质量门做成用户填写表单。
- 不用固定玄幻模板、固定书名模板、固定角色名模板解决问题。
- 不通过特定题材规则修复通用写作问题。

## 4. 三层字段治理模型

本计划最终要实现 12 类字段，但不是全部同等强制。

### 4.1 第一层: 所有小说强制有

这些字段任何小说都需要。可以很轻，但不能缺。

```text
emotional_contract
relationship_ledger
chapter_execution_contract
payoff_matrix
narration_contract
time_model
antagonist_pressure
```

作用：

- 情绪不漂移。
- 关系变化有证据。
- 每章有明确推进目标。
- 伏笔、承诺和结局能兑现。
- 文风、视角和节奏稳定。
- 时间推进不混乱。
- 外部压力不断线。

质量门口径：

- 第一层缺失时，长篇不能进入正式写作。
- 短篇可以允许极简结构，但必须有字段和最小内容。

### 4.2 第二层: 大多数小说默认有

这些字段默认生成，但允许按题材轻量化。

```text
resource_economy
social_order
geography_model
```

权重示例：

| 类型 | resource_economy | social_order | geography_model |
| --- | --- | --- | --- |
| 玄幻 / 修仙 | strong | strong | strong |
| 科幻 / 星际 | strong | default | strong |
| 都市 | default | strong | default |
| 言情 | optional/default | default | optional/default |
| 悬疑 | optional | default/strong | strong |
| 现实 | optional/default | default | optional |

质量门口径：

- 第二层缺失不一定阻止第一章。
- 但应产生 warning，并要求在前 1-2 章或项目初始化修复中补齐。

### 4.3 第三层: 类型触发字段

这些字段根据题材决定强度。

```text
power_progression
artifact_ledger
```

启用示例：

- 玄幻、修仙、异能、游戏、升级流: `power_progression = strong`
- 科幻、机甲、赛博: `power_progression = default/strong`，可映射为技术成长或装备升级
- 悬疑、推理: `artifact_ledger = strong`，映射为证据/线索台账
- 言情、现实: `artifact_ledger = optional/default`，映射为信物、文件、照片、关键物品
- 纯现实短篇: 两者都可弱化或为空

质量门口径：

- 题材强相关时缺失可 blocking。
- 题材弱相关时只 warning。

## 5. 字段设计

### 5.1 `field_requirements`

内部合同需要保存每个字段的强度，避免硬套题材。

```json
{
  "field_requirements": {
    "emotional_contract": "strong",
    "relationship_ledger": "strong",
    "chapter_execution_contract": "strong",
    "payoff_matrix": "strong",
    "narration_contract": "strong",
    "time_model": "strong",
    "antagonist_pressure": "strong",
    "resource_economy": "default",
    "social_order": "default",
    "geography_model": "default",
    "power_progression": "genre_strong",
    "artifact_ledger": "genre_default"
  }
}
```

可选取值：

```text
strong
default
optional
disabled
genre_strong
genre_default
```

### 5.2 `resource_economy`

资源、货币和经济体系。

```json
{
  "currency": "主货币或主要计量单位；没有货币时可为空",
  "value_scale": "说明普通人、核心角色和高阶资源之间的价值尺度",
  "resource_types": ["货币", "稀缺资源", "身份资源", "信息资源"],
  "income_sources": ["角色可获得资源的常规方式"],
  "cost_examples": ["典型消费、升级、交换、行动代价"],
  "scarcity_rules": ["哪些资源稀缺、为什么稀缺、谁控制稀缺资源"],
  "trade_rules": ["资源能否交易、交易限制、交易风险"],
  "class_impact": "资源如何影响角色阶层、机会、关系和冲突"
}
```

### 5.3 `emotional_contract`

情绪承诺和读者情感路径。

```json
{
  "primary_emotion": "本书主要情绪体验",
  "emotional_promise": "读者持续阅读时期待被兑现的情绪承诺",
  "emotional_beats": [
    "开局阶段的主要情绪",
    "中段阶段的主要情绪变化",
    "高潮阶段的主要情绪冲突",
    "结尾阶段的情绪兑现"
  ],
  "payoff_requirements": [
    "必须兑现的情绪承诺",
    "必须回应的早期情绪伏笔"
  ],
  "ending_emotional_state": "结尾时主要角色和读者应抵达的情绪状态"
}
```

### 5.4 `relationship_ledger`

人物关系网。

```json
[
  {
    "characters": ["角色A", "角色B"],
    "relationship_type": "关系类型和变化方向",
    "start_state": "关系起点",
    "current_state": "当前关系状态",
    "desired_end_state": "结局或阶段结尾的目标状态",
    "conflicts": ["关系中的主要阻力"],
    "secrets": ["影响关系变化的隐藏信息"],
    "turning_points": ["计划中的关系转折"],
    "last_changed_chapter": null
  }
]
```

### 5.5 `power_progression`

力量、成长、技术或职业升级体系。

```json
{
  "system_name": "力量、职业、技术、地位或能力成长体系名称",
  "levels": ["阶段1", "阶段2", "阶段3"],
  "advancement_costs": ["成长或升级需要付出的代价"],
  "bottlenecks": ["阻止角色继续成长的瓶颈"],
  "failure_consequences": ["失败、越级、滥用能力的后果"],
  "anti_power_creep_rules": ["防止能力膨胀或剧情失衡的规则"],
  "character_current_levels": [
    {
      "character": "角色名",
      "level": "当前阶段",
      "evidence": "该状态来自哪一章、合同或设定"
    }
  ]
}
```

### 5.6 `social_order`

社会制度、组织、学校、宗门、公司、阶层。

```json
{
  "institutions": ["组织、学校、公司、家族、政权、社群或其他制度实体"],
  "rank_system": "身份、阶层、职位、等级或评价体系",
  "exam_or_promotion_rules": ["晋升、选拔、考试、评审或准入规则"],
  "laws": ["明面规则、禁令、潜规则或约束"],
  "class_structure": "不同群体之间的机会、权力和资源差异",
  "authority_conflicts": ["制度内部或制度之间的冲突"]
}
```

### 5.7 `geography_model`

地理、空间、关键场所。

```json
{
  "regions": ["主要区域或故事空间"],
  "important_locations": [
    {
      "name": "地点名",
      "role": "该地点在剧情中的作用",
      "known_facts": ["该地点的已知事实"]
    }
  ],
  "distance_rules": ["地点之间的距离、通行时间或空间关系"],
  "travel_constraints": ["移动限制、进入条件或空间风险"],
  "location_changes": []
}
```

### 5.8 `time_model`

时间、历法、年龄线、期限。

```json
{
  "calendar": "故事使用的时间单位、历法或叙事时间口径",
  "story_start_time": "故事开始时的时间状态",
  "elapsed_time": "当前已经经过的时间",
  "age_progression": [
    {
      "character": "角色名",
      "start_age": "起始年龄或阶段",
      "current_age": "当前年龄或阶段"
    }
  ],
  "deadline_events": ["必须在特定时间前发生或解决的事件"],
  "time_skip_rules": ["哪些事件不能跳过，哪些日常过程可以摘要跨越"]
}
```

### 5.9 `artifact_ledger`

关键物品、神器、证据、信物、设备。

```json
[
  {
    "name": "物品、证据、设备、信物或关键资产名称",
    "owner": "当前持有者或控制者",
    "origin": "来源",
    "ability": "功能、意义或可证明的信息",
    "cost_or_limit": "使用限制、代价或风险",
    "last_seen_chapter": null,
    "status": "seeded"
  }
]
```

### 5.10 `antagonist_pressure`

反派、对手或外部压力计划。没有传统反派时也要映射成疾病、制度、误会、自然灾害或内心困境。

```json
{
  "primary_pressure": "主要对抗压力；没有传统反派时填写制度、疾病、灾害、误会或内心困境",
  "antagonists": [
    {
      "name": "对手、压力源或阻力名称",
      "goal": "它想达成什么",
      "resources": ["它掌握的资源、权力、信息或行动优势"],
      "knowledge_state": "它知道什么、不知道什么、误判什么",
      "current_move": "当前正在采取的行动",
      "escalation_plan": ["后续如何升级压力"],
      "defeat_condition": "什么条件下这个压力被化解、转化或击败"
    }
  ]
}
```

### 5.11 `payoff_matrix`

承诺、伏笔、结局兑现矩阵。

```json
[
  {
    "promise": "主角会理解力量真正代价",
    "introduced_chapter": null,
    "payoff_target": "结局前",
    "payoff_chapter": null,
    "status": "open",
    "evidence": []
  }
]
```

状态：

```text
open
seeded
paid_off
dropped
blocked
```

### 5.12 `narration_contract`

叙事视角、文风和节奏。

```json
{
  "pov": "有限第三人称，主要贴近主角",
  "tense": "中文过去时/自然叙述",
  "narrative_distance": "贴近感官和选择，不做上帝视角剧透",
  "dialogue_style": "短句推进冲突，避免解释腔",
  "description_density": "关键场景细描，过渡场景简写",
  "chapter_pacing": "每章至少有目标、冲突、选择、代价和钩子",
  "forbidden_style_drift": ["英文括注", "工具说明", "提纲式正文", "突然网文外吐槽"]
}
```

### 5.13 `chapter_execution_contract`

每章执行合同。它不是全书字段，而是每章字段模板。

```json
{
  "chapter_number": 1,
  "scene_goal": "本章要完成的具体剧情目标",
  "conflict": "本章核心冲突或阻力",
  "choice": "角色在本章必须做出的选择",
  "cost": "本章选择带来的代价",
  "reveal": "本章新增揭示的信息",
  "emotional_beat": "本章情绪变化",
  "relationship_delta": "本章关系变化",
  "power_delta": "本章能力、职业、地位或认知变化",
  "resource_delta": "本章资源、物品、证据或机会变化",
  "hook_opened": ["本章新打开的伏笔或问题"],
  "hook_paid_off": [],
  "character_change": "本章角色状态变化",
  "world_change": "本章世界、制度、关系或局势变化",
  "payoff_target": "本章服务于哪个长期承诺或结局兑现"
}
```

## 6. 数据落点

### 6.1 `creation_contract.rs`

计划给 `SessionCreationDraftState` 增加：

```text
field_requirements
resource_economy
emotional_contract
relationship_ledger
power_progression
social_order
geography_model
time_model
artifact_ledger
antagonist_pressure
payoff_matrix
narration_contract
```

注意：

- 不把 `chapter_execution_contract` 直接作为草案里的固定单章字段。
- 草案层可保存 `chapter_execution_defaults` 或 `chapter_execution_policy`。
- 用户可见草案只展示摘要。

### 6.2 `novel_studio.rs`

计划给 `NovelCreationDraft`、`StoryContract`、`NovelProjectManifest` 增加同样字段。

当前实现采用内聚结构保存：

```text
structured_contract_v2: NovelContractV2
```

工具参数层仍暴露 12 类字段，便于 LLM/worker 用结构化 JSON 写入；项目文件中集中保存，避免 manifest 顶层继续横向膨胀。

计划给 `ChapterContractRecord` 增加：

```text
scene_goal
conflict
choice
cost
reveal
emotional_beat
relationship_delta
power_delta
resource_delta
hook_opened
hook_paid_off
character_change
world_change
payoff_target
```

### 6.3 `novel_bible.rs`

计划给 `StoryBible` 增加长期状态：

```text
structured_contract_v2
```

章节批准后才更新这些字段。

### 6.4 `novel_workflow_driver.rs`

项目初始化 JSON 从当前字段扩展为 v2 字段。

当前必需字段类似：

```text
title, language, genre, brief, premise, ending_direction,
protagonist_arc, world_imagery, main_causal_spine,
title_rationale, themes, characters, world_rules,
style_rules, must_avoid, outline, chapter_unit_target
```

升级后要求：

```text
title, language, genre, brief, premise, ending_direction,
protagonist_arc, world_imagery, main_causal_spine,
title_rationale, themes, characters, world_rules,
style_rules, must_avoid, outline, chapter_unit_target,
field_requirements, resource_economy, emotional_contract,
relationship_ledger, power_progression, social_order,
geography_model, time_model, artifact_ledger,
antagonist_pressure, payoff_matrix, narration_contract
```

但要区分：

- 第一层字段缺失: blocking
- 第二层字段缺失: warning 或自动轻量补齐
- 第三层字段缺失: 根据题材强度判断

### 6.5 面板

worker 装备 `writing` 后，工具配置界面应展示摘要配置：

- 每章字数档位: 2500 / 5000
- 每轮章节数
- 质量模式
- 导出格式
- 是否只导出已批准章节

合同草案对话中展示：

```text
书名
题材
总字数 / 每章字数 / 预计章节
故事前提
结局方向
主角弧线
世界核心规则
情感承诺
资源/力量体系摘要
主要关系
社会/地点摘要
分卷方向
是否开始写
```

不展示完整 12 类 JSON，除非用户明确说“显示完整合同”。

## 7. 上下文包策略

写每章时不注入完整合同全文，而是生成当前章上下文包：

```text
全书合同摘要
当前卷合同
当前章执行合同
角色权威表
当前关系状态
当前情绪状态
当前资源/力量状态
当前对手压力
未兑现 payoff
最近 2-3 章摘要
必要的时间/地点/物品状态
```

禁止注入：

- 全部正文章节
- 未批准草稿
- 大量旧失败合同
- 长篇工具日志
- 完整 12 类字段原始 JSON

## 8. 质量门升级

### 8.1 合同质量门

合同阶段只检查能否开书：

- 书名存在，且不是临时占位。
- 总字数、每章档位、语言、题材有效。
- 结局方向、主线因果、世界意象存在。
- 第一层字段存在。
- 第二层和第三层按题材给 warning 或 blocking。

合同阶段不要再因为章节标题不够好而 blocking。

### 8.2 章节质量门

章节质量门分层：

```text
Blocker
Metadata Repair
Warning
Accepted
```

Blocker：

- 正文缺失。
- 乱码、外文残片、工具字段污染。
- 主角名字漂移。
- 章节明显没写完。
- 严重重复。
- truth 会污染后续。

Metadata Repair：

- 标题不够好。
- 摘要不准确。
- key facts 不完整。
- continuity updates 漏写。
- 分卷名或章节名泛化。

Warning：

- 节奏略慢。
- 标题意象一般。
- 局部错字。
- 描写重复。

Accepted：

- 可批准，进入 story bible / truth / exports。

### 8.3 v2 字段专项检查

新增检查：

- 资源/货币不能突然改变。
- 力量升级必须有代价。
- 人物关系变化必须有事件证据。
- 情绪转折必须有铺垫。
- 关键物品归属不能漂移。
- 时间线不能倒错。
- 地点移动不能无成本。
- 对手压力不能突然消失。
- payoff_matrix 里的承诺必须逐步推进或明确保留。
- 每章至少推进一个有效维度：剧情、人物、情绪、关系、世界、力量、资源、伏笔、对手压力。

## 9. 迁移策略

旧项目不能崩。

### 9.1 manifest 迁移

如果旧 `project.json` 没有 v2 字段：

- 初始化空结构。
- 从旧 `world_rules` 推断 `resource_economy`、`power_progression`、`social_order`。
- 从旧 `characters` 推断 `relationship_ledger`。
- 从旧 `outline` 和 `ending_direction` 推断 `payoff_matrix`。
- 推断不了就保持空结构并标记 `needs_enrichment`。

### 9.2 story bible 迁移

如果旧 `story_bible.json` 没有 v2 字段：

- 保留旧字段。
- 补默认 v2 空结构。
- 下一次 `repair_project_state` 或 `run_next_chapter` 前允许 worker 生成 enrichment patch。

### 9.3 章节迁移

旧章节没有细化 `chapter_execution_contract` 时：

- 从 `summary`、`key_facts`、`continuity_updates` 推断轻量字段。
- 不重写正文。
- 不因旧章节缺字段阻止读取、导出或续写。

## 10. 分阶段实施

### Phase 1: Schema 与兼容默认值

- [x] 新增 v2 类型。
- [x] 加入 serde 默认。
- [x] 旧项目读取不崩。
- [x] 不改现有行为。

验证：

- `cargo test -p benshu-builtin-tools writing --lib`
- 旧项目 `status/export/read_truth` 正常。

### Phase 2: 合同生成与吸收

- [x] `creation_contract.rs` 吸收 v2 字段。
- [x] `novel_workflow_driver.rs` 初设 JSON 输出 v2 字段。
- [x] 面板可见草案只展示摘要。

验证：

- 真实面板: “帮我写小说”只追问。
- 真实面板: 补题材后返回可见草案。
- 完整合同不因章节标题问题 blocked。

### Phase 3: 项目初始化与 story bible 持久化

- [x] `approve_draft` 写入正式项目。
- [x] `StoryBible` 写入 v2 长期状态。
- [x] `status` 能显示 v2 摘要。

验证：

- 真实面板确认“开始写第一章”后创建项目。
- `project.json` 和 `story_bible.json` 有 v2 字段。

### Phase 4: 每章执行合同升级

- [x] `ChapterContractRecord` 增加细化字段。
- [x] `run_next_chapter`/章节计划链路可保存当前章执行合同字段。
- [x] `compose_context` 通过 story bible prompt view 注入摘要化 v2 字段，不注入正文。

验证：

- 第一章能写。
- 聊天框只返回进度、路径、摘要。
- 章节文件不把合同 JSON 写进正文。

### Phase 5: approve 后状态更新

- [x] 审稿通过不等于批准。
- [x] 只有 approve 后更新 story bible 权威状态。
- [x] 未批准草稿不能污染后续上下文。
- [x] approve 后按章节摘要、key facts、continuity updates 增量更新 v2 关系、情绪、力量、资源、物品、对手压力和 payoff 证据。

验证：

- 人为制造 needs_revision，确认 story bible 不更新。
- approve 后 v2 状态更新。
- Phase 8 继续用真实长篇校准增量质量。

### Phase 6: 质量门和 metadata repair

- [x] 标题问题只走 metadata repair，不因标题争议重写正文。
- [x] v2 字段专项检查先接入 warning，避免合同阶段再次挡住正文。
- [x] 同类元数据问题不触发无限重写正文。

验证：

- 标题不好不重写正文。
- 资源/人物/情绪漂移能被发现。

### Phase 7: 面板与导出

- [x] 面板展示合同摘要。
- [x] 导出 TXT/MD 插入卷结构、章节标题。
- [x] 用户明确要求时可显示完整合同或摘要。

验证：

- Windows 可直接打开 TXT。
- 面板不会因为长正文撑爆聊天历史。
- “合同摘要/完整合同”能返回结构化合同摘要或完整视图。

### Phase 8: 真实长篇回归

真实面板测试：

- 都市玄幻，5 万字，每章 2500，至少写完 5 章。
- 异界玄幻，5 万字，每章 2500，写完整短篇。
- 言情短篇，2-3 章，验证情感合同。
- 科幻短篇，验证资源/技术/地理字段。
- 悬疑短篇，验证 artifact_ledger / payoff_matrix。

必须检查：

- 书名是否来自结局、主线和世界意象。
- 主角名是否稳定。
- 章节标题是否由正文或章节目标支撑。
- 情感线是否推进。
- 资源/力量是否不乱跳。
- 结尾是否兑现 payoff。

## 11. 风险

### 11.1 合同过重导致慢

风险：

- 初设 JSON 变长。
- 本地模型生成合同更慢。

缓解：

- 面板前台先展示摘要。
- 完整合同后台生成。
- 第二层/第三层允许轻量默认。
- 写章节时按需抽取，不全量注入。

### 11.2 字段过多导致 LLM 填空作文

风险：

- LLM 为了填字段而生成空泛内容。

缓解：

- 第一层强制具体。
- 第二层允许简短。
- 第三层按题材启用。
- 质量门只要求可用，不要求华丽。

### 11.3 误把题材策略写死

风险：

- 玄幻、言情、科幻被同一套词污染。

缓解：

- 通过 `field_requirements` 管强弱。
- 字段语义通用，内容由 LLM 根据用户题材生成。
- 代码只治理结构，不写固定剧情。

### 11.4 旧项目迁移污染

风险：

- 从旧 `outline` 推断错误。

缓解：

- 推断字段标记 `inferred`。
- 不把 inferred 当成用户指定。
- 允许后续 enrichment patch。

### 11.5 质量门再次过严

风险：

- 还没写正文就 blocked。

缓解：

- 合同门只管能否开书。
- 标题、摘要、key facts 等元数据问题走 metadata repair。
- 正文是最高价值产物，不能因为元数据问题重写正文。

## 12. 完成口径

本升级完成必须同时满足：

- v2 字段进入 draft / manifest / story bible。
- 旧项目读取和导出不崩。
- 面板草案可见但不臃肿。
- “按这个开始 / 开始写第一章”稳定进入写作。
- 合同阶段不再因章节标题 blocked。
- 每章执行合同包含细化字段。
- approve 后才更新 v2 权威状态。
- 真实面板至少完成 5 章小说回归。

## 13. 推荐优先级

优先实现顺序：

1. 合同质量门降级，先解决“合同挡住正文”。
2. 确认语路由修复，先解决“按这个开始又 planning”。
3. v2 schema 和默认值。
4. 合同生成/吸收 v2 字段。
5. story bible 持久化。
6. 每章执行合同细化。
7. v2 状态更新和质量门。
8. 面板摘要展示。
9. 长篇真实回归。

这条顺序能先恢复可写能力，再增强质量治理，避免继续把质量门写成正文生成的阻塞器。
