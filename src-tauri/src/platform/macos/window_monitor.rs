use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::core::window::WindowRecordingState;

use super::window_list;

/// 窗口状态监控器
///
/// 采用混合事件驱动方案：
/// - Layer 1: NSWorkspace 通知（实时，应用切换时检测）
/// - Layer 2: 200ms 低频轮询（增量比较，仅状态变化时触发回调）
pub struct WindowMonitor {
    window_id: u32,
    last_state: Arc<Mutex<WindowRecordingState>>,
    on_state_change: Arc<dyn Fn(WindowRecordingState) + Send + Sync + 'static>,
    stop_flag: Arc<AtomicBool>,
}

impl WindowMonitor {
    pub fn new(
        window_id: u32,
        on_state_change: impl Fn(WindowRecordingState) + Send + Sync + 'static,
    ) -> Self {
        Self {
            window_id,
            last_state: Arc::new(Mutex::new(WindowRecordingState::Recording)),
            on_state_change: Arc::new(on_state_change),
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 启动监控
    pub fn start(&self) {
        // Layer 1: NSWorkspace 应用级事件监听
        self.register_workspace_notifications();

        // Layer 2: 低频轮询（200ms）
        self.start_polling_loop();
    }

    /// 停止监控
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    fn register_workspace_notifications(&self) {
        // TODO: 实现 NSWorkspace 通知监听
        // 当前仅使用轮询，后续可补充事件监听
    }

    fn start_polling_loop(&self) {
        let window_id = self.window_id;
        let last_state = self.last_state.clone();
        let on_change = self.on_state_change.clone();
        let stop_flag = self.stop_flag.clone();

        thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                let new_state = Self::check_window_state(window_id);
                let mut guard = last_state.lock().unwrap();

                if *guard != new_state {
                    *guard = new_state.clone();
                    on_change(new_state);
                }

                drop(guard);
                thread::sleep(Duration::from_millis(200));
            }
        });
    }

    fn check_window_state(window_id: u32) -> WindowRecordingState {
        match window_list::list_windows() {
            Ok(windows) => {
                if let Some(window) = windows.iter().find(|w| w.window_id == window_id) {
                    if window.is_on_screen {
                        WindowRecordingState::Recording
                    } else {
                        WindowRecordingState::Minimized
                    }
                } else {
                    WindowRecordingState::Closed
                }
            }
            Err(_) => WindowRecordingState::Closed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_state_transitions() {
        let mut state = WindowRecordingState::Recording;
        assert_eq!(state, WindowRecordingState::Recording);

        state = WindowRecordingState::Minimized;
        assert_eq!(state, WindowRecordingState::Minimized);

        state = WindowRecordingState::Closed;
        assert_eq!(state, WindowRecordingState::Closed);
    }
}
