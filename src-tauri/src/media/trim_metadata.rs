use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app::error::{AppError, AppResult};
use crate::core::cut::{AudioActivitySample, CutTimeline, FrameDiffSample};

/// Recording-time metadata used to build a CutTimeline after recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimMetadata {
    pub duration_nanos: u64,
    pub audio_activity: Vec<AudioActivitySample>,
    pub visual_activity: Vec<FrameDiffSample>,
    /// Number of audio activity samples dropped due to cap overflow.
    #[serde(default)]
    pub audio_activity_dropped_count: u64,
    /// Number of visual activity samples dropped due to cap overflow.
    #[serde(default)]
    pub visual_activity_dropped_count: u64,
    /// Whether activity sampling was truncated due to cap overflow.
    /// When true, cut timeline analysis may not cover the full recording.
    #[serde(default)]
    pub activity_truncated: bool,
}

/// JSON sidecar reader/writer for trim metadata and cut timelines.
pub struct TrimMetadataWriter;

impl TrimMetadataWriter {
    pub fn write_metadata(path: &Path, metadata: &TrimMetadata) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::RecordingWriteFailed {
                reason: format!("创建裁剪元数据目录失败: {error}"),
            })?;
        }
        let json = serde_json::to_string_pretty(metadata).map_err(|error| {
            AppError::RecordingWriteFailed {
                reason: format!("序列化裁剪元数据失败: {error}"),
            }
        })?;
        fs::write(path, json).map_err(|error| AppError::RecordingWriteFailed {
            reason: format!("写入裁剪元数据失败: {error}"),
        })
    }

    pub fn read_metadata(path: &Path) -> AppResult<TrimMetadata> {
        let json = fs::read_to_string(path).map_err(|error| AppError::TrimProcessingFailed {
            reason: format!("读取裁剪元数据失败: {error}"),
        })?;
        serde_json::from_str(&json).map_err(|error| AppError::TrimProcessingFailed {
            reason: format!("解析裁剪元数据失败: {error}"),
        })
    }

    pub fn write_cut_timeline(path: &Path, timeline: &CutTimeline) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| AppError::RecordingWriteFailed {
                reason: format!("创建裁剪时间线目录失败: {error}"),
            })?;
        }
        let json = serde_json::to_string_pretty(timeline).map_err(|error| {
            AppError::RecordingWriteFailed {
                reason: format!("序列化裁剪时间线失败: {error}"),
            }
        })?;
        fs::write(path, json).map_err(|error| AppError::RecordingWriteFailed {
            reason: format!("写入裁剪时间线失败: {error}"),
        })
    }

    pub fn read_cut_timeline(path: &Path) -> AppResult<CutTimeline> {
        let json = fs::read_to_string(path).map_err(|error| AppError::TrimProcessingFailed {
            reason: format!("读取裁剪时间线失败: {error}"),
        })?;
        serde_json::from_str(&json).map_err(|error| AppError::TrimProcessingFailed {
            reason: format!("解析裁剪时间线失败: {error}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cut::{AudioActivitySample, FrameDiffSample};
    use crate::core::frame::MediaTimestamp;

    #[test]
    fn trim_metadata_round_trips_json() {
        let metadata = TrimMetadata {
            duration_nanos: 10_000_000_000,
            audio_activity: vec![AudioActivitySample {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(1_000_000_000),
                rms: 0.01,
            }],
            visual_activity: vec![FrameDiffSample {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(33_333_333),
                change_ratio: 0.002,
            }],
            audio_activity_dropped_count: 0,
            visual_activity_dropped_count: 0,
            activity_truncated: false,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: TrimMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, metadata);
    }

    #[test]
    fn trim_metadata_truncation_fields_serialize() {
        let metadata = TrimMetadata {
            duration_nanos: 10_000_000_000,
            audio_activity: Vec::new(),
            visual_activity: Vec::new(),
            audio_activity_dropped_count: 42,
            visual_activity_dropped_count: 7,
            activity_truncated: true,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"audioActivityDroppedCount\":42"));
        assert!(json.contains("\"visualActivityDroppedCount\":7"));
        assert!(json.contains("\"activityTruncated\":true"));
    }

    #[test]
    fn trim_metadata_truncation_fields_default_when_missing() {
        // Simulates reading a legacy sidecar without truncation fields.
        let json = r#"{"durationNanos":1000,"audioActivity":[],"visualActivity":[]}"#;
        let parsed: TrimMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.audio_activity_dropped_count, 0);
        assert_eq!(parsed.visual_activity_dropped_count, 0);
        assert!(!parsed.activity_truncated);
    }

    #[test]
    fn write_and_read_cut_timeline_file() {
        let path = std::env::temp_dir().join("luzhi-cut-timeline-test.json");
        let timeline = CutTimeline::empty(5_000_000_000);

        TrimMetadataWriter::write_cut_timeline(&path, &timeline).unwrap();
        let parsed = TrimMetadataWriter::read_cut_timeline(&path).unwrap();

        assert_eq!(parsed, timeline);
        let _ = std::fs::remove_file(path);
    }
}
