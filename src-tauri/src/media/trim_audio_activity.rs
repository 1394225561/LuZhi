use crate::core::cut::{AudioActivitySample, TrimConfig};
use crate::core::frame::{MediaTimestamp, MixedAudioChunk};

pub const BASE_RMS_BUCKET_NANOS: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseAudioActivitySample {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub sum_squares: f64,
    pub sample_count: u64,
}

impl BaseAudioActivitySample {
    pub fn rms(&self) -> f32 {
        if self.sample_count == 0 {
            return 0.0;
        }
        (self.sum_squares / self.sample_count as f64).sqrt() as f32
    }
}

#[derive(Default)]
pub struct BaseAudioActivityAnalyzer {
    bucket_start_nanos: Option<u64>,
    sum_squares: f64,
    sample_count: u64,
}

impl BaseAudioActivityAnalyzer {
    pub fn push_chunk(&mut self, chunk: &MixedAudioChunk) -> Vec<BaseAudioActivitySample> {
        if chunk.sample_rate == 0 || chunk.channels == 0 || chunk.samples.is_empty() {
            return Vec::new();
        }

        let frames = chunk.samples.len() as u64 / chunk.channels as u64;
        if frames == 0 {
            return Vec::new();
        }

        let mut output = Vec::new();
        for frame in 0..frames {
            let frame_nanos = chunk
                .timestamp
                .nanos
                .saturating_add(frame.saturating_mul(1_000_000_000) / chunk.sample_rate as u64);

            let bucket_start = self.bucket_start_nanos.get_or_insert(frame_nanos);
            if frame_nanos > bucket_start.saturating_add(BASE_RMS_BUCKET_NANOS)
                && self.sample_count > 0
            {
                output.push(self.take_bucket());
                self.bucket_start_nanos = Some(frame_nanos);
            }

            for channel in 0..chunk.channels as u64 {
                let index = (frame * chunk.channels as u64 + channel) as usize;
                if let Some(sample) = chunk.samples.get(index) {
                    let sample = *sample as f64;
                    self.sum_squares += sample * sample;
                    self.sample_count += 1;
                }
            }
        }

        output
    }

    pub fn flush(&mut self) -> Option<BaseAudioActivitySample> {
        if self.sample_count == 0 {
            return None;
        }
        Some(self.take_bucket())
    }

    fn take_bucket(&mut self) -> BaseAudioActivitySample {
        let start = self.bucket_start_nanos.unwrap_or(0);
        let sample = BaseAudioActivitySample {
            start: MediaTimestamp::from_nanos(start),
            end: MediaTimestamp::from_nanos(start.saturating_add(BASE_RMS_BUCKET_NANOS)),
            sum_squares: self.sum_squares,
            sample_count: self.sample_count,
        };
        self.bucket_start_nanos = None;
        self.sum_squares = 0.0;
        self.sample_count = 0;
        sample
    }
}

pub fn aggregate_base_audio_activity(
    buckets: &[BaseAudioActivitySample],
    config: TrimConfig,
) -> Vec<AudioActivitySample> {
    if buckets.is_empty() {
        return Vec::new();
    }

    let mut output = Vec::new();
    let mut window_start = buckets[0].start.nanos;
    let mut window_end = window_start.saturating_add(config.rms_window_nanos);
    let mut sum_squares = 0.0f64;
    let mut sample_count = 0u64;

    for bucket in buckets {
        if bucket.start.nanos >= window_end && sample_count > 0 {
            output.push(activity_sample(window_start, window_end, sum_squares, sample_count));
            window_start = bucket.start.nanos;
            window_end = window_start.saturating_add(config.rms_window_nanos);
            sum_squares = 0.0;
            sample_count = 0;
        }

        sum_squares += bucket.sum_squares;
        sample_count += bucket.sample_count;
    }

    if sample_count > 0 {
        let end = buckets.last().map(|bucket| bucket.end.nanos).unwrap_or(window_end);
        output.push(activity_sample(window_start, end, sum_squares, sample_count));
    }

    output
}

fn activity_sample(
    start: u64,
    end: u64,
    sum_squares: f64,
    sample_count: u64,
) -> AudioActivitySample {
    let rms = if sample_count == 0 {
        0.0
    } else {
        (sum_squares / sample_count as f64).sqrt() as f32
    };
    AudioActivitySample {
        start: MediaTimestamp::from_nanos(start),
        end: MediaTimestamp::from_nanos(end),
        rms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cut::{TrimConfig, TrimSensitivity};
    use crate::core::frame::{MediaTimestamp, MixedAudioChunk};
    use std::sync::Arc;

    fn chunk(start: u64, sample_count: usize, value: f32) -> MixedAudioChunk {
        MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(start),
            sample_rate: 10,
            channels: 1,
            samples: Arc::from(vec![value; sample_count].into_boxed_slice()),
        }
    }

    #[test]
    fn base_analyzer_emits_100ms_buckets() {
        let mut analyzer = BaseAudioActivityAnalyzer::default();
        // 20 samples at sample_rate=10 → timestamps 0, 100ms, 200ms, …, 1900ms.
        // This spans two 100ms bucket windows. Frames at exact boundaries
        // stay in the current bucket; emission happens when a frame crosses
        // past the boundary. Expected: 9 emitted buckets (the last bucket
        // with 2 samples remains in the analyzer until flush).
        let samples = analyzer.push_chunk(&chunk(0, 20, 0.5));

        assert_eq!(samples.len(), 9);
        assert_eq!(samples[0].start.nanos, 0);
        assert_eq!(samples[0].end.nanos, BASE_RMS_BUCKET_NANOS);
        assert!((samples[0].rms() - 0.5).abs() < 0.001);
        assert_eq!(samples[0].sample_count, 2);

        // Flush emits the remaining bucket.
        let remaining = analyzer.flush().unwrap();
        assert_eq!(remaining.sample_count, 2);
        assert_eq!(remaining.start.nanos, 1_800_000_000);
    }

    #[test]
    fn aggregate_base_buckets_uses_current_sensitivity_window() {
        let buckets = (0..10)
            .map(|i| BaseAudioActivitySample {
                start: MediaTimestamp::from_nanos(i * BASE_RMS_BUCKET_NANOS),
                end: MediaTimestamp::from_nanos((i + 1) * BASE_RMS_BUCKET_NANOS),
                sum_squares: 0.0,
                sample_count: 1,
            })
            .collect::<Vec<_>>();

        let high = aggregate_base_audio_activity(
            &buckets,
            TrimConfig::from_sensitivity(TrimSensitivity::High),
        );
        let low = aggregate_base_audio_activity(
            &buckets,
            TrimConfig::from_sensitivity(TrimSensitivity::Low),
        );

        assert_eq!(high.len(), 2);
        assert_eq!(high[0].end.nanos - high[0].start.nanos, 500_000_000);
        assert_eq!(low.len(), 1);
        assert_eq!(low[0].end.nanos - low[0].start.nanos, 1_000_000_000);
    }

    #[test]
    fn base_analyzer_accumulates_short_chunks_before_emitting_bucket() {
        let mut analyzer = BaseAudioActivityAnalyzer::default();

        // Five 20ms chunks at 0, 20ms, 40ms, 60ms, 80ms — all within the
        // first 100ms bucket [0, 100000000ns). No bucket is emitted yet.
        for i in 0..5 {
            let samples = analyzer.push_chunk(&MixedAudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 20_000_000),
                sample_rate: 1_000,
                channels: 1,
                samples: Arc::from(vec![0.25; 20].into_boxed_slice()),
            });
            assert!(samples.is_empty(), "chunk {i} should not trigger bucket emission");
        }

        // A 6th chunk at 120ms crosses the bucket boundary (>100ms), emitting
        // the accumulated bucket with 100 samples (5 chunks × 20 samples).
        let samples = analyzer.push_chunk(&MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(120_000_000),
            sample_rate: 1_000,
            channels: 1,
            samples: Arc::from(vec![0.25; 20].into_boxed_slice()),
        });

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].start.nanos, 0);
        assert_eq!(samples[0].end.nanos, BASE_RMS_BUCKET_NANOS);
        assert_eq!(samples[0].sample_count, 100);
    }
}
