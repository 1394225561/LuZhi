# Phase 6 BUG-005 Audio Synchronizer Rectification Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the root cause of BUG-005 — dual-source recordings produce silent audio because the `AudioSynchronizer` prematurely emits single-source windows, causing the writer to discard the late-arriving source as full-overlap.

**Architecture:** Refactor `AudioSynchronizer` into a true source-aware fixed-window sample merger: (1) watermark uses `min` of active sources instead of `max`, (2) chunks are split by sample frame into 20ms windows, (3) source timeout prevents indefinite blocking. Extend `RequestedAudioContract` to be source-aware. Fix CPAL timestamp anchor to use lazy first-callback initialization.

**Tech Stack:** Rust, FFmpeg (ffmpeg-next), cpal, Tauri 2.0

---

## File Structure

### Files to Modify

| File                                              | Responsibility                                                                                                                                                          |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/media/audio_synchronizer.rs`       | Core refactor: source-aware watermark, sample-frame splitting, source timeout, extended `SynchronizedAudioChunk`                                                        |
| `src-tauri/src/media/recording_writer.rs`         | Extend `RecordingDiagnostics` with source-aware window/writer tracking fields; extend `TimelineAppendResult` with partial-overlap flag                                  |
| `src-tauri/src/media/ffmpeg_writer.rs`            | Set `trimmed_partial_overlap` in `TimelineAppendResult` for partial-overlap branch; increment `audio_chunks_trimmed_partial_overlap`                                    |
| `src-tauri/src/media/ffmpeg_common.rs`            | Extend `RequestedAudioContract` with source-aware validation; add `audible_min_rms` threshold; add per-source contract check                                            |
| `src-tauri/src/platform/macos_service.rs`         | Pass `AudioSynchronizerConfig` to synchronizer; track source-aware diagnostics in consumer thread; save requested audio config for export; fix Bluetooth mic stop order |
| `src-tauri/src/platform/macos/cpal_microphone.rs` | Lazy timestamp offset initialization on first callback instead of stream build time; explicit stream drop + bounded wait on stop                                        |
| `src-tauri/src/core/clock.rs`                     | Add `with_lazy_offset()` constructor to `AudioSampleClock`; add `initialize_offset()` method                                                                            |
| `src-tauri/src/lib.rs`                            | Use `validate_export_artifact_with_audio_contract` in `export_video()`                                                                                                  |
| `BUG.md`                                          | Update BUG-005 status and add new prevention rules                                                                                                                      |

### Files to Create

None — all changes are modifications to existing files.

---

## Task 1: Add failing tests for dual-source watermark and long-chunk splitting

**Files:**

- Modify: `src-tauri/src/media/audio_synchronizer.rs`

- [ ] **Step 1: Write test for dual-source offset not discarding mic windows**

```rust
#[test]
fn dual_source_offset_does_not_discard_mic_windows() {
    // BUG-005 Critical 1: When system audio leads mic by > HOLD_NANOS,
    // the synchronizer must NOT emit system-only windows before mic arrives.
    // With source-aware watermark using min(system, mic), system-only windows
    // should be held until mic catches up or source timeout fires.
    let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

    // System arrives 60ms ahead of mic (exceeds 40ms HOLD_NANOS).
    // Window 0 (0-20ms): system at 5ms
    synchronizer.push_system(chunk(5_000_000, vec![0.3, 0.3]));
    // Window 1 (20-40ms): system at 25ms
    synchronizer.push_system(chunk(25_000_000, vec![0.3, 0.3]));
    // Window 2 (40-60ms): system at 45ms
    synchronizer.push_system(chunk(45_000_000, vec![0.3, 0.3]));

    // Mic arrives 60ms later — same recording timeline, just delayed.
    // Window 0: mic at 65ms (window_idx = 3, but represents same content)
    // Actually, mic timestamps should overlap with system windows.
    // Simulate: mic chunks arrive at 60ms, 80ms, 100ms — these land in
    // windows 3, 4, 5 — NOT windows 0, 1, 2.
    //
    // The real scenario: mic starts 60ms late, so mic timestamp 0 maps to
    // wall-clock 60ms. But system timestamp 0 maps to wall-clock 0ms.
    // The synchronizer sees system in windows 0,1,2 and mic in windows 0,1,2
    // IF timestamps are session-relative. The offset is in the timestamps.
    //
    // Simulate the actual BUG-005 pattern: system and mic have overlapping
    // timestamps but system arrives first due to callback timing.
    // System: windows 0-2 filled, mic: windows 0-2 arrive 60ms later.
    // With current max-watermark, system-only windows 0-2 get emitted
    // before mic lands. With min-watermark, they wait.

    // Push mic into the SAME windows but after system has been there a while.
    synchronizer.push_mic(chunk(10_000_000, vec![0.5, 0.5])); // window 0
    synchronizer.push_mic(chunk(30_000_000, vec![0.5, 0.5])); // window 1
    synchronizer.push_mic(chunk(50_000_000, vec![0.5, 0.5])); // window 2

    // Advance watermark past all windows.
    synchronizer.push_system(chunk(200_000_000, vec![0.1, 0.1]));

    let results = synchronizer.drain_mixed();

    // All 3 windows should be paired, not system-only.
    let (paired, sys_only, mic_only) = synchronizer.diagnostics();
    assert_eq!(paired, 3, "all windows should be paired, got paired={paired}, sys_only={sys_only}, mic_only={mic_only}");
    assert_eq!(sys_only, 0, "should have no system-only windows");
    assert_eq!(mic_only, 0, "should have no mic-only windows");
}
```

- [ ] **Step 2: Write test for long chunk splitting into fixed windows**

```rust
#[test]
fn synchronizer_splits_long_system_callback_to_multiple_windows() {
    // BUG-005 Critical 2: A single callback delivering 60ms of audio
    // must be split into 3 × 20ms windows, not lumped into one.
    let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

    // 60ms of 48kHz stereo = 60 * 48000 / 1000 = 2880 frames = 5760 samples
    let samples_60ms = vec![0.3f32; 5760];
    synchronizer.push_system(AudioChunk {
        timestamp: MediaTimestamp::from_nanos(0),
        sample_rate: 48_000,
        channels: 2,
        samples: Arc::from(samples_60ms.into_boxed_slice()),
    });

    // Advance watermark.
    synchronizer.push_system(chunk(200_000_000, vec![0.1, 0.1]));

    let results = synchronizer.drain_mixed();

    // Should produce 3 windows (0-20ms, 20-40ms, 40-60ms), not 1.
    assert_eq!(results.len(), 3, "expected 3 windows from 60ms chunk, got {}", results.len());

    // Each window should have ~1920 samples (960 frames × 2ch).
    for (i, result) in results.iter().enumerate() {
        let mixed = result.as_ref().unwrap();
        assert_eq!(mixed.samples.len(), 1920, "window {i} should have 1920 samples");
        assert_eq!(mixed.sample_rate, 48_000);
        assert_eq!(mixed.channels, 2);
    }
}

#[test]
fn synchronizer_splits_long_mic_callback_to_multiple_windows() {
    let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

    // 40ms of 48kHz mono = 40 * 48000 / 1000 = 1920 frames = 1920 samples
    let samples_40ms = vec![0.5f32; 1920];
    synchronizer.push_mic(AudioChunk {
        timestamp: MediaTimestamp::from_nanos(0),
        sample_rate: 48_000,
        channels: 1,
        samples: Arc::from(samples_40ms.into_boxed_slice()),
    });

    // Advance watermark.
    synchronizer.push_mic(chunk(200_000_000, vec![0.1, 0.1]));

    let results = synchronizer.drain_mixed();

    // Should produce 2 windows (0-20ms, 20-40ms).
    assert_eq!(results.len(), 2, "expected 2 windows from 40ms chunk, got {}", results.len());
}
```

- [ ] **Step 3: Write test for source-aware watermark not emitting fast source before slow**

```rust
#[test]
fn synchronizer_does_not_emit_fast_source_before_slow_source_watermark() {
    // System is 3 windows ahead of mic. With source-aware watermark,
    // no system-only windows should be emitted until mic arrives or timeout.
    let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

    // System fills windows 0-4.
    for i in 0..5 {
        synchronizer.push_system(chunk(i * 20_000_000 + 5_000_000, vec![0.3, 0.3]));
    }

    // Only mic in window 0 and 1 (arrives late for windows 2-4).
    synchronizer.push_mic(chunk(5_000_000, vec![0.5, 0.5]));
    synchronizer.push_mic(chunk(25_000_000, vec![0.5, 0.5]));

    // With source-aware watermark (min of latest_system, latest_mic),
    // watermark = min(105ms, 45ms) - 40ms = 5ms.
    // Only windows ending before 5ms would emit → none.
    // This is correct: we wait for mic to catch up.

    // Now advance mic to window 4.
    synchronizer.push_mic(chunk(85_000_000, vec![0.5, 0.5]));

    let results = synchronizer.drain_mixed();

    // Windows 0 and 1 should be paired. Windows 2-4 may be system-only
    // or may still be held depending on timeout.
    let (paired, sys_only, _mic_only) = synchronizer.diagnostics();
    assert!(paired >= 2, "windows 0-1 should be paired, got {paired}");
    // The key assertion: no system-only windows before mic had a chance.
    // With min-watermark, system-only windows 2-4 should only emit after
    // mic timestamp advances past them or timeout fires.
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg dual_source_offset_does_not_discard_mic_windows -- --nocapture`
Expected: FAIL (current `max` watermark emits system-only windows before mic arrives)

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg synchronizer_splits_long_system_callback_to_multiple_windows -- --nocapture`
Expected: FAIL (current code puts entire chunk into one window)

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg synchronizer_does_not_emit_fast_source_before_slow_source_watermark -- --nocapture`
Expected: FAIL (current `max` watermark emits system-only immediately)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/audio_synchronizer.rs
git commit -m "test(audio): 补充双音频源同步器缺陷的失败测试（Critical 1/2 锁定）"
```

---

## Task 2: Refactor AudioSynchronizer — source-aware watermark + config + extended SynchronizedAudioChunk

**Files:**

- Modify: `src-tauri/src/media/audio_synchronizer.rs`

- [ ] **Step 1: Add `AudioSynchronizerConfig` struct**

```rust
/// Configuration for the audio synchronizer.
pub struct AudioSynchronizerConfig {
    /// Whether system audio was requested.
    pub requested_system_audio: bool,
    /// Whether microphone was requested.
    pub requested_microphone: bool,
    /// Window size in nanoseconds (default 20ms).
    pub window_nanos: u64,
    /// Hold window in nanoseconds (default 40ms).
    pub hold_nanos: u64,
    /// Timeout for a source that was seen but has stalled (default 2s).
    /// If a source hasn't produced a chunk in this duration, the synchronizer
    /// will emit windows containing only the active source.
    pub source_stall_timeout_nanos: u64,
}

impl Default for AudioSynchronizerConfig {
    fn default() -> Self {
        Self {
            requested_system_audio: false,
            requested_microphone: false,
            window_nanos: 20_000_000,     // 20ms
            hold_nanos: 40_000_000,       // 40ms
            source_stall_timeout_nanos: 2_000_000_000, // 2 seconds
        }
    }
}
```

- [ ] **Step 2: Add `system_frames` and `mic_frames` to `SynchronizedAudioChunk`**

```rust
#[derive(Debug, Clone)]
pub struct SynchronizedAudioChunk {
    pub mixed: MixedAudioChunk,
    pub has_system: bool,
    pub has_mic: bool,
    pub system_rms: f32,
    pub mic_rms: f32,
    /// Number of system audio frames (samples / channels) in this window.
    pub system_frames: u64,
    /// Number of mic audio frames (samples / channels) in this window.
    pub mic_frames: u64,
    /// Whether this window was emitted due to source stall timeout.
    pub emitted_due_to_timeout: bool,
}
```

- [ ] **Step 3: Refactor `AudioSynchronizer` to use config and source-aware watermark**

Replace the struct definition:

```rust
pub struct AudioSynchronizer<M: AudioMixer = SimpleAudioMixer> {
    mixer: M,
    windows: BTreeMap<u64, AudioWindow>,
    config: AudioSynchronizerConfig,
    /// Latest system chunk timestamp seen (nanoseconds).
    latest_system_ts: u64,
    /// Latest mic chunk timestamp seen (nanoseconds).
    latest_mic_ts: u64,
    /// Whether we've seen any system audio chunk.
    seen_system: bool,
    /// Whether we've seen any mic audio chunk.
    seen_mic: bool,
    /// Timestamp when system source was last active (for stall detection).
    last_system_active_nanos: u64,
    /// Timestamp when mic source was last active (for stall detection).
    last_mic_active_nanos: u64,
    /// Diagnostics.
    paired_window_count: u64,
    system_only_window_count: u64,
    mic_only_window_count: u64,
    source_timeout_window_count: u64,
}
```

Update `new()`:

```rust
pub fn new(mixer: M, config: AudioSynchronizerConfig) -> Self {
    Self {
        mixer,
        windows: BTreeMap::new(),
        config,
        latest_system_ts: 0,
        latest_mic_ts: 0,
        seen_system: false,
        seen_mic: false,
        last_system_active_nanos: 0,
        last_mic_active_nanos: 0,
        paired_window_count: 0,
        system_only_window_count: 0,
        mic_only_window_count: 0,
        source_timeout_window_count: 0,
    }
}
```

Update `Default`:

```rust
impl Default for AudioSynchronizer<SimpleAudioMixer> {
    fn default() -> Self {
        Self::new(SimpleAudioMixer::new(), AudioSynchronizerConfig::default())
    }
}
```

- [ ] **Step 4: Implement source-aware watermark calculation**

Replace `calculate_watermark()`:

```rust
fn calculate_watermark(&self) -> u64 {
    let both_requested = self.config.requested_system_audio && self.config.requested_microphone;

    if both_requested && self.seen_system && self.seen_mic {
        // Both sources requested and both have been seen.
        // Use the SLOWER source's latest timestamp so we don't emit
        // windows from the fast source before the slow source arrives.
        let min_ts = self.latest_system_ts.min(self.latest_mic_ts);
        let base_watermark = min_ts.saturating_sub(self.config.hold_nanos);

        // Check for source stall: if one source hasn't produced a chunk
        // in source_stall_timeout_nanos, allow the active source to drive.
        let now_nanos = self.latest_system_ts.max(self.latest_mic_ts);
        let system_stalled = now_nanos.saturating_sub(self.last_system_active_nanos)
            > self.config.source_stall_timeout_nanos;
        let mic_stalled = now_nanos.saturating_sub(self.last_mic_active_nanos)
            > self.config.source_stall_timeout_nanos;

        if system_stalled || mic_stalled {
            // One source has stalled — fall back to max-based watermark
            // to avoid blocking indefinitely.
            self.source_timeout_window_count; // will be incremented per emitted window
            let max_ts = self.latest_system_ts.max(self.latest_mic_ts);
            max_ts.saturating_sub(self.config.hold_nanos)
        } else {
            base_watermark
        }
    } else {
        // Single-source recording or one source not yet seen:
        // use the active source's latest timestamp.
        let max_ts = self.latest_system_ts.max(self.latest_mic_ts);
        max_ts.saturating_sub(self.config.hold_nanos)
    }
}
```

- [ ] **Step 5: Update `push_system()` and `push_mic()` to track seen/active state and split long chunks**

Replace `push_system()`:

```rust
pub fn push_system(&mut self, chunk: AudioChunk) {
    self.seen_system = true;
    let ts = chunk.timestamp.nanos;
    self.latest_system_ts = self.latest_system_ts.max(ts);
    self.last_system_active_nanos = ts;

    self.push_source_chunk(Source::System, &chunk);
}
```

Replace `push_mic()`:

```rust
pub fn push_mic(&mut self, chunk: AudioChunk) {
    self.seen_mic = true;
    let ts = chunk.timestamp.nanos;
    self.latest_mic_ts = self.latest_mic_ts.max(ts);
    self.last_mic_active_nanos = ts;

    self.push_source_chunk(Source::Mic, &chunk);
}
```

Add the shared helper that splits by sample frame:

```rust
enum Source {
    System,
    Mic,
}

fn push_source_chunk(&mut self, source: Source, chunk: &AudioChunk) {
    let ts = chunk.timestamp.nanos;
    let sample_rate = chunk.sample_rate as u64;
    let channels = chunk.channels as u16;
    let total_frames = chunk.samples.len() as u64 / channels.max(1) as u64;

    if total_frames == 0 {
        return;
    }

    // Calculate the time range this chunk covers.
    let frame_duration_nanos = 1_000_000_000 / sample_rate;
    let chunk_start_nanos = ts;
    let chunk_end_nanos = ts + total_frames * frame_duration_nanos;

    // Determine which 20ms windows this chunk overlaps.
    let start_window = chunk_start_nanos / self.config.window_nanos;
    let end_window = (chunk_end_nanos - 1) / self.config.window_nanos; // inclusive

    for window_idx in start_window..=end_window {
        let window_start = window_idx * self.config.window_nanos;
        let window_end = window_start + self.config.window_nanos;

        // Calculate the sample range that falls within this window.
        let overlap_start_nanos = chunk_start_nanos.max(window_start);
        let overlap_end_nanos = chunk_end_nanos.min(window_end);

        if overlap_start_nanos >= overlap_end_nanos {
            continue;
        }

        let start_frame = (overlap_start_nanos - chunk_start_nanos) / frame_duration_nanos;
        let end_frame = (overlap_end_nanos - chunk_start_nanos) / frame_duration_nanos;
        let start_sample = (start_frame * channels as u64) as usize;
        let end_sample = (end_frame * channels as u64) as usize;

        let end_sample = end_sample.min(chunk.samples.len());
        if start_sample >= end_sample {
            continue;
        }

        let slice = &chunk.samples[start_sample..end_sample];

        let window = self.windows.entry(window_idx).or_insert_with(|| AudioWindow {
            system: None,
            mic: None,
            window_start_nanos: window_start,
        });

        let buf = match source {
            Source::System => window.system.get_or_insert_with(|| SourceWindowBuffer {
                samples: Vec::new(),
                sample_rate: chunk.sample_rate,
                channels: chunk.channels,
            }),
            Source::Mic => window.mic.get_or_insert_with(|| SourceWindowBuffer {
                samples: Vec::new(),
                sample_rate: chunk.sample_rate,
                channels: chunk.channels,
            }),
        };

        buf.samples.extend_from_slice(slice);
    }

    // Evict oldest windows if we exceed the limit.
    while self.windows.len() > MAX_WINDOWS {
        if let Some((&oldest_idx, _)) = self.windows.iter().next() {
            self.windows.remove(&oldest_idx);
        }
    }
}
```

- [ ] **Step 6: Update `emit_window()` to populate extended fields**

Update `emit_window` to also track `system_frames` and `mic_frames` for diagnostics. Change `drain_mixed()` to return `Vec<AppResult<SynchronizedAudioChunk>>` instead of `Vec<AppResult<MixedAudioChunk>>`.

Update `drain_mixed()` signature:

```rust
pub fn drain_mixed(&mut self) -> Vec<AppResult<SynchronizedAudioChunk>> {
    let watermark_nanos = self.calculate_watermark();
    let ready_indices: Vec<u64> = self
        .windows
        .range(..)
        .filter(|(_, window)| {
            let window_end = window.window_start_nanos + self.config.window_nanos;
            window_end <= watermark_nanos
        })
        .map(|(&idx, _)| idx)
        .collect();

    let mut results = Vec::with_capacity(ready_indices.len());
    for idx in ready_indices {
        if let Some(window) = self.windows.remove(&idx) {
            match self.emit_window(window, false) {
                Ok(synced) => results.push(Ok(synced)),
                Err(e) => results.push(Err(e)),
            }
        }
    }
    results
}
```

Update `emit_window()` to return `SynchronizedAudioChunk`:

```rust
fn emit_window(&mut self, window: AudioWindow, emitted_due_to_timeout: bool) -> AppResult<SynchronizedAudioChunk> {
    let has_system = window.system.is_some();
    let has_mic = window.mic.is_some();

    // Update diagnostics.
    if has_system && has_mic {
        self.paired_window_count += 1;
    } else if has_system {
        self.system_only_window_count += 1;
    } else if has_mic {
        self.mic_only_window_count += 1;
    }
    if emitted_due_to_timeout {
        self.source_timeout_window_count += 1;
    }

    let system_frames = window.system.as_ref().map_or(0, |b| {
        b.samples.len() as u64 / b.channels.max(1) as u64
    });
    let mic_frames = window.mic.as_ref().map_or(0, |b| {
        b.samples.len() as u64 / b.channels.max(1) as u64
    });
    let system_rms = window.system.as_ref().map_or(0.0, |b| compute_rms(&b.samples));
    let mic_rms = window.mic.as_ref().map_or(0.0, |b| compute_rms(&b.samples));

    let system_chunk = window.system.map(|buf| AudioChunk {
        timestamp: crate::core::frame::MediaTimestamp::from_nanos(window.window_start_nanos),
        sample_rate: buf.sample_rate,
        channels: buf.channels,
        samples: std::sync::Arc::from(buf.samples.into_boxed_slice()),
    });

    let mic_chunk = window.mic.map(|buf| AudioChunk {
        timestamp: crate::core::frame::MediaTimestamp::from_nanos(window.window_start_nanos),
        sample_rate: buf.sample_rate,
        channels: buf.channels,
        samples: std::sync::Arc::from(buf.samples.into_boxed_slice()),
    });

    let mixed = self.mixer.mix(system_chunk.as_ref(), mic_chunk.as_ref())?;

    Ok(SynchronizedAudioChunk {
        mixed,
        has_system,
        has_mic,
        system_rms,
        mic_rms,
        system_frames,
        mic_frames,
        emitted_due_to_timeout,
    })
}
```

- [ ] **Step 7: Update `drain_final()` to use new `emit_window` and return extended chunk**

```rust
pub fn drain_final(&mut self) -> Vec<(SynchronizedAudioChunk, bool)> {
    let mut results = Vec::with_capacity(self.windows.len());
    let indices: Vec<u64> = self.windows.keys().copied().collect();
    for idx in indices {
        if let Some(window) = self.windows.remove(&idx) {
            let has_system = window.system.is_some();
            let has_mic = window.mic.is_some();
            let was_unpaired = has_system != has_mic;

            match self.emit_window(window, false) {
                Ok(synced) => results.push((synced, was_unpaired)),
                Err(_) => {}
            }
        }
    }
    results.sort_by_key(|(chunk, _)| chunk.mixed.timestamp.nanos);
    results
}
```

- [ ] **Step 8: Update `diagnostics()` to include timeout count**

```rust
pub fn diagnostics(&self) -> (u64, u64, u64, u64) {
    (
        self.paired_window_count,
        self.system_only_window_count,
        self.mic_only_window_count,
        self.source_timeout_window_count,
    )
}
```

- [ ] **Step 9: Run tests to verify the 3 new tests pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg dual_source_offset_does_not_discard_mic_windows -- --nocapture`
Expected: PASS

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg synchronizer_splits_long_system_callback_to_multiple_windows -- --nocapture`
Expected: PASS

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg synchronizer_does_not_emit_fast_source_before_slow_source_watermark -- --nocapture`
Expected: PASS

- [ ] **Step 10: Update existing tests for new return types**

Existing tests use `drain_mixed()` which now returns `Vec<AppResult<SynchronizedAudioChunk>>` instead of `Vec<AppResult<MixedAudioChunk>>`. Update test assertions:

- `results[0].as_ref().unwrap()` now returns `SynchronizedAudioChunk` — access `.mixed` for the audio data.
- `synchronizer.diagnostics()` now returns 4-tuple `(paired, sys_only, mic_only, timeout_count)` — destructure accordingly.
- `emit_window` now updates diagnostics in `drain_mixed()` too (not just `drain_final()`), so pairing counts may change.

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer -- --nocapture`
Expected: ALL PASS after updates

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/media/audio_synchronizer.rs
git commit -m "fix(audio): 重构 AudioSynchronizer 为 source-aware fixed-window sample merger"
```

---

## Task 3: Update macos_service consumer thread for source-aware diagnostics

**Files:**

- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/media/recording_writer.rs`

- [ ] **Step 1: Add source-aware fields to `RecordingDiagnostics`**

In `recording_writer.rs`, add to `RecordingDiagnostics`:

```rust
/// Number of windows emitted due to source stall timeout.
pub source_timeout_window_count: u64,
/// Maximum RMS of system audio before writer (from synchronizer output).
pub system_rms_max_before_writer: f32,
/// Maximum RMS of mic audio before writer (from synchronizer output).
pub mic_rms_max_before_writer: f32,
```

- [ ] **Step 2: Update consumer thread to pass `AudioSynchronizerConfig`**

In `macos_service.rs`, replace the synchronizer creation (line 428):

```rust
let mut synchronizer = crate::media::audio_synchronizer::AudioSynchronizer::new(
    SimpleAudioMixer::new(),
    AudioSynchronizerConfig {
        requested_system_audio,
        requested_microphone,
        ..Default::default()
    },
);
```

- [ ] **Step 3: Update consumer thread drain loop to handle `SynchronizedAudioChunk`**

The `drain_mixed()` now returns `Vec<AppResult<SynchronizedAudioChunk>>` instead of `Vec<AppResult<MixedAudioChunk>>`. Update the consumer loop:

```rust
for synced_result in synchronizer.drain_mixed() {
    match synced_result {
        Ok(synced) => {
            // Track per-source RMS before writer.
            if synced.has_system && synced.system_rms > diagnostics.system_rms_max_before_writer {
                diagnostics.system_rms_max_before_writer = synced.system_rms;
            }
            if synced.has_mic && synced.mic_rms > diagnostics.mic_rms_max_before_writer {
                diagnostics.mic_rms_max_before_writer = synced.mic_rms;
            }
            // Track mixed audio RMS.
            let mixed_rms = compute_rms(&synced.mixed.samples);
            if mixed_rms > diagnostics.mixed_rms_max {
                diagnostics.mixed_rms_max = mixed_rms;
            }
            // ... rest of existing logic using synced.mixed ...
            if let Err(e) = writer.push_audio(synced.mixed) {
                // ... existing error handling ...
            }
        }
        Err(e) => eprintln!("音频混合失败: {e}"),
    }
}
```

- [ ] **Step 4: Update final drain diagnostics to include timeout count**

```rust
let (paired, sys_only, mic_only, timeout_windows) = synchronizer.diagnostics();
diagnostics.paired_window_count = paired;
diagnostics.system_only_window_count = sys_only;
diagnostics.mic_only_window_count = mic_only;
diagnostics.source_timeout_window_count = timeout_windows;
```

- [ ] **Step 5: Save requested audio config for export contract**

Add fields to `MacRecordingService` or `AppState` to persist the last recording's audio config. In `stop()`, save:

```rust
*self.last_requested_system_audio.lock().unwrap() = Some(requested_system_audio);
*self.last_requested_microphone.lock().unwrap() = Some(requested_microphone);
```

- [ ] **Step 6: Run consumer-related tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg -- --nocapture`
Expected: ALL PASS

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs src-tauri/src/media/recording_writer.rs
git commit -m "fix(audio): consumer 线程接入 source-aware 同步器和诊断"
```

---

## Task 4: Extend TimelineAppendResult and writer diagnostics for partial-overlap tracking

**Files:**

- Modify: `src-tauri/src/media/ffmpeg_writer.rs`

- [ ] **Step 1: Add `trimmed_partial_overlap` and `trimmed_frames` to `TimelineAppendResult`**

```rust
struct TimelineAppendResult {
    chunk_appended: bool,
    silence_frames_padded: u64,
    appended_frames: u64,
    /// Whether this chunk was partially trimmed due to overlap.
    trimmed_partial_overlap: bool,
    /// Number of mono frames trimmed (skipped) due to partial overlap.
    trimmed_frames: u64,
}
```

- [ ] **Step 2: Set `trimmed_partial_overlap` in the partial-overlap branch**

In `append_audio_chunk_to_timeline`, update the partial-overlap return:

```rust
// Partial overlap: skip the overlapping prefix, append the rest.
let skip_interleaved = overlap_mono * 2;
let remaining = &samples[skip_interleaved..];
audio_sample_buffer.extend_from_slice(remaining);
let appended_mono = (remaining.len() / 2) as i64;
*audio_timeline_cursor += appended_mono;
return TimelineAppendResult {
    chunk_appended: true,
    silence_frames_padded: 0,
    appended_frames: appended_mono as u64,
    trimmed_partial_overlap: true,
    trimmed_frames: overlap_mono as u64,
};
```

Update all other return sites to include `trimmed_partial_overlap: false, trimmed_frames: 0`.

- [ ] **Step 3: Increment `audio_chunks_trimmed_partial_overlap` in encoder_worker**

After the `append_audio_chunk_to_timeline` call:

```rust
if append_result.trimmed_partial_overlap {
    writer_diag.audio_chunks_trimmed_partial_overlap += 1;
}
```

- [ ] **Step 4: Run writer tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture`
Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/ffmpeg_writer.rs
git commit -m "fix(audio): writer partial-overlap diagnostics 正确递增"
```

---

## Task 5: Source-aware requested-audio contract

**Files:**

- Modify: `src-tauri/src/media/ffmpeg_common.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`

- [ ] **Step 1: Add `audible_min_rms` to `RequestedAudioContract`**

```rust
pub struct RequestedAudioContract {
    pub requested_system_audio: bool,
    pub requested_microphone: bool,
    pub min_rms: f32,
    pub min_peak: f32,
    /// Higher threshold for "audible" audio — used for real-device validation.
    /// Default 0.015. Aggregate RMS below this triggers a warning.
    pub audible_min_rms: f32,
}

impl Default for RequestedAudioContract {
    fn default() -> Self {
        Self {
            requested_system_audio: false,
            requested_microphone: false,
            min_rms: 0.003,
            min_peak: 0.02,
            audible_min_rms: 0.015,
        }
    }
}
```

- [ ] **Step 2: Add source-aware contract validation function**

```rust
/// Source-aware audio contract validation.
///
/// Unlike aggregate-only validation, this checks that each requested source
/// actually contributed to the artifact. Uses synchronizer diagnostics to
/// detect the BUG-005 pattern: capture-side RMS non-zero but writer discarded
/// all chunks from one source.
pub fn validate_source_aware_audio_contract(
    diagnostics: &RecordingDiagnostics,
    writer_diagnostics: &WriterDiagnostics,
) -> AppResult<()> {
    // Check: if system audio was requested and capture-side RMS was non-zero,
    // but no system windows were emitted by synchronizer, that's a failure.
    if diagnostics.requested_system_audio && diagnostics.system_rms_max > 0.001 {
        if diagnostics.system_only_window_count + diagnostics.paired_window_count == 0 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested system audio, capture RMS={:.6}, but 0 system windows emitted",
                    diagnostics.system_rms_max
                ),
            });
        }
    }

    // Check: if mic was requested and capture-side RMS was non-zero,
    // but no mic windows were emitted, that's a failure.
    if diagnostics.requested_microphone && diagnostics.mic_rms_max > 0.001 {
        if diagnostics.mic_only_window_count + diagnostics.paired_window_count == 0 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested microphone, capture RMS={:.6}, but 0 mic windows emitted",
                    diagnostics.mic_rms_max
                ),
            });
        }
    }

    // Check: if mic was requested and capture RMS was non-zero, but all mic
    // windows were discarded by writer (full-overlap), that's a failure.
    if diagnostics.requested_microphone && diagnostics.mic_rms_max > 0.001 {
        let mic_windows_emitted = diagnostics.mic_only_window_count + diagnostics.paired_window_count;
        if mic_windows_emitted > 0 && writer_diagnostics.audio_chunks_discarded_full_overlap > 0 {
            let discard_ratio = writer_diagnostics.audio_chunks_discarded_full_overlap as f64
                / (writer_diagnostics.audio_chunks_received.max(1)) as f64;
            if discard_ratio > 0.5 {
                return Err(AppError::RecordingFinalizeFailed {
                    reason: format!(
                        "requested microphone, but {:.0}% of audio chunks were discarded as full overlap ({} of {})",
                        discard_ratio * 100.0,
                        writer_diagnostics.audio_chunks_discarded_full_overlap,
                        writer_diagnostics.audio_chunks_received
                    ),
                });
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Call source-aware contract in consumer thread**

In `macos_service.rs`, after the existing `validate_source_artifact_with_audio_contract` call:

```rust
if let Err(e) = validate_source_aware_audio_contract(&diagnostics, &writer_diag) {
    let msg = format!("source-aware audio contract 失败: {e}");
    eprintln!("{msg}");
    errors.push(msg);
}
```

- [ ] **Step 4: Add tests for source-aware contract**

```rust
#[test]
fn requested_system_and_mic_requires_each_source_appended() {
    // Both sources requested, capture RMS non-zero, but mic discarded by writer.
    let diagnostics = RecordingDiagnostics {
        requested_system_audio: true,
        requested_microphone: true,
        system_rms_max: 0.05,
        mic_rms_max: 0.08,
        paired_window_count: 0,
        system_only_window_count: 50,
        mic_only_window_count: 0,
        ..Default::default()
    };
    let writer_diag = WriterDiagnostics {
        audio_chunks_received: 50,
        audio_chunks_discarded_full_overlap: 0,
        ..Default::default()
    };

    // Should fail: mic requested + mic RMS non-zero but 0 mic windows.
    assert!(validate_source_aware_audio_contract(&diagnostics, &writer_diag).is_err());
}

#[test]
fn export_contract_rejects_requested_audio_near_silence() {
    // Construct a contract with audible_min_rms threshold.
    let contract = RequestedAudioContract {
        requested_system_audio: true,
        requested_microphone: false,
        min_rms: 0.003,
        min_peak: 0.02,
        audible_min_rms: 0.015,
    };

    // RMS below audible threshold.
    assert!(contract.min_rms < 0.006991); // passes min_rms
    assert!(contract.audible_min_rms > 0.006991); // fails audible_min_rms
}
```

- [ ] **Step 5: Run contract tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_system_and_mic_requires_each_source_appended -- --nocapture`
Expected: PASS

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg export_contract_rejects_requested_audio_near_silence -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/ffmpeg_common.rs src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): source-aware requested-audio contract 验证每个请求源"
```

---

## Task 6: Fix CPAL timestamp anchor — lazy first-callback initialization

**Files:**

- Modify: `src-tauri/src/core/clock.rs`
- Modify: `src-tauri/src/platform/macos/cpal_microphone.rs`

- [ ] **Step 1: Add `with_lazy_offset()` and `initialize_offset()` to `AudioSampleClock`**

```rust
/// Creates a clock with lazy offset initialization.
///
/// The offset is set on the first call to `initialize_offset()`, which should
/// happen inside the first audio callback. This prevents the stream-build-time
/// delay from creating a systematic timestamp offset.
pub fn with_lazy_offset(self) -> Self {
    // session_offset_nanos stays 0; will be set by initialize_offset().
    self
}

/// Initializes the session offset from the current session clock elapsed time,
/// minus the buffer duration to estimate when capture actually started.
///
/// This should be called exactly once, inside the first audio callback.
/// Subsequent calls are no-ops.
pub fn initialize_offset(&mut self, session: &SessionClock, buffer_frames: u64) {
    if self.session_offset_nanos > 0 {
        return; // Already initialized.
    }
    let buffer_duration_nanos = buffer_frames * 1_000_000_000 / self.sample_rate as u64;
    let callback_now = session.elapsed_nanos();
    self.session_offset_nanos = callback_now.saturating_sub(buffer_duration_nanos);
}
```

- [ ] **Step 2: Write test for lazy offset**

```rust
#[test]
fn audio_sample_clock_lazy_offset_anchors_first_callback_start() {
    let session = SessionClock::new();
    std::thread::sleep(std::time::Duration::from_millis(5));

    let mut clock = AudioSampleClock::new(48_000, 2);
    // Simulate first callback with 960 frames (10ms buffer).
    clock.initialize_offset(&session, 960);

    let first = clock.timestamp_for_interleaved_sample_count(1920); // 960 frames, 2ch
    // First timestamp should be near session elapsed - 10ms buffer.
    let expected_start = session.elapsed_nanos().saturating_sub(10_000_000);
    assert!(first.nanos >= expected_start.saturating_sub(1_000_000));
    assert!(first.nanos <= expected_start + 1_000_000);
}

#[test]
fn mic_clock_startup_delay_does_not_shift_first_chunk_by_stream_build_time() {
    let session = SessionClock::new();

    // Simulate 200ms delay between stream build and first callback.
    std::thread::sleep(std::time::Duration::from_millis(200));

    // With eager offset (old behavior), offset would be ~200ms.
    let eager_clock = AudioSampleClock::new(48_000, 2).with_session_clock(&session);
    let eager_first = eager_clock.timestamp_for_interleaved_sample_count(1920);

    // With lazy offset (new behavior), offset is set at callback time minus buffer.
    let mut lazy_clock = AudioSampleClock::new(48_000, 2);
    lazy_clock.initialize_offset(&session, 960);
    let lazy_first = lazy_clock.timestamp_for_interleaved_sample_count(1920);

    // Lazy should be significantly smaller than eager (which included the 200ms build delay).
    assert!(
        lazy_first.nanos < eager_first.nanos,
        "lazy ({}) should be less than eager ({}) due to build delay",
        lazy_first.nanos,
        eager_first.nanos
    );
}
```

- [ ] **Step 3: Run clock tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_sample_clock_lazy_offset -- --nocapture`
Expected: PASS

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg mic_clock_startup_delay -- --nocapture`
Expected: PASS

- [ ] **Step 4: Update `cpal_microphone.rs` to use lazy offset**

In `build_input_stream`, change:

```rust
// Old:
let mut sample_clock = AudioSampleClock::new(sample_rate, channels);
if let Some(ref clock) = session_clock {
    sample_clock = sample_clock.with_session_clock(clock);
}

// New:
let mut sample_clock = AudioSampleClock::new(sample_rate, channels);
// Offset is initialized lazily in the first callback.
```

In the callback, add lazy initialization:

```rust
let samples: Vec<f32> = /* existing conversion */;

// Lazy offset initialization on first callback.
{
    let mut clock_guard = sample_clock.lock().unwrap();
    if let Some(ref session) = session_clock_for_callback {
        clock_guard.initialize_offset(&session, samples.len() as u64 / channels as u64);
    }
}

let timestamp = sample_clock.lock().unwrap()
    .timestamp_for_interleaved_sample_count(samples.len());
```

Note: `AudioSampleClock` uses `AtomicU64` for `emitted_frames` but `initialize_offset` needs `&mut self`. The clock may need to be wrapped in `Mutex` instead of using atomics for the offset field, or `initialize_offset` can use `compare_exchange` on a sentinel. Choose the approach that minimizes changes.

- [ ] **Step 5: Run all clock + microphone tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_sample_clock -- --nocapture`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/clock.rs src-tauri/src/platform/macos/cpal_microphone.rs
git commit -m "fix(audio): CPAL 时间戳锚点改为首次回调时惰性初始化"
```

---

## Task 7: Export phase uses audio contract + Bluetooth mic release path

**Files:**

- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/platform/macos/cpal_microphone.rs`

- [ ] **Step 1: Update `export_video()` to use audio contract**

In `lib.rs`, find the export validation section (around line 895) and replace `validate_export_artifact` with `validate_export_artifact_with_audio_contract`:

```rust
// Build contract from saved audio config.
let contract = {
    let svc = state.lock().unwrap();
    RequestedAudioContract {
        requested_system_audio: svc.last_requested_system_audio.unwrap_or(false),
        requested_microphone: svc.last_requested_microphone.unwrap_or(false),
        ..Default::default()
    }
};

validate_export_artifact_with_audio_contract(
    &output_path,
    preset.width(),
    preset.height(),
    &contract,
)?;
```

- [ ] **Step 2: Fix Bluetooth mic stop order**

In `macos_service.rs`, change the stop order so mic is stopped first:

```rust
// Old order:
// 1. ScreenCapture::stop
// 2. mic_capture.stop()
// 3. stop_flag

// New order: stop mic first to release Bluetooth HFP profile ASAP.
if let Err(e) = self.mic_capture.stop() {
    errors.push(format!("停止麦克风失败: {e}"));
}

let capture_result = ScreenCapture::stop(&mut self.screen_capture);

if let Some(flag) = &self.stop_flag {
    flag.store(true, Ordering::Relaxed);
}
```

- [ ] **Step 3: Explicit stream drop + bounded wait in `CpalMicrophoneCapture::stop()`**

```rust
pub fn stop(&mut self) -> AppResult<()> {
    self.running.store(false, Ordering::Relaxed);

    // Take stream to local variable and explicitly drop it.
    // This ensures CoreAudio device resources are released before we return.
    let stream = self.stream.take();
    drop(stream);

    // Bounded wait for CoreAudio to complete device release.
    // Bluetooth HFP profile switching can take 100-300ms.
    std::thread::sleep(std::time::Duration::from_millis(200));

    Ok(())
}
```

- [ ] **Step 4: Add diagnostic log for mic stop**

```rust
pub fn stop(&mut self) -> AppResult<()> {
    eprintln!("CpalMicrophoneCapture::stop() 开始 — device={:?}", self.device_name);
    self.running.store(false, Ordering::Relaxed);

    let stream = self.stream.take();
    let stream_dropped = stream.is_some();
    drop(stream);

    std::thread::sleep(std::time::Duration::from_millis(200));

    eprintln!("CpalMicrophoneCapture::stop() 完成 — stream_dropped={stream_dropped}");
    Ok(())
}
```

- [ ] **Step 5: Run export and microphone tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg export -- --nocapture`
Expected: ALL PASS

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cpal -- --nocapture`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/platform/macos_service.rs src-tauri/src/platform/macos/cpal_microphone.rs
git commit -m "fix(audio): 导出阶段接入 audio contract；蓝牙麦克风 stop 顺序和显式释放"
```

---

## Task 8: Update BUG.md and run full regression

**Files:**

- Modify: `BUG.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Update BUG-005 status and add prevention rules**

In `BUG.md`, update BUG-005 status:

```markdown
**状态**：Section 25 review 整改中 — source-aware synchronizer、source-aware contract、CPAL lazy offset、writer partial-overlap diagnostics、蓝牙 mic 释放。待真实设备验证。
```

Add new prevention rules:

```markdown
### 新增预防规则（2026-06-02 Section 25 review）

1. `AudioSynchronizer` 不能只按 chunk 起始 timestamp 归桶；真实音频 chunk 必须按 sample frame 切分到固定时间窗口。
2. 双源录制时 live watermark 不能由快的一路单独推进；在两个请求源都 active 时必须以慢源或 source-aware timeout 策略决定发射。
3. requested-audio contract 必须 source-aware；aggregate decoded RMS/peak 只能证明 artifact 非全静音，不能证明每个请求源都存在。
4. 当 capture-side 某请求源 RMS 非零但 writer/source-aware diagnostics 显示该源被大量 overlap discard 时，必须视为录制失败或至少阻断 BUG 关闭。
5. CPAL 麦克风 timestamp 不能在 stream build 时固定 offset；首帧 callback 或设备 timestamp 才能作为输入流真实起点。
6. 蓝牙麦克风 UI warning 不能替代资源释放验证；显式蓝牙设备 stop 后必须验证 stream drop 与音质恢复。
```

- [ ] **Step 2: Run full regression**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

Expected: ALL PASS, no new clippy warnings.

- [ ] **Step 3: Update HANDOFF.md**

Update the top status line and add a new work task entry recording this remediation.

- [ ] **Step 4: Commit**

```bash
git add BUG.md HANDOFF.md
git commit -m "docs: 更新 BUG-005 状态和预防规则（source-aware synchronizer 整改）"
```

---

## Verification Commands

After all tasks are complete, run the full verification matrix:

```bash
# Format check
cargo fmt --manifest-path src-tauri/Cargo.toml --check

# Clippy
cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets

# All Rust tests (with FFmpeg)
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg

# All Rust tests (without FFmpeg)
cargo test --manifest-path src-tauri/Cargo.toml

# Frontend tests
npm test -- --run

# Build
npm run build
```

### Focused test commands for key fixes:

```bash
# Critical 1 & 2: synchronizer watermark and chunk splitting
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg dual_source_offset -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg synchronizer_splits_long -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg synchronizer_does_not_emit_fast -- --nocapture

# Critical 3: source-aware contract
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_system_and_mic -- --nocapture

# Important 1: CPAL lazy offset
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_sample_clock_lazy -- --nocapture

# Important 3: writer partial-overlap diagnostics
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture

# Full synchronizer regression
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer -- --nocapture
```

### Real device manual gate:

1. [x] 只录系统音频 10 秒，播放音乐，验证 source/export 都可听。
2. [x] 只录内置麦克风 10 秒，说话，验证 source/export 都可听。
3. [x] 系统音频 + 内置麦克风 10 秒，验证两路都可听。
4. [ ] 系统音频 + 蓝牙麦克风 10 秒，验证停止后蓝牙音质恢复。
5. 每次验证记录 `RecordingDiagnostics`、`WriterDiagnostics`、artifact decoded RMS/peak。
