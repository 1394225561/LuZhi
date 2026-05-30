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
    AudioCaptureFailed {
        reason: String,
    },
    AudioDeviceNotFound {
        name: String,
    },
    AudioMixFailed {
        reason: String,
    },
    CaptureStopTimeout {
        reason: String,
    },
    RecordingWriteFailed {
        reason: String,
    },
    RecordingFinalizeFailed {
        reason: String,
    },
    CursorProcessingFailed {
        reason: String,
    },
    TrimProcessingFailed {
        reason: String,
    },
    ExportFailed {
        reason: String,
    },
    ExportCancelled,
    LicenseFailed {
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
            AppError::AudioCaptureFailed { reason } => {
                write!(formatter, "音频捕获失败：{reason}")
            }
            AppError::AudioDeviceNotFound { name } => {
                write!(formatter, "未找到音频设备：{name}")
            }
            AppError::AudioMixFailed { reason } => {
                write!(formatter, "音频混音失败：{reason}")
            }
            AppError::CaptureStopTimeout { reason } => {
                write!(formatter, "停止录制超时：{reason}")
            }
            AppError::RecordingWriteFailed { reason } => {
                write!(formatter, "写入录制文件失败：{reason}")
            }
            AppError::RecordingFinalizeFailed { reason } => {
                write!(formatter, "完成录制文件失败：{reason}")
            }
            AppError::CursorProcessingFailed { reason } => {
                write!(formatter, "光标效果处理失败：{reason}")
            }
            AppError::TrimProcessingFailed { reason } => {
                write!(formatter, "空白裁剪处理失败：{reason}")
            }
            AppError::ExportFailed { reason } => {
                write!(formatter, "导出失败：{reason}")
            }
            AppError::ExportCancelled => {
                write!(formatter, "导出已取消")
            }
            AppError::LicenseFailed { reason } => {
                write!(formatter, "授权状态处理失败：{reason}")
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

    #[test]
    fn trim_processing_error_uses_chinese_message() {
        let error = AppError::TrimProcessingFailed {
            reason: "没有裁剪元数据".to_string(),
        };

        assert_eq!(error.to_string(), "空白裁剪处理失败：没有裁剪元数据");
    }

    #[test]
    fn export_error_uses_chinese_message() {
        let error = AppError::ExportFailed {
            reason: "源文件不存在".to_string(),
        };
        assert_eq!(error.to_string(), "导出失败：源文件不存在");
    }

    #[test]
    fn export_cancelled_uses_chinese_message() {
        assert_eq!(AppError::ExportCancelled.to_string(), "导出已取消");
    }
}
