# Windows MVP 功能开发设计文档

> 创建日期：2026-06-19
> 状态：设计完成，待实现计划
> 范围：Windows 平台追平当前 macOS MVP 主链路

---

## 背景

LuZhi 当前已经打通 macOS MVP 主链路：录制、系统音频、麦克风、窗口录制、光标美化、空白裁剪、历史库和三种导出预设。Windows 侧目前仍停在环境准备和占位模块阶段：

- `src-tauri/src/lib.rs` 对非 macOS 构建有 `compile_error!`。
- `AppState` 直接持有 `MacRecordingService`。
- `src-tauri/src/platform/windows/` 下的 `dxgi_capture.rs`、`wasapi_loopback.rs`、`window_capture.rs` 仍返回 `NativeCaptureUnavailable`。
- 前端可以独立运行，但完整 Windows Tauri 原生应用不能作为 MVP 验收对象。

本设计采用“先平台接线，再逐个替换 Windows 占位实现”的路线。目标是让 Windows 端追平当前 macOS MVP 主链路，而不是新增产品路线图外功能。

## 目标

Windows MVP 的硬验收目标：

- Windows Tauri 原生应用能构建并启动。
- 全屏录制可用，目标为 1080p/30fps 稳定录制。
- 窗口录制可用，采用 Windows Graphics Capture 的窗口级捕获，不把桌面裁剪作为正式方案。
- 系统音频通过 WASAPI loopback 捕获。
- 麦克风复用现有 `cpal` 链路。
- 双音频混音复用现有 `AudioSynchronizer`、`SimpleAudioMixer` 和 source-aware diagnostics。
- 光标坐标和点击元数据能被采集，并进入现有 cursor effect/export overlay 管线。
- 录后预览、历史库、空白裁剪和三种导出预设复用现有 App/Media 层。

## 非目标

首个 Windows MVP 不做：

- 区域录制。
- 激活码服务端协议和 Windows Credential Manager 持久化。
- 4K/60fps 稳定性承诺。
- D3D11 texture 到 FFmpeg 的零拷贝优化。
- DXGI Desktop Duplication 作为窗口录制主方案。
- 背景美化、自动缩放、字幕、模板、平台发布 API。

---

## 方案选择

### 采用方案：平台接线优先

先抽出平台无关的录制服务边界，让 Windows app 能编译启动，并以结构化中文错误暴露未实现能力；随后依次替换视频捕获、系统音频、窗口枚举、光标元数据等占位模块。

选择原因：

- 每一步都有可验证状态。
- 不会在产品主链路外沉淀一次性 Spike 代码。
- 能最大化复用现有 writer、音频同步、导出、历史库和 UI。
- macOS 现有能力可以通过同一组回归测试守住。

### 放弃方案

不采用“先独立写 DXGI/WASAPI 样例再迁移”，因为样例容易脱离 Tauri/App/Media 真实链路。

不采用“复制 `MacRecordingService` 为完整 Windows 版本再改”，因为公共 consumer、writer、diagnostics 和收尾逻辑会产生分叉，后续 bug 修复成本高。

---

## 总体架构

```text
React UI
  - RecordingPanel / WindowSelector / PreviewView
  - 只传递模式、窗口 ID、音频开关、导出请求等轻量数据
  ↓ Tauri invoke/event

Rust App Logic
  - 平台无关 AppState
  - RecordingService trait/object boundary
  - RecordingLibrary / ExportService / LicenseService / PermissionService
  ↓

Platform Recording Service
  - macOS: MacRecordingService
  - Windows: WindowsRecordingService
  ↓

Native / Media Modules
  - WindowsGraphicsCapture: display/window video capture
  - WasapiLoopback: system audio
  - cpal microphone
  - existing media writer, mixer, cursor, trim, export
```

核心变化：

- `AppState` 不再直接写死 `MacRecordingService`。
- 新增平台服务工厂，按 `cfg(target_os)` 初始化 macOS 或 Windows 服务。
- 移除非 macOS `compile_error!`，Windows 未完成能力改为运行时结构化错误。
- `MacRecordingService` 现有行为保持不变。
- `WindowsRecordingService` 先对齐现有服务方法，再逐步接入 Windows native 模块。

---

## 模块设计

### 1. 平台服务接线

新增平台无关录制服务接口，覆盖当前 `lib.rs` 对服务的实际调用面：

- `state()`
- `start(config, audio_config, beautify_snapshot, cursor_dispatcher)`
- `start_window(window_id, show_system_cursor, audio_config, beautify_snapshot, cursor_dispatcher)`
- `stop()`
- `pause()`
- `resume()`
- `mic_level_ref()`
- `take_window_state_receiver()`
- `current_session_id()`
- `last_cursor_metadata_path()`
- `last_effect_timeline_path()`
- `last_trim_metadata_path()`
- `last_cut_timeline_path()`
- `last_recording_output_path()`
- `set_last_effect_timeline_path()`
- `set_last_cut_timeline_path()`
- source request flags used by export/audio contracts

`WindowsRecordingService` 初期可以在底层捕获尚未完成时返回 `NativeCaptureUnavailable`，但必须：

- 不假装录制成功。
- 不注册历史记录。
- 不遗留 tick runtime 或 mic runtime。
- 状态机回到可解释状态。

### 2. Windows 视频捕获

Windows 视频主方案使用 Windows Graphics Capture，而不是 DXGI Desktop Duplication。

职责划分：

| 文件 | 职责 |
|------|------|
| `src-tauri/src/platform/windows/graphics_capture.rs` | Windows Graphics Capture display/window 捕获主实现 |
| `src-tauri/src/platform/windows/window_capture.rs` | 窗口枚举、窗口状态、HWND/WindowId 辅助 |
| `src-tauri/src/platform/windows/dxgi_capture.rs` | 保留为后续 fallback 或性能 Spike，不作为 MVP 主线 |

全屏录制：

- 使用 Windows Graphics Capture 创建 display capture item。
- 输出 `VideoFrameRef`。
- MVP 目标为 1080p/30fps。

窗口录制：

- 使用 Windows Graphics Capture 创建 window capture item。
- 前端复用现有 `WindowSelector`。
- Rust Windows 层根据选中窗口标识定位目标窗口并创建 capture item。
- 不使用“桌面帧 + 窗口矩形裁剪”作为正式窗口录制方案。

捕获帧规则：

- 帧池格式使用 BGRA，对应现有 `PixelFormat::Bgra8`。
- 帧时间转换为现有 `MediaTimestamp` 语义。
- 回调只做 copy/convert/enqueue。
- 编码、导出、裁剪和 UI 事件不在捕获回调里执行。
- 窗口尺寸变化时重建 frame pool，避免旧尺寸帧污染 writer。
- 不保存已归还给 frame pool 的底层 D3D surface 引用。

依据：

- Microsoft Learn: [Screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)
- Microsoft Learn: [GraphicsCaptureItem](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscaptureitem?view=winrt-26100)
- Microsoft Learn: [Direct3D11CaptureFramePool](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.direct3d11captureframepool?view=winrt-26100)

### 3. Windows 系统音频

`wasapi_loopback.rs` 实现默认输出设备 loopback。

要求：

- 输出现有 `AudioChunk`。
- 保留真实 `sample_rate`、`channels`、`timestamp`。
- 使用同一 session clock 语义对齐视频和麦克风。
- WASAPI 不可用时返回中文错误。
- 用户关闭系统音频后，允许仅麦克风录制继续。

### 4. 麦克风与混音

麦克风优先复用现有 `cpal` 能力，不新增 Windows 专用麦克风管线。

Windows 服务必须复用：

- `AudioSynchronizer`
- `SimpleAudioMixer`
- `DenoiseMode`
- source-aware audio diagnostics
- 现有音频 artifact contract

请求了系统音频或麦克风但 writer 未收到对应源时，仍必须 hard fail。

### 5. 窗口枚举与状态

Windows 窗口列表至少包含：

- `window_id`
- `title`
- `app_name`
- `bundle_id` 等价字段可为空
- `is_on_screen`
- `width`
- `height`
- `thumbnail` 可为空

窗口状态处理：

- 目标窗口关闭：触发自动停止，复用现有 finalize/register 路径。
- 目标窗口最小化：不输出假成功视频；按实现能力返回暂停或失败事件，并显示中文提示。
- 受保护内容或不可捕获窗口：返回中文错误。
- WGC 捕获边框或系统提示属于 Windows 平台行为，MVP 接受。

### 6. 光标元数据

新增 Windows cursor source：

- 采集鼠标位置。
- 采集点击 down/up。
- 使用同一 session clock 生成时间戳。
- 输出现有 `CursorSample` 和 `CursorClick`。
- 首版 cursor kind 可以退化为 Arrow，但坐标和点击必须准确。

录后继续复用：

- `CursorEffectEngine`
- effect timeline JSON
- FFmpeg cursor overlay export

---

## 数据流

### 视频

```text
RecordingPanel mode/window id
  ↓
WindowsRecordingService
  ↓
WindowsGraphicsCapture
  - display item for fullscreen
  - window item for window recording
  ↓
Direct3D11CaptureFramePool
  ↓
VideoFrameRef { timestamp, width, height, Bgra8 }
  ↓
RecordingWriter
  ↓
original recording artifact + metadata
  ↓
cursor effects / trim / export
```

### 音频

```text
WasapiLoopback ───────┐
                      ↓
cpal microphone ─→ AudioSynchronizer + SimpleAudioMixer
                      ↓
                MixedAudioChunk
                      ↓
                RecordingWriter
```

### 状态事件

```text
Windows native event
  ↓
WindowsRecordingService
  ↓
Tauri event
  - recording-state-changed
  - window-state-changed
  - mic-level
  - export-progress
  ↓
React UI
```

---

## 错误处理

| 场景 | 处理 |
|------|------|
| Windows Graphics Capture 不支持 | 返回“当前设备不支持 Windows 屏幕捕获” |
| display/window capture item 创建失败 | 返回“无法创建捕获目标，请重新选择窗口或显示器” |
| 窗口关闭 | 自动停止录制并 finalize |
| 窗口最小化 | 暂停或失败，但不得输出假成功视频 |
| 受保护内容不可捕获 | 返回中文错误 |
| WASAPI 不可用 | 提示关闭系统音频后可继续仅麦克风录制 |
| 麦克风请求但不可用 | 录制启动失败并说明原因 |
| 捕获回调入队失败过多 | 记录 diagnostics，按现有 drop contract 失败 |
| writer finalize 失败 | 复用现有失败路径，不注册假历史记录 |

所有错误必须 fail visibly：不静默降级、不绕过失败、不把占位实现报告为成功。

---

## 测试策略

### 自动化单元测试

覆盖不依赖真实 Windows API 的逻辑：

- 平台服务工厂按平台选择正确服务。
- 移除 `compile_error!` 后 Windows 构建路径可进入服务初始化。
- `WindowsRecordingService` 在底层返回错误时不会留下 runtime 或假结果。
- WGC frame timestamp 到 `MediaTimestamp` 的转换逻辑。
- frame pool size change 触发 recreate 标记。
- WASAPI chunk 到 `AudioChunk` 的转换保留 sample rate、channels、timestamp。
- source-aware audio contract 继续拒绝请求源缺失。
- 窗口关闭事件映射到自动停止路径。

### 跨平台回归

每个 Windows 接线任务完成后运行：

```powershell
npm test -- --run
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo check --manifest-path src-tauri/Cargo.toml
```

具备 FFmpeg 开发库后追加：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

macOS 不回退要求：

- `MacRecordingService` 行为保持不变。
- 现有窗口录制、导出、美化相关测试仍通过。
- 不为 Windows 编译删除 macOS-only 能力或降低 macOS 验收口径。

### Windows 人工验收

首个 Windows MVP 完成时验证：

- Windows Tauri app 能启动。
- 全屏录制 1080p/30fps，停止后进入预览。
- 全屏录制带系统音频。
- 全屏录制带麦克风。
- 全屏录制同时带系统音频和麦克风，回放无明显音画漂移。
- 窗口列表能枚举常见应用窗口。
- 选择窗口后可录制目标窗口。
- 录制中关闭目标窗口，会自动停止并 finalize。
- 录制中最小化目标窗口，不假成功，并按设计给出中文提示。
- 光标位置和点击效果在导出视频中基本对齐。
- Bilibili / YouTube 16:9 导出可播放。
- Douyin 9:16 导出可播放。
- Xiaohongshu 1:1 导出可播放。
- WASAPI 不可用时，关闭系统音频后可只录麦克风。
- 录制失败时 UI 显示中文错误，不生成假历史记录。

---

## 文件规划

### 新增或重点修改

| 文件 | 责任 |
|------|------|
| `src-tauri/src/app/recording_service.rs` | 平台无关录制服务接口和服务工厂 |
| `src-tauri/src/platform/windows_service.rs` | WindowsRecordingService |
| `src-tauri/src/platform/windows/graphics_capture.rs` | Windows Graphics Capture 视频捕获 |
| `src-tauri/src/platform/windows/wasapi_loopback.rs` | WASAPI loopback 系统音频 |
| `src-tauri/src/platform/windows/window_capture.rs` | Windows 窗口枚举和状态 |
| `src-tauri/src/platform/windows/cursor_source.rs` | Windows 光标位置和点击元数据 |
| `src-tauri/src/lib.rs` | AppState 服务类型和平台接线 |
| `docs/platform-diff/windows-development-environment.md` | 按实现结果补充 WGC/WASAPI 验证命令 |
| `tests/2026-06-19-windows-mvp-checklist.md` | Windows MVP 人工验收清单 |

### 保持复用

| 模块 | 复用内容 |
|------|----------|
| `src/components/window-selector.tsx` | 窗口选择 UI |
| `src/components/recording-panel.tsx` | 录制模式和参数 UI |
| `src-tauri/src/media/audio_synchronizer.rs` | 双音频时间线同步 |
| `src-tauri/src/media/audio_mixer.rs` | 混音和降噪 |
| `src-tauri/src/media/recording_writer.rs` | writer contract 和 diagnostics |
| `src-tauri/src/media/ffmpeg_writer.rs` | 原始录制 artifact |
| `src-tauri/src/media/trim_exporter.rs` | 三种导出预设和裁剪导出 |
| `src-tauri/src/media/cursor_engine.rs` | 光标 effect timeline |
| `src-tauri/src/media/cursor_overlay.rs` | 导出光标 overlay |

---

## 验收边界

完成定义：

- Windows app 能构建和启动。
- Windows 全屏、窗口、系统音频、麦克风、光标元数据、预览、导出主链路可用。
- 未支持或不可捕获场景有中文错误。
- macOS 现有主链路不回退。

明确不验收：

- 4K/60fps 稳定。
- 最小化窗口内容捕获。
- DRM/受保护内容捕获。
- 无 Windows 系统捕获提示或边框。
- 区域录制。
- 激活码服务端协议。

---

## 实施顺序建议

1. 平台服务接口和 `AppState` 接线，移除 Windows `compile_error!`。
2. `WindowsRecordingService` 占位服务，确保 Windows app 可启动且失败可解释。
3. Windows Graphics Capture 全屏捕获。
4. WASAPI loopback 系统音频。
5. cpal 麦克风接入 Windows service。
6. 双音频混音和 writer contract 验证。
7. Windows 窗口枚举和 WGC 窗口捕获。
8. Windows cursor source。
9. 导出、历史库、人工验收清单和文档更新。

正式实施计划应把每一步拆成 TDD 任务，并明确每个任务的测试命令、预期失败和提交点。
