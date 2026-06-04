use crate::app::permission_service::{PermissionProbe, PermissionStatus, RecordingPermissions};

pub struct MacPermissionProbe;

impl PermissionProbe for MacPermissionProbe {
    fn recording_permissions(&self) -> RecordingPermissions {
        RecordingPermissions {
            screen_recording: Self::check_screen_recording(),
            microphone: Self::check_microphone(),
            accessibility: Self::check_accessibility(),
        }
    }
}

impl MacPermissionProbe {
    fn check_screen_recording() -> PermissionStatus {
        // SAFETY: CGPreflightScreenCaptureAccess 是无状态 C 函数，调用安全。
        let allowed = unsafe { CGPreflightScreenCaptureAccess() };
        map_screen_preflight(allowed)
    }

    fn check_microphone() -> PermissionStatus {
        let status = unsafe { av_authorization_status_for_audio() };
        map_av_authorization_status(status)
    }

    fn check_accessibility() -> PermissionStatus {
        // SAFETY: AXIsProcessTrusted() 是无状态查询函数，无副作用。
        // 返回 true 表示用户已在系统偏好设置中授权辅助功能权限。
        let trusted = unsafe { AXIsProcessTrusted() };
        if trusted {
            PermissionStatus::Granted
        } else {
            // 无法通过此 API 区分"从未请求"和"用户已拒绝"，
            // 对 UX 而言映射为 NotDetermined（用户可能需要在系统偏好设置中授权）。
            PermissionStatus::NotDetermined
        }
    }
}

/// 将 CGPreflightScreenCaptureAccess 布尔值映射为 PermissionStatus。
///
/// 返回 false 时无法区分"从未请求过"和"用户已拒绝"，均映射为 NotDetermined。
fn map_screen_preflight(allowed: bool) -> PermissionStatus {
    if allowed {
        PermissionStatus::Granted
    } else {
        PermissionStatus::NotDetermined
    }
}

/// 将 AVAuthorizationStatus 整数值映射为 PermissionStatus。
///
/// 0 = NotDetermined, 1 = Restricted, 2 = Denied, 3 = Authorized
fn map_av_authorization_status(status: isize) -> PermissionStatus {
    match status {
        3 => PermissionStatus::Granted,
        2 => PermissionStatus::Denied,
        1 => PermissionStatus::Denied, // Restricted 等同于拒绝
        0 => PermissionStatus::NotDetermined,
        _ => PermissionStatus::Unknown,
    }
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// macOS 10.15+ 屏幕录制权限预检。
    /// 返回 true 表示用户已授权，false 表示尚未授权或已拒绝。
    fn CGPreflightScreenCaptureAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// 检查当前进程是否被信任使用辅助功能 API。
    /// 返回 true 表示用户已在系统偏好设置 > 隐私与安全 > 辅助功能中授权。
    fn AXIsProcessTrusted() -> bool;
}

/// 调用 AVFoundation 的 AVCaptureDevice 类方法查询麦克风授权状态。
///
/// SAFETY: AVCaptureDevice 是线程安全的类，类方法调用不涉及可变状态。
unsafe fn av_authorization_status_for_audio() -> isize {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;
    use objc2_foundation::NSString;

    // 获取 AVCaptureDevice 类
    let cls = match AnyClass::get(c"AVCaptureDevice") {
        Some(c) => c,
        None => return -1, // 类不存在 → Unknown
    };

    // AVMediaTypeAudio = @"soun" (Apple 定义的紧凑字符串常量)
    let media_type = NSString::from_str("soun");

    // AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
    let status: isize = unsafe { msg_send![cls, authorizationStatusForMediaType: &*media_type] };

    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_av_authorized_to_granted() {
        assert_eq!(map_av_authorization_status(3), PermissionStatus::Granted);
    }

    #[test]
    fn maps_av_denied_to_denied() {
        assert_eq!(map_av_authorization_status(2), PermissionStatus::Denied);
    }

    #[test]
    fn maps_av_restricted_to_denied() {
        assert_eq!(map_av_authorization_status(1), PermissionStatus::Denied);
    }

    #[test]
    fn maps_av_not_determined() {
        assert_eq!(
            map_av_authorization_status(0),
            PermissionStatus::NotDetermined
        );
    }

    #[test]
    fn maps_unknown_av_status() {
        assert_eq!(map_av_authorization_status(-1), PermissionStatus::Unknown);
    }

    #[test]
    fn maps_screen_preflight_true_to_granted() {
        assert_eq!(map_screen_preflight(true), PermissionStatus::Granted);
    }

    #[test]
    fn maps_screen_preflight_false_to_not_determined() {
        assert_eq!(map_screen_preflight(false), PermissionStatus::NotDetermined);
    }
}
