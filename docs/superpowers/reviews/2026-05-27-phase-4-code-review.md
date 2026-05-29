# Phase 4 Code Review: Cursor Effects

> 日期：2026-05-27
> 评审范围：Phase 4 光标平滑与点击放大
> Git 范围：`9bfc4ee6fa853c50730bcaacda285e84ba6316fc^..7e5cabc92370e3164a3ca4047beb93a86d2d1580`
> 结论：With fixes，不建议直接合并

## 1. 评审目标

本次评审重点回答两个问题：

1. Phase 4 是否完整完成了开发任务。
2. Phase 4 相关代码是否存在阻塞捕获主链路、内存安全、线程安全、资源释放路径异常等问题。

额外检查：

- 是否遵守 `HANDOFF.md` 中的当前项目状态和 Phase 4 剩余 gate。
- 是否遵守 `BUG.md` 中的预防规则。
- 是否遵守 `docs/architecture/project-architecture-and-overall-planning.md` 中的数据流红线：音视频帧流不得经过前端 JS 层。
- 是否遵守 Phase 4 计划 `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md`。

## 2. 总体结论

Phase 4 主体功能已经基本完成：

- Rust 侧新增 cursor timeline model、cursor processor trait、cursor smoothing / Bezier interpolation / click magnification engine。
- 录制期通过 `CursorMetadataRuntime` 独立采集 cursor metadata。
- 停止录制后写入 cursor metadata sidecar。
- 前端只通过 Tauri command 设置美化配置、触发 timeline 构建和导出边界，没有接收光标 sample stream、视频帧或音频帧。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`，为避免双光标提供了控制点。
- `tests/phase-4-w7-w8-checklist.md` 已记录自动化验证和剩余人工 gate。

但是当前不建议直接合并。阻塞点主要集中在：

- 光标 metadata 与视频 frame 的时间基没有被严格证明为同一 session-relative origin。
- 新录制 session 可能继承上一段录制的 effect timeline path。
- cursor sidecar 写入失败会中断 stop 后续 cleanup / state completion。
- cursor metadata buffer 和 preview timeline rebuild 存在长录制/高频交互下的性能与资源风险。

## 3. 自动化验证结果

本次评审期间实际执行：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, 103 tests
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 22 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS
- `npm run build`: PASS
- `npm test -- --run`: PASS, 25 tests

注意：

- clippy warning 中有 1 个 Phase 4 新增 warning：`src-tauri/src/app/cursor_metadata_runtime.rs` 未使用 `Mutex` import。
- 其余 warning 主要是既有 FFI 命名/未使用项。
- `npm test` 输出 `Window.scrollTo()` 未实现提示，但测试通过。

## 4. Strengths

1. 架构方向正确
   - cursor smoothing、Bezier interpolation、click magnification 都在 Rust 侧完成。
   - React 侧只发轻量 command，不接触媒体帧和光标 sample stream。
   - 录后 `CursorEffectEngine` 构建 timeline，没有插入 ScreenCaptureKit frame callback。

2. 捕获主链路未被直接阻塞
   - ScreenCaptureKit callback 仍通过 bounded media channel 和 `try_send_drop_newest` 传递媒体数据。
   - cursor 采集使用独立 polling thread。
   - timeline 构建发生在录后 command 路径。

3. 算法测试覆盖较完整
   - 空输入、单点、jitter smoothing、快速跳变、30fps/60fps 窗口、Bezier frame interval、click state machine、serde round-trip 均有测试。

4. Native Safety Gate 有显式记录
   - `docs/superpowers/reviews/2026-05-27-phase-4-native-safety-notes.md` 已记录 CoreGraphics 和 `showsCursor` 检查点。
   - `tests/phase-4-w7-w8-checklist.md` 保留了仍需人工验证的双光标和 native safety 项。

## 5. Issues

### Critical 1: Cursor metadata 与视频 frame 未严格共享同一时间基

位置：

- `src-tauri/src/platform/macos_service.rs:111`
- `src-tauri/src/platform/macos_service.rs:125`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:152`

现象：

- `MacRecordingService::start()` 先调用 `self.screen_capture.start_combined(...)` 启动 ScreenCaptureKit。
- 之后才创建 `SessionClock`，并将这个 clock 传给 microphone 和 cursor runtime。
- 视频 frame timestamp 则由 `ScreenCaptureKit` callback 中的 `TimestampNormalizer` 独立按首帧 PTS 归零。

为什么重要：

Phase 4 的核心要求是 cursor event 与视频 frame 使用同一 session-relative time base。当前实现只能保证两者都大致从录制开始附近归零，但无法证明同一真实时刻会得到同一 `MediaTimestamp`。如果 `startCaptureWithCompletionHandler` 完成、首帧到达、`SessionClock::new()` 创建之间存在延迟，cursor timeline 可能整体偏移，最终表现为光标效果和视频内容不同步。

风险类型：

- 功能完整性风险：Phase 4 时间线和视频画面无法精确对齐。
- 用户可见风险：点击放大可能晚于或早于真实点击。
- 测试缺口：当前测试只验证 cursor runtime 使用 `SessionClock`，没有验证 video/cursor 同源。

建议修复：

1. 在所有 capture 组件启动之前创建一个统一 recording session clock/origin。
2. ScreenCaptureKit video/system audio timestamp 归一化也基于同一个 origin，而不是独立首帧归零。
3. 为 `TimestampNormalizer` 或 capture service 增加 fake-clock / fake-timestamp 测试，验证同一真实时刻的 video frame 和 cursor sample 映射到相同或可解释的 session-relative timestamp。
4. 如暂时无法完全同源，至少显式记录 offset，并在构建 `EffectTimeline` 时补偿。

建议验收：

- 新增测试：`video_and_cursor_timestamps_share_session_origin`。
- 手动录制含明显点击动作的视频，检查 timeline JSON 中 click timestamp 与视频动作时间是否匹配。

### Important 1: 新录制可能继承上一段录制的 effect timeline path

位置：

- `src-tauri/src/platform/macos_service.rs:223`
- `src-tauri/src/platform/macos_service.rs:95`

现象：

`stop()` 会把 `self.last_effect_timeline_path.clone()` 写入新的 `RecordingResult.effect_timeline_path`。但 `start()` 没有清空 `last_effect_timeline_path`。如果录制 A 构建了 effect timeline，然后开始录制 B，录制 B stop 后可能返回录制 A 的 effect timeline path。

为什么重要：

- 录制结果与 sidecar 文件会错绑。
- 后续 export 或 UI 展示可能引用旧文件。
- 如果 Phase 6 compositor 使用该 path，会把上一段录制的 cursor effect 应用到下一段素材。

建议修复：

1. `MacRecordingService::start()` 成功进入新 session 时清空：
   - `self.last_cursor_metadata_path = None`
   - `self.last_effect_timeline_path = None`
2. 更稳妥方案：引入 recording session id，metadata/effect timeline path 都绑定 session id。

建议验收：

- 新增测试：第一段录制设置 `last_effect_timeline_path`，第二段录制开始后 stop，确认第二段 `RecordingResult.effect_timeline_path == None`，直到重新构建 timeline。

### Important 2: cursor metadata sidecar 写入失败会破坏 stop cleanup / 状态收敛

位置：

- `src-tauri/src/platform/macos_service.rs:216`
- `src-tauri/src/platform/macos_service.rs:226`
- `src-tauri/src/platform/macos_service.rs:241`

现象：

`RecordingMetadataWriter::write_metadata(&path, &metadata)?` 失败时会提前返回。此时：

- native capture 已 stop。
- consumer thread 已 join。
- cursor runtime 已 stop。
- 但 mic level reset 未执行。
- state machine `stop()` / `complete()` 未执行。

为什么重要：

这是典型的资源释放路径异常。媒体资源已经停止，但 service state 可能仍停留在 `Recording` 或其他非最终状态。前端后续查询状态、再次开始录制、错误恢复都可能出现不一致。

风险类型：

- 资源释放路径异常。
- 状态机与真实资源状态脱节。
- 错误恢复路径不可靠。

建议修复：

1. 将 stop 的资源释放、runtime 停止、mic reset、state transition 放进 finally-style 路径。
2. sidecar 写入错误可以记录并在 cleanup 后返回，也可以将状态显式转为 `Failed`，但不能跳过 cleanup。
3. 将 sidecar 写入失败与 native stop 失败分别建模，避免一个 post-process 错误掩盖资源释放结果。

建议验收：

- 给 `RecordingMetadataWriter` 抽 trait 或注入 writer mock。
- 新增测试：metadata write 失败时，capture 已 stop、mic level reset、state 进入 Completed 或 Failed 中的明确最终状态。

### Important 3: cursor sample buffer 仍有高开销和 click metadata 无界增长风险

位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:54`
- `src-tauri/src/app/cursor_metadata_runtime.rs:103`

现象：

- sample 达到 `DEFAULT_MAX_CURSOR_SAMPLES = 120_000` 后，每次新增 sample 都执行 `Vec::remove(0)`。
- `Vec::remove(0)` 会移动后续所有元素，超过上限后每次 poll 都是 O(n)。
- click metadata 没有类似上限。

为什么重要：

当前 cursor runtime 虽然不在 ScreenCaptureKit callback 中，但仍是录制期常驻线程。长录制达到 sample cap 后，O(n) 移动会造成持续 CPU 压力。大量点击也可能造成 click vector 无界增长。

风险类型：

- 录制期性能风险。
- 长录制内存风险。
- 可能间接影响主进程调度和编码消费线程。

建议修复：

1. samples 改成 `VecDeque` 或固定 ring buffer。
2. clicks 增加上限，或按 session sidecar 分段写入。
3. 对超过上限的行为定义策略：drop oldest、spill to disk、或记录 truncated 标记。

建议验收：

- 新增测试：超过 sample cap 时不会改变顺序，并且不会 O(n) remove。
- 新增测试：clicks 超过 cap 时按预期丢弃旧数据或截断。

### Important 4: preview 控件变更会同步触发大量 timeline build

位置：

- `src/components/preview-view.tsx:68`
- `src-tauri/src/lib.rs:357`
- `src-tauri/src/lib.rs:387`
- `src-tauri/src/lib.rs:397`

现象：

前端每次 beautify control 改变都会：

1. `setBeautifyConfig(...)`
2. `buildCursorEffectTimeline()`

其中 slider 的 `onValueChange` 会在拖动过程中高频触发。Rust command 中同步读 metadata JSON、构建 timeline、写 effect JSON。

为什么重要：

对短录制看不明显，但长录制 timeline frame 数可能较大。高频拖动 slider 会排队多个 post-process job，造成 Tauri command 线程压力和 UI 卡顿风险。

风险类型：

- 非捕获主链路阻塞，但仍可能阻塞 app command 响应。
- 重复 JSON 读写和 timeline 分配造成额外内存/CPU 压力。

建议修复：

1. 前端 debounce slider / switch 触发。
2. 后端对正在构建的 timeline 做 single-flight 或取消旧任务。
3. timeline 构建放进 `spawn_blocking` 或独立 post-process runtime。
4. `build_cursor_effect_timeline` 在 recording 状态下直接拒绝。

建议验收：

- 新增前端测试：连续 slider change 只触发一次 debounced timeline build。
- 新增 Rust 测试或集成约束：recording 状态下 build command 返回错误。

### Minor 1: sidecar 文件名使用毫秒时间戳，快速连续构建可能覆盖

位置：

- `src-tauri/src/lib.rs:436`
- `src-tauri/src/platform/macos_service.rs:360`

现象：

`cursor-metadata-{millis}.json` 和 `cursor-effects-{millis}.json` 使用当前毫秒时间戳命名。如果同一毫秒内多次生成，可能覆盖。

建议修复：

- 加 session id、atomic counter、随机后缀，或使用 `tempfile`。

### Minor 2: Phase 4 plan 文件仍是 untracked

位置：

- `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md`

现象：

`git status --short` 显示该文件未跟踪，但它是本次 Phase 4 评审和整改的重要上下文。

建议修复：

- 若该计划文件应作为 Phase 4 可追溯记录，请纳入提交。

### Minor 3: Phase 4 新增 unused import warning

位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:2`

现象：

`use std::sync::{Arc, Mutex};` 中 `Mutex` 未使用。

建议修复：

- 删除未使用的 `Mutex` import。

## 6. Phase 4 完整性检查

### 已满足

- `core/timeline.rs` 中包含 cursor sample、click、frame、effect timeline serde model。
- `core/processor.rs` 提供 `CursorProcessor` trait boundary。
- `media/cursor_engine.rs` 提供 smoothing、Bezier interpolation、click effect builder、timeline composition。
- `app/cursor_metadata_runtime.rs` 提供 cursor metadata recorder/runtime。
- `platform/macos/cursor_source.rs` 使用 CoreGraphics 读取 cursor position 和 button state。
- `RecordingResult` 已包含 cursor metadata path 和 effect timeline path。
- `set_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- `src/lib/tauri.ts` 已添加对应 TypeScript wrapper 和类型。
- `PreviewView` 已调用 Tauri command，而不是 console-only hook。
- `CaptureConfig.show_system_cursor` 已接入 ScreenCaptureKit。
- `tests/phase-4-w7-w8-checklist.md` 已更新自动化和人工验证状态。

### 未完全满足或仍需人工 gate

- 真实视频导出中包含光标平滑和点击放大仍需 Phase 6 FFmpeg compositor。
- Native Safety Gate 仍需人工逐行审查与 macOS 手动验证。
- 双光标行为仍需 `npm run tauri dev` 手动确认。
- 时间基对齐没有足够实现和测试证明，需作为 Phase 4 整改项处理。

## 7. 捕获主链路、内存安全、线程安全、资源释放专项结论

### 捕获主链路

结论：没有发现 cursor effect 算法直接进入 ScreenCaptureKit callback。

证据：

- cursor polling 在 `CursorMetadataRuntime` 独立 thread。
- smoothing/interpolation/click effect 在 `build_cursor_effect_timeline` 录后 command。
- ScreenCaptureKit callback 仍使用 bounded channel `try_send_drop_newest`。

剩余风险：

- timeline build command 目前同步执行，虽不阻塞 capture callback，但可能阻塞 Tauri command 响应。

### 内存安全

结论：`cursor_source.rs` 的 CoreGraphics FFI 路径表面上是成对释放的，但仍需人工 Native Safety Gate。

证据：

- `CGEventCreate` null check 后调用 `CGEventGetLocation`。
- `CFRelease(event as CFTypeRef)` 在读取位置后释放。
- `CGEventSourceButtonState` 是只读状态查询。

剩余风险：

- 长录制下 cursor samples `Vec::remove(0)` 高开销。
- click metadata 无界增长。
- sidecar JSON 构建会一次性持有完整 timeline，长录制可能造成较大内存峰值。

### 线程安全

结论：未发现明显数据竞争；共享状态多由 `Mutex` / `AtomicBool` 管理。

需要关注：

- `CursorMetadataRuntime::stop()` 直接 join polling thread。如果 `source.snapshot()` 在系统 API 层长时间阻塞，stop 会等待。不过当前 CoreGraphics 调用预计很短，风险较低。
- `build_cursor_effect_timeline` 读写 sidecar 与 service path 更新没有 session id，可能与用户快速操作产生逻辑竞态。

### 资源释放路径

结论：存在需要修复的问题。

关键问题：

- cursor sidecar 写入失败会跳过 mic reset 和 state machine completion。
- new session 不清理 old effect timeline path，造成资源/metadata 关联错误。

## 8. BUG.md 预防规则检查

检查项：

- 未发现 Phase 4 diff 新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- `PreviewView` 导出按钮没有被 `motion.div whileTap` 直接包裹，符合 BUG-003 预防规则。
- `recording-panel.tsx` 中存在既有 `motion.button whileTap`，但这不是 BUG-003 中的 `motion.div` 包裹交互 Button 模式，未作为阻塞问题。

## 9. 建议整改顺序

1. 修复统一时间基问题，并补测试。
2. 修复新 session 清理旧 sidecar path 问题，并补测试。
3. 修复 stop cleanup 的 finally-style 状态收敛问题，并补测试。
4. 将 cursor metadata buffer 改为 ring buffer / `VecDeque`，clicks 增加上限。
5. 给 preview beautify rebuild 增加 debounce / single-flight / `spawn_blocking`。
6. 清理 unused import 和 sidecar 文件名碰撞风险。
7. 完成 `npm run tauri dev` 手动验证：
   - cursor metadata JSON 写入。
   - click 采集。
   - effect timeline JSON 生成。
   - cursor beautify 开启时无双光标。
   - clean macOS permission behavior。

## 10. Ready To Merge?

**Ready to merge: With fixes**

原因：

核心功能方向正确，自动化测试全部通过，且没有发现 cursor 算法直接阻塞 capture callback。但时间基对齐、sidecar 生命周期和 stop cleanup 路径会影响 Phase 4 的核心正确性与恢复可靠性，应在合并前修复。

## 11. Round 1 整改复审（2026-05-27）

> 评审范围：Phase 4 从 `9bfc4ee6fa853c50730bcaacda285e84ba6316fc` 到当前工作区，重点复审第 1 轮整改后的代码。
> 当前工作区：Phase 4 计划文件已 staged；5 个源码文件存在整改修改；本 review 文档仍为 untracked。
> 结论：仍为 With fixes，不建议直接合并。整改修复了多条资源路径问题，但时间戳语义出现新的核心正确性风险，且异步 post-process 任务仍缺 session 归属保护。

### 11.1 本轮已确认修复或改善

- `src-tauri/src/platform/macos_service.rs:99-101`：新录制开始时清空 `last_cursor_metadata_path` 和 `last_effect_timeline_path`，修复了普通顺序录制场景下旧 sidecar path 泄漏到新结果的问题。
- `src-tauri/src/platform/macos_service.rs:188-271`：`stop()` 改成收集错误并尽量执行 capture stop、mic stop、consumer join、cursor runtime stop、mic level reset 和状态机终态迁移，上一轮 “sidecar 写入失败跳过后续 cleanup” 的主问题已改善。
- `src-tauri/src/app/cursor_metadata_runtime.rs:36-59`：cursor samples 改为 `VecDeque`，超上限后 `pop_front()`，避免上一轮 `Vec::remove(0)` 对 samples 的持续 O(n) 压力。
- `src-tauri/src/app/cursor_metadata_runtime.rs:14,107-109`：click metadata 增加 10,000 条上限，修复无界增长；仍有 O(n) 细节风险，见 Minor。
- `src-tauri/src/lib.rs:374-418`：timeline 构建移到 `spawn_blocking`，不会直接占用 async command executor 做重 CPU/JSON 工作。
- `src/components/preview-view.tsx:68-81`：前端加入 300ms debounce，减少 slider 高频操作反复触发 timeline build。
- `src-tauri/src/lib.rs:458-469` 与 `src-tauri/src/platform/macos_service.rs:387-398`：sidecar 文件名加入进程内 atomic counter，降低毫秒级文件名碰撞风险。
- BUG.md Gate：`rg` 未发现源码新增 `data-tauri-drag-region="false"`；`whileTap` 只出现在既有 `motion.button`，未重新引入 BUG-003 的 `motion.div whileTap` 包裹交互 Button 模式。

### 11.2 Critical

#### Critical 1: 统一时间基整改绕开了 CMSampleBuffer PTS，反而破坏媒体时间戳语义

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:151-157`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:282-304`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:363-379`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:465-479`

现象：

本轮整改将 video/system audio timestamp 改为 `delegate.ivars().session_clock.elapsed_nanos()`。这让 video、system audio、cursor 在代码表面共享了同一个 `SessionClock`，但它不再使用 ScreenCaptureKit / CoreMedia 提供的 `CMSampleBuffer` presentation timestamp。

更关键的是，当前 video timestamp 在完成像素拷贝、`Arc<[u8]>` 分配并 unlock 之后才读取 `elapsed_nanos()`；system audio timestamp 也在 format 校验、AudioBufferList 分配、PCM 转换、block buffer release 之后才读取。也就是说 timestamp 记录的是 callback 处理到后半段的 wall-clock 时间，而不是媒体样本实际捕获/展示时间。

为什么阻塞：

- Phase 4 需要光标事件与视频帧按同一时间轴对齐。当前实现牺牲了媒体 PTS，无法证明 “同一真实事件” 能映射到正确的视频 frame timestamp。
- 在 CPU 忙、内存分配慢、audio conversion 慢或 callback 排队时，timestamp 会包含调度延迟和处理延迟，产生抖动、漂移或音画不同步。
- system audio 每个 chunk 使用处理完成时的时间戳，而非 buffer 自身 PTS/持续时间；后续 audio synchronizer 会基于这个偏移后的 timestamp 做配对。
- `CMSampleBufferTimingInfo`、`CMSampleBufferGetSampleTimingInfo`、`extract_timestamp_nanos()` 现在变成 unused warning，说明原来的媒体 PTS 通路被绕开了，而不是被正确对齐到 session origin。

建议修复：

1. 保留 `CMSampleBuffer` PTS 作为 video/system audio 的样本时间来源。
2. 在 session start 时记录一个可与 CoreMedia host time 对齐的 session origin，或记录 `first_media_pts <-> session_clock_elapsed` offset，再将 PTS 归一化为 session-relative timestamp。
3. cursor runtime 继续使用 session clock 可以接受，但必须证明这个 clock 与媒体 PTS 的映射关系稳定。
4. 最低限度的临时缓解是 callback 入口立即取 arrival timestamp，再做拷贝/转换；但这仍不等价于媒体 PTS，只能作为短期兜底。
5. 增加 fake-clock/fake-PTS 测试，证明 video frame、system audio chunk、cursor sample 在同一真实时刻映射到同一 session-relative timestamp。

### 11.3 Important

#### Important 1: async timeline build 仍可把旧 effect path 写回新 session

位置：

- `src-tauri/src/lib.rs:350-358`
- `src-tauri/src/lib.rs:374-428`
- `src-tauri/src/platform/macos_service.rs:99-101`
- `src-tauri/src/platform/macos_service.rs:238-240`

现象：

`build_cursor_effect_timeline()` 先读取 `last_cursor_metadata_path()`，释放 service lock，随后在 `spawn_blocking` 中读旧 metadata、构建 timeline、写 effect JSON。构建完成后，它再次拿 service lock 并调用 `set_last_effect_timeline_path(Some(...))`。

如果用户在旧 build 运行期间开始新录制，`MacRecordingService::start()` 会清空 path；但旧 build 结束后仍可把旧 effect path 写回 service。之后新录制 `stop()` 会把这个 stale path 拷进新的 `RecordingResult.effect_timeline_path`。

为什么重要：

- 这是 session 资源归属竞态，不是单纯 UI 显示问题。
- Phase 6 compositor 一旦消费该 path，可能把 A 录制的 cursor effect 应用到 B 录制。
- 当前 command 只在 `RecordingState::Recording` 时拒绝，未用 session id/path generation 校验完成时写回的对象是否仍是同一段录制。

建议修复：

1. 为 recording session 引入 generation id/session id。
2. `last_cursor_metadata_path`、`last_effect_timeline_path` 与 session id 一起存储。
3. build 开始时捕获 `(session_id, metadata_path)`，build 完成后只有当当前 service 的 session id 和 metadata path 仍匹配时才写回 effect path。
4. 后端增加 single-flight 或取消旧任务；至少在 start 新录制时让旧 post-process 结果不能污染新 session。

#### Important 2: preview debounce 破坏了 `setBeautifyConfig -> buildTimeline` 的顺序语义

位置：

- `src/components/preview-view.tsx:70-81`
- `src/App.test.tsx:820-850`

现象：

`handleBeautifyChange()` 现在 fire-and-forget 调用 `setBeautifyConfig(currentBeautifyConfig(config))`，随后启动 debounce timer；timer 到期后直接调用 `buildCursorEffectTimeline()`。两者没有 await/then 链接，也没有捕获 `setBeautifyConfig` rejection。

为什么重要：

- 如果 `set_beautify_config` 调用失败，timeline build 仍会继续，用的是旧配置。
- 如果 Tauri command 调度延迟，build 可能早于 config 写入完成，生成旧配置的 timeline。
- 现有测试只断言 `set_beautify_config` 被调用，没有使用 fake timers 验证 debounce 后一定 build，也没有验证 build 发生在 config 成功之后。

建议修复：

1. debounce 一个 async job：先 await `setBeautifyConfig(nextConfig)`，成功后再 build。
2. 或新增单个后端 command，将 “设置配置 + 构建 timeline” 作为一个原子操作。
3. 对 `setBeautifyConfig` 和 `buildCursorEffectTimeline` 都做错误捕获与用户可见失败状态。
4. 前端测试使用 fake timers，验证连续变更只 build 一次，且 build 在 config resolve 后发生。

#### Important 3: debounce timer 没有 unmount cleanup，会在离开预览后继续触发后端任务

位置：

- `src/components/preview-view.tsx:1`
- `src/components/preview-view.tsx:68-81`

现象：

`debounceRef` 中的 timer 没有在组件 unmount 时清理。用户切换美化选项后立刻返回录制页或开始下一段录制，300ms 后旧 `PreviewView` 仍会触发 `buildCursorEffectTimeline()`。

为什么重要：

- 这会放大 Important 1 的 session/path 竞态。
- 这是前端资源释放路径缺口：组件销毁后仍保留 timer 和后端调用。
- 用户快速操作时，旧页面动作可能影响新录制 session 的 service 状态。

建议修复：

1. 引入 `useEffect` cleanup：unmount 时 `clearTimeout(debounceRef.current)`。
2. 对 in-flight build 增加 generation token/ignore flag，离开 preview 后不再写 UI 状态或触发后续动作。
3. 后端仍需 session id 校验，不能只依赖前端 cleanup。

#### Important 4: 自测清单把“同一时间基”标记为完成，但当前实现和测试尚不能支撑

位置：

- `tests/phase-4-w7-w8-checklist.md:13`
- `tests/phase-4-w7-w8-checklist.md:69`

现象：

清单写明 “光标事件与视频帧使用同一时间基” 已完成，并注明 `CursorMetadataRuntime` 使用 `SessionClock`。但 video/system audio 的时间戳问题见 Critical 1：仅 cursor runtime 使用 `SessionClock` 不足以证明媒体 frame 与 cursor event 同源。

另外清单记录 clippy 为 22 个 pre-existing warnings；当前实际 `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 为 24 warnings，其中包含本轮时间戳通路被绕开后产生的 unused timing helper。

建议修复：

1. 在 Critical 1 修复前，将同一时间基清单项改回未完成或标注 residual risk。
2. 修复后补 timestamp-origin proof test，再重新勾选。
3. 更新 clippy warning 记录，或在恢复 PTS helper 后把 warning 降回预期。

### 11.4 Minor

#### Minor 1: click metadata 已有上限，但满载后仍是 `Vec::remove(0)`

位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:37`
- `src-tauri/src/app/cursor_metadata_runtime.rs:107-109`

本轮已经通过 `DEFAULT_MAX_CURSOR_CLICKS = 10_000` 消除了无界增长，这是正确方向。但 click 达到上限后每次新 click 仍会 `remove(0)` 并移动最多 9,999 个元素。通常点击频率远低于 sample，风险比上一轮低很多；若继续按严格性能 gate 收尾，建议 clicks 也改为 `VecDeque<CursorClick>`。

#### Minor 2: `build_cursor_effect_timeline` 只拒绝 Recording，未覆盖 Paused/Processing

位置：

- `src-tauri/src/lib.rs:339-347`

当前只在 `RecordingState::Recording` 返回错误。Paused 本质上仍属于活跃录制 session；Processing 也不是稳定可消费结果状态。虽然开始录制时已经清空 path，很多情况下会因缺 metadata path 而失败，但 command 的状态约束不够明确。建议改成只允许 Completed/Failed 且 metadata path 与 session id 匹配，或至少拒绝 Recording/Paused/Processing。

#### Minor 3: progress 事件只有 0 和成功 100，失败时没有 terminal/failure progress

位置：

- `src-tauri/src/lib.rs:360-366`
- `src-tauri/src/lib.rs:430-436`

如果 read/build/write 任一步失败，前端只能收到 `post-process-progress` 的 0，没有对应失败/完成事件。这不是 capture 主链路问题，但会让 post-process UI 状态难以收敛。建议后续和导出进度事件一起补一个 `failed` 或 error payload。

### 11.5 Phase 4 完整性复审结论

已完成的部分：

- Phase 4 的核心 Rust model、cursor smoothing、Bezier interpolation、click magnification state/effect、metadata sidecar、Tauri command、Preview UI wiring 都已落地。
- React 没有接收媒体帧或 cursor sample stream，符合架构中 “媒体主链路不进前端 JS” 的红线。
- 光标效果仍是录后处理，没有把 smoothing/interpolation/click effect 放入 ScreenCaptureKit callback。
- `show_system_cursor` 已接入 SCK raw cursor visibility 控制点。

未完全完成或仍需 gate 的部分：

- “cursor event 与 video frame 同一时间基” 尚未正确完成；本轮实现需要重新整改。
- 真实导出视频包含光标平滑/点击放大仍依赖 Phase 6 FFmpeg compositor，Phase 4 只能验收 timeline contract。
- Native Safety Gate 和双光标检查仍需人工 macOS 验证。
- async timeline build 尚未按 session 归属隔离，不能算资源路径完全可靠。

### 11.6 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor algorithm 进入 ScreenCaptureKit callback。
- SCK callback 仍通过 bounded channel 的 `try_send_drop_newest` 投递。
- `build_cursor_effect_timeline` 已移入 `spawn_blocking`，减少 command executor 阻塞。
- 但 SCK callback 内的像素拷贝和 audio conversion 后才取 timestamp，会影响媒体时间语义；这属于 Critical 1，不是算法阻塞但会影响主链路数据正确性。

内存安全：

- `cursor_source.rs` 中 `CGEventCreate` null check 后 `CFRelease`，表面上配对；仍需 Native Safety Gate 人工逐行确认。
- audio block buffer 在多条 return path 上有 release，未看到本轮新增明显泄漏。
- samples 已 bounded + `VecDeque`；clicks bounded 但可进一步改 `VecDeque`。
- timeline JSON 仍会一次性读 metadata、构建 frames、pretty serialize；这是录后内存峰值风险，不在捕获 callback。

线程安全/竞态：

- Rust 数据竞争层面未发现明显问题，共享状态使用 `Mutex`/`AtomicBool`。
- 逻辑竞态仍存在：post-process build 没有 session id，可能跨 session 写回 stale path。
- 前端 timer 未 cleanup，会在组件销毁后触发后端任务，扩大上述竞态。

资源释放路径：

- stop cleanup 主路径较上一轮明显改善。
- 仍需补充：前端 timer cleanup、post-process job cancel/session guard、失败 progress 收敛。

### 11.7 本轮实际验证

```bash
git diff --check 9bfc4ee6fa853c50730bcaacda285e84ba6316fc
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm test -- --run
npm run build
```

结果：

- `git diff --check 9bfc4ee6fa853c50730bcaacda285e84ba6316fc`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, 103 tests
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 24 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, 24 warnings
- `npm test -- --run`: PASS, 25 tests；输出 `Window.scrollTo()` 未实现提示
- `npm run build`: PASS

### 11.8 Round 1 Ready To Merge?

**Ready to merge: No / With fixes**

优先整改顺序：

1. 修复 ScreenCaptureKit media timestamp：恢复使用 CMSampleBuffer PTS，并映射到统一 session origin。
2. 给 `build_cursor_effect_timeline` 加 session id/generation guard，防止旧 post-process 结果污染新录制。
3. 修复 Preview debounce：await config 成功后再 build，并在 unmount 时清理 timer。
4. 更新 Phase 4 checklist 与测试：增加 timestamp-origin proof、debounce ordering、session stale path race 测试。
5. 收尾 clicks `VecDeque`、Paused/Processing 状态拒绝和 progress failure event。

## 12. Round 2 整改复审（2026-05-27）

> 评审范围：用户已完成 `## 11. Round 1 整改复审（2026-05-27）` 所列问题后的 Phase 4 相关代码。
> 当前结论：With fixes，不建议直接合并；没有发现新的捕获主链路阻塞或明显内存安全硬伤，但仍存在时间戳边界、cursor runtime 停止时机、stale build 返回语义和测试缺口。
> 说明：本轮曾启动独立审查代理，但代理超时未返回有效结果；以下结论基于本地静态审查、diff 审查和实际验证命令。

### 12.1 本轮已确认修复或改善

- `src-tauri/src/platform/macos/screen_capture_kit.rs:149-155`：video timestamp 改为在 callback 入口读取 `CMSampleBuffer` PTS，再映射到 session-relative timestamp，修复了上一轮“像素拷贝后才取 wall-clock timestamp”的核心回归。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:204-209`：system audio timestamp 同样在 callback 入口读取 PTS，避免 audio conversion 后才取时间。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:124-141`：新增 `normalize_pts()`，通过 `pts_origin` 将 CoreMedia PTS 映射到共享 `SessionClock` 域。
- `src-tauri/src/platform/macos_service.rs:117-134`：`SessionClock` 在启动 ScreenCaptureKit、microphone、cursor runtime 前创建，并传入 `start_combined()`，统一 session clock 的方向正确。
- `src-tauri/src/platform/macos_service.rs:45-60` 与 `src-tauri/src/lib.rs:356-469`：新增 `session_id` / metadata path guard，旧 post-process job 不再能直接写回新 session 的 `last_effect_timeline_path`。
- `src/components/preview-view.tsx:68-78`：debounce timer 增加 unmount cleanup，避免离开 preview 后旧 timer 继续触发后端任务。
- `src/components/preview-view.tsx:80-95`：debounced job 改为先 `setBeautifyConfig(nextConfig)`，成功后再 `buildCursorEffectTimeline()`，修复上一轮配置写入和 build 的顺序风险。
- `src-tauri/src/app/cursor_metadata_runtime.rs:36-37,107-129`：click metadata 也改为 `VecDeque`，满载后 `pop_front()`，消除 click cap 满载后的 O(n) `remove(0)`。
- `src-tauri/src/lib.rs:345-349`：`build_cursor_effect_timeline` 已拒绝 `Recording | Paused | Processing`，比上一轮只拒绝 `Recording` 更明确。
- `src-tauri/src/app/events.rs:82-87` 与 `src-tauri/src/lib.rs:430-453`：post-process progress 增加 `error` 字段，失败时会发错误 progress。
- `tests/phase-4-w7-w8-checklist.md:13,69`：checklist 更新了同一时间基说明和 clippy warning 数量。

### 12.2 Important Findings

#### Important 1: `normalize_pts()` 以首个 callback PTS 建立全局 origin，仍可能压扁或放大时间戳

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:124-141`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:149-155`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:204-209`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:493-508`

现象：

`normalize_pts()` 使用第一个到达 callback 的 PTS 作为 `first_pts`，同时记录当时的 `session_clock.elapsed_nanos()`。后续 PTS 统一执行：

```rust
pts_nanos.saturating_sub(first_pts).saturating_add(session_at_first)
```

这比上一轮“用 callback 处理完成时间做 timestamp”明显更好，但仍有边界风险：

- 第一个到达的 callback 不一定是整个 session 最早的有效 media PTS。video/audio 两路 callback 可能乱序到达。
- 第一个 callback 可能随后因为 `image_buffer` 缺失、format_desc 缺失、ASBD 无效、samples 为空等原因被丢弃，但它已经建立了全局 origin。
- 如果后续有效 sample 的 PTS 早于 `first_pts`，`saturating_sub()` 会把这段差值压成 0，导致多个真实不同时间的 sample 映射到同一个 `session_at_first` 附近。
- `extract_timestamp_nanos()` 失败时返回 0；如果这个 0 先建立 origin，后续真实 PTS 会映射成异常大的 session timestamp。

为什么重要：

Phase 4 的核心正确性依赖 cursor event、video frame 和 audio chunk 的可解释时间对齐。当前方案已经恢复了 PTS，但 origin 建立策略仍可能在异常/乱序 callback 下产生 timeline 折叠或漂移。它不一定频繁发生，但一旦发生会表现为 click effect 与画面动作明显错位，且自动化测试目前没有覆盖。

建议修复：

1. 将 `extract_timestamp_nanos()` 改为返回 `Option<u64>`，PTS 读取失败时丢弃该 sample 或使用明确 fallback，不允许 `0` 静默参与 origin。
2. 只在确认当前 sample 会被发送前建立 origin，避免无效 sample 污染 `pts_origin`。
3. 对 video/audio 分别保留首个有效 PTS，再用 session start 或最早有效 PTS 建立一致 origin；不要让乱序晚到/早到 callback 被 `saturating_sub()` 静默压扁。
4. 增加纯函数测试覆盖：
   - invalid first PTS 不建立 origin。
   - later valid PTS 不因 first invalid PTS 映射成异常大值。
   - audio first、video second 且 video PTS 更早时不会折叠。
   - out-of-order PTS 的处理策略可解释。

#### Important 2: cursor runtime 停止晚于媒体 sink 清空，metadata duration 可能长于真实视频

位置：

- `src-tauri/src/platform/macos_service.rs:197-231`
- `src-tauri/src/app/cursor_metadata_runtime.rs:149-162`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:727-766`

现象：

`MacRecordingService::stop()` 当前顺序是：

1. `ScreenCapture::stop(&mut self.screen_capture)`。
2. `self.mic_capture.stop()`。
3. signal consumer stop。
4. join consumer。
5. stop cursor runtime。

而 `MacScreenCapture::stop()` 会先 clear sinks，再等待 `stopCaptureWithCompletionHandler` 最多 5 秒。cursor runtime 在这期间仍会按 fps 采样，`CursorMetadataRuntime::finish()` 的 `duration_nanos` 也来自停止 cursor runtime 时的 `session_clock.elapsed_nanos()`。

为什么重要：

如果 SCK stop 等待较慢，cursor metadata 的 duration 和 samples 会覆盖一段“没有有效 video frame 会被接收”的时间。后续 `CursorEffectEngine::build_timeline()` 会按 metadata duration 生成 cursor frames，可能长于实际视频素材。Phase 6 compositor 消费时可能出现尾部 cursor timeline 超出素材，或 click effect 尾部错位。

建议修复：

1. 在媒体 sink 清空/停止捕获的同一时刻停止 cursor runtime，至少应早于等待长耗时 stop completion 和 consumer join。
2. 更稳妥：用最后一个有效 video frame timestamp 或 writer finalized duration 作为 `RecordingMetadata.duration_nanos`，而不是 cursor runtime stop wall-clock。
3. 增加测试或 fake service 验证：SCK stop 延迟不会让 metadata duration 比 video duration 多出 stop 等待时间。

#### Important 3: stale post-process build 不再污染 service，但仍会向调用方返回旧 timeline 成功

位置：

- `src-tauri/src/lib.rs:356-469`
- `src-tauri/src/lib.rs:472-485`

现象：

本轮 `session_id` guard 已阻止旧 build 写回 `MacRecordingService`，这是正确修复。但如果 build 完成时 session 已变化，代码只是跳过 `set_last_effect_timeline_path()`，随后仍发送 `progress: 100` 并返回 `CursorEffectSummaryPayload { effect_timeline_path: old_path }`。

为什么重要：

- service 不再被污染，但调用方仍拿到了旧 session 的成功 summary。
- preview/export UI 可能把该旧 path 当作当前操作成功结果。
- 这会让 session 归属问题从 service state 转移到 command return value/UI 层。

建议修复：

1. session mismatch 时返回明确的 stale/cancelled error，不发送成功 progress。
2. 如果需要保留文件，可以写入后删除或标记 orphan，但不要让调用方认为它属于当前 session。
3. 增加测试覆盖：build 过程中 session id 改变时，command 返回 stale error，且不 emit success。

### 12.3 Minor Findings

#### Minor 1: Preview debounce 修复缺少回归测试

位置：

- `src/components/preview-view.tsx:68-95`
- `src/App.test.tsx:820-850`

现象：

代码层面已经修复了 debounce cleanup 和 `setBeautifyConfig -> buildCursorEffectTimeline` 顺序，但前端测试仍只断言 `set_beautify_config` 被调用，没有使用 fake timers 验证：

- 300ms 内连续变更只 build 一次。
- unmount 后 timer 不再触发。
- `build_cursor_effect_timeline` 发生在 `set_beautify_config` resolve 之后。
- `set_beautify_config` reject 时不会继续 build。

建议修复：

补充 fake timer 测试，锁住这次整改，避免后续 UI 调整再次破坏顺序。

#### Minor 2: checklist 对“同一时间基”的勾选偏乐观

位置：

- `tests/phase-4-w7-w8-checklist.md:13`

现象：

checklist 已说明 “仍需 macOS 手动验证时间对齐精度”，但仍将该项标为 `[x]`。鉴于 Important 1 的 origin 边界还未闭环，建议改成 `[ ]` 或拆成两项：

- `[x] 代码路径已使用 PTS -> session origin 映射`
- `[ ] 异常/乱序 PTS 与真实 macOS 录制对齐已验证`

### 12.4 Phase 4 完整性复审结论

已完成：

- `core/timeline.rs`、`core/processor.rs`、`media/cursor_engine.rs` 构成了 Phase 4 cursor effect timeline 的核心模型和算法边界。
- `app/cursor_metadata_runtime.rs` 与 `platform/macos/cursor_source.rs` 能在录制期采集 cursor samples 和 click transitions。
- `MacRecordingService.stop()` 能写 cursor metadata sidecar，并将 path 放入 `RecordingResult`。
- `build_cursor_effect_timeline` 能读取 metadata、按 beautify config 生成 effect timeline sidecar。
- `PreviewView` 已接入 beautify config、timeline build 和 export command。
- `CaptureConfig.show_system_cursor` 已接入 ScreenCaptureKit `setShowsCursor`，为双光标策略提供控制点。
- React 没有接收视频帧、音频帧或 cursor sample stream，符合架构红线。

仍未完全完成或需要 gate：

- 时间基实现比上一轮更接近正确，但 `pts_origin` 建立策略仍需修复和测试证明。
- cursor metadata duration 与真实 video duration 的停止边界还不够严谨。
- stale build 的 service 污染已修复，但 command return value 仍可能返回旧 session 的成功结果。
- 真实导出视频包含光标平滑/点击放大仍依赖 Phase 6 FFmpeg compositor。
- Native Safety Gate 与双光标行为仍需 macOS 人工验证。

### 12.5 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click effect 构建进入 ScreenCaptureKit callback。
- SCK callback 仍通过 bounded channel `try_send_drop_newest` 投递 frame/chunk。
- timeline build 已在 `spawn_blocking` 中执行，不直接占用 async executor 做重 CPU/JSON 工作。
- callback 内仍存在必要的视频像素拷贝和 audio conversion；这是现有 SCK data ownership 约束，不是 Phase 4 新增算法阻塞。

内存安全：

- `cursor_source.rs` 的 `CGEventCreate` / `CFRelease` 路径表面配对，仍需 Native Safety Gate 人工确认。
- SCK audio `block_buffer` 在当前 return path 中有 release，未看到本轮新增明显泄漏。
- samples 和 clicks 都已使用 `VecDeque` + cap，录制期 metadata 不再无界增长。
- timeline 构建仍会一次性读 metadata、生成 frames、pretty serialize JSON；这是录后内存峰值风险，不阻塞 capture callback。

线程安全与竞态：

- Rust 数据竞争层面未发现明显问题，共享状态使用 `Mutex` / `AtomicBool`。
- `session_id` guard 解决了 service path 跨 session 写回问题。
- stale build 的 command return value 仍有逻辑竞态，需要返回 cancelled/stale。
- 前端 debounce timer 已有 cleanup，资源释放路径较上一轮改善。

资源释放路径：

- `MacRecordingService.stop()` 的资源释放聚合路径较上一轮保持改善。
- 剩余主要是 cursor runtime stop 时机：应与媒体 capture 有更明确的停止边界，避免 metadata duration 覆盖停止等待时间。

### 12.6 BUG.md 预防规则检查

检查结果：

- 源码中未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 源码中未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- `src/components/recording-panel.tsx` 中仍有既有 `motion.button whileTap`，这不是 BUG-003 的直接父容器拦截模式，本轮不作为阻塞问题。

执行命令：

```bash
rg -n 'data-tauri-drag-region="false"|whileTap' src src-tauri
```

### 12.7 本轮实际验证

```bash
git diff --check 9bfc4ee6fa853c50730bcaacda285e84ba6316fc
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm test -- --run
npm run build
```

结果：

- `git diff --check 9bfc4ee6fa853c50730bcaacda285e84ba6316fc`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, 104 tests
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 21 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, 21 warnings
- `npm test -- --run`: PASS, 25 tests；输出 `Window.scrollTo()` 未实现提示
- `npm run build`: PASS

### 12.8 Round 2 Ready To Merge?

**Ready to merge: No / With fixes**

优先整改顺序：

1. 修复 `normalize_pts()` 的 origin 建立策略：`extract_timestamp_nanos()` 返回 `Option<u64>`，无效 sample 不建立 origin，乱序/跨流 PTS 有明确策略和测试。
2. 调整 cursor runtime stop 时机或 metadata duration 来源，确保 cursor timeline 不长于真实 video duration。
3. session mismatch 时让 `build_cursor_effect_timeline` 返回 stale/cancelled error，不 emit 100% success，不向调用方返回旧 path。
4. 补 Preview debounce fake timer 测试，覆盖顺序、cleanup、reject 不 build。
5. 更新 checklist，把“时间基同源”拆成代码路径完成和真实/异常边界验证两个 gate。

## 13. Round 3 整改复审（2026-05-27）

> 评审范围：用户已完成 `## 12. Round 2 整改复审（2026-05-27）` 所列问题后的 Phase 4 相关代码。
> 当前结论：With fixes，不建议直接合并。Phase 4 主体功能已落地，且 stale build 返回语义、无效 PTS 处理、Preview debounce 测试等有明显改善；但 cursor runtime 停止边界和 PTS origin 乱序语义仍未完全闭环。
> 交叉验证：本轮使用 `$superpowers:requesting-code-review` 启动独立审查代理 `Helmholtz`，其结论与本地审查一致：Round 2 的 cursor runtime stop timing 和 PTS origin 边界仍需继续整改。

### 13.1 本轮已确认修复或改善

- `src-tauri/src/platform/macos/screen_capture_kit.rs:513-529`：`extract_timestamp_nanos()` 已改为返回 `Option<u64>`，PTS 无效时返回 `None`，不再用 `0` 静默污染 `pts_origin`。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:160-165` 与 `src-tauri/src/platform/macos/screen_capture_kit.rs:219-224`：video/audio callback 入口先读取 PTS；PTS 无效时直接丢弃 frame/chunk。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:195-198` 与 `src-tauri/src/platform/macos/screen_capture_kit.rs:350-353`：只有在 frame/chunk 通过基本校验后才调用 `normalize_pts()`，避免无效 sample 建立全局 origin。
- `src-tauri/src/lib.rs:465-480`：session/path mismatch 时现在会发 error progress 并返回 `Err("录制会话已变更，光标效果构建已取消")`，不再向调用方返回旧 timeline path 成功。
- `src/App.test.tsx:889-983`：新增 3 个 Preview debounce 回归测试，覆盖连续变更只 build 一次、unmount 后不触发 build、`set_beautify_config` reject 后不继续 build。
- `tests/phase-4-w7-w8-checklist.md:13-14`：将“同一时间基”拆成代码路径已完成和真实 macOS 对齐待人工验证两个 gate，比上一轮 `[x]` 单项更准确。
- `tests/phase-4-w7-w8-checklist.md:66-73`：验证摘要更新到 Round 3：Rust 108 tests、前端 28 tests。

### 13.2 Important Findings

#### Important 1: cursor runtime 仍然停得太晚，metadata duration 仍可能长于真实视频

位置：

- `src-tauri/src/platform/macos_service.rs:200-211`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:782-796`
- `src-tauri/src/app/cursor_metadata_runtime.rs:149-162`

现象：

`MacRecordingService::stop()` 当前顺序仍是：

1. `ScreenCapture::stop(&mut self.screen_capture)`。
2. `self.mic_capture.stop()`。
3. stop cursor runtime。
4. signal consumer stop。
5. join consumer。

问题在于 `MacScreenCapture::stop()` 会先 clear sinks，然后调用 `stopCaptureWithCompletionHandler` 并最多等待 5 秒完成回调。cursor runtime 要到这个等待结束后才停止，而 `CursorMetadataRuntime::finish()` 使用 `session_clock.elapsed_nanos()` 作为 `RecordingMetadata.duration_nanos`。

为什么重要：

这意味着 ScreenCaptureKit sink 已经不再接收有效 video frame 后，cursor runtime 仍可能继续采样，且这段 stop 等待时间会被计入 cursor metadata duration。后续 `CursorEffectEngine` 会按更长的 duration 生成 `EffectTimeline.frames`，Phase 6 compositor 消费时可能出现 cursor timeline 长于真实视频、尾部 cursor frame 无对应画面、点击效果尾部错位等问题。

本轮整改状态：

- 代码注释写了“Cursor stops at the moment media capture ends”，但实际 cursor stop 发生在 `ScreenCapture::stop()` 返回之后，而不是 sink clear 或最后有效 media timestamp 的边界。
- 因此 Round 2 的“cursor runtime 停止时机”整改没有真正完成。

建议修复：

1. 在调用 `ScreenCapture::stop()` 前或在清空 media sink 的同一逻辑边界停止 cursor runtime。
2. 更稳妥的方案是由 writer/consumer 记录最后一个有效 video frame timestamp，以该 timestamp 修正 `RecordingMetadata.duration_nanos`。
3. 增加 fake stop-delay 测试：模拟 SCK stop 等待 5 秒，断言 cursor metadata duration 不包含这段等待。
4. 如果必须先 stop SCK 再 stop cursor runtime，至少在 stop 前捕获 `session_clock.elapsed_nanos()` 作为 media stop boundary，并传给 cursor recorder finish。

#### Important 2: `normalize_pts()` 仍会压扁乱序有效 PTS，且首个 origin 的 session time 仍可能包含 callback 内处理延迟

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:124-151`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:160-198`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:219-353`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:1195-1201`

现象：

`compute_normalized_pts()` 对已有 origin 的处理仍是：

```rust
pts_nanos.saturating_sub(first_pts).saturating_add(session_at_first)
```

如果 audio callback 先到并建立了 `first_pts`，随后 video callback 带着更早但有效的 PTS 到达，这个更早 PTS 会被 `saturating_sub()` 压成 0，最终映射到 `session_at_first`。新增测试 `pts_normalize_saturates_negative_delta_to_origin` 还明确固化了这个行为。

另一个细节是：video 在 callback 入口读 PTS，但直到完成 image buffer 获取、lock、像素拷贝、unlock 后才调用 `normalize_pts()`；audio 也是完成 format 校验、AudioBufferList 获取、PCM 转换后才 normalize。若该 sample 是第一个有效 sample，`normalize_pts()` 内读取的 `session_clock.elapsed_nanos()` 仍包含 callback 内处理延迟，不是 callback entry 或 session start 对应的时间。

为什么重要：

Phase 4 需要可解释的 cursor/video/audio 时间对齐。无效 PTS 不污染 origin 已修复，但有效 PTS 乱序、跨流先后到达、首个 origin 的处理延迟仍会造成边界错位。这个风险在常规短录制中可能不明显，但它会让“同一真实事件映射到同一 session-relative 时间”的证明不成立。

建议修复：

1. 在 callback entry 同时捕获 `pts_nanos` 和 `session_at_callback_entry`，后续校验通过后用这个 entry session time 建立 origin，而不是在拷贝/转换后再读 session clock。
2. 设计乱序 PTS 策略，不要默默压扁早于 first_pts 的有效 sample。可选方向：
   - 基于 session start 建立固定 PTS offset，而不是基于首个到达 sample。
   - 维护 earliest valid PTS 并允许重新基准化，或显式丢弃早于 origin 的 out-of-order sample 并记录统计。
   - 对 video/audio 分别建立 origin，再在上层对齐到共同 session boundary。
3. 修改/替换 `pts_normalize_saturates_negative_delta_to_origin`，不要把“压扁更早有效 PTS”作为期望行为固化。
4. 增加测试：audio 先到但 video PTS 更早、video 先到但 audio PTS 更早、首个 sample callback 处理很慢时 origin 不包含处理延迟。

### 13.3 Minor Findings

#### Minor 1: Preview debounce 测试仍未完全锁住 resolve-before-build 顺序

位置：

- `src/components/preview-view.tsx:80-95`
- `src/App.test.tsx:889-983`

现象：

实现层面 `setBeautifyConfig(nextConfig).then(() => buildCursorEffectTimeline())` 是正确方向，且本轮新增测试覆盖了 debounce、unmount cleanup 和 reject 不 build。但是测试还没有用一个手动控制 resolve 的 delayed Promise 来证明 `build_cursor_effect_timeline` 一定发生在 `set_beautify_config` resolve 之后。

另外，unmount/reject 两个测试使用真实 `setTimeout(500)`，会让测试更慢，也可能在未来 CI 压力下变得不稳定。

建议修复：

1. 用 fake timers + controllable Promise 覆盖严格顺序：
   - advance 300ms 后，`set_beautify_config` pending 时 build 不发生。
   - resolve `set_beautify_config` 后，build 才发生。
2. 将 unmount/reject 测试也迁移到 fake timers，避免真实等待。

#### Minor 2: 本轮新增 clippy warning：`explicit_auto_deref`

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:151`

现象：

`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 通过，但 warning 数从 checklist 记录的 21 变为 22，其中新增：

```rust
compute_normalized_pts(&mut *origin, pts_nanos, session_now)
```

clippy 建议改为：

```rust
compute_normalized_pts(&mut origin, pts_nanos, session_now)
```

这不是功能阻塞，但作为 Round 3 新增 warning，建议随下一轮整改顺手清掉，避免 checklist warning 数继续漂移。

### 13.4 Phase 4 完整性复审结论

已完成：

- 光标元数据采集、`CursorSample` / `CursorClick` / `EffectTimeline` 模型、移动平均平滑、Bezier 插值、点击放大状态机、effect timeline 生成均已落地。
- `CursorMetadataRuntime` 使用独立线程采集 metadata，未将 cursor effect 算法放入 ScreenCaptureKit callback。
- metadata sidecar 和 effect timeline sidecar 写入路径已存在，`RecordingResult` 带有对应 path。
- `set_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入，并且 timeline build 运行在 `spawn_blocking`。
- Preview UI 已接入 Tauri command，且新增 debounce 与 cleanup。
- `show_system_cursor` 已接入 ScreenCaptureKit `setShowsCursor`，为避免双光标提供控制点。
- React 未承载音视频帧流或 cursor sample stream，符合架构红线。

仍未完全完成或需 gate：

- cursor metadata duration 与真实 video duration 的停止边界仍不严谨。
- PTS origin 的有效乱序样本处理还不够正确。
- 真实 macOS 录制中的 PTS/click/video 动作对齐仍需人工验证。
- Native Safety Gate 和双光标检查仍需人工逐行/手动确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 compositor。

### 13.5 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click effect 进入 ScreenCaptureKit callback。
- SCK callback 仍使用 bounded channel 的 `try_send_drop_newest`，不会因为 consumer 慢而阻塞等待。
- timeline build 已放入 `spawn_blocking`，不会在 async command executor 中同步做重 CPU/JSON 工作。
- callback 内仍存在必要的视频像素拷贝和 audio conversion，这是 CVPixelBuffer/AudioBuffer 生命周期要求，不是 Phase 4 算法引入的额外阻塞。

内存安全：

- `cursor_source.rs` 的 `CGEventCreate` / `CFRelease` 表面配对；仍需 Native Safety Gate 人工确认。
- SCK audio `block_buffer` 在当前 return path 中有 release，未发现本轮新增泄漏。
- `CursorMetadataRecorder` 的 samples 和 clicks 都已 bounded + `VecDeque`，录制期 metadata 不再无界增长。
- timeline 构建仍会一次性读 metadata、生成 frames、pretty serialize JSON；属于录后内存峰值风险，不在 capture callback。

线程安全与竞态：

- Rust 数据竞争层面未发现明显问题，共享状态使用 `Mutex` / `AtomicBool`。
- `session_id` + metadata path guard 已解决 stale build 写回 service 以及向调用方返回旧 path 成功的问题。
- Preview debounce timer 已有 cleanup，离开 preview 后旧 timer 不应继续触发后端任务。
- 剩余主要是时间语义竞态：有效 PTS 乱序到达时当前 origin 策略会压扁时间戳。

资源释放路径：

- `MacRecordingService.stop()` 聚合错误并尽量执行 mic reset、cursor metadata sidecar、state transition，较早期版本明显改善。
- 但 cursor runtime 的停止边界仍在 `ScreenCapture::stop()` 之后，无法证明 metadata duration 与 media end 完全一致。

### 13.6 BUG.md 预防规则检查

检查结果：

- 源码中未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 源码中未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- `src/components/recording-panel.tsx` 中仍有既有 `motion.button whileTap`，不属于 BUG-003 的父容器拦截模式，本轮不作为阻塞问题。

执行命令：

```bash
rg -n 'data-tauri-drag-region="false"|whileTap' src src-tauri
```

### 13.7 本轮实际验证

```bash
git diff --check 9bfc4ee6fa853c50730bcaacda285e84ba6316fc
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm test -- --run
npm run build
```

结果：

- `git diff --check 9bfc4ee6fa853c50730bcaacda285e84ba6316fc`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, 108 tests
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 22 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, 21 warnings
- `npm test -- --run`: PASS, 28 tests；输出多条 `Window.scrollTo()` 未实现提示
- `npm run build`: PASS

### 13.8 Round 3 Ready To Merge?

**Ready to merge: No / With fixes**

优先整改顺序：

1. 修复 cursor runtime stop boundary：在 media capture end 边界停止 cursor runtime，或用最后有效 video timestamp 修正 metadata duration。
2. 修复 PTS origin 乱序语义：不要用 `saturating_sub()` 静默压扁早于 first_pts 的有效 sample；callback entry 同时捕获 PTS 和 session time。
3. 替换 `pts_normalize_saturates_negative_delta_to_origin` 测试，不要固化错误压扁行为；增加跨流乱序 PTS 测试。
4. 补严 Preview debounce 顺序测试，使用 fake timers + controllable Promise 验证 build 只在 config resolve 后发生。
5. 清理本轮新增 clippy `explicit_auto_deref` warning，并同步 checklist warning 数。

## 14. Round 4 整改复审（2026-05-27）

> 评审范围：用户已完成 `## 13. Round 3 整改复审（2026-05-27）` 所列问题后的 Phase 4 相关代码。
> 当前结论：With fixes，不建议直接合并。Round 3 的两个主要阻塞项（cursor runtime stop boundary、PTS origin 乱序语义）已经明显修复，捕获主链路、资源释放和内存上限路径未发现新的阻塞问题；但仍存在两个会影响下一轮稳定性的 Important 问题：beautify config 前后端状态漂移、同一 session 内多次 timeline build 乱序覆盖。
> 交叉验证：本轮使用 `$superpowers:requesting-code-review` 启动独立审查代理 `Hume`。独立审查未发现 Round 3 范围内剩余阻塞问题，并确认 stop boundary、PTS origin、Preview debounce、clippy warning 与 BUG.md 检查均已改善。本地复审在此基础上补充了两个跨 session / 同 session 的配置与异步构建一致性问题。

### 14.1 本轮已确认修复或改善

- `src-tauri/src/platform/macos_service.rs:107-110`：新 session 开始时已清空 `last_cursor_metadata_path`、`last_effect_timeline_path`，并递增 `session_id`，修复上一段录制 effect timeline path 污染下一段录制的问题。
- `src-tauri/src/platform/macos_service.rs:197-207`：`stop()` 已在 native capture stop 之前停止 `CursorMetadataRuntime`，避免 `stopCaptureWithCompletionHandler` 最多 5 秒等待时间继续计入 cursor metadata duration。
- `src-tauri/src/platform/macos_service.rs:235-283`：cursor metadata sidecar 写入失败不再提前跳过后续 cleanup；错误会聚合，mic level reset 和 state machine terminal transition 路径仍会执行。
- `src-tauri/src/app/cursor_metadata_runtime.rs:1-14` 与 `src-tauri/src/app/cursor_metadata_runtime.rs:57-121`：samples/clicks 已改为 `VecDeque`，且 samples 和 clicks 都有上限，避免长录制中 `Vec::remove(0)` 的 O(n) 成本和 click metadata 无界增长。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:132-162`：`compute_normalized_pts()` 已改为有符号 delta 归一化，早于 first PTS 的有效 sample 不再被 `saturating_sub()` 直接压扁到 origin。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:170-210` 与 `src-tauri/src/platform/macos/screen_capture_kit.rs:232-368`：video/audio callback 都在入口捕获 `pts_nanos` 和 `session_entry_nanos`，通过校验后再用 callback entry session time 建立/应用 origin，避免 origin 包含像素拷贝或音频转换耗时。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:531-545`：无效 PTS 继续返回 `None` 并丢弃，不污染 `pts_origin`。
- `src-tauri/src/lib.rs:334-497`：`build_cursor_effect_timeline()` 已拒绝 Recording/Paused/Processing 状态，timeline 构建放入 `spawn_blocking`，并在写回 effect path 前用 `session_id + metadata_path` 检查 stale build。
- `src/components/preview-view.tsx:68-96`：Preview 侧已加入 300ms debounce、unmount cleanup，并保证 `setBeautifyConfig(nextConfig)` resolve 后才调用 `buildCursorEffectTimeline()`。
- `src/App.test.tsx:889-1035`：前端测试已覆盖连续变更只 build 一次、unmount 清理 timer、config reject 不 build，以及 config pending 时不提前 build。
- `tests/phase-4-w7-w8-checklist.md:13-14` 与 `tests/phase-4-w7-w8-checklist.md:60-73`：checklist 保持“代码路径完成 / 真实 macOS 对齐待人工验证”的拆分，并记录 Round 3 自动化验证结果。

### 14.2 Important Findings

#### Important 1: 前后端存在两份 beautify config 真相源，跨录制 session 会漂移

位置：

- `src/components/preview-view.tsx:47-51`
- `src/components/preview-view.tsx:59-66`
- `src/components/preview-view.tsx:80-95`
- `src-tauri/src/lib.rs:277-290`
- `src/App.tsx:138-151`

现象：

`PreviewView` 每次挂载时都会用本地默认值初始化：

```ts
const [cursorMagnification, setCursorMagnification] = useState(true)
const [magnificationFactor, setMagnificationFactor] = useState([2])
const [cursorSmoothing, setCursorSmoothing] = useState(true)
const [autoTrimSilences, setAutoTrimSilences] = useState(false)
const [trimSensitivity, setTrimSensitivity] = useState<'low' | 'medium' | 'high'>('medium')
```

但后端 `AppState.beautify_config` 是进程级持久状态。Preview 只在用户主动改控件时调用 `set_beautify_config`，没有在进入 Preview 时从后端读取当前配置，也没有在开始新录制前把前端当前 UI 默认值重新同步到后端。

同时，开始录制前 `App.tsx` 会调用 `setCaptureMode()`；后端 `set_capture_mode()` 会读取后端保存的 `beautify_config` 来决定 `CaptureConfig.show_system_cursor`：

```rust
let show_system_cursor =
    !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);
```

为什么重要：

这会产生跨 session 的状态漂移。例如：

1. 录制 A 完成后进入 Preview。
2. 用户关闭光标放大 / 光标平滑，后端保存为 `false/false`。
3. 用户返回录制，再完成录制 B。
4. 录制 B 的 Preview 重新挂载，UI 又显示默认 `cursorMagnification=true`、`cursorSmoothing=true`。
5. 但后端仍可能保存上一轮 `false/false`，并且下一次 `set_capture_mode()` 会按旧后端配置决定是否录入系统光标。

结果可能是：

- UI 显示美化开启，但后端 build timeline 时按旧配置关闭点击放大 / 平滑。
- UI 显示默认开启光标美化，但下一次录制仍按旧后端配置让 `show_system_cursor=true`，后续如果又生成美化光标，可能重新引入双光标风险。
- 录制素材 cursor visibility、effect timeline 和 Preview 控件状态三者不再可解释。

风险类型：

- Phase 4 功能完整性风险。
- 双光标策略一致性风险。
- 跨录制 session 状态污染。

建议修复：

1. 将 beautify config 提升为 `App.tsx` 的单一状态源，由 Preview 通过 props 使用和修改。
2. 或新增 `get_beautify_config` Tauri command，`PreviewView` 挂载时从后端初始化 UI 状态。
3. `set_capture_mode()` 不应隐式读取一个可能与 UI 脱节的 beautify config；可以在 `startRecording` 前显式同步当前 beautify config，或把 `show_system_cursor` 决策绑定到当前录制 session 的 config snapshot。
4. 为跨 session 增加回归测试：
   - Preview 中关闭 cursor smoothing。
   - 返回 idle 再进入下一次 Preview。
   - 断言 UI、后端 `set_beautify_config`、`set_capture_mode` 对 `show_system_cursor` 的决策保持一致。

建议验收：

- 新增前端测试覆盖 Preview remount 后控件状态不回到与后端不一致的默认值。
- 新增 Rust 或集成测试覆盖 `show_system_cursor` 由当前 session 的 beautify config 决定，而不是上一段录制残留状态。

#### Important 2: 同一 session 内多个 timeline build 仍可能乱序覆盖最新结果

位置：

- `src/components/preview-view.tsx:83-95`
- `src-tauri/src/lib.rs:377-427`
- `src-tauri/src/lib.rs:457-480`

现象：

Preview 侧 debounce 只取消尚未触发的 timer：

```ts
if (debounceRef.current) {
  clearTimeout(debounceRef.current)
}
debounceRef.current = setTimeout(() => {
  debounceRef.current = null
  void setBeautifyConfig(nextConfig)
    .then(() => buildCursorEffectTimeline())
    .catch(...)
}, 300)
```

一旦某次 timer 已经触发，`setBeautifyConfig()` 和 `buildCursorEffectTimeline()` 就会进入后端。后续用户继续拖动 slider 或切换开关时，只能取消新的未触发 timer，不能取消已经在 `spawn_blocking` 中运行的 timeline build。

后端当前 stale guard 只检查：

```rust
service.current_session_id() == session_id
    && service.last_cursor_metadata_path().as_deref() == Some(metadata_path.as_str())
```

这个 guard 可以阻止“旧录制 session 的 build 写回新 session”，但不能区分同一 session 内不同 beautify config 的多个 build。若 build A 使用旧 config、build B 使用新 config，且 build A 比 build B 更晚完成，build A 仍会通过 session/path 检查并覆盖 `last_effect_timeline_path`。

为什么重要：

长录制 metadata 较大时，timeline build + pretty JSON 写盘耗时可能明显增加。用户拖动 `magnificationFactor` 或快速切换 smoothing/magnification 时，会产生多个同 session build。乱序完成会导致：

- UI 最后显示的控件值与后端保存的 effect timeline 文件不匹配。
- export 可能使用旧参数生成的 timeline。
- 调试时难以判断某个 `cursor-effects-*.json` 对应哪一次 UI 配置。

风险类型：

- 录后处理异步一致性风险。
- 资源浪费和重复 JSON 读写风险。
- 后续 Phase 6 compositor 输入不确定风险。

建议修复：

1. 给 beautify config 增加单调递增 `generation` 或 `revision`。
2. `set_beautify_config` 写入后递增 revision，并返回当前 revision。
3. `build_cursor_effect_timeline` 捕获 `(session_id, metadata_path, beautify_revision)`，完成后只有 revision 仍为最新才写回 `last_effect_timeline_path` 和返回 success。
4. 或在后端实现 single-flight / cancel old job：同一 session 同一 metadata path 同一时间只允许最新 build 生效。
5. effect timeline sidecar 中可记录 config snapshot / revision，便于 Phase 6 export 校验。

建议验收：

- 新增测试：同一 session 触发 build A 和 build B，模拟 build B 先完成、build A 后完成，断言最终 `last_effect_timeline_path` 保持 build B，不被 build A 覆盖。
- 新增前端测试：连续 slider 变化后只让最后一次配置对应的 build 被视为成功。

### 14.3 Minor Findings

#### Minor 1: `extract_timestamp_nanos()` 对 `CMTime` 有效性检查仍偏宽

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:531-545`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:132-146`

现象：

`extract_timestamp_nanos()` 当前只检查 `pts.timescale > 0`，然后将：

```rust
let seconds = pts.value as f64 / pts.timescale as f64;
return Some((seconds * 1_000_000_000.0) as u64);
```

没有显式检查：

- `CMTime.flags` 是否包含 valid。
- `pts.value` 是否为负。
- `u64 -> i64` cast 在 `compute_normalized_pts()` 中是否越界。

为什么重要：

真实 ScreenCaptureKit PTS 大概率是有效、非负且远小于 `i64::MAX` 的 host-time 纳秒值，所以这不是当前阻塞项。但 PTS 归一化是 Phase 4 cursor/video/audio 对齐的核心路径，建议把边界写实，避免未来遇到特殊 sample 时发生负值 cast 到巨大 `u64` 或 `u64 as i64` wrap。

建议修复：

1. 检查 `CMTimeFlags` valid 位，invalid/indefinite sample 返回 `None`。
2. 检查 `pts.value >= 0`。
3. `compute_normalized_pts()` 改用 `i128` 或 checked arithmetic，避免 `u64 as i64` 隐式 wrap。
4. 增加测试覆盖负 PTS、invalid flags、超大 PTS。

### 14.4 Phase 4 完整性复审结论

已完成：

- `CursorSample` / `CursorClick` / `EffectTimeline` serde 模型已落地。
- 光标轨迹平滑、Bezier 插值、点击放大状态机、timeline builder 已落地并有单元测试。
- 录制期 cursor metadata 采集在独立 `CursorMetadataRuntime` 线程中完成，没有进入 ScreenCaptureKit callback。
- samples/clicks metadata 已有上限，长录制内存增长受控。
- 停止录制后写入 cursor metadata sidecar，后续可生成 effect timeline sidecar。
- `set_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 边界已接入。
- `build_cursor_effect_timeline` 已放入 `spawn_blocking`，并拒绝录制中构建。
- 前端 Preview 已接入 Tauri command，且有 debounce、unmount cleanup、config resolve 顺序测试。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`，具备避免双光标的底层控制点。
- React 未接收音视频帧流，也未接收 cursor sample stream，符合架构红线。

仍未完全完成或需 gate：

- beautify config 当前还不是单一真相源，跨 session 可能与 UI 脱节。
- 同一 session 内多个 timeline build 缺少 revision/cancel guard，可能乱序覆盖最新结果。
- 真实 macOS 录制中的 CMSampleBuffer PTS 与 click/video 动作对齐仍需人工验证。
- Native Safety Gate 仍需人工逐行检查 `cursor_source.rs` CoreGraphics FFI 和 `screen_capture_kit.rs` SCK FFI。
- 双光标策略仍需 `npm run tauri dev` 手动确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor。

### 14.5 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 进入 ScreenCaptureKit callback。
- SCK callback 仍只做 PTS 提取、必要的 pixel/audio 数据复制转换、bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内有 `Mutex`，但只保护一个 `Option<(u64, u64)>` 的轻量计算；没有等待 UI、磁盘、timeline build 或导出任务。
- timeline build 在录后 command 路径，并使用 `spawn_blocking`。
- cursor polling 独立线程运行，`CursorSnapshotSource::snapshot()` 不在 SCK callback 内调用。

内存安全：

- `cursor_source.rs` 中 `CGEventCreate` 返回 event 后有 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；表面配对正确，仍需 Native Safety Gate 人工确认。
- `screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取并复制到 `Arc<[u8]>`，unlock 后不再引用原始 base address。
- audio `block_buffer` 当前 return path 有 release；本轮未发现新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded，`VecDeque::pop_front()` 避免长录制后每次 O(n) 移动。
- timeline 构建会一次性读 metadata、生成 frames、pretty serialize JSON，这是录后内存峰值风险，不阻塞捕获主链路；后续可按 Phase 6 实际素材规模再优化。

线程安全：

- 共享 service/config 状态使用 `Arc<Mutex<...>>`，数据竞争层面未发现明显问题。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path` 已能阻止跨 session stale build 写回。
- 仍需补同 session build revision guard，防止旧 config build 乱序覆盖新 config build。

资源释放路径：

- `MacRecordingService.stop()` 当前会先停止 cursor runtime，再 stop native capture / mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点人工审查区域。

### 14.6 BUG.md 预防规则检查

检查结果：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现 `motion.div whileTap` 直接包裹交互 Button。
- `src/components/recording-panel.tsx` 中存在 `motion.button whileTap`，这是交互元素自身使用 `whileTap`，不属于 BUG-003 的父容器拦截模式。
- `src/App.test.tsx` 中有 `data-tauri-drag-region="false"` 的断言，属于预防测试，不是源码新增 wrapper。

执行命令：

```bash
rg -n 'data-tauri-drag-region="false"|whileTap' src src-tauri
```

命中解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443`：断言不存在 false drag region。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171`：`motion.button whileTap`，不属于 `motion.div` 直接包 Button。

### 14.7 本轮实际验证

```bash
git diff --check 9bfc4ee6fa853c50730bcaacda285e84ba6316fc
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm test -- --run
npm run build
```

结果：

- `git diff --check 9bfc4ee6fa853c50730bcaacda285e84ba6316fc`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, 108 tests
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 21 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, 21 warnings
- `npm test -- --run`: PASS, 29 tests；输出多条 `Window.scrollTo()` 未实现提示
- `npm run build`: PASS

说明：

- `cargo clippy` 不再包含 Round 3 的 `explicit_auto_deref` warning。
- 当前 21 个 Rust warning 仍主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题。
- `Window.scrollTo()` 是 jsdom 未实现提示，测试仍通过。

### 14.8 Round 4 Ready To Merge?

**Ready to merge: No / With fixes**

本轮没有发现新的 Critical，也没有发现会直接阻塞捕获主链路的 Phase 4 新代码。但仍建议在进入最终合并前完成以下整改：

1. 统一 beautify config 真相源：避免 Preview UI 默认值、后端持久配置、`show_system_cursor` 决策跨 session 漂移。
2. 为同一 session 内 timeline build 增加 revision / request id / cancellation guard：避免旧 config 的慢 build 覆盖新 config 的结果。
3. 收紧 `extract_timestamp_nanos()` 的 `CMTime` 有效性检查，并将 PTS delta 运算改成 checked / `i128`，补充负值、invalid flags、超大值测试。
4. 保持 checklist 中真实 macOS PTS 对齐、Native Safety Gate、双光标人工检查为未完成 gate。
5. Phase 6 接入真实 FFmpeg compositor 后，再做导出视频中的光标平滑和点击放大人工验收。

## 15. Round 5 整改复审（2026-05-28）

> 评审范围：Phase 4 从 `fb6950554ca94950da81da93520893299cc8cf7a` 到 `568004e0d90c19d2f3d7f784ed2c1264c0bb119e`，并额外复审当前工作区 Round 5 未提交整改文件：
>
> - `src-tauri/src/lib.rs`
> - `src-tauri/src/platform/macos/screen_capture_kit.rs`
> - `src/App.test.tsx`
> - `src/components/preview-view.tsx`
> - `src/lib/tauri.ts`
>
> 当前结论：**With fixes**，不建议直接合并。未发现 Critical，也未发现 cursor smoothing / Bezier / click magnification 进入捕获主链路；Round 4 的配置 revision guard、`get_beautify_config`、CMTime 有效性检查已有明显改善。但仍有两个会影响 Phase 4/Phase 6 正确性的 Important 问题：pending beautify config 仍可在导出/返回录制时被丢弃或绕过；当光标美化全部关闭时仍可能生成可渲染 cursor overlay timeline，给 Phase 6 compositor 留下双光标风险。
>
> 交叉验证：本轮使用 `$superpowers:requesting-code-review` 启动独立审查代理 `Planck`。独立审查结论与本地审查一致：无 Critical；核心 Important 是 Preview debounce 下 pending config 可能未落到后端就被 export/back/unmount 绕过；Minor 是 `compute_normalized_pts()` 仍有 `u64 as i64` 边界风险。代理额外执行了 `git diff --check`、targeted cmtime 测试和 `npm test -- --run src/App.test.tsx`，均通过。

### 15.1 本轮已确认修复或改善

- `src-tauri/src/lib.rs:61-64`：`AppState` 新增 `beautify_revision: Arc<AtomicU64>`，为同一 session 内异步 timeline build 的 stale guard 提供 revision 维度。
- `src-tauri/src/lib.rs:325-345`：`set_beautify_config()` 改为写入后返回 revision，新增 `get_beautify_config()`，用于 Preview 挂载时从后端状态初始化 UI。
- `src-tauri/src/lib.rs:366-383` 与 `src-tauri/src/lib.rs:471-500`：`build_cursor_effect_timeline()` 捕获 `(session_id, metadata_path, beautify_revision)`，完成后复核 session、metadata path 和 revision；若配置或 session 已变更，则返回取消错误，不再写回 stale `last_effect_timeline_path`。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:531-565`：新增 `cmtime_to_nanos()`，显式拒绝缺失 `Valid` flag、`PositiveInfinity`、`NegativeInfinity`、`Indefinite`、负值和非正 timescale 的 `CMTime`，避免无效 PTS 污染 `pts_origin`。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:1248-1295`：补充 `cmtime_to_nanos` 边界测试，覆盖 valid、missing valid flag、infinity、indefinite、negative value、zero timescale。
- `src/components/preview-view.tsx:71-91`：Preview 挂载时读取后端 `getBeautifyConfig()`，修复 Round 4 中“Preview UI 每次用硬编码默认值重新挂载”的主要漂移问题。
- `src/App.test.tsx:1038-1065`：新增前端测试 `initializes beautify controls from backend on preview mount`，验证 Preview 控件能从后端初始化。
- `src/lib/tauri.ts:99-108`：`setBeautifyConfig()` 返回 `Promise<number>`，并新增 `getBeautifyConfig()` wrapper，与 Rust command 名称匹配。

### 15.2 Critical Findings

本轮未发现 Critical。

没有发现以下阻塞性问题：

- 未发现 cursor smoothing / Bezier interpolation / click magnification 进入 ScreenCaptureKit callback。
- 未发现新增会阻塞捕获 callback 的磁盘写入、UI 调用、timeline build 或 export 调用。
- 未发现新增明显 use-after-free、未配对 `CFRelease`、CVPixelBuffer unlock 后继续引用 base address 等硬性内存安全问题。
- 未发现 Round 4 已修复的跨 session stale build 写回再次出现。

### 15.3 Important Findings

#### Important 1: pending beautify config 仍可能在导出或返回录制时被丢弃/绕过

位置：

- `src/components/preview-view.tsx:69`
- `src/components/preview-view.tsx:93-101`
- `src/components/preview-view.tsx:103-119`
- `src/components/preview-view.tsx:121-124`
- `src-tauri/src/lib.rs:280-293`
- `src-tauri/src/lib.rs:520-529`

现象：

`PreviewView` 当前仍使用 300ms debounce 延迟写入后端：

```ts
debounceRef.current = setTimeout(() => {
  debounceRef.current = null
  void setBeautifyConfig(nextConfig)
    .then(() => buildCursorEffectTimeline())
    .catch((error) => {
      console.error('光标效果处理失败', error)
    })
}, 300)
```

组件 unmount 时会清理 timer：

```ts
if (debounceRef.current) {
  clearTimeout(debounceRef.current)
}
```

但 `handleExport()` 直接调用 `exportVideo(preset)`，没有先 flush 或 await pending config：

```ts
const handleExport = (preset: ExportPreset) => {
  void exportVideo(preset).catch((error) => {
    console.error('导出失败', error)
  })
}
```

因此存在以下可复现场景：

1. 用户在 Preview 中切换 `光标平滑` 或 `光标放大`。
2. 300ms debounce 尚未触发。
3. 用户立即点击导出。
4. `export_video()` 调用 `build_cursor_effect_timeline()`，但后端 `beautify_config` 仍是旧配置。
5. 导出的 effect timeline 与 UI 最后一刻显示的配置不一致。

另一个场景：

1. 用户切换光标美化配置。
2. 300ms debounce 尚未触发。
3. 用户立即点击“返回录制”。
4. Preview unmount cleanup 清掉 timer，新的配置完全没有写入后端。
5. 下一次 `set_capture_mode()` 会按旧的后端 `beautify_config` 决定 `CaptureConfig.show_system_cursor`：

```rust
let show_system_cursor =
    !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);
```

为什么重要：

- Round 5 虽然新增了 `get_beautify_config()`，但它只解决“挂载时从后端读初值”的问题，没有解决“用户刚改完配置但 debounce 未落库”的问题。
- `show_system_cursor` 是双光标策略的关键控制点。如果 pending config 被清掉，下一段录制可能按旧配置录入或隐藏系统光标。
- `export_video()` 依赖后端当前配置 rebuild timeline。导出前不 flush pending config，会导致导出结果与 UI 可见状态不一致。
- 这是 Phase 4 功能完整性风险，也是 Phase 6 compositor 输入一致性风险。

建议修复：

1. 将 pending config 封装成可复用 `flushBeautifyConfig()`：
   - 清理 pending timer。
   - 立即调用并 await `setBeautifyConfig(latestConfig)`。
   - 成功后再按需要调用 `buildCursorEffectTimeline()`。
2. `handleExport()` 在调用 `exportVideo()` 前必须先 flush 最新 UI config。
3. `onBack` 前也应 flush 或明确丢弃用户未保存修改；考虑产品语义，建议 flush。
4. 配置正在同步时禁用导出按钮，或显示处理中状态，避免重复 export/build。
5. 更稳妥的后端 contract：`export_video` 和 `build_cursor_effect_timeline` 接收 config snapshot / revision，由调用方传入当前 UI config；后端验证 revision 后构建。
6. 增加前端测试：
   - 切换 smoothing 后立即点击导出，断言先调用 `set_beautify_config`，再调用 `export_video`。
   - 切换 magnification 后立即点击返回录制，断言 pending config 已写入后端或明确取消并恢复 UI。
   - debounce 未触发时 unmount，不应静默丢失用户最后一次已确认的控件状态。

建议验收：

- `npm test -- --run src/App.test.tsx` 新增上述 pending flush 测试并通过。
- 手动验证：切换光标平滑后立刻导出，timeline JSON 中应反映最新配置；切换后立刻返回录制，再开始下一段录制，`show_system_cursor` 决策应与最新 UI 配置一致。

#### Important 2: 光标美化全部关闭时仍可能生成可渲染 overlay timeline，给 Phase 6 留下双光标风险

位置：

- `src-tauri/src/lib.rs:285-286`
- `src-tauri/src/lib.rs:405-433`
- `src-tauri/src/media/cursor_engine.rs:118-124`
- `src-tauri/src/media/cursor_engine.rs:403-429`
- `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md:2778-2781`

现象：

计划中对双光标策略的预期是：

- cursor beautify enabled：raw SCK frames 不包含系统光标，录后生成 timeline 给 compositor 叠加。
- cursor beautify disabled：raw SCK frames 保留系统光标，兼容原始录制。

当前 `set_capture_mode()` 的 raw cursor 控制是：

```rust
let show_system_cursor =
    !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);
```

这意味着当 `cursor_magnification=false` 且 `cursor_smoothing=false` 时，raw SCK frames 会保留系统光标，这是合理的。

但 `build_cursor_effect_timeline()` 无论两项是否都关闭，仍会调用 `CursorEffectEngine::build_timeline()`：

```rust
let engine = CursorEffectEngine::with_smoothing(
    ClickAnimationConfig {
        max_scale: if config.cursor_magnification {
            config.magnification_factor.clamp(1.0, 3.0)
        } else {
            1.0
        },
        peak_opacity: if config.cursor_magnification {
            0.35
        } else {
            0.0
        },
    },
    config.cursor_smoothing,
);
```

当 `cursor_smoothing=false` 时，engine 会保留原始 samples：

```rust
let smoothed = if self.smoothing_enabled {
    ...
} else {
    samples.to_vec()
};
```

随后 `BezierInterpolator::sample_frames()` 仍生成 `CursorFrame`，且每帧默认：

```rust
scale: 1.0,
opacity: 1.0,
```

为什么重要：

- 如果 Phase 6 compositor 简单按 `EffectTimeline.frames` 渲染自定义 cursor overlay，那么“美化关闭”的素材会同时包含 raw system cursor 和自定义 overlay cursor。
- 这与计划中 “With cursor beautify disabled before recording, raw SCK frames keep the system cursor for compatibility” 的语义冲突：保留 raw cursor 时，后续不应再叠加一个可见 cursor overlay。
- 目前 checklist 只保留了双光标人工 gate，没有自动化契约证明“美化关闭时 timeline 不会导致 overlay”。
- 这是 Phase 4 与 Phase 6 compositor 交界处的 contract 风险，不一定在当前 mock/export 边界立即可见，但会在真实渲染接入时放大。

建议修复：

1. 当 `cursor_magnification == false && cursor_smoothing == false` 时，`build_cursor_effect_timeline()` 直接生成空/no-op cursor overlay timeline：
   - `frames: []`
   - `click_effects: []`
   - 或在 timeline 中加入 `render_cursor_overlay: false` / `enabled: false` 字段。
2. 如果仍需要输出原始 cursor positions 给后续分析，应明确区分 “metadata timeline” 与 “render effect timeline”，避免 compositor 误用。
3. 将 beautify config snapshot 写入 effect timeline sidecar，Phase 6 compositor 根据 snapshot 判断是否渲染 cursor overlay。
4. 增加 Rust 测试：
   - config `false/false` 时，生成的 render timeline 不含可见 cursor frames/click effects，或带有明确 disabled 标记。
   - config `true/false` 或 `false/true` 时，raw cursor 应隐藏，并生成可渲染 overlay timeline。
5. 增加文档/checklist：
   - 区分 “原始 cursor metadata sidecar 始终可采集” 和 “render effect timeline 是否可见叠加”。

建议验收：

- `cargo test --manifest-path src-tauri/Cargo.toml cursor_engine` 或新增 command 层单元测试通过。
- 手动验证保留到 `npm run tauri dev`：关闭所有 cursor effects 后录制，raw 视频应只有系统光标；开启任一 cursor effect 后录制，raw 视频不含系统光标，后续 overlay timeline 可用。

### 15.4 Minor Findings

#### Minor 1: `compute_normalized_pts()` 仍使用 `u64 as i64`，CMTime hardening 还差最后一层 checked arithmetic

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:132-146`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:531-550`

现象：

Round 5 已新增 `cmtime_to_nanos()`，这解决了 Round 4 中：

- `CMTime.flags` 未检查。
- negative PTS cast 到巨大 `u64`。
- invalid / indefinite PTS 污染 origin。

但核心归一化仍有：

```rust
let delta = pts_nanos as i64 - first_pts as i64;
let result = delta.saturating_add(session_at_first as i64);
result.max(0) as u64
```

如果未来遇到超大 PTS 或 session elapsed 超过 `i64::MAX` 的边界值，`as i64` 会 wrap。真实 ScreenCaptureKit host-time PTS 大概率远低于该范围，当前实际风险较低。

为什么重要：

- PTS normalization 是 Phase 4 cursor/video/audio 对齐的核心路径。
- 该路径的测试目前覆盖了早于 first PTS、无效 PTS、负值 CMTime 等常规边界，但没有覆盖超大值 cast/wrap。
- 这不是当前 merge blocker，但属于 native/media 时间戳路径的健壮性债务。

建议修复：

1. 将 `compute_normalized_pts()` 改为 `i128` 运算：
   - `let delta = pts_nanos as i128 - first_pts as i128;`
   - `let result = delta + session_at_first as i128;`
   - clamp 到 `0..=u64::MAX`。
2. 或使用 checked/sub/saturating 组合，避免隐式 wrap。
3. 增加测试覆盖：
   - `pts_nanos > i64::MAX`
   - `first_pts > i64::MAX`
   - `session_at_first > i64::MAX`
   - delta 为极大正数/负数。

### 15.5 Phase 4 完整性复审结论

已完成或基本完成：

- `core/timeline.rs` 已定义 `CursorSample`、`CursorClick`、`CursorFrame`、`CursorClickEffect`、`EffectTimeline` serde 模型。
- `core/processor.rs` 已定义 `CursorProcessor` 边界。
- `media/cursor_engine.rs` 已实现移动平均平滑、Bezier 插值、点击放大 effect 构建、timeline composition，并包含算法单元测试。
- `app/cursor_metadata_runtime.rs` 已提供录制期 cursor metadata 采集 runtime，samples/clicks 均有上限，使用 `VecDeque::pop_front()` 避免长录制 O(n) 删除。
- `platform/macos/cursor_source.rs` 已通过 CoreGraphics 读取 cursor position 与 button state。
- `MacRecordingService.stop()` 已在 native capture stop 前停止 cursor runtime，sidecar 写入失败不再跳过 cleanup，最终状态机路径较早期版本明显改善。
- `RecordingResult` 已包含 `cursorMetadataPath` 和 `effectTimelinePath`。
- `set_beautify_config`、`get_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- `build_cursor_effect_timeline` 已放入 `spawn_blocking`，并拒绝 Recording / Paused / Processing 状态。
- `session_id + metadata_path + beautify_revision` 已能阻止跨 session 和大部分同 session stale build 写回。
- Preview 已接入后端命令，具备 debounce、unmount cleanup、set-before-build 顺序测试和 backend config 初始化测试。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`。
- React 未接收视频帧、音频帧或 cursor sample stream，符合架构红线。

仍未完全完成或需作为下一轮整改/gate：

- pending beautify config 在导出/返回录制前没有 flush，仍会造成 UI 状态、后端配置、`show_system_cursor` 决策、export timeline 不一致。
- “cursor effects 全关”时缺少 no-op overlay contract，Phase 6 compositor 若直接渲染 `EffectTimeline.frames` 仍可能造成双光标。
- 真实 macOS 录制中 CMSampleBuffer PTS 与 click/video 动作对齐仍需人工 `npm run tauri dev` 验证。
- `cursor_source.rs` CoreGraphics FFI 与 `screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList 仍需 Native Safety Gate 人工逐行审查。
- 双光标策略仍需手动确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor。

### 15.6 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- SCK callback 当前仍只做：
  - PTS 提取和归一化。
  - 必要的 CVPixelBuffer → `Arc<[u8]>` 拷贝。
  - 必要的 AudioBufferList 读取和 PCM 转换。
  - bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内使用 `Mutex<Option<(u64, u64)>>`，但只做轻量时间戳归一化，没有等待 UI、磁盘、timeline build 或 export。
- Cursor polling 由 `CursorMetadataRuntime` 独立线程执行，不在 SCK callback 内调用 `CursorSnapshotSource::snapshot()`。
- Timeline build 是录后 command 路径，并使用 `spawn_blocking`，不阻塞捕获 callback。

内存安全：

- `cursor_source.rs` 中 `CGEventCreate` 返回 event 后做 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；静态复审表面配对正确，但仍需人工 Native Safety Gate。
- `screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取，并复制到 `Arc<[u8]>`；unlock 后不再引用原始 base address。
- Audio `block_buffer` 路径在当前 return 分支均有 release；未发现 Round 5 新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded，长录制内存增长受控。
- Timeline 构建仍一次性读 metadata、生成 frames、pretty serialize JSON，这是录后内存峰值风险，不影响捕获主链路；Phase 6 接入真实素材规模后应复测。

线程安全：

- Service/config/tick/mic runtime 共享状态使用 `Arc<Mutex<...>>`，revision 使用 `AtomicU64`；未发现明显数据竞争。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path + beautify_revision` stale guard 已阻止旧 session / 旧 revision build 写回 service。
- 剩余线程/异步一致性问题集中在前端 pending debounce config 未 flush：这不是 Rust data race，但会造成用户操作顺序与后端配置提交顺序不一致。

资源释放路径：

- `MacRecordingService.stop()` 先停止 cursor runtime，再停止 native capture/mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点。
- Preview unmount cleanup 能取消未触发 timer，但这也造成 pending config 可能被静默丢弃，是 Important 1 的资源/状态收敛问题。

### 15.7 BUG.md 预防规则检查

执行命令：

```bash
rg -n 'data-tauri-drag-region="false"|whileTap' src src-tauri
```

结果解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443`：测试断言不存在 `data-tauri-drag-region="false"`，不是源码新增 wrapper。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171`：`motion.button whileTap`，是交互元素自身使用 `whileTap`，不是 BUG-003 中 `motion.div whileTap` 作为 Button 直接父容器的拦截模式。

结论：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

### 15.8 本轮实际验证

本地执行：

```bash
git diff --check fb6950554ca94950da81da93520893299cc8cf7a
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm test -- --run src/App.test.tsx
npm run build
```

结果：

- `git diff --check fb6950554ca94950da81da93520893299cc8cf7a`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **114 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 21 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, 21 warnings
- `npm test -- --run src/App.test.tsx`: PASS, **30 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- `npm run build`: PASS

说明：

- 本轮最初尝试过一条错误的 targeted cargo test 命令：`cargo test --manifest-path src-tauri/Cargo.toml screen_capture_kit::tests::cmtime screen_capture_kit::tests::pts cursor_metadata_runtime cursor_engine -- --nocapture`，Cargo 只接受一个 TESTNAME，因此该命令因参数格式失败；随后已用完整 `cargo test` 覆盖验证。
- 当前 21 个 Rust warning 仍主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮未发现新的 clippy error。
- jsdom 的 `Window.scrollTo()` 提示仍为测试环境限制，不影响测试通过。

### 15.9 Round 5 Ready To Merge?

**Ready to merge: With fixes / No**

本轮没有发现 Critical，也没有发现会直接阻塞捕获主链路的 Phase 4 新代码。Phase 4 主体功能已经基本完成，Rust/native 资源释放路径也较早期版本明显稳固。但建议在最终合并前完成以下整改：

1. **修复 pending beautify config flush**：导出、返回录制、Preview unmount 前，不能静默丢弃或绕过用户最后一次配置修改；导出必须使用最新 UI 配置。
2. **定义 cursor effects 全关时的 no-op overlay contract**：当 raw SCK frames 保留系统光标时，`EffectTimeline` 不应再被 Phase 6 compositor 误解为“需要渲染一个可见 cursor overlay”。
3. **继续收紧 PTS normalization arithmetic**：`compute_normalized_pts()` 改为 `i128` 或 checked arithmetic，并补超大值测试。
4. **保持人工 gate 未完成状态**：真实 macOS PTS/click 对齐、Native Safety Gate、双光标策略仍需 `npm run tauri dev` 与人工逐行审查。
5. **Phase 6 compositor 接入前补 contract 测试**：验证 compositor 消费 effect timeline 时能区分“渲染 cursor overlay”和“仅保留 raw system cursor”。

## 16. Round 6 整改复审（2026-05-28）

> 评审范围：用户已完成 `## 15. Round 5 整改复审（2026-05-28）` 所列整改任务后的 Phase 4 相关代码。
>
> 本轮额外复审当前工作区未提交整改文件：
>
> - `src-tauri/src/lib.rs`
> - `src-tauri/src/media/cursor_engine.rs`
> - `src-tauri/src/platform/macos/screen_capture_kit.rs`
> - `src/App.test.tsx`
> - `src/components/preview-view.tsx`
> - `src/lib/tauri.ts`
> - `docs/superpowers/reviews/2026-05-27-phase-4-code-review.md`
>
> 当前结论：**With fixes / No**，不建议直接合并。Round 5 的三项明确整改已有明显进展：导出前 pending config flush 已修复，cursor effects 全关时 command 层已生成空/no-op timeline，PTS delta 已改为 `i128` 并补充边界测试。静态复审仍未发现 cursor smoothing / Bezier interpolation / click magnification 进入 ScreenCaptureKit 捕获回调，也未发现新的明显内存安全硬伤。但本轮发现一个更深层的 Phase 4 / Phase 6 contract 问题：raw system cursor 是否进入原始素材是在录制开始前决定的，而 effect timeline 是否生成 overlay 仍读取预览期可变的全局 beautify config，二者没有绑定到同一个 recording session snapshot。除此之外，“返回录制”路径的 pending config flush 仍未等待完成，存在下一次录制使用旧配置的竞态。
>
> 交叉验证：本轮使用 `$superpowers:requesting-code-review` 启动独立审查代理 `Singer`。独立审查结论与本地审查基本一致：无 Critical；`export` flush、no-op timeline、`i128` PTS normalization 已基本修复；仍需修复“返回录制”未 await flush 的竞态。独立审查还复现过一次 `src/App.test.tsx` 的异步断言失败，本地随后完整复跑通过，判断为测试存在 flaky 风险而非稳定功能失败。

### 16.1 本轮已确认修复或改善

- `src/components/preview-view.tsx:69-120`：新增 `pendingConfigRef` 与 `flushPendingConfig()`，导出前会清理 debounce timer 并等待 `setBeautifyConfig()` 完成，修复 Round 5 中“切换配置后立刻导出可能用旧后端配置”的主要路径。
- `src/components/preview-view.tsx:141-146`：`handleExport()` 已改为 `flushPendingConfig().then(() => exportVideo(preset))`，导出调用顺序正确。
- `src/App.test.tsx:1067-1117`：新增 `flushes pending beautify config before export`，验证立即导出时 `set_beautify_config` 先于 `export_video`。
- `src/App.test.tsx:1119-1165`：新增“返回录制”时应 flush pending config 的测试，覆盖了曾经完全丢弃 pending config 的问题；但测试只验证调用发生，没有验证 `onBack()` 等待调用完成，见 Important 2。
- `src-tauri/src/lib.rs:406-412`：当 `cursor_magnification == false && cursor_smoothing == false` 时，`build_cursor_effect_timeline()` 直接生成 `frames: []`、`click_effects: []` 的空 `EffectTimeline`，修复 Round 5 中“美化全关仍可能生成可渲染 overlay”的直接风险。
- `src-tauri/src/media/cursor_engine.rs:710-737`：新增 engine 层“features off 时 frame scale/opacity 中性”的测试，并在注释中明确 command 层负责 no-op timeline short-circuit。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:130-146`：`compute_normalized_pts()` 已改用 `i128` delta 并 clamp 到 `0..=u64::MAX`，修复 Round 5 中 `u64 as i64` wrap 的边界风险。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:1304-1336`：新增 PTS 超大值、超大 first PTS、正向溢出 clamp、极端负 delta clamp 的测试。
- `src-tauri/src/lib.rs:370-380` 与 `src-tauri/src/lib.rs:489-500`：`session_id + metadata_path + beautify_revision` stale guard 仍在，旧 session / 旧 revision build 不会写回 `last_effect_timeline_path`。

### 16.2 Critical Findings

本轮未发现 Critical。

没有发现以下阻塞性问题：

- 未发现 cursor smoothing / Bezier interpolation / click magnification 进入 ScreenCaptureKit callback。
- 未发现新增磁盘 IO、JSON 序列化、timeline build、Tauri emit 等重操作进入捕获回调。
- 未发现新增明显 use-after-free、CVPixelBuffer unlock 后继续读 base address、CoreGraphics event 未释放等硬性内存安全问题。
- 未发现 `CursorMetadataRuntime` samples/clicks 重新退化为无界增长。
- 未发现 Round 4/Round 5 已修复的跨 session stale build 写回重新出现。

### 16.3 Important Findings

#### Important 1: 缺少 recording session 级 beautify config snapshot，raw cursor 和 overlay timeline 仍可能错配

位置：

- `src-tauri/src/lib.rs:281-287`
- `src-tauri/src/lib.rs:392-396`
- `src-tauri/src/lib.rs:406-443`
- `src-tauri/src/media/recording_metadata.rs:12-17`
- `src-tauri/src/media/recording_writer.rs:7-14`
- `src/components/preview-view.tsx:123-138`

现象：

录制开始前，`set_capture_mode()` 会读取当时后端全局 `beautify_config`，并据此决定原始 ScreenCaptureKit 帧是否保留系统光标：

```rust
let beautify_config = state
    .beautify_config
    .lock()
    .map_err(|_| "美化配置锁已损坏".to_string())?
    .clone();
let show_system_cursor =
    !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);
```

而录制完成后，`build_cursor_effect_timeline()` 会再次读取当前全局 `beautify_config` 来决定是否生成空 timeline、是否启用 smoothing、是否生成 click effects：

```rust
let config = state
    .beautify_config
    .lock()
    .map_err(|_| "美化配置锁已损坏".to_string())?
    .clone();

let timeline = if !config.cursor_magnification && !config.cursor_smoothing {
    EffectTimeline {
        fps: metadata.fps,
        duration_nanos: metadata.duration_nanos,
        frames: Vec::new(),
        click_effects: Vec::new(),
    }
} else {
    ...
};
```

这两个决策读取的是同一个全局配置槽，但它们发生在不同时间点：前者在录制开始前，后者在 Preview 中用户可继续调整配置之后。当前 `RecordingMetadata` 只保存 `fps`、`duration_nanos`、`cursor_samples`、`cursor_clicks`，`RecordingResult` 也没有保存本次录制的 beautify config / raw cursor visibility snapshot。

可复现场景 A：可能导出后没有光标

1. 用户开始录制前开启 cursor smoothing 或 cursor magnification。
2. `set_capture_mode()` 将 `show_system_cursor=false`，raw SCK frames 不包含系统光标。
3. 录制完成进入 Preview。
4. 用户关闭 cursor smoothing 和 cursor magnification。
5. `build_cursor_effect_timeline()` 因当前 config 为 `false/false` 生成空 timeline。
6. Phase 6 compositor 若按空 timeline 不渲染 overlay，最终视频可能既没有 raw system cursor，也没有 overlay cursor。

可复现场景 B：仍可能双光标

1. 用户开始录制前关闭 cursor smoothing 和 cursor magnification。
2. `set_capture_mode()` 将 `show_system_cursor=true`，raw SCK frames 已包含系统光标。
3. 录制完成进入 Preview。
4. 用户开启 cursor smoothing 或 cursor magnification。
5. `build_cursor_effect_timeline()` 基于当前 config 生成可渲染 overlay timeline。
6. Phase 6 compositor 叠加 overlay 后，最终视频可能同时包含 raw system cursor 和自定义 cursor overlay。

为什么重要：

- Round 5 的 no-op timeline 修复只解决了“当前 config 为 `false/false` 时不要生成 overlay”，但没有解决“录制时 raw cursor visibility 与导出时 overlay 策略必须同源”的更深层 contract。
- `show_system_cursor` 是原始素材层面的不可逆决策：一旦 raw SCK frames 已经包含或不包含系统光标，后续 Preview 配置不能再假设素材状态会随之变化。
- Phase 4 的目标是为 Phase 6 compositor 提供可解释输入。当前 effect timeline 缺少 session config snapshot，Phase 6 无法判断某个 timeline 应该被渲染，还是应该保留 raw cursor。
- 这是 Phase 4 功能完整性风险，不是单纯 UI 状态问题。它会直接影响真实导出视频是否无光标或双光标。

建议修复：

1. 在录制开始时创建 `BeautifyConfigPayload` 的 session snapshot，并和 `show_system_cursor` 一起绑定到当前 `session_id`。
2. 将该 snapshot 写入 `RecordingMetadata` 或新增 `RecordingSessionMetadata` 字段，例如：

```rust
pub struct RecordingMetadata {
    pub fps: u32,
    pub duration_nanos: u64,
    pub cursor_samples: Vec<CursorSample>,
    pub cursor_clicks: Vec<CursorClick>,
    pub beautify_config: BeautifyConfigSnapshot,
    pub raw_system_cursor_visible: bool,
}
```

3. `build_cursor_effect_timeline()` 应基于 metadata 中的 session snapshot 决定 overlay contract，而不是直接读取 Preview 当前全局配置来决定“是否有 overlay”。
4. 如果产品希望 Preview 中调整 smoothing/magnification 可影响当前录制的导出，需要明确限制：
   - 若 raw system cursor visible，则不能生成可见 cursor overlay，或必须提示该项只对下一次录制生效。
   - 若 raw system cursor hidden，则不能生成空 cursor overlay，除非有明确“导出无光标”的产品语义。
5. 将 config snapshot 写入 effect timeline sidecar，Phase 6 compositor 读取时不需要猜测。
6. 增加 Rust command 层测试：
   - 录制时 effects on / Preview 后 effects off：不得得到“raw cursor hidden + empty overlay”的不可见光标组合。
   - 录制时 effects off / Preview 后 effects on：不得得到“raw cursor visible + visible overlay”的双光标组合。
   - effect timeline sidecar 中包含可解释的 `rawSystemCursorVisible` 或 `renderCursorOverlay` contract。

建议验收：

- `cargo test --manifest-path src-tauri/Cargo.toml cursor` 或新增 app command/service 层测试通过。
- 手动验证：分别覆盖“录制前开、预览后关”和“录制前关、预览后开”两种顺序，确认导出策略不会无光标或双光标。

#### Important 2: “返回录制”路径 flush pending config 未 await，下一次录制仍可能读到旧 config

位置：

- `src/components/preview-view.tsx:112-120`
- `src/components/preview-view.tsx:149-152`
- `src-tauri/src/lib.rs:281-287`
- `src/App.tsx:205-214`
- `src/App.tsx:125-151`
- `src/App.test.tsx:1119-1165`

现象：

`flushPendingConfig()` 自身返回 `Promise<void>`，且会等待 `setBeautifyConfig(config)`：

```ts
const flushPendingConfig = (): Promise<void> => {
  if (debounceRef.current) {
    clearTimeout(debounceRef.current)
    debounceRef.current = null
  }
  const config = pendingConfigRef.current
  if (!config) return Promise.resolve()
  pendingConfigRef.current = null
  return setBeautifyConfig(config).then(() => {})
}
```

导出路径正确等待了它：

```ts
void flushPendingConfig()
  .then(() => exportVideo(preset))
  .catch((error) => {
    console.error('导出失败', error)
  })
```

但返回录制路径没有等待：

```ts
const handleBack = () => {
  void flushPendingConfig()
  onBack()
}
```

`onBack()` 会立即把 app 带回 idle；用户如果马上点击“开始录制”，`handleStartRecording()` 会调用 `setCaptureMode()`，而 `setCaptureMode()` 会读取后端当前 `beautify_config` 决定 `show_system_cursor`。如果 `setBeautifyConfig()` 尚未完成，下一段录制仍可能使用旧配置。

为什么重要：

- Round 5 的“pending config 被清掉并丢失”问题已改善，但“返回录制后立刻开始下一段”仍存在 async ordering race。
- 该 race 影响的不是普通 UI 显示，而是下一段录制的 raw cursor visibility，属于 Phase 4 双光标策略的主链路配置。
- 新增测试只验证 `set_beautify_config` 被调用，没有验证 `onBack()` 在 `set_beautify_config` resolve 后才发生，因此没有覆盖真实风险。

建议修复：

1. 将 `handleBack` 改为 async ordering：

```ts
const handleBack = () => {
  void flushPendingConfig()
    .then(() => onBack())
    .catch((error) => {
      console.error('保存美化配置失败', error)
    })
}
```

2. 在 flush 期间禁用“返回录制”和导出按钮，避免重复点击或返回后立即开始录制。
3. 更稳妥：将 latest beautify config 提升到 `App` 层，在开始录制前显式 `await setBeautifyConfig(latestConfig)`，让 `setCaptureMode()` 永远读取已同步的 config。
4. 修正测试：使用一个手动 resolve 的 `set_beautify_config` Promise，断言 promise resolve 之前 UI 还没有返回 idle / 没有出现“开始录制”，resolve 后才返回。

建议验收：

- `npm test -- --run src/App.test.tsx` 增加 strict ordering 测试并通过。
- 手动验证：在 Preview 切换 cursor effects 后立即返回并立即开始下一段录制，后端 `show_system_cursor` 与最后一次 UI 配置一致。

#### Important 3: backend config 初始化测试存在 flaky 风险

位置：

- `src/App.test.tsx:1038-1065`
- `src/components/preview-view.tsx:75-92`

现象：

独立审查代理 `Singer` 在运行 `npm test -- --run src/App.test.tsx` 时复现过一次失败：

```text
expected smoothing switch unchecked, received checked
```

失败点是 `initializes beautify controls from backend on preview mount` 在 `await screen.findByText('预览与美化')` 之后立即读取 switch `data-state`。但 `getBeautifyConfig()` 是 `useEffect()` 中的异步调用，`findByText('预览与美化')` 只能证明 Preview 已渲染，不能证明 backend config 已 resolve 并完成 React state commit。

本地随后完整复跑同一命令通过：

```bash
npm test -- --run src/App.test.tsx
```

结果为 32 tests PASS。因此该问题目前更像测试异步等待不严谨，而不是稳定功能失败。

为什么重要：

- Phase 4 当前大量依赖前端命令顺序测试保证 config/timeline/export contract，flaky test 会降低整改信号质量。
- 如果该测试偶现失败，会让 CI / 本地复审结论不稳定。

建议修复：

1. 将断言包进 `waitFor`：

```ts
await vi.waitFor(() => {
  expect(switches[1].getAttribute('data-state')).toBe('unchecked')
})
```

2. 更稳妥：在测试中等待 `get_beautify_config` 被调用并 resolve 后，再查询 switch；避免复用 resolve 前的 DOM 状态。
3. 可考虑在 UI 层增加 config 初始化中的禁用态，但这属于产品体验选择，不是本轮必须项。

建议验收：

- 连续多次运行 `npm test -- --run src/App.test.tsx` 不再偶发失败。

### 16.4 Minor Findings

#### Minor 1: `cmtime_to_nanos()` 仍使用 `f64` 转换，极端 CMTime 可能有精度损失

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:550`

现象：

Round 6 已确认 `compute_normalized_pts()` 的 `u64 as i64` 问题已修复，核心 delta 运算现在使用 `i128`。不过 `cmtime_to_nanos()` 仍通过 `f64` 做秒数转换：

```rust
let seconds = pts.value as f64 / pts.timescale as f64;
return Some((seconds * 1_000_000_000.0) as u64);
```

真实 ScreenCaptureKit host-time PTS 通常不会触达这个极端边界，因此这不是当前 merge blocker。但在超大 `CMTime.value` 或特殊 timescale 下，`f64` 可能产生精度损失，甚至在浮点结果超出整数范围时依赖 Rust float-to-int saturating cast 语义。

建议修复：

1. 用整数运算替代浮点转换：

```rust
let value = pts.value as u128;
let scale = pts.timescale as u128;
let nanos = value.checked_mul(1_000_000_000)?.checked_div(scale)?;
u64::try_from(nanos).ok()
```

2. 增加测试覆盖：
   - 大 `CMTime.value` 且不会溢出 `u64` 的精确转换。
   - 大 `CMTime.value` 乘以 1e9 后超过 `u64::MAX` 时返回 `None` 或 clamp，行为必须明确。

#### Minor 2: `tests/phase-4-w7-w8-checklist.md` 验证摘要仍停留在 Round 3

位置：

- `tests/phase-4-w7-w8-checklist.md:66-73`

现象：

Checklist 仍写着：

- Round 3
- Rust 108 tests
- Frontend 29 tests

本轮实际本地验证为：

- Rust 119 tests PASS
- Frontend `src/App.test.tsx` 32 tests PASS

这不影响代码运行，但会让下一轮整改和人工验收误读当前覆盖状态。

建议修复：

- 在完成本轮整改后同步更新 `tests/phase-4-w7-w8-checklist.md` 的 Verification Summary。
- 保留真实 macOS PTS/click 对齐、Native Safety Gate、双光标人工检查为未完成状态，不要因为自动化通过而误标完成。

### 16.5 Phase 4 完整性复审结论

已完成或基本完成：

- `core/timeline.rs` 已定义 cursor sample/click/frame/effect timeline serde model。
- `core/processor.rs` 已提供 `CursorProcessor` trait 边界。
- `media/cursor_engine.rs` 已实现移动平均平滑、Bezier 插值、点击放大 effect 构建和 timeline composition。
- `app/cursor_metadata_runtime.rs` 已提供录制期 cursor metadata runtime；samples/clicks 均有上限，使用 `VecDeque::pop_front()`，长录制内存增长受控。
- `platform/macos/cursor_source.rs` 已通过 CoreGraphics 读取 cursor position 和 button state。
- `MacRecordingService.stop()` 已在 native capture stop 前停止 cursor runtime，cursor sidecar 写入失败不再跳过 cleanup。
- `RecordingResult` 已包含 `cursorMetadataPath` / `effectTimelinePath`。
- `set_beautify_config`、`get_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- `build_cursor_effect_timeline` 已放入 `spawn_blocking`，并拒绝 Recording / Paused / Processing 状态。
- `session_id + metadata_path + beautify_revision` stale guard 已能阻止跨 session 和旧 revision build 写回。
- Preview 已接入后端命令，导出前 pending config flush 已修复。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`。
- `cursor_magnification=false && cursor_smoothing=false` 时 command 层已生成空/no-op timeline。
- PTS invalid/negative/special flag 检查已加强，PTS delta 已改为 `i128`。
- React 未接收视频帧、音频帧或 cursor sample stream，符合架构红线。

仍未完全完成或需作为下一轮整改/gate：

- raw cursor visibility 与 overlay timeline 缺少 recording session snapshot contract，仍可能无光标或双光标。
- “返回录制”路径未 await pending config flush，下一次录制仍可能按旧后端配置决定 `show_system_cursor`。
- backend config 初始化测试有 flaky 风险，需要收紧异步等待。
- 真实 macOS 录制中 CMSampleBuffer PTS 与 click/video 动作对齐仍需 `npm run tauri dev` 人工验证。
- `cursor_source.rs` CoreGraphics FFI 与 `screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList 仍需 Native Safety Gate 人工逐行审查。
- 双光标策略仍需真实录制素材人工确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor 接入。

结论：Phase 4 主体功能已经基本完成，但“session snapshot / raw cursor / overlay timeline”契约仍是 Phase 4 与 Phase 6 之间的关键缺口。因此本轮不建议直接合并。

### 16.6 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- SCK callback 当前仍只做：
  - PTS 提取与归一化。
  - CVPixelBuffer 数据复制到 `Arc<[u8]>`。
  - AudioBufferList 读取与 PCM 转换。
  - bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内使用 `Mutex<Option<(u64, u64)>>`，只做轻量时间戳映射；未等待 UI、磁盘、timeline build 或 export。
- Cursor polling 由 `CursorMetadataRuntime` 独立线程执行，不在 SCK callback 内调用 `CursorSnapshotSource::snapshot()`。
- Timeline build 是录后 command 路径，并使用 `spawn_blocking`，不会阻塞捕获 callback。
- `MacScreenCapture::start_stream()` / `stop()` 中等待 SCK start/stop completion 的同步等待发生在 Tauri `spawn_blocking` 的 recording service 调用路径中，不在 SCK callback 内。

内存安全：

- `cursor_source.rs` 中 `CGEventCreate` 返回 event 后做 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；静态复审表面配对正确，仍需人工 Native Safety Gate。
- `screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取，并复制到 `Arc<[u8]>`；unlock 后不再引用原始 base address。
- Audio `block_buffer` 当前 return 分支均有 `cf_release`；未发现 Round 6 新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded。
- Timeline 构建仍一次性读取 metadata、生成 frames、pretty serialize JSON；这是录后内存峰值风险，不影响捕获主链路。Phase 6 接入真实长素材后仍需压力测试。

线程安全：

- Service/config/tick/mic runtime 共享状态使用 `Arc<Mutex<...>>`，beautify revision 使用 `AtomicU64`；未发现 Rust 数据竞争。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path + beautify_revision` stale guard 已阻止旧 session / 旧 revision build 写回 service。
- 剩余线程/异步一致性问题集中在前端：`handleBack()` 不 await `flushPendingConfig()`，可能造成用户操作顺序与后端配置提交顺序不一致。

资源释放路径：

- `MacRecordingService.stop()` 先停止 cursor runtime，再停止 native capture/mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点。
- Preview unmount cleanup 会清理 debounce timer，并尝试 `setBeautifyConfig(config)`；但这仍是 fire-and-forget，不应作为保证下一次录制配置一致的唯一路径。

### 16.7 BUG.md 预防规则检查

执行命令：

```bash
rg -n 'data-tauri-drag-region="false"|whileTap' src src-tauri
```

结果：

```text
src/components/recording-panel.tsx:89:              whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:155:            whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:171:            whileTap={{ scale: 0.98 }}
src/App.test.tsx:431:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/App.test.tsx:443:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
```

结果解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443` 是测试断言不存在 `data-tauri-drag-region="false"`，不是源码新增 wrapper。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171` 是 `motion.button whileTap`，`whileTap` 位于交互元素自身，不是 BUG-003 中 `motion.div whileTap` 作为 Button 直接父容器的拦截模式。

结论：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

### 16.8 本轮实际验证

本地执行：

```bash
git diff --check fb6950554ca94950da81da93520893299cc8cf7a
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm test -- --run src/App.test.tsx
npm run build
rg -n 'data-tauri-drag-region="false"|whileTap' src src-tauri
```

结果：

- `git diff --check fb6950554ca94950da81da93520893299cc8cf7a`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **119 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 21 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, 21 warnings
- `npm test -- --run src/App.test.tsx`: PASS, **32 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- `npm run build`: PASS
- BUG.md 规则扫描：未发现新增违规 pattern

补充说明：

- 独立审查代理 `Singer` 曾复现 `npm test -- --run src/App.test.tsx` 失败，结果为 31 passed / 1 failed，失败点是 `initializes beautify controls from backend on preview mount` 未等待 async backend config state commit。
- 本地随后完整复跑同一命令通过，结果为 32 passed。因此本轮将其归类为 **Important flaky test risk**，不是当前稳定失败。
- 当前 21 个 Rust warning 仍主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮未发现新的 clippy error。

### 16.9 Round 6 Ready To Merge?

**Ready to merge: With fixes / No**

本轮没有发现 Critical，也没有发现会直接阻塞捕获主链路的 Phase 4 新代码。Round 5 明确列出的三个整改点大多已完成：

1. **导出前 pending config flush**：已修复。
2. **cursor effects 全关时 no-op overlay timeline**：command 层已修复。
3. **PTS normalization arithmetic**：已改为 `i128`，并补充边界测试。

但建议在最终合并前继续完成以下整改：

1. **建立 recording session 级 beautify config / raw cursor snapshot contract**：录制时的 `show_system_cursor` 决策和导出时的 overlay timeline 决策必须来自同一个 session snapshot，避免无光标或双光标。
2. **修复返回录制的 flush ordering**：`handleBack()` 必须等待 `flushPendingConfig()` 完成后再 `onBack()`，或在开始下一段录制前强制同步 latest beautify config。
3. **修复 flaky frontend test**：`initializes beautify controls from backend on preview mount` 需要等待 `getBeautifyConfig()` 的异步状态提交后再断言。
4. **可选收紧 CMTime integer conversion**：将 `cmtime_to_nanos()` 的 `f64` 转换改为 checked integer arithmetic。
5. **同步 Phase 4 checklist 验证摘要**：更新 Rust/frontend test count，同时保留真实 macOS PTS 对齐、Native Safety Gate、双光标人工检查为未完成。
6. **继续保留人工 gate**：真实 macOS 录制中的 click/video 对齐、CoreGraphics/SCK FFI Native Safety、双光标策略仍需 `npm run tauri dev` 与人工逐行审查。

## 17. Round 7 整改复审（2026-05-28）

> 评审范围：用户已完成 `## 16. Round 6 整改复审（2026-05-28）` 所列整改任务后的 Phase 4 相关代码。
>
> 本轮复审当前工作区未提交整改文件，重点关注：
>
> - `src-tauri/src/lib.rs`
> - `src-tauri/src/core/timeline.rs`
> - `src-tauri/src/media/recording_metadata.rs`
> - `src-tauri/src/app/cursor_metadata_runtime.rs`
> - `src-tauri/src/platform/macos/screen_capture_kit.rs`
> - `src-tauri/src/platform/macos_service.rs`
> - `src/components/preview-view.tsx`
> - `src/App.test.tsx`
> - `src/lib/tauri.ts`
> - `tests/phase-4-w7-w8-checklist.md`
>
> 当前结论：**With fixes / No**，不建议直接合并。Round 6 的明确整改项已有明显进展：录制 session 级 `BeautifyConfigSnapshot` 已写入 cursor metadata；`build_cursor_effect_timeline()` 已改为从 metadata snapshot 构建；“返回录制”正常 pending 路径已等待 `flushPendingConfig()`；`cmtime_to_nanos()` 已改为 checked integer arithmetic；frontend flaky 测试等待已收紧；Phase 4 checklist 验证摘要已更新。静态复审仍未发现 cursor smoothing / Bezier interpolation / click magnification 进入 ScreenCaptureKit 捕获回调，也未发现新的明显内存安全或线程释放硬伤。但本轮仍发现两个 Important：Preview debounce timer 触发后仍存在 in-flight config write 未被 flush 等待的竞态；同时当前“录制开始 snapshot”方案让 Preview 美化控件不再真实影响当前录制的 timeline/export，Phase 4 的 Preview/export 语义需要重新收敛。
>
> 交叉验证：本轮继续使用 `$superpowers:requesting-code-review` 启动独立审查代理 `Ramanujan`。独立审查结论与本地审查一致：无 Critical；Round 6 修复真实存在；核心 Important 是 `pendingConfigRef` 在 debounce 触发时过早清空导致 in-flight write 不被 `flushPendingConfig()` 等待，以及 `build_cursor_effect_timeline()` 只读取录制开始 snapshot 后 Preview 控件无法影响当前 timeline/export。

### 17.1 本轮已确认修复或改善

- `src-tauri/src/core/timeline.rs:75-87`：新增 `BeautifyConfigSnapshot`，包含 `cursor_magnification`、`magnification_factor`、`cursor_smoothing`、`auto_trim_silences`、`trim_sensitivity`、`raw_system_cursor_visible`。
- `src-tauri/src/media/recording_metadata.rs:12-18`：`RecordingMetadata` 已持久化 `beautify_config: BeautifyConfigSnapshot`，使录制 session 的美化配置与原始光标可见性可以随 metadata sidecar 保存。
- `src-tauri/src/lib.rs:138-155`：`start_recording()` 已在录制开始时创建 `BeautifyConfigSnapshot`，并传入 `MacRecordingService::start()`。
- `src-tauri/src/platform/macos_service.rs:104-169`：`MacRecordingService::start()` 已接收 `beautify_snapshot` 并传给 `CursorMetadataRuntime::spawn()`。
- `src-tauri/src/app/cursor_metadata_runtime.rs:132-139`：`CursorMetadataRecorder::finish()` 已把录制开始时的 `beautify_snapshot` 写入 `RecordingMetadata`。
- `src-tauri/src/lib.rs:414-454`：`build_cursor_effect_timeline()` 已改为从 `metadata.beautify_config` 构建 timeline，不再直接用 Preview 期可变的全局 `beautify_config` 决定 smoothing/click 配置。
- `src/components/preview-view.tsx:149-155`：`handleBack()` 正常路径已改为 `flushPendingConfig().then(() => onBack())`，修复 Round 6 中“返回录制”完全不等待 flush 的主路径。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:535-555`：`cmtime_to_nanos()` 已改为 `u128 checked_mul + checked_div + u64::try_from`，不再使用 `f64` 转换。
- `src/App.test.tsx:1038-1072`：`initializes beautify controls from backend on preview mount` 已增加 `vi.waitFor()`，收紧异步状态提交等待。
- `tests/phase-4-w7-w8-checklist.md:66-73`：Verification Summary 已更新为 Round 6，Rust 120 tests / frontend 32 tests。
- `src-tauri/src/platform/macos/screen_capture_kit.rs:1304-1338`：`compute_normalized_pts()` 的 `i128` 边界测试仍在，超大 PTS / 正向溢出 / 极端负 delta 均覆盖。

### 17.2 Critical Findings

本轮未发现 Critical。

没有发现以下阻塞性问题：

- 未发现 cursor smoothing / Bezier interpolation / click magnification 进入 ScreenCaptureKit callback。
- 未发现新增磁盘 IO、JSON 序列化、timeline build、Tauri emit、export 等重操作进入捕获回调。
- 未发现新增明显 use-after-free、CVPixelBuffer unlock 后继续读 base address、CoreGraphics event 未释放等硬性内存安全问题。
- 未发现 `CursorMetadataRuntime` samples/clicks 重新退化为无界增长。
- 未发现 Round 4/Round 5 已修复的跨 session stale build 写回重新出现。

### 17.3 Important Findings

#### Important 1: debounce 已触发但 config write 仍在飞行时，export/back 仍可越过 flush

位置：

- `src/components/preview-view.tsx:112-120`
- `src/components/preview-view.tsx:123-138`
- `src/components/preview-view.tsx:141-155`
- `src-tauri/src/lib.rs:295-309`
- `src-tauri/src/lib.rs:138-155`
- `src/App.test.tsx:1074-1124`
- `src/App.test.tsx:1126-1172`

现象：

`flushPendingConfig()` 目前只检查 `pendingConfigRef.current`：

```ts
const flushPendingConfig = (): Promise<void> => {
  if (debounceRef.current) {
    clearTimeout(debounceRef.current)
    debounceRef.current = null
  }
  const config = pendingConfigRef.current
  if (!config) return Promise.resolve()
  pendingConfigRef.current = null
  return setBeautifyConfig(config).then(() => {})
}
```

当用户切换 Preview 美化开关后，如果 300ms debounce timer 尚未触发，`flushPendingConfig()` 可以清 timer 并等待 `setBeautifyConfig(config)`，这个路径已经修复。

但 timer 一旦触发，`handleBeautifyChange()` 会先把 `pendingConfigRef.current` 清空，再发起异步 `setBeautifyConfig(nextConfig)`：

```ts
debounceRef.current = setTimeout(() => {
  debounceRef.current = null
  pendingConfigRef.current = null
  void setBeautifyConfig(nextConfig)
    .then(() => buildCursorEffectTimeline())
    .catch((error) => {
      console.error('光标效果处理失败', error)
    })
}, 300)
```

此时如果用户马上点击“导出”或“返回录制”，`handleExport()` / `handleBack()` 虽然都会调用 `flushPendingConfig()`，但 `pendingConfigRef.current` 已经是 `null`，所以 flush 会立即 resolve，无法等待已经发起但尚未完成的 `setBeautifyConfig(nextConfig)`。

为什么重要：

- Round 6 修复了“timer 未触发前”pending config 被丢弃的问题，但没有覆盖“timer 已触发、后端写入仍 pending”的竞态窗口。
- 如果用户在这个窗口点击“返回录制”，`onBack()` 会先执行，下一次 `handleStartRecording()` 会调用 `setCaptureMode()`；而 `set_capture_mode()` 会读取后端当前 `beautify_config` 决定 `CaptureConfig.show_system_cursor`。
- 如果正在飞行的 `set_beautify_config` 还没完成，下一段录制仍可能按旧配置决定 raw system cursor visibility，进而重新引入无光标/双光标策略漂移。
- 这个问题不在 ScreenCaptureKit callback 热路径内，但会影响捕获开始前的关键配置，属于 Phase 4 主链路配置一致性风险。
- 当前新增测试只覆盖了“点击导出/返回时 debounce 尚未触发”的路径；没有覆盖“debounce 已触发但 promise 未 resolve”的严格顺序。

建议修复：

1. 增加一个 in-flight promise ref，例如：

```ts
const configWriteRef = useRef<Promise<void> | null>(null)
```

2. 将所有 `setBeautifyConfig()` 写入包装为同一个 helper：

```ts
const writeBeautifyConfig = (config: BeautifyConfig): Promise<void> => {
  const promise = setBeautifyConfig(config).then(() => {})
  configWriteRef.current = promise
  return promise.finally(() => {
    if (configWriteRef.current === promise) {
      configWriteRef.current = null
    }
  })
}
```

3. `flushPendingConfig()` 同时处理两个状态：
   - 有 `pendingConfigRef.current`：取消 timer，立即写入并等待。
   - 没有 pending config 但有 `configWriteRef.current`：等待当前正在飞行的写入完成。

4. 对 `handleExport()` 和 `handleBack()` 保持 `await flushPendingConfig()` 后再继续。
5. 更稳妥的后端兜底：`start_recording()` 在真正开始 native capture 前重新根据当前 `beautify_config` 计算 `show_system_cursor`，或校验 `capture_config.show_system_cursor == !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing)`，避免命令级调用顺序绕过前端约束。
6. 增加 frontend strict ordering 测试：
   - 切换开关。
   - `vi.advanceTimersByTime(350)` 让 debounce 触发。
   - 让 `set_beautify_config` 返回一个手动 resolve 的 Promise。
   - 点击“导出”：在 resolve 前断言 `export_video` 未调用；resolve 后才调用。
   - 点击“返回录制”：在 resolve 前断言 UI 仍未返回 idle / `onBack` 未执行；resolve 后才返回。

建议验收：

- `npm test -- --run src/App.test.tsx` 增加上述 in-flight strict ordering 测试并通过。
- 手动验证：Preview 中切换光标美化开关，等待 300ms 后立即点击返回并立即开始下一段录制，后端最终使用的 `show_system_cursor` 必须与最后一次 UI 配置一致。

#### Important 2: Preview 控件不再真实影响当前录制的 timeline/export，Phase 4 交互语义需要重新收敛

位置：

- `src-tauri/src/lib.rs:138-155`
- `src-tauri/src/lib.rs:414-454`
- `src-tauri/src/core/timeline.rs:65-73`
- `src-tauri/src/core/timeline.rs:75-87`
- `src/components/preview-view.tsx:47-52`
- `src/components/preview-view.tsx:123-138`
- `src/components/preview-view.tsx:141-146`
- `src/App.test.tsx:820-850`
- `src/App.test.tsx:889-925`
- `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md:2872`

现象：

Round 6 为了解决 raw cursor 和 overlay timeline 错配，已将 `build_cursor_effect_timeline()` 改为读取录制 metadata 中的 session snapshot：

```rust
let snapshot = &metadata.beautify_config;

let timeline = if !snapshot.cursor_magnification && !snapshot.cursor_smoothing {
    EffectTimeline {
        fps: metadata.fps,
        duration_nanos: metadata.duration_nanos,
        frames: Vec::new(),
        click_effects: Vec::new(),
    }
} else {
    let engine = CursorEffectEngine::with_smoothing(
        ClickAnimationConfig { ... },
        snapshot.cursor_smoothing,
    );
    ...
}
```

这个方向解决了上一轮的深层风险：raw system cursor 是否进入原始素材，是录制开始前的不可逆决策，不能让 Preview 期可变配置随意改变 overlay 策略。

但当前 Preview UI 仍然表现为“修改当前录制的光标美化配置”：

- Preview 中仍有 `cursorMagnification`、`magnificationFactor`、`cursorSmoothing` 等控件。
- 每次开关变更后仍调用 `setBeautifyConfig(nextConfig).then(() => buildCursorEffectTimeline())`。
- 导出前仍 `flushPendingConfig().then(() => exportVideo(preset))`。
- Phase 4 计划仍写着 “Toggling 光标平滑 / 光标放大 in preview calls set_beautify_config and builds a cursor effect timeline”。

现在的问题是：Preview 写入的全局 `beautify_config` 不再参与当前 recording 的 `build_cursor_effect_timeline()`，因为 timeline 只使用录制开始时的 metadata snapshot。用户在 Preview 中关闭/开启 smoothing 或 magnification，前端状态会变化，也会触发后端 build，但后端 build 输出仍按录制开始时的 snapshot 生成。

可复现场景：

1. 用户录制开始前开启 cursor smoothing / cursor magnification。
2. `start_recording()` 写入 metadata snapshot：`cursor_smoothing=true`、`cursor_magnification=true`、`raw_system_cursor_visible=false`。
3. 录制完成进入 Preview。
4. 用户在 Preview 中关闭 cursor smoothing 或 cursor magnification。
5. 前端调用 `setBeautifyConfig(false/...)` 并触发 `buildCursorEffectTimeline()`。
6. 后端读取 metadata snapshot，仍按录制开始时的 `true/true` 生成 timeline。
7. 用户以为当前导出已关闭效果，但实际当前 timeline/export 不会跟随 Preview 控件。

反向也成立：

1. 用户录制开始前关闭 cursor effects，raw system cursor 被录入素材。
2. metadata snapshot 为 `false/false`、`raw_system_cursor_visible=true`。
3. Preview 中开启 effects 后，后端仍根据 metadata snapshot 生成空 timeline。
4. 用户以为开启了效果，但当前导出不会产生 overlay。

为什么重要：

- 这是功能完整性 / 产品语义问题，不是捕获回调性能问题。
- Round 6 的 snapshot 方案避免了无光标/双光标，但目前是通过“冻结所有导出意图”实现的，导致 Preview 控件对当前素材的效果失真。
- 这会削弱 Phase 4 的 Preview/export integration：UI、测试和计划都还在表达“Preview 调整会生成当前效果时间线”，但 Rust 实现已经变成“录制开始时的配置决定当前效果时间线”。
- `beautify_revision` stale guard 也因此语义变得混乱：当前 timeline build 输入不再依赖 live beautify config，但 live config revision 变化仍会取消 build，可能把与当前 build 输入无关的 Preview 配置变化当作 stale。

建议修复方向：

1. 明确产品语义，二选一，不要继续保持 UI 和后端语义分裂：
   - **方案 A：Preview 控件只影响下一次录制。** 那就需要在 UI 上禁用或标注当前素材不可变，并停止在 Preview 变更后触发当前 recording 的 `buildCursorEffectTimeline()`。
   - **方案 B：Preview 控件影响当前导出。** 那就需要把 metadata 中的不可变 raw cursor fact 与 Preview 的可变 export intent 分开。

2. 如果选择方案 B，建议数据模型拆分：
   - `RecordingMetadata` 保存不可变事实：
     - `raw_system_cursor_visible`
     - `recording_started_with_cursor_overlay_intent` 或类似字段
     - cursor samples / clicks / fps / duration
   - `build_cursor_effect_timeline()` 读取当前 Preview export config 作为可变导出意图。
   - 构建前根据 raw cursor fact 应用安全策略：
     - 如果 `raw_system_cursor_visible == true`，禁止生成可见 cursor overlay，或明确提示“本次素材已录入系统光标，光标平滑/放大仅对下一次录制生效”。
     - 如果 `raw_system_cursor_visible == false`，即使用户关闭 smoothing/magnification，也必须明确是否允许“导出无光标”；若不允许，则至少生成 baseline cursor overlay（scale=1.0、opacity=1.0、无点击放大）以避免无光标。
   - `EffectTimeline` 或旁路 export contract 中应包含明确字段，例如 `renderCursorOverlay`、`rawSystemCursorVisible`、`exportBeautifyConfig`，方便 Phase 6 compositor 不靠空 frames 猜语义。

3. 如果继续使用 metadata snapshot 作为当前 timeline 的唯一输入，需要同步修改前端/计划/测试：
   - Preview 中光标开关不应再表现为当前录制的导出设置。
   - `src/App.test.tsx:820-850`、`src/App.test.tsx:889-925` 等“切换后 build 当前 timeline”的测试语义需要重写。
   - `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md:2872` 需要补充“Preview 调整是否影响当前 recording”的明确约束。

建议验收：

- 增加 Rust command 层测试：
  - raw cursor visible + Preview 开启 overlay：不得产生双光标风险。
  - raw cursor hidden + Preview 关闭 overlay：不得无意生成无光标导出，除非产品明确允许。
  - timeline/export sidecar 能表达 `rawSystemCursorVisible` / `renderCursorOverlay` 语义。
- 增加 frontend 测试：
  - 若 Preview 控件只影响下次录制，UI 不应触发当前 timeline build。
  - 若 Preview 控件影响当前导出，导出前的 config 应真实进入后端 build contract。

### 17.4 Minor Findings

本轮未发现新的 Minor blocker。

说明：

- Round 6 的 `cmtime_to_nanos()` `f64` 精度风险已修复，不再作为本轮 Minor。
- `tests/phase-4-w7-w8-checklist.md` 的验证摘要已更新，不再停留在 Round 3。
- 现有 21 个 Rust warning 仍主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮没有新增 clippy error。

### 17.5 Phase 4 完整性复审结论

已完成或基本完成：

- `core/timeline.rs` 已定义 cursor sample/click/frame/effect timeline serde model。
- `core/processor.rs` 已提供 `CursorProcessor` trait 边界。
- `media/cursor_engine.rs` 已实现移动平均平滑、Bezier 插值、点击放大 effect 构建和 timeline composition。
- `app/cursor_metadata_runtime.rs` 已提供录制期 cursor metadata runtime；samples/clicks 均有上限，使用 `VecDeque::pop_front()`，长录制内存增长受控。
- `platform/macos/cursor_source.rs` 已通过 CoreGraphics 读取 cursor position 和 button state。
- `MacRecordingService.stop()` 已在 native capture stop 前停止 cursor runtime，cursor sidecar 写入失败不再跳过 cleanup。
- `RecordingResult` 已包含 `cursorMetadataPath` / `effectTimelinePath`。
- `set_beautify_config`、`get_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- `build_cursor_effect_timeline` 已放入 `spawn_blocking`，并拒绝 Recording / Paused / Processing 状态。
- `session_id + metadata_path + beautify_revision` stale guard 仍能阻止跨 session 和旧 revision build 写回。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`。
- cursor effects 全关时，基于 metadata snapshot 的 command 层会生成空/no-op timeline。
- PTS invalid/negative/special flag 检查已加强，PTS delta 已使用 `i128`，CMTime conversion 已使用 checked integer arithmetic。
- React 未接收视频帧、音频帧或 cursor sample stream，符合架构红线。

仍未完全完成或需作为下一轮整改/gate：

- Preview debounce 的 in-flight config write 仍未被 `flushPendingConfig()` 等待，返回/导出路径仍存在配置顺序竞态。
- Preview 控件与当前 recording timeline/export 的语义不一致：后端以录制开始 snapshot 为准，UI 仍表现为 Preview 调整当前导出效果。
- `EffectTimeline` sidecar 仍未显式包含 `rawSystemCursorVisible` / `renderCursorOverlay` 等 Phase 6 compositor 可直接消费的渲染契约字段。
- 真实 macOS 录制中 CMSampleBuffer PTS 与 click/video 动作对齐仍需 `npm run tauri dev` 人工验证。
- `cursor_source.rs` CoreGraphics FFI 与 `screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList 仍需 Native Safety Gate 人工逐行审查。
- 双光标策略仍需真实录制素材人工确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor 接入。

结论：Phase 4 主体功能已经基本完成，但 Preview/export 配置契约仍未闭环。因此本轮仍不建议直接合并。

### 17.6 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- SCK callback 当前仍只做：
  - CMSampleBuffer PTS 提取与归一化。
  - CVPixelBuffer 数据复制到 `Arc<[u8]>`。
  - AudioBufferList 读取与 PCM 转换。
  - bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内使用 `Mutex<Option<(u64, u64)>>`，只做轻量时间戳映射；未等待 UI、磁盘、timeline build 或 export。
- Cursor polling 由 `CursorMetadataRuntime` 独立线程执行，不在 SCK callback 内调用 `CursorSnapshotSource::snapshot()`。
- Timeline build 是录后 command 路径，并使用 `spawn_blocking`，不会阻塞捕获 callback。
- `MacScreenCapture::start_stream()` / `stop()` 中等待 SCK start/stop completion 的同步等待发生在 Tauri `spawn_blocking` 的 recording service 调用路径中，不在 SCK callback 内。

内存安全：

- `cursor_source.rs` 中 `CGEventCreate` 返回 event 后做 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；静态复审表面配对正确，仍需人工 Native Safety Gate。
- `screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取，并复制到 `Arc<[u8]>`；unlock 后不再引用原始 base address。
- Audio `block_buffer` 当前 return 分支均有 `cf_release`；未发现 Round 7 新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded。
- Timeline 构建仍一次性读取 metadata、生成 frames、pretty serialize JSON；这是录后内存峰值风险，不影响捕获主链路。Phase 6 接入真实长素材后仍需压力测试。

线程安全：

- Service/config/tick/mic runtime 共享状态使用 `Arc<Mutex<...>>`，beautify revision 使用 `AtomicU64`；未发现 Rust 数据竞争。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path + beautify_revision` stale guard 已阻止旧 session / 旧 revision build 写回 service。
- 剩余线程/异步一致性问题集中在前端：debounce timer 触发后正在飞行的 `setBeautifyConfig()` 没有被 `flushPendingConfig()` 跟踪和等待。

资源释放路径：

- `MacRecordingService.stop()` 先停止 cursor runtime，再停止 native capture/mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点。
- Preview unmount cleanup 会清理 debounce timer，并尝试 `setBeautifyConfig(config)`；但它仍是 fire-and-forget，不能作为保证下一次录制配置一致的唯一机制。下一轮应通过 in-flight promise tracking 或 App 层 latest config 同步来闭环。

### 17.7 BUG.md 预防规则检查

执行命令：

```bash
rg -n 'data-tauri-drag-region="false"|whileTap|motion\.div' src src-tauri
```

结果要点：

```text
src/components/recording-panel.tsx:89:              whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:155:            whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:171:            whileTap={{ scale: 0.98 }}
src/App.test.tsx:431:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/App.test.tsx:443:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
```

结果解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443` 是测试断言不存在 `data-tauri-drag-region="false"`，不是源码新增 wrapper。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171` 是 `motion.button whileTap`，`whileTap` 位于交互元素自身，不是 BUG-003 中 `motion.div whileTap` 作为 Button 直接父容器的拦截模式。
- `motion.div` 仍用于页面/装饰动画，没有发现作为 Button 直接父容器并带 `whileTap` 的违规结构。

结论：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

### 17.8 本轮实际验证

本地主审执行：

```bash
git diff --check fb6950554ca94950da81da93520893299cc8cf7a
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm test -- --run src/App.test.tsx
npm run build
rg -n 'data-tauri-drag-region="false"|whileTap|motion\.div' src src-tauri
```

结果：

- `git diff --check fb6950554ca94950da81da93520893299cc8cf7a`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **120 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 21 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, 21 warnings
- `npm test -- --run src/App.test.tsx`: PASS, **32 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- `npm run build`: PASS
- BUG.md 规则扫描：未发现新增违规 pattern

独立审查代理 `Ramanujan` 额外报告：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **120 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 21 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS
- `npm test -- --run`: PASS, **32 tests**
- `npm run build`: PASS

补充说明：

- 当前 21 个 Rust warning 仍主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮未发现新的 clippy error。
- 本轮没有运行 `npm run tauri dev`，真实 macOS 录制、click/video 对齐、双光标视觉检查、Native Safety Gate 仍需人工验证。

### 17.9 Round 7 Ready To Merge?

**Ready to merge: With fixes / No**

本轮没有发现 Critical，也没有发现会直接阻塞捕获主链路的 Phase 4 新代码。Round 6 明确列出的整改项大多已完成：

1. **recording session 级 beautify config snapshot**：已写入 metadata，并被 timeline build 使用。
2. **返回录制 pending flush 主路径**：`handleBack()` 已等待 `flushPendingConfig()`。
3. **flaky frontend test**：backend config 初始化断言已使用 `vi.waitFor()`。
4. **CMTime integer conversion**：已改为 checked integer arithmetic。
5. **Phase 4 checklist 验证摘要**：已更新到 Rust 120 / frontend 32。

但建议在最终合并前继续完成以下整改：

1. **修复 Preview debounce in-flight config write race**：`flushPendingConfig()` 必须等待已经触发但尚未 resolve 的 `setBeautifyConfig()`。
2. **收敛 Preview 控件与当前 timeline/export 的产品/技术契约**：明确 Preview 修改是影响当前导出还是仅影响下一次录制，并同步 UI、Rust command、tests、plan/checklist。
3. **为 Phase 6 compositor 明确渲染契约**：不要只靠 empty/non-empty `frames` 推断语义，建议在 timeline/export sidecar 中包含 `rawSystemCursorVisible` / `renderCursorOverlay` / `exportBeautifyConfig` 等字段。
4. **继续保留人工 gate**：真实 macOS 录制中的 click/video 对齐、CoreGraphics/SCK FFI Native Safety、双光标策略仍需 `npm run tauri dev` 与人工逐行审查。

## 18. Round 8 整改复审（2026-05-28）

> 评审背景：用户已完成 `## 17. Round 7 整改复审（2026-05-28）` 中的整改任务，并明确 Important 2 选择 **方案 B**：Preview 控件影响当前导出；metadata 中的 raw cursor fact 与 Preview 期可变 export intent 分离。
>
> 评审范围：Phase 4 相关代码从 `fb6950554ca94950da81da93520893299cc8cf7a` 到当前工作区未提交改动，重点复审：
>
> - `src-tauri/src/lib.rs`
> - `src-tauri/src/core/timeline.rs`
> - `src-tauri/src/media/cursor_engine.rs`
> - `src-tauri/src/media/recording_metadata.rs`
> - `src-tauri/src/app/cursor_metadata_runtime.rs`
> - `src-tauri/src/platform/macos/screen_capture_kit.rs`
> - `src-tauri/src/platform/macos_service.rs`
> - `src/components/preview-view.tsx`
> - `src/App.test.tsx`
> - `src/lib/tauri.ts`
> - `tests/phase-4-w7-w8-checklist.md`
>
> 当前结论：**With fixes / No**，暂不建议直接合并。Round 7 的方案 B 方向已经基本落地：后端现在用 metadata 中的 `raw_system_cursor_visible` 作为不可变事实，并用当前全局 `beautify_config` 作为导出意图；`EffectTimeline` 也加入了 `raw_system_cursor_visible` / `render_cursor_overlay` 字段。静态复审和自动化验证未发现 Critical，也未发现光标算法进入 ScreenCaptureKit 捕获 callback。但本轮仍发现 3 个 Important：Preview 配置写入仍未串行化，旧 in-flight write 可覆盖新配置；方案 B 的后端安全分支缺少 command 层回归测试；raw cursor 已录入时开启 overlay 的错误目前只进 console，用户不可见。
>
> 交叉验证说明：本轮按 `$superpowers:requesting-code-review` 流程尝试启动独立审查代理 `Feynman`，但代理返回 503，未获得有效审查结果。因此本节结论以本地主审、实际命令验证和代码逐段复核为准。

### 18.1 本轮已确认修复或改善

- `src-tauri/src/core/timeline.rs:68-76`：`EffectTimeline` 已新增 `raw_system_cursor_visible` 和 `render_cursor_overlay`，Phase 6 compositor 不再只能通过 `frames` 是否为空猜测是否渲染 cursor overlay。
- `src-tauri/src/core/timeline.rs:79-91`：`BeautifyConfigSnapshot` 保留录制开始时的配置，并显式保存 `raw_system_cursor_visible`，其中 `raw_system_cursor_visible` 是不可变安全事实。
- `src-tauri/src/lib.rs:138-150`：`start_recording()` 仍在录制开始时写入 `BeautifyConfigSnapshot`，并用 `config.show_system_cursor` 记录 raw SCK frames 是否包含系统光标。
- `src-tauri/src/lib.rs:406-410`：`build_cursor_effect_timeline()` 已重新读取当前全局 `beautify_config`，Preview 期修改可以作为当前 export intent 参与构建，修复上一轮“Preview 控件不影响当前导出”的核心语义分裂。
- `src-tauri/src/lib.rs:420-430`：构建时先读取 `metadata.beautify_config.raw_system_cursor_visible`，再根据当前 config 计算 `want_overlay`；若 raw cursor 已录入且用户想开启 overlay，则拒绝构建，避免双光标。
- `src-tauri/src/lib.rs:432-462`：当 raw cursor 被隐藏时，即使 Preview 中关闭 smoothing/magnification，也会构建 baseline cursor overlay，避免“raw hidden + empty overlay”导致导出无光标。
- `src-tauri/src/lib.rs:473-474`：无论 timeline 来自 engine 还是空 no-op 分支，最终都会覆盖写入 `raw_system_cursor_visible` / `render_cursor_overlay`，使 sidecar 与 metadata fact 一致。
- `src/components/preview-view.tsx:71-72`：新增 `configWriteRef`，尝试跟踪正在飞行的 `setBeautifyConfig()`。
- `src/components/preview-view.tsx:123-135`：`flushPendingConfig()` 已同时考虑 pending debounce config 与当前 `configWriteRef`，比 Round 6 只能 flush 未触发 timer 的实现有明显改善。
- `src/components/preview-view.tsx:156-170`：导出和返回录制主路径都会先调用 `flushPendingConfig()`，再进入 `exportVideo()` 或 `onBack()`。

### 18.2 Critical Findings

本轮未发现 Critical。

没有发现以下阻塞性问题：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 进入 ScreenCaptureKit callback。
- 未发现新增磁盘 IO、JSON 读写、timeline build、Tauri emit、export 等重操作进入 SCK capture callback。
- 未发现新增明显 use-after-free、CVPixelBuffer unlock 后继续读 base address、CoreGraphics event 未释放等硬性内存安全问题。
- 未发现 `CursorMetadataRuntime` samples/clicks 重新退化为无界增长。
- 未发现 `MacRecordingService.stop()` 重新出现 sidecar 写入失败后跳过 cleanup / 状态机悬挂的问题。

### 18.3 Important Findings

#### Important 1: Preview 配置写入仍未串行化，旧 in-flight write 可能晚到覆盖新配置

位置：

- `src/components/preview-view.tsx:113-120`
- `src/components/preview-view.tsx:123-135`
- `src/components/preview-view.tsx:145-149`
- `src/components/preview-view.tsx:156-170`
- `src-tauri/src/lib.rs:340-350`
- `src-tauri/src/lib.rs:406-410`
- `src-tauri/src/lib.rs:519-523`

现象：

Round 7 新增了 `configWriteRef`：

```ts
const writeBeautifyConfig = (config: BeautifyConfig): Promise<void> => {
  const promise = setBeautifyConfig(config).then(() => {})
  configWriteRef.current = promise
  return promise.finally(() => {
    if (configWriteRef.current === promise) {
      configWriteRef.current = null
    }
  })
}
```

这可以解决“debounce 已触发、当前唯一写入仍在飞行、用户立刻导出/返回”的一部分问题。但是它仍不是严格串行化队列，只记录了“最新 promise”。如果旧写入 A 已经发起，用户又产生新配置 B 并立即导出/返回，则 `flushPendingConfig()` 会发起 B：

```ts
const pendingPromise = pendingConfigRef.current
  ? writeBeautifyConfig(pendingConfigRef.current)
  : Promise.resolve()
pendingConfigRef.current = null
const inFlightPromise = configWriteRef.current ?? Promise.resolve()
return Promise.all([pendingPromise, inFlightPromise]).then(() => {})
```

当存在 pending B 时，`writeBeautifyConfig(B)` 会把 `configWriteRef.current` 改成 B 的 promise。此后 `inFlightPromise` 读到的也是 B，而不是更早已经在飞行的 A。因此 flush 等待的是 B，不会等待 A。

可复现场景：

1. 用户在 Preview 中把 smoothing 从 `true` 改成 `false`。
2. 300ms debounce 触发，发起 `setBeautifyConfig(A=false)`，A 仍在飞行。
3. 用户又马上把 smoothing 改回 `true`，此时 `pendingConfigRef.current = B=true`。
4. 用户立即点击导出或返回录制。
5. `flushPendingConfig()` 发起 `setBeautifyConfig(B=true)` 并等待 B。
6. 如果 Tauri/后端调度使 A 在 B 之后才完成，后端全局 `beautify_config` 最终会被 A 覆盖回旧值。
7. A 的 `.then(() => buildCursorEffectTimeline())` 还可能在 B 导出构建之后继续触发旧配置 timeline build。若此时 `beautify_revision` 最后停在 A，旧 build 仍可能通过 `current_revision == build_revision` guard 并写回 `last_effect_timeline_path`。

为什么重要：

- 这不是单纯 UI 状态延迟，而是 Phase 4 双光标策略的主链路配置一致性问题。
- `set_beautify_config()` 是后端进程级全局状态写入，`set_capture_mode()` 会读取它来决定 `CaptureConfig.show_system_cursor`。
- `build_cursor_effect_timeline()` 当前按方案 B 读取全局 `beautify_config` 作为当前 export intent。旧写入晚到会导致当前导出意图回退。
- `beautify_revision` guard 能防止“较早 revision 的 build 覆盖较晚 revision”，但不能防止“较早 UI 意图的写入晚到后成为最新 revision”。换句话说，revision 保护的是完成顺序，不是用户意图顺序。
- 当前 `src/App.test.tsx` 只覆盖 debounce 未触发时导出/返回会先 flush，未覆盖多个 in-flight write 乱序完成。

建议修复：

1. 前端写入必须串行化，推荐使用 promise chain，而不是单个 latest promise ref：

```ts
const writeChainRef = useRef<Promise<void>>(Promise.resolve())

const enqueueBeautifyConfigWrite = (config: BeautifyConfig): Promise<void> => {
  const write = writeChainRef.current
    .catch(() => {})
    .then(() => setBeautifyConfig(config).then(() => {}))
  writeChainRef.current = write
  return write
}
```

2. 所有路径都通过同一个 enqueue helper：
   - debounce timer 写入。
   - `flushPendingConfig()` 写入。
   - unmount cleanup 如果仍保留写入，也必须走同一个队列。
3. `flushPendingConfig()` 应等待“队列中所有已入队写入 + 本次 pending 写入”，而不是只等待 latest promise。
4. 更稳妥的后端兜底：`set_beautify_config` payload 增加 frontend-side monotonic sequence，后端只接受 sequence 更大的配置，拒绝或忽略过期写入。
5. `buildCursorEffectTimeline()` 也应与配置写入序列绑定，旧配置写入完成后触发的旧 build 不应覆盖新配置 build。

建议验收测试：

1. 新增 frontend strict ordering 测试：
   - 第一次 toggle 触发 debounce，并让 A 的 `set_beautify_config` promise pending。
   - 第二次 toggle 后立即导出，发起 B。
   - 先 resolve B，再 resolve A。
   - 断言最终不会以 A 的配置调用 export/build，也不会让旧 build 覆盖新 build。
2. 新增 frontend 返回录制测试：
   - A in-flight、B pending、点击返回。
   - B 完成前不得回到 idle。
   - A 晚到时不得影响下一次 `set_capture_mode()` 使用的最终配置。
3. 如做后端 sequence guard，增加 Rust 或 command wrapper 测试：
   - sequence 2 先写入成功后，sequence 1 晚到应被忽略。

#### Important 2: 方案 B 的后端渲染契约缺少 command 层回归测试

位置：

- `src-tauri/src/lib.rs:420-474`
- `src-tauri/src/core/timeline.rs:68-76`
- `src-tauri/src/media/recording_metadata.rs:12-18`
- `src-tauri/src/media/cursor_engine.rs:713-741`
- `src/App.test.tsx:1074-1172`

现象：

Round 7 已经按方案 B 重构了核心逻辑：

```rust
let raw_visible = metadata.beautify_config.raw_system_cursor_visible;
let want_overlay = config.cursor_magnification || config.cursor_smoothing;

if raw_visible && want_overlay {
    return Err(
        "本次素材已录入系统光标，无法叠加美化光标效果。请先关闭光标美化后重新录制。"
            .to_string(),
    );
}

let render_cursor_overlay = !raw_visible;
```

这是正确方向，但目前缺少直接覆盖这些安全分支的 Rust command 层测试。现有 Rust 测试主要覆盖：

- `EffectTimeline` serde round-trip。
- `RecordingMetadata` round-trip。
- `CursorEffectEngine` smoothing/interpolation/click 纯算法。
- engine 在 features off 时 frame scale/opacity 中性。

缺口是：没有测试证明 `build_cursor_effect_timeline()` 或其可提取的纯 helper 满足方案 B 的关键 contract。

当前缺少的分支：

1. `raw_system_cursor_visible == true` 且当前 Preview 打开 smoothing/magnification：
   - 预期：拒绝生成 overlay，不能产生双光标。
   - 当前代码：返回错误。
   - 测试：缺失。
2. `raw_system_cursor_visible == true` 且当前 Preview 关闭 smoothing/magnification：
   - 预期：允许 no-op timeline，`render_cursor_overlay == false`，`frames` 为空。
   - 当前代码：进入 empty `EffectTimeline` 分支，并覆盖 raw/render 字段。
   - 测试：缺失。
3. `raw_system_cursor_visible == false` 且当前 Preview 关闭 smoothing/magnification：
   - 预期：生成 baseline cursor overlay，`render_cursor_overlay == true`，避免无光标导出。
   - 当前代码：因为 `!raw_visible` 为 true，仍调用 engine，clicks 为空，scale/opacity 中性。
   - 测试：缺失。
4. `raw_system_cursor_visible == false` 且当前 Preview 开启 smoothing/magnification：
   - 预期：生成可渲染 overlay timeline，并按当前 config 应用 smoothing/click。
   - 当前代码：调用 engine。
   - 测试：只有 engine 间接覆盖，没有 command contract 测试。

为什么重要：

- 这些分支是 Phase 4 与 Phase 6 compositor 的交界契约，比单纯 serde round-trip 更关键。
- 如果后续重构 `build_cursor_effect_timeline()` 或 compositor 接入时误把 `frames.is_empty()` 当成唯一信号，双光标或无光标风险会回归。
- Round 7 选择方案 B 后，`raw_system_cursor_visible` 是不可变事实，当前 `beautify_config` 是可变导出意图，这个分离必须用测试锁住。
- `tests/phase-4-w7-w8-checklist.md:73` 写了 “+in-flight config write tracking, flush ordering”，但没有体现方案 B 后端 contract 测试。

建议修复：

1. 从 `build_cursor_effect_timeline()` 中提取一个纯函数或小 helper，例如：

```rust
fn build_effect_timeline_from_metadata(
    metadata: RecordingMetadata,
    config: BeautifyConfigPayload,
) -> Result<EffectTimeline, String>
```

2. 为 helper 增加 4 组测试：
   - raw visible + overlay requested -> Err，错误信息包含“已录入系统光标”。
   - raw visible + overlay not requested -> `render_cursor_overlay == false`，`raw_system_cursor_visible == true`，`frames.is_empty()`。
   - raw hidden + overlay not requested -> `render_cursor_overlay == true`，`raw_system_cursor_visible == false`，有 baseline frames，所有 scale/opacity 中性。
   - raw hidden + overlay requested -> `render_cursor_overlay == true`，有 frames，magnification 时有 click effect。
3. 如果不抽 helper，也应通过临时 metadata 文件和 command wrapper 做集成级测试，至少覆盖 raw visible reject 与 raw hidden baseline 两条高风险分支。
4. 更新 checklist，把这些测试列为 Phase 4/Phase 6 contract gate。

#### Important 3: raw cursor 已录入时开启 overlay 的错误只写 console，用户不可见

位置：

- `src-tauri/src/lib.rs:423-427`
- `src/components/preview-view.tsx:149-152`
- `src/components/preview-view.tsx:157-160`

现象：

后端在 raw cursor 已录入且用户开启 overlay intent 时返回错误：

```rust
if raw_visible && want_overlay {
    return Err(
        "本次素材已录入系统光标，无法叠加美化光标效果。请先关闭光标美化后重新录制。"
            .to_string(),
    );
}
```

但前端处理方式是：

```ts
void writeBeautifyConfig(nextConfig)
  .then(() => buildCursorEffectTimeline())
  .catch((error) => {
    console.error('光标效果处理失败', error)
  })
```

导出路径也是：

```ts
void flushPendingConfig()
  .then(() => exportVideo(preset))
  .catch((error) => {
    console.error('导出失败', error)
  })
```

用户在 Preview 中打开光标平滑/放大后，只会看到控件状态变化；如果当前素材 raw cursor 已录入，timeline build/export 实际失败，但 UI 不显示失败原因，也不恢复控件状态。

为什么重要：

- 方案 B 的安全策略是“raw cursor visible 时禁止 overlay”，这是正确的。但如果错误不可见，用户会误以为当前导出已经开启效果。
- 这会造成产品语义混乱：Preview 控件表现为“影响当前导出”，后端则安全拒绝，前端没有把拒绝反馈给用户。
- 对下一轮整改而言，这会掩盖双光标保护逻辑是否真实生效。用户和测试都很难区分“成功生成 timeline”与“后端拒绝但 UI 没提示”。

建议修复：

1. `PreviewView` 增加可见错误状态，例如 `beautifyError` 或复用现有错误展示机制。
2. `buildCursorEffectTimeline()` / `exportVideo()` 返回 raw cursor conflict 错误时，在 UI 上显示：
   - “本次素材已录入系统光标，无法叠加光标平滑/放大。该设置将影响下一次录制。”
3. 更好的 UX 是在进入 Preview 后读取 metadata 或 build summary，若 raw cursor visible，则禁用当前素材的 smoothing/magnification overlay 控件，或明确标注只影响下一次录制。
4. 增加 frontend 测试：
   - mock `build_cursor_effect_timeline` reject，断言错误信息可见。
   - mock `export_video` reject，断言导出错误可见。
   - raw cursor conflict 时，不应静默保留一个看似已成功应用的 UI 状态。

### 18.4 Minor Findings

#### Minor 1: checklist 对 in-flight config write tracking 的表述过度

位置：

- `tests/phase-4-w7-w8-checklist.md:66-73`
- `src/App.test.tsx:1074-1172`

现象：

checklist 写道：

```markdown
- `npm test -- --run`: **32 tests** PASS (+in-flight config write tracking, flush ordering)
```

但当前新增测试只覆盖：

- debounce 尚未触发时，点击导出会先 `set_beautify_config` 再 `export_video`。
- debounce 尚未触发时，点击返回会写入 pending config。

没有覆盖：

- debounce 已触发但旧写入仍 pending。
- A/B 两次写入乱序完成。
- 旧写入晚到后不应覆盖新配置。
- 旧写入触发的旧 build 不应覆盖新导出 build。

建议修复：

- checklist 改成更精确的描述，例如 “pending config flush ordering”。
- 等 Important 1 的 strict in-flight/乱序测试补齐后，再写 “in-flight config write tracking”。

#### Minor 2: `cursor_engine.rs` 测试注释与 Round 7 方案 B 后的行为不一致

位置：

- `src-tauri/src/media/cursor_engine.rs:713-719`

现象：

测试注释仍写：

```rust
// The command layer in lib.rs short-circuits to an empty
// EffectTimeline when both features are off, before reaching the engine.
```

但 Round 7 方案 B 后，command 层不是简单地在 features off 时 short-circuit：

- 如果 `raw_system_cursor_visible == true` 且 features off，才生成空/no-op timeline。
- 如果 `raw_system_cursor_visible == false` 且 features off，仍会调用 engine 生成 baseline cursor overlay，避免无光标导出。

建议修复：

- 更新注释为“engine supports neutral overlay frames when features are off; command layer decides whether to use them based on raw cursor visibility”。
- 避免后续维护者误以为 features off 永远不需要 overlay timeline。

#### Minor 3: stale/cancelled build 仍会先写临时 timeline 文件

位置：

- `src-tauri/src/lib.rs:476-478`
- `src-tauri/src/lib.rs:511-540`

现象：

`build_cursor_effect_timeline()` 在 `spawn_blocking` 中先写 effect timeline JSON：

```rust
RecordingMetadataWriter::write_effect_timeline(&path, &timeline)
```

随后回到 async path 才检查 session/metadata/revision 是否仍匹配。如果检查失败，会返回 stale/cancelled error，但刚写出的临时 JSON 文件不会删除。

为什么重要：

- 这不是捕获主链路问题，也不是 correctness blocker，因为 service 不会写回 stale path。
- 但高频 Preview 调整或多次 stale build 会在 temp 目录留下未引用文件。

建议修复：

- 如果 guard 失败，尽量 `remove_file(&path)`，错误可忽略。
- 或先把 timeline 写到 request-scoped temp path，guard 通过后再发布/rename 到正式路径。
- 如果保留现状，至少在 checklist 中标为临时资源清理风险，等待 Phase 6 文件生命周期一起治理。

#### Minor 4: `start_recording()` 对 `show_system_cursor` 仍依赖调用方先执行 `set_capture_mode()`

位置：

- `src-tauri/src/lib.rs:138-150`
- `src-tauri/src/lib.rs:289-309`

现象：

`set_capture_mode()` 根据当前 `beautify_config` 决定 `CaptureConfig.show_system_cursor`。`start_recording()` 则直接使用已有 `capture_config.show_system_cursor` 写入 `raw_system_cursor_visible`。

App 当前主路径是：

1. `setCaptureMode(...)`
2. `setAudioConfig(...)`
3. `startRecording()`

因此正常 UI 流程暂时成立。但后端 command contract 仍依赖调用顺序。如果外部调用、测试或未来重构在 `set_beautify_config()` 后直接调用 `start_recording()`，可能出现：

- 当前 `beautify_config` 表示 overlay intent。
- `capture_config.show_system_cursor` 仍是旧值。
- metadata 中 `raw_system_cursor_visible` 与当前 config 意图不一致。

建议修复：

- 在 `start_recording()` 即将开始 native capture 前，重新根据当前 `beautify_config` 推导 `config.show_system_cursor`，或至少校验：

```rust
config.show_system_cursor == !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing)
```

- 如果选择校验，错误信息应提示调用方先同步 capture mode。
- 这样可以降低前端调用顺序或异步 race 对 raw cursor fact 的影响。

### 18.5 Phase 4 完整性复审结论

已完成或基本完成：

- `CursorSample` / `CursorClick` / `CursorFrame` / `CursorClickEffect` / `EffectTimeline` serde 模型已落地。
- `CursorProcessor` trait 边界已落地。
- 移动平均平滑、Bezier 插值、点击放大状态机和 `CursorEffectEngine` 已落地并有算法测试。
- 录制期 cursor metadata 采集运行在独立 `CursorMetadataRuntime` 线程，不进入 SCK capture callback。
- `CursorMetadataRecorder` samples/clicks 均 bounded，避免长录制无界增长。
- 停止录制后写入 cursor metadata sidecar，且 sidecar 写入失败不再跳过 cleanup。
- `RecordingMetadata` 已持久化 `BeautifyConfigSnapshot`，其中 `raw_system_cursor_visible` 作为不可变 raw cursor fact。
- `build_cursor_effect_timeline()` 已采用方案 B：metadata raw fact + 当前 Preview export intent。
- `EffectTimeline` 已显式包含 `raw_system_cursor_visible` / `render_cursor_overlay`。
- raw cursor visible + overlay requested 会被拒绝，避免双光标。
- raw cursor hidden + overlay disabled 会生成 baseline overlay，避免无光标。
- `set_beautify_config`、`get_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- `build_cursor_effect_timeline` 使用 `spawn_blocking`，并拒绝 `Recording | Paused | Processing` 状态。
- `session_id + metadata_path + beautify_revision` guard 仍能阻止跨 session stale build 写回。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`。
- PTS invalid/negative/special flag 检查已加强，PTS delta 使用 `i128`，CMTime conversion 使用 checked integer arithmetic。
- React 未接收视频帧、音频帧或 cursor sample stream，符合架构红线。

仍未完全完成或需作为下一轮整改/gate：

- Preview 配置写入队列没有串行化，旧 in-flight write 可晚到覆盖新配置。
- 旧配置写入完成后触发的旧 `buildCursorEffectTimeline()` 仍可能覆盖新导出结果。
- 方案 B 的核心后端安全分支缺少 Rust command/helper 测试。
- raw cursor conflict 错误只写 console，用户不可见。
- `tests/phase-4-w7-w8-checklist.md` 对 in-flight tracking 的测试说明过度，需要修正或补测试。
- `cursor_engine.rs` 中 features-off 注释与当前 command 行为不一致。
- stale/cancelled build 会留下未引用 temp timeline JSON。
- 真实 macOS 录制中 CMSampleBuffer PTS 与 click/video 动作对齐仍需 `npm run tauri dev` 人工验证。
- `cursor_source.rs` CoreGraphics FFI 与 `screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList 仍需 Native Safety Gate 人工逐行审查。
- 双光标策略和 raw-hidden baseline overlay 仍需真实素材人工确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor 接入。

结论：

Phase 4 主体功能已经基本完成，方案 B 的核心方向正确，但“Preview 配置写入顺序”和“Phase 6 渲染契约测试”仍未闭环。本轮建议继续整改后再合并。

### 18.6 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- SCK callback 当前仍只做 CMSampleBuffer PTS 提取、CVPixelBuffer 数据复制、AudioBufferList 读取/PCM 转换、bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内使用 `Mutex<Option<(u64, u64)>>`，但只保护轻量时间戳映射；没有等待 UI、磁盘、timeline build 或 export。
- Cursor polling 由 `CursorMetadataRuntime` 独立线程执行，不在 SCK callback 内调用 CoreGraphics cursor source。
- Timeline build 是录后 Tauri command 路径，并使用 `spawn_blocking`。
- `MacScreenCapture::start_stream()` / `stop()` 中等待 SCK start/stop completion 的同步等待发生在 recording service 的 blocking 调用路径，不在 SCK callback 内。

内存安全：

- `cursor_source.rs` 中 `CGEventCreate` 返回 event 后做 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；静态复审表面配对正确，仍需人工 Native Safety Gate。
- `screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取，并复制到 `Arc<[u8]>`；unlock 后不再引用原始 base address。
- Audio `block_buffer` 当前 return 分支均有 `cf_release`；未发现 Round 8 新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded。
- Timeline 构建仍一次性读取 metadata、生成 frames、pretty serialize JSON；这是录后内存峰值风险，不影响捕获主链路。Phase 6 接入真实长素材后仍需压力测试。
- stale/cancelled build 可能留下未引用 temp JSON，属于资源清理问题，不是内存安全问题。

线程安全：

- Rust 侧共享 service/config/tick/mic runtime 状态使用 `Arc<Mutex<...>>`，beautify revision 使用 `AtomicU64`；未发现 Rust 数据竞争。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path + beautify_revision` stale guard 可以阻止旧 session / 旧 revision build 写回 service。
- 主要线程/异步一致性风险在前端 promise 调度：`configWriteRef` 只记录 latest promise，不能保证多次 `setBeautifyConfig` 按用户意图顺序落到后端。
- 后端 `set_beautify_config()` 自身是 Mutex 保护的原子写入，但没有 request sequence，所以无法识别旧 UI 意图晚到。

资源释放路径：

- `MacRecordingService.stop()` 当前会先停止 cursor runtime，再 stop native capture / mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点。
- Preview unmount cleanup 会清理 debounce timer，并尝试 fire-and-forget 写入 pending config；这不能作为主路径一致性保证，只能作为 best-effort cleanup。下一轮应通过串行写入队列和可等待 flush 闭环。

### 18.7 BUG.md 预防规则检查

执行命令：

```bash
rg -n 'data-tauri-drag-region="false"|whileTap|motion\.div' src src-tauri
```

命中要点：

```text
src/components/preview-view.tsx:184:    <motion.div
src/components/preview-view.tsx:228:              <motion.div
src/components/preview-view.tsx:325:                <motion.div
src/components/preview-view.tsx:385:                <motion.div
src/components/recording-status-bar.tsx:30:    <motion.div
src/components/error-view.tsx:13:    <motion.div
src/components/processing-view.tsx:10:    <motion.div
src/components/recording-panel.tsx:89:              whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:155:            whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:171:            whileTap={{ scale: 0.98 }}
src/App.test.tsx:431:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/App.test.tsx:443:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
```

解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443` 是测试断言不存在 `data-tauri-drag-region="false"`，不是源码新增 wrapper。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171` 是 `motion.button whileTap`，`whileTap` 位于交互元素自身，不是 BUG-003 中 `motion.div whileTap` 作为 Button 直接父容器的拦截模式。
- 多处 `motion.div` 用于页面/装饰动画，没有发现 `motion.div` 带 `whileTap` 并直接包裹交互 Button。

结论：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

### 18.8 本轮实际验证

本轮实际执行：

```bash
git diff --check fb6950554ca94950da81da93520893299cc8cf7a
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm test -- --run src/App.test.tsx
npm run build
npm test -- --run
rg -n 'data-tauri-drag-region="false"|whileTap|motion\.div' src src-tauri
```

结果：

- `git diff --check fb6950554ca94950da81da93520893299cc8cf7a`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **120 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 21 warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, 21 warnings
- `npm test -- --run src/App.test.tsx`: PASS, **32 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- `npm run build`: PASS
- `npm test -- --run`: PASS, **32 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- BUG.md 规则扫描：未发现新增违规 pattern

补充说明：

- 当前 21 个 Rust warning 仍主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮未发现新的 clippy error。
- 本轮没有运行 `npm run tauri dev`，真实 macOS 录制、click/video 对齐、双光标视觉检查、Native Safety Gate 仍需人工验证。
- `tests/phase-4-w7-w8-checklist.md` 的 “in-flight config write tracking” 表述需要在下一轮随测试补齐或修正。

### 18.9 Round 8 Ready To Merge?

**Ready to merge: With fixes / No**

本轮未发现 Critical，也未发现会直接阻塞捕获主链路的新增 Phase 4 代码。Round 7 选择的方案 B 已经把核心数据契约向正确方向推进：

1. metadata 中保存 raw cursor immutable fact。
2. Preview 当前配置作为 export intent 参与构建。
3. raw cursor visible + overlay requested 被拒绝，避免双光标。
4. raw cursor hidden + overlay disabled 会生成 baseline overlay，避免无光标。
5. timeline sidecar 已包含 `rawSystemCursorVisible` / `renderCursorOverlay`。

但建议在最终合并前继续完成以下整改：

1. **串行化 Preview config writes**：保证多次 `setBeautifyConfig()` 按用户意图顺序落到后端，旧写入晚到不能覆盖新配置。
2. **阻止旧写入触发的旧 timeline build 覆盖新导出**：写入序列、build request id、revision guard 需要共同表达“用户最新意图”。
3. **补方案 B 后端 contract 测试**：raw visible reject、raw visible no-overlay、raw hidden baseline、raw hidden overlay 四条分支必须有 Rust 测试。
4. **给 raw cursor conflict 做可见 UI 反馈**：不要只 `console.error`。
5. **修正文档与注释**：checklist 的 in-flight 测试描述、`cursor_engine.rs` features-off 注释需要与现状一致。
6. **继续保留人工 gate**：真实 macOS click/video 对齐、CoreGraphics/SCK FFI Native Safety、双光标和 baseline overlay 视觉效果仍需人工验证。

## 19. Round 9 整改复审（2026-05-28）

> 评审背景：用户已完成 `## 18. Round 8 整改复审（2026-05-28）` 中的整改任务，本轮复审当前工作区未提交改动，重点确认 Round 8 的 6 条整改建议是否真正闭环，并继续专项检查 Phase 4 是否完整完成、是否存在阻塞捕获主链路、内存安全、线程安全和资源释放路径异常。
>
> 当前工作区状态：
>
> - `src-tauri/src/lib.rs`
> - `src-tauri/src/media/cursor_engine.rs`
> - `src/App.test.tsx`
> - `src/components/preview-view.tsx`
> - `tests/phase-4-w7-w8-checklist.md`
>
> 当前结论：**With fixes / No**，暂不建议直接合并。本轮未发现 Critical，也未发现 cursor smoothing / Bezier interpolation / click magnification 进入 ScreenCaptureKit 捕获 callback；Round 8 的主要整改大多已经完成。但仍有 1 个 Important：`flushPendingConfig()` 会吞掉 `setBeautifyConfig()` 写入失败，导致导出或返回录制可以在后端仍为旧配置时继续执行，破坏“Preview 当前配置影响当前导出”的方案 B 契约。
>
> 交叉验证：本轮按 `$superpowers:requesting-code-review` 流程启动独立审查代理 `Mill`。独立审查与本地主审结论一致：无 Critical；核心 Important 是 `flushPendingConfig()` 的 `.catch(() => {})` 会把配置写入失败转成成功，导致 export 继续使用 stale backend config。独立审查同时指出 checklist 中 “raw cursor conflict error UI” 的测试描述偏乐观。

### 19.1 本轮已确认修复或改善

- `src/components/preview-view.tsx:71`：Round 8 的 `configWriteRef` 已替换为 `writeChainRef`，写入不再只追踪 latest promise。
- `src/components/preview-view.tsx:115-120`：`enqueueConfigWrite()` 使用 promise chain 串行化 `setBeautifyConfig()`，可以保证 A 写入完成后才执行 B 写入，避免旧 in-flight write 晚到覆盖新配置。
- `src/components/preview-view.tsx:126-135`：`flushPendingConfig()` 会清理 debounce timer，并把 pending config 入队到同一条 write chain；导出和返回录制主路径会等待该 chain。
- `src/components/preview-view.tsx:145-159`：debounce 触发后的 config write 完成后才调用 `buildCursorEffectTimeline()`，并在 raw cursor conflict 时设置 `beautifyError`。
- `src/components/preview-view.tsx:163-175`：导出路径会先 `flushPendingConfig()`，再调用 `exportVideo()`，并在 raw cursor conflict 时设置 `beautifyError`。
- `src/components/preview-view.tsx:432-435`：新增可见错误提示，不再只把 raw cursor conflict 写入 console。
- `src-tauri/src/lib.rs:144-152`：`start_recording()` 会重新根据当前 `beautify_config` 推导 `show_system_cursor`，降低调用方忘记先 `set_capture_mode()` 时 raw cursor fact 与当前意图不一致的风险。
- `src-tauri/src/lib.rs:376-437`：抽出了 `build_effect_timeline_from_metadata()` 纯 helper，集中表达方案 B contract：metadata 中的 `raw_system_cursor_visible` 是不可变事实，当前 Preview config 是导出意图。
- `src-tauri/src/lib.rs:676-742`：新增 4 个 Rust contract 测试，覆盖 raw visible reject、raw visible no-overlay、raw hidden baseline、raw hidden overlay。
- `src-tauri/src/lib.rs:548-551`：stale/cancelled build 在 guard 失败时会尝试删除已经写出的临时 timeline JSON，减少未引用 temp 文件残留。
- `src-tauri/src/media/cursor_engine.rs:715-721`：features-off 注释已更新为当前方案 B 语义：engine 支持 neutral overlay frames，command layer 根据 raw cursor visibility 决定是否调用。
- `src/App.test.tsx:1174-1243`：新增 promise chain serialization 测试，覆盖 A in-flight、B pending、点击导出后必须按 `A -> B -> export` 顺序完成。
- `tests/phase-4-w7-w8-checklist.md:68-73`：验证摘要已更新到 Round 8，Rust 124 tests / frontend 33 tests。

### 19.2 Critical Findings

本轮未发现 Critical。

没有发现以下阻塞性问题：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- 未发现新增磁盘 IO、JSON 读写、timeline build、Tauri emit、export 等重操作进入 SCK capture callback。
- 未发现新增明显 use-after-free、CVPixelBuffer unlock 后继续读 base address、CoreGraphics event 未释放等硬性内存安全问题。
- 未发现 `CursorMetadataRuntime` samples/clicks 退化为无界增长。
- 未发现 `MacRecordingService.stop()` 重新出现 sidecar 写入失败后跳过 cleanup / 状态机悬挂的问题。

### 19.3 Important Findings

#### Important 1: `flushPendingConfig()` 吞掉配置写入失败，导出可能继续使用旧后端配置

位置：

- `src/components/preview-view.tsx:126-135`
- `src/components/preview-view.tsx:163-175`
- `src/components/preview-view.tsx:178-184`
- `src-tauri/src/lib.rs:351-361`
- `src-tauri/src/lib.rs:483-497`
- `src-tauri/src/lib.rs:586-596`

现象：

Round 8 将写入顺序改成 promise chain，这是正确方向。但 `flushPendingConfig()` 当前实现会吞掉 chain 中的错误：

```ts
const flushPendingConfig = (): Promise<void> => {
  if (debounceRef.current) {
    clearTimeout(debounceRef.current)
    debounceRef.current = null
  }
  if (pendingConfigRef.current) {
    enqueueConfigWrite(pendingConfigRef.current)
    pendingConfigRef.current = null
  }
  return writeChainRef.current.catch(() => {})
}
```

这会把 `setBeautifyConfig()` 的失败转成成功。导出路径随后继续执行：

```ts
void flushPendingConfig()
  .then(() => exportVideo(preset))
```

因此只要 pending 或 in-flight config write 失败，`exportVideo()` 仍会运行，并使用 Rust 后端中尚未更新的旧 `beautify_config`。

可复现场景：

1. 用户在 Preview 中关闭 `cursorSmoothing` 或 `cursorMagnification`。
2. 前端 state 已经更新，`pendingConfigRef.current` 保存了用户最新意图。
3. 用户马上点击导出。
4. `flushPendingConfig()` 入队并执行 `setBeautifyConfig(latestConfig)`。
5. Tauri invoke reject，或后端 `set_beautify_config` 返回错误。
6. `flushPendingConfig()` 的 `.catch(() => {})` 吞掉错误并 resolve。
7. `exportVideo()` 继续调用后端。
8. 后端 `build_cursor_effect_timeline()` 读取的是旧 `beautify_config`，不是用户看到的当前 Preview 控件状态。

为什么重要：

- 方案 B 的核心契约是“metadata raw cursor fact 不可变，Preview 当前 config 作为当前导出意图”。如果 flush 失败后仍导出，就不能保证导出使用的是 Preview 当前意图。
- `build_cursor_effect_timeline()` 当前会读取全局 `beautify_config` 作为 export intent；这个值由 `set_beautify_config()` 写入。如果写入失败被吞，旧配置仍会参与 timeline/export。
- 这会重新带来 raw cursor / overlay 策略不一致风险。例如 UI 上用户认为已经关闭 overlay，但后端旧配置仍要求 overlay；或者 UI 上用户认为已开启效果，但后端仍按旧配置导出。
- 这不是捕获 callback 热路径问题，但属于 Phase 4 Preview/export 主链路一致性问题，建议合并前修复。

建议修复：

1. 保留 promise chain 的 recoverable 语义，但不要在 `flushPendingConfig()` 主路径吞错。
2. 推荐把“队列恢复”和“本次 flush 结果”分开：

```ts
const enqueueConfigWrite = (config: BeautifyConfig): Promise<void> => {
  const previous = writeChainRef.current
  const write = previous
    .catch(() => {})
    .then(() => setBeautifyConfig(config).then(() => {}))
  writeChainRef.current = write
  return write
}

const flushPendingConfig = (): Promise<void> => {
  if (debounceRef.current) {
    clearTimeout(debounceRef.current)
    debounceRef.current = null
  }
  if (pendingConfigRef.current) {
    const pending = pendingConfigRef.current
    pendingConfigRef.current = null
    return enqueueConfigWrite(pending)
  }
  return writeChainRef.current
}
```

3. `handleExport()`：如果 flush reject，不应调用 `exportVideo()`；应展示可见错误，例如“美化配置保存失败，请重试导出”。
4. `handleBack()`：是否允许保存失败后仍返回录制需要产品确认。若为了避免卡死可以保留 fallback `onBack()`，但应显示或记录可见失败，并注意下一次录制可能使用旧配置。
5. 新增前端回归测试：
   - pending config flush 中 `set_beautify_config` reject。
   - 点击导出。
   - 断言 `export_video` 未被调用。
   - 断言页面显示可见错误。
6. 可选后端兜底：为 `set_beautify_config` 增加 frontend monotonic sequence，后端忽略过期配置写入；这能防止未来其他入口绕过前端 chain。

### 19.4 Minor Findings

#### Minor 1: checklist 对 raw cursor conflict error UI 的测试覆盖描述偏乐观

位置：

- `tests/phase-4-w7-w8-checklist.md:73`
- `src/components/preview-view.tsx:432-435`
- `src/App.test.tsx:1174-1243`

现象：

checklist 写道：

```markdown
- `npm test -- --run`: **33 tests** PASS (+promise chain serialization, raw cursor conflict error UI)
```

当前 UI 确实新增了 `beautifyError` 可见展示，但新增的第 33 个测试主要验证 promise chain serialization 和 export 等待顺序，并没有断言 raw cursor conflict 错误会显示在页面上。

为什么重要：

- raw cursor conflict 是方案 B 防双光标策略的关键用户可见分支。
- 如果后续 UI 重构把错误状态删掉，当前测试不会失败。
- checklist 会让后续整改者误以为错误 UI 已经有自动化覆盖。

建议修复：

1. 新增测试：mock `build_cursor_effect_timeline` reject `本次素材已录入系统光标...`，断言页面展示该错误。
2. 新增测试：mock `export_video` reject 同类错误，断言页面展示该错误。
3. 或在补测试前把 checklist 文案改成 `+promise chain serialization; raw cursor conflict error UI implementation`，不要写成已覆盖。

#### Minor 2: raw hidden baseline contract 测试未断言 frames 非空

位置：

- `src-tauri/src/lib.rs:708-725`

现象：

测试名是：

```rust
fn raw_hidden_no_overlay_generates_baseline_frames()
```

但断言只检查：

```rust
assert!(!timeline.raw_system_cursor_visible);
assert!(timeline.render_cursor_overlay);
for frame in &timeline.frames {
    assert_eq!(frame.scale, 1.0);
    assert_eq!(frame.opacity, 1.0);
}
```

如果未来回归导致 `timeline.frames` 为空，`for` 循环不会执行，测试仍会通过。

为什么重要：

- raw cursor hidden + overlay disabled 的关键目标是避免“无光标导出”。
- 这个分支必须证明会生成 baseline overlay frames，而不只是设置 `render_cursor_overlay == true`。

建议修复：

在该测试中增加：

```rust
assert!(!timeline.frames.is_empty());
```

必要时还可以断言第一帧坐标来自 metadata sample，进一步锁住 baseline overlay 内容。

### 19.5 Phase 4 完整性复审结论

已完成或基本完成：

- `CursorSample` / `CursorClick` / `CursorFrame` / `CursorClickEffect` / `EffectTimeline` serde 模型已落地。
- `EffectTimeline` 已包含 `raw_system_cursor_visible` / `render_cursor_overlay`，为 Phase 6 compositor 提供明确渲染 contract。
- `CursorProcessor` trait 边界已落地。
- 移动平均平滑、Bezier 插值、点击放大状态机和 `CursorEffectEngine` 已落地并有算法测试。
- 录制期 cursor metadata 采集运行在独立 `CursorMetadataRuntime` 线程，不进入 SCK capture callback。
- `CursorMetadataRecorder` samples/clicks 均 bounded，避免长录制无界增长。
- 停止录制后写入 cursor metadata sidecar，且 sidecar 写入失败不再跳过 cleanup。
- `RecordingMetadata` 已持久化 `BeautifyConfigSnapshot`，其中 `raw_system_cursor_visible` 作为不可变 raw cursor fact。
- `build_cursor_effect_timeline()` 已采用方案 B：metadata raw fact + 当前 Preview export intent。
- raw cursor visible + overlay requested 会被拒绝，避免双光标。
- raw cursor hidden + overlay disabled 会生成 baseline overlay，避免无光标。
- `set_beautify_config`、`get_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- Preview 已接入 beautify config、timeline build 和 export command。
- Preview config writes 已通过 promise chain 串行化，避免 A/B 写入乱序完成。
- `build_cursor_effect_timeline` 使用 `spawn_blocking`，并拒绝 `Recording | Paused | Processing` 状态。
- `session_id + metadata_path + beautify_revision` guard 仍能阻止跨 session stale build 写回。
- stale build guard 失败后会尝试清理未引用 temp timeline JSON。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`。
- `start_recording()` 会重新根据当前 beautify config 推导 `show_system_cursor`，降低调用顺序风险。
- PTS invalid/negative/special flag 检查已加强，PTS delta 使用 `i128`，CMTime conversion 使用 checked integer arithmetic。
- React 未接收视频帧、音频帧或 cursor sample stream，符合架构红线。

仍未完全完成或需作为下一轮整改/gate：

- `flushPendingConfig()` 会吞掉 `setBeautifyConfig()` 失败，导致导出可能继续使用旧后端配置。
- raw cursor conflict error UI 缺少前端可见性回归测试。
- raw hidden baseline Rust 测试未断言 `frames` 非空。
- 真实 macOS 录制中 CMSampleBuffer PTS 与 click/video 动作对齐仍需 `npm run tauri dev` 人工验证。
- `cursor_source.rs` CoreGraphics FFI 与 `screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList 仍需 Native Safety Gate 人工逐行审查。
- 双光标策略和 raw-hidden baseline overlay 仍需真实素材人工确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor 接入。

结论：

Phase 4 主体功能已经基本完成，Round 8 的大部分整改有效，当前剩余阻塞集中在 Preview/export 配置 flush 失败路径。该问题修复并补充测试后，Phase 4 可以进入下一轮合并前复审；人工 Native Safety Gate 和真实 macOS 视觉检查仍需保留。

### 19.6 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- SCK callback 当前仍只做 CMSampleBuffer PTS 提取、CVPixelBuffer 数据复制、AudioBufferList 读取/PCM 转换、bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内使用 `Mutex<Option<(u64, u64)>>`，但只保护轻量时间戳映射；没有等待 UI、磁盘、timeline build 或 export。
- Cursor polling 由 `CursorMetadataRuntime` 独立线程执行，不在 SCK callback 内调用 CoreGraphics cursor source。
- Timeline build 是录后 Tauri command 路径，并使用 `spawn_blocking`。
- `MacScreenCapture::start_stream()` / `stop()` 中等待 SCK start/stop completion 的同步等待发生在 recording service 的 blocking 调用路径，不在 SCK callback 内。
- 本轮未发现新增阻塞捕获主链路的问题。

内存安全：

- `cursor_source.rs` 中 `CGEventCreate` 返回 event 后做 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；静态复审表面配对正确，仍需人工 Native Safety Gate。
- `screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取，并复制到 `Arc<[u8]>`；unlock 后不再引用原始 base address。
- Audio `block_buffer` 当前 return 分支均有 `cf_release`；未发现 Round 9 新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded。
- Timeline 构建仍一次性读取 metadata、生成 frames、pretty serialize JSON；这是录后内存峰值风险，不影响捕获主链路。Phase 6 接入真实长素材后仍需压力测试。
- stale/cancelled build 会尝试删除未引用 temp JSON；资源清理较 Round 8 改善。

线程安全：

- Rust 侧共享 service/config/tick/mic runtime 状态使用 `Arc<Mutex<...>>`，beautify revision 使用 `AtomicU64`；未发现 Rust 数据竞争。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path + beautify_revision` stale guard 可以阻止旧 session / 旧 revision build 写回 service。
- 前端 promise chain 已能保证多次 `setBeautifyConfig()` 按用户意图顺序落到后端。
- 剩余线程/异步一致性风险不是顺序问题，而是失败传播问题：flush 失败被吞后，后续 export/back 会继续执行。

资源释放路径：

- `MacRecordingService.stop()` 当前会先停止 cursor runtime，再 stop native capture / mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点。
- Preview unmount cleanup 会清理 debounce timer，并尝试 fire-and-forget 写入 pending config；这只能作为 best-effort cleanup，不应作为主路径一致性保证。主路径应由 `flushPendingConfig()` 正确传播失败来保证。

### 19.7 BUG.md 预防规则检查

执行命令：

```bash
rg -n 'data-tauri-drag-region="false"|whileTap|motion\.div' src src-tauri
```

命中要点：

```text
src/components/preview-view.tsx:198:    <motion.div
src/components/preview-view.tsx:242:              <motion.div
src/components/preview-view.tsx:339:                <motion.div
src/components/preview-view.tsx:362:                </motion.div>
src/components/preview-view.tsx:399:                <motion.div
src/components/preview-view.tsx:424:                </motion.div>
src/components/preview-view.tsx:471:    </motion.div>
src/components/recording-status-bar.tsx:30:    <motion.div
src/components/recording-status-bar.tsx:39:        <motion.div
src/components/recording-status-bar.tsx:50:            <motion.div
src/components/recording-status-bar.tsx:57:        </motion.div>
src/components/recording-status-bar.tsx:74:              <motion.div
src/components/recording-status-bar.tsx:109:    </motion.div>
src/components/recording-panel.tsx:65:    <motion.div
src/components/recording-panel.tsx:89:              whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:155:            whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:171:            whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:185:                  <motion.div
src/components/recording-panel.tsx:208:    </motion.div>
src/components/processing-view.tsx:10:    <motion.div
src/components/processing-view.tsx:17:        <motion.div
src/components/processing-view.tsx:22:        </motion.div>
src/components/processing-view.tsx:25:    </motion.div>
src/components/error-view.tsx:13:    <motion.div
src/components/error-view.tsx:37:    </motion.div>
src/App.test.tsx:431:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/App.test.tsx:443:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
```

解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443` 是测试断言不存在 `data-tauri-drag-region="false"`，不是源码新增 wrapper。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171` 是 `motion.button whileTap`，`whileTap` 位于交互元素自身，不是 BUG-003 中的 `motion.div whileTap` 作为 Button 直接父容器拦截模式。
- 多处 `motion.div` 用于页面/装饰动画，没有发现 `motion.div` 带 `whileTap` 并直接包裹交互 Button。

结论：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

### 19.8 本轮实际验证

本轮实际执行：

```bash
git diff --check HEAD
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n 'data-tauri-drag-region="false"|whileTap|motion\.div' src src-tauri
```

结果：

- `git diff --check HEAD`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **124 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, **21 warnings**（既有 macOS FFI naming / unused unsafe / dead_code 类 warning）
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, **21 warnings**
- `npm run build`: PASS
- `npm test -- --run`: PASS, **33 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- BUG.md 规则扫描：未发现新增违规 pattern

补充说明：

- 本轮没有运行 `npm run tauri dev`，真实 macOS 录制、click/video 对齐、双光标视觉检查、Native Safety Gate 仍需人工验证。
- 当前 Rust warnings 主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮未发现 clippy error。

### 19.9 Round 9 Ready To Merge?

**Ready to merge: With fixes / No**

本轮未发现 Critical，也未发现会直接阻塞捕获主链路的新增 Phase 4 代码。Round 8 的多数整改已经有效：

1. Preview config writes 已串行化，旧 in-flight write 不应再晚到覆盖新配置。
2. 旧 build stale guard 失败时会清理 temp timeline JSON。
3. 方案 B 的 4 条后端 contract 分支已有 Rust helper 测试。
4. raw cursor conflict 已有可见 UI 状态。
5. `start_recording()` 会重新推导 `show_system_cursor`，降低 command 调用顺序风险。
6. `cursor_engine.rs` 注释已与方案 B 行为对齐。

但建议在最终合并前继续完成以下整改：

1. **修复 `flushPendingConfig()` 错误吞掉问题**：配置写入失败时不得继续导出当前素材；应展示可见错误并阻止 `exportVideo()`。
2. **补前端失败路径测试**：pending config flush reject 时 `export_video` 不应被调用；raw cursor conflict reject 时错误信息应可见。
3. **补严 raw hidden baseline Rust 测试**：`raw_hidden_no_overlay_generates_baseline_frames` 应断言 `timeline.frames` 非空。
4. **同步 checklist 文案**：在 raw cursor conflict error UI 测试补齐前，不要写成该 UI 已由 frontend tests 覆盖。
5. **继续保留人工 gate**：真实 macOS click/video 对齐、CoreGraphics/SCK FFI Native Safety、双光标和 baseline overlay 视觉效果仍需人工验证。

## 20. Round 10 整改复审（2026-05-28）

> 评审背景：用户已完成 `## 19. Round 9 整改复审（2026-05-28）` 中列出的整改任务，本轮复审当前工作区未提交改动，重点确认 Round 9 的 4 条整改建议是否真正闭环，并继续专项检查 Phase 4 是否完整完成、是否存在阻塞捕获主链路、内存安全、线程安全和资源释放路径异常。
>
> 当前工作区状态：
>
> - `docs/superpowers/reviews/2026-05-27-phase-4-code-review.md`
> - `src-tauri/src/lib.rs`
> - `src-tauri/src/media/cursor_engine.rs`
> - `src/App.test.tsx`
> - `src/components/preview-view.tsx`
> - `tests/phase-4-w7-w8-checklist.md`
>
> 当前结论：**With fixes / No**，暂不建议直接合并。本轮未发现 Critical，也未发现 cursor smoothing / Bezier interpolation / click magnification 进入 ScreenCaptureKit 捕获 callback。Round 9 的 export 阻塞项已经修复并通过测试确认；剩余 1 个 Important：`handleBack()` 在配置 flush 失败后仍会继续返回录制页，可能让下一次录制沿用后端旧美化配置并造成 raw cursor visibility 与用户最后看到的 Preview 设置不一致。
>
> 交叉验证：本轮按 `$superpowers:requesting-code-review` 流程启动独立审查代理 `Kepler`。独立审查结论与本地主审一致：无 Critical；Round 9 的 export-specific blocker 已修复；核心剩余 Important 是 `handleBack()` 仍在 flush reject 后调用 `onBack()`；另有 Minor：`HANDOFF.md` 中 Phase 4 验证数据仍为旧值。

### 20.1 本轮已确认修复或改善

- `src/components/preview-view.tsx:115-120`：`enqueueConfigWrite()` 保持 promise chain 串行化写入，旧 in-flight write 失败后队列可恢复，后续写入仍按用户意图顺序落到后端。
- `src/components/preview-view.tsx:128-138`：`flushPendingConfig()` 已移除 `.catch(() => {})` 吞错逻辑。当前函数会清理 debounce timer、把 pending config 入队到同一条 write chain，并直接返回 `writeChainRef.current`，让调用方收到真实 reject。
- `src/components/preview-view.tsx:165-174`：导出路径 `handleExport()` 会先等待 `flushPendingConfig()`，如果 config flush reject，不会继续调用 `exportVideo()`；raw cursor conflict 错误会进入 `beautifyError` 可见状态。
- `src/App.test.tsx:1276-1317`：新增 `blocks export when flushPendingConfig rejects and shows error` 测试，覆盖 pending config flush reject 后 `export_video` 不应被调用，并断言错误信息可见。
- `src/App.test.tsx:1245-1274`：新增 `shows error when build_cursor_effect_timeline rejects with raw cursor conflict` 测试，覆盖 raw cursor conflict 在 timeline build 阶段的可见错误 UI。
- `src/App.test.tsx:1174-1243`：promise chain serialization 测试仍覆盖 A in-flight、B pending、点击导出后必须按 `A -> B -> export` 顺序完成。
- `src-tauri/src/lib.rs:376-437`：`build_effect_timeline_from_metadata()` 继续集中表达方案 B contract：metadata 中的 `raw_system_cursor_visible` 是不可变事实，当前 Preview config 是当前导出意图。
- `src-tauri/src/lib.rs:709-725`：`raw_hidden_no_overlay_generates_baseline_frames` 已补 `assert!(!timeline.frames.is_empty())`，防止 raw cursor hidden + overlay disabled 分支退化成“标记为渲染 overlay 但没有 baseline frames”。
- `src-tauri/src/media/cursor_engine.rs:715-721`：features-off 注释已与当前方案 B 行为一致：engine 支持 neutral overlay frames，command layer 根据 raw cursor visibility 决定是否调用。
- `tests/phase-4-w7-w8-checklist.md:66-73`：验证摘要已更新为 Round 9，Rust 124 tests / frontend 35 tests，并明确新增覆盖 flush failure blocks export、raw cursor conflict error UI。

### 20.2 Critical Findings

本轮未发现 Critical。

没有发现以下阻塞性问题：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- 未发现新增磁盘 IO、JSON 读写、timeline build、Tauri emit、export 等重操作进入 SCK capture callback。
- 未发现新增明显 use-after-free、CVPixelBuffer unlock 后继续读 base address、CoreGraphics event 未释放等硬性内存安全问题。
- 未发现 AudioBufferList retained `block_buffer` 新增泄漏路径。
- 未发现 `CursorMetadataRuntime` samples/clicks 退化为无界增长。
- 未发现 `MacRecordingService.stop()` 重新出现 sidecar 写入失败后跳过 cleanup / 状态机悬挂的问题。

### 20.3 Important Findings

#### Important 1: `handleBack()` 仍在配置 flush 失败后继续返回录制页，可能让下一次录制沿用旧后端配置

位置：

- `src/components/preview-view.tsx:177-184`
- `src/components/preview-view.tsx:128-138`
- `src-tauri/src/lib.rs:138-160`
- `src-tauri/src/lib.rs:351-361`

现象：

Round 9 的 export 路径已经修复：`flushPendingConfig()` reject 后不会继续 `exportVideo()`。但返回录制路径仍然在 flush reject 后直接调用 `onBack()`：

```ts
const handleBack = () => {
  void flushPendingConfig()
    .then(() => onBack())
    .catch((error) => {
      console.error('保存美化配置失败', error)
      onBack()
    })
}
```

这意味着：

1. 用户在 Preview 中切换 `cursorMagnification` / `cursorSmoothing`。
2. 前端 UI 已经显示最新状态，`pendingConfigRef.current` 保存最新意图。
3. 用户立即点击“返回录制”。
4. `flushPendingConfig()` 触发 `setBeautifyConfig(latestConfig)`。
5. Tauri invoke reject，或后端 `set_beautify_config` 返回错误。
6. `handleBack()` catch 后仍执行 `onBack()`。
7. 用户进入录制页并再次开始录制。
8. `start_recording()` 会根据 Rust 后端中的旧 `beautify_config` 重新推导 `show_system_cursor`，而不是根据用户最后看到的 Preview UI 状态。

为什么重要：

- 方案 B 的安全契约依赖“当前 Preview config 作为下一次录制/当前导出的真实意图”。导出路径已经保证 flush 失败不继续，但返回路径仍保留 stale backend config 风险。
- `start_recording()` 当前会重新根据后端 `beautify_config` 推导 `CaptureConfig.show_system_cursor`。如果 flush 失败后仍返回，下一次录制的 raw cursor 可见性可能与用户以为保存成功的设置不一致。
- 这不是捕获 callback 热路径问题，但属于 Preview -> Recording 主链路一致性问题。它会影响双光标策略的前置事实：raw cursor 是否录入。
- 若用户以为已经打开光标美化，但后端仍是旧的“关闭美化”配置，下一次录制可能继续录入系统光标；反之也可能隐藏系统光标并依赖后续 overlay。

建议修复：

1. `handleBack()` 在 `flushPendingConfig()` reject 时不要直接 `onBack()`。
2. 推荐行为：留在 Preview，并设置可见错误，例如：

```ts
const handleBack = () => {
  void flushPendingConfig()
    .then(() => onBack())
    .catch((error) => {
      const msg = String(error)
      console.error('保存美化配置失败', error)
      setBeautifyError(msg.includes('已录入系统光标')
        ? msg
        : '美化配置保存失败，请重试或恢复设置后再返回录制。')
    })
}
```

3. 若产品上必须允许返回，则需要显式提供“放弃未保存设置并返回”的二次确认，而不是失败后静默返回。
4. 新增前端回归测试：
   - mock `set_beautify_config` reject。
   - 在 Preview 中切换开关后立即点击“返回录制”。
   - 断言页面仍停留在 Preview。
   - 断言错误信息可见。
   - 断言不会进入 idle/recording panel。
5. 可选加强：为 `set_beautify_config` 加 frontend monotonic sequence 或 request id，后端忽略过期配置写入。当前 promise chain 已缓解顺序问题，但 sequence 能防止未来新入口绕过 chain。

### 20.4 Minor Findings

#### Minor 1: `HANDOFF.md` 中 Phase 4 验证数据仍为旧值

位置：

- `HANDOFF.md:100-106`

现象：

`HANDOFF.md` 的 Phase 4 记录仍写：

```markdown
- `cargo test --manifest-path src-tauri/Cargo.toml` **103 tests** 通过
- `cargo clippy --all-targets` 无 error（22 pre-existing SCK FFI warnings）
- `npm test -- --run` **25 tests** 通过
```

当前实际验证为：

- Rust tests：**124 tests**
- Frontend tests：**35 tests**
- clippy/build warnings：**21 warnings**

为什么重要：

- 仓库约定中 `HANDOFF.md` 是开发任务进度、开发细节记录、多轮对话交接的源文件。
- 后续 agent 或人工接手时，会误以为 Phase 4 仍停留在早期测试数量和旧 warning 状态。

建议修复：

1. 更新 `HANDOFF.md` Phase 4 验证结果为当前事实。
2. 保留剩余 gate：
   - `npm run tauri dev` 手动验证真实 macOS 录制、光标元数据、点击采集、时间线 JSON、双光标策略。
   - `platform/macos/cursor_source.rs` CoreGraphics FFI Native Safety Gate 人工逐行审查。
   - `platform/macos/screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList Native Safety Gate 人工逐行审查。
   - Phase 6 接入生产 FFmpeg compositor 后做真实视频光标平滑和点击放大人工验收。

### 20.5 Phase 4 完整性复审结论

已完成或基本完成：

- `CursorSample` / `CursorClick` / `CursorFrame` / `CursorClickEffect` / `EffectTimeline` serde 模型已落地。
- `EffectTimeline` 已包含 `raw_system_cursor_visible` / `render_cursor_overlay`，为 Phase 6 compositor 提供明确渲染 contract。
- `BeautifyConfigSnapshot` 已持久化 recording-time raw cursor fact，其中 `raw_system_cursor_visible` 是不可变安全约束。
- `CursorProcessor` trait 边界已落地。
- 移动平均平滑、Bezier 插值、点击放大状态机和 `CursorEffectEngine` 已落地并有算法测试。
- 录制期 cursor metadata 采集运行在独立 `CursorMetadataRuntime` 线程，不进入 SCK capture callback。
- `CursorMetadataRecorder` samples/clicks 均 bounded，避免长录制无界增长。
- 停止录制后写入 cursor metadata sidecar，且 sidecar 写入失败不再跳过 cleanup。
- `RecordingMetadata` 已保存 fps、duration、cursor samples、cursor clicks、beautify snapshot。
- `build_cursor_effect_timeline()` 已采用方案 B：metadata raw fact + 当前 Preview export intent。
- raw cursor visible + overlay requested 会被拒绝，避免双光标。
- raw cursor visible + overlay not requested 会生成 no-op/empty timeline，交给原始素材中的系统光标。
- raw cursor hidden + overlay not requested 会生成 baseline neutral cursor overlay frames，避免无光标导出。
- raw cursor hidden + overlay requested 会生成 overlay frames 和 click effects。
- `set_beautify_config`、`get_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- Preview 已接入 beautify config、timeline build 和 export command。
- Preview config writes 已通过 promise chain 串行化，避免 A/B 写入乱序完成。
- 导出路径会等待 pending/in-flight config writes；flush 失败后不会继续 export。
- raw cursor conflict 在 build/export 失败时有可见 UI 反馈，并有前端测试覆盖。
- `build_cursor_effect_timeline` 使用 `spawn_blocking`，并拒绝 `Recording | Paused | Processing` 状态。
- `session_id + metadata_path + beautify_revision` guard 可以阻止跨 session / 旧 revision stale build 写回 service。
- stale build guard 失败后会尝试清理未引用 temp timeline JSON。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`。
- `start_recording()` 会重新根据当前后端 beautify config 推导 `show_system_cursor`，降低 `set_capture_mode()` 调用顺序风险。
- PTS invalid/negative/special flag 检查已加强，PTS delta 使用 `i128`，CMTime conversion 使用 checked integer arithmetic。
- React 未接收视频帧、音频帧或 cursor sample stream，符合架构红线。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

仍未完全完成或需作为下一轮整改/gate：

- `handleBack()` flush reject 后仍会直接返回录制页，可能导致下一次录制使用旧后端美化配置。
- `HANDOFF.md` Phase 4 验证数据仍需同步到当前结果。
- 真实 macOS 录制中 CMSampleBuffer PTS 与 click/video 动作对齐仍需 `npm run tauri dev` 人工验证。
- `cursor_source.rs` CoreGraphics FFI 与 `screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList 仍需 Native Safety Gate 人工逐行审查。
- 双光标策略和 raw-hidden baseline overlay 仍需真实素材人工确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor 接入。

结论：

Phase 4 作为“光标效果 foundation + metadata/timeline/export boundary”已经基本完整，且 Round 9 的 export 主路径一致性问题已经修复。当前不建议直接合并的原因不再是 export，而是返回录制路径仍存在 stale backend config 风险。修复 `handleBack()` 失败路径并同步 `HANDOFF.md` 后，可以进入下一轮合并前复审；Native Safety Gate 和真实 macOS 视觉检查仍需保留为人工 gate。

### 20.6 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- SCK callback 当前仍只做 CMSampleBuffer PTS 提取、CVPixelBuffer 数据复制、AudioBufferList 读取/PCM 转换、bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内使用 `Mutex<Option<(u64, u64)>>`，但只保护轻量时间戳映射；没有等待 UI、磁盘、timeline build 或 export。
- Cursor polling 由 `CursorMetadataRuntime` 独立线程执行，不在 SCK callback 内调用 CoreGraphics cursor source。
- Timeline build 是录后 Tauri command 路径，并使用 `spawn_blocking`。
- `MacScreenCapture::start_stream()` / `stop()` 中等待 SCK start/stop completion 的同步等待发生在 recording service 的 blocking 调用路径，不在 SCK callback 内。
- 本轮未发现新增阻塞捕获主链路的问题。

内存安全：

- `platform/macos/cursor_source.rs` 中 `CGEventCreate` 返回 event 后做 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；静态复审表面配对正确，仍需人工 Native Safety Gate。
- `platform/macos/screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取，并复制到 `Arc<[u8]>`；unlock 后不再引用原始 base address。
- Audio `block_buffer` 当前 return 分支均有 `cf_release`；未发现新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded。
- Timeline 构建仍一次性读取 metadata、生成 frames、pretty serialize JSON；这是录后内存峰值风险，不影响捕获主链路。Phase 6 接入真实长素材后仍需压力测试。
- stale/cancelled build 会尝试删除未引用 temp JSON；资源清理较 Round 8/9 改善。

线程安全：

- Rust 侧共享 service/config/tick/mic runtime 状态使用 `Arc<Mutex<...>>`，beautify revision 使用 `AtomicU64`；未发现 Rust 数据竞争。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path + beautify_revision` stale guard 可以阻止旧 session / 旧 revision build 写回 service。
- 前端 promise chain 已能保证多次 `setBeautifyConfig()` 按用户意图顺序落到后端。
- export 路径已能正确传播 flush 失败，不会继续使用 stale backend config 导出。
- 剩余线程/异步一致性风险集中在 `handleBack()`：flush 失败后仍继续导航，可能让下一次录制从旧后端 config 开始。

资源释放路径：

- `MacRecordingService.stop()` 当前会先停止 cursor runtime，再 stop native capture / mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点。
- Preview unmount cleanup 会清理 debounce timer，并尝试 fire-and-forget 写入 pending config；这是 best-effort cleanup，不应作为主路径一致性保证。
- 导出主路径已由 `flushPendingConfig()` 正确传播失败来保证一致性；返回录制主路径仍需同样处理。

### 20.7 BUG.md 预防规则检查

执行命令：

```bash
rg -n 'data-tauri-drag-region="false"|whileTap|motion\.div' src src-tauri
```

命中要点：

```text
src/App.test.tsx:431:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/App.test.tsx:443:    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/components/preview-view.tsx:197:    <motion.div
src/components/preview-view.tsx:241:              <motion.div
src/components/preview-view.tsx:338:                <motion.div
src/components/preview-view.tsx:361:                </motion.div>
src/components/preview-view.tsx:398:                <motion.div
src/components/preview-view.tsx:423:                </motion.div>
src/components/preview-view.tsx:470:    </motion.div>
src/components/recording-status-bar.tsx:30:    <motion.div
src/components/recording-status-bar.tsx:39:        <motion.div
src/components/recording-status-bar.tsx:50:            <motion.div
src/components/recording-status-bar.tsx:57:        </motion.div>
src/components/recording-status-bar.tsx:74:              <motion.div
src/components/recording-status-bar.tsx:109:    </motion.div>
src/components/recording-panel.tsx:65:    <motion.div
src/components/recording-panel.tsx:89:              whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:155:            whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:171:            whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:185:                  <motion.div
src/components/recording-panel.tsx:208:    </motion.div>
src/components/processing-view.tsx:10:    <motion.div
src/components/processing-view.tsx:17:        <motion.div
src/components/processing-view.tsx:22:        </motion.div>
src/components/processing-view.tsx:25:    </motion.div>
src/components/error-view.tsx:13:    <motion.div
src/components/error-view.tsx:37:    </motion.div>
```

解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443` 是测试断言不存在 `data-tauri-drag-region="false"`，不是源码新增 wrapper。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171` 是 `motion.button whileTap`，`whileTap` 位于交互元素自身，不是 BUG-003 中的 `motion.div whileTap` 作为 Button 直接父容器拦截模式。
- 多处 `motion.div` 用于页面/装饰动画，没有发现 `motion.div` 带 `whileTap` 并直接包裹交互 Button。

结论：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

### 20.8 本轮实际验证

本轮实际执行：

```bash
git diff --check HEAD
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n 'data-tauri-drag-region="false"|whileTap|motion\.div' src src-tauri
```

结果：

- `git diff --check HEAD`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **124 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, **21 warnings**（既有 macOS FFI naming / unused unsafe / dead_code 类 warning）
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, **21 warnings**
- `npm run build`: PASS
- `npm test -- --run`: PASS, **35 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- BUG.md 规则扫描：未发现新增违规 pattern

补充说明：

- 本轮没有运行 `npm run tauri dev`，真实 macOS 录制、click/video 对齐、双光标视觉检查、Native Safety Gate 仍需人工验证。
- 当前 Rust warnings 主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮未发现 clippy error。

### 20.9 Round 10 Ready To Merge?

**Ready to merge: With fixes / No**

本轮未发现 Critical，也未发现会直接阻塞捕获主链路的新增 Phase 4 代码。Round 9 的主要整改已经有效：

1. `flushPendingConfig()` 不再吞掉 config write 失败。
2. export 路径会等待 pending/in-flight config 写入。
3. config flush reject 时不会继续 `exportVideo()`。
4. raw cursor conflict 错误有可见 UI 和前端回归测试。
5. raw hidden baseline contract 测试已断言 `timeline.frames` 非空。
6. checklist 已同步到 Rust 124 tests / frontend 35 tests。

但建议在最终合并前继续完成以下整改：

1. **修复 `handleBack()` 失败路径**：配置写入失败时不要静默返回录制页；应展示可见错误并留在 Preview，或提供明确的“放弃未保存设置”交互。
2. **补前端回归测试**：`set_beautify_config` reject 后点击“返回录制”不应调用 `onBack()`，页面应显示错误并留在 Preview。
3. **同步 `HANDOFF.md`**：更新 Phase 4 验证结果为 Rust 124 tests、frontend 35 tests、clippy/build 21 warnings，并保留人工 gate。
4. **继续保留人工 gate**：真实 macOS click/video 对齐、CoreGraphics/SCK FFI Native Safety、双光标和 baseline overlay 视觉效果仍需人工验证。

## 21. Round 11 整改复审（2026-05-29）

> 评审背景：用户已完成 `## 20. Round 10 整改复审（2026-05-28）` 中的整改任务，本轮复审当前工作区未提交改动，重点确认 Round 10 的 `handleBack()` 失败路径和 `HANDOFF.md` 同步是否闭环，并继续专项检查 Phase 4 是否完整完成、是否存在阻塞捕获主链路、内存安全、线程安全和资源释放路径异常。
>
> 本轮输入：
>
> - `HANDOFF.md`
> - `BUG.md`
> - `.codex/rules/0-global.md` ~ `.codex/rules/5-docs.md`
> - `docs/architecture/project-architecture-and-overall-planning.md`
> - `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md`
> - `docs/superpowers/reviews/2026-05-27-phase-4-code-review.md`
> - `tests/phase-4-w7-w8-checklist.md`
> - 当前 Phase 4 相关代码与未提交 Round 10 整改
>
> 当前结论：**With fixes / No**，暂不建议直接合并。本轮未发现 Critical，也未发现 cursor smoothing / Bezier interpolation / click magnification 进入 ScreenCaptureKit 捕获 callback。Round 10 的 `handleBack()` 阻塞项已经修复，`HANDOFF.md` 验证数据也已同步到 Rust 124 tests / frontend 36 tests。剩余 2 个 Important：raw cursor 已隐藏但 cursor metadata 采样完全失败时，后续可能成功生成“需要渲染 overlay 但没有 frames”的时间线；Preview/export 普通失败只写 console，用户不可见。
>
> 交叉验证：本轮按 `$superpowers:requesting-code-review` 流程启动独立审查代理 `Harvey`。独立审查结论与本地主审一致：无 Critical；Round 10 主问题已修复；核心剩余 Important 是 raw-hidden empty cursor metadata 会造成 cursorless export 风险，以及 generic Preview/export 错误缺少可见反馈。独立审查同时指出 checklist 对后处理内存峰值的表述偏乐观。

### 21.1 本轮重点确认结果

Round 10 整改已闭环项：

- `src/components/preview-view.tsx:179-189`：`handleBack()` 现在会先等待 `flushPendingConfig()`；如果 flush reject，不再调用 `onBack()`，而是留在 Preview 并设置 `beautifyError`。
- `src/App.test.tsx:1319-1355`：新增 `stays on preview when handleBack flush fails and shows error` 回归测试，覆盖“切换配置后立即返回录制、后端写入失败、页面仍停留 Preview 且错误可见”的场景。
- `HANDOFF.md:100-107`：Phase 4 验证结果已同步为 Rust **124 tests**、frontend **36 tests**、clippy/build **21 warnings**。
- `tests/phase-4-w7-w8-checklist.md:66-73`：Verification Summary 已同步到 Round 10，记录 frontend **36 tests**，并说明新增 handleBack 失败路径测试。

本轮未发现的问题：

- 未发现新的 Critical。
- 未发现 cursor smoothing / Bezier interpolation / click magnification 进入 SCK callback。
- 未发现新增磁盘 IO、JSON 读写、timeline build、Tauri invoke/export 等重操作进入捕获 callback。
- 未发现新增 Rust 数据竞争、明显 use-after-free 或释放路径提前中断。
- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button 的 BUG-003 模式。

### 21.2 Strengths

1. Round 10 的返回录制一致性问题已修复

   - `handleBack()` 已与 `handleExport()` 一样等待 pending / in-flight config writes。
   - flush 失败不会静默返回 idle/recording 入口，降低下一次录制沿用 stale backend config 的风险。
   - 前端回归测试覆盖了该路径，不再只靠人工观察。

2. Phase 4 核心架构边界仍然清楚

   - cursor metadata 采集在 `CursorMetadataRuntime` 独立线程，不在 ScreenCaptureKit callback 中调用 CoreGraphics cursor source。
   - smoothing、Bezier interpolation、click magnification 均在录后 `build_cursor_effect_timeline` / `export_video` command 路径中执行。
   - `build_cursor_effect_timeline` 使用 `spawn_blocking`，避免在 async command executor 中直接做完整 JSON 读写和 timeline 构建。

3. raw cursor / overlay contract 比早期版本稳固

   - `BeautifyConfigSnapshot.raw_system_cursor_visible` 作为 recording-time 不可变事实写入 metadata。
   - `build_effect_timeline_from_metadata()` 使用 metadata raw fact + 当前 Preview export intent 的方案 B。
   - raw cursor visible + overlay requested 会返回错误，避免双光标。
   - raw cursor hidden + overlay disabled 会生成 baseline neutral cursor overlay frames，避免用户关闭“美化效果”后素材完全无光标。

4. 线程与资源释放路径较前几轮明显改善

   - `MacRecordingService.stop()` 会先停止 cursor runtime，再 stop native capture / mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
   - cursor sidecar 写入失败已改成收集错误并继续 cleanup。
   - stale timeline build guard 失败后会尝试删除未引用 temp timeline JSON。
   - `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免 SCK 异步 stop 未回调时提前 drop stream/delegate。

5. 自动化验证数据已重新同步

   - Rust tests 当前为 **124 passed**。
   - 前端 tests 当前为 **36 passed**。
   - checklist 与 HANDOFF 已反映 Round 10 后的测试数量。

### 21.3 Critical Findings

本轮未发现 Critical。

没有发现以下阻塞性问题：

- 没有发现 Phase 4 cursor 算法直接阻塞捕获 callback。
- 没有发现明显 use-after-free。
- 没有发现 sidecar 写入失败跳过 capture/mic/consumer cleanup 的旧问题复发。
- 没有发现 export/back 主路径继续吞掉 config write 失败。

### 21.4 Important Findings

#### Important 1: raw cursor 隐藏后若 cursor metadata 采样完全失败，可能成功生成无光标 timeline

位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:165-171`
- `src-tauri/src/lib.rs:147-160`
- `src-tauri/src/lib.rs:390-436`
- `src-tauri/src/media/cursor_engine.rs:403-411`

现象：

`CursorMetadataRuntime` 的 polling loop 当前对 `source.snapshot()` 失败使用 `if let Ok(snapshot)` 静默忽略：

```rust
if let Ok(snapshot) = source.snapshot() {
    recorder.record_snapshot(
        MediaTimestamp::from_nanos(session_clock.elapsed_nanos()),
        snapshot,
    );
}
```

如果 CoreGraphics cursor snapshot 在整段录制中持续失败，最终 metadata 仍会被 `recorder.finish()` 写出，但 `cursor_samples` 为空。

同时，`start_recording()` 会根据当前 beautify config 推导 `show_system_cursor`：

```rust
let expected_show_system_cursor =
    !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);
```

默认 beautify config 是 cursor magnification / smoothing 开启，因此默认 raw system cursor 会被隐藏。也就是说，一旦 cursor polling 完全失败，就可能出现：

1. 原始 SCK 素材里没有系统光标。
2. cursor metadata sidecar 存在，但 `cursor_samples` 为空。
3. `build_effect_timeline_from_metadata()` 进入 raw hidden 分支。
4. `CursorEffectEngine::build_timeline()` 对 empty samples 返回 `frames = []`、`render_cursor_overlay = true`。
5. command 返回成功，Phase 6 compositor 看到“应渲染 overlay”但没有任何 cursor frames。

为什么重要：

- 这是 Phase 4 raw cursor policy 的核心正确性漏洞：默认开启美化时 raw cursor 被隐藏，overlay 就成为唯一光标来源。
- 如果 empty metadata 被当作成功，后续导出可能既没有系统光标，也没有 overlay 光标，形成 cursorless export。
- 用户不会看到错误；从 command 层看 timeline build 是成功的。
- 这不是捕获 callback 热路径阻塞问题，但会直接破坏 Phase 4 的“隐藏系统光标防双光标 + 后续 overlay 补回光标”的安全契约。

建议修复：

1. 在 metadata 中记录 cursor polling health，例如：
   - `cursor_sample_count`
   - `cursor_snapshot_error_count`
   - `cursor_polling_failed` / `cursor_capture_available`
   - 或至少在 runtime finish 时能判断是否曾成功采样。
2. 在 `build_effect_timeline_from_metadata()` 中增加 guard：
   - 当 `raw_system_cursor_visible == false` 且 `metadata.cursor_samples.is_empty()` 时，返回明确错误。
   - 错误文案建议可见给用户，例如：`本次素材未录入系统光标，且光标元数据为空，无法生成光标时间线。请重新录制或关闭光标美化。`
3. 或者在录制开始前使用更保守策略：
   - 只有 cursor metadata source 首次 health check 成功后才允许隐藏 system cursor。
   - 如果 health check 失败，保持 `show_system_cursor = true` 并禁用 overlay，避免 cursorless export。
4. 增加 Rust 回归测试：
   - `raw_hidden_empty_cursor_samples_returns_error`
   - `raw_visible_empty_cursor_samples_generates_empty_noop_timeline`
   - 可选：`cursor_runtime_records_snapshot_failures`
5. checklist 中增加 manual/auto gate：
   - “raw cursor hidden 时 cursor metadata 为空必须 fail closed，而不是成功生成 empty overlay timeline。”

建议优先级：

- 合并前建议修复。该问题会影响真实素材导出的光标可见性，且默认配置下更容易触发。

#### Important 2: Preview/export 普通失败只写 console，用户不可见

位置：

- `src/components/preview-view.tsx:157-163`
- `src/components/preview-view.tsx:167-176`
- `src/components/preview-view.tsx:179-188`

现象：

当前 `handleBeautifyChange()` 和 `handleExport()` 的 catch 分支只在错误文本包含 `已录入系统光标` 时设置 `beautifyError`：

```ts
if (msg.includes('已录入系统光标')) {
  setBeautifyError(msg)
}
```

因此以下错误只会进入 `console.error`，不会显示在 UI：

- `没有可用的光标元数据，请先完成一次录制`
- `光标效果时间线构建任务失败`
- effect timeline JSON 写入失败
- metadata JSON 读取失败 / parse 失败
- `美化配置锁已损坏`
- Tauri invoke 网络/桥接层失败
- 非 raw-cursor 的 `setBeautifyConfig()` reject

Round 10 的 `handleBack()` 已经显示 generic 保存失败错误，但 `handleBeautifyChange()` 与 `handleExport()` 仍没有 generic 用户反馈。

为什么重要：

- 用户点击导出后，如果后端失败但错误不是 raw cursor conflict，会看到界面没有变化，只能从开发者 console 里知道原因。
- export 主路径虽然不会继续使用 stale config，但失败不可见会让用户误以为按钮无响应。
- Phase 4 目标包含“Preview 接入 beautify/export command boundary”，命令失败路径应有最小可见反馈，否则 UI 闭环不完整。
- 后续 Phase 6 接入真实 FFmpeg compositor 后，更多 export failure 会走普通错误路径；现在不处理会放大用户体验问题。

建议修复：

1. 提取统一错误展示 helper，例如：

```ts
const messageForBeautifyError = (error: unknown, fallback: string) => {
  const msg = String(error)
  return msg.includes('已录入系统光标') ? msg : fallback
}
```

2. `handleBeautifyChange()` catch：
   - raw cursor conflict 显示原文。
   - generic timeline/config failure 显示：`光标效果处理失败，请重试或重新录制。`
3. `handleExport()` catch：
   - raw cursor conflict 显示原文。
   - generic export failure 显示：`导出失败，请重试或检查录制素材。`
4. `getBeautifyConfig()` 初始化失败可以考虑显示轻量错误或保留 console；这不是当前阻塞项。
5. 增加前端回归测试：
   - `build_cursor_effect_timeline` reject generic error 后，UI 显示 generic 处理失败。
   - `export_video` reject generic error 后，UI 显示 generic 导出失败。
   - `set_beautify_config` reject generic error 后，export 不继续且 UI 显示 generic 保存/导出失败。

建议优先级：

- 合并前建议修复。它不影响捕获主链路安全，但影响 Phase 4 command/UI path 的可恢复性和可诊断性。

### 21.5 Minor Findings

#### Minor 1: checklist 对后处理内存峰值的表述偏乐观

位置：

- `tests/phase-4-w7-w8-checklist.md:43-45`
- `src-tauri/src/lib.rs:491-503`
- `src-tauri/src/media/cursor_engine.rs:102-132`
- `src-tauri/src/media/recording_metadata.rs:42-68`

现象：

checklist 当前写：

```markdown
- [x] 1080p 素材处理过程中内存无无界增长。（`CursorMetadataRecorder` 上限 120k samples）
```

这个描述只能证明“录制期 cursor metadata buffer 有上限”，不能证明“录后 timeline build 内存无无界增长”。当前 timeline build 仍会：

1. `read_metadata()` 一次性读取完整 metadata JSON 到 String。
2. 反序列化完整 `RecordingMetadata`。
3. `CursorEffectEngine` 一次性生成完整 `Vec<CursorFrame>`。
4. `serde_json::to_string_pretty()` 一次性生成完整 timeline JSON。
5. `fs::write()` 一次性写出。

为什么重要：

- 这不是 Phase 4 当前合并的阻塞项，也不阻塞捕获主链路。
- 但长录制素材下，post-process 内存峰值仍与 duration / fps 成正比。
- checklist 过度乐观会误导后续 Phase 6，以为完整后处理内存已经被证明稳定。

建议修复：

1. 将 checklist 文案改为：
   - `录制期 cursor metadata buffer 有上限。`
   - `录后 timeline build 内存峰值仍需长录制压力验证。`
2. 增加 Phase 6/后续 gate：
   - 10 分钟 / 30fps 1080p metadata timeline build 压力测试。
   - 60fps 长素材 JSON size / peak memory 观察。
3. 后续可考虑 streaming writer 或按 chunk 写 timeline，避免 full JSON string 峰值。

### 21.6 Phase 4 完整性复审结论

已完成或基本完成：

- `CursorSample` / `CursorClick` / `CursorFrame` / `CursorClickEffect` / `EffectTimeline` serde 模型已落地。
- `EffectTimeline` 已包含 `raw_system_cursor_visible` / `render_cursor_overlay`，为 Phase 6 compositor 提供明确渲染 contract。
- `BeautifyConfigSnapshot` 已持久化 recording-time raw cursor fact，其中 `raw_system_cursor_visible` 是不可变安全约束。
- `CursorProcessor` trait 边界已落地。
- 移动平均平滑、Bezier 插值、点击放大状态机和 `CursorEffectEngine` 已落地并有算法测试。
- 录制期 cursor metadata 采集运行在独立 `CursorMetadataRuntime` 线程，不进入 SCK capture callback。
- `CursorMetadataRecorder` samples/clicks 均 bounded，避免录制期 cursor metadata 无界增长。
- 停止录制后写入 cursor metadata sidecar，且 sidecar 写入失败不再跳过 cleanup。
- `RecordingMetadata` 已保存 fps、duration、cursor samples、cursor clicks、beautify snapshot。
- `build_cursor_effect_timeline()` 已采用方案 B：metadata raw fact + 当前 Preview export intent。
- raw cursor visible + overlay requested 会被拒绝，避免双光标。
- raw cursor visible + overlay not requested 会生成 no-op/empty timeline，交给原始素材中的系统光标。
- raw cursor hidden + overlay not requested 会生成 baseline neutral cursor overlay frames，避免用户关闭美化效果后素材完全无光标。
- raw cursor hidden + overlay requested 会生成 overlay frames 和 click effects。
- `set_beautify_config`、`get_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- Preview 已接入 beautify config、timeline build 和 export command。
- Preview config writes 已通过 promise chain 串行化，避免 A/B 写入乱序完成。
- 导出路径会等待 pending/in-flight config writes；flush 失败后不会继续 export。
- 返回录制路径也会等待 pending/in-flight config writes；flush 失败后会留在 Preview 并显示错误。
- raw cursor conflict 在 build/export/back 失败时有可见 UI 反馈，并有前端测试覆盖。
- `build_cursor_effect_timeline` 使用 `spawn_blocking`，并拒绝 `Recording | Paused | Processing` 状态。
- `session_id + metadata_path + beautify_revision` guard 可以阻止跨 session / 旧 revision stale build 写回 service。
- stale build guard 失败后会尝试清理未引用 temp timeline JSON。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`。
- `start_recording()` 会重新根据当前后端 beautify config 推导 `show_system_cursor`，降低 `set_capture_mode()` 调用顺序风险。
- PTS invalid/negative/special flag 检查已加强，PTS delta 使用 `i128`，CMTime conversion 使用 checked integer arithmetic。
- React 未接收视频帧、音频帧或 cursor sample stream，符合架构红线。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。
- `HANDOFF.md` 与 `tests/phase-4-w7-w8-checklist.md` 已同步 Round 10 后自动化验证数量。

仍未完全完成或需作为下一轮整改/gate：

- raw system cursor hidden 时，empty cursor metadata 应 fail closed；当前会返回 successful empty overlay timeline。
- Preview/export generic error 需要可见 UI 反馈；当前只有 raw cursor conflict 被显示。
- checklist 对 post-process 内存峰值的表述需收紧，避免把 metadata buffer bounded 等同于 timeline build 内存已验证。
- 真实 macOS 录制中 CMSampleBuffer PTS 与 click/video 动作对齐仍需 `npm run tauri dev` 人工验证。
- `cursor_source.rs` CoreGraphics FFI 与 `screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList 仍需 Native Safety Gate 人工逐行审查。
- 双光标策略和 raw-hidden baseline overlay 仍需真实素材人工确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor 接入。

结论：

Phase 4 作为“光标效果 foundation + metadata/timeline/export boundary”已经基本完整，Round 10 的返回路径一致性问题也已修复。当前不建议直接合并的原因转移到两个更深的失败路径：raw cursor hidden + empty metadata 的 cursorless export 风险，以及普通 Preview/export 失败不可见。修复这两项并补充回归测试后，可以进入下一轮合并前复审；Native Safety Gate 和真实 macOS 视觉检查仍需保留为人工 gate。

### 21.7 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- SCK callback 当前仍只做 CMSampleBuffer PTS 提取、CVPixelBuffer 数据复制、AudioBufferList 读取/PCM 转换、bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内使用 `Mutex<Option<(u64, u64)>>`，但只保护轻量时间戳映射；没有等待 UI、磁盘、timeline build 或 export。
- Cursor polling 由 `CursorMetadataRuntime` 独立线程执行，不在 SCK callback 内调用 CoreGraphics cursor source。
- Timeline build 是录后 Tauri command 路径，并使用 `spawn_blocking`。
- `MacScreenCapture::start_stream()` / `stop()` 中等待 SCK start/stop completion 的同步等待发生在 recording service 的 blocking 调用路径，不在 SCK callback 内。
- 本轮未发现新增阻塞捕获主链路的问题。

内存安全：

- `platform/macos/cursor_source.rs` 中 `CGEventCreate` 返回 event 后做 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；静态复审表面配对正确，仍需人工 Native Safety Gate。
- `platform/macos/screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取，并复制到 `Arc<[u8]>`；unlock 后不再引用原始 base address。
- Audio `block_buffer` 当前 return 分支均有 `cf_release`；未发现新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded。
- `CursorMetadataRuntime` 静默忽略 snapshot failure 不属于内存安全问题，但会导致 raw-hidden empty metadata 正确性问题，见 Important 1。
- Timeline 构建仍一次性读取 metadata、生成 frames、pretty serialize JSON；这是录后内存峰值风险，不影响捕获主链路。Phase 6 接入真实长素材后仍需压力测试。
- stale/cancelled build 会尝试删除未引用 temp JSON；资源清理较 Round 8/9 改善。

线程安全：

- Rust 侧共享 service/config/tick/mic runtime 状态使用 `Arc<Mutex<...>>`，beautify revision 使用 `AtomicU64`；未发现 Rust 数据竞争。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path + beautify_revision` stale guard 可以阻止旧 session / 旧 revision build 写回 service。
- 前端 promise chain 已能保证多次 `setBeautifyConfig()` 按用户意图顺序落到后端。
- export 和 back 路径已能正确传播 flush 失败，不会继续使用 stale backend config 导出或进入下一次录制入口。
- `CursorMetadataRuntime::stop()` 仍是直接 join polling thread；若 `source.snapshot()` 在系统 API 层异常长时间阻塞，stop 会等待。当前 CoreGraphics 调用预计很短，风险较低，仍建议人工 Native Safety Gate 关注。

资源释放路径：

- `MacRecordingService.stop()` 当前会先停止 cursor runtime，再 stop native capture / mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点。
- Preview unmount cleanup 会清理 debounce timer，并尝试 fire-and-forget 写入 pending config；这是 best-effort cleanup，不作为 export/back 主路径一致性保证。
- 导出和返回录制主路径已由 `flushPendingConfig()` 正确传播失败来保证一致性。

### 21.8 BUG.md 预防规则检查

执行命令：

```bash
rg -n "whileTap|motion\\.div|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents" src src-tauri/src src-tauri/capabilities src-tauri/tauri.conf.json
```

命中要点：

```text
src/App.test.tsx:431: expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/App.test.tsx:443: expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/components/recording-panel.tsx:89: whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:155: whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:171: whileTap={{ scale: 0.98 }}
src/components/preview-view.tsx:202: <motion.div
src/components/preview-view.tsx:246: <motion.div
src/components/preview-view.tsx:343: <motion.div
src/components/preview-view.tsx:403: <motion.div
src/components/recording-status-bar.tsx:30: <motion.div
src/components/processing-view.tsx:10: <motion.div
src/components/error-view.tsx:13: <motion.div
```

解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443` 是测试断言不存在 `data-tauri-drag-region="false"`，不是源码新增 wrapper。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171` 是 `motion.button whileTap`，`whileTap` 位于交互元素自身，不是 BUG-003 中的 `motion.div whileTap` 作为 Button 直接父容器拦截模式。
- 多处 `motion.div` 用于页面/装饰动画，没有发现 `motion.div` 带 `whileTap` 并直接包裹交互 Button。
- 未发现新增 `setIgnoreCursorEvents(true)` 或全窗口鼠标忽略逻辑。

结论：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

### 21.9 本轮实际验证

本轮实际执行：

```bash
git diff --check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n "whileTap|motion\\.div|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents" src src-tauri/src src-tauri/capabilities src-tauri/tauri.conf.json
```

结果：

- `git diff --check`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **124 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, **21 warnings**（既有 macOS FFI naming / unused unsafe / dead_code 类 warning）
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, **21 warnings**
- `npm run build`: PASS
- `npm test -- --run`: PASS, **36 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- BUG.md 规则扫描：未发现新增违规 pattern

补充说明：

- 本轮没有运行 `npm run tauri dev`，真实 macOS 录制、click/video 对齐、双光标视觉检查、Native Safety Gate 仍需人工验证。
- 当前 Rust warnings 主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮未发现 clippy error。

### 21.10 Round 11 Ready To Merge?

**Ready to merge: With fixes / No**

本轮未发现 Critical，也未发现会直接阻塞捕获主链路的新增 Phase 4 代码。Round 10 的主要整改已经有效：

1. `handleBack()` flush reject 后不再继续返回录制页。
2. `handleBack()` generic failure 会显示可见错误。
3. 新增前端回归测试覆盖“返回录制失败留在 Preview”。
4. `HANDOFF.md` 已同步 Rust 124 tests / frontend 36 tests / 21 warnings。
5. checklist 已同步 Round 10 验证摘要。

但建议在最终合并前继续完成以下整改：

1. **raw-hidden empty metadata fail closed**：当 `raw_system_cursor_visible == false` 且 `cursor_samples.is_empty()` 时，`build_effect_timeline_from_metadata()` 应返回错误或录制时保守保留系统光标，避免 cursorless export。
2. **补 Rust 回归测试**：覆盖 raw hidden + empty cursor samples 必须失败；raw visible + empty cursor samples 可以生成 no-op timeline。
3. **补 cursor polling health 记录**：至少记录 snapshot error count / successful sample count，方便后续 UI 与日志诊断。
4. **普通 Preview/export 错误可见化**：`handleBeautifyChange()` 和 `handleExport()` 对非 raw cursor conflict 错误也应设置 generic `beautifyError`。
5. **补前端回归测试**：generic `build_cursor_effect_timeline` reject、generic `export_video` reject、generic `set_beautify_config` reject 均应有可见错误，且 export 不继续。
6. **收紧 checklist 内存描述**：区分“录制期 cursor metadata bounded”和“录后 timeline build 内存峰值仍需长素材压力验证”。
7. **继续保留人工 gate**：真实 macOS click/video 对齐、CoreGraphics/SCK FFI Native Safety、双光标和 baseline overlay 视觉效果仍需人工验证。

## 22. Round 12 整改复审（2026-05-29）

> 评审背景：用户已完成 `## 21. Round 11 整改复审（2026-05-29）` 中的整改任务，本轮复审当前工作区未提交改动，重点确认 Round 11 的 7 条整改建议是否真正闭环，并继续专项检查 Phase 4 是否完整完成、是否存在阻塞捕获主链路、内存安全、线程安全和资源释放路径异常。
>
> 输入与评审依据：
>
> - `docs/architecture/project-architecture-and-overall-planning.md`
> - `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md`
> - `docs/superpowers/reviews/2026-05-27-phase-4-code-review.md`
> - `tests/phase-4-w7-w8-checklist.md`
> - `HANDOFF.md`
> - `BUG.md`
> - `.codex/rules/0-global.md`
> - `.codex/rules/1-coding-style.md`
> - `.codex/rules/2-testing.md`
> - `.codex/rules/3-git-commit.md`
> - `.codex/rules/4-security.md`
> - `.codex/rules/5-docs.md`
> - 当前 Phase 4 相关代码与未提交 Round 11 整改
>
> 当前结论：**With fixes / No**，暂不建议直接合并。本轮未发现 Critical，也未发现 cursor smoothing / Bezier interpolation / click magnification 进入 ScreenCaptureKit 捕获 callback。Round 11 的主要阻塞项已经基本闭环：raw-hidden empty metadata 已 fail closed，generic Preview/export/config failure 已可见，cursor polling health 已写入 metadata，checklist 内存描述也已收紧。剩余 1 个 Important：旧的 `buildCursorEffectTimeline()` stale/cancelled reject 可能在较新的成功构建后覆盖 UI，显示误导性的失败错误。
>
> 交叉验证：本轮按 `$superpowers:requesting-code-review` 流程启动独立审查代理 `Noether`。独立审查结论与本地主审基本一致：无 Critical、无 Important；Round 11 功能性阻塞项已闭环。独立审查额外指出两个 Minor：`HANDOFF.md` 测试数量仍为旧值，以及 cursor health counter 缺少直接 recorder 单元测试。本地主审在此基础上补充发现 stale build UI 误报风险，并将其评为 Important。

### 22.1 本轮重点确认结果

Round 11 建议整改项闭环情况：

1. **raw-hidden empty metadata fail closed**：已完成。
   - `src-tauri/src/lib.rs:390-395`：`build_effect_timeline_from_metadata()` 在 `raw_system_cursor_visible == false` 且 `metadata.cursor_samples.is_empty()` 时返回明确错误。
   - `src-tauri/src/lib.rs:753-768`：新增 `raw_hidden_empty_cursor_samples_returns_error` 回归测试。
   - `src-tauri/src/lib.rs:771-786`：新增 `raw_visible_empty_cursor_samples_generates_empty_noop_timeline` 回归测试，保留 raw-visible 空 metadata 的 no-op 合法路径。

2. **补 Rust 回归测试**：已完成主要合同测试。
   - 已覆盖 raw-hidden empty samples fail closed。
   - 已覆盖 raw-visible empty samples noop。
   - 仍有一个小测试缺口：`CursorMetadataRecorder` health counter increment 没有直接单元测试，见 Minor 2。

3. **补 cursor polling health 记录**：已完成基础字段和 runtime 记录。
   - `src-tauri/src/media/recording_metadata.rs:18-21`：`RecordingMetadata` 新增 `cursor_snapshot_success_count` / `cursor_snapshot_error_count`，并通过 `#[serde(default)]` 保持旧 sidecar 兼容。
   - `src-tauri/src/app/cursor_metadata_runtime.rs:42-43`：recorder 保存成功/失败计数。
   - `src-tauri/src/app/cursor_metadata_runtime.rs:69-70`：成功 snapshot 时递增 success count。
   - `src-tauri/src/app/cursor_metadata_runtime.rs:137-139`：新增 `record_snapshot_failure()`。
   - `src-tauri/src/app/cursor_metadata_runtime.rs:177-186`：runtime 对 `source.snapshot()` 的 `Err(_)` 递增 failure count。
   - `src-tauri/src/app/cursor_metadata_runtime.rs:148-149`：finish 时写入 metadata。

4. **普通 Preview/export 错误可见化**：已完成。
   - `src/components/preview-view.tsx:41-44`：新增 `messageForBeautifyError()`。
   - `src/components/preview-view.tsx:162-165`：beautify/timeline build generic failure 显示 `光标效果处理失败，请重试或重新录制。`。
   - `src/components/preview-view.tsx:172-175`：export generic failure 显示 `导出失败，请重试或检查录制素材。`。
   - `src/components/preview-view.tsx:181-184`：back/flush generic failure 显示 `美化配置保存失败，请重试或恢复设置后再返回录制。`。
   - `src/components/preview-view.tsx:432-435`：错误信息已渲染到 UI，而不是仅写 console。

5. **补前端回归测试**：已完成主要失败路径覆盖。
   - `src/App.test.tsx:1357-1386`：`build_cursor_effect_timeline` generic reject 后 UI 显示 generic 处理失败。
   - `src/App.test.tsx:1388-1419`：`export_video` generic reject 后 UI 显示 generic 导出失败。
   - `src/App.test.tsx:1421-1447`：`set_beautify_config` generic reject 后 UI 显示 generic 处理失败。
   - `src/App.test.tsx:1276-1317`：flush 失败时 export 不继续。
   - `src/App.test.tsx:1319-1355`：back flush 失败时留在 Preview。

6. **收紧 checklist 内存描述**：已完成。
   - `tests/phase-4-w7-w8-checklist.md:45-46`：文案已区分“录制期 cursor metadata buffer 有上限”和“录后 timeline build 内存峰值仍需长录制压力验证”。

7. **继续保留人工 gate**：已保留。
   - `tests/phase-4-w7-w8-checklist.md:63-65`：Native Safety / `showsCursor` / 双光标检查仍未勾选。
   - 本轮仍未运行 `npm run tauri dev`，真实 macOS 录制、click/video 对齐、双光标视觉检查、Native Safety Gate 继续保留为人工 gate。

### 22.2 Strengths

1. Round 11 的两个核心正确性问题已经实质修复

   - raw cursor hidden 后，empty cursor metadata 不再成功生成“需要 overlay 但没有 frames”的 timeline。
   - Preview/export/config 的普通失败路径不再只写 console，用户可以看到最小可恢复提示。

2. raw cursor / overlay contract 更清晰

   - `build_effect_timeline_from_metadata()` 继续集中表达方案 B contract：metadata 中的 `raw_system_cursor_visible` 是录制期不可变事实，当前 Preview config 是导出意图。
   - raw visible + overlay requested 仍被拒绝，避免双光标。
   - raw visible + no overlay 仍生成 empty/no-op timeline，交给原始素材中的系统光标。
   - raw hidden + no overlay 仍生成 baseline neutral cursor overlay frames，避免用户关闭美化效果后素材完全无光标。
   - raw hidden + empty samples 已 fail closed，避免 cursorless export。

3. Preview 写入顺序和失败传播比早期版本稳固

   - `writeChainRef` 可以串行化多次 `setBeautifyConfig()`，避免 A/B 写入乱序完成。
   - `flushPendingConfig()` 不再吞掉失败；导出和返回录制主路径会看到 reject。
   - export flush 失败不再继续调用后端导出。
   - back flush 失败会留在 Preview 并显示错误。

4. 捕获主链路边界仍然符合 Phase 4 架构

   - cursor metadata polling 在 `CursorMetadataRuntime` 独立线程，不在 SCK callback 内调用 CoreGraphics cursor source。
   - smoothing、Bezier interpolation、click magnification 均在录后 command 路径执行。
   - `build_cursor_effect_timeline()` 继续使用 `spawn_blocking`。
   - React 没有接收视频帧、音频帧或 cursor sample stream，只发送 config / command intent。

5. 测试数量与覆盖面继续增加

   - Rust 当前为 126 tests。
   - 前端当前为 39 tests。
   - 新增测试覆盖 raw-hidden empty metadata、raw-visible empty noop、generic build/export/config failure、export flush reject、back flush reject 等关键路径。

### 22.3 Critical Findings

本轮未发现 Critical。

没有发现以下阻塞性问题：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- 未发现新增磁盘 IO、JSON 读写、timeline build、Tauri emit、export 等重操作进入 SCK capture callback。
- 未发现新增明显 use-after-free、CVPixelBuffer unlock 后继续读 base address、CoreGraphics event 未释放等硬性内存安全问题。
- 未发现 `CursorMetadataRuntime` samples/clicks 退化为无界增长。
- 未发现 `MacRecordingService.stop()` 重新出现 sidecar 写入失败后跳过 cleanup / 状态机悬挂的问题。
- 未发现 React 接收媒体帧或 cursor sample stream。

### 22.4 Important Findings

#### Important 1: stale/cancelled timeline build 可能在新构建成功后显示误导性失败错误

位置：

- `src/components/preview-view.tsx:154-165`
- `src/components/preview-view.tsx:432-435`
- `src-tauri/src/lib.rs:547-570`

现象：

`handleBeautifyChange()` 的 debounce callback 每次都会调用：

```ts
void enqueueConfigWrite(nextConfig)
  .then(() => {
    setBeautifyError(null)
    return buildCursorEffectTimeline()
  })
  .catch((error) => {
    console.error('光标效果处理失败', error)
    setBeautifyError(messageForBeautifyError(error, '光标效果处理失败，请重试或重新录制。'))
  })
```

后端为了防止旧构建污染新 session / 新 revision，已经在 `build_cursor_effect_timeline()` 中做了 stale guard：

```rust
if service.current_session_id() == session_id
    && service.last_cursor_metadata_path().as_deref() == Some(metadata_path.as_str())
    && current_revision == build_revision
{
    service.set_last_effect_timeline_path(Some(path.to_string_lossy().to_string()));
} else {
    let _ = std::fs::remove_file(&path);
    let msg = if current_revision != build_revision {
        "光标美化配置已变更，构建已取消".to_string()
    } else {
        "录制会话已变更，光标效果构建已取消".to_string()
    };
    return Err(msg);
}
```

这个后端 reject 本身是正确的：旧 revision 的构建结果必须被取消。但前端当前把所有 reject 都视为用户可见错误，会出现以下竞态：

1. 用户快速调整光标美化配置，触发 Build A。
2. 用户继续调整配置，触发 Build B，并且 B 是最新意图。
3. 后端因为 revision 变化取消 Build A，返回 `光标美化配置已变更，构建已取消`。
4. Build B 随后成功，或者已经成功。
5. Build A 的 catch 仍可能调用 `setBeautifyError('光标效果处理失败，请重试或重新录制。')`。
6. UI 显示失败，但实际最新配置可能已经成功生成 timeline。

为什么重要：

- stale build cancellation 是预期控制流，不应作为用户错误显示。
- 当前错误文案会让用户误以为最新的光标处理失败，降低 Preview 可信度。
- Phase 4 已经把 config write 串行化，但 timeline build 本身仍可能与后续 revision 交错；后端 guard 会安全取消旧 build，前端也需要识别“旧请求返回”。
- 这不是捕获主链路安全问题，但属于 Preview command/UI path 的异步一致性问题，建议合并前修复。

建议修复：

1. 在前端增加 timeline build sequence / generation guard。

   示例方向：

   ```ts
   const buildSeqRef = useRef(0)

   const runLatestTimelineBuild = (): Promise<void> => {
     const seq = ++buildSeqRef.current
     setBeautifyError(null)
     return buildCursorEffectTimeline()
       .then(() => {
         if (seq === buildSeqRef.current) {
           setBeautifyError(null)
         }
       })
       .catch((error) => {
         const msg = String(error)
         const isStaleCancel =
           msg.includes('配置已变更，构建已取消') ||
           msg.includes('录制会话已变更，光标效果构建已取消')
         if (seq !== buildSeqRef.current || isStaleCancel) {
           return
         }
         setBeautifyError(messageForBeautifyError(error, '光标效果处理失败，请重试或重新录制。'))
       })
   }
   ```

2. 或最小修复：`messageForBeautifyError()` / catch 分支对 `构建已取消` 类错误静默处理。

   - 优点：改动小。
   - 缺点：不能覆盖所有旧请求晚到的问题；sequence guard 更稳。

3. 新增前端回归测试：

   - 模拟 Build A reject `光标美化配置已变更，构建已取消`。
   - 模拟 Build B resolve 成功。
   - 断言最终 UI 不显示 `光标效果处理失败`。

4. 再补一个顺序反转测试更稳：

   - Build A pending。
   - Build B resolve 成功并清空错误。
   - Build A 随后 reject stale cancel。
   - 断言 A 的 late reject 不覆盖 B 的成功状态。

建议优先级：

- 合并前建议修复。该问题不会破坏生成文件安全性，但会给用户展示错误事实，并且容易在快速拖动 slider / toggle 时触发。

### 22.5 Minor Findings

#### Minor 1: `HANDOFF.md` Phase 4 验证数量仍是 Round 10 / Round 11 前的旧值

位置：

- `HANDOFF.md:103`
- `HANDOFF.md:107`
- `tests/phase-4-w7-w8-checklist.md:70`
- `tests/phase-4-w7-w8-checklist.md:74`

现象：

`HANDOFF.md` 当前写：

```markdown
- `cargo test --manifest-path src-tauri/Cargo.toml` **124 tests** 通过（+48 相比 Phase 3）
- `npm test -- --run` **36 tests** 通过（+13 相比 Phase 3）
```

但本轮实际验证与 checklist 已经是：

```markdown
- `cargo test --manifest-path src-tauri/Cargo.toml`: **126 tests** PASS
- `npm test -- --run`: **39 tests** PASS
```

为什么重要：

- `HANDOFF.md` 是项目约定中的多轮交接核心文件。
- 后续 agent 或人工接手时会误以为 Round 11 新增测试尚未计入，造成状态漂移。
- 这是文档问题，不是 runtime 风险。

建议修复：

1. 更新 `HANDOFF.md` Phase 4 验证结果为 Rust **126 tests**、frontend **39 tests**。
2. 保留 `cargo clippy` / `cargo build` 的 **21 warnings** 表述。
3. 如需准确描述增量，可同步更新括号中的 “+X 相比 Phase 3”。

#### Minor 2: cursor health counter 缺少直接 recorder 单元测试

位置：

- `src-tauri/src/app/cursor_metadata_runtime.rs:69-70`
- `src-tauri/src/app/cursor_metadata_runtime.rs:137-149`
- `src-tauri/src/app/cursor_metadata_runtime.rs:220-363`

现象：

当前代码已经实现：

- `record_snapshot()` 增加 `snapshot_success_count`。
- `record_snapshot_failure()` 增加 `snapshot_error_count`。
- `finish()` 写入 `RecordingMetadata`。
- `RecordingMetadata` serde round-trip 测试覆盖了字段序列化。

但 `CursorMetadataRecorder` 的测试模块里没有直接断言：

- 调用 `record_snapshot()` 后 `cursor_snapshot_success_count == 1`。
- 调用 `record_snapshot_failure()` 后 `cursor_snapshot_error_count == 1`。

为什么重要：

- health counter 是 Round 11 新增诊断字段，最好用一个小单测锁住。
- 这不是合并阻塞项；字段当前路径简单，且 metadata round-trip 已覆盖结构。
- 但直接测试能防止未来重构 recorder 时漏掉计数递增。

建议修复：

新增一个 recorder 单元测试，例如：

```rust
#[test]
fn recorder_counts_snapshot_successes_and_failures() {
    let mut recorder = CursorMetadataRecorder::new(30, beautify_snapshot(false));
    recorder.record_snapshot(MediaTimestamp::from_nanos(0), snapshot(false));
    recorder.record_snapshot_failure();

    let metadata = recorder.finish(33_333_333);

    assert_eq!(metadata.cursor_snapshot_success_count, 1);
    assert_eq!(metadata.cursor_snapshot_error_count, 1);
}
```

可复用当前测试里的 snapshot 构造逻辑，避免引入新抽象。

#### Minor 3: export 成功后不会清理旧的 `beautifyError`

位置：

- `src/components/preview-view.tsx:169-175`
- `src/components/preview-view.tsx:432-435`

现象：

`handleExport()` 当前只在 catch 中设置错误：

```ts
const handleExport = (preset: ExportPreset) => {
  void flushPendingConfig()
    .then(() => exportVideo(preset))
    .catch((error) => {
      console.error('导出失败', error)
      setBeautifyError(messageForBeautifyError(error, '导出失败，请重试或检查录制素材。'))
    })
}
```

如果用户先遇到一次 export failure，随后重试并成功，旧的 `beautifyError` 仍会留在 UI 中。

为什么重要：

- 用户可能看到“导出失败”但实际第二次导出已经成功。
- 这会降低导出路径的反馈可信度。
- 这是 UI 状态清理问题，不影响捕获主链路或 timeline 文件安全。

建议修复：

1. 在 export 成功后清理错误：

   ```ts
   void flushPendingConfig()
     .then(() => exportVideo(preset))
     .then(() => setBeautifyError(null))
     .catch(...)
   ```

2. 如果希望点击导出时立即移除旧错误，也可以在 `handleExport()` 开始处先 `setBeautifyError(null)`；但若 flush/export 失败，需要 catch 再设置。
3. 增加前端测试：
   - 第一次 `export_video` reject，显示 `导出失败`。
   - 第二次 `export_video` resolve。
   - 断言 `导出失败` 不再显示。

### 22.6 Phase 4 完整性复审结论

已完成或基本完成：

- `CursorSample` / `CursorClick` / `CursorFrame` / `CursorClickEffect` / `EffectTimeline` serde 模型已落地。
- `EffectTimeline` 已包含 `raw_system_cursor_visible` / `render_cursor_overlay`，为 Phase 6 compositor 提供明确渲染 contract。
- `BeautifyConfigSnapshot` 已持久化 recording-time raw cursor fact，其中 `raw_system_cursor_visible` 是不可变安全约束。
- `CursorProcessor` trait 边界已落地。
- 移动平均平滑、Bezier 插值、点击放大状态机和 `CursorEffectEngine` 已落地并有算法测试。
- 录制期 cursor metadata 采集运行在独立 `CursorMetadataRuntime` 线程，不进入 SCK capture callback。
- `CursorMetadataRecorder` samples/clicks 均 bounded，避免录制期 cursor metadata 无界增长。
- `CursorMetadataRecorder` / `CursorMetadataRuntime` 已记录 cursor snapshot success/error count。
- 停止录制后写入 cursor metadata sidecar，且 sidecar 写入失败不再跳过 cleanup。
- `RecordingMetadata` 已保存 fps、duration、cursor samples、cursor clicks、beautify snapshot、cursor polling health counters。
- `build_cursor_effect_timeline()` 已采用方案 B：metadata raw fact + 当前 Preview export intent。
- raw cursor visible + overlay requested 会被拒绝，避免双光标。
- raw cursor visible + overlay not requested 会生成 no-op/empty timeline，交给原始素材中的系统光标。
- raw cursor hidden + overlay not requested 会生成 baseline neutral cursor overlay frames，避免用户关闭美化效果后素材完全无光标。
- raw cursor hidden + overlay requested 会生成 overlay frames 和 click effects。
- raw cursor hidden + empty cursor samples 已 fail closed，避免 cursorless export。
- `set_beautify_config`、`get_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- Preview 已接入 beautify config、timeline build 和 export command。
- Preview config writes 已通过 promise chain 串行化，避免 A/B 写入乱序完成。
- 导出路径会等待 pending/in-flight config writes；flush 失败后不会继续 export。
- 返回录制路径也会等待 pending/in-flight config writes；flush 失败后会留在 Preview 并显示错误。
- raw cursor conflict、raw-hidden empty metadata、generic build/export/config failure 均有可见 UI 反馈。
- `build_cursor_effect_timeline` 使用 `spawn_blocking`，并拒绝 `Recording | Paused | Processing` 状态。
- `session_id + metadata_path + beautify_revision` guard 可以阻止跨 session / 旧 revision stale build 写回 service。
- stale build guard 失败后会尝试清理未引用 temp timeline JSON。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`。
- `start_recording()` 会重新根据当前后端 beautify config 推导 `show_system_cursor`，降低 `set_capture_mode()` 调用顺序风险。
- PTS invalid/negative/special flag 检查已加强，PTS delta 使用 `i128`，CMTime conversion 使用 checked integer arithmetic。
- React 未接收视频帧、音频帧或 cursor sample stream，符合架构红线。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。
- `tests/phase-4-w7-w8-checklist.md` 已同步 Round 11 后自动化验证数量和内存 gate 表述。

仍未完全完成或需作为下一轮整改/gate：

- Preview 需要识别 stale/cancelled timeline build，避免旧构建取消错误覆盖最新成功状态。
- `HANDOFF.md` Phase 4 验证数量需同步到 Rust 126 tests / frontend 39 tests。
- cursor health counter 建议补一个直接 recorder 单元测试。
- export 成功后建议清理旧 `beautifyError`。
- 真实 macOS 录制中 CMSampleBuffer PTS 与 click/video 动作对齐仍需 `npm run tauri dev` 人工验证。
- `cursor_source.rs` CoreGraphics FFI 与 `screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList 仍需 Native Safety Gate 人工逐行审查。
- 双光标策略和 raw-hidden baseline overlay 仍需真实素材人工确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor 接入。

结论：

Phase 4 作为“光标效果 foundation + metadata/timeline/export boundary”已经基本完整，Round 11 的主要功能性整改有效。当前不建议直接合并的主要原因不是捕获主链路安全，而是 Preview 异步构建反馈仍有 stale cancel 误报风险。修复 stale/cancelled build UI guard，并同步 `HANDOFF.md` 后，可以进入下一轮合并前复审；Native Safety Gate 和真实 macOS 视觉检查仍需保留为人工 gate。

### 22.7 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- SCK callback 当前仍只做 CMSampleBuffer PTS 提取、CVPixelBuffer 数据复制、AudioBufferList 读取/PCM 转换、bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内使用 `Mutex<Option<(u64, u64)>>`，但只保护轻量时间戳映射；没有等待 UI、磁盘、timeline build 或 export。
- Cursor polling 由 `CursorMetadataRuntime` 独立线程执行，不在 SCK callback 内调用 CoreGraphics cursor source。
- Timeline build 是录后 Tauri command 路径，并使用 `spawn_blocking`。
- `MacScreenCapture::start_stream()` / `stop()` 中等待 SCK start/stop completion 的同步等待发生在 recording service 的 blocking 调用路径，不在 SCK callback 内。
- 本轮未发现新增阻塞捕获主链路的问题。

内存安全：

- `platform/macos/cursor_source.rs` 中 `CGEventCreate` 返回 event 后做 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；静态复审表面配对正确，仍需人工 Native Safety Gate。
- `platform/macos/screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取，并复制到 `Arc<[u8]>`；unlock 后不再引用原始 base address。
- Audio `block_buffer` 当前 return 分支均有 `cf_release`；未发现新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded。
- `CursorMetadataRuntime` 已记录 snapshot failure count；失败不再被完全不可诊断地吞掉。
- Timeline 构建仍一次性读取 metadata、生成 frames、pretty serialize JSON；这是录后内存峰值风险，不影响捕获主链路。Phase 6 接入真实长素材后仍需压力测试。
- stale/cancelled build 会尝试删除未引用 temp JSON；资源清理较早期版本改善。

线程安全：

- Rust 侧共享 service/config/tick/mic runtime 状态使用 `Arc<Mutex<...>>`，beautify revision 使用 `AtomicU64`；未发现 Rust 数据竞争。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path + beautify_revision` stale guard 可以阻止旧 session / 旧 revision build 写回 service。
- 前端 promise chain 已能保证多次 `setBeautifyConfig()` 按用户意图顺序落到后端。
- export 和 back 路径已能正确传播 flush 失败，不会继续使用 stale backend config 导出或进入下一次录制入口。
- 剩余前端异步一致性风险集中在 timeline build 返回顺序：旧 build 的 stale/cancelled reject 可能覆盖最新 build 的成功状态。
- `CursorMetadataRuntime::stop()` 仍是直接 join polling thread；若 `source.snapshot()` 在系统 API 层异常长时间阻塞，stop 会等待。当前 CoreGraphics 调用预计很短，仍建议人工 Native Safety Gate 关注。

资源释放路径：

- `MacRecordingService.stop()` 当前会先停止 cursor runtime，再 stop native capture / mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点。
- Preview unmount cleanup 会清理 debounce timer，并尝试 fire-and-forget 写入 pending config；这是 best-effort cleanup，不作为 export/back 主路径一致性保证。
- 导出和返回录制主路径已由 `flushPendingConfig()` 正确传播失败来保证一致性。

### 22.8 BUG.md 预防规则检查

执行命令：

```bash
rg -n "whileTap|motion\\.div|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents" src src-tauri/src src-tauri/capabilities src-tauri/tauri.conf.json
```

命中要点：

```text
src/App.test.tsx:431: expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/App.test.tsx:443: expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/components/preview-view.tsx:198: <motion.div
src/components/preview-view.tsx:242: <motion.div
src/components/preview-view.tsx:339: <motion.div
src/components/preview-view.tsx:399: <motion.div
src/components/recording-status-bar.tsx:30: <motion.div
src/components/recording-status-bar.tsx:39: <motion.div
src/components/recording-status-bar.tsx:50: <motion.div
src/components/recording-status-bar.tsx:74: <motion.div
src/components/error-view.tsx:13: <motion.div
src/components/recording-panel.tsx:65: <motion.div
src/components/recording-panel.tsx:89: whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:155: whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:171: whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:185: <motion.div
src/components/processing-view.tsx:10: <motion.div
src/components/processing-view.tsx:17: <motion.div
```

解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443` 是测试断言不存在 `data-tauri-drag-region="false"`，不是源码新增 wrapper。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171` 是 `motion.button whileTap`，`whileTap` 位于交互元素自身，不是 BUG-003 中的 `motion.div whileTap` 作为 Button 直接父容器拦截模式。
- 多处 `motion.div` 用于页面、装饰或布局动画，没有发现 `motion.div` 带 `whileTap` 并直接包裹交互 Button。
- 未发现新增 `setIgnoreCursorEvents(true)` 或全窗口鼠标忽略逻辑。

结论：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- 未发现新增 `setIgnoreCursorEvents(true)`。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

### 22.9 本轮实际验证

本轮实际执行：

```bash
git diff --check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n "whileTap|motion\\.div|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents" src src-tauri/src src-tauri/capabilities src-tauri/tauri.conf.json
```

结果：

- `git diff --check`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **126 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, **21 warnings**（既有 macOS FFI naming / unused unsafe / dead_code 类 warning）
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, **21 warnings**
- `npm run build`: PASS
- `npm test -- --run`: PASS, **39 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- BUG.md 规则扫描：未发现新增违规 pattern

补充说明：

- 本轮没有运行 `npm run tauri dev`，真实 macOS 录制、click/video 对齐、双光标视觉检查、Native Safety Gate 仍需人工验证。
- 当前 Rust warnings 主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮未发现 clippy error。

### 22.10 Round 12 Ready To Merge?

**Ready to merge: With fixes / No**

本轮未发现 Critical，也未发现会直接阻塞捕获主链路的新增 Phase 4 代码。Round 11 的主要整改已经有效：

1. raw-hidden empty metadata 已 fail closed。
2. raw-visible empty metadata 已保留 no-op 合法路径。
3. cursor polling health 已写入 metadata。
4. generic build/export/config failure 已有可见 UI。
5. export flush 失败不会继续 export。
6. back flush 失败会留在 Preview。
7. checklist 已区分录制期 bounded buffer 与录后 timeline build 内存峰值 gate。

但建议在最终合并前继续完成以下整改：

1. **stale/cancelled build UI guard**：旧的 `buildCursorEffectTimeline()` 返回 `配置已变更，构建已取消` 或 `录制会话已变更，光标效果构建已取消` 时，不应覆盖最新成功状态或显示 generic failure。
2. **补前端回归测试**：覆盖旧 build stale cancel 晚到时不显示 `光标效果处理失败`，尤其是 “Build B 成功后 Build A reject” 的顺序反转场景。
3. **同步 `HANDOFF.md`**：更新 Phase 4 验证结果为 Rust 126 tests、frontend 39 tests、clippy/build 21 warnings。
4. **补 cursor health counter 直接测试**：`CursorMetadataRecorder` 调用 success/failure 后，finish metadata 中 count 正确。
5. **export 成功后清理旧错误**：成功 retry 后应清空旧 `beautifyError`，避免 UI 继续显示过期失败。
6. **继续保留人工 gate**：真实 macOS click/video 对齐、CoreGraphics/SCK FFI Native Safety、双光标和 baseline overlay 视觉效果仍需人工验证。

## 23. Round 13 整改复审（2026-05-29）

> 评审背景：用户已完成 `## 22. Round 12 整改复审（2026-05-29）` 中列出的整改任务。本轮复审当前工作区未提交改动，重点确认 Round 12 的 stale/cancelled build UI guard、前端回归测试、`HANDOFF.md` 验证数量同步、cursor health counter 测试、export 成功清理旧错误是否闭环，并继续专项检查 Phase 4 是否完整完成、是否存在阻塞捕获主链路、内存安全、线程安全和资源释放路径异常。
>
> 本轮输入：
>
> - `HANDOFF.md`
> - `BUG.md`
> - `docs/architecture/project-architecture-and-overall-planning.md`
> - `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md`
> - `docs/superpowers/reviews/2026-05-27-phase-4-code-review.md`
> - `tests/phase-4-w7-w8-checklist.md`
> - 当前 Phase 4 相关代码与未提交 Round 12 整改
>
> 当前结论：**With fixes / No**，暂不建议直接合并。本轮未发现 Critical，也未发现新增 cursor smoothing / Bezier interpolation / click magnification 阻塞 ScreenCaptureKit callback。Round 12 的 product code 方向基本正确：stale/cancelled build UI guard 已实现，raw-hidden empty metadata 已 fail closed，generic Preview/export/config failure 已有可见 UI，export 成功 retry 会清理旧错误，`HANDOFF.md` 与 checklist 的验证数量已同步到 Rust 127 / frontend 42。但 Round 12 最核心的 stale/cancelled build 回归测试目前没有真正触发 A/B 两个 timeline build，测试可在未覆盖目标竞态时通过，因此仍需合并前修正。
>
> 交叉验证：本轮按 `$superpowers:requesting-code-review` 流程启动独立审查代理 `Anscombe`。独立审查结论与本地主审一致：无 Critical；product code 基本闭环；核心 Important 是 `src/App.test.tsx` 中新增 stale/cancelled build 测试没有推进 debounce timer、没有断言两个 build 真实发生，因而不能证明 Round 12 的主风险已被测试锁住。独立审查还指出 `HANDOFF.md` 与 checklist 顶部 `最后更新` 日期仍停留在 `2026-05-27`。

### 23.1 本轮重点确认结果

Round 12 整改已闭环项：

- `src/components/preview-view.tsx:41-47`：`messageForBeautifyError()` 已把 `配置已变更，构建已取消` / `录制会话已变更，光标效果构建已取消` 映射为空字符串，避免 stale/cancelled build 被当成普通失败展示。
- `src/components/preview-view.tsx:80`、`src/components/preview-view.tsx:161-178`：新增 `buildSeqRef`，debounced timeline build catch 中会检查当前 seq；旧请求返回时不会覆盖最新请求的 UI 状态。
- `src/components/preview-view.tsx:183-192`：export 成功后会 `setBeautifyError(null)`，避免一次失败后成功 retry 仍显示旧错误。
- `src-tauri/src/app/cursor_metadata_runtime.rs:137-149`、`src-tauri/src/app/cursor_metadata_runtime.rs:366-394`：cursor snapshot success/error counter 已写入 metadata，并补充 `recorder_counts_snapshot_successes_and_failures` 单测。
- `src-tauri/src/lib.rs:390-395`、`src-tauri/src/lib.rs:752-767`：raw system cursor hidden 且 `cursor_samples` 为空时已 fail closed，不再成功生成 empty overlay timeline。
- `HANDOFF.md:100-107`：Phase 4 验证结果已同步为 Rust **127 tests**、frontend **42 tests**、clippy/build **21 warnings**。
- `tests/phase-4-w7-w8-checklist.md:67-74`：Verification Summary 已同步到 Round 12，记录 Rust **127 tests**、frontend **42 tests**。

本轮未发现的问题：

- 未发现新的 Critical。
- 未发现 cursor smoothing、Bezier interpolation、click magnification 进入 SCK callback。
- 未发现新增磁盘 IO、JSON 读写、timeline build、Tauri invoke/export 等重操作进入捕获 callback。
- 未发现新增 Rust 数据竞争、明显 use-after-free 或资源释放路径提前中断。
- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button 的 BUG-003 模式。

### 23.2 Strengths

1. stale/cancelled build 的 product code 防护方向正确

   - 前端不再把后端 stale guard 返回的 cancel message 映射成 generic failure。
   - `buildSeqRef` 让旧 build 的 late reject/resolve 不能覆盖最新 build 的 UI 状态。
   - 该策略与 Rust 侧 `session_id + metadata_path + beautify_revision` guard 是互补关系：Rust 防止旧 build 写回 service，React 防止旧请求污染用户反馈。

2. raw cursor / overlay contract 更稳固

   - `build_effect_timeline_from_metadata()` 已提取为纯 helper，测试覆盖 raw visible/raw hidden、overlay requested/not requested、empty cursor samples 等关键分支。
   - raw cursor visible + overlay requested 会继续拒绝，避免双光标。
   - raw cursor hidden + empty cursor samples 已 fail closed，避免默认隐藏系统光标后导出无光标。
   - raw cursor hidden + overlay disabled 仍会生成 baseline neutral frames，保留“原始素材无系统光标时仍需要 overlay 光标”的安全契约。

3. Preview/export 失败反馈比前几轮完整

   - raw cursor conflict、raw-hidden empty metadata、generic timeline build failure、generic export failure、generic config write failure 都有可见 UI 反馈。
   - export/back 主路径仍会等待 `flushPendingConfig()`；flush 失败不会继续 export 或离开 Preview。
   - export retry 成功后旧错误会被清理，避免用户看到过期失败。

4. 录制期 cursor metadata 健康状态更可诊断

   - `CursorMetadataRuntime` 不再完全吞掉 snapshot failure，而是累计 error count。
   - metadata sidecar 现在保存 `cursor_snapshot_success_count` 和 `cursor_snapshot_error_count`，为后续真实 macOS 排障提供信号。
   - 字段使用 `#[serde(default)]`，旧 metadata JSON 仍可反序列化。

5. Phase 4 架构红线仍然清楚

   - React 未接收视频帧、音频帧或 cursor sample stream。
   - cursor metadata 采集在独立 polling runtime；timeline build 在录后 command 路径并使用 `spawn_blocking`。
   - 当前新增逻辑主要集中在 command contract、Preview UI feedback、metadata health counter 和测试，不改变捕获 callback 的重负载路径。

### 23.3 Critical Findings

本轮未发现 Critical。

没有发现以下阻塞性问题：

- 没有发现 Phase 4 cursor 算法直接阻塞捕获 callback。
- 没有发现明显 use-after-free。
- 没有发现 sidecar 写入失败跳过 capture/mic/consumer cleanup 的旧问题复发。
- 没有发现 export/back 主路径继续吞掉 config write 失败。
- 没有发现 raw-hidden empty metadata 继续成功生成 cursorless timeline。

### 23.4 Important Findings

#### Important 1: 新增 stale/cancelled build 回归测试没有真正覆盖目标竞态

位置：

- `src/App.test.tsx:1449-1498`
- `src/App.test.tsx:1501-1553`
- `src/components/preview-view.tsx:158-180`

现象：

Round 12 新增了两条测试：

```ts
it('does not show error when stale cancelled build rejects after a successful build', async () => {
  // ...
  fireEvent.click(switches[0])
  fireEvent.click(switches[1])

  buildDeferreds[1].resolve(undefined)
  await vi.waitFor(() => {
    expect(screen.queryByText(/光标效果处理失败/)).toBeNull()
  })

  buildDeferreds[0].reject(new Error('光标美化配置已变更，构建已取消'))
  await act(async () => {})
  expect(screen.queryByText(/光标效果处理失败/)).toBeNull()
})
```

但 `PreviewView` 的 timeline build 是 300ms debounce 后才会执行：

```ts
debounceRef.current = setTimeout(() => {
  // enqueueConfigWrite(nextConfig).then(... buildCursorEffectTimeline ...)
}, 300)
```

这两条测试存在三个问题：

1. 没有使用 fake timers 或真实等待推进 300ms debounce。
2. 没有断言 `build_cursor_effect_timeline` 被调用过，更没有断言调用了两次。
3. 连续第二次点击会清理第一次 debounce timer，因此 Build A 在测试中通常没有机会 dispatch。

为什么重要：

- Round 12 的核心风险正是 “Build B 已成功，Build A 之后 stale-cancel reject，旧请求不应覆盖最新成功状态”。
- 当前测试可以在没有触发 Build A / Build B 的情况下通过，也就是说，即使移除 `buildSeqRef` guard 或 stale cancel message guard，测试仍可能是绿色。
- 这会削弱后续整改信号：看起来已经补了回归测试，但实际上没有锁住这次 review 要求的主竞态。

建议修复：

1. 重写其中至少一条测试，确保真正发生两个 build：

   ```ts
   vi.useFakeTimers()

   fireEvent.click(switches[0])
   await act(async () => {
     vi.advanceTimersByTime(350)
   })
   await vi.waitFor(() => {
     expect(buildCallCount).toBe(1)
   })

   fireEvent.click(switches[1])
   await act(async () => {
     vi.advanceTimersByTime(350)
   })
   await vi.waitFor(() => {
     expect(buildCallCount).toBe(2)
   })
   ```

2. 用 deferred promise 控制顺序：

   - 第一次 `build_cursor_effect_timeline` 返回 `buildA.promise`，保持 pending。
   - 第二次返回 `Promise.resolve()` 或 `buildB.promise`。
   - 确认 Build B 成功后，reject Build A 为 `new Error('光标美化配置已变更，构建已取消')`。
   - 断言 UI 不显示 `光标效果处理失败`。

3. 建议补充两类断言：

   - `expect(buildCallCount).toBe(2)`，防止测试空转。
   - `expect(screen.queryByText(/光标效果处理失败/)).toBeNull()` 在 late reject 后仍成立。

4. 可选：同时覆盖另一个 stale message：

   - `录制会话已变更，光标效果构建已取消`

建议优先级：

- 合并前建议修复。product code 看起来已经处理了目标问题，但当前测试不能证明该问题未来不会回归。

### 23.5 Minor Findings

#### Minor 1: `HANDOFF.md` 与 checklist 顶部更新时间仍是旧日期

位置：

- `HANDOFF.md:3`
- `tests/phase-4-w7-w8-checklist.md:3`

现象：

`HANDOFF.md` 顶部仍写：

```markdown
> 最后更新：2026-05-27 | Phase 4 光标平滑与点击放大实现完成（自动化验证全部通过，Native Safety Gate 待人工审查）。
```

`tests/phase-4-w7-w8-checklist.md` 顶部仍写：

```markdown
> 最后更新：2026-05-27 | 自动化验证全部通过，Native Safety Gate 待人工审查，FFmpeg compositor 待 Phase 6 接入
```

但文件正文已经记录 Round 12 / 2026-05-29 的验证数量和状态。

为什么重要：

- `HANDOFF.md` 是项目约定中的多轮交接核心文件。
- checklist 是 Phase 4 自测清单，顶部 metadata 与正文不一致会让后续接手者误判文档是否已同步。
- 这是文档一致性问题，不是 runtime 风险。

建议修复：

1. 将 `HANDOFF.md` 顶部更新为 `2026-05-29`，状态可写为 `Phase 4 Round 12 整改复审后自动化验证通过，Native Safety Gate 待人工审查`。
2. 将 `tests/phase-4-w7-w8-checklist.md` 顶部更新为 `2026-05-29`，保持 Native Safety Gate / Phase 6 compositor 待接入说明。

#### Minor 2: raw-hidden empty metadata 的错误文案暗示“关闭光标美化”可恢复当前素材

位置：

- `src-tauri/src/lib.rs:390-393`

现象：

当前错误文案为：

```rust
"本次素材未录入系统光标，且光标元数据为空，无法生成光标时间线。请重新录制或关闭光标美化。"
```

但该分支的前提是 `raw_system_cursor_visible == false` 且 `cursor_samples.is_empty()`。此时原始素材里没有系统光标，metadata 又没有 overlay 输入；用户在 Preview 中“关闭光标美化”并不能让当前素材恢复光标。正确恢复方式是先关闭光标美化，再重新录制，或修复 cursor metadata 采集后重新录制。

为什么重要：

- 文案可能误导用户以为只要在当前 Preview 关闭光标美化就能继续导出。
- 这不会破坏数据安全，因为 command 已经 fail closed；但会影响可诊断性。

建议修复：

将文案改为更精确的形式，例如：

```rust
"本次素材未录入系统光标，且光标元数据为空，无法生成光标时间线。请关闭光标美化后重新录制，或重新录制以恢复光标元数据。"
```

### 23.6 Phase 4 完整性复审结论

已完成或基本完成：

- `CursorSample` / `CursorClick` / `CursorFrame` / `CursorClickEffect` / `EffectTimeline` serde 模型已落地。
- `EffectTimeline` 已包含 `raw_system_cursor_visible` / `render_cursor_overlay`，为 Phase 6 compositor 提供明确渲染 contract。
- `BeautifyConfigSnapshot` 已持久化 recording-time raw cursor fact，其中 `raw_system_cursor_visible` 是不可变安全约束。
- `CursorProcessor` trait 边界已落地。
- 移动平均平滑、Bezier 插值、点击放大状态机和 `CursorEffectEngine` 已落地并有算法测试。
- 录制期 cursor metadata 采集运行在独立 `CursorMetadataRuntime` 线程，不进入 SCK capture callback。
- `CursorMetadataRecorder` samples/clicks 均 bounded，避免录制期 cursor metadata 无界增长。
- `CursorMetadataRecorder` / `CursorMetadataRuntime` 已记录 cursor snapshot success/error count，并有直接单元测试。
- 停止录制后写入 cursor metadata sidecar，且 sidecar 写入失败不再跳过 cleanup。
- `RecordingMetadata` 已保存 fps、duration、cursor samples、cursor clicks、beautify snapshot、cursor polling health counters。
- `build_cursor_effect_timeline()` 已采用方案 B：metadata raw fact + 当前 Preview export intent。
- raw cursor visible + overlay requested 会被拒绝，避免双光标。
- raw cursor visible + overlay not requested 会生成 no-op/empty timeline，交给原始素材中的系统光标。
- raw cursor hidden + overlay not requested 会生成 baseline neutral cursor overlay frames，避免用户关闭美化效果后素材完全无光标。
- raw cursor hidden + overlay requested 会生成 overlay frames 和 click effects。
- raw cursor hidden + empty cursor samples 已 fail closed，避免 cursorless export。
- `set_beautify_config`、`get_beautify_config`、`build_cursor_effect_timeline`、`export_video` command 已接入。
- Preview 已接入 beautify config、timeline build 和 export command。
- Preview config writes 已通过 promise chain 串行化，避免 A/B 写入乱序完成。
- 导出路径会等待 pending/in-flight config writes；flush 失败后不会继续 export。
- 返回录制路径也会等待 pending/in-flight config writes；flush 失败后会留在 Preview 并显示错误。
- raw cursor conflict、raw-hidden empty metadata、generic build/export/config failure 均有可见 UI 反馈。
- export 成功 retry 会清理旧 `beautifyError`。
- Preview product code 已识别 stale/cancelled timeline build，避免旧构建取消错误覆盖最新成功状态。
- `build_cursor_effect_timeline` 使用 `spawn_blocking`，并拒绝 `Recording | Paused | Processing` 状态。
- `session_id + metadata_path + beautify_revision` guard 可以阻止跨 session / 旧 revision stale build 写回 service。
- stale build guard 失败后会尝试清理未引用 temp timeline JSON。
- `CaptureConfig.show_system_cursor` 已接入 `SCStreamConfiguration::setShowsCursor`。
- `start_recording()` 会重新根据当前后端 beautify config 推导 `show_system_cursor`，降低 `set_capture_mode()` 调用顺序风险。
- PTS invalid/negative/special flag 检查已加强，PTS delta 使用 `i128`，CMTime conversion 使用 checked integer arithmetic。
- React 未接收视频帧、音频帧或 cursor sample stream，符合架构红线。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。
- `HANDOFF.md` 与 `tests/phase-4-w7-w8-checklist.md` 正文验证数量已同步到 Rust 127 / frontend 42。

仍未完全完成或需作为下一轮整改/gate：

- stale/cancelled build 的前端回归测试需要重写，确保真的 dispatch Build A / Build B，并验证 Build A late reject 不污染 Build B 成功状态。
- `HANDOFF.md` 与 checklist 顶部 `最后更新` 日期需同步到 2026-05-29。
- raw-hidden empty metadata 的错误文案需避免暗示“关闭光标美化”可恢复当前素材。
- 真实 macOS 录制中 CMSampleBuffer PTS 与 click/video 动作对齐仍需 `npm run tauri dev` 人工验证。
- `cursor_source.rs` CoreGraphics FFI 与 `screen_capture_kit.rs` SCK/CVPixelBuffer/AudioBufferList 仍需 Native Safety Gate 人工逐行审查。
- 双光标策略和 raw-hidden baseline overlay 仍需真实素材人工确认。
- 真实导出视频包含光标平滑和点击放大仍依赖 Phase 6 FFmpeg compositor 接入。

结论：

Phase 4 作为“光标效果 foundation + metadata/timeline/export boundary”已经基本完整，Round 12 的 product code 整改方向正确，捕获主链路与资源释放路径未见新增阻塞问题。当前不建议直接合并的主要原因是测试质量：Round 12 的核心竞态虽然在代码里有防护，但回归测试没有真实触发该竞态。修复测试后，再同步两个文档 header 与错误文案，即可进入下一轮合并前复审；Native Safety Gate 和真实 macOS 视觉检查仍需保留为人工 gate。

### 23.7 捕获主链路、内存安全、线程安全、资源释放专项复审

捕获主链路：

- 未发现 cursor smoothing、Bezier interpolation、click magnification 在 ScreenCaptureKit callback 内执行。
- SCK callback 当前仍只做 CMSampleBuffer PTS 提取、CVPixelBuffer 数据复制、AudioBufferList 读取/PCM 转换、bounded channel `try_send_drop_newest`。
- `normalize_pts()` 在 callback 内使用 `Mutex<Option<(u64, u64)>>`，只保护轻量时间戳映射；没有等待 UI、磁盘、timeline build 或 export。
- Cursor polling 由 `CursorMetadataRuntime` 独立线程执行，不在 SCK callback 内调用 CoreGraphics cursor source。
- Timeline build 是录后 Tauri command 路径，并使用 `spawn_blocking`。
- `MacScreenCapture::start_stream()` / `stop()` 中等待 SCK start/stop completion 的同步等待发生在 recording service 的 blocking 调用路径，不在 SCK callback 内。
- 本轮未发现新增阻塞捕获主链路的问题。

内存安全：

- `platform/macos/cursor_source.rs` 中 `CGEventCreate` 返回 event 后做 null 检查，并在 `CGEventGetLocation` 后 `CFRelease`；静态复审表面配对正确，仍需人工 Native Safety Gate。
- `platform/macos/screen_capture_kit.rs` 的 CVPixelBuffer base address 在 lock 后读取，并复制到 `Arc<[u8]>`；unlock 后不再引用原始 base address。
- Audio `block_buffer` 当前 return 分支均有 `cf_release`；未发现新增泄漏路径。
- `CursorMetadataRecorder` samples/clicks 均 bounded。
- `CursorMetadataRuntime` 已记录 snapshot success/error count；失败不再被完全不可诊断地吞掉。
- Timeline 构建仍一次性读取 metadata、生成 frames、pretty serialize JSON；这是录后内存峰值风险，不影响捕获主链路。Phase 6 接入真实长素材后仍需压力测试。
- stale/cancelled build 会尝试删除未引用 temp JSON；资源清理较早期版本改善。

线程安全：

- Rust 侧共享 service/config/tick/mic runtime 状态使用 `Arc<Mutex<...>>`，beautify revision 使用 `AtomicU64`；未发现 Rust 数据竞争。
- `CursorMetadataRuntime` 用 `AtomicBool` 停止并 join 线程，Drop 中也会 stop。
- `session_id + metadata_path + beautify_revision` stale guard 可以阻止旧 session / 旧 revision build 写回 service。
- 前端 promise chain 已能保证多次 `setBeautifyConfig()` 按用户意图顺序落到后端。
- export 和 back 路径已能正确传播 flush 失败，不会继续使用 stale backend config 导出或进入下一次录制入口。
- 前端 product code 已增加 `buildSeqRef` 和 stale/cancel message guard，理论上能防止旧 build 的 late reject 污染最新 UI；但当前测试没有真正覆盖该竞态，见 Important 1。
- `CursorMetadataRuntime::stop()` 仍是直接 join polling thread；若 `source.snapshot()` 在系统 API 层异常长时间阻塞，stop 会等待。当前 CoreGraphics 调用预计很短，仍建议人工 Native Safety Gate 关注。

资源释放路径：

- `MacRecordingService.stop()` 当前会先停止 cursor runtime，再 stop native capture / mic capture、signal consumer、join consumer、写 sidecar、reset mic level、推进 state machine。
- cursor metadata sidecar 写入失败会记录错误并继续 cleanup，不再造成状态机悬挂。
- `MacScreenCapture::stop()` 超时时保留 native handles 并设置 `needs_reset`，避免潜在 use-after-free；这仍属于 Native Safety Gate 重点。
- Preview unmount cleanup 会清理 debounce timer，并尝试 fire-and-forget 写入 pending config；这是 best-effort cleanup，不作为 export/back 主路径一致性保证。
- 导出和返回录制主路径已由 `flushPendingConfig()` 正确传播失败来保证一致性。

### 23.8 BUG.md 预防规则检查

执行命令：

```bash
rg -n "whileTap|motion\\.div|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents" src src-tauri/src src-tauri/capabilities src-tauri/tauri.conf.json
```

命中要点：

```text
src/App.test.tsx:431: expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/App.test.tsx:443: expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
src/components/preview-view.tsx:217: <motion.div
src/components/preview-view.tsx:261: <motion.div
src/components/preview-view.tsx:358: <motion.div
src/components/preview-view.tsx:418: <motion.div
src/components/recording-status-bar.tsx:30: <motion.div
src/components/recording-status-bar.tsx:39: <motion.div
src/components/recording-status-bar.tsx:50: <motion.div
src/components/recording-status-bar.tsx:74: <motion.div
src/components/error-view.tsx:13: <motion.div
src/components/recording-panel.tsx:65: <motion.div
src/components/recording-panel.tsx:89: whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:155: whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:171: whileTap={{ scale: 0.98 }}
src/components/recording-panel.tsx:185: <motion.div
src/components/processing-view.tsx:10: <motion.div
src/components/processing-view.tsx:17: <motion.div
```

解释：

- `src/App.test.tsx:431`、`src/App.test.tsx:443` 是测试断言不存在 `data-tauri-drag-region="false"`，不是源码新增 wrapper。
- `src/components/recording-panel.tsx:89`、`src/components/recording-panel.tsx:155`、`src/components/recording-panel.tsx:171` 是 `motion.button whileTap`，`whileTap` 位于交互元素自身，不是 BUG-003 中的 `motion.div whileTap` 作为 Button 直接父容器拦截模式。
- 多处 `motion.div` 用于页面、装饰或布局动画，没有发现 `motion.div` 带 `whileTap` 并直接包裹交互 Button。
- 未发现新增 `setIgnoreCursorEvents(true)` 或全窗口鼠标忽略逻辑。

结论：

- 未发现新增 `data-tauri-drag-region="false"` 区域级 wrapper。
- 未发现新增 `motion.div whileTap` 直接包裹交互 Button。
- 未发现新增 `setIgnoreCursorEvents(true)`。
- 未发现本轮 Phase 4 改动违反 BUG-001/BUG-002/BUG-003 的预防规则。

### 23.9 本轮实际验证

本轮实际执行：

```bash
git diff --check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n "whileTap|motion\\.div|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents" src src-tauri/src src-tauri/capabilities src-tauri/tauri.conf.json
```

结果：

- `git diff --check`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, **127 tests**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, **21 warnings**（既有 macOS FFI naming / unused unsafe / dead_code 类 warning；命令曾短暂等待 build directory lock，随后正常完成）
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS, **21 warnings**
- `npm run build`: PASS
- `npm test -- --run`: PASS, **42 tests**；输出多条 `Window.scrollTo()` 未实现提示，测试仍通过
- BUG.md 规则扫描：未发现新增违规 pattern

补充说明：

- 本轮没有运行 `npm run tauri dev`，真实 macOS 录制、click/video 对齐、双光标视觉检查、Native Safety Gate 仍需人工验证。
- 当前 Rust warnings 主要集中在 macOS FFI 命名、unused unsafe、unused FFI helper / wrapper 等既有问题；本轮未发现 clippy error。

### 23.10 Round 13 Ready To Merge?

**Ready to merge: With fixes / No**

本轮未发现 Critical，也未发现会直接阻塞捕获主链路的新增 Phase 4 代码。Round 12 的主要 product code 整改已经有效：

1. stale/cancelled build 的 UI guard 已实现。
2. raw-hidden empty metadata 已 fail closed。
3. raw-visible empty metadata 已保留 no-op 合法路径。
4. cursor polling health 已写入 metadata，并补充直接 recorder 单测。
5. generic build/export/config failure 已有可见 UI。
6. export flush 失败不会继续 export。
7. back flush 失败会留在 Preview。
8. export 成功 retry 会清理旧 `beautifyError`。
9. `HANDOFF.md` 与 checklist 正文验证数量已同步到 Rust 127 / frontend 42。

但建议在最终合并前继续完成以下整改：

1. **重写 stale/cancelled build 前端回归测试**：使用 fake timers 或明确等待推进 debounce，确保 Build A 和 Build B 都真实 dispatch；断言 `build_cursor_effect_timeline` 调用两次；验证 Build B 成功后 Build A stale reject 不显示 `光标效果处理失败`。
2. **同步文档顶部 metadata**：更新 `HANDOFF.md` 和 `tests/phase-4-w7-w8-checklist.md` 的 `最后更新` 日期为 `2026-05-29`，避免 header 与正文状态漂移。
3. **收紧 raw-hidden empty metadata 错误文案**：避免暗示用户只需关闭当前 Preview 的光标美化即可恢复当前素材；建议写成“关闭光标美化后重新录制”。
4. **继续保留人工 gate**：真实 macOS click/video 对齐、CoreGraphics/SCK FFI Native Safety、双光标和 baseline overlay 视觉效果仍需人工验证。
