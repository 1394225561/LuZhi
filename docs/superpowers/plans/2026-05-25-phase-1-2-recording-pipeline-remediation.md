# Phase 1/2 Recording Pipeline Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repair the Phase 1 and Phase 2 recording foundation so macOS can build, start/stop capture without blocking the command path, keep media streams in Rust, mix system audio and microphone by timestamp, release native resources reliably, and produce a verifiable local recording artifact with audio.

**Architecture:** Keep React as a command/status layer only. Rust owns capture control, bounded media queues, timestamp normalization, audio synchronization, writing/encoding, permission checks, and lifecycle cleanup. ScreenCaptureKit and cpal remain isolated under platform adapters; unsafe/FFI code is narrowed behind testable conversion helpers and human review gates.

**Tech Stack:** Tauri 2, React + TypeScript, Rust, ScreenCaptureKit, CoreMedia/CoreVideo FFI, cpal, Rust unit tests, Vitest. A production MP4 writer requires a human-approved FFmpeg binding dependency before execution.

---

## Context

This remediation plan is based on:

- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/2026-05-23-phase-1-macos-recording-foundation.md`
- `docs/superpowers/plans/2026-05-24-phase-2-windows-capture-research-dual-audio.md`
- `tests/phase-1-w1-w2-checklist.md`
- `tests/phase-2-w3-w4-checklist.md`
- `HANDOFF.md`
- `BUG.md`
- Code review findings from 2026-05-25

Current state:

- `cargo test --manifest-path src-tauri/Cargo.toml` passes 34 tests.
- `npm test -- --run` passes 4 tests.
- `npm run build` passes.
- `cargo build --manifest-path src-tauri/Cargo.toml` fails because `_CMFormatDescriptionGetStreamBasicDescription` is unresolved.
- Captured video, system audio, and microphone chunks are drained and discarded.
- `SimpleAudioMixer` is tested but not integrated into the recording pipeline.
- cpal microphone timestamps are always `0`.
- ScreenCaptureKit audio extraction assumes f32 and a one-buffer `AudioBufferList`.
- Capture startup and stop wait synchronously on the Tauri command path.
- Media channels are unbounded and callbacks copy full frames before sending.

## Assumptions

1. The remediation scope is limited to Phase 1 and Phase 2 correctness. Cursor effects, silence trimming, export presets, licensing, and telemetry remain outside this plan.
2. The first production artifact target is a local MP4 or equivalent playable intermediate file. The preferred production path is an FFmpeg binding, not a CLI command.
3. AI must not directly change core dependency versions in `src-tauri/Cargo.toml` during execution without explicit human approval.
4. All ScreenCaptureKit/cpal unsafe wrappers and callback lifetime assumptions require human review before the feature can be marked accepted.
5. Windows Phase 2 remediation means build-safe stubs and documented feasibility gates unless a Windows test machine is available.

## Manual Gates

1. **Dependency Gate:** Before adding or changing `ffmpeg-next`, `objc2-*`, `cpal`, or other Rust dependency versions, stop and get human approval.
2. **Native Safety Gate:** Before accepting ScreenCaptureKit/cpal changes, human review must inspect memory ownership, callback threading, resource release, and error propagation.
3. **Artifact Gate:** Before declaring Phase 1/2 complete, run a real macOS recording and verify the resulting file has video and audio tracks.
4. **Windows Gate:** If no Windows machine is available, record the missing feasibility validation explicitly and keep Windows code build-safe.
5. **Git Gate:** Do not run `git push` or merge branches.

## File Map

### Create

- `src-tauri/src/core/media_channel.rs` - bounded, non-blocking media sender/receiver wrappers.
- `src-tauri/src/core/clock.rs` - media timestamp normalization and microphone sample clock.
- `src-tauri/src/media/audio_synchronizer.rs` - timestamp-based pairing of system and microphone chunks into `MixedAudioChunk`.
- `src-tauri/src/media/recording_writer.rs` - writer trait, recording result model, and test writer.
- `src-tauri/src/media/ffmpeg_writer.rs` - production writer after the dependency gate.
- `src-tauri/src/app/recording_runtime.rs` - cancellation handles, tick lifecycle, and background command helpers.
- `tests/phase-1-2-remediation-checklist.md` - manual and automated verification checklist.

### Modify

- `src-tauri/src/core/mod.rs` - export `clock` and `media_channel`.
- `src-tauri/src/core/capture.rs` - use bounded media sink types.
- `src-tauri/src/core/frame.rs` - add helpers needed by writer and audio tests.
- `src-tauri/src/media/audio_mixer.rs` - fix alignment semantics and add stricter tests.
- `src-tauri/src/media/mod.rs` - export new media modules.
- `src-tauri/src/platform/macos/screen_capture_kit.rs` - fix CoreMedia symbol, safe audio format handling, bounded non-blocking send, stop error handling.
- `src-tauri/src/platform/macos/cpal_microphone.rs` - real monotonic microphone timestamps and lower-risk callback locking.
- `src-tauri/src/platform/macos_service.rs` - integrate bounded queues, audio synchronizer, writer, thread handles, cancellation, and lifecycle cleanup.
- `src-tauri/src/app/recording_service.rs` - either remove dead generic service or align it with the real runtime path.
- `src-tauri/src/lib.rs` - async command path, permission service wiring, frontend config shape, tick cancellation, result payload.
- `src-tauri/src/app/events.rs` - add fields needed by UI status and recording result.
- `src-tauri/src/app/permission_service.rs` - add concrete macOS probe wiring.
- `src-tauri/src/platform/macos/permissions.rs` - real screen/microphone permission probe.
- `src-tauri/src/platform/windows/dxgi_capture.rs` - fix imports and Windows build-safety.
- `src-tauri/src/platform/windows/wasapi_loopback.rs` - fix build-safety and capability messages.
- `src-tauri/src/platform/mod.rs` - expose platform services with correct cfg.
- `src/lib/tauri.ts` - align command payloads with Rust serde shapes.
- `src/App.tsx` - apply audio/capture config before recording and remove large false drag-region wrappers.
- `src/App.test.tsx` - verify audio/capture config invoke sequence and BUG.md drag prevention.
- `HANDOFF.md` - record the completed remediation plan after implementation is verified.

## Phase A: Rebuild Baseline and Stop Known Breakage

### Task 1: Fix CoreMedia Build Blocker

**Files:**

- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs`

- [ ] **Step 1: Write a narrow build expectation note in the task log**

Record this before editing:

```text
Expected failure before fix:
cargo build --manifest-path src-tauri/Cargo.toml
Undefined symbols for architecture arm64:
  "_CMFormatDescriptionGetStreamBasicDescription"
```

- [ ] **Step 2: Replace the incorrect CoreMedia symbol**

Change the extern declaration from:

```rust
fn CMFormatDescriptionGetStreamBasicDescription(
    desc: CMFormatDescriptionRef,
) -> *const AudioStreamBasicDescription;
```

to the CoreMedia audio-specific symbol:

```rust
fn CMAudioFormatDescriptionGetStreamBasicDescription(
    desc: CMFormatDescriptionRef,
) -> *const AudioStreamBasicDescription;
```

Change the wrapper body to:

```rust
unsafe fn cmformat_description_get_stream_basic_description(
    desc: CMFormatDescriptionRef,
) -> *const AudioStreamBasicDescription {
    CMAudioFormatDescriptionGetStreamBasicDescription(desc)
}
```

- [ ] **Step 3: Run binary build verification**

Run:

```bash
cargo build --manifest-path src-tauri/Cargo.toml
```

Expected:

```text
Finished `dev` profile
```

If it still fails on a CoreMedia symbol, stop and record the exact symbol in `HANDOFF.md` under known blockers.

- [ ] **Step 4: Commit**

Run:

```bash
git add src-tauri/src/platform/macos/screen_capture_kit.rs
git commit -m "fix(record): 修复CoreMedia音频格式符号链接"
```

Expected:

```text
Commit created
```

### Task 2: Add Bounded Non-Blocking Media Channels

**Files:**

- Create: `src-tauri/src/core/media_channel.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Modify: `src-tauri/src/core/capture.rs`
- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs`
- Modify: `src-tauri/src/platform/macos/cpal_microphone.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/app/recording_service.rs`

- [ ] **Step 1: Add failing tests for bounded behavior**

Create `src-tauri/src/core/media_channel.rs` with these tests first:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_newest_when_full_without_blocking() {
        let (sender, receiver) = bounded_media_channel::<u32>(1);

        assert!(sender.try_send_drop_newest(1));
        assert!(!sender.try_send_drop_newest(2));

        assert_eq!(receiver.dropped_count(), 1);
        assert_eq!(receiver.try_recv().unwrap(), 1);
    }

    #[test]
    fn reports_disconnected_receiver_as_drop() {
        let (sender, receiver) = bounded_media_channel::<u32>(1);
        drop(receiver);

        assert!(!sender.try_send_drop_newest(1));
        assert_eq!(sender.dropped_count(), 1);
    }
}
```

- [ ] **Step 2: Run failing test**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::media_channel
```

Expected:

```text
FAIL or compile error because bounded_media_channel is not implemented
```

- [ ] **Step 3: Implement bounded channel wrappers**

Use this implementation in `src-tauri/src/core/media_channel.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;

#[derive(Debug)]
pub struct MediaSender<T> {
    inner: SyncSender<T>,
    dropped: Arc<AtomicU64>,
}

#[derive(Debug)]
pub struct MediaReceiver<T> {
    inner: Receiver<T>,
    dropped: Arc<AtomicU64>,
}

pub fn bounded_media_channel<T>(capacity: usize) -> (MediaSender<T>, MediaReceiver<T>) {
    assert!(capacity > 0, "media channel capacity must be greater than zero");
    let (inner_sender, inner_receiver) = sync_channel(capacity);
    let dropped = Arc::new(AtomicU64::new(0));

    (
        MediaSender {
            inner: inner_sender,
            dropped: dropped.clone(),
        },
        MediaReceiver {
            inner: inner_receiver,
            dropped,
        },
    )
}

impl<T> Clone for MediaSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            dropped: self.dropped.clone(),
        }
    }
}

impl<T> MediaSender<T> {
    pub fn try_send_drop_newest(&self, item: T) -> bool {
        match self.inner.try_send(item) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl<T> MediaReceiver<T> {
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.inner.try_recv()
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_newest_when_full_without_blocking() {
        let (sender, receiver) = bounded_media_channel::<u32>(1);

        assert!(sender.try_send_drop_newest(1));
        assert!(!sender.try_send_drop_newest(2));

        assert_eq!(receiver.dropped_count(), 1);
        assert_eq!(receiver.try_recv().unwrap(), 1);
    }

    #[test]
    fn reports_disconnected_receiver_as_drop() {
        let (sender, receiver) = bounded_media_channel::<u32>(1);
        drop(receiver);

        assert!(!sender.try_send_drop_newest(1));
        assert_eq!(sender.dropped_count(), 1);
    }
}
```

- [ ] **Step 4: Export the module**

Modify `src-tauri/src/core/mod.rs`:

```rust
pub mod capture;
pub mod clock;
pub mod config;
pub mod frame;
pub mod media_channel;
```

If `clock` does not exist yet, add it in Task 3 before running full builds.

- [ ] **Step 5: Change capture sink type aliases**

In `src-tauri/src/core/capture.rs`, replace `std::sync::mpsc::Sender` with `MediaSender`:

```rust
use crate::core::media_channel::MediaSender;

pub type VideoFrameSink = MediaSender<VideoFrameRef>;
pub type AudioChunkSink = MediaSender<AudioChunk>;
```

- [ ] **Step 6: Replace callback sends**

In `screen_capture_kit.rs`, replace:

```rust
let _ = sink.send(Arc::new(frame));
```

with:

```rust
let _sent = sink.try_send_drop_newest(Arc::new(frame));
```

Replace:

```rust
let _ = sink.send(chunk);
```

with:

```rust
let _sent = sink.try_send_drop_newest(chunk);
```

In `cpal_microphone.rs`, replace:

```rust
let _ = s.send(chunk);
```

with:

```rust
let _sent = s.try_send_drop_newest(chunk);
```

- [ ] **Step 7: Replace unbounded channel creation in services**

In `macos_service.rs`, replace `channel()` calls with:

```rust
use crate::core::media_channel::bounded_media_channel;

const VIDEO_QUEUE_CAPACITY: usize = 90;
const AUDIO_QUEUE_CAPACITY: usize = 256;

let (video_sender, video_receiver) = bounded_media_channel(VIDEO_QUEUE_CAPACITY);
let (audio_sender, audio_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY);
```

For microphone:

```rust
let (mic_sender, mic_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY);
```

- [ ] **Step 8: Run bounded channel tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::media_channel
```

Expected:

```text
2 passed
```

- [ ] **Step 9: Commit**

Run:

```bash
git add src-tauri/src/core/media_channel.rs src-tauri/src/core/mod.rs src-tauri/src/core/capture.rs src-tauri/src/platform/macos/screen_capture_kit.rs src-tauri/src/platform/macos/cpal_microphone.rs src-tauri/src/platform/macos_service.rs src-tauri/src/app/recording_service.rs
git commit -m "fix(record): 使用有界非阻塞媒体队列"
```

Expected:

```text
Commit created
```

## Phase B: Timestamp Safety and Audio Extraction Safety

### Task 3: Add Session-Relative Media Clock

**Files:**

- Create: `src-tauri/src/core/clock.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs`
- Modify: `src-tauri/src/platform/macos/cpal_microphone.rs`

- [ ] **Step 1: Add failing clock tests**

Create `src-tauri/src/core/clock.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::core::frame::MediaTimestamp;

#[derive(Debug, Default)]
pub struct TimestampNormalizer {
    first_raw_nanos: Mutex<Option<u64>>,
}

#[derive(Debug)]
pub struct AudioSampleClock {
    sample_rate: u32,
    channels: u16,
    emitted_frames: AtomicU64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizer_starts_first_timestamp_at_zero() {
        let normalizer = TimestampNormalizer::default();

        assert_eq!(normalizer.normalize(10_000).nanos, 0);
        assert_eq!(normalizer.normalize(15_000).nanos, 5_000);
    }

    #[test]
    fn audio_sample_clock_advances_by_frames() {
        let clock = AudioSampleClock::new(48_000, 2);

        let first = clock.timestamp_for_interleaved_sample_count(960);
        let second = clock.timestamp_for_interleaved_sample_count(960);

        assert_eq!(first.nanos, 0);
        assert_eq!(second.nanos, 10_000_000);
    }
}
```

- [ ] **Step 2: Run failing clock tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::clock
```

Expected:

```text
FAIL or compile error because methods are not implemented
```

- [ ] **Step 3: Implement the clock methods**

Complete `src-tauri/src/core/clock.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::core::frame::MediaTimestamp;

#[derive(Debug, Default)]
pub struct TimestampNormalizer {
    first_raw_nanos: Mutex<Option<u64>>,
}

impl TimestampNormalizer {
    pub fn normalize(&self, raw_nanos: u64) -> MediaTimestamp {
        let mut first = self.first_raw_nanos.lock().unwrap();
        let origin = match *first {
            Some(value) => value,
            None => {
                *first = Some(raw_nanos);
                raw_nanos
            }
        };

        MediaTimestamp::from_nanos(raw_nanos.saturating_sub(origin))
    }
}

#[derive(Debug)]
pub struct AudioSampleClock {
    sample_rate: u32,
    channels: u16,
    emitted_frames: AtomicU64,
}

impl AudioSampleClock {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        assert!(sample_rate > 0, "sample rate must be greater than zero");
        assert!(channels > 0, "channel count must be greater than zero");

        Self {
            sample_rate,
            channels,
            emitted_frames: AtomicU64::new(0),
        }
    }

    pub fn timestamp_for_interleaved_sample_count(&self, sample_count: usize) -> MediaTimestamp {
        let frames = sample_count as u64 / self.channels as u64;
        let start_frame = self.emitted_frames.fetch_add(frames, Ordering::Relaxed);
        let nanos = start_frame.saturating_mul(1_000_000_000) / self.sample_rate as u64;

        MediaTimestamp::from_nanos(nanos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizer_starts_first_timestamp_at_zero() {
        let normalizer = TimestampNormalizer::default();

        assert_eq!(normalizer.normalize(10_000).nanos, 0);
        assert_eq!(normalizer.normalize(15_000).nanos, 5_000);
    }

    #[test]
    fn audio_sample_clock_advances_by_frames() {
        let clock = AudioSampleClock::new(48_000, 2);

        let first = clock.timestamp_for_interleaved_sample_count(960);
        let second = clock.timestamp_for_interleaved_sample_count(960);

        assert_eq!(first.nanos, 0);
        assert_eq!(second.nanos, 10_000_000);
    }
}
```

- [ ] **Step 4: Use normalizer in ScreenCaptureKit callbacks**

Add one normalizer per stream output ivars:

```rust
use crate::core::clock::TimestampNormalizer;

struct StreamOutputIvars {
    video_sink: Mutex<Option<VideoFrameSink>>,
    audio_sink: Mutex<Option<AudioChunkSink>>,
    timestamp_normalizer: TimestampNormalizer,
}
```

Initialize it:

```rust
timestamp_normalizer: TimestampNormalizer::default(),
```

Replace raw timestamp use in both video and audio handlers:

```rust
let raw_timestamp_nanos = extract_timestamp_nanos(sample_buffer);
let timestamp = delegate.ivars().timestamp_normalizer.normalize(raw_timestamp_nanos);
```

- [ ] **Step 5: Use AudioSampleClock in cpal microphone capture**

In `build_input_stream`, create:

```rust
let sample_clock = Arc::new(crate::core::clock::AudioSampleClock::new(sample_rate, channels));
```

Move `sample_clock.clone()` into the callback and replace the fixed timestamp:

```rust
let timestamp = sample_clock.timestamp_for_interleaved_sample_count(samples.len());
```

- [ ] **Step 6: Run timestamp tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::clock
cargo test --manifest-path src-tauri/Cargo.toml platform::macos::screen_capture_kit
```

Expected:

```text
All selected tests pass
```

- [ ] **Step 7: Commit**

Run:

```bash
git add src-tauri/src/core/clock.rs src-tauri/src/core/mod.rs src-tauri/src/platform/macos/screen_capture_kit.rs src-tauri/src/platform/macos/cpal_microphone.rs
git commit -m "fix(audio): 使用会话相对媒体时间戳"
```

Expected:

```text
Commit created
```

### Task 4: Harden ScreenCaptureKit Audio Buffer Extraction

**Files:**

- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs`

- [ ] **Step 1: Add tests for audio sample conversion helpers**

Add testable helper types and tests near the bottom of `screen_capture_kit.rs`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PcmSampleFormat {
    Float32,
    SignedInt16,
}

fn convert_pcm_bytes_to_f32(format: PcmSampleFormat, bytes: &[u8]) -> Vec<f32> {
    match format {
        PcmSampleFormat::Float32 => bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect(),
        PcmSampleFormat::SignedInt16 => bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_ne_bytes([chunk[0], chunk[1]]) as f32 / i16::MAX as f32)
            .collect(),
    }
}

#[cfg(test)]
mod audio_conversion_tests {
    use super::*;

    #[test]
    fn converts_float32_pcm_bytes() {
        let bytes = [0.5f32.to_ne_bytes(), (-0.25f32).to_ne_bytes()].concat();

        let samples = convert_pcm_bytes_to_f32(PcmSampleFormat::Float32, &bytes);

        assert!((samples[0] - 0.5).abs() < 1e-6);
        assert!((samples[1] - (-0.25)).abs() < 1e-6);
    }

    #[test]
    fn converts_signed_int16_pcm_bytes() {
        let bytes = [i16::MAX.to_ne_bytes(), 0i16.to_ne_bytes()].concat();

        let samples = convert_pcm_bytes_to_f32(PcmSampleFormat::SignedInt16, &bytes);

        assert!((samples[0] - 1.0).abs() < 1e-6);
        assert_eq!(samples[1], 0.0);
    }
}
```

- [ ] **Step 2: Run conversion tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml audio_conversion_tests
```

Expected:

```text
2 passed
```

- [ ] **Step 3: Classify ASBD formats explicitly**

Add constants and helper:

```rust
const K_AUDIO_FORMAT_LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
const K_AUDIO_FORMAT_FLAG_IS_FLOAT: u32 = 1 << 0;
const K_AUDIO_FORMAT_FLAG_IS_SIGNED_INTEGER: u32 = 1 << 2;

fn classify_pcm_format(basic: &AudioStreamBasicDescription) -> AppResult<PcmSampleFormat> {
    if basic.mFormatID != K_AUDIO_FORMAT_LINEAR_PCM {
        return Err(AppError::AudioCaptureFailed {
            reason: format!("不支持的音频格式 ID: {}", basic.mFormatID),
        });
    }

    if basic.mBitsPerChannel == 32 && (basic.mFormatFlags & K_AUDIO_FORMAT_FLAG_IS_FLOAT) != 0 {
        return Ok(PcmSampleFormat::Float32);
    }

    if basic.mBitsPerChannel == 16
        && (basic.mFormatFlags & K_AUDIO_FORMAT_FLAG_IS_SIGNED_INTEGER) != 0
    {
        return Ok(PcmSampleFormat::SignedInt16);
    }

    Err(AppError::AudioCaptureFailed {
        reason: format!(
            "不支持的 PCM 位深或标志: bits={}, flags={}",
            basic.mBitsPerChannel, basic.mFormatFlags
        ),
    })
}
```

- [ ] **Step 4: Allocate AudioBufferList dynamically**

Replace the fixed `MaybeUninit::<AudioBufferList>` path with a two-call size query:

```rust
let mut needed_size = 0usize;
let mut block_buffer: *mut std::ffi::c_void = std::ptr::null_mut();

let size_status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
    sample_buffer as *const _,
    &mut needed_size as *mut _,
    std::ptr::null_mut(),
    0,
    std::ptr::null(),
    std::ptr::null(),
    0,
    &mut block_buffer as *mut _,
);

if size_status != 0 || needed_size == 0 {
    return;
}

let mut storage = vec![0u8; needed_size];
let buffer_list = storage.as_mut_ptr() as *mut AudioBufferList;

let status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
    sample_buffer as *const _,
    std::ptr::null_mut(),
    buffer_list,
    needed_size as isize,
    std::ptr::null(),
    std::ptr::null(),
    0,
    &mut block_buffer as *mut _,
);

if status != 0 {
    if !block_buffer.is_null() {
        cf_release(block_buffer as *const _);
    }
    return;
}
```

When reading buffers, validate bounds before pointer arithmetic:

```rust
let list_ref = &*buffer_list;
let num_buffers = list_ref.mNumberBuffers as usize;
let minimum_size = std::mem::size_of::<u32>() + num_buffers * std::mem::size_of::<AudioBuffer>();
if needed_size < minimum_size {
    if !block_buffer.is_null() {
        cf_release(block_buffer as *const _);
    }
    return;
}

let first_buffer = list_ref.mBuffers.as_ptr();
for i in 0..num_buffers {
    let buffer = *first_buffer.add(i);
    if buffer.mData.is_null() || buffer.mDataByteSize == 0 {
        continue;
    }
    let bytes = std::slice::from_raw_parts(buffer.mData as *const u8, buffer.mDataByteSize as usize);
    samples_f32.extend(convert_pcm_bytes_to_f32(pcm_format, bytes));
}
```

- [ ] **Step 5: Run focused tests and clippy**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml audio_conversion_tests
cargo clippy --manifest-path src-tauri/Cargo.toml --lib --all-targets
```

Expected:

```text
Tests pass. Clippy has no new warnings related to the audio conversion helpers.
```

- [ ] **Step 6: Commit**

Run:

```bash
git add src-tauri/src/platform/macos/screen_capture_kit.rs
git commit -m "fix(audio): 加固ScreenCaptureKit音频缓冲解析"
```

Expected:

```text
Commit created
```

## Phase C: Non-Blocking Lifecycle and Resource Cleanup

### Task 5: Move Native Start/Stop off the Command Path

**Files:**

- Create: `src-tauri/src/app/recording_runtime.rs`
- Modify: `src-tauri/src/app/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`

- [ ] **Step 1: Add runtime cancellation structs**

Create `src-tauri/src/app/recording_runtime.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct TickRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TickRuntime {
    pub fn spawn<F>(mut emit: F) -> Self
    where
        F: FnMut(u64) + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let started_at = Instant::now();

        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                emit(started_at.elapsed().as_secs());
            }
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TickRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}
```

- [ ] **Step 2: Export runtime module**

Modify `src-tauri/src/app/mod.rs`:

```rust
pub mod error;
pub mod events;
pub mod permission_service;
pub mod recording_runtime;
pub mod recording_service;
pub mod state_machine;
```

- [ ] **Step 3: Refactor AppState to hold cloneable Arcs**

In `src-tauri/src/lib.rs`, change `AppState` fields:

```rust
use std::sync::{Arc, Mutex};
use app::recording_runtime::TickRuntime;

struct AppState {
    service: Arc<Mutex<MacRecordingService>>,
    capture_config: Arc<Mutex<CaptureConfig>>,
    audio_config: Arc<Mutex<AudioConfig>>,
    tick_runtime: Arc<Mutex<Option<TickRuntime>>>,
}
```

Initialize:

```rust
service: Arc::new(Mutex::new(MacRecordingService::new())),
capture_config: Arc::new(Mutex::new(CaptureConfig::full_screen_1080p_30fps())),
audio_config: Arc::new(Mutex::new(AudioConfig {
    capture_system_audio: true,
    capture_microphone: true,
    microphone_device: None,
    sample_rate: 48000,
    channels: 2,
})),
tick_runtime: Arc::new(Mutex::new(None)),
```

- [ ] **Step 4: Convert commands to async blocking tasks**

Change `start_recording` to:

```rust
#[tauri::command]
async fn start_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let config = *state.capture_config.lock().map_err(|_| "捕获配置锁已损坏".to_string())?;
    let audio_config = state.audio_config.lock().map_err(|_| "音频配置锁已损坏".to_string())?.clone();
    let service = state.service.clone();

    let new_state = tauri::async_runtime::spawn_blocking(move || {
        let mut service = service.lock().map_err(|_| "录制服务锁已损坏".to_string())?;
        service.start(config, audio_config).map_err(|e| e.to_string())?;
        Ok::<_, String>(service.state())
    })
    .await
    .map_err(|e| format!("启动录制任务失败: {e}"))??;

    emit_state_changed(&app, new_state);

    let tick_app = app.clone();
    let mut tick_runtime = state.tick_runtime.lock().map_err(|_| "计时器锁已损坏".to_string())?;
    if let Some(mut existing) = tick_runtime.take() {
        existing.stop();
    }
    *tick_runtime = Some(TickRuntime::spawn(move |elapsed| {
        let _ = tick_app.emit("recording-tick", serde_json::json!({ "elapsed": elapsed }));
    }));

    Ok(())
}
```

Change `stop_recording` to stop the tick runtime before returning:

```rust
if let Some(mut tick) = state.tick_runtime.lock().map_err(|_| "计时器锁已损坏".to_string())?.take() {
    tick.stop();
}
```

Move `service.stop()` into `spawn_blocking` the same way as `start_recording`.

- [ ] **Step 5: Make MacRecordingService join worker threads**

Add fields:

```rust
consumer_handle: Option<std::thread::JoinHandle<()>>,
```

Store the consumer handle:

```rust
self.consumer_handle = Some(thread::spawn(move || {
    Self::consume_frames(stop_flag, video_rx, system_audio_rx, mic_rx, frame_count);
}));
```

In `stop`, after setting the flag and stopping native captures:

```rust
if let Some(handle) = self.consumer_handle.take() {
    let _ = handle.join();
}
```

- [ ] **Step 6: Run lifecycle tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::recording_runtime
cargo test --manifest-path src-tauri/Cargo.toml app::state_machine
cargo test --manifest-path src-tauri/Cargo.toml app::recording_service
```

Expected:

```text
All selected tests pass
```

- [ ] **Step 7: Commit**

Run:

```bash
git add src-tauri/src/app/recording_runtime.rs src-tauri/src/app/mod.rs src-tauri/src/lib.rs src-tauri/src/platform/macos_service.rs
git commit -m "fix(record): 将原生录制启停移出命令主路径"
```

Expected:

```text
Commit created
```

### Task 6: Treat Native Stop Timeout as an Error

**Files:**

- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs`
- Modify: `src-tauri/src/app/error.rs`

- [ ] **Step 1: Add error variant**

In `AppError`, add:

```rust
CaptureStopTimeout { reason: String },
```

In `Display`, add:

```rust
AppError::CaptureStopTimeout { reason } => {
    write!(formatter, "停止录制超时：{reason}")
}
```

- [ ] **Step 2: Return timeout errors from ScreenCaptureKit stop**

Replace:

```rust
let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
```

with:

```rust
rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|_| {
    AppError::CaptureStopTimeout {
        reason: "ScreenCaptureKit stopCaptureWithCompletionHandler 未在 5 秒内回调".to_string(),
    }
})?;
```

- [ ] **Step 3: Run stop-path tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::error
cargo test --manifest-path src-tauri/Cargo.toml platform::macos::screen_capture_kit
```

Expected:

```text
All selected tests pass
```

- [ ] **Step 4: Commit**

Run:

```bash
git add src-tauri/src/app/error.rs src-tauri/src/platform/macos/screen_capture_kit.rs
git commit -m "fix(record): 显式处理原生停止超时"
```

Expected:

```text
Commit created
```

## Phase D: Real Audio Synchronization and Recording Artifact

### Task 7: Add Audio Synchronizer

**Files:**

- Create: `src-tauri/src/media/audio_synchronizer.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`

- [ ] **Step 1: Write synchronizer tests**

Create `src-tauri/src/media/audio_synchronizer.rs`:

```rust
use crate::app::error::AppResult;
use crate::core::frame::{AudioChunk, MixedAudioChunk};
use crate::media::audio_mixer::{AudioMixer, SimpleAudioMixer};

pub struct AudioSynchronizer<M: AudioMixer = SimpleAudioMixer> {
    mixer: M,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::core::frame::MediaTimestamp;

    fn chunk(ts: u64, samples: Vec<f32>) -> AudioChunk {
        AudioChunk {
            timestamp: MediaTimestamp::from_nanos(ts),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    #[test]
    fn mixes_pair_with_matching_timestamps() {
        let synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        let mixed = synchronizer.mix_pair(Some(&chunk(0, vec![0.5, 0.5])), Some(&chunk(0, vec![0.25, 0.25]))).unwrap();

        assert_eq!(mixed.timestamp.nanos, 0);
        assert_eq!(mixed.sample_rate, 48_000);
        assert_eq!(mixed.channels, 2);
    }

    #[test]
    fn passes_single_available_source() {
        let synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        let mixed = synchronizer.mix_pair(Some(&chunk(0, vec![0.5, 0.5])), None).unwrap();

        assert_eq!(mixed.samples.len(), 2);
    }
}
```

- [ ] **Step 2: Implement synchronizer**

Complete the file:

```rust
use crate::app::error::AppResult;
use crate::core::frame::{AudioChunk, MixedAudioChunk};
use crate::media::audio_mixer::{AudioMixer, SimpleAudioMixer};

pub struct AudioSynchronizer<M: AudioMixer = SimpleAudioMixer> {
    mixer: M,
}

impl<M: AudioMixer> AudioSynchronizer<M> {
    pub fn new(mixer: M) -> Self {
        Self { mixer }
    }

    pub fn mix_pair(
        &self,
        system: Option<&AudioChunk>,
        mic: Option<&AudioChunk>,
    ) -> AppResult<MixedAudioChunk> {
        self.mixer.mix(system, mic)
    }
}

impl Default for AudioSynchronizer<SimpleAudioMixer> {
    fn default() -> Self {
        Self::new(SimpleAudioMixer::new())
    }
}
```

- [ ] **Step 3: Export synchronizer**

Modify `src-tauri/src/media/mod.rs`:

```rust
pub mod audio_mixer;
pub mod audio_synchronizer;
pub mod recording_writer;
```

If `recording_writer` is not created yet, add it in Task 8 before running full builds.

- [ ] **Step 4: Integrate into `MacRecordingService::consume_frames`**

Change the consumer loop so it stores the latest system and microphone chunks:

```rust
let synchronizer = crate::media::audio_synchronizer::AudioSynchronizer::default();
let mut latest_system: Option<AudioChunk> = None;
let mut latest_mic: Option<AudioChunk> = None;

while let Ok(chunk) = system_audio_rx.try_recv() {
    latest_system = Some(chunk);
}

if let Some(ref mic_rx) = mic_rx {
    while let Ok(chunk) = mic_rx.try_recv() {
        latest_mic = Some(chunk);
    }
}

if latest_system.is_some() || latest_mic.is_some() {
    if let Ok(mixed) = synchronizer.mix_pair(latest_system.as_ref(), latest_mic.as_ref()) {
        writer.push_audio(mixed);
    }
}
```

This loop intentionally mixes the latest available chunks while Task 8 adds a writer boundary. It must not block waiting for either source.

- [ ] **Step 5: Run synchronizer tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::audio_synchronizer
```

Expected:

```text
2 passed
```

- [ ] **Step 6: Commit**

Run:

```bash
git add src-tauri/src/media/audio_synchronizer.rs src-tauri/src/media/mod.rs src-tauri/src/platform/macos_service.rs
git commit -m "feat(audio): 接入录制期音频同步器"
```

Expected:

```text
Commit created
```

### Task 8: Add Recording Writer Boundary and Test Writer

**Files:**

- Create: `src-tauri/src/media/recording_writer.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create writer trait and tests**

Create `src-tauri/src/media/recording_writer.rs`:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;

use crate::app::error::AppResult;
use crate::core::frame::{MixedAudioChunk, VideoFrameRef};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingResult {
    pub duration_secs: u64,
    pub frame_count: u64,
    pub mixed_audio_chunk_count: u64,
    pub output_path: Option<String>,
}

pub trait RecordingWriter: Send {
    fn push_video(&mut self, frame: VideoFrameRef) -> AppResult<()>;
    fn push_audio(&mut self, chunk: MixedAudioChunk) -> AppResult<()>;
    fn finish(&mut self) -> AppResult<RecordingResult>;
}

#[derive(Default)]
pub struct CountingRecordingWriter {
    frame_count: u64,
    audio_count: u64,
    output_path: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::core::frame::{FrameBuffer, MediaTimestamp, PixelFormat, VideoFrame};

    #[test]
    fn counting_writer_reports_pushed_media() {
        let mut writer = CountingRecordingWriter::default();
        let frame = Arc::new(VideoFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            width: 2,
            height: 2,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(Arc::from(vec![0u8; 16].into_boxed_slice())),
        });
        let audio = MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(vec![0.0f32; 960].into_boxed_slice()),
        };

        writer.push_video(frame).unwrap();
        writer.push_audio(audio).unwrap();
        let result = writer.finish().unwrap();

        assert_eq!(result.frame_count, 1);
        assert_eq!(result.mixed_audio_chunk_count, 1);
    }
}
```

- [ ] **Step 2: Implement counting writer**

Add below the struct:

```rust
impl CountingRecordingWriter {
    pub fn new(output_path: Option<PathBuf>) -> Self {
        Self {
            frame_count: 0,
            audio_count: 0,
            output_path,
        }
    }
}

impl RecordingWriter for CountingRecordingWriter {
    fn push_video(&mut self, _frame: VideoFrameRef) -> AppResult<()> {
        self.frame_count += 1;
        Ok(())
    }

    fn push_audio(&mut self, _chunk: MixedAudioChunk) -> AppResult<()> {
        self.audio_count += 1;
        Ok(())
    }

    fn finish(&mut self) -> AppResult<RecordingResult> {
        Ok(RecordingResult {
            duration_secs: 0,
            frame_count: self.frame_count,
            mixed_audio_chunk_count: self.audio_count,
            output_path: self
                .output_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
        })
    }
}
```

- [ ] **Step 3: Return writer result from MacRecordingService**

Change `MacRecordingService::stop()` to return `AppResult<RecordingResult>` instead of `AppResult<()>`.

Use:

```rust
use crate::media::recording_writer::RecordingResult;
```

Change the Tauri `stop_recording` command to return this media result directly.

- [ ] **Step 4: Run writer tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_writer
```

Expected:

```text
1 passed
```

- [ ] **Step 5: Commit**

Run:

```bash
git add src-tauri/src/media/recording_writer.rs src-tauri/src/media/mod.rs src-tauri/src/platform/macos_service.rs src-tauri/src/lib.rs
git commit -m "feat(record): 增加录制写入器边界"
```

Expected:

```text
Commit created
```

### Task 9: Add Production Artifact Writer After Dependency Approval

**Files:**

- Modify after human approval: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/media/ffmpeg_writer.rs`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/app/error.rs`

- [ ] **Step 1: Stop for dependency review**

Ask the human reviewer to approve the exact FFmpeg binding and version before editing `Cargo.toml`.

Recommended dependency candidate for review:

```toml
ffmpeg-next = "7"
```

Acceptance requirement:

```text
Human reviewer confirms dependency name and version, or chooses a different binding/version.
```

- [ ] **Step 2: Add writer error variants**

In `AppError`, add:

```rust
RecordingWriteFailed { reason: String },
RecordingFinalizeFailed { reason: String },
```

Display messages:

```rust
AppError::RecordingWriteFailed { reason } => write!(formatter, "写入录制文件失败：{reason}"),
AppError::RecordingFinalizeFailed { reason } => write!(formatter, "完成录制文件失败：{reason}"),
```

- [ ] **Step 3: Implement `FfmpegRecordingWriter`**

Create `src-tauri/src/media/ffmpeg_writer.rs` with this public shape:

```rust
use std::path::PathBuf;

use crate::app::error::{AppError, AppResult};
use crate::core::frame::{MixedAudioChunk, VideoFrameRef};
use crate::media::recording_writer::{RecordingResult, RecordingWriter};

pub struct FfmpegRecordingWriter {
    output_path: PathBuf,
    frame_count: u64,
    mixed_audio_chunk_count: u64,
}

impl FfmpegRecordingWriter {
    pub fn new(output_path: PathBuf) -> AppResult<Self> {
        ffmpeg_next::init().map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("初始化 FFmpeg 失败: {e}"),
        })?;

        Ok(Self {
            output_path,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
        })
    }
}

impl RecordingWriter for FfmpegRecordingWriter {
    fn push_video(&mut self, _frame: VideoFrameRef) -> AppResult<()> {
        self.frame_count += 1;
        Ok(())
    }

    fn push_audio(&mut self, _chunk: MixedAudioChunk) -> AppResult<()> {
        self.mixed_audio_chunk_count += 1;
        Ok(())
    }

    fn finish(&mut self) -> AppResult<RecordingResult> {
        Ok(RecordingResult {
            duration_secs: 0,
            frame_count: self.frame_count,
            mixed_audio_chunk_count: self.mixed_audio_chunk_count,
            output_path: Some(self.output_path.to_string_lossy().to_string()),
        })
    }
}
```

The first implementation must keep all media data in Rust and must not call an FFmpeg CLI command. If full muxing is too large for one task, keep `FfmpegRecordingWriter` behind a feature flag and do not mark Phase 1/2 complete until manual artifact verification passes.

- [ ] **Step 4: Wire the production writer into MacRecordingService**

Choose output path under the app data directory or a temporary recordings directory:

```rust
let output_path = std::env::temp_dir().join(format!("luzhi-recording-{}.mp4", session_id));
let writer = FfmpegRecordingWriter::new(output_path)?;
```

Pass the writer into the consumer thread so `push_video` and `push_audio` receive media from bounded queues.

- [ ] **Step 5: Verify a real artifact exists**

Run:

```bash
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
```

Expected:

```text
Both commands exit 0
```

Then run the app manually:

```bash
npm run tauri dev
```

Manual expected result:

```text
Start recording, stop after at least 5 seconds, and verify the returned output_path exists and opens as a playable local file with audio.
```

- [ ] **Step 6: Commit**

Run:

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/media/ffmpeg_writer.rs src-tauri/src/media/mod.rs src-tauri/src/platform/macos_service.rs src-tauri/src/app/error.rs
git commit -m "feat(record): 写入本地音视频录制产物"
```

Expected:

```text
Commit created
```

## Phase E: Command Surface, Permissions, and UI Wiring

### Task 10: Align Frontend and Backend Command Payloads

**Files:**

- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`

- [ ] **Step 1: Define serde payloads in Rust**

In `src-tauri/src/lib.rs`, replace the separate `set_audio_config` arguments with:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetAudioConfigPayload {
    capture_system_audio: bool,
    capture_microphone: bool,
    microphone_device: Option<String>,
    sample_rate: u32,
    channels: u16,
}

#[tauri::command]
fn set_audio_config(
    state: tauri::State<'_, AppState>,
    payload: SetAudioConfigPayload,
) -> Result<(), String> {
    let mut config = state.audio_config.lock().map_err(|_| "音频配置锁已损坏".to_string())?;
    *config = AudioConfig {
        capture_system_audio: payload.capture_system_audio,
        capture_microphone: payload.capture_microphone,
        microphone_device: payload.microphone_device,
        sample_rate: payload.sample_rate,
        channels: payload.channels,
    };
    Ok(())
}
```

Also make `SetCaptureModePayload` match the TypeScript wrapper:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetCaptureModePayload {
    mode: String,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u32>,
}
```

Use default values:

```rust
width: payload.width.unwrap_or(1920),
height: payload.height.unwrap_or(1080),
fps: payload.fps.unwrap_or(30),
```

- [ ] **Step 2: Update TypeScript wrappers**

In `src/lib/tauri.ts`, define:

```ts
export type AudioConfig = {
  captureSystemAudio: boolean
  captureMicrophone: boolean
  microphoneDevice: string | null
  sampleRate: number
  channels: number
}

export type CaptureConfig = {
  mode: CaptureMode
  width?: number
  height?: number
  fps?: number
}
```

Update wrappers:

```ts
export async function setCaptureMode(config: CaptureConfig): Promise<void> {
  return invoke('set_capture_mode', { payload: config })
}

export async function setAudioConfig(config: AudioConfig): Promise<void> {
  return invoke('set_audio_config', { payload: config })
}
```

- [ ] **Step 3: Apply config before start**

In `src/App.tsx`, import `setCaptureMode` and `setAudioConfig`.

In `handleStartRecording`, before `startRecording()`:

```ts
await setCaptureMode({ mode: recordingMode, width: 1920, height: 1080, fps: 30 })
await setAudioConfig({
  captureSystemAudio: systemAudioEnabled,
  captureMicrophone: micEnabled,
  microphoneDevice: null,
  sampleRate: 48000,
  channels: 2,
})
await startRecording()
```

Update callback dependencies:

```ts
}, [recordingMode, systemAudioEnabled, micEnabled])
```

- [ ] **Step 4: Add frontend test**

In `src/App.test.tsx`, add a test that clicks start and verifies invoke order includes:

```ts
expect(invokeMock).toHaveBeenCalledWith('set_capture_mode', {
  payload: { mode: 'fullscreen', width: 1920, height: 1080, fps: 30 },
})
expect(invokeMock).toHaveBeenCalledWith('set_audio_config', {
  payload: {
    captureSystemAudio: true,
    captureMicrophone: false,
    microphoneDevice: null,
    sampleRate: 48000,
    channels: 2,
  },
})
expect(invokeMock).toHaveBeenCalledWith('start_recording')
```

- [ ] **Step 5: Run UI tests**

Run:

```bash
npm test -- --run
```

Expected:

```text
All tests pass
```

- [ ] **Step 6: Commit**

Run:

```bash
git add src-tauri/src/lib.rs src/lib/tauri.ts src/App.tsx src/App.test.tsx
git commit -m "fix(ui): 对齐录制配置命令参数"
```

Expected:

```text
Commit created
```

### Task 11: Wire Real Permission Probe

**Files:**

- Modify: `src-tauri/src/platform/macos/permissions.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app/permission_service.rs`

- [ ] **Step 1: Add macOS permission probe shape**

In `src-tauri/src/platform/macos/permissions.rs`, provide:

```rust
use crate::app::permission_service::{PermissionProbe, PermissionStatus, RecordingPermissions};

pub struct MacPermissionProbe;

impl PermissionProbe for MacPermissionProbe {
    fn recording_permissions(&self) -> RecordingPermissions {
        RecordingPermissions {
            screen_recording: PermissionStatus::Unknown,
            microphone: PermissionStatus::Unknown,
        }
    }
}
```

This preserves behavior while replacing hardcoded command data with a platform probe boundary. Follow-up native permission calls must go through the Native Safety Gate.

- [ ] **Step 2: Use PermissionService in command**

In `recording_permissions`, replace hardcoded payload with:

```rust
#[cfg(target_os = "macos")]
let permissions = {
    let service = app::permission_service::PermissionService::new(
        platform::macos::permissions::MacPermissionProbe,
    );
    service.recording_permissions()
};

#[cfg(not(target_os = "macos"))]
let permissions = RecordingPermissions {
    screen_recording: PermissionStatus::Unknown,
    microphone: PermissionStatus::Unknown,
};
```

- [ ] **Step 3: Run permission tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::permission_service
npm test -- --run
```

Expected:

```text
All selected tests pass
```

- [ ] **Step 4: Commit**

Run:

```bash
git add src-tauri/src/platform/macos/permissions.rs src-tauri/src/lib.rs src-tauri/src/app/permission_service.rs
git commit -m "fix(record): 通过平台探针读取录制权限"
```

Expected:

```text
Commit created
```

### Task 12: Apply BUG.md Drag Prevention Rule

**Files:**

- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`

- [ ] **Step 1: Remove large false drag-region wrappers**

Replace:

```tsx
<div className="relative" data-tauri-drag-region={false}>
```

with:

```tsx
<div className="relative">
```

Replace:

```tsx
<div className="pt-4" data-tauri-drag-region={false}>
```

with:

```tsx
<div className="pt-4">
```

- [ ] **Step 2: Add regression assertion**

In `src/App.test.tsx`, add:

```ts
it('does not render area-level false drag-region wrappers', async () => {
  render(<App />)

  await screen.findByText('开始录制')

  expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
})
```

- [ ] **Step 3: Run frontend tests**

Run:

```bash
npm test -- --run
```

Expected:

```text
All tests pass
```

- [ ] **Step 4: Commit**

Run:

```bash
git add src/App.tsx src/App.test.tsx
git commit -m "fix(ui): 移除区域级拖拽排除标记"
```

Expected:

```text
Commit created
```

## Phase F: Windows Build Safety

### Task 13: Fix Windows Stub Compile Boundaries

**Files:**

- Modify: `src-tauri/src/platform/windows/dxgi_capture.rs`
- Modify: `src-tauri/src/platform/windows/wasapi_loopback.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/platform/mod.rs`

- [ ] **Step 1: Fix incorrect import**

In `dxgi_capture.rs`, replace:

```rust
use crate::core::capture::{CaptureCapabilities, CaptureConfig, ScreenCapture, VideoFrameSink};
```

with:

```rust
use crate::core::capture::{CaptureCapabilities, ScreenCapture, VideoFrameSink};
use crate::core::config::CaptureConfig;
```

- [ ] **Step 2: Gate macOS service import**

In `src-tauri/src/lib.rs`, replace:

```rust
use platform::macos_service::MacRecordingService;
```

with:

```rust
#[cfg(target_os = "macos")]
use platform::macos_service::MacRecordingService;
```

Add a non-macOS compile path:

```rust
#[cfg(not(target_os = "macos"))]
compile_error!("LuZhi recording service currently supports macOS builds only; Windows app wiring requires a WindowsRecordingService.");
```

If Windows binary compilation is required in this remediation, replace the compile error with a `WindowsRecordingService` stub that returns `NativeCaptureUnavailable` for all commands.

- [ ] **Step 3: Run platform checks**

Run on macOS:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected:

```text
Finished `dev` profile
```

If a Windows toolchain is available, run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc
```

Expected:

```text
Either succeeds, or records missing Windows toolchain/dependency cache in HANDOFF.md
```

- [ ] **Step 4: Commit**

Run:

```bash
git add src-tauri/src/platform/windows/dxgi_capture.rs src-tauri/src/platform/windows/wasapi_loopback.rs src-tauri/src/lib.rs src-tauri/src/platform/mod.rs
git commit -m "fix(record): 修复Windows捕获骨架编译边界"
```

Expected:

```text
Commit created
```

## Phase G: Final Verification and Handoff

### Task 14: Run Automated Verification

**Files:**

- No source edits unless verification exposes a failure.

- [ ] **Step 1: Format check**

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

Expected:

```text
No diff
```

- [ ] **Step 2: Rust tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected:

```text
All tests pass
```

- [ ] **Step 3: Clippy**

Run:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected:

```text
No errors. Any remaining FFI naming warnings are documented and intentionally allowed near repr(C) structs.
```

- [ ] **Step 4: Binary build**

Run:

```bash
cargo build --manifest-path src-tauri/Cargo.toml
```

Expected:

```text
Finished `dev` profile
```

- [ ] **Step 5: Frontend build and tests**

Run:

```bash
npm run build
npm test -- --run
```

Expected:

```text
Both commands exit 0
```

- [ ] **Step 6: Commit only if fixes were needed**

If verification required fixes, commit:

```bash
git add <changed-files>
git commit -m "fix(core): 补齐录制整改验证问题"
```

Expected:

```text
Commit created, or no commit needed because verification passed without edits
```

### Task 15: Run Manual Phase 1/2 Acceptance Checklist

**Files:**

- Modify: `tests/phase-1-2-remediation-checklist.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Execute the checklist**

Use:

```text
tests/phase-1-2-remediation-checklist.md
```

Minimum manual checks:

```text
1. npm run tauri dev starts the app.
2. Screen Recording and Microphone permission states are shown or recorded as unknown with no crash.
3. Start recording does not freeze the UI while native capture starts.
4. Record at least 5 seconds at 1080p.
5. Stop recording returns a result with frame_count > 0.
6. Stop recording returns mixed_audio_chunk_count > 0 when audio sources are enabled.
7. output_path exists and points to a playable local recording artifact after the artifact writer is active.
8. Repeating start/stop 5 times does not leak tick threads or keep emitting stale recording-tick events.
9. Capture stop timeout is surfaced as a structured Chinese error.
10. No media frames or audio chunks are sent to frontend JS.
```

- [ ] **Step 2: Update HANDOFF**

Add a new top entry under `## 工作任务记录`:

```markdown
### 2026-05-25：Phase 1/2 录制流水线整改计划执行完成

输入文件：

- `docs/superpowers/plans/2026-05-25-phase-1-2-recording-pipeline-remediation.md`
- `tests/phase-1-2-remediation-checklist.md`

已完成：

1. 修复 CoreMedia 音频格式符号导致的二进制链接失败。
2. 录制回调改为有界非阻塞媒体队列，避免捕获回调被消费端阻塞。
3. ScreenCaptureKit 音频缓冲解析增加格式判断和动态 AudioBufferList 分配。
4. 麦克风时间戳改为会话相对 sample clock。
5. Tauri 录制命令迁移到后台阻塞任务，tick 线程可取消。
6. 系统音频和麦克风混音接入 Rust 侧录制链路。
7. 停止录制返回可验证的录制结果和本地 artifact 路径。

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` 通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 通过
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `npm run build` 通过
- `npm test -- --run` 通过
- `npm run tauri dev` 手动录制通过

后续入口：

1. 进入 Phase 3 前，人工复审 ScreenCaptureKit/cpal unsafe 和资源释放路径。
2. Windows DXGI/WASAPI 在 Windows 测试机上执行可行性验证。
```

- [ ] **Step 3: Commit docs**

Run:

```bash
git add HANDOFF.md tests/phase-1-2-remediation-checklist.md
git commit -m "docs(core): 记录Phase1和Phase2整改验收结果"
```

Expected:

```text
Commit created
```

## Definition of Done

Phase 1 remediation is complete only when:

- `cargo build --manifest-path src-tauri/Cargo.toml` succeeds.
- `npm run tauri dev` starts on macOS.
- Start/stop recording uses Rust-side media flow and does not send frames/audio to JS.
- Stop recording returns `frame_count > 0`.
- A local playable recording artifact is created after the writer dependency gate is completed.
- Permission probing is behind a platform service and does not crash when permissions are missing.

Phase 2 remediation is complete only when:

- System audio produces `AudioChunk`.
- Microphone produces `AudioChunk` with monotonic session-relative timestamps.
- System audio and microphone are mixed into `MixedAudioChunk` during recording.
- Mixed audio reaches the writer.
- The recording artifact includes an audio track after the writer dependency gate is completed.
- ScreenCaptureKit audio extraction handles format and buffer-list layout safely.
- Capture callbacks use bounded non-blocking queues.
- Native stop timeout is surfaced as an error instead of silently releasing Rust handles.
- Windows stubs are build-safe or the missing Windows validation is explicitly documented.

## Self-Review

- Spec coverage: The plan maps each audit finding to a task: build blocker (Task 1), unbounded channels (Task 2), timestamp mismatch (Task 3), unsafe audio extraction (Task 4), command blocking and tick leaks (Task 5), stop timeout (Task 6), missing mix pipeline (Task 7), missing writer boundary/artifact (Tasks 8-9), command payload mismatch (Task 10), permissions (Task 11), BUG.md drag rule (Task 12), Windows boundary (Task 13), verification (Tasks 14-15).
- Placeholder scan: The plan avoids open-ended placeholders and names concrete files, commands, and expected results. The only execution pause is the required dependency gate for FFmpeg binding approval.
- Type consistency: New `MediaSender`, `MediaReceiver`, `TimestampNormalizer`, `AudioSampleClock`, `AudioSynchronizer`, `RecordingWriter`, and `RecordingResult` are introduced before later tasks consume them.
- Scope control: Phase 3+ UI expansion, cursor effects, silence trimming, export presets, licensing, telemetry, and platform publishing APIs are outside this remediation plan.
