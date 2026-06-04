use serde::{Deserialize, Serialize};

use crate::core::frame::MediaTimestamp;

/// Cursor shape kind detected during recording or inferred from context.
///
/// Used by the overlay renderer to draw the appropriate glyph instead of
/// a generic white circle. Defaults to `Arrow` for backward compatibility
/// with timelines that lack kind information.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CursorKind {
    /// Standard arrow pointer (default).
    #[default]
    Arrow,
    /// Hand pointer for clickable elements (buttons, links).
    Hand,
    /// I-beam text cursor for editable text fields.
    IBeam,
}

/// Cursor position sampled during recording using the shared media time base.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorSample {
    pub timestamp: MediaTimestamp,
    pub x: f32,
    pub y: f32,
    /// Cursor shape kind. Defaults to Arrow for legacy timelines.
    #[serde(default)]
    pub kind: CursorKind,
}

/// Mouse button recorded for click effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Click transition phase recorded from the cursor source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClickPhase {
    Down,
    Up,
}

/// Cursor click event with position captured at transition time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorClick {
    pub timestamp: MediaTimestamp,
    pub button: MouseButton,
    pub phase: ClickPhase,
    pub x: f32,
    pub y: f32,
}

/// Per-video-frame cursor effect state used by the post-process renderer.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorFrame {
    pub timestamp: MediaTimestamp,
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub opacity: f32,
    /// Cursor shape kind. Defaults to Arrow for legacy timelines.
    #[serde(default)]
    pub kind: CursorKind,
}

/// Click magnification effect window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorClickEffect {
    pub start: MediaTimestamp,
    pub end: MediaTimestamp,
    pub x: f32,
    pub y: f32,
    pub max_scale: f32,
    pub peak_opacity: f32,
}

/// Complete cursor effect timeline generated after recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectTimeline {
    pub fps: u32,
    pub duration_nanos: u64,
    pub frames: Vec<CursorFrame>,
    pub click_effects: Vec<CursorClickEffect>,
    /// Whether the raw SCK frames contain the system cursor.
    pub raw_system_cursor_visible: bool,
    /// Whether Phase 6 compositor should render a cursor overlay.
    pub render_cursor_overlay: bool,
    /// The PTS origin of the source video in nanoseconds.
    /// When decoding source MP4, the first frame's PTS may not be 0 in the
    /// SessionClock time domain. Overlay timestamps must subtract this origin
    /// to align with cursor timeline.
    #[serde(default)]
    pub source_pts_origin_nanos: u64,
}

/// Beautify config frozen at the moment recording starts. Stored in
/// RecordingMetadata. The `raw_system_cursor_visible` field is the immutable
/// safety constraint; other fields serve as an audit trail.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeautifyConfigSnapshot {
    pub cursor_magnification: bool,
    pub magnification_factor: f32,
    pub cursor_smoothing: bool,
    pub auto_trim_silences: bool,
    pub trim_sensitivity: String,
    pub raw_system_cursor_visible: bool,
}

/// Display geometry captured at recording start. Used to normalize
/// cursor coordinates from macOS global screen space to source video
/// pixel space.
///
/// Coordinate normalization formula:
/// ```text
/// local_x = global_x - content_origin_x
/// local_y = global_y - content_origin_y
/// source_x = local_x * stream_width / content_width
/// source_y = local_y * stream_height / content_height
/// ```
///
/// If the display Y-axis is flipped relative to `CGEventGetLocation()`,
/// the mapper must flip `local_y` before scaling.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureGeometry {
    /// CoreGraphics display identifier.
    pub display_id: u32,
    /// Content rect origin X in global screen points.
    pub content_origin_x: f32,
    /// Content rect origin Y in global screen points.
    pub content_origin_y: f32,
    /// Content rect width in global screen points.
    pub content_width: f32,
    /// Content rect height in global screen points.
    pub content_height: f32,
    /// Retina point-to-pixel scale (1.0 for non-Retina, 2.0 for Retina).
    pub point_pixel_scale: f32,
    /// Stream output width in pixels (source artifact frame width).
    pub stream_width: u32,
    /// Stream output height in pixels (source artifact frame height).
    pub stream_height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;

    #[test]
    fn cursor_sample_serializes_camel_case() {
        let sample = CursorSample {
            timestamp: MediaTimestamp::from_nanos(1_000_000),
            x: 120.5,
            y: 240.25,
            kind: CursorKind::Arrow,
        };

        let json = serde_json::to_string(&sample).unwrap();

        assert!(json.contains("\"timestamp\""));
        assert!(json.contains("\"nanos\":1000000"));
        assert!(json.contains("\"x\":120.5"));
        assert!(json.contains("\"y\":240.25"));
    }

    #[test]
    fn effect_timeline_round_trips() {
        let timeline = EffectTimeline {
            fps: 60,
            duration_nanos: 33_333_333,
            frames: vec![
                CursorFrame {
                    timestamp: MediaTimestamp::from_nanos(0),
                    x: 10.0,
                    y: 10.0,
                    scale: 1.0,
                    opacity: 1.0,
                    kind: CursorKind::Arrow,
                },
                CursorFrame {
                    timestamp: MediaTimestamp::from_nanos(16_666_667),
                    x: 20.0,
                    y: 20.0,
                    scale: 1.5,
                    opacity: 1.0,
                    kind: CursorKind::Arrow,
                },
            ],
            click_effects: vec![CursorClickEffect {
                start: MediaTimestamp::from_nanos(0),
                end: MediaTimestamp::from_nanos(300_000_000),
                x: 10.0,
                y: 10.0,
                max_scale: 2.0,
                peak_opacity: 0.35,
            }],
            raw_system_cursor_visible: false,
            render_cursor_overlay: true,
            source_pts_origin_nanos: 0,
        };

        let json = serde_json::to_string(&timeline).unwrap();
        let parsed: EffectTimeline = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, timeline);
    }

    #[test]
    fn capture_geometry_serializes_camel_case() {
        let geo = CaptureGeometry {
            display_id: 1,
            content_origin_x: 0.0,
            content_origin_y: 25.0,
            content_width: 1920.0,
            content_height: 1080.0,
            point_pixel_scale: 2.0,
            stream_width: 1920,
            stream_height: 1080,
        };
        let json = serde_json::to_string(&geo).unwrap();
        assert!(json.contains("\"displayId\""));
        assert!(json.contains("\"contentOriginX\""));
        assert!(json.contains("\"pointPixelScale\""));
        let parsed: CaptureGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, geo);
    }

    #[test]
    fn click_phase_and_button_use_stable_names() {
        let click = CursorClick {
            timestamp: MediaTimestamp::from_nanos(42),
            button: MouseButton::Left,
            phase: ClickPhase::Down,
            x: 1.0,
            y: 2.0,
        };

        let json = serde_json::to_string(&click).unwrap();

        assert!(json.contains("\"button\":\"left\""));
        assert!(json.contains("\"phase\":\"down\""));
    }

    #[test]
    fn cursor_kind_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&CursorKind::Arrow).unwrap(),
            "\"arrow\""
        );
        assert_eq!(
            serde_json::to_string(&CursorKind::Hand).unwrap(),
            "\"hand\""
        );
        assert_eq!(
            serde_json::to_string(&CursorKind::IBeam).unwrap(),
            "\"iBeam\""
        );
    }

    #[test]
    fn cursor_kind_defaults_to_arrow_for_old_json() {
        let json = r#"{"timestamp":{"nanos":0},"x":1.0,"y":2.0}"#;
        let sample: CursorSample = serde_json::from_str(json).unwrap();
        assert_eq!(sample.kind, CursorKind::Arrow);
    }

    #[test]
    fn cursor_frame_defaults_to_arrow_for_old_json() {
        let json = r#"{"timestamp":{"nanos":0},"x":1.0,"y":2.0,"scale":1.0,"opacity":1.0}"#;
        let frame: CursorFrame = serde_json::from_str(json).unwrap();
        assert_eq!(frame.kind, CursorKind::Arrow);
    }
}
