//! Production helpers for FFmpeg artifact inspection and time-base conversion.
//!
//! Shared between writer, exporter, and integration tests.

use crate::app::error::{AppError, AppResult};
use ffmpeg_next::Rational;

/// Contract specifying which audio sources were requested for a recording.
///
/// Used by validation to distinguish "silent AAC is expected" from
/// "user requested audio but got silence" — the root cause of BUG-005.
#[derive(Clone, Debug)]
pub struct RequestedAudioContract {
    /// Whether system audio capture was requested.
    pub requested_system_audio: bool,
    /// Whether microphone capture was requested.
    pub requested_microphone: bool,
    /// Minimum decoded RMS to consider audio non-silent.
    /// Default: 0.003 (conservative threshold for audible content).
    pub min_rms: f64,
    /// Minimum decoded peak to consider audio non-silent.
    /// Default: 0.02 (conservative threshold for audible content).
    pub min_peak: f32,
    /// Higher threshold for "audible" audio — used for real-device diagnostics.
    /// Default 0.015. Aggregate RMS below this triggers a **warning** (not hard fail).
    ///
    /// Global decoded RMS is diluted by silence padding (leading gaps, middle gaps,
    /// tail padding to video end). A recording with real audio content can have
    /// global RMS well below this threshold. This field is NOT used as a hard gate;
    /// it only produces `eprintln!` warnings for diagnostics.
    pub audible_min_rms: f64,
}

impl Default for RequestedAudioContract {
    fn default() -> Self {
        Self {
            requested_system_audio: false,
            requested_microphone: false,
            min_rms: 0.003,
            min_peak: 0.02,
            audible_min_rms: 0.015,
        }
    }
}

impl RequestedAudioContract {
    /// Returns true if any audio source was requested.
    pub fn any_audio_requested(&self) -> bool {
        self.requested_system_audio || self.requested_microphone
    }
}

/// Inspection result for an FFmpeg-produced media file.
#[derive(Debug)]
pub struct MediaArtifactInspection {
    pub file_size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_nanos: u64,
    pub video_duration_nanos: u64,
    pub audio_duration_nanos: u64,
    pub video_frame_count: Option<u64>,
    pub video_avg_fps: Option<f64>,
    pub has_video_stream: bool,
    pub has_audio_stream: bool,
    /// Number of decoded audio samples (per channel).
    pub audio_sample_count: Option<u64>,
    /// RMS (root mean square) of decoded audio samples.
    pub audio_rms: Option<f64>,
    /// Peak absolute value of decoded audio samples.
    pub audio_peak: Option<f32>,
    /// Audio stream sample rate in Hz.
    pub audio_sample_rate: Option<u32>,
    /// Number of audio channels.
    pub audio_channels: Option<u16>,
}

/// Converts a PTS value in `time_base` units to nanoseconds.
///
/// `value * time_base.0 / time_base.1 * 1_000_000_000` — computed as
/// `value * time_base.0 * 1_000_000_000 / time_base.1` to avoid truncation.
pub fn time_base_units_to_nanos(value: i64, time_base: Rational) -> AppResult<i64> {
    if time_base.1 == 0 {
        return Err(AppError::ExportFailed {
            reason: "时间基分母为零".to_string(),
        });
    }
    // Use i128 to avoid overflow on the multiplication.
    let result =
        (value as i128) * (time_base.0 as i128) * 1_000_000_000i128 / (time_base.1 as i128);
    Ok(result as i64)
}

/// Converts nanoseconds to `time_base` units.
///
/// `nanos * time_base.1 / (time_base.0 * 1_000_000_000)` — computed as
/// `nanos * time_base.1 / time_base.0 / 1_000_000_000` to avoid truncation.
pub fn nanos_to_time_base_units(nanos: u64, time_base: Rational) -> AppResult<i64> {
    if time_base.0 == 0 {
        return Err(AppError::ExportFailed {
            reason: "时间基分子为零".to_string(),
        });
    }
    // Use i128 to avoid overflow.
    let result =
        (nanos as i128) * (time_base.1 as i128) / ((time_base.0 as i128) * 1_000_000_000i128);
    Ok(result as i64)
}

/// Opens the media file at `path` with FFmpeg and returns stream metadata.
///
/// Used by both the export pipeline (to validate output) and integration tests
/// (to verify produced artifacts).
pub fn inspect_media_artifact(path: &std::path::Path) -> AppResult<MediaArtifactInspection> {
    let metadata = std::fs::metadata(path).map_err(|e| AppError::RecordingWriteFailed {
        reason: format!("检查导出文件失败: {e}"),
    })?;

    let ictx = ffmpeg_next::format::input(path).map_err(|e| AppError::ExportFailed {
        reason: format!("打开导出文件进行检查失败: {e}"),
    })?;

    let mut has_video_stream = false;
    let mut has_audio_stream = false;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut duration_nanos = 0u64;
    let mut video_duration_nanos = 0u64;
    let mut audio_duration_nanos = 0u64;
    let mut video_frame_count = None;
    let mut video_avg_fps = None;
    let mut audio_sample_count = None;
    let mut audio_rms = None;
    let mut audio_peak = None;
    let mut audio_sample_rate = None;
    let mut audio_channels = None;

    for stream in ictx.streams() {
        let params = stream.parameters();
        let stream_tb = stream.time_base();
        // Compute stream-level duration in nanoseconds.
        let stream_dur_nanos = if stream.duration() > 0 {
            time_base_units_to_nanos(stream.duration(), stream_tb)
                .map(|n| n.max(0) as u64)
                .unwrap_or(0)
        } else {
            0
        };
        match params.medium() {
            ffmpeg_next::media::Type::Video => {
                has_video_stream = true;
                // SAFETY: ffmpeg-next 7.x Parameters does not expose width/height
                // safe accessors. We read AVCodecParameters.width/height directly,
                // which are plain integers set by the demuxer. No mutation occurs.
                let par = unsafe { &*params.as_ptr() };
                width = par.width as u32;
                height = par.height as u32;
                video_duration_nanos = stream_dur_nanos;
                // nb_frames may not always be available; best-effort read.
                let nb = stream.frames();
                if nb > 0 {
                    video_frame_count = Some(nb as u64);
                    if stream_dur_nanos > 0 {
                        video_avg_fps =
                            Some(nb as f64 / (stream_dur_nanos as f64 / 1_000_000_000.0));
                    }
                }
            }
            ffmpeg_next::media::Type::Audio => {
                has_audio_stream = true;
                audio_duration_nanos = stream_dur_nanos;
            }
            _ => {}
        }
    }

    // FFmpeg container duration is in AV_TIME_BASE units (microseconds).
    // Convert to nanoseconds by multiplying by 1000.
    if ictx.duration() > 0 {
        duration_nanos = (ictx.duration() as u64).saturating_mul(1000);
    } else {
        // Fallback: use first video/audio stream duration * stream time_base.
        for stream in ictx.streams() {
            if stream.duration() > 0 {
                let tb = stream.time_base();
                let nanos = time_base_units_to_nanos(stream.duration(), tb);
                if let Ok(n) = nanos {
                    if n > 0 {
                        duration_nanos = n as u64;
                        break;
                    }
                }
            }
        }
    }

    // Decode audio stream to compute RMS and peak if requested.
    if has_audio_stream {
        match decode_audio_stats(path) {
            Ok(stats) => {
                audio_sample_count = Some(stats.sample_count);
                audio_rms = Some(stats.rms);
                audio_peak = Some(stats.peak);
                audio_sample_rate = Some(stats.sample_rate);
                audio_channels = Some(stats.channels);
            }
            Err(e) => {
                // Non-fatal: log warning but don't fail inspection.
                eprintln!("警告: 解码音频统计信息失败: {e}");
            }
        }
    }

    Ok(MediaArtifactInspection {
        file_size_bytes: metadata.len(),
        width,
        height,
        duration_nanos,
        video_duration_nanos,
        audio_duration_nanos,
        video_frame_count,
        video_avg_fps,
        has_video_stream,
        has_audio_stream,
        audio_sample_count,
        audio_rms,
        audio_peak,
        audio_sample_rate,
        audio_channels,
    })
}

/// Decodes the audio stream and computes RMS and peak statistics.
struct AudioStats {
    sample_count: u64,
    rms: f64,
    peak: f32,
    sample_rate: u32,
    channels: u16,
}

fn decode_audio_stats(path: &std::path::Path) -> AppResult<AudioStats> {
    let mut ictx = ffmpeg_next::format::input(path).map_err(|e| AppError::ExportFailed {
        reason: format!("打开文件进行音频解码失败: {e}"),
    })?;

    // Find audio stream.
    let audio_stream_index = ictx
        .streams()
        .find(|s| s.parameters().medium() == ffmpeg_next::media::Type::Audio)
        .map(|s| s.index())
        .ok_or_else(|| AppError::ExportFailed {
            reason: "未找到音频流".to_string(),
        })?;

    let stream = ictx
        .stream(audio_stream_index)
        .ok_or_else(|| AppError::ExportFailed {
            reason: "无法获取音频流".to_string(),
        })?;

    let codecpar = stream.parameters();
    let decoder = ffmpeg_next::codec::context::Context::from_parameters(codecpar).map_err(|e| {
        AppError::ExportFailed {
            reason: format!("创建音频解码器失败: {e}"),
        }
    })?;

    let mut decoder = decoder
        .decoder()
        .audio()
        .map_err(|e| AppError::ExportFailed {
            reason: format!("打开音频解码器失败: {e}"),
        })?;

    let mut sample_count: u64 = 0;
    let mut sum_squares: f64 = 0.0;
    let mut peak: f32 = 0.0;

    /// Processes a decoded audio frame, accumulating RMS and peak stats.
    fn process_audio_frame(
        frame: &ffmpeg_next::frame::Audio,
        sample_count: &mut u64,
        sum_squares: &mut f64,
        peak: &mut f32,
    ) {
        let num_channels = frame.channels() as usize;
        let num_samples = frame.samples();
        for ch in 0..num_channels {
            let plane = frame.plane::<f32>(ch);
            for &sample in &plane[..num_samples] {
                let abs = sample.abs();
                if abs > *peak {
                    *peak = abs;
                }
                *sum_squares += (sample as f64) * (sample as f64);
                *sample_count += 1;
            }
        }
    }

    let mut packet_iter = ictx.packets();
    loop {
        match packet_iter.next() {
            Some((stream, packet)) => {
                if stream.index() != audio_stream_index {
                    continue;
                }
                decoder
                    .send_packet(&packet)
                    .map_err(|e| AppError::ExportFailed {
                        reason: format!("发送音频包到解码器失败: {e}"),
                    })?;

                let mut decoded = ffmpeg_next::frame::Audio::empty();
                while decoder.receive_frame(&mut decoded).is_ok() {
                    process_audio_frame(&decoded, &mut sample_count, &mut sum_squares, &mut peak);
                }
            }
            None => break,
        }
    }

    // Flush decoder.
    decoder.send_eof().ok();
    let mut decoded = ffmpeg_next::frame::Audio::empty();
    while decoder.receive_frame(&mut decoded).is_ok() {
        process_audio_frame(&decoded, &mut sample_count, &mut sum_squares, &mut peak);
    }

    let rms = if sample_count > 0 {
        (sum_squares / sample_count as f64).sqrt()
    } else {
        0.0
    };

    Ok(AudioStats {
        sample_count,
        rms,
        peak,
        sample_rate: decoder.rate(),
        channels: decoder.channels(),
    })
}

/// Opens the media file at `path` with FFmpeg and returns stream metadata
/// including decoded audio statistics (RMS, peak, sample count).
///
/// This function always decodes the audio stream to compute statistics,
/// unlike `inspect_media_artifact` which only decodes when audio stream exists.
/// Use this when you need to verify audio content (e.g., not silent).
pub fn inspect_media_artifact_with_audio_stats(
    path: &std::path::Path,
) -> AppResult<MediaArtifactInspection> {
    let mut inspection = inspect_media_artifact(path)?;

    // If we already have audio stats, return as-is.
    if inspection.audio_rms.is_some() {
        return Ok(inspection);
    }

    // Otherwise, try to decode audio stats.
    if inspection.has_audio_stream {
        match decode_audio_stats(path) {
            Ok(stats) => {
                inspection.audio_sample_count = Some(stats.sample_count);
                inspection.audio_rms = Some(stats.rms);
                inspection.audio_peak = Some(stats.peak);
            }
            Err(e) => {
                eprintln!("警告: 解码音频统计信息失败: {e}");
            }
        }
    }

    Ok(inspection)
}

/// Validates that an export artifact is playable:
/// - File size > 0
/// - Has video stream
/// - Has audio stream
/// - Dimensions match expected preset
/// - Duration > 0
pub fn validate_export_artifact(
    path: &std::path::Path,
    expected_width: u32,
    expected_height: u32,
) -> AppResult<()> {
    let inspection = inspect_media_artifact(path)?;

    if inspection.file_size_bytes == 0 {
        return Err(AppError::ExportFailed {
            reason: "导出文件为空".to_string(),
        });
    }
    if !inspection.has_video_stream {
        return Err(AppError::ExportFailed {
            reason: "导出文件缺少视频流".to_string(),
        });
    }
    if !inspection.has_audio_stream {
        return Err(AppError::ExportFailed {
            reason: "导出文件缺少音频流".to_string(),
        });
    }
    if inspection.width != expected_width || inspection.height != expected_height {
        return Err(AppError::ExportFailed {
            reason: format!(
                "导出分辨率不匹配：期望 {}×{}，实际 {}×{}",
                expected_width, expected_height, inspection.width, inspection.height
            ),
        });
    }
    if inspection.duration_nanos == 0 {
        return Err(AppError::ExportFailed {
            reason: "导出文件时长为零".to_string(),
        });
    }

    // Stream-level duration checks to catch PTS/muxer bugs (e.g., BUG-004).
    if inspection.video_duration_nanos == 0 {
        return Err(AppError::ExportFailed {
            reason: "导出视频流时长为零".to_string(),
        });
    }
    if inspection.has_audio_stream && inspection.audio_duration_nanos == 0 {
        return Err(AppError::ExportFailed {
            reason: "导出音频流时长为零".to_string(),
        });
    }
    // Reject if video and audio duration drift exceeds 500ms.
    // This catches the case where video packets are written with wrong
    // time base (e.g., 0.03s video with 19s audio).
    if inspection.has_audio_stream && inspection.video_duration_nanos > 0 {
        let drift = inspection
            .video_duration_nanos
            .abs_diff(inspection.audio_duration_nanos);
        if drift > 500_000_000 {
            return Err(AppError::ExportFailed {
                reason: format!(
                    "导出视频/音频时长偏差过大：视频 {}ms，音频 {}ms，偏差 {}ms",
                    inspection.video_duration_nanos / 1_000_000,
                    inspection.audio_duration_nanos / 1_000_000,
                    drift / 1_000_000
                ),
            });
        }
    }

    Ok(())
}

/// Validates that a source recording artifact is valid for export:
/// - File exists and size > 0
/// - Has video stream
/// - Has audio stream
/// - Duration > 0
pub fn validate_source_artifact(path: &std::path::Path) -> AppResult<MediaArtifactInspection> {
    let inspection = inspect_media_artifact(path)?;

    if inspection.file_size_bytes == 0 {
        return Err(AppError::RecordingWriteFailed {
            reason: "录制文件为空".to_string(),
        });
    }
    if !inspection.has_video_stream {
        return Err(AppError::RecordingWriteFailed {
            reason: "录制文件缺少视频流".to_string(),
        });
    }
    if !inspection.has_audio_stream {
        return Err(AppError::RecordingWriteFailed {
            reason: "录制文件缺少音频流".to_string(),
        });
    }
    if inspection.duration_nanos == 0 {
        return Err(AppError::RecordingWriteFailed {
            reason: "录制文件时长为零".to_string(),
        });
    }

    // Reject source artifacts with excessive A/V duration drift (>1s).
    // This catches the case where writer PTS model produces video duration
    // significantly shorter than audio duration.
    if inspection.has_audio_stream
        && inspection.video_duration_nanos > 0
        && inspection.audio_duration_nanos > 0
    {
        let drift = inspection
            .video_duration_nanos
            .abs_diff(inspection.audio_duration_nanos);
        if drift > 1_000_000_000 {
            return Err(AppError::RecordingWriteFailed {
                reason: format!(
                    "录制视频/音频时长偏差过大：视频 {}ms，音频 {}ms，偏差 {}ms",
                    inspection.video_duration_nanos / 1_000_000,
                    inspection.audio_duration_nanos / 1_000_000,
                    drift / 1_000_000
                ),
            });
        }
    }

    Ok(inspection)
}

/// Validates a source artifact with an audio content contract.
///
/// When the contract specifies that audio was requested (system and/or mic),
/// this function decodes the audio stream and verifies that the decoded
/// RMS/peak exceed the contract thresholds. This catches the case where
/// an AAC track exists but contains silence (BUG-005).
///
/// When no audio was requested, a silent AAC track is allowed but the
/// returned inspection will have `audio_rms ≈ 0`.
pub fn validate_source_artifact_with_audio_contract(
    path: &std::path::Path,
    contract: &RequestedAudioContract,
) -> AppResult<MediaArtifactInspection> {
    let inspection = validate_source_artifact(path)?;

    // Only enforce audio content contract when audio was actually requested.
    if !contract.any_audio_requested() {
        return Ok(inspection);
    }

    // Audio was requested — verify decoded content is non-silent.
    if !inspection.has_audio_stream {
        return Err(AppError::RecordingWriteFailed {
            reason: "请求了音频录制但文件缺少音频流".to_string(),
        });
    }

    let rms = inspection.audio_rms.unwrap_or(0.0);
    let peak = inspection.audio_peak.unwrap_or(0.0);
    let sample_count = inspection.audio_sample_count.unwrap_or(0);

    if sample_count == 0 {
        return Err(AppError::RecordingWriteFailed {
            reason: format!(
                "请求了音频录制但解码后无音频样本（system={}, mic={})",
                contract.requested_system_audio, contract.requested_microphone,
            ),
        });
    }

    if rms < contract.min_rms && peak < contract.min_peak {
        return Err(AppError::RecordingWriteFailed {
            reason: format!(
                "请求了音频录制但解码后音频近乎静音（RMS={:.6} < {:.6}，peak={:.6} < {:.6}，system={}, mic={}）",
                rms, contract.min_rms, peak, contract.min_peak,
                contract.requested_system_audio, contract.requested_microphone,
            ),
        });
    }

    // Level 2: audible diagnostics — warn when aggregate RMS is below the
    // audible threshold, but do NOT hard fail.
    //
    // Global decoded RMS is computed over the entire audio stream duration
    // including silence padding (leading gaps, middle gaps, tail padding to
    // video end). This dilutes the RMS significantly when real audio content
    // is short relative to the total timeline. A recording with real audible
    // content (peak > min_peak) can have global RMS well below audible_min_rms.
    //
    // The Level 1 check (min_rms/min_peak) already prevents truly silent
    // artifacts. This check only produces diagnostics warnings.
    if rms < contract.audible_min_rms {
        eprintln!(
            "警告: 请求了音频录制但 aggregate RMS 低于可听建议阈值（RMS={:.6} < {:.6}，peak={:.6}，system={}, mic={}）",
            rms,
            contract.audible_min_rms,
            peak,
            contract.requested_system_audio,
            contract.requested_microphone,
        );
    }

    Ok(inspection)
}

/// Validates an export artifact with an audio content contract.
///
/// Similar to `validate_source_artifact_with_audio_contract` but for the
/// export output. Checks dimensions, duration drift, and audio content.
pub fn validate_export_artifact_with_audio_contract(
    path: &std::path::Path,
    expected_width: u32,
    expected_height: u32,
    contract: &RequestedAudioContract,
) -> AppResult<()> {
    // Run the standard export validation first.
    validate_export_artifact(path, expected_width, expected_height)?;

    // Only enforce audio content contract when audio was actually requested.
    if !contract.any_audio_requested() {
        return Ok(());
    }

    // Decode and check audio content.
    let inspection = inspect_media_artifact_with_audio_stats(path)?;

    let rms = inspection.audio_rms.unwrap_or(0.0);
    let peak = inspection.audio_peak.unwrap_or(0.0);
    let sample_count = inspection.audio_sample_count.unwrap_or(0);

    if sample_count == 0 {
        return Err(AppError::ExportFailed {
            reason: format!(
                "导出文件请求了音频但解码后无音频样本（system={}, mic={}）",
                contract.requested_system_audio, contract.requested_microphone,
            ),
        });
    }

    if rms < contract.min_rms && peak < contract.min_peak {
        return Err(AppError::ExportFailed {
            reason: format!(
                "导出文件请求了音频但解码后近乎静音（RMS={:.6} < {:.6}，peak={:.6} < {:.6}）",
                rms, contract.min_rms, peak, contract.min_peak,
            ),
        });
    }

    // Level 2: audible diagnostics — warn only, do NOT hard fail.
    // Same rationale as source artifact: global RMS is diluted by silence padding.
    if rms < contract.audible_min_rms {
        eprintln!(
            "警告: 导出文件 aggregate RMS 低于可听建议阈值（RMS={:.6} < {:.6}，peak={:.6}）",
            rms, contract.audible_min_rms, peak,
        );
    }

    Ok(())
}

/// Maps an FFmpeg error to a user-facing `AppError`.
///
/// Handles common FFmpeg error codes with precise Chinese messages:
/// - `Eof`: end of stream (expected during decode loops)
/// - `EAGAIN` (POSIX): decoder needs more input (expected, not an error)
/// - Other errors: mapped to `ExportFailed` with the FFmpeg error description
///
/// # Usage
/// Use this when calling FFmpeg APIs that return `ffmpeg_next::Error` to
/// provide precise error context instead of generic "unknown error" messages.
pub fn ffmpeg_error_to_app_error(e: ffmpeg_next::Error, context: &str) -> AppError {
    use ffmpeg_next::util::error::EAGAIN;
    match e {
        ffmpeg_next::Error::Eof => AppError::ExportFailed {
            reason: format!("{context}: 流已结束"),
        },
        ffmpeg_next::Error::Other { errno } if errno == EAGAIN => AppError::ExportFailed {
            reason: format!("{context}: 解码器需要更多数据"),
        },
        ffmpeg_next::Error::Other { errno } => AppError::ExportFailed {
            reason: format!("{context}: FFmpeg 错误 {errno} ({e})"),
        },
        _ => AppError::ExportFailed {
            reason: format!("{context}: {e}"),
        },
    }
}

/// Maps an FFmpeg error to a recording `AppError`.
///
/// Similar to `ffmpeg_error_to_app_error` but for recording write operations.
pub fn ffmpeg_error_to_recording_error(e: ffmpeg_next::Error, context: &str) -> AppError {
    use ffmpeg_next::util::error::EAGAIN;
    match e {
        ffmpeg_next::Error::Eof => AppError::RecordingWriteFailed {
            reason: format!("{context}: 流已结束"),
        },
        ffmpeg_next::Error::Other { errno } if errno == EAGAIN => AppError::RecordingWriteFailed {
            reason: format!("{context}: 编码器需要更多数据"),
        },
        ffmpeg_next::Error::Other { errno } => AppError::RecordingWriteFailed {
            reason: format!("{context}: FFmpeg 错误 {errno} ({e})"),
        },
        _ => AppError::RecordingWriteFailed {
            reason: format!("{context}: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_base_to_nanos_rational_1_30() {
        // 1/30 time base: 1 unit = 1/30 second = 33_333_333.33 nanos
        let tb = Rational(1, 30);
        let nanos = time_base_units_to_nanos(1, tb).unwrap();
        assert_eq!(nanos, 33_333_333); // truncated
    }

    #[test]
    fn time_base_to_nanos_rational_1_48000() {
        // 1/48000 time base (audio): 1 unit = ~20833 nanos
        let tb = Rational(1, 48000);
        let nanos = time_base_units_to_nanos(48000, tb).unwrap();
        assert_eq!(nanos, 1_000_000_000); // 1 second
    }

    #[test]
    fn nanos_to_time_base_rational_1_30() {
        let tb = Rational(1, 30);
        let units = nanos_to_time_base_units(1_000_000_000, tb).unwrap();
        assert_eq!(units, 30); // 1 second = 30 frames at 1/30
    }

    #[test]
    fn nanos_to_time_base_rational_1_48000() {
        let tb = Rational(1, 48000);
        let units = nanos_to_time_base_units(1_000_000_000, tb).unwrap();
        assert_eq!(units, 48000); // 1 second = 48000 audio units
    }

    #[test]
    fn time_base_roundtrip() {
        let tb = Rational(1, 30);
        let original_nanos = 2_000_000_000i64; // 2 seconds
        let units = nanos_to_time_base_units(original_nanos as u64, tb).unwrap();
        let back = time_base_units_to_nanos(units, tb).unwrap();
        // Should be close (truncation may lose < 1 frame worth of precision).
        assert!((back - original_nanos).unsigned_abs() < 34_000_000); // < 1 frame
    }

    #[test]
    fn time_base_zero_denominator_returns_error() {
        let tb = Rational(1, 0);
        assert!(time_base_units_to_nanos(100, tb).is_err());
    }

    #[test]
    fn nanos_to_time_base_zero_numerator_returns_error() {
        let tb = Rational(0, 30);
        assert!(nanos_to_time_base_units(100, tb).is_err());
    }

    #[test]
    fn inspect_media_artifact_reports_nonzero_audio_rms() {
        use crate::test_support::ffmpeg_helpers::{
            create_synthetic_source_artifact_strict, unique_media_path,
        };

        let path = unique_media_path("audio-rms-test", "mp4");
        create_synthetic_source_artifact_strict(&path, 64, 48, 2_000_000_000).unwrap();

        let inspection = inspect_media_artifact(&path).unwrap();

        assert!(inspection.has_audio_stream);
        assert!(inspection.audio_rms.is_some());
        let rms = inspection.audio_rms.unwrap();
        assert!(
            rms > 0.01,
            "expected non-zero audio RMS for synthetic artifact, got {rms}"
        );
        assert!(inspection.audio_peak.is_some());
        assert!(inspection.audio_peak.unwrap() > 0.01);
        assert!(inspection.audio_sample_count.is_some());
        assert!(inspection.audio_sample_count.unwrap() > 0);
        assert!(inspection.audio_sample_rate.is_some());
        assert_eq!(inspection.audio_sample_rate.unwrap(), 48_000);
        assert!(inspection.audio_channels.is_some());
        assert_eq!(inspection.audio_channels.unwrap(), 2);

        // Cleanup.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn inspect_media_artifact_with_audio_stats_returns_stats() {
        use crate::test_support::ffmpeg_helpers::{
            create_synthetic_source_artifact_strict, unique_media_path,
        };

        let path = unique_media_path("audio-stats-test", "mp4");
        create_synthetic_source_artifact_strict(&path, 64, 48, 1_000_000_000).unwrap();

        let inspection = inspect_media_artifact_with_audio_stats(&path).unwrap();

        assert!(inspection.audio_rms.is_some());
        assert!(inspection.audio_rms.unwrap() > 0.01);

        // Cleanup.
        let _ = std::fs::remove_file(&path);
    }

    // --- RequestedAudioContract tests ---

    #[test]
    fn validate_source_artifact_allows_silent_track_when_no_audio_requested() {
        use crate::test_support::ffmpeg_helpers::{
            create_synthetic_source_artifact_strict, unique_media_path,
        };

        let path = unique_media_path("contract-no-audio", "mp4");
        // Create artifact with audio (non-silent).
        create_synthetic_source_artifact_strict(&path, 64, 48, 1_000_000_000).unwrap();

        let contract = RequestedAudioContract {
            requested_system_audio: false,
            requested_microphone: false,
            ..Default::default()
        };

        // Should succeed — no audio requested, so contract is trivially satisfied.
        let result = validate_source_artifact_with_audio_contract(&path, &contract);
        assert!(
            result.is_ok(),
            "should allow silent when no audio requested"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validate_source_artifact_rejects_silent_audio_when_audio_requested() {
        use crate::media::recording_writer::RecordingWriter;
        use crate::test_support::ffmpeg_helpers::unique_media_path;

        let path = unique_media_path("contract-silent-reject", "mp4");
        // Create a video-only artifact (no audio stream at all).
        {
            use crate::media::ffmpeg_writer::FfmpegRecordingWriter;
            use crate::test_support::ffmpeg_helpers::test_video_frame_at;
            let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();
            writer.push_video(test_video_frame_at(0)).unwrap();
            writer.finish().unwrap();
        }

        let contract = RequestedAudioContract {
            requested_system_audio: true,
            requested_microphone: false,
            ..Default::default()
        };

        // Should fail — audio was requested but artifact has only silent track.
        let result = validate_source_artifact_with_audio_contract(&path, &contract);
        assert!(
            result.is_err(),
            "should reject silent audio when audio requested"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("静音") || err_msg.contains("音频"),
            "error should mention silent audio: {err_msg}"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validate_source_artifact_accepts_non_silent_audio_when_requested() {
        use crate::test_support::ffmpeg_helpers::{
            create_synthetic_source_artifact_strict, unique_media_path,
        };

        let path = unique_media_path("contract-non-silent", "mp4");
        // Create artifact with non-silent audio.
        create_synthetic_source_artifact_strict(&path, 64, 48, 2_000_000_000).unwrap();

        let contract = RequestedAudioContract {
            requested_system_audio: true,
            requested_microphone: false,
            min_rms: 0.001,
            min_peak: 0.01,
            ..Default::default()
        };

        let result = validate_source_artifact_with_audio_contract(&path, &contract);
        assert!(
            result.is_ok(),
            "should accept non-silent audio: {:?}",
            result.err()
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validate_export_artifact_rejects_silent_audio_when_audio_requested() {
        use crate::media::recording_writer::RecordingWriter;
        use crate::test_support::ffmpeg_helpers::unique_media_path;

        let path = unique_media_path("contract-export-silent", "mp4");
        // Create a minimal artifact with video and silent audio.
        {
            use crate::media::ffmpeg_writer::FfmpegRecordingWriter;
            use crate::test_support::ffmpeg_helpers::test_video_frame_at;
            let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();
            for i in 0..3 {
                writer
                    .push_video(test_video_frame_at(i * 33_333_333))
                    .unwrap();
            }
            writer.finish().unwrap();
        }

        let contract = RequestedAudioContract {
            requested_system_audio: false,
            requested_microphone: true,
            ..Default::default()
        };

        // Export validation should also enforce the contract.
        let result = validate_export_artifact_with_audio_contract(&path, 1920, 1080, &contract);
        assert!(
            result.is_err(),
            "should reject silent export when audio requested"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn requested_audio_contract_any_audio_requested() {
        let c1 = RequestedAudioContract {
            requested_system_audio: false,
            requested_microphone: false,
            ..Default::default()
        };
        assert!(!c1.any_audio_requested());

        let c2 = RequestedAudioContract {
            requested_system_audio: true,
            requested_microphone: false,
            ..Default::default()
        };
        assert!(c2.any_audio_requested());

        let c3 = RequestedAudioContract {
            requested_system_audio: false,
            requested_microphone: true,
            ..Default::default()
        };
        assert!(c3.any_audio_requested());
    }

    #[test]
    fn synthetic_source_helper_strict_produces_valid_artifact() {
        // R5: Verify that the strict helper produces a valid artifact
        // when all pushes succeed (short duration to avoid queue overflow).
        use crate::test_support::ffmpeg_helpers::{
            create_synthetic_source_artifact_strict, unique_media_path,
        };

        let path = unique_media_path("strict-helper", "mp4");
        let result = create_synthetic_source_artifact_strict(&path, 64, 48, 500_000_000);
        assert!(
            result.is_ok(),
            "strict helper should succeed: {:?}",
            result.err()
        );

        let inspection = inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_video_stream);
        assert!(inspection.has_audio_stream);
        assert!(
            inspection.audio_rms.unwrap() > 0.01,
            "audio should be non-silent"
        );

        let _ = std::fs::remove_file(&path);
    }

    // --- BUG-005_2 low-RMS regression tests ---
    // These tests verify that artifacts with real but quiet audio content
    // (RMS between min_rms and audible_min_rms) pass validation without
    // hard failure. The audible check is now warning-only.

    #[test]
    fn requested_audio_contract_allows_low_but_non_silent_rms() {
        // BUG-005_2 regression: RMS ≈ 0.010, above min_rms=0.003 but below
        // audible_min_rms=0.015. Must NOT hard fail — the Level 1 check
        // (min_rms/min_peak) already prevents silent artifacts.
        use crate::test_support::ffmpeg_helpers::{
            create_synthetic_source_artifact_strict_with_amplitude, unique_media_path,
        };

        let path = unique_media_path("low-rms-source", "mp4");
        // Low amplitude (0.01) produces low but non-zero RMS.
        let result = create_synthetic_source_artifact_strict_with_amplitude(
            &path,
            64,
            48,
            2_000_000_000,
            0.01,
        );
        assert!(result.is_ok(), "helper should succeed: {:?}", result.err());

        let contract = RequestedAudioContract {
            requested_system_audio: true,
            requested_microphone: false,
            ..Default::default()
        };

        let result = validate_source_artifact_with_audio_contract(&path, &contract);
        assert!(
            result.is_ok(),
            "low-RMS artifact should pass (audible check is warning-only): {:?}",
            result.err()
        );

        let inspection = result.unwrap();
        let rms = inspection.audio_rms.unwrap();
        assert!(
            rms < 0.015,
            "test premise: RMS should be below audible_min_rms, got {rms}"
        );
        assert!(
            rms > 0.003,
            "test premise: RMS should be above min_rms, got {rms}"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_audio_contract_allows_low_but_non_silent_rms() {
        // BUG-005_2 regression: export validation also uses warning-only audible check.
        // Note: validate_export_artifact checks dimensions against expected values,
        // so we use the same dimensions as the helper (64×48).
        use crate::test_support::ffmpeg_helpers::{
            create_synthetic_source_artifact_strict_with_amplitude, unique_media_path,
        };

        let path = unique_media_path("low-rms-export", "mp4");
        let result = create_synthetic_source_artifact_strict_with_amplitude(
            &path,
            1920,
            1080,
            2_000_000_000,
            0.01,
        );
        assert!(result.is_ok(), "helper should succeed: {:?}", result.err());

        let contract = RequestedAudioContract {
            requested_system_audio: true,
            requested_microphone: false,
            ..Default::default()
        };

        let result = validate_export_artifact_with_audio_contract(&path, 1920, 1080, &contract);
        assert!(
            result.is_ok(),
            "low-RMS export should pass (audible check is warning-only): {:?}",
            result.err()
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn requested_audio_contract_still_rejects_near_silent_audio() {
        // Level 1 check (min_rms/min_peak) must still hard-fail for near-silent artifacts.
        use crate::media::recording_writer::RecordingWriter;
        use crate::test_support::ffmpeg_helpers::unique_media_path;

        let path = unique_media_path("near-silent-reject", "mp4");
        // Create a video-only artifact (no real audio content).
        {
            use crate::media::ffmpeg_writer::FfmpegRecordingWriter;
            use crate::test_support::ffmpeg_helpers::test_video_frame_at;
            let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();
            writer.push_video(test_video_frame_at(0)).unwrap();
            writer.push_video(test_video_frame_at(33_333_333)).unwrap();
            writer.finish().unwrap();
        }

        let contract = RequestedAudioContract {
            requested_system_audio: true,
            requested_microphone: false,
            ..Default::default()
        };

        let result = validate_source_artifact_with_audio_contract(&path, &contract);
        assert!(
            result.is_err(),
            "near-silent artifact should still be rejected by Level 1 check"
        );

        let _ = std::fs::remove_file(&path);
    }
}
