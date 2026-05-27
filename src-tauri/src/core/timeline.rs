use serde::{Deserialize, Serialize};

use crate::core::frame::MediaTimestamp;

/// Cursor position sampled during recording using the shared media time base.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorSample {
    pub timestamp: MediaTimestamp,
    pub x: f32,
    pub y: f32,
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
                },
                CursorFrame {
                    timestamp: MediaTimestamp::from_nanos(16_666_667),
                    x: 20.0,
                    y: 20.0,
                    scale: 1.5,
                    opacity: 1.0,
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
        };

        let json = serde_json::to_string(&timeline).unwrap();
        let parsed: EffectTimeline = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, timeline);
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
}
