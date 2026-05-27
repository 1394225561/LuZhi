use crate::app::error::AppResult;
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
