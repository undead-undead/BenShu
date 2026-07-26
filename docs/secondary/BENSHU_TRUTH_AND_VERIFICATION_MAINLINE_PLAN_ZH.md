# BenShu Truth And Verification Mainline Plan（中文）

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 关联核心文档: `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`
>
> 关联原则摘要: `docs/secondary/BENSHU_HARDNESS_DESIGN_PRINCIPLES.md`
>
> 关联证据链: `docs/secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md`
>
> 文档定位: 这是 BenShu “默认不瞎猜、该验证就验证、不能验证就明确说不确定”的主线实施计划，覆盖知识事实、工具事实、执行事实与环境/状态事实。

---

## 0. 文档目标

### 0.1 状态标记

- `[x]` 已完成
- `[~]` 部分完成
- `[ ]` 未完成

### 0.2 一句话目标

`BenShu 必须成为一个 truth-first、verification-first 的个人主代理，而不是一个会把猜测包装成事实的幻想执行器。`

本文解决的问题不是“联网工具有没有”，而是：

- 什么时候必须验证一个事实，而不只是“觉得大概对”
- 什么时候必须联网验证
- 什么时候必须先确认工具/命令/文件/模型/运行时真实存在
- 什么时候必须先确认某个动作真的执行成功
- 什么时候可以仅凭本地上下文回答
- 什么时候必须明确说“不确定”
- 什么时候必须带来源
- 这些判断如何进入 `brain / tool / trace / witness / gateway / panel`

---

## 1. 总结论

这条线是 **主线级能力**，不是体验增强项。

如果 BenShu 未来要成为 `Jarvis`，那么以下能力必须统一成立：

1. 不能把推断写成事实
2. 不能把计划写成结果
3. 不能把过时知识写成最新事实
4. 不能把未执行的工具调用、命令、文件修改描述成已完成
5. 不能把未观察到的运行时/环境状态写成已存在
6. 不能把未执行的联网验证描述成已验证
7. 不能在无法确认时继续“顺嘴编下去”

所以这份计划应视为：

- `hardness` 的产品化落地
- `unified tracing` 的真伪治理补完
- `gateway/panel` 的显式风险展示补完

而不是一个孤立小功能。

---

## 2. 当前现状与差距

### 2.1 已有地基

- `[x]` 已有联网工具面
  - [web_search.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/web_search.rs)
  - [web_fetch.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/web_fetch.rs)
  - [browser.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/browser.rs)
  - [realtime_lookup.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/realtime_lookup.rs)
- `[x]` 已有工具选择与运行时能力路由
  - [tool_search.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/tool_search.rs)
  - [mod.rs](/home/biubiuboy/BenShu/crates/brain/src/skills/tool/mod.rs)
- `[x]` 已有 tool routing / capability route
  - [mod.rs](/home/biubiuboy/BenShu/crates/brain/src/skills/tool/mod.rs)
- `[x]` 已有 trace / witness / scorecard 基础设施
  - 见 `telemetry` 与 `brain` 当前主线
- `[x]` 已有 `hardness` 原则层
  - Truth First
  - Verification First
  - Explicit Risk

### 2.2 还没形成主线的部分

- `[x]` 已形成独立的 `truth / verification policy engine`
  - `query classifier`
  - `source-required` 判定
  - `local-context` notice
  - `truth/verification guidance prompt`
  已开始统一由 `brain::agent::truth_verification_policy` 承接
- `[x]` 已有统一的“任何事实声明都必须有观察/执行/验证依据”的总 contract
- `[x]` 已有统一的“是否必须联网验证”判定 contract
- `[x]` 已有统一的“是否必须先确认工具存在/工具成功/执行成功/状态存在”判定 contract
- `[x]` 已有“回答必须带来源” contract 主干
  - `source-required` 判定
  - `source missing` 降级
  - `search_results_only / source_content_observed` 区分
  - `panel / scorecard` 聚合与筛选
  - `source_content_observed + cite_required` 时，最终回答会自动附着来源摘要
  已由独立 `truth / verification policy engine + middleware` 共同承接
- `[x]` 已有统一的“未验证 / 已验证 / 推断 / 不确定”响应标签
- `[x]` 已把联网验证结果稳定收进 `RunTrace / Witness / Scorecard`
- `[x]` 已把工具/执行/状态验证结果稳定收进 `RunTrace / Witness / Scorecard`
- `[x]` 已把这些状态强制展示到 `gateway / panel`

### 2.3 现阶段最本质的差距

当前 BenShu 的问题已经不再是“没有 Truth And Verification 主线”，而是：

- 统一 `truth / verification policy engine` 已经落地
- runtime 已有系统级硬约束主干，且 `stream_chat / think` 已开始注入统一 truth/verification system prompt
- `provider` 适配层继续统一消费共享 `request.system_prompt`，不再额外派生第二套 `provider-specific prompt`
- 工具调用后已能形成结构化验证结果，并支持把来源摘要、执行依据、状态依据附着到最终回答
- `panel/gateway` 已把“已验证 / 未验证 / 推断 / 不确定”做成显式产品语义，后续主要是增强项打磨

一句话：

`当前我们已经有完整的 Truth And Verification Mainline 主干；后续剩余项主要属于增强与打磨，而不再是主线缺口。`

---

## 3. 产品原则

> 说明：
>
> 本节表示“这些产品原则是否已经被工程主线稳定承接”，不是“概念是否已经提出”。
>
> - `[x]` 表示已有稳定 contract、回归和产品面共同支撑
> - `[~]` 表示主干已经进入代码、回归和展示层，但还没有做到全链路稳定覆盖
> - `[ ]` 表示仍主要停留在原则描述

### 3.1 Truth First

- `[x]` 不能把未验证事实写成已确认事实
- `[x]` 不能把模型推断写成外部世界事实
- `[x]` 不能把未观察到的工具/命令/文件/状态写成已成立事实
- `[x]` 不能把旧知识写成“最新情况”

### 3.2 Verification First

- `[x]` 涉及时效性、外部事实、价格、天气、法规、新闻、最新版本、人物/公司现任信息时，默认优先验证
- `[x]` 涉及工具存在性、工具可用性、工具执行结果、文件修改结果、命令执行结果、模型/运行时 readiness 时，默认优先验证
- `[x]` 涉及高风险建议时，默认优先验证
- `[x]` 如果验证失败，不能假装验证已经成功

### 3.3 Explicit Uncertainty

- `[x]` 不确定时必须说不确定
- `[x]` 应明确区分：
  - 已验证
  - 未验证
  - 推断
  - 需要澄清

### 3.4 Source Required When Appropriate

- `[x]` 外部事实类回答在可行时应附来源
- `[x]` 若用户要求“查一下 / 搜一下 / 给链接 / 给来源”，必须显式附来源或明确说明来源仍缺失
- `[x]` 工具与执行类回答在可行时应附执行依据
  - 例如真实 tool result、command result、artifact、trace note、state snapshot

### 3.5 Recovery Before Bluffing

- `[x]` 如果工具不可用、搜索失败、页面抓取失败，系统应回退为：
  - 明确说明验证失败
  - 请求澄清
  - 或只给保守结论
- `[x]` 不能因为工具失败而转为胡乱补全
- `[x]` 不能因为“看起来大概率存在”就跳过工具/执行/状态确认

---

## 4. 范围边界

### 4.1 本计划负责

- `brain` 的真伪与验证路由策略
- `builtin-tools` 的验证执行面收口
- `trace / witness / scorecard` 的真伪证据链
- `gateway / panel` 的状态显式化
- 知识、工具、执行、环境/状态四类事实的统一 contract

### 4.2 本计划不负责

- 外部搜索引擎本身的质量改造
- 通用联网基础设施全部重写
- 替代现有 `web_search / web_fetch / browser` 的全部实现

---

## 5. 统一状态模型

### 5.1 Response Truth Status

- `[x]` `verified`
- `[x]` `unverified`
- `[x]` `inferred`
- `[x]` `uncertain`
- `[x]` `clarification_required`

### 5.2 Verification Domain

- `[x]` `knowledge_fact`
- `[x]` `tool_fact`
- `[x]` `execution_fact`
- `[x]` `state_fact`

### 5.3 Verification Mode

- `[x]` `none`
- `[x]` `local_context_only`
- `[x]` `tool_inventory_check`
- `[x]` `runtime_state_check`
- `[x]` `execution_result_check`
- `[x]` `tool_lookup`
- `[x]` `web_search_fetch`
- `[x]` `browser_validation`
- `[x]` `realtime_lookup`

### 5.4 Verification Outcome

- `[x]` `verification_succeeded`
- `[x]` `verification_not_required`
- `[x]` `verification_tool_unavailable`
- `[x]` `verification_fetch_failed`
- `[x]` `verification_source_insufficient`
- `[x]` `verification_execution_missing`
- `[x]` `verification_state_missing`
- `[x]` `verification_skipped_by_policy_gap`

### 5.5 Source Posture

- `[x]` `sources_attached`
- `[x]` `sources_referenced_but_not_attached`
- `[x]` `execution_evidence_attached`
- `[x]` `state_evidence_attached`
- `[x]` `no_sources_required`
- `[x]` `sources_required_but_missing`

---

## 6. 与当前工具面的对齐原则

### 6.1 现有工具继续复用

- `[x]` `web_search`
- `[x]` `web_fetch`
- `[x]` `browser_browse`
- `[x]` `realtime_lookup`
- `[x]` `tool_search`

### 6.2 不再让模型自己“随缘拼流程”

- `[x]` 对 `latest-info / current / today / recent / who is / current price / forecast / release version / current policy` 一类问题，统一硬路由到验证模式
- `[x]` 对“帮我调用工具 / 帮我确认工具有没有 / 帮我看文件是否改了 / 帮我确认当前系统状态”的问题，统一硬路由到工具/执行/状态验证模式
- `[x]` 对“外部事实但可静态回答”的问题，允许本地知识回答，但必须能够标记为 `unverified` 或 `inferred`
- `[x]` 对必须先看附件/文档的内容，继续保持 `document_understand` 的硬门槛语义

当前进展：

- `latest / current / today / price / release version` 已有最小 query 分类回归，并会优先落到 `RealtimeLookup` 验证模式
  - 现已覆盖例如：
    - `今天 OpenAI 最新新闻`
    - `英伟达当前股价`
    - `Bun 最新发布版本`
    - `最近 bun 发布了什么版本`
    - `当前 OpenAI API 定价政策是什么`
    - `美国现任总统是谁`
    - `who is the current president`
- `工具/执行/状态` 的确认句式现在也会进入最小回归：
  - `帮我看当前系统状态`
  - `帮我看文件是否改了`
  - `帮我调用 ffmpeg 跑一下`
  - `帮我调用 python runtime 跑一下`
  - 也已覆盖例如：
    - `帮我确认 git 有没有安装`
    - `帮我确认 quickjs runtime 已经准备好了吗`
    - `帮我确认当前目录有没有未提交改动`
    - `is ffmpeg installed`
    - `is docker available right now`
    - `is quickjs ready right now`
    - `check whether git status shows changes`
- `document_understand` 的硬门槛语义仍在 query classification 与 media follow-up 回归里保持
- `静态外部事实` 已开始进入最小 `local_context_allowed` 回归：
  - `介绍一下 OpenAI`
  - `tell me about OpenAI`
  - 当前回答前 runtime 也会将这类请求显式标记为：
    - `truth_status=Unverified`
    - `verification_requirement=LocalContextAllowed`
    - `verification_mode=LocalContextOnly`
  - 当前 `Scorecard / failure_reasons` 也会额外区分：
    - `verification::knowledge_fact::local_context_only`
- 工具/执行/状态验证的统一“全部硬路由”仍可继续补更完整模式覆盖

### 6.3 与我当前联网工具相比，BenShu 当前主要差距

- `[x]` 已有搜索、抓取、浏览零件
- `[x]` `search -> open/read -> cite -> final answer` 主线已形成
  - `web_search` 会显式标记 `search_results_only`
  - `web_fetch / browser_browse` 会显式标记 `source_content_observed`
  - `source_content_observed + cite_required + sources_attached` 时，最终回答会自动附着 `Sources:` 摘要
  - 工具/运行时验证若产出 `execution_evidence / state_evidence`，最终回答也会自动附着 `Execution Evidence:` / `State Evidence:` 摘要
- `[x]` “必须验证”的系统级策略层已进 `query classifier + middleware + trace / witness / panel`
- `[x]` “不能验证就明确说不确定”的响应 contract 已进入运行时降级主线
- `[x]` 结构化来源输出与前台状态展示已进 `RunTrace / Witness / Panel / Scorecard`
  - 若后续再补独立聚合 API，属于增强项，不再影响当前主线成立

### 6.4 本计划不是只处理 web verification

- `[x]` 联网验证只是其中一个子面
- `[x]` 已统一进入主线的验证域包括：
  - 工具存在性验证
  - 工具成功执行验证
  - 命令/文件/API 执行结果验证
  - 模型/运行时/宿主环境状态验证
- `[x]` 当前主线目标已经成立：
  - 任何事实声明都不能脱离观察、执行或验证依据

### 6.5 Web / Verification Tool Surface 收口策略

结论先写清楚：

- `[x]` 已在 contract/result 层收口成统一的 `web / verification` 能力模块
- `[x]` 不应把所有 web 工具硬并成一个超级工具

原因如下：

#### 不建议硬并成一个工具

- `[x]` `web_search` 的职责是“找来源”
- `[x]` `web_fetch` 的职责是“读内容”
- `[x]` `browser_browse` 的职责是“处理交互式网页与动态页面”
- `[x]` `realtime_lookup` 的职责是“处理结构化实时信息”
- `[x]` `tool_search` 的职责是“先帮 runtime 找对工具”

这些能力并不等价。

如果硬并成一个超大 `web_tool`，会带来：

- schema 过肥
- 模型更容易乱传参数
- 安全边界不清晰
- 测试矩阵更难维护
- 真伪与来源 contract 更难统一

#### 应该在模块层统一

更合理的方式是：

- `[x]` 工具保持分工独立
- `[x]` 模块级统一：
  - `VerificationDomain`
  - `VerificationMode`
  - `VerificationOutcome`
  - `VerificationSource`
  - `SourcePosture`
  - `VerificationResultEnvelope`
  - `VerificationFollowupPlan`

也就是说：

- 对模型暴露：仍是多个明确工具
- 对工程内部：应收口成一个统一 `web_verification` / `fact_verification` 能力域

#### 当前建议结构

- `[x]` 保留独立工具：
  - `web_search`
  - `web_fetch`
  - `browser_browse`
  - `realtime_lookup`
  - `tool_search`
- `[x]` 在 `brain` 层统一路由：
  - 先判断是否需要验证
  - 再判断属于哪类验证域
  - 再选择合适工具链
  - 最后统一回收 `VerificationResultEnvelope`
- `[x]` 在 `builtin-tools` 层统一 result contract，而不是统一 tool entrypoint

一句话收口：

`建议模块合并，工具分立。`

---

## 7. 详细开发计划

### Phase V0：现状收口与 contract 立项

状态：`[x]`

- `[x]` 在 `brain` 中新增 truth/verification contract 的正式结构体
  - `VerificationDomain`
  - `VerificationRequirement`
  - `VerificationMode`
  - `VerificationOutcome`
  - `SourcePosture`
  - `TruthStatus`
  - `QueryVerificationPlan`
- `[x]` 为 `RunTrace.metadata`、`Witness notes`、`Scorecard` 预留统一字段
- `[x]` 明确哪些 query pattern 必须进入验证模式
- `[x]` 明确哪些 query pattern 可以 `local_context_only`
- `[x]` 明确四类验证域：
  - `knowledge_fact`
  - `tool_fact`
  - `execution_fact`
  - `state_fact`

建议落点：

- `crates/brain/src/skills/tool/`
- `crates/brain/src/agent/`
- `crates/telemetry/src/`

### Phase V1：Query 风险分类与硬路由

状态：`[x]`

- `[x]` 新增 query classifier:
  - 最新/当前/今天/最近
  - 外部人物/公司/价格/天气/版本
  - 工具存在性/可用性确认
  - 执行结果确认
  - 环境/状态确认
  - 法律/医疗/金融高风险
- `[x]` 将这些分类映射到：
  - `verification_required`
  - `verification_recommended`
  - `local_context_allowed`
- `[x]` 对必须验证问题，若工具不可用，返回“验证失败”而不是直接猜
  - 当前最小 regression 已覆盖：
    - `latest info`
    - `current price`
    - `release version`
    - `current role holder`
    - `current policy`
    - `current system state`
    - `file change confirmation`
    - `external tool invocation`
    - `runtime invocation`
    - `english cli installation`
    - `english tool availability`
    - `english runtime ready`
    - `english git status confirmation`
    - `english execution completion`
    - `medical high-risk advice`
    - `legal high-risk advice`
    - `financial high-risk advice`
    - `tool unavailable downgrade`
    - `high-risk advice downgrade`
    - `document_understand hard gate`
    的最小 regression

建议落点：

- [mod.rs](/home/biubiuboy/BenShu/crates/brain/src/skills/tool/mod.rs)

### Phase V2：统一验证执行面

状态：`[x]`

- `[x]` 收口 `web_search + web_fetch + browser + realtime_lookup + tool_search + runtime/state checks` 的统一结果结构
- `[x]` 新增 verification result envelope，至少包含：
  - `verification_domain`
  - `verification_mode`
  - `verification_outcome`
  - `sources`
  - `execution_evidence`
  - `state_evidence`
  - `observed_at`
  - `notes`
- `[x]` 把“搜到了但没读”、“读了但没引用”、“工具存在但没执行”、“执行计划存在但没结果”这类情况显式化
  - 当前 `tool_search` 已开始返回 `verification_plan + verification_preview`
  - 当前 `tool_search` 已开始返回 `verification_followup`
- 当前 `web_search / web_fetch / realtime_lookup` 已支持结构化 `verification_preview`
- 当前 `web_search / web_fetch / browser_browse` 已支持结构化 `verification_followup`
  - `web_search` 明确标记 `search_results_only` 并建议继续 `web_fetch`
  - `web_fetch` 明确标记 `source_content_observed`，表示可进入带来源回答
  - `browser_browse.navigate / snapshot / screenshot` 现在也会在知识事实场景下标记 `source_content_observed`
- 当前 `runtime_surface` 已开始返回 `verification_preview`
- 当前 `browser_browse` 已支持可选 `structured` 结果与 `verification_preview`
- `[x]` 完成 `web / verification` 模块收口，但保留工具分立
  - 当前已接到：
    - `tool_search`
    - `web_search`
    - `web_fetch`
    - `realtime_lookup`
    - `runtime_surface`
    - `browser_browse`
  - 更广泛 execution/state tool 面的继续覆盖，已归入后续增强项，不再阻塞当前阶段完成判定

建议落点：

- `crates/builtin-tools/src/tool/`
- `crates/brain/src/skills/tool/`

### Phase V3：回答 contract 与不确定性表达

状态：`[x]`

- `[x]` 在最终回答生成前写入：
  - `truth_status`
  - `verification_domain`
  - `source_posture`
  - `verification_mode`
  - `verification_outcome`
- `[x]` 系统提示中强制要求：
  - `runtime middleware` 已强制：
    - 未验证不能写成已验证
    - 推断不能写成事实
    - 验证失败必须说明失败
  - `stream_chat / think` 已通过独立 `truth / verification policy engine` 注入统一 `TRUTH AND VERIFICATION CONTRACT`
  - `provider` 适配层继续统一消费共享 `request.system_prompt`，不再额外派生第二套 `provider-specific prompt` 逻辑
- `[x]` 工具/执行/状态类回答必须区分：
  - 已观察到结果
  - 仅计划执行
  - 未发现证据
- `[x]` 为“需要来源但缺来源”的场景加硬性降级

当前进展：

- `middleware` 已开始把工具层 `verification_preview` 收成统一 runtime note
- `truth / verification policy engine` 已开始统一承接：
  - `query classifier`
  - `source-required` 判定
  - `local-context` notice
  - `source missing / downgrade` contract
  - `TRUTH AND VERIFICATION CONTRACT` prompt 文本
- `BeforeResponse` 已开始产出：
  - `truth_status`
  - `verification_domain`
  - `verification_requirement`
  - `verification_mode`
  - `verification_outcome`
  - `source_posture`
  - `verification_last_tool`
- `BeforeResponse` 现已对这些场景做最终回答显式降级：
  - `Unverified / Inferred / Uncertain / ClarificationRequired`
  - `SourcesRequiredButMissing / SourcesReferencedButNotAttached`
  - `VerificationExecutionMissing / VerificationStateMissing / VerificationFetchFailed / VerificationToolUnavailable / VerificationSourceInsufficient / VerificationSkippedByPolicyGap`
- `stream_chat` 与 `reasoner.think` 现在都已开始注入统一的 `TRUTH AND VERIFICATION CONTRACT` system prompt，保证：
  - 未验证不能写成已验证
  - 搜索结果未读源页时不能写成已确认事实
  - 仅凭本地上下文回答时必须显式保持 `unverified / inferred`
- 若用户明确要求“给来源 / 给链接”，当前即使走 `LocalContextOnly`，回答前也会被标记为：
  - `source_posture=SourcesRequiredButMissing`
  - `verification_cite_required=true`
- 若用户明确要求“给来源 / 给链接”，当前即使已经走过工具验证，只要仍停在：
  - `search_results_only`
  - `SourcesReferencedButNotAttached`
  - 或其他尚未形成 source-backed answer 的状态
  回答前也会被升级为：
  - `source_posture=SourcesRequiredButMissing`
  - `verification_cite_required=true`
  - 并强制追加 source-missing downgrade notice
- `Scorecard` 当前也会为这类情况产出独立 failure key：
  - `verification::source_required::still_missing`
- `panel` 当前也已提供直接筛选入口：
  - `Source Required = Still Missing`
  - `Filter Source Missing`
- 若已经进入 `source_content_observed + SourcesAttached`
  - 则不会误报“来源仍缺失”
- 若已经进入 `source_content_observed + SourcesAttached + cite_required`
  - 则最终回答会自动附着 `Sources:` 摘要
- `RunTrace / Panel` 当前也已显示 `verification_sources_json` 对应的来源摘要，不再只停在 `source_count`
- `RunTrace / Witness / Panel` 现也会显式显示 `truth_verification_guidance_active`
- 当前收口判断：
  - 统一 `truth / verification policy engine + runtime system prompt + middleware downgrade` 已构成主线
  - `provider` 侧通过共享 `request.system_prompt` 继续消费同一 contract，不再额外引入第二套 `provider-specific prompt / response template`

建议落点：

- `crates/brain/src/agent/reasoner.rs`
- `crates/brain/src/agent/middleware.rs`
- `crates/brain/src/agent/run_trace_builder.rs`

### Phase V4：Trace / Witness / Scorecard 收口

状态：`[x]`

- `[x]` `RunTrace.metadata` 收 truth/verification 字段
- `[x]` `RuntimeStage` 投影这组状态
- `[x]` witness notes 显式记录：
  - `truth_status`
  - `verification_mode`
  - `verification_outcome`
  - `source_posture`
- `[x]` scorecard 对这些结果给出：
  - `warn`
  - `failure_reasons`
  - 适度 score penalty

当前进展：

- `RunTrace.metadata` 已开始收：
  - `truth_status`
  - `verification_domain`
  - `verification_requirement`
  - `verification_mode`
  - `verification_outcome`
  - `verification_answer_readiness`
  - `verification_next_tools`
  - `verification_cite_required`
  - `source_posture`
  - `verification_last_tool`
  - `verification_tools`
- witness notes 已开始投影这组 runtime metadata
- `scorecard` 现已对 truth/verification 给出正式判定：
  - `verification::truth_status::*`
  - `verification::<domain>::<outcome>`
  - `verification::source_posture::*`
- 这组结果在 transcript/outcome 本身成功时保持 `warn`，不会伪装成 `pass`，也不会误升成整体 `fail`

建议落点：

- `crates/brain/src/agent/`
- `crates/telemetry/src/eval.rs`

### Phase V5：Gateway / Panel 产品化

状态：`[x]`

- `[x]` `gateway` API 透出统一 truth/verification 状态
- `[x]` `panel` 显示：
  - `Verified`
  - `Unverified`
  - `Inferred`
  - `Uncertain`
  - `Source Missing`
- `[x]` `Witness / Scorecard / Metrics` 支持按：
  - `truth_status`
  - `verification_mode`
  - `verification_outcome`
  - `source_posture`
  筛选

当前进展：

- `panel > RunTrace > Runtime Governance` 已开始显示：
  - `truth_status`
  - `verification_domain`
  - `verification_mode`
  - `verification_outcome`
  - `verification_answer_readiness`
  - `verification_next_tools`
  - `verification_cite_required`
  - `source_posture`
  - `verification_last_tool`
- `Witness Log` 现已支持 truth/verification 过滤行：
  - `Truth`
  - `Domain`
  - `Requirement`
  - `Mode`
  - `Outcome`
  - `Readiness`
  - `Next Tool`
  - `Cite Required`
  - `Sources`
  - `Verified By`
- `Scorecard` 查询现已支持同一组 truth/verification 字段筛选
- `Metrics` 子页现已提供 truth/verification 过滤入口，并复用同一查询面
- `Witness / Scorecard` 查询回归现已覆盖：
  - `verification_answer_readiness`
  - `verification_next_tools`
  - `verification_cite_required`
- `panel` 现已为 `LocalContextAllowed + LocalContextOnly` 提供更直接的筛选入口：
  - `Local Context = Allowed but Unverified`
  - `Filter Local Context`
- 后续增强项：
  - 若需要，再补更独立的 gateway 聚合 API，而不是只复用现有 telemetry query 面

建议落点：

- `apps/gateway/src/api/handlers/`
- `apps/panel/src/app_state.rs`
- `apps/panel/src/ui/agent/mod.rs`

### Phase V6：最小回归与强制门槛

状态：`[x]`

- `[x]` 最新信息问题未联网时，不得直接给“已确认口吻”回答
- `[x]` 外部事实问题验证失败时，必须降级为明确不确定
- `[x]` 需要来源的问题若无来源，不得输出“已验证”标签
- `[x]` 工具不存在或工具未执行成功时，不得输出“已执行/已调用”
- `[x]` 文件/命令/API 没有真实结果时，不得输出“已完成”
- `[x]` 运行时/模型/宿主状态未观察到时，不得输出“已存在/已就绪”
- `[x]` `document_understand` 的既有硬门槛继续保留
- `[x]` 对高风险问题补最小 regression suite

当前进展：

- `BeforeResponse` 已对这些情况强制显式降级：
  - `Unverified / Inferred / Uncertain / ClarificationRequired`
  - `VerificationExecutionMissing / VerificationStateMissing`
  - `VerificationToolUnavailable / VerificationFetchFailed / VerificationSourceInsufficient / VerificationSkippedByPolicyGap`
  - `SourcesRequiredButMissing / SourcesReferencedButNotAttached`
- `BeforeResponse` 已开始读取 `verification_followup`
  - `search_results_only` 会触发显式降级，避免“搜到了结果但还没读源页”时继续用确认口吻回答
- 现有最小 regression 已覆盖：
  - 运行时执行结果未观察到时，不得说“已准备好”
  - 外部事实缺来源时，不得保留“已确认口吻”
  - 最新/今天类问题若只完成计划、未完成验证，不得保留“已确认口吻”
  - 工具不可用时，不得继续保留“已存在/已可调用”口吻
  - 工具真实报错时，不得继续保留“已确认/已完成”口吻
  - 医疗/法律/金融高风险建议若未完成验证，不得继续保留确认式建议口吻
  - `document_understand` 仍保持 `Required + ToolLookup` 的硬门槛，不会退化成 `local_context_only`
- 后续若继续扩展：
  - 可再补更系统化的高风险 regression suite，但这已属于增强项，不再阻塞当前主线完成判定

---

## 8. 当前建议优先级

### P0：立刻做

- `[x]` `Phase V0`
- `[x]` `Phase V1`

因为这是“默认不瞎猜”的最低前提。

### P1：紧接着做

- `[x]` `Phase V2`
- `[x]` `Phase V3`

因为统一验证执行面与回答 contract 已经到位，后续若继续推进主要属于增强而非主线阻塞。

### P2：随后做

- `[x]` `Phase V4`
- `[x]` `Phase V5`

因为这决定：

- 能不能审计
- 能不能回放
- 用户能不能看到“这条回答到底有多真”

### P3：收口

- `[x]` `Phase V6`

---

## 9. 完成标准

满足以下条件时，这条主线可以视为完成：

- `[x]` 最新/当前类问题默认进入验证判定
- `[x]` truth/verification 会进入 `RunTrace / Witness / Scorecard`
- `[x]` 缺来源、未验证、仅计划执行时，最终回答会显式降级
- `[x]` 工具/执行/状态类问题默认进入相应验证判定
- `[x]` 工具失败时不会转为瞎猜
- `[x]` 回答能显式区分 `verified / unverified / inferred / uncertain`
- `[x]` 来源状态能进入 `trace / witness / scorecard`
- `[x]` 工具证据、执行证据、状态证据也能进入 `trace / witness / scorecard`
- `[x]` `gateway / panel` 能直接显示真伪与验证状态
- `[x]` `Witness / Scorecard / Metrics` 能按 `readiness / next_tool / cite_required` 查询或筛选
- `[x]` `LocalContextAllowed + LocalContextOnly` 会在回答层显式标记为 `Unverified`，并进入 `Scorecard` 独立 failure key
- `[x]` 至少有一组 regression 测试证明：
  - 未验证不会伪装为已验证
  - 验证失败不会被包装成成功
  - `RunTrace / Witness` 会保留 truth/verification 与 evidence 计数
  - `latest/current`、工具存在性、runtime readiness、仓库改动确认、静态外部实体说明等自然句式都会进入最小验证分类回归

### 9.1 当前收口判断

- `[x]` 这条主线的业务开发项已经完成
- `[x]` 后续若继续扩展：
  - 更丰富的句式覆盖
  - 更独立的聚合 API
  - 更细的前台读面
  均属于增强项，不再构成当前主线未完成项

---

## 10. 一句话结论

`BenShu 要成为 Jarvis，就必须把“默认不瞎猜、该验证就验证、不能验证就明确说不确定”做成一条主线，而不是留在提示词和零散工具能力层。`
