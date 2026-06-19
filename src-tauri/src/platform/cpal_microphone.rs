use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, StreamConfig};

use crate::app::error::{AppError, AppResult};
use crate::core::capture::{
    AudioCapabilities, AudioCapture, AudioChunkSink, AudioConfig, AudioDevice,
};
use crate::core::clock::SessionClock;
use crate::core::frame::AudioChunk;

/// Structured diagnostics from the last `stop()` call (Minor 1).
///
/// Captures the full stop lifecycle for post-mortem analysis of
/// Bluetooth HFP profile release issues.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpalMicrophoneStopDiagnostics {
    pub stop_requested: bool,
    pub stream_existed: bool,
    pub pause_attempted: bool,
    pub pause_ok: bool,
    pub pause_error: Option<String>,
    pub stream_dropped: bool,
    pub callbacks_after_stop: u64,
    pub stop_wait_ms: u64,
}

/// Wrapper to make `cpal::Stream` `Send`.
///
/// cpal marks `Stream` as `!Send` via `NotSendSyncAcrossAllPlatforms` for
/// portability. On macOS (CoreAudio), streams can be created, started, and
/// dropped from any thread — the callbacks run on the audio IO thread but
/// stream lifecycle management is thread-safe.
struct SendStream(cpal::Stream);

// SAFETY: CoreAudio streams are created and destroyed on the calling thread;
// the audio callbacks run on the IO thread but only use lock-free atomics
// and try_send on bounded channels, which are already Send+Sync.
unsafe impl Send for SendStream {}

/// Microphone audio capture using the cpal crate.
///
/// Captures audio from the default input device (or a specified device)
/// and sends `AudioChunk`s through the provided sink.
pub struct CpalMicrophoneCapture {
    stream: Option<SendStream>,
    running: Arc<AtomicBool>,
    session_clock: Option<Arc<SessionClock>>,
    /// Count of callbacks that fired after `running` was set to false.
    callbacks_after_stop: Arc<AtomicU64>,
    /// Diagnostics from the last `stop()` call.
    last_stop_diagnostics: CpalMicrophoneStopDiagnostics,
}

impl CpalMicrophoneCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
            running: Arc::new(AtomicBool::new(false)),
            session_clock: None,
            callbacks_after_stop: Arc::new(AtomicU64::new(0)),
            last_stop_diagnostics: CpalMicrophoneStopDiagnostics::default(),
        }
    }

    /// Returns diagnostics from the last `stop()` call.
    pub fn last_stop_diagnostics(&self) -> &CpalMicrophoneStopDiagnostics {
        &self.last_stop_diagnostics
    }

    /// Stop microphone capture and return structured diagnostics.
    ///
    /// This is the preferred method for callers that need to preserve
    /// stop diagnostics (e.g., for RecordingResult or bug investigation).
    /// The trait method `stop()` calls this internally but discards the return value.
    pub fn stop_with_diagnostics(&mut self) -> AppResult<CpalMicrophoneStopDiagnostics> {
        eprintln!("CpalMicrophoneCapture::stop_with_diagnostics() 开始");

        let mut diag = CpalMicrophoneStopDiagnostics::default();
        diag.stop_requested = true;

        // Reset callback-after-stop counter.
        self.callbacks_after_stop.store(0, Ordering::Relaxed);

        // Signal running=false first — callbacks check this flag and exit early.
        self.running.store(false, Ordering::Relaxed);

        if let Some(send_stream) = self.stream.take() {
            diag.stream_existed = true;

            // Explicit pause before drop — captures CoreAudio stop error.
            // drop() alone silently ignores stop/uninitialize errors.
            diag.pause_attempted = true;
            match send_stream.0.pause() {
                Ok(()) => {
                    diag.pause_ok = true;
                    eprintln!("CpalMicrophoneCapture::pause() 成功");
                }
                Err(e) => {
                    diag.pause_error = Some(format!("{:?}", e));
                    eprintln!("CpalMicrophoneCapture::pause() 失败: {:?}", e);
                }
            }

            // Drop stream to release CoreAudio resources.
            drop(send_stream);
            diag.stream_dropped = true;

            // Bounded wait for CoreAudio to complete device release.
            // Bluetooth HFP profile switching can take 100-300ms.
            let wait_start = std::time::Instant::now();
            std::thread::sleep(std::time::Duration::from_millis(300));
            diag.stop_wait_ms = wait_start.elapsed().as_millis() as u64;

            diag.callbacks_after_stop = self.callbacks_after_stop.load(Ordering::Acquire);

            eprintln!(
                "CpalMicrophoneCapture::stop_with_diagnostics() 完成 — stream_dropped={}, pause_ok={}, callbacks_after_stop={}, waited={}ms",
                diag.stream_dropped, diag.pause_ok, diag.callbacks_after_stop, diag.stop_wait_ms
            );
        } else {
            eprintln!("CpalMicrophoneCapture::stop_with_diagnostics() 完成 — 无活跃 stream");
        }

        self.last_stop_diagnostics = diag.clone();
        Ok(diag)
    }

    /// Sets the shared session clock so microphone timestamps use the same
    /// time basis as ScreenCaptureKit (host clock).
    pub fn set_session_clock(&mut self, clock: Arc<SessionClock>) {
        self.session_clock = Some(clock);
    }
}

impl Default for CpalMicrophoneCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCapture for CpalMicrophoneCapture {
    fn start(&mut self, config: AudioConfig, sink: AudioChunkSink) -> AppResult<()> {
        if self.running.load(Ordering::Relaxed) {
            return Err(AppError::InvalidState {
                current: "recording",
                action: "start audio capture",
            });
        }

        let host = cpal::default_host();

        // Select the microphone device.
        let device = if let Some(ref device_name) = config.microphone_device {
            host.input_devices()
                .map_err(|e| AppError::AudioDeviceNotFound {
                    name: format!("枚举设备失败: {}", e),
                })?
                .find(|d| d.name().map(|n| n == *device_name).unwrap_or(false))
                .ok_or_else(|| AppError::AudioDeviceNotFound {
                    name: device_name.clone(),
                })?
        } else {
            host.default_input_device()
                .ok_or_else(|| AppError::AudioDeviceNotFound {
                    name: "默认麦克风".to_string(),
                })?
        };

        // Configure the input stream.
        //
        // IMPORTANT: Always use the device's default config for the actual
        // hardware stream. The requested sample_rate/channels from the frontend
        // represent the desired *mixer output* format (48kHz stereo), not what
        // the hardware supports. The AudioMixer downstream handles resampling.
        let supported_config =
            device
                .default_input_config()
                .map_err(|e| AppError::AudioCaptureFailed {
                    reason: format!("获取麦克风默认配置失败: {}", e),
                })?;

        let stream_config = StreamConfig {
            channels: supported_config.channels(),
            sample_rate: supported_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        // Log the negotiation result for diagnostics.
        let requested_rate = config.sample_rate;
        let requested_ch = config.channels;
        let actual_rate = stream_config.sample_rate.0;
        let actual_ch = stream_config.channels;
        if requested_rate > 0 && requested_rate != actual_rate
            || requested_ch > 0 && requested_ch != actual_ch
        {
            eprintln!(
                "麦克风配置协商: 请求 {}Hz/{}ch, 设备实际 {}Hz/{}ch",
                requested_rate, requested_ch, actual_rate, actual_ch
            );
        }

        let sample_format = supported_config.sample_format();
        let running = self.running.clone();
        let callbacks_after_stop = self.callbacks_after_stop.clone();
        let sample_rate_val = stream_config.sample_rate.0;
        let channels_val = stream_config.channels;
        let session_clock = self.session_clock.clone();

        // Build the input stream based on the sample format.
        // cpal provides f32, i16, and u16 sample formats.
        let stream = match sample_format {
            SampleFormat::F32 => build_input_stream::<f32>(
                &device,
                &stream_config,
                sink.clone(),
                running.clone(),
                callbacks_after_stop.clone(),
                sample_rate_val,
                channels_val,
                session_clock.clone(),
            ),
            SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &stream_config,
                sink.clone(),
                running.clone(),
                callbacks_after_stop.clone(),
                sample_rate_val,
                channels_val,
                session_clock.clone(),
            ),
            SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &stream_config,
                sink,
                running.clone(),
                callbacks_after_stop,
                sample_rate_val,
                channels_val,
                session_clock,
            ),
            _ => {
                return Err(AppError::AudioCaptureFailed {
                    reason: format!("不支持的采样格式: {:?}", sample_format),
                });
            }
        }?;

        // Set running before play() so the first callback sees running=true.
        // Roll back if play() fails to avoid a stuck running state.
        self.running.store(true, Ordering::Relaxed);
        match stream.play() {
            Ok(()) => {
                self.stream = Some(SendStream(stream));
            }
            Err(e) => {
                self.running.store(false, Ordering::Relaxed);
                return Err(AppError::AudioCaptureFailed {
                    reason: format!("启动麦克风采集失败: {}", e),
                });
            }
        }

        Ok(())
    }

    fn stop(&mut self) -> AppResult<()> {
        self.stop_with_diagnostics()?;
        Ok(())
    }

    fn device_list(&self) -> AppResult<Vec<AudioDevice>> {
        let host = cpal::default_host();
        let default_device = host.default_input_device();
        let default_name = default_device
            .as_ref()
            .and_then(|d| d.name().ok())
            .unwrap_or_default();

        let devices = host
            .input_devices()
            .map_err(|e| AppError::AudioCaptureFailed {
                reason: format!("枚举音频设备失败: {}", e),
            })?;

        let mut result = Vec::new();
        for device in devices {
            if let Ok(name) = device.name() {
                result.push(AudioDevice {
                    id: name.clone(),
                    name: name.clone(),
                    is_default: name == default_name,
                });
            }
        }

        Ok(result)
    }

    fn capabilities(&self) -> AudioCapabilities {
        AudioCapabilities {
            supports_system_audio: false,
            supports_microphone: true,
        }
    }
}

/// Builds a cpal input stream for the given sample type.
///
/// The callback converts samples to f32 in [-1.0, 1.0] range, packs them
/// into `AudioChunk`s, and sends them through the sink.
fn build_input_stream<T: cpal::SizedSample>(
    device: &Device,
    config: &StreamConfig,
    sink: AudioChunkSink,
    running: Arc<AtomicBool>,
    callbacks_after_stop: Arc<AtomicU64>,
    sample_rate: u32,
    channels: u16,
    session_clock: Option<Arc<SessionClock>>,
) -> AppResult<cpal::Stream>
where
    f32: cpal::FromSample<T>,
{
    // Use lazy offset: the session offset is set on the first audio callback
    // (not at stream build time) to prevent build-delay from inflating timestamps.
    let sample_clock = Arc::new(crate::core::clock::AudioSampleClock::new(
        sample_rate,
        channels,
    ));

    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _info: &cpal::InputCallbackInfo| {
                if !running.load(Ordering::Relaxed) {
                    callbacks_after_stop.fetch_add(1, Ordering::Relaxed);
                    return;
                }

                let samples: Vec<f32> = data.iter().map(|s| s.to_sample::<f32>()).collect();

                if samples.is_empty() {
                    return;
                }

                // Lazy offset initialization on first callback.
                // This prevents stream-build delay from inflating the first chunk's timestamp.
                if let Some(ref session) = session_clock {
                    let buffer_frames = samples.len() as u64 / channels.max(1) as u64;
                    sample_clock.initialize_offset(session, buffer_frames);
                }

                let timestamp = sample_clock.timestamp_for_interleaved_sample_count(samples.len());

                let chunk = AudioChunk {
                    timestamp,
                    sample_rate,
                    channels,
                    samples: Arc::from(samples.into_boxed_slice()),
                };

                // Drop logging is handled by MediaSender with source="mic".
                // Don't use `let _` — the return value is intentionally not needed here
                // but we avoid silent ignore to satisfy BUG-005 rule 25.
                sink.try_send_drop_newest(chunk);
            },
            |err| {
                // Audio stream error — log but don't panic.
                eprintln!("麦克风采集错误: {}", err);
            },
            None,
        )
        .map_err(|e| AppError::AudioCaptureFailed {
            reason: format!("构建麦克风输入流失败: {}", e),
        })?;

    Ok(stream)
}
