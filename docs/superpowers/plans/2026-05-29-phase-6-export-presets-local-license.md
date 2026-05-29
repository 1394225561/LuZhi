# Phase 6 Export Presets and Local License Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete W11-W12 by closing the Phase 5 FFmpeg/export Gate, adding fixed 16:9 / 9:16 / 1:1 export presets with progress/cancel, and exposing local 14-day trial plus activation-status interfaces.

**Architecture:** Phase 6 is split into small phases so the Phase 5 carry-over items do not become one large diff: export pipeline preflight, real playable export, preset UI/progress, and local license. Rust remains the only layer that touches media files, cut timelines, FFmpeg bindings, export cancellation, and license persistence; React only sends commands and displays lightweight status, summaries, paths, and errors.

**Tech Stack:** Tauri 2, Rust 2021, optional `ffmpeg-next` feature, serde JSON sidecars, React 19 + TypeScript + Tailwind + shadcn/ui, Vitest/React Testing Library.

---

## Source Inputs

- `HANDOFF.md`
- `BUG.md`
- `.codex/rules/0-global.md` through `.codex/rules/5-docs.md`
- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/2026-05-29-phase-5-ffmpeg-trim-completion.md`
- `docs/superpowers/reviews/2026-05-29-phase-5-code-review.md`, especially `## 16. 三轮整改补充复审（2026-05-29）：排除 Phase 6 合并项后的 Phase 5 代码复核`
- `tests/phase-5-w9-w10-checklist.md`
- `tests/phase-6-w11-w12-checklist.md`

## Assumptions

1. Phase 6 may absorb the two Phase 5 deferred product items from review section 16: real playable FFmpeg trimmed export and post-recording trim sensitivity re-aggregation.
2. Phase 6 must first close the export-pipeline preflight before it can claim "三种格式导出成功".
3. Export presets are fixed product presets, not a template system.
4. Local authorization means 14-day local trial state plus activation status/query boundary. It does not include a service-side activation protocol, activation-code generation, anti-abuse backend, or private verification key.
5. AI-generated FFmpeg and platform credential code requires human Native Safety review before switching it into the default product path.

## Out Of Scope

- Subtitles, summaries, knowledge base, templates, team collaboration.
- Platform publishing APIs.
- Background beautification, shortcut overlays, automatic zoom/key-area analysis.
- Changing core dependency versions in `src-tauri/Cargo.toml`.
- Real server activation protocol or hardcoded activation private keys.

## Phase Breakdown

### Phase 6A: Export Pipeline Preflight

Success criteria:

- Export presets and output paths are deterministic and tested.
- Writer push/finish failures are fatal before production writer is enabled.
- `export_video()` forms a structured export request/plan through the existing `TrimExporter` boundary.
- `outputPath` remains `None` unless a real non-empty playable file exists.
- Trim sensitivity can be rebuilt from sensitivity-independent base audio buckets.

### Phase 6B: Playable FFmpeg Export

Success criteria:

- Original recording artifact is preserved.
- Export creates a separate playable output artifact.
- Auto-trim off exports the full source.
- Auto-trim on consumes `CutTimeline` and exports a shorter playable file when cuts exist.
- FFmpeg integration uses Rust binding/C API only; no CLI command construction.
- Automated FFmpeg tests inspect source/export artifacts for dimensions, duration, streams, file size, and original preservation.

### Phase 6C: Presets, Progress, Cancel, Preview UI

Success criteria:

- Bilibili / YouTube 16:9, Douyin 9:16, and Xiaohongshu 1:1 presets use fixed dimensions/crop policies.
- Export progress is visible in UI.
- Export progress is emitted by Rust exporter callbacks and includes intermediate values, not just 0/100 UI state.
- Export can be cancelled.
- Cancel and failure paths clean partial output files.

### Phase 6D: Local License

Success criteria:

- First launch initializes a 14-day local trial.
- UI can display trial remaining / expired / activated status.
- Activation status command boundary exists without implementing a server protocol.
- No private key, DSN, analytics key, or activation secret is hardcoded.

### Phase 6E: Gates, Docs, Handoff

Success criteria:

- Automated checks pass.
- Manual gates are documented: playable file, A/V sync, original preservation, cancel cleanup, 10-minute 1080p pressure, Native Safety review, BUG.md scan.
- `HANDOFF.md` records final status and remaining risks.

## File Structure

Create:

- `src-tauri/src/media/export_presets.rs`
  - Fixed preset IDs, dimensions, aspect ratios, and crop/fit policy.
- `src-tauri/src/media/export_paths.rs`
  - Generates independent output paths and validates output files before returning `outputPath`.
- `src-tauri/src/media/trim_audio_activity.rs`
  - Sensitivity-independent 100ms base RMS buckets and re-aggregation into configured RMS windows.
- `src-tauri/src/media/original_recording_artifact.rs`
  - Focused helper for selecting the production recording writer and validating the original recording artifact before export.
- `src-tauri/src/media/ffmpeg_test_support.rs`
  - Feature-gated helpers for generating synthetic FFmpeg artifacts and inspecting streams/dimensions/duration through bindings.
- `src-tauri/src/app/export_service.rs`
  - Pure orchestration helper for building export requests, validating source/output paths, and calling a `TrimExporter`.
- `src-tauri/src/app/license_service.rs`
  - Local trial and activation status model, clock abstraction, and store abstraction.
- `src-tauri/tests/ffmpeg_export.rs`
  - Feature-gated integration tests for FFmpeg-enabled export.
- `src/components/license-status.tsx`
  - Compact UI status for trial/activation.

Modify:

- `src-tauri/src/media/mod.rs`
  - Export new media modules.
- `src-tauri/src/app/mod.rs`
  - Export `export_service` and `license_service`.
- `src-tauri/src/app/error.rs`
  - Add export/license error variants with Chinese messages.
- `src-tauri/src/app/events.rs`
  - Add export progress/cancel/status payloads and license status payload.
- `src-tauri/src/media/trim_exporter.rs`
  - Move preset details to `export_presets`, extend request with effect timeline path and cancel/progress hooks.
- `src-tauri/src/media/ffmpeg_writer.rs`
  - Replace skeleton behavior with a real original recording artifact writer before any playable export is enabled.
- `src-tauri/src/media/trim_metadata.rs`
  - Add metadata schema version and base audio activity buckets while keeping legacy `audioActivity` compatibility.
- `src-tauri/src/platform/macos_service.rs`
  - Store base audio buckets; keep writer errors fatal.
- `src-tauri/src/lib.rs`
  - Wire `export_video`, `cancel_export`, `license_status`, and activation-status commands.
- `src/lib/tauri.ts`
  - Mirror new payload types and commands; add post-process progress listener.
- `src/components/preview-view.tsx`
  - Show export progress, cancel button, preset labels, output success path, and Gate copy only when appropriate.
- `src/App.tsx`
  - Display license status in idle/preview shell without moving business state into React.
- `src/App.test.tsx`
  - Add frontend coverage for progress/cancel/output/license UI.
- `tests/phase-6-w11-w12-checklist.md`
  - Track exact automated and manual gates.
- `HANDOFF.md`
  - Update only after implementation and verification are complete.

## Task 1: Export Preset and Path Contracts

**Files:**

- Create: `src-tauri/src/media/export_presets.rs`
- Create: `src-tauri/src/media/export_paths.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/media/trim_exporter.rs`
- Test: inline unit tests in the two new modules and `trim_exporter.rs`

- [ ] **Step 1: Write failing preset tests**

Add this test module content to the new file `src-tauri/src/media/export_presets.rs` before implementation:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_fixed_mvp_dimensions() {
        assert_eq!(ExportPreset::Bilibili.spec().width, 1920);
        assert_eq!(ExportPreset::Bilibili.spec().height, 1080);
        assert_eq!(ExportPreset::Douyin.spec().width, 1080);
        assert_eq!(ExportPreset::Douyin.spec().height, 1920);
        assert_eq!(ExportPreset::Xiaohongshu.spec().width, 1080);
        assert_eq!(ExportPreset::Xiaohongshu.spec().height, 1080);
    }

    #[test]
    fn preset_parse_rejects_templates_or_unknown_values() {
        assert_eq!("bilibili".parse::<ExportPreset>().unwrap(), ExportPreset::Bilibili);
        assert_eq!("douyin".parse::<ExportPreset>().unwrap(), ExportPreset::Douyin);
        assert_eq!(
            "xiaohongshu".parse::<ExportPreset>().unwrap(),
            ExportPreset::Xiaohongshu
        );
        assert!("template".parse::<ExportPreset>().is_err());
        assert!("youtube-shorts".parse::<ExportPreset>().is_err());
    }
}
```

Also add this line to `src-tauri/src/media/mod.rs` before running the failing test:

```rust
pub mod export_presets;
```

- [ ] **Step 2: Run preset test and verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_presets
```

Expected: FAIL because `ExportPreset` and `ExportPresetSpec` are not implemented in `export_presets.rs`.

- [ ] **Step 3: Implement preset model**

Replace `src-tauri/src/media/export_presets.rs` with:

```rust
/// Fixed export preset selected by the frontend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportPreset {
    Bilibili,
    Douyin,
    Xiaohongshu,
}

/// Scaling policy for fixed MVP presets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportScalePolicy {
    FitWithBars,
    CenterCrop,
}

/// Immutable export settings for an MVP preset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportPresetSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_bitrate_kbps: u32,
    pub audio_bitrate_kbps: u32,
    pub scale_policy: ExportScalePolicy,
}

impl ExportPreset {
    pub fn spec(self) -> ExportPresetSpec {
        match self {
            ExportPreset::Bilibili => ExportPresetSpec {
                id: "bilibili",
                display_name: "Bilibili / YouTube",
                width: 1920,
                height: 1080,
                fps: 30,
                video_bitrate_kbps: 8_000,
                audio_bitrate_kbps: 192,
                scale_policy: ExportScalePolicy::FitWithBars,
            },
            ExportPreset::Douyin => ExportPresetSpec {
                id: "douyin",
                display_name: "抖音",
                width: 1080,
                height: 1920,
                fps: 30,
                video_bitrate_kbps: 8_000,
                audio_bitrate_kbps: 192,
                scale_policy: ExportScalePolicy::CenterCrop,
            },
            ExportPreset::Xiaohongshu => ExportPresetSpec {
                id: "xiaohongshu",
                display_name: "小红书",
                width: 1080,
                height: 1080,
                fps: 30,
                video_bitrate_kbps: 6_000,
                audio_bitrate_kbps: 192,
                scale_policy: ExportScalePolicy::CenterCrop,
            },
        }
    }
}

impl std::str::FromStr for ExportPreset {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "bilibili" => Ok(Self::Bilibili),
            "douyin" => Ok(Self::Douyin),
            "xiaohongshu" => Ok(Self::Xiaohongshu),
            other => Err(format!("未知导出预设：{other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_fixed_mvp_dimensions() {
        assert_eq!(ExportPreset::Bilibili.spec().width, 1920);
        assert_eq!(ExportPreset::Bilibili.spec().height, 1080);
        assert_eq!(ExportPreset::Douyin.spec().width, 1080);
        assert_eq!(ExportPreset::Douyin.spec().height, 1920);
        assert_eq!(ExportPreset::Xiaohongshu.spec().width, 1080);
        assert_eq!(ExportPreset::Xiaohongshu.spec().height, 1080);
    }

    #[test]
    fn preset_parse_rejects_templates_or_unknown_values() {
        assert_eq!("bilibili".parse::<ExportPreset>().unwrap(), ExportPreset::Bilibili);
        assert_eq!("douyin".parse::<ExportPreset>().unwrap(), ExportPreset::Douyin);
        assert_eq!(
            "xiaohongshu".parse::<ExportPreset>().unwrap(),
            ExportPreset::Xiaohongshu
        );
        assert!("template".parse::<ExportPreset>().is_err());
        assert!("youtube-shorts".parse::<ExportPreset>().is_err());
    }
}
```

- [ ] **Step 4: Write failing output path tests**

Create `src-tauri/src/media/export_paths.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::export_presets::ExportPreset;

    #[test]
    fn export_path_is_separate_from_source_path() {
        let source = std::env::temp_dir()
            .join("luzhi-export-path-test")
            .join("raw.mp4");
        let output = export_output_path(&source, ExportPreset::Bilibili, 7).unwrap();

        assert_ne!(output, source);
        assert!(output.to_string_lossy().contains("bilibili"));
        assert!(output.to_string_lossy().ends_with(".mp4"));
    }

    #[test]
    fn non_empty_output_requires_existing_non_empty_file() {
        let path = std::env::temp_dir().join("luzhi-empty-output-test.mp4");
        let _ = std::fs::remove_file(&path);

        assert!(validate_non_empty_output(&path).is_err());

        std::fs::write(&path, b"not empty").unwrap();
        assert!(validate_non_empty_output(&path).is_ok());
        let _ = std::fs::remove_file(path);
    }
}
```

Also add this line to `src-tauri/src/media/mod.rs` before running the failing test:

```rust
pub mod export_paths;
```

- [ ] **Step 5: Run output path test and verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_paths
```

Expected: FAIL because `export_output_path()` and `validate_non_empty_output()` do not exist.

- [ ] **Step 6: Implement output path helper**

Replace `src-tauri/src/media/export_paths.rs` with:

```rust
use std::path::{Path, PathBuf};

use crate::app::error::{AppError, AppResult};
use crate::media::export_presets::ExportPreset;

pub fn export_output_path(
    source_path: &Path,
    preset: ExportPreset,
    sequence: u64,
) -> AppResult<PathBuf> {
    let parent = source_path.parent().ok_or_else(|| AppError::RecordingWriteFailed {
        reason: "原始录制文件没有父目录，无法生成导出路径".to_string(),
    })?;
    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| AppError::RecordingWriteFailed {
            reason: "原始录制文件名无效，无法生成导出路径".to_string(),
        })?;
    let preset_id = preset.spec().id;
    Ok(parent.join(format!("{stem}-{preset_id}-export-{sequence}.mp4")))
}

pub fn validate_non_empty_output(path: &Path) -> AppResult<()> {
    let metadata = std::fs::metadata(path).map_err(|error| AppError::RecordingWriteFailed {
        reason: format!("导出文件不存在或不可访问: {error}"),
    })?;
    if metadata.len() == 0 {
        return Err(AppError::RecordingWriteFailed {
            reason: "导出文件为空，不能返回 outputPath".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::export_presets::ExportPreset;

    #[test]
    fn export_path_is_separate_from_source_path() {
        let source = std::env::temp_dir()
            .join("luzhi-export-path-test")
            .join("raw.mp4");
        let output = export_output_path(&source, ExportPreset::Bilibili, 7).unwrap();

        assert_ne!(output, source);
        assert!(output.to_string_lossy().contains("bilibili"));
        assert!(output.to_string_lossy().ends_with(".mp4"));
    }

    #[test]
    fn non_empty_output_requires_existing_non_empty_file() {
        let path = std::env::temp_dir().join("luzhi-empty-output-test.mp4");
        let _ = std::fs::remove_file(&path);

        assert!(validate_non_empty_output(&path).is_err());

        std::fs::write(&path, b"not empty").unwrap();
        assert!(validate_non_empty_output(&path).is_ok());
        let _ = std::fs::remove_file(path);
    }
}
```

This helper intentionally validates only existence and non-empty size. Real media playability, dimensions, duration, and stream presence are verified later by the FFmpeg inspection tests in Task 6.

- [ ] **Step 7: Wire modules and move preset import**

Modify `src-tauri/src/media/mod.rs`:

```rust
pub mod audio_mixer;
pub mod audio_synchronizer;
pub mod cursor_engine;
pub mod export_paths;
pub mod export_presets;
#[cfg(feature = "ffmpeg")]
pub mod ffmpeg_writer;
pub mod mic_level;
pub mod recording_metadata;
pub mod recording_writer;
pub mod silence_detector;
pub mod trim_exporter;
pub mod trim_metadata;
```

Modify `src-tauri/src/media/trim_exporter.rs`:

```rust
use std::path::PathBuf;

#[cfg(feature = "ffmpeg")]
use crate::app::error::AppError;
use crate::app::error::AppResult;
use crate::core::cut::CutTimeline;
use crate::media::export_presets::ExportPreset;
```

Delete the old local `ExportPreset` enum and its `FromStr` impl from `trim_exporter.rs`. Keep the existing tests, updating imports to use `crate::media::export_presets::ExportPreset`.

- [ ] **Step 8: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/media/mod.rs src-tauri/src/media/export_presets.rs src-tauri/src/media/export_paths.rs src-tauri/src/media/trim_exporter.rs
git commit -m "feat(export): 固定三种导出预设与输出路径"
```

## Task 2: Sensitivity-Independent Base RMS Buckets

**Files:**

- Create: `src-tauri/src/media/trim_audio_activity.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/media/trim_metadata.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline unit tests in `trim_audio_activity.rs`, `trim_metadata.rs`, and existing cut timeline tests

- [ ] **Step 1: Write failing base bucket aggregation tests**

Create `src-tauri/src/media/trim_audio_activity.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cut::{TrimConfig, TrimSensitivity};
    use crate::core::frame::{MediaTimestamp, MixedAudioChunk};
    use std::sync::Arc;

    fn chunk(start: u64, sample_count: usize, value: f32) -> MixedAudioChunk {
        MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(start),
            sample_rate: 10,
            channels: 1,
            samples: Arc::from(vec![value; sample_count].into_boxed_slice()),
        }
    }

    #[test]
    fn base_analyzer_emits_100ms_buckets() {
        let mut analyzer = BaseAudioActivityAnalyzer::default();
        let samples = analyzer.push_chunk(&chunk(0, 10, 0.5));

        assert_eq!(samples.len(), 10);
        assert_eq!(samples[0].start.nanos, 0);
        assert_eq!(samples[0].end.nanos, BASE_RMS_BUCKET_NANOS);
        assert!((samples[0].rms() - 0.5).abs() < 0.001);
    }

    #[test]
    fn aggregate_base_buckets_uses_current_sensitivity_window() {
        let buckets = (0..10)
            .map(|i| BaseAudioActivitySample {
                start: MediaTimestamp::from_nanos(i * BASE_RMS_BUCKET_NANOS),
                end: MediaTimestamp::from_nanos((i + 1) * BASE_RMS_BUCKET_NANOS),
                sum_squares: 0.0,
                sample_count: 1,
            })
            .collect::<Vec<_>>();

        let high = aggregate_base_audio_activity(
            &buckets,
            TrimConfig::from_sensitivity(TrimSensitivity::High),
        );
        let low = aggregate_base_audio_activity(
            &buckets,
            TrimConfig::from_sensitivity(TrimSensitivity::Low),
        );

        assert_eq!(high.len(), 2);
        assert_eq!(high[0].end.nanos - high[0].start.nanos, 500_000_000);
        assert_eq!(low.len(), 1);
        assert_eq!(low[0].end.nanos - low[0].start.nanos, 1_000_000_000);
    }

    #[test]
    fn base_analyzer_accumulates_short_chunks_before_emitting_bucket() {
        let mut analyzer = BaseAudioActivityAnalyzer::default();

        // Five 20ms chunks should produce exactly one 100ms bucket, not five
        // mislabeled 100ms buckets.
        for i in 0..4 {
            let samples = analyzer.push_chunk(&MixedAudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 20_000_000),
                sample_rate: 1_000,
                channels: 1,
                samples: Arc::from(vec![0.25; 20].into_boxed_slice()),
            });
            assert!(samples.is_empty());
        }

        let samples = analyzer.push_chunk(&MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(80_000_000),
            sample_rate: 1_000,
            channels: 1,
            samples: Arc::from(vec![0.25; 20].into_boxed_slice()),
        });

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].start.nanos, 0);
        assert_eq!(samples[0].end.nanos, BASE_RMS_BUCKET_NANOS);
        assert_eq!(samples[0].sample_count, 100);
    }
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml trim_audio_activity
```

Expected: FAIL because base bucket types/functions do not exist.

- [ ] **Step 3: Implement base bucket module**

Replace `src-tauri/src/media/trim_audio_activity.rs` with:

```rust
use crate::core::cut::{AudioActivitySample, TrimConfig};
use crate::core::frame::{MediaTimestamp, MixedAudioChunk};

pub const BASE_RMS_BUCKET_NANOS: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseAudioActivitySample {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub sum_squares: f64,
    pub sample_count: u64,
}

impl BaseAudioActivitySample {
    pub fn rms(&self) -> f32 {
        if self.sample_count == 0 {
            return 0.0;
        }
        (self.sum_squares / self.sample_count as f64).sqrt() as f32
    }
}

#[derive(Default)]
pub struct BaseAudioActivityAnalyzer {
    bucket_start_nanos: Option<u64>,
    sum_squares: f64,
    sample_count: u64,
}

impl BaseAudioActivityAnalyzer {
    pub fn push_chunk(&mut self, chunk: &MixedAudioChunk) -> Vec<BaseAudioActivitySample> {
        if chunk.sample_rate == 0 || chunk.channels == 0 || chunk.samples.is_empty() {
            return Vec::new();
        }

        let frames = chunk.samples.len() as u64 / chunk.channels as u64;
        if frames == 0 {
            return Vec::new();
        }

        let mut output = Vec::new();
        for frame in 0..frames {
            let frame_nanos = chunk
                .timestamp
                .nanos
                .saturating_add(frame.saturating_mul(1_000_000_000) / chunk.sample_rate as u64);

            let bucket_start = self.bucket_start_nanos.get_or_insert(frame_nanos);
            if frame_nanos >= bucket_start.saturating_add(BASE_RMS_BUCKET_NANOS)
                && self.sample_count > 0
            {
                output.push(self.take_bucket());
                self.bucket_start_nanos = Some(frame_nanos);
            }

            for channel in 0..chunk.channels as u64 {
                let index = (frame * chunk.channels as u64 + channel) as usize;
                if let Some(sample) = chunk.samples.get(index) {
                    let sample = *sample as f64;
                    self.sum_squares += sample * sample;
                    self.sample_count += 1;
                }
            }
        }

        output
    }

    pub fn flush(&mut self) -> Option<BaseAudioActivitySample> {
        if self.sample_count == 0 {
            return None;
        }
        Some(self.take_bucket())
    }

    fn take_bucket(&mut self) -> BaseAudioActivitySample {
        let start = self.bucket_start_nanos.unwrap_or(0);
        let sample = BaseAudioActivitySample {
            start: MediaTimestamp::from_nanos(start),
            end: MediaTimestamp::from_nanos(start.saturating_add(BASE_RMS_BUCKET_NANOS)),
            sum_squares: self.sum_squares,
            sample_count: self.sample_count,
        };
        self.bucket_start_nanos = None;
        self.sum_squares = 0.0;
        self.sample_count = 0;
        sample
    }
}

pub fn aggregate_base_audio_activity(
    buckets: &[BaseAudioActivitySample],
    config: TrimConfig,
) -> Vec<AudioActivitySample> {
    if buckets.is_empty() {
        return Vec::new();
    }

    let mut output = Vec::new();
    let mut window_start = buckets[0].start.nanos;
    let mut window_end = window_start.saturating_add(config.rms_window_nanos);
    let mut sum_squares = 0.0f64;
    let mut sample_count = 0u64;

    for bucket in buckets {
        if bucket.start.nanos >= window_end && sample_count > 0 {
            output.push(activity_sample(window_start, window_end, sum_squares, sample_count));
            window_start = bucket.start.nanos;
            window_end = window_start.saturating_add(config.rms_window_nanos);
            sum_squares = 0.0;
            sample_count = 0;
        }

        sum_squares += bucket.sum_squares;
        sample_count += bucket.sample_count;
    }

    if sample_count > 0 {
        let end = buckets.last().map(|bucket| bucket.end.nanos).unwrap_or(window_end);
        output.push(activity_sample(window_start, end, sum_squares, sample_count));
    }

    output
}

fn activity_sample(
    start: u64,
    end: u64,
    sum_squares: f64,
    sample_count: u64,
) -> AudioActivitySample {
    let rms = if sample_count == 0 {
        0.0
    } else {
        (sum_squares / sample_count as f64).sqrt() as f32
    };
    AudioActivitySample {
        start: MediaTimestamp::from_nanos(start),
        end: MediaTimestamp::from_nanos(end),
        rms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cut::{TrimConfig, TrimSensitivity};
    use crate::core::frame::{MediaTimestamp, MixedAudioChunk};
    use std::sync::Arc;

    fn chunk(start: u64, sample_count: usize, value: f32) -> MixedAudioChunk {
        MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(start),
            sample_rate: 10,
            channels: 1,
            samples: Arc::from(vec![value; sample_count].into_boxed_slice()),
        }
    }

    #[test]
    fn base_analyzer_emits_100ms_buckets() {
        let mut analyzer = BaseAudioActivityAnalyzer::default();
        let samples = analyzer.push_chunk(&chunk(0, 10, 0.5));

        assert_eq!(samples.len(), 10);
        assert_eq!(samples[0].start.nanos, 0);
        assert_eq!(samples[0].end.nanos, BASE_RMS_BUCKET_NANOS);
        assert!((samples[0].rms() - 0.5).abs() < 0.001);
    }

    #[test]
    fn aggregate_base_buckets_uses_current_sensitivity_window() {
        let buckets = (0..10)
            .map(|i| BaseAudioActivitySample {
                start: MediaTimestamp::from_nanos(i * BASE_RMS_BUCKET_NANOS),
                end: MediaTimestamp::from_nanos((i + 1) * BASE_RMS_BUCKET_NANOS),
                sum_squares: 0.0,
                sample_count: 1,
            })
            .collect::<Vec<_>>();

        let high = aggregate_base_audio_activity(
            &buckets,
            TrimConfig::from_sensitivity(TrimSensitivity::High),
        );
        let low = aggregate_base_audio_activity(
            &buckets,
            TrimConfig::from_sensitivity(TrimSensitivity::Low),
        );

        assert_eq!(high.len(), 2);
        assert_eq!(high[0].end.nanos - high[0].start.nanos, 500_000_000);
        assert_eq!(low.len(), 1);
        assert_eq!(low[0].end.nanos - low[0].start.nanos, 1_000_000_000);
    }

    #[test]
    fn base_analyzer_accumulates_short_chunks_before_emitting_bucket() {
        let mut analyzer = BaseAudioActivityAnalyzer::default();

        for i in 0..4 {
            let samples = analyzer.push_chunk(&MixedAudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 20_000_000),
                sample_rate: 1_000,
                channels: 1,
                samples: Arc::from(vec![0.25; 20].into_boxed_slice()),
            });
            assert!(samples.is_empty());
        }

        let samples = analyzer.push_chunk(&MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(80_000_000),
            sample_rate: 1_000,
            channels: 1,
            samples: Arc::from(vec![0.25; 20].into_boxed_slice()),
        });

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].start.nanos, 0);
        assert_eq!(samples[0].end.nanos, BASE_RMS_BUCKET_NANOS);
        assert_eq!(samples[0].sample_count, 100);
    }
}
```

- [ ] **Step 4: Add module export**

Modify `src-tauri/src/media/mod.rs` and add:

```rust
pub mod trim_audio_activity;
```

- [ ] **Step 5: Extend trim metadata with versioned base buckets**

Modify `src-tauri/src/media/trim_metadata.rs` imports:

```rust
use crate::core::cut::{AudioActivitySample, CutTimeline, FrameDiffSample, TrimConfig};
use crate::media::trim_audio_activity::{
    aggregate_base_audio_activity, BaseAudioActivitySample,
};
```

Replace `TrimMetadata` with:

```rust
pub const TRIM_METADATA_SCHEMA_VERSION: u32 = 2;

/// Recording-time metadata used to build a CutTimeline after recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimMetadata {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub duration_nanos: u64,
    #[serde(default)]
    pub base_audio_activity: Vec<BaseAudioActivitySample>,
    #[serde(default)]
    pub audio_activity: Vec<AudioActivitySample>,
    pub visual_activity: Vec<FrameDiffSample>,
    #[serde(default)]
    pub audio_activity_dropped_count: u64,
    #[serde(default)]
    pub visual_activity_dropped_count: u64,
    #[serde(default)]
    pub activity_truncated: bool,
}

fn default_schema_version() -> u32 {
    1
}

impl TrimMetadata {
    pub fn derive_audio_activity(&self, config: TrimConfig) -> Vec<AudioActivitySample> {
        if !self.base_audio_activity.is_empty() {
            return aggregate_base_audio_activity(&self.base_audio_activity, config);
        }
        self.audio_activity.clone()
    }
}
```

Update all `TrimMetadata { ... }` construction sites to include:

```rust
schema_version: crate::media::trim_metadata::TRIM_METADATA_SCHEMA_VERSION,
base_audio_activity: Vec::new(),
```

When constructing new production metadata from the consumer, fill `base_audio_activity` with collected base buckets and leave `audio_activity` empty.

- [ ] **Step 6: Replace recording-time windowed RMS collection**

Modify `src-tauri/src/platform/macos_service.rs` imports:

```rust
use crate::media::trim_audio_activity::BaseAudioActivityAnalyzer;
use crate::media::trim_metadata::{
    TrimMetadata, TrimMetadataWriter, TRIM_METADATA_SCHEMA_VERSION,
};
```

In `consume_frames()`, replace:

```rust
let sensitivity = crate::core::cut::TrimSensitivity::from_str(trim_sensitivity_str)
    .unwrap_or(crate::core::cut::TrimSensitivity::Medium);
let mut rms_analyzer = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(sensitivity));
```

with:

```rust
let mut base_audio_analyzer = BaseAudioActivityAnalyzer::default();
```

Replace each loop over `rms_analyzer.push_chunk(&mixed)` with:

```rust
for sample in base_audio_analyzer.push_chunk(&mixed) {
    if !push_bounded_base_audio_sample(
        &mut base_audio_activity,
        sample,
        MAX_AUDIO_SAMPLES,
    ) {
        audio_dropped += 1;
    }
}
```

After final audio drain, flush the base analyzer once:

```rust
if let Some(sample) = base_audio_analyzer.flush() {
    if !push_bounded_base_audio_sample(&mut base_audio_activity, sample, MAX_AUDIO_SAMPLES) {
        audio_dropped += 1;
    }
}
```

Add helper near existing bounded helpers:

```rust
fn push_bounded_base_audio_sample(
    samples: &mut Vec<crate::media::trim_audio_activity::BaseAudioActivitySample>,
    sample: crate::media::trim_audio_activity::BaseAudioActivitySample,
    max: usize,
) -> bool {
    if samples.len() < max {
        samples.push(sample);
        true
    } else {
        false
    }
}
```

Build `TrimMetadata` with:

```rust
TrimMetadata {
    schema_version: TRIM_METADATA_SCHEMA_VERSION,
    duration_nanos,
    base_audio_activity,
    audio_activity: Vec::new(),
    visual_activity,
    audio_activity_dropped_count: audio_dropped,
    visual_activity_dropped_count: visual_dropped,
    activity_truncated,
}
```

- [ ] **Step 7: Derive audio activity during cut timeline build**

Modify `build_cut_timeline()` in `src-tauri/src/lib.rs`:

```rust
let metadata = TrimMetadataWriter::read_metadata(
    PathBuf::from(&trim_metadata_path_for_blocking).as_path(),
)
.map_err(|error| error.to_string())?;
let audio_activity = metadata.derive_audio_activity(trim_config);
let detector = SilenceDetectorEngine::new(trim_config);
let timeline = detector
    .analyze(
        &audio_activity,
        &metadata.visual_activity,
        metadata.duration_nanos,
    )
    .map_err(|error| error.to_string())?;
```

- [ ] **Step 8: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml trim_
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/media/mod.rs src-tauri/src/media/trim_audio_activity.rs src-tauri/src/media/trim_metadata.rs src-tauri/src/platform/macos_service.rs src-tauri/src/lib.rs
git commit -m "feat(trim): 支持录后灵敏度完整重聚合"
```

## Task 3: Export Service Boundary and Output Validation

**Files:**

- Create: `src-tauri/src/app/export_service.rs`
- Modify: `src-tauri/src/app/mod.rs`
- Modify: `src-tauri/src/app/error.rs`
- Modify: `src-tauri/src/media/trim_exporter.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline unit tests in `export_service.rs`, `macos_service.rs`, and `events.rs`

- [ ] **Step 1: Add export error variants**

Modify `src-tauri/src/app/error.rs` and add variants:

```rust
ExportFailed {
    reason: String,
},
ExportCancelled,
LicenseFailed {
    reason: String,
},
```

Add `Display` arms:

```rust
AppError::ExportFailed { reason } => {
    write!(formatter, "导出失败：{reason}")
}
AppError::ExportCancelled => {
    write!(formatter, "导出已取消")
}
AppError::LicenseFailed { reason } => {
    write!(formatter, "授权状态处理失败：{reason}")
}
```

Add tests:

```rust
#[test]
fn export_error_uses_chinese_message() {
    let error = AppError::ExportFailed {
        reason: "源文件不存在".to_string(),
    };
    assert_eq!(error.to_string(), "导出失败：源文件不存在");
}

#[test]
fn export_cancelled_uses_chinese_message() {
    assert_eq!(AppError::ExportCancelled.to_string(), "导出已取消");
}
```

- [ ] **Step 2: Run error tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_error
```

Expected: PASS after error variants are wired.

- [ ] **Step 3: Extend `TrimExportRequest`**

Modify `src-tauri/src/media/trim_exporter.rs`:

```rust
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::media::export_presets::ExportPreset;
```

Replace `TrimExportRequest` with:

```rust
#[derive(Clone, Debug)]
pub struct ExportProgressReporter {
    callback: Arc<dyn Fn(u8) + Send + Sync>,
}

impl ExportProgressReporter {
    pub fn new(callback: Arc<dyn Fn(u8) + Send + Sync>) -> Self {
        Self { callback }
    }

    pub fn report(&self, progress: u8) {
        (self.callback)(progress.min(100));
    }
}

impl PartialEq for ExportProgressReporter {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

#[derive(Clone, Debug)]
pub struct TrimExportRequest {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub preset: ExportPreset,
    pub cut_timeline: CutTimeline,
    pub effect_timeline_path: Option<PathBuf>,
    pub cancel_token: Arc<AtomicBool>,
    pub progress: Option<ExportProgressReporter>,
}

impl PartialEq for TrimExportRequest {
    fn eq(&self, other: &Self) -> bool {
        self.input_path == other.input_path
            && self.output_path == other.output_path
            && self.preset == other.preset
            && self.cut_timeline == other.cut_timeline
            && self.effect_timeline_path == other.effect_timeline_path
    }
}
```

Update `MockTrimExporter::export()`:

```rust
fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
    if request.cancel_token.load(Ordering::Relaxed) {
        return Err(crate::app::error::AppError::ExportCancelled);
    }
    let result = TrimExportResult {
        output_path: request.output_path.clone(),
        cut_count: request.cut_timeline.cuts.len(),
    };
    if let Some(progress) = &request.progress {
        progress.report(100);
    }
    self.requests.push(request);
    Ok(result)
}
```

Update existing tests to include:

```rust
effect_timeline_path: None,
cancel_token: Arc::new(AtomicBool::new(false)),
progress: None,
```

- [ ] **Step 4: Create export service tests**

Create `src-tauri/src/app/export_service.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cut::CutTimeline;
    use crate::media::export_presets::ExportPreset;
    use crate::media::trim_exporter::MockTrimExporter;
    use std::sync::{atomic::AtomicBool, Arc};

    #[test]
    fn export_service_rejects_missing_source_artifact() {
        let temp = std::env::temp_dir().join("missing-source-artifact.mp4");
        let _ = std::fs::remove_file(&temp);
        let mut exporter = MockTrimExporter::new();
        let cancel = Arc::new(AtomicBool::new(false));

        let result = export_recording_with_timeline(
            &mut exporter,
            temp,
            None,
            ExportPreset::Bilibili,
            CutTimeline::empty(10_000_000_000),
            None,
            cancel,
            None,
            1,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("源文件"));
    }

    #[test]
    fn export_service_sends_structured_request_to_exporter() {
        let dir = std::env::temp_dir().join("luzhi-export-service-test");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("raw.mp4");
        std::fs::write(&source, b"raw").unwrap();
        let mut exporter = MockTrimExporter::new();
        let cancel = Arc::new(AtomicBool::new(false));

        let result = export_recording_with_timeline(
            &mut exporter,
            source.clone(),
            None,
            ExportPreset::Douyin,
            CutTimeline::empty(10_000_000_000),
            None,
            cancel,
            None,
            9,
        )
        .unwrap();

        assert_eq!(result.cut_count, 0);
        assert_eq!(exporter.requests().len(), 1);
        assert_eq!(exporter.requests()[0].input_path, source);
        assert_eq!(exporter.requests()[0].preset, ExportPreset::Douyin);
        let _ = std::fs::remove_dir_all(dir);
    }
}
```

- [ ] **Step 5: Run export service tests and verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_service
```

Expected: FAIL because `export_recording_with_timeline()` does not exist.

- [ ] **Step 6: Implement export service**

Replace `src-tauri/src/app/export_service.rs` with:

```rust
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

use crate::app::error::{AppError, AppResult};
use crate::core::cut::CutTimeline;
use crate::media::export_paths::{export_output_path, validate_non_empty_output};
use crate::media::export_presets::ExportPreset;
use crate::media::trim_exporter::{
    ExportProgressReporter, TrimExportRequest, TrimExportResult, TrimExporter,
};

pub fn export_recording_with_timeline(
    exporter: &mut dyn TrimExporter,
    input_path: PathBuf,
    requested_output_path: Option<PathBuf>,
    preset: ExportPreset,
    cut_timeline: CutTimeline,
    effect_timeline_path: Option<PathBuf>,
    cancel_token: Arc<AtomicBool>,
    progress: Option<ExportProgressReporter>,
    sequence: u64,
) -> AppResult<TrimExportResult> {
    if !input_path.exists() {
        return Err(AppError::ExportFailed {
            reason: format!("源文件不存在: {}", input_path.to_string_lossy()),
        });
    }
    if input_path.metadata().map(|metadata| metadata.len()).unwrap_or(0) == 0 {
        return Err(AppError::ExportFailed {
            reason: "源文件为空，无法导出".to_string(),
        });
    }

    let output_path = requested_output_path
        .unwrap_or(export_output_path(&input_path, preset, sequence)?);
    if output_path == input_path {
        return Err(AppError::ExportFailed {
            reason: "导出文件不能覆盖原始录制文件".to_string(),
        });
    }

    let result = exporter.export(TrimExportRequest {
        input_path,
        output_path,
        preset,
        cut_timeline,
        effect_timeline_path,
        cancel_token,
        progress,
    })?;

    validate_non_empty_output(&result.output_path)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cut::CutTimeline;
    use crate::media::export_presets::ExportPreset;
    use crate::media::trim_exporter::{MockTrimExporter, TrimExportRequest, TrimExportResult, TrimExporter};
    use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

    struct FileCreatingExporter;

    impl TrimExporter for FileCreatingExporter {
        fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
            if request.cancel_token.load(Ordering::Relaxed) {
                return Err(AppError::ExportCancelled);
            }
            std::fs::write(&request.output_path, b"mp4").map_err(|error| AppError::ExportFailed {
                reason: error.to_string(),
            })?;
            Ok(TrimExportResult {
                output_path: request.output_path,
                cut_count: request.cut_timeline.cuts.len(),
            })
        }
    }

    #[test]
    fn export_service_rejects_missing_source_artifact() {
        let temp = std::env::temp_dir().join("missing-source-artifact.mp4");
        let _ = std::fs::remove_file(&temp);
        let mut exporter = MockTrimExporter::new();
        let cancel = Arc::new(AtomicBool::new(false));

        let result = export_recording_with_timeline(
            &mut exporter,
            temp,
            None,
            ExportPreset::Bilibili,
            CutTimeline::empty(10_000_000_000),
            None,
            cancel,
            None,
            1,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("源文件"));
    }

    #[test]
    fn export_service_sends_structured_request_to_exporter() {
        let dir = std::env::temp_dir().join("luzhi-export-service-test");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("raw.mp4");
        std::fs::write(&source, b"raw").unwrap();
        let mut exporter = FileCreatingExporter;
        let cancel = Arc::new(AtomicBool::new(false));

        let result = export_recording_with_timeline(
            &mut exporter,
            source.clone(),
            None,
            ExportPreset::Douyin,
            CutTimeline::empty(10_000_000_000),
            None,
            cancel,
            None,
            9,
        )
        .unwrap();

        assert_eq!(result.cut_count, 0);
        assert!(result.output_path.exists());
        assert_ne!(result.output_path, source);
        let _ = std::fs::remove_dir_all(dir);
    }
}
```

- [ ] **Step 7: Export module**

Modify `src-tauri/src/app/mod.rs`:

```rust
pub mod cursor_metadata_runtime;
pub mod error;
pub mod events;
pub mod export_service;
pub mod mic_level_runtime;
pub mod permission_service;
pub mod recording_runtime;
pub mod recording_service;
pub mod state_machine;
```

- [ ] **Step 8: Track last source artifact**

Modify `src-tauri/src/platform/macos_service.rs` struct:

```rust
last_recording_output_path: Option<String>,
```

Initialize it to `None` in `new()`, clear it in `start()`, set it in `stop()` after `result` is available:

```rust
self.last_recording_output_path = result.output_path.clone();
```

Add method:

```rust
pub fn last_recording_output_path(&self) -> Option<String> {
    self.last_recording_output_path.clone()
}
```

- [ ] **Step 9: Wire `export_video()` to structured request**

Modify `src-tauri/src/lib.rs` `export_video()` after cursor/cut timeline creation:

```rust
use crate::app::export_service::export_recording_with_timeline;
use crate::media::export_presets::ExportPreset;
use crate::media::trim_exporter::MockTrimExporter;
use crate::media::trim_metadata::TrimMetadataWriter;
use std::sync::{atomic::AtomicBool, Arc};

let export_preset: ExportPreset = preset.parse()?;
```

Read source and cut timeline:

```rust
let source_path = {
    let service = state
        .service
        .lock()
        .map_err(|_| "录制服务锁已损坏".to_string())?;
    service
        .last_recording_output_path()
        .ok_or_else(|| "没有可用的原始录制文件，请先完成一次可播放录制".to_string())?
};
let cut_timeline = if let Some(path) = cut.as_ref().map(|summary| summary.cut_timeline_path.clone()) {
    TrimMetadataWriter::read_cut_timeline(PathBuf::from(path).as_path())
        .map_err(|error| error.to_string())?
} else {
    let trim_metadata_path = {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        service.last_trim_metadata_path()
    };
    let duration_nanos = if let Some(path) = trim_metadata_path {
        TrimMetadataWriter::read_metadata(PathBuf::from(path).as_path())
            .map_err(|error| error.to_string())?
            .duration_nanos
    } else {
        0
    };
    CutTimeline::empty(duration_nanos)
};
```

Until the production FFmpeg exporter is enabled, call `MockTrimExporter` only in a pure test helper, not the product command. In product command, keep returning `output_path: None` with explicit Gate copy until Task 6 is complete:

```rust
let output_path = None;
```

Add a pure helper test in `lib.rs` or `export_service.rs` that proves command data can form `TrimExportRequest`; do not return a fake `outputPath`.

Add this assertion to the auto-trim-off test path:

```rust
assert!(exporter.requests()[0].cut_timeline.cuts.is_empty());
assert!(exporter.requests()[0].cut_timeline.duration_nanos > 0);
```

- [ ] **Step 10: Run focused export boundary tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_
```

Expected: PASS.

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/app/mod.rs src-tauri/src/app/error.rs src-tauri/src/app/export_service.rs src-tauri/src/media/trim_exporter.rs src-tauri/src/platform/macos_service.rs src-tauri/src/lib.rs
git commit -m "feat(export): 接入结构化导出服务边界"
```

## Task 4: Export Progress and Cancel Runtime

**Files:**

- Modify: `src-tauri/src/app/events.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Test: `src-tauri/src/app/events.rs`, `src/App.test.tsx`

- [ ] **Step 1: Add progress payload tests**

Add to `src-tauri/src/app/events.rs` tests:

```rust
#[test]
fn export_progress_serializes_camel_case() {
    let payload = ExportProgressPayload {
        preset: "bilibili",
        progress: 45,
        cancellable: true,
        output_path: None,
        error: None,
    };
    let json = serde_json::to_string(&payload).unwrap();
    assert!(json.contains("\"preset\":\"bilibili\""));
    assert!(json.contains("\"progress\":45"));
    assert!(json.contains("\"cancellable\":true"));
    assert!(json.contains("\"outputPath\":null"));
}
```

- [ ] **Step 2: Implement progress payload**

Add to `src-tauri/src/app/events.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgressPayload {
    pub preset: &'static str,
    pub progress: u8,
    pub cancellable: bool,
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
```

- [ ] **Step 3: Add export cancel state**

Modify `AppState` in `src-tauri/src/lib.rs`:

```rust
export_cancel_token: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>>,
export_sequence: Arc<AtomicU64>,
```

Initialize in `Default`:

```rust
export_cancel_token: Arc::new(Mutex::new(None)),
export_sequence: Arc::new(AtomicU64::new(0)),
```

- [ ] **Step 4: Add `cancel_export` command**

Add to `src-tauri/src/lib.rs`:

```rust
#[tauri::command]
fn cancel_export(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let guard = state
        .export_cancel_token
        .lock()
        .map_err(|_| "导出取消状态锁已损坏".to_string())?;
    if let Some(token) = guard.as_ref() {
        token.store(true, Ordering::Relaxed);
    }
    Ok(())
}
```

Register it in `generate_handler!`.

- [ ] **Step 5: Emit export progress**

In `export_video()`, after parsing preset:

```rust
let export_preset: ExportPreset = preset.parse()?;
let preset_id = export_preset.spec().id;
let cancel_token = Arc::new(std::sync::atomic::AtomicBool::new(false));
{
    let mut guard = state
        .export_cancel_token
        .lock()
        .map_err(|_| "导出取消状态锁已损坏".to_string())?;
    *guard = Some(cancel_token.clone());
}
    let _ = app.emit(
        "export-progress",
    ExportProgressPayload {
        preset: preset_id,
        progress: 0,
        cancellable: true,
        output_path: None,
        error: None,
    },
    );
```

Create a progress reporter that the exporter can call from packet/frame batches:

```rust
let progress_app = app.clone();
let progress = ExportProgressReporter::new(Arc::new(move |progress| {
    let _ = progress_app.emit(
        "export-progress",
        ExportProgressPayload {
            preset: preset_id,
            progress,
            cancellable: progress < 100,
            output_path: None,
            error: None,
        },
    );
}));
```

Pass `Some(progress)` into `export_recording_with_timeline()`. The FFmpeg exporter must report progress based on processed keep-duration or encoded frame/packet timestamp, not just start/end events.

Add a focused Rust test in `src-tauri/src/app/export_service.rs` or `src-tauri/src/media/trim_exporter.rs` to prove the progress reporter is a real exporter callback, not cosmetic UI state:

```rust
use std::sync::Mutex;

#[test]
fn export_service_forwards_intermediate_progress_from_exporter() {
    let dir = std::env::temp_dir().join("luzhi-export-progress-test");
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("raw.mp4");
    std::fs::write(&source, b"raw").unwrap();

    struct ProgressingExporter;
    impl TrimExporter for ProgressingExporter {
        fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
            let progress = request.progress.as_ref().expect("progress reporter");
            progress.report(8);
            progress.report(47);
            progress.report(91);
            std::fs::write(&request.output_path, b"mp4").map_err(|error| AppError::ExportFailed {
                reason: error.to_string(),
            })?;
            progress.report(100);
            Ok(TrimExportResult {
                output_path: request.output_path,
                cut_count: request.cut_timeline.cuts.len(),
            })
        }
    }

    let observed = Arc::new(Mutex::new(Vec::<u8>::new()));
    let observed_for_callback = observed.clone();
    let reporter = ExportProgressReporter::new(Arc::new(move |progress| {
        observed_for_callback.lock().unwrap().push(progress);
    }));

    let mut exporter = ProgressingExporter;
    export_recording_with_timeline(
        &mut exporter,
        source,
        None,
        ExportPreset::Bilibili,
        CutTimeline::empty(3_000_000_000),
        None,
        Arc::new(AtomicBool::new(false)),
        Some(reporter),
        1,
    )
    .unwrap();

    let observed = observed.lock().unwrap();
    assert!(observed.iter().any(|progress| *progress > 0 && *progress < 100));
    assert_eq!(observed.last().copied(), Some(100));
    let _ = std::fs::remove_dir_all(dir);
}
```

Before returning success:

```rust
let _ = app.emit(
    "export-progress",
    ExportProgressPayload {
        preset: preset_id,
        progress: 100,
        cancellable: false,
        output_path: output_path.clone(),
        error: None,
    },
);
```

On error/cancel, emit with `progress: 0`, `cancellable: false`, `error: Some(message)`.

Use this cleanup pattern so the token is cleared on success and error paths:

```rust
let result = async {
    // build timelines and run export here
    Ok::<ExportSummaryPayload, String>(summary)
}
.await;

{
    let mut guard = state
        .export_cancel_token
        .lock()
        .map_err(|_| "导出取消状态锁已损坏".to_string())?;
    *guard = None;
}

result
```

- [ ] **Step 6: Wire TypeScript commands/events**

Modify `src/lib/tauri.ts`:

```ts
export type ExportProgressPayload = {
  preset: ExportPreset
  progress: number
  cancellable: boolean
  outputPath: string | null
  error?: string
}

export async function cancelExport(): Promise<void> {
  return invoke('cancel_export')
}

export function onExportProgress(callback: (payload: ExportProgressPayload) => void): Promise<UnlistenFn> {
  return listen<ExportProgressPayload>('export-progress', (event) => {
    callback(event.payload)
  })
}
```

- [ ] **Step 7: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_progress_serializes_camel_case
cargo test --manifest-path src-tauri/Cargo.toml export_service_forwards_intermediate_progress_from_exporter
npm test -- --run src/App.test.tsx
```

Expected: Rust test PASS. Frontend should still PASS before UI changes because new TS functions are unused.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/app/events.rs src-tauri/src/lib.rs src/lib/tauri.ts
git commit -m "feat(export): 增加导出进度与取消命令"
```

## Task 5: Preview Export UI

**Files:**

- Modify: `src/components/preview-view.tsx`
- Modify: `src/App.test.tsx`
- Test: `src/App.test.tsx`

- [ ] **Step 1: Add frontend test for playable export success**

Add to `src/App.test.tsx`:

```tsx
it('shows playable export success when outputPath is returned', async () => {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'get_beautify_config') {
      return Promise.resolve({
        cursorMagnification: true,
        magnificationFactor: 2,
        cursorSmoothing: true,
        autoTrimSilences: false,
        trimSensitivity: 'medium',
      })
    }
    if (command === 'set_beautify_config') return Promise.resolve()
    if (command === 'export_video') {
      return Promise.resolve({
        frameCount: 1,
        clickEffectCount: 0,
        effectTimelinePath: '/tmp/effects.json',
        cutCount: 0,
        totalCutNanos: 0,
        cutTimelinePath: null,
        outputPath: '/tmp/luzhi-export.mp4',
      })
    }
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)
  await screen.findByText('预览与美化')

  const exportButtons = screen.getAllByRole('button', { name: '导出' })
  await act(async () => {
    fireEvent.click(exportButtons[0])
  })

  await vi.waitFor(() => {
    expect(screen.getByText('已生成可播放导出文件')).toBeTruthy()
  })
  expect(screen.queryByText(/FFmpeg 编码器接入后/)).toBeNull()
})
```

- [ ] **Step 2: Add frontend test for cancel button**

Add:

```tsx
it('can request export cancellation from preview', async () => {
  let exportResolve: (value: unknown) => void = () => {}
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'get_beautify_config') {
      return Promise.resolve({
        cursorMagnification: true,
        magnificationFactor: 2,
        cursorSmoothing: true,
        autoTrimSilences: false,
        trimSensitivity: 'medium',
      })
    }
    if (command === 'set_beautify_config') return Promise.resolve()
    if (command === 'export_video') {
      return new Promise((resolve) => { exportResolve = resolve })
    }
    if (command === 'cancel_export') return Promise.resolve()
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)
  await screen.findByText('预览与美化')

  const exportButtons = screen.getAllByRole('button', { name: '导出' })
  await act(async () => {
    fireEvent.click(exportButtons[0])
  })

  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: '取消导出' }))
  })

  expect(invokeMock).toHaveBeenCalledWith('cancel_export', undefined)
  await act(async () => {
    exportResolve({
      frameCount: 1,
      clickEffectCount: 0,
      effectTimelinePath: '/tmp/effects.json',
      cutCount: 0,
      totalCutNanos: 0,
      cutTimelinePath: null,
      outputPath: null,
    })
  })
})
```

- [ ] **Step 3: Implement Preview UI state**

Modify imports in `src/components/preview-view.tsx`:

```tsx
import {
  buildCursorEffectTimeline,
  buildCutTimeline,
  cancelExport,
  exportVideo,
  getBeautifyConfig,
  onExportProgress,
  setBeautifyConfig,
  type BeautifyConfig,
  type ExportPreset,
  type ExportProgressPayload,
  type ExportSummary,
  type RecordingResult,
} from '@/lib/tauri'
```

Add state:

```tsx
const [exportProgress, setExportProgress] = useState<ExportProgressPayload | null>(null)
const [isExporting, setIsExporting] = useState(false)
```

Add listener:

```tsx
useEffect(() => {
  let unlisten: (() => void) | undefined
  void onExportProgress((payload) => {
    setExportProgress(payload)
    setIsExporting(payload.cancellable && payload.progress < 100)
  }).then((fn) => { unlisten = fn })
  return () => { unlisten?.() }
}, [])
```

Update `handleExport()`:

```tsx
const handleExport = (preset: ExportPreset) => {
  const revision = exportRevisionRef.current
  setIsExporting(true)
  setExportProgress({ preset, progress: 0, cancellable: true, outputPath: null })
  void flushPendingConfig()
    .then(() => exportVideo(preset))
    .then((summary) => {
      if (revision !== exportRevisionRef.current) return
      setBeautifyError(null)
      setExportSummary(summary)
    })
    .catch((error) => {
      const msg = messageForBeautifyError(error, '导出失败，请重试或检查录制素材。')
      if (!msg) return
      console.error('导出失败', error)
      setBeautifyError(msg)
    })
    .finally(() => {
      setIsExporting(false)
    })
}
```

Add handler:

```tsx
const handleCancelExport = () => {
  void cancelExport().catch((error) => {
    console.error('取消导出失败', error)
  })
}
```

Update summary UI:

```tsx
{exportProgress && isExporting && (
  <div className="mb-3 rounded-lg border border-border/50 bg-secondary/30 p-3 text-xs text-muted-foreground">
    <div className="mb-2 flex items-center justify-between">
      <span>正在导出 {exportProgress.progress}%</span>
      <Button
        size="sm"
        variant="ghost"
        className="h-7 px-2 text-xs"
        onClick={handleCancelExport}
      >
        取消导出
      </Button>
    </div>
    <div className="h-1.5 overflow-hidden rounded-full bg-muted">
      <div
        className="h-full rounded-full bg-foreground transition-all"
        style={{ width: `${Math.max(0, Math.min(exportProgress.progress, 100))}%` }}
      />
    </div>
  </div>
)}
{exportSummary && (
  <div className="mb-3 rounded-lg border border-border/50 bg-secondary/30 p-3 text-xs text-muted-foreground space-y-1">
    {exportSummary.cutCount > 0 ? (
      <p>
        已生成裁剪时间线：{exportSummary.cutCount} 段，预计剪除{' '}
        {Math.round(exportSummary.totalCutNanos / 1_000_000_000)} 秒
      </p>
    ) : (
      <p>未检测到可裁剪空白段</p>
    )}
    {exportSummary.outputPath ? (
      <p className="opacity-80">已生成可播放导出文件</p>
    ) : (
      <p className="opacity-60">FFmpeg 编码器接入后将生成可播放文件</p>
    )}
  </div>
)}
```

Disable export buttons during export:

```tsx
<Button
  size="sm"
  disabled={isExporting}
  className="h-8 px-4 rounded-lg bg-surface hover:bg-surface-hover text-foreground border border-border/50"
  onClick={() => handleExport(preset.id)}
>
  导出
</Button>
```

The `style={{ width: ... }}` line is allowed here only for dynamic progress-bar width. Keep all static styling in Tailwind.

- [ ] **Step 4: Run frontend tests**

Run:

```bash
npm test -- --run src/App.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/preview-view.tsx src/App.test.tsx
git commit -m "feat(ui): 展示导出进度与可播放结果"
```

## Task 6A: Original Recording Artifact Writer

**Files:**

- Create: `src-tauri/src/media/original_recording_artifact.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/media/ffmpeg_writer.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Test: inline Rust tests and FFmpeg feature tests

This task is mandatory before playable export work. Without a reliable original recording artifact, `export_video()` has no safe source file and Phase 6 must not claim any export preset is complete.

- [ ] **Step 1: Add source artifact validation tests**

Create `src-tauri/src/media/original_recording_artifact.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_artifact_requires_existing_non_empty_file() {
        let path = std::env::temp_dir().join("luzhi-missing-source-artifact.mp4");
        let _ = std::fs::remove_file(&path);

        assert!(validate_source_artifact(&path).is_err());

        std::fs::write(&path, b"source").unwrap();
        assert!(validate_source_artifact(&path).is_ok());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_artifact_error_mentions_original_recording() {
        let path = std::env::temp_dir().join("luzhi-empty-source-artifact.mp4");
        std::fs::write(&path, []).unwrap();
        let error = validate_source_artifact(&path).unwrap_err().to_string();

        assert!(error.contains("原始录制文件"));
        let _ = std::fs::remove_file(path);
    }
}
```

- [ ] **Step 2: Implement source artifact validator**

Replace `src-tauri/src/media/original_recording_artifact.rs` with:

```rust
use std::path::Path;

use crate::app::error::{AppError, AppResult};

pub fn validate_source_artifact(path: &Path) -> AppResult<()> {
    let metadata = std::fs::metadata(path).map_err(|error| AppError::RecordingWriteFailed {
        reason: format!("原始录制文件不存在或不可访问: {error}"),
    })?;
    if metadata.len() == 0 {
        return Err(AppError::RecordingWriteFailed {
            reason: "原始录制文件为空，不能用于导出".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_artifact_requires_existing_non_empty_file() {
        let path = std::env::temp_dir().join("luzhi-missing-source-artifact.mp4");
        let _ = std::fs::remove_file(&path);

        assert!(validate_source_artifact(&path).is_err());

        std::fs::write(&path, b"source").unwrap();
        assert!(validate_source_artifact(&path).is_ok());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_artifact_error_mentions_original_recording() {
        let path = std::env::temp_dir().join("luzhi-empty-source-artifact.mp4");
        std::fs::write(&path, []).unwrap();
        let error = validate_source_artifact(&path).unwrap_err().to_string();

        assert!(error.contains("原始录制文件"));
        let _ = std::fs::remove_file(path);
    }
}
```

Add to `src-tauri/src/media/mod.rs`:

```rust
pub mod original_recording_artifact;
```

- [ ] **Step 3: Make `FfmpegRecordingWriter` produce a real source artifact**

Add feature-gated tests first under `src-tauri/src/media/ffmpeg_writer.rs`:

```rust
#[cfg(all(test, feature = "ffmpeg"))]
mod ffmpeg_recording_writer_tests {
    use super::*;
    use crate::media::ffmpeg_test_support::{
        inspect_media_artifact, test_audio_chunk_at, test_video_frame_at, unique_media_path,
    };

    #[test]
    fn ffmpeg_recording_writer_creates_playable_source_artifact() {
        let output = unique_media_path("source-recording", "mp4");
        let mut writer = FfmpegRecordingWriter::new(output.clone()).unwrap();

        writer.push_video(test_video_frame_at(0)).unwrap();
        writer.push_video(test_video_frame_at(33_333_333)).unwrap();
        writer.push_audio(test_audio_chunk_at(0)).unwrap();
        writer.push_audio(test_audio_chunk_at(20_000_000)).unwrap();

        let result = writer.finish().unwrap();
        assert_eq!(result.output_path, Some(output.to_string_lossy().to_string()));

        let inspected = inspect_media_artifact(&output).unwrap();
        assert!(inspected.has_video_stream);
        assert!(inspected.has_audio_stream);
        assert!(inspected.file_size_bytes > 0);
        assert!(inspected.duration_nanos > 0);
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn ffmpeg_recording_writer_finish_failure_does_not_return_output_path() {
        let output = unique_media_path("source-recording-failure", "mp4");
        let mut writer = FfmpegRecordingWriter::new(output.clone()).unwrap();

        writer.force_test_finish_failure();
        let error = writer.finish().unwrap_err().to_string();

        assert!(error.contains("录制") || error.contains("FFmpeg"));
        assert!(!output.exists() || std::fs::metadata(&output).unwrap().len() == 0);
        let _ = std::fs::remove_file(output);
    }
}
```

Create `src-tauri/src/media/ffmpeg_test_support.rs` under `#[cfg(any(test, feature = "ffmpeg"))]` if shared helpers are needed. The helper must inspect artifacts with `ffmpeg_next::format::input()` and stream metadata; it must not call `ffprobe`, `ffmpeg`, shell commands, or string-built CLI arguments.
If `force_test_finish_failure()` is needed, keep it behind `#[cfg(test)]` so no test hook is exposed in production builds.

Replace the skeleton behavior in `src-tauri/src/media/ffmpeg_writer.rs` under the `ffmpeg` feature so:

1. `FfmpegRecordingWriter::new(output_path)` initializes FFmpeg and opens an output context.
2. `push_video()` encodes or safely queues video frames without blocking ScreenCaptureKit callbacks.
3. `push_audio()` encodes or safely queues mixed audio chunks.
4. `finish()` finalizes muxing, validates the output file exists and is non-empty, and returns `RecordingResult.output_path = Some(...)`.
5. Any push or finish error is returned through `RecordingWriter` and reaches `RecordingFinalizeFailed`.
6. Partial original recording artifacts are removed on fatal writer failure.
7. `finish()` returns only after `inspect_media_artifact()` confirms a video stream, an audio stream, non-zero duration, and non-zero file size.

Do not switch the default writer until this step passes FFmpeg feature tests and Native Safety review.

- [ ] **Step 4: Switch default recording writer only under `ffmpeg` feature**

In `MacRecordingService::start()`, replace the unconditional counting writer with feature-gated selection:

```rust
#[cfg(feature = "ffmpeg")]
let writer: Box<dyn RecordingWriter> = Box::new(
    crate::media::ffmpeg_writer::FfmpegRecordingWriter::new(recording_output_path())?
);

#[cfg(not(feature = "ffmpeg"))]
let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));
```

Add a helper that generates original recording paths under `std::env::temp_dir().join("luzhi-recordings")`, and ensure it never reuses an existing path.

- [ ] **Step 5: Verify stop result and source validation**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml original_recording_artifact
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_creates_playable_source_artifact
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer
```

Expected: PASS in an FFmpeg-enabled environment. If FFmpeg development libraries are unavailable, record the linker error and keep Task 6A open.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/original_recording_artifact.rs src-tauri/src/media/mod.rs src-tauri/src/media/ffmpeg_writer.rs src-tauri/src/platform/macos_service.rs
git commit -m "feat(record): 产出可导出的原始录制文件"
```

## Task 6: FFmpeg Playable Export Gate

**Files:**

- Create: `src-tauri/src/media/ffmpeg_test_support.rs`
- Modify: `src-tauri/src/media/trim_exporter.rs`
- Modify: `src-tauri/src/media/export_presets.rs`
- Modify: `src-tauri/src/media/export_paths.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/tests/ffmpeg_export.rs`
- Test: feature-gated Rust tests

This task touches FFmpeg binding behavior. It must be implemented in small commits and manually reviewed before becoming the default product path. A task is not complete until tests inspect the generated media container through bindings and prove dimensions, duration, stream presence, original preservation, progress callbacks, and cancel cleanup.

- [ ] **Step 1: Add FFmpeg test support helpers**

Create `src-tauri/src/media/ffmpeg_test_support.rs` and export it from `media/mod.rs` only for tests / `ffmpeg` feature builds:

```rust
#[cfg(any(test, feature = "ffmpeg"))]
pub mod ffmpeg_test_support;
```

The helper module must provide:

```rust
pub struct MediaArtifactInspection {
    pub file_size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_nanos: u64,
    pub has_video_stream: bool,
    pub has_audio_stream: bool,
}

pub fn unique_media_path(prefix: &str, extension: &str) -> PathBuf;
pub fn test_video_frame_at(timestamp_nanos: u64) -> VideoFrameRef;
pub fn test_audio_chunk_at(timestamp_nanos: u64) -> MixedAudioChunk;
pub fn create_synthetic_source_artifact(
    path: &Path,
    width: u32,
    height: u32,
    duration_nanos: u64,
) -> AppResult<()>;
pub fn inspect_media_artifact(path: &Path) -> AppResult<MediaArtifactInspection>;
```

Rules for these helpers:

1. `inspect_media_artifact()` uses `ffmpeg_next::format::input(path)` and stream metadata; no `ffprobe`, no FFmpeg CLI, no shell commands.
2. `create_synthetic_source_artifact()` may use the same internal FFmpeg writer/exporter helpers, but it must write a real file with video and audio streams.
3. `unique_media_path()` must never reuse an existing path.
4. Tests must remove their own generated artifacts.

- [ ] **Step 2: Add feature-gated integration tests**

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
use luzhi_lib::media::ffmpeg_test_support::{
    create_synthetic_source_artifact, inspect_media_artifact, unique_media_path,
};
use luzhi_lib::media::trim_exporter::{
    ExportProgressReporter, FfmpegTrimExporter, TrimExportRequest, TrimExporter,
};

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

    let result = exporter.export(request);
    assert!(result.is_err());
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
        let result = exporter.export(TrimExportRequest {
            input_path: source.clone(),
            output_path: output.clone(),
            preset,
            cut_timeline: CutTimeline::empty(source_inspection.duration_nanos),
            effect_timeline_path: None,
            cancel_token: Arc::new(AtomicBool::new(false)),
            progress: Some(progress),
        }).unwrap();

        assert_eq!(result.output_path, output);
        assert!(source.exists(), "export must preserve the original source artifact");
        let inspection = inspect_media_artifact(&output).unwrap();
        assert_eq!(inspection.width, preset.spec().width);
        assert_eq!(inspection.height, preset.spec().height);
        assert!(inspection.has_video_stream);
        assert!(inspection.has_audio_stream);
        assert!(inspection.file_size_bytes > 0);
        assert!(duration_delta_nanos(inspection.duration_nanos, source_inspection.duration_nanos)
            <= 500_000_000);
        assert!(observed_progress.lock().unwrap().iter().any(|p| *p > 0 && *p < 100));
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
    exporter.export(TrimExportRequest {
        input_path: source.clone(),
        output_path: output.clone(),
        preset: ExportPreset::Bilibili,
        cut_timeline: timeline,
        effect_timeline_path: None,
        cancel_token: Arc::new(AtomicBool::new(false)),
        progress: None,
    }).unwrap();

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

fn duration_delta_nanos(left: u64, right: u64) -> u64 {
    left.max(right) - left.min(right)
}
```

- [ ] **Step 3: Run feature tests and verify the first real-output tests fail before implementation**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_rejects_missing_source
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_applies_cut_timeline_and_preserves_original
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_cancel_removes_partial_output_and_preserves_original
```

Expected before implementation:

- Missing-source rejection may PASS.
- Real-output preset/trim/cancel tests FAIL because exporter does not yet transcode.
- If the environment lacks FFmpeg dev libraries, record the linker error in `tests/phase-6-w11-w12-checklist.md` and keep this task open.

- [ ] **Step 4: Implement real exporter behind `ffmpeg` feature**

Implement `FfmpegTrimExporter::export()` with these non-negotiable rules:

```rust
#[cfg(feature = "ffmpeg")]
impl TrimExporter for FfmpegTrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
        if request.cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
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

        ffmpeg_next::init().map_err(|error| AppError::ExportFailed {
            reason: format!("初始化 FFmpeg 失败: {error}"),
        })?;

        // Required implementation contract:
        // 1. Open input via ffmpeg_next::format::input(&request.input_path).
        // 2. Open output via ffmpeg_next::format::output(&request.output_path).
        // 3. Resolve request.preset.spec() and apply fixed scale/crop policy.
        // 4. Use request.cut_timeline.keeps to copy/transcode only kept ranges.
        // 5. Report intermediate progress from processed keep-duration or encoded timestamp.
        // 6. Check request.cancel_token between packet/frame batches.
        // 7. Remove partial output on cancel or error.
        // 8. Return only after the output context is finalized and inspect_media_artifact()
        //    confirms file size, dimensions, duration, video stream, and audio stream.
        //
        // Human Native Safety Gate must review the concrete FFmpeg context,
        // stream, packet, encoder, decoder, scaler, resampler, timestamp, and
        // resource-release code before this path is enabled by default.

        Err(AppError::ExportFailed {
            reason: "FFmpeg 导出实现需在本步骤补齐并完成人工 Native Safety 审查后启用".to_string(),
        })
    }
}
```

The concrete implementation must replace the final `Err(...)` before Task 6 can be checked off. Do not commit a default product path that still returns this error.

Use an explicit cleanup pattern around the concrete FFmpeg work:

```rust
let result = run_ffmpeg_export(&request);
if result.is_err() || request.cancel_token.load(Ordering::Relaxed) {
    let _ = std::fs::remove_file(&request.output_path);
}
let result = result?;
if let Some(progress) = &request.progress {
    progress.report(100);
}
Ok(result)
```

The implementation must never delete, truncate, or overwrite `request.input_path`.

- [ ] **Step 5: Switch product command only after real output test passes**

In `export_video()`, instantiate the production exporter only under `ffmpeg`:

```rust
#[cfg(feature = "ffmpeg")]
let mut exporter = crate::media::trim_exporter::FfmpegTrimExporter;

#[cfg(not(feature = "ffmpeg"))]
return Err("FFmpeg 导出未启用，请使用带 ffmpeg feature 的构建。".to_string());
```

Then call `export_recording_with_timeline()` in `spawn_blocking`, validate output, and set:

```rust
output_path: Some(export_result.output_path.to_string_lossy().to_string()),
```

- [ ] **Step 6: Add manual FFmpeg verification**

Run the app with the FFmpeg feature enabled:

```bash
npm run tauri -- dev --features ffmpeg
```

Manual scenario:

1. Record a 1080p full-screen sample with system audio or microphone.
2. Confirm `stop_recording` returns a non-null original `outputPath` from Task 6A.
3. Export Bilibili / YouTube 16:9 with auto-trim off.
4. Export Douyin 9:16 with auto-trim off.
5. Export Xiaohongshu 1:1 with auto-trim off.
6. Enable auto-trim and export a sample containing at least 6 seconds of silent still content.
7. Confirm exported file exists, is non-empty, opens in a video player, and audio is present.
8. Record binding-inspected dimensions, duration, stream presence, file size, and duration delta in `tests/phase-6-w11-w12-checklist.md`.
9. Confirm original recording file still exists after every export.
10. Confirm cancellation removes partial output file.

- [ ] **Step 7: Run feature and non-feature test suites**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets
npm test -- --run
```

Expected: PASS. Existing SCK FFI warnings may remain, but no new clippy errors.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/media/ffmpeg_test_support.rs src-tauri/src/media/mod.rs src-tauri/src/media/trim_exporter.rs src-tauri/src/lib.rs src-tauri/tests/ffmpeg_export.rs tests/phase-6-w11-w12-checklist.md
git commit -m "feat(export): 接入可播放 FFmpeg 导出"
```

## Task 7: Local Trial and Activation Status Service

**Files:**

- Create: `src-tauri/src/app/license_service.rs`
- Modify: `src-tauri/src/app/mod.rs`
- Modify: `src-tauri/src/app/events.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Test: inline Rust tests and frontend tests in Task 8

- [ ] **Step 1: Write license service tests**

Create `src-tauri/src/app/license_service.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_status_starts_fourteen_day_trial() {
        let mut trial_store = MemoryTrialStore::default();
        let mut activation_store = MemoryActivationCredentialStore::default();
        let clock = FixedClock { now_secs: 1_000 };
        let service = LicenseService::new(&mut trial_store, &mut activation_store, clock);

        let status = service.status().unwrap();

        assert_eq!(status.kind, LicenseStatusKind::Trial);
        assert_eq!(status.trial_days_remaining, 14);
        assert!(!status.is_expired);
    }

    #[test]
    fn trial_expires_after_fourteen_days() {
        let mut trial_store = MemoryTrialStore {
            state: Some(LocalTrialState {
                trial_started_at_secs: 1_000,
            }),
        };
        let mut activation_store = MemoryActivationCredentialStore::default();
        let clock = FixedClock {
            now_secs: 1_000 + 15 * 24 * 60 * 60,
        };
        let service = LicenseService::new(&mut trial_store, &mut activation_store, clock);

        let status = service.status().unwrap();

        assert_eq!(status.kind, LicenseStatusKind::Expired);
        assert_eq!(status.trial_days_remaining, 0);
        assert!(status.is_expired);
    }

    #[test]
    fn activated_status_comes_from_credential_store_and_overrides_trial_expiry() {
        let mut trial_store = MemoryTrialStore {
            state: Some(LocalTrialState {
                trial_started_at_secs: 1_000,
            }),
        };
        let mut activation_store = MemoryActivationCredentialStore {
            activated_at_secs: Some(2_000),
        };
        let clock = FixedClock {
            now_secs: 1_000 + 30 * 24 * 60 * 60,
        };
        let service = LicenseService::new(&mut trial_store, &mut activation_store, clock);

        let status = service.status().unwrap();

        assert_eq!(status.kind, LicenseStatusKind::Activated);
        assert!(status.activated);
        assert!(!status.is_expired);
    }

    #[test]
    fn file_trial_state_does_not_contain_activation_entitlement() {
        let state = LocalTrialState {
            trial_started_at_secs: 1_000,
        };
        let json = serde_json::to_string(&state).unwrap();

        assert!(json.contains("trialStartedAtSecs"));
        assert!(!json.contains("activated"));
    }

    #[test]
    fn local_trial_state_rejects_activation_entitlement_field() {
        let json = r#"{"trialStartedAtSecs":1000,"activatedAtSecs":2000}"#;
        let error = serde_json::from_str::<LocalTrialState>(json).unwrap_err();

        assert!(error.to_string().contains("unknown field"));
    }
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml license_service
```

Expected: FAIL because the trial store, credential store, and service types do not exist.

- [ ] **Step 3: Implement local license service**

Replace `src-tauri/src/app/license_service.rs` with:

```rust
use serde::{Deserialize, Serialize};

use crate::app::error::{AppError, AppResult};

const TRIAL_SECONDS: u64 = 14 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LicenseStatusKind {
    Trial,
    Expired,
    Activated,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatus {
    pub kind: LicenseStatusKind,
    pub trial_days_remaining: u8,
    pub is_expired: bool,
    pub activated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct LocalTrialState {
    pub trial_started_at_secs: u64,
}

pub trait LicenseClock: Copy {
    fn now_secs(self) -> u64;
}

#[derive(Clone, Copy)]
pub struct SystemLicenseClock;

impl LicenseClock for SystemLicenseClock {
    fn now_secs(self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    }
}

pub trait TrialStore {
    fn read(&mut self) -> AppResult<Option<LocalTrialState>>;
    fn write(&mut self, state: &LocalTrialState) -> AppResult<()>;
}

pub trait ActivationCredentialStore {
    fn activated_at_secs(&mut self) -> AppResult<Option<u64>>;
}

/// File-backed store is acceptable only for the non-secret local trial marker.
pub struct FileTrialStore {
    path: std::path::PathBuf,
}

impl FileTrialStore {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl TrialStore for FileTrialStore {
    fn read(&mut self) -> AppResult<Option<LocalTrialState>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let json = std::fs::read_to_string(&self.path).map_err(|error| AppError::LicenseFailed {
            reason: format!("读取本地授权状态失败: {error}"),
        })?;
        serde_json::from_str(&json).map(Some).map_err(|error| AppError::LicenseFailed {
            reason: format!("解析本地授权状态失败: {error}"),
        })
    }

    fn write(&mut self, state: &LocalTrialState) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| AppError::LicenseFailed {
                reason: format!("创建授权状态目录失败: {error}"),
            })?;
        }
        let json = serde_json::to_string_pretty(state).map_err(|error| AppError::LicenseFailed {
            reason: format!("序列化授权状态失败: {error}"),
        })?;
        std::fs::write(&self.path, json).map_err(|error| AppError::LicenseFailed {
            reason: format!("写入授权状态失败: {error}"),
        })
    }
}

/// Phase 6 product default: no server activation protocol yet, so this returns
/// no entitlement. It preserves the command/interface boundary without storing
/// fake activation in a file.
pub struct NoopActivationCredentialStore;

impl ActivationCredentialStore for NoopActivationCredentialStore {
    fn activated_at_secs(&mut self) -> AppResult<Option<u64>> {
        Ok(None)
    }
}

/// Future production implementation must use macOS Keychain / Windows
/// Credential Manager / platform equivalent and pass Native Safety review.
pub struct SystemCredentialLicenseStore;

impl ActivationCredentialStore for SystemCredentialLicenseStore {
    fn activated_at_secs(&mut self) -> AppResult<Option<u64>> {
        Ok(None)
    }
}

pub struct LicenseService<'a, T, A, C>
where
    T: TrialStore,
    A: ActivationCredentialStore,
    C: LicenseClock,
{
    trial_store: &'a mut T,
    activation_store: &'a mut A,
    clock: C,
}

impl<'a, T, A, C> LicenseService<'a, T, A, C>
where
    T: TrialStore,
    A: ActivationCredentialStore,
    C: LicenseClock,
{
    pub fn new(trial_store: &'a mut T, activation_store: &'a mut A, clock: C) -> Self {
        Self {
            trial_store,
            activation_store,
            clock,
        }
    }

    pub fn status(mut self) -> AppResult<LicenseStatus> {
        let now = self.clock.now_secs();
        let state = match self.trial_store.read()? {
            Some(state) => state,
            None => {
                let state = LocalTrialState {
                    trial_started_at_secs: now,
                };
                self.trial_store.write(&state)?;
                state
            }
        };

        if self.activation_store.activated_at_secs()?.is_some() {
            return Ok(LicenseStatus {
                kind: LicenseStatusKind::Activated,
                trial_days_remaining: 0,
                is_expired: false,
                activated: true,
            });
        }

        let elapsed = now.saturating_sub(state.trial_started_at_secs);
        if elapsed >= TRIAL_SECONDS {
            return Ok(LicenseStatus {
                kind: LicenseStatusKind::Expired,
                trial_days_remaining: 0,
                is_expired: true,
                activated: false,
            });
        }

        let remaining = TRIAL_SECONDS - elapsed;
        let days = ((remaining + 24 * 60 * 60 - 1) / (24 * 60 * 60)).min(14) as u8;
        Ok(LicenseStatus {
            kind: LicenseStatusKind::Trial,
            trial_days_remaining: days,
            is_expired: false,
            activated: false,
        })
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct FixedClock {
    now_secs: u64,
}

#[cfg(test)]
impl LicenseClock for FixedClock {
    fn now_secs(self) -> u64 {
        self.now_secs
    }
}

#[cfg(test)]
#[derive(Default)]
struct MemoryTrialStore {
    state: Option<LocalTrialState>,
}

#[cfg(test)]
impl TrialStore for MemoryTrialStore {
    fn read(&mut self) -> AppResult<Option<LocalTrialState>> {
        Ok(self.state.clone())
    }

    fn write(&mut self, state: &LocalTrialState) -> AppResult<()> {
        self.state = Some(state.clone());
        Ok(())
    }
}

#[cfg(test)]
#[derive(Default)]
struct MemoryActivationCredentialStore {
    activated_at_secs: Option<u64>,
}

#[cfg(test)]
impl ActivationCredentialStore for MemoryActivationCredentialStore {
    fn activated_at_secs(&mut self) -> AppResult<Option<u64>> {
        Ok(self.activated_at_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_status_starts_fourteen_day_trial() {
        let mut trial_store = MemoryTrialStore::default();
        let mut activation_store = MemoryActivationCredentialStore::default();
        let clock = FixedClock { now_secs: 1_000 };
        let service = LicenseService::new(&mut trial_store, &mut activation_store, clock);

        let status = service.status().unwrap();

        assert_eq!(status.kind, LicenseStatusKind::Trial);
        assert_eq!(status.trial_days_remaining, 14);
        assert!(!status.is_expired);
    }

    #[test]
    fn trial_expires_after_fourteen_days() {
        let mut trial_store = MemoryTrialStore {
            state: Some(LocalTrialState {
                trial_started_at_secs: 1_000,
            }),
        };
        let mut activation_store = MemoryActivationCredentialStore::default();
        let clock = FixedClock {
            now_secs: 1_000 + 15 * 24 * 60 * 60,
        };
        let service = LicenseService::new(&mut trial_store, &mut activation_store, clock);

        let status = service.status().unwrap();

        assert_eq!(status.kind, LicenseStatusKind::Expired);
        assert_eq!(status.trial_days_remaining, 0);
        assert!(status.is_expired);
    }

    #[test]
    fn activated_status_comes_from_credential_store_and_overrides_trial_expiry() {
        let mut trial_store = MemoryTrialStore {
            state: Some(LocalTrialState {
                trial_started_at_secs: 1_000,
            }),
        };
        let mut activation_store = MemoryActivationCredentialStore {
            activated_at_secs: Some(2_000),
        };
        let clock = FixedClock {
            now_secs: 1_000 + 30 * 24 * 60 * 60,
        };
        let service = LicenseService::new(&mut trial_store, &mut activation_store, clock);

        let status = service.status().unwrap();

        assert_eq!(status.kind, LicenseStatusKind::Activated);
        assert!(status.activated);
        assert!(!status.is_expired);
    }

    #[test]
    fn file_trial_state_does_not_contain_activation_entitlement() {
        let state = LocalTrialState {
            trial_started_at_secs: 1_000,
        };
        let json = serde_json::to_string(&state).unwrap();

        assert!(json.contains("trialStartedAtSecs"));
        assert!(!json.contains("activated"));
    }

    #[test]
    fn local_trial_state_rejects_activation_entitlement_field() {
        let json = r#"{"trialStartedAtSecs":1000,"activatedAtSecs":2000}"#;
        let error = serde_json::from_str::<LocalTrialState>(json).unwrap_err();

        assert!(error.to_string().contains("unknown field"));
    }
}
```

- [ ] **Step 4: Export module and payload**

Modify `src-tauri/src/app/mod.rs`:

```rust
pub mod license_service;
```

Add to `src-tauri/src/app/events.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatusPayload {
    pub kind: &'static str,
    pub trial_days_remaining: u8,
    pub is_expired: bool,
    pub activated: bool,
}

impl From<crate::app::license_service::LicenseStatus> for LicenseStatusPayload {
    fn from(status: crate::app::license_service::LicenseStatus) -> Self {
        let kind = match status.kind {
            crate::app::license_service::LicenseStatusKind::Trial => "trial",
            crate::app::license_service::LicenseStatusKind::Expired => "expired",
            crate::app::license_service::LicenseStatusKind::Activated => "activated",
        };
        Self {
            kind,
            trial_days_remaining: status.trial_days_remaining,
            is_expired: status.is_expired,
            activated: status.activated,
        }
    }
}
```

- [ ] **Step 5: Add Tauri commands**

Modify imports in `src-tauri/src/lib.rs`:

```rust
use app::events::LicenseStatusPayload;
use app::license_service::{
    FileTrialStore, LicenseService, NoopActivationCredentialStore, SystemLicenseClock,
};
use tauri::Manager;
```

Add helper:

```rust
fn license_state_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("读取应用数据目录失败: {error}"))
        .map(|dir| dir.join("license-state.json"))
}
```

Add commands:

```rust
#[tauri::command]
fn license_status(app: AppHandle) -> Result<LicenseStatusPayload, String> {
    let path = license_state_path(&app)?;
    let mut trial_store = FileTrialStore::new(path);
    let mut activation_store = NoopActivationCredentialStore;
    let service = LicenseService::new(&mut trial_store, &mut activation_store, SystemLicenseClock);
    service.status().map(LicenseStatusPayload::from).map_err(|error| error.to_string())
}

#[tauri::command]
fn activation_status(app: AppHandle) -> Result<LicenseStatusPayload, String> {
    license_status(app)
}

#[tauri::command]
fn activate_license(_app: AppHandle, code: String) -> Result<(), String> {
    if code.trim().is_empty() {
        return Err("激活码不能为空".to_string());
    }
    Err("服务端激活协议未接入，当前版本仅提供本地试用与激活状态接口".to_string())
}
```

Security boundary:

- The file-backed store may persist only the local trial start marker.
- Activated status can be returned in tests through `MemoryActivationCredentialStore`, proving the interface without product-side fake activation.
- Do not persist activation entitlement in the file-backed trial store in product code.
- If a production activation state is added, implement `SystemCredentialLicenseStore` with macOS Keychain / Windows Credential Manager / equivalent OS credential storage and keep it behind Native Safety review.
- Until secure credential storage exists, `activation_status()` may report trial/expired status and `activate_license()` must return the explicit “服务端激活协议未接入” error for non-empty codes.

Register commands:

```rust
license_status,
activation_status,
activate_license,
```

- [ ] **Step 6: Wire TypeScript API**

Modify `src/lib/tauri.ts`:

```ts
export type LicenseStatus = {
  kind: 'trial' | 'expired' | 'activated'
  trialDaysRemaining: number
  isExpired: boolean
  activated: boolean
}

export async function fetchLicenseStatus(): Promise<LicenseStatus> {
  return invoke<LicenseStatus>('license_status')
}

export async function fetchActivationStatus(): Promise<LicenseStatus> {
  return invoke<LicenseStatus>('activation_status')
}

export async function activateLicense(code: string): Promise<void> {
  return invoke('activate_license', { code })
}
```

- [ ] **Step 7: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml license
npm test -- --run src/App.test.tsx
```

Expected: Rust license tests PASS; frontend still PASS before UI component is used.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/app/mod.rs src-tauri/src/app/license_service.rs src-tauri/src/app/events.rs src-tauri/src/lib.rs src/lib/tauri.ts
git commit -m "feat(auth): 增加本地试用与激活状态接口"
```

## Task 8: License Status UI

**Files:**

- Create: `src/components/license-status.tsx`
- Modify: `src/App.tsx`
- Modify: `src/components/preview-view.tsx`
- Modify: `src/App.test.tsx`
- Test: `src/App.test.tsx`

- [ ] **Step 1: Add frontend license tests**

Add to `src/App.test.tsx`:

```tsx
it('shows local trial days in idle state', async () => {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'license_status') {
      return Promise.resolve({
        kind: 'trial',
        trialDaysRemaining: 12,
        isExpired: false,
        activated: false,
      })
    }
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)

  expect(await screen.findByText('试用剩余 12 天')).toBeInTheDocument()
})

it('shows expired local trial state', async () => {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'license_status') {
      return Promise.resolve({
        kind: 'expired',
        trialDaysRemaining: 0,
        isExpired: true,
        activated: false,
      })
    }
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)

  expect(await screen.findByText('试用已过期')).toBeInTheDocument()
})

it('shows activated local license state', async () => {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'license_status') {
      return Promise.resolve({
        kind: 'activated',
        trialDaysRemaining: 0,
        isExpired: false,
        activated: true,
      })
    }
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)

  expect(await screen.findByText('已激活')).toBeInTheDocument()
})
```

- [ ] **Step 2: Create compact component**

Create `src/components/license-status.tsx`:

```tsx
import type { LicenseStatus as LicenseStatusPayload } from '@/lib/tauri'
import { cn } from '@/lib/utils'

interface LicenseStatusProps {
  status: LicenseStatusPayload | null
}

export function LicenseStatus({ status }: LicenseStatusProps) {
  if (!status) return null

  const label = status.activated
    ? '已激活'
    : status.isExpired
      ? '试用已过期'
      : `试用剩余 ${status.trialDaysRemaining} 天`

  return (
    <div
      className={cn(
        'rounded-lg border px-3 py-2 text-xs',
        status.activated
          ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-300'
          : status.isExpired
            ? 'border-destructive/40 bg-destructive/10 text-destructive'
            : 'border-border/50 bg-secondary/30 text-muted-foreground',
      )}
    >
      {label}
    </div>
  )
}
```

- [ ] **Step 3: Wire App state**

Modify imports in `src/App.tsx`:

```tsx
import { LicenseStatus } from '@/components/license-status'
```

Extend Tauri imports:

```tsx
fetchLicenseStatus,
type LicenseStatus as LicenseStatusPayload,
```

Add state:

```tsx
const [licenseStatus, setLicenseStatus] = useState<LicenseStatusPayload | null>(null)
```

In initial `useEffect()` add:

```tsx
void fetchLicenseStatus().then(setLicenseStatus).catch(() => setLicenseStatus(null))
```

Render in idle state near the recording panel using normal layout flow, not absolute positioning:

```tsx
<div className="flex w-full items-start justify-end">
  <LicenseStatus status={licenseStatus} />
</div>
```

Pass the license status into Preview:

```tsx
return (
  <PreviewView
    onBack={handleBackToIdle}
    recordingResult={recordingResult}
    licenseStatus={licenseStatus}
  />
)
```

Update `PreviewViewProps` in `src/components/preview-view.tsx`:

```tsx
interface PreviewViewProps {
  onBack: () => void
  recordingResult?: RecordingResult | null
  licenseStatus?: LicenseStatusPayload | null
}
```

Render in the preview header right slot inside `PreviewView`:

```tsx
<div className="shrink-0">
  <LicenseStatus status={licenseStatus} />
</div>
```

Keep the badge compact; do not add an activation form unless product confirms activation-code behavior. During manual verification, check idle and preview layouts at 360px, 768px, and desktop widths and record that the badge does not overlap the main controls.

- [ ] **Step 4: Run frontend tests**

Run:

```bash
npm test -- --run src/App.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/license-status.tsx src/App.tsx src/App.test.tsx
git commit -m "feat(auth): 展示本地试用状态"
```

## Task 9: Checklist, Manual Gates, and Handoff

**Files:**

- Modify: `tests/phase-6-w11-w12-checklist.md`
- Modify: `HANDOFF.md`
- Optional create: `docs/superpowers/reviews/2026-05-29-phase-6-native-safety-notes.md`

- [ ] **Step 1: Complete automated verification**

Run:

```bash
git diff --check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

Expected:

- `git diff --check`: PASS.
- `cargo fmt`: PASS.
- `cargo test`: PASS.
- `cargo clippy`: PASS with no new errors.
- `cargo build`: PASS.
- `npm run build`: PASS.
- `npm test -- --run`: PASS.
- BUG scan finds no product-code regression.

- [ ] **Step 2: Complete FFmpeg feature verification**

Run when FFmpeg dev libraries are available:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets
```

Expected: PASS. If local environment cannot link FFmpeg, record the exact linker error in the checklist and do not mark FFmpeg manual gates complete.

- [ ] **Step 3: Manual verification**

Record results in `tests/phase-6-w11-w12-checklist.md`:

```markdown
## Verification Summary (2026-05-29, Phase 6)

- `git diff --check`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, <count> tests
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS
- `npm run build`: PASS
- `npm test -- --run`: PASS, <count> tests
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`: PASS / BLOCKED with reason
- Manual FFmpeg export: PASS / BLOCKED with reason
- Native Safety Gate: PASS / BLOCKED with reviewer/date
```

Also add an artifact evidence table for every FFmpeg scenario:

```markdown
## FFmpeg Artifact Evidence

| Scenario | Source path | Output path | Output bytes | Width x Height | Video stream | Audio stream | Source duration | Output duration | Duration delta | Original exists before/after | Inspector | Date |
|---|---|---|---:|---|---|---|---:|---:|---:|---|---|---|
| original recording artifact |  | n/a |  |  |  |  |  | n/a | n/a | n/a |  |  |
| 16:9 auto-trim off |  |  |  | 1920x1080 |  |  |  |  |  |  |  |  |
| 9:16 auto-trim off |  |  |  | 1080x1920 |  |  |  |  |  |  |  |  |
| 1:1 auto-trim off |  |  |  | 1080x1080 |  |  |  |  |  |  |  |  |
| 16:9 auto-trim on |  |  |  | 1920x1080 |  |  |  |  |  |  |  |  |
| cancel cleanup |  |  | n/a | n/a | n/a | n/a |  | n/a | n/a |  |  |  |
```

Replace `<count>` with actual test counts from command output.

- [ ] **Step 4: Update HANDOFF**

Add a top entry under `## 工作任务记录` with:

```markdown
### 2026-05-29：Phase 6 导出预设与本地授权实现

输入文件：

- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md`
- `tests/phase-6-w11-w12-checklist.md`

本轮完成：

1. Phase 5 FFmpeg Gate 合并为 Phase 6 前置导出流水线并完成。
2. 三种固定导出预设可生成独立 playable output。
3. 导出进度与取消接入 UI。
4. 本地 14 天试用状态与激活状态接口完成。

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` 通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 通过
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `npm run build` 通过
- `npm test -- --run` 通过

剩余待完成：

- 若 FFmpeg feature 或 Native Safety Gate 未完成，在这里明确记录阻塞原因。
```

Keep only the newest seven work-task entries, preserving the required HANDOFF structure.

- [ ] **Step 5: Commit**

```bash
git add tests/phase-6-w11-w12-checklist.md HANDOFF.md docs/superpowers/reviews/2026-05-29-phase-6-native-safety-notes.md
git commit -m "docs(export): 更新 Phase 6 验收与交接"
```

## Full Verification Matrix

Run before claiming Phase 6 complete:

```bash
git diff --check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

Run when FFmpeg development libraries are available:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets
npm run tauri -- dev --features ffmpeg
```

Manual gates:

- [ ] Bilibili / YouTube 16:9 export opens and contains video/audio.
- [ ] Douyin 9:16 export opens and contains video/audio.
- [ ] Xiaohongshu 1:1 export opens and contains video/audio.
- [ ] Auto-trim off exports full source.
- [ ] Auto-trim on consumes `CutTimeline` and produces shorter output when cuts exist.
- [ ] Binding inspection records dimensions, duration, video stream, audio stream, and file size for original/export artifacts.
- [ ] Export progress contains at least one intermediate 1-99 value from exporter callback.
- [ ] Original recording artifact still exists after export.
- [ ] Export cancel removes partial output.
- [ ] Export failure returns Chinese structured error and does not return fake `outputPath`.
- [ ] License badge does not overlap controls at 360px, 768px, or desktop widths.
- [ ] 10-minute 1080p recording/export pressure check records stop time, sidecar size, memory peak, and export time.
- [ ] Native Safety review covers `ffmpeg_writer.rs`, `trim_exporter.rs`, SCK callback, and credential persistence.
- [ ] BUG.md prevention scan passes.

## Self-Review

Spec coverage:

- Phase 6 fixed export presets: Task 1, Task 5, Task 6.
- Export progress/cancel: Task 4, Task 5.
- Phase 5 FFmpeg Gate merge: Task 3, Task 6.
- Trim sensitivity re-aggregation: Task 2.
- Local trial and activation status interface: Task 7, Task 8.
- Checklist and handoff: Task 9.
- BUG.md prevention rules: Task 9 verification matrix.

Placeholder scan:

- No deferred-work markers or cross-task shortcut instructions are used as implementation steps.
- The only explicit unfinished FFmpeg block is a safety gate in Task 6 that must be replaced before the task can be checked off; it is not an acceptable final implementation.

Type consistency:

- `ExportPreset` is centralized in `media/export_presets.rs`.
- `TrimExportRequest` always carries `input_path`, `output_path`, `preset`, `cut_timeline`, `effect_timeline_path`, `cancel_token`, and optional `progress`.
- `LicenseStatusPayload` maps Rust status to TS `LicenseStatus`, with activated state sourced through `ActivationCredentialStore`.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md`.

Recommended execution path:

1. **Subagent-Driven (recommended)** - Dispatch a fresh worker per task, review between tasks, and do not start Task 6 until Task 6A has produced a real original recording artifact.
2. **Inline Execution fallback** - Use `superpowers:executing-plans` task-by-task if subagents are unavailable, keeping the same review gates after Task 3, Task 6A, Task 6, and Task 8.
