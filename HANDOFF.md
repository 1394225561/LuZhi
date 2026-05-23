# LuZhi 项目交接文档

> 最后更新：2026-05-23 | 完成系统架构设计与 MVP W1-W12 执行计划落地

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

---

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
