use std::sync::mpsc::{channel, Receiver};

use crate::app::error::AppResult;
use crate::app::state_machine::{RecordingState, RecordingStateMachine};
use crate::core::capture::{AudioCapture, AudioConfig, ScreenCapture};
use crate::core::config::CaptureConfig;
use crate::core::frame::{AudioChunk, VideoFrameRef};
use crate::media::audio_mixer::{AudioMixer, SimpleAudioMixer};

/// Orchestrates a recording session over platform capture adapters.
///
/// `C` provides video frames and optionally system audio.
/// `A` provides microphone audio.
/// During recording, the service mixes system audio and microphone in real time.
pub struct RecordingService<C: ScreenCapture, A: AudioCapture> {
    capture: C,
    audio_capture: A,
    state_machine: RecordingStateMachine,
    mixer: SimpleAudioMixer,
    video_receiver: Option<Receiver<VideoFrameRef>>,
    audio_receiver: Option<Receiver<AudioChunk>>,
    mic_receiver: Option<Receiver<AudioChunk>>,
}

impl<C: ScreenCapture, A: AudioCapture> RecordingService<C, A> {
    pub fn new(capture: C, audio_capture: A) -> Self {
        Self {
            capture,
            audio_capture,
            state_machine: RecordingStateMachine::new(),
            mixer: SimpleAudioMixer::new(),
            video_receiver: None,
            audio_receiver: None,
            mic_receiver: None,
        }
    }

    pub fn state(&self) -> RecordingState {
        self.state_machine.state()
    }

    /// Starts both video and audio capture.
    ///
    /// `config` controls screen capture resolution and fps.
    /// `audio_config` controls system audio and microphone settings.
    pub fn start(&mut self, config: CaptureConfig, audio_config: AudioConfig) -> AppResult<()> {
        self.state_machine.start()?;

        // Start video (and system audio if the capture supports it).
        let (video_sender, video_receiver) = channel();
        if let Err(error) = self.capture.start(config, video_sender) {
            self.state_machine.fail();
            return Err(error);
        }
        self.video_receiver = Some(video_receiver);

        // Start microphone capture if requested.
        if audio_config.capture_microphone {
            let (mic_sender, mic_receiver) = channel();
            if let Err(error) = self.audio_capture.start(audio_config.clone(), mic_sender) {
                // Roll back video capture on mic failure.
                let _ = self.capture.stop();
                self.video_receiver = None;
                self.state_machine.fail();
                return Err(error);
            }
            self.mic_receiver = Some(mic_receiver);
        }

        Ok(())
    }

    /// Stops all active captures and finalizes the session.
    pub fn stop(&mut self) -> AppResult<()> {
        let capture_result = self.capture.stop();
        let mic_result = self.audio_capture.stop();

        self.video_receiver = None;
        self.audio_receiver = None;
        self.mic_receiver = None;

        capture_result?;
        mic_result?;

        self.state_machine.stop()?;
        self.state_machine.complete()?;
        Ok(())
    }

    /// Returns a reference to the audio mixer for testing.
    pub fn mixer(&self) -> &dyn AudioMixer {
        &self.mixer
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Sender;

    use super::*;
    use crate::app::error::AppError;
    use crate::core::capture::{
        AudioCapabilities, AudioChunkSink, AudioDevice, CaptureCapabilities, VideoFrameSink,
    };

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

    #[derive(Default)]
    struct MockAudioCapture {
        started: bool,
        stopped: bool,
        fail_on_start: bool,
    }

    impl AudioCapture for MockAudioCapture {
        fn start(&mut self, _config: AudioConfig, _sink: AudioChunkSink) -> AppResult<()> {
            if self.fail_on_start {
                return Err(AppError::AudioCaptureFailed {
                    reason: "mock audio start failure".to_string(),
                });
            }
            self.started = true;
            Ok(())
        }

        fn stop(&mut self) -> AppResult<()> {
            self.stopped = true;
            Ok(())
        }

        fn device_list(&self) -> AppResult<Vec<AudioDevice>> {
            Ok(vec![])
        }

        fn capabilities(&self) -> AudioCapabilities {
            AudioCapabilities {
                supports_system_audio: false,
                supports_microphone: true,
            }
        }
    }

    #[test]
    fn start_moves_service_to_recording() {
        let capture = MockScreenCapture::default();
        let audio = MockAudioCapture::default();
        let mut service = RecordingService::new(capture, audio);

        service
            .start(
                CaptureConfig::full_screen_1080p_30fps(),
                AudioConfig {
                    capture_system_audio: false,
                    capture_microphone: true,
                    microphone_device: None,
                    sample_rate: 48000,
                    channels: 2,
                },
            )
            .unwrap();

        assert_eq!(service.state(), RecordingState::Recording);
        assert!(service.video_receiver.is_some());
    }

    #[test]
    fn stop_moves_service_to_completed() {
        let capture = MockScreenCapture::default();
        let audio = MockAudioCapture::default();
        let mut service = RecordingService::new(capture, audio);

        service
            .start(
                CaptureConfig::full_screen_1080p_30fps(),
                AudioConfig {
                    capture_system_audio: false,
                    capture_microphone: true,
                    microphone_device: None,
                    sample_rate: 48000,
                    channels: 2,
                },
            )
            .unwrap();
        service.stop().unwrap();

        assert_eq!(service.state(), RecordingState::Completed);
        assert!(service.video_receiver.is_none());
    }

    #[test]
    fn start_failure_transitions_to_failed_state() {
        let capture = MockScreenCapture {
            fail_on_start: true,
            ..Default::default()
        };
        let audio = MockAudioCapture::default();
        let mut service = RecordingService::new(capture, audio);

        let error = service
            .start(
                CaptureConfig::full_screen_1080p_30fps(),
                AudioConfig {
                    capture_system_audio: false,
                    capture_microphone: true,
                    microphone_device: None,
                    sample_rate: 48000,
                    channels: 2,
                },
            )
            .unwrap_err();

        assert_eq!(service.state(), RecordingState::Failed);
        assert!(matches!(error, AppError::CaptureFailed { .. }));
    }

    #[test]
    fn mic_failure_rolls_back_video_capture() {
        let capture = MockScreenCapture::default();
        let audio = MockAudioCapture {
            fail_on_start: true,
            ..Default::default()
        };
        let mut service = RecordingService::new(capture, audio);

        let error = service
            .start(
                CaptureConfig::full_screen_1080p_30fps(),
                AudioConfig {
                    capture_system_audio: false,
                    capture_microphone: true,
                    microphone_device: None,
                    sample_rate: 48000,
                    channels: 2,
                },
            )
            .unwrap_err();

        assert_eq!(service.state(), RecordingState::Failed);
        assert!(matches!(error, AppError::AudioCaptureFailed { .. }));
    }
}
