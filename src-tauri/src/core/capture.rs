use std::sync::mpsc::Sender;

use crate::app::error::AppResult;
use crate::core::config::CaptureConfig;
use crate::core::frame::VideoFrameRef;

/// Channel sender used by native capture adapters to hand video frames to Rust services.
pub type VideoFrameSink = Sender<VideoFrameRef>;

/// Capture features supported by the current platform adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureCapabilities {
    pub supports_full_screen: bool,
    pub supports_window: bool,
    pub supports_region: bool,
    pub supports_4k: bool,
}

impl CaptureCapabilities {
    /// Phase 1 macOS scope only enables full-screen capture.
    pub fn phase_one_macos() -> Self {
        Self {
            supports_full_screen: true,
            supports_window: false,
            supports_region: false,
            supports_4k: false,
        }
    }
}

/// Platform capture boundary implemented by macOS and Windows adapters.
pub trait ScreenCapture: Send {
    /// Starts capture with the given configuration and sends frames through `sink`.
    ///
    /// Implementations must keep native frame ownership on the Rust side and use
    /// the sink only as a thread handoff boundary. If startup fails, no frames
    /// should be sent after the error is returned.
    fn start(&mut self, config: CaptureConfig, sink: VideoFrameSink) -> AppResult<()>;

    /// Stops capture and releases platform resources.
    ///
    /// Calling `stop` must make a best effort to prevent any later frame delivery
    /// through the sink created by `start`.
    fn stop(&mut self) -> AppResult<()>;

    /// Returns the capture modes and resolutions supported by this adapter.
    fn capabilities(&self) -> CaptureCapabilities;
}
