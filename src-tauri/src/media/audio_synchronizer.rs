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
}

/// Maximum timestamp gap (in nanoseconds) to consider two chunks a pair.
/// 10 ms = 10_000_000 ns.
const PAIR_WINDOW_NANOS: u64 = 10_000_000;

impl<M: AudioMixer> AudioSynchronizer<M> {
    pub fn new(mixer: M) -> Self {
        Self {
            mixer,
            system_queue: VecDeque::new(),
            mic_queue: VecDeque::new(),
        }
    }

    /// Enqueue a system audio chunk for ordered pairing.
    pub fn push_system(&mut self, chunk: AudioChunk) {
        self.system_queue.push_back(chunk);
    }

    /// Enqueue a microphone audio chunk for ordered pairing.
    pub fn push_mic(&mut self, chunk: AudioChunk) {
        self.mic_queue.push_back(chunk);
    }

    /// Drain all queued chunks, pairing system and mic by closest timestamp.
    ///
    /// Returns one `MixedAudioChunk` per consumed system chunk, plus any
    /// leftover mic-only chunks.
    pub fn drain_mixed(&mut self) -> Vec<AppResult<MixedAudioChunk>> {
        let mut results = Vec::new();

        while let Some(system) = self.system_queue.pop_front() {
            let mic = if let Some(front_mic) = self.mic_queue.front() {
                if front_mic.timestamp.nanos.abs_diff(system.timestamp.nanos) <= PAIR_WINDOW_NANOS {
                    self.mic_queue.pop_front()
                } else {
                    None
                }
            } else {
                None
            };
            results.push(self.mixer.mix(Some(&system), mic.as_ref()));
        }

        // Leftover mic-only chunks.
        while let Some(mic) = self.mic_queue.pop_front() {
            results.push(self.mixer.mix(None, Some(&mic)));
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
    fn leaves_unmatched_mic_for_next_system() {
        let mut synchronizer = AudioSynchronizer::new(SimpleAudioMixer::new());
        // Mic arrives 50ms before system — outside the 10ms window.
        synchronizer.push_mic(chunk(0, vec![0.2, 0.2]));
        synchronizer.push_system(chunk(50_000_000, vec![0.5, 0.5]));

        let results = synchronizer.drain_mixed();
        // Mic is unmatched, gets its own entry; system also gets its own.
        assert_eq!(results.len(), 2);
    }
}
