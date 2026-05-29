use crate::core::cut::{
    AudioActivitySample, CutReason, CutSegment, CutTimeline, FrameDiffSample, KeepSegment,
    TrimConfig,
};
use crate::core::frame::{FrameBuffer, MediaTimestamp, MixedAudioChunk, PixelFormat, VideoFrame};
use crate::core::processor::SilenceDetector;

/// Stateful windowed RMS aggregator for mixed audio.
///
/// Accumulates raw audio samples and emits one `AudioActivitySample` per
/// configured window (500ms–1000ms). This prevents short transient noise
/// from creating spurious silence candidates.
pub struct AudioRmsAnalyzer {
    config: TrimConfig,
    sample_buffer: Vec<f32>,
    window_start_nanos: u64,
    sample_rate: u32,
    channels: u16,
}

impl AudioRmsAnalyzer {
    pub fn new(config: TrimConfig) -> Self {
        Self {
            config,
            sample_buffer: Vec::new(),
            window_start_nanos: 0,
            sample_rate: 0,
            channels: 0,
        }
    }

    pub fn window_nanos(&self) -> u64 {
        self.config.rms_window_nanos
    }

    /// Feeds a single mixed audio chunk and returns one `AudioActivitySample`
    /// per complete configured window. Partial-window remainder is retained
    /// internally and merged into the next call or emitted by `flush()`.
    ///
    /// When the incoming chunk's timestamp has a discontinuity (gap > one window
    /// from the expected buffer position), the current remainder is flushed and
    /// the window resets to the new timestamp. This prevents timestamp drift
    /// when audio chunks are dropped or arrive out of order.
    pub fn push_chunk(&mut self, chunk: &MixedAudioChunk) -> Vec<AudioActivitySample> {
        if chunk.sample_rate == 0 || chunk.channels == 0 || chunk.samples.is_empty() {
            return Vec::new();
        }

        // Initialize on first chunk.
        if self.sample_rate == 0 {
            self.sample_rate = chunk.sample_rate;
            self.channels = chunk.channels;
            self.window_start_nanos = chunk.timestamp.nanos;
        }

        // Detect timestamp discontinuity: if the chunk timestamp is more than
        // one window ahead of where our buffer expects to be, flush the current
        // remainder and reset to the new timestamp. Only trigger on forward gaps
        // (chunk arriving later than expected), not on minor jitter.
        let buffer_duration_nanos = if self.sample_rate > 0 && self.channels > 0 {
            let buffer_frames = self.sample_buffer.len() as u64 / self.channels as u64;
            buffer_frames.saturating_mul(1_000_000_000) / self.sample_rate as u64
        } else {
            0
        };
        let expected_nanos = self.window_start_nanos + buffer_duration_nanos;
        let forward_gap = chunk.timestamp.nanos.saturating_sub(expected_nanos);
        let mut samples = Vec::new();
        if forward_gap > self.config.rms_window_nanos {
            // Discontinuity detected — flush remainder and reset.
            if let Some(remainder) = self.flush() {
                samples.push(remainder);
            }
            self.window_start_nanos = chunk.timestamp.nanos;
            self.sample_buffer.clear();
            // Re-initialize format from this chunk.
            self.sample_rate = chunk.sample_rate;
            self.channels = chunk.channels;
        }

        self.sample_buffer.extend_from_slice(&chunk.samples);

        // Number of interleaved samples that fit in one configured window.
        let window_samples =
            (self.sample_rate as u64 * self.channels as u64 * self.config.rms_window_nanos
                / 1_000_000_000) as usize;
        if window_samples == 0 {
            return samples;
        }

        while self.sample_buffer.len() >= window_samples {
            // Drain exactly one window worth of samples and compute RMS inline
            // to avoid allocating a temporary Vec per window.
            let mut sum_squares = 0.0f32;
            for s in self.sample_buffer.drain(..window_samples) {
                sum_squares += s * s;
            }
            let rms = (sum_squares / window_samples as f32).sqrt();

            let window_nanos = self.config.rms_window_nanos;
            let end_nanos = self.window_start_nanos.saturating_add(window_nanos);
            samples.push(AudioActivitySample {
                start: MediaTimestamp::from_nanos(self.window_start_nanos),
                end: MediaTimestamp::from_nanos(end_nanos),
                rms,
            });
            self.window_start_nanos = end_nanos;
        }

        samples
    }

    /// Flushes any remaining samples as a final window (shorter than configured).
    pub fn flush(&mut self) -> Option<AudioActivitySample> {
        if self.sample_buffer.is_empty() || self.sample_rate == 0 {
            return None;
        }

        let sum_squares = self.sample_buffer.iter().map(|s| s * s).sum::<f32>();
        let rms = (sum_squares / self.sample_buffer.len() as f32).sqrt();

        let frames = self.sample_buffer.len() as u64 / self.channels as u64;
        let buffer_nanos = frames.saturating_mul(1_000_000_000) / self.sample_rate as u64;
        let end_nanos = self.window_start_nanos.saturating_add(buffer_nanos);

        self.sample_buffer.clear();

        Some(AudioActivitySample {
            start: MediaTimestamp::from_nanos(self.window_start_nanos),
            end: MediaTimestamp::from_nanos(end_nanos),
            rms,
        })
    }
}

/// Computes low-resolution grayscale frame differences without touching React.
pub struct FrameDiffAnalyzer {
    thumb_width: u32,
    thumb_height: u32,
}

impl FrameDiffAnalyzer {
    pub fn new(thumb_width: u32, thumb_height: u32) -> Self {
        Self {
            thumb_width: thumb_width.max(1),
            thumb_height: thumb_height.max(1),
        }
    }

    pub fn diff_pair(
        &self,
        previous: &VideoFrame,
        current: &VideoFrame,
    ) -> Result<FrameDiffSample, String> {
        if previous.pixel_format != PixelFormat::Bgra8 || current.pixel_format != PixelFormat::Bgra8
        {
            return Err("帧差分仅支持 BGRA8 像素格式".to_string());
        }
        if previous.width == 0 || previous.height == 0 || current.width == 0 || current.height == 0
        {
            return Err("帧差分输入尺寸无效".to_string());
        }

        let previous_thumb = self.downsample_grayscale(previous)?;
        let current_thumb = self.downsample_grayscale(current)?;
        let diff_sum = previous_thumb
            .iter()
            .zip(current_thumb.iter())
            .map(|(a, b)| (*a as i16 - *b as i16).unsigned_abs() as f32 / 255.0)
            .sum::<f32>();
        let change_ratio = diff_sum / previous_thumb.len() as f32;

        Ok(FrameDiffSample {
            start: previous.timestamp,
            end: current.timestamp,
            change_ratio,
        })
    }

    fn downsample_grayscale(&self, frame: &VideoFrame) -> Result<Vec<u8>, String> {
        let bytes = match &frame.buffer {
            FrameBuffer::Owned(bytes) => bytes.as_ref(),
        };
        // stride_bytes accounts for row padding that ScreenCaptureKit may add.
        let stride = if frame.stride_bytes > 0 {
            frame.stride_bytes
        } else {
            frame.width as usize * 4
        };
        let min_stride = frame.width as usize * 4;
        if stride < min_stride {
            return Err(format!(
                "帧 stride ({stride}) 小于行像素字节数 ({min_stride})"
            ));
        }
        let expected = stride
            .checked_mul(frame.height as usize)
            .ok_or("stride × height 整数溢出")?;
        if bytes.len() < expected {
            return Err("帧像素数据长度不足".to_string());
        }

        let mut output = Vec::with_capacity((self.thumb_width * self.thumb_height) as usize);
        for y in 0..self.thumb_height {
            for x in 0..self.thumb_width {
                let src_x = (x as u64 * frame.width as u64 / self.thumb_width as u64) as usize;
                let src_y = (y as u64 * frame.height as u64 / self.thumb_height as u64) as usize;
                let offset = src_y * stride + src_x * 4;
                let b = bytes[offset] as f32;
                let g = bytes[offset + 1] as f32;
                let r = bytes[offset + 2] as f32;
                let gray = (0.114 * b + 0.587 * g + 0.299 * r)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                output.push(gray);
            }
        }
        Ok(output)
    }
}

/// Pure post-recording engine that combines audio silence and visual stillness.
pub struct SilenceDetectorEngine {
    config: TrimConfig,
}

impl SilenceDetectorEngine {
    pub fn new(config: TrimConfig) -> Self {
        Self { config }
    }

    fn is_candidate(&self, audio: &AudioActivitySample, visual: &FrameDiffSample) -> bool {
        audio.rms <= self.config.audio_rms_threshold
            && visual.change_ratio <= self.config.visual_change_threshold
            && ranges_overlap(
                audio.start.nanos,
                audio.end.nanos,
                visual.start.nanos,
                visual.end.nanos,
            )
    }

    fn build_keeps(&self, cuts: &[CutSegment], duration_nanos: u64) -> Vec<KeepSegment> {
        if cuts.is_empty() {
            return CutTimeline::empty(duration_nanos).keeps;
        }

        let mut keeps = Vec::new();
        let mut cursor = 0u64;
        for cut in cuts {
            if cursor < cut.start.nanos {
                keeps.push(KeepSegment {
                    start: MediaTimestamp::from_nanos(cursor),
                    end: cut.start,
                });
            }
            cursor = cut.end.nanos;
        }
        if cursor < duration_nanos {
            keeps.push(KeepSegment {
                start: MediaTimestamp::from_nanos(cursor),
                end: MediaTimestamp::from_nanos(duration_nanos),
            });
        }
        keeps
    }
}

impl SilenceDetector for SilenceDetectorEngine {
    fn analyze(
        &self,
        audio: &[AudioActivitySample],
        visual: &[FrameDiffSample],
        duration_nanos: u64,
    ) -> crate::app::error::AppResult<CutTimeline> {
        if audio.is_empty() || visual.is_empty() || duration_nanos == 0 {
            return Ok(CutTimeline::empty(duration_nanos));
        }

        // Two-pointer sweep: both audio and visual are time-ordered from the
        // consumer thread, so we can skip visual samples that end before the
        // current audio sample starts. This reduces the candidate scan from
        // O(N*M) to O(N + M).
        let mut candidates: Vec<(u64, u64, f32, f32)> = Vec::new();
        let mut vi = 0;
        for audio_sample in audio {
            // Advance visual cursor past samples that end before audio starts.
            while vi < visual.len() && visual[vi].end.nanos <= audio_sample.start.nanos {
                vi += 1;
            }
            // Check overlapping visual samples; break when visual starts after audio ends.
            for visual_sample in &visual[vi..] {
                if visual_sample.start.nanos >= audio_sample.end.nanos {
                    break;
                }
                if !self.is_candidate(audio_sample, visual_sample) {
                    continue;
                }
                let start = audio_sample.start.nanos.max(visual_sample.start.nanos);
                let end = audio_sample.end.nanos.min(visual_sample.end.nanos);
                if end > start {
                    candidates.push((start, end, audio_sample.rms, visual_sample.change_ratio));
                }
            }
        }
        candidates.sort_by_key(|candidate| candidate.0);

        let mut merged: Vec<(u64, u64, Vec<f32>, Vec<f32>)> = Vec::new();
        for (start, end, rms, change) in candidates {
            if let Some(last) = merged.last_mut() {
                if start <= last.1.saturating_add(self.config.merge_gap_nanos) {
                    last.1 = last.1.max(end);
                    last.2.push(rms);
                    last.3.push(change);
                    continue;
                }
            }
            merged.push((start, end, vec![rms], vec![change]));
        }

        let mut cuts = Vec::new();
        for (start, end, rms_values, visual_values) in merged {
            if end.saturating_sub(start) < self.config.min_candidate_nanos {
                continue;
            }
            let cut_start = start
                .saturating_add(self.config.buffer_nanos)
                .min(duration_nanos);
            let cut_end = end
                .saturating_sub(self.config.buffer_nanos)
                .min(duration_nanos);
            if cut_end <= cut_start {
                continue;
            }
            let cut_duration = cut_end - cut_start;
            if cut_duration < self.config.absolute_min_cut_nanos {
                continue;
            }

            let mean_audio_rms = mean(&rms_values);
            let mean_visual_change = mean(&visual_values);
            cuts.push(CutSegment {
                start: MediaTimestamp::from_nanos(cut_start),
                end: MediaTimestamp::from_nanos(cut_end),
                reason: CutReason::SilentAndStill,
                mean_audio_rms,
                mean_visual_change,
            });
        }

        let total_cut_nanos = cuts
            .iter()
            .map(|cut| cut.end.nanos.saturating_sub(cut.start.nanos))
            .sum();
        let keeps = self.build_keeps(&cuts, duration_nanos);

        Ok(CutTimeline {
            duration_nanos,
            cuts,
            keeps,
            total_cut_nanos,
        })
    }
}

fn ranges_overlap(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start < b_end && b_start < a_end
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::core::cut::{AudioActivitySample, FrameDiffSample, TrimSensitivity};

    fn mixed_chunk(start_nanos: u64, samples: Vec<f32>) -> MixedAudioChunk {
        MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(start_nanos),
            sample_rate: 48_000,
            channels: 1,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    #[test]
    fn rms_window_uses_configured_500ms_to_1000ms_window() {
        let low = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Low));
        let high = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::High));

        assert_eq!(low.window_nanos(), 1_000_000_000);
        assert_eq!(high.window_nanos(), 500_000_000);
    }

    #[test]
    fn silence_chunk_produces_low_rms_sample() {
        let mut analyzer =
            AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        // 48_000 samples @ 48kHz mono = 1s > 750ms window → emits one sample.
        let samples = analyzer.push_chunk(&mixed_chunk(0, vec![0.0; 48_000]));
        assert_eq!(samples.len(), 1);
        assert!(samples[0].rms < 0.001);
    }

    #[test]
    fn background_noise_remains_explainable() {
        let mut analyzer =
            AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        let samples = analyzer.push_chunk(&mixed_chunk(0, vec![0.015; 48_000]));
        assert_eq!(samples.len(), 1);
        assert!(samples[0].rms > 0.014);
        assert!(samples[0].rms < 0.016);
    }

    #[test]
    fn loud_chunk_is_not_silent() {
        let mut analyzer =
            AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        let samples = analyzer.push_chunk(&mixed_chunk(0, vec![0.25; 48_000]));
        assert_eq!(samples.len(), 1);
        assert!(samples[0].rms > 0.2);
    }

    #[test]
    fn audio_rms_aggregates_small_chunks_by_window() {
        let mut analyzer =
            AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        // 10ms chunks @ 48kHz mono = 480 samples each. Need 750ms / 10ms = 75 chunks.
        let mut sample_count = 0;
        for i in 0..80 {
            let chunk = mixed_chunk(i * 10_000_000, vec![0.01; 480]);
            sample_count += analyzer.push_chunk(&chunk).len();
        }
        // After 75 chunks (750ms), one window emits. At 80 chunks, one sample.
        assert_eq!(sample_count, 1);
        // Flush the remainder.
        assert!(analyzer.flush().is_some());
    }

    #[test]
    fn audio_rms_short_transient_does_not_form_long_candidate() {
        let mut analyzer =
            AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        // 100ms of silence (4800 samples) — below 750ms window.
        let chunk = mixed_chunk(0, vec![0.0; 4_800]);
        assert!(analyzer.push_chunk(&chunk).is_empty());
        // Flush should produce the short sample.
        let flushed = analyzer.flush().unwrap();
        assert!(flushed.rms < 0.001);
    }

    #[test]
    fn audio_rms_emits_exact_configured_window_and_keeps_remainder() {
        let mut analyzer =
            AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        // 1s chunk @ 48kHz mono = 48_000 samples. Window = 750ms = 36_000 samples.
        // Should emit one 750ms sample and keep 250ms (12_000 samples) as remainder.
        let samples = analyzer.push_chunk(&mixed_chunk(0, vec![0.1; 48_000]));
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].end.nanos - samples[0].start.nanos, 750_000_000);
        // Flush the 250ms remainder.
        let remainder = analyzer.flush().unwrap();
        assert_eq!(remainder.end.nanos - remainder.start.nanos, 250_000_000);
    }

    #[test]
    fn audio_rms_large_chunk_emits_multiple_windows() {
        let mut analyzer =
            AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        // 1.6s chunk @ 48kHz mono = 76_800 samples. Window = 750ms = 36_000 samples.
        // Should emit two full windows (750ms each) and keep 100ms as remainder.
        let samples = analyzer.push_chunk(&mixed_chunk(0, vec![0.1; 76_800]));
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].end.nanos - samples[0].start.nanos, 750_000_000);
        assert_eq!(samples[1].end.nanos - samples[1].start.nanos, 750_000_000);
        // Flush the 100ms remainder.
        let remainder = analyzer.flush().unwrap();
        assert_eq!(remainder.end.nanos - remainder.start.nanos, 100_000_000);
    }

    #[test]
    fn audio_rms_resets_on_timestamp_gap() {
        let mut analyzer =
            AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        // First chunk: 0–250ms (12_000 samples @ 48kHz mono). Below 750ms window.
        let samples = analyzer.push_chunk(&mixed_chunk(0, vec![0.1; 12_000]));
        assert!(samples.is_empty());

        // Second chunk jumps to 2s — gap exceeds 750ms window.
        // Should flush the 250ms remainder and reset to 2s.
        let samples = analyzer.push_chunk(&mixed_chunk(2_000_000_000, vec![0.05; 48_000]));
        // First element is the flushed 250ms remainder from before the gap.
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].start.nanos, 0);
        assert_eq!(samples[0].end.nanos, 250_000_000);
        // Second element is a full 750ms window starting at 2s.
        assert_eq!(samples[1].start.nanos, 2_000_000_000);
        assert_eq!(samples[1].end.nanos, 2_750_000_000);
    }

    #[test]
    fn audio_rms_preserves_timestamps_after_dropped_chunk() {
        let mut analyzer =
            AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        // Simulate continuous chunks at 0ms, 10ms, 20ms.
        analyzer.push_chunk(&mixed_chunk(0, vec![0.1; 480]));
        analyzer.push_chunk(&mixed_chunk(10_000_000, vec![0.1; 480]));
        analyzer.push_chunk(&mixed_chunk(20_000_000, vec![0.1; 480]));

        // Chunk at 2s simulates a large gap (many dropped chunks).
        let samples = analyzer.push_chunk(&mixed_chunk(2_000_000_000, vec![0.05; 480]));
        // Should flush the 30ms remainder from 0–30ms.
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].start.nanos, 0);
        assert_eq!(samples[0].end.nanos, 30_000_000);
        // Analyzer is now reset to 2s — next flush should start from there.
        let remainder = analyzer.flush().unwrap();
        assert_eq!(remainder.start.nanos, 2_000_000_000);
    }

    // ── Frame-diff tests ──

    use crate::core::frame::{FrameBuffer, PixelFormat, VideoFrame};

    fn bgra_frame(timestamp: u64, width: u32, height: u32, pixel: [u8; 4]) -> VideoFrame {
        let mut bytes = Vec::new();
        for _ in 0..(width * height) {
            bytes.extend_from_slice(&pixel);
        }
        VideoFrame {
            timestamp: MediaTimestamp::from_nanos(timestamp),
            width,
            height,
            stride_bytes: width as usize * 4,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(Arc::from(bytes.into_boxed_slice())),
        }
    }

    #[test]
    fn frame_diff_static_frames_are_near_zero() {
        let analyzer = FrameDiffAnalyzer::new(4, 4);
        let first = bgra_frame(0, 4, 4, [0, 0, 0, 255]);
        let second = bgra_frame(33_333_333, 4, 4, [0, 0, 0, 255]);

        let diff = analyzer.diff_pair(&first, &second).unwrap();

        assert!(diff.change_ratio < 0.001);
    }

    #[test]
    fn frame_diff_detects_loading_animation_like_change() {
        let analyzer = FrameDiffAnalyzer::new(4, 4);
        let first = bgra_frame(0, 4, 4, [0, 0, 0, 255]);
        let second = bgra_frame(33_333_333, 4, 4, [255, 255, 255, 255]);

        let diff = analyzer.diff_pair(&first, &second).unwrap();

        assert!(diff.change_ratio > 0.9);
    }

    #[test]
    fn frame_diff_detects_small_cursor_like_motion() {
        let analyzer = FrameDiffAnalyzer::new(4, 4);
        let first = bgra_frame(0, 4, 4, [0, 0, 0, 255]);
        let mut second = bgra_frame(33_333_333, 4, 4, [0, 0, 0, 255]);
        {
            let FrameBuffer::Owned(buffer) = &mut second.buffer;
            let mut bytes = buffer.to_vec();
            bytes[0] = 255;
            bytes[1] = 255;
            bytes[2] = 255;
            second.buffer = FrameBuffer::Owned(Arc::from(bytes.into_boxed_slice()));
        }

        let diff = analyzer.diff_pair(&first, &second).unwrap();

        assert!(diff.change_ratio > 0.05);
    }

    #[test]
    fn frame_diff_handles_padded_rows() {
        let analyzer = FrameDiffAnalyzer::new(4, 4);
        // 4x4 BGRA with stride = 5*4 = 20 bytes (1 pixel padding per row).
        let pixel = [100u8, 150, 200, 255];
        let mut bytes = Vec::new();
        for _ in 0..4 {
            for _ in 0..4 {
                bytes.extend_from_slice(&pixel);
            }
            bytes.extend_from_slice(&[0u8; 4]); // padding pixel
        }
        let first = VideoFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            width: 4,
            height: 4,
            stride_bytes: 20,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(Arc::from(bytes.into_boxed_slice())),
        };
        let second = first.clone();
        // Set second timestamp to satisfy diff_pair requirements.
        let mut second = VideoFrame {
            timestamp: MediaTimestamp::from_nanos(33_333_333),
            ..second
        };
        // Identical content → diff should be near zero despite padding.
        let diff = analyzer.diff_pair(&first, &second).unwrap();
        assert!(diff.change_ratio < 0.001);

        // Now modify the second frame's first pixel to verify stride-aware reading.
        // FrameBuffer currently has only the Owned variant, so direct binding is safe.
        let FrameBuffer::Owned(buf) = &second.buffer;
        let mut bytes = buf.to_vec();
        bytes[0] = 255;
        bytes[1] = 255;
        bytes[2] = 255;
        second.buffer = FrameBuffer::Owned(Arc::from(bytes.into_boxed_slice()));
        let diff = analyzer.diff_pair(&first, &second).unwrap();
        assert!(diff.change_ratio > 0.01);
    }

    #[test]
    fn frame_diff_rejects_stride_smaller_than_width_bytes() {
        let analyzer = FrameDiffAnalyzer::new(4, 4);
        // width=10 → min stride = 40, but we set stride=4.
        let frame = VideoFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            width: 10,
            height: 4,
            stride_bytes: 4,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(Arc::from(vec![0u8; 16].into_boxed_slice())),
        };
        let err = analyzer.downsample_grayscale(&frame).unwrap_err();
        assert!(err.contains("小于行像素字节数"));
    }

    #[test]
    fn frame_diff_rejects_stride_height_overflow() {
        let analyzer = FrameDiffAnalyzer::new(4, 4);
        // stride=usize::MAX/2, height=4 → stride*height overflows.
        let frame = VideoFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            width: 1,
            height: 4,
            stride_bytes: usize::MAX / 2,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(Arc::from(vec![0u8; 4].into_boxed_slice())),
        };
        let err = analyzer.downsample_grayscale(&frame).unwrap_err();
        assert!(err.contains("整数溢出"));
    }

    // ── Detector tests ──

    fn audio_sample(start: u64, end: u64, rms: f32) -> AudioActivitySample {
        AudioActivitySample {
            start: MediaTimestamp::from_nanos(start),
            end: MediaTimestamp::from_nanos(end),
            rms,
        }
    }

    fn visual_sample(start: u64, end: u64, change_ratio: f32) -> FrameDiffSample {
        FrameDiffSample {
            start: MediaTimestamp::from_nanos(start),
            end: MediaTimestamp::from_nanos(end),
            change_ratio,
        }
    }

    #[test]
    fn detector_requires_audio_silence_and_low_visual_change() {
        let config = TrimConfig::from_sensitivity(TrimSensitivity::High);
        let detector = SilenceDetectorEngine::new(config);
        let audio = vec![audio_sample(0, 7_000_000_000, 0.001)];
        let visual_active = vec![visual_sample(0, 7_000_000_000, 0.2)];

        let timeline = detector
            .analyze(&audio, &visual_active, 10_000_000_000)
            .unwrap();

        assert!(timeline.cuts.is_empty());
        assert_eq!(timeline.total_cut_nanos, 0);
    }

    #[test]
    fn detector_does_not_cut_short_pause() {
        let config = TrimConfig::from_sensitivity(TrimSensitivity::High);
        let detector = SilenceDetectorEngine::new(config);
        let audio = vec![audio_sample(1_000_000_000, 2_500_000_000, 0.001)];
        let visual = vec![visual_sample(1_000_000_000, 2_500_000_000, 0.001)];

        let timeline = detector.analyze(&audio, &visual, 5_000_000_000).unwrap();

        assert!(timeline.cuts.is_empty());
    }

    #[test]
    fn detector_cuts_long_silent_and_still_segment_with_buffer() {
        let config = TrimConfig::from_sensitivity(TrimSensitivity::Medium);
        let detector = SilenceDetectorEngine::new(config);
        let audio = vec![audio_sample(2_000_000_000, 9_000_000_000, 0.001)];
        let visual = vec![visual_sample(2_000_000_000, 9_000_000_000, 0.001)];

        let timeline = detector.analyze(&audio, &visual, 12_000_000_000).unwrap();

        assert_eq!(timeline.cuts.len(), 1);
        assert_eq!(timeline.cuts[0].start.nanos, 2_400_000_000);
        assert_eq!(timeline.cuts[0].end.nanos, 8_600_000_000);
        assert_eq!(timeline.total_cut_nanos, 6_200_000_000);
    }

    #[test]
    fn detector_merges_adjacent_candidates() {
        let config = TrimConfig::from_sensitivity(TrimSensitivity::High);
        let detector = SilenceDetectorEngine::new(config);
        let audio = vec![
            audio_sample(0, 5_500_000_000, 0.001),
            audio_sample(5_800_000_000, 11_500_000_000, 0.001),
        ];
        let visual = vec![
            visual_sample(0, 5_500_000_000, 0.001),
            visual_sample(5_800_000_000, 11_500_000_000, 0.001),
        ];

        let timeline = detector.analyze(&audio, &visual, 14_000_000_000).unwrap();

        assert_eq!(timeline.cuts.len(), 1);
        assert!(timeline.cuts[0].start.nanos < 500_000_000);
        assert!(timeline.cuts[0].end.nanos > 11_000_000_000);
    }

    #[test]
    fn detector_empty_inputs_return_noop_timeline() {
        let config = TrimConfig::from_sensitivity(TrimSensitivity::Medium);
        let detector = SilenceDetectorEngine::new(config);

        let timeline = detector.analyze(&[], &[], 3_000_000_000).unwrap();

        assert!(timeline.cuts.is_empty());
        assert_eq!(timeline.keeps.len(), 1);
    }

    #[test]
    fn detector_generates_cuts_from_observed_activity_duration() {
        // Simulates the Critical 1 fix: writer returns duration_secs=0, so
        // duration_nanos falls back to the max activity end timestamp.
        let config = TrimConfig::from_sensitivity(TrimSensitivity::Medium);
        let detector = SilenceDetectorEngine::new(config);
        let audio = vec![audio_sample(2_000_000_000, 9_000_000_000, 0.001)];
        let visual = vec![visual_sample(2_000_000_000, 9_000_000_000, 0.001)];
        let observed_duration = 12_000_000_000u64; // max(audio.end, visual.end) + margin

        let timeline = detector
            .analyze(&audio, &visual, observed_duration)
            .unwrap();

        assert_eq!(timeline.cuts.len(), 1);
        assert!(timeline.total_cut_nanos > 0);
    }
}
