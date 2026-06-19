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
    ImportFailed {
        reason: String,
    },
    RecordingNotFound(String),
    IndexCorrupted {
        reason: String,
    },
    /// 窗口未找到
    WindowNotFound {
        window_id: u32,
    },
    /// 窗口已最小化，无法启动录制
    WindowMinimized {
        window_id: u32,
    },
    /// 窗口已关闭
    WindowClosed {
        window_id: u32,
    },
    /// 窗口访问被拒绝（如 DRM 保护内容）
    WindowAccessDenied {
        window_id: u32,
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
            Self::ImportFailed { reason } => write!(formatter, "导入失败：{reason}"),
            Self::RecordingNotFound(id) => write!(formatter, "录制未找到：{id}"),
            Self::IndexCorrupted { reason } => write!(formatter, "索引损坏：{reason}"),
            Self::WindowNotFound { window_id } => {
                write!(formatter, "窗口未找到：{window_id}")
            }
            Self::WindowMinimized { window_id } => {
                write!(formatter, "窗口已最小化，请恢复窗口后重试：{window_id}")
            }
            Self::WindowClosed { window_id } => {
                write!(formatter, "窗口已关闭：{window_id}")
            }
            Self::WindowAccessDenied { window_id } => {
                write!(formatter, "窗口访问被拒绝（可能受 DRM 保护）：{window_id}")
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

    #[test]
    fn import_failed_chinese_message() {
        let err = AppError::ImportFailed {
            reason: "非本应用录制的文件".to_string(),
        };
        assert_eq!(err.to_string(), "导入失败：非本应用录制的文件");
    }

    #[test]
    fn recording_not_found_chinese_message() {
        let err = AppError::RecordingNotFound("rec-123".to_string());
        assert_eq!(err.to_string(), "录制未找到：rec-123");
    }

    #[test]
    fn index_corrupted_chinese_message() {
        let err = AppError::IndexCorrupted {
            reason: "JSON 解析失败".to_string(),
        };
        assert_eq!(err.to_string(), "索引损坏：JSON 解析失败");
    }

    #[test]
    fn window_not_found_chinese_message() {
        let err = AppError::WindowNotFound { window_id: 42 };
        assert_eq!(err.to_string(), "窗口未找到：42");
    }

    #[test]
    fn window_minimized_chinese_message() {
        let err = AppError::WindowMinimized { window_id: 42 };
        assert_eq!(err.to_string(), "窗口已最小化，请恢复窗口后重试：42");
    }

    #[test]
    fn window_closed_chinese_message() {
        let err = AppError::WindowClosed { window_id: 42 };
        assert_eq!(err.to_string(), "窗口已关闭：42");
    }

    #[test]
    fn window_access_denied_chinese_message() {
        let err = AppError::WindowAccessDenied { window_id: 42 };
        assert_eq!(err.to_string(), "窗口访问被拒绝（可能受 DRM 保护）：42");
    }
}
