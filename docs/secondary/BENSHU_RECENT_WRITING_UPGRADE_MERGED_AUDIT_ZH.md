# BenShu 近期四份写作升级文档合并总结与代码审查

> 日期: 2026-06-04
> 口径: 按 `docs/secondary` 最近修改时间选取四份升级文档，并对当前代码做代码层核对。
> 范围: writing 工具、gateway chat host、panel chat/artifact surface。

## 1. 本次合并的四份文档

1. `BENSHU_WRITING_SINGLE_STATE_MACHINE_REPAIR_PLAN_ZH.md`
   - 核心: 写作合同必须收敛到单一权威状态机。
   - 目标: 修复“合同生成/确认后仍无法进入第一章”。

2. `BENSHU_WRITING_TOOL_REFACTOR_PHASE_1_5_ZH.md`
   - 核心: 合同强类型化、情感状态机、intent policy 收口、写作工具模块化、真实面板回归。
   - 目标: 减少冗余规则，避免靠散点 `contains` 修真实写作问题。

3. `BENSHU_WRITING_WORKFLOW_OPTIMALITY_AUDIT_ZH.md`
   - 核心: 五项主线最优解。
   - 五项主线:
     - 合同到正文的路由稳定性。
     - 命名质量门。
     - 角色/实体权威表强制执行。
     - 写作状态机。
     - 元数据问题不得阻塞正文生成。

4. `BENSHU_CHAT_STREAM_AND_ARTIFACT_SURFACE_PLAN_ZH.md`
   - 核心: 聊天界面不要塞长正文，写作正文保存为 artifact，面板展示进度、摘要和打开按钮。
   - 目标: 支持合同完整展示、artifact 按钮、SSE 进度流式。

## 2. 合并后的主线结论

四份文档其实指向同一个产品闭环:

```text
用户自然语言
-> 轻量 intake / 具体写作需求
-> LLM 生成合同候选
-> normalizer 只修结构边界
-> Rust 强类型合同 validate
-> ready 合同写入 current_contract
-> 面板展示完整合同，用户自然语言修改或确认
-> approve draft / 创建或恢复 project
-> writer workflow 写章节 artifact
-> 正文保存到文件，聊天只显示摘要、进度、路径和审查状态
-> approved 章节才更新 truth / summary / hooks / ledgers / export
```

关键治理边界:

- 写作策略归 writing 工具，不归 `runtime-policy-core`、`brain/reasoner` 或 gateway 主聊天逻辑。
- gateway 只做 host adapter: session、HTTP、task、artifact、SSE/progress surface。
- 合同权威只允许一个: `SessionCreationDraftState.current_contract`。
- `pending_contract_candidate` 只能作为候选，不能用于写正文。
- blocked/needs-repair 合同不能污染 title、outline、characters、genre、brief、premise 等可确认字段。
- 元数据问题只修 metadata，不能因为标题/摘要问题重写正文。
- 正文是 artifact，不默认进入聊天历史。

## 3. 当前代码已落地的部分

### 3.1 合同权威状态机已开始收敛

代码证据:

- `SessionCreationDraftState` 已有:
  - `diagnostics`
  - `current_contract`
  - `pending_contract_candidate`
- `submit_generated_contract_candidate_to_draft` 是新合同候选提交入口。
- ready 合同才会:
  - `apply_strong_novel_contract_to_creation_draft`
  - 写入 `current_contract`
  - 清空 `pending_contract_candidate`
  - 切到 `ContractReady`

对应文件:

- `crates/builtin-tools/src/tool/writing/creation_contract.rs`
- `crates/builtin-tools/src/tool/writing/creation_contract_normalizer.rs`
- `crates/builtin-tools/src/tool/writing/creation_contract_model.rs`

### 3.2 ContractNormalizer 已落地

代码证据:

- 已新增 `creation_contract_normalizer.rs`。
- `NovelCreationContract::parse_json_boundary` 已先走 normalizer。
- normalizer 做的是结构边界修复，不生成剧情内容。

已覆盖能力:

- fenced JSON / raw JSON 提取。
- flat JSON 转 typed JSON。
- `title` 字符串归一到 `title.canonical_title`。
- `ending_direction` 等别名归一。
- string/list 类型归一。

### 3.3 gateway 已部分降级为 host adapter

代码证据:

- `apps/gateway/src/api/handlers/chat_tool_host.rs` 中合同候选提交改为调用:
  - `submit_generated_contract_candidate_to_draft`
- gateway 不再直接调用旧的:
  - `generated_contract_gate_result`
  - `apply_generated_contract_to_creation_draft_relaxed`
  - `sanitize_generated_contract_for_absorption`
  - `creation_draft_contract_blocking_issues`

当前状态:

- 主路径比之前干净。
- gateway 仍有“寻找最近合同候选文本”的 host 辅助逻辑，见后面的风险项。

### 3.4 合同 blocked 与 task status 有更明确边界

代码证据:

- `session_surface::creation_contract_status_for_draft`
  - 合同未 ready 时映射为 blocked。
  - `ContractReady / Approved / Writing` 才不阻塞。
- `session_surface::creation_contract_draft_is_confirmable`
  - 明确只有 ready/approved/writing 可确认。

意义:

- 可以避免“内部合同未通过，但外层 task 显示 completed”的一部分误导。

### 3.5 artifact / stream surface 已有底座

代码证据:

- `ChatResponse` 已有 `artifacts: Vec<ChatArtifactRef>`。
- gateway 已有 `/api/chat/stream`，返回:
  - `accepted`
  - `status`
  - `artifact`
  - `final`
  - `error`
- panel API 已有 `ChatStreamEvent`。
- panel 已有 `open_target` 按钮和 `/api/artifacts/open`。

当前状态:

- 结构化 artifact 按钮和路径打开能力已有。
- `/api/chat/stream` 当前更像包装同步 `/api/chat` 的 SSE 壳，不是真正 token streaming。

### 3.6 元数据修复与正文分层已有底座

代码证据:

- `novel_studio` 已有:
  - `chapter_quality_gate`
  - `chapter_metadata_gate`
  - `repair_latest_chapter_metadata`
  - `metadata_gate_needs_repair`
- `novel_workflow_driver` 已有:
  - metadata repair loop
  - metadata blocker result
  - warning-only 不触发正文修订的测试。

意义:

- “标题/摘要/key_facts/continuity_updates 问题不应重写正文”的方向已接线。

## 4. 代码层仍有风险或不一致

### 风险 1: 旧自然语言合同吸收函数仍在 production 代码里

当前仍存在:

- `apply_generated_contract_to_creation_draft_relaxed`
- `apply_generated_contract_to_creation_draft_inner`
- `generated_contract_field`
- `generated_contract_list`
- `apply_generated_structured_fiction_contract`
- `apply_generated_fiction_contract_sentence_fallbacks`

位置:

- `crates/builtin-tools/src/tool/writing/creation_contract.rs`

当前判断:

- 新合同候选提交主路径已经不再依赖它们。
- 但这些函数仍是 public / production 可调用，未来很容易被重新接回主路径。

风险:

- 旧 fallback 一旦回流，就会重新出现:
  - 半成品字段进入 draft。
  - 用户控制语进入 genre/brief/premise。
  - “未指定”合同被误认为可用。
  - 书名、角色名错位。

建议:

- 将旧自然语言吸收函数改名并降级为 legacy/migration 专用。
- 新合同路径只能调用 `submit_generated_contract_candidate_to_draft`。
- 对旧函数增加注释和测试，明确不能用于新合同 ready 判定。

### 风险 2: gateway 仍有合同候选文本识别逻辑

当前代码:

- `chat_tool_host.rs` 中 `creation_contract_text_is_usable_candidate`
- `latest_completed_contract_text`
- `latest_assistant_contract_text`

这些逻辑会从 task result / assistant message 中找合同候选。

当前判断:

- 这属于 host adapter 边界附近的灰区。
- 它没有直接判断合同质量，最终会交给 writing 的 `submit_generated_contract_candidate_to_draft`。
- 但“什么文本像合同”这个判断仍写在 gateway，长期看不够干净。

风险:

- gateway 仍然知道“合同草案/书名/题材/title/characters/ending”等写作语义。
- 如果以后其他写作类型扩展，gateway 可能继续膨胀。

建议:

- 将 `creation_contract_text_is_usable_candidate` 移到 writing 的 session surface 或 creation contract 模块。
- gateway 只调用 `writing.is_contract_candidate_text(text)`。

### 风险 3: `/api/chat/stream` 仍不是真正的渐进式运行流

当前代码:

- `chat_stream_handler` 先发送 accepted/status。
- 然后内部仍调用 `chat_handler(...)`。
- 最后一次性发 artifacts/final。

当前判断:

- 这满足“面板不会完全沉默”的最低要求。
- 但还不能稳定展示“正在生成合同 / 已生成角色 / 已生成大纲 / 正在写章节”等细粒度阶段。

风险:

- 长合同或长章节时，用户仍可能看到前面一两条 status 后继续等待。
- 本地 provider 真 token streaming 没有完全产品化时，面板体验仍像“卡住”。

建议:

- 优先接 durable task checkpoint -> SSE event。
- 不急着做 provider token streaming。
- 写作 workflow 的阶段 checkpoint 应映射成 UI status event。

### 风险 4: 合同生命周期枚举仍比文档目标简化

文档目标状态:

```text
Intake / DraftingContract / ContractCandidateGenerated / ContractNeedsRepair /
ContractReady / Approved / Writing / Paused / Blocked / Completed / Cancelled
```

当前代码:

```text
DraftingContract / ContractReady / Approved / Writing / Blocked / Cleared
```

当前判断:

- 简化版不是错误，能覆盖主路径。
- 但 `NeedsRepair`、`Paused`、`Completed` 没有成为显式合同状态，部分语义依赖 task status 或 diagnostics。

风险:

- 合同自动修复、暂停恢复、完成展示仍可能要从多个来源推断。

建议:

- 先不要急着扩枚举。
- 如果真实面板仍出现“blocked/completed/repairing 混淆”，再补显式状态。
- 不应为了贴文档一次性新增空状态。

### 风险 5: chat.rs 仍承担较多通用任务判定

当前代码:

- `chat.rs` 中仍有大量通用任务、artifact、实时查询、memory、creation planning、completion gate 判断。

当前判断:

- 这不是写作工具独有问题。
- 但写作相关语义应继续从 `chat.rs` 迁出，只保留通用 task supervisor 和 host plumbing。

风险:

- 如果继续在 `chat.rs` 中加写作条件，之前“修一次坏一次”的问题会回来。

建议:

- 写作相关判断只允许:
  - marker routing。
  - 调 writing host adapter。
  - task/artifact/SSE plumbing。
- 具体合同、角色、标题、章节质量判断必须在 writing 工具内。

## 5. 合并后的优先级建议

### P0: 不再新增散点修复

接下来遇到写作问题，不要先加:

- 题材词表。
- 书名黑名单。
- gateway contains。
- 正文字符串替换。

先判断问题属于:

- 合同候选提交。
- intent policy。
- project approval。
- writer workflow。
- body gate。
- metadata gate。
- artifact/panel surface。

### P1: 收口 legacy 合同吸收

目标:

- 新合同路径只允许:

```text
normalizer -> strong contract -> validate -> current_contract
```

旧文本 fallback:

- 只能用于旧项目迁移或非小说文档展示兼容。
- 不能用于 ready 判定。

### P2: 把 gateway 的合同候选识别迁回 writing

目标:

- gateway 不出现“书名/题材/title/characters/ending”这类写作语义判断。
- gateway 只调用 writing 暴露的 helper。

### P3: 将写作阶段 checkpoint 接入 `/api/chat/stream`

目标:

- 用户能看到:
  - 正在生成合同。
  - 正在校验合同。
  - 合同待确认。
  - 正在创建项目。
  - 正在写第 N 章。
  - 正在审稿/修元数据。
  - 已保存 artifact。

### P4: 真实面板回归

必须真实面板，不 mock:

```text
帮我写小说
写都市玄幻小说，每章2500字，至少5万字起
开始写第一章
```

验收:

- 泛化开场只追问，不启动重任务。
- 具体需求自动生成完整合同。
- 用户能看到完整合同。
- 确认后进入 writer，不回到 creation planning。
- 第一章保存 artifact。
- 聊天框只显示摘要、路径、状态，不显示正文全文。

## 6. 当前总体判断

四份文档并不冲突，应该合并成一条主线:

```text
写作工具内部强状态机 + 强类型合同 + 分层质量门
gateway 只做 host adapter
panel 只展示权威状态、进度和 artifact
```

当前代码已经比前几轮干净很多，尤其是:

- `current_contract/pending_contract_candidate` 已进入 draft。
- 合同候选提交已形成单一事务入口。
- gateway 主合同质量判断已经收掉一大块。
- artifact / SSE / 面板打开能力已有底座。

但还不能说完全闭环，因为:

- legacy 自然语言合同吸收仍在 production 文件中。
- gateway 仍有合同候选文本识别语义。
- `/api/chat/stream` 还不是完整运行阶段流。
- 真实面板“确认后写第一章”仍需要回归验证。

下一步最稳的做法不是继续大改，而是先收口 P1/P2，再跑真实面板回归；如果仍失败，再按失败点进入 P3/P4。
