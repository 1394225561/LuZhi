use crate::app::permission_service::{
    PermissionProbe, PermissionStatus, RecordingPermissions,
};

/// macOS permission probe placeholder.
///
/// Real Screen Recording / Microphone detection will be activated after
/// human review of the ScreenCaptureKit integration boundary.
pub struct MacPermissionProbe;

impl PermissionProbe for MacPermissionProbe {
    fn recording_permissions(&self) -> RecordingPermissions {
        RecordingPermissions {
            screen_recording: PermissionStatus::Unknown,
            microphone: PermissionStatus::Unknown,
        }
    }
}
