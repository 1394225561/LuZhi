# bug 备忘清单

记录已知 bug。**重要**：每条 bug 修复后，都要总结对应的**预防规则**。

## 未解决

### BUG-005: 音频捕获失败

**现象**：

- 录制前开启 `麦克风`，点击开始录制
- `录制中界面` 报错：
  - `音频捕获失败：构建麦克风输入流失败：The requested stream configuration is not supported by the device.`

---

### 延期-001: 透明区域鼠标点击不穿透

**现象**：窗口视觉透明区域的鼠标点击不会穿透到被覆盖的应用（桌面、其他窗口），点击透明区域不会激活后方应用。

**原因**：Tauri 2 的 `setIgnoreCursorEvents(true)` 是全窗口级开关，开启后整个窗口（包括面板按钮）都无法交互。无像素级点击穿透支持。

**计划方案**（后期实现）：

- 方案 A：自定义 NSWindow `hitTest:` 重写，透明像素处返回 nil（需原生 macOS 代码）
- 方案 B：主窗口忽略鼠标事件 + 独立子窗口承载控制面板
- 方案 C：使用窗口 shape mask 裁剪到面板区域

---

## 已解决

### BUG-004: 导出视频无法播放（视频 PTS 被压缩到 0.03s）

**现象**：

- 导出视频只有封面第一帧有画面，从第二帧开始黑屏
- 原 20 秒视频导出后变成 2 分多钟
- `ffprobe` 显示 export video stream duration 仅 `0.032552s`，而 audio/container 约 `19s`

**根因**（两层叠加）：

1. **exporter packet rescale 空操作**：`trim_exporter.rs` 中 `enc_pkt.rescale_ts(video_enc_tb, video_enc_tb)` 等于没有转换。`write_header()` 后 MP4 muxer 可能把 output stream time base 改为 `1/15360`，但 PTS `0,1,2...` 仍被当作 `1/30` 单位写入，实际被解释为 `1/15360` 单位 → 501 帧仅 `501/15360 ≈ 0.03s`。
2. **source writer PTS 模型与真实时间脱节**：writer 使用固定帧序号 `0,1,2...` 作为 PTS，但 `duration_secs` 来自最后一帧的真实 timestamp。当帧率不稳定或有丢帧时，source video duration 远短于 audio/container duration。

**修复**：

1. `trim_exporter.rs`：`write_header()` 后读取 muxer 真实 output stream time base，所有 encoded packet 使用 `rescale_ts(enc_tb, out_tb)` 正确转换。
2. `ffmpeg_writer.rs`：视频 PTS 改为基于真实 frame timestamp 转换到 encoder time base，保证 monotonicity。
3. `ffmpeg_common.rs`：`MediaArtifactInspection` 增加 `video_duration_nanos` / `audio_duration_nanos` 字段，validation 检查 video/audio drift。

**预防规则**：

- FFmpeg muxer 写包必须使用 `write_header()` 后的真实 output stream time base，不能假设 encoder time base 等于 muxer time base
- artifact validation 必须检查 per-stream duration（video 和 audio），不能只看 container duration
- writer 不得只用 frame count 伪造真实录制时间轴，必须处理 frame sparsity/drop
- playable export manual gate 必须包含 ffprobe stream-level duration/fps 检查

---

### BUG-006: 系统音频稀疏时间轴导致 source writer duration drift

**现象**：

- 只开启系统音频，并且系统存在音频播放时（比如打开音乐播放器播放音乐），点击开始录制
- `录制中界面` 报错：
  - `完成录制文件失败：录制写入器完成失败：写入录制文件失败：录制视频/音频时长偏差过大：视频 22600ms，音频 12821ms，偏差 9779ms`
- 终端日志如下：

```
     Running `target/debug/luzhi`
[libx264 @ 0x713711c00] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0x713711c00] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit
[aac @ 0x713712300] Qavg: 993.605
[libx264 @ 0x713711c00] frame I:3     Avg QP: 9.00  size:392678
[libx264 @ 0x713711c00] frame P:676   Avg QP: 1.82  size: 23728
[libx264 @ 0x713711c00] mb I  I16..4: 100.0%  0.0%  0.0%
[libx264 @ 0x713711c00] mb P  I16..4:  2.3%  0.0%  0.0%  P16..4: 17.8%  0.0%  0.0%  0.0%  0.0%    skip:79.8%
[libx264 @ 0x713711c00] final ratefactor: 6.86
[libx264 @ 0x713711c00] coded y,uvDC,uvAC intra: 34.7% 18.1% 17.0% inter: 7.8% 2.6% 2.4%
[libx264 @ 0x713711c00] i16 v,h,dc,p: 59% 38%  2%  2%
[libx264 @ 0x713711c00] i8c dc,h,v,p: 77% 14%  8%  1%
[libx264 @ 0x713711c00] kb/s:6085.99
录制写入器完成失败: 写入录制文件失败：录制视频/音频时长偏差过大：视频 22600ms，音频 12821ms，偏差 9779ms
```

**根因**：

系统音频捕获天然可能是稀疏的：用户开始录制后几秒才播放声音，中途暂停播放，或结束前没有声音。同步器和 mixer 已经保留 timestamp，但 writer 把 timestamp 丢了，最终 AAC stream duration 只等于"有声样本总长度"，不是"录制时间轴长度"。

**预防规则**：

- 音频输入 chunk 必须携带真实 sample_rate/channels/timestamp；统一输出格式只能在 mixer/writer timeline 层完成
- writer 必须尊重 mixed audio timestamp；对前导 gap、中间 gap、尾部 gap 写入 silence，不能把稀疏音频压缩成连续短音轨

---

### BUG-007: 导出的视频没有美化（默认美化开启时无光标）

**现象**：

- 默认美化开启（`cursor_magnification=true, cursor_smoothing=true`）时，录制会隐藏系统光标
- FFmpeg exporter 未应用 `effect_timeline_path` 绘制光标效果
- 导出视频无光标、无美化，违背 MVP "录屏 + AI 自动美化 + 一键导出" 核心目标

**根因**：

1. `build_cursor_effect_timeline()` 失败时静默降级为 `effect_timeline_path: None`
2. `FfmpegTrimExporter::export()` 从未读取 `effect_timeline_path` JSON
3. 没有 cursor overlay compositor 在导出帧上绘制光标

**修复**：

1. 新增 `cursor_overlay.rs` 模块：`CursorOverlayRenderer` 加载 EffectTimeline，映射坐标到输出帧空间（支持 FitWithBars 和 CenterCrop），在 YUV420P 帧上绘制光标（白点 + 暗边框 + 点击放大环）
2. `trim_exporter.rs`：导出时加载 effect timeline 并在每帧编码前叠加 cursor overlay
3. `lib.rs`：当美化开启（`cursor_magnification || cursor_smoothing`）时，cursor timeline 构建失败必须阻断导出，不再静默降级
4. `ffmpeg_writer.rs`：重构为 worker-backed 架构，编码在独立线程执行，bounded channel 提供背压

**预防规则**：

- 当 raw system cursor hidden 时，exporter 必须应用 cursor effect timeline，或导出必须失败，不得静默产出无光标视频
- cursor effect timeline 不能只作为 request 字段存在，exporter 必须实际读取并应用
- 默认美化开关、capture `show_system_cursor`、export compositor 三者必须作为端到端 contract 测试
- UI 不能把"可播放导出成功"文案等同于"美化导出成功"

---

### BUG-008: cursor overlay scale/radius 数值溢出导致 export panic

**现象**：

- 如果当前系统没有音频输出时，录制能成功，但是`美化界面`导出会报错：
  - `导出失败，请重试或检查录制素材。`
- 终端日志如下：

```
     Running `target/debug/luzhi`
[libx264 @ 0xab32e4700] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0xab32e4700] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit
[aac @ 0xab32e4e00] Qavg: 65536.000
[libx264 @ 0xab32e4700] frame I:2     Avg QP:17.00  size:288984
[libx264 @ 0xab32e4700] frame P:384   Avg QP: 5.91  size: 33077
[libx264 @ 0xab32e4700] mb I  I16..4: 100.0%  0.0%  0.0%
[libx264 @ 0xab32e4700] mb P  I16..4:  4.2%  0.0%  0.0%  P16..4: 17.9%  0.0%  0.0%  0.0%  0.0%    skip:77.8%
[libx264 @ 0xab32e4700] final ratefactor: 12.31
[libx264 @ 0xab32e4700] coded y,uvDC,uvAC intra: 40.9% 17.2% 14.0% inter: 9.2% 2.6% 2.2%
[libx264 @ 0xab32e4700] i16 v,h,dc,p: 52% 43%  3%  2%
[libx264 @ 0xab32e4700] i8c dc,h,v,p: 78% 14%  7%  1%
[libx264 @ 0xab32e4700] kb/s:7811.50
[libx264 @ 0xab32e6680] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0xab32e6680] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit

thread 'tokio-rt-worker' (7976776) panicked at src/media/cursor_overlay.rs:263:30:
attempt to multiply with overflow
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
[aac @ 0xab32e6300] Qavg: nan
[aac @ 0xab32e6300] 1 frames left in the queue on closing
[libx264 @ 0xab32e6680] final ratefactor: 20.64
```

**根因**：

cursor effect timeline 来自录制时的外部输入和动画计算，不应被视为可信数值。`scale` 可能异常大、非有限值、或由错误时间轴插值得到极端值。当前 rasterizer 用 `i32` 做平方，debug 构建会 panic，release 构建则可能溢出后产生错误绘制。

**预防规则**：

- cursor/effect timeline 来自外部输入，所有 `x/y/scale/timestamp` 参与 rasterization 前必须 finite check 和范围 clamp
- overlay 距离计算不得依赖 debug/release 不同行为；平方和半径计算必须使用足够宽的整数类型或 saturating arithmetic

---

### BUG-001: 录制面板不是浮动小组件，有800x600背景方框

**现象**：主录制控制面板应为浮动小组件，但显示时有一个800x600的不透明背景方框。修复 body CSS 后背景从深色变为白色，但方框仍然存在。

**根因**：Tauri 透明窗口需要**三层独立**都透明，缺一不可：

| 层        | 机制                        | 问题                                                 |
| --------- | --------------------------- | ---------------------------------------------------- |
| NSWindow  | `"transparent": true`       | ✅ 已配置                                            |
| WKWebView | `macos-private-api` feature | ❌ 缺失：macOS WKWebView 独立于 CSS 绘制默认白色背景 |
| CSS       | html/body 无背景色          | ❌ body `bg-background` (#040506) + html 未设透明    |

第一轮修复只解决了 CSS 层 body 的问题，但 WKWebView 的白色背景随即暴露。之前 body `#040506` 覆盖了 WKWebView 的白色，使人误以为只有 CSS 问题。

**修复**：

1. `Cargo.toml`：tauri 添加 `macos-private-api` feature → 通过 KVC 禁用 WKWebView 的 `drawsBackground`
2. `styles.css`：body 移除 `bg-background` + html 添加 `background-color: transparent`
3. 需要全屏背景的视图（processing、error、preview）已在各自根 div 显式设置 `bg-background`

**预防规则**：

- Tauri macOS 透明窗口 = NSWindow + WKWebView + CSS，三层缺一不可
- 排查透明窗口问题必须逐层验证，不能只看 CSS
- `macos-private-api` feature 是 macOS 平台上实现真正透明 WKWebView 的必要条件

---

### BUG-002: 窗口无法拖动

**现象**：所有状态下窗口都无法通过鼠标拖动移动。

**根因**（多层叠加，逐层剥离后才暴露下一层）：

| 层             | 问题                                                                | 详情                                                                                                                                                                                        |
| -------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| ACL 权限       | `capabilities/default.json` 缺少 `core:window:allow-start-dragging` | Tauri 2 的 drag.js 在 mousedown 时通过 IPC 调用 Rust 命令 `plugin:window                                                                                                                    | start_dragging`，没有此权限 IPC 被静默拒绝。**这是拖拽完全不工作的致命原因** |
| 拖拽区域标记   | bare `data-tauri-drag-region` 渲染为 `"true"`                       | React 将裸属性渲染为 `"true"`，Tauri drag.js 对 `"true"` 的处理是仅元素自身直接点击触发（`el === composedPath[0]`），子元素区域不触发。面板覆盖大部分区域，实际可拖拽区只剩 `p-8` 32px 窄边 |
| macOS API 限制 | `performWindowDragWithEvent:` 需窗口为焦点窗口                      | 即使权限和属性都正确，底层 macOS API 在窗口未获焦点时静默失败（tauri#11605）。声明式机制对浮动面板窗口不可靠                                                                                |
| 排除逻辑过严   | `{false}` 排除 + `attr === 'false'` 检查双重拦截                    | 面板包裹在 `{false}` div 中，`closest('[data-tauri-drag-region]')` 最先匹配到 `{false}` 父元素，handler 拒绝拖拽。面板内容区（非按钮空白处）完全无法拖拽                                    |

**修复**（迭代 4 轮，最终方案：程序化拖拽替代声明式）：

1. **`src-tauri/capabilities/default.json`**：添加 `core:window:allow-start-dragging` 权限 — 解禁 IPC 调用
2. **`src-tauri/tauri.conf.json`**：添加 `"acceptFirstMouse": true` — macOS 首次点击可同时聚焦+拖拽
3. **`src/App.tsx`**：实现程序化拖拽 handler，直接调用 `getCurrentWindow().startDragging()`，绕过不可靠的声明式 drag.js
   - 添加 `data-tauri-drag-region="deep"` 到所有外层容器（idle、recording、preview、processing、error）
   - 拖拽 handler 逻辑：
     - **允许拖拽**：点击在 `[data-tauri-drag-region]` 区域内且不是交互元素
     - **阻止拖拽**：点击目标为 `button, input, select, textarea, a` 或 `role` 为 `button/link/menuitem/tab/checkbox/radio/slider/switch` 或 `contenteditable` / 非 `-1` 的 `tabindex`
   - 使用动态 `import('@tauri-apps/api/window')` 避免测试环境顶层导入报错
   - **不检查 `{false}` 值** — 交互元素检查已足够，`{false}` 标记仅保留供 Tauri 内置 drag.js 作 fallback

**预防规则**：

- **Tauri 2 窗口拖拽必须授予 ACL 权限**：`core:window:allow-start-dragging` 不在 `core:default` 范围内，必须显式添加
- **声明式 `data-tauri-drag-region` 不可靠**：受 macOS 焦点窗口限制，对浮动面板类 app 应优先使用程序化 `startDragging()` API
- **不要用 `{false}` 做区域级拖拽排除**：用交互元素选择器（`closest('button, input, ...')`）精确排除，而非用 `{false}` 阻止整个面板区域。`{false}` 应该是交互元素自身的标记，不是容器的标记
- **每个状态视图都需要拖拽区域**：包括 processing、error 等过渡状态
- **交互元素白名单要覆盖完整**：button, input, select, textarea, a, [role="button"], [role="slider"], [role="switch"], [contenteditable], [tabindex] 等

---

### BUG-003: 点击"开始录制"无法切换到录制状态

**现象**：点击"开始录制"按钮后，界面没有切换到录制状态栏（迷你播放器）。

**根因**：Button 被包裹在 `motion.div` 的 `whileTap={{ scale: 0.99 }}` 中。framer-motion 的 whileTap 会捕获指针事件来实现缩放动画，阻止了点击事件到达内部的 Button 元素。

**修复**：移除 Button 外层的 `motion.div` 包裹，改用 CSS `active:scale-[0.99]` 实现按压反馈效果。

**预防规则**：

- framer-motion 的 `whileTap` 会拦截指针事件，不要将其作为可交互元素（Button、Link 等）的直接父容器。
- 如需在可交互元素上添加按压动画，优先使用 CSS `active:` 伪类，或将 `whileTap` 直接放在元素本身上（如 `motion.button`）。

---
