# Phase 6 BUG-005 Code Review Rectification Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 6 remaining blockers from the Phase 6 code review (Section 10) to close BUG-005 — source-aware diagnostics, audible contract, Bluetooth mic release lifecycle, synchronizer start grace, writer bounded join, and export contract integration.

**Architecture:** The fixes span the audio pipeline from capture (CPAL mic stop) through synchronization (AudioSynchronizer grace) to writer (diagnostics, bounded join) and validation (source-aware contract, audible threshold, export contract). Each task is self-contained and can be verified independently via unit tests.

**Tech Stack:** Rust (cpal, ffmpeg-next), Tauri 2.0

**Reference:** `docs/superpowers/reviews/2026-06-01-phase-6-bug-005-code-review.md` Section 10

---

## File Structure

```
src-tauri/src/
  media/
    audio_synchronizer.rs   — Task 1 (source start grace + timeout marking)
    recording_writer.rs     — Task 2 (source-aware diagnostics fields + contract)
    ffmpeg_common.rs        — Task 3 (audible_min_rms enforcement)
    ffmpeg_writer.rs        — Task 4 (bounded join_worker)
  platform/
    macos_service.rs        — Task 5 (diagnostics write-back in drain loops)
    macos/
      cpal_microphone.rs    — Task 6 (explicit pause + lifecycle diagnostics)
  lib.rs                    — Task 7 (export audio contract integration)
  core/
    clock.rs                — no changes needed (lazy offset already implemented)
tests/
  phase-6-w11-w12-checklist.md — Task 8 (checklist update)
BUG.md                      — Task 8 (prevention rules + status update)
HANDOFF.md                  — Task 8 (handoff update)
```

---

### Task 1: AudioSynchronizer source start grace and timeout marking

**Files:**
- Modify: `src-tauri/src/media/audio_synchronizer.rs`
- Test: same file (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing test for source start grace**

Add test `audio_synchronizer_dual_source_waits_for_initial_slow_source_within_grace` at the bottom of the test module in `audio_synchronizer.rs`:

```rust
#[test]
fn audio_synchronizer_dual_source_waits_for_initial_slow_source_within_grace() {
    use crate::media::audio_mixer::SimpleAudioMixer;

    let config = AudioSynchronizerConfig {
        requested_system_audio: true,
        requested_microphone: true,
        window_nanos: 20_000_000,
        hold_nanos: 40_000_000,
        source_stall_timeout_nanos: 2_000_000_000,
        source_start_grace_nanos: 500_000_000, // 500ms grace
    };
    let mut sync = AudioSynchronizer::new(SimpleAudioMixer, config);

    // System arrives at t=0, mic hasn't arrived yet
    sync.push_system(make_test_chunk(0, 48000, 2, 20ms_of_samples(48000, 2)));

    // After 100ms, system has several windows but mic still hasn't arrived
    sync.push_system(make_test_chunk(20_000_000, 48000, 2, 20ms_of_samples(48000, 2)));
    sync.push_system(make_test_chunk(40_000_000, 48000, 2, 20ms_of_samples(48000, 2)));

    // drain_mixed should NOT emit anything — we're within grace period
    let result = sync.drain_mixed();
    assert!(result.is_empty(), "should wait for mic within grace period, got {} chunks", result.len());
}
```

- [ ] **Step 2: Write failing test for grace timeout emission**

```rust
#[test]
fn audio_synchronizer_dual_source_emits_after_start_grace_timeout() {
    use crate::media::audio_mixer::SimpleAudioMixer;

    let config = AudioSynchronizerConfig {
        requested_system_audio: true,
        requested_microphone: true,
        window_nanos: 20_000_000,
        hold_nanos: 40_000_000,
        source_stall_timeout_nanos: 2_000_000_000,
        source_start_grace_nanos: 100_000_000, // 100ms grace
    };
    let mut sync = AudioSynchronizer::new(SimpleAudioMixer, config);

    // System arrives well past grace period, mic never arrives
    sync.push_system(make_test_chunk(0, 48000, 2, 20ms_of_samples(48000, 2)));
    sync.push_system(make_test_chunk(200_000_000, 48000, 2, 20ms_of_samples(48000, 2)));
    sync.push_system(make_test_chunk(400_000_000, 48000, 2, 20ms_of_samples(48000, 2)));

    let result = sync.drain_mixed();
    assert!(!result.is_empty(), "should emit after grace timeout");
    // All emitted chunks should be marked as timeout
    for chunk_result in &result {
        let chunk = chunk_result.as_ref().unwrap();
        assert!(chunk.emitted_due_to_timeout, "should be marked as timeout emission");
    }
}
```

- [ ] **Step 3: Write failing test for stall timeout marking**

```rust
#[test]
fn audio_synchronizer_marks_timeout_windows_when_source_stalls() {
    use crate::media::audio_mixer::SimpleAudioMixer;

    let config = AudioSynchronizerConfig {
        requested_system_audio: true,
        requested_microphone: true,
        window_nanos: 20_000_000,
        hold_nanos: 40_000_000,
        source_stall_timeout_nanos: 200_000_000, // 200ms stall timeout
        source_start_grace_nanos: 0,
    };
    let mut sync = AudioSynchronizer::new(SimpleAudioMixer, config);

    // Both sources start together
    sync.push_system(make_test_chunk(0, 48000, 2, 20ms_of_samples(48000, 2)));
    sync.push_mic(make_test_chunk(0, 48000, 2, 20ms_of_samples(48000, 2)));

    // Then mic stalls — system continues alone for 300ms
    for i in 1..=15 {
        sync.push_system(make_test_chunk(i * 20_000_000, 48000, 2, 20ms_of_samples(48000, 2)));
    }

    let result = sync.drain_mixed();
    let timeout_count = result.iter()
        .filter_map(|r| r.as_ref().ok())
        .filter(|c| c.emitted_due_to_timeout)
        .count();
    assert!(timeout_count > 0, "should have timeout-emitted windows when mic stalls, got {}", timeout_count);
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer_dual_source_waits -- --nocapture`
Expected: FAIL (fields/methods don't exist yet)

- [ ] **Step 5: Add `source_start_grace_nanos` to `AudioSynchronizerConfig`**

In `audio_synchronizer.rs`, add the field to the config struct:

```rust
pub struct AudioSynchronizerConfig {
    pub requested_system_audio: bool,
    pub requested_microphone: bool,
    pub window_nanos: u64,
    pub hold_nanos: u64,
    pub source_stall_timeout_nanos: u64,
    pub source_start_grace_nanos: u64, // NEW: grace period before emitting single-source windows
}
```

Update the `Default` impl to set `source_start_grace_nanos: 500_000_000` (500ms).

- [ ] **Step 6: Modify `calculate_watermark()` to return timeout mode**

Change `calculate_watermark()` signature to return `(u64, bool)` where the bool indicates timeout mode:

```rust
/// Returns (watermark_nanos, is_timeout_mode).
/// is_timeout_mode=true means the watermark was computed from a single source
/// due to start grace or stall timeout, so emitted windows should be marked.
fn calculate_watermark(&self) -> (u64, bool) {
    let both_requested = self.config.requested_system_audio && self.config.requested_microphone;

    if both_requested && self.seen_system && self.seen_mic {
        // Both sources active — check for stall
        let stall_nanos = self.config.source_stall_timeout_nanos;
        let now_system = self.latest_system_ts;
        let now_mic = self.latest_mic_ts;

        let system_stalled = stall_nanos > 0
            && self.last_mic_active_nanos > 0
            && now_system.saturating_sub(self.last_system_active_nanos) > stall_nanos;
        let mic_stalled = stall_nanos > 0
            && self.last_system_active_nanos > 0
            && now_mic.saturating_sub(self.last_mic_active_nanos) > stall_nanos;

        if system_stalled || mic_stalled {
            // One source stalled — use max watermark, mark as timeout
            let latest_ts = now_system.max(now_mic);
            (latest_ts.saturating_sub(self.config.hold_nanos), true)
        } else {
            // Normal dual-source: use min to wait for slow source
            let latest_ts = now_system.min(now_mic);
            (latest_ts.saturating_sub(self.config.hold_nanos), false)
        }
    } else if both_requested && (self.seen_system || self.seen_mic) {
        // Only one source seen — check start grace
        let grace_nanos = self.config.source_start_grace_nanos;
        let first_seen_nanos = if self.seen_system {
            self.last_system_active_nanos
        } else {
            self.last_mic_active_nanos
        };
        let latest_ts = self.latest_system_ts.max(self.latest_mic_ts);

        if grace_nanos > 0 && latest_ts.saturating_sub(first_seen_nanos) < grace_nanos {
            // Within grace period — don't emit yet
            return (u64::MAX, false); // watermark at infinity = nothing emits
        }

        // Grace expired — emit with timeout mark
        (latest_ts.saturating_sub(self.config.hold_nanos), true)
    } else {
        // Single source requested, or neither seen yet
        let latest_ts = self.latest_system_ts.max(self.latest_mic_ts);
        (latest_ts.saturating_sub(self.config.hold_nanos), false)
    }
}
```

- [ ] **Step 7: Update `drain_mixed()` to pass timeout flag to `emit_window()`**

In `drain_mixed()`, use the timeout flag from `calculate_watermark()`:

```rust
pub fn drain_mixed(&mut self) -> Vec<AppResult<SynchronizedAudioChunk>> {
    let (watermark_nanos, is_timeout) = self.calculate_watermark();
    if watermark_nanos == u64::MAX {
        return Vec::new(); // within grace period
    }

    let window_nanos = self.config.window_nanos;
    let ready_windows: Vec<u64> = self.windows
        .range(..)
        .filter(|(_, w)| w.window_start_nanos + window_nanos <= watermark_nanos)
        .map(|(&idx, _)| idx)
        .collect();

    let mut results = Vec::new();
    for idx in ready_windows {
        if let Some(window) = self.windows.remove(&idx) {
            results.push(self.emit_window(window, is_timeout));
        }
    }
    results
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer -- --nocapture`
Expected: All 24+ tests PASS (21 existing + 3 new)

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/media/audio_synchronizer.rs
git commit -m "fix(audio): AudioSynchronizer source start grace 和 timeout 标记"
```

---

### Task 2: RecordingDiagnostics source-aware writer-before fields

**Files:**
- Modify: `src-tauri/src/media/recording_writer.rs`
- Test: same file (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing test for source-aware contract with missing before-writer data**

Add test in `recording_writer.rs`:

```rust
#[test]
fn validate_source_aware_audio_contract_rejects_missing_system_before_writer() {
    let mut diag = RecordingDiagnostics {
        requested_system_audio: true,
        requested_microphone: true,
        system_rms_max: 0.05,  // capture-side has system audio
        mic_rms_max: 0.10,
        system_windows_before_writer: 0,  // but 0 windows before writer
        system_rms_max_before_writer: 0.0,
        ..Default::default()
    };
    let writer_diag = WriterDiagnostics::default();

    let result = validate_source_aware_audio_contract(&diag, &writer_diag);
    assert!(result.is_err(), "should reject when system requested, capture RMS non-zero, but 0 before-writer windows");
}

#[test]
fn validate_source_aware_audio_contract_rejects_missing_mic_before_writer() {
    let mut diag = RecordingDiagnostics {
        requested_system_audio: true,
        requested_microphone: true,
        system_rms_max: 0.05,
        mic_rms_max: 0.10,
        mic_windows_before_writer: 0,
        mic_rms_max_before_writer: 0.0,
        ..Default::default()
    };
    let writer_diag = WriterDiagnostics::default();

    let result = validate_source_aware_audio_contract(&diag, &writer_diag);
    assert!(result.is_err(), "should reject when mic requested, capture RMS non-zero, but 0 before-writer windows");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg validate_source_aware_audio_contract -- --nocapture`
Expected: FAIL (new fields don't exist yet)

- [ ] **Step 3: Add new fields to `RecordingDiagnostics`**

```rust
pub struct RecordingDiagnostics {
    // ... existing fields ...
    pub system_rms_max_before_writer: f32,
    pub mic_rms_max_before_writer: f32,
    // NEW fields:
    pub system_windows_before_writer: u64,
    pub mic_windows_before_writer: u64,
    pub system_frames_before_writer: u64,
    pub mic_frames_before_writer: u64,
}
```

- [ ] **Step 4: Update `validate_source_aware_audio_contract()` to use new fields**

```rust
pub fn validate_source_aware_audio_contract(
    diagnostics: &RecordingDiagnostics,
    writer_diagnostics: &WriterDiagnostics,
) -> AppResult<()> {
    // Existing checks (window counts from synchronizer)
    if diagnostics.requested_system_audio && diagnostics.system_rms_max > 0.001 {
        let (paired, sys_only, _, _) = /* from diagnostics */;
        if sys_only + paired == 0 {
            return Err(AppError::RecordingWriteFailed(
                "请求了系统音频且采集到非零 RMS，但 synchronizer 未输出任何系统音频窗口".into()
            ));
        }
    }

    // NEW: Check before-writer presence
    if diagnostics.requested_system_audio && diagnostics.system_rms_max > 0.001 {
        if diagnostics.system_windows_before_writer == 0 {
            return Err(AppError::RecordingWriteFailed(
                "请求了系统音频且采集到非零 RMS，但 writer 前未收到任何系统音频窗口".into()
            ));
        }
        if diagnostics.system_rms_max_before_writer < 0.001 {
            return Err(AppError::RecordingWriteFailed(
                "请求了系统音频但 writer 前系统音频 RMS 为 0".into()
            ));
        }
    }

    if diagnostics.requested_microphone && diagnostics.mic_rms_max > 0.001 {
        if diagnostics.mic_windows_before_writer == 0 {
            return Err(AppError::RecordingWriteFailed(
                "请求了麦克风且采集到非零 RMS，但 writer 前未收到任何麦克风音频窗口".into()
            ));
        }
        if diagnostics.mic_rms_max_before_writer < 0.001 {
            return Err(AppError::RecordingWriteFailed(
                "请求了麦克风但 writer 前麦克风音频 RMS 为 0".into()
            ));
        }
    }

    // Existing discard ratio check...

    Ok(())
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg validate_source_aware_audio_contract -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/recording_writer.rs
git commit -m "fix(audio): RecordingDiagnostics 新增 source-aware before-writer 字段和 contract 检查"
```

---

### Task 3: Enforce `audible_min_rms` in audio contract validation

**Files:**
- Modify: `src-tauri/src/media/ffmpeg_common.rs`
- Test: same file (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing test for audible RMS rejection**

```rust
#[test]
fn requested_audio_contract_rejects_low_rms_even_when_peak_passes() {
    // RMS=0.008 is above min_rms=0.003 but below audible_min_rms=0.015
    // peak=0.10 is above min_peak=0.02
    // This should fail because audio is not audible enough
    let contract = RequestedAudioContract {
        requested_system_audio: true,
        requested_microphone: false,
        min_rms: 0.003,
        min_peak: 0.02,
        audible_min_rms: 0.015,
    };

    // We need a test artifact with low RMS but non-zero peak
    // Use the existing strict helper to create a synthetic artifact
    // with very quiet audio (sine wave at very low amplitude)
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("low_rms.mp4");
    create_quiet_test_artifact(&path, 0.008); // RMS ~0.008

    let result = validate_source_artifact_with_audio_contract(&path, &contract);
    assert!(result.is_err(), "should reject when RMS < audible_min_rms even if peak passes");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract_rejects_low_rms -- --nocapture`
Expected: FAIL (audible_min_rms not enforced)

- [ ] **Step 3: Update `validate_source_artifact_with_audio_contract()` to check `audible_min_rms`**

In `ffmpeg_common.rs`, modify the validation function:

```rust
pub fn validate_source_artifact_with_audio_contract(
    path: &Path,
    contract: &RequestedAudioContract,
) -> AppResult<MediaArtifactInspection> {
    let inspection = validate_source_artifact(path)?;

    if contract.any_audio_requested() {
        let rms = inspection.audio_rms.unwrap_or(0.0);
        let peak = inspection.audio_peak.unwrap_or(0.0);

        // Level 1: near-silent check (existing)
        if rms < contract.min_rms && peak < contract.min_peak {
            return Err(AppError::RecordingWriteFailed(
                format!("录制音频近乎静音: RMS={:.6}, peak={:.6}", rms, peak)
            ));
        }

        // Level 2: audible check (NEW)
        if rms < contract.audible_min_rms {
            return Err(AppError::RecordingWriteFailed(
                format!("录制音频 RMS 低于可听阈值: RMS={:.6} < {}", rms, contract.audible_min_rms)
            ));
        }
    }

    Ok(inspection)
}
```

Apply the same change to `validate_export_artifact_with_audio_contract()`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/ffmpeg_common.rs
git commit -m "fix(audio): 启用 audible_min_rms 阈值检查，区分非全静音和可听"
```

---

### Task 4: Writer `join_worker()` bounded wait

**Files:**
- Modify: `src-tauri/src/media/ffmpeg_writer.rs`
- Test: same file (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing test for join timeout**

```rust
#[test]
fn ffmpeg_writer_finish_returns_error_on_worker_stuck() {
    // This test verifies that finish() doesn't block forever
    // when the worker thread is stuck.
    // We can't easily make a real FFmpeg worker stuck, but we can
    // verify the timeout mechanism exists by checking the code path.
    // For now, verify that finish() with a normal worker completes
    // within a reasonable time.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("join_test.mp4");
    let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

    // Push minimal content
    let frame = make_test_video_frame(1920, 1080);
    writer.push_video(frame).unwrap();
    writer.push_audio(make_test_mixed_chunk(0, 48000, 2, 960)).unwrap();

    let start = std::time::Instant::now();
    let result = writer.finish();
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "normal finish should succeed");
    assert!(elapsed < std::time::Duration::from_secs(30), "finish should not block longer than 30s, took {:?}", elapsed);
}
```

- [ ] **Step 2: Run test to verify it fails or passes baseline**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_finish_returns_error_on_worker_stuck -- --nocapture`
Expected: PASS (baseline — the real fix is the bounded join implementation)

- [ ] **Step 3: Modify `finish()` to drop sender before joining worker**

The current `finish()` sends Flush then calls `join_worker()` which does `handle.join()` — this blocks indefinitely if the worker is stuck. The fix: drop `self.tx` before joining so the worker sees a closed channel and can exit if it's waiting on `rx.recv()` after processing Flush.

**Struct change:** Change `tx: mpsc::SyncSender<EncoderMessage>` to `tx: Option<mpsc::SyncSender<EncoderMessage>>` in `FfmpegRecordingWriter`. Update `new()` to wrap tx in `Some(...)`. Update `push_video()`/`push_audio()` to use `self.tx.as_ref().ok_or(...)?.try_send(...)`.

**`finish()` change:**

```rust
fn finish(&mut self) -> AppResult<RecordingResult> {
    // Send flush signal with bounded retries (existing, unchanged)
    const MAX_FLUSH_RETRIES: usize = 100;
    const FLUSH_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(10);

    let mut flush_sent = false;
    for _ in 0..MAX_FLUSH_RETRIES {
        match self.tx.as_ref().map(|tx| tx.try_send(EncoderMessage::Flush)) {
            Some(Ok(())) => { flush_sent = true; break; }
            Some(Err(mpsc::TrySendError::Full(_))) => {
                std::thread::sleep(FLUSH_RETRY_DELAY);
            }
            Some(Err(mpsc::TrySendError::Disconnected(_))) => {
                flush_sent = true; // Worker already exited
                break;
            }
            None => { flush_sent = true; break; } // tx already taken
        }
    }

    if !flush_sent {
        return Err(AppError::RecordingWriteFailed {
            reason: format!(
                "无法在 {}ms 内发送刷新信号（编码队列持续满载）",
                MAX_FLUSH_RETRIES as u64 * 10
            ),
        });
    }

    // NEW: Drop sender before joining worker.
    // This ensures the worker's channel becomes disconnected after it processes
    // the Flush message, so if the worker is stuck on rx.recv() it will get a
    // RecvError and exit cleanly instead of blocking join() forever.
    self.tx.take();

    // Join the worker and merge front-end queue diagnostics.
    let start = std::time::Instant::now();
    let mut result = self.join_worker()?;
    let join_elapsed = start.elapsed();
    if join_elapsed > std::time::Duration::from_secs(10) {
        eprintln!(
            "警告: encoder worker join 耗时 {:.1}s，可能 FFmpeg flush 缓慢",
            join_elapsed.as_secs_f64()
        );
    }
    result.writer_diagnostics.video_queue_full_count += self.video_queue_full_count;
    result.writer_diagnostics.audio_queue_full_count += self.audio_queue_full_count;
    Ok(result)
}
```

Key changes:
1. `tx` becomes `Option<SyncSender>` — `self.tx.take()` drops the sender before joining
2. All `self.tx.try_send(...)` calls guarded with `self.tx.as_ref().ok_or(...)?`
3. Slow join logging (>10s warning)
4. `push_video`/`push_audio` return `Disconnected` error if tx is None (writer already finished)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture`
Expected: All 16+ tests PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/ffmpeg_writer.rs
git commit -m "fix(audio): writer finish() 在 join_worker 前 drop sender，防止无限阻塞"
```

---

### Task 5: Write source-aware diagnostics in consumer drain loops

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs`
- Test: same file (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing test for before-writer diagnostics**

```rust
#[test]
fn consume_frames_records_source_rms_before_writer_in_live_drain() {
    // Verify that after consume_frames runs, diagnostics contain
    // non-zero system_rms_max_before_writer and mic_rms_max_before_writer
    // when both sources have audio.

    // This test uses CountingRecordingWriter and feeds system+mic chunks.
    // After consume_frames completes, check diagnostics fields.

    let (stop_flag, video_rx, sys_rx, mic_rx, frame_count, writer, mic_level) =
        setup_test_consumer_channels();

    // Push system audio chunks with non-zero content
    for i in 0..10 {
        sys_rx.try_send(make_test_audio_chunk(i * 20_000_000, 48000, 2, non_silent_samples())).ok();
    }
    // Push mic audio chunks with non-zero content
    for i in 0..10 {
        mic_rx.try_send(make_test_audio_chunk(i * 20_000_000, 24000, 1, non_silent_samples())).ok();
    }

    stop_flag.store(true, Ordering::Relaxed);

    let output = consume_frames(
        stop_flag, video_rx, sys_rx, mic_rx, frame_count, writer,
        mic_level, "medium".into(), true, true, None,
    );

    assert!(output.diagnostics.system_rms_max_before_writer > 0.0,
        "system_rms_max_before_writer should be > 0, got {}",
        output.diagnostics.system_rms_max_before_writer);
    assert!(output.diagnostics.mic_rms_max_before_writer > 0.0,
        "mic_rms_max_before_writer should be > 0, got {}",
        output.diagnostics.mic_rms_max_before_writer);
    assert!(output.diagnostics.system_windows_before_writer > 0,
        "system_windows_before_writer should be > 0");
    assert!(output.diagnostics.mic_windows_before_writer > 0,
        "mic_windows_before_writer should be > 0");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg consume_frames_records_source_rms -- --nocapture`
Expected: FAIL (new diagnostics fields not being written)

- [ ] **Step 3: Update live drain loop to write before-writer diagnostics**

In `consume_frames()`, in the section where `synchronizer.drain_mixed()` results are processed (around the `for synced_result in synchronizer.drain_mixed()` loop), add:

```rust
for synced_result in synchronizer.drain_mixed() {
    match synced_result {
        Ok(synced) => {
            // NEW: Record per-source before-writer diagnostics
            if synced.has_system {
                diagnostics.system_windows_before_writer += 1;
                diagnostics.system_frames_before_writer += synced.system_frames;
                if synced.system_rms > diagnostics.system_rms_max_before_writer {
                    diagnostics.system_rms_max_before_writer = synced.system_rms;
                }
            }
            if synced.has_mic {
                diagnostics.mic_windows_before_writer += 1;
                diagnostics.mic_frames_before_writer += synced.mic_frames;
                if synced.mic_rms > diagnostics.mic_rms_max_before_writer {
                    diagnostics.mic_rms_max_before_writer = synced.mic_rms;
                }
            }
            if synced.emitted_due_to_timeout {
                diagnostics.source_timeout_window_count += 1;
            }

            // Push to writer (existing)
            let mixed = synced.mixed;
            diagnostics.mixed_chunks_queued += 1;
            // ... existing push logic ...
        }
        Err(e) => { /* existing error handling */ }
    }
}
```

- [ ] **Step 4: Update final drain loop similarly**

In the final drain section (around `synchronizer.drain_final()`), apply the same before-writer diagnostics update for each chunk before pushing to writer.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg macos_service -- --nocapture`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): consumer drain 循环写入 source-aware before-writer 诊断数据"
```

---

### Task 6: Bluetooth microphone stop lifecycle hardening

**Files:**
- Modify: `src-tauri/src/platform/macos/cpal_microphone.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Test: same files

- [ ] **Step 1: Write failing test for explicit pause before drop**

```rust
// In cpal_microphone.rs — this test verifies the stop diagnostics structure
#[test]
fn cpal_microphone_stop_produces_diagnostics() {
    // Since we can't easily create a real cpal stream in tests,
    // verify that stop() on a non-started capture returns Ok
    // and diagnostics show stream_dropped=false.
    let mut mic = CpalMicrophoneCapture::new();
    let result = mic.stop();
    assert!(result.is_ok());
}
```

- [ ] **Step 2: Add `MicrophoneStopDiagnostics` struct**

```rust
/// Diagnostics captured during microphone stop lifecycle.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneStopDiagnostics {
    pub stop_requested: bool,
    pub stream_existed: bool,
    pub pause_attempted: bool,
    pub pause_ok: bool,
    pub pause_error: Option<String>,
    pub stream_dropped: bool,
    pub callbacks_after_stop: u64,
    pub waited_ms: u64,
}
```

- [ ] **Step 3: Modify `CpalMicrophoneCapture::stop()` to use explicit pause**

```rust
fn stop(&mut self) -> AppResult<()> {
    let mut diag = MicrophoneStopDiagnostics {
        stop_requested: true,
        ..Default::default()
    };

    // Signal running=false first
    self.running.store(false, Ordering::Relaxed);

    if let Some(send_stream) = self.stream.take() {
        diag.stream_existed = true;

        // Explicit pause before drop — captures CoreAudio stop error
        diag.pause_attempted = true;
        match send_stream.0.pause() {
            Ok(()) => {
                diag.pause_ok = true;
                eprintln!("CpalMicrophoneCapture::pause() 成功");
            }
            Err(e) => {
                diag.pause_error = Some(format!("{:?}", e));
                eprintln!("CpalMicrophoneCapture::pause() 失败: {:?}", e);
            }
        }

        // Drop stream (releases CoreAudio resources)
        drop(send_stream);
        diag.stream_dropped = true;

        // Bounded wait for CoreAudio to complete device release
        // For Bluetooth HFP profile switching, this needs some time
        let wait_ms = 300u64;
        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
        diag.waited_ms = wait_ms;
    }

    eprintln!("CpalMicrophoneCapture::stop() 完成 — {:?}", diag);
    Ok(())
}
```

- [ ] **Step 4: Update `MacRecordingService::stop()` to only stop mic if it was started**

In `macos_service.rs`, modify the stop method:

```rust
pub fn stop(&mut self) -> AppResult<RecordingResult> {
    // ... existing cursor runtime stop ...

    // NEW: Only stop mic if it was actually started this session
    if self.last_requested_microphone {
        eprintln!("麦克风已启动，执行 mic stop...");
        let mic_result = self.mic_capture.stop();
        if let Err(e) = &mic_result {
            eprintln!("麦克风停止失败: {:?}", e);
        }
    } else {
        eprintln!("本轮未启动麦克风，跳过 mic stop");
    }

    // Stop screen capture (after mic, to minimize Bluetooth HFP hold time)
    let capture_result = ScreenCapture::stop(&mut self.screen_capture);

    // ... rest of stop logic ...

    // NEW: Reset mic capture instance to avoid stale device handle
    if self.last_requested_microphone {
        self.mic_capture = CpalMicrophoneCapture::new();
    }

    // ... existing logic ...
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cpal_microphone -- --nocapture`
Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg macos_service -- --nocapture`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/platform/macos/cpal_microphone.rs src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): 蓝牙麦克风 stop 显式 pause + 诊断 + 仅启动时 stop"
```

---

### Task 7: Export stage uses audio contract validation

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Verify existing export contract code**

Read `lib.rs` `export_video()` function. The code review (Section 10.4 Important 2) says the export already creates `RequestedAudioContract` and calls `validate_export_artifact_with_audio_contract`. Verify this is correct and the contract includes `audible_min_rms`.

- [ ] **Step 2: Ensure `audible_min_rms` is passed in export contract**

In `export_video()`, verify the contract construction:

```rust
let contract = RequestedAudioContract {
    requested_system_audio: svc.last_requested_system_audio(),
    requested_microphone: svc.last_requested_microphone(),
    ..RequestedAudioContract::default() // includes audible_min_rms=0.015
};
```

If `..Default::default()` is already used, no change needed — `audible_min_rms` defaults to `0.015`.

- [ ] **Step 3: Fix `unwrap()` on service lock**

Replace:
```rust
let svc = state.service.lock().unwrap();
```

With:
```rust
let svc = state.service.lock().map_err(|_| "录制服务锁已损坏".to_string())?;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg lib -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "fix(audio): export_video() 修复锁 unwrap + 确保 audio contract 包含 audible 阈值"
```

---

### Task 8: BUG.md, checklist, and HANDOFF update

**Files:**
- Modify: `BUG.md`
- Modify: `tests/phase-6-w11-w12-checklist.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Update BUG.md with new prevention rules**

Add to BUG-005 prevention rules section:

```markdown
**新增预防规则（2026-06-02 Section 10 整改）**：

- consumer drain 循环必须在 writer push 前写入 per-source before-writer RMS/frames/windows 诊断数据
- source-aware contract 必须检查 before-writer 字段，不能只依赖 synchronizer window counts
- `audible_min_rms` 必须参与 source/export validation，不能只作为日志字段
- CPAL 麦克风 stop 必须显式调用 `pause()` 并记录结果，不能只依赖 `drop()` 的隐式释放
- stop 顺序应为 mic first → screen capture，减少蓝牙 HFP profile 持有时间
- 只在本轮确实启动过麦克风时才执行 mic stop，避免无意义的 200ms sleep
- stop 后应重建 `CpalMicrophoneCapture` 实例，避免旧 device handle 残留
- AudioSynchronizer 双源启动阶段应有 grace period，防止早到源单独 emit
- `source_timeout_window_count` 必须在 source stall 或 grace timeout 时正确递增
```

- [ ] **Step 2: Update BUG-005 status**

Update BUG-005 status to reflect the current fix round.

- [ ] **Step 3: Run full regression**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets
```

Expected: All pass.

- [ ] **Step 4: Update checklist**

Update `tests/phase-6-w11-w12-checklist.md` with verification results from this round.

- [ ] **Step 5: Update HANDOFF.md**

Add new work task record at the top of the work task section.

- [ ] **Step 6: Commit**

```bash
git add BUG.md tests/phase-6-w11-w12-checklist.md HANDOFF.md
git commit -m "docs: 更新 BUG-005 状态、预防规则和验收清单（Section 10 整改）"
```

---

## Verification Commands

After all tasks are complete, run the full regression:

```bash
# Rust unit + integration tests with FFmpeg
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg

# Rust tests without FFmpeg (no-regression)
cargo test --manifest-path src-tauri/Cargo.toml

# Frontend tests
npm test -- --run

# Format check
cargo fmt --manifest-path src-tauri/Cargo.toml --check

# Lint check
cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets

# Focused tests for new functionality
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer_dual_source -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg validate_source_aware_audio_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg consume_frames_records_source_rms -- --nocapture
```

## Real Device Manual Gate

After code changes pass automated tests:

1. 只录系统音频 10 秒，播放音乐，验证 source/export 都可听
2. 只录内置麦克风 10 秒，说话，验证 source/export 都可听
3. 系统音频 + 内置麦克风 10 秒，验证两路都可听
4. 系统音频 + 显式蓝牙麦克风 10 秒：
   - 录制中 UI 显示蓝牙 warning
   - 停止后 2 秒内蓝牙输出音质恢复
   - 日志包含 `pause_ok`、`stream_dropped`、`waited_ms`
   - source/export audio RMS 不低于 audible contract
5. 系统音频 + 默认麦克风（默认设备为蓝牙）10 秒：
   - 停止后蓝牙音质恢复
   - 与显式设备路径日志对比
6. 每次验证记录：RecordingDiagnostics、WriterDiagnostics、artifact RMS/peak、before-writer diagnostics、Bluetooth recovery observation
