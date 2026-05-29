use std::path::PathBuf;

use serde::Serialize;

use crate::app::error::AppResult;
use crate::core::frame::{MixedAudioChunk, VideoFrameRef};

/// Result returned after finalizing a recording session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingResult {
    pub duration_secs: u64,
    pub frame_count: u64,
    pub mixed_audio_chunk_count: u64,
    pub output_path: Option<String>,
    pub cursor_metadata_path: Option<String>,
    pub effect_timeline_path: Option<String>,
    pub trim_metadata_path: Option<String>,
    pub cut_timeline_path: Option<String>,
}

/// Trait for writing recorded media to a file or other sink.
pub trait RecordingWriter: Send {
    fn push_video(&mut self, frame: VideoFrameRef) -> AppResult<()>;
    fn push_audio(&mut self, chunk: MixedAudioChunk) -> AppResult<()>;
    fn finish(&mut self) -> AppResult<RecordingResult>;
}

/// Test writer that counts pushed media without encoding.
#[derive(Default)]
pub struct CountingRecordingWriter {
    frame_count: u64,
    audio_count: u64,
    output_path: Option<PathBuf>,
}

impl CountingRecordingWriter {
    pub fn new(output_path: Option<PathBuf>) -> Self {
        Self {
            frame_count: 0,
            audio_count: 0,
            output_path,
        }
    }
}

impl RecordingWriter for CountingRecordingWriter {
    fn push_video(&mut self, _frame: VideoFrameRef) -> AppResult<()> {
        self.frame_count += 1;
        Ok(())
    }

    fn push_audio(&mut self, _chunk: MixedAudioChunk) -> AppResult<()> {
        self.audio_count += 1;
        Ok(())
    }

    fn finish(&mut self) -> AppResult<RecordingResult> {
        Ok(RecordingResult {
            duration_secs: 0,
            frame_count: self.frame_count,
            mixed_audio_chunk_count: self.audio_count,
            output_path: self
                .output_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::core::frame::{FrameBuffer, MediaTimestamp, PixelFormat, VideoFrame};

    #[test]
    fn counting_writer_reports_pushed_media() {
        let mut writer = CountingRecordingWriter::default();
        let frame = Arc::new(VideoFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            width: 2,
            height: 2,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(Arc::from(vec![0u8; 16].into_boxed_slice())),
        });
        let audio = MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(vec![0.0f32; 960].into_boxed_slice()),
        };

        writer.push_video(frame).unwrap();
        writer.push_audio(audio).unwrap();
        let result = writer.finish().unwrap();

        assert_eq!(result.frame_count, 1);
        assert_eq!(result.mixed_audio_chunk_count, 1);
        assert_eq!(result.cursor_metadata_path, None);
        assert_eq!(result.effect_timeline_path, None);
    }

    #[test]
    fn recording_result_serializes_sidecar_paths_as_camel_case() {
        let result = RecordingResult {
            duration_secs: 1,
            frame_count: 30,
            mixed_audio_chunk_count: 2,
            output_path: None,
            cursor_metadata_path: Some("/tmp/cursor.json".to_string()),
            effect_timeline_path: Some("/tmp/effects.json".to_string()),
            trim_metadata_path: Some("/tmp/trim-metadata.json".to_string()),
            cut_timeline_path: Some("/tmp/cut-timeline.json".to_string()),
        };

        let json = serde_json::to_string(&result).unwrap();

        assert!(json.contains("\"cursorMetadataPath\":\"/tmp/cursor.json\""));
        assert!(json.contains("\"effectTimelinePath\":\"/tmp/effects.json\""));
        assert!(json.contains("\"trimMetadataPath\":\"/tmp/trim-metadata.json\""));
        assert!(json.contains("\"cutTimelinePath\":\"/tmp/cut-timeline.json\""));
    }
}
