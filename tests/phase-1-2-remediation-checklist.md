# Phase 1/2 录制流水线整改自测清单

## 目标

验证 Phase 1 和 Phase 2 整改后，macOS 录制主链路具备可构建、可启动、可停止、可释放资源、可混音、可产出本地录制产物的最小闭环，并确认捕获线程不会被 UI、编码、混音或队列背压阻塞。

## 自动化验证

- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过。
- [x] `cargo test --manifest-path src-tauri/Cargo.toml` 通过（50 tests）。
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 无 error（FFI 命名 warning 可接受）。
- [x] `cargo build --manifest-path src-tauri/Cargo.toml` 通过，不再出现 CoreMedia 未定义符号。
- [x] `npm run build` 通过。
- [x] `npm test -- --run` 通过（7 tests）。

## Phase 1 功能验收

- [x] `npm run tauri dev` 能启动桌面应用。
- [x] React UI 显示中文录制入口。
- [x] 点击"开始录制"后，Rust 状态机进入 `recording`。
- [ ] 原生捕获启动期间 UI 不冻结。
- [x] 点击"停止录制"后，Rust 状态机进入 `completed` 或明确错误状态。
- [x] `stop_recording` 返回 `frame_count > 0`（CountingRecordingWriter 已接入）。
- [ ] 启用 artifact writer 后，`stop_recording` 返回非空 `output_path`。
- [ ] `output_path` 指向本地存在的可播放录制文件。
- [x] 权限缺失时应用不崩溃，并返回中文错误或权限状态。
- [x] `RecordingResult` 前后端契约统一为 camelCase。
- [x] stop 失败时状态机转为 `Failed`，允许重新 start。

## Phase 2 功能验收

- [x] 系统音频可生成 `AudioChunk`（ScreenCaptureKit 条件注册音频输出）。
- [ ] 麦克风可生成 `AudioChunk`。
- [x] 麦克风 `AudioChunk.timestamp` 单调递增（AudioSampleClock 已接入）。
- [x] 系统音频和麦克风可生成 `MixedAudioChunk`（AudioSynchronizer 有序配对）。
- [x] 录制停止结果中 `mixed_audio_chunk_count > 0`（CountingRecordingWriter 计数）。
- [ ] 启用 artifact writer 后，录制文件包含音频轨道。
- [x] 前端系统音频/麦克风开关会在 `start_recording` 前传给 Rust。
- [x] `set_audio_config` 的 TypeScript payload 与 Rust serde payload 一致。
- [x] `captureSystemAudio` 配置被 ScreenCaptureKit 尊重（条件注册音频输出）。

## 捕获主链路与性能风险

- [x] 视频帧回调使用有界非阻塞队列，队列满时不阻塞 ScreenCaptureKit 回调。
- [x] 音频回调使用有界非阻塞队列，队列满时不阻塞 ScreenCaptureKit/cpal 回调。
- [x] cpal 音频回调热路径使用 `AtomicBool` 替代 `Mutex<bool>`，sink 直接持有 clone。
- [x] 音频配对使用有序时间戳窗口（10ms），替代"仅保留最新"策略。
- [ ] 捕获线程不等待 UI 事件。
- [ ] 捕获线程不等待 FFmpeg 写入或混音完成。
- [ ] 录制 1080p 3 分钟无明显卡顿。
- [ ] 重复开始/停止 5 次无崩溃。
- [ ] stop 后不再继续发出旧 session 的 `recording-tick`。

## 内存安全与资源释放

- [ ] ScreenCaptureKit 视频 buffer 在 unlock 前完成拷贝或保留合法所有权。
- [ ] ScreenCaptureKit 音频 `AudioBufferList` 动态按系统返回大小分配。
- [x] ScreenCaptureKit 音频格式解析检查 format id、format flags、bits per channel、channel count。
- [x] ScreenCaptureKit 音频布局解析支持非交错（non-interleaved）PCM 缓冲区。
- [x] ASBD 字段验证（mFramesPerPacket、mBytesPerFrame、mChannelsPerFrame 非零）。
- [ ] 不再把所有音频样本无条件 cast 为 `*const f32`。
- [ ] `CMBlockBuffer` retained 引用在所有返回路径正确释放。
- [ ] `stopCaptureWithCompletionHandler` 超时返回结构化错误，不静默清空 Rust handle。
- [x] cpal stream drop 后不会继续发送 chunk（AtomicBool 控制）。
- [x] 消费线程有 stop flag 且 stop 时 join。
- [x] 没有新增不必要的 `unwrap()` 或 `expect()`。

## BUG.md 规则回归

- [x] `src-tauri/Cargo.toml` 保留 `tauri` 的 `macos-private-api` feature。
- [x] `src-tauri/tauri.conf.json` 保留 `macOSPrivateApi: true`。
- [x] `src-tauri/tauri.conf.json` 保留窗口 `transparent: true` 和 `acceptFirstMouse: true`。
- [x] `src-tauri/capabilities/default.json` 保留 `core:window:allow-start-dragging`。
- [x] 前端不再使用大容器级 `data-tauri-drag-region={false}`。
- [x] 交互元素仍通过 button/input/role/tabindex/contenteditable 选择器排除拖拽。

## Windows 边界

- [x] macOS 构建不会编译 Windows DXGI/WASAPI stub。
- [x] Windows stub 文件中的 `CaptureConfig` 从 `crate::core::config` 导入。
- [x] 无 Windows 测试机时，`HANDOFF.md` 明确记录 Windows 可行性验证未执行。
- [ ] 有 Windows 测试机时，`cargo check --target x86_64-pc-windows-msvc` 结果已记录。

## 人工审查

- [ ] ScreenCaptureKit unsafe/FFI 代码已逐行审查。
- [x] cpal `SendStream` unsafe wrapper 已审查线程安全假设（AtomicBool + try_send）。
- [ ] `SendSCStream` unsafe wrapper 已审查线程安全和生命周期假设。
- [ ] FFmpeg binding 依赖版本已人工批准。
- [ ] 本地录制 artifact 已人工播放验证。
- [ ] macOS 权限探测（CGPreflightScreenCaptureAccess / AVAudioApplication）需人工审查后启用。
