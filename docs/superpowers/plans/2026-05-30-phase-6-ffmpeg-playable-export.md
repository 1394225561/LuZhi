# Phase 6 FFmpeg Playable Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成 Phase 6 review 中 Important 6 和 Important 5：让 FFmpeg writer/exporter 产出真实可播放 artifact，并用 feature-gated integration tests 验证 source artifact、三种 preset export、cut timeline、progress、cancel cleanup 和原始文件保留。

**Architecture:** FFmpeg 仅在 `ffmpeg` feature 下启用，通过 `ffmpeg-next` binding / C API 路径工作，不调用 `ffmpeg` 或 `ffprobe` CLI。录制 source writer 使用内部编码 worker 和 byte-budgeted bounded queue，避免把 FFmpeg 编码压力推回 ScreenCaptureKit callback 或阻塞捕获主链路；导出 exporter 在 blocking worker 中运行，使用结构化 `TrimExportRequest`、fixed preset scale policy、cut timeline keep segments、cancel token 和 progress callback。生产 artifact inspection 放在 `media::ffmpeg_common`，测试 helper 只复用生产 inspection，不反向依赖 `test_support`。

**Tech Stack:** Rust 2021、Tauri 2、`ffmpeg-next` optional feature、ScreenCaptureKit/cpal capture pipeline、Rust integration tests、现有 `RecordingWriter` / `TrimExporter` trait boundary。

---

## Scope

本计划只覆盖 Phase 6 export 的 FFmpeg 新功能闭环：

- 完成 `FfmpegRecordingWriter` 真实写入 original recording artifact。
- 完成 `FfmpegTrimExporter` 真实导出三种固定 preset。
- 新增 `src-tauri/tests/ffmpeg_export.rs` artifact-level tests。
- 修正 `src-tauri/src/test_support/ffmpeg_helpers.rs`，避免 synthetic helper 在未产出真实文件时误报成功。
- 在 `ffmpeg` feature + Native Safety review 通过后，把 production writer/exporter 接入产品路径。
- 更新 `tests/phase-6-w11-w12-checklist.md`、`HANDOFF.md` 和 FFmpeg artifact evidence。

不纳入本计划：

- 平台发布 API、模板系统、云端转码。
- 服务端激活协议。
- 修改 `Cargo.toml` 核心依赖版本。
- 新增非必要第三方依赖。

需要执行前确认的实现口径：

- 该计划要求导出 output 至少应用 cut timeline 和 fixed scale/crop policy。
- `effect_timeline_path` 继续通过 request boundary 传入 exporter；cursor timeline 构建失败或 cursor metadata 不存在时，基础 playable export 必须继续执行并传入 `None`。若本轮不实现 cursor overlay compositing，文档和 checklist 必须明确“可播放导出已完成，光标视觉重绘未完成”，不能宣称完整 AI 美化导出。
- 无系统音频且无麦克风时，writer/exporter 必须生成 silent AAC track，保持 MP4 artifact 始终有 audio stream；不能让“无声录屏”变成不可导出。
- 非 `ffmpeg` 构建中的 product command 永远返回明确 Gate summary/error 和 `output_path: None`，不得调用 `MockTrimExporter` 伪造成功导出。
- `ffmpeg` feature 默认仍关闭；只有 FFmpeg dev libraries 可用且 Native Safety Gate 通过后，才允许在产品构建中启用。

## File Structure

Create:

- `src-tauri/tests/ffmpeg_export.rs`
  - Feature-gated integration tests for writer/exporter artifact behavior.
- `src-tauri/src/media/ffmpeg_common.rs`
  - Feature-gated production helpers: FFmpeg init, timestamp conversion, progress clamp, partial output cleanup, artifact inspection, timestamp mapping, packet rescale, and audio slicing helpers.

Modify:

- `src-tauri/src/media/ffmpeg_writer.rs`
  - Replace skeleton counter-only behavior with nonblocking source artifact writer.
- `src-tauri/src/media/trim_exporter.rs`
  - Replace `FfmpegTrimExporter` skeleton error with real binding-based export.
- `src-tauri/src/test_support/ffmpeg_helpers.rs`
  - Make synthetic source generation fail unless a real playable artifact exists.
- `src-tauri/src/media/mod.rs`
  - Export `ffmpeg_common` under `#[cfg(feature = "ffmpeg")]`.
- `src-tauri/src/platform/macos_service.rs`
  - Select `FfmpegRecordingWriter` under `ffmpeg` feature only after writer tests pass and Native Safety review is recorded.
- `src-tauri/src/lib.rs`
  - Ensure FFmpeg export work runs through `spawn_blocking`, keeps non-FFmpeg Gate behavior, and never leaves stale cancel state.
- `tests/phase-6-w11-w12-checklist.md`
  - Record exact automated and manual FFmpeg evidence.
- `HANDOFF.md`
  - Update final status only after verification.
- `reference/tasks/phase-6-ffmpeg-implementation.md`
  - Optional: mark this implementation plan as the authoritative execution plan for Important 5/6.

## Success Criteria

- `FfmpegRecordingWriter::finish()` returns `Some(output_path)` only when the file exists, is non-empty, has video stream, has an audio stream or generated silent AAC track, and has duration > 0.
- `FfmpegTrimExporter::export()` returns `Ok` only when output exists, is non-empty, has expected dimensions for the selected preset, has video/audio streams, and preserves the original source file.
- Cancel or export failure removes partial output and preserves source.
- Export progress emits at least one intermediate value in `1..=99` and final `100` on success.
- Missing source, source/output same path, and output validation failure return Chinese structured errors and remove only the planned output path.
- No FFmpeg CLI process is spawned.
- No raw media frame/audio stream crosses into frontend JS.
- Native Safety review covers FFmpeg context, frame, packet, scaler/filter, resampler, encoder, decoder, timestamp, thread, queue, and cleanup paths.

## Task 0: Environment and Safety Gate

**Files:**

- Modify: `tests/phase-6-w11-w12-checklist.md`

- [ ] **Step 1: Verify FFmpeg development headers**

Run:

```bash
pkg-config --libs --cflags libavformat libavcodec libavutil libswscale libswresample libavfilter
```

Expected:

- PASS: prints include/library flags for all listed FFmpeg libraries.
- BLOCKED: if `pkg-config` or FFmpeg dev headers are missing, record the exact error in `tests/phase-6-w11-w12-checklist.md` and do not mark FFmpeg tests complete.

- [ ] **Step 2: Verify feature build environment**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

Expected:

- In an environment without FFmpeg dev libraries: FAIL at dependency linking/configuration; record exact error.
- In an environment with FFmpeg dev libraries: tests compile. This step verifies environment only; real artifact red/green checks happen after Task 1 adds tests.

- [ ] **Step 3: Add Native Safety checklist section**

Append this checklist to `tests/phase-6-w11-w12-checklist.md` under Manual Gates:

```markdown
## Native Safety Gate Details

- [ ] `ffmpeg_writer.rs`: byte-budgeted queue cannot exceed the documented memory cap and `push_*` never blocks capture callback.
- [ ] `ffmpeg_writer.rs`: encoder worker joins exactly once and partial source artifact is removed on fatal error.
- [ ] `ffmpeg_writer.rs`: AVFrame/AVPacket ownership is handled by `ffmpeg-next` wrappers or explicitly released once.
- [ ] `trim_exporter.rs`: input file is never deleted, truncated, or opened for write.
- [ ] `trim_exporter.rs`: cancel/failure removes only planned output path.
- [ ] `trim_exporter.rs`: decoded timestamps are retimed monotonically after cut removal.
- [ ] `trim_exporter.rs`: missing/invalid PTS fallback, packet time-base rescale, B-frame reorder, and interleaved write ordering are documented and tested.
- [ ] `trim_exporter.rs`: audio frames crossing keep boundaries are sliced or split without A/V drift.
- [ ] `trim_exporter.rs`: decoder/filter/resampler/encoder EOF flush order is deterministic for both streams.
- [ ] `trim_exporter.rs`: scaler/filter/resampler contexts are dropped after trailer write or error.
- [ ] `lib.rs`: export runs in blocking worker and active cancel token is cleared on every return path.
```

- [ ] **Step 4: Commit environment-gate docs**

Run:

```bash
git add tests/phase-6-w11-w12-checklist.md
git commit -m "docs(export): 记录FFmpeg环境与安全门禁"
```

## Task 1: Add FFmpeg Artifact Tests First

**Files:**

- Create: `src-tauri/tests/ffmpeg_export.rs`
- Create: `src-tauri/src/media/ffmpeg_common.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/test_support/ffmpeg_helpers.rs`

- [ ] **Step 1: Add production artifact inspector**

Create `src-tauri/src/media/ffmpeg_common.rs` with production inspection helpers that tests can reuse:

```rust
use std::path::Path;

use crate::app::error::{AppError, AppResult};

pub struct MediaArtifactInspection {
    pub file_size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_nanos: u64,
    pub has_video_stream: bool,
    pub has_audio_stream: bool,
}

pub fn inspect_media_artifact(path: &Path) -> AppResult<MediaArtifactInspection> {
    let metadata = std::fs::metadata(path).map_err(|error| AppError::ExportFailed {
        reason: format!("检查媒体文件失败: {error}"),
    })?;

    let input = ffmpeg_next::format::input(path).map_err(|error| AppError::ExportFailed {
        reason: format!("打开媒体文件进行检查失败: {error}"),
    })?;

    let mut has_video_stream = false;
    let mut has_audio_stream = false;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut duration_nanos = container_duration_nanos(&input);

    for stream in input.streams() {
        let params = stream.codecpar();
        match params.medium() {
            ffmpeg_next::media::Type::Video => {
                has_video_stream = true;
                width = params.width() as u32;
                height = params.height() as u32;
            }
            ffmpeg_next::media::Type::Audio => {
                has_audio_stream = true;
            }
            _ => {}
        }

        duration_nanos = duration_nanos.max(stream_duration_nanos(&stream));
    }

    Ok(MediaArtifactInspection {
        file_size_bytes: metadata.len(),
        width,
        height,
        duration_nanos,
        has_video_stream,
        has_audio_stream,
    })
}

fn container_duration_nanos(input: &ffmpeg_next::format::context::Input) -> u64 {
    let duration = input.duration();
    if duration <= 0 {
        return 0;
    }
    (duration as u64).saturating_mul(1_000)
}

fn stream_duration_nanos(stream: &ffmpeg_next::Stream<'_>) -> u64 {
    let duration = stream.duration();
    if duration <= 0 {
        return 0;
    }
    let time_base = stream.time_base();
    let numerator = time_base.numerator().max(0) as u64;
    let denominator = time_base.denominator().max(1) as u64;
    (duration as u64)
        .saturating_mul(numerator)
        .saturating_mul(1_000_000_000)
        / denominator
}
```

Modify `src-tauri/src/media/mod.rs`:

```rust
#[cfg(feature = "ffmpeg")]
pub mod ffmpeg_common;
```

Rules:

- Production writer/exporter use this inspector, never `crate::test_support`.
- `test_support::ffmpeg_helpers` may re-export or call this inspector.
- `ffmpeg_next::format::context::Input::duration()` is in AV_TIME_BASE units, not nanoseconds; convert to nanoseconds.
- If container duration is missing, use stream duration and time base.

- [ ] **Step 2: Harden synthetic source helper**

Modify `create_synthetic_source_artifact()` in `src-tauri/src/test_support/ffmpeg_helpers.rs` so it validates the writer result before returning `Ok(())`:

```rust
    let result = writer.finish()?;
    let returned = result.output_path.as_deref().ok_or_else(|| {
        AppError::RecordingWriteFailed {
            reason: "FFmpeg synthetic source writer did not return an output path".to_string(),
        }
    })?;
    if std::path::Path::new(returned) != path {
        return Err(AppError::RecordingWriteFailed {
            reason: format!(
                "FFmpeg synthetic source writer returned unexpected path: {}",
                returned
            ),
        });
    }

    let inspected = crate::media::ffmpeg_common::inspect_media_artifact(path)?;
    if inspected.file_size_bytes == 0
        || inspected.duration_nanos == 0
        || !inspected.has_video_stream
        || !inspected.has_audio_stream
    {
        return Err(AppError::RecordingWriteFailed {
            reason: "FFmpeg synthetic source artifact is not playable video+audio media".to_string(),
        });
    }
    Ok(())
```

Expected before Task 2:

- This helper fails because the current `FfmpegRecordingWriter` returns `output_path: None`.

- [ ] **Step 3: Fix FFmpeg test frame helpers**

Modify `synthetic_video_frame_at()` in `src-tauri/src/test_support/ffmpeg_helpers.rs`:

```rust
VideoFrame {
    timestamp: MediaTimestamp::from_nanos(timestamp_nanos),
    width,
    height,
    stride_bytes: stride,
    pixel_format: PixelFormat::Bgra8,
    buffer: FrameBuffer::Owned(Arc::from(buffer.into_boxed_slice())),
}
```

Rules:

- `VideoFrame::stride_bytes` is `usize`; do not cast it to `u32`.
- Keep `test_video_frame_at()` small for unit tests, but synthetic source helpers should use requested dimensions and interleaved timestamps.

- [ ] **Step 4: Create feature-gated integration test file**

Create `src-tauri/tests/ffmpeg_export.rs`:

```rust
#![cfg(feature = "ffmpeg")]

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use luzhi_lib::core::cut::{CutReason, CutSegment, CutTimeline, KeepSegment};
use luzhi_lib::core::frame::MediaTimestamp;
use luzhi_lib::media::export_presets::ExportPreset;
use luzhi_lib::media::ffmpeg_common::inspect_media_artifact;
use luzhi_lib::media::ffmpeg_writer::FfmpegRecordingWriter;
use luzhi_lib::media::recording_writer::RecordingWriter;
use luzhi_lib::media::trim_exporter::{
    ExportProgressReporter, FfmpegTrimExporter, TrimExportRequest, TrimExporter,
};
use luzhi_lib::test_support::ffmpeg_helpers::{
    create_synthetic_source_artifact, test_audio_chunk_at, test_video_frame_at,
    unique_media_path,
};

#[test]
fn ffmpeg_recording_writer_creates_playable_source_artifact() {
    let output = unique_media_path("recording-source", "mp4");
    let mut writer = FfmpegRecordingWriter::new(output.clone()).unwrap();

    for step in 0..100 {
        let audio_ts = step * 20_000_000;
        writer.push_audio(test_audio_chunk_at(audio_ts)).unwrap();

        if step % 2 == 0 {
            let frame_index = step / 2;
            writer
                .push_video(test_video_frame_at(frame_index * 33_333_333))
                .unwrap();
        }
    }

    let result = writer.finish().unwrap();
    assert_eq!(result.output_path.as_deref(), Some(output.to_string_lossy().as_ref()));

    let inspected = inspect_media_artifact(&output).unwrap();
    assert!(inspected.file_size_bytes > 0);
    assert!(inspected.duration_nanos > 0);
    assert!(inspected.has_video_stream);
    assert!(inspected.has_audio_stream);

    let _ = std::fs::remove_file(output);
}

#[test]
fn ffmpeg_recording_writer_video_before_audio_stays_bounded() {
    let output = unique_media_path("video-before-audio", "mp4");
    let mut writer = FfmpegRecordingWriter::new(output.clone()).unwrap();

    writer.push_video(test_video_frame_at(0)).unwrap();
    writer.push_video(test_video_frame_at(33_333_333)).unwrap();
    writer.push_audio(test_audio_chunk_at(0)).unwrap();
    writer.push_audio(test_audio_chunk_at(20_000_000)).unwrap();

    let result = writer.finish().unwrap();
    assert_eq!(result.output_path.as_deref(), Some(output.to_string_lossy().as_ref()));
    let inspected = inspect_media_artifact(&output).unwrap();
    assert!(inspected.has_video_stream);
    assert!(inspected.has_audio_stream);

    let _ = std::fs::remove_file(output);
}

#[test]
fn ffmpeg_recording_writer_without_audio_generates_silent_track() {
    let output = unique_media_path("silent-source", "mp4");
    let mut writer = FfmpegRecordingWriter::new(output.clone()).unwrap();

    for frame_index in 0..30 {
        writer
            .push_video(test_video_frame_at(frame_index * 33_333_333))
            .unwrap();
    }

    let result = writer.finish().unwrap();
    assert_eq!(result.output_path.as_deref(), Some(output.to_string_lossy().as_ref()));
    let inspected = inspect_media_artifact(&output).unwrap();
    assert!(inspected.has_video_stream);
    assert!(inspected.has_audio_stream, "silent recording must still have AAC track");

    let _ = std::fs::remove_file(output);
}

#[test]
fn ffmpeg_exporter_rejects_missing_source() {
    let mut exporter = FfmpegTrimExporter;
    let request = TrimExportRequest {
        input_path: unique_media_path("missing-source", "mp4"),
        output_path: unique_media_path("missing-output", "mp4"),
        preset: ExportPreset::Bilibili,
        cut_timeline: CutTimeline::empty(1_000_000_000),
        effect_timeline_path: None,
        cancel_token: Arc::new(AtomicBool::new(false)),
        progress: None,
    };

    let error = exporter.export(request).unwrap_err().to_string();
    assert!(error.contains("源文件"));
}

#[test]
fn ffmpeg_exporter_rejects_same_input_output_path() {
    let source = unique_media_path("same-path-source", "mp4");
    create_synthetic_source_artifact(&source, 1920, 1080, 1_000_000_000).unwrap();
    let mut exporter = FfmpegTrimExporter;

    let error = exporter
        .export(TrimExportRequest {
            input_path: source.clone(),
            output_path: source.clone(),
            preset: ExportPreset::Bilibili,
            cut_timeline: CutTimeline::empty(1_000_000_000),
            effect_timeline_path: None,
            cancel_token: Arc::new(AtomicBool::new(false)),
            progress: None,
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("覆盖原始录制文件"));
    assert!(source.exists());
    let _ = std::fs::remove_file(source);
}

#[test]
fn ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source() {
    let source = unique_media_path("source-all-presets", "mp4");
    create_synthetic_source_artifact(&source, 1920, 1080, 4_000_000_000).unwrap();
    let source_inspection = inspect_media_artifact(&source).unwrap();

    for preset in [
        ExportPreset::Bilibili,
        ExportPreset::Douyin,
        ExportPreset::Xiaohongshu,
    ] {
        let output = unique_media_path(preset.spec().id, "mp4");
        let observed_progress = Arc::new(Mutex::new(Vec::<u8>::new()));
        let progress_for_callback = observed_progress.clone();
        let progress = ExportProgressReporter::new(Arc::new(move |value| {
            progress_for_callback.lock().unwrap().push(value);
        }));

        let mut exporter = FfmpegTrimExporter;
        let result = exporter
            .export(TrimExportRequest {
                input_path: source.clone(),
                output_path: output.clone(),
                preset,
                cut_timeline: CutTimeline::empty(source_inspection.duration_nanos),
                effect_timeline_path: None,
                cancel_token: Arc::new(AtomicBool::new(false)),
                progress: Some(progress),
            })
            .unwrap();

        assert_eq!(result.output_path, output);
        assert!(source.exists(), "export must preserve the original source artifact");
        let inspection = inspect_media_artifact(&output).unwrap();
        assert_eq!(inspection.width, preset.spec().width);
        assert_eq!(inspection.height, preset.spec().height);
        assert!(inspection.has_video_stream);
        assert!(inspection.has_audio_stream);
        assert!(inspection.file_size_bytes > 0);
        assert!(
            duration_delta_nanos(inspection.duration_nanos, source_inspection.duration_nanos)
                <= 500_000_000
        );
        assert!(observed_progress
            .lock()
            .unwrap()
            .iter()
            .any(|progress| *progress > 0 && *progress < 100));
        assert_eq!(observed_progress.lock().unwrap().last().copied(), Some(100));
        let _ = std::fs::remove_file(output);
    }

    let _ = std::fs::remove_file(source);
}

#[test]
fn ffmpeg_exporter_applies_cut_timeline_and_preserves_original() {
    let source = unique_media_path("source-trimmed", "mp4");
    create_synthetic_source_artifact(&source, 1920, 1080, 8_000_000_000).unwrap();
    let output = unique_media_path("trimmed-output", "mp4");
    let timeline = CutTimeline {
        duration_nanos: 8_000_000_000,
        cuts: vec![CutSegment {
            start: MediaTimestamp::from_nanos(2_000_000_000),
            end: MediaTimestamp::from_nanos(6_000_000_000),
            reason: CutReason::SilentAndStill,
            mean_audio_rms: 0.001,
            mean_visual_change: 0.001,
        }],
        keeps: vec![
            KeepSegment {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(2_000_000_000),
            },
            KeepSegment {
                start: MediaTimestamp::from_nanos(6_000_000_000),
                end: MediaTimestamp::from_nanos(8_000_000_000),
            },
        ],
        total_cut_nanos: 4_000_000_000,
    };

    let mut exporter = FfmpegTrimExporter;
    exporter
        .export(TrimExportRequest {
            input_path: source.clone(),
            output_path: output.clone(),
            preset: ExportPreset::Bilibili,
            cut_timeline: timeline,
            effect_timeline_path: None,
            cancel_token: Arc::new(AtomicBool::new(false)),
            progress: None,
        })
        .unwrap();

    assert!(source.exists());
    let source_inspection = inspect_media_artifact(&source).unwrap();
    let output_inspection = inspect_media_artifact(&output).unwrap();
    assert!(output_inspection.duration_nanos + 3_000_000_000 < source_inspection.duration_nanos);
    assert!(duration_delta_nanos(output_inspection.duration_nanos, 4_000_000_000) <= 700_000_000);

    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_file(output);
}

#[test]
fn ffmpeg_exporter_cancel_removes_partial_output_and_preserves_original() {
    let source = unique_media_path("source-cancel", "mp4");
    create_synthetic_source_artifact(&source, 1920, 1080, 8_000_000_000).unwrap();
    let output = unique_media_path("cancel-output", "mp4");
    let cancel_token = Arc::new(AtomicBool::new(false));
    let token_for_callback = cancel_token.clone();
    let progress = ExportProgressReporter::new(Arc::new(move |value| {
        if value > 0 {
            token_for_callback.store(true, Ordering::Relaxed);
        }
    }));

    let mut exporter = FfmpegTrimExporter;
    let result = exporter.export(TrimExportRequest {
        input_path: source.clone(),
        output_path: output.clone(),
        preset: ExportPreset::Bilibili,
        cut_timeline: CutTimeline::empty(8_000_000_000),
        effect_timeline_path: None,
        cancel_token,
        progress: Some(progress),
    });

    assert!(result.is_err());
    assert!(source.exists());
    assert!(!output.exists(), "cancel must remove partial output");

    let _ = std::fs::remove_file(source);
}

#[test]
fn rust_code_does_not_shell_out_to_ffmpeg_or_ffprobe() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![root];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(
                !content.contains("std::process::Command"),
                "{} must not spawn FFmpeg CLI",
                path.display()
            );
            assert!(
                !content.contains("Command::new(\"ffmpeg\")")
                    && !content.contains("Command::new(\"ffprobe\")")
                    && !content.contains("process::Command::new(\"ffmpeg\")")
                    && !content.contains("process::Command::new(\"ffprobe\")"),
                "{} must not launch FFmpeg CLI binary names",
                path.display()
            );
        }
    }
}

fn duration_delta_nanos(left: u64, right: u64) -> u64 {
    left.max(right) - left.min(right)
}
```

- [ ] **Step 5: Run red tests without committing failure state**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_creates_playable_source_artifact
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_video_before_audio_stays_bounded
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_without_audio_generates_silent_track
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_rejects_missing_source
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_rejects_same_input_output_path
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_applies_cut_timeline_and_preserves_original
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_cancel_removes_partial_output_and_preserves_original
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg rust_code_does_not_shell_out_to_ffmpeg_or_ffprobe
```

Expected:

- Missing-source rejection may pass.
- Writer/source/export tests fail with current skeleton behavior or with FFmpeg environment error.
- Do not commit while these tests are red. Commit the tests together with the writer/exporter implementation tasks that make them pass, or keep them as local work-in-progress during TDD.

- [ ] **Step 6: Keep red tests uncommitted**

Do not create a commit in Task 1. The first commit containing `src-tauri/tests/ffmpeg_export.rs` happens after Task 2 or Task 4 makes the relevant test subset green.

## Task 2: Implement Nonblocking FFmpeg Source Writer

**Files:**

- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/media/ffmpeg_writer.rs`
- Test: `src-tauri/tests/ffmpeg_export.rs`

- [ ] **Step 1: Extend shared FFmpeg helpers**

Extend the `src-tauri/src/media/ffmpeg_common.rs` created in Task 1:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

pub fn init_ffmpeg_for_export() -> AppResult<()> {
    ffmpeg_next::init().map_err(|error| AppError::ExportFailed {
        reason: format!("初始化 FFmpeg 失败: {error}"),
    })
}

pub fn init_ffmpeg_for_recording() -> AppResult<()> {
    ffmpeg_next::init().map_err(|error| AppError::RecordingWriteFailed {
        reason: format!("初始化 FFmpeg 失败: {error}"),
    })
}

pub fn remove_partial_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

pub fn check_cancelled(token: &AtomicBool) -> AppResult<()> {
    if token.load(Ordering::Relaxed) {
        return Err(AppError::ExportCancelled);
    }
    Ok(())
}

pub fn report_progress(
    progress: Option<&crate::media::trim_exporter::ExportProgressReporter>,
    processed_nanos: u64,
    total_nanos: u64,
) {
    if let Some(progress) = progress {
        if total_nanos == 0 {
            progress.report(1);
            return;
        }
        let value = ((processed_nanos.saturating_mul(98) / total_nanos).min(98) + 1) as u8;
        progress.report(value);
    }
}
```

- [ ] **Step 2: Replace writer fields with worker state**

In `src-tauri/src/media/ffmpeg_writer.rs`, replace counter-only fields with a worker-backed writer:

```rust
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

const FFMPEG_WRITER_QUEUE_CAPACITY: usize = 24;
const FFMPEG_WRITER_MAX_QUEUED_BYTES: usize = 192 * 1024 * 1024;
const FFMPEG_WRITER_MAX_BOOTSTRAP_VIDEO_FRAMES: usize = 4;

enum FfmpegWriterCommand {
    Video(VideoFrameRef),
    Audio(MixedAudioChunk),
    Finish,
}

struct FfmpegQueuedCommand {
    bytes: usize,
    command: FfmpegWriterCommand,
}

struct FfmpegWriterWorkerResult {
    frame_count: u64,
    mixed_audio_chunk_count: u64,
    duration_nanos: u64,
}

pub struct FfmpegRecordingWriter {
    output_path: PathBuf,
    sender: Option<SyncSender<FfmpegQueuedCommand>>,
    worker: Option<JoinHandle<AppResult<FfmpegWriterWorkerResult>>>,
    frame_count: u64,
    mixed_audio_chunk_count: u64,
    queued_bytes: Arc<AtomicUsize>,
    send_failed: bool,
}
```

The writer must satisfy these constraints:

- `push_video()` and `push_audio()` estimate queued bytes before `try_send`; they return `RecordingWriteFailed` if the byte budget, item capacity, or receiver connection would be exceeded.
- `push_*()` must not encode directly on the caller thread.
- `finish()` sends `Finish`, joins the worker exactly once, and removes partial output on any error.
- The worker owns all FFmpeg contexts on the encoder thread.
- A single queued 1080p BGRA frame is about 8MB. `FFMPEG_WRITER_MAX_QUEUED_BYTES = 192MB` caps video buffering to roughly 24 1080p frames, before FFmpeg-internal buffers. Any larger backlog is treated as a fatal writer error so the capture path never grows memory unbounded.
- Future 4K reserve behavior must be measured manually before enabling 4K export by default; 4K frames hit the byte budget after far fewer queued frames.

- [ ] **Step 3: Implement nonblocking send methods**

In `impl RecordingWriter for FfmpegRecordingWriter`:

```rust
fn push_video(&mut self, frame: VideoFrameRef) -> AppResult<()> {
    let estimated_bytes = estimate_video_frame_bytes(&frame);
    self.reserve_queue_bytes(estimated_bytes)?;
    let queued = FfmpegQueuedCommand {
        bytes: estimated_bytes,
        command: FfmpegWriterCommand::Video(frame),
    };
    let send_result = match self.sender.as_ref() {
        Some(sender) => sender.try_send(queued),
        None => {
            self.release_queue_bytes(estimated_bytes);
            return Err(AppError::RecordingWriteFailed {
                reason: "FFmpeg 写入器已完成，不能继续写入视频帧".to_string(),
            });
        }
    };
    match send_result {
        Ok(()) => {
            self.frame_count += 1;
            Ok(())
        }
        Err(error) => {
            self.release_queue_bytes(estimated_bytes);
            self.send_failed = true;
            Err(AppError::RecordingWriteFailed {
                reason: match error {
                    TrySendError::Full(_) => "FFmpeg 写入队列已满，录制产物不完整".to_string(),
                    TrySendError::Disconnected(_) => "FFmpeg 写入线程已断开".to_string(),
                },
            })
        }
    }
}

fn push_audio(&mut self, chunk: MixedAudioChunk) -> AppResult<()> {
    let estimated_bytes = estimate_audio_chunk_bytes(&chunk);
    self.reserve_queue_bytes(estimated_bytes)?;
    let queued = FfmpegQueuedCommand {
        bytes: estimated_bytes,
        command: FfmpegWriterCommand::Audio(chunk),
    };
    let send_result = match self.sender.as_ref() {
        Some(sender) => sender.try_send(queued),
        None => {
            self.release_queue_bytes(estimated_bytes);
            return Err(AppError::RecordingWriteFailed {
                reason: "FFmpeg 写入器已完成，不能继续写入音频".to_string(),
            });
        }
    };
    match send_result {
        Ok(()) => {
            self.mixed_audio_chunk_count += 1;
            Ok(())
        }
        Err(error) => {
            self.release_queue_bytes(estimated_bytes);
            self.send_failed = true;
            Err(AppError::RecordingWriteFailed {
                reason: match error {
                    TrySendError::Full(_) => "FFmpeg 写入队列已满，录制产物不完整".to_string(),
                    TrySendError::Disconnected(_) => "FFmpeg 写入线程已断开".to_string(),
                },
            })
        }
    }
}
```

Add helper methods:

```rust
fn estimate_video_frame_bytes(frame: &VideoFrameRef) -> usize {
    frame.stride_bytes.saturating_mul(frame.height as usize)
}

fn estimate_audio_chunk_bytes(chunk: &MixedAudioChunk) -> usize {
    chunk.samples.len().saturating_mul(std::mem::size_of::<f32>())
}

impl FfmpegRecordingWriter {
    fn reserve_queue_bytes(&mut self, bytes: usize) -> AppResult<()> {
        loop {
            let previous = self.queued_bytes.load(Ordering::Relaxed);
            let next = previous.saturating_add(bytes);
            if next > FFMPEG_WRITER_MAX_QUEUED_BYTES {
                self.send_failed = true;
                return Err(AppError::RecordingWriteFailed {
                    reason: format!(
                        "FFmpeg 写入队列超过内存预算: {} > {} bytes",
                        next, FFMPEG_WRITER_MAX_QUEUED_BYTES
                    ),
                });
            }
            if self
                .queued_bytes
                .compare_exchange_weak(previous, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(());
            }
        }
    }

    fn release_queue_bytes(&mut self, bytes: usize) {
        self.queued_bytes.fetch_sub(bytes, Ordering::Relaxed);
    }
}
```

Worker consumption must release byte budget:

```rust
fn release_queued_bytes(counter: &AtomicUsize, bytes: usize) {
    counter.fetch_sub(bytes, Ordering::Relaxed);
}
```

The worker receives `FfmpegQueuedCommand`, processes `queued.command`, and calls `release_queued_bytes(&queued_bytes, queued.bytes)` in a small scope guard or immediately after moving media into FFmpeg-owned buffers. Do not leave queued bytes write-only; otherwise long recordings will fail after the first 192MB even when the worker is keeping up.

- [ ] **Step 4: Implement worker lifecycle**

`FfmpegRecordingWriter::new(output_path)` should:

```rust
pub fn new(output_path: PathBuf) -> AppResult<Self> {
    crate::media::ffmpeg_common::init_ffmpeg_for_recording()?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| AppError::RecordingWriteFailed {
            reason: format!("创建录制输出目录失败: {error}"),
        })?;
    }

    let (sender, receiver) = sync_channel(FFMPEG_WRITER_QUEUE_CAPACITY);
    let worker_output_path = output_path.clone();
    let queued_bytes = Arc::new(AtomicUsize::new(0));
    let worker_queued_bytes = queued_bytes.clone();
    let worker = thread::spawn(move || {
        run_ffmpeg_recording_worker(worker_output_path, receiver, worker_queued_bytes)
    });

    Ok(Self {
        output_path,
        sender: Some(sender),
        worker: Some(worker),
        frame_count: 0,
        mixed_audio_chunk_count: 0,
        queued_bytes,
        send_failed: false,
    })
}
```

`finish()` should:

```rust
fn finish(&mut self) -> AppResult<RecordingResult> {
    if let Some(sender) = self.sender.take() {
        let _ = sender.try_send(FfmpegQueuedCommand {
            bytes: 0,
            command: FfmpegWriterCommand::Finish,
        });
    }

    let worker = self.worker.take().ok_or_else(|| AppError::RecordingWriteFailed {
        reason: "FFmpeg 写入线程已结束".to_string(),
    })?;
    let worker_result = worker.join().map_err(|_| AppError::RecordingWriteFailed {
        reason: "FFmpeg 写入线程异常终止".to_string(),
    })?;

    match worker_result {
        Ok(result) if !self.send_failed => Ok(RecordingResult {
            duration_secs: result.duration_nanos / 1_000_000_000,
            frame_count: result.frame_count,
            mixed_audio_chunk_count: result.mixed_audio_chunk_count,
            output_path: Some(self.output_path.to_string_lossy().to_string()),
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
        }),
        Ok(_) => {
            crate::media::ffmpeg_common::remove_partial_file(&self.output_path);
            Err(AppError::RecordingWriteFailed {
                reason: "FFmpeg 写入队列曾失败，录制产物已清理".to_string(),
            })
        }
        Err(error) => {
            crate::media::ffmpeg_common::remove_partial_file(&self.output_path);
            Err(error)
        }
    }
}
```

If `Finish` cannot be enqueued because the bounded queue is full, dropping `sender` is intentional: the worker must drain already queued media and treat channel disconnect as a finish signal.

- [ ] **Step 5: Implement encoder worker**

Implement `run_ffmpeg_recording_worker(output_path, receiver)` with these exact behavior rules:

- Buffer at most `FFMPEG_WRITER_MAX_BOOTSTRAP_VIDEO_FRAMES` before audio metadata arrives. If audio arrives later, encode those bootstrap frames normally. If no audio arrives by finish, generate a silent AAC track covering the video duration.
- Return `RecordingWriteFailed` if the stream lacks video frames.
- Open MP4 output context once stream metadata is known.
- Video input is `PixelFormat::Bgra8`; encode H.264 at the captured frame size and 30fps time base.
- Audio input is interleaved `f32`; resample to encoder-supported AAC sample format and encode AAC.
- Silent audio track uses the recording sample-rate default `48_000`, 2 channels, and zero samples long enough to cover the video duration.
- Use frame timestamps converted from nanoseconds to stream time base.
- Flush video and audio encoders before writing trailer.
- After trailer write, call `crate::media::ffmpeg_common::inspect_media_artifact(&output_path)` and fail if file size, stream presence, or duration is invalid.

Implementation notes for FFmpeg binding usage:

```rust
// Video encoder target:
// codec: libx264 when available, otherwise H264 encoder from FFmpeg
// pixel format: YUV420P
// time_base: 1/30
// bit_rate: 8_000_000

// Audio encoder target:
// codec: AAC
// sample_rate: chunk.sample_rate, default 48_000
// channels: chunk.channels, default 2
// bit_rate: 192_000
```

No `std::process::Command`, shell string, `ffmpeg`, or `ffprobe` call is allowed.

- [ ] **Step 6: Run writer artifact test**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_creates_playable_source_artifact
```

Expected:

- PASS in FFmpeg-enabled environment.
- Output artifact inspection confirms file size > 0, duration > 0, video stream, and audio stream.

- [ ] **Step 7: Commit source writer**

Run:

```bash
git add src-tauri/src/media/ffmpeg_common.rs src-tauri/src/media/mod.rs src-tauri/src/media/ffmpeg_writer.rs src-tauri/tests/ffmpeg_export.rs src-tauri/src/test_support/ffmpeg_helpers.rs
git commit -m "feat(record): 实现FFmpeg原始录制产物写入"
```

## Task 3: Wire Source Artifact Writer Behind FFmpeg Feature

**Files:**

- Modify: `src-tauri/src/platform/macos_service.rs`
- Test: existing inline Rust tests and `src-tauri/tests/ffmpeg_export.rs`

- [ ] **Step 1: Add feature-gated writer factory**

In `src-tauri/src/platform/macos_service.rs`, add a small writer factory near `MacRecordingService::start()`:

```rust
#[cfg(feature = "ffmpeg")]
fn production_recording_writer() -> AppResult<Box<dyn RecordingWriter>> {
    Ok(Box::new(crate::media::ffmpeg_writer::FfmpegRecordingWriter::new(
        recording_output_path(),
    )?))
}

#[cfg(not(feature = "ffmpeg"))]
fn production_recording_writer() -> AppResult<Box<dyn RecordingWriter>> {
    Ok(Box::new(CountingRecordingWriter::new(None)))
}
```

Use it in `start()`:

```rust
let writer = production_recording_writer()?;
```

Add helper:

```rust
fn recording_output_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("recording-{millis}-{seq}.mp4"))
}
```

- [ ] **Step 2: Preserve non-FFmpeg Gate behavior**

Verify non-FFmpeg builds still return explicit export Gate copy when no source artifact exists, not a fake `outputPath`.

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_video_without_source_in_non_ffmpeg_build_returns_explicit_gate_error
```

Expected:

- PASS if the test exists.
- If the test does not exist, add it before modifying behavior.

- [ ] **Step 3: Run non-FFmpeg baseline**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected:

- PASS without requiring FFmpeg dev libraries.

- [ ] **Step 4: Run FFmpeg feature recording test**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_creates_playable_source_artifact
```

Expected:

- PASS in FFmpeg-enabled environment.

- [ ] **Step 5: Commit product writer wiring**

Run:

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "feat(record): 接入FFmpeg录制产物写入器"
```

## Task 4: Implement Playable Preset Exporter

**Files:**

- Modify: `src-tauri/src/media/trim_exporter.rs`
- Modify: `src-tauri/src/media/ffmpeg_common.rs`
- Test: `src-tauri/src/media/trim_exporter.rs`
- Test: `src-tauri/tests/ffmpeg_export.rs`

- [ ] **Step 1: Add timestamp and muxing helpers**

Extend `src-tauri/src/media/ffmpeg_common.rs` with timestamp conversion and packet mux helpers:

```rust
#[cfg(feature = "ffmpeg")]
pub fn time_base_units_to_nanos(
    units: i64,
    time_base: ffmpeg_next::Rational,
) -> AppResult<u64> {
    if units < 0 {
        return Err(AppError::ExportFailed {
            reason: format!("媒体时间戳为负数: {units}"),
        });
    }
    let numerator = time_base.numerator();
    let denominator = time_base.denominator();
    if numerator <= 0 || denominator <= 0 {
        return Err(AppError::ExportFailed {
            reason: format!("媒体 time_base 无效: {numerator}/{denominator}"),
        });
    }
    Ok((units as u64)
        .saturating_mul(numerator as u64)
        .saturating_mul(1_000_000_000)
        / denominator as u64)
}

#[cfg(feature = "ffmpeg")]
pub fn nanos_to_time_base_units(nanos: u64, time_base: ffmpeg_next::Rational) -> AppResult<i64> {
    let numerator = time_base.numerator();
    let denominator = time_base.denominator();
    if numerator <= 0 || denominator <= 0 {
        return Err(AppError::ExportFailed {
            reason: format!("媒体 time_base 无效: {numerator}/{denominator}"),
        });
    }
    let units = (nanos as u128)
        .saturating_mul(denominator as u128)
        / (numerator as u128).saturating_mul(1_000_000_000);
    if units > i64::MAX as u128 {
        return Err(AppError::ExportFailed {
            reason: "媒体时间戳超过 i64 上限".to_string(),
        });
    }
    Ok(units as i64)
}

#[cfg(feature = "ffmpeg")]
pub fn prepare_encoded_packet_for_mux(
    packet: &mut ffmpeg_next::Packet,
    stream_index: usize,
    encoder_time_base: ffmpeg_next::Rational,
    output_stream_time_base: ffmpeg_next::Rational,
) {
    packet.set_stream(stream_index);
    packet.rescale_ts(encoder_time_base, output_stream_time_base);
    packet.set_position(-1);
}
```

Rules:

- Encoded packets must call `prepare_encoded_packet_for_mux()` before `write_interleaved()`.
- Packet PTS/DTS/duration are encoder time-base values until `packet.rescale_ts(...)` has run.
- Never write a packet with the decoder/input stream time base after re-encoding.

- [ ] **Step 2: Add cut timeline and decoded timestamp helpers**

In `src-tauri/src/media/trim_exporter.rs` under `#[cfg(feature = "ffmpeg")]`, add pure helper functions:

```rust
#[cfg(feature = "ffmpeg")]
fn keep_segments_or_full(timeline: &CutTimeline) -> Vec<crate::core::cut::KeepSegment> {
    if timeline.keeps.is_empty() {
        vec![crate::core::cut::KeepSegment {
            start: crate::core::frame::MediaTimestamp::from_nanos(0),
            end: crate::core::frame::MediaTimestamp::from_nanos(timeline.duration_nanos),
        }]
    } else {
        timeline.keeps.clone()
    }
}

#[cfg(feature = "ffmpeg")]
fn kept_output_timestamp_nanos(input_nanos: u64, keeps: &[crate::core::cut::KeepSegment]) -> Option<u64> {
    let mut output_offset = 0u64;
    for keep in keeps {
        let start = keep.start.nanos;
        let end = keep.end.nanos;
        if input_nanos >= start && input_nanos < end {
            return Some(output_offset + input_nanos - start);
        }
        output_offset = output_offset.saturating_add(end.saturating_sub(start));
    }
    None
}

#[cfg(feature = "ffmpeg")]
#[derive(Clone, Copy, Debug)]
struct TimestampFallbackState {
    last_input_nanos: Option<u64>,
    fallback_step_nanos: u64,
}

#[cfg(feature = "ffmpeg")]
impl TimestampFallbackState {
    fn new(fallback_step_nanos: u64) -> Self {
        Self {
            last_input_nanos: None,
            fallback_step_nanos: fallback_step_nanos.max(1),
        }
    }
}

#[cfg(feature = "ffmpeg")]
fn decoded_frame_timestamp_nanos(
    best_effort_timestamp: Option<i64>,
    pts: Option<i64>,
    input_time_base: ffmpeg_next::Rational,
    state: &mut TimestampFallbackState,
) -> AppResult<u64> {
    let timestamp = match best_effort_timestamp.or(pts) {
        Some(value) => crate::media::ffmpeg_common::time_base_units_to_nanos(value, input_time_base)?,
        None => state
            .last_input_nanos
            .map(|last| last.saturating_add(state.fallback_step_nanos))
            .unwrap_or(0),
    };

    if let Some(last) = state.last_input_nanos {
        if timestamp.saturating_add(state.fallback_step_nanos) < last {
            return Err(AppError::ExportFailed {
                reason: format!("解码时间戳倒退: {timestamp} < {last}"),
            });
        }
    }

    state.last_input_nanos = Some(timestamp.max(state.last_input_nanos.unwrap_or(0)));
    Ok(timestamp)
}

#[cfg(feature = "ffmpeg")]
#[derive(Clone, Copy, Debug, Default)]
struct MonotonicPtsState {
    last_pts: Option<i64>,
}

#[cfg(feature = "ffmpeg")]
fn enforce_monotonic_pts(candidate_pts: i64, state: &mut MonotonicPtsState) -> i64 {
    let pts = match state.last_pts {
        Some(last) if candidate_pts <= last => last.saturating_add(1),
        _ => candidate_pts,
    };
    state.last_pts = Some(pts);
    pts
}
```

Rules:

- Prefer decoded frame `timestamp()` (`best_effort_timestamp`) over `pts()` so B-frame reorder uses display order.
- If both best-effort timestamp and PTS are missing, use monotonic fallback from the previous accepted input timestamp plus frame duration. The first missing timestamp starts at `0`.
- If a decoded timestamp moves backwards by more than one fallback frame duration, return `ExportFailed`; do not silently create a corrupt timeline.
- After cut retiming, every encoded output frame must pass through `enforce_monotonic_pts()` before encoding.

- [ ] **Step 3: Add audio slicing helpers**

In `src-tauri/src/media/trim_exporter.rs` under `#[cfg(feature = "ffmpeg")]`, add audio slice planning that can split one decoded audio frame across multiple keep segments:

```rust
#[cfg(feature = "ffmpeg")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct AudioSlicePlan {
    source_sample_offset: usize,
    sample_frames: usize,
    output_start_nanos: u64,
}

#[cfg(feature = "ffmpeg")]
fn audio_slices_for_keeps(
    frame_start_nanos: u64,
    sample_rate: u32,
    sample_frames: usize,
    keeps: &[crate::core::cut::KeepSegment],
) -> Vec<AudioSlicePlan> {
    if sample_rate == 0 || sample_frames == 0 {
        return Vec::new();
    }

    let frame_duration_nanos =
        sample_frames as u64 * 1_000_000_000u64 / sample_rate as u64;
    let frame_end_nanos = frame_start_nanos.saturating_add(frame_duration_nanos);
    let mut output_offset_nanos = 0u64;
    let mut slices = Vec::new();

    for keep in keeps {
        let keep_start = keep.start.nanos;
        let keep_end = keep.end.nanos;
        let slice_start = frame_start_nanos.max(keep_start);
        let slice_end = frame_end_nanos.min(keep_end);

        if slice_start < slice_end {
            let source_sample_offset = nanos_to_sample_floor(
                slice_start.saturating_sub(frame_start_nanos),
                sample_rate,
            );
            let source_sample_end = nanos_to_sample_floor(
                slice_end.saturating_sub(frame_start_nanos),
                sample_rate,
            )
            .min(sample_frames);
            if source_sample_end > source_sample_offset {
                slices.push(AudioSlicePlan {
                    source_sample_offset,
                    sample_frames: source_sample_end - source_sample_offset,
                    output_start_nanos: output_offset_nanos + slice_start - keep_start,
                });
            }
        }

        output_offset_nanos =
            output_offset_nanos.saturating_add(keep_end.saturating_sub(keep_start));
    }

    slices
}

#[cfg(feature = "ffmpeg")]
fn nanos_to_sample_floor(delta_nanos: u64, sample_rate: u32) -> usize {
    ((delta_nanos as u128 * sample_rate as u128) / 1_000_000_000u128) as usize
}
```

Rules:

- Audio frame slicing is sample-frame based, not byte based. Multiply `source_sample_offset` and `sample_frames` by channel count only when copying interleaved sample buffers.
- A decoded audio frame that crosses a keep boundary must be split or trimmed before resampling/encoding.
- A slice in the second keep segment is retimed to start after the first keep segment duration, not at its original input timestamp.

- [ ] **Step 4: Add helper tests**

Add these unit tests under `#[cfg(all(test, feature = "ffmpeg"))]` in `src-tauri/src/media/trim_exporter.rs` or `src-tauri/src/media/ffmpeg_common.rs` as appropriate:

```rust
#[test]
fn kept_output_timestamp_second_keep_is_monotonic() {
    let keeps = vec![
        crate::core::cut::KeepSegment {
            start: crate::core::frame::MediaTimestamp::from_nanos(0),
            end: crate::core::frame::MediaTimestamp::from_nanos(2_000_000_000),
        },
        crate::core::cut::KeepSegment {
            start: crate::core::frame::MediaTimestamp::from_nanos(6_000_000_000),
            end: crate::core::frame::MediaTimestamp::from_nanos(8_000_000_000),
        },
    ];

    assert_eq!(kept_output_timestamp_nanos(1_000_000_000, &keeps), Some(1_000_000_000));
    assert_eq!(kept_output_timestamp_nanos(2_000_000_000, &keeps), None);
    assert_eq!(kept_output_timestamp_nanos(6_500_000_000, &keeps), Some(2_500_000_000));
}

#[test]
fn missing_pts_uses_monotonic_fallback() {
    let mut state = TimestampFallbackState::new(33_333_333);
    let time_base = ffmpeg_next::Rational(1, 1_000_000_000);

    assert_eq!(
        decoded_frame_timestamp_nanos(Some(10_000_000), None, time_base, &mut state).unwrap(),
        10_000_000
    );
    assert_eq!(
        decoded_frame_timestamp_nanos(None, None, time_base, &mut state).unwrap(),
        43_333_333
    );
}

#[test]
fn output_pts_is_forced_monotonic_after_retiming() {
    let mut state = MonotonicPtsState::default();

    assert_eq!(enforce_monotonic_pts(10, &mut state), 10);
    assert_eq!(enforce_monotonic_pts(10, &mut state), 11);
    assert_eq!(enforce_monotonic_pts(9, &mut state), 12);
}

#[test]
fn audio_frame_crossing_keep_boundary_is_split() {
    let keeps = vec![
        crate::core::cut::KeepSegment {
            start: crate::core::frame::MediaTimestamp::from_nanos(0),
            end: crate::core::frame::MediaTimestamp::from_nanos(20_000_000),
        },
        crate::core::cut::KeepSegment {
            start: crate::core::frame::MediaTimestamp::from_nanos(60_000_000),
            end: crate::core::frame::MediaTimestamp::from_nanos(100_000_000),
        },
    ];

    let slices = audio_slices_for_keeps(0, 48_000, 4_800, &keeps);

    assert_eq!(
        slices,
        vec![
            AudioSlicePlan {
                source_sample_offset: 0,
                sample_frames: 960,
                output_start_nanos: 0,
            },
            AudioSlicePlan {
                source_sample_offset: 2_880,
                sample_frames: 1_920,
                output_start_nanos: 20_000_000,
            },
        ]
    );
}

#[test]
fn packet_timestamps_are_rescaled_before_muxing() {
    let mut packet = ffmpeg_next::Packet::empty();
    packet.set_pts(Some(1_500));
    packet.set_dts(Some(1_480));
    packet.set_duration(40);

    crate::media::ffmpeg_common::prepare_encoded_packet_for_mux(
        &mut packet,
        2,
        ffmpeg_next::Rational(1, 1_000),
        ffmpeg_next::Rational(1, 100),
    );

    assert_eq!(packet.stream(), 2);
    assert_eq!(packet.pts(), Some(150));
    assert_eq!(packet.dts(), Some(148));
    assert_eq!(packet.duration(), 4);
    assert_eq!(packet.position(), -1);
}
```

- [ ] **Step 5: Add fixed preset filter policy**

In the same feature-gated section:

```rust
#[cfg(feature = "ffmpeg")]
fn video_filter_chain_for_preset(
    preset: ExportPreset,
    _input_width: u32,
    _input_height: u32,
) -> String {
    let spec = preset.spec();
    match spec.scale_policy {
        crate::media::export_presets::ExportScalePolicy::FitWithBars => format!(
            "scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2,setsar=1,format=yuv420p",
            spec.width, spec.height, spec.width, spec.height
        ),
        crate::media::export_presets::ExportScalePolicy::CenterCrop => format!(
            "scale={}:{}:force_original_aspect_ratio=increase,crop={}:{},setsar=1,format=yuv420p",
            spec.width, spec.height, spec.width, spec.height
        ),
    }
}
```

Rules:

- Filter graph strings are built only from trusted preset constants.
- No user text is inserted into the graph.
- `_input_width` and `_input_height` are accepted for future validation and must not be user-provided strings.

- [ ] **Step 6: Replace `FfmpegTrimExporter` skeleton with cleanup shell**

Implement top-level error/cancel cleanup before adding detailed transcoding:

```rust
#[cfg(feature = "ffmpeg")]
impl TrimExporter for FfmpegTrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
        if request.cancel_token.load(Ordering::Relaxed) {
            return Err(AppError::ExportCancelled);
        }
        if !request.input_path.exists() {
            return Err(AppError::ExportFailed {
                reason: format!("源文件不存在: {}", request.input_path.to_string_lossy()),
            });
        }
        if request.input_path == request.output_path {
            return Err(AppError::ExportFailed {
                reason: "导出文件不能覆盖原始录制文件".to_string(),
            });
        }

        crate::media::ffmpeg_common::init_ffmpeg_for_export()?;
        let result = run_ffmpeg_export(&request);
        if result.is_err() || request.cancel_token.load(Ordering::Relaxed) {
            crate::media::ffmpeg_common::remove_partial_file(&request.output_path);
        }
        let result = result?;
        if let Some(progress) = &request.progress {
            progress.report(100);
        }
        Ok(result)
    }
}
```

- [ ] **Step 7: Implement `run_ffmpeg_export()`**

`run_ffmpeg_export(request)` must implement this concrete pipeline:

1. Open input with `ffmpeg_next::format::input(&request.input_path)`.
2. Find best video and audio streams.
3. Open decoder for video and audio streams. Set each decoder packet time base from its input stream.
4. Open MP4 output context at `request.output_path`.
5. Create H.264 video encoder using preset dimensions, 30fps, target bitrate from `request.preset.spec()`.
6. Create AAC audio encoder using input sample rate/channels or fallback 48kHz stereo.
7. Build video scaling/cropping through FFmpeg filter graph from `video_filter_chain_for_preset()`.
8. Decode packets.
9. For each decoded video frame:
   - Convert `frame.timestamp().or_else(|| frame.pts())` to nanoseconds through `decoded_frame_timestamp_nanos()`.
   - Skip frame when `kept_output_timestamp_nanos()` returns `None`.
   - Convert retimed output nanoseconds to encoder time-base units through `nanos_to_time_base_units()`.
   - Reassign output PTS after `enforce_monotonic_pts()`.
   - Push through filter graph.
   - Encode, call `prepare_encoded_packet_for_mux()`, then `write_interleaved()`.
10. For each decoded audio frame:
   - Convert `frame.timestamp().or_else(|| frame.pts())` to nanoseconds through `decoded_frame_timestamp_nanos()`.
   - Compute `audio_slices_for_keeps()` using the decoded frame start, input sample rate, sample-frame count, and keep segments.
   - For each slice, copy only the selected interleaved samples into a short-lived audio frame.
   - Reassign output PTS from `AudioSlicePlan.output_start_nanos` through `nanos_to_time_base_units()` and `enforce_monotonic_pts()`.
   - Resample to encoder format.
   - Encode, call `prepare_encoded_packet_for_mux()`, then `write_interleaved()`.
11. Check `request.cancel_token` between packet batches and after every progress report.
12. Report progress from processed kept duration through `ffmpeg_common::report_progress()`.
13. Flush in this order:
   - send EOF to video/audio decoders
   - drain decoded video frames through filter graph
   - drain decoded audio frames through the resampler
   - send EOF to video/audio encoders
   - drain encoded packets with `prepare_encoded_packet_for_mux()`
14. Write trailer only after both encoders have been drained.
15. Inspect output artifact with binding inspection:
   - dimensions equal preset width/height
   - file size > 0
   - duration > 0
   - video stream exists
   - audio stream exists
16. Return `TrimExportResult { output_path: request.output_path.clone(), cut_count: request.cut_timeline.cuts.len() }`.

If any step fails, return `AppError::ExportFailed` with Chinese context and let the cleanup shell remove partial output.

Explicit non-goals for this task:

- `request.effect_timeline_path` is accepted and forwarded through the request boundary, but cursor overlay compositing is optional for this playable-export task. If the path is `None`, export still succeeds without cursor redraw.
- Do not copy input packets directly for video or audio when a cut timeline is present; copy paths make timestamp retiming and boundary slicing ambiguous.

- [ ] **Step 8: Run helper tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg kept_output_timestamp_second_keep_is_monotonic
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_frame_crossing_keep_boundary_is_split
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg packet_timestamps_are_rescaled_before_muxing
```

Expected:

- PASS in FFmpeg-enabled environment.
- If FFmpeg dev libraries are missing, record the environment error and keep Task 4 unchecked.

- [ ] **Step 9: Run exporter artifact tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_rejects_same_input_output_path
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_applies_cut_timeline_and_preserves_original
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_cancel_removes_partial_output_and_preserves_original
```

Expected:

- PASS in FFmpeg-enabled environment.
- Output dimensions match selected preset.
- Source artifact still exists after export and cancel.
- Cancel removes partial output.

- [ ] **Step 10: Run all FFmpeg tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

Expected:

- PASS in FFmpeg-enabled environment.

- [ ] **Step 11: Commit playable exporter**

Run:

```bash
git add src-tauri/src/media/trim_exporter.rs src-tauri/src/media/ffmpeg_common.rs src-tauri/tests/ffmpeg_export.rs
git commit -m "feat(export): 实现FFmpeg预设导出"
```

## Task 5: Product Command Export Threading and Cleanup

**Files:**

- Modify: `src-tauri/src/app/events.rs`
- Modify: `src-tauri/src/app/export_service.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Test: `src-tauri/src/app/events.rs`
- Test: `src-tauri/src/app/export_service.rs`
- Test: `src-tauri/src/lib.rs`
- Test: existing frontend export tests

- [ ] **Step 1: Make export summary cursor timeline nullable**

Modify `src-tauri/src/app/events.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummaryPayload {
    pub frame_count: usize,
    pub click_effect_count: usize,
    pub effect_timeline_path: Option<String>,
    pub cut_count: usize,
    pub total_cut_nanos: u64,
    pub cut_timeline_path: Option<String>,
    pub output_path: Option<String>,
}
```

Update the serialization test:

```rust
#[test]
fn export_summary_serializes_nullable_effect_timeline_path() {
    let payload = ExportSummaryPayload {
        frame_count: 0,
        click_effect_count: 0,
        effect_timeline_path: None,
        cut_count: 0,
        total_cut_nanos: 0,
        cut_timeline_path: None,
        output_path: None,
    };

    let json = serde_json::to_string(&payload).unwrap();

    assert!(json.contains("\"effectTimelinePath\":null"));
    assert!(json.contains("\"outputPath\":null"));
}
```

Modify `src/lib/tauri.ts`:

```ts
export type ExportSummary = {
  frameCount: number
  clickEffectCount: number
  effectTimelinePath: string | null
  cutCount: number
  totalCutNanos: number
  cutTimelinePath: string | null
  outputPath: string | null
}
```

Rules:

- `build_cursor_effect_timeline` may still return a non-null `effectTimelinePath` on success.
- `export_video` must be able to return `effectTimelinePath: null` when cursor timeline is unavailable but playable export can continue.

- [ ] **Step 2: Add export command helpers**

In `src-tauri/src/lib.rs`, add small helpers near `cancel_export()`:

```rust
fn clear_export_cancel_token(
    state: &tauri::State<'_, AppState>,
    token: &Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let mut guard = state
        .export_cancel_token
        .lock()
        .map_err(|_| "导出取消状态锁已损坏".to_string())?;
    if guard
        .as_ref()
        .map(|active| Arc::ptr_eq(active, token))
        .unwrap_or(false)
    {
        *guard = None;
    }
    Ok(())
}

fn emit_export_terminal_progress(
    app: &AppHandle,
    preset_id: &'static str,
    output_path: Option<String>,
    error: Option<String>,
) {
    let _ = app.emit(
        "export-progress",
        ExportProgressPayload {
            preset: preset_id,
            progress: 100,
            cancellable: false,
            output_path,
            error,
        },
    );
}

fn ffmpeg_gate_message() -> String {
    "FFmpeg 导出功能未启用，无法生成可播放导出文件。请使用 FFmpeg 构建版本以启用完整导出功能。"
        .to_string()
}

fn export_summary_from_parts(
    cursor: Option<&CursorEffectSummaryPayload>,
    cut: Option<&CutTimelineSummaryPayload>,
    output_path: Option<String>,
) -> ExportSummaryPayload {
    ExportSummaryPayload {
        frame_count: cursor.map(|summary| summary.frame_count).unwrap_or(0),
        click_effect_count: cursor
            .map(|summary| summary.click_effect_count)
            .unwrap_or(0),
        effect_timeline_path: cursor.map(|summary| summary.effect_timeline_path.clone()),
        cut_count: cut.map(|summary| summary.cut_count).unwrap_or(0),
        total_cut_nanos: cut.map(|summary| summary.total_cut_nanos).unwrap_or(0),
        cut_timeline_path: cut.map(|summary| summary.cut_timeline_path.clone()),
        output_path,
    }
}
```

Rules:

- After the cancel token is installed, every early return must call `clear_export_cancel_token(...)` and emit terminal export progress.
- Do not use `?` on a fallible export-preparation step after token setup unless the error path has already gone through these helpers.
- Product command code must not instantiate `MockTrimExporter`; `MockTrimExporter` remains a unit-test helper only.

- [ ] **Step 3: Make cursor timeline optional for export**

Replace the hard-failing cursor build in `export_video()`:

```rust
let cursor = match build_cursor_effect_timeline(app.clone(), state.clone()).await {
    Ok(summary) => Some(summary),
    Err(error) => {
        let _ = app.emit(
            "post-process-progress",
            PostProcessProgressPayload {
                stage: "cursor",
                progress: 0,
                error: Some(format!(
                    "光标时间线不可用，本次导出将不叠加光标效果: {error}"
                )),
            },
        );
        None
    }
};
let effect_timeline = cursor
    .as_ref()
    .map(|summary| PathBuf::from(&summary.effect_timeline_path));
```

Rules:

- Cursor timeline failure or missing cursor metadata must not block basic playable export.
- The exporter receives `effect_timeline_path: None` in this case.
- Do not fall back to a stale `service.last_effect_timeline_path()` from a previous build after the current cursor build failed.
- If future product scope makes cursor overlay mandatory for a specific export mode, add a separate explicit mode flag; do not make the default playable export fail implicitly.

- [ ] **Step 4: Keep cut timeline failure terminal**

Keep auto-trim timeline build as a terminal export-preparation step, but route failure through cleanup:

```rust
let cut = if config.auto_trim_silences {
    match build_cut_timeline(app.clone(), state.clone()).await {
        Ok(summary) => Some(summary),
        Err(error) => {
            emit_export_terminal_progress(&app, preset_id, None, Some(error.clone()));
            clear_export_cancel_token(&state, &cancel_token)?;
            return Err(error);
        }
    }
} else {
    None
};
```

When reading the cut timeline JSON or trim metadata later, use the same terminal-error pattern instead of raw `?`.

- [ ] **Step 5: Return non-FFmpeg Gate before source/output/exporter work**

In `export_video()`, after optional cursor and cut summaries are available, add the non-FFmpeg branch before source artifact lookup and before output path generation:

```rust
#[cfg(not(feature = "ffmpeg"))]
{
    let message = ffmpeg_gate_message();
    emit_export_terminal_progress(&app, preset_id, None, Some(message));
    clear_export_cancel_token(&state, &cancel_token)?;
    return Ok(export_summary_from_parts(cursor.as_ref(), cut.as_ref(), None));
}
```

Rules:

- Non-FFmpeg builds return a stable Gate summary/error whether source artifact is missing or present.
- Non-FFmpeg builds always return `output_path: None`.
- Non-FFmpeg builds never call `MockTrimExporter`, never call `export_recording_with_timeline()`, and never fabricate a successful output path.

Expected terminal event:

```rust
ExportProgressPayload {
    progress: 100,
    cancellable: false,
    output_path: None,
    error: Some(ffmpeg_gate_message()),
    preset: preset_id,
}
```

- [ ] **Step 6: Build FFmpeg request inputs after Gate branch**

Only in the `#[cfg(feature = "ffmpeg")]` branch, read the current recording source and trim metadata after the non-FFmpeg Gate branch:

```rust
let (trim_metadata_path, source_path) = match state.service.lock() {
    Ok(service) => (
        service.last_trim_metadata_path(),
        service.last_recording_output_path(),
    ),
    Err(_) => {
        let error = "录制服务锁已损坏".to_string();
        emit_export_terminal_progress(&app, preset_id, None, Some(error.clone()));
        clear_export_cancel_token(&state, &cancel_token)?;
        return Err(error);
    }
};

let source_path = match source_path {
    Some(path) => PathBuf::from(path),
    None => {
        let error = "没有可用的原始录制文件，请先完成一次可播放录制".to_string();
        emit_export_terminal_progress(&app, preset_id, None, Some(error.clone()));
        clear_export_cancel_token(&state, &cancel_token)?;
        return Err(error);
    }
};

let cut_timeline = if let Some(ref summary) = cut {
    let path = PathBuf::from(&summary.cut_timeline_path);
    match TrimMetadataWriter::read_cut_timeline(&path) {
        Ok(timeline) => timeline,
        Err(error) => {
            let error = error.to_string();
            emit_export_terminal_progress(&app, preset_id, None, Some(error.clone()));
            clear_export_cancel_token(&state, &cancel_token)?;
            return Err(error);
        }
    }
} else {
    let duration_nanos = match trim_metadata_path {
        Some(path) => match TrimMetadataWriter::read_metadata(PathBuf::from(path).as_path()) {
            Ok(metadata) => metadata.duration_nanos,
            Err(error) => {
                let error = error.to_string();
                emit_export_terminal_progress(&app, preset_id, None, Some(error.clone()));
                clear_export_cancel_token(&state, &cancel_token)?;
                return Err(error);
            }
        },
        None => 0,
    };
    core::cut::CutTimeline::empty(duration_nanos)
};

let sequence = state
    .export_sequence
    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    + 1;

let output_path = match export_output_path(&source_path, export_preset, sequence) {
    Ok(path) => path,
    Err(error) => {
        let error = error.to_string();
        emit_export_terminal_progress(&app, preset_id, None, Some(error.clone()));
        clear_export_cancel_token(&state, &cancel_token)?;
        return Err(error);
    }
};

let app_for_progress = app.clone();
let progress_reporter = ExportProgressReporter::new(Arc::new(move |progress| {
    let _ = app_for_progress.emit(
        "export-progress",
        ExportProgressPayload {
            preset: preset_id,
            progress,
            cancellable: true,
            output_path: None,
            error: None,
        },
    );
}));
```

Rules:

- This branch reads `source_path` and `trim_metadata_path` from the service, but it must not read `last_effect_timeline_path`; the current `cursor` result is the only source for `effect_timeline`.
- `CutTimeline::empty(0)` is allowed only when trim metadata is absent; artifact inspection still validates the real output duration after export.
- Every service lock, cut timeline read, trim metadata read, output path generation, and source missing failure emits terminal progress and clears the active token.

- [ ] **Step 7: Move FFmpeg exporter work into blocking task**

Move only the heavy exporter call to `tauri::async_runtime::spawn_blocking`:

```rust
let token_for_export = cancel_token.clone();
let token_for_cleanup = cancel_token.clone();
let join_result = tauri::async_runtime::spawn_blocking(move || {
    let mut exporter = media::trim_exporter::FfmpegTrimExporter;
    app::export_service::export_recording_with_timeline(
        &mut exporter,
        source_path,
        Some(output_path),
        export_preset,
        cut_timeline,
        effect_timeline,
        token_for_export,
        Some(progress_reporter),
        sequence,
    )
})
.await;

let export_result = match join_result {
    Ok(result) => result,
    Err(error) => {
        let error = format!("导出任务异常终止: {error}");
        emit_export_terminal_progress(&app, preset_id, None, Some(error.clone()));
        clear_export_cancel_token(&state, &token_for_cleanup)?;
        return Err(error);
    }
};
```

Rules:

- Real FFmpeg export must not run on Tauri async runtime core thread.
- The progress callback may emit Tauri events from the blocking thread through cloned `AppHandle`.
- Use `token_for_cleanup` when clearing state after the blocking task returns, so the closure can own `token_for_export` without losing the identity check.
- Join errors must emit terminal progress and clear the active token before returning.

- [ ] **Step 8: Handle success and failure consistently**

Replace the result handling with the shared helpers:

```rust
match export_result {
    Ok(result) => {
        let output = result.output_path.to_string_lossy().to_string();
        emit_export_terminal_progress(&app, preset_id, Some(output.clone()), None);
        clear_export_cancel_token(&state, &token_for_cleanup)?;
        Ok(export_summary_from_parts(cursor.as_ref(), cut.as_ref(), Some(output)))
    }
    Err(error) => {
        let error = error.to_string();
        emit_export_terminal_progress(&app, preset_id, None, Some(error.clone()));
        clear_export_cancel_token(&state, &token_for_cleanup)?;
        Err(error)
    }
}
```

Rules:

- Success emits final `progress: 100`, `cancellable: false`, and non-null `output_path`.
- Failure/cancel emits final `progress: 100`, `cancellable: false`, and `output_path: None`.
- Active cancel token is cleared on success, failure, cancel, source missing, cut read failure, and blocking-task join error.

- [ ] **Step 9: Add command helper tests**

Add these tests in `src-tauri/src/lib.rs`:

```rust
#[test]
fn export_summary_allows_missing_cursor_timeline() {
    let cut = CutTimelineSummaryPayload {
        cut_count: 1,
        total_cut_nanos: 2_000_000_000,
        cut_timeline_path: "/tmp/cuts.json".to_string(),
    };

    let summary = export_summary_from_parts(None, Some(&cut), None);

    assert_eq!(summary.frame_count, 0);
    assert_eq!(summary.click_effect_count, 0);
    assert_eq!(summary.effect_timeline_path, None);
    assert_eq!(summary.cut_count, 1);
    assert_eq!(summary.output_path, None);
}

#[cfg(not(feature = "ffmpeg"))]
#[test]
fn non_ffmpeg_gate_summary_never_fakes_output() {
    let cursor = CursorEffectSummaryPayload {
        frame_count: 2,
        click_effect_count: 1,
        effect_timeline_path: "/tmp/effects.json".to_string(),
    };

    let summary = export_summary_from_parts(Some(&cursor), None, None);

    assert_eq!(summary.effect_timeline_path, Some("/tmp/effects.json".to_string()));
    assert_eq!(summary.output_path, None);
    assert!(ffmpeg_gate_message().contains("FFmpeg 导出功能未启用"));
}

#[cfg(not(feature = "ffmpeg"))]
#[test]
fn product_export_command_does_not_instantiate_mock_exporter() {
    let source = include_str!("lib.rs");
    let start = source
        .find("async fn export_video")
        .expect("export_video command should exist");
    let end = source[start..]
        .find("\nfn effect_timeline_path")
        .map(|offset| start + offset)
        .expect("export_video should end before effect_timeline_path");
    let export_video_body = &source[start..end];

    assert!(!export_video_body.contains("MockTrimExporter::new()"));
    assert!(!export_video_body.contains("media::trim_exporter::MockTrimExporter"));
}
```

- [ ] **Step 10: Add export service validation cleanup test**

In `src-tauri/src/app/export_service.rs` tests, add an exporter that creates an empty output and returns `Ok`:

```rust
struct EmptyFileExporter;

impl TrimExporter for EmptyFileExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
        std::fs::write(&request.output_path, b"").map_err(|error| AppError::ExportFailed {
            reason: error.to_string(),
        })?;
        Ok(TrimExportResult {
            output_path: request.output_path,
            cut_count: request.cut_timeline.cuts.len(),
        })
    }
}

#[test]
fn export_service_removes_partial_output_on_validation_failure() {
    let dir = std::env::temp_dir().join("luzhi-export-validation-cleanup-test");
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("raw.mp4");
    let output = dir.join("export.mp4");
    std::fs::write(&source, b"raw").unwrap();
    let mut exporter = EmptyFileExporter;

    let result = export_recording_with_timeline(
        &mut exporter,
        source.clone(),
        Some(output.clone()),
        ExportPreset::Bilibili,
        CutTimeline::empty(1_000_000_000),
        None,
        Arc::new(AtomicBool::new(false)),
        None,
        1,
    );

    assert!(result.is_err());
    assert!(source.exists());
    assert!(!output.exists());
    let _ = std::fs::remove_dir_all(dir);
}
```

- [ ] **Step 11: Run command/export service tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_summary_allows_missing_cursor_timeline
cargo test --manifest-path src-tauri/Cargo.toml non_ffmpeg_gate_summary_never_fakes_output
cargo test --manifest-path src-tauri/Cargo.toml product_export_command_does_not_instantiate_mock_exporter
cargo test --manifest-path src-tauri/Cargo.toml export_service_removes_partial_output_on_validation_failure
cargo test --manifest-path src-tauri/Cargo.toml export_service
cargo test --manifest-path src-tauri/Cargo.toml export_video
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter
npm test -- --run
```

Expected:

- Non-FFmpeg tests pass without FFmpeg dev libraries.
- FFmpeg tests pass only in FFmpeg-enabled environment.
- Frontend accepts `effectTimelinePath: null` in export summaries.

- [ ] **Step 12: Commit command threading**

Run:

```bash
git add src-tauri/src/app/events.rs src-tauri/src/app/export_service.rs src-tauri/src/lib.rs src/lib/tauri.ts
git commit -m "fix(export): 将FFmpeg导出移入阻塞任务"
```

## Task 6: Final Verification, Docs, and Manual Gates

**Files:**

- Modify: `tests/phase-6-w11-w12-checklist.md`
- Modify: `HANDOFF.md`
- Optional modify: `reference/tasks/phase-6-ffmpeg-implementation.md`

- [ ] **Step 1: Run baseline verification**

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

Expected:

- PASS.

- [ ] **Step 2: Run FFmpeg verification**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_creates_playable_source_artifact
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_video_before_audio_stays_bounded
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_without_audio_generates_silent_track
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_rejects_missing_source
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_rejects_same_input_output_path
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_applies_cut_timeline_and_preserves_original
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_cancel_removes_partial_output_and_preserves_original
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg kept_output_timestamp_second_keep_is_monotonic
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_frame_crossing_keep_boundary_is_split
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg packet_timestamps_are_rescaled_before_muxing
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg rust_code_does_not_shell_out_to_ffmpeg_or_ffprobe
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

Expected:

- PASS when FFmpeg dev libraries are installed.
- If environment is missing FFmpeg dev libraries, record the exact error and keep FFmpeg gates unchecked.

- [ ] **Step 3: Run export command cleanup verification**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_summary_allows_missing_cursor_timeline
cargo test --manifest-path src-tauri/Cargo.toml non_ffmpeg_gate_summary_never_fakes_output
cargo test --manifest-path src-tauri/Cargo.toml product_export_command_does_not_instantiate_mock_exporter
cargo test --manifest-path src-tauri/Cargo.toml export_service_removes_partial_output_on_validation_failure
```

Expected:

- PASS without FFmpeg dev libraries.
- Non-FFmpeg product export never fabricates output and never instantiates `MockTrimExporter`.
- Export service removes empty/invalid partial output while preserving source.

- [ ] **Step 4: Complete manual artifact evidence table**

Fill `tests/phase-6-w11-w12-checklist.md` FFmpeg Artifact Evidence rows:

```markdown
| Scenario | Source path | Output path | Output bytes | Width x Height | Video stream | Audio stream | Source duration | Output duration | Duration delta | Original exists before/after | Inspector | Date |
```

Required scenarios:

- original recording artifact
- 16:9 auto-trim off
- 9:16 auto-trim off
- 1:1 auto-trim off
- 16:9 auto-trim on
- cancel cleanup
- no-audio recording with generated silent AAC track
- cursor timeline unavailable but playable export succeeds with `effectTimelinePath: null`

- [ ] **Step 5: Run manual app verification**

Run:

```bash
npm run tauri -- dev --features ffmpeg
```

Manual checks:

- Record a 1080p session with system audio and microphone enabled.
- Stop recording and confirm `stop_recording` returns non-null `outputPath`.
- Export Bilibili 16:9, Douyin 9:16, and Xiaohongshu 1:1.
- Confirm each output opens in a media player and contains video/audio.
- Enable auto-trim and confirm shorter output when cuts exist.
- Cancel an export after progress starts and confirm partial output is removed.
- Confirm original source artifact still exists after success, failure, and cancel.
- Turn off system audio and microphone, record a short session, and confirm exported MP4 still has an AAC audio stream.
- Temporarily make cursor metadata unavailable, export, and confirm playable output still succeeds with no cursor overlay.
- Record 10 minutes at 1080p and document stop time, sidecar size, memory peak, FFmpeg writer queue peak, export time, A/V sync notes.
- Document 4K reserve behavior separately before enabling 4K export by default; note whether `FFMPEG_WRITER_MAX_QUEUED_BYTES` trips under expected 4K capture pressure.

- [ ] **Step 6: Update handoff status without overclaim**

Update `HANDOFF.md`:

- Mark Important 5 complete only if `src-tauri/tests/ffmpeg_export.rs` exists and FFmpeg artifact tests pass in an FFmpeg-enabled environment.
- Mark Important 6 complete only if writer/exporter produce inspected playable artifacts and Native Safety Gate is checked.
- If cursor overlay is not implemented, state that playable export is complete but cursor effect compositing remains a separate blocked item.
- State explicitly whether no-audio recordings generate silent AAC track.
- State explicitly that production writer/exporter use `media::ffmpeg_common`, not `test_support`.
- State explicitly that non-FFmpeg product export remains Gate-only and returns no fake `outputPath`.

- [ ] **Step 7: Commit docs and checklist**

Run:

```bash
git add tests/phase-6-w11-w12-checklist.md HANDOFF.md reference/tasks/phase-6-ffmpeg-implementation.md
git commit -m "docs(export): 更新FFmpeg导出验收证据"
```

## Rollback and Safety Notes

- If FFmpeg writer fails under feature build, keep default non-FFmpeg product path as explicit Gate and do not set `last_recording_output_path`.
- If exporter fails after creating output, remove only `request.output_path`.
- Never remove `request.input_path`.
- If encoder worker queue fills, return writer error and clean partial source artifact; do not block the capture callback to wait for FFmpeg.
- If no audio input exists, generate a silent AAC track; do not fail recording/export solely because the session is silent.
- If cursor timeline generation fails, export without cursor overlay and return `effectTimelinePath: null`; do not block playable export.
- If output validation fails after exporter returns `Ok`, remove the invalid output and preserve the source.
- Non-FFmpeg product command must remain Gate-only for every source state and must never return a fake output path.
- Production writer/exporter must never depend on `crate::test_support` or test-only artifact inspectors.
- If FFmpeg feature cannot link locally, record environment error and keep Important 5/6 open.

## Self-Review Checklist

- Important 5 coverage: `src-tauri/tests/ffmpeg_export.rs` includes missing source, same input/output rejection, source artifact, video-before-audio, no-audio silent track, all presets, cut timeline, cancel cleanup, artifact inspection, progress callback, and no-CLI static scan.
- Important 6 coverage: `FfmpegRecordingWriter` and `FfmpegTrimExporter` both replace skeleton behavior with real artifacts before being marked done.
- Exporter timestamp coverage: decoded best-effort timestamp, missing PTS fallback, B-frame display order, retimed monotonic output PTS, packet `rescale_ts`, and interleaved mux ordering are tested or manually reviewed.
- Exporter audio coverage: boundary-crossing audio frames are sample-sliced and second keep segment audio is retimed without A/V drift.
- Product command coverage: cursor timeline failure does not block playable export; non-FFmpeg build returns Gate summary/error with `outputPath: null` and never uses `MockTrimExporter`.
- Cleanup coverage: cancel, exporter error, output validation failure, source missing, cut read failure, and blocking join error all clear active cancel token and remove only planned output.
- BUG.md rules: no transparent-window or drag-region behavior touched; no `motion.div whileTap` added.
- Security rules: no CLI, no command string construction, no hardcoded secret.
- Architecture rules: media frames remain Rust-side; export runs outside frontend JS; production code uses `media::ffmpeg_common` instead of `test_support`; default `ffmpeg` feature remains opt-in.
- Native Safety: human review is a required gate before claiming Phase 6 export complete.
