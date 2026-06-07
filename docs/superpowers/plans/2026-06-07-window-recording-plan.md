# 窗口录制功能实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 LuZhi 添加 macOS 窗口录制功能，允许用户选择单个应用窗口进行录制，而非全屏。

**Architecture:** 复用现有 ScreenCaptureKit 架构，通过 `SCContentFilter` 指定单个窗口进行捕获。新增 `WindowCapture` Trait 作为平台抽象层，macOS 完整实现，Windows 占位预留。窗口状态监控采用混合事件驱动（NSWorkspace 通知 + 200ms 轮询）。

**Tech Stack:** Rust, ScreenCaptureKit (objc2), Tauri 2, React, TypeScript, Tailwind CSS

---

## 文件结构

### 新增文件

| 文件 | 职责 |
|------|------|
| `src-tauri/src/core/window.rs` | `WindowInfo`、`WindowRecordingState` 数据结构 |
| `src-tauri/src/platform/macos/window_list.rs` | 窗口枚举和缩略图获取 |
| `src-tauri/src/platform/macos/window_monitor.rs` | 窗口状态监控（混合事件驱动） |
| `src-tauri/src/platform/windows/window_capture.rs` | Windows 窗口捕获占位实现 |
| `src/components/window-selector.tsx` | 窗口选择器组件 |
| `src/components/window-selector.test.tsx` | 窗口选择器测试 |

### 修改文件

| 文件 | 改动说明 |
|------|----------|
| `src-tauri/src/core/config.rs:3-8` | 扩展 `CaptureMode::Window` 移除开发中注释，`CaptureConfig` 添加 `window_id` 字段 |
| `src-tauri/src/core/capture.rs` | 添加 `WindowCapture` Trait |
| `src-tauri/src/app/error.rs:8-60` | 添加窗口相关错误类型 |
| `src-tauri/src/platform/macos/screen_capture_kit.rs` | 添加 `start_window_stream()` 方法 |
| `src-tauri/src/platform/macos_service.rs` | 集成窗口录制逻辑 |
| `src-tauri/src/lib.rs:162-172` | 添加 Tauri 命令，修改 `start_recording` |
| `src/lib/tauri.ts` | 添加 `WindowInfo` 类型和 `list_windows`、`set_window_id` 函数 |
| `src/components/recording-panel.tsx` | 集成窗口选择器，修改模式切换逻辑 |
| `src/components/recording-panel.test.tsx` | 添加窗口模式测试 |

---

## Task 1: 数据结构与错误类型

**Files:**
- Modify: `src-tauri/src/core/config.rs:3-43`
- Create: `src-tauri/src/core/window.rs`
- Modify: `src-tauri/src/app/error.rs:8-60`

- [ ] **Step 1: 扩展 CaptureConfig 添加 window_id 字段**

```rust
// src-tauri/src/core/config.rs

/// Recording target type selected by the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode {
    FullScreen,
    /// 窗口录制
    Window,
    /// 区域录制（开发中，暂不可用）
    Area,
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
```

- [ ] **Step 2: 创建 WindowInfo 和 WindowRecordingState 数据结构**

```rust
// src-tauri/src/core/window.rs

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
#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum WindowRecordingState {
    /// 正常录制中
    Recording,
    /// 窗口已最小化，录制暂停
    Minimized,
    /// 窗口已关闭，录制停止
    Closed,
}
```

- [ ] **Step 3: 扩展 AppError 添加窗口相关错误类型**

```rust
// src-tauri/src/app/error.rs

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AppError {
    // 现有错误保持不变...

    /// 窗口未找到
    WindowNotFound { window_id: u32 },
    /// 窗口已最小化，无法启动录制
    WindowMinimized { window_id: u32 },
    /// 窗口已关闭
    WindowClosed { window_id: u32 },
}

// 在 Display impl 中添加：
AppError::WindowNotFound { window_id } => {
    write!(formatter, "窗口未找到：{window_id}")
}
AppError::WindowMinimized { window_id } => {
    write!(formatter, "窗口已最小化，请恢复窗口后重试：{window_id}")
}
AppError::WindowClosed { window_id } => {
    write!(formatter, "窗口已关闭：{window_id}")
}
```

- [ ] **Step 4: 在 lib.rs 中声明新模块**

```rust
// src-tauri/src/core/mod.rs 或 src-tauri/src/lib.rs
pub mod window;
```

- [ ] **Step 5: 运行 Rust 测试验证编译**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/core/config.rs src-tauri/src/core/window.rs src-tauri/src/app/error.rs
git commit -m "feat(core): 添加窗口录制数据结构和错误类型"
```

---

## Task 2: WindowCapture 平台抽象 Trait

**Files:**
- Modify: `src-tauri/src/core/capture.rs`

- [ ] **Step 1: 添加 WindowCapture Trait 定义**

```rust
// src-tauri/src/core/capture.rs

use crate::core::window::{WindowInfo, WindowRecordingState};
use std::sync::Arc;

/// 平台无关的窗口捕获接口
pub trait WindowCapture: Send {
    /// 获取当前可见窗口列表
    fn list_windows(&self) -> AppResult<Vec<WindowInfo>>;

    /// 获取窗口缩略图（Base64 PNG）
    fn get_thumbnail(&self, window_id: u32) -> AppResult<Option<String>>;

    /// 启动窗口捕获流
    fn start_window_stream(
        &mut self,
        window_id: u32,
        capture_system_audio: bool,
        video_sink: VideoFrameSink,
        audio_sink: AudioChunkSink,
        session_clock: Arc<crate::core::clock::SessionClock>,
    ) -> AppResult<()>;

    /// 停止窗口捕获流
    fn stop_window_stream(&mut self) -> AppResult<()>;

    /// 检查窗口当前状态
    fn window_state(&self, window_id: u32) -> AppResult<WindowRecordingState>;
}
```

- [ ] **Step 2: 运行编译检查**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/core/capture.rs
git commit -m "feat(core): 添加 WindowCapture 平台抽象 Trait"
```

---

## Task 3: Windows 占位实现

**Files:**
- Create: `src-tauri/src/platform/windows/window_capture.rs`

- [ ] **Step 1: 创建 WinWindowCapture 占位实现**

```rust
// src-tauri/src/platform/windows/window_capture.rs

use std::sync::Arc;

use crate::app::error::{AppError, AppResult};
use crate::core::capture::{AudioChunkSink, VideoFrameSink, WindowCapture};
use crate::core::window::{WindowInfo, WindowRecordingState};

/// Windows 窗口捕获占位实现
///
/// 当前为占位实现，所有方法返回 NativeCaptureUnavailable。
/// 待 DXGI Desktop Duplication 完整实现后，补充窗口捕获逻辑。
pub struct WinWindowCapture;

impl WindowCapture for WinWindowCapture {
    fn list_windows(&self) -> AppResult<Vec<WindowInfo>> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }

    fn get_thumbnail(&self, _window_id: u32) -> AppResult<Option<String>> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }

    fn start_window_stream(
        &mut self,
        _window_id: u32,
        _capture_system_audio: bool,
        _video_sink: VideoFrameSink,
        _audio_sink: AudioChunkSink,
        _session_clock: Arc<crate::core::clock::SessionClock>,
    ) -> AppResult<()> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }

    fn stop_window_stream(&mut self) -> AppResult<()> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }

    fn window_state(&self, _window_id: u32) -> AppResult<WindowRecordingState> {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口录制尚未实现",
        })
    }
}
```

- [ ] **Step 2: 在 platform/mod.rs 中声明模块**

```rust
// src-tauri/src/platform/mod.rs 或 windows/mod.rs
pub mod window_capture;
```

- [ ] **Step 3: 运行编译检查**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/platform/windows/window_capture.rs
git commit -m "feat(platform): 添加 Windows 窗口捕获占位实现"
```

---

## Task 4: macOS 窗口枚举实现

**Files:**
- Create: `src-tauri/src/platform/macos/window_list.rs`

- [ ] **Step 1: 创建 window_list.rs 实现窗口枚举**

```rust
// src-tauri/src/platform/macos/window_list.rs

use objc2::rc::Retained;
use objc2_foundation::NSArray;
use objc2_screen_capture_kit::{SCShareableContent, SCWindow};

use crate::app::error::{AppError, AppResult};
use crate::core::window::WindowInfo;

/// 获取当前可见窗口列表
pub fn list_windows() -> AppResult<Vec<WindowInfo>> {
    let content = get_shareable_content_sync()?;
    let windows = unsafe { content.windows() };
    let mut result = Vec::new();

    for i in 0..windows.count() {
        let window = unsafe { windows.objectAtIndex(i) };

        // 过滤条件
        if !should_include_window(&window) {
            continue;
        }

        let info = window_to_info(&window);
        result.push(info);
    }

    // 按应用名称排序
    result.sort_by(|a, b| a.app_name.cmp(&b.app_name).then(a.title.cmp(&b.title)));

    Ok(result)
}

/// 获取窗口缩略图（Base64 PNG）
pub fn get_window_thumbnail(window_id: u32) -> AppResult<Option<String>> {
    // TODO: 实现 SCScreenshotManager.captureImage
    // 当前返回 None，缩略图为可选功能
    Ok(None)
}

/// 同步获取 SCShareableContent
fn get_shareable_content_sync() -> AppResult<Retained<SCShareableContent>> {
    use std::sync::{Arc, Mutex};
    use block2::RcBlock;

    let result: Arc<Mutex<Option<AppResult<Retained<SCShareableContent>>>>> =
        Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    let block = RcBlock::new(move |content: *mut SCShareableContent, error: *mut objc2_foundation::NSError| {
        let outcome = if error.is_null() && !content.is_null() {
            Ok(unsafe { Retained::retain(content) }.unwrap())
        } else {
            Err(AppError::CaptureFailed {
                reason: "获取屏幕内容失败".to_string(),
            })
        };
        *result_clone.lock().unwrap() = Some(outcome);
    });

    unsafe {
        SCShareableContent::getShareableContentWithCompletionHandler(&block);
    }

    // 等待异步回调完成
    let mut attempts = 0;
    loop {
        let guard = result.lock().unwrap();
        if guard.is_some() {
            return guard.as_ref().unwrap().clone();
        }
        drop(guard);

        attempts += 1;
        if attempts > 100 {
            return Err(AppError::CaptureFailed {
                reason: "获取屏幕内容超时".to_string(),
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 判断窗口是否应该包含在列表中
fn should_include_window(window: &SCWindow) -> bool {
    // 排除无标题窗口
    let title = unsafe { window.title() };
    if title.is_none() || title.unwrap().len() == 0 {
        return false;
    }

    // 排除自身应用窗口（LuZhi）
    let app_name = unsafe { window.owningApplication() }
        .map(|app| unsafe { app.applicationName() }.to_string())
        .unwrap_or_default();

    if app_name == "LuZhi" || app_name == "录智" {
        return false;
    }

    true
}

/// 将 SCWindow 转换为 WindowInfo
fn window_to_info(window: &SCWindow) -> WindowInfo {
    let title = unsafe { window.title() }
        .map(|s| s.to_string())
        .unwrap_or_default();

    let (app_name, bundle_id) = unsafe { window.owningApplication() }
        .map(|app| {
            let name = unsafe { app.applicationName() }.to_string();
            let bundle = unsafe { app.bundleIdentifier() }.map(|s| s.to_string());
            (name, bundle)
        })
        .unwrap_or_default();

    let frame = unsafe { window.frame() };
    let is_on_screen = unsafe { window.isOnScreen() };

    WindowInfo {
        window_id: unsafe { window.windowID() },
        title,
        app_name,
        bundle_id,
        is_on_screen,
        width: frame.size.width as f64,
        height: frame.size.height as f64,
        thumbnail: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_info_serializes_to_json() {
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
    }
}
```

- [ ] **Step 2: 在 platform/macos/mod.rs 中声明模块**

```rust
// src-tauri/src/platform/macos/mod.rs
pub mod window_list;
```

- [ ] **Step 3: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p luzhi -- window_list
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/platform/macos/window_list.rs
git commit -m "feat(macos): 实现窗口枚举功能"
```

---

## Task 5: macOS 窗口状态监控

**Files:**
- Create: `src-tauri/src/platform/macos/window_monitor.rs`

- [ ] **Step 1: 创建 window_monitor.rs 实现混合事件监控**

```rust
// src-tauri/src/platform/macos/window_monitor.rs

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
    on_state_change: Box<dyn Fn(WindowRecordingState) + Send + 'static>,
    stop_flag: Arc<AtomicBool>,
}

impl WindowMonitor {
    pub fn new(
        window_id: u32,
        on_state_change: impl Fn(WindowRecordingState) + Send + 'static,
    ) -> Self {
        Self {
            window_id,
            last_state: Arc::new(Mutex::new(WindowRecordingState::Recording)),
            on_state_change: Box::new(on_state_change),
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 启动监控
    pub fn start(&mut self) {
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
        let on_change = &self.on_state_change as *const dyn Fn(WindowRecordingState);
        let stop_flag = self.stop_flag.clone();

        // SAFETY: on_change 生命周期由 WindowMonitor 保证
        let on_change = unsafe { &*on_change };

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
```

- [ ] **Step 2: 在 platform/macos/mod.rs 中声明模块**

```rust
// src-tauri/src/platform/macos/mod.rs
pub mod window_monitor;
```

- [ ] **Step 3: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p luzhi -- window_monitor
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/platform/macos/window_monitor.rs
git commit -m "feat(macos): 实现窗口状态监控（混合事件驱动）"
```

---

## Task 6: MacScreenCapture 窗口捕获扩展

**Files:**
- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs`

- [ ] **Step 1: 添加 start_window_stream 方法**

```rust
// src-tauri/src/platform/macos/screen_capture_kit.rs

impl MacScreenCapture {
    /// 启动窗口捕获流
    pub fn start_window_stream(
        &mut self,
        window_id: u32,
        capture_system_audio: bool,
        video_sink: VideoFrameSink,
        audio_sink: AudioChunkSink,
        session_clock: Arc<crate::core::clock::SessionClock>,
    ) -> AppResult<()> {
        use objc2_foundation::NSArray;

        // 检查是否需要重置
        if self.needs_reset {
            return Err(AppError::NativeCaptureUnavailable {
                reason: "上次停止录制超时，请重启应用后再试",
            });
        }

        // 获取 SCShareableContent
        let content = Self::get_shareable_content_sync()?;
        let windows = unsafe { content.windows() };

        // 查找目标窗口
        let target_window = (0..windows.count())
            .map(|i| unsafe { windows.objectAtIndex(i) })
            .find(|w| unsafe { w.windowID() } == window_id)
            .ok_or(AppError::WindowNotFound { window_id })?;

        // 验证窗口是否在屏幕上
        if !unsafe { target_window.isOnScreen() } {
            return Err(AppError::WindowMinimized { window_id });
        }

        // 读取窗口几何信息
        let frame = unsafe { target_window.frame() };
        let stream_width = frame.size.width as u32;
        let stream_height = frame.size.height as u32;

        let capture_geometry = crate::core::timeline::CaptureGeometry {
            display_id: 0, // 窗口模式不需要 display_id
            content_origin_x: frame.origin.x as f32,
            content_origin_y: frame.origin.y as f32,
            content_width: frame.size.width as f32,
            content_height: frame.size.height as f32,
            point_pixel_scale: 1.0,
            stream_width,
            stream_height,
        };
        self.last_capture_geometry = Some(capture_geometry);

        // 创建内容过滤器：仅捕获目标窗口
        let empty_windows: Retained<NSArray<SCWindow>> = unsafe { NSArray::new() };
        let filter = unsafe {
            SCContentFilter::initWithDesktopIndependentWindow(
                SCContentFilter::alloc(),
                &target_window,
            )
        };

        // 配置流
        let stream_config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            stream_config.setWidth(stream_width as usize);
            stream_config.setHeight(stream_height as usize);
            stream_config.setCapturesAudio(capture_system_audio);
            stream_config.setSampleRate(48000);
            stream_config.setChannelCount(2);
            stream_config.setShowsCursor(true); // 窗口模式始终显示光标
            stream_config.setQueueDepth(8);
            stream_config.setPixelFormat(0x42475241); // BGRA

            let frame_interval = objc2_core_media::CMTime {
                value: 1,
                timescale: 30, // 默认 30fps
                flags: objc2_core_media::CMTimeFlags(1),
                epoch: 0,
            };
            stream_config.setMinimumFrameInterval(frame_interval);
        }

        // 创建流代理
        let delegate = StreamOutput::new(video_sink, audio_sink, session_clock);

        // 创建 SCStream
        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &stream_config,
                Some(objc2::runtime::ProtocolObject::from_ref(&*delegate)),
            )
        };

        // 添加视频输出
        unsafe {
            if let Err(e) = stream.addStreamOutput_type_sampleHandlerQueue_error(
                objc2::runtime::ProtocolObject::from_ref(&*delegate),
                SCStreamOutputType::Screen,
                None,
            ) {
                return Err(AppError::CaptureFailed {
                    reason: format!("添加视频输出失败: {}", e),
                });
            }
        }

        // 添加音频输出（如果需要）
        if capture_system_audio {
            unsafe {
                if let Err(e) = stream.addStreamOutput_type_sampleHandlerQueue_error(
                    objc2::runtime::ProtocolObject::from_ref(&*delegate),
                    SCStreamOutputType::Audio,
                    None,
                ) {
                    return Err(AppError::CaptureFailed {
                        reason: format!("添加音频输出失败: {}", e),
                    });
                }
            }
        }

        // 启动流
        unsafe {
            if let Err(e) = stream.startCaptureWithError() {
                return Err(AppError::CaptureFailed {
                    reason: format!("启动捕获失败: {}", e),
                });
            }
        }

        self.stream = Some(SendSCStream(stream));
        self.delegate = Some(delegate);
        self.running = true;
        self.audio_sink = if capture_system_audio {
            Some(audio_sink)
        } else {
            None
        };

        Ok(())
    }
}
```

- [ ] **Step 2: 运行编译检查**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/platform/macos/screen_capture_kit.rs
git commit -m "feat(macos): 扩展 MacScreenCapture 支持窗口捕获"
```

---

## Task 7: MacRecordingService 集成窗口录制

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs`

- [ ] **Step 1: 添加窗口录制启动方法**

```rust
// src-tauri/src/platform/macos_service.rs

impl MacRecordingService {
    /// 启动窗口录制
    pub fn start_window(
        &mut self,
        window_id: u32,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        self.state_machine.start()?;

        // 清理旧会话状态
        self.last_cursor_metadata_path = None;
        self.last_effect_timeline_path = None;
        self.last_trim_metadata_path = None;
        self.last_cut_timeline_path = None;
        self.last_recording_output_path = None;
        self.session_id = self.session_id.wrapping_add(1);

        // 重置麦克风电平
        if let Ok(mut guard) = self.mic_level.lock() {
            *guard = 0.0;
        }

        // 创建会话时钟
        let session_clock = Arc::new(crate::core::clock::SessionClock::new());

        // 创建通道
        const VIDEO_QUEUE_CAPACITY: usize = 90;
        const AUDIO_QUEUE_CAPACITY: usize = 256;
        let (video_sender, video_receiver) = bounded_media_channel(VIDEO_QUEUE_CAPACITY, "video");
        let (audio_sender, audio_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY, "system");

        // 启动窗口捕获流
        if let Err(error) = self.screen_capture.start_window_stream(
            window_id,
            audio_config.capture_system_audio,
            video_sender,
            audio_sender,
            session_clock.clone(),
        ) {
            self.state_machine.force_idle();
            return Err(error);
        }

        // 保存接收器
        self.video_receiver = Some(video_receiver);
        self.system_audio_receiver = Some(audio_receiver);
        self.last_requested_system_audio = audio_config.capture_system_audio;
        self.last_requested_microphone = audio_config.capture_microphone;

        // 启动麦克风（如果启用）
        if audio_config.capture_microphone {
            let (mic_sender, mic_receiver) = bounded_media_channel(AUDIO_QUEUE_CAPACITY, "mic");
            // ... 启动麦克风捕获
            self.mic_receiver = Some(mic_receiver);
        }

        Ok(())
    }
}
```

- [ ] **Step 2: 运行编译检查**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "feat(service): 集成窗口录制到 MacRecordingService"
```

---

## Task 8: Tauri 命令层

**Files:**
- Modify: `src-tauri/src/lib.rs:162-220`

- [ ] **Step 1: 添加 list_windows 命令**

```rust
// src-tauri/src/lib.rs

use crate::core::window::WindowInfo;

#[tauri::command]
async fn list_windows() -> Result<Vec<WindowInfo>, String> {
    #[cfg(target_os = "macos")]
    {
        platform::macos::window_list::list_windows().map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("窗口录制尚未支持当前平台".to_string())
    }
}
```

- [ ] **Step 2: 添加 set_window_id 命令**

```rust
// src-tauri/src/lib.rs

#[tauri::command]
async fn set_window_id(
    state: tauri::State<'_, AppState>,
    window_id: u32,
) -> Result<(), String> {
    // 验证窗口存在
    #[cfg(target_os = "macos")]
    {
        let windows = platform::macos::window_list::list_windows()
            .map_err(|e| e.to_string())?;

        let window = windows.iter().find(|w| w.window_id == window_id)
            .ok_or(format!("窗口未找到：{window_id}"))?;

        if !window.is_on_screen {
            return Err(format!("窗口已最小化，请恢复窗口后重试：{}", window.title));
        }
    }

    // 更新配置
    let mut config = state
        .capture_config
        .lock()
        .map_err(|_| "捕获配置锁已损坏".to_string())?;

    config.mode = core::config::CaptureMode::Window;
    config.window_id = Some(window_id);

    Ok(())
}
```

- [ ] **Step 3: 修改 start_recording 命令支持窗口模式**

```rust
// src-tauri/src/lib.rs

#[tauri::command]
async fn start_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let config = *state
        .capture_config
        .lock()
        .map_err(|_| "捕获配置锁已损坏".to_string())?;

    // 根据模式分发
    match config.mode {
        core::config::CaptureMode::FullScreen => {
            // 现有全屏录制逻辑
        }
        core::config::CaptureMode::Window => {
            // 窗口录制逻辑
            let window_id = config.window_id
                .ok_or("未选择录制窗口".to_string())?;

            // ... 启动窗口录制
        }
        core::config::CaptureMode::Area => {
            return Err("区域录制模式正在开发中".to_string());
        }
    }

    Ok(())
}
```

- [ ] **Step 4: 在 Tauri Builder 中注册新命令**

```rust
// src-tauri/src/lib.rs

tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![
        // 现有命令...
        list_windows,
        set_window_id,
    ])
```

- [ ] **Step 5: 运行编译检查**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(tauri): 添加 list_windows 和 set_window_id 命令"
```

---

## Task 9: 前端类型定义

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 添加 WindowInfo 类型和函数**

```typescript
// src/lib/tauri.ts

export type WindowInfo = {
  windowId: number
  title: string
  appName: string
  bundleId: string | null
  isOnScreen: boolean
  width: number
  height: number
  thumbnail: string | null
}

export type WindowRecordingState = 'recording' | 'minimized' | 'closed'

export async function listWindows(): Promise<WindowInfo[]> {
  return invoke('list_windows')
}

export async function setWindowId(windowId: number): Promise<void> {
  return invoke('set_window_id', { windowId })
}
```

- [ ] **Step 2: 提交**

```bash
git add src/lib/tauri.ts
git commit -m "feat(types): 添加窗口录制前端类型定义"
```

---

## Task 10: WindowSelector 组件

**Files:**
- Create: `src/components/window-selector.tsx`
- Create: `src/components/window-selector.test.tsx`

- [ ] **Step 1: 创建 WindowSelector 组件**

```tsx
// src/components/window-selector.tsx

import { useState, useEffect } from 'react'
import { Monitor, Loader2 } from 'lucide-react'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { cn } from '@/lib/utils'
import { listWindows, type WindowInfo } from '@/lib/tauri'

interface WindowSelectorProps {
  isOpen: boolean
  onSelect: (window: WindowInfo) => void
  onClose: () => void
}

export function WindowSelector({ isOpen, onSelect, onClose }: WindowSelectorProps) {
  const [windows, setWindows] = useState<WindowInfo[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (isOpen) {
      setLoading(true)
      listWindows()
        .then(setWindows)
        .catch(() => setWindows([]))
        .finally(() => setLoading(false))
    }
  }, [isOpen])

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>选择录制窗口</DialogTitle>
        </DialogHeader>

        {loading ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
          </div>
        ) : windows.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
            <Monitor className="w-12 h-12 mb-4" />
            <p>未找到可用窗口</p>
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-3 max-h-[400px] overflow-y-auto">
            {windows.map((window) => (
              <WindowCard
                key={window.windowId}
                window={window}
                onSelect={() => onSelect(window)}
              />
            ))}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

function WindowCard({
  window,
  onSelect,
}: {
  window: WindowInfo
  onSelect: () => void
}) {
  return (
    <button
      onClick={onSelect}
      disabled={!window.isOnScreen}
      className={cn(
        'flex flex-col gap-2 p-3 rounded-xl border transition-all duration-200',
        'hover:bg-surface-hover hover:border-border',
        'disabled:opacity-50 disabled:cursor-not-allowed'
      )}
    >
      {/* 缩略图 */}
      {window.thumbnail ? (
        <img
          src={`data:image/png;base64,${window.thumbnail}`}
          className="w-full h-24 object-cover rounded-lg"
          alt={window.title}
        />
      ) : (
        <div className="w-full h-24 bg-secondary rounded-lg flex items-center justify-center">
          <Monitor className="w-8 h-8 text-muted-foreground" />
        </div>
      )}

      {/* 应用信息 */}
      <div className="flex items-center gap-2 text-left">
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium truncate">{window.title}</p>
          <p className="text-xs text-muted-foreground truncate">{window.appName}</p>
        </div>
      </div>

      {/* 状态指示 */}
      {!window.isOnScreen && (
        <span className="text-xs text-amber-500">窗口已最小化</span>
      )}
    </button>
  )
}
```

- [ ] **Step 2: 创建 WindowSelector 测试**

```tsx
// src/components/window-selector.test.tsx

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { WindowSelector } from './window-selector'
import { listWindows, type WindowInfo } from '@/lib/tauri'

vi.mock('@/lib/tauri', () => ({
  listWindows: vi.fn(),
}))

const mockWindows: WindowInfo[] = [
  {
    windowId: 1,
    title: 'Safari',
    appName: 'Safari',
    bundleId: 'com.apple.Safari',
    isOnScreen: true,
    width: 1920,
    height: 1080,
    thumbnail: null,
  },
  {
    windowId: 2,
    title: 'Terminal',
    appName: 'Terminal',
    bundleId: 'com.apple.Terminal',
    isOnScreen: false,
    width: 800,
    height: 600,
    thumbnail: null,
  },
]

describe('WindowSelector', () => {
  beforeEach(() => {
    vi.mocked(listWindows).mockResolvedValue(mockWindows)
  })

  it('renders window list when open', async () => {
    render(<WindowSelector isOpen={true} onSelect={vi.fn()} onClose={vi.fn()} />)

    await waitFor(() => {
      expect(screen.getByText('Safari')).toBeInTheDocument()
      expect(screen.getByText('Terminal')).toBeInTheDocument()
    })
  })

  it('disables minimized windows', async () => {
    render(<WindowSelector isOpen={true} onSelect={vi.fn()} onClose={vi.fn()} />)

    await waitFor(() => {
      const terminalCard = screen.getByText('Terminal').closest('button')
      expect(terminalCard).toBeDisabled()
    })
  })

  it('calls onSelect when window is clicked', async () => {
    const onSelect = vi.fn()
    render(<WindowSelector isOpen={true} onSelect={onSelect} onClose={vi.fn()} />)

    await waitFor(() => {
      fireEvent.click(screen.getByText('Safari'))
    })

    expect(onSelect).toHaveBeenCalledWith(mockWindows[0])
  })

  it('shows loading state', () => {
    vi.mocked(listWindows).mockReturnValue(new Promise(() => {})) // 永不 resolve

    render(<WindowSelector isOpen={true} onSelect={vi.fn()} onClose={vi.fn()} />)

    expect(screen.getByRole('status')).toBeInTheDocument()
  })
})
```

- [ ] **Step 3: 运行测试**

```bash
npm test -- src/components/window-selector.test.tsx
```

- [ ] **Step 4: 提交**

```bash
git add src/components/window-selector.tsx src/components/window-selector.test.tsx
git commit -m "feat(ui): 添加窗口选择器组件"
```

---

## Task 11: RecordingPanel 集成窗口选择

**Files:**
- Modify: `src/components/recording-panel.tsx:50-136`
- Modify: `src/components/recording-panel.test.tsx`

- [ ] **Step 1: 修改 RecordingPanel 集成窗口选择器**

```tsx
// src/components/recording-panel.tsx

import { useState } from 'react'
import { WindowSelector } from './window-selector'
import { setWindowId, type WindowInfo } from '@/lib/tauri'

export function RecordingPanel({
  recordingMode,
  setRecordingMode,
  // ... 其他 props
}: RecordingPanelProps) {
  const [selectedWindow, setSelectedWindow] = useState<WindowInfo | null>(null)
  const [showWindowSelector, setShowWindowSelector] = useState(false)

  const handleModeChange = (mode: 'fullscreen' | 'window' | 'area') => {
    setRecordingMode(mode)

    if (mode === 'window') {
      setShowWindowSelector(true)
    }
  }

  const handleWindowSelect = async (window: WindowInfo) => {
    setSelectedWindow(window)
    setShowWindowSelector(false)

    try {
      await setWindowId(window.windowId)
    } catch (error) {
      console.error('设置窗口失败:', error)
    }
  }

  return (
    <div>
      {/* 模式选择 */}
      <div className="flex gap-2">
        {modes.map((mode) => (
          <motion.button
            key={mode.id}
            onClick={() => handleModeChange(mode.id)}
            disabled={mode.id === 'area'}
            className={cn(
              'flex-1 flex flex-col items-center gap-1.5 py-3 px-2 rounded-xl transition-all duration-200',
              recordingMode === mode.id
                ? 'bg-surface border border-border text-foreground'
                : 'bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground',
            )}
          >
            <mode.icon className="w-5 h-5" />
            <span className="text-xs font-medium">{mode.label}</span>
          </motion.button>
        ))}
      </div>

      {/* 已选窗口预览 */}
      {recordingMode === 'window' && selectedWindow && (
        <div className="mt-3 p-3 rounded-xl bg-surface border">
          <div className="flex items-center gap-2">
            {selectedWindow.thumbnail ? (
              <img
                src={`data:image/png;base64,${selectedWindow.thumbnail}`}
                className="w-16 h-12 object-cover rounded"
                alt={selectedWindow.title}
              />
            ) : (
              <div className="w-16 h-12 bg-secondary rounded flex items-center justify-center">
                <Monitor className="w-6 h-6 text-muted-foreground" />
              </div>
            )}
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium truncate">{selectedWindow.title}</p>
              <p className="text-xs text-muted-foreground">{selectedWindow.appName}</p>
            </div>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setShowWindowSelector(true)}
            >
              更换
            </Button>
          </div>
        </div>
      )}

      {/* 窗口选择器 */}
      <WindowSelector
        isOpen={showWindowSelector}
        onSelect={handleWindowSelect}
        onClose={() => setShowWindowSelector(false)}
      />

      {/* 窗口模式下隐藏分辨率选择 */}
      {recordingMode !== 'window' && (
        <div className="mb-4">
          {/* 现有的分辨率和 FPS 选择器 */}
        </div>
      )}
    </div>
  )
}
```

- [ ] **Step 2: 添加窗口模式测试**

```tsx
// src/components/recording-panel.test.tsx

describe('RecordingPanel - Window Mode', () => {
  it('opens window selector when switching to window mode', async () => {
    render(<RecordingPanel recordingMode="fullscreen" setRecordingMode={vi.fn()} ... />)

    fireEvent.click(screen.getByText('窗口'))

    await waitFor(() => {
      expect(screen.getByText('选择录制窗口')).toBeInTheDocument()
    })
  })

  it('hides resolution selector in window mode', () => {
    render(<RecordingPanel recordingMode="window" setRecordingMode={vi.fn()} ... />)

    expect(screen.queryByText('画面参数')).not.toBeInTheDocument()
  })
})
```

- [ ] **Step 3: 运行测试**

```bash
npm test -- src/components/recording-panel.test.tsx
```

- [ ] **Step 4: 提交**

```bash
git add src/components/recording-panel.tsx src/components/recording-panel.test.tsx
git commit -m "feat(ui): RecordingPanel 集成窗口选择功能"
```

---

## Task 12: 窗口状态事件与提示

**Files:**
- Modify: `src/components/recording-status-bar.tsx` 或 `src/App.tsx`

- [ ] **Step 1: 添加窗口状态事件监听**

```tsx
// src/App.tsx 或录制状态栏组件

import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'
import { toast } from 'sonner'

useEffect(() => {
  const unlisten = listen('window-state-changed', (event) => {
    const { state, windowTitle } = event.payload as {
      state: 'minimized' | 'closed'
      windowTitle: string
    }

    switch (state) {
      case 'minimized':
        toast.warning('录制暂停', {
          description: `窗口"${windowTitle}"已最小化，恢复窗口后继续录制`,
        })
        break
      case 'closed':
        toast.error('录制停止', {
          description: `窗口"${windowTitle}"已关闭`,
        })
        break
    }
  })

  return () => {
    unlisten.then((fn) => fn())
  }
}, [])
```

- [ ] **Step 2: 添加窗口状态指示 UI**

```tsx
// 录制状态栏中显示窗口状态

{recordingMode === 'window' && windowState === 'minimized' && (
  <div className="flex items-center gap-2 text-amber-500">
    <Minimize2 className="w-4 h-4" />
    <span className="text-sm">窗口已最小化，录制暂停</span>
  </div>
)}

{recordingMode === 'window' && windowState === 'closed' && (
  <div className="flex items-center gap-2 text-red-500">
    <X className="w-4 h-4" />
    <span className="text-sm">窗口已关闭，录制停止</span>
  </div>
)}
```

- [ ] **Step 3: 提交**

```bash
git add src/App.tsx
git commit -m "feat(ui): 添加窗口状态事件监听和提示"
```

---

## Task 13: 集成验证

- [ ] **Step 1: 运行完整测试套件**

```bash
npm test -- --run
cargo test --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 2: 运行构建检查**

```bash
npm run build
cargo build --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 3: 手动验证清单**

| 场景 | 预期结果 |
|------|---------|
| 切换到窗口模式 | 自动弹出窗口选择器 |
| 选择窗口 | 显示窗口预览，隐藏分辨率选择 |
| 点击"更换" | 重新打开窗口选择器 |
| 启动录制 | 窗口模式录制成功 |
| 最小化窗口 | 显示"录制暂停"提示 |
| 恢复窗口 | 录制继续 |
| 关闭窗口 | 录制自动停止 |

- [ ] **Step 4: 最终提交**

```bash
git add -A
git commit -m "feat: 窗口录制功能完成

- 新增 WindowCapture 平台抽象 Trait
- macOS 完整实现窗口枚举、捕获、状态监控
- Windows 占位实现预留
- 前端窗口选择器组件
- 混合事件驱动的窗口状态监控"
```

---

## 里程碑对齐

本计划对应 PRD 里程碑 **W3-4** 中的窗口录制部分。

预计实现周期：3-5 个工作日（macOS 平台）。
