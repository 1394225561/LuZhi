# Windows Video-Only MVP Design

> 创建日期：2026-06-19
>
> 状态：设计完成，待用户最终审阅
>
> 范围：针对 Windows MVP code review 中 `#1/#2/#5/#7/#9/#12` 的产品与架构决策

---

## 背景

`docs/superpowers/specs/2026-06-19-windows-mvp-design.md` 原目标是让 Windows 追平当前 macOS MVP 主链路，包括全屏录制、窗口录制、WASAPI 系统音频、麦克风、光标元数据和导出。

后续 code review 记录在 `docs/superpowers/reviews/2026-06-19-windows-mvp-code-review-and-fix-plan.md` 中。审查结论是：完整追平范围过大，且当前 Windows WGC/WASAPI 存在 false-success 行为，会让应用进入 Recording 但不产出真实视频或系统音频。

本设计把本轮 Windows 验收收缩为 **Video-Only MVP**：先证明 Windows 能真实录到全屏画面、停止后生成可预览素材，同时对未实现能力 fail-fast，不制造假成功。

## 已确认决策

1. Windows 本轮选择“最小可录制 MVP”，不是完整追平 macOS。
2. 音频口径为视频优先：本轮不验收 WASAPI 系统音频，不验收麦克风。
3. 光标口径为保留系统原始光标：本轮不隐藏系统光标，不启用 cursor overlay，不要求 Windows capture geometry。
4. 前端禁用不可用入口，后端仍做 fail-fast 兜底。
5. macOS 行为必须保持现状，不跟随 Windows 降级。

## 目标

Windows 本轮硬验收目标：

- Windows Tauri 应用能构建启动。
- Windows 只允许全屏、无音频、保留系统光标录制。
- 全屏 WGC 必须真实启动并产生视频帧。
- `start_recording` 返回成功前，WGC 启动必须完成握手，不能只 spawn worker 就返回成功。
- 停止后生成可预览素材，且 `duration_secs > 0`、`frame_count > 0`、`output_path != None`。
- 不可用能力在前端禁用，在后端请求时返回明确中文错误。
- 失败录制不进入正常历史库。

## 非目标

本轮明确不做：

- Windows 窗口录制。
- Windows 区域录制。
- WASAPI 系统音频录制。
- Windows 麦克风录制。
- Windows cursor overlay / 光标美化。
- Windows capture geometry / 多屏 DPI 光标映射。
- Windows HWND 类型迁移。
- CPAL Windows `SendStream` 生命周期重构。
- D3D11 texture 零拷贝或 GPU 优化。

这些能力后续应分别进入独立设计或实现计划，避免和本轮 video-only 闭环混在一起。

---

## 产品范围

### Windows 支持能力

Windows 本轮只支持：

- 捕获模式：全屏。
- 视频：Windows Graphics Capture 全屏录制。
- 音频：不录制系统音频，不录制麦克风。
- 光标：保留系统原始光标在源视频里。
- 导出/预览/历史库：复用现有录后链路，但只在录制成功时注册正常历史记录。

### Windows 禁用能力

Windows 前端应禁用：

- 窗口录制。
- 区域录制。
- 系统音频开关。
- 麦克风开关。
- 需要隐藏系统光标的 cursor overlay / 光标美化入口。

禁用文案采用短句即可：

```text
Windows MVP 暂仅支持全屏无音频录制
```

后端必须保留同等校验，不能依赖前端禁用。

### macOS 行为

macOS 不受 Windows video-only 口径影响。macOS 仍支持当前已有能力：

- 全屏录制。
- 窗口录制。
- 系统音频。
- 麦克风。
- 光标美化。
- 裁剪、预览、历史库和导出。

---

## 架构设计

### WindowsRecordingService 边界

`WindowsRecordingService::start()` 只接受满足以下条件的配置：

- `CaptureMode::FullScreen`
- `audio_config.capture_system_audio == false`
- `audio_config.capture_microphone == false`
- `beautify_snapshot.raw_system_cursor_visible == true`

任一条件不满足时，必须在启动 native capture 前返回错误并将状态转为 `Failed` 或保持可重新 start 的终态。

`WindowsRecordingService::start_window()` 本轮直接 fail-fast：

```text
Windows MVP 暂仅支持全屏录制
```

### Writer 创建顺序

writer 必须在 WGC 启动前创建。

推荐顺序：

1. 校验 Windows video-only 配置。
2. `state_machine.start()`。
3. 清理上一轮 session path。
4. 创建 writer。
5. 创建 bounded media channels。
6. 启动 WGC，并等待启动握手。
7. 启动 shared consumer。
8. 返回 `Ok(())`。

如果 writer 创建失败，不能启动 WGC，也不能留下 Recording 状态。

### WGC 启动握手

`WindowsGraphicsCapture::start_display()` 不能只 spawn worker 后立即返回成功。它必须等待 worker 报告以下结果之一：

- `Started`: WinRT/D3D/frame pool/session 已创建，捕获已启动。
- `Failed(AppError)`: WGC 不支持、初始化失败、权限/系统能力不足、D3D 初始化失败等。

只有收到 `Started` 才能返回 `Ok(())`。

握手不要求等第一帧到达，但必须证明 capture session 已经启动。第一帧到达通过手动验收和 diagnostics 证明。

### Audio handling

Windows video-only 模式不启动 `WasapiLoopback`，不启动 `CpalMicrophoneCapture`。

传给 shared consumer 的 requested flags 必须是：

```text
requested_system_audio = false
requested_microphone = false
```

`system_audio_rx` 可以是空 channel receiver，用于满足 shared consumer 输入结构；因为 requested audio flags 为 false，source-aware audio contract 不应要求音频。

`WasapiLoopback::capabilities()` 本轮不应让产品层认为系统音频可用。若仍保留模块骨架，启动时必须 fail-fast，不得返回假成功。

### Cursor handling

Windows video-only 模式保留系统原始光标：

- `config.show_system_cursor = true`
- `beautify_snapshot.raw_system_cursor_visible = true`
- 不启动 `CursorMetadataRuntime`，或只写 noop/raw-visible metadata。
- 不生成 cursor overlay 需求。

本轮不解决 Windows `CaptureGeometry`。因为 raw cursor 已经在视频里，不需要 overlay 坐标映射。

### HWND handling

本轮不迁移 `WindowInfo.window_id` 类型。理由：

- Windows window recording 不在本轮产品范围内。
- 前端禁用窗口录制。
- 后端 `start_window()` fail-fast。
- HWND 不进入生产录制链路。

HWND 类型迁移必须在后续 Windows window recording 设计里处理。

### CPAL Send safety

本轮不启用 Windows microphone capture，因此不依赖 Windows CPAL stream lifecycle。

仍需保留安全结论：

- `unsafe impl Send for SendStream` 当前不能作为 Windows 麦克风可用的依据。
- 后续启用 Windows 麦克风前，必须单独设计 CPAL WASAPI stream owner-thread 或完成平台安全审查。

---

## Error Handling

Windows 后端错误文案：

| 场景 | 错误文案 |
|------|----------|
| 请求窗口录制 | `Windows MVP 暂仅支持全屏录制` |
| 请求区域录制 | `Windows MVP 暂仅支持全屏录制` |
| 请求系统音频 | `Windows MVP 暂不支持系统音频录制，请关闭系统音频后重试` |
| 请求麦克风 | `Windows MVP 暂不支持麦克风录制，请关闭麦克风后重试` |
| 请求隐藏系统光标/光标美化 | `Windows MVP 暂不支持光标美化，请保留系统光标` |
| WGC 不支持 | `Windows 屏幕录制启动失败：当前设备不支持 Windows Graphics Capture` |
| WGC 初始化失败 | `Windows 屏幕录制启动失败：{底层原因}` |

失败规则：

- writer 创建失败：不启动 WGC，返回错误。
- WGC 握手失败：停止已创建资源，状态进入 `Failed`。
- consumer/write/finalize 失败：返回 `StopRecordingResponse { failed: true }`，保留 diagnostics 和 `finalization_errors`。
- `failed == true` 的录制不得注册为正常历史记录。

---

## Testing Design

### Rust unit tests

Windows service tests must avoid真实 native device APIs. Use mocks or injectable factories for:

- writer factory
- graphics capture starter
- consumer result path

Required tests:

- Windows rejects system audio request before starting WGC.
- Windows rejects microphone request before starting WGC.
- Windows rejects window recording via `start_window()`.
- Windows rejects raw cursor hidden / cursor overlay request.
- Writer factory failure does not start WGC and leaves service in failed/restartable state.
- WGC startup handshake failure returns error and does not start consumer.
- WGC startup handshake success starts consumer.
- Stop timeout returns failed response rather than hanging.

### Frontend tests

Required tests:

- On Windows platform capability, window/area modes are disabled or unavailable.
- On Windows platform capability, system audio and microphone toggles are disabled.
- On Windows platform capability, start request uses fullscreen, no audio, raw cursor visible.
- macOS/default behavior keeps existing controls enabled.

### Manual Windows gates

Run on a Windows 10/11 machine:

1. Start full-screen recording for 5-10 seconds.
2. Stop recording.
3. Confirm preview opens.
4. Confirm history entry is created only when `failed == false`.
5. Confirm result has `frame_count > 0`.
6. Confirm result has `duration_secs > 0`.
7. Confirm result has `output_path`.
8. Confirm output file can be opened by the preview/export path.
9. Confirm system audio/mic/window controls are disabled or fail-fast with the agreed Chinese messages.

### macOS regression gates

Before merging, run macOS verification:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib consume_frames -- --test-threads=1
npm test -- --run
npm run build
```

Manual macOS gates:

- Full-screen recording still works.
- Window recording still works.
- System audio + microphone still work.
- Cursor overlay/export still works.

---

## Relationship To Review Findings

| Finding | Decision |
|---------|----------|
| `#1` WGC false success | Must fix by implementing startup handshake and real full-screen WGC. |
| `#2` WASAPI false success | Do not implement this round; disable/fail-fast system audio. |
| `#5` writer rollback | Must fix by creating writer before WGC or adding rollback guard. |
| `#7` HWND truncation | Defer because window recording is disabled/fail-fast this round. |
| `#9` CPAL Send safety | Defer because Windows microphone is disabled/fail-fast this round. |
| `#12` capture geometry | Defer because raw system cursor remains visible and overlay is disabled. |

---

## Success Criteria

The implementation is complete when:

- Windows cannot enter Recording for unsupported mode/audio/cursor requests.
- Windows full-screen no-audio recording uses real WGC and produces frames.
- `start_recording` only succeeds after WGC startup handshake succeeds.
- Writer creation failure cannot leak WGC resources.
- Stop cannot hang indefinitely on consumer timeout.
- Successful Windows recording creates a normal history entry.
- Failed Windows recording does not create a normal history entry.
- macOS existing behavior and tests are not regressed.

## Open Follow-Up Specs

Future specs should cover:

- Windows WASAPI system audio MVP.
- Windows microphone / CPAL WASAPI lifecycle safety.
- Windows window recording and HWND-safe IDs.
- Windows cursor overlay, capture geometry, DPI, and multi-display mapping.
