# BUG-0010 / BUG-0011 第三轮 Code Review、根因定位与整改方案

> 日期：2026-06-04
> 范围：第二轮 BUG 修复提交 `07051c548a675de6ddc61c39f3ab335624c15444^..81b86bb7b7ea9054b93a9ea80692e9480315441b`
> 关联人工验证：`BUG.md` 中 `BUG-0010_3`、`BUG-0011_3`
> 结论：BUG-0010 / BUG-0011 仍未达到验收。BUG-0010 的最高置信根因是第二轮新增同步 AX 查询后，cursor 坐标被打上了延后的时间戳；BUG-0011 的最高置信根因是 AX hit-test 使用了错误的 root object，导致真实 kind 查询持续 fallback 为 Arrow。

## 1. 本轮输入与审查目标

### 1.2 第二轮人工验证反馈

BUG-0010：

- 现象：导出视频光标定位仍存在偏移。
- 新特征：垂直方向高度定位正确。
- X 轴特征：水平方向偶尔准确，偶尔偏左，偶尔偏右。
- 期望：美化光标与源视频光标精确定位，无偏移。

BUG-0011：

- 现象：导出视频任何情况下都是黑底白边 Arrow。
- 未满足：
  - 悬停按钮/链接时应为白底黑边 Hand。
  - 悬停文本框时应为黑底白边 IBeam。
  - Arrow 尾部还需要补把柄形状。

### 1.3 本轮审查目标

1. 对第二轮修复做细致 code review。
2. 系统化定位 BUG-0010 / BUG-0011 仍失败的现有根因。
3. 给出下一轮可执行的整改方案和验证门禁。
4. 复核实现是否遵循 `BUG.md` 中相关预防规则。

## 2. 总体结论

第二轮修复完成了若干必要基础工作：

1. 移除了 BUG-0010 的无条件 Y 轴翻转。
2. `CursorSnapshot.kind -> CursorSample.kind -> CursorFrame.kind -> CursorOverlayRenderer` 的数据字段通路已经存在。
3. Arrow / IBeam 的黑底白边颜色契约已经接近验收。
4. Click 坐标已改为 source video pixel 坐标空间。

但两个核心行为仍没有闭环：

1. BUG-0010：第二轮新增同步 AX kind 查询后，`MacCursorSource.snapshot()` 先读取坐标，再执行可能阻塞的 AX hit-test；`CursorMetadataRuntime` 在 `snapshot()` 返回后才写入 timestamp。坐标采样时刻和记录 timestamp 不再一致，横向移动时会出现方向相关的左右偏移。
2. BUG-0011：`cursor_kind.rs` 把 `AXUIElementCreateApplication(0)` 当作 system-wide accessibility object。macOS SDK 明确提供独立的 `AXUIElementCreateSystemWide()`。当前 provider 很可能持续查询失败或查错作用域，全部 fallback 为 Arrow。
3. BUG-0010 还有一层未完全证明的风险：当前 `CaptureGeometry` 仍只使用 `SCDisplay.frame()` 和配置的 stream size，没有记录或校验实际 SCK content rect、CVPixelBuffer 尺寸、source artifact frame size。时间戳修复后若仍有固定 X 偏移，应继续在这一层取证和修复。

合并评估：

- **Ready to merge? No**
- 原因：两条人工验证失败都能在当前实现中找到高置信根因，且自动测试没有覆盖真实 AX provider、snapshot 延迟、视频帧时间戳与 cursor 时间戳对齐。

## 3. 系统化调试结论

### 3.1 BUG-0010 数据流追踪

当前录制阶段的数据流：

```text
CursorMetadataRuntime loop
  -> source.snapshot()
     -> CGEventCreate()
     -> CGEventGetLocation(event)        // 这里读到 cursor position
     -> CFRelease(event)
     -> query_cursor_kind(x, y)           // 这里执行同步 AX hit-test，可能阻塞
     -> CursorSnapshot { x, y, kind }
  -> MediaTimestamp::from_nanos(session_clock.elapsed_nanos())
  -> recorder.record_snapshot(timestamp, snapshot)
  -> CursorSample { timestamp, x, y, kind }
```

关键代码位置：

- `src-tauri/src/platform/macos/cursor_source.rs:66-69`
- `src-tauri/src/app/cursor_metadata_runtime.rs:263-267`

问题点：

- `CGEventGetLocation()` 读到的位置属于 `snapshot()` 开始阶段。
- `query_cursor_kind()` 在同一个 `snapshot()` 内同步执行，并且是 Accessibility API，可能耗时、失败、被权限或目标进程响应拖慢。
- `CursorMetadataRuntime` 在 `snapshot()` 完成之后才调用 `session_clock.elapsed_nanos()` 生成 timestamp。
- 因此同一个 sample 的 `x/y` 与 `timestamp` 代表两个不同时间点。

为什么匹配人工现象：

```text
鼠标向右移动：
  实际 t=100ms 坐标 x=500
  AX 查询耗时 40ms
  代码把 x=500 记录为 t=140ms
  导出 t=140ms 的视频帧时，overlay 使用旧位置，表现为偏左

鼠标向左移动：
  同样的延迟会表现为偏右

鼠标垂直移动少：
  Y 轴看起来正确，X 轴随移动方向出现左/右漂移
```

这比“单纯线性坐标公式错误”更贴合 `BUG-0010_3` 的描述。公式错误通常产生稳定方向、稳定比例或边缘相关偏移；人工反馈是“偶尔左、偶尔右”，更像时间戳错位或动态平滑滞后。

### 3.2 BUG-0010 仍需保留的第二层假设

当前 `CaptureGeometry` 读取逻辑：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:620-648`
  - `content_origin_x/y = display.frame().origin`
  - `content_width/height = display.frame().size`
  - `point_pixel_scale = stream_width / frame.size.width`
  - `stream_width/height = config.width/config.height`
- `src-tauri/src/app/cursor_metadata_runtime.rs:64-76`
  - `source_x = (global_x - origin_x) * scale_x`
  - `source_y = (global_y - origin_y) * scale_y`

尚未证明的点：

1. SCK 实际输出的 `CVPixelBuffer` 宽高是否始终等于 `CaptureConfig.width/height`。
2. SCK 对非 16:9 显示器缩放到 1920x1080 时，是拉伸、fit、crop，还是使用了 content inset。
3. `SCDisplay.frame()` 是否等于实际被 `SCContentFilter` 捕获并写入 source artifact 的 content rect。
4. 多显示器场景下 `displays.objectAtIndex(0)` 是否就是实际被录制的显示器。

这层暂不作为最高置信根因，因为它更常产生固定或位置相关偏移；但它违反了 `BUG.md` 中 “ScreenCaptureKit display origin、contentRect、pointPixelScale、stream output size 必须进入 cursor 坐标归一化” 的完整性要求。下一轮应加诊断，不要继续靠推测。

### 3.3 BUG-0011 数据流追踪

当前 kind 数据流：

```text
MacCursorSource.snapshot()
  -> query_cursor_kind(point.x, point.y)
     -> AXUIElementCreateApplication(0)
     -> AXUIElementCopyElementAtPosition(app_element, x, y, &element)
     -> failure or wrong scope
     -> CursorKind::Arrow
  -> CursorSnapshot { kind: Arrow }
  -> CursorSample { kind: Arrow }
  -> CursorFrame { kind: Arrow }
  -> CursorOverlayRenderer draws Arrow
```

关键代码位置：

- `src-tauri/src/platform/macos/cursor_kind.rs:97-128`
- `src-tauri/src/platform/macos/cursor_source.rs:69-87`
- `src-tauri/src/app/cursor_metadata_runtime.rs:149-154`
- `src-tauri/src/media/cursor_overlay.rs:456-470`

SDK 证据：

- 本机 macOS SDK header：
  - `AXUIElementCopyElementAtPosition` 说明：如果传 system-wide accessibility object，hit-test 不限制到某个 app。
  - `AXUIElementCreateApplication(pid)` 说明：创建指定 pid app 的 top-level accessibility object。
  - `AXUIElementCreateSystemWide()` 说明：返回 system-wide accessibility object。
  - 路径：`/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/System/Library/Frameworks/ApplicationServices.framework/Versions/A/Frameworks/HIServices.framework/Versions/A/Headers/AXUIElement.h:334-371`

结论：

- `AXUIElementCreateApplication(0)` 不是 system-wide hit-test root。
- 当前实现注释 “pid=0 creates a system-wide accessibility element” 与 SDK 语义冲突。
- 这能直接解释 `BUG-0011_3`：字段通路和 renderer 都存在，但 provider 实际没有产出 Hand / IBeam。

### 3.4 自动测试为什么没发现

已执行的相关测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_mapper
cargo test --manifest-path src-tauri/Cargo.toml cursor_kind
cargo test --manifest-path src-tauri/Cargo.toml cursor_overlay
```

结果：

- `cursor_mapper`：10 passed。
- `cursor_kind`：3 passed，但只覆盖 enum serialization 和 recorder 保留 kind。
- `cursor_overlay`：0 tests matched，过滤名没有命中现有 renderer 测试。

测试缺口：

1. 没有测试 `CursorMetadataRuntime` 在慢 `snapshot()` 下是否使用了正确采样 timestamp。
2. 没有测试 `MacCursorSource` 中坐标读取和 kind 查询的时间语义。
3. 没有测试 AX provider 使用 `AXUIElementCreateSystemWide()`。
4. 没有测试 AX 查询失败 / fallback Arrow 的诊断是否进入 metadata 或 stop/export 日志。
5. 没有测试 Hand / IBeam 实际 rendered glyph 的 Y plane 像素值。
6. 没有测试实际视频帧尺寸与 `CaptureGeometry.stream_width/height` 的一致性。

## 4. Code Review Findings

### 4.1 Critical 1: BUG-0010 坐标采样时间与记录 timestamp 错位

位置：

- `src-tauri/src/platform/macos/cursor_source.rs:66-69`
- `src-tauri/src/app/cursor_metadata_runtime.rs:263-267`

当前行为：

```rust
let point = CGEventGetLocation(event);
CFRelease(event as CFTypeRef);

let kind = cursor_kind::query_cursor_kind(point.x as f32, point.y as f32);
```

```rust
match source.snapshot() {
    Ok(snapshot) => {
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(session_clock.elapsed_nanos()),
            snapshot,
        );
    }
}
```

问题：

- `snapshot()` 内新增同步 AX 查询后，`snapshot()` 的耗时不再可以忽略。
- timestamp 在 `snapshot()` 返回后生成，因此被 AX 查询耗时污染。
- Cursor position 和 CursorKind 在同一个 snapshot 中混合，但它们对时间精度的要求不同：
  - position 必须贴近视频帧时间。
  - kind 可以低频、缓存、延迟更新。

影响：

- 直接导致动态横向偏移。
- AX 越慢、鼠标移动越快，偏移越明显。
- 偏移方向随鼠标移动方向变化，符合人工反馈。

建议修复：

1. 最小修复：

```rust
while !thread_stop.load(Ordering::Relaxed) {
    let sample_timestamp = MediaTimestamp::from_nanos(session_clock.elapsed_nanos());
    match source.snapshot() {
        Ok(snapshot) => recorder.record_snapshot(sample_timestamp, snapshot),
        Err(_) => recorder.record_snapshot_failure(),
    }
    thread::sleep(interval);
}
```

2. 更稳健修复：
   - 扩展 `CursorSnapshot` 携带 `captured_at: MediaTimestamp`。
   - `MacCursorSource` 在 `CGEventGetLocation()` 之前或之后立即记录 `captured_at`。
   - `CursorMetadataRecorder.record_snapshot()` 使用 `snapshot.captured_at`。
3. 将 kind 查询从高频 position 采样中解耦：
   - position 每帧采样。
   - kind 低频查询，例如 10Hz，或 cursor 移动超过阈值再查。
   - kind 查询失败时使用上一次 kind 或 Arrow fallback，但必须记录诊断。

必须新增测试：

1. `runtime_timestamps_snapshot_at_poll_start_not_after_slow_source`
   - Mock source 在返回前 sleep。
   - 断言 sample timestamp 不随 source sleep 后移。
2. `snapshot_duration_does_not_shift_cursor_position_timestamp`
   - 验证 source query duration 进入 diagnostics，而不是污染 timestamp。

### 4.2 Critical 2: BUG-0011 AX hit-test root object 错误

位置：

- `src-tauri/src/platform/macos/cursor_kind.rs:99-113`

当前行为：

```rust
// pid=0 creates a system-wide accessibility element.
let app_element = AXUIElementCreateApplication(0);
```

问题：

- `AXUIElementCreateApplication(pid)` 是为指定 pid 创建 application-level AX object。
- system-wide object 应通过 `AXUIElementCreateSystemWide()` 创建。
- 当前 `pid=0` 不是可靠的跨应用 hit-test root。
- `AXUIElementCopyElementAtPosition()` 在传普通 application object 时会限制或错误作用域。

影响：

- `AXUIElementCopyElementAtPosition()` 很可能返回错误或查不到当前鼠标所在 UI。
- 代码在失败时直接 fallback Arrow。
- `BUG-0011_3` 中 “任何情况下都是黑底白边箭头” 与该根因完全吻合。

建议修复：

1. FFI 增加：

```rust
fn AXUIElementCreateSystemWide() -> AXUIElementRef;
fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout_in_seconds: f32) -> i32;
```

2. 使用 system-wide object：

```rust
let system = AXUIElementCreateSystemWide();
AXUIElementSetMessagingTimeout(system, 0.05);
let result = AXUIElementCopyElementAtPosition(system, global_x as f64, global_y as f64, &mut element);
CFRelease(system);
```

3. 记录 `result` error code：
   - `kAXErrorNoValue`
   - `kAXErrorIllegalArgument`
   - `kAXErrorInvalidUIElement`
   - `kAXErrorCannotComplete`
   - `kAXErrorNotImplemented`
4. 将 AX failure count、fallback count、kind distribution 暴露到 recording metadata 或 stop diagnostics。

必须新增测试：

1. 将 AX FFI 包成可 mock 的 provider trait，测试 query 使用 system-wide root。
2. 测试 AX error code 进入 diagnostics。
3. 测试全部 fallback Arrow 时 diagnostics 可见，不能静默通过。

### 4.3 Critical 3: BUG-0010 几何归一化仍缺少实际 SCK source geometry 证据

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:620-648`
- `src-tauri/src/app/cursor_metadata_runtime.rs:51-76`

问题：

- `CaptureGeometry.stream_width/height` 来自 `CaptureConfig`，不是实际 `CVPixelBuffer`。
- `content_width/height` 来自 `SCDisplay.frame()`，没有证明等于 SCK 捕获写入 source artifact 的 content rect。
- `point_pixel_scale` 字段被写入，但 mapper 实际只使用 `stream_width / content_width` 和 `stream_height / content_height`。
- 诊断日志只打印 display frame 和 configured stream size，不能证明 actual source video geometry。

影响：

- 在非 16:9 显示器、Retina、外接屏、多显示器或后续窗口/区域录制中，仍可能出现 X/Y 坐标映射误差。
- 即使 BUG-0010 的动态偏移由 timestamp 解决，固定边缘偏移仍可能在这层出现。

建议修复：

1. 首帧视频到达时记录实际：
   - `cvpixelbuffer_width`
   - `cvpixelbuffer_height`
   - `bytes_per_row`
   - `display_id`
   - `SCDisplay.frame()`
   - `CaptureConfig.width/height`
2. 若 actual frame size 与 `CaptureGeometry.stream_width/height` 不一致，应更新 metadata 或 hard fail。
3. 若 SCK 可获取 content rect / scale factor / attachments，应优先使用实际 content rect。
4. 添加非 16:9 display-to-stream 映射测试，覆盖中心、左右边缘、上下边缘。

### 4.4 Important 1: CursorKind 分类过浅，修完 AX root 后仍可能漏判 Hand/IBeam

位置：

- `src-tauri/src/platform/macos/cursor_kind.rs:168-181`

当前分类：

```rust
Some("AXTextField") | Some("AXTextArea") | Some("AXStaticText") => CursorKind::IBeam,
Some("AXButton") | Some("AXLink") | ... => CursorKind::Hand,
_ => CursorKind::Arrow,
```

问题：

- 浏览器、Tauri、Electron、原生 App 的 hit-test 结果常常是 child text、group、image、web area，而不是直接 `AXButton` / `AXLink`。
- `AXStaticText` 不等于可编辑文本，把普通 label 判成 IBeam 会产生错误。
- 未检查：
  - parent chain
  - `AXSubrole`
  - `AXRoleDescription`
  - `AXUIElementCopyActionNames`
  - `AXEnabled`
  - editable / focused / value 属性

建议修复：

1. IBeam：
   - `AXTextField`
   - `AXTextArea`
   - 或明确 editable 的 text role
   - 不要把普通 `AXStaticText` 默认归为 IBeam。
2. Hand：
   - `AXLink`
   - `AXButton`
   - `AXMenuItem`
   - 或当前元素/父元素支持 `AXPress` action。
3. 对 hit-test 到 child text 的场景，向父节点最多回溯 3-5 层。
4. 输出 `role/subrole/actions/classified_kind` 采样日志，供人工验证。

### 4.5 Important 2: AX diagnostics 存在但未进入可见输出

位置：

- `src-tauri/src/platform/macos/cursor_kind.rs:24-28`
- `src-tauri/src/platform/macos/cursor_kind.rs:80-88`

问题：

- `AX_QUERY_FAILURE_COUNT` 和 `AX_FALLBACK_ARROW_COUNT` 只存在于模块内。
- 没有写入 metadata。
- 没有在 stop/export/build timeline 时打印结构化日志。
- 没有出现在 UI 或人工验证所需的诊断输出中。

违反规则：

- `BUG.md` BUG-0011 新增预防规则 10：
  - target-aware 查询失败必须 fallback Arrow，但 fallback 不能掩盖全部样本都未识别的问题，必须有诊断计数。

建议修复：

1. 增加 `CursorKindDiagnostics`：

```rust
pub struct CursorKindDiagnostics {
    pub ax_query_failure_count: u64,
    pub ax_fallback_arrow_count: u64,
    pub arrow_count: u64,
    pub hand_count: u64,
    pub ibeam_count: u64,
    pub max_query_duration_ms: u64,
    pub avg_query_duration_ms: f32,
}
```

2. 写入 `RecordingMetadata` 或 `RecordingDiagnostics`。
3. `build_cursor_effect_timeline()` 或 stop 结束时打印一次结构化摘要。
4. 当 `hand_count == 0 && ibeam_count == 0 && ax_query_failure_count > 0` 时，人工验证必须视为未通过。

### 4.6 Important 3: AX 同步查询每帧执行，且 stop 无 bounded join

位置：

- `src-tauri/src/platform/macos/cursor_source.rs:69`
- `src-tauri/src/app/cursor_metadata_runtime.rs:286-288`

问题：

- 每个 cursor snapshot 都同步执行 AX hit-test。
- 如果 Accessibility API 因权限、目标 app 卡顿或 messaging timeout 卡住，cursor runtime 线程会变慢。
- `CursorMetadataRuntime::stop()` 直接 `join()`，没有 timeout。

影响：

- cursor samples 变稀疏，增加动态偏移和插值误差。
- stop/finalize 可能被 AX 调用拖慢。

建议修复：

1. AX 查询限频，例如 10Hz。
2. 为 AX 设置 messaging timeout。
3. 记录 snapshot duration。
4. stop 使用 bounded join 或在 AX provider 内保证严格超时。

### 4.7 Minor: 注释与测试描述有陈旧内容

位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:624-629`
- `src-tauri/src/media/cursor_overlay.rs:698-699`

问题：

- mapper 测试注释仍写 “Current implementation with flip_y=true”。
- renderer 测试注释仍写 Arrow tip pixel 是 black，但第二轮颜色契约已经改为 white outline。

建议修复：

- 清理陈旧注释，避免下一轮 code review 误判当前实现意图。

### 4.8 Minor / Test Gap: 第二轮计划中的 terminal error UI 测试未落地

位置：

- `src/components/preview-view.tsx:111-119`
- `src/App.test.tsx`

问题：

- 第二轮计划列了 `src/App.test.tsx` terminal error 前端测试。
- diff 中只看到 `preview-view.tsx` 逻辑变更，没有看到对应测试。

建议修复：

- 补测试：模拟 `export-progress` 事件 `{ cancellable: false, error: "..." }`，断言：
  - `isExporting` 被清理。
  - 错误详情显示在 UI。

## 5. 根因定位结论

### 5.1 BUG-0010 最可能根因

**主根因，高置信：**

第二轮把 `query_cursor_kind()` 插入 `MacCursorSource.snapshot()` 后，坐标采样与 timestamp 记录之间新增了同步 AX 查询耗时。`CursorMetadataRuntime` 在 `snapshot()` 返回后才打 timestamp，导致 cursor 坐标被写入错误时间点。横向移动时表现为方向相关的左右偏移。

证据：

1. 代码顺序明确：先 `CGEventGetLocation()`，后 `query_cursor_kind()`，再 runtime 写 timestamp。
2. AX API 可失败、可 timeout、可被目标进程响应拖慢。
3. 人工现象是 X 轴偶尔左/右，符合时间错位，不符合固定线性映射错误。
4. 该行为由第二轮新增 CursorKind provider 后引入或显著放大。

**次级假设，需在修主根因后继续验证：**

1. 视频帧 timestamp 当前以 callback entry 对齐，而不是实际 frame capture time，对快速移动 cursor 也可能引入延迟。
2. `CaptureGeometry` 未用实际 source video geometry，非标准比例或多显示器下可能仍有固定偏移。
3. smoothing/interpolation 本身会改变轨迹；如果人工验证要求“精确贴合真实 cursor”，验证时需要关闭 smoothing，或在验收定义中区分“原始定位正确”和“美化轨迹平滑”。

### 5.2 BUG-0011 最可能根因

**主根因，高置信：**

AX provider 使用 `AXUIElementCreateApplication(0)` 作为 system-wide root，这与 SDK 语义冲突。导致 `AXUIElementCopyElementAtPosition()` 查询失败或被错误作用域限制，所有样本 fallback Arrow。

证据：

1. SDK 中 `AXUIElementCreateApplication(pid)` 和 `AXUIElementCreateSystemWide()` 是不同 API。
2. `AXUIElementCopyElementAtPosition()` 文档说明传 system-wide object 才能不限制到某个 app。
3. 当前实现失败时静默返回 Arrow。
4. 人工现象是任何情况下都是 Arrow，说明 renderer 不是主要问题，timeline 中很可能只有 Arrow。

**次级根因：**

1. 分类只看 immediate role，实际 UI 命中 child text / group 时会漏判。
2. `AXStaticText -> IBeam` 会误判普通静态文本。
3. fallback / failure 诊断没有可见化，导致“全部 fallback”不会被自动发现。

## 6. 推荐整改方案

### Phase 0: 先加诊断和失败测试

目标：下一轮不要继续靠人工肉眼猜偏移来源。

任务：

1. 新增慢 snapshot 测试：
   - Mock source 在 `snapshot()` 内 sleep。
   - 验证 timestamp 使用 poll start / captured_at，而不是 snapshot return time。
2. 新增 AX provider mock 测试：
   - 验证使用 system-wide root。
   - 验证 AX error code 进入 diagnostics。
   - 验证全部 fallback Arrow 会暴露诊断。
3. 新增 `CursorKindDiagnostics`：
   - query failure count
   - fallback Arrow count
   - Arrow/Hand/IBeam count
   - max/avg query duration
4. 新增 cursor timing diagnostics：
   - snapshot start timestamp
   - snapshot duration
   - record timestamp
   - nearest video frame delta
5. 新增 geometry diagnostics：
   - configured stream size
   - actual first CVPixelBuffer size
   - display frame
   - computed scale
   - raw x/y 和 normalized x/y 抽样。

验证：

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata_runtime
cargo test --manifest-path src-tauri/Cargo.toml cursor_kind
```

### Phase 1: 修复 BUG-0010 timestamp 错位

推荐最小变更：

1. 在 `CursorMetadataRuntime::spawn()` loop 中，调用 `source.snapshot()` 之前生成 timestamp。
2. `record_snapshot()` 使用该 timestamp。
3. 记录 `snapshot_duration` 到 diagnostics。

推荐稳健变更：

1. `CursorSnapshot` 新增 `captured_at: MediaTimestamp`。
2. `CursorSnapshotSource::snapshot()` 负责返回真实坐标采样时刻。
3. `MacCursorSource` 在 `CGEventGetLocation()` 附近记录 `captured_at`。
4. `CursorMetadataRecorder` 不再由外部传 timestamp，而是使用 snapshot 内部 timestamp。

注意：

- 不要把 AX query 的完成时间作为 cursor position 时间。
- kind 可以低频和缓存；position 不可以被低频化。
- 若实现 `captured_at` 需要让 `MacCursorSource` 持有 `SessionClock`，必须保持 App Logic / Native Module 边界清晰，不要让视频帧经过 JS。

验证门禁：

1. 慢 snapshot 测试通过。
2. 开启 AX 查询时，静止 cursor 不偏移。
3. 快速横向移动录制后，左右偏移显著消失。
4. `snapshot_duration_ms` 增大时，sample timestamp 不随之漂移。

### Phase 2: 修复 BUG-0011 AX provider

任务：

1. FFI 增加 `AXUIElementCreateSystemWide()`。
2. 替换 `AXUIElementCreateApplication(0)`。
3. 增加 `AXUIElementSetMessagingTimeout()`。
4. 记录 AX error code。
5. 增加 kind distribution diagnostics。
6. 查询失败时 fallback Arrow，但 diagnostics 必须可见。

建议分类策略：

```text
IBeam:
  - AXTextField
  - AXTextArea
  - editable text role / editable attribute

Hand:
  - AXButton
  - AXLink
  - AXMenuItem
  - current element or parent supports AXPress action

Fallback:
  - unknown role
  - no accessibility permission
  - AX query timeout / cannot complete
  - no value
```

父节点回溯：

- 对命中 `AXStaticText`、`AXGroup`、`AXImage`、`AXWebArea` 的场景，最多回溯 3-5 层 parent。
- 任意父节点支持 press action 或 link/button role，即判 Hand。
- 静态文本只有在明确 editable 时才判 IBeam。

验证门禁：

1. 普通桌面：Arrow count 增长。
2. 悬停按钮 / 链接：Hand count 增长，导出为 Hand。
3. 悬停文本框：IBeam count 增长，导出为 IBeam。
4. 关闭 Accessibility 权限：fallback Arrow，但 diagnostics 显示 failure count，不允许静默宣称通过。

### Phase 3: 修复/验证 SCK geometry

触发条件：

- Phase 1 修复 timestamp 后，BUG-0010 仍有固定 X 轴偏移。
- 或 manual gate 覆盖非 16:9 / Retina / 外接屏时仍失败。

任务：

1. 首帧到达时记录 actual `CVPixelBuffer` width/height。
2. 如果 actual frame size 与 `CaptureGeometry.stream_width/height` 不一致：
   - 更新 metadata 中的 source frame size。
   - 或 hard fail 并提示 geometry 不一致。
3. 查 SCK 是否可提供 content rect / scale factor / attachments；如有，改用实际 content rect。
4. 多显示器场景不要固定 `displays.objectAtIndex(0)`，应和实际录制目标 display 绑定。

验证门禁：

1. 1080p 主屏四角。
2. Retina 主屏四角和中心。
3. 非 16:9 显示器左右边缘。
4. 外接屏负 origin。
5. CenterCrop / FitWithBars 导出映射。

### Phase 4: Renderer 和 UI 补测试

任务：

1. `cursor_overlay.rs` 新增 rendered Hand / IBeam Y plane 像素测试。
2. 验证 hotspot 对齐：
   - Arrow tip 对齐 x/y。
   - Hand fingertip 对齐 x/y。
   - IBeam center hotspot 对齐 x/y。
3. 补 terminal error UI 测试：
   - terminal export-progress error 显示。
   - `isExporting` 清理。

验证：

```bash
cargo test --manifest-path src-tauri/Cargo.toml rendered_arrow
cargo test --manifest-path src-tauri/Cargo.toml hand_glyph
cargo test --manifest-path src-tauri/Cargo.toml ibeam_glyph
npm test -- --run
```

## 7. 建议整改顺序

1. **Task 1:** 新增慢 snapshot 失败测试，锁定 BUG-0010 timestamp 错位。
2. **Task 2:** 修复 timestamp 生成位置或让 `CursorSnapshot` 携带 `captured_at`。
3. **Task 3:** AX 查询限频/cache，并记录 snapshot duration。
4. **Task 4:** 新增 AX provider mock 测试，锁定 system-wide root。
5. **Task 5:** 使用 `AXUIElementCreateSystemWide()` 替换 `AXUIElementCreateApplication(0)`。
6. **Task 6:** 增加 AX messaging timeout、error code、kind distribution diagnostics。
7. **Task 7:** 扩展 CursorKind 分类：parent/action/editable。
8. **Task 8:** 增加 geometry actual frame diagnostics。
9. **Task 9:** 如果 Phase 1 后仍偏移，修复 SCK geometry 归一化。
10. **Task 10:** 补 renderer Hand/IBeam、hotspot、terminal error UI 测试。
11. **Task 11:** 更新 `BUG.md` 预防规则和人工验证结果。
12. **Task 12:** 更新 `HANDOFF.md` 工作记录。

## 8. 必须保留的人工验证门禁

BUG-0010：

1. 静止 cursor 放四角，导出 overlay 与目标点一致。
2. 快速水平移动 cursor，导出 overlay 不再随移动方向左/右漂。
3. 开启 AX 查询和关闭 AX 查询，对坐标定位不产生差异。
4. Retina 显示器四角和中心。
5. 外接显示器负 origin。
6. 关闭 cursor smoothing 做定位验收；开启 smoothing 只验证轨迹观感，不用于判定原始坐标是否准确。

BUG-0011：

1. 普通桌面：Arrow。
2. 按钮 / 链接：Hand。
3. 文本框：IBeam。
4. Accessibility 权限缺失：显示或记录明确 fallback diagnostics。
5. 导出 effect timeline 中必须出现非 Arrow kind 样本。
6. `hand_count > 0` 和 `ibeam_count > 0` 是人工验证通过的诊断前置条件。

## 9. `BUG.md` 预防规则复核

BUG-0010 已有规则复核：

1. cursor metadata 必须与 source video 一致坐标空间。
   - 当前部分满足，但 timestamp 错位会让“同一坐标空间”在时间维度上失效。
2. SCK display origin、contentRect、pointPixelScale、stream output size 必须进入归一化。
   - 当前 display origin 和 stream config 已进入；contentRect 和 actual output size 未充分证明。
3. glyph 绘制必须以 hotspot 对齐目标点。
   - 当前 renderer 已有 hotspot 概念，但 Hand/IBeam 实际渲染测试不足。
4. 多显示器、Retina、CenterCrop/FitWithBars 必须有坐标回归测试或人工门禁。
   - mapper 测试已有一部分；实际 SCK geometry 与动态时间对齐测试缺失。
5. CursorSample 和 CursorClick 必须处于同一 source video pixel 坐标空间。
   - 第二轮已修 click 坐标；仍需保证 timestamp 一致。

BUG-0011 已有规则复核：

1. cursor timeline 必须携带稳定 cursor kind 或 glyph 信息。
   - 字段通路满足；provider 产出不满足。
2. target-aware 查询失败必须 fallback Arrow，不能阻塞录制或导出。
   - fallback 有；同步查询和 stop join 风险仍需修。
3. cursor asset/bitmask 更新必须配套像素级回归测试。
   - Arrow 有较强测试；Hand/IBeam rendered-level 测试不足。
4. `CursorKind` enum 存在不代表 target-aware 已完成，必须有 kind source。
   - 当前 kind source 存在但 AX root 错误，实际行为未完成。
5. target-aware 查询失败必须有诊断计数。
   - 计数存在但未可见化，仍不满足。

## 10. 最终判断

本轮 review 判断：

- BUG-0010 不应继续优先改 Y 轴或 glyph hotspot；应优先修 cursor position timestamp 被 AX 查询延后的问题。
- BUG-0011 不应继续优先改 renderer；应优先修 AX provider root object、classification、diagnostics。
- 修复顺序应是：
  1. timestamp 正确性
  2. AX system-wide provider
  3. diagnostics 可见化
  4. SCK actual geometry 取证和必要修复
  5. renderer/UI 测试补齐

完成上述整改前，不建议合并第二轮 BUG-0010 / BUG-0011 修复为最终状态。
