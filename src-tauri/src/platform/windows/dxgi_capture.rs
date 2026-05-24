use crate::app::error::{AppError, AppResult};
use crate::core::capture::{CaptureCapabilities, CaptureConfig, ScreenCapture, VideoFrameSink};

/// Windows screen capture stub using DXGI Desktop Duplication.
///
/// This is a placeholder implementation for Phase 2. All methods return
/// `NativeCaptureUnavailable` until DXGI integration is implemented.
pub struct DxgiCapture;

impl DxgiCapture {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DxgiCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenCapture for DxgiCapture {
    fn start(&mut self, _config: CaptureConfig, _sink: VideoFrameSink) -> AppResult<()> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "DXGI Desktop Duplication 尚未实现",
        })
    }

    fn stop(&mut self) -> AppResult<()> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "DXGI Desktop Duplication 尚未实现",
        })
    }

    fn capabilities(&self) -> CaptureCapabilities {
        CaptureCapabilities {
            supports_full_screen: false,
            supports_window: false,
            supports_region: false,
            supports_4k: false,
        }
    }
}
