# MVP 未完成功能规格说明

> 生成日期：2026-06-07
> 审查范围：PRD v1.0 定义的 13 个 Must Have 功能
> 状态：2 个未实现，2 个部分完成

---

## 概述

根据 PRD `docs/PRD/LuZhi_PRD_final_version.md` 和架构文档 `docs/architecture/project-architecture-and-overall-planning.md`，对当前源码进行 MVP 功能完整性审查后，以下功能尚未完全实现。

| 功能 | 状态 | 优先级 | 预计里程碑 |
|------|------|--------|-----------|
| 窗口录制 | ❌ 未实现 | P1 | W3-4 |
| 区域录制 | ❌ 未实现 | P1 | W3-4 |
| 系统音频（Windows） | ⚠️ 部分完成 | P1 | W3-4 |
| 激活码授权 | ⚠️ 部分完成 | P2 | W11-12 |

---

## 1. 窗口录制

### 当前状态

- **UI 入口**：`src/components/recording-panel.tsx:86` 有"窗口"按钮，但 `disabled` 并显示"该模式正在开发中"
- **后端枚举**：`src-tauri/src/core/config.rs:6` 定义 `CaptureMode::Window`，注释标记"开发中, 暂不可用"
- **Tauri 命令**：`src-tauri/src/lib.rs:170-172` 对非全屏模式返回错误

### 待完成项

- [ ] macOS：实现 `SCShareableContent` 窗口枚举和选择
- [ ] macOS：实现窗口 ID 到 SCStream 的绑定
- [ ] Windows：实现 DXGI Desktop Duplication 窗口模式
- [ ] UI：实现窗口选择器组件（列表/搜索/预览）
- [ ] UI：实现窗口录制模式下的参数配置
- [ ] 测试：窗口切换、最小化、关闭等边界场景

### 技术参考

- ScreenCaptureKit 窗口选择：`SCShareableContent.excludingDesktopWindows`
- 窗口捕获需要处理窗口 ID 动态变化、多显示器、窗口遮挡等场景

---

## 2. 区域录制

### 当前状态

- **UI 入口**：`src/components/recording-panel.tsx:87` 有"区域"按钮，`disabled` 并显示"即将推出"
- **后端枚举**：`src-tauri/src/core/config.rs:8` 定义 `CaptureMode::Area`，注释标记"开发中, 暂不可用"
- **Tauri 命令**：与窗口录制相同，启动时返回错误

### 待完成项

- [ ] UI：实现区域选择器（拖拽矩形、坐标显示、最小尺寸限制 320x240）
- [ ] UI：实现区域调整手柄（拖拽边框调整大小）
- [ ] UI：实现区域预览叠加层（半透明遮罩 + 选区高亮）
- [ ] macOS：实现 SCStream 的 `sourceRect` 裁剪配置
- [ ] Windows：实现 DXGI 的区域捕获
- [ ] 持久化：保存用户上次选择的区域坐标
- [ ] 测试：多显示器、DPI 缩放、区域越界等场景

### 技术参考

- ScreenCaptureKit 区域捕获：`SCStreamConfiguration.sourceRect`
- 最小尺寸限制：PRD 要求 320x240

---

## 3. 系统音频录制（Windows）

### 当前状态

- **macOS**：✅ 完整实现，SCStream 同时输出视频帧和系统音频
- **Windows**：❌ 占位实现
  - `src-tauri/src/platform/windows/wasapi_loopback.rs` 全部返回 `NativeCaptureUnavailable`
- **UI**：系统音频开关存在（`recording-panel.tsx:181-194`），但 Windows 下无效

### 待完成项

- [ ] 实现 WASAPI loopback 模式音频捕获
- [ ] 实现 `wasapi-rs` crate 集成
- [ ] 处理不同音频设备的兼容性（Realtek、Conexant 等）
- [ ] 实现音频格式转换（WASAPI float → FFmpeg format）
- [ ] 实现系统音频与麦克风混音（Windows 版 `audio_mixer.rs`）
- [ ] 降级方案：仅麦克风模式（当 WASAPI 不可用时）
- [ ] 测试：主流声卡驱动、蓝牙音频、USB 音频设备

### 技术参考

- WASAPI loopback：`IAudioClient` + `AUDCLNT_STREAMFLAGS_LOOPBACK`
- 参考 crate：`wasapi-rs`、`cpal`（Windows 后端）

---

## 4. 激活码授权

### 当前状态

- **UI 入口**：`src/lib/tauri.ts:296-298` 定义 `activateLicense` 函数
- **Tauri 命令**：`src-tauri/src/lib.rs:817-822` `activate_license` 命令存在，但返回错误"服务端激活协议未接入"
- **后端**：`src-tauri/src/app/license_service.rs:99-108` `NoopActivationCredentialStore` 占位实现，始终返回 `None`

### 待完成项

- [ ] 设计激活码格式（推荐：`XXXX-XXXX-XXXX-XXXX`）
- [ ] 实现激活码生成算法（离线可验证）
- [ ] 实现激活码验证逻辑（签名验证、过期检查、设备绑定）
- [ ] 实现激活凭证持久化（OS Keychain 存储）
- [ ] 实现激活状态 UI（输入框、验证中状态、成功/失败反馈）
- [ ] 实现多设备授权策略（单设备/多设备）
- [ ] 服务端：激活码发放接口（可选，MVP 可先用离线方案）
- [ ] 测试：无效码、过期码、已使用码、网络异常等场景

### 技术参考

- 安全要求：`src-tauri/../.claude/rules/4-security.md` 禁止硬编码私钥
- 存储要求：使用 OS Keychain（macOS Keychain / Windows Credential Manager）
- 激活码方案参考：Ed25519 签名 + 服务端公钥验证

---

## Windows 平台整体状态

所有上述功能在 Windows 平台均未实现，原因：

1. `src-tauri/src/lib.rs:41` 有 `compile_error!` 保护，阻止非 macOS 构建
2. DXGI/WASAPI 模块全部为占位实现
3. WindowsRecordingService 尚未接入

### Windows 实现路径

```
1. 移除 compile_error! 保护
2. 实现 WindowsRecordingService（Trait 实现）
3. 实现 DXGI Desktop Duplication（视频捕获）
4. 实现 WASAPI loopback（音频捕获）
5. 实现 Windows 平台的光标元采集
6. 集成测试和性能优化
```

---

## 相关文档

- PRD：`docs/PRD/LuZhi_PRD_final_version.md`
- 架构：`docs/architecture/project-architecture-and-overall-planning.md`
- Bug 记录：`BUG.md`
- 项目进度：`HANDOFF.md`
