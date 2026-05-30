use std::path::PathBuf;

#[cfg(feature = "ffmpeg")]
use crate::app::error::AppError;
use crate::app::error::AppResult;
use crate::core::cut::CutTimeline;
use crate::media::export_presets::ExportPreset;

/// Structured request for a future FFmpeg binding implementation.
#[derive(Clone, Debug, PartialEq)]
pub struct TrimExportRequest {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub preset: ExportPreset,
    pub cut_timeline: CutTimeline,
}

/// Result of a structured trim export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrimExportResult {
    pub output_path: PathBuf,
    pub cut_count: usize,
}

/// Exporter boundary that consumes CutTimeline without shelling out to FFmpeg CLI.
pub trait TrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult>;
}

/// Test exporter that records requests without touching source files.
#[derive(Default)]
pub struct MockTrimExporter {
    requests: Vec<TrimExportRequest>,
}

impl MockTrimExporter {
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
        }
    }

    pub fn requests(&self) -> &[TrimExportRequest] {
        &self.requests
    }
}

impl TrimExporter for MockTrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
        let result = TrimExportResult {
            output_path: request.output_path.clone(),
            cut_count: request.cut_timeline.cuts.len(),
        };
        self.requests.push(request);
        Ok(result)
    }
}

/// Feature-gated production boundary. It intentionally accepts structured data
/// rather than a shell command string, preserving the no-CLI security rule.
#[cfg(feature = "ffmpeg")]
pub struct FfmpegTrimExporter;

#[cfg(feature = "ffmpeg")]
impl TrimExporter for FfmpegTrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
        if request.input_path.as_os_str().is_empty() || request.output_path.as_os_str().is_empty() {
            return Err(AppError::RecordingWriteFailed {
                reason: "裁剪导出路径无效".to_string(),
            });
        }
        Err(AppError::RecordingWriteFailed {
            reason: "FFmpeg 裁剪导出需要生产编码器接入后启用".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::cut::CutTimeline;
    use crate::media::export_presets::ExportPreset;

    #[test]
    fn mock_exporter_consumes_cut_timeline_without_deleting_original() {
        let mut exporter = MockTrimExporter::new();
        let request = TrimExportRequest {
            input_path: PathBuf::from("/tmp/raw.mov"),
            output_path: PathBuf::from("/tmp/export.mp4"),
            preset: ExportPreset::Bilibili,
            cut_timeline: CutTimeline::empty(10_000_000_000),
        };

        let result = exporter.export(request).unwrap();

        assert_eq!(result.output_path, PathBuf::from("/tmp/export.mp4"));
        assert_eq!(result.cut_count, 0);
        assert_eq!(exporter.requests().len(), 1);
        assert_eq!(
            exporter.requests()[0].input_path,
            PathBuf::from("/tmp/raw.mov")
        );
    }

    #[test]
    fn export_preset_rejects_unknown_value() {
        assert_eq!(
            "bilibili".parse::<ExportPreset>().unwrap(),
            ExportPreset::Bilibili
        );
        assert!("unknown".parse::<ExportPreset>().is_err());
    }
}
