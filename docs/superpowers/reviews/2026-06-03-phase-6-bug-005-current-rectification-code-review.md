# Phase 6 BUG-005 Current Rectification Code Review

> 日期：2026-06-03  
> 审查范围：当前工作区未提交改动，基线为 `HEAD = 5c505770660abb97d86ece1e20deaeaddef6cfd4`

## 1. 总体结论

**Ready to merge? No.**

当前工作区新增了 source-aware channel drop logging、CPAL callback drop logging 调整、`RecordingFinalizeGuard` RAII 拆分、stop-during-startup 测试和 high-drop 测试。这些方向是有价值的，但本轮整改没有关闭上一次 follow-up review 的核心 blocker：

1. `FfmpegRecordingWriter::join_worker()` 仍在 timeout 后调用无界 `handle.join()`。
2. `MacRecordingService::stop()` / `RecordingFinalizeGuard::join_consumer()` 仍在 consumer timeout 后调用无界 `handle.join()`。
3. drop ratio 仍使用 `dropped / received`，而不是 `dropped / (received + dropped)`。
4. 蓝牙 mic stop diagnostics 仍没有进入 stop 返回结构，且 mic capture 重建后诊断丢失。
5. source-aware artifact 证据仍偏 aggregate，不能严格证明每个 requested source 都进入 artifact。
6. `cargo fmt --check` 当前失败。

因此，这轮代码不能合并。建议先按本文第 7 节顺序整改，再重新跑完整门禁。

## 2. 本轮已完成且方向正确的部分

1. `MediaSender` 增加了 `source: &'static str` 字段，`bounded_media_channel()` 调用点开始传入 `"video"`、`"system"`、`"mic"` 等 source。
2. `MediaSender::try_send_drop_newest()` 开始在第 1 次和每 100 次 drop 时打印 source-aware warning，避免完全静默。
3. `cpal_microphone.rs` 中 `let _ = sink.try_send_drop_newest(chunk)` 被替换为直接调用，drop 由 channel 层统一记录。
4. `MacRecordingService::stop()` 被拆成 `RecordingFinalizeGuard` 多个步骤，代码可读性比原先大块 stop 流程更好。
5. 新增 `consume_frames_respects_stop_flag_set_before_start`，覆盖 consumer 启动前 stop flag 已设置的场景。
6. 新增 `consume_frames_drains_queued_data_on_stop`，覆盖 stop 后 drain 已入队数据的路径。
7. 新增 `consume_frames_fails_on_high_audio_drop_ratio`，覆盖明显高于 10% 的 drop hard fail。

这些改动可以保留，但不能替代上轮 Critical/Important finding 的修复。

## 3. Critical Findings

### Critical 1: FFmpeg writer timeout 后仍会无界 join

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:83`
- `src-tauri/src/media/ffmpeg_writer.rs:94`
- `src-tauri/src/media/ffmpeg_writer.rs:96`
- `src-tauri/src/media/ffmpeg_writer.rs:102`
- `src-tauri/src/media/ffmpeg_writer.rs:103`

当前逻辑：

```rust
match rx.recv_timeout(WORKER_RESULT_TIMEOUT) {
    Ok(result) => result,
    Err(mpsc::RecvTimeoutError::Timeout) => {
        eprintln!("警告: FFmpeg worker 超时未返回结果 ...");
        if let Some(handle) = self.worker.take() {
            match handle.join() {
                ...
            }
        }
    }
    ...
}
```

问题：

1. `recv_timeout(10s)` 本来用于给 writer finalize 建立上限。
2. timeout 分支随后调用 `handle.join()`，把 bounded timeout 又变成了 unbounded wait。
3. 如果 worker 卡在 FFmpeg encoder flush、muxer IO、`write_trailer()`、文件系统写入或底层释放路径，`finish()` 仍可能永久阻塞。
4. 这直接违反 `BUG.md` 规则 15 和 23：
   - 录制停止/finalize 的 bounded 语义必须覆盖 flush send 和 worker join。
   - writer worker 和 consumer thread 的 join 必须有 bounded timeout，超时返回结构化错误而非无限阻塞。

影响：

1. 用户点击停止录制后，consumer thread 会卡在 `writer.finish()`。
2. consumer 卡住后，上层 stop 也可能卡在 consumer join。
3. 蓝牙 mic stop、状态机 terminal transition、错误返回、UI 恢复都可能受影响。

建议修复：

1. timeout 分支不要调用 `handle.join()`。
2. timeout 后立即返回 `AppError::RecordingWriteFailed`，明确说明 worker 超时且可能仍在后台执行。
3. 如果需要 panic 信息，在 worker closure 内部用 `std::panic::catch_unwind()` 包住 `encoder_worker()`，再通过 result channel 发送 panic error。
4. `RecvTimeoutError::Disconnected` 可以尝试 join，因为通道断开通常说明 worker 已退出或 panic；真正危险的是 timeout 分支。

建议新增测试：

1. `ffmpeg_writer_finish_returns_error_when_worker_result_times_out`
2. `ffmpeg_writer_finish_timeout_does_not_block_on_join`
3. `ffmpeg_worker_panic_is_reported_through_result_channel`

### Critical 2: consumer timeout 后仍会无界 join

位置：

- `src-tauri/src/platform/macos_service.rs:381`
- `src-tauri/src/platform/macos_service.rs:385`
- `src-tauri/src/platform/macos_service.rs:391`
- `src-tauri/src/platform/macos_service.rs:392`
- `src-tauri/src/platform/macos_service.rs:411`
- `src-tauri/src/platform/macos_service.rs:412`

当前逻辑：

```rust
match rx.recv_timeout(CONSUMER_RESULT_TIMEOUT) {
    Ok(output) => (output, false),
    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
        self.errors.push(...);
        if let Some(handle) = self.service.consumer_handle.take() {
            if let Err(panic) = handle.join() {
                ...
            }
        }
        (empty_output, true)
    }
    ...
}
```

问题：

1. `recv_timeout(15s)` 本来用于给 consumer finalize 建立上限。
2. timeout 分支随后调用 `handle.join()`，仍可能无限等待。
3. fallback 分支在没有 `consumer_result_rx` 但有 `consumer_handle` 时也直接 `join()`，同样没有上限。
4. 这意味着 `MacRecordingService::stop()` 仍不具备真正 bounded 语义。

影响：

1. consumer 如果卡在 writer finalize、artifact validation、trim metadata 生成或其他阻塞路径，stop 仍可能永久卡住。
2. 即使 mic 已经先 stop，整体状态机仍可能无法进入 completed/failed terminal state。
3. UI 可能长时间停留在“停止中/录制中”状态，用户无法恢复。

建议修复：

1. consumer timeout 后不要 join，直接记录结构化错误，使用 `empty_output` 继续执行后续 cleanup。
2. 丢弃 `JoinHandle`，让线程 detached；错误信息中明确 consumer 可能仍在后台执行。
3. 用 consumer thread 内部 `catch_unwind()` 或 result channel 返回 panic 信息。
4. 删除 fallback direct join 分支，或者改成记录状态不一致并返回错误，不做无界 join。

建议新增测试：

1. `mac_recording_stop_returns_error_when_consumer_result_times_out`
2. `mac_recording_stop_timeout_does_not_block_on_consumer_join`
3. `mac_recording_stop_continues_cleanup_after_consumer_timeout`

## 4. Important Findings

### Important 1: drop ratio 分母仍错误，阈值附近会 false positive

位置：

- `src-tauri/src/platform/macos_service.rs:938`
- `src-tauri/src/platform/macos_service.rs:939`
- `src-tauri/src/platform/macos_service.rs:940`
- `src-tauri/src/platform/macos_service.rs:955`
- `src-tauri/src/platform/macos_service.rs:956`
- `src-tauri/src/platform/macos_service.rs:957`

当前逻辑：

```rust
let total = diagnostics.system_chunks_received.max(1) as f64;
let ratio = diagnostics.system_chunks_dropped as f64 / total;
```

问题：

1. `received` 是成功进入 receiver 并被 consumer 收到的数量。
2. `dropped` 是 sender 尝试发送但因为 full/disconnected 被丢弃的数量。
3. 真实 attempted total 应为 `received + dropped`。
4. 当前代码用 `dropped / received`，会系统性高估 drop ratio。

示例：

| received | dropped |        当前代码 |       正确 ratio |
| -------: | ------: | --------------: | ---------------: |
|       95 |      10 | 10 / 95 = 10.5% |  10 / 105 = 9.5% |
|       90 |      10 | 10 / 90 = 11.1% | 10 / 100 = 10.0% |
|        9 |       1 |   1 / 9 = 11.1% |   1 / 10 = 10.0% |

影响：

1. 真实低于 10% 的 drop 可能被 hard fail。
2. 这会重新制造 BUG-005_2 类型 false positive：诊断 warning 被错误升级为录制失败。
3. 当前新增测试只覆盖明显 high drop：received=10、dropped=2。它不能证明边界附近正确。

建议修复：

```rust
let attempted = diagnostics
    .system_chunks_received
    .saturating_add(diagnostics.system_chunks_dropped)
    .max(1) as f64;
let ratio = diagnostics.system_chunks_dropped as f64 / attempted;
```

mic 分支同理。

边界策略也需要明确：

1. 如果规则是“超过 10% hard fail”，使用 `ratio > 0.10`。
2. 如果规则是“达到 10% hard fail”，使用 `ratio >= 0.10`。
3. `BUG.md` 当前写法偏向“少量 drop <10% 只是 warning”，建议保留 `>`，并补测试锁住。

建议新增测试：

1. `consume_frames_passes_when_attempted_drop_ratio_below_10_percent`
2. `consume_frames_fails_when_attempted_drop_ratio_above_10_percent`
3. `consume_frames_drop_ratio_uses_received_plus_dropped_denominator`
4. `consume_frames_drop_ratio_boundary_is_documented`

### Important 2: 蓝牙 mic stop diagnostics 仍没有结构化返回或持久保留

位置：

- `src-tauri/src/platform/macos/cpal_microphone.rs:19`
- `src-tauri/src/platform/macos/cpal_microphone.rs:204`
- `src-tauri/src/platform/macos/cpal_microphone.rs:253`
- `src-tauri/src/platform/macos_service.rs:336`
- `src-tauri/src/platform/macos_service.rs:337`
- `src-tauri/src/platform/macos_service.rs:485`
- `src-tauri/src/media/recording_writer.rs:240`

当前实现：

1. `CpalMicrophoneStopDiagnostics` 结构体存在。
2. `CpalMicrophoneCapture::stop()` 仍返回 `AppResult<()>`。
3. diagnostics 写入 `self.last_stop_diagnostics`。
4. `MacRecordingService::stop()` 通过 `last_stop_diagnostics()` 读取并 `eprintln!`。
5. 随后 `self.service.mic_capture = CpalMicrophoneCapture::new()`，旧 capture 和其中的 `last_stop_diagnostics` 被丢弃。
6. `RecordingResult` 没有 mic stop diagnostics 字段。

问题：

1. 这不满足 `BUG.md` 规则 21：“蓝牙麦克风 stop 必须返回结构化 `CpalMicrophoneStopDiagnostics`，不能只靠 eprintln。”
2. diagnostics 没有进入 `RecordingDiagnostics`、`RecordingResult` 或 app-level error。
3. 上层 UI、自动化检查和后续 bug 定位都拿不到 pause/drop/wait/callbacks_after_stop。

建议修复：

1. 最小变更：给 `RecordingDiagnostics` 增加 `mic_stop_diagnostics: Option<CpalMicrophoneStopDiagnostics>`。
2. 或给 `RecordingResult` 增加 `mic_stop_diagnostics` 字段，让 stop command 返回给前端。
3. 在 reset mic capture 前 clone diagnostics 并写入 result/diagnostics。
4. 文档中明确 diagnostics 是 machine-readable，而不是只用于日志。

建议新增测试：

1. `mac_recording_stop_preserves_mic_stop_diagnostics_after_capture_reset`
2. `recording_result_serializes_mic_stop_diagnostics`
3. `cpal_stop_records_pause_drop_wait_callbacks`

### Important 3: source-aware artifact 证据仍偏 aggregate

位置：

- `src-tauri/src/media/recording_writer.rs:17`
- `src-tauri/src/media/recording_writer.rs:204`
- `src-tauri/src/media/recording_writer.rs:207`
- `src-tauri/src/media/recording_writer.rs:210`

当前状态：

1. `RecordingDiagnostics` 有 before-writer per-source counters：
   - `system_windows_before_writer`
   - `mic_windows_before_writer`
   - `system_frames_before_writer`
   - `mic_frames_before_writer`
2. 但 `WriterDiagnostics` 仍是 aggregate：
   - `audio_chunks_received`
   - `audio_chunks_appended`
   - `audio_chunks_discarded_full_overlap`
   - `audio_real_frames_appended`
3. `validate_source_aware_audio_contract()` 的 writer discard 检查也是全局 ratio。

问题：

1. before-writer 计数能证明某个 source 到达 writer 之前。
2. aggregate writer diagnostics 只能证明“有某些 audio chunk 被 writer append/encode”。
3. 当 system+mic 同时 requested 时，它不能严格证明每个 requested source 都成功进入 artifact。
4. 如果将来某一路被 writer timeline overlap 逻辑系统性 discard，aggregate ratio 可能被另一路掩盖。

建议修复：

1. 在 synchronizer 输出或 writer input 层保留 per-source contribution metadata。
2. `WriterDiagnostics` 增加 per-source counters，例如：
   - `system_frames_appended`
   - `mic_frames_appended`
   - `system_chunks_discarded_full_overlap`
   - `mic_chunks_discarded_full_overlap`
   - `system_rms_max_before_encode`
   - `mic_rms_max_before_encode`
3. `validate_source_aware_audio_contract()` 对 requested system/mic 分别检查 writer-side append/discard。
4. 对双源场景新增“某一路被 writer 全部 discard，但另一路仍有输出”的回归测试。

### Important 4: `cargo fmt --check` 当前失败

命令：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

结果：失败。

涉及位置：

- `src-tauri/src/core/media_channel.rs:58`
- `src-tauri/src/platform/macos_service.rs:326`
- `src-tauri/src/platform/macos_service.rs:383`
- `src-tauri/src/platform/macos_service.rs:501`

问题：

1. 当前工作区未通过 Rust 格式化门禁。
2. 即使功能测试通过，也不能按项目合并口径进入下一阶段。

建议修复：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
```

然后重新运行：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
```

## 5. Minor Findings

### Minor 1: channel drop 日志对 video source 文案不准确

位置：

- `src-tauri/src/core/media_channel.rs:61`
- `src-tauri/src/platform/macos_service.rs:188`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:808`

当前日志固定为：

```rust
eprintln!(
    "警告: {} 音频通道丢弃 (累计 {} 次)",
    self.source, count
);
```

问题：

1. `source` 可能是 `"video"`。
2. video drop 时会打印类似“video 音频通道丢弃”，排查时会误导。
3. fallback source `"audio_fallback"` 也不是清晰的 user-facing source。

建议修复：

1. 将文案改为“媒体通道丢弃”。
2. 或把 `source` 和 `kind` 分开，例如 `source="video"`, `kind="frame"`。

### Minor 2: `BUG.md` 与 `HANDOFF.md` manual gate 状态冲突

位置：

- `BUG.md:15`
- `BUG.md:74`
- `HANDOFF.md:3`

当前状态：

1. `BUG.md` 写 BUG-005 / BUG-005_2 “人工验证通过”。
2. `HANDOFF.md` 写“真实设备 manual gate 仍待完成”。

问题：

1. release gate 状态不一致。
2. 后续交接时无法判断真实设备验证是否已经完成。

建议修复：

1. 如果真实设备 gate 尚未完成，`BUG.md` 应改为“自动化修复完成，真实设备 manual gate 待完成”。
2. 如果确实已完成，`HANDOFF.md` 需要列出具体验证日期、设备、场景和结果。

### Minor 3: `RecordingFinalizeGuard` 注释对 panic cleanup 的承诺偏强

位置：

- `src-tauri/src/platform/macos_service.rs:285`
- `src-tauri/src/platform/macos_service.rs:536`

当前注释写：

1. RAII guard ensures all recording resources are released on drop。
2. Drop 确保 panic 时资源仍被释放。

实际 Drop 只做：

```rust
self.service.stop_flag = None;
if let Ok(mut guard) = self.service.mic_level.lock() {
    *guard = 0.0;
}
```

问题：

1. Drop 没有 stop screen capture。
2. Drop 没有 stop mic capture。
3. Drop 没有 join/detach consumer。
4. Drop 没有 reset mic capture instance。
5. 因此它只能称为“basic cleanup safety net”，不能称为“all recording resources are released”。

建议修复：

1. 调整注释，避免过度承诺。
2. 如果确实要 RAII 释放全部资源，需要把 stop capture、mic stop、consumer detach、mic reset 都设计成 Drop-safe 且幂等。

## 6. 验证记录

### 已运行

```bash
cargo test --manifest-path src-tauri/Cargo.toml consume_frames_ -- --nocapture
```

结果：通过，7 tests passed。

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

结果：通过，239 tests passed。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

结果：

1. 第一次完整运行失败一次，失败用例为 `media::cursor_overlay::tests::cursor_overlay_skips_mapped_infinite_coordinates`。
2. 单独重跑该用例通过。
3. 第二次完整运行通过，292 tests passed，integration/doc tests 也通过。

判断：这次 transient failure 不作为本轮主 blocker，但建议后续关注 FFmpeg feature suite 是否存在并发或全局状态 flakiness。

```bash
npm test -- --run
```

结果：通过，52 tests passed。

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

结果：失败。详见 Important 4。

### 未运行

```bash
cargo clippy
```

原因：`cargo fmt --check` 已失败，建议先完成格式化和 Critical 修复后再跑 clippy。

## 7. 推荐整改顺序

1. **先修 Critical 1：FFmpeg writer timeout 后不要 join。**
   - timeout 直接返回结构化错误。
   - worker closure 内 `catch_unwind()`，panic 通过 result channel 返回。
   - 补 timeout 不阻塞测试。

2. **修 Critical 2：consumer timeout 后不要 join。**
   - timeout 后继续 cleanup。
   - 删除或改造 fallback direct join。
   - 补 stop timeout 不阻塞测试。

3. **修 Important 1：drop ratio 分母。**
   - system/mic 都改为 `dropped / (received + dropped)`。
   - 补 9.5%、10%、10.5% 附近边界测试。

4. **修 Important 2：mic stop diagnostics 结构化返回。**
   - 放入 `RecordingDiagnostics` 或 `RecordingResult`。
   - 在重建 mic capture 前持久化。
   - 补序列化和 reset 后保留测试。

5. **修 Important 3：writer-side per-source diagnostics。**
   - writer diagnostics 从 aggregate 扩展为 per-source。
   - source-aware contract 分别验证 system/mic writer-side append/discard。

6. **修 Minor 文档和日志。**
   - `BUG.md` / `HANDOFF.md` manual gate 状态统一。
   - `HANDOFF.md` 输入文件改为 follow-up findings。
   - media channel 日志文案改为“媒体通道”或 source/kind 分层。
   - 调整 RAII guard 注释。

7. **最后跑完整门禁。**
   - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`
   - `npm test -- --run`
   - `cargo clippy`

## 8. BUG.md 预防规则复核

本轮仍未满足或未完全满足的预防规则：

1. **规则 15**：录制停止/finalize 的 bounded 语义必须覆盖 flush send 和 worker join。  
   当前 `ffmpeg_writer.rs` timeout 后仍 join，不满足。

2. **规则 21**：蓝牙麦克风 stop 必须返回结构化 diagnostics，不能只靠 eprintln。  
   当前 diagnostics 只保存在 mic capture 内部并打印，reset 后丢失，不满足。

3. **规则 22**：少量音频 chunk drop 只能 warning，hard fail 需基于 drop ratio 阈值。  
   当前已按 ratio hard fail，但 denominator 错误，阈值附近会误判，部分满足。

4. **规则 23**：writer worker 和 consumer thread 的 join 必须有 bounded timeout，超时返回结构化错误而非无限阻塞。  
   当前 writer 和 consumer timeout 后都仍可能无界 join，不满足。

5. **规则 24**：音频 channel drop logging 必须标识 source。  
   当前已做到 source 标识，但日志文案对 video source 不准确，基本满足但需 polish。

6. **规则 25**：CPAL callback 中的 drop 不能用 `let _` 静默忽略。  
   当前已移除 `let _`，满足。

7. **规则 26**：录制 stop 路径必须使用 RAII guard 或等效机制，确保 panic 时资源仍被释放。  
   当前已有 guard，但 Drop 只做 basic cleanup，注释与实际能力不一致，部分满足。

8. **规则 27**：stop-during-startup 场景必须有测试覆盖。  
   当前新增 `consume_frames_respects_stop_flag_set_before_start`，满足基础覆盖。
