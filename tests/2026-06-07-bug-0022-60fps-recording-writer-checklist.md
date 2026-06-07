# 2026-06-07 BUG-0022 60fps 录制 writer 时基自测清单

## 范围

- 修复 60fps 录制结束时，source artifact 校验报“录制视频/音频时长偏差过大”的问题。
- 本轮只触碰录制 writer 的视频 fps/time base 传递、macOS 全屏录制创建 writer 的 fps 参数，以及后端 `set_capture_mode` fps allowlist 校验。
- 不改音频同步器、麦克风采集、系统音频采集、窗口录制 30fps 行为。

## Root Cause 验证

- [x] 从用户日志确认 x264 输出帧数为 `7 + 1608 = 1615`。
- [x] 计算 `1615 / 30fps = 53.8s`，对应报错中的视频 `53800ms`。
- [x] 计算 `1615 / 60fps = 26.9s`，接近报错中的音频 `28288ms`。
- [x] 代码确认全屏 SCK 采集使用 `config.fps`，但 `FfmpegRecordingWriter` 内部视频 time base、PTS rescale、尾部 padding 均硬编码 30fps。

## 自动化验证

- [x] RED：新增 `ffmpeg_writer_preserves_av_duration_at_60fps`，修复前 `FfmpegRecordingWriter::with_fps` 不存在，无法编译，证明缺少 fps-aware writer 入口。
- [x] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_writer_preserves_av_duration_at_60fps` 通过。
- [x] 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_writer -- --test-threads=1` 通过，26 tests。
- [x] Code Review Finding：`cargo test --manifest-path src-tauri/Cargo.toml --lib capture_mode_update` 通过，覆盖 `120fps` payload 在后端边界被拒绝。
- [x] Code Review Finding：`cargo test --manifest-path src-tauri/Cargo.toml --lib fullscreen_writer_settings_uses_capture_fps` 通过，覆盖 fullscreen `CaptureConfig.fps -> writer settings` 生产链路。
- [x] 空白检查：`git diff --check` 通过。
- [x] 触碰文件格式：`rustfmt --check --edition 2021 --config skip_children=true src-tauri/src/lib.rs src-tauri/src/platform/macos_service.rs src-tauri/src/media/ffmpeg_writer.rs` 通过。
- [x] 已知格式漂移：普通 `rustfmt --check --edition 2021 src-tauri/src/lib.rs src-tauri/src/platform/macos_service.rs src-tauri/src/media/ffmpeg_writer.rs` 会沿模块树报告未触碰文件 `src-tauri/src/app/error.rs`、`src-tauri/src/app/recording_library.rs` 的既有格式差异。

## 人工验证建议

- [ ] 全屏录制选择 60fps，同时开启系统音频和麦克风，录制约 25-35 秒后点击结束录制，应进入预览而不是报“视频/音频时长偏差过大”。
- [ ] 查看终端日志，x264 帧数除以 60fps 后应接近录制实际时长；artifact 校验不应再把视频时长按 30fps 放大。
- [ ] 再录一次 30fps，确认默认 30fps 路径仍能正常结束并生成可播放文件。
