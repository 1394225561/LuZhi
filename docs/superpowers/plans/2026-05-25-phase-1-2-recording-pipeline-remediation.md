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

## 2026-05-25 Round 2 Code Review Findings

> Review input: first remediation implementation after `tests/phase-1-2-remediation-checklist.md` was partially checked.
> Review focus: Phase 1/2 task completeness, Phase 2 capture hot path blocking risk, memory safety, thread safety, and resource release paths.

### Fresh Verification Evidence

Commands run during this review:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc
```

Observed results:

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` exited 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` passed `43` Rust tests.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` exited 0, but still emitted warnings in the macOS FFI files.
- `cargo build --manifest-path src-tauri/Cargo.toml` exited 0, but still emitted warnings in the macOS FFI files.
- `npm run build` exited 0.
- `npm test -- --run` passed `6` Vitest tests.
- `cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc` did not reach code checking on this machine because the Rust target is not installed:

```text
error[E0463]: can't find crate for `std`
= note: the `x86_64-pc-windows-msvc` target may not be installed
```

### Completion Judgment

Phase 1 status: **partially complete, not accepted yet**.

- Completed: macOS binary build blocker fixed, Tauri command path moved to `spawn_blocking`, bounded queues are present, UI can call Rust commands, Rust-side media flow does not send frames/audio to frontend JS.
- Not complete: no playable local recording artifact is produced, `stop_recording` is not proven to return `frame_count > 0`, frontend does not correctly read the returned recording result shape, and real permission probing remains a placeholder.

Phase 2 status: **incomplete**.

- Completed: audio data types, cpal microphone capture, ScreenCaptureKit system audio extraction, `SimpleAudioMixer`, `AudioSynchronizer`, and bounded audio queues exist.
- Not complete: mixed audio is not delivered to a writer, `mixed_audio_chunk_count` is always `0`, the system-audio toggle is ignored by the backend, timestamp pairing is not continuous, and the native audio callback path still contains avoidable locking.

### Blocking Findings

#### R2-001 Critical: Recording writer boundary exists but is not wired into the real recording pipeline

Evidence:

- `src-tauri/src/platform/macos_service.rs`
  - `consume_frames()` counts video frames but discards the actual `VideoFrameRef`.
  - `consume_frames()` creates mixed audio but discards the `MixedAudioChunk`.
  - `stop()` returns `mixed_audio_chunk_count: 0` and `output_path: None`.
- `src-tauri/src/media/ffmpeg_writer.rs`
  - `FfmpegRecordingWriter` is only a feature-gated skeleton and does not encode or mux media.

Impact:

- Phase 1 cannot satisfy "录制结束后产出可播放视频文件".
- Phase 2 cannot satisfy "Mixed audio reaches the writer" or "录制文件包含音频轨道".
- Manual checklist items for `frame_count > 0`, `mixed_audio_chunk_count > 0`, and playable `output_path` remain unaccepted.

Required remediation:

- Pass a `RecordingWriter` into the consumer thread.
- Call `writer.push_video(frame)` for every drained video frame.
- Call `writer.push_audio(mixed)` for every mixed audio chunk.
- Return `writer.finish()` from `MacRecordingService::stop()`.
- For non-FFmpeg builds, use `CountingRecordingWriter` only as an explicit development fallback and make the returned `output_path` remain `None`.
- Do not mark artifact acceptance complete until a real writer creates a playable file with an audio track.

Verification:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_writer
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

Manual acceptance after real writer is enabled:

```text
1. Start a 1080p recording for at least 5 seconds.
2. Stop recording.
3. Confirm returned frameCount > 0.
4. Confirm returned mixedAudioChunkCount > 0 when audio is enabled.
5. Confirm returned outputPath exists.
6. Confirm the outputPath opens as a playable local file with video and audio tracks.
```

#### R2-002 Important: Frontend and backend recording result field names are mismatched

Evidence:

- `src-tauri/src/media/recording_writer.rs` derives `#[serde(rename_all = "camelCase")]` for `RecordingResult`.
- `src/lib/tauri.ts` defines `RecordingResult` with snake_case fields: `duration_secs`, `frame_count`, `mixed_audio_chunk_count`, `output_path`.
- `src/App.tsx` reads snake_case fields from `stopRecording()` result.

Impact:

- Once `stop_recording` returns a result, the frontend will read `undefined` for duration, frame count, mixed audio count, and output path.
- Preview UI cannot reliably display recording statistics or load the produced artifact.

Required remediation:

- Change TypeScript result shape to camelCase:

```ts
export type RecordingResult = {
  durationSecs: number
  frameCount: number
  mixedAudioChunkCount: number
  outputPath: string | null
}
```

- Change `handleStopRecording` in `src/App.tsx` to read camelCase:

```ts
setRecordingResult({
  durationSecs: result.durationSecs,
  frameCount: result.frameCount,
  mixedAudioChunkCount: result.mixedAudioChunkCount,
  outputPath: result.outputPath ?? null,
})
```

- Add a Vitest case where `stop_recording` resolves with camelCase fields and the preview displays the returned frame count.

Verification:

```bash
npm test -- --run
npm run build
```

#### R2-003 Important: Backend ignores `captureSystemAudio`

Evidence:

- `src/App.tsx` sends `captureSystemAudio` through `set_audio_config`.
- `src-tauri/src/lib.rs` stores `capture_system_audio` in `AudioConfig`.
- `src-tauri/src/platform/macos_service.rs` always creates the system audio channel and always calls `MacScreenCapture::start_combined(config, video_sender, audio_sender)`.
- `src-tauri/src/platform/macos/screen_capture_kit.rs` always calls `stream_config.setCapturesAudio(true)`.

Impact:

- User disabling system audio in the UI still causes ScreenCaptureKit system audio capture to be requested.
- Permission prompts and capture behavior can diverge from UI state.

Required remediation:

- Thread `capture_system_audio` into the `MacScreenCapture` start path.
- Set `stream_config.setCapturesAudio(audio_config.capture_system_audio)`.
- If system audio is disabled, either avoid adding `SCStreamOutputType::Audio` or attach a sink that is intentionally unused and cannot affect result counters.
- Add a unit test or adapter-level test seam proving disabled system audio does not push system chunks.

Verification:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

Manual acceptance:

```text
1. Disable system audio in the UI.
2. Start and stop recording.
3. Confirm no system audio chunks are counted or mixed.
4. Enable system audio and repeat.
5. Confirm system audio chunks can contribute to mixedAudioChunkCount.
```

#### R2-004 Important: Audio synchronizer is not a continuous timestamp pairing stage

Evidence:

- `src-tauri/src/platform/macos_service.rs::consume_frames()` drains all available system audio but keeps only the latest chunk.
- The same loop drains all available microphone audio but keeps only the latest chunk.
- It then mixes only one latest system chunk and one latest microphone chunk per 10 ms loop.
- `AudioSampleClock` starts microphone timestamps at zero independently from ScreenCaptureKit's first sample timestamp.

Impact:

- Audio chunks can be dropped before reaching the mixer even when queues are healthy.
- Mixed audio can contain gaps or mismatched pairs.
- "timestamp-based alignment" exists at the mixer level but is not yet implemented as a real stream synchronization stage.

Required remediation:

- Replace latest-only draining with ordered audio queues inside the consumer thread.
- Pair chunks by timestamp window and mix each pair exactly once.
- Preserve single-source chunks as mixed output when only one enabled source is available.
- Track `mixed_audio_chunk_count` from actual writer pushes.
- Add tests covering:
  - system-only recording,
  - mic-only recording,
  - both sources with matching timestamps,
  - both sources with offset timestamps,
  - no duplicate mixing of the same chunk.

Verification:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::audio_synchronizer
cargo test --manifest-path src-tauri/Cargo.toml
```

#### R2-005 Important: cpal audio callback still locks mutexes on the hot path

Evidence:

- `src-tauri/src/platform/macos/cpal_microphone.rs::build_input_stream()` locks `running` on every audio callback.
- The same callback locks `sink` before sending.
- `CpalMicrophoneCapture::start()` and `stop()` lock the same `running` state.

Impact:

- The cpal audio IO callback can block on a mutex, increasing underrun/dropout risk.
- This conflicts with the Phase 2 review focus on avoiding capture hot path blocking.

Required remediation:

- Replace `Arc<Mutex<bool>>` with `Arc<AtomicBool>` for `running`.
- Remove `Arc<Mutex<Option<AudioChunkSink>>>` around the sink; move a cloned `MediaSender<AudioChunk>` directly into the callback.
- Keep `try_send_drop_newest()` as the only channel operation in the callback.
- Do not log synchronously from the callback beyond the existing cpal error callback path.

Verification:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

Manual acceptance:

```text
1. Record with microphone enabled for at least 3 minutes.
2. Confirm no obvious audio dropouts or UI stalls.
3. Stop and restart recording 5 times.
4. Confirm no crash and no stale recording-tick events.
```

#### R2-006 Important: Stop timeout leaves the recording service in an awkward state

Evidence:

- `src-tauri/src/platform/macos/screen_capture_kit.rs::stop()` clears delegate sinks before `stopCaptureWithCompletionHandler`.
- If the stop completion callback times out, `stop()` returns `CaptureStopTimeout` before setting `self.stream = None`, `self.delegate = None`, or `self.running = false`.
- `src-tauri/src/platform/macos_service.rs::stop()` returns early on `capture_result?` before moving the state machine to `Processing` or `Completed`.
- `src-tauri/src/lib.rs::stop_recording()` stops the tick runtime before calling the service stop path.

Impact:

- UI tick can stop while backend state remains `Recording`.
- A later `start_recording` can be rejected by the state machine or `MacScreenCapture::running`.
- Native handles may remain retained after an error path.

Required remediation:

- Decide and document the timeout policy:
  - strict policy: keep native handles for safety, mark state as `Failed`, and surface a recoverable error to UI; or
  - cleanup policy: clear Rust handles after timeout only after confirming ScreenCaptureKit callbacks cannot use freed objects.
- Ensure `MacRecordingService::stop()` sets the app state to `Failed` when stop fails.
- Emit `recording-state-changed` with `failed` on stop failure.
- Keep tick cancellation, but include an error-state transition so the UI does not appear idle while backend remains recording.

Verification:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::state_machine
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

Manual acceptance:

```text
1. Simulate or force a stop timeout path.
2. Confirm stop_recording returns a Chinese structured error.
3. Confirm recording-state-changed emits failed.
4. Confirm a later start attempt either works after cleanup or returns a clear recoverable error.
```

#### R2-007 Moderate: ScreenCaptureKit audio buffer parsing needs another native safety pass

Evidence:

- `AudioBufferList` is represented with a one-element trailing array and then indexed via pointer arithmetic.
- The minimum-size check uses `size_of::<u32>() + num_buffers * size_of::<AudioBuffer>()`, which does not explicitly use the offset of `mBuffers`.
- `classify_pcm_format()` checks format id, float/signed-int flags, and bit depth, but does not verify interleaving, endian flags, `mBytesPerFrame`, or packet/frame consistency.
- Multiple buffers are appended sequentially, which is not equivalent to interleaved stereo if CoreAudio returns non-interleaved buffers.

Impact:

- Some valid CoreAudio layouts may be converted incorrectly.
- Non-interleaved system audio can be misinterpreted as interleaved audio.
- This remains a human-review gate before Phase 2 can be accepted.

Required remediation:

- Add explicit handling for interleaved vs non-interleaved PCM.
- Validate `mBytesPerFrame`, `mFramesPerPacket`, `mBytesPerPacket`, channel count, and byte order assumptions.
- Convert non-interleaved buffers into interleaved `f32` samples before creating `AudioChunk`.
- Add tests for conversion helpers using:
  - interleaved float32 stereo,
  - interleaved signed int16 stereo,
  - non-interleaved float32 stereo,
  - unsupported endian/layout flags.

Verification:

```bash
cargo test --manifest-path src-tauri/Cargo.toml platform::macos::screen_capture_kit::audio_conversion_tests
cargo test --manifest-path src-tauri/Cargo.toml
```

Manual review gate:

```text
Human reviewer must inspect ScreenCaptureKit AudioBufferList ownership, layout handling, retained block buffer release, and callback lifetime assumptions before marking this accepted.
```

#### R2-008 Moderate: macOS permission probe is still a placeholder

Evidence:

- `src-tauri/src/platform/macos/permissions.rs::MacPermissionProbe` returns `PermissionStatus::Unknown` for both screen recording and microphone.

Impact:

- UI cannot display accurate `denied` or `notDetermined` permission state.
- Phase 1 acceptance around permission guidance remains weak.

Required remediation:

- Keep the `PermissionService` boundary.
- Add native macOS permission status calls only after the native safety gate.
- Until native calls are implemented, document `Unknown` as an explicit temporary limitation and do not mark permission detection as fully complete.

Verification after native permission calls are added:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::permission_service
npm test -- --run
```

Manual acceptance:

```text
1. Run with Screen Recording permission denied.
2. Confirm UI shows a Chinese denied message.
3. Run with Microphone permission denied.
4. Confirm UI shows a Chinese denied message.
5. Grant both permissions.
6. Confirm UI no longer shows denied warnings.
```

### Round 2 Remediation Task List

Use this list as the execution order for the second remediation pass:

1. [ ] Fix frontend `RecordingResult` camelCase contract and add Vitest coverage.
2. [ ] Wire `RecordingWriter` into `MacRecordingService` and return writer-derived counts.
3. [ ] Preserve and count mixed audio chunks instead of discarding them.
4. [ ] Respect `captureSystemAudio` in ScreenCaptureKit configuration and output registration.
5. [ ] Replace latest-only audio draining with ordered timestamp pairing.
6. [ ] Remove mutex locking from the cpal audio callback hot path.
7. [ ] Define and implement a consistent stop-timeout state/resource policy.
8. [ ] Harden ScreenCaptureKit audio layout parsing and add conversion tests.
9. [ ] Document or implement real macOS permission probing.
10. [ ] Re-run full automated verification and update `tests/phase-1-2-remediation-checklist.md`.
11. [ ] Run manual artifact/audio acceptance before marking Phase 1/2 complete.

### Round 2 Definition of Done

Round 2 remediation is complete only when:

- `stop_recording` returns frontend-readable camelCase fields.
- `frameCount > 0` after a real macOS recording.
- `mixedAudioChunkCount > 0` when at least one audio source is enabled and produces chunks.
- System audio disabled in UI means ScreenCaptureKit does not request or count system audio.
- Capture callbacks use bounded non-blocking sends and no avoidable mutex locks on cpal's audio callback path.
- Stop timeout produces a structured Chinese error and a consistent app state transition.
- Native resource release behavior is documented for normal stop, failed stop, and timeout stop.
- The artifact writer is either explicitly documented as not yet active or produces a playable local file with an audio track.
- All unchecked manual items in `tests/phase-1-2-remediation-checklist.md` are either checked with evidence or left unchecked with a dated reason.
- Full verification commands pass or their blockers are documented:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

## 2026-05-25 Round 3 Code Review Findings

> Review input: second remediation pass after commits `f17b8e113e161abec9de3d975e57ef06a87ccf7b..HEAD` plus current uncommitted changes.
> Review focus: whether Phase 1 / Phase 2 are fully complete, and whether Phase 2 has capture-main-path blocking, memory safety, thread safety, or resource-release risks.

### Fresh Verification Evidence

Commands run during this review:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

Observed results:

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` exited 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` passed `50` Rust tests.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` exited 0, but emitted warnings in macOS FFI files and `macos_service.rs`.
- `cargo build --manifest-path src-tauri/Cargo.toml` exited 0, with the same warning class.
- `npm run build` exited 0.
- `npm test -- --run` passed `7` Vitest tests.

Manual verification not performed in this review:

- Real `npm run tauri dev` recording session.
- 1080p 3-minute recording stability.
- 5 repeated start/stop cycles.
- Playable local artifact validation.
- Real microphone/system-audio capture validation.

### Completion Judgment

Phase 1 status: **not complete yet**.

- Completed: macOS build blocker is fixed, commands are moved off the async command path with `spawn_blocking`, bounded Rust media queues are present, frontend/backend `RecordingResult` contract is camelCase, and Rust-side frame/audio flow does not route media through frontend JS.
- Not complete: no playable local recording artifact is produced; `CountingRecordingWriter` is still the active writer; FFmpeg writer is only a skeleton; several manual performance/resource checks remain unverified.

Phase 2 status: **not complete yet**.

- Completed: cpal callback hot path no longer uses the prior `Mutex<bool>` / sink mutex, `captureSystemAudio` is threaded into ScreenCaptureKit configuration, audio conversion handles basic interleaved and non-interleaved PCM cases, and mixed chunks are now passed into the writer fallback.
- Not complete: audio synchronization is still not a reliable continuous pairing stage, stop can discard queued media before writer finalization, retained CoreMedia block-buffer release needs another pass, and stop-timeout recovery can leave the capture adapter unable to restart.

### Blocking Findings

#### R3-001 Blocking: Phase 1/2 artifact requirement is still unmet

Evidence:

- `src-tauri/src/platform/macos_service.rs`
  - `MacRecordingService::start()` always creates `CountingRecordingWriter::new(None)`.
  - `stop()` can only return writer-derived counters, not a playable media file path.
- `src-tauri/src/media/ffmpeg_writer.rs`
  - `FfmpegRecordingWriter` only increments counters in `push_video()` / `push_audio()`.
  - `finish()` returns `Some(output_path)` without actually encoding, muxing, or creating the file.

Impact:

- Phase 1 cannot satisfy local playable recording artifact acceptance.
- Phase 2 cannot satisfy "recording artifact includes an audio track".
- Any checklist item implying artifact playback remains unaccepted.

Required remediation:

- Keep `CountingRecordingWriter` explicitly labeled as a development fallback.
- Do not return non-empty `outputPath` from a writer that has not created a file.
- Either fully implement a feature-gated `FfmpegRecordingWriter` that creates a playable artifact, or keep artifact acceptance unchecked with a dated limitation.
- Add a verification step that checks `outputPath` exists before returning it.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_writer
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

Manual acceptance:

```text
1. Start a 1080p recording for at least 5 seconds.
2. Stop recording.
3. Confirm frameCount > 0.
4. Confirm outputPath is non-empty only when a real file was created.
5. Confirm the file opens locally and contains video.
6. Confirm the file contains an audio track when audio is enabled.
```

#### R3-002 Important: Stop can drop queued media before writer finalization

Evidence:

- `src-tauri/src/platform/macos_service.rs::stop()` sets `stop_flag` before stopping capture and joining the consumer thread.
- `consume_frames()` checks the flag at the top of the loop and breaks immediately.
- Any frames/chunks already queued in `video_rx`, `system_audio_rx`, or `mic_rx` at that moment are not drained before `writer.finish()`.

Impact:

- The last frames/audio chunks can be lost during normal stop.
- Short recordings can undercount frames/audio or return `0` even if native capture produced data.
- Writer finalization does not represent all media delivered before stop.

Required remediation:

- Change the consumer lifecycle from "stop flag means exit now" to "stop flag means capture is ending, drain remaining queues, then finish".
- Stop native capture first or close/clear sinks, then allow the consumer to drain until all receivers are empty.
- Add a unit-testable helper for the drain loop so this can be verified without real ScreenCaptureKit.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml platform::macos_service
cargo test --manifest-path src-tauri/Cargo.toml
```

Suggested regression test:

```text
Queue video/audio chunks, signal stop, run the consumer finalization path, and assert writer counts include all queued chunks.
```

#### R3-003 Important: `CMBlockBuffer` retained release path may leak on the size-query call

Evidence:

- `CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer()` is called once to query `needed_size`.
- That first call passes a real `block_buffer_out` pointer.
- If CoreMedia writes a retained block buffer during the query call, the pointer can be overwritten by the second call and the first retained reference is never released.

Impact:

- System-audio callbacks can leak retained CoreMedia buffers over time.
- Long recordings increase the risk because this code runs on every audio sample buffer.

Required remediation:

- During the first size-query call, pass a null `block_buffer_out` if the API permits it, or use a separate pointer and release it on every return path.
- Keep exactly one owner variable for the second retained block buffer and release it after copied sample conversion.
- Add comments documenting the ownership contract for both calls.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml platform::macos::screen_capture_kit::audio_conversion_tests
cargo test --manifest-path src-tauri/Cargo.toml
```

Manual review gate:

```text
Human reviewer must inspect both CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer calls and confirm every retained CMBlockBuffer is released exactly once.
```

#### R3-004 Important: Audio synchronizer still does not implement reliable continuous timestamp pairing

Evidence:

- `AudioSynchronizer::drain_mixed()` only compares each system chunk with `mic_queue.front()`.
- If the front mic chunk is outside the pairing window, later mic chunks are never considered for that system chunk.
- The same `drain_mixed()` call then drains all leftover microphone chunks as mic-only output, so it cannot wait for a later system chunk.
- The test named `leaves_unmatched_mic_for_next_system` actually asserts that the unmatched mic is emitted immediately.

Impact:

- Out-of-order or jittered callbacks can produce avoidable mic-only/system-only chunks instead of paired mixed chunks.
- The "timestamp-based alignment" claim is still weaker than the Phase 2 requirement.
- Mixed audio can contain gaps or duplicate-like single-source segments around jitter boundaries.

Required remediation:

- Decide the intended synchronization policy:
  - low-latency policy: emit single-source chunks after a bounded watermark delay; or
  - strict pairing policy: retain unmatched chunks until they age out beyond a configured window.
- Search the queue for the closest timestamp within the window, not just the front item.
- Rename/fix the misleading test so expected behavior matches its name.
- Add tests for jittered arrival order and delayed matching.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::audio_synchronizer
cargo test --manifest-path src-tauri/Cargo.toml
```

#### R3-005 Important: Stop-timeout recovery still leaves restart behavior inconsistent

Evidence:

- `ScreenCaptureKit::stop()` clears delegate sinks before calling `stopCaptureWithCompletionHandler`.
- If the completion callback times out, it returns before setting `self.stream = None`, `self.delegate = None`, or `self.running = false`.
- `MacRecordingService::stop()` transitions the state machine to `Failed` on stop error.
- The state machine allows a later `start()` from `Failed`, but `MacScreenCapture::start_combined()` still rejects start when `self.running` remains true.

Impact:

- UI can show a failed/retry state while the platform adapter still refuses restart.
- Rust handles can remain retained after a stop timeout.
- The checklist item "stop failure allows restart" is not proven by the current code.

Required remediation:

- Define one explicit timeout policy:
  - conservative policy: keep native handles retained, mark the session unrecoverable until app restart, and surface that message clearly; or
  - recovery policy: safely clear Rust handles only after proving callbacks cannot use freed objects.
- Make `MacRecordingService` and `MacScreenCapture` agree on whether restart is allowed after timeout.
- Add tests around stop failure state transitions using a mock capture adapter.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::state_machine
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

Manual acceptance:

```text
1. Simulate or force ScreenCaptureKit stop timeout.
2. Confirm a Chinese structured error is returned.
3. Confirm recording-state-changed emits failed.
4. Confirm retry either succeeds or returns an explicit unrecoverable/native-restart-required error.
```

#### R3-006 Moderate: BUG-002 drag-region prevention has an error-state regression

Evidence:

- `src/components/error-view.tsx` still contains a container-level `data-tauri-drag-region={false}` wrapper around the error action buttons.
- `src/App.test.tsx` checks only the idle screen for `[data-tauri-drag-region="false"]`.

Impact:

- This partially reintroduces the BUG-002 pattern the project explicitly wants to avoid.
- The regression test can pass while a non-idle state violates the rule.

Required remediation:

- Remove the container-level false drag-region marker from `ErrorView`.
- Rely on the central interactive-element selector in `App.tsx` to exclude buttons from programmatic dragging.
- Expand the regression test to render or reach the error state before checking for false drag-region wrappers.

Verification after remediation:

```bash
npm test -- --run
npm run build
```

### Round 3 Remediation Task List

Use this list as the next execution order:

1. [ ] Fix writer/artifact semantics: never return a fake `outputPath`; document fallback vs real artifact clearly.
2. [ ] Change stop finalization so the consumer drains already queued media before `writer.finish()`.
3. [ ] Fix `CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer` retained buffer ownership on both calls.
4. [ ] Redesign `AudioSynchronizer` around an explicit watermark/timeout policy and closest-match pairing.
5. [ ] Define and implement a consistent ScreenCaptureKit stop-timeout recovery policy.
6. [ ] Remove the `ErrorView` container-level `data-tauri-drag-region={false}` regression and broaden the test.
7. [ ] Update `tests/phase-1-2-remediation-checklist.md` so only manually verified items are checked.
8. [ ] Re-run full automated verification.
9. [ ] Run real macOS recording acceptance before marking Phase 1/2 complete.

### Round 3 Definition of Done

Round 3 remediation is complete only when:

- `outputPath` is `None/null` unless a real file exists.
- Normal stop drains all queued frames/audio before writer finalization.
- CoreMedia retained block buffers have a documented release path for every return branch.
- Audio synchronization tests prove jittered and delayed chunks are paired or aged out according to the chosen policy.
- Stop timeout produces a consistent state and a documented retry/restart behavior.
- BUG-002 regression tests cover non-idle states.
- Automated verification passes with current working tree.
- Manual checklist items remain unchecked until verified on a real macOS recording session.

## 2026-05-26 Round 4 Code Review Findings

> Review input: Round 3 remediation changes in the current working tree.
> Review focus: whether Phase 1 / Phase 2 are fully complete, and whether Phase 2 still has capture-main-path blocking, memory safety, thread safety, or resource-release risks.

### Fresh Verification Evidence

Commands run during this review:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

Observed results:

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` exited 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` passed `53` Rust tests.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` exited 0, but emitted warnings:
  - macOS FFI naming warnings in `src-tauri/src/platform/macos/screen_capture_kit.rs`.
  - dead-code warnings for unused FFI helpers and `SendStream` tuple field.
  - `clippy::unnecessary_lazy_evaluations` in `src-tauri/src/platform/macos_service.rs`.
- `cargo build --manifest-path src-tauri/Cargo.toml` exited 0 with the same warning class.
- `npm run build` exited 0.
- `npm test -- --run` passed `8` Vitest tests.

Manual verification not performed in this review:

- Real `npm run tauri dev` recording session.
- 1080p 3-minute recording stability.
- 5 repeated start/stop cycles.
- Playable local artifact validation.
- Real microphone/system-audio capture validation.
- Native safety review of ScreenCaptureKit/cpal unsafe wrappers.

### Completion Judgment

Phase 1 status: **not complete yet**.

- Completed: Tauri/React scaffold builds; Rust state machine and command wiring exist; `start_recording` / `stop_recording` are moved to `spawn_blocking`; frame/audio streams remain in Rust; `RecordingResult` is camelCase; fake `outputPath` from the FFmpeg skeleton has been corrected to `None`.
- Not complete: the app still does not produce a playable local recording artifact. `MacRecordingService::start()` still uses `CountingRecordingWriter::new(None)`, and `FfmpegRecordingWriter` still does not encode, mux, or create a file.

Phase 2 status: **not complete yet**.

- Completed: bounded non-blocking media queues are used; cpal callback no longer holds the prior running-state mutex; ScreenCaptureKit system-audio parsing handles basic interleaved and non-interleaved PCM; `captureSystemAudio` is respected; BUG-002 false drag-region regression in `ErrorView` has been removed and covered by Vitest.
- Not complete: microphone and ScreenCaptureKit timestamps do not share one real session clock; audio synchronization lacks a watermark/age-out policy; stop finalization still has a producer/consumer race; ScreenCaptureKit stop-timeout recovery still has native lifecycle risk; several unsafe ownership paths still need human review.

### Blocking Findings

#### R4-001 Blocking: Phase 1/2 artifact requirement remains unmet

Evidence:

- `src-tauri/src/platform/macos_service.rs`
  - `MacRecordingService::start()` creates `Box::new(CountingRecordingWriter::new(None))`.
  - This writer counts frames/audio only; it does not create a recording file.
- `src-tauri/src/media/ffmpeg_writer.rs`
  - `FfmpegRecordingWriter::push_video()` and `push_audio()` only increment counters.
  - `finish()` returns `output_path: None`, correctly avoiding a fake path, but still does not produce the Phase 1 artifact.

Impact:

- Phase 1 cannot satisfy "recording ends with a playable local video file".
- Phase 2 cannot satisfy "recording file contains an audio track".
- Manual artifact acceptance must remain unchecked.

Required remediation:

- Keep `CountingRecordingWriter` as an explicit development/test fallback.
- Implement a real writer path behind the existing `ffmpeg` feature or another human-approved writer boundary.
- Return a non-empty `outputPath` only after the file exists and is closed/flushed.
- Add a writer-level verification that checks `output_path.exists()` before returning it.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_writer
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

Manual acceptance:

```text
1. Start a 1080p recording for at least 5 seconds.
2. Stop recording.
3. Confirm frameCount > 0.
4. Confirm outputPath is non-empty only when the file exists.
5. Open the file locally and confirm it contains video.
6. Enable audio and confirm the file contains an audio track.
```

#### R4-002 Important: Stop finalization still has a race that can drop tail media

Evidence:

- `src-tauri/src/platform/macos_service.rs::stop()` sets `stop_flag` before calling `ScreenCapture::stop()` and `mic_capture.stop()`.
- `consume_frames()` exits the main loop as soon as `stop_flag` is observed.
- Round 3 added a final drain after the loop, which fixes already-queued media, but native producers can still enqueue frames/chunks after that final drain begins or completes because capture is stopped afterward from another thread.

Impact:

- Tail video/audio can be dropped during normal stop.
- Short recordings can undercount frame/audio totals.
- Writer finalization may not include every media item delivered before native capture fully stopped.

Required remediation:

- Reverse the shutdown semantics:
  - first clear/stop native producers so no new media can be enqueued;
  - then signal the consumer that no more producer input is expected;
  - then drain until receivers are empty and finalize the writer.
- Prefer a two-state consumer signal such as `stop_requested` plus `producers_stopped`, or close/drop all sender handles before final drain.
- Add a unit-testable drain helper that can prove queued media is counted after stop.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml platform::macos_service
cargo test --manifest-path src-tauri/Cargo.toml
```

Suggested regression test:

```text
1. Queue video/audio chunks.
2. Simulate stop requested while producers can still enqueue one tail chunk.
3. Mark producers stopped.
4. Run final drain.
5. Assert writer counts include all queued and tail chunks.
```

#### R4-003 Important: ScreenCaptureKit stop-timeout recovery may drop live native handles

Evidence:

- `src-tauri/src/platform/macos/screen_capture_kit.rs::stop()` keeps `stream` and `delegate` retained when `stopCaptureWithCompletionHandler` times out, then sets `running = false` and `needs_reset = true`.
- `start_stream()` later drops `self.stream` and `self.delegate` when `needs_reset` is true.
- A timeout means ScreenCaptureKit has not confirmed the old stream is stopped. Dropping the old delegate/stream before a late callback is ruled out can invalidate callback assumptions.

Impact:

- Restart after stop timeout may create use-after-free or stale-callback behavior.
- UI/state machine can allow retry while the native adapter is in an uncertain lifecycle state.
- Resource release behavior after timeout is not yet safe enough for acceptance.

Required remediation:

- Pick one explicit policy:
  - conservative policy: stop timeout marks the platform adapter unrecoverable until app restart; retain native handles and return a clear Chinese error on later start attempts;
  - recovery policy: retain old handles until a late completion callback or other native signal proves the old stream is stopped, then release.
- Make `MacRecordingService`, `MacScreenCapture`, and frontend retry state agree on that policy.
- Add tests with a mock capture adapter for stop-timeout state transitions.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::state_machine
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

Manual acceptance:

```text
1. Force or simulate ScreenCaptureKit stop timeout.
2. Confirm stop_recording returns a Chinese structured error.
3. Confirm recording-state-changed emits failed.
4. Confirm retry either returns a clear unrecoverable/native-restart-required error or succeeds only after safe cleanup is proven.
```

#### R4-004 Important: Microphone and ScreenCaptureKit timestamps do not share one real session clock

Evidence:

- `src-tauri/src/platform/macos/screen_capture_kit.rs` uses `TimestampNormalizer` over CMSampleBuffer presentation timestamps.
- `src-tauri/src/platform/macos/cpal_microphone.rs` uses an independent `AudioSampleClock` that starts at 0 when the cpal callback begins.
- `src-tauri/src/platform/macos_service.rs::start()` starts ScreenCaptureKit first, then microphone capture. Both sources then appear to start at timestamp 0 even though their real start times can differ.

Impact:

- Phase 2 timestamp alignment can pair chunks that are not actually simultaneous.
- System audio and microphone drift/offset measurement is not possible from the current timestamps.
- The existing tests prove monotonic local timestamps, but not cross-source timestamp correctness.

Required remediation:

- Introduce a shared recording session clock origin in `MacRecordingService`.
- Convert both SCK and cpal timestamps to the same session-relative basis.
- For cpal, prefer callback-provided timing when available; if unavailable, derive sample timestamps from the session start instant plus accumulated frames.
- Add tests that simulate staggered SCK and mic start times and verify the expected offset is preserved.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::clock
cargo test --manifest-path src-tauri/Cargo.toml media::audio_synchronizer
cargo test --manifest-path src-tauri/Cargo.toml
```

#### R4-005 Moderate: Audio synchronizer still lacks a watermark/age-out policy

Evidence:

- `src-tauri/src/media/audio_synchronizer.rs::drain_mixed()` now searches the microphone queue for the closest timestamp within 10 ms.
- The same call drains all unmatched mic chunks as mic-only output immediately.
- There is no watermark or "hold until old enough" rule for delayed system/mic chunks.

Impact:

- Jittered or delayed callbacks can still become avoidable single-source chunks.
- Continuous mixed audio can contain unnecessary gaps around jitter boundaries.
- Phase 2's "continuous MixedAudioChunk" requirement remains only partially satisfied.

Required remediation:

- Define an explicit policy:
  - low-latency: emit single-source chunks only after they are older than a bounded watermark;
  - strict pairing: retain unmatched chunks until they age out beyond a configured window.
- Add tests for delayed matching, age-out, queue cap behavior, and out-of-order arrival.
- Keep `MAX_QUEUE_SIZE`, but record dropped old chunks in a counter if the writer/result should surface it later.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::audio_synchronizer
cargo test --manifest-path src-tauri/Cargo.toml
```

#### R4-006 Moderate: `AudioBufferList` bounds check still needs a native-layout safety pass

Evidence:

- `src-tauri/src/platform/macos/screen_capture_kit.rs` represents `AudioBufferList` with `mNumberBuffers: u32` followed by `mBuffers: [AudioBuffer; 1]`.
- The minimum-size check uses `size_of::<u32>() + num_buffers * size_of::<AudioBuffer>()`.
- On 64-bit layouts, C struct padding and the real offset of `mBuffers` should be accounted for explicitly.

Impact:

- The current check may understate the minimum byte size needed before pointer arithmetic.
- This remains an unsafe FFI boundary requiring human review before Phase 2 acceptance.

Required remediation:

- Compute required size as `size_of::<AudioBufferList>() + (num_buffers - 1) * size_of::<AudioBuffer>()` when `num_buffers > 0`, or use an explicit offset calculation for `mBuffers`.
- Reject `num_buffers == 0`.
- Add conversion-helper tests where feasible, and keep a manual native review gate for the actual FFI layout.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml platform::macos::screen_capture_kit::audio_conversion_tests
cargo test --manifest-path src-tauri/Cargo.toml
```

Manual review gate:

```text
Human reviewer must confirm AudioBufferList layout, mBuffers offset, retained block-buffer release, and sample data lifetime assumptions.
```

### Round 4 Remediation Task List

Use this list as the next execution order:

1. [ ] Decide and implement the artifact writer path, or explicitly keep Phase 1 artifact acceptance unchecked until FFmpeg muxing is implemented.
2. [ ] Redesign stop shutdown ordering so producers are stopped/closed before consumer final drain and `writer.finish()`.
3. [ ] Define a conservative or recoverable ScreenCaptureKit stop-timeout policy and make backend/frontend retry behavior match it.
4. [ ] Add a shared session clock so SCK system audio/video and cpal microphone timestamps use one time basis.
5. [ ] Add watermark/age-out semantics to `AudioSynchronizer` and tests for delayed/out-of-order chunks.
6. [ ] Fix or document the `AudioBufferList` layout size calculation and keep it under native safety review.
7. [ ] Clean up non-FFI clippy warnings in `macos_service.rs`; leave FFI naming warnings only if intentionally allowed.
8. [ ] Update `tests/phase-1-2-remediation-checklist.md` so artifact, real mic/system audio, long recording, repeat start/stop, and unsafe review remain unchecked until manually verified.
9. [ ] Re-run full automated verification.
10. [ ] Run real macOS recording acceptance before marking Phase 1/2 complete.

### Round 4 Definition of Done

Round 4 remediation is complete only when:

- `outputPath` is non-empty only when a real playable local file exists.
- Normal stop cannot enqueue media after final drain begins.
- Stop timeout has one documented policy and retry behavior follows it.
- Video, system audio, and microphone timestamps share one session clock basis.
- Audio synchronization retains unmatched chunks until match or age-out according to an explicit policy.
- `AudioBufferList` pointer arithmetic is guarded by a layout-correct size check.
- Automated verification passes with current working tree.
- Manual checklist items remain unchecked until verified on a real macOS recording session.

## 2026-05-26 Post-Round 4 Code Review Findings

> Review input: current working tree after commit `6d7dc08 fix(record): Round 4 修复停止管线、时间戳同步与音频配对策略`.
> Review focus: whether Phase 1 / Phase 2 are fully complete, and whether Phase 2 still has capture-main-path blocking, memory safety, thread safety, or resource-release risks.

### Fresh Verification Evidence

Commands run during this review:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

Observed results:

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` exited 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` passed `54` Rust tests.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` exited 0, but still emitted warnings:
  - macOS FFI naming warnings in `src-tauri/src/platform/macos/screen_capture_kit.rs`.
  - `unused_unsafe` warning around nested `Retained::retain(content)`.
  - dead-code warnings for unused CoreMedia helpers and the `SendStream` tuple field.
- `cargo build --manifest-path src-tauri/Cargo.toml` exited 0 with the same warning class.
- `npm run build` exited 0.
- `npm test -- --run` passed `8` Vitest tests, with the existing jsdom `Window.scrollTo()` not-implemented warning.

Manual verification not performed in this review:

- Real `npm run tauri dev` recording session.
- 1080p 3-minute recording stability.
- 5 repeated start/stop cycles.
- Playable local artifact validation.
- Real microphone/system-audio capture validation.
- Native safety review of ScreenCaptureKit/cpal unsafe wrappers.

### Completion Judgment

Phase 1 status: **not complete yet**.

- Completed: Tauri/React scaffold builds; Rust state machine and command wiring exist; `start_recording` / `stop_recording` are moved to `spawn_blocking`; frame/audio streams remain in Rust; `RecordingResult` is camelCase; fake `outputPath` is no longer returned.
- Not complete: the app still does not produce a playable local recording artifact. `MacRecordingService::start()` still uses `CountingRecordingWriter::new(None)`, and `FfmpegRecordingWriter` still does not encode, mux, or create a file.

Phase 2 status: **not complete yet**.

- Completed: bounded non-blocking media queues are used; cpal callback no longer holds the prior running-state mutex; ScreenCaptureKit system-audio parsing handles basic interleaved and non-interleaved PCM; `captureSystemAudio` is respected; BUG-002 false drag-region regression in `ErrorView` has been removed and covered by Vitest; Round 4 changed stop ordering so native captures are stopped before the consumer final drain.
- Not complete: microphone and ScreenCaptureKit timestamps still do not share one real session clock; non-interleaved audio conversion still has callback panic / layout mismatch risk; audio synchronization has a watermark sentinel bug; pause/resume is state-only; macOS permission probing remains a placeholder; several unsafe ownership paths still need human review.

### Blocking Findings

#### R5-001 Blocking: Phase 1/2 artifact requirement remains unmet

Evidence:

- `src-tauri/src/platform/macos_service.rs`
  - `MacRecordingService::start()` creates `Box::new(CountingRecordingWriter::new(None))`.
  - This writer counts frames/audio only; it does not create a recording file.
- `src-tauri/src/media/ffmpeg_writer.rs`
  - `FfmpegRecordingWriter::push_video()` and `push_audio()` only increment counters.
  - `finish()` returns `output_path: None`, correctly avoiding a fake path, but still does not produce the Phase 1 artifact.

Impact:

- Phase 1 cannot satisfy "recording ends with a playable local video file".
- Phase 2 cannot satisfy "recording file contains an audio track".
- Manual artifact acceptance must remain unchecked.

Required remediation:

- Keep `CountingRecordingWriter` as an explicit development/test fallback.
- Implement a real writer path behind the existing `ffmpeg` feature or another human-approved writer boundary.
- Return a non-empty `outputPath` only after the file exists and is closed/flushed.
- Add a writer-level verification that checks `output_path.exists()` before returning it.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::recording_writer
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

Manual acceptance:

```text
1. Start a 1080p recording for at least 5 seconds.
2. Stop recording.
3. Confirm frameCount > 0.
4. Confirm outputPath is non-empty only when the file exists.
5. Open the file locally and confirm it contains video.
6. Enable audio and confirm the file contains an audio track.
```

#### R5-002 Important: Microphone and ScreenCaptureKit timestamps still do not share one real session clock

Evidence:

- `src-tauri/src/platform/macos_service.rs::start()` starts `MacScreenCapture::start_combined(...)` first.
- Only after ScreenCaptureKit starts does the service create `SessionClock` and pass it to `CpalMicrophoneCapture`.
- `src-tauri/src/platform/macos/screen_capture_kit.rs` still uses `TimestampNormalizer` over CMSampleBuffer presentation timestamps and normalizes the first SCK sample to 0.
- `src-tauri/src/platform/macos/cpal_microphone.rs` anchors only cpal timestamps to `SessionClock`.

Impact:

- SCK video/system audio and cpal microphone are still on different effective clock bases.
- Real start-time offset between SCK and cpal can be erased or distorted.
- Phase 2 timestamp pairing can mix chunks that are not actually simultaneous.

Required remediation:

- Create the shared session clock before starting any capture source.
- Thread that shared clock into both ScreenCaptureKit and cpal timestamp conversion.
- For SCK, convert raw CMSampleBuffer timestamps into the same session-relative basis rather than using a separate first-sample-zero normalizer.
- For cpal, prefer callback timing when available; if unavailable, derive timestamps from session start plus accumulated frames.
- Add tests that simulate staggered SCK and mic start times and verify the expected offset is preserved.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::clock
cargo test --manifest-path src-tauri/Cargo.toml media::audio_synchronizer
cargo test --manifest-path src-tauri/Cargo.toml
```

#### R5-003 Important: Non-interleaved audio conversion can panic or produce metadata/sample layout mismatches

Evidence:

- `src-tauri/src/platform/macos/screen_capture_kit.rs::deinterleave_buffers()` derives `frames` from only `buffers[0].mDataByteSize`.
- The function builds `channel_samples` with `.take(channels).filter(...)`, so the number of collected channel buffers may be lower than `channels`.
- The interleave loop indexes `channel[frame_idx]` for every collected channel without verifying that all channel sample vectors are at least `frames` long.
- The returned `AudioChunk` keeps `channels` from ASBD even if fewer channel buffers were actually converted.

Impact:

- A shorter second channel buffer can panic inside the ScreenCaptureKit audio callback.
- Missing/null channel buffers can produce samples that do not match `AudioChunk.channels`.
- This remains a native callback safety risk and a Phase 2 audio correctness blocker.

Required remediation:

- Reject non-interleaved buffers unless `buffers.len() >= channels`.
- Verify every converted channel buffer has exactly the same frame count before interleaving.
- If any channel buffer is null, short, or malformed, drop that sample buffer without panicking.
- Add tests for:
  - non-interleaved buffers with mismatched lengths,
  - fewer buffers than ASBD channel count,
  - null/empty channel buffer rejection,
  - valid non-interleaved stereo success.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml platform::macos::screen_capture_kit::audio_conversion_tests
cargo test --manifest-path src-tauri/Cargo.toml
```

Manual review gate:

```text
Human reviewer must inspect non-interleaved AudioBufferList handling, panic-free callback behavior, and sample layout assumptions before accepting Phase 2 native audio.
```

#### R5-004 Important: AudioSynchronizer watermark uses `0` as a sentinel even though `0` is a valid media timestamp

Evidence:

- `src-tauri/src/media/audio_synchronizer.rs::latest_system_ts` is initialized to `0`.
- `drain_mixed()` treats `latest_system_ts == 0` as "no system chunks have ever been seen".
- The first normalized system chunk can legitimately have timestamp `0`.

Impact:

- After seeing a real system chunk at timestamp 0, unmatched mic chunks can still be emitted immediately as mic-only.
- The intended hold/age-out behavior is bypassed at the beginning of a normal recording session.
- Jittered early callbacks can create avoidable single-source audio chunks.

Required remediation:

- Replace `latest_system_ts: u64` with `Option<u64>`.
- Treat `None` as "no system chunks ever seen" and `Some(0)` as a valid watermark.
- Add tests for:
  - system chunk at timestamp 0 followed by unmatched mic chunk,
  - delayed matching after a zero timestamp system chunk,
  - age-out after the watermark advances.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml media::audio_synchronizer
cargo test --manifest-path src-tauri/Cargo.toml
```

#### R5-005 Important: Stop-timeout policy is now conservative, but resource and retry behavior still needs explicit acceptance

Evidence:

- `src-tauri/src/platform/macos/screen_capture_kit.rs::stop()` keeps `stream` and `delegate` retained when `stopCaptureWithCompletionHandler` times out, sets `running = false`, and sets `needs_reset = true`.
- `start_stream()` rejects later starts while `needs_reset` is true with "上次停止录制超时，请重启应用后再试".
- This avoids dropping live native handles, but it intentionally leaves the native lifecycle unrecoverable until app restart.

Impact:

- The current policy is safer than freeing uncertain native handles, but it can retain native resources after a stop timeout.
- UI retry can move back to idle while the backend will reject a later start until restart.
- Manual/resource validation is still required before Phase 2 can be accepted.

Required remediation:

- Document the conservative timeout policy in code comments, `HANDOFF.md`, and the manual checklist.
- Make frontend retry/back behavior surface "restart required" clearly after this error.
- Add tests with a mock capture adapter for stop-timeout state transitions and frontend failed-state behavior.
- If a recoverable policy is desired later, retain old handles until a late completion callback or other native signal proves the old stream is stopped before release.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::state_machine
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

Manual acceptance:

```text
1. Force or simulate ScreenCaptureKit stop timeout.
2. Confirm stop_recording returns a Chinese structured error.
3. Confirm recording-state-changed emits failed.
4. Confirm retry clearly returns restart-required behavior, or succeeds only after safe cleanup is proven.
5. Confirm retained native resources do not keep sending media after sinks are cleared.
```

#### R5-006 Moderate: Pause/resume is state-only and does not pause capture or writing

Evidence:

- `src-tauri/src/platform/macos_service.rs::pause()` only calls `self.state_machine.pause()`.
- `src-tauri/src/platform/macos_service.rs::resume()` only calls `self.state_machine.resume()`.
- ScreenCaptureKit, cpal, the consumer thread, and the writer continue running while the state is `Paused`.

Impact:

- UI can show "paused" while media capture and counting continue.
- A paused section will still be present in the eventual recording artifact.
- If pause/resume is considered part of Phase 2 command completion, its backend semantics are incomplete.

Required remediation:

- Decide the MVP pause semantics:
  - true pause: stop or suppress capture/writer input while paused;
  - UI-only pause placeholder: disable/hide pause controls until real semantics are implemented.
- If true pause is chosen, ensure the capture callback hot path still remains non-blocking and uses an atomic pause gate or writer-side segment handling.
- Add tests proving frame/audio counts do not advance during pause, or document pause as not accepted yet.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::state_machine
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

#### R5-007 Moderate: macOS permission probe remains a placeholder

Evidence:

- `src-tauri/src/platform/macos/permissions.rs::MacPermissionProbe` returns `PermissionStatus::Unknown` for both screen recording and microphone.

Impact:

- UI cannot accurately show `denied` or `notDetermined` on real macOS permission failures.
- Phase 1 permission guidance is not fully complete.

Required remediation:

- Keep `PermissionService` as the test seam.
- Implement real macOS permission probes only after native safety review:
  - screen recording via `CGPreflightScreenCaptureAccess()`;
  - microphone via `AVAudioApplication.recordPermission` or `AVCaptureDevice.authorizationStatus(for: .audio)`.
- Until then, keep permission checklist items unchecked and document `Unknown` as a dated limitation.

Verification after remediation:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::permission_service
npm test -- --run
```

Manual acceptance:

```text
1. Run with Screen Recording permission denied.
2. Confirm UI shows a Chinese denied message.
3. Run with Microphone permission denied.
4. Confirm UI shows a Chinese denied message.
5. Grant both permissions.
6. Confirm UI no longer shows denied warnings.
```

### BUG.md Rule Check

Checked during this review:

- `src-tauri/Cargo.toml` still keeps Tauri `macos-private-api`.
- `src-tauri/tauri.conf.json` still keeps `macOSPrivateApi: true`, `transparent: true`, and `acceptFirstMouse: true`.
- `src-tauri/capabilities/default.json` still keeps `core:window:allow-start-dragging`.
- No current source file contains container-level `data-tauri-drag-region={false}` or `data-tauri-drag-region="false"`.
- Current `whileTap` uses are on `motion.button` itself, not on a wrapper around a nested `Button`; this does not reintroduce BUG-003's exact failure pattern.

### Post-Round 4 Remediation Task List

Use this list as the next execution order:

1. [ ] Implement or explicitly defer the real artifact writer path; keep Phase 1 artifact acceptance unchecked until a playable file exists.
2. [ ] Create the shared session clock before all capture starts and thread it into both ScreenCaptureKit and cpal timestamp conversion.
3. [ ] Harden non-interleaved audio conversion so malformed channel buffers are rejected without callback panic.
4. [ ] Replace `AudioSynchronizer::latest_system_ts` sentinel with `Option<u64>` and add zero-timestamp watermark tests.
5. [ ] Document and test the conservative stop-timeout policy, including frontend retry/restart-required behavior.
6. [ ] Decide pause semantics; either implement true capture/write suppression or mark pause as UI-only/not accepted.
7. [ ] Implement or explicitly document the macOS permission probe placeholder limitation.
8. [ ] Clean up non-FFI warnings where low risk; keep FFI naming warnings only if intentionally allowed.
9. [ ] Update `tests/phase-1-2-remediation-checklist.md` so artifact, real mic/system audio, long recording, repeated start/stop, stop-timeout, and unsafe review remain unchecked until manually verified.
10. [ ] Re-run full automated verification.
11. [ ] Run real macOS recording acceptance before marking Phase 1/2 complete.

### Post-Round 4 Definition of Done

The next remediation pass is complete only when:

- `outputPath` is non-empty only when a real playable local file exists.
- Video, system audio, and microphone timestamps share one real session clock basis.
- Non-interleaved audio conversion is panic-free and rejects malformed channel layouts.
- Audio synchronizer watermark behavior is correct when the first valid timestamp is `0`.
- Stop timeout has one documented policy and frontend retry behavior follows it.
- Pause/resume either has true backend semantics or is clearly not part of accepted Phase 1/2 behavior.
- macOS permission behavior is either implemented or explicitly left unchecked with dated rationale.
- Automated verification passes with current working tree.
- Manual checklist items remain unchecked until verified on a real macOS recording session and native safety review.

## Self-Review

- Spec coverage: The plan maps each audit finding to a task: build blocker (Task 1), unbounded channels (Task 2), timestamp mismatch (Task 3), unsafe audio extraction (Task 4), command blocking and tick leaks (Task 5), stop timeout (Task 6), missing mix pipeline (Task 7), missing writer boundary/artifact (Tasks 8-9), command payload mismatch (Task 10), permissions (Task 11), BUG.md drag rule (Task 12), Windows boundary (Task 13), verification (Tasks 14-15).
- Placeholder scan: The plan avoids open-ended placeholders and names concrete files, commands, and expected results. The only execution pause is the required dependency gate for FFmpeg binding approval.
- Type consistency: New `MediaSender`, `MediaReceiver`, `TimestampNormalizer`, `AudioSampleClock`, `AudioSynchronizer`, `RecordingWriter`, and `RecordingResult` are introduced before later tasks consume them.
- Scope control: Phase 3+ UI expansion, cursor effects, silence trimming, export presets, licensing, telemetry, and platform publishing APIs are outside this remediation plan.
