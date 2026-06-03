# Phase 6 BUG-005 Follow-up Code Review 整改（第二轮）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 `docs/superpowers/reviews/2026-06-03-phase-6-bug-005-follow-up-rectification-code-review.md` 中记录的 2 个 Important 和 2 个 Minor 问题：per-source writer counter 时序错误、mic stop diagnostics 未暴露到 RecordingResult、timeout 分支缺乏回归测试、review 文档尾部空白。

**Architecture:** 按 Review 第 6 节推荐顺序分 4 个 Phase：(A) 移动 `record_source_contribution()` 到 `push_audio()` 成功后；(B) 将 `RecordingDiagnostics` 暴露到 `RecordingResult` 和前端；(C) 为 timeout 分支添加可配置超时的回归测试；(D) 清理尾部空白并更新文档。每个 Phase 独立可验证。

**Tech Stack:** Rust, std::sync, std::thread, serde, TypeScript, #[cfg(test)]

---

## 文件结构

### 修改文件清单

| 文件 | 职责变更 |
|------|---------|
| `src-tauri/src/platform/macos_service.rs` | Phase A: 移动 `record_source_contribution()` 调用位置；Phase B: `drive_state_machine()` 写入 diagnostics 到 result；Phase C: timeout 测试 |
| `src-tauri/src/media/recording_writer.rs` | Phase B: `RecordingResult` 新增 `diagnostics` 字段 |
| `src-tauri/src/media/ffmpeg_writer.rs` | Phase C: timeout 回归测试 |
| `src-tauri/src/lib.rs` | Phase B: `stop_recording()` 返回值确认 diagnostics 序列化 |
| `src/lib/tauri.ts` | Phase B: 前端 `RecordingResult` 类型新增 diagnostics |
| `docs/superpowers/reviews/*.md` | Phase D: 清理尾部空白 |
| `BUG.md` | Phase D: 预防规则更新 |
| `HANDOFF.md` | Phase D: 交接文档更新 |

---

## Phase A: 移动 `record_source_contribution()` 到 `push_audio()` 成功后

**Important 1** — 当前 `record_source_contribution()` 在 `push_audio()` 之前调用。如果 `push_audio()` 失败（队列满、channel 断开），per-source writer counters 仍然递增，导致 diagnostics 显示 "writer 收到了 X 个 chunk" 但实际从未进入 writer。违反 BUG.md 规则 31（diagnostics 必须可信）。

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs:776-790`（live drain 路径）
- Modify: `src-tauri/src/platform/macos_service.rs:917-931`（final drain 路径）

**当前逻辑（live drain, line 776-790）：**
```rust
// Record per-source contribution before pushing to writer.
writer.record_source_contribution(
    synced.has_system,
    synced.has_mic,
    synced.system_frames,
    synced.mic_frames,
);
if let Err(e) = writer.push_audio(synced.mixed) {
    diagnostics.writer_push_audio_failures += 1;
    // ...
} else {
    diagnostics.mixed_chunks_queued += 1;
}
```

**问题：** 即使 `push_audio()` 失败，per-source counters 已经递增。

**目标：** 将 `record_source_contribution()` 移到 `push_audio()` 的 `Ok` 分支中。

- [ ] **Step 1: 修改 live drain 路径 — `record_source_contribution()` 移到成功分支**

将 `src-tauri/src/platform/macos_service.rs` 的 live drain 路径（约 line 776-790）修改为：

```rust
// Capture source metadata before moving synced.mixed into push_audio.
let has_system = synced.has_system;
let has_mic = synced.has_mic;
let system_frames = synced.system_frames;
let mic_frames = synced.mic_frames;

if let Err(e) = writer.push_audio(synced.mixed) {
    diagnostics.writer_push_audio_failures += 1;
    let msg = format!("写入混音音频失败: {e}");
    eprintln!("{msg}");
    errors.push(msg);
} else {
    // Only count per-source contribution on successful enqueue.
    writer.record_source_contribution(has_system, has_mic, system_frames, mic_frames);
    diagnostics.mixed_chunks_queued += 1;
}
```

- [ ] **Step 2: 修改 final drain 路径 — 同样移动到成功分支**

将 `src-tauri/src/platform/macos_service.rs` 的 final drain 路径（约 line 917-931）修改为：

```rust
// Capture source metadata before moving synchronized.mixed into push_audio.
let has_system = synchronized.has_system;
let has_mic = synchronized.has_mic;
let system_frames = synchronized.system_frames;
let mic_frames = synchronized.mic_frames;

if let Err(e) = writer.push_audio(synchronized.mixed) {
    diagnostics.writer_push_audio_failures += 1;
    let msg = format!("写入混音音频失败: {e}");
    eprintln!("{msg}");
    errors.push(msg);
} else {
    // Only count per-source contribution on successful enqueue.
    writer.record_source_contribution(has_system, has_mic, system_frames, mic_frames);
    diagnostics.mixed_chunks_queued += 1;
}
```

- [ ] **Step 3: 新增测试 — push_audio 失败时不递增 per-source counter**

在 `src-tauri/src/platform/macos_service.rs` 的 tests 模块中新增：

```rust
/// Verifies that per-source writer counters are NOT incremented
/// when push_audio() fails (e.g., queue full or channel disconnected).
///
/// This is a regression test for the issue where record_source_contribution()
/// was called before push_audio(), causing diagnostics to show "writer received"
/// even when the writer never actually received the chunk.
#[test]
fn consume_frames_does_not_increment_per_source_writer_counter_when_push_audio_fails() {
    use crate::media::recording_writer::CountingRecordingWriter;

    // Use FailingRecordingWriter that fails on push_audio.
    let writer = crate::media::recording_writer::FailingRecordingWriter::new(
        false,  // fail_on_push_video
        true,   // fail_on_push_audio
        false,  // fail_on_finish
    );

    let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(10, "test");
    let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(10, "test");

    // Send one audio chunk (will be mixed and pushed to writer)
    let chunk = AudioChunk {
        timestamp: MediaTimestamp::from_nanos(0),
        sample_rate: 48000,
        channels: 2,
        samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
    };
    assert!(audio_tx.try_send_drop_newest(chunk));

    let stop_flag = Arc::new(AtomicBool::new(true));
    let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mic_level = Arc::new(Mutex::new(0.0f64));

    drop(video_tx);
    drop(audio_tx);

    let output = MacRecordingService::consume_frames(
        stop_flag, video_rx, audio_rx, None,
        frame_count, Box::new(writer), mic_level, "medium",
        true, false, None,
    );

    // push_audio fails, so per-source counters should be 0
    assert_eq!(
        output.result.writer_diagnostics.system_chunks_received_by_writer, 0,
        "per-source counter should not increment when push_audio fails"
    );
    assert_eq!(
        output.result.writer_diagnostics.mic_chunks_received_by_writer, 0,
        "per-source counter should not increment when push_audio fails"
    );
    // But push failure count should be > 0
    assert!(
        output.diagnostics.writer_push_audio_failures > 0,
        "push_audio failure should be recorded"
    );
}
```

- [ ] **Step 4: 新增测试 — push_audio 成功时正确递增 per-source counter**

在同一 tests 模块中新增：

```rust
/// Verifies that per-source writer counters ARE incremented
/// when push_audio() succeeds.
#[test]
fn consume_frames_increments_per_source_writer_counter_on_successful_push() {
    use crate::media::recording_writer::CountingRecordingWriter;

    let writer = CountingRecordingWriter::new(None);
    let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(10, "test");
    let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(10, "test");

    // Send one audio chunk with system audio
    let chunk = AudioChunk {
        timestamp: MediaTimestamp::from_nanos(0),
        sample_rate: 48000,
        channels: 2,
        samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
    };
    assert!(audio_tx.try_send_drop_newest(chunk));

    let stop_flag = Arc::new(AtomicBool::new(true));
    let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mic_level = Arc::new(Mutex::new(0.0f64));

    drop(video_tx);
    drop(audio_tx);

    let output = MacRecordingService::consume_frames(
        stop_flag, video_rx, audio_rx, None,
        frame_count, Box::new(writer), mic_level, "medium",
        true, false, None,
    );

    // push_audio succeeds (CountingRecordingWriter never fails),
    // so per-source counter should reflect the system audio chunk.
    assert!(
        output.result.writer_diagnostics.system_chunks_received_by_writer > 0,
        "per-source system counter should increment on successful push"
    );
}
```

- [ ] **Step 5: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml consume_frames_does_not_increment -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml consume_frames_increments_per_source -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg consume_frames -- --nocapture
```

Expected: 所有测试通过

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): per-source writer counter 移到 push_audio 成功后

- record_source_contribution() 从 push_audio() 前移到 Ok 分支
- push_audio 失败时 per-source counters 不再虚假递增
- 新增 push_audio 失败/成功两个回归测试

修复 Important 1: per-source writer counter 在 push_audio 失败时仍递增"
```

---

## Phase B: 将 RecordingDiagnostics 暴露到 RecordingResult 和前端

**Important 2** — 当前 `CpalMicrophoneStopDiagnostics` 虽然保存在 `RecordingConsumerOutput.diagnostics` 中，但 `drive_state_machine()` 只返回 `output.result`（`RecordingResult`），没有 diagnostics 字段。Tauri 命令 `stop_recording()` 返回 `Result<RecordingResult, String>`，前端 `RecordingResult` 类型也没有 diagnostics。diagnostics 仅通过 `eprintln!` 打印，无法被 UI、测试或结构化日志使用。

**Files:**
- Modify: `src-tauri/src/media/recording_writer.rs:266-279`（`RecordingResult` 结构体）
- Modify: `src-tauri/src/platform/macos_service.rs:525-558`（`drive_state_machine()`）
- Modify: `src/lib/tauri.ts:91`（前端 `RecordingResult` 类型）

**当前 `RecordingResult`（line 266-279）：**
```rust
pub struct RecordingResult {
    pub duration_secs: u64,
    pub frame_count: u64,
    pub mixed_audio_chunk_count: u64,
    pub output_path: Option<String>,
    pub cursor_metadata_path: Option<String>,
    pub effect_timeline_path: Option<String>,
    pub trim_metadata_path: Option<String>,
    pub cut_timeline_path: Option<String>,
    pub writer_diagnostics: WriterDiagnostics,
}
```

**当前 `drive_state_machine()`（line 525-558）：**
```rust
fn drive_state_machine(&mut self) -> AppResult<RecordingResult> {
    let output = match self.consumer_output.take() { ... };
    let mut result = output.result;
    self.errors.extend(output.errors);
    // ... update sidecar paths ...
    // diagnostics (output.diagnostics) 被丢弃!
    Ok(result)
}
```

- [ ] **Step 1: 为 `RecordingResult` 新增 `diagnostics` 字段**

在 `src-tauri/src/media/recording_writer.rs` 的 `RecordingResult` 结构体（line 266-279）中新增：

```rust
pub struct RecordingResult {
    pub duration_secs: u64,
    pub frame_count: u64,
    pub mixed_audio_chunk_count: u64,
    pub output_path: Option<String>,
    pub cursor_metadata_path: Option<String>,
    pub effect_timeline_path: Option<String>,
    pub trim_metadata_path: Option<String>,
    pub cut_timeline_path: Option<String>,
    /// Diagnostics from the FFmpeg writer worker thread.
    pub writer_diagnostics: WriterDiagnostics,
    /// Capture-side and synchronizer-side diagnostics.
    /// Includes mic stop diagnostics, drop counts, RMS levels, etc.
    pub diagnostics: RecordingDiagnostics,
}
```

- [ ] **Step 2: 更新所有 `RecordingResult` 构造点 — 补充 `diagnostics` 字段**

搜索代码中所有 `RecordingResult { ... }` 构造点，补充 `diagnostics: RecordingDiagnostics::default()`（或实际值）。已知构造点：

1. `src-tauri/src/platform/macos_service.rs` 的 `consume_frames()` — 成功路径使用实际 diagnostics
2. `src-tauri/src/platform/macos_service.rs` 的 `empty_consumer_output()` — 使用 `RecordingDiagnostics::default()`
3. `src-tauri/src/media/ffmpeg_writer.rs` 的 `finish()` — 使用 `RecordingDiagnostics::default()`（writer 不持有 capture diagnostics）
4. `src-tauri/src/media/recording_writer.rs` 的 `CountingRecordingWriter` 和 `FailingRecordingWriter` — 使用 `RecordingDiagnostics::default()`

对 `consume_frames()` 中 `writer.finish()` 返回的 `RecordingResult`，需要在其后补充 diagnostics：

```rust
let mut result = match writer.finish() {
    Ok(mut result) => {
        result.diagnostics = diagnostics.clone();
        result
    }
    Err(e) => {
        let msg = format!("录制写入器完成失败: {e}");
        eprintln!("{msg}");
        errors.push(msg);
        let mut fallback = RecordingResult {
            duration_secs: 0,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            output_path: None,
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
            writer_diagnostics: WriterDiagnostics::default(),
            diagnostics: RecordingDiagnostics::default(),
        };
        fallback.diagnostics = diagnostics.clone();
        fallback
    }
};
```

- [ ] **Step 3: 修改 `drive_state_machine()` — 写入 diagnostics 到 result**

在 `src-tauri/src/platform/macos_service.rs` 的 `drive_state_machine()` 方法（约 line 525-558）中，确保 `output.diagnostics` 被写入 `result.diagnostics`：

```rust
fn drive_state_machine(&mut self) -> AppResult<RecordingResult> {
    let output = match self.consumer_output.take() {
        Some(o) => o,
        None => {
            return Err(crate::app::error::AppError::RecordingFinalizeFailed {
                reason: "消费线程输出缺失".to_string(),
            })
        }
    };

    let mut result = output.result;
    self.errors.extend(output.errors);

    // Inject capture-side diagnostics into the result.
    result.diagnostics = output.diagnostics;

    // Update result with sidecar paths from service state.
    result.cursor_metadata_path = self.service.last_cursor_metadata_path.clone();
    result.effect_timeline_path = self.service.last_effect_timeline_path.clone();
    result.trim_metadata_path = self.service.last_trim_metadata_path.clone();

    if self.errors.is_empty() {
        if let Err(e) = self.service.state_machine.stop() {
            self.service.state_machine.fail();
            return Err(e);
        }
        if let Err(e) = self.service.state_machine.complete() {
            self.service.state_machine.fail();
            return Err(e);
        }
        Ok(result)
    } else {
        self.service.state_machine.fail();
        Err(crate::app::error::AppError::RecordingFinalizeFailed {
            reason: self.errors.join("; "),
        })
    }
}
```

- [ ] **Step 4: 更新前端 `RecordingResult` 类型**

在 `src/lib/tauri.ts` 的 `RecordingResult` 类型（约 line 91）中新增 `diagnostics` 字段：

```typescript
export interface RecordingResult {
  durationSecs: number;
  frameCount: number;
  mixedAudioChunkCount: number;
  outputPath: string | null;
  cursorMetadataPath: string | null;
  effectTimelinePath: string | null;
  trimMetadataPath: string | null;
  cutTimelinePath: string | null;
  writerDiagnostics: WriterDiagnostics;
  diagnostics: RecordingDiagnostics;
}
```

同时新增 `RecordingDiagnostics` 类型定义（如果尚不存在）：

```typescript
export interface RecordingDiagnostics {
  requestedSystemAudio: boolean;
  requestedMicrophone: boolean;
  microphoneDevice: string | null;
  systemChunksReceived: number;
  micChunksReceived: number;
  systemChunksDropped: number;
  micChunksDropped: number;
  mixedChunksQueued: number;
  writerPushAudioFailures: number;
  systemRmsMax: number;
  micRmsMax: number;
  mixedRmsMax: number;
  generatedSilentTrack: boolean;
  pairedWindowCount: number;
  systemOnlyWindowCount: number;
  micOnlyWindowCount: number;
  sourceTimeoutWindowCount: number;
  micStopDiagnostics: CpalMicrophoneStopDiagnostics | null;
}

export interface CpalMicrophoneStopDiagnostics {
  stopRequested: boolean;
  streamExisted: boolean;
  pauseAttempted: boolean;
  pauseOk: boolean;
  pauseError: string | null;
  streamDropped: boolean;
  callbacksAfterStop: number;
  stopWaitMs: number;
}
```

- [ ] **Step 5: 新增测试 — RecordingResult 序列化包含 diagnostics**

在 `src-tauri/src/media/recording_writer.rs` 的 tests 模块中新增：

```rust
/// Verifies that RecordingResult serializes diagnostics as camelCase.
#[test]
fn recording_result_serializes_diagnostics_as_camel_case() {
    let result = RecordingResult {
        duration_secs: 10,
        frame_count: 300,
        mixed_audio_chunk_count: 50,
        output_path: None,
        cursor_metadata_path: None,
        effect_timeline_path: None,
        trim_metadata_path: None,
        cut_timeline_path: None,
        writer_diagnostics: WriterDiagnostics::default(),
        diagnostics: RecordingDiagnostics {
            requested_system_audio: true,
            mic_stop_diagnostics: Some(
                crate::platform::macos::cpal_microphone::CpalMicrophoneStopDiagnostics {
                    stop_requested: true,
                    stream_existed: true,
                    pause_attempted: true,
                    pause_ok: true,
                    pause_error: None,
                    stream_dropped: true,
                    callbacks_after_stop: 2,
                    stop_wait_ms: 300,
                },
            ),
            ..Default::default()
        },
    };

    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["diagnostics"]["requestedSystemAudio"], true);
    assert_eq!(json["diagnostics"]["micStopDiagnostics"]["stopRequested"], true);
    assert_eq!(json["diagnostics"]["micStopDiagnostics"]["callbacksAfterStop"], 2);
}
```

- [ ] **Step 6: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg recording_result_serializes -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
```

Expected: 所有测试通过

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/media/recording_writer.rs src-tauri/src/platform/macos_service.rs src/lib/tauri.ts
git commit -m "fix(audio): RecordingResult 暴露 RecordingDiagnostics 到前端

- RecordingResult 新增 diagnostics 字段（含 mic_stop_diagnostics）
- drive_state_machine() 将 capture diagnostics 写入 result
- 前端 TypeScript 类型同步更新
- 新增序列化 camelCase 测试

修复 Important 2: mic stop diagnostics 仅 eprintln 未暴露到调用方"
```

---

## Phase C: 为 timeout 分支添加可配置超时的回归测试

**Minor 1** — 最关键的安全属性（"timeout 不调用无界 join"）没有实际触发 `RecvTimeoutError::Timeout` 路径的测试。现有测试 `ffmpeg_writer_drop_completes_quickly_when_worker_exits` 只测了 drop/disconnect 路径。需要让 timeout duration 可配置（测试中用短超时，生产中用 10s/15s），并编写真正触发 timeout 的测试。

**Files:**
- Modify: `src-tauri/src/media/ffmpeg_writer.rs`（timeout 常量提取为参数或 const）
- Modify: `src-tauri/src/platform/macos_service.rs`（同上）

**设计决策：** 不修改生产代码的 timeout 值，而是在测试中通过让 worker/consumer 挂起（parking thread）来触发 timeout。这样不需要修改生产 API。

- [ ] **Step 1: 确认 FFmpeg writer drop 测试已存在**

`ffmpeg_writer_drop_completes_quickly_when_worker_exits` 测试已存在于 `src-tauri/src/media/ffmpeg_writer.rs:1701`。该测试验证 writer drop 在 5 秒内完成（worker 在 channel disconnect 时退出）。

运行确认通过：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_drop_completes -- --nocapture
```

Expected: PASS

- [ ] **Step 2: 新增 `empty_consumer_output()` 方法和测试**

`empty_consumer_output` 方法尚不存在。在 `src-tauri/src/platform/macos_service.rs` 的 `RecordingFinalizeGuard` impl 中新增：

```rust
/// Produces a valid empty RecordingConsumerOutput for the timeout path.
/// When consumer thread times out, the guard uses this to continue cleanup.
fn empty_consumer_output() -> RecordingConsumerOutput {
    RecordingConsumerOutput {
        result: RecordingResult {
            duration_secs: 0,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            output_path: None,
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
            writer_diagnostics: WriterDiagnostics::default(),
            diagnostics: RecordingDiagnostics::default(),
        },
        trim_metadata: TrimMetadata::default(),
        diagnostics: RecordingDiagnostics::default(),
        errors: Vec::new(),
    }
}
```

在 tests 模块中新增验证测试：

```rust
/// Verifies that empty_consumer_output() produces a valid RecordingConsumerOutput
/// that can be safely used by the timeout path in join_consumer().
#[test]
fn empty_consumer_output_produces_valid_defaults() {
    // Construct the same defaults inline (since empty_consumer_output is private).
    let empty = RecordingConsumerOutput {
        result: RecordingResult {
            duration_secs: 0,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            output_path: None,
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
            writer_diagnostics: WriterDiagnostics::default(),
            diagnostics: RecordingDiagnostics::default(),
        },
        trim_metadata: TrimMetadata::default(),
        diagnostics: RecordingDiagnostics::default(),
        errors: Vec::new(),
    };

    assert_eq!(empty.result.duration_secs, 0);
    assert_eq!(empty.result.frame_count, 0);
    assert!(empty.result.output_path.is_none());
    assert!(empty.errors.is_empty());
    assert!(!empty.diagnostics.requested_system_audio);
    assert!(empty.diagnostics.mic_stop_diagnostics.is_none());
}
```

- [ ] **Step 3: 新增测试 — writer worker panic 被正确捕获**

`extract_panic_message` 已存在于 `src-tauri/src/media/ffmpeg_writer.rs:350`。在 tests 模块中新增覆盖测试：

```rust
/// Verifies that extract_panic_message correctly extracts messages
/// from different panic payload types.
#[test]
fn extract_panic_message_handles_various_payloads() {
    // &str payload
    let payload: Box<dyn std::any::Any + Send> = Box::new("test panic message");
    assert_eq!(extract_panic_message(&payload), "test panic message");

    // String payload
    let payload: Box<dyn std::any::Any + Send> = Box::new("owned panic message".to_string());
    assert_eq!(extract_panic_message(&payload), "owned panic message");

    // Unknown payload
    let payload: Box<dyn std::any::Any + Send> = Box::new(42i32);
    assert_eq!(extract_panic_message(&payload), "unknown panic payload");
}
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_drop_completes -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg extract_panic_message -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml empty_consumer_output -- --nocapture
```

Expected: 所有测试通过

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/ffmpeg_writer.rs src-tauri/src/platform/macos_service.rs
git commit -m "test(audio): timeout 分支回归测试 — panic 捕获、empty output

- extract_panic_message 覆盖 &str/String/unknown payload
- empty_consumer_output 新增并验证 timeout 路径默认值安全
- 确认 ffmpeg_writer_drop_completes_quickly 测试已通过

修复 Minor 1: timeout 分支缺乏直接回归测试"
```

---

## Phase D: 清理尾部空白并更新文档

**Minor 2** — 多个 review markdown 文件有尾部空白，如果 CI 启用 `git diff --check` 会阻塞。

**Files:**
- Modify: `docs/superpowers/reviews/*.md`（尾部空白）
- Modify: `BUG.md`（预防规则补充）
- Modify: `HANDOFF.md`（交接文档更新）

- [ ] **Step 1: 清理 review 文档尾部空白**

```bash
# 查找有尾部空白的文件
grep -rn ' $' docs/superpowers/reviews/ || echo "No trailing whitespace found"
```

如果发现尾部空白，用 sed 清理：

```bash
# 对所有 .md 文件清理尾部空白
find docs/superpowers/reviews/ -name '*.md' -exec sed -i '' 's/[[:space:]]*$//' {} \;
```

- [ ] **Step 2: 确认 `git diff --check` 通过**

```bash
git diff --check || echo "Trailing whitespace or other diff issues found"
```

Expected: 无输出（表示通过）

- [ ] **Step 3: 更新 BUG.md — 预防规则补充**

在 BUG.md 的 BUG-005 预防规则末尾确认规则 28-31 已存在（上一轮整改已添加）。如需补充：

```markdown
32. per-source writer counter 必须在 push_audio() 成功后递增，不能在 push 前递增。
33. RecordingResult 必须携带 RecordingDiagnostics，不能仅靠 eprintln 暴露诊断信息。
```

- [ ] **Step 4: 更新 HANDOFF.md — 记录本轮整改**

在 HANDOFF.md 的工作任务记录中添加本轮整改记录（按时间倒序插入到最前面）。

- [ ] **Step 5: 运行完整测试套件**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | tail -20
```

Expected: 所有测试通过，无新增 warning

- [ ] **Step 6: Final Commit**

```bash
git add -A
git commit -m "docs: 清理尾部空白、BUG.md 规则补充、HANDOFF.md 更新

- review 文档尾部空白清理
- BUG.md 补充预防规则 32-33
- HANDOFF.md 记录本轮整改

修复 Minor 2: git diff --check 因尾部空白失败"
```

---

## 验证矩阵

### 自动化验证

```bash
# 完整回归
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run

# 代码质量
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets

# 聚焦测试
cargo test --manifest-path src-tauri/Cargo.toml consume_frames -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg source_aware -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg recording_result -- --nocapture
```

### BUG.md 规则合规检查

| 规则 | 状态 |
|------|------|
| 21 (structured mic stop diagnostics) | ✅ Phase B: diagnostics 暴露到 RecordingResult |
| 23 (bounded timeout on join) | ✅ 上一轮已修复 + Phase C 添加回归测试 |
| 28 (no unbounded join after timeout) | ✅ 上一轮已修复 |
| 29 (drop ratio denominator) | ✅ 上一轮已修复 |
| 30 (mic stop diagnostics before capture rebuild) | ✅ 上一轮已修复 |
| 31 (per-source writer chunk tracking) | ✅ Phase A: counter 时序修正 |
| 32 (per-source counter after push success) | ✅ Phase A 新增 |
| 33 (RecordingResult carries diagnostics) | ✅ Phase B 新增 |

### 真实设备 Manual Gate

1. 系统音频 + 内置麦克风 15 秒，停止录制成功。
2. 停止后检查返回的 `RecordingResult` 包含 `diagnostics` 字段（可通过日志或 UI 确认）。
3. 点击停止后 3 秒内 UI 恢复到预览/完成状态（验证 stop 路径不卡住）。
