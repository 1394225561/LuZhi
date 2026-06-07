use serde::{Deserialize, Serialize};

/// 窗口元数据，用于前端展示和选择
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// 窗口唯一标识符（CGWindowID / SCWindow.windowID）
    pub window_id: u32,
    /// 窗口标题
    pub title: String,
    /// 所属应用名称
    pub app_name: String,
    /// 应用 Bundle ID（用于获取应用图标）
    pub bundle_id: Option<String>,
    /// 窗口是否在屏幕上可见（未最小化）
    pub is_on_screen: bool,
    /// 窗口尺寸（点）
    pub width: f64,
    pub height: f64,
    /// 窗口缩略图（Base64 编码的 PNG，可选延迟加载）
    pub thumbnail: Option<String>,
}

/// 窗口录制状态
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum WindowRecordingState {
    /// 正常录制中
    Recording,
    /// 窗口已最小化，录制暂停
    Minimized,
    /// 窗口已关闭，录制停止
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_info_serializes_to_camel_case_json() {
        let info = WindowInfo {
            window_id: 123,
            title: "Safari".to_string(),
            app_name: "Safari".to_string(),
            bundle_id: Some("com.apple.Safari".to_string()),
            is_on_screen: true,
            width: 1920.0,
            height: 1080.0,
            thumbnail: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"windowId\":123"));
        assert!(json.contains("\"appName\":\"Safari\""));
        assert!(json.contains("\"isOnScreen\":true"));
    }

    #[test]
    fn window_recording_state_equality() {
        assert_eq!(
            WindowRecordingState::Recording,
            WindowRecordingState::Recording
        );
        assert_ne!(
            WindowRecordingState::Recording,
            WindowRecordingState::Minimized
        );
    }
}
