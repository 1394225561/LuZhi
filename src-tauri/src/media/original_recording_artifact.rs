use std::path::Path;

use crate::app::error::{AppError, AppResult};

pub fn validate_source_artifact(path: &Path) -> AppResult<()> {
    let metadata = std::fs::metadata(path).map_err(|error| AppError::ExportFailed {
        reason: format!("原始录制文件不存在或不可访问: {error}"),
    })?;
    if metadata.len() == 0 {
        return Err(AppError::ExportFailed {
            reason: "原始录制文件为空，不能用于导出".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_artifact_requires_existing_non_empty_file() {
        let path = std::env::temp_dir().join("luzhi-missing-source-artifact.mp4");
        let _ = std::fs::remove_file(&path);

        assert!(validate_source_artifact(&path).is_err());

        std::fs::write(&path, b"source").unwrap();
        assert!(validate_source_artifact(&path).is_ok());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_artifact_error_mentions_original_recording() {
        let path = std::env::temp_dir().join("luzhi-empty-source-artifact.mp4");
        std::fs::write(&path, []).unwrap();
        let error = validate_source_artifact(&path).unwrap_err().to_string();

        assert!(error.contains("原始录制文件"));
        let _ = std::fs::remove_file(path);
    }
}
