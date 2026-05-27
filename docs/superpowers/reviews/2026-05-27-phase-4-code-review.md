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
