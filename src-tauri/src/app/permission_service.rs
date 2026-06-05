/// Permission status for a single system capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
    Unknown,
}

/// Combined recording permissions required by the app.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingPermissions {
    pub screen_recording: PermissionStatus,
    pub microphone: PermissionStatus,
    /// Accessibility permission. Currently retained for diagnostics and future
    /// AX-specific probes; NSCursor-based cursor kind does not require it.
    pub accessibility: PermissionStatus,
}

/// Platform-specific permission probe.
pub trait PermissionProbe: Send + Sync {
    fn recording_permissions(&self) -> RecordingPermissions;
}

/// Service that delegates permission checks to a platform probe.
pub struct PermissionService<P: PermissionProbe> {
    probe: P,
}

impl<P: PermissionProbe> PermissionService<P> {
    pub fn new(probe: P) -> Self {
        Self { probe }
    }

    pub fn recording_permissions(&self) -> RecordingPermissions {
        self.probe.recording_permissions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct GrantedProbe;

    impl PermissionProbe for GrantedProbe {
        fn recording_permissions(&self) -> RecordingPermissions {
            RecordingPermissions {
                screen_recording: PermissionStatus::Granted,
                microphone: PermissionStatus::Granted,
                accessibility: PermissionStatus::Granted,
            }
        }
    }

    #[test]
    fn service_returns_probe_permissions() {
        let service = PermissionService::new(GrantedProbe);

        assert_eq!(
            service.recording_permissions(),
            RecordingPermissions {
                screen_recording: PermissionStatus::Granted,
                microphone: PermissionStatus::Granted,
                accessibility: PermissionStatus::Granted,
            }
        );
    }
}
