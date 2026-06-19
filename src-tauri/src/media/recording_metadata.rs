use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app::cursor_metadata_runtime::CursorKindDiagnostics;
use crate::app::error::{AppError, AppResult};
use crate::core::timeline::{
    BeautifyConfigSnapshot, CaptureGeometry, CursorClick, CursorSample, EffectTimeline,
};

/// Recording sidecar metadata saved next to the intermediate recording artifact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingMetadata {
    pub fps: u32,
    pub duration_nanos: u64,
    pub cursor_samples: Vec<CursorSample>,
    pub cursor_clicks: Vec<CursorClick>,
    pub beautify_config: BeautifyConfigSnapshot,
    #[serde(default)]
    pub cursor_snapshot_success_count: u64,
    #[serde(default)]
    pub cursor_snapshot_error_count: u64,
    /// Display geometry captured at recording start. `None` for legacy metadata.
    #[serde(default)]
    pub capture_geometry: Option<CaptureGeometry>,
    /// Cursor kind distribution and AX query diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub cursor_kind_diagnostics: Option<CursorKindDiagnostics>,
    /// Media timeline alignment diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub media_timeline_diagnostics: Option<MediaTimelineDiagnostics>,
    /// Cursor sampling rate and coverage diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub cursor_timing_diagnostics: Option<CursorTimingDiagnostics>,
    /// The raw PTS (in nanoseconds) of the first video frame from CMSampleBuffer.
    /// Used by trim_exporter to align decoded source PTS with cursor timeline.
    #[serde(default)]
    pub source_pts_origin_nanos: u64,
}

/// Diagnostics for aligning video frame PTS, cursor timestamps, and export overlay timestamps.
/// Allows post-hoc analysis of whether cursor overlay is using the correct timebase.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTimelineDiagnostics {
    /// First valid video PTS from CMSampleBuffer (raw, before normalization).
    pub first_video_pts_nanos_raw: u64,
    /// Session clock value when first video frame callback was entered.
    pub first_video_session_entry_nanos: u64,
    /// First normalized video timestamp (after pts_origin mapping).
    pub first_video_normalized_nanos: u64,
    /// Last normalized video timestamp written to source MP4.
    pub last_video_normalized_nanos: u64,
    /// Total video frames written.
    pub video_frame_count: u64,
    /// First cursor sample timestamp (from SessionClock).
    pub first_cursor_sample_nanos: u64,
    /// Last cursor sample timestamp.
    pub last_cursor_sample_nanos: u64,
    /// Total cursor samples recorded.
    pub cursor_sample_count: u64,
    /// Actual CVPixelBuffer size on first frame (width, height).
    pub first_frame_actual_size: Option<(u32, u32)>,
}

impl Default for MediaTimelineDiagnostics {
    fn default() -> Self {
        Self {
            first_video_pts_nanos_raw: 0,
            first_video_session_entry_nanos: 0,
            first_video_normalized_nanos: 0,
            last_video_normalized_nanos: 0,
            video_frame_count: 0,
            first_cursor_sample_nanos: 0,
            last_cursor_sample_nanos: 0,
            cursor_sample_count: 0,
            first_frame_actual_size: None,
        }
    }
}

/// Diagnostics for cursor sampling rate and coverage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorTimingDiagnostics {
    /// Minimum interval between consecutive cursor samples (nanos).
    pub sample_interval_min_nanos: u64,
    /// Maximum interval between consecutive cursor samples (nanos).
    pub sample_interval_max_nanos: u64,
    /// Average interval between consecutive cursor samples (nanos).
    pub sample_interval_avg_nanos: u64,
    /// Number of cursor samples that were outside the capture geometry.
    pub samples_outside_geometry: u64,
    /// Accessibility permission status at recording start.
    pub accessibility_permission_at_start: String,
}

impl Default for CursorTimingDiagnostics {
    fn default() -> Self {
        Self {
            sample_interval_min_nanos: 0,
            sample_interval_max_nanos: 0,
            sample_interval_avg_nanos: 0,
            samples_outside_geometry: 0,
            accessibility_permission_at_start: "unknown".to_string(),
        }
    }
}

/// JSON sidecar reader/writer for cursor metadata and effect timelines.
pub struct RecordingMetadataWriter;

impl RecordingMetadataWriter {
    pub fn write_metadata(path: &Path, metadata: &RecordingMetadata) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::RecordingWriteFailed {
                reason: error.to_string(),
            })?;
        }

        let json = serde_json::to_string_pretty(metadata).map_err(|error| {
            AppError::RecordingWriteFailed {
                reason: error.to_string(),
            }
        })?;

        fs::write(path, json).map_err(|error| AppError::RecordingWriteFailed {
            reason: error.to_string(),
        })
    }

    pub fn read_metadata(path: &Path) -> AppResult<RecordingMetadata> {
        let json = fs::read_to_string(path).map_err(|error| AppError::RecordingWriteFailed {
            reason: error.to_string(),
        })?;

        serde_json::from_str(&json).map_err(|error| AppError::RecordingWriteFailed {
            reason: error.to_string(),
        })
    }

    pub fn write_effect_timeline(path: &Path, timeline: &EffectTimeline) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::RecordingWriteFailed {
                reason: error.to_string(),
            })?;
        }

        let json = serde_json::to_string_pretty(timeline).map_err(|error| {
            AppError::RecordingWriteFailed {
                reason: error.to_string(),
            }
        })?;

        fs::write(path, json).map_err(|error| AppError::RecordingWriteFailed {
            reason: error.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::{ClickPhase, CursorClick, CursorKind, CursorSample, MouseButton};

    #[test]
    fn metadata_round_trips_json() {
        let metadata = RecordingMetadata {
            fps: 30,
            duration_nanos: 1_000_000_000,
            cursor_samples: vec![CursorSample {
                timestamp: MediaTimestamp::from_nanos(0),
                x: 10.0,
                y: 20.0,
                kind: CursorKind::default(),
            }],
            cursor_clicks: vec![CursorClick {
                timestamp: MediaTimestamp::from_nanos(10_000_000),
                button: MouseButton::Left,
                phase: ClickPhase::Down,
                x: 10.0,
                y: 20.0,
            }],
            beautify_config: BeautifyConfigSnapshot {
                cursor_magnification: true,
                magnification_factor: 2.0,
                cursor_smoothing: true,
                auto_trim_silences: false,
                trim_sensitivity: "medium".to_string(),
                raw_system_cursor_visible: false,
            },
            cursor_snapshot_success_count: 1,
            cursor_snapshot_error_count: 0,
            capture_geometry: None,
            cursor_kind_diagnostics: None,
            media_timeline_diagnostics: None,
            cursor_timing_diagnostics: None,
            source_pts_origin_nanos: 0,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: RecordingMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, metadata);
    }

    #[test]
    fn write_and_read_metadata_file() {
        let path = std::env::temp_dir().join("luzhi-metadata-test.json");
        let metadata = RecordingMetadata {
            fps: 60,
            duration_nanos: 50_000_000,
            cursor_samples: vec![],
            cursor_clicks: vec![],
            beautify_config: BeautifyConfigSnapshot {
                cursor_magnification: true,
                magnification_factor: 2.0,
                cursor_smoothing: true,
                auto_trim_silences: false,
                trim_sensitivity: "medium".to_string(),
                raw_system_cursor_visible: false,
            },
            cursor_snapshot_success_count: 0,
            cursor_snapshot_error_count: 5,
            capture_geometry: None,
            cursor_kind_diagnostics: None,
            media_timeline_diagnostics: None,
            cursor_timing_diagnostics: None,
            source_pts_origin_nanos: 0,
        };

        RecordingMetadataWriter::write_metadata(&path, &metadata).unwrap();
        let parsed = RecordingMetadataWriter::read_metadata(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(parsed, metadata);
    }

    #[test]
    fn recording_metadata_serializes_media_timeline_diagnostics() {
        let metadata = RecordingMetadata {
            fps: 30,
            duration_nanos: 5_000_000_000,
            cursor_samples: vec![],
            cursor_clicks: vec![],
            beautify_config: BeautifyConfigSnapshot {
                cursor_magnification: false,
                magnification_factor: 1.0,
                cursor_smoothing: false,
                auto_trim_silences: false,
                trim_sensitivity: "medium".to_string(),
                raw_system_cursor_visible: false,
            },
            cursor_snapshot_success_count: 0,
            cursor_snapshot_error_count: 0,
            capture_geometry: None,
            cursor_kind_diagnostics: None,
            media_timeline_diagnostics: Some(MediaTimelineDiagnostics {
                first_video_pts_nanos_raw: 100_000_000,
                first_video_session_entry_nanos: 50_000_000,
                first_video_normalized_nanos: 0,
                last_video_normalized_nanos: 5_000_000_000,
                video_frame_count: 150,
                first_cursor_sample_nanos: 10_000_000,
                last_cursor_sample_nanos: 5_010_000_000,
                cursor_sample_count: 500,
                first_frame_actual_size: Some((1920, 1080)),
            }),
            cursor_timing_diagnostics: Some(CursorTimingDiagnostics {
                sample_interval_min_nanos: 8_000_000,
                sample_interval_max_nanos: 12_000_000,
                sample_interval_avg_nanos: 10_000_000,
                samples_outside_geometry: 0,
                accessibility_permission_at_start: "granted".to_string(),
            }),
            source_pts_origin_nanos: 0,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: RecordingMetadata = serde_json::from_str(&json).unwrap();

        let mtd = deserialized.media_timeline_diagnostics.unwrap();
        assert_eq!(mtd.first_video_pts_nanos_raw, 100_000_000);
        assert_eq!(mtd.video_frame_count, 150);
        assert_eq!(mtd.first_frame_actual_size, Some((1920, 1080)));

        let ctd = deserialized.cursor_timing_diagnostics.unwrap();
        assert_eq!(ctd.sample_interval_avg_nanos, 10_000_000);
        assert_eq!(ctd.accessibility_permission_at_start, "granted");
    }

    #[test]
    fn new_fields_default_when_absent_from_json() {
        // Simulate legacy metadata without new fields.
        let json = r#"{
            "fps": 30,
            "durationNanos": 1000000000,
            "cursorSamples": [],
            "cursorClicks": [],
            "beautifyConfig": {
                "cursorMagnification": false,
                "magnificationFactor": 1.0,
                "cursorSmoothing": false,
                "autoTrimSilences": false,
                "trimSensitivity": "medium",
                "rawSystemCursorVisible": false
            }
        }"#;

        let parsed: RecordingMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.fps, 30);
        assert!(parsed.media_timeline_diagnostics.is_none());
        assert!(parsed.cursor_timing_diagnostics.is_none());
    }
}
