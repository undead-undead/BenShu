# BenShu Wasm 工具协议

## 定位

Wasm 工具用于承载可独立更新的第三方或用户自定义工具。它不是替代 Rust 内置工具的系统内核，而是插件化执行面：

- 主程序无需重新编译即可新增工具
- 工具以沙箱方式执行
- 工具能力通过 manifest 显式声明
- agent 只能看到 manifest 暴露的工具说明和参数 schema

## 工具包结构

推荐目录结构：

```text
data/skills/<tool_name>/
  SKILL.md
  scripts/
    <tool_name>.wasm
  references/
  templates/
```

`SKILL.md` 必须带 YAML frontmatter。最小示例：

```yaml
---
name: clean_markdown
description: Clean webpage text into Markdown.
runtime: wasm
script: clean_markdown.wasm
interface: "type Args = { input: string }"
permissions:
  filesystem: read_skill
  network: false
resources:
  timeout_secs: 5
  max_memory_mb: 64
  max_output_bytes: 1048576
wasm:
  abi: wasi-component-run-string-v1
  entrypoint: run
  sha256: null
---
```

## ABI

当前支持的 ABI：

```text
wasi-component-run-string-v1
```

当前约定：

- Wasm 文件必须放在 `scripts/`
- `script` 必须是相对路径，不能包含 `..`
- runtime 必须写 `wasm`
- 默认入口函数是 `run`
- 当前 Wasm 协议只支持本地执行，不开放网络

## 权限

权限必须显式声明。默认策略：

```yaml
permissions:
  filesystem: read_skill
  network: false
  browser: false
  env: []
  allowed_paths: []
```

当前 Wasm 工具不允许：

- `network: true`
- `browser: true`
- 逃逸 `scripts/` 的 wasm 路径

后续如果要开放更高权限，必须先补 host capability API 和审批链，不能让 wasm 直接穿透系统状态。

## 安全链路

当前 Wasm 工具会经过：

- manifest 解析与协议校验
- `SkillVerifier` 对说明文本和使用说明做安全扫描
- 可选 sha256 校验
- `PolicyGuard` pre-check
- Wasmtime + WASI 沙箱
- 内存限制
- 输出大小限制
- 执行超时
- `PolicyGuard` post-filter

## 和 Rust 内置工具的边界

Rust 内置工具继续承担高权限系统能力：

- memory / RAG
- browser / web
- delegation / A2A
- document / office / media
- model runtime / artifact 管理

Wasm 工具适合：

- 文本清洗
- 格式转换
- 小型计算
- 低权限自定义 API wrapper
- 可下载插件

一句话：

```text
Rust native tools are the trusted kernel.
Wasm tools are the sandboxed plugin surface.
```
