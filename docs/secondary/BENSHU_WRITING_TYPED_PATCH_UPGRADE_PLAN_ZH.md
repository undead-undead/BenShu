# BenShu 写作合同 typed patch / 字段补丁升级方案

## 目标

把现有“分段合同 JSON”升级为“字段级 typed patch 合并机制”。

这不是推翻当前写作工具，而是在现有底座上补一层更稳的边界：

- 保留现有 `Skeleton / Characters / Plot / Governance` 分段。
- 保留现有题材分类：`FictionGenreProfile`、`default_field_requirements`、`genre_governance_profile`。
- 保留现有强类型合同：`NovelCreationContract`、`NovelContractV2`。
- 保留现有质量门：`typed_contract_gate`、命名质量门、合同生命周期。
- 新增字段级 patch 边界，让本地模型即使不稳定输出完整 JSON，也能可靠补齐合同字段。

核心原则：

```text
LLM 负责创意字段。
Rust 负责状态、类型、合并、质量门和权威合同。
```

## 当前已有底座

### 1. 分段合同

已有文件：

- `crates/builtin-tools/src/tool/writing/creation_contract/staged_prompts.rs`

当前阶段：

- `Skeleton`：书名、前提、终局、主角弧线、世界观意象、总主线因果链。
- `Characters`：角色权威表、关系线。
- `Plot`：分卷/阶段、近期章节包、伏笔/兑现矩阵。
- `Governance`：主题、世界规则、风格、必须避免、治理字段。

当前不足：

- 每段仍偏向“输出 JSON 块”。
- 如果本地模型输出自然语言、Markdown、半截 JSON，整段可能失败。
- 字段写入边界不够细，坏字段可能拖累整段。

### 2. 题材分类

已有文件：

- `crates/builtin-tools/src/tool/writing/longform_policy.rs`
- `crates/builtin-tools/src/tool/writing/novel_contract_v2.rs`
- `crates/builtin-tools/src/tool/writing/novel_bible.rs`

已有能力：

- `FictionGenreProfile`
  - `Fantasy`
  - `Xianxia`
  - `ScienceFiction`
  - `Romance`
  - `General`
- `default_field_requirements(genre)`
  - 根据题材决定 `power_progression`、`artifact_ledger` 等字段强度。
- `genre_governance_profile(genre, language)`
  - 根据题材生成治理轴，例如玄幻防力量膨胀、言情控制关系温度、科幻控制技术边界。

当前修正方向：

- typed patch 必须复用这些分类。
- 不能新建另一套题材判断。
- 不能把玄幻、言情、科幻字段全部硬塞给所有小说。

### 3. 强类型合同

已有文件：

- `crates/builtin-tools/src/tool/writing/creation_contract_model.rs`
- `crates/builtin-tools/src/tool/writing/novel_contract_v2.rs`

已有结构：

- `NovelCreationContract`
- `TitleContract`
- `CharacterContract`
- `EndingContract`
- `OutlineContract`
- `NovelContractV2`

typed patch 的最终合并目标必须仍然是这些结构。

### 4. 合同质量门

已有文件：

- `crates/builtin-tools/src/tool/writing/typed_contract_gate.rs`
- `crates/builtin-tools/src/tool/writing/naming/*`
- `crates/builtin-tools/src/tool/writing/creation_contract/contract_candidate.rs`

已有职责：

- `typed_contract_gate` 判断强类型合同是否可确认。
- `naming` 判断书名、章节名、角色名质量。
- `contract_candidate` 防止坏合同污染可确认草案。

typed patch 不应该绕过这些质量门。

## 用户体验不变

用户不需要知道 typed patch。

用户看到的仍然是：

```text
当前小说合同草案
可自然语言修改
通过质量门后可确认
确认后开始写正文
```

内部可以多次生成 patch，但面板应展示合并后的“当前合同视图”，而不是把多个 patch 原样摊给用户。

## typed patch 是什么

typed patch 是一个“字段补丁对象”。

它不是完整合同，而是对合同中某一类字段的有边界更新。

示例：

```json
{
  "patch_type": "title_patch",
  "title": {
    "canonical_title": "雾城燃星",
    "candidates": ["雾城燃星", "尘火入城", "夜校星骨"],
    "rationale": "书名来自终局中主角在被压制的都市灵能秩序里点燃公开反抗，也对应世界观里的雾城和星核意象。"
  }
}
```

这个 patch 只能改书名字段，不能偷偷改角色、题材、结局。

## 为什么需要 typed patch

当前分段已经降低了模型难度，但仍有一个问题：

本地模型不一定稳定输出标准 JSON。

特别是 Qwen/Gemma 这类本地模型可能输出：

- 自然语言合同。
- Markdown 包裹的 JSON。
- 半截 JSON。
- 字段名变形。
- JSON 前后混解释文字。
- 某一段内容完整，但整体不是合法 JSON。

如果仍要求完整 JSON 块，就容易出现：

```text
合同输出不能解析为 JSON
```

typed patch 的意义是：

- 每次只补少量字段。
- 字段级解析。
- 字段级校验。
- 字段级合并。
- 坏 patch 只影响当前字段，不污染整份合同。

## 总体数据流

```text
用户自然语言
  ↓
SessionCreationDraftState 当前草案
  ↓
ContractValidationReport 找缺口
  ↓
select_contract_completion_stage 选择阶段
  ↓
select_patch_request 选择字段补丁类型
  ↓
LLM 生成 patch 文本
  ↓
normalize_patch_boundary 尽力解析字段补丁
  ↓
TypedPatch::validate_scope 校验 patch 只能改允许字段
  ↓
apply_patch_to_draft 合并到 SessionCreationDraftState
  ↓
NovelCreationContract::from_draft / current_contract
  ↓
typed_contract_gate 整体质量门
  ↓
面板展示合并后的合同
```

## Patch 类型设计

### 1. `title_patch`

作用：

- 补或修书名。
- 补书名候选。
- 补书名理由。

允许字段：

- `title.canonical_title`
- `title.candidates`
- `title.rationale`
- `title.source`

必须满足：

- 书名能从结局、大纲、世界观、主角弧线或关键爽点解释出来。
- 书名理由必须具体。
- 不能只是抽象概念拼接。
- 不能污染角色名、题材、正文。

不允许：

- 改角色表。
- 改结局。
- 改章节规划。

### 2. `skeleton_patch`

作用：

- 补合同骨架。

允许字段：

- `genre`
- `brief`
- `premise`
- `ending`
- `protagonist_arc`
- `world_imagery`
- `main_causal_spine`
- `target_units`
- `chapter_unit_target`
- `max_chapters_per_turn`

必须满足：

- 如果用户已指定题材、字数、每章档位，不能改。
- 终局必须能反推主线。
- 主线必须能解释世界观意象。

### 3. `character_patch`

作用：

- 补角色权威表。
- 补关系线。
- 修角色名漂移。

允许字段：

- `characters`
- `structured.relationship_ledger`
- `structured.emotional_state_ledger`

必须满足：

- 恰好一个主角槽位。
- 至少一个非主角关键角色。
- 关键角色必须有欲望、恐惧、底线、弧线起点、弧线终点。
- 关系线引用的人物必须存在于角色权威表。

不允许：

- 在角色 patch 中改书名、题材、总字数。

### 4. `plot_patch`

作用：

- 补分卷/阶段规划。
- 补近期章节目标。
- 补伏笔/兑现矩阵。

允许字段：

- `outline.volumes`
- `outline.near_chapters`
- `outline.raw_outline`
- `structured.payoff_matrix`

必须满足：

- 每个近期章节必须有具体事件目标。
- 每个近期章节必须有不可逆变化。
- 分卷目标必须服务于终局。
- 伏笔必须有兑现方向。

不允许：

- 生成正文。
- 改角色名。

### 5. `governance_patch`

作用：

- 补题材治理字段。

允许字段：

- `themes`
- `world_rules`
- `style_rules`
- `must_avoid`
- `structured.emotional_contract`
- `structured.relationship_ledger`
- `structured.antagonist_pressure`
- `structured.narration_contract`
- 按题材选择的扩展字段。

题材扩展字段：

玄幻/仙侠：

- `structured.resource_economy`
- `structured.power_progression`
- `structured.social_order`
- `structured.geography_model`

科幻：

- `structured.resource_economy`
- `structured.power_progression`
- `structured.social_order`
- `structured.time_model`
- `structured.geography_model`

言情：

- `structured.emotional_contract`
- `structured.relationship_ledger`
- `structured.social_order`
- `structured.time_model`

泛类型：

- 只要求通用治理字段。
- 类型专属字段只有在题材自然需要时才填。

必须复用：

- `longform_policy::fiction_genre_profile`
- `novel_contract_v2::default_field_requirements`

### 6. `metadata_patch`

作用：

- 修已经接近 ready 的合同元数据。

允许字段：

- 书名理由。
- 章节标题计划。
- 卷名。
- 摘要。
- 缺失但可从已存在合同推导的小字段。

不允许：

- 重写正文。
- 重写整份合同。
- 大幅改变用户指定方向。

## 不同类型小说如何复用 typed patch

typed patch 不应该为每一种小说复制一套新系统。

错误方向：

```text
FantasyTitlePatch
RomanceTitlePatch
ScienceFictionTitlePatch
FantasyCharacterPatch
RomanceCharacterPatch
...
```

这样会导致代码膨胀，而且以后新增题材又要新增一套 patch。

正确方向：

```text
TitlePatch
SkeletonPatch
CharacterPatch
PlotPatch
GovernancePatch
MetadataPatch
  +
GenrePatchProfile
```

也就是说：

- patch 类型是通用的。
- 题材 profile 决定字段强度、字段解释和扩展字段。
- 同一个 `GovernancePatch` 根据题材生成不同字段要求。
- 同一个 `CharacterPatch` 根据题材生成不同角色锚点侧重。
- 同一个 `TitlePatch` 根据题材调整命名质量门，但仍必须来自剧情、终局、世界观或爽点。

## GenrePatchProfile

建议新增一个轻量结构，但它必须由现有分类生成，不能新建分类体系。

```rust
struct GenrePatchProfile {
    genre_profile: FictionGenreProfile,
    required_patch_fields: BTreeMap<String, PatchFieldStrength>,
    prompt_hints: Vec<String>,
    quality_axes: Vec<String>,
}
```

来源：

```text
FictionGenreProfile
  + default_field_requirements(genre)
  + genre_governance_profile(genre, language)
  -> GenrePatchProfile
```

它只做“patch 字段选择器”，不负责生成小说内容。

## 通用 patch 如何按题材变化

### `title_patch`

所有小说都复用同一个 `TitlePatch`。

通用要求：

- 书名必须能从终局、大纲、世界观、主角弧线或关键爽点解释出来。
- 书名理由必须具体。
- 不能只是抽象概念堆叠。

题材差异：

玄幻/仙侠：

- 书名可引用力量代价、宗门、天命、灵脉、禁地等当前合同独有意象。
- 不能只叫“尘某”“某劫”“某归墟”这类泛化古风词。

科幻：

- 书名可引用技术边界、星际尺度、文明冲突、意识、能源、航行约束等独有意象。
- 不能只是“静默”“余温”“协议”这种抽象概念总结。

言情：

- 书名可引用关系转折、情绪承诺、共同选择、现实压力或人物关系中的独有意象。
- 不强求宏大世界观名词。

悬疑/推理：

- 书名可引用核心谜面、关键物证、反转意象。
- 不能提前剧透最终真相。

### `skeleton_patch`

所有小说都复用同一个 `SkeletonPatch`。

通用字段：

- 题材
- 简述
- 故事前提
- 终局方向
- 终局状态
- 主角弧线
- 世界观意象
- 总主线因果链
- 总字数
- 每章档位

题材差异：

玄幻/仙侠：

- `world_imagery` 应包含力量秩序、资源代价或世界规则意象。
- `main_causal_spine` 应能解释主角如何一步步获得力量，但不能无代价膨胀。

科幻：

- `world_imagery` 应包含技术边界、空间尺度、制度或文明冲突。
- `main_causal_spine` 应说明技术、认知或组织变化如何推进冲突。

言情：

- `world_imagery` 可以是现实环境、职业/家庭/城市情绪，不必硬塞奇观。
- `main_causal_spine` 应说明关系如何因选择、误解、信任和现实压力变化。

### `character_patch`

所有小说都复用同一个 `CharacterPatch`。

通用字段：

- 角色名
- 角色身份
- 欲望
- 恐惧
- 底线
- 弧线起点
- 弧线终点
- 关系线

题材差异：

玄幻/仙侠：

- 主角锚点要包含力量、资源或阶层压力。
- 对手压力源要能约束成长，不是只负责被打败。

科幻：

- 主角锚点可包含技术伦理、身份权限、文明立场或认知边界。
- 对手可以是机构、系统、文明、AI、资本或人。

言情：

- 主角锚点更重情感欲望、关系恐惧、现实底线。
- 对手/压力源不一定是反派，可以是误解、家庭、职业、距离、价值冲突。

### `plot_patch`

所有小说都复用同一个 `PlotPatch`。

通用字段：

- 分卷/阶段
- 近期章节目标
- 不可逆变化
- 伏笔/兑现矩阵

题材差异：

玄幻/仙侠：

- 每个阶段要控制成长阶梯和代价。
- 卷尾不可逆变化不能只是“变强”，要改变关系、资源、身份或敌我格局。

科幻：

- 每个阶段要推进技术边界、文明冲突或认知突破。
- 卷尾变化要影响系统、组织、航线、能源、身份或世界规则。

言情：

- 每个阶段要推进关系状态。
- 卷尾变化要改变信任、选择、距离、承诺或现实压力。

### `governance_patch`

`GovernancePatch` 是题材差异最大的 patch，但仍然是同一个 patch 类型。

通用字段永远存在：

- 主题
- 世界规则
- 叙事风格
- 必须避免
- 情感承诺
- 关系线
- 叙事视角
- 伏笔/承诺兑现
- 反派/压力源

题材扩展由 `GenrePatchProfile` 决定。

玄幻/仙侠扩展：

- `resource_economy`
- `power_progression`
- `social_order`
- `geography_model`

科幻扩展：

- `resource_economy`
- `power_progression`
- `social_order`
- `time_model`
- `geography_model`

言情扩展：

- `emotional_contract`
- `relationship_ledger`
- `social_order`
- `time_model`

悬疑/推理扩展：

- `artifact_ledger`
- `payoff_matrix`
- `time_model`
- `social_order`

泛类型扩展：

- 只补通用字段。
- 如果用户题材自然需要，再由 `default_field_requirements` 提升某些字段强度。

## 字段强度而不是硬必填

不同类型小说不应该把字段简单分成“有/没有”。

应该用字段强度：

```rust
enum PatchFieldStrength {
    Required,
    Strong,
    Default,
    Optional,
    Disabled,
}
```

含义：

- `Required`：没有就不能进入可确认合同。
- `Strong`：强建议，缺失会触发补齐，但必要时可降为 blocker 说明。
- `Default`：正常补齐，缺失不一定阻止合同。
- `Optional`：只有用户要求或已有内容需要时补。
- `Disabled`：当前题材不应强行要求。

示例：

玄幻：

```text
power_progression = Required/Strong
resource_economy = Strong
relationship_ledger = Default
artifact_ledger = Optional
```

言情：

```text
relationship_ledger = Required/Strong
emotional_contract = Required/Strong
power_progression = Disabled/Optional
resource_economy = Optional
```

科幻：

```text
world_rules = Required
resource_economy = Strong
power_progression = Strong, 但解释为技术/权限/认知进阶，不是修炼等级
time_model = Default/Strong
```

这样可以避免两种错误：

- 所有小说都套玄幻升级体系。
- 所有小说都只剩通用空字段，失去题材控制力。

## 题材混合时如何处理

用户可能说：

```text
都市玄幻
赛博朋克玄幻
科幻言情
异世界重生玄幻
校园悬疑言情
```

不要只选一个分类然后丢掉其他维度。

建议策略：

1. 选择一个主 profile。
2. 保留辅助 profile 的强字段。
3. 冲突字段按用户重点和故事前提裁决。

示例：

都市玄幻：

- 主 profile：Fantasy。
- 辅助维度：都市现实秩序。
- 必须有 `power_progression`。
- 也要有 `social_order`，但解释为城市机构、学校、公司、官方组织等。

赛博朋克玄幻：

- 主 profile：Fantasy 或 ScienceFiction，由用户重点决定。
- 如果强调灵能/修炼，主 profile 是 Fantasy。
- 如果强调技术/网络/义体，主 profile 是 ScienceFiction。
- 两者都需要防膨胀，但一个防“力量万能”，一个防“技术万能”。

科幻言情：

- 主 profile 可能是 ScienceFiction。
- `relationship_ledger` 和 `emotional_contract` 提升到 Strong。
- 不让感情线被世界观设定吞掉。

## 代码复用方式

建议新增函数：

```rust
fn genre_patch_profile(
    user_message: &str,
    draft: &SessionCreationDraftState,
) -> GenrePatchProfile
```

内部只调用现有分类：

```rust
let profile = longform_policy::fiction_genre_profile(user_message, Some(&draft.genre));
let requirements = novel_contract_v2::default_field_requirements(&draft.genre);
let governance = novel_bible::genre_governance_profile(&draft.genre, &draft.language);
```

如果 `genre_governance_profile` 当前不是 pub，可以先只复用 `FictionGenreProfile` 和 `default_field_requirements`，不要为了调用它破坏模块边界。

## 质量门如何按题材变化

`typed_contract_gate` 不应该写一堆题材硬编码。

它应该问 `GenrePatchProfile`：

```text
这个字段是不是 Required？
这个字段是不是 Strong？
这个字段为空是 blocker、repair 还是 warning？
```

例如：

- 玄幻缺 `power_progression`：blocker 或 strong repair。
- 言情缺 `power_progression`：不阻塞。
- 言情缺关系线：blocker。
- 科幻缺技术/资源边界：blocker 或 strong repair。

这样质量门仍然是通用的，只是字段强度来自题材 profile。

## Prompt 如何按题材变化

Prompt 不要写死“玄幻必须……”到全局。

应由 `GenrePatchProfile` 渲染：

```text
当前题材 profile：Fantasy
本 patch 必须补强字段：
- power_progression: Strong
- resource_economy: Strong
- social_order: Default

本 patch 不应强行补：
- artifact_ledger
```

这能让模型知道当前字段重点，也能让 Rust 侧知道如何验收。

## Patch 结构建议

Rust 侧新增枚举：

```rust
enum CreationContractPatch {
    Title(TitlePatch),
    Skeleton(SkeletonPatch),
    Characters(CharacterPatch),
    Plot(PlotPatch),
    Governance(GovernancePatch),
    Metadata(MetadataPatch),
}
```

每个 patch 有三个方法：

```rust
impl CreationContractPatch {
    fn patch_type(&self) -> CreationContractPatchType;
    fn validate_scope(&self, draft: &SessionCreationDraftState) -> PatchValidationReport;
    fn apply_to_draft(self, draft: &mut SessionCreationDraftState) -> PatchApplyReport;
}
```

不要让 patch 直接写 `current_contract`。

正确顺序：

```text
patch -> draft fields -> normalize draft -> build typed contract -> typed gate -> current_contract
```

## Patch 边界解析

新增模块建议：

```text
crates/builtin-tools/src/tool/writing/creation_contract/patch.rs
crates/builtin-tools/src/tool/writing/creation_contract/patch_normalizer.rs
crates/builtin-tools/src/tool/writing/creation_contract/patch_prompt.rs
```

职责：

- `patch.rs`
  - patch 类型定义。
  - patch 作用域校验。
  - patch 应用。
- `patch_normalizer.rs`
  - 从模型输出中提取 patch。
  - 容忍 JSON 外壳漂移。
  - 容忍自然语言字段包。
- `patch_prompt.rs`
  - 生成每类 patch 的 prompt。
  - 复用现有 staged prompt 选择逻辑。

注意：

- 不要把这些逻辑放进 gateway。
- 不要放进 runtime-policy-core。
- 这是 writing 工具自己的合同治理。

## 自然语言字段包兼容

typed patch 不等于只能 JSON。

为了兼容本地模型，patch normalizer 应该支持三层输入：

### 第一层：标准 JSON patch

优先解析。

### 第二层：JSON-ish patch

复用现有 `creation_contract_normalizer` 的经验，修复常见字段名漂移。

### 第三层：自然语言字段包

例如模型输出：

```text
书名：雾城燃星
理由：来自终局里主角点燃雾城星核，也对应都市玄幻世界观。
```

如果当前请求是 `title_patch`，可以提取为：

```rust
TitlePatch {
    canonical_title: "雾城燃星",
    rationale: "...",
}
```

但自然语言字段包必须受 patch 作用域限制。

## 合并规则

### 用户指定优先

如果字段来自用户明确指定：

- 题材
- 书名
- 总字数
- 每章档位
- 语言
- 禁止事项

patch 不能覆盖，除非用户明确要求修改。

### 已通过字段不轻易覆盖

如果字段已通过质量门：

- patch 不能重写。
- 只能补空字段或修 metadata。

### 候选不等于权威

未通过 gate 的 patch：

- 可以进入 `pending_contract_candidate` 或 patch diagnostics。
- 不能进入 `current_contract`。
- 不能进入可确认草案。

### 整体 gate 仍是最终裁判

字段 patch 通过后，仍必须重新跑：

- `NovelCreationContract::normalize`
- `typed_contract_gate`
- naming gate
- lifecycle readiness

## 与现有分段的关系

当前分段不删除。

关系应该是：

```text
Skeleton stage -> skeleton_patch/title_patch
Characters stage -> character_patch
Plot stage -> plot_patch
Governance stage -> governance_patch
Metadata repair -> metadata_patch
```

分段负责“下一步该补什么”。

typed patch 负责“这一小步到底能写哪些字段”。

## 与现有合同展示的关系

面板仍展示合并后的合同。

不展示：

- patch 原始 JSON。
- patch 内部诊断。
- 多段机器输出。

展示：

- 当前标准小说合同草案。
- 可确认 / 不可确认状态。
- 缺失或阻塞原因。
- 用户可以自然语言修改。

## 阶段计划

### Phase 1：补 patch 类型和作用域

新增：

- `creation_contract/patch.rs`

内容：

- `CreationContractPatch`
- `CreationContractPatchType`
- 各 patch struct
- `validate_scope`
- `apply_to_draft`

只做结构，不接 LLM。

验证：

- 单元测试：title patch 不能改角色。
- 单元测试：character patch 不能改书名。
- 单元测试：用户指定字段不被覆盖。

### Phase 2：接入 patch normalizer

新增：

- `creation_contract/patch_normalizer.rs`

支持：

- 标准 JSON patch。
- JSON-ish patch。
- 自然语言字段包。

验证：

- Qwen 风格自然语言书名包能提取 title patch。
- Markdown 包裹 JSON 能解析。
- 半截合同不能污染 draft。

### Phase 3：改 staged prompt 输出 patch

修改：

- `creation_contract/staged_prompts.rs`

从“输出一段合同 JSON”改成“输出当前阶段 patch”。

注意：

- Governance patch 必须复用现有 genre profile。
- 不要新建分类。

验证：

- 玄幻 Governance prompt 包含成长/资源/防膨胀。
- 言情 Governance prompt 不强制力量等级。
- 科幻 Governance prompt 不强制修炼层级。

### Phase 4：patch 合并进入候选流程

修改：

- `creation_contract/contract_candidate.rs`
- `creation_contract/repair_coordinator.rs`

流程：

```text
raw output -> patch normalizer -> apply patch to clone draft -> build typed contract -> gate -> commit/pending
```

验证：

- 好 patch 合并。
- 坏 patch 只留下诊断。
- patch 不通过时不会把 draft 变成半成品。

### Phase 5：保留完整合同 JSON 兼容路径

不能一次删掉完整 JSON 合同路径。

兼容规则：

- 如果模型输出完整 `NovelCreationContract`，仍可走现有路径。
- 如果完整合同失败，再尝试 patch 解析。
- 如果当前 prompt 明确是 patch，优先 patch 解析。

这样降低迁移风险。

### Phase 6：面板合同展示核对

核对：

- `session_surface.rs`
- `creation_contract/surface.rs`

要求：

- 展示合并后的合同。
- 不展示 patch 内部 JSON。
- 不把 pending patch 当 ready 合同。
- 不把 blocked task 显示成 completed。

### Phase 7：真实模型回归

测试矩阵：

- Gemma 本地模型。
- Qwen 本地模型。
- 轻量开场：`帮我写小说`
- 具体需求：`写都市玄幻小说，每章2500字，至少5万字起`
- 修改合同：`书名更有吸引力`
- 修改角色：`主角换成女性`
- 确认写作：`按这个开始，写第一章`

通过标准：

- 合同能合并展示。
- 未通过质量门不能确认。
- 通过后能写第一章。
- 第一章不混 JSON 残片。
- 角色名遵守权威表。

## 风险

### 风险 1：代码再膨胀

缓解：

- patch 模块只负责字段补丁。
- 不把命名质量、合同展示、写作正文逻辑塞进 patch。

### 风险 2：patch 和完整合同路径重复

缓解：

- 完整合同路径保留为兼容。
- 新逻辑优先用于 staged prompt。
- 后续稳定后再考虑清理旧自然语言合同解析。

### 风险 3：字段 patch 太细导致多次调用变慢

缓解：

- 不必每个字段一次调用。
- 按阶段聚合 patch。
- 优先改最容易失败的 `Skeleton`、`Characters`。

### 风险 4：上下文连续性丢失

缓解：

- 每次 patch prompt 都带当前权威合同摘要。
- patch 只能补当前字段。
- 合并后跑整体质量门。

## 最小可行实现顺序

建议不要一次把所有阶段改完。

优先顺序：

1. `title_patch`
2. `skeleton_patch`
3. `character_patch`
4. `plot_patch`
5. `governance_patch`
6. `metadata_patch`

原因：

- 当前最痛的是书名错位、角色漂移、合同无法补齐。
- 这三类问题主要在 title/skeleton/character。
- Plot/Governance 可以继续沿用现有分段 JSON，逐步迁移。

## 完成标准

代码层面：

- 有独立 patch 类型。
- 有独立 patch normalizer。
- staged prompt 可以输出 patch。
- patch 只能写允许字段。
- patch 合并后必须走 typed gate。
- 坏 patch 不污染可确认合同。

产品层面：

- 用户看到的是完整合同草案。
- 用户可以自然语言修改。
- 合同 ready 前不能开始写正文。
- 合同 ready 后“按这个开始”稳定进入写作。

质量层面：

- Qwen 不输出标准完整 JSON 时仍能补齐部分字段。
- 书名必须来自剧情、终局、世界观或爽点。
- 角色权威表不漂移。
- 不同题材不会被套同一批专属字段。

## 当前代码对照状态

本节用于防止把“方案”误认为“已完成实现”。以下状态已按当前代码重新核对。

### 已实现

- 分段合同。
- 题材分类。
- 强类型合同。
- 合同质量门。
- pending/current 隔离。
- 合同展示合并视图；用户看到的是当前合同草案/可确认合同，而不是多段原始 patch。
- 第一阶段 typed patch：`title_patch`、`skeleton_patch`、`character_patch`。
- 第二阶段 typed patch：`plot_patch`、`governance_patch`、`metadata_patch`。
- patch 作用域控制。
- patch normalizer。
- Skeleton/Characters/Plot/Governance 阶段 patch prompt 薄调度。
- `GenrePatchProfile` 复用现有 `FictionGenreProfile` 和 `default_field_requirements`，不新增题材体系。
- `typed_contract_gate` 已读取字段强度；当前只有 `Required` 会硬阻断，`Strong` 用于 prompt/补齐，不破坏既有 ready 合同。
- patch 提交流程为：`raw output -> patch normalizer -> draft clone -> strong contract -> typed gate -> commit/pending`。
- `staged_prompts.rs` 已删除旧 JSON schema fallback，改为 patch prompt 薄调度。
- 阶段选择不再在主流程散落文本 `contains`，改为集中 issue 分类函数。

### 刻意保留的兼容路径

完整 JSON 合同和完整自然语言字段包仍然可以进入原有强类型合同路径。
typed patch 作为 staged prompt 的优先路径和完整合同失败后的 fallback。
后续删除旧解析必须以真实模型回归稳定为前提，不能为了“纯粹”破坏兼容。

### 尚未完成

- Phase 7 的真实模型/真实面板回归尚未执行。
- 旧自然语言完整合同兼容解析尚未删除。
- `genre_governance_profile` 仍未抽成公开 helper；当前 `GenrePatchProfile` 只复用 `fiction_genre_profile` 和 `default_field_requirements`。

### 后续约束

- 不应继续增加提示词规则，也不能回到 `contract_candidate.rs` 继续堆 loose parser。
- 新增字段补丁必须进入 `patch_normalizer.rs`。
- 题材字段选择必须走 `GenrePatchProfile`。
- 写作合同逻辑不得放回 gateway。

## 近期代码审查结论与实施约束

本节补充 2026-06-11 对当前写作工具代码的复核结论。当前代码已经不是
“第一阶段过渡态”，而是 typed patch 主链路已落地、完整合同兼容路径保留
的状态。

### 当前结论

已确认落地：

- 原先集中在 `creation_contract/surface.rs` 的大段合同 prompt 已经迁出。
- `staged_prompts.rs` 只做阶段选择和薄调度，不再保留完整 JSON schema fallback。
- Skeleton / Characters / Plot / Governance 阶段均走 `patch_prompt.rs` 渲染 patch prompt。
- `contract_candidate.rs` 支持显式 patch 优先、完整 JSON / 自然语言完整合同兼容、patch fallback。
- `patch_normalizer.rs` 已支持 `title/skeleton/character/plot/governance/metadata` 六类 patch。
- `GenrePatchProfile` 已复用现有 `fiction_genre_profile` 和 `default_field_requirements`。
- `typed_contract_gate.rs` 已读取 `PatchFieldStrength`；当前 `Required` 才硬阻断，`Strong` 用于 prompt/补齐。
- `pending_contract_candidate/current_contract` 隔离仍然有效，坏合同不会直接进入可确认草案。

保留边界：

- 完整 JSON 合同路径保留，用于兼容较稳定的模型输出。
- 完整自然语言字段包路径保留，用于兼容真实本地模型的非 JSON 输出。
- `contract_candidate.rs` 中旧 field-pack 解析是兼容层，不再作为新增字段补丁扩展点。
- 新增 patch 类型或字段解析必须进入 `patch.rs` / `patch_normalizer.rs` / `patch_prompt.rs`。

仍需谨慎：

- 真实模型/真实面板回归尚未在本次文档复核中执行。
- 彻底删除旧自然语言完整合同解析会提高真实模型回归风险，暂不做。
- `genre_governance_profile` 仍保持模块私有；当前 `GenrePatchProfile` 只复用公开可用的题材 profile 和字段需求。

### 已完成范围

当前 typed patch 范围已经覆盖：

1. `title_patch`
2. `skeleton_patch`
3. `character_patch`
4. `plot_patch`
5. `governance_patch`
6. `metadata_patch`

后续不能再回到“每出一个问题就往主流程堆 loose parser”的方式。

### 第一阶段必须做到

`title_patch`：

- 只允许改 `title` 相关字段。
- 不能改角色、题材、结局、大纲。
- 必须验证书名来自剧情、终局、世界观、主角弧线或关键爽点。
- 支持自然语言字段包，例如“书名：... 理由：...”。

`skeleton_patch`：

- 只允许改题材、简述、前提、终局、主角弧线、世界观意象、总主线因果链、字数档位。
- 用户已明确指定的字段不能被模型覆盖。
- patch 合并后必须能构造 `NovelCreationContract` 并跑 typed gate。

`character_patch`：

- 只允许改角色权威表、关系线、情绪状态账本。
- 必须恰好一个主角槽位。
- 关系线引用的人物必须存在于角色权威表。
- 不能把模型高频默认名当成可靠角色名，除非用户明确指定。

### 第一阶段禁止事项

禁止继续扩 `contract_candidate.rs` 的字段包解析。

具体要求：

- 新增字段包解析必须进入 `creation_contract/patch_normalizer.rs`。
- `contract_candidate.rs` 只负责候选提交、兼容完整合同 JSON、调用 patch fallback、记录 pending/current。
- 不允许在 `contract_candidate.rs` 继续添加 loose parser、字段 label 表、题材判断。

禁止继续扩 `staged_prompts.rs` 的 prompt 细节。

具体要求：

- `staged_prompts.rs` 后续只保留阶段选择或薄调度。
- patch prompt 渲染迁到 `creation_contract/patch_prompt.rs`。
- 题材字段强度迁到 `GenrePatchProfile`。

禁止把写作合同逻辑放回 gateway。

具体要求：

- gateway 只做 API/会话/运行时适配。
- 写作合同、patch、质量门、合同展示都属于 writing 工具。

禁止绕过 typed gate。

具体要求：

- patch 即使解析成功，也只能先应用到 draft clone。
- clone 构造成强类型合同后必须跑 `typed_contract_gate`。
- 通过后才允许写入 `current_contract` 或进入可确认状态。

### 当前文件结构

已存在：

```text
crates/builtin-tools/src/tool/writing/creation_contract/patch.rs
crates/builtin-tools/src/tool/writing/creation_contract/patch_normalizer.rs
crates/builtin-tools/src/tool/writing/creation_contract/patch_prompt.rs
```

已存在：

```text
crates/builtin-tools/src/tool/writing/creation_contract/genre_patch_profile.rs
```

`GenrePatchProfile` 已独立在 `genre_patch_profile.rs`，不能再把题材字段强度逻辑塞回
`staged_prompts.rs`。

### 第一阶段完成标准

代码层面：

- [x] 有 `CreationContractPatch` 和 `CreationContractPatchType`。
- [x] 有 `PatchFieldStrength` 或等价字段强度结构。
- [x] 有 `patch_normalizer`，至少支持 `title/skeleton/character`。
- [x] `contract_candidate.rs` 能在完整 JSON 失败后尝试 patch fallback。
- [x] 显式 patch 优先读取原始模型边界文本，避免中文 sanitizer 清掉 `patch_type`。
- [x] 完整 JSON / 完整自然语言字段包仍走完整合同兼容路径，不被 patch 抢走。
- [x] patch 失败不会污染 `current_contract`。
- [x] patch 成功后仍通过 typed gate 决定是否 ready。

测试层面：

- [x] title patch 不能改角色。
- [x] character patch 不能改书名。
- [x] skeleton patch 不能覆盖用户指定字数和题材。
- [x] Qwen 风格自然语言书名字段包能被提取。
- [x] 不完整 patch 只能进入 pending/diagnostic，不能变成可确认合同。
- [x] “按这个开始”在合同未 ready 时必须明确阻止，而不是重新 planning 或伪 completed。

产品层面：

- [x] 用户仍然只看到合并后的合同草案。
- [x] 用户不需要知道 patch。
- [x] 合同未 ready 时，面板明确展示还缺什么。
- [x] 合同 ready 后，用户自然语言确认可以稳定进入第一章写作。

### 第二阶段完成状态

以下内容已完成迁移或按兼容边界收口：

- [x] `plot_patch`
- [x] `governance_patch`
- [x] `metadata_patch`
- [x] `typed_contract_gate` 接入 `PatchFieldStrength`
- [x] 阶段选择不再在主流程散落文本 `contains`，改为集中 issue 分类函数
- [x] `staged_prompts.rs` 删除旧 JSON schema fallback，改为 patch prompt 薄调度
- [x] 旧自然语言合同解析保留为兼容路径；新增/扩展字段补丁不得继续写入 `contract_candidate.rs`

这样既完成 typed patch 主链路，又避免把完整合同兼容路径一次性砍掉导致真实模型
回归风险扩大。
