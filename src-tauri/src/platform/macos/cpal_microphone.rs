use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, StreamConfig};

use crate::app::error::{AppError, AppResult};
use crate::core::capture::{
    AudioCapabilities, AudioCapture, AudioChunkSink, AudioConfig, AudioDevice,
};
use crate::core::clock::SessionClock;
use crate::core::frame::AudioChunk;

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
}

impl CpalMicrophoneCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
            running: Arc::new(AtomicBool::new(false)),
            session_clock: None,
        }
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
                sample_rate_val,
                channels_val,
                session_clock.clone(),
            ),
            SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &stream_config,
                sink.clone(),
                running.clone(),
                sample_rate_val,
                channels_val,
                session_clock.clone(),
            ),
            SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &stream_config,
                sink,
                running.clone(),
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

        stream.play().map_err(|e| AppError::AudioCaptureFailed {
            reason: format!("启动麦克风采集失败: {}", e),
        })?;

        self.running.store(true, Ordering::Relaxed);
        self.stream = Some(SendStream(stream));

        Ok(())
    }

    fn stop(&mut self) -> AppResult<()> {
        self.running.store(false, Ordering::Relaxed);
        self.stream = None;
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
    sample_rate: u32,
    channels: u16,
    session_clock: Option<Arc<SessionClock>>,
) -> AppResult<cpal::Stream>
where
    f32: cpal::FromSample<T>,
{
    // Use lazy offset: the session offset is set on the first audio callback
    // (not at stream build time) to prevent build-delay from inflating timestamps.
    let sample_clock = Arc::new(crate::core::clock::AudioSampleClock::new(sample_rate, channels));

    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _info: &cpal::InputCallbackInfo| {
                if !running.load(Ordering::Relaxed) {
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

                let _ = sink.try_send_drop_newest(chunk);
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
