# BUG-0010 / BUG-0011 第二轮 Code Review、根因定位与整改方案

> 日期：2026-06-04
> 范围：第一轮 BUG 修复提交 `bd9a6d3a593fceba4224e1109e2a6b4ec2b28fac..1edc6292dfe0412e4e1a6b3444ff9ae1041a23d6`
> 关联人工验证：`BUG.md` 中 `BUG-0010_1`、`BUG-0011_1`、`BUG-0012_1`
> 结论：BUG-0010 / BUG-0011 仍未达到验收；BUG-0012 人工验证通过，但仍有一个 terminal error UI 边界建议整改。

## 1. 本轮输入与审查目标

### 1.1 输入文件

- `tests/2026-06-04-bug-0010-0011-second-round-checklist.md`
- 第一轮修复 diff：`bd9a6d3a593fceba4224e1109e2a6b4ec2b28fac..1edc6292dfe0412e4e1a6b3444ff9ae1041a23d6`

### 1.2 人工验证反馈

BUG-0010 第一轮人工验证：

- 现象：导出视频光标依旧产生很大偏移。
- 新偏移方向：垂直向上。
- 偏移幅度：超过半个屏幕高度。

BUG-0011 第一轮人工验证：

- 现象：任何情况下都是白底黑边箭头。
- 仍未满足：
  - 普通桌面：黑底白边箭头。
  - 悬停按钮/链接：白底黑边手形。
  - 悬停文本框：黑底白边 I-beam。

BUG-0012 第一轮人工验证：

- 人工验证通过。

### 1.3 本轮审查目标

1. 对第一轮修复做细致 code review。
2. 定位 BUG-0010 / BUG-0011 仍失败的现有根因。
3. 给出下一轮可执行的修复方案和验证门禁。
4. 检查代码是否遵循 `BUG.md` 中相关预防规则。

## 2. 总体结论

第一轮修复把正确的架构骨架搭起来了，但两个核心行为没有闭环：

1. BUG-0010：坐标归一化的位置放对了，但 Y 轴方向未经实测确认就被硬编码翻转，导致新的大幅垂直偏移。
2. BUG-0011：`CursorKind` 类型和渲染通路已经存在，但采样侧从未识别真实 kind，所有样本仍固定为 `Arrow`；同时 Arrow / IBeam glyph 颜色与验收相反。
3. BUG-0012：进度粒度实现基本符合目标，人工验证通过；但前端 terminal error event 的显示路径仍有一个边界问题。

合并评估：

- **Ready to merge? No**
- 原因：BUG-0010 / BUG-0011 仍失败核心验收，且现有自动测试没有覆盖失败场景。

## 3. Code Review Findings

### 3.1 Critical 1: BUG-0010 Y 轴被无条件翻转

位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:59`
- `src-tauri/src/app/cursor_metadata_runtime.rs:70-77`

当前实现：

```rust
flip_y: true, // CGEventGetLocation Y is bottom-up, video is top-down
```

```rust
let local_y = if self.flip_y {
    self.content_height - local_y_raw
} else {
    local_y_raw
};
```

问题：

- 原计划中已经明确写过：`CGEventGetLocation()` 与 ScreenCaptureKit display/content rect 的 Y 轴方向必须实测确认。
- 第一轮代码没有保留“是否 flip”的可配置语义，而是直接默认 `flip_y: true`。
- 人工验证反馈是“垂直向上偏移超过半屏”，这正好符合错误 Y flip 的数学特征。

数学例子：

假设 source video 高度为 1080，真实 cursor 在下半屏 `y = 900`：

```text
当前公式：
source_y = 1080 - 900 = 180

结果：
真实位置在 900，overlay 画到 180，向上偏移 720px。
```

为什么测试没发现：

- 现有 mapper 测试主要使用中心点，例如 `y = 540`。
- 中心点翻转后仍然是 540，因此错误实现也能通过测试。
- 缺少 top-left、bottom-right、非零 origin、负 origin、Retina 四角测试。

影响：

- 直接违反 `BUG.md` 中 BUG-0010 预防规则：
  - cursor metadata 必须与 source video 使用一致坐标空间。
  - 多显示器、Retina scale、CenterCrop/FitWithBars 必须有坐标回归测试或人工门禁。

修复方向：

1. 先新增失败测试，不允许直接改公式。
2. 将 Y 轴方向变为显式语义，例如：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CursorYAxis {
    Down,
    Up,
}
```

3. 默认不应无条件 flip。应以真实设备验证或统一坐标源为准。
4. 建议 `CaptureGeometry` 使用和 `CGEventGetLocation()` 一致的 global display 坐标来源。
5. 修复后至少覆盖：
   - top-left。
   - bottom-right。
   - 非零 origin。
   - 负 origin 外接屏。
   - Retina point-to-pixel scale。

外部参考：

- Apple Quartz Display Services global display coordinate system 文档：`https://developer.apple.com/library/archive/documentation/GraphicsImaging/Conceptual/QuartzDisplayServicesConceptual/Articles/Overview.html`
- 该文档只能作为坐标系交叉参考，最终仍必须用真实设备日志确认 `CGEventGetLocation()` 与所选 display geometry 是否同一坐标空间。

### 3.2 Critical 2: BUG-0011 所有样本仍固定写入 Arrow

位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:21-27`
- `src-tauri/src/app/cursor_metadata_runtime.rs:165`
- `src-tauri/src/platform/macos/cursor_source.rs:68-83`

当前 `CursorSnapshot`：

```rust
pub struct CursorSnapshot {
    pub x: f32,
    pub y: f32,
    pub left_down: bool,
    pub right_down: bool,
    pub middle_down: bool,
}
```

当前 sample 写入：

```rust
self.samples.push_back(CursorSample {
    timestamp,
    x: norm_x,
    y: norm_y,
    kind: CursorKind::Arrow,
});
```

问题：

- 数据源没有任何 target / system cursor / Accessibility role 信息。
- `record_snapshot()` 固定写 `CursorKind::Arrow`。
- `CursorEffectEngine` 虽然会保留 kind，但输入永远是 Arrow。
- `CursorOverlayRenderer` 虽然能按 kind 选择 glyph，但运行时永远拿不到 Hand / IBeam。

为什么人工验证“任何情况下都是箭头”：

```text
MacCursorSource.snapshot()
  -> CursorSnapshot(x/y/buttons only)
  -> CursorMetadataRecorder.record_snapshot()
  -> kind: CursorKind::Arrow
  -> CursorEffectEngine preserves Arrow
  -> CursorOverlayRenderer draws Arrow
```

这不是偶发，也不是识别失败 fallback，而是当前实现没有识别逻辑。

影响：

- 直接违反 BUG-0011 期望：
  - 可点击区域 Hand。
  - 输入框 IBeam。
- 也没有真正满足 `BUG.md` 预防规则：
  - cursor timeline 必须携带稳定 cursor kind 或 glyph 信息。
  - target-aware cursor 识别失败时 fallback Arrow。

修复方向：

1. 扩展 `CursorSnapshot`：

```rust
pub struct CursorSnapshot {
    pub x: f32,
    pub y: f32,
    pub left_down: bool,
    pub right_down: bool,
    pub middle_down: bool,
    pub kind: CursorKind,
}
```

2. `CursorMetadataRecorder` 使用 `snapshot.kind`：

```rust
kind: snapshot.kind,
```

3. macOS 侧增加 kind provider。
4. 查询失败 fallback `Arrow`，并记录诊断计数。
5. target-aware 查询不得放在 ScreenCaptureKit callback 中。

候选实现路径：

```text
CursorMetadataRuntime 后台线程
  -> MacCursorSource.snapshot()
  -> 读取 CGEvent 位置和 buttons
  -> 低频查询 CursorKindProvider
  -> CursorSnapshot { x, y, buttons, kind }
```

macOS target-aware 可选策略：

1. Accessibility hit-test：
   - `AXTextField` / `AXTextArea` / editable role -> `IBeam`
   - `AXButton` / `AXLink` / `AXMenuItem` / supports press action -> `Hand`
   - 其他或失败 -> `Arrow`
2. 当前系统 cursor 查询：
   - 如果采用 AppKit `NSCursor` 相关 API，必须确认跨应用 hover 是否稳定。
3. 两者组合：
   - 优先 Accessibility role。
   - 无权限或失败时 fallback Arrow。

外部参考：

- Apple Accessibility hit-test API：`https://developer.apple.com/documentation/applicationservices/1462077-axuielementcopyelementatposition`

安全与性能约束：

- Accessibility 查询需要人工审查权限与失败路径。
- 查询不能阻塞录制主链路。
- 建议限频，例如 10Hz 或 cursor 移动超过阈值才查询。
- 查询失败必须记录诊断计数，但不能阻断录制或导出。

### 3.3 Critical 3: Arrow / IBeam glyph 颜色与验收相反

位置：

- `src-tauri/src/media/cursor_overlay.rs:64-92`
- `src-tauri/src/media/cursor_overlay.rs:124-146`
- `src-tauri/src/media/cursor_overlay.rs:527-540`

当前颜色语义：

```rust
const B: GlyphPixel = GlyphPixel::Black(255);
const W: GlyphPixel = GlyphPixel::White(255);
```

```rust
GlyphPixel::Black(_) -> Y=16
GlyphPixel::White(_) -> Y=235
```

问题：

- 计划要求：
  - Arrow：黑色填充、白色描边。
  - Hand：白色填充、黑色描边。
  - IBeam：黑色主体、白色描边。
- 当前 Arrow bitmap 是 B 外边 + W 内部，表现为白底黑边箭头。
- 当前 IBeam 也偏白色主体、黑色边。
- 这与人工验证“显示为白底黑边箭头”一致。

为什么测试没发现：

- `fit_with_bars_identity_mapping` 只检查附近像素 `b > 0`。
- 该断言无法区分黑、白、描边、填充。
- 没有 snapshot-like 测试，也没有像素级颜色契约测试。

修复方向：

1. 替换 Arrow bitmap：
   - 外层描边用 `W`。
   - 主体填充用 `B`。
2. 替换 IBeam bitmap：
   - 主体用 `B`。
   - 外层描边用 `W`。
3. 保持 Hand 为白底黑边，但按人工反馈进一步精修形态：
   - 大拇指和食指伸直。
   - 其他三指向掌心弯曲。
   - 视觉整体更短、更接近系统手形。
4. 增加像素级测试：
   - Arrow 内部 sample 点接近黑色 Y 值。
   - Arrow 描边 sample 点接近白色 Y 值。
   - Hand 内部 sample 点接近白色 Y 值。
   - Hand 描边 sample 点接近黑色 Y 值。
   - IBeam 主体 sample 点接近黑色 Y 值。
   - IBeam 描边 sample 点接近白色 Y 值。

### 3.4 Important 1: `CursorClick` 仍保存 raw 坐标

位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:211-220`

当前实现：

```rust
self.clicks.push_back(CursorClick {
    timestamp,
    button,
    phase,
    x: snapshot.x,
    y: snapshot.y,
});
```

问题：

- `CursorSample` 已经归一化为 source video pixel 坐标。
- `CursorClick` 仍保留 raw global 坐标。
- 当前 overlay ring 主要依据 frame position 画，因此这个问题不一定立即可见。
- 但 persisted cursor metadata 已经违反同一坐标空间契约，后续任何使用 `click_effects.x/y` 的渲染或诊断都会出错。

修复方向：

1. `record_button_transition()` 接收归一化坐标。
2. `CursorClick.x/y` 写入 norm_x/norm_y。
3. 增加 click 坐标归一化测试。

### 3.5 Important 2: CaptureGeometry 来源仍需真实设备确认

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:620-634`

当前实现：

```rust
let frame = display.frame();
let point_pixel_scale = if frame.size.width > 0.0 {
    stream_width as f32 / frame.size.width as f32
} else {
    1.0
};
```

问题：

- 计划里要求 display origin、contentRect、pointPixelScale、stream output size 都进入坐标归一化。
- 当前只使用 `display.frame()`。
- `point_pixel_scale` 从 stream width 推导，并且 mapper 实际不直接使用该字段。
- 真实 SCK `frame()` 与 `CGEventGetLocation()` 是否同坐标系没有被日志或测试证明。

修复方向：

1. 如果 crate 暴露 `contentRect` / `pointPixelScale`，优先读取真实字段。
2. 如果不暴露，必须在代码注释和测试中明确：
   - `display.frame()` 的坐标来源。
   - `CGEventGetLocation()` 的坐标来源。
   - 为什么二者可直接相减。
3. 加一次真实设备诊断日志，用于人工审查：

```text
display_id
display.frame.origin
display.frame.size
stream_width/stream_height
raw_cursor_x/raw_cursor_y
mapped_cursor_x/mapped_cursor_y
```

4. 诊断日志必须限量，不得高频刷屏。

### 3.6 Important 3: terminal export error event 可能不显示

位置：

- `src/components/preview-view.tsx:111-118`
- `src/components/preview-view.tsx:519-538`

当前 listener：

```tsx
if (!payload.cancellable && payload.error) {
  setIsExporting(false)
} else {
  setIsExporting(payload.cancellable && payload.progress < 100)
}
```

当前 UI：

```tsx
{exportProgress && isExporting && (
  ...
)}
```

问题：

- terminal error event 会把 `isExporting` 设为 false。
- 进度块只在 `isExporting` true 时渲染。
- 如果 command promise 没有 reject，或者 no-FFmpeg gate 走 Ok + terminal event，`payload.error` 可能不会展示。

修复方向：

1. listener 收到 terminal error 时同步写入 `beautifyError`：

```tsx
if (!payload.cancellable && payload.error) {
  setIsExporting(false)
  setBeautifyError(payload.error)
}
```

2. 或单独渲染 `exportProgress.error`，不要放在 exporting-only block 内。
3. 增加前端测试：
   - terminal `export-progress` error event 后，页面展示 error 文案。
   - `isExporting` 变 false。
   - 再次导出成功后 error 清理。

### 3.7 Minor: 死代码与 warning

位置：

- `src-tauri/src/media/cursor_overlay.rs:354`
- `src-tauri/src/media/cursor_overlay.rs:419`

问题：

- glyph 替换圆点后，`radius` 变量只用于计算但未直接使用。
- `draw_circle_i64()` 已无调用，仅 `draw_circle_outline_i64()` 仍用于 click ring。

修复方向：

- 删除未使用变量和死函数。
- 保留 click ring 所需函数。

## 4. 系统化调试结论

### 4.1 Phase 1: Root Cause Investigation

已读错误/现象：

- BUG-0010 新现象是垂直向上大幅偏移。
- BUG-0011 新现象是所有状态都是白底黑边箭头。

可稳定解释：

- BUG-0010 可由无条件 Y flip 稳定解释。
- BUG-0011 可由固定 `CursorKind::Arrow` + Arrow bitmap 颜色相反稳定解释。

近期变更定位：

- 第一轮新增 `CursorCoordinateMapper`，并将 `flip_y` 默认设为 true。
- 第一轮新增 `CursorKind`，但没有新增真实 kind 识别。
- 第一轮替换 glyph，但没有颜色契约测试。

### 4.2 Phase 2: Pattern Analysis

工作链路：

```text
MacCursorSource.snapshot()
  -> CursorSnapshot
  -> CursorMetadataRecorder.record_snapshot()
  -> RecordingMetadata.cursor_samples
  -> build_effect_timeline_from_metadata()
  -> CursorEffectEngine
  -> EffectTimeline.frames
  -> CursorOverlayRenderer.draw_on_frame()
```

BUG-0010 差异点：

- 旧问题：raw global 坐标被当作 source video 坐标。
- 第一轮修复：加入归一化，但在 mapper 中引入无条件 Y flip。
- 缺口：未用 top/bottom 测试和真实设备日志验证 Y 轴方向。

BUG-0011 差异点：

- 旧问题：没有 kind，画白圆。
- 第一轮修复：有 kind enum 和 glyph renderer。
- 缺口：没有 kind source，所有样本仍 Arrow。
- 缺口：glyph 颜色没有像素契约。

### 4.3 Phase 3: Hypothesis

BUG-0010 假设：

> 光标仍大幅向上偏移，是因为当前 `CursorCoordinateMapper` 对已经 top-down 对齐的 cursor/display 坐标做了额外 Y flip。

支持证据：

- 代码硬编码 `flip_y: true`。
- 人工验证是垂直向上偏移。
- 中心点测试无法暴露该错误。

BUG-0011 假设：

> 导出永远是箭头，是因为采样侧没有真实 kind 检测，`record_snapshot()` 固定写 Arrow；箭头白底黑边，是因为 Arrow bitmap 本身使用黑边白心。

支持证据：

- `CursorSnapshot` 没有 kind。
- `MacCursorSource` 只返回 x/y/buttons。
- `record_snapshot()` 固定写 `CursorKind::Arrow`。
- Arrow glyph bitmap 与验收颜色相反。

### 4.4 Phase 4: Implementation Direction

本轮未直接修改业务代码。下一轮修复必须先写失败测试，再改实现。

第二轮修复入口自测清单已创建：

- `tests/2026-06-04-bug-0010-0011-second-round-checklist.md`

## 5. 下一轮整改方案

### Phase 1: BUG-0010 坐标 Y 轴与四角测试

目标：

- 修复垂直向上偏移。
- 用测试防止中心点误判。

建议改动文件：

- `src-tauri/src/app/cursor_metadata_runtime.rs`
- `src-tauri/src/platform/macos/screen_capture_kit.rs`
- `src-tauri/src/core/timeline.rs`（仅当需要扩展 `CaptureGeometry` 语义）
- `tests/2026-06-04-bug-0010-0011-second-round-checklist.md`

步骤：

1. 新增 mapper top-left 测试。
2. 新增 mapper bottom-right 测试。
3. 新增非零 origin 四角测试。
4. 新增负 origin 四角测试。
5. 新增 Retina point-to-pixel 测试。
6. 运行测试，确认当前实现失败。
7. 移除无条件 `flip_y: true` 或改成显式 `CursorYAxis`。
8. 补真实设备诊断日志，人工确认一次坐标来源。
9. 运行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_mapper
```

人工门禁：

- 1080p 主显示器四角、中心、下半屏移动。
- Retina 显示器四角、中心、下半屏移动。
- 导出 overlay 与源 cursor 对齐。

### Phase 2: BUG-0011 CursorKind 真实识别

目标：

- 普通状态 Arrow。
- 按钮/链接 Hand。
- 文本输入 IBeam。

建议改动文件：

- `src-tauri/src/core/timeline.rs`
- `src-tauri/src/app/cursor_metadata_runtime.rs`
- `src-tauri/src/platform/macos/cursor_source.rs`
- 可选新增：`src-tauri/src/platform/macos/cursor_kind.rs`

步骤：

1. 扩展 `CursorSnapshot` 携带 `kind`。
2. `CursorMetadataRecorder` 写入 `snapshot.kind`。
3. 增加 fake source 单元测试，覆盖 Arrow / Hand / IBeam。
4. macOS 新增 target-aware kind provider。
5. target-aware provider 必须 fallback Arrow。
6. target-aware provider 必须记录失败计数。
7. 查询必须限频，不进入 SCK callback。
8. 运行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_engine
cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata
```

人工门禁：

- 普通桌面导出为 Arrow。
- 悬停按钮/链接导出为 Hand。
- 悬停文本框导出为 IBeam。

### Phase 3: BUG-0011 glyph 颜色与 hotspot

目标：

- Arrow 黑底白边。
- Hand 白底黑边，形态更接近短手形。
- IBeam 黑底白边。
- click ring 以 hotspot 为中心。

建议改动文件：

- `src-tauri/src/media/cursor_overlay.rs`

步骤：

1. 替换 Arrow bitmap。
2. 替换 IBeam bitmap。
3. 精修 Hand bitmap。
4. 新增像素级颜色测试。
5. 新增 hotspot 测试。
6. 运行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay
```

人工门禁：

- 检查三类 cursor 视觉样式。
- 检查 click magnification 中心不漂移。

### Phase 4: BUG-0012 terminal error UI 边界

目标：

- 保持 BUG-0012 已通过的进度行为。
- 修复 terminal error event 不显示的边界。

建议改动文件：

- `src/components/preview-view.tsx`
- `src/App.test.tsx`

步骤：

1. listener 收到 `!payload.cancellable && payload.error` 时设置 `beautifyError`。
2. 或渲染 `exportProgress.error` 到 exporting block 外。
3. 新增前端测试覆盖 terminal error event。
4. 运行：

```bash
npm test -- --run
```

### Phase 5: 收口与回归

步骤：

1. 修复 `CursorClick` raw 坐标问题。
2. 清理 `cursor_overlay.rs` 死代码和 warning。
3. 更新 `BUG.md` 第二轮人工验证结果。
4. 更新 `HANDOFF.md` 工作记录。
5. 执行完整回归：

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
npm run build
```

## 6. 本轮已执行验证

命令 1：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_mapper
```

结果：

- 4 tests passed。
- 但测试只覆盖中心点和粗略 outside 判断，未覆盖 top/bottom 方向。

命令 2：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay
```

结果：

- 10 unit tests passed。
- 1 integration test passed。
- 但测试只验证“有像素变化”和基础映射，未验证 Arrow/Hand/IBeam 颜色和真实 kind。

观测到的 warning：

- `cursor_overlay.rs` 中 `radius` unused。
- `draw_circle_i64()` dead code。
- 这些不是当前人工失败根因，但应在下一轮收口时清理。

## 7. 需写回或继续遵守的预防规则

BUG-0010 补充强调：

1. mapper 测试不能只测中心点，必须测 top/bottom。
2. Y 轴翻转必须有真实设备证据或显式 geometry 语义，不能硬编码假设。
3. `CursorSample` 和 `CursorClick` 必须处于同一 source video pixel 坐标空间。

BUG-0011 补充强调：

1. `CursorKind` enum 存在不代表 target-aware 已完成；必须有 kind source。
2. renderer 测试必须验证具体 glyph 类型和颜色，不得只检查“像素非零”。
3. target-aware 查询失败必须 fallback Arrow，但 fallback 不能掩盖全部样本都未识别的问题，必须有诊断计数。

BUG-0012 补充强调：

1. terminal progress error 必须被 UI 展示。
2. no-FFmpeg gate、取消、失败都必须让 UI 清理 exporting 状态并显示明确结果。

## 8. 后续执行建议

建议按以下顺序整改：

1. 先修 BUG-0010 Y 轴，因为坐标错误会影响所有光标形态判断和 hotspot 验收。
2. 再接入 BUG-0011 kind source，因为没有 kind source 时 Hand/IBeam 永远不会出现。
3. 再修 glyph 颜色和手形细节，因为这是视觉契约。
4. 最后处理 BUG-0012 terminal error UI 边界和死代码清理。

执行方式建议：

- 使用 `superpowers:subagent-driven-development`，每个 Phase 后做 code review。
- 每个 Phase 必须先写失败测试，再改代码。
- 涉及 macOS Accessibility / ScreenCaptureKit 坐标读取的底层调用，必须人工审查权限、线程和内存安全。
