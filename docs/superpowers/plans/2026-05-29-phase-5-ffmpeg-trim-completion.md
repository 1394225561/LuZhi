# Phase 5 FFmpeg Trim Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete Phase 5 from “cut timeline and export boundary contract” to “real playable trimmed export”, and make post-recording trim sensitivity fully re-buildable.

**Architecture:** Keep recording and post-processing in Rust; React only sends config/export commands and receives summaries/paths. Recording preserves an original media artifact, while export produces a separate playable file by consuming `CutTimeline` through a structured FFmpeg binding boundary, never by building FFmpeg CLI strings. Trim metadata stores sensitivity-independent base activity, and `build_cut_timeline()` derives the user-selected sensitivity at post-process time.

**Tech Stack:** Tauri 2, Rust 2021, `ffmpeg-next` optional feature, ScreenCaptureKit/cpal capture, React/TypeScript Preview UI, serde JSON sidecars.

---

## Product Decisions

### Decision 1: FFmpeg Gate Completion

Adopt this product behavior:

- A completed recording must preserve the original recording artifact.
- Export must create a new playable output file and return `outputPath: Some(...)`.
- If auto-trim is enabled, export consumes `CutTimeline` and removes cut ranges.
- If auto-trim is disabled, export still creates a playable output using the full source recording.
- Do not return a fake path before a real file exists.
- Do not delete or overwrite the original recording artifact during export.
- FFmpeg integration must use binding/C API through Rust, not CLI strings.

Rationale:

- This matches `docs/architecture/project-architecture-and-overall-planning.md` Phase 5 delivery: “原始素材保留，可重新导出” and “通过 FFmpeg 封装执行裁剪导出”.
- It keeps the current safety boundary from Phase 5: frontend does not receive media frames or activity streams.

### Decision 2: Trim Sensitivity Semantics

Adopt this product behavior:

- Preview sensitivity is a post-recording tuning control.
- Changing sensitivity after recording must rebuild the cut timeline from the same original metadata.
- Low/Medium/High changes all detection semantics, including RMS window, thresholds, candidate duration, and buffer.
- Users should not need to re-record just to make sensitivity changes fully take effect.

Rationale:

- This is the most intuitive preview workflow: record once, tune, export.
- It avoids hidden “RMS window was locked at recording start” behavior.

Implementation consequence:

- Recording metadata must stop storing only already-windowed RMS samples tied to one sensitivity.
- It should store a fixed base window, for example 100ms RMS buckets.
- `build_cut_timeline()` re-aggregates those base buckets into 500ms/750ms/1000ms windows based on the current sensitivity.

---

## File Structure

Modify:

- `src-tauri/src/core/cut.rs`
  - Add metadata schema/version fields needed to distinguish base RMS buckets from derived RMS windows.
- `src-tauri/src/media/silence_detector.rs`
  - Add base RMS bucket aggregation.
  - Keep `SilenceDetectorEngine` pure and deterministic.
- `src-tauri/src/media/trim_metadata.rs`
  - Read/write versioned trim metadata.
  - Keep compatibility for existing `audioActivity` sidecars during migration.
- `src-tauri/src/platform/macos_service.rs`
  - Record base RMS buckets in the consumer thread.
  - Continue keeping visual diff low-frequency and bounded.
  - Switch default writer from `CountingRecordingWriter` to production writer only after the FFmpeg writer passes tests and feature checks.
- `src-tauri/src/media/ffmpeg_writer.rs`
  - Replace skeleton writer with real original recording writer.
  - Ensure writer errors propagate through `stop()` instead of being swallowed.
- `src-tauri/src/media/trim_exporter.rs`
  - Replace gated stub with real structured FFmpeg trim exporter under the `ffmpeg` feature.
  - Keep mock exporter tests independent from FFmpeg availability.
- `src-tauri/src/lib.rs`
  - Make `export_video()` call the real exporter when source recording exists.
  - Return `outputPath: Some(path)` only after verifying the file exists and is non-empty.
- `src-tauri/src/app/events.rs`
  - Preserve the existing export summary shape unless a later product decision explicitly adds fields; avoid exposing internal activity streams.
- `src/lib/tauri.ts`
  - Mirror any payload shape changes.
- `src/components/preview-view.tsx`
  - Show a playable export path when present.
  - Preserve current FFmpeg Gate copy only when no real exporter is available.
- `src/App.test.tsx`
  - Add Preview/export behavior tests.
- `tests/phase-5-w9-w10-checklist.md`
  - Move FFmpeg Gate items from unchecked to checked only after real manual verification.
- `HANDOFF.md`
  - Update Phase 5 status only after automated verification and manual FFmpeg checks.

Create:

- `src-tauri/src/media/trim_audio_activity.rs`
  - Small focused module for base RMS buckets and re-aggregation.
- `src-tauri/src/media/export_paths.rs`
  - Deterministic source/export path helpers with tests.
- `src-tauri/tests/ffmpeg_trim_export.rs`
  - Feature-gated integration tests for real FFmpeg export.

---

## Task 1: Lock the Product Contract in Tests

**Files:**

- Modify: `src-tauri/src/app/events.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/App.test.tsx`
- Modify: `tests/phase-5-w9-w10-checklist.md`

- [ ] **Step 1: Add Rust event serialization coverage for real export output**

Add a test in `src-tauri/src/app/events.rs` that preserves current camelCase shape and asserts a real output path is representable:

```rust
#[test]
fn export_summary_serializes_playable_output_path() {
    let payload = ExportSummaryPayload {
        frame_count: 10,
        click_effect_count: 1,
        effect_timeline_path: "/tmp/effects.json".to_string(),
        cut_count: 2,
        total_cut_nanos: 3_000_000_000,
        cut_timeline_path: Some("/tmp/cuts.json".to_string()),
        output_path: Some("/tmp/luzhi-export.mp4".to_string()),
    };

    let json = serde_json::to_string(&payload).unwrap();

    assert!(json.contains("\"outputPath\":\"/tmp/luzhi-export.mp4\""));
    assert!(json.contains("\"cutTimelinePath\":\"/tmp/cuts.json\""));
}
```

- [ ] **Step 2: Run the focused Rust test**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_summary_serializes_playable_output_path
```

Expected: PASS.

- [ ] **Step 3: Add frontend contract test for playable export summary**

Add or update a test in `src/App.test.tsx` so a non-null `outputPath` is not shown as an FFmpeg Gate:

```tsx
it('shows playable export result when export returns outputPath', async () => {
  vi.mocked(tauri.exportVideo).mockResolvedValueOnce({
    frameCount: 120,
    clickEffectCount: 0,
    effectTimelinePath: '/tmp/effects.json',
    cutCount: 1,
    totalCutNanos: 2_000_000_000,
    cutTimelinePath: '/tmp/cuts.json',
    outputPath: '/tmp/export.mp4',
  })

  render(<PreviewView onBack={vi.fn()} recordingResult={recordingResultWithoutOutput} />)

  await userEvent.click(screen.getByRole('button', { name: '导出' }))

  expect(await screen.findByText(/已生成裁剪时间线：1 段/)).toBeInTheDocument()
  expect(screen.queryByText(/FFmpeg 编码器接入后将生成可播放文件/)).not.toBeInTheDocument()
})
```

If this repository's tests use a different render helper, keep the assertion shape and adapt only the test harness.

- [ ] **Step 4: Run the focused frontend test**

Run:

```bash
npm test -- --run src/App.test.tsx
```

Expected: PASS after the test harness is aligned with existing patterns.

- [ ] **Step 5: Update checklist wording without marking FFmpeg done**

In `tests/phase-5-w9-w10-checklist.md`, keep these unchecked until Task 8 manual checks pass:

```markdown
- [ ] FFmpeg 封装能消费 `CutTimeline`。（FFmpeg Gate：需生产编码器接入并完成手动验收）
- [ ] 裁剪后视频可播放。（FFmpeg Gate）
- [ ] 音视频同步未明显漂移。（FFmpeg Gate）
```

Commit:

```bash
git add src-tauri/src/app/events.rs src/App.test.tsx tests/phase-5-w9-w10-checklist.md
git commit -m "test: lock phase 5 export product contract"
```

---

## Task 2: Add Sensitivity-Independent Audio Activity

**Files:**

- Create: `src-tauri/src/media/trim_audio_activity.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/core/cut.rs`
- Modify: `src-tauri/src/media/silence_detector.rs`
- Modify: `src-tauri/src/media/trim_metadata.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`

- [ ] **Step 1: Create base RMS bucket model**

Create `src-tauri/src/media/trim_audio_activity.rs`:

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

pub struct BaseAudioActivityAnalyzer {
    bucket_start_nanos: u64,
    last_sample_end_nanos: u64,
    sample_rate: u32,
    channels: u16,
    sum_squares: f64,
    sample_count: u64,
    initialized: bool,
}

impl Default for BaseAudioActivityAnalyzer {
    fn default() -> Self {
        Self {
            bucket_start_nanos: 0,
            last_sample_end_nanos: 0,
            sample_rate: 0,
            channels: 0,
            sum_squares: 0.0,
            sample_count: 0,
            initialized: false,
        }
    }
}

impl BaseAudioActivityAnalyzer {
    pub fn push_chunk(&mut self, chunk: &MixedAudioChunk) -> Vec<BaseAudioActivitySample> {
        if chunk.sample_rate == 0 || chunk.channels == 0 || chunk.samples.is_empty() {
            return Vec::new();
        }

        if !self.initialized {
            self.initialized = true;
            self.bucket_start_nanos = chunk.timestamp.nanos;
            self.last_sample_end_nanos = chunk.timestamp.nanos;
            self.sample_rate = chunk.sample_rate;
            self.channels = chunk.channels;
        }

        if chunk.sample_rate != self.sample_rate || chunk.channels != self.channels {
            let mut emitted = Vec::new();
            if let Some(sample) = self.flush_current_bucket() {
                emitted.push(sample);
            }
            self.bucket_start_nanos = chunk.timestamp.nanos;
            self.last_sample_end_nanos = chunk.timestamp.nanos;
            self.sample_rate = chunk.sample_rate;
            self.channels = chunk.channels;
            self.sum_squares = 0.0;
            self.sample_count = 0;
            emitted.extend(self.push_chunk_after_format_reset(chunk));
            return emitted;
        }

        let mut emitted = Vec::new();
        let expected_next = self.last_sample_end_nanos;
        let forward_gap = chunk.timestamp.nanos.saturating_sub(expected_next);
        if self.sample_count > 0 && forward_gap > BASE_RMS_BUCKET_NANOS {
            if let Some(sample) = self.flush_current_bucket() {
                emitted.push(sample);
            }
            self.bucket_start_nanos = chunk.timestamp.nanos;
        }
        if chunk.timestamp.nanos.saturating_add(BASE_RMS_BUCKET_NANOS) < self.last_sample_end_nanos {
            return emitted;
        }

        emitted.extend(self.push_chunk_after_format_reset(chunk));
        emitted
    }

    fn push_chunk_after_format_reset(
        &mut self,
        chunk: &MixedAudioChunk,
    ) -> Vec<BaseAudioActivitySample> {
        let mut emitted = Vec::new();
        let frames = chunk.samples.len() as u64 / chunk.channels as u64;
        if frames == 0 {
            return emitted;
        }

        let nanos_per_frame = 1_000_000_000f64 / chunk.sample_rate as f64;
        for (frame_index, frame) in chunk.samples.chunks(chunk.channels as usize).enumerate() {
            let frame_nanos =
                chunk.timestamp.nanos + (frame_index as f64 * nanos_per_frame).round() as u64;

            while frame_nanos >= self.bucket_start_nanos.saturating_add(BASE_RMS_BUCKET_NANOS) {
                if let Some(sample) = self.flush_current_bucket() {
                    emitted.push(sample);
                }
                self.bucket_start_nanos =
                    self.bucket_start_nanos.saturating_add(BASE_RMS_BUCKET_NANOS);
            }

            for sample in frame {
                let value = *sample as f64;
                self.sum_squares += value * value;
                self.sample_count += 1;
            }
            self.last_sample_end_nanos =
                frame_nanos.saturating_add(nanos_per_frame.round() as u64);
        }

        emitted
    }

    pub fn flush(&mut self) -> Option<BaseAudioActivitySample> {
        self.flush_current_bucket()
    }

    fn flush_current_bucket(&mut self) -> Option<BaseAudioActivitySample> {
        if self.sample_count == 0 {
            return None;
        }
        let sample = BaseAudioActivitySample {
            start: MediaTimestamp::from_nanos(self.bucket_start_nanos),
            end: MediaTimestamp::from_nanos(self.last_sample_end_nanos),
            sum_squares: self.sum_squares,
            sample_count: self.sample_count,
        };
        self.sum_squares = 0.0;
        self.sample_count = 0;
        Some(sample)
    }
}

pub fn aggregate_base_audio_activity(
    base: &[BaseAudioActivitySample],
    config: TrimConfig,
) -> Vec<AudioActivitySample> {
    if base.is_empty() {
        return Vec::new();
    }

    let mut output = Vec::new();
    let mut window_start = base[0].start.nanos;
    let mut window_end = window_start.saturating_add(config.rms_window_nanos);
    let mut sum_squares = 0.0f64;
    let mut sample_count = 0u64;

    for bucket in base {
        while bucket.start.nanos >= window_end {
            if sample_count > 0 {
                output.push(AudioActivitySample {
                    start: MediaTimestamp::from_nanos(window_start),
                    end: MediaTimestamp::from_nanos(window_end),
                    rms: (sum_squares / sample_count as f64).sqrt() as f32,
                });
            }
            window_start = window_end;
            window_end = window_start.saturating_add(config.rms_window_nanos);
            sum_squares = 0.0;
            sample_count = 0;
        }

        sum_squares += bucket.sum_squares;
        sample_count += bucket.sample_count;
    }

    if sample_count > 0 {
        output.push(AudioActivitySample {
            start: MediaTimestamp::from_nanos(window_start),
            end: MediaTimestamp::from_nanos(window_end),
            rms: (sum_squares / sample_count as f64).sqrt() as f32,
        });
    }

    output
}
```

- [ ] **Step 2: Export the module**

In `src-tauri/src/media/mod.rs`, add:

```rust
pub mod trim_audio_activity;
```

- [ ] **Step 3: Add unit tests for bucket aggregation**

Add tests in `trim_audio_activity.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::core::cut::{TrimConfig, TrimSensitivity};
    use crate::core::frame::{MediaTimestamp, MixedAudioChunk};

    fn chunk(start_nanos: u64, samples: Vec<f32>) -> MixedAudioChunk {
        MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(start_nanos),
            sample_rate: 10,
            channels: 1,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    #[test]
    fn base_audio_activity_emits_100ms_buckets() {
        let mut analyzer = BaseAudioActivityAnalyzer::default();
        let emitted = analyzer.push_chunk(&chunk(0, vec![0.5, 0.5, 0.5]));
        let tail = analyzer.flush().unwrap();

        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0].start.nanos, 0);
        assert_eq!(emitted[0].end.nanos, 100_000_000);
        assert_eq!(tail.start.nanos, 200_000_000);
    }

    #[test]
    fn aggregation_uses_current_sensitivity_window() {
        let base = (0..10)
            .map(|i| BaseAudioActivitySample {
                start: MediaTimestamp::from_nanos(i * 100_000_000),
                end: MediaTimestamp::from_nanos((i + 1) * 100_000_000),
                sum_squares: 1.0,
                sample_count: 100,
            })
            .collect::<Vec<_>>();

        let low = aggregate_base_audio_activity(
            &base,
            TrimConfig::from_sensitivity(TrimSensitivity::Low),
        );
        let high = aggregate_base_audio_activity(
            &base,
            TrimConfig::from_sensitivity(TrimSensitivity::High),
        );

        assert_eq!(low.len(), 1);
        assert_eq!(low[0].end.nanos - low[0].start.nanos, 1_000_000_000);
        assert_eq!(high.len(), 2);
        assert_eq!(high[0].end.nanos - high[0].start.nanos, 500_000_000);
    }
}
```

- [ ] **Step 4: Run the new tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml trim_audio_activity
```

Expected: PASS.

- [ ] **Step 5: Update trim metadata schema**

Change `TrimMetadata` in `src-tauri/src/media/trim_metadata.rs` to include base samples:

```rust
use crate::media::trim_audio_activity::BaseAudioActivitySample;

fn trim_metadata_schema_version() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimMetadata {
    #[serde(default = "trim_metadata_schema_version")]
    pub schema_version: u32,
    pub duration_nanos: u64,
    #[serde(default)]
    pub base_audio_activity: Vec<BaseAudioActivitySample>,
    #[serde(default)]
    pub audio_activity: Vec<AudioActivitySample>,
    pub visual_activity: Vec<FrameDiffSample>,
}
```

Set `schema_version` to `2` for new writes. Keep `schema_version`, `base_audio_activity`, and `audio_activity` readable with serde defaults so old sidecars can still parse during development.

In the same task, update every existing `TrimMetadata { ... }` literal in `src-tauri/src/platform/macos_service.rs` so the crate still compiles before Task 3:

```rust
TrimMetadata {
    schema_version: 1,
    duration_nanos: 0,
    base_audio_activity: Vec::new(),
    audio_activity: Vec::new(),
    visual_activity: Vec::new(),
}
```

- [ ] **Step 6: Update metadata tests**

In `trim_metadata.rs`, update `trim_metadata_round_trips_json()` so it asserts `schemaVersion` and `baseAudioActivity`:

```rust
assert!(json.contains("\"schemaVersion\":2"));
assert!(json.contains("\"baseAudioActivity\""));
```

- [ ] **Step 7: Run metadata tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml trim_metadata
```

Expected: PASS.

Commit:

```bash
git add src-tauri/src/media/trim_audio_activity.rs src-tauri/src/media/mod.rs src-tauri/src/media/trim_metadata.rs
git commit -m "feat: record sensitivity-independent trim audio activity"
```

---

## Task 3: Rebuild Cut Timeline From Base Activity

**Files:**

- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/media/silence_detector.rs`
- Modify: `src-tauri/src/media/trim_metadata.rs`

- [ ] **Step 1: Replace recording-time RMS analyzer usage**

In `src-tauri/src/platform/macos_service.rs`, replace the recording-time `AudioRmsAnalyzer` with `BaseAudioActivityAnalyzer`.

Imports:

```rust
use crate::media::trim_audio_activity::BaseAudioActivityAnalyzer;
```

Consumer locals:

```rust
let mut base_audio_analyzer = BaseAudioActivityAnalyzer::default();
const MAX_BASE_AUDIO_SAMPLES: usize = 360_000; // 10h @ 10/sec
let mut base_audio_activity = Vec::new();
```

When mixed audio is drained:

```rust
for sample in base_audio_analyzer.push_chunk(&mixed) {
    push_bounded_base_audio_sample(&mut base_audio_activity, sample, MAX_BASE_AUDIO_SAMPLES);
}
```

At final drain:

```rust
if let Some(sample) = base_audio_analyzer.flush() {
    push_bounded_base_audio_sample(&mut base_audio_activity, sample, MAX_BASE_AUDIO_SAMPLES);
}
```

The new `RecordingConsumerOutput` metadata should be:

```rust
TrimMetadata {
    schema_version: 2,
    duration_nanos,
    base_audio_activity,
    audio_activity: Vec::new(),
    visual_activity,
}
```

- [ ] **Step 2: Add bounded helper for base audio**

Add in `macos_service.rs`:

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

- [ ] **Step 3: Update `build_cut_timeline()` aggregation**

In `src-tauri/src/lib.rs`, after reading metadata and before calling `SilenceDetectorEngine::analyze()`, derive audio activity:

```rust
let audio_activity = if !metadata.base_audio_activity.is_empty() {
    crate::media::trim_audio_activity::aggregate_base_audio_activity(
        &metadata.base_audio_activity,
        trim_config,
    )
} else {
    metadata.audio_activity.clone()
};

let detector = SilenceDetectorEngine::new(trim_config);
let timeline = detector
    .analyze(
        &audio_activity,
        &metadata.visual_activity,
        metadata.duration_nanos,
    )
    .map_err(|error| error.to_string())?;
```

This keeps old sidecars usable while new sidecars get full post-recording sensitivity semantics.

- [ ] **Step 4: Add focused Rust test for post-recording sensitivity**

Add a pure test near `build_cut_timeline` helpers or in `trim_audio_activity.rs`:

```rust
#[test]
fn build_cut_timeline_reaggregates_base_audio_activity_by_sensitivity() {
    use crate::core::cut::{FrameDiffSample, TrimConfig, TrimSensitivity};
    use crate::core::frame::MediaTimestamp;
    use crate::media::silence_detector::SilenceDetectorEngine;
    use crate::media::trim_audio_activity::{
        aggregate_base_audio_activity, BaseAudioActivitySample,
    };
    use crate::core::processor::SilenceDetector;

    let base = (0..100)
        .map(|i| BaseAudioActivitySample {
            start: MediaTimestamp::from_nanos(i * 100_000_000),
            end: MediaTimestamp::from_nanos((i + 1) * 100_000_000),
            sum_squares: 0.0,
            sample_count: 4800,
        })
        .collect::<Vec<_>>();
    let visual = vec![FrameDiffSample {
        start: MediaTimestamp::from_nanos(0),
        end: MediaTimestamp::from_nanos(10_000_000_000),
        change_ratio: 0.0,
    }];

    let low_config = TrimConfig::from_sensitivity(TrimSensitivity::Low);
    let high_config = TrimConfig::from_sensitivity(TrimSensitivity::High);
    let low_audio = aggregate_base_audio_activity(&base, low_config);
    let high_audio = aggregate_base_audio_activity(&base, high_config);

    assert!(low_audio.len() < high_audio.len());

    let low_timeline = SilenceDetectorEngine::new(low_config)
        .analyze(&low_audio, &visual, 10_000_000_000)
        .unwrap();
    let high_timeline = SilenceDetectorEngine::new(high_config)
        .analyze(&high_audio, &visual, 10_000_000_000)
        .unwrap();

    assert!(high_timeline.total_cut_nanos >= low_timeline.total_cut_nanos);
}
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml reaggregates_base_audio_activity
```

Expected: PASS.

- [ ] **Step 6: Run all Rust unit tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

Commit:

```bash
git add src-tauri/src/platform/macos_service.rs src-tauri/src/lib.rs src-tauri/src/media/silence_detector.rs src-tauri/src/media/trim_metadata.rs src-tauri/src/media/trim_audio_activity.rs
git commit -m "feat: rebuild trim timeline from base audio activity"
```

---

## Task 4: Add Deterministic Recording and Export Paths

**Files:**

- Create: `src-tauri/src/media/export_paths.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create path helper module**

Create `src-tauri/src/media/export_paths.rs`:

```rust
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::media::trim_exporter::ExportPreset;

pub fn recordings_dir() -> PathBuf {
    std::env::temp_dir().join("luzhi-recordings")
}

pub fn original_recording_path(session_id: u64) -> PathBuf {
    recordings_dir().join(format!("recording-{session_id}.mp4"))
}

pub fn export_output_path(preset: ExportPreset) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    recordings_dir().join(format!("export-{preset:?}-{millis}-{seq}.mp4"))
}
```

- [ ] **Step 2: Export the module**

In `src-tauri/src/media/mod.rs`, add:

```rust
pub mod export_paths;
```

- [ ] **Step 3: Add path tests**

Add tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::trim_exporter::ExportPreset;

    #[test]
    fn original_recording_path_is_mp4_under_recordings_dir() {
        let path = original_recording_path(42);
        assert!(path.ends_with("luzhi-recordings/recording-42.mp4"));
    }

    #[test]
    fn export_output_path_is_unique() {
        let first = export_output_path(ExportPreset::Bilibili);
        let second = export_output_path(ExportPreset::Bilibili);
        assert_ne!(first, second);
        assert_eq!(first.extension().and_then(|value| value.to_str()), Some("mp4"));
    }
}
```

- [ ] **Step 4: Run path tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_paths
```

Expected: PASS.

Commit:

```bash
git add src-tauri/src/media/export_paths.rs src-tauri/src/media/mod.rs
git commit -m "feat: add deterministic media export paths"
```

---

## Task 5: Implement Original Recording Writer

**Files:**

- Modify: `src-tauri/src/media/ffmpeg_writer.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/media/recording_writer.rs`

This task is the Native Safety Gate for FFmpeg recording. It should be implemented and reviewed separately.

- [ ] **Step 1: Add a writer factory boundary before FFmpeg code**

In `macos_service.rs`, create a small helper so non-FFmpeg test builds keep using `CountingRecordingWriter`, while FFmpeg-enabled builds fail fast if the production writer cannot initialize. Do not silently fall back to a counting writer when the `ffmpeg` feature is enabled.

```rust
fn recording_writer_for_session(session_id: u64) -> AppResult<Box<dyn RecordingWriter>> {
    #[cfg(feature = "ffmpeg")]
    {
        let path = crate::media::export_paths::original_recording_path(session_id);
        return crate::media::ffmpeg_writer::FfmpegRecordingWriter::new(path)
            .map(|writer| Box::new(writer) as Box<dyn RecordingWriter>);
    }

    #[cfg(not(feature = "ffmpeg"))]
    {
        Ok(Box::new(CountingRecordingWriter::new(None)))
    }
}
```

Use it in `start()`:

```rust
let writer: Box<dyn RecordingWriter> = recording_writer_for_session(self.session_id)?;
```

- [ ] **Step 2: Make writer errors fatal to finalization**

Change `consume_frames()` to remember writer errors:

```rust
let mut writer_error: Option<String> = None;
```

Whenever `push_video`, `push_audio`, or `finish()` fails, store the first error. Extend `RecordingConsumerOutput`:

```rust
struct RecordingConsumerOutput {
    result: RecordingResult,
    trim_metadata: TrimMetadata,
    writer_error: Option<String>,
}
```

In `stop()`, after join:

```rust
if let Some(error) = &consumer_output.writer_error {
    errors.push(format!("录制写入失败: {error}"));
}
```

This prevents a broken FFmpeg writer from returning a successful but unplayable recording.

- [ ] **Step 3: Implement FFmpeg writer resource lifecycle**

Replace the skeleton in `ffmpeg_writer.rs` with a real writer using `ffmpeg-next`.

Required behavior:

- `new(output_path)` initializes FFmpeg and creates parent directories.
- `push_video(frame)` accepts BGRA frames and enqueues them into a dedicated encoding worker with a bounded queue. Synchronous FFmpeg encode inside `consume_frames()` is out of scope for this plan because it can slow the drain loop and cause capture-channel drops.
- `push_audio(chunk)` accepts interleaved f32 PCM and follows the same bounded worker rule.
- The worker converts video to YUV420P or another encoder-supported format, converts/resamples audio to the selected encoder format, and writes packets in timestamp order.
- `finish()` flushes encoders, writes trailer, closes the output context, verifies file metadata exists, and returns `RecordingResult { output_path: Some(...) }`.
- All `ffmpeg-next` errors map to `AppError::RecordingWriteFailed`.
- No media frame is sent to React.
- No FFmpeg CLI command string exists anywhere in this path.

Implementation notes:

- Prefer H.264 + AAC in MP4 for MVP if available.
- Preserve source timestamps by converting `MediaTimestamp.nanos` to stream time bases.
- If an encoder is unavailable on the developer machine, return a structured `RecordingWriteFailed` and fall back only if the product explicitly accepts non-playable dev builds. Product-complete Phase 5 should not silently fall back in release builds.
- Ensure RAII drop does not panic; errors must surface through `finish()`.

- [ ] **Step 4: Add unit test for writer factory fallback**

Add in `macos_service.rs` tests:

```rust
#[cfg(not(feature = "ffmpeg"))]
#[test]
fn recording_writer_factory_returns_writer() {
    let mut writer = recording_writer_for_session(1).unwrap();
    let result = writer.finish().unwrap();

    assert_eq!(result.output_path, None);
}
```

- [ ] **Step 5: Add feature-gated smoke test for real writer**

Create a feature-gated test in `ffmpeg_writer.rs`:

```rust
#[cfg(all(test, feature = "ffmpeg"))]
mod ffmpeg_tests {
    use std::sync::Arc;

    use super::*;
    use crate::core::frame::{
        FrameBuffer, MediaTimestamp, MixedAudioChunk, PixelFormat, VideoFrame,
    };

    #[test]
    fn ffmpeg_writer_creates_nonempty_playable_artifact() {
        let path = std::env::temp_dir()
            .join("luzhi-recordings")
            .join("ffmpeg-writer-smoke.mp4");
        let _ = std::fs::remove_file(&path);

        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();
        for i in 0..30 {
            let frame = Arc::new(VideoFrame {
                timestamp: MediaTimestamp::from_nanos(i * 33_333_333),
                width: 64,
                height: 64,
                stride_bytes: 64 * 4,
                pixel_format: PixelFormat::Bgra8,
                buffer: FrameBuffer::Owned(Arc::from(vec![0u8; 64 * 64 * 4].into_boxed_slice())),
            });
            writer.push_video(frame).unwrap();
        }
        writer
            .push_audio(MixedAudioChunk {
                timestamp: MediaTimestamp::from_nanos(0),
                sample_rate: 48_000,
                channels: 2,
                samples: Arc::from(vec![0.0f32; 48_000 * 2].into_boxed_slice()),
            })
            .unwrap();

        let result = writer.finish().unwrap();

        assert_eq!(result.output_path, Some(path.to_string_lossy().to_string()));
        assert!(std::fs::metadata(path).unwrap().len() > 0);
    }
}
```

- [ ] **Step 6: Run writer tests**

Run without FFmpeg:

```bash
cargo test --manifest-path src-tauri/Cargo.toml recording_writer_factory_returns_writer
```

Expected: PASS.

Run with FFmpeg:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_creates_nonempty_playable_artifact
```

Expected: PASS on a machine with FFmpeg development libraries. If it fails because FFmpeg libraries are unavailable, record that as an environment blocker, not a code pass.

Commit:

```bash
git add src-tauri/src/media/ffmpeg_writer.rs src-tauri/src/platform/macos_service.rs src-tauri/src/media/recording_writer.rs
git commit -m "feat: write original recordings with ffmpeg"
```

---

## Task 6: Implement Structured FFmpeg Trim Exporter

**Files:**

- Modify: `src-tauri/src/media/trim_exporter.rs`
- Create: `src-tauri/tests/ffmpeg_trim_export.rs`

This task is the Native Safety Gate for playable trimmed export. Review resource release and timestamp handling carefully.

- [ ] **Step 1: Extend `TrimExportRequest` for no-trim exports**

Keep the request shape and allow `cut_timeline.cuts` to be empty:

```rust
pub struct TrimExportRequest {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub preset: ExportPreset,
    pub cut_timeline: CutTimeline,
}
```

No new frontend fields are needed for Phase 5.

- [ ] **Step 2: Implement keep segment derivation helper**

Add in `trim_exporter.rs`:

```rust
fn export_keeps(timeline: &CutTimeline) -> &[crate::core::cut::KeepSegment] {
    &timeline.keeps
}
```

Add test:

```rust
#[test]
fn exporter_uses_keep_segments_from_cut_timeline() {
    let timeline = CutTimeline::empty(5_000_000_000);
    assert_eq!(export_keeps(&timeline).len(), 1);
    assert_eq!(export_keeps(&timeline)[0].start.nanos, 0);
    assert_eq!(export_keeps(&timeline)[0].end.nanos, 5_000_000_000);
}
```

- [ ] **Step 3: Implement FFmpeg exporter behavior**

Under `#[cfg(feature = "ffmpeg")]`, make `FfmpegTrimExporter::export()` perform a real export.

Required behavior:

- Validate input exists and is non-empty.
- Validate the `CutTimeline` is sorted, non-overlapping, and within `duration_nanos`; reject invalid timelines with a structured error.
- Create output parent directory.
- Open input through FFmpeg binding.
- Decode video/audio streams.
- For each decoded frame/sample, map input timestamp into the concatenated export timeline:
  - Drop media whose timestamp is inside a cut segment.
  - Keep media whose timestamp falls inside a keep segment.
  - Shift kept timestamps left by the sum of earlier cut durations.
- Split audio frames at cut boundaries instead of dropping a whole decoded audio frame when only part of it overlaps a cut. Video may cut on decoded frame boundaries for MVP, because the exporter re-encodes and does not require source keyframe alignment.
- Ensure output PTS/DTS are monotonic per stream after timestamp remapping.
- Re-encode/mux into `request.output_path`.
- Flush encoders and write trailer.
- Return `TrimExportResult { output_path, cut_count }` only after output file exists and has non-zero size.
- Preserve original input file.
- Report errors as `AppError::RecordingWriteFailed` unless a dedicated export error variant is added in the same task with serialization/tests.

Timestamp helper to add before FFmpeg plumbing:

```rust
fn remap_kept_timestamp_nanos(timeline: &CutTimeline, input_nanos: u64) -> Option<u64> {
    let mut removed_before = 0u64;
    for cut in &timeline.cuts {
        if input_nanos >= cut.start.nanos && input_nanos < cut.end.nanos {
            return None;
        }
        if input_nanos >= cut.end.nanos {
            removed_before =
                removed_before.saturating_add(cut.end.nanos.saturating_sub(cut.start.nanos));
        }
    }
    Some(input_nanos.saturating_sub(removed_before))
}
```

Tests:

```rust
#[test]
fn remap_kept_timestamp_drops_cut_ranges_and_shifts_later_media() {
    use crate::core::cut::{CutReason, CutSegment, KeepSegment};
    use crate::core::frame::MediaTimestamp;

    let timeline = CutTimeline {
        duration_nanos: 10_000_000_000,
        cuts: vec![CutSegment {
            start: MediaTimestamp::from_nanos(2_000_000_000),
            end: MediaTimestamp::from_nanos(4_000_000_000),
            reason: CutReason::SilentAndStill,
            mean_audio_rms: 0.0,
            mean_visual_change: 0.0,
        }],
        keeps: vec![
            KeepSegment {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(2_000_000_000),
            },
            KeepSegment {
                start: MediaTimestamp::from_nanos(4_000_000_000),
                end: MediaTimestamp::from_nanos(10_000_000_000),
            },
        ],
        total_cut_nanos: 2_000_000_000,
    };

    assert_eq!(remap_kept_timestamp_nanos(&timeline, 1_000_000_000), Some(1_000_000_000));
    assert_eq!(remap_kept_timestamp_nanos(&timeline, 3_000_000_000), None);
    assert_eq!(remap_kept_timestamp_nanos(&timeline, 5_000_000_000), Some(3_000_000_000));
}
```

- [ ] **Step 4: Add feature-gated integration test**

Create `src-tauri/tests/ffmpeg_trim_export.rs`:

```rust
#![cfg(feature = "ffmpeg")]

use luzhi_lib::core::cut::CutTimeline;
use luzhi_lib::core::frame::{
    FrameBuffer, MediaTimestamp, MixedAudioChunk, PixelFormat, VideoFrame,
};
use luzhi_lib::media::ffmpeg_writer::FfmpegRecordingWriter;
use luzhi_lib::media::recording_writer::RecordingWriter;
use luzhi_lib::media::trim_exporter::{
    ExportPreset, FfmpegTrimExporter, TrimExportRequest, TrimExporter,
};

#[test]
fn ffmpeg_trim_exporter_creates_nonempty_output_for_noop_timeline() {
    let input = std::env::temp_dir()
        .join("luzhi-recordings")
        .join("trim-export-input.mp4");
    let output = std::env::temp_dir()
        .join("luzhi-recordings")
        .join("trim-export-noop.mp4");
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);

    let mut writer = FfmpegRecordingWriter::new(input.clone()).unwrap();
    for i in 0..30 {
        let frame = std::sync::Arc::new(VideoFrame {
            timestamp: MediaTimestamp::from_nanos(i * 33_333_333),
            width: 64,
            height: 64,
            stride_bytes: 64 * 4,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(std::sync::Arc::from(
                vec![0u8; 64 * 64 * 4].into_boxed_slice(),
            )),
        });
        writer.push_video(frame).unwrap();
    }
    writer
        .push_audio(MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 2,
            samples: std::sync::Arc::from(vec![0.0f32; 48_000 * 2].into_boxed_slice()),
        })
        .unwrap();
    writer.finish().unwrap();

    let mut exporter = FfmpegTrimExporter;
    let result = exporter
        .export(TrimExportRequest {
            input_path: input.clone(),
            output_path: output.clone(),
            preset: ExportPreset::Bilibili,
            cut_timeline: CutTimeline::empty(1_000_000_000),
        })
        .unwrap();

    assert_eq!(result.output_path, output);
    assert!(std::fs::metadata(&result.output_path).unwrap().len() > 0);
    assert!(std::fs::metadata(input).unwrap().len() > 0);
}
```

- [ ] **Step 5: Run exporter tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remap_kept_timestamp_drops_cut_ranges_and_shifts_later_media
```

Expected: PASS.

Run with FFmpeg:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_trim_exporter_creates_nonempty_output_for_noop_timeline
```

Expected: PASS on a machine with FFmpeg development libraries.

Commit:

```bash
git add src-tauri/src/media/trim_exporter.rs src-tauri/tests/ffmpeg_trim_export.rs
git commit -m "feat: export playable trimmed videos with ffmpeg"
```

---

## Task 7: Wire `export_video()` to the Real Exporter

**Files:**

- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/media/trim_exporter.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/components/preview-view.tsx`
- Modify: `src/App.test.tsx`

- [ ] **Step 1: Add source recording path lookup**

In `MacRecordingService`, add:

```rust
pub fn last_recording_output_path(&self) -> Option<String> {
    self.last_recording_output_path.clone()
}
```

Back this with a new `last_recording_output_path: Option<String>` field set in `stop()` from `result.output_path.clone()`.

Add the field:

```rust
last_recording_output_path: Option<String>,
```

Initialize it in `new()`:

```rust
last_recording_output_path: None,
```

Clear it at recording start with the other stale session paths:

```rust
self.last_recording_output_path = None;
```

Set it in `stop()` after the consumer result is available:

```rust
self.last_recording_output_path = result.output_path.clone();
```

- [ ] **Step 2: Make export require a real source file**

In `export_video()` after building timelines:

```rust
let source_path = {
    let service = state
        .service
        .lock()
        .map_err(|_| "录制服务锁已损坏".to_string())?;
    service
        .last_recording_output_path()
        .ok_or_else(|| "没有可导出的可播放录制文件，请先完成一次 FFmpeg 录制。".to_string())?
};
```

- [ ] **Step 3: Build request and call exporter under feature flag**

In `export_video()`, use the cut sidecar when auto-trim is enabled. When auto-trim is disabled, derive a no-op timeline from the last trim metadata duration; if no trim metadata exists, return a structured error instead of inventing a duration.

Make sure the exporter trait is in scope:

```rust
use media::trim_exporter::TrimExporter;
```

```rust
let cut_timeline = if let Some(summary) = &cut {
    TrimMetadataWriter::read_cut_timeline(std::path::Path::new(&summary.cut_timeline_path))
        .map_err(|error| error.to_string())?
} else {
    let trim_metadata_path = {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        service
            .last_trim_metadata_path()
            .ok_or_else(|| "没有可用的录制时长元数据，无法导出。请重新录制。".to_string())?
    };
    let metadata = TrimMetadataWriter::read_metadata(std::path::Path::new(&trim_metadata_path))
        .map_err(|error| error.to_string())?;
    CutTimeline::empty(metadata.duration_nanos)
};
let output_path = media::export_paths::export_output_path(_export_preset);

let request = media::trim_exporter::TrimExportRequest {
    input_path: std::path::PathBuf::from(source_path),
    output_path: output_path.clone(),
    preset: _export_preset,
    cut_timeline,
};

#[cfg(feature = "ffmpeg")]
let export_result = tauri::async_runtime::spawn_blocking(move || {
    let mut exporter = media::trim_exporter::FfmpegTrimExporter;
    exporter.export(request).map_err(|error| error.to_string())
})
.await
.map_err(|join_error| format!("FFmpeg 导出任务失败: {join_error}"))??;

#[cfg(not(feature = "ffmpeg"))]
{
    let _ = request;
    return Err("FFmpeg 导出未启用，请使用带 ffmpeg feature 的构建。".to_string());
};
```

Then return:

```rust
output_path: Some(export_result.output_path.to_string_lossy().to_string()),
```

- [ ] **Step 4: Update frontend no-Gate display**

In `PreviewView`, keep current Gate copy only when `outputPath` is null. Add a concise success line when present:

```tsx
{exportSummary.outputPath && (
  <p className="opacity-80">已生成可播放导出文件</p>
)}
```

- [ ] **Step 5: Add frontend test**

Assert the Gate message is absent when `outputPath` is present, as in Task 1.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml export_summary
npm test -- --run src/App.test.tsx
```

Expected: PASS.

Commit:

```bash
git add src-tauri/src/lib.rs src-tauri/src/platform/macos_service.rs src/components/preview-view.tsx src/App.test.tsx src/lib/tauri.ts
git commit -m "feat: wire export command to ffmpeg trim exporter"
```

---

## Task 8: Verification and Manual Gates

**Files:**

- Modify: `tests/phase-5-w9-w10-checklist.md`
- Modify: `HANDOFF.md`
- Modify: `docs/superpowers/reviews/2026-05-29-phase-5-code-review.md`

- [ ] **Step 1: Run formatting and standard tests**

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

Expected: all PASS. Clippy should not introduce new warnings outside already documented FFI warnings.

- [ ] **Step 2: Run FFmpeg feature verification**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo build --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

Expected: PASS on an environment with FFmpeg development libraries.

- [ ] **Step 3: Scan BUG.md prevention rules**

Run:

```bash
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

Expected:

- No `data-tauri-drag-region={false}` or `"false"`.
- No cursor ignore/click-through API usage unless explicitly reviewed.
- Any remaining `whileTap` must be limited to safe button feedback and documented.

- [ ] **Step 4: Manual recording/export test**

Manual test:

1. Launch app with FFmpeg feature enabled.
2. Record 10-15 seconds at 1080p.
3. Include at least one 6-8 second silent/still segment.
4. Stop recording.
5. Enable auto trim.
6. Switch sensitivity Low, Medium, High and rebuild/export each time.
7. Confirm each export returns `outputPath`.
8. Play each output in the app or system player.
9. Confirm original recording still exists.
10. Confirm exported duration is shorter when cut segments are detected.
11. Confirm audio/video sync has no obvious drift.

- [ ] **Step 5: Manual long recording pressure test**

Manual test:

1. Record 10 minutes at 1080p.
2. Confirm capture remains responsive.
3. Confirm stop completes.
4. Confirm trim metadata size is bounded.
5. Confirm export completes or returns a structured FFmpeg error.
6. Confirm no panic and no leaked background task.

- [ ] **Step 6: Update checklist and review doc**

Only after Step 4 and Step 5 both pass, update `tests/phase-5-w9-w10-checklist.md`:

```markdown
- [x] FFmpeg 封装能消费 `CutTimeline`。
- [x] 裁剪后视频可播放。
- [x] 音视频同步未明显漂移。
```

Add a new review subsection noting:

- FFmpeg Gate is closed.
- Sensitivity is now post-recording rebuildable.
- Original artifact is preserved.
- Any remaining manual risk belongs to broader Phase 6 presets/compositor work.

- [ ] **Step 7: Final verification**

Run:

```bash
git diff --check HEAD
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

Expected: all PASS.

Commit:

```bash
git add tests/phase-5-w9-w10-checklist.md HANDOFF.md docs/superpowers/reviews/2026-05-29-phase-5-code-review.md
git commit -m "docs: close phase 5 ffmpeg trim gates"
```

---

## Risk Controls

- Capture callback must remain minimal: no FFmpeg, no RMS aggregation, no frame diff inside ScreenCaptureKit callbacks.
- Consumer thread may enqueue media into the recording writer, but expensive FFmpeg encoding must run in the writer worker; writer failures must propagate to `stop()` as `RecordingFinalizeFailed`.
- Export must run in `spawn_blocking` or an equivalent background task; never block Tauri command/event loop with FFmpeg work.
- All FFmpeg integration must use structured paths and Rust bindings; no command string construction.
- Original recording path and export path must be separate.
- Export result must verify the output file exists and has non-zero size before returning `outputPath`.
- Manual Native Safety Gate is mandatory for `ffmpeg_writer.rs` and `trim_exporter.rs`.

---

## Success Criteria

Phase 5 can be called complete only when all of these are true:

- `build_cut_timeline()` rebuilds from base RMS buckets using the current Preview sensitivity.
- Changing sensitivity after recording can change the resulting cut timeline without re-recording.
- `export_video()` returns `outputPath: Some(...)` only for an actual playable file.
- A cut timeline with one or more cuts produces a shorter playable export.
- A no-op cut timeline produces a playable export equivalent to the original duration.
- Original recording artifact remains available after export.
- FFmpeg tests pass in an FFmpeg-enabled environment.
- Manual playback and sync checks pass.
- BUG.md prevention scan has no regression.
