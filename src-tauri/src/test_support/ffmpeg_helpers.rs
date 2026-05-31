use std::path::PathBuf;

use crate::core::frame::{
    FrameBuffer, MediaTimestamp, MixedAudioChunk, PixelFormat, VideoFrame, VideoFrameRef,
};
use std::sync::Arc;

// Re-export from production module for backward compatibility.
#[cfg(feature = "ffmpeg")]
pub use crate::media::ffmpeg_common::{inspect_media_artifact, MediaArtifactInspection};

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

    for i in 0..num_frames {
        let ts = i * frame_duration;
        let frame = synthetic_video_frame_at(ts, width, height);
        writer.push_video(frame)?;
    }

    for i in 0..num_audio {
        let ts = i * audio_interval;
        let chunk = synthetic_audio_chunk_at(ts);
        writer.push_audio(chunk)?;
    }

    writer.finish()?;
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
