# Phase 6 Code Review Remediation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all issues identified in `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md`

**Architecture:** The code review identified 2 Critical, 4 Important, and 3 Minor issues across the export pipeline, audio activity analyzer, and documentation. All issues are localized fixes that don't require architectural changes.

**Tech Stack:** Rust (Tauri 2.0), FFmpeg feature gate, cargo test

---

## File Map

| File | Issue | Change |
|------|-------|--------|
| `src-tauri/src/lib.rs` | Critical 1, Critical 2 | Add non-FFmpeg guard; wire export to ExportService |
| `src-tauri/src/media/trim_audio_activity.rs` | Important 1 | Fix bucket boundary `>` → `>=` |
| `src-tauri/src/platform/macos_service.rs` | Important 2 | Capture audio push errors + add test |
| `src-tauri/src/app/export_service.rs` | Important 3 | Clean up partial output on cancel/failure |
| `HANDOFF.md` | Important 4 | Correct overclaim in documentation |

---

## Task 1: Critical 1 — Non-FFmpeg Build Path

**Problem:** `export_video()` assumes `source_path` is always `Some(...)` in non-FFmpeg builds, but `CountingRecordingWriter::new(None)` never produces a source artifact. The function would hit a dead path with an unhelpful error.

**Files:**
- Modify: `src-tauri/src/lib.rs:722-759`

- [ ] **Step 1: Add `#[cfg(not(feature = "ffmpeg"))]` guard**

Add the following block after the `(trim_metadata_path, source_path, effect_timeline)` destructuring (line ~722):

```rust
// In non-FFmpeg builds, source artifact is not available.
// Return a clear FFmpeg Gate error instead of requiring a source artifact.
#[cfg(not(feature = "ffmpeg"))]
if source_path.is_none() {
    // Clear cancel token before returning.
    {
        let mut guard = state
            .export_cancel_token
            .lock()
            .map_err(|_| "导出取消状态锁已损坏".to_string())?;
        *guard = None;
    }

    // Emit error progress.
    let _ = app.emit(
        "export-progress",
        ExportProgressPayload {
            preset: preset_id,
            progress: 100,
            cancellable: false,
            output_path: None,
            error: Some("FFmpeg 导出功能未启用，无法生成可播放导出文件。请使用 FFmpeg 构建版本以启用完整导出功能。".to_string()),
        },
    );

    return Ok(ExportSummaryPayload {
        frame_count: cursor.frame_count,
        click_effect_count: cursor.click_effect_count,
        effect_timeline_path: cursor.effect_timeline_path,
        cut_count: cut.as_ref().map(|summary| summary.cut_count).unwrap_or(0),
        total_cut_nanos: cut
            .as_ref()
            .map(|summary| summary.total_cut_nanos)
            .unwrap_or(0),
        cut_timeline_path: cut.map(|summary| summary.cut_timeline_path),
        output_path: None,
    });
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "fix(export): 非 FFmpeg 构建返回明确 gate 错误"
```

---

## Task 2: Critical 2 — Wire export_video to ExportService

**Problem:** `export_video()` builds timelines but never calls `ExportService.export_recording_with_timeline()`. The export pipeline is disconnected.

**Files:**
- Modify: `src-tauri/src/lib.rs:660-760`

- [ ] **Step 1: Add required imports**

Add to the imports section of `lib.rs`:

```rust
use media::export_paths::export_output_path;
use media::trim_exporter::ExportProgressReporter;
```

- [ ] **Step 2: Convert source_path to PathBuf**

After the `#[cfg(not(feature = "ffmpeg"))]` guard, change:

```rust
// Before (line ~760):
// let source_path = source_path;

// After:
let source_path = source_path
    .ok_or_else(|| "没有可用的原始录制文件，请先完成一次可播放录制".to_string())
    .map(PathBuf::from)?;
```

- [ ] **Step 3: Create progress reporter and call export service**

Add after the `source_path` conversion:

```rust
// Create progress reporter that emits events to the frontend.
let progress_reporter = ExportProgressReporter::new(Arc::new(move |progress| {
    let _ = app_for_progress.emit("export-progress", ExportProgressPayload {
        preset: preset_id,
        progress: progress.percent,
        cancellable: true,
        output_path: None,
        error: None,
    });
}));

// Call the export service
#[cfg(feature = "ffmpeg")]
let export_result = {
    let mut exporter = media::trim_exporter::FfmpegTrimExporter;
    app::export_service::export_recording_with_timeline(
        &mut exporter,
        &source_path,
        effect_timeline.as_deref(),
        cut.as_ref(),
        &export_preset,
        cancel_token.clone(),
        progress_reporter,
    )
};
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "fix(export): 接入 ExportService 和 TrimExporter"
```

---

## Task 3: Important 1 — Base RMS Bucket Boundary

**Problem:** Bucket boundary check uses `>` instead of `>=`, causing a sample exactly at `bucket_start + 100ms` to be included in the current bucket instead of the next one. This violates strict `[start, start+100ms)` semantics.

**Files:**
- Modify: `src-tauri/src/media/trim_audio_activity.rs:52`

- [ ] **Step 1: Fix boundary operator**

Change line 52 from:

```rust
if frame_nanos > bucket_start.saturating_add(BASE_RMS_BUCKET_NANOS)
```

To:

```rust
if frame_nanos >= bucket_start.saturating_add(BASE_RMS_BUCKET_NANOS)
```

- [ ] **Step 2: Update affected test**

The test `base_analyzer_emits_bucket_when_boundary_crossed` expects 10 buckets but will now get 19 (each sample triggers a new bucket due to exact boundary). Update the assertion:

```rust
// With strict [start, start+100ms) semantics, 20 samples at sample_rate=10
// produces 19 emitted buckets (each with 1 sample).
assert_eq!(activity.len(), 19);
```

- [ ] **Step 3: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -- trim_audio_activity`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/media/trim_audio_activity.rs
git commit -m "fix(audio): 修正 base RMS bucket 边界为严格 [start, start+100ms)"
```

---

## Task 4: Important 2 — Audio Push Error Not Captured

**Problem:** In the live-loop `consume_frames`, `writer.push_audio()` errors are silently ignored (only logged to stderr). The final drain captures errors, but live-loop errors are lost.

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs` (live-loop audio push)
- Modify: `src-tauri/src/platform/macos_service.rs` (add test)

- [ ] **Step 1: Capture audio push error in live-loop**

Find the live-loop `push_audio` call and change from:

```rust
if let Err(_e) = writer.push_audio(mixed) {
    eprintln!("写入混音音频失败: {_e}");
}
```

To:

```rust
if let Err(e) = writer.push_audio(mixed) {
    let msg = format!("写入混音音频失败: {e}");
    eprintln!("{msg}");
    errors.push(msg);
}
```

- [ ] **Step 2: Add test for audio push failure**

Add the following test to the `#[cfg(test)] mod tests` block:

```rust
/// Verifies that writer push_audio errors are collected in the live loop,
/// matching the final drain behavior.
#[test]
fn consume_frames_writer_push_audio_failure_records_error() {
    use crate::media::recording_writer::FailingRecordingWriter;

    let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1);
    let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(2);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mic_level = Arc::new(Mutex::new(0.0f64));

    // Send one audio chunk before stopping.
    let chunk = AudioChunk {
        timestamp: MediaTimestamp::from_nanos(0),
        sample_rate: 44100,
        channels: 1,
        samples: Arc::from(vec![0.0f32; 441].into_boxed_slice()),
    };
    assert!(audio_tx.try_send_drop_newest(chunk));

    // Writer that fails on push_audio.
    let writer: Box<dyn RecordingWriter> =
        Box::new(FailingRecordingWriter::new(false, true, false));

    // Drop senders so the channel drains.
    drop(video_tx);
    drop(audio_tx);

    // Use a separate thread to set stop_flag after a brief delay,
    // giving the consumer time to process the queued audio.
    let flag_clone = stop_flag.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        flag_clone.store(true, Ordering::Relaxed);
    });

    let output = MacRecordingService::consume_frames(
        stop_flag,
        video_rx,
        audio_rx,
        None,
        frame_count,
        writer,
        mic_level,
        "medium",
    );

    stopper.join().unwrap();

    assert!(
        output.errors.iter().any(|e| e.contains("写入混音音频失败")),
        "expected push_audio error in consumer output, got: {:?}",
        output.errors
    );
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -- consume_frames_writer_push_audio_failure`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): live-loop push_audio 错误不再被吞掉"
```

---

## Task 5: Important 3 — Partial Output Cleanup

**Problem:** When export fails or is cancelled, the partial output file is left on disk.

**Files:**
- Modify: `src-tauri/src/app/export_service.rs`

- [ ] **Step 1: Add cleanup logic around export call**

Wrap the `exporter.export()` call with cleanup:

```rust
let planned_output_path = output_path.clone();
let result = exporter.export(TrimExportRequest {
    source_path: source_path.to_path_buf(),
    output_path,
    effect_timeline,
    cut_timeline,
    preset,
    cancel_token,
    progress_reporter,
});

match result {
    Ok(result) => {
        if let Err(error) = validate_non_empty_output(&result.output_path) {
            let _ = std::fs::remove_file(&result.output_path);
            return Err(error);
        }
        Ok(result)
    }
    Err(error) => {
        let _ = std::fs::remove_file(&planned_output_path);
        Err(error)
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/app/export_service.rs
git commit -m "fix(export): cancel/failure 时清理 partial output 文件"
```

---

## Task 6: Important 4 — Documentation Overclaim

**Problem:** HANDOFF.md claims fixes were made in specific commits (`a17382a`, `e2a6ae2`) when those commits were actually formatting/refactoring, not behavioral fixes.

**Files:**
- Modify: `HANDOFF.md`

- [ ] **Step 1: Update Code Review 整改 section**

Replace the misleading section with accurate timeline:

```markdown
Code Review 整改（2026-05-30）：

**整改背景**：Code Review 文件 `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 于 commit `8e050f0` 时生成。后续 commits `a17382a` 和 `e2a6ae2` 包含格式化和锁合并等重构，但未包含行为修复。以下整改在 review 之后进行：

1. **Critical 1 修复**：`export_video()` 在非 FFmpeg 构建中不再因缺失 source artifact 陷入死路，返回明确 FFmpeg Gate 错误。
2. **Critical 2 修复**：`export_video()` 接入 `ExportService` 和 `TrimExporter`，cancel token 和 progress callback 进入 exporter boundary。
3. **Important 2 修复**：live-loop `push_audio` 错误不再被吞掉，与 final drain 保持一致进入 `errors`。新增 `consume_frames_writer_push_audio_failure_records_error` 测试验证。
4. **Important 3 修复**：`ExportService` 在 cancel/failure/validation error 时清理 partial output 文件。
5. **Important 1 修复**：Base RMS bucket 边界改为严格 `[start, start+100ms)`，exact-boundary sample 进入下一个 bucket。
6. **Minor 1 修复**：`MAX_AUDIO_SAMPLES` 注释更正为 `~2h @ 10/sec (100ms base buckets)`。
7. **Minor 3 修复**：清理 unused variable warnings。
```

- [ ] **Step 2: Commit**

```bash
git add HANDOFF.md
git commit -m "docs: 更正 Code Review 整改时间线描述"
```

---

## Task 7: Final Verification

- [ ] **Step 1: Run full test suite**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: All tests pass (198+ tests)

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected: No errors (warnings acceptable)

- [ ] **Step 3: Run format check**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

Expected: PASS

- [ ] **Step 4: Run frontend tests**

```bash
npm test -- --run
```

Expected: All tests pass (51 tests)

---

## Notes on Remaining Items (Not Blocking Phase 6)

The following items from the review are **not** part of this remediation:

1. **Minor 1 (Reviewer)**: No concurrent export guard — UI disables button, backend guard is defense-in-depth
2. **Minor 2 (Reviewer)**: Medium 750ms window not integer multiple of 100ms — semantic imprecision, needs sensitivity switching refinement
3. **Important 5**: Missing FFmpeg artifact integration tests — requires FFmpeg feature enabled
4. **Important 6**: `FfmpegRecordingWriter`/`FfmpegTrimExporter` still skeletons — requires Native Safety review

These are tracked in HANDOFF.md as remaining items.
