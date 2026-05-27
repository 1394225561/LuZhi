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
