# Phase 5 Silence Trimming Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Phase 5 blank-segment detection foundation: collect low-cost audio/visual activity metadata during recording, generate a conservative `CutTimeline` after recording, and wire the Preview/export boundary without sending media frames through React.

**Architecture:** Audio RMS and low-resolution frame-difference signals are computed in Rust on the recording consumer/post-process path, never in the ScreenCaptureKit callback and never in React. The pure `SilenceDetector` engine combines mixed-audio activity and visual activity into a JSON `CutTimeline`; the current export command consumes this timeline as structured data while real playable FFmpeg output remains gated by the existing production encoder work. Original recording artifacts are preserved and Phase 6 export presets can consume the `CutTimeline` sidecar.

**Tech Stack:** Tauri 2, React + TypeScript + Tailwind, Rust, ScreenCaptureKit, cpal, serde/serde_json, ffmpeg-next boundary, Vitest, React Testing Library, Cargo test/clippy/fmt.

---

## 0. Scope And Assumptions

Inputs:

- `HANDOFF.md`
- `docs/architecture/project-architecture-and-overall-planning.md`
- `tests/phase-5-w9-w10-checklist.md`
- `.codex/rules/0-global.md`
- `.codex/rules/1-coding-style.md`
- `.codex/rules/2-testing.md`
- `.codex/rules/3-git-commit.md`
- `.codex/rules/4-security.md`
- `.codex/rules/5-docs.md`
- `BUG.md`
- Current Phase 4 implementation and review status.

Current project facts this plan must respect:

- Phase 4 has completed cursor metadata and effect timeline sidecars.
- `RecordingResult.output_path` is currently `None` because `CountingRecordingWriter` is still the default writer and the feature-gated `FfmpegRecordingWriter` is a skeleton.
- `PreviewView` already stores `autoTrimSilences` and `trimSensitivity` in the existing beautify config path.
- `export_video` currently rebuilds the cursor effect timeline and returns a cursor summary.
- The frontend must not receive video frames, audio chunks, frame-diff samples, or audio activity streams.
- `Cargo.toml` core dependency versions must not be changed automatically.

Assumptions:

- Phase 5 is allowed to add Rust modules, Tauri command boundaries, TS types, UI status wiring, and tests.
- Phase 5 should not implement subtitles, summaries, templates, background beautification, automatic zoom-to-key-area, platform publishing APIs, or license logic.
- Phase 5 should not change the default recording writer to a production FFmpeg writer unless a human separately approves that execution scope.
- The deliverable is a conservative, deterministic `CutTimeline` plus export-boundary contract. A real playable trimmed MP4 remains a manual/Phase 6 gate until the production FFmpeg encoder/muxer exists.

Success criteria:

- Rust unit tests cover RMS windows, silence/background-noise thresholds, short pauses, low-resolution frame diff, static frames, animated frames, mouse-movement-like changes, candidate merging, buffers, empty/overlap/continuous timelines, and serde round-trips.
- Recording stop writes a trim metadata sidecar containing bounded audio/visual activity samples.
- `build_cut_timeline` builds a `CutTimeline` sidecar from trim metadata and current trim intent.
- `export_video` consumes both cursor effect and cut timeline contracts and returns one combined export summary.
- React invokes only Tauri commands and displays lightweight export/trim summaries and errors.
- `tests/phase-5-w9-w10-checklist.md` is updated with automatic status and manual gates.

Manual gates:

- **FFmpeg Gate:** Real playable trimmed output requires the production FFmpeg encoder/muxer path. Phase 5 must not fabricate an output path when no playable file exists.
- **Performance Gate:** Long recording memory pressure for trim metadata and cut timeline generation must be tested with real 1080p material.
- **Architecture Gate:** Verify `src/lib/tauri.ts`, `src/App.tsx`, and `src/components/preview-view.tsx` do not receive audio/video/frame-diff streams.
- **BUG.md Gate:** Confirm no new `data-tauri-drag-region="false"` wrapper, no `motion.div whileTap` direct parent around interactive buttons, and no `setIgnoreCursorEvents(true)`.

---

## 1. File Map

### Create

- `src-tauri/src/core/cut.rs` — trim config, audio/visual activity sample types, `CutSegment`, `KeepSegment`, `CutTimeline`, serde tests.
- `src-tauri/src/media/silence_detector.rs` — pure RMS analysis, low-resolution frame-diff analysis, conservative blank-candidate merge, `SilenceDetectorEngine`.
- `src-tauri/src/media/trim_metadata.rs` — JSON sidecar reader/writer for recording-time trim metadata and post-recording cut timeline.
- `src-tauri/src/media/trim_exporter.rs` — structured `CutTimeline` export request/result boundary and mock exporter tests; no FFmpeg CLI string building.

### Modify

- `src-tauri/src/core/mod.rs` — export `cut`.
- `src-tauri/src/core/processor.rs` — add `SilenceDetector` trait using `AudioActivitySample`, `FrameDiffSample`, and `CutTimeline`.
- `src-tauri/src/app/events.rs` — add `CutTimelineSummaryPayload` and `ExportSummaryPayload`.
- `src-tauri/src/app/error.rs` — add trim/export error variants with Chinese messages.
- `src-tauri/src/media/mod.rs` — export `silence_detector`, `trim_metadata`, and `trim_exporter`.
- `src-tauri/src/media/recording_writer.rs` — extend `RecordingResult` with `trim_metadata_path` and `cut_timeline_path`.
- `src-tauri/src/media/ffmpeg_writer.rs` — keep feature-gated skeleton compiling after `RecordingResult` shape changes.
- `src-tauri/src/platform/macos_service.rs` — collect bounded trim metadata in the consumer thread and write sidecar after recording stops.
- `src-tauri/src/lib.rs` — add `build_cut_timeline`, update `export_video` to return combined export summary, keep cursor build behavior intact.
- `src/lib/tauri.ts` — add `CutTimelineSummary`, `ExportSummary`, `buildCutTimeline()`, update `exportVideo()` return type.
- `src/components/preview-view.tsx` — trigger cut timeline build/export when auto-trim is enabled; display lightweight trim/export errors and summary.
- `src/App.test.tsx` — add frontend command-order, error, and no-media-stream tests.
- `tests/phase-5-w9-w10-checklist.md` — record automatic/manual status.
- `HANDOFF.md` — update only after implementation and verification are complete.

### Reference Only

- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md`
- `tests/phase-4-w7-w8-checklist.md`
- `BUG.md`

---

## 2. Phase Breakdown

- **Phase A:** Core `CutTimeline` model and trait boundary.
- **Phase B:** Pure audio RMS and frame-diff analyzers.
- **Phase C:** Conservative cut candidate engine.
- **Phase D:** Recording consumer integration and trim metadata sidecar.
- **Phase E:** Tauri command/export boundary.
- **Phase F:** Preview UI and TS command wiring.
- **Phase G:** Checklist, handoff, and verification.

---

### Task 1: Core Cut Timeline Model

**Files:**

- Create: `src-tauri/src/core/cut.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Modify: `src-tauri/src/core/processor.rs`

- [ ] **Step 1: Write failing serde and timeline tests**

Create `src-tauri/src/core/cut.rs` with only the test module below so the test fails because the types do not exist:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;

    #[test]
    fn cut_timeline_round_trips_camel_case_json() {
        let timeline = CutTimeline {
            duration_nanos: 20_000_000_000,
            cuts: vec![CutSegment {
                start: MediaTimestamp::from_nanos(6_400_000_000),
                end: MediaTimestamp::from_nanos(12_100_000_000),
                reason: CutReason::SilentAndStill,
                mean_audio_rms: 0.004,
                mean_visual_change: 0.002,
            }],
            keeps: vec![
                KeepSegment {
                    start: MediaTimestamp::from_nanos(0),
                    end: MediaTimestamp::from_nanos(6_400_000_000),
                },
                KeepSegment {
                    start: MediaTimestamp::from_nanos(12_100_000_000),
                    end: MediaTimestamp::from_nanos(20_000_000_000),
                },
            ],
            total_cut_nanos: 5_700_000_000,
        };

        let json = serde_json::to_string(&timeline).unwrap();
        assert!(json.contains("\"durationNanos\":20000000000"));
        assert!(json.contains("\"totalCutNanos\":5700000000"));
        assert!(json.contains("\"reason\":\"silentAndStill\""));

        let parsed: CutTimeline = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, timeline);
    }

    #[test]
    fn trim_sensitivity_maps_to_conservative_defaults() {
        let low = TrimConfig::from_sensitivity(TrimSensitivity::Low);
        let medium = TrimConfig::from_sensitivity(TrimSensitivity::Medium);
        let high = TrimConfig::from_sensitivity(TrimSensitivity::High);

        assert_eq!(low.rms_window_nanos, 1_000_000_000);
        assert_eq!(medium.rms_window_nanos, 750_000_000);
        assert_eq!(high.rms_window_nanos, 500_000_000);

        assert_eq!(low.min_candidate_nanos, 8_000_000_000);
        assert_eq!(medium.min_candidate_nanos, 6_000_000_000);
        assert_eq!(high.min_candidate_nanos, 5_000_000_000);

        assert!(low.audio_rms_threshold < high.audio_rms_threshold);
        assert!(low.visual_change_threshold < high.visual_change_threshold);
    }

    #[test]
    fn empty_cut_timeline_keeps_full_duration() {
        let timeline = CutTimeline::empty(10_000_000_000);

        assert!(timeline.cuts.is_empty());
        assert_eq!(timeline.total_cut_nanos, 0);
        assert_eq!(
            timeline.keeps,
            vec![KeepSegment {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(10_000_000_000),
            }]
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::cut -- --nocapture
```

Expected: FAIL with missing `CutTimeline`, `CutSegment`, `CutReason`, `KeepSegment`, `TrimConfig`, or `TrimSensitivity`.

- [ ] **Step 3: Implement `core/cut.rs` model**

Replace `src-tauri/src/core/cut.rs` with:

```rust
use serde::{Deserialize, Serialize};

use crate::core::frame::MediaTimestamp;

/// User-facing trim sensitivity selected in Preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrimSensitivity {
    Low,
    Medium,
    High,
}

impl TrimSensitivity {
    /// Parses the frontend string value used by `BeautifyConfigPayload`.
    pub fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(format!("未知裁剪灵敏度：{other}")),
        }
    }
}

/// Conservative configuration for blank-segment detection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimConfig {
    pub rms_window_nanos: u64,
    pub audio_rms_threshold: f32,
    pub visual_change_threshold: f32,
    pub min_candidate_nanos: u64,
    pub absolute_min_cut_nanos: u64,
    pub buffer_nanos: u64,
    pub merge_gap_nanos: u64,
}

impl TrimConfig {
    /// Defaults are intentionally conservative to avoid cutting thinking time.
    pub fn from_sensitivity(sensitivity: TrimSensitivity) -> Self {
        match sensitivity {
            TrimSensitivity::Low => Self {
                rms_window_nanos: 1_000_000_000,
                audio_rms_threshold: 0.012,
                visual_change_threshold: 0.006,
                min_candidate_nanos: 8_000_000_000,
                absolute_min_cut_nanos: 2_000_000_000,
                buffer_nanos: 500_000_000,
                merge_gap_nanos: 500_000_000,
            },
            TrimSensitivity::Medium => Self {
                rms_window_nanos: 750_000_000,
                audio_rms_threshold: 0.02,
                visual_change_threshold: 0.01,
                min_candidate_nanos: 6_000_000_000,
                absolute_min_cut_nanos: 2_000_000_000,
                buffer_nanos: 400_000_000,
                merge_gap_nanos: 500_000_000,
            },
            TrimSensitivity::High => Self {
                rms_window_nanos: 500_000_000,
                audio_rms_threshold: 0.03,
                visual_change_threshold: 0.015,
                min_candidate_nanos: 5_000_000_000,
                absolute_min_cut_nanos: 2_000_000_000,
                buffer_nanos: 300_000_000,
                merge_gap_nanos: 500_000_000,
            },
        }
    }
}

/// Audio activity sampled from mixed audio after recording.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioActivitySample {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub rms: f32,
}

/// Low-resolution visual activity sampled from captured video frames.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameDiffSample {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub change_ratio: f32,
}

/// Reason a segment was selected for cutting.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CutReason {
    SilentAndStill,
}

/// Segment removed from the exported timeline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutSegment {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub reason: CutReason,
    pub mean_audio_rms: f32,
    pub mean_visual_change: f32,
}

/// Segment kept in the exported timeline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepSegment {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
}

/// Complete post-recording cut timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutTimeline {
    pub duration_nanos: u64,
    pub cuts: Vec<CutSegment>,
    pub keeps: Vec<KeepSegment>,
    pub total_cut_nanos: u64,
}

impl CutTimeline {
    /// Creates a no-op timeline that keeps the entire recording.
    pub fn empty(duration_nanos: u64) -> Self {
        Self {
            duration_nanos,
            cuts: Vec::new(),
            keeps: vec![KeepSegment {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(duration_nanos),
            }],
            total_cut_nanos: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;

    #[test]
    fn cut_timeline_round_trips_camel_case_json() {
        let timeline = CutTimeline {
            duration_nanos: 20_000_000_000,
            cuts: vec![CutSegment {
                start: MediaTimestamp::from_nanos(6_400_000_000),
                end: MediaTimestamp::from_nanos(12_100_000_000),
                reason: CutReason::SilentAndStill,
                mean_audio_rms: 0.004,
                mean_visual_change: 0.002,
            }],
            keeps: vec![
                KeepSegment {
                    start: MediaTimestamp::from_nanos(0),
                    end: MediaTimestamp::from_nanos(6_400_000_000),
                },
                KeepSegment {
                    start: MediaTimestamp::from_nanos(12_100_000_000),
                    end: MediaTimestamp::from_nanos(20_000_000_000),
                },
            ],
            total_cut_nanos: 5_700_000_000,
        };

        let json = serde_json::to_string(&timeline).unwrap();
        assert!(json.contains("\"durationNanos\":20000000000"));
        assert!(json.contains("\"totalCutNanos\":5700000000"));
        assert!(json.contains("\"reason\":\"silentAndStill\""));

        let parsed: CutTimeline = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, timeline);
    }

    #[test]
    fn trim_sensitivity_maps_to_conservative_defaults() {
        let low = TrimConfig::from_sensitivity(TrimSensitivity::Low);
        let medium = TrimConfig::from_sensitivity(TrimSensitivity::Medium);
        let high = TrimConfig::from_sensitivity(TrimSensitivity::High);

        assert_eq!(low.rms_window_nanos, 1_000_000_000);
        assert_eq!(medium.rms_window_nanos, 750_000_000);
        assert_eq!(high.rms_window_nanos, 500_000_000);

        assert_eq!(low.min_candidate_nanos, 8_000_000_000);
        assert_eq!(medium.min_candidate_nanos, 6_000_000_000);
        assert_eq!(high.min_candidate_nanos, 5_000_000_000);

        assert!(low.audio_rms_threshold < high.audio_rms_threshold);
        assert!(low.visual_change_threshold < high.visual_change_threshold);
    }

    #[test]
    fn empty_cut_timeline_keeps_full_duration() {
        let timeline = CutTimeline::empty(10_000_000_000);

        assert!(timeline.cuts.is_empty());
        assert_eq!(timeline.total_cut_nanos, 0);
        assert_eq!(
            timeline.keeps,
            vec![KeepSegment {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(10_000_000_000),
            }]
        );
    }
}
```

- [ ] **Step 4: Export module and add processor trait**

Modify `src-tauri/src/core/mod.rs`:

```rust
pub mod capture;
pub mod clock;
pub mod config;
pub mod cut;
pub mod frame;
pub mod media_channel;
pub mod processor;
pub mod timeline;
```

Modify `src-tauri/src/core/processor.rs`:

```rust
use crate::app::error::AppResult;
use crate::core::cut::{AudioActivitySample, CutTimeline, FrameDiffSample};
use crate::core::timeline::{CursorClick, CursorSample, EffectTimeline};

/// Builds a post-recording cursor effect timeline from recorded cursor metadata.
pub trait CursorProcessor: Send + Sync {
    /// Converts raw cursor samples and click events into a per-frame effect timeline.
    fn build_timeline(
        &self,
        samples: &[CursorSample],
        clicks: &[CursorClick],
        fps: u32,
        duration_nanos: u64,
    ) -> AppResult<EffectTimeline>;
}

/// Builds a post-recording cut timeline from audio and visual activity metadata.
pub trait SilenceDetector: Send + Sync {
    /// Converts mixed-audio activity and low-resolution frame-diff samples into cut segments.
    fn analyze(
        &self,
        audio: &[AudioActivitySample],
        visual: &[FrameDiffSample],
        duration_nanos: u64,
    ) -> AppResult<CutTimeline>;
}
```

- [ ] **Step 5: Run test to verify it passes**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::cut core::processor -- --nocapture
```

Expected: PASS for `core::cut` tests.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/cut.rs src-tauri/src/core/mod.rs src-tauri/src/core/processor.rs
git commit -m "feat(core): 新增裁剪时间线模型"
```

---

### Task 2: Audio RMS Analyzer

**Files:**

- Create: `src-tauri/src/media/silence_detector.rs`
- Modify: `src-tauri/src/media/mod.rs`

- [ ] **Step 1: Write failing RMS tests**

Create `src-tauri/src/media/silence_detector.rs` with:

```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::core::cut::{TrimConfig, TrimSensitivity};
    use crate::core::frame::{MediaTimestamp, MixedAudioChunk};

    fn mixed_chunk(start_nanos: u64, samples: Vec<f32>) -> MixedAudioChunk {
        MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(start_nanos),
            sample_rate: 48_000,
            channels: 1,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    #[test]
    fn rms_window_uses_configured_500ms_to_1000ms_window() {
        let low = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Low));
        let high = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::High));

        assert_eq!(low.window_nanos(), 1_000_000_000);
        assert_eq!(high.window_nanos(), 500_000_000);
    }

    #[test]
    fn silence_chunk_produces_low_rms_sample() {
        let analyzer = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        let samples = analyzer.analyze_chunks(&[mixed_chunk(0, vec![0.0; 48_000])]);

        assert_eq!(samples.len(), 1);
        assert!(samples[0].rms < 0.001);
    }

    #[test]
    fn background_noise_remains_explainable() {
        let analyzer = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        let samples = analyzer.analyze_chunks(&[mixed_chunk(0, vec![0.015; 48_000])]);

        assert_eq!(samples.len(), 1);
        assert!(samples[0].rms > 0.014);
        assert!(samples[0].rms < 0.016);
    }

    #[test]
    fn loud_chunk_is_not_silent() {
        let analyzer = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        let samples = analyzer.analyze_chunks(&[mixed_chunk(0, vec![0.25; 48_000])]);

        assert_eq!(samples.len(), 1);
        assert!(samples[0].rms > 0.2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::silence_detector::tests::rms -- --nocapture
```

Expected: FAIL because `AudioRmsAnalyzer` does not exist.

- [ ] **Step 3: Implement RMS analyzer**

Replace the top of `src-tauri/src/media/silence_detector.rs` with:

```rust
use crate::core::cut::{AudioActivitySample, TrimConfig};
use crate::core::frame::{MediaTimestamp, MixedAudioChunk};

/// Computes mixed-audio RMS activity samples using a conservative window.
pub struct AudioRmsAnalyzer {
    config: TrimConfig,
}

impl AudioRmsAnalyzer {
    pub fn new(config: TrimConfig) -> Self {
        Self { config }
    }

    pub fn window_nanos(&self) -> u64 {
        self.config.rms_window_nanos
    }

    pub fn analyze_chunks(&self, chunks: &[MixedAudioChunk]) -> Vec<AudioActivitySample> {
        chunks
            .iter()
            .filter_map(|chunk| {
                if chunk.sample_rate == 0 || chunk.channels == 0 || chunk.samples.is_empty() {
                    return None;
                }

                let sum_squares = chunk
                    .samples
                    .iter()
                    .map(|sample| sample * sample)
                    .sum::<f32>();
                let rms = (sum_squares / chunk.samples.len() as f32).sqrt();
                let frames = chunk.samples.len() as u64 / chunk.channels as u64;
                let duration_nanos =
                    frames.saturating_mul(1_000_000_000) / chunk.sample_rate as u64;

                Some(AudioActivitySample {
                    start: chunk.timestamp,
                    end: MediaTimestamp::from_nanos(
                        chunk.timestamp.nanos.saturating_add(duration_nanos),
                    ),
                    rms,
                })
            })
            .collect()
    }
}
```

Keep the test module from Step 1 below this implementation.

- [ ] **Step 4: Export module**

Modify `src-tauri/src/media/mod.rs`:

```rust
pub mod audio_mixer;
pub mod audio_synchronizer;
pub mod cursor_engine;
#[cfg(feature = "ffmpeg")]
pub mod ffmpeg_writer;
pub mod mic_level;
pub mod recording_metadata;
pub mod recording_writer;
pub mod silence_detector;
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::silence_detector::tests:: -- --nocapture
```

Expected: PASS for the four RMS tests.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/silence_detector.rs src-tauri/src/media/mod.rs
git commit -m "feat(export): 新增音频静音检测"
```

---

### Task 3: Low-Resolution Frame Difference Analyzer

**Files:**

- Modify: `src-tauri/src/media/silence_detector.rs`

- [ ] **Step 1: Add failing frame-diff tests**

Append these tests to the existing test module in `src-tauri/src/media/silence_detector.rs`:

```rust
use crate::core::frame::{FrameBuffer, PixelFormat, VideoFrame};

fn bgra_frame(timestamp: u64, width: u32, height: u32, pixel: [u8; 4]) -> VideoFrame {
    let mut bytes = Vec::new();
    for _ in 0..(width * height) {
        bytes.extend_from_slice(&pixel);
    }
    VideoFrame {
        timestamp: MediaTimestamp::from_nanos(timestamp),
        width,
        height,
        pixel_format: PixelFormat::Bgra8,
        buffer: FrameBuffer::Owned(Arc::from(bytes.into_boxed_slice())),
    }
}

#[test]
fn frame_diff_static_frames_are_near_zero() {
    let analyzer = FrameDiffAnalyzer::new(4, 4);
    let first = bgra_frame(0, 4, 4, [0, 0, 0, 255]);
    let second = bgra_frame(33_333_333, 4, 4, [0, 0, 0, 255]);

    let diff = analyzer.diff_pair(&first, &second).unwrap();

    assert!(diff.change_ratio < 0.001);
}

#[test]
fn frame_diff_detects_loading_animation_like_change() {
    let analyzer = FrameDiffAnalyzer::new(4, 4);
    let first = bgra_frame(0, 4, 4, [0, 0, 0, 255]);
    let second = bgra_frame(33_333_333, 4, 4, [255, 255, 255, 255]);

    let diff = analyzer.diff_pair(&first, &second).unwrap();

    assert!(diff.change_ratio > 0.9);
}

#[test]
fn frame_diff_detects_small_cursor_like_motion() {
    let analyzer = FrameDiffAnalyzer::new(4, 4);
    let first = bgra_frame(0, 4, 4, [0, 0, 0, 255]);
    let mut second = bgra_frame(33_333_333, 4, 4, [0, 0, 0, 255]);
    if let FrameBuffer::Owned(buffer) = &mut second.buffer {
        let mut bytes = buffer.to_vec();
        bytes[0] = 255;
        bytes[1] = 255;
        bytes[2] = 255;
        second.buffer = FrameBuffer::Owned(Arc::from(bytes.into_boxed_slice()));
    }

    let diff = analyzer.diff_pair(&first, &second).unwrap();

    assert!(diff.change_ratio > 0.05);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::silence_detector::tests::frame_diff -- --nocapture
```

Expected: FAIL because `FrameDiffAnalyzer` does not exist.

- [ ] **Step 3: Implement frame-diff analyzer**

Insert this implementation above the test module in `src-tauri/src/media/silence_detector.rs`:

```rust
use crate::core::cut::FrameDiffSample;
use crate::core::frame::{FrameBuffer, PixelFormat, VideoFrame};

/// Computes low-resolution grayscale frame differences without touching React.
pub struct FrameDiffAnalyzer {
    thumb_width: u32,
    thumb_height: u32,
}

impl FrameDiffAnalyzer {
    pub fn new(thumb_width: u32, thumb_height: u32) -> Self {
        Self {
            thumb_width: thumb_width.max(1),
            thumb_height: thumb_height.max(1),
        }
    }

    pub fn diff_pair(&self, previous: &VideoFrame, current: &VideoFrame) -> Result<FrameDiffSample, String> {
        if previous.pixel_format != PixelFormat::Bgra8 || current.pixel_format != PixelFormat::Bgra8 {
            return Err("帧差分仅支持 BGRA8 像素格式".to_string());
        }
        if previous.width == 0 || previous.height == 0 || current.width == 0 || current.height == 0 {
            return Err("帧差分输入尺寸无效".to_string());
        }

        let previous_thumb = self.downsample_grayscale(previous)?;
        let current_thumb = self.downsample_grayscale(current)?;
        let diff_sum = previous_thumb
            .iter()
            .zip(current_thumb.iter())
            .map(|(a, b)| (*a as i16 - *b as i16).unsigned_abs() as f32 / 255.0)
            .sum::<f32>();
        let change_ratio = diff_sum / previous_thumb.len() as f32;

        Ok(FrameDiffSample {
            start: previous.timestamp,
            end: current.timestamp,
            change_ratio,
        })
    }

    fn downsample_grayscale(&self, frame: &VideoFrame) -> Result<Vec<u8>, String> {
        let bytes = match &frame.buffer {
            FrameBuffer::Owned(bytes) => bytes.as_ref(),
        };
        let expected = frame.width as usize * frame.height as usize * 4;
        if bytes.len() < expected {
            return Err("帧像素数据长度不足".to_string());
        }

        let mut output = Vec::with_capacity((self.thumb_width * self.thumb_height) as usize);
        for y in 0..self.thumb_height {
            for x in 0..self.thumb_width {
                let src_x = (x as u64 * frame.width as u64 / self.thumb_width as u64) as usize;
                let src_y = (y as u64 * frame.height as u64 / self.thumb_height as u64) as usize;
                let offset = (src_y * frame.width as usize + src_x) * 4;
                let b = bytes[offset] as f32;
                let g = bytes[offset + 1] as f32;
                let r = bytes[offset + 2] as f32;
                let gray = (0.114 * b + 0.587 * g + 0.299 * r).round().clamp(0.0, 255.0) as u8;
                output.push(gray);
            }
        }
        Ok(output)
    }
}
```

- [ ] **Step 4: Run frame-diff tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::silence_detector::tests::frame_diff -- --nocapture
```

Expected: PASS for three frame-diff tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/silence_detector.rs
git commit -m "feat(export): 新增低分辨率帧差分"
```

---

### Task 4: Conservative Blank Segment Engine

**Files:**

- Modify: `src-tauri/src/media/silence_detector.rs`

- [ ] **Step 1: Add failing cut-candidate tests**

Append these tests to `src-tauri/src/media/silence_detector.rs`:

```rust
use crate::core::cut::{AudioActivitySample, FrameDiffSample};

fn audio_sample(start: u64, end: u64, rms: f32) -> AudioActivitySample {
    AudioActivitySample {
        start: MediaTimestamp::from_nanos(start),
        end: MediaTimestamp::from_nanos(end),
        rms,
    }
}

fn visual_sample(start: u64, end: u64, change_ratio: f32) -> FrameDiffSample {
    FrameDiffSample {
        start: MediaTimestamp::from_nanos(start),
        end: MediaTimestamp::from_nanos(end),
        change_ratio,
    }
}

#[test]
fn detector_requires_audio_silence_and_low_visual_change() {
    let config = TrimConfig::from_sensitivity(TrimSensitivity::High);
    let detector = SilenceDetectorEngine::new(config);
    let audio = vec![audio_sample(0, 7_000_000_000, 0.001)];
    let visual_active = vec![visual_sample(0, 7_000_000_000, 0.2)];

    let timeline = detector.analyze(&audio, &visual_active, 10_000_000_000).unwrap();

    assert!(timeline.cuts.is_empty());
    assert_eq!(timeline.total_cut_nanos, 0);
}

#[test]
fn detector_does_not_cut_short_pause() {
    let config = TrimConfig::from_sensitivity(TrimSensitivity::High);
    let detector = SilenceDetectorEngine::new(config);
    let audio = vec![audio_sample(1_000_000_000, 2_500_000_000, 0.001)];
    let visual = vec![visual_sample(1_000_000_000, 2_500_000_000, 0.001)];

    let timeline = detector.analyze(&audio, &visual, 5_000_000_000).unwrap();

    assert!(timeline.cuts.is_empty());
}

#[test]
fn detector_cuts_long_silent_and_still_segment_with_buffer() {
    let config = TrimConfig::from_sensitivity(TrimSensitivity::Medium);
    let detector = SilenceDetectorEngine::new(config);
    let audio = vec![audio_sample(2_000_000_000, 9_000_000_000, 0.001)];
    let visual = vec![visual_sample(2_000_000_000, 9_000_000_000, 0.001)];

    let timeline = detector.analyze(&audio, &visual, 12_000_000_000).unwrap();

    assert_eq!(timeline.cuts.len(), 1);
    assert_eq!(timeline.cuts[0].start.nanos, 2_400_000_000);
    assert_eq!(timeline.cuts[0].end.nanos, 8_600_000_000);
    assert_eq!(timeline.total_cut_nanos, 6_200_000_000);
}

#[test]
fn detector_merges_adjacent_candidates() {
    let config = TrimConfig::from_sensitivity(TrimSensitivity::High);
    let detector = SilenceDetectorEngine::new(config);
    let audio = vec![
        audio_sample(0, 5_500_000_000, 0.001),
        audio_sample(5_800_000_000, 11_500_000_000, 0.001),
    ];
    let visual = vec![
        visual_sample(0, 5_500_000_000, 0.001),
        visual_sample(5_800_000_000, 11_500_000_000, 0.001),
    ];

    let timeline = detector.analyze(&audio, &visual, 14_000_000_000).unwrap();

    assert_eq!(timeline.cuts.len(), 1);
    assert!(timeline.cuts[0].start.nanos < 500_000_000);
    assert!(timeline.cuts[0].end.nanos > 11_000_000_000);
}

#[test]
fn detector_empty_inputs_return_noop_timeline() {
    let config = TrimConfig::from_sensitivity(TrimSensitivity::Medium);
    let detector = SilenceDetectorEngine::new(config);

    let timeline = detector.analyze(&[], &[], 3_000_000_000).unwrap();

    assert!(timeline.cuts.is_empty());
    assert_eq!(timeline.keeps.len(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::silence_detector::tests::detector -- --nocapture
```

Expected: FAIL because `SilenceDetectorEngine` does not exist.

- [ ] **Step 3: Implement `SilenceDetectorEngine`**

Add this implementation to `src-tauri/src/media/silence_detector.rs`:

```rust
use crate::app::error::AppResult;
use crate::core::cut::{CutReason, CutSegment, CutTimeline, KeepSegment};
use crate::core::processor::SilenceDetector;

/// Pure post-recording engine that combines audio silence and visual stillness.
pub struct SilenceDetectorEngine {
    config: TrimConfig,
}

impl SilenceDetectorEngine {
    pub fn new(config: TrimConfig) -> Self {
        Self { config }
    }

    fn is_candidate(&self, audio: &AudioActivitySample, visual: &FrameDiffSample) -> bool {
        audio.rms <= self.config.audio_rms_threshold
            && visual.change_ratio <= self.config.visual_change_threshold
            && ranges_overlap(audio.start.nanos, audio.end.nanos, visual.start.nanos, visual.end.nanos)
    }

    fn build_keeps(&self, cuts: &[CutSegment], duration_nanos: u64) -> Vec<KeepSegment> {
        if cuts.is_empty() {
            return CutTimeline::empty(duration_nanos).keeps;
        }

        let mut keeps = Vec::new();
        let mut cursor = 0u64;
        for cut in cuts {
            if cursor < cut.start.nanos {
                keeps.push(KeepSegment {
                    start: MediaTimestamp::from_nanos(cursor),
                    end: cut.start,
                });
            }
            cursor = cut.end.nanos;
        }
        if cursor < duration_nanos {
            keeps.push(KeepSegment {
                start: MediaTimestamp::from_nanos(cursor),
                end: MediaTimestamp::from_nanos(duration_nanos),
            });
        }
        keeps
    }
}

impl SilenceDetector for SilenceDetectorEngine {
    fn analyze(
        &self,
        audio: &[AudioActivitySample],
        visual: &[FrameDiffSample],
        duration_nanos: u64,
    ) -> AppResult<CutTimeline> {
        if audio.is_empty() || visual.is_empty() || duration_nanos == 0 {
            return Ok(CutTimeline::empty(duration_nanos));
        }

        let mut candidates: Vec<(u64, u64, f32, f32)> = Vec::new();
        for audio_sample in audio {
            for visual_sample in visual {
                if !self.is_candidate(audio_sample, visual_sample) {
                    continue;
                }
                let start = audio_sample.start.nanos.max(visual_sample.start.nanos);
                let end = audio_sample.end.nanos.min(visual_sample.end.nanos);
                if end > start {
                    candidates.push((start, end, audio_sample.rms, visual_sample.change_ratio));
                }
            }
        }
        candidates.sort_by_key(|candidate| candidate.0);

        let mut merged: Vec<(u64, u64, Vec<f32>, Vec<f32>)> = Vec::new();
        for (start, end, rms, change) in candidates {
            if let Some(last) = merged.last_mut() {
                if start <= last.1.saturating_add(self.config.merge_gap_nanos) {
                    last.1 = last.1.max(end);
                    last.2.push(rms);
                    last.3.push(change);
                    continue;
                }
            }
            merged.push((start, end, vec![rms], vec![change]));
        }

        let mut cuts = Vec::new();
        for (start, end, rms_values, visual_values) in merged {
            if end.saturating_sub(start) < self.config.min_candidate_nanos {
                continue;
            }
            let cut_start = start.saturating_add(self.config.buffer_nanos).min(duration_nanos);
            let cut_end = end.saturating_sub(self.config.buffer_nanos).min(duration_nanos);
            if cut_end <= cut_start {
                continue;
            }
            let cut_duration = cut_end - cut_start;
            if cut_duration < self.config.absolute_min_cut_nanos {
                continue;
            }

            let mean_audio_rms = mean(&rms_values);
            let mean_visual_change = mean(&visual_values);
            cuts.push(CutSegment {
                start: MediaTimestamp::from_nanos(cut_start),
                end: MediaTimestamp::from_nanos(cut_end),
                reason: CutReason::SilentAndStill,
                mean_audio_rms,
                mean_visual_change,
            });
        }

        let total_cut_nanos = cuts
            .iter()
            .map(|cut| cut.end.nanos.saturating_sub(cut.start.nanos))
            .sum();
        let keeps = self.build_keeps(&cuts, duration_nanos);

        Ok(CutTimeline {
            duration_nanos,
            cuts,
            keeps,
            total_cut_nanos,
        })
    }
}

fn ranges_overlap(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start < b_end && b_start < a_end
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}
```

- [ ] **Step 4: Run detector tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::silence_detector::tests::detector -- --nocapture
```

Expected: PASS for detector tests.

- [ ] **Step 5: Run all silence detector tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::silence_detector -- --nocapture
```

Expected: PASS for RMS, frame-diff, and detector tests.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/silence_detector.rs
git commit -m "feat(export): 生成保守裁剪时间线"
```

---

### Task 5: Trim Metadata Sidecar

**Files:**

- Create: `src-tauri/src/media/trim_metadata.rs`
- Modify: `src-tauri/src/media/mod.rs`

- [ ] **Step 1: Write failing metadata tests**

Create `src-tauri/src/media/trim_metadata.rs`:

```rust
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cut::{AudioActivitySample, CutTimeline, FrameDiffSample};
    use crate::core::frame::MediaTimestamp;

    #[test]
    fn trim_metadata_round_trips_json() {
        let metadata = TrimMetadata {
            duration_nanos: 10_000_000_000,
            audio_activity: vec![AudioActivitySample {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(1_000_000_000),
                rms: 0.01,
            }],
            visual_activity: vec![FrameDiffSample {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(33_333_333),
                change_ratio: 0.002,
            }],
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: TrimMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, metadata);
    }

    #[test]
    fn write_and_read_cut_timeline_file() {
        let path = std::env::temp_dir().join("luzhi-cut-timeline-test.json");
        let timeline = CutTimeline::empty(5_000_000_000);

        TrimMetadataWriter::write_cut_timeline(&path, &timeline).unwrap();
        let parsed = TrimMetadataWriter::read_cut_timeline(&path).unwrap();

        assert_eq!(parsed, timeline);
        let _ = std::fs::remove_file(path);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::trim_metadata -- --nocapture
```

Expected: FAIL because `TrimMetadata` and `TrimMetadataWriter` do not exist.

- [ ] **Step 3: Implement trim metadata writer**

Replace `src-tauri/src/media/trim_metadata.rs` with:

```rust
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app::error::{AppError, AppResult};
use crate::core::cut::{AudioActivitySample, CutTimeline, FrameDiffSample};

/// Recording-time metadata used to build a CutTimeline after recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimMetadata {
    pub duration_nanos: u64,
    pub audio_activity: Vec<AudioActivitySample>,
    pub visual_activity: Vec<FrameDiffSample>,
}

/// JSON sidecar reader/writer for trim metadata and cut timelines.
pub struct TrimMetadataWriter;

impl TrimMetadataWriter {
    pub fn write_metadata(path: &Path, metadata: &TrimMetadata) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::RecordingWriteFailed {
                reason: format!("创建裁剪元数据目录失败: {error}"),
            })?;
        }
        let json = serde_json::to_string_pretty(metadata).map_err(|error| {
            AppError::RecordingWriteFailed {
                reason: format!("序列化裁剪元数据失败: {error}"),
            }
        })?;
        fs::write(path, json).map_err(|error| AppError::RecordingWriteFailed {
            reason: format!("写入裁剪元数据失败: {error}"),
        })
    }

    pub fn read_metadata(path: &Path) -> AppResult<TrimMetadata> {
        let json = fs::read_to_string(path).map_err(|error| AppError::RecordingWriteFailed {
            reason: format!("读取裁剪元数据失败: {error}"),
        })?;
        serde_json::from_str(&json).map_err(|error| AppError::RecordingWriteFailed {
            reason: format!("解析裁剪元数据失败: {error}"),
        })
    }

    pub fn write_cut_timeline(path: &Path, timeline: &CutTimeline) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::RecordingWriteFailed {
                reason: format!("创建裁剪时间线目录失败: {error}"),
            })?;
        }
        let json = serde_json::to_string_pretty(timeline).map_err(|error| {
            AppError::RecordingWriteFailed {
                reason: format!("序列化裁剪时间线失败: {error}"),
            }
        })?;
        fs::write(path, json).map_err(|error| AppError::RecordingWriteFailed {
            reason: format!("写入裁剪时间线失败: {error}"),
        })
    }

    pub fn read_cut_timeline(path: &Path) -> AppResult<CutTimeline> {
        let json = fs::read_to_string(path).map_err(|error| AppError::RecordingWriteFailed {
            reason: format!("读取裁剪时间线失败: {error}"),
        })?;
        serde_json::from_str(&json).map_err(|error| AppError::RecordingWriteFailed {
            reason: format!("解析裁剪时间线失败: {error}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cut::{AudioActivitySample, FrameDiffSample};
    use crate::core::frame::MediaTimestamp;

    #[test]
    fn trim_metadata_round_trips_json() {
        let metadata = TrimMetadata {
            duration_nanos: 10_000_000_000,
            audio_activity: vec![AudioActivitySample {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(1_000_000_000),
                rms: 0.01,
            }],
            visual_activity: vec![FrameDiffSample {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(33_333_333),
                change_ratio: 0.002,
            }],
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: TrimMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, metadata);
    }

    #[test]
    fn write_and_read_cut_timeline_file() {
        let path = std::env::temp_dir().join("luzhi-cut-timeline-test.json");
        let timeline = CutTimeline::empty(5_000_000_000);

        TrimMetadataWriter::write_cut_timeline(&path, &timeline).unwrap();
        let parsed = TrimMetadataWriter::read_cut_timeline(&path).unwrap();

        assert_eq!(parsed, timeline);
        let _ = std::fs::remove_file(path);
    }
}
```

- [ ] **Step 4: Export module**

Modify `src-tauri/src/media/mod.rs`:

```rust
pub mod audio_mixer;
pub mod audio_synchronizer;
pub mod cursor_engine;
#[cfg(feature = "ffmpeg")]
pub mod ffmpeg_writer;
pub mod mic_level;
pub mod recording_metadata;
pub mod recording_writer;
pub mod silence_detector;
pub mod trim_metadata;
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::trim_metadata -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/trim_metadata.rs src-tauri/src/media/mod.rs
git commit -m "feat(export): 新增裁剪元数据边车文件"
```

---

### Task 6: Recording Consumer Trim Metadata Collection

**Files:**

- Modify: `src-tauri/src/media/recording_writer.rs`
- Modify: `src-tauri/src/media/ffmpeg_writer.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Extend `RecordingResult` tests first**

Modify the serialization test in `src-tauri/src/media/recording_writer.rs`:

```rust
#[test]
fn recording_result_serializes_sidecar_paths_as_camel_case() {
    let result = RecordingResult {
        duration_secs: 1,
        frame_count: 30,
        mixed_audio_chunk_count: 2,
        output_path: None,
        cursor_metadata_path: Some("/tmp/cursor.json".to_string()),
        effect_timeline_path: Some("/tmp/effects.json".to_string()),
        trim_metadata_path: Some("/tmp/trim-metadata.json".to_string()),
        cut_timeline_path: Some("/tmp/cut-timeline.json".to_string()),
    };

    let json = serde_json::to_string(&result).unwrap();

    assert!(json.contains("\"cursorMetadataPath\":\"/tmp/cursor.json\""));
    assert!(json.contains("\"effectTimelinePath\":\"/tmp/effects.json\""));
    assert!(json.contains("\"trimMetadataPath\":\"/tmp/trim-metadata.json\""));
    assert!(json.contains("\"cutTimelinePath\":\"/tmp/cut-timeline.json\""));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_writer::tests::recording_result_serializes_sidecar_paths_as_camel_case -- --nocapture
```

Expected: FAIL because `trim_metadata_path` and `cut_timeline_path` are missing.

- [ ] **Step 3: Update `RecordingResult`**

Modify `src-tauri/src/media/recording_writer.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingResult {
    pub duration_secs: u64,
    pub frame_count: u64,
    pub mixed_audio_chunk_count: u64,
    pub output_path: Option<String>,
    pub cursor_metadata_path: Option<String>,
    pub effect_timeline_path: Option<String>,
    pub trim_metadata_path: Option<String>,
    pub cut_timeline_path: Option<String>,
}
```

Update every `RecordingResult` constructor in `recording_writer.rs`, `ffmpeg_writer.rs`, and `platform/macos_service.rs` to include:

```rust
trim_metadata_path: None,
cut_timeline_path: None,
```

Update `src/lib/tauri.ts`:

```ts
export type RecordingResult = {
  durationSecs: number
  frameCount: number
  mixedAudioChunkCount: number
  outputPath: string | null
  cursorMetadataPath: string | null
  effectTimelinePath: string | null
  trimMetadataPath: string | null
  cutTimelinePath: string | null
}
```

- [ ] **Step 4: Add consumer metadata collection**

In `src-tauri/src/platform/macos_service.rs`, introduce an internal return type near `MacRecordingService`:

```rust
struct RecordingConsumerOutput {
    result: RecordingResult,
    trim_metadata: crate::media::trim_metadata::TrimMetadata,
}
```

Change `consumer_handle` to:

```rust
consumer_handle: Option<thread::JoinHandle<RecordingConsumerOutput>>,
```

In `consume_frames`, create analyzers before the loop:

```rust
let rms_analyzer = crate::media::silence_detector::AudioRmsAnalyzer::new(
    crate::core::cut::TrimConfig::from_sensitivity(crate::core::cut::TrimSensitivity::Medium),
);
let frame_diff_analyzer = crate::media::silence_detector::FrameDiffAnalyzer::new(64, 36);
let mut previous_frame: Option<crate::core::frame::VideoFrame> = None;
let mut visual_activity = Vec::new();
let mut audio_activity = Vec::new();
```

When draining each video frame, update visual activity from the consumer thread:

```rust
if let Some(previous) = previous_frame.as_ref() {
    if let Ok(diff) = frame_diff_analyzer.diff_pair(previous, frame.as_ref()) {
        visual_activity.push(diff);
    }
}
previous_frame = Some(frame.as_ref().clone());
```

When draining mixed audio, update audio activity:

```rust
let samples = rms_analyzer.analyze_chunks(std::slice::from_ref(&mixed));
audio_activity.extend(samples);
```

After `writer.finish()`, return:

```rust
let result = writer.finish().unwrap_or(RecordingResult {
    duration_secs: 0,
    frame_count: 0,
    mixed_audio_chunk_count: 0,
    output_path: None,
    cursor_metadata_path: None,
    effect_timeline_path: None,
    trim_metadata_path: None,
    cut_timeline_path: None,
});
let duration_nanos = result.duration_secs.saturating_mul(1_000_000_000);
RecordingConsumerOutput {
    result,
    trim_metadata: crate::media::trim_metadata::TrimMetadata {
        duration_nanos,
        audio_activity,
        visual_activity,
    },
}
```

In `stop()`, after joining the consumer, write trim metadata:

```rust
let mut result = consumer_output.result;
let trim_metadata_path = {
    let path = trim_metadata_path();
    match crate::media::trim_metadata::TrimMetadataWriter::write_metadata(
        &path,
        &consumer_output.trim_metadata,
    ) {
        Ok(()) => Some(path.to_string_lossy().to_string()),
        Err(e) => {
            errors.push(format!("裁剪元数据写入失败: {e}"));
            None
        }
    }
};
result.trim_metadata_path = trim_metadata_path;
```

Add helper function next to `cursor_metadata_path()`:

```rust
fn trim_metadata_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("trim-metadata-{millis}-{seq}.json"))
}
```

- [ ] **Step 5: Add focused service test**

Add an inline test in `src-tauri/src/platform/macos_service.rs`:

```rust
#[test]
fn trim_metadata_path_uses_luzhi_recordings_directory() {
    let path = trim_metadata_path();

    assert!(path.to_string_lossy().contains("luzhi-recordings"));
    assert!(path.to_string_lossy().contains("trim-metadata-"));
}
```

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_writer platform::macos_service::tests::trim_metadata_path -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/media/recording_writer.rs src-tauri/src/media/ffmpeg_writer.rs src-tauri/src/platform/macos_service.rs src/lib/tauri.ts
git commit -m "feat(export): 录制期写入裁剪元数据"
```

---

### Task 7: Build Cut Timeline Command

**Files:**

- Modify: `src-tauri/src/app/events.rs`
- Modify: `src-tauri/src/app/error.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add event payload tests**

In `src-tauri/src/app/events.rs`, add:

```rust
/// Summary returned after building a cut timeline.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CutTimelineSummaryPayload {
    pub cut_count: usize,
    pub total_cut_nanos: u64,
    pub cut_timeline_path: String,
}
```

Add test:

```rust
#[test]
fn cut_timeline_summary_serializes_camel_case() {
    let payload = CutTimelineSummaryPayload {
        cut_count: 2,
        total_cut_nanos: 3_000_000_000,
        cut_timeline_path: "/tmp/cuts.json".to_string(),
    };

    let json = serde_json::to_string(&payload).unwrap();

    assert!(json.contains("\"cutCount\":2"));
    assert!(json.contains("\"totalCutNanos\":3000000000"));
    assert!(json.contains("\"cutTimelinePath\":\"/tmp/cuts.json\""));
}
```

- [ ] **Step 2: Run test to verify payload compiles**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::events::tests::cut_timeline_summary_serializes_camel_case -- --nocapture
```

Expected: PASS after adding the payload and test.

- [ ] **Step 3: Add trim error variant**

Modify `src-tauri/src/app/error.rs`:

```rust
TrimProcessingFailed {
    reason: String,
},
```

Add display arm:

```rust
AppError::TrimProcessingFailed { reason } => {
    write!(formatter, "空白裁剪处理失败：{reason}")
}
```

Add test:

```rust
#[test]
fn trim_processing_error_uses_chinese_message() {
    let error = AppError::TrimProcessingFailed {
        reason: "没有裁剪元数据".to_string(),
    };

    assert_eq!(error.to_string(), "空白裁剪处理失败：没有裁剪元数据");
}
```

- [ ] **Step 4: Implement `build_cut_timeline` command**

In `src-tauri/src/lib.rs`, import:

```rust
use app::events::{CutTimelineSummaryPayload, CursorEffectSummaryPayload, MicLevelPayload, PermissionPayload, PostProcessProgressPayload, RecordingStatusPayload};
use core::cut::{TrimConfig, TrimSensitivity};
use media::silence_detector::SilenceDetectorEngine;
use media::trim_metadata::TrimMetadataWriter;
```

Add helper:

```rust
fn cut_timeline_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("cut-timeline-{millis}-{seq}.json"))
}
```

Add command:

```rust
#[tauri::command]
async fn build_cut_timeline(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<CutTimelineSummaryPayload, String> {
    {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        if matches!(
            service.state(),
            RecordingState::Recording | RecordingState::Paused | RecordingState::Processing
        ) {
            return Err("录制进行中，无法构建裁剪时间线。请先停止录制。".to_string());
        }
    }

    let trim_metadata_path = {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        service
            .last_trim_metadata_path()
            .ok_or_else(|| "没有可用的裁剪元数据，请先完成一次录制".to_string())?
    };

    let config = state
        .beautify_config
        .lock()
        .map_err(|_| "美化配置锁已损坏".to_string())?
        .clone();
    let sensitivity = TrimSensitivity::from_str(config.trim_sensitivity.as_str())?;
    let trim_config = TrimConfig::from_sensitivity(sensitivity);

    let _ = app.emit(
        "post-process-progress",
        PostProcessProgressPayload {
            stage: "trim",
            progress: 0,
            error: None,
        },
    );

    let app_for_blocking = app.clone();
    let join_result = tauri::async_runtime::spawn_blocking(move || {
        let metadata = TrimMetadataWriter::read_metadata(PathBuf::from(&trim_metadata_path).as_path())
            .map_err(|error| error.to_string())?;
        let detector = SilenceDetectorEngine::new(trim_config);
        let timeline = detector
            .analyze(&metadata.audio_activity, &metadata.visual_activity, metadata.duration_nanos)
            .map_err(|error| error.to_string())?;
        let path = cut_timeline_path();
        TrimMetadataWriter::write_cut_timeline(&path, &timeline)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((path, timeline))
    })
    .await;

    let (path, timeline) = match join_result {
        Ok(Ok(inner)) => inner,
        Ok(Err(error)) => {
            let _ = app_for_blocking.emit(
                "post-process-progress",
                PostProcessProgressPayload {
                    stage: "trim",
                    progress: 0,
                    error: Some(format!("裁剪时间线构建失败: {error}")),
                },
            );
            return Err(error);
        }
        Err(join_error) => {
            let msg = format!("裁剪时间线构建任务失败: {join_error}");
            let _ = app_for_blocking.emit(
                "post-process-progress",
                PostProcessProgressPayload {
                    stage: "trim",
                    progress: 0,
                    error: Some(msg.clone()),
                },
            );
            return Err(msg);
        }
    };

    {
        let mut service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        service.set_last_cut_timeline_path(Some(path.to_string_lossy().to_string()));
    }

    let _ = app_for_blocking.emit(
        "post-process-progress",
        PostProcessProgressPayload {
            stage: "trim",
            progress: 100,
            error: None,
        },
    );

    Ok(CutTimelineSummaryPayload {
        cut_count: timeline.cuts.len(),
        total_cut_nanos: timeline.total_cut_nanos,
        cut_timeline_path: path.to_string_lossy().to_string(),
    })
}
```

Add command to `tauri::generate_handler!`:

```rust
build_cut_timeline,
```

Add service accessors in `src-tauri/src/platform/macos_service.rs`:

```rust
pub fn last_trim_metadata_path(&self) -> Option<String> {
    self.last_trim_metadata_path.clone()
}

pub fn set_last_cut_timeline_path(&mut self, path: Option<String>) {
    self.last_cut_timeline_path = path;
}
```

Add fields:

```rust
last_trim_metadata_path: Option<String>,
last_cut_timeline_path: Option<String>,
```

Clear them in `start()` and copy them into `RecordingResult` in `stop()`.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::events app::error media::silence_detector media::trim_metadata -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/app/events.rs src-tauri/src/app/error.rs src-tauri/src/lib.rs src-tauri/src/platform/macos_service.rs
git commit -m "feat(export): 接入裁剪时间线命令"
```

---

### Task 8: Structured Trim Export Boundary

**Files:**

- Create: `src-tauri/src/media/trim_exporter.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/app/events.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing trim exporter tests**

Create `src-tauri/src/media/trim_exporter.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::cut::CutTimeline;

    #[test]
    fn mock_exporter_consumes_cut_timeline_without_deleting_original() {
        let mut exporter = MockTrimExporter::new();
        let request = TrimExportRequest {
            input_path: PathBuf::from("/tmp/raw.mov"),
            output_path: PathBuf::from("/tmp/export.mp4"),
            preset: ExportPreset::Bilibili,
            cut_timeline: CutTimeline::empty(10_000_000_000),
        };

        let result = exporter.export(request).unwrap();

        assert_eq!(result.output_path, PathBuf::from("/tmp/export.mp4"));
        assert_eq!(result.cut_count, 0);
        assert_eq!(exporter.requests().len(), 1);
        assert_eq!(exporter.requests()[0].input_path, PathBuf::from("/tmp/raw.mov"));
    }

    #[test]
    fn export_preset_rejects_unknown_value() {
        assert_eq!(ExportPreset::from_str("bilibili").unwrap(), ExportPreset::Bilibili);
        assert!(ExportPreset::from_str("unknown").is_err());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::trim_exporter -- --nocapture
```

Expected: FAIL because exporter types do not exist.

- [ ] **Step 3: Implement exporter boundary**

Replace `src-tauri/src/media/trim_exporter.rs` with:

```rust
use std::path::PathBuf;

use crate::app::error::{AppError, AppResult};
use crate::core::cut::CutTimeline;

/// Export preset selected by the frontend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportPreset {
    Bilibili,
    Douyin,
    Xiaohongshu,
}

impl ExportPreset {
    pub fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "bilibili" => Ok(Self::Bilibili),
            "douyin" => Ok(Self::Douyin),
            "xiaohongshu" => Ok(Self::Xiaohongshu),
            other => Err(format!("未知导出预设：{other}")),
        }
    }
}

/// Structured request for a future FFmpeg binding implementation.
#[derive(Clone, Debug, PartialEq)]
pub struct TrimExportRequest {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub preset: ExportPreset,
    pub cut_timeline: CutTimeline,
}

/// Result of a structured trim export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrimExportResult {
    pub output_path: PathBuf,
    pub cut_count: usize,
}

/// Exporter boundary that consumes CutTimeline without shelling out to FFmpeg CLI.
pub trait TrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult>;
}

/// Test exporter that records requests without touching source files.
#[derive(Default)]
pub struct MockTrimExporter {
    requests: Vec<TrimExportRequest>,
}

impl MockTrimExporter {
    pub fn new() -> Self {
        Self { requests: Vec::new() }
    }

    pub fn requests(&self) -> &[TrimExportRequest] {
        &self.requests
    }
}

impl TrimExporter for MockTrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
        let result = TrimExportResult {
            output_path: request.output_path.clone(),
            cut_count: request.cut_timeline.cuts.len(),
        };
        self.requests.push(request);
        Ok(result)
    }
}

/// Feature-gated production boundary. It intentionally accepts structured data
/// rather than a shell command string, preserving the no-CLI security rule.
#[cfg(feature = "ffmpeg")]
pub struct FfmpegTrimExporter;

#[cfg(feature = "ffmpeg")]
impl TrimExporter for FfmpegTrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
        if request.input_path.as_os_str().is_empty() || request.output_path.as_os_str().is_empty() {
            return Err(AppError::RecordingWriteFailed {
                reason: "裁剪导出路径无效".to_string(),
            });
        }
        Err(AppError::RecordingWriteFailed {
            reason: "FFmpeg 裁剪导出需要生产编码器接入后启用".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::cut::CutTimeline;

    #[test]
    fn mock_exporter_consumes_cut_timeline_without_deleting_original() {
        let mut exporter = MockTrimExporter::new();
        let request = TrimExportRequest {
            input_path: PathBuf::from("/tmp/raw.mov"),
            output_path: PathBuf::from("/tmp/export.mp4"),
            preset: ExportPreset::Bilibili,
            cut_timeline: CutTimeline::empty(10_000_000_000),
        };

        let result = exporter.export(request).unwrap();

        assert_eq!(result.output_path, PathBuf::from("/tmp/export.mp4"));
        assert_eq!(result.cut_count, 0);
        assert_eq!(exporter.requests().len(), 1);
        assert_eq!(exporter.requests()[0].input_path, PathBuf::from("/tmp/raw.mov"));
    }

    #[test]
    fn export_preset_rejects_unknown_value() {
        assert_eq!(ExportPreset::from_str("bilibili").unwrap(), ExportPreset::Bilibili);
        assert!(ExportPreset::from_str("unknown").is_err());
    }
}
```

- [ ] **Step 4: Export module**

Modify `src-tauri/src/media/mod.rs`:

```rust
pub mod trim_exporter;
```

- [ ] **Step 5: Update combined export payload**

In `src-tauri/src/app/events.rs`, add:

```rust
/// Summary returned after export command prepares all Phase 4/5 timelines.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummaryPayload {
    pub frame_count: usize,
    pub click_effect_count: usize,
    pub effect_timeline_path: String,
    pub cut_count: usize,
    pub total_cut_nanos: u64,
    pub cut_timeline_path: Option<String>,
    pub output_path: Option<String>,
}
```

Add serialization test:

```rust
#[test]
fn export_summary_serializes_camel_case() {
    let payload = ExportSummaryPayload {
        frame_count: 10,
        click_effect_count: 1,
        effect_timeline_path: "/tmp/effects.json".to_string(),
        cut_count: 2,
        total_cut_nanos: 3_000_000_000,
        cut_timeline_path: Some("/tmp/cuts.json".to_string()),
        output_path: None,
    };

    let json = serde_json::to_string(&payload).unwrap();

    assert!(json.contains("\"frameCount\":10"));
    assert!(json.contains("\"clickEffectCount\":1"));
    assert!(json.contains("\"cutCount\":2"));
    assert!(json.contains("\"totalCutNanos\":3000000000"));
    assert!(json.contains("\"cutTimelinePath\":\"/tmp/cuts.json\""));
    assert!(json.contains("\"outputPath\":null"));
}
```

- [ ] **Step 6: Update `export_video` command**

In `src-tauri/src/lib.rs`, change return type:

```rust
async fn export_video(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    preset: String,
) -> Result<ExportSummaryPayload, String>
```

Use `ExportPreset::from_str(&preset)?` for validation.

Build cursor timeline first:

```rust
let cursor = build_cursor_effect_timeline(app.clone(), state.clone()).await?;
```

If `beautify_config.auto_trim_silences` is true, call:

```rust
let cut = build_cut_timeline(app, state).await?;
```

Return:

```rust
Ok(ExportSummaryPayload {
    frame_count: cursor.frame_count,
    click_effect_count: cursor.click_effect_count,
    effect_timeline_path: cursor.effect_timeline_path,
    cut_count: cut.as_ref().map(|summary| summary.cut_count).unwrap_or(0),
    total_cut_nanos: cut.as_ref().map(|summary| summary.total_cut_nanos).unwrap_or(0),
    cut_timeline_path: cut.map(|summary| summary.cut_timeline_path),
    output_path: None,
})
```

Keep `output_path: None` until a playable production export file exists.

- [ ] **Step 7: Run tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::trim_exporter app::events -- --nocapture
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/media/trim_exporter.rs src-tauri/src/media/mod.rs src-tauri/src/app/events.rs src-tauri/src/lib.rs
git commit -m "feat(export): 定义裁剪导出边界"
```

---

### Task 9: Frontend Command Types And Preview Wiring

**Files:**

- Modify: `src/lib/tauri.ts`
- Modify: `src/components/preview-view.tsx`
- Modify: `src/App.test.tsx`

- [ ] **Step 1: Update TypeScript command types**

Modify `src/lib/tauri.ts`:

```ts
export type CutTimelineSummary = {
  cutCount: number
  totalCutNanos: number
  cutTimelinePath: string
}

export type ExportSummary = {
  frameCount: number
  clickEffectCount: number
  effectTimelinePath: string
  cutCount: number
  totalCutNanos: number
  cutTimelinePath: string | null
  outputPath: string | null
}

export async function buildCutTimeline(): Promise<CutTimelineSummary> {
  return invoke<CutTimelineSummary>('build_cut_timeline')
}

export async function exportVideo(preset: ExportPreset): Promise<ExportSummary> {
  return invoke<ExportSummary>('export_video', { preset })
}
```

- [ ] **Step 2: Add failing frontend tests**

Add to `src/App.test.tsx`:

```ts
it('builds cut timeline when auto trim is toggled in preview', async () => {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'get_beautify_config') {
      return Promise.resolve({
        cursorMagnification: true, magnificationFactor: 2,
        cursorSmoothing: true, autoTrimSilences: false, trimSensitivity: 'medium',
      })
    }
    if (command === 'set_beautify_config') return Promise.resolve()
    if (command === 'build_cursor_effect_timeline') return Promise.resolve({ frameCount: 1, clickEffectCount: 0, effectTimelinePath: '/tmp/effects.json' })
    if (command === 'build_cut_timeline') return Promise.resolve({ cutCount: 1, totalCutNanos: 3_000_000_000, cutTimelinePath: '/tmp/cuts.json' })
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)
  await screen.findByText('预览与美化')
  invokeMock.mockClear()

  const switches = screen.getAllByRole('switch')
  fireEvent.click(switches[2])

  await vi.waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('build_cut_timeline', undefined)
  })
})

it('exports with auto trim enabled after flushing config', async () => {
  const callOrder: string[] = []
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'get_beautify_config') {
      return Promise.resolve({
        cursorMagnification: true, magnificationFactor: 2,
        cursorSmoothing: true, autoTrimSilences: false, trimSensitivity: 'medium',
      })
    }
    if (command === 'set_beautify_config') {
      callOrder.push('set_beautify_config')
      return Promise.resolve()
    }
    if (command === 'export_video') {
      callOrder.push('export_video')
      return Promise.resolve({
        frameCount: 1,
        clickEffectCount: 0,
        effectTimelinePath: '/tmp/effects.json',
        cutCount: 1,
        totalCutNanos: 3_000_000_000,
        cutTimelinePath: '/tmp/cuts.json',
        outputPath: null,
      })
    }
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)
  await screen.findByText('预览与美化')
  invokeMock.mockClear()

  const switches = screen.getAllByRole('switch')
  fireEvent.click(switches[2])

  const exportButtons = screen.getAllByRole('button', { name: '导出' })
  await act(async () => {
    fireEvent.click(exportButtons[0])
  })

  await vi.waitFor(() => {
    expect(callOrder).toEqual(['set_beautify_config', 'export_video'])
  })
})
```

- [ ] **Step 3: Run tests to verify failure**

Run:

```bash
npm test -- --run
```

Expected: FAIL because `buildCutTimeline` is not called from Preview.

- [ ] **Step 4: Wire Preview to build cut timeline**

Modify imports in `src/components/preview-view.tsx`:

```ts
buildCutTimeline,
```

In the debounce chain after `buildCursorEffectTimeline()`:

```ts
return buildCursorEffectTimeline().then(() => {
  if (nextConfig.autoTrimSilences) {
    return buildCutTimeline()
  }
})
```

In `messageForBeautifyError`, preserve backend trim messages:

```ts
return msg.includes('已录入系统光标') ||
  msg.includes('光标元数据为空') ||
  msg.includes('裁剪元数据') ||
  msg.includes('裁剪时间线')
  ? msg
  : fallback
```

In `handleExport`, keep the existing `flushPendingConfig().then(() => exportVideo(preset))` path. No frontend-side cut logic belongs here because Rust owns the export contract.

- [ ] **Step 5: Run frontend tests**

Run:

```bash
npm test -- --run
```

Expected: PASS with the new tests included.

- [ ] **Step 6: Architecture scan**

Run:

```bash
rg -n "AudioActivitySample|FrameDiffSample|MixedAudioChunk|VideoFrame|cursor_samples|visual_activity|audio_activity" src
```

Expected: No hits in React/TS files except type names intentionally absent. If the command finds these in `src/`, remove the stream/data exposure before proceeding.

- [ ] **Step 7: Commit**

```bash
git add src/lib/tauri.ts src/components/preview-view.tsx src/App.test.tsx
git commit -m "feat(ui): 接入自动裁剪命令"
```

---

### Task 10: Checklist, Handoff, And Final Verification

**Files:**

- Modify: `tests/phase-5-w9-w10-checklist.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Update Phase 5 checklist**

Update `tests/phase-5-w9-w10-checklist.md` after running the final verification commands in Step 3. Keep unchecked manual gates explicit. Copy the exact test counts and warning counts from terminal output; do not estimate them and do not reuse Phase 4 counts. Use this structure:

```markdown
## Verification Summary (2026-05-29, Phase 5)

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, exact test count copied from command output
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, exact warning count copied from command output
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS
- `npm run build`: PASS
- `npm test -- --run`: PASS, exact test count copied from command output

## Remaining Manual Gates

- [ ] Real playable FFmpeg trimmed export once production encoder/muxer is connected.
- [ ] 10 minute 1080p trim metadata and cut timeline memory-pressure check.
- [ ] Manual review that original recording artifacts are preserved.
- [ ] BUG.md prevention scan for drag/whileTap/click-through regressions.
```

- [ ] **Step 2: Update HANDOFF**

Add a new top entry under `## 工作任务记录` and keep only the most recent 7 entries. Run Step 3 first, then write the exact Rust/frontend test counts and warning count from command output into this section:

```markdown
### 2026-05-29：Phase 5 空白段检测与自动裁剪实现

输入文件：

- `docs/superpowers/plans/2026-05-29-phase-5-silence-trimming.md`
- `tests/phase-5-w9-w10-checklist.md`

本轮完成：

1. `core/cut.rs` 建立 `CutTimeline` / `CutSegment` / `KeepSegment` / activity sample serde 模型。
2. `media/silence_detector.rs` 实现音频 RMS、低分辨率帧差分、保守候选合并和裁剪缓冲策略。
3. `media/trim_metadata.rs` 建立 trim metadata 与 cut timeline JSON sidecar。
4. `MacRecordingService` 消费线程采集 bounded trim metadata，停止录制后写入 sidecar。
5. Tauri 命令 `build_cut_timeline` 和 `export_video` 接入裁剪时间线边界。
6. Preview UI 接入 auto-trim command path，前端仍不接触音视频帧或 activity stream。

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` 通过，并记录命令输出中的精确测试数量
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 无 error，并记录命令输出中的精确 warning 数量
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `npm run build` 通过
- `npm test -- --run` 通过，并记录命令输出中的精确测试数量

剩余待完成（不阻塞 Phase 5 code contract，但需关注）：

- 真实可播放裁剪导出需生产 FFmpeg encoder/muxer 接入后人工验收。
- 长录制 trim metadata / cut timeline 内存峰值压力测试。
- 人工确认原始素材保留与重新导出路径。
```

- [ ] **Step 3: Run final verification commands**

Run:

```bash
git diff --check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n "whileTap|motion\\.div|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents" src src-tauri/src src-tauri/capabilities src-tauri/tauri.conf.json
rg -n "AudioActivitySample|FrameDiffSample|MixedAudioChunk|VideoFrame|visual_activity|audio_activity" src
```

Expected:

- All build/test commands exit 0.
- BUG.md scan has no new `data-tauri-drag-region="false"` wrapper and no `motion.div whileTap` direct Button parent pattern.
- Architecture scan has no media/activity stream types in frontend code.

- [ ] **Step 4: Commit docs**

```bash
git add tests/phase-5-w9-w10-checklist.md HANDOFF.md
git commit -m "docs(export): 更新Phase 5验证记录"
```

---

## 3. Self-Review

Spec coverage:

- Audio RMS configurable 500ms to 1000ms: Task 1 and Task 2.
- Silence, short pause, background noise: Task 2 and Task 4.
- Low-resolution grayscale frame diff: Task 3.
- Static frames, animation, cursor-like movement: Task 3.
- No capture callback blocking: Task 6 computes in consumer thread; final scan in Task 10.
- Dual-signal cut condition: Task 4.
- Minimum durations, 5 to 8 second candidate threshold, 300 to 500ms buffer, adjacent merge: Task 1 and Task 4.
- Explainable `CutTimeline`: Task 1, Task 5, Task 7.
- FFmpeg boundary consumes `CutTimeline` without CLI string building: Task 8.
- Original artifact preservation: Task 8 mock exporter and Task 10 manual gate.
- Frontend command boundary without media streams: Task 9 and Task 10 scan.

Placeholder scan:

- This plan contains no `TBD`, no unspecified test names, and no undefined command names.
- Production playable FFmpeg output is explicitly a manual gate because current codebase has no playable raw writer.

Type consistency:

- Rust uses `CutTimelineSummaryPayload` for `build_cut_timeline`.
- Rust uses `ExportSummaryPayload` for `export_video`.
- TypeScript uses `CutTimelineSummary` and `ExportSummary`.
- `RecordingResult` adds `trimMetadataPath` and `cutTimelinePath` in Rust/TS.

---

## 4. Execution Notes

- Use a worktree at execution time if other Phase 4 review changes are still in the main working tree.
- Do not change `Cargo.toml` dependency versions without explicit human approval.
- Do not run `git push`, `npm publish`, or merge commands.
- Request code review after each natural checkpoint: Task 4, Task 7, Task 9, and Task 10.
