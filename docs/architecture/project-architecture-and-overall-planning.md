# 录智项目系统架构设计与 MVP 开发执行计划

> 版本：v1.0
> 日期：2026-05-23
> 输入依据：`docs/PRD/LuZhi_PRD_final_version.md`、`.codex/rules/`、`reference/tasks/architecture-task.md`

## 0. 关键结论

录智的 MVP 采用“macOS 先打穿的分层流水线架构”：先用 ScreenCaptureKit 跑通录制、缓存、录后美化、裁剪、FFmpeg 导出的端到端闭环，再通过统一 Rust Trait 接入 Windows 的 DXGI 与 WASAPI 实现。MVP 的主验收目标是 1080p 稳定录制、录后光标美化、空白段裁剪和三种比例导出；4K 在架构、配置和降级策略中预留，不作为首版唯一硬验收。

核心原则：

- React 前端只负责展示、交互与轻量状态，不承载录制状态机，不接触视频帧和音频流。
- Rust App Logic 负责业务编排、状态机、权限检测、错误归一化和任务生命周期管理。
- Native Modules 负责 ScreenCaptureKit、DXGI、WASAPI、cpal、FFmpeg、GPU 相关底层能力。
- 捕获线程优先级最高，任何 AI 美化、空白检测、导出任务都不得阻塞捕获主链路。
- 底层跨平台 API 与 FFI 代码必须由人工逐行审查内存安全、线程安全和资源释放路径。

## 1. 架构目标与范围

### 1.1 MVP 必须覆盖

- 录制：全屏、窗口、区域录制，优先保证 1080p 稳定，保留 4K 配置能力与降级路径。
- 音频：系统音频和麦克风采集，Rust 侧按时间戳对齐并混音。
- AI 美化：光标轨迹平滑、点击放大效果、空白段自动裁剪。
- 导出：16:9、9:16、1:1 三种本地导出预设。
- 授权：14 天本地试用状态、激活状态接口边界、系统凭据存储接口。

### 1.2 MVP 明确不做

- 字幕、摘要、知识库、模板系统、团队协作。
- 平台发布 API 或自动投稿。
- 快捷键叠加、背景美化、自动缩放关键区域。
- 完整商业化后端、激活码生成服务、反滥用系统。
- 不把实时 4K AI 美化作为首版硬目标。

## 2. 总体分层架构

```text
React UI
  - 中文界面展示
  - 录制参数输入
  - 录制/暂停/停止/导出命令
  - 状态、进度、错误、缩略图展示
  ↓ Tauri invoke / event

Rust App Logic
  - RecordingStateMachine
  - RecordingService
  - ExportService
  - LicenseService
  - PermissionService
  - 应用级错误与事件模型
  ↓ Rust Trait

Native Modules
  - macOS: ScreenCaptureKit、系统权限、系统音频
  - Windows: DXGI Desktop Duplication、WASAPI loopback、UAC 限制处理
  - cpal 麦克风采集
  - FFmpeg binding / C API 封装
  - GPU 渲染与硬编能力预留
```

数据流红线：视频帧流和音频流严禁经过前端 JS 层。前端只接收状态、时长、进度、错误、缩略图路径等轻量数据。

## 3. 进程与线程模型

### 3.1 进程职责

Tauri 主进程承载 Rust App Logic 和 Native Modules。WebView 渲染进程承载 React UI。WebView 与 Rust 之间通过 Tauri Command 和事件通信，传递结构化命令和轻量状态，不传递媒体帧。

### 3.2 线程模型

```text
Tauri 主事件循环
  - 接收 UI 命令：开始录制、暂停、停止、导出
  - 推送轻量事件：状态、进度、错误、缩略图路径
  - 不做帧处理、不做编码、不做音频混音

录制控制线程
  - 维护 RecordingStateMachine
  - 启停视频捕获、音频捕获、写盘和编码任务
  - 处理取消、异常、资源释放

视频捕获线程
  - macOS MVP：ScreenCaptureKit
  - Windows 接入：DXGI Desktop Duplication
  - 输出带 MediaTimestamp 的 VideoFrameRef

音频捕获线程
  - 系统音频和麦克风独立采集
  - 输出带 MediaTimestamp 的 AudioChunk

处理线程池
  - 光标轨迹平滑参数计算
  - 点击放大状态机
  - 空白段检测所需低频帧差分
  - MVP 以录后处理为主，避免阻塞捕获

编码/封装线程
  - 使用 FFmpeg binding / C API
  - 接收视频帧、混音音频、裁剪时间线
  - 写出中间文件和最终导出文件
```

### 3.3 阻塞控制

- 捕获线程不得等待 UI 响应。
- 捕获线程不得等待光标美化、空白检测、导出处理完成。
- 预览缩略图允许降采样和丢弃旧帧。
- 空白检测使用低频抽样，不占用主帧流。
- 导出和美化属于录后任务，可以慢于实时，但不能影响录制稳定性。

## 4. Rust 模块与 Trait 设计

### 4.1 建议目录结构

```text
src-tauri/src/
  app/
    state_machine.rs
    recording_service.rs
    export_service.rs
    license_service.rs
    permission_service.rs
    events.rs
    error.rs

  core/
    capture.rs
    frame.rs
    processor.rs
    encoder.rs
    config.rs
    timeline.rs

  platform/
    macos/
      screen_capture_kit.rs
      system_audio.rs
      permissions.rs
    windows/
      dxgi_capture.rs
      wasapi_loopback.rs
      permissions.rs

  media/
    ffmpeg_encoder.rs
    audio_mixer.rs
    cursor_engine.rs
    silence_detector.rs
```

### 4.2 核心数据类型草案

```rust
pub struct MediaTimestamp {
    pub nanos: u64,
}

pub struct VideoFrame {
    pub timestamp: MediaTimestamp,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub buffer: FrameBuffer,
}

pub type VideoFrameRef = std::sync::Arc<VideoFrame>;

pub struct AudioChunk {
    pub timestamp: MediaTimestamp,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: std::sync::Arc<[f32]>,
}

pub struct CursorSample {
    pub timestamp: MediaTimestamp,
    pub x: f32,
    pub y: f32,
}

pub struct CursorClick {
    pub timestamp: MediaTimestamp,
    pub button: MouseButton,
    pub phase: ClickPhase,
}
```

`MediaTimestamp` 必须来自同一单调时钟基准。禁止用“第 N 帧对应第 N 段音频”的方式硬对齐。

### 4.3 捕获接口

```rust
pub trait ScreenCapture: Send {
    fn start(&mut self, config: CaptureConfig, sink: VideoFrameSink) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn capabilities(&self) -> CaptureCapabilities;
}

pub trait AudioCapture: Send {
    fn start(&mut self, config: AudioConfig, sink: AudioChunkSink) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn device_list(&self) -> Result<Vec<AudioDevice>>;
    fn capabilities(&self) -> AudioCapabilities;
}
```

约束：

- App Logic 只能依赖 Trait，不直接依赖 ScreenCaptureKit、DXGI 或 WASAPI 类型。
- 平台特有能力通过 `capabilities()` 暴露，例如系统音频支持情况、硬编支持情况、管理员窗口捕获限制。
- 测试环境使用 Mock 实现，不调用真实系统录屏 API。

### 4.4 处理与编码接口

```rust
pub trait CursorProcessor: Send + Sync {
    fn build_timeline(
        &self,
        samples: &[CursorSample],
        clicks: &[CursorClick],
        video_clock: &VideoClock,
    ) -> Result<EffectTimeline>;
}

pub trait SilenceDetector: Send + Sync {
    fn analyze(
        &self,
        audio: &[AudioChunk],
        visual_samples: &[FrameDiffSample],
    ) -> Result<CutTimeline>;
}

pub trait MediaEncoder: Send {
    fn open(&mut self, config: EncodeConfig) -> Result<()>;
    fn push_video(&mut self, frame: VideoFrameRef) -> Result<()>;
    fn push_audio(&mut self, chunk: MixedAudioChunk) -> Result<()>;
    fn finish(&mut self) -> Result<EncodeResult>;
}
```

FFmpeg 只能通过 binding / C API 封装调用，不允许拼接 CLI 命令字符串。

## 5. 核心数据流设计

### 5.1 视频主链路

```text
屏幕像素 / 窗口帧
  ↓ ScreenCaptureKit / DXGI
VideoFrameRef { timestamp, format, size, buffer }
  ↓ 有界 Ring Buffer
Raw Recording Writer / Encoder
  ↓ 中间录制文件 + 元数据
Post Process Pipeline
  ↓ 光标平滑、点击放大、空白段检测、裁剪时间线
FFmpeg Encoder / Muxer
  ↓
最终导出 mp4
```

MVP 阶段优先保证中间录制文件可靠。光标美化和空白裁剪以录后处理为主，降低录制期实时处理风险。

### 5.2 音频主链路

```text
系统音频捕获        麦克风捕获
  ↓ timestamp       ↓ timestamp
AudioChunk          AudioChunk
  ↓                 ↓
Audio Synchronizer + Mixer
  ↓
MixedAudioChunk
  ↓
FFmpeg audio stream
```

系统音频与麦克风可能存在不同采样率、声道数和设备延迟。Rust 侧统一执行：

1. 转换为统一采样率和声道布局。
2. 根据 `MediaTimestamp` 对齐。
3. 对短缺区间填充静音，对重叠区间混音。
4. 输出连续 `MixedAudioChunk` 给 FFmpeg。

### 5.3 缓冲与背压

视频帧和音频块使用有界 Ring Buffer 解决捕获端与消费端速率差。

背压策略：

- 原始录制写盘优先，不轻易丢帧。
- 预览缩略图可丢弃旧帧，始终展示最近状态。
- 空白检测只取低频抽样，不挤占主帧队列。
- 后处理任务录后执行，不影响捕获期稳定性。
- Buffer 水位超过阈值时向 App Logic 上报性能警告，UI 用中文提示用户降低分辨率或帧率。

### 5.4 零拷贝与高效传递策略

MVP 不承诺全链路零拷贝，目标是少拷贝、可度量、可演进。

- 统一帧对象使用 `Arc<VideoFrame>` 传递所有权，避免业务层重复复制。
- macOS 路径优先保留 ScreenCaptureKit 提供的原始像素缓冲引用能力。
- Windows 路径预留 DXGI texture 到编码或 GPU 处理的互操作接口。
- FFmpeg 封装层需要记录每次格式转换和内存复制点，作为性能优化依据。
- GPU 不可用或互操作失败时，降级为 CPU 处理和录后导出，不影响录制可用性。

## 6. 光标平滑与放大引擎

### 6.1 元数据采集

录制期只采集光标元数据，不实时烘焙效果：

```text
CursorSample { x, y, timestamp }
CursorClick { button, down/up, timestamp }
```

这些事件与视频帧使用同一时间基，录后生成 `EffectTimeline`。

### 6.2 移动平均滤波

移动平均窗口按帧率自适应：

- 30fps：5 帧左右窗口。
- 60fps：7 到 9 帧窗口。
- 高频抖动场景：增加权重平滑，但限制最大延迟，避免光标明显滞后。

边界条件：

- 0 帧输入返回空轨迹。
- 单点输入保持原位置。
- 坐标突变超过阈值时保留跳变，避免跨窗口移动被错误拉成曲线。

### 6.3 贝塞尔插值

滤波后的关键点生成分段贝塞尔曲线。插值输出按视频时间戳采样，保证每个输出帧都能找到对应光标位置。

实现要求：

- 控制点由相邻轨迹方向和速度估算。
- 低速短距离移动减少插值强度，避免光标漂移。
- 快速长距离移动保留速度感，避免过度平滑。

### 6.4 点击放大状态机

```text
Idle -> PressedExpand -> Hold -> ReleaseShrink -> Idle
```

状态说明：

- `Idle`：正常光标大小。
- `PressedExpand`：点击瞬间进入放大动画。
- `Hold`：短暂停留，突出点击动作。
- `ReleaseShrink`：缓动恢复。

输出不是直接改帧，而是生成效果时间线：

```text
CursorEffect {
  start,
  end,
  position,
  scale_curve,
  opacity_curve
}
```

## 7. 空白段自动裁剪

### 7.1 双信号确认

空白段检测必须同时结合音频 RMS 和画面帧差分：

```text
音频 RMS 静音
  +
画面帧差分低变化
  +
持续时间超过阈值
  =
候选空白段
```

### 7.2 音频 RMS 策略

- 使用 500ms 到 1000ms 滑动窗口。
- 阈值初始采用保守默认值，后续可根据用户素材统计调整。
- 系统音频和麦克风混音前后都可采样，但最终判断以混音后音轨为主。

### 7.3 帧差分策略

- 不直接处理全分辨率画面。
- 抽样帧先缩小到低分辨率灰度图。
- 计算相邻帧差分均值或块级变化率。
- 屏幕录制中的加载动画、终端输出、鼠标移动需要被识别为“仍有变化”。

### 7.4 时间窗口与误剪保护

- 小于 2 秒的静默不裁剪。
- 超过 5 到 8 秒且画面变化很低才进入候选。
- 候选段前后保留 300 到 500ms 缓冲。
- 默认只生成建议裁剪时间线，保留原始素材。

裁剪时间线示例：

```text
keep: 00:00.000-00:12.400
cut:  00:12.400-00:18.900
keep: 00:18.900-01:04.200
```

## 8. FFmpeg 编码与导出设计

### 8.1 封装原则

- 使用 FFmpeg binding / C API，禁止直接调用 CLI。
- 用户输入不得拼接成命令行参数。
- 编码任务运行在独立线程，进度通过事件上报给 UI。
- 导出失败必须返回结构化错误，UI 用中文展示。

### 8.2 中间文件与最终文件

MVP 建议保留两类产物：

- 原始中间录制文件：用于回退、重新裁剪、重新导出。
- 最终导出文件：按用户选择的比例和编码参数生成。

### 8.3 导出预设

| 预设 | 比例 | 默认目标 | MVP 策略 |
| --- | --- | --- | --- |
| B 站 / YouTube | 16:9 | 1080p | 等比缩放，黑边或裁切策略固定 |
| 抖音 | 9:16 | 1080p | 居中裁切，后续可接入关键区域分析 |
| 小红书 | 1:1 | 1080p | 居中裁切或缩放填充 |

导出预设保持固定，不扩展为模板系统。

## 9. 4K 性能保障与降级策略

MVP 口径：4K 是架构预留能力，不作为首版唯一硬验收。主验收目标是 1080p 稳定录制、录后美化和导出。

### 9.1 macOS 路径

- ScreenCaptureKit 优先用于高性能捕获。
- 预留 Metal 渲染和硬件编码能力。
- GPU 不可用时降级为录后 CPU 处理。

### 9.2 Windows 路径

- DXGI Desktop Duplication 用于屏幕捕获。
- WASAPI loopback 用于系统音频。
- 预留 D3D11 / DX12 texture 互操作与硬编路径。
- 管理员权限窗口捕获受限时，提供中文提示。

### 9.3 降级策略

- 4K 可降级到 30fps。
- 高负载下优先关闭实时预览缩略图。
- 空白检测使用低分辨率抽样。
- 光标美化和裁剪录后执行。
- 导出可以慢于实时，但录制不能卡顿。

## 10. 权限、安全与授权边界

### 10.1 系统权限

- macOS 首次启动检测 Screen Recording 和 Microphone 权限。
- 权限缺失时给出中文引导，不得崩溃。
- Windows 录制受限窗口时提示用户权限限制。

### 10.2 Tauri 安全

- 禁止开启 `allow-all`。
- 只开放必要的 Tauri API 权限。
- 敏感信息不得硬编码。

### 10.3 授权系统

MVP 只设计本地试用与激活状态：

- `LicenseService` 负责查询试用期、激活状态、过期状态。
- 授权信息使用操作系统 Keychain 或安全凭据存储。
- 服务端激活协议、激活码生成、反滥用策略不进入 MVP 架构主线。

## 11. W1-W12 MVP 开发执行计划

### Phase 1 / W1-W2：脚手架与 macOS 录制闭环

研发重点：

- 搭建 Tauri 2.0 + React + TypeScript + Tailwind 工程骨架。
- 建立 Rust `app/core/platform/media` 基础模块。
- 实现 macOS ScreenCaptureKit 原型。
- 建立录制状态机和轻量事件通道。
- 输出可播放中间文件或 mp4。

交付物：

- macOS 全屏录制 1080p 可启动、停止、写盘。
- 前端可发起录制命令并收到状态事件。
- 首版错误枚举与权限检测。

依赖关系：

- Phase 2 的音频与 Windows 接入依赖 Phase 1 的 Trait 和状态机。

主要风险：

- ScreenCaptureKit 权限和系统版本差异。
- 录制停止后的资源释放不完整。

应对措施：

- 先限定最低 macOS 版本。
- 首周就实现权限检测和失败提示。
- 所有底层回调释放路径人工审查。

### Phase 2 / W3-W4：Windows 捕获预研与双音频链路

研发重点：

- 建立 DXGI 与 WASAPI 的 Trait 实现雏形。
- 接入 cpal 麦克风采集。
- 建立 `MediaTimestamp` 时间基。
- 实现系统音频和麦克风对齐混音原型。

交付物：

- macOS 系统音频和麦克风混音可写入录制文件。
- Windows 录屏和系统音频完成可行性样例。
- 音频设备列表和错误提示模型。

依赖关系：

- Phase 3 UI 需要音频设备列表、权限状态和录制参数模型。

主要风险：

- Windows WASAPI loopback 设备兼容性高风险。
- 不同音频设备采样率和延迟差异导致音画不同步。

应对措施：

- 准备 Plan B：Windows MVP 可先麦克风录制，系统音频标记实验能力。
- 用时间戳对齐和重采样，不用帧数硬对齐。

### Phase 3 / W5-W6：录制 UI 与前后端联调

研发重点：

- 实现中文录制控制面板。
- 支持全屏、窗口、区域模式参数选择。
- 显示权限状态、录制状态、时长、错误提示。
- 完成前端 Tauri invoke/event 封装。

交付物：

- 完整录制流程 UI 可用。
- 前端不接触视频帧和音频流。
- 录制失败、权限缺失、设备不可用均有中文提示。

依赖关系：

- Phase 4 的光标效果依赖录制流程和元数据采集入口。

主要风险：

- 录制状态机被前端重复实现。
- UI 调用与 Rust 状态不同步。

应对措施：

- 前端只显示 Rust 下发状态。
- 状态迁移只在 Rust `RecordingStateMachine` 中发生。

### Phase 4 / W7-W8：光标平滑与点击放大

研发重点：

- 采集鼠标位置与点击事件。
- 实现移动平均滤波。
- 实现贝塞尔轨迹插值。
- 实现点击放大状态机。
- 生成 `EffectTimeline` 并在录后处理阶段叠加。

交付物：

- 光标轨迹无明显抖动。
- 点击动画有平滑放大和恢复。
- 光标算法单元测试覆盖边界输入。

依赖关系：

- Phase 5 空白检测可复用时间线模型。
- Phase 6 导出需要消费效果时间线。

主要风险：

- 光标效果影响编码性能。
- 过度平滑导致光标位置滞后。

应对措施：

- MVP 坚持录后处理。
- 滤波窗口按帧率自适应，并限制最大延迟。

### Phase 5 / W9-W10：空白段检测与自动裁剪

研发重点：

- 实现音频 RMS 静音检测。
- 实现低分辨率帧差分。
- 合并候选空白段。
- 输出 `CutTimeline`。
- 通过 FFmpeg 封装执行裁剪导出。

交付物：

- 长静音且画面低变化片段可识别。
- 短暂停顿不被默认裁剪。
- 原始素材保留，可重新导出。

依赖关系：

- Phase 6 三种导出预设消费裁剪时间线。

主要风险：

- 误剪用户思考时间。
- 加载动画和静态画面被误判。

应对措施：

- 默认阈值保守。
- 候选段前后保留缓冲。
- 初期输出建议时间线，不破坏原始素材。

### Phase 6 / W11-W12：导出预设与本地授权

研发重点：

- 实现 16:9、9:16、1:1 导出预设。
- 实现导出进度与取消。
- 实现本地 14 天试用状态。
- 实现激活状态接口与凭据存储接口。
- 汇总错误上报与性能日志。

交付物：

- 三种比例导出成功。
- 试用状态可展示。
- 激活状态接口存在但不展开服务端协议。
- MVP 可交付给 20 个种子用户测试。

依赖关系：

- 后续 V2 背景美化和自动缩放复用导出与效果时间线。

主要风险：

- 授权系统发散成商业化后端。
- 导出比例裁切体验不稳定。

应对措施：

- 授权只做本地状态和接口。
- 导出预设固定，不做模板系统。

## 12. 前端 UI 与 Rust 联调节奏

| 阶段 | 前端重点 | Rust 重点 | 联调验证 |
| --- | --- | --- | --- |
| W1-W2 | 最小录制按钮和状态展示 | macOS 录制闭环 | 点击开始/停止后产出文件 |
| W3-W4 | 设备列表与音频开关 | 音频捕获与混音 | 音频设备可选，输出文件有音轨 |
| W5-W6 | 完整中文录制流程 | 状态机、权限、错误事件 | UI 状态完全来自 Rust |
| W7-W8 | 光标效果开关与预览入口 | 光标时间线 | 导出视频包含光标效果 |
| W9-W10 | 空白段建议展示 | 检测与裁剪时间线 | 裁剪前后时长可解释 |
| W11-W12 | 导出对话框、试用状态 | 导出预设、LicenseService | 三种格式导出，试用状态可展示 |

## 13. 集成风险矩阵

| 风险 | 阶段 | 概率 | 影响 | 应对 |
| --- | --- | --- | --- | --- |
| macOS 权限导致录制失败 | W1-W2 | 中 | 高 | 首次启动权限检测，中文引导 |
| ScreenCaptureKit 系统版本差异 | W1-W2 | 中 | 中 | 锁定最低版本，能力检测 |
| Windows WASAPI 兼容性 | W3-W4 | 高 | 高 | 先做可行性样例，准备麦克风优先 Plan B |
| 音画不同步 | W3-W4 | 中 | 高 | 统一时间戳、重采样、混音后验证 |
| 前端误承载业务状态机 | W5-W6 | 中 | 中 | Rust 作为唯一状态源 |
| 光标算法过度平滑 | W7-W8 | 中 | 中 | 自适应窗口和边界测试 |
| 空白裁剪误剪 | W9-W10 | 高 | 高 | 双信号确认、保守阈值、保留原始素材 |
| 4K 性能不达标 | 全程 | 高 | 中 | MVP 验收以 1080p 为主，4K 降级到 30fps |
| 授权系统范围膨胀 | W11-W12 | 中 | 中 | 只做本地试用和激活状态接口 |

## 14. 测试与验证策略

### 14.1 Rust 单元测试

必须覆盖：

- `RecordingStateMachine` 状态迁移。
- 光标平滑算法：0 帧、单点、高频抖动、快速跳变。
- 点击放大状态机。
- RMS 静音检测。
- 帧差分边界条件。
- 裁剪时间线合并逻辑。
- 音频时间戳对齐和静音填充。

目标：核心逻辑覆盖率大于 85%。

### 14.2 前端测试

必须覆盖：

- 录制控制组件。
- 权限提示。
- 导出预设选择。
- 试用状态展示。
- Tauri `invoke` mock。

目标：React UI 组件覆盖率大于 60%。

### 14.3 手动验证

每个 Phase 都需要执行 `tests/` 目录下对应自测清单。完成后在 `HANDOFF.md` 中记录验证结论、阻塞点和下一步入口。

## 15. V2 架构演进

### 15.1 背景模糊 / 纯色背景

预留方式：

- 复用 `EffectTimeline`。
- 在 `Render Effects` 阶段增加背景处理效果。
- GPU 可用时走 Metal / DX 路径，GPU 不可用时降级为录后 CPU 处理。

### 15.2 自动缩放关键区域

预留方式：

- 录制期保留鼠标、窗口、点击、输入等元数据入口。
- V2 引入关键区域分析器，输出缩放时间线。
- 导出阶段消费缩放时间线，不修改捕获主链路。

### 15.3 GPU 渲染管线

预留方式：

- 平台能力通过 `capabilities()` 暴露。
- 帧结构保留平台 buffer / texture 互操作扩展点。
- 编码封装层记录 CPU/GPU 复制点，为性能优化提供依据。

## 16. 过度设计边界

MVP 禁止引入：

- 插件系统。
- 云端协作。
- 平台发布 API。
- 字幕、摘要、知识库。
- 模板市场。
- 完整商业化后端。
- 实时 4K AI 美化的硬承诺。
- 前端 JS 音视频帧处理。

架构要为 V2 留接口，但不提前实现 V2 功能。
