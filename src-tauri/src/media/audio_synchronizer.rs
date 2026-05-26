use std::collections::VecDeque;

use crate::app::error::AppResult;
use crate::core::frame::{AudioChunk, MixedAudioChunk};
use crate::media::audio_mixer::{AudioMixer, SimpleAudioMixer};

/// Pairs system audio and microphone chunks by timestamp and mixes them.
///
/// Chunks are queued via `push_system` / `push_mic` and paired by proximity
/// when `drain_mixed` is called.  The pairing window is 10 ms — within one
/// audio callback period at typical sample rates.
pub struct AudioSynchronizer<M: AudioMixer = SimpleAudioMixer> {
    mixer: M,
    system_queue: VecDeque<AudioChunk>,
    mic_queue: VecDeque<AudioChunk>,
    /// Latest system chunk timestamp seen across all drain calls.
    /// Used as the watermark basis: mic chunks more than PAIR_WINDOW_NANOS
    /// behind this are too old to ever match a future system chunk.
    latest_system_ts: u64,
}

/// Maximum timestamp gap (in nanoseconds) to consider two chunks a pair.
/// 10 ms = 10_000_000 ns.
const PAIR_WINDOW_NANOS: u64 = 10_000_000;

/// Maximum queue size to bound memory when one source produces faster than
/// the other. Oldest chunks are dropped first.
const MAX_QUEUE_SIZE: usize = 1000;

/// Maximum time a single-source chunk is held waiting for a match before
/// being emitted alone (500 ms).
const MAX_HOLD_NANOS: u64 = 500_000_000;

impl<M: AudioMixer> AudioSynchronizer<M> {
    pub fn new(mixer: M) -> Self {
        Self {
            mixer,
            system_queue: VecDeque::new(),
            mic_queue: VecDeque::new(),
            latest_system_ts: 0,
        }
    }

    /// Enqueue a system audio chunk for ordered pairing.
    pub fn push_system(&mut self, chunk: AudioChunk) {
        if self.system_queue.len() >= MAX_QUEUE_SIZE {
            self.system_queue.pop_front();
        }
        self.system_queue.push_back(chunk);
    }

    /// Enqueue a microphone audio chunk for ordered pairing.
    pub fn push_mic(&mut self, chunk: AudioChunk) {
        if self.mic_queue.len() >= MAX_QUEUE_SIZE {
            self.mic_queue.pop_front();
        }
        self.mic_queue.push_back(chunk);
    }

    /// Drain all queued chunks, pairing system and mic by closest timestamp.
    ///
    /// For each system chunk, searches the entire mic queue for the closest
    /// match within `PAIR_WINDOW_NANOS`. Unmatched mic chunks are held until
    /// they age out beyond the watermark (latest system timestamp minus the
    /// pairing window) or `MAX_HOLD_NANOS`, whichever is later.
    pub fn drain_mixed(&mut self) -> Vec<AppResult<MixedAudioChunk>> {
        let mut results = Vec::new();

        // Phase 1: For each system chunk, find the closest mic chunk within
        // the pairing window by searching the entire queue.
        while let Some(system) = self.system_queue.pop_front() {
            let sys_ts = system.timestamp.nanos;
            self.latest_system_ts = self.latest_system_ts.max(sys_ts);

            let mut best_idx: Option<usize> = None;
            let mut best_diff = PAIR_WINDOW_NANOS + 1;

            for (i, mic_chunk) in self.mic_queue.iter().enumerate() {
                let diff = mic_chunk.timestamp.nanos.abs_diff(sys_ts);
                if diff <= PAIR_WINDOW_NANOS && diff < best_diff {
                    best_diff = diff;
                    best_idx = Some(i);
                }
            }

            let mic = best_idx.map(|idx| self.mic_queue.remove(idx).unwrap());
            results.push(self.mixer.mix(Some(&system), mic.as_ref()));
        }

        // Phase 2: Emit mic-only chunks that are too old to ever match a
        // future system chunk.
        //
        // If no system chunks have ever been seen, emit mic chunks without
        // waiting — this is either a mic-only recording or system hasn't
        // started producing yet. In the latter case a few early mic-only
        // chunks is an acceptable tradeoff for the low-latency policy.
        if self.latest_system_ts == 0 {
            while let Some(mic) = self.mic_queue.pop_front() {
                results.push(self.mixer.mix(None, Some(&mic)));
            }
        } else {
            // Two horizons govern age-out:
            //   system_horizon = latest_system_ts - PAIR_WINDOW_NANOS
            //     Mic chunks behind this cannot match any future system chunk
            //     (system timestamps only move forward).
            //   mic_horizon = newest_mic_ts - MAX_HOLD_NANOS
            //     Bounds hold time via queue span when mic is far ahead.
            let system_horizon = self.latest_system_ts.saturating_sub(PAIR_WINDOW_NANOS);

            let mic_horizon = self
                .mic_queue
                .back()
                .map(|newest| newest.timestamp.nanos.saturating_sub(MAX_HOLD_NANOS))
                .unwrap_or(0);

            let horizon = system_horizon.max(mic_horizon);

            while let Some(mic) = self.mic_queue.pop_front() {
                if mic.timestamp.nanos < horizon {
                    results.push(self.mixer.mix(None, Some(&mic)));
                } else {
                    self.mic_queue.push_front(mic);
                    break;
                }
            }
        }

        results
    }

    /// Mixes a single pair of system and microphone audio chunks.
    ///
    /// At least one of `system` or `mic` must be `Some`.
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
    fn drains_ordered_pairs_by_timestamp() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());
        synchronizer.push_system(chunk(0, vec![0.5, 0.5]));
        synchronizer.push_system(chunk(10_000_000, vec![0.3, 0.3]));
        synchronizer.push_mic(chunk(0, vec![0.25, 0.25]));
        synchronizer.push_mic(chunk(10_000_000, vec![0.1, 0.1]));

        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());
    }

    #[test]
    fn handles_system_only_recording() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());
        synchronizer.push_system(chunk(0, vec![0.5, 0.5]));

        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn handles_mic_only_recording() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());
        synchronizer.push_mic(chunk(0, vec![0.2, 0.2]));

        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn emits_mic_when_no_system_ever_seen() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());
        // Mic at 50ms — outside the pairing window from a future system at 0.
        // But no system has ever been seen, so emit immediately.
        synchronizer.push_mic(chunk(0, vec![0.2, 0.2]));
        synchronizer.push_system(chunk(50_000_000, vec![0.5, 0.5]));

        let results = synchronizer.drain_mixed();
        // Mic is emitted because latest_system_ts was 0 when Phase 2 ran.
        // Then system arrives, emits alone.
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn holds_mic_when_system_seen_and_within_window_potential() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());
        // First drain: establish latest_system_ts at 10ms.
        synchronizer.push_system(chunk(10_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(12_000_000, vec![0.1, 0.1]));
        let results = synchronizer.drain_mixed();
        // System at 10ms pairs with mic at 12ms (diff=2ms < 10ms).
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());

        // Second drain: mic at 5ms arrives after the system horizon is at
        // 10ms. system_horizon = 10ms - 10ms = 0, so mic at 5ms < 0 is
        // false — it's held for a future system chunk.
        synchronizer.push_mic(chunk(5_000_000, vec![0.3, 0.3]));
        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 0); // held

        // Third drain: system at 20ms moves the horizon to 10ms.
        // Mic at 5ms < 10ms → aged out, emitted as mic-only.
        synchronizer.push_system(chunk(20_000_000, vec![0.7, 0.7]));
        let results = synchronizer.drain_mixed();
        assert_eq!(results.len(), 2); // system alone + aged-out mic
    }

    #[test]
    fn ages_out_mic_when_queue_span_exceeds_max_hold() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());
        // Establish system horizon so we enter the watermark path.
        synchronizer.push_system(chunk(10_000_000, vec![0.5, 0.5]));
        synchronizer.push_mic(chunk(10_000_000, vec![0.25, 0.25]));
        let _ = synchronizer.drain_mixed();

        // Push mic chunks spanning > MAX_HOLD_NANOS (500ms).
        synchronizer.push_mic(chunk(0, vec![0.1, 0.1]));
        synchronizer.push_mic(chunk(600_000_000, vec![0.2, 0.2]));

        let results = synchronizer.drain_mixed();
        // mic_horizon = 600ms - 500ms = 100ms. Mic at 0 < 100ms → aged out.
        // Mic at 600ms held.
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }
}
