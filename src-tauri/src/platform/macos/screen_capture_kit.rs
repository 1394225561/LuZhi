use crate::app::error::{AppError, AppResult};
use crate::core::capture::{CaptureCapabilities, ScreenCapture, VideoFrameSink};
use crate::core::config::CaptureConfig;

/// macOS ScreenCaptureKit adapter boundary.
///
/// The `start` method is intentionally guarded: it returns
/// `NativeCaptureUnavailable` until a human reviewer has verified
/// buffer ownership, callback thread handoff, and stop-path resource
/// release for the real ScreenCaptureKit integration.
pub struct MacScreenCapture {
    running: bool,
}

impl MacScreenCapture {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for MacScreenCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenCapture for MacScreenCapture {
    fn start(&mut self, _config: CaptureConfig, _sink: VideoFrameSink) -> AppResult<()> {
        self.running = true;
        Err(AppError::NativeCaptureUnavailable {
            reason: "ScreenCaptureKit native callback must pass human review before activation",
        })
    }

    fn stop(&mut self) -> AppResult<()> {
        self.running = false;
        Ok(())
    }

    fn capabilities(&self) -> CaptureCapabilities {
        CaptureCapabilities::phase_one_macos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn mac_capture_exposes_phase_one_capabilities() {
        let capture = MacScreenCapture::new();
        let capabilities = capture.capabilities();

        assert!(capabilities.supports_full_screen);
        assert!(!capabilities.supports_window);
        assert!(!capabilities.supports_region);
    }

    #[test]
    fn mac_capture_requires_human_reviewed_native_activation() {
        let mut capture = MacScreenCapture::new();
        let (sender, _receiver) = channel();

        let error = capture
            .start(CaptureConfig::full_screen_1080p_30fps(), sender)
            .unwrap_err();

        assert_eq!(
            error,
            AppError::NativeCaptureUnavailable {
                reason: "ScreenCaptureKit native callback must pass human review before activation",
            }
        );
    }
}
