# BenShu Hardness 设计原则

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 状态: 次级专题摘要（2026-03-25 收敛版）
>
> 主约束来源:
>
> - `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`
> - `docs/secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md`
> - `docs/secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md`
> - `docs/secondary/BENSHU_UNIFIED_TRACING_CONTRACT.md`

---

## 1. 文档定位

这份文档现在只保留 `hardness` 的设计动机与长期原则摘要。

它不再承担：

- 最终工程规范
- 最终执行顺序
- 最终完成口径

这些内容统一回到核心文档。

---

## 2. 一句话定义

`BenShu Hardness = 认知主权 + 可验证执行 + 非破坏性默认 + 风险显式化 + 可恢复治理。`

---

## 3. 稳定原则

以下原则已经稳定，应视为 BenShu 长期成立的 hardness 核心：

### 3.1 Truth First

- 不能把未执行写成已执行
- 不能把推断写成事实
- 不能把计划写成结果

### 3.2 Verification First

- 高风险动作应尽量绑定执行证据
- fallback、budget exhaustion、治理决策要能进入 trace / witness / replay

### 3.3 Non-Destructive By Default

- 缺少充分 authority 或 confirmation 时，不默认做不可逆动作
- 管理对象是风险和失控行为，不是用户本人

### 3.4 Explicit Risk

系统应明确暴露：

- 降级
- 不确定性
- fallback reason
- replay gap
- budget exhaustion

### 3.5 Recovery First

系统不仅要会执行，还要能：

- checkpoint
- replay
- restore
- recover

---

## 4. BenShu 特有的 Hardness 约束

与通用宿主平台不同，BenShu 作为长期运行的个人主代理系统，还必须额外保持：

- 主代理所有权
  - task ownership
  - memory ownership
  - approval ownership
  - final responsibility
- 治理继承
  - spawn / delegation / transport / restore 过程中的治理上下文不能靠隐式默认值漂移
- 生命周期硬度
  - task / trace / witness / artifact / memory 都要有正式生命周期
- 记忆主权
  - 不允许把“本体记忆导出”当成默认能力
  - backup 默认应是 sealed + restore-only
- 协议硬度
  - delegation 必须带 ownership、causality、return-mode

---

## 5. 不应照抄的部分

外部高可信代理平台的思路可以借鉴，但以下内容不应直接复制：

- 宿主平台式绝对指令层级
- 过强保守性
- 把平台安全边界直接投射为产品边界

BenShu 的产品语义应始终坚持：

- 用户安全第一
- 用户利益优先
- 系统稳定是为了更好服务用户
- 不把用户当作被管控对象

---

## 6. 已进入主文档的内容

以下内容已被核心文档吸收：

- 用户保护优先于用户管控
  - 见 `DEVELOPMENT_STANDARDS_AGENTOS.md`
- prime-agent ownership
  - 见 `secondary/BENSHU_PRIME_AGENT_ARCHITECTURE.md`
- trace / replay / witness / scorecard 投影链
  - 见 `BENSHU_UNIFIED_TRACING_CONTRACT.md`
- hardness 在各阶段的执行要求
  - 见 `secondary/BENSHU_AGENTOS_EXECUTION_PLAN_ZH.md`

---

## 7. 后续如果继续深化

若未来还要继续扩展 hardness 专题，建议只围绕下面几条真正还值得深化的方向展开：

- authority / budget / recovery 三轴治理细化
- 更完整的 delegation inheritance
- 长周期 recovery / audit / replay 策略
- 面向个人用户的自保护语义，而不是平台式惩罚控制

---

## 8. 一句话结论

`hardness` 现在应被视为 BenShu 的跨系统产品原则，而不是单独一份不断膨胀的平行规范；长期有效约束已回收到核心文档，本文件只保留设计立场与解释性摘要。
