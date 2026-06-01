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
    export_recording_with_timeline(
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

/// Cut export produces output with duration within ±500ms of expected.
/// Source is 10 seconds; cut [3s, 7s] should produce ~6 seconds output.
#[test]
fn cut_export_duration_within_tolerance() {
    let source = create_source("int-cut-dur", 10_000_000_000);
    let output = unique_path("int-cut-dur-out", "mp4");

    let cut_timeline = CutTimeline {
        duration_nanos: 10_000_000_000,
        keeps: vec![
            KeepSegment {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(3_000_000_000),
            },
            KeepSegment {
                start: MediaTimestamp::from_nanos(7_000_000_000),
                end: MediaTimestamp::from_nanos(10_000_000_000),
            },
        ],
        cuts: vec![CutSegment {
            start: MediaTimestamp::from_nanos(3_000_000_000),
            end: MediaTimestamp::from_nanos(7_000_000_000),
            reason: CutReason::SilentAndStill,
            mean_audio_rms: 0.0,
            mean_visual_change: 0.0,
        }],
        total_cut_nanos: 4_000_000_000,
    };

    let mut exporter = FfmpegTrimExporter;
    export_recording_with_timeline(
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

    // Expected duration: ~6 seconds (10s - 4s cut).
    // Tolerance: ±500ms for codec delay and seek imprecision.
    let expected_nanos = 6_000_000_000u64;
    let tolerance_nanos = 500_000_000u64; // 500ms
    let diff = if inspection.duration_nanos > expected_nanos {
        inspection.duration_nanos - expected_nanos
    } else {
        expected_nanos - inspection.duration_nanos
    };
    assert!(
        diff <= tolerance_nanos,
        "output duration {}ns should be within ±500ms of expected {}ns (diff={}ns)",
        inspection.duration_nanos,
        expected_nanos,
        diff
    );

    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&output);
}

/// Export with keep segment at the very start of source produces valid output.
/// Source is 10s; keep only [0s, 1s] → output ~1s.
#[test]
fn start_of_source_keep_segment() {
    let source = create_source("int-start-keep", 10_000_000_000);
    let output = unique_path("int-start-keep-out", "mp4");

    let cut_timeline = CutTimeline {
        duration_nanos: 10_000_000_000,
        keeps: vec![KeepSegment {
            start: MediaTimestamp::from_nanos(0),
            end: MediaTimestamp::from_nanos(1_000_000_000),
        }],
        cuts: vec![CutSegment {
            start: MediaTimestamp::from_nanos(1_000_000_000),
            end: MediaTimestamp::from_nanos(10_000_000_000),
            reason: CutReason::SilentAndStill,
            mean_audio_rms: 0.0,
            mean_visual_change: 0.0,
        }],
        total_cut_nanos: 9_000_000_000,
    };

    let mut exporter = FfmpegTrimExporter;
    export_recording_with_timeline(
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

    // Expected duration: ~1s.
    let expected_nanos = 1_000_000_000u64;
    let tolerance_nanos = 500_000_000u64; // 500ms
    let diff = if inspection.duration_nanos > expected_nanos {
        inspection.duration_nanos - expected_nanos
    } else {
        expected_nanos - inspection.duration_nanos
    };
    assert!(
        diff <= tolerance_nanos,
        "output duration {}ns should be within ±500ms of expected {}ns (diff={}ns)",
        inspection.duration_nanos,
        expected_nanos,
        diff
    );

    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&output);
}

/// R4: When render_cursor_overlay=false and frames are empty, export succeeds.
/// This is the "raw cursor already visible" no-op path (BUG-007).
#[test]
fn ffmpeg_exporter_accepts_render_cursor_overlay_false_noop_timeline() {
    let source = create_source("int-cursor-noop", 500_000_000);
    let output = unique_path("int-cursor-noop-out", "mp4");

    // Write an effect timeline with render_cursor_overlay=false, empty frames.
    let timeline_path = unique_path("timeline-noop", "json");
    let timeline_json = r#"{
        "fps": 30,
        "durationNanos": 500000000,
        "frames": [],
        "clickEffects": [],
        "rawSystemCursorVisible": true,
        "renderCursorOverlay": false
    }"#;
    std::fs::write(&timeline_path, timeline_json).unwrap();

    let mut exporter = FfmpegTrimExporter;
    let result = export_recording_with_timeline(
        &mut exporter,
        source.clone(),
        Some(output.clone()),
        ExportPreset::Bilibili,
        CutTimeline::empty(500_000_000),
        Some(timeline_path.clone()),
        Arc::new(AtomicBool::new(false)),
        None,
        1,
    );

    assert!(
        result.is_ok(),
        "render_cursor_overlay=false with empty frames should succeed: {:?}",
        result.err()
    );
    assert!(output.exists());

    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&output);
    let _ = std::fs::remove_file(&timeline_path);
}

/// R4: When render_cursor_overlay=true but frames are empty, export must fail.
/// This prevents silent "no cursor, no beautification" exports (BUG-007).
#[test]
fn ffmpeg_exporter_rejects_required_overlay_with_empty_timeline() {
    let source = create_source("int-cursor-required", 500_000_000);
    let output = unique_path("int-cursor-required-out", "mp4");

    // Write an effect timeline with render_cursor_overlay=true, empty frames.
    let timeline_path = unique_path("timeline-required", "json");
    let timeline_json = r#"{
        "fps": 30,
        "durationNanos": 500000000,
        "frames": [],
        "clickEffects": [],
        "rawSystemCursorVisible": false,
        "renderCursorOverlay": true
    }"#;
    std::fs::write(&timeline_path, timeline_json).unwrap();

    let mut exporter = FfmpegTrimExporter;
    let result = export_recording_with_timeline(
        &mut exporter,
        source.clone(),
        Some(output.clone()),
        ExportPreset::Bilibili,
        CutTimeline::empty(500_000_000),
        Some(timeline_path.clone()),
        Arc::new(AtomicBool::new(false)),
        None,
        1,
    );

    assert!(
        result.is_err(),
        "render_cursor_overlay=true with empty frames must fail"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("光标美化") || err_msg.contains("效果时间线"),
        "error should mention cursor overlay failure: {}",
        err_msg
    );

    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&output);
    let _ = std::fs::remove_file(&timeline_path);
}
