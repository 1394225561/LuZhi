use std::path::PathBuf;

use crate::app::error::{AppError, AppResult};
use crate::core::frame::{MixedAudioChunk, VideoFrameRef};
use crate::media::recording_writer::{RecordingResult, RecordingWriter};

/// Production writer that uses FFmpeg to encode video and audio into a media file.
///
/// Currently a skeleton — full muxing requires additional implementation
/// with FFmpeg's C API bindings.
pub struct FfmpegRecordingWriter {
    output_path: PathBuf,
    frame_count: u64,
    mixed_audio_chunk_count: u64,
}

impl FfmpegRecordingWriter {
    pub fn new(output_path: PathBuf) -> AppResult<Self> {
        ffmpeg_next::init().map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("初始化 FFmpeg 失败: {e}"),
        })?;

        Ok(Self {
            output_path,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
        })
    }
}

impl RecordingWriter for FfmpegRecordingWriter {
    fn push_video(&mut self, _frame: VideoFrameRef) -> AppResult<()> {
        // TODO: Encode video frame via FFmpeg
        self.frame_count += 1;
        Ok(())
    }

    fn push_audio(&mut self, _chunk: MixedAudioChunk) -> AppResult<()> {
        // TODO: Encode audio chunk via FFmpeg
        self.mixed_audio_chunk_count += 1;
        Ok(())
    }

    fn finish(&mut self) -> AppResult<RecordingResult> {
        // TODO: Finalize muxing and close output file
        Ok(RecordingResult {
            duration_secs: 0,
            frame_count: self.frame_count,
            mixed_audio_chunk_count: self.mixed_audio_chunk_count,
            // This is a skeleton writer — no file is actually created.
            // Return None to avoid misleading callers into thinking a
            // playable artifact exists. The path is stored for future use
            // when FFmpeg muxing is implemented.
            output_path: None,
        })
    }
}
