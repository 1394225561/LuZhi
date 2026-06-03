# BUG-0010 / BUG-0011 / BUG-0012 Root Cause And Fix Plan

> 日期：2026-06-03
> 范围：`BUG.md` 未解决项 BUG-0010、BUG-0011、BUG-0012
> 结论：三条 bug 均可从当前实现链路定位到明确原因；建议按“坐标修正 → 光标形态 → 导出进度”分阶段修复。

## 1. 总体结论

本次定位基于当前代码静态链路审查，没有修改代码。

核心判断：

1. **BUG-0010 光标偏移**：当前录制侧保存的是 macOS 全局屏幕坐标，导出侧却按 source video pixel 坐标解释，缺少 display/contentRect/pointPixelScale/stream size 的坐标归一化；后续绘制真实箭头/手/I-beam 时还必须处理 hotspot，否则图标目标点仍会偏。
2. **BUG-0011 光标不好看**：当前 cursor timeline 没有 cursor kind / target type / hotspot 字段，渲染器固定绘制“黑边白色圆点”，无法根据普通、可点击、输入框状态切换形态。
3. **BUG-0012 导出进度不优雅**：前端已有进度 UI 和事件订阅，但后端 FFmpeg exporter 只按 keep segment 完成度上报。自动裁剪关闭时只有一个 keep segment，因此表现为 0% 停很久，接近完成时直接到 99/100%。

推荐修复顺序：

1. 先修 **BUG-0010**，保证光标 overlay 坐标与源视频对齐。
2. 再修 **BUG-0011**，在正确坐标上绘制不同 cursor glyph。
3. 最后修 **BUG-0012**，提升导出体验，不影响光标/编码正确性。

## 2. BUG-0010: 导出的视频光标定位不对

### 2.1 现象

经过美化后导出的视频中，光标位置相对源视频产生向左上方偏移。

### 2.2 关键证据

证据 1：macOS cursor source 直接读取全局坐标。

- `src-tauri/src/platform/macos/cursor_source.rs:65`
- `src-tauri/src/platform/macos/cursor_source.rs:68`

当前 `MacCursorSource::snapshot()` 使用 `CGEventGetLocation(event)`，然后把 `point.x` / `point.y` 原样写入 `CursorSnapshot`。

证据 2：cursor metadata recorder 没有做任何坐标转换。

- `src-tauri/src/app/cursor_metadata_runtime.rs:69`
- `src-tauri/src/app/cursor_metadata_runtime.rs:75`
- `src-tauri/src/app/cursor_metadata_runtime.rs:77`

`CursorMetadataRecorder::record_snapshot()` 直接把 snapshot 的 `x/y` 写入 `CursorSample`。

证据 3：导出 overlay 假设 cursor timeline 已经是 source video pixel 坐标。

- `src-tauri/src/media/cursor_overlay.rs:7`
- `src-tauri/src/media/cursor_overlay.rs:9`
- `src-tauri/src/media/cursor_overlay.rs:100`
- `src-tauri/src/media/cursor_overlay.rs:204`

`CursorOverlayRenderer` 的注释和实现都把 timeline 中的 `x/y` 当作源视频像素坐标，然后再映射到导出尺寸。

证据 4：ScreenCaptureKit 当前固定抓第一个 display，并将输出缩放到 `CaptureConfig` 指定尺寸。

- `src-tauri/src/platform/macos/screen_capture_kit.rs:639`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:641`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:655`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:656`

当前代码选择 `displays.objectAtIndex(0)`，创建全屏 display filter，并把 stream 输出尺寸设置为 `config.width/config.height`。但这些 display 的 `frame/contentRect/pointPixelScale` 没有进入 cursor metadata。

### 2.3 根因

当前链路混用了三个不同坐标空间：

1. **全局屏幕坐标**：`CGEventGetLocation()` 返回鼠标在 macOS 全局桌面空间中的位置。
2. **捕获内容坐标**：ScreenCaptureKit 的 display/contentRect 空间，可能有非零 origin、多显示器负坐标、Retina point/pixel scale。
3. **源视频像素坐标**：FFmpeg source artifact 解码后的帧尺寸，例如 1920x1080。

当前实现把第 1 类坐标直接保存，然后在导出时按第 3 类坐标使用。只要真实 display origin、contentRect、pointPixelScale 或 stream scaling 与 1920x1080 不完全一致，overlay 位置就会系统性偏移。

此外，当前圆点绘制没有 hotspot 概念。后续如果改成箭头/手/I-beam，必须用 hotspot 对齐目标点；否则即使坐标归一化正确，图标左上角或视觉中心仍可能和真实点击点错位。

### 2.4 解决方案

#### Phase A: 增加捕获几何数据

新增一个只在 Rust 后端使用的捕获几何结构，例如：

```rust
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureGeometry {
    pub display_id: u32,
    pub content_origin_x: f32,
    pub content_origin_y: f32,
    pub content_width: f32,
    pub content_height: f32,
    pub point_pixel_scale: f32,
    pub stream_width: u32,
    pub stream_height: u32,
}
```

建议存放位置：

- 类型：`src-tauri/src/core/timeline.rs` 或 `src-tauri/src/media/recording_metadata.rs`
- metadata 字段：`RecordingMetadata { capture_geometry: Option<CaptureGeometry>, ... }`

说明：

1. 保持 `Option<CaptureGeometry>` 可以兼容已有旧 metadata。
2. `stream_width/stream_height` 对应最终 source artifact frame size。
3. `content_origin/content_width/content_height` 应取与 `CGEventGetLocation()` 同一坐标系下的 ScreenCaptureKit content rect 或 display frame。

`objc2-screen-capture-kit` 当前可用 API 中已暴露 `SCDisplay::displayID()`、`SCDisplay::frame()`、`SCDisplay::pointPixelScale()`、`SCDisplay::contentRect()`，理论上无需新增底层依赖。底层安全调用仍需人工审查。

#### Phase B: 录制时把全局坐标归一化到 source video 坐标

建议在 `CursorMetadataRuntime::spawn()` 时传入 `CaptureGeometry` 或一个纯 Rust `CursorCoordinateMapper`。

归一化公式：

```text
local_x = global_x - content_origin_x
local_y = global_y - content_origin_y

source_x = local_x * stream_width / content_width
source_y = local_y * stream_height / content_height
```

注意事项：

1. 如果实测发现 `CGEventGetLocation()` 与 SCK `contentRect` 的 y 轴方向相反，则应在 mapper 中集中处理：

   ```text
   local_y = content_height - (global_y - content_origin_y)
   ```

   不要在 overlay renderer 中零散修正。

2. 超出 content rect 的坐标应保留为 `None` 或被标记为 outside，不应 clamp 成边缘点，否则鼠标在非捕获显示器上会被错误绘制到视频边缘。
3. `CursorSample.x/y` 修复后应明确语义为 source video pixel coordinate。若需要调试，可额外保留 raw global 坐标，但 exporter 只能使用归一化坐标。

#### Phase C: overlay renderer 只消费归一化坐标

`CursorOverlayRenderer` 的 `CursorCoordMapper` 继续负责 source video → export output 的 FitWithBars / CenterCrop 变换，不再承担全局屏幕坐标修正。

修复后职责边界：

1. `MacCursorSource`：读取 raw cursor snapshot。
2. `CursorMetadataRuntime`：把 raw snapshot 归一化为 source video 坐标。
3. `CursorEffectEngine`：平滑、插值、click scale，不改变坐标空间。
4. `CursorOverlayRenderer`：source video 坐标 → export output 坐标，并绘制 glyph。

### 2.5 测试建议

自动测试：

1. `cursor_mapper_maps_identity_display_to_stream_pixels`
2. `cursor_mapper_subtracts_non_zero_display_origin`
3. `cursor_mapper_handles_negative_display_origin`
4. `cursor_mapper_scales_points_to_1080p_stream`
5. `cursor_mapper_marks_cursor_outside_capture_region`
6. `cursor_overlay_uses_hotspot_when_rendering_arrow`

人工门禁：

1. 1080p 主屏录制：鼠标移动到四角和中心，导出后 overlay 与真实目标点一致。
2. Retina 屏幕录制：验证 point/pixel scale 不导致偏移。
3. 多显示器：主屏、外接屏、负坐标 display 均验证。
4. 三种导出预设：Bilibili FitWithBars、Douyin CenterCrop、小红书 CenterCrop 坐标一致。

## 3. BUG-0011: 美化后的光标不好看

### 3.1 现象

美化后导出视频中的光标是白色圆点，不符合预期的 macOS 风格 cursor。

期望：

1. 普通状态：黑底白边箭头。
2. 可点击区域：白底黑边手形。
3. 输入框区域：黑底白边 I-beam。

### 3.2 关键证据

证据 1：cursor snapshot 没有 target/kind 信息。

- `src-tauri/src/app/cursor_metadata_runtime.rs:20`
- `src-tauri/src/app/cursor_metadata_runtime.rs:23`
- `src-tauri/src/app/cursor_metadata_runtime.rs:24`
- `src-tauri/src/app/cursor_metadata_runtime.rs:25`

当前 `CursorSnapshot` 只有 `x/y` 和左右中键按下状态。

证据 2：cursor frame 没有形态或 hotspot。

- `src-tauri/src/core/timeline.rs:45`
- `src-tauri/src/core/timeline.rs:47`
- `src-tauri/src/core/timeline.rs:49`
- `src-tauri/src/core/timeline.rs:50`

当前 `CursorFrame` 只有 `timestamp/x/y/scale/opacity`。

证据 3：renderer 固定画圆。

- `src-tauri/src/media/cursor_overlay.rs:163`
- `src-tauri/src/media/cursor_overlay.rs:165`
- `src-tauri/src/media/cursor_overlay.rs:253`
- `src-tauri/src/media/cursor_overlay.rs:265`
- `src-tauri/src/media/cursor_overlay.rs:276`

`CursorOverlayRenderer` 固定 `cursor_radius = 12.0`，先画黑色圆形 outline，再画白色圆形 fill，click scale 只额外画圆环。

### 3.3 根因

当前系统只有“位置 + 点击放大”的抽象，没有“当前 cursor 形态”的数据通路。renderer 也没有可扩展的 glyph 绘制层，因此只能输出白色圆点。

如果想做到“可点击区域显示手形、输入框显示 I-beam”，只修改绘制代码不够，还必须在采样阶段知道鼠标下方目标类型，或拿到当前系统 cursor 形态。

### 3.4 解决方案

#### Phase A: 扩展 timeline cursor 形态契约

新增稳定 serde enum：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CursorKind {
    Arrow,
    Hand,
    IBeam,
}
```

扩展 `CursorSample` 或 `CursorFrame`：

```rust
pub struct CursorSample {
    pub timestamp: MediaTimestamp,
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub kind: CursorKind,
}

pub struct CursorFrame {
    pub timestamp: MediaTimestamp,
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub opacity: f32,
    #[serde(default)]
    pub kind: CursorKind,
}
```

建议也新增 hotspot：

```rust
pub struct CursorGlyphMetrics {
    pub hotspot_x: f32,
    pub hotspot_y: f32,
}
```

hotspot 不一定要逐帧存储，也可以由 `CursorKind` 查表得到：

1. Arrow：hotspot 在箭头尖。
2. Hand：hotspot 在食指尖。
3. IBeam：hotspot 在中线中心或平台约定点。

#### Phase B: target/kind 识别

推荐先做两层方案。

第一层：默认可交付修复。

1. 无法识别目标时，全部使用 `CursorKind::Arrow`。
2. 先替换掉白色圆点，保证普通录屏观感明显提升。
3. click magnification 继续画在 hotspot 周围，不改变 cursor 本体。

第二层：target-aware 识别。

macOS 侧可以通过 Accessibility hit-test 判断鼠标下方元素 role：

1. `AXTextField` / `AXTextArea` / 可编辑文本 role → `IBeam`
2. `AXButton` / `AXLink` / `AXMenuItem` / 支持 press action 的元素 → `Hand`
3. 其他或识别失败 → `Arrow`

性能约束：

1. Accessibility hit-test 不应在 SCK callback 中执行，只能在 `CursorMetadataRuntime` 后台线程中执行。
2. 建议限频，例如 10Hz 或仅 cursor 移动超过阈值时查询。
3. 查询失败必须 fallback 为 `Arrow`，并记录诊断计数，不能阻塞录制。
4. 该部分涉及 macOS 原生 API 和权限，必须人工审查。

备选方案：

如果能稳定获取当前系统 cursor image/kind，则优先记录系统 cursor kind/image；这样比用 AX role 推断更贴近真实系统行为。但当前代码没有这条通路，不能假设已经可用。

#### Phase C: 使用本地 glyph asset 或 bitmask 渲染

不建议纯代码手搓复杂手形。原因：

1. 箭头和 I-beam 可用少量几何图元画出来。
2. 手形要达到 macOS 风格，纯代码像素路径维护成本高，且不利于后续调尺寸和抗锯齿。

推荐方案：

1. 在仓库中放置本地 cursor glyph 资源或静态 bitmask：
   - `arrow_black_white`
   - `hand_white_black`
   - `ibeam_black_white`
2. 每个 glyph 带固定 `width/height/hotspot_x/hotspot_y`。
3. renderer 在 YUV420P 上做 alpha blend。
4. 如果只做黑白 cursor，可以先只操作 Y plane；如后续使用彩色/半透明资源，再补 U/V plane 合成。

不建议本轮新增图片解码依赖。若必须使用 PNG 解码库，需要单独人工确认 `Cargo.toml` 依赖变更；更保守的方式是把小尺寸 alpha mask 编译成 Rust 静态数组。

#### Phase D: 渲染器替换圆点逻辑

替换位置：

- `src-tauri/src/media/cursor_overlay.rs`

建议抽象：

```rust
struct CursorGlyph {
    width: usize,
    height: usize,
    hotspot_x: f32,
    hotspot_y: f32,
    pixels: &'static [GlyphPixel],
}

enum GlyphPixel {
    Transparent,
    Black(u8),
    White(u8),
}
```

绘制规则：

1. `out_x/out_y` 表示 cursor hotspot 在 output frame 中的位置。
2. glyph 左上角为：

   ```text
   draw_x = out_x - hotspot_x * scale
   draw_y = out_y - hotspot_y * scale
   ```

3. click scale 影响 glyph 尺寸；click ring 仍围绕 hotspot 绘制。
4. glyph 绘制必须保留 BUG-008 的 finite check、range clamp、i64 arithmetic。

### 3.5 测试建议

自动测试：

1. `cursor_kind_serializes_camel_case`
2. `cursor_frame_defaults_to_arrow_for_old_json`
3. `cursor_engine_preserves_sample_kind_through_interpolation`
4. `cursor_overlay_draws_arrow_hotspot_at_cursor_position`
5. `cursor_overlay_draws_hand_when_kind_is_hand`
6. `cursor_overlay_draws_ibeam_when_kind_is_ibeam`
7. `cursor_overlay_click_ring_uses_hotspot_not_glyph_center`
8. `cursor_overlay_invalid_kind_or_old_timeline_falls_back_to_arrow`

人工门禁：

1. 普通桌面移动：导出为黑底白边箭头。
2. 悬停按钮/链接：导出为白底黑边手形。
3. 悬停输入框/文本编辑区域：导出为黑底白边 I-beam。
4. 点击放大时，放大中心必须是目标 hotspot，不是图标中心。
5. 三种导出预设下 hotspot 对齐不变。

## 4. BUG-0012: 点击导出按钮后的交互不够优雅

### 4.1 现象

点击导出后没有连续百分比反馈，用户看到进度从 0% 直接到导出完成。

### 4.2 关键证据

证据 1：前端已有进度事件订阅。

- `src/components/preview-view.tsx:109`
- `src/components/preview-view.tsx:111`
- `src/components/preview-view.tsx:112`
- `src/components/preview-view.tsx:113`

`PreviewView` 已监听 `export-progress`，并根据 payload 更新 `exportProgress` 和 `isExporting`。

证据 2：前端已有进度条 UI。

- `src/components/preview-view.tsx:514`
- `src/components/preview-view.tsx:517`
- `src/components/preview-view.tsx:527`
- `src/components/preview-view.tsx:530`

当前 UI 已显示 `正在导出 {progress}%` 和进度条宽度。

证据 3：后端开始导出时只发 0%。

- `src-tauri/src/lib.rs:731`
- `src-tauri/src/lib.rs:736`

`export_video()` 开始时 emit `progress: 0`。

证据 4：FFmpeg exporter 只按 keep segment 上报。

- `src-tauri/src/media/trim_exporter.rs:462`
- `src-tauri/src/media/trim_exporter.rs:1076`
- `src-tauri/src/media/trim_exporter.rs:1079`
- `src-tauri/src/media/trim_exporter.rs:1080`

当前进度计算为：

```rust
let pct = ((seg_idx + 1) * 99 / total_keeps).clamp(1, 99) as u8;
```

这意味着只有一个 keep segment 时，中间不会上报任何真实进度。

证据 5：自动裁剪关闭时确实只有一个 keep segment。

- `src-tauri/src/core/cut.rs:129`
- `src-tauri/src/core/cut.rs:134`

`CutTimeline::empty()` 会把整个录制作为一个 keep segment。

### 4.3 根因

UI 不是主要问题。主要问题是后端 exporter 的进度粒度太粗，且没有把“已处理的视频时间/帧数”转换成持续百分比。

当前进度模型只适合“很多 keep segment”的裁剪场景，不适合最常见的“不裁剪整段导出”场景。

### 4.4 解决方案

#### Phase A: 后端进度改为时间/帧级

在 `FfmpegTrimExporter::export()` 中计算总 keep 时长：

```rust
let total_keep_nanos: u64 = request
    .cut_timeline
    .keeps
    .iter()
    .map(|segment| segment.end.nanos.saturating_sub(segment.start.nanos))
    .sum();
```

每处理一个 video frame 后，根据 source timestamp 计算已处理 keep 时长：

```text
processed_keep_nanos =
  sum(previous_keep_segment_durations)
  + clamp(frame_source_nanos - current_segment_start, 0, current_segment_duration)
```

进度计算：

```text
progress = 1 + processed_keep_nanos * 98 / total_keep_nanos
```

约束：

1. 中间进度只能在 1..99。
2. 100% 只能在 exporter 写完 trailer，并且上层 artifact validation 成功后上报。
3. 进度必须单调递增。
4. 如果 `total_keep_nanos == 0`，fallback 到当前 segment 级逻辑或保持 1%。

#### Phase B: 节流上报，避免事件风暴

新增局部 helper：

```rust
struct ProgressState {
    last_reported: u8,
}

impl ProgressState {
    fn maybe_report(&mut self, reporter: &ExportProgressReporter, next: u8) {
        let next = next.clamp(1, 99);
        if next > self.last_reported {
            self.last_reported = next;
            reporter.report(next);
        }
    }
}
```

可选增强：增加时间节流，例如每 200ms 最多 emit 一次。若只在百分比变化时 emit，1080p 导出最多约 99 次，通常可以接受。

#### Phase C: 覆盖 cursor/cut 准备阶段

当前 `export_video()` 在进入 FFmpeg 前还会构建 cursor timeline 和 cut timeline。如果这些阶段耗时明显，用户仍可能停在 0%。

建议在 `src-tauri/src/lib.rs` 中补充轻量进度：

1. export command 开始：0%
2. cursor timeline 完成：5%
3. cut timeline 完成或跳过：10%
4. FFmpeg export：10%..99%
5. validation + success：100%

实现方式：

1. 保持 `ExportProgressReporter` 仍接收 `u8`，避免扩大 API 面。
2. lib.rs 中给 FFmpeg reporter 包一层 range mapper：

   ```text
   mapped = 10 + exporter_progress * 89 / 100
   ```

3. 失败或取消仍按现有 terminal progress 事件清理 UI。

#### Phase D: 前端小幅增强

前端当前已经显示进度条，不需要重写。

建议补充：

1. terminal error payload 到达时显示 `payload.error`，而不是只依赖 `exportVideo()` promise catch。
2. progress 到 100 且 `outputPath` 存在时，保留成功摘要。
3. 取消导出后显示“导出已取消”或直接回到可导出状态，由产品确认。

### 4.5 测试建议

Rust 单元/集成测试：

1. `export_progress_reports_intermediate_values_for_single_keep_segment`
2. `export_progress_is_monotonic`
3. `export_progress_never_reports_100_before_success`
4. `export_progress_handles_zero_duration_timeline`
5. FFmpeg integration：真实 synthetic source 导出时，progress callback 至少收到一个 `1..99` 的中间值和最终 `100`。

前端测试：

1. `PreviewView displays export progress percentage`
2. `PreviewView updates progress bar width on export-progress event`
3. `PreviewView clears exporting state on terminal success`
4. `PreviewView clears exporting state on terminal error`

人工门禁：

1. 无裁剪导出 10 秒视频：进度应从 0% 连续增长到 100%。
2. 有自动裁剪导出：跨多个 keep segment 时进度仍单调。
3. 取消导出：进度 UI 清理，不显示成功文件路径。
4. FFmpeg validation 失败：不显示 100% 成功态。

## 5. 跨 bug 约束

### 5.1 数据流红线

修复过程中不得把视频帧、音频帧或 cursor sample stream 发送到 React。

允许：

1. 录制结束后的 cursor/effect/cut/export summary。
2. 轻量 progress event。
3. 结构化 diagnostics。

不允许：

1. 前端参与逐帧 cursor compositing。
2. 前端接收媒体帧。
3. 前端用 JS 计算视频帧级 overlay。

### 5.2 原生 API 人工审查

以下变更必须人工审查：

1. 读取 `SCDisplay::frame/contentRect/pointPixelScale/displayID` 的 Objective-C / objc2 调用。
2. Accessibility hit-test / AXUIElement 相关调用。
3. cursor glyph 渲染若涉及 unsafe buffer 操作，需重点检查 bounds、stride、YUV plane subsampling。

### 5.3 Cargo.toml 约束

不建议为 BUG-0011 直接新增图片解码依赖。优先方案：

1. 使用 Rust 静态 bitmask。
2. 或使用仓库本地已展开的 alpha mask 数据。

如果确实需要新增 `image` / `tiny-skia` 等依赖，必须单独提出并由人工确认，不能混在 bug 修复里悄悄修改依赖。

## 6. 推荐实施计划

### Phase 1: BUG-0010 坐标归一化

目标：

- cursor samples 写入 source video pixel 坐标。
- overlay renderer 不再消费全局屏幕坐标。

建议改动：

1. 新增 `CaptureGeometry` / `CursorCoordinateMapper`。
2. `MacScreenCapture::start_stream()` 读取 selected display geometry。
3. `MacRecordingService::start()` 将 geometry 传入 cursor runtime。
4. `CursorMetadataRecorder` 写入归一化坐标。
5. 新增 mapper 单元测试和 overlay hotspot 预备测试。

验证：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay
```

### Phase 2: BUG-0011 cursor kind 和 glyph

目标：

- 普通导出不再出现白色圆点。
- 至少默认 `Arrow` 能正确绘制并 hotspot 对齐。
- target-aware 识别按风险可拆成后续子阶段。

建议改动：

1. 新增 `CursorKind`。
2. timeline serde 兼容旧 JSON。
3. `CursorEffectEngine` 插值时保留 kind。
4. `CursorOverlayRenderer` 用 glyph 替换圆点。
5. 可选：增加 AX target 识别。

验证：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_engine
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay
```

### Phase 3: BUG-0012 进度粒度

目标：

- 单 keep segment 导出也有中间进度。
- progress 单调，不提前报 100。
- 前端现有进度条真实可见。

建议改动：

1. `FfmpegTrimExporter` 增加 frame/time based progress。
2. `lib.rs export_video()` 增加准备阶段进度和 range mapping。
3. 前端补 terminal error/success 的细节测试。

验证：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg export_progress
npm test -- --run
```

## 7. 建议写入 BUG.md 的预防规则

修复完成后，建议在 `BUG.md` 对应 bug 下补充以下预防规则。

BUG-0010：

1. cursor metadata 必须保存或使用与 source video 一致的坐标空间，不能把全局屏幕坐标直接交给 exporter。
2. ScreenCaptureKit display origin、contentRect、pointPixelScale、stream output size 必须进入 cursor 坐标归一化。
3. cursor glyph 绘制必须以 hotspot 对齐目标点，不能以图标左上角或视觉中心对齐。
4. 多显示器、Retina scale、CenterCrop/FitWithBars 必须有坐标回归测试或人工门禁。

BUG-0011：

1. cursor timeline 必须携带稳定的 cursor kind 或 glyph 信息；renderer 不得固定画圆点冒充系统 cursor。
2. target-aware cursor 识别失败时必须 fallback 为 Arrow，不能阻塞录制或导出。
3. cursor glyph 必须有 hotspot metadata，并由测试验证 hotspot 对齐。
4. cursor asset/bitmask 更新必须配套像素级或 snapshot-like 回归测试。

BUG-0012：

1. export progress 不能只按 keep segment 上报；单段完整导出也必须产生中间进度。
2. export progress 必须单调递增，且 100% 只能在导出和 artifact validation 成功后发送。
3. 取消、失败、no-FFmpeg gate 等 terminal path 必须发送 `cancellable=false` 的 terminal progress，让 UI 清理导出状态。
4. 前端进度条测试必须模拟中间 `export-progress` 事件，而不能只验证最终 summary。
