# Phase 6 BUG-005 / BUG-005_2 整改后 Code Review Findings

> 日期：2026-06-02
> 审查范围：`e77b9ab9d1b75b060e43a0e4a58e76ebeeccdb72..caf6cb400d6d2a29128d5242244b30b7cdb2cc2b`
> 审查目标：复审 Phase 6 音频链路相关整改，确认 BUG-005 / BUG-005_2 修复方案，并记录后续编码需要处理的风险点。

## 1. 输入依据

- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/reviews/2026-06-01-phase-6-bug-005-code-review.md`
- `BUG.md`
- `HANDOFF.md`
- 本轮重点代码：
  - `src-tauri/src/core/clock.rs`
  - `src-tauri/src/lib.rs`
  - `src-tauri/src/media/audio_synchronizer.rs`
  - `src-tauri/src/media/ffmpeg_common.rs`
  - `src-tauri/src/media/ffmpeg_writer.rs`
  - `src-tauri/src/media/recording_writer.rs`
  - `src-tauri/src/platform/macos/cpal_microphone.rs`
  - `src-tauri/src/platform/macos_service.rs`
  - `src-tauri/src/test_support/ffmpeg_helpers.rs`

## 2. 总体结论

1. **BUG-005 / BUG-005_2 的核心修复方向成立**。
   - `audible_min_rms=0.015` 已从 hard fail 降级为 warning，解决了 BUG-005_2 中 `RMS=0.010835 < 0.015` 导致录制失败的 false positive。
   - `min_rms + min_peak` 仍作为近静音 hard gate，避免真正 silent artifact 被误判成功。
   - source-aware presence contract 已从 RMS 门控改为 chunks/windows/frames 计数，避免低音量源被误判为缺失。
   - writer gap 分支已保证 padding silence 后继续 append 当前真实 PCM，避免 BUG-009/BUG-005 中 leading gap 吞真实音频的问题复发。

2. **本轮未发现直接推翻人工验证结果的 Critical 缺陷**。
   - 用户已人工验证 BUG-005 和 BUG-005_2 通过。
   - 代码层面确认 low-RMS contract、source-aware contract、dual-source grace、writer gap RMS 保留相关测试均通过。

3. **仍有若干 Important 风险需要后续编码收口**。
   - 这些问题主要影响 stop/finalize 的极端卡死、低概率 false positive、timestamp 边界跳变，以及 source-aware diagnostics 的证据强度。
   - 建议在继续 Phase 6 合并前至少处理 Important 1-3；Important 4 和 Minor 1 可作为后续 hardening，但已经写入 `BUG.md` 预防规则。

## 3. BUG-005 修复方案总结

BUG-005 的根因是多层叠加，不是单点问题：

1. CPAL 使用 UI 目标格式作为硬件 stream config，真实设备只支持 24kHz/1ch 时会失败或异常。
2. writer audio timeline 早期没有完整覆盖 first gap、middle gap、tail gap、full overlap、partial overlap、out-of-order。
3. mixer 多声道处理曾存在 metadata/sample layout 风险。
4. 旧 synchronizer 以 per-chunk 配对，导致 system/mic 可能在同一时间轴双写。
5. `audible_min_rms` 曾被升级为硬失败阈值，制造 BUG-005_2 false positive。

当前修复组合：

1. `cpal_microphone.rs` 使用设备 `default_input_config()`，UI 目标格式只作为下游 mixer/output 目标。
2. `AudioSynchronizer` 改为 source-aware fixed-window merger，同一窗口只输出一个 mixed chunk。
3. synchronizer 保留 per-source metadata，避免 system 48kHz/2ch 与 mic 24kHz/1ch 或 48kHz/1ch 混淆。
4. writer timeline helper 统一处理 gap/overlap/contiguous，gap 分支补静音后继续 append 真实 samples。
5. writer diagnostics 区分 received/appended/discarded/trimmed/real frames/silence padding/silent track。
6. source/export artifact validation 解码 AAC 并检查 decoded RMS/peak。
7. source-aware contract 使用 chunks/windows/frames 判定请求源是否存在，不再把低 RMS 当成源缺失。
8. 蓝牙麦克风 stop 顺序调整为 mic first，并显式 pause/drop/recreate mic capture。

## 4. BUG-005_2 修复方案总结

BUG-005_2 的直接根因：

> `audible_min_rms=0.015` 是“可听性建议阈值”，但 Section 10 将它作为 source/export artifact validation 的 hard fail。真实录制 `RMS=0.010835` 已高于 `min_rms=0.003`，且 writer 诊断证明真实 PCM 已进入 AAC；但因为低于 `0.015` 被误判为失败。

当前修复组合：

1. `validate_source_artifact_with_audio_contract()` 中 `audible_min_rms` 只打印 warning，不返回 `Err`。
2. `validate_export_artifact_with_audio_contract()` 中 `audible_min_rms` 同样只打印 warning。
3. hard fail 只保留 Level 1 条件：`rms < min_rms && peak < min_peak`。
4. 新增 low-RMS 回归测试，覆盖 `RMS≈0.010` 通过、near-silent 失败。
5. `BUG.md` 已补充 BUG-005 和 BUG-005_2 的预防规则，尤其是“不能根据 aggregate RMS 直接判定用户听感响度”。

## 5. Code Review Findings

### Important 1：`finish()` / consumer join 仍不是严格 bounded

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:198`
- `src-tauri/src/media/ffmpeg_writer.rs:250`
- `src-tauri/src/platform/macos_service.rs:342`

问题：

1. `FfmpegRecordingWriter::finish()` 只对 `Flush` 消息发送做了 bounded retry。
2. `self.tx.take()` 能解决 worker 卡在 `rx.recv()` 的一类问题，但不能解决 worker 卡在 FFmpeg flush、muxer IO、`write_trailer()`、artifact validation 或其他阻塞路径的问题。
3. `join_worker()` 内部仍直接调用 `handle.join()`。
4. `MacRecordingService::stop()` 对 consumer thread 的 `handle.join()` 也没有超时。
5. 当前 10s warning 是在 `join()` 返回后才打印，因此不是 bounded join；如果 `join()` 永不返回，warning 也不会出现。

影响：

- 用户点击停止录制时，极端情况下仍可能无限等待。
- 这违反架构文档中“捕获/停止链路不能被编码或 IO 长时间阻塞”的原则。
- 对蓝牙 profile release 也有间接影响：即使 mic 已 stop，UI/状态机可能仍卡在 finalize。

建议修复：

1. 将 encoder worker result 通过 `mpsc`/`oneshot` 返回，前端 holder 不直接无界 `join()`。
2. `finish()` 在发送 Flush 后等待 result channel，设置明确超时，例如 10s。
3. 超时后返回 `RecordingWriteFailed`，记录 worker 可能泄漏/仍在后台，但不阻塞 stop 路径。
4. consumer thread join 同样引入 bounded wait 或拆分为可超时 result channel。
5. 增加 fake worker / slow writer 测试：
   - `ffmpeg_writer_finish_times_out_when_worker_never_returns`
   - `mac_recording_stop_does_not_wait_unbounded_for_consumer`

### Important 2：CPAL lazy offset 用 `0` 同时表示未初始化和合法 offset

位置：

- `src-tauri/src/core/clock.rs:118`
- `src-tauri/src/core/clock.rs:124`
- `src-tauri/src/platform/macos/cpal_microphone.rs:279`

问题：

1. `AudioSampleClock.session_offset_nanos` 初始值为 `0`。
2. `initialize_offset()` 使用 `compare_exchange(0, offset, ...)` 判断“首次初始化”。
3. 但真实 `offset=0` 是合法值：首个 CPAL callback 早于或等于 buffer duration 时，`callback_now.saturating_sub(buffer_duration)` 会得到 `0`。
4. 如果首次 offset 合法为 0，后续 callback 仍会认为 offset 未初始化，并可能 CAS 成非 0。
5. 这会导致 mic timestamp 在 emitted frames 已推进后发生整体 offset 跳变。

触发示例：

1. session start 后 5ms 进入首个 callback。
2. buffer duration 为 10ms。
3. `offset = 5ms - 10ms = 0`。
4. 第一批 timestamp 从 0 开始。
5. 第二个 callback 到 15ms，`offset = 15ms - 10ms = 5ms`，CAS 从 0 变成 5ms 成功。
6. 第二批 timestamp 变成 `已发帧时间 + 5ms`，中途多出 5ms gap。

影响：

- mic timestamp 可能出现一次性跳变。
- system/mic source-aware window 对齐可能被扰动。
- 在双源录制、蓝牙设备低 buffer/快速 callback 下可能出现偶发未配对窗口或不必要 silence padding。

建议修复：

1. 使用 sentinel，例如 `AtomicU64::new(u64::MAX)` 表示未初始化。
2. 或新增 `AtomicBool offset_initialized`，offset 本身允许为 0。
3. 增加测试：
   - `audio_sample_clock_lazy_offset_zero_is_stable_after_second_callback`
   - `audio_sample_clock_lazy_offset_first_call_wins_even_when_zero`

### Important 3：requested audio channel drop 被当成 stop 失败，可能制造 false positive

位置：

- `src-tauri/src/platform/macos_service.rs:813`
- `src-tauri/src/platform/macos_service.rs:821`

问题：

1. 代码文案是“警告: 系统音频通道丢弃了 N 个音频块”。
2. 但该 warning 会被 `errors.push(msg)` 收集。
3. `MacRecordingService::stop()` 最终只要 `errors` 非空就返回 `RecordingFinalizeFailed`。
4. 因此，只要 requested system/mic 发生任意 chunk drop，即使最终 artifact 可听、RMS/peak contract 通过、source-aware contract 通过，也会让录制失败。

影响：

- 高负载下偶发 1 个 audio chunk drop 会变成用户可见失败。
- 这与 BUG-005_2 的模式相似：diagnostic warning 被升级成 hard failure。
- 对长时间录制尤其敏感，录制越久越容易出现偶发 drop。

建议修复：

1. 将少量 drop 降级为 diagnostics warning，不进入 `errors`。
2. 设置 hard fail 条件，例如：
   - drop ratio 超过阈值；
   - 或 drop 后 artifact contract/source-aware contract 失败；
   - 或连续长时间音频断流。
3. 在 `RecordingDiagnostics` 中保留 drop count 和 drop ratio。
4. 增加测试：
   - `consume_frames_warns_but_does_not_fail_on_small_audio_drop_when_contract_passes`
   - `consume_frames_fails_on_excessive_audio_drop_ratio`

### Important 4：source-aware contract 证明力仍停留在 writer 前，不是 artifact-level per-source 证据

位置：

- `src-tauri/src/platform/macos_service.rs:583`
- `src-tauri/src/platform/macos_service.rs:715`
- `src-tauri/src/media/recording_writer.rs:134`
- `src-tauri/src/media/recording_writer.rs:178`

问题：

1. `system_windows_before_writer` / `mic_windows_before_writer` 在 `writer.push_audio()` 前递增。
2. contract 文案把这些字段视为 “reached writer” 证据，但严格来说它们只是 “synchronizer produced source content before push”。
3. `writer.push_audio()` 失败时已经会进入 `errors`，所以这不是当前最直接的 false negative。
4. 但从 source-aware 证据链看，目前仍无法证明每一路请求源都实际进入最终 artifact：
   - artifact RMS/peak 是 aggregate；
   - writer diagnostics 是 aggregate mixed chunk；
   - 没有 per-source post-writer / post-mix / encoded contribution 统计。
5. 如果未来某一路只出现在少数窗口，而这些窗口在 writer 层被 aggregate discard ratio 掩盖，contract 可能无法定位具体缺失源。

影响：

- 当前合同足以定位 BUG-005_2 的直接根因，但还不是完整的 per-source artifact contract。
- 对“系统音频可听但 mic 缺失”或“mic 可听但 system 缺失”的精确自动判定仍有限。

建议修复：

1. 将字段名或文案明确为 `before_writer_push`，避免误导。
2. 在 synchronizer/mixer 输出中保留 per-source contribution metadata，并随 mixed chunk 进入 writer diagnostics。
3. writer diagnostics 增加 per-source accepted/appended/discarded frames，至少在 source-aware mixed chunk metadata 层记录。
4. 增加测试：
   - `source_aware_contract_rejects_when_requested_mic_windows_are_all_discarded`
   - `writer_diagnostics_records_per_source_appended_frames`

### Minor 1：蓝牙 mic stop 的错误与释放诊断仍不完整

位置：

- `src-tauri/src/platform/macos/cpal_microphone.rs:182`
- `src-tauri/src/platform/macos/cpal_microphone.rs:187`
- `src-tauri/src/platform/macos/cpal_microphone.rs:196`

问题：

1. `pause()` 失败只 `eprintln!`，不会进入 `AppResult` 或 diagnostics。
2. stop 后是否还有 callback 触发没有计数。
3. 300ms wait 是经验值，没有 release-stable 指标。
4. 蓝牙 HFP 是否恢复仍主要依赖人工听感 manual gate。

影响：

- 当前人工验证已通过，不阻断 BUG-005/BUG-005_2 关闭。
- 但如果后续再次出现蓝牙 profile 未释放，诊断字段还不足以快速区分 pause 失败、drop 失败、callback after stop、系统 profile 切换慢。

建议修复：

1. 在 `CpalMicrophoneCapture` 增加 stop diagnostics：
   - `pause_attempted`
   - `pause_failed`
   - `stream_dropped`
   - `callbacks_after_stop`
   - `stop_wait_ms`
2. `pause()` 失败至少进入 warning diagnostics；是否 hard fail 由上层策略决定。
3. 保留 manual gate：蓝牙麦克风录制后，确认系统输出音质在停止后恢复。

## 6. 已更新的 BUG.md 预防规则

本轮已在 `BUG.md` 中补充：

1. `BUG-005` 新增完整预防规则，覆盖 CPAL 配置协商、source-aware synchronizer、writer timeline、diagnostics、bounded stop、CPAL lazy offset、蓝牙 stop、manual gate。
2. `BUG-005_2` 扩充预防规则，明确：
   - `audible_min_rms` 只能 warning；
   - aggregate RMS 会被 silence padding 稀释；
   - hard fail 只由 `rms < min_rms && peak < min_peak` 触发；
   - low-RMS source/export regression 必须保留；
   - 修改阈值前必须查看真实设备日志。

## 7. 建议后续编码 Phase

### Phase A：真正 bounded finalize

目标：

- `finish()` 和 `stop()` 不再无界等待 worker / consumer。

建议步骤：

1. 重构 writer worker result 返回方式，避免直接无界 `join()`。
2. 为 writer finish result 设置超时。
3. 为 consumer result 设置超时。
4. 超时返回明确错误，并保留诊断。
5. 添加 fake worker / slow consumer 测试。

### Phase B：修复 CPAL lazy offset sentinel

目标：

- offset 合法为 0 时不会被后续 callback 重新初始化。

建议步骤：

1. 将 `session_offset_nanos` 初始值改为 sentinel 或增加 `AtomicBool`。
2. 保证 first-call wins，即使 first offset 是 0。
3. 添加两条 zero-offset 回归测试。

### Phase C：drop warning 与 hard failure 分层

目标：

- 少量音频 drop 不再直接导致 stop 失败。

建议步骤：

1. `system_chunks_dropped > 0` / `mic_chunks_dropped > 0` 先进入 diagnostics warning。
2. 增加 drop ratio 阈值。
3. 只有 drop ratio 过高或 artifact/source-aware contract 失败时才 hard fail。
4. 添加 small-drop pass 与 high-drop fail 测试。

### Phase D：source-aware post-writer diagnostics

目标：

- per-source contract 从 “before writer push” 升级到更接近 artifact contribution 的证据链。

建议步骤：

1. 明确命名现有字段为 before-push 语义，或更新文案。
2. 将 per-source metadata 随 mixed chunk 传入 writer diagnostics。
3. 记录 per-source accepted/appended/discarded frames。
4. 增加 per-source writer discard 回归测试。

### Phase E：蓝牙 mic release diagnostics hardening

目标：

- 下次蓝牙 profile 未释放时能从日志中直接定位 stop lifecycle 边界。

建议步骤：

1. 增加 `CpalMicrophoneStopDiagnostics`。
2. 记录 pause/drop/wait/callback-after-stop。
3. stop 后重建 capture 实例保持现状。
4. 保留真实设备 manual gate。

## 8. 已运行验证

本轮 review 后执行了以下验证：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract -- --nocapture
```

结果：

- 3 passed
- 覆盖 low-RMS source contract、near-silent reject、any_audio_requested。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg validate_source_aware_audio_contract -- --nocapture
```

结果：

- 3 passed
- 覆盖 missing system before-writer、missing mic before-writer、capture RMS nonzero but before-writer RMS zero。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer_dual_source -- --nocapture
```

结果：

- 2 passed
- 覆盖 dual-source start grace 等待和 grace timeout emission。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_sample_clock -- --nocapture
```

结果：

- 2 passed
- 现有测试通过，但尚未覆盖 first offset 合法为 0 后的二次 callback 稳定性。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_preserves_non_silent_audio_after -- --nocapture
```

结果：

- 2 passed
- 覆盖 leading gap / middle gap 后 decoded RMS 与 peak 保留。

```bash
git diff --check
```

结果：

- 通过。

## 9. 既有 warnings

测试输出中仍存在既有 warning，未在本轮修改：

1. `unused_unsafe`：`src/platform/macos/screen_capture_kit.rs`
2. `private_interfaces`：`RecordingConsumerOutput` 比 `MacRecordingService::consume_frames` 更私有
3. `dead_code`：`TimelineAppendResult.trimmed_frames` 未读
4. ScreenCaptureKit FFI 字段 non-snake-case / unused declarations

这些 warning 不属于 BUG-005_2 的直接根因，但建议后续单独清理，尤其是 `private_interfaces` 和 `trimmed_frames`，避免真实诊断字段继续“写了但没人读”。

## 10. 合并建议

1. BUG-005 / BUG-005_2 的核心修复可以视为已通过人工验证和自动回归的当前基线。
2. 若本分支准备进入更高稳定性门槛，建议先处理：
   - Important 1：bounded finalize
   - Important 2：CPAL lazy offset sentinel
   - Important 3：drop warning/error 分层
3. Important 4 和 Minor 1 建议作为 Phase 6 audio hardening 的后续任务，不建议完全忽略。
4. 不建议再把未经真实设备标定的 loudness 阈值升级为 hard fail；这条已经写入 `BUG.md` 预防规则。
