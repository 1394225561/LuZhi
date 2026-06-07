use std::sync::Arc;

use crate::app::error::{AppError, AppResult};
use crate::core::capture::{AudioChunkSink, VideoFrameSink, WindowCapture};
use crate::core::window::{WindowInfo, WindowRecordingState};

/// Windows 窗口捕获占位实现
///
/// 当前为占位实现，所有方法返回 NativeCaptureUnavailable。
/// 待 DXGI Desktop Duplication 完整实现后，补充窗口捕获逻辑。
pub struct WinWindowCapture;

impl WindowCapture for WinWindowCapture {
    fn list_windows(&self) -> AppResult<Vec<WindowInfo>> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }

    fn get_thumbnail(&self, _window_id: u32) -> AppResult<Option<String>> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }

    fn start_window_stream(
        &mut self,
        _window_id: u32,
        _capture_system_audio: bool,
        _show_system_cursor: bool,
        _video_sink: VideoFrameSink,
        _audio_sink: AudioChunkSink,
        _session_clock: Arc<crate::core::clock::SessionClock>,
    ) -> AppResult<()> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }

    fn stop_window_stream(&mut self) -> AppResult<()> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }

    fn window_state(&self, _window_id: u32) -> AppResult<WindowRecordingState> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }
}
