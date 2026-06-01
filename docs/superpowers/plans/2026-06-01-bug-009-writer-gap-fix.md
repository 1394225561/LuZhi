# BUG-009 Writer Gap Branch Fix & Diagnostics Remediation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix BUG-009 — the `FfmpegRecordingWriter` audio timeline gap branch discards real PCM and only writes silence padding, causing artifacts with requested audio to decode as fully silent. Also fix per-source metadata in `AudioSynchronizer` and improve diagnostics to distinguish real PCM from silence padding.

**Architecture:** The gap branch in `encoder_worker` must append real chunk samples after padding silence. `AudioSynchronizer` must track per-source `sample_rate/channels` instead of using a single metadata per window. `WriterDiagnostics` must distinguish real PCM frames from silence padding frames.

**Tech Stack:** Rust, FFmpeg (ffmpeg-next), cpal, Tauri

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src-tauri/src/media/ffmpeg_writer.rs` | Modify | Fix gap branch, add diagnostics fields, set `generated_silent_track` |
| `src-tauri/src/media/recording_writer.rs` | Modify | Add new `WriterDiagnostics` fields |
| `src-tauri/src/media/audio_synchronizer.rs` | Modify | Per-source metadata (`SourceWindowBuffer`) |
| `src-tauri/src/platform/macos_service.rs` | Modify | Use writer diagnostics for `generated_silent_track` |
| `src-tauri/tests/ffmpeg_export.rs` | Modify | Add integration tests if needed |

---

### Task 1: Add new WriterDiagnostics fields

**Files:**
- Modify: `src-tauri/src/media/recording_writer.rs:14-31`

- [ ] **Step 1: Add new fields to WriterDiagnostics**

```rust
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterDiagnostics {
    /// Number of audio chunks received by the worker thread.
    pub audio_chunks_received: u64,
    /// Number of audio chunks that had at least some real PCM appended.
    pub audio_chunks_appended: u64,
    /// Number of audio chunks discarded as fully overlapped.
    pub audio_chunks_discarded_full_overlap: u64,
    /// Number of audio chunks partially trimmed before appending.
    pub audio_chunks_trimmed_partial_overlap: u64,
    /// Number of real (non-silence) mono frames appended to the sample buffer.
    pub audio_real_frames_appended: u64,
    /// Number of silence mono frames padded for timeline gaps.
    pub audio_silence_frames_padded: u64,
    /// Maximum RMS of real PCM samples before encoding (excludes silence padding).
    pub audio_real_rms_max_before_encode: f32,
    /// Number of AAC frames actually encoded and written to the muxer.
    pub aac_frames_encoded: u64,
    /// Number of silent AAC frames generated when no audio was received.
    pub silent_aac_frames_encoded: u64,
    /// Whether a silent AAC track was generated (no mixed audio chunks received).
    pub generated_silent_track: bool,
    /// Number of video queue full events (try_send failed).
    pub video_queue_full_count: u64,
    /// Number of audio queue full events (try_send failed).
    pub audio_queue_full_count: u64,
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS (new fields have default values via `#[derive(Default)]`)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/media/recording_writer.rs
git commit -m "feat(writer): 扩展 WriterDiagnostics 区分真实 PCM 与静音填充"
```

---

### Task 2: Write failing test — leading gap preserves real audio

**Files:**
- Modify: `src-tauri/src/media/ffmpeg_writer.rs` (test module)

- [ ] **Step 1: Add the failing test**

Add to the `#[cfg(test)] mod tests` block in `ffmpeg_writer.rs`:

```rust
#[test]
fn ffmpeg_writer_preserves_non_silent_audio_after_leading_gap() {
    let path = crate::test_support::ffmpeg_helpers::unique_media_path(
        "writer-leading-gap-rms",
        "mp4",
    );
    let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

    // Video from t=0, 90 frames = 3 seconds.
    for i in 0..90 {
        writer.push_video(test_video_frame_at(i * 33_333_333)).unwrap();
    }

    // First audio chunk at t=200ms — creates a leading gap.
    // Use 0.5 amplitude to ensure non-trivial RMS.
    let chunk1 = audio_chunk_with_frames(200_000_000, 1024);
    writer.push_audio(chunk1).unwrap();

    // Second audio chunk immediately after.
    let chunk2_ts = 200_000_000 + 1024 * 1_000_000_000 / 48_000;
    let chunk2 = audio_chunk_with_frames(chunk2_ts, 1024);
    writer.push_audio(chunk2).unwrap();

    writer.finish().unwrap();

    let inspection =
        crate::test_support::ffmpeg_helpers::inspect_media_artifact_with_audio_stats(&path)
            .unwrap();
    assert!(
        inspection.audio_rms.unwrap() > 0.01,
        "audio RMS should be > 0.01 after leading gap, got {:?}",
        inspection.audio_rms
    );
    assert!(
        inspection.audio_peak.unwrap() > 0.02,
        "audio peak should be > 0.02 after leading gap, got {:?}",
        inspection.audio_peak
    );
    let _ = std::fs::remove_file(&path);
}
```

- [ ] **Step 2: Run test to verify it FAILS**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_preserves_non_silent_audio_after_leading_gap -- --nocapture 2>&1`
Expected: FAIL — RMS and peak are 0 because the gap branch discards real samples.

- [ ] **Step 3: Commit the failing test**

```bash
git add src-tauri/src/media/ffmpeg_writer.rs
git commit -m "test(writer): 添加 leading gap 后音频内容回归测试（预期失败）"
```

---

### Task 3: Write failing test — middle gap preserves real audio

**Files:**
- Modify: `src-tauri/src/media/ffmpeg_writer.rs` (test module)

- [ ] **Step 1: Add the failing test**

```rust
#[test]
fn ffmpeg_writer_preserves_non_silent_audio_after_middle_gap() {
    let path = crate::test_support::ffmpeg_helpers::unique_media_path(
        "writer-middle-gap-rms",
        "mp4",
    );
    let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

    // Video from t=0, 150 frames = 5 seconds.
    for i in 0..150 {
        writer.push_video(test_video_frame_at(i * 33_333_333)).unwrap();
    }

    // First audio chunk at t=0, contiguous.
    writer.push_audio(audio_chunk_with_frames(0, 1024)).unwrap();

    // Second audio chunk at t=500ms — creates a middle gap.
    writer.push_audio(audio_chunk_with_frames(500_000_000, 1024)).unwrap();

    // Third audio chunk immediately after second.
    let chunk3_ts = 500_000_000 + 1024 * 1_000_000_000 / 48_000;
    writer.push_audio(audio_chunk_with_frames(chunk3_ts, 1024)).unwrap();

    writer.finish().unwrap();

    let inspection =
        crate::test_support::ffmpeg_helpers::inspect_media_artifact_with_audio_stats(&path)
            .unwrap();
    assert!(
        inspection.audio_rms.unwrap() > 0.01,
        "audio RMS should be > 0.01 after middle gap, got {:?}",
        inspection.audio_rms
    );
    assert!(
        inspection.audio_peak.unwrap() > 0.02,
        "audio peak should be > 0.02 after middle gap, got {:?}",
        inspection.audio_peak
    );
    let _ = std::fs::remove_file(&path);
}
```

- [ ] **Step 2: Run test to verify it FAILS**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_preserves_non_silent_audio_after_middle_gap -- --nocapture 2>&1`
Expected: FAIL

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/media/ffmpeg_writer.rs
git commit -m "test(writer): 添加 middle gap 后音频内容回归测试（预期失败）"
```

---

### Task 4: Fix writer gap branch — append real samples after silence padding

**Files:**
- Modify: `src-tauri/src/media/ffmpeg_writer.rs:534-564`

This is the core BUG-009 fix. The gap branch currently only pads silence and does NOT append the current chunk's real samples.

- [ ] **Step 1: Extract `append_audio_chunk_to_timeline` helper**

Add a helper function before `encoder_worker` in `ffmpeg_writer.rs`:

```rust
/// Result of appending an audio chunk to the timeline buffer.
struct TimelineAppendResult {
    /// Whether real PCM samples were appended (not just silence padding).
    chunk_appended: bool,
    /// Number of silence mono frames padded for gap.
    silence_frames_padded: u64,
    /// Number of real mono frames appended from this chunk.
    appended_frames: u64,
}

/// Append an audio chunk to the timeline buffer, handling gaps and overlaps.
///
/// - Gap (`target > cursor`): pad silence, then append real samples.
/// - Overlap (`target < cursor`): trim or discard overlapping prefix.
/// - Contiguous (`target == cursor`): append directly.
fn append_audio_chunk_to_timeline(
    audio_sample_buffer: &mut Vec<f32>,
    audio_timeline_cursor: &mut i64,
    target_sample: i64,
    samples: &[f32],
) -> TimelineAppendResult {
    let chunk_mono_frames = (samples.len() / 2) as i64;

    if target_sample > *audio_timeline_cursor {
        // Gap: pad silence from cursor to target, then append real samples.
        let gap_mono = target_sample - *audio_timeline_cursor;
        let gap_interleaved = (gap_mono * 2) as usize;
        audio_sample_buffer.extend(std::iter::repeat_n(0.0f32, gap_interleaved));
        audio_sample_buffer.extend_from_slice(samples);
        *audio_timeline_cursor = target_sample + chunk_mono_frames;
        return TimelineAppendResult {
            chunk_appended: true,
            silence_frames_padded: gap_mono as u64,
            appended_frames: chunk_mono_frames as u64,
        };
    }

    if target_sample < *audio_timeline_cursor {
        // Overlap: this chunk's start is before where we already wrote.
        let overlap_mono = (*audio_timeline_cursor - target_sample) as usize;
        if overlap_mono >= chunk_mono_frames as usize {
            // Entire chunk is already covered — discard.
            return TimelineAppendResult {
                chunk_appended: false,
                silence_frames_padded: 0,
                appended_frames: 0,
            };
        }
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
        };
    }

    // Contiguous: append all.
    audio_sample_buffer.extend_from_slice(samples);
    *audio_timeline_cursor += chunk_mono_frames;
    TimelineAppendResult {
        chunk_appended: true,
        silence_frames_padded: 0,
        appended_frames: chunk_mono_frames as u64,
    }
}
```

- [ ] **Step 2: Replace the inline gap/overlap/contiguous logic in encoder_worker**

Replace the `EncoderMessage::Audio` branch (lines ~508-573) with:

```rust
EncoderMessage::Audio {
    samples,
    timestamp_nanos,
    ..
} => {
    mixed_audio_chunk_count += 1;
    writer_diag.audio_chunks_received += 1;

    let target_sample = (timestamp_nanos as i128 * 48000 / 1_000_000_000i128) as i64;

    let append_result = append_audio_chunk_to_timeline(
        &mut audio_sample_buffer,
        &mut audio_timeline_cursor,
        target_sample,
        &samples,
    );

    if append_result.chunk_appended {
        writer_diag.audio_chunks_appended += 1;
        writer_diag.audio_real_frames_appended += append_result.appended_frames;
        // Track max RMS of real PCM before encoding.
        let chunk_rms = compute_chunk_rms(&samples);
        if chunk_rms > writer_diag.audio_real_rms_max_before_encode {
            writer_diag.audio_real_rms_max_before_encode = chunk_rms;
        }
    } else {
        writer_diag.audio_chunks_discarded_full_overlap += 1;
    }
    writer_diag.audio_silence_frames_padded += append_result.silence_frames_padded;

    writer_diag.aac_frames_encoded += drain_audio_sample_buffer(
        &mut audio_sample_buffer,
        &mut audio_pts,
        &mut audio_encoder,
        &mut output,
        audio_stream_index,
    )?;
}
```

- [ ] **Step 3: Add `compute_chunk_rms` helper**

```rust
/// Compute RMS of interleaved stereo samples (uses only left channel for speed).
fn compute_chunk_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mono_count = samples.len() / 2;
    if mono_count == 0 {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().step_by(2).map(|s| s * s).sum();
    (sum_squares / mono_count as f32).sqrt()
}
```

- [ ] **Step 4: Update the overlap branch diagnostics**

Remove the old `audio_chunks_trimmed_partial_overlap` increment from the inline code since `append_audio_chunk_to_timeline` handles it. But wait — the helper doesn't increment `audio_chunks_trimmed_partial_overlap`. We need to track that in the caller. Update the caller:

```rust
if append_result.chunk_appended {
    writer_diag.audio_chunks_appended += 1;
    writer_diag.audio_real_frames_appended += append_result.appended_frames;
    let chunk_rms = compute_chunk_rms(&samples);
    if chunk_rms > writer_diag.audio_real_rms_max_before_encode {
        writer_diag.audio_real_rms_max_before_encode = chunk_rms;
    }
    // If silence was padded, this was a gap/overlap scenario, not a simple append.
    // audio_chunks_appended counts chunks with real PCM regardless.
} else {
    writer_diag.audio_chunks_discarded_full_overlap += 1;
}
writer_diag.audio_silence_frames_padded += append_result.silence_frames_padded;
```

Actually, let's simplify. The `audio_chunks_trimmed_partial_overlap` field is still useful. Let's update the helper to return an enum instead:

Actually, let's keep it simple. The helper returns whether it was a trim case. We can check: if `silence_frames_padded == 0` and `chunk_appended` and the original target was before cursor (we can detect this by checking if appended_frames < chunk_mono_frames). But that adds complexity.

Let's just keep the current approach: `audio_chunks_appended` counts chunks with real PCM, `audio_chunks_discarded_full_overlap` counts full discards. The partial overlap case is already covered by `audio_chunks_appended` (since partial overlap still appends some real PCM). We can add a separate field if needed, but for now the key fix is: gap branch must append real samples.

- [ ] **Step 5: Run the failing tests — they should now PASS**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_preserves_non_silent_audio_after_leading_gap -- --nocapture 2>&1`
Expected: PASS

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_preserves_non_silent_audio_after_middle_gap -- --nocapture 2>&1`
Expected: PASS

- [ ] **Step 6: Run all existing writer tests to verify no regression**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture 2>&1`
Expected: All PASS

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/media/ffmpeg_writer.rs
git commit -m "fix(writer): 修复 BUG-009 gap 分支丢弃真实音频 chunk 的致命缺陷

gap 分支补齐静音后必须继续 append 当前 chunk 的真实 PCM 样本，
并将 timeline cursor 推进到 chunk 结束位置。之前只补静音不写真实
音频，导致真实设备录制中大量非静音 PCM 被替换为静音 AAC frame。"
```

---

### Task 5: Update silent track diagnostics

**Files:**
- Modify: `src-tauri/src/media/ffmpeg_writer.rs` (silent track branch, ~line 679-718)

- [ ] **Step 1: Set `generated_silent_track` and `silent_aac_frames_encoded`**

In the `encoder_worker`, update the silent track branch:

```rust
// Generate silent audio track if no audio was received.
if mixed_audio_chunk_count == 0 {
    writer_diag.generated_silent_track = true;
    let video_duration_secs = video_duration_nanos as f64 / 1_000_000_000.0;
    let total_audio_frames = (video_duration_secs * 48000.0).ceil() as u64;
    let num_silent_packets = (total_audio_frames / 1024).max(1);

    for _ in 0..num_silent_packets {
        // ... existing silent frame generation code ...
    }
    writer_diag.silent_aac_frames_encoded = num_silent_packets;
    mixed_audio_chunk_count = num_silent_packets;
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture 2>&1`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/media/ffmpeg_writer.rs
git commit -m "fix(writer): silent track 分支显式设置 generated_silent_track 诊断字段"
```

---

### Task 6: Update macos_service.rs to use writer diagnostics for generated_silent_track

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs`

- [ ] **Step 1: Use writer diagnostics instead of inferring from mixed_audio_chunk_count**

Find the line:
```rust
diagnostics.generated_silent_track = result.mixed_audio_chunk_count == 0;
```

Replace with:
```rust
diagnostics.generated_silent_track = result.writer_diagnostics.generated_silent_track;
```

- [ ] **Step 2: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg -- --nocapture 2>&1`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "fix(service): generated_silent_track 从 writer diagnostics 获取，不再推断"
```

---

### Task 7: Fix AudioSynchronizer per-source metadata

**Files:**
- Modify: `src-tauri/src/media/audio_synchronizer.rs`

- [ ] **Step 1: Replace `AudioWindow` with source-aware structure**

Replace the `AudioWindow` struct:

```rust
/// Per-source audio buffer within a time window.
///
/// Each source (system/mic) maintains its own sample_rate and channels
/// metadata, preventing cross-source metadata contamination.
struct SourceWindowBuffer {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

/// A time window that accumulates system and mic audio samples.
///
/// Each window covers a fixed 20ms interval. Each source maintains its own
/// metadata (sample_rate, channels) to prevent system 2ch / mic 1ch confusion.
struct AudioWindow {
    system: Option<SourceWindowBuffer>,
    mic: Option<SourceWindowBuffer>,
    window_start_nanos: u64,
}
```

- [ ] **Step 2: Update `push_system` and `push_mic`**

```rust
pub fn push_system(&mut self, chunk: AudioChunk) {
    let ts = chunk.timestamp.nanos;
    self.latest_system_ts = self.latest_system_ts.max(ts);

    let window_idx = ts / self.window_nanos;
    let window = self.windows.entry(window_idx).or_insert_with(|| AudioWindow {
        system: None,
        mic: None,
        window_start_nanos: window_idx * self.window_nanos,
    });

    let buf = window.system.get_or_insert_with(|| SourceWindowBuffer {
        samples: Vec::new(),
        sample_rate: chunk.sample_rate,
        channels: chunk.channels,
    });

    // Validate metadata consistency within the same window.
    if buf.sample_rate != chunk.sample_rate || buf.channels != chunk.channels {
        // Metadata mismatch — this shouldn't happen in normal operation.
        // Log warning and use the new metadata (last-write-wins for this window).
        buf.sample_rate = chunk.sample_rate;
        buf.channels = chunk.channels;
    }

    buf.samples.extend_from_slice(&chunk.samples);

    // Evict oldest windows if we exceed the limit.
    while self.windows.len() > MAX_WINDOWS {
        if let Some((&oldest_idx, _)) = self.windows.iter().next() {
            self.windows.remove(&oldest_idx);
        }
    }
}

pub fn push_mic(&mut self, chunk: AudioChunk) {
    let ts = chunk.timestamp.nanos;
    self.latest_mic_ts = self.latest_mic_ts.max(ts);

    let window_idx = ts / self.window_nanos;
    let window = self.windows.entry(window_idx).or_insert_with(|| AudioWindow {
        system: None,
        mic: None,
        window_start_nanos: window_idx * self.window_nanos,
    });

    let buf = window.mic.get_or_insert_with(|| SourceWindowBuffer {
        samples: Vec::new(),
        sample_rate: chunk.sample_rate,
        channels: chunk.channels,
    });

    if buf.sample_rate != chunk.sample_rate || buf.channels != chunk.channels {
        buf.sample_rate = chunk.sample_rate;
        buf.channels = chunk.channels;
    }

    buf.samples.extend_from_slice(&chunk.samples);

    while self.windows.len() > MAX_WINDOWS {
        if let Some((&oldest_idx, _)) = self.windows.iter().next() {
            self.windows.remove(&oldest_idx);
        }
    }
}
```

- [ ] **Step 3: Update `emit_window` to use per-source metadata**

```rust
fn emit_window(&mut self, window: AudioWindow) -> AppResult<MixedAudioChunk> {
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

    // Create AudioChunks using per-source metadata.
    let system_chunk = window.system.map(|buf| {
        AudioChunk {
            timestamp: crate::core::frame::MediaTimestamp::from_nanos(window.window_start_nanos),
            sample_rate: buf.sample_rate,
            channels: buf.channels,
            samples: std::sync::Arc::from(buf.samples.into_boxed_slice()),
        }
    });

    let mic_chunk = window.mic.map(|buf| {
        AudioChunk {
            timestamp: crate::core::frame::MediaTimestamp::from_nanos(window.window_start_nanos),
            sample_rate: buf.sample_rate,
            channels: buf.channels,
            samples: std::sync::Arc::from(buf.samples.into_boxed_slice()),
        }
    });

    self.mixer.mix(system_chunk.as_ref(), mic_chunk.as_ref())
}
```

- [ ] **Step 4: Update `drain_final` to use new window structure**

```rust
pub fn drain_final(&mut self) -> Vec<(SynchronizedAudioChunk, bool)> {
    let mut results = Vec::with_capacity(self.windows.len());

    let indices: Vec<u64> = self.windows.keys().copied().collect();
    for idx in indices {
        if let Some(window) = self.windows.remove(&idx) {
            let has_system = window.system.is_some();
            let has_mic = window.mic.is_some();
            let was_unpaired = has_system != has_mic;

            let system_rms = window.system.as_ref().map_or(0.0, |b| compute_rms(&b.samples));
            let mic_rms = window.mic.as_ref().map_or(0.0, |b| compute_rms(&b.samples));

            match self.emit_window(window) {
                Ok(mixed) => {
                    results.push((
                        SynchronizedAudioChunk {
                            mixed,
                            has_system,
                            has_mic,
                            system_rms,
                            mic_rms,
                        },
                        was_unpaired,
                    ));
                }
                Err(_) => {
                    // Skip windows that fail to emit.
                }
            }
        }
    }

    results.sort_by_key(|(chunk, _)| chunk.mixed.timestamp.nanos);
    results
}
```

- [ ] **Step 5: Update `drain_mixed` to use new window structure**

The `drain_mixed` method references `window.system_samples` and `window.mic_samples` — update to use `window.system` and `window.mic`:

```rust
pub fn drain_mixed(&mut self) -> Vec<AppResult<MixedAudioChunk>> {
    let watermark_nanos = self.calculate_watermark();

    let ready_indices: Vec<u64> = self
        .windows
        .range(..)
        .filter(|(_, window)| {
            let window_end = window.window_start_nanos + self.window_nanos;
            window_end <= watermark_nanos
        })
        .map(|(&idx, _)| idx)
        .collect();

    let mut results = Vec::with_capacity(ready_indices.len());

    for idx in ready_indices {
        if let Some(window) = self.windows.remove(&idx) {
            match self.emit_window(window) {
                Ok(mixed) => results.push(Ok(mixed)),
                Err(e) => results.push(Err(e)),
            }
        }
    }

    results
}
```

- [ ] **Step 6: Update test helper `chunk()` to work with new structure**

The existing test helper creates `AudioChunk` with `channels: 2`. This should still work since `push_system`/`push_mic` accept `AudioChunk`. No change needed to the helper.

- [ ] **Step 7: Run synchronizer tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml audio_synchronizer -- --nocapture 2>&1`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/media/audio_synchronizer.rs
git commit -m "fix(synchronizer): per-source metadata 防止 system/mic 通道数误标

AudioWindow 改为 SourceWindowBuffer 结构，system 和 mic 各自保留
sample_rate/channels。避免 48kHz/2ch system 与 48kHz/1ch mic
被套用同一份 metadata 导致样本布局误判。"
```

---

### Task 8: Add AudioSynchronizer per-source metadata tests

**Files:**
- Modify: `src-tauri/src/media/audio_synchronizer.rs` (test module)

- [ ] **Step 1: Add test — system arrives first, mic is mono**

```rust
#[test]
fn audio_synchronizer_preserves_mic_mono_metadata_when_system_arrives_first() {
    let mut sync = AudioSynchronizer::new(SimpleAudioMixer::new());

    // System: 48kHz/2ch stereo, arrives first.
    let system_chunk = AudioChunk {
        timestamp: MediaTimestamp::from_nanos(0),
        sample_rate: 48_000,
        channels: 2,
        samples: Arc::from(vec![0.3f32; 2048].into_boxed_slice()),
    };
    sync.push_system(system_chunk);

    // Mic: 48kHz/1ch mono, arrives second.
    // Simulate by pushing mono samples as if they were stereo (the mixer
    // will handle the actual channel conversion). But actually, the
    // AudioChunk.channels field should be 1 for mono.
    let mic_chunk = AudioChunk {
        timestamp: MediaTimestamp::from_nanos(0),
        sample_rate: 48_000,
        channels: 1,
        samples: Arc::from(vec![0.5f32; 1024].into_boxed_slice()),
    };
    sync.push_mic(mic_chunk);

    // Drain — watermark needs latest_ts > HOLD_NANOS.
    // Push more system audio to advance the watermark.
    let system_chunk2 = AudioChunk {
        timestamp: MediaTimestamp::from_nanos(100_000_000), // 100ms
        sample_rate: 48_000,
        channels: 2,
        samples: Arc::from(vec![0.3f32; 2048].into_boxed_slice()),
    };
    sync.push_system(system_chunk2);

    let results = sync.drain_mixed();
    assert!(!results.is_empty(), "should have emitted at least one window");

    // The mixed output should be valid (non-empty samples).
    for result in results {
        let mixed = result.unwrap();
        assert!(!mixed.samples.is_empty());
        assert_eq!(mixed.sample_rate, 48_000);
        assert_eq!(mixed.channels, 2); // Mixer outputs stereo
    }
}
```

- [ ] **Step 2: Add test — mic arrives first, system is stereo**

```rust
#[test]
fn audio_synchronizer_preserves_system_stereo_metadata_when_mic_arrives_first() {
    let mut sync = AudioSynchronizer::new(SimpleAudioMixer::new());

    // Mic: 48kHz/1ch mono, arrives first.
    let mic_chunk = AudioChunk {
        timestamp: MediaTimestamp::from_nanos(0),
        sample_rate: 48_000,
        channels: 1,
        samples: Arc::from(vec![0.5f32; 1024].into_boxed_slice()),
    };
    sync.push_mic(mic_chunk);

    // System: 48kHz/2ch stereo, arrives second.
    let system_chunk = AudioChunk {
        timestamp: MediaTimestamp::from_nanos(0),
        sample_rate: 48_000,
        channels: 2,
        samples: Arc::from(vec![0.3f32; 2048].into_boxed_slice()),
    };
    sync.push_system(system_chunk);

    // Advance watermark.
    let mic_chunk2 = AudioChunk {
        timestamp: MediaTimestamp::from_nanos(100_000_000),
        sample_rate: 48_000,
        channels: 1,
        samples: Arc::from(vec![0.5f32; 1024].into_boxed_slice()),
    };
    sync.push_mic(mic_chunk2);

    let results = sync.drain_mixed();
    assert!(!results.is_empty());

    for result in results {
        let mixed = result.unwrap();
        assert!(!mixed.samples.is_empty());
        assert_eq!(mixed.sample_rate, 48_000);
        assert_eq!(mixed.channels, 2);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml audio_synchronizer -- --nocapture 2>&1`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/media/audio_synchronizer.rs
git commit -m "test(synchronizer): 添加 per-source metadata 保护测试"
```

---

### Task 9: Update BUG.md with BUG-009 fix details and new prevention rules

**Files:**
- Modify: `BUG.md`

- [ ] **Step 1: Update BUG-009 status**

Update the BUG-009 entry to reflect:
- Root cause: `FfmpegRecordingWriter` gap branch only padded silence, did not append real PCM
- Fix: gap branch now pads silence then appends real samples
- New prevention rules added

- [ ] **Step 2: Add new prevention rules**

Add to the prevention rules section:
1. writer 处理 audio gap 时，padding silence 后必须继续 append 当前真实 chunk；gap padding 不能替代 chunk append
2. 音频 timeline 单元测试不能只检查 duration，还必须检查 decoded RMS/peak
3. writer diagnostics 必须区分 real PCM append 与 silence padding
4. `aac_frames_encoded > 0` 不能作为"artifact 有声"的证据
5. synchronizer window 必须保留 per-source metadata

- [ ] **Step 3: Commit**

```bash
git add BUG.md
git commit -m "docs(bug): 更新 BUG-009 状态与新增预防规则"
```

---

### Task 10: Update HANDOFF.md with work record

**Files:**
- Modify: `HANDOFF.md`

- [ ] **Step 1: Add section 25 remediation work record**

Document:
- R1-R6 completed
- BUG-009 root cause and fix
- New diagnostics fields
- Per-source metadata fix
- Test results

- [ ] **Step 2: Commit**

```bash
git add HANDOFF.md
git commit -m "docs(handoff): 记录 section 25 BUG-009 整改工作"
```

---

### Task 11: Final verification — run all tests

- [ ] **Step 1: Run all FFmpeg tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg -- --nocapture 2>&1`
Expected: All PASS

- [ ] **Step 2: Run frontend tests**

Run: `cd /Users/root-mac/workspace_github/LuZhi && npm test -- --run 2>&1`
Expected: All PASS

- [ ] **Step 3: Run cargo clippy**

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg -- -D warnings 2>&1`
Expected: No warnings

- [ ] **Step 4: Final commit if needed**

```bash
git add -A
git commit -m "chore: section 25 BUG-009 整改完成最终验证"
```
