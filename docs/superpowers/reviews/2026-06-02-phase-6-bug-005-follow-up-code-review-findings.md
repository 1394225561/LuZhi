# Phase 6 BUG-005 Follow-up Code Review Findings

> 日期：2026-06-02  
> 审查范围：`caf6cb400d6d2a29128d5242244b30b7cdb2cc2b..5c505770660abb97d86ece1e20deaeaddef6cfd4`  
> 关联提交：
>
> 1. `8fb60b6` - CPAL lazy offset sentinel (`AtomicBool` 替代 `0` 做 sentinel)
> 2. `a701a26` - drop warning/error 分层（10% drop ratio hard fail）
> 3. `9c1b353` - source-aware discard 对称化测试
> 4. `839a328` - bounded finalize（writer 10s, consumer 15s timeout）
> 5. `d8da662` - 蓝牙 mic stop diagnostics
> 6. `5c50577` - `BUG.md` + `HANDOFF.md` 更新

## 1. 输入依据

- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/reviews/2026-06-02-phase-6-bug-005-post-rectification-code-review.md`
- `docs/superpowers/plans/2026-06-02-phase-6-bug-005-post-rectification-code-review-rectification.md`
- `reference/tasks/0-code-review-rectification-task.md`
- `BUG.md`
- `HANDOFF.md`

重点复核 `BUG.md` 中 BUG-005 / BUG-005_2 的预防规则，尤其：

1. 录制停止/finalize 的 bounded 语义必须覆盖 flush send 和 worker join；只在 `join()` 返回后记录耗时不等于 bounded join。
2. writer worker 和 consumer thread 的 join 必须有 bounded timeout（worker 10s、consumer 15s），超时返回结构化错误而非无限阻塞。
3. 少量音频 chunk drop（<10% drop ratio）只能进入 diagnostics warning，不能直接导致 stop 失败。
4. 蓝牙麦克风 stop 必须返回结构化 `CpalMicrophoneStopDiagnostics`，不能只靠 `eprintln!`。
5. writer diagnostics 不能用 aggregate queued / encoded 指标冒充每个 requested source 的 artifact 级有声证据。

## 2. 总体结论

**Ready to merge? No.**

本轮整改的方向是正确的，但仍存在阻断合并的问题：

1. **Critical blocker**：所谓 bounded finalize 仍然不是严格 bounded。`recv_timeout()` 超时后代码又调用无界 `handle.join()`，stop/finalize 仍可能永久阻塞。
2. **Important**：drop ratio 使用 `dropped / received`，不是 `dropped / (received + dropped)`，阈值附近仍会制造 BUG-005_2 类型 false positive。
3. **Important**：蓝牙 mic stop diagnostics 只是内部保存并打印，随后 capture 被重建，结构化诊断没有真正返回或保留。
4. **Important**：source-aware writer diagnostics 仍是 aggregate 证据，不能证明每个 requested source 都进入 artifact。

可以肯定的部分：

1. CPAL lazy offset 已经用 `AtomicBool` 避免 `0` sentinel 歧义，并补了 zero-offset 回归测试。
2. small audio drop 不再直接进入 `errors`，新增测试能覆盖少量 drop 不 hard fail。
3. system/mic full-overlap discard 的对称测试已补上。
4. worker/consumer result channel 的方向正确，只是 timeout 分支又被无界 join 抵消了。

## 3. Critical Findings

### Critical 1: `FfmpegRecordingWriter::join_worker()` timeout 后仍无界 `join()`

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:94`
- `src-tauri/src/media/ffmpeg_writer.rs:96`
- `src-tauri/src/media/ffmpeg_writer.rs:102`
- `src-tauri/src/media/ffmpeg_writer.rs:103`

当前逻辑：

```rust
match rx.recv_timeout(WORKER_RESULT_TIMEOUT) {
    Ok(result) => result,
    Err(mpsc::RecvTimeoutError::Timeout) => {
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

1. `recv_timeout(10s)` 本来是为了让 writer worker 在超时后返回结构化错误。
2. 但 timeout arm 立刻调用 `handle.join()`。
3. 如果 worker 卡在 FFmpeg flush、muxer IO、`write_trailer()`、encoder flush 或文件系统阻塞路径上，`handle.join()` 会一直等待 worker 退出。
4. 因此 `finish()` 仍可能无限阻塞，并没有满足 "worker 10s timeout" 的整改目标。

影响：

1. 用户点击停止录制后，consumer thread 会卡在 `writer.finish()`。
2. 上层 `MacRecordingService::stop()` 即使设置了 consumer timeout，也会进入另一个无界 join 问题（见 Critical 2）。
3. 这违反 `BUG.md` 预防规则 15 和 23。
4. 也违反架构文档中“捕获/停止链路不得被编码或 IO 长时间阻塞”的要求。

建议修复：

1. timeout arm 不要调用 `handle.join()`。
2. timeout 后直接返回 `AppError::RecordingWriteFailed { reason: "FFmpeg worker 超时未返回结果..." }`。
3. 丢弃 `JoinHandle` 让线程 detached；同时在错误中明确记录 worker 可能仍在后台运行。
4. 如果需要 panic 信息，在 worker closure 内用 `std::panic::catch_unwind()` 包住 `encoder_worker()`，panic 信息通过 result channel 发送。
5. `RecvTimeoutError::Disconnected` 分支可以 join，因为 sender 已断开通常表示 worker 已退出或 panic；真正危险的是 timeout 分支。

建议新增测试：

1. `ffmpeg_writer_finish_returns_error_when_worker_result_times_out`
2. `ffmpeg_writer_finish_timeout_does_not_call_blocking_join`
3. `ffmpeg_worker_panic_is_reported_through_result_channel`

### Critical 2: `MacRecordingService::stop()` consumer timeout 后仍无界 `join()`

位置：

- `src-tauri/src/platform/macos_service.rs:350`
- `src-tauri/src/platform/macos_service.rs:353`
- `src-tauri/src/platform/macos_service.rs:355`
- `src-tauri/src/platform/macos_service.rs:365`
- `src-tauri/src/platform/macos_service.rs:366`
- `src-tauri/src/platform/macos_service.rs:385`
- `src-tauri/src/platform/macos_service.rs:388`

当前逻辑：

```rust
match rx.recv_timeout(CONSUMER_RESULT_TIMEOUT) {
    Ok(output) => (output, false),
    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
        errors.push(...);
        if let Some(handle) = self.consumer_handle.take() {
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

1. `recv_timeout(15s)` 超时后，代码又调用 `handle.join()`。
2. 如果 consumer thread 卡在 writer finalize、artifact validation、trim metadata 生成或其他阻塞路径，`stop()` 仍会无限等待。
3. fallback 分支在没有 result channel 但有 `consumer_handle` 时也直接 `join()`，虽然正常路径不应进入，但一旦状态不一致仍存在无界等待。

影响：

1. `MacRecordingService::stop()` 仍可能永久卡住。
2. 蓝牙 mic 已先 stop 并释放 HFP profile，但 UI/状态机可能无法完成停止流程。
3. 光标/trim sidecar 写入、状态机 terminal transition、错误返回都可能被阻塞。
4. 这同样违反 `BUG.md` 预防规则 23。

建议修复：

1. consumer timeout 后不要 join，立即使用 `empty_output` 继续 cleanup，并把 timeout error 放入 `errors`。
2. 丢弃 `JoinHandle`，明确记录 consumer 可能仍在后台运行。
3. 对 panic 信息同样使用 consumer thread 内部 `catch_unwind()` 或 panic result channel。
4. fallback direct join 分支应删除，或至少改成直接记录状态异常并返回错误，不做无界 join。

建议新增测试：

1. `mac_recording_stop_returns_error_when_consumer_result_times_out`
2. `mac_recording_stop_timeout_does_not_block_on_consumer_join`
3. `mac_recording_stop_continues_cleanup_after_consumer_timeout`

## 4. Important Findings

### Important 1: drop ratio 分母错误，阈值附近仍会 false positive

位置：

- `src-tauri/src/platform/macos_service.rs:865`
- `src-tauri/src/platform/macos_service.rs:866`
- `src-tauri/src/platform/macos_service.rs:867`
- `src-tauri/src/platform/macos_service.rs:882`
- `src-tauri/src/platform/macos_service.rs:883`
- `src-tauri/src/platform/macos_service.rs:884`

当前逻辑：

```rust
let total = diagnostics.system_chunks_received.max(1) as f64;
let ratio = diagnostics.system_chunks_dropped as f64 / total;
```

问题：

1. `system_chunks_received` 是成功进入 channel 并被 consumer 收到的 chunk 数。
2. `system_chunks_dropped` 是发送端因为 channel full / disconnected 未能送入 receiver 的 chunk 数。
3. 真实 attempted total 应为 `received + dropped`。
4. 当前分母只用 `received`，会系统性高估 drop ratio。

示例：

| received | dropped | 当前代码 | 真实 drop ratio |
|---:|---:|---:|---:|
| 95 | 10 | 10 / 95 = 10.5% | 10 / 105 = 9.5% |
| 90 | 10 | 10 / 90 = 11.1% | 10 / 100 = 10.0% |
| 9 | 1 | 1 / 9 = 11.1% | 1 / 10 = 10.0% |

影响：

1. 低于 10% 的真实 drop ratio 可能被 hard fail。
2. 这会重新制造 BUG-005_2 类型问题：diagnostics warning 被升级为 stop failure。
3. 当前新增测试只覆盖 `1/11 ~= 9.1%` 的 pass case，没有覆盖边界和 high-drop fail。
4. 日志里的“总计”目前也打印的是 received，而不是 attempted total，排查时会误导。

建议修复：

```rust
let attempted = diagnostics
    .system_chunks_received
    .saturating_add(diagnostics.system_chunks_dropped)
    .max(1) as f64;
let ratio = diagnostics.system_chunks_dropped as f64 / attempted;
```

mic 分支同理。

边界策略需要明确：

1. 如果规则是“超过 10% hard fail”，使用 `ratio > 0.10`。
2. 如果规则是“达到 10% hard fail”，使用 `ratio >= 0.10`。
3. `BUG.md` 当前写法是“<10% warning；hard fail 需基于 drop ratio 阈值”，建议在整改计划中明确 `>=` 还是 `>`。

建议新增测试：

1. `consume_frames_passes_when_drop_ratio_below_10_percent`
2. `consume_frames_fails_when_drop_ratio_above_10_percent`
3. `consume_frames_drop_ratio_uses_received_plus_dropped_denominator`
4. `consume_frames_drop_ratio_boundary_is_documented`

### Important 2: 蓝牙 mic stop diagnostics 没有真正返回或持久保留

位置：

- `src-tauri/src/platform/macos/cpal_microphone.rs:19`
- `src-tauri/src/platform/macos/cpal_microphone.rs:54`
- `src-tauri/src/platform/macos/cpal_microphone.rs:68`
- `src-tauri/src/platform/macos/cpal_microphone.rs:204`
- `src-tauri/src/platform/macos/cpal_microphone.rs:253`
- `src-tauri/src/platform/macos_service.rs:307`
- `src-tauri/src/platform/macos_service.rs:308`
- `src-tauri/src/platform/macos_service.rs:456`

当前实现：

1. `CpalMicrophoneStopDiagnostics` 结构体已新增。
2. `CpalMicrophoneCapture::stop()` 仍返回 `AppResult<()>`。
3. diagnostics 写入 `self.last_stop_diagnostics`。
4. `MacRecordingService::stop()` 通过 getter 读取并 `eprintln!("{:?}")`。
5. 随后 `self.mic_capture = CpalMicrophoneCapture::new()`，旧 diagnostics 随对象被丢弃。

问题：

1. 这不满足 `BUG.md` 规则 21 的“必须返回结构化 diagnostics，不能只靠 eprintln”。
2. 结构化数据没有进入 `RecordingDiagnostics`、`RecordingResult` 或 app-level error。
3. 用户/上层 UI/后续自动化检查无法拿到 pause/drop/wait/callbacks_after_stop。
4. 如果再次出现蓝牙 profile 未释放，仍主要依赖日志文本和人工听感，不利于稳定复现。

建议修复：

由于 `AudioCapture` trait 当前是：

```rust
fn stop(&mut self) -> AppResult<()>;
```

建议采用较小改动：

1. 为 `CpalMicrophoneCapture` 增加 inherent method：

```rust
pub fn stop_with_diagnostics(&mut self) -> AppResult<CpalMicrophoneStopDiagnostics>
```

2. `impl AudioCapture for CpalMicrophoneCapture` 的 `stop()` 调用 `stop_with_diagnostics()`，丢弃返回 diagnostics，以保持 trait 兼容。
3. `MacRecordingService::stop()` 在 mic 路径调用 `stop_with_diagnostics()`。
4. 将返回的 diagnostics 写入 `RecordingDiagnostics` 或 `RecordingResult`。
5. 如果暂不想扩展 public result，至少把 diagnostics 保存在 `MacRecordingService` 的 `last_mic_stop_diagnostics` 字段中，供后续命令读取。

建议新增测试：

1. `cpal_microphone_stop_returns_structured_diagnostics`
2. `mac_recording_stop_preserves_mic_stop_diagnostics_after_capture_reset`
3. `mic_stop_diagnostics_serializes_pause_drop_wait_callbacks`

### Important 3: source-aware discard 仍是 aggregate，不能证明每个请求源进入 artifact

位置：

- `src-tauri/src/media/recording_writer.rs:17`
- `src-tauri/src/media/recording_writer.rs:51`
- `src-tauri/src/media/recording_writer.rs:90`
- `src-tauri/src/media/recording_writer.rs:204`
- `src-tauri/src/media/recording_writer.rs:210`
- `src-tauri/src/platform/macos_service.rs:632`
- `src-tauri/src/platform/macos_service.rs:673`
- `src-tauri/src/media/ffmpeg_writer.rs:690`
- `src-tauri/src/media/ffmpeg_writer.rs:696`
- `src-tauri/src/core/frame.rs:59`

当前实现链路：

1. `SynchronizedAudioChunk` 有 `has_system` / `has_mic` / `system_frames` / `mic_frames`。
2. `macos_service.rs` 在调用 `writer.push_audio()` 前记录 `system_windows_before_writer` / `mic_windows_before_writer` / frames / RMS。
3. `MixedAudioChunk` 只包含 timestamp/sample_rate/channels/samples，没有 source metadata。
4. `EncoderMessage::Audio` 只包含 samples 和 timestamp。
5. `WriterDiagnostics` 只有 aggregate audio chunk counters。
6. `validate_source_aware_audio_contract()` 对 writer full-overlap discard 也只看 aggregate discard ratio。

问题：

1. before-writer 计数只能证明 synchronizer 产出了某源内容，并且准备推给 writer。
2. 它不能证明该源被 writer accepted、appended、encoded 或最终出现在 artifact。
3. 双源场景中，如果 mic 只出现在少量 windows，而这些 windows 被 writer full-overlap discard；system 有大量 windows 正常 append，aggregate discard ratio 可能低于 50%，artifact aggregate RMS/peak 也可能由 system 撑起。
4. 此时 mic requested、mic before-writer 非零、aggregate artifact 非静音、aggregate writer discard ratio 不高，contract 仍可能通过，但 mic 实际缺失。

影响：

1. diagnostics 仍可能把 before-writer 证据误当作 per-source artifact 证据。
2. 无法自动区分“系统音频可听但 mic 缺失”和“mic 可听但系统音频缺失”。
3. 这与 BUG-005/BUG-005_2 预防规则中的 source-aware contract 目标还有差距。

建议修复：

1. 引入 source-aware mixed chunk metadata，例如：

```rust
pub struct SourceAwareMixedAudioChunk {
    pub mixed: MixedAudioChunk,
    pub has_system: bool,
    pub has_mic: bool,
    pub system_frames: u64,
    pub mic_frames: u64,
    pub system_rms: f32,
    pub mic_rms: f32,
}
```

2. 或扩展 `MixedAudioChunk`，但要注意所有调用方和 serialization 影响。
3. `RecordingWriter::push_audio()` 接收 source-aware metadata，或新增旁路 diagnostics sink。
4. `FfmpegRecordingWriter` 在 append/discard/trim 时按 source 记录：
   - `system_frames_received_by_writer`
   - `mic_frames_received_by_writer`
   - `system_frames_appended_by_writer`
   - `mic_frames_appended_by_writer`
   - `system_frames_discarded_full_overlap`
   - `mic_frames_discarded_full_overlap`
5. `validate_source_aware_audio_contract()` 对每个 requested source 检查 source-specific append/discard ratio。

建议新增测试：

1. `source_aware_contract_rejects_when_requested_mic_windows_are_partially_discarded_but_aggregate_ratio_is_low`
2. `source_aware_contract_rejects_when_requested_system_windows_are_partially_discarded_but_aggregate_ratio_is_low`
3. `writer_diagnostics_records_per_source_appended_frames`
4. `writer_diagnostics_records_per_source_discarded_frames`

## 5. Minor Findings

### Minor 1: `AudioSampleClock` initialized flag 和 offset publish 顺序存在低概率并发可见性窗口

位置：

- `src-tauri/src/core/clock.rs:132`
- `src-tauri/src/core/clock.rs:137`
- `src-tauri/src/core/clock.rs:153`

当前逻辑：

```rust
if self
    .offset_initialized
    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
    .is_ok()
{
    self.session_offset_nanos.store(offset, Ordering::Release);
}
```

问题：

1. 代码先把 `offset_initialized` 置为 true，再 store offset。
2. 如果未来 CPAL callback 出现并发或重入，另一个 callback 可能看到 initialized=true，跳过初始化，然后读取旧的 `session_offset_nanos=0`。
3. 当前 macOS CoreAudio callback 通常不会并发调用同一个 stream closure，因此这是低概率风险；但代码使用 atomics/CAS，语义上已经在表达可并发调用。

影响：

1. 首个 offset 非 0 时，极端并发下可能有一个 chunk 使用 0 offset。
2. 可能造成 mic timestamp 初始跳变。

建议修复：

1. 使用 `OnceLock<u64>` / `OnceCell<u64>` 表示一次性初始化值。
2. 或改用 `AtomicU64::MAX` sentinel，在同一个 atomic 上 compare_exchange，避免 flag/value 双原子 publish gap。
3. 如果保留双 atomic，必须保证任何观察到 initialized=true 的线程也能观察到已发布 offset。

### Minor 2: `BUG.md` 与 `HANDOFF.md` 对 manual gate 状态描述冲突

位置：

- `BUG.md:15`
- `BUG.md:70`
- `HANDOFF.md:3`

当前状态：

1. `BUG.md` 写 `BUG-005` / `BUG-005_2` 当前状态为“已修复 + 人工验证通过”。
2. `HANDOFF.md` 顶部写“真实设备 manual gate 仍待完成”。

问题：

1. 同一轮 Phase 6 音频问题的验证状态不一致。
2. 后续合并、发布或继续整改时，会误判真实设备 gate 是否完成。
3. `BUG.md` 预防规则 20 明确要求 manual gate 覆盖只系统音频、只麦克风、系统+麦克风、蓝牙麦克风、低音量输入、停止后蓝牙音质恢复、source 与 export 都可听。

建议修复：

1. 如果 manual gate 确实未完成，将 `BUG.md` 状态改为“代码整改完成；真实设备 manual gate 待完成”。
2. 如果 manual gate 已完成，在 `HANDOFF.md` 中补充具体日期、设备、场景、结果，并移除“仍待完成”。
3. 不建议保留“人工验证通过”和“manual gate 仍待完成”同时存在。

## 6. 推荐整改顺序

### Phase A1: 修正真正 bounded finalize

优先级最高。

目标：

1. worker timeout 后绝不再调用无界 `join()`。
2. consumer timeout 后绝不再调用无界 `join()`。
3. timeout 后返回结构化错误，并继续执行可执行的 cleanup。

建议步骤：

1. 修改 `ffmpeg_writer.rs` timeout arm：超时后直接返回 `RecordingWriteFailed`。
2. 修改 worker closure：用 `catch_unwind()` 把 panic 信息通过 result channel 传回。
3. 修改 `macos_service.rs` consumer timeout arm：超时后不 join，继续后续 cleanup 并返回 finalize error。
4. 删除或改造 direct join fallback。
5. 添加 hang worker / hang consumer 测试。

### Phase C1: 修正 drop ratio 计算和边界测试

目标：

1. 使用 `dropped / (received + dropped)`。
2. 日志中的 “总计” 表示 attempted total。
3. 明确 10% 边界策略。

建议步骤：

1. 修改 system/mic drop ratio denominator。
2. 新增 `<10% pass` 测试。
3. 新增 `>10% fail` 测试。
4. 新增 `10% boundary` 测试。

### Phase E1: 让蓝牙 mic diagnostics 进入结构化结果

目标：

1. 不只打印 `CpalMicrophoneStopDiagnostics`。
2. stop 后即使重建 mic capture，诊断仍可被读取。

建议步骤：

1. 新增 `stop_with_diagnostics()`。
2. `MacRecordingService::stop()` 使用 diagnostics 返回值。
3. 将 diagnostics 挂入 `RecordingDiagnostics` / `RecordingResult` / service last field。
4. 补序列化或 getter 测试。

### Phase D1: per-source writer diagnostics

目标：

1. 记录每个 requested source 在 writer 层的 accepted/appended/discarded frames。
2. source-aware contract 不再依赖 aggregate discard ratio。

建议步骤：

1. 引入 source-aware mixed metadata。
2. 扩展 writer diagnostics 字段。
3. 修改 `validate_source_aware_audio_contract()`。
4. 补 low aggregate discard、single source missing 的回归测试。

### Phase Docs: 统一 manual gate 状态

目标：

1. `BUG.md` 与 `HANDOFF.md` 对真实设备验证状态一致。
2. manual gate 结果可追溯。

## 7. 已运行的聚焦验证

本次 review 期间运行了以下聚焦测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml audio_sample_clock -- --nocapture
```

结果：

- 4 passed
- 覆盖 `audio_sample_clock_advances_by_frames`
- 覆盖 `audio_sample_clock_lazy_offset_anchors_first_callback_start`
- 覆盖 `audio_sample_clock_lazy_offset_zero_is_stable_after_second_callback`
- 覆盖 `audio_sample_clock_lazy_offset_first_call_wins_even_when_zero`

```bash
cargo test --manifest-path src-tauri/Cargo.toml source_aware_contract_rejects -- --nocapture
```

结果：

- 2 passed
- 覆盖 requested system windows 全部被 full-overlap discard
- 覆盖 requested mic windows 全部被 full-overlap discard

```bash
cargo test --manifest-path src-tauri/Cargo.toml consume_frames_warns_but_does_not_fail_on_small_audio_drop -- --nocapture
```

结果：

- 1 passed
- 覆盖 small drop ratio 不进入 hard fail

注意：

1. 本次 review 未运行完整 test suite。
2. 测试输出仍包含既有 warnings：
   - `unused_unsafe` in `screen_capture_kit.rs`
   - `private_interfaces` for `RecordingConsumerOutput`
   - ScreenCaptureKit FFI 字段 non-snake-case / unused declarations

## 8. Review Scope Notes

1. 本文评审的是用户列出的 Phase 6 整改提交范围 `caf6cb4..5c50577`。
2. 工作区当前还存在未提交文档变更：
   - `reference/tasks/0-code-review-rectification-task.md`
   - `docs/superpowers/plans/2026-06-02-phase-6-bug-005-post-rectification-code-review-rectification.md`
   - `docs/superpowers/reviews/2026-06-02-phase-6-bug-005-post-rectification-code-review.md`
3. 本 review 将这些文档作为需求上下文参考，但 findings 的代码定位以当前工作区源码行号为准。

## 9. Final Assessment

当前状态不建议合并。

阻断原因不是整体方向错误，而是 bounded finalize 的关键语义没有真正落地：只要 timeout 后仍调用无界 `join()`，录制停止路径仍可能卡死。这是本轮 Phase A 的核心目标，也直接写入 `BUG.md` 预防规则。

建议先修 Critical 1 和 Critical 2，再处理 drop ratio 分母、蓝牙 diagnostics 返回/保留，以及 per-source writer diagnostics。完成后再跑完整回归与真实设备 manual gate。
