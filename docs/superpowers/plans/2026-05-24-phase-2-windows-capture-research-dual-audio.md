# Phase 2 / W3-W4 实施计划：Windows 捕获预研与双音频链路

> 日期：2026-05-24
> 基于：`docs/architecture/project-architecture-and-overall-planning.md` Phase 2 章节
> 输入依据：Phase 1 已交付的 Trait、状态机、帧类型、录制服务
> 验收标准：`tests/phase-2-w3-w4-checklist.md`

## Context

Phase 1 完成了 Tauri 2.0 脚手架、录制状态机、RecordingService、ScreenCapture Trait、VideoFrame 类型和 UI 骨架。但 ScreenCaptureKit 的 `start()` 是硬门控（返回 `NativeCaptureUnavailable`），音频链路完全缺失，Windows 平台模块不存在。

Phase 2 的目标是在 macOS 上打通"视频捕获 + 系统音频 + 麦克风采集 + 混音"的完整音频链路，同时为 Windows DXGI/WASAPI 预留 Trait stub 骨架。

**关键决策（已确认）：**
- ScreenCaptureKit 统一捕获音视频（SCStream 同时输出视频帧和系统音频）
- RecordingService 泛型扩展为 `<C: ScreenCapture, A: AudioCapture>`
- 录制期实时混音（非录后混音）
- Windows 仅做 Trait stub 骨架，不包含 FFI 代码

---

## Task 1: 添加音频数据类型和 AudioCapture Trait

**目标：** 在 core 层定义音频相关类型和 Trait，与架构文档对齐。

**修改文件：**
- `src-tauri/src/core/frame.rs` — 新增 `AudioChunk`、`MixedAudioChunk`
- `src-tauri/src/core/capture.rs` — 新增 `AudioCapture` Trait、`AudioConfig`、`AudioDevice`、`AudioCapabilities`、`AudioChunkSink`
- `src-tauri/src/core/mod.rs` — 确认导出

**具体变更：**

`frame.rs` 新增：
```rust
pub struct AudioChunk {
    pub timestamp: MediaTimestamp,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Arc<[f32]>,
}

pub struct MixedAudioChunk {
    pub timestamp: MediaTimestamp,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Arc<[f32]>,
}
```

`capture.rs` 新增：
```rust
pub type AudioChunkSink = Sender<AudioChunk>;

pub struct AudioConfig {
    pub capture_system_audio: bool,
    pub capture_microphone: bool,
    pub microphone_device: Option<String>,
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub struct AudioCapabilities {
    pub supports_system_audio: bool,
    pub supports_microphone: bool,
}

pub trait AudioCapture: Send {
    fn start(&mut self, config: AudioConfig, sink: AudioChunkSink) -> AppResult<()>;
    fn stop(&mut self) -> AppResult<()>;
    fn device_list(&self) -> AppResult<Vec<AudioDevice>>;
    fn capabilities(&self) -> AudioCapabilities;
}
```

**验证：** `cargo build` 通过，`cargo test` 通过（既有测试不受影响）。

---

## Task 2: 添加 AppError 音频变体

**目标：** 扩展错误模型以覆盖音频相关错误场景。

**修改文件：**
- `src-tauri/src/app/error.rs`

**具体变更：**
新增错误变体：
```rust
AudioCaptureFailed { reason: String }      // 音频捕获运行时失败
AudioDeviceNotFound { name: String }       // 指定麦克风设备不存在
AudioMixFailed { reason: String }          // 混音失败（采样率不兼容等）
```

每个变体实现中文 Display 消息。

**验证：** `cargo build` 通过。

---

## Task 3: 激活 ScreenCaptureKit 真实实现（音视频统一捕获）

**目标：** 将 `MacScreenCapture` 从 stub 替换为真实的 ScreenCaptureKit 集成，同时实现视频帧和系统音频捕获。SCStream 天然支持同时输出视频帧和系统音频，因此音视频在同一个 Task 中实现。

**修改文件：**
- `src-tauri/src/platform/macos/screen_capture_kit.rs` — 重写为真实实现（`MacCapture` 同时实现 `ScreenCapture` + `AudioCapture`）
- `src-tauri/Cargo.toml` — 添加 `objc2-core-media`、`objc2-core-video`、`objc2-screen-capture-kit`（objc2 系列 crate 提供 ScreenCaptureKit 安全绑定）

**实现要点：**
1. 创建 `SCStream` 配置（屏幕捕获 1080p@30fps + 音频输出 `capturesAudio = true`）
2. 视频回调：接收 `CMSampleBuffer` → 提取像素数据 → 转为 `VideoFrameRef` → 发送到 `VideoFrameSink`
3. 音频回调：接收 `CMSampleBuffer` → 提取 PCM 数据 → 转为 `AudioChunk` → 发送到 `AudioChunkSink`
4. `ScreenCapture` Trait 实现：视频捕获控制
5. `AudioCapture` Trait 实现：系统音频捕获，`capabilities()` 返回 `supports_system_audio: true`
6. `stop()` 完整释放：停止 SCStream、注销回调、清理 buffer 引用

**关键约束：**
- 系统音频需要 macOS 13+ 和 Screen Recording 权限
- 音频和视频来自同一 SCStream，时间戳天然对齐（同一时钟基准）
- 音频格式转换：CMSampleBuffer (Int16/Float32) → 统一 f32 采样
- MacCapture 构造时接收 `AudioChunkSink`，存储在结构体中；`ScreenCapture::start()` 只接收 `VideoFrameSink`，音频 sink 在构造时注入

**人工审查检查点（必须在激活前完成）：**
- [ ] buffer 所有权：CMSampleBuffer → Arc<[u8]> 的内存拷贝路径是否安全
- [ ] 回调线程：SCStream 回调在哪个线程？channel 发送是否线程安全？
- [ ] stop 路径：所有 `retain`/`release` 是否配对？是否存在 use-after-free？
- [ ] 错误处理：回调中的错误如何传播？是否会导致 panic？
- [ ] 帧率控制：回调频率是否受控？是否存在帧率失控导致内存暴涨？
- [ ] 音频 buffer 生命周期：CMSampleBuffer 音频数据的引用是否安全
- [ ] 采样格式转换：Int16 → f32 转换是否有精度损失或溢出

**验证：** `cargo build` 通过，`cargo test` 通过。实际录屏验证需要 `npm run tauri dev`（验证视频帧和系统音频均被采集）。

---

## Task 4: cpal 麦克风采集

**目标：** 实现独立的麦克风采集模块，通过 cpal crate 捕获麦克风音频。

**修改文件：**
- `src-tauri/src/platform/macos/cpal_microphone.rs` — 新建
- `src-tauri/src/platform/macos/mod.rs` — 导出新模块
- `src-tauri/Cargo.toml` — 添加 `cpal` 依赖

**实现要点：**
1. `CpalMicrophoneCapture` 结构体实现 `AudioCapture` Trait
2. `start()` 使用 cpal 打开默认麦克风设备，配置采样率和声道数
3. 音频回调将 cpal 的 `f32` 采样打包为 `AudioChunk` 发送到 sink
4. `stop()` 停止 cpal stream 并释放资源
5. `device_list()` 枚举系统可用音频输入设备
6. `capabilities()` 返回 `supports_system_audio: false, supports_microphone: true`

**依赖：** `cpal = "0.15"`（需人工审查版本）

**验证：** `cargo build` 通过，`cargo test` 通过（Mock 测试不调用真实设备）。

---

## Task 5: 音频混音器

**目标：** 实现实时音频混音，将系统音频和麦克风音频合并为 MixedAudioChunk。

**修改文件：**
- `src-tauri/src/media/audio_mixer.rs` — 新建
- `src-tauri/src/media/mod.rs` — 新建（media 模块入口）
- `src-tauri/src/lib.rs` — 导出 media 模块

**实现要点：**
1. `SimpleAudioMixer` 实现混音逻辑：
   - 重采样到统一采样率（48000 Hz）
   - 统一声道布局（立体声）
   - 基于 `MediaTimestamp` 对齐，短缺区间静音填充（零值 f32）
   - 重叠区间加权混音，硬限幅防止 clipping（`sample.clamp(-1.0, 1.0)`）
2. `AudioMixer` Trait 定义（可 Mock 测试）：
   ```rust
   pub trait AudioMixer: Send {
       fn mix(&self, system: Option<&AudioChunk>, mic: Option<&AudioChunk>) -> AppResult<MixedAudioChunk>;
   }
   ```

**算法注释要求：** 重采样算法、混音权重、限幅策略必须有注释说明思路。

**验证：** 单元测试覆盖：
- 系统音频和麦克风采样率一致时正常混音
- 采样率不一致时正确重采样
- 一方缺失时静音填充
- 双方重叠时不爆音（clipping 测试）

---

## Task 6: 扩展 RecordingService 为音视频统一编排

**目标：** 将 RecordingService 从单视频扩展为音视频统一管理。

**修改文件：**
- `src-tauri/src/app/recording_service.rs` — 重写泛型签名和实现

**具体变更：**
```rust
pub struct RecordingService<C: ScreenCapture, A: AudioCapture> {
    capture: C,
    audio_capture: A,
    state_machine: RecordingStateMachine,
    video_receiver: Option<Receiver<VideoFrameRef>>,
    audio_receiver: Option<Receiver<AudioChunk>>,
}
```

- `start(config, audio_config)` — 同时启动视频和音频捕获
- `stop()` — 同时停止音视频，等待所有 channel 关闭
- **实时混音：** RecordingService 内部持有 `AudioMixer` 实例，录制期间消费系统音频和麦克风 `AudioChunk`，实时输出 `MixedAudioChunk`（通过 channel 传递给后续编码/写盘环节）
- 现有测试更新以适配新泛型签名（使用 MockAudioCapture）

**验证：** `cargo test` 通过，包括更新后的 RecordingService 测试。

---

## Task 7: Windows DXGI/WASAPI Trait stub 骨架

**目标：** 创建 Windows 平台目录结构和 Trait stub，与 macOS 的 hard gate 模式一致。

**修改文件：**
- `src-tauri/src/platform/windows/mod.rs` — 新建
- `src-tauri/src/platform/windows/dxgi_capture.rs` — 新建
- `src-tauri/src/platform/windows/wasapi_loopback.rs` — 新建
- `src-tauri/src/platform/mod.rs` — 添加条件编译 `pub mod windows`

**实现要点：**
- `DxgiCapture` 实现 `ScreenCapture` Trait，所有方法返回 `NativeCaptureUnavailable`
- `WasapiLoopback` 实现 `AudioCapture` Trait，所有方法返回 `NativeCaptureUnavailable`
- `capabilities()` 返回全 false 的能力集
- 条件编译：`#[cfg(target_os = "windows")]` 确保 macOS 编译不报错

**验证：** macOS 上 `cargo build` 通过（Windows 模块被条件编译排除）。

---

## Task 8: 扩展 Tauri 命令和事件

**目标：** 将前端定义的 8 个缺失 Tauri 命令连接到 Rust 后端。

**修改文件：**
- `src-tauri/src/lib.rs` — 新增命令处理器，注册 `manage()` 状态

**新增命令：**
- `start_recording` — 调用 RecordingService.start()
- `stop_recording` — 调用 RecordingService.stop()
- `pause_recording` — 暂停录制
- `resume_recording` — 恢复录制
- `set_capture_mode` — 更新捕获配置
- `set_audio_config` — 更新音频配置

**新增事件发射：**
- `recording-state-changed` — 状态变化时推送
- `recording-tick` — 录制计时（每秒推送 elapsed）
- `mic-level` — 麦克风音量级别

**前置步骤：扩展状态机添加 Paused 状态：**
- 在 `RecordingStateMachine` 中添加 `Paused` 状态
- 状态图：`Idle -> Recording -> Paused -> Recording`（暂停/恢复循环），`Recording -> Processing -> Completed`
- `pause()` 方法：从 `Recording` 迁移到 `Paused`
- `resume()` 方法：从 `Paused` 迁移到 `Recording`
- 更新 `RecordingState::as_str()` 返回 `"paused"`
- 更新 `RecordingStatusPayload` 的 `can_start` 逻辑
- 与前端 TypeScript `RecordingState` 类型的 `'paused'` 变体对齐

**验证：** `cargo build` 通过，`cargo test` 通过。

---

## Task 9: 音频相关单元测试

**目标：** 满足 `tests/phase-2-w3-w4-checklist.md` 中的测试要求。

**新增测试文件：**
- `src-tauri/src/media/audio_mixer.rs` 内 `#[cfg(test)] mod tests`
- `src-tauri/src/core/frame.rs` 内补充 AudioChunk 相关测试

**测试用例：**
1. 音频时间戳对齐：两个 AudioChunk 时间戳不一致时正确对齐
2. 静音填充：一方 AudioChunk 缺失时输出零值采样
3. 不同采样率：44100Hz 和 48000Hz 输入正确重采样到统一输出
4. Clipping 防止：双声道满幅信号混音后不超 [-1.0, 1.0]
5. MockAudioCapture 测试：AudioCapture Trait 的 Mock 实现验证 start/stop 流程

**验证：** `cargo test` 通过，核心逻辑覆盖率 > 85%。

---

## Task 10: Phase 2 验证与交接

**目标：** 执行自测清单，更新 HANDOFF.md。

**步骤：**
1. `cargo fmt --check` + `cargo clippy` 通过
2. `cargo test` 全部通过
3. `npm run build` 通过
4. `npm run test` 通过
5. 逐项执行 `tests/phase-2-w3-w4-checklist.md`，记录结果
6. 更新 `HANDOFF.md` 记录 Phase 2 完成状态
7. 检查无 TODO/TBD/FIXME 残留（除 ScreenCaptureKit 人工审查标记）

---

## 依赖变更汇总

`Cargo.toml` 新增依赖（需人工审查版本）：
- `cpal = "0.15"` — 麦克风音频采集
- `objc2-core-media` — ScreenCaptureKit FFI 绑定（CMSampleBuffer 等）
- `objc2-core-video` — 视频帧相关类型
- `objc2-screen-capture-kit` — SCStream API 绑定

## 手动门控

1. **Task 3 前**：ScreenCaptureKit FFI 代码必须由人工逐行审查内存安全、线程安全和资源释放路径
2. **Task 4 前**：`cpal` crate 版本需人工审查
3. **所有 Task 完成后**：执行 `tests/phase-2-w3-w4-checklist.md` 全部检查项

## 验证方式

1. `cargo test` — 全部 Rust 单元测试通过
2. `npm run build` — 前端构建通过
3. `npm run test` — 前端测试通过
4. `npm run tauri dev` — 桌面端启动，验证：
   - 开始录制后系统音频和麦克风被采集
   - 录制的视频文件包含音轨
   - 停止录制后状态正确转换
5. `tests/phase-2-w3-w4-checklist.md` — 逐项验证
