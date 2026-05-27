use crate::app::error::{AppError, AppResult};
use crate::core::frame::MediaTimestamp;
use crate::core::processor::CursorProcessor;
use crate::core::timeline::{
    ClickPhase, CursorClick, CursorClickEffect, CursorFrame, CursorSample, EffectTimeline,
    MouseButton,
};

const DEFAULT_JUMP_THRESHOLD_PIXELS: f32 = 240.0;

/// Smoothing settings derived from the capture frame rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmoothingConfig {
    pub window_size: usize,
    pub jump_threshold_pixels: u32,
}

impl SmoothingConfig {
    pub fn for_fps(fps: u32) -> Self {
        let window_size = if fps <= 30 {
            5
        } else if fps >= 60 {
            9
        } else {
            7
        };

        Self {
            window_size,
            jump_threshold_pixels: DEFAULT_JUMP_THRESHOLD_PIXELS as u32,
        }
    }
}

/// Adaptive moving-average smoother for cursor samples.
pub struct CursorSmoother {
    config: SmoothingConfig,
}

impl CursorSmoother {
    pub fn new(config: SmoothingConfig) -> Self {
        Self { config }
    }

    pub fn smooth(&self, samples: &[CursorSample]) -> Vec<CursorSample> {
        if samples.len() <= 1 {
            return samples.to_vec();
        }

        let radius = self.config.window_size / 2;
        let jump_threshold = self.config.jump_threshold_pixels as f32;

        samples
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                if is_large_jump(samples, index, jump_threshold) {
                    return *sample;
                }

                let start = index.saturating_sub(radius);
                let end = (index + radius + 1).min(samples.len());
                let window = &samples[start..end];

                let (sum_x, sum_y) = window.iter().fold((0.0f32, 0.0f32), |acc, item| {
                    (acc.0 + item.x, acc.1 + item.y)
                });
                let count = window.len() as f32;

                CursorSample {
                    timestamp: sample.timestamp,
                    x: sum_x / count,
                    y: sum_y / count,
                }
            })
            .collect()
    }
}

fn is_large_jump(samples: &[CursorSample], index: usize, threshold: f32) -> bool {
    if index == 0 {
        return false;
    }

    let previous = samples[index - 1];
    let current = samples[index];
    let dx = current.x - previous.x;
    let dy = current.y - previous.y;
    (dx * dx + dy * dy).sqrt() >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::CursorSample;

    fn sample(nanos: u64, x: f32, y: f32) -> CursorSample {
        CursorSample {
            timestamp: MediaTimestamp::from_nanos(nanos),
            x,
            y,
        }
    }

    fn total_variation(samples: &[CursorSample]) -> f32 {
        samples
            .windows(2)
            .map(|pair| {
                let dx = pair[1].x - pair[0].x;
                let dy = pair[1].y - pair[0].y;
                (dx * dx + dy * dy).sqrt()
            })
            .sum()
    }

    #[test]
    fn smoothing_empty_input_returns_empty() {
        let smoother = CursorSmoother::new(SmoothingConfig::for_fps(30));

        assert!(smoother.smooth(&[]).is_empty());
    }

    #[test]
    fn smoothing_single_point_keeps_position() {
        let smoother = CursorSmoother::new(SmoothingConfig::for_fps(30));
        let input = vec![sample(0, 10.0, 20.0)];

        let output = smoother.smooth(&input);

        assert_eq!(output, input);
    }

    #[test]
    fn high_frequency_jitter_is_reduced() {
        let smoother = CursorSmoother::new(SmoothingConfig::for_fps(60));
        let input = vec![
            sample(0, 100.0, 100.0),
            sample(16_666_667, 103.0, 99.0),
            sample(33_333_334, 97.0, 101.0),
            sample(50_000_001, 102.0, 98.0),
            sample(66_666_668, 98.0, 102.0),
            sample(83_333_335, 101.0, 100.0),
        ];

        let output = smoother.smooth(&input);

        assert_eq!(output.len(), input.len());
        assert!(total_variation(&output) < total_variation(&input));
    }

    #[test]
    fn fast_cross_region_jump_is_preserved() {
        let smoother = CursorSmoother::new(SmoothingConfig::for_fps(60));
        let input = vec![
            sample(0, 10.0, 10.0),
            sample(16_666_667, 12.0, 11.0),
            sample(33_333_334, 900.0, 700.0),
            sample(50_000_001, 902.0, 701.0),
        ];

        let output = smoother.smooth(&input);

        assert!(output[2].x > 760.0, "jump was over-smoothed: {}", output[2].x);
        assert!(output[2].y > 590.0, "jump was over-smoothed: {}", output[2].y);
    }

    #[test]
    fn adaptive_window_differs_between_30fps_and_60fps() {
        assert_eq!(SmoothingConfig::for_fps(30).window_size, 5);
        assert_eq!(SmoothingConfig::for_fps(60).window_size, 9);
    }
}
