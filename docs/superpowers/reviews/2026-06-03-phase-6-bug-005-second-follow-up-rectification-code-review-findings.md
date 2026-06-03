# Phase 6 BUG-005 Second Follow-up Rectification Code Review Findings

> 日期：2026-06-03
> 审查对象：当前工作区未提交改动
> 审查结论：**Ready to merge? No**

## 1. 审查范围

重点文件：

- `src-tauri/src/platform/macos_service.rs`
- `src-tauri/src/media/recording_writer.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/platform/macos/cpal_microphone.rs`
- `src/lib/tauri.ts`
- `src/App.tsx`

本次重点核查目标：

1. per-source writer counter 是否只在 `push_audio()` 成功后递增。
2. `RecordingResult` 是否真正携带 `RecordingDiagnostics`，并且前端可消费。
3. timeout 分支是否有直接回归测试。
4. review 文档尾部空白是否清理。
5. 是否遵循 `BUG.md` 中 BUG-005 / BUG-005_2 的预防规则。

## 2. 总体结论

本轮整改方向是正确的，但当前工作区仍不能合并。

已确认有价值的修复：

1. `record_source_contribution()` 已从 `push_audio()` 前移动到 `push_audio()` 成功分支后，关闭了 per-source counter false evidence 的主要实现问题。
2. Rust `RecordingResult` 已新增 `diagnostics: RecordingDiagnostics` 字段。
3. `drive_state_machine()` 在成功返回前会把 capture/synchronizer diagnostics 注入 `RecordingResult`。
4. `CpalMicrophoneStopDiagnostics` 已添加 `#[serde(rename_all = "camelCase")]`，与前端 camelCase 结构一致。
5. 文档尾部空白清理通过 `git diff --check`。

仍需修复的阻塞点：

1. 前端构建失败：`App.tsx` 没有透传新增的 `writerDiagnostics` / `diagnostics` 字段。
2. 失败路径仍只返回 `String`，最需要诊断的失败场景仍丢失结构化 diagnostics。
3. TypeScript `RecordingDiagnostics` 类型没有覆盖 Rust 的 before-writer 关键字段。
4. timeout 分支测试仍没有直接覆盖 `RecvTimeoutError::Timeout`。
5. `RecordingWriter::record_source_contribution()` trait 注释仍描述旧调用顺序。

## 3. Strengths

### 3.1 per-source writer counter 时序修复方向正确

位置：

- `src-tauri/src/platform/macos_service.rs:791`
- `src-tauri/src/platform/macos_service.rs:938`

当前实现先保存 `synced` / `synchronized` 的 source metadata，再执行 `writer.push_audio(...)`。只有 `push_audio()` 成功后才调用：

```rust
writer.record_source_contribution(has_system, has_mic, system_frames, mic_frames);
diagnostics.mixed_chunks_queued += 1;
```

评审结论：

- 这符合 `BUG.md` 规则 32：per-source writer counter 必须在 `push_audio()` 成功后递增。
- 新增测试 `consume_frames_does_not_increment_per_source_writer_counter_when_push_audio_fails` 能锁住 push 失败时 counter 不虚假递增。
- 新增测试 `consume_frames_increments_per_source_writer_counter_on_successful_push` 能锁住成功路径仍正确计数。

### 3.2 Rust `RecordingResult` 已携带 diagnostics

位置：

- `src-tauri/src/media/recording_writer.rs:268`
- `src-tauri/src/media/recording_writer.rs:281`
- `src-tauri/src/platform/macos_service.rs:541`
- `src-tauri/src/platform/macos_service.rs:545`

当前 `RecordingResult` 新增：

```rust
pub diagnostics: RecordingDiagnostics,
```

`drive_state_machine()` 中也有：

```rust
let mut result = output.result;
result.diagnostics = output.diagnostics;
```

评审结论：

- 成功 stop 路径上，capture-side、synchronizer-side、mic stop diagnostics 已经能进入 `RecordingResult`。
- 新增 `recording_result_serializes_diagnostics_as_camel_case` 测试覆盖了 `diagnostics.requestedSystemAudio` 和 `micStopDiagnostics.stopRequested`。

### 3.3 mic stop diagnostics 序列化命名已修正

位置：

- `src-tauri/src/platform/macos/cpal_microphone.rs:16`

当前 `CpalMicrophoneStopDiagnostics` 已添加：

```rust
#[serde(rename_all = "camelCase")]
```

评审结论：

- `stop_requested`、`callbacks_after_stop` 等字段序列化为 `stopRequested`、`callbacksAfterStop`。
- 与 `src/lib/tauri.ts` 的 `CpalMicrophoneStopDiagnostics` 类型命名一致。

### 3.4 文档尾部空白清理通过

验证命令：

```bash
git diff --check
```

结果：

- 通过，无 trailing whitespace 报告。

## 4. Critical Findings

### Critical 1: 前端 stop 后丢弃新增 diagnostics，导致 `npm run build` 失败

位置：

- `src/App.tsx:190`
- `src/lib/tauri.ts:91`
- `src/lib/tauri.ts:100`
- `src/lib/tauri.ts:101`

当前问题：

`src/lib/tauri.ts` 中 `RecordingResult` 已新增：

```ts
writerDiagnostics: WriterDiagnostics
diagnostics: RecordingDiagnostics
```

但 `src/App.tsx` 的 stop 流程仍手动重新组装对象：

```ts
setRecordingResult({
  durationSecs: result.durationSecs,
  frameCount: result.frameCount,
  mixedAudioChunkCount: result.mixedAudioChunkCount,
  outputPath: result.outputPath ?? null,
  cursorMetadataPath: result.cursorMetadataPath ?? null,
  effectTimelinePath: result.effectTimelinePath ?? null,
  trimMetadataPath: result.trimMetadataPath ?? null,
  cutTimelinePath: result.cutTimelinePath ?? null,
})
```

它没有带上 `writerDiagnostics` 和 `diagnostics`。

为什么重要：

1. 这是构建级 blocker。
2. 即使 Rust 成功返回完整 `RecordingResult`，前端也会在 state 层丢掉新增 diagnostics。
3. 违反本轮 Phase B 的目标：`RecordingResult` 暴露 `RecordingDiagnostics` 后，前端也应该能消费。
4. 违反 `BUG.md` 规则 33 的目标：不能仅靠 eprintln 暴露诊断信息。

已验证失败：

```bash
npm run build
```

失败摘要：

```text
src/App.tsx(190,26): error TS2345:
Argument of type '{ durationSecs: number; ... }' is not assignable to
SetStateAction<RecordingResult | null>.
Type ... is missing the following properties from type 'RecordingResult':
writerDiagnostics, diagnostics
```

建议修复：

最小修复：

```ts
setRecordingResult({
  durationSecs: result.durationSecs,
  frameCount: result.frameCount,
  mixedAudioChunkCount: result.mixedAudioChunkCount,
  outputPath: result.outputPath ?? null,
  cursorMetadataPath: result.cursorMetadataPath ?? null,
  effectTimelinePath: result.effectTimelinePath ?? null,
  trimMetadataPath: result.trimMetadataPath ?? null,
  cutTimelinePath: result.cutTimelinePath ?? null,
  writerDiagnostics: result.writerDiagnostics,
  diagnostics: result.diagnostics,
})
```

更简单的修复：

```ts
setRecordingResult(result)
```

如果保留手动重组，需要同步更新相关 frontend test mocks，让 mock 的 `stop_recording` 返回值包含 `writerDiagnostics` 和 `diagnostics`。

## 5. Important Findings

### Important 1: 失败路径仍丢失结构化 diagnostics

位置：

- `src-tauri/src/lib.rs:242`
- `src-tauri/src/lib.rs:255`
- `src-tauri/src/lib.rs:280`
- `src-tauri/src/platform/macos_service.rs:541`
- `src-tauri/src/platform/macos_service.rs:563`
- `src-tauri/src/platform/macos_service.rs:564`

当前问题：

`drive_state_machine()` 中先把 diagnostics 注入 result：

```rust
let mut result = output.result;
self.errors.extend(output.errors);
result.diagnostics = output.diagnostics;
```

但如果 `self.errors` 非空，最终不会返回这个 `result`：

```rust
self.service.state_machine.fail();
Err(crate::app::error::AppError::RecordingFinalizeFailed {
    reason: self.errors.join("; "),
})
```

Tauri command `stop_recording()` 又把 `AppError` 转成纯字符串：

```rust
result.map_err(|e| e.to_string())
```

因此失败路径中结构化 diagnostics 仍无法到达前端或自动化测试。

为什么重要：

1. 最需要 diagnostics 的正是失败场景：
   - `push_audio()` 失败；
   - artifact contract 失败；
   - writer finish 失败；
   - consumer timeout；
   - mic stop 失败；
   - source-aware contract 失败。
2. 当前成功路径已改善，但失败路径仍接近“只靠错误字符串 + eprintln”。
3. 这没有完全满足 `BUG.md` 规则 21 和规则 33 的目标。
4. 后续定位蓝牙 HFP release、writer enqueue failure、source-aware contract failure 时，前端和自动化记录拿不到结构化字段。

建议修复：

推荐改为结构化 stop response，例如：

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StopRecordingResponse {
    result: RecordingResult,
    error: Option<String>,
}
```

或者定义结构化错误：

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StopRecordingError {
    message: String,
    diagnostics: RecordingDiagnostics,
    writer_diagnostics: WriterDiagnostics,
}
```

如果短期不改 command 返回类型，至少在 `Err` 前 emit 一个结构化 diagnostics event，例如 `recording-diagnostics`。但这只是兜底，API contract 仍建议显式化。

建议新增测试：

1. `stop_recording_returns_diagnostics_on_finalize_error`
2. `mac_recording_stop_failure_preserves_mic_stop_diagnostics`
3. `push_audio_failure_error_includes_structured_diagnostics`

### Important 2: TypeScript `RecordingDiagnostics` 类型缺少 before-writer 关键字段

位置：

- `src/lib/tauri.ts:121`
- `src-tauri/src/media/recording_writer.rs:91`
- `src-tauri/src/media/recording_writer.rs:92`
- `src-tauri/src/media/recording_writer.rs:96`
- `src-tauri/src/media/recording_writer.rs:98`
- `src-tauri/src/media/recording_writer.rs:100`
- `src-tauri/src/media/recording_writer.rs:102`

当前 Rust `RecordingDiagnostics` 包含以下字段：

```rust
pub system_rms_max_before_writer: f32,
pub mic_rms_max_before_writer: f32,
pub system_windows_before_writer: u64,
pub mic_windows_before_writer: u64,
pub system_frames_before_writer: u64,
pub mic_frames_before_writer: u64,
```

serde camelCase 后对应：

```ts
systemRmsMaxBeforeWriter
micRmsMaxBeforeWriter
systemWindowsBeforeWriter
micWindowsBeforeWriter
systemFramesBeforeWriter
micFramesBeforeWriter
```

但 `src/lib/tauri.ts` 的 `RecordingDiagnostics` 只到：

```ts
sourceTimeoutWindowCount: number
micStopDiagnostics: CpalMicrophoneStopDiagnostics | null
```

为什么重要：

1. before-writer 字段是 BUG-005 / BUG-005_2 的核心诊断字段。
2. 它们用于区分：
   - capture 完全没收到；
   - synchronizer 没 emit；
   - before-writer 有 source，但 writer 没收到；
   - artifact validation 误判。
3. 当前 TS 类型与 Rust serde shape 不一致。
4. 即使实际 JSON 中存在这些字段，前端类型层无法安全消费，后续 UI 或日志采集容易遗漏。

建议修复：

在 `src/lib/tauri.ts` 中补齐：

```ts
systemRmsMaxBeforeWriter: number
micRmsMaxBeforeWriter: number
systemWindowsBeforeWriter: number
micWindowsBeforeWriter: number
systemFramesBeforeWriter: number
micFramesBeforeWriter: number
```

建议同步增强 Rust 序列化测试：

```rust
assert_eq!(json["diagnostics"]["systemWindowsBeforeWriter"], ...);
assert_eq!(json["diagnostics"]["micFramesBeforeWriter"], ...);
```

建议新增或更新前端测试：

1. mock `stop_recording` 返回完整 diagnostics。
2. 验证 `recordingResult.diagnostics.systemWindowsBeforeWriter` 能在 state 中保留。

### Important 3: timeout 分支仍缺少直接回归测试

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:104`
- `src-tauri/src/platform/macos_service.rs:402`
- `src-tauri/src/media/ffmpeg_writer.rs:1701`
- `src-tauri/src/platform/macos_service.rs:2211`

当前实现已经做到：

- FFmpeg worker timeout 分支不再 `handle.join()`。
- consumer timeout 分支不再 `handle.join()`。

但新增测试并没有真正触发 `RecvTimeoutError::Timeout`：

1. `ffmpeg_writer_drop_completes_quickly_when_worker_exits` 覆盖的是 drop/disconnect 后 worker 快速退出，不是 `recv_timeout` timeout 分支。
2. `extract_panic_message_handles_various_payloads` 覆盖 panic message helper。
3. `empty_consumer_output_produces_valid_defaults` 覆盖默认 output 构造，但没有触发 `join_consumer()` 的 timeout。

为什么重要：

1. 本轮 Phase C 的目标是“timeout 分支回归测试”。
2. BUG.md 规则 28 的关键安全性质是：timeout 后绝不能调用无界 join。
3. 当前实现代码看起来正确，但没有测试锁住真正危险分支。
4. 后续如果有人把 timeout 分支里的 `handle.join()` 加回来，现有测试不一定会失败。

建议修复：

推荐拆出可测试 helper，并允许注入 timeout duration：

```rust
fn join_worker_with_timeout(&mut self, timeout: Duration) -> AppResult<RecordingResult>
```

测试中使用极短 timeout 和 never-send channel：

```rust
let (_tx, rx) = std::sync::mpsc::channel::<AppResult<RecordingResult>>();
let handle = std::thread::spawn(|| std::thread::park());
// 调用 helper，断言在短时间内返回 timeout error，且不会 join 阻塞。
```

consumer 侧同理，可抽出：

```rust
fn join_consumer_with_timeout(&mut self, timeout: Duration)
```

建议新增测试：

1. `ffmpeg_writer_join_worker_timeout_returns_without_joining`
2. `ffmpeg_writer_finish_timeout_does_not_block_on_join`
3. `mac_recording_join_consumer_timeout_returns_without_joining`
4. `mac_recording_stop_timeout_preserves_cleanup_path`

### Important 4: HANDOFF 中验证结果与实际审查验证不一致

位置：

- `HANDOFF.md:100`
- `HANDOFF.md:101`
- `HANDOFF.md:102`

当前 HANDOFF 记录：

```text
- cargo test --manifest-path src-tauri/Cargo.toml 247 tests 通过
- npm test -- --run 52 tests 通过
- cargo fmt --check 通过
```

本次审查实际验证：

1. `npm run build` 失败。
2. `cargo test --manifest-path src-tauri/Cargo.toml` 本次全量运行出现 1 个时间敏感测试失败：
   - `core::clock::tests::audio_sample_clock_lazy_offset_anchors_first_callback_start`
3. 该失败单独复跑通过，且 `src-tauri/src/core/clock.rs` 本轮无 diff，因此更像既有 flaky 测试或并发负载下的时序风险。

为什么重要：

1. HANDOFF 是多轮交接文件，不能记录与当前实际门禁相矛盾的状态。
2. 当前至少需要标注：
   - 前端 build 未通过；
   - full cargo test 本次有 flaky/failure 观察；
   - targeted tests 通过；
   - npm test 通过。
3. 如果后续开发者只看 HANDOFF，可能误以为当前工作区已经可合并。

建议修复：

更新 HANDOFF 中本轮验证结果，至少改为：

```text
- cargo fmt --manifest-path src-tauri/Cargo.toml --check 通过
- 定向 Rust 回归测试通过
- npm test -- --run 52 tests 通过
- npm run build 未通过：App.tsx 缺少 writerDiagnostics/diagnostics
- cargo test --manifest-path src-tauri/Cargo.toml 本次观察到 core::clock 时间敏感测试失败；单独复跑通过，需后续稳定化或重新跑全量
```

同时，如果计划中要求 `cargo test --features ffmpeg` 和 `cargo clippy`，需要补跑并记录，或明确标注未执行。

## 6. Minor Findings

### Minor 1: `record_source_contribution()` trait 注释仍描述旧顺序

位置：

- `src-tauri/src/media/recording_writer.rs:290`
- `src-tauri/src/media/recording_writer.rs:291`

当前注释：

```rust
/// Record that the next push_audio() call contains data from the given sources.
/// Called by consume_frames() before each push_audio() to enable per-source tracking.
```

问题：

1. 实现已经改为 `push_audio()` 成功后调用。
2. 注释仍说 “before each push_audio()”。
3. 这与 `BUG.md` 规则 32 直接矛盾。

为什么重要：

- 这个 trait 是后续 writer 实现的调用约定文档。
- 注释错误会诱导后来者重新把调用顺序改回错误状态。

建议修复：

改为：

```rust
/// Record that a successfully enqueued audio chunk contained data from the given sources.
/// Must be called only after push_audio() succeeds so diagnostics do not count failed enqueue attempts.
```

### Minor 2: `App.test.tsx` 存在额外 TS mock 签名错误

位置：

- `src/App.test.tsx:896`

`npm run build` 同时报告：

```text
src/App.test.tsx(896,42): error TS2345:
Argument of type '(event: string, cb: (event: { payload: unknown; }) => void) => Promise<() => void>'
is not assignable to parameter of type ...
```

说明：

- 该错误不一定由本轮 diagnostics 改动直接引入。
- 但它会和 `App.tsx` 的 `RecordingResult` 缺字段问题一起阻塞 `npm run build`。

建议修复：

把 mock callback 类型对齐 Tauri `EventCallback<unknown>` 的完整事件结构，至少包含：

```ts
{
  event: string
  id: number
  payload: unknown
}
```

已有测试文件其他位置有类似写法，可复用同一模式。

## 7. BUG.md 预防规则复核

### 已满足或基本满足

1. **规则 28**：writer worker 和 consumer thread timeout 后绝不能调用无界 join。  
   实现层面已基本满足：timeout 分支不再 `join()`。

2. **规则 29**：drop ratio 分母必须使用 received + dropped。  
   上一轮整改已修复，本轮未破坏。

3. **规则 30**：mic stop diagnostics 必须在重建 capture 前写入 RecordingDiagnostics。  
   `write_sidecars()` 在 `reset_mic()` 前写入 `output.diagnostics.mic_stop_diagnostics`，实现层面满足。

4. **规则 31**：writer diagnostics 必须区分 per-source chunks received。  
   `WriterDiagnostics` 已有 `system_chunks_received_by_writer` / `mic_chunks_received_by_writer`。

5. **规则 32**：per-source writer counter 必须在 `push_audio()` 成功后递增。  
   本轮实现满足。

### 未完全满足

1. **规则 21**：蓝牙麦克风 stop 必须返回结构化 diagnostics，不能只靠 eprintln。  
   成功路径可返回，但失败路径仍会被 `Err(String)` 截断。需要结构化错误或 diagnostics event。

2. **规则 33**：`RecordingResult` 必须携带 `RecordingDiagnostics`，不能仅靠 eprintln 暴露诊断信息。  
   Rust 成功路径满足；前端 `App.tsx` 当前丢弃字段，失败路径仍不返回结构化 diagnostics，因此未完整满足。

3. **规则 28** 的测试覆盖部分。  
   实现层面满足，但 timeout 分支缺直接回归测试，不足以锁死未来回归。

## 8. 验证记录

### 8.1 通过

```bash
git diff --check
```

结果：通过。

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

结果：通过。

```bash
cargo test --manifest-path src-tauri/Cargo.toml consume_frames_does_not_increment -- --nocapture
```

结果：通过。

```bash
cargo test --manifest-path src-tauri/Cargo.toml consume_frames_increments_per_source -- --nocapture
```

结果：通过。

```bash
cargo test --manifest-path src-tauri/Cargo.toml recording_result_serializes_diagnostics_as_camel_case -- --nocapture
```

结果：通过。

```bash
cargo test --manifest-path src-tauri/Cargo.toml empty_consumer_output_produces_valid_defaults -- --nocapture
```

结果：通过。

```bash
npm test -- --run
```

结果：52 tests passed。

### 8.2 未通过

```bash
npm run build
```

结果：失败。

失败点：

1. `src/App.tsx:190` 缺少 `writerDiagnostics` / `diagnostics`。
2. `src/App.test.tsx:896` Tauri event mock callback 类型不匹配。

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

结果：本次全量运行失败 1 个测试。

失败点：

```text
core::clock::tests::audio_sample_clock_lazy_offset_anchors_first_callback_start
```

补充调查：

```bash
cargo test --manifest-path src-tauri/Cargo.toml audio_sample_clock_lazy_offset_anchors_first_callback_start -- --nocapture
```

结果：单独复跑通过。

判断：

- `src-tauri/src/core/clock.rs` 本轮无未提交 diff。
- 该失败更像真实时间 + 1ms 容忍度在并发测试负载下的 flaky 风险。
- 不应归因于本轮 diagnostics 整改代码，但当前全量门禁不能记录为稳定通过。

### 8.3 未执行

本次审查未执行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo clippy --manifest-path src-tauri/Cargo.toml
```

后续整改完成后应补跑，或在 HANDOFF / checklist 中明确标注未执行原因。

## 9. 建议整改顺序

### Phase A: 修复前端构建 blocker

目标：

- `npm run build` 至少不再因为 `RecordingResult` 缺字段失败。
- 成功 stop 返回的 diagnostics 不在 React state 层丢失。

建议步骤：

1. 修改 `src/App.tsx`，优先使用 `setRecordingResult(result)`。
2. 如果手动重组对象，则补齐 `writerDiagnostics` 和 `diagnostics`。
3. 更新 `App.test.tsx` 中 stop mock 的返回对象，补完整 diagnostics 字段。
4. 修复 `App.test.tsx:896` 的 event mock callback 类型。
5. 运行：

```bash
npm run build
npm test -- --run
```

### Phase B: 补齐 TS diagnostics 类型

目标：

- `src/lib/tauri.ts` 的 `RecordingDiagnostics` 与 Rust serde shape 对齐。

建议步骤：

1. 补齐 6 个 before-writer 字段。
2. Rust 序列化测试覆盖 before-writer camelCase 字段。
3. 前端测试或类型级 mock 保留这些字段。

### Phase C: 处理失败路径 diagnostics 暴露

目标：

- stop 失败时也能拿到结构化 diagnostics。

建议方案：

1. 设计 `StopRecordingResponse` 或结构化错误类型。
2. `MacRecordingService::stop()` / `RecordingFinalizeGuard::drive_state_machine()` 在有 errors 时保留 result/diagnostics。
3. Tauri command 不再只返回 `Err(String)`，或至少 emit structured diagnostics event。
4. 前端类型同步更新。
5. 新增失败路径测试。

### Phase D: 补 timeout 分支直接测试

目标：

- 直接触发 `RecvTimeoutError::Timeout`，验证不会无界 join。

建议步骤：

1. 抽出可注入 timeout duration 的 helper。
2. writer 侧用 never-send result channel + parked thread 覆盖 timeout。
3. consumer 侧用 never-send result channel + parked thread 覆盖 timeout。
4. 断言返回耗时小于测试 timeout 上限。

### Phase E: 文档与门禁收口

目标：

- HANDOFF 与实际验证结果一致。
- 计划要求的门禁全部补齐或明确标注未执行。

建议步骤：

1. 更新 `HANDOFF.md` 的验证结果，避免继续记录当前工作区“全量通过”。
2. 补跑：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo clippy --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

3. 如果 `core::clock` 测试仍 flaky，单独修复或记录为独立问题，不能把全量测试写成稳定通过。

## 10. 最终评估

**Ready to merge? No.**

理由：

1. 当前 `npm run build` 明确失败。
2. 新增 diagnostics 在前端成功路径被丢弃。
3. 失败路径仍不能结构化返回 diagnostics。
4. TypeScript diagnostics 类型不完整。
5. timeout 安全属性缺直接测试覆盖。
6. HANDOFF 当前验证记录与本次实际门禁结果不一致。

建议完成第 9 节的 Phase A-D 后再重新执行完整门禁，并发起下一轮复审。
