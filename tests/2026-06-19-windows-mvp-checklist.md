# Windows MVP 手动验收清单

## 环境

- [ ] Windows 10/11 测试机
- [ ] WebView2 Runtime 已安装
- [ ] Rust stable-msvc 活跃
- [ ] Visual Studio Build Tools C++ 工作负载已安装
- [ ] FFmpeg 开发库已配置（如测试 `--features ffmpeg`）

## 构建

- [ ] `npm test -- --run` 通过
- [ ] `npm run build` 通过
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` 通过
- [ ] `npm run tauri:dev` 启动应用

## 全屏录制

- [ ] 1080p/30fps 全屏录制启动
- [ ] 停止后进入预览
- [ ] 录制出现在历史记录中
- [ ] 开启系统音频录制包含可听系统音频
- [ ] 开启麦克风录制包含可听麦克风音频
- [ ] 同时开启系统音频 + 麦克风无明显 A/V 漂移

## 窗口录制

- [ ] 窗口选择器列出记事本或终端
- [ ] 最小化窗口被禁用或返回中文错误
- [ ] 选中窗口通过 Windows Graphics Capture 录制
- [ ] 关闭目标窗口自动停止或明显失败
- [ ] 受保护/不可用窗口不生成假成功录制

## 光标和导出

- [ ] 预览/导出中光标位置与视频对齐
- [ ] 启用时导出视频中出现点击效果
- [ ] Bilibili / YouTube 16:9 导出可打开播放
- [ ] 抖音 9:16 导出可打开播放
- [ ] 小红书 1:1 导出可打开播放

## 失败模式

- [ ] WASAPI 不可用路径说明可以禁用系统音频
- [ ] 请求麦克风不可用路径明显失败
- [ ] 失败录制不创建假历史记录项

## 状态说明

| 项目 | 状态 | 备注 |
|------|------|------|
| 全屏录制 (WGC) | BLOCKED | WGC 内部 worker 需在 Windows 设备上实测 |
| WASAPI 系统音频 | BLOCKED | WASAPI 内部 worker 需在 Windows 设备上实测 |
| cpal 麦克风 | 待验证 | cpal 跨平台，理论上可直接工作 |
| 窗口枚举 | 待验证 | EnumWindows 已实现，需实测 |
| 窗口录制 (WGC) | BLOCKED | WGC 窗口捕获需在 Windows 设备上实测 |
| 导出 (FFmpeg) | 待验证 | 依赖 FFmpeg 开发库配置 |
