use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::cursor_metadata_runtime::CursorMetadataRuntime;
use crate::app::error::{AppError, AppResult};
use crate::app::recording_consumer::{
    consume_frames, RecordingConsumerInput, RecordingConsumerOutput,
};
use crate::app::recording_service_boundary::{
    CursorMainThreadDispatcher, PlatformRecordingService,
};
use crate::app::state_machine::{RecordingState, RecordingStateMachine};
use crate::core::capture::{AudioCapture, AudioConfig};
use crate::core::clock::SessionClock;
use crate::core::config::CaptureConfig;
use crate::core::frame::{AudioChunk, VideoFrameRef};
use crate::core::media_channel::{bounded_media_channel, MediaReceiver};
use crate::core::timeline::BeautifyConfigSnapshot;
use crate::core::window::WindowRecordingState;
use crate::media::recording_metadata::RecordingMetadataWriter;
use crate::media::recording_writer::{
    RecordingDiagnostics, RecordingResult, StopRecordingResponse, WriterDiagnostics,
};
use crate::platform::cpal_microphone::CpalMicrophoneCapture;
use crate::platform::windows::cursor_source::WindowsCursorSource;
use crate::platform::windows::graphics_capture::WindowsGraphicsCapture;
use crate::platform::windows::wasapi_loopback::WasapiLoopback;

/// Timeout for the consumer thread to return its result after stop.
const CONSUMER_RESULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

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

    // Capture primitives
    graphics_capture: WindowsGraphicsCapture,
    system_audio_capture: WasapiLoopback,
    mic_capture: CpalMicrophoneCapture,

    // Media channels
    video_receiver: Option<MediaReceiver<VideoFrameRef>>,
    system_audio_receiver: Option<MediaReceiver<AudioChunk>>,
    mic_receiver: Option<MediaReceiver<AudioChunk>>,

    // Consumer
    stop_flag: Option<Arc<AtomicBool>>,
    pause_flag: Arc<AtomicBool>,
    consumer_handle: Option<thread::JoinHandle<()>>,
    consumer_result_rx: Option<std::sync::mpsc::Receiver<RecordingConsumerOutput>>,

    // Cursor
    cursor_runtime: Option<CursorMetadataRuntime>,
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
            graphics_capture: WindowsGraphicsCapture::new(),
            system_audio_capture: WasapiLoopback::new(),
            mic_capture: CpalMicrophoneCapture::new(),
            video_receiver: None,
            system_audio_receiver: None,
            mic_receiver: None,
            stop_flag: None,
            pause_flag: Arc::new(AtomicBool::new(false)),
            consumer_handle: None,
            consumer_result_rx: None,
            cursor_runtime: None,
        }
    }

    fn clear_session_paths(&mut self) {
        self.last_cursor_metadata_path = None;
        self.last_effect_timeline_path = None;
        self.last_trim_metadata_path = None;
        self.last_cut_timeline_path = None;
        self.last_recording_output_path = None;
    }
}

fn cursor_metadata_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("cursor-metadata-{millis}-{seq}.json"))
}

fn trim_metadata_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("trim-metadata-{millis}-{seq}.json"))
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
        config: CaptureConfig,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        _cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        self.state_machine.start()?;
        self.clear_session_paths();
        self.session_id = self.session_id.wrapping_add(1);
        self.last_requested_system_audio = audio_config.capture_system_audio;
        self.last_requested_microphone = audio_config.capture_microphone;
        if let Ok(mut level) = self.mic_level.lock() {
            *level = 0.0;
        }
        self.pause_flag.store(false, Ordering::Relaxed);

        let session_clock = Arc::new(SessionClock::new());
        let stop_flag = Arc::new(AtomicBool::new(false));
        let (video_sender, video_receiver) = bounded_media_channel(90, "video");
        let (system_sender, system_receiver) = bounded_media_channel(256, "system");

        // Start WGC display capture.
        if let Err(error) =
            self.graphics_capture
                .start_display(config, video_sender, session_clock.clone())
        {
            self.state_machine.fail();
            return Err(error);
        }

        // Start WASAPI loopback if system audio requested.
        if audio_config.capture_system_audio {
            self.system_audio_capture
                .set_session_clock(session_clock.clone());
            if let Err(error) = self
                .system_audio_capture
                .start(audio_config.clone(), system_sender)
            {
                let _ = self.graphics_capture.stop();
                self.state_machine.fail();
                return Err(error);
            }
        }
        // Start cpal microphone if requested.
        let mic_rx = if audio_config.capture_microphone {
            self.mic_capture.set_session_clock(session_clock.clone());
            let (mic_sender, mic_rx) = bounded_media_channel(256, "mic");
            if let Err(error) = self.mic_capture.start(audio_config.clone(), mic_sender) {
                let _ = self.graphics_capture.stop();
                let _ = self.system_audio_capture.stop();
                self.state_machine.fail();
                return Err(error);
            }
            Some(mic_rx)
        } else {
            None
        };

        // Start cursor metadata runtime.
        let cursor_source = WindowsCursorSource::new(session_clock.clone());
        self.cursor_runtime = Some(CursorMetadataRuntime::spawn(
            cursor_source,
            config.fps,
            session_clock.clone(),
            beautify_snapshot.clone(),
            None, // capture_geometry will be set after first frame
        ));

        // Create writer.
        #[cfg(feature = "ffmpeg")]
        let writer: Box<dyn crate::media::recording_writer::RecordingWriter> = {
            let output_path = crate::media::export_paths::original_recording_path();
            self.last_recording_output_path = Some(output_path.to_string_lossy().to_string());
            Box::new(
                crate::media::ffmpeg_writer::FfmpegRecordingWriter::with_fps(
                    output_path,
                    config.fps,
                )?,
            )
        };

        #[cfg(not(feature = "ffmpeg"))]
        let writer: Box<dyn crate::media::recording_writer::RecordingWriter> =
            Box::new(crate::media::recording_writer::CountingRecordingWriter::new(None));

        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (consumer_result_tx, consumer_result_rx) =
            std::sync::mpsc::channel::<RecordingConsumerOutput>();
        self.consumer_result_rx = Some(consumer_result_rx);
        self.stop_flag = Some(stop_flag.clone());

        let trim_sensitivity = beautify_snapshot.trim_sensitivity.clone();
        let requested_system_audio = audio_config.capture_system_audio;
        let requested_microphone = audio_config.capture_microphone;
        let microphone_device = audio_config.microphone_device.clone();
        let denoise_mode = audio_config.denoise_mode;
        let mic_level = self.mic_level.clone();
        let pause_flag = self.pause_flag.clone();
        self.consumer_handle = Some(thread::spawn(move || {
            let output = consume_frames(RecordingConsumerInput {
                stop_flag,
                pause_flag,
                video_rx: video_receiver,
                system_audio_rx: system_receiver,
                mic_rx,
                frame_count,
                writer,
                mic_level,
                trim_sensitivity,
                requested_system_audio,
                requested_microphone,
                microphone_device,
                denoise_mode,
            });
            let _ = consumer_result_tx.send(output);
        }));

        Ok(())
    }

    fn start_window(
        &mut self,
        window_id: u32,
        config: CaptureConfig,
        _show_system_cursor: bool,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        self.state_machine.start()?;
        self.clear_session_paths();
        self.session_id = self.session_id.wrapping_add(1);
        self.last_requested_system_audio = audio_config.capture_system_audio;
        self.last_requested_microphone = audio_config.capture_microphone;
        if let Ok(mut level) = self.mic_level.lock() {
            *level = 0.0;
        }
        self.pause_flag.store(false, Ordering::Relaxed);

        let session_clock = Arc::new(SessionClock::new());
        let stop_flag = Arc::new(AtomicBool::new(false));
        let (video_sender, video_receiver) = bounded_media_channel(90, "video");
        let (system_sender, system_receiver) = bounded_media_channel(256, "system");

        // Start WGC window capture.
        if let Err(error) = self.graphics_capture.start_window(
            window_id,
            config.clone(),
            video_sender,
            session_clock.clone(),
        ) {
            self.state_machine.fail();
            return Err(error);
        }

        // Start WASAPI loopback if system audio requested.
        if audio_config.capture_system_audio {
            self.system_audio_capture
                .set_session_clock(session_clock.clone());
            if let Err(error) = self
                .system_audio_capture
                .start(audio_config.clone(), system_sender)
            {
                let _ = self.graphics_capture.stop();
                self.state_machine.fail();
                return Err(error);
            }
        }
        // Start cpal microphone if requested.
        let mic_rx = if audio_config.capture_microphone {
            self.mic_capture.set_session_clock(session_clock.clone());
            let (mic_sender, mic_rx) = bounded_media_channel(256, "mic");
            if let Err(error) = self.mic_capture.start(audio_config.clone(), mic_sender) {
                let _ = self.graphics_capture.stop();
                let _ = self.system_audio_capture.stop();
                self.state_machine.fail();
                return Err(error);
            }
            Some(mic_rx)
        } else {
            None
        };

        // Start cursor metadata runtime.
        let cursor_source = WindowsCursorSource::new(session_clock.clone());
        self.cursor_runtime = Some(CursorMetadataRuntime::spawn(
            cursor_source,
            config.fps,
            session_clock.clone(),
            beautify_snapshot.clone(),
            None,
        ));

        // Create writer.
        #[cfg(feature = "ffmpeg")]
        let writer: Box<dyn crate::media::recording_writer::RecordingWriter> = {
            let output_path = crate::media::export_paths::original_recording_path();
            self.last_recording_output_path = Some(output_path.to_string_lossy().to_string());
            Box::new(
                crate::media::ffmpeg_writer::FfmpegRecordingWriter::with_fps(
                    output_path,
                    config.fps,
                )?,
            )
        };

        #[cfg(not(feature = "ffmpeg"))]
        let writer: Box<dyn crate::media::recording_writer::RecordingWriter> =
            Box::new(crate::media::recording_writer::CountingRecordingWriter::new(None));

        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (consumer_result_tx, consumer_result_rx) =
            std::sync::mpsc::channel::<RecordingConsumerOutput>();
        self.consumer_result_rx = Some(consumer_result_rx);
        self.stop_flag = Some(stop_flag.clone());

        let trim_sensitivity = beautify_snapshot.trim_sensitivity.clone();
        let requested_system_audio = audio_config.capture_system_audio;
        let requested_microphone = audio_config.capture_microphone;
        let microphone_device = audio_config.microphone_device.clone();
        let denoise_mode = audio_config.denoise_mode;
        let mic_level = self.mic_level.clone();
        let pause_flag = self.pause_flag.clone();
        self.consumer_handle = Some(thread::spawn(move || {
            let output = consume_frames(RecordingConsumerInput {
                stop_flag,
                pause_flag,
                video_rx: video_receiver,
                system_audio_rx: system_receiver,
                mic_rx,
                frame_count,
                writer,
                mic_level,
                trim_sensitivity,
                requested_system_audio,
                requested_microphone,
                microphone_device,
                denoise_mode,
            });
            let _ = consumer_result_tx.send(output);
        }));

        let _ = cursor_main_thread_dispatcher;

        Ok(())
    }

    fn stop(&mut self) -> AppResult<StopRecordingResponse> {
        // 1. Stop cursor runtime.
        let cursor_metadata = self
            .cursor_runtime
            .as_mut()
            .and_then(|runtime| runtime.stop());
        self.cursor_runtime = None;

        // 2. Stop graphics capture.
        let capture_stop_result = self.graphics_capture.stop();

        // 3. Stop WASAPI.
        let _ = self.system_audio_capture.stop();

        // 4. Stop mic.
        if self.last_requested_microphone {
            let _ = self.mic_capture.stop();
        }

        // 5. Signal consumer stop.
        if let Some(stop) = self.stop_flag.take() {
            stop.store(true, Ordering::Relaxed);
        }

        // 6. Receive consumer output.
        let consumer_output = if let Some(rx) = self.consumer_result_rx.take() {
            match rx.recv_timeout(CONSUMER_RESULT_TIMEOUT) {
                Ok(output) => Some(output),
                Err(_) => {
                    eprintln!("警告: 消费者线程超时未返回结果");
                    None
                }
            }
        } else {
            None
        };

        // 7. Wait for consumer thread (bounded — do not hang if consumer is stuck).
        //    If recv_timeout already timed out, the consumer is likely stuck in
        //    writer.finish() or artifact validation. Detach rather than hang.
        if consumer_output.is_some() {
            // Consumer already returned a result via channel — safe to join.
            if let Some(handle) = self.consumer_handle.take() {
                let _ = handle.join();
            }
        } else if self.consumer_handle.is_some() {
            // Consumer timed out. Detach the thread to avoid blocking stop().
            eprintln!("警告: 消费者线程已超时，放弃等待以避免挂起");
            self.consumer_handle = None;
        }

        // 8. Write cursor metadata sidecar.
        if let Some(ref metadata) = cursor_metadata {
            let path = cursor_metadata_path();
            if let Err(e) = RecordingMetadataWriter::write_metadata(&path, metadata) {
                eprintln!("写入光标元数据失败: {e}");
            } else {
                self.last_cursor_metadata_path = Some(path.to_string_lossy().to_string());
            }
        }

        // 9. Write trim metadata sidecar.
        if let Some(ref output) = consumer_output {
            let trim_path = trim_metadata_path();
            if let Err(e) = crate::media::trim_metadata::TrimMetadataWriter::write_metadata(
                &trim_path,
                &output.trim_metadata,
            ) {
                eprintln!("写入裁剪元数据失败: {e}");
            } else {
                self.last_trim_metadata_path = Some(trim_path.to_string_lossy().to_string());
            }
        }

        let (mut result, diagnostics, mut errors) = if let Some(output) = consumer_output {
            (output.result, output.diagnostics, output.errors)
        } else {
            (
                RecordingResult {
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
                    finalization_errors: vec!["消费者线程超时".to_string()],
                },
                RecordingDiagnostics::default(),
                vec!["消费者线程超时".to_string()],
            )
        };

        // Merge capture stop errors.
        if let Err(e) = capture_stop_result {
            errors.push(format!("屏幕捕获停止失败: {e}"));
        }

        // 10. Reset mic level.
        if let Ok(mut level) = self.mic_level.lock() {
            *level = 0.0;
        }

        // 11. Populate result sidecar paths.
        result.cursor_metadata_path = self.last_cursor_metadata_path.clone();
        result.effect_timeline_path = self.last_effect_timeline_path.clone();
        result.trim_metadata_path = self.last_trim_metadata_path.clone();
        result.cut_timeline_path = self.last_cut_timeline_path.clone();
        result.diagnostics = diagnostics;
        result.finalization_errors = errors.clone();

        // 12. Drive state machine.
        // Transition to Processing first (required before complete/fail).
        if let Err(e) = self.state_machine.stop() {
            self.state_machine.fail();
            return Err(e);
        }
        let failed = !errors.is_empty() || result.duration_secs == 0;
        if failed {
            self.state_machine.fail();
        } else if let Err(e) = self.state_machine.complete() {
            self.state_machine.fail();
            return Err(e);
        }

        // 13. Clear channel receivers.
        self.video_receiver = None;
        self.system_audio_receiver = None;
        self.mic_receiver = None;

        Ok(StopRecordingResponse { result, failed })
    }

    fn pause(&mut self) -> AppResult<()> {
        self.state_machine.pause()?;
        self.pause_flag.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn resume(&mut self) -> AppResult<()> {
        self.state_machine.resume()?;
        self.pause_flag.store(false, Ordering::Relaxed);
        Ok(())
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

    // This test touches real CPAL/WASAPI device paths. On machines without
    // capture devices (e.g., CI, VMs, or review environments), it can panic
    // with HRESULT(0x80040154) "没有注册类". Use `cargo test -- --ignored`
    // to run this manually on a machine with capture devices.
    #[test]
    #[ignore]
    fn windows_service_fails_visibly_before_native_capture_exists() {
        let mut service = WindowsRecordingService::new();
        let result = service.start(
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
        );

        // The service may fail immediately (e.g., WGC not supported) or succeed
        // initially (worker thread will fail later). Both are valid — the key
        // assertion is that failures are visible, not silent.
        match result {
            Ok(()) => {
                // Worker started — stop it to verify cleanup works.
                let stop_result = service.stop();
                assert!(stop_result.is_ok(), "stop should succeed after start");
            }
            Err(err) => {
                // Immediate failure — verify it's a visible error.
                assert!(
                    err.to_string().contains("Windows")
                        || err.to_string().contains("屏幕捕获")
                        || err.to_string().contains("NativeCaptureUnavailable")
                        || err.to_string().contains("不支持"),
                    "unexpected error: {err}"
                );
                assert_eq!(service.state(), RecordingState::Failed);
            }
        }
    }
}
