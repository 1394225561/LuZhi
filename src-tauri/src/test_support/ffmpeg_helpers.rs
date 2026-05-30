use std::path::PathBuf;

use crate::core::frame::{
    FrameBuffer, MediaTimestamp, MixedAudioChunk, PixelFormat, VideoFrame, VideoFrameRef,
};
use std::sync::Arc;

pub struct MediaArtifactInspection {
    pub file_size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_nanos: u64,
    pub has_video_stream: bool,
    pub has_audio_stream: bool,
}

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
        sample_rate: 44100,
        channels: 1,
        samples: Arc::from(vec![0.5f32; 1024].into_boxed_slice()),
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
        stride_bytes: stride as u32,
        pixel_format: PixelFormat::Bgra8,
        buffer: FrameBuffer::Owned(Arc::from(buffer.into_boxed_slice())),
    })
}

#[cfg(feature = "ffmpeg")]
fn synthetic_audio_chunk_at(timestamp_nanos: u64) -> MixedAudioChunk {
    MixedAudioChunk {
        timestamp: MediaTimestamp::from_nanos(timestamp_nanos),
        sample_rate: 44100,
        channels: 1,
        samples: Arc::from(vec![0.25f32; 1024].into_boxed_slice()),
    }
}

#[cfg(feature = "ffmpeg")]
pub fn inspect_media_artifact(path: &std::path::Path) -> crate::app::error::AppResult<MediaArtifactInspection> {
    use crate::app::error::{AppError, AppResult};

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
        let codecpar = stream.codecpar();
        match codecpar.medium() {
            ffmpeg_next::media::Type::Video => {
                has_video_stream = true;
                width = codecpar.width() as u32;
                height = codecpar.height() as u32;
            }
            ffmpeg_next::media::Type::Audio => {
                has_audio_stream = true;
            }
            _ => {}
        }
    }

    // Duration from container metadata
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
