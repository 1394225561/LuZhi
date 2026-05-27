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
    pub effect_timeline_path: String,
}

/// Lightweight post-processing progress payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostProcessProgressPayload {
    pub stage: &'static str,
    pub progress: u8,
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
            effect_timeline_path: "/tmp/effects.json".to_string(),
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
        };

        let json = serde_json::to_string(&payload).unwrap();

        assert!(json.contains("\"stage\":\"cursor\""));
        assert!(json.contains("\"progress\":100"));
    }
}
