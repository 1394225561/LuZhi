# Phase 3 Code Review Remediation Checklist

> 最后更新：2026-05-26 | 来源：Phase 3 首版代码审查整改项

## 目标

修复 Phase 3 首版审查发现的并发状态、线程生命周期、权限测试隔离和测试覆盖缺口，使录制 UI 与前后端联调达到可合并标准。

## Phase A：前端开始录制防重入

- [ ] `src/App.tsx` 增加启动中防重入状态，例如 `isStarting` 或等价机制。
- [ ] 点击“开始录制”后，在 Rust 事件返回前不会再次执行 `setCaptureMode`。
- [ ] 点击“开始录制”后，在 Rust 事件返回前不会再次执行 `setAudioConfig`。
- [ ] 点击“开始录制”后，在 Rust 事件返回前不会再次执行 `startRecording`。
- [ ] 启动失败后 UI 进入 `failed`，并且可以返回 idle 后重新开始。
- [ ] 启动成功收到 `recording-state-changed: recording` 后清理启动中状态。
- [ ] 前端测试真实连续触发两次开始点击，断言 `start_recording` 只调用 1 次。

## Phase B：前端停止录制防重入

- [ ] `src/App.tsx` 增加停止中防重入状态，例如 `isStopping` 或等价机制。
- [ ] 连续点击停止按钮不会重复调用 `stopRecording`。
- [ ] 停止成功后进入 preview，并记录 `RecordingResult`。
- [ ] 停止失败后进入 failed，并展示错误信息。
- [ ] 前端测试真实连续触发两次停止点击，断言 `stop_recording` 只调用 1 次。

## Phase C：Mic-Level Runtime 生命周期

- [ ] Rust 侧新增可停止、可 join 的 mic-level runtime，或复用现有 runtime 模式。
- [ ] `AppState` 保存 mic-level runtime handle，而不是只保存 stop flag。
- [ ] `start_recording` 启动新 mic-level runtime 前会停止并清理旧 runtime。
- [ ] `stop_recording` 会停止并 join mic-level runtime。
- [ ] runtime `Drop` 路径会兜底 stop + join。
- [ ] 快速 start/stop 后旧 mic-level 线程不会继续 emit。

## Phase D：麦克风关闭与电平重置

- [ ] 每次 `MacRecordingService::start()` 前将共享 `mic_level` 重置为 `0.0`。
- [ ] 每次 `MacRecordingService::stop()` 后将共享 `mic_level` 重置为 `0.0`。
- [ ] `capture_microphone == false` 时不启动常驻 mic-level 推送线程。
- [ ] 麦克风关闭时前端不会显示上一轮录制残留电平。
- [ ] 麦克风关闭时如需发送事件，只允许发送明确的 `level: 0.0`。
- [ ] 麦克风开启时说话电平上升，停止后归零。

## Phase E：权限测试隔离

- [ ] `permissions.rs` 将 AVFoundation 状态映射拆成纯函数。
- [ ] `permissions.rs` 将 screen preflight 布尔值映射拆成纯函数。
- [ ] 普通单测不调用 `MacPermissionProbe::recording_permissions()`。
- [ ] 单测覆盖 AV status `3 -> Granted`。
- [ ] 单测覆盖 AV status `2 -> Denied`。
- [ ] 单测覆盖 AV status `1 -> Denied`。
- [ ] 单测覆盖 AV status `0 -> NotDetermined`。
- [ ] 单测覆盖未知 AV status `-> Unknown`。
- [ ] 单测覆盖 screen preflight `true -> Granted`。
- [ ] 单测覆盖 screen preflight `false -> NotDetermined`。

## Phase F：Mic-Level 事件与序列化测试

- [ ] Rust 测试覆盖 `MicLevelPayload { level }` 序列化字段为 `level`。
- [ ] 前端测试 mock `listen('mic-level')` 并保存 callback。
- [ ] 前端测试手动触发 `mic-level` payload 后，UI 麦克风电平发生可观察变化。
- [ ] 前端测试覆盖 recording 状态退出后会调用 mic-level unlisten。
- [ ] 前端测试覆盖麦克风关闭时不会展示残留动态电平。

## Phase G：MicLevelDetector 测试清理

- [ ] `half_amplitude_returns_around_half` 测试改名，名称与断言一致。
- [ ] 删除测试注释中的临时自我修正语句。
- [ ] 保留低幅、满幅、静音、reset、窗口大小边界测试。

## 自动化验证

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过。
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` 通过。
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 无 error。
- [ ] `cargo build --manifest-path src-tauri/Cargo.toml` 通过。
- [ ] `npm run build` 通过。
- [ ] `npm test -- --run` 通过。

## 手动验证

- [ ] `npm run tauri dev` 启动成功。
- [ ] 快速点击开始/停止 10 次，UI 不进入错误状态且后端无悬挂录制。
- [ ] 麦克风关闭时开始录制，电平保持 0 或不显示动态电平。
- [ ] 麦克风开启时开始录制，说话电平明显上升。
- [ ] 停止录制后麦克风电平归零。
- [ ] 断开麦克风后开始录制，应用不崩溃。
- [ ] 权限关闭后重启应用，中文权限引导正确。
- [ ] 权限开启后返回首页，权限状态重新检测正确。

## 架构红线复核

- [ ] 前端没有接收视频帧。
- [ ] 前端没有接收音频流。
- [ ] 前端没有实现 Rust 录制状态机的替代逻辑。
- [ ] 前端没有拼接 FFmpeg 参数。
- [ ] Rust 捕获回调中没有新增阻塞等待。
- [ ] 音频/视频队列仍为有界非阻塞发送。
- [ ] 没有新增 Tauri `allow-all` 权限。
- [ ] 没有修改生产环境配置。
- [ ] 没有执行 `git push` 或发布命令。

## BUG.md 预防规则复核

- [ ] 未新增 `data-tauri-drag-region="false"` 区域级包裹。
- [ ] 所有状态视图仍保留可拖拽区域。
- [ ] 按钮、select、input 等交互元素不会触发拖拽。
- [ ] 未使用 `motion.div` 直接包裹可点击 Button 来实现按压动画。
- [ ] 若新增交互元素，已纳入拖拽排除选择器或自身语义可被识别。

## 合并门槛

- [ ] Critical 问题已修复。
- [ ] Important 问题已修复或有人工接受记录。
- [ ] 自动化验证全部通过。
- [ ] 手动验证全部通过。
- [ ] `tests/phase-3-w5-w6-checklist.md` 根据实际手动验证结果更新。
- [ ] `permissions.rs` FFI 已完成人工 Native Safety Gate 审查。

