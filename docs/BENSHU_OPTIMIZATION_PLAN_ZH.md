# BenShu 优化计划（中文）

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 测试链口径: 本文所有性能热点、时延与吞吐判断，默认以 `GPU 优先测试链` 为基准；`CPU` 结果只用于 fallback、诊断与保底兼容验证，不直接作为主路径性能结论。

> 关联总规范: [DEVELOPMENT_STANDARDS_AGENTOS.md](/home/biubiuboy/BenShu/docs/DEVELOPMENT_STANDARDS_AGENTOS.md)

> 文档定位: 这是面向 `BenShu` 全仓性能压榨与内存/带宽优化的专题计划，目标是系统性识别哪些 crate 值得做零拷贝或近零拷贝改造，以及哪些地方存在值得处理的复杂度与数据搬运问题，并给出优先级、落点与实施顺序。
>
> 本文不追求“所有 clone 都消灭”，而是优先解决：
>
> - 大模型推理主路径
> - 向量/存储主路径
> - 大文件/媒体/文档处理路径
>
> 当前结论先行:
>
> - 最值得优先改造的 crate 是：
>   - `inference`
>   - `engram`
>   - `builtin-tools`
>   - `security`
>   - `sensory`
>   - `providers`
> - `brain` 整体仍不是第一主战场，但完整 `Agent` 通道里已经出现一组值得单独推进的近零拷贝热点：
>   - `foreground_runtime`
>   - `context`
>   - `reasoner`
>   - `run_trace_builder`
> - 真正的核心主战场是：
>   - `crates/inference`
>   - `crates/engram`
>   - `crates/builtin-tools`
> - 对主脑 `KV cache` 路线来说，零拷贝/近零拷贝不是锦上添花，而是后续真实 `llama.cpp KV live` 集成能否接近论文级收益的关键条件。

---

## 0. 文档目标

### 0.0 与开发准则的关系

这份文档不是独立规范，而是：

- 面向性能、零拷贝、复杂度和结构收口的专项执行计划
- 必须严格服从 [DEVELOPMENT_STANDARDS_AGENTOS.md](/home/biubiuboy/BenShu/docs/DEVELOPMENT_STANDARDS_AGENTOS.md)

如果两者出现冲突，必须以开发准则为准，尤其必须严格遵守下面这些约束：

1. **生产优先，而非演示优先**
- 不能为了跑分或局部 benchmark，破坏主路径稳定性、可恢复性、可审计性

2. **显式边界，高于隐式魔法**
- 不能为了减少一次 clone 或一次函数调用，就把关键语义重新塞回隐式全局、环境变量、隐藏缓存或跨层穿透依赖

3. **单一职责，但允许高内聚**
- 性能优化不能成为继续往热点大文件堆代码的理由
- 结构收口阶段，性能改造也必须优先服从模块归位

4. **主路径优先于旁路能力**
- 优化必须优先落在主工厂、主 API、主 UI、主运行时路径
- 不允许只在旁路实验入口做快，而主路径仍然不受益

5. **对硬编码保持极高警惕**
- 阈值、策略、路由、tool surface、allocator、hasher 选择若可能演化为配置，不应轻率写死

6. **所有机制都必须服务于用户**
- 不能为了“更快”而牺牲正确性、解释性、审批、安全或恢复语义

一句话：

**本优化计划的所有行动，都必须在不破坏 AgentOS 主线机制的前提下进行。**

### 0.1 状态标记

- `[x]` 已识别
- `[~]` 部分具备基础
- `[ ]` 尚未改造

### 0.2 本文回答的问题

- 哪些 crate 值得做零拷贝/近零拷贝改造
- 哪些文件是高收益热点
- 哪些地方只是“代码洁癖式减少 clone”，哪些地方是真正的性能瓶颈
- 应该按什么顺序推进

### 0.3 本文不回答的问题

- 不逐项追究每一个 `String::to_string()` 是否值得优化
- 不把“少一次小拷贝”误当成系统级性能改造
- 不承诺第一轮就让所有 I/O 和序列化都变成严格零拷贝

### 0.3.1 严格遵则的具体含义

对这份文档来说，“严格遵则”不是口号，而是下面这些具体约束：

- 不允许为了性能绕过：
  - `hardness`
  - `truth / verification`
  - `background compression`
  - `memory authority`
  - `trace / witness / scorecard`
- 不允许为了性能把主路径能力偷偷挪到旁路实现里
- 不允许为了性能继续把主路径实现堆回：
  - `foreground_runtime.rs`
  - `skills/tool/mod.rs`
  - `middleware.rs`
  - `tactical.rs`
  这类历史热点文件
- 不允许为了性能引入无法解释、无法回退、无法观测的“黑魔法优化”
- 不允许把“临时试验性提速”写成稳定主路径行为而不在文档中补边界说明

### 0.4 零拷贝主线摘要

这份文档虽然已经升级成“优化计划”，但其中的零拷贝 / 近零拷贝主线并没有消失。

当前零拷贝主线的核心对象仍然非常明确：

1. `crates/inference`
- 目标：让主脑 `KV / page / quant / tensor view` 这类真正吃大块数据的路径减少 materialize、减少 shadow copy、逐步向共享 backing store 靠拢

2. `crates/engram`
- 目标：让 blob / vector / document storage 尽量维持 cheap-clone bytes 或 borrowed-first 读路径，减少 `copy_from_slice` 和中间 `Vec<f32>`

3. `crates/builtin-tools`
- 目标：让 PDF / 图片 / 网页 / 媒体类工具尽量走 artifact ref、bytes、流式引用，而不是大 base64 / 大 payload 内联

4. `crates/brain`
- 目标：不是把所有消息对象做成“严格零拷贝”，而是在完整通道热路径里减少：
  - 整段历史 `to_vec()`
  - 背景整对象 clone
  - trace 为少量字段 clone 全背景

一句话：

**零拷贝主线现在仍然存在，只是被放回了更大的优化框架里，并且优先级已经被重新排成“先救当前体验，再做深层底座”。**

### 0.5 与 `BENSHU_LOCAL_FAST_CHAT_REFACTOR_PLAN_ZH.md` 的边界

这份文档与 [BENSHU_LOCAL_FAST_CHAT_REFACTOR_PLAN_ZH.md](/home/biubiuboy/BenShu/docs/BENSHU_LOCAL_FAST_CHAT_REFACTOR_PLAN_ZH.md) 有交叉，但不属于重复文档。

边界应当明确为：

- `BENSHU_LOCAL_FAST_CHAT_REFACTOR_PLAN_ZH.md`
  - 解决：
    - `Fast Chat / Full Agent Chat`
    - `hardness gate`
    - 工具面缩面
    - prompt profile
    - 路由与通道升级

- `BENSHU_OPTIMIZATION_PLAN_ZH.md`
  - 解决：
    - 零拷贝 / 近零拷贝
    - 复杂度热点
    - 内存增长风险
    - 数据搬运
    - 热点大文件结构收口

两者确实有交集，主要集中在：

- 完整通道为什么慢
- `brain` 热路径为什么值得优化
- 工具面和背景层为什么会影响 prompt 体积

但它们的职责不同：

- 前者是“系统行为与运行通道重构”
- 后者是“性能与实现层优化”

因此当前结论是：

**两份文档存在必要交叉，但还没有到应该合并成一份的程度。**

---

## 1. 判断标准

### 1.1 什么叫值得做

只有满足下面至少一条，才值得进入零拷贝改造主线：

- 处于主脑推理热路径
- 涉及大块 `Vec<u8>` / `Vec<f32>` / 图像 / 音频 / 文档页数据
- 当前实现存在明显的“读整块 -> 再复制 -> 再编码/序列化”的链路
- 复制本身会吃掉本来想省下来的内存/带宽收益
- 零拷贝/近零拷贝能直接放大：
  - 吞吐
  - 内存占用
  - 上下文上限
  - 家用机器承载能力

### 1.2 什么叫不值得优先做

下面这些当前不应排在前面：

- 小对象 `String` 拼接
- 低频配置读写
- 管理型 metadata clone
- 与实际数据体量无关的语义对象复制

一句话：

**我们优先处理“大数据块主路径”，而不是“代码看起来不够优雅”的小复制。**

---

## 2. 全仓优先级总览

### 2.1 S 级：必须优先看

1. `crates/inference`
2. `crates/engram`
3. `crates/builtin-tools`

### 2.2 A 级：很值得做

4. `crates/security`
5. `crates/sensory`
6. `crates/providers`

### 2.3 B 级：可做但不优先

7. `crates/comm`
8. `crates/brain`

### 2.4 C 级：当前不值得优先投入

- `auth`
- `connectors`
- `kernel`
- `knowledge`
- `mcp`
- `orchestrator`
- `scheduler`
- `state`
- `telemetry`

---

## 3. 按 Crate 的改造清单

## 3.0 性能库兼容性与落位前提

在继续推进性能改造前，需要先确认“引入哪些库不会和现有 crate 体系冲突”。当前工作区的实际情况如下：

- `bytes`：已经在 workspace 中统一声明，并且已在 `brain / providers / engram` 中使用
- `rayon`：已经在 workspace 中声明，并且已在 `brain / engram` 中使用
- `simd-json`：已经在 workspace 中声明，并在 `brain` 中启用
- `brain` 当前还额外使用：
  - `fxhash`
  - `seahash`
- 当前未见正式引入：
  - `mimalloc`
  - `smallvec`
  - `compact_str`
  - `ahash`
  - `rustc-hash`
  - `simdutf8`

### 3.0.1 当前判断：不存在立即冲突

按现有 crate 边界与依赖图，下面这些库可以被纳入优化候选，而不会直接与当前体系冲突：

- `mimalloc`
- `smallvec`
- `compact_str`
- `ahash` 或 `rustc-hash`
- `simdutf8`

原因很简单：

- 它们当前都还没有被仓库主线正式依赖
- 不会直接和：
  - `tokio`
  - `bytes`
  - `rayon`
  - `serde_json`
  - `redb`
  - `llama-cpp`
  - `egui`
  这些核心依赖形成硬冲突

### 3.0.2 真正需要注意的地方

真正要避免的不是“库名冲突”，而是“同一热点里混用多套策略”：

- `HashMap/HashSet` 热点里不要同时混用：
  - `fxhash`
  - `ahash`
  - `rustc-hash`
- 小字符串热点如果引入 `compact_str`，应先限定到：
  - metadata
  - labels
  - route tags
  不要一上来扩散到所有跨 crate API
- `mimalloc` 这类全局 allocator 应放在应用层验证：
  - `gateway`
  - `panel`
  - 单独基准
  而不是先在每个 crate 里零散尝试

一句话：

**当前不存在“不能加”的硬冲突，真正的约束是要尊重现有 crate 边界，不在同一热点里引入互相重叠的优化策略。**

### 3.0.3 建议采用的性能库落位

#### A. 直接复用现有依赖，不再重复引库

- `bytes`
  - 主要继续落在：
    - `providers`
    - `engram`
    - `gateway/connectors`
- `rayon`
  - 主要继续落在：
    - `engram`
    - `brain` 中离线或批量处理路径
- `simd-json`
  - 只在 JSON 解析真是热点的路径继续推进
  - 不建议强行扩散到所有 crate

#### B. 新增但低冲突、值得优先试验

- `smallvec`
  - 最适合：
    - `brain`
    - `builtin-tools`
    - `gateway`
  - 典型对象：
    - recent window
    - 小 tool set
    - evidence refs
    - 短小消息片段列表

- `compact_str`
  - 最适合：
    - `brain`
    - `telemetry`
    - `gateway`
  - 典型对象：
    - metadata key/value
    - route labels
    - session tags
    - background decision/status 字段

- `simdutf8`
  - 最适合：
    - `providers`
    - `gateway`
    - `connectors`
  - 典型对象：
    - 高频文本入口
    - HTTP/streaming 文本边界

#### C. 需要先做局部策略统一，再引入

- `ahash` 或 `rustc-hash`
  - 只能二选一作为“新的内部热点 hasher”
  - 不建议和当前 `fxhash / seahash` 在同一类热点里继续叠加
  - 如果推进，优先落：
    - `brain` 热路径小型 `HashMap/HashSet`
    - `gateway` 内部短生命周期 map

#### D. 应用层验证后再决定是否全仓推广

- `mimalloc`
  - 不应先从 crate 内部散点引入
  - 应先在：
    - `apps/gateway`
    - 可能的话也包括 `apps/panel`
    做 A/B 压测
  - 观察：
    - 分配吞吐
    - RSS
    - 长会话稳定性

### 3.0.4 按 crate 的推荐落位

- `crates/brain`
  - 优先：
    - `smallvec`
    - `compact_str`
  - 谨慎推进：
    - 单一新 hasher（`ahash` 或 `rustc-hash`）

- `crates/providers`
  - 优先：
    - 继续用 `bytes`
    - `simdutf8`

- `crates/engram`
  - 优先：
    - 继续用 `bytes`
    - 继续用 `rayon`
  - 暂不优先：
    - `compact_str`

- `crates/inference`
  - 当前更重要的是：
    - 数据布局
    - scratch buffer
    - arena/backing store
  - 不是优先靠这些通用性能库解决

- `apps/gateway`
  - 优先：
    - `bytes`
    - `simdutf8`
  - 试验性：
    - `mimalloc`

- `apps/panel`
  - 不应引入太重的后端性能依赖
  - 如果要尝试：
    - `mimalloc`
  - 但要先看实际收益

## 3.1 `crates/inference` `优先级: S`

### 为什么它是第一优先级

这是主脑推理内核所在位置。

如果这里还是：

- 影子页复制
- 大块 `Vec<u8>` 重建
- 压缩前后多次 materialize

那后面即使算法方向对，也很难接近真正的论文级收益。

### 关键热点

- [llama_cpp.rs](/home/biubiuboy/BenShu/crates/inference/src/backend/llama_cpp.rs)
- [engine.rs](/home/biubiuboy/BenShu/crates/inference/src/engine.rs)
- [quant.rs](/home/biubiuboy/BenShu/crates/inference/src/quant.rs)
- [onnx_runtime.rs](/home/biubiuboy/BenShu/crates/inference/src/backend/onnx_runtime.rs)

### 当前问题

#### 1. `llama_cpp.rs`

当前 `KV conservative prototype` 还是：

- 用 `KvEngine` 分配阴影页
- 填充 shadow 数据
- 压缩 shadow 页
- 再计算 `projected_*`

这说明当前是：

- **有压缩潜力证明**
- **没有真实 `KV live` 接管**

因此它不是零拷贝，而是“影子复制 + 估算收益”。

#### 2. `engine.rs`

当前页结构：

- `k_data: Vec<u8>`
- `v_data: Vec<u8>`

压缩时会：

- `fp16_bytes_to_f32(...)`
- 重新量化
- 再生成新的 `Vec<u8>`

这意味着：

- 数据页不是共享 backing store
- 压缩不是 in-place
- 也不是 metadata-only 状态切换

#### 3. `quant.rs`

现在有这类路径：

- `f.to_le_bytes().to_vec()`
- 编码时大量新分配
- 解码时重建 `Vec<f32>`

它们是明显的中间复制热点。

#### 4. `onnx_runtime.rs`

现在有：

- `data[..hidden_size].to_vec()`

说明输出 tensor 视图在最终返回前被重新 materialize 成新 `Vec<f32>`。

### 改造目标

- 真实 `KV live` 接管
- page/block 级共享 backing store
- 尽量向：
  - in-place quantize
  - overlay metadata
  - borrowed tensor view
  - reusable scratch buffer
  靠拢

### 建议改造顺序

1. `llama_cpp.rs`
   - 先把 shadow prototype 向真实 `KV live` 接管推进
2. `engine.rs`
   - 把页结构从“独立 `Vec<u8>`”推进到“共享 backing + 压缩视图”
3. `quant.rs`
   - 改预分配和直接写入
4. `onnx_runtime.rs`
   - 改 borrowed view / delayed materialization

### 收益预期

- 真实降低 `KV live`
- 降低 `context_pressure`
- 减少 `trims / resets`
- 为家用机器承载更大模型打基础

---

## 3.2 `crates/engram` `优先级: S`

### 为什么它是第二优先级

这是向量、文档、CAS、索引的存储层。

它本来就已经有较好的基础：

- `Bytes`
- 存储 trait 抽象

所以继续往零拷贝走，收益高、阻力也相对小。

### 关键热点

- [storage/mod.rs](/home/biubiuboy/BenShu/crates/engram/src/storage/mod.rs)
- [storage/redb_impl.rs](/home/biubiuboy/BenShu/crates/engram/src/storage/redb_impl.rs)
- [storage/in_memory.rs](/home/biubiuboy/BenShu/crates/engram/src/storage/in_memory.rs)
- [vector_store.rs](/home/biubiuboy/BenShu/crates/engram/src/vector_store.rs)

### 当前问题

#### 1. 接口层已经友好，但落地实现还在复制

[storage/mod.rs](/home/biubiuboy/BenShu/crates/engram/src/storage/mod.rs) 已经大量使用 `Bytes`，这是好地基。

但 [redb_impl.rs](/home/biubiuboy/BenShu/crates/engram/src/storage/redb_impl.rs) 里仍大量：

- `Bytes::copy_from_slice(...)`

这意味着：

- 上层接口想便宜 clone
- 但底层每次读出来都重新拷了一遍

#### 2. 向量解码会重复 materialize

`get_vector_f32()` 现在会：

- 从 bytes 拆 4 字节 chunk
- 重建新的 `Vec<f32>`

这对大批量检索路径不是理想状态。

#### 3. in-memory store 也在大量复制

[in_memory.rs](/home/biubiuboy/BenShu/crates/engram/src/storage/in_memory.rs) 里有很多：

- `Bytes::copy_from_slice`

虽然这是内存实现，但如果把它用于频繁测试或高频路径，仍然存在可优化空间。

### 改造目标

- 底层存储尽量返回 borrowed 或 cheap-clone bytes
- 向量反序列化延后
- 能不立刻 `Vec<f32>` 就不立刻 `Vec<f32>`
- 让文档、向量、内容 blob 在 store 层尽量只 materialize 一次

### 建议改造顺序

1. `redb_impl.rs`
2. `storage/mod.rs`
3. `vector_store.rs`
4. `in_memory.rs`

### 收益预期

- 降低存储读路径复制成本
- 降低 ANN / rerank 前的数据搬运
- 提升文档和向量主线整体吞吐

---

## 3.3 `crates/builtin-tools` `优先级: S`

### 为什么它是第三优先级

这里有大量：

- PDF
- 图片
- 网页
- 媒体
- 命令输出

这类“大块数据处理”路径。

### 关键热点

- [pdf_parse.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/pdf_parse.rs)
- [web_fetch.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/web_fetch.rs)
- [browser.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/browser.rs)
- [media_runtime.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/media_runtime.rs)

### 当前问题

#### 1. `pdf_parse.rs`

存在典型热点：

- 图片 bytes -> base64 data URL
- artifact/read/write 多次 materialize
- 页图像和富文本结构可能反复重编码

这对大 PDF 来说是明显的额外内存/CPU 消耗。

#### 2. 大数据经常以内联文本返回

对工具层来说，很多时候真正该传的是：

- artifact ref
- path/ref id
- borrowed bytes

而不是：

- 大 base64
- 整块字符串内联

### 改造目标

- `artifact ref` 优先于大 base64 内联
- `Bytes` 优先于 `Vec<u8>` 重复复制
- 图像/文档页走流式/引用式返回
- 能走本地 artifact contract 的就不走内嵌大 payload

### 建议改造顺序

1. `pdf_parse.rs`
2. `web_fetch.rs`
3. `media_runtime.rs`
4. `browser.rs`

### 收益预期

- 大文档/图片工具内存占用明显下降
- 降低 panel/tool contract 中的无意义内联开销
- 对 PDF/视觉工具收益最大

---

## 3.4 `crates/security` `优先级: A`

### 为什么值得做

安全层里最值得做的是大文件备份/恢复，不是小字符串。

### 关键热点

- [memory_backup.rs](/home/biubiuboy/BenShu/crates/security/src/memory_backup.rs)
- [vault.rs](/home/biubiuboy/BenShu/crates/security/src/vault.rs)
- [encryption.rs](/home/biubiuboy/BenShu/crates/security/src/encryption.rs)

### 当前问题

#### 1. `memory_backup.rs`

当前模式是：

- `tokio::fs::read`
- 整块加密
- 整块写回

这对大文件很重。

### 改造目标

- streaming hash
- streaming encrypt/decrypt
- 避免大文件整块入内存

### 收益预期

- 备份/恢复峰值内存显著下降
- 对大内存仓或大持久化文件更友好

---

## 3.5 `crates/sensory` `优先级: A`

### 为什么值得做

音频天然是大块连续数据，最怕：

- `input.to_vec()`
- `vec![input.to_vec()]`
- 上传前整块复制

### 关键热点

- [audio/mod.rs](/home/biubiuboy/BenShu/crates/sensory/src/audio/mod.rs)
- [audio/cloud.rs](/home/biubiuboy/BenShu/crates/sensory/src/audio/cloud.rs)
- [vision/ocr.rs](/home/biubiuboy/BenShu/crates/sensory/src/vision/ocr.rs)

### 当前问题

`resample_to_16k()` 里现在有：

- `input.to_vec()`
- `vec![input.to_vec()]`

这是典型可优化点。

### 改造目标

- reusable buffer
- slice/view 优先
- 长音频上传尽量减少整块复制

### 收益预期

- STT/TTS/音频预处理峰值内存下降
- 对长音频收益明显

---

## 3.6 `crates/providers` `优先级: A`

### 为什么值得做

provider 层是网络流式入口，如果这里每次都：

- 拷贝网络块
- 再转字符串

那长流输出也会有额外损耗。

### 关键热点

- [utils.rs](/home/biubiuboy/BenShu/crates/providers/src/utils.rs)
- [openai.rs](/home/biubiuboy/BenShu/crates/providers/src/openai.rs)
- [anthropic.rs](/home/biubiuboy/BenShu/crates/providers/src/anthropic.rs)
- [gemini.rs](/home/biubiuboy/BenShu/crates/providers/src/gemini.rs)

### 当前问题

#### 1. `utils.rs`

`SseBuffer` 已经用了 `BytesMut`，这很好。

但仍有：

- `chunk.to_vec()`
- `String::from_utf8(...)`

说明还能少一次 materialize。

### 改造目标

- SSE chunk 尽量原地切片
- 延迟字符串化
- 图像/多媒体 payload 减少 base64 中转

### 收益预期

- 流式 provider 更稳
- 长输出与多媒体输入路径开销下降

---

## 3.7 `crates/comm` `优先级: B`

### 关键热点

- [protocol/mod.rs](/home/biubiuboy/BenShu/crates/comm/src/protocol/mod.rs)
- [transport/bus.rs](/home/biubiuboy/BenShu/crates/comm/src/transport/bus.rs)
- [client/mod.rs](/home/biubiuboy/BenShu/crates/comm/src/client/mod.rs)

### 当前问题

主要是：

- `serde_json::to_vec`
- payload materialize

### 判断

这更像：

- 协议优化
- 序列化次数优化

不是当前最值得先打的“零拷贝主战场”。

---

## 3.8 `crates/brain` `优先级: B`

### 关键热点

- [reasoner.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/reasoner.rs)
- [foreground_runtime.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/foreground_runtime.rs)
- [context.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/context.rs)
- [run_trace_builder.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/run_trace_builder.rs)
- memory facade / comm runtime 等消息链路

### 当前问题

主要是：

- `String`
- `Message`
- metadata
- summary/context clone

### 判断

这类优化更多是：

- 减少中小对象复制
- 降低 GC/分配噪音

但对“大模型推理性能压榨”不是第一优先。

### 需要补充收口的现实变化

上面这个判断对“主脑纯消息编排”依然成立，但对当前已经变胖的完整 `Agent` 通道来说，`brain` 里有一组复制热点已经不再只是“代码洁癖式减少 clone”。

尤其在下面这些路径里：

- 完整通道工具调用
- 背景压缩刷新
- session checkpoint / recover
- run trace / runtime metadata 投影

复制已经直接影响：

- 首轮 prefill 前的内存搬运
- 工具调用完成后的收尾延迟
- 长会话同 session 的额外 token 与对象构建成本

所以 `brain` 当前更准确的口径是：

- **整体仍是 B 级**
- **但完整通道 / 背景压缩热路径应按 B+ 处理**

### 当前真正值得纳入零拷贝计划的热点

#### 1. `foreground_runtime.rs`

在 [foreground_runtime.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/foreground_runtime.rs) 里，当前完整通道的热路径有两处非常典型的 materialize：

- `maybe_refresh_background(...)`
  - `let mut background_messages = messages.to_vec();`
- `finalize_outcome(...)`
  - `let mut persisted_messages = messages.to_vec();`

这说明当前在：

- 生成背景候选
- checkpoint 已完成会话

时，都会对整段历史做一次新的 `Vec<Message>` 复制。

这在：

- 工具调用后的第二段 LLM 推理
- 长 session 收尾

里已经是实打实的热路径成本。

#### 2. `context.rs`

在 [context.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/context.rs) 里，`filtered_background_envelope(...)` 当前会：

- `let mut filtered = envelope.clone();`

然后再做 recent-history 去重与 session-layer 裁剪。

这意味着：

- 每次 build context
- 即使只想裁掉最近消息里已经出现过的 backend object

也要先把整份 `BackgroundEnvelope` clone 一遍。

对长 session 背景层来说，这已经不是“小 metadata clone”。

#### 3. `reasoner.rs`

在 [reasoner.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/reasoner.rs) 里，当前完整通道仍有多处以 `Vec<Message>` 作为中间重建单位：

- `messages[start..].to_vec()`
- `new_messages = new_messages[start..].to_vec()`
- `history_snapshot = messages.clone()`
- provider request 前直接 `messages.clone()`

这些在：

- smart pruning
- history distillation
- route-aware full chat

链路里会叠加成额外分配和复制。

#### 4. `run_trace_builder.rs`

在 [run_trace_builder.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/run_trace_builder.rs) 里，当前 trace 侧为了投影 metadata，会：

- `let background = self.background_envelope.read().clone();`

而 trace 真正需要的只是：

- revision
- quality signal
- 少量 lifecycle / decision 字段

这里 clone 整个背景对象再取少量字段，属于典型“为读面投影付出过大的复制成本”。

### 这些热点为什么值得放进零拷贝计划

因为它们已经不再只是：

- `String`
- 小 metadata
- 偶发配置对象

而是完整通道里真实叠在一起的：

- 历史消息数组复制
- 背景层整对象复制
- trace 读面复制

它们虽然不如 `KV cache` 那样是第一战场，但已经能直接影响：

- 本地主脑完整通道延迟
- 工具调用后第二段回答延迟
- 同 session 长会话的额外内存与 token 密度

### 对 `brain` 的近零拷贝改造目标

#### 目标 1：checkpoint / background refresh 不再依赖整段历史 `to_vec()`

方向：

- 引入 borrowed history slice + appended tail 的组合
- 或引入 `SessionCheckpointDelta`
- 或使用 `Cow<[Message]>` / `SmallVec<[Message; N]>` 之类的近零拷贝策略

原则：

- 不要求所有 `Message` 严格零拷贝
- 但要避免“每次收尾都重新复制整段会话历史”

#### 目标 2：背景过滤从“整份 envelope clone”改成“按需投影”

方向：

- `ContextManager` 只构建 render-time view
- session-layer 去重改成 borrowed filter / projection
- `recent_window_summary` 去重不再强制复制整份背景对象

原则：

- 优先让“渲染出的上下文”变化
- 而不是先复制背景，再修改背景副本

#### 目标 3：history pruning / distillation 尽量改成 slice-first

方向：

- `prepare_messages(...)` 优先保留 slice 视图
- 只在真正需要写新 summary message 时 materialize 新数组
- provider request 只在最终一跳才收敛成所有权容器

原则：

- 裁历史时优先切片
- 不要每个阶段都 `to_vec()` 一次

#### 目标 4：trace / telemetry 只投影必要字段

方向：

- run trace 构建器只读背景层必要字段
- 避免为了 trace metadata clone 整份 `BackgroundEnvelope`

原则：

- trace 读面要 cheap-read
- 不要让可观测性反过来放大主链路复制

### `brain` 的建议改造顺序

1. [foreground_runtime.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/foreground_runtime.rs)
   - 先收 `messages.to_vec()` 两个热点
2. [context.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/context.rs)
   - 把 `filtered_background_envelope(...)` 改成投影式过滤
3. [reasoner.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/reasoner.rs)
   - 把 pruning/distillation 改成 slice-first
4. [run_trace_builder.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/run_trace_builder.rs)
   - trace 仅读必要字段

### `brain` 这组改造的收益预期

- 完整通道收尾延迟下降
- 背景压缩主线额外复制下降
- 本地主脑长 session 的对象分配压力下降
- 为 `Local Fast Chat / Full Agent Chat` 双通道继续减肥打基础

### `brain` 里顺手应一起看的复杂度问题

这一组不是严格意义上的“零拷贝”，但它们和上面的复制热点叠在一起，会一起放大完整通道延迟。

#### 1. `foreground_runtime.rs` 的多次全量消息扫描

在 [foreground_runtime.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/foreground_runtime.rs) 的 `finalize_outcome(...)` 里，当前会对同一份 `messages` 连续做多轮统计：

- `planned_tool_call_count`
- `tool_result_count`
- `collect_dangling_tool_call_ids(messages)`

这本质上是对同一批消息反复扫描。

当前它不是灾难，因为历史窗口有上限；但对完整通道来说，这是典型“每轮都付出的线性成本”，应收成：

- 单次遍历同时产出
  - `planned`
  - `completed`
  - `dangling`

而不是拆成多次扫描。

#### 2. `reasoner.rs` 的 history pruning / distillation 仍会反复全量处理旧历史

在 [reasoner.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/reasoner.rs) 的 `prepare_messages(...)` 里，当前 smart pruning 路径会：

- 先切出 `to_summarize`
- 再遍历 `to_summarize` 计算 SHA 缓存键
- cache miss 时再把整段旧历史拼成 summary 输入

所以它的真实成本不是一处 clone，而是：

- 每轮都要重新 hash 旧历史
- cache miss 时还要再次遍历并构造大字符串

这条路径对长 session 是标准的 `O(n)` 历史重扫，不是假问题。

建议方向：

- 引入增量 summary checkpoint
- 或增量 history digest
- 避免“每轮从头 hash 整段旧历史”

#### 3. `tactical.rs` 的 session/background 推断是多轮重复扫描

在 [tactical.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/tactical.rs) 里，当前 `infer_session_candidate(...)` 会围绕同一份 `recent_messages` 连续调用：

- `infer_backend_contexts(...)`
- `infer_retrieved_memory_objects(...)`
- `infer_web_session_objects(...)`
- `infer_artifact_session_objects(...)`
- `infer_task_session_objects(...)`
- `infer_tool_session_objects(...)`
- `infer_multimodal_session_objects(...)`
- `infer_working_mode(...)`
- 后面还会再扫一次 active topics / goals / followups

当前这条不是第一优先 bug，因为：

- recent window 现在被限制在 `6` 条
- 每个对象池也被限制在 `6/8` 个

所以它**当前不是复杂度爆炸点**。

但如果未来：

- recent window 增大
- backend object 家族继续扩
- 背景推断逻辑继续加规则

这条结构会迅速变成“同一批消息被很多小函数反复扫一遍”。

建议方向：

- 把 recent-message 分析改成一次归一化提取
- 再把归一化中间结构分发给各 object builder

也就是从：

- `多轮 scan`

改成：

- `单轮 parse + 多视图消费`

#### 4. Fast Chat 的最近上下文恢复仍会取整份 session

在 [foreground_runtime.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/foreground_runtime.rs) 的 `recent_fast_session_context(...)` 里，当前会：

- `memory.retrieve_session(session_id).await`
- 然后从 `session.messages` 里倒序取最后几条

这意味着 Fast Chat 想要的其实只是最近 `N` 条，
但当前接口语义还是“先恢复整份 session，再截尾部”。

这条在短 session 下问题不大，但对极长 session 来说，说明：

- Fast Chat 仍被整份 session 恢复接口绑住

建议方向：

- `Memory` 增加 recent-session window API
- 或 checkpoint 层单独保留 recent tail

### 这些复杂度问题里，哪些是真的优先级高

#### 高优先级

- `finalize_outcome(...)` 多次消息扫描
- `prepare_messages(...)` 对旧历史的重复 hash / summary 输入重建

#### 中优先级

- Fast Chat 的 recent tail 恢复方式

#### 先记录、暂不作为第一刀

- `tactical.rs` 的多轮 recent-message 扫描

原因不是它设计完美，而是：

- 当前 `recent_messages` 已被硬限制到很小
- 现在真正拖慢完整通道的还不是这里

### 一句话收口

对 `brain` 这条线来说，下一轮优化不能只盯“减少 clone”，还要同时盯：

- 重复扫描
- 重复 hash
- 重复 summary 输入构建

否则会出现“复制少了，但完整通道仍然不够快”的情况。

---

## 4. 按执行逻辑排序的正式计划

### 4.0 排序原则

这份计划不再按“哪个 crate 理论上最值得优化”来排，而按下面这个真实执行逻辑来排：

1. 先修当前线上/长会话里已经能直接感知到的延迟与增长问题
2. 再收完整通道、工具面和背景压缩主路径
3. 再下沉到工具/存储/流式链路
4. 最后再做高风险、深底层、回报高但改造面大的推理内核重构

一句话：

**先救当前体验和稳定性，再做深层高收益内核优化。**

## 4.1 Phase Z0：当前可感知延迟与增长风险止血

目标：

- 先把会直接拖慢完整通道、或导致长期运行内存增长的点收掉

顺序：

1. `brain/memory/episodic.rs`
2. `brain/reasoner.rs`
3. `gateway/api/handlers/chat.rs`
4. `brain/foreground_runtime.rs`
5. `brain/run_trace_builder.rs`

完成标准：

- 热缓存与计数器不再长期只增不减
- 后台持久化任务不再无界堆积
- 完整通道收尾不再重复复制整段历史
- trace 读面不再为少量字段 clone 整份背景对象

## 4.2 Phase Z1：完整通道与背景压缩热路径

目标：

- 把 `brain` 里已经影响完整通道延迟的复制热点与复杂度热点继续收掉

顺序：

1. `foreground_runtime.rs`
2. `context.rs`
3. `reasoner.rs`
4. `stream_chat_runtime.rs`
5. `run_trace_builder.rs`

完成标准：

- `checkpoint / background refresh` 不再为整段历史无脑 `to_vec()`
- `BackgroundEnvelope` 过滤改成投影式 / borrowed-first
- `prepare_messages(...)` 主路径以 slice-first 为主
- Fast Chat / Full Chat 不再共用过重的历史恢复与上下文构造路径
- trace metadata 不再 clone 整份背景对象

## 4.3 Phase Z2：工具链、媒体链与网络流减肥

目标：

- 把当前最容易把 prompt、artifact 和内存拉胖的工具/流式链路收掉

顺序：

1. `builtin-tools/pdf_parse.rs`
2. `builtin-tools/web_fetch.rs`
3. `builtin-tools/media_runtime.rs`
4. `builtin-tools/browser.rs`
5. `providers/utils.rs`
6. 各流式 provider
7. `sensory/audio/*`
8. `comm/*`

完成标准：

- 大图像/PDF 页更多走 artifact ref
- Base64 大内联显著减少
- SSE/streaming 路径减少不必要 materialize
- 长音频/长媒体路径降低峰值内存

## 4.4 Phase Z3：存储与向量路径

目标：

- 把 `engram` 的接口层零拷贝友好优势真正落到实现层

顺序：

1. `engram/storage/redb_impl.rs`
2. `engram/storage/mod.rs`
3. `engram/vector_store.rs`
4. `engram/storage/in_memory.rs`

完成标准：

- 读取 blob/vector 时减少额外 `copy_from_slice`
- 向量路径减少中间 `Vec<f32>` 重建
- 热点检索和 durable round-trip 的搬运成本下降

## 4.5 Phase Z4：主脑推理内核与 KV/页布局深改

目标：

- 再进入最深层、改造成本最高、但长期收益最大的推理内核优化

顺序：

1. `inference/backend/llama_cpp.rs`
2. `inference/engine.rs`
3. `inference/quant.rs`
4. `inference/backend/onnx_runtime.rs`

完成标准：

- `KV conservative prototype` 不再只是 shadow copy
- 真实 `KV live` 开始受压缩路径影响
- `projected_*` 逐步变成真实收益而不是估算收益
- 页/块结构朝共享 backing store 与可复用 scratch buffer 收口

## 4.6 Phase Z5：备份、安全与第二梯队大块数据路径

目标：

- 收口第二梯队但仍然值得做的真实大块数据路径

顺序：

1. `security/memory_backup.rs`
2. `security/vault.rs`
3. `security/encryption.rs`
4. 其他未在前面阶段覆盖、但仍有大块数据搬运的 sensory/provider 路径

完成标准：

- 大文件备份/恢复更多走 streaming hash / encrypt / decrypt
- 安全层大对象处理不再整块读入内存
- 第二梯队媒体/网络路径不再留下明显峰值内存热点

---

## 4.7 Phase Z6：热点大文件结构收口

目标：

- 把已经明显偏离单一职责、并持续吸收主路径实现的热点大文件收回来
- 让后续能力继续落主路径时，不再默认堆进历史热点文件

判断依据：

- [DEVELOPMENT_STANDARDS_AGENTOS.md](/home/biubiuboy/BenShu/docs/DEVELOPMENT_STANDARDS_AGENTOS.md) `1.3 单一职责，但允许高内聚`
- [DEVELOPMENT_STANDARDS_AGENTOS.md](/home/biubiuboy/BenShu/docs/DEVELOPMENT_STANDARDS_AGENTOS.md) `1.4 主路径优先于旁路能力`
- 当前主线路径已进入结构收口阶段，不应继续把新增实现堆回历史热点文件

### 当前最明显不符合结构收口预期的热点文件

#### 1. `crates/brain/src/agent/foreground_runtime.rs`

- 当前体量：约 `5846` 行
- 问题性质：
  - 前台聊天主路径
  - `Fast Chat`
  - `Full Agent Chat`
  - background refresh
  - checkpoint / session restore
  - runtime hook / trace / task 组装
  - 大量产品级回归支撑
  全都堆在一个文件里

这已经不是“高内聚”，而是典型历史热点继续吸收主路径逻辑。

建议拆分方向：

- `foreground_chat_fast.rs`
- `foreground_chat_full.rs`
- `foreground_checkpoint.rs`
- `foreground_background.rs`
- `foreground_runtime_trace.rs`

优先级：

- **最高**

#### 2. `crates/brain/src/skills/tool/mod.rs`

- 当前体量：约 `4087` 行
- 问题性质：
  - 工具能力路由
  - prompt-visible 工具筛面
  - hard route / soft route
  - 文档/文件/runtime/tool contract 特殊逻辑
  - schema 扁平化与 provider 兼容逻辑

它已经不是简单的“tool registry”，而是在承担半个工具路由引擎。

建议拆分方向：

- `capability_routing.rs`
- `tool_surface.rs`
- `tool_contract.rs`
- `tool_schema_normalization.rs`

优先级：

- **高**

#### 3. `crates/brain/src/agent/middleware.rs`

- 当前体量：约 `3612` 行
- 问题性质：
  - runtime middleware
  - governance / truth / verification 注入
  - hook metadata
  - tracing / contract 补面

现在更像“主路径治理总线汇编处”，不是单个中间件模块。

建议拆分方向：

- `middleware_governance.rs`
- `middleware_tracing.rs`
- `middleware_context_contract.rs`
- `middleware_runtime_notes.rs`

优先级：

- **高**

#### 4. `crates/brain/src/agent/tactical.rs`

- 当前体量：约 `3285` 行
- 问题性质：
  - entropy monitor
  - speculative task slot
  - background tactics
  - workspace/source 推断
  - relationship/session candidate 推断

虽然前面已经收过一轮，但它仍然是明显的多职责热点。

建议拆分方向：

- `tactical_entropy.rs`
- `tactical_background.rs`
- `tactical_workspace.rs`
- `tactical_routes.rs`

优先级：

- **高**

#### 5. `apps/panel/src/app_state.rs`

- 当前体量：约 `4384` 行
- 问题性质：
  - 面板状态
  - Agent 交互状态
  - 全局模型配置
  - API 响应承接
  - UI 运行时共享状态

作为应用装配层允许更高聚合度，但继续长下去会很难维护。

建议拆分方向：

- `app_state_agent.rs`
- `app_state_models.rs`
- `app_state_runtime.rs`
- `app_state_ui.rs`

优先级：

- **中**

### 体量很大但仍可暂缓的文件

下面这些文件虽然也大，但当前更偏单领域内聚，不是第一批结构收口对象：

- [pdf_parse.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/pdf_parse.rs)
- [agent_memory.rs](/home/biubiuboy/BenShu/crates/engram/src/agent_memory.rs)
- [vector_store.rs](/home/biubiuboy/BenShu/crates/engram/src/vector_store.rs)
- [memory/mod.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/memory/mod.rs)

它们值得拆，但优先级低于上面那批“主路径热点继续吸收能力”的文件。

### 完成标准

- `foreground_runtime.rs` 不再同时承担快通道、完整通道、背景刷新、checkpoint、trace 组装
- 工具能力路由不再继续堆在单个 `mod.rs`
- middleware 的治理注入、trace 注入、runtime 注释被拆成明确模块
- tactical 的 entropy / background / workspace 推断不再共处一处
- panel 的 `app_state.rs` 不再同时吸收所有全局状态语义

一句话：

**优化不只包括零拷贝与复杂度治理，也包括把已经超载的热点文件按现有开发准则重新收回边界。**

---

## 5. 最值得立刻开的 10 个具体文件

按新的执行顺序，最该立刻开的不是最底层 `KV live`，而是当前已经能直接影响完整通道体验的文件：

1. [episodic.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/memory/episodic.rs)
2. [foreground_runtime.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/foreground_runtime.rs)
3. [reasoner.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/reasoner.rs)
4. [chat.rs](/home/biubiuboy/BenShu/apps/gateway/src/api/handlers/chat.rs)
5. [context.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/context.rs)
6. [run_trace_builder.rs](/home/biubiuboy/BenShu/crates/brain/src/agent/run_trace_builder.rs)
7. [pdf_parse.rs](/home/biubiuboy/BenShu/crates/builtin-tools/src/tool/pdf_parse.rs)
8. [utils.rs](/home/biubiuboy/BenShu/crates/providers/src/utils.rs)
9. [redb_impl.rs](/home/biubiuboy/BenShu/crates/engram/src/storage/redb_impl.rs)
10. [llama_cpp.rs](/home/biubiuboy/BenShu/crates/inference/src/backend/llama_cpp.rs)

---

## 6. 一句话收口

**BenShu 的优化，不该从“全仓到处删 clone”开始，也不该一上来就跳进最深的 `KV live` 重构。更符合当前主线逻辑的顺序是：先修当前可感知延迟与增长风险，再收完整通道与工具/存储热路径，最后再下沉到底层推理内核。**

尤其对主脑 `KV cache` 来说：

**如果后续要接近 TurboQuant 论文那种压缩收益，零拷贝/近零拷贝不是可选增强，而是基础条件。**
