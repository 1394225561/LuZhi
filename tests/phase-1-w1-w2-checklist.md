# Phase 1 / W1-W2 自测清单：脚手架与 macOS 录制闭环

## 目标

验证 Tauri 2.0 + React + Rust 工程骨架可运行，macOS ScreenCaptureKit 录制主链路可启动、停止并产出可播放文件。

## 环境检查

- [ ] macOS 测试机满足项目最低系统版本要求。
- [ ] 已授予 Screen Recording 权限。
- [ ] 已授予 Microphone 权限，或录制时能给出明确中文提示。
- [ ] `.env` 或敏感配置未提交到仓库。
- [ ] Tauri 权限未开启 `allow-all`。

## 功能验证

- [ ] React UI 可启动，并显示中文录制入口。
- [ ] 点击“开始录制”后，Rust 状态机进入录制中状态。
- [ ] 点击“停止录制”后，Rust 状态机进入处理中或已完成状态。
- [ ] 录制结束后产出可播放视频文件。
- [ ] 录制过程中 UI 能显示录制状态和时长。
- [ ] 权限缺失时应用不崩溃，并显示中文引导。

## 架构验证

- [ ] 视频帧未传入前端 JS 层。
- [ ] 前端只通过 Tauri invoke/event 收发轻量命令和状态。
- [ ] ScreenCaptureKit 相关代码隔离在 `platform/macos` 边界内。
- [ ] App Logic 只依赖 Trait，不直接依赖平台 API 类型。

## 性能与稳定性

- [ ] 1080p 录制 3 分钟无明显卡顿。
- [ ] 停止录制后捕获资源完整释放。
- [ ] 重复开始/停止 5 次无崩溃。
- [ ] 捕获错误能转为应用级错误事件。

## 测试要求

- [ ] `RecordingStateMachine` 有状态迁移单元测试。
- [ ] 权限检测逻辑有可 mock 的测试入口。
- [ ] Mock `ScreenCapture` 可驱动录制服务测试。

## 人工审查

- [ ] ScreenCaptureKit 回调、线程与资源释放路径已人工审查。
- [ ] 没有新增不必要的 `unwrap()` 或 `expect()`。
- [ ] 没有引入 PRD 禁止的功能。
