use std::path::PathBuf;

use serde::Serialize;

use crate::app::error::AppResult;
use crate::core::frame::{MixedAudioChunk, VideoFrameRef};

/// Diagnostics from the FFmpeg writer worker thread.
///
/// Tracks what happened to audio chunks after they were enqueued,
/// distinguishing between "queued for encoding" and "actually encoded
/// into the AAC stream". This distinction is critical for BUG-005
/// diagnosis — `mixed_chunks_queued > 0` does not prove audio is in
/// the artifact.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterDiagnostics {
    /// Number of audio chunks received by the worker thread.
    pub audio_chunks_received: u64,
    /// Number of audio chunks that had at least some real PCM appended.
    pub audio_chunks_appended: u64,
    /// Number of audio chunks discarded as fully overlapped.
    pub audio_chunks_discarded_full_overlap: u64,
    /// Number of audio chunks partially trimmed before appending.
    pub audio_chunks_trimmed_partial_overlap: u64,
    /// Number of real (non-silence) mono frames appended to the sample buffer.
    pub audio_real_frames_appended: u64,
    /// Number of silence mono frames padded for timeline gaps.
    pub audio_silence_frames_padded: u64,
    /// Maximum RMS of real PCM samples before encoding (excludes silence padding).
    pub audio_real_rms_max_before_encode: f32,
    /// Number of AAC frames actually encoded and written to the muxer.
    pub aac_frames_encoded: u64,
    /// Number of silent AAC frames generated when no audio was received.
    pub silent_aac_frames_encoded: u64,
    /// Whether a silent AAC track was generated (no mixed audio chunks received).
    pub generated_silent_track: bool,
    /// Number of video queue full events (try_send failed).
    pub video_queue_full_count: u64,
    /// Number of audio queue full events (try_send failed).
    pub audio_queue_full_count: u64,
}

/// Audio diagnostics collected during a recording session.
///
/// Tracks requested audio sources, chunk counts, drop counts, and RMS levels
/// to help diagnose "silent audio" issues where requested sources don't appear
/// in the final artifact.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDiagnostics {
    /// Whether system audio was requested for this session.
    pub requested_system_audio: bool,
    /// Whether microphone was requested for this session.
    pub requested_microphone: bool,
    /// Microphone device name if specified (None = system default).
    pub microphone_device: Option<String>,
    /// Number of system audio chunks received from capture.
    pub system_chunks_received: u64,
    /// Number of microphone chunks received from capture.
    pub mic_chunks_received: u64,
    /// Number of system audio chunks dropped by media channel.
    pub system_chunks_dropped: u64,
    /// Number of microphone chunks dropped by media channel.
    pub mic_chunks_dropped: u64,
    /// Number of mixed audio chunks queued to writer (may not all be encoded).
    pub mixed_chunks_queued: u64,
    /// Number of writer push_audio failures.
    pub writer_push_audio_failures: u64,
    /// Maximum RMS observed in system audio chunks.
    pub system_rms_max: f32,
    /// Maximum RMS observed in microphone chunks.
    pub mic_rms_max: f32,
    /// Maximum RMS observed in mixed audio chunks.
    pub mixed_rms_max: f32,
    /// Whether a silent AAC track was generated (no mixed audio chunks).
    pub generated_silent_track: bool,
    /// Number of time windows that had both system and mic audio (paired).
    pub paired_window_count: u64,
    /// Number of time windows with system-only audio.
    pub system_only_window_count: u64,
    /// Number of time windows with mic-only audio.
    pub mic_only_window_count: u64,
    /// Number of windows emitted due to source stall timeout.
    pub source_timeout_window_count: u64,
    /// Maximum RMS of system audio before writer (from synchronizer output).
    pub system_rms_max_before_writer: f32,
    /// Maximum RMS of mic audio before writer (from synchronizer output).
    pub mic_rms_max_before_writer: f32,
    /// Number of system audio windows that reached writer (before push_audio).
    pub system_windows_before_writer: u64,
    /// Number of mic audio windows that reached writer (before push_audio).
    pub mic_windows_before_writer: u64,
    /// Number of system audio frames that reached writer (before push_audio).
    pub system_frames_before_writer: u64,
    /// Number of mic audio frames that reached writer (before push_audio).
    pub mic_frames_before_writer: u64,
}

/// Source-aware audio contract validation.
///
/// Unlike aggregate-only validation, this checks that each requested source
/// actually contributed to the artifact. Uses synchronizer diagnostics to
/// detect the BUG-005 pattern: capture-side RMS non-zero but writer discarded
/// all chunks from one source.
///
/// **Phase C fix**: source presence checks are based on chunk/window/frame
/// counts, NOT gated by RMS. RMS is only used for "capture had sound but
// writer lost it" detection, not for deciding whether to run presence checks.
pub fn validate_source_aware_audio_contract(
    diagnostics: &RecordingDiagnostics,
    writer_diagnostics: &WriterDiagnostics,
) -> crate::app::error::AppResult<()> {
    use crate::app::error::AppError;

    // --- System audio presence checks ---
    if diagnostics.requested_system_audio {
        if diagnostics.system_chunks_received == 0 {
            // Capture source missing entirely — this is a real failure.
            return Err(AppError::RecordingFinalizeFailed {
                reason: "requested system audio but 0 chunks received from capture".to_string(),
            });
        }
        // Chunks received — verify they reached the synchronizer and writer.
        let system_windows = diagnostics.system_only_window_count + diagnostics.paired_window_count;
        if system_windows == 0 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested system audio, received {} chunks, but 0 system windows emitted",
                    diagnostics.system_chunks_received
                ),
            });
        }
        if diagnostics.system_windows_before_writer == 0 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested system audio, received {} chunks, but 0 system windows reached writer",
                    diagnostics.system_chunks_received
                ),
            });
        }
        if diagnostics.system_frames_before_writer == 0 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested system audio, received {} chunks, but 0 system frames reached writer",
                    diagnostics.system_chunks_received
                ),
            });
        }
        // Capture had sound but writer-side RMS is 0 — something went wrong
        // between synchronizer and encoder.
        if diagnostics.system_rms_max > 0.001 && diagnostics.system_rms_max_before_writer < 0.001 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested system audio, capture RMS={:.6}, but RMS before writer is 0",
                    diagnostics.system_rms_max
                ),
            });
        }
    }

    // --- Microphone presence checks ---
    if diagnostics.requested_microphone {
        if diagnostics.mic_chunks_received == 0 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: "requested microphone but 0 chunks received from capture".to_string(),
            });
        }
        let mic_windows = diagnostics.mic_only_window_count + diagnostics.paired_window_count;
        if mic_windows == 0 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested microphone, received {} chunks, but 0 mic windows emitted",
                    diagnostics.mic_chunks_received
                ),
            });
        }
        if diagnostics.mic_windows_before_writer == 0 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested microphone, received {} chunks, but 0 mic windows reached writer",
                    diagnostics.mic_chunks_received
                ),
            });
        }
        if diagnostics.mic_frames_before_writer == 0 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested microphone, received {} chunks, but 0 mic frames reached writer",
                    diagnostics.mic_chunks_received
                ),
            });
        }
        if diagnostics.mic_rms_max > 0.001 && diagnostics.mic_rms_max_before_writer < 0.001 {
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested microphone, capture RMS={:.6}, but RMS before writer is 0",
                    diagnostics.mic_rms_max
                ),
            });
        }
    }

    // --- Writer discard check (symmetric for both sources) ---
    // If most audio chunks were discarded by writer (full-overlap), that's a
    // failure regardless of which source was requested.
    if writer_diagnostics.audio_chunks_received > 0
        && writer_diagnostics.audio_chunks_discarded_full_overlap > 0
    {
        let discard_ratio = writer_diagnostics.audio_chunks_discarded_full_overlap as f64
            / writer_diagnostics.audio_chunks_received as f64;
        if discard_ratio > 0.5 {
            let source_desc = match (
                diagnostics.requested_system_audio,
                diagnostics.requested_microphone,
            ) {
                (true, true) => "system+mic",
                (true, false) => "system",
                (false, true) => "mic",
                (false, false) => "none",
            };
            return Err(AppError::RecordingFinalizeFailed {
                reason: format!(
                    "requested {}, but {:.0}% of audio chunks were discarded as full overlap ({} of {})",
                    source_desc,
                    discard_ratio * 100.0,
                    writer_diagnostics.audio_chunks_discarded_full_overlap,
                    writer_diagnostics.audio_chunks_received
                ),
            });
        }
    }

    Ok(())
}

/// Result returned after finalizing a recording session.
#[derive(Clone, Debug, Serialize)]
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
    /// Diagnostics from the FFmpeg writer worker thread.
    pub writer_diagnostics: WriterDiagnostics,
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
            writer_diagnostics: WriterDiagnostics::default(),
        })
    }
}

/// Test writer that fails on configurable operations.
#[cfg(test)]
pub struct FailingRecordingWriter {
    fail_push_video: bool,
    fail_push_audio: bool,
    fail_finish: bool,
    frame_count: u64,
    audio_count: u64,
}

#[cfg(test)]
impl FailingRecordingWriter {
    pub fn new(fail_push_video: bool, fail_push_audio: bool, fail_finish: bool) -> Self {
        Self {
            fail_push_video,
            fail_push_audio,
            fail_finish,
            frame_count: 0,
            audio_count: 0,
        }
    }
}

#[cfg(test)]
impl RecordingWriter for FailingRecordingWriter {
    fn push_video(&mut self, _frame: VideoFrameRef) -> AppResult<()> {
        if self.fail_push_video {
            Err(crate::app::error::AppError::RecordingFinalizeFailed {
                reason: "fake push_video failure".to_string(),
            })
        } else {
            self.frame_count += 1;
            Ok(())
        }
    }

    fn push_audio(&mut self, _chunk: MixedAudioChunk) -> AppResult<()> {
        if self.fail_push_audio {
            Err(crate::app::error::AppError::RecordingFinalizeFailed {
                reason: "fake push_audio failure".to_string(),
            })
        } else {
            self.audio_count += 1;
            Ok(())
        }
    }

    fn finish(&mut self) -> AppResult<RecordingResult> {
        if self.fail_finish {
            Err(crate::app::error::AppError::RecordingFinalizeFailed {
                reason: "fake finish failure".to_string(),
            })
        } else {
            Ok(RecordingResult {
                duration_secs: 0,
                frame_count: self.frame_count,
                mixed_audio_chunk_count: self.audio_count,
                output_path: None,
                cursor_metadata_path: None,
                effect_timeline_path: None,
                trim_metadata_path: None,
                cut_timeline_path: None,
                writer_diagnostics: WriterDiagnostics::default(),
            })
        }
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
            stride_bytes: 8,
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
            writer_diagnostics: WriterDiagnostics::default(),
        };

        let json = serde_json::to_string(&result).unwrap();

        assert!(json.contains("\"cursorMetadataPath\":\"/tmp/cursor.json\""));
        assert!(json.contains("\"effectTimelinePath\":\"/tmp/effects.json\""));
        assert!(json.contains("\"trimMetadataPath\":\"/tmp/trim-metadata.json\""));
        assert!(json.contains("\"cutTimelinePath\":\"/tmp/cut-timeline.json\""));
    }

    #[test]
    fn failing_writer_push_video_returns_error() {
        use crate::core::frame::{FrameBuffer, MediaTimestamp, PixelFormat, VideoFrame};

        let mut writer = FailingRecordingWriter::new(true, false, false);
        let frame = Arc::new(VideoFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            width: 2,
            height: 2,
            stride_bytes: 8,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(Arc::from(vec![0u8; 16].into_boxed_slice())),
        });
        assert!(writer.push_video(frame).is_err());
    }

    #[test]
    fn failing_writer_push_audio_returns_error() {
        let mut writer = FailingRecordingWriter::new(false, true, false);
        let audio = MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(vec![0.0f32; 960].into_boxed_slice()),
        };
        assert!(writer.push_audio(audio).is_err());
    }

    #[test]
    fn failing_writer_finish_returns_error() {
        let mut writer = FailingRecordingWriter::new(false, false, true);
        assert!(writer.finish().is_err());
    }

    #[test]
    fn failing_writer_succeeds_when_not_configured_to_fail() {
        use crate::core::frame::{FrameBuffer, MediaTimestamp, PixelFormat, VideoFrame};

        let mut writer = FailingRecordingWriter::new(false, false, false);
        let frame = Arc::new(VideoFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            width: 2,
            height: 2,
            stride_bytes: 8,
            pixel_format: PixelFormat::Bgra8,
            buffer: FrameBuffer::Owned(Arc::from(vec![0u8; 16].into_boxed_slice())),
        });
        let audio = MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(vec![0.0f32; 960].into_boxed_slice()),
        };

        assert!(writer.push_video(frame).is_ok());
        assert!(writer.push_audio(audio).is_ok());
        let result = writer.finish().unwrap();
        assert_eq!(result.frame_count, 1);
        assert_eq!(result.mixed_audio_chunk_count, 1);
    }

    #[test]
    fn validate_source_aware_audio_contract_rejects_missing_system_before_writer() {
        let diag = RecordingDiagnostics {
            requested_system_audio: true,
            requested_microphone: true,
            system_chunks_received: 100,
            mic_chunks_received: 80,
            system_rms_max: 0.05,
            mic_rms_max: 0.10,
            mic_windows_before_writer: 5,
            mic_rms_max_before_writer: 0.08,
            system_windows_before_writer: 0, // 0 windows reached writer
            system_rms_max_before_writer: 0.0,
            paired_window_count: 5,
            ..Default::default()
        };
        let writer_diag = WriterDiagnostics::default();

        let result = validate_source_aware_audio_contract(&diag, &writer_diag);
        assert!(
            result.is_err(),
            "should reject when system requested, chunks received, but 0 before-writer windows"
        );
    }

    #[test]
    fn validate_source_aware_audio_contract_rejects_missing_mic_before_writer() {
        let diag = RecordingDiagnostics {
            requested_system_audio: true,
            requested_microphone: true,
            system_chunks_received: 100,
            mic_chunks_received: 80,
            system_rms_max: 0.05,
            mic_rms_max: 0.10,
            system_windows_before_writer: 5,
            system_rms_max_before_writer: 0.04,
            mic_windows_before_writer: 0, // 0 windows reached writer
            mic_rms_max_before_writer: 0.0,
            paired_window_count: 5,
            ..Default::default()
        };
        let writer_diag = WriterDiagnostics::default();

        let result = validate_source_aware_audio_contract(&diag, &writer_diag);
        assert!(
            result.is_err(),
            "should reject when mic requested, chunks received, but 0 before-writer windows"
        );
    }

    #[test]
    fn validate_source_aware_audio_contract_rejects_zero_rms_before_writer() {
        let diag = RecordingDiagnostics {
            requested_system_audio: true,
            requested_microphone: false,
            system_chunks_received: 100,
            system_rms_max: 0.05,
            system_windows_before_writer: 5,
            system_frames_before_writer: 1000,
            system_rms_max_before_writer: 0.0, // windows exist but RMS is 0
            paired_window_count: 5,
            ..Default::default()
        };
        let writer_diag = WriterDiagnostics::default();

        let result = validate_source_aware_audio_contract(&diag, &writer_diag);
        assert!(
            result.is_err(),
            "should reject when capture RMS non-zero but RMS before writer is 0"
        );
    }
}
