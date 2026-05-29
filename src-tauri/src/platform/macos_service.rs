use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::macos::cpal_microphone::CpalMicrophoneCapture;
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
use crate::core::cut::TrimConfig;
use crate::media::audio_mixer::SimpleAudioMixer;
use crate::media::mic_level::MicLevelDetector;
use crate::media::recording_metadata::RecordingMetadataWriter;
use crate::media::recording_writer::{CountingRecordingWriter, RecordingResult, RecordingWriter};
use crate::media::silence_detector::{AudioRmsAnalyzer, FrameDiffAnalyzer};
use crate::media::trim_metadata::{TrimMetadata, TrimMetadataWriter};

/// Internal return type that bundles the recording result with trim metadata.
struct RecordingConsumerOutput {
    result: RecordingResult,
    trim_metadata: TrimMetadata,
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
    consumer_handle: Option<thread::JoinHandle<RecordingConsumerOutput>>,
    frame_count: Arc<std::sync::atomic::AtomicU64>,
    /// 当前麦克风 RMS 电平值 (0.0 ~ 1.0)，由消费线程周期性更新，外部通过 `mic_level()` 读取。
    mic_level: Arc<Mutex<f64>>,
    cursor_runtime: Option<CursorMetadataRuntime>,
    last_cursor_metadata_path: Option<String>,
    last_effect_timeline_path: Option<String>,
    last_trim_metadata_path: Option<String>,
    last_cut_timeline_path: Option<String>,
    /// Monotonically incrementing session counter. Used to guard async
    /// post-process jobs against writing stale results into a new session.
    session_id: u64,
}

impl MacRecordingService {
    pub fn last_cursor_metadata_path(&self) -> Option<String> {
        self.last_cursor_metadata_path.clone()
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
            frame_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            mic_level: Arc::new(Mutex::new(0.0)),
            cursor_runtime: None,
            last_cursor_metadata_path: None,
            last_effect_timeline_path: None,
            last_trim_metadata_path: None,
            last_cut_timeline_path: None,
            session_id: 0,
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
    ) -> AppResult<()> {
        self.state_machine.start()?;

        // Clear stale session state from any previous recording.
        self.last_cursor_metadata_path = None;
        self.last_effect_timeline_path = None;
        self.last_trim_metadata_path = None;
        self.last_cut_timeline_path = None;
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
        let (video_sender, video_receiver) = bounded_media_channel(VIDEO_QUEUE_CAPACITY);
        let (audio_sender, audio_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY);

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
            let (mic_sender, mic_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY);
            self.mic_capture.set_session_clock(session_clock.clone());
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

        // Start cursor metadata runtime for cursor effects.
        self.cursor_runtime = Some(CursorMetadataRuntime::spawn(
            MacCursorSource::new(),
            config.fps,
            session_clock.clone(),
            beautify_snapshot,
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

        let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));
        self.consumer_handle = Some(thread::spawn(move || {
            Self::consume_frames(
                stop_flag,
                video_rx,
                system_audio_rx,
                mic_rx,
                frame_count,
                writer,
                mic_level,
            )
        }));

        Ok(())
    }

    /// Stops all captures and finalizes the recording session.
    ///
    /// All resource-release steps (capture stop, consumer join, cursor stop,
    /// mic reset, state machine transition) are executed regardless of
    /// intermediate errors. Errors are collected and returned together so
    /// a sidecar write failure never skips cleanup.
    pub fn stop(&mut self) -> AppResult<RecordingResult> {
        let mut errors: Vec<String> = Vec::new();

        // Stop cursor runtime BEFORE native captures so the metadata duration
        // reflects the moment we decided to stop, not the SCK async teardown
        // (stopCaptureWithCompletionHandler can block for up to 5s).
        let cursor_metadata = self
            .cursor_runtime
            .as_mut()
            .and_then(|runtime| runtime.stop());
        self.cursor_runtime = None;

        // Stop native captures so no new media can be enqueued.
        let capture_result = ScreenCapture::stop(&mut self.screen_capture);
        let mic_result = self.mic_capture.stop();

        // Signal the consumer thread to stop.
        if let Some(flag) = &self.stop_flag {
            flag.store(true, Ordering::Relaxed);
        }

        // Join the consumer thread and get writer result.
        let empty_result = RecordingResult {
            duration_secs: 0,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            output_path: None,
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
        };
        let empty_output = RecordingConsumerOutput {
            result: empty_result,
            trim_metadata: TrimMetadata {
                duration_nanos: 0,
                audio_activity: Vec::new(),
                visual_activity: Vec::new(),
            },
        };
        let consumer_output = if let Some(handle) = self.consumer_handle.take() {
            handle.join().unwrap_or(empty_output)
        } else {
            empty_output
        };
        let mut result = consumer_output.result;

        self.stop_flag = None;

        // Write cursor metadata sidecar (cursor runtime is already stopped).
        let cursor_metadata_path = match cursor_metadata {
            Some(metadata) => {
                let path = cursor_metadata_path();
                match RecordingMetadataWriter::write_metadata(&path, &metadata) {
                    Ok(()) => Some(path.to_string_lossy().to_string()),
                    Err(e) => {
                        errors.push(format!("光标元数据写入失败: {e}"));
                        None
                    }
                }
            }
            None => None,
        };

        result.cursor_metadata_path = cursor_metadata_path.clone();
        result.effect_timeline_path = self.last_effect_timeline_path.clone();
        self.last_cursor_metadata_path = cursor_metadata_path;

        // Write trim metadata sidecar for post-recording silence detection.
        let trim_metadata_path = {
            let path = trim_metadata_path();
            match TrimMetadataWriter::write_metadata(&path, &consumer_output.trim_metadata) {
                Ok(()) => Some(path.to_string_lossy().to_string()),
                Err(e) => {
                    errors.push(format!("裁剪元数据写入失败: {e}"));
                    None
                }
            }
        };
        result.trim_metadata_path = trim_metadata_path.clone();
        self.last_trim_metadata_path = trim_metadata_path;

        // Reset mic level after session ends — always executed.
        if let Ok(mut guard) = self.mic_level.lock() {
            *guard = 0.0;
        }

        // Collect capture/mic stop errors.
        if let Err(e) = capture_result {
            errors.push(format!("屏幕录制停止失败: {e}"));
        }
        if let Err(e) = mic_result {
            errors.push(format!("麦克风停止失败: {e}"));
        }

        // Always drive state machine to a terminal state.
        if errors.is_empty() {
            if let Err(e) = self.state_machine.stop() {
                self.state_machine.fail();
                return Err(e);
            }
            if let Err(e) = self.state_machine.complete() {
                self.state_machine.fail();
                return Err(e);
            }
            Ok(result)
        } else {
            self.state_machine.fail();
            Err(crate::app::error::AppError::RecordingFinalizeFailed {
                reason: errors.join("; "),
            })
        }
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
        mut writer: Box<dyn RecordingWriter>,
        mic_level: Arc<Mutex<f64>>,
    ) -> RecordingConsumerOutput {
        let mut synchronizer = crate::media::audio_synchronizer::AudioSynchronizer::default();
        let mut mic_detector = MicLevelDetector::new(4096); // ~85ms 窗口 @ 48kHz

        // Trim metadata collectors — low-cost activity samples for post-recording
        // silence detection. Computed in the consumer thread, never sent to React.
        let rms_analyzer = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(
            crate::core::cut::TrimSensitivity::Medium,
        ));
        let frame_diff_analyzer = FrameDiffAnalyzer::new(64, 36);
        let mut previous_frame: Option<crate::core::frame::VideoFrame> = None;
        let mut visual_activity = Vec::new();
        let mut audio_activity = Vec::new();

        loop {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            // Drain video frames (non-blocking).
            while let Ok(frame) = video_rx.try_recv() {
                frame_count.fetch_add(1, Ordering::Relaxed);
                // Collect frame-diff samples for trim metadata.
                if let Some(ref previous) = previous_frame {
                    if let Ok(diff) = frame_diff_analyzer.diff_pair(previous, &frame) {
                        visual_activity.push(diff);
                    }
                }
                previous_frame = Some((*frame).clone());
                if let Err(e) = writer.push_video(frame) {
                    eprintln!("写入视频帧失败: {e}");
                }
            }

            // Enqueue system audio chunks for ordered pairing.
            while let Ok(chunk) = system_audio_rx.try_recv() {
                synchronizer.push_system(chunk);
            }

            // Enqueue microphone audio chunks and compute RMS level.
            if let Some(ref mic_rx) = mic_rx {
                while let Ok(chunk) = mic_rx.try_recv() {
                    // Feed mic samples to the level detector
                    let level = mic_detector.push_samples(&chunk.samples);
                    if let Ok(mut guard) = mic_level.lock() {
                        *guard = level;
                    }
                    synchronizer.push_mic(chunk);
                }
            }

            // Pair by timestamp proximity and mix.
            for mixed_result in synchronizer.drain_mixed() {
                match mixed_result {
                    Ok(mixed) => {
                        // Collect audio RMS samples for trim metadata.
                        let samples = rms_analyzer.analyze_chunks(std::slice::from_ref(&mixed));
                        audio_activity.extend(samples);
                        if let Err(e) = writer.push_audio(mixed) {
                            eprintln!("写入混音音频失败: {e}");
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
            if let Some(ref previous) = previous_frame {
                if let Ok(diff) = frame_diff_analyzer.diff_pair(previous, &frame) {
                    visual_activity.push(diff);
                }
            }
            previous_frame = Some((*frame).clone());
            if let Err(e) = writer.push_video(frame) {
                eprintln!("写入视频帧失败: {e}");
            }
        }

        while let Ok(chunk) = system_audio_rx.try_recv() {
            synchronizer.push_system(chunk);
        }

        if let Some(ref mic_rx) = mic_rx {
            while let Ok(chunk) = mic_rx.try_recv() {
                synchronizer.push_mic(chunk);
            }
        }

        for mixed_result in synchronizer.drain_mixed() {
            match mixed_result {
                Ok(mixed) => {
                    let samples = rms_analyzer.analyze_chunks(std::slice::from_ref(&mixed));
                    audio_activity.extend(samples);
                    if let Err(e) = writer.push_audio(mixed) {
                        eprintln!("写入混音音频失败: {e}");
                    }
                }
                Err(e) => eprintln!("音频混合失败: {e}"),
            }
        }

        let result = writer.finish().unwrap_or(RecordingResult {
            duration_secs: 0,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            output_path: None,
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
        });
        let duration_nanos = result.duration_secs.saturating_mul(1_000_000_000);
        RecordingConsumerOutput {
            result,
            trim_metadata: TrimMetadata {
                duration_nanos,
                audio_activity,
                visual_activity,
            },
        }
    }
}

impl Default for MacRecordingService {
    fn default() -> Self {
        Self::new()
    }
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
