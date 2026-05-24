use crate::app::error::{AppError, AppResult};
use crate::core::capture::{
    AudioCapabilities, AudioCapture, AudioChunkSink, AudioConfig, AudioDevice,
};

/// Windows audio capture stub using WASAPI loopback.
///
/// This is a placeholder implementation for Phase 2. All methods return
/// `NativeCaptureUnavailable` until WASAPI integration is implemented.
pub struct WasapiLoopback;

impl WasapiLoopback {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WasapiLoopback {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCapture for WasapiLoopback {
    fn start(&mut self, _config: AudioConfig, _sink: AudioChunkSink) -> AppResult<()> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "WASAPI Loopback 尚未实现",
        })
    }

    fn stop(&mut self) -> AppResult<()> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "WASAPI Loopback 尚未实现",
        })
    }

    fn device_list(&self) -> AppResult<Vec<AudioDevice>> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "WASAPI Loopback 尚未实现",
        })
    }

    fn capabilities(&self) -> AudioCapabilities {
        AudioCapabilities {
            supports_system_audio: false,
            supports_microphone: false,
        }
    }
}
