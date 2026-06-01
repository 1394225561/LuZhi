# macOS 兼容性要求

> 最后更新：2026-06-01

## 1. 最低系统版本

| 依赖 | 最低版本 | 说明 |
|------|---------|------|
| Tauri 2.0 | macOS 10.15 (Catalina) | 框架最低要求 |
| ScreenCaptureKit | macOS 12.3 (Monterey) | 录屏 API，**实际最低版本** |
| CGPreflightScreenCaptureAccess | macOS 10.15 (Catalina) | 屏幕录制权限预检 |
| AVFoundation (麦克风权限查询) | macOS 10.14 (Mojave) | `AVCaptureDevice` 授权状态查询 |

**结论：本软件最低运行版本为 macOS 12.3 (Monterey)**，因为 ScreenCaptureKit 是录屏的核心依赖，无法降级。

---

## 2. 系统权限

本软件运行时需要以下 macOS 系统权限：

| 权限 | 用途 | 首次触发时机 |
|------|------|-------------|
| **屏幕录制** (Screen Recording) | ScreenCaptureKit 捕获屏幕画面 | 首次点击"开始录制" |
| **麦克风** (Microphone) | cpal 采集麦克风音频 | 首次开启麦克风录制 |

### 权限行为

- **无屏幕录制权限**：软件不会崩溃，会引导用户到系统设置中授权。
- **无麦克风权限**：仅系统音频录制不受影响；开启麦克风时会提示授权。
- **权限可随时在"系统设置 → 隐私与安全性"中修改。**

---

## 3. 麦克风设备兼容性

### 3.1 采集架构

```
任意麦克风设备 (任意采样率/通道数)
        ↓ cpal 采集（使用设备 default_input_config）
AudioChunk { sample_rate: 原始值, channels: 原始值 }
        ↓ AudioMixer
resample() → 重采样到 48kHz
to_stereo() → 转为 2ch 交织格式
        ↓
MixedAudioChunk { sample_rate: 48000, channels: 2 }
        ↓ FFmpeg AAC 编码器
```

### 3.2 支持的设备规格

| 设备类型 | 典型采样率 | 通道数 | 处理方式 |
|---------|-----------|--------|---------|
| 蓝牙耳机 (HFP 模式) | 8000–24000 Hz | 1ch (mono) | 重采样 → 48kHz，mono → stereo |
| 内置麦克风 | 44100 Hz | 1ch (mono) | 重采样 → 48kHz，mono → stereo |
| USB 麦克风 | 48000 Hz | 1–2ch | 采样率直通，mono → stereo |
| 专业声卡 | 96000–192000 Hz | 2ch+ | 重采样 → 48kHz，截取前 2ch |
| 多声道设备 (4-8ch) | 任意 | 4–8ch | 重采样 → 48kHz，截取前 2ch |

### 3.3 重要说明

- **软件不依赖设备支持 48kHz/2ch**。无论设备报告什么采样率和通道数，AudioMixer 都会统一转换。
- **配置协商日志**：终端会输出 `麦克风配置协商: 请求 48000Hz/2ch, 设备实际 XXXXHz/Xch`，这是诊断信息，不代表不兼容。
- **重采样质量**：录制阶段使用线性插值重采样（实时性优先），导出阶段使用 FFmpeg swresample（质量优先）。

---

## 4. 录制分辨率与帧率

| 配置 | 支持情况 |
|------|---------|
| 1080p (1920×1080) | ✅ MVP 主验收分辨率 |
| 4K (3840×2160) | ⚠️ 架构预留，MVP 不作为硬验收 |
| 非标准分辨率 | ✅ ScreenCaptureKit 支持任意分辨率，writer 内部缩放到 1080p |
| 30 fps | ✅ 当前固定帧率 |
| 60 fps | ❌ MVP 不支持，后续版本考虑 |

---

## 5. FFmpeg 依赖

| 项目 | 说明 |
|------|------|
| 绑定方式 | `ffmpeg-next` (Rust C API binding)，不调用 CLI |
| Feature gate | `ffmpeg` Cargo feature，默认不启用 |
| 开发环境 | 需安装 FFmpeg dev libraries (`pkg-config` 检测) |
| 运行时 | 静态链接，用户无需安装 FFmpeg |

### 开发环境安装

```bash
# macOS (Homebrew)
brew install ffmpeg pkg-config
```

---

## 6. 已知平台差异

| 项目 | macOS | Windows (计划中) |
|------|-------|-----------------|
| 录屏引擎 | ScreenCaptureKit | DXGI + WASAPI |
| 麦克风采集 | cpal (CoreAudio) | cpal (WASAPI) |
| 权限模型 | 系统弹窗授权 | UAC 提权 |
| 透明窗口 | NSWindow + WKWebView + CSS | 待实现 |
| 拖拽 | 程序化 `startDragging()` | 待实现 |
| 硬件编解码 | VideoToolbox (预留) | DXVA2 (预留) |

---

## 7. Tauri 配置相关

| 配置项 | 值 | 说明 |
|-------|---|------|
| `macOSPrivateApi` | `true` | 启用 WKWebView 透明背景 |
| `acceptFirstMouse` | `true` | 首次点击同时聚焦+拖拽 |
| `transparent` | `true` | NSWindow 透明 |
| `decorations` | `false` | 无标题栏 |
| CSP `media-src` | `self asset:` | 允许 `<video>` 加载本地文件 |
| Asset Protocol scope | `$TEMP/luzhi-recordings/**` | 限制文件访问范围 |
