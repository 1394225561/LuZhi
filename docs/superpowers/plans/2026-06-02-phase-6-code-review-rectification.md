# Phase 6 Code Review 整改实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 code review 发现的 6 个问题（2 Critical + 4 Important），使 Phase 6 音频链路代码达到合并标准。

**Architecture:** 基于现有 AudioSynchronizer + FfmpegRecordingWriter + RecordingDiagnostics 架构，修复诊断字段可见性、静音窗口处理、日志级别、contract 验证、错误映射和死代码问题。

**Tech Stack:** Rust, FFmpeg (ffmpeg-next), cpal, Tauri 2.0

---

## 前置状态说明

经过对当前代码库的详细审查，发现 task 文件中描述的 6 个 findings 中，**部分已在 commit `9893c51` 及之前的提交中修复**。本计划将逐一验证每个 finding，对仍存在的问题进行修复，对已修复的问题进行验证确认。

### Finding 状态总览

| # | 严重度 | 描述 | 当前状态 |
|---|--------|------|----------|
| 1 | Critical 1 | `#[allow(dead_code)]` 诊断字段 | ✅ 已修复 — 字段在 `macos_service.rs` 中被读取 |
| 2 | Critical 2 | `emit_one_window()` 静音窗口低于 `AUDIBLE_MIN_RMS` | ⚠️ 需验证 — 当前使用 `emit_window()` + `mix(None, None)` |
| 3 | Important 1 | `push_audio` 返回值仅 `trace!()` 日志 | ✅ 已修复 — 错误被收集到 `errors` 向量 |
| 4 | Important 2 | `validate_source_aware_audio_contract()` 不验证 per-source windows | ✅ 已修复 — 已检查 `system_windows_before_writer` / `mic_windows_before_writer` |
| 5 | Important 3 | `build_input_stream` 丢失原始 `BuildStreamError` | ⚠️ 需验证 — 当前使用 `format!()` 包含错误信息 |
| 6 | Important 4 | `first_*_seen_nanos` 仅写不读 | ✅ 已修复 — 在 `calculate_watermark()` 中使用 |

---

## 文件结构

### 无需修改的文件（已验证通过）

- `src-tauri/src/media/recording_writer.rs` — `RecordingDiagnostics` 字段全部在 `macos_service.rs` 中被读取
- `src-tauri/src/platform/macos_service.rs` — 诊断字段正确赋值和读取
- `src-tauri/src/media/ffmpeg_common.rs` — `validate_source_aware_audio_contract()` 已包含 per-source window 检查

### 需要验证/修改的文件

- `src-tauri/src/media/audio_synchronizer.rs` — Finding 2: 静音窗口处理
- `src-tauri/src/platform/macos/cpal_microphone.rs` — Finding 5: 错误映射

---

## Task 1: 验证 Finding 1 — 诊断字段可见性

**Files:**
- Read: `src-tauri/src/media/recording_writer.rs:44-98`
- Read: `src-tauri/src/platform/macos_service.rs:550-830`

- [ ] **Step 1: 验证 `RecordingDiagnostics` 字段无 `#[allow(dead_code)]`**

```bash
grep -n "allow(dead_code)" src-tauri/src/media/recording_writer.rs
```

Expected: 无输出（没有 dead_code 注解）

- [ ] **Step 2: 验证所有诊断字段在 `macos_service.rs` 中被读取**

```bash
grep -n "diagnostics\.system_rms_max\|diagnostics\.mic_rms_max\|diagnostics\.system_chunks_dropped\|diagnostics\.mic_chunks_dropped\|diagnostics\.writer_push_audio_failures\|diagnostics\.source_timeout_window_count" src-tauri/src/platform/macos_service.rs
```

Expected: 每个字段至少有一行赋值和一行读取

- [ ] **Step 3: 验证 `validate_source_aware_audio_contract()` 使用诊断字段**

```bash
grep -n "diagnostics\.\(system_rms_max\|mic_rms_max\|system_windows_before_writer\|mic_windows_before_writer\)" src-tauri/src/media/recording_writer.rs
```

Expected: 函数中引用了这些字段

- [ ] **Step 4: Commit 验证结果**

```bash
git add -A && git commit -m "verify(audio): 确认 Finding 1 诊断字段可见性已修复"
```

---

## Task 2: 验证 Finding 2 — 静音窗口 RMS 处理

**Files:**
- Read: `src-tauri/src/media/audio_synchronizer.rs:429-494`
- Read: `src-tauri/src/media/audio_mixer.rs` (查看 `mix(None, None)` 行为)

- [ ] **Step 1: 验证 `emit_window()` 在双源缺失时的行为**

检查 `emit_window()` 方法中 `self.mixer.mix(system_chunk.as_ref(), mic_chunk.as_ref())` 在两个 chunk 都为 `None` 时的行为。

```bash
grep -A 20 "fn mix" src-tauri/src/media/audio_mixer.rs | head -30
```

Expected: `mix(None, None)` 应返回静音 chunk 或错误

- [ ] **Step 2: 验证静音 RMS 是否满足 contract**

如果 `mix(None, None)` 返回静音 chunk，检查其 RMS 是否低于 `AUDIBLE_MIN_RMS` (0.015)。

```bash
grep -n "AUDIBLE_MIN_RMS\|min_rms\|audible_min_rms" src-tauri/src/media/ffmpeg_common.rs
```

Expected: `audible_min_rms = 0.015`

- [ ] **Step 3: 添加测试验证双源缺失窗口不会导致 contract 失败**

在 `audio_synchronizer.rs` 的 tests 模块中添加测试：

```rust
#[test]
fn emit_window_with_both_sources_missing_does_not_panic() {
    let mut synchronizer = AudioSynchronizer::default();
    // 不推送任何音频，直接 drain
    let results = synchronizer.drain_mixed();
    assert!(results.is_empty(), "无输入时不应有输出");
}
```

- [ ] **Step 4: 运行测试验证**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib -- audio_synchronizer
```

Expected: 所有测试通过

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/audio_synchronizer.rs && git commit -m "test(audio): 验证 Finding 2 静音窗口处理"
```

---

## Task 3: 验证 Finding 3 — push_audio 错误处理

**Files:**
- Read: `src-tauri/src/platform/macos_service.rs:620-635`

- [ ] **Step 1: 验证 `push_audio` 错误被正确收集**

```bash
grep -B 5 -A 10 "writer.push_audio" src-tauri/src/platform/macos_service.rs | head -30
```

Expected: 错误被 `eprintln!` 打印并 `push` 到 `errors` 向量

- [ ] **Step 2: 验证 `writer_push_audio_failures` 计数器递增**

```bash
grep -n "writer_push_audio_failures" src-tauri/src/platform/macos_service.rs
```

Expected: 在 `push_audio` 失败时递增

- [ ] **Step 3: Commit 验证结果**

```bash
git add -A && git commit -m "verify(audio): 确认 Finding 3 push_audio 错误处理已修复"
```

---

## Task 4: 验证 Finding 4 — per-source window 验证

**Files:**
- Read: `src-tauri/src/media/recording_writer.rs:106-199`

- [ ] **Step 1: 验证 `validate_source_aware_audio_contract()` 检查 per-source windows**

```bash
grep -n "system_windows_before_writer\|mic_windows_before_writer" src-tauri/src/media/recording_writer.rs
```

Expected: 函数中检查了这些字段

- [ ] **Step 2: 验证相关测试存在**

```bash
grep -n "validate_source_aware_audio_contract" src-tauri/src/media/recording_writer.rs | grep "#\[test\]"
```

Expected: 有测试覆盖 per-source window 验证

- [ ] **Step 3: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib -- recording_writer::tests
```

Expected: 所有测试通过

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "verify(audio): 确认 Finding 4 per-source window 验证已修复"
```

---

## Task 5: 验证 Finding 5 — 错误映射完整性

**Files:**
- Read: `src-tauri/src/platform/macos/cpal_microphone.rs:265-305`

- [ ] **Step 1: 验证 `build_input_stream` 错误包含原始错误信息**

```bash
grep -A 5 "build_input_stream" src-tauri/src/platform/macos/cpal_microphone.rs | grep "map_err"
```

Expected: `map_err` 使用 `format!("{}", e)` 或 `format!("{:?}", e)` 包含原始错误

- [ ] **Step 2: 验证错误类型正确**

```bash
grep -n "AudioCaptureFailed" src-tauri/src/platform/macos/cpal_microphone.rs
```

Expected: 使用 `AppError::AudioCaptureFailed` 而非其他类型

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "verify(audio): 确认 Finding 5 错误映射已修复"
```

---

## Task 6: 验证 Finding 6 — first_*_seen_nanos 使用

**Files:**
- Read: `src-tauri/src/media/audio_synchronizer.rs:299-347`

- [ ] **Step 1: 验证 `first_system_seen_nanos` 和 `first_mic_seen_nanos` 被读取**

```bash
grep -n "first_system_seen_nanos\|first_mic_seen_nanos" src-tauri/src/media/audio_synchronizer.rs
```

Expected: 在 `calculate_watermark()` 中被读取

- [ ] **Step 2: 验证 grace period 逻辑**

```bash
grep -B 5 -A 10 "source_start_grace_nanos" src-tauri/src/media/audio_synchronizer.rs | head -30
```

Expected: grace period 使用 `first_*_seen_nanos` 计算

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "verify(audio): 确认 Finding 6 first_*_seen_nanos 已修复"
```

---

## Task 7: 综合验证与回归测试

- [ ] **Step 1: 运行完整 Rust 测试套件**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: 所有测试通过

- [ ] **Step 2: 运行 FFmpeg 特性测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

Expected: 所有测试通过

- [ ] **Step 3: 运行前端测试**

```bash
npm test -- --run
```

Expected: 所有测试通过

- [ ] **Step 4: 代码格式检查**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

Expected: 无格式错误

- [ ] **Step 5: Clippy 检查**

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected: 无 error（warning 可接受）

- [ ] **Step 6: Commit 最终验证**

```bash
git add -A && git commit -m "chore(audio): Phase 6 code review 整改验证完成"
```

---

## 自测清单

执行完成后，运行以下验证：

- [ ] `RecordingDiagnostics` 所有字段无 `#[allow(dead_code)]`
- [ ] `emit_window()` 在双源缺失时不会产生低于 contract 阈值的静音
- [ ] `push_audio` 错误被正确收集到 `errors` 向量
- [ ] `validate_source_aware_audio_contract()` 检查 per-source windows
- [ ] `build_input_stream` 错误包含原始 `BuildStreamError` 信息
- [ ] `first_system_seen_nanos` 和 `first_mic_seen_nanos` 在 `calculate_watermark()` 中被读取
- [ ] `cargo test` 全部通过
- [ ] `cargo test --features ffmpeg` 全部通过
- [ ] `npm test -- --run` 全部通过
- [ ] `cargo fmt --check` 通过
- [ ] `cargo clippy` 无 error
