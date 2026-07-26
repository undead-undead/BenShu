# BenShu Windows 原生 Runtime 重启回归清单

## 目标

验证以下链路在 Windows 原生环境中真实成立：

1. 用户在面板修改 `Llama.cpp Runtime` 参数并保存
2. 参数稳定写入 `benshu.yaml`
3. 网关返回“需要重启 / 已请求重启”的真实结果
4. Windows 原生本地运行时宿主被自动重启
5. 重启后的 runtime 实际使用新的参数

同样验证：

1. 用户在面板修改 `Windows ML / ONNX Runtime` 参数并保存
2. 图像服务或其他 Windows ML 宿主被自动重启
3. 重启后图像执行参数真实生效

---

## 前提条件

### 面板/网关

- 必须在 Windows 原生环境启动 `BenShu` 面板
- 不使用 WSL 桥接作为正式验证路径
- 面板启动后，网关由内嵌模式自动拉起

### Main Brain

- 已在 agent 装备页给目标 agent 装备本地 GGUF 模型
- `benshu` agent 已有：
  - `local_model_artifact`
  - `base_url`
  - `model`
- Windows 侧可发现：
  - `restart_llama_server_vulkan.ps1`
  - `llama-server.exe`

### Windows ML / ONNX Runtime

- 已在面板全局模型绑定里选择图像模型目录
- 图像模型目录是可识别的本地 ONNX bundle，或者是后续可导出的源模型目录
- Windows 侧可发现：
  - `restart_onnx_directml_image_bridge.ps1`
  - `benshu-windows-image-service.exe`
  - 如需导出 ONNX，则还要有 Python

---

## 验证一：Llama.cpp Runtime 保存与自动重启

### 步骤

1. 打开面板 `Local Models -> Llama.cpp Runtime`
2. 修改一个容易确认的参数，例如：
   - `Context length`
   - `GPU offload layers`
   - `Eval batch size`
3. 点击 `Apply Llama.cpp Runtime`
4. 观察面板状态提示
5. 打开 `benshu.yaml` 确认字段已写入
6. 查看 Windows 侧 `llama-server` 进程是否被拉起或重启
7. 查看对应日志，确认启动参数已更新

### 预期结果

- 面板提示不再是“缺少托管重启入口”
- 至少出现这类语义：
  - 参数已保存
  - Main Brain 运行时重启请求已发出
- `benshu.yaml` 中 `llama_cpp_runtime` 为新值
- `llama-server` 有新的 PID，或重启时间刷新
- 启动参数中可看到：
  - `-c`
  - `-ngl`
  - `-b`
  - `-ub`
  - `--parallel`
  等对应值已经变化

### 建议检查位置

- `benshu.yaml`
- `%TEMP%\\benshu-llama-vulkan.out.log`
- `%TEMP%\\benshu-llama-vulkan.err.log`
- `%TEMP%\\benshu-llama-vulkan.pid`

---

## 验证二：Windows ML / ONNX Runtime 保存与自动重启

### 步骤

1. 打开面板 `Local Models -> Windows ML / ONNX Runtime`
2. 修改一个图像侧参数，例如：
   - `Image steps`
   - `Guidance`
3. 点击保存
4. 观察面板状态提示
5. 打开 `benshu.yaml` 确认 `windows_ml_runtime.image_profile` 已写入
6. 查看 Windows 图像服务进程是否被拉起或重启
7. 查看图像服务日志与环境变量生效情况

### 预期结果

- 面板提示：
  - 参数已保存
  - Windows ML 运行时重启请求已发出
- `benshu.yaml` 中 `windows_ml_runtime.image_profile.steps/guidance` 为新值
- Windows 图像服务重新拉起
- 新进程实际读取到了：
  - `BENSHU_ONNX_IMAGE_STEPS`
  - `BENSHU_ONNX_IMAGE_GUIDANCE_SCALE`

### 建议检查位置

- `benshu.yaml`
- `%TEMP%\\benshu-onnx-directml-image.out.log`
- `%TEMP%\\benshu-onnx-directml-image.err.log`
- `%TEMP%\\benshu-onnx-directml-image.pid`

---

## 验证三：保存后参数不漂

### 步骤

1. 修改参数并保存
2. 退出面板
3. 重新打开面板
4. 回到同一设置页
5. 检查控件回填值

### 预期结果

- 界面回填值与刚才保存的一致
- 不会恢复成旧值
- 不会出现某些字段写入 YAML 但面板读回错误

---

## 验证四：真实能力回归

### Main Brain

1. 在面板中打开聊天界面
2. 发一条纯文本请求
3. 再发一条带图片理解的请求
4. 确认主脑仍正常承接

### 图像能力

1. 发起一条“请生成一张图片”的真实聊天请求
2. 确认主脑最终落到 `generate_image`
3. 查看图像服务日志中本次请求是否到达

### 预期结果

- 调参数不会把主脑或图像 runtime 打坏
- 真实聊天仍能完成交付

---

## 当前代码已具备的能力

- 保存 `llama_cpp_runtime` 或 `windows_ml_runtime` 后，网关会判断是否需要重启
- 网关会在 Windows 环境尝试自动发现 runtime host 重启入口
- 图像 runtime 正式优先走 Rust Windows 服务，而不是把 Python 当正式边界
- 图像 runtime 的 `steps`、`guidance` 已接入自动重启命令

---

## 当前仍需在 Windows 原生环境确认的事项

- `llama-server.exe` 的自动发现路径在最终打包目录下是否稳定
- `benshu-windows-image-service.exe` 在最终打包目录下是否稳定可发现
- ONNX bundle 已就绪时，图像 runtime 是否完全不再依赖 Python
- 面板提示文案是否足够小白友好

---

## 判定标准

满足以下条件即可判定这条主线通过：

1. 面板保存后不只写配置，还能自动触发正式 runtime host 重启
2. 重启后的进程真实带着新参数运行
3. 面板重开后参数稳定回填
4. 聊天主线与图像主线未被回归破坏
