# Phase 6 BUG-005 Follow-up Rectification Code Review

> 日期：2026-06-03
> 审查对象：Phase 6 follow-up rectification commit `9aa436fa51392bea523d1910621d0c3653872353`
> Git range：`5c505770660abb97d86ece1e20deaeaddef6cfd4..9aa436fa51392bea523d1910621d0c3653872353`

## 1. 总体结论

**Ready to merge? With fixes / Not yet.**

本轮整改把上一份 review 中最危险的两个 bounded finalize 问题基本关掉了：

1. `FfmpegRecordingWriter::join_worker()` 的 timeout 分支不再调用无界 `handle.join()`。
2. `RecordingFinalizeGuard::join_consumer()` 的 timeout 分支不再调用无界 `handle.join()`，fallback 分支也不再直接 join。
3. system/mic drop ratio 分母已改为 `received + dropped`，并补了 9.09% pass、10.71% fail 的边界测试。
4. `CpalMicrophoneCapture::stop_with_diagnostics()` 已新增，且 mic capture 重建前会先保存 diagnostics。
5. `WriterDiagnostics` 已新增 per-source counters，`validate_source_aware_audio_contract()` 也补了 per-source writer 检查。

但是仍有两个 Important 问题需要先修复：

1. per-source writer counter 在 `push_audio()` 之前递增，可能把“尝试送入 writer”误报成“writer 已收到”。
2. mic stop diagnostics 目前只进入内部 `RecordingConsumerOutput.diagnostics` 并被日志打印，最终没有进入 `RecordingResult` / Tauri stop 返回值，仍不是上层可消费的结构化结果。

因此本轮不再有新的 Critical blocker，但还不建议直接合并。建议先按本文第 6 节完成一个小型 follow-up rectification，再重新跑完整门禁。

## 2. 审查范围

### 2.1 重点代码文件

- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/platform/macos_service.rs`
- `src-tauri/src/platform/macos/cpal_microphone.rs`
- `src-tauri/src/media/recording_writer.rs`
- `src-tauri/src/core/media_channel.rs`
- `src-tauri/src/app/recording_service.rs`
- `src-tauri/src/platform/macos/screen_capture_kit.rs`

### 2.2 重点对照文档

- `HANDOFF.md`
- `BUG.md`
- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md`
- `docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md`
- `docs/superpowers/plans/2026-06-03-phase-6-bug-005-follow-up-rectification.md`
- `docs/superpowers/reviews/2026-06-03-phase-6-bug-005-current-rectification-code-review.md`

## 3. 已确认关闭的问题

### 3.1 FFmpeg writer timeout 后不再无界 join

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:94`
- `src-tauri/src/media/ffmpeg_writer.rs:104`
- `src-tauri/src/media/ffmpeg_writer.rs:112`

当前 timeout 分支行为：

1. `rx.recv_timeout(WORKER_RESULT_TIMEOUT)` 超时后直接进入 `RecvTimeoutError::Timeout` 分支。
2. 分支内只执行 `self.worker.take()`，丢弃 `JoinHandle` 让 worker detached。
3. 不再调用 `handle.join()`。
4. 直接返回 `AppError::RecordingWriteFailed`，错误文案包含 worker 超时信息。

评审结论：

- 代码层面符合 BUG.md 规则 28：“timeout 后绝不能调用无界 join()”。
- `Disconnected` 分支保留 join 是可接受的，因为 result channel 已断开通常意味着 worker 已退出或 panic，此时 join 不会等待正常执行中的 worker。

剩余风险：

- 新增测试 `ffmpeg_writer_drop_completes_quickly_when_worker_exits` 只覆盖 drop writer 导致 channel disconnect 的快速退出，不是真正的 `recv_timeout` 分支。详见 Minor Finding 1。

### 3.2 Consumer timeout 后不再无界 join

位置：

- `src-tauri/src/platform/macos_service.rs:394`
- `src-tauri/src/platform/macos_service.rs:404`
- `src-tauri/src/platform/macos_service.rs:434`

当前 timeout / fallback 行为：

1. `rx.recv_timeout(CONSUMER_RESULT_TIMEOUT)` 超时后直接丢弃 `consumer_handle`。
2. 不再调用 `handle.join()`。
3. 使用 `empty_output` 继续执行 `write_sidecars()`、`reset_mic()`、`collect_errors()`、`drive_state_machine()`。
4. fallback 分支中 “没有 result channel 但仍有 handle” 也不再无界 join，而是记录状态不一致错误并丢弃 handle。

评审结论：

- 代码层面符合 BUG.md 规则 23 / 28。
- stop 路径具备真正 bounded 语义：consumer 卡在 writer finalize、artifact validation、metadata generation 时，主 stop 路径不会永久等待。

剩余风险：

- 当前没有直接模拟 consumer timeout 的测试。详见 Minor Finding 1。

### 3.3 Drop ratio 分母修正

位置：

- `src-tauri/src/platform/macos_service.rs:983`
- `src-tauri/src/platform/macos_service.rs:1003`

当前实现：

```rust
let attempted = diagnostics
    .system_chunks_received
    .saturating_add(diagnostics.system_chunks_dropped)
    .max(1) as f64;
let ratio = diagnostics.system_chunks_dropped as f64 / attempted;
```

mic 分支同理。

评审结论：

- 分母已从 `received` 修正为 `received + dropped`。
- 阈值策略为 `ratio > 0.10` hard fail，符合“少量 drop warning，大于 10% hard fail”的语义。
- 新增边界测试覆盖：
  - `consume_frames_passes_when_attempted_drop_ratio_below_10_percent`
  - `consume_frames_fails_when_attempted_drop_ratio_above_10_percent`

### 3.4 Source-aware channel drop logging

位置：

- `src-tauri/src/core/media_channel.rs:19`
- `src-tauri/src/core/media_channel.rs:54`
- `src-tauri/src/platform/macos_service.rs:188`
- `src-tauri/src/platform/macos_service.rs:189`
- `src-tauri/src/platform/macos_service.rs:207`

当前实现：

1. `bounded_media_channel()` 增加 `source: &'static str` 参数。
2. `MediaSender::try_send_drop_newest()` 在第 1 次和每 100 次 drop 时打印 source-aware warning。
3. macOS 主链路为 video/system/mic 分别传入 `"video"`、`"system"`、`"mic"`。

评审结论：

- 符合 BUG.md 规则 24。
- CPAL callback 处已不再使用 `let _ = sink.try_send_drop_newest(chunk)`，符合 BUG.md 规则 25。

## 4. Important Findings

### Important 1: per-source writer counter 在 push_audio 之前递增，可能产生 false evidence

位置：

- `src-tauri/src/platform/macos_service.rs:776`
- `src-tauri/src/platform/macos_service.rs:783`
- `src-tauri/src/platform/macos_service.rs:917`
- `src-tauri/src/platform/macos_service.rs:924`
- `src-tauri/src/media/ffmpeg_writer.rs:264`
- `src-tauri/src/media/recording_writer.rs:247`

当前逻辑：

```rust
writer.record_source_contribution(
    synced.has_system,
    synced.has_mic,
    synced.system_frames,
    synced.mic_frames,
);
if let Err(e) = writer.push_audio(synced.mixed) {
    diagnostics.writer_push_audio_failures += 1;
    ...
} else {
    diagnostics.mixed_chunks_queued += 1;
}
```

问题：

1. `record_source_contribution()` 在 `push_audio()` 之前调用。
2. `FfmpegRecordingWriter::record_source_contribution()` 会立即递增：
   - `system_chunks_received_by_writer`
   - `mic_chunks_received_by_writer`
3. 如果 `push_audio()` 因 encoder queue full、channel disconnected、format validation failure 等原因失败，writer worker 实际没有收到该 chunk。
4. 但最终 `WriterDiagnostics` 仍会显示该 source “received by writer”。

为什么重要：

1. 字段名和 contract 语义都是 writer-level evidence：`system_chunks_received_by_writer` / `mic_chunks_received_by_writer`。
2. 现在记录的是 “consumer 尝试提交给 writer”，不是 “writer 已成功接收”。
3. 这会削弱 BUG.md 规则 31：“writer diagnostics 必须区分 per-source chunks received，不能只用 aggregate。”
4. 在失败定位时，日志可能误导：
   - before-writer 有 source；
   - writer per-source counter 也非零；
   - 但 chunk 实际没有成功入队 worker。
5. 虽然 `push_audio()` 失败会进入 `errors`，stop 通常会失败，但 diagnostics 本身仍然不可信；后续排查会更难区分 “writer enqueue 失败” 和 “writer worker 收到后丢弃”。

建议修复：

1. 最小修复：把 `record_source_contribution()` 移到 `push_audio()` 成功分支之后。

```rust
match writer.push_audio(synced.mixed) {
    Ok(()) => {
        writer.record_source_contribution(
            synced.has_system,
            synced.has_mic,
            synced.system_frames,
            synced.mic_frames,
        );
        diagnostics.mixed_chunks_queued += 1;
    }
    Err(e) => {
        diagnostics.writer_push_audio_failures += 1;
        ...
    }
}
```

2. 更稳妥的设计：新增一个单一 writer API，例如 `push_audio_with_source_contribution(synced)`，让 writer 在成功 enqueue 后原子地记录 per-source contribution，避免调用顺序错误。
3. 字段命名可考虑更精确：
   - 如果记录成功 enqueue：`system_chunks_enqueued_to_writer`
   - 如果记录 worker thread 实际收到：需要把 source metadata 传进 `EncoderMessage::Audio`，在 worker 处理 `EncoderMessage::Audio` 时递增。

建议新增测试：

1. `consume_frames_does_not_increment_per_source_writer_counter_when_push_audio_fails`
2. `ffmpeg_writer_per_source_counter_tracks_successful_audio_enqueue_only`
3. 若采用 worker-side 方案，再补 `encoder_worker_counts_per_source_after_receive`

### Important 2: mic stop diagnostics 没有进入最终 stop 返回值，仍不是上层可消费的结构化结果

位置：

- `src-tauri/src/platform/macos/cpal_microphone.rs:78`
- `src-tauri/src/platform/macos_service.rs:341`
- `src-tauri/src/platform/macos_service.rs:450`
- `src-tauri/src/platform/macos_service.rs:525`
- `src-tauri/src/media/recording_writer.rs:56`
- `src-tauri/src/media/recording_writer.rs:268`
- `src-tauri/src/lib.rs:238`
- `src/lib/tauri.ts:91`

当前逻辑：

1. `CpalMicrophoneCapture::stop_with_diagnostics()` 已返回 `CpalMicrophoneStopDiagnostics`。
2. `RecordingFinalizeGuard::stop_captures()` 调用 `stop_with_diagnostics()` 并暂存到 `self.mic_stop_diag`。
3. `write_sidecars()` 在 reset mic capture 前写入 `output.diagnostics.mic_stop_diagnostics`。
4. `output.diagnostics` 会被 `eprintln!` 打印。
5. `drive_state_machine()` 最终只取 `output.result` 并返回 `RecordingResult`。
6. `RecordingResult` 没有 `diagnostics` 或 `mic_stop_diagnostics` 字段。
7. Tauri command `stop_recording()` 返回 `Result<RecordingResult, String>`。
8. 前端 `RecordingResult` 类型也没有 diagnostics 字段。

问题：

1. diagnostics 已经在 capture 重建前保存，但只保存在内部 `RecordingConsumerOutput.diagnostics`，最终没有暴露出去。
2. UI、自动化测试、manual gate 记录、后续 bug triage 都拿不到结构化 `pause_attempted` / `pause_ok` / `stream_dropped` / `callbacks_after_stop` / `stop_wait_ms`。
3. 当前仍接近 “只靠 eprintln”，没有完全满足 BUG.md 规则 21 的目标。
4. BUG.md 规则 30 写的是 “mic stop diagnostics 必须在重建 capture 前写入 RecordingDiagnostics”，这一点做到了；但原 review 和整改目标强调的是“结构化返回/持久保留”，当前还没有完成返回层面的闭环。

为什么重要：

1. 蓝牙 HFP profile release 是真实设备问题，仅靠终端日志不利于后续手工验收记录和自动化采集。
2. 一旦 stop 失败，错误字符串里也不会携带完整 diagnostics；成功返回时前端也看不到 diagnostics。
3. 这会降低后续定位 “pause 失败” / “drop stream 后仍有 callback” / “等待时间不足” 的效率。

建议修复：

1. 给 `RecordingResult` 增加：

```rust
pub diagnostics: RecordingDiagnostics,
```

或最小字段：

```rust
pub mic_stop_diagnostics: Option<CpalMicrophoneStopDiagnostics>,
```

2. 在 `drive_state_machine()` 中返回前写入：

```rust
let mut result = output.result;
result.diagnostics = output.diagnostics;
```

3. 同步更新前端类型 `src/lib/tauri.ts`：

```ts
export type CpalMicrophoneStopDiagnostics = {
  stopRequested: boolean
  streamExisted: boolean
  pauseAttempted: boolean
  pauseOk: boolean
  pauseError: string | null
  streamDropped: boolean
  callbacksAfterStop: number
  stopWaitMs: number
}
```

4. 如果担心 `RecordingResult` 太大，也可以先仅暴露 `micStopDiagnostics`，后续再统一暴露完整 recording diagnostics。
5. stop 失败路径如果仍返回 `Err(String)`，建议把 diagnostics 至少写入一个 structured sidecar 或在错误结构中保留，避免失败时丢诊断。

建议新增测试：

1. `recording_result_serializes_mic_stop_diagnostics_as_camel_case`
2. `mac_recording_stop_preserves_mic_stop_diagnostics_after_capture_reset`
3. `stop_recording_returns_mic_stop_diagnostics_when_microphone_requested`
4. `cpal_stop_with_diagnostics_no_active_stream_sets_stop_requested`

## 5. Minor Findings

### Minor 1: bounded timeout 分支缺少直接回归测试

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:104`
- `src-tauri/src/media/ffmpeg_writer.rs:1696`
- `src-tauri/src/platform/macos_service.rs:394`

当前测试情况：

1. `ffmpeg_writer_drop_completes_quickly_when_worker_exits` 验证的是 writer 被 drop 后 tx 断开，worker 能快速退出。
2. 该测试没有触发 `join_worker()` 的 `RecvTimeoutError::Timeout` 分支。
3. consumer timeout 路径也没有一个真实超时测试。

为什么重要：

1. 本轮最关键的安全要求就是 “timeout 后不再 join”。
2. 当前只能靠代码 review 发现是否有人把 timeout 分支改回 `handle.join()`。
3. 未来重构时缺少测试会增加回归风险。

建议修复：

1. 将 timeout duration 抽为 test-only 可配置常量：

```rust
#[cfg(test)]
const WORKER_RESULT_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const WORKER_RESULT_TIMEOUT: Duration = Duration::from_secs(10);
```

2. 或抽出纯 helper：

```rust
fn wait_worker_result_with_timeout(
    rx: Receiver<AppResult<RecordingResult>>,
    handle: Option<JoinHandle<()>>,
    timeout: Duration,
) -> AppResult<RecordingResult>
```

3. 用一个 parked thread + 永不发送 result 的 receiver 测试：
   - 函数在 timeout 后返回错误；
   - 不调用 join；
   - elapsed 小于一个短阈值。

建议新增测试：

1. `ffmpeg_writer_join_worker_timeout_returns_without_joining`
2. `recording_finalize_join_consumer_timeout_returns_without_joining`
3. `consumer_timeout_continues_cleanup_and_fails_state_machine`

### Minor 2: `git diff --check` 当前失败，新增 review docs 有 trailing whitespace

位置示例：

- `docs/superpowers/reviews/2026-06-02-phase-6-bug-005-follow-up-code-review-findings.md:3`
- `docs/superpowers/reviews/2026-06-02-phase-6-bug-005-follow-up-code-review-findings.md:4`
- `docs/superpowers/reviews/2026-06-02-phase-6-bug-005-post-rectification-code-review.md:3`
- `docs/superpowers/reviews/2026-06-02-phase-6-bug-005-post-rectification-code-review.md:4`
- `docs/superpowers/reviews/2026-06-03-phase-6-bug-005-current-rectification-code-review.md:3`
- `docs/superpowers/reviews/2026-06-03-phase-6-bug-005-current-rectification-code-review.md:508`

命令结果：

```bash
git diff --check 5c505770660abb97d86ece1e20deaeaddef6cfd4..9aa436fa51392bea523d1910621d0c3653872353
```

失败原因：

- 多处 Markdown 行尾存在 trailing whitespace。

为什么重要：

1. 这不影响 Rust/前端运行。
2. 但如果 CI 或合并前检查使用 `git diff --check`，会直接挡住合并。
3. `HANDOFF.md` 写了 `cargo fmt --check` 通过，但没有覆盖 git whitespace gate。

建议修复：

1. 去掉新增 Markdown 中非必要 trailing whitespace。
2. 如果这些 trailing whitespace 是刻意用于 Markdown hard line break，建议改成显式 `<br>` 或放弃依赖行尾两个空格。
3. 若项目不检查 docs whitespace，可在后续门禁说明中明确 `git diff --check` 不作为强制 gate。

## 6. 建议整改顺序

### Phase A: 修正 per-source writer counter 语义

目标：

- `system_chunks_received_by_writer` / `mic_chunks_received_by_writer` 不再在 `push_audio()` 失败时虚增。

建议步骤：

1. 将 `record_source_contribution()` 调用移动到 `push_audio()` 成功分支。
2. 如果要保持 “writer received” 的严格语义，考虑把 source metadata 放入 `EncoderMessage::Audio`，在 worker thread 收到 message 时计数。
3. 补失败路径测试。
4. 跑：
   - `cargo test --manifest-path src-tauri/Cargo.toml source_aware -- --nocapture`
   - `cargo test --manifest-path src-tauri/Cargo.toml consume_frames_writer_push_audio_failure_records_error -- --nocapture`
   - `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg source_aware -- --nocapture`

### Phase B: 让 mic stop diagnostics 进入 stop 返回结构

目标：

- stop 成功时，调用方能从结构化返回中读取 mic stop diagnostics。
- stop 失败时，也尽量不丢 diagnostics。

建议步骤：

1. 扩展 `RecordingResult`，加入 `diagnostics: RecordingDiagnostics` 或 `mic_stop_diagnostics`。
2. `RecordingConsumerOutput.diagnostics` 在 `drive_state_machine()` 中写入 result。
3. 更新 `src/lib/tauri.ts` 类型。
4. 补 serde camelCase 测试和 stop result 测试。
5. 跑：
   - `cargo test --manifest-path src-tauri/Cargo.toml recording_result_serializes -- --nocapture`
   - `cargo test --manifest-path src-tauri/Cargo.toml mic_stop -- --nocapture`
   - `npm test -- --run`

### Phase C: 补 timeout 分支直接测试

目标：

- 用自动化测试锁住 “timeout 后不 join” 的核心规则。

建议步骤：

1. 抽 helper 或 test-only timeout。
2. 构造不发送 result 的 channel 和不会退出的 thread。
3. 断言 helper/stop 路径在短时间内返回 timeout error。
4. 跑：
   - `cargo test --manifest-path src-tauri/Cargo.toml timeout -- --nocapture`
   - `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg timeout -- --nocapture`

### Phase D: 清理 Markdown trailing whitespace

目标：

- `git diff --check` 通过，避免合并前 hygiene gate 失败。

建议步骤：

1. 清理新增 review docs 中的行尾空格。
2. 跑：
   - `git diff --check 5c505770660abb97d86ece1e20deaeaddef6cfd4..HEAD`

## 7. BUG.md 预防规则复核

### 已满足或基本满足

1. **规则 23**：writer worker 和 consumer thread 的 join 必须有 bounded timeout。
   - 当前代码 timeout 分支不再 join。
2. **规则 24**：音频 channel drop logging 必须标识 source。
   - `MediaSender` 已包含 source，调用点已传入 video/system/mic。
3. **规则 25**：CPAL callback 中的 drop 不能用 `let _` 静默忽略。
   - 当前 callback 直接调用 `sink.try_send_drop_newest(chunk)`。
4. **规则 28**：timeout 后绝不能调用无界 join。
   - 当前代码路径符合。
5. **规则 29**：drop ratio 分母必须使用 received + dropped。
   - 当前 system/mic 分支符合。
6. **规则 30**：mic stop diagnostics 必须在重建 capture 前写入 RecordingDiagnostics。
   - 当前内部 `RecordingDiagnostics` 写入时机符合。

### 仍需加强

1. **规则 21**：蓝牙麦克风 stop 必须返回结构化 diagnostics，不能只靠 eprintln。
   - 当前 `stop_with_diagnostics()` 返回结构化 diagnostics，但最终 stop command 没有返回给上层。
   - 需要让 `RecordingResult` 或错误结构携带 diagnostics。
2. **规则 31**：writer diagnostics 必须区分 per-source chunks received。
   - 当前字段已区分 per-source。
   - 但计数时机在 `push_audio()` 前，不能严格证明 writer 已收到。

## 8. Verification

本次 review 过程中已执行以下命令。

### Passed

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

结果：通过。

```bash
cargo test --manifest-path src-tauri/Cargo.toml consume_frames -- --nocapture
```

结果：9 tests passed。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg source_aware -- --nocapture
```

结果：7 tests passed。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_drop_completes_quickly_when_worker_exits -- --nocapture
```

结果：1 test passed。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

结果：297 tests passed。

```bash
npm test -- --run
```

结果：52 tests passed。

### Failed / Warning

```bash
git diff --check 5c505770660abb97d86ece1e20deaeaddef6cfd4..9aa436fa51392bea523d1910621d0c3653872353
```

结果：失败，原因是新增 review docs 存在 trailing whitespace。

### 既有 warnings

`cargo test` 输出中仍存在既有 warnings，例如：

1. `RecordingConsumerOutput` 可见性低于 `MacRecordingService::consume_frames`。
2. `screen_capture_kit.rs` 中部分 FFI 字段命名非 snake_case。
3. `screen_capture_kit.rs` 中存在 unnecessary unsafe / dead_code warning。
4. ffmpeg feature 下 `TimelineAppendResult::trimmed_frames` 当前未读取。

这些 warnings 不阻塞本次整改结论，但建议后续单独治理，避免真正的新 warning 被噪音淹没。

## 9. Manual Gate

自动化测试无法替代真实设备验证。Phase 6 / BUG-005 后续仍需人工覆盖：

1. 只录系统音频。
2. 只录麦克风。
3. 同时录系统音频 + 麦克风。
4. 蓝牙麦克风 stop 后 HFP profile 是否恢复。
5. 低音量输入不触发 false failure。
6. source artifact 和 export artifact 均可听。
7. writer/consumer timeout 失败时 UI 能恢复到 terminal state。

## 10. Final Assessment

本轮代码比上一版明显接近可合并状态：bounded finalize 的核心阻塞已修掉，drop ratio 也修正并有边界测试。

剩余问题集中在 diagnostics 的“证据质量”：

1. per-source writer counters 需要只在成功入队或 worker 实际收到后递增。
2. mic stop diagnostics 需要进入最终 stop 返回结构，而不是只存在内部 diagnostics 和日志。

修完这两项后，再补 timeout 直接测试和 docs whitespace 清理，本轮 Phase 6 follow-up rectification 才适合进入下一次 merge 前 review。
