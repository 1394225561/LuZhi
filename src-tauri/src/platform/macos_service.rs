use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use super::macos::cpal_microphone::CpalMicrophoneCapture;
use super::macos::screen_capture_kit::MacScreenCapture;
use crate::app::error::AppResult;
use crate::app::state_machine::{RecordingState, RecordingStateMachine};
use crate::core::capture::{AudioCapture, AudioConfig, ScreenCapture};
use crate::core::config::CaptureConfig;
use crate::core::frame::{AudioChunk, VideoFrameRef};
use crate::core::media_channel::{bounded_media_channel, MediaReceiver};
use crate::media::audio_mixer::SimpleAudioMixer;

/// Non-generic recording service for macOS.
///
/// `MacScreenCapture` implements both `ScreenCapture` and `AudioCapture` because
/// the underlying SCStream is a unified pipeline — video and system audio come
/// from the same stream. This service uses `start_combined()` to set both sinks
/// atomically, then manages the microphone capture separately via `CpalMicrophoneCapture`.
pub struct MacRecordingService {
    screen_capture: MacScreenCapture,
    mic_capture: CpalMicrophoneCapture,
    state_machine: RecordingStateMachine,
    _mixer: SimpleAudioMixer,
    video_receiver: Option<MediaReceiver<VideoFrameRef>>,
    system_audio_receiver: Option<MediaReceiver<AudioChunk>>,
    mic_receiver: Option<MediaReceiver<AudioChunk>>,
    stop_flag: Option<Arc<AtomicBool>>,
    frame_count: Arc<std::sync::atomic::AtomicU64>,
}

impl MacRecordingService {
    pub fn new() -> Self {
        Self {
            screen_capture: MacScreenCapture::new(),
            mic_capture: CpalMicrophoneCapture::new(),
            state_machine: RecordingStateMachine::new(),
            _mixer: SimpleAudioMixer::new(),
            video_receiver: None,
            system_audio_receiver: None,
            mic_receiver: None,
            stop_flag: None,
            frame_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub fn state(&self) -> RecordingState {
        self.state_machine.state()
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count.load(Ordering::Relaxed)
    }

    /// Starts video + system audio capture via SCStream, and optionally microphone.
    pub fn start(&mut self, config: CaptureConfig, audio_config: AudioConfig) -> AppResult<()> {
        self.state_machine.start()?;

        // Create bounded channels for video and system audio.
        const VIDEO_QUEUE_CAPACITY: usize = 90;
        const AUDIO_QUEUE_CAPACITY: usize = 256;
        let (video_sender, video_receiver) = bounded_media_channel(VIDEO_QUEUE_CAPACITY);
        let (audio_sender, audio_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY);

        // Start the unified SCStream with both sinks.
        if let Err(error) = self
            .screen_capture
            .start_combined(config, video_sender, audio_sender)
        {
            self.state_machine.fail();
            return Err(error);
        }
        self.video_receiver = Some(video_receiver);
        self.system_audio_receiver = Some(audio_receiver);

        // Start microphone capture if requested.
        if audio_config.capture_microphone {
            let (mic_sender, mic_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY);
            if let Err(error) = self.mic_capture.start(audio_config, mic_sender) {
                // Rollback: stop screen capture.
                let _ = ScreenCapture::stop(&mut self.screen_capture);
                self.video_receiver = None;
                self.system_audio_receiver = None;
                self.state_machine.fail();
                return Err(error);
            }
            self.mic_receiver = Some(mic_receiver);
        }

        // Spawn frame consumer thread (drain mode).
        let stop_flag = Arc::new(AtomicBool::new(false));
        self.stop_flag = Some(stop_flag.clone());

        let video_rx = self.video_receiver.take().unwrap();
        let system_audio_rx = self.system_audio_receiver.take().unwrap();
        let mic_rx = self.mic_receiver.take();
        let frame_count = self.frame_count.clone();
        frame_count.store(0, Ordering::Relaxed);

        thread::spawn(move || {
            Self::consume_frames(stop_flag, video_rx, system_audio_rx, mic_rx, frame_count);
        });

        Ok(())
    }

    /// Stops all captures and finalizes the recording session.
    pub fn stop(&mut self) -> AppResult<()> {
        // Signal the consumer thread to stop.
        if let Some(flag) = &self.stop_flag {
            flag.store(true, Ordering::Relaxed);
        }

        // Stop captures.
        let capture_result = ScreenCapture::stop(&mut self.screen_capture);
        let mic_result = self.mic_capture.stop();

        self.stop_flag = None;

        capture_result?;
        mic_result?;

        self.state_machine.stop()?;
        self.state_machine.complete()?;
        Ok(())
    }

    pub fn pause(&mut self) -> AppResult<()> {
        self.state_machine.pause()
    }

    pub fn resume(&mut self) -> AppResult<()> {
        self.state_machine.resume()
    }

    /// Background thread that drains video and audio channels to prevent blocking.
    fn consume_frames(
        stop_flag: Arc<AtomicBool>,
        video_rx: MediaReceiver<VideoFrameRef>,
        system_audio_rx: MediaReceiver<AudioChunk>,
        mic_rx: Option<MediaReceiver<AudioChunk>>,
        frame_count: Arc<std::sync::atomic::AtomicU64>,
    ) {
        loop {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            // Drain video frames (non-blocking).
            while let Ok(_frame) = video_rx.try_recv() {
                frame_count.fetch_add(1, Ordering::Relaxed);
            }

            // Drain system audio (non-blocking).
            while let Ok(_chunk) = system_audio_rx.try_recv() {
                // Drain — encoding not yet implemented.
            }

            // Drain microphone audio (non-blocking).
            if let Some(ref mic_rx) = mic_rx {
                while let Ok(_chunk) = mic_rx.try_recv() {
                    // Drain — encoding not yet implemented.
                }
            }

            // Sleep briefly to avoid busy-waiting.
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

impl Default for MacRecordingService {
    fn default() -> Self {
        Self::new()
    }
}
