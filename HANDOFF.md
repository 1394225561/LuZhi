# LuZhi 项目交接文档

> 最后更新：2026-05-23 | 完成系统架构设计与 MVP W1-W12 执行计划落地

## 项目概述

LuZhi 是中国版 Screen Studio，核心聚焦“录屏 + AI 自动美化 + 一键导出”。MVP 坚持极致单点，不做字幕、摘要、知识库、模板系统、团队协作和平台发布 API。

当前技术栈约束：

- 桌面框架：Tauri 2.0
- 前端：React + TypeScript + Tailwind + shadcn/ui
- Rust 底层：ScreenCaptureKit、DXGI、WASAPI、cpal、FFmpeg binding
- 数据流红线：音视频帧流不得经过前端 JS 层

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

## 工作任务记录

### 2026-05-23：系统架构设计

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

### 2026-05-24：Phase 1 worktree 合并与清理

- 将 `feat/phase-1-macos-recording` 合并到 `feat/architecture-planning`。
- 清理 `.worktrees/phase-1-macos-recording` worktree。
- Phase 1 Task 1-10 全部完成，14 Rust tests + 2 frontend tests 通过。
- 下一步：人工审查 ScreenCaptureKit 边界后激活真实实现，进入 Phase 2。

### 2026-05-24：Phase 1 Task 4-10 完成交接

#### 1. 当前任务上下文

正在执行 `docs/superpowers/plans/2026-05-23-phase-1-macos-recording-foundation.md` 的 Phase 1 / W1-W2：脚手架与 macOS 录制闭环。当前位于隔离 worktree：

- 路径：`/Users/root-mac/workspace_github/LuZhi/.worktrees/phase-1-macos-recording`
- 分支：`feat/phase-1-macos-recording`
- 最新实现提交：`d37275d test(ui): 覆盖录制入口界面`

#### 2. 已完成进度

- Task 4：录制状态机 `state_machine.rs`，4 个测试通过。
- Task 5：录制服务 `recording_service.rs`，MockScreenCapture 编排，2 个测试通过。
- Task 6：权限检测边界 `permission_service.rs` + macOS 平台桩，1 个测试通过。
- Task 7：macOS ScreenCaptureKit 边界 `screen_capture_kit.rs`，start() 返回 NativeCaptureUnavailable 等待人工审查，2 个测试通过。
- Task 8：Tauri commands + events，`recording_status` / `recording_permissions` 命令注册，2 个测试通过。
- Task 9：中文录制 UI，`src/lib/tauri.ts` + `App.tsx` 替换，`npm run build` 通过。
- Task 10：前端测试，vitest + testing-library，2 个测试通过。
- 全量验证通过：14 Rust tests, cargo fmt, cargo clippy, 2 frontend tests, npm run build。

#### 3. 中断时的处置决策

休眠触发时，Task 11（Phase 1 验收与交接）正在进行中：

- 全量 Rust 测试 14 个通过。
- cargo fmt 和 clippy 无警告。
- 前端 2 个测试通过，构建通过。
- 需要提交格式修复和 HANDOFF.md 更新。

#### 4. 架构与关键决策

- `MacScreenCapture::start()` 仍返回 `NativeCaptureUnavailable`，真实 ScreenCaptureKit 集成需要人工审查后单独激活。
- `Cargo.toml` 核心依赖版本未被 AI 修改。
- 前端 UI 使用 Tailwind 深色主题，状态通过 Tauri invoke 获取。
- `src-tauri/src/app/mod.rs` 已导出全部 5 个子模块：error, events, permission_service, recording_service, state_machine。

#### 5. 立即执行清单

1. 恢复后进入 worktree：
   ```bash
   cd /Users/root-mac/workspace_github/LuZhi/.worktrees/phase-1-macos-recording
   ```
2. 提交格式修复：
   ```bash
   git add -A && git commit -m "chore(core): 修复cargo fmt格式问题"
   ```
3. 更新 HANDOFF.md 并提交。
4. 执行 `tests/phase-1-w1-w2-checklist.md` 验收清单逐项检查。
5. 人工审查 Task 7 的 ScreenCaptureKit 边界，确认后激活真实实现。

#### 6. 当前报错/阻碍

当前无阻塞报错。

已知待办事项：

- `cargo fmt` 已修复格式但尚未提交。
- `AppError` 的 4 个 `Display` 分支可补表驱动测试（非阻塞）。
- Task 7 的 ScreenCaptureKit 真实实现需人工审查激活。

---

### 2026-05-24：Phase 1 core types 冬眠交接

#### 1. 当前任务上下文

正在执行 `docs/superpowers/plans/2026-05-23-phase-1-macos-recording-foundation.md` 的 Phase 1 / W1-W2：脚手架与 macOS 录制闭环。当前仍位于隔离 worktree：

- 路径：`/Users/root-mac/workspace_github/LuZhi/.worktrees/phase-1-macos-recording`
- 分支：`feat/phase-1-macos-recording`
- 最新实现提交：`5225dd7 feat(core): 定义录制核心数据结构`
- 当前执行方式：按 `subagent-driven-development` 流程，每个任务执行后做 spec compliance review 与 code quality review。

#### 2. 已完成进度

- Rust 工具链已恢复可用：
  - `cargo 1.95.0 (f2d3ce0bd 2026-03-21)`
  - `rustc 1.95.0 (59807616e 2026-04-14)`
- Phase 1 Task 1 已完成并通过双审：
  - 补交 `src-tauri/Cargo.lock`。
  - 中文化 scaffold 可见文案与应用元信息。
  - 删除独立 `src/App.css`，只保留全局 `src/styles.css`。
  - 按新增 `DESIGN.md` 约束，将临时 UI 调整为近黑 Raycast 风格。
  - 验证通过：`npm run build`、`cargo test --manifest-path src-tauri/Cargo.toml`。
- Phase 1 Task 2 已完成并通过双审：
  - 新增 `src-tauri/src/core/frame.rs`、`config.rs`、`capture.rs`、`mod.rs`。
  - 定义 `MediaTimestamp`、`VideoFrame`、`VideoFrameRef`、`CaptureConfig`、`CaptureCapabilities`、`ScreenCapture`。
  - `ScreenCapture` trait 已补充 sink 线程交接、启动失败、stop 释放资源等公共接口注释。
  - `lib.rs` 改为返回 `tauri::Result<()>`，移除 scaffold `expect`。
  - `main.rs` 使用中文启动失败输出。
  - 验证通过：`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`cargo test --manifest-path src-tauri/Cargo.toml core::`。
- Phase 1 Task 3 已提前随 Task 2 完成并通过双审：
  - 原因：Task 2 的 `capture.rs` 需要 `crate::app::error::AppResult`，计划顺序存在编译依赖缺口。
  - 新增 `src-tauri/src/app/error.rs` 与 `src-tauri/src/app/mod.rs`。
  - `AppError` 与中文 `Display` 输出符合计划。
  - 验证通过：`cargo test --manifest-path src-tauri/Cargo.toml app::error`。
- 新增规则资产：
  - `AGENTS.md` 已加入 `DESIGN.md` 关键文件说明。
  - `DESIGN.md` 已加入 worktree，后续 UI 必须严格遵循。

#### 3. 中断时的处置决策

休眠触发时，刚完成 Task 3 的 code quality review：

- Task 3 审查结论：通过。
- 非阻塞建议：后续可补一个表驱动测试覆盖 `AppError` 的 4 个 `Display` 分支，当前不阻塞 Phase 1 继续推进。
- 当前没有未完成的代码编辑，选择“快速收尾”：只更新交接文档并提交现场。
- 未回滚任何实现提交。

#### 4. 架构与关键决策

- `DESIGN.md` 是当前 worktree 的新增视觉标准；后续所有前端 UI 必须使用其中的深色 Raycast 风格 token 与组件约束。
- 前端仍只是展示壳，不再调用 scaffold 的 `greet` Tauri command；真实录制命令等到 Task 8/9 按计划接入。
- `AppError` 提前进入 Task 2 提交是为了保持 `ScreenCapture` trait 使用计划指定的 `AppResult`，不是额外功能扩张。
- `src-tauri/Cargo.toml` 核心依赖版本未被 AI 修改。
- 尚未触碰 ScreenCaptureKit、unsafe/FFI、底层回调或 buffer 生命周期代码。
- `BUG.md` 当前暂无预防规则；本轮 review 已确认没有额外 bug 规则需要套用。

#### 5. 立即执行清单

1. 恢复后进入 worktree：
   ```bash
   cd /Users/root-mac/workspace_github/LuZhi/.worktrees/phase-1-macos-recording
   ```
2. 读取最新规则与交接：
   ```bash
   sed -n '1,260p' AGENTS.md
   sed -n '1,260p' DESIGN.md
   sed -n '1,260p' HANDOFF.md
   ```
3. 从 Phase 1 Task 4 开始：按 TDD 创建 `src-tauri/src/app/state_machine.rs` 的状态机测试，先看失败，再补实现。
4. Task 4 预期验证：
   ```bash
   cargo test --manifest-path src-tauri/Cargo.toml app::state_machine
   ```
5. Task 4 完成后继续执行 spec compliance review，再执行 code quality review；两者通过后再进入 Task 5。

#### 6. 当前报错/阻碍

当前无阻塞报错。

已知非阻塞事项：

- Task 3 code quality review 建议后续补全 `AppError` 四个 `Display` 分支的表驱动测试。
- `DESIGN.md` 和 `AGENTS.md` 是本轮新增/修改的规则资产，冬眠提交会一并保存，确保恢复后不会丢失视觉规范。

---

### 2026-05-24：Phase 1 scaffold 中断交接

#### 1. 当前任务上下文

正在执行 `docs/superpowers/plans/2026-05-23-phase-1-macos-recording-foundation.md` 的 Phase 1 / W1-W2：脚手架与 macOS 录制闭环。执行方式为 subagent-driven development，当前位于隔离 worktree：

- 路径：`/Users/root-mac/workspace_github/LuZhi/.worktrees/phase-1-macos-recording`
- 分支：`feat/phase-1-macos-recording`
- 最新实现提交：`fb7e648 chore(core): 保存Phase1脚手架现场`

#### 2. 已完成进度

- 已创建隔离 worktree，并从 `feat/architecture-planning` 派生实现分支。
- 已通过 create-tauri-app 生成 Tauri 2 + React + TypeScript scaffold。
- 已复制 scaffold 到 worktree 根目录。
- 已安装 Node 依赖和 Tailwind Vite 插件。
- 已配置 `vite.config.ts` 使用 `@tailwindcss/vite`。
- 已创建 `src/styles.css`，内容为 `@import "tailwindcss";`。
- 已删除临时 scaffold 目录 `luzhi/`。
- 已运行 `npm run build`，结果通过。
- 已按用户要求终止 `brew install rust`，不再继续 Homebrew 安装 Rust。

#### 3. 中断时的处置决策

休眠触发时，工作停在 Phase 1 Task 1 的 scaffold 验证阶段。选择“快速收尾”：

- 保留已经复制到 worktree 根目录的 scaffold。
- 删除临时生成目录 `luzhi/`。
- 不继续推进 Task 2，因为 Rust/Cargo 尚未配置。
- 不回滚 scaffold 修改，因为 Node 侧构建已通过，当前是可恢复的稳定点。

#### 4. 架构与关键决策

- 前端 scaffold 只作为 UI 壳，后续录制状态机仍必须放在 Rust 侧。
- `.gitignore` 保留 `.worktrees/`，避免隔离 worktree 被误追踪。
- `src-tauri/Cargo.toml` 依赖版本来自官方 scaffold，AI 未主动调整 Rust 核心依赖版本；恢复后仍需人工审查。
- 继续遵守数据流红线：音视频帧流不得进入前端 JS 层。

#### 5. 立即执行清单

1. 用户手动配置 `rustup`，确认 `cargo` 和 `rustc` 在 PATH：
   ```bash
   which cargo
   which rustc
   cargo --version
   rustc --version
   ```
2. 回到 worktree 并运行 Rust 验证：
   ```bash
   cd /Users/root-mac/workspace_github/LuZhi/.worktrees/phase-1-macos-recording
   cargo test --manifest-path src-tauri/Cargo.toml
   ```
3. 完成 Task 1 的 spec compliance review 与 code quality review；通过后进入 Task 2：Rust core media types and capture trait。

#### 6. 当前报错/阻碍

- `cargo` 当前不可用：
  ```text
  cargo not found
  ```
- `rustup` 此前也不可用：
  ```text
  rustup not found
  ```
- `brew install rust` 曾启动并下载依赖，但用户要求终止，最终会话输出包含：
  ```text
  Error: SIGTERM
  ```
- 用户计划手动清理 Homebrew `.incomplete` 缓存并手动配置 `rustup`。

---
