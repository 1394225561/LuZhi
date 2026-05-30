//! FFmpeg artifact-level integration tests.
//!
//! These tests verify the complete export pipeline:
//! source artifact → ExportService → FfmpegTrimExporter → playable output.

#![cfg(feature = "ffmpeg")]

use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

use luzhi_lib::app::export_service::export_recording_with_timeline;
use luzhi_lib::core::cut::{CutReason, CutSegment, CutTimeline, KeepSegment};
use luzhi_lib::core::frame::MediaTimestamp;
use luzhi_lib::media::export_presets::ExportPreset;
use luzhi_lib::media::trim_exporter::FfmpegTrimExporter;

fn unique_path(prefix: &str, ext: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir()
        .join("luzhi-integration")
        .join(format!("{prefix}-{millis}-{seq}.{ext}"))
}

fn create_source(prefix: &str, duration_nanos: u64) -> PathBuf {
    let path = unique_path(prefix, "mp4");
    luzhi_lib::test_support::ffmpeg_helpers::create_synthetic_source_artifact(
        &path,
        1920,
        1080,
        duration_nanos,
    )
    .unwrap();
    path
}

/// Full-duration export produces a valid MP4 with video + audio streams.
#[test]
fn full_duration_export_produces_playable_mp4() {
    let source = create_source("int-full", 500_000_000);
    let output = unique_path("int-full-out", "mp4");

    let mut exporter = FfmpegTrimExporter;
    let result = export_recording_with_timeline(
        &mut exporter,
        source.clone(),
        Some(output.clone()),
        ExportPreset::Bilibili,
        CutTimeline::empty(500_000_000),
        None,
        Arc::new(AtomicBool::new(false)),
        None,
        1,
    )
    .unwrap();

    assert_eq!(result.output_path, output);
    assert!(output.exists());
    assert!(std::fs::metadata(&output).unwrap().len() > 0);

    let inspection =
        luzhi_lib::test_support::ffmpeg_helpers::inspect_media_artifact(&output).unwrap();
    assert!(inspection.has_video_stream, "must have video stream");
    assert!(inspection.has_audio_stream, "must have audio stream");
    assert_eq!(inspection.width, 1920);
    assert_eq!(inspection.height, 1080);

    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&output);
}

/// Douyin 9:16 preset scales output to 1080×1920.
#[test]
fn douyin_preset_scales_to_portrait() {
    let source = create_source("int-douyin", 500_000_000);
    let output = unique_path("int-douyin-out", "mp4");

    let mut exporter = FfmpegTrimExporter;
    let result = export_recording_with_timeline(
        &mut exporter,
        source.clone(),
        Some(output.clone()),
        ExportPreset::Douyin,
        CutTimeline::empty(500_000_000),
        None,
        Arc::new(AtomicBool::new(false)),
        None,
        1,
    )
    .unwrap();

    assert!(result.output_path.exists());
    let inspection =
        luzhi_lib::test_support::ffmpeg_helpers::inspect_media_artifact(&output).unwrap();
    assert!(inspection.has_video_stream);
    assert_eq!(inspection.width, 1080);
    assert_eq!(inspection.height, 1920);

    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&output);
}

/// Xiaohongshu 1:1 preset scales output to 1080×1080.
#[test]
fn xiaohongshu_preset_scales_to_square() {
    let source = create_source("int-xhs", 500_000_000);
    let output = unique_path("int-xhs-out", "mp4");

    let mut exporter = FfmpegTrimExporter;
    let result = export_recording_with_timeline(
        &mut exporter,
        source.clone(),
        Some(output.clone()),
        ExportPreset::Xiaohongshu,
        CutTimeline::empty(500_000_000),
        None,
        Arc::new(AtomicBool::new(false)),
        None,
        1,
    )
    .unwrap();

    assert!(result.output_path.exists());
    let inspection =
        luzhi_lib::test_support::ffmpeg_helpers::inspect_media_artifact(&output).unwrap();
    assert!(inspection.has_video_stream);
    assert_eq!(inspection.width, 1080);
    assert_eq!(inspection.height, 1080);

    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&output);
}

/// Cancel before start returns ExportCancelled.
#[test]
fn export_cancelled_before_start() {
    let source = create_source("int-cancel", 500_000_000);
    let output = unique_path("int-cancel-out", "mp4");

    let cancel = Arc::new(AtomicBool::new(true));
    let mut exporter = FfmpegTrimExporter;
    let result = export_recording_with_timeline(
        &mut exporter,
        source.clone(),
        Some(output.clone()),
        ExportPreset::Bilibili,
        CutTimeline::empty(500_000_000),
        None,
        cancel,
        None,
        1,
    );

    assert!(result.is_err());
    assert!(!output.exists(), "cancelled export should not leave output");

    let _ = std::fs::remove_file(&source);
}

/// Missing source file returns ExportFailed.
#[test]
fn export_missing_source_returns_error() {
    let missing = PathBuf::from("/tmp/luzhi-nonexistent-source.mp4");
    let output = unique_path("int-missing-out", "mp4");

    let mut exporter = FfmpegTrimExporter;
    let result = export_recording_with_timeline(
        &mut exporter,
        missing,
        Some(output.clone()),
        ExportPreset::Bilibili,
        CutTimeline::empty(1_000_000_000),
        None,
        Arc::new(AtomicBool::new(false)),
        None,
        1,
    );

    assert!(result.is_err());
    let _ = std::fs::remove_file(&output);
}

/// Export with cut timeline produces shorter output than source.
/// Source is 8 seconds; cut [2s, 6s] should produce ~4 seconds output.
#[test]
fn cut_timeline_export_produces_shorter_output() {
    let source = create_source("int-cut", 8_000_000_000);
    let output = unique_path("int-cut-out", "mp4");

    let cut_timeline = CutTimeline {
        duration_nanos: 8_000_000_000,
        keeps: vec![
            KeepSegment {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(2_000_000_000),
            },
            KeepSegment {
                start: MediaTimestamp::from_nanos(6_000_000_000),
                end: MediaTimestamp::from_nanos(8_000_000_000),
            },
        ],
        cuts: vec![CutSegment {
            start: MediaTimestamp::from_nanos(2_000_000_000),
            end: MediaTimestamp::from_nanos(6_000_000_000),
            reason: CutReason::SilentAndStill,
            mean_audio_rms: 0.0,
            mean_visual_change: 0.0,
        }],
        total_cut_nanos: 4_000_000_000,
    };

    let mut exporter = FfmpegTrimExporter;
    let result = export_recording_with_timeline(
        &mut exporter,
        source.clone(),
        Some(output.clone()),
        ExportPreset::Bilibili,
        cut_timeline,
        None,
        Arc::new(AtomicBool::new(false)),
        None,
        1,
    )
    .unwrap();

    assert!(output.exists());
    let inspection =
        luzhi_lib::test_support::ffmpeg_helpers::inspect_media_artifact(&output).unwrap();
    assert!(inspection.has_video_stream, "must have video stream");
    assert!(inspection.has_audio_stream, "must have audio stream");
    assert_eq!(inspection.width, 1920);
    assert_eq!(inspection.height, 1080);
    // Output should be roughly 4 seconds (source 8s minus 4s cut).
    // Allow some tolerance for codec delay.
    assert!(
        inspection.duration_nanos < 6_000_000_000,
        "output duration should be shorter than source after cut: {}ns",
        inspection.duration_nanos
    );

    // Original source should still exist.
    assert!(source.exists(), "original source should be preserved");

    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&output);
}
