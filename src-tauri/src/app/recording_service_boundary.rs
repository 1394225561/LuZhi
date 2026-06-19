use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use crate::app::error::AppResult;
use crate::app::state_machine::RecordingState;
use crate::core::capture::AudioConfig;
use crate::core::config::CaptureConfig;
use crate::core::timeline::BeautifyConfigSnapshot;
use crate::core::window::WindowRecordingState;
use crate::media::recording_writer::StopRecordingResponse;

/// Runs platform-specific main-thread work needed by cursor metadata capture.
///
/// macOS uses this to read AppKit cursor state. Windows implementations can
/// execute the task inline because the planned cursor source does not need an
/// AppKit-style main-thread hop.
pub trait CursorMainThreadDispatcher: Send + Sync + 'static {
    fn run_on_main_thread(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String>;
}

/// Platform service API consumed by Tauri command handlers.
///
/// This is intentionally shaped around the existing `MacRecordingService`
/// surface so the first refactor can preserve macOS behavior.
pub trait PlatformRecordingService: Send {
    fn state(&self) -> RecordingState;

    fn start(
        &mut self,
        config: CaptureConfig,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()>;

    fn start_window(
        &mut self,
        window_id: u32,
        show_system_cursor: bool,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()>;

    fn stop(&mut self) -> AppResult<StopRecordingResponse>;
    fn pause(&mut self) -> AppResult<()>;
    fn resume(&mut self) -> AppResult<()>;

    fn mic_level_ref(&self) -> Arc<Mutex<f64>>;
    fn take_window_state_receiver(&mut self) -> Option<Receiver<WindowRecordingState>>;

    fn current_session_id(&self) -> u64;
    fn last_cursor_metadata_path(&self) -> Option<String>;
    fn last_effect_timeline_path(&self) -> Option<String>;
    fn set_last_effect_timeline_path(&mut self, path: Option<String>);
    fn last_trim_metadata_path(&self) -> Option<String>;
    fn last_cut_timeline_path(&self) -> Option<String>;
    fn set_last_cut_timeline_path(&mut self, path: Option<String>);
    fn last_recording_output_path(&self) -> Option<String>;
    fn last_requested_system_audio(&self) -> bool;
    fn last_requested_microphone(&self) -> bool;
}
