/// Recording target type selected by the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode {
    FullScreen,
    /// 窗口录制
    Window,
    /// 区域录制（开发中，暂不可用）
    Area,
}

impl CaptureMode {
    pub fn mode_from_str(s: &str) -> Result<Self, String> {
        match s {
            "fullscreen" => Ok(Self::FullScreen),
            "window" => Ok(Self::Window),
            "area" => Ok(Self::Area),
            other => Err(format!("未知捕获模式: {other}")),
        }
    }
}

/// Capture settings used to start a recording session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureConfig {
    pub mode: CaptureMode,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub show_system_cursor: bool,
    /// 目标窗口 ID（仅 Window 模式有效）
    pub window_id: Option<u32>,
}

impl CaptureConfig {
    /// Default Phase 1 target: full-screen 1080p at 30 fps.
    pub fn full_screen_1080p_30fps() -> Self {
        Self {
            mode: CaptureMode::FullScreen,
            width: 1920,
            height: 1080,
            fps: 30,
            show_system_cursor: true,
            window_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_phase_one_config_targets_1080p() {
        let config = CaptureConfig::full_screen_1080p_30fps();

        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
        assert_eq!(config.fps, 30);
    }

    #[test]
    fn parse_fullscreen_mode() {
        assert_eq!(
            CaptureMode::mode_from_str("fullscreen").unwrap(),
            CaptureMode::FullScreen
        );
    }

    #[test]
    fn parse_window_mode() {
        assert_eq!(
            CaptureMode::mode_from_str("window").unwrap(),
            CaptureMode::Window
        );
    }

    #[test]
    fn parse_area_mode() {
        assert_eq!(
            CaptureMode::mode_from_str("area").unwrap(),
            CaptureMode::Area
        );
    }

    #[test]
    fn parse_invalid_mode_returns_error() {
        let result = CaptureMode::mode_from_str("invalid");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("未知"));
    }

    #[test]
    fn parse_empty_string_returns_error() {
        let result = CaptureMode::mode_from_str("");
        assert!(result.is_err());
    }

    #[test]
    fn default_config_records_system_cursor_until_effect_pipeline_is_enabled() {
        let config = CaptureConfig::full_screen_1080p_30fps();

        assert!(config.show_system_cursor);
    }
}
