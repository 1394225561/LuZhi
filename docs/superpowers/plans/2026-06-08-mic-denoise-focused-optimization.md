# 麦克风降噪聚焦优化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不新增依赖、不改通道架构的前提下，针对麦克风低频嗡声和峰值硬削波失真做两项可验证优化。

**Architecture:** 本计划只修改 Rust 侧音频处理链路。第一部分把默认麦克风高通截止频率从 80Hz 提升到 100Hz，并用链路级测试证明 30Hz 嗡声比旧 80Hz baseline 更低，同时保留低频人声保护测试。第二部分用透明软限幅器替换 hard clamp，只在接近满幅或超幅时压缩，避免 `tanh` 全范围染色。

**Tech Stack:** Rust, Tauri backend, existing `audio_denoise.rs` / `audio_mixer.rs`, no new crates, no `Cargo.toml` changes.

---

## Scope Decisions

本计划明确不做以下事项：

1. 不添加 `rubato` 或任何新依赖；高质量重采样另开 Spike，经人工确认依赖后再实施。
2. 不修改 `src-tauri/src/core/media_channel.rs` 的 drop-oldest 策略；当前 `MediaSender` 只持有 `SyncSender<T>`，发送端无法 `try_recv`，需要单独队列设计。
3. 不回退 `NoiseFloorSuppressor` 到更激进的 `open=0.020 / close=0.004 / attack=8ms`；BUG-0024 已验证当前参数更保护轻声、开头和尾音。
4. 不修改 `NOTCH_Q`；陷波瞬态振铃需要先设计能测实际默认常量的可靠测试。
5. 不在执行前把 `BUG.md` 写成“已修复”；文档更新只在代码验证通过后进行。

---

## File Structure

| 文件 | 操作 | 职责 |
|------|------|------|
| `src-tauri/src/media/audio_denoise.rs` | Modify | 默认麦克风高通 cutoff 提升到 100Hz；新增链路级低频嗡声衰减测试和低频人声保护测试 |
| `src-tauri/src/media/audio_mixer.rs` | Modify | 用透明 soft-knee limiter 替换 `clamp_samples`；新增 limiter 单元测试和 mixer 路径测试 |
| `tests/2026-06-08-mic-denoise-focused-optimization-checklist.md` | Create | 保存本轮自动测试和人工验收清单 |
| `BUG.md` | Modify after verification | 记录 BUG-0025 的已验证根因、修复和预防规则 |
| `HANDOFF.md` | Modify after verification | 按时间倒序记录本轮实施与验证结果 |

---

## Phase 0: Baseline

### Task 0: Confirm Current Audio Baseline

**Files:**
- Read-only: `src-tauri/src/media/audio_denoise.rs`
- Read-only: `src-tauri/src/media/audio_mixer.rs`

- [ ] **Step 1: Run current denoise tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture
```

Expected: PASS. Current baseline should report 14 denoise tests passing. Existing warnings outside `audio_denoise.rs` are acceptable if tests pass.

- [ ] **Step 2: Run current mixer tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture
```

Expected: PASS. Current baseline should report 22 mixer tests passing. Existing warnings outside `audio_mixer.rs` are acceptable if tests pass.

- [ ] **Step 3: Inspect worktree before editing**

Run:

```bash
git status --short
```

Expected: Only known user/plan files should be listed. Do not revert unrelated files.

---

## Phase 1: Stronger Low-Frequency Hum Attenuation

### Task 1: Add Chain-Level Highpass Regression Tests

**Files:**
- Modify: `src-tauri/src/media/audio_denoise.rs`

- [ ] **Step 1: Add failing 30Hz hum test and voice preservation guard**

In `src-tauri/src/media/audio_denoise.rs`, inside `#[cfg(test)] mod tests`, add these tests after `denoise_chain_reduces_low_level_broadband_noise_floor`:

```rust
    #[test]
    fn denoise_chain_attenuates_30hz_hum_beyond_80hz_baseline() {
        let input = sine(30.0, 1.0, 0.6);

        let mut old_highpass = HighpassFilter::new(80.0, SAMPLE_RATE);
        let old_output: Vec<f32> = input
            .iter()
            .map(|sample| old_highpass.process(*sample))
            .collect();
        let old_rms = steady_rms_after_warmup(&old_output);

        let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);
        let denoised: Vec<f32> = input.iter().map(|sample| chain.process(*sample)).collect();
        let denoised_rms = steady_rms_after_warmup(&denoised);

        assert!(
            denoised_rms < old_rms * 0.75,
            "30Hz hum should be lower than the old 80Hz highpass baseline: old={old_rms}, denoised={denoised_rms}"
        );
    }

    #[test]
    fn denoise_chain_preserves_200hz_low_voice_after_stronger_highpass() {
        let input = sine(200.0, 1.0, 0.3);
        let input_rms = steady_rms_after_warmup(&input);

        let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);
        let denoised: Vec<f32> = input.iter().map(|sample| chain.process(*sample)).collect();
        let denoised_rms = steady_rms_after_warmup(&denoised);

        assert!(
            denoised_rms > input_rms * 0.82,
            "200Hz low voice should remain usable after stronger highpass: input={input_rms}, denoised={denoised_rms}"
        );
    }
```

- [ ] **Step 2: Run the new RED test**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_attenuates_30hz_hum_beyond_80hz_baseline -- --nocapture
```

Expected: FAIL with message containing `30Hz hum should be lower than the old 80Hz highpass baseline`. If this test unexpectedly passes, stop and inspect the printed `old=` and `denoised=` values before changing production code.

- [ ] **Step 3: Run the low-voice guard**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_preserves_200hz_low_voice_after_stronger_highpass -- --nocapture
```

Expected: PASS. This guard should pass before and after the highpass cutoff change.

- [ ] **Step 4: Commit tests**

```bash
git add src-tauri/src/media/audio_denoise.rs
git commit -m "test(audio): 覆盖 30Hz 嗡声衰减与低频人声保真"
```

---

### Task 2: Raise Microphone Highpass Cutoff to 100Hz

**Files:**
- Modify: `src-tauri/src/media/audio_denoise.rs`

- [ ] **Step 1: Update the highpass doc comment**

In `HighpassFilter::new()` docs, change the cutoff guidance from:

```rust
    /// - `cutoff_hz`: 截止频率（Hz），建议 80Hz
```

to:

```rust
    /// - `cutoff_hz`: 截止频率（Hz），麦克风链路默认使用 100Hz
```

- [ ] **Step 2: Add an explicit default cutoff constant**

In `MicrophoneDenoiseChain::new()`, replace the current constants and `highpass` field with:

```rust
    pub fn new(sample_rate: f64) -> Self {
        const HIGHPASS_CUTOFF_HZ: f64 = 100.0;
        const NOTCH_Q: f64 = 35.0;
        const NOTCH_FREQUENCIES_HZ: [f64; 6] = [50.0, 60.0, 100.0, 120.0, 150.0, 180.0];

        Self {
            highpass: HighpassFilter::new(HIGHPASS_CUTOFF_HZ, sample_rate),
            notches: NOTCH_FREQUENCIES_HZ
                .into_iter()
                .filter(|freq| *freq < sample_rate / 2.0)
                .map(|freq| NotchFilter::new(freq, sample_rate, NOTCH_Q))
                .collect(),
            noise_suppressor: NoiseFloorSuppressor::new(sample_rate),
        }
    }
```

- [ ] **Step 3: Run the RED test again**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_attenuates_30hz_hum_beyond_80hz_baseline -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Run denoise regression tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture
```

Expected: PASS. This must include existing BUG-0024 guards:

```text
denoise_chain_reduces_low_level_broadband_noise_floor
denoise_chain_preserves_quiet_voice_near_noise_gate
denoise_chain_does_not_hold_back_voice_after_quiet_noise
denoise_chain_preserves_quiet_voice_tail_after_normal_speech
denoise_chain_preserves_voice_band_signal
```

- [ ] **Step 5: Run mixer regression tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture
```

Expected: PASS. This confirms the source-aware mixer tests still pass after the default denoise chain change.

- [ ] **Step 6: Commit implementation**

```bash
git add src-tauri/src/media/audio_denoise.rs
git commit -m "fix(audio): 提升麦克风高通截止频率至 100Hz"
```

---

## Phase 2: Transparent Soft Limiter

### Task 3: Add Soft Limiter Regression Tests

**Files:**
- Modify: `src-tauri/src/media/audio_mixer.rs`

- [ ] **Step 1: Add soft limiter tests**

In `src-tauri/src/media/audio_mixer.rs`, inside `#[cfg(test)] mod tests`, add these tests after `clipping_protection`:

```rust
    #[test]
    fn soft_limiter_preserves_samples_below_knee() {
        let samples = vec![-0.95f32, -0.5, 0.0, 0.5, 0.95];
        let result = soft_limit_samples(&samples);

        assert_eq!(result.len(), samples.len());
        for (input, output) in samples.iter().zip(result.iter()) {
            assert!(
                (*input - *output).abs() < 1e-6,
                "samples below the knee should be unchanged: input={input}, output={output}"
            );
        }
    }

    #[test]
    fn soft_limiter_compresses_peaks_without_hard_plateau() {
        let samples = vec![0.96f32, 1.0, 1.2, 1.5, 2.0];
        let result = soft_limit_samples(&samples);

        for output in &result {
            assert!(
                *output > 0.95 && *output < 1.0,
                "positive peaks should be smoothly compressed below full scale: output={output}"
            );
        }

        for pair in result.windows(2) {
            assert!(
                pair[1] > pair[0],
                "soft limiter should remain monotonic above the knee: previous={}, next={}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn soft_limiter_limits_extreme_values_near_full_scale() {
        let samples = vec![10.0f32, -10.0];
        let result = soft_limit_samples(&samples);

        assert!(result[0] > 0.99 && result[0] < 1.0);
        assert!(result[1] < -0.99 && result[1] > -1.0);
    }

    #[test]
    fn mixer_uses_soft_limiter_for_single_source_peaks() {
        let mixer = SimpleAudioMixer::new(DenoiseMode::default());
        let chunk = make_chunk(0, 48000, 2, vec![2.0, -2.0]);

        let result = mixer.mix(Some(&chunk), None).unwrap();

        assert!(result.samples[0] > 0.99 && result.samples[0] < 1.0);
        assert!(result.samples[1] < -0.99 && result.samples[1] > -1.0);
    }
```

- [ ] **Step 2: Run the soft limiter test to verify RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib soft_limiter_preserves_samples_below_knee -- --nocapture
```

Expected: Compile FAIL with a message containing `cannot find function 'soft_limit_samples'`.

- [ ] **Step 3: Commit tests**

Do not commit if Step 2 failed for a reason other than `soft_limit_samples` missing.

```bash
git add src-tauri/src/media/audio_mixer.rs
git commit -m "test(audio): 覆盖透明软限幅器行为"
```

---

### Task 4: Replace Hard Clamp with Soft-Knee Limiter

**Files:**
- Modify: `src-tauri/src/media/audio_mixer.rs`

- [ ] **Step 1: Update mixer documentation comments**

At the top of `SimpleAudioMixer`, change:

```rust
/// 4. Weighted sum mixing with hard clipping protection
```

to:

```rust
/// 4. Weighted sum mixing with soft limiting protection
```

In `mix_two()` docs, change:

```rust
/// 5. Hard clamp to [-1.0, 1.0] to prevent clipping
```

to:

```rust
/// 5. Soft-limit peaks near full scale to prevent hard clipping
```

- [ ] **Step 2: Replace `clamp_samples` calls**

In `passthrough()`, replace:

```rust
    let clamped = clamp_samples(&stereo);
```

with:

```rust
    let limited = soft_limit_samples(&stereo);
```

and replace:

```rust
        samples: clamped.into(),
```

with:

```rust
        samples: limited.into(),
```

In `mix_two()`, replace:

```rust
    // Hard clamp to prevent clipping
    let clamped = clamp_samples(&output);
```

with:

```rust
    // Soft-limit peaks to prevent hard clipping artifacts.
    let limited = soft_limit_samples(&output);
```

and replace:

```rust
        samples: clamped.into(),
```

with:

```rust
        samples: limited.into(),
```

- [ ] **Step 3: Replace `clamp_samples` with soft limiter functions**

Replace the existing `clamp_samples` function:

```rust
/// Hard-clamps all samples to [-1.0, 1.0] to prevent clipping.
fn clamp_samples(samples: &[f32]) -> Vec<f32> {
    samples.iter().map(|s| s.clamp(-1.0, 1.0)).collect()
}
```

with:

```rust
/// Soft-limits audio samples near full scale while preserving normal levels.
///
/// Samples at or below ±0.95 are left unchanged. Above that knee, peaks are
/// compressed smoothly toward ±1.0 without creating the flat plateau caused by
/// hard clipping.
fn soft_limit_samples(samples: &[f32]) -> Vec<f32> {
    samples
        .iter()
        .map(|sample| soft_limit_sample(*sample))
        .collect()
}

fn soft_limit_sample(sample: f32) -> f32 {
    const KNEE_START: f32 = 0.95;
    const KNEE_WIDTH: f32 = 1.0 - KNEE_START;

    let magnitude = sample.abs();
    if magnitude <= KNEE_START {
        return sample;
    }

    let over_knee = magnitude - KNEE_START;
    let limited = KNEE_START + KNEE_WIDTH * (over_knee / (over_knee + KNEE_WIDTH));

    sample.signum() * limited.min(1.0)
}
```

- [ ] **Step 4: Run soft limiter tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib soft_limiter -- --nocapture
```

Expected: PASS. All four tests from Task 3 should pass.

- [ ] **Step 5: Run mixer regression tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture
```

Expected: PASS. Existing tests such as `passthrough_single_source`, `mix_two_aligned_sources`, `clipping_protection`, and source-aware denoise bypass tests must still pass.

- [ ] **Step 6: Commit implementation**

```bash
git add src-tauri/src/media/audio_mixer.rs
git commit -m "fix(audio): 用透明软限幅器替换硬削波"
```

---

## Phase 3: Documentation and Verification

### Task 5: Create Focused Self-Test Checklist

**Files:**
- Create: `tests/2026-06-08-mic-denoise-focused-optimization-checklist.md`

- [ ] **Step 1: Create the checklist file**

Create `tests/2026-06-08-mic-denoise-focused-optimization-checklist.md` with exactly this content:

```markdown
# 麦克风降噪聚焦优化自测清单

## Phase 0: Baseline

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture` 基线通过
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture` 基线通过
- [ ] `git status --short` 已确认没有误碰无关文件

## Phase 1: 100Hz 麦克风高通

- [ ] `denoise_chain_attenuates_30hz_hum_beyond_80hz_baseline` 修改前失败
- [ ] `denoise_chain_attenuates_30hz_hum_beyond_80hz_baseline` 修改后通过
- [ ] `denoise_chain_preserves_200hz_low_voice_after_stronger_highpass` 通过
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture` 通过
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture` 通过

## Phase 2: 透明软限幅器

- [ ] `soft_limiter_preserves_samples_below_knee` 修改前因 `soft_limit_samples` 不存在而编译失败
- [ ] `soft_limiter_preserves_samples_below_knee` 修改后通过
- [ ] `soft_limiter_compresses_peaks_without_hard_plateau` 通过
- [ ] `soft_limiter_limits_extreme_values_near_full_scale` 通过
- [ ] `mixer_uses_soft_limiter_for_single_source_peaks` 通过
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture` 通过

## Final Regression

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过
- [ ] `rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs` 通过
- [ ] `git diff --check` 通过

## Manual Verification

- [ ] 有线耳机麦克风开启降噪，静音环境录制 10 秒，低频嗡声比修复前更弱
- [ ] 有线耳机麦克风开启降噪，正常说话 10 秒，开头不吞字，尾音不过早切断
- [ ] 同时录制系统音频和麦克风，确认系统音频音质不受麦克风 denoise 影响
- [ ] 播放较大音量系统音频并说话，确认录制结果没有硬削波的撕裂感
```

- [ ] **Step 2: Commit checklist**

```bash
git add tests/2026-06-08-mic-denoise-focused-optimization-checklist.md
git commit -m "test(audio): 添加麦克风聚焦优化自测清单"
```

---

### Task 6: Update BUG and HANDOFF After Verification

**Files:**
- Modify: `BUG.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Run final automated verification**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: PASS. Existing warnings are acceptable only if all tests pass.

Run:

```bash
rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs
```

Expected: PASS with no diff.

Run:

```bash
git diff --check
```

Expected: PASS with no whitespace errors.

- [ ] **Step 2: Add BUG-0025 entry**

In `BUG.md`, under `## 已解决` and above BUG-0024, add:

```markdown
### BUG-0025: 麦克风低频嗡声与峰值硬削波失真 ✅ 已修复-待人工验证

**现象**：开启麦克风录制后，部分设备录制结果中仍可能出现低沉嗡声；大音量混音场景中，峰值被 hard clamp 后可能产生不自然的撕裂感。

**根因**：

1. 麦克风链路默认 80Hz 二阶高通对 30Hz 一类低频嗡声仍不够激进。
2. `audio_mixer.rs` 使用 hard clamp，把所有超过 ±1.0 的峰值直接截平成平台，容易产生高频谐波失真。

**修复**：

1. 将默认麦克风高通截止频率提升到 100Hz，并新增 30Hz 嗡声相对旧 80Hz baseline 的回归测试。
2. 新增 200Hz 低频人声保护测试，避免为了压低频嗡声误伤低频人声可用性。
3. 用透明 soft-knee limiter 替换 hard clamp；±0.95 以内完全保持原样，超过阈值后平滑压向 ±1.0。
4. 保持 source-aware 行为：麦克风 denoise 仍只作用于麦克风输入，不影响系统音频路径。
5. 不新增依赖、不修改 `Cargo.toml`、不修改媒体通道架构。

**预防规则**：

65. 麦克风低频降噪参数调整必须同时覆盖低频噪声衰减和低频人声保真，不能只验证单个低频噪声点。
66. 默认麦克风 denoise 参数变更必须继续跑 BUG-0024 的轻声、开头恢复、尾音保留和 source-aware 系统音频旁路回归。
67. 混音峰值保护不得使用 hard clamp 作为最终音质策略；必须使用保留正常电平、仅压缩峰值的透明限幅器。
68. 新增重采样依赖或媒体通道策略变更必须单独开 Spike 和计划，不能混入麦克风 DSP 参数小步优化。
```

- [ ] **Step 3: Add HANDOFF work record**

In `HANDOFF.md`, under `## 工作任务记录` and above the current newest record, add:

```markdown
### 2026-06-08：BUG-0025 麦克风低频嗡声与峰值硬削波失真聚焦优化

输入文件：

- 用户要求：按评估建议重新制定并执行聚焦实施计划
- `HANDOFF.md`
- `BUG.md`
- `src-tauri/src/media/audio_denoise.rs`
- `src-tauri/src/media/audio_mixer.rs`

根因结论：

1. 原深度优化计划把降噪参数、soft clip、sinc 重采样和通道 drop-oldest 混在一起，范围过大且部分实现前提不成立
2. 本轮只保留可小步验证的两项：默认麦克风高通 100Hz 和透明 soft-knee limiter
3. `rubato` 重采样和媒体通道 drop-oldest 策略需要单独 Spike，不在本轮实现

已完成：

1. 新增 30Hz 嗡声相对旧 80Hz highpass baseline 的链路级回归测试
2. 将默认麦克风高通截止频率提升到 100Hz
3. 新增 200Hz 低频人声保真回归，避免低频降噪误伤人声
4. 用透明 soft-knee limiter 替换 `audio_mixer.rs` 中的 hard clamp
5. 新增 soft limiter 单元测试和 mixer 路径测试
6. 新增本轮自测清单

当前验证结果：

- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_attenuates_30hz_hum_beyond_80hz_baseline -- --nocapture` 修改前失败
- GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_attenuates_30hz_hum_beyond_80hz_baseline -- --nocapture` 修改后通过
- GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_preserves_200hz_low_voice_after_stronger_highpass -- --nocapture` 通过
- RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib soft_limiter_preserves_samples_below_knee -- --nocapture` 修改前因 `soft_limit_samples` 不存在而编译失败
- GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib soft_limiter -- --nocapture` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过
- `rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs` 通过
- `git diff --check` 通过

改动文件：

- **修改**: `src-tauri/src/media/audio_denoise.rs`
- **修改**: `src-tauri/src/media/audio_mixer.rs`
- **修改**: `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-08-mic-denoise-focused-optimization-checklist.md`

人工复核建议：

1. 有线耳机麦克风开启降噪，静音环境录制 10 秒，确认低频嗡声弱于修复前
2. 开启降噪后正常说话 10 秒，确认开头不被吞字，尾音不过早切断
3. 同时录制系统音频和麦克风，确认系统音频音质不受影响
4. 大音量系统音频 + 麦克风同时录制，确认没有 hard clamp 撕裂感
```

After inserting this record, keep only the newest 7 work records in `HANDOFF.md`.

- [ ] **Step 4: Commit docs**

```bash
git add BUG.md HANDOFF.md
git commit -m "docs: 记录 BUG-0025 麦克风聚焦优化"
```

---

## Final Verification

### Task 7: Final Full Regression

**Files:**
- Verify: `src-tauri/src/media/audio_denoise.rs`
- Verify: `src-tauri/src/media/audio_mixer.rs`
- Verify: `BUG.md`
- Verify: `HANDOFF.md`
- Verify: `tests/2026-06-08-mic-denoise-focused-optimization-checklist.md`

- [ ] **Step 1: Run full Rust lib tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: PASS.

- [ ] **Step 2: Run targeted audio tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run formatting and whitespace checks**

```bash
rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs
git diff --check
```

Expected: PASS.

- [ ] **Step 4: Confirm no forbidden scope changes**

Run:

```bash
git diff -- src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/core/media_channel.rs
```

Expected: No diff. This confirms the plan did not add dependencies or change media channel strategy.

---

## Self-Review

Spec coverage:

1. Low-frequency hum attenuation is covered by Task 1 and Task 2.
2. Low-frequency voice preservation is covered by Task 1 and final denoise regression.
3. Hard clamp replacement is covered by Task 3 and Task 4.
4. No new dependency and no media channel change are covered by Scope Decisions and Final Verification Task 7 Step 4.
5. Project documentation and checklist requirements are covered by Task 5 and Task 6.

Placeholder scan:

1. No placeholder markers or undefined later work remain.
2. Every code-changing step includes exact code blocks.
3. Every test step includes exact command and expected result.

Type consistency:

1. `soft_limit_samples` is introduced in Task 4 and referenced only by tests added in Task 3.
2. `HIGHPASS_CUTOFF_HZ` is local to `MicrophoneDenoiseChain::new()` and used in the same function.
3. No task references `rubato`, `try_send_drop_oldest`, or other out-of-scope APIs.

---

## Plan Complete

Plan saved to `docs/superpowers/plans/2026-06-08-mic-denoise-focused-optimization.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — 每个 Task 派发独立子代理执行，Task 间做 review，快速迭代
2. **Inline Execution** — 在当前会话中按 Task 顺序执行，带检查点

**Which approach?**
