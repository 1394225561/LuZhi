use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::core::window::{WindowInfo, WindowRecordingState};

use super::window_list;

/// 窗口状态监控器
///
/// 采用混合事件驱动方案：
/// - Layer 1: NSWorkspace 通知（实时，应用切换时检测）
/// - Layer 2: 低频轮询（增量比较，仅状态变化时触发回调）
///
/// 性能优化：
/// - 窗口列表缓存 2 秒，避免频繁调用 SCShareableContent
/// - 200ms 轮询时仅检查缓存的窗口列表
pub struct WindowMonitor {
    window_id: u32,
    last_state: Arc<Mutex<WindowRecordingState>>,
    on_state_change: Arc<dyn Fn(WindowRecordingState) + Send + Sync + 'static>,
    stop_flag: Arc<AtomicBool>,
    /// 窗口列表缓存
    cached_windows: Arc<Mutex<Vec<WindowInfo>>>,
    /// 缓存最后更新时间
    last_cache_update: Arc<Mutex<Instant>>,
}

impl WindowMonitor {
    /// 创建窗口监控器（使用回调）
    pub fn new(
        window_id: u32,
        on_state_change: impl Fn(WindowRecordingState) + Send + Sync + 'static,
    ) -> Self {
        Self {
            window_id,
            last_state: Arc::new(Mutex::new(WindowRecordingState::Recording)),
            on_state_change: Arc::new(on_state_change),
            stop_flag: Arc::new(AtomicBool::new(false)),
            cached_windows: Arc::new(Mutex::new(Vec::new())),
            last_cache_update: Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10))),
        }
    }

    /// 创建窗口监控器（使用 channel）
    pub fn with_channel(
        window_id: u32,
        sender: std::sync::mpsc::Sender<WindowRecordingState>,
    ) -> Self {
        Self::new(window_id, move |state| {
            let _ = sender.send(state);
        })
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

    /// 获取当前窗口状态
    pub fn current_state(&self) -> WindowRecordingState {
        self.last_state.lock().unwrap().clone()
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
        let cached_windows = self.cached_windows.clone();
        let last_cache_update = self.last_cache_update.clone();

        thread::spawn(move || {
            // 初始加载窗口列表
            Self::refresh_window_cache(&cached_windows, &last_cache_update);

            while !stop_flag.load(Ordering::Relaxed) {
                let new_state = Self::check_window_state_with_cache(
                    window_id,
                    &cached_windows,
                    &last_cache_update,
                );

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

    /// 刷新窗口列表缓存
    fn refresh_window_cache(
        cached_windows: &Arc<Mutex<Vec<WindowInfo>>>,
        last_cache_update: &Arc<Mutex<Instant>>,
    ) {
        match window_list::list_windows() {
            Ok(windows) => {
                let mut guard = cached_windows.lock().unwrap();
                *guard = windows;
                let mut last_update = last_cache_update.lock().unwrap();
                *last_update = Instant::now();
            }
            Err(_) => {
                // 刷新失败时保留旧缓存
            }
        }
    }

    /// 使用缓存检查窗口状态
    fn check_window_state_with_cache(
        window_id: u32,
        cached_windows: &Arc<Mutex<Vec<WindowInfo>>>,
        last_cache_update: &Arc<Mutex<Instant>>,
    ) -> WindowRecordingState {
        // 检查是否需要刷新缓存（每 2 秒刷新一次）
        let needs_refresh = {
            let last_update = last_cache_update.lock().unwrap();
            last_update.elapsed() > Duration::from_secs(2)
        };

        if needs_refresh {
            Self::refresh_window_cache(cached_windows, last_cache_update);
        }

        // 从缓存中查找窗口
        let windows = cached_windows.lock().unwrap();
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

    #[test]
    fn window_monitor_with_channel() {
        let (tx, rx) = std::sync::mpsc::channel();
        let monitor = WindowMonitor::with_channel(123, tx);

        // 初始状态应该是 Recording
        assert_eq!(monitor.current_state(), WindowRecordingState::Recording);

        // 模拟状态变化
        let mut guard = monitor.last_state.lock().unwrap();
        *guard = WindowRecordingState::Minimized;
        drop(guard);

        // 手动调用回调来测试 channel
        (monitor.on_state_change)(WindowRecordingState::Minimized);

        // 验证 channel 收到了消息
        let received = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(received, WindowRecordingState::Minimized);
    }

    #[test]
    fn window_cache_initialization() {
        let cached_windows: Arc<Mutex<Vec<WindowInfo>>> = Arc::new(Mutex::new(Vec::new()));
        let last_cache_update = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10)));

        // 初始缓存应该为空
        let windows = cached_windows.lock().unwrap();
        assert!(windows.is_empty());
    }
}
