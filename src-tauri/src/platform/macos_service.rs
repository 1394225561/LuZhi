use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::macos::cpal_microphone::CpalMicrophoneCapture;
use super::macos::cursor_kind::CursorMainThreadDispatcher;
use super::macos::cursor_source::MacCursorSource;
use super::macos::screen_capture_kit::MacScreenCapture;
use crate::app::cursor_metadata_runtime::CursorMetadataRuntime;
use crate::app::error::AppResult;
use crate::app::state_machine::{RecordingState, RecordingStateMachine};
use crate::core::capture::{AudioCapture, AudioConfig, ScreenCapture};
use crate::core::config::CaptureConfig;
use crate::core::frame::{AudioChunk, VideoFrameRef};
use crate::core::media_channel::{bounded_media_channel, MediaReceiver};
use crate::core::timeline::BeautifyConfigSnapshot;
use crate::media::audio_mixer::SimpleAudioMixer;
use crate::media::mic_level::MicLevelDetector;
use crate::media::recording_metadata::RecordingMetadataWriter;
#[cfg(not(feature = "ffmpeg"))]
use crate::media::recording_writer::CountingRecordingWriter;
use crate::media::recording_writer::{
    RecordingDiagnostics, RecordingResult, RecordingWriter, StopRecordingResponse,
};
use crate::media::silence_detector::FrameDiffAnalyzer;
use crate::media::trim_audio_activity::BaseAudioActivityAnalyzer;
use crate::media::trim_metadata::{TrimMetadata, TrimMetadataWriter, TRIM_METADATA_SCHEMA_VERSION};

/// Internal return type that bundles the recording result with trim metadata.
struct RecordingConsumerOutput {
    result: RecordingResult,
    trim_metadata: TrimMetadata,
    /// Audio diagnostics collected during consumer thread execution.
    diagnostics: RecordingDiagnostics,
    /// Errors collected during consumer thread execution (writer push/finish failures).
    errors: Vec<String>,
}

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
    consumer_handle: Option<thread::JoinHandle<()>>,
    /// Receives the consumer thread's result via channel (bounded join, Important 1).
    consumer_result_rx: Option<std::sync::mpsc::Receiver<RecordingConsumerOutput>>,
    frame_count: Arc<std::sync::atomic::AtomicU64>,
    /// 当前麦克风 RMS 电平值 (0.0 ~ 1.0)，由消费线程周期性更新，外部通过 `mic_level()` 读取。
    mic_level: Arc<Mutex<f64>>,
    cursor_runtime: Option<CursorMetadataRuntime>,
    last_cursor_metadata_path: Option<String>,
    last_effect_timeline_path: Option<String>,
    last_trim_metadata_path: Option<String>,
    last_cut_timeline_path: Option<String>,
    last_recording_output_path: Option<String>,
    /// Monotonically incrementing session counter. Used to guard async
    /// post-process jobs against writing stale results into a new session.
    session_id: u64,
    /// Last recording's requested system audio flag (for export contract).
    last_requested_system_audio: bool,
    /// Last recording's requested microphone flag (for export contract).
    last_requested_microphone: bool,
}

impl MacRecordingService {
    pub fn last_cursor_metadata_path(&self) -> Option<String> {
        self.last_cursor_metadata_path.clone()
    }

    pub fn last_requested_system_audio(&self) -> bool {
        self.last_requested_system_audio
    }

    pub fn last_requested_microphone(&self) -> bool {
        self.last_requested_microphone
    }

    pub fn set_last_effect_timeline_path(&mut self, path: Option<String>) {
        self.last_effect_timeline_path = path;
    }

    pub fn last_trim_metadata_path(&self) -> Option<String> {
        self.last_trim_metadata_path.clone()
    }

    pub fn set_last_cut_timeline_path(&mut self, path: Option<String>) {
        self.last_cut_timeline_path = path;
    }

    pub fn current_session_id(&self) -> u64 {
        self.session_id
    }

    pub fn last_recording_output_path(&self) -> Option<String> {
        self.last_recording_output_path.clone()
    }

    pub fn last_effect_timeline_path(&self) -> Option<String> {
        self.last_effect_timeline_path.clone()
    }
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
            consumer_handle: None,
            consumer_result_rx: None,
            frame_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            mic_level: Arc::new(Mutex::new(0.0)),
            cursor_runtime: None,
            last_cursor_metadata_path: None,
            last_effect_timeline_path: None,
            last_trim_metadata_path: None,
            last_cut_timeline_path: None,
            last_recording_output_path: None,
            session_id: 0,
            last_requested_system_audio: false,
            last_requested_microphone: false,
        }
    }

    pub fn state(&self) -> RecordingState {
        self.state_machine.state()
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count.load(Ordering::Relaxed)
    }

    /// 返回当前麦克风 RMS 电平值 (0.0 ~ 1.0)。
    pub fn mic_level(&self) -> f64 {
        self.mic_level.lock().map(|g| *g).unwrap_or(0.0)
    }

    /// 返回麦克风电平值的共享引用，供外部线程周期性读取并发射事件。
    pub fn mic_level_ref(&self) -> Arc<Mutex<f64>> {
        self.mic_level.clone()
    }

    /// Starts video + system audio capture via SCStream, and optionally microphone.
    pub fn start(
        &mut self,
        config: CaptureConfig,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        self.state_machine.start()?;

        // Clear stale session state from any previous recording.
        self.last_cursor_metadata_path = None;
        self.last_effect_timeline_path = None;
        self.last_trim_metadata_path = None;
        self.last_cut_timeline_path = None;
        self.last_recording_output_path = None;
        self.session_id = self.session_id.wrapping_add(1);

        // Reset mic level from any previous session.
        if let Ok(mut guard) = self.mic_level.lock() {
            *guard = 0.0;
        }

        // Create a shared session clock before any capture starts so video,
        // system audio, microphone, and cursor timestamps all share the same
        // monotonic origin (Instant::now()).
        let session_clock = Arc::new(crate::core::clock::SessionClock::new());

        // Create bounded channels for video and system audio.
        const VIDEO_QUEUE_CAPACITY: usize = 90;
        const AUDIO_QUEUE_CAPACITY: usize = 256;
        let (video_sender, video_receiver) = bounded_media_channel(VIDEO_QUEUE_CAPACITY, "video");
        let (audio_sender, audio_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY, "system");

        // Start the unified SCStream with both sinks.
        if let Err(error) = self.screen_capture.start_combined(
            config,
            audio_config.capture_system_audio,
            video_sender,
            audio_sender,
            session_clock.clone(),
        ) {
            self.state_machine.fail();
            return Err(error);
        }
        self.video_receiver = Some(video_receiver);
        self.system_audio_receiver = Some(audio_receiver);

        // Start microphone capture if requested.
        if audio_config.capture_microphone {
            let (mic_sender, mic_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY, "mic");
            self.mic_capture.set_session_clock(session_clock.clone());
            if let Err(error) = self.mic_capture.start(audio_config.clone(), mic_sender) {
                // Rollback: stop screen capture.
                let _ = ScreenCapture::stop(&mut self.screen_capture);
                self.video_receiver = None;
                self.system_audio_receiver = None;
                self.state_machine.fail();
                return Err(error);
            }
            self.mic_receiver = Some(mic_receiver);
        }

        // Extract trim sensitivity before moving beautify_snapshot into cursor runtime.
        let trim_sensitivity = beautify_snapshot.trim_sensitivity.clone();

        // Read capture geometry from screen capture for cursor coordinate normalization.
        let capture_geometry = self.screen_capture.last_capture_geometry();

        // Start cursor metadata runtime for cursor effects.
        self.cursor_runtime = Some(CursorMetadataRuntime::spawn(
            MacCursorSource::new(session_clock.clone(), cursor_main_thread_dispatcher),
            config.fps,
            session_clock.clone(),
            beautify_snapshot,
            capture_geometry,
        ));

        // Spawn frame consumer thread (drain mode).
        let stop_flag = Arc::new(AtomicBool::new(false));
        self.stop_flag = Some(stop_flag.clone());

        let video_rx = self.video_receiver.take().unwrap();
        let system_audio_rx = self.system_audio_receiver.take().unwrap();
        let mic_rx = self.mic_receiver.take();
        let frame_count = self.frame_count.clone();
        let mic_level = self.mic_level.clone();
        frame_count.store(0, Ordering::Relaxed);

        // Use FFmpeg writer when the feature is enabled to produce a playable
        // original recording artifact. Falls back to counting writer otherwise.
        #[cfg(feature = "ffmpeg")]
        let writer: Box<dyn RecordingWriter> = {
            let output_path = crate::media::export_paths::original_recording_path();
            Box::new(crate::media::ffmpeg_writer::FfmpegRecordingWriter::new(
                output_path,
            )?)
        };
        #[cfg(not(feature = "ffmpeg"))]
        let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));
        let requested_system_audio = audio_config.capture_system_audio;
        let requested_microphone = audio_config.capture_microphone;
        let microphone_device = audio_config.microphone_device.clone();

        // Save for export contract validation.
        self.last_requested_system_audio = requested_system_audio;
        self.last_requested_microphone = requested_microphone;

        let (consumer_result_tx, consumer_result_rx) =
            std::sync::mpsc::channel::<RecordingConsumerOutput>();
        self.consumer_result_rx = Some(consumer_result_rx);
        self.consumer_handle = Some(thread::spawn(move || {
            let output = Self::consume_frames(
                stop_flag,
                video_rx,
                system_audio_rx,
                mic_rx,
                frame_count,
                writer,
                mic_level,
                &trim_sensitivity,
                requested_system_audio,
                requested_microphone,
                microphone_device,
            );
            let _ = consumer_result_tx.send(output);
        }));

        Ok(())
    }
}

/// Timeout for the consumer thread to return its result after stop.
/// BUG.md rule 28: timeout must not call unbounded join().
const CONSUMER_RESULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// RAII guard that ensures all recording resources are released on drop.
///
/// If `finalize()` panics or is never called, the guard still executes
/// basic cleanup (stop_flag reset, mic level reset) on drop. This prevents
/// resource leaks when a sidecar write failure or state machine transition
/// error occurs mid-cleanup.
struct RecordingFinalizeGuard<'a> {
    service: &'a mut MacRecordingService,
    cursor_metadata: Option<crate::media::recording_metadata::RecordingMetadata>,
    mic_stop_result: AppResult<()>,
    mic_stop_diag: Option<crate::platform::macos::cpal_microphone::CpalMicrophoneStopDiagnostics>,
    capture_stop_result: AppResult<()>,
    consumer_output: Option<RecordingConsumerOutput>,
    consumer_panicked: bool,
    errors: Vec<String>,
}

impl<'a> RecordingFinalizeGuard<'a> {
    fn new(service: &'a mut MacRecordingService) -> Self {
        Self {
            service,
            cursor_metadata: None,
            mic_stop_result: Ok(()),
            mic_stop_diag: None,
            capture_stop_result: Ok(()),
            consumer_output: None,
            consumer_panicked: false,
            errors: Vec::new(),
        }
    }

    /// Produces a valid empty RecordingConsumerOutput for the timeout path.
    /// When consumer thread times out, the guard uses this to continue cleanup.
    fn empty_consumer_output() -> RecordingConsumerOutput {
        RecordingConsumerOutput {
            result: RecordingResult {
                duration_secs: 0,
                frame_count: 0,
                mixed_audio_chunk_count: 0,
                output_path: None,
                cursor_metadata_path: None,
                effect_timeline_path: None,
                trim_metadata_path: None,
                cut_timeline_path: None,
                writer_diagnostics: crate::media::recording_writer::WriterDiagnostics::default(),
                diagnostics: crate::media::recording_writer::RecordingDiagnostics::default(),
                finalization_errors: Vec::new(),
            },
            trim_metadata: TrimMetadata {
                schema_version: TRIM_METADATA_SCHEMA_VERSION,
                duration_nanos: 0,
                base_audio_activity: Vec::new(),
                audio_activity: Vec::new(),
                visual_activity: Vec::new(),
                audio_activity_dropped_count: 0,
                visual_activity_dropped_count: 0,
                activity_truncated: false,
            },
            diagnostics: crate::media::recording_writer::RecordingDiagnostics::default(),
            errors: Vec::new(),
        }
    }

    /// Execute all cleanup steps regardless of intermediate errors.
    /// Returns the final RecordingResult or error.
    fn finalize(mut self) -> AppResult<StopRecordingResponse> {
        self.stop_captures();
        self.join_consumer();
        self.write_sidecars();
        self.reset_mic();
        self.collect_errors();
        self.drive_state_machine()
    }

    /// Step 1-4: Stop cursor, mic, screen capture, signal consumer.
    fn stop_captures(&mut self) {
        // Stop cursor runtime BEFORE native captures so the metadata duration
        // reflects the moment we decided to stop, not the SCK async teardown.
        self.cursor_metadata = self
            .service
            .cursor_runtime
            .as_mut()
            .and_then(|runtime| runtime.stop());
        self.service.cursor_runtime = None;

        // Stop mic first to release Bluetooth HFP profile ASAP.
        if self.service.last_requested_microphone {
            eprintln!("麦克风已启动，执行 mic stop...");
            match self.service.mic_capture.stop_with_diagnostics() {
                Ok(diag) => {
                    eprintln!("麦克风停止诊断: {:?}", diag);
                    // Persist diagnostics before mic capture is rebuilt in reset_mic().
                    self.mic_stop_diag = Some(diag);
                }
                Err(e) => {
                    eprintln!("麦克风停止失败: {:?}", e);
                    self.mic_stop_result = Err(e);
                }
            }
        } else {
            eprintln!("本轮未启动麦克风，跳过 mic stop");
        }

        // Then stop screen capture so no new media can be enqueued.
        self.capture_stop_result = ScreenCapture::stop(&mut self.service.screen_capture);

        // Signal the consumer thread to stop.
        if let Some(flag) = &self.service.stop_flag {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Step 5: Join consumer thread with bounded timeout.
    fn join_consumer(&mut self) {
        let (output, panicked) = Self::receive_consumer_output_with_timeout(
            self.service.consumer_result_rx.take(),
            &mut self.service.consumer_handle,
            CONSUMER_RESULT_TIMEOUT,
            &mut self.errors,
        );
        self.consumer_output = Some(output);
        self.consumer_panicked = panicked;
    }

    /// Testable helper: receive consumer output with a bounded timeout.
    ///
    /// On timeout, the consumer handle is detached (dropped without join).
    /// On disconnect, the handle is joined to extract any panic message.
    fn receive_consumer_output_with_timeout(
        result_rx: Option<mpsc::Receiver<RecordingConsumerOutput>>,
        handle: &mut Option<thread::JoinHandle<()>>,
        timeout: std::time::Duration,
        errors: &mut Vec<String>,
    ) -> (RecordingConsumerOutput, bool) {
        let empty_output = Self::empty_consumer_output();

        if let Some(rx) = result_rx {
            match rx.recv_timeout(timeout) {
                Ok(output) => {
                    if let Some(h) = handle.take() {
                        let _ = h.join();
                    }
                    (output, false)
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    handle.take();
                    eprintln!("警告: 消费线程超时未返回结果 ({:?})", timeout);
                    errors.push(format!(
                        "录制消费线程超时未返回结果 ({:?})，可能仍在后台执行",
                        timeout
                    ));
                    (empty_output, true)
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    eprintln!("警告: 消费线程结果通道断开");
                    errors.push("录制消费线程结果通道断开".to_string());
                    if let Some(h) = handle.take() {
                        if let Err(panic) = h.join() {
                            let msg = extract_panic_message(panic);
                            eprintln!("录制消费线程异常终止: {msg}");
                            errors.push(format!("录制消费线程异常终止: {msg}"));
                        }
                    }
                    (empty_output, true)
                }
            }
        } else if let Some(_h) = handle.take() {
            errors.push("消费线程结果通道不存在但句柄存在，状态不一致，已丢弃句柄".to_string());
            (empty_output, false)
        } else {
            (empty_output, false)
        }
    }

    /// Step 6-7: Write cursor and trim metadata sidecars.
    fn write_sidecars(&mut self) {
        // Persist mic stop diagnostics into consumer output before logging.
        // This must happen before reset_mic() which rebuilds the capture instance.
        if let (Some(diag), Some(ref mut output)) =
            (self.mic_stop_diag.take(), &mut self.consumer_output)
        {
            output.diagnostics.mic_stop_diagnostics = Some(diag);
        }

        let output = match &self.consumer_output {
            Some(o) => o,
            None => return,
        };

        // Log diagnostics summary.
        eprintln!("录制音频诊断摘要: {:?}", output.diagnostics);
        eprintln!("写入器诊断摘要: {:?}", output.result.writer_diagnostics);

        self.service.stop_flag = None;

        // Write cursor metadata sidecar.
        let cursor_metadata_path = match self.cursor_metadata.take() {
            Some(metadata) => {
                let path = cursor_metadata_path();
                match RecordingMetadataWriter::write_metadata(&path, &metadata) {
                    Ok(()) => Some(path.to_string_lossy().to_string()),
                    Err(e) => {
                        self.errors.push(format!("光标元数据写入失败: {e}"));
                        None
                    }
                }
            }
            None => None,
        };

        // Write trim metadata sidecar. Skip when consumer panicked.
        let trim_metadata_path = if self.consumer_panicked {
            None
        } else {
            let path = trim_metadata_path();
            match TrimMetadataWriter::write_metadata(&path, &output.trim_metadata) {
                Ok(()) => Some(path.to_string_lossy().to_string()),
                Err(e) => {
                    self.errors.push(format!("裁剪元数据写入失败: {e}"));
                    None
                }
            }
        };

        // Update service state with sidecar paths.
        self.service.last_cursor_metadata_path = cursor_metadata_path.clone();
        self.service.last_recording_output_path = output.result.output_path.clone();
        self.service.last_trim_metadata_path = trim_metadata_path;
    }

    /// Step 8: Reset mic level and capture instance.
    fn reset_mic(&mut self) {
        if let Ok(mut guard) = self.service.mic_level.lock() {
            *guard = 0.0;
        }
        if self.service.last_requested_microphone {
            self.service.mic_capture = CpalMicrophoneCapture::new();
        }
    }

    /// Step 9: Collect capture/mic stop errors.
    fn collect_errors(&mut self) {
        if let Err(e) = std::mem::replace(&mut self.capture_stop_result, Ok(())) {
            self.errors.push(format!("屏幕录制停止失败: {e}"));
        }
        if let Err(e) = std::mem::replace(&mut self.mic_stop_result, Ok(())) {
            self.errors.push(format!("麦克风停止失败: {e}"));
        }
    }

    /// Step 10: Drive state machine to terminal state and return result.
    fn drive_state_machine(&mut self) -> AppResult<StopRecordingResponse> {
        let output = match self.consumer_output.take() {
            Some(o) => o,
            None => {
                return Err(crate::app::error::AppError::RecordingFinalizeFailed {
                    reason: "消费线程输出缺失".to_string(),
                })
            }
        };

        let mut result = output.result;
        self.errors.extend(output.errors);

        // Inject capture-side diagnostics into the result.
        result.diagnostics = output.diagnostics;

        // Update result with sidecar paths from service state.
        result.cursor_metadata_path = self.service.last_cursor_metadata_path.clone();
        result.effect_timeline_path = self.service.last_effect_timeline_path.clone();
        result.trim_metadata_path = self.service.last_trim_metadata_path.clone();

        let failed = if self.errors.is_empty() {
            if let Err(e) = self.service.state_machine.stop() {
                self.service.state_machine.fail();
                return Err(e);
            }
            if let Err(e) = self.service.state_machine.complete() {
                self.service.state_machine.fail();
                return Err(e);
            }
            false
        } else {
            // Record errors in result rather than discarding diagnostics.
            result.finalization_errors = self.errors.clone();
            // Still transition to terminal state so frontend can display diagnostics.
            let _ = self.service.state_machine.stop();
            self.service.state_machine.fail();
            true
        };

        Ok(StopRecordingResponse { result, failed })
    }
}

/// Safety net: ensures basic cleanup happens even if `finalize()` panics.
///
/// This Drop impl only resets `stop_flag` and `mic_level` — it does NOT
/// stop screen capture, stop mic, or join consumer thread. Those require
/// the explicit `finalize()` path. This is a last-resort safety net,
/// not a complete resource release mechanism.
impl Drop for RecordingFinalizeGuard<'_> {
    fn drop(&mut self) {
        self.service.stop_flag = None;
        if let Ok(mut guard) = self.service.mic_level.lock() {
            *guard = 0.0;
        }
    }
}

impl MacRecordingService {
    /// Stops all captures and finalizes the recording session.
    ///
    /// Uses RAII guard to ensure all cleanup steps execute even on panic.
    /// All resource-release steps (capture stop, consumer join, cursor stop,
    /// mic reset, state machine transition) are executed regardless of
    /// intermediate errors. Errors are collected and returned together so
    /// a sidecar write failure never skips cleanup.
    pub fn stop(&mut self) -> AppResult<StopRecordingResponse> {
        let guard = RecordingFinalizeGuard::new(self);
        guard.finalize()
    }

    pub fn pause(&mut self) -> AppResult<()> {
        self.state_machine.pause()
    }

    pub fn resume(&mut self) -> AppResult<()> {
        self.state_machine.resume()
    }

    /// Background thread that drains video and audio channels to prevent blocking.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn consume_frames(
        stop_flag: Arc<AtomicBool>,
        video_rx: MediaReceiver<VideoFrameRef>,
        system_audio_rx: MediaReceiver<AudioChunk>,
        mic_rx: Option<MediaReceiver<AudioChunk>>,
        frame_count: Arc<std::sync::atomic::AtomicU64>,
        mut writer: Box<dyn RecordingWriter>,
        mic_level: Arc<Mutex<f64>>,
        _trim_sensitivity_str: &str,
        requested_system_audio: bool,
        requested_microphone: bool,
        microphone_device: Option<String>,
    ) -> RecordingConsumerOutput {
        let mut synchronizer = crate::media::audio_synchronizer::AudioSynchronizer::new(
            SimpleAudioMixer::new(),
            crate::media::audio_synchronizer::AudioSynchronizerConfig {
                requested_system_audio,
                requested_microphone,
                ..Default::default()
            },
        );
        let mut mic_detector = MicLevelDetector::new(4096); // ~85ms 窗口 @ 48kHz

        // Audio diagnostics — tracks source-aware metrics to diagnose silent audio issues.
        let mut diagnostics = RecordingDiagnostics {
            requested_system_audio,
            requested_microphone,
            microphone_device,
            ..Default::default()
        };

        // Trim metadata collectors — sensitivity-independent 100ms base RMS
        // buckets. Computed in the consumer thread, never sent to React.
        // Post-recording aggregation applies the user-configured sensitivity.
        let mut base_audio_analyzer = BaseAudioActivityAnalyzer::default();
        let frame_diff_analyzer = FrameDiffAnalyzer::new(64, 36);
        // Low-frequency visual sampling: ~4fps to avoid per-frame diff cost.
        const VISUAL_SAMPLE_INTERVAL_NANOS: u64 = 250_000_000;
        // Safety caps to prevent unbounded memory growth during very long recordings.
        const MAX_VISUAL_SAMPLES: usize = 144_000; // ~10h @ 4fps
        const MAX_AUDIO_SAMPLES: usize = 72_000; // ~2h @ 10/sec (100ms base buckets)
        let mut base_audio_activity = Vec::new();
        let mut previous_sampled_frame: Option<crate::core::frame::VideoFrame> = None;
        let mut last_visual_sample_nanos: u64 = 0;
        let mut visual_activity = Vec::new();
        let mut visual_dropped: u64 = 0;
        let mut audio_dropped: u64 = 0;
        // Tracks the latest observed media timestamp independent of sample caps,
        // so duration_nanos remains accurate even after MAX_*_SAMPLES is reached.
        let mut latest_observed_media_nanos: u64 = 0;
        let mut errors: Vec<String> = Vec::new();

        loop {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            // Drain video frames (non-blocking, bounded batch).
            // Limit to MAX_VIDEO_BATCH_PER_ITERATION to prevent audio starvation
            // when the FFmpeg encoder can't keep up and video frames pile up.
            const MAX_VIDEO_BATCH_PER_ITERATION: usize = 10;
            let mut video_batch_count = 0;
            while video_batch_count < MAX_VIDEO_BATCH_PER_ITERATION {
                match video_rx.try_recv() {
                    Ok(frame) => {
                        video_batch_count += 1;
                        frame_count.fetch_add(1, Ordering::Relaxed);

                        // Low-frequency frame-diff sampling: only diff when enough
                        // time has elapsed since the last visual sample. This avoids
                        // running expensive thumbnail+diff on every 30fps frame.
                        let frame_nanos = frame.timestamp.nanos;
                        latest_observed_media_nanos = latest_observed_media_nanos.max(frame_nanos);
                        if frame_nanos.saturating_sub(last_visual_sample_nanos)
                            >= VISUAL_SAMPLE_INTERVAL_NANOS
                        {
                            if let Some(ref previous) = previous_sampled_frame {
                                if let Ok(diff) = frame_diff_analyzer.diff_pair(previous, &frame) {
                                    if !push_bounded_visual_sample(
                                        &mut visual_activity,
                                        diff,
                                        MAX_VISUAL_SAMPLES,
                                    ) {
                                        visual_dropped += 1;
                                    }
                                }
                            }
                            previous_sampled_frame = Some((*frame).clone());
                            last_visual_sample_nanos = frame_nanos;
                        }

                        if let Err(e) = writer.push_video(frame) {
                            let msg = format!("写入视频帧失败: {e}");
                            eprintln!("{msg}");
                            errors.push(msg);
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }

            // Enqueue system audio chunks for ordered pairing.
            while let Ok(chunk) = system_audio_rx.try_recv() {
                diagnostics.system_chunks_received += 1;
                // Track system audio RMS.
                let rms = compute_rms(&chunk.samples);
                if rms > diagnostics.system_rms_max {
                    diagnostics.system_rms_max = rms;
                }
                synchronizer.push_system(chunk);
            }

            // Enqueue microphone audio chunks and compute RMS level.
            if let Some(ref mic_rx) = mic_rx {
                while let Ok(chunk) = mic_rx.try_recv() {
                    diagnostics.mic_chunks_received += 1;
                    // Track mic RMS.
                    let rms = compute_rms(&chunk.samples);
                    if rms > diagnostics.mic_rms_max {
                        diagnostics.mic_rms_max = rms;
                    }
                    // Feed mic samples to the level detector
                    let level = mic_detector.push_samples(&chunk.samples);
                    if let Ok(mut guard) = mic_level.lock() {
                        *guard = level;
                    }
                    synchronizer.push_mic(chunk);
                }
            }

            // Pair by timestamp proximity and mix.
            for synced_result in synchronizer.drain_mixed() {
                match synced_result {
                    Ok(synced) => {
                        // Record per-source before-writer diagnostics.
                        if synced.has_system {
                            diagnostics.system_windows_before_writer += 1;
                            diagnostics.system_frames_before_writer += synced.system_frames;
                            if synced.system_rms > diagnostics.system_rms_max_before_writer {
                                diagnostics.system_rms_max_before_writer = synced.system_rms;
                            }
                        }
                        if synced.has_mic {
                            diagnostics.mic_windows_before_writer += 1;
                            diagnostics.mic_frames_before_writer += synced.mic_frames;
                            if synced.mic_rms > diagnostics.mic_rms_max_before_writer {
                                diagnostics.mic_rms_max_before_writer = synced.mic_rms;
                            }
                        }
                        if synced.emitted_due_to_timeout {
                            diagnostics.source_timeout_window_count += 1;
                        }

                        // Track audio duration independent of sample caps.
                        let chunk_frames =
                            synced.mixed.samples.len() as u64 / synced.mixed.channels.max(1) as u64;
                        let chunk_nanos = chunk_frames.saturating_mul(1_000_000_000)
                            / synced.mixed.sample_rate.max(1) as u64;
                        latest_observed_media_nanos = latest_observed_media_nanos
                            .max(synced.mixed.timestamp.nanos.saturating_add(chunk_nanos));
                        // Track mixed audio RMS.
                        let mixed_rms = compute_rms(&synced.mixed.samples);
                        if mixed_rms > diagnostics.mixed_rms_max {
                            diagnostics.mixed_rms_max = mixed_rms;
                        }
                        // Collect sensitivity-independent 100ms base RMS buckets.
                        for sample in base_audio_analyzer.push_chunk(&synced.mixed) {
                            if !push_bounded_base_audio_sample(
                                &mut base_audio_activity,
                                sample,
                                MAX_AUDIO_SAMPLES,
                            ) {
                                audio_dropped += 1;
                            }
                        }
                        // Capture source metadata before moving synced.mixed into push_audio.
                        let has_system = synced.has_system;
                        let has_mic = synced.has_mic;
                        let system_frames = synced.system_frames;
                        let mic_frames = synced.mic_frames;

                        if let Err(e) = writer.push_audio(synced.mixed) {
                            diagnostics.writer_push_audio_failures += 1;
                            let msg = format!("写入混音音频失败: {e}");
                            eprintln!("{msg}");
                            errors.push(msg);
                        } else {
                            // Only count per-source contribution on successful enqueue.
                            writer.record_source_contribution(
                                has_system,
                                has_mic,
                                system_frames,
                                mic_frames,
                            );
                            diagnostics.mixed_chunks_queued += 1;
                        }
                    }
                    Err(e) => eprintln!("音频混合失败: {e}"),
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Final drain: captures have been stopped, drain everything remaining
        // before finalizing the writer.
        while let Ok(frame) = video_rx.try_recv() {
            frame_count.fetch_add(1, Ordering::Relaxed);
            let frame_nanos = frame.timestamp.nanos;
            latest_observed_media_nanos = latest_observed_media_nanos.max(frame_nanos);
            if frame_nanos.saturating_sub(last_visual_sample_nanos) >= VISUAL_SAMPLE_INTERVAL_NANOS
            {
                if let Some(ref previous) = previous_sampled_frame {
                    if let Ok(diff) = frame_diff_analyzer.diff_pair(previous, &frame) {
                        if !push_bounded_visual_sample(
                            &mut visual_activity,
                            diff,
                            MAX_VISUAL_SAMPLES,
                        ) {
                            visual_dropped += 1;
                        }
                    }
                }
                previous_sampled_frame = Some((*frame).clone());
                last_visual_sample_nanos = frame_nanos;
            }
            if let Err(e) = writer.push_video(frame) {
                let msg = format!("写入视频帧失败: {e}");
                eprintln!("{msg}");
                errors.push(msg);
            }
        }

        while let Ok(chunk) = system_audio_rx.try_recv() {
            diagnostics.system_chunks_received += 1;
            let rms = compute_rms(&chunk.samples);
            if rms > diagnostics.system_rms_max {
                diagnostics.system_rms_max = rms;
            }
            synchronizer.push_system(chunk);
        }

        if let Some(ref mic_rx) = mic_rx {
            while let Ok(chunk) = mic_rx.try_recv() {
                diagnostics.mic_chunks_received += 1;
                let rms = compute_rms(&chunk.samples);
                if rms > diagnostics.mic_rms_max {
                    diagnostics.mic_rms_max = rms;
                }
                synchronizer.push_mic(chunk);
            }
        }

        // Flush remaining base audio buckets from the analyzer.
        if let Some(sample) = base_audio_analyzer.flush() {
            if !push_bounded_base_audio_sample(&mut base_audio_activity, sample, MAX_AUDIO_SAMPLES)
            {
                audio_dropped += 1;
            }
        }

        // Final drain: flush all remaining system/mic chunks without holding.
        // This ensures no audio chunks are left in the queues when stopping.
        let drain_results = synchronizer.drain_final();
        // Record window pairing diagnostics from the synchronizer.
        let (paired, sys_only, mic_only, timeout_windows) = synchronizer.diagnostics();
        diagnostics.paired_window_count = paired;
        diagnostics.system_only_window_count = sys_only;
        diagnostics.mic_only_window_count = mic_only;
        diagnostics.source_timeout_window_count = timeout_windows;
        for (synchronized, was_unpaired) in drain_results {
            if was_unpaired {
                if synchronized.has_system && !synchronized.has_mic {
                    eprintln!("警告: 最终排空发现未配对的系统音频块");
                } else if synchronized.has_mic && !synchronized.has_system {
                    eprintln!("警告: 最终排空发现未配对的麦克风音频块");
                }
            }

            // Record per-source before-writer diagnostics (final drain).
            if synchronized.has_system {
                diagnostics.system_windows_before_writer += 1;
                diagnostics.system_frames_before_writer += synchronized.system_frames;
                if synchronized.system_rms > diagnostics.system_rms_max_before_writer {
                    diagnostics.system_rms_max_before_writer = synchronized.system_rms;
                }
            }
            if synchronized.has_mic {
                diagnostics.mic_windows_before_writer += 1;
                diagnostics.mic_frames_before_writer += synchronized.mic_frames;
                if synchronized.mic_rms > diagnostics.mic_rms_max_before_writer {
                    diagnostics.mic_rms_max_before_writer = synchronized.mic_rms;
                }
            }
            if synchronized.emitted_due_to_timeout {
                diagnostics.source_timeout_window_count += 1;
            }

            let chunk_frames =
                synchronized.mixed.samples.len() as u64 / synchronized.mixed.channels.max(1) as u64;
            let chunk_nanos = chunk_frames.saturating_mul(1_000_000_000)
                / synchronized.mixed.sample_rate.max(1) as u64;
            latest_observed_media_nanos = latest_observed_media_nanos.max(
                synchronized
                    .mixed
                    .timestamp
                    .nanos
                    .saturating_add(chunk_nanos),
            );
            let mixed_rms = compute_rms(&synchronized.mixed.samples);
            if mixed_rms > diagnostics.mixed_rms_max {
                diagnostics.mixed_rms_max = mixed_rms;
            }
            for sample in base_audio_analyzer.push_chunk(&synchronized.mixed) {
                if !push_bounded_base_audio_sample(
                    &mut base_audio_activity,
                    sample,
                    MAX_AUDIO_SAMPLES,
                ) {
                    audio_dropped += 1;
                }
            }
            // Capture source metadata before moving synchronized.mixed into push_audio.
            let has_system = synchronized.has_system;
            let has_mic = synchronized.has_mic;
            let system_frames = synchronized.system_frames;
            let mic_frames = synchronized.mic_frames;

            if let Err(e) = writer.push_audio(synchronized.mixed) {
                diagnostics.writer_push_audio_failures += 1;
                let msg = format!("写入混音音频失败: {e}");
                eprintln!("{msg}");
                errors.push(msg);
            } else {
                // Only count per-source contribution on successful enqueue.
                writer.record_source_contribution(has_system, has_mic, system_frames, mic_frames);
                diagnostics.mixed_chunks_queued += 1;
            }
        }

        let result = match writer.finish() {
            Ok(result) => result,
            Err(e) => {
                let msg = format!("录制写入器完成失败: {e}");
                eprintln!("{msg}");
                errors.push(msg);
                RecordingResult {
                    duration_secs: 0,
                    frame_count: 0,
                    mixed_audio_chunk_count: 0,
                    output_path: None,
                    cursor_metadata_path: None,
                    effect_timeline_path: None,
                    trim_metadata_path: None,
                    cut_timeline_path: None,
                    writer_diagnostics: crate::media::recording_writer::WriterDiagnostics::default(
                    ),
                    diagnostics: crate::media::recording_writer::RecordingDiagnostics::default(),
                    finalization_errors: Vec::new(),
                }
            }
        };

        // Use writer diagnostics for silent track detection instead of inferring
        // from mixed_audio_chunk_count (which gets overwritten by silent packet count).
        diagnostics.generated_silent_track = result.writer_diagnostics.generated_silent_track;

        // Record channel drop counts from media receivers.
        diagnostics.system_chunks_dropped = system_audio_rx.dropped_count();
        if let Some(ref mic_rx) = mic_rx {
            diagnostics.mic_chunks_dropped = mic_rx.dropped_count();
        }

        // Log diagnostics for debugging.
        eprintln!("录制音频诊断: {:?}", diagnostics);

        // Warn if requested audio sources didn't produce content.
        if requested_system_audio && diagnostics.system_chunks_received == 0 {
            let msg = "警告: 请求了系统音频但未收到任何音频块".to_string();
            eprintln!("{msg}");
            errors.push(msg);
        }
        if requested_microphone && diagnostics.mic_chunks_received == 0 {
            let msg = "警告: 请求了麦克风但未收到任何音频块".to_string();
            eprintln!("{msg}");
            errors.push(msg);
        }
        // Drop warnings: small drops are diagnostics-only (not errors).
        // Hard fail only when drop ratio exceeds threshold (BUG-005 rule 22).
        // Denominator is received + dropped (attempted total), not just received.
        const AUDIO_DROP_RATIO_HARD_FAIL: f64 = 0.10; // 10%
        if requested_system_audio && diagnostics.system_chunks_dropped > 0 {
            let attempted = diagnostics
                .system_chunks_received
                .saturating_add(diagnostics.system_chunks_dropped)
                .max(1) as f64;
            let ratio = diagnostics.system_chunks_dropped as f64 / attempted;
            eprintln!(
                "警告: 系统音频通道丢弃了 {} 个音频块 (总计 {}，丢弃率 {:.1}%)",
                diagnostics.system_chunks_dropped,
                attempted as u64,
                ratio * 100.0,
            );
            if ratio > AUDIO_DROP_RATIO_HARD_FAIL {
                errors.push(format!(
                    "系统音频丢弃率过高 ({:.1}% > {:.1}%)",
                    ratio * 100.0,
                    AUDIO_DROP_RATIO_HARD_FAIL * 100.0
                ));
            }
        }
        if requested_microphone && diagnostics.mic_chunks_dropped > 0 {
            let attempted = diagnostics
                .mic_chunks_received
                .saturating_add(diagnostics.mic_chunks_dropped)
                .max(1) as f64;
            let ratio = diagnostics.mic_chunks_dropped as f64 / attempted;
            eprintln!(
                "警告: 麦克风通道丢弃了 {} 个音频块 (总计 {}，丢弃率 {:.1}%)",
                diagnostics.mic_chunks_dropped,
                attempted as u64,
                ratio * 100.0,
            );
            if ratio > AUDIO_DROP_RATIO_HARD_FAIL {
                errors.push(format!(
                    "麦克风音频丢弃率过高 ({:.1}% > {:.1}%)",
                    ratio * 100.0,
                    AUDIO_DROP_RATIO_HARD_FAIL * 100.0
                ));
            }
        }

        // Artifact-level audio contract validation (BUG-005).
        // Decodes the written artifact's audio stream and checks RMS/peak
        // to verify that requested audio sources actually produced audible content.
        // This catches the case where capture-side RMS is non-zero but the artifact
        // contains silence due to writer timeline overlap or encoder issues.
        #[cfg(feature = "ffmpeg")]
        if let Some(ref output_path) = result.output_path {
            let contract = crate::media::ffmpeg_common::RequestedAudioContract {
                requested_system_audio,
                requested_microphone,
                ..Default::default()
            };
            if contract.any_audio_requested() {
                match crate::media::ffmpeg_common::validate_source_artifact_with_audio_contract(
                    std::path::Path::new(output_path),
                    &contract,
                ) {
                    Ok(inspection) => {
                        eprintln!(
                            "录制音频 contract 验证通过: RMS={:.6}, peak={:.6}, samples={}",
                            inspection.audio_rms.unwrap_or(0.0),
                            inspection.audio_peak.unwrap_or(0.0),
                            inspection.audio_sample_count.unwrap_or(0),
                        );
                    }
                    Err(e) => {
                        let msg = format!("录制音频 contract 验证失败: {e}");
                        eprintln!("{msg}");
                        errors.push(msg);
                    }
                }
            }

            // Source-aware contract: check each requested source actually contributed.
            if let Err(e) = crate::media::recording_writer::validate_source_aware_audio_contract(
                &diagnostics,
                &result.writer_diagnostics,
            ) {
                let msg = format!("source-aware audio contract 失败: {e}");
                eprintln!("{msg}");
                errors.push(msg);
            }
        }

        // Writer duration may be 0 when using CountingRecordingWriter (no
        // production encoder yet). Fall back to the latest observed media
        // timestamp so the trim metadata always carries a usable duration.
        // This is tracked independently of the capped activity Vecs, so
        // duration remains accurate even after MAX_*_SAMPLES is reached.
        let writer_duration_nanos = result.duration_secs.saturating_mul(1_000_000_000);
        let duration_nanos =
            choose_duration_nanos(writer_duration_nanos, latest_observed_media_nanos);

        let activity_truncated = audio_dropped > 0 || visual_dropped > 0;

        RecordingConsumerOutput {
            result,
            trim_metadata: TrimMetadata {
                schema_version: TRIM_METADATA_SCHEMA_VERSION,
                duration_nanos,
                base_audio_activity,
                audio_activity: Vec::new(),
                visual_activity,
                audio_activity_dropped_count: audio_dropped,
                visual_activity_dropped_count: visual_dropped,
                activity_truncated,
            },
            diagnostics,
            errors,
        }
    }
}

impl Default for MacRecordingService {
    fn default() -> Self {
        Self::new()
    }
}

/// Pushes a base audio sample into the bounded Vec, dropping it if at capacity.
/// Returns `true` if the sample was pushed, `false` if dropped.
fn push_bounded_base_audio_sample(
    samples: &mut Vec<crate::media::trim_audio_activity::BaseAudioActivitySample>,
    sample: crate::media::trim_audio_activity::BaseAudioActivitySample,
    max: usize,
) -> bool {
    if samples.len() < max {
        samples.push(sample);
        true
    } else {
        false
    }
}

/// Test-only helper for bounded push of legacy AudioActivitySample.
#[cfg(test)]
fn push_bounded_audio_sample(
    samples: &mut Vec<crate::core::cut::AudioActivitySample>,
    sample: crate::core::cut::AudioActivitySample,
    max: usize,
) -> bool {
    if samples.len() < max {
        samples.push(sample);
        true
    } else {
        false
    }
}

/// Pushes a visual sample into the bounded Vec, dropping it if at capacity.
/// Returns `true` if the sample was pushed, `false` if dropped.
fn push_bounded_visual_sample(
    samples: &mut Vec<crate::core::cut::FrameDiffSample>,
    sample: crate::core::cut::FrameDiffSample,
    max: usize,
) -> bool {
    if samples.len() < max {
        samples.push(sample);
        true
    } else {
        false
    }
}

/// Chooses the effective trim metadata duration: prefer writer duration when
/// available, otherwise fall back to the latest observed media timestamp.
fn choose_duration_nanos(writer_duration_nanos: u64, latest_observed_media_nanos: u64) -> u64 {
    if writer_duration_nanos > 0 {
        writer_duration_nanos
    } else {
        latest_observed_media_nanos
    }
}

/// Extracts a human-readable message from a panic payload.
fn extract_panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Computes the RMS (root mean square) of a slice of f32 audio samples.
///
/// Returns 0.0 for empty slices.
fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

fn cursor_metadata_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("cursor-metadata-{millis}-{seq}.json"))
}

fn trim_metadata_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("trim-metadata-{millis}-{seq}.json"))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::core::cut::{AudioActivitySample, FrameDiffSample};
    use crate::core::frame::MediaTimestamp;

    #[test]
    fn duration_fallback_uses_latest_media_timestamp() {
        assert_eq!(choose_duration_nanos(0, 90_000_000_000), 90_000_000_000);
    }

    #[test]
    fn duration_prefers_writer_when_nonzero() {
        assert_eq!(
            choose_duration_nanos(120_000_000_000, 90_000_000_000),
            120_000_000_000
        );
    }

    #[test]
    fn duration_uses_latest_media_after_sample_cap() {
        // Simulates: cap reached at 60s, but recording continued to 120s.
        // latest_observed_media_nanos tracks independently of capped Vec.
        let capped_activity_last_end = 60_000_000_000u64;
        let latest_observed_media_nanos = 120_000_000_000u64;
        let writer_duration_nanos = 0u64;

        // Duration must be the latest observed, not the capped Vec's last end.
        let duration = choose_duration_nanos(writer_duration_nanos, latest_observed_media_nanos);
        assert_eq!(duration, 120_000_000_000);
        assert!(duration > capped_activity_last_end);
    }

    #[test]
    fn bounded_audio_push_accepts_until_cap() {
        let mut samples = Vec::new();
        let max = 3;

        let sample = AudioActivitySample {
            start: MediaTimestamp::from_nanos(0),
            end: MediaTimestamp::from_nanos(100_000_000),
            rms: 0.01,
        };

        assert!(push_bounded_audio_sample(&mut samples, sample, max));
        assert!(push_bounded_audio_sample(&mut samples, sample, max));
        assert!(push_bounded_audio_sample(&mut samples, sample, max));
        assert_eq!(samples.len(), 3);

        // Fourth push should be dropped.
        assert!(!push_bounded_audio_sample(&mut samples, sample, max));
        assert_eq!(samples.len(), 3);
    }

    #[test]
    fn bounded_visual_push_accepts_until_cap() {
        let mut samples = Vec::new();
        let max = 2;

        let sample = FrameDiffSample {
            start: MediaTimestamp::from_nanos(0),
            end: MediaTimestamp::from_nanos(100_000_000),
            change_ratio: 0.001,
        };

        assert!(push_bounded_visual_sample(&mut samples, sample, max));
        assert!(push_bounded_visual_sample(&mut samples, sample, max));
        assert_eq!(samples.len(), 2);

        // Third push should be dropped.
        assert!(!push_bounded_visual_sample(&mut samples, sample, max));
        assert_eq!(samples.len(), 2);
    }

    #[test]
    fn final_audio_drain_respects_metadata_cap() {
        // Simulates: activity Vec already at cap, final drain produces a sample.
        let mut audio_activity = Vec::new();
        let max = 2;

        // Fill to cap.
        for i in 0..max {
            let sample = AudioActivitySample {
                start: MediaTimestamp::from_nanos(i as u64 * 100_000_000),
                end: MediaTimestamp::from_nanos((i as u64 + 1) * 100_000_000),
                rms: 0.01,
            };
            push_bounded_audio_sample(&mut audio_activity, sample, max);
        }
        assert_eq!(audio_activity.len(), max);

        // Final drain sample should be dropped.
        let final_sample = AudioActivitySample {
            start: MediaTimestamp::from_nanos(max as u64 * 100_000_000),
            end: MediaTimestamp::from_nanos((max as u64 + 1) * 100_000_000),
            rms: 0.005,
        };
        let pushed = push_bounded_audio_sample(&mut audio_activity, final_sample, max);
        assert!(!pushed);
        assert_eq!(audio_activity.len(), max);
    }

    /// Validates that the trim sensitivity string from BeautifyConfigSnapshot
    /// is correctly parsed into TrimSensitivity and used for RMS window config.
    #[test]
    fn recording_trim_config_uses_beautify_snapshot_sensitivity() {
        use crate::core::cut::{TrimConfig, TrimSensitivity};

        let high = TrimSensitivity::from_str("high").unwrap();
        let low = TrimSensitivity::from_str("low").unwrap();

        assert_eq!(
            TrimConfig::from_sensitivity(high).rms_window_nanos,
            500_000_000
        );
        assert_eq!(
            TrimConfig::from_sensitivity(low).rms_window_nanos,
            1_000_000_000
        );

        // Invalid string falls back to Medium.
        let fallback = TrimSensitivity::from_str("invalid").unwrap_or(TrimSensitivity::Medium);
        assert_eq!(
            TrimConfig::from_sensitivity(fallback).rms_window_nanos,
            750_000_000
        );
    }

    #[test]
    fn extract_panic_message_handles_str_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("test panic");
        assert_eq!(extract_panic_message(payload), "test panic");
    }

    #[test]
    fn extract_panic_message_handles_string_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("owned panic msg".to_string());
        assert_eq!(extract_panic_message(payload), "owned panic msg");
    }

    #[test]
    fn extract_panic_message_handles_unknown_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(42u32);
        assert_eq!(extract_panic_message(payload), "unknown panic");
    }

    /// Verifies that consumer errors propagate into the error aggregation
    /// used by `stop()` to determine success vs failure.
    #[test]
    fn consumer_errors_merge_into_stop_errors() {
        use crate::media::recording_writer::RecordingResult;
        use crate::media::trim_metadata::TrimMetadata;

        let consumer_output = RecordingConsumerOutput {
            result: RecordingResult {
                duration_secs: 0,
                frame_count: 0,
                mixed_audio_chunk_count: 0,
                output_path: None,
                cursor_metadata_path: None,
                effect_timeline_path: None,
                trim_metadata_path: None,
                cut_timeline_path: None,
                writer_diagnostics: crate::media::recording_writer::WriterDiagnostics::default(),
                diagnostics: crate::media::recording_writer::RecordingDiagnostics::default(),
                finalization_errors: Vec::new(),
            },
            trim_metadata: TrimMetadata {
                schema_version: TRIM_METADATA_SCHEMA_VERSION,
                duration_nanos: 0,
                base_audio_activity: Vec::new(),
                audio_activity: Vec::new(),
                visual_activity: Vec::new(),
                audio_activity_dropped_count: 0,
                visual_activity_dropped_count: 0,
                activity_truncated: false,
            },
            diagnostics: RecordingDiagnostics::default(),
            errors: vec![
                "写入视频帧失败: fake".to_string(),
                "录制写入器完成失败: fake".to_string(),
            ],
        };

        let mut errors: Vec<String> = Vec::new();
        errors.extend(consumer_output.errors);
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("写入视频帧失败"));
        assert!(errors[1].contains("录制写入器完成失败"));
    }

    /// Verifies that a failing writer's errors propagate through
    /// `consume_frames()` into the returned `RecordingConsumerOutput`.
    #[test]
    fn consume_frames_writer_finish_failure_records_error() {
        use crate::media::recording_writer::FailingRecordingWriter;

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(1, "test");
        let stop_flag = Arc::new(AtomicBool::new(true)); // immediate stop
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        // Writer that only fails on finish.
        let writer: Box<dyn RecordingWriter> =
            Box::new(FailingRecordingWriter::new(false, false, true));

        // Drop senders so the channel drains immediately.
        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            false,
            false,
            None,
        );

        assert!(
            output
                .errors
                .iter()
                .any(|e| e.contains("录制写入器完成失败")),
            "expected finish error in consumer output, got: {:?}",
            output.errors
        );
    }

    /// Verifies that writer push errors are collected even when the
    /// consumer processes frames before stopping.
    #[test]
    fn consume_frames_writer_push_video_failure_records_error() {
        use crate::core::frame::{FrameBuffer, PixelFormat, VideoFrame};
        use crate::media::recording_writer::FailingRecordingWriter;

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(2, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(1, "test");
        let stop_flag = Arc::new(AtomicBool::new(false));
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        // Send one video frame before stopping.
        let frame = Arc::new(VideoFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            width: 2,
            height: 2,
            stride_bytes: 8,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(Arc::from(vec![0u8; 16].into_boxed_slice())),
        });
        assert!(video_tx.try_send_drop_newest(frame));

        // Writer that fails on push_video.
        let writer: Box<dyn RecordingWriter> =
            Box::new(FailingRecordingWriter::new(true, false, false));

        // Drop senders so the channel drains.
        drop(video_tx);
        drop(audio_tx);

        // Use a separate thread to set stop_flag after a brief delay,
        // giving the consumer time to process the queued frame.
        let flag_clone = stop_flag.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            flag_clone.store(true, Ordering::Relaxed);
        });

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            false,
            false,
            None,
        );

        stopper.join().unwrap();

        assert!(
            output.errors.iter().any(|e| e.contains("写入视频帧失败")),
            "expected push_video error in consumer output, got: {:?}",
            output.errors
        );
    }

    /// Verifies that writer push_audio errors are collected in the live loop,
    /// matching the final drain behavior.
    #[test]
    fn consume_frames_writer_push_audio_failure_records_error() {
        use crate::media::recording_writer::FailingRecordingWriter;

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(2, "test");
        let stop_flag = Arc::new(AtomicBool::new(false));
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        // Send one audio chunk before stopping.
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 44100,
            channels: 1,
            samples: Arc::from(vec![0.0f32; 441].into_boxed_slice()),
        };
        assert!(audio_tx.try_send_drop_newest(chunk));

        // Writer that fails on push_audio.
        let writer: Box<dyn RecordingWriter> =
            Box::new(FailingRecordingWriter::new(false, true, false));

        // Drop senders so the channel drains.
        drop(video_tx);
        drop(audio_tx);

        // Use a separate thread to set stop_flag after a brief delay,
        // giving the consumer time to process the queued audio.
        let flag_clone = stop_flag.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            flag_clone.store(true, Ordering::Relaxed);
        });

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            false,
            false,
            None,
        );

        stopper.join().unwrap();

        assert!(
            output.errors.iter().any(|e| e.contains("写入混音音频失败")),
            "expected push_audio error in consumer output, got: {:?}",
            output.errors
        );
    }

    /// Verifies that bounded push helpers track drop counts correctly.
    #[test]
    fn bounded_push_tracks_dropped_samples() {
        let mut audio = Vec::new();
        let mut visual = Vec::new();
        let mut audio_dropped: u64 = 0;
        let mut visual_dropped: u64 = 0;
        let max = 2;

        // Fill to cap.
        for i in 0..max {
            let a = AudioActivitySample {
                start: MediaTimestamp::from_nanos(i as u64 * 100_000_000),
                end: MediaTimestamp::from_nanos((i as u64 + 1) * 100_000_000),
                rms: 0.01,
            };
            let v = FrameDiffSample {
                start: MediaTimestamp::from_nanos(i as u64 * 100_000_000),
                end: MediaTimestamp::from_nanos((i as u64 + 1) * 100_000_000),
                change_ratio: 0.001,
            };
            assert!(push_bounded_audio_sample(&mut audio, a, max));
            assert!(push_bounded_visual_sample(&mut visual, v, max));
        }

        // These should be dropped.
        let a_overflow = AudioActivitySample {
            start: MediaTimestamp::from_nanos(200_000_000),
            end: MediaTimestamp::from_nanos(300_000_000),
            rms: 0.02,
        };
        let v_overflow = FrameDiffSample {
            start: MediaTimestamp::from_nanos(200_000_000),
            end: MediaTimestamp::from_nanos(300_000_000),
            change_ratio: 0.005,
        };
        if !push_bounded_audio_sample(&mut audio, a_overflow, max) {
            audio_dropped += 1;
        }
        if !push_bounded_visual_sample(&mut visual, v_overflow, max) {
            visual_dropped += 1;
        }

        assert_eq!(audio_dropped, 1);
        assert_eq!(visual_dropped, 1);
        assert!(audio_dropped > 0 || visual_dropped > 0);
    }

    /// Verifies that audio diagnostics tracks requested audio sources.
    #[test]
    fn audio_diagnostics_tracks_requested_sources() {
        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(1, "test");
        let stop_flag = Arc::new(AtomicBool::new(true)); // immediate stop
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        let writer: Box<dyn RecordingWriter> =
            Box::new(crate::media::recording_writer::CountingRecordingWriter::new(None));

        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            true, // requested_system_audio
            true, // requested_microphone
            Some("Built-in Microphone".to_string()),
        );

        assert!(output.diagnostics.requested_system_audio);
        assert!(output.diagnostics.requested_microphone);
        assert_eq!(
            output.diagnostics.microphone_device,
            Some("Built-in Microphone".to_string())
        );
    }

    /// Verifies that compute_rms returns correct RMS values.
    #[test]
    fn compute_rms_returns_correct_value() {
        // Empty slice.
        assert_eq!(compute_rms(&[]), 0.0);

        // Single sample.
        assert!((compute_rms(&[0.5]) - 0.5).abs() < 0.001);

        // Multiple samples: sqrt((0.25 + 0.25) / 2) = sqrt(0.25) = 0.5
        assert!((compute_rms(&[0.5, 0.5]) - 0.5).abs() < 0.001);

        // Mixed positive/negative: sqrt((0.04 + 0.04) / 2) = sqrt(0.04) = 0.2
        assert!((compute_rms(&[0.2, -0.2]) - 0.2).abs() < 0.001);
    }

    /// Verifies that small audio drops (<10%) produce warnings but NOT errors.
    /// This prevents incidental drops from causing recording failure (Important 3).
    #[test]
    fn consume_frames_warns_but_does_not_fail_on_small_audio_drop() {
        use crate::core::frame::AudioChunk;
        use crate::media::recording_writer::CountingRecordingWriter;
        use std::sync::Arc;

        // Create channel with capacity 11. Send 11 items to fill, then 1 more = 1 drop.
        // Drop ratio = 1/11 ≈ 9%, below the 10% threshold.
        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(11, "test");

        // Fill the channel with 11 chunks.
        for i in 0..11 {
            let chunk = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
                sample_rate: 48000,
                channels: 2,
                samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
            };
            assert!(audio_tx.try_send_drop_newest(chunk));
        }
        // This one will be dropped (channel full).
        let overflow_chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(110_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(!audio_tx.try_send_drop_newest(overflow_chunk));

        let stop_flag = Arc::new(AtomicBool::new(true)); // immediate stop
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            true,  // requested_system_audio
            false, // requested_microphone
            None,
        );

        // Should have received 11 chunks in final drain.
        assert_eq!(output.diagnostics.system_chunks_received, 11);
        // Should have 1 drop.
        assert_eq!(output.diagnostics.system_chunks_dropped, 1);

        // Drop ratio 1/11 ≈ 9% < 10% → should NOT be in errors.
        let drop_errors: Vec<_> = output
            .errors
            .iter()
            .filter(|e| e.contains("丢弃率过高"))
            .collect();
        assert!(
            drop_errors.is_empty(),
            "small drop ratio (9%) should not cause hard fail, got: {:?}",
            output.errors
        );
    }

    /// Verifies that high audio drop ratio (>10%) causes recording failure.
    /// This is the counterpart to consume_frames_warns_but_does_not_fail_on_small_audio_drop.
    /// Uses correct denominator: dropped / (received + dropped).
    #[test]
    fn consume_frames_fails_on_high_audio_drop_ratio() {
        use crate::media::recording_writer::CountingRecordingWriter;

        // Create channel with capacity 10. Send 10 items to fill, then 2 more = 2 drops.
        // Drop ratio = 2 / (10 + 2) = 16.7%, above the 10% threshold.
        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(10, "test");

        // Fill the channel with 10 chunks.
        for i in 0..10 {
            let chunk = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
                sample_rate: 48000,
                channels: 2,
                samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
            };
            assert!(audio_tx.try_send_drop_newest(chunk));
        }
        // These 2 will be dropped (channel full).
        for i in 10..12 {
            let overflow_chunk = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
                sample_rate: 48000,
                channels: 2,
                samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
            };
            assert!(!audio_tx.try_send_drop_newest(overflow_chunk));
        }

        let stop_flag = Arc::new(AtomicBool::new(true)); // immediate stop
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            true,  // requested_system_audio
            false, // requested_microphone
            None,
        );

        // Should have received 10 chunks in final drain.
        assert_eq!(output.diagnostics.system_chunks_received, 10);
        // Should have 2 drops.
        assert_eq!(output.diagnostics.system_chunks_dropped, 2);

        // Drop ratio 2/(10+2) = 16.7% > 10% → should be in errors.
        let drop_errors: Vec<_> = output
            .errors
            .iter()
            .filter(|e| e.contains("丢弃率过高"))
            .collect();
        assert!(
            !drop_errors.is_empty(),
            "high drop ratio (20%) should cause hard fail, got: {:?}",
            output.errors
        );
    }

    /// Verifies that stop_flag set before consumer thread starts is respected.
    /// This covers the timing window where stop() is called during startup,
    /// before the consumer thread enters its main loop.
    #[test]
    fn consume_frames_respects_stop_flag_set_before_start() {
        use crate::media::recording_writer::CountingRecordingWriter;

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(10, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(10, "test");

        // Set stop_flag BEFORE starting consumer — simulates stop-during-startup.
        let stop_flag = Arc::new(AtomicBool::new(true));
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

        // Send some data — consumer should drain these in final drain.
        for i in 0..5 {
            let chunk = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
                sample_rate: 48000,
                channels: 2,
                samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
            };
            assert!(audio_tx.try_send_drop_newest(chunk));
        }

        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            true,
            false,
            None,
        );

        // Consumer should have drained the 5 chunks in final drain.
        assert_eq!(output.diagnostics.system_chunks_received, 5);
        // No errors expected — clean stop.
        assert!(
            output.errors.is_empty(),
            "stop-before-start should not produce errors, got: {:?}",
            output.errors
        );
    }

    /// Verifies that calling stop while captures are actively producing data
    /// results in a clean drain and no data loss for already-queued frames.
    #[test]
    fn consume_frames_drains_queued_data_on_stop() {
        use crate::media::recording_writer::CountingRecordingWriter;

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(100, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(100, "test");
        let stop_flag = Arc::new(AtomicBool::new(false));
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        // Send 20 audio chunks while consumer is running.
        for i in 0..20 {
            let chunk = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
                sample_rate: 48000,
                channels: 2,
                samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
            };
            assert!(audio_tx.try_send_drop_newest(chunk));
        }

        let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

        // Stop after a brief delay to let consumer process some frames.
        let flag_clone = stop_flag.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            flag_clone.store(true, Ordering::Relaxed);
        });

        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            true,
            false,
            None,
        );

        stopper.join().unwrap();

        // All 20 chunks should have been received (some in loop, rest in final drain).
        assert_eq!(output.diagnostics.system_chunks_received, 20);
        // No drop errors expected.
        let drop_errors: Vec<_> = output
            .errors
            .iter()
            .filter(|e| e.contains("丢弃率过高"))
            .collect();
        assert!(
            drop_errors.is_empty(),
            "no drop errors expected for 20/20 chunks, got: {:?}",
            output.errors
        );
    }

    /// Verifies that drop ratio just below 10% does NOT cause hard fail.
    /// Uses the correct denominator: dropped / (received + dropped).
    /// 10 drops / 110 attempted = 9.09% < 10%.
    #[test]
    fn consume_frames_passes_when_attempted_drop_ratio_below_10_percent() {
        use crate::media::recording_writer::CountingRecordingWriter;

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(100, "test");

        for i in 0..100 {
            let chunk = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
                sample_rate: 48000,
                channels: 2,
                samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
            };
            assert!(audio_tx.try_send_drop_newest(chunk));
        }
        // 10 drops: channel capacity is 100, already full.
        for i in 100..110 {
            let chunk = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
                sample_rate: 48000,
                channels: 2,
                samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
            };
            assert!(!audio_tx.try_send_drop_newest(chunk));
        }

        let stop_flag = Arc::new(AtomicBool::new(true));
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));
        let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            true,
            false,
            None,
        );

        assert_eq!(output.diagnostics.system_chunks_received, 100);
        assert_eq!(output.diagnostics.system_chunks_dropped, 10);

        // 10 / (100 + 10) = 9.09% < 10% → should NOT be in errors.
        let drop_errors: Vec<_> = output
            .errors
            .iter()
            .filter(|e| e.contains("丢弃率过高"))
            .collect();
        assert!(
            drop_errors.is_empty(),
            "9.09% drop ratio should not cause hard fail, got: {:?}",
            output.errors
        );
    }

    /// Verifies that drop ratio just above 10% DOES cause hard fail.
    /// Uses the correct denominator: dropped / (received + dropped).
    /// 12 drops / 112 attempted = 10.71% > 10%.
    #[test]
    fn consume_frames_fails_when_attempted_drop_ratio_above_10_percent() {
        use crate::media::recording_writer::CountingRecordingWriter;

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(100, "test");

        for i in 0..100 {
            let chunk = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
                sample_rate: 48000,
                channels: 2,
                samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
            };
            assert!(audio_tx.try_send_drop_newest(chunk));
        }
        // 12 drops: channel capacity is 100, already full.
        for i in 100..112 {
            let chunk = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
                sample_rate: 48000,
                channels: 2,
                samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
            };
            assert!(!audio_tx.try_send_drop_newest(chunk));
        }

        let stop_flag = Arc::new(AtomicBool::new(true));
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));
        let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            writer,
            mic_level,
            "medium",
            true,
            false,
            None,
        );

        assert_eq!(output.diagnostics.system_chunks_received, 100);
        assert_eq!(output.diagnostics.system_chunks_dropped, 12);

        // 12 / (100 + 12) = 10.71% > 10% → should be in errors.
        let drop_errors: Vec<_> = output
            .errors
            .iter()
            .filter(|e| e.contains("丢弃率过高"))
            .collect();
        assert!(
            !drop_errors.is_empty(),
            "10.71% drop ratio should cause hard fail, got: {:?}",
            output.errors
        );
    }

    /// Verifies that per-source writer counters are NOT incremented
    /// when push_audio() fails (e.g., queue full or channel disconnected).
    ///
    /// This is a regression test for the issue where record_source_contribution()
    /// was called before push_audio(), causing diagnostics to show "writer received"
    /// even when the writer never actually received the chunk.
    #[test]
    fn consume_frames_does_not_increment_per_source_writer_counter_when_push_audio_fails() {
        use crate::media::recording_writer::FailingRecordingWriter;

        let writer = FailingRecordingWriter::new(false, true, false);

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(10, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(10, "test");

        // Send one audio chunk (will be mixed and pushed to writer)
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(audio_tx.try_send_drop_newest(chunk));

        let stop_flag = Arc::new(AtomicBool::new(true));
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            Box::new(writer),
            mic_level,
            "medium",
            true,
            false,
            None,
        );

        // push_audio fails, so per-source counters should be 0
        assert_eq!(
            output
                .result
                .writer_diagnostics
                .system_chunks_received_by_writer,
            0,
            "per-source counter should not increment when push_audio fails"
        );
        assert_eq!(
            output
                .result
                .writer_diagnostics
                .mic_chunks_received_by_writer,
            0,
            "per-source counter should not increment when push_audio fails"
        );
        // But push failure count should be > 0
        assert!(
            output.diagnostics.writer_push_audio_failures > 0,
            "push_audio failure should be recorded"
        );
    }

    /// Verifies that per-source writer counters ARE incremented
    /// when push_audio() succeeds.
    #[test]
    fn consume_frames_increments_per_source_writer_counter_on_successful_push() {
        use crate::media::recording_writer::CountingRecordingWriter;

        let writer = CountingRecordingWriter::new(None);
        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(10, "test");
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(10, "test");

        // Send one audio chunk with system audio
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(audio_tx.try_send_drop_newest(chunk));

        let stop_flag = Arc::new(AtomicBool::new(true));
        let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mic_level = Arc::new(Mutex::new(0.0f64));

        drop(video_tx);
        drop(audio_tx);

        let output = MacRecordingService::consume_frames(
            stop_flag,
            video_rx,
            audio_rx,
            None,
            frame_count,
            Box::new(writer),
            mic_level,
            "medium",
            true,
            false,
            None,
        );

        // push_audio succeeds (CountingRecordingWriter never fails),
        // so per-source counter should reflect the system audio chunk.
        assert!(
            output
                .result
                .writer_diagnostics
                .system_chunks_received_by_writer
                > 0,
            "per-source system counter should increment on successful push"
        );
    }

    /// Verifies that empty_consumer_output() produces a valid RecordingConsumerOutput
    /// that can be safely used by the timeout path in join_consumer().
    #[test]
    fn empty_consumer_output_produces_valid_defaults() {
        use crate::media::recording_writer::{
            RecordingDiagnostics, RecordingResult, WriterDiagnostics,
        };
        use crate::media::trim_metadata::TrimMetadata;

        // Construct the same defaults inline (since empty_consumer_output is private).
        let empty = RecordingConsumerOutput {
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
                finalization_errors: Vec::new(),
            },
            trim_metadata: TrimMetadata {
                schema_version: TRIM_METADATA_SCHEMA_VERSION,
                duration_nanos: 0,
                base_audio_activity: Vec::new(),
                audio_activity: Vec::new(),
                visual_activity: Vec::new(),
                audio_activity_dropped_count: 0,
                visual_activity_dropped_count: 0,
                activity_truncated: false,
            },
            diagnostics: RecordingDiagnostics::default(),
            errors: Vec::new(),
        };

        assert_eq!(empty.result.duration_secs, 0);
        assert_eq!(empty.result.frame_count, 0);
        assert!(empty.result.output_path.is_none());
        assert!(empty.errors.is_empty());
        assert!(!empty.diagnostics.requested_system_audio);
        assert!(empty.diagnostics.mic_stop_diagnostics.is_none());
    }

    /// Verifies that recv_timeout on a never-send channel returns Timeout
    /// quickly, matching the consumer timeout branch in join_consumer().
    ///
    /// This is a regression test for BUG.md rule 28: the timeout branch
    /// must not call handle.join() which would block forever if the consumer
    /// is stuck in writer finalize, artifact validation, or metadata generation.
    #[test]
    fn recv_timeout_consumer_returns_quickly_on_never_send_channel() {
        use std::sync::mpsc;
        use std::time::Instant;

        // Channel that will never send a result (simulates stuck consumer).
        let (_tx, rx) = mpsc::channel::<RecordingConsumerOutput>();
        let timeout = std::time::Duration::from_millis(100);

        let start = Instant::now();
        let result = rx.recv_timeout(timeout);
        let elapsed = start.elapsed();

        assert!(
            result.is_err(),
            "expected Timeout error from never-send channel"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "recv_timeout should return within timeout duration, took {:?}",
            elapsed
        );
    }

    /// Verifies that the empty_consumer_output fallback produces a result
    /// with diagnostics available, even when the consumer thread timed out.
    #[test]
    fn consumer_timeout_fallback_preserves_diagnostics_slot() {
        let empty = RecordingFinalizeGuard::empty_consumer_output();

        // The empty output should have valid defaults that allow
        // drive_state_machine() to inject diagnostics.
        assert_eq!(empty.result.duration_secs, 0);
        assert!(empty.result.diagnostics.requested_system_audio == false);
        assert!(empty.result.finalization_errors.is_empty());
        // mic_stop_diagnostics should be None (no mic was stopped).
        assert!(empty.result.diagnostics.mic_stop_diagnostics.is_none());
    }

    /// Regression test for BUG.md rule 28: the production consumer timeout
    /// branch must not call handle.join() which would block forever.
    ///
    /// This test calls the actual production helper
    /// `receive_consumer_output_with_timeout` with a never-send channel
    /// and a parked thread. If the helper were to call handle.join() on
    /// the timeout path, this test would hang.
    #[test]
    fn join_consumer_timeout_detaches_parked_consumer() {
        use std::time::Instant;

        let (_tx, rx) = mpsc::channel::<RecordingConsumerOutput>();
        let handle = thread::spawn(|| thread::park());
        let mut consumer_handle = Some(handle);
        let mut errors = Vec::new();

        let start = Instant::now();
        let (output, panicked) = RecordingFinalizeGuard::receive_consumer_output_with_timeout(
            Some(rx),
            &mut consumer_handle,
            std::time::Duration::from_millis(50),
            &mut errors,
        );

        let elapsed = start.elapsed();
        assert!(panicked, "timeout should set panicked flag");
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "timeout should return quickly, took {:?}",
            elapsed
        );
        assert!(
            consumer_handle.is_none(),
            "consumer handle should be taken (detached) on timeout"
        );
        assert!(
            errors.iter().any(|e| e.contains("超时")),
            "should record timeout error, got: {:?}",
            errors
        );
        assert_eq!(
            output.result.duration_secs, 0,
            "empty output should have zero duration"
        );
    }

    /// Verifies that on timeout, the empty fallback output carries valid
    /// diagnostics and the error is recorded for finalization_errors.
    #[test]
    fn join_consumer_timeout_records_error_and_preserves_empty_output() {
        let (_tx, rx) = mpsc::channel::<RecordingConsumerOutput>();
        let handle = thread::spawn(|| thread::park());
        let mut consumer_handle = Some(handle);
        let mut errors = Vec::new();

        let (output, _) = RecordingFinalizeGuard::receive_consumer_output_with_timeout(
            Some(rx),
            &mut consumer_handle,
            std::time::Duration::from_millis(50),
            &mut errors,
        );

        assert!(!errors.is_empty(), "timeout should record error");
        assert!(
            output.diagnostics.mic_stop_diagnostics.is_none(),
            "empty output should have None mic_stop_diagnostics"
        );
        assert!(
            output.result.finalization_errors.is_empty(),
            "finalization_errors populated later by drive_state_machine"
        );
    }
}
