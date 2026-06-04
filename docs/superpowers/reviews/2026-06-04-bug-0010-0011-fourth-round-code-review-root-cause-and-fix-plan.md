# BUG-0010 / BUG-0011 第四轮 Code Review、根因定位与整改方案

> 日期：2026-06-04
> 范围：第三轮整改后的当前工作区实现，重点文件为 `cursor_metadata_runtime.rs`、`cursor_source.rs`、`cursor_kind.rs`、`screen_capture_kit.rs`、`cursor_engine.rs`、`trim_exporter.rs`
> 关联人工验证：`BUG.md` 中 `BUG-0010_5`、`BUG-0011_5`（第 3 轮人工验证结果）
> 结论：BUG-0010 / BUG-0011 仍未达到验收。第三轮确实修掉了上一层高置信问题，但新的人工现象指向下一层缺口：BUG-0010 需要把视频帧时间轴、cursor 时间轴、source artifact 时间轴做成可证明的一致契约，并隔离 smoothing 对定位验收的影响；BUG-0011 需要补齐 Accessibility 权限门禁、真实 AX 诊断落盘、provider 与 metadata 计数汇总，以及必要时引入系统光标形态来源。

## 1. 本轮输入与审查目标

### 1.1 本轮人工验证反馈

BUG-0010：

- 现象：导出视频光标定位精度有改善，但依旧存在偏移。
- 新特征：
  - 垂直方向定位正确。
  - 录屏刚开始时水平方向定位准确。
  - 经过几段移动后，水平方向开始出现向右偏移。
- 期望：美化光标与源视频光标精确定位，无偏移。

BUG-0011：

- 现象：完全没有改善，导出视频任何情况下仍是黑底白边 Arrow。
- 未满足：
  - 普通桌面：黑底白边 Arrow，且箭头尾部需要加把柄形状。
  - 按钮/链接：白底黑边 Hand，手形需要更精致。
  - 文本框：黑底白边 IBeam。

### 1.2 本轮审查目标

1. 复核第三轮整改是否真正覆盖上一轮 review 的 Critical 问题。
2. 基于 `BUG-0010_5` / `BUG-0011_5` 重新定位剩余根因。
3. 给出下一轮可执行整改方案、编码顺序、测试门禁和人工验证门禁。
4. 复核 `BUG.md` 中预防规则是否已经真正闭环。

## 2. 总体结论

第三轮整改已经完成了必要基础修复：

1. `MacCursorSource` 已在 `CGEventGetLocation()` 后立即记录 `captured_at_nanos`，避免 AX 查询耗时污染 position timestamp。
2. `CursorMetadataRuntime` 已优先使用 `snapshot.captured_at_nanos`，否则使用 poll-start timestamp。
3. AX provider 已改为 `AXUIElementCreateSystemWide()`，并设置 50ms messaging timeout。
4. AX kind 查询已限频到 10Hz。
5. `AXStaticText` 不再默认判为 IBeam，分类已支持 parent chain 和 `AXPress`。
6. 首帧 `CVPixelBuffer` 尺寸已经打印诊断日志。

但当前实现仍存在几个关键未闭环点：

1. BUG-0010 的剩余偏移不再像单纯 Y flip 或 snapshot 内 AX 延迟，而更像 **视频帧时间轴、cursor metadata 时间轴、effect timeline 时间轴和导出时 source timestamp 之间缺少显式对齐契约**，并且 smoothing/interpolation 可能把“美化轨迹”与“定位验收”混在一起。
2. `screen_capture_kit.rs` 当前使用第一帧 PTS 和第一帧 callback entry time 建立映射；cursor 使用 `SessionClock::elapsed_nanos()`。两者是否同源、是否存在固定 offset、是否在 source MP4 中被保留，没有 metadata 证据可核查。
3. `RecordingMetadata` 没有保存视频首帧 timestamp、source artifact 首帧 PTS、PTS origin、actual frame size、sample-to-frame delta 等定位必要数据。人工看到“几段移动后向右偏”时，当前 sidecar 无法判断是时间轴滞后、平滑滞后、帧率量化、丢帧、还是几何映射。
4. BUG-0011 的 AX 查询仍可能因为 **Accessibility 权限缺失** 全部 fallback Arrow；当前权限系统只检查 Screen Recording 和 Microphone，没有检查 Accessibility，也没有在录制前给出 target-aware cursor 的门禁。
5. `cursor_kind.rs` 的全局 AX 计数器与 `CursorMetadataRecorder` 的 metadata 计数没有合并。最终写入 metadata 的 `cursor_kind_diagnostics` 只有 recorder 自己统计的 Arrow/Hand/IBeam，`ax_query_failure_count` 和 `ax_fallback_arrow_count` 被 `Default::default()` 清零，导致“全部 fallback Arrow”的真实原因仍可能不可见。
6. `MacCursorKindProvider` trait 已存在，但 `MacCursorSource` 没有使用它，而是直接调用 `query_cursor_kind()`；provider 的限频实现与测试价值没有进入生产路径。

合并评估：

- **Ready to merge? No**
- 原因：两个 bug 的核心验收仍失败；当前实现缺少足够诊断来证明下一轮修复是否命中根因；BUG-0011 还缺少 Accessibility 权限闭环。

## 3. BUG-0010 系统化定位

### 3.1 已排除或已显著缓解的问题

第三轮之后，以下旧根因已经不再是最高置信解释：

1. 无条件 Y flip：已移除，人工反馈也显示 Y 轴高度正确。
2. `snapshot()` 里 AX 查询延迟污染坐标 timestamp：`captured_at_nanos` 已在 `CGEventGetLocation()` 后记录，并由 runtime 使用。
3. AX 每帧同步查询导致 position 采样长期卡顿：当前 kind 查询已 100ms TTL 限频。

这些修复与“精度有改善”相吻合。

### 3.2 当前最高置信根因：媒体时间轴契约缺失

当前关键数据流：

```text
录制开始
  -> SessionClock::new()

SCK 视频帧
  -> CMSampleBuffer PTS(host time)
  -> first valid PTS + callback entry elapsed 建立 pts_origin
  -> VideoFrame.timestamp = normalize_pts(PTS)
  -> FFmpeg writer 用 VideoFrame.timestamp 写 source MP4 PTS

Cursor
  -> CGEventGetLocation()
  -> captured_at_nanos = SessionClock.elapsed_nanos()
  -> CursorSample.timestamp = captured_at_nanos

Post-process
  -> CursorEffectEngine 从 timestamp=0 开始生成 per-frame timeline
  -> Trim exporter 解码 source MP4
  -> source_nanos = decoded raw_pts 转纳秒
  -> overlay.find_cursor_frame(source_nanos)
```

关键代码证据：

- `src-tauri/src/platform/macos_service.rs:183-236`
  - 创建同一个 `SessionClock`，先启动 SCK，再启动 cursor runtime。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:131-150`
  - 第一帧 PTS 建立 origin 时返回 `session_entry_nanos`，不是 0。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:175-225`
  - 视频帧 timestamp 使用 `normalize_pts(delegate, pts_nanos, session_entry_nanos)`。
- `src-tauri/src/platform/macos/cursor_source.rs:83-91`
  - cursor position 采样后使用 `SessionClock.elapsed_nanos()`。
- `src-tauri/src/media/ffmpeg_writer.rs:680-688`
  - source MP4 的视频 PTS 由 `VideoFrame.timestamp` 换算到 30fps time_base。
- `src-tauri/src/media/trim_exporter.rs:681-695`
  - 导出时先记录 `video_pts_offset` 用于 output PTS，但 overlay 使用的是未减 offset 的 `raw_pts`。
- `src-tauri/src/media/trim_exporter.rs:913-923`
  - overlay 的 `source_nanos` 直接由 `raw_pts` 转纳秒后传入 `draw_on_frame()`。
- `src-tauri/src/media/cursor_engine.rs:103-132`
  - effect timeline 从 timestamp 0 开始按 fps 生成 frame。

当前不能证明的点：

1. source MP4 解码得到的 `raw_pts` 是否完整保留了录制阶段 `VideoFrame.timestamp` 的 offset。
2. `trim_exporter` 用 `raw_pts` 转出的 `source_nanos` 是否与 `CursorSample.timestamp` 在同一个零点。
3. source MP4 第一帧 PTS、`RecordingMetadata.cursor_samples[0].timestamp`、`RecordingMetadata.duration_nanos` 三者是否同一时间域。
4. 当 writer 为保证 PTS 单调执行 `pts.max(last_video_pts + 1)` 时，是否在丢帧或重复帧情况下引入了时间轴漂移。
5. 自动裁剪关闭时，cut timeline 使用的 duration 是否来自 trim metadata / writer duration，并是否与 cursor metadata duration 完全一致。

这组问题能解释“开始准确，移动几段后向右偏”：

- 开始时若 cursor 和视频帧接近同一位置，固定 offset 不明显。
- 移动数次后，如果 overlay 取到的是略早或略晚的 cursor frame，视觉上会在 X 轴出现方向性滞后或领先。
- 如果人工移动路径主要向左或右，时间轴错位会表现为稳定向右或向左，而不是几何公式那种全局固定比例偏移。

### 3.3 第二层根因：smoothing / Bezier 可能污染定位验收

当前 `CursorEffectEngine` 默认会在构建 timeline 时启用 smoothing，除非 UI 配置关闭：

- `src-tauri/src/lib.rs:471-486`
- `src-tauri/src/media/cursor_engine.rs:421-428`

`CursorSmoother` 使用移动平均窗口：

- 30fps：window size 5。
- 60fps：window size 9。

`BezierInterpolator` 再对样本插值：

- `src-tauri/src/media/cursor_engine.rs:141-180`

风险：

1. 移动平均天然会引入轨迹滞后或提前，尤其在方向变化时。
2. Bezier 控制点会根据前后样本重塑轨迹，可能让 overlay 偏离真实系统 cursor 的瞬时位置。
3. 人工验收描述是“精确定位”，但当前美化功能默认把“精确复现位置”和“平滑美化轨迹”混在一起。

结论：

- 下一轮必须先提供 **raw positioning mode** 或测试路径：关闭 smoothing、关闭 click magnification，仅绘制 cursor glyph，以验证坐标和时间轴。
- 只有 raw positioning 通过后，才能单独验收 smoothing 的视觉效果。

### 3.4 第三层风险：几何诊断仍不足以证明 source video 坐标空间

当前 mapper：

- `src-tauri/src/app/cursor_metadata_runtime.rs:57-82`
  - 使用 `stream_width / content_width` 和 `stream_height / content_height`。

当前 geometry 来源：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:630-659`
  - `content_origin` 和 `content_size` 来自 `SCDisplay.frame()`。
  - `stream_width/height` 来自配置。

已新增但仍不足的诊断：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:199-207`
  - 只打印首帧 actual buffer size，没有写入 sidecar。

缺口：

1. actual buffer size 没有写入 `RecordingMetadata`，导出阶段无法读取和校验。
2. 没有把 `SCDisplay.frame()`、configured stream size、actual source MP4 stream size、decoded frame size 放在同一份诊断里对比。
3. 没有记录每 N 个 cursor sample 的 raw global x/y 和 normalized x/y。

本轮人工反馈 Y 轴正确，因此几何不是最高置信根因；但几何契约仍未完全闭环。

## 4. BUG-0011 系统化定位

### 4.1 已完成但仍失败的部分

第三轮之后，代码上已经存在：

1. `CursorSnapshot.kind`。
2. `record_snapshot()` 使用 `snapshot.kind` 写入 `CursorSample`。
3. `CursorEffectEngine` 保留 kind。
4. `CursorOverlayRenderer` 按 kind 选择 glyph。
5. AX root 使用 `AXUIElementCreateSystemWide()`。
6. AX timeout 50ms。
7. parent chain + `AXPress` 分类。

因此“永远 Arrow”不再是字段通路缺失，也不再是 renderer 固定画 Arrow 的问题。剩余问题集中在 kind source 本身和诊断闭环。

### 4.2 当前最高置信根因：缺少 Accessibility 权限门禁

`cursor_kind.rs` 明确依赖 Accessibility：

- `src-tauri/src/platform/macos/cursor_kind.rs:8-10`
  - 无权限时 AX 调用返回错误并 fallback Arrow。

但当前权限系统只检查：

- `src-tauri/src/platform/macos/permissions.rs:6-10`
  - Screen Recording。
  - Microphone。

当前没有检查：

- `AXIsProcessTrusted()`。
- `AXIsProcessTrustedWithOptions()`。
- Accessibility 权限未授权时的 UI 提示。
- cursor kind 需要 AX 权限时的录制前门禁。

这能直接解释 `BUG-0011_5`：

```text
AX 未授权
  -> AXUIElementCopyElementAtPosition 返回错误
  -> query_cursor_kind() fallback Arrow
  -> CursorSnapshot.kind = Arrow
  -> CursorSample.kind = Arrow
  -> CursorFrame.kind = Arrow
  -> Renderer 只画 Arrow
```

因为 fallback 是预期安全行为，如果没有权限门禁和诊断，人工看到的就是“完全没改善”。

### 4.3 Critical 诊断缺陷：AX 全局计数没有写入 metadata

`cursor_kind.rs` 中已有全局计数器：

- `AX_QUERY_FAILURE_COUNT`
- `AX_FALLBACK_ARROW_COUNT`
- `KIND_ARROW_COUNT`
- `KIND_HAND_COUNT`
- `KIND_IBEAM_COUNT`

并且提供：

- `cursor_kind_diagnostics_snapshot()`

但 `CursorMetadataRecorder.finish()` 当前写入的是：

```rust
cursor_kind_diagnostics: Some(CursorKindDiagnostics {
    arrow_count: self.arrow_count,
    hand_count: self.hand_count,
    ibeam_count: self.ibeam_count,
    ..Default::default()
})
```

代码位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:254-259`

结果：

1. `ax_query_failure_count` 永远落盘为 0。
2. `ax_fallback_arrow_count` 永远落盘为 0。
3. stop 时打印的 `ax_fail` / `ax_fallback_arrow` 来自 metadata，也会是 0。
4. 即使 AX 查询全部失败，metadata 也只会显示 Arrow 多、Hand/IBeam 为 0，无法说明原因。

这直接违反 `BUG.md` 预防规则：

- BUG-0011 规则 10：fallback 不能掩盖全部样本都未识别的问题，必须有诊断计数。

### 4.4 Provider trait 没有进入生产路径

`cursor_kind.rs` 中存在 `CursorKindProvider` 和 `MacCursorKindProvider`：

- `src-tauri/src/platform/macos/cursor_kind.rs:139-180`

但 `MacCursorSource` 生产代码没有使用该 provider，而是直接：

```rust
self.cached_kind = cursor_kind::query_cursor_kind(point.x as f32, point.y as f32);
```

代码位置：

- `src-tauri/src/platform/macos/cursor_source.rs:89-92`

影响：

1. provider trait 的 mock 测试无法验证生产 `MacCursorSource` 行为。
2. 生产路径和测试路径脱节。
3. `MacCursorKindProvider` 自带的 TTL 缓存没有被使用；生产路径重复实现了一份 TTL。
4. 后续若要替换为系统 cursor source、AX source、混合 source，会继续增加分叉。

建议把 `MacCursorSource` 改为泛型或 trait object：

```rust
pub struct MacCursorSource<P: CursorKindProvider> {
    session_clock: Arc<SessionClock>,
    kind_provider: P,
}
```

或：

```rust
pub struct MacCursorSource {
    session_clock: Arc<SessionClock>,
    kind_provider: Box<dyn CursorKindProvider>,
}
```

### 4.5 仅靠 AX role 推断可能仍不够

即使 Accessibility 权限已经授权，AX role 仍可能不能完全代表系统当前 cursor shape：

1. 浏览器 / Electron / Tauri 中 hover 到链接时，hit-test 可能返回 `AXGroup`、`AXWebArea`、`AXStaticText` 或匿名 child。
2. 某些 clickable 区域没有 `AXPress` action，但系统 cursor 已经变成 hand。
3. 文本编辑区域可能需要读取 editable/focused/value 属性或 AppKit/WebKit 语义才能确认 IBeam。
4. macOS 当前系统 cursor shape 与 AX target role 是两个不同来源：AX 能推断 target，不能保证等同于系统 cursor glyph。

因此下一轮应预留两条路径：

- Phase A：先把 AX 权限 + 诊断做实，确认 AX 是否能产出 Hand/IBeam。
- Phase B：若 AX 已授权但仍全 Arrow，引入系统 cursor shape source 或更强的 AX 属性采样日志。

## 5. Code Review Findings

### 5.1 Critical 1: BUG-0010 缺少媒体时间轴对齐契约和可观测诊断

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:131-166`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:175-225`
- `src-tauri/src/platform/macos/cursor_source.rs:83-91`
- `src-tauri/src/media/ffmpeg_writer.rs:680-688`
- `src-tauri/src/media/trim_exporter.rs:681-695`
- `src-tauri/src/media/trim_exporter.rs:913-923`

问题：

- SCK PTS、SessionClock、source MP4 PTS、cursor sample timestamp、effect timeline timestamp 没有统一的 metadata 契约。
- 导出时 overlay 使用 `raw_pts` 转换为 `source_nanos`，但没有校验它与 cursor timeline 的零点一致。
- 当前无法通过 sidecar 判断剩余 X 偏移到底来自时间轴 offset、插值、丢帧还是几何。

建议修复：

1. 新增 `MediaTimelineDiagnostics`，写入 `RecordingMetadata`：
   - `session_started_at_kind`（例如 `Instant` 只能描述，不落绝对值）。
   - `first_video_pts_nanos_raw`。
   - `first_video_session_entry_nanos`。
   - `first_video_normalized_nanos`。
   - `first_cursor_sample_nanos`。
   - `last_video_timestamp_nanos`。
   - `last_cursor_sample_nanos`。
   - `video_frame_count`。
   - `cursor_sample_count`。
2. source MP4 写完后或导出前读取第一帧 PTS，并与 metadata 对比：
   - 若 source MP4 第一帧 PTS 不等于录制阶段期望零点，导出 overlay 必须使用同一 offset。
3. `EffectTimeline` 增加可选字段：
   - `source_time_origin_nanos` 或 `cursor_time_offset_nanos`。
4. `trim_exporter` 计算 overlay timestamp 时显式处理 offset：

```text
decoded_source_nanos = raw_pts_to_nanos(raw_pts)
overlay_source_nanos = decoded_source_nanos - source_video_pts_origin_nanos + cursor_timeline_origin_nanos
```

5. 添加自动测试：构造 `raw_pts` 带非零 origin 的 synthetic timeline，验证 overlay 选中的 cursor frame 正确。

### 5.2 Critical 2: BUG-0010 没有 raw positioning 验收路径，smoothing 可能污染定位结论

位置：

- `src-tauri/src/lib.rs:471-500`
- `src-tauri/src/media/cursor_engine.rs:421-428`
- `src-tauri/src/media/cursor_engine.rs:103-180`

问题：

- 默认 cursor smoothing 会对坐标做移动平均和 Bezier 插值。
- 人工验收要求“精确定位”，但当前验收看到的是美化轨迹，不一定是 raw coordinate mapping。
- 若 smoothing 引入 2-4 帧滞后，快速横向移动时就会表现为 X 轴偏移。

建议修复：

1. 新增开发/诊断用的 `rawCursorPositioning` 配置或测试命令。
2. 验证 BUG-0010 时强制：
   - cursor smoothing = false。
   - cursor magnification = false 或 scale = 1。
   - 只渲染 glyph。
3. `CursorEffectEngine` 增加测试：
   - smoothing off 时 timeline frame 必须在对应 timestamp 命中原始样本或线性插值。
   - smoothing on 时允许视觉偏移，但不能用于“精确定位”验收。
4. `BUG.md` 增加预防规则：定位验收必须先在 raw positioning mode 下通过，再验收 smoothing/magnification。

### 5.3 Critical 3: BUG-0011 缺少 Accessibility 权限检查和录制前门禁

位置：

- `src-tauri/src/platform/macos/permissions.rs:5-10`
- `src-tauri/src/app/permission_service.rs:10-15`
- `src-tauri/src/platform/macos/cursor_kind.rs:8-10`

问题：

- target-aware cursor kind 依赖 Accessibility，但权限模型没有这个字段。
- 未授权时 AX query 失败并 fallback Arrow，UI 和 metadata 都不明确提示。
- 用户可能已经授权 Screen Recording 和 Microphone，但未授权 Accessibility，从而永远 Arrow。

建议修复：

1. `RecordingPermissions` 增加：

```rust
pub accessibility: PermissionStatus
```

2. macOS probe 增加 FFI：

```rust
fn AXIsProcessTrusted() -> bool;
```

3. `recording_permissions` 返回 accessibility 状态。
4. 前端在启用 cursor beautification 时，如果 accessibility 未 granted：
   - 显示明确提示。
   - 禁止或降级 target-aware cursor。
   - 提供“需要授权后重新启动 App/重新录制”的提示。
5. 录制开始时如果 raw cursor hidden 且需要 target-aware glyph，应记录权限状态到 metadata。

### 5.4 Critical 4: BUG-0011 AX 失败计数没有写入 metadata

位置：

- `src-tauri/src/platform/macos/cursor_kind.rs:103-123`
- `src-tauri/src/app/cursor_metadata_runtime.rs:240-260`
- `src-tauri/src/app/cursor_metadata_runtime.rs:318-332`

问题：

- 全局 AX counters 有值，但 finish 时没有合并。
- metadata 中 `ax_query_failure_count` 和 `ax_fallback_arrow_count` 实际为 0。
- stop 日志基于 metadata 打印，因此也会误导为 AX 没失败。

建议修复：

1. `CursorMetadataRecorder.finish()` 接收 provider/global diagnostics：

```rust
let mut diag = cursor_kind_diagnostics_snapshot();
diag.arrow_count = self.arrow_count;
diag.hand_count = self.hand_count;
diag.ibeam_count = self.ibeam_count;
```

2. 或将 provider diagnostics 作为 `CursorMetadataRuntime` 的依赖注入。
3. 添加测试：
   - 模拟 AX failure 后 finish metadata 必须包含 failure count。
   - `hand_count == 0 && ibeam_count == 0 && ax_query_failure_count > 0` 时必须输出 warning 或设置 metadata warning 字段。
4. 导出 / build timeline 阶段读取 metadata，如果全 Arrow 且 AX failure > 0，应提示用户本次录制无法识别 cursor kind。

### 5.5 Important 1: `CursorKindProvider` trait 未接入生产路径

位置：

- `src-tauri/src/platform/macos/cursor_kind.rs:139-180`
- `src-tauri/src/platform/macos/cursor_source.rs:53-115`

问题：

- provider trait 目前只被测试 mock 使用。
- `MacCursorSource` 直接调用 free function，导致生产路径难以 mock。
- TTL 缓存逻辑存在两份。

建议修复：

1. `MacCursorSource` 持有 `Box<dyn CursorKindProvider>`。
2. `MacCursorSource::new()` 默认注入 `MacCursorKindProvider::new()`。
3. 测试中注入：
   - always Hand provider。
   - always IBeam provider。
   - failing provider。
   - slow provider。
4. 删除 `MacCursorSource` 中重复 TTL，统一放在 provider 内。

### 5.6 Important 2: AX 分类缺少可审计采样日志

位置：

- `src-tauri/src/platform/macos/cursor_kind.rs:255-302`

问题：

- 当前只返回 kind，不记录命中的 role/action/parent depth。
- 如果授权后仍全 Arrow，无法判断命中的是 `AXWebArea`、`AXGroup`、`AXStaticText`，还是 action 读取失败。

建议修复：

1. 增加限频采样日志，写入 metadata 或 stderr：
   - raw point。
   - result code。
   - role chain。
   - actions。
   - classified kind。
   - query duration。
2. 日志必须限频，例如每秒最多 2 条，避免刷屏。
3. 对人工验证用例分别记录：
   - 桌面。
   - Tauri button。
   - Safari/Chrome link。
   - native text field。
   - Tauri input。

### 5.7 Important 3: `CursorKind` 识别可能需要系统 cursor source，不应只依赖 AX

问题：

- AX target role 是间接推断。
- 系统 cursor shape 是直接事实。
- 如果 AX 已授权但仍不能稳定识别 Hand/IBeam，应改为或补充使用系统 cursor source。

建议探索：

1. 尝试 AppKit `NSCursor.currentSystemCursor` 或可用的 CGS/private-safe 替代方案。注意不要引入不可发布的私有 API。
2. 如果系统 cursor shape 无法直接读取，保留 AX role provider，但扩大分类：
   - editable 属性。
   - focused/value settable。
   - subrole。
   - role description。
   - web link attributes。
3. 设计 provider 链：

```text
SystemCursorShapeProvider
  -> 成功识别 Arrow/Hand/IBeam 则使用
AXTargetProvider
  -> 作为 fallback / 辅助
Arrow
  -> 最终安全 fallback
```

### 5.8 Important 4: Arrow glyph 还未满足“尾部加把柄形状”的人工期望

位置：

- `src-tauri/src/media/cursor_overlay.rs`

问题：

- BUG-0011_5 明确追加：Arrow 尾部需要加把柄形状。
- 第三轮重点在 kind source，未说明 arrow glyph 形状是否已更新。

建议修复：

1. 更新 Arrow bitmask，让箭头尾部具备 macOS 风格把柄。
2. 增加像素级测试：
   - arrow tail 区域存在黑色主体。
   - white outline 包围主体。
   - hotspot 仍在箭头尖端。
3. 人工验证里单独截帧检查 Arrow 外形，不要和 Hand/IBeam 识别混为一个结论。

### 5.9 Minor: 未使用 FFI 声明应清理

位置：

- `src-tauri/src/platform/macos/cursor_kind.rs:63`

问题：

- `AXUIElementCreateApplication(pid)` 仍在 extern block 中，但已不应使用。

建议：

- 删除该 FFI 声明，避免后续误用。

## 6. BUG.md 预防规则复核

### 6.1 BUG-0010 规则复核

已满足：

1. `CursorSample` / `CursorClick` 已归一化到 source video pixel 空间。
2. Y 轴不再硬编码翻转。
3. `snapshot()` 内 AX 查询不再污染 position timestamp。
4. 首帧 CVPixelBuffer size 已打印。

未完全满足：

1. “ScreenCaptureKit display origin、contentRect、pointPixelScale、stream output size 必须进入 cursor 坐标归一化”仍缺实际 source artifact / decoded frame 对比。
2. “首帧视频到达时必须记录 actual CVPixelBuffer 尺寸并与 CaptureGeometry 对比”目前只是 stderr 打印，没有写入 metadata，也没有 hard check。
3. 多显示器、Retina、CenterCrop/FitWithBars 仍缺端到端坐标回归。
4. 缺少“视频帧 timestamp 与 cursor timestamp 必须共享可证明 media timebase”的规则。
5. 缺少“定位验收必须先关闭 smoothing/magnification”的规则。

建议新增 BUG-0010 预防规则：

11. video frame PTS、cursor sample timestamp、effect timeline timestamp、export overlay source timestamp 必须共享同一 source media timebase；任何 offset 都必须写入 metadata 并由测试验证。
12. source artifact 第一帧 PTS 和 metadata 首帧/末帧时间必须有诊断对比；导出 overlay 不得隐式假设 raw_pts 从 0 开始。
13. 光标定位验收必须先在 raw positioning mode 下进行，禁止 smoothing/magnification 影响坐标正确性判断。
14. cursor diagnostics 必须落盘到 sidecar，不能只打印到 stderr。

### 6.2 BUG-0011 规则复核

已满足：

1. timeline 已携带 `CursorKind`。
2. renderer 已按 kind 绘制不同 glyph。
3. AX root 已改为 system-wide。
4. AX timeout 已设置。
5. `AXStaticText` 不再默认 IBeam。

未完全满足：

1. target-aware 查询失败的诊断计数没有正确落盘。
2. Accessibility 权限没有纳入 permission model。
3. provider trait 没有接入生产路径。
4. 分类结果缺少 role/action/parent chain 采样日志。
5. Arrow glyph 的“尾部把柄”期望尚未单独闭环。

建议新增 BUG-0011 预防规则：

15. target-aware cursor 依赖 Accessibility 时，Accessibility 权限必须进入录制前门禁和 metadata；未授权不能静默表现为全 Arrow。
16. AX failure/fallback/kind distribution 必须来自实际 provider 并写入 RecordingMetadata，不能只统计 recorder 最终 kind。
17. `CursorKindProvider` 必须可注入并覆盖生产路径，避免 mock 测试与真实采样脱节。
18. 每次 target-aware 整改必须提供 role/action/classification 采样日志或等价诊断，证明 Hand/IBeam 的来源。
19. glyph 形状变更必须配套 hotspot、颜色、关键形状区域的像素级测试。

## 7. 推荐整改 Phase

### Phase 0: 先补诊断，不先猜公式

目标：让下一轮人工验证能回答“为什么偏”和“为什么全 Arrow”。

任务：

1. 新增 `MediaTimelineDiagnostics` 并写入 `RecordingMetadata`。
2. 新增 `CursorTimingDiagnostics`：
   - first cursor sample。
   - last cursor sample。
   - sample interval min/max/avg。
   - skipped/outside count。
3. 把首帧 actual CVPixelBuffer size 写入 metadata。
4. 合并 `cursor_kind_diagnostics_snapshot()` 到 metadata。
5. stop 后打印完整结构化摘要：
   - video first/last。
   - cursor first/last。
   - AX permission。
   - AX failure/fallback。
   - kind distribution。

建议测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata_runtime
cargo test --manifest-path src-tauri/Cargo.toml recording_metadata
cargo test --manifest-path src-tauri/Cargo.toml cursor_kind
```

### Phase 1: 修 BUG-0011 的权限与诊断闭环

原因：BUG-0011 “完全没改善”高度可疑是权限或 fallback 不可见；先做这个能快速判断 AX 路线是否可行。

任务：

1. `RecordingPermissions` 增加 `accessibility`。
2. macOS probe 增加 `AXIsProcessTrusted()`。
3. 前端展示 Accessibility 状态。
4. 启用 cursor beautification 时，如果 Accessibility 未授权，给出明确提示。
5. metadata 记录 `accessibility_permission_at_record_start`。
6. provider 真实 failure/fallback 计数进入 metadata。

人工验证门禁：

1. 未授权 Accessibility 时，UI 能明确提示，不再让用户误以为 Hand/IBeam 已可用。
2. 授权 Accessibility 后录制按钮/链接/文本框，metadata 中至少出现 Hand 或 IBeam；若没有，必须看到 AX result code 和 role chain。

### Phase 2: 将 `CursorKindProvider` 接入生产路径

任务：

1. `MacCursorSource` 改为持有 provider。
2. `MacCursorSource::new()` 注入 `MacCursorKindProvider::new()`。
3. 测试注入 mock provider，覆盖：
   - Hand。
   - IBeam。
   - always Arrow。
   - failure。
   - slow query。
4. TTL 缓存只保留在 provider 中。

验证：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_source
cargo test --manifest-path src-tauri/Cargo.toml cursor_kind
```

### Phase 3: 修 BUG-0010 时间轴对齐

任务：

1. 记录 source MP4 第一帧 PTS，和录制时 first video normalized timestamp 对比。
2. `EffectTimeline` 或 `RecordingMetadata` 中保存 `source_video_pts_origin_nanos`。
3. `trim_exporter` overlay timestamp 明确减去 source video origin，再映射到 cursor timeline。
4. 如果 source MP4 PTS 被 muxer 重写，导出阶段必须重新读取并使用实际 demux PTS origin。
5. 添加测试：
   - source video PTS 从 200ms 开始，cursor samples 从 200ms 开始，overlay 仍选中正确 frame。
   - source video PTS 从 0 开始，cursor samples 从 0 开始，行为不变。
   - cut timeline 不改变 source timestamp 查询。

验证：

```bash
cargo test --manifest-path src-tauri/Cargo.toml trim_exporter
cargo test --manifest-path src-tauri/Cargo.toml cursor_overlay
cargo test --manifest-path src-tauri/Cargo.toml cursor_engine
```

### Phase 4: 加 raw positioning 验收模式

任务：

1. 增加诊断构建路径：smoothing off、magnification off、glyph only。
2. 前端或命令层允许人工验证时生成 raw cursor overlay。
3. 对同一素材生成两份 timeline：
   - raw positioning timeline。
   - beautified timeline。
4. 先让 raw positioning 通过，再调整 smoothing。

人工验证门禁：

1. 静止 cursor：raw overlay 与源位置重合。
2. 水平匀速移动：raw overlay 不持续向右/向左漂。
3. 多段水平移动：raw overlay 不出现累计漂移。
4. smoothing on 后若有视觉滞后，应作为美化策略单独评估，不算坐标映射错误。

### Phase 5: 修 BUG-0011 glyph 外形

任务：

1. Arrow 增加尾部把柄形状。
2. Hand glyph 调整为更接近 macOS hand：
   - 食指和大拇指伸直。
   - 其他三指向掌心弯曲。
   - 整体视觉短一些。
3. IBeam 保持黑底白边，确认 hotspot 居中。
4. 增加像素级测试和人工截帧验收。

## 8. 建议编码顺序

1. `PermissionStatus` / `RecordingPermissions` 加 `accessibility` 字段。
2. macOS `permissions.rs` 加 `AXIsProcessTrusted()`。
3. 前端类型和权限提示同步更新。
4. `CursorKindDiagnostics` 合并真实 AX counters，写入 metadata。
5. `MacCursorSource` 注入 `CursorKindProvider`。
6. 增加 AX role/action 采样日志。
7. 增加 `MediaTimelineDiagnostics` 到 metadata。
8. source MP4 / decoded frame 第一帧 PTS 诊断。
9. `trim_exporter` overlay timestamp 显式 origin mapping。
10. raw positioning mode / tests。
11. glyph bitmask 外形调整。
12. 更新 `BUG.md` 的本轮根因、整改内容和新增预防规则。

## 9. 自动测试清单

必须补：

1. `permissions_reports_accessibility_status`
2. `cursor_kind_diagnostics_merges_ax_failure_counts_into_metadata`
3. `mac_cursor_source_uses_injected_kind_provider`
4. `mac_cursor_source_preserves_position_timestamp_with_slow_kind_provider`
5. `effect_timeline_raw_positioning_disables_smoothing`
6. `overlay_uses_source_pts_origin_when_input_pts_is_non_zero`
7. `recording_metadata_serializes_media_timeline_diagnostics`
8. `arrow_glyph_has_tail_handle_and_hotspot_still_at_tip`

建议命令：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata_runtime
cargo test --manifest-path src-tauri/Cargo.toml cursor_kind
cargo test --manifest-path src-tauri/Cargo.toml cursor_engine
cargo test --manifest-path src-tauri/Cargo.toml cursor_overlay
cargo test --manifest-path src-tauri/Cargo.toml recording_metadata
cargo test --manifest-path src-tauri/Cargo.toml permissions
cargo test --manifest-path src-tauri/Cargo.toml trim_exporter
npm test -- --run
```

## 10. 人工验证清单

### 10.1 BUG-0010 raw positioning

录制条件：

- cursor smoothing off。
- cursor magnification off 或 scale=1。
- raw system cursor hidden，overlay glyph on。
- Accessibility 状态记录在 metadata 中。

场景：

1. 静止桌面四角和中心。
2. 水平从左到右匀速移动。
3. 水平从右到左匀速移动。
4. 多段移动：左到右、停顿、右到左、停顿、再左到右。
5. Retina 显示器。
6. 外接显示器负 origin。

通过标准：

- Y 轴保持准确。
- X 轴不随时间累计向右或向左漂。
- metadata 中 video/cursor first/last timestamp 差异在预期范围内。

### 10.2 BUG-0010 beautified timeline

前置条件：

- raw positioning 已通过。

场景：

1. smoothing on。
2. magnification on。
3. click effect on。

通过标准：

- 平滑效果不产生明显延迟。
- 如果有设计性 smoothing 滞后，必须可配置或降低窗口。

### 10.3 BUG-0011 target-aware kind

前置条件：

- Accessibility granted。
- metadata 中 `accessibilityPermissionAtRecordStart=granted`。
- AX failure/fallback/kind distribution 可见。

场景：

1. 普通桌面：Arrow。
2. Tauri 按钮：Hand。
3. 浏览器链接：Hand。
4. 原生文本框：IBeam。
5. Tauri / Electron input：IBeam。

通过标准：

- metadata 中 Hand/IBeam count > 0。
- 如果某场景失败，metadata 或日志能看到 role/action/classification 依据。
- 导出视频 glyph 与 metadata kind 分布一致。

### 10.4 BUG-0011 glyph 外形

场景：

1. Arrow 截帧。
2. Hand 截帧。
3. IBeam 截帧。

通过标准：

- Arrow 黑底白边，并有尾部把柄。
- Hand 白底黑边，外形短而清晰。
- IBeam 黑底白边，hotspot 居中且不遮挡文本定位。

## 11. 本轮最终判断

BUG-0010：

- 第三轮修复有效降低了偏移，但剩余问题已经进入“时间轴契约 + smoothing 污染 + 诊断不足”层面。
- 下一轮不要继续盲改 mapper 公式；应先让 source video PTS、cursor sample timestamp、effect timeline timestamp、overlay source timestamp 可观测、可测试、可对齐。

BUG-0011：

- 字段通路和 renderer 不再是主问题。
- 当前最高置信根因是 Accessibility 权限未纳入门禁，且 AX failure/fallback 诊断没有真实落盘。
- 修权限和诊断后，才能判断 AX 分类是否足够；若授权后仍全 Arrow，再引入系统 cursor shape source 或扩展 AX 属性分类。

推荐下一轮整改优先级：

1. Accessibility 权限 + AX diagnostics 落盘。
2. `CursorKindProvider` 接入生产路径。
3. BUG-0010 media timeline diagnostics。
4. overlay timestamp origin mapping。
5. raw positioning 验收模式。
6. glyph 外形收尾。
