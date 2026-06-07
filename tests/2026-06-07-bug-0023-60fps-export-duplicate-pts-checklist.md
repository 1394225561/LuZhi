# 2026-06-07 BUG-0023 60fps 源素材导出重复 PTS 自测清单

## 范围

- 修复 60fps 录制素材进入预览后，在导出阶段报“导出失败，请重试或检查录制素材”的问题。
- 本轮只触碰 FFmpeg trim/export 的视频输出 PTS 单调性和对应回归测试。
- 不改录制 writer、音频同步器、导出 preset fps、前端 UI 文案。

## Root Cause 验证

- [x] 用户日志确认录制 finalize 已通过：`录制音频 contract 验证通过`，writer diagnostics 中 `audio_chunks_appended=481`。
- [x] 用户日志确认失败发生在导出阶段：第二段 x264 日志后出现 `non-strictly-monotonic PTS`。
- [x] 用户日志确认 muxer 拒绝重复时间戳：`stream 0: 1024 >= 1024`。
- [x] 代码确认导出 preset 固定为 30fps，而 60fps 源帧重采样到 `1/30` encoder time base 时可能两个源帧映射到同一个输出 PTS。
- [x] 代码确认旧逻辑 `out_pts.max(last_video_out_pts)` 只保证非递减，仍允许重复 PTS 进入编码器。

## 自动化验证

- [x] RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_exports_60fps_source_to_30fps_preset_without_duplicate_pts -- --nocapture` 修复前失败，错误为 `写入视频数据包失败: Invalid argument`，并复现 `non-strictly-monotonic PTS`。
- [x] GREEN：同一测试修复后通过。
- [x] 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg trim_exporter -- --test-threads=1` 通过，8 tests。
- [x] 录制 writer 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_writer_preserves_av_duration_at_60fps` 通过。
- [x] 导出 artifact contract 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg validate_export_artifact -- --test-threads=1` 通过。
- [x] 空白检查：`git diff --check` 通过。
- [x] 触碰文件格式：`rustfmt --check --edition 2021 --config skip_children=true src-tauri/src/media/trim_exporter.rs` 通过。

## 人工验证建议

- [ ] 全屏录制选择 60fps，同时开启系统音频和麦克风，录制约 10-30 秒后停止，应进入预览。
- [ ] 在预览页导出 Bilibili / YouTube preset，应成功生成导出文件，不再出现 `non-strictly-monotonic PTS` 或 `non monotonically increasing dts`。
- [ ] 回放导出文件，确认视频时长接近实际录制时长，音频存在且可播放。
