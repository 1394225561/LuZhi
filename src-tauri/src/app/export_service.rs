use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

use crate::app::error::{AppError, AppResult};
use crate::core::cut::CutTimeline;
use crate::media::export_paths::{export_output_path, validate_non_empty_output};
use crate::media::export_presets::ExportPreset;
use crate::media::trim_exporter::{
    ExportProgressReporter, TrimExportRequest, TrimExportResult, TrimExporter,
};

pub fn export_recording_with_timeline(
    exporter: &mut dyn TrimExporter,
    input_path: PathBuf,
    requested_output_path: Option<PathBuf>,
    preset: ExportPreset,
    cut_timeline: CutTimeline,
    effect_timeline_path: Option<PathBuf>,
    cancel_token: Arc<AtomicBool>,
    progress: Option<ExportProgressReporter>,
    sequence: u64,
) -> AppResult<TrimExportResult> {
    if !input_path.exists() {
        return Err(AppError::ExportFailed {
            reason: format!("源文件不存在: {}", input_path.to_string_lossy()),
        });
    }
    if input_path
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0)
        == 0
    {
        return Err(AppError::ExportFailed {
            reason: "源文件为空，无法导出".to_string(),
        });
    }

    let output_path =
        requested_output_path.unwrap_or(export_output_path(&input_path, preset, sequence)?);
    if output_path == input_path {
        return Err(AppError::ExportFailed {
            reason: "导出文件不能覆盖原始录制文件".to_string(),
        });
    }

    // Clone output path for cleanup on failure/cancel.
    let planned_output_path = output_path.clone();

    let result = exporter.export(TrimExportRequest {
        input_path,
        output_path,
        preset,
        cut_timeline,
        effect_timeline_path,
        cancel_token,
        progress,
    });

    // Clean up partial output on error.
    match result {
        Ok(result) => {
            // Validate the output file.
            if let Err(error) = validate_non_empty_output(&result.output_path) {
                // Clean up empty/invalid output file.
                let _ = std::fs::remove_file(&result.output_path);
                return Err(error);
            }
            Ok(result)
        }
        Err(error) => {
            // Clean up partial output file on error (cancel or failure).
            let _ = std::fs::remove_file(&planned_output_path);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cut::CutTimeline;
    use crate::media::export_presets::ExportPreset;
    use crate::media::trim_exporter::{TrimExportRequest, TrimExportResult, TrimExporter};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FileCreatingExporter;

    impl TrimExporter for FileCreatingExporter {
        fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
            if request.cancel_token.load(Ordering::Relaxed) {
                return Err(AppError::ExportCancelled);
            }
            std::fs::write(&request.output_path, b"mp4").map_err(|error| {
                AppError::ExportFailed {
                    reason: error.to_string(),
                }
            })?;
            Ok(TrimExportResult {
                output_path: request.output_path,
                cut_count: request.cut_timeline.cuts.len(),
            })
        }
    }

    #[test]
    fn export_service_rejects_missing_source_artifact() {
        let temp = std::env::temp_dir().join("missing-source-artifact.mp4");
        let _ = std::fs::remove_file(&temp);
        let mut exporter = crate::media::trim_exporter::MockTrimExporter::new();
        let cancel = Arc::new(AtomicBool::new(false));

        let result = export_recording_with_timeline(
            &mut exporter,
            temp,
            None,
            ExportPreset::Bilibili,
            CutTimeline::empty(10_000_000_000),
            None,
            cancel,
            None,
            1,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("源文件"));
    }

    #[test]
    fn export_service_sends_structured_request_to_exporter() {
        let dir = std::env::temp_dir().join("luzhi-export-service-test");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("raw.mp4");
        std::fs::write(&source, b"raw").unwrap();
        let mut exporter = FileCreatingExporter;
        let cancel = Arc::new(AtomicBool::new(false));

        let result = export_recording_with_timeline(
            &mut exporter,
            source.clone(),
            None,
            ExportPreset::Douyin,
            CutTimeline::empty(10_000_000_000),
            None,
            cancel,
            None,
            9,
        )
        .unwrap();

        assert_eq!(result.cut_count, 0);
        assert!(result.output_path.exists());
        assert_ne!(result.output_path, source);
        let _ = std::fs::remove_dir_all(dir);
    }
}
