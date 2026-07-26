# BenShu 内置 llama.cpp Runtime 打包说明

## 目标

Windows 推荐安装包应自带可用的 `llama-server.exe` 运行时，让用户在面板里选择 GGUF / Qwen 模型后可以直接加载，不需要先手动下载或配置 llama.cpp。

Lite 安装包可以不带 runtime，以减小体积；这种情况下用户仍可通过环境变量或自定义路径提供自己的 llama.cpp。

## 随包目录

打包脚本会准备：

```text
runtimes/
  llama.cpp/
    b9592/
      llama-server.exe
      llama-server-impl.dll
      llama.dll
      ggml*.dll
      ...
```

安装后对应：

```text
{app}/runtimes/llama.cpp/b9592/llama-server.exe
```

gateway 会从安装目录向上查找 `runtimes/llama.cpp/bNNNN/llama-server.exe`，并用 `llama-server.exe --version` 校验最低 build。

## 最低版本

当前最低 build 是 `b9592`。原因是近期 Qwen / Gemma 等 GGUF 模型需要较新的 llama.cpp 运行时，否则可能出现：

- 模型加载失败；
- tokenizer / chat template 不兼容；
- runtime 崩溃或输出异常；
- 面板显示模型可选，但实际启动失败。

版本门在两处生效：

- gateway 诊断和自动 runtime 发现；
- Windows 启动脚本 `start_llama_server_vulkan.ps1` / `restart_llama_server_vulkan.ps1`。

## 准备方式

打包前运行：

```powershell
.\scripts\windows\provision_llama_cpp_runtime.ps1
```

默认行为：

1. 优先复用本机已有的 `BENSHU_WINDOWS_LLAMA_CPP_DIR` / `LLAMA_CPP_DIR`。
2. 再查找 `D:\llama.cpp\b9592`、`C:\llama.cpp\b9592` 等常见路径。
3. 都没有时，从 llama.cpp release 下载 `llama-b9592-bin-win-vulkan-x64.zip`。
4. 解压/复制到 `runtimes\llama.cpp\b9592`。
5. 执行 `llama-server.exe --version`，确认 build 满足要求。

也可以显式指定：

```powershell
.\scripts\windows\provision_llama_cpp_runtime.ps1 `
  -Build 9592 `
  -SourceDir "D:\llama.cpp\b9592"
```

或：

```powershell
.\scripts\windows\provision_llama_cpp_runtime.ps1 `
  -Build 9592 `
  -ArchivePath "D:\downloads\llama-b9592-bin-win-vulkan-x64.zip"
```

## 安装包集成

`build_windows_setup.ps1` 已接入 runtime provisioning。

`benshu_setup.iss` 已新增 `runtime` 组件：

- Recommended：默认包含内置 llama.cpp runtime。
- Lite：不包含 runtime，用户需要自行提供或之后在面板配置。
- Custom：用户可选择是否安装 runtime。

同时安装包会带上：

```text
scripts/windows/start_llama_server_vulkan.ps1
scripts/windows/restart_llama_server_vulkan.ps1
scripts/windows/stop_llama_server_vulkan.ps1
```

这些脚本是面板保存本地模型配置后自动启动/重启 BenShu 托管 llama.cpp runtime 的控制入口。

## 不随 git 提交二进制

`runtimes/llama.cpp/` 被 `.gitignore` 忽略。二进制 runtime 由打包脚本准备，不进入源码仓库。

这样可以避免仓库膨胀，也避免频繁更新 llama.cpp 时产生大体积提交。
