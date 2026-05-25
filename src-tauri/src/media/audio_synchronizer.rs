use crate::app::error::AppResult;
use crate::core::frame::{AudioChunk, MixedAudioChunk};
use crate::media::audio_mixer::{AudioMixer, SimpleAudioMixer};

/// Pairs system audio and microphone chunks by timestamp and mixes them.
pub struct AudioSynchronizer<M: AudioMixer = SimpleAudioMixer> {
    mixer: M,
}

impl<M: AudioMixer> AudioSynchronizer<M> {
    pub fn new(mixer: M) -> Self {
        Self { mixer }
    }

    /// Mixes a pair of system and microphone audio chunks.
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
