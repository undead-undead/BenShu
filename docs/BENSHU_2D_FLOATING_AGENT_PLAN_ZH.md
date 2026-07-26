# BenShu 2D 悬浮代理技术方案

> 平台口径: `Windows 原生` 是 BenShu 的正式产品主线与主要承载平台；`WSL / WSL2 / Linux` 路径仅用于开发测试、快速联调与验证，不应被表述为默认产品部署路径。

> 关联核心文档: `docs/DEVELOPMENT_STANDARDS_AGENTOS.md`
>
> 关联背景信息窗主线: `docs/secondary/BENSHU_BACKGROUND_COMPRESSION_MAINLINE_PLAN_ZH.md`
>
> 文档定位: 本文档用于定义 `Windows + egui + windows-rs` 路线下的 `2D 悬浮代理` 技术方案。它回答“在不引入 Tauri、不切换主面板栈的前提下，如何把 BenShu 现有面板最小化为一个可交互、可说话、可持续存在的 2D 桌面代理”。

---

## 0. 目标

本方案的目标不是直接实现高复杂度 Live2D 或完整 3D 数字人，而是先构建一条与当前 BenShu 技术栈一致、可快速落地、可持续升级的前台代理路线：

- 保持现有 `egui / eframe` 面板主栈不变
- 使用 `windows-rs` 补 Windows 原生悬浮窗能力
- 先实现一个轻量 `2D 悬浮代理`
- 让该代理具备：
  - 可显示
  - 可说话
  - 可切换表情/状态
  - 可响应基础交互
  - 可和现有 `Agent / Memory / Background Window Compression` 主线联动

一句话说：

`这不是一个新桌面壳项目，而是 BenShu 现有前台的一种“最小化形态”。`

---

## 1. 为什么先做 2D

相对 3D 数字人，2D 悬浮代理更适合当前阶段：

- 资产成本低
- 开发速度快
- 与 `egui` 兼容度高
- 更容易做成稳定的桌面前台
- 更适合先验证“面板最小化 -> 悬浮代理”这条产品路径

当前阶段的重点不是“做到大型游戏角色质量”，而是：

- 把 Agent 变成桌面上持续存在的实体
- 让它有状态反馈
- 让它有语音出口
- 让它和现有后台能力联动

---

## 2. 总体架构

### 2.1 角色分工

建议采用如下分层：

- `Panel (egui)`
  - 完整控制台
  - 设置、trace、记忆、调试、任务详情

- `Floating Agent Shell (egui + windows-rs)`
  - 面板最小化后的悬浮窗壳
  - 负责桌面显示、点击交互、状态提示

- `2D Avatar Renderer (egui texture-based)`
  - 负责 2D 角色贴图显示
  - 负责表情状态机、嘴型状态机、轻量动画

- `Voice Loop`
  - 负责 TTS 播放、可选 STT 接入、播放状态通知

- `Brain Mainline`
  - 负责 Agent 推理、背景信息窗压缩、记忆、任务执行、工具调用

### 2.2 技术边界

这条路线不引入：

- `Tauri`
- `WebView` 角色前台
- 第三套桌面 UI 主栈

这条路线优先复用：

- 现有 `egui / eframe`
- 现有 `windows-rs`
- 现有 `Agent runtime`
- 现有 `Background Window Compression`

---

## 3. 最小功能定义

### 3.1 MVP 必须具备

第一版 2D 悬浮代理至少要有：

- 悬浮窗显示
- 透明背景
- 置顶
- 可拖动
- 最小交互点击区
- 角色立绘显示
- 角色状态切换
- 语音播报时嘴型切换
- 基础表情状态切换
- 与当前 Agent 状态联动

### 3.2 MVP 暂不追求

第一版不追求：

- Live2D 级别参数化变形
- 骨骼 2D 动画系统
- 完整换装系统
- 复杂小游戏式交互
- 多角色系统

---

## 4. 窗口层方案

### 4.1 `windows-rs` 负责的内容

`windows-rs` 主要负责补全 Windows 原生桌宠/悬浮窗能力：

- `WS_EX_LAYERED`
  - 透明窗口
- `WS_EX_TOPMOST`
  - 始终置顶
- `WS_EX_TOOLWINDOW`
  - 不抢主任务栏表现
- 可选 `WS_EX_TRANSPARENT`
  - 鼠标点击穿透（仅在某些状态启用）
- 窗口圆角、阴影、DWM 细节
- 托盘图标
- 最小化 / 恢复 / 关闭行为钩子

### 4.2 `egui` 负责的内容

`egui` 继续负责：

- 角色贴图绘制
- 状态气泡
- 按钮 / 设置小面板
- 调试开关
- 角色交互热点

### 4.3 建议的前台模式

建议做三种窗口模式：

1. `Panel Mode`
  - 正常完整面板

2. `Floating Agent Mode`
  - 悬浮代理
  - 常驻桌面

3. `Collapsed Silent Mode`
  - 更小状态点 / 托盘
  - 只保留语音或通知反馈

---

## 5. 2D 角色渲染方案

### 5.1 第一版渲染方式

第一版建议直接采用：

`texture-based layered avatar`

即：

- 基础身体图
- 眼睛状态图
- 嘴型状态图
- 可选表情覆盖层
- 可选状态特效层

这些都用普通贴图完成，不引入重型动画引擎。

### 5.2 资源层结构建议

建议最小资源结构如下：

```text
assets/avatar/
  base/
    idle_body.png
  eyes/
    open.png
    half.png
    closed.png
  mouth/
    neutral.png
    a.png
    i.png
    u.png
    smile.png
  emotion/
    happy.png
    thinking.png
    alert.png
    sleepy.png
  fx/
    glow_idle.png
    voice_ring.png
```

### 5.3 动画方式

第一版动画不做复杂骨骼，而采用：

- 定时切换贴图
- 位置轻微偏移
- 缩放轻微呼吸
- 透明度/颜色渐变

这样就足够实现：

- 眨眼
- 待机呼吸感
- 说话嘴型切换
- 提醒状态

---

## 6. 状态机设计

### 6.1 顶层状态

建议角色状态机至少分为：

- `Idle`
- `Listening`
- `Thinking`
- `Speaking`
- `Alert`
- `Sleeping`
- `Error`

### 6.2 表情层

表情层可与顶层状态叠加：

- `Neutral`
- `Happy`
- `Concerned`
- `Focused`
- `Curious`
- `Tired`

### 6.3 嘴型层

嘴型状态单独管理：

- `Closed`
- `SmallOpen`
- `WideOpen`
- `Smile`

第一版不追求真音素嘴型，只做：

- 静音
- 轻开口
- 大开口
- 微笑说话

即可。

---

## 7. 与现有 Agent 主线的联动

### 7.1 可直接复用的现有能力

当前 BenShu 已经有这些可直接复用能力：

- `Foreground Runtime`
  - 当前任务状态
  - 正在回复 / 正在思考 / 正在工具执行

- `Background Window Compression`
  - 当前主任务背景
  - workspace focus
  - relationship/persona/session layer

- `Memory`
  - 长期偏好
  - 当前 session 持续性

- `Telemetry / Trace`
  - 当前阶段状态
  - 错误态
  - 中断/恢复事件

### 7.2 悬浮代理建议监听的最小事件

建议悬浮代理先订阅这些事件：

- `foreground_thinking_started`
- `foreground_tool_execution_started`
- `foreground_response_started`
- `foreground_response_finished`
- `foreground_interrupted`
- `background_refresh_applied`
- `memory_recovered`
- `error_state_raised`

### 7.3 状态映射建议

建议先做简单映射：

- 正在思考 -> `Thinking`
- 正在语音播报 -> `Speaking`
- 收到唤醒词 / 用户输入 -> `Listening`
- 有高优先级提醒 -> `Alert`
- 长时间空闲 -> `Idle` / `Sleeping`

---

## 8. 语音联动

### 8.1 第一版建议

第一版先做：

- `TTS only`
- 悬浮代理跟随 TTS 状态做嘴型和说话状态切换

原因：

- 先让角色“会说”
- 比先让它“会听”更容易快速形成存在感

### 8.2 第二版再补

第二版可以补：

- `STT`
- 按键说话
- 热词唤醒
- 悬浮窗状态提示

---

## 9. 资源与性能预算

### 9.1 第一版资源规模

若采用贴图分层方案，第一版大致可控制在：

- `10MB ~ 150MB`

取决于：

- 贴图分辨率
- 表情数量
- 是否包含音频资源
- 是否包含多套皮肤

### 9.2 运行时性能预期

2D 贴图方案对桌面常驻更友好：

- GPU 占用相对低
- 内存占用可控
- 比 3D 数字人更适合作为常驻窗口

第一版目标不是极致画质，而是：

- 稳
- 轻
- 可持续常驻

---

## 10. 开发阶段建议

### Phase A：窗口壳

- `windows-rs` 补透明、置顶、拖动、托盘
- `egui` 内做最小悬浮窗容器

### Phase B：角色显示

- 接入基础立绘
- 加眼睛和嘴型分层
- 实现待机/说话状态切换

### Phase C：状态联动

- 读取 Agent 前台状态
- Thinking / Speaking / Idle 联动

### Phase D：语音联动

- TTS 播报时驱动嘴型和表情

### Phase E：产品化增强

- 悬浮小菜单
- 右键设置
- 前台/托盘切换
- 多主题皮肤

---

## 11. 当前最推荐路线

如果按当前 BenShu 的代码现实与开发效率综合判断，推荐路线是：

1. 继续使用 `egui` 作为前台壳
2. 用 `windows-rs` 补原生悬浮窗能力
3. 第一版数字人坚持做 `2D texture-based avatar`
4. 优先接 `TTS`，先形成“会说话的悬浮代理”
5. 与现有 `Background Window Compression` 和 `Foreground Runtime` 做状态联动

不要在第一版直接上：

- 复杂 Live2D
- 完整 3D
- 第二套前端栈

---

## 12. 一句话结论

`BenShu 的 2D 悬浮代理最合理的落地方式，是继续沿用 egui 做前台壳，用 windows-rs 补 Windows 原生悬浮窗能力，再用贴图分层 + 轻量状态机先做出“会说话、会切状态、能常驻桌面”的 2D Agent。`
