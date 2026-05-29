use crate::core::cut::{
    AudioActivitySample, CutReason, CutSegment, CutTimeline, FrameDiffSample, KeepSegment,
    TrimConfig,
};
use crate::core::frame::{FrameBuffer, MediaTimestamp, MixedAudioChunk, PixelFormat, VideoFrame};
use crate::core::processor::SilenceDetector;

/// Computes mixed-audio RMS activity samples using a conservative window.
pub struct AudioRmsAnalyzer {
    config: TrimConfig,
}

impl AudioRmsAnalyzer {
    pub fn new(config: TrimConfig) -> Self {
        Self { config }
    }

    pub fn window_nanos(&self) -> u64 {
        self.config.rms_window_nanos
    }

    pub fn analyze_chunks(&self, chunks: &[MixedAudioChunk]) -> Vec<AudioActivitySample> {
        chunks
            .iter()
            .filter_map(|chunk| {
                if chunk.sample_rate == 0 || chunk.channels == 0 || chunk.samples.is_empty() {
                    return None;
                }

                let sum_squares = chunk
                    .samples
                    .iter()
                    .map(|sample| sample * sample)
                    .sum::<f32>();
                let rms = (sum_squares / chunk.samples.len() as f32).sqrt();
                let frames = chunk.samples.len() as u64 / chunk.channels as u64;
                let duration_nanos =
                    frames.saturating_mul(1_000_000_000) / chunk.sample_rate as u64;

                Some(AudioActivitySample {
                    start: chunk.timestamp,
                    end: MediaTimestamp::from_nanos(
                        chunk.timestamp.nanos.saturating_add(duration_nanos),
                    ),
                    rms,
                })
            })
            .collect()
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
        let expected = frame.width as usize * frame.height as usize * 4;
        if bytes.len() < expected {
            return Err("帧像素数据长度不足".to_string());
        }

        let mut output = Vec::with_capacity((self.thumb_width * self.thumb_height) as usize);
        for y in 0..self.thumb_height {
            for x in 0..self.thumb_width {
                let src_x = (x as u64 * frame.width as u64 / self.thumb_width as u64) as usize;
                let src_y = (y as u64 * frame.height as u64 / self.thumb_height as u64) as usize;
                let offset = (src_y * frame.width as usize + src_x) * 4;
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

        let mut candidates: Vec<(u64, u64, f32, f32)> = Vec::new();
        for audio_sample in audio {
            for visual_sample in visual {
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
        let analyzer = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        let samples = analyzer.analyze_chunks(&[mixed_chunk(0, vec![0.0; 48_000])]);

        assert_eq!(samples.len(), 1);
        assert!(samples[0].rms < 0.001);
    }

    #[test]
    fn background_noise_remains_explainable() {
        let analyzer = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        let samples = analyzer.analyze_chunks(&[mixed_chunk(0, vec![0.015; 48_000])]);

        assert_eq!(samples.len(), 1);
        assert!(samples[0].rms > 0.014);
        assert!(samples[0].rms < 0.016);
    }

    #[test]
    fn loud_chunk_is_not_silent() {
        let analyzer = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(TrimSensitivity::Medium));
        let samples = analyzer.analyze_chunks(&[mixed_chunk(0, vec![0.25; 48_000])]);

        assert_eq!(samples.len(), 1);
        assert!(samples[0].rms > 0.2);
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
        if let FrameBuffer::Owned(buffer) = &mut second.buffer {
            let mut bytes = buffer.to_vec();
            bytes[0] = 255;
            bytes[1] = 255;
            bytes[2] = 255;
            second.buffer = FrameBuffer::Owned(Arc::from(bytes.into_boxed_slice()));
        }

        let diff = analyzer.diff_pair(&first, &second).unwrap();

        assert!(diff.change_ratio > 0.05);
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
}
