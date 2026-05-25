use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::core::frame::MediaTimestamp;

/// Normalizes raw timestamps so the first timestamp in a session becomes zero.
///
/// Each capture stream (video, system audio) should have its own normalizer
/// to handle different clock bases independently.
#[derive(Debug, Default)]
pub struct TimestampNormalizer {
    first_raw_nanos: Mutex<Option<u64>>,
}

impl TimestampNormalizer {
    /// Converts a raw nanosecond timestamp to a session-relative timestamp.
    ///
    /// The first call establishes the session origin (returns 0).
    /// Subsequent calls return the delta from that origin.
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

/// Monotonic sample clock for microphone capture.
///
/// Generates session-relative timestamps based on the number of
/// interleaved audio samples emitted, derived from sample rate and channel count.
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

    /// Returns the timestamp for the next batch of `sample_count` interleaved samples.
    ///
    /// Advances the internal frame counter so subsequent calls return later timestamps.
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
