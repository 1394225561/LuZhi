use serde::{Deserialize, Serialize};

use crate::core::frame::MediaTimestamp;

/// User-facing trim sensitivity selected in Preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrimSensitivity {
    Low,
    Medium,
    High,
}

impl TrimSensitivity {
    /// Parses the frontend string value used by `BeautifyConfigPayload`.
    pub fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(format!("未知裁剪灵敏度：{other}")),
        }
    }
}

/// Conservative configuration for blank-segment detection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimConfig {
    pub rms_window_nanos: u64,
    pub audio_rms_threshold: f32,
    pub visual_change_threshold: f32,
    pub min_candidate_nanos: u64,
    pub absolute_min_cut_nanos: u64,
    pub buffer_nanos: u64,
    pub merge_gap_nanos: u64,
}

impl TrimConfig {
    /// Defaults are intentionally conservative to avoid cutting thinking time.
    pub fn from_sensitivity(sensitivity: TrimSensitivity) -> Self {
        match sensitivity {
            TrimSensitivity::Low => Self {
                rms_window_nanos: 1_000_000_000,
                audio_rms_threshold: 0.012,
                visual_change_threshold: 0.006,
                min_candidate_nanos: 8_000_000_000,
                absolute_min_cut_nanos: 2_000_000_000,
                buffer_nanos: 500_000_000,
                merge_gap_nanos: 500_000_000,
            },
            TrimSensitivity::Medium => Self {
                rms_window_nanos: 750_000_000,
                audio_rms_threshold: 0.02,
                visual_change_threshold: 0.01,
                min_candidate_nanos: 6_000_000_000,
                absolute_min_cut_nanos: 2_000_000_000,
                buffer_nanos: 400_000_000,
                merge_gap_nanos: 500_000_000,
            },
            TrimSensitivity::High => Self {
                rms_window_nanos: 500_000_000,
                audio_rms_threshold: 0.03,
                visual_change_threshold: 0.015,
                min_candidate_nanos: 5_000_000_000,
                absolute_min_cut_nanos: 2_000_000_000,
                buffer_nanos: 300_000_000,
                merge_gap_nanos: 500_000_000,
            },
        }
    }
}

/// Audio activity sampled from mixed audio after recording.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioActivitySample {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub rms: f32,
}

/// Low-resolution visual activity sampled from captured video frames.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameDiffSample {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub change_ratio: f32,
}

/// Reason a segment was selected for cutting.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CutReason {
    SilentAndStill,
}

/// Segment removed from the exported timeline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutSegment {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub reason: CutReason,
    pub mean_audio_rms: f32,
    pub mean_visual_change: f32,
}

/// Segment kept in the exported timeline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepSegment {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
}

/// Complete post-recording cut timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutTimeline {
    pub duration_nanos: u64,
    pub cuts: Vec<CutSegment>,
    pub keeps: Vec<KeepSegment>,
    pub total_cut_nanos: u64,
}

impl CutTimeline {
    /// Creates a no-op timeline that keeps the entire recording.
    pub fn empty(duration_nanos: u64) -> Self {
        Self {
            duration_nanos,
            cuts: Vec::new(),
            keeps: vec![KeepSegment {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(duration_nanos),
            }],
            total_cut_nanos: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;

    #[test]
    fn cut_timeline_round_trips_camel_case_json() {
        let timeline = CutTimeline {
            duration_nanos: 20_000_000_000,
            cuts: vec![CutSegment {
                start: MediaTimestamp::from_nanos(6_400_000_000),
                end: MediaTimestamp::from_nanos(12_100_000_000),
                reason: CutReason::SilentAndStill,
                mean_audio_rms: 0.004,
                mean_visual_change: 0.002,
            }],
            keeps: vec![
                KeepSegment {
                    start: MediaTimestamp::from_nanos(0),
                    end: MediaTimestamp::from_nanos(6_400_000_000),
                },
                KeepSegment {
                    start: MediaTimestamp::from_nanos(12_100_000_000),
                    end: MediaTimestamp::from_nanos(20_000_000_000),
                },
            ],
            total_cut_nanos: 5_700_000_000,
        };

        let json = serde_json::to_string(&timeline).unwrap();
        assert!(json.contains("\"durationNanos\":20000000000"));
        assert!(json.contains("\"totalCutNanos\":5700000000"));
        assert!(json.contains("\"reason\":\"silentAndStill\""));

        let parsed: CutTimeline = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, timeline);
    }

    #[test]
    fn trim_sensitivity_maps_to_conservative_defaults() {
        let low = TrimConfig::from_sensitivity(TrimSensitivity::Low);
        let medium = TrimConfig::from_sensitivity(TrimSensitivity::Medium);
        let high = TrimConfig::from_sensitivity(TrimSensitivity::High);

        assert_eq!(low.rms_window_nanos, 1_000_000_000);
        assert_eq!(medium.rms_window_nanos, 750_000_000);
        assert_eq!(high.rms_window_nanos, 500_000_000);

        assert_eq!(low.min_candidate_nanos, 8_000_000_000);
        assert_eq!(medium.min_candidate_nanos, 6_000_000_000);
        assert_eq!(high.min_candidate_nanos, 5_000_000_000);

        assert!(low.audio_rms_threshold < high.audio_rms_threshold);
        assert!(low.visual_change_threshold < high.visual_change_threshold);
    }

    #[test]
    fn empty_cut_timeline_keeps_full_duration() {
        let timeline = CutTimeline::empty(10_000_000_000);

        assert!(timeline.cuts.is_empty());
        assert_eq!(timeline.total_cut_nanos, 0);
        assert_eq!(
            timeline.keeps,
            vec![KeepSegment {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(10_000_000_000),
            }]
        );
    }
}
