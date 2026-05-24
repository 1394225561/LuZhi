use std::fmt::{Display, Formatter};

/// Application-level result type shared by recording orchestration modules.
pub type AppResult<T> = Result<T, AppError>;

/// Error categories surfaced by the Rust application layer.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AppError {
    InvalidState {
        current: &'static str,
        action: &'static str,
    },
    PermissionDenied {
        permission: &'static str,
    },
    NativeCaptureUnavailable {
        reason: &'static str,
    },
    CaptureFailed {
        reason: String,
    },
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::InvalidState { current, action } => {
                write!(formatter, "当前状态 {current} 不允许执行 {action}")
            }
            AppError::PermissionDenied { permission } => {
                write!(formatter, "缺少系统权限：{permission}")
            }
            AppError::NativeCaptureUnavailable { reason } => {
                write!(formatter, "当前录制能力不可用：{reason}")
            }
            AppError::CaptureFailed { reason } => {
                write!(formatter, "录制失败：{reason}")
            }
        }
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_error_uses_chinese_message() {
        let error = AppError::PermissionDenied {
            permission: "屏幕录制",
        };

        assert_eq!(error.to_string(), "缺少系统权限：屏幕录制");
    }
}
