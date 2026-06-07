# LuZhi 项目交接文档

> 最后更新：2026-06-07 | BUG-0023 60fps 录制素材导出重复 PTS 修复完成，导出器会跳过映射到同一 30fps 输出 PTS tick 的源帧。
>
> 更新本文件时，**必须**保持”项目概述 → 完整开发计划 → 工作任务记录（按**时间倒序**，并且只保留最近的 7 条记录） → 冬眠记录（按**时间倒序**，并且只保留最近的 7 条记录）”的结构顺序。

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

### 2026-06-07：BUG-0023 60fps 录制素材导出时报“导出失败”

输入文件：

- 用户反馈：60fps 录制已能进入预览，但导出阶段报“导出失败，请重试或检查录制素材”
- 用户提供完整终端日志：录制 audio contract 通过，导出阶段 x264 报 `non-strictly-monotonic PTS`，MP4 muxer 报 `stream 0: 1024 >= 1024`
- `HANDOFF.md`
- `BUG.md`
- `.claude/rules/0-global.md`
- `.claude/rules/2-testing.md`
- `src-tauri/src/media/trim_exporter.rs`
- `src-tauri/src/media/export_presets.rs`

根因结论：

1. 这次不是录制 finalize 失败：日志显示 `录制音频 contract 验证通过`，writer diagnostics 中 `audio_chunks_appended=481`
2. 失败发生在导出阶段：第二段 x264 日志中出现多次 `non-strictly-monotonic PTS`
3. 导出 preset 当前固定 30fps；60fps 源素材相邻两帧重采样到导出 encoder time base `1/30` 时，可能映射到相同输出 PTS
4. 旧逻辑 `out_pts.max(last_video_out_pts)` 只保证非递减，仍会把重复 PTS 送入 x264，最终 MP4 muxer 拒绝 `1024 >= 1024`

已完成：

1. `FfmpegTrimExporter` 的 `last_video_out_pts` 初始值改为 `-1`，确保首帧 PTS=0 可写入
2. 导出视频帧若映射后的 `out_pts <= last_video_out_pts`，直接跳过该重复 tick 源帧
3. 新增 `ffmpeg_exporter_exports_60fps_source_to_30fps_preset_without_duplicate_pts` 回归测试，使用 60fps writer 生成源素材，再导出到 Bilibili 30fps preset
4. 更新 `BUG.md` BUG-0023 预防规则和本轮自测清单

当前验证结果：

- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_exports_60fps_source_to_30fps_preset_without_duplicate_pts -- --nocapture` 修复前失败，错误为 `写入视频数据包失败: Invalid argument`，并复现 `non-strictly-monotonic PTS`
- GREEN：同一测试修复后通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg trim_exporter -- --test-threads=1` 通过（8 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_writer_preserves_av_duration_at_60fps` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg validate_export_artifact -- --test-threads=1` 通过
- `rustfmt --check --edition 2021 --config skip_children=true src-tauri/src/media/trim_exporter.rs` 通过
- `git diff --check` 通过
- 剩余 warning 为既有 FFI/可见性/死代码类告警

改动文件：

- **修改**: `src-tauri/src/media/trim_exporter.rs`
- **修改**: `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-07-bug-0023-60fps-export-duplicate-pts-checklist.md`

人工复核建议：

1. 全屏录制选择 60fps，同时开启系统音频和麦克风，录制约 10-30 秒后停止，应进入预览
2. 在预览页导出 Bilibili / YouTube preset，应成功生成导出文件，不再出现 `non-strictly-monotonic PTS` 或 `non monotonically increasing dts`
3. 回放导出文件，确认视频时长接近实际录制时长，音频存在且可播放

### 2026-06-07：BUG-0022 60fps 录制结束时报视频/音频时长偏差过大

输入文件：

- 用户反馈：选择 60fps 录制，点击结束录制后界面报错“录制视频/音频时长偏差过大”
- 用户提供完整终端日志：x264 输出 `frame I:7`、`frame P:1608`，最终校验报视频 `53800ms`、音频 `28288ms`
- `HANDOFF.md`
- `BUG.md`
- `.claude/rules/0-global.md`
- `.claude/rules/2-testing.md`
- `src-tauri/src/lib.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/platform/macos_service.rs`

根因结论：

1. x264 总帧数为 `7 + 1608 = 1615` 帧；按 30fps 计算约 `53.8s`，正好对应报错视频 `53800ms`
2. 同样 1615 帧按 60fps 计算约 `26.9s`，接近报错音频 `28.3s`，说明实际录制时长并非视频真的 53.8 秒
3. 全屏 ScreenCaptureKit 已使用 `config.fps` 设置最小帧间隔；但 `FfmpegRecordingWriter` 内部 H.264 encoder time base、video stream time base、视频 PTS 计算、packet rescale 和尾部 audio padding 均硬编码 30fps
4. 根因是 capture fps 没有传入 writer，导致 60fps 视频在 MP4 写入阶段被按 30fps 时间基准解释，视频时长被放大约 2 倍

已完成：

1. `FfmpegRecordingWriter::new()` 保持默认 30fps，新增 `FfmpegRecordingWriter::with_fps(output_path, video_fps)`
2. FFmpeg worker 统一使用传入的 `video_fps` 设置 encoder/stream time base、PTS 计算、packet rescale、尾部 padding frame duration
3. macOS 全屏录制创建 FFmpeg writer 时传入 `config.fps`
4. 保持窗口录制现有 30fps 行为不变，因为当前窗口捕获路径本身仍配置为 30fps
5. 新增 `ffmpeg_writer_preserves_av_duration_at_60fps` 回归测试，覆盖 3 秒 60fps 视频 + 3 秒音频 artifact 的 A/V drift
6. Code Review Finding 修复：`set_capture_mode` payload 在后端边界只允许 30/60fps，`120` 等非支持值直接返回错误
7. Code Review Finding 修复：新增 fullscreen writer settings 生产链路测试，覆盖 `CaptureConfig.fps -> FFmpeg writer settings`
8. 更新 `BUG.md` BUG-0022 预防规则和本轮自测清单

当前验证结果：

- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_writer_preserves_av_duration_at_60fps -- --nocapture` 修复前失败，`FfmpegRecordingWriter::with_fps` 不存在
- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib capture_mode_update` 在补边界校验前失败，`120fps` payload 未被拒绝
- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib fullscreen_writer_settings_uses_capture_fps` 在补生产链路 helper 前编译失败，`recording_writer_settings_for_capture` 不存在
- GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_writer_preserves_av_duration_at_60fps` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_writer -- --test-threads=1` 通过（26 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml --lib capture_mode_update` 通过（2 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml --lib fullscreen_writer_settings_uses_capture_fps` 通过
- `rustfmt --check --edition 2021 --config skip_children=true src-tauri/src/lib.rs src-tauri/src/platform/macos_service.rs src-tauri/src/media/ffmpeg_writer.rs` 通过
- `git diff --check` 通过
- 普通 `rustfmt --check --edition 2021 src-tauri/src/lib.rs src-tauri/src/platform/macos_service.rs src-tauri/src/media/ffmpeg_writer.rs` 会沿模块树报告既有未触碰文件 `src-tauri/src/app/error.rs`、`src-tauri/src/app/recording_library.rs` 的格式差异，本轮未顺手格式化无关文件
- 剩余 warning 为既有 FFI/可见性/死代码类告警

改动文件：

- **修改**: `src-tauri/src/lib.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/platform/macos_service.rs`
- **修改**: `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-07-bug-0022-60fps-recording-writer-checklist.md`

人工复核建议：

1. 全屏录制选择 60fps，同时开启系统音频和麦克风，录制约 25-35 秒后点击结束录制，应进入预览，不再报视频/音频时长偏差过大
2. 查看终端日志，x264 帧数除以 60fps 后应接近实际录制时长；artifact 校验不应再把视频时长按 30fps 放大
3. 再录一次 30fps，确认默认 30fps 路径仍能正常结束并生成可播放文件

### 2026-06-07：BUG-0021 有线耳机麦克风电流声残留降噪优化

输入文件：

- 用户反馈：开启麦克风录制并使用有线耳机麦克风时，已有高通滤波降噪后仍有轻微电流声
- `HANDOFF.md`
- `BUG.md`
- `.claude/rules/0-global.md`
- `.claude/rules/2-testing.md`
- `src-tauri/src/media/audio_denoise.rs`
- `src-tauri/src/media/audio_mixer.rs`

根因结论：

1. 当前降噪开关对应 `DenoiseMode::Highpass`，实际只在 `SimpleAudioMixer` 麦克风路径中使用 80Hz 二阶高通滤波器
2. 80Hz 高通能抑制 DC、低频轰鸣和部分 50/60Hz 基频，但对有线耳机麦克风常见的 100/120Hz、150/180Hz 低阶谐波和稳定窄带尖峰抑制不足
3. 最小安全插入点是 Rust 侧 mixer 的麦克风处理链路；无需触碰 cpal 采集线程、FFI、前端 UI 或核心依赖

已完成：

1. `audio_denoise.rs` 新增 `NotchFilter` 二阶窄带陷波滤波器
2. 新增 `MicrophoneDenoiseChain`，串联 80Hz 高通和 50/60/100/120/150/180Hz 窄带 notch
3. `SimpleAudioMixer` 将每通道状态从单一 `HighpassFilter` 升级为 `MicrophoneDenoiseChain`
4. 外部配置保持不变：仍使用现有“降噪（去除电流声）”开关和 `DenoiseMode::Highpass`
5. 降噪链路仍只处理麦克风，系统音频保持原样
6. 新增合成音频测试覆盖工频谐波进一步衰减与多个非 notch 人声代表频段保真
7. 新增系统单源回归测试，确保降噪开启时系统音频不会进入麦克风降噪链路
8. Code Review 后补强：工频谐波测试改为同时叠加的 composite 信号，并逐个目标频点验证衰减
9. Code Review 后修复：麦克风通道数变化时重建每通道降噪链路，避免 mono/stereo 切换导致状态下标越界 panic
10. 更新 `BUG.md` BUG-0021 预防规则和本轮自测清单

当前验证结果：

- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_reduces_powerline_harmonics_beyond_highpass` 修复前失败，失败信息为 `highpass=0.04565383, denoised=0.04565383`
- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib mixer_highpass_does_not_filter_system_only_audio` 旧行为下失败，失败信息为 `System-only audio should not be denoised, got avg 0.000012978191`
- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib mixer_highpass_handles_mic_channel_count_increase_without_panic` 旧行为下失败，失败信息为 `index out of bounds: the len is 1 but the index is 1`
- GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise` 通过（10 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer` 通过（21 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过（341 tests）；剩余 warning 为既有 FFI/可见性/死代码类告警
- `rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs` 通过
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 未全绿：在既有未触碰文件 `src-tauri/src/app/error.rs`、`src-tauri/src/app/recording_library.rs` 上报告格式差异；本轮未顺手格式化无关文件

改动文件：

- **修改**: `src-tauri/src/media/audio_denoise.rs`, `src-tauri/src/media/audio_mixer.rs`
- **修改**: `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-07-mic-denoise-harmonics-checklist.md`

人工复核建议：

1. 使用有线耳机麦克风，开启降噪录制 10 秒静音环境；回放确认轻微电流声进一步减弱
2. 同一设备关闭降噪录制，作为对照确认开关仍有效
3. 开启降噪录制正常说话，确认人声明显可懂、无明显闷声或抽吸感
4. 同时录制系统音频和麦克风，确认系统音频音质不受影响

### 2026-06-07：BUG-0020 双音频窗口录制退出时近乎静音误判修复

输入文件：

- 用户反馈：窗口录制同时开启系统音频和麦克风，通过“退出”或 `Command + Q` 结束软件进程后，录制 finalize 报“请求了音频录制但解码后音频近乎静音”
- `BUG.md` BUG-005 / BUG-005_2 / BUG-0013 预防规则
- `HANDOFF.md`
- `.claude/rules/0-global.md`
- `.claude/rules/2-testing.md`

根因结论：

1. 日志证明 `writer.finish()`、mic stop、RecordingDiagnostics、WriterDiagnostics 和 artifact decode validation 均已执行；这不是退出事件未清理或 writer 未完成
2. 失败点是 source artifact aggregate RMS/peak Level 1 校验先于 source-aware contract 执行
3. 系统音频全程静音、麦克风较安静时，global decoded RMS/peak 被系统静音与时间轴 padding 稀释到阈值以下；但 source-aware/writer 诊断已证明系统音频和麦克风 chunks/windows/frames/counters 均到达 writer

已完成：

1. `RequestedAudioContract` 新增 `allow_quiet_when_source_verified`，默认保持严格校验
2. source artifact contract 在 source-aware 已验证且 decoded 音频非零时，将低音量 aggregate 判定降级为 warning
3. macOS 录制收尾改为先执行 `validate_source_aware_audio_contract()`，只有通过后才允许 source artifact low-volume bypass
4. 导出路径未启用该 bypass，避免无 source-aware 诊断的 export 校验误放宽
5. 新增回归测试覆盖 source-aware verified 双源低音量通过、全零音频仍失败、source artifact contract 构造策略
6. 更新 `BUG.md` BUG-0020 预防规则和本轮自测清单

当前验证结果：

- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg requested_audio_contract_allows_quiet_dual_source_when_source_verified` 修复前失败，失败信息为 `RMS=0.002007 < 0.003000`、`peak=0.002338 < 0.020000`
- GREEN：同一测试修复后通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg requested_audio_contract_source_verified_still_rejects_zero_audio` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg source_artifact_audio_contract` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg requested_audio_contract` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过（333 tests）
- `npm test -- --run` 通过（80 tests）
- `npm run build` 通过
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过；剩余 warning 为既有 FFI/可见性/死代码类告警
- `rustfmt --check src-tauri/src/media/ffmpeg_common.rs src-tauri/src/platform/macos_service.rs` 通过
- `git diff --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg` 未全绿：本轮新增/相关音频测试通过，但 `media::cursor_overlay::tests::rendered_arrow_rgba_blends_yuv_planes` 单独运行仍失败，失败断言为 `arrow cursor should be visible on Y plane near cursor position`；该测试不在本轮音频 contract 修改链路中

改动文件：

- **修改**: `src-tauri/src/media/ffmpeg_common.rs`, `src-tauri/src/platform/macos_service.rs`
- **修改**: `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-07-bug-0020-window-recording-quit-audio-contract-checklist.md`

人工复核建议：

1. 窗口录制同时开启系统音频和麦克风，保持系统音频静音，用较低麦克风音量录制 5-10 秒后点击“退出”结束应用；应不再报近乎静音 finalize failure
2. 同一场景用 `Command + Q` 结束应用；应生成可预览录制结果，若音量偏低只应在日志中出现 warning
3. 真正麦克风无输入/权限异常/无 chunks 的场景仍应被 source-aware contract 拦截为失败

### 2026-06-07：窗口录制 Code Review 阻断问题修复

输入文件：

- `docs/superpowers/plans/2026-06-07-window-recording-plan.md`
- Code review 范围：`1c5d4260cde5ee8970caa4bd801bb8ea13d0d3b9 ~ f4d386927afc8bce06f717738ddc0a1a121d2cb2`
- `BUG.md`
- `.claude/rules/0-global.md`
- `.claude/rules/1-coding-style.md`
- `.claude/rules/2-testing.md`
- `.claude/rules/4-security.md`

已完成：

1. 修复窗口 ID 丢失：前端 `App` 持有选中窗口 ID，开始录制时传入 `set_capture_mode.windowId`；后端配置更新保留窗口模式已有 `window_id`
2. 修复窗口关闭只 toast：窗口关闭事件复用 stop/finalize/register 清理路径自动停止录制，并在 completed 事件中携带录制结果供前端预览
3. 最小化/恢复窗口时触发后端 pause/resume 状态，暂停期间消费线程继续排空但丢弃音视频，UI 状态不再只靠 toast 文案
4. 修复窗口录制启动失败回滚：writer 创建提前到 native capture 启动之前，mic 启动失败会停止窗口 monitor 和 SCK stream
5. 修复 Retina/多显示器窗口几何：窗口模式使用 `SCContentFilter.contentRect()` 和 `pointPixelScale()` 计算 `CaptureGeometry`
6. macOS `MacScreenCapture` 实现 `WindowCapture` trait，capabilities 改为 `supports_window: true`
7. `WindowSelector` 不再吞掉 `list_windows` 错误，新增错误文案和重试按钮；合并重复 minimized window 测试
8. 窗口 monitor 文档/行为对齐为 200ms 轮询，不再声称未实现的 NSWorkspace 通知层
9. 明确缩略图在 MVP 中为 `None` 降级，UI 使用占位图；未擅自修改核心依赖版本或新增编码依赖
10. 更新 `BUG.md` BUG-0019 预防规则和本轮自测清单

当前验证结果：

- `npm test -- src/App.test.tsx -t "preserves the selected window"` 通过
- `npm test -- src/App.test.tsx -t "uses recording result from completed state event"` 通过
- `npm test -- src/components/window-selector.test.tsx` **6 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib consume_frames_drops_queued_media_when_stopped_while_paused` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过；剩余 warning 为既有 FFI/可见性/死代码类告警

改动文件：

- **修改**: `src/App.tsx`, `src/App.test.tsx`, `src/lib/tauri.ts`, `src/components/recording-panel.tsx`, `src/components/window-selector.tsx`, `src/components/window-selector.test.tsx`
- **修改**: `src-tauri/src/lib.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/platform/macos/screen_capture_kit.rs`, `src-tauri/src/platform/macos/window_list.rs`, `src-tauri/src/platform/macos/window_monitor.rs`
- **修改**: `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-07-window-recording-review-fixes-checklist.md`

人工复核建议：

1. 选择一个正常窗口录制：点击开始后不应再出现“未选择录制窗口”
2. 录制中关闭被录制窗口：应提示录制停止，录制结果进入预览并出现在历史记录
3. 录制中最小化/恢复被录制窗口：状态栏应切换暂停/录制中，暂停期间内容不应继续写入最终录制
4. Retina 屏窗口录制后导出美化：光标位置应与源视频一致
5. 窗口选择器在权限/枚举失败时显示错误和“重试”，而不是“未找到可用窗口”

### 2026-06-05：播放控件完整功能实现

输入文件：

- `docs/superpowers/specs/2026-06-05-playback-controls-design.md`
- `docs/superpowers/plans/2026-06-05-playback-controls.md`
- `reference/ui/ui_spec.md`
- `.claude/rules/0-global.md`
- `.claude/rules/1-coding-style.md`
- `.claude/rules/2-testing.md`

已完成：

1. 添加 `videoRef`、`videoContainerRef`、`isSeekingRef` 引用
2. 修改初始状态：`currentTime` 从 45→0，`duration` 从 180→0（由视频元数据驱动），新增 `isMuted` 状态
3. 添加视频事件监听 useEffect（`loadedmetadata`、`timeupdate`、`play`、`pause`、`ended`）
4. 移除 `<video>` 标签的原生 `controls` 属性，添加 `ref` 和 `playsInline`
5. 实现 `handlePlayPause`（调用 `video.play()` / `video.pause()`）
6. 实现 `handleSkipBack` / `handleSkipForward`（±10 秒跳转）
7. 进度条使用 `onValueCommit` 实现松手 seek，`isSeekingRef` 防止拖拽时跳动
8. 实现 `handleVolumeChange` 和 `handleMuteToggle`，音量图标切换 `Volume2` / `VolumeX`
9. 实现 `handleFullscreen`（`requestFullscreen()` / `exitFullscreen()`）
10. 所有播放控件按钮添加 `disabled={!recordingResult?.outputPath}`
11. 新增 3 个前端测试：play/pause 按钮调用 video.play()、无视频源时按钮禁用、slider 存在性验证

当前验证结果：

- `npm test -- --run` **63 tests** 通过（+3 相比之前）
- `npm run build` 通过
- `git diff --check` 通过

改动文件：

- **修改**: `src/components/preview-view.tsx`, `src/App.test.tsx`
- **新增**: `docs/superpowers/specs/2026-06-05-playback-controls-design.md`, `docs/superpowers/plans/2026-06-05-playback-controls.md`

人工复核建议：

1. 播放按钮点击 → 视频开始播放，图标变为 Pause
2. 暂停按钮点击 → 视频暂停，图标变为 Play
3. 后退按钮 → 视频后退 10 秒
4. 前进按钮 → 视频前进 10 秒
5. 进度条拖拽 → 视频 seek 到目标位置，松手前不跳动
6. 音量滑块 → 视频音量实时变化
7. 静音图标点击 → 视频静音/取消静音，图标切换
8. 全屏按钮 → 视频容器进入全屏
9. 播放结束 → 自动暂停在最后一帧
10. 无录制文件 → 控件按钮禁用

### 2026-06-05：BUG-0014 美化界面拖拽触发范围第三轮修复

输入文件：

- `BUG.md` BUG-0014
- 用户二次反馈：导出视频后保存路径文字无法拖选，仍会触发窗口拖拽
- 用户三次反馈：第二轮修复过头，美化界面所有区域都无法触发窗口拖拽；期望主预览区域和右侧边栏空白可拖，按钮/文本不可拖
- `reference/ui/ui_spec.md`
- `reference/ui/ui-migration-spec.md`
- `.codex/rules/0-global.md`
- `.codex/rules/1-coding-style.md`
- `.codex/rules/2-testing.md`

已完成：

1. 按 systematic debugging 完成根因调查：`App.tsx` 使用 `closest('[data-tauri-drag-region]')`，导致根拖拽区域下的标题/说明文本继承拖拽能力
2. 识别第二个阻断复制的根因：全局 `selectstart` 被 `preventDefault()`，即使拖拽修复后也无法选择文本
3. 根据用户二次反馈补充根因：`PreviewView` 根容器仍保留 `data-tauri-drag-region="deep"`，真实 Tauri WebView 原生拖拽机制会绕过程序化监听过滤，导出保存路径仍位于该祖先下
4. 根据用户三次反馈补充根因：第二轮将 `data-luzhi-drag-region` 放在预览页根节点，并要求事件目标本身带标记；左右内容容器覆盖根节点，导致空白区域也无法命中拖拽标记
5. 新增/调整前端回归测试覆盖：主预览区域空白触发拖拽、右侧边栏空白触发拖拽、预览文本不触发拖拽、预览文本允许 `selectstart`、导出保存路径不位于 Tauri 原生 drag-region 祖先下且不会触发 `startDragging()`
6. 最小修复：保留右键菜单禁用；移除全局 `selectstart` 阻止；将 `src` 中真实 `data-tauri-drag-region` 全部替换为应用自定义 `data-luzhi-drag-region`
7. `PreviewView` 改为在主预览区域和右侧边栏两个内容容器上放置 `data-luzhi-drag-region`
8. 程序化窗口拖拽改为：命中自定义拖拽容器内空白区域才触发；按钮、输入控件、视频控件、可交互角色和文本元素不触发
9. 更新 `BUG.md` 根因、修复内容和预防规则
10. 新增并更新本轮自测清单

当前验证结果：

- 第一轮目标红绿测试：修复前 2 failed / 1 passed；修复后 `npm test -- src/App.test.tsx -t "does not start window drag|keeps direct blank|allows preview text selection"` **3 tests** 通过
- 第二轮导出路径测试：修复前 `keeps exported output path outside Tauri native drag regions` 失败，失败证据显示最近祖先为 `data-tauri-drag-region="deep"` 的预览页根容器；修复后目标测试 **4 tests** 通过
- 第三轮左右容器测试：修复前 `starts window drag from main preview container blank area` / `starts window drag from right sidebar blank area` 失败；修复后 BUG-0014 目标测试 **5 tests** 通过
- `npm test -- --run` **60 tests** 通过
- `npm run build` 通过
- `git diff --check` 通过

改动文件：

- **修改**: `src/App.tsx`, `src/App.test.tsx`, `src/components/preview-view.tsx`, `src/components/processing-view.tsx`, `src/components/error-view.tsx`, `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-05-bug-0014-drag-region-checklist.md`

人工复核建议：

1. 预览美化界面拖动空白区域：窗口可以移动
2. 拖动标题/说明文本：不会移动窗口，文本可被选择复制
3. 导出成功后拖选保存路径文字：不会移动窗口，路径可被选中复制
4. 点击/拖动返回、播放、滑块、开关、导出按钮：不会触发窗口拖拽

### 2026-06-05：BUG-0011 第 6 轮 FFI Code Review Findings 修复

输入文件：

- 用户反馈：第 6 轮人工验证效果没问题
- Code review findings：重点修复 NSCursor FFI selector guard、AppKit 线程边界、autorelease 生命周期、C string 类型安全和 clippy 近邻告警
- `BUG.md` BUG-0011 预防规则 25-29

已完成：

1. 新增 `CursorMainThreadDispatcher`，生产录制链路通过 Tauri `AppHandle::run_on_main_thread` 执行 AppKit cursor 读取
2. `MacCursorKindProvider` 构造时必须接收 main-thread dispatcher，避免生产路径在 cursor metadata 后台线程直接调用 AppKit
3. ObjC 消息发送前增加 class/instance method guard，缺失 selector 时 fail closed
4. selector 参数改为 `&CStr` / `c"..."`，移除裸 `&[u8]` C string 边界
5. autorelease pool 改为 `objc_autoreleasePoolPush/Pop`
6. legacy AX 的 `NSWorkspace` ObjC 调用同步使用 guarded helper
7. 新增 main-thread reader 调度成功/失败测试
8. 清理本轮 review 指出的 `cursor_source.rs` / `cursor_metadata_runtime.rs` unused import 告警
9. 更新 BUG.md 和本轮自测清单

当前验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --lib cursor_kind` **12 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` **294 tests** 通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib` 通过；剩余 **32 个既有 warning**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `npm test -- --run` **55 tests** 通过
- `git diff --check` 通过

改动文件：

- **修改**: `src-tauri/src/platform/macos/cursor_kind.rs`, `src-tauri/src/platform/macos/cursor_source.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/app/cursor_metadata_runtime.rs`, `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-05-bug-0011-ffi-review-findings-checklist.md`

人工复核建议：

1. 普通桌面/空白区域：导出美化光标为 Arrow
2. WebView/浏览器按钮或链接：导出美化光标为 Hand
3. 文本输入框：导出美化光标为 IBeam
4. 四指上划进入 Mission Control：macOS 显示 Arrow，导出美化视频也显示 Arrow

## 冬眠记录

### 2026-06-04：BUG-0011 方案B 实现完成、准备尝试方案A 冬眠

#### 1. 当前任务上下文

BUG-0011 光标类型始终为 Arrow。已尝试方案B（增强 AX 分类），包括前台应用 AX 查询，但 WebView 内容对 AX hit-test 不透明。准备尝试方案A（NSCursor API）。当前在 `feat/architecture-planning` 分支。

#### 2. 已完成进度

- 方案B 实现：添加 `AXUIElementCreateApplication` + `get_frontmost_app_pid()` + 前台应用优先查询
- 测试结论：`src=front` 查询成功，但 `role_chain` 始终返回 `AXMenuBar`/`AXGroup`，无法穿透 WebView
- 诊断日志显示 `arrow=480 hand=0 ibeam=0`，确认 AX API 对 WebView 内容无效
- Git 已提交（`2a6f814`），未 push

#### 3. 中断时的处置决策

方案B 代码已提交，状态稳定。决定尝试方案A（NSCursor API），冬眠保存现场。

#### 4. 架构与关键决策

- **AX API 无法穿透 WebView**：`AXUIElementCopyElementAtPosition` 无论系统级还是应用级查询，都无法获取 WebView 内部 DOM 元素的 AX role
- **前台应用 PID 方案**：通过 `NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier` 获取 PID，再用 `AXUIElementCreateApplication(pid)` 查询。技术上可行，但 WebView 内容不暴露交互元素
- **方案A（NSCursor API）**：下一步调研 `[NSCursor currentCursor]` 在后台线程查询实时光标类型的可行性

#### 5. 立即执行清单

1. 调研 `[NSCursor currentCursor]` API 的线程安全性和实时性
2. 如果可行，实现 NSCursor-based CursorKindProvider
3. 验证：在 Tauri 应用中悬停链接/按钮/输入框，检查光标类型是否正确切换
4. 验证完成后移除 diagnostic logging

#### 6. 当前报错/阻碍

无编译报错。核心阻碍是 AX API 对 WebView 内容的固有限制。

---

### 2026-06-03：BUG-0010/0011/0012 实施计划编写完成冬眠

#### 1. 当前任务上下文

完成 BUG-0010（光标偏移）、BUG-0011（光标不好看）、BUG-0012（导出进度不优雅）三条未解决 bug 的根因分析和详细实施计划编写。当前在 `feat/architecture-planning` 分支。

#### 2. 已完成进度

- 完成三条 bug 的静态链路审查和根因定位（无代码修改）
- 编写 16 个 Task 的详细实施计划，保存至 `docs/superpowers/plans/2026-06-03-bug-0010-0011-0012-fix-plan.md`
- 计划包含完整代码、测试、验证命令、commit 消息
- 更新 BUG.md 新增 BUG-0010/0011/0012 未解决条目
- Git 已提交（`556ad79`），未 push

#### 3. 中断时的处置决策

休眠触发时，计划编写刚完成，未开始任何代码修改。选择"直接冬眠"：无需回滚或额外收尾，状态完全稳定。

#### 4. 架构与关键决策

- **坐标归一化在 recorder 层完成**：`CursorMetadataRuntime` 接收 `CaptureGeometry`，`CursorMetadataRecorder` 内部持有 `CursorCoordinateMapper`，在 `record_snapshot()` 时将全局屏幕坐标归一化为源视频像素坐标。`CursorOverlayRenderer` 不变。
- **CursorKind 使用 `#[serde(default)]`**：旧 timeline JSON 没有 `kind` 字段时默认为 `Arrow`，保证向后兼容。
- **glyph 渲染使用静态 bitmask**：不引入图片解码依赖，arrow/hand/ibeam 的像素数据编译为 Rust 静态数组。
- **导出进度分段**：准备阶段占 0-10%，FFmpeg 导出占 10-99%，validation 成功后报 100%。

#### 5. 立即执行清单

1. 用户审阅计划，选择执行方式（Subagent-Driven 或 Inline）
2. 按 Phase 1 → Phase 2 → Phase 3 顺序执行 16 个 Task
3. Phase 1 Task 3 完成后，在真实设备上验证 `SCDisplay.frame()` 返回值

#### 6. 当前报错/阻碍

无阻塞报错。

待确认项：

- SCDisplay 的 `pointPixelScale` 不在 `SCDisplay` 上，需确认获取方式
- `CGEventGetLocation()` Y 轴方向需实测
- 手形和 I-beam 的 glyph 像素数据需设计

---

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
