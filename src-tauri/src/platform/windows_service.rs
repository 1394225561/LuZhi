use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use crate::app::error::{AppError, AppResult};
use crate::app::recording_service_boundary::{
    CursorMainThreadDispatcher, PlatformRecordingService,
};
use crate::app::state_machine::{RecordingState, RecordingStateMachine};
use crate::core::capture::AudioConfig;
use crate::core::config::CaptureConfig;
use crate::core::timeline::BeautifyConfigSnapshot;
use crate::core::window::WindowRecordingState;
use crate::media::recording_writer::{
    RecordingDiagnostics, RecordingResult, StopRecordingResponse, WriterDiagnostics,
};

pub struct WindowsRecordingService {
    state_machine: RecordingStateMachine,
    mic_level: Arc<Mutex<f64>>,
    session_id: u64,
    last_cursor_metadata_path: Option<String>,
    last_effect_timeline_path: Option<String>,
    last_trim_metadata_path: Option<String>,
    last_cut_timeline_path: Option<String>,
    last_recording_output_path: Option<String>,
    last_requested_system_audio: bool,
    last_requested_microphone: bool,
}

impl WindowsRecordingService {
    pub fn new() -> Self {
        Self {
            state_machine: RecordingStateMachine::new(),
            mic_level: Arc::new(Mutex::new(0.0)),
            session_id: 0,
            last_cursor_metadata_path: None,
            last_effect_timeline_path: None,
            last_trim_metadata_path: None,
            last_cut_timeline_path: None,
            last_recording_output_path: None,
            last_requested_system_audio: false,
            last_requested_microphone: false,
        }
    }

    fn unavailable(reason: &'static str) -> AppError {
        AppError::NativeCaptureUnavailable { reason }
    }
}

impl Default for WindowsRecordingService {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformRecordingService for WindowsRecordingService {
    fn state(&self) -> RecordingState {
        self.state_machine.state()
    }

    fn start(
        &mut self,
        _config: CaptureConfig,
        audio_config: AudioConfig,
        _beautify_snapshot: BeautifyConfigSnapshot,
        _cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        self.last_requested_system_audio = audio_config.capture_system_audio;
        self.last_requested_microphone = audio_config.capture_microphone;
        self.session_id = self.session_id.wrapping_add(1);
        self.state_machine.fail();
        Err(Self::unavailable("Windows Graphics Capture 尚未接入"))
    }

    fn start_window(
        &mut self,
        _window_id: u32,
        _show_system_cursor: bool,
        audio_config: AudioConfig,
        _beautify_snapshot: BeautifyConfigSnapshot,
        _cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        self.last_requested_system_audio = audio_config.capture_system_audio;
        self.last_requested_microphone = audio_config.capture_microphone;
        self.session_id = self.session_id.wrapping_add(1);
        self.state_machine.fail();
        Err(Self::unavailable("Windows 窗口录制尚未接入"))
    }

    fn stop(&mut self) -> AppResult<StopRecordingResponse> {
        if let Ok(mut level) = self.mic_level.lock() {
            *level = 0.0;
        }
        Ok(StopRecordingResponse {
            result: RecordingResult {
                duration_secs: 0,
                frame_count: 0,
                mixed_audio_chunk_count: 0,
                output_path: None,
                cursor_metadata_path: None,
                effect_timeline_path: None,
                trim_metadata_path: None,
                cut_timeline_path: None,
                writer_diagnostics: WriterDiagnostics::default(),
                diagnostics: RecordingDiagnostics::default(),
                finalization_errors: vec!["Windows 录制尚未启动".to_string()],
            },
            failed: true,
        })
    }

    fn pause(&mut self) -> AppResult<()> {
        Err(Self::unavailable("Windows 录制尚未启动"))
    }

    fn resume(&mut self) -> AppResult<()> {
        Err(Self::unavailable("Windows 录制尚未启动"))
    }

    fn mic_level_ref(&self) -> Arc<Mutex<f64>> {
        self.mic_level.clone()
    }

    fn take_window_state_receiver(&mut self) -> Option<Receiver<WindowRecordingState>> {
        None
    }

    fn current_session_id(&self) -> u64 {
        self.session_id
    }

    fn last_cursor_metadata_path(&self) -> Option<String> {
        self.last_cursor_metadata_path.clone()
    }

    fn last_effect_timeline_path(&self) -> Option<String> {
        self.last_effect_timeline_path.clone()
    }

    fn set_last_effect_timeline_path(&mut self, path: Option<String>) {
        self.last_effect_timeline_path = path;
    }

    fn last_trim_metadata_path(&self) -> Option<String> {
        self.last_trim_metadata_path.clone()
    }

    fn last_cut_timeline_path(&self) -> Option<String> {
        self.last_cut_timeline_path.clone()
    }

    fn set_last_cut_timeline_path(&mut self, path: Option<String>) {
        self.last_cut_timeline_path = path;
    }

    fn last_recording_output_path(&self) -> Option<String> {
        self.last_recording_output_path.clone()
    }

    fn last_requested_system_audio(&self) -> bool {
        self.last_requested_system_audio
    }

    fn last_requested_microphone(&self) -> bool {
        self.last_requested_microphone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::capture::DenoiseMode;
    use crate::core::config::CaptureConfig;

    struct InlineDispatcher;

    impl CursorMainThreadDispatcher for InlineDispatcher {
        fn run_on_main_thread(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            task();
            Ok(())
        }
    }

    #[test]
    fn windows_service_fails_visibly_before_native_capture_exists() {
        let mut service = WindowsRecordingService::new();
        let err = service
            .start(
                CaptureConfig::full_screen_1080p_30fps(),
                AudioConfig {
                    capture_system_audio: true,
                    capture_microphone: true,
                    microphone_device: None,
                    sample_rate: 48000,
                    channels: 2,
                    denoise_mode: DenoiseMode::default(),
                },
                BeautifyConfigSnapshot {
                    cursor_magnification: true,
                    magnification_factor: 2.0,
                    cursor_smoothing: true,
                    auto_trim_silences: false,
                    trim_sensitivity: "medium".to_string(),
                    raw_system_cursor_visible: false,
                },
                Box::new(InlineDispatcher),
            )
            .unwrap_err();

        assert!(err.to_string().contains("Windows Graphics Capture"));
        assert_eq!(service.state(), RecordingState::Failed);
        assert!(service.last_recording_output_path().is_none());
    }
}
