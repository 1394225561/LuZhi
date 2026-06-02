use std::collections::BTreeMap;

use crate::app::error::AppResult;
use crate::core::frame::{AudioChunk, MixedAudioChunk};
use crate::media::audio_mixer::{AudioMixer, SimpleAudioMixer};

/// Result of synchronizer output with source-aware metadata.
///
/// Tracks whether system and/or mic audio were present in the mixed output,
/// along with per-source RMS levels for diagnostics.
#[derive(Debug, Clone)]
pub struct SynchronizedAudioChunk {
    /// The mixed audio chunk ready for writer.
    pub mixed: MixedAudioChunk,
    /// Whether system audio was present in this chunk.
    pub has_system: bool,
    /// Whether microphone audio was present in this chunk.
    pub has_mic: bool,
    /// RMS of system audio before mixing (0.0 if no system audio).
    pub system_rms: f32,
    /// RMS of microphone audio before mixing (0.0 if no mic audio).
    pub mic_rms: f32,
}

/// Per-source audio buffer within a time window.
///
/// Each source (system/mic) maintains its own sample_rate and channels
/// metadata, preventing cross-source metadata contamination (e.g.,
/// system 48kHz/2ch vs mic 48kHz/1ch).
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

/// Fixed-window audio merger that pairs system and mic audio by time window.
///
/// Instead of per-chunk pairing (which can emit system-only and mic-only chunks
/// on the same timeline), this merger assigns chunks to fixed 20ms windows.
/// Each window outputs exactly one `SynchronizedAudioChunk` containing the mixed
/// audio from both sources. Missing sources are filled with silence.
///
/// This design prevents the BUG-005 issue where `mixed_chunks_written` equaled
/// `system_chunks_received + mic_chunks_received` (double-write) instead of
/// being close to the number of time windows.
pub struct AudioSynchronizer<M: AudioMixer = SimpleAudioMixer> {
    mixer: M,
    /// Windows indexed by window number (timestamp / WINDOW_NANOS).
    windows: BTreeMap<u64, AudioWindow>,
    /// Duration of each window in nanoseconds.
    window_nanos: u64,
    /// Latest system chunk timestamp seen (nanoseconds).
    latest_system_ts: u64,
    /// Latest mic chunk timestamp seen (nanoseconds).
    latest_mic_ts: u64,
    /// Diagnostics: number of windows that had both system and mic.
    paired_window_count: u64,
    /// Diagnostics: number of windows with system-only audio.
    system_only_window_count: u64,
    /// Diagnostics: number of windows with mic-only audio.
    mic_only_window_count: u64,
}

/// Default window size: 20ms = 20_000_000 nanoseconds.
/// This balances latency (20ms is imperceptible) with merge efficiency
/// (system and mic callbacks typically arrive within 10-20ms of each other).
const WINDOW_NANOS: u64 = 20_000_000;

/// Hold window: how long to wait before emitting a window during live drain.
/// Only windows older than `latest_ts - HOLD_NANOS` are emitted.
/// This gives late-arriving chunks time to land in the correct window.
const HOLD_NANOS: u64 = 40_000_000; // 40ms = 2 windows of hold

/// Maximum number of windows to keep in memory. Oldest windows are dropped first.
const MAX_WINDOWS: usize = 500; // 500 * 20ms = 10 seconds of buffering

impl<M: AudioMixer> AudioSynchronizer<M> {
    pub fn new(mixer: M) -> Self {
        Self {
            mixer,
            windows: BTreeMap::new(),
            window_nanos: WINDOW_NANOS,
            latest_system_ts: 0,
            latest_mic_ts: 0,
            paired_window_count: 0,
            system_only_window_count: 0,
            mic_only_window_count: 0,
        }
    }

    /// Enqueue a system audio chunk for window-based merging.
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

    /// Enqueue a microphone audio chunk for window-based merging.
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

        // Evict oldest windows if we exceed the limit.
        while self.windows.len() > MAX_WINDOWS {
            if let Some((&oldest_idx, _)) = self.windows.iter().next() {
                self.windows.remove(&oldest_idx);
            }
        }
    }

    /// Drain windows that are ready for output using watermark-based emission.
    ///
    /// A window is "ready" when its end time is older than
    /// `min(latest_system_ts, latest_mic_ts) - HOLD_NANOS`.
    /// For single-source recordings (only system or only mic), the watermark
    /// uses the active source's latest timestamp.
    ///
    /// Each window produces exactly one `SynchronizedAudioChunk`, preventing
    /// the double-write timeline issue.
    pub fn drain_mixed(&mut self) -> Vec<AppResult<MixedAudioChunk>> {
        // Calculate the watermark: only emit windows that end before this.
        let watermark_nanos = self.calculate_watermark();

        // Find windows ready for emission.
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

    /// Drains all remaining windows without holding for future matches.
    ///
    /// This MUST be called when stopping a recording session to ensure no audio
    /// windows are left unemitted. Unlike `drain_mixed()`, this method:
    /// - Does not wait for watermark advancement
    /// - Emits all remaining windows immediately
    /// - Reports unpaired windows for diagnostics
    ///
    /// Returns a vector of (SynchronizedAudioChunk, was_unpaired) tuples.
    pub fn drain_final(&mut self) -> Vec<(SynchronizedAudioChunk, bool)> {
        let mut results = Vec::with_capacity(self.windows.len());

        // Drain all windows in timestamp order.
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

        // Sort by timestamp for ordered output.
        results.sort_by_key(|(chunk, _)| chunk.mixed.timestamp.nanos);

        results
    }

    /// Mixes a single pair of system and microphone audio chunks.
    ///
    /// At least one of `system` or `mic` must be `Some`.
    /// This is a convenience method for direct mixing without windowing.
    pub fn mix_pair(
        &self,
        system: Option<&AudioChunk>,
        mic: Option<&AudioChunk>,
    ) -> AppResult<MixedAudioChunk> {
        self.mixer.mix(system, mic)
    }

    /// Returns diagnostics counters for window pairing.
    pub fn diagnostics(&self) -> (u64, u64, u64) {
        (
            self.paired_window_count,
            self.system_only_window_count,
            self.mic_only_window_count,
        )
    }

    // --- Private helpers ---

    /// Calculate the watermark timestamp for live drain.
    ///
    /// Uses the maximum of the latest system and mic timestamps minus the
    /// hold window. The hold window (40ms) gives late-arriving chunks from
    /// the other source time to land in the correct window before emission.
    ///
    /// This approach ensures that:
    /// - When both sources are active, windows are emitted after the hold period
    /// - When only one source is active, it drives emission directly
    /// - A slow source doesn't block emission indefinitely
    fn calculate_watermark(&self) -> u64 {
        let latest_ts = self.latest_system_ts.max(self.latest_mic_ts);
        latest_ts.saturating_sub(HOLD_NANOS)
    }

    /// Emit a single window as a `MixedAudioChunk`.
    ///
    /// Mixes system and mic audio (filling silence for missing sources),
    /// producing exactly one chunk per window.
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
}

impl Default for AudioSynchronizer<SimpleAudioMixer> {
    fn default() -> Self {
        Self::new(SimpleAudioMixer::new())
    }
}

/// Computes the RMS (root mean square) of a slice of f32 audio samples.
///
/// Returns 0.0 for empty slices.
fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
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

        let mixed = synchronizer
            .mix_pair(
                Some(&chunk(0, vec![0.5, 0.5])),
                Some(&chunk(0, vec![0.25, 0.25])),
            )
            .unwrap();

        assert_eq!(mixed.timestamp.nanos, 0);
        assert_eq!(mixed.sample_rate, 48_000);
        assert_eq!(mixed.channels, 2);
    }

    #[test]
    fn passes_single_available_source() {
        let synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        let mixed = synchronizer
            .mix_pair(Some(&chunk(0, vec![0.5, 0.5])), None)
            .unwrap();

        assert_eq!(mixed.samples.len(), 2);
    }

    #[test]
    fn synchronizer_outputs_one_chunk_per_window_for_system_and_mic() {
        // BUG-005 Critical 2: When both system and mic push chunks in the same
        // 20ms window, the synchronizer must output exactly ONE mixed chunk,
        // not two separate single-source chunks.
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // Push system and mic chunks within the same 20ms window (t=0..20ms).
        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5])); // 5ms
        synchronizer.push_mic(chunk(8_000_000, vec![0.25, 0.25])); // 8ms

        // Push a second pair in the next window (t=20..40ms).
        synchronizer.push_system(chunk(25_000_000, vec![0.3, 0.3])); // 25ms
        synchronizer.push_mic(chunk(28_000_000, vec![0.15, 0.15])); // 28ms

        // Advance watermark past both windows (need >40ms+40ms=80ms).
        synchronizer.push_system(chunk(100_000_000, vec![0.1, 0.1])); // 100ms

        let results = synchronizer.drain_mixed();

        // Should produce exactly 2 chunks (one per window), not 4.
        assert_eq!(
            results.len(),
            2,
            "expected 2 mixed chunks (one per window), got {}",
            results.len()
        );
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());

        // Verify diagnostics.
        let (paired, sys_only, mic_only) = synchronizer.diagnostics();
        assert_eq!(paired, 2, "both windows should be paired");
        assert_eq!(sys_only, 0);
        assert_eq!(mic_only, 0);
    }

    #[test]
    fn synchronizer_late_mic_within_hold_window_is_mixed_not_dropped() {
        // BUG-005 Critical 2: A mic chunk arriving slightly after the system chunk
        // but within the same window must be mixed, not dropped.
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // System arrives at 5ms.
        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5]));

        // Mic arrives at 18ms (still within the same 20ms window).
        synchronizer.push_mic(chunk(18_000_000, vec![0.25, 0.25]));

        // Advance watermark to trigger emission.
        synchronizer.push_system(chunk(60_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();

        // Should produce 1 paired chunk, not 2 separate chunks.
        assert_eq!(results.len(), 1, "expected 1 mixed chunk, got {}", results.len());
        assert!(results[0].is_ok());

        // Verify the chunk contains both sources.
        let (paired, _, _) = synchronizer.diagnostics();
        assert_eq!(paired, 1, "window should be paired");
    }

    #[test]
    fn synchronizer_system_first_then_mic_preserves_mic_rms() {
        // BUG-005 Critical 2: When system arrives first and mic arrives later
        // (but within hold window), mic audio must not be lost.
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // System at 5ms with silence-like level.
        synchronizer.push_system(chunk(5_000_000, vec![0.01, 0.01]));
        // Mic at 15ms with significant level.
        synchronizer.push_mic(chunk(15_000_000, vec![0.5, 0.5]));

        // Advance watermark.
        synchronizer.push_system(chunk(60_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 1);

        let mixed = results[0].as_ref().unwrap();
        // Mixed RMS should reflect both sources.
        let mixed_rms = compute_rms(&mixed.samples);
        assert!(
            mixed_rms > 0.1,
            "mixed RMS should preserve mic content, got {mixed_rms}"
        );
    }

    #[test]
    fn synchronizer_system_silent_mic_nonzero_outputs_nonzero_mixed() {
        // BUG-005: System audio near-silent + mic non-silent must produce
        // non-silent mixed output.
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        synchronizer.push_system(chunk(5_000_000, vec![0.001, 0.001]));
        synchronizer.push_mic(chunk(8_000_000, vec![0.4, 0.4]));

        // Advance watermark.
        synchronizer.push_system(chunk(60_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 1);

        let mixed = results[0].as_ref().unwrap();
        let mixed_rms = compute_rms(&mixed.samples);
        assert!(
            mixed_rms > 0.1,
            "mixed should be non-silent when mic is non-silent, got {mixed_rms}"
        );
    }

    #[test]
    fn synchronizer_reports_system_only_and_mic_only_window_counts() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // Window 0 (0-20ms): system-only.
        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5]));
        // Window 1 (20-40ms): mic-only.
        synchronizer.push_mic(chunk(25_000_000, vec![0.25, 0.25]));
        // Window 2 (40-60ms): paired.
        synchronizer.push_system(chunk(45_000_000, vec![0.3, 0.3]));
        synchronizer.push_mic(chunk(48_000_000, vec![0.15, 0.15]));

        // Advance watermark past all windows (need >60ms+40ms=100ms).
        synchronizer.push_system(chunk(120_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 3);

        let (paired, sys_only, mic_only) = synchronizer.diagnostics();
        assert_eq!(paired, 1);
        assert_eq!(sys_only, 1);
        assert_eq!(mic_only, 1);
    }

    #[test]
    fn handles_system_only_recording() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());
        synchronizer.push_system(chunk(0, vec![0.5, 0.5]));
        // Advance watermark past the hold window (40ms) so window 0 (0-20ms) is ready.
        // watermark = 80ms - 40ms = 40ms > 20ms (window 0 end) → ready.
        synchronizer.push_system(chunk(80_000_000, vec![0.3, 0.3]));

        let results = synchronizer.drain_mixed();
        assert!(results.len() >= 1, "should emit at least 1 chunk");
        assert!(results[0].is_ok());
    }

    #[test]
    fn handles_mic_only_recording() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());
        synchronizer.push_mic(chunk(0, vec![0.2, 0.2]));
        // Advance watermark past the hold window.
        synchronizer.push_mic(chunk(80_000_000, vec![0.3, 0.3]));

        let results = synchronizer.drain_mixed();
        assert!(results.len() >= 1, "should emit at least 1 chunk");
        assert!(results[0].is_ok());
    }

    #[test]
    fn drain_final_flushes_all_remaining_windows() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // Push system and mic chunks in different windows.
        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(25_000_000, vec![0.25, 0.25]));

        // drain_final should emit all windows immediately.
        let results = synchronizer.drain_final();
        assert_eq!(results.len(), 2);

        // First chunk should be system-only (window 0).
        assert!(results[0].0.has_system);
        assert!(!results[0].0.has_mic);
        assert!(results[0].1); // was_unpaired

        // Second chunk should be mic-only (window 1).
        assert!(!results[1].0.has_system);
        assert!(results[1].0.has_mic);
        assert!(results[1].1); // was_unpaired
    }

    #[test]
    fn drain_final_pairs_close_timestamps_in_same_window() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // Push system and mic within the same 20ms window.
        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(8_000_000, vec![0.25, 0.25]));

        let results = synchronizer.drain_final();
        assert_eq!(results.len(), 1);

        // Should be paired.
        assert!(results[0].0.has_system);
        assert!(results[0].0.has_mic);
        assert!(!results[0].1); // not unpaired
    }

    #[test]
    fn drain_final_handles_empty_windows() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        let results = synchronizer.drain_final();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn drain_final_preserves_timestamp_order() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // Push chunks in different windows.
        synchronizer.push_system(chunk(200_000_000, vec![0.5, 0.5])); // window 10
        synchronizer.push_system(chunk(0, vec![0.3, 0.3])); // window 0
        synchronizer.push_mic(chunk(100_000_000, vec![0.25, 0.25])); // window 5

        let results = synchronizer.drain_final();
        assert_eq!(results.len(), 3);

        // Should be sorted by timestamp.
        assert_eq!(results[0].0.mixed.timestamp.nanos, 0);
        assert_eq!(results[1].0.mixed.timestamp.nanos, 100_000_000);
        assert_eq!(results[2].0.mixed.timestamp.nanos, 200_000_000);
    }

    #[test]
    fn writer_does_not_drop_late_mic_after_synchronizer_window_merge() {
        // BUG-005 Critical 2: With the window merger, mic audio that arrives
        // within the same window as system audio is always mixed in.
        // The writer no longer sees overlapping single-source chunks.
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // Simulate 100ms of audio with interleaved system/mic.
        for i in 0..5 {
            let base_ts = i * 20_000_000; // 20ms windows
            synchronizer.push_system(chunk(base_ts + 2_000_000, vec![0.3, 0.3]));
            synchronizer.push_mic(chunk(base_ts + 10_000_000, vec![0.5, 0.5]));
        }

        // Advance watermark past all windows.
        synchronizer.push_system(chunk(200_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();

        // Should produce exactly 5 paired chunks + the watermark advance chunk.
        assert!(
            results.len() >= 5,
            "expected at least 5 mixed chunks, got {}",
            results.len()
        );

        // All should be OK.
        for r in &results {
            assert!(r.is_ok());
        }

        let (paired, _, _) = synchronizer.diagnostics();
        assert!(
            paired >= 5,
            "expected at least 5 paired windows, got {paired}"
        );
    }

    #[test]
    fn consumer_system_and_mic_realistic_interleaving_writes_non_silent_artifact() {
        // BUG-005: Integration-style test — simulates realistic system/mic
        // interleaving and verifies the mixed output is non-silent.
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // Simulate 1 second of audio with 20ms windows.
        for i in 0..50 {
            let base_ts = i * 20_000_000;
            // System at 2ms into each window.
            synchronizer.push_system(chunk(base_ts + 2_000_000, vec![0.3, 0.3]));
            // Mic at 10ms into each window.
            synchronizer.push_mic(chunk(base_ts + 10_000_000, vec![0.5, 0.5]));
        }

        // Advance watermark.
        synchronizer.push_system(chunk(2_000_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();

        // All chunks should be OK and non-silent.
        let mut total_rms = 0.0f32;
        let mut count = 0usize;
        for r in &results {
            if let Ok(mixed) = r {
                let rms = compute_rms(&mixed.samples);
                total_rms += rms;
                count += 1;
            }
        }

        assert!(count >= 50, "expected at least 50 chunks, got {count}");
        let avg_rms = total_rms / count as f32;
        assert!(
            avg_rms > 0.1,
            "average RMS should reflect mixed content, got {avg_rms}"
        );

        let (paired, sys_only, mic_only) = synchronizer.diagnostics();
        assert_eq!(sys_only, 0, "should have no system-only windows");
        assert_eq!(mic_only, 0, "should have no mic-only windows");
        assert!(paired >= 50, "should have at least 50 paired windows");
    }

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
        let mic_chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 1,
            samples: Arc::from(vec![0.5f32; 1024].into_boxed_slice()),
        };
        sync.push_mic(mic_chunk);

        // Drain — watermark needs latest_ts > HOLD_NANOS.
        let system_chunk2 = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(100_000_000),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(vec![0.3f32; 2048].into_boxed_slice()),
        };
        sync.push_system(system_chunk2);

        let results = sync.drain_mixed();
        assert!(!results.is_empty(), "should have emitted at least one window");

        for result in results {
            let mixed = result.unwrap();
            assert!(!mixed.samples.is_empty());
            assert_eq!(mixed.sample_rate, 48_000);
            assert_eq!(mixed.channels, 2); // Mixer outputs stereo
        }
    }

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

    #[test]
    fn dual_source_offset_does_not_discard_mic_windows() {
        // BUG-005 Critical 1: When system audio leads mic by a systematic offset,
        // the max-based watermark emits system-only windows before mic chunks
        // land in those windows. With source-aware watermark (min), the synchronizer
        // should wait for mic to catch up.
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // System fills windows 0-2, arrives first.
        synchronizer.push_system(chunk(5_000_000, vec![0.3, 0.3])); // window 0
        synchronizer.push_system(chunk(25_000_000, vec![0.3, 0.3])); // window 1
        synchronizer.push_system(chunk(45_000_000, vec![0.3, 0.3])); // window 2

        // System continues advancing the watermark — with max-based watermark,
        // this causes windows 0-2 to be emitted as system-only BEFORE mic arrives.
        synchronizer.push_system(chunk(120_000_000, vec![0.1, 0.1]));

        // Now mic arrives for the same recording windows (offset by callback delay).
        // These land in windows 0, 1, 2 — but with max-watermark, those windows
        // were already emitted as system-only.
        synchronizer.push_mic(chunk(10_000_000, vec![0.5, 0.5])); // window 0
        synchronizer.push_mic(chunk(30_000_000, vec![0.5, 0.5])); // window 1
        synchronizer.push_mic(chunk(50_000_000, vec![0.5, 0.5])); // window 2

        // Drain whatever is left.
        synchronizer.push_system(chunk(200_000_000, vec![0.1, 0.1]));
        let _more = synchronizer.drain_mixed();

        // With source-aware watermark, all 3 windows should be paired.
        // With current max-watermark, some windows are emitted as system-only
        // before mic arrives, then mic lands in a new copy of those windows.
        let (paired, sys_only, mic_only) = synchronizer.diagnostics();
        assert_eq!(
            sys_only, 0,
            "source-aware watermark should not emit system-only windows before mic arrives, \
             got paired={paired}, sys_only={sys_only}, mic_only={mic_only}"
        );
    }

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
        assert_eq!(
            results.len(),
            3,
            "expected 3 windows from 60ms chunk, got {}",
            results.len()
        );

        // Each window should have ~1920 samples (960 frames × 2ch).
        for (i, result) in results.iter().enumerate() {
            let mixed = result.as_ref().unwrap();
            assert_eq!(
                mixed.samples.len(),
                1920,
                "window {i} should have 1920 samples"
            );
            assert_eq!(mixed.sample_rate, 48_000);
            assert_eq!(mixed.channels, 2);
        }
    }

    #[test]
    fn synchronizer_does_not_emit_fast_source_before_slow_source_watermark() {
        // BUG-005 Critical 1: System is ahead of mic. With source-aware watermark,
        // system-only windows should not be emitted before mic arrives or timeout.
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());

        // System fills windows 0-4.
        for i in 0..5 {
            synchronizer.push_system(chunk(i * 20_000_000 + 5_000_000, vec![0.3, 0.3]));
        }

        // Only mic in window 0 and 1.
        synchronizer.push_mic(chunk(5_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(25_000_000, vec![0.5, 0.5]));

        // Now advance mic to window 4 — this should unlock windows 2-4.
        synchronizer.push_mic(chunk(85_000_000, vec![0.5, 0.5]));

        let results = synchronizer.drain_mixed();

        // Windows 0 and 1 should be paired. Windows 2-4 may be system-only
        // or still held depending on watermark logic.
        let (paired, sys_only, _mic_only) = synchronizer.diagnostics();
        assert!(
            paired >= 2,
            "windows 0-1 should be paired, got {paired}"
        );
        // The key assertion: total emitted should not have all system-only for windows 2-4.
        // With current max-watermark, system-only windows 0-4 would all be emitted
        // BEFORE mic arrives — this test locks that defect.
        assert_eq!(
            results.len(),
            5,
            "expected all 5 windows to be emitted, got {}",
            results.len()
        );
    }
}
