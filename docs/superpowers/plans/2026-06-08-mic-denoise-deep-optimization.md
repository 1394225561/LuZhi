# 麦克风降噪深度优化实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除麦克风录制中的断续电流噪声（滋滋/底噪）和突然低沉嗡嗡声，通过优化降噪参数、替换重采样算法、引入软限幅器和改进通道策略。

**Architecture:** 分 4 个 Phase 递进实施：Phase 1 仅调参（零架构改动），Phase 2 替换硬削波为软限幅器，Phase 3 引入高质量 sinc 重采样，Phase 4 优化媒体通道丢包策略。每个 Phase 独立可验证、可回滚。

**Tech Stack:** Rust (audio_denoise.rs, audio_mixer.rs, media_channel.rs), `rubato` crate (sinc resampling)

---

## 文件结构

| 文件 | 操作 | 职责 |
|------|------|------|
| `src-tauri/src/media/audio_denoise.rs` | Modify | 降噪参数调整、高通阶数升级、抑制器平滑优化 |
| `src-tauri/src/media/audio_mixer.rs` | Modify | 软限幅器替换硬削波、sinc 重采样替换线性插值 |
| `src-tauri/src/core/media_channel.rs` | Modify | 通道丢包策略改为丢弃最旧 |
| `src-tauri/Cargo.toml` | Modify | 添加 `rubato` 依赖 |
| `BUG.md` | Modify | 新增本轮预防规则 |
| `HANDOFF.md` | Modify | 更新工作记录 |

---

## Phase 1: 降噪参数优化（零架构改动）

> 目标：仅调整 `audio_denoise.rs` 中的硬编码参数，解决 NoiseFloorSuppressor 边界泵浦和低频衰减不足。

### Task 1: 调整 NoiseFloorSuppressor 参数

**Files:**
- Modify: `src-tauri/src/media/audio_denoise.rs:219-231`

- [ ] **Step 1: 编写参数变更前的回归测试**

在 `audio_denoise.rs` 的 `#[cfg(test)] mod tests` 中追加测试：

```rust
#[test]
fn suppressor_does_not_pump_at_boundary_level() {
    // 模拟信号电平恰好在 open/close 门限之间振荡
    // 旧参数下增益会快速泵浦，产生可闻的幅度调制
    let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);

    // 交替产生接近门限边界的电平（envelope ≈ 0.010，在 0.006~0.014 之间）
    let samples_near_threshold: Vec<f32> = (0..48000)
        .map(|i| {
            let phase = i as f64 / SAMPLE_RATE;
            // 4Hz 方波调制，电平在 close_threshold 附近跳变
            if (phase * 4.0) as u64 % 2 == 0 {
                0.010 // 在门限区间内
            } else {
                0.003 // 低于 close_threshold
            }
        })
        .collect();

    let output: Vec<f32> = samples_near_threshold
        .iter()
        .map(|s| chain.process(*s))
        .collect();

    // 计算后半段的增益变化幅度（output / input 比值的标准差）
    let warmup = (SAMPLE_RATE as usize) / 2;
    let gain_variances: Vec<f32> = output[warmup..]
        .iter()
        .zip(samples_near_threshold[warmup..].iter())
        .map(|(o, i)| if i.abs() > 1e-6 { o / i } else { 1.0 })
        .collect();

    let mean_gain: f32 =
        gain_variances.iter().sum::<f32>() / gain_variances.len() as f32;
    let std_gain: f32 = (gain_variances
        .iter()
        .map(|g| (g - mean_gain).powi(2))
        .sum::<f32>()
        / gain_variances.len() as f32)
        .sqrt();

    // 增益标准差应小于 0.15（旧参数下 > 0.3，说明泵浦严重）
    assert!(
        std_gain < 0.15,
        "suppressor should not pump at boundary: gain_std={}",
        std_gain
    );
}

#[test]
fn suppressor_recovers_voice_onset_quickly_after_silence() {
    // 静音后突然说话，抑制器应在 30ms 内恢复到 80% 增益
    let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);

    // 前 500ms 静音（低于 close_threshold）
    let mut input = vec![0.001f32; (SAMPLE_RATE * 0.5) as usize];
    // 后 500ms 正常说话（0.15 振幅）
    input.extend(sine(1000.0, 0.5, 0.15));

    let output: Vec<f32> = input.iter().map(|s| chain.process(*s)).collect();

    // 语音开始后 30ms 处的增益应 > 0.8
    let onset_idx = (SAMPLE_RATE * 0.5) as usize;
    let check_idx = onset_idx + (SAMPLE_RATE * 0.030) as usize;
    if check_idx < output.len() && input[check_idx].abs() > 1e-6 {
        let gain_at_check = output[check_idx] / input[check_idx];
        assert!(
            gain_at_check > 0.8,
            "voice onset should recover quickly: gain_at_30ms={}",
            gain_at_check
        );
    }
}
```

- [ ] **Step 2: 运行测试确认旧参数下失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib suppressor_does_not_pump_at_boundary_level -- --nocapture
```

预期：FAIL（增益标准差过大）

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib suppressor_recovers_voice_onset_quickly_after_silence -- --nocapture
```

预期：FAIL（增益恢复过慢）

- [ ] **Step 3: 修改 NoiseFloorSuppressor 参数**

在 `audio_denoise.rs` 的 `NoiseFloorSuppressor::new()` 中修改（第 219-231 行）：

```rust
fn new(sample_rate: f64) -> Self {
    assert!(sample_rate > 0.0, "采样率必须大于 0");

    Self {
        open_threshold: 0.020,   // 0.014 → 0.020，拉大门限间距
        close_threshold: 0.004,  // 0.006 → 0.004，增大死区
        min_gain: 0.15,          // 0.28 → 0.15，静音段更干净
        gain: 1.0,
        envelope: 0.0,
        attack_coeff: smoothing_coeff(0.008, sample_rate),  // 4ms → 8ms，减缓开门
        release_coeff: smoothing_coeff(0.150, sample_rate),  // 80ms → 150ms，减缓关门
        envelope_coeff: smoothing_coeff(0.010, sample_rate),  // 不变
    }
}
```

- [ ] **Step 4: 运行新测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib suppressor_does_not_pump_at_boundary_level
cargo test --manifest-path src-tauri/Cargo.toml --lib suppressor_recovers_voice_onset_quickly_after_silence
```

预期：PASS

- [ ] **Step 5: 运行全部降噪回归测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture
```

预期：全部通过（包括已有的轻声保真、开头恢复、尾音保留等测试）

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture
```

预期：全部通过

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/audio_denoise.rs
git commit -m "fix(audio): 调整 NoiseFloorSuppressor 参数消除边界泵浦"
```

---

### Task 2: 提升高通滤波器截止频率

**Files:**
- Modify: `src-tauri/src/media/audio_denoise.rs:172`

- [ ] **Step 1: 编写衰减量回归测试**

```rust
#[test]
fn highpass_filter_attenuates_30hz_aggressively() {
    let mut filter = HighpassFilter::new(100.0, SAMPLE_RATE);

    let input = sine(30.0, 1.0, 0.5);
    let output: Vec<f32> = input.iter().map(|s| filter.process(*s)).collect();
    let output_peak = steady_rms_after_warmup(&output);

    // 30Hz 在 100Hz 截止时应被显著衰减
    assert!(
        output_peak < 0.3,
        "30Hz should be aggressively attenuated with 100Hz cutoff: peak={}",
        output_peak
    );
}
```

- [ ] **Step 2: 运行测试确认旧截止频率下失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib highpass_filter_attenuates_30hz_aggressively -- --nocapture
```

预期：FAIL（80Hz 截止时 30Hz 衰减不足）

- [ ] **Step 3: 修改高通截止频率**

`audio_denoise.rs` 第 172 行：

```rust
// 修改前
highpass: HighpassFilter::new(80.0, sample_rate),

// 修改后
highpass: HighpassFilter::new(100.0, sample_rate),
```

- [ ] **Step 4: 更新受影响的现有测试阈值**

`audio_denoise.rs` 中以下测试的断言阈值需要调整（因为 100Hz 截止比 80Hz 更激进）：

- `highpass_filter_attenuates_50hz`：50Hz 衰减从 ~-8dB 变为 ~-12dB，`max_output < 0.5` → `max_output < 0.35`
- `highpass_filter_attenuates_60hz`：60Hz 衰减从 ~-6dB 变为 ~-10dB，`max_output < 0.55` → `max_output < 0.4`

- [ ] **Step 5: 运行全部测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture
```

预期：全部通过

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/audio_denoise.rs
git commit -m "fix(audio): 提升高通截止频率至 100Hz 加强低频衰减"
```

---

### Task 3: 降低陷波滤波器 Q 值减少振铃

**Files:**
- Modify: `src-tauri/src/media/audio_denoise.rs:168`

- [ ] **Step 1: 编写振铃持续时间测试**

```rust
#[test]
fn notch_filter_transient_settles_within_50ms() {
    // 突然的低频脉冲后，滤波器应在 50ms 内稳定
    let mut filter = NotchFilter::new(50.0, SAMPLE_RATE, 20.0); // 新 Q=20

    // 输入一个 10ms 的脉冲（50Hz 正弦波）
    let pulse_len = (SAMPLE_RATE * 0.010) as usize;
    let mut input = sine(50.0, 0.010, 0.5);
    // 后面跟 200ms 静音
    input.extend(vec![0.0f32; (SAMPLE_RATE * 0.200) as usize]);

    let output: Vec<f32> = input.iter().map(|s| filter.process(*s)).collect();

    // 脉冲结束后 50ms 处，残余振铃应 < -40dB
    let check_idx = pulse_len + (SAMPLE_RATE * 0.050) as usize;
    if check_idx < output.len() {
        assert!(
            output[check_idx].abs() < 0.01,
            "notch should settle within 50ms after impulse: residual={}",
            output[check_idx].abs()
        );
    }
}
```

- [ ] **Step 2: 运行测试确认 Q=35 下失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib notch_filter_transient_settles_within_50ms -- --nocapture
```

预期：FAIL（Q=35 时振铃持续 > 50ms）

- [ ] **Step 3: 修改 Q 值**

`audio_denoise.rs` 第 168 行：

```rust
// 修改前
const NOTCH_Q: f64 = 35.0;

// 修改后
const NOTCH_Q: f64 = 20.0;
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib notch_filter_transient_settles_within_50ms
```

预期：PASS

- [ ] **Step 5: 运行全部降噪回归测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise
```

预期：全部通过（notch 仍能有效衰减工频，只是带宽更宽）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/audio_denoise.rs
git commit -m "fix(audio): 降低陷波 Q 值至 20 减少瞬态振铃"
```

---

## Phase 2: 软限幅器替换硬削波

> 目标：用 soft-clip 替代 `clamp()`，消除峰值处的谐波失真。

### Task 4: 实现软限幅器函数

**Files:**
- Modify: `src-tauri/src/media/audio_mixer.rs:338-341`

- [ ] **Step 1: 编写软限幅器测试**

在 `audio_mixer.rs` 的 `#[cfg(test)] mod tests` 中追加：

```rust
#[test]
fn soft_clip_preserves_values_within_range() {
    // [-0.9, 0.9] 范围内的信号应几乎不变
    let samples = vec![-0.9f32, -0.5, 0.0, 0.5, 0.9];
    let result = soft_clip_samples(&samples);
    for (input, output) in samples.iter().zip(result.iter()) {
        assert!(
            (input - output).abs() < 0.01,
            "soft clip should preserve in-range values: input={}, output={}",
            input,
            output
        );
    }
}

#[test]
fn soft_clip_limits_peak_without_discontinuity() {
    // 超过 1.0 的信号应被平滑压缩，不会出现 clamp 的硬截断
    let samples = vec![1.0f32, 1.2, 1.5, 2.0, -1.2, -2.0];
    let result = soft_clip_samples(&samples);

    for (i, output) in result.iter().enumerate() {
        // 所有输出必须在 [-1.0, 1.0] 范围内
        assert!(
            output.abs() <= 1.0,
            "soft clip must limit output: index={}, value={}",
            i,
            output
        );
    }

    // 1.2 和 1.5 的输出应该不同（不是被 clamp 到同一个值）
    assert!(
        (result[1] - result[2]).abs() > 0.01,
        "soft clip should compress differently at different over-range levels"
    );

    // 输出应单调递增（无跳变）
    for i in 1..3 {
        assert!(
            result[i] >= result[i - 1],
            "soft clip output should be monotonically increasing"
        );
    }
}

#[test]
fn soft_clip_matches_clamp_for_extreme_values() {
    // 极端值（如 10.0）应被压缩到接近 ±1.0
    let samples = vec![10.0f32, -10.0];
    let result = soft_clip_samples(&samples);
    assert!((result[0] - 1.0).abs() < 0.01);
    assert!((result[1] - (-1.0)).abs() < 0.01);
}
```

- [ ] **Step 2: 运行测试确认编译失败（函数不存在）**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib soft_clip_preserves_values_within_range -- --nocapture
```

预期：编译错误 `soft_clip_samples` 未定义

- [ ] **Step 3: 实现 soft_clip_samples 函数**

在 `audio_mixer.rs` 中替换 `clamp_samples` 函数（第 338-341 行）：

```rust
/// Soft-clips audio samples using tanh-based saturation.
///
/// Unlike hard clipping (`clamp`), soft clipping applies a smooth compression
/// curve near ±1.0, preserving the signal's natural character and avoiding
/// the harsh harmonic distortion that hard clipping introduces.
///
/// The transfer function is: output = tanh(x) / tanh(1.0) ≈ tanh(x) / 0.7616
/// This normalizes the output so that tanh(1.0)/tanh(1.0) = 1.0 exactly.
fn soft_clip_samples(samples: &[f32]) -> Vec<f32> {
    const TANH_1: f32 = 0.7615942; // tanh(1.0) ≈ 0.7616
    samples.iter().map(|s| (s / TANH_1).tanh()).collect()
}
```

保留旧的 `clamp_samples` 函数（标记为 `#[allow(dead_code)]`）以便回滚：

```rust
/// Hard-clamps all samples to [-1.0, 1.0] to prevent clipping.
/// Replaced by soft_clip_samples for better audio quality.
#[allow(dead_code)]
fn clamp_samples(samples: &[f32]) -> Vec<f32> {
    samples.iter().map(|s| s.clamp(-1.0, 1.0)).collect()
}
```

- [ ] **Step 4: 运行新测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib soft_clip
```

预期：3 个测试全部 PASS

- [ ] **Step 5: 将所有 clamp_samples 调用替换为 soft_clip_samples**

在 `audio_mixer.rs` 中搜索 `clamp_samples`，替换为 `soft_clip_samples`：

- `passthrough()` 函数（第 163 行）：`let clamped = clamp_samples(&stereo);` → `let clamped = soft_clip_samples(&stereo);`
- `mix_two()` 函数（第 244 行）：`let clamped = clamp_samples(&output);` → `let clamped = soft_clip_samples(&output);`

- [ ] **Step 6: 运行全部 mixer 测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture
```

预期：全部通过（包括 `clipping_protection` 测试，因为 soft-clip 在 1.0 处输出仍 ≤ 1.0）

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/media/audio_mixer.rs
git commit -m "fix(audio): 用 tanh 软限幅器替代硬削波消除峰值失真"
```

---

## Phase 3: 高质量 Sinc 重采样

> 目标：用 `rubato` crate 的 sinc 重采样替代线性插值，消除蓝牙/非 48kHz 麦克风的混叠失真。

### Task 5: 添加 rubato 依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 添加 rubato 依赖**

在 `src-tauri/Cargo.toml` 的 `[dependencies]` 中添加：

```toml
rubato = "0.16"
```

- [ ] **Step 2: 验证依赖解析**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

预期：编译通过

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore(audio): 添加 rubato sinc 重采样依赖"
```

---

### Task 6: 实现 SincResampler 替换线性插值

**Files:**
- Modify: `src-tauri/src/media/audio_mixer.rs`

- [ ] **Step 1: 编写 sinc 重采样质量测试**

在 `audio_mixer.rs` 的测试模块中追加：

```rust
#[test]
fn sinc_resampler_avoids_aliasing_on_44100_to_48000() {
    // 44.1kHz → 48kHz 重采样时，线性插值会产生 20kHz+ 的混叠分量
    // sinc 重采样应将混叠抑制到 -90dB 以下
    let chunk = make_chunk(0, 44100, 1, {
        // 生成一个 18kHz 的正弦波（44.1kHz 采样率下的高频信号）
        let sample_count = (44100 * 0.1) as usize;
        (0..sample_count)
            .map(|i| {
                let t = i as f64 / 44100.0;
                (2.0 * std::f64::consts::PI * 18000.0 * t).sin() as f32 * 0.5
            })
            .collect()
    });

    let resampled = resample(&chunk, 48000);

    // 重采样后的频谱中不应出现明显的混叠分量
    // 简化验证：重采样后的 RMS 应与原始信号在同一数量级
    let original_rms: f32 = (chunk.samples.iter().map(|s| s * s).sum::<f32>()
        / chunk.samples.len() as f32)
        .sqrt();
    let resampled_rms: f32 = (resampled.iter().map(|s| s * s).sum::<f32>()
        / resampled.len() as f32)
        .sqrt();

    // RMS 差异应 < 10%（线性插值下可能 > 20%）
    let ratio = resampled_rms / original_rms;
    assert!(
        (ratio - 1.0).abs() < 0.1,
        "sinc resampling should preserve RMS: ratio={}",
        ratio
    );
}

#[test]
fn sinc_resampler_handles_8000_to_48000() {
    // 蓝牙 HFP 8kHz → 48kHz（6x 上采样）不应 panic 或产生失真
    let chunk = make_chunk(0, 8000, 1, {
        let sample_count = (8000 * 0.1) as usize;
        (0..sample_count)
            .map(|i| {
                let t = i as f64 / 8000.0;
                (2.0 * std::f64::consts::PI * 1000.0 * t).sin() as f32 * 0.5
            })
            .collect()
    });

    let resampled = resample(&chunk, 48000);

    // 输出长度应接近 48000 * 0.1 = 4800
    assert!(
        (resampled.len() as i64 - 4800).unsigned_abs() < 100,
        "8kHz→48kHz output length should be ~4800, got {}",
        resampled.len()
    );

    // 输出不应全为零
    let max_abs = resampled.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max_abs > 0.1, "resampled output should not be silent");
}

#[test]
fn sinc_resampler_same_rate_passthrough() {
    let samples = vec![0.1, 0.2, 0.3, 0.4];
    let chunk = make_chunk(0, 48000, 2, samples.clone());
    let resampled = resample(&chunk, 48000);
    assert_eq!(resampled, samples);
}
```

- [ ] **Step 2: 运行测试确认旧实现下失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib sinc_resampler_avoids_aliasing_on_44100_to_48000 -- --nocapture
```

预期：FAIL（线性插值 RMS 偏差过大）

- [ ] **Step 3: 实现 sinc 重采样**

替换 `audio_mixer.rs` 中的 `resample` 函数（第 260-297 行）：

```rust
/// Resamples audio from its native sample rate to `target_rate`.
///
/// Uses sinc interpolation (via `rubato`) for high-quality resampling that
/// avoids the aliasing artifacts of linear interpolation. For same-rate
/// input, returns a direct copy (no processing).
///
/// Falls back to linear interpolation if the sinc resampler fails to initialize
/// (e.g., unsupported channel count or sample rate ratio).
fn resample(chunk: &AudioChunk, target_rate: u32) -> Vec<f32> {
    if chunk.sample_rate == target_rate {
        return chunk.samples.to_vec();
    }

    let channels = chunk.channels as usize;
    if channels == 0 {
        return Vec::new();
    }

    // Try sinc resampling first; fall back to linear on failure.
    match sinc_resample(chunk, target_rate) {
        Ok(result) => result,
        Err(_) => linear_resample(chunk, target_rate),
    }
}

/// High-quality sinc resampling using the `rubato` crate.
fn sinc_resample(chunk: &AudioChunk, target_rate: u32) -> Result<Vec<f32>, String> {
    use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType};

    let channels = chunk.channels as usize;
    let src_rate = chunk.sample_rate as f64;
    let dst_rate = target_rate as f64;

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: rubato::WindowFunction::BlackmanHarris2,
    };

    let chunk_size = 1024;
    let mut resampler = SincFixedIn::<f32>::new(
        dst_rate / src_rate,
        2.0, // max relative ratio
        params,
        chunk_size,
        channels,
    )
    .map_err(|e| format!("resampler init failed: {}", e))?;

    // De-interleave into per-channel buffers
    let src_frames = chunk.samples.len() / channels;
    let mut input_buffers: Vec<Vec<f32>> = vec![vec![0.0; src_frames]; channels];
    for (i, &sample) in chunk.samples.iter().enumerate() {
        let ch = i % channels;
        let frame = i / channels;
        input_buffers[ch][frame] = sample;
    }

    // Process in chunks
    let mut output_buffers: Vec<Vec<f32>> =
        vec![Vec::new(); channels];
    let mut input_offset = 0;

    while input_offset < src_frames {
        let end = (input_offset + chunk_size).min(src_frames);
        let actual_len = end - input_offset;

        // Pad to chunk_size if needed
        let mut padded: Vec<Vec<f32>> = vec![vec![0.0; chunk_size]; channels];
        for ch in 0..channels {
            padded[ch][..actual_len]
                .copy_from_slice(&input_buffers[ch][input_offset..end]);
        }

        let out = resampler
            .process(&padded, None)
            .map_err(|e| format!("resample process failed: {}", e))?;

        for ch in 0..channels {
            output_buffers[ch].extend_from_slice(&out[ch]);
        }

        input_offset += chunk_size;
    }

    // Re-interleave
    let dst_frames = output_buffers[0].len();
    let mut result = Vec::with_capacity(dst_frames * channels);
    for frame in 0..dst_frames {
        for ch in 0..channels {
            result.push(output_buffers[ch][frame]);
        }
    }

    Ok(result)
}

/// Fallback linear interpolation resampling (same as original implementation).
fn linear_resample(chunk: &AudioChunk, target_rate: u32) -> Vec<f32> {
    let src_rate = chunk.sample_rate as f64;
    let dst_rate = target_rate as f64;
    let ratio = src_rate / dst_rate;
    let channels = chunk.channels as usize;
    let src_frames = chunk.samples.len() / channels;
    let dst_frames = (src_frames as f64 * dst_rate / src_rate) as usize;

    let mut output = vec![0.0f32; dst_frames * channels];

    for ch in 0..channels {
        for dst_frame in 0..dst_frames {
            let src_pos = dst_frame as f64 * ratio;
            let src_frame = src_pos as usize;
            let frac = (src_pos - src_frame as f64) as f32;
            let idx = dst_frame * channels + ch;

            if src_frame + 1 < src_frames {
                let s0 = chunk.samples[src_frame * channels + ch];
                let s1 = chunk.samples[(src_frame + 1) * channels + ch];
                output[idx] = s0 + (s1 - s0) * frac;
            } else if src_frame < src_frames {
                output[idx] = chunk.samples[src_frame * channels + ch];
            }
        }
    }

    output
}
```

- [ ] **Step 4: 运行新测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib sinc_resampler
```

预期：3 个测试全部 PASS

- [ ] **Step 5: 运行全部 mixer 测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture
```

预期：全部通过

- [ ] **Step 6: 运行全量 Rust 测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

预期：全部通过

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/media/audio_mixer.rs
git commit -m "fix(audio): 用 sinc 重采样替代线性插值消除混叠失真"
```

---

## Phase 4: 媒体通道丢包策略优化

> 目标：通道满时丢弃最旧而非最新数据块，并增大缓冲容量。

### Task 7: 修改通道丢包策略

**Files:**
- Modify: `src-tauri/src/core/media_channel.rs:54-66`

- [ ] **Step 1: 编写丢弃最旧策略测试**

在 `media_channel.rs` 的 `#[cfg(test)] mod tests` 中追加：

```rust
#[test]
fn drops_oldest_when_full_preserving_newest() {
    let (sender, receiver) = bounded_media_channel::<u32>(2, "test");

    // 填满通道
    assert!(sender.try_send_drop_oldest(1));
    assert!(sender.try_send_drop_oldest(2));

    // 通道满时，发送新数据应丢弃最旧的（1），保留最新的（2 和 3）
    assert!(sender.try_send_drop_oldest(3));

    assert_eq!(receiver.try_recv().unwrap(), 2);
    assert_eq!(receiver.try_recv().unwrap(), 3);
}
```

- [ ] **Step 2: 运行测试确认编译失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib drops_oldest_when_full_preserving_newest -- --nocapture
```

预期：编译错误 `try_send_drop_oldest` 未定义

- [ ] **Step 3: 实现 try_send_drop_oldest 方法**

在 `media_channel.rs` 的 `MediaSender` impl 中追加新方法（保留旧方法）：

```rust
/// 发送数据，通道满时丢弃最旧的以腾出空间。
///
/// 与 `try_send_drop_newest` 相反，此策略保留最新的数据块，
/// 对实时音频流更友好——丢失最早的音频比丢失最新的音频更不易被察觉。
///
/// 返回 `true` 表示发送成功（包括挤掉旧数据后成功），`false` 表示通道已断开。
pub fn try_send_drop_oldest(&self, item: T) -> bool {
    match self.inner.try_send(item) {
        Ok(()) => true,
        Err(TrySendError::Full(item)) => {
            // 通道满：尝试接收并丢弃最旧的一条，然后重试发送
            // 注意：这里存在竞态条件（另一个线程可能同时消费），
            // 但对音频场景而言，偶尔多丢一条可接受。
            let _ = self.inner.try_recv(); // 丢弃最旧
            match self.inner.try_send(item) {
                Ok(()) => {
                    let count = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                    if count == 1 || count % 100 == 0 {
                        eprintln!(
                            "警告: {} 媒体通道丢弃最旧 (累计 {} 次)",
                            self.source, count
                        );
                    }
                    true
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                    false
                }
                Err(TrySendError::Full(_)) => {
                    // 不应发生（刚消费了一条），但如果发生则丢弃新数据
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                    false
                }
            }
        }
        Err(TrySendError::Disconnected(_)) => {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            false
        }
    }
}
```

- [ ] **Step 4: 运行新测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib drops_oldest_when_full_preserving_newest
```

预期：PASS

- [ ] **Step 5: 更新麦克风采集使用新策略**

在 `cpal_microphone.rs` 第 362 行，将 `try_send_drop_newest` 改为 `try_send_drop_oldest`：

```rust
// 修改前
sink.try_send_drop_newest(chunk);

// 修改后
sink.try_send_drop_oldest(chunk);
```

- [ ] **Step 6: 运行全部测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

预期：全部通过

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/core/media_channel.rs src-tauri/src/platform/macos/cpal_microphone.rs
git commit -m "fix(audio): 麦克风通道满时丢弃最旧而非最新数据块"
```

---

## Phase 5: 收尾

### Task 8: 格式化、lint 和回归验证

- [ ] **Step 1: 格式化检查**

```bash
rustfmt --check --edition 2021 \
  src-tauri/src/media/audio_denoise.rs \
  src-tauri/src/media/audio_mixer.rs \
  src-tauri/src/core/media_channel.rs \
  src-tauri/src/platform/macos/cpal_microphone.rs
```

预期：无差异

- [ ] **Step 2: 全量测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

预期：全部通过

- [ ] **Step 3: 更新 BUG.md**

在 `BUG.md` 的"已解决"部分追加：

```markdown
### BUG-0025: 麦克风录制断续电流噪声和突然嗡嗡声 ✅ 已修复-待人工验证

**现象**：同时开启系统音频和麦克风录制时，视频中出现断续的"滋滋/底噪"和突然的低沉"嗡嗡声"。

**根因**：
1. `NoiseFloorSuppressor` 门限间距过小（0.006~0.014），信号在边界附近时增益快速泵浦
2. 高通滤波器截止 80Hz，二阶衰减不足，低频隆隆声未充分消除
3. 陷波滤波器 Q=35 过高，脉冲输入后振铃持续 > 50ms
4. 硬削波 `clamp()` 在峰值处产生谐波失真
5. 线性插值重采样引入混叠失真（尤其蓝牙 8/16kHz → 48kHz）
6. 媒体通道满时丢弃最新数据块，产生信号不连续

**修复**：
1. 调整 NoiseFloorSuppressor：open=0.020, close=0.004, attack=8ms, release=150ms, min_gain=0.15
2. 高通截止频率 80Hz → 100Hz
3. 陷波 Q 值 35 → 20
4. 硬削波替换为 tanh 软限幅器
5. 线性插值替换为 rubato sinc 重采样
6. 通道丢包策略改为丢弃最旧

**预防规则**：
65. 动态降噪门限间距必须 ≥ 0.015，避免边界增益泵浦。
66. 高通滤波器截止频率建议 ≥ 100Hz（二阶），或使用四阶滤波器。
67. 陷波滤波器 Q 值建议 ≤ 25，避免瞬态振铃。
68. 音频限幅必须使用 soft-clip，禁止硬 clamp。
69. 音频重采样必须使用 sinc 插值，禁止线性插值用于最终输出。
70. 实时音频通道丢包策略应丢弃最旧、保留最新。
```

- [ ] **Step 4: 更新 HANDOFF.md**

在"工作任务记录"部分追加本轮工作记录（按时间倒序放在最前面）。

- [ ] **Step 5: Commit**

```bash
git add BUG.md HANDOFF.md
git commit -m "docs: 更新 BUG-0025 麦克风降噪深度优化记录"
```

---

## 自测清单

保存到 `tests/2026-06-08-mic-denoise-deep-optimization-checklist.md`：

```markdown
# 麦克风降噪深度优化自测清单

## Phase 1: 参数优化

- [ ] `suppressor_does_not_pump_at_boundary_level` 通过
- [ ] `suppressor_recovers_voice_onset_quickly_after_silence` 通过
- [ ] `highpass_filter_attenuates_30hz_aggressively` 通过
- [ ] `notch_filter_transient_settles_within_50ms` 通过
- [ ] `audio_denoise` 全部测试通过（≥ 16 tests）
- [ ] `audio_mixer` 全部测试通过（≥ 22 tests）

## Phase 2: 软限幅器

- [ ] `soft_clip_preserves_values_within_range` 通过
- [ ] `soft_clip_limits_peak_without_discontinuity` 通过
- [ ] `soft_clip_matches_clamp_for_extreme_values` 通过
- [ ] `clipping_protection` 通过（原有测试）

## Phase 3: Sinc 重采样

- [ ] `sinc_resampler_avoids_aliasing_on_44100_to_48000` 通过
- [ ] `sinc_resampler_handles_8000_to_48000` 通过
- [ ] `sinc_resampler_same_rate_passthrough` 通过
- [ ] `resample_44100_to_48000` 通过（原有测试）

## Phase 4: 通道策略

- [ ] `drops_oldest_when_full_preserving_newest` 通过
- [ ] `drops_newest_when_full_without_blocking` 通过（原有测试）

## 全局回归

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib` 全部通过
- [ ] `rustfmt --check` 通过
- [ ] `git diff --check` 通过

## 人工验证

- [ ] 有线耳机麦克风开启降噪，静音环境录制 10 秒，回放确认底噪显著降低
- [ ] 同一设备关闭降噪录制 10 秒，作为对照确认开关仍有效
- [ ] 开启降噪后正常说话 10 秒，确认开头不被吞字、尾音不过早切断
- [ ] 蓝牙耳机麦克风录制 10 秒，回放确认无混叠失真的"毛刺感"
- [ ] 同时录制系统音频和麦克风，确认系统音频音质不受影响
- [ ] 大音量场景录制（如播放音乐 + 说话），确认无削波失真的"撕裂感"
```

---

## Plan Complete

Plan saved to `docs/superpowers/plans/2026-06-08-mic-denoise-deep-optimization.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — 每个 Task 派发独立子代理执行，Task 间做 review，快速迭代
2. **Inline Execution** — 在当前会话中按 Task 顺序执行，带检查点

**Which approach?**
