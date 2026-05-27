# Phase 4 Cursor Effects Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Phase 4 cursor effects foundation: record cursor metadata during macOS recording, generate a tested `EffectTimeline` with smoothing, Bezier interpolation, and click magnification, and wire the post-recording command/UI path without moving media frames through React.

**Architecture:** Cursor position and click metadata are collected in Rust during recording using the same session-relative time base as media frames. The cursor engine is a pure Rust post-processing module that transforms metadata into an `EffectTimeline`; rendering into final video stays behind an export boundary because the production FFmpeg compositor is scheduled for Phase 6. React only sends config and export commands and receives lightweight summaries, never cursor samples as a stream and never video/audio frames.

**Tech Stack:** Tauri 2, React + TypeScript + Tailwind, Rust, ScreenCaptureKit, CoreGraphics FFI, serde/serde_json, Vitest, React Testing Library, Cargo test/clippy/fmt.

---

## 0. Scope And Assumptions

Inputs:

- `HANDOFF.md`
- `docs/architecture/project-architecture-and-overall-planning.md`
- `tests/phase-4-w7-w8-checklist.md`
- `.codex/rules/0-global.md`
- `.codex/rules/1-coding-style.md`
- `.codex/rules/2-testing.md`
- `.codex/rules/3-git-commit.md`
- `.codex/rules/4-security.md`
- `.codex/rules/5-docs.md`
- `BUG.md`
- `reference/ui/ui_spec.md`

Assumptions:

- Phase 4 is macOS-first and must not add Windows-specific behavior beyond keeping cross-platform module boundaries compilable where they already compile.
- No `Cargo.toml` core dependency version changes are needed.
- Cursor effects are post-recording. The recording callback path may enqueue lightweight cursor metadata through a bounded runtime, but smoothing, interpolation, effect construction, and JSON writing must not run on ScreenCaptureKit callbacks.
- The current repository has no production video compositor. Phase 4 must produce and consume `EffectTimeline` deterministically, write sidecar metadata/timeline JSON, and expose the export boundary. Pixel baking into MP4 is verified with a counting/mock renderer now and connected to the production FFmpeg path in Phase 6.
- `SCStreamConfiguration::showsCursor` currently records the system cursor into raw frames. Phase 4 must explicitly decide when to set it to `false` to avoid double cursors. The plan chooses to keep raw recording behavior unchanged until the mock effect pipeline is wired, then add a guarded switch for cursor-hidden recording when `cursorMagnification` or `cursorSmoothing` is enabled.

Out of scope:

- Subtitles, summaries, templates, background beautification, automatic zoom-to-key-area, platform publishing APIs.
- Implementing Phase 5 silence trimming.
- Implementing Phase 6 production FFmpeg video export.

Success criteria:

- Rust unit tests cover empty input, single point, jitter smoothing, fast jumps, 30fps/60fps adaptive windows, Bezier frame coverage, click state transitions, consecutive clicks, and `EffectTimeline` serde round-trip.
- Recording result contains a cursor metadata sidecar path and, after post-processing, an effect timeline sidecar path.
- Frontend invokes Rust beautify/export commands and keeps media-frame red lines intact.
- `tests/phase-4-w7-w8-checklist.md` is updated with automatic/manual status and remaining Native Safety Gate items.

Manual gates:

- **Native Safety Gate:** `platform/macos/cursor_source.rs` CoreGraphics FFI must be reviewed line-by-line for memory ownership and permission implications.
- **Performance Gate:** cursor metadata polling must be confirmed not to block capture callbacks and not to grow memory without bounds.
- **Architecture Gate:** verify `src/lib/tauri.ts`, `src/App.tsx`, and `src/components/preview-view.tsx` do not receive cursor sample streams, video frames, or audio frames.
- **BUG.md Gate:** confirm no new `data-tauri-drag-region="false"` wrappers and no `motion.div whileTap` direct parent around interactive buttons.

---

## 1. File Map

### Create

- `src-tauri/src/core/timeline.rs` — cursor metadata types, cursor frame samples, click effects, and `EffectTimeline` serde model.
- `src-tauri/src/core/processor.rs` — `CursorProcessor` trait boundary used by app logic and future export code.
- `src-tauri/src/media/cursor_engine.rs` — pure smoothing, Bezier interpolation, click animation, and timeline builder.
- `src-tauri/src/media/recording_metadata.rs` — JSON sidecar model and writer for recording metadata/effect timelines.
- `src-tauri/src/app/cursor_metadata_runtime.rs` — polling runtime and pure recorder that converts cursor snapshots into `CursorSample`/`CursorClick`.
- `src-tauri/src/platform/macos/cursor_source.rs` — macOS CoreGraphics cursor snapshot source.

### Modify

- `src-tauri/src/core/frame.rs` — derive serde for `MediaTimestamp`.
- `src-tauri/src/core/mod.rs` — export `timeline` and `processor`.
- `src-tauri/src/media/mod.rs` — export `cursor_engine` and `recording_metadata`.
- `src-tauri/src/app/mod.rs` — export `cursor_metadata_runtime`.
- `src-tauri/src/app/events.rs` — add lightweight `CursorEffectSummaryPayload` and `PostProcessProgressPayload`.
- `src-tauri/src/app/error.rs` — add cursor/post-process error variants with Chinese messages.
- `src-tauri/src/media/recording_writer.rs` — extend `RecordingResult` with cursor/effect sidecar metadata fields.
- `src-tauri/src/platform/macos/mod.rs` — export `cursor_source`.
- `src-tauri/src/platform/macos/screen_capture_kit.rs` — expose cursor visibility config through `CaptureConfig` integration when enabled.
- `src-tauri/src/platform/macos_service.rs` — start/stop cursor metadata runtime and save metadata sidecar after recording stops.
- `src-tauri/src/lib.rs` — store beautify config, add `set_beautify_config`, `build_cursor_effect_timeline`, and `export_video` command boundary.
- `src/lib/tauri.ts` — align command payloads and return types.
- `src/components/preview-view.tsx` — replace console-only beautify/export hooks with Tauri calls.
- `src/App.test.tsx` — add frontend command wiring tests.
- `tests/phase-4-w7-w8-checklist.md` — record automatic/manual verification status.
- `HANDOFF.md` — update only after implementation and verification are complete.

### Reference Only

- `docs/architecture/project-architecture-and-overall-planning.md`
- `BUG.md`
- `reference/ui/ui_spec.md`

---

## 2. Phase Breakdown

- **Phase A:** Core timeline model and processor trait.
- **Phase B:** Pure cursor algorithm engine.
- **Phase C:** Cursor metadata collection and macOS source.
- **Phase D:** Recording-service integration and sidecar files.
- **Phase E:** Tauri command/UI wiring.
- **Phase F:** Verification, checklist, handoff, and review notes.

Each task below is intentionally small enough to execute and review independently.

---

### Task 1: Timeline Model And Serde Contract

**Files:**

- Create: `src-tauri/src/core/timeline.rs`
- Modify: `src-tauri/src/core/frame.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Test: inline Rust tests in `src-tauri/src/core/timeline.rs` and `src-tauri/src/core/frame.rs`

- [ ] **Step 1: Write failing timeline serde tests**

Add this test module to the new file `src-tauri/src/core/timeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;

    #[test]
    fn cursor_sample_serializes_camel_case() {
        let sample = CursorSample {
            timestamp: MediaTimestamp::from_nanos(1_000_000),
            x: 120.5,
            y: 240.25,
        };

        let json = serde_json::to_string(&sample).unwrap();

        assert!(json.contains("\"timestamp\""));
        assert!(json.contains("\"nanos\":1000000"));
        assert!(json.contains("\"x\":120.5"));
        assert!(json.contains("\"y\":240.25"));
    }

    #[test]
    fn effect_timeline_round_trips() {
        let timeline = EffectTimeline {
            fps: 60,
            duration_nanos: 33_333_333,
            frames: vec![
                CursorFrame {
                    timestamp: MediaTimestamp::from_nanos(0),
                    x: 10.0,
                    y: 10.0,
                    scale: 1.0,
                    opacity: 1.0,
                },
                CursorFrame {
                    timestamp: MediaTimestamp::from_nanos(16_666_667),
                    x: 20.0,
                    y: 20.0,
                    scale: 1.5,
                    opacity: 1.0,
                },
            ],
            click_effects: vec![CursorClickEffect {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(300_000_000),
                x: 10.0,
                y: 10.0,
                max_scale: 2.0,
                peak_opacity: 0.35,
            }],
        };

        let json = serde_json::to_string(&timeline).unwrap();
        let parsed: EffectTimeline = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, timeline);
    }

    #[test]
    fn click_phase_and_button_use_stable_names() {
        let click = CursorClick {
            timestamp: MediaTimestamp::from_nanos(42),
            button: MouseButton::Left,
            phase: ClickPhase::Down,
            x: 1.0,
            y: 2.0,
        };

        let json = serde_json::to_string(&click).unwrap();

        assert!(json.contains("\"button\":\"left\""));
        assert!(json.contains("\"phase\":\"down\""));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::timeline -- --nocapture
```

Expected: FAIL because `src-tauri/src/core/timeline.rs` and timeline types do not exist.

- [ ] **Step 3: Implement timeline types**

Create `src-tauri/src/core/timeline.rs` with:

```rust
use serde::{Deserialize, Serialize};

use crate::core::frame::MediaTimestamp;

/// Cursor position sampled during recording using the shared media time base.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorSample {
    pub timestamp: MediaTimestamp,
    pub x: f32,
    pub y: f32,
}

/// Mouse button recorded for click effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Click transition phase recorded from the cursor source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClickPhase {
    Down,
    Up,
}

/// Cursor click event with position captured at transition time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorClick {
    pub timestamp: MediaTimestamp,
    pub button: MouseButton,
    pub phase: ClickPhase,
    pub x: f32,
    pub y: f32,
}

/// Per-video-frame cursor effect state used by the post-process renderer.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorFrame {
    pub timestamp: MediaTimestamp,
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub opacity: f32,
}

/// Click magnification effect window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorClickEffect {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub x: f32,
    pub y: f32,
    pub max_scale: f32,
    pub peak_opacity: f32,
}

/// Complete cursor effect timeline generated after recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectTimeline {
    pub fps: u32,
    pub duration_nanos: u64,
    pub frames: Vec<CursorFrame>,
    pub click_effects: Vec<CursorClickEffect>,
}
```

Modify `src-tauri/src/core/frame.rs` imports and derive for `MediaTimestamp`:

```rust
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Monotonic timestamp shared by video frames, audio chunks, and UI events.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct MediaTimestamp {
    pub nanos: u64,
}
```

Modify `src-tauri/src/core/mod.rs`:

```rust
pub mod capture;
pub mod clock;
pub mod config;
pub mod frame;
pub mod media_channel;
pub mod timeline;
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core:: -- --nocapture
```

Expected: PASS for timeline serde tests and existing frame tests. Do not export `processor` in this task; `processor.rs` is created in Task 2.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/timeline.rs src-tauri/src/core/frame.rs src-tauri/src/core/mod.rs
git commit -m "feat(cursor): 建立光标效果时间线模型"
```

---

### Task 2: CursorProcessor Trait Boundary

**Files:**

- Create: `src-tauri/src/core/processor.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Test: inline Rust tests compile through Task 5 implementation

- [ ] **Step 1: Write the trait contract**

Create `src-tauri/src/core/processor.rs`:

```rust
use crate::app::error::AppResult;
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
```

- [ ] **Step 2: Export the processor module**

Modify `src-tauri/src/core/mod.rs`:

```rust
pub mod capture;
pub mod clock;
pub mod config;
pub mod frame;
pub mod media_channel;
pub mod processor;
pub mod timeline;
```

- [ ] **Step 3: Run compile check to verify missing implementations are acceptable**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::processor -- --nocapture
```

Expected: PASS because `processor.rs` now exists and `core/mod.rs` exports it.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/core/processor.rs src-tauri/src/core/mod.rs
git commit -m "feat(cursor): 定义光标处理器边界"
```

---

### Task 3: Adaptive Moving Average Smoother

**Files:**

- Create: `src-tauri/src/media/cursor_engine.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Test: inline Rust tests in `src-tauri/src/media/cursor_engine.rs`

- [ ] **Step 1: Write failing smoother tests**

Add to `src-tauri/src/media/cursor_engine.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::CursorSample;

    fn sample(nanos: u64, x: f32, y: f32) -> CursorSample {
        CursorSample {
            timestamp: MediaTimestamp::from_nanos(nanos),
            x,
            y,
        }
    }

    fn total_variation(samples: &[CursorSample]) -> f32 {
        samples
            .windows(2)
            .map(|pair| {
                let dx = pair[1].x - pair[0].x;
                let dy = pair[1].y - pair[0].y;
                (dx * dx + dy * dy).sqrt()
            })
            .sum()
    }

    #[test]
    fn smoothing_empty_input_returns_empty() {
        let smoother = CursorSmoother::new(SmoothingConfig::for_fps(30));

        assert!(smoother.smooth(&[]).is_empty());
    }

    #[test]
    fn smoothing_single_point_keeps_position() {
        let smoother = CursorSmoother::new(SmoothingConfig::for_fps(30));
        let input = vec![sample(0, 10.0, 20.0)];

        let output = smoother.smooth(&input);

        assert_eq!(output, input);
    }

    #[test]
    fn high_frequency_jitter_is_reduced() {
        let smoother = CursorSmoother::new(SmoothingConfig::for_fps(60));
        let input = vec![
            sample(0, 100.0, 100.0),
            sample(16_666_667, 103.0, 99.0),
            sample(33_333_334, 97.0, 101.0),
            sample(50_000_001, 102.0, 98.0),
            sample(66_666_668, 98.0, 102.0),
            sample(83_333_335, 101.0, 100.0),
        ];

        let output = smoother.smooth(&input);

        assert_eq!(output.len(), input.len());
        assert!(total_variation(&output) < total_variation(&input));
    }

    #[test]
    fn fast_cross_region_jump_is_preserved() {
        let smoother = CursorSmoother::new(SmoothingConfig::for_fps(60));
        let input = vec![
            sample(0, 10.0, 10.0),
            sample(16_666_667, 12.0, 11.0),
            sample(33_333_334, 900.0, 700.0),
            sample(50_000_001, 902.0, 701.0),
        ];

        let output = smoother.smooth(&input);

        assert!(output[2].x > 760.0, "jump was over-smoothed: {}", output[2].x);
        assert!(output[2].y > 590.0, "jump was over-smoothed: {}", output[2].y);
    }

    #[test]
    fn adaptive_window_differs_between_30fps_and_60fps() {
        assert_eq!(SmoothingConfig::for_fps(30).window_size, 5);
        assert_eq!(SmoothingConfig::for_fps(60).window_size, 9);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::cursor_engine -- --nocapture
```

Expected: FAIL because `CursorSmoother` and `SmoothingConfig` do not exist.

- [ ] **Step 3: Implement smoother**

Create the top of `src-tauri/src/media/cursor_engine.rs`:

```rust
use crate::app::error::{AppError, AppResult};
use crate::core::frame::MediaTimestamp;
use crate::core::processor::CursorProcessor;
use crate::core::timeline::{
    ClickPhase, CursorClick, CursorClickEffect, CursorFrame, CursorSample, EffectTimeline,
    MouseButton,
};

const DEFAULT_JUMP_THRESHOLD_PIXELS: f32 = 240.0;

/// Smoothing settings derived from the capture frame rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmoothingConfig {
    pub window_size: usize,
    pub jump_threshold_pixels: u32,
}

impl SmoothingConfig {
    pub fn for_fps(fps: u32) -> Self {
        let window_size = if fps <= 30 {
            5
        } else if fps >= 60 {
            9
        } else {
            7
        };

        Self {
            window_size,
            jump_threshold_pixels: DEFAULT_JUMP_THRESHOLD_PIXELS as u32,
        }
    }
}

/// Adaptive moving-average smoother for cursor samples.
pub struct CursorSmoother {
    config: SmoothingConfig,
}

impl CursorSmoother {
    pub fn new(config: SmoothingConfig) -> Self {
        Self { config }
    }

    pub fn smooth(&self, samples: &[CursorSample]) -> Vec<CursorSample> {
        if samples.len() <= 1 {
            return samples.to_vec();
        }

        let radius = self.config.window_size / 2;
        let jump_threshold = self.config.jump_threshold_pixels as f32;

        samples
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                if is_large_jump(samples, index, jump_threshold) {
                    return *sample;
                }

                let start = index.saturating_sub(radius);
                let end = (index + radius + 1).min(samples.len());
                let window = &samples[start..end];

                let (sum_x, sum_y) = window.iter().fold((0.0f32, 0.0f32), |acc, item| {
                    (acc.0 + item.x, acc.1 + item.y)
                });
                let count = window.len() as f32;

                CursorSample {
                    timestamp: sample.timestamp,
                    x: sum_x / count,
                    y: sum_y / count,
                }
            })
            .collect()
    }
}

fn is_large_jump(samples: &[CursorSample], index: usize, threshold: f32) -> bool {
    if index == 0 {
        return false;
    }

    let previous = samples[index - 1];
    let current = samples[index];
    let dx = current.x - previous.x;
    let dy = current.y - previous.y;
    (dx * dx + dy * dy).sqrt() >= threshold
}
```

Modify `src-tauri/src/media/mod.rs`:

```rust
pub mod audio_mixer;
pub mod audio_synchronizer;
pub mod cursor_engine;
#[cfg(feature = "ffmpeg")]
pub mod ffmpeg_writer;
pub mod mic_level;
pub mod recording_writer;
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::cursor_engine -- --nocapture
```

Expected: PASS for the five smoother tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/cursor_engine.rs src-tauri/src/media/mod.rs
git commit -m "feat(cursor): 实现自适应光标平滑"
```

---

### Task 4: Bezier Frame Interpolation

**Files:**

- Modify: `src-tauri/src/media/cursor_engine.rs`
- Test: inline Rust tests in `src-tauri/src/media/cursor_engine.rs`

- [ ] **Step 1: Add failing interpolation tests**

Append inside the existing `tests` module in `src-tauri/src/media/cursor_engine.rs`:

```rust
#[test]
fn interpolation_empty_input_returns_empty() {
    let interpolator = BezierInterpolator::new(30);

    assert!(interpolator.sample_frames(&[], 1_000_000_000).is_empty());
}

#[test]
fn interpolation_single_point_covers_each_frame_timestamp() {
    let interpolator = BezierInterpolator::new(30);
    let input = vec![sample(0, 50.0, 80.0)];

    let frames = interpolator.sample_frames(&input, 100_000_000);

    assert_eq!(frames.len(), 4);
    assert_eq!(frames[0].timestamp.nanos, 0);
    assert_eq!(frames[1].timestamp.nanos, 33_333_333);
    assert_eq!(frames[2].timestamp.nanos, 66_666_666);
    assert_eq!(frames[3].timestamp.nanos, 99_999_999);
    assert!(frames.iter().all(|frame| frame.x == 50.0 && frame.y == 80.0));
}

#[test]
fn interpolation_reaches_last_sample_position() {
    let interpolator = BezierInterpolator::new(60);
    let input = vec![
        sample(0, 0.0, 0.0),
        sample(50_000_000, 30.0, 30.0),
        sample(100_000_000, 120.0, 80.0),
    ];

    let frames = interpolator.sample_frames(&input, 100_000_000);
    let last = frames.last().unwrap();

    assert!(last.x > 100.0, "last x too far from final sample: {}", last.x);
    assert!(last.y > 65.0, "last y too far from final sample: {}", last.y);
}

#[test]
fn interpolation_uses_60fps_frame_interval() {
    let interpolator = BezierInterpolator::new(60);
    let input = vec![sample(0, 0.0, 0.0), sample(50_000_000, 60.0, 0.0)];

    let frames = interpolator.sample_frames(&input, 50_000_000);

    assert_eq!(frames.len(), 4);
    assert_eq!(frames[1].timestamp.nanos, 16_666_666);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::cursor_engine::tests::interpolation -- --nocapture
```

Expected: FAIL because `BezierInterpolator` does not exist.

- [ ] **Step 3: Implement Bezier interpolation**

Add below `CursorSmoother` in `src-tauri/src/media/cursor_engine.rs`:

```rust
/// Samples smoothed cursor samples at video-frame timestamps using cubic Bezier segments.
pub struct BezierInterpolator {
    fps: u32,
}

impl BezierInterpolator {
    pub fn new(fps: u32) -> Self {
        Self { fps: fps.max(1) }
    }

    pub fn sample_frames(&self, samples: &[CursorSample], duration_nanos: u64) -> Vec<CursorFrame> {
        if samples.is_empty() {
            return Vec::new();
        }

        let frame_interval = 1_000_000_000u64 / self.fps as u64;
        let mut frames = Vec::new();
        let mut timestamp = 0u64;

        while timestamp <= duration_nanos {
            let position = if samples.len() == 1 {
                samples[0]
            } else {
                self.sample_at(samples, timestamp)
            };

            frames.push(CursorFrame {
                timestamp: MediaTimestamp::from_nanos(timestamp),
                x: position.x,
                y: position.y,
                scale: 1.0,
                opacity: 1.0,
            });

            timestamp = timestamp.saturating_add(frame_interval);
            if frame_interval == 0 {
                break;
            }
        }

        frames
    }

    fn sample_at(&self, samples: &[CursorSample], timestamp: u64) -> CursorSample {
        let segment_index = samples
            .windows(2)
            .position(|pair| {
                timestamp >= pair[0].timestamp.nanos && timestamp <= pair[1].timestamp.nanos
            })
            .unwrap_or_else(|| {
                if timestamp < samples[0].timestamp.nanos {
                    0
                } else {
                    samples.len().saturating_sub(2)
                }
            });

        let p0 = samples[segment_index.saturating_sub(1)];
        let p1 = samples[segment_index];
        let p2 = samples[(segment_index + 1).min(samples.len() - 1)];
        let p3 = samples[(segment_index + 2).min(samples.len() - 1)];

        let span = p2.timestamp.nanos.saturating_sub(p1.timestamp.nanos).max(1);
        let t = ((timestamp.saturating_sub(p1.timestamp.nanos)) as f32 / span as f32).clamp(0.0, 1.0);

        let distance = distance_between(p1, p2);
        let strength = (distance / 240.0).clamp(0.15, 0.65);

        let c1 = (
            p1.x + (p2.x - p0.x) * strength / 3.0,
            p1.y + (p2.y - p0.y) * strength / 3.0,
        );
        let c2 = (
            p2.x - (p3.x - p1.x) * strength / 3.0,
            p2.y - (p3.y - p1.y) * strength / 3.0,
        );

        let (x, y) = cubic_bezier((p1.x, p1.y), c1, c2, (p2.x, p2.y), t);

        CursorSample {
            timestamp: MediaTimestamp::from_nanos(timestamp),
            x,
            y,
        }
    }
}

fn distance_between(a: CursorSample, b: CursorSample) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

fn cubic_bezier(
    p0: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    p3: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let inv = 1.0 - t;
    let b0 = inv * inv * inv;
    let b1 = 3.0 * inv * inv * t;
    let b2 = 3.0 * inv * t * t;
    let b3 = t * t * t;

    (
        b0 * p0.0 + b1 * c1.0 + b2 * c2.0 + b3 * p3.0,
        b0 * p0.1 + b1 * c1.1 + b2 * c2.1 + b3 * p3.1,
    )
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::cursor_engine -- --nocapture
```

Expected: PASS for smoother and interpolation tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/cursor_engine.rs
git commit -m "feat(cursor): 实现贝塞尔轨迹插值"
```

---

### Task 5: Click Magnification State Machine

**Files:**

- Modify: `src-tauri/src/media/cursor_engine.rs`
- Test: inline Rust tests in `src-tauri/src/media/cursor_engine.rs`

- [ ] **Step 1: Add failing click state tests**

Append inside `tests` in `src-tauri/src/media/cursor_engine.rs`:

```rust
fn click(nanos: u64, phase: ClickPhase, x: f32, y: f32) -> CursorClick {
    CursorClick {
        timestamp: MediaTimestamp::from_nanos(nanos),
        button: MouseButton::Left,
        phase,
        x,
        y,
    }
}

#[test]
fn click_state_machine_transitions_through_expected_states() {
    let mut machine = ClickAnimationMachine::new(ClickAnimationConfig::default());

    assert_eq!(machine.state(), ClickAnimationState::Idle);

    machine.apply_click(click(0, ClickPhase::Down, 10.0, 20.0));
    assert_eq!(machine.state(), ClickAnimationState::PressedExpand);

    machine.advance_to(MediaTimestamp::from_nanos(140_000_000));
    assert_eq!(machine.state(), ClickAnimationState::Hold);

    machine.apply_click(click(160_000_000, ClickPhase::Up, 10.0, 20.0));
    assert_eq!(machine.state(), ClickAnimationState::ReleaseShrink);

    machine.advance_to(MediaTimestamp::from_nanos(400_000_000));
    assert_eq!(machine.state(), ClickAnimationState::Idle);
}

#[test]
fn click_effect_has_expand_and_shrink_window() {
    let builder = ClickEffectBuilder::new(ClickAnimationConfig::default());
    let effects = builder.build(&[
        click(0, ClickPhase::Down, 10.0, 20.0),
        click(150_000_000, ClickPhase::Up, 11.0, 21.0),
    ]);

    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].start.nanos, 0);
    assert_eq!(effects[0].end.nanos, 450_000_000);
    assert_eq!(effects[0].x, 10.0);
    assert_eq!(effects[0].max_scale, 2.0);
}

#[test]
fn consecutive_clicks_do_not_leave_machine_stuck() {
    let builder = ClickEffectBuilder::new(ClickAnimationConfig::default());
    let effects = builder.build(&[
        click(0, ClickPhase::Down, 10.0, 20.0),
        click(40_000_000, ClickPhase::Up, 10.0, 20.0),
        click(90_000_000, ClickPhase::Down, 30.0, 40.0),
        click(130_000_000, ClickPhase::Up, 30.0, 40.0),
    ]);

    assert_eq!(effects.len(), 2);
    assert!(effects[0].end.nanos <= effects[1].start.nanos + 450_000_000);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::cursor_engine::tests::click -- --nocapture
```

Expected: FAIL because click animation types do not exist.

- [ ] **Step 3: Implement click animation**

Add to `src-tauri/src/media/cursor_engine.rs`:

```rust
const EXPAND_NANOS: u64 = 120_000_000;
const HOLD_NANOS: u64 = 80_000_000;
const SHRINK_NANOS: u64 = 300_000_000;

/// Click magnification animation durations and visual strength.
#[derive(Clone, Copy, Debug)]
pub struct ClickAnimationConfig {
    pub max_scale: f32,
    pub peak_opacity: f32,
}

impl Default for ClickAnimationConfig {
    fn default() -> Self {
        Self {
            max_scale: 2.0,
            peak_opacity: 0.35,
        }
    }
}

/// Runtime click animation state used for deterministic tests and effect construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClickAnimationState {
    Idle,
    PressedExpand,
    Hold,
    ReleaseShrink,
}

/// Small click state machine matching the Phase 4 required transition path.
pub struct ClickAnimationMachine {
    state: ClickAnimationState,
    config: ClickAnimationConfig,
    pressed_at: Option<MediaTimestamp>,
    release_started_at: Option<MediaTimestamp>,
}

impl ClickAnimationMachine {
    pub fn new(config: ClickAnimationConfig) -> Self {
        Self {
            state: ClickAnimationState::Idle,
            config,
            pressed_at: None,
            release_started_at: None,
        }
    }

    pub fn state(&self) -> ClickAnimationState {
        self.state
    }

    pub fn apply_click(&mut self, click: CursorClick) {
        match click.phase {
            ClickPhase::Down => {
                self.state = ClickAnimationState::PressedExpand;
                self.pressed_at = Some(click.timestamp);
                self.release_started_at = None;
            }
            ClickPhase::Up => {
                if self.state != ClickAnimationState::Idle {
                    self.state = ClickAnimationState::ReleaseShrink;
                    self.release_started_at = Some(click.timestamp);
                }
            }
        }
    }

    pub fn advance_to(&mut self, timestamp: MediaTimestamp) {
        match self.state {
            ClickAnimationState::PressedExpand => {
                if let Some(start) = self.pressed_at {
                    if timestamp.nanos.saturating_sub(start.nanos) >= EXPAND_NANOS {
                        self.state = ClickAnimationState::Hold;
                    }
                }
            }
            ClickAnimationState::ReleaseShrink => {
                if let Some(start) = self.release_started_at {
                    if timestamp.nanos.saturating_sub(start.nanos) >= SHRINK_NANOS {
                        self.state = ClickAnimationState::Idle;
                        self.pressed_at = None;
                        self.release_started_at = None;
                    }
                }
            }
            ClickAnimationState::Idle | ClickAnimationState::Hold => {}
        }
    }

    pub fn config(&self) -> ClickAnimationConfig {
        self.config
    }
}

/// Converts recorded click transitions into magnification effect windows.
pub struct ClickEffectBuilder {
    config: ClickAnimationConfig,
}

impl ClickEffectBuilder {
    pub fn new(config: ClickAnimationConfig) -> Self {
        Self { config }
    }

    pub fn build(&self, clicks: &[CursorClick]) -> Vec<CursorClickEffect> {
        let mut effects = Vec::new();
        let mut pending_down: Option<CursorClick> = None;

        for click in clicks.iter().copied().filter(|click| click.button == MouseButton::Left) {
            match click.phase {
                ClickPhase::Down => {
                    pending_down = Some(click);
                }
                ClickPhase::Up => {
                    if let Some(down) = pending_down.take() {
                        effects.push(CursorClickEffect {
                            start: down.timestamp,
                            end: MediaTimestamp::from_nanos(
                                click.timestamp
                                    .nanos
                                    .saturating_add(EXPAND_NANOS + HOLD_NANOS + SHRINK_NANOS),
                            ),
                            x: down.x,
                            y: down.y,
                            max_scale: self.config.max_scale,
                            peak_opacity: self.config.peak_opacity,
                        });
                    }
                }
            }
        }

        if let Some(down) = pending_down {
            effects.push(CursorClickEffect {
                start: down.timestamp,
                end: MediaTimestamp::from_nanos(
                    down.timestamp
                        .nanos
                        .saturating_add(EXPAND_NANOS + HOLD_NANOS + SHRINK_NANOS),
                ),
                x: down.x,
                y: down.y,
                max_scale: self.config.max_scale,
                peak_opacity: self.config.peak_opacity,
            });
        }

        effects
    }
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::cursor_engine -- --nocapture
```

Expected: PASS for smoother, interpolation, and click tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/cursor_engine.rs
git commit -m "feat(cursor): 实现点击放大状态机"
```

---

### Task 6: CursorEffectEngine Composition

**Files:**

- Modify: `src-tauri/src/media/cursor_engine.rs`
- Test: inline Rust tests in `src-tauri/src/media/cursor_engine.rs`

- [ ] **Step 1: Add failing engine tests**

Append inside `tests`:

```rust
#[test]
fn engine_builds_empty_timeline_for_empty_samples() {
    let engine = CursorEffectEngine::default();

    let timeline = engine.build_timeline(&[], &[], 30, 1_000_000_000).unwrap();

    assert_eq!(timeline.fps, 30);
    assert_eq!(timeline.duration_nanos, 1_000_000_000);
    assert!(timeline.frames.is_empty());
    assert!(timeline.click_effects.is_empty());
}

#[test]
fn engine_builds_frames_and_click_effects() {
    let engine = CursorEffectEngine::default();
    let samples = vec![
        sample(0, 10.0, 10.0),
        sample(33_333_333, 30.0, 20.0),
        sample(66_666_666, 80.0, 50.0),
    ];
    let clicks = vec![
        click(33_333_333, ClickPhase::Down, 30.0, 20.0),
        click(80_000_000, ClickPhase::Up, 30.0, 20.0),
    ];

    let timeline = engine
        .build_timeline(&samples, &clicks, 30, 100_000_000)
        .unwrap();

    assert_eq!(timeline.fps, 30);
    assert_eq!(timeline.frames.len(), 4);
    assert_eq!(timeline.click_effects.len(), 1);
}

#[test]
fn engine_rejects_zero_fps() {
    let engine = CursorEffectEngine::default();
    let error = engine
        .build_timeline(&[sample(0, 1.0, 1.0)], &[], 0, 100_000_000)
        .unwrap_err();

    assert!(error.to_string().contains("帧率"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::cursor_engine::tests::engine -- --nocapture
```

Expected: FAIL because `CursorEffectEngine` does not exist.

- [ ] **Step 3: Add error variant**

Modify `src-tauri/src/app/error.rs`:

```rust
pub enum AppError {
    InvalidState {
        current: &'static str,
        action: &'static str,
    },
    PermissionDenied {
        permission: &'static str,
    },
    NativeCaptureUnavailable {
        reason: &'static str,
    },
    CaptureFailed {
        reason: String,
    },
    AudioCaptureFailed {
        reason: String,
    },
    AudioDeviceNotFound {
        name: String,
    },
    AudioMixFailed {
        reason: String,
    },
    CursorProcessingFailed {
        reason: String,
    },
    CaptureStopTimeout {
        reason: String,
    },
    RecordingWriteFailed {
        reason: String,
    },
    RecordingFinalizeFailed {
        reason: String,
    },
}
```

Add the `Display` branch:

```rust
AppError::CursorProcessingFailed { reason } => {
    write!(formatter, "光标效果处理失败：{reason}")
}
```

- [ ] **Step 4: Implement engine**

Add below click builder in `src-tauri/src/media/cursor_engine.rs`:

```rust
/// Pure post-recording cursor effect engine.
pub struct CursorEffectEngine {
    click_config: ClickAnimationConfig,
    smoothing_enabled: bool,
}

impl CursorEffectEngine {
    pub fn new(click_config: ClickAnimationConfig) -> Self {
        Self {
            click_config,
            smoothing_enabled: true,
        }
    }

    pub fn with_smoothing(click_config: ClickAnimationConfig, smoothing_enabled: bool) -> Self {
        Self {
            click_config,
            smoothing_enabled,
        }
    }
}

impl Default for CursorEffectEngine {
    fn default() -> Self {
        Self::new(ClickAnimationConfig::default())
    }
}

impl CursorProcessor for CursorEffectEngine {
    fn build_timeline(
        &self,
        samples: &[CursorSample],
        clicks: &[CursorClick],
        fps: u32,
        duration_nanos: u64,
    ) -> AppResult<EffectTimeline> {
        if fps == 0 {
            return Err(AppError::CursorProcessingFailed {
                reason: "帧率必须大于 0".to_string(),
            });
        }

        if samples.is_empty() {
            return Ok(EffectTimeline {
                fps,
                duration_nanos,
                frames: Vec::new(),
                click_effects: Vec::new(),
            });
        }

        let smoothed = if self.smoothing_enabled {
            let smoother = CursorSmoother::new(SmoothingConfig::for_fps(fps));
            smoother.smooth(samples)
        } else {
            samples.to_vec()
        };
        let interpolator = BezierInterpolator::new(fps);
        let mut frames = interpolator.sample_frames(&smoothed, duration_nanos);
        let click_effects = ClickEffectBuilder::new(self.click_config).build(clicks);

        apply_click_scale_to_frames(&mut frames, &click_effects);

        Ok(EffectTimeline {
            fps,
            duration_nanos,
            frames,
            click_effects,
        })
    }
}

fn apply_click_scale_to_frames(frames: &mut [CursorFrame], effects: &[CursorClickEffect]) {
    for frame in frames {
        for effect in effects {
            if frame.timestamp.nanos < effect.start.nanos || frame.timestamp.nanos > effect.end.nanos
            {
                continue;
            }

            let span = effect.end.nanos.saturating_sub(effect.start.nanos).max(1);
            let t = (frame.timestamp.nanos.saturating_sub(effect.start.nanos) as f32 / span as f32)
                .clamp(0.0, 1.0);
            let wave = if t <= 0.35 {
                t / 0.35
            } else {
                1.0 - ((t - 0.35) / 0.65)
            }
            .clamp(0.0, 1.0);

            frame.scale = frame.scale.max(1.0 + (effect.max_scale - 1.0) * wave);
            frame.opacity = frame.opacity.max(effect.peak_opacity * wave);
        }
    }
}
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_engine -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml app::error -- --nocapture
```

Expected: PASS. Existing `permission_error_uses_chinese_message` remains passing.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/cursor_engine.rs src-tauri/src/app/error.rs
git commit -m "feat(cursor): 生成光标效果时间线"
```

---

### Task 7: Recording Metadata Sidecar Model

**Files:**

- Create: `src-tauri/src/media/recording_metadata.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Test: inline Rust tests in `src-tauri/src/media/recording_metadata.rs`

- [ ] **Step 1: Write failing metadata tests**

Create `src-tauri/src/media/recording_metadata.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::{ClickPhase, CursorClick, CursorSample, MouseButton};

    #[test]
    fn metadata_round_trips_json() {
        let metadata = RecordingMetadata {
            fps: 30,
            duration_nanos: 1_000_000_000,
            cursor_samples: vec![CursorSample {
                timestamp: MediaTimestamp::from_nanos(0),
                x: 10.0,
                y: 20.0,
            }],
            cursor_clicks: vec![CursorClick {
                timestamp: MediaTimestamp::from_nanos(10_000_000),
                button: MouseButton::Left,
                phase: ClickPhase::Down,
                x: 10.0,
                y: 20.0,
            }],
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: RecordingMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, metadata);
    }

    #[test]
    fn write_and_read_metadata_file() {
        let path = std::env::temp_dir().join("luzhi-metadata-test.json");
        let metadata = RecordingMetadata {
            fps: 60,
            duration_nanos: 50_000_000,
            cursor_samples: vec![],
            cursor_clicks: vec![],
        };

        RecordingMetadataWriter::write_metadata(&path, &metadata).unwrap();
        let parsed = RecordingMetadataWriter::read_metadata(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(parsed, metadata);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_metadata -- --nocapture
```

Expected: FAIL because `RecordingMetadata` and writer functions do not exist.

- [ ] **Step 3: Implement metadata sidecar**

Add above tests in `src-tauri/src/media/recording_metadata.rs`:

```rust
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app::error::{AppError, AppResult};
use crate::core::timeline::{CursorClick, CursorSample, EffectTimeline};

/// Recording sidecar metadata saved next to the intermediate recording artifact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingMetadata {
    pub fps: u32,
    pub duration_nanos: u64,
    pub cursor_samples: Vec<CursorSample>,
    pub cursor_clicks: Vec<CursorClick>,
}

/// JSON sidecar reader/writer for cursor metadata and effect timelines.
pub struct RecordingMetadataWriter;

impl RecordingMetadataWriter {
    pub fn write_metadata(path: &Path, metadata: &RecordingMetadata) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::RecordingWriteFailed {
                reason: error.to_string(),
            })?;
        }

        let json = serde_json::to_string_pretty(metadata).map_err(|error| {
            AppError::RecordingWriteFailed {
                reason: error.to_string(),
            }
        })?;

        fs::write(path, json).map_err(|error| AppError::RecordingWriteFailed {
            reason: error.to_string(),
        })
    }

    pub fn read_metadata(path: &Path) -> AppResult<RecordingMetadata> {
        let json = fs::read_to_string(path).map_err(|error| AppError::RecordingWriteFailed {
            reason: error.to_string(),
        })?;

        serde_json::from_str(&json).map_err(|error| AppError::RecordingWriteFailed {
            reason: error.to_string(),
        })
    }

    pub fn write_effect_timeline(path: &Path, timeline: &EffectTimeline) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::RecordingWriteFailed {
                reason: error.to_string(),
            })?;
        }

        let json = serde_json::to_string_pretty(timeline).map_err(|error| {
            AppError::RecordingWriteFailed {
                reason: error.to_string(),
            }
        })?;

        fs::write(path, json).map_err(|error| AppError::RecordingWriteFailed {
            reason: error.to_string(),
        })
    }
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_metadata -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/recording_metadata.rs src-tauri/src/media/mod.rs
git commit -m "feat(cursor): 写入录制光标元数据"
```

---

### Task 8: Cursor Metadata Recorder And Runtime

**Files:**

- Create: `src-tauri/src/app/cursor_metadata_runtime.rs`
- Modify: `src-tauri/src/app/mod.rs`
- Test: inline Rust tests in `src-tauri/src/app/cursor_metadata_runtime.rs`

- [ ] **Step 1: Write failing recorder tests**

Create `src-tauri/src/app/cursor_metadata_runtime.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::{ClickPhase, MouseButton};

    #[test]
    fn recorder_keeps_samples_in_order() {
        let mut recorder = CursorMetadataRecorder::new(30);

        recorder.record_snapshot(
            MediaTimestamp::from_nanos(0),
            CursorSnapshot {
                x: 1.0,
                y: 2.0,
                left_down: false,
                right_down: false,
                middle_down: false,
            },
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(33_333_333),
            CursorSnapshot {
                x: 3.0,
                y: 4.0,
                left_down: false,
                right_down: false,
                middle_down: false,
            },
        );

        let metadata = recorder.finish(66_666_666);

        assert_eq!(metadata.fps, 30);
        assert_eq!(metadata.cursor_samples.len(), 2);
        assert_eq!(metadata.cursor_samples[1].x, 3.0);
    }

    #[test]
    fn recorder_detects_left_click_down_and_up() {
        let mut recorder = CursorMetadataRecorder::new(60);

        recorder.record_snapshot(
            MediaTimestamp::from_nanos(0),
            CursorSnapshot {
                x: 10.0,
                y: 20.0,
                left_down: false,
                right_down: false,
                middle_down: false,
            },
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(16_666_666),
            CursorSnapshot {
                x: 11.0,
                y: 21.0,
                left_down: true,
                right_down: false,
                middle_down: false,
            },
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(33_333_333),
            CursorSnapshot {
                x: 12.0,
                y: 22.0,
                left_down: false,
                right_down: false,
                middle_down: false,
            },
        );

        let metadata = recorder.finish(50_000_000);

        assert_eq!(metadata.cursor_clicks.len(), 2);
        assert_eq!(metadata.cursor_clicks[0].button, MouseButton::Left);
        assert_eq!(metadata.cursor_clicks[0].phase, ClickPhase::Down);
        assert_eq!(metadata.cursor_clicks[1].phase, ClickPhase::Up);
    }

    #[test]
    fn recorder_caps_sample_count_to_bound_memory() {
        let mut recorder = CursorMetadataRecorder::with_max_samples(30, 3);

        for i in 0..10 {
            recorder.record_snapshot(
                MediaTimestamp::from_nanos(i * 33_333_333),
                CursorSnapshot {
                    x: i as f32,
                    y: i as f32,
                    left_down: false,
                    right_down: false,
                    middle_down: false,
                },
            );
        }

        let metadata = recorder.finish(333_333_333);

        assert_eq!(metadata.cursor_samples.len(), 3);
        assert_eq!(metadata.cursor_samples[0].x, 7.0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::cursor_metadata_runtime -- --nocapture
```

Expected: FAIL because recorder/runtime types do not exist.

- [ ] **Step 3: Implement recorder and runtime**

Add above tests in `src-tauri/src/app/cursor_metadata_runtime.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::app::error::AppResult;
use crate::core::clock::SessionClock;
use crate::core::frame::MediaTimestamp;
use crate::core::timeline::{ClickPhase, CursorClick, CursorSample, MouseButton};
use crate::media::recording_metadata::RecordingMetadata;

const DEFAULT_MAX_CURSOR_SAMPLES: usize = 120_000;

/// Snapshot of the current cursor position and button states.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorSnapshot {
    pub x: f32,
    pub y: f32,
    pub left_down: bool,
    pub right_down: bool,
    pub middle_down: bool,
}

/// Source that can poll the current cursor state.
pub trait CursorSnapshotSource: Send + 'static {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot>;
}

/// Pure recorder that converts snapshots into timeline metadata.
pub struct CursorMetadataRecorder {
    fps: u32,
    max_samples: usize,
    samples: Vec<CursorSample>,
    clicks: Vec<CursorClick>,
    previous_snapshot: Option<CursorSnapshot>,
}

impl CursorMetadataRecorder {
    pub fn new(fps: u32) -> Self {
        Self::with_max_samples(fps, DEFAULT_MAX_CURSOR_SAMPLES)
    }

    pub fn with_max_samples(fps: u32, max_samples: usize) -> Self {
        Self {
            fps,
            max_samples,
            samples: Vec::new(),
            clicks: Vec::new(),
            previous_snapshot: None,
        }
    }

    pub fn record_snapshot(&mut self, timestamp: MediaTimestamp, snapshot: CursorSnapshot) {
        if self.samples.len() >= self.max_samples {
            self.samples.remove(0);
        }

        self.samples.push(CursorSample {
            timestamp,
            x: snapshot.x,
            y: snapshot.y,
        });

        if let Some(previous) = self.previous_snapshot {
            self.record_button_transition(
                timestamp,
                snapshot,
                MouseButton::Left,
                previous.left_down,
                snapshot.left_down,
            );
            self.record_button_transition(
                timestamp,
                snapshot,
                MouseButton::Right,
                previous.right_down,
                snapshot.right_down,
            );
            self.record_button_transition(
                timestamp,
                snapshot,
                MouseButton::Middle,
                previous.middle_down,
                snapshot.middle_down,
            );
        }

        self.previous_snapshot = Some(snapshot);
    }

    fn record_button_transition(
        &mut self,
        timestamp: MediaTimestamp,
        snapshot: CursorSnapshot,
        button: MouseButton,
        was_down: bool,
        is_down: bool,
    ) {
        if was_down == is_down {
            return;
        }

        self.clicks.push(CursorClick {
            timestamp,
            button,
            phase: if is_down { ClickPhase::Down } else { ClickPhase::Up },
            x: snapshot.x,
            y: snapshot.y,
        });
    }

    pub fn finish(self, duration_nanos: u64) -> RecordingMetadata {
        RecordingMetadata {
            fps: self.fps,
            duration_nanos,
            cursor_samples: self.samples,
            cursor_clicks: self.clicks,
        }
    }
}

/// Background cursor metadata runtime. It polls at capture fps and stores only metadata.
pub struct CursorMetadataRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<RecordingMetadata>>,
}

impl CursorMetadataRuntime {
    pub fn spawn<S>(mut source: S, fps: u32, session_clock: Arc<SessionClock>) -> Self
    where
        S: CursorSnapshotSource,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let interval = Duration::from_nanos(1_000_000_000u64 / fps.max(1) as u64);

        let handle = thread::spawn(move || {
            let mut recorder = CursorMetadataRecorder::new(fps.max(1));
            while !thread_stop.load(Ordering::Relaxed) {
                if let Ok(snapshot) = source.snapshot() {
                    recorder.record_snapshot(
                        MediaTimestamp::from_nanos(session_clock.elapsed_nanos()),
                        snapshot,
                    );
                }
                thread::sleep(interval);
            }

            recorder.finish(session_clock.elapsed_nanos())
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) -> Option<RecordingMetadata> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .take()
            .and_then(|handle| handle.join().ok())
    }
}

impl Drop for CursorMetadataRuntime {
    fn drop(&mut self) {
        if self.handle.is_some() {
            let _ = self.stop();
        }
    }
}
```

Modify `src-tauri/src/app/mod.rs`:

```rust
pub mod cursor_metadata_runtime;
pub mod error;
pub mod events;
pub mod mic_level_runtime;
pub mod permission_service;
pub mod recording_runtime;
pub mod recording_service;
pub mod state_machine;
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::cursor_metadata_runtime -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/app/cursor_metadata_runtime.rs src-tauri/src/app/mod.rs
git commit -m "feat(cursor): 记录录制期光标元数据"
```

---

### Task 9: macOS CoreGraphics Cursor Source

**Files:**

- Create: `src-tauri/src/platform/macos/cursor_source.rs`
- Modify: `src-tauri/src/platform/macos/mod.rs`
- Test: compile-only through `cargo test`

- [ ] **Step 1: Create macOS cursor source**

Create `src-tauri/src/platform/macos/cursor_source.rs`:

```rust
use crate::app::cursor_metadata_runtime::{CursorSnapshot, CursorSnapshotSource};
use crate::app::error::{AppError, AppResult};

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[allow(non_camel_case_types)]
type CGEventRef = *const std::ffi::c_void;

#[allow(non_camel_case_types)]
type CFTypeRef = *const std::ffi::c_void;

const K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION_STATE: i32 = 0;
const K_CG_MOUSE_BUTTON_LEFT: u32 = 0;
const K_CG_MOUSE_BUTTON_RIGHT: u32 = 1;
const K_CG_MOUSE_BUTTON_CENTER: u32 = 2;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *const std::ffi::c_void) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
}

/// macOS cursor snapshot source backed by CoreGraphics.
pub struct MacCursorSource;

impl MacCursorSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacCursorSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorSnapshotSource for MacCursorSource {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot> {
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return Err(AppError::CursorProcessingFailed {
                    reason: "读取鼠标位置失败".to_string(),
                });
            }

            let point = CGEventGetLocation(event);
            CFRelease(event as CFTypeRef);

            Ok(CursorSnapshot {
                x: point.x as f32,
                y: point.y as f32,
                left_down: CGEventSourceButtonState(
                    K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION_STATE,
                    K_CG_MOUSE_BUTTON_LEFT,
                ),
                right_down: CGEventSourceButtonState(
                    K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION_STATE,
                    K_CG_MOUSE_BUTTON_RIGHT,
                ),
                middle_down: CGEventSourceButtonState(
                    K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION_STATE,
                    K_CG_MOUSE_BUTTON_CENTER,
                ),
            })
        }
    }
}
```

Modify `src-tauri/src/platform/macos/mod.rs`:

```rust
pub mod cpal_microphone;
pub mod cursor_source;
pub mod permissions;
pub mod screen_capture_kit;
```

- [ ] **Step 2: Run compile check**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml platform::macos::cursor_source -- --nocapture
```

Expected: PASS compile-only. No test should call real CoreGraphics cursor polling in CI.

- [ ] **Step 3: Native Safety Gate review note**

Add this review note at the top of `src-tauri/src/platform/macos/cursor_source.rs`:

```rust
// ⚠️ 人工审查检查点：
// - CGEventCreate returns a retained event that must be released with CFRelease.
// - CGEventSourceButtonState only reads button state; it must not post or synthesize input.
// - This source is polled from CursorMetadataRuntime, not from ScreenCaptureKit callbacks.
// - Polling failures return a structured Rust error and must not panic.
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/platform/macos/cursor_source.rs src-tauri/src/platform/macos/mod.rs
git commit -m "feat(cursor): 接入macOS光标采样源"
```

---

### Task 10: Recording Service Integration And Sidecar Paths

**Files:**

- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/media/recording_writer.rs`
- Modify: `src/lib/tauri.ts`
- Test: existing Rust tests plus new serialization test

- [ ] **Step 1: Extend RecordingResult test first**

Modify the `counting_writer_reports_pushed_media` assertion in `src-tauri/src/media/recording_writer.rs`:

```rust
assert_eq!(result.frame_count, 1);
assert_eq!(result.mixed_audio_chunk_count, 1);
assert_eq!(result.cursor_metadata_path, None);
assert_eq!(result.effect_timeline_path, None);
```

Add a new test:

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
    };

    let json = serde_json::to_string(&result).unwrap();

    assert!(json.contains("\"cursorMetadataPath\":\"/tmp/cursor.json\""));
    assert!(json.contains("\"effectTimelinePath\":\"/tmp/effects.json\""));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_writer -- --nocapture
```

Expected: FAIL because `RecordingResult` does not have sidecar path fields.

- [ ] **Step 3: Extend `RecordingResult` and counting writer**

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
}
```

Modify `CountingRecordingWriter::finish()`:

```rust
Ok(RecordingResult {
    duration_secs: 0,
    frame_count: self.frame_count,
    mixed_audio_chunk_count: self.audio_count,
    output_path: self
        .output_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string()),
    cursor_metadata_path: None,
    effect_timeline_path: None,
})
```

Update every `RecordingResult` literal in `src-tauri/src/platform/macos_service.rs` tests and empty fallback paths to include:

```rust
cursor_metadata_path: None,
effect_timeline_path: None,
```

- [ ] **Step 4: Integrate cursor runtime in macOS service**

Modify imports in `src-tauri/src/platform/macos_service.rs`:

```rust
use super::macos::cursor_source::MacCursorSource;
use crate::app::cursor_metadata_runtime::CursorMetadataRuntime;
use crate::core::clock::SessionClock;
use crate::media::recording_metadata::RecordingMetadataWriter;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
```

Add fields to `MacRecordingService`:

```rust
cursor_runtime: Option<CursorMetadataRuntime>,
last_cursor_metadata_path: Option<String>,
last_effect_timeline_path: Option<String>,
```

Initialize them in `new()`:

```rust
cursor_runtime: None,
last_cursor_metadata_path: None,
last_effect_timeline_path: None,
```

In `start()`, replace the existing session clock creation with a shared variable used by both microphone and cursor runtime:

```rust
let session_clock = Arc::new(SessionClock::new());
```

After microphone setup and before spawning the media consumer:

```rust
self.cursor_runtime = Some(CursorMetadataRuntime::spawn(
    MacCursorSource::new(),
    config.fps,
    session_clock.clone(),
));
```

In `stop()`, after joining the media consumer and before returning `Ok(result)`:

```rust
let cursor_metadata = self
    .cursor_runtime
    .as_mut()
    .and_then(|runtime| runtime.stop());
self.cursor_runtime = None;

let cursor_metadata_path = if let Some(metadata) = cursor_metadata {
    let path = cursor_metadata_path();
    RecordingMetadataWriter::write_metadata(&path, &metadata)?;
    Some(path.to_string_lossy().to_string())
} else {
    None
};

let mut result = result;
result.cursor_metadata_path = cursor_metadata_path.clone();
result.effect_timeline_path = self.last_effect_timeline_path.clone();
self.last_cursor_metadata_path = cursor_metadata_path;
```

Add helper at file bottom:

```rust
fn cursor_metadata_path() -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("cursor-metadata-{millis}.json"))
}
```

Add public accessors:

```rust
pub fn last_cursor_metadata_path(&self) -> Option<String> {
    self.last_cursor_metadata_path.clone()
}

pub fn set_last_effect_timeline_path(&mut self, path: Option<String>) {
    self.last_effect_timeline_path = path;
}
```

- [ ] **Step 5: Update frontend type**

Modify `src/lib/tauri.ts`:

```typescript
export type RecordingResult = {
  durationSecs: number
  frameCount: number
  mixedAudioChunkCount: number
  outputPath: string | null
  cursorMetadataPath: string | null
  effectTimelinePath: string | null
}
```

Update `src/App.tsx` stop-result mapping:

```typescript
setRecordingResult({
  durationSecs: result.durationSecs,
  frameCount: result.frameCount,
  mixedAudioChunkCount: result.mixedAudioChunkCount,
  outputPath: result.outputPath ?? null,
  cursorMetadataPath: result.cursorMetadataPath ?? null,
  effectTimelinePath: result.effectTimelinePath ?? null,
})
```

Update every mocked `stop_recording` result in `src/App.test.tsx` to include:

```typescript
cursorMetadataPath: '/tmp/cursor.json',
effectTimelinePath: null,
```

Use `null` for tests that do not need metadata.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_writer -- --nocapture
npm test -- --run
```

Expected: Rust recording writer tests pass; frontend tests pass after mock updates.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs src-tauri/src/media/recording_writer.rs src/lib/tauri.ts src/App.tsx src/App.test.tsx
git commit -m "feat(cursor): 保存光标元数据边车文件"
```

---

### Task 11: Beautify Config And Timeline Commands

**Files:**

- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app/events.rs`
- Modify: `src/lib/tauri.ts`
- Test: Rust serde tests and frontend invoke tests

- [ ] **Step 1: Add Rust payload serde tests**

Add to `src-tauri/src/app/events.rs` tests:

```rust
#[test]
fn cursor_effect_summary_serializes_camel_case() {
    let payload = CursorEffectSummaryPayload {
        frame_count: 10,
        click_effect_count: 2,
        effect_timeline_path: "/tmp/effects.json".to_string(),
    };

    let json = serde_json::to_string(&payload).unwrap();

    assert!(json.contains("\"frameCount\":10"));
    assert!(json.contains("\"clickEffectCount\":2"));
    assert!(json.contains("\"effectTimelinePath\":\"/tmp/effects.json\""));
}

#[test]
fn post_process_progress_serializes_camel_case() {
    let payload = PostProcessProgressPayload {
        stage: "cursor",
        progress: 100,
    };

    let json = serde_json::to_string(&payload).unwrap();

    assert!(json.contains("\"stage\":\"cursor\""));
    assert!(json.contains("\"progress\":100"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::events -- --nocapture
```

Expected: FAIL because payload structs do not exist.

- [ ] **Step 3: Add event payloads**

Add to `src-tauri/src/app/events.rs`:

```rust
/// Summary returned after building a cursor effect timeline.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorEffectSummaryPayload {
    pub frame_count: usize,
    pub click_effect_count: usize,
    pub effect_timeline_path: String,
}

/// Lightweight post-processing progress payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostProcessProgressPayload {
    pub stage: &'static str,
    pub progress: u8,
}
```

- [ ] **Step 4: Add AppState config and commands**

Modify imports in `src-tauri/src/lib.rs`:

```rust
use app::events::{
    CursorEffectSummaryPayload, MicLevelPayload, PermissionPayload, PostProcessProgressPayload,
    RecordingStatusPayload,
};
use core::processor::CursorProcessor;
use media::cursor_engine::{ClickAnimationConfig, CursorEffectEngine};
use media::recording_metadata::RecordingMetadataWriter;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
```

Add config type:

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BeautifyConfigPayload {
    cursor_magnification: bool,
    magnification_factor: f32,
    cursor_smoothing: bool,
    auto_trim_silences: bool,
    trim_sensitivity: String,
}

impl Default for BeautifyConfigPayload {
    fn default() -> Self {
        Self {
            cursor_magnification: true,
            magnification_factor: 2.0,
            cursor_smoothing: true,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
        }
    }
}
```

Add field to `AppState`:

```rust
beautify_config: Arc<Mutex<BeautifyConfigPayload>>,
```

Initialize:

```rust
beautify_config: Arc::new(Mutex::new(BeautifyConfigPayload::default())),
```

Add commands:

```rust
#[tauri::command]
fn set_beautify_config(
    state: tauri::State<'_, AppState>,
    config: BeautifyConfigPayload,
) -> Result<(), String> {
    let mut guard = state
        .beautify_config
        .lock()
        .map_err(|_| "美化配置锁已损坏".to_string())?;
    *guard = config;
    Ok(())
}

#[tauri::command]
fn build_cursor_effect_timeline(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<CursorEffectSummaryPayload, String> {
    let metadata_path = {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        service
            .last_cursor_metadata_path()
            .ok_or_else(|| "没有可用的光标元数据，请先完成一次录制".to_string())?
    };

    let _ = app.emit(
        "post-process-progress",
        PostProcessProgressPayload {
            stage: "cursor",
            progress: 0,
        },
    );

    let metadata = RecordingMetadataWriter::read_metadata(PathBuf::from(&metadata_path).as_path())
        .map_err(|error| error.to_string())?;
    let config = state
        .beautify_config
        .lock()
        .map_err(|_| "美化配置锁已损坏".to_string())?
        .clone();

    let engine = CursorEffectEngine::with_smoothing(
        ClickAnimationConfig {
            max_scale: if config.cursor_magnification {
                config.magnification_factor.clamp(1.0, 3.0)
            } else {
                1.0
            },
            peak_opacity: if config.cursor_magnification { 0.35 } else { 0.0 },
        },
        config.cursor_smoothing,
    );

    let clicks = if config.cursor_magnification {
        metadata.cursor_clicks.as_slice()
    } else {
        &[]
    };

    let timeline = engine
        .build_timeline(
            &metadata.cursor_samples,
            clicks,
            metadata.fps,
            metadata.duration_nanos,
        )
        .map_err(|error| error.to_string())?;

    let path = effect_timeline_path();
    RecordingMetadataWriter::write_effect_timeline(&path, &timeline)
        .map_err(|error| error.to_string())?;

    {
        let mut service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        service.set_last_effect_timeline_path(Some(path.to_string_lossy().to_string()));
    }

    let _ = app.emit(
        "post-process-progress",
        PostProcessProgressPayload {
            stage: "cursor",
            progress: 100,
        },
    );

    Ok(CursorEffectSummaryPayload {
        frame_count: timeline.frames.len(),
        click_effect_count: timeline.click_effects.len(),
        effect_timeline_path: path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
fn export_video(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    preset: String,
) -> Result<CursorEffectSummaryPayload, String> {
    if !matches!(preset.as_str(), "bilibili" | "douyin" | "xiaohongshu") {
        return Err(format!("未知导出预设：{preset}"));
    }

    build_cursor_effect_timeline(app, state)
}

fn effect_timeline_path() -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("cursor-effects-{millis}.json"))
}
```

Register commands in `invoke_handler`:

```rust
set_beautify_config,
build_cursor_effect_timeline,
export_video
```

- [ ] **Step 5: Update TypeScript command wrappers**

Modify `src/lib/tauri.ts`:

```typescript
export type CursorEffectSummary = {
  frameCount: number
  clickEffectCount: number
  effectTimelinePath: string
}

export async function setBeautifyConfig(config: BeautifyConfig): Promise<void> {
  return invoke('set_beautify_config', { config })
}

export async function buildCursorEffectTimeline(): Promise<CursorEffectSummary> {
  return invoke<CursorEffectSummary>('build_cursor_effect_timeline')
}

export async function exportVideo(preset: ExportPreset): Promise<CursorEffectSummary> {
  return invoke<CursorEffectSummary>('export_video', { preset })
}
```

- [ ] **Step 6: Run focused tests/build**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::events -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml cursor_engine -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml recording_metadata -- --nocapture
npm run build
```

Expected: Rust tests pass and TypeScript build succeeds.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/app/events.rs src/lib/tauri.ts
git commit -m "feat(cursor): 接入光标效果时间线命令"
```

---

### Task 12: Preview UI Command Wiring

**Files:**

- Modify: `src/components/preview-view.tsx`
- Modify: `src/App.test.tsx`
- Test: Vitest

- [ ] **Step 1: Add failing frontend tests**

Add to `src/App.test.tsx`:

```typescript
it('sends beautify config when cursor smoothing is toggled in preview', async () => {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'set_beautify_config') return Promise.resolve()
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)

  await screen.findByText('预览与美化')
  invokeMock.mockClear()

  const switches = screen.getAllByRole('switch')
  fireEvent.click(switches[1])

  await vi.waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('set_beautify_config', {
      config: {
        cursorMagnification: true,
        magnificationFactor: 2,
        cursorSmoothing: false,
        autoTrimSilences: false,
        trimSensitivity: 'medium',
      },
    })
  })
})

it('calls export_video from preview export button', async () => {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'export_video') {
      return Promise.resolve({
        frameCount: 30,
        clickEffectCount: 1,
        effectTimelinePath: '/tmp/effects.json',
      })
    }
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)

  await screen.findByText('预览与美化')
  invokeMock.mockClear()

  const exportButtons = screen.getAllByText('导出')
  fireEvent.click(exportButtons[0])

  await vi.waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('export_video', { preset: 'bilibili' })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm test -- --run src/App.test.tsx
```

Expected: FAIL because `PreviewView` still logs to console and does not call Tauri wrappers.

- [ ] **Step 3: Wire PreviewView to Tauri wrappers**

Modify imports in `src/components/preview-view.tsx`:

```typescript
import {
  buildCursorEffectTimeline,
  exportVideo,
  setBeautifyConfig,
  type BeautifyConfig,
  type ExportPreset,
  type RecordingResult,
} from '@/lib/tauri'
```

Replace `handleBeautifyChange` with:

```typescript
const currentBeautifyConfig = (patch: Partial<BeautifyConfig>): BeautifyConfig => ({
  cursorMagnification,
  magnificationFactor: magnificationFactor[0],
  cursorSmoothing,
  autoTrimSilences,
  trimSensitivity,
  ...patch,
})

const handleBeautifyChange = (config: Partial<BeautifyConfig>) => {
  void setBeautifyConfig(currentBeautifyConfig(config))
    .then(() => buildCursorEffectTimeline())
    .catch((error) => {
      console.error('光标效果处理失败', error)
    })
}
```

Replace `handleExport` with:

```typescript
const handleExport = (preset: ExportPreset) => {
  void exportVideo(preset).catch((error) => {
    console.error('导出失败', error)
  })
}
```

Keep existing UI text Chinese and do not add new in-app explanatory paragraphs.

- [ ] **Step 4: Run focused frontend tests**

Run:

```bash
npm test -- --run src/App.test.tsx
npm run build
```

Expected: PASS for all frontend tests and TypeScript build.

- [ ] **Step 5: Commit**

```bash
git add src/components/preview-view.tsx src/App.test.tsx
git commit -m "feat(cursor): 连接预览页光标美化命令"
```

---

### Task 13: Cursor Visibility Policy For Raw Capture

**Files:**

- Modify: `src-tauri/src/core/config.rs`
- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: Rust config tests

- [ ] **Step 1: Add failing config test**

Add to `src-tauri/src/core/config.rs` tests:

```rust
#[test]
fn default_config_records_system_cursor_until_effect_pipeline_is_enabled() {
    let config = CaptureConfig::full_screen_1080p_30fps();

    assert!(config.show_system_cursor);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::config::tests::default_config_records_system_cursor_until_effect_pipeline_is_enabled -- --nocapture
```

Expected: FAIL because `CaptureConfig` has no `show_system_cursor` field.

- [ ] **Step 3: Add config field and SCK wiring**

Modify `CaptureConfig`:

```rust
pub struct CaptureConfig {
    pub mode: CaptureMode,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub show_system_cursor: bool,
}
```

Modify default:

```rust
show_system_cursor: true,
```

Modify `set_capture_mode` config construction in `src-tauri/src/lib.rs`:

```rust
let beautify_config = state
    .beautify_config
    .lock()
    .map_err(|_| "美化配置锁已损坏".to_string())?
    .clone();
let show_system_cursor = !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);

*config = CaptureConfig {
    mode,
    width: payload.width.unwrap_or(1920),
    height: payload.height.unwrap_or(1080),
    fps: payload.fps.unwrap_or(30),
    show_system_cursor,
};
```

Modify every test literal of `CaptureConfig` to include `show_system_cursor: true`.

Modify `src-tauri/src/platform/macos/screen_capture_kit.rs`:

```rust
stream_config.setShowsCursor(config.show_system_cursor);
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::config -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml macos_service -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Manual double-cursor check**

Run after implementation:

```bash
npm run tauri dev
```

Expected manual result:

- With cursor beautify enabled before recording, raw SCK frames should not include the system cursor, and the generated timeline should be available for post-processing.
- With cursor beautify disabled before recording, raw SCK frames keep the system cursor for compatibility.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/config.rs src-tauri/src/platform/macos/screen_capture_kit.rs src-tauri/src/lib.rs
git commit -m "feat(cursor): 控制原始录制光标可见性"
```

---

### Task 14: Full Verification Matrix

**Files:**

- No code changes
- Verification outputs copied into checklist/HANDOFF in Task 15

- [ ] **Step 1: Rust format**

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

Expected: PASS.

- [ ] **Step 2: Rust tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS. The final count should be greater than the current 76 tests because Phase 4 adds cursor timeline, algorithm, metadata, and serde tests.

- [ ] **Step 3: Rust clippy**

Run:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected: no errors. Existing FFI naming warnings may remain if they are the same pre-existing class of warnings documented in `HANDOFF.md`.

- [ ] **Step 4: Rust build**

Run:

```bash
cargo build --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 5: Frontend build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 6: Frontend tests**

Run:

```bash
npm test -- --run
```

Expected: PASS. The final count should be greater than the current 23 tests because Phase 4 adds preview command wiring tests.

- [ ] **Step 7: Manual app verification**

Run:

```bash
npm run tauri dev
```

Manual expected results:

- Start/stop 1080p full-screen recording.
- `RecordingResult.cursorMetadataPath` is non-null after stopping.
- The metadata JSON contains `cursorSamples` and `cursorClicks` arrays.
- Toggling “光标平滑” or “光标放大” in preview calls `set_beautify_config` and builds a cursor effect timeline.
- Clicking an export preset calls `export_video` and writes an effect timeline JSON.
- Fast start/stop still does not deadlock or lose the stop entry point.
- No cursor metadata is sent to React as a stream.
- No video or audio frames are sent to React.

---

### Task 15: Checklist, HANDOFF, And Review Notes

**Files:**

- Modify: `tests/phase-4-w7-w8-checklist.md`
- Modify: `HANDOFF.md`
- Optional create: `docs/superpowers/reviews/2026-05-27-phase-4-native-safety-notes.md`

- [ ] **Step 1: Update checklist status**

Update `tests/phase-4-w7-w8-checklist.md` with this header:

```markdown
> 最后更新：2026-05-27 | 自动化验证待执行，Native Safety Gate 待人工审查
```

Mark automatic items `[x]` only when their verification command has passed. Keep manual video-rendering items unchecked if the production FFmpeg compositor is still absent, and annotate them:

```markdown
- [ ] 导出视频包含光标平滑效果。（需 Phase 6 FFmpeg compositor 接入后做人工视频检查；Phase 4 自动化已验证 EffectTimeline 生成）
- [ ] 导出视频包含点击放大效果。（需 Phase 6 FFmpeg compositor 接入后做人工视频检查；Phase 4 自动化已验证 CursorClickEffect 生成）
```

- [ ] **Step 2: Update HANDOFF**

Add a new top record under “工作任务记录” using this structure:

```markdown
### 2026-05-27：Phase 4 光标平滑与点击放大计划/实现状态

输入文件：

- `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md`
- `tests/phase-4-w7-w8-checklist.md`

本轮完成：

1. `core/timeline.rs` 建立 `CursorSample` / `CursorClick` / `EffectTimeline` serde 模型。
2. `media/cursor_engine.rs` 实现移动平均、贝塞尔插值、点击放大状态机。
3. `app/cursor_metadata_runtime.rs` 和 `platform/macos/cursor_source.rs` 建立录制期光标元数据采集。
4. `MacRecordingService` 停止录制后写入光标元数据边车文件。
5. Tauri 命令和预览页接入光标美化配置与时间线生成。

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`：记录实际通过/失败状态
- `cargo test --manifest-path src-tauri/Cargo.toml`：记录实际测试数量和通过/失败状态
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`：记录实际 error/warning 状态
- `cargo build --manifest-path src-tauri/Cargo.toml`：记录实际通过/失败状态
- `npm run build`：记录实际通过/失败状态
- `npm test -- --run`：记录实际测试数量和通过/失败状态

剩余待完成：

- `npm run tauri dev` 手动验证光标元数据、点击采集、时间线 JSON 和双光标策略。
- `platform/macos/cursor_source.rs` CoreGraphics FFI Native Safety Gate 人工逐行审查。
- Phase 6 接入生产 FFmpeg compositor 后做真实视频光标效果人工验收。
```

When writing the final `HANDOFF.md`, copy the concrete command outcomes from Task 14 into these lines. Keep only the most recent 7 work records, preserving the existing document structure.

- [ ] **Step 3: Add optional safety notes**

If the native FFI review is performed in this session, create `docs/superpowers/reviews/2026-05-27-phase-4-native-safety-notes.md`:

```markdown
# Phase 4 Native Safety Notes

## Reviewed Files

- `src-tauri/src/platform/macos/cursor_source.rs`
- `src-tauri/src/platform/macos/screen_capture_kit.rs`

## Checks

- `CGEventCreate` result is null-checked.
- `CGEventCreate` retained event is released with `CFRelease`.
- Cursor polling reads state only and does not synthesize input.
- Cursor polling runs in `CursorMetadataRuntime`, separate from ScreenCaptureKit callbacks.
- `SCStreamConfiguration::showsCursor` is controlled through `CaptureConfig`.

## Remaining Manual Checks

- Verify app permission prompts on a clean macOS user account.
- Verify no double cursor appears when cursor beautify is enabled.
```

- [ ] **Step 4: Commit docs**

```bash
git add tests/phase-4-w7-w8-checklist.md HANDOFF.md docs/superpowers/reviews/2026-05-27-phase-4-native-safety-notes.md
git commit -m "docs(cursor): 更新Phase 4验证记录"
```

If the optional safety notes file was not created, omit it from `git add`.

---

## 3. Self-Review

Spec coverage:

- Metadata capture: Tasks 8, 9, 10.
- Shared time base: Task 8 uses `SessionClock`; Task 10 shares the same clock setup point as microphone capture.
- Metadata sidecar: Tasks 7 and 10.
- Moving average smoothing: Task 3.
- Adaptive 30fps/60fps window: Task 3.
- Fast jump preservation: Task 3.
- Bezier interpolation per video timestamp: Task 4.
- Click magnification state machine: Task 5.
- `EffectTimeline` serde: Tasks 1, 6, 7, 11.
- Post-recording timeline generation: Tasks 6, 7, 11.
- UI command wiring without media streams: Task 12.
- Performance guardrails and manual gates: Tasks 8, 9, 14, 15.
- BUG.md prevention rules: Manual gates and Task 12.

Known remaining gap:

- Real MP4 pixel compositing is not implemented in this Phase because the repository currently has only a counting writer and the production FFmpeg export path is Phase 6. Phase 4 still validates the full cursor timeline contract and export command boundary so Phase 6 can consume it directly.

Placeholder scan:

- No undefined file paths are referenced as implementation targets.
- No new dependency versions are requested.
- All code-facing steps include concrete snippets, commands, and expected outcomes.

Type consistency:

- `CursorSample`, `CursorClick`, `CursorFrame`, `CursorClickEffect`, and `EffectTimeline` are defined in Task 1 and reused consistently.
- `CursorProcessor::build_timeline()` signature in Task 2 matches `CursorEffectEngine` implementation in Task 6.
- `RecordingResult.cursorMetadataPath` and `effectTimelinePath` use serde camelCase and matching TypeScript fields.
- `set_beautify_config`, `build_cursor_effect_timeline`, and `export_video` command names match `src/lib/tauri.ts`.

---

## 4. Execution Notes

Recommended execution mode: subagent-driven development, one task per agent, with review after each task. The highest-risk review points are Task 9 and Task 13 because they touch macOS FFI and raw capture cursor visibility.

Do not run `git push` or merge. Commit locally only after each task passes its focused verification.
