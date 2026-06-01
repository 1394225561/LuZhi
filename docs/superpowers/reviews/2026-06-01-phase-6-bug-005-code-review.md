# Phase 6 Code Review: BUG-005 第 25 节整改后复审

> 日期：2026-06-01
> 评审范围：Phase 6 FFmpeg playable export / original recording artifact / audio synchronizer / writer diagnostics / requested-audio contract
> Git 状态：HEAD `014d28c` + dirty worktree
> 前置章节：`docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 第 25 节
> 结论：No，不建议合并；BUG-005 仍未关闭。第 25 节修复了 BUG-009 的 writer gap 丢 PCM 问题，但真实设备验证暴露出新的双音频源同步和 contract 漏检问题。

## 1. 评审目标

本次复审重点回答两个问题：

1. Phase 6 FFmpeg playable export 相关整改是否真正修复 BUG-005 / BUG-009 引出的录制音频问题。
2. BUG-005 `验证结果` 中记录的“源视频和导出视频播放时听不见系统音频/麦克风声音、蓝牙麦克风释放异常”的根因是什么，后续应该如何修复。

额外检查：

- 是否遵守 `BUG.md` 中已有预防规则。
- 是否区分 capture-side 指标、writer-side 指标、artifact decoded 指标，避免用“麦克风 UI 电平”或“AAC frame count”冒充 artifact 有声。
- 是否遵守音视频帧流不进入前端 JS 层的数据流红线。
- 是否存在阻塞捕获主链路、资源释放、线程 join、native audio device lifecycle 等风险。

## 2. 审查输入

本轮读取和对照：

- `HANDOFF.md`
- `BUG.md` 中 BUG-005 最新 `验证结果` 日志
- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md`
- `docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md`
- `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 第 25 节
- 当前 dirty worktree 中 Phase 6 相关代码：
  - `src-tauri/src/media/ffmpeg_writer.rs`
  - `src-tauri/src/media/audio_synchronizer.rs`
  - `src-tauri/src/media/audio_mixer.rs`
  - `src-tauri/src/media/recording_writer.rs`
  - `src-tauri/src/media/ffmpeg_common.rs`
  - `src-tauri/src/media/trim_exporter.rs`
  - `src-tauri/src/platform/macos_service.rs`
  - `src-tauri/src/platform/macos/cpal_microphone.rs`
  - `src-tauri/src/platform/macos/screen_capture_kit.rs`
  - `src-tauri/src/test_support/ffmpeg_helpers.rs`
  - `src-tauri/src/lib.rs`
  - `src/components/recording-panel.tsx`
  - `src/lib/tauri.ts`

同时请求了独立 code reviewer subagent 做只读交叉审查。本文结论综合本地审查和子审查结果。

## 3. 总体结论

Phase 6 相关整改目前不能合并，BUG-005 仍不能关闭。

第 25 节整改已经修复了一个真实问题：

- `FfmpegRecordingWriter` 的 gap 分支现在会先补 silence，再 append 当前真实 PCM。
- `WriterDiagnostics` 已能区分 real PCM frames、silence padding、silent AAC track。
- `AudioSynchronizer` 已把 system/mic metadata 拆成 per-source 存储，避免简单的 2ch/1ch 元数据污染。

但 BUG-005 最新真实设备验证说明，音频链路仍有新的主缺陷：

1. 录制源视频和导出视频播放时听不见系统音频/麦克风声音。
2. source artifact 的 requested-audio contract 通过，但 decoded RMS 只有 `0.006991`，peak 只有 `0.039398`，这只证明 artifact 不是全 0，不证明可听，也不证明两个请求源都进入 artifact。
3. 双源录制日志中 paired window 只有 88 个，但 system-only 411 个、mic-only 405 个。
4. writer 收到 904 个 audio chunks，只 append 499 个，full-overlap discard 正好是 405 个。
5. `audio_chunks_discarded_full_overlap=405` 与 `mic_only_window_count=405` 完全吻合，强烈说明大量 mic-only windows 被 writer 当作重叠时间线丢弃。

本轮最可能根因：

> `AudioSynchronizer` 还不是真正的 source-aware fixed-window merger。它目前只是按 chunk 起始 timestamp 放入 20ms bucket，并用 `max(latest_system_ts, latest_mic_ts)` 推进 live watermark。真实设备中只要 system/mic 有启动延迟、callback 抖动、CPAL timestamp offset 或蓝牙 HFP 延迟，先到源就会被过早输出为单源 mixed chunk；后到源落入同一时间段时，writer 只能按 full-overlap 丢弃。

Ready to merge：**No**。

BUG-005 状态：**未关闭，需继续整改**。

## 4. BUG-005 最新日志解读

BUG.md 最新日志关键片段：

```text
RecordingDiagnostics {
  requested_system_audio: true,
  requested_microphone: true,
  system_chunks_received: 499,
  mic_chunks_received: 493,
  system_chunks_dropped: 0,
  mic_chunks_dropped: 0,
  mixed_chunks_queued: 904,
  writer_push_audio_failures: 0,
  system_rms_max: 0.017147802,
  mic_rms_max: 0.09349334,
  mixed_rms_max: 0.088926174,
  generated_silent_track: false,
  paired_window_count: 88,
  system_only_window_count: 411,
  mic_only_window_count: 405
}
```

```text
录制音频 contract 验证通过:
RMS=0.006991, peak=0.039398, samples=972800
```

```text
WriterDiagnostics {
  audio_chunks_received: 904,
  audio_chunks_appended: 499,
  audio_chunks_discarded_full_overlap: 405,
  audio_chunks_trimmed_partial_overlap: 0,
  audio_real_frames_appended: 479040,
  audio_silence_frames_padded: 3840,
  audio_real_rms_max_before_encode: 0.014637499,
  aac_frames_encoded: 474,
  silent_aac_frames_encoded: 0,
  generated_silent_track: false,
  video_queue_full_count: 0,
  audio_queue_full_count: 0
}
```

这些数字说明：

1. 采集侧不是完全无声。
   - `system_rms_max=0.017147802`
   - `mic_rms_max=0.09349334`
   - `mixed_rms_max=0.088926174`
2. 队列没有明显丢包。
   - `system_chunks_dropped=0`
   - `mic_chunks_dropped=0`
   - `writer_push_audio_failures=0`
   - `audio_queue_full_count=0`
3. 旧的 53s 音频时长膨胀问题本轮没有复现。
4. 但双源 window 合并严重异常。
   - 期望：system + mic 同时录制时，大多数 window 应该是 paired。
   - 实际：paired 只有 88，system-only 411，mic-only 405。
5. writer 的 full-overlap discard 数正好等于 mic-only window 数。
   - `mic_only_window_count=405`
   - `audio_chunks_discarded_full_overlap=405`
   - 这不是巧合，说明 mic-only chunks 很可能在 writer timeline 上被前面已写入的 system-only chunks 完整覆盖。
6. artifact contract 通过但人耳听不见，说明 contract 阈值和语义不足。
   - RMS `0.006991` 只略高于默认阈值 `0.003`。
   - peak `0.039398` 只略高于默认阈值 `0.02`。
   - 对录屏产品来说，这只能证明“解码后有微弱非零样本”，不能证明“录到了可听内容”。
   - 更不能证明 system/mic 两个请求源分别存在。

## 5. Findings

### Critical 1：AudioSynchronizer live watermark 使用 `max`，会过早输出单源窗口

位置：

- `src-tauri/src/media/audio_synchronizer.rs`
- `drain_mixed()`
- `calculate_watermark()`

当前实现：

```rust
fn calculate_watermark(&self) -> u64 {
    let latest_ts = self.latest_system_ts.max(self.latest_mic_ts);
    latest_ts.saturating_sub(HOLD_NANOS)
}
```

问题：

1. 注释语义是“给迟到源留 hold window”，但 `max(system, mic)` 会让快的一路推动水位。
2. 当 system audio timestamp 领先 mic 40ms 以上时，system-only window 会被提前输出。
3. 之后 mic chunk 到达同一时间段时，原 window 已经从 `windows` 中删除，只能产生 mic-only window 或落入后续 window。
4. writer 看到两个同一时间段附近的 single-source mixed chunks，只能按 timeline overlap append 第一个、discard 后一个。

与 BUG.md 日志对应：

```text
paired_window_count: 88
system_only_window_count: 411
mic_only_window_count: 405
audio_chunks_discarded_full_overlap: 405
```

影响：

- 双源录制实际退化成大量单源 chunk。
- 后到源可能完全被 writer 丢弃。
- 麦克风 UI 电平仍然有波动，因为 capture-side 收到了 mic samples，但 artifact 没有保留这些 samples。

修复建议：

1. `AudioSynchronizer` 必须知道本次 requested sources。
2. 当 `requested_system_audio && requested_microphone` 且两个源都已 seen 时，watermark 应以慢的一路为准：

```rust
let latest_ts = self.latest_system_ts.min(self.latest_mic_ts);
latest_ts.saturating_sub(HOLD_NANOS)
```

3. 如果某一路长期未出现，不能无限等待。应增加 source timeout，例如：
   - `SOURCE_START_GRACE_NANOS`：录制开始后等待另一源启动。
   - `SOURCE_STALL_TIMEOUT_NANOS`：某源曾经出现但持续停滞，允许输出单源窗口，同时记录 timeout。
4. `drain_mixed()` 不应只返回 `MixedAudioChunk`，应返回 `SynchronizedAudioChunk`，让 caller 知道每个输出 window 是否包含 system/mic。

### Critical 2：AudioSynchronizer 按 chunk 起始时间整块归桶，没有按 20ms window 切分样本

位置：

- `src-tauri/src/media/audio_synchronizer.rs`
- `push_system()`
- `push_mic()`
- `emit_window()`

当前行为：

1. 根据 `chunk.timestamp.nanos / window_nanos` 选择一个 window。
2. 把整个 `chunk.samples` append 到该 window 的 source buffer。
3. `emit_window()` 把整个 buffer 以 `window_start_nanos` 作为 timestamp 输出。

问题：

- 真实 callback 不保证正好 20ms。
- 蓝牙麦克风、24kHz/1ch 设备或 CoreAudio buffer 可能一次回调超过 20ms。
- 如果一个 chunk 实际覆盖 40ms 或更长，当前实现仍把它作为一个 20ms window 起点输出。
- writer 以输出 chunk 的 sample length 推进 `audio_timeline_cursor`，这会覆盖后续多个 window。
- 后续 window 的 mic/system chunk 即使 timestamp 不同，也可能落在 writer 已写入范围内，被 full overlap discard。

与日志对应：

```text
audio_chunks_received: 904
audio_chunks_appended: 499
audio_chunks_discarded_full_overlap: 405
```

修复建议：

1. `AudioSynchronizer` 不应按 chunk 归桶，而应按 sample frame 切分。
2. 对每个输入 source chunk：
   - 根据 `sample_rate/channels` 计算每个 frame 的时间范围。
   - 将样本切到一个或多个 fixed windows。
   - 每个 window 中只保留该窗口对应的样本片段。
3. 每个输出 window 应为固定 20ms duration，或最后一个 final window 允许短帧但必须显式标记。
4. 缺失源以同 window duration 的 silence 补齐，再交给 mixer。
5. 新增测试覆盖：
   - `synchronizer_splits_long_system_callback_to_multiple_windows`
   - `synchronizer_splits_long_mic_callback_to_multiple_windows`
   - `dual_source_offset_does_not_discard_mic_windows`

### Critical 3：requested-audio contract 是 aggregate-only，无法证明每个请求源存在

位置：

- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/platform/macos_service.rs`

当前 contract：

```rust
if rms < contract.min_rms && peak < contract.min_peak {
    return Err(...);
}
```

问题：

1. 它只检查 artifact 整体 decoded RMS/peak。
2. 如果请求 system+mic，但 mic 完全丢失，只要 system 有一点残余非零样本，contract 仍可能通过。
3. 当前阈值偏低，`RMS=0.006991`、`peak=0.039398` 就通过，但用户实际听不见。
4. contract 未把 capture-side per-source RMS 与 writer-side per-source append/discard 关联起来。

违反的 BUG.md 预防规则：

- “请求录制音频源”必须和“实际写入非静音音频内容”建立可验证 contract。
- 麦克风 UI 电平只能作为 capture-side indicator，不能作为 recording artifact 成功证据。
- artifact validation 必须包含 audio RMS/peak 检查，不能只依赖 stream presence 和 duration。

本轮需要补充的新规则：

- artifact aggregate RMS/peak 只能证明“artifact 非全静音”，不能证明“每个请求源都被保留”。
- requested-audio contract 必须 source-aware。

修复建议：

1. `SynchronizedAudioChunk` 携带 per-source 元数据：
   - `has_system`
   - `has_mic`
   - `system_rms`
   - `mic_rms`
   - `system_frames`
   - `mic_frames`
2. `RecordingDiagnostics` 增加：
   - `system_windows_emitted`
   - `mic_windows_emitted`
   - `system_windows_appended_to_writer`
   - `mic_windows_appended_to_writer`
   - `system_windows_discarded_by_writer`
   - `mic_windows_discarded_by_writer`
   - `system_rms_max_before_writer`
   - `mic_rms_max_before_writer`
3. writer 输入可以扩展为 source-aware mixed chunk metadata，或另建旁路 diagnostics channel。
4. 当请求某源且 capture RMS 非零，但该源 emitted/appended 为 0 或 discard ratio 过高，应失败。
5. 对当前日志，`mic_rms_max=0.09349334` 但 `mic_only_window_count=405` 且 full-overlap discard=405，应被判为 contract failure。

### Important 1：CPAL 麦克风时间戳 offset 在 stream build 时固定，蓝牙/慢启动设备容易产生系统性偏移

位置：

- `src-tauri/src/core/clock.rs`
- `AudioSampleClock::with_session_clock()`
- `src-tauri/src/platform/macos/cpal_microphone.rs`
- `build_input_stream()`

当前逻辑：

```rust
let mut sample_clock = AudioSampleClock::new(sample_rate, channels);
if let Some(ref clock) = session_clock {
    sample_clock = sample_clock.with_session_clock(clock);
}
```

`with_session_clock()` 在 build stream 时设置 offset：

```rust
self.session_offset_nanos = session.elapsed_nanos();
```

问题：

1. stream build 与第一帧 callback 之间可能存在明显延迟。
2. 蓝牙耳机麦克风尤其可能因为 HFP profile 切换、设备启动、buffer warm-up 产生延迟。
3. offset 过早固定会让 mic timestamp 与真实到达的 system/video timestamp 有系统偏移。
4. 如果偏移超过 synchronizer hold window，就会触发 Critical 1 的单源窗口洪泛。

修复建议：

1. 优先读取 `cpal::InputCallbackInfo` 中的 input timestamp。如果可用，用设备实际 capture timestamp 对齐 session clock。
2. 如果 cpal timestamp 不可靠，则在首个 callback 内初始化 offset：

```text
first_chunk_start = session_clock.elapsed_nanos() - buffer_duration_nanos
```

3. `AudioSampleClock` 应支持 lazy offset 初始化，而不是 build stream 时固定。
4. 增加测试：
   - `audio_sample_clock_lazy_offset_anchors_first_callback_start`
   - `mic_clock_startup_delay_does_not_shift_first_chunk_by_stream_build_time`

### Important 2：导出阶段未使用 requested-audio contract

位置：

- `src-tauri/src/lib.rs`
- `export_video()`

当前搜索结果显示导出阶段仍调用：

```rust
media::ffmpeg_common::validate_export_artifact(...)
```

问题：

- `validate_export_artifact_with_audio_contract()` 已存在，但 export command 没有使用。
- 如果 source artifact 勉强通过，export 可能继续输出低电平或近乎静音 artifact。
- 用户反馈“源视频、导出视频都听不见”，说明 source 和 export 两个阶段都需要 contract。

修复建议：

1. 在 `AppState` 或 `MacRecordingService` 中保存 last recording 的 requested audio config。
2. `export_video()` 构造 `RequestedAudioContract`。
3. 用 `validate_export_artifact_with_audio_contract()` 替代 plain validation。
4. 后续 source-aware contract 完成后，export 也应继承 source-aware 结果，而不是只看 aggregate RMS/peak。

### Important 3：writer partial-overlap diagnostics 没有递增

位置：

- `src-tauri/src/media/ffmpeg_writer.rs`
- `append_audio_chunk_to_timeline()`
- `encoder_worker()`

当前逻辑：

- full overlap 时 `chunk_appended=false`，外层递增 `audio_chunks_discarded_full_overlap`。
- partial overlap 时会 trim 并 append remaining samples，但没有告诉外层发生了 partial trim。
- `audio_chunks_trimmed_partial_overlap` 因此仍可能长期为 0。

问题：

- 违反 `BUG.md` 中 writer diagnostics 必须区分 appended/discarded/trimmed/encoded 的预防规则。
- 后续定位 timeline corruption 时会低估 overlap 情况。

修复建议：

1. 扩展 `TimelineAppendResult`：

```rust
struct TimelineAppendResult {
    chunk_appended: bool,
    silence_frames_padded: u64,
    appended_frames: u64,
    trimmed_partial_overlap: bool,
    trimmed_frames: u64,
}
```

2. partial overlap 分支设置 `trimmed_partial_overlap=true`。
3. 外层递增：

```rust
if append_result.trimmed_partial_overlap {
    writer_diag.audio_chunks_trimmed_partial_overlap += 1;
}
```

### Important 4：蓝牙麦克风释放问题需要资源生命周期验证，不能只做 UI 提示

位置：

- `src-tauri/src/platform/macos_service.rs`
- `src-tauri/src/platform/macos/cpal_microphone.rs`

BUG.md 新现象：

- 同时开启系统音频和蓝牙耳机麦克风，结束录制后没有释放麦克风进程，耳机音质持续很差。
- 系统默认麦克风如果实际指向蓝牙耳机，结束录制后可以释放，音质恢复。

当前 stop 顺序：

```rust
let capture_result = ScreenCapture::stop(&mut self.screen_capture);
let mic_result = self.mic_capture.stop();
if let Some(flag) = &self.stop_flag {
    flag.store(true, Ordering::Relaxed);
}
```

`CpalMicrophoneCapture::stop()`：

```rust
self.running.store(false, Ordering::Relaxed);
self.stream = None;
Ok(())
```

风险：

1. `cpal::Stream` drop 对蓝牙 HFP profile 的释放可能不是同步完成。
2. 当前没有 stop 后确认回调停止、没有等待 CoreAudio device release、没有重新创建 `CpalMicrophoneCapture` 实例。
3. 显式蓝牙设备路径和系统默认设备路径可能走 CoreAudio 不同 device handle，表现不同。

修复建议：

1. stop 时先停止 mic capture，再停止 ScreenCaptureKit，降低蓝牙 HFP profile 持有时间。
2. `CpalMicrophoneCapture::stop()` 中：
   - `running=false`
   - take stream 到局部变量并显式 drop
   - 增加短 bounded wait，例如 100-300ms，让 CoreAudio 完成设备释放。
3. stop 后重置 `self.mic_capture = CpalMicrophoneCapture::new()`，避免旧 device handle 残留。
4. 增加 diagnostic log：
   - selected mic device
   - actual stream config
   - stop start/end
   - callbacks after stop count
5. 对蓝牙设备继续保留 UI warning，但不能把 release 问题标记为仅 UI 已处理。

## 6. 推荐修复计划

### R1：先补失败测试，锁定同步器现有缺陷

新增测试建议：

1. `dual_source_offset_does_not_discard_mic_windows`
   - system 每 20ms 一个 chunk。
   - mic 整体延迟 60ms，但仍代表同一录制时间线。
   - 期望：在双源 requested 模式下，synchronizer 不应把大批 system-only 提前输出。

2. `synchronizer_splits_long_callbacks_to_fixed_windows`
   - 构造一个 60ms mic chunk。
   - 期望被拆成 3 个 20ms windows。
   - 每个 window 输出 sample length 与 20ms 对齐。

3. `synchronizer_does_not_emit_fast_source_before_slow_source_watermark`
   - system 领先 mic 多个 windows。
   - 期望在 timeout 前不输出 system-only。

4. `requested_system_and_mic_requires_each_source_appended`
   - capture-side system/mic RMS 均非零。
   - 模拟 writer 丢弃 mic source。
   - 期望 contract failure。

5. `export_contract_rejects_requested_audio_near_silence`
   - 构造低 RMS artifact。
   - 请求音频时 export validation 应失败。

### R2：重构 AudioSynchronizer 为真正 fixed-window sample merger

实现要点：

1. `AudioSynchronizer::new()` 或新 constructor 接收 requested source flags：

```rust
pub struct AudioSynchronizerConfig {
    pub requested_system_audio: bool,
    pub requested_microphone: bool,
    pub window_nanos: u64,
    pub hold_nanos: u64,
    pub source_stall_timeout_nanos: u64,
}
```

2. `push_system()` / `push_mic()` 调用共享 helper：

```rust
fn push_source_chunk(source: AudioSource, chunk: AudioChunk)
```

3. helper 按 frame 切分：

```text
chunk_start_nanos = chunk.timestamp.nanos
frame_duration = 1_000_000_000 / sample_rate
for frame range overlapping each 20ms window:
  copy exact source samples into that window
```

4. 每个 window 的 source buffer 记录：

```rust
struct SourceWindowBuffer {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
    source_frames: u64,
    rms_max: f32,
}
```

5. `drain_mixed()` 返回：

```rust
Vec<AppResult<SynchronizedAudioChunk>>
```

6. `SynchronizedAudioChunk` 包含：

```rust
pub struct SynchronizedAudioChunk {
    pub mixed: MixedAudioChunk,
    pub has_system: bool,
    pub has_mic: bool,
    pub system_rms: f32,
    pub mic_rms: f32,
    pub system_frames: u64,
    pub mic_frames: u64,
    pub emitted_due_to_timeout: bool,
}
```

### R3：改 macos_service consumer 统计 source-aware diagnostics

需要修改：

- live drain loop
- final drain loop
- `RecordingDiagnostics`

新增或调整字段：

```rust
pub system_windows_emitted: u64,
pub mic_windows_emitted: u64,
pub paired_window_count: u64,
pub system_only_window_count: u64,
pub mic_only_window_count: u64,
pub source_timeout_window_count: u64,
pub system_rms_max_before_writer: f32,
pub mic_rms_max_before_writer: f32,
```

并在 writer push 前记录每个 source 是否进入 mixed output。

### R4：source-aware requested-audio contract

短期实现：

1. 如果请求某源且 capture-side RMS 超过阈值，但 synchronizer emitted source window 数为 0，失败。
2. 如果请求某源且该源 emitted window discard ratio 过高，失败。
3. 如果 artifact aggregate RMS 低于更贴近可听的阈值，warning 或 failure。

建议阈值：

- 保留 `min_rms=0.003` 用作“非全静音”底线。
- 增加 `audible_min_rms`，例如 `0.01` 或 `0.015`，用于真实设备 manual gate。
- 对 mic 可用相对阈值：artifact/source RMS 不应低于 capture RMS 的某个比例，例如 20%-30%。具体比例需要用真实设备样本校准。

长期实现：

- 真正区分 system/mic 在 final artifact 中的贡献需要分轨或源标记后的 offline inspection。MVP 可以先用 source-aware synchronizer/writer diagnostics 做强约束。

### R5：修正 CPAL 时间戳锚点

建议步骤：

1. 先补测试证明当前 build-time offset 会造成启动延迟偏移。
2. `AudioSampleClock` 支持 lazy first-callback anchor。
3. 在 callback 中用 data length 计算 buffer duration：

```rust
let frames = data.len() as u64 / channels as u64;
let buffer_duration_nanos = frames * 1_000_000_000 / sample_rate as u64;
let callback_now = session_clock.elapsed_nanos();
let first_start = callback_now.saturating_sub(buffer_duration_nanos);
```

4. 后续 chunk 仍按 emitted frames 单调递增。

### R6：导出阶段使用 audio contract

修改 `export_video()`：

1. 保存 last recording audio request：

```rust
last_requested_system_audio
last_requested_microphone
last_microphone_device
```

2. 导出完成后：

```rust
let contract = RequestedAudioContract {
    requested_system_audio,
    requested_microphone,
    ..Default::default()
};
validate_export_artifact_with_audio_contract(&output, width, height, &contract)?;
```

3. 待 R4 完成后，导出也应检查 source-aware diagnostics 或继承 source artifact contract 结果。

### R7：蓝牙麦克风释放路径收口

建议先不把它与主音频无声 bug 混在同一 patch 中修。它应该作为独立小修：

1. 调整 stop 顺序：mic stop before screen stop。
2. 显式 drop stream 并 bounded wait。
3. stop 后重建 `CpalMicrophoneCapture`。
4. 增加 callbacks-after-stop diagnostic。
5. 人工验证：
   - 显式选择蓝牙麦克风，停止后音质是否恢复。
   - 系统默认麦克风指向蓝牙时，停止后音质是否恢复。
   - 内置麦克风不应回归。

## 7. 推荐验证命令

现有聚焦测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_preserves_non_silent_audio_after_leading_gap -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_preserves_non_silent_audio_after_middle_gap -- --nocapture
```

新增测试完成后建议运行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg dual_source_offset_does_not_discard_mic_windows -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg synchronizer_splits_long_callbacks_to_fixed_windows -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_system_and_mic_requires_each_source_appended -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg export_contract_rejects_requested_audio_near_silence -- --nocapture
```

完整回归建议：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

真实设备 manual gate：

1. 只录系统音频 10 秒，播放音乐，验证 source/export 都可听。
2. 只录内置麦克风 10 秒，说话，验证 source/export 都可听。
3. 系统音频 + 内置麦克风 10 秒，验证两路都可听。
4. 系统音频 + 蓝牙麦克风 10 秒，验证：
   - 录制中音质下降是否提示明确。
   - 停止后蓝牙输出音质是否恢复。
   - source/export 是否可听。
5. 每次验证都记录：
   - `RecordingDiagnostics`
   - `WriterDiagnostics`
   - artifact decoded RMS/peak/sample_count
   - paired/system-only/mic-only/source-timeout window counts

## 8. BUG.md 预防规则补充建议

建议在 BUG-005 下新增以下预防规则：

1. `AudioSynchronizer` 不能只按 chunk 起始 timestamp 归桶；真实音频 chunk 必须按 sample frame 切分到固定时间窗口。
2. 双源录制时 live watermark 不能由快的一路单独推进；在两个请求源都 active 时必须以慢源或 source-aware timeout 策略决定发射。
3. requested-audio contract 必须 source-aware；aggregate decoded RMS/peak 只能证明 artifact 非全静音，不能证明每个请求源都存在。
4. 当 capture-side 某请求源 RMS 非零但 writer/source-aware diagnostics 显示该源被大量 overlap discard 时，必须视为录制失败或至少阻断 BUG 关闭。
5. CPAL 麦克风 timestamp 不能在 stream build 时固定 offset；首帧 callback 或设备 timestamp 才能作为输入流真实起点。
6. 蓝牙麦克风 UI warning 不能替代资源释放验证；显式蓝牙设备 stop 后必须验证 stream drop 与音质恢复。

## 9. 本轮未执行项

本轮用户请求是审查、定位和记录方案，因此只做文档记录，没有修改生产代码。

未执行自动测试：

- 未运行 `cargo test`。
- 未运行 `npm test`。
- 未运行真实设备 manual gate。

后续编码应先补失败测试，再按 R1-R7 分阶段修复。
