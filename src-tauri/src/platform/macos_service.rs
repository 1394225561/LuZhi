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
use crate::media::audio_mixer::SimpleAudioMixer;
use crate::media::mic_level::MicLevelDetector;
use crate::media::recording_metadata::RecordingMetadataWriter;
use crate::media::recording_writer::{CountingRecordingWriter, RecordingResult, RecordingWriter};
use crate::media::silence_detector::FrameDiffAnalyzer;
use crate::media::trim_audio_activity::BaseAudioActivityAnalyzer;
use crate::media::trim_metadata::{TrimMetadata, TrimMetadataWriter, TRIM_METADATA_SCHEMA_VERSION};

/// Internal return type that bundles the recording result with trim metadata.
struct RecordingConsumerOutput {
    result: RecordingResult,
    trim_metadata: TrimMetadata,
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

        // Extract trim sensitivity before moving beautify_snapshot into cursor runtime.
        let trim_sensitivity = beautify_snapshot.trim_sensitivity.clone();

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
                &trim_sensitivity,
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
                schema_version: TRIM_METADATA_SCHEMA_VERSION,
                duration_nanos: 0,
                base_audio_activity: Vec::new(),
                audio_activity: Vec::new(),
                visual_activity: Vec::new(),
                audio_activity_dropped_count: 0,
                visual_activity_dropped_count: 0,
                activity_truncated: false,
            },
            errors: Vec::new(),
        };
        let (consumer_output, consumer_panicked) = if let Some(handle) = self.consumer_handle.take()
        {
            match handle.join() {
                Ok(output) => (output, false),
                Err(panic) => {
                    let msg = extract_panic_message(panic);
                    eprintln!("录制消费线程异常终止: {msg}");
                    errors.push(format!("录制消费线程异常终止: {msg}"));
                    (empty_output, true)
                }
            }
        } else {
            (empty_output, false)
        };
        let mut result = consumer_output.result;
        errors.extend(consumer_output.errors);

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
        // Skip when consumer panicked — the metadata would be empty/misleading.
        let trim_metadata_path = if consumer_panicked {
            None
        } else {
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn consume_frames(
        stop_flag: Arc<AtomicBool>,
        video_rx: MediaReceiver<VideoFrameRef>,
        system_audio_rx: MediaReceiver<AudioChunk>,
        mic_rx: Option<MediaReceiver<AudioChunk>>,
        frame_count: Arc<std::sync::atomic::AtomicU64>,
        mut writer: Box<dyn RecordingWriter>,
        mic_level: Arc<Mutex<f64>>,
        trim_sensitivity_str: &str,
    ) -> RecordingConsumerOutput {
        let mut synchronizer = crate::media::audio_synchronizer::AudioSynchronizer::default();
        let mut mic_detector = MicLevelDetector::new(4096); // ~85ms 窗口 @ 48kHz

        // Trim metadata collectors — sensitivity-independent 100ms base RMS
        // buckets. Computed in the consumer thread, never sent to React.
        // Post-recording aggregation applies the user-configured sensitivity.
        let mut base_audio_analyzer = BaseAudioActivityAnalyzer::default();
        let frame_diff_analyzer = FrameDiffAnalyzer::new(64, 36);
        // Low-frequency visual sampling: ~4fps to avoid per-frame diff cost.
        const VISUAL_SAMPLE_INTERVAL_NANOS: u64 = 250_000_000;
        // Safety caps to prevent unbounded memory growth during very long recordings.
        const MAX_VISUAL_SAMPLES: usize = 144_000; // ~10h @ 4fps
        const MAX_AUDIO_SAMPLES: usize = 72_000; // ~10h @ 2/sec (750ms window)
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

            // Drain video frames (non-blocking).
            while let Ok(frame) = video_rx.try_recv() {
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
                        // Track audio duration independent of sample caps.
                        let chunk_frames =
                            mixed.samples.len() as u64 / mixed.channels.max(1) as u64;
                        let chunk_nanos = chunk_frames.saturating_mul(1_000_000_000)
                            / mixed.sample_rate.max(1) as u64;
                        latest_observed_media_nanos = latest_observed_media_nanos
                            .max(mixed.timestamp.nanos.saturating_add(chunk_nanos));
                        // Collect sensitivity-independent 100ms base RMS buckets.
                        for sample in base_audio_analyzer.push_chunk(&mixed) {
                            if !push_bounded_base_audio_sample(
                                &mut base_audio_activity,
                                sample,
                                MAX_AUDIO_SAMPLES,
                            ) {
                                audio_dropped += 1;
                            }
                        }
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
                    let chunk_frames = mixed.samples.len() as u64 / mixed.channels.max(1) as u64;
                    let chunk_nanos = chunk_frames.saturating_mul(1_000_000_000)
                        / mixed.sample_rate.max(1) as u64;
                    latest_observed_media_nanos = latest_observed_media_nanos
                        .max(mixed.timestamp.nanos.saturating_add(chunk_nanos));
                    for sample in base_audio_analyzer.push_chunk(&mixed) {
                        if !push_bounded_base_audio_sample(
                            &mut base_audio_activity,
                            sample,
                            MAX_AUDIO_SAMPLES,
                        ) {
                            audio_dropped += 1;
                        }
                    }
                    if let Err(e) = writer.push_audio(mixed) {
                        let msg = format!("写入混音音频失败: {e}");
                        eprintln!("{msg}");
                        errors.push(msg);
                    }
                }
                Err(e) => eprintln!("音频混合失败: {e}"),
            }
        }

        // Flush remaining base audio buckets from the analyzer.
        if let Some(sample) = base_audio_analyzer.flush() {
            if !push_bounded_base_audio_sample(&mut base_audio_activity, sample, MAX_AUDIO_SAMPLES) {
                audio_dropped += 1;
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
                }
            }
        };

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
        use crate::core::cut::TrimConfig;
        use crate::media::recording_writer::FailingRecordingWriter;

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1);
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(1);
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

        let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(2);
        let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(1);
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
        );

        stopper.join().unwrap();

        assert!(
            output.errors.iter().any(|e| e.contains("写入视频帧失败")),
            "expected push_video error in consumer output, got: {:?}",
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
}
