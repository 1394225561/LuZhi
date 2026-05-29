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

/// Samples smoothed cursor samples at video-frame timestamps using cubic Bezier segments.
pub struct BezierInterpolator {
    fps: u32,
}

impl BezierInterpolator {
    pub fn new(fps: u32) -> Self {
        Self { fps: fps.max(1) }
    }

    pub fn sample_frames(&self, samples: &[CursorSample], duration_nanos: u64) -> Vec<CursorFrame> {
        if samples.is_empty() {
            return Vec::new();
        }

        let frame_interval = 1_000_000_000u64 / self.fps as u64;
        let mut frames = Vec::new();
        let mut timestamp = 0u64;

        while timestamp <= duration_nanos {
            let position = if samples.len() == 1 {
                samples[0]
            } else {
                self.sample_at(samples, timestamp)
            };

            frames.push(CursorFrame {
                timestamp: MediaTimestamp::from_nanos(timestamp),
                x: position.x,
                y: position.y,
                scale: 1.0,
                opacity: 1.0,
            });

            timestamp = timestamp.saturating_add(frame_interval);
            if frame_interval == 0 {
                break;
            }
        }

        frames
    }

    fn sample_at(&self, samples: &[CursorSample], timestamp: u64) -> CursorSample {
        let segment_index = samples
            .windows(2)
            .position(|pair| {
                timestamp >= pair[0].timestamp.nanos && timestamp <= pair[1].timestamp.nanos
            })
            .unwrap_or_else(|| {
                if timestamp < samples[0].timestamp.nanos {
                    0
                } else {
                    samples.len().saturating_sub(2)
                }
            });

        let p0 = samples[segment_index.saturating_sub(1)];
        let p1 = samples[segment_index];
        let p2 = samples[(segment_index + 1).min(samples.len() - 1)];
        let p3 = samples[(segment_index + 2).min(samples.len() - 1)];

        let span = p2.timestamp.nanos.saturating_sub(p1.timestamp.nanos).max(1);
        let t =
            ((timestamp.saturating_sub(p1.timestamp.nanos)) as f32 / span as f32).clamp(0.0, 1.0);

        let distance = distance_between(p1, p2);
        let strength = (distance / 240.0).clamp(0.15, 0.65);

        let c1 = (
            p1.x + (p2.x - p0.x) * strength / 3.0,
            p1.y + (p2.y - p0.y) * strength / 3.0,
        );
        let c2 = (
            p2.x - (p3.x - p1.x) * strength / 3.0,
            p2.y - (p3.y - p1.y) * strength / 3.0,
        );

        let (x, y) = cubic_bezier((p1.x, p1.y), c1, c2, (p2.x, p2.y), t);

        CursorSample {
            timestamp: MediaTimestamp::from_nanos(timestamp),
            x,
            y,
        }
    }
}

fn distance_between(a: CursorSample, b: CursorSample) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

fn cubic_bezier(
    p0: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    p3: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let inv = 1.0 - t;
    let b0 = inv * inv * inv;
    let b1 = 3.0 * inv * inv * t;
    let b2 = 3.0 * inv * t * t;
    let b3 = t * t * t;

    (
        b0 * p0.0 + b1 * c1.0 + b2 * c2.0 + b3 * p3.0,
        b0 * p0.1 + b1 * c1.1 + b2 * c2.1 + b3 * p3.1,
    )
}

const EXPAND_NANOS: u64 = 120_000_000;
const HOLD_NANOS: u64 = 80_000_000;
const SHRINK_NANOS: u64 = 300_000_000;

/// Click magnification animation durations and visual strength.
#[derive(Clone, Copy, Debug)]
pub struct ClickAnimationConfig {
    pub max_scale: f32,
    pub peak_opacity: f32,
}

impl Default for ClickAnimationConfig {
    fn default() -> Self {
        Self {
            max_scale: 2.0,
            peak_opacity: 0.35,
        }
    }
}

/// Runtime click animation state used for deterministic tests and effect construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClickAnimationState {
    Idle,
    PressedExpand,
    Hold,
    ReleaseShrink,
}

/// Small click state machine matching the Phase 4 required transition path.
pub struct ClickAnimationMachine {
    state: ClickAnimationState,
    config: ClickAnimationConfig,
    pressed_at: Option<MediaTimestamp>,
    release_started_at: Option<MediaTimestamp>,
}

impl ClickAnimationMachine {
    pub fn new(config: ClickAnimationConfig) -> Self {
        Self {
            state: ClickAnimationState::Idle,
            config,
            pressed_at: None,
            release_started_at: None,
        }
    }

    pub fn state(&self) -> ClickAnimationState {
        self.state
    }

    pub fn apply_click(&mut self, click: CursorClick) {
        match click.phase {
            ClickPhase::Down => {
                self.state = ClickAnimationState::PressedExpand;
                self.pressed_at = Some(click.timestamp);
                self.release_started_at = None;
            }
            ClickPhase::Up => {
                if self.state != ClickAnimationState::Idle {
                    self.state = ClickAnimationState::ReleaseShrink;
                    self.release_started_at = Some(click.timestamp);
                }
            }
        }
    }

    pub fn advance_to(&mut self, timestamp: MediaTimestamp) {
        match self.state {
            ClickAnimationState::PressedExpand => {
                if let Some(start) = self.pressed_at {
                    if timestamp.nanos.saturating_sub(start.nanos) >= EXPAND_NANOS {
                        self.state = ClickAnimationState::Hold;
                    }
                }
            }
            ClickAnimationState::ReleaseShrink => {
                if let Some(start) = self.release_started_at {
                    if timestamp.nanos.saturating_sub(start.nanos) >= SHRINK_NANOS {
                        self.state = ClickAnimationState::Idle;
                        self.pressed_at = None;
                        self.release_started_at = None;
                    }
                }
            }
            ClickAnimationState::Idle | ClickAnimationState::Hold => {}
        }
    }

    pub fn config(&self) -> ClickAnimationConfig {
        self.config
    }
}

/// Converts recorded click transitions into magnification effect windows.
pub struct ClickEffectBuilder {
    config: ClickAnimationConfig,
}

impl ClickEffectBuilder {
    pub fn new(config: ClickAnimationConfig) -> Self {
        Self { config }
    }

    pub fn build(&self, clicks: &[CursorClick]) -> Vec<CursorClickEffect> {
        let mut effects = Vec::new();
        let mut pending_down: Option<CursorClick> = None;

        for click in clicks
            .iter()
            .copied()
            .filter(|click| click.button == MouseButton::Left)
        {
            match click.phase {
                ClickPhase::Down => {
                    pending_down = Some(click);
                }
                ClickPhase::Up => {
                    if let Some(down) = pending_down.take() {
                        effects.push(CursorClickEffect {
                            start: down.timestamp,
                            end: MediaTimestamp::from_nanos(
                                click
                                    .timestamp
                                    .nanos
                                    .saturating_add(EXPAND_NANOS + HOLD_NANOS + SHRINK_NANOS),
                            ),
                            x: down.x,
                            y: down.y,
                            max_scale: self.config.max_scale,
                            peak_opacity: self.config.peak_opacity,
                        });
                    }
                }
            }
        }

        if let Some(down) = pending_down {
            effects.push(CursorClickEffect {
                start: down.timestamp,
                end: MediaTimestamp::from_nanos(
                    down.timestamp
                        .nanos
                        .saturating_add(EXPAND_NANOS + HOLD_NANOS + SHRINK_NANOS),
                ),
                x: down.x,
                y: down.y,
                max_scale: self.config.max_scale,
                peak_opacity: self.config.peak_opacity,
            });
        }

        effects
    }
}

/// Pure post-recording cursor effect engine.
pub struct CursorEffectEngine {
    click_config: ClickAnimationConfig,
    smoothing_enabled: bool,
}

impl CursorEffectEngine {
    pub fn new(click_config: ClickAnimationConfig) -> Self {
        Self {
            click_config,
            smoothing_enabled: true,
        }
    }

    pub fn with_smoothing(click_config: ClickAnimationConfig, smoothing_enabled: bool) -> Self {
        Self {
            click_config,
            smoothing_enabled,
        }
    }
}

impl Default for CursorEffectEngine {
    fn default() -> Self {
        Self::new(ClickAnimationConfig::default())
    }
}

impl CursorProcessor for CursorEffectEngine {
    fn build_timeline(
        &self,
        samples: &[CursorSample],
        clicks: &[CursorClick],
        fps: u32,
        duration_nanos: u64,
    ) -> AppResult<EffectTimeline> {
        if fps == 0 {
            return Err(AppError::CursorProcessingFailed {
                reason: "帧率必须大于 0".to_string(),
            });
        }

        if samples.is_empty() {
            return Ok(EffectTimeline {
                fps,
                duration_nanos,
                frames: Vec::new(),
                click_effects: Vec::new(),
                raw_system_cursor_visible: false,
                render_cursor_overlay: true,
            });
        }

        let smoothed = if self.smoothing_enabled {
            let smoother = CursorSmoother::new(SmoothingConfig::for_fps(fps));
            smoother.smooth(samples)
        } else {
            samples.to_vec()
        };
        let interpolator = BezierInterpolator::new(fps);
        let mut frames = interpolator.sample_frames(&smoothed, duration_nanos);
        let click_effects = ClickEffectBuilder::new(self.click_config).build(clicks);

        apply_click_scale_to_frames(&mut frames, &click_effects);

        Ok(EffectTimeline {
            fps,
            duration_nanos,
            frames,
            click_effects,
            raw_system_cursor_visible: false,
            render_cursor_overlay: true,
        })
    }
}

fn apply_click_scale_to_frames(frames: &mut [CursorFrame], effects: &[CursorClickEffect]) {
    for frame in frames {
        for effect in effects {
            if frame.timestamp.nanos < effect.start.nanos
                || frame.timestamp.nanos > effect.end.nanos
            {
                continue;
            }

            let span = effect.end.nanos.saturating_sub(effect.start.nanos).max(1);
            let t = (frame.timestamp.nanos.saturating_sub(effect.start.nanos) as f32 / span as f32)
                .clamp(0.0, 1.0);
            let wave = if t <= 0.35 {
                t / 0.35
            } else {
                1.0 - ((t - 0.35) / 0.65)
            }
            .clamp(0.0, 1.0);

            frame.scale = frame.scale.max(1.0 + (effect.max_scale - 1.0) * wave);
            frame.opacity = frame.opacity.max(effect.peak_opacity * wave);
        }
    }
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

        assert!(
            output[2].x > 760.0,
            "jump was over-smoothed: {}",
            output[2].x
        );
        assert!(
            output[2].y > 590.0,
            "jump was over-smoothed: {}",
            output[2].y
        );
    }

    #[test]
    fn adaptive_window_differs_between_30fps_and_60fps() {
        assert_eq!(SmoothingConfig::for_fps(30).window_size, 5);
        assert_eq!(SmoothingConfig::for_fps(60).window_size, 9);
    }

    #[test]
    fn interpolation_empty_input_returns_empty() {
        let interpolator = BezierInterpolator::new(30);

        assert!(interpolator.sample_frames(&[], 1_000_000_000).is_empty());
    }

    #[test]
    fn interpolation_single_point_covers_each_frame_timestamp() {
        let interpolator = BezierInterpolator::new(30);
        let input = vec![sample(0, 50.0, 80.0)];

        let frames = interpolator.sample_frames(&input, 100_000_000);

        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0].timestamp.nanos, 0);
        assert_eq!(frames[1].timestamp.nanos, 33_333_333);
        assert_eq!(frames[2].timestamp.nanos, 66_666_666);
        assert_eq!(frames[3].timestamp.nanos, 99_999_999);
        assert!(frames
            .iter()
            .all(|frame| frame.x == 50.0 && frame.y == 80.0));
    }

    #[test]
    fn interpolation_reaches_last_sample_position() {
        let interpolator = BezierInterpolator::new(60);
        let input = vec![
            sample(0, 0.0, 0.0),
            sample(50_000_000, 30.0, 30.0),
            sample(100_000_000, 120.0, 80.0),
        ];

        let frames = interpolator.sample_frames(&input, 100_000_000);
        let last = frames.last().unwrap();

        assert!(
            last.x > 100.0,
            "last x too far from final sample: {}",
            last.x
        );
        assert!(
            last.y > 65.0,
            "last y too far from final sample: {}",
            last.y
        );
    }

    #[test]
    fn interpolation_uses_60fps_frame_interval() {
        let interpolator = BezierInterpolator::new(60);
        let input = vec![sample(0, 0.0, 0.0), sample(50_000_000, 60.0, 0.0)];

        let frames = interpolator.sample_frames(&input, 50_000_000);

        assert_eq!(frames.len(), 4);
        assert_eq!(frames[1].timestamp.nanos, 16_666_666);
    }

    fn click(nanos: u64, phase: ClickPhase, x: f32, y: f32) -> CursorClick {
        CursorClick {
            timestamp: MediaTimestamp::from_nanos(nanos),
            button: MouseButton::Left,
            phase,
            x,
            y,
        }
    }

    #[test]
    fn click_state_machine_transitions_through_expected_states() {
        let mut machine = ClickAnimationMachine::new(ClickAnimationConfig::default());

        assert_eq!(machine.state(), ClickAnimationState::Idle);

        machine.apply_click(click(0, ClickPhase::Down, 10.0, 20.0));
        assert_eq!(machine.state(), ClickAnimationState::PressedExpand);

        machine.advance_to(MediaTimestamp::from_nanos(140_000_000));
        assert_eq!(machine.state(), ClickAnimationState::Hold);

        machine.apply_click(click(160_000_000, ClickPhase::Up, 10.0, 20.0));
        assert_eq!(machine.state(), ClickAnimationState::ReleaseShrink);

        machine.advance_to(MediaTimestamp::from_nanos(500_000_000));
        assert_eq!(machine.state(), ClickAnimationState::Idle);
    }

    #[test]
    fn click_effect_has_expand_and_shrink_window() {
        let builder = ClickEffectBuilder::new(ClickAnimationConfig::default());
        let effects = builder.build(&[
            click(0, ClickPhase::Down, 10.0, 20.0),
            click(150_000_000, ClickPhase::Up, 11.0, 21.0),
        ]);

        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].start.nanos, 0);
        assert_eq!(effects[0].end.nanos, 650_000_000);
        assert_eq!(effects[0].x, 10.0);
        assert_eq!(effects[0].max_scale, 2.0);
    }

    #[test]
    fn consecutive_clicks_do_not_leave_machine_stuck() {
        let builder = ClickEffectBuilder::new(ClickAnimationConfig::default());
        let effects = builder.build(&[
            click(0, ClickPhase::Down, 10.0, 20.0),
            click(40_000_000, ClickPhase::Up, 10.0, 20.0),
            click(90_000_000, ClickPhase::Down, 30.0, 40.0),
            click(130_000_000, ClickPhase::Up, 30.0, 40.0),
        ]);

        assert_eq!(effects.len(), 2);
        assert!(effects[0].end.nanos <= effects[1].start.nanos + 450_000_000);
    }

    #[test]
    fn engine_builds_empty_timeline_for_empty_samples() {
        let engine = CursorEffectEngine::default();

        let timeline = engine.build_timeline(&[], &[], 30, 1_000_000_000).unwrap();

        assert_eq!(timeline.fps, 30);
        assert_eq!(timeline.duration_nanos, 1_000_000_000);
        assert!(timeline.frames.is_empty());
        assert!(timeline.click_effects.is_empty());
    }

    #[test]
    fn engine_builds_frames_and_click_effects() {
        let engine = CursorEffectEngine::default();
        let samples = vec![
            sample(0, 10.0, 10.0),
            sample(33_333_333, 30.0, 20.0),
            sample(66_666_666, 80.0, 50.0),
        ];
        let clicks = vec![
            click(33_333_333, ClickPhase::Down, 30.0, 20.0),
            click(80_000_000, ClickPhase::Up, 30.0, 20.0),
        ];

        let timeline = engine
            .build_timeline(&samples, &clicks, 30, 100_000_000)
            .unwrap();

        assert_eq!(timeline.fps, 30);
        assert_eq!(timeline.frames.len(), 4);
        assert_eq!(timeline.click_effects.len(), 1);
    }

    #[test]
    fn engine_rejects_zero_fps() {
        let engine = CursorEffectEngine::default();
        let error = engine
            .build_timeline(&[sample(0, 1.0, 1.0)], &[], 0, 100_000_000)
            .unwrap_err();

        assert!(error.to_string().contains("帧率"));
    }

    #[test]
    fn engine_features_off_frames_have_neutral_scale_and_opacity() {
        // When both magnification (max_scale=1.0, peak_opacity=0.0) and
        // smoothing are disabled, the engine still generates cursor frames and
        // click effects, but click effects produce zero visible impact on frame
        // scale/opacity. The command layer in lib.rs decides whether to invoke
        // the engine based on raw cursor visibility: when raw cursor is hidden
        // and features are off, the engine is still called to produce baseline
        // neutral frames for the overlay timeline.
        let engine = CursorEffectEngine::with_smoothing(
            ClickAnimationConfig {
                max_scale: 1.0,
                peak_opacity: 0.0,
            },
            false,
        );
        let timeline = engine
            .build_timeline(
                &[sample(0, 100.0, 100.0), sample(16_666_667, 200.0, 200.0)],
                &[click(8_000_000, ClickPhase::Down, 100.0, 100.0)],
                60,
                33_333_333,
            )
            .unwrap();

        // All frames have neutral scale and opacity — no visible click effect.
        for frame in &timeline.frames {
            assert_eq!(frame.scale, 1.0);
            assert_eq!(frame.opacity, 1.0);
        }
    }
}
