//! Production helpers for FFmpeg artifact inspection and time-base conversion.
//!
//! Shared between writer, exporter, and integration tests.

use crate::app::error::{AppError, AppResult};
use ffmpeg_next::Rational;

/// Inspection result for an FFmpeg-produced media file.
pub struct MediaArtifactInspection {
    pub file_size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_nanos: u64,
    pub has_video_stream: bool,
    pub has_audio_stream: bool,
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

    for stream in ictx.streams() {
        let params = stream.parameters();
        match params.medium() {
            ffmpeg_next::media::Type::Video => {
                has_video_stream = true;
                // SAFETY: ffmpeg-next 7.x Parameters does not expose width/height
                // safe accessors. We read AVCodecParameters.width/height directly,
                // which are plain integers set by the demuxer. No mutation occurs.
                let par = unsafe { &*params.as_ptr() };
                width = par.width as u32;
                height = par.height as u32;
            }
            ffmpeg_next::media::Type::Audio => {
                has_audio_stream = true;
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

    Ok(MediaArtifactInspection {
        file_size_bytes: metadata.len(),
        width,
        height,
        duration_nanos,
        has_video_stream,
        has_audio_stream,
    })
}

/// Validates that an export artifact is playable:
/// - File size > 0
/// - Has video stream
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

    Ok(())
}

/// Validates that a source recording artifact is valid for export:
/// - File exists and size > 0
/// - Has video stream
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
    if inspection.duration_nanos == 0 {
        return Err(AppError::RecordingWriteFailed {
            reason: "录制文件时长为零".to_string(),
        });
    }

    Ok(inspection)
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
}
