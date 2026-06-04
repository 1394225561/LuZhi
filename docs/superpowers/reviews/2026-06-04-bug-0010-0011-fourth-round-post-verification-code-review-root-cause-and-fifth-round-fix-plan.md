# BUG-0010 / BUG-0011 第四轮人工验证失败后 Code Review、根因定位与第五轮整改方案

> 日期：2026-06-04
> 范围：第四轮整改 commit range `135cd135ca4443c4d0998b897bd914e027a8311a..692155ec881ad9d3f0b6f829cda795f43fc407ce`，并额外结合第四轮人工验证日志、真实 sidecar artifact 与当前工作区代码。
> 关联人工验证：`BUG.md` 中 `BUG-0010_7`、`BUG-0011_7`。
> 结论：第四轮不能合并。BUG-0010 / BUG-0011 的人工失败与当前代码缺口一致：关键诊断字段没有真实落盘，overlay PTS origin mapping 没有用同一 timebase，raw positioning mode 没有生产入口，AX hit-test 成功但命中菜单栏而非鼠标下元素，Accessibility 状态没有进入录制级门禁和 metadata。

---

## 1. 输入与审查目标

### 1.1 输入文件

- 运行时日志附件：`/Users/root-mac/.codex/attachments/ebc7b831-c430-42bf-8166-e4df692be94c/pasted-text.txt`
- 本次真实录制 artifact：
  - `/private/var/folders/h8/2vjd0nlx79q4m4q97xcdgh080000gn/T/luzhi-recordings/cursor-metadata-1780558391120-0.json`
  - `/private/var/folders/h8/2vjd0nlx79q4m4q97xcdgh080000gn/T/luzhi-recordings/cursor-effects-1780558393201-0.json`
  - `/private/var/folders/h8/2vjd0nlx79q4m4q97xcdgh080000gn/T/luzhi-recordings/recording-1780558372899-0.mp4`
  - `/private/var/folders/h8/2vjd0nlx79q4m4q97xcdgh080000gn/T/luzhi-recordings/recording-1780558372899-0-bilibili-export-1.mp4`

### 1.2 用户人工验证反馈

BUG-0010：

- 垂直方向定位正确。
- 水平方向仍按移动方向偏移：
  - 大幅向左移动后，静止光标向左偏。
  - 大幅向右移动后，静止光标向右偏。
- 期望：美化光标与源视频光标准确重合，无方向相关漂移。

BUG-0011：

- 完全没有改善。
- 导出视频任何情况下都是黑底白边 Arrow。
- Arrow 仍没有尾柄。
- 未出现 Hand / IBeam。

### 1.3 审查目标

1. 复核第四轮实现是否真正完成其计划中的 Critical / Important / Minor 项。
2. 按系统化调试流程定位第四轮后仍失败的真实根因。
3. 给出第五轮可执行整改方案、编码顺序、测试门禁和人工验证门禁。
4. 复核 `BUG.md` 预防规则是否真正闭环。

---

## 2. 总体结论

第四轮实现有若干基础改进：

1. `CursorKindProvider` 已注入 `MacCursorSource`。
2. `cursor_kind_diagnostics_merged()` 已把 AX 全局计数器合并到 recorder-local kind counts。
3. `AXIsProcessTrusted()` 已进入权限探测。
4. `cursor_kind.rs` 有 AX role chain 采样日志。
5. `cursor_overlay.rs` 的静态 Arrow bitmask 已添加尾柄像素。
6. `CursorEffectEngine` 有 raw positioning API 和单元测试。

但第四轮的核心目标没有在真实生产路径闭环：

1. `MediaTimelineDiagnostics` / `CursorTimingDiagnostics` 只定义了结构体和序列化测试，真实录制 sidecar 中仍为 `null`。
2. `sourcePtsOriginNanos` 在真实 metadata/effect timeline 中仍为 `0`。
3. 源 MP4 实际首帧 PTS 非 0，`ffprobe` 显示 `start_time=0.166016`，但导出 overlay 没有减掉这个 origin。
4. `lib.rs` 传给 exporter 的 origin 语义错误：它试图把 raw CMSampleBuffer host-time PTS 当成 source MP4 demux PTS origin 使用，二者不是同一个 timebase。
5. raw positioning mode 没有前端/后端生产入口。用户关闭 smoothing 和 magnification 时，当前代码会让 SCK 录入系统原始光标，而不是保持系统光标隐藏并只渲染 raw glyph overlay。
6. AX 查询日志显示查询成功但始终命中 `AXMenuBar` / `AXApplication`，说明当前传入 AX hit-test 的 point 很可能没有被转换到 AX 实际命中所需坐标空间。
7. Accessibility 目前只是 UI 提示，不是录制前门禁；权限状态没有写入真实 metadata。
8. Arrow 尾柄只测了静态 bitmask，没有验证导出渲染后的 Y-plane 像素，因此无法解释人工仍看到“无尾柄”。

合并评估：

- **Ready to merge? No**
- 原因：第四轮关键修复没有真实覆盖人工验收路径，且已有真实 artifact 证明 PTS origin 和 diagnostics 没有落盘。

---

## 3. 关键运行时证据

### 3.1 运行时日志证据

日志片段：

```text
[cursor-geometry] display_id=1 frame_origin=(0.0, 0.0) frame_size=(1512.0×982.0) stream=1920×1080 point_pixel_scale=1.27
[sck-first-frame] actual_buffer=1920×1080 bytes_per_row=7680
[cursor-kind-classify] point=(479,945) kind=Arrow role_chain=["AXMenuBar", "AXApplication"] ax_press=false rc=0 dur=17208µs
[cursor-kind-classify] point=(837,70) kind=Arrow role_chain=["AXMenuBar", "AXApplication"] ax_press=false rc=0 dur=3331µs
[cursor-kind-classify] point=(774,427) kind=Arrow role_chain=["AXMenuBar", "AXApplication"] ax_press=false rc=0 dur=711µs
...
[cursor-kind-diagnostics] arrow=471 hand=0 ibeam=0 ax_fail=83 ax_fallback_arrow=75
[cursor-kind-diagnostics] ⚠️ 未检测到 Hand/IBeam，AX 查询存在失败
```

解读：

1. `rc=0` 说明 AX hit-test 在这些采样中不是单纯权限失败。
2. 但 role chain 始终是 `AXMenuBar` / `AXApplication`，不随鼠标悬停按钮、链接、输入框变化。
3. 这更符合“AX 查询成功但查询点落到了错误的屏幕位置”，而不是“Accessibility 未授权导致全部失败”。
4. 当前日志没有记录 raw point、转换前后 point、display bounds、AX 命中元素 frame，无法证明坐标语义。

### 3.2 cursor metadata sidecar 证据

命令：

```bash
jq '{fps, durationNanos, sampleCount:(.cursorSamples|length), firstSample:.cursorSamples[0], lastSample:.cursorSamples[-1], beautifyConfig, captureGeometry, cursorKindDiagnostics, mediaTimelineDiagnostics, cursorTimingDiagnostics, sourcePtsOriginNanos}' \
  /private/var/folders/h8/2vjd0nlx79q4m4q97xcdgh080000gn/T/luzhi-recordings/cursor-metadata-1780558391120-0.json
```

关键输出：

```json
{
  "fps": 30,
  "durationNanos": 18131441500,
  "sampleCount": 471,
  "firstSample": {
    "timestamp": { "nanos": 350871958 },
    "x": 963.51196,
    "y": 624.33203,
    "kind": "arrow"
  },
  "lastSample": {
    "timestamp": { "nanos": 18093070875 },
    "x": 684.42957,
    "y": 137.74089,
    "kind": "arrow"
  },
  "cursorKindDiagnostics": {
    "axQueryFailureCount": 83,
    "axFallbackArrowCount": 75,
    "arrowCount": 471,
    "handCount": 0,
    "ibeamCount": 0
  },
  "mediaTimelineDiagnostics": null,
  "cursorTimingDiagnostics": null,
  "sourcePtsOriginNanos": 0
}
```

解读：

1. 真实 sidecar 没有 `mediaTimelineDiagnostics`。
2. 真实 sidecar 没有 `cursorTimingDiagnostics`。
3. `sourcePtsOriginNanos` 仍是 `0`。
4. 首个 cursor sample 是 `350,871,958ns`，不是 `0`。
5. `kind` 分布为 100% Arrow。

### 3.3 effect timeline 证据

命令：

```bash
jq '{fps, durationNanos, frameCount:(.frames|length), firstFrame:.frames[0], lastFrame:.frames[-1], clickEffects:(.clickEffects|length), rawSystemCursorVisible, renderCursorOverlay, sourcePtsOriginNanos}' \
  /private/var/folders/h8/2vjd0nlx79q4m4q97xcdgh080000gn/T/luzhi-recordings/cursor-effects-1780558393201-0.json
```

关键输出：

```json
{
  "fps": 30,
  "durationNanos": 18131441500,
  "frameCount": 544,
  "firstFrame": {
    "timestamp": { "nanos": 0 },
    "x": 963.51196,
    "y": 624.33203,
    "scale": 1.0,
    "opacity": 1.0,
    "kind": "arrow"
  },
  "lastFrame": {
    "timestamp": { "nanos": 18099999819 },
    "x": 684.4295,
    "y": 137.74089,
    "scale": 1.9208685,
    "opacity": 1.0,
    "kind": "arrow"
  },
  "clickEffects": 7,
  "rawSystemCursorVisible": false,
  "renderCursorOverlay": true,
  "sourcePtsOriginNanos": 0
}
```

解读：

1. timeline 从 `0` 开始，但真实 cursor sample 首样本从 `350ms` 左右开始。
2. timeline origin 仍为 `0`。
3. 当前导出不是 raw positioning：存在 `clickEffects=7`，末帧 `scale=1.9208685`。
4. 所有 timeline frames 仍是 Arrow。

### 3.4 source MP4 PTS 证据

命令：

```bash
ffprobe -v error -select_streams v:0 \
  -show_entries stream=time_base,start_pts,start_time,duration_ts,duration,avg_frame_rate,nb_frames,width,height \
  -of json \
  /private/var/folders/h8/2vjd0nlx79q4m4q97xcdgh080000gn/T/luzhi-recordings/recording-1780558372899-0.mp4
```

关键输出：

```json
{
  "width": 1920,
  "height": 1080,
  "avg_frame_rate": "8085/274",
  "time_base": "1/15360",
  "start_pts": 2550,
  "start_time": "0.166016",
  "duration_ts": 280576,
  "duration": "18.266667",
  "nb_frames": "539"
}
```

首帧列表：

```json
{
  "frames": [
    {
      "pts": 2550,
      "pts_time": "0.166016",
      "best_effort_timestamp": 2550,
      "best_effort_timestamp_time": "0.166016"
    },
    {
      "pts": 3062,
      "pts_time": "0.199349",
      "best_effort_timestamp": 3062,
      "best_effort_timestamp_time": "0.199349"
    }
  ]
}
```

解读：

1. 第四轮计划中担心的 “source MP4 第一帧 PTS 不为 0” 在真实 artifact 中已经发生。
2. 当前 effect timeline `sourcePtsOriginNanos=0`，所以 `trim_exporter` 实际没有减掉 `166ms` 的 source MP4 demux origin。
3. 水平移动时，166ms 的 overlay query 时间偏差会表现为方向相关偏移：
   - 如果 overlay 查询了偏晚/偏早的 cursor frame，向右移动时视觉上会偏右或偏左。
   - 用户反馈“向左移动后左偏、向右移动后右偏”符合时间轴错位或平滑/Bezier 污染，而不是固定几何比例错误。

---

## 4. Code Review Findings

### 4.1 Critical 1：MediaTimelineDiagnostics / CursorTimingDiagnostics 没有真实写入 sidecar

文件：

- `src-tauri/src/app/cursor_metadata_runtime.rs:240-262`
- `src-tauri/src/platform/macos_service.rs:491-501`
- `src-tauri/src/media/recording_metadata.rs:50-113`

问题：

`RecordingMetadata` 新增了 `MediaTimelineDiagnostics` / `CursorTimingDiagnostics`，但 `CursorMetadataRecorder.finish()` 仍固定：

```rust
media_timeline_diagnostics: None,
cursor_timing_diagnostics: None,
source_pts_origin_nanos: 0,
```

`macos_service.rs` 的 `write_sidecars()` 直接把这份 metadata 写入磁盘，没有在 stop 阶段补充 timeline/cursor diagnostics。

影响：

1. 违反 `BUG.md` BUG-0010 预防规则 11 / 12 / 14。
2. 第四轮声明“diagnostics 落盘”与真实 artifact 不符。
3. 无法证明视频帧、cursor sample、effect timeline、export overlay 使用同一 source media timebase。
4. 真实 sidecar 已证实这些字段为 `null`。

修复方向：

1. 录制开始时建立诊断会话上下文。
2. `CursorMetadataRecorder` 持续记录：
   - first/last cursor sample nanos
   - sample interval min/max/avg
   - sample count
   - samples outside geometry count
   - Accessibility permission at start
3. stop 时从 `MacScreenCapture` 获取：
   - first frame raw CMSampleBuffer PTS
   - session entry nanos at first frame
   - actual CVPixelBuffer size
4. 从 writer 或 source artifact inspection 获取：
   - source MP4 first demux PTS nanos
   - source MP4 stream time base
   - first/last video normalized nanos
   - frame count
5. 在写 sidecar 前合并进 `RecordingMetadata`。

### 4.2 Critical 2：overlay PTS origin mapping 使用了错误的 timebase

文件：

- `src-tauri/src/lib.rs:517-519`
- `src-tauri/src/media/trim_exporter.rs:913-930`
- `src-tauri/src/core/timeline.rs:100-105`

问题：

`lib.rs` 当前把：

```rust
timeline.source_pts_origin_nanos = mtd.first_video_pts_nanos_raw;
```

传给 `trim_exporter`。但 `mtd.first_video_pts_nanos_raw` 是 CMSampleBuffer host-time PTS，而 `trim_exporter` 中的：

```rust
let decoded_nanos = time_base_units_to_nanos(raw_pts, video_time_base)
```

来自 source MP4 demux PTS。host-time PTS 与 source MP4 demux PTS 不是同一 timebase。

当前真实 artifact 中 metadata 未填，所以 origin 是 `0`。如果未来按当前代码把 raw host PTS 填进去，则会出现更严重问题：source MP4 demux PTS 只有数百毫秒到几十秒量级，raw host PTS 可能是系统 host clock 大值，`saturating_sub()` 会把大量帧压成 0。

影响：

1. 第四轮 Critical 目标“overlay timestamp 显式 origin mapping”没有成立。
2. 真实 source MP4 `start_time=0.166016s`，但 timeline origin 为 0，导致 overlay 至少有 166ms 的 source PTS offset 风险。
3. 移动场景下会表现为方向相关水平偏移。

修复方向：

1. 不要把 raw CMSampleBuffer host-time PTS 作为 export overlay origin。
2. `trim_exporter` 在导出前读取实际 source MP4 第一帧 demux PTS：
   - `source_first_pts_in_video_tb`
   - `source_first_pts_nanos = time_base_units_to_nanos(source_first_pts_in_video_tb, video_time_base)`
3. overlay query timestamp 应明确计算为：

```text
source_cursor_nanos = decoded_frame_demux_nanos - source_first_demux_pts_nanos + cursor_timeline_origin_nanos
```

MVP 当前 cursor timeline origin 可先约束为 `0`，但必须显式写入 metadata/timeline，并由测试验证。

4. 进度计算如果使用 source timestamp，也要同步使用相同 origin mapping，避免 progress 与 overlay 各用一套时间。

### 4.3 Critical 3：raw positioning mode 没有生产入口

文件：

- `src-tauri/src/media/cursor_engine.rs:394-405`
- `src-tauri/src/lib.rs:159-165`
- `src-tauri/src/lib.rs:472-502`
- `src/lib/tauri.ts:37-43`
- `src/components/preview-view.tsx:74-79`

问题：

`CursorEffectEngine::raw_positioning()` 只在单元测试中可达。生产路径中：

1. 前端 `BeautifyConfig` 没有 `rawCursorPositioning` 或等价诊断 flag。
2. `build_effect_timeline_from_metadata()` 不会选择 `CursorEffectEngine::raw_positioning()`。
3. 用户关闭 smoothing + magnification 时，`start_recording()` 会设置：

```rust
config.show_system_cursor = !(cursor_magnification || cursor_smoothing);
```

也就是 SCK 录制系统原始光标，而不是“隐藏系统光标 + 渲染 raw glyph overlay”。

影响：

1. 第四轮人工验收清单中的 raw positioning 实际不可执行。
2. 关闭 smoothing/magnification 并不等于 raw overlay 验收。
3. BUG-0010 定位仍可能被 Bezier interpolation、click scale、系统原始 cursor 可见性混淆。

修复方向：

1. 增加独立诊断配置：`rawCursorPositioning`。
2. raw positioning 启用时：
   - 录制阶段：`show_system_cursor=false`
   - timeline 构建：`CursorEffectEngine::raw_positioning()`
   - clicks：传空数组或引擎强制忽略 click effects
   - renderer：仍渲染 glyph
3. raw mode 不应绑定到 `cursorMagnification=false && cursorSmoothing=false`，否则无法区分“用户想录入系统光标”和“开发者要验证 overlay 坐标”。

### 4.4 Critical 4：raw linear interpolation 对真实首样本 > 0 不安全

文件：

- `src-tauri/src/media/cursor_engine.rs:494-530`

问题：

`linear_interpolate_frames()` 从 `t=0` 开始生成 frame，但真实 cursor samples 首样本往往不是 0。本次真实 sidecar 首样本为：

```json
"firstSample": {
  "timestamp": { "nanos": 350871958 }
}
```

当前实现中：

```rust
let alpha = (t - current_ts) as f32 / (next_ts - current_ts) as f32;
```

当 `t < current_ts` 时：

- debug build：u64 subtraction 可能 panic。
- release build：可能 wrap 成极大值，再被 clamp 到 1.0，导致早期 frames 错误使用 next sample。

影响：

1. 即使第五轮接入 raw positioning，也会在真实素材中出错。
2. 早期静止/起始阶段可能产生错误坐标，污染“开始是否准确”的验收。

修复方向：

1. 对 `t <= first_sample.timestamp.nanos` 显式 hold 第一条 sample。
2. 对 `t >= last_sample.timestamp.nanos` hold 最后一条 sample。
3. 插值只在 `[current_ts, next_ts]` 闭区间内执行。
4. 所有 delta 用 `saturating_sub()` 或显式比较。

### 4.5 Critical 5：AX hit-test 查询成功但命中错误元素

文件：

- `src-tauri/src/platform/macos/cursor_source.rs:91-99`
- `src-tauri/src/platform/macos/cursor_kind.rs:270-337`
- `src-tauri/src/app/cursor_metadata_runtime.rs:147-160`

问题：

`MacCursorSource` 当前把 `CGEventGetLocation()` 的 `point.x/point.y` 直接传给 `CursorKindProvider`：

```rust
let kind = self.kind_provider.query(point.x as f32, point.y as f32);
```

而 cursor position normalization 是在 recorder 中完成的：

```rust
mapper.map(snapshot.x, snapshot.y)
```

AX kind query 没有使用任何 capture geometry 或 AX-specific coordinate mapper。运行日志显示查询成功但始终命中菜单栏：

```text
kind=Arrow role_chain=["AXMenuBar", "AXApplication"] rc=0
```

影响：

1. BUG-0011 仍会 100% Arrow。
2. Accessibility 授权、provider 注入、parent chain 分类都无法发挥作用，因为 hit-test 的输入点已经错了。
3. 当前日志不能证明 AX 期望坐标与 CGEvent/SCK 坐标一致。

修复方向：

1. 不要继续假设 `CGEventGetLocation()`、`SCDisplay.frame()`、`AXUIElementCopyElementAtPosition()` 三者坐标语义一致。
2. 增加 `AxCoordinateMapper` 或等价转换层，输入包括：
   - raw CGEvent point
   - selected display frame
   - main display height / global screen bounds
   - capture geometry
3. 第五轮第一步先做诊断，不直接修：
   - 同一采样日志中记录 raw point、candidate AX point、display bounds、role chain、element frame。
   - 在 Tauri 按钮、浏览器链接、文本框上人工采样，确认哪个 candidate 命中正确 role。
4. 确认后再固定转换公式。

### 4.6 Critical 6：Accessibility 不是录制前门禁，也没有写入 metadata

文件：

- `src-tauri/src/platform/macos/permissions.rs:27-38`
- `src-tauri/src/lib.rs:134-180`
- `src/App.tsx:322-326`
- `src-tauri/src/media/recording_metadata.rs:90-101`

问题：

Accessibility 权限只做了 UI 提示：

```tsx
需要辅助功能权限才能识别手形/文本光标。当前将显示标准箭头。
```

但开始录制时没有：

1. 读取当前 Accessibility 状态。
2. 将状态传给 cursor metadata runtime。
3. 写入 `CursorTimingDiagnostics.accessibility_permission_at_start`。
4. 在 target-aware cursor 验收模式下阻断或明确降级。

影响：

1. 违反 `BUG.md` BUG-0011 规则 15。
2. 未授权时仍可静默录制出全 Arrow。
3. sidecar 无法证明“全 Arrow 是未授权、AX 坐标错误、分类规则错误，还是 renderer 问题”。

修复方向：

1. `start_recording()` 时读取 `RecordingPermissions.accessibility`。
2. 传入 `MacRecordingService::start()` / `CursorMetadataRuntime::spawn()`。
3. 写入 `CursorTimingDiagnostics.accessibility_permission_at_start`。
4. 如果开启 target-aware cursor 验收且未授权：
   - 推荐阻断录制并返回明确错误；
   - 或在 metadata 中写入 `accessibility=notDetermined`，并在 UI/export summary 中明确“本次 cursor kind 已降级为 Arrow”。

### 4.7 Important 1：AX counters 是进程全局累计，不是本次录制 delta

文件：

- `src-tauri/src/platform/macos/cursor_kind.rs:34-43`
- `src-tauri/src/platform/macos/cursor_kind.rs:141-153`
- `src-tauri/src/platform/macos_service.rs:170-236`

问题：

`AX_QUERY_FAILURE_COUNT`、`AX_FALLBACK_ARROW_COUNT`、`KIND_*_COUNT` 是进程级全局 atomics。录制开始时没有 reset，也没有记录 start snapshot，停止时写入的是进程累计值。

影响：

1. 多轮录制后 metadata 不能代表本次录制。
2. 第五轮人工验证可能被历史失败污染。
3. 违反“diagnostics 必须来自实际 provider 并写入 RecordingMetadata”的精神。

修复方向：

1. 录制开始 snapshot 全局 counters。
2. 停止时 snapshot 全局 counters。
3. sidecar 写入 delta。
4. 更稳妥方案：将 counters 放到 `MacCursorKindProvider` 实例中，由 provider 随 runtime 生命周期结束一起返回 diagnostics。

### 4.8 Important 2：AX 失败路径没有足够日志

文件：

- `src-tauri/src/platform/macos/cursor_kind.rs:301-304`

问题：

当 `AXUIElementCopyElementAtPosition()` 返回非 0 或 element null 时，代码直接：

```rust
AX_QUERY_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
return CursorKind::Arrow;
```

没有限频记录：

- result code
- point
- query duration
- permission state
- candidate coordinate

影响：

1. 未授权、timeout、坐标越界、屏幕空间错误都混成 Arrow。
2. 人工日志仍难以回答“为什么 Hand/IBeam 没出现”。

修复方向：

失败路径也进入统一 `AxClassificationLog`，role_chain 可为空，但必须记录 rc、point、duration、permission。

### 4.9 Important 3：Arrow 尾柄测试没有覆盖导出渲染结果

文件：

- `src-tauri/src/media/cursor_overlay.rs:1127-1158`

问题：

`arrow_glyph_has_tail_handle_and_hotspot_still_at_tip()` 只检查静态 `ARROW_GLYPH` bitmask。它不能证明：

1. `CursorOverlayRenderer` 实际画出了 tail。
2. FitWithBars/CenterCrop 后 tail 仍可见。
3. scale/hotspot/clamp 后 tail 没被裁掉。
4. export 使用的是新 timeline / 新 renderer。

影响：

人工仍看到“无尾柄”时，当前测试无法定位是 bitmask、renderer、scale、crop、旧二进制、还是视频视觉尺度问题。

修复方向：

1. 新增 rendered Y-plane tail 测试。
2. 至少覆盖：
   - `scale=1.0`
   - FitWithBars
   - CenterCrop
   - cursor 在中间区域，避免边缘裁剪影响
3. 断言 tail handle body/outline 的具体 Y 值区域。

### 4.10 Minor：AX 日志限频与计划不一致

文件：

- `src-tauri/src/platform/macos/cursor_kind.rs:244-246`

问题：

第四轮计划要求“每秒最多 2 条”，当前实现为：

```rust
const LOG_INTERVAL: Duration = Duration::from_secs(1);
```

即每秒最多 1 条。

影响：

不影响功能，但人工快速悬停按钮/文本框时采样证据偏少。

修复方向：

改为 500ms interval 或 token bucket。

---

## 5. BUG-0010 系统化根因定位

### 5.1 已排除或降低优先级的问题

1. Y 轴无条件翻转：已移除，用户反馈 Y 方向正确。
2. AX 查询耗时污染 position timestamp：`captured_at_nanos` 已在 `CGEventGetLocation()` 后立即记录，第三轮后精度有改善。
3. 纯几何 scale 错误：如果是几何错误，偏移通常是固定比例或固定方向；用户反馈是按水平移动方向偏移，更像时间错位或平滑轨迹污染。

### 5.2 当前最高置信根因：source MP4 demux PTS origin 没有进入 overlay timebase

数据流：

```text
ScreenCaptureKit callback
  -> CMSampleBuffer raw PTS
  -> normalize_pts(..., SessionClock)
  -> VideoFrame.timestamp
  -> FFmpeg writer encodes source MP4
  -> source MP4 demux PTS has start_time=0.166016s

Cursor runtime
  -> CGEventGetLocation()
  -> SessionClock.elapsed_nanos()
  -> CursorSample.timestamp
  -> EffectTimeline.frames timestamp from 0..duration

Export
  -> decode source MP4 raw_pts
  -> decoded_nanos = raw_pts in source MP4 time_base
  -> source_nanos = decoded_nanos - timeline.source_pts_origin_nanos
  -> overlay.find_cursor_frame(source_nanos)
```

当前断裂点：

1. source MP4 demux PTS origin 是 `166.016ms`。
2. `timeline.source_pts_origin_nanos` 是 `0`。
3. 因此 export overlay 用 `decoded_nanos` 直接查询 cursor timeline。
4. cursor timeline 的第一帧从 `0` 开始，真实 cursor sample 第一条从 `350.872ms` 开始。

这会导致：

1. 起始静止阶段看起来可能还可以，因为 cursor 没动。
2. 水平移动后，overlay 查询的 cursor frame 与源视频实际 frame 不同步。
3. 当移动方向稳定时，时间错位会表现为稳定方向偏差。

### 5.3 第二层根因：raw positioning 验收实际上没有隔离 smoothing / Bezier / click scale

当前用户关闭 smoothing/magnification 不等于 raw overlay：

1. 如果录制前关闭这两个开关，系统 cursor 会被 SCK 录入源视频。
2. 如果导出前关闭这两个开关，`build_effect_timeline_from_metadata()` 仍可能用 Bezier path 生成 baseline frames。
3. 当前真实 effect timeline 中 `clickEffects=7`，末帧 `scale=1.9208685`，证明本次导出不是 raw positioning 验收。

结论：

BUG-0010 必须先通过一个真正的 raw overlay 模式验证：

- source video 不包含系统 cursor。
- overlay 只画 glyph。
- 不使用 smoothing。
- 不使用 Bezier。
- 不使用 click magnification。
- 不使用 output timeline PTS 查询 cursor，必须使用 source timeline PTS。

---

## 6. BUG-0011 系统化根因定位

### 6.1 已排除或降低优先级的问题

1. `CursorSnapshot.kind` 字段缺失：已存在。
2. recorder 固定写 Arrow：已改为使用 `snapshot.kind`。
3. renderer 固定画圆点：已改为按 `CursorKind` 选 glyph。
4. AX root 使用 `AXUIElementCreateApplication(0)`：已改为 `AXUIElementCreateSystemWide()`。
5. Accessibility 完全未授权：不能作为当前最高置信解释，因为日志中多次 `rc=0` 且返回了 role chain。

### 6.2 当前最高置信根因：AX hit-test 的 point 坐标语义错误或未验证

证据：

```text
[cursor-kind-classify] point=(479,945) kind=Arrow role_chain=["AXMenuBar", "AXApplication"] rc=0
[cursor-kind-classify] point=(837,70) kind=Arrow role_chain=["AXMenuBar", "AXApplication"] rc=0
[cursor-kind-classify] point=(774,427) kind=Arrow role_chain=["AXMenuBar", "AXApplication"] rc=0
```

这些点的 Y 值横跨 70、427、945，但 role chain 仍持续命中 `AXMenuBar`。这不符合正常鼠标悬停按钮/链接/文本框的命中结果。

可能原因：

1. `CGEventGetLocation()` 返回的坐标与 AX hit-test 使用的 screen coordinate 不是同一语义。
2. 多显示器或 Retina 缩放下，AX 需要 point-space 坐标，但传入值可能已经按另一空间解释。
3. `SCDisplay.frame()` 与 AX global screen bounds 的 origin/height 不一致。
4. Tauri/WebView 内容区的 AX hierarchy 需要更深 parent chain 或使用 attribute/action 辅助，但当前日志显示根本没有命中内容区，优先级低于坐标问题。

### 6.3 第二层根因：诊断仍不足

当前日志没有记录：

- Accessibility permission at recording start。
- AX failure result code 分布。
- raw CGEvent point 与 AX candidate point 对照。
- 命中 AX element 的 frame / role / subrole / title / actions。
- 本次录制 delta counters。

因此第五轮需要先用诊断证明“坐标在哪一层错”，再固定修复公式。

---

## 7. BUG.md 预防规则复核

### 7.1 BUG-0010 规则复核

| 规则                                                               | 状态     | 说明                                                           |
| ------------------------------------------------------------------ | -------- | -------------------------------------------------------------- |
| 1. cursor metadata 必须与 source video 坐标空间一致                | 部分闭环 | 几何归一化已有，但 source video timebase 未闭环                |
| 2. SCK display origin/contentRect/scale/stream size 必须进入归一化 | 部分闭环 | `CaptureGeometry` 存在，但 AX kind query 没用坐标 mapper       |
| 3. glyph 必须 hotspot 对齐                                         | 部分闭环 | bitmask 有 hotspot，但 rendered/export 级测试不足              |
| 4. 多显示器/Retina/Crop/Fit 必须测试                               | 部分闭环 | mapper 单测有，真实导出和 AX 仍不足                            |
| 5. mapper 测试不能只测中心点                                       | 已改善   | 第二轮已补                                                     |
| 6. Y 轴翻转必须有设备证据                                          | 已改善   | 硬编码 flip 已移除                                             |
| 7. CursorSample 和 CursorClick 同坐标空间                          | 部分闭环 | 当前 click 坐标归一化，但 timeline timebase 不闭环             |
| 8. 阻塞操作不得污染 timestamp                                      | 已改善   | `captured_at_nanos` 已前置                                     |
| 9. position 和 kind 时间精度分离                                   | 部分闭环 | kind TTL 存在，但 AX 坐标错误导致分类无效                      |
| 10. 首帧 actual size 必须记录并对比                                | 未闭环   | 只打印 stderr，真实 metadata 没有 diagnostics                  |
| 11. video/cursor/effect/export 必须共享 source timebase            | 未闭环   | 真实 artifact 证明 source PTS origin 未传入                    |
| 12. source 首帧 PTS 必须诊断对比                                   | 未闭环   | `ffprobe` 有非 0，但 sidecar 没有                              |
| 13. 定位验收必须先 raw positioning                                 | 未闭环   | raw mode 无生产入口                                            |
| 14. cursor diagnostics 必须落盘                                    | 未闭环   | `mediaTimelineDiagnostics` / `cursorTimingDiagnostics` 为 null |

### 7.2 BUG-0011 规则复核

| 规则                                                          | 状态           | 说明                                              |
| ------------------------------------------------------------- | -------------- | ------------------------------------------------- |
| 1. cursor timeline 必须携带 kind/glyph                        | 部分闭环       | 字段存在，但真实 kind 全 Arrow                    |
| 2. target-aware 失败 fallback Arrow 但不能阻塞                | 已满足基础行为 | fallback 存在                                     |
| 3. glyph 必须有 hotspot metadata                              | 部分闭环       | 静态测试有，rendered/export 级不足                |
| 4. glyph 更新必须配套像素测试                                 | 部分闭环       | bitmask 测试有，导出渲染测试不足                  |
| 8. enum 存在不代表 target-aware 完成                          | 未闭环         | 真实 kind source 仍无效                           |
| 9. renderer 测试必须验证 glyph 类型和颜色                     | 部分闭环       | rendered tail 不足                                |
| 10. fallback 不能掩盖全部未识别                               | 部分闭环       | diagnostics 有 AX counts，但没有本次 delta 和权限 |
| 11. AX 必须用 system-wide root                                | 已改善         | 已使用 `AXUIElementCreateSystemWide()`            |
| 12. AX 查询必须 timeout                                       | 已改善         | 50ms timeout 存在                                 |
| 13. parent chain 至少 3 层                                    | 已改善         | 当前回溯 3 层                                     |
| 14. AXStaticText 不等于 editable text                         | 已改善         | 分类规则已收敛                                    |
| 15. Accessibility 必须进入门禁和 metadata                     | 未闭环         | UI 提示有，录制门禁和 metadata 无                 |
| 16. diagnostics 必须来自实际 provider 并写入 metadata         | 部分闭环       | AX counts 合并了，但为进程累计，不是本次 delta    |
| 17. Provider 必须注入生产路径                                 | 已改善         | `MacCursorSource` 使用 provider                   |
| 18. target-aware 整改必须提供 role/action/classification 证据 | 部分闭环       | 有成功日志，但失败路径和坐标证据不足              |
| 19. glyph 形状变更必须配套 hotspot/颜色/关键区域测试          | 部分闭环       | 缺 rendered/export 级 tail 测试                   |

---

## 8. 第五轮整改方案

> 原则：不要继续叠加猜测式修复。第五轮必须先把真实诊断落盘，再按证据修 timebase、raw mode、AX 坐标。每个 Phase 都需要在 `tests/` 目录新增自测清单。

### Phase 0：新增第五轮自测清单

目标：

- 先生成 `tests/2026-06-04-bug-0010-0011-fifth-round-checklist.md`。
- 清单必须覆盖自动测试、artifact 检查、人工验收。

建议内容：

1. 自动测试：
   - `cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata_runtime`
   - `cargo test --manifest-path src-tauri/Cargo.toml cursor_engine`
   - `cargo test --manifest-path src-tauri/Cargo.toml cursor_overlay`
   - `cargo test --manifest-path src-tauri/Cargo.toml trim_exporter --features ffmpeg`
   - `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`
   - `npm test -- --run`
2. artifact 检查：
   - cursor metadata 中 `mediaTimelineDiagnostics != null`
   - cursor metadata 中 `cursorTimingDiagnostics != null`
   - cursor metadata 中 `cursorTimingDiagnostics.accessibilityPermissionAtStart` 有真实值
   - effect timeline 中 `sourcePtsOriginNanos` 等于 source MP4 first demux PTS nanos，或导出端另有明确的 demux origin 诊断字段
   - source MP4 `ffprobe start_time` 非 0 时，overlay query 仍使用减 origin 后 timestamp
3. 人工验收：
   - raw overlay 静止四角/中心
   - raw overlay 左右匀速移动
   - raw overlay 多段移动
   - Accessibility granted 后 Button/Link -> Hand
   - Accessibility granted 后 TextField/TextArea -> IBeam
   - Arrow rendered tail 可见

### Phase 1：真实 diagnostics 落盘

目标：

让真实 `cursor-metadata-*.json` 能解释本次录制，不再只有结构体和 stderr。

涉及文件：

- `src-tauri/src/app/cursor_metadata_runtime.rs`
- `src-tauri/src/media/recording_metadata.rs`
- `src-tauri/src/platform/macos_service.rs`
- `src-tauri/src/platform/macos/screen_capture_kit.rs`
- `src-tauri/src/media/recording_writer.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`

关键改动：

1. `CursorMetadataRecorder` 新增统计字段：
   - `first_sample_nanos: Option<u64>`
   - `last_sample_nanos: Option<u64>`
   - `prev_sample_nanos: Option<u64>`
   - `sample_interval_min_nanos: Option<u64>`
   - `sample_interval_max_nanos: u64`
   - `sample_interval_sum_nanos: u128`
   - `sample_interval_count: u64`
   - `samples_outside_geometry: u64`
2. `record_snapshot()` 在 `mapper.map()` 返回 `None` 时递增 `samples_outside_geometry`。
3. `finish()` 接收或后置合并：
   - `accessibility_permission_at_start`
   - `media_timeline_diagnostics`
   - AX counters delta
4. `MacRecordingService::start()` 在录制开始 snapshot：
   - Accessibility permission
   - AX global counters start snapshot
5. `MacRecordingService::stop()` / `write_sidecars()` 在写 metadata 前填充：
   - `cursorTimingDiagnostics`
   - `mediaTimelineDiagnostics`
   - `cursorKindDiagnostics` delta

验收：

- 新录制 sidecar 中不允许出现：

```json
"mediaTimelineDiagnostics": null
"cursorTimingDiagnostics": null
```

- stop 日志必须打印完整摘要：

```text
[media-timeline-diagnostics] source_first_demux_pts=... first_cursor=... last_cursor=... frames=...
[cursor-timing-diagnostics] min=... max=... avg=... outside=... accessibility=...
```

### Phase 2：修正 PTS origin mapping，统一 source MP4 demux timebase

目标：

让 export overlay 的 source timestamp 与 source MP4 demux PTS 使用同一 timebase，避免 166ms 或类似 offset。

涉及文件：

- `src-tauri/src/media/trim_exporter.rs`
- `src-tauri/src/media/cursor_overlay.rs`
- `src-tauri/src/core/timeline.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/media/recording_metadata.rs`

关键改动：

1. `trim_exporter` 在导出时读取 source MP4 实际 first decoded video PTS。
2. first PTS 只用 `video_time_base` 转成 nanos。
3. overlay source timestamp 计算改为：

```rust
let decoded_nanos = time_base_units_to_nanos(raw_pts, video_time_base)
    .unwrap_or(0)
    .max(0) as u64;
let source_nanos = decoded_nanos
    .saturating_sub(source_first_demux_pts_nanos)
    .saturating_add(cursor_timeline_origin_nanos);
```

4. MVP 可先固定 `cursor_timeline_origin_nanos=0`，但要明确写在 `EffectTimeline` 或 exporter request diagnostics 里。
5. 不再使用 `MediaTimelineDiagnostics.first_video_pts_nanos_raw` 作为 `sourcePtsOriginNanos`。

新增测试：

1. `overlay_uses_demux_pts_origin_when_input_pts_is_non_zero`
   - 模拟 decoded frame PTS 从 200ms 开始。
   - cursor timeline 从 0 开始。
   - decoded 200ms 应查询 cursor 0ms。
2. `overlay_preserves_source_timestamp_after_cut_segments`
   - cut timeline 有多个 keep segments。
   - output PTS 连续，但 overlay 查询仍使用 source timestamp。
3. `source_pts_origin_does_not_use_raw_cmsamplebuffer_host_pts`
   - 防止再次把 host-time raw PTS 塞进 demux origin。

验收：

- 对本次真实 artifact，`ffprobe start_time=0.166016` 时，导出日志必须显示：

```text
[overlay-timing] source_first_demux_pts_nanos=166016000 decoded_nanos=166016000 overlay_source_nanos=0
```

### Phase 3：打通真正 raw positioning overlay 模式

目标：

让 BUG-0010 的定位验收可以完全隔离 smoothing / Bezier / click magnification / raw system cursor。

涉及文件：

- `src/lib/tauri.ts`
- `src/components/preview-view.tsx`
- `src/App.test.tsx`
- `src-tauri/src/lib.rs`
- `src-tauri/src/core/timeline.rs`
- `src-tauri/src/media/cursor_engine.rs`

关键改动：

1. `BeautifyConfigPayload` / TS `BeautifyConfig` 新增：

```text
rawCursorPositioning: boolean
```

2. 前端可先放在开发/诊断区域，文案必须中文，例如：

```text
原始定位验收
```

3. `start_recording()` 中 show cursor 策略改为：

```text
if rawCursorPositioning {
  show_system_cursor = false
} else {
  show_system_cursor = !(cursor_magnification || cursor_smoothing)
}
```

4. `build_effect_timeline_from_metadata()`：

```text
if config.raw_cursor_positioning {
  engine = CursorEffectEngine::raw_positioning()
  clicks = []
} else {
  existing behavior
}
```

5. `CursorEffectEngine::linear_interpolate_frames()` 修复首样本 > 0：
   - `t <= first_ts` hold first sample
   - `t >= last_ts` hold last sample
   - 中间段只在合法区间插值

新增测试：

1. `raw_positioning_config_keeps_system_cursor_hidden`
2. `build_effect_timeline_uses_raw_engine_when_raw_positioning_enabled`
3. `raw_positioning_handles_first_sample_after_zero`
4. `raw_positioning_ignores_click_effects`

验收：

- effect timeline raw mode：

```json
"clickEffects": []
```

- 所有 frames：
  - `scale=1.0`
  - `opacity=1.0`
  - 无 Bezier overshoot

### Phase 4：AX 坐标诊断与修复

目标：

证明并修正 AX hit-test point 的坐标空间，让 Hand/IBeam 能真实采样。

涉及文件：

- `src-tauri/src/platform/macos/cursor_kind.rs`
- `src-tauri/src/platform/macos/cursor_source.rs`
- `src-tauri/src/app/cursor_metadata_runtime.rs`
- `src-tauri/src/platform/macos/screen_capture_kit.rs`

建议顺序：

1. 先新增诊断，不直接改分类逻辑。
2. 采样日志扩展为：

```text
[cursor-kind-classify]
raw_point=(x,y)
candidate_ax_point=(x,y)
display_frame=(x,y,w,h)
main_display_height=...
role_chain=[...]
actions=[...]
element_frame=(x,y,w,h)
rc=...
dur=...
```

3. 同时尝试至少两个 candidate：
   - candidate A：raw CGEvent point
   - candidate B：基于 main display height 的 flipped point
   - 如有多显示器，candidate C：基于 display frame origin/height 的 local-to-global 转换
4. 人工在 Tauri 按钮、浏览器链接、文本框、桌面空白处采样一次。
5. 选出命中正确 role 的转换公式，固定为 `AxCoordinateMapper`。

修复后分类策略：

1. 输入框：
   - `AXTextField`
   - `AXTextArea`
   - 或明确 editable attribute 为 true
2. 可点击：
   - `AXButton`
   - `AXLink`
   - `AXMenuItem`
   - `AXPress` action
   - parent chain 内 clickable
3. fallback：
   - Arrow
   - 但必须记录 fallback reason

新增测试：

1. `ax_coordinate_mapper_keeps_top_left_when_candidate_matches`
2. `ax_coordinate_mapper_flips_y_for_ax_when_required`
3. `cursor_kind_failure_log_records_result_code`
4. `cursor_kind_diagnostics_are_recording_delta_not_process_total`

人工验收：

- Accessibility granted：
  - Tauri button -> Hand
  - 浏览器 link -> Hand
  - 原生 text field -> IBeam
  - Tauri/Electron input -> IBeam
- metadata：
  - `handCount > 0`
  - `ibeamCount > 0`
  - role chain 日志能指出 Hand/IBeam 来源

### Phase 5：Arrow tail rendered/export 级验证

目标：

解释并修复“人工仍看到 Arrow 无尾柄”的可见性问题。

涉及文件：

- `src-tauri/src/media/cursor_overlay.rs`
- `src-tauri/src/media/trim_exporter.rs`

关键改动：

1. 保留现有 bitmask 测试。
2. 新增 rendered Y-plane 测试：
   - 构造 `CursorFrame { kind: Arrow, scale: 1.0 }`
   - cursor 放在画面中心
   - 调用 `CursorOverlayRenderer::draw_on_frame()`
   - 断言 tail handle 区域存在黑色 body 和白色 outline
3. 如果 24x24 glyph 在 1080p 导出中视觉过小，可评估 cursor glyph baseline scale，但不能影响 hotspot 对齐。
4. 如果 tail 被 draw clamp/crop 裁掉，需要调整 margin 或 hotspot/tail layout。

新增测试：

1. `arrow_tail_renders_on_y_plane_at_scale_one`
2. `arrow_tail_renders_with_fit_with_bars`
3. `arrow_tail_renders_with_center_crop`

人工验收：

- 普通桌面 Arrow：
  - 黑底白边
  - 尾柄可见
  - hotspot 仍在箭头尖端

### Phase 6：文档与状态更新

目标：

第五轮完成后，更新项目状态，避免“已修复-待人工验证”掩盖真实失败。

涉及文件：

- `BUG.md`
- `HANDOFF.md`
- `tests/2026-06-04-bug-0010-0011-fifth-round-checklist.md`

建议更新：

1. `BUG.md` 中新增：
   - `BUG-0010_8: 第 5 轮整改状态`
   - `BUG-0011_8: 第 5 轮整改状态`
2. 记录真实根因：
   - BUG-0010：source MP4 demux PTS origin 未进入 overlay timebase；raw mode 无生产入口。
   - BUG-0011：AX hit-test 坐标语义错误/未验证；权限和 diagnostics 未进入录制 metadata。
3. `HANDOFF.md` 工作任务记录新增第五轮整改完成记录。
4. checklist 勾选实际命令输出，不要只写“已通过”。

---

## 9. 推荐编码顺序

推荐严格按以下顺序执行，不要并行修改同一文件：

1. Phase 0：创建第五轮自测清单。
2. Phase 1：真实 diagnostics 落盘。
3. Phase 2：修 source MP4 demux PTS origin mapping。
4. Phase 3：raw positioning 生产入口 + raw interpolation 首样本修复。
5. Phase 4：AX 坐标诊断，先采样确认，再固定 `AxCoordinateMapper`。
6. Phase 5：Arrow rendered tail 测试。
7. Phase 6：BUG/HANDOFF/checklist 文档更新。

不要先改 AX 分类规则。当前 evidence 指向 hit-test 坐标错误，分类规则继续扩展只会让问题更难定位。

不要先调 smoothing 参数。当前 BUG-0010 必须先 raw overlay 通过，再讨论 smoothing 视觉效果。

不要继续把 host-time raw PTS 与 source MP4 demux PTS 混用。所有 PTS offset 必须带明确 timebase。

---

## 10. 自动测试门禁

第五轮至少需要新增或补齐以下测试：

### Rust

1. `finish_writes_cursor_timing_diagnostics_to_metadata`
2. `finish_writes_media_timeline_diagnostics_to_metadata`
3. `cursor_kind_diagnostics_are_recording_delta`
4. `raw_positioning_handles_first_sample_after_zero`
5. `raw_positioning_ignores_click_effects`
6. `build_effect_timeline_uses_raw_positioning_when_config_enabled`
7. `overlay_uses_demux_pts_origin_when_input_pts_is_non_zero`
8. `overlay_preserves_source_timestamp_after_cut_segments`
9. `ax_failure_log_records_result_code_and_duration`
10. `arrow_tail_renders_on_y_plane_at_scale_one`
11. `arrow_tail_renders_with_fit_with_bars`
12. `arrow_tail_renders_with_center_crop`

### Frontend

1. `raw positioning toggle updates beautify config`
2. `accessibility warning remains visible when permission not granted`
3. `start recording passes raw positioning intent through backend config`

### 必跑命令

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata_runtime
cargo test --manifest-path src-tauri/Cargo.toml cursor_engine
cargo test --manifest-path src-tauri/Cargo.toml cursor_overlay
cargo test --manifest-path src-tauri/Cargo.toml trim_exporter --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
npm run build
```

---

## 11. 人工验证门禁

### 11.1 BUG-0010 raw positioning

前置条件：

- `rawCursorPositioning=true`
- source recording 不包含系统 cursor
- effect timeline `clickEffects=[]`
- all frame `scale=1.0`
- `sourcePtsOriginNanos` 或 exporter demux origin diagnostics 非 0 时被正确减掉

验证项：

1. 静止四角和中心：overlay 与源目标点重合。
2. 左到右匀速移动：overlay 不持续左/右漂。
3. 右到左匀速移动：overlay 不持续左/右漂。
4. 多段移动 + 停顿：不出现累计漂移。
5. Retina 显示器：不偏移。
6. `ffprobe start_time` 非 0 的 source artifact：overlay 仍准确。

### 11.2 BUG-0011 target-aware kind

前置条件：

- Accessibility granted。
- metadata 中 `accessibilityPermissionAtStart=granted`。
- AX logs 显示命中内容区元素，不再持续 `AXMenuBar`。

验证项：

1. 普通桌面：Arrow，黑底白边，有尾柄。
2. Tauri 按钮：Hand。
3. 浏览器链接：Hand。
4. 原生文本框：IBeam。
5. Tauri/Electron input：IBeam。
6. metadata 中 `handCount > 0`。
7. metadata 中 `ibeamCount > 0`。
8. metadata 中 AX failure/fallback/kind distribution 是本次录制 delta。

### 11.3 未授权 Accessibility

验证项：

1. UI 显示明确提示。
2. metadata 中 `accessibilityPermissionAtStart=notDetermined` 或 `denied`。
3. 如果未授权仍允许录制，metadata/export summary 必须明确“target-aware cursor 已降级为 Arrow”。
4. 不允许静默宣称 target-aware cursor 已通过。

---

## 12. 最小成功标准

第五轮完成后，必须能用真实 artifact 证明：

1. `cursor-metadata-*.json` 中存在非空 `mediaTimelineDiagnostics`。
2. `cursor-metadata-*.json` 中存在非空 `cursorTimingDiagnostics`。
3. source MP4 `start_time != 0` 时，export overlay 使用 `decoded_demux_pts - first_demux_pts` 查询 cursor timeline。
4. raw positioning mode 在生产路径可触发。
5. raw positioning mode 下无 smoothing / Bezier / click scale。
6. raw interpolation 对首样本大于 0 安全。
7. AX hit-test 不再持续命中 `AXMenuBar`。
8. Accessibility 状态写入 metadata。
9. Hand/IBeam 在人工悬停目标上出现。
10. Arrow tail 在 rendered/export 级测试和人工导出中都可见。

---

## 13. 后续执行建议

建议先把本文件作为第五轮整改输入，新增对应 implementation plan：

- 建议计划文件：`docs/superpowers/plans/2026-06-04-bug-0010-0011-fifth-round-rectification.md`
- 建议自测清单：`tests/2026-06-04-bug-0010-0011-fifth-round-checklist.md`

执行第五轮时必须遵守：

1. 先写失败测试，再改实现。
2. 每个 Phase 后运行对应最小测试。
3. 每个 Phase 后更新 checklist。
4. 不要修改 `Cargo.toml` 核心依赖版本。
5. 不要触碰产品路线图外功能。
6. 不要把平台 FFI 安全假设藏在代码里；新增 AX/SCK 坐标转换必须有诊断证据和注释。
