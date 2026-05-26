# Phase 3 / W5-W6 自测清单：录制 UI 与前后端联调

> 最后更新：2026-05-26 | 自动化验证已完成，手动验证待执行

## 目标

验证中文录制流程完整可用，前端仅作为展示和命令层，录制状态机仍由 Rust 统一维护。

## UI 验证

- [x] 全屏录制入口显示中文文案。（自动化：14 tests 覆盖全部中文文案渲染）
- [x] 窗口录制入口显示中文文案。
- [x] 区域录制入口显示中文文案。
- [x] 分辨率和帧率参数可选择。（自动化：renders resolution and fps selectors 测试）
- [x] 系统音频和麦克风开关可选择。（自动化：renders audio settings 测试）
- [x] 权限缺失时显示中文引导。（自动化：denied + notDetermined 权限提示测试）
- [ ] 录制中显示时长和当前状态。（需手动：`npm run tauri dev` 验证 timer 显示）
- [ ] 停止录制后进入预览或完成状态。（需手动：`npm run tauri dev` 验证）

## 前后端联调

- [x] 前端使用 Tauri invoke 发起开始录制。（自动化：sends set_capture_mode... 测试验证 3 个 invoke 调用顺序）
- [x] 前端使用 Tauri invoke 发起停止录制。（自动化：displays recording result after stop 测试）
- [x] Rust 通过 event 下发状态变化。（自动化：recording-state-changed 事件监听测试）
- [x] Rust 通过 event 下发错误和进度。（自动化：错误状态流转测试）
- [x] UI 状态完全来自 Rust 下发事件。（自动化：App.tsx 仅使用 recording-state-changed 事件驱动状态）

## 架构红线

- [x] 前端未接收视频帧。（人工审查：App.tsx/lib/tauri.ts 无视频帧流数据）
- [x] 前端未接收音频流。（人工审查：App.tsx/lib/tauri.ts 无音频流数据）
- [x] 前端未实现录制状态机。（人工审查：AppState 为 'idle'|'recording'|... 展示状态，RecordingStateMachine 在 Rust 侧）
- [x] 前端未直接拼接 FFmpeg 参数。（人工审查：lib/tauri.ts 无 FFmpeg 参数调用）

## 稳定性验证

- [x] 快速点击开始/停止不会导致状态错乱。（代码加固：handleStartRecording 检查 appState !== 'idle' 直接 return）
- [x] 录制失败后可恢复到可再次录制状态。（handleBackToIdle + handleRetry 重置全部状态）
- [ ] 设备不可用时不崩溃。（需手动：断开麦克风后开始录制）
- [x] 权限变更后重新检测状态正确。（handleBackToIdle 中调用 fetchRecordingPermissions 重新检测）

## 测试要求

- [x] React 录制控制组件有 Vitest 测试。（14 个测试，覆盖 idling/recording/failed 全状态）
- [x] Tauri `invoke` 已 mock。（vi.mock 覆盖 @tauri-apps/api/core + @tauri-apps/api/event）
- [x] 错误提示组件有测试。（error view + notDetermined 权限引导测试）
- [x] 权限提示组件有测试。（denied 权限 + notDetermined 权限两类提示均有测试覆盖）

## 人工审查

- [x] 应用界面文字均为中文。（代码审查：全部 UI 文案为中文硬编码，无英文占位）
- [x] 没有引入 Antd/Arco 等重型 UI 库。（package.json 审查：仅有 React 19 + Tailwind 4 + shadcn/ui 轻量组件）
- [x] 没有将核心业务逻辑放入 React 组件。（代码审查：状态机、录制逻辑、权限检测、FFmpeg 均在 Rust 侧）

## 手动验证（待执行）

1. `npm run tauri dev` 启动应用
2. 验证空闲界面：全屏/窗口/区域模式切换正常，分辨率/FPS 下拉可用，窗口/区域模式显示"即将推出"
3. 验证权限引导：在系统设置中关闭权限后重启，确认中文引导文案正确
4. 验证录制流程：选择全屏→开始录制→录制中查看计时更新→停止录制→进入预览
5. 快速点击开始/停止 10 次，确认无崩溃
6. 断开音频设备后录制，确认不崩溃
7. 从错误状态返回首页，确认权限已重新检测
