# Phase 6 BUG-005 整改后 Code Review 整改实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 `docs/superpowers/reviews/2026-06-02-phase-6-bug-005-post-rectification-code-review.md` 中记录的 4 个 Important 和 1 个 Minor 问题。

**Architecture:** 按 Review 建议的 Phase A-E 分阶段修复：bounded finalize、CPAL lazy offset sentinel、drop warning/error 分层、source-aware post-writer diagnostics、蓝牙 mic stop diagnostics。每个 Phase 独立可验证。

**Tech Stack:** Rust (cpal, ffmpeg-next), Tauri 2.0, tokio

---

## 文件结构

### 修改文件清单

| 文件 | 职责变更 |
|------|---------|
| `src-tauri/src/media/ffmpeg_writer.rs` | Phase A: worker result channel + bounded join |
| `src-tauri/src/platform/macos_service.rs` | Phase A: consumer bounded join; Phase C: drop warning 分层; Phase D: source-aware post-writer |
| `src-tauri/src/core/clock.rs` | Phase B: lazy offset sentinel |
| `src-tauri/src/platform/macos/cpal_microphone.rs` | Phase B: offset 初始化; Phase E: stop diagnostics |
| `src-tauri/src/media/recording_writer.rs` | Phase C: drop ratio 阈值; Phase D: diagnostics 字段扩展 |
| `BUG.md` | 更新预防规则 |

---

## Task 1: Phase B — CPAL lazy offset sentinel

**优先级最高**：此问题影响双源录制的时间戳对齐，且修复最简单、风险最低。

**Files:**
- Modify: `src-tauri/src/core/clock.rs:118-140`
- Modify: `src-tauri/src/platform/macos/cpal_microphone.rs:279`
- Test: `src-tauri/src/core/clock.rs` (existing tests section)

- [ ] **Step 1: 写失败测试 — zero offset 稳定性**

在 `src-tauri/src/core/clock.rs` 的 tests 模块中新增：

```rust
#[test]
fn audio_sample_clock_lazy_offset_zero_is_stable_after_second_callback() {
    use std::sync::atomic::Ordering;

    let clock = AudioSampleClock::new(48000, 2);
    let session = SessionClock::new();

    // Simulate: first callback at 5ms, buffer 10ms -> offset = 0
    // (5ms - 10ms saturates to 0)
    // The sentinel must accept 0 as a valid initialized value.
    clock.initialize_offset(0);
    assert!(clock.offset_initialized());
    assert_eq!(clock.session_offset_nanos.load(Ordering::Acquire), 0);

    // Second callback tries to set offset = 5ms — must be rejected (first-call wins)
    clock.initialize_offset(5_000_000);
    assert_eq!(clock.session_offset_nanos.load(Ordering::Acquire), 0);
}

#[test]
fn audio_sample_clock_lazy_offset_first_call_wins_even_when_zero() {
    use std::sync::atomic::Ordering;

    let clock = AudioSampleClock::new(48000, 2);

    // First offset is 0 — must be stored
    assert!(clock.initialize_offset(0));
    assert_eq!(clock.session_offset_nanos.load(Ordering::Acquire), 0);

    // Second call returns false (already initialized)
    assert!(!clock.initialize_offset(1000));
    assert_eq!(clock.session_offset_nanos.load(Ordering::Acquire), 0);
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml audio_sample_clock_lazy_offset_zero -- --nocapture
```

Expected: FAIL — `initialize_offset` 方法和 `offset_initialized()` 不存在，或 CAS 逻辑用 0 做 sentinel。

- [ ] **Step 3: 修改 `AudioSampleClock` — 增加 initialized flag**

在 `src-tauri/src/core/clock.rs` 中：

1. `AudioSampleClock` 新增字段：

```rust
offset_initialized: AtomicBool,
```

2. `new()` 中初始化：

```rust
offset_initialized: AtomicBool::new(false),
```

3. `initialize_offset()` 改为使用 `AtomicBool` 做初始化守卫，不再用 `0` 做 sentinel：

```rust
pub fn initialize_offset(&self, offset_nanos: u64) -> bool {
    if self
        .offset_initialized
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        self.session_offset_nanos.store(offset_nanos, Ordering::Release);
        true
    } else {
        false
    }
}
```

4. 新增查询方法：

```rust
pub fn offset_initialized(&self) -> bool {
    self.offset_initialized.load(Ordering::Acquire)
}
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml audio_sample_clock -- --nocapture
```

Expected: PASS（包括新增的 2 个和既有的 2 个）。

- [ ] **Step 5: 更新 `cpal_microphone.rs` — 适配新 API**

确认 `build_input_stream()` 中调用 `initialize_offset()` 的逻辑兼容新签名（返回 `bool` 语义不变）。无需修改调用方式。

- [ ] **Step 6: 完整回归**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: 294+ tests PASS。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/core/clock.rs src-tauri/src/platform/macos/cpal_microphone.rs
git commit -m "fix(core): CPAL lazy offset sentinel — 使用 AtomicBool 替代 0 做未初始化标记

- AudioSampleClock 新增 offset_initialized: AtomicBool
- initialize_offset() 使用 CAS(AtomicBool) 做 first-call-wins
- 修复 offset 合法为 0 时被后续 callback 重新初始化的边界问题
- 新增 2 个 zero-offset 回归测试

修复 Important 2: CPAL lazy offset 用 0 同时表示未初始化和合法 offset"
```

---

## Task 2: Phase C — drop warning 与 hard failure 分层

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs:813-821`
- Modify: `src-tauri/src/media/recording_writer.rs` (RecordingDiagnostics)
- Test: `src-tauri/src/platform/macos_service.rs` (existing tests section)

- [ ] **Step 1: 写失败测试 — small drop 不导致 stop 失败**

在 `src-tauri/src/platform/macos_service.rs` 的 tests 模块中新增：

```rust
#[test]
fn consume_frames_warns_but_does_not_fail_on_small_audio_drop_when_contract_passes() {
    // 构造 RecordingDiagnostics: requested system audio, 有少量 drop (2/640 = 0.3%)
    // 但 artifact contract 通过、source-aware contract 通过
    // 验证: stop 不返回 RecordingFinalizeFailed
    // 验证: drop 信息进入 diagnostics 而非 errors
}
```

具体实现需要根据现有 test helper 构造。

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg consume_frames_warns -- --nocapture
```

Expected: FAIL。

- [ ] **Step 3: 修改 `macos_service.rs` — drop 降级为 warning**

在 `consume_frames()` 中，将 system/mic chunk drop 从 `errors.push()` 改为仅写入 diagnostics + `eprintln!`：

```rust
// 当前代码 (约 line 813):
if diagnostics.system_chunks_dropped > 0 {
    errors.push(format!("警告: 系统音频通道丢弃了 {} 个音频块", diagnostics.system_chunks_dropped));
}

// 改为:
if diagnostics.system_chunks_dropped > 0 {
    eprintln!(
        "警告: 系统音频通道丢弃了 {} 个音频块 (总计 {}，丢弃率 {:.1}%)",
        diagnostics.system_chunks_dropped,
        diagnostics.system_chunks_received,
        (diagnostics.system_chunks_dropped as f64 / diagnostics.system_chunks_received.max(1) as f64) * 100.0,
    );
}
// mic 同理
```

- [ ] **Step 4: 新增 drop ratio hard fail 条件**

在 artifact validation 之后、errors 收集之前，增加基于 drop ratio 的 hard fail：

```rust
const AUDIO_DROP_RATIO_HARD_FAIL: f64 = 0.10; // 10%

let system_drop_ratio = if diagnostics.system_chunks_received > 0 {
    diagnostics.system_chunks_dropped as f64 / diagnostics.system_chunks_received as f64
} else {
    0.0
};
let mic_drop_ratio = if diagnostics.mic_chunks_received > 0 {
    diagnostics.mic_chunks_dropped as f64 / diagnostics.mic_chunks_received as f64
} else {
    0.0
};

if system_drop_ratio > AUDIO_DROP_RATIO_HARD_FAIL {
    errors.push(format!(
        "系统音频丢弃率过高 ({:.1}% > {:.1}%)",
        system_drop_ratio * 100.0,
        AUDIO_DROP_RATIO_HARD_FAIL * 100.0
    ));
}
if mic_drop_ratio > AUDIO_DROP_RATIO_HARD_FAIL {
    errors.push(format!(
        "麦克风音频丢弃率过高 ({:.1}% > {:.1}%)",
        mic_drop_ratio * 100.0,
        AUDIO_DROP_RATIO_HARD_FAIL * 100.0
    ));
}
```

- [ ] **Step 5: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg consume_frames -- --nocapture
```

Expected: PASS。

- [ ] **Step 6: 完整回归**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

Expected: 全部 PASS。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs src-tauri/src/media/recording_writer.rs
git commit -m "fix(audio): drop warning 与 hard failure 分层 — 少量 drop 不再导致 stop 失败

- system/mic chunk drop 降级为 eprintln warning + diagnostics
- 新增 10% drop ratio hard fail 阈值
- RecordingDiagnostics 新增 drop_ratio 字段
- 新增 small-drop pass 测试

修复 Important 3: requested audio channel drop 被当成 stop 失败"
```

---

## Task 3: Phase D — source-aware post-writer diagnostics 命名与证据链

**Files:**
- Modify: `src-tauri/src/media/recording_writer.rs` (RecordingDiagnostics 字段命名)
- Modify: `src-tauri/src/platform/macos_service.rs` (diagnostics 写入)
- Test: `src-tauri/src/media/recording_writer.rs`

- [ ] **Step 1: 写失败测试 — writer discard 覆盖 system 和 mic**

在 `src-tauri/src/media/recording_writer.rs` 的 tests 模块中新增：

```rust
#[test]
fn source_aware_contract_rejects_when_requested_mic_windows_are_all_discarded() {
    // 构造 diagnostics: requested mic, mic_windows_before_writer = 100
    // writer full_overlap_discard = 100 (全部被丢弃)
    // 验证 contract 返回 Err
}

#[test]
fn source_aware_contract_rejects_when_requested_system_windows_are_all_discarded() {
    // 对称覆盖 system 路径
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg source_aware_contract_rejects -- --nocapture
```

Expected: FAIL — 当前 discard ratio 只检查 mic 路径。

- [ ] **Step 3: 修改 `recording_writer.rs` — 对称化 discard 检查**

在 `validate_source_aware_audio_contract()` 中，将 writer discard ratio 检查对称覆盖 system 和 mic：

```rust
// 当前只在 requested_microphone 分支内检查 discard
// 改为: 在两个分支都检查

if diagnostics.requested_system_audio && diagnostics.system_windows_before_writer > 0 {
    let total_received = diagnostics.writer_diagnostics.audio_chunks_received.max(1) as f64;
    let discard_ratio = diagnostics.writer_diagnostics.audio_chunks_discarded_full_overlap as f64 / total_received;
    if discard_ratio > 0.5 {
        return Err(AppError::RecordingWriteFailed(format!(
            "系统音频 writer discard 比例过高 ({:.0}%)",
            discard_ratio * 100.0
        )));
    }
}
// mic 分支同理
```

- [ ] **Step 4: 更新字段命名文案**

将 `system_windows_before_writer` / `mic_windows_before_writer` 的注释/文档明确为 "synchronizer output before writer push" 语义，避免误读为 "confirmed in artifact"。

- [ ] **Step 5: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg source_aware -- --nocapture
```

Expected: PASS。

- [ ] **Step 6: 完整回归**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

Expected: 全部 PASS。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/media/recording_writer.rs src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): source-aware contract 对称化 discard 检查 — system 和 mic 都受保护

- writer discard ratio 检查对称覆盖 requested system 和 mic
- 明确 before_writer 字段语义为 synchronizer output before push
- 新增 system discard 测试

修复 Important 4: source-aware contract 证明力与 system/mic 对称性"
```

---

## Task 4: Phase A — 真正 bounded finalize

**这是最复杂的 Phase**，涉及 writer worker 和 consumer thread 的超时机制。

**Files:**
- Modify: `src-tauri/src/media/ffmpeg_writer.rs` (worker result channel)
- Modify: `src-tauri/src/platform/macos_service.rs` (consumer bounded join)
- Test: `src-tauri/src/media/ffmpeg_writer.rs`

- [ ] **Step 1: 写失败测试 — worker 超时返回错误**

在 `src-tauri/src/media/ffmpeg_writer.rs` 的 tests 模块中新增：

```rust
#[test]
fn ffmpeg_writer_finish_returns_error_when_worker_hangs() {
    // 构造一个不会处理 Flush 的 mock worker
    // 验证 finish() 在超时后返回 Err 而非无限阻塞
}
```

具体实现需要注入一个可控的 worker 或使用 timeout wrapper。

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_finish_returns_error -- --nocapture
```

Expected: FAIL — 当前 `join_worker()` 无界等待。

- [ ] **Step 3: 重构 writer worker result 返回**

在 `src-tauri/src/media/ffmpeg_writer.rs` 中：

1. worker 通过 `mpsc::Sender<Result<WriterDiagnostics, String>>` 回传结果。
2. `FfmpegRecordingWriter` 新增 `result_rx: mpsc::Receiver<Result<WriterDiagnostics, String>>`。
3. `join_worker()` 改为 bounded receive：

```rust
const WORKER_RESULT_TIMEOUT: Duration = Duration::from_secs(10);

fn join_worker(&mut self) -> AppResult<WriterDiagnostics> {
    // Drop sender to signal worker we're done
    self.tx.take();

    // Wait for result with timeout
    let result = self.result_rx.recv_timeout(WORKER_RESULT_TIMEOUT);
    match result {
        Ok(Ok(diagnostics)) => Ok(diagnostics),
        Ok(Err(e)) => Err(AppError::RecordingWriteFailed(e)),
        Err(_timeout) => {
            eprintln!("警告: FFmpeg worker 超时未返回结果 ({:?})", WORKER_RESULT_TIMEOUT);
            Err(AppError::RecordingWriteFailed("FFmpeg worker 超时".to_string()))
        }
    }
}
```

4. `encoder_worker()` 在完成时通过 channel 发送结果。
5. `finish()` 保持 bounded Flush retry，但 join 改用 channel receive。

- [ ] **Step 4: Consumer thread bounded join**

在 `src-tauri/src/platform/macos_service.rs` 的 `stop()` 中，对 consumer thread 的 join 增加超时：

```rust
const CONSUMER_JOIN_TIMEOUT: Duration = Duration::from_secs(15);

// 当前: let consumer_output = handle.join()...
// 改为: 使用 thread::park_timeout 或 result channel
```

如果 consumer thread 超时，记录诊断并返回结构化错误。

- [ ] **Step 5: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture
```

Expected: PASS。

- [ ] **Step 6: 完整回归**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

Expected: 全部 PASS。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/media/ffmpeg_writer.rs src-tauri/src/platform/macos_service.rs
git commit -m "fix(audio): 真正 bounded finalize — worker 和 consumer join 超时

- writer worker result 通过 channel 返回，join 改为 bounded receive (10s)
- consumer thread join 增加超时 (15s)
- 超时返回结构化 RecordingWriteFailed 错误
- 新增 worker hang 超时测试

修复 Important 1: finish()/consumer join 仍不是严格 bounded"
```

---

## Task 5: Phase E — 蓝牙 mic stop diagnostics

**Files:**
- Modify: `src-tauri/src/platform/macos/cpal_microphone.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `BUG.md`

- [ ] **Step 1: 新增 `CpalMicrophoneStopDiagnostics` 结构体**

在 `src-tauri/src/platform/macos/cpal_microphone.rs` 中新增：

```rust
#[derive(Debug, Clone, Default)]
pub struct CpalMicrophoneStopDiagnostics {
    pub stop_requested: bool,
    pub stream_existed: bool,
    pub pause_attempted: bool,
    pub pause_ok: bool,
    pub pause_error: Option<String>,
    pub stream_dropped: bool,
    pub callbacks_after_stop: u64,
    pub stop_wait_ms: u64,
}
```

- [ ] **Step 2: 修改 `CpalMicrophoneCapture` — 结构化 stop diagnostics**

1. 新增 `stop_diagnostics: Arc<AtomicU64>` 用于 `callbacks_after_stop` 计数。
2. callback 中当 `running=false` 时递增计数。
3. `stop()` 返回 `CpalMicrophoneStopDiagnostics`：

```rust
pub fn stop(&mut self) -> AppResult<CpalMicrophoneStopDiagnostics> {
    let mut diag = CpalMicrophoneStopDiagnostics::default();
    diag.stop_requested = true;

    self.running.store(false, Ordering::Relaxed);

    let stream = self.stream.take();
    diag.stream_existed = stream.is_some();

    if let Some(s) = stream {
        diag.pause_attempted = true;
        match s.0.pause() {
            Ok(()) => { diag.pause_ok = true; }
            Err(e) => { diag.pause_error = Some(format!("{:?}", e)); }
        }
        drop(s);
        diag.stream_dropped = true;

        // Wait for callbacks to settle
        let wait_start = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(300));
        diag.stop_wait_ms = wait_start.elapsed().as_millis() as u64;
    }

    diag.callbacks_after_stop = self.callbacks_after_stop_count.load(Ordering::Acquire);
    Ok(diag)
}
```

- [ ] **Step 3: 更新 `macos_service.rs` — 使用结构化 diagnostics**

在 `MacRecordingService::stop()` 中，使用返回的 diagnostics：

```rust
let mic_stop_diag = self.mic_capture.stop()?;
eprintln!("麦克风停止诊断: {:?}", mic_stop_diag);
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: 全部 PASS。

- [ ] **Step 5: 更新 BUG.md 预防规则**

在 BUG-005 预防规则中补充：

```markdown
21. 蓝牙麦克风 stop 必须返回结构化 `CpalMicrophoneStopDiagnostics`（pause_attempted/pause_ok/stream_dropped/callbacks_after_stop/stop_wait_ms），不能只靠 eprintln。
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/platform/macos/cpal_microphone.rs src-tauri/src/platform/macos_service.rs BUG.md
git commit -m "fix(audio): 蓝牙 mic stop diagnostics — 结构化 pause/drop/wait 诊断

- 新增 CpalMicrophoneStopDiagnostics 结构体
- stop() 返回 pause_attempted/pause_ok/stream_dropped/callbacks_after_stop
- macos_service 使用结构化诊断替代纯 eprintln
- BUG.md 补充预防规则

修复 Minor 1: 蓝牙 mic stop 的错误与释放诊断仍不完整"
```

---

## Task 6: 完整回归验证与 BUG.md 更新

**Files:**
- Modify: `BUG.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: 完整回归测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected: 全部 PASS，无新增 warning。

- [ ] **Step 2: 聚焦测试验证**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_sample_clock -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg source_aware -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture
```

Expected: 全部 PASS。

- [ ] **Step 3: 更新 BUG.md**

确认所有预防规则已正确补充。

- [ ] **Step 4: 更新 HANDOFF.md**

记录本轮整改内容和验证结果。

- [ ] **Step 5: Final Commit**

```bash
git add BUG.md HANDOFF.md
git commit -m "docs: 更新 BUG.md 和 HANDOFF.md — Phase 6 code review 整改完成

- BUG.md: 补充 CPAL lazy offset、drop 分层、bounded finalize 预防规则
- HANDOFF.md: 记录本轮 5 个 Phase 整改内容和验证结果"
```

---

## 验证矩阵

### 自动化验证

```bash
# 完整回归
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run

# 代码质量
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets

# 聚焦测试
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg audio_sample_clock -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg source_aware -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg requested_audio_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg consume_frames -- --nocapture
```

### 真实设备 Manual Gate

1. 系统音频 + 内置麦克风 15 秒，停止录制成功。
2. 系统音频 + 蓝牙麦克风 15 秒，停止后蓝牙音质恢复。
3. 不采集音频，只录屏 15 秒，停止录制成功。
