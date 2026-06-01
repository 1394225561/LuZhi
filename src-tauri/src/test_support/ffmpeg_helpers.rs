use std::path::PathBuf;

use crate::core::frame::{
    FrameBuffer, MediaTimestamp, MixedAudioChunk, PixelFormat, VideoFrame, VideoFrameRef,
};
use std::sync::Arc;

// Re-export from production module for backward compatibility.
#[cfg(feature = "ffmpeg")]
pub use crate::media::ffmpeg_common::{
    inspect_media_artifact, inspect_media_artifact_with_audio_stats, MediaArtifactInspection,
    RequestedAudioContract, validate_source_artifact_with_audio_contract,
    validate_export_artifact_with_audio_contract,
};

pub fn unique_media_path(prefix: &str, extension: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir()
        .join("luzhi-test-artifacts")
        .join(format!("{prefix}-{millis}-{seq}.{extension}"))
}

pub fn test_video_frame_at(timestamp_nanos: u64) -> VideoFrameRef {
    // 2x2 BGRA frame
    let buffer = vec![0u8; 16]; // 2*2*4 bytes
    Arc::new(VideoFrame {
        timestamp: MediaTimestamp::from_nanos(timestamp_nanos),
        width: 2,
        height: 2,
        stride_bytes: 8,
        pixel_format: PixelFormat::Bgra8,
        buffer: FrameBuffer::Owned(Arc::from(buffer.into_boxed_slice())),
    })
}

pub fn test_audio_chunk_at(timestamp_nanos: u64) -> MixedAudioChunk {
    MixedAudioChunk {
        timestamp: MediaTimestamp::from_nanos(timestamp_nanos),
        sample_rate: 48_000,
        channels: 2,
        // 1024 samples per channel, stereo interleaved = 2048 total
        samples: Arc::from(vec![0.5f32; 2048].into_boxed_slice()),
    }
}

/// Creates a synthetic source artifact, tolerating queue-full push errors.
///
/// Use this for tests that don't need to verify every frame/chunk was accepted
/// (e.g., backpressure tests, smoke tests). For audio content verification,
/// use `create_synthetic_source_artifact_strict` instead.
#[cfg(feature = "ffmpeg")]
pub fn create_synthetic_source_artifact(
    path: &std::path::Path,
    width: u32,
    height: u32,
    duration_nanos: u64,
) -> crate::app::error::AppResult<()> {
    use crate::app::error::AppError;
    use crate::media::recording_writer::RecordingWriter;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("创建测试目录失败: {e}"),
        })?;
    }

    let mut writer = crate::media::ffmpeg_writer::FfmpegRecordingWriter::new(path.to_path_buf())?;

    let fps = 30u64;
    let frame_duration = 1_000_000_000 / fps;
    let num_frames = (duration_nanos / frame_duration).max(1);
    let audio_interval = 20_000_000; // 20ms audio chunks
    let num_audio = (duration_nanos / audio_interval).max(1);

    // Push video frames, tolerating queue-full errors (non-blocking send).
    for i in 0..num_frames {
        let ts = i * frame_duration;
        let frame = synthetic_video_frame_at(ts, width, height);
        let _ = writer.push_video(frame); // Ignore queue-full errors
    }

    // Give the encoder worker time to process video before audio.
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Push audio chunks, tolerating queue-full errors (non-blocking send).
    for i in 0..num_audio {
        let ts = i * audio_interval;
        let chunk = synthetic_audio_chunk_at(ts);
        let _ = writer.push_audio(chunk); // Ignore queue-full errors
    }

    let result = writer.finish()?;
    verify_artifact_output(path, &result)?;
    Ok(())
}

/// Creates a synthetic source artifact with strict push error checking.
///
/// Every push_video and push_audio call must succeed — any queue-full error
/// causes an immediate Err return. Use this for tests that verify audio
/// content (RMS/peak) where dropped frames/chunks would invalidate the test.
#[cfg(feature = "ffmpeg")]
pub fn create_synthetic_source_artifact_strict(
    path: &std::path::Path,
    width: u32,
    height: u32,
    duration_nanos: u64,
) -> crate::app::error::AppResult<()> {
    use crate::app::error::AppError;
    use crate::media::recording_writer::RecordingWriter;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("创建测试目录失败: {e}"),
        })?;
    }

    let mut writer = crate::media::ffmpeg_writer::FfmpegRecordingWriter::new(path.to_path_buf())?;

    let fps = 30u64;
    let frame_duration = 1_000_000_000 / fps;
    let num_frames = (duration_nanos / frame_duration).max(1);
    let audio_interval = 20_000_000; // 20ms audio chunks
    let num_audio = (duration_nanos / audio_interval).max(1);

    // Push video frames with pacing to avoid queue overflow.
    // In test environments frames are pushed much faster than real-time,
    // so we add small sleeps to let the encoder worker keep up.
    for i in 0..num_frames {
        let ts = i * frame_duration;
        let frame = synthetic_video_frame_at(ts, width, height);
        writer.push_video(frame).map_err(|e| {
            AppError::RecordingWriteFailed {
                reason: format!("strict helper: push_video 失败 (frame {i}): {e}"),
            }
        })?;
        // Pace: every 5 frames, yield to encoder worker.
        if i % 5 == 0 && i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    // Give the encoder worker time to drain the video queue before audio.
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Push audio chunks with pacing.
    for i in 0..num_audio {
        let ts = i * audio_interval;
        let chunk = synthetic_audio_chunk_at(ts);
        writer.push_audio(chunk).map_err(|e| {
            AppError::RecordingWriteFailed {
                reason: format!("strict helper: push_audio 失败 (chunk {i}): {e}"),
            }
        })?;
        // Pace: every 5 chunks, yield to encoder worker.
        if i % 5 == 0 && i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    let result = writer.finish()?;
    verify_artifact_output(path, &result)?;
    Ok(())
}

/// Shared output verification for both tolerant and strict helpers.
#[cfg(feature = "ffmpeg")]
fn verify_artifact_output(
    path: &std::path::Path,
    result: &crate::media::recording_writer::RecordingResult,
) -> crate::app::error::AppResult<()> {
    use crate::app::error::AppError;

    if result.output_path.as_deref() != Some(path.to_string_lossy().as_ref()) {
        return Err(AppError::RecordingWriteFailed {
            reason: format!(
                "测试 helper 输出路径不匹配：期望 {:?}，实际 {:?}",
                path, result.output_path
            ),
        });
    }
    // Self-verify artifact has valid video + audio streams and non-zero duration.
    let inspected = inspect_media_artifact(path)?;
    if !inspected.has_video_stream || !inspected.has_audio_stream || inspected.duration_nanos == 0 {
        return Err(AppError::RecordingWriteFailed {
            reason: format!(
                "测试 helper 生成的 artifact 无效：video={}, audio={}, duration={}ns",
                inspected.has_video_stream, inspected.has_audio_stream, inspected.duration_nanos
            ),
        });
    }
    Ok(())
}

#[cfg(feature = "ffmpeg")]
fn synthetic_video_frame_at(timestamp_nanos: u64, width: u32, height: u32) -> VideoFrameRef {
    let stride = width as usize * 4;
    let buffer_size = stride * height as usize;
    let mut buffer = vec![0u8; buffer_size];
    // Fill with a simple pattern to ensure non-zero content
    for (i, byte) in buffer.iter_mut().enumerate() {
        *byte = (i % 256) as u8;
    }
    Arc::new(VideoFrame {
        timestamp: MediaTimestamp::from_nanos(timestamp_nanos),
        width,
        height,
        stride_bytes: stride,
        pixel_format: PixelFormat::Bgra8,
        buffer: FrameBuffer::Owned(Arc::from(buffer.into_boxed_slice())),
    })
}

#[cfg(feature = "ffmpeg")]
fn synthetic_audio_chunk_at(timestamp_nanos: u64) -> MixedAudioChunk {
    MixedAudioChunk {
        timestamp: MediaTimestamp::from_nanos(timestamp_nanos),
        sample_rate: 48_000,
        channels: 2,
        // 1024 samples per channel, stereo interleaved = 2048 total
        samples: Arc::from(vec![0.25f32; 2048].into_boxed_slice()),
    }
}
