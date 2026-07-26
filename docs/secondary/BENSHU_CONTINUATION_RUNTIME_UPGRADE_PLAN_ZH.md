# BenShu Context Governance Runtime 升级方案

> 状态: 非 KV 主线保留并继续推进；磁盘 KV / 对外 KV 复用路线已撤销。
> 范围: 长上下文治理、压缩协作、工具调用 exact replay、worker continuation、面板可观测、上下文错误、artifact 引用、真实回归测试。
> 产品方向: Windows 原生优先；WSL bridge 仅作为当前开发测试路径。
> 重要边界: 本文不要求 provider/backend 暴露模型内部缓存，也不把文本缓存伪装成模型缓存。

## 0. 本次修正说明

上一版文档把 KV 磁盘复用路线删掉时，也过度压缩了已经完成或正在推进的其他升级内容，这是不对的。

正确取舍是：

- 删除: disk prefix cache、backend payload export/import、cached/suffix token 命中率、对外 `kv_cache_reuse` 能力声明。
- 保留: provider continuation 合同、tool exact replay、worker frontier、工具自治 context package 边界、上下文超限错误、面板进度、后台任务、artifact ref、真实回归矩阵。
- 继续推进: 上下文治理和任务连续性；通用聊天层不缓存、套用或继承旧任务正文。

## 1. 为什么要做

BenShu 的核心形态不是单轮问答，而是：

- 多轮聊天。
- 记忆/RAG。
- 工具调用。
- worker delegate。
- 长任务后台执行。
- 小说、论文、PDF、代码等 artifact 产物。
- 面板可见的真实执行进度。

这些场景反复遇到同一个问题：

```text
用户任务在多轮、工具、worker、后台任务之间推进，
但目标、上下文、工具结果、产物路径和进度容易断裂。
```

所以 Continuation Runtime 的目标不是“做 KV cache”，而是建立 BenShu 的通用连续运行能力：

```text
语义层知道任务如何连续
工具层知道结果如何接回
worker 层知道自己推进到哪里
artifact 层知道大内容放在哪里
gateway/面板知道正在发生什么
测试层知道这些机制是否真的稳定
```

## 2. 不是什么

Continuation Runtime 不应该变成新的大脑，也不应该吞掉现有模块职责。

它不是：

- 背景压缩。
- 记忆库。
- 知识库。
- 工具策略。
- worker 路由器。
- 写作工具。
- 浏览器工具。
- 某个模型的专用 runtime。
- 反机器人/指纹绕过系统。
- 模型内部 KV 持久化系统。

它只负责一个通用问题：

```text
同一个任务、同一个会话、同一个工具链、同一个 worker run，
如何可靠地从上一步继续到下一步，并只携带当前步骤必要的语义依据。
```

## 3. 与压缩层是否冲突

不冲突。

压缩层决定“给模型看什么”。

Continuation Runtime 决定“这些材料如何作为稳定引用、摘要、receipt、frontier 被接回下一步”。

真正的冲突来自不稳定上下文：

- 每轮重写系统提示。
- 每轮重排工具定义。
- 每轮重新改写背景摘要。
- 把正文、网页、PDF 大段内容直接塞聊天历史。
- 工具返回后重新 canonicalize 成另一个字节形态。

为了协同，压缩层和运行时需要满足：

- 摘要有 `summary_id` / `revision`。
- 未变化摘要保持稳定引用。
- 新事件追加在后段。
- 大内容进入 artifact / knowledge；工具需要 context package 时由工具自治层生成，系统层只引用其摘要或路径。
- 上下文不够时显式错误或拆步，不静默截断。

## 4. 设计原则

### 4.1 通用，不绑定某个模型

BenShu 不能做某个模型、某个 tokenizer、某个 chat template 的专用 continuation 方案。

通用层只管：

- continuation id。
- task/turn/worker/tool/artifact 标识。
- prompt/render fingerprint。
- tool replay receipt。
- provider/runtime telemetry。
- 上下文错误。
- 降级路径。

具体后端自己管：

- tokenization。
- chat template。
- hidden reasoning。
- 运行时内部缓存。
- 模型私有状态。

### 4.2 宁可降级，不能假连续

错误连续比冷启动更危险。

必须满足：

- 用户任务匹配。
- session/turn/worker frontier 匹配。
- tool call id 匹配。
- artifact/truth/summary refs 匹配。
- output contract 匹配。
- context budget 足够。

任一不满足，只能降级、拆步或返回 blocker，不能假装完成。

### 4.3 面板可见

用户不需要理解内部结构，但必须能感受到：

- 任务不是卡死。
- 当前在等待模型、工具、worker、artifact 写入还是后台继续。
- 为什么这轮慢。
- 为什么需要拆步。
- 为什么不能继续。
- 当前产物在哪里。

### 4.4 真实测试优先

底层单元测试可以存在，但不能替代真实 panel/gateway 回归。

agent、编排、工具能力测试必须走真实面板或 gateway 聊天接口，不使用 mock 作为最终证明。

## 5. 治理边界

上下文治理必须分开治理，不能把工具局部能力当成系统通用能力。

### 5.1 系统通用层

系统通用层只负责跨工具、跨 worker、跨任务类型都成立的合同：

- task / session / turn / worker / artifact 标识。
- continuation hint。
- worker frontier。
- tool replay receipt。
- ToolOutcome envelope。
- artifact ref。
- context limit error。
- prompt surface / budget telemetry。
- 前台快速返回和后台进度。

系统通用层不理解“小说章节”“论文证据”“网页列表”“PDF 引用格式”等工具业务。

### 5.2 工具自治层

每个工具可以有自己的 context package、policy、truth、ledger、audit、revision、export 规则。

例如：

- writing 工具可以有 story/document contract、truth ledger、chapter context、审稿修订、伏笔债务。
- browser / web_search 可以有页面观察、证据抽取、链接候选、网络摘要。
- pdf / document 工具可以有结构、引用、页码、导出规则。

这些都属于工具自己的治理，应该放在对应工具模块和对应 worker policy 里，不应该塞进 `runtime-policy-core`、`brain` 通用编排或 gateway 聊天特例。

### 5.3 交界面

工具自治层对系统通用层只暴露通用边界：

- `preview`
- `artifact_ref`
- `receipt`
- `status`
- `progress`
- `blocker`
- `evidence`
- `summary`
- `next_step_hint`

系统通用层根据这些边界做续接、进度、验收和追问；具体如何构造上下文包，由工具自己负责。

## 6. 核心能力

### 6.1 Tool Call Exact Replay

工具调用时，模型真实采样出的工具调用块可能和下一轮客户端传回的规范化 JSON 不一致。

需要记录：

```text
tool_call_id -> sampled_tool_call_block fingerprint/ref
```

工具结果回来时：

1. 优先使用可用的 provider 协议 continuation。
2. 再使用 tool_call_id exact replay receipt。
3. 再基于当前轮任务合同、receipt、artifact_ref 和摘要重新构造必要上下文。
4. 最后冷启动重建上下文。

不能默认把工具历史重新格式化后再假装同一个上下文。

### 6.2 Worker Frontier

BenShu 有主 agent 和 worker，不是单一模型 server。

所以 continuation 不能只按聊天 session 管，还要区分：

- user_session_id。
- turn_id。
- worker_run_id。
- tool_call_id。
- artifact_id。
- continuation_frontier_id。

worker frontier 至少应能说明：

- 当前任务目标。
- 已完成步骤。
- 当前 artifact。
- 当前 summary/truth/context refs。
- 最近一次工具 receipt。
- 下一步建议或 blocker。

主 agent 仍是唯一前台 agent。worker 只负责装备工具后的具体执行。

### 6.3 Protocol Live Continuation

某些 provider 协议天然支持 tool/result continuation。

通用层不假设所有 provider 都支持，只声明能力：

- 支持则优先走 provider continuation。
- 不支持则走 exact replay / 当前轮必要上下文重建 / cold replay。

远端 API 或本地 OpenAI-compatible bridge 不承诺真实模型内部缓存复用。它们只能提供协议级 continuation 和 telemetry。

### 6.4 Thinking / Final / Artifact 分离

面板应该展示进度，但不能把内部推理和大正文混进聊天历史。

需要分离：

- progress event。
- tool event。
- worker event。
- artifact write receipt。
- final answer。

聊天框默认展示：

- 当前阶段。
- 简短摘要。
- 章节号/步骤号。
- 字数/进度。
- artifact 路径。
- 是否通过审查。

正文、网页全文、PDF 内容、大段代码进入 artifact / knowledge；工具自治 context package 只通过摘要或路径回到系统层。

用户明确要求查看正文时，也优先展示摘要/节选和文件路径，不默认把巨大正文塞回聊天历史。

### 6.5 上下文长度错误显式化

上下文超限不能静默裁剪后假装成功。

错误应该包含：

- prompt tokens。
- configured ctx。
- requested output tokens。
- overflow amount。
- 哪个部分占用最大。
- 建议动作：压缩、拆步、后台化、降低单步输出、调大 ctx。

这类错误应该成为 runtime 可处理信号，而不是普通文本。

### 6.6 产物与上下文合同

长任务不应该靠聊天历史里的全文续写。

系统通用层应该依赖：

- task contract。
- output contract。
- artifact contract。
- truth/summary/plan refs。
- worker checkpoint。

工具自治层可以进一步依赖自己的 context package。例如 writing 工具可以读取 artifact/truth/summary/chapter context；browser 工具可以读取 page trace/network summary；document 工具可以读取 page map/citation map。

### 6.7 大工具结果持久化

工具或 worker 产生的大结果应该统一变成：

- preview: 给模型和聊天框看的短内容。
- artifact_ref: 完整内容路径或记录 id。
- receipt: status、kind、hash、size、summary、evidence、error_class。

这样可以避免工具输出撑爆上下文，也能让后续步骤通过 ref 精确读取。

## 7. 当前 BenShu 已有底座

### 7.1 已有能力

当前代码已经具备一些基础：

- `provider-core::ChatRequest` 有 `session_id`、`continuation_hint` 和 `enable_cache_control`。
- `ProviderCapabilityView` 有 context window、vision、tools、streaming、locality 等字段。
- `ProviderContinuationCapability` 保留协议级能力，例如 tool exact replay、protocol live continuation、thinking/final split、structured context errors。
- `ContinuationHint` 可携带 session、turn、worker、tool、artifact、frontier、visible prompt fingerprint。
- `ContinuationTelemetry` 保留 provider/runtime 报告的 mode、source、prompt tokens、prefill/decode、miss reason、tool replay/protocol live 标记。
- `brain` reasoner 已把 continuation hint 写入 request extra 和 runtime metadata。
- `brain` 已将 provider telemetry 写入 after-LLM hook metadata。
- executor 已生成 tool replay receipt。
- `ToolCallData` 已有 result truncation、receipt、outcome 相关字段。
- `gateway` 已有前台快速返回、后台任务和任务状态接口。
- `panel` 已能显示任务状态、后台进度和部分 telemetry。
- `compression` 已有 text/json/preview 等通用压缩入口；工具专属压缩策略仍由对应工具模块治理。
- `prompt_surface` 已记录 static/dynamic/governance/tool surface 字符统计。
- `state` 已有 task/checkpoint/artifact 元数据基础。

### 7.2 主要缺口

非 KV 主线仍然有缺口：

- 当前轮必要上下文构造还没有在任务进入、delegate、工具调用、产物验收几个节点完全统一。
- ToolOutcome envelope 还没有覆盖所有工具结果。
- worker frontier 与面板任务进度还没有完全产品闭环。
- 大结果 artifact ref 还没有成为所有工具的统一默认行为。
- 上下文预算和输出合同仍需更细的动态分配。
- 工具自治 context package 还没有全部通过统一 receipt / artifact_ref / summary 暴露给系统通用层。
- 写作、论文、文件产物等长任务的工具自治闭环仍需各工具分别治理。
- 真实面板回归矩阵仍需持续执行。

### 7.3 已撤销能力

以下能力不再作为 BenShu 当前路线：

- `kv_cache_reuse` 对外能力声明。
- disk prefix cache 外层。
- backend 私有 payload export/import。
- cached/suffix token 命中率 telemetry。
- disk payload status/reason/version telemetry。
- 面板 KV/disk payload 展示。

底层推理 backend 可以继续管理自己运行所需的内部缓存和 session 状态；这属于模型执行内部机制，不是 BenShu 对外承诺的 continuation 能力。

## 8. Crate 边界

### 8.1 `crates/provider-core`

负责合同，不负责实现。

保留：

- `ProviderContinuationCapability`。
- `ContinuationHint`。
- `ContinuationTelemetry`。
- `ContextLimitError`。

不应该：

- 保存磁盘文件。
- 持有模型内部缓存。
- 理解工具业务策略。

### 8.2 `crates/inference`

负责本地模型执行和硬件/内存估算。

应该：

- 管理 backend 内部运行状态。
- 提供 context/memory 预算估算。
- 返回清晰推理错误。
- 保留后端自己需要的内部 cache/session 清理能力。

不应该：

- 对外承诺跨轮 KV 复用。
- 暴露 backend 私有 payload。
- 保存 disk continuation cache。
- 理解主 agent 编排或工具业务。

### 8.3 `crates/providers`

负责协议适配。

应该：

- 将 `ContinuationHint` 翻译给本地/远端 provider。
- 将 provider usage/timing 映射为标准 telemetry。
- OpenAI-compatible / Anthropic-compatible provider 声明可用的协议 continuation 能力。
- 支持 tool exact replay 所需的消息格式稳定性。

不应该：

- 把 provider 特例写进 brain。
- 宣称不可验证的模型内部缓存命中。
- 把模型内部状态暴露给上层。

### 8.4 `crates/brain`

负责语义连续性。

应该：

- 稳定 prompt surface。
- 传递 session/turn/tool/worker continuation ids。
- 在 executor/provider 边界记录 tool exact replay receipt。
- 消费 provider telemetry。
- 在 run trace / panel event 中暴露 continuation 状态。
- 上下文不足时返回结构化 runtime blocker。
- 选择直接处理、单 worker、多 worker或后台任务姿态。

不应该：

- 管理模型内部缓存文件。
- 保存后端 payload。
- 为了“续跑”改变用户语义。

### 8.5 `apps/gateway`

负责产品生命周期和面板体验。

应该：

- 前台快速返回和后台任务状态。
- 模型加载、卸载、关闭时的生命周期管理。
- 聊天/任务详情展示进度、miss reason、frontier、artifact refs。
- 保证 Windows 原生路径可用，WSL bridge 只是测试路径。

不应该：

- 保存模型内部缓存 payload。
- 解析 backend cache 文件。
- 硬编码天气、股票、小说、论文等任务特例。
- 把运行时临时文件作为知识库材料给 LLM 读取。

### 8.6 其他 crate

| crate | 适合承接 | 不适合承接 |
| --- | --- | --- |
| `builtin-tools` | 工具自治 context package、ToolOutcome、receipt、artifact ref、工具自有 policy。 | 系统级 continuation、模型内部缓存。 |
| `compression` | 稳定摘要和 preview 支撑。 | 工具自治 context package、provider continuation 或工具路由。 |
| `knowledge` / `memory-*` / `engram` | 语义记忆、知识、素材、可检索摘要。 | 保存运行时内部状态或工具临时全文。 |
| `state` | task、frontier、checkpoint、artifact、runtime event、receipt 元数据。 | 大块模型内部状态。 |
| `runtime-policy-core` | 系统级 policy：预算、审批、后台化、工作区边界、验证要求。 | 浏览器/搜索/写作等工具专属 policy。 |
| `loop-guard` | 消费 tool receipt/telemetry，判断重复工具调用、无进展循环。 | provider session 或模型缓存。 |
| `orchestrator` | 显存/上下文/资源压力治理。 | 语义编排和工具业务。 |
| `telemetry` | trace、progress、context error、真实回归观测。 | 改变运行行为。 |
| `security` | 路径安全、隐私、审计、清理策略。 | 推理续跑算法。 |

## 9. 数据结构

### 9.1 Provider Continuation Capability

当前非 KV 形态：

```rust
pub struct ProviderContinuationCapability {
    pub tool_call_exact_replay: bool,
    pub protocol_live_continuation: bool,
    pub thinking_final_split: bool,
    pub structured_context_errors: bool,
}
```

### 9.2 Continuation Hint

```rust
pub struct ContinuationHint {
    pub user_session_id: Option<String>,
    pub turn_id: Option<String>,
    pub worker_run_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub artifact_id: Option<String>,
    pub previous_response_id: Option<String>,
    pub continuation_frontier_id: Option<String>,
    pub visible_prompt_fingerprint: Option<String>,
}
```

### 9.3 Continuation Telemetry

当前非 KV 形态：

```rust
pub struct ContinuationTelemetry {
    pub mode: String,
    pub cache_source: String,
    pub prompt_tokens: Option<u32>,
    pub prefill_ms: Option<u64>,
    pub decode_ms: Option<u64>,
    pub miss_reason: Option<String>,
    pub tool_exact_replay_used: bool,
    pub protocol_live_continuation_used: bool,
}
```

说明：这里的 `cache_source` 是历史字段名，后续可以迁移成更准确的 `context_source` / `continuation_source`。当前不要再用它表达 KV 命中。

### 9.4 Tool Replay Receipt

```rust
pub struct ToolCallReplayReceipt {
    pub tool_call_id: String,
    pub replay_mode: String,
    pub sampled_call_fingerprint: String,
    pub sampled_call_ref: String,
    pub normalized_call_fingerprint: String,
}
```

sampled block 较大或敏感时保存 sidecar ref，不进入普通日志。

### 9.5 Context Limit Error

```rust
pub struct ContextLimitError {
    pub prompt_tokens: u32,
    pub configured_context_tokens: u32,
    pub requested_output_tokens: u32,
    pub overflow_tokens: u32,
    pub largest_section: Option<String>,
    pub recommended_actions: Vec<String>,
}
```

### 9.6 ToolOutcome / Receipt

统一工具结果应逐步收敛到：

```text
status
kind
preview
artifact_ref
evidence
error_class
fingerprint
receipt_id
```

executor 层负责统一包装，避免每个工具都手写一套不同 receipt。

## 10. 请求生命周期

### 10.1 普通聊天

```text
brain 判断直接处理 / 单 worker / 多 worker / 后台任务
构造当前轮必要 prompt，并排除无关历史和旧产物
provider 执行模型调用
telemetry 返回给 brain/gateway
必要摘要、frontier、artifact ref 写入 state
final answer 返回用户
```

### 10.2 工具调用

```text
模型生成 tool call
executor/provider 记录 sampled tool block receipt
工具执行
工具结果生成 ToolOutcome / receipt
大结果写 artifact
下一轮用 receipt / artifact_ref / frontier 接回
重复或无进展时 loop guard 介入
```

### 10.3 Worker delegate

```text
主 agent 判断需要 worker
创建 worker_run_id
传递最小任务合同/context refs/continuation hint
worker 使用自身装备工具执行
worker 返回 artifact/tool receipt/progress
主 agent 把 worker frontier 接回当前 turn
```

### 10.4 长任务

```text
前台快速返回任务已进入后台
后台按 step/chapter/section/tool run 推进
每步写 progress event
正文和大结果进 artifact
系统层 summary/frontier/artifact refs 稳定 revision
工具层 truth/context package 由工具自己维护
用户追问时系统读取 frontier/receipt/artifact ref，再由对应工具读取自己的 context package
```

## 11. 面板体验

面板不需要暴露复杂内部结构，但要让用户感受到系统在连续执行。

聊天界面应该显示：

- 正在等待模型。
- 正在调用工具。
- 正在等待 worker。
- 正在写入 artifact。
- 当前步骤/章节/阶段。
- 简短摘要。
- 文件路径。
- 是否后台继续。
- blocker 或下一步。

任务详情页可以显示：

- provider continuation mode。
- continuation source。
- miss reason。
- prefill/decode 时间。
- worker_run_id。
- frontier id。
- artifact refs。
- 工具自治 context package 路径或摘要。

不再显示：

- cached tokens。
- suffix tokens。
- disk payload status。
- disk payload reason。
- disk payload version。

## 12. Windows 原生约束

- 运行路径来自 panel/runtime 配置，不写死。
- 面板关闭 gateway 时，项目相关进程应一起关闭，除非用户明确最小化/后台运行。
- WSL bridge 只能作为测试路径，不是产品前提。
- 文件句柄要短，避免 Windows 下无法删除/rename。
- 清理入口必须可用。
- 用户配置尽量通过面板完成，不写死到代码或脚本里。

## 13. 安全与隐私

Continuation Runtime 可能引用用户 prompt、工具结果摘要、知识片段和产物路径。

要求：

- 默认本地保存。
- 不上传。
- 不把运行时临时内容自动写入知识库。
- 不作为 artifact 暴露给 LLM 自由读取。
- 支持按任务、session、worker、agent 清理。
- telemetry 只记录必要 fingerprint/token count/status，不记录隐藏推理文本。

## 14. 升级状态

### 14.0 当前落地标记

| 阶段 | 状态 | 当前动作 |
| --- | --- | --- |
| Phase 1: 合同和 telemetry | 已完成，KV 字段已清理 | `provider-core` 保留非 KV continuation capability/hint/telemetry/context-limit 合同。 |
| Phase 2: prompt/context 治理 | 已完成一部分 | prompt surface 统计、context budget、语言/输入识别、前台观察窗等已落地；通用层必要上下文构造与工具自治 package 仍需分界收敛。 |
| Phase 3: 上下文错误显式化 | 已完成一部分 | `ContextLimitError` 已成为 provider-core 结构化合同，brain/gateway 需要继续完善用户可见动作。 |
| Phase 4: Tool exact replay receipt | 已完成一部分 | executor 已生成 replay receipt，OpenAI-compatible provider 会优先保留 sampled replay block。 |
| Phase 5: Worker frontier | 已完成一部分 | reasoner 已生成 session/turn/worker/frontier hint，delegate worker 使用独立 worker session。 |
| Phase 6: 大结果 artifact 化 | 已完成一部分 | 工具输出压缩、result truncation、artifact ref 基础已在推进，仍需扩大覆盖面。 |
| Phase 7: 面板产品化 | 已完成一部分 | 前台快速返回、后台任务、trace 展示已有基础；进度展示和追问体验仍需继续打磨。 |
| Phase 8: 写作/长产物连续性 | 工具自治层已完成一部分 | writing/novel_studio 已有 project/truth/summary/export 机制；这属于写作工具治理，不代表系统通用 context package 已完成。 |
| Phase 9: 真实回归矩阵 | 持续执行 | 每阶段需要真实 gateway/panel 回归，不用 mock 作为最终证明。 |
| Phase X: 磁盘 KV / backend payload | 已撤销 | 删除 disk prefix cache、payload export/import、KV telemetry 和能力声明。 |

### 14.1 2026-05-20 真实回归记录

本轮使用真实 gateway `/api/chat` 和本地 Windows llama.cpp bridge 回归，未使用 mock。

环境记录：

- gateway: `http://127.0.0.1:3000`
- WSL 测试桥: `http://172.18.176.1:28013/v1`
- model alias: `benshu-main-brain`
- 前台观察窗: 约 5 秒，超出后进入后台任务。

执行结果：

- 普通聊天 20 轮: 20/20 完成；无工具调用；平均总耗时约 10.97 秒；第 20 轮同 session 总结前文耗时约 35.08 秒，说明多轮历史会显著增加本地模型响应时间。
- 简单实时查询 20 轮: 初测 18/20 完成；失败项为中文“纽约”天气地理编码失败、泛化“重要时事新闻”超时。
- 修复后问题项复测 5/5 完成：纽约天气、伦敦天气、以太坊价格、重要时事新闻、科技新闻均走真实工具并返回来源。

本轮代码修正：

- `realtime_lookup` 天气地理编码增加本地语言地名 fallback，避免中文地名在 Open-Meteo geocoding 中空结果或误匹配。
- `realtime_lookup` 新闻类查询对广义新闻/分类新闻优先 RSS，并过滤明显旧年份来源，减少“最新新闻”返回陈旧网页。
- routing 价格查询规则补齐“数量问句 + 可识别资产/指数/股票目标 => price_lookup”，覆盖“现在以太坊多少钱？”这类自然语言。

仍需继续验证：

- 新闻内容是否足够贴合中文用户语义，目前短实时新闻可给来源，但来源标题可能仍以英文为主。
- 前台 5 秒转后台对普通聊天体验偏硬；简单问答多数会在后台完成，而不是前台直接吐出最终答案。
- 写作、PDF/论文 artifact、gateway 重启恢复、模型卸载/关闭、上下文超限错误仍未在本轮完成完整矩阵。

### Phase 1: 合同和 telemetry

- [x] `provider-core` 增加 continuation capability/hint/telemetry。
- [x] `providers` 映射已有协议能力。
- [x] `brain` 将 telemetry 标准写入 run trace。
- [x] 清理 KV/disk payload 相关字段。

### Phase 2: prompt/context 治理

- [x] prompt static/dynamic/governance/tool surface 统计。
- [x] 语言识别和用户语言合同。
- [x] runtime context budget 基础。
- [x] 前台快速返回观察窗。
- [ ] 当前轮必要上下文构造在所有入口统一。
- [ ] 工具自治 context package 只通过 receipt / artifact_ref / summary 暴露给系统层。
- [ ] 输出合同动态预算继续收敛。

### Phase 3: 上下文错误显式化

- [x] 上下文超限返回结构化错误。
- [x] provider-core 定义 `ContextLimitError`。
- [ ] brain 将所有上下文错误统一转为可恢复 blocker。
- [ ] gateway/面板展示专用上下文错误状态。
- [ ] 禁止静默裁剪后声称完成。

### Phase 4: Tool exact replay receipt

- [x] executor/provider 边界记录 sampled tool block。
- [x] 工具结果回合优先 exact replay。
- [x] canonical fallback 只做兜底。
- [ ] ToolOutcome envelope 覆盖所有工具结果。

### Phase 5: Worker frontier

- [x] 增加 worker_run/frontier 标识。
- [x] delegate/handover/continuous task 传递 continuation hint。
- [x] 多 worker 共享 provider 时隔离 frontier。
- [ ] 重启后的 frontier 恢复和面板追问体验继续完善。

### Phase 6: 大结果 artifact 化

- [x] 工具输出进入上下文前二次 compression guard。
- [x] `ToolCallData` 记录 result truncation 元数据。
- [x] 写作章节默认保留内部 markdown，并同步导出用户友好的 txt。
- [ ] executor 统一 oversized spill 到 artifact ref。
- [ ] 网页、PDF、长文本、代码结果统一 preview + artifact_ref。

### Phase 7: 面板产品化

- [x] 前台快速返回 + 后台任务基础。
- [x] 面板 telemetry card 去掉 KV/disk payload 展示。
- [x] 聊天内容虚拟滚动基础。
- [ ] 进度事件更自然地展示给用户。
- [ ] 任务详情页支持按 artifact/frontier 追问。

### Phase 8: 写作/长产物连续性

- [x] writing 工具有自己的 project/truth/summary/context/export 机制。
- [x] 明确 writing context package 属于工具自治层，不属于系统通用层。
- [x] 章节工作稿保留，用户侧同步 txt 导出。
- [x] 每次默认不越界自动生成过多章节。
- [ ] 标题/人物随机性和用户指定优先级继续验证。
- [ ] 每章长度合同、审稿修订循环、伏笔债务、truth 反校验继续真实回归。

### Phase 9: 真实回归矩阵

必须覆盖：

- [x] 普通聊天 20 轮。
- [x] 简单实时查询 20 轮。
- [x] 天气/价格/新闻等短实时任务。
- 工具调用后续接。
- worker delegate。
- 写作连续任务。
- PDF/论文 artifact。
- gateway 重启恢复。
- 模型卸载/关闭。
- 上下文超限错误。

## 15. 测试要求

### 15.1 单元/集成测试

- capability parsing。
- continuation hint serialization。
- telemetry roundtrip。
- tool replay receipt lookup。
- canonical fallback。
- context limit error。
- ToolOutcome envelope。
- worker frontier metadata。
- artifact ref 写入和读取。

### 15.2 真实 panel/gateway 回归

必须覆盖：

- 普通聊天 20 轮。
- 简单实时查询 20 轮。
- 工具调用后续接。
- delegate worker 工具调用。
- 长 artifact 写入。
- 写作项目多章节。
- 网关重启后继续同 session。
- 模型卸载后进程关闭。
- 上下文错误可见。

## 16. 升级收益

- 工具调用后更少重复 delegate/重复调用。
- worker 任务更容易连续。
- 写作/论文/代码等长任务不依赖聊天历史全文。
- 面板能解释任务进展。
- 上下文不足不再静默导致漂移。
- 本地模型慢/快原因更容易诊断。
- 大内容不会默认撑爆聊天框和短期记忆。

## 17. 代价和风险

### 17.1 错误连续风险

风险：

- 工具 receipt 接错任务。
- worker 继续了上一个任务的 artifact。
- 旧 artifact 被当成新任务上下文。
- output contract 和用户最新意图不匹配。

缓解：

- session/turn/worker/artifact/frontier id 必须匹配。
- 用户新任务默认不复用旧产物内容；只有同 session 且用户明确指代时才携带相关上下文。
- artifact ref 必须带 task/session/project 归属。
- 无法确认时返回 blocker，而不是自动继续。

### 17.2 Prompt 不稳定导致效果差

风险：

- 每轮系统提示、工具列表、摘要、worker 描述顺序变化。
- 压缩层每次重写摘要文本。
- 工具结果 canonicalize 后字节不一致。

缓解：

- prompt surface 分 static/dynamic/governance/tool surface。
- 工具渲染排序稳定。
- 摘要加 `summary_id` / `revision`，未变化时保持稳定引用。
- 大正文、大网页、大 PDF 不进聊天历史，进入 artifact/knowledge；工具自治 context package 不直接混入系统通用层。

### 17.3 旧内容污染风险

风险：

- 旧测试产物被新任务误用。
- 自动经验系统把旧内容当方法长期套用。
- 知识库和素材库边界不清。

缓解：

- 删除自动进化内容套用。
- 只在同 session 或用户明确引用时使用旧产物。
- 经验只保留可审计方法，不自动注入前台 prompt。
- 运行时临时文件不进入知识库。

### 17.4 性能收益边界

取消 KV 复用后，不能期待模型内部 prefill 级加速。

仍然可以获得：

- 更少无关上下文。
- 更少重复工具调用。
- 更少重复 delegate。
- 更稳定的长任务上下文。
- 更低的聊天历史膨胀。

## 18. 当前结论

BenShu 不再走磁盘 KV 或对外 KV 复用路线。

后续优化重点是：

- 少塞无关上下文。
- 稳定引用摘要、artifact、truth、receipt 和 frontier。
- 让工具边界可审计。
- 让后台任务可观察、可追问、可恢复。
- 用真实面板回归验证每个机制是否真的改善用户体验。
