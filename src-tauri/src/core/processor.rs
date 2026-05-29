use crate::app::error::AppResult;
use crate::core::cut::{AudioActivitySample, CutTimeline, FrameDiffSample};
use crate::core::timeline::{CursorClick, CursorSample, EffectTimeline};

/// Builds a post-recording cursor effect timeline from recorded cursor metadata.
pub trait CursorProcessor: Send + Sync {
    /// Converts raw cursor samples and click events into a per-frame effect timeline.
    fn build_timeline(
        &self,
        samples: &[CursorSample],
        clicks: &[CursorClick],
        fps: u32,
        duration_nanos: u64,
    ) -> AppResult<EffectTimeline>;
}

/// Builds a post-recording cut timeline from audio and visual activity metadata.
pub trait SilenceDetector: Send + Sync {
    /// Converts mixed-audio activity and low-resolution frame-diff samples into cut segments.
    fn analyze(
        &self,
        audio: &[AudioActivitySample],
        visual: &[FrameDiffSample],
        duration_nanos: u64,
    ) -> AppResult<CutTimeline>;
}
