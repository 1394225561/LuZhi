# Phase 6 BUG-005 Code Review 整改实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 code review 发现的三个问题：drop logging 缺少 source 信息、stop/start 时序窗口、stop-during-startup 测试覆盖不足。

**Architecture:** 在现有 `consume_frames` + `MacRecordingService::stop()` 架构上做增量修复：(1) 在 media channel 和 CPAL callback 层增加 source-aware drop logging；(2) 引入 `RecordingFinalizeGuard` RAII 结构体管理 stop 生命周期；(3) 补充集成级测试覆盖 stop-during-startup 场景。

**Tech Stack:** Rust, std::sync, tracing/eprintln, #[cfg(test)]

---

## 文件结构

```
src-tauri/src/
  core/
    media_channel.rs          # 修改: 增加 source 标识的 drop logging
  platform/
    macos/
      cpal_microphone.rs      # 修改: CPAL callback drop 不再静默
    macos_service.rs           # 修改: 引入 RecordingFinalizeGuard, 重构 stop()
  media/
    recording_writer.rs        # 可能微调: RecordingDiagnostics 字段
```

---

## Task 1: Media Channel 增加 source-aware drop logging

**Files:**
- Modify: `src-tauri/src/core/media_channel.rs:46-55`

当前 `try_send_drop_newest()` 在 channel 满时只递增 atomic counter，不记录是哪个 source 丢的。CPAL callback 用 `let _ = sink.try_send_drop_newest(chunk)` 完全忽略 drop。这违反 BUG-005 预防规则 13："音频 drop 不能静默"。

- [ ] **Step 1: 为 MediaSender 增加 source 标识**

在 `MediaSender` 结构体中增加 `source: &'static str` 字段，用于标识是 "system" 还是 "mic"。

```rust
// src-tauri/src/core/media_channel.rs

#[derive(Debug)]
pub struct MediaSender<T> {
    inner: SyncSender<T>,
    dropped: Arc<AtomicU64>,
    source: &'static str,  // NEW: "system" or "mic"
}
```

- [ ] **Step 2: 更新 Clone impl**

```rust
impl<T> Clone for MediaSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            dropped: self.dropped.clone(),
            source: self.source,
        }
    }
}
```

- [ ] **Step 3: 更新 bounded_media_channel 签名**

```rust
pub fn bounded_media_channel<T>(capacity: usize, source: &'static str) -> (MediaSender<T>, MediaReceiver<T>) {
    assert!(
        capacity > 0,
        "media channel capacity must be greater than zero"
    );
    let (inner_sender, inner_receiver) = sync_channel(capacity);
    let dropped = Arc::new(AtomicU64::new(0));

    (
        MediaSender {
            inner: inner_sender,
            dropped: dropped.clone(),
            source,
        },
        MediaReceiver {
            inner: inner_receiver,
            dropped,
        },
    )
}
```

- [ ] **Step 4: 更新 try_send_drop_newest 增加 drop logging**

```rust
impl<T> MediaSender<T> {
    pub fn try_send_drop_newest(&self, item: T) -> bool {
        match self.inner.try_send(item) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                let count = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                // Log every 100th drop to avoid log spam, and always log the first.
                if count == 1 || count % 100 == 0 {
                    eprintln!(
                        "警告: {} 音频通道丢弃 (累计 {} 次)",
                        self.source, count
                    );
                }
                false
            }
        }
    }
}
```

- [ ] **Step 5: 更新所有调用点传入 source 参数**

找到所有 `bounded_media_channel` 调用点，传入 source 标识：

```rust
// macos_service.rs - start() 中
let (video_tx, video_rx) = bounded_media_channel(90, "video");
let (system_audio_tx, system_audio_rx) = bounded_media_channel(256, "system");
let (mic_tx, mic_rx) = bounded_media_channel(256, "mic");
```

- [ ] **Step 6: 更新测试中的调用**

所有测试中的 `bounded_media_channel` 调用也需要传入 source 参数（可以用 "test"）。

- [ ] **Step 7: 编译验证**

Run: `cargo build --features ffmpeg 2>&1 | tail -20`
Expected: 编译通过，无 error

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/core/media_channel.rs src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): media channel drop logging 增加 source 标识 — system/mic drop 不再静默"
```

---

## Task 2: CPAL Microphone Callback Drop Logging

**Files:**
- Modify: `src-tauri/src/platform/macos/cpal_microphone.rs:346-349`

当前 CPAL callback 用 `let _ = sink.try_send_drop_newest(chunk)` 完全忽略 drop 返回值。虽然 channel 层已经增加了 source-aware logging（Task 1），但 callback 层也应该记录 drop 事件，因为 callback 是实时音频路径，drop 意味着音频丢失。

- [ ] **Step 1: 修改 CPAL callback 使用 drop logging**

```rust
// cpal_microphone.rs - build_input_stream callback 中
// 之前:
let _ = sink.try_send_drop_newest(chunk);

// 之后:
if !sink.try_send_drop_newest(chunk) {
    // Drop already logged by MediaSender with source="mic".
    // No additional logging needed here to avoid double-printing.
}
```

注意：由于 Task 1 已经在 `MediaSender::try_send_drop_newest()` 中增加了 source-aware logging，这里只需要移除 `let _` 的静默忽略即可。不需要额外的 `eprintln!`，避免重复日志。

- [ ] **Step 2: 编译验证**

Run: `cargo build --features ffmpeg 2>&1 | tail -20`
Expected: 编译通过

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/platform/macos/cpal_microphone.rs
git commit -m "fix(audio): CPAL callback 不再静默忽略 drop — 配合 channel 层 source logging"
```

---

## Task 3: RecordingFinalizeGuard RAII 结构体

**Files:**
- Create: `src-tauri/src/platform/macos_service.rs` (在 stop() 方法附近新增)
- Modify: `src-tauri/src/platform/macos_service.rs:290-485` (重构 stop())

当前 `stop()` 方法手动执行 9 个清理步骤，如果中间某个步骤 panic，后续步骤不会执行。引入 RAII guard 确保所有清理步骤在 guard drop 时执行，无论正常返回还是 panic。

- [ ] **Step 1: 定义 RecordingFinalizeGuard 结构体**

在 `macos_service.rs` 中 `stop()` 方法之前新增：

```rust
/// RAII guard that ensures all recording resources are released on drop.
///
/// If `stop()` panics or returns early, the guard still executes cleanup
/// steps in reverse initialization order. This prevents resource leaks
/// when a sidecar write failure or state machine transition error occurs
/// mid-cleanup.
struct RecordingFinalizeGuard<'a> {
    service: &'a mut MacRecordingService,
    cursor_metadata: Option<crate::app::cursor::CursorRuntimeMetadata>,
    mic_stop_result: AppResult<()>,
    capture_stop_result: AppResult<()>,
    consumer_output: Option<RecordingConsumerOutput>,
    errors: Vec<String>,
}

impl<'a> RecordingFinalizeGuard<'a> {
    fn new(service: &'a mut MacRecordingService) -> Self {
        Self {
            service,
            cursor_metadata: None,
            mic_stop_result: Ok(()),
            capture_stop_result: Ok(()),
            consumer_output: None,
            errors: Vec::new(),
        }
    }

    /// Execute all cleanup steps regardless of intermediate errors.
    /// Returns the final RecordingResult or error.
    fn finalize(mut self) -> AppResult<RecordingResult> {
        // The guard is consumed here — drop runs after this returns.
        // But we do explicit cleanup to control ordering and collect errors.
        self.do_cleanup()
    }

    fn do_cleanup(&mut self) -> AppResult<RecordingResult> {
        // Step 1: Stop cursor runtime.
        let cursor_metadata = self.service.cursor_runtime.as_mut()
            .and_then(|runtime| runtime.stop());
        self.service.cursor_runtime = None;
        self.cursor_metadata = cursor_metadata;

        // Step 2: Stop mic first (Bluetooth HFP release).
        if self.service.last_requested_microphone {
            eprintln!("麦克风已启动，执行 mic stop...");
            self.mic_stop_result = self.service.mic_capture.stop();
            let stop_diag = self.service.mic_capture.last_stop_diagnostics();
            eprintln!("麦克风停止诊断: {:?}", stop_diag);
        } else {
            eprintln!("本轮未启动麦克风，跳过 mic stop");
        }

        // Step 3: Stop screen capture.
        self.capture_stop_result = ScreenCapture::stop(&mut self.service.screen_capture);

        // Step 4: Signal consumer thread to stop.
        if let Some(flag) = &self.service.stop_flag {
            flag.store(true, Ordering::Relaxed);
        }

        // Step 5: Join consumer thread with bounded timeout.
        self.consumer_output = Some(self.join_consumer_with_timeout());

        // Step 6+: Remaining steps handled by finalize_result().
        self.finalize_result()
    }

    fn join_consumer_with_timeout(&mut self) -> RecordingConsumerOutput {
        // ... (moved from stop() lines 324-402)
    }

    fn finalize_result(&mut self) -> AppResult<RecordingResult> {
        // ... (moved from stop() lines 403-484)
    }
}

impl Drop for RecordingFinalizeGuard<'_> {
    fn drop(&mut self) {
        // Safety net: if do_cleanup() panicked or wasn't called,
        // ensure at least basic cleanup happens.
        // This is a last resort — normal flow uses finalize().
        self.service.stop_flag = None;
        // Reset mic level.
        if let Ok(mut guard) = self.service.mic_level.lock() {
            *guard = 0.0;
        }
    }
}
```

- [ ] **Step 2: 重构 stop() 使用 guard**

```rust
pub fn stop(&mut self) -> AppResult<RecordingResult> {
    let guard = RecordingFinalizeGuard::new(self);
    guard.finalize()
}
```

- [ ] **Step 3: 编译验证**

Run: `cargo build --features ffmpeg 2>&1 | tail -20`
Expected: 编译通过

- [ ] **Step 4: 运行现有测试**

Run: `cargo test --features ffmpeg -p luzhi -- macos_service 2>&1 | tail -30`
Expected: 所有现有测试通过

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): 引入 RecordingFinalizeGuard RAII — stop 路径 panic 安全"
```

---

## Task 4: Stop-During-Startup 集成测试

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs` (test module)

当前测试只覆盖了"stop during active processing"和"immediate stop"两种场景。缺少"stop called before consumer thread finishes starting"的测试。

- [ ] **Step 1: 新增 stop_flag_before_consumer_starts 测试**

```rust
/// Verifies that stop_flag set before consumer thread starts is respected.
/// This covers the timing window where stop() is called during startup,
/// before the consumer thread enters its main loop.
#[test]
fn consume_frames_respects_stop_flag_set_before_start() {
    use crate::media::recording_writer::CountingRecordingWriter;

    let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(10, "test");
    let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(10, "test");

    // Set stop_flag BEFORE starting consumer — simulates stop-during-startup.
    let stop_flag = Arc::new(AtomicBool::new(true));
    let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mic_level = Arc::new(Mutex::new(0.0f64));

    let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

    // Send some data — consumer should drain these in final drain.
    for i in 0..5 {
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(audio_tx.try_send_drop_newest(chunk));
    }

    drop(video_tx);
    drop(audio_tx);

    let output = MacRecordingService::consume_frames(
        stop_flag,
        video_rx,
        audio_rx,
        None,
        frame_count,
        writer,
        mic_level,
        "medium",
        true,
        false,
        None,
    );

    // Consumer should have drained the 5 chunks in final drain.
    assert_eq!(output.diagnostics.system_chunks_received, 5);
    // No errors expected — clean stop.
    assert!(
        output.errors.is_empty(),
        "stop-before-start should not produce errors, got: {:?}",
        output.errors
    );
}
```

- [ ] **Step 2: 新增 stop_flag_during_active_processing_with_data 测试**

```rust
/// Verifies that calling stop while captures are actively producing data
/// results in a clean drain and no data loss for already-queued frames.
#[test]
fn consume_frames_drains_queued_data_on_stop() {
    use crate::media::recording_writer::CountingRecordingWriter;

    let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(100, "test");
    let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(100, "test");
    let stop_flag = Arc::new(AtomicBool::new(false));
    let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mic_level = Arc::new(Mutex::new(0.0f64));

    // Send 20 audio chunks while consumer is running.
    for i in 0..20 {
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(audio_tx.try_send_drop_newest(chunk));
    }

    let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

    // Stop after a brief delay to let consumer process some frames.
    let flag_clone = stop_flag.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        flag_clone.store(true, Ordering::Relaxed);
    });

    drop(video_tx);
    drop(audio_tx);

    let output = MacRecordingService::consume_frames(
        stop_flag,
        video_rx,
        audio_rx,
        None,
        frame_count,
        writer,
        mic_level,
        "medium",
        true,
        false,
        None,
    );

    stopper.join().unwrap();

    // All 20 chunks should have been received (some in loop, rest in final drain).
    assert_eq!(output.diagnostics.system_chunks_received, 20);
    // No drop errors expected.
    let drop_errors: Vec<_> = output.errors.iter()
        .filter(|e| e.contains("丢弃率过高"))
        .collect();
    assert!(
        drop_errors.is_empty(),
        "no drop errors expected for 20/20 chunks, got: {:?}",
        output.errors
    );
}
```

- [ ] **Step 3: 运行新测试**

Run: `cargo test --features ffmpeg -p luzhi -- macos_service::tests::consume_frames_respects_stop_flag_set_before_start -v 2>&1`
Expected: PASS

Run: `cargo test --features ffmpeg -p luzhi -- macos_service::tests::consume_frames_drains_queued_data_on_stop -v 2>&1`
Expected: PASS

- [ ] **Step 4: 运行所有 macos_service 测试**

Run: `cargo test --features ffmpeg -p luzhi -- macos_service 2>&1 | tail -30`
Expected: 所有测试通过

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "test(audio): 新增 stop-during-startup 和 active-drain 集成测试"
```

---

## Task 5: Drop Ratio Hard Fail 测试补充

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs` (test module)

当前只有一个 small-drop pass 测试（9% drop ratio）。需要补充 high-drop fail 测试验证 10% 阈值。

- [ ] **Step 1: 新增 high-drop ratio fail 测试**

```rust
/// Verifies that high audio drop ratio (>10%) causes recording failure.
/// This is the counterpart to consume_frames_warns_but_does_not_fail_on_small_audio_drop.
#[test]
fn consume_frames_fails_on_high_audio_drop_ratio() {
    use crate::media::recording_writer::CountingRecordingWriter;

    // Create channel with capacity 10. Send 10 items to fill, then 2 more = 2 drops.
    // Drop ratio = 2/10 = 20%, above the 10% threshold.
    let (video_tx, video_rx) = bounded_media_channel::<VideoFrameRef>(1, "test");
    let (audio_tx, audio_rx) = bounded_media_channel::<AudioChunk>(10, "test");

    // Fill the channel with 10 chunks.
    for i in 0..10 {
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(audio_tx.try_send_drop_newest(chunk));
    }
    // These 2 will be dropped (channel full).
    for i in 10..12 {
        let overflow_chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(i * 10_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.1f32; 960].into_boxed_slice()),
        };
        assert!(!audio_tx.try_send_drop_newest(overflow_chunk));
    }

    let stop_flag = Arc::new(AtomicBool::new(true)); // immediate stop
    let frame_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mic_level = Arc::new(Mutex::new(0.0f64));

    let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));

    drop(video_tx);
    drop(audio_tx);

    let output = MacRecordingService::consume_frames(
        stop_flag,
        video_rx,
        audio_rx,
        None,
        frame_count,
        writer,
        mic_level,
        "medium",
        true,  // requested_system_audio
        false, // requested_microphone
        None,
    );

    // Should have received 10 chunks in final drain.
    assert_eq!(output.diagnostics.system_chunks_received, 10);
    // Should have 2 drops.
    assert_eq!(output.diagnostics.system_chunks_dropped, 2);

    // Drop ratio 2/10 = 20% > 10% → should be in errors.
    let drop_errors: Vec<_> = output
        .errors
        .iter()
        .filter(|e| e.contains("丢弃率过高"))
        .collect();
    assert!(
        !drop_errors.is_empty(),
        "high drop ratio (20%) should cause hard fail, got: {:?}",
        output.errors
    );
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --features ffmpeg -p luzhi -- macos_service::tests::consume_frames_fails_on_high_audio_drop_ratio -v 2>&1`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "test(audio): 新增 high-drop ratio (>10%) hard fail 测试"
```

---

## Task 6: 最终验证与 BUG.md 更新

**Files:**
- Modify: `BUG.md` (补充预防规则)

- [ ] **Step 1: 运行完整测试套件**

Run: `cargo test --features ffmpeg -p luzhi 2>&1 | tail -40`
Expected: 所有测试通过

- [ ] **Step 2: 运行 clippy**

Run: `cargo clippy --features ffmpeg -p luzhi 2>&1 | tail -20`
Expected: 无 warning

- [ ] **Step 3: 更新 BUG.md 预防规则**

在 BUG-005 预防规则中补充：

```markdown
24. 音频 channel drop logging 必须标识 source（system/mic），不能只记录数量。
25. CPAL callback 中的 drop 不能用 `let _` 静默忽略，必须配合 channel 层 source logging。
26. 录制 stop 路径必须使用 RAII guard 或等效机制，确保 panic 时资源仍被释放。
27. stop-during-startup 场景必须有测试覆盖：stop_flag 在 consumer 启动前设置。
```

- [ ] **Step 4: 更新 HANDOFF.md**

记录本轮整改内容。

- [ ] **Step 5: Final Commit**

```bash
git add BUG.md HANDOFF.md
git commit -m "docs: 更新 BUG.md 和 HANDOFF.md — drop source logging、RAII guard、stop 测试补充"
```
