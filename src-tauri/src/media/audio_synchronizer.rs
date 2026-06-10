use std::collections::BTreeMap;

use crate::app::error::AppResult;
use crate::core::capture::DenoiseMode;
use crate::core::frame::{AudioChunk, MixedAudioChunk};
use crate::media::audio_mixer::{AudioMixer, SimpleAudioMixer};

/// Result of synchronizer output with source-aware metadata.
///
/// Tracks whether system and/or mic audio were present in the mixed output,
/// along with per-source RMS levels and frame counts for diagnostics.
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
    /// Number of system audio frames (samples / channels) in this window.
    pub system_frames: u64,
    /// Number of mic audio frames (samples / channels) in this window.
    pub mic_frames: u64,
    /// Whether this window was emitted due to source stall timeout.
    pub emitted_due_to_timeout: bool,
}

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
    /// Grace period for the second source to appear (default 500ms).
    /// When both sources are requested but only one has been seen, the synchronizer
    /// will NOT emit single-source windows until this grace period expires.
    /// Prevents premature system-only emission when mic starts late.
    pub source_start_grace_nanos: u64,
}

impl Default for AudioSynchronizerConfig {
    fn default() -> Self {
        Self {
            requested_system_audio: false,
            requested_microphone: false,
            window_nanos: 20_000_000,                  // 20ms
            hold_nanos: 40_000_000,                    // 40ms
            source_stall_timeout_nanos: 2_000_000_000, // 2 seconds
            source_start_grace_nanos: 500_000_000,     // 500ms
        }
    }
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

/// Source identifier for the shared push helper.
enum Source {
    System,
    Mic,
}

/// Fixed-window audio merger that pairs system and mic audio by time window.
///
/// Instead of per-chunk pairing (which can emit system-only and mic-only chunks
/// on the same timeline), this merger assigns chunks to fixed 20ms windows.
/// Each window outputs exactly one `SynchronizedAudioChunk` containing the mixed
/// audio from both sources. Missing sources are filled with silence.
///
/// **Source-aware watermark**: When both system and mic are requested and both
/// have been seen, the watermark uses `min(latest_system, latest_mic)` to prevent
/// the fast source from driving emission before the slow source arrives. A source
/// stall timeout prevents indefinite blocking.
///
/// **Sample-frame splitting**: Long audio chunks (e.g., 60ms Bluetooth buffers)
/// are split by sample frame into multiple 20ms windows, preventing the writer
/// from seeing inflated chunk durations that overlap subsequent windows.
pub struct AudioSynchronizer<M: AudioMixer = SimpleAudioMixer> {
    mixer: M,
    /// Windows indexed by window number (timestamp / window_nanos).
    windows: BTreeMap<u64, AudioWindow>,
    /// Synchronizer configuration.
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
    /// Timestamp when system source was first seen (for grace period).
    first_system_seen_nanos: u64,
    /// Timestamp when mic source was first seen (for grace period).
    first_mic_seen_nanos: u64,
    /// Diagnostics: number of windows that had both system and mic.
    paired_window_count: u64,
    /// Diagnostics: number of windows with system-only audio.
    system_only_window_count: u64,
    /// Diagnostics: number of windows with mic-only audio.
    mic_only_window_count: u64,
    /// Diagnostics: number of windows emitted due to source stall timeout.
    source_timeout_window_count: u64,
}

/// Maximum number of windows to keep in memory. Oldest windows are dropped first.
const MAX_WINDOWS: usize = 500; // 500 * 20ms = 10 seconds of buffering

impl<M: AudioMixer> AudioSynchronizer<M> {
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
            first_system_seen_nanos: 0,
            first_mic_seen_nanos: 0,
            paired_window_count: 0,
            system_only_window_count: 0,
            mic_only_window_count: 0,
            source_timeout_window_count: 0,
        }
    }

    /// Enqueue a system audio chunk for window-based merging.
    ///
    /// Long chunks are automatically split into fixed 20ms windows by sample frame.
    pub fn push_system(&mut self, chunk: AudioChunk) {
        let ts = chunk.timestamp.nanos;
        if !self.seen_system {
            self.first_system_seen_nanos = ts;
        }
        self.seen_system = true;
        self.latest_system_ts = self.latest_system_ts.max(ts);
        self.last_system_active_nanos = ts;

        self.push_source_chunk(Source::System, &chunk);
    }

    /// Enqueue a microphone audio chunk for window-based merging.
    ///
    /// Long chunks are automatically split into fixed 20ms windows by sample frame.
    pub fn push_mic(&mut self, chunk: AudioChunk) {
        let ts = chunk.timestamp.nanos;
        if !self.seen_mic {
            self.first_mic_seen_nanos = ts;
        }
        self.seen_mic = true;
        self.latest_mic_ts = self.latest_mic_ts.max(ts);
        self.last_mic_active_nanos = ts;

        self.push_source_chunk(Source::Mic, &chunk);
    }

    /// Drain windows that are ready for output using watermark-based emission.
    ///
    /// When both sources are requested and active, uses `min(system_ts, mic_ts)`
    /// as the watermark basis to prevent the fast source from driving emission
    /// before the slow source arrives.
    ///
    /// Each window produces exactly one `SynchronizedAudioChunk`, preventing
    /// the double-write timeline issue.
    pub fn drain_mixed(&mut self) -> Vec<AppResult<SynchronizedAudioChunk>> {
        let (watermark_nanos, is_timeout) = self.calculate_watermark();

        // Within grace period — nothing to emit yet.
        if watermark_nanos == u64::MAX {
            return Vec::new();
        }

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
                match self.emit_window(window, is_timeout) {
                    Ok(synced) => results.push(Ok(synced)),
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
        let indices: Vec<u64> = self.windows.keys().copied().collect();
        for idx in indices {
            if let Some(window) = self.windows.remove(&idx) {
                let has_system = window.system.is_some();
                let has_mic = window.mic.is_some();
                let was_unpaired = has_system != has_mic;

                match self.emit_window(window, false) {
                    Ok(synced) => results.push((synced, was_unpaired)),
                    Err(_) => {
                        // Skip windows that fail to emit.
                    }
                }
            }
        }
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
    ///
    /// Returns `(paired_window_count, system_only_window_count, mic_only_window_count, source_timeout_window_count)`.
    pub fn diagnostics(&self) -> (u64, u64, u64, u64) {
        (
            self.paired_window_count,
            self.system_only_window_count,
            self.mic_only_window_count,
            self.source_timeout_window_count,
        )
    }

    // --- Private helpers ---

    /// Calculate the watermark timestamp for live drain.
    ///
    /// Returns `(watermark_nanos, is_timeout_mode)`.
    /// `is_timeout_mode=true` means the watermark was computed from a single source
    /// due to start grace expiry or stall timeout, so emitted windows should be
    /// marked with `emitted_due_to_timeout=true`.
    ///
    /// When both sources are requested and have been seen, uses `min(system_ts, mic_ts)`
    /// to prevent the fast source from driving emission. Falls back to `max` when:
    /// - Only one source is requested
    /// - One source hasn't been seen yet (after grace period)
    /// - One source has stalled (no chunks for `source_stall_timeout_nanos`)
    fn calculate_watermark(&self) -> (u64, bool) {
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
                // to avoid blocking indefinitely. Mark as timeout.
                let max_ts = self.latest_system_ts.max(self.latest_mic_ts);
                (max_ts.saturating_sub(self.config.hold_nanos), true)
            } else {
                (base_watermark, false)
            }
        } else if both_requested && (self.seen_system || self.seen_mic) {
            // Only one source seen — check start grace period.
            let grace_nanos = self.config.source_start_grace_nanos;
            let first_seen_nanos = if self.seen_system {
                self.first_system_seen_nanos
            } else {
                self.first_mic_seen_nanos
            };
            let latest_ts = self.latest_system_ts.max(self.latest_mic_ts);

            if grace_nanos > 0 && latest_ts.saturating_sub(first_seen_nanos) < grace_nanos {
                // Within grace period — don't emit yet (watermark at infinity).
                return (u64::MAX, false);
            }

            // Grace expired — emit with timeout mark.
            (latest_ts.saturating_sub(self.config.hold_nanos), true)
        } else {
            // Single source requested, or neither seen yet.
            let max_ts = self.latest_system_ts.max(self.latest_mic_ts);
            (max_ts.saturating_sub(self.config.hold_nanos), false)
        }
    }

    /// Push a source chunk, splitting by sample frame into fixed 20ms windows.
    ///
    /// Long chunks (e.g., 60ms Bluetooth buffers) are split so each window
    /// contains only the samples that fall within its 20ms time range.
    fn push_source_chunk(&mut self, source: Source, chunk: &AudioChunk) {
        let ts = chunk.timestamp.nanos;
        let sample_rate = chunk.sample_rate as u64;
        let channels = chunk.channels as u16;
        let total_frames = chunk.samples.len() as u64 / channels.max(1) as u64;

        if total_frames == 0 || sample_rate == 0 {
            return;
        }

        // Calculate the time range this chunk covers.
        let frame_duration_nanos = 1_000_000_000 / sample_rate;
        let chunk_start_nanos = ts;
        let chunk_end_nanos = ts + total_frames * frame_duration_nanos;

        // Determine which windows this chunk overlaps.
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

            let window = self
                .windows
                .entry(window_idx)
                .or_insert_with(|| AudioWindow {
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

    /// Emit a single window as a `SynchronizedAudioChunk`.
    ///
    /// Mixes system and mic audio (filling silence for missing sources),
    /// producing exactly one chunk per window with source-aware metadata.
    fn emit_window(
        &mut self,
        window: AudioWindow,
        emitted_due_to_timeout: bool,
    ) -> AppResult<SynchronizedAudioChunk> {
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

        let system_frames = window
            .system
            .as_ref()
            .map_or(0, |b| b.samples.len() as u64 / b.channels.max(1) as u64);
        let mic_frames = window
            .mic
            .as_ref()
            .map_or(0, |b| b.samples.len() as u64 / b.channels.max(1) as u64);
        let system_rms = window
            .system
            .as_ref()
            .map_or(0.0, |b| compute_rms(&b.samples));
        let mic_rms = window.mic.as_ref().map_or(0.0, |b| compute_rms(&b.samples));

        // Create AudioChunks using per-source metadata.
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
}

impl Default for AudioSynchronizer<SimpleAudioMixer> {
    fn default() -> Self {
        Self::new(
            SimpleAudioMixer::new(DenoiseMode::default()),
            AudioSynchronizerConfig::default(),
        )
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

    fn mic_mono_chunk(ts: u64, sample_rate: u32, samples: Vec<f32>) -> AudioChunk {
        AudioChunk {
            timestamp: MediaTimestamp::from_nanos(ts),
            sample_rate,
            channels: 1,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    fn dual_source_synchronizer() -> AudioSynchronizer<SimpleAudioMixer> {
        AudioSynchronizer::new(
            SimpleAudioMixer::new(DenoiseMode::default()),
            AudioSynchronizerConfig {
                requested_system_audio: true,
                requested_microphone: true,
                ..Default::default()
            },
        )
    }

    #[test]
    fn synchronizer_outputs_full_windows_for_44100hz_mic_callbacks() {
        let sample_rate = 44_100u32;
        let total_frames = sample_rate as usize;
        let callback_frames = 512usize;
        let mut synchronizer = AudioSynchronizer::new(
            SimpleAudioMixer::new(DenoiseMode::Highpass),
            AudioSynchronizerConfig {
                requested_system_audio: false,
                requested_microphone: true,
                ..Default::default()
            },
        );

        let mut start_frame = 0usize;
        while start_frame < total_frames {
            let frame_count = callback_frames.min(total_frames - start_frame);
            let samples: Vec<f32> = (start_frame..start_frame + frame_count)
                .map(|frame| {
                    let t = frame as f64 / sample_rate as f64;
                    (2.0 * std::f64::consts::PI * 777.0 * t).sin() as f32 * 0.2
                })
                .collect();
            let ts = start_frame as u64 * 1_000_000_000 / sample_rate as u64;

            synchronizer.push_mic(mic_mono_chunk(ts, sample_rate, samples));
            start_frame += frame_count;
        }

        let results = synchronizer.drain_final();

        assert_eq!(results.len(), 50, "1s input should produce 50 windows");
        for (idx, (synced, _)) in results.iter().enumerate() {
            assert_eq!(
                synced.mixed.samples.len(),
                1_920,
                "20ms 44.1kHz mic window {idx} should resample to exactly 48kHz stereo"
            );
        }
    }

    #[test]
    fn mixes_pair_with_matching_timestamps() {
        let synchronizer = AudioSynchronizer::default();

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
        let synchronizer = AudioSynchronizer::default();

        let mixed = synchronizer
            .mix_pair(Some(&chunk(0, vec![0.5, 0.5])), None)
            .unwrap();

        assert_eq!(mixed.samples.len(), 2);
    }

    #[test]
    fn synchronizer_outputs_one_chunk_per_window_for_system_and_mic() {
        let mut synchronizer = AudioSynchronizer::default();

        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(8_000_000, vec![0.25, 0.25]));

        synchronizer.push_system(chunk(25_000_000, vec![0.3, 0.3]));
        synchronizer.push_mic(chunk(28_000_000, vec![0.15, 0.15]));

        synchronizer.push_system(chunk(100_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();

        assert_eq!(
            results.len(),
            2,
            "expected 2 mixed chunks, got {}",
            results.len()
        );
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());

        let (paired, sys_only, mic_only, _) = synchronizer.diagnostics();
        assert_eq!(paired, 2, "both windows should be paired");
        assert_eq!(sys_only, 0);
        assert_eq!(mic_only, 0);
    }

    #[test]
    fn synchronizer_late_mic_within_hold_window_is_mixed_not_dropped() {
        let mut synchronizer = AudioSynchronizer::default();

        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(18_000_000, vec![0.25, 0.25]));

        synchronizer.push_system(chunk(60_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();

        assert_eq!(
            results.len(),
            1,
            "expected 1 mixed chunk, got {}",
            results.len()
        );
        assert!(results[0].is_ok());

        let (paired, _, _, _) = synchronizer.diagnostics();
        assert_eq!(paired, 1, "window should be paired");
    }

    #[test]
    fn synchronizer_system_first_then_mic_preserves_mic_rms() {
        let mut synchronizer = AudioSynchronizer::default();

        synchronizer.push_system(chunk(5_000_000, vec![0.01, 0.01]));
        synchronizer.push_mic(chunk(15_000_000, vec![0.5, 0.5]));

        synchronizer.push_system(chunk(60_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 1);

        let synced = results[0].as_ref().unwrap();
        let mixed_rms = compute_rms(&synced.mixed.samples);
        assert!(
            mixed_rms > 0.1,
            "mixed RMS should preserve mic content, got {mixed_rms}"
        );
    }

    #[test]
    fn synchronizer_system_silent_mic_nonzero_outputs_nonzero_mixed() {
        let mut synchronizer = AudioSynchronizer::default();

        synchronizer.push_system(chunk(5_000_000, vec![0.001, 0.001]));
        synchronizer.push_mic(chunk(8_000_000, vec![0.4, 0.4]));

        synchronizer.push_system(chunk(60_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 1);

        let synced = results[0].as_ref().unwrap();
        let mixed_rms = compute_rms(&synced.mixed.samples);
        assert!(
            mixed_rms > 0.1,
            "mixed should be non-silent when mic is non-silent, got {mixed_rms}"
        );
    }

    #[test]
    fn synchronizer_reports_system_only_and_mic_only_window_counts() {
        let mut synchronizer = AudioSynchronizer::default();

        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(25_000_000, vec![0.25, 0.25]));
        synchronizer.push_system(chunk(45_000_000, vec![0.3, 0.3]));
        synchronizer.push_mic(chunk(48_000_000, vec![0.15, 0.15]));

        synchronizer.push_system(chunk(120_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 3);

        let (paired, sys_only, mic_only, _) = synchronizer.diagnostics();
        assert_eq!(paired, 1);
        assert_eq!(sys_only, 1);
        assert_eq!(mic_only, 1);
    }

    #[test]
    fn handles_system_only_recording() {
        let mut synchronizer = AudioSynchronizer::default();
        synchronizer.push_system(chunk(0, vec![0.5, 0.5]));
        synchronizer.push_system(chunk(80_000_000, vec![0.3, 0.3]));

        let results = synchronizer.drain_mixed();
        assert!(results.len() >= 1, "should emit at least 1 chunk");
        assert!(results[0].is_ok());
    }

    #[test]
    fn handles_mic_only_recording() {
        let mut synchronizer = AudioSynchronizer::default();
        synchronizer.push_mic(chunk(0, vec![0.2, 0.2]));
        synchronizer.push_mic(chunk(80_000_000, vec![0.3, 0.3]));

        let results = synchronizer.drain_mixed();
        assert!(results.len() >= 1, "should emit at least 1 chunk");
        assert!(results[0].is_ok());
    }

    #[test]
    fn drain_final_flushes_all_remaining_windows() {
        let mut synchronizer = AudioSynchronizer::default();

        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(25_000_000, vec![0.25, 0.25]));

        let results = synchronizer.drain_final();
        assert_eq!(results.len(), 2);

        assert!(results[0].0.has_system);
        assert!(!results[0].0.has_mic);
        assert!(results[0].1);

        assert!(!results[1].0.has_system);
        assert!(results[1].0.has_mic);
        assert!(results[1].1);
    }

    #[test]
    fn drain_final_pairs_close_timestamps_in_same_window() {
        let mut synchronizer = AudioSynchronizer::default();

        synchronizer.push_system(chunk(5_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(8_000_000, vec![0.25, 0.25]));

        let results = synchronizer.drain_final();
        assert_eq!(results.len(), 1);

        assert!(results[0].0.has_system);
        assert!(results[0].0.has_mic);
        assert!(!results[0].1);
    }

    #[test]
    fn drain_final_handles_empty_windows() {
        let mut synchronizer = AudioSynchronizer::default();
        let results = synchronizer.drain_final();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn drain_final_preserves_timestamp_order() {
        let mut synchronizer = AudioSynchronizer::default();

        synchronizer.push_system(chunk(200_000_000, vec![0.5, 0.5]));
        synchronizer.push_system(chunk(0, vec![0.3, 0.3]));
        synchronizer.push_mic(chunk(100_000_000, vec![0.25, 0.25]));

        let results = synchronizer.drain_final();
        assert_eq!(results.len(), 3);

        assert_eq!(results[0].0.mixed.timestamp.nanos, 0);
        assert_eq!(results[1].0.mixed.timestamp.nanos, 100_000_000);
        assert_eq!(results[2].0.mixed.timestamp.nanos, 200_000_000);
    }

    #[test]
    fn writer_does_not_drop_late_mic_after_synchronizer_window_merge() {
        let mut synchronizer = AudioSynchronizer::default();

        for i in 0..5 {
            let base_ts = i * 20_000_000;
            synchronizer.push_system(chunk(base_ts + 2_000_000, vec![0.3, 0.3]));
            synchronizer.push_mic(chunk(base_ts + 10_000_000, vec![0.5, 0.5]));
        }

        synchronizer.push_system(chunk(200_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();

        assert!(
            results.len() >= 5,
            "expected at least 5 mixed chunks, got {}",
            results.len()
        );

        for r in &results {
            assert!(r.is_ok());
        }

        let (paired, _, _, _) = synchronizer.diagnostics();
        assert!(
            paired >= 5,
            "expected at least 5 paired windows, got {paired}"
        );
    }

    #[test]
    fn consumer_system_and_mic_realistic_interleaving_writes_non_silent_artifact() {
        let mut synchronizer = AudioSynchronizer::default();

        for i in 0..50 {
            let base_ts = i * 20_000_000;
            synchronizer.push_system(chunk(base_ts + 2_000_000, vec![0.3, 0.3]));
            synchronizer.push_mic(chunk(base_ts + 10_000_000, vec![0.5, 0.5]));
        }

        synchronizer.push_system(chunk(2_000_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();

        let mut total_rms = 0.0f32;
        let mut count = 0usize;
        for r in &results {
            if let Ok(synced) = r {
                let rms = compute_rms(&synced.mixed.samples);
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

        let (paired, sys_only, mic_only, _) = synchronizer.diagnostics();
        assert_eq!(sys_only, 0, "should have no system-only windows");
        assert_eq!(mic_only, 0, "should have no mic-only windows");
        assert!(paired >= 50, "should have at least 50 paired windows");
    }

    #[test]
    fn audio_synchronizer_preserves_mic_mono_metadata_when_system_arrives_first() {
        let mut sync = AudioSynchronizer::default();

        let system_chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(vec![0.3f32; 2048].into_boxed_slice()),
        };
        sync.push_system(system_chunk);

        let mic_chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 1,
            samples: Arc::from(vec![0.5f32; 1024].into_boxed_slice()),
        };
        sync.push_mic(mic_chunk);

        let system_chunk2 = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(100_000_000),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(vec![0.3f32; 2048].into_boxed_slice()),
        };
        sync.push_system(system_chunk2);

        let results = sync.drain_mixed();
        assert!(
            !results.is_empty(),
            "should have emitted at least one window"
        );

        for result in results {
            let synced = result.unwrap();
            assert!(!synced.mixed.samples.is_empty());
            assert_eq!(synced.mixed.sample_rate, 48_000);
            assert_eq!(synced.mixed.channels, 2);
        }
    }

    #[test]
    fn audio_synchronizer_preserves_system_stereo_metadata_when_mic_arrives_first() {
        let mut sync = AudioSynchronizer::default();

        let mic_chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 1,
            samples: Arc::from(vec![0.5f32; 1024].into_boxed_slice()),
        };
        sync.push_mic(mic_chunk);

        let system_chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(vec![0.3f32; 2048].into_boxed_slice()),
        };
        sync.push_system(system_chunk);

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
            let synced = result.unwrap();
            assert!(!synced.mixed.samples.is_empty());
            assert_eq!(synced.mixed.sample_rate, 48_000);
            assert_eq!(synced.mixed.channels, 2);
        }
    }

    #[test]
    fn dual_source_offset_does_not_discard_mic_windows() {
        // BUG-005 Critical 1: When system audio leads mic by a systematic offset,
        // the max-based watermark emits system-only windows before mic chunks
        // land in those windows. With source-aware watermark (min), the synchronizer
        // should wait for mic to catch up.
        let mut synchronizer = dual_source_synchronizer();

        // System fills windows 0-2, arrives first.
        synchronizer.push_system(chunk(5_000_000, vec![0.3, 0.3]));
        synchronizer.push_system(chunk(25_000_000, vec![0.3, 0.3]));
        synchronizer.push_system(chunk(45_000_000, vec![0.3, 0.3]));

        // System continues advancing the watermark — with max-based watermark,
        // this causes windows 0-2 to be emitted as system-only BEFORE mic arrives.
        synchronizer.push_system(chunk(120_000_000, vec![0.1, 0.1]));

        // Now mic arrives for the same recording windows (offset by callback delay).
        synchronizer.push_mic(chunk(10_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(30_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(50_000_000, vec![0.5, 0.5]));

        // Drain whatever is left.
        synchronizer.push_system(chunk(200_000_000, vec![0.1, 0.1]));
        let _more = synchronizer.drain_mixed();

        let (paired, sys_only, mic_only, _) = synchronizer.diagnostics();
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
        let mut synchronizer = AudioSynchronizer::default();

        // 60ms of 48kHz stereo = 60 * 48000 / 1000 = 2880 frames = 5760 samples
        let samples_60ms = vec![0.3f32; 5760];
        synchronizer.push_system(AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(samples_60ms.into_boxed_slice()),
        });

        synchronizer.push_system(chunk(200_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();

        assert_eq!(
            results.len(),
            3,
            "expected 3 windows from 60ms chunk, got {}",
            results.len()
        );

        for (i, result) in results.iter().enumerate() {
            let synced = result.as_ref().unwrap();
            assert_eq!(
                synced.mixed.samples.len(),
                1920,
                "window {i} should have 1920 samples"
            );
            assert_eq!(synced.mixed.sample_rate, 48_000);
            assert_eq!(synced.mixed.channels, 2);
        }
    }

    #[test]
    fn synchronizer_splits_long_mic_callback_to_multiple_windows() {
        let mut synchronizer = AudioSynchronizer::default();

        // 40ms of 48kHz mono = 40 * 48000 / 1000 = 1920 frames = 1920 samples
        let samples_40ms = vec![0.5f32; 1920];
        synchronizer.push_mic(AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 1,
            samples: Arc::from(samples_40ms.into_boxed_slice()),
        });

        synchronizer.push_mic(chunk(200_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();

        assert_eq!(
            results.len(),
            2,
            "expected 2 windows from 40ms chunk, got {}",
            results.len()
        );
    }

    #[test]
    fn synchronizer_does_not_emit_fast_source_before_slow_source_watermark() {
        // With source-aware watermark, windows beyond the slow source's latest
        // timestamp are held — they should NOT be emitted as system-only.
        let mut synchronizer = dual_source_synchronizer();

        // System fills windows 0-4.
        for i in 0..5 {
            synchronizer.push_system(chunk(i * 20_000_000 + 5_000_000, vec![0.3, 0.3]));
        }

        // Mic only in windows 0 and 1.
        synchronizer.push_mic(chunk(5_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(25_000_000, vec![0.5, 0.5]));

        // Advance mic to window 4 (85ms → window_idx=4).
        // watermark = min(105ms, 85ms) - 40ms = 45ms
        // Windows ending before 45ms: 0 (20ms), 1 (40ms) → 2 windows emitted.
        // Windows 2-4 are correctly held (mic hasn't advanced past them yet).
        synchronizer.push_mic(chunk(85_000_000, vec![0.5, 0.5]));

        let results = synchronizer.drain_mixed();

        let (paired, sys_only, _mic_only, _) = synchronizer.diagnostics();
        assert_eq!(paired, 2, "windows 0-1 should be paired");
        assert_eq!(sys_only, 0, "no system-only windows should be emitted");
        // Only 2 windows emitted — the min-watermark correctly holds windows 2-4.
        assert_eq!(
            results.len(),
            2,
            "expected 2 windows (held by min-watermark), got {}",
            results.len()
        );
    }

    #[test]
    fn audio_synchronizer_dual_source_waits_for_initial_slow_source_within_grace() {
        // When both sources are requested but only system has arrived,
        // synchronizer should NOT emit within the grace period.
        let config = AudioSynchronizerConfig {
            requested_system_audio: true,
            requested_microphone: true,
            source_start_grace_nanos: 500_000_000, // 500ms grace
            ..Default::default()
        };
        let mut sync =
            AudioSynchronizer::new(SimpleAudioMixer::new(DenoiseMode::default()), config);

        // System arrives at t=0
        sync.push_system(chunk(0, vec![0.3, 0.3]));
        // System continues at t=20ms, 40ms
        sync.push_system(chunk(20_000_000, vec![0.3, 0.3]));
        sync.push_system(chunk(40_000_000, vec![0.3, 0.3]));

        // Within grace period — should NOT emit
        let result = sync.drain_mixed();
        assert!(
            result.is_empty(),
            "should wait for mic within grace period, got {} chunks",
            result.len()
        );
    }

    #[test]
    fn audio_synchronizer_dual_source_emits_after_start_grace_timeout() {
        // When grace period expires without the second source arriving,
        // synchronizer should emit with timeout marking.
        let config = AudioSynchronizerConfig {
            requested_system_audio: true,
            requested_microphone: true,
            source_start_grace_nanos: 100_000_000, // 100ms grace
            ..Default::default()
        };
        let mut sync =
            AudioSynchronizer::new(SimpleAudioMixer::new(DenoiseMode::default()), config);

        // System arrives at t=0
        sync.push_system(chunk(0, vec![0.3, 0.3]));
        // System continues well past grace period (200ms, 400ms)
        sync.push_system(chunk(200_000_000, vec![0.3, 0.3]));
        sync.push_system(chunk(400_000_000, vec![0.3, 0.3]));

        let result = sync.drain_mixed();
        assert!(!result.is_empty(), "should emit after grace timeout");
        for chunk_result in &result {
            let synced = chunk_result.as_ref().unwrap();
            assert!(
                synced.emitted_due_to_timeout,
                "should be marked as timeout emission"
            );
        }
    }

    #[test]
    fn audio_synchronizer_marks_timeout_windows_when_source_stalls() {
        // When both sources start together but one stalls,
        // the synchronizer should emit stalled-source windows as timeout.
        let config = AudioSynchronizerConfig {
            requested_system_audio: true,
            requested_microphone: true,
            source_stall_timeout_nanos: 200_000_000, // 200ms stall timeout
            source_start_grace_nanos: 0,
            ..Default::default()
        };
        let mut sync =
            AudioSynchronizer::new(SimpleAudioMixer::new(DenoiseMode::default()), config);

        // Both sources start together at t=0
        sync.push_system(chunk(0, vec![0.3, 0.3]));
        sync.push_mic(chunk(0, vec![0.5, 0.5]));

        // Then mic stalls — system continues alone for 300ms
        for i in 1..=15 {
            sync.push_system(chunk(i * 20_000_000, vec![0.3, 0.3]));
        }

        let result = sync.drain_mixed();
        let timeout_count = result
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .filter(|c| c.emitted_due_to_timeout)
            .count();
        assert!(
            timeout_count > 0,
            "should have timeout-emitted windows when mic stalls, got {}",
            timeout_count
        );
    }

    #[test]
    fn drain_mixed_with_no_input_returns_empty() {
        // Finding 2: When no audio is pushed, drain should return empty.
        // The mixer returns an error for (None, None) which is correctly
        // handled by skipping the window.
        let mut synchronizer = AudioSynchronizer::default();
        let results = synchronizer.drain_mixed();
        assert!(
            results.is_empty(),
            "should return empty when no input, got {} chunks",
            results.len()
        );
    }

    #[test]
    fn drain_final_with_no_input_returns_empty() {
        // Finding 2: drain_final should also return empty when no input.
        let mut synchronizer = AudioSynchronizer::default();
        let results = synchronizer.drain_final();
        assert!(
            results.is_empty(),
            "should return empty when no input, got {} chunks",
            results.len()
        );
    }
}
