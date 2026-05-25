use crate::app::permission_service::{PermissionProbe, PermissionStatus, RecordingPermissions};

/// macOS 权限探测的占位实现。
///
/// 当前返回 `Unknown`，因为真实的 Screen Recording 和 Microphone 权限检测
/// 需要调用 `CGPreflightScreenCaptureAccess()` 和 `AVAudioApplication` 等原生 API，
/// 这些调用属于 Native Safety Gate 范畴，需人工审查后启用。
///
/// 后续实现需要：
/// 1. Screen Recording: `CGPreflightScreenCaptureAccess()` (macOS 10.15+)
/// 2. Microphone: `AVAudioApplication.recordPermission` (macOS 14+)
///    或 `AVCaptureDevice.authorizationStatus(for: .audio)` (更早版本)
pub struct MacPermissionProbe;

impl PermissionProbe for MacPermissionProbe {
    fn recording_permissions(&self) -> RecordingPermissions {
        RecordingPermissions {
            screen_recording: PermissionStatus::Unknown,
            microphone: PermissionStatus::Unknown,
        }
    }
}
