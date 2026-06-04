use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::app::error::AppResult;
use crate::core::clock::SessionClock;
use crate::core::frame::MediaTimestamp;
use crate::core::timeline::{
    BeautifyConfigSnapshot, CaptureGeometry, ClickPhase, CursorClick, CursorKind, CursorSample,
    MouseButton,
};
use crate::media::recording_metadata::RecordingMetadata;

const DEFAULT_MAX_CURSOR_SAMPLES: usize = 120_000;
const DEFAULT_MAX_CURSOR_CLICKS: usize = 10_000;

/// Snapshot of the current cursor position, button states, and kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorSnapshot {
    pub x: f32,
    pub y: f32,
    pub left_down: bool,
    pub right_down: bool,
    pub middle_down: bool,
    pub kind: CursorKind,
}

/// Source that can poll the current cursor state.
pub trait CursorSnapshotSource: Send + 'static {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot>;
}

/// Maps cursor coordinates from macOS global screen space to source
/// video pixel space.
///
/// Both `CGEventGetLocation()` and `SCDisplay.frame()` use the same macOS
/// global display coordinate system: origin at top-left of the primary
/// display, Y increases downward. No Y-axis flip is needed.
pub struct CursorCoordinateMapper {
    origin_x: f32,
    origin_y: f32,
    scale_x: f32,
    scale_y: f32,
    stream_width: f32,
    stream_height: f32,
}

impl CursorCoordinateMapper {
    pub fn new(geometry: &CaptureGeometry) -> Self {
        Self {
            origin_x: geometry.content_origin_x,
            origin_y: geometry.content_origin_y,
            scale_x: geometry.stream_width as f32 / geometry.content_width.max(1.0),
            scale_y: geometry.stream_height as f32 / geometry.content_height.max(1.0),
            stream_width: geometry.stream_width as f32,
            stream_height: geometry.stream_height as f32,
        }
    }

    /// Map global screen coordinates to source video pixel coordinates.
    /// Returns `None` if the cursor is outside the capture region.
    pub fn map(&self, global_x: f32, global_y: f32) -> Option<(f32, f32)> {
        let source_x = (global_x - self.origin_x) * self.scale_x;
        let source_y = (global_y - self.origin_y) * self.scale_y;

        if source_x < 0.0
            || source_y < 0.0
            || source_x > self.stream_width
            || source_y > self.stream_height
        {
            return None;
        }

        Some((source_x, source_y))
    }
}

/// Pure recorder that converts snapshots into timeline metadata.
pub struct CursorMetadataRecorder {
    fps: u32,
    max_samples: usize,
    max_clicks: usize,
    samples: VecDeque<CursorSample>,
    clicks: VecDeque<CursorClick>,
    previous_snapshot: Option<CursorSnapshot>,
    beautify_snapshot: BeautifyConfigSnapshot,
    snapshot_success_count: u64,
    snapshot_error_count: u64,
    coordinate_mapper: Option<CursorCoordinateMapper>,
}

impl CursorMetadataRecorder {
    pub fn new(
        fps: u32,
        beautify_snapshot: BeautifyConfigSnapshot,
        capture_geometry: Option<CaptureGeometry>,
    ) -> Self {
        Self::with_max_samples(
            fps,
            DEFAULT_MAX_CURSOR_SAMPLES,
            beautify_snapshot,
            capture_geometry,
        )
    }

    pub fn with_max_samples(
        fps: u32,
        max_samples: usize,
        beautify_snapshot: BeautifyConfigSnapshot,
        capture_geometry: Option<CaptureGeometry>,
    ) -> Self {
        Self {
            fps,
            max_samples,
            max_clicks: DEFAULT_MAX_CURSOR_CLICKS,
            samples: VecDeque::new(),
            clicks: VecDeque::new(),
            previous_snapshot: None,
            beautify_snapshot,
            snapshot_success_count: 0,
            snapshot_error_count: 0,
            coordinate_mapper: capture_geometry.map(|geo| CursorCoordinateMapper::new(&geo)),
        }
    }

    pub fn record_snapshot(&mut self, timestamp: MediaTimestamp, snapshot: CursorSnapshot) {
        self.snapshot_success_count += 1;
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }

        // Normalize coordinates from global screen space to source video pixel space.
        let (norm_x, norm_y) = if let Some(ref mapper) = self.coordinate_mapper {
            match mapper.map(snapshot.x, snapshot.y) {
                Some((x, y)) => (x, y),
                None => {
                    // Cursor outside capture region: skip this sample.
                    self.previous_snapshot = Some(snapshot);
                    return;
                }
            }
        } else {
            // No geometry available: use raw coordinates (legacy behavior).
            (snapshot.x, snapshot.y)
        };

        self.samples.push_back(CursorSample {
            timestamp,
            x: norm_x,
            y: norm_y,
            kind: snapshot.kind,
        });

        if let Some(previous) = self.previous_snapshot {
            self.record_button_transition(
                timestamp,
                snapshot,
                MouseButton::Left,
                previous.left_down,
                snapshot.left_down,
            );
            self.record_button_transition(
                timestamp,
                snapshot,
                MouseButton::Right,
                previous.right_down,
                snapshot.right_down,
            );
            self.record_button_transition(
                timestamp,
                snapshot,
                MouseButton::Middle,
                previous.middle_down,
                snapshot.middle_down,
            );
        }

        self.previous_snapshot = Some(snapshot);
    }

    fn record_button_transition(
        &mut self,
        timestamp: MediaTimestamp,
        snapshot: CursorSnapshot,
        button: MouseButton,
        was_down: bool,
        is_down: bool,
    ) {
        if was_down == is_down {
            return;
        }

        if self.clicks.len() >= self.max_clicks {
            self.clicks.pop_front();
        }

        self.clicks.push_back(CursorClick {
            timestamp,
            button,
            phase: if is_down {
                ClickPhase::Down
            } else {
                ClickPhase::Up
            },
            x: snapshot.x,
            y: snapshot.y,
        });
    }

    pub fn record_snapshot_failure(&mut self) {
        self.snapshot_error_count += 1;
    }

    pub fn finish(
        self,
        duration_nanos: u64,
        capture_geometry: Option<CaptureGeometry>,
    ) -> RecordingMetadata {
        RecordingMetadata {
            fps: self.fps,
            duration_nanos,
            cursor_samples: self.samples.into(),
            cursor_clicks: self.clicks.into(),
            beautify_config: self.beautify_snapshot,
            cursor_snapshot_success_count: self.snapshot_success_count,
            cursor_snapshot_error_count: self.snapshot_error_count,
            capture_geometry,
        }
    }
}

/// Background cursor metadata runtime. It polls at capture fps and stores only metadata.
pub struct CursorMetadataRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<RecordingMetadata>>,
}

impl CursorMetadataRuntime {
    pub fn spawn<S>(
        mut source: S,
        fps: u32,
        session_clock: Arc<SessionClock>,
        beautify_snapshot: BeautifyConfigSnapshot,
        capture_geometry: Option<CaptureGeometry>,
    ) -> Self
    where
        S: CursorSnapshotSource,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let interval = Duration::from_nanos(1_000_000_000u64 / fps.max(1) as u64);

        let handle = thread::spawn(move || {
            let mut recorder =
                CursorMetadataRecorder::new(fps.max(1), beautify_snapshot, capture_geometry);
            while !thread_stop.load(Ordering::Relaxed) {
                match source.snapshot() {
                    Ok(snapshot) => {
                        recorder.record_snapshot(
                            MediaTimestamp::from_nanos(session_clock.elapsed_nanos()),
                            snapshot,
                        );
                    }
                    Err(_) => {
                        recorder.record_snapshot_failure();
                    }
                }
                thread::sleep(interval);
            }

            recorder.finish(session_clock.elapsed_nanos(), capture_geometry)
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) -> Option<RecordingMetadata> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.take().and_then(|handle| handle.join().ok())
    }
}

impl Drop for CursorMetadataRuntime {
    fn drop(&mut self) {
        if self.handle.is_some() {
            let _ = self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::{ClickPhase, MouseButton};

    #[test]
    fn recorder_keeps_samples_in_order() {
        let mut recorder = CursorMetadataRecorder::new(
            30,
            BeautifyConfigSnapshot {
                cursor_magnification: true,
                magnification_factor: 2.0,
                cursor_smoothing: true,
                auto_trim_silences: false,
                trim_sensitivity: "medium".to_string(),
                raw_system_cursor_visible: false,
            },
            None,
        );

        recorder.record_snapshot(
            MediaTimestamp::from_nanos(0),
            CursorSnapshot {
                x: 1.0,
                y: 2.0,
                left_down: false,
                right_down: false,
                middle_down: false,
                kind: CursorKind::Arrow,
            },
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(33_333_333),
            CursorSnapshot {
                x: 3.0,
                y: 4.0,
                left_down: false,
                right_down: false,
                middle_down: false,
                kind: CursorKind::Arrow,
            },
        );

        let metadata = recorder.finish(66_666_666, None);

        assert_eq!(metadata.fps, 30);
        assert_eq!(metadata.cursor_samples.len(), 2);
        assert_eq!(metadata.cursor_samples[1].x, 3.0);
    }

    #[test]
    fn recorder_detects_left_click_down_and_up() {
        let mut recorder = CursorMetadataRecorder::new(
            60,
            BeautifyConfigSnapshot {
                cursor_magnification: true,
                magnification_factor: 2.0,
                cursor_smoothing: true,
                auto_trim_silences: false,
                trim_sensitivity: "medium".to_string(),
                raw_system_cursor_visible: false,
            },
            None,
        );

        recorder.record_snapshot(
            MediaTimestamp::from_nanos(0),
            CursorSnapshot {
                x: 10.0,
                y: 20.0,
                left_down: false,
                right_down: false,
                middle_down: false,
                kind: CursorKind::Arrow,
            },
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(16_666_666),
            CursorSnapshot {
                x: 11.0,
                y: 21.0,
                left_down: true,
                right_down: false,
                middle_down: false,
                kind: CursorKind::Arrow,
            },
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(33_333_333),
            CursorSnapshot {
                x: 12.0,
                y: 22.0,
                left_down: false,
                right_down: false,
                middle_down: false,
                kind: CursorKind::Arrow,
            },
        );

        let metadata = recorder.finish(50_000_000, None);

        assert_eq!(metadata.cursor_clicks.len(), 2);
        assert_eq!(metadata.cursor_clicks[0].button, MouseButton::Left);
        assert_eq!(metadata.cursor_clicks[0].phase, ClickPhase::Down);
        assert_eq!(metadata.cursor_clicks[1].phase, ClickPhase::Up);
    }

    #[test]
    fn recorder_caps_sample_count_to_bound_memory() {
        let mut recorder = CursorMetadataRecorder::with_max_samples(
            30,
            3,
            BeautifyConfigSnapshot {
                cursor_magnification: true,
                magnification_factor: 2.0,
                cursor_smoothing: true,
                auto_trim_silences: false,
                trim_sensitivity: "medium".to_string(),
                raw_system_cursor_visible: false,
            },
            None,
        );

        for i in 0..10 {
            recorder.record_snapshot(
                MediaTimestamp::from_nanos(i * 33_333_333),
                CursorSnapshot {
                    x: i as f32,
                    y: i as f32,
                    left_down: false,
                    right_down: false,
                    middle_down: false,
                    kind: CursorKind::Arrow,
                },
            );
        }

        let metadata = recorder.finish(333_333_333, None);

        assert_eq!(metadata.cursor_samples.len(), 3);
        assert_eq!(metadata.cursor_samples[0].x, 7.0);
    }

    #[test]
    fn recorder_stores_beautify_snapshot_in_metadata() {
        let snapshot = BeautifyConfigSnapshot {
            cursor_magnification: false,
            magnification_factor: 1.5,
            cursor_smoothing: false,
            auto_trim_silences: true,
            trim_sensitivity: "high".to_string(),
            raw_system_cursor_visible: true,
        };
        let recorder = CursorMetadataRecorder::new(30, snapshot.clone(), None);
        let metadata = recorder.finish(1_000_000_000, None);

        assert_eq!(metadata.beautify_config, snapshot);
    }

    #[test]
    fn recorder_counts_snapshot_successes_and_failures() {
        let mut recorder = CursorMetadataRecorder::new(
            30,
            BeautifyConfigSnapshot {
                cursor_magnification: true,
                magnification_factor: 2.0,
                cursor_smoothing: true,
                auto_trim_silences: false,
                trim_sensitivity: "medium".to_string(),
                raw_system_cursor_visible: false,
            },
            None,
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(0),
            CursorSnapshot {
                x: 1.0,
                y: 2.0,
                left_down: false,
                right_down: false,
                middle_down: false,
                kind: CursorKind::Arrow,
            },
        );
        recorder.record_snapshot_failure();

        let metadata = recorder.finish(33_333_333, None);

        assert_eq!(metadata.cursor_snapshot_success_count, 1);
        assert_eq!(metadata.cursor_snapshot_error_count, 1);
    }

    #[test]
    fn cursor_mapper_maps_identity_display_to_stream_pixels() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 0.0,
            content_width: 1920.0,
            content_height: 1080.0,
            point_pixel_scale: 1.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        let (x, y) = mapper.map(960.0, 540.0).unwrap();
        assert!((x - 960.0).abs() < 0.01);
        assert!((y - 540.0).abs() < 0.01);
    }

    #[test]
    fn cursor_mapper_subtracts_non_zero_display_origin() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 256.0,
            content_origin_y: 25.0,
            content_width: 1920.0,
            content_height: 1080.0,
            point_pixel_scale: 1.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        // Cursor at global (256+960, 25+540) should map to source (960, 540).
        let (x, y) = mapper.map(1216.0, 565.0).unwrap();
        assert!((x - 960.0).abs() < 0.01);
        assert!((y - 540.0).abs() < 1.0);
    }

    #[test]
    fn cursor_mapper_scales_points_to_1080p_stream() {
        // Retina 2x: display is 960x540 points, stream is 1920x1080 pixels.
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 0.0,
            content_width: 960.0,
            content_height: 540.0,
            point_pixel_scale: 2.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        let (x, y) = mapper.map(480.0, 270.0).unwrap();
        assert!((x - 960.0).abs() < 0.01);
        assert!((y - 540.0).abs() < 0.01);
    }

    #[test]
    fn cursor_mapper_marks_cursor_outside_capture_region() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 0.0,
            content_width: 1920.0,
            content_height: 1080.0,
            point_pixel_scale: 1.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        // Cursor far outside the display area.
        assert!(mapper.map(-500.0, -500.0).is_none());
        assert!(mapper.map(5000.0, 5000.0).is_none());
    }

    #[test]
    fn recorder_normalizes_coordinates_when_geometry_present() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 0.0,
            content_width: 1920.0,
            content_height: 1080.0,
            point_pixel_scale: 1.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mut recorder = CursorMetadataRecorder::new(
            30,
            BeautifyConfigSnapshot {
                cursor_magnification: false,
                magnification_factor: 1.0,
                cursor_smoothing: false,
                auto_trim_silences: false,
                trim_sensitivity: "medium".to_string(),
                raw_system_cursor_visible: false,
            },
            Some(geo),
        );

        // Cursor at center of display.
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(0),
            CursorSnapshot {
                x: 960.0,
                y: 540.0,
                left_down: false,
                right_down: false,
                middle_down: false,
                kind: CursorKind::Arrow,
            },
        );

        let metadata = recorder.finish(33_333_333, Some(geo));
        assert_eq!(metadata.cursor_samples.len(), 1);
        assert!((metadata.cursor_samples[0].x - 960.0).abs() < 0.01);
        assert!((metadata.cursor_samples[0].y - 540.0).abs() < 0.01);
    }

    #[test]
    fn cursor_mapper_maps_top_left_corner() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 0.0,
            content_width: 1920.0,
            content_height: 1080.0,
            point_pixel_scale: 1.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        // Top-left corner: global (0, 0).
        // With flip_y=true: local_y = 1080 - 0 = 1080, source_y = 1080 → out of bounds!
        // With flip_y=false: local_y = 0, source_y = 0 → correct.
        let result = mapper.map(0.0, 0.0);
        // Current implementation with flip_y=true returns None (out of bounds).
        // This is WRONG — top-left should map to (0, 0).
        assert!(
            result.is_some(),
            "top-left corner (0,0) should map to source (0,0), got None"
        );
        let (x, y) = result.unwrap();
        assert!((x - 0.0).abs() < 1.0, "expected x≈0, got {x}");
        assert!((y - 0.0).abs() < 1.0, "expected y≈0, got {y}");
    }

    #[test]
    fn cursor_mapper_maps_bottom_right_corner() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 0.0,
            content_width: 1920.0,
            content_height: 1080.0,
            point_pixel_scale: 1.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        // Bottom-right corner: global (1919, 1079).
        // With flip_y=true: local_y = 1080 - 1079 = 1, source_y = 1 → WRONG (should be ~1079).
        // With flip_y=false: local_y = 1079, source_y = 1079 → correct.
        let result = mapper.map(1919.0, 1079.0);
        assert!(
            result.is_some(),
            "bottom-right corner should map to source, got None"
        );
        let (x, y) = result.unwrap();
        assert!((x - 1919.0).abs() < 1.0, "expected x≈1919, got {x}");
        assert!((y - 1079.0).abs() < 1.0, "expected y≈1079, got {y}");
    }

    #[test]
    fn cursor_mapper_maps_lower_half_screen() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 0.0,
            content_width: 1920.0,
            content_height: 1080.0,
            point_pixel_scale: 1.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        // Cursor at y=900 (lower half of 1080p display).
        // With flip_y=true: source_y = 1080 - 900 = 180 → WRONG (should be 900).
        // With flip_y=false: source_y = 900 → correct.
        let (x, y) = mapper.map(960.0, 900.0).unwrap();
        assert!((x - 960.0).abs() < 0.01);
        assert!((y - 900.0).abs() < 1.0, "expected y≈900, got {y}");
    }

    #[test]
    fn cursor_mapper_handles_negative_origin() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: -1920.0,
            content_origin_y: 0.0,
            content_width: 1920.0,
            content_height: 1080.0,
            point_pixel_scale: 1.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        // Cursor on secondary display at global x=-960 (center of -1920..0 range).
        let (x, y) = mapper.map(-960.0, 540.0).unwrap();
        assert!((x - 960.0).abs() < 0.01, "expected x≈960, got {x}");
        assert!((y - 540.0).abs() < 1.0, "expected y≈540, got {y}");
    }

    #[test]
    fn cursor_mapper_retina_top_left() {
        // Retina 2x: display 960×540 points → 1920×1080 stream pixels.
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 0.0,
            content_width: 960.0,
            content_height: 540.0,
            point_pixel_scale: 2.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        // Top-left in points: (0, 0).
        let result = mapper.map(0.0, 0.0);
        assert!(result.is_some(), "Retina top-left should not be out of bounds");
        let (x, y) = result.unwrap();
        assert!((x - 0.0).abs() < 1.0, "expected x≈0, got {x}");
        assert!((y - 0.0).abs() < 1.0, "expected y≈0, got {y}");
    }

    #[test]
    fn cursor_mapper_retina_bottom_right() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 0.0,
            content_width: 960.0,
            content_height: 540.0,
            point_pixel_scale: 2.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let mapper = CursorCoordinateMapper::new(&geo);
        // Bottom-right in points: (959, 539).
        let result = mapper.map(959.0, 539.0);
        assert!(result.is_some(), "Retina bottom-right should not be out of bounds");
        let (x, y) = result.unwrap();
        assert!((x - 1918.0).abs() < 2.0, "expected x≈1918, got {x}");
        assert!((y - 1078.0).abs() < 2.0, "expected y≈1078, got {y}");
    }
}
