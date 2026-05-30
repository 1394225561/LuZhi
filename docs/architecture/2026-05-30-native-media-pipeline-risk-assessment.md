# 放弃 FFmpeg 转向平台原生媒体管线的风险与成本评估

> 日期：2026-05-30
> 状态：评估稿，尚未作为最终架构决策
> 背景：当前 PRD 与总体架构计划在视频处理、视频编码、音频处理上以 FFmpeg / `ffmpeg-next` 为主线。本文评估若彻底放弃 FFmpeg，改为 macOS 与 Windows 各自原生实现，会带来的难点、风险点和迁移成本。

## 1. 结论摘要

彻底放弃 FFmpeg，在技术上可行。macOS 和 Windows 都具备原生捕获、合成、硬件编码和 MP4/MOV 封装能力，可以支撑类似 Screen Studio 的美观屏幕录制视频，包括自动缩放、鼠标跟随、平滑动画、点击放大和多比例导出。

但这不是简单降本。更准确的描述是：

- FFmpeg 方案的复杂度集中在第三方媒体库、C API、动态库打包、许可证、filter graph、packet/frame 生命周期。
- 原生方案的复杂度集中在双平台底层 API、GPU compositor、硬件编码兼容性、音画同步、COM/ObjC/CoreMedia/D3D 资源生命周期和测试矩阵。

短期看，原生方案开发成本更高，尤其 Windows 侧风险明显高于 macOS。长期看，原生方案可以减少 FFmpeg 打包体积、许可证治理和第三方库分发复杂度，更贴近桌面产品的系统能力，但需要承担更重的平台工程能力建设。

当前仓库的 FFmpeg 代码耦合仍然较浅，`ffmpeg-next` 仍是 optional feature，`FfmpegRecordingWriter` 和 `FfmpegTrimExporter` 都还是骨架，默认录制也未接入真实 FFmpeg writer。因此如果要转向，现在是相对合适的窗口；等 FFmpeg writer/exporter 真正实现后再切换，成本会显著升高。

## 2. 当前方案中 FFmpeg 承担的职责

当前 PRD 和架构文档中，FFmpeg 大致承担以下能力：

1. 视频处理：裁剪、缩放、转码、格式转换、导出比例适配。
2. 视频编码：H.264 / H.265 编码。
3. 音频处理：系统音频和麦克风混音后的编码、重采样、封装。
4. 媒体容器封装：生成可播放的 MP4/MOV 等最终文件。
5. 验收与测试辅助：通过 FFmpeg/ffprobe 风格的 artifact inspection 验证真实视频输出。

如果放弃 FFmpeg，这些职责不能消失，只是需要分配给平台原生模块。

## 3. 原生替代路线概览

### 3.1 macOS 原生路线

建议能力分配：

- 捕获：ScreenCaptureKit。
- 视频合成：Metal 或 CoreImage，必要时结合 CoreAnimation 进行 overlay。
- 视频编码：AVAssetWriter + VideoToolbox 硬件编码。
- 音频编码：AVAssetWriter 音频 input，输出 AAC。
- 容器封装：AVAssetWriter 输出 MP4/MOV。
- 导出裁剪：AVAssetReader + 自研 timeline/compositor + AVAssetWriter，或基于已保存原始帧/压缩帧重新渲染导出。

macOS 侧整体可行性较高，但仍然涉及 Objective-C / CoreFoundation / CoreMedia / CoreVideo / AVFoundation 的资源生命周期和线程安全问题。

### 3.2 Windows 原生路线

建议能力分配：

- 捕获：Windows Graphics Capture 或 DXGI Desktop Duplication。
- 系统音频：WASAPI loopback。
- 麦克风：WASAPI 或 cpal。
- 视频合成：Direct3D11 / Direct2D / shader pipeline。
- 视频编码：Media Foundation Sink Writer + H.264 encoder MFT，优先硬件编码，必要时 fallback。
- 音频编码：Media Foundation AAC encoder。
- 容器封装：Media Foundation MP4 sink / Sink Writer。

Windows 侧可行，但工程风险较高。主要原因是 COM、Media Foundation、D3D11、驱动、硬件编码器和系统组件安装状态都会进入问题空间。

## 4. 最大变化：从“一套媒体后端”变成“两套平台后端”

FFmpeg 方案的一个主要优势是跨平台媒体处理逻辑相对统一。原生方案下，架构必须接受以下事实：

- macOS 和 Windows 会有不同 writer。
- macOS 和 Windows 会有不同 exporter。
- macOS 和 Windows 会有不同 compositor。
- macOS 和 Windows 会有不同 encoder 参数与失败模式。
- macOS 和 Windows 会有不同 artifact inspection 方法。

仍然应该共享的部分：

- `EffectTimeline`
- `CutTimeline`
- `ExportPreset`
- 光标平滑算法
- 自动缩放决策算法
- 空白段检测算法
- 导出任务状态机
- Tauri command/event 边界
- 前端 UI 与进度展示

不适合强行共享的部分：

- 帧合成 API 调用
- GPU texture / pixel buffer 管理
- 硬件编码器调用
- 音频 encoder/muxer 调用
- 平台资源释放路径

核心架构原则应调整为：业务 timeline 跨平台共享，媒体执行后端平台分治。

## 5. 视频处理风险

### 5.1 Screen Studio-like 效果不是编码器能力

自动缩放、鼠标跟随、平滑动画、点击放大，核心不是 FFmpeg 或 VideoToolbox/Media Foundation 的能力，而是产品自己的 timeline 与 compositor 能力。

需要实现的真实能力包括：

- 采集每一帧对应的光标位置、点击、窗口/屏幕尺寸、录制区域、时间戳。
- 生成虚拟摄像机轨迹，控制画面跟随。
- 处理光标抖动，避免画面随微小移动频繁抖动。
- 设置 dead zone，光标在安全区域内时不移动视角。
- 识别点击、停顿、输入等触发条件，触发局部放大。
- 生成 zoom in / zoom out 的 easing curve，缩放和位移不能线性跳变。
- 处理多比例导出时的裁剪、留白、缩放和边缘约束。
- 自绘光标、点击波纹、阴影、高亮区域等 overlay。
- 在导出时逐帧重建最终画面。

FFmpeg filter graph 可以处理 scale/crop/overlay 的一部分，但产品级的自动缩放逻辑仍然要自己写。放弃 FFmpeg 后，真正新增的是必须自研平台 compositor。

### 5.2 compositor 难点

macOS compositor 难点：

- `CVPixelBuffer` 与 Metal/CoreImage 的转换。
- Retina/HiDPI 坐标到真实像素坐标的映射。
- 色彩空间和 alpha blending。
- 渲染输出尺寸与导出预设比例的一致性。
- 逐帧渲染时不能产生不必要 CPU 拷贝。
- 长视频导出时 GPU/CPU 内存不能持续增长。

Windows compositor 难点：

- D3D texture 生命周期。
- DXGI capture frame 到 compositor 的 zero-copy 或低拷贝路径。
- Direct2D/DirectWrite/自定义 shader 之间的取舍。
- GPU device lost 后的重建。
- 高 DPI、多显示器、不同刷新率环境。
- 与 Media Foundation encoder 的 texture/sample 交接。

核心风险：两端 compositor 视觉效果可能不一致。即使 timeline 完全相同，插值、采样、颜色、阴影、缩放质量也可能出现平台差异。

## 6. 视频编码风险

### 6.1 macOS 编码风险

macOS 可通过 AVAssetWriter 和 VideoToolbox 实现 H.264/HEVC 硬件编码。风险包括：

- `CVPixelBufferPool` 配置错误导致频繁分配或内存上涨。
- pixel format 选择不当导致额外转换，如 BGRA 到 NV12。
- 时间戳必须单调递增，否则 writer append 失败或导出卡顿。
- frame duration 不稳定会导致播放抖动。
- `AVAssetWriterInput` 的 ready 状态必须处理，不能无界塞帧。
- writer 状态机复杂，failed/cancelled/completed 后不能继续 append。
- `finishWriting`、cancel、partial output 删除必须覆盖所有错误路径。
- HEVC 在兼容性和授权层面需要谨慎，不适合作为唯一默认。
- 色彩空间处理不好会导致导出颜色发灰、偏暗或饱和度异常。

### 6.2 Windows 编码风险

Windows 可通过 Media Foundation Sink Writer 和 H.264/AAC encoder MFT 实现编码封装。风险包括：

- COM 初始化和线程模型复杂。
- Media Foundation media type 参数繁多，错误配置可能只在特定机器上失败。
- H.264 encoder profile、level、bitrate、GOP、frame rate 参数需要精确控制。
- 硬件编码器行为依赖 GPU 和驱动。
- Windows N/KN 版本可能缺少 Media Feature Pack。
- 远程桌面、虚拟机、显卡驱动异常环境下行为不稳定。
- D3D texture 直接送 encoder 的路径复杂；退回 CPU copy 会影响性能。
- encoder drain/finalize 顺序错误会损坏输出文件尾部。
- 如果硬件编码不可用，是否提供软件 fallback 需要单独决策。不用 FFmpeg 后，软件 fallback 没有现成统一方案。

Windows 是原生方案里最大的不确定性来源。

## 7. 音频处理风险

放弃 FFmpeg 后，音频侧不能只依赖“平台会自动处理”。仍需建立清晰的 Rust 音频处理层。

### 7.1 必须处理的问题

- 系统音频和麦克风采样率不一致。
- 系统音频和麦克风 channel layout 不一致。
- 麦克风、系统音频、视频帧时间戳来自不同 clock。
- 长录制时音频时钟可能漂移。
- 蓝牙耳机存在高延迟和设备切换。
- 音频混音可能出现削波，需要 limiter 或 headroom。
- 静音段检测使用的 RMS metadata 需要与最终音频时间线一致。
- 自动裁剪后音频拼接不能产生爆音或 discontinuity。
- cancel/stop 时 audio encoder 需要 drain，不能漏尾或损坏文件。

### 7.2 macOS 音频路径

可行路线：

- ScreenCaptureKit 获取系统音频。
- cpal 或原生 API 获取麦克风。
- Rust 侧统一 sample rate、声道数、时间戳。
- 混音后交给 AVAssetWriter 音频 input 编码 AAC。

风险：

- ScreenCaptureKit 音频和 cpal 麦克风时间源不同。
- AVAssetWriter 对 audio sample timing 较敏感。
- 混音和重采样质量需要可接受，不能只做简单拼接。

### 7.3 Windows 音频路径

可行路线：

- WASAPI loopback 获取系统音频。
- WASAPI 或 cpal 获取麦克风。
- Rust 侧重采样、混音、时间戳对齐。
- Media Foundation AAC encoder 编码。

风险：

- WASAPI event/callback 模型与录制线程协调复杂。
- 设备热插拔和默认设备切换需要处理。
- 系统音频和麦克风的 clock drift 在长录制中更容易暴露。
- AAC encoder 输入格式限制需要适配。

## 8. 容器封装风险

不建议自行实现 MP4/MOV container writer。

原因是 MP4/MOV 并不是简单写 header，真正复杂的是：

- sample table
- chunk offset
- keyframe index
- duration/timescale
- H.264 SPS/PPS 与 `avcC`
- HEVC `hvcC`
- AAC codec config
- fast start
- 大文件 offset
- fragmented MP4
- partial output 清理
- 不同播放器兼容性

推荐：

- macOS 使用 AVAssetWriter 负责 MP4/MOV 封装。
- Windows 使用 Media Foundation Sink Writer / MP4 sink 负责 MP4 封装。

不要把“放弃 FFmpeg”理解成“自己写 MP4”。自己写容器会把项目带入更高风险区。

## 9. Native Safety 风险

放弃 FFmpeg 不代表 Native Safety 风险消失，只是风险从 FFmpeg C API 转移到平台原生 API。

### 9.1 macOS Native Safety 风险

- Objective-C retain/release/autorelease 生命周期。
- CoreFoundation create/copy rule。
- `CMSampleBuffer`、`CVPixelBuffer`、`CMBlockBuffer` 引用关系。
- `CVPixelBufferLockBaseAddress` 与 unlock 必须配对。
- ScreenCaptureKit callback 线程不能执行阻塞工作。
- `AVAssetWriter` append 和 finish 不能并发乱序。
- Rust wrapper 中不能随意把 native 对象标记为 `Send` / `Sync`。
- stop/cancel/failure 路径必须释放 capture、writer、pixel buffer pool、临时文件。

### 9.2 Windows Native Safety 风险

- COM `AddRef` / `Release` 生命周期。
- COM apartment/threading model。
- Media Foundation startup/shutdown 生命周期。
- `IMFSample`、`IMFMediaBuffer`、D3D texture 引用关系。
- `IDXGIOutputDuplication::AcquireNextFrame` 与 `ReleaseFrame` 必须配对。
- D3D device/context 跨线程访问限制。
- GPU device lost 后不能继续使用旧资源。
- encoder drain、finalize、flush、cancel 顺序必须明确。

因此 Native Safety Gate 需要从“FFmpeg 代码审查”调整为：

- macOS CoreMedia/AVFoundation/VideoToolbox/ScreenCaptureKit Native Safety Gate。
- Windows COM/DXGI/D3D/Media Foundation Native Safety Gate。

Windows 的人工审查复杂度预计高于 macOS。

## 10. 主链路阻塞风险

无论使用 FFmpeg 还是原生方案，捕获主链路都不能被编码、合成、导出阻塞。

原生方案下需要特别保证：

- ScreenCaptureKit callback 不等待 compositor。
- DXGI/WGC frame arrived 不等待 encoder。
- 音频 callback 不等待混音、重采样或文件写入。
- writer 队列必须有界。
- 队列满时有明确策略：丢帧、降级、停止录制或报错。
- 导出任务必须在 blocking worker 或专用线程运行。
- cancel token 必须能让长导出尽快退出。

风险点：

- AVAssetWriter / Media Foundation append 慢时，不能反向拖住 capture callback。
- GPU compositor 慢时，不能无限缓存 raw frames。
- 音频如果不允许丢，视频如果允许丢，需要定义 A/V sync 取舍。
- 长录制压力下，队列和 buffer pool 是最容易泄漏或膨胀的位置。

## 11. 测试与验收风险

原 FFmpeg 方案可通过 FFmpeg/ffprobe 或 `ffmpeg-next` inspection 验证 artifact。放弃 FFmpeg 后，测试策略需要重建。

需要覆盖：

- 输出文件存在且非空。
- 输出文件可被系统播放器播放。
- video stream 尺寸、帧率、duration 符合预设。
- audio stream sample rate、channel、duration 正确。
- A/V sync 在长录制后无明显漂移。
- 自动裁剪后音频无明显爆音。
- 光标跟随和 zoom timeline 视觉正确。
- 三种预设：16:9、9:16、1:1。
- cancel 导出清理 partial output。
- 原始录制 artifact 不被覆盖。
- 10 分钟 1080p 压测。
- 高 DPI、多显示器、不同刷新率。

验收工具替代：

- macOS 可用 AVFoundation 读取 metadata 做自动验证。
- Windows 可用 Media Foundation 读取 metadata 做自动验证。
- 如项目要求彻底无 FFmpeg，测试也不应依赖 ffprobe。
- 视觉效果需要 golden video / sampled frame / 人工样片验收组合。

## 12. 当前仓库迁移影响

当前源码对 FFmpeg 的实际耦合较浅，主要体现在：

- `src-tauri/Cargo.toml` 中 `ffmpeg-next` 是 optional dependency。
- `src-tauri/src/media/ffmpeg_writer.rs` 仍是骨架，不写真实文件。
- `src-tauri/src/media/trim_exporter.rs` 中 `FfmpegTrimExporter` 仍是骨架。
- `RecordingWriter` trait 是通用抽象，可以保留。
- `TrimExporter` trait 是通用抽象，可以保留。
- `ExportPreset`、`CutTimeline`、`EffectTimeline` 仍可作为共享业务模型。
- 默认录制仍使用 `CountingRecordingWriter`，未真正接入 FFmpeg writer。

迁移成本主要在文档、命名和计划口径：

- PRD 中视频处理、视频编码、音频处理需从 FFmpeg 改为 native media pipeline。
- 总体架构文档需要把 FFmpeg 后端替换为平台原生后端。
- Phase 6 FFmpeg playable export plan 需要作废或改写。
- Review/checklist 中 FFmpeg Gate 需要改为 Native Export Gate。
- UI 文案中“FFmpeg 编码器接入后”需要改为“可播放导出接入后”或更具体的平台 gate。
- 类型命名上，`FfmpegRecordingWriter` / `FfmpegTrimExporter` 应替换为 `NativeRecordingWriter`、`PlatformRecordingWriter` 或平台具体实现名。

建议保留现有抽象边界，不要把平台 API 直接泄漏到 app command 层。

## 13. 粗略成本评估

以下是单人或小团队在已有 Rust/Tauri 基础上继续推进的粗估，实际成本取决于 native 音视频经验和测试设备覆盖。

| 工作项                                     | 粗略成本 | 风险 |
| ------------------------------------------ | -------: | ---- |
| macOS 原生 playable recording writer       |   1-3 周 | 中   |
| macOS 原生 preset export + trim            |   1-3 周 | 中   |
| macOS Screen Studio-like compositor 初版   |   3-6 周 | 中高 |
| macOS 长录制、A/V sync、取消、异常路径打磨 |   2-4 周 | 中高 |
| Windows capture/audio 从 stub 到可用       |   3-6 周 | 高   |
| Windows Media Foundation writer/exporter   |   4-8 周 | 高   |
| Windows D3D compositor                     |   4-8 周 | 高   |
| 双平台一致性、样片验收、压力测试           |   3-6 周 | 高   |

如果只承诺 macOS 先打穿，原生路线是可控但不轻的工程。

如果承诺 macOS + Windows 同时达到 Screen Studio-like 产品质感，应按数月级工程评估，并且需要准备充足 native 测试设备。

## 14. 风险排序

从当前项目状态看，原生方案风险优先级如下：

1. Windows 原生媒体栈复杂度。
2. 双平台视觉效果一致性。
3. 长录制音画同步漂移。
4. GPU compositor 的质量、性能和内存控制。
5. 硬件编码兼容性和 fallback 策略。
6. 取消、失败、异常停止时的资源释放和 partial output 清理。
7. 测试矩阵扩大。
8. 现有文档、计划、验收口径迁移。
9. H.264/HEVC 商业分发与系统 codec 可用性确认。

## 15. 建议的决策方式

不建议一次性宣布“彻底双平台原生替换并立即完成”。更稳的决策路径：

1. 停止继续加深 FFmpeg writer/exporter 实现，避免沉没成本扩大。
2. 将 Phase 6 从 `FFmpeg playable export` 改为 `Native playable export` 评估计划。
3. 先做 macOS 原生 writer/exporter 技术尖刺：
   - 输入 synthetic frames + synthetic audio。
   - 输出 10 秒 MP4。
   - 验证可播放、duration、A/V sync、取消清理。
4. 再做 macOS compositor 技术尖刺：
   - 输入固定 timeline。
   - 输出含自动缩放、光标 overlay、点击放大的样片。
5. Windows 单独设为后续 Phase：
   - 先打通 capture + audio。
   - 再打通 Media Foundation writer。
   - 最后打通 D3D compositor。
6. 只有 macOS 原生 writer/exporter 尖刺通过后，再正式修改 PRD 和总体架构主线。

## 16. 推荐架构方向

若决定放弃 FFmpeg，推荐目标架构为：

```text
Shared Rust App Logic
  - Recording state
  - Export service
  - Export presets
  - CutTimeline
  - EffectTimeline
  - Cursor smoothing
  - Auto zoom decision
  - Silence detection
  - Progress/cancel/error model

Platform Media Backend Trait
  - RecordingWriter
  - TrimExporter / NativeExporter
  - Compositor
  - ArtifactInspector

macOS Backend
  - ScreenCaptureKit
  - CoreImage/Metal compositor
  - AVAssetWriter + VideoToolbox
  - AVFoundation artifact inspection

Windows Backend
  - Windows Graphics Capture or DXGI
  - WASAPI
  - D3D11 compositor
  - Media Foundation Sink Writer
  - Media Foundation artifact inspection
```

关键原则：

- 前端仍不接触媒体帧。
- 捕获主链路仍不能被合成、编码、导出阻塞。
- 业务 timeline 共享，平台媒体执行分治。
- 不自行实现 MP4/MOV container。
- 所有 native API 调用必须经过人工 Native Safety Gate。
- macOS 先打穿，Windows 后追平。

## 17. 最终建议

从长期产品角度，放弃 FFmpeg、转向平台原生媒体管线是合理选项，尤其适合追求轻包体、较少第三方许可证治理、贴近系统硬件能力的桌面产品。

但从短期交付角度，它不是降本方案，而是一次架构路线切换。短期会增加开发周期、Native Safety 审查和测试设备成本。Windows 侧尤其不能低估。

建议当前不要继续推进完整 FFmpeg writer/exporter，而是先做 macOS 原生可播放导出的技术尖刺。该尖刺通过后，再正式更新 PRD、总体架构文档和 Phase 6 计划；如果尖刺暴露出不可接受的时间/质量风险，再回到 FFmpeg 或混合路线。

## 18. 参考资料

- Apple ScreenCaptureKit Documentation: https://developer.apple.com/documentation/screencapturekit
- Apple AVAssetWriter Documentation: https://developer.apple.com/documentation/avfoundation/avassetwriter
- Apple VideoToolbox VTCompressionSession API: https://developer.apple.com/documentation/videotoolbox/vtcompressionsession-api-collection
- Microsoft Desktop Duplication API: https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api
- Microsoft Windows Graphics Capture Frame Pool: https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.direct3d11captureframepool
- Microsoft Media Foundation Sink Writer: https://learn.microsoft.com/en-us/windows/win32/medfound/sink-writer
- Microsoft H.264 Video Encoder: https://learn.microsoft.com/en-us/windows/win32/medfound/h-264-video-encoder
- Microsoft AAC Encoder: https://learn.microsoft.com/windows/win32/medfound/aac-encoder
