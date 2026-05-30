use serde::Serialize;

use crate::app::permission_service::{PermissionStatus, RecordingPermissions};
use crate::app::state_machine::RecordingState;

/// Recording status payload sent to the frontend.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatusPayload {
    pub state: &'static str,
    pub can_start: bool,
}

impl From<RecordingState> for RecordingStatusPayload {
    fn from(state: RecordingState) -> Self {
        Self {
            state: state.as_str(),
            can_start: matches!(
                state,
                RecordingState::Idle | RecordingState::Completed | RecordingState::Failed
            ),
        }
    }
}

/// Permission status payload sent to the frontend.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPayload {
    pub screen_recording: PermissionStatusPayload,
    pub microphone: PermissionStatusPayload,
}

/// Mic level payload sent to the frontend during recording.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicLevelPayload {
    pub level: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionStatusPayload {
    Granted,
    Denied,
    NotDetermined,
    Unknown,
}

impl From<PermissionStatus> for PermissionStatusPayload {
    fn from(status: PermissionStatus) -> Self {
        match status {
            PermissionStatus::Granted => PermissionStatusPayload::Granted,
            PermissionStatus::Denied => PermissionStatusPayload::Denied,
            PermissionStatus::NotDetermined => PermissionStatusPayload::NotDetermined,
            PermissionStatus::Unknown => PermissionStatusPayload::Unknown,
        }
    }
}

impl From<RecordingPermissions> for PermissionPayload {
    fn from(permissions: RecordingPermissions) -> Self {
        Self {
            screen_recording: permissions.screen_recording.into(),
            microphone: permissions.microphone.into(),
        }
    }
}

/// Summary returned after building a cursor effect timeline.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorEffectSummaryPayload {
    pub frame_count: usize,
    pub click_effect_count: usize,
    /// Path to the cursor effect timeline JSON file. `None` when cursor
    /// metadata is unavailable or the build was skipped/failed.
    pub effect_timeline_path: Option<String>,
}

/// Summary returned after building a cut timeline.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CutTimelineSummaryPayload {
    pub cut_count: usize,
    pub total_cut_nanos: u64,
    pub cut_timeline_path: String,
}

/// Summary returned after export command prepares all Phase 4/5 timelines.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummaryPayload {
    pub frame_count: usize,
    pub click_effect_count: usize,
    /// Path to cursor effect timeline. `None` when cursor effects are
    /// unavailable — basic playable export still proceeds without them.
    pub effect_timeline_path: Option<String>,
    pub cut_count: usize,
    pub total_cut_nanos: u64,
    pub cut_timeline_path: Option<String>,
    pub output_path: Option<String>,
}

/// Lightweight post-processing progress payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostProcessProgressPayload {
    pub stage: &'static str,
    pub progress: u8,
    /// When set, indicates the post-process stage failed with this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Export progress payload emitted during video export.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgressPayload {
    pub preset: &'static str,
    pub progress: u8,
    pub cancellable: bool,
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// License status payload sent to the frontend.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatusPayload {
    pub kind: &'static str,
    pub trial_days_remaining: u8,
    pub is_expired: bool,
    pub activated: bool,
}

impl From<crate::app::license_service::LicenseStatus> for LicenseStatusPayload {
    fn from(status: crate::app::license_service::LicenseStatus) -> Self {
        let kind = match status.kind {
            crate::app::license_service::LicenseStatusKind::Trial => "trial",
            crate::app::license_service::LicenseStatusKind::Expired => "expired",
            crate::app::license_service::LicenseStatusKind::Activated => "activated",
        };
        Self {
            kind,
            trial_days_remaining: status.trial_days_remaining,
            is_expired: status.is_expired,
            activated: status.activated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_status_can_start() {
        let payload = RecordingStatusPayload::from(RecordingState::Idle);

        assert_eq!(payload.state, "idle");
        assert!(payload.can_start);
    }

    #[test]
    fn recording_status_cannot_start() {
        let payload = RecordingStatusPayload::from(RecordingState::Recording);

        assert_eq!(payload.state, "recording");
        assert!(!payload.can_start);
    }

    #[test]
    fn mic_level_payload_serializes_camel_case() {
        let payload = MicLevelPayload { level: 0.75 };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"level\""));
        assert!(json.contains("0.75"));
    }

    #[test]
    fn cursor_effect_summary_serializes_camel_case() {
        let payload = CursorEffectSummaryPayload {
            frame_count: 10,
            click_effect_count: 2,
            effect_timeline_path: Some("/tmp/effects.json".to_string()),
        };

        let json = serde_json::to_string(&payload).unwrap();

        assert!(json.contains("\"frameCount\":10"));
        assert!(json.contains("\"clickEffectCount\":2"));
        assert!(json.contains("\"effectTimelinePath\":\"/tmp/effects.json\""));
    }

    #[test]
    fn post_process_progress_serializes_camel_case() {
        let payload = PostProcessProgressPayload {
            stage: "cursor",
            progress: 100,
            error: None,
        };

        let json = serde_json::to_string(&payload).unwrap();

        assert!(json.contains("\"stage\":\"cursor\""));
        assert!(json.contains("\"progress\":100"));
        // error field should be absent when None
        assert!(!json.contains("error"));
    }

    #[test]
    fn post_process_progress_includes_error_when_present() {
        let payload = PostProcessProgressPayload {
            stage: "cursor",
            progress: 0,
            error: Some("构建失败".to_string()),
        };

        let json = serde_json::to_string(&payload).unwrap();

        assert!(json.contains("\"error\":\"构建失败\""));
    }

    #[test]
    fn cut_timeline_summary_serializes_camel_case() {
        let payload = CutTimelineSummaryPayload {
            cut_count: 2,
            total_cut_nanos: 3_000_000_000,
            cut_timeline_path: "/tmp/cuts.json".to_string(),
        };

        let json = serde_json::to_string(&payload).unwrap();

        assert!(json.contains("\"cutCount\":2"));
        assert!(json.contains("\"totalCutNanos\":3000000000"));
        assert!(json.contains("\"cutTimelinePath\":\"/tmp/cuts.json\""));
    }

    #[test]
    fn export_summary_serializes_camel_case() {
        let payload = ExportSummaryPayload {
            frame_count: 10,
            click_effect_count: 1,
            effect_timeline_path: Some("/tmp/effects.json".to_string()),
            cut_count: 2,
            total_cut_nanos: 3_000_000_000,
            cut_timeline_path: Some("/tmp/cuts.json".to_string()),
            output_path: None,
        };

        let json = serde_json::to_string(&payload).unwrap();

        assert!(json.contains("\"frameCount\":10"));
        assert!(json.contains("\"clickEffectCount\":1"));
        assert!(json.contains("\"cutCount\":2"));
        assert!(json.contains("\"totalCutNanos\":3000000000"));
        assert!(json.contains("\"cutTimelinePath\":\"/tmp/cuts.json\""));
        assert!(json.contains("\"outputPath\":null"));
    }

    #[test]
    fn export_progress_serializes_camel_case() {
        let payload = ExportProgressPayload {
            preset: "bilibili",
            progress: 45,
            cancellable: true,
            output_path: None,
            error: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"preset\":\"bilibili\""));
        assert!(json.contains("\"progress\":45"));
        assert!(json.contains("\"cancellable\":true"));
        assert!(json.contains("\"outputPath\":null"));
    }
}
