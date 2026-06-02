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

## 10. 追加复审（2026-06-02）：第 25 节整改后 + BUG-005 蓝牙释放新问题

### 10.1 本次追加审查范围

本节用于记录 2026-06-02 第 25 节整改后的追加复审结论，重点覆盖 `BUG.md` 第 38 行起人工验证记录的新现象：

1. 显式选择蓝牙耳机麦克风（日志中 `microphone_device: Some("drizzle")`）后，停止录制时日志显示 `CpalMicrophoneCapture::stop() 完成 — stream_dropped=true`，但蓝牙输出音质没有恢复。
2. 选择系统默认麦克风（日志中 `microphone_device: None`，人工观察默认设备疑似仍为蓝牙耳机麦克风）后，停止录制时同样显示 `stream_dropped=true`，但蓝牙输出音质可以恢复。
3. 这两组最新日志没有复现旧的 `视频 10s / 音频 53s` duration inflation；录制和导出可以完成，source artifact 的 aggregate audio contract 也通过。

本次对照的关键代码范围：

- `src-tauri/src/platform/macos/cpal_microphone.rs`
- `src-tauri/src/platform/macos_service.rs`
- `src-tauri/src/media/audio_synchronizer.rs`
- `src-tauri/src/media/recording_writer.rs`
- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/lib.rs`
- `tests/phase-6-w11-w12-checklist.md`

当前 HEAD 为 `6957862`，工作区中 `BUG.md` 有人工验证记录的未提交变更。本文只记录审查结论和后续编码方案，不修改生产代码。

### 10.2 更新后的总评结论

Phase 6 仍不建议合并，BUG-005 仍不能关闭。

但需要把 BUG-005 的状态拆成两个层次看：

1. **旧问题已明显收敛**：最新两组真实设备日志均未复现 `音频 53s` 时长膨胀；writer 也没有出现 full-overlap discard，说明第 25 节整改对 synchronizer/window splitting/writer timeline 的主链路已有实际改善。
2. **新 blocker 是蓝牙输入设备释放**：显式选择蓝牙麦克风时，录制停止后 macOS 仍停留在低音质 HFP/SCO 体验，说明“录制结束释放输入设备并恢复蓝牙输出 profile”没有闭环。
3. **诊断与 contract 仍不足以签 off**：`system_rms_max_before_writer` 和 `mic_rms_max_before_writer` 在两组最新日志中都仍为 `0.0`，证明 source-aware diagnostics 没有在 consumer live/final drain 中真正写入；当前 source-aware contract 因此无法证明每个请求源在 writer 前都存在。

结论：

- Ready to merge：**No**
- BUG-005：**未关闭**
- 下一轮编码优先级：先修复诊断/contract 盲区，再独立修复蓝牙麦克风 release lifecycle。

### 10.3 最新 BUG.md 日志解读

两组复现日志对比：

| 项目                           |        情况 1：显式蓝牙麦克风 |        情况 2：系统默认麦克风 |
| ------------------------------ | ----------------------------: | ----------------------------: |
| `microphone_device`            |             `Some("drizzle")` |                        `None` |
| `system_chunks_received`       |                           640 |                           674 |
| `mic_chunks_received`          |                           618 |                           655 |
| `paired_window_count`          |                           619 |                           656 |
| `system_only_window_count`     |                            22 |                            19 |
| `mic_only_window_count`        |                             0 |                             0 |
| `source_timeout_window_count`  |                             0 |                             0 |
| `system_rms_max`               |                   0.028443791 |                    0.02666031 |
| `mic_rms_max`                  |                    0.12326609 |                    0.29693782 |
| `mixed_rms_max`                |                   0.059752557 |                    0.16463715 |
| `system_rms_max_before_writer` |                           0.0 |                           0.0 |
| `mic_rms_max_before_writer`    |                           0.0 |                           0.0 |
| contract                       | `RMS=0.006896, peak=0.163129` | `RMS=0.010561, peak=0.434141` |
| writer full-overlap discard    |                             0 |                             0 |
| `stream_dropped`               |                          true |                          true |
| 人工观察                       |        停止后蓝牙音质没有恢复 |            停止后蓝牙音质恢复 |

这些数字说明：

1. 本轮日志不再支持“writer overlap 丢掉麦克风导致无声”的旧主因。
   - 两组都是 `audio_chunks_received == audio_chunks_appended`。
   - 两组都是 `audio_chunks_discarded_full_overlap=0`。
   - paired windows 占绝大多数，`mic_only_window_count=0`。
2. source/export artifact 不是全静音。
   - contract 通过，decoded RMS/peak 非零。
   - writer `audio_real_rms_max_before_encode` 分别为 `0.06016173` 和 `0.16452658`。
3. 人工观察的新问题不在 FFmpeg writer 主链路，而在 macOS/CPAL 麦克风设备生命周期边界。
   - 两组 stop 日志完全同形：都只有 start/end 和 `stream_dropped=true`。
   - 但显式设备路径不恢复，默认设备路径恢复。
   - 因此 `stream_dropped=true` 只能说明 Rust 侧 `Option<cpal::Stream>` 被 take/drop，不能说明 CoreAudio input unit 已 stop/uninitialize 成功，也不能说明蓝牙 HFP profile 已切回 A2DP。
4. `system_rms_max_before_writer=0.0` 和 `mic_rms_max_before_writer=0.0` 是一个独立诊断缺陷。
   - `AudioSynchronizer` 已经在 `SynchronizedAudioChunk` 中携带 `system_rms` / `mic_rms` / `system_frames` / `mic_frames`。
   - 但 `macos_service.rs` 的 live drain 和 final drain 只把 `synced.mixed` 推给 writer，没有把 per-source fields 写回 `RecordingDiagnostics`。
   - 这会让后续 source-aware contract 的判断缺少最关键证据。

### 10.4 Findings

#### Critical 1：蓝牙麦克风 stop 只证明 Rust stream 被 drop，不能证明 CoreAudio/HFP 已释放

位置：

- `src-tauri/src/platform/macos/cpal_microphone.rs`
- `CpalMicrophoneCapture::stop()`

当前 stop 行为：

1. `running=false`
2. `self.stream.take()`
3. `drop(stream)`
4. `sleep(200ms)`
5. 日志输出 `stream_dropped=true`

问题：

1. 没有显式调用 `StreamTrait::pause()`。
2. 没有记录 `pause()` 是否成功或失败。
3. 没有记录 stop 后是否仍有 callback 进入。
4. 没有区分“显式蓝牙设备”与“默认设备”实际解析到的底层 endpoint。
5. 没有验证 macOS 蓝牙 profile 是否从 HFP/SCO 恢复到高音质输出。

本地依赖源码证据：

- `cpal 0.15.3` 的 `StreamTrait` 提供 `pause(&self) -> Result<(), PauseStreamError>`。
- macOS backend 的 `pause()` 会调用 CoreAudio `audio_unit.stop()`，并能把 stop error 返回给调用方。
- `coreaudio-rs 0.11.3` 的 `AudioUnit::drop()` 会调用 `self.stop().ok()` 和 `AudioUnitUninitialize(...).ok()`，即 drop 路径会忽略 stop/uninitialize 错误。

因此，当前 `stream_dropped=true` 不是 release success 证据。它只能证明对象生命周期走到了 Rust drop，不能证明底层 AudioUnit 停止成功。

影响：

- 显式蓝牙麦克风可能继续占用输入链路。
- macOS 可能继续让蓝牙耳机停在 HFP/SCO 低音质模式。
- 用户停止录制后系统音频仍断续/变差，体验上等同“麦克风进程未释放”。

建议修复：

1. 在 `stop()` 中对 stream 显式 `pause()`，记录 `pause_ok` / `pause_error`。
2. `pause()` 后再 drop stream；drop 仍保留，但不再作为唯一证据。
3. 增加 callback-after-stop 计数：callback 发现 `running=false` 时递增计数，stop 结束时输出。
4. 增加 bounded wait，并等待 callback 计数稳定，而不是固定 sleep 后直接认为释放完成。
5. `MacRecordingService` 应记录本轮是否真的启动过 mic；未启动 mic 时不要无条件调用 `mic_capture.stop()` 并 sleep。
6. stop 完成后重置 `CpalMicrophoneCapture` 实例，避免显式设备路径的 stream/device 相关状态残留到下一轮。

#### Critical 2：source-aware diagnostics 没有写入 writer 前 per-source RMS/frames

位置：

- `src-tauri/src/platform/macos_service.rs`
- live drain：`for synced_result in synchronizer.drain_mixed()`
- final drain：`for (synchronized, was_unpaired) in drain_results`
- `src-tauri/src/media/recording_writer.rs`

现象：

最新两组日志中：

```text
system_rms_max_before_writer: 0.0
mic_rms_max_before_writer: 0.0
```

但 capture-side 明明有非零 RMS：

```text
system_rms_max: 0.028443791 / 0.02666031
mic_rms_max: 0.12326609 / 0.29693782
```

问题：

1. `SynchronizedAudioChunk` 已携带 per-source RMS/frames。
2. consumer 只统计了 mixed RMS，并把 `synced.mixed` 推给 writer。
3. `RecordingDiagnostics.system_rms_max_before_writer` / `mic_rms_max_before_writer` 从未在 live/final drain 被更新。
4. 因此当前日志无法回答“每个请求源是否在 writer 前仍存在”。

影响：

- source-aware contract 缺证据。
- 人工验证时很容易再次把 capture-side UI 电平误当作 artifact/source success。
- 对 BUG.md 预防规则“请求录制音频源必须和实际写入非静音音频内容建立可验证 contract”仍未完全满足。

建议修复：

在 live drain 和 final drain 中，在 `writer.push_audio(...)` 前写入：

```text
if synced.has_system:
  system_windows_before_writer += 1
  system_frames_before_writer += synced.system_frames
  system_rms_max_before_writer = max(..., synced.system_rms)

if synced.has_mic:
  mic_windows_before_writer += 1
  mic_frames_before_writer += synced.mic_frames
  mic_rms_max_before_writer = max(..., synced.mic_rms)

if synced.emitted_due_to_timeout:
  source_timeout_window_count += 1
```

建议把 `RecordingDiagnostics` 补充为：

```rust
pub system_windows_before_writer: u64,
pub mic_windows_before_writer: u64,
pub system_frames_before_writer: u64,
pub mic_frames_before_writer: u64,
pub system_rms_max_before_writer: f32,
pub mic_rms_max_before_writer: f32,
```

#### Critical 3：`audible_min_rms` 已定义但未参与 validation

位置：

- `src-tauri/src/media/ffmpeg_common.rs`
- `RequestedAudioContract`
- `validate_source_artifact_with_audio_contract()`
- `validate_export_artifact_with_audio_contract()`

当前状态：

```rust
pub audible_min_rms: f64, // default 0.015
```

但 source/export validation 实际只检查：

```rust
if rms < contract.min_rms && peak < contract.min_peak {
    return Err(...);
}
```

问题：

1. `audible_min_rms` 没有被读取。
2. 最新显式蓝牙日志 `RMS=0.006896`，低于 `audible_min_rms=0.015`，但 contract 仍通过。
3. 当前阈值只能证明“非全静音”，不能证明“用户可听”。

建议修复：

1. 明确 `audible_min_rms` 的语义：
   - 若它是 manual gate 阈值，应在日志中输出 warning。
   - 若它是产品 contract，应在请求音频时 fail。
2. 对 BUG-005 建议先按 fail 处理：请求了 system 或 mic 且 aggregate RMS 低于 `audible_min_rms` 时，返回明确错误。
3. 增加测试覆盖“RMS 低于 audible 阈值但 peak 通过”的情况，防止 peak 偶发尖峰掩盖整体不可听。
4. 若担心误杀真实安静环境，可结合 capture-side RMS 做相对判断：capture RMS 明显非零时，artifact RMS 低于 capture RMS 的某个比例则 fail。

#### Important 1：AudioSynchronizer 缺少 source start grace，双源启动边界仍可能输出早到单源窗口

位置：

- `src-tauri/src/media/audio_synchronizer.rs`
- `calculate_watermark()`

第 25 节整改已经把“双源都 seen 后”的 watermark 改成 `min(system_ts, mic_ts)`，并按 sample frame 切分长 chunk，这解决了旧日志中的大规模 overlap discard。

但当前仍有启动边界：

1. 当 system/mic 都 requested，但只有一路已经 seen 时，`calculate_watermark()` 会 fallback 到 `max(...) - hold_nanos`。
2. 如果蓝牙 mic 首个 callback 慢于 hold window，早到的 system windows 仍可能先被输出为 system-only。
3. 后续 mic 首帧到来后，已经 emit 的 window 无法再合并。

最新日志里 system-only window 还有 22 / 19 个，数量不大，但这说明启动边界仍存在。

建议修复：

1. `AudioSynchronizerConfig` 增加 `source_start_grace_nanos`。
2. 双源 requested 且只 seen 一路时，在 grace 内不 live emit 单源窗口。
3. grace 超时后允许输出，但必须标记 `emitted_due_to_timeout=true`。
4. `calculate_watermark()` 不应只返回 watermark，应返回 `(watermark, timeout_mode)`，让 `drain_mixed()` 能把 timeout 原因传给 `emit_window()`。

#### Important 2：`emitted_due_to_timeout` / `source_timeout_window_count` 目前诊断链路不完整

位置：

- `src-tauri/src/media/audio_synchronizer.rs`

当前 `emit_window(window, emitted_due_to_timeout)` 支持 timeout 标记，但 `drain_mixed()` 调用时固定传 `false`。

影响：

- 即使 `calculate_watermark()` 因 source stall fallback 到 max watermark，输出 window 也不会被标记为 timeout。
- `source_timeout_window_count` 可能长期为 0，不能用于判断“单源输出是正常停止排空、启动 grace 超时，还是 source stall”。

建议修复：

1. 让 watermark 计算返回 timeout/stall reason。
2. live drain 输出因 timeout/stall 发射的 window 时，设置 `emitted_due_to_timeout=true`。
3. final drain 的 unpaired warning 保持独立，不要与 live source timeout 混淆。

#### Important 3：`FfmpegRecordingWriter::finish()` 发送 Flush 有界，但 `join_worker()` 仍可能无界等待

位置：

- `src-tauri/src/media/ffmpeg_writer.rs`
- `finish()`
- `join_worker()`

第 24 节整改把 `send(Flush)` 改成 `try_send(Flush)` + bounded retry，这是进步。但 `finish()` 随后仍调用：

```rust
let mut result = self.join_worker()?;
```

`join_worker()` 内部直接 `handle.join()`。如果 FFmpeg worker 卡在 encoder/muxer flush、IO 或异常路径上，stop 线程仍会阻塞到 worker 返回。

建议修复：

1. worker 通过 result channel 回传 `RecordingResult`，主线程用 bounded receive 等待。
2. 如果超时，记录 worker stuck diagnostic，并返回结构化错误。
3. 如无法强杀 Rust thread，至少要保证 UI stop 路径不会无限等待，并清理可清理的 output file。

#### Important 4：麦克风设备选择以设备名作为 ID，显式蓝牙路径不稳定

位置：

- `src-tauri/src/lib.rs`
- `list_microphone_devices()`
- `set_audio_config()`
- `src-tauri/src/platform/macos/cpal_microphone.rs`

当前：

1. `list_microphone_devices()` 返回 `name`。
2. 前端把 `name` 作为 `microphone_device`。
3. 后端用 `host.input_devices().find(|d| d.name() == device_name)` 找第一个同名设备。

风险：

- 蓝牙设备可能有多个 endpoint 或重复/变化的名称。
- 显式选择 `"drizzle"` 和默认设备 `None` 可能解析到不同的底层 input endpoint。
- 当前日志没有记录“显式选择名称最终解析到哪个 host/device/default 状态”，不利于定位为什么显式路径不释放而默认路径释放。

建议修复：

1. 如果 cpal/macOS 能提供更稳定的 device identifier，应使用 stable id。
2. 如果只能拿到 name，至少返回 `index + name + is_default + duplicate_count`，并在 start log 中记录选中的 index/name/default 状态。
3. UI 选择值不要只存裸 name；重复名称时应显示区分信息。

#### Important 5：Phase 6 自测清单与 HANDOFF 状态不一致

位置：

- `tests/phase-6-w11-w12-checklist.md`
- `HANDOFF.md`

现状：

- `HANDOFF.md` 已记录第 25 节整改后 `cargo test --features ffmpeg`、`cargo test`、`npm test`、`cargo fmt --check`、`cargo clippy` 通过。
- `tests/phase-6-w11-w12-checklist.md` 仍停留在 2026-05-30 状态，写着 FFmpeg dev libraries blocked、Manual FFmpeg export blocked、Native Safety Gate blocked。

影响：

- 后续 reviewer 会误判 Phase 6 验收状态。
- 不利于人工 gate 继续记录 source/export artifact 证据。

建议修复：

在下一轮编码完成后同步更新 checklist，至少增加：

- 第 25/26 节 FFmpeg feature test 结果。
- BUG-005 source/export RMS/peak 证据。
- 显式蓝牙麦克风 release manual gate。
- 默认蓝牙麦克风 release manual gate。
- 内置麦克风无回归 gate。

#### Minor 1：导出 contract 读取 service lock 使用 `unwrap()`

位置：

- `src-tauri/src/lib.rs`
- `export_video()`

当前：

```rust
let svc = state.service.lock().unwrap();
```

建议改为：

```rust
let svc = state
    .service
    .lock()
    .map_err(|_| "录制服务锁已损坏".to_string())?;
```

这不是 BUG-005 主因，但属于错误处理一致性问题。

#### Minor 2：未启动麦克风时也会调用 `mic_capture.stop()` 并 sleep 200ms

位置：

- `src-tauri/src/platform/macos_service.rs`
- `MacRecordingService::stop()`
- `CpalMicrophoneCapture::stop()`

当前 stop 总是调用 mic stop。若本轮没有启动 microphone stream，`stream_dropped=false`，但仍 sleep 200ms。

建议：

- 增加 `mic_capture_started_this_session` 或复用 `last_requested_microphone`。
- 只有本轮确实启动了 mic stream 时才执行 pause/drop/wait。
- 未启动时仅清理状态，不做蓝牙 release wait。

### 10.5 BUG-005 新 bug 的根因定位

本次新增 bug 的 failure boundary 已经可以定位到：

> 不是 FFmpeg writer duration，不是 artifact aggregate audio validation，也不是旧的 massive full-overlap discard；而是显式蓝牙麦克风输入设备的 stop/release lifecycle 没有可验证闭环。

最强根因假设：

> 显式选择蓝牙麦克风时，CPAL/CoreAudio input stream 没有被显式 pause/stop 并确认成功；当前代码仅 drop stream，且 CoreAudio drop 路径会忽略 stop/uninitialize 错误。因此 macOS 可能继续认为蓝牙输入链路处于活动状态，导致耳机停留在 HFP/SCO 低音质 profile。默认设备路径之所以恢复，可能是 default input endpoint 与显式 name 匹配到的 endpoint 不同，或系统对 default stream 的 profile release 行为不同。

证据：

1. 两组最新日志都没有旧的 53s drift。
2. 两组 writer 都没有 full-overlap discard。
3. 两组 artifact 都通过 aggregate audio contract。
4. 两组 stop 日志都只有 `stream_dropped=true`，但人工结果相反。
5. `stream_dropped=true` 不是 CoreAudio stop success 证据。
6. CPAL macOS `pause()` 可返回 `audio_unit.stop()` 的错误，但当前没有调用。
7. `AudioUnit::drop()` 忽略 stop/uninitialize 错误，因此 drop 失败不会出现在日志或错误路径中。

仍需用 instrumentation 证明的点：

1. 显式蓝牙路径 stop 时 `pause()` 是否成功。
2. drop 后是否仍有 callback 进入。
3. 显式 `Some("drizzle")` 和默认 `None` 是否解析到同一个底层 device/endpoint。
4. 停止后 macOS 蓝牙 profile 是否在 0.5s/1s/2s 内恢复。

### 10.6 推荐修复方案（编码 Phase）

#### Phase A：补齐 source-aware diagnostics 和 contract

目标：

- 先让日志能证明每个请求源是否进入 writer 前。
- 防止 `system_rms_max_before_writer=0.0` / `mic_rms_max_before_writer=0.0` 这类诊断空洞继续通过。

实施：

1. 在 `RecordingDiagnostics` 增加 per-source writer-before fields：
   - `system_windows_before_writer`
   - `mic_windows_before_writer`
   - `system_frames_before_writer`
   - `mic_frames_before_writer`
   - 保留并实际写入 `system_rms_max_before_writer`
   - 保留并实际写入 `mic_rms_max_before_writer`
2. 在 live drain 和 final drain 推 writer 前更新这些字段。
3. 更新 `validate_source_aware_audio_contract()`：
   - 请求 system 且 capture RMS 非零时，要求 system before-writer windows/frames/RMS 非零。
   - 请求 mic 且 capture RMS 非零时，要求 mic before-writer windows/frames/RMS 非零。
   - 如果 requested source capture RMS 明显非零，但 before-writer RMS 为 0，直接 fail。
4. 增加失败测试：
   - `validate_source_aware_audio_contract_rejects_missing_system_before_writer`
   - `validate_source_aware_audio_contract_rejects_missing_mic_before_writer`
   - `consume_frames_records_source_rms_before_writer_in_live_drain`
   - `consume_frames_records_source_rms_before_writer_in_final_drain`

验收：

- 最新真实设备日志中 `system_rms_max_before_writer` / `mic_rms_max_before_writer` 必须非零。
- contract 不能在 source-aware fields 全 0 时通过。

#### Phase B：启用 audible contract

目标：

- 区分“非全静音”和“可听”。

实施：

1. 在 source/export validation 中读取 `audible_min_rms`。
2. 对请求音频的 source artifact：
   - `rms < min_rms && peak < min_peak`：fail，表示近乎静音。
   - `rms < audible_min_rms`：建议先 fail，直到真实设备样本证明阈值需要降低。
3. 对 export artifact 使用同样规则。
4. 增加测试：
   - `requested_audio_contract_rejects_low_rms_even_when_peak_passes`
   - `export_audio_contract_rejects_low_rms_even_when_peak_passes`
   - `requested_audio_contract_allows_no_audio_when_no_source_requested`

验收：

- `RMS=0.006896` 这类低于 `audible_min_rms=0.015` 的 artifact 不应静默通过。
- 若后续人工确认蓝牙真实音量天然偏低，再把阈值从 fail 调整为 warning 必须有样本依据。

#### Phase C：蓝牙麦克风 release lifecycle hardening

目标：

- 让显式蓝牙麦克风停止后可验证地释放输入设备，并恢复输出音质。

实施：

1. `SendStream` 增加明确方法或在 `stop()` 中访问内部 stream：
   - `pause()`
   - `drop()`
2. `CpalMicrophoneCapture` 增加 diagnostics：
   - `stop_requested_at`
   - `pause_attempted`
   - `pause_ok`
   - `pause_error`
   - `stream_dropped`
   - `callbacks_after_stop`
   - `waited_ms`
3. callback 中如果 `running=false`，递增 `callbacks_after_stop` 后返回。
4. stop 顺序建议：
   - `running=false`
   - `stream.take()`
   - 对 stream 调用 `pause()` 并记录结果
   - drop stream
   - bounded wait 直到 callback-after-stop 稳定，最长 500ms-1000ms
   - 输出完整 stop diagnostics
5. `MacRecordingService`：
   - 记录本轮是否启动过 microphone。
   - 只在启动过 mic 时执行 mic stop。
   - stop 后重建 `CpalMicrophoneCapture::new()`，下一轮 start 再 set session clock。
6. 对显式蓝牙设备保留 UI warning，但 warning 不能替代 release gate。

验收：

- 显式选择蓝牙麦克风录制 10s，停止后 2s 内输出音质恢复。
- 默认麦克风指向蓝牙时录制 10s，停止后 2s 内输出音质恢复。
- 内置麦克风录制 10s，不引入额外 stop error。
- 日志必须包含 pause/drop/callback-after-stop 证据。

#### Phase D：AudioSynchronizer 启动 grace 和 timeout 标记

目标：

- 修掉双源启动阶段早到源可能单独 emit 的边界。
- 让 `source_timeout_window_count` 真正有诊断意义。

实施：

1. `AudioSynchronizerConfig` 增加 `source_start_grace_nanos`。
2. 双源 requested 且只 seen 一路时，在 grace 内不 live emit。
3. grace 超时后允许 emit，但设置 `emitted_due_to_timeout=true`。
4. `calculate_watermark()` 返回 timeout/stall mode。
5. `drain_mixed()` 根据 mode 传入 `emit_window(..., true/false)`。

新增测试：

- `audio_synchronizer_dual_source_waits_for_initial_slow_source_within_grace`
- `audio_synchronizer_dual_source_emits_after_start_grace_timeout`
- `audio_synchronizer_marks_timeout_windows_when_source_stalls`

#### Phase E：writer finish bounded join

目标：

- 避免 `finish()` 已经有界发送 Flush 后仍在 `join_worker()` 无限阻塞。

实施：

1. worker 新增 result channel。
2. main thread 使用 bounded wait 等待 result。
3. 超时返回 `RecordingWriteFailed`，记录 worker stuck。
4. 失败时清理当前 output file。

新增测试：

- 用 fake/stub worker 或可注入 writer 测试 flush timeout 和 join timeout。
- 保留现有 FFmpeg writer audio timeline tests。

#### Phase F：设备标识、日志和 checklist 收口

目标：

- 降低显式 device path 与 default path 行为差异的排查成本。
- 让验收文档反映真实状态。

实施：

1. `list_microphone_devices()` 返回可区分重复名称的信息。
2. start 日志记录：
   - requested microphone device
   - resolved device name
   - is default
   - actual sample rate/channels
   - Bluetooth heuristic result
3. 更新 `tests/phase-6-w11-w12-checklist.md`，补上第 25/26 节验证记录和蓝牙 release gates。

### 10.7 建议测试与 manual gate

优先新增/修正的自动化测试：

1. `validate_source_aware_audio_contract_rejects_missing_system_before_writer`
2. `validate_source_aware_audio_contract_rejects_missing_mic_before_writer`
3. `consume_frames_records_source_rms_before_writer_in_live_drain`
4. `consume_frames_records_source_rms_before_writer_in_final_drain`
5. `requested_audio_contract_rejects_low_rms_even_when_peak_passes`
6. `export_audio_contract_rejects_low_rms_even_when_peak_passes`
7. `audio_synchronizer_dual_source_waits_for_initial_slow_source_within_grace`
8. `audio_synchronizer_marks_timeout_windows_when_source_stalls`
9. `cpal_microphone_stop_pauses_before_drop`（需要先抽象可测试 stream trait，避免直接 mock `cpal::Stream`）

聚焦命令建议：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg validate_source_aware_audio_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture
```

完整回归建议：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

真实设备 manual gate：

1. 只录系统音频 10 秒，播放音乐，source/export 都可听。
2. 只录内置麦克风 10 秒，说话，source/export 都可听。
3. 系统音频 + 内置麦克风 10 秒，两路都可听。
4. 系统音频 + 显式蓝牙麦克风 10 秒：
   - 录制中允许音质因 HFP 变差，但 UI 需要有 warning。
   - 停止后 2 秒内蓝牙输出音质必须恢复。
   - 日志必须有 `pause_ok`、`stream_dropped`、`callbacks_after_stop`。
   - source/export audio RMS 不低于 audible contract。
5. 系统音频 + 默认麦克风（默认设备为蓝牙）10 秒：
   - 停止后 2 秒内蓝牙输出音质必须恢复。
   - 与显式设备路径日志对比 resolved device。
6. 每次 manual gate 都记录：
   - `RecordingDiagnostics`
   - `WriterDiagnostics`
   - source artifact RMS/peak/sample_count
   - export artifact RMS/peak/sample_count
   - paired/system-only/mic-only/timeout window count
   - Bluetooth output quality recovery observation

### 10.8 本次追加复审的验证记录

本次为文档追加复审，没有修改生产代码。

已做的只读核查：

1. 读取 `HANDOFF.md`，确认第 25 节整改范围和当前 Phase 6 状态。
2. 读取 `BUG.md`，确认两组蓝牙新 bug 复现日志。
3. 读取 `cpal_microphone.rs`，确认 stop 当前只 `take/drop/sleep`，没有显式 `pause()`。
4. 读取 `macos_service.rs`，确认 live/final drain 未写入 `system_rms_max_before_writer` / `mic_rms_max_before_writer`。
5. 读取 `recording_writer.rs`，确认 source-aware contract 仍主要依赖 window counts 和 aggregate full-overlap discard ratio。
6. 读取 `ffmpeg_common.rs`，确认 `audible_min_rms` 已定义但 source/export validation 未使用。
7. 读取 `ffmpeg_writer.rs`，确认 Flush send 有 bounded retry，但 `join_worker()` 仍直接 `join()`。
8. 读取本地 cargo registry 中 `cpal 0.15.3` / `coreaudio-rs 0.11.3` 相关源码，确认 `pause()` 可返回 CoreAudio stop error，而 `AudioUnit::drop()` 会忽略 stop/uninitialize 错误。
9. 读取 `tests/phase-6-w11-w12-checklist.md`，确认 checklist 仍停留在旧的 FFmpeg blocked 状态。

此前聚焦测试记录：

```text
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer -- --nocapture
结果：21 passed

cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_sample_clock -- --nocapture
结果：2 passed

cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg validate_source_aware_audio_contract -- --nocapture
结果：matched 0 tests
```

注意：

- `validate_source_aware_audio_contract` 当前 matched 0 tests 是测试缺口，下一轮编码必须补。
- 本次没有运行完整 cargo/npm 回归，因为本次用户请求是把审查结论、定位和修复方案记录到 review 文档中。
- 蓝牙 release 问题必须通过真实设备 manual gate 关闭，单元测试只能验证 stop 调用顺序、诊断字段和错误路径。

---

### 10.9 追加复审（2026-06-02）：Section 10 整改后 BUG-005_2 录制音频失败

本节记录 Section 10 整改提交后，人工真实设备验证新暴露的 BUG-005_2。

输入依据：

- `BUG.md` 的 `### BUG-005_2: 录制音频失败`
- Section 10 整改实现范围：`6957862e79c9b8f4efb7b2df5a7151b114dc759d..9893c51daf9838c69bfea9916709ee9f2f453c97`
- 本轮只读审查过的代码：
  - `src-tauri/src/media/ffmpeg_common.rs`
  - `src-tauri/src/media/recording_writer.rs`
  - `src-tauri/src/media/audio_synchronizer.rs`
  - `src-tauri/src/media/ffmpeg_writer.rs`
  - `src-tauri/src/platform/macos_service.rs`
  - `src-tauri/src/platform/macos/cpal_microphone.rs`
  - `src-tauri/src/lib.rs`

本轮结论：

- Phase 6 仍不建议合并。
- BUG-005 不能关闭。
- BUG-005_2 的直接根因不是 capture 未收到音频，也不是 writer 未写入音频，而是 Section 10 把 `audible_min_rms=0.015` 从“可听性诊断阈值”升级成了 source/export artifact 的硬失败阈值。
- 真实录制 artifact 的 aggregate decoded RMS 为 `0.010835`，已经高于非静音阈值 `min_rms=0.003`，并且 capture/writer 诊断均证明音频真实进入了 writer；但它低于新加的 `audible_min_rms=0.015`，因此 stop 阶段被误判为录制失败。

#### 10.9.1 BUG-005_2 日志证据链

BUG-005_2 的终端日志给出了足够清晰的边界证据：

```text
RecordingDiagnostics {
  requested_system_audio: true,
  requested_microphone: true,
  system_chunks_received: 646,
  mic_chunks_received: 617,
  system_chunks_dropped: 0,
  mic_chunks_dropped: 0,
  mixed_chunks_queued: 647,
  writer_push_audio_failures: 0,
  system_rms_max: 0.022819687,
  mic_rms_max: 0.27012125,
  mixed_rms_max: 0.1314065,
  generated_silent_track: false,
  paired_window_count: 618,
  system_only_window_count: 29,
  mic_only_window_count: 0,
  source_timeout_window_count: 0,
  system_rms_max_before_writer: 0.023172297,
  mic_rms_max_before_writer: 0.267092,
  system_windows_before_writer: 647,
  mic_windows_before_writer: 618,
  system_frames_before_writer: 620160,
  mic_frames_before_writer: 296160
}
```

这些字段说明：

1. **capture 侧正常**：
   - `system_chunks_received=646`
   - `mic_chunks_received=617`
   - `system_rms_max=0.022819687`
   - `mic_rms_max=0.27012125`
2. **channel 没有 drop**：
   - `system_chunks_dropped=0`
   - `mic_chunks_dropped=0`
3. **synchronizer 输出正常**：
   - `paired_window_count=618`
   - `system_only_window_count=29`
   - `mic_only_window_count=0`
4. **writer 前 source-aware 诊断已经补齐且非零**：
   - `system_rms_max_before_writer=0.023172297`
   - `mic_rms_max_before_writer=0.267092`
   - `system_windows_before_writer=647`
   - `mic_windows_before_writer=618`
5. **writer 侧没有丢弃主链路音频**：
   - `audio_chunks_received=647`
   - `audio_chunks_appended=647`
   - `audio_chunks_discarded_full_overlap=0`
   - `audio_chunks_trimmed_partial_overlap=0`
   - `audio_real_frames_appended=620160`
   - `audio_real_rms_max_before_encode=0.13140391`
   - `aac_frames_encoded=615`
   - `generated_silent_track=false`

真正的失败点只有这一行：

```text
录制音频 contract 验证失败: 写入录制文件失败：请求了音频录制但 RMS 低于可听阈值（RMS=0.010835 < 0.015000，system=true, mic=true）
```

因此 BUG-005_2 的 failure boundary 是 artifact validation，不是 capture、synchronizer、writer、CPAL stop 或蓝牙 profile release。

#### 10.9.2 系统化定位结论

按照 systematic debugging 的分层排查，本次边界可以这样收敛：

1. **不是无音频输入**：
   - system/mic chunks received 均为数百个。
   - capture-side RMS 均非零。
2. **不是队列丢包**：
   - system/mic dropped 均为 0。
   - writer push failures 为 0。
3. **不是旧 BUG-005/BUG-009 的 overlap 丢 PCM**：
   - `audio_chunks_discarded_full_overlap=0`
   - `audio_chunks_trimmed_partial_overlap=0`
   - `audio_chunks_appended=audio_chunks_received`
4. **不是 source-aware before-writer 诊断缺失**：
   - Section 10 之后 before-writer fields 已经写入，并且都非零。
5. **不是 silent AAC track**：
   - `generated_silent_track=false`
   - `aac_frames_encoded=615`
   - decoded artifact RMS 为 `0.010835`，不是 0。
6. **是 audible threshold 语义错误**：
   - `RequestedAudioContract::audible_min_rms` 默认值为 `0.015`。
   - `validate_source_artifact_with_audio_contract()` 在 `rms < audible_min_rms` 时直接返回 `RecordingWriteFailed`。
   - `MacRecordingService::consume_frames()` 将该错误加入 `errors`。
   - `MacRecordingService::stop()` 最终把 `errors` 转成 `RecordingFinalizeFailed`，导致用户点击结束录制时报错。

根因一句话：

> Section 10 把一个尚未经过真实设备标定的 aggregate loudness 阈值当成录制成败 contract，导致“真实有声但整体音量偏低”的 artifact 被误判为失败。

#### 10.9.3 Code Review Findings

##### Critical 1：`audible_min_rms` 被作为硬失败阈值，直接制造 BUG-005_2

位置：

- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/platform/macos_service.rs`
- `src-tauri/src/lib.rs`

问题：

1. `RequestedAudioContract` 注释里描述 `audible_min_rms` 是 real-device validation / warning 语义，但实现中 source/export validation 都直接 `Err`。
2. `audible_min_rms=0.015` 没有真实设备标定依据。
3. 真实设备日志已经证明 `RMS=0.010835` 的录制并非 silent，也不是 writer 丢音频，但仍被硬失败。
4. 该 hard gate 同时影响录制停止路径和导出路径：
   - 录制：`validate_source_artifact_with_audio_contract()` -> `errors.push()` -> `RecordingFinalizeFailed`
   - 导出：`validate_export_artifact_with_audio_contract()` -> 删除 export output -> return error

影响：

- 只要开启音频采集，实际内容音量稍低就可能在停止录制时报错。
- 用户会误以为录制失败或音频链路坏掉。
- BUG-005 的 contract 目标从“防止 silent artifact”变成了“强制响度达标”，语义越界。

修复方向：

- `min_rms/min_peak` 保持硬失败，用于防止 silent / near-silent artifact。
- `audible_min_rms` 降级为 warning / diagnostics / manual gate，不参与 `AppError`。
- 后续若要恢复 hard gate，必须先通过真实设备样本标定，至少覆盖：
  - 只录系统音频，低音量播放音乐
  - 只录内置麦克风，小声说话
  - 系统音频 + 内置麦克风
  - 系统音频 + 蓝牙 HFP 麦克风

##### Important 1：source-aware contract 仍被 capture RMS 阈值门控，存在低音量请求源漏检

位置：

- `src-tauri/src/media/recording_writer.rs`

问题：

`validate_source_aware_audio_contract()` 当前只在如下条件成立时检查 source 是否到达 writer 前：

```rust
diagnostics.requested_system_audio && diagnostics.system_rms_max > 0.001
diagnostics.requested_microphone && diagnostics.mic_rms_max > 0.001
```

这会产生一个新盲区：

- 用户请求了某一路音频；
- capture 确实收到 chunks / frames；
- 但 RMS 低于 0.001；
- contract 会跳过该 source 的 before-writer presence 检查。

source presence contract 不应该只由 RMS 决定。低音量或短时静音不等于 source 没有被请求、没有被采集、没有进入 writer。

修复方向：

1. source presence 检查应优先基于请求状态和 chunk/window/frame 计数：
   - requested system 且 system chunks received > 0 时，要求 system before-writer windows/frames > 0。
   - requested mic 且 mic chunks received > 0 时，要求 mic before-writer windows/frames > 0。
2. RMS 只用于判断“内容是否接近静音”或“可听性 warning”，不能作为是否执行 presence contract 的前置条件。
3. capture chunks 为 0 仍应按当前逻辑 fail/warn，因为这是 capture source 缺失。

##### Important 2：writer discard 检查只覆盖 mic，不覆盖 system

位置：

- `src-tauri/src/media/recording_writer.rs`

问题：

当前 full-overlap discard ratio 检查只在 `requested_microphone` 分支内执行。系统音频如果出现类似大规模 discard，source-aware contract 不会对称拦截。

影响：

- BUG-005 历史问题既可能吞 mic，也可能吞 system。
- 只检查 mic 会让“只录系统音频但 writer 丢掉系统音频”的回归更容易漏过。

修复方向：

1. 将 writer discard / trim 风险检查对 requested system 和 requested mic 都执行。
2. 更理想的做法是让 writer diagnostics 记录 source-specific append/discard，但当前 writer 只收到 mixed chunk，无法直接区分 source；因此下一轮至少应结合 before-writer source diagnostics + aggregate writer discard ratio 做对称保护。

##### Important 3：planned tests 未完整落地，覆盖不到 BUG-005_2

位置：

- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/platform/macos_service.rs`
- `docs/superpowers/plans/2026-06-02-phase-6-bug-005-rectification.md`

本轮聚焦命令结果：

```text
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract -- --nocapture
结果：1 passed，只命中 requested_audio_contract_any_audio_requested

cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg validate_source_aware_audio_contract -- --nocapture
结果：3 passed

cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer_dual_source -- --nocapture
结果：2 passed
```

测试缺口：

1. 没有覆盖 `RMS≈0.010~0.011`、高于 `min_rms` 但低于 `audible_min_rms` 的 artifact。
2. 没有覆盖 `validate_export_artifact_with_audio_contract()` 的低 RMS 非静音通过/告警行为。
3. 计划中提到的 `consume_frames_records_source_rms_before_writer_in_live_drain` / `final_drain` 未落地。
4. 现有 synthetic helper 默认音频幅度较高，不会触发真实设备低 RMS 区间。

修复方向：

- 增加可指定音频 amplitude 的 strict artifact helper，或在测试内直接用 `FfmpegRecordingWriter` 写入低 RMS chunk。
- 添加低 RMS 非静音 regression tests。
- 添加 consume_frames before-writer diagnostics tests，防止未来再次出现 `system_rms_max_before_writer=0.0` 这类诊断空洞。

##### Important 4：writer join 仍不是严格 bounded join

位置：

- `src-tauri/src/media/ffmpeg_writer.rs`

问题：

Section 10 将 `tx` 改为 `Option<SyncSender>`，并在 `join_worker()` 前 `self.tx.take()`，这可以避免 worker 卡在 `rx.recv()` 时 join 无限等待。但当前 `join_worker()` 仍直接调用 `handle.join()`；如果 worker 卡在 FFmpeg encoder/muxer flush、IO 或异常资源释放路径，主线程仍会等待到 worker 返回。

影响：

- 这不是 BUG-005_2 的直接根因。
- 但它没有完全满足“writer bounded join”的初始目标。
- stop 路径仍可能在 FFmpeg worker 内部卡住时长时间阻塞。

修复方向：

1. worker 使用 result channel 回传 `RecordingResult`。
2. 主线程使用 bounded receive 等待 result。
3. 超时后返回结构化 `RecordingWriteFailed`，记录 worker stuck diagnostics。
4. 如无法杀死 Rust thread，至少保证 UI stop 路径不会无限等待，并清理可清理的 output file。

##### Important 5：蓝牙 release lifecycle 仍需要真实设备 gate，但不是 BUG-005_2 主因

位置：

- `src-tauri/src/platform/macos/cpal_microphone.rs`
- `src-tauri/src/platform/macos_service.rs`

当前进展：

- `CpalMicrophoneCapture::stop()` 已显式调用 `pause()`。
- stop 后 drop stream，并等待 300ms。
- `MacRecordingService::stop()` 只在本轮请求 mic 时调用 mic stop。
- stop 后重建 `CpalMicrophoneCapture::new()`。

仍缺少：

- `MicrophoneStopDiagnostics` 结构体。
- `callbacks_after_stop` 计数。
- 显式设备路径与默认设备路径的 resolved device 日志。
- stop 后 0.5s / 1s / 2s 的蓝牙 profile recovery 证据。

结论：

- BUG-005_2 的这次失败不是蓝牙 release 导致的，因为日志已显示 `pause()` 成功、audio writer 成功、最终错误来自 RMS contract。
- 但蓝牙 release 作为 BUG-005 的另一个 blocker 仍需要继续通过 manual gate 验证。

#### 10.9.4 推荐修复方案（下一轮编码 Phase）

##### Phase A：修复 `audible_min_rms` hard gate false positive

目标：

- 录制/导出 contract 继续阻止 silent artifact。
- 真实有声但整体 RMS 低于 `0.015` 的 artifact 不再导致 stop/export 失败。

建议实现：

1. 保留 hard fail：

```rust
if rms < contract.min_rms && peak < contract.min_peak {
    return Err(...);
}
```

2. 将 audible check 改为 warning：

```rust
if rms < contract.audible_min_rms {
    eprintln!(
        "警告: 请求了音频录制但 aggregate RMS 低于可听建议阈值（RMS={:.6} < {:.6}，system={}, mic={}）",
        rms,
        contract.audible_min_rms,
        contract.requested_system_audio,
        contract.requested_microphone,
    );
}
```

3. `validate_export_artifact_with_audio_contract()` 同步采用 warning，不删除 export output。
4. 如果需要结构化 warning，优先新增轻量 `AudioContractWarning` / `AudioValidationSummary`，但不要为了本轮修复引入大范围重构。

推荐优先级：

- 先做最小修复：hard fail -> warning。
- 再用真实设备样本决定是否需要更精细的 loudness contract。

##### Phase B：补 low-RMS regression tests

新增测试：

1. `requested_audio_contract_allows_low_but_non_silent_rms_with_warning`
   - 构造 decoded RMS 约 `0.010~0.011` 的 source artifact。
   - contract: `min_rms=0.003`, `min_peak=0.02`, `audible_min_rms=0.015`。
   - 期望：validation `Ok`，不返回 `RecordingWriteFailed`。
2. `export_audio_contract_allows_low_but_non_silent_rms_with_warning`
   - 同样覆盖 export validation。
   - 期望：不删除 output，不返回 `ExportFailed`。
3. `requested_audio_contract_still_rejects_near_silent_audio`
   - decoded `rms < min_rms` 且 `peak < min_peak`。
   - 期望：仍然 hard fail。

测试实现建议：

- 新增 test helper：`create_synthetic_source_artifact_with_audio_amplitude(path, width, height, duration_nanos, amplitude)`。
- 或直接在 test 内使用 `FfmpegRecordingWriter` 写入低幅度 `MixedAudioChunk`。
- 注意不要只测 `requested_audio_contract_any_audio_requested` 这种结构体方法；必须真实解码 artifact RMS。

##### Phase C：修正 source-aware contract 的 RMS 门控

目标：

- source presence contract 不因低 RMS 被跳过。
- 低音量内容不等于 source 缺失。

建议逻辑：

```text
if requested_system_audio:
  if system_chunks_received == 0:
    fail/warn: capture source missing
  if system_chunks_received > 0:
    require system_windows_before_writer > 0
    require system_frames_before_writer > 0

if requested_microphone:
  if mic_chunks_received == 0:
    fail/warn: capture source missing
  if mic_chunks_received > 0:
    require mic_windows_before_writer > 0
    require mic_frames_before_writer > 0
```

RMS 使用方式：

- `system_rms_max_before_writer == 0` / `mic_rms_max_before_writer == 0` 不应无条件 hard fail。
- 当 capture RMS 明显非零、before-writer RMS 为 0 时，可以 hard fail，因为这是“采集有声但 writer 前变静音”。
- 当 capture RMS 本身很低时，应记录 warning，交给 artifact near-silent contract 判断。

新增测试：

- `validate_source_aware_audio_contract_checks_requested_system_even_when_quiet`
- `validate_source_aware_audio_contract_checks_requested_mic_even_when_quiet`
- `validate_source_aware_audio_contract_allows_quiet_source_with_frames`
- `validate_source_aware_audio_contract_rejects_requested_source_chunks_with_zero_before_writer_frames`

##### Phase D：补 consume_frames before-writer diagnostics 测试

目标：

- 防止 Section 10 已修复的 before-writer diagnostics 再次回归。

新增测试：

- `consume_frames_records_source_rms_before_writer_in_live_drain`
- `consume_frames_records_source_rms_before_writer_in_final_drain`

验收点：

- `system_windows_before_writer > 0`
- `mic_windows_before_writer > 0`
- `system_frames_before_writer > 0`
- `mic_frames_before_writer > 0`
- `system_rms_max_before_writer > 0`
- `mic_rms_max_before_writer > 0`

##### Phase E：继续收口蓝牙 release lifecycle

目标：

- 解决 BUG-005 原始蓝牙 HFP profile release blocker。
- 避免把 BUG-005_2 的 validation false positive 和蓝牙 release 问题混在一起。

建议实现：

1. `CpalMicrophoneCapture` 增加 `MicrophoneStopDiagnostics`：
   - `stop_requested`
   - `stream_existed`
   - `pause_attempted`
   - `pause_ok`
   - `pause_error`
   - `stream_dropped`
   - `callbacks_after_stop`
   - `waited_ms`
2. callback 中当 `running=false` 时递增 `callbacks_after_stop`。
3. stop 后等待 callback-after-stop 稳定，最长 500ms-1000ms。
4. `list_microphone_devices()` / start log 增加 resolved device 信息：
   - requested device name
   - resolved device name
   - is default
   - duplicate index
   - actual sample rate/channels
   - Bluetooth heuristic
5. 真实设备 manual gate：
   - 显式选择蓝牙麦克风，停止后 2 秒内音质恢复。
   - 默认麦克风指向蓝牙，停止后 2 秒内音质恢复。
   - 内置麦克风无回归。

##### Phase F：真正 bounded writer join

目标：

- 让 stop 路径在 FFmpeg worker 卡住时有结构化超时结果。

建议实现：

1. worker 结果通过 channel 返回。
2. main thread bounded wait。
3. 超时记录：
   - output path
   - queued video/audio counts
   - writer diagnostics snapshot
   - flush_sent 状态
4. 返回 `RecordingWriteFailed`，不要无限等待 `handle.join()`。

#### 10.9.5 下一轮验证矩阵

聚焦自动化命令：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg export_audio_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg validate_source_aware_audio_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg consume_frames_records_source_rms -- --nocapture
```

完整回归：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

真实设备 manual gate：

1. 不采集音频，只录屏 15 秒，停止录制成功。
2. 只采集系统音频 15 秒，低音量播放音乐，停止录制成功；source/export 有音频流，非 silent。
3. 只采集麦克风 15 秒，小声说话，停止录制成功；source/export 有音频流，非 silent。
4. 同时采集系统音频 + 内置麦克风 15 秒，停止录制成功；source/export 有音频流，非 silent。
5. 同时采集系统音频 + 显式蓝牙麦克风 15 秒，停止录制成功；停止后 2 秒内蓝牙输出音质恢复。
6. 同时采集系统音频 + 默认麦克风（默认设备为蓝牙）15 秒，停止录制成功；停止后 2 秒内蓝牙输出音质恢复。

每次 manual gate 必须记录：

- `RecordingDiagnostics`
- `WriterDiagnostics`
- source artifact decoded RMS/peak/sample_count
- export artifact decoded RMS/peak/sample_count
- 是否触发 audible warning
- 蓝牙 stop diagnostics
- 人工听感：录制内容是否可听、停止后蓝牙音质是否恢复

#### 10.9.6 本轮只读验证记录

已执行：

```text
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract -- --nocapture
结果：1 passed，只命中 requested_audio_contract_any_audio_requested；暴露 low-RMS contract 测试缺口。

cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg validate_source_aware_audio_contract -- --nocapture
结果：3 passed。

cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_synchronizer_dual_source -- --nocapture
结果：2 passed。
```

测试输出仍有既有 warnings，包括：

- `unused_unsafe`
- `private_interfaces`
- `trimmed_frames is never read`
- ScreenCaptureKit FFI 命名 warnings

这些 warnings 不是 BUG-005_2 的直接根因，但下一轮若修改相关文件，应避免新增 warning。

#### 10.9.7 本轮审查结论摘要

一句话结论：

> BUG-005_2 是 Section 10 audible contract 的 false positive：音频真实进入 writer 并编码进 artifact，但 aggregate decoded RMS 低于未经标定的 `0.015`，被错误当作录制失败。

下一轮编码第一优先级：

1. 将 `audible_min_rms` hard fail 改为 warning。
2. 补 low-RMS 非静音 artifact regression tests。
3. 修正 source-aware contract 的 RMS 门控和 system/mic 对称性。
4. 再继续蓝牙 release lifecycle 和 writer bounded join 的剩余收口。
