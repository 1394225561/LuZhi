/// Recording target type selected by the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode {
    FullScreen,
}

/// Capture settings used to start a recording session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureConfig {
    pub mode: CaptureMode,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl CaptureConfig {
    /// Default Phase 1 target: full-screen 1080p at 30 fps.
    pub fn full_screen_1080p_30fps() -> Self {
        Self {
            mode: CaptureMode::FullScreen,
            width: 1920,
            height: 1080,
            fps: 30,
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
}
