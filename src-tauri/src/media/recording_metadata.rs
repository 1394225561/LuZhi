use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

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
    use crate::core::timeline::{ClickPhase, CursorClick, CursorSample, MouseButton};

    #[test]
    fn metadata_round_trips_json() {
        let metadata = RecordingMetadata {
            fps: 30,
            duration_nanos: 1_000_000_000,
            cursor_samples: vec![CursorSample {
                timestamp: MediaTimestamp::from_nanos(0),
                x: 10.0,
                y: 20.0,
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
        };

        RecordingMetadataWriter::write_metadata(&path, &metadata).unwrap();
        let parsed = RecordingMetadataWriter::read_metadata(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(parsed, metadata);
    }
}
