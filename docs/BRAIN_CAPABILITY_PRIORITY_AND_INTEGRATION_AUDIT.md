# Brain Capability Priority And Integration Audit

> Platform Positioning: `Windows Native` is BenShu's formal product path and primary host platform; `WSL / WSL2 / Linux` routes are development/testing lanes for fast iteration and must not be presented as the default product deployment path.

> 状态: 次级审计摘要（2026-03-25 收敛版）
>
> 主约束来源:
>
> - `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`
> - `docs/secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md`
> - `docs/secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md`
> - `docs/secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md`

---

## 1. 文档定位

这份文档现在只保留 `brain` 专题审计的长期结论。

它不再承担：

- `brain` 的最终规范定义
- 全局执行计划
- 主路径完成口径

这些口径统一回到核心文档。

---

## 2. 稳定排序结论

如果以主代理 runtime 的产品级优先级来看，稳定结论仍然是：

### 必做

- 主代理 ownership / delegation contract
- 统一执行状态机
- 真正可用的 preemptive interruption
- 完整多模态输入链路

### 应做

- `CommClient / swarm runtime` profile 化

### 可做

- autopilot / prewarming

### 选做

- hyperbolic navigation

---

## 3. 当前已稳定进入主路径的判断

以下判断现在已经不应继续当作“待论证观点”，而应视为现阶段已成立的主线路径现实：

### 3.1 Ownership / Delegation

- `BenShu` 是唯一前台主代理
- specialist 通过 `A2A` 在后台协作
- delegation 已开始带 ownership / return-mode / causality

### 3.2 Runtime State / Interruption

- 前台任务已具备真实 task/trace 主路径
- interrupt / cancel / interject 已接入主路径
- 会话级 stop 与自然语言控制已开始收束

### 3.3 Multimodal Main Path

- 图片、PDF、音频、视频附件已通过共享入口进入主路径
- 后续重点不再是“有没有主线路由”，而是 provider-native 抽象继续增强

### 3.4 Comm / A2A

- runtime profile 已显式建模
- receive-side owner rollup / inbox / processed events 已接通
- `comm` 现在更适合作为后台 specialist 协作底座来理解，而不是用户侧主入口

---

## 4. 仍值得继续深化的方向

如果未来继续做 `brain` 专题深化，建议只围绕以下还真正值得单独推进的东西：

- provider-native 无差别多模态抽象继续增强
- 更完整的 witness / governance / replay 控制台
- 更深的 distributed `comm` 生产闭环
- 更清晰的 autopilot / prewarming profile 边界

---

## 5. 不再建议在本文件继续维护的内容

- 各 crate 的实时完成度长表
- 与执行计划重复的阶段拆解
- 与 prime-agent 架构重复的 ownership 口径
- 与 tracing 契约重复的 trace / witness / replay 定义

---

## 6. 一句话结论

这份文档现在应被理解为 `brain` 的阶段性审计摘要，而不是长期核心规范；长期有效结论已收敛进 `标准 + 执行计划 + prime architecture + tracing contract`，本文件只保留优先级判断和后续深化方向。
