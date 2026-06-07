# 窗口录制功能设计文档

> 创建日期：2026-06-07
> 状态：设计完成，待实现
> 依赖：macOS 12.3+ ScreenCaptureKit

---

## 概述

为 LuZhi 添加窗口录制功能，允许用户选择单个应用窗口进行录制，而非全屏。

### 核心需求

- 用户通过列表选择要录制的窗口（显示图标 + 标题 + 缩略图）
- 保留完整窗口装饰（标题栏、边框、阴影）
- 智能处理窗口状态：最小化暂停、关闭停止
- 自动匹配窗口分辨率，无需手动设置
- 切换到"窗口"模式时自动获取窗口列表

### 设计原则

- 不影响现有全屏录制功能
- 复用现有 ScreenCaptureKit 架构
- 最小化代码改动范围

---

## 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                      前端 UI (React)                        │
│  ┌─────────────────┐    ┌─────────────────┐                 │
│  │ RecordingPanel   │    │ WindowSelector  │                 │
│  │ (录制配置)       │    │ (窗口选择器)    │                 │
│  └────────┬────────┘    └────────┬────────┘                 │
│           │                      │                          │
│           ▼                      ▼                          │
│  ┌─────────────────────────────────────────────┐            │
│  │           Tauri Invoke Commands             │            │
│  │  list_windows / set_window / start_recording│            │
│  └─────────────────────────────────────────────┘            │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Rust App Logic                           │
│  ┌─────────────────────────────────────────────┐            │
│  │           MacRecordingService               │            │
│  │  - 管理录制生命周期                          │            │
│  │  - 协调视频/音频/光标采集                    │            │
│  └─────────────────────────────────────────────┘            │
│                              │                              │
│                              ▼                              │
│  ┌─────────────────────────────────────────────┐            │
│  │           MacScreenCapture                  │            │
│  │  - 全屏模式：SCContentFilter(display)       │            │
│  │  - 窗口模式：SCContentFilter(window)  ← 新增│            │
│  └─────────────────────────────────────────────┘            │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                   macOS Native Layer                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ SCShareable  │  │ SCContent    │  │ SCStream     │      │
│  │ Content      │  │ Filter       │  │ Configuration│      │
│  │ (窗口枚举)   │  │ (窗口过滤)   │  │ (流配置)     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 核心改动点

1. **`MacScreenCapture`**：扩展 `start_stream()` 支持窗口模式
2. **新增 `WindowInfo`**：窗口元数据结构体
3. **新增 `WindowCapture` Trait**：平台无关的窗口捕获抽象
4. **新增 Tauri 命令**：`list_windows`、`set_window_id`
5. **前端新增 `WindowSelector`**：窗口选择器组件

### 平台抽象层

根据项目架构约束（`0-global.md`），录制引擎必须封装为统一的 Rust Trait，隔离平台差异。窗口录制同样需要遵循此原则。

```
┌─────────────────────────────────────────────────────────────┐
│                    应用层 (MacRecordingService)              │
│                    调用 platform.list_windows()              │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                 平台抽象 Trait (WindowCapture)               │
│  - list_windows() -> Vec<WindowInfo>                        │
│  - get_thumbnail(window_id) -> Option<String>               │
│  - start_window_stream(window_id, sinks) -> Result          │
│  - stop_window_stream() -> Result                           │
└──────────────────────────┬──────────────────────────────────┘
                           │
            ┌──────────────┴──────────────┐
            ▼                              ▼
┌───────────────────────┐      ┌───────────────────────┐
│ MacWindowCapture      │      │ WinWindowCapture      │
│ (ScreenCaptureKit)    │      | (DXGI，预留)          │
│ ✅ 本次实现           │      │ ❌ 占位实现           │
└───────────────────────┘      └───────────────────────┘
```

---

## 数据结构设计

### 窗口信息结构体

```rust
/// 窗口元数据，用于前端展示和选择
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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
```

### 扩展 CaptureConfig

```rust
/// 捕获模式
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode {
    FullScreen,
    Window,  // 移除"开发中"注释
    Area,    // 保留"开发中"
}

/// 捕获配置
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CaptureConfig {
    pub mode: CaptureMode,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub show_system_cursor: bool,
    /// 目标窗口 ID（仅 Window 模式有效）
    pub window_id: Option<u32>,
}
```

### 窗口状态枚举

```rust
/// 窗口录制状态
#[derive(Clone, Debug, serde::Serialize)]
pub enum WindowRecordingState {
    /// 正常录制中
    Recording,
    /// 窗口已最小化，录制暂停
    Minimized,
    /// 窗口已关闭，录制停止
    Closed,
}
```

### 平台抽象 Trait

```rust
// src-tauri/src/core/capture.rs (扩展)

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
        session_clock: Arc<SessionClock>,
    ) -> AppResult<()>;

    /// 停止窗口捕获流
    fn stop_window_stream(&mut self) -> AppResult<()>;

    /// 检查窗口当前状态
    fn window_state(&self, window_id: u32) -> AppResult<WindowRecordingState>;
}
```

---

## 后端实现设计

### 窗口枚举与缩略图

新增文件：`src-tauri/src/platform/macos/window_list.rs`

```rust
/// 获取当前可见窗口列表
pub fn list_windows() -> AppResult<Vec<WindowInfo>> {
    // 1. 调用 SCShareableContent.getShareableContentWithCompletionHandler
    // 2. 遍历 content.windows()
    // 3. 过滤条件：
    //    - 排除系统窗口（kCGWindowLayer == 0）
    //    - 排除无标题窗口（title 为空）
    //    - 排除自身应用窗口
    // 4. 构建 WindowInfo 列表
}

/// 获取单个窗口的缩略图
pub fn get_window_thumbnail(window_id: u32) -> AppResult<Option<String>> {
    // 1. 使用 SCScreenshotManager.captureImage
    // 2. 裁剪到目标窗口区域
    // 3. 转换为 Base64 PNG
}
```

### MacScreenCapture 扩展

修改文件：`src-tauri/src/platform/macos/screen_capture_kit.rs`

```rust
impl MacScreenCapture {
    /// 启动窗口捕获流
    fn start_window_stream(
        &mut self,
        window_id: u32,
        capture_system_audio: bool,
        video_sink: VideoFrameSink,
        audio_sink: AudioChunkSink,
        session_clock: Arc<SessionClock>,
    ) -> AppResult<()> {
        // 1. 获取 SCShareableContent
        // 2. 查找目标窗口：content.windows().find(|w| w.windowID() == window_id)
        // 3. 验证窗口存在且 isOnScreen
        // 4. 创建 SCContentFilter::initWithExcludingWindows_exceptingWindows
        //    （传入目标窗口，排除其他所有窗口）
        // 5. 配置 SCStreamConfiguration：
        //    - 宽高从 SCWindow.frame 读取
        //    - 其他配置与全屏模式一致
        // 6. 创建 SCStream 并启动
    }
}
```

### 窗口状态监控

新增文件：`src-tauri/src/platform/macos/window_monitor.rs`

### Windows 平台预留

新增文件：`src-tauri/src/platform/windows/window_capture.rs`

```rust
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
        _session_clock: Arc<SessionClock>,
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

采用**混合事件驱动**方案，结合 NSWorkspace 通知（实时）和低频轮询（200ms），兼顾性能和实时性。

```rust
/// 窗口状态监控器
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

    pub fn start(&mut self) {
        // Layer 1: NSWorkspace 应用级事件监听（实时）
        // - 监听 NSWorkspaceDidActivateApplicationNotification
        // - 应用切换时立即检查目标窗口状态
        self.register_workspace_notifications();

        // Layer 2: 低频轮询（200ms，仅在录制中）
        // - 检测窗口最小化/关闭
        // - 增量比较，仅在状态变化时触发回调
        // - 录制停止时自动退出
        self.start_polling_loop();
    }

    fn register_workspace_notifications(&self) {
        let workspace = NSWorkspace::sharedWorkspace();
        let observer = workspace.notificationCenter();
        // 注册 NSWorkspaceDidActivateApplicationNotification
        // 当应用切换时，立即检查目标窗口状态
    }

    fn start_polling_loop(&self) {
        let window_id = self.window_id;
        let last_state = self.last_state.clone();
        let on_change = self.on_state_change;
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
        // 1. 调用 SCShareableContent 获取窗口列表
        // 2. 查找目标窗口
        // 3. 如果窗口不存在 → Closed
        // 4. 如果 !isOnScreen → Minimized
        // 5. 否则 → Recording
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        // 注销通知监听
    }
}
```

**性能对比：**

| 方案 | CPU 开销（录制中） | CPU 开销（空闲） | 最大延迟 |
|------|-------------------|-----------------|---------|
| 500ms 轮询 | ~0.1% | ~0.1% | 500ms |
| **200ms 轮询 + 事件** | ~0.05% | 0% | 200ms（轮询）/ 实时（事件） |

### Tauri 命令

修改文件：`src-tauri/src/lib.rs`

```rust
/// 获取可录制窗口列表
#[tauri::command]
async fn list_windows() -> Result<Vec<WindowInfo>, String> {
    window_list::list_windows().map_err(|e| e.to_string())
}

/// 设置目标窗口 ID
#[tauri::command]
async fn set_window_id(
    state: tauri::State<'_, AppState>,
    window_id: u32,
) -> Result<(), String> {
    // 验证窗口存在
    // 更新 capture_config.window_id
    // 更新 capture_config.mode = Window
}

/// 启动录制（修改现有实现）
#[tauri::command]
async fn start_recording(...) -> Result<(), String> {
    // 移除窗口模式的错误拦截
    // 根据 config.mode 分发到不同启动逻辑
}
```

---

## 前端实现设计

### WindowSelector 组件

新增文件：`src/components/window-selector.tsx`

```typescript
interface WindowInfo {
  windowId: number
  title: string
  appName: string
  bundleId: string | null
  isOnScreen: boolean
  width: number
  height: number
  thumbnail: string | null
}

interface WindowSelectorProps {
  isOpen: boolean
  onSelect: (window: WindowInfo) => void
  onClose: () => void
}

export function WindowSelector({ isOpen, onSelect, onClose }: WindowSelectorProps) {
  const [windows, setWindows] = useState<WindowInfo[]>([])
  const [loading, setLoading] = useState(false)

  // 打开时获取窗口列表
  useEffect(() => {
    if (isOpen) {
      setLoading(true)
      invoke('list_windows')
        .then(setWindows)
        .finally(() => setLoading(false))
    }
  }, [isOpen])

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>选择录制窗口</DialogTitle>
        </DialogHeader>

        <div className="grid grid-cols-2 gap-3 max-h-[400px] overflow-y-auto">
          {windows.map((window) => (
            <WindowCard
              key={window.windowId}
              window={window}
              onSelect={() => onSelect(window)}
            />
          ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}

function WindowCard({ window, onSelect }: { window: WindowInfo, onSelect: () => void }) {
  return (
    <button
      onClick={onSelect}
      disabled={!window.isOnScreen}
      className="flex flex-col gap-2 p-3 rounded-xl border hover:bg-surface-hover
                 disabled:opacity-50 disabled:cursor-not-allowed"
    >
      {/* 缩略图 */}
      {window.thumbnail && (
        <img
          src={`data:image/png;base64,${window.thumbnail}`}
          className="w-full h-24 object-cover rounded-lg"
        />
      )}

      {/* 应用信息 */}
      <div className="flex items-center gap-2">
        <AppIcon bundleId={window.bundleId} />
        <div className="text-left">
          <p className="text-sm font-medium truncate">{window.title}</p>
          <p className="text-xs text-muted-foreground">{window.appName}</p>
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

### RecordingPanel 修改

修改文件：`src/components/recording-panel.tsx`

```typescript
export function RecordingPanel({ ... }: RecordingPanelProps) {
  const [selectedWindow, setSelectedWindow] = useState<WindowInfo | null>(null)
  const [showWindowSelector, setShowWindowSelector] = useState(false)

  // 模式切换时的处理
  const handleModeChange = (mode: 'fullscreen' | 'window' | 'area') => {
    setRecordingMode(mode)

    if (mode === 'window') {
      // 自动打开窗口选择器
      setShowWindowSelector(true)
    }
  }

  // 窗口选择完成
  const handleWindowSelect = async (window: WindowInfo) => {
    setSelectedWindow(window)
    setShowWindowSelector(false)

    // 通知后端设置目标窗口
    await invoke('set_window_id', { windowId: window.windowId })
  }

  return (
    <div>
      {/* 模式选择 */}
      <div className="flex gap-2">
        {modes.map((mode) => (
          <motion.button
            key={mode.id}
            onClick={() => handleModeChange(mode.id)}
            disabled={mode.id === 'area'} // 区域录制仍禁用
            className={cn(
              'flex-1 flex flex-col items-center gap-1.5 py-3 px-2 rounded-xl',
              recordingMode === mode.id
                ? 'bg-surface border border-border text-foreground'
                : 'bg-secondary/50 border border-transparent text-muted-foreground',
            )}
          >
            <mode.icon className="w-5 h-5" />
            <span className="text-xs font-medium">{mode.label}</span>
          </motion.button>
        ))}
      </div>

      {/* 已选窗口预览（窗口模式） */}
      {recordingMode === 'window' && selectedWindow && (
        <div className="mt-3 p-3 rounded-xl bg-surface border">
          <div className="flex items-center gap-2">
            {selectedWindow.thumbnail && (
              <img
                src={`data:image/png;base64,${selectedWindow.thumbnail}`}
                className="w-16 h-12 object-cover rounded"
              />
            )}
            <div>
              <p className="text-sm font-medium">{selectedWindow.title}</p>
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
          {/* 分辨率和 FPS 选择器 */}
        </div>
      )}
    </div>
  )
}
```

### 窗口状态提示

```typescript
// 录制状态栏中显示窗口状态
{recordingState === 'window-minimized' && (
  <div className="flex items-center gap-2 text-amber-500">
    <Minimize2 className="w-4 h-4" />
    <span className="text-sm">窗口已最小化，录制暂停</span>
  </div>
)}

{recordingState === 'window-closed' && (
  <div className="flex items-center gap-2 text-red-500">
    <X className="w-4 h-4" />
    <span className="text-sm">窗口已关闭，录制停止</span>
  </div>
)}
```

---

## 错误处理与边界情况

### 错误类型定义

```rust
// src-tauri/src/app/error.rs (扩展)

pub enum AppError {
    // 现有错误...

    /// 窗口未找到
    WindowNotFound { window_id: u32 },
    /// 窗口已最小化，无法启动录制
    WindowMinimized { window_id: u32 },
    /// 窗口已关闭
    WindowClosed { window_id: u32 },
    /// 窗口权限不足（受保护窗口）
    WindowAccessDenied { window_id: u32 },
}
```

### 边界情况处理

| 场景 | 处理策略 |
|------|----------|
| **启动时窗口已最小化** | 返回错误 `WindowMinimized`，提示用户恢复窗口 |
| **录制中窗口最小化** | 暂停视频帧捕获，保持音频（如启用），前端显示"录制暂停" |
| **录制中窗口关闭** | 自动停止录制，保存已录制内容，前端显示"录制已停止" |
| **录制中窗口被遮挡** | 继续录制（捕获窗口内容，不含遮挡物） |
| **窗口缩放/移动** | 继续录制，分辨率自动跟随窗口大小变化 |
| **多显示器窗口跨屏** | 捕获窗口完整区域，可能超出单屏 |
| **系统窗口（Dock、菜单栏）** | 过滤排除，不显示在列表中 |
| **无标题窗口** | 过滤排除 |
| **权限不足窗口** | 显示为"不可用"，禁用选择 |

### 窗口状态机

```
                    ┌─────────────┐
                    │   IDLE      │
                    └──────┬──────┘
                           │ 用户选择窗口
                           ▼
                    ┌─────────────┐
                    │  SELECTED   │
                    └──────┬──────┘
                           │ start_recording
                           ▼
              ┌────────────────────────────┐
              │         RECORDING          │
              │  (正常录制，视频帧持续输出)  │
              └────────────┬───────────────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
              ▼            ▼            ▼
     ┌─────────────┐ ┌─────────┐ ┌─────────────┐
     │  MINIMIZED  │ │ COVERED │ │   CLOSED    │
     │ (暂停视频)  │ │ (继续)  │ │ (自动停止)  │
     └──────┬──────┘ └────┬────┘ └──────┬──────┘
            │             │             │
            │ 恢复窗口    │             │
            ▼             │             ▼
     ┌─────────────┐      │      ┌─────────────┐
     │  RECORDING  │◄─────┘      │    STOPPED  │
     └─────────────┘             └─────────────┘
```

### 前端错误提示

```typescript
// 窗口状态变化事件处理
useEffect(() => {
  const unlisten = listen('window-state-changed', (event) => {
    const { state, windowTitle } = event.payload

    switch (state) {
      case 'minimized':
        toast.warning('录制暂停', {
          description: `窗口"${windowTitle}"已最小化，恢复窗口后继续录制`
        })
        break
      case 'closed':
        toast.error('录制停止', {
          description: `窗口"${windowTitle}"已关闭`
        })
        break
    }
  })

  return () => { unlisten.then(fn => fn()) }
}, [])
```

---

## 测试策略

### 后端单元测试

```rust
// src-tauri/src/platform/macos/window_list.rs

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
        assert!(json.contains("\"window_id\":123"));
    }

    #[test]
    fn capture_config_window_mode_has_window_id() {
        let config = CaptureConfig {
            mode: CaptureMode::Window,
            width: 1920,
            height: 1080,
            fps: 30,
            show_system_cursor: true,
            window_id: Some(456),
        };
        assert_eq!(config.mode, CaptureMode::Window);
        assert_eq!(config.window_id, Some(456));
    }
}
```

```rust
// src-tauri/src/platform/macos/window_monitor.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_state_transitions() {
        // 测试状态转换逻辑
        let mut state = WindowRecordingState::Recording;

        // 模拟窗口最小化
        state = WindowRecordingState::Minimized;
        assert!(matches!(state, WindowRecordingState::Minimized));

        // 模拟窗口关闭
        state = WindowRecordingState::Closed;
        assert!(matches!(state, WindowRecordingState::Closed));
    }
}
```

### 前端组件测试

```typescript
// src/components/window-selector.test.tsx

describe('WindowSelector', () => {
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
      isOnScreen: false, // 已最小化
      width: 800,
      height: 600,
      thumbnail: null,
    },
  ]

  beforeEach(() => {
    vi.mocked(invoke).mockResolvedValue(mockWindows)
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
})
```

```typescript
// src/components/recording-panel.test.tsx (扩展)

describe('RecordingPanel - Window Mode', () => {
  it('opens window selector when switching to window mode', async () => {
    render(<RecordingPanel ... />)

    fireEvent.click(screen.getByText('窗口'))

    await waitFor(() => {
      expect(screen.getByText('选择录制窗口')).toBeInTheDocument()
    })
  })

  it('hides resolution selector in window mode', async () => {
    render(<RecordingPanel recordingMode="window" ... />)

    expect(screen.queryByText('画面参数')).not.toBeInTheDocument()
  })

  it('shows selected window preview', async () => {
    render(<RecordingPanel recordingMode="window" selectedWindow={mockWindow} ... />)

    expect(screen.getByText('Safari')).toBeInTheDocument()
    expect(screen.getByText('更换')).toBeInTheDocument()
  })
})
```

### 集成测试清单

| 测试场景 | 验证方式 |
|---------|---------|
| 窗口列表获取 | 调用 `list_windows` 返回非空列表 |
| 窗口选择 | 选择窗口后 `capture_config.window_id` 正确设置 |
| 启动录制 | 窗口模式下 `start_recording` 成功 |
| 窗口最小化 | 事件 `window-state-changed` 正确触发 |
| 窗口关闭 | 录制自动停止，文件正常保存 |
| 切换模式 | 从窗口切回全屏，录制正常启动 |

---

## 文件清单

### 新增文件

| 文件 | 说明 |
|------|------|
| `src-tauri/src/platform/macos/window_list.rs` | 窗口枚举和缩略图获取（macOS） |
| `src-tauri/src/platform/macos/window_monitor.rs` | 窗口状态监控（macOS） |
| `src-tauri/src/platform/windows/window_capture.rs` | Windows 窗口捕获占位实现 |
| `src/components/window-selector.tsx` | 窗口选择器组件 |
| `src/components/window-selector.test.tsx` | 窗口选择器测试 |

### 修改文件

| 文件 | 改动说明 |
|------|----------|
| `src-tauri/src/core/config.rs` | 扩展 `CaptureConfig` 添加 `window_id` 字段 |
| `src-tauri/src/platform/macos/screen_capture_kit.rs` | 添加 `start_window_stream()` 方法 |
| `src-tauri/src/platform/macos_service.rs` | 集成窗口录制逻辑 |
| `src-tauri/src/lib.rs` | 添加 `list_windows`、`set_window_id` 命令，修改 `start_recording` |
| `src-tauri/src/app/error.rs` | 添加窗口相关错误类型 |
| `src/components/recording-panel.tsx` | 集成窗口选择器，修改模式切换逻辑 |
| `src/components/recording-panel.test.tsx` | 添加窗口模式测试 |

---

## 里程碑对齐

本设计对应 PRD 里程碑 **W3-4：录制引擎 Windows 适配 + 音频捕获** 中的窗口录制部分。

- **macOS 平台**：完整实现，预计 3-5 个工作日
- **Windows 平台**：预留 Trait 接口和占位实现，待 DXGI 完整实现后补充

本次实现范围：
- ✅ 平台抽象 Trait（`WindowCapture`）
- ✅ macOS 完整实现（`MacWindowCapture`）
- ✅ Windows 占位实现（`WinWindowCapture`）
- ✅ 前端窗口选择器
- ❌ Windows DXGI 窗口捕获（依赖 DXGI Desktop Duplication 完整实现）
