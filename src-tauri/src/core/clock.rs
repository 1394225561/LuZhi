use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

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

/// Shared recording session clock.
///
/// Captured once at recording start and shared with all capture sources
/// so timestamps from SCK (CMSampleBuffer host time) and cpal (derived
/// from `Instant::now()`) share one real-time basis.
#[derive(Debug)]
pub struct SessionClock {
    start: Instant,
}

impl SessionClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Nanoseconds elapsed since the session clock was created.
    pub fn elapsed_nanos(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }
}

impl Default for SessionClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Monotonic sample clock for microphone capture.
///
/// Generates session-relative timestamps based on the number of
/// interleaved audio samples emitted, derived from sample rate and channel count.
/// When anchored to a `SessionClock`, the first timestamp reflects the real
/// elapsed time since recording started.
///
/// **Lazy offset**: Use `with_lazy_offset()` + `initialize_offset()` to set the
/// session offset on the first audio callback, preventing stream-build delay from
/// creating a systematic timestamp offset (BUG-005 Important 1).
#[derive(Debug)]
pub struct AudioSampleClock {
    sample_rate: u32,
    channels: u16,
    emitted_frames: AtomicU64,
    /// Offset added to every sample-count-derived timestamp so the first
    /// chunk's timestamp equals the wall-clock elapsed time at callback entry.
    /// Uses AtomicU64 to support lazy initialization from `&self` (via CAS).
    session_offset_nanos: AtomicU64,
}

impl AudioSampleClock {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        assert!(sample_rate > 0, "sample rate must be greater than zero");
        assert!(channels > 0, "channel count must be greater than zero");

        Self {
            sample_rate,
            channels,
            emitted_frames: AtomicU64::new(0),
            session_offset_nanos: AtomicU64::new(0),
        }
    }

    /// Anchors this clock to a session clock so the first timestamp reflects
    /// the real elapsed time since the session started, rather than starting
    /// at zero independently.
    ///
    /// **Prefer `with_lazy_offset()` + `initialize_offset()` for mic capture**
    /// to avoid the stream-build delay offsetting timestamps.
    pub fn with_session_clock(self, session: &SessionClock) -> Self {
        self.session_offset_nanos
            .store(session.elapsed_nanos(), Ordering::Relaxed);
        self
    }

    /// Initializes the session offset lazily from the first audio callback.
    ///
    /// Computes `callback_now - buffer_duration` to estimate when capture
    /// actually started, preventing the stream-build delay from inflating
    /// the first chunk's timestamp.
    ///
    /// Uses CAS so only the first call takes effect; subsequent calls are no-ops.
    /// This method is safe to call from `&self` (no `&mut` required).
    pub fn initialize_offset(&self, session: &SessionClock, buffer_frames: u64) {
        let buffer_duration_nanos = buffer_frames * 1_000_000_000 / self.sample_rate as u64;
        let callback_now = session.elapsed_nanos();
        let offset = callback_now.saturating_sub(buffer_duration_nanos);

        // Only set if currently 0 (first-call wins).
        let _ = self.session_offset_nanos.compare_exchange(
            0,
            offset,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    /// Returns the timestamp for the next batch of `sample_count` interleaved samples.
    ///
    /// Advances the internal frame counter so subsequent calls return later timestamps.
    pub fn timestamp_for_interleaved_sample_count(&self, sample_count: usize) -> MediaTimestamp {
        let frames = sample_count as u64 / self.channels as u64;
        let start_frame = self.emitted_frames.fetch_add(frames, Ordering::Relaxed);
        let nanos = start_frame.saturating_mul(1_000_000_000) / self.sample_rate as u64;
        let offset = self.session_offset_nanos.load(Ordering::Relaxed);

        MediaTimestamp::from_nanos(nanos + offset)
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

    #[test]
    fn session_clock_elapsed_is_monotonic() {
        let clock = SessionClock::new();
        let first = clock.elapsed_nanos();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = clock.elapsed_nanos();
        assert!(second > first);
    }

    #[test]
    fn audio_clock_with_session_offset_starts_at_elapsed_time() {
        let session = SessionClock::new();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let clock = AudioSampleClock::new(48_000, 2).with_session_clock(&session);

        let first = clock.timestamp_for_interleaved_sample_count(960);
        // First timestamp should be close to the session elapsed time when
        // with_session_clock was called (a few microseconds at most).
        assert!(first.nanos > 0);
        assert!(first.nanos < session.elapsed_nanos() + 1_000_000);
    }

    #[test]
    fn audio_sample_clock_lazy_offset_anchors_first_callback_start() {
        let session = SessionClock::new();
        std::thread::sleep(std::time::Duration::from_millis(5));

        let clock = AudioSampleClock::new(48_000, 2);
        // Simulate first callback with 960 frames (10ms buffer @ 48kHz stereo).
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
        let lazy_clock = AudioSampleClock::new(48_000, 2);
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
}
