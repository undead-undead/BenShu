# BenShu 本地模型栈与媒体预处理统一计划

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 测试链口径: 当前与后续本地模型栈联调、smoke test、role-binding readiness 与延迟验证默认遵循 `GPU 优先原则`；`CPU` 仅承担 fallback、诊断与最低可用性验证，不作为默认本地模型性能结论来源。

> 主约束来源:
>
> - `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`
> - `docs/secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md`
> - `docs/secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md`
>
> 文档定位:
>
> - 本文件是“本地模型栈与媒体预处理”的专项实施计划
> - 不承担新的总规范职责
> - 若与开发准则或执行蓝图冲突，以核心文档为准
> - 代码落点、模块归位与旧代码回收顺序应优先服从开发准则、AgentOS 执行计划和实际 crate 边界。
>
> 状态标记:
>
> - `[x]` 已完成
> - `[~]` 部分完成
> - `[ ]` 未完成

## 1. 目的

本文档定义 BenShu 在“本地 Jarvis”方向上的模型栈分层、统一范围与媒体预处理职责边界。

目标不是把所有能力硬压成一个万能 trait，而是明确：

- 哪些能力应统一到同一个本地模型系统中管理
- 哪些能力应保留专用任务接口
- 视频/音频预处理链应归属 builtin tool / runtime，而不是直接塞进大模型主接口

同时需要明确：

- 本计划回答“本地模型栈与媒体预处理能力要做到什么程度”
- 本计划后续若继续推进，默认应先参考当前 crate 边界、开发准则和 AgentOS 执行计划，再决定代码落点。
- 当本计划对应的 runtime / tool / provider 主线开发完成后，还应同步检查并重构 `apps/panel` 中对应组件、状态面板与交互入口，避免 Panel 长期停留在旧能力面或旧术语上

---

## 2. 结论先行

### 2.0 原生 Windows 优先原则

后续如果 BenShu 明确以 **原生 Windows 优先** 为目标，则本地模型栈的 backend 选择应遵循以下总原则：

- `LLM / SLM`：
  优先保持 `llama.cpp` 主线，不把主脑执行面整体改写成 `ONNX`
- `Embedding / Rerank / STT / OCR / 小型战术模型`：
  优先评估并逐步统一到 `ONNX Runtime + DirectML / WinML` 的 Windows 原生执行面
- `TTS`：
  继续允许 `Piper` 这类专用后端存在，不为了“统一”强行改写掉已稳定的本地语音链
- `VLM`：
  短期仍跟随 `llama.cpp` / 既有本地视觉后端；是否迁入 `ONNX/DirectML` 取决于模型生态而不是文档偏好

这意味着：

- Windows 原生优先时，不应再把“所有本地模型都统一塞进同一种推理内核”当成目标
- 正确目标应是：
  `LLM 主脑继续走最适合聊天与工具调用的后端；小模型层逐步收敛到 Windows 原生通用执行面。`
- 这里的“小模型层 Windows 原生统一执行面”不是某一张显卡的专线，而是面向：
  - `AMD`
  - `NVIDIA`
  - 未来可用的 `NPU / 其他 Windows 原生执行设备`
  的统一产品主线
- 因此，本计划后续提到 `ONNX Runtime + DirectML / WinML` 时，默认含义都是：
  `Windows 原生统一小模型执行层`，而不是 `AMD/A 卡专线`
- 同时，对于开发与测试链：
  `WSL / WSL2` 仍可作为高效率联调入口，但若承担性能或体验结论验证职责，必须优先接入 GPU 执行路径，而不是默认退回 CPU。

### 2.1 本地模型需要统一，但统一的是管理层

应统一的不是所有任务方法签名，而是：

- 模型加载入口
- 模型注册与 capability 声明
- 资源预算与调度
- 生命周期与缓存策略
- trace / witness / telemetry
- fallback / degradation 契约

不应强行统一的是任务本身的专用接口：

- `generate`
- `embed`
- `rerank`
- `transcribe`
- `synthesize`
- `recognize`

因此，正确目标是：

`统一本地模型系统，不强行统一所有任务 trait。`

### 2.2 视频/音频预处理链不应归入大模型主接口

视频/音频前处理本质上是：

- 抽帧
- 切片
- 转码
- 采样率转换
- 缩略图生成
- 元数据探测

这些都应归入：

- `builtin-tools`
- `runtime_surface`
- 受控媒体处理 wrapper

而不是归入：

- `LLM`
- `VLM`
- `STT`
- `TTS`

因此，正确目标是：

`媒体预处理走 builtin/runtime 工具链，模型只负责理解预处理结果。`

---

## 3. 完整本地 Jarvis 所需模型栈

一个较完整的本地 Jarvis，至少需要以下能力层：

### 3.1 核心模型层

- 主对话/推理 `LLM`
- 战术反思 `SLM`
- `Embedding`
- `Rerank`
- `STT`
- `TTS`

说明：

- 在“完整本地 Jarvis”目标下，上述 6 类核心模型层都应视为必备能力，不应把 `SLM` 单独拔高成例外体系
- `SLM` 仍属于统一模型系统中的文本模型一类，应继续服从“统一工厂 + 多专用 trait”的总体原则
- 它的特殊性主要体现在 runtime 角色：更适合作为主 `LLM` 之前的 tactical pre-pass，而不是独立承担最终执行与最终答复

### 3.1.1 Windows 原生优先下的推荐 backend 分层

如果目标平台是 **原生 Windows**，推荐按以下后端分层理解“完整本地 Jarvis”：

- 主对话/推理 `LLM`
  - 优先：`llama.cpp`
- 战术反思 `SLM`
  - 优先：`llama.cpp`
  - 后续可评估小型 `ONNX` 战术模型，但不作为第一主线
- `Embedding`
  - 优先：`ONNX Runtime + DirectML / WinML`
  - 次选：现有 `Candle`
- `Rerank`
  - 优先：`ONNX Runtime + DirectML / WinML`
  - 次选：现有 `Candle`
- `STT`
  - 优先：后续收敛到 `ONNX Runtime + DirectML / WinML`
  - 过渡期：现有 `Whisper/Candle`
- `TTS`
  - 优先：保持 `Piper` 等专用后端
  - 不强求与其他小模型统一
- `OCR`
  - 优先：`ONNX Runtime + DirectML / WinML` 或现有 `Tesseract/Wasm` 双轨
  - 目标是先有统一 capability contract，再决定是否淘汰旧后端
- `VLM`
  - 继续由本地视觉主线承接，不要求现在并入 `ONNX Runtime`

补充说明：

- `ONNX Runtime + DirectML / WinML` 在本计划里承担的是：
  `Windows 原生统一小模型执行层`
- 它服务的对象不区分 `AMD` 或 `NVIDIA`
- 是否最终落到 `DirectML / WinML / 其他 Windows 原生执行提供器`，应由平台能力与设备支持决定，而不是由文档先写死成单一显卡路线

### 3.1.2 角色 / 当前后端 / GPU 能力 / Windows 正式推荐后端对照表

下表用于明确三个容易混淆的问题：

- 某类能力本身能不能吃 `GPU`
- 当前仓库里主要由哪条后端承接
- 在 `Windows 原生` 正式产品主线下，后续应该优先收敛到哪条执行面

| 角色/能力 | 当前主要后端 | 是否可用 GPU | 当前仓库中的现实情况 | Windows 正式推荐后端 |
| --- | --- | --- | --- | --- |
| `LLM / 主脑` | `llama.cpp (GGUF)` | 能 | 当前本地主脑主线；已支持本地多模态与 `mmproj` | `llama.cpp` |
| `SLM / tactical` | `llama.cpp (GGUF)` | 能 | 当前最自然的小型文本主线；无 `SLM` 时允许 passthrough | `llama.cpp`，后续可评估小型 `ONNX` |
| `VLM / 本地多模态` | `llama.cpp + mmproj` | 能 | 当前本地多模态主线；适合主脑/视觉复用 | 继续优先 `llama.cpp` |
| `Embedding` | `Candle / ONNX` | 能 | 任务本身可吃 GPU；但当前 `AMD + Windows` 语境下，`Candle` 往往不理想 | `ONNX Runtime + DirectML / WinML` |
| `Rerank` | `Candle / ONNX` | 能 | 任务本身可吃 GPU；`Candle` 在 `CUDA/Metal` 更自然，在 `Windows + AMD` 不应作为长期主线 | `ONNX Runtime + DirectML / WinML` |
| `STT / Whisper` | `WhisperCandleBackend` | 能 | 语音转写本身可吃 GPU；当前仓库主实现仍偏 `Whisper/Candle` | 过渡期保留 `Whisper/Candle`，正式主线收敛到 `ONNX Runtime + DirectML / WinML` 或更合适的 Windows 原生语音后端 |
| `TTS` | `Piper` | 能 | 本地链已稳定；是否上 GPU 取决于具体后端实现，不必为统一强改 | 保持 `Piper` 等专用后端 |
| `OCR` | `Tesseract / Wasm / ONNX` | 能 | OCR 本身可吃 GPU；当前仓库仍是多后端并存 | `ONNX Runtime + DirectML / WinML` 或现有 `Tesseract/Wasm` 双轨 |
| `NLU / classifier / fact_check` | `Candle` | 能 | 判别类/分类类任务可以吃 GPU；但当前 `Candle` 在 `Windows + AMD` 上不应视作最终路线 | `ONNX Runtime + DirectML / WinML` |
| `Diffusion / 图像生成` | `Candle diffusion` | 能 | 任务本身强依赖 GPU；但不属于 `llama.cpp` 适配范围 | 保留专用扩散后端，按 Windows 原生 GPU 适配单独推进 |
| `safetensors` 本地模型后端 | `Candle / ONNX / 其他张量后端` | 能 | `safetensors` 只是模型资产形态，不等于只能 CPU | 按任务类型分流到 `ONNX Runtime + DirectML / WinML` 或其他更合适的专用后端 |

补充判断：

- `llama.cpp` 适合的核心范围主要是：
  - `GGUF` 文本生成模型
  - 一部分 `VLM`
  - 主脑 / `SLM` / 本地多模态主线
- `llama.cpp` 不应被误当成“所有本地 AI 任务的统一后端”：
  - `embedding`
  - `rerank`
  - `NLU / fact_check`
  - `diffusion`
  这些任务虽然都可以用 `GPU`，但通常更适合 `ONNX`、`Whisper`、扩散模型专用后端，或现有更窄的任务后端
- 当前仓库里的 `Candle` 仍然是正式依赖，不是死代码；但对 `Windows + AMD` 这条正式产品主线，不应再把它当成小模型 GPU 主路线的最终答案

### 3.1.3 llama.cpp 的 gpu_layers 自适应策略

当前 `llama.cpp` 主脑线不再采用：

- `有 GPU 就固定 100`
- `没 GPU 就固定 0`

这种粗粒度策略。

现在应明确按下面这套原则理解 `gpu_layers`：

- 目标口径：
  - `Windows / GPU 优先`
  - 优先尝试硬件卸载
  - 但不再默认把所有层无脑全量 offload 到 GPU
- 输入因素：
  - 当前 host 的 `VRAM` 预算
  - 当前已使用 `VRAM`
  - 主模型估算体积
  - `mmproj` / 多模态桥额外体积
  - 显存拓扑：
    - `DedicatedGpu`
    - `SharedGpu`
    - `UnifiedMemory`
- 输出结果：
  - 自动选择分档 `gpu_layers`
  - 例如：
    - `100`
    - `80`
    - `64`
    - `48`
    - `32`
    - `24`
    - `16`
    - `8`
    - `0`

这意味着：

- 在独显预算充足时，`llama.cpp` 仍然可以接近“全量层卸载”
- 在独显紧张时，会自动退成“部分层卸载”
- 在共享显存或预算明显不足时，会直接退到更保守的小档位，必要时退到 `0`

同时要特别区分：

- `gpu_layers` 自适应
  - 解决的是：
    - 模型权重有多少层放 GPU
- `n_ctx / KV` 自适应
  - 解决的是：
    - 运行时上下文窗口和缓存池大小

这两者不是同一个问题，也不应混为一谈。

当前实现上的现实边界也需要写清：

- 由于 `llama.cpp` 的 `n_gpu_layers` 需要在模型真正加载前决定
- 系统在这一时刻通常还拿不到“已加载模型的精确层数”
- 因此当前策略仍然属于：
  - `按预算分档的自适应`
- 还不是：
  - `基于真实层数的完美逐层比例分配`

但这已经比旧的 `100 / 0` 固定策略明显更符合：

- `Windows 原生 + GPU 优先`
- 不同显卡档位
- 多模态 `mmproj` 并存
- 主脑/视觉真实运行压力

### 3.2 多模态理解层

- `VLM`
- `OCR`

### 3.3 媒体处理层

- 视频前处理
- 音频前处理
- `ffmpeg / ffprobe` 等 runtime 依赖

这里要强调：

- `VLM / OCR / STT` 是模型能力
- 视频/音频前处理不是模型能力，而是运行时处理能力

---

## 4. 当前代码状态

### 4.1 已有统一入口

当前 `benshu-inference` 已经具备第一版统一工厂入口：

- `InferenceFactory::create_backend(...)`
- `InferenceFactory::create_vision_backend(...)`
- `InferenceFactory::create_ocr_backend(...)`
- `InferenceFactory::create_embedding_backend(...)`
- `InferenceFactory::create_rerank_backend(...)`
- `InferenceFactory::create_stt_backend(...)`
- `InferenceFactory::create_tts_backend(...)`

这说明当前系统已经统一到了：

- 加载入口
- capability 分发入口

但如果以 **原生 Windows 优先** 作为新的主导原则，当前还缺一条明确能力：

- `ONNX Runtime + DirectML / WinML` 的统一小模型执行面

也就是说，当前统一工厂已经具备“接入位置”，但还没有完成 Windows 原生小模型主线的后端补齐。

### 4.2 当前并不是单一 trait 大统一

当前仍然是“统一工厂 + 多专用 trait”结构：

- `ModelBackend`
- `VisionModelBackend`
- `EmbeddingBackend`
- `RerankBackend`
- `SttBackend`
- `TtsBackend`
- `OcrBackend`

这本身是合理的，不应把它当成需要消灭的问题。

### 4.3 媒体预处理链已完成主路径收口

视频/音频前处理现在已经不再只是 inference 边上的 utility，而是已正式收进：

- `builtin tool registry`
- `runtime surface contract`
- `trace / witness`
- provider-level media outcome / strategy contract

当前剩余工作不再是“把媒体预处理拉进主路径”，而是：

- 把同一 contract 继续扩展到更多云侧多模态 provider / runtime 入口
- 在 `apps/panel` 同步完成能力面、状态面与术语收口

### 4.4 Windows 原生优先下的现实判断

如果继续按原生 Windows 优先推进，当前模型栈的现实状态应这样理解：

- `LLM / SLM`
  - 已有明确本地主线，可继续强化
- `Embedding / Rerank / STT / OCR`
  - 有现成后端，但还没有统一到“Windows 原生最佳执行面”
- `TTS`
  - 已有稳定本地路径，不构成当前 Windows 原生阻塞点
- `ONNX Runtime + DirectML / WinML`
  - 系统层通常已具备基础环境
  - BenShu 工程层尚未把它接成统一执行主线

---

## 5. 目标架构

### 5.1 模型层：统一模型系统

所有本地模型能力应统一纳入同一个 `Local Model System`：

- 统一工厂
- 统一注册
- 统一 capability 元数据
- 统一资源控制
- 统一 trace / witness

但专用 trait 保留：

- 文本生成
- 向量生成
- 重排
- 语音识别
- 语音合成
- OCR
- 视觉理解

同时应把 `SLM` 视为统一模型系统中的正式组成部分，而不是临时旁路：

- 它继续复用统一 inference backend / factory
- 在 runtime 语义上承担 `tactical_orchestrator` 角色
- 没有 `SLM` 时应允许主 `LLM` 直接 passthrough
- 有 `SLM` 时优先作为主 `LLM` 前的 tactical pre-pass，而不是和主 `LLM` 争夺最终输出权

在 **Windows 原生优先** 前提下，应进一步把目标架构写清楚为：

- `LLM / SLM 主脑层`
  - 继续以 `llama.cpp` 为主
- `Small Model Utility Layer`
  - 逐步收敛到 `ONNX Runtime + DirectML / WinML`
  - 主要覆盖：
    - `embedding`
    - `rerank`
    - `stt`
    - `ocr`
    - 小型 tactical / classifier / router model
- `专用运行时层`
  - 保留 `Piper`、`Tesseract` 等在现实中更稳定的专用后端
  - 不为了“统一美观”立刻强拆

### 5.2 预处理层：统一媒体 runtime 系统

媒体前处理应形成独立 `Media Runtime Surface`：

- `probe_media`
- `extract_video_frames`
- `extract_audio_track`
- `transcode_audio`
- `normalize_audio`
- `render_video_thumbnail`

这层应具备：

- 统一工具注册
- 统一 source / scope / capability_domain
- 统一 trace / witness
- 统一错误面
- 统一缓存/工件路径

### 5.3 调用关系

标准调用路径应为：

1. builtin/runtime 预处理媒体
2. 生成标准中间结果
3. 把中间结果交给 `VLM / STT / OCR`
4. 由 `LLM / brain` 汇总成最终响应

而不是：

1. 直接把原始视频/音频塞给主模型赌它自己会处理

---

## 6. 修改原则

### 6.1 模型系统统一原则

- 统一入口，不统一任务语义
- 统一 capability 元数据，不统一成一个万能方法
- 统一调度与资源，不破坏专用 trait 清晰度
- 在 Windows 原生优先场景下，统一的是：
  - capability 管理层
  - resource / telemetry / fallback / readiness
  - 而不是强制所有模型任务都共享一个推理后端

### 6.1.1 替代式重构与旧机制清理原则

本计划后续实施时，默认遵循：

- 如果新机制已经接入主路径
- 已通过编译、主路径测试或最小 smoke 验证
- 且确认旧机制不再承担独立主路径职责

则应删除已被替代的旧代码、旧适配层、旧兼容入口与旧文档口径，而不是长期保留两套并行机制。

允许暂时保留旧代码的条件只能是：

- 新机制尚未完成主路径接线
- 仍缺关键验证
- 仍有明确兼容对象尚未迁移

并且保留时必须同时写清：

- 为什么当前不能删
- 影响范围是什么
- 后续回收条件是什么

一句话约束：

`本计划默认采用“替代后收旧”的重构策略，不鼓励把已核实无误的新旧机制长期并存。`

### 6.2 媒体处理职责边界

- 媒体前处理属于 runtime/tool 层
- 模型只负责理解，不负责主前处理链
- 云模型可支持“直接吃媒体”，但本地主路径仍应保留独立预处理链

### 6.3 证据链原则

模型层与媒体层都必须进入：

- run trace
- stage metadata
- witness

不能只停留在局部日志或 utility 成功/失败返回值。

---

## 7. 计划拆分

当前状态（2026-03-29）：

- `Phase A-D` 主线已完成
- 本文后续开发重点不再是重复收口旧主线，而是进入 `Windows Native` 新阶段
- 后续若继续扩展更多云侧多模态 provider / runtime 入口，仍应沿用本计划已收口的 capability / outcome / strategy contract，而不是再开旁路实现
- `WSL2 / Linux ROCm` 在本文后续阶段中的定位应降级为：
  - 测试/验证路径
  - smoke/perf 对照路径
  - backend 实验路径
  - 非默认产品部署路径

### 7.1 已完成阶段归档

以下阶段已完成，保留在本文中作为结构归档与后续扩展基线。

### Phase A: Local Model System 收口

- `[x]` 统一本地模型 capability 清单
- `[x]` 为各 backend 显式声明：
  - `[x]` `llm`
  - `[x]` `slm`
  - `[x]` `embedding`
  - `[x]` `rerank`
  - `[x]` `stt`
  - `[x]` `tts`
  - `[x]` `vlm`
  - `[x]` `ocr`
- `[x]` 统一进入 model registry / telemetry / witness
- `[x]` 明确本地与云端在 capability 声明上的对齐契约

当前进展（2026-03-28）：
- inference 已补统一 `BackendSource / ModelRole / BackendFactoryDescriptor / BackendBindingDescriptor`
- 已支持 path-aware 角色声明：
  - 本地文本模型默认声明 `llm + slm`
  - 本地带视觉组件的文本模型额外声明 `vlm`
  - 云端文本入口默认声明 `llm + slm`
- 本地 `embedding / rerank / stt / tts / ocr` 已进入 factory / registry 主路径
- tactical SLM 启动链已复用这套声明视图做加载日志、`RunTrace.metadata`、stage metadata 与 witness notes
- Phase A 现在已收口；后续继续做的是：
  - 为 `embedding / rerank / stt / tts / ocr` 增加更广的上层消费面
  - 把同类 descriptor 继续接到更多非 agent 主线场景

### Phase B: Media Runtime Surface 建立

- `[x]` 将视频/音频前处理从散落 utility 收成受控 builtin/runtime 工具
- `[x]` 建立第一批媒体工具：
  - `[x]` `probe_media`
  - `[x]` `extract_video_frames`
  - `[x]` `extract_audio_track`
  - `[x]` `normalize_audio`
  - `[x]` `render_video_thumbnail`
- `[x]` 显式 capability domain：
  - `[x]` `media_runtime`
  - `[x]` `video_preprocess`
  - `[x]` `audio_preprocess`

当前进展（2026-03-28）：
- 第一批媒体预处理工具已进入 builtin/tool registry 主路径
- `probe_media` 统一承接 `ffprobe`
- `extract_video_frames / render_video_thumbnail` 进入 `video_preprocess`
- `extract_audio_track / normalize_audio` 进入 `audio_preprocess`
- `audio_preprocess` 现在显式提供共享 artifact/helper：
  - `extract_audio_track_artifact`
  - `normalize_audio_artifact`
- 媒体预处理结果已进入 `run trace / stage metadata / witness` 第一版证据链
- 输出型媒体工具在 artifact manager 存在时已显式注册 session-scoped artifact
- `artifact_registration / artifact_kind` 已进入媒体 runtime 返回值与统一证据链第一版
- `document_understand` 的音频/视频主路径已开始复用这套显式 helper：
  - 音频默认先走 `normalize_audio`
  - 视频默认先走 `extract_video_frames`
- provider 与 builtin-tools 的视频抽帧实现已收成共享 helper，旧 `extract_frames(...)` API 已移除
- provider 与 builtin-tools 的音频前处理实现现在也统一复用 `audio_preprocess`，不再各自保留一套 `ffmpeg` 音频转换逻辑
- provider 的本地 STT 前处理已改成共享音频标准化 helper，不再把容器字节直接当 PCM
- 共享 helper 已从“实现复用”推进到“显式媒体工件与执行面”语义

### Phase C: 模型层与媒体层对接

- `[x]` VLM 默认吃 `extract_video_frames` 结果
- `[x]` STT 默认吃 `normalize_audio` 结果
- `[x]` OCR 默认吃图像页/帧图像结果
- `[x]` brain / router 层不再把媒体预处理和模型理解混成一个能力面

当前进展（2026-03-28）：
- `document_understand` 的音频成功路径现在会显式声明：
  - `media_preprocess_route = normalize_audio`
  - `media_preprocess_consumed = true`
  - `media_preprocess_consumer = stt`
- `document_understand` 的视频成功路径现在会显式声明：
  - `media_preprocess_route = extract_video_frames`
  - `media_preprocess_consumed = true`
  - `media_preprocess_consumer = vlm`
- `document_understand` 的图像 OCR 成功路径现在也会显式声明：
  - `media_preprocess_route = image_page_raster`
  - `media_preprocess_source_kind = direct_image`
  - `media_preprocess_consumed = true`
  - `media_preprocess_consumer = ocr`
- `document_understand` 的视频 `extract_text` 路径现在也已进入独立 OCR 主路径：
  - `media_preprocess_route = extract_video_frames`
  - `media_preprocess_source_kind = video_frame_image`
  - `frame_source_contracts[*].source_contract_ref = video_frame:<n>`
  - `media_preprocess_consumer = ocr`
- `pdf_parse` 的 `page_routes` 现在会在页图类解析路由上显式带出来源契约：
  - `source_contract_kind = pdf_page_image`
  - `source_contract_ref = pdf_page:<n>`
- 这条“已消费 + 来源契约”语义已进入 runtime note / `RunTrace.metadata` / witness
- `text_extract` 现在也进入同一套图片 OCR contract：
  - `media_preprocess_route = image_page_raster`
  - `media_preprocess_source_kind = direct_image`
  - `media_pipeline_outcome`
  - `media_preprocess_consumer = ocr`
- `NativeProvider` 的本地多模态主线现在也会在 provider telemetry 中显式带出：
  - `provider_media_preprocess_consumed_by`
  - `provider_media_preprocess_consumption_routes`
- `LlamaCpp` 的本地多模态主线现在也会带出同类 provider-level consumption evidence
- `alternate_model_fallback` 现在不只停在 guidance：
  - `Reasoner` 与 `stream_chat` 会把下一跳工具面收窄到 `document_understanding` 的 preferred tools
  - 同时显式记录 `media_followup_capability_route = document_understanding`
  - 以及 `media_followup_execution_surface = document_understanding_alternate_model_fallback`
- `attachment_fallback` 现在也进入同一套稳定 capability contract：
  - `Reasoner` 与 `stream_chat` 会把下一跳工具面收窄到 `document_understanding` 的 preferred tools
  - 并显式记录 `media_followup_execution_surface = document_understanding_attachment_fallback`
- `clarification_or_manual_review` 现在也有统一 capability contract 主语义：
  - 显式记录 `media_followup_capability_route = document_understanding`
  - 以及 `media_followup_execution_surface = document_understanding_clarification_or_manual_review`
- `brain/router` 现在不只从 `document_understand`，也会从 `text_extract` 的图片 OCR 结果推导同一套 media follow-up contract
- 命中 media follow-up contract 时，`stream_chat` 与 `think` 现在不只收窄到 `document_understanding` 工具面，也会把
  - `capability_route = document_understanding`
  - `preferred_capability_domain = document_understanding`
  - `media_followup_execution_surface = ...`
  直接写入下一跳请求 `extra_params`
- provider-level 多模态 follow-up contract 现在也会持久化回 assistant 历史消息 metadata：
  - `provider_media_preprocess_followup_strategies`
  - `provider_media_preprocess_attachment_fallback_routes`
  - `provider_media_preprocess_alternate_model_fallback_routes`
  - `provider_media_preprocess_clarification_routes`
- 因此下一轮 `think / stream_chat` 不只会从 `document_understand / text_extract` 的工具结果推导 follow-up，也会从本地 provider 主线回写的 media contract 继续推导同一套 capability route 与 execution surface
- Phase C 主线现在已收口；后续若接入更多云侧多模态 provider，应沿用同一 contract 扩展，而不是再新开旁路

### Phase D: Trace / Witness 完整收口

- `[x]` 媒体预处理进入 run trace
- `[x]` 进入 stage metadata
- `[x]` 进入 witness
- `[x]` 区分：
  - `[x]` 预处理成功但模型失败
  - `[x]` 预处理失败
  - `[x]` 模型成功但结果不足

当前进展（2026-03-28）：
- 媒体预处理的 `tool/status/kind/input/output/engine/cleanup/frames` 已进入 `RunTrace.metadata`
- 输出型媒体工具的 `artifact_registered/source_kind/artifact_kind/artifact_uri` 已进入 `RunTrace.metadata`
- `document_understand` 的 `media_preprocess_consumed_by / media_preprocess_consumption_routes` 已进入 `RunTrace.metadata`
- `NativeProvider` 的 provider telemetry 消费字段也已进入 `RunTrace.metadata` 与 witness
- `NativeProvider / LlamaCpp` 的 provider-level 多模态主线现在也会显式区分：
  - `preprocess_failed`
  - `model_failed_after_preprocess`
  - `model_result_insufficient`
- `NativeProvider / LlamaCpp` 的 provider-level 多模态主线现在也会显式带出 follow-up strategy contract：
  - `attachment_fallback`
  - `alternate_model_fallback`
  - `clarification_or_manual_review`
- 上述 provider-level strategy 现在也会进入 `RunTrace.metadata` / `RuntimeStage::Reasoning` / witness
- 上述 provider-level strategy 现在也会持久化进 assistant 历史消息 metadata，供下一轮 `think / stream_chat` 继续恢复：
  - `provider_media_preprocess_followup_strategies`
  - `provider_media_preprocess_attachment_fallback_routes`
  - `provider_media_preprocess_alternate_model_fallback_routes`
  - `provider_media_preprocess_clarification_routes`
- `RuntimeStage::Execution` 与 witness notes 现在也会带上述媒体 artifact / consumption 字段
- `RuntimeStage::Execution` 与 witness notes 现在也会带：
  - `media_preprocess_source_kinds`
  - `media_preprocess_source_refs`
- 这套来源契约现在不只覆盖 `document_understand` 与 `text_extract`，也覆盖直接 `pdf_parse` 调用时的 `page_routes[*].source_contract_*`
- `document_understand` 现在还会显式返回 `media_pipeline_outcome`，并区分：
  - `preprocess_failed`
  - `model_failed_after_preprocess`
  - `model_result_insufficient`
- 这三类结果现在也已进入 `RunTrace.metadata` / stage metadata / witness
- `runtime_media_preprocess_consumption_surface` 现在覆盖：
  - `document_understand`
  - `text_extract`
  - `pdf_parse`
- 因此图片 OCR 的失败结果以及 PDF 页图来源契约都会进入统一证据链
- brain/runtime 现在还会把这些 outcome 进一步映射成显式策略：
  - `attachment_fallback`
  - `alternate_model_fallback`
  - `clarification_or_manual_review`
- 这些 follow-up strategy 现在也会进入下一跳 `BeforeLlm` guidance：
  - `Reasoner` 与 `stream_chat` 会把已知策略注入系统提示
  - `RunTrace / stage metadata / witness` 也会记录 `media_followup_strategies` 与 `media_followup_guidance_active`
- `brain/router` 现在也会把这套策略继续推进成显式执行面：
  - `capability_route = document_understanding`
  - `preferred_capability_domain = document_understanding`
  - `media_followup_execution_surface = ...`
- Phase D 主线现在已收口；后续若新增更多媒体 provider/runtime 入口，应继续复用同一 outcome / strategy contract

### 7.2 Windows Native 新阶段

后续若继续开发本地模型栈，应以 **原生 Windows 优先** 为新主线，并明确区分：

- `主脑层`
  - `LLM / SLM`
  - 继续优先 `llama.cpp`
- `小模型层`
  - `embedding / rerank / stt / ocr / tactical small models`
  - 逐步统一到 `ONNX Runtime + DirectML / WinML`
- `专用后端层`
- `tts`
  - 保留 `Piper` 等专用后端，不强求统一到 `ONNX`
- `验证层`
  - `WSL2 / Linux ROCm`
  - 仅用于测试、对照、实验，不再作为默认用户路径

### 7.3 专用模型优先、多模态兜底、自适应能力路由

在本地模型栈继续扩展多模态能力时，执行顺序必须遵守下面这条主原则：

`优先走最专用、最稳定、最可控的全局角色模型；多模态主脑 / VLM 负责高层理解、复杂布局解释与 fallback，不承担所有输入的第一落点。`

这条原则拆开后是：

- `STT`
  - 优先走全局 `Speech-to-Text` 角色绑定
  - 不应默认写死某个云插件或某个具体模型名
  - 若用户未指定 backend，则先走全局默认 STT runtime，再根据 runtime/fallback contract 决定是否降级
- `TTS`
  - 继续走全局 `Text-to-Speech` 角色绑定
  - 不通过多模态主脑兜底
  - 保持专用输出后端语义
- `OCR`
  - 对“文本提取为主”的图片、扫描件、文档页、视频帧，先走专用 OCR 路径
  - OCR 成功时直接产出结果，不应再额外绕行多模态模型
  - OCR 失败或结果明显不足时，才升级到多模态理解层
- `VLM / 多模态主脑`
  - 负责图像语义理解、复杂布局解释、OCR fallback、以及 OCR 结果后的综合理解
  - 不应替代专用 STT/TTS/OCR 的默认第一执行面

实现上还必须遵守另一条硬约束：

`适配目标是 llama.cpp / provider 的能力契约，不是某一个具体模型名。`

也就是说：

- 不为 `Gemma`、`LLaVA`、`Qwen-VL` 单独写产品逻辑分支
- 而是根据当前本地执行面是否具备：
  - 文本能力
  - 视觉能力
  - `mmproj` / 等效视觉投影支持
  - provider-level multimodal request contract
  来决定是否可作为 `VLM` 或 `主脑+VLM` 角色使用
- 这样做的收益是：
  - 更符合 `Windows 原生 + llama.cpp` 的正式产品主线
  - 更容易兼容后续不同本地多模态模型
  - 不会把路由层写成“绑定单模型名字”的脆弱结构

当前代码侧已经开始按这条原则收口：

- `document_understand`
  - 图片/视频文字提取走 `ocr_first_multimodal_fallback`
  - OCR 结果不足或失败时再回退到多模态理解
- `text_extract`
  - 同样进入 `ocr_first_multimodal_fallback`
  - 且在 fallback 不可用时返回结构化 error，而不是直接崩掉
- `voice / transcribe_audio`
  - 默认不再强制写死 `cloud-whisper`
  - 而是把 plugin 选择权交回全局 STT 角色绑定
- `visual / pdf_parse`
  - 本地视觉 fallback 不再写死 `llava-v1.5-7b`
  - 改为按当前 provider / sensory / llama.cpp 视觉能力面决定

因此，这一阶段的正确产品语义不是：

- “先问多模态大模型能不能做一切”

而是：

- “先问专用角色模型是否能稳定完成”
- “必要时再升级到多模态理解层”
- “升级判断基于 capability contract，而不是基于某个模型名字”

### 7.4 llama.cpp 兼容诊断读面

由于当前 `llama.cpp` 是以库方式嵌入系统，而不是让用户单独下载一个外部可执行程序，因此：

`用户不应该自己猜某个模型是否兼容 llama.cpp。系统必须直接给出结构化兼容诊断。`

这份诊断至少要分成三层：

- `格式兼容`
  - 当前绑定是否是 `GGUF`
  - 若不是 `GGUF`，则直接标记为 `not_llama_cpp_artifact`
- `角色兼容`
  - 当前绑定在 `llama.cpp` 路线上能否承担：
    - `llm`
    - `slm`
    - `vlm`
  - 若没有解析到 `mmproj` / 等效视觉投影，则明确标成：
    - `text_compatible`
    - `text_only_no_mmproj`
  - 若已解析到 `mmproj` 并具备视觉角色，则标成：
    - `multimodal_compatible`
    - `resolved`
- `当前主机运行兼容`
  - 当前主机是否已经选中 `llama.cpp` 执行面
  - 当前主机是否只是文本可用
  - 当前主机是否已进入多模态可用状态
  - 若当前主机仍未选择或仍在 fallback，则要给出明确状态说明，而不是只让用户看底层日志

这一层兼容诊断的适配目标也必须继续遵守上一节的硬约束：

`适配目标是 llama.cpp / provider 的能力契约，不是某一个具体模型名。`

也就是说：

- 不为 `Gemma`、`LLaVA`、`Qwen-VL` 单独写产品逻辑分支
- 而是根据当前本地执行面是否具备：
  - 文本能力
  - 视觉能力
  - `mmproj` / 等效视觉投影支持
  - provider-level multimodal request contract
  来决定它是否可承担：
  - `主脑 LLM`
  - `SLM`
  - `VLM`
  - 或 `主脑 + VLM`

当前代码里，这一层已经接进：

- `gateway /api/system/local-model-stack`
  - 每个 role binding 新增 `llama_cpp` 兼容诊断面
- `panel`
  - 直接展示：
    - `llama.cpp Compatibility`
    - `llama.cpp Roles`
    - `llama.cpp mmproj`
    - `llama.cpp Host Status`
    - `llama.cpp Note`
    - `llama.cpp Host Note`

这样用户看到的就不再只是：

- `artifact_kind = gguf`
- `factory_id = llama_cpp`

而是更直接的产品语义：

- 这是 `llama.cpp` 文本兼容模型
- 它还能不能做多模态
- 是否缺少 `mmproj`
- 当前主机到底有没有真正把它跑在 `llama.cpp` 路线上

当前 `llama.cpp` 侧的多模态接入也应继续遵守同一条“能力契约优先”的规则：

- 先根据：
  - 显式配置的 `mmproj`
  - 同目录自动发现到的 `mmproj*.gguf / mmproj*.safetensors`
  来判断是否具备视觉投影资产
- 只要视觉投影资产存在，就进入“多模态候选”路径
- projector 不再按单一模型名写死，而是按：
  - `mmproj` 张量结构
  - 以及必要时的文件名提示
  进行自适应加载
- 若 projector 无法识别或加载失败，则明确退回：
  - `text_compatible`
  - `text_only_no_mmproj`
  - 或 host/runtime fallback

这样后续接入新的 `llama.cpp` 多模态模型时，系统需要适配的是：

- `GGUF + mmproj` 资产组合
- provider/runtime multimodal contract
- projector 结构

而不是为某一个具体模型名单独写一条产品分支。

#### Phase W1: Windows Native Backend 分层定案

- `[x]` 明确 `LLM / SLM` 的 Windows 原生正式主线仍为 `llama.cpp`
- `[x]` 明确 `embedding / rerank / stt / ocr / tactical small models` 的 Windows 原生统一执行面目标为 `ONNX Runtime + DirectML / WinML`
- `[x]` 明确 `tts` 保持专用后端路线，不为了“统一”硬改成 `ONNX`
- `[x]` 在文档、gateway、panel 中统一写清：
  - `[x]` Windows 原生是正式产品主线
  - `[x]` `WSL2 / Linux ROCm` 仅为测试/验证路径
  - `[x]` `Models > Local` 读面现在会显式显示：
    - `[x]` `product_mainline`
    - `[x]` `host_runtime`
    - `[x]` `validation_tracks`
    - `[x]` `product_track`
    - `[x]` `preferred_backend`
    - `[x]` `current_backend`
    - `[x]` `execution_provider`

当前进展（2026-03-29）：
- `gateway /api/system/local-model-stack` 已补：
  - `host_runtime`
  - `product_mainline`
  - `validation_tracks`
  - `windows_native_priority`
- 各 role binding 已补：
  - `product_track`
  - `preferred_backend`
  - `current_backend`
  - `execution_provider`
- `panel > Models > Local` 已开始直接展示上述字段
- 因此 W1 的“口径定案与读面显式化”已起步；后续未完成部分主要是把这套定案继续下沉到真实 backend 选择与 readiness 策略

#### Phase W2: Windows 原生小模型执行面接入

- `[x]` 为 `embedding` 接入 `ONNX Runtime + DirectML / WinML` backend
- `[x]` 为 `rerank` 接入 `ONNX Runtime + DirectML / WinML` backend
- `[x]` 为 `stt` 接入或评估 `ONNX Runtime + DirectML / WinML` 主线
- `[x]` 为 `ocr` 接入或评估 `ONNX Runtime + DirectML / WinML` 主线
- `[x]` 为后续战术小模型预留统一 `ONNX` role binding 与 readiness contract
- `[x]` 保持现有 `Candle / Tesseract / Piper / Wasm` 路线作为可回退后端，而不是在新 backend 未验证前直接硬删

当前进展（2026-03-29）：
- `inference` 已新增统一 `WindowsNativeRuntimeStatus`
- `inference` 已新增统一 `LocalModelContractDescriptor`
- `inference` 已为 `embedding / rerank` 新增独立 `onnx_*_winml` factory
- `inference` 已为 `embedding / rerank` 落地真实的 `ONNX Runtime + DirectML` Windows-only backend 代码
- `inference` 已把 `onnx_*_winml` 工厂改成“创建阶段前置校验”
  - 若 `windows_native_onnx` 未链接
  - 或 `onnxruntime.dll` 未就位
  - 或当前 host 不是 Windows 原生主线
  - 就不会把 Windows 原生执行层伪装成可用 backend
- `inference` 已新增正式构建特性：
  - `windows_native_onnx`
  - 用于显式表示当前 Windows build 是否已链接原生小模型执行 backend
- 当前会显式产出：
  - `small_model_runtime_target`
  - `small_model_runtime_readiness`
  - `small_model_runtime_reason`
  - `main_brain_runtime_target`
- 当前 readiness 不再只看 `DirectML.dll`
  - 还会进一步检查：
    - `windows_native_onnx` 是否已链接
    - `onnxruntime.dll` / `ORT_DYLIB_PATH` 是否可用
- 以及每个 role 当前配置的：
  - `artifact_kind`
  - `target_readiness`
  - `target_reason`
- `gateway /api/system/local-model-stack` 与 `panel > Models > Local` 现在也会显式显示：
  - `effective_runtime_state`
  - `effective_runtime_reason`
  - 让“目标 backend”和“当前实际运行态”不再混成一层
- `gateway /api/system/local-model-stack` 与 `panel > Models > Local` 现在还会显式显示：
  - `windows_native_plan_status`
  - `windows_native_plan_note`
  - 让 `stt / ocr` 的“已评估、当前保留专用运行时”成为正式产品语义，而不是留在文档口径里
- `gateway /api/system/local-model-stack` 与 `panel > Models > Local` 现在还会继续显示：
  - `small_model_execution_provider`
  - `small_model_device_target`
  - `small_model_fallback_mode`
  - 让 Windows 原生小模型执行面的 provider/device/fallback 语义不再藏在实现细节里
- `gateway /api/system/local-model-stack` 已接入这套运行时状态
- `panel > Models > Local` 已能直接显示：
  - Windows 原生小模型执行面目标
  - 当前 host 是否处于 `windows_native_mainline` 还是验证路径
  - 当前小模型运行时 readiness 与原因
  - 当前模型包是 `onnx` 还是 `safetensors/gguf/external runtime`
  - 离 Windows 原生小模型目标还差什么
  - 每个 role 的 `Class / Failure Reason`
- `gateway /api/system/local-model-stack` 与 `panel > Models > Local` 现在也会把战术小模型 role 作为正式条目展示：
  - `nlu`
  - `fact_check`
  - 它们与 `embedding / rerank` 一样进入统一的 Windows-native `role binding / readiness contract`
- `gateway /api/system/local-model-stack` 与 `panel > Models > Local` 现在还会显式区分：
  - `host_validation_status`
  - `host_validation_note`
  - 用来把“backend 已接入”与“当前 Windows 产品主机上是否已观察到成功运行”分成两个正式状态，而不再混在同一个 `~` 语义里
- 当前这组 host validation 状态语义包括：
  - `validated_on_current_windows_host`
  - `pending_windows_host_validation`
  - `pending_windows_runtime_observation`
  - `not_required_specialized_runtime`
  - `not_required_main_brain_track`
- 当前不会再把“Windows 上探测到 DirectML runtime”直接展示成 `ready`
  - 若真实执行 backend 还未链接，会显式展示为 `windows_native_backend_unlinked`
  - 若 backend 已链接但 `onnxruntime.dll` 缺失，会显式展示为 `windows_native_runtime_missing`
- `stt / ocr` 现在也已完成“评估”这一半：
  - `stt` 当前正式产品路径保留在 `shared_stt_runtime`
  - `ocr` 当前正式产品路径保留在 `document_ocr_runtime / tesseract`
  - `gateway/panel` 会明确显示这两个 role 现在是 `evaluation_complete_keep_specialized_runtime`
- 因此 W2 现在已完成：
  - 统一执行面状态层
  - 契约识别
  - `onnx` 工厂识别
  - 真实 Windows-only ORT backend 代码落位
  - 创建阶段前置失败
  - gateway/panel 实际运行态透出
  - `stt / ocr` 的 Windows-native 评估结论显式化
  - 战术小模型 `nlu / fact_check` 的统一 role binding / readiness contract
  - `embedding / rerank` 的 host validation 状态显式化
- 这里的 `windows_native_*` 状态字段也都按“统一 Windows 原生执行层”理解，不代表只针对某一种 GPU 厂商
- `describe_binding()` 现在也能把 `model.onnx` 目录识别成：
  - `onnx_embedding_winml`
  - `onnx_rerank_winml`
- 尚未完成的是：
  - 在真实 Windows 产品主机上捕获 `embedding / rerank` 的首次成功运行样本
  - 将该样本继续沉淀成长期 regression 基线

#### Phase W3: Gateway / Panel Windows Native 可见化

- `[x]` 在 `gateway` 暴露统一的 Windows 原生 backend/readiness/source/profile 读面
- `[x]` 在 `panel` 的 `Models > Local` 中显示：
  - `[x]` 当前 role 绑定
  - `[x]` 当前 backend
  - `[x]` 当前执行面来源（`llama.cpp` / `ONNX Runtime + DirectML / WinML` / fallback）
  - `[x]` readiness / fallback reason / degraded 状态
  - `[x]` top-level execution provider / device target / fallback mode
  - `[x]` windows-native plan / evaluation note
- `[x]` 明确区分“Windows 原生正式主线”与“WSL2 测试入口”，避免用户把测试路径误判为默认部署路径

#### Phase W4: Windows Native Trace / Witness / Fallback 收口

- `[x]` 让 `ONNX Runtime + DirectML / WinML` 小模型执行面进入统一 trace / stage metadata / witness
- `[x]` 为 Windows 原生 backend 显式记录：
  - `[x]` backend kind
  - `[x]` execution provider
  - `[x]` device/fallback
  - `[x]` readiness/degraded reason
- `[x]` 将 Windows 原生 backend 失败、CPU fallback、模型不兼容等结果纳入统一 outcome / strategy contract
- `[x]` 确保 `panel` 与 `gateway` 能直接读到这些 Windows 原生诊断字段

当前进展（2026-03-29）：
- `brain` 已新增独立 `windows_native_trace` 模块
- `RunTrace.metadata` 现在会显式带：
  - `windows_native_host_runtime`
  - `windows_native_product_mainline`
  - `windows_native_validation_tracks`
  - `windows_native_priority`
  - `windows_native_small_model_runtime_target`
  - `windows_native_small_model_execution_linked`
  - `windows_native_small_model_execution_provider`
  - `windows_native_small_model_device_target`
  - `windows_native_small_model_fallback_mode`
  - `windows_native_small_model_runtime_outcome`
  - `windows_native_small_model_runtime_strategy`
  - `windows_native_small_model_runtime_readiness`
  - `windows_native_small_model_runtime_reason`
  - `windows_native_main_brain_runtime_target`
- `RuntimeStage::Execution` / `RuntimeStage::TraceAudit` 现在也会回放这组 Windows-native metadata
- `telemetry` 的 witness notes 现在会投影：
  - `runtime_windows_native_*`
- `gateway/panel` 现在除了 `effective runtime`，还会显式显示：
  - `Deployment Lane / Strategy / Note`
  - top-level `Small Model Outcome / Strategy`
  - role-level `Effective Outcome / Class / Failure Reason / Strategy`
- `inference` 侧现在已把 Windows-host 的关键状态分类抽成纯逻辑 helper，并有单测覆盖：
  - `windows_native_active`
  - `runtime_missing`
  - `backend_unlinked`
  - `accelerator_unavailable`
  - `validation_only`
- `inference` 侧新增了统一 `windows-native` 失败诊断 helper，能够把真实小模型错误归并到：
  - `model_contract_incompatible`
  - `accelerator_resource_exhausted`
  - `cpu_fallback_no_accelerator_route`
  - `cpu_fallback_provider_downgrade`
  - `cpu_fallback_active`
  - `windows_native_provider_execution_failed`
  - `windows_native_execution_failed`
  - 或当前 host/runtime 对应的既有 outcome / strategy
- `factory_impls` 已开始在真实 `onnx_embedding_winml / onnx_rerank_winml` 创建链路中附带这套 outcome / strategy 诊断
- `engram` 侧的真实执行链也已开始复用这套诊断：
  - `Embedder::new/embed/embed_batch`
  - `LocalCandleReranker::new/rerank`
  - `HybridSearchEngine` 不再静默吞掉 reranker 初始化失败，而是显式记录 fallback 告警
- `engram` 的 `HybridSearchStats` 现在也开始结构化记录：
  - `windows_native_embed_outcome / class / strategy / note`
  - `windows_native_rerank_outcome / class / strategy / note`
- `engram` 的 `HybridSearchStats` 现在也会继续显式记录 role 级：
  - `provider`
  - `device_target`
  - `fallback_mode`
- 这些字段已经进入：
  - `BeforeResponse` runtime hook metadata
  - `RunTrace.metadata`
  - `RuntimeStage::Execution / TraceAudit`
  - witness notes
  - scorecard / eval 的 `warn` 与 `failure_reasons`
  - `gateway /metrics`
  - `panel` 的 metrics DTO
  - `agent_memory` runtime metadata（`engram.windows_native.*`）
- `panel` 的 Metrics 子页现在也会直接显示：
  - `Embedding` 的 Windows-native `Outcome / Class / Strategy / Note`
  - `Embedding` 的 `Provider / Device / Fallback`
  - `Rerank` 的 Windows-native `Outcome / Class / Strategy / Note`
  - `Rerank` 的 `Provider / Device / Fallback`
  - 并新增显式 `Class`，用于区分：
    - `provider_downgrade`
    - `no_accelerator_route`
    - `provider_failure`
    - `runtime_failure`
    - `fallback_runtime`
- `telemetry` 的查询面现在也支持按 Windows-native 小模型角色结果直接筛选：
  - `WitnessLogQuery` 可按 `embed/rerank outcome/strategy` 精确过滤
  - `ScorecardQuery` 也可按同一组 `embed/rerank outcome/strategy` 过滤关联 scorecard
  - 两者现在都可按标准 `windows_native::<role>::<outcome>` failure reason 直接筛选
  - 两者现在也可按粗粒度 `Class` 直接筛：
    - `provider_downgrade`
    - `no_accelerator_route`
    - `provider_failure`
    - `runtime_failure`
    - `fallback_runtime`
- `panel` 的选中 witness 卡片现在会直接暴露这组快速筛选入口：
  - 可以一键筛 `Matching Witness Logs`
  - 也可以一键筛 `Matching Scorecards`
  - 并会在存在非 active outcome 时显式显示对应 `Class / Failure Reason`
- 这意味着 Windows 原生小模型执行面已经不只在 `gateway/panel` 可见，也进入了统一 trace/witness 主线，并且 role 级 `provider/device/fallback` 也已进入结构化证据链
- 因此 W4 已完成；当前剩余缺口已收敛到：
  - 在真实 Windows host 上完成 `embedding / rerank` 的运行验证
  - 将真实 Windows host 上的执行结果沉淀成长期 regression 样本

---

## 8. 用户未配置小模型时的 Fallback Matrix

在 **Windows 原生优先** 新阶段里，需要明确区分三种状态：

- `windows_native_active`
  - 已配置并成功走 `ONNX Runtime + DirectML / WinML`
- `fallback_runtime_active`
  - Windows-native 小模型执行面未接管，但旧专用后端仍承担主路径
- `unconfigured / pending / incompatible`
  - 用户未配置、runtime 未就绪、或模型包契约不兼容

执行口径应固定如下：

- `LLM / SLM`
  - 若用户未配置 Windows-native 小模型层，不影响主脑主线
  - 继续按 `llama.cpp` 主脑路径运行

- `embedding / rerank`
  - 若用户已提供 `onnx` 且 Windows-native runtime ready：
    - 优先走 `ONNX Runtime + DirectML / WinML`
  - 若用户未提供 `onnx`，但已有旧专用后端可用：
    - 明确标记为 `fallback_runtime_active`
    - 继续由既有 `Candle / safetensors` 路径承担
  - 若用户既未配置可用模型、又无旧后端可承担：
    - 明确标记为 `unconfigured`
    - 不伪装成已可用

- `STT / OCR`
  - 当前仍以既有专用后端为主路径
  - Windows-native 统一执行层接管前：
    - 用户未配置 Windows-native 小模型，不应阻断现有专用后端
    - 但 panel/gateway 必须明确显示当前仍在 specialized/fallback 路径

- `TTS`
  - 保持专用后端
  - 不因未接入 `ONNX Runtime + DirectML / WinML` 而被视作异常

- `WSL2 / Linux ROCm`
  - 只能作为 `validation_only`
  - 不得因为验证路径可运行，就把产品态标成 Windows-native ready

产品与诊断面约束：

- `panel / gateway`
  - 必须同时显示：
    - `preferred backend`
    - `current backend`
    - `effective runtime`
    - `outcome / strategy / note`
- 若用户未配置或配置不兼容：
  - 必须显式显示 `unconfigured / incompatible / runtime_missing / backend_unlinked`
  - 不能默默回退后让用户误以为已经走到 Windows-native 主线

一句话原则：

`用户未配置小模型时，可以继续走旧专用后端或 fallback，但不能伪装成 Windows-native 主线已经可用。`

---

## 9. 当前建议优先级

当前最应优先做的是：

1. 完成 `Phase W1`，先把 `Windows 原生正式主线` 与 `WSL2 测试路径` 的边界写死
2. 完成 `Phase W2`，优先为 `embedding / rerank` 接入 `ONNX Runtime + DirectML / WinML`
3. 完成 `Phase W3`，把 `Models > Local` 的 backend/readiness/source/fallback 视图补齐
4. 完成 `Phase W4`，让 Windows 原生 backend 与 fallback 全量进入 trace / witness

当前不应优先做的是：

- 把 `LLM / SLM` 主脑为了“统一”硬改成 `ONNX`
- 继续把 `WSL2 / Linux ROCm` 写成与 Windows 原生平级的默认产品路径
- 把所有模型 trait 强行合并成一个万能接口
- 让主 LLM 直接承担媒体前处理职责

---

## 10. 最终原则

最终要实现的是：

`模型系统统一管理，媒体预处理独立成面，二者通过受控中间结果连接。`

这比“所有东西都塞进一个大模型接口”更稳定，也更符合 BenShu 的运行时架构方向。

在 **原生 Windows 优先** 前提下，还应进一步明确成：

- `LLM / SLM`
  - 主脑层
  - 优先保持 `llama.cpp`
- `embedding / rerank / stt / ocr / tactical small models`
  - 小模型层
  - 逐步统一到 `ONNX Runtime + DirectML / WinML`
- `tts`
  - 保持专用后端
- `WSL2 / Linux ROCm`
  - 仅承担测试、对照、实验职责
  - 不再作为默认用户部署路径

同时，本计划完成不只以 backend/runtime 接线为准，还应包含：

- `apps/panel` 对应组件与状态展示的同步重构
- Panel 侧能力入口、术语、状态与主路径 runtime 语义保持一致
- 已被替代的旧 Panel 交互面在确认无误后同步回收
