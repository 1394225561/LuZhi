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

---

## 冬眠记录

---
