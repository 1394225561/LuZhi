# Phase 6 FFmpeg 实现任务评估

> 来源：`docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` Important 5 & Important 6
>
> 创建时间：2026-05-30

---

## 背景

Phase 6 Code Review 发现两个未完成的 Important 级别问题，它们存在依赖关系：

- **Important 6**（根因）：`FfmpegRecordingWriter` / `FfmpegTrimExporter` 仍是骨架
- **Important 5**（依赖 Important 6）：缺少 FFmpeg artifact-level integration tests

---

## Important 6：FFmpeg Writer/Exporter 骨架实现

### 当前状态

| 组件 | 文件 | 行为 |
|------|------|------|
| `FfmpegRecordingWriter::push_video()` | `src-tauri/src/media/ffmpeg_writer.rs:31` | 只递增计数，不写文件 |
| `FfmpegRecordingWriter::push_audio()` | `src-tauri/src/media/ffmpeg_writer.rs:44` | 只递增计数，不写文件 |
| `FfmpegRecordingWriter::finish()` | `src-tauri/src/media/ffmpeg_writer.rs` | 返回 `output_path: None`，不创建文件 |
| `FfmpegTrimExporter::export()` | `src-tauri/src/media/trim_exporter.rs:120` | 做少量 validation 后返回 `ExportFailed("实现需补齐")` |

### 完成所需工作

1. **FFmpeg C API binding 实现**
   - 视频编码（H.264 via libx264）
   - 音频编码（AAC via libfdk_aac 或 native aac）
   - Muxing（MP4 container）
   - 时间戳对齐与 A/V sync

2. **Native Safety 审查**（项目规则强制要求）
   - `跨平台底层 API 的底层安全调用，AI 生成后必须由人工逐行审查内存安全与并发安全`
   - 涉及 FFmpeg buffer 生命周期、指针传递、错误处理路径

3. **环境依赖**
   - `pkg-config` + FFmpeg development headers（libavcodec, libavformat, libavutil, libswscale, libswresample）
   - macOS: `brew install ffmpeg` 或 `brew install ffmpeg@6`
   - Windows: vcpkg 或预编译 FFmpeg SDK

### 验收标准

- `FfmpegRecordingWriter::finish()` 返回 `Some(output_path)` 前，文件必须存在、非空、含视频/音频 stream、duration > 0
- `FfmpegTrimExporter::export()` 返回 `Ok` 前，输出文件必须通过 binding inspection（ffprobe 验证）

---

## Important 5：FFmpeg Integration Tests

### 当前状态

- Phase 6 plan 明确要求创建 `src-tauri/tests/ffmpeg_export.rs`
- 当前仓库无该文件
- `ffmpeg_test_support.rs` 存在，但没有 artifact-level test 使用它验证真实 exporter

### Plan 要求的测试场景

| 测试 | 场景 |
|------|------|
| `ffmpeg_exporter_rejects_missing_source` | 缺少 source artifact 时返回明确错误 |
| `ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source` | 三种 preset（Bilibili 16:9、抖音 9:16、小红书 1:1）导出 |
| `ffmpeg_exporter_applies_cut_timeline_and_preserves_original` | cut timeline 裁剪 + 原始文件保留 |
| `ffmpeg_exporter_cancel_removes_partial_output_and_preserves_original` | 取消时清理 partial output |
| artifact inspection | dimensions、duration、streams、file size |
| intermediate progress callback | 进度回调触发 |

### 额外问题

`create_synthetic_source_artifact()` 当前调用 `FfmpegRecordingWriter` 后直接 `writer.finish()?; Ok(())`，但 `FfmpegRecordingWriter::finish()` 返回 `output_path: None` 且不创建文件。因此该 helper 在 FFmpeg feature 可编译时也可能"成功返回但没有真实 source artifact"。

### 验收标准

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_rejects_missing_source
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_applies_cut_timeline_and_preserves_original
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_cancel_removes_partial_output_and_preserves_original
```

---

## 依赖关系

```
Important 6 (FFmpeg Writer/Exporter 实现)
    └── Important 5 (Integration Tests)
```

- Important 5 的测试依赖 Important 6 的真实实现
- Important 6 的实现需要 Native Safety 人工审查

---

## 风险评估

| 风险 | 等级 | 说明 |
|------|------|------|
| FFmpeg API 复杂度 | 高 | 编码 + muxing 涉及多个 AVFormatContext/AVCodecContext 生命周期管理 |
| 内存安全 | 高 | FFmpeg buffer 必须手动管理，use-after-free / double-free 风险 |
| A/V 同步 | 中 | 系统音频与麦克风混音后的时间戳对齐 |
| 环境依赖 | 中 | FFmpeg dev headers 安装可能因平台而异 |

---

## 建议执行顺序

1. **环境准备**：确认 FFmpeg dev libraries 可用（`pkg-config --cflags libavcodec`）
2. **Important 6**：实现 `FfmpegRecordingWriter` + `FfmpegTrimExporter`
3. **Native Safety 审查**：人工逐行审查 FFmpeg 相关 unsafe 代码
4. **Important 5**：创建 `src-tauri/tests/ffmpeg_export.rs` 并通过所有测试
5. **文档更新**：更新 HANDOFF.md 和 checklist 反映实际状态
