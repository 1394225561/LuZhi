use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(feature = "ffmpeg")]
use crate::app::error::AppError;
use crate::app::error::AppResult;
use crate::core::cut::CutTimeline;
use crate::media::export_presets::ExportPreset;

#[derive(Clone)]
pub struct ExportProgressReporter {
    callback: Arc<dyn Fn(u8) + Send + Sync>,
}

impl std::fmt::Debug for ExportProgressReporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportProgressReporter")
            .field("callback", &"<closure>")
            .finish()
    }
}

impl ExportProgressReporter {
    pub fn new(callback: Arc<dyn Fn(u8) + Send + Sync>) -> Self {
        Self { callback }
    }

    pub fn report(&self, progress: u8) {
        (self.callback)(progress.min(100));
    }
}

impl PartialEq for ExportProgressReporter {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// Structured request for a future FFmpeg binding implementation.
#[derive(Clone, Debug)]
pub struct TrimExportRequest {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub preset: ExportPreset,
    pub cut_timeline: CutTimeline,
    pub effect_timeline_path: Option<PathBuf>,
    pub cancel_token: Arc<AtomicBool>,
    pub progress: Option<ExportProgressReporter>,
}

impl PartialEq for TrimExportRequest {
    fn eq(&self, other: &Self) -> bool {
        self.input_path == other.input_path
            && self.output_path == other.output_path
            && self.preset == other.preset
            && self.cut_timeline == other.cut_timeline
            && self.effect_timeline_path == other.effect_timeline_path
    }
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
        if request.cancel_token.load(Ordering::Relaxed) {
            return Err(crate::app::error::AppError::ExportCancelled);
        }
        let result = TrimExportResult {
            output_path: request.output_path.clone(),
            cut_count: request.cut_timeline.cuts.len(),
        };
        if let Some(progress) = &request.progress {
            progress.report(100);
        }
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
        if request.cancel_token.load(Ordering::Relaxed) {
            return Err(AppError::ExportCancelled);
        }
        if !request.input_path.exists() {
            return Err(AppError::ExportFailed {
                reason: format!("源文件不存在: {}", request.input_path.to_string_lossy()),
            });
        }
        if request.input_path == request.output_path {
            return Err(AppError::ExportFailed {
                reason: "导出文件不能覆盖原始录制文件".to_string(),
            });
        }

        // Human Native Safety Gate must review the concrete FFmpeg context,
        // stream, packet, encoder, decoder, scaler, resampler, timestamp, and
        // resource-release code before this path is enabled by default.
        Err(AppError::ExportFailed {
            reason: "FFmpeg 导出实现需在本步骤补齐并完成人工 Native Safety 审查后启用".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::cut::CutTimeline;
    use crate::media::export_presets::ExportPreset;
    use std::sync::{atomic::AtomicBool, Arc};

    #[test]
    fn mock_exporter_consumes_cut_timeline_without_deleting_original() {
        let mut exporter = MockTrimExporter::new();
        let request = TrimExportRequest {
            input_path: PathBuf::from("/tmp/raw.mov"),
            output_path: PathBuf::from("/tmp/export.mp4"),
            preset: ExportPreset::Bilibili,
            cut_timeline: CutTimeline::empty(10_000_000_000),
            effect_timeline_path: None,
            cancel_token: Arc::new(AtomicBool::new(false)),
            progress: None,
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
