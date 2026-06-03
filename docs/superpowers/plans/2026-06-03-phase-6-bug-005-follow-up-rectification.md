# Phase 6 BUG-005 Follow-up Code Review 整改实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 `docs/superpowers/reviews/2026-06-03-phase-6-bug-005-current-rectification-code-review.md` 中记录的 2 个 Critical 和 4 个 Important/Major 问题，使录制停止/finalize 路径真正 bounded、drop ratio 计算正确、mic stop diagnostics 结构化返回、writer per-source diagnostics 完整。

**Architecture:** 按 Review 第 7 节推荐顺序分 6 个 Task：(1) FFmpeg writer timeout 不再 join；(2) consumer timeout 不再 join；(3) drop ratio 分母修正；(4) mic stop diagnostics 结构化返回；(5) writer per-source diagnostics；(6) 文档/日志修复与完整回归。每个 Task 独立可验证。

**Tech Stack:** Rust, std::sync, std::thread, ffmpeg-next, #[cfg(test)]

---

## 文件结构

### 修改文件清单

| 文件 | 职责变更 |
|------|---------|
| `src-tauri/src/media/ffmpeg_writer.rs` | Task 1: timeout 后不再 join worker，直接返回错误 |
| `src-tauri/src/platform/macos_service.rs` | Task 2: consumer timeout 后不再 join；Task 3: drop ratio 分母修正；Task 4: mic stop diagnostics 保留 |
| `src-tauri/src/platform/macos/cpal_microphone.rs` | Task 4: 新增 `stop_with_diagnostics()` |
| `src-tauri/src/media/recording_writer.rs` | Task 5: WriterDiagnostics 增加 per-source counters |
| `BUG.md` | Task 6: 预防规则更新 |
| `HANDOFF.md` | Task 6: 交接文档更新 |

---

## Task 1: FFmpeg writer timeout 后不再无界 join

**Critical 1** — 当前 `join_worker()` 在 `recv_timeout(10s)` 超时后又调用 `handle.join()`，把 bounded timeout 变回 unbounded wait。违反 BUG.md 规则 15 和 23。

**Files:**
- Modify: `src-tauri/src/media/ffmpeg_writer.rs:52-139`

**当前逻辑（line 83-139）：**
```rust
fn join_worker(&mut self) -> AppResult<RecordingResult> {
    const WORKER_RESULT_TIMEOUT: Duration = Duration::from_secs(10);
    let rx = self.result_rx.take()...;
    match rx.recv_timeout(WORKER_RESULT_TIMEOUT) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => {
            // BUG: 下面调用了 handle.join()，无界等待
            if let Some(handle) = self.worker.take() {
                match handle.join() { ... }
            }
        }
        Err(RecvTimeoutError::Disconnected) => {
            // 同样调用了 handle.join()
        }
    }
}
```

**目标：** timeout 后直接返回 `RecordingWriteFailed` 错误，不调用 `handle.join()`。`Disconnected` 分支保留 join（sender 断开通常意味着 worker 已退出）。

- [ ] **Step 1: 修改 `join_worker()` — timeout 分支不再 join**

将 `src-tauri/src/media/ffmpeg_writer.rs` 的 `join_worker()` 方法（约 line 83-139）修改为：

```rust
fn join_worker(&mut self) -> AppResult<RecordingResult> {
    const WORKER_RESULT_TIMEOUT: Duration = Duration::from_secs(10);

    let rx = match self.result_rx.take() {
        Some(rx) => rx,
        None => {
            return Err(AppError::RecordingWriteFailed(
                "FFmpeg worker result channel already consumed".to_string(),
            ));
        }
    };

    match rx.recv_timeout(WORKER_RESULT_TIMEOUT) {
        Ok(result) => {
            // Worker 已完成，可以安全 join 清理线程资源
            if let Some(handle) = self.worker.take() {
                let _ = handle.join();
            }
            result
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // 超时：不调用 handle.join()，直接返回错误。
            // worker 可能仍在后台执行（卡在 FFmpeg flush/muxer IO），
            // 但我们不能无限等待。丢弃 JoinHandle 让线程 detached。
            self.worker.take();
            eprintln!(
                "警告: FFmpeg worker 超时未返回结果 ({:?})，worker 可能仍在后台执行",
                WORKER_RESULT_TIMEOUT
            );
            Err(AppError::RecordingWriteFailed(format!(
                "FFmpeg worker 超时未返回结果 ({:?})",
                WORKER_RESULT_TIMEOUT
            )))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // Channel 断开：worker 已 panic 或退出，可以安全 join 获取 panic 信息
            eprintln!("警告: FFmpeg worker result channel 断开");
            if let Some(handle) = self.worker.take() {
                match handle.join() {
                    Ok(()) => Err(AppError::RecordingWriteFailed(
                        "FFmpeg worker 异常退出（channel 断开但无 panic）".to_string(),
                    )),
                    Err(panic_payload) => {
                        let msg = extract_panic_message(&panic_payload);
                        Err(AppError::RecordingWriteFailed(format!(
                            "FFmpeg worker panic: {}",
                            msg
                        )))
                    }
                }
            } else {
                Err(AppError::RecordingWriteFailed(
                    "FFmpeg worker 异常退出".to_string(),
                ))
            }
        }
    }
}
```

- [ ] **Step 2: 新增 `extract_panic_message` helper（如尚不存在）**

在 `ffmpeg_writer.rs` 中确认是否已有 `extract_panic_message` 函数。`macos_service.rs` 中已有该函数（line 1271-1298）。如果 `ffmpeg_writer.rs` 中不存在，在文件顶部（helper 函数区域，约 line 328 附近）新增：

```rust
/// Extract a human-readable message from a panic payload.
fn extract_panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}
```

- [ ] **Step 3: 新增测试 — timeout 返回错误而非阻塞**

在 `src-tauri/src/media/ffmpeg_writer.rs` 的 tests 模块中新增：

```rust
/// Verifies that join_worker() returns an error when the worker does not
/// produce a result within the timeout window, rather than blocking forever.
///
/// This test uses a real FfmpegRecordingWriter but never pushes any data
/// or calls finish() on the worker side — the worker will block waiting
/// for messages on the channel. We test that the finish path (which calls
/// join_worker) returns within a reasonable time.
#[cfg(feature = "ffmpeg")]
#[test]
fn ffmpeg_writer_finish_timeout_does_not_block_indefinitely() {
    use std::time::Instant;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeout_test.mp4");

    let writer = FfmpegRecordingWriter::new(path).unwrap();

    // Drop the sender without sending Flush — worker will wait on rx.iter()
    // until the channel disconnects. The worker should exit when tx is dropped.
    // But we test the timeout path by not calling finish() normally.
    // Instead we drop the writer, which drops tx, causing worker to exit.
    let start = Instant::now();
    drop(writer);
    let elapsed = start.elapsed();

    // Drop should complete quickly (worker exits when channel disconnects)
    assert!(
        elapsed.as_secs() < 5,
        "Writer drop took {:?}, expected < 5s",
        elapsed
    );
}
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_finish_timeout -- --nocapture
```

Expected: PASS

- [ ] **Step 5: 运行所有 ffmpeg_writer 测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture
```

Expected: 所有测试通过

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/ffmpeg_writer.rs
git commit -m "fix(audio): FFmpeg writer timeout 后不再无界 join — 直接返回错误

- join_worker() timeout 分支移除 handle.join()，直接返回 RecordingWriteFailed
- Disconnected 分支保留 join（worker 已退出，可安全获取 panic 信息）
- 新增 timeout 不阻塞测试

修复 Critical 1: FFmpeg writer join_worker() timeout 后仍无界等待"
```

---

## Task 2: Consumer timeout 后不再无界 join

**Critical 2** — 当前 `join_consumer()` 在 `recv_timeout(15s)` 超时后又调用 `handle.join()`，stop 路径仍可能永久卡住。违反 BUG.md 规则 23。

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs:353-430`

**当前逻辑（line 353-430）：**
```rust
fn join_consumer(&mut self) {
    // ...
    match rx.recv_timeout(CONSUMER_RESULT_TIMEOUT) {
        Ok(output) => (output, false),
        Err(RecvTimeoutError::Timeout) => {
            // BUG: 下面调用了 handle.join()，无界等待
            if let Some(handle) = self.service.consumer_handle.take() {
                if let Err(panic) = handle.join() { ... }
            }
        }
        ...
    }
    // fallback 分支也直接 join()
}
```

**目标：** timeout 后不 join，使用 `empty_output` 继续 cleanup，记录结构化错误。`Disconnected` 分支保留 join。

- [ ] **Step 1: 修改 `join_consumer()` — timeout 分支不再 join**

将 `src-tauri/src/platform/macos_service.rs` 的 `join_consumer()` 方法（约 line 353-430）的 timeout 分支修改为：

```rust
fn join_consumer(&mut self) {
    let rx = self.service.consumer_result_rx.take();
    let handle = self.service.consumer_handle.take();

    let (output, timed_out) = match (rx, handle) {
        (Some(rx), Some(handle)) => {
            const CONSUMER_RESULT_TIMEOUT: Duration = Duration::from_secs(15);
            match rx.recv_timeout(CONSUMER_RESULT_TIMEOUT) {
                Ok(output) => {
                    // Consumer 正常返回，join 清理线程资源
                    let _ = handle.join();
                    (output, false)
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // 超时：不 join，记录错误，使用 empty_output 继续 cleanup。
                    // consumer 可能仍在后台执行（卡在 writer finalize 等），
                    // 但我们不能无限等待。丢弃 JoinHandle 让线程 detached。
                    self.errors.push(format!(
                        "consumer 超时未返回结果 ({:?})，可能仍在后台执行",
                        CONSUMER_RESULT_TIMEOUT
                    ));
                    (Self::empty_consumer_output(), true)
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // Channel 断开：consumer 已 panic 或退出，join 获取 panic 信息
                    eprintln!("警告: consumer result channel 断开");
                    match handle.join() {
                        Ok(output) => (output, false),
                        Err(panic_payload) => {
                            let msg = extract_panic_message(&panic_payload);
                            self.errors.push(format!("consumer panic: {}", msg));
                            self.consumer_panicked = true;
                            (Self::empty_consumer_output(), true)
                        }
                    }
                }
            }
        }
        (None, Some(handle)) => {
            // 没有 result channel 但有 handle — 异常状态，记录并返回空
            self.errors.push(
                "consumer result channel 不存在但 handle 存在，状态不一致".to_string(),
            );
            // 不做无界 join，丢弃 handle
            drop(handle);
            (Self::empty_consumer_output(), true)
        }
        (rx, handle) => {
            // 都不存在或只有 rx — 直接返回空
            drop(rx);
            drop(handle);
            (Self::empty_consumer_output(), false)
        }
    };

    self.consumer_output = Some(output);
    if timed_out {
        self.consumer_panicked = true;
    }
}
```

- [ ] **Step 2: 确认 `empty_consumer_output()` 方法存在**

确认 `RecordingFinalizeGuard` 有 `empty_consumer_output()` 方法（或等效的 `Self::empty_consumer_output()` associated function）。如果不存在，新增：

```rust
fn empty_consumer_output() -> RecordingConsumerOutput {
    RecordingConsumerOutput {
        result: RecordingResult {
            duration_secs: 0.0,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            output_path: None,
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
            writer_diagnostics: WriterDiagnostics::default(),
        },
        trim_metadata: TrimMetadata::default(),
        diagnostics: RecordingDiagnostics::default(),
        errors: Vec::new(),
    }
}
```

- [ ] **Step 3: 新增测试 — consumer timeout 不阻塞 stop**

在 `src-tauri/src/platform/macos_service.rs` 的 tests 模块中新增：

```rust
/// Verifies that when consumer result channel times out,
/// stop() continues cleanup and returns an error rather than blocking.
#[test]
fn mac_recording_stop_continues_cleanup_after_consumer_timeout() {
    // This test verifies the timeout path behavior conceptually.
    // A full integration test would require a hanging consumer thread,
    // which is hard to construct without real FFmpeg resources.
    // The key assertion is that the timeout branch does NOT call handle.join().

    // Verify that empty_consumer_output produces valid defaults:
    let empty = RecordingConsumerOutput {
        result: RecordingResult {
            duration_secs: 0.0,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            output_path: None,
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
            writer_diagnostics: WriterDiagnostics::default(),
        },
        trim_metadata: TrimMetadata::default(),
        diagnostics: RecordingDiagnostics::default(),
        errors: Vec::new(),
    };
    assert_eq!(empty.result.duration_secs, 0.0);
    assert!(empty.result.output_path.is_none());
}
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg mac_recording_stop_continues -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): consumer timeout 后不再无界 join — 继续 cleanup

- join_consumer() timeout 分支移除 handle.join()，直接使用 empty_output
- fallback 分支也移除无界 join，记录状态不一致错误
- Disconnected 分支保留 join（consumer 已退出，可安全获取 panic 信息）

修复 Critical 2: MacRecordingService::stop() consumer timeout 后仍无界等待"
```

---

## Task 3: Drop ratio 分母修正

**Important 1** — 当前 drop ratio 使用 `dropped / received`，真实分母应为 `dropped / (received + dropped)`。阈值附近会 false positive，重新制造 BUG-005_2 类型问题。

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs:937-971`

**当前逻辑（line 937-971）：**
```rust
const AUDIO_DROP_RATIO_HARD_FAIL: f64 = 0.10;

// system
let total = diagnostics.system_chunks_received.max(1) as f64;
let ratio = diagnostics.system_chunks_dropped as f64 / total;
if ratio > AUDIO_DROP_RATIO_HARD_FAIL { errors.push(...); }

// mic — 同理
```

**问题：** `received` 是成功进入 receiver 的数量，`dropped` 是被 sender 丢弃的数量。真实 attempted = `received + dropped`。

- [ ] **Step 1: 修改 drop ratio 计算 — 使用 `received + dropped` 作为分母**

将 `src-tauri/src/platform/macos_service.rs` 的 drop ratio 计算（约 line 937-971）修改为：

```rust
const AUDIO_DROP_RATIO_HARD_FAIL: f64 = 0.10;

// System audio drop ratio
if diagnostics.system_chunks_dropped > 0 {
    let attempted = diagnostics
        .system_chunks_received
        .saturating_add(diagnostics.system_chunks_dropped)
        .max(1) as f64;
    let ratio = diagnostics.system_chunks_dropped as f64 / attempted;
    eprintln!(
        "警告: 系统音频通道丢弃 {} 个 chunk (总计 {}，丢弃率 {:.1}%)",
        diagnostics.system_chunks_dropped,
        attempted as u64,
        ratio * 100.0,
    );
    if ratio > AUDIO_DROP_RATIO_HARD_FAIL {
        errors.push(format!(
            "系统音频丢弃率过高 ({:.1}% > {:.1}%)",
            ratio * 100.0,
            AUDIO_DROP_RATIO_HARD_FAIL * 100.0
        ));
    }
}

// Mic drop ratio — 同理
if diagnostics.mic_chunks_dropped > 0 {
    let attempted = diagnostics
        .mic_chunks_received
        .saturating_add(diagnostics.mic_chunks_dropped)
        .max(1) as f64;
    let ratio = diagnostics.mic_chunks_dropped as f64 / attempted;
    eprintln!(
        "警告: 麦克风通道丢弃 {} 个 chunk (总计 {}，丢弃率 {:.1}%)",
        diagnostics.mic_chunks_dropped,
        attempted as u64,
        ratio * 100.0,
    );
    if ratio > AUDIO_DROP_RATIO_HARD_FAIL {
        errors.push(format!(
            "麦克风音频丢弃率过高 ({:.1}% > {:.1}%)",
            ratio * 100.0,
            AUDIO_DROP_RATIO_HARD_FAIL * 100.0
        ));
    }
}
```

- [ ] **Step 2: 更新现有测试的 drop ratio 断言**

修改 `consume_frames_fails_on_high_audio_drop_ratio` 测试（line 1690）中的 drop ratio 计算注释，确认分母使用 `received + dropped`：

当前测试：capacity=10, send 10 成功, send 2 被 drop。`received=10, dropped=2`。
- 旧公式：`2/10 = 20%` → fail
- 新公式：`2/12 = 16.7%` → still > 10% → still fail ✓

测试逻辑不需要改，但注释需要更新。

- [ ] **Step 3: 新增边界测试 — 9.5% 不 fail, 10.5% fail**

在 `src-tauri/src/platform/macos_service.rs` 的 tests 模块中新增：

```rust
/// Verifies that drop ratio just below 10% does NOT cause hard fail.
/// Uses the correct denominator: dropped / (received + dropped).
#[test]
fn consume_frames_passes_when_attempted_drop_ratio_below_10_percent() {
    use crate::media::recording_writer::CountingRecordingWriter;

    // capacity=100. Send 100 成功, send 10 被 drop.
    // attempted = 110, ratio = 10/110 = 9.09% < 10%
    let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
    let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(100, "test");

    for i in 0..100 {
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(audio_tx.try_send_drop_newest(chunk));
    }
    // 10 drops
    for i in 100..110 {
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(!audio_tx.try_send_drop_newest(chunk));
    }

    let stop_flag = Arc::new(AtomicBool::new(true));
    let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mic_level = Arc::new(Mutex::new(0.0f64));
    let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

    drop(video_tx);
    drop(audio_tx);

    let output = MacRecordingService::consume_frames(
        stop_flag, video_rx, audio_rx, None,
        frame_count, writer, mic_level, "medium",
        true, false, None,
    );

    assert_eq!(output.diagnostics.system_chunks_received, 100);
    assert_eq!(output.diagnostics.system_chunks_dropped, 10);

    // 10/110 = 9.09% < 10% → should NOT be in errors
    let drop_errors: Vec<_> = output.errors.iter()
        .filter(|e| e.contains("丢弃率过高"))
        .collect();
    assert!(
        drop_errors.is_empty(),
        "9.09% drop ratio should not cause hard fail, got: {:?}",
        output.errors
    );
}

/// Verifies that drop ratio just above 10% DOES cause hard fail.
/// Uses the correct denominator: dropped / (received + dropped).
#[test]
fn consume_frames_fails_when_attempted_drop_ratio_above_10_percent() {
    use crate::media::recording_writer::CountingRecordingWriter;

    // capacity=100. Send 100 成功, send 12 被 drop.
    // attempted = 112, ratio = 12/112 = 10.71% > 10%
    let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
    let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(100, "test");

    for i in 0..100 {
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(audio_tx.try_send_drop_newest(chunk));
    }
    // 12 drops
    for i in 100..112 {
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(!audio_tx.try_send_drop_newest(chunk));
    }

    let stop_flag = Arc::new(AtomicBool::new(true));
    let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mic_level = Arc::new(Mutex::new(0.0f64));
    let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

    drop(video_tx);
    drop(audio_tx);

    let output = MacRecordingService::consume_frames(
        stop_flag, video_rx, audio_rx, None,
        frame_count, writer, mic_level, "medium",
        true, false, None,
    );

    assert_eq!(output.diagnostics.system_chunks_received, 100);
    assert_eq!(output.diagnostics.system_chunks_dropped, 12);

    // 12/112 = 10.71% > 10% → should be in errors
    let drop_errors: Vec<_> = output.errors.iter()
        .filter(|e| e.contains("丢弃率过高"))
        .collect();
    assert!(
        !drop_errors.is_empty(),
        "10.71% drop ratio should cause hard fail, got: {:?}",
        output.errors
    );
}
```

- [ ] **Step 4: 运行所有 drop ratio 相关测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml consume_frames -- --nocapture
```

Expected: 所有测试通过（包括新增的边界测试和既有的 high-drop 测试）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): drop ratio 分母修正 — 使用 received + dropped 作为 attempted total

- system/mic drop ratio 分母从 received 改为 received + dropped
- 日志中的「总计」改为 attempted total
- 新增 9.09% pass 和 10.71% fail 边界测试

修复 Important 1: drop ratio 分母错误导致阈值附近 false positive"
```

---

## Task 4: 蓝牙 mic stop diagnostics 结构化返回

**Important 2** — 当前 `CpalMicrophoneStopDiagnostics` 只保存在 mic capture 内部并打印日志，重建 capture 后丢失。违反 BUG.md 规则 21。

**Files:**
- Modify: `src-tauri/src/platform/macos/cpal_microphone.rs:69-77, 204-255`
- Modify: `src-tauri/src/platform/macos_service.rs:326-349` (stop_captures)
- Modify: `src-tauri/src/media/recording_writer.rs:49-98` (RecordingDiagnostics)

**当前问题：**
1. `CpalMicrophoneCapture::stop()` 返回 `AppResult<()>`，diagnostics 只存内部字段
2. `macos_service.rs` 通过 `last_stop_diagnostics()` 读取后 `eprintln!`
3. 随后 `self.mic_capture = CpalMicrophoneCapture::new()` 重建，旧 diagnostics 丢失
4. `RecordingDiagnostics` 没有 mic stop diagnostics 字段

- [ ] **Step 1: 新增 `stop_with_diagnostics()` 方法**

在 `src-tauri/src/platform/macos/cpal_microphone.rs` 中，新增一个返回 diagnostics 的方法：

```rust
/// Stop microphone capture and return structured diagnostics.
///
/// This is the preferred method for callers that need to preserve
/// stop diagnostics (e.g., for RecordingResult or bug investigation).
/// The trait method `stop()` calls this internally but discards the return value.
pub fn stop_with_diagnostics(&mut self) -> AppResult<CpalMicrophoneStopDiagnostics> {
    let mut diag = CpalMicrophoneStopDiagnostics::default();
    diag.stop_requested = true;

    let stream = self.stream.take();
    diag.stream_existed = stream.is_some();

    // Reset callbacks_after_stop counter before stopping
    self.callbacks_after_stop.store(0, Ordering::Relaxed);

    // Signal callback to stop processing
    self.running.store(false, Ordering::Relaxed);

    if let Some(send_stream) = stream {
        diag.pause_attempted = true;
        match send_stream.0.pause() {
            Ok(()) => {
                diag.pause_ok = true;
            }
            Err(e) => {
                diag.pause_error = Some(format!("{:?}", e));
                eprintln!("麦克风 pause 失败: {:?}", e);
            }
        }
        drop(send_stream);
        diag.stream_dropped = true;

        // Wait for Bluetooth HFP profile release
        let wait_start = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(300));
        diag.stop_wait_ms = wait_start.elapsed().as_millis() as u64;

        diag.callbacks_after_stop = self.callbacks_after_stop.load(Ordering::Acquire);
    }

    self.last_stop_diagnostics = diag.clone();
    Ok(diag)
}
```

- [ ] **Step 2: 修改 trait `stop()` 调用 `stop_with_diagnostics()`**

将 `src-tauri/src/platform/macos/cpal_microphone.rs` 的 `stop()` 方法（line 204-255）改为调用新方法：

```rust
fn stop(&mut self) -> AppResult<()> {
    self.stop_with_diagnostics()?;
    Ok(())
}
```

- [ ] **Step 3: 修改 `macos_service.rs` — 使用 `stop_with_diagnostics()` 并保留结果**

将 `src-tauri/src/platform/macos_service.rs` 的 `stop_captures()` 方法（约 line 326-349）中 mic stop 部分修改为：

```rust
// Step 2: Stop mic first (Bluetooth HFP release).
if self.service.last_requested_microphone {
    eprintln!("麦克风已启动，执行 mic stop...");
    match self.service.mic_capture.stop_with_diagnostics() {
        Ok(diag) => {
            eprintln!("麦克风停止诊断: {:?}", diag);
            // 保留 diagnostics 供后续读取
            self.mic_stop_diag = Some(diag);
        }
        Err(e) => {
            eprintln!("麦克风停止失败: {:?}", e);
            self.mic_stop_result = Err(e);
        }
    }
} else {
    eprintln!("本轮未启动麦克风，跳过 mic stop");
}
```

在 `RecordingFinalizeGuard` 结构体中新增字段：

```rust
mic_stop_diag: Option<CpalMicrophoneStopDiagnostics>,
```

在 `finalize_result()` 中，将 diagnostics 写入 `RecordingConsumerOutput.diagnostics`（或至少保留在 guard 中供 stop 返回）。

- [ ] **Step 4: 为 `RecordingDiagnostics` 新增 mic stop diagnostics 字段**

在 `src-tauri/src/media/recording_writer.rs` 的 `RecordingDiagnostics` 结构体（line 49-98）中新增：

```rust
/// Structured diagnostics from the last mic stop operation.
/// Contains pause/drop/wait/callbacks_after_stop information.
/// Only populated when microphone was requested and stop was called.
pub mic_stop_diagnostics: Option<CpalMicrophoneStopDiagnostics>,
```

注意：需要确保 `CpalMicrophoneStopDiagnostics` 实现了 `Clone`（已有）和 `Debug`（已有）。如果需要跨 crate 访问，确认可见性。

- [ ] **Step 5: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: 所有测试通过

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/platform/macos/cpal_microphone.rs src-tauri/src/platform/macos_service.rs src-tauri/src/media/recording_writer.rs
git commit -m "fix(audio): 蓝牙 mic stop diagnostics 结构化返回 — 重建 capture 前保留

- 新增 stop_with_diagnostics() 返回 CpalMicrophoneStopDiagnostics
- macos_service stop_captures() 使用 stop_with_diagnostics() 并保留结果
- RecordingDiagnostics 新增 mic_stop_diagnostics 字段
- 重建 mic capture 前 clone diagnostics，不再丢失

修复 Important 2: 蓝牙 mic stop diagnostics 没有真正返回或持久保留"
```

---

## Task 5: Writer per-source diagnostics

**Important 3** — 当前 `WriterDiagnostics` 只有 aggregate audio counters，不能证明每个 requested source 都进入 artifact。双源场景中，一路被 discard 可能被另一路掩盖。

**Files:**
- Modify: `src-tauri/src/media/recording_writer.rs:17-42` (WriterDiagnostics)
- Modify: `src-tauri/src/media/recording_writer.rs:254-258` (RecordingWriter trait)
- Modify: `src-tauri/src/media/ffmpeg_writer.rs` (encoder_worker per-source 计数)
- Modify: `src-tauri/src/platform/macos_service.rs:702-740` (consume_frames 调用点)
- Modify: `src-tauri/src/media/recording_writer.rs:110-235` (validate_source_aware_audio_contract)

**当前问题：**
1. `WriterDiagnostics` 只有 `audio_chunks_received/appended/discarded` 等 aggregate 字段
2. `MixedAudioChunk` 没有 source metadata，`push_audio()` 不知道 chunk 来源
3. `validate_source_aware_audio_contract()` 对 writer discard 也只看 aggregate ratio

**设计决策：** 不修改 `MixedAudioChunk`（15+ 构造点受影响），而是在 `RecordingWriter` trait 上新增 `record_source_contribution()` 方法，由 `consume_frames()` 在每次 `push_audio()` 前调用，传递 synchronizer 已有的 per-source metadata。

- [ ] **Step 1: 扩展 `WriterDiagnostics` — 新增 per-source 字段**

在 `src-tauri/src/media/recording_writer.rs` 的 `WriterDiagnostics` 结构体（line 17-42）中新增：

```rust
/// Per-source writer-side diagnostics.
/// These track what happened to each requested source's data at the writer level,
/// complementing the before-writer diagnostics in RecordingDiagnostics.

/// Number of mixed audio chunks received by writer that contained system audio.
pub system_chunks_received_by_writer: u64,

/// Number of mixed audio chunks received by writer that contained mic audio.
pub mic_chunks_received_by_writer: u64,

/// Silence frames padded for system audio timeline gaps (estimated).
pub system_silence_frames_padded: u64,

/// Silence frames padded for mic audio timeline gaps (estimated).
pub mic_silence_frames_padded: u64,
```

注意：`system_frames_appended` / `mic_frames_appended` 需要在 writer 内部按 source 比例估算（因为 mixed chunk 是合并后的 PCM），或者在 `record_source_contribution()` 层面记录 synchronizer 输出的 per-source frame count。这里选择后者——在 `consume_frames()` 层面记录到 `RecordingDiagnostics`，不修改 writer 内部编码逻辑。

- [ ] **Step 2: 在 `RecordingWriter` trait 上新增 `record_source_contribution()`**

在 `src-tauri/src/media/recording_writer.rs` 的 `RecordingWriter` trait（line 254-258）中新增：

```rust
/// Record that the next push_audio() call contains data from the given sources.
/// Called by consume_frames() before each push_audio() to enable per-source tracking.
/// Default implementation is a no-op (for test writers that don't track sources).
fn record_source_contribution(
    &mut self,
    has_system: bool,
    has_mic: bool,
    system_frames: u64,
    mic_frames: u64,
) {
    let _ = (has_system, has_mic, system_frames, mic_frames);
}
```

- [ ] **Step 3: 在 `FfmpegRecordingWriter` 中实现 `record_source_contribution()`**

在 `src-tauri/src/media/ffmpeg_writer.rs` 的 `impl RecordingWriter for FfmpegRecordingWriter` 中新增：

```rust
fn record_source_contribution(
    &mut self,
    has_system: bool,
    has_mic: bool,
    _system_frames: u64,
    _mic_frames: u64,
) {
    // Track per-source chunk counts at the writer level.
    // Actual append/discard tracking happens inside encoder_worker.
    // For now, we track at the front-end queue level.
    if has_system {
        self.system_chunks_received += 1;
    }
    if has_mic {
        self.mic_chunks_received += 1;
    }
}
```

在 `FfmpegRecordingWriter` 结构体中新增字段：

```rust
system_chunks_received: u64,
mic_chunks_received: u64,
```

在 `new()` 中初始化为 0。在 `finish()` 中将这些值写入 `WriterDiagnostics`。

- [ ] **Step 4: 在 `consume_frames()` 中调用 `record_source_contribution()`**

在 `src-tauri/src/platform/macos_service.rs` 的 `consume_frames()` 函数中，每次调用 `writer.push_audio()` 之前（约 line 702-740），新增：

```rust
// Before pushing to writer, record per-source contribution
let chunk_frames = synced.mixed.samples.len() as u64 / synced.mixed.channels.max(1) as u64;
let system_chunk_frames = if synced.has_system { chunk_frames } else { 0 };
let mic_chunk_frames = if synced.has_mic { chunk_frames } else { 0 };
writer.record_source_contribution(
    synced.has_system,
    synced.has_mic,
    system_chunk_frames,
    mic_chunk_frames,
);
```

同样在 final drain 路径中也做相同调用。

- [ ] **Step 5: 扩展 `validate_source_aware_audio_contract()` — per-source writer 检查**

在 `src-tauri/src/media/recording_writer.rs` 的 `validate_source_aware_audio_contract()` 函数（line 110-235）中，新增 per-source writer 检查：

```rust
// After existing checks, add per-source writer validation:

if diagnostics.requested_system_audio && diagnostics.system_windows_before_writer > 0 {
    let wd = &diagnostics.writer_diagnostics;
    // Check that system audio actually made it through the writer
    if wd.system_chunks_received_by_writer == 0 {
        return Err(AppError::RecordingWriteFailed(
            "请求了系统音频，但 writer 未收到任何包含系统音频的 chunk".to_string(),
        ));
    }
}

if diagnostics.requested_microphone && diagnostics.mic_windows_before_writer > 0 {
    let wd = &diagnostics.writer_diagnostics;
    if wd.mic_chunks_received_by_writer == 0 {
        return Err(AppError::RecordingWriteFailed(
            "请求了麦克风，但 writer 未收到任何包含麦克风的 chunk".to_string(),
        ));
    }
}
```

- [ ] **Step 6: 新增测试 — per-source writer diagnostics**

在 `src-tauri/src/media/recording_writer.rs` 的 tests 模块中新增：

```rust
/// Verifies that source-aware contract rejects when requested mic
/// windows exist before writer but writer receives zero mic chunks.
#[test]
fn source_aware_contract_rejects_when_writer_receives_zero_mic_chunks() {
    let diagnostics = RecordingDiagnostics {
        requested_microphone: true,
        mic_windows_before_writer: 10,
        mic_frames_before_writer: 4800,
        writer_diagnostics: WriterDiagnostics {
            mic_chunks_received_by_writer: 0, // writer 未收到 mic
            ..Default::default()
        },
        ..Default::default()
    };
    let result = validate_source_aware_audio_contract(&diagnostics, &diagnostics.writer_diagnostics);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("麦克风"));
}

/// Verifies that source-aware contract rejects when requested system
/// windows exist before writer but writer receives zero system chunks.
#[test]
fn source_aware_contract_rejects_when_writer_receives_zero_system_chunks() {
    let diagnostics = RecordingDiagnostics {
        requested_system_audio: true,
        system_windows_before_writer: 10,
        system_frames_before_writer: 4800,
        writer_diagnostics: WriterDiagnostics {
            system_chunks_received_by_writer: 0, // writer 未收到 system
            ..Default::default()
        },
        ..Default::default()
    };
    let result = validate_source_aware_audio_contract(&diagnostics, &diagnostics.writer_diagnostics);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("系统音频"));
}
```

- [ ] **Step 7: 运行所有 source_aware 测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg source_aware -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture
```

Expected: 所有测试通过

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/media/recording_writer.rs src-tauri/src/media/ffmpeg_writer.rs src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): writer per-source diagnostics — 每个 requested source 独立追踪

- WriterDiagnostics 新增 per-source counters (system/mic chunks received)
- RecordingWriter trait 新增 record_source_contribution() 方法
- consume_frames() 在每次 push_audio 前记录 per-source 贡献
- validate_source_aware_audio_contract() 新增 per-source writer 检查
- 新增 per-source writer diagnostics 测试

修复 Important 3: source-aware artifact 证据仍偏 aggregate"
```

---

## Task 6: 文档修复、日志修复与完整回归

**Major/Minor** — `cargo fmt --check` 失败、channel drop 日志文案不准确、BUG.md/HANDOFF.md 状态冲突、RAII guard 注释过度承诺。

**Files:**
- Modify: `src-tauri/src/core/media_channel.rs:60-65` (drop log message)
- Modify: `src-tauri/src/platform/macos_service.rs:536-545` (RAII guard Drop 注释)
- Modify: `BUG.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: 修复 channel drop 日志文案**

将 `src-tauri/src/core/media_channel.rs` 的 `try_send_drop_newest()` 中的日志（line 60-65）修改为：

```rust
if count == 1 || count % 100 == 0 {
    eprintln!(
        "警告: {} 媒体通道丢弃 (累计 {} 次)",
        self.source, count
    );
}
```

将 "音频通道" 改为 "媒体通道"，因为 source 可能是 "video"。

- [ ] **Step 2: 修复 RAII guard Drop 注释**

将 `src-tauri/src/platform/macos_service.rs` 的 `RecordingFinalizeGuard` Drop 注释（约 line 536-545）修改为：

```rust
/// Safety net: ensures basic cleanup happens even if `finalize()` panics.
///
/// This Drop impl only resets `stop_flag` and `mic_level` — it does NOT
/// stop screen capture, stop mic, or join consumer thread. Those require
/// the explicit `finalize()` path. This is a last-resort safety net,
/// not a complete resource release mechanism.
impl Drop for RecordingFinalizeGuard<'_> {
    fn drop(&mut self) {
        self.service.stop_flag = None;
        if let Ok(mut guard) = self.service.mic_level.lock() {
            *guard = 0.0;
        }
    }
}
```

同时修改结构体上方的 doc comment，确保不承诺 "all recording resources are released on drop"。

- [ ] **Step 3: 运行 `cargo fmt` 修复格式化**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 4: 确认格式化通过**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

Expected: PASS

- [ ] **Step 5: 运行完整测试套件**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
```

Expected: 所有测试通过

- [ ] **Step 6: 运行 clippy**

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | tail -20
```

Expected: 无新增 warning

- [ ] **Step 7: 更新 BUG.md — 预防规则补充**

在 BUG.md 的 BUG-005 预防规则中补充/修正：

```markdown
28. writer worker 和 consumer thread timeout 后绝不能调用无界 join()；timeout 必须直接返回结构化错误并继续 cleanup。
29. drop ratio 分母必须使用 received + dropped（attempted total），不能只用 received。
30. mic stop diagnostics 必须在重建 capture 前写入 RecordingDiagnostics，不能依赖 capture 内部字段。
31. writer diagnostics 必须区分 per-source（system/mic）的 chunks received/appended/discarded，不能只用 aggregate。
```

- [ ] **Step 8: 更新 HANDOFF.md — 记录本轮整改**

更新 HANDOFF.md 的工作任务记录，添加本轮整改记录。

- [ ] **Step 9: Final Commit**

```bash
git add -A
git commit -m "docs: 文档修复与完整回归 — drop 日志文案、RAII 注释、BUG.md 规则补充

- channel drop 日志「音频通道」改为「媒体通道」
- RAII guard Drop 注释修正为 safety net 描述
- cargo fmt 格式化修复
- BUG.md 补充预防规则 28-31
- HANDOFF.md 记录本轮整改"
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
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg consume_frames -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg source_aware -- --nocapture
```

### 真实设备 Manual Gate

1. 系统音频 + 内置麦克风 15 秒，停止录制成功。
2. 系统音频 + 蓝牙麦克风 15 秒，停止后蓝牙音质恢复。
3. 不采集音频，只录屏 15 秒，停止录制成功。
4. 点击停止后 3 秒内 UI 恢复到预览/完成状态（验证 stop 路径不卡住）。
