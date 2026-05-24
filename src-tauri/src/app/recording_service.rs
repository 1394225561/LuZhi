use std::sync::mpsc::{channel, Receiver};

use crate::app::error::AppResult;
use crate::app::state_machine::{RecordingState, RecordingStateMachine};
use crate::core::capture::ScreenCapture;
use crate::core::config::CaptureConfig;
use crate::core::frame::VideoFrameRef;

/// Orchestrates a recording session over a platform capture adapter.
pub struct RecordingService<C: ScreenCapture> {
    capture: C,
    state_machine: RecordingStateMachine,
    frame_receiver: Option<Receiver<VideoFrameRef>>,
}

impl<C: ScreenCapture> RecordingService<C> {
    pub fn new(capture: C) -> Self {
        Self {
            capture,
            state_machine: RecordingStateMachine::new(),
            frame_receiver: None,
        }
    }

    pub fn state(&self) -> RecordingState {
        self.state_machine.state()
    }

    pub fn start(&mut self, config: CaptureConfig) -> AppResult<()> {
        self.state_machine.start()?;
        let (sender, receiver) = channel();
        self.capture.start(config, sender)?;
        self.frame_receiver = Some(receiver);
        Ok(())
    }

    pub fn stop(&mut self) -> AppResult<()> {
        self.capture.stop()?;
        self.state_machine.stop()?;
        self.state_machine.complete()?;
        self.frame_receiver = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Sender;

    use super::*;
    use crate::app::error::AppError;
    use crate::core::capture::{CaptureCapabilities, VideoFrameSink};

    #[derive(Default)]
    struct MockScreenCapture {
        started: bool,
        stopped: bool,
        fail_on_start: bool,
        sink_seen: Option<Sender<VideoFrameRef>>,
    }

    impl ScreenCapture for MockScreenCapture {
        fn start(&mut self, _config: CaptureConfig, sink: VideoFrameSink) -> AppResult<()> {
            if self.fail_on_start {
                return Err(AppError::CaptureFailed {
                    reason: "mock start failure".to_string(),
                });
            }
            self.started = true;
            self.sink_seen = Some(sink);
            Ok(())
        }

        fn stop(&mut self) -> AppResult<()> {
            self.stopped = true;
            Ok(())
        }

        fn capabilities(&self) -> CaptureCapabilities {
            CaptureCapabilities::phase_one_macos()
        }
    }

    #[test]
    fn start_moves_service_to_recording() {
        let capture = MockScreenCapture::default();
        let mut service = RecordingService::new(capture);

        service
            .start(CaptureConfig::full_screen_1080p_30fps())
            .unwrap();

        assert_eq!(service.state(), RecordingState::Recording);
        assert!(service.frame_receiver.is_some());
    }

    #[test]
    fn stop_moves_service_to_completed() {
        let capture = MockScreenCapture::default();
        let mut service = RecordingService::new(capture);

        service
            .start(CaptureConfig::full_screen_1080p_30fps())
            .unwrap();
        service.stop().unwrap();

        assert_eq!(service.state(), RecordingState::Completed);
        assert!(service.frame_receiver.is_none());
    }
}
