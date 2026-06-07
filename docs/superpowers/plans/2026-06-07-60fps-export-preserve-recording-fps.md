# 60fps Export Preserve Recording FPS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让录制素材的美化导出默认跟随录制帧率，30fps 录制导出 30fps，60fps 录制导出 60fps，同时保留无元数据或显式 30fps 请求时的 30fps 兼容路径。

**Architecture:** 保留现有 `ExportPresetSpec` 的尺寸、码率和 fallback fps，不新增前端导出选项。Rust 导出请求增加可选 `video_fps` 合约，Tauri `export_video` 从录制 metadata 读取并校验 fps 后传给 `FfmpegTrimExporter`，FFmpeg exporter 使用该 fps 设置 encoder time base 和输出 PTS 映射。

**Tech Stack:** Rust, Tauri 2.0, ffmpeg-next, existing `FfmpegRecordingWriter`, existing `FfmpegTrimExporter`, existing Rust unit/integration tests.

---

## File Structure

| File | Action | Responsibility |
| --- | --- | --- |
| `src-tauri/src/media/trim_exporter.rs` | Modify | Extend `TrimExportRequest` with `video_fps`, validate requested export fps, and make FFmpeg export use requested fps instead of always using preset fps. |
| `src-tauri/src/app/export_service.rs` | Modify | Thread optional export fps through the app-level export service into `TrimExportRequest`. |
| `src-tauri/src/lib.rs` | Modify | Resolve recording fps from cursor metadata for fresh and history exports, validate 30/60 allowlist, and pass it into export service. |
| `src-tauri/tests/ffmpeg_export.rs` | Modify | Update `export_recording_with_timeline` call sites after signature change and add an integration-level 60fps preservation regression. |
| `BUG.md` | Modify | Record BUG-0024 and prevention rules for 60fps exports being unintentionally downconverted. |
| `HANDOFF.md` | Modify | Add a newest work record summarizing the implementation and verification. |
| `tests/2026-06-07-bug-0024-60fps-export-preserve-recording-fps-checklist.md` | Create | Manual and automated verification checklist for this change. |

## Assumptions

- The product behavior is: export fps follows recording metadata by default.
- Only 30fps and 60fps are supported because backend capture config already allowlists `30 | 60`.
- Existing presets still define output dimensions and bitrates. Their `fps: 30` remains a fallback for legacy recordings without metadata and tests that explicitly pass `video_fps: None`.
- If a metadata path exists but cannot be read, export should fail visibly. If metadata path is absent, export falls back to preset fps for legacy compatibility.
- No frontend UI changes are needed in this iteration.

## Phase 1: Add Export FPS Request Contract

### Task 1: Add `video_fps` to `TrimExportRequest` and central fps validation

**Files:**
- Modify: `src-tauri/src/media/trim_exporter.rs`

- [ ] **Step 1: Write failing tests for export fps resolution**

Add these tests in `src-tauri/src/media/trim_exporter.rs` inside the existing `#[cfg(test)] mod tests`, next to `export_preset_rejects_unknown_value`.

```rust
    #[test]
    fn export_fps_defaults_to_preset_fps_when_unspecified() {
        let fps = resolve_export_fps(None, ExportPreset::Bilibili).unwrap();

        assert_eq!(fps, 30);
    }

    #[test]
    fn export_fps_accepts_supported_recording_fps() {
        let fps = resolve_export_fps(Some(60), ExportPreset::Bilibili).unwrap();

        assert_eq!(fps, 60);
    }

    #[test]
    fn export_fps_rejects_unsupported_recording_fps() {
        let err = resolve_export_fps(Some(120), ExportPreset::Bilibili).unwrap_err();

        assert!(err.to_string().contains("fps"));
        assert!(err.to_string().contains("120"));
    }
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib export_fps_ -- --nocapture
```

Expected: FAIL to compile because `resolve_export_fps` does not exist yet.

- [ ] **Step 3: Add request field and resolver**

In `src-tauri/src/media/trim_exporter.rs`, replace the current imports:

```rust
#[cfg(feature = "ffmpeg")]
use crate::app::error::AppError;
use crate::app::error::AppResult;
```

with:

```rust
use crate::app::error::{AppError, AppResult};
```

Update `TrimExportRequest` by adding `video_fps` immediately after `preset`:

```rust
pub struct TrimExportRequest {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub preset: ExportPreset,
    pub video_fps: Option<u32>,
    pub cut_timeline: CutTimeline,
    pub effect_timeline_path: Option<PathBuf>,
    /// Directory containing cursor PNG assets (arrow/hand/ibeam .png files).
    /// Required when `effect_timeline_path` is set and `render_cursor_overlay` is true.
    pub cursor_assets_dir: Option<PathBuf>,
    pub cancel_token: Arc<AtomicBool>,
    pub progress: Option<ExportProgressReporter>,
}
```

Update `PartialEq` so request comparisons include the requested fps:

```rust
impl PartialEq for TrimExportRequest {
    fn eq(&self, other: &Self) -> bool {
        self.input_path == other.input_path
            && self.output_path == other.output_path
            && self.preset == other.preset
            && self.video_fps == other.video_fps
            && self.cut_timeline == other.cut_timeline
            && self.effect_timeline_path == other.effect_timeline_path
            && self.cursor_assets_dir == other.cursor_assets_dir
    }
}
```

Add this helper below the `PartialEq` impl:

```rust
pub(crate) fn resolve_export_fps(
    requested_video_fps: Option<u32>,
    preset: ExportPreset,
) -> AppResult<u32> {
    let fps = requested_video_fps.unwrap_or_else(|| preset.spec().fps);
    match fps {
        30 | 60 => Ok(fps),
        other => Err(AppError::ExportFailed {
            reason: format!("不支持的导出 fps: {other}，仅支持 30 或 60"),
        }),
    }
}
```

Add `video_fps: None,` immediately after each `preset: ...` field in existing `TrimExportRequest { ... }` literals in this file. The affected local test requests are found by:

```bash
rg -n "TrimExportRequest \\{" src-tauri/src/media/trim_exporter.rs
```

- [ ] **Step 4: Run tests and verify GREEN**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib export_fps_ -- --nocapture
```

Expected: PASS for the three `export_fps_...` tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/trim_exporter.rs
git commit -m "fix(export): 增加导出帧率请求合约"
```

## Phase 2: Thread Export FPS Through Export Service

### Task 2: Forward `video_fps` through `export_recording_with_timeline`

**Files:**
- Modify: `src-tauri/src/app/export_service.rs`
- Modify: `src-tauri/tests/ffmpeg_export.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing export service forwarding test**

In `src-tauri/src/app/export_service.rs`, inside the existing test module, add this exporter and test after `FileCreatingExporter`.

```rust
    struct CapturingFileExporter {
        captured: Arc<std::sync::Mutex<Option<TrimExportRequest>>>,
    }

    impl TrimExporter for CapturingFileExporter {
        fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
            *self.captured.lock().unwrap() = Some(request.clone());
            std::fs::write(&request.output_path, b"mp4").map_err(|error| {
                AppError::ExportFailed {
                    reason: error.to_string(),
                }
            })?;
            Ok(TrimExportResult {
                output_path: request.output_path,
                cut_count: request.cut_timeline.cuts.len(),
            })
        }
    }

    #[test]
    fn export_service_forwards_requested_video_fps_to_exporter() {
        let dir = std::env::temp_dir().join("luzhi-export-service-fps-test");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("raw.mp4");
        std::fs::write(&source, b"raw").unwrap();
        let captured = Arc::new(std::sync::Mutex::new(None));
        let mut exporter = CapturingFileExporter {
            captured: captured.clone(),
        };
        let cancel = Arc::new(AtomicBool::new(false));

        let result = export_recording_with_timeline(
            &mut exporter,
            source.clone(),
            None,
            ExportPreset::Bilibili,
            Some(60),
            CutTimeline::empty(10_000_000_000),
            None,
            None,
            cancel,
            None,
            10,
        )
        .unwrap();

        assert!(result.output_path.exists());
        assert_eq!(
            captured.lock().unwrap().as_ref().unwrap().video_fps,
            Some(60)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
```

- [ ] **Step 2: Run test and verify RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib export_service_forwards_requested_video_fps_to_exporter -- --nocapture
```

Expected: FAIL to compile because `export_recording_with_timeline` does not accept a `video_fps` argument yet.

- [ ] **Step 3: Update service function signature and request construction**

In `src-tauri/src/app/export_service.rs`, change the function signature from:

```rust
pub fn export_recording_with_timeline(
    exporter: &mut dyn TrimExporter,
    input_path: PathBuf,
    requested_output_path: Option<PathBuf>,
    preset: ExportPreset,
    cut_timeline: CutTimeline,
    effect_timeline_path: Option<PathBuf>,
    cursor_assets_dir: Option<PathBuf>,
    cancel_token: Arc<AtomicBool>,
    progress: Option<ExportProgressReporter>,
    sequence: u64,
) -> AppResult<TrimExportResult> {
```

to:

```rust
pub fn export_recording_with_timeline(
    exporter: &mut dyn TrimExporter,
    input_path: PathBuf,
    requested_output_path: Option<PathBuf>,
    preset: ExportPreset,
    video_fps: Option<u32>,
    cut_timeline: CutTimeline,
    effect_timeline_path: Option<PathBuf>,
    cursor_assets_dir: Option<PathBuf>,
    cancel_token: Arc<AtomicBool>,
    progress: Option<ExportProgressReporter>,
    sequence: u64,
) -> AppResult<TrimExportResult> {
```

In the `TrimExportRequest` construction, add the forwarded fps:

```rust
    let result = exporter.export(TrimExportRequest {
        input_path,
        output_path,
        preset,
        video_fps,
        cut_timeline,
        effect_timeline_path,
        cursor_assets_dir,
        cancel_token,
        progress,
    });
```

Update existing `export_recording_with_timeline(...)` calls that should keep current behavior by inserting `None` immediately after the `ExportPreset::...` argument. This applies to:

```text
src-tauri/src/app/export_service.rs
src-tauri/tests/ffmpeg_export.rs
src-tauri/src/lib.rs
```

The production `src-tauri/src/lib.rs` call will be changed from `None` to metadata-driven fps in Phase 3.

- [ ] **Step 4: Run forwarding test and existing export service tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib export_service_ -- --nocapture
```

Expected: PASS for export service tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/app/export_service.rs src-tauri/tests/ffmpeg_export.rs src-tauri/src/lib.rs src-tauri/src/media/trim_exporter.rs
git commit -m "fix(export): 传递导出帧率请求参数"
```

## Phase 3: Make FFmpeg Export Honor Requested FPS

### Task 3: Preserve 60fps when `TrimExportRequest.video_fps = Some(60)`

**Files:**
- Modify: `src-tauri/src/media/trim_exporter.rs`

- [ ] **Step 1: Write failing FFmpeg regression test**

In `src-tauri/src/media/trim_exporter.rs`, inside `#[cfg(feature = "ffmpeg")] mod ffmpeg_tests`, add this test after `ffmpeg_exporter_exports_60fps_source_to_30fps_preset_without_duplicate_pts`.

```rust
        #[test]
        fn ffmpeg_exporter_preserves_60fps_when_requested() {
            let duration_nanos = 1_000_000_000;
            let source = create_60fps_source("export-60fps-preserve-source", duration_nanos);
            let output = ffmpeg_helpers::unique_media_path("export-60fps-preserve-out", "mp4");

            let mut exporter = FfmpegTrimExporter;
            let result = exporter
                .export(TrimExportRequest {
                    input_path: source.clone(),
                    output_path: output.clone(),
                    preset: ExportPreset::Bilibili,
                    video_fps: Some(60),
                    cut_timeline: CutTimeline::empty(duration_nanos),
                    effect_timeline_path: None,
                    cursor_assets_dir: None,
                    cancel_token: Arc::new(AtomicBool::new(false)),
                    progress: None,
                })
                .unwrap();

            assert_eq!(result.output_path, output);
            let inspection = ffmpeg_helpers::inspect_media_artifact(&output).unwrap();
            assert!(inspection.has_video_stream);
            assert!(inspection.has_audio_stream);
            let fps = inspection
                .video_avg_fps
                .expect("FFmpeg inspection should expose video_avg_fps for test artifacts");
            assert!(
                (55.0..=65.0).contains(&fps),
                "60fps source should export near 60fps when requested, got {fps:.2}"
            );
            assert!(
                inspection.video_duration_nanos <= 1_500_000_000,
                "60fps export should not inflate duration, got {}ms",
                inspection.video_duration_nanos / 1_000_000
            );

            let _ = std::fs::remove_file(&source);
            let _ = std::fs::remove_file(&output);
        }
```

Also update the existing `ffmpeg_exporter_exports_60fps_source_to_30fps_preset_without_duplicate_pts` request to pass `video_fps: None`; that keeps the explicit downconversion regression alive.

- [ ] **Step 2: Run test and verify RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_preserves_60fps_when_requested -- --nocapture
```

Expected: FAIL because the exporter still uses `preset_spec.fps` and produces near 30fps.

- [ ] **Step 3: Use resolved fps in FFmpeg exporter**

In `src-tauri/src/media/trim_exporter.rs`, replace:

```rust
        // Video encoder (H.264) — use fps-based time base.
        let out_fps = preset_spec.fps;
```

with:

```rust
        // Video encoder (H.264) uses the request fps when available.
        // Preset fps remains the fallback for legacy exports without metadata.
        let out_fps = resolve_export_fps(request.video_fps, request.preset)?;
```

Update the duplicate PTS comment near `if out_pts <= last_video_out_pts` from preset-specific wording to request-fps wording:

```rust
                                // When the requested export fps is lower than
                                // source fps, multiple decoded frames can map
                                // to the same output PTS tick. Encoding both
                                // would make x264 produce duplicate DTS/PTS and
                                // the MP4 muxer rejects the packet. Drop
                                // duplicate-tick frames so output follows the
                                // requested fps.
```

- [ ] **Step 4: Run focused FFmpeg tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_preserves_60fps_when_requested -- --nocapture
```

Expected: PASS.

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_exports_60fps_source_to_30fps_preset_without_duplicate_pts -- --nocapture
```

Expected: PASS. This proves fallback 30fps downconversion still works and duplicate PTS stays fixed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/trim_exporter.rs
git commit -m "fix(export): 按请求帧率配置 FFmpeg 导出"
```

## Phase 4: Resolve Recording FPS in Tauri Export Path

### Task 4: Read recording metadata fps and pass it into export service

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing metadata fps resolver tests**

In `src-tauri/src/lib.rs`, inside the existing `#[cfg(test)] mod tests`, add these tests after `capture_mode_update_rejects_unsupported_fps`.

```rust
    fn export_fps_metadata_path(prefix: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir()
            .join("luzhi-export-fps-tests")
            .join(format!("{prefix}-{seq}.json"))
    }

    #[test]
    fn export_video_fps_uses_recording_metadata_fps() {
        let path = export_fps_metadata_path("fps-60");
        let metadata = metadata_with_raw_visible(false);
        RecordingMetadataWriter::write_metadata(&path, &metadata).unwrap();

        let fps = resolve_export_video_fps_from_metadata_path(Some(
            path.to_string_lossy().to_string(),
        ))
        .unwrap();

        assert_eq!(fps, Some(60));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn export_video_fps_falls_back_when_metadata_path_missing() {
        let fps = resolve_export_video_fps_from_metadata_path(None).unwrap();

        assert_eq!(fps, None);
    }

    #[test]
    fn export_video_fps_rejects_unsupported_metadata_fps() {
        let path = export_fps_metadata_path("fps-120");
        let mut metadata = metadata_with_raw_visible(false);
        metadata.fps = 120;
        RecordingMetadataWriter::write_metadata(&path, &metadata).unwrap();

        let err = resolve_export_video_fps_from_metadata_path(Some(
            path.to_string_lossy().to_string(),
        ))
        .unwrap_err();

        assert!(err.contains("fps"));
        assert!(err.contains("120"));
        let _ = std::fs::remove_file(path);
    }
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib export_video_fps_ -- --nocapture
```

Expected: FAIL to compile because `resolve_export_video_fps_from_metadata_path` does not exist yet.

- [ ] **Step 3: Add metadata fps resolver**

In `src-tauri/src/lib.rs`, add this helper near `export_video`, before the `#[tauri::command] async fn export_video(...)` definition:

```rust
fn resolve_export_video_fps_from_metadata_path(
    metadata_path: Option<String>,
) -> Result<Option<u32>, String> {
    let Some(path) = metadata_path else {
        return Ok(None);
    };

    let metadata = RecordingMetadataWriter::read_metadata(Path::new(&path))
        .map_err(|error| format!("读取录制帧率元数据失败：{error}"))?;
    match metadata.fps {
        30 | 60 => Ok(Some(metadata.fps)),
        other => Err(format!("录制元数据 fps 不受支持：{other}，仅支持 30 或 60")),
    }
}
```

- [ ] **Step 4: Thread cursor metadata path through `export_video`**

In `src-tauri/src/lib.rs::export_video`, change the path resolution block from a pair:

```rust
        let (trim_metadata_path, source_path) = if let Some(ref id) = recording_id {
```

to a triple:

```rust
        let (trim_metadata_path, cursor_metadata_path, source_path) = if let Some(ref id) = recording_id {
```

For the history branch, collect cursor metadata path from library context:

```rust
            let cursor = ctx
                .entry
                .cursor_metadata_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string());
            (trim, cursor, source)
```

For the fresh recording branch, collect cursor metadata path from service:

```rust
            let cursor = service.last_cursor_metadata_path();
            (trim, cursor, source)
```

After resolving `source_path`, resolve export fps:

```rust
        let export_video_fps = resolve_export_video_fps_from_metadata_path(cursor_metadata_path)?;
```

In the `spawn_blocking` call to `app::export_service::export_recording_with_timeline`, pass `export_video_fps` immediately after `export_preset`:

```rust
                    export_preset,
                    export_video_fps,
                    cut_timeline,
```

- [ ] **Step 5: Run metadata resolver tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib export_video_fps_ -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "fix(export): 从录制元数据读取导出帧率"
```

## Phase 5: Integration Verification and Documentation

### Task 5: Add end-to-end regression and update project records

**Files:**
- Modify: `src-tauri/tests/ffmpeg_export.rs`
- Modify: `BUG.md`
- Modify: `HANDOFF.md`
- Create: `tests/2026-06-07-bug-0024-60fps-export-preserve-recording-fps-checklist.md`

- [ ] **Step 1: Add integration helper for 60fps source**

In `src-tauri/tests/ffmpeg_export.rs`, add this helper after the existing `create_source` helper:

```rust
fn create_60fps_source(prefix: &str, duration_nanos: u64) -> PathBuf {
    use luzhi_lib::media::recording_writer::RecordingWriter;
    use luzhi_lib::test_support::ffmpeg_helpers::{test_audio_chunk_at, test_video_frame_at};

    let path = unique_path(prefix, "mp4");
    let mut writer =
        luzhi_lib::media::ffmpeg_writer::FfmpegRecordingWriter::with_fps(path.clone(), 60)
            .unwrap();

    let fps = 60u64;
    let frame_duration = 1_000_000_000 / fps;
    let frame_count = (duration_nanos / frame_duration).max(1);
    for i in 0..frame_count {
        writer
            .push_video(test_video_frame_at(i * frame_duration))
            .unwrap();
        if i % 5 == 0 && i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    let audio_interval = 20_000_000u64;
    let audio_count = (duration_nanos / audio_interval).max(1);
    for i in 0..audio_count {
        writer
            .push_audio(test_audio_chunk_at(i * audio_interval))
            .unwrap();
        if i % 10 == 0 && i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    let result = writer.finish().unwrap();
    assert_eq!(
        result.output_path.as_deref(),
        Some(path.to_string_lossy().as_ref())
    );
    path
}
```

- [ ] **Step 2: Add integration test for service-level 60fps export**

Add this test after `full_duration_export_produces_playable_mp4`:

```rust
#[test]
fn full_duration_export_preserves_requested_60fps() {
    let duration_nanos = 1_000_000_000;
    let source = create_60fps_source("int-60fps", duration_nanos);
    let output = unique_path("int-60fps-out", "mp4");

    let mut exporter = FfmpegTrimExporter;
    let result = export_recording_with_timeline(
        &mut exporter,
        source.clone(),
        Some(output.clone()),
        ExportPreset::Bilibili,
        Some(60),
        CutTimeline::empty(duration_nanos),
        None,
        None,
        Arc::new(AtomicBool::new(false)),
        None,
        1,
    )
    .unwrap();

    assert_eq!(result.output_path, output);
    let inspection =
        luzhi_lib::test_support::ffmpeg_helpers::inspect_media_artifact(&output).unwrap();
    assert!(inspection.has_video_stream, "must have video stream");
    assert!(inspection.has_audio_stream, "must have audio stream");
    assert_eq!(inspection.width, 1920);
    assert_eq!(inspection.height, 1080);
    let fps = inspection
        .video_avg_fps
        .expect("integration artifact should expose video_avg_fps");
    assert!(
        (55.0..=65.0).contains(&fps),
        "expected export near 60fps, got {fps:.2}"
    );

    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&output);
}
```

- [ ] **Step 3: Run integration regression**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test ffmpeg_export --features ffmpeg full_duration_export_preserves_requested_60fps -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Update BUG.md**

Add this entry at the top of the `## 已解决` section in `BUG.md`:

```markdown
### BUG-0024: 60fps 录制默认美化导出变成 30fps ✅ 已修复-待人工验证

**现象**：选择 60fps 录制后，录制可进入预览且导出不再因重复 PTS 失败，但最终美化导出视频仍是 30fps。

**根因**：导出 preset 的 `ExportPresetSpec.fps` 固定为 30fps。BUG-0023 修复了 60fps 源素材降到 30fps 时的 duplicate PTS，但导出链路没有从录制 metadata 读取源录制 fps，也没有把 fps 作为导出请求的一部分传给 FFmpeg exporter，因此 60fps 录制默认仍按 30fps 导出。

**修复**：

1. `TrimExportRequest` 增加 `video_fps: Option<u32>`，保留 `ExportPresetSpec.fps` 作为 legacy fallback。
2. `export_recording_with_timeline()` 将可选导出 fps 传入 exporter，mock 和真实 FFmpeg 路径共用同一请求合约。
3. `export_video` 从录制 cursor metadata 读取并校验 fps，只允许 30/60；fresh recording 和 history re-export 都走同一解析逻辑。
4. `FfmpegTrimExporter` 使用请求 fps 设置 H.264 encoder time base、stream time base、视频 PTS 映射和光标 overlay fps。
5. 新增 60fps 源素材导出后仍接近 60fps 的 FFmpeg 回归测试，同时保留 60fps 源显式降到 30fps fallback 的 duplicate PTS 回归。

**预防规则**：

60. 导出 preset 的尺寸、码率和 fps fallback 不能替代录制素材真实 fps；默认导出行为必须显式传递录制 fps。
61. 修复 60fps 录制链路时必须同时覆盖 writer、export service、FFmpeg exporter 和历史记录 re-export 的 fps 传递。
62. 60fps 源素材的 30fps fallback/downconvert 测试必须继续保留，防止 duplicate PTS 回归。
63. 从 metadata 读取 fps 时只能接受后端支持的 allowlist，不能让损坏或未来格式的任意 fps 进入 encoder time base。
```

- [ ] **Step 5: Create verification checklist**

Create `tests/2026-06-07-bug-0024-60fps-export-preserve-recording-fps-checklist.md`:

```markdown
# 2026-06-07 BUG-0024 60fps 导出保持录制帧率自测清单

## 范围

- 60fps 录制的美化导出默认保持 60fps。
- 30fps 录制和无 metadata 的 legacy 导出仍保持 30fps fallback。
- 保留 BUG-0023 的 60fps 源降到 30fps 时 duplicate PTS 防护。
- 不新增前端导出帧率选择，不改导出 preset 尺寸和码率。

## Root Cause 验证

- [ ] 确认 `ExportPresetSpec.fps` 仍为 30，只作为 fallback。
- [ ] 确认 `TrimExportRequest.video_fps` 能表达录制 metadata 中的 60fps。
- [ ] 确认 `export_video` fresh recording 从 `service.last_cursor_metadata_path()` 读取 fps。
- [ ] 确认 `export_video` history re-export 从 `RecordingLibrary` entry 的 `cursor_metadata_path` 读取 fps。
- [ ] 确认 metadata fps 只允许 30/60，120 等值会 hard fail。

## 自动化验证

- [ ] RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib export_fps_ -- --nocapture` 在实现 resolver 前失败。
- [ ] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib export_fps_ -- --nocapture` 通过。
- [ ] RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib export_service_forwards_requested_video_fps_to_exporter -- --nocapture` 在服务签名更新前失败。
- [ ] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib export_service_forwards_requested_video_fps_to_exporter -- --nocapture` 通过。
- [ ] RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_preserves_60fps_when_requested -- --nocapture` 在 exporter 使用请求 fps 前输出接近 30fps 并失败。
- [ ] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_preserves_60fps_when_requested -- --nocapture` 通过。
- [ ] 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_exports_60fps_source_to_30fps_preset_without_duplicate_pts -- --nocapture` 通过。
- [ ] 元数据解析：`cargo test --manifest-path src-tauri/Cargo.toml --lib export_video_fps_ -- --nocapture` 通过。
- [ ] 集成：`cargo test --manifest-path src-tauri/Cargo.toml --test ffmpeg_export --features ffmpeg full_duration_export_preserves_requested_60fps -- --nocapture` 通过。
- [ ] 回归集合：`cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg trim_exporter -- --test-threads=1` 通过。
- [ ] 空白检查：`git diff --check` 通过。
- [ ] 触碰文件格式：`rustfmt --check --edition 2021 --config skip_children=true src-tauri/src/media/trim_exporter.rs src-tauri/src/app/export_service.rs src-tauri/src/lib.rs src-tauri/tests/ffmpeg_export.rs` 通过。

## 人工验证建议

- [ ] 全屏选择 60fps，同时开启系统音频和麦克风，录制 10-30 秒后停止，应进入预览。
- [ ] 在预览页导出 Bilibili preset，应成功生成导出文件。
- [ ] 使用系统播放器或媒体信息工具确认导出视频帧率接近 60fps。
- [ ] 再录制 30fps 并导出，确认输出接近 30fps。
- [ ] 从历史记录重新打开同一个 60fps 录制并导出，确认 history re-export 也接近 60fps。
```

- [ ] **Step 6: Update HANDOFF.md**

Add a newest work record under `## 工作任务记录` with this content:

```markdown
### 2026-06-07：BUG-0024 60fps 录制默认美化导出保持录制帧率

输入文件：

- 用户确认：默认导出应跟随录制 fps，60fps 录制最终美化导出不应固定变成 30fps
- `HANDOFF.md`
- `BUG.md`
- `.claude/rules/0-global.md`
- `.claude/rules/2-testing.md`
- `src-tauri/src/media/trim_exporter.rs`
- `src-tauri/src/app/export_service.rs`
- `src-tauri/src/lib.rs`

根因结论：

1. BUG-0023 解决了 60fps 源素材降到 30fps preset 时的 duplicate PTS 导出失败。
2. 但产品默认导出路径仍使用 `ExportPresetSpec.fps=30`，没有读取录制 metadata 中的 60fps。
3. 导出请求缺少 fps 字段，`FfmpegTrimExporter` 只能按 preset fallback fps 设置 encoder time base。

已完成：

1. `TrimExportRequest` 增加 `video_fps: Option<u32>`，并统一校验 30/60fps。
2. `export_recording_with_timeline()` 将可选 fps 传入 exporter。
3. `export_video` fresh recording 和 history re-export 均从 cursor metadata 读取录制 fps，缺失 metadata 时保留 30fps fallback。
4. `FfmpegTrimExporter` 使用请求 fps 配置 H.264 encoder time base 和输出 PTS 映射。
5. 新增 60fps 导出保持 60fps 的 FFmpeg 回归测试，并保留 60fps 源降 30fps duplicate PTS 回归。

当前验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --lib export_fps_ -- --nocapture` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib export_service_forwards_requested_video_fps_to_exporter -- --nocapture` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib export_video_fps_ -- --nocapture` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_preserves_60fps_when_requested -- --nocapture` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_exports_60fps_source_to_30fps_preset_without_duplicate_pts -- --nocapture` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --test ffmpeg_export --features ffmpeg full_duration_export_preserves_requested_60fps -- --nocapture` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg trim_exporter -- --test-threads=1` 通过
- `git diff --check` 通过
- `rustfmt --check --edition 2021 --config skip_children=true src-tauri/src/media/trim_exporter.rs src-tauri/src/app/export_service.rs src-tauri/src/lib.rs src-tauri/tests/ffmpeg_export.rs` 通过

改动文件：

- **修改**: `src-tauri/src/media/trim_exporter.rs`
- **修改**: `src-tauri/src/app/export_service.rs`
- **修改**: `src-tauri/src/lib.rs`
- **修改**: `src-tauri/tests/ffmpeg_export.rs`
- **修改**: `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-07-bug-0024-60fps-export-preserve-recording-fps-checklist.md`

人工复核建议：

1. 全屏录制选择 60fps，同时开启系统音频和麦克风，录制约 10-30 秒后停止，应进入预览并可导出。
2. 检查导出文件帧率接近 60fps，视频时长接近实际录制时长，音频存在且可播放。
3. 从历史记录重新导出同一条 60fps 录制，确认输出也接近 60fps。
4. 再录制并导出 30fps，确认默认 30fps 路径未退化。
```

- [ ] **Step 7: Run final verification suite**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib export_fps_ -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib export_service_forwards_requested_video_fps_to_exporter -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib export_video_fps_ -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_preserves_60fps_when_requested -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_exporter_exports_60fps_source_to_30fps_preset_without_duplicate_pts -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test ffmpeg_export --features ffmpeg full_duration_export_preserves_requested_60fps -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg trim_exporter -- --test-threads=1
git diff --check
rustfmt --check --edition 2021 --config skip_children=true src-tauri/src/media/trim_exporter.rs src-tauri/src/app/export_service.rs src-tauri/src/lib.rs src-tauri/tests/ffmpeg_export.rs
```

Expected:

```text
All listed tests pass.
git diff --check exits 0.
rustfmt --check exits 0 for touched files.
Existing unrelated warnings may remain, but no new failing test or formatting error is accepted.
```

- [ ] **Step 8: Commit docs and checklist**

```bash
git add BUG.md HANDOFF.md tests/2026-06-07-bug-0024-60fps-export-preserve-recording-fps-checklist.md src-tauri/tests/ffmpeg_export.rs
git commit -m "test(export): 覆盖 60fps 导出保持录制帧率"
```

## Self-Review Notes

- Spec coverage: The plan covers request fps contract, service forwarding, FFmpeg encoder fps, Tauri metadata resolution for fresh/history export, fallback behavior, BUG/HANDOFF/checklist updates, and final verification.
- Red-flag scan: No banned planning patterns are present. Each code-changing step includes concrete code or exact replacement text.
- Type consistency: The same field name `video_fps: Option<u32>` is used in `TrimExportRequest`, `export_recording_with_timeline`, FFmpeg exporter tests, integration tests, and production export call.
