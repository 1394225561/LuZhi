# 麦克风高通滤波器实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为麦克风采集添加高通滤波器，滤除 50Hz/60Hz 工频干扰（电流声），并提供用户可配置的降噪开关。

**Architecture:** 在 `SimpleAudioMixer` 层添加可选的高通滤波器处理。滤波器仅应用于麦克风通道（系统音频保持原样），使用二阶 Butterworth IIR 高通滤波器，截止频率 80Hz。通过 `AudioConfig` 传递配置，前端录制面板提供降噪开关。

**Tech Stack:** Rust（IIR 滤波器实现）、Tauri（命令/事件）、React + shadcn/ui（前端开关）

---

## 文件结构

| 文件 | 操作 | 说明 |
|------|------|------|
| `src-tauri/src/media/audio_denoise.rs` | 新增 | 高通滤波器实现 |
| `src-tauri/src/media/audio_mixer.rs` | 修改 | 集成滤波器到混音管道 |
| `src-tauri/src/media/mod.rs` | 修改 | 注册新模块 |
| `src-tauri/src/core/capture.rs` | 修改 | `AudioConfig` 添加降噪配置字段 |
| `src-tauri/src/platform/macos_service.rs` | 修改 | 传递降噪配置到 mixer |
| `src-tauri/src/lib.rs` | 修改 | 更新 Tauri 命令参数 |
| `src/lib/tauri.ts` | 修改 | 更新 TypeScript 类型定义 |
| `src/components/recording-panel.tsx` | 修改 | 添加降噪开关 UI |
| `src/App.tsx` | 修改 | 传递降噪配置到后端 |

---

## Task 1: 实现高通滤波器模块

**Files:**
- Create: `src-tauri/src/media/audio_denoise.rs`
- Test: 同文件内 `#[cfg(test)] mod tests`

### 背景

电流声通常是 50Hz（中国）或 60Hz（美国）的工频干扰及其谐波。二阶 Butterworth 高通滤波器可以有效滤除这些低频噪声，同时保留人声（通常 100Hz 以上）。

**滤波器设计参数：**
- 类型：二阶 Butterworth 高通滤波器
- 截止频率：80Hz（低于 50Hz 工频，留出余量）
- 采样率：48kHz（与项目统一输出采样率一致）
- 实现：Direct Form II Transposed（数值稳定性好）

**数学推导：**

二阶 Butterworth 高通滤波器的传递函数：

```
H(z) = (b0 + b1*z^-1 + b2*z^-2) / (1 + a1*z^-1 + a2*z^-2)
```

其中系数计算：
```
ωc = 2π * fc / fs
α = sin(ωc) / (2 * Q)  // Q = 1/√2 for Butterworth

b0 = (1 + cos(ωc)) / 2
b1 = -(1 + cos(ωc))
b2 = (1 + cos(ωc)) / 2
a0 = 1 + α
a1 = -2 * cos(ωc)
a2 = 1 - α

// 归一化
b0 /= a0; b1 /= a0; b2 /= a0;
a1 /= a0; a2 /= a0;
```

- [ ] **Step 1: 编写高通滤波器结构体和构造函数**

```rust
// src-tauri/src/media/audio_denoise.rs

use std::f64::consts::PI;

/// 二阶 Butterworth 高通滤波器（Direct Form II Transposed）。
///
/// 用于滤除麦克风采集中的低频噪声（如 50Hz/60Hz 工频干扰）。
/// 滤波器仅应用于麦克风通道，系统音频保持原样。
///
/// 参考：https://www.w3.org/2011/audio/audio-eq-cookbook.html
pub struct HighpassFilter {
    /// 滤波器系数
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    /// 状态缓冲（Direct Form II Transposed）
    z1: f64,
    z2: f64,
}

impl HighpassFilter {
    /// 创建高通滤波器。
    ///
    /// # 参数
    /// - `cutoff_hz`: 截止频率（Hz），建议 80Hz
    /// - `sample_rate`: 采样率（Hz），通常 48000
    pub fn new(cutoff_hz: f64, sample_rate: f64) -> Self {
        assert!(cutoff_hz > 0.0, "截止频率必须大于 0");
        assert!(sample_rate > 0.0, "采样率必须大于 0");
        assert!(cutoff_hz < sample_rate / 2.0, "截止频率必须小于奈奎斯特频率");

        // Butterworth Q = 1/√2
        let q = 1.0 / 2.0_f64.sqrt();
        let omega_c = 2.0 * PI * cutoff_hz / sample_rate;
        let alpha = omega_c.sin() / (2.0 * q);

        let cos_omega_c = omega_c.cos();

        // 计算未归一化系数
        let b0 = (1.0 + cos_omega_c) / 2.0;
        let b1 = -(1.0 + cos_omega_c);
        let b2 = (1.0 + cos_omega_c) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega_c;
        let a2 = 1.0 - alpha;

        // 归一化
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// 处理单个样本。
    pub fn process(&mut self, input: f32) -> f32 {
        let x = input as f64;
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y as f32
    }

    /// 重置滤波器状态。
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}
```

- [ ] **Step 2: 编写滤波器单元测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highpass_filter_removes_dc_offset() {
        let mut filter = HighpassFilter::new(80.0, 48000.0);

        // 输入恒定 DC 偏移（0.5），高通滤波器应将其衰减到接近 0
        let mut output = 0.0;
        for _ in 0..4800 {
            output = filter.process(0.5);
        }

        // 经过 100ms（4800 样本），DC 应被充分衰减
        assert!(
            output.abs() < 0.01,
            "DC offset should be attenuated, got {}",
            output
        );
    }

    #[test]
    fn highpass_filter_preserves_high_frequency() {
        let mut filter = HighpassFilter::new(80.0, 48000.0);

        // 输入 1kHz 正弦波
        let freq = 1000.0;
        let sample_rate = 48000.0;
        let mut max_output = 0.0f32;

        for i in 0..4800 {
            let t = i as f64 / sample_rate;
            let input = (2.0 * PI * freq * t).sin() as f32;
            let output = filter.process(input);
            max_output = max_output.max(output.abs());
        }

        // 1kHz 应几乎无衰减（增益 > 0.9）
        assert!(
            max_output > 0.9,
            "1kHz signal should pass through, got max {}",
            max_output
        );
    }

    #[test]
    fn highpass_filter_attenuates_50hz() {
        let mut filter = HighpassFilter::new(80.0, 48000.0);

        // 输入 50Hz 正弦波（工频干扰）
        let freq = 50.0;
        let sample_rate = 48000.0;
        let mut max_output = 0.0f32;

        // 运行足够长时间让滤波器稳定
        for i in 0..48000 {
            let t = i as f64 / sample_rate;
            let input = (2.0 * PI * freq * t).sin() as f32;
            let output = filter.process(input);
            if i > 4800 {
                // 跳过前 100ms（瞬态响应）
                max_output = max_output.max(output.abs());
            }
        }

        // 50Hz 应被显著衰减（增益 < 0.3）
        assert!(
            max_output < 0.3,
            "50Hz should be attenuated, got max {}",
            max_output
        );
    }

    #[test]
    fn highpass_filter_attenuates_60hz() {
        let mut filter = HighpassFilter::new(80.0, 48000.0);

        let freq = 60.0;
        let sample_rate = 48000.0;
        let mut max_output = 0.0f32;

        for i in 0..48000 {
            let t = i as f64 / sample_rate;
            let input = (2.0 * PI * freq * t).sin() as f32;
            let output = filter.process(input);
            if i > 4800 {
                max_output = max_output.max(output.abs());
            }
        }

        assert!(
            max_output < 0.3,
            "60Hz should be attenuated, got max {}",
            max_output
        );
    }

    #[test]
    fn highpass_filter_reset() {
        let mut filter = HighpassFilter::new(80.0, 48000.0);

        // 处理一些样本
        for _ in 0..1000 {
            filter.process(0.5);
        }

        filter.reset();

        // 重置后状态应为 0
        assert_eq!(filter.z1, 0.0);
        assert_eq!(filter.z2, 0.0);
    }

    #[test]
    #[should_panic(expected = "截止频率必须大于 0")]
    fn highpass_filter_rejects_zero_cutoff() {
        HighpassFilter::new(0.0, 48000.0);
    }

    #[test]
    #[should_panic(expected = "截止频率必须小于奈奎斯特频率")]
    fn highpass_filter_rejects_nyquist_cutoff() {
        HighpassFilter::new(24000.0, 48000.0);
    }
}
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise`
Expected: 全部 8 个测试通过

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/media/audio_denoise.rs
git commit -m "feat(audio): 新增高通滤波器模块

实现二阶 Butterworth 高通滤波器，用于滤除麦克风采集中的
低频噪声（50Hz/60Hz 工频干扰）。

- 截止频率 80Hz，采样率 48kHz
- Direct Form II Transposed 实现（数值稳定性好）
- 8 个单元测试覆盖：DC 衰减、高频保留、50Hz/60Hz 衰减"
```

---

## Task 2: 扩展 AudioConfig 添加降噪配置

**Files:**
- Modify: `src-tauri/src/core/capture.rs:52-65`
- Test: 同文件内已有测试

- [ ] **Step 1: 添加降噪模式枚举和配置字段**

```rust
// src-tauri/src/core/capture.rs

/// 音频降噪模式。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DenoiseMode {
    /// 不降噪
    None,
    /// 高通滤波器（滤除低频噪声）
    Highpass,
}

impl Default for DenoiseMode {
    fn default() -> Self {
        Self::None
    }
}

/// Configuration for audio capture.
#[derive(Clone, Debug)]
pub struct AudioConfig {
    /// Whether to capture system audio output.
    pub capture_system_audio: bool,
    /// Whether to capture microphone input.
    pub capture_microphone: bool,
    /// Specific microphone device name; `None` uses system default.
    pub microphone_device: Option<String>,
    /// Target sample rate in Hz (e.g., 48000).
    pub sample_rate: u32,
    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channels: u16,
    /// 降噪模式。
    pub denoise_mode: DenoiseMode,
}
```

- [ ] **Step 2: 更新所有 AudioConfig 构造点**

搜索项目中所有 `AudioConfig { ... }` 构造点，添加 `denoise_mode: DenoiseMode::default()`。

需要检查的文件：
- `src-tauri/src/platform/macos_service.rs`
- `src-tauri/src/lib.rs`

- [ ] **Step 3: 运行编译验证**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/core/capture.rs
git commit -m "feat(audio): AudioConfig 添加降噪配置字段

新增 DenoiseMode 枚举（None/Highpass）和 AudioConfig.denoise_mode 字段，
为麦克风降噪功能提供配置入口。"
```

---

## Task 3: 集成高通滤波器到 SimpleAudioMixer

**Files:**
- Modify: `src-tauri/src/media/audio_mixer.rs`
- Test: 同文件内已有测试 + 新增测试

- [ ] **Step 1: 修改 AudioMixer trait 支持降噪配置**

```rust
// src-tauri/src/media/audio_mixer.rs

use crate::core::capture::DenoiseMode;
use crate::core::frame::{AudioChunk, MediaTimestamp, MixedAudioChunk};

/// Unified output sample rate for mixed audio (48 kHz).
const MIXED_SAMPLE_RATE: u32 = 48_000;

/// Unified output channel count (stereo).
const MIXED_CHANNELS: u16 = 2;

/// Trait abstracting audio mixing for testability.
pub trait AudioMixer: Send {
    fn mix(
        &self,
        system: Option<&AudioChunk>,
        mic: Option<&AudioChunk>,
    ) -> AppResult<MixedAudioChunk>;
}
```

- [ ] **Step 2: 修改 SimpleAudioMixer 持有滤波器状态**

由于 `AudioMixer::mix` 接受 `&self`（不可变引用），滤波器需要内部可变性。使用 `std::sync::Mutex` 包装滤波器状态。

```rust
use std::sync::Mutex;
use crate::media::audio_denoise::HighpassFilter;

/// Real-time audio mixer that combines system audio and microphone input.
pub struct SimpleAudioMixer {
    /// 高通滤波器（仅在 denoise_mode=Highpass 时使用）
    highpass_filters: Mutex<Option<Vec<HighpassFilter>>>,
    /// 降噪模式
    denoise_mode: DenoiseMode,
}

impl SimpleAudioMixer {
    pub fn new(denoise_mode: DenoiseMode) -> Self {
        Self {
            highpass_filters: Mutex::new(None),
            denoise_mode,
        }
    }

    /// 获取或初始化滤波器（每个通道一个）。
    fn get_or_init_filters(&self, channels: usize) -> Option<Vec<HighpassFilter>> {
        if self.denoise_mode != DenoiseMode::Highpass {
            return None;
        }

        let mut filters = self.highpass_filters.lock().unwrap();
        if filters.is_none() {
            *filters = Some(
                (0..channels)
                    .map(|_| HighpassFilter::new(80.0, MIXED_SAMPLE_RATE as f64))
                    .collect(),
            );
        }
        filters.clone()
    }
}

impl Default for SimpleAudioMixer {
    fn default() -> Self {
        Self::new(DenoiseMode::default())
    }
}
```

- [ ] **Step 3: 添加 apply_highpass 函数**

```rust
/// 对音频样本应用高通滤波器。
///
/// 滤波器按通道独立处理（每个通道有独立的状态）。
fn apply_highpass(samples: &[f32], channels: u16, filters: &mut [HighpassFilter]) -> Vec<f32> {
    let channels = channels as usize;
    let mut output = Vec::with_capacity(samples.len());

    for (i, &sample) in samples.iter().enumerate() {
        let ch = i % channels;
        output.push(filters[ch].process(sample));
    }

    output
}
```

- [ ] **Step 4: 修改 passthrough 和 mix_two 函数集成滤波器**

```rust
/// Single-source passthrough: resample + convert to stereo if needed.
fn passthrough(chunk: &AudioChunk, mixer: &SimpleAudioMixer) -> AppResult<MixedAudioChunk> {
    validate_audio_chunk(chunk)?;
    let resampled = resample(chunk, MIXED_SAMPLE_RATE);

    // 仅对麦克风通道应用降噪（通过检查 denoise_mode）
    let filtered = if mixer.denoise_mode == DenoiseMode::Highpass {
        if let Some(mut filters) = mixer.get_or_init_filters(chunk.channels as usize) {
            apply_highpass(&resampled, chunk.channels, &mut filters)
        } else {
            resampled
        }
    } else {
        resampled
    };

    let stereo = to_stereo(&filtered, chunk.channels);
    let clamped = clamp_samples(&stereo);

    Ok(MixedAudioChunk {
        timestamp: chunk.timestamp,
        sample_rate: MIXED_SAMPLE_RATE,
        channels: MIXED_CHANNELS,
        samples: clamped.into(),
    })
}

/// Mix two audio sources with timestamp alignment.
fn mix_two(
    system: &AudioChunk,
    mic: &AudioChunk,
    mixer: &SimpleAudioMixer,
) -> AppResult<MixedAudioChunk> {
    validate_audio_chunk(system)?;
    validate_audio_chunk(mic)?;

    let sys_resampled = resample(system, MIXED_SAMPLE_RATE);
    let mic_resampled = resample(mic, MIXED_SAMPLE_RATE);

    // 仅对麦克风通道应用降噪
    let mic_filtered = if mixer.denoise_mode == DenoiseMode::Highpass {
        if let Some(mut filters) = mixer.get_or_init_filters(mic.channels as usize) {
            apply_highpass(&mic_resampled, mic.channels, &mut filters)
        } else {
            mic_resampled
        }
    } else {
        mic_resampled
    };

    let sys_stereo = to_stereo(&sys_resampled, system.channels);
    let mic_stereo = to_stereo(&mic_filtered, mic.channels);

    // ... 其余混音逻辑不变 ...
}
```

- [ ] **Step 5: 修改 mix 方法传递 mixer 引用**

```rust
impl AudioMixer for SimpleAudioMixer {
    fn mix(
        &self,
        system: Option<&AudioChunk>,
        mic: Option<&AudioChunk>,
    ) -> AppResult<MixedAudioChunk> {
        match (system, mic) {
            (Some(sys), Some(mic)) => mix_two(sys, mic, self),
            (Some(sys), None) => passthrough(sys, self),
            (None, Some(mic)) => passthrough(mic, self),
            (None, None) => Err(AppError::AudioMixFailed {
                reason: "系统音频和麦克风均无数据".to_string(),
            }),
        }
    }
}
```

- [ ] **Step 6: 添加降噪集成测试**

```rust
#[test]
fn mixer_with_highpass_removes_dc_offset_from_mic() {
    let mixer = SimpleAudioMixer::new(DenoiseMode::Highpass);

    // 创建带有 DC 偏移的麦克风输入
    let mic = make_chunk(0, 48000, 1, vec![0.5; 4800]);

    let result = mixer.mix(None, Some(&mic)).unwrap();

    // 经过高通滤波，DC 应被衰减
    let avg: f32 = result.samples.iter().sum::<f32>() / result.samples.len() as f32;
    assert!(
        avg.abs() < 0.01,
        "DC offset should be removed, got avg {}",
        avg
    );
}

#[test]
fn mixer_without_highpass_preserves_dc_offset() {
    let mixer = SimpleAudioMixer::new(DenoiseMode::None);

    let mic = make_chunk(0, 48000, 1, vec![0.5; 4800]);

    let result = mixer.mix(None, Some(&mic)).unwrap();

    // 无降噪时，DC 应保持
    let avg: f32 = result.samples.iter().sum::<f32>() / result.samples.len() as f32;
    assert!(
        avg > 0.4,
        "DC offset should be preserved without filter, got avg {}",
        avg
    );
}

#[test]
fn mixer_highpass_only_affects_mic_not_system() {
    let mixer = SimpleAudioMixer::new(DenoiseMode::Highpass);

    // 系统音频带有 DC 偏移
    let sys = make_chunk(0, 48000, 1, vec![0.5; 4800]);
    // 麦克风带有 DC 偏移
    let mic = make_chunk(0, 48000, 1, vec![0.5; 4800]);

    let result = mixer.mix(Some(&sys), Some(&mic)).unwrap();

    // 混音后，系统音频的 DC 应保留（0.5 * 0.5 = 0.25）
    // 麦克风的 DC 应被滤除（接近 0）
    // 总体应接近 0.25
    let avg: f32 = result.samples.iter().sum::<f32>() / result.samples.len() as f32;
    assert!(
        avg > 0.15 && avg < 0.35,
        "System audio DC should be preserved, mic DC should be removed, got avg {}",
        avg
    );
}
```

- [ ] **Step 7: 运行所有测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer`
Expected: 所有测试通过（包括新增的 3 个降噪测试）

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/media/audio_mixer.rs
git commit -m "feat(audio): 集成高通滤波器到混音管道

- SimpleAudioMixer 支持 DenoiseMode 配置
- 仅对麦克风通道应用高通滤波，系统音频保持原样
- 新增 3 个集成测试验证降噪效果"
```

---

## Task 4: 注册新模块到 media/mod.rs

**Files:**
- Modify: `src-tauri/src/media/mod.rs`

- [ ] **Step 1: 添加模块声明**

```rust
// src-tauri/src/media/mod.rs

pub mod audio_denoise;
pub mod audio_mixer;
pub mod audio_synchronizer;
// ... 其余不变 ...
```

- [ ] **Step 2: 运行编译验证**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/media/mod.rs
git commit -m "chore(audio): 注册 audio_denoise 模块"
```

---

## Task 5: 更新 macos_service.rs 传递降噪配置

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs`

- [ ] **Step 1: 找到 AudioConfig 构造点并添加 denoise_mode**

搜索 `AudioConfig {` 找到构造点，添加 `denoise_mode: DenoiseMode::default()` 或从配置中读取。

- [ ] **Step 2: 找到 SimpleAudioMixer::new() 调用点并传递配置**

搜索 `SimpleAudioMixer::new()` 调用点，改为 `SimpleAudioMixer::new(config.denoise_mode)`。

- [ ] **Step 3: 运行编译验证**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/platform/macos_service.rs
git commit -m "feat(audio): macos_service 传递降噪配置到 mixer"
```

---

## Task 6: 更新 Tauri 命令支持降噪配置

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 在 set_audio_config 命令中添加 denoise_mode 参数**

```rust
// src-tauri/src/lib.rs

#[tauri::command]
async fn set_audio_config(
    state: State<'_, AppState>,
    capture_system_audio: bool,
    capture_microphone: bool,
    microphone_device: Option<String>,
    denoise_mode: Option<String>,  // 新增参数
) -> Result<(), String> {
    let mode = match denoise_mode.as_deref() {
        Some("highpass") => DenoiseMode::Highpass,
        _ => DenoiseMode::None,
    };

    // ... 更新配置 ...
}
```

- [ ] **Step 2: 更新 TypeScript 类型定义**

```typescript
// src/lib/tauri.ts

export type DenoiseMode = 'none' | 'highpass';

export interface AudioConfig {
  captureSystemAudio: boolean;
  captureMicrophone: boolean;
  microphoneDevice?: string;
  denoiseMode?: DenoiseMode;
}

export async function setAudioConfig(config: AudioConfig): Promise<void> {
  return invoke('set_audio_config', {
    captureSystemAudio: config.captureSystemAudio,
    captureMicrophone: config.captureMicrophone,
    microphoneDevice: config.microphoneDevice,
    denoiseMode: config.denoiseMode ?? 'none',
  });
}
```

- [ ] **Step 3: 运行编译验证**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/lib.rs src/lib/tauri.ts
git commit -m "feat(audio): Tauri 命令支持降噪配置

- set_audio_config 新增 denoise_mode 参数
- TypeScript 类型同步更新"
```

---

## Task 7: 添加前端降噪开关 UI

**Files:**
- Modify: `src/components/recording-panel.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: 在 RecordingPanel 添加降噪开关**

在录制面板的麦克风控制区域添加降噪开关：

```tsx
// src/components/recording-panel.tsx

import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";

interface RecordingPanelProps {
  // ... 现有 props ...
  denoiseEnabled: boolean;
  onDenoiseChange: (enabled: boolean) => void;
}

export function RecordingPanel({
  // ... 现有 props ...
  denoiseEnabled,
  onDenoiseChange,
}: RecordingPanelProps) {
  return (
    // ... 现有 JSX ...
    <div className="flex items-center justify-between">
      <Label htmlFor="denoise" className="text-sm">
        降噪（去除电流声）
      </Label>
      <Switch
        id="denoise"
        checked={denoiseEnabled}
        onCheckedChange={onDenoiseChange}
      />
    </div>
    // ... 其余 JSX ...
  );
}
```

- [ ] **Step 2: 在 App.tsx 管理降噪状态并传递到后端**

```tsx
// src/App.tsx

const [denoiseEnabled, setDenoiseEnabled] = useState(false);

const handleDenoiseChange = useCallback(async (enabled: boolean) => {
  setDenoiseEnabled(enabled);
  try {
    await setAudioConfig({
      captureSystemAudio: audioConfig.captureSystemAudio,
      captureMicrophone: audioConfig.captureMicrophone,
      microphoneDevice: audioConfig.microphoneDevice,
      denoiseMode: enabled ? 'highpass' : 'none',
    });
  } catch (error) {
    console.error('Failed to update denoise config:', error);
  }
}, [audioConfig]);
```

- [ ] **Step 3: 运行前端测试**

Run: `npm test -- --run`
Expected: 所有测试通过

- [ ] **Step 4: 提交**

```bash
git add src/components/recording-panel.tsx src/App.tsx
git commit -m "feat(ui): 添加麦克风降噪开关

- RecordingPanel 新增降噪 Switch 组件
- App.tsx 管理降噪状态并同步到后端"
```

---

## Task 8: 完整验证与回归测试

**Files:**
- Test: 整个项目

- [ ] **Step 1: 运行 Rust 全量测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 所有测试通过

- [ ] **Step 2: 运行 Rust clippy 检查**

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --lib`
Expected: 无新增 warning

- [ ] **Step 3: 运行 Rust 格式检查**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
Expected: 格式正确

- [ ] **Step 4: 运行前端测试**

Run: `npm test -- --run`
Expected: 所有测试通过

- [ ] **Step 5: 运行前端构建**

Run: `npm run build`
Expected: 构建成功

- [ ] **Step 6: 运行 git diff 检查**

Run: `git diff --check`
Expected: 无 trailing whitespace

- [ ] **Step 7: 提交**

```bash
git add -A
git commit -m "test(audio): 高通滤波器完整回归测试通过"
```

---

## 自测清单

### 功能测试

| # | 测试场景 | 预期结果 | 验证方式 |
|---|---------|---------|---------|
| 1 | 开启降噪，录制带有电流声的麦克风 | 导出视频中电流声被滤除 | 人工听 |
| 2 | 关闭降噪，录制带有电流声的麦克风 | 导出视频中电流声保留 | 人工听 |
| 3 | 开启降噪，录制正常说话 | 人声清晰，无失真 | 人工听 |
| 4 | 开启降噪，录制音乐播放（系统音频） | 音乐不受影响 | 人工听 |
| 5 | 开启降噪，同时录制系统音频和麦克风 | 系统音频原样，麦克风降噪 | 人工听 |

### 单元测试

| # | 测试用例 | 预期 |
|---|---------|------|
| 1 | highpass_filter_removes_dc_offset | DC 衰减到 < 0.01 |
| 2 | highpass_filter_preserves_high_frequency | 1kHz 增益 > 0.9 |
| 3 | highpass_filter_attenuates_50hz | 50Hz 增益 < 0.3 |
| 4 | highpass_filter_attenuates_60hz | 60Hz 增益 < 0.3 |
| 5 | highpass_filter_reset | 重置后状态为 0 |
| 6 | highpass_filter_rejects_zero_cutoff | panic |
| 7 | highpass_filter_rejects_nyquist_cutoff | panic |
| 8 | mixer_with_highpass_removes_dc_offset_from_mic | 麦克风 DC 被滤除 |
| 9 | mixer_without_highpass_preserves_dc_offset | 无降噪时 DC 保留 |
| 10 | mixer_highpass_only_affects_mic_not_system | 系统音频不受影响 |

### UI 测试

| # | 测试场景 | 预期 |
|---|---------|------|
| 1 | 降噪开关默认状态 | 默认关闭 |
| 2 | 点击降噪开关 | 状态切换，调用 setAudioConfig |
| 3 | 开启降噪后开始录制 | 后端使用 Highpass 模式 |

---

## 人工验证门禁

1. **电流声滤除**：使用有线耳机麦克风录制，开启降噪后回放，电流声应明显减弱或消失
2. **人声保真**：开启降噪后录制正常说话，人声应清晰无失真
3. **系统音频不受影响**：同时录制系统音频和麦克风，开启降噪后，系统音频（如音乐）应保持原样
4. **开关切换**：录制过程中切换降噪开关（需要停止并重新录制生效），验证配置正确传递
