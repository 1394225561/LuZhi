# LuZhi 项目交接文档

> 最后更新：2026-05-24 | 修复 BUG-001（透明窗口）与 BUG-002（窗口拖拽），程序化拖拽方案落地
> 更新本文件时，**必须**保持“项目概述 → 完整开发计划 → 工作任务记录（按**时间倒序**，并且只保留最近的 3 次记录） → 冬眠记录（按**时间倒序**，并且只保留最近的 3 次记录）”的结构顺序。

## 项目概述

LuZhi 是中国版 Screen Studio，核心聚焦“录屏 + AI 自动美化 + 一键导出”。MVP 坚持极致单点，不做字幕、摘要、知识库、模板系统、团队协作和平台发布 API。

当前技术栈约束：

- 桌面框架：Tauri 2.0
- 前端：React + TypeScript + Tailwind + shadcn/ui
- Rust 底层：ScreenCaptureKit、DXGI、WASAPI、cpal、FFmpeg binding
- 数据流红线：音视频帧流不得经过前端 JS 层

---

## 完整开发计划

已落地主架构与计划文档：

- `docs/architecture/project-architecture-and-overall-planning.md`

关键决策：

1. MVP 采用“macOS 先打穿的分层流水线架构”。
2. 4K 作为架构预留能力，不作为首版唯一硬验收；MVP 主验收为 1080p 稳定录制、录后美化和导出。
3. Windows 通过同一组 Rust Trait 在 W3-W8 逐步追平，不单独开业务管线。
4. 光标美化和空白裁剪在 MVP 阶段以录后处理为主，避免阻塞捕获线程。
5. 授权系统只做本地试用状态和激活状态接口，不展开服务端激活协议。

W1-W12 Phase：

- Phase 1 / W1-W2：脚手架与 macOS 录制闭环
- Phase 2 / W3-W4：Windows 捕获预研与双音频链路
- Phase 3 / W5-W6：录制 UI 与前后端联调
- Phase 4 / W7-W8：光标平滑与点击放大
- Phase 5 / W9-W10：空白段检测与自动裁剪
- Phase 6 / W11-W12：导出预设与本地授权

### 系统架构设计

输入文件：

- `reference/tasks/architecture-task.md`
- `docs/PRD/LuZhi_PRD_final_version.md`
- `.codex/rules/0-global.md`
- `.codex/rules/1-coding-style.md`
- `.codex/rules/2-testing.md`
- `.codex/rules/3-git-commit.md`
- `.codex/rules/4-security.md`
- `.codex/rules/5-docs.md`
- `BUG.md`

已完成：

- 明确进程与线程模型。
- 明确 React / Rust App Logic / Native Modules 三层边界。
- 明确跨平台 Trait 抽象：`ScreenCapture`、`AudioCapture`、`CursorProcessor`、`SilenceDetector`、`MediaEncoder`。
- 明确核心数据流、Ring Buffer、时间戳对齐、音频混音和 FFmpeg 封装原则。
- 明确光标平滑、点击放大、空白段检测和 4K 降级策略。
- 明确 W1-W12 MVP 执行计划、风险点和前后端联调节奏。
- 为每个 Phase 生成自测清单到 `tests/` 目录。

后续入口：

1. 人工审阅 `docs/architecture/project-architecture-and-overall-planning.md`。
2. 确认无调整后，进入详细实施计划编写。
3. 启动 W1-W2 时，先按 `tests/phase-1-w1-w2-checklist.md` 建立验收闭环。

---

## 工作任务记录

### 2026-05-24：BUG-001 & BUG-002 深度修复

输入文件：

- `BUG.md`（原记录 + 补充更新）
- `src-tauri/Cargo.toml`（添加 macos-private-api）
- `src-tauri/tauri.conf.json`（启用 macOSPrivateApi + acceptFirstMouse）
- `src-tauri/capabilities/default.json`（添加拖拽权限）
- `src/App.tsx`（程序化拖拽 handler + 拖拽区域标记）
- `src/components/`（全部 5 个视图组件的拖拽区域配置）
- `src/styles.css`（html 透明 + body 去背景）
- `.cargo/registry/.../tauri-2.11.2/src/window/scripts/drag.js`（Tauri 源码审查）

已完成：

- BUG-001 修复（4 轮迭代）：
  - CSS 层：`styles.css` body 移除 `bg-background`，html 添加 `background-color: transparent`
  - WKWebView 层：`Cargo.toml` 启用 `macos-private-api` feature，`tauri.conf.json` 启用 `macOSPrivateApi: true`
  - 视觉伪影：移除 RecordingPanel/StatusBar 的 `shadow-2xl shadow-black/30`、`backdrop-blur-xl`，移除 idle 装饰渐变 div
- BUG-002 修复（4 轮迭代）：
  - ACL 权限：`capabilities/default.json` 添加 `core:window:allow-start-dragging`
  - macOS 首次点击：`tauri.conf.json` 添加 `acceptFirstMouse: true`
  - 拖拽区域：全部 5 个视图（idle/recording/preview/processing/error）添加 `data-tauri-drag-region="deep"`
  - 程序化拖拽：`App.tsx` 自定义 `mousedown` handler，调用 `getCurrentWindow().startDragging()`，绕过 drag.js 的 macOS 焦点窗口限制
  - 交互元素排除：handler 用 `closest('button,input,select,textarea,a,[role=...],[contenteditable],[tabindex]')` 精确排除
- BUG-003 补充修复：`recording-status-bar.tsx` pause/stop 按钮移除 `motion.div whileTap` 包裹
- `BUG.md` 更新：BUG-001/002 完整根因分析 + 预防规则，新增延期-001（点击穿透）
- Tauri 源码审查：drag.js 注入机制、`isDragRegion` 逻辑、macOS `performWindowDragWithEvent:` 调用链

验证结果：

- `npm run build` 通过
- `npm run test` 通过（4/4）
- `cargo build` 通过（clean）

关键决策：

- **放弃声明式 `data-tauri-drag-region`，采用程序化 `startDragging()` API**：声明式机制在 macOS 上受焦点窗口限制（tauri#11605），对浮动面板 app 不可靠
- **不检查 `{false}` 值**：交互元素选择器已足够，`{false}` 排除反会阻止面板内容区拖拽
- **动态 `import('@tauri-apps/api/window')`**：避免测试环境顶层导入报错

后续入口：

1. `npm run tauri dev` 验证所有视图拖拽和透明效果
2. 进入 Phase 3 (W5-W6) 前后端联调

---

### 2026-05-24：UI 移植（v0 → Tauri）

输入文件：

- `reference/ui/ui_spec.md`
- `docs/architecture/project-architecture-and-overall-planning.md`
- `_v0_reference/`（Next.js 设计稿）

已完成：

- 依赖安装：class-variance-authority, clsx, tailwind-merge, lucide-react, framer-motion, @radix-ui/react-switch/slider/select/slot
- shadcn/ui 配置：components.json, 路径别名 `@/*` → `src/*`
- Raycast 主题：CSS 自定义属性 + Tailwind v4 `@theme inline` 映射
- shadcn/ui 组件：Button, Switch, Slider, Select
- 业务组件：RecordingPanel, RecordingStatusBar, PreviewView, ProcessingView, ErrorView
- Tauri invoke 封装：完整 Commands/Events 类型定义和函数
- App.tsx 五态路由：idle/recording/preview/processing/failed
- 前端测试：4 个测试覆盖核心交互
- BUG 修复：3 个（透明窗口、拖拽区域、按钮点击）
- 移植规范文档：`reference/ui/ui-migration-spec.md`

验证结果：

- `npm run build` 通过（JS 464 KB, CSS 115 KB）
- `npm run test` 通过（4/4）
- `cargo test` 通过（14/14）

后续入口：

1. `npm run tauri dev` 验证桌面端效果
2. 将 console.log 占位替换为真实 Tauri invoke
3. 进入 Phase 3 (W5-W6) 前后端联调

---

### 2026-05-23：Phase 1 实施计划

已生成实施计划：

- `docs/superpowers/plans/2026-05-23-phase-1-macos-recording-foundation.md`

执行范围：

- 仅覆盖 Phase 1 / W1-W2：脚手架与 macOS 录制闭环。
- 不覆盖 Windows、音频混音、光标美化、空白裁剪、导出预设和授权系统。

执行要求：

1. 按任务顺序执行，每个任务完成后提交一次。
2. 执行前确认 `src-tauri/Cargo.toml` 依赖版本经过人工审查。
3. ScreenCaptureKit 原生回调、线程切换、buffer 生命周期和 unsafe/FFI 代码必须人工逐行审查。
4. Phase 1 完成后逐项执行 `tests/phase-1-w1-w2-checklist.md`。

---

## 冬眠记录

### 2026-05-24：BUG-001 & BUG-002 修复完成冬眠

#### 1. 当前任务上下文

修复 Tauri 2.0 浮动面板窗口的 BUG-001（800x600 不透明背景）和 BUG-002（窗口无法拖动）。当前在 `feat/architecture-planning` 分支，最新提交 `ecbdb69`。

#### 2. 已完成进度

- BUG-001 三层透明架构修复（NSWindow + WKWebView + CSS）
- BUG-002 程序化拖拽方案（自定义 mousedown handler + `getCurrentWindow().startDragging()`）
- 5 个视图全部支持拖拽（idle/recording/preview/processing/error）
- 交互元素正确排除（按钮、滑块、开关等不触发拖拽）
- CSS 阴影伪影清除
- BUG.md 预防规则完整更新
- 全部验证通过：`npm run build`、`npm run test` 4/4、`cargo build`

#### 3. 中断时的处置决策

休眠触发时，用户已验证拖拽功能正常，所有修改处于稳定完成态。选择"快速收尾"：提交现场并更新 HANDOFF.md。

#### 4. 架构与关键决策

- **程序化拖拽 > 声明式拖拽**：Tauri 2 的 `data-tauri-drag-region` 在 macOS 上受焦点窗口限制，不可靠。自定义 mousedown handler + `startDragging()` API 是更健壮的方案
- **交互元素白名单 > `{false}` 排除**：用 `closest('button,input,select,...')` 精确排除交互元素，不做区域级 `{false}` 排除
- **三层透明**：NSWindow (`transparent: true`) + WKWebView (`macos-private-api`) + CSS (html/body transparent) 缺一不可
- **ACL 权限是硬门控**：`core:window:allow-start-dragging` 不在 `core:default` 内，必须显式授予

#### 5. 立即执行清单

1. `npm run tauri dev` 最终验证桌面端效果
2. 将 console.log 占位替换为真实 Tauri invoke
3. 录制计时器和麦克风音量替换为 Tauri event 监听
4. 进入 Phase 3 (W5-W6) 前后端联调

#### 6. 当前报错/阻碍

当前无阻塞报错。

已知待办事项：
- 延期-001：透明区域鼠标点击不穿透（需原生 macOS 代码，记录在 BUG.md）
- PreviewView 和 App.tsx 中有 console.log 占位
- 录制计时器和麦克风音量需替换为 Tauri event 监听
- 权限检测需连接真实系统权限 API

---

### 2026-05-24：UI 移植完成冬眠

#### 1. 当前任务上下文

完成 `_v0_reference/` UI 设计稿到 Tauri 2.0 项目的完整移植。当前在 `feat/architecture-planning` 分支，最新提交 `7f4ee1a`。

#### 2. 已完成进度

- 10 个 Task 全部完成（依赖→主题→组件→封装→业务组件→路由→测试）
- 修复 3 个 BUG（透明窗口背景、拖拽区域缺失、按钮点击拦截）
- 创建 `reference/ui/ui-migration-spec.md` 移植规范文档（389 行）
- 编译通过，4 个前端测试通过

#### 3. 中断时的处置决策

休眠触发时，工作处于稳定完成态：

- 所有 UI 组件已创建并验证
- 最后一个操作是创建移植规范文档并更新索引
- 无进行中的代码修改，无需回滚
- 选择"快速收尾"：提交现场并生成交接文档

#### 4. 架构与关键决策

- 主题选择 Raycast 近黑中性色（`#040506` canvas），非 v0 原始紫色系
- 窗口配置 `decorations: false` + `transparent: true` 实现无边框透明
- `motion.div` 的 `whileTap` 不能作为可交互元素的父容器（会拦截点击）
- 拖拽区域需要 `data-tauri-drag-region`，可交互元素需要 `{false}` 排除
- `<html>` 必须添加 `class="dark"` 才能激活 CSS 主题变量

#### 5. 立即执行清单

1. 运行 `npm run tauri dev` 验证桌面端效果
2. 检查浮动面板是否正确显示（无背景方框）
3. 检查窗口拖拽和按钮点击是否正常
4. 如有问题，参考 `reference/ui/ui-migration-spec.md` 第 7 章排查

#### 6. 当前报错/阻碍

当前无阻塞报错。

已知待办事项：

- PreviewView 和 App.tsx 中有 console.log 占位，需替换为真实 Tauri invoke
- 录制计时器和麦克风音量需替换为 Tauri event 监听
- 权限检测需连接真实系统权限 API

---

### 2026-05-24：Phase 1 worktree 合并与清理

- 将 `feat/phase-1-macos-recording` 合并到 `feat/architecture-planning`。
- 清理 `.worktrees/phase-1-macos-recording` worktree。
- Phase 1 Task 1-10 全部完成，14 Rust tests + 2 frontend tests 通过。
- 下一步：人工审查 ScreenCaptureKit 边界后激活真实实现，进入 Phase 2。
