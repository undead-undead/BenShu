# BenShu 个人 Jarvis 落地路线图

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 关联核心文档: `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`
>
> 关联执行蓝图: `docs/secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md`
>
> 关联前台架构立场: `docs/secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md`
>
> 关联背景连续性主线: `docs/secondary/BENSHU_BACKGROUND_COMPRESSION_MAINLINE_PLAN_ZH.md`
>
> 文档定位: 这是“下一阶段产品落地路线”文档，不替代工程总规范，也不改写主执行计划。

---

## 0. 文档目标

这份文档用于回答一个更接近产品落地的问题：

在 AgentOS 主线重构基本收口之后，BenShu 接下来应该如何继续演进，才能真正成为“个人的、真正开箱即用的 Jarvis”。

本文件关注的是：

- 用户第一次安装后是否能直接用
- 默认体验是否稳定、连贯、可恢复
- 本地模型、工具、记忆、治理是否服务于同一个个人助手形态
- 后续阶段应该按什么顺序继续施工

本文件不负责：

- 改写已有重构阶段定义
- 替代 tracing 契约
- 替代 crate 级技术规格

---

## 1. 北极星定义

### 1.1 我们要做的不是“一个很强的 Agent Demo”

BenShu 的目标不是做一个单项能力很强、但需要大量人工配置和理解内部结构才能用起来的实验系统。

我们的目标是：

- 用户安装后就能直接开始对话
- 系统可以连续理解、执行、记忆、恢复
- 用户不需要先理解 swarm、artifact、witness、delegation 才能使用
- 系统默认站在用户一边，帮助用户完成目标，而不是管控用户

### 1.2 “个人真正的 Jarvis” 的最小定义

若 BenShu 要称得上“个人真正的 Jarvis”，至少应满足：

1. 默认只有一个前台人格，即 `BenShu`
2. 默认交互路径足够简单，能直接开始对话与做事
3. 用户能随时打断、插话、停止、恢复控制
4. 系统能记住关键上下文，并在重启后恢复关键状态
5. 默认配置在本地可运行，云能力只是增强项，不是前提
6. 当能力不足时，系统会安全降级，而不是直接崩掉或沉默失败

---

## 2. 长期产品语义

### 2.1 单一前台，后台多专家

用户面对的始终是一个统一的 `BenShu`。

后台可以继续存在：

- specialist agents
- A2A delegation
- retrieval specialists
- coding / research / multimodal execution agents

但这些都不应成为用户必须理解的前提。

### 2.2 所有机制都以用户利益为先

治理、审批、限流、降级、恢复、背压与 query protection 的目标，都应是：

- 保护用户安全
- 避免用户损失
- 防止系统失控
- 尽量帮助用户继续完成目标

任何机制都不应演化成“限制用户”本身。

### 2.3 本地优先，但不排斥云增强

默认产品语义应是：

- 在没有复杂云配置的情况下，本地也能完成基础对话、工具执行和记忆主路径
- 云模型、远程服务、多模态增强是加分项，不是开机前置条件

---

## 3. 后续阶段总顺序

在当前主线重构收口之后，建议按下面顺序推进：

1. `阶段 A0`: 最小本地测试模型闭环
2. `阶段 A`: 整个 agent 主链路端到端冒烟收口
3. `阶段 B`: 本地大模型接口彻底打通
4. `阶段 C`: 真正的开箱即用体验收口
5. `阶段 D`: 长期稳定性、恢复与持续运行能力强化

这个顺序的原因很简单：

- 没有最小本地测试模型闭环，主链路 smoke 很难稳定复现
- 没有主链路 smoke，后续所有增强都容易把问题混在一起
- 没有本地模型主路径，无法称得上“个人开箱即用”
- 没有体验收口，系统依然更像工程平台而不是个人助手
- 没有恢复与持续运行能力，就很难形成真正长期可信赖的个人系统

---

## 4. 阶段 A0: 最小本地测试模型闭环

### 4.1 目标

在继续扩大 smoke 覆盖面之前，先建立一条稳定、可下载、可缓存、可自动探测、可在本地和 CI 复用的最小模型主路径。

### 4.2 这一阶段不是“完整本地模型体系”

本阶段的目标不是一次性完成所有本地大模型能力，而是先拿到一组足够稳定的最小模型接口，让：

- smoke 能真正跑起来
- 问题能稳定复现
- 后续 provider / capability / degrade 不会和“模型根本没接好”混在一起

### 4.2.1 WSL2 在本阶段的角色

本项目的正式产品主环境仍然是 `Windows 原生`。

`WSL2` 在本阶段的定位不是产品落地环境，而是：

- GPU 推理测试通道
- 本地模型接口验证通道
- 最小 smoke 的加速验证环境

也就是说：

- 正式产品语义、正式开箱体验、正式 Windows 兼容性，最终都必须回到 `Windows 原生`
- 但在 A0 阶段，为了更快打通本地 GPU 推理与最小模型闭环，允许先使用 `WSL2 + GPU` 作为开发与测试环境

### 4.2.2 为什么先借 WSL2 打通测试通道

这样做的目的不是改变产品目标，而是降低 A0 的启动成本：

- 更快验证本地模型是否真的能跑
- 更快验证 GPU 推理、下载、缓存、回退逻辑
- 更快验证 6 类最小接口矩阵
- 把“模型没接好”和“Windows 原生产品路径问题”分开排查

### 4.3 最小本地模型接口矩阵

参考 Hugging Face 当前任务分类与通用 agent 小模型演进方向，BenShu 应优先收口这 6 类本地接口：

1. `Text Agent Core`
2. `Multimodal Agent Core`
3. `Embedding`
4. `Rerank`
5. `STT`
6. `TTS`

说明：

- `Text Agent Core` 对应 text-only 小模型，用于最小 chat / tool / reasoning 主路径
- `Multimodal Agent Core` 对应 image-text-to-text / any-to-any 小模型，用于视觉、文档、多模态输入主路径
- `OCR` 不再作为长期顶级主类，而应收敛为：
  - `Multimodal Agent Core` 的能力之一
  - 或 `document / vision fallback`

### 4.4 这一阶段要做的事

- 定义最小模型 profile 与稳定默认值
- 明确下载 / 缓存 / 探测 / 校验 / 回退逻辑
- 让本地测试、开发机、未来 CI 共享同一套模型入口语义
- 把 provider metadata / capability view 与这 6 类接口对齐
- 给 OCR 做接口重定位：
  - 保留 backend
  - 改上层路由
  - 默认优先多模态主模型
  - 专门 OCR 退为 fallback
- 建立 `WSL2 + GPU` 测试通道：
  - 只作为 A0 测试/验证环境
  - 不改变 `Windows 原生` 的正式产品主环境定位
  - 先验证最小模型下载、加载、推理、回退
  - 验证完成后把稳定接口与 profile 回灌到 `Windows 原生` 主路径

### 4.4.1 A0 推荐的分步顺序

1. 打通 `WSL2 + GPU` 的最小推理验证环境
2. 在这条通道上验证：
   - `Text Agent Core`
   - `Embedding`
   - `Rerank`
   - `STT`
   - `TTS`
   - `Multimodal Agent Core`
3. 固化统一的 capability / metadata / degrade 语义
4. 把验证稳定的接口与 profile 回灌到 `Windows 原生` 主路径
5. 再进入 `阶段 A` 的整链路 smoke

### 4.4.2 `WSL2 + GPU` 测试通道执行清单

为避免 A0 再次扩散，本阶段默认只做“最小可验证闭环”，不追求一步到位。

第 1 组：环境底座确认

- 确认 `WSL2` 内核、`/dev/dxg`、GPU 可见性
- 确认用户态 GPU 工具链是否可用，例如：
  - `nvidia-smi`
  - `libcuda`
  - `rocminfo`
  - `libhsa-runtime64`
  - `vulkaninfo`
- 明确当前测试机属于哪条路径：
  - `WSL2 + NVIDIA CUDA`
  - `WSL2 + AMD ROCm`
  - `WSL2 + Vulkan`
  - `WSL2 CPU fallback`

当前建议的厂商分路：

- `NVIDIA`:
  - 优先 `WSL2 + CUDA`
  - `Vulkan` 仅作为特定后端或补充路径
- `AMD Radeon 7000 / 9000`:
  - 优先 `WSL2 + ROCm`
  - 不把纯 `Vulkan` 视为默认主推理路径
- `Intel`:
  - 暂不作为 A0 默认 GPU 主路径
  - 可先允许 CPU 或后续专项适配

当前代码现状判断：

- `Windows 原生双路` 的探测与 profile 分流已经部分写好
- `DXGI` 探测、`NVIDIA -> CudaPreferred`、`AMD -> VulkanPreferred` 都已有代码表达
- 但它们目前更接近“适配意图已写”，还不是“真实双路 GPU 推理闭环已经过完整 smoke”
- A0 的重点不是重新发明双路，而是把这些已写适配真正验证成可运行主路径

### 4.4.2.1 A0 runtime profiles

为避免后续继续把不同平台和不同 GPU 路径混在一起，A0 先显式收口这些 profile：

测试期 profile：

- `local_wsl2_nvidia_cuda`
- `local_wsl2_amd_rocm`
- `local_wsl2_cpu_fallback`

正式产品期 profile：

- `native_win_nvidia`
- `native_win_amd`
- `native_win_cpu_fallback`

要求：

- profile 必须显式进入 capability / metadata / tracing
- 不允许继续依赖“猜当前大概是什么环境”的隐式分支
- 同一套 6 类模型接口在不同 profile 下只能改变后端实现与 degrade 行为，不能改变顶层产品语义

第 2 组：BenShu 编译与特性确认

- 明确 `benshu-inference` 的测试期 feature 组合
- 明确哪些 crate 需要打开：
  - `cuda`
  - `llama_cpp`
  - `vulkan`
- 若后续引入 `rocm/hip` 路径，也必须形成显式 feature 或 runtime profile 约束
- 明确测试期 profile，不把开发期临时 feature 直接冒充正式产品默认配置

第 3 组：最小模型组确认

- 为 `Text Agent Core` 选 1 个最小模型
- 为 `Multimodal Agent Core` 选 1 个最小模型
- 为 `Embedding` 选 1 个最小模型
- 为 `Rerank` 选 1 个最小模型
- 为 `STT` 选 1 个最小模型
- 为 `TTS` 选 1 个最小模型

选型原则：

- 体积尽量小
- 下载与缓存逻辑稳定
- 社区与官方生态成熟
- 能明确表达 capability
- 失败时可安全回退

第 4 组：统一入口确认

- 统一模型下载入口
- 统一缓存目录约定
- 统一探测与健康检查入口
- 统一 capability metadata 输出
- 统一 degrade / fallback 语义

第 5 组：最小验证脚本与 smoke

- 单模型加载验证
- 单轮推理验证
- 能力探测验证
- fallback 验证
- `trace / task / witness` 最小联通验证

第 6 组：回灌到 Windows 原生

- 把验证通过的 capability 语义迁回 `Windows 原生` 主路径
- 把下载、缓存、探测、profile、fallback 设计迁回正式环境
- 保证 `Windows 原生` 仍是唯一正式产品环境
- 保证 `WSL2` 只是测试与验证辅助路径

### 4.4.3 `WSL2 + GPU` 测试通道验收口径

当且仅当下面条件成立，才可认为 A0 的测试通道基本打通：

1. `WSL2` 内能稳定识别当前 GPU 路径
2. 至少 1 个 `Text Agent Core` 模型能稳定完成本地推理
3. `Embedding + Rerank` 能稳定完成最小检索闭环
4. `STT + TTS` 至少能跑通最小音频闭环
5. `Multimodal Agent Core` 或其 fallback 已能稳定完成最小读图/读文档路径
6. 失败时能明确知道是：
   - 环境问题
   - 模型问题
   - feature 编译问题
   - provider / capability 接线问题
7. 已形成可以回灌到 `Windows 原生` 的稳定接口口径

### 4.4.4 A0 阶段明确不做的事

为控制范围，A0 默认不做下面这些更重的工作：

- 不把 `WSL2` 变成正式产品主运行环境
- 不一次性补完所有本地模型族
- 不在 A0 阶段追求全部多模态能力都走 GPU
- 不把 OCR 继续扩成独立顶级主接口
- 不为了测试方便破坏 `Windows 原生` 最终落地目标

### 4.4.4.1 当前双路适配盘点结论

截至当前阶段，可以把现状判断为：

已写：

- `Windows DXGI` GPU 探测已存在
- `NVIDIA -> CudaPreferred`
- `AMD -> VulkanPreferred`
- `TensorRT eligible` 的 NVIDIA 判定已存在
- 部分 runtime metadata / profile 语义已接入 provider 读面

未测：

- 未形成真正的 `Windows 原生 NVIDIA` 端到端 smoke
- 未形成真正的 `Windows 原生 AMD` 端到端 smoke
- 未形成 `WSL2 + AMD ROCm` 最小闭环 smoke

缺口：

- `AMD` 路当前更多停留在探测与 profile 偏好层，还没有真正的本地 GPU 推理闭环
- `NVIDIA` 路虽然更接近可用，但仍需真实 smoke，而不是只靠 capability 判定
- `llama.cpp` 当前仍有部分 GPU 分路逻辑需要继续清理，但 `CUDA` 误绑 `Vulkan` 的一处关键耦合已修正

因此 A0 的核心工作不是“重新设计双路”，而是：

- 把已写的分路意图验证成真实可跑的测试通道
- 再把稳定语义回灌到 `Windows 原生`

### 4.4.4.2 `local_wsl2_amd_rocm` 可行性结论

基于当前官方资料，`local_wsl2_amd_rocm` 对本项目是可行路线，但应明确它是“官方支持但有限制”的测试通道，而不是无条件稳定的最终运行环境。

已确认的正面条件：

- `AMD Radeon RX 7900 XTX` 属于 AMD 官方列出的 WSL 支持 GPU 范围
- `Ubuntu 22.04 + WSL2` 属于 AMD 官方推荐与支持组合
- AMD 官方明确提供了 `WSL How to guide - Use ROCm on Radeon GPUs`

需要接受的限制：

- `amd-smi` 在 WSL 中不支持
- `ROCm-profiler` 在 WSL 中不支持
- 调试器在 WSL 中不支持
- WSL2 下 LLM / PyTorch 推理可能出现性能低于原生 Linux、脚本失败、驱动超时等问题
- 在 Python 虚拟环境中运行时，可能需要手动修复 `libhsa-runtime64.so`

对 BenShu 的含义：

- 这条路适合作为 A0 测试与模型验证通道
- 不适合被误判为“已经等于正式产品 GPU 主路径”
- 我们必须在产品语义上继续坚持：
  - `Windows 原生` 才是正式主环境
  - `WSL2 + AMD ROCm` 只是测试与验证辅助路径

### 4.4.4.3 `local_wsl2_amd_rocm` 当前代码缺口

截至当前代码状态，这条路线的“环境层”已经成立，但推理栈仍处在第一阶段接线状态：

- `HardwareStatus` 现在已经能表达：
  - `ROCm/HIP` 运行时是否可用
  - `rocminfo` fallback 探测来源
  - AMD WSL2 环境下的 GPU 身份与基本 runtime 语义
- `provider/runtime metadata` 现在已经能暴露：
  - `runtime:rocm-available`
  - `runtime:probe-source:rocminfo`
- `llama.cpp` 侧已修正：
  - `CUDA` offload 不再误绑 `Vulkan`
- `llama.cpp + ROCm` 第一阶段接线已建立：
  - `benshu-inference` 已接入 `rocm` feature
  - 第一阶段按 `dynamic-link` 方式编译，避开当前 WSL2 静态链接 `-fPIC` 问题
  - 已新增最小 `GGUF` smoke harness
- `量化 GGUF` 已完成第一次真实冒烟：
  - 模型：`Qwen/Qwen2.5-0.5B-Instruct-GGUF`
  - 量化档位：`Q4_K_M`
  - 文件：`qwen2.5-0.5b-instruct-q4_k_m.gguf`
  - 路径：`models/smoke/qwen2.5-0.5b-instruct-q4_k_m.gguf`
  - 结果：在 `WSL2 + AMD ROCm/HIP + RX 7900 XTX` 上成功完成模型加载、GPU 全层 offload 与单轮生成
  - 入口：smoke 现已优先按 `GGUF_MODEL_PATH` 覆盖，其次查固定路径 `models/smoke/...`，最后自动从 Hugging Face 拉取并走缓存
- 但 `benshu-inference` 目前仍没有真正成形的 `ROCm/HIP` 本地推理后端
- 也还没有：
  - 默认随仓测试模型
  - 默认随仓测试模型

因此 `local_wsl2_amd_rocm` 的现实任务应拆成两段：

1. 先验证环境层是否成立
2. 再补本地推理栈对 ROCm 的最小模型接线与 smoke

### 4.4.4.4 `local_wsl2_amd_rocm` 最小落地清单

第一步：环境成立性

- 确认宿主 Windows 驱动版本与 AMD WSL 支持矩阵匹配
- 确认 WSL 发行版为官方支持版本
- 安装 WSL 版 Radeon/ROCm 用户态
- 验证最小命令是否能识别 AMD GPU

第二步：运行库成立性

- 确认 `libhsa-runtime64.so`
- 确认 HIP / ROCm 基本运行库
- 确认虚拟环境下 runtime library 不会被错误覆盖

第三步：BenShu 侧成立性

- 已完成第一阶段：
  - `HardwareStatus` 增加 `ROCm/HIP` runtime 感知
  - Linux/WSL AMD 探测增加 `rocminfo` fallback
  - provider/runtime metadata 增加 `runtime:rocm-available`
  - 修正 `llama.cpp` 的 `CUDA`/`Vulkan` 判定耦合
  - `benshu-inference` 接入 `rocm` Cargo feature
  - 增加 `GGUF_MODEL_PATH` 驱动的 `llama.cpp + ROCm` 最小 smoke harness
  - 已用量化 `Q4_K_M` GGUF 跑通真实单轮推理 smoke
- 下一步继续：
  - 为 `local_wsl2_amd_rocm` 建立显式 runtime profile
  - 固化默认 smoke 模型下载/缓存入口
  - 建立最小 smoke：
    - 模型加载
    - 单轮推理
    - capability 输出
    - degrade 语义

第四步：回灌 Windows 原生

- 把在 WSL2 上验证稳定的接口语义迁回 `native_win_amd`
- 保证最终产品主路径仍在 Windows 原生环境

### 4.4.4.5 当前机器的实测状态

基于当前开发机的实际检查，`local_wsl2_amd_rocm` 现在已经完成环境层打通，并已能识别 `RX 7900 XTX / gfx1100`。

当前已满足：

- 宿主机 GPU: `AMD Radeon RX 7900 XTX`
- WSL 发行版: `Ubuntu 22.04.5`
- WSL 内核: `6.6.87.2-microsoft-standard-WSL2`
- `WSLg` 已启用
- `/dev/dxg` 存在，说明 GPU 透传底座已在
- 已安装 `amdgpu-install`
- 已安装 `ROCm 7.2`
- 已存在 `rocminfo`
- 已存在 `hipcc`
- 已存在 `rocm_agent_enumerator`
- 已存在 `libhsa-runtime64.so / libamdhip64.so / libhiprtc.so / librocblas.so`
- `rocminfo` 已识别：
  - `Name: gfx1100`
  - `Marketing Name: AMD Radeon RX 7900 XTX`
- `hipconfig --platform` 已返回 `amd`
- `rocm_agent_enumerator` 已返回 `gfx1100`

当前未满足：

- `rocm-smi` 在当前 WSL 环境下仍不可用，报错为 `Driver not initialized (amdgpu not found in modules)`
- 当前 `vulkaninfo` 仍落在 `llvmpipe`
- `benshu-inference` 仍没有成形的 `ROCm/HIP` 推理后端，项目侧闭环尚未建立

因此当前判断是：

- 这台机器“已经打通 `local_wsl2_amd_rocm` 的环境层与运行库层”
- 但“项目侧 ROCm 推理闭环尚未建立”
- A0 下一步应转向 `BenShu` 侧最小 ROCm 接线，而不是继续纠缠环境安装

### 4.4.4.6 `local_wsl2_amd_rocm` 官方安装命令清单

对当前这台 `Ubuntu 22.04.5 + WSL2 + RX 7900 XTX` 机器，AMD 官方当前口径给出的 WSL 安装主路径是：

前提检查：

- 宿主 Windows 已安装兼容的 WSL 专用 AMD 驱动
- WSL 发行版为 `Ubuntu 22.04`
- 安装后需要重启宿主机

Ubuntu 22.04 安装 `amdgpu-install`：

```bash
sudo apt update
wget https://repo.radeon.com/amdgpu-install/7.2/ubuntu/jammy/amdgpu-install_7.2.70200-1_all.deb
sudo apt install ./amdgpu-install_7.2.70200-1_all.deb
```

安装 WSL + ROCm usecase：

```bash
sudo amdgpu-install -y --usecase=wsl,rocm --no-dkms
```

安装后验证：

```bash
rocminfo
```

按 AMD 官方说明，成功时应至少看到类似：

- `Name: gfx1100`
- `Marketing Name: Radeon RX 7900 XTX`

对本项目的补充验证命令建议：

```bash
ldconfig -p | rg 'libhsa-runtime64|libamdhip64'
which rocminfo
```

当前机器与上面清单的对照结果：

- 已满足：`Ubuntu 22.04.5`
- 已满足：`WSL2`
- 已满足：`RX 7900 XTX`
- 已满足：`amdgpu-install`
- 已满足：`rocminfo`
- 已满足：`ROCm/HIP/HSA` 运行库
- 已满足：`HIP compiler (hipcc)`
- 已验证：`rocminfo` 能识别 `gfx1100 / AMD Radeon RX 7900 XTX`
- 已验证：`hipconfig --platform = amd`
- 已验证：`rocm_agent_enumerator = gfx1100`
- 未满足：`rocm-smi` 可用性
- 未满足：`vulkan` GPU 路
- 未确认：宿主 Windows 是否已安装 AMD 官方 `Adrenalin Edition 26.1.1 for WSL2` 或兼容版本

执行策略：

- 现在不再重复环境安装
- 先以 `rocminfo + hipcc + runtime libraries` 作为 A0 的 AMD 环境验收基线
- 明确接受 `rocm-smi` 在 WSL 下不可作为强依赖
- 直接进入 `BenShu` 侧的最小 ROCm 接线与 smoke

### 4.4.5 A0 默认模型候选

A0 阶段的策略不是一开始就支持海量模型，而是先给每一类接口配 1 组“常用且稳定”的默认候选，优先保证：

- 下载稳定
- 生态成熟
- 接口清晰
- 回退容易

建议默认候选如下：

1. `Text Agent Core`
   - 默认: `Qwen/Qwen2.5-3B-Instruct`
   - 低配回退: `Qwen/Qwen2.5-1.5B-Instruct`
   - GGUF 常用分发可优先兼容同系量化版本

2. `Multimodal Agent Core`
   - 默认: `Qwen/Qwen2.5-VL-3B-Instruct`
   - 更轻量 smoke 候选: `HuggingFaceTB/SmolVLM2-2.2B-Instruct`

3. `Embedding`
   - 默认: `BAAI/bge-small-en-v1.5`
   - 当前 A0 smoke 已实测通过: `BAAI/bge-small-en-v1.5`

4. `Rerank`
   - 默认最小: `BAAI/bge-reranker-v2-minicpm-layerwise`
   - 常用升级: `BAAI/bge-reranker-v2-m3`
   - 当前 A0 smoke 先锁定兼容现有 `XLM-RoBERTa` loader 的 `BAAI/bge-reranker-base`

5. `STT`
   - 默认: `openai/whisper-base`
   - 更强但更重: `openai/whisper-small`
   - 当前 A0 smoke 已先用更轻的 `openai/whisper-tiny` 跑通真实本地转写链路

6. `TTS`
   - 默认: `piper-en_US-lessac-medium`
   - 以 `Piper` 兼容声线作为主路径，不把云 TTS 当成本地默认前提
   - 当前 A0 已先补 `Piper` 调用契约 smoke，锁定本地目录结构与进程调用接口

说明：

- 以上只是 A0 的默认候选，不是长期唯一支持列表
- 最终产品必须允许用户自由选择模型，只要模型声明满足统一 capability contract
- A0 的目标是先形成稳定默认值，而不是限制用户未来只能用这些模型

### 4.4.6 BenShu 量化策略

本项目需要自己的量化策略，而不是把量化完全交给外部分发约定。

对 A0 阶段，建议先采用下面这套清晰、保守、可解释的量化口径：

`Text Agent Core`

- 默认平衡档: `Q4_K_M`
- 高质量档: `Q5_K_M` 或 `Q8_0`
- 极低内存 smoke: `IQ4_XS` 或同级低内存量化

`Multimodal Agent Core`

- 默认优先保守，先不追求极限压缩
- 优先 `4-bit / 8-bit` 稳定量化
- 若多模态主模型效果不稳，可先允许 CPU 或较重配置运行，不强求 A0 就做到最省

`Embedding / Rerank / STT / TTS`

- 优先使用官方 safetensors / onnx / 原生小模型权重
- 这几类在 A0 阶段优先追求“稳定可跑”，不强行先做复杂量化矩阵
- 当前已完成第一阶段 smoke：
  - `Embedding`: `BAAI/bge-small-en-v1.5`
  - `Rerank`: `BAAI/bge-reranker-base`
  - `STT`: `openai/whisper-tiny`
  - `TTS`: `piper-en_US-lessac-medium`（当前为契约级 smoke）
  - 其中 `Embedding / Rerank / STT` 默认先走 `Candle + safetensors`

当前代码收口说明：

- 现已形成统一 `A0 model profile`：
  - `Text Agent Core`: `Qwen/Qwen2.5-0.5B-Instruct-GGUF` `Q4_K_M`
  - `Embedding`: `BAAI/bge-small-en-v1.5`
  - `Rerank`: `BAAI/bge-reranker-base`
  - `STT`: `openai/whisper-tiny`
  - `TTS`: `piper-en_US-lessac-medium`
- 现已形成统一总 smoke：
  - `cargo test -p benshu-inference test_a0_model_profile_smoke --test smoke_test --features 'llama_cpp rocm' -- --nocapture`
  - `scripts/run_a0_local_model_smoke.sh`
- 当前机器上这条总 smoke 已通过：
  - `WSL2 + AMD ROCm/HIP + RX 7900 XTX`
  - 5 类默认模型/接口已按同一 profile 串通
- 现已补上更贴近真实前台的 `live frontend baseline`：
  - 默认优先 `models/live/qwen2.5-3b-instruct-q4_k_m.gguf`
  - 真实加载 `data/agents/benshu/AGENT.md + IDENTITY.md`
  - 中英文短问答都会检查是否泄露 `[CRITIQUE] / <|end|> / <|user|>` 等内部标记
  - 当前 `3B Q4_K_M + ROCm` 实测：
    - 中文前台基线约 `852ms`
    - 英文前台基线约 `366ms`
    - 未再出现内部标记泄露

- `Embedding` 默认 smoke 已具备固定解析顺序：
  - `BENSHU_EMBED_MODEL_DIR`
  - `models/smoke/embedding/...`
  - Hugging Face cache 自动拉取
- `Rerank` 默认 smoke 已具备固定解析顺序：
  - `BENSHU_RERANK_MODEL_DIR`
  - `models/smoke/rerank/...`
  - Hugging Face cache 自动拉取
- `Rerank` 主路径已修正一处真实 bug：
  - 之前把 `position_ids` 错传成 `attention_mask`
  - 现在已改成正确的 `attention_mask + token_type_ids`
- `STT` 默认 smoke 已具备固定解析顺序：
  - `BENSHU_STT_MODEL_DIR`
  - `models/smoke/stt/...`
  - Hugging Face cache 自动拉取
- `STT` 主路径已修正一处真实 bug：
  - `audio_candle` 之前把 `mel_filters` 长度错误写死
  - 现在已改成按 `Whisper N_FFT` 正确校验
- `TTS` 第一阶段 smoke 当前收口为接口级契约验证：
  - 验证 `model.onnx + piper binary` 目录结构
  - 验证 stdin -> stdout 的本地进程调用语义
  - 下一阶段再补真实 `Piper` 模型与声线自动下载闭环
- 当前总 smoke 的残留观察项：
  - `llama.cpp/ROCm` 运行结束后还有 `SharedSignalPool` leak warning
  - 目前不阻塞 A0 通过，但后续需要单独清理
  - `llama-cpp-sys` 在共享 `target` 目录下重复构建 `rocm` 动态库时，安装阶段仍可能因 `File exists` panic
  - 当前可通过独立 `CARGO_TARGET_DIR` 规避，不阻塞本地 `3B + ROCm` 验证

总体原则：

- 默认优先“能稳定完成任务”的量化，不优先追求最小体积
- 量化选择必须服从 capability、显存预算和 degrade 语义
- 量化档位应成为 profile 的一部分，而不是散落在路径名和手工约定里

### 4.4.7 最终用户自由选择原则

即使 A0 阶段定义了默认模型组，正式产品仍应满足：

- 用户可以自由替换每一类模型
- 系统负责检查 capability 是否满足要求
- 若模型能力不足，系统应明确降级而不是静默失败
- 默认模型只用于：
  - 首次开箱
  - smoke
  - 文档示例
  - 最小支持口径

也就是说，BenShu 最终需要的是：

- `统一接口`
- `统一 capability`
- `统一 degrade`

而不是“把模型列表硬编码死”。

### 4.5 完成标准

- 至少 1 套最小模型组可以在本地稳定拉起
- `Text Agent Core + Embedding + Rerank + STT + TTS` 至少具备第一阶段 smoke
- 多模态读图 / 读文档路径已有统一入口语义
- OCR 已不再以独立顶级主接口口径继续扩张

---

## 5. 阶段 A: 主链路端到端冒烟收口

### 5.1 目标

先证明 BenShu 现在已经是一个可以稳定跑通的个人助手主体，而不是一组各自完成的子系统。

### 5.2 必须覆盖的主路径

- 单轮 chat
- 多轮连续会话
- tool 调用
- trace / replay / witness
- approval / deny
- delegation
- stop / interject
- restore-only backup

### 5.3 这一阶段要做的事

- 把现有分散 smoke 收成统一入口
- 给 Linux 和 Windows 建立同口径 smoke
- 每条 smoke 都输出稳定断言，而不是只看“不报错”
- 断言至少覆盖：
  - `task_id`
  - `trace_id`
  - `replayable`
  - `witness`
  - `task status`

阶段 A 第一批 smoke 入口现已固定为：

- `scripts/run_stage_a_agent_smoke.sh`

当前第一批主路径覆盖：

- `foreground chat -> trace / replay`
- `real harness foreground -> witness / replay`
- `approval guard -> deny`
- `prime ownership / delegation`
- `preemptive chat -> stop / interject`
- `gateway session tasks -> replay / witness / session-stop`
- `communication software -> bridge conversational control`
- `telegram inbound update -> text / callback 解析`
- `panel runtime state -> trace / witness 投影、selection 保持与 stale selection 清理`
- `panel chat stop state -> cancel promise 收尾与用户状态提示`

### 5.4 完成标准

- 有统一 smoke 入口
- 至少 3 条完整主路径稳定通过
- 主路径失败时能快速定位是 `gateway`、`brain`、`provider`、`tool`、`state` 哪一层出问题
- 第一批入口先不追求覆盖全部系统，但现在已经从 `brain runtime` 扩到 `gateway` 稳定读写口和 `panel` 运行时状态机基线

### 5.5 当前备注

- `panel` 当前很可能仍存在较多不符合个人用户直觉的交互与页面逻辑问题
- 这件事暂不阻塞当前 `阶段 A` 的主链路 smoke 推进
- 待 `阶段 A` 基线稳定后，需要单独做一轮 `panel` 逻辑审计与重构收口，而不是零碎修补
- 真实 `Telegram bot` 已完成 `getMe / getWebhookInfo / getUpdates` 验活，当前 token 与轮询基线正常
- 真实 `Telegram bot` 已确认收到首条真实入站消息：`/start` 与普通文本 `测试` 均能通过 `getUpdates` 看到
- `gateway` 侧现已补上 Telegram connector 启动链路，真实 bot 消息已被实际消费，日志确认收到了 `/start` 与 `测试`
- Telegram 主链路后续曾暴露过几类阻塞项，当前已收敛为稳定现状：
  - 本地 `GGUF` backend 缺口已补通，`benshu` 主 agent 可在 gateway 环境中加载本地量化模型并启动 `bridge`
  - `engram` 新增了统一的 sync/async bridge，避免在已有 Tokio runtime 内再次直接 `block_on`
  - 已替换 [embedder.rs](/home/biubiuboy/BenShu/crates/engram/src/embedder.rs)、[hybrid_search.rs](/home/biubiuboy/BenShu/crates/engram/src/hybrid_search.rs) 与 [local_reranker.rs](/home/biubiuboy/BenShu/crates/engram/src/local_reranker.rs) 的旧调用路径
  - `llama.cpp` 会话前缀失配时未彻底清空 KV cache，已在 [llama_cpp.rs](/home/biubiuboy/BenShu/crates/inference/src/backend/llama_cpp.rs) 修正，避免再次触发 `NoKvCacheSlot`
  - 旧 `SwarmRouter` 路由链已整体退出主线，worker 选择现已统一回到 `A2A / Coordinator / CapabilityRouter` 主路径，不再额外制造本地模型无谓超时
  - Telegram connector 发送层此前静默吞掉 `sendMessage` 非 2xx 错误，且默认强制 `Markdown`；现已在 [telegram.rs](/home/biubiuboy/BenShu/crates/connectors/src/telegram.rs) 改成纯文本发送、检查真实返回码，并对超长内容自动分片
- 进一步收口了实时通信链路的上下文治理：
  - `telegram / slack / discord / feishu / dingtalk / qq` 这类实时 connector 会话现在只保留更短的历史窗口，避免把外部通信聊天拖成超大本地 prompt
  - 外部 connector 不再把完整内部 provider/推理错误原样回显给用户，而是压成简短可重试提示，避免错误文本继续污染后续上下文
  - 即使主 agent 把内部错误当成“正常回复”产出，`gateway bridge` 现在也会在外发前再次压缩这类内容，避免 Telegram 再收到长内部报错
  - 实时 connector 上的 `interject / reprioritize` 现在不再先发固定“收到，正在调整计划”提示，避免用户把这类中间态文案误认为正式答复
  - 外发前会截掉 `<|end|>`、`[CRITIQUE]`、`<|user|>` 一类内部生成痕迹，避免本地模型的反思/模板残片直接出现在 Telegram 对话中
- 最新 live 调试还定位到一条“答非所问”的根因：
  - `gateway bridge` 之前只要看到 session 还挂着 `active agent` 映射，就会把新的普通入站消息误判为“插话/重规划”
  - 因此像“你好”“你是谁”这种本应作为新一轮正常对话处理的消息，会错误走到固定的 interjection 文案，例如“正在根据新意见重新调整计划”
  - 该判断现已改成必须同时满足“存在 active agent 映射”且“该 agent 当前确实还有 live foreground task”，否则一律按正常新回合处理
- 综上，Telegram 侧当前更适合视为“主链路已打通、仍需持续 live 回归”的状态，而不是继续把单次 live 调试现场当成完成口径

---

## 6. 阶段 B: 本地大模型接口彻底打通

### 6.1 目标

让 BenShu 在“纯本地默认配置”下也能成为真正可用的个人助手，而不是必须依赖外部模型供应商。

### 6.2 核心原则

- 本地能力优先接入主路径
- 模型差异必须由 contract 和 capability 表达，不靠隐式假设
- 能力不够时优先降级，不让系统整体不可用

### 6.3 这一阶段要做的事

- 统一本地 provider 接线
- 明确本地模型 profile：
  - 低配默认
  - 标准默认
  - 高配增强
- 把这些能力显式化：
  - context window
  - tool calling
  - multimodal
  - embeddings / rerank
  - fallback
- 建立 graceful degrade：
  - 上下文预算缩减
  - schema 压缩
  - 轻量检索回退
  - 能力缺失时明确说明

### 6.4 完成标准

- 在纯本地配置下可完成基础 chat + tool + memory 主链路
- 本地模型失败时不会拖死系统
- 用户能清楚知道当前是本地路径、云路径还是降级路径

---

## 7. 阶段 C: 真正的开箱即用体验收口

### 7.1 目标

把 BenShu 从“开发者可以配置好的系统”推进到“个人用户第一次安装就能开始使用的产品”。

### 7.2 这一阶段要做的事

- 收口首次启动体验：
  - 自动生成默认配置
  - 自动检测必要依赖
  - 缺失项给出明确修复建议
- 收口默认人格与工作模式：
  - 只有一个前台 `BenShu`
  - specialist 完全后台化
- 收口默认交互控制：
  - 聊天区只保留必要控制，例如一个 `停止`
  - 插话、暂停、改优先级优先走自然语言
- 收口默认读面：
  - `task`
  - `trace`
  - `replay`
  - `witness`
  - `restore`
- 单独重审 `panel` 的整体页面逻辑与用户心智：
  - 优先按“个人真正开箱即用的 Jarvis”重构，而不是继续叠加开发者视角控件
  - 优先解决入口分散、控制语义不一致、信息暴露层级混乱的问题

### 7.3 完成标准

- 新机器按文档启动后，最低配置即可进入可对话状态
- 用户不需要理解内部术语也能完成第一次对话和第一次执行
- 默认失败信息是“可理解、可恢复、可继续”的

---

## 8. 阶段 D: 长期稳定性与恢复强化

### 8.1 目标

让 BenShu 像一个长期陪伴的个人系统，而不只是一次性会话工具。

### 8.2 这一阶段要做的事

- 强化会话恢复
- 强化 restore-only backup 常态化运行
- 强化 retention / archive / prune 验证
- 强化长任务取消、恢复与解释
- 保证文档、测试、主路径一起演进

### 8.3 完成标准

- 系统重启后关键状态不丢
- 长任务中断后可解释、可恢复、可继续
- 关键恢复路径在各平台上持续可验证

---

## 9. 当前建议的直接施工顺序

如果从今天开始继续推进，建议按下面顺序施工：

1. 先做 `阶段 A0`
2. 再做 `阶段 A`
3. 然后做 `阶段 B`
4. 接着做 `阶段 C`
5. 最后持续推进 `阶段 D`

也就是说，当前最值得立刻开工的不是继续扩功能，而是先把最小本地测试模型闭环建起来，再把整个 agent 主链路冒烟彻底跑顺、跑稳、跑清楚。

---

## 10. 与现有文档的关系

- 若问题是“长期工程约束是什么”，看 `DEVELOPMENT_STANDARDS_AGENTOS.md`
- 若问题是“当前主线重构按什么顺序执行”，看 `secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md`
- 若问题是“前台为何坚持单一 Prime Agent”，看 `secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md`
- 若问题是“接下来如何把系统做成个人真正开箱即用的 Jarvis”，看本文件
