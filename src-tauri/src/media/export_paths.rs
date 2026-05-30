use std::path::{Path, PathBuf};

use crate::app::error::{AppError, AppResult};
use crate::media::export_presets::ExportPreset;

pub fn export_output_path(
    source_path: &Path,
    preset: ExportPreset,
    sequence: u64,
) -> AppResult<PathBuf> {
    let parent = source_path.parent().ok_or_else(|| AppError::ExportFailed {
        reason: "原始录制文件没有父目录，无法生成导出路径".to_string(),
    })?;
    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| AppError::ExportFailed {
            reason: "原始录制文件名无效，无法生成导出路径".to_string(),
        })?;
    let preset_id = preset.spec().id;
    Ok(parent.join(format!("{stem}-{preset_id}-export-{sequence}.mp4")))
}

/// Generates a unique output path for the original recording artifact.
///
/// Placed in the system temp directory under `luzhi-recordings/` with a
/// timestamped filename to avoid collisions across recording sessions.
pub fn original_recording_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("recording-{millis}-{seq}.mp4"))
}

pub fn validate_non_empty_output(path: &Path) -> AppResult<()> {
    let metadata = std::fs::metadata(path).map_err(|error| AppError::ExportFailed {
        reason: format!("导出文件不存在或不可访问: {error}"),
    })?;
    if metadata.len() == 0 {
        return Err(AppError::ExportFailed {
            reason: "导出文件为空，不能返回 outputPath".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::export_presets::ExportPreset;

    #[test]
    fn export_path_is_separate_from_source_path() {
        let source = std::env::temp_dir()
            .join("luzhi-export-path-test")
            .join("raw.mp4");
        let output = export_output_path(&source, ExportPreset::Bilibili, 7).unwrap();

        assert_ne!(output, source);
        assert!(output.to_string_lossy().contains("bilibili"));
        assert!(output.to_string_lossy().ends_with(".mp4"));
    }

    #[test]
    fn non_empty_output_requires_existing_non_empty_file() {
        let path = std::env::temp_dir().join("luzhi-empty-output-test.mp4");
        let _ = std::fs::remove_file(&path);

        assert!(validate_non_empty_output(&path).is_err());

        std::fs::write(&path, b"not empty").unwrap();
        assert!(validate_non_empty_output(&path).is_ok());
        let _ = std::fs::remove_file(path);
    }
}
