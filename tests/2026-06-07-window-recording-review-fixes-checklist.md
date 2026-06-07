# 2026-06-07 窗口录制 Code Review 修复自测清单

## 自动化验证

- [x] `npm test -- src/App.test.tsx -t "preserves the selected window"`
- [x] `npm test -- src/App.test.tsx -t "uses recording result from completed state event"`
- [x] `npm test -- src/components/window-selector.test.tsx`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib consume_frames_drops_queued_media_when_stopped_while_paused`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [x] `npm test -- --run`
- [x] `npm run build`
- [x] `cargo build --manifest-path src-tauri/Cargo.toml`
- [x] `git diff --check`

## 重点人工验证

- [ ] 窗口模式选择 Safari/Terminal 等正常窗口后点击开始录制，后端不再报“未选择录制窗口”
- [ ] 录制中关闭目标窗口，录制自动停止并进入预览/历史记录
- [ ] 录制中最小化目标窗口，状态栏切换暂停；暂停期间内容不继续写入最终录制；恢复目标窗口后回到录制中
- [ ] Retina 显示器上录制窗口后构建光标效果，导出光标位置无明显偏移
- [ ] 窗口枚举失败时 WindowSelector 显示错误文案和“重试”，不会误显示为“未找到可用窗口”
- [ ] 窗口缩略图为空时显示占位预览，选择和开始录制流程不受影响

## Review 防线

- [ ] `set_capture_mode` 不会用缺失 `windowId` 的 payload 清空窗口模式已有 `window_id`
- [ ] 自动停止路径复用手动停止的 finalize/register 逻辑
- [ ] 暂停路径不只更新 UI/状态机，消费线程会排空但丢弃暂停期间媒体
- [ ] `start_window()` 中 writer 创建失败不会发生在 SCK stream/monitor/mic 已启动之后
- [ ] 窗口 capture geometry 使用 SCK `contentRect` + `pointPixelScale`
- [ ] macOS `WindowCapture` trait 实现与 capabilities 一致
