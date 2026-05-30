//! Production helpers for FFmpeg artifact inspection.
//!
//! Shared between writer, exporter, and integration tests.

use crate::app::error::{AppError, AppResult};

/// Inspection result for an FFmpeg-produced media file.
pub struct MediaArtifactInspection {
    pub file_size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_nanos: u64,
    pub has_video_stream: bool,
    pub has_audio_stream: bool,
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

    if ictx.duration() > 0 {
        duration_nanos = ictx.duration() as u64;
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

    Ok(())
}
