# LuZhi 项目交接文档

> 最后更新：2026-05-27 | Phase 4 光标平滑与点击放大实现完成（自动化验证全部通过，Native Safety Gate 待人工审查）。
>
> 更新本文件时，**必须**保持“项目概述 → 完整开发计划 → 工作任务记录（按**时间倒序**，并且只保留最近的 7 条记录） → 冬眠记录（按**时间倒序**，并且只保留最近的 7 条记录）”的结构顺序。

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

### 已知问题阻塞点

1. [x] **CoreMedia 链接错误阻塞 `npm run tauri dev`**（已修复：2026-05-25）
   - **症状**：`cargo build --bin luzhi` 报 `_CMFormatDescriptionGetStreamBasicDescription` 符号未定义
   - **根因**：使用了错误的符号名 `CMFormatDescriptionGetStreamBasicDescription`，正确符号为 `CMAudioFormatDescriptionGetStreamBasicDescription`
   - **修复**：Task 1 已替换为正确的 CoreMedia 音频格式符号，`cargo build` 通过

---

## 工作任务记录

### 2026-05-27：Phase 4 光标平滑与点击放大实现

输入文件：

- `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md`
- `tests/phase-4-w7-w8-checklist.md`

本轮完成（15 Tasks）：

1. `core/timeline.rs` 建立 `CursorSample` / `CursorClick` / `EffectTimeline` serde 模型。
2. `media/cursor_engine.rs` 实现移动平均、贝塞尔插值、点击放大状态机、CursorEffectEngine 组合。
3. `app/cursor_metadata_runtime.rs` 和 `platform/macos/cursor_source.rs` 建立录制期光标元数据采集。
4. `MacRecordingService` 停止录制后写入光标元数据边车文件。
5. Tauri 命令（`set_beautify_config`、`build_cursor_effect_timeline`、`export_video`）和预览页接入光标美化配置与时间线生成。
6. `CaptureConfig.show_system_cursor` 控制 SCK 原始光标绘制（美化开启时隐藏系统光标防双光标）。

验证结果：

- `cargo fmt --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` **103 tests** 通过（+27 相比 Phase 3）
- `cargo clippy --all-targets` 无 error（22 pre-existing SCK FFI warnings）
- `cargo build` 通过
- `npm run build` 通过
- `npm test -- --run` **25 tests** 通过（+2 相比 Phase 3）

改动文件：

- **新增**: `src-tauri/src/core/timeline.rs`, `src-tauri/src/core/processor.rs`, `src-tauri/src/media/cursor_engine.rs`, `src-tauri/src/media/recording_metadata.rs`, `src-tauri/src/app/cursor_metadata_runtime.rs`, `src-tauri/src/platform/macos/cursor_source.rs`
- **修改**: `src-tauri/src/core/frame.rs`, `src-tauri/src/core/mod.rs`, `src-tauri/src/core/config.rs`, `src-tauri/src/media/mod.rs`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/app/mod.rs`, `src-tauri/src/app/error.rs`, `src-tauri/src/app/events.rs`, `src-tauri/src/platform/macos/mod.rs`, `src-tauri/src/platform/macos/screen_capture_kit.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/lib.rs`, `src/lib/tauri.ts`, `src/components/preview-view.tsx`, `src/App.tsx`, `src/App.test.tsx`, `tests/phase-4-w7-w8-checklist.md`

剩余待完成（不阻塞合并判断但需关注）：

- `npm run tauri dev` 手动验证光标元数据、点击采集、时间线 JSON 和双光标策略。
- `platform/macos/cursor_source.rs` CoreGraphics FFI Native Safety Gate 人工逐行审查。
- Phase 6 接入生产 FFmpeg compositor 后做真实视频光标效果人工验收。

---

### 2026-05-27：Phase 3 Round 3 整改（复审后的剩余问题修复）

输入文件：

- `docs/superpowers/plans/2026-05-26-phase-3-code-review-remediation.md` Section 11

本轮修复（5 个 Phase）：

1. **R3-B**：`MicLevelRuntime` 旧 runtime 清理提升到 `mic_enabled` 分支之前，确保新 session 开始前无条件清理
2. **R3-C**：修复 `audio_clock_with_session_offset_starts_at_elapsed_time` flaky 测试（增加 1ms sleep）
3. **R3-A**：录制态麦克风电平可见闭环 — `RecordingStatusBar` 新增 `micEnabled`/`micVolume` props 和 5 段电平指示条，补充 `data-level` 测试
4. **R3-D**：文档与 checklist 状态同步
5. **R3-E**：`PreviewView` 导出卡片移除 `motion.div whileTap`，改用 CSS `active:scale-[0.98]`

验证结果：

- `cargo fmt --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` **76 tests** 通过（flaky 已修复）
- `cargo clippy --all-targets` 无 error（21 pre-existing warnings）
- `cargo build` 通过
- `npm run build` 通过
- `npm test -- --run` **23 tests** 通过

改动文件：

- **修改**: `src-tauri/src/lib.rs`, `src-tauri/src/core/clock.rs`, `src/components/recording-status-bar.tsx`, `src/App.tsx`, `src/App.test.tsx`, `src/components/preview-view.tsx`, `tests/phase-3-code-review-remediation-checklist.md`, `tests/phase-3-w5-w6-checklist.md`, `HANDOFF.md`

剩余待完成（不阻塞合并判断但需关注）：

- `npm run tauri dev` 手动验证（录制态 mic 电平可见性、快速点击稳定性、麦克风断开不崩溃）
- `permissions.rs` FFI Native Safety Gate 人工逐行审查
- 架构红线人工复核
- BUG.md 预防规则人工复核

---

### 2026-05-26：Phase 3 Code Review 首版整改 (Round 2)

输入文件：

- `docs/superpowers/plans/2026-05-26-phase-3-code-review-remediation.md`
- `tests/phase-3-code-review-remediation-checklist.md`

Round 2 完成项（7 个 Phase）：

1. **Phase A**：前端开始录制防重入 — `App.tsx` 增加 `isStartingRef` 守卫
2. **Phase B**：前端停止录制防重入 — `App.tsx` 增加 `isStoppingRef` 守卫
3. **Phase C**：`MicLevelRuntime` 生命周期管理 — 新建 `app/mic_level_runtime.rs`，实现 stop/join/Drop
4. **Phase D**：麦克风关闭与电平重置 — `capture_microphone == false` 时不启常驻 mic runtime；start/stop 中重置 `mic_level`
5. **Phase E**：权限测试隔离 — 提取 `map_av_authorization_status()`、`map_screen_preflight()` 纯函数
6. **Phase F**：测试补齐 — `MicLevelPayload` 序列化测试 + mic-level 事件测试 + 双击防重入测试
7. **Phase G**：`MicLevelDetector` 测试清理 — 重命名测试、清理注释

Round 2 复审发现的问题（已进入 Round 3 修复）：
- 防重入锁完全依赖事件清理，事件丢失可能造成 UI/Rust 脱节
- 录制态 UI 无 mic-level 可见展示入口
- `start_recording` 旧 `MicLevelRuntime` 清理只在 mic_enabled 分支执行
- Rust 时钟测试 flaky
- HANDOFF/checklist 状态与代码事实不一致

验证结果：

- `cargo fmt --check` 通过
- `cargo test` **76 tests** 通过（1 flaky 需复跑）
- `cargo clippy --all-targets` 无 error
- `cargo build` 通过
- `npm run build` 通过
- `npm test -- --run` **21 tests** 通过（Round 2 结束时实际为 21，HANDOFF 曾误写为 17）

改动文件：

- **新增**: `src-tauri/src/app/mic_level_runtime.rs`
- **修改**: `src/App.tsx`, `src/App.test.tsx`, `src-tauri/src/lib.rs`, `src-tauri/src/app/mod.rs`, `src-tauri/src/platform/macos/permissions.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/app/events.rs`, `src-tauri/src/media/mic_level.rs`

---

### 2026-05-26：Phase 3 录制 UI 与前后端联调完成

输入文件：

- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/` (本 Phase 计划在 `.claude/plans/` 中)
- `tests/phase-3-w5-w6-checklist.md`

已完成（9 个 Task 全部完成）：

1. **Task 1**：`CaptureMode` 枚举新增 `Window`/`Area` 变体 + `mode_from_str()` 解析方法
2. **Task 2**：`MacPermissionProbe` 接入真实 macOS 权限 API（`CGPreflightScreenCaptureAccess` + `AVCaptureDevice.authorizationStatusForMediaType:`）
3. **Task 3**：`MicLevelDetector` — 滑动窗口 RMS 电平计算器（`media/mic_level.rs`）
4. **Task 4**：消费线程集成 `MicLevelDetector` + `mic-level` 事件（100ms 间隔）推送
5. **Task 5**：`RecordingPanel` 新增分辨率/FPS 下拉 + 窗口/区域模式"即将推出"提示 + 按钮禁用
6. **Task 6**：`App.tsx` 替换 mic 随机数模拟为真实 `mic-level` 事件 + 状态转换加固 + 权限变更重检
7. **Task 7**：`lib/tauri.ts` 补充 `MicLevelPayload` 类型
8. **Task 8**：测试扩展（69 Rust + 14 前端 测试全部通过）
9. **Task 9**：手动验证清单更新到 `tests/phase-3-w5-w6-checklist.md`

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` 69 tests 通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 无 error（21 个 FFI naming warning 可接受）
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `npm run build` 通过
- `npm test -- --run` 14 tests 通过

关键决策：

- **窗口/区域模式前端传参、后端预留**：`CaptureMode` 枚举已扩展，`set_capture_mode` 接受三种模式参数，`start_recording` 对非全屏返回中文错误
- **真实 macOS 权限 API**：`CGPreflightScreenCaptureAccess()` + `AVCaptureDevice.authorizationStatusForMediaType:` 通过 FFI/objc2 调用，状态映射 0→NotDetermined, 1→Denied, 2→Denied, 3→Granted
- **MicLevelDetector 100ms 推送**：独立线程每 100ms 从 `Arc<Mutex<f64>>` 读取 RMS 值并通过 Tauri event 推送，替换前端 setInterval 模拟
- **权限变更重检**：`handleBackToIdle` 中重新调用 `fetchRecordingPermissions()`
- **状态转换加固**：所有 handler 增加当前状态检查守卫（`appState !== 'idle'` 直接 return）

后续入口：

1. `npm run tauri dev` 执行 Phase 3 手动验证清单（7 项）
2. 人工审查 `permissions.rs` 中的 FFI 调用（Native Safety Gate）
3. 进入 Phase 4（W7-W8）光标平滑与点击放大

---

### 2026-05-25：Phase 1/2 录制流水线整改计划执行完成

输入文件：

- `docs/superpowers/plans/2026-05-25-phase-1-2-recording-pipeline-remediation.md`
- `tests/phase-1-2-remediation-checklist.md`

已完成（15 个 Task 全部完成）：

1. **Task 1**：修复 CoreMedia 音频格式符号 `CMAudioFormatDescriptionGetStreamBasicDescription`
2. **Task 2**：录制回调改为有界非阻塞媒体队列 `MediaSender`/`MediaReceiver`
3. **Task 3**：ScreenCaptureKit 和 cpal 时间戳归一化（`TimestampNormalizer` + `AudioSampleClock`）
4. **Task 4**：音频缓冲解析增加 PCM 格式判断和动态 `AudioBufferList` 分配
5. **Task 5**：Tauri 录制命令迁移到 `spawn_blocking`，`TickRuntime` 可取消
6. **Task 6**：`stopCaptureWithCompletionHandler` 超时返回 `CaptureStopTimeout` 错误
7. **Task 7**：`AudioSynchronizer` 封装 `SimpleAudioMixer`，接入录制链路
8. **Task 8**：`RecordingWriter` trait 抽象 + `CountingRecordingWriter` 测试实现
9. **Task 9**：FFmpeg 生产写入器骨架（feature-gated `ffmpeg-next`）
10. **Task 10**：前端/后端命令参数对齐（`setCaptureMode`/`setAudioConfig` payload）
11. **Task 11**：权限探测改用 `PermissionService` + `MacPermissionProbe`
12. **Task 12**：移除区域级 `data-tauri-drag-region={false}` 标记
13. **Task 13**：Windows stub 编译边界修复（`CaptureConfig` 导入路径 + `compile_error!`）
14. **Task 14**：自动化验证矩阵（fmt/clippy/test/build）
15. **Task 15**：手动验收清单 + HANDOFF 更新

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` 43 tests 通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 无 error（FFI 命名 warning 可接受）
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `npm run build` 通过
- `npm test -- --run` 6 tests 通过

关键决策：

- **有界非阻塞队列**：`try_send_drop_newest` 策略，队列满时丢弃最新帧，避免捕获回调阻塞
- **会话相对时间戳**：第一个时间戳归零，解决跨源时间戳不一致问题
- **动态 AudioBufferList 分配**：两步查询模式，避免固定大小缓冲区溢出
- **FFmpeg feature gate**：`ffmpeg-next` 通过 Cargo feature 控制，开发环境可不安装

后续入口：

1. 进入 Phase 3 前，人工复审 ScreenCaptureKit/cpal unsafe 和资源释放路径
2. Windows DXGI/WASAPI 在 Windows 测试机上执行可行性验证
3. FFmpeg 写入器完整实现（编码 + 封装）

---

### 2026-05-24：端到端录制流水线连接

输入文件：

- `.claude/plans/docs-architecture-project-architecture-jaunty-horizon.md`

**根因：** `npm run tauri dev` 验证发现数据流在 5 层全部断裂：前端未调用 Tauri 命令、Tauri 命令仅切换状态机未调用录制服务、AppState 不持有 RecordingService、帧无消费者、预览界面为占位文字。

已完成（7 个 Task 全部完成）：

1. **Task 1**：`MacRecordingService` 具体结构体（`platform/macos_service.rs`），封装 `MacScreenCapture` + `CpalMicrophoneCapture` + 状态机 + 混音器 + drain 线程
2. **Task 2**：`AppState` 重构为持有 `Mutex<MacRecordingService>`（替代裸状态机）
3. **Task 3**：所有 Tauri 命令更新为调用真实 `MacRecordingService`（start/stop/pause/resume）
4. **Task 4**：帧消费线程（drain 模式，防止 channel 阻塞捕获线程）+ `recording-tick` 事件发射
5. **Task 5**：前端 `App.tsx` 替换 TODO 注释为真实 Tauri invoke 调用 + 事件监听
6. **Task 6**：`stop_recording` 返回 `RecordingResult`，`PreviewView` 显示录制统计
7. **Task 7**：验证（fmt/clippy/test/build 全通过）

验证结果：

- `cargo fmt --check` 通过
- `cargo clippy` 无错误
- `cargo test` 34/34 通过
- `cargo build --lib` 通过
- `npm run build` 通过
- `npm run test` 4/4 通过

关键决策：

- **`start_combined()` 方法**：`MacScreenCapture` 新增 `start_combined(config, video_sink, audio_sink)` 绕过 trait 方法歧义，原子化设置两个 sink
- **Drain 模式帧消费**：后台线程以 10ms 间隔 `try_recv()` 消费帧，计数器记录帧数，不做编码
- **`recording-tick` 线程**：独立线程每秒 emit 事件到前端，驱动计时器显示
- **`RecordingResult`**：`stop_recording` 返回帧数和时长，`output_path: None`（FFmpeg 编码未实现）

已知问题：

- **CoreMedia 链接错误**：`CMFormatDescriptionGetStreamBasicDescription` 符号在当前系统上无法解析（CoreMedia.framework 符号链接损坏），导致 `cargo build --bin luzhi` 失败。`cargo build --lib` 和 `cargo test` 正常。需修复系统 CoreMedia 框架或改用替代 API。

后续入口：

1. 修复 CoreMedia 链接问题后，`npm run tauri dev` 端到端验证
2. 进入 FFmpeg 集成实现视频编码和文件写入
3. 实现 `recording-tick` 精确计时（基于 SCStream 时间戳）

---

### 2026-05-24：Phase 2 完成（W3-W4 双音频链路）

输入文件：

- `docs/superpowers/plans/2026-05-24-phase-2-windows-capture-research-dual-audio.md`
- `tests/phase-2-w3-w4-checklist.md`

已完成（10 个 Task 全部完成）：

1. **Task 1**：`AudioChunk`、`MixedAudioChunk` 数据类型 + `AudioCapture` Trait + `AudioConfig`、`AudioDevice`、`AudioCapabilities`
2. **Task 2**：`AppError` 新增 `AudioCaptureFailed`、`AudioDeviceNotFound`、`AudioMixFailed` 三个变体
3. **Task 3**：`MacScreenCapture` 真实实现（SCStream 音视频统一捕获，`define_class!` 宏 + FFI 绑定）
4. **Task 4**：`CpalMicrophoneCapture` 实现（cpal 麦克风采集，`SendStream` 解决 `!Send` 问题）
5. **Task 5**：`SimpleAudioMixer` 实现（线性插值重采样 + 时间戳对齐 + 等权混音 + 硬限幅）
6. **Task 6**：`RecordingService<C: ScreenCapture, A: AudioCapture>` 双泛型重构，含 rollback 逻辑
7. **Task 7**：Windows `DxgiCapture` + `WasapiLoopback` Trait stub 骨架（`#[cfg(target_os = "windows")]`）
8. **Task 8**：Tauri 命令扩展（start/stop/pause/resume/set_capture_mode/set_audio_config）+ 状态机 Paused 状态 + `recording-state-changed` 事件
9. **Task 9**：音频单元测试补充（34 个测试全部通过）
10. **Task 10**：Phase 2 验证与交接

验证结果：

- `cargo fmt --check` 通过
- `cargo clippy` 无错误（FFI 相关 warning 可接受）
- `cargo test` 34/34 通过
- `npm run build` 通过
- `npm run test` 4/4 通过

关键决策：

- **SCStream 统一音视频**：ScreenCaptureKit 的 SCStream 同时输出视频帧和系统音频，无需分离
- **SendSCStream wrapper**：objc2 的 SCStream 是 `!Send`，用 unsafe newtype 解决（基于原子引用计数）
- **SendStream wrapper**：cpal 的 Stream 也是 `!Send`，同样用 unsafe newtype 解决
- **`objc2::define_class!` 宏**：必须使用完整路径调用，不能通过 `use` 导入
- **双泛型 RecordingService**：`<C: ScreenCapture, A: AudioCapture>` 分离视频捕获和麦克风捕获
- **实时混音**：录制期间实时混合系统音频和麦克风（非录后混音）

后续入口：

1. `npm run tauri dev` 验证端到端音视频录制
2. 进入 Phase 3（W5-W6）录制 UI 与前后端联调
3. ScreenCaptureKit FFI 代码需人工逐行审查内存安全

---

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

### 2026-05-24：端到端流水线连接完成冬眠

#### 1. 当前任务上下文

修复 `npm run tauri dev` 端到端验证发现的数据流断裂问题（5 层全部断裂）。当前在 `feat/architecture-planning` 分支。

#### 2. 已完成进度

- `MacRecordingService` 具体结构体（`platform/macos_service.rs`），封装 SCStream + cpal + 状态机 + drain 线程
- `MacScreenCapture::start_combined()` 原子化设置 video + audio sink
- `AppState` 重构为 `Mutex<MacRecordingService>`（替代裸状态机）
- 所有 6 个 Tauri 命令连接到真实服务 + `recording-tick` 事件发射
- 前端 `App.tsx` 替换 TODO 为真实 invoke + 事件监听
- `PreviewView` 接收 `RecordingResult` 显示录制统计
- 全部验证通过：`cargo fmt`、`cargo clippy`、`cargo test` 34/34、`npm run build`、`npm run test` 4/4

#### 3. 中断时的处置决策

休眠触发时，所有 7 个 Task 已完成，代码处于稳定态。用户要求将 CoreMedia 链接错误记录到"已知问题阻塞点"后执行冬眠。选择"直接冬眠"：无需回滚或额外收尾。

#### 4. 架构与关键决策

- **`MacRecordingService` 替代泛型 `RecordingService<C, A>`**：macOS 的 SCStream 是统一流，需要同一个 `MacScreenCapture` 对象同时处理视频和系统音频，泛型设计无法满足
- **`start_combined()` 方法**：绕过 `ScreenCapture`/`AudioCapture` trait 方法歧义（两个 trait 都有 `start()`/`stop()`），原子化设置两个 sink
- **Drain 模式帧消费**：后台线程 10ms 间隔 `try_recv()` 消费帧，防止 channel 阻塞 SCStream 回调线程。不做编码
- **`recording-tick` 线程**：独立线程每秒 emit 事件到前端，驱动计时器

#### 5. 立即执行清单

1. 修复 CoreMedia 链接错误（见"已知问题阻塞点"第 1 项的 3 个方案）
2. `npm run tauri dev` 端到端验证完整录制流程
3. 进入 FFmpeg 集成实现视频编码和文件写入

#### 6. 当前报错/阻碍

**CoreMedia 链接错误**（阻塞 `npm run tauri dev`）：

```
Undefined symbols for architecture arm64:
  "_CMFormatDescriptionGetStreamBasicDescription", referenced from:
    luzhi_lib::platform::macos::screen_capture_kit::cmformat_description_get_stream_basic_description
ld: symbol(s) not found for architecture arm64
```

根因：`/System/Library/Frameworks/CoreMedia.framework/CoreMedia` 是断开的符号链接。详见"已知问题阻塞点"第 1 项。

---

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
