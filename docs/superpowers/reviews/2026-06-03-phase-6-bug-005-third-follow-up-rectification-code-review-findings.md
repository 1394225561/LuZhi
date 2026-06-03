# Phase 6 BUG-005 Third Follow-up Rectification Code Review Findings

> 日期：2026-06-03
> 审查结论：**Ready to merge? No**

## 1. 审查输入

本次重点核查目标：

1. 前端是否真正透传并保留 `writerDiagnostics` / `diagnostics` / `finalizationErrors`。
2. TypeScript `RecordingDiagnostics` 是否与 Rust serde shape 对齐，尤其是 before-writer 字段。
3. 失败路径是否保留结构化 diagnostics，同时保持清晰的失败语义。
4. timeout 分支是否既实现 bounded 语义，又有足够强的回归测试锁定生产分支。
5. 是否遵循 `BUG.md` 中 BUG-005 / BUG-005_2 的预防规则，尤其是规则 28、32、33、34。

## 3. 总体结论

本轮整改关闭了上一轮 review 中的大部分明确阻塞点：

1. `npm run build` 已恢复通过。
2. `App.tsx` 已将新增 diagnostics 字段写入 `RecordingResult` state。
3. TypeScript `RecordingDiagnostics` 已补齐 6 个 before-writer 字段。
4. `CpalMicrophoneStopDiagnostics` 已序列化为 camelCase。
5. per-source writer counter 已改为只在 `push_audio()` 成功后递增。
6. writer / consumer timeout 分支实现上已避免 timeout 后调用无界 `join()`。
7. Rust 和前端自动化测试均通过。

但当前仍不建议合并，原因是剩余问题集中在两个关键质量点：

1. **hard finalize failure 的 API / UI 失败语义不清晰**：后端有错误时把状态机置为 `Failed`，但 command 仍 `Ok(result)`；前端将 `finalizationErrors` 当作 warning 仅 `console.warn`。这会让真正的 finalize 失败在 command 层表现为成功。
2. **timeout 回归测试没有打到生产 timeout 分支**：实现代码按静态检查是正确的，但测试只验证标准库模式，不能防止后续把 `handle.join()` 放回生产 timeout 分支。

## 4. Strengths

### 4.1 前端构建 blocker 已关闭

位置：

- `src/App.tsx:190`
- `src/App.tsx:199`
- `src/App.tsx:200`
- `src/App.tsx:201`
- `src/App.test.tsx:175`
- `src/App.test.tsx:476`
- `src/App.test.tsx:477`
- `src/App.test.tsx:478`
- `src/App.test.tsx:581`

当前 `handleStopRecording()` 在保存 `RecordingResult` 时已经带上：

```ts
writerDiagnostics: result.writerDiagnostics,
diagnostics: result.diagnostics,
finalizationErrors: result.finalizationErrors ?? [],
```

评审结论：

- 上一轮 Critical 1 的 TypeScript 构建错误已修复。
- stop mock 也补齐了新增字段，前端测试不再依赖不完整 payload。
- `npm run build` 本次审查通过。

### 4.2 TypeScript diagnostics shape 已补齐 before-writer 字段

位置：

- `src/lib/tauri.ts:123`
- `src/lib/tauri.ts:141`
- `src/lib/tauri.ts:142`
- `src/lib/tauri.ts:143`
- `src/lib/tauri.ts:144`
- `src/lib/tauri.ts:145`
- `src/lib/tauri.ts:146`

新增字段：

```ts
systemRmsMaxBeforeWriter: number
micRmsMaxBeforeWriter: number
systemWindowsBeforeWriter: number
micWindowsBeforeWriter: number
systemFramesBeforeWriter: number
micFramesBeforeWriter: number
```

评审结论：

- 这些字段与 Rust `RecordingDiagnostics` 的 serde camelCase 输出一致。
- 这关闭了上一轮 Important 2。
- 符合 `BUG.md` BUG-005_2 预防规则 6：before-writer diagnostics 必须记录每个源的 windows、frames、RMS。

### 4.3 mic stop diagnostics camelCase 已修复

位置：

- `src-tauri/src/platform/macos/cpal_microphone.rs:16`
- `src/lib/tauri.ts:150`

当前 `CpalMicrophoneStopDiagnostics` 已添加：

```rust
#[serde(rename_all = "camelCase")]
```

评审结论：

- Rust `stop_requested` / `callbacks_after_stop` 等字段会序列化为前端期望的 `stopRequested` / `callbacksAfterStop`。
- 这修复了后端结构体与 TypeScript 类型不一致的问题。

### 4.4 per-source writer counter 时序正确

位置：

- `src-tauri/src/platform/macos_service.rs:793`
- `src-tauri/src/platform/macos_service.rs:798`
- `src-tauri/src/platform/macos_service.rs:800`
- `src-tauri/src/platform/macos_service.rs:940`
- `src-tauri/src/platform/macos_service.rs:945`
- `src-tauri/src/platform/macos_service.rs:947`
- `src-tauri/src/media/recording_writer.rs:295`

当前实现：

- 先调用 `writer.push_audio(...)`。
- 只有 `push_audio()` 成功后才调用 `writer.record_source_contribution(...)`。
- trait 注释也已改为“successfully enqueued audio chunk”。

评审结论：

- 符合 `BUG.md` 规则 32：per-source writer counter 必须在 `push_audio()` 成功后递增，不能在 push 前递增。
- 新增测试覆盖：
  - `consume_frames_does_not_increment_per_source_writer_counter_when_push_audio_fails`
  - `consume_frames_increments_per_source_writer_counter_on_successful_push`

### 4.5 drop ratio 分母仍保持正确

位置：

- `src-tauri/src/platform/macos_service.rs:1003`
- `src-tauri/src/platform/macos_service.rs:1023`

当前 system / mic drop ratio 均使用：

```rust
received + dropped
```

作为 attempted total。

评审结论：

- 符合 `BUG.md` 规则 29。
- 现有边界测试覆盖 9.09% pass 与 10.71% fail。

### 4.6 timeout 实现代码按静态检查是 bounded 的

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:104`
- `src-tauri/src/media/ffmpeg_writer.rs:112`
- `src-tauri/src/media/ffmpeg_writer.rs:116`
- `src-tauri/src/platform/macos_service.rs:403`
- `src-tauri/src/platform/macos_service.rs:411`
- `src-tauri/src/platform/macos_service.rs:415`

当前实现：

- `FfmpegRecordingWriter::join_worker()` timeout 后 `self.worker.take()`，不调用 `handle.join()`。
- `RecordingFinalizeGuard::join_consumer()` timeout 后 `self.service.consumer_handle.take()`，不调用 `handle.join()`。

评审结论：

- 实现层符合 `BUG.md` 规则 28：timeout 后绝不能调用无界 join。
- 但测试强度仍不足，详见 Important 2。

## 5. Critical Findings

未发现 Critical 级别问题。

本轮没有发现会直接导致当前代码无法构建、无法运行基本测试、或明显引入数据流红线违规的问题。

## 6. Important Findings

### Important 1: hard finalize failure 被包装成 `Ok(RecordingResult)`，前端又按 warning 处理

位置：

- `src-tauri/src/platform/macos_service.rs:553`
- `src-tauri/src/platform/macos_service.rs:562`
- `src-tauri/src/platform/macos_service.rs:564`
- `src-tauri/src/platform/macos_service.rs:567`
- `src-tauri/src/platform/macos_service.rs:569`
- `src-tauri/src/lib.rs:255`
- `src-tauri/src/lib.rs:278`
- `src/App.tsx:203`

当前行为：

`drive_state_machine()` 在 `self.errors` 非空时：

1. 把错误写入 `result.finalization_errors`。
2. 尝试 `state_machine.stop()`。
3. 调用 `state_machine.fail()`。
4. 仍然 `Ok(result)` 返回。

对应代码形态：

```rust
if self.errors.is_empty() {
    ...
} else {
    result.finalization_errors = self.errors.clone();
    let _ = self.service.state_machine.stop();
    self.service.state_machine.fail();
}
Ok(result)
```

随后 `stop_recording()` command：

```rust
emit_state_changed(&app, new_state);
result.map_err(|e| e.to_string())
```

因为 `service.stop()` 返回 `Ok(result)`，所以 Tauri command resolve 成功；但 `new_state` 是 `Failed`，前端也会收到 failed state event。

前端当前处理：

```ts
if (result.finalizationErrors && result.finalizationErrors.length > 0) {
  console.warn('录制完成但有警告:', result.finalizationErrors)
}
```

问题：

- `finalizationErrors` 里可能不是软 warning，而是 hard failure：
  - `writer.push_audio()` 失败；
  - `writer.finish()` 失败；
  - high drop ratio hard fail；
  - source-aware contract failure；
  - artifact contract failure；
  - consumer timeout；
  - capture stop / mic stop failure；
  - sidecar write failure。
- 后端状态机把 session 标成 `Failed`，但 command 层却表现为 `Ok`。
- 前端把这些错误文案标为“录制完成但有警告”，并且只写 `console.warn`，用户界面拿不到具体失败详情。
- 前端状态存在竞态：
  - state event 到达时，UI 会进入 `failed` 且显示通用错误“录制过程中发生错误”；
  - command result 到达时，`recordingResult` 已被保存；
  - fallback 查询如果看到 `failed`，不会进入 preview；
  - 如果 failed event 丢失，`handleStopRecording()` 也不会进入 preview，但可能保持 recording 态直到后续状态同步。

为什么重要：

1. 违反 AGENTS / 项目约定中的“Fail visibly, not silently”：不能绕过错误仍报告成功。
2. 弱化 `BUG.md` 规则 34 的本意。规则 34 要求 stop 失败时保留 `finalization_errors` 和完整 diagnostics，但不是把 hard failure 改成成功。
3. 后续排查 BUG-005 时，最有价值的结构化错误没有稳定进入 UI 错误态。
4. 产品层可能把“录制完成但有警告”误解为 artifact 可用，进而继续导出或展示不完整结果。

建议修复：

推荐方案 A：定义显式结构化 stop response。

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopRecordingResponse {
    pub result: RecordingResult,
    pub failed: bool,
    pub finalization_errors: Vec<String>,
}
```

语义：

- command 永远返回结构化 response，除非 stop command 自身无法执行（锁损坏、spawn_blocking panic 等）。
- `failed=true` 表示 finalize hard failure，前端必须进入 failed UI。
- `result` 始终携带 `diagnostics` / `writerDiagnostics` / `finalizationErrors`。

推荐方案 B：定义结构化错误 payload。

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopRecordingError {
    pub message: String,
    pub result: RecordingResult,
}
```

语义：

- `stop_recording()` 对 hard failure 返回 rejected command。
- rejected payload 仍携带完整 result diagnostics。
- 前端 catch 分支解析结构化错误并显示具体错误。

无论选 A 还是 B，都建议前端：

1. 对非空 `finalizationErrors` 展示具体错误详情，不只 `console.warn`。
2. 保存 diagnostics 供后续错误页 / 日志 / debug 面板消费。
3. 不把 hard failure 文案写成“警告”。

建议新增测试：

1. Rust：
   - `stop_finalize_error_returns_structured_diagnostics`
   - `stop_with_consumer_errors_marks_failed_but_preserves_result`
   - `push_audio_failure_stop_response_contains_writer_push_audio_failures`
2. Frontend：
   - `shows finalize error details when stop returns finalizationErrors`
   - `does not enter preview when stop response failed is true`
   - `preserves diagnostics on finalize failure`

严重级别：Important。

理由：这不是当前构建 blocker，但它影响 Phase 6 BUG-005 整改最核心的失败诊断与用户可见语义。

### Important 2: timeout 回归测试没有真正覆盖生产 timeout 分支

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:94`
- `src-tauri/src/media/ffmpeg_writer.rs:104`
- `src-tauri/src/media/ffmpeg_writer.rs:112`
- `src-tauri/src/media/ffmpeg_writer.rs:1749`
- `src-tauri/src/media/ffmpeg_writer.rs:1779`
- `src-tauri/src/platform/macos_service.rs:398`
- `src-tauri/src/platform/macos_service.rs:403`
- `src-tauri/src/platform/macos_service.rs:411`
- `src-tauri/src/platform/macos_service.rs:2265`

当前实现代码：

- `join_worker()` 确实使用 `recv_timeout(WORKER_RESULT_TIMEOUT)`。
- `join_worker()` timeout 分支没有 `handle.join()`。
- `join_consumer()` 确实使用 `recv_timeout(CONSUMER_RESULT_TIMEOUT)`。
- `join_consumer()` timeout 分支没有 `handle.join()`。

当前新增测试：

- `recv_timeout_returns_quickly_on_never_send_channel`
- `detached_thread_does_not_block_on_drop`
- `recv_timeout_consumer_returns_quickly_on_never_send_channel`
- `consumer_timeout_fallback_preserves_diagnostics_slot`

问题：

- `recv_timeout_returns_quickly_on_never_send_channel` 只验证标准库 `Receiver::recv_timeout()` 会 timeout。
- `detached_thread_does_not_block_on_drop` 只验证 drop `JoinHandle` 不阻塞。
- consumer 侧测试同样只验证标准库 channel 行为和 fallback output shape。
- 这些测试没有调用：
  - `FfmpegRecordingWriter::join_worker()` 的 timeout 分支；
  - `FfmpegRecordingWriter::finish()` 在 worker stuck 时的 timeout 行为；
  - `RecordingFinalizeGuard::join_consumer()` 的 timeout 分支。

为什么重要：

1. `BUG.md` 规则 28 的风险点不是标准库 `recv_timeout()`，而是生产 timeout 分支中有人再次加入无界 `handle.join()`。
2. 现在的测试无法锁住生产代码。后续如果把 `handle.join()` 放回 `join_worker()` timeout 分支，这些测试仍可能全部通过。
3. Phase 6 本轮整改目标写的是“timeout 分支直接回归测试”，当前测试更接近“timeout 模式验证”，不是生产分支回归。

建议修复：

提取可测试 helper，使 timeout duration 可注入。

writer 侧建议形态：

```rust
fn join_worker_with_timeout(
    worker: &mut Option<thread::JoinHandle<()>>,
    rx: mpsc::Receiver<AppResult<RecordingResult>>,
    timeout: Duration,
) -> AppResult<RecordingResult>
```

生产调用：

```rust
self.join_worker_with_timeout(WORKER_RESULT_TIMEOUT)
```

测试调用：

```rust
let (_tx, rx) = mpsc::channel::<AppResult<RecordingResult>>();
let handle = thread::spawn(|| thread::park());
let mut worker = Some(handle);

let start = Instant::now();
let result = join_worker_with_timeout(&mut worker, rx, Duration::from_millis(50));

assert!(result.is_err());
assert!(start.elapsed() < Duration::from_secs(1));
assert!(worker.is_none());
```

consumer 侧建议类似抽取：

```rust
fn join_consumer_with_timeout(&mut self, timeout: Duration)
```

或提取纯 helper：

```rust
fn receive_consumer_output_with_timeout(
    rx: mpsc::Receiver<RecordingConsumerOutput>,
    handle: &mut Option<JoinHandle<()>>,
    timeout: Duration,
) -> (RecordingConsumerOutput, bool, Vec<String>)
```

建议新增测试：

1. `join_worker_timeout_returns_without_joining_parked_worker`
2. `finish_timeout_returns_recording_write_failed_without_blocking`
3. `join_consumer_timeout_detaches_parked_consumer`
4. `join_consumer_timeout_records_error_and_preserves_empty_output`

测试成功标准：

- 测试必须调用生产 helper，不只调用 `rx.recv_timeout()`。
- elapsed 必须明显小于 10s / 15s 生产 timeout。
- timeout 后 `JoinHandle` 被 take/drop，不能保留可被后续无界 join 的 handle。
- 返回错误或 fallback output 必须携带结构化错误路径。

严重级别：Important。

理由：实现当前是正确的，但测试没有锁住最危险回归点。BUG-005 的阻塞/挂死问题属于高风险路径，应要求生产分支级别测试。

## 7. Minor Findings

### Minor 1: TypeScript 把 `finalizationErrors` 声明为必填，但 Rust 空数组会省略

位置：

- `src-tauri/src/media/recording_writer.rs:285`
- `src-tauri/src/media/recording_writer.rs:846`
- `src-tauri/src/media/recording_writer.rs:863`
- `src/lib/tauri.ts:103`
- `src/App.tsx:201`

当前 Rust：

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub finalization_errors: Vec<String>,
```

并且测试 `recording_result_omits_empty_finalization_errors` 明确要求空数组不序列化。

当前 TypeScript：

```ts
finalizationErrors: string[]
```

问题：

- 对成功 stop 结果，Rust JSON 可以没有 `finalizationErrors` 字段。
- TS 类型却告诉所有调用者该字段必定存在。
- `App.tsx` 当前使用 `result.finalizationErrors ?? []` 做了局部兜底，但类型定义仍不诚实。
- 其他未来调用者可能直接读 `result.finalizationErrors.length` 并在成功路径报错。

建议修复：

二选一：

方案 A：API 永远返回数组。

```rust
#[serde(default)]
pub finalization_errors: Vec<String>,
```

然后删除“empty omitted”的测试，改为断言空数组存在。

方案 B：TypeScript 标为可选。

```ts
finalizationErrors?: string[]
```

并在 `stopRecording()` 边界归一化：

```ts
export async function stopRecording(): Promise<RecordingResult> {
  const result = await invoke<RecordingResult>('stop_recording')
  return {
    ...result,
    finalizationErrors: result.finalizationErrors ?? [],
  }
}
```

本仓库若偏向“前后端契约显式”，更推荐方案 A：让 Rust 始终序列化空数组，减少前端可选字段扩散。

严重级别：Minor。

理由：当前 `App.tsx` 已局部兜底，不是立即可见 bug；但类型契约不一致会制造后续维护风险。

## 8. BUG.md 规则符合性核查

### 已符合

- 规则 28：实现代码中 writer / consumer timeout 后没有调用无界 `join()`。
- 规则 29：drop ratio 分母使用 `received + dropped`。
- 规则 30：mic stop diagnostics 在重建 capture 前写入 `RecordingDiagnostics`。
- 规则 31：writer diagnostics 区分 `system_chunks_received_by_writer` / `mic_chunks_received_by_writer`。
- 规则 32：per-source writer counter 已在 `push_audio()` 成功后递增。
- 规则 33：`RecordingResult` 成功路径携带 `RecordingDiagnostics`。
- 规则 34：失败路径开始携带 `finalization_errors` 和 diagnostics，不再只丢到 `Err(String)`。

### 仍需补强

- 规则 28：需要生产 timeout 分支级回归测试，而不是仅验证标准库 timeout 模式。
- 规则 34：需要明确 hard failure 的 API 语义。保留 diagnostics 是必要条件，但不能因此把 hard failure 表达成 command success + warning。

## 9. 验证结果

本次审查实际执行并通过：

```bash
git diff --check 9aa436fa51392bea523d1910621d0c3653872353..2ccd85dfd90f621a4d3e144b0b0a2cbd95e0fae1
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm run build
npm test -- --run
```

结果摘要：

- `git diff --check`：通过。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml`：251 passed。
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`：通过；包含 308 unit tests 与 integration tests。
- `npm run build`：通过。
- `npm test -- --run`：52 passed。

备注：

- Rust 测试输出中存在既有 warnings，例如 `screen_capture_kit.rs` 的 FFI 命名 warning、`RecordingConsumerOutput` private interface warning 等。本次 review 未将这些列为整改 blocker，因为它们不属于本轮 diff 的核心行为回归。
- 当前工作区仍存在若干未提交文档改动和 untracked review/plan 文档；本次审查没有修改或回退这些用户已有工作。

## 10. 建议整改 Phase

### Phase A: 修正 stop/finalize 失败语义

目标：

- hard finalize failure 必须 visibly fail。
- 前端仍能拿到完整 diagnostics。

建议任务：

1. 设计并实现 `StopRecordingResponse` 或 `StopRecordingError`。
2. 后端将 `finalizationErrors` 与 `failed` 语义显式化。
3. 前端失败页显示具体 `finalizationErrors`。
4. 保留 `recordingResult.diagnostics` 供后续错误诊断。

验收：

- writer finish failure 不会进入 preview。
- UI 展示具体 finalize error，而不是通用“录制过程中发生错误”。
- diagnostics / writerDiagnostics 在失败场景仍可访问。

### Phase B: 加强生产 timeout 分支测试

目标：

- 测试必须调用生产 timeout helper。
- 防止 timeout 分支重新引入无界 `join()`。

建议任务：

1. 抽取 writer join timeout helper 并允许注入短 timeout。
2. 抽取 consumer join timeout helper并允许注入短 timeout。
3. 使用 never-send channel + parked thread 触发真实 timeout 分支。
4. 断言返回时间 bounded、handle 被 detach、错误/fallback 信息保留。

验收：

- 如果在生产 timeout 分支加入 `handle.join()`，测试会挂住或失败。
- 测试时间不依赖 10s / 15s 生产 timeout。

### Phase C: 对齐 `finalizationErrors` 类型契约

目标：

- Rust serde shape 与 TypeScript 类型完全一致。

建议任务：

1. 若保留 TS 必填数组，则 Rust 不再 skip empty `finalizationErrors`。
2. 若保留 Rust skip empty，则 TS 改为可选并在 command boundary 归一化。
3. 更新对应 Rust / frontend tests。

验收：

- 成功路径和失败路径的 `RecordingResult` 类型都与真实 JSON 一致。

## 11. Assessment

**Ready to merge? No**

理由：

核心实现方向正确，自动化门禁也已通过；但 Phase 6 BUG-005 整改的目标不只是“测试绿”，还包括失败路径可诊断、可见且语义可信。当前 hard finalize failure 会以 `Ok(RecordingResult)` 返回并在前端被称为 warning，这会误导后续 UI 和排障流程。timeout 实现本身已修正，但测试还没有锁住生产 timeout 分支，无法防止同类阻塞 bug 回归。

建议完成 Important 1 和 Important 2 后再合并；Minor 1 可与 Important 1 一并收口。
