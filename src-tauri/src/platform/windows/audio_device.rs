use std::sync::Arc;

use crate::core::clock::AudioSampleClock;
use crate::core::frame::{AudioChunk, MediaTimestamp};

pub fn interleaved_i16_to_f32(input: &[i16]) -> Vec<f32> {
    input
        .iter()
        .map(|sample| (*sample as f32 / i16::MAX as f32).clamp(-1.0, 1.0))
        .collect()
}

pub fn interleaved_f32_to_chunk(
    timestamp: MediaTimestamp,
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
) -> AudioChunk {
    AudioChunk {
        timestamp,
        sample_rate,
        channels,
        samples: Arc::from(samples.into_boxed_slice()),
    }
}

pub fn timestamp_for_packet(clock: &AudioSampleClock, sample_count: usize) -> MediaTimestamp {
    clock.timestamp_for_interleaved_sample_count(sample_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i16_conversion_clamps_to_unit_range() {
        let samples = interleaved_i16_to_f32(&[i16::MIN, 0, i16::MAX]);

        assert!(samples[0] <= -1.0);
        assert_eq!(samples[1], 0.0);
        assert!(samples[2] <= 1.0);
    }

    #[test]
    fn chunk_preserves_wasapi_format_metadata() {
        let chunk =
            interleaved_f32_to_chunk(MediaTimestamp::from_nanos(5), 44100, 2, vec![0.0; 882]);

        assert_eq!(chunk.timestamp.nanos, 5);
        assert_eq!(chunk.sample_rate, 44100);
        assert_eq!(chunk.channels, 2);
        assert_eq!(chunk.samples.len(), 882);
    }

    #[test]
    fn packet_timestamp_advances_by_sample_frames() {
        let clock = AudioSampleClock::new(48_000, 2);
        let first = timestamp_for_packet(&clock, 960);
        let second = timestamp_for_packet(&clock, 960);

        assert_eq!(first.nanos, 0);
        assert_eq!(second.nanos, 10_000_000);
    }
}
