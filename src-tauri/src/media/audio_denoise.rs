use std::f64::consts::PI;

/// 二阶 Butterworth 高通滤波器（Direct Form II Transposed）。
///
/// 用于滤除麦克风采集中的低频噪声（如 50Hz/60Hz 工频干扰）。
/// 滤波器仅应用于麦克风通道，系统音频保持原样。
///
/// 参考：https://www.w3.org/2011/audio/audio-eq-cookbook.html
#[derive(Clone)]
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
        assert!(
            cutoff_hz < sample_rate / 2.0,
            "截止频率必须小于奈奎斯特频率"
        );

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
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// 二阶窄带陷波滤波器（notch biquad）。
///
/// 用于压制麦克风中稳定的工频尖峰及低阶谐波。`q` 越高，压制频带越窄，
/// 对相邻人声频段影响越小。
#[derive(Clone)]
pub struct NotchFilter {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl NotchFilter {
    /// 创建陷波滤波器。
    ///
    /// # 参数
    /// - `frequency_hz`: 需要压制的中心频率（Hz）。
    /// - `sample_rate`: 采样率（Hz）。
    /// - `q`: 品质因数，建议 30-40 之间以保持窄带处理。
    pub fn new(frequency_hz: f64, sample_rate: f64, q: f64) -> Self {
        assert!(frequency_hz > 0.0, "中心频率必须大于 0");
        assert!(sample_rate > 0.0, "采样率必须大于 0");
        assert!(q > 0.0, "Q 值必须大于 0");
        assert!(
            frequency_hz < sample_rate / 2.0,
            "中心频率必须小于奈奎斯特频率"
        );

        let omega = 2.0 * PI * frequency_hz / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();

        let b0 = 1.0;
        let b1 = -2.0 * cos_omega;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;

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
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// 麦克风降噪链路。
///
/// 先用高通去除 DC/低频轰鸣，再用窄带陷波器压制 50/60Hz 及低阶谐波，
/// 最后用低电平向下扩展器降低静音段残留底噪。
/// 该链路只用于麦克风输入，系统音频保持原样。
#[derive(Clone)]
pub struct MicrophoneDenoiseChain {
    highpass: HighpassFilter,
    notches: Vec<NotchFilter>,
    noise_suppressor: NoiseFloorSuppressor,
}

impl MicrophoneDenoiseChain {
    /// 创建麦克风降噪链路。
    ///
    /// # 参数
    /// - `sample_rate`: 采样率（Hz），项目混音输出通常为 48000。
    pub fn new(sample_rate: f64) -> Self {
        const NOTCH_Q: f64 = 35.0;
        const NOTCH_FREQUENCIES_HZ: [f64; 6] = [50.0, 60.0, 100.0, 120.0, 150.0, 180.0];

        Self {
            highpass: HighpassFilter::new(80.0, sample_rate),
            notches: NOTCH_FREQUENCIES_HZ
                .into_iter()
                .filter(|freq| *freq < sample_rate / 2.0)
                .map(|freq| NotchFilter::new(freq, sample_rate, NOTCH_Q))
                .collect(),
            noise_suppressor: NoiseFloorSuppressor::new(sample_rate),
        }
    }

    /// 处理单个麦克风样本。
    pub fn process(&mut self, input: f32) -> f32 {
        let mut output = self.highpass.process(input);
        for notch in &mut self.notches {
            output = notch.process(output);
        }
        self.noise_suppressor.process(output)
    }

    /// 重置滤波器状态。
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.highpass.reset();
        for notch in &mut self.notches {
            notch.reset();
        }
        self.noise_suppressor.reset();
    }
}

/// 低电平噪声向下扩展器。
///
/// 低于噪声门限时平滑降低增益，正常说话电平保持接近原样，避免残留电流底噪
/// 在静音段被持续听到。
#[derive(Clone)]
struct NoiseFloorSuppressor {
    open_threshold: f32,
    close_threshold: f32,
    min_gain: f32,
    gain: f32,
    envelope: f32,
    attack_coeff: f32,
    release_coeff: f32,
    envelope_coeff: f32,
}

impl NoiseFloorSuppressor {
    fn new(sample_rate: f64) -> Self {
        assert!(sample_rate > 0.0, "采样率必须大于 0");

        Self {
            open_threshold: 0.014,
            close_threshold: 0.006,
            min_gain: 0.28,
            gain: 1.0,
            envelope: 0.0,
            attack_coeff: smoothing_coeff(0.004, sample_rate),
            release_coeff: smoothing_coeff(0.080, sample_rate),
            envelope_coeff: smoothing_coeff(0.010, sample_rate),
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        self.envelope += (input.abs() - self.envelope) * self.envelope_coeff;

        let target_gain = if self.envelope >= self.open_threshold {
            1.0
        } else if self.envelope <= self.close_threshold {
            self.min_gain
        } else {
            let t = (self.envelope - self.close_threshold)
                / (self.open_threshold - self.close_threshold);
            self.min_gain + (1.0 - self.min_gain) * t
        };

        let coeff = if target_gain > self.gain {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.gain += (target_gain - self.gain) * coeff;

        input * self.gain
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.envelope = 0.0;
    }
}

fn smoothing_coeff(time_seconds: f64, sample_rate: f64) -> f32 {
    (1.0 - (-1.0 / (time_seconds * sample_rate)).exp()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f64 = 48_000.0;

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
        (sum_squares / samples.len() as f32).sqrt()
    }

    fn sine(freq_hz: f64, seconds: f64, amplitude: f32) -> Vec<f32> {
        let sample_count = (SAMPLE_RATE * seconds) as usize;
        (0..sample_count)
            .map(|i| {
                let t = i as f64 / SAMPLE_RATE;
                (2.0 * PI * freq_hz * t).sin() as f32 * amplitude
            })
            .collect()
    }

    fn composite_sine(freqs_hz: &[f64], seconds: f64, amplitude_per_tone: f32) -> Vec<f32> {
        let sample_count = (SAMPLE_RATE * seconds) as usize;
        (0..sample_count)
            .map(|i| {
                let t = i as f64 / SAMPLE_RATE;
                freqs_hz
                    .iter()
                    .map(|freq| (2.0 * PI * freq * t).sin() as f32 * amplitude_per_tone)
                    .sum()
            })
            .collect()
    }

    fn deterministic_wideband_noise(seconds: f64, amplitude: f32) -> Vec<f32> {
        let sample_count = (SAMPLE_RATE * seconds) as usize;
        let mut state = 0x5eed_1234_u32;

        (0..sample_count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let unit = ((state >> 8) as f32) / 16_777_215.0;
                (unit * 2.0 - 1.0) * amplitude
            })
            .collect()
    }

    fn steady_rms_after_warmup(samples: &[f32]) -> f32 {
        let warmup = (SAMPLE_RATE as usize) / 5;
        rms(&samples[warmup.min(samples.len())..])
    }

    fn tone_magnitude_after_warmup(samples: &[f32], freq_hz: f64) -> f32 {
        let warmup = (SAMPLE_RATE as usize) / 5;
        let samples = &samples[warmup.min(samples.len())..];
        if samples.is_empty() {
            return 0.0;
        }

        let (sin_sum, cos_sum) =
            samples
                .iter()
                .enumerate()
                .fold((0.0f64, 0.0f64), |(sin_sum, cos_sum), (i, sample)| {
                    let phase = 2.0 * PI * freq_hz * i as f64 / SAMPLE_RATE;
                    (
                        sin_sum + *sample as f64 * phase.sin(),
                        cos_sum + *sample as f64 * phase.cos(),
                    )
                });

        (2.0 * (sin_sum.hypot(cos_sum)) / samples.len() as f64) as f32
    }

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

        // 50Hz 应被显著衰减（二阶 Butterworth 在 80Hz 截止时，50Hz 处约 -8dB）
        assert!(
            max_output < 0.5,
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

        // 60Hz 应被显著衰减（二阶 Butterworth 在 80Hz 截止时，60Hz 处约 -6dB）
        assert!(
            max_output < 0.55,
            "60Hz should be attenuated, got max {}",
            max_output
        );
    }

    #[test]
    fn notch_filter_attenuates_center_frequency() {
        let input = sine(120.0, 1.0, 0.5);
        let input_rms = steady_rms_after_warmup(&input);

        let mut filter = NotchFilter::new(120.0, SAMPLE_RATE, 35.0);
        let output: Vec<f32> = input.iter().map(|sample| filter.process(*sample)).collect();
        let output_rms = steady_rms_after_warmup(&output);

        assert!(
            output_rms < input_rms * 0.2,
            "notch should attenuate center frequency: input={input_rms}, output={output_rms}"
        );
    }

    #[test]
    fn denoise_chain_reduces_powerline_harmonics_beyond_highpass() {
        const POWERLINE_HARMONICS_HZ: [f64; 6] = [50.0, 60.0, 100.0, 120.0, 150.0, 180.0];
        let input = composite_sine(&POWERLINE_HARMONICS_HZ, 1.0, 0.08);

        let mut filters = [HighpassFilter::new(80.0, SAMPLE_RATE)];
        let highpass_only: Vec<f32> = input
            .iter()
            .map(|sample| filters[0].process(*sample))
            .collect();
        let highpass_rms = steady_rms_after_warmup(&highpass_only);

        let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);
        let denoised: Vec<f32> = input.iter().map(|sample| chain.process(*sample)).collect();
        let denoised_rms = steady_rms_after_warmup(&denoised);

        assert!(
            denoised_rms < highpass_rms * 0.45,
            "denoise chain should reduce residual powerline harmonics: highpass={highpass_rms}, denoised={denoised_rms}"
        );

        for freq in POWERLINE_HARMONICS_HZ {
            let highpass_tone = tone_magnitude_after_warmup(&highpass_only, freq);
            let denoised_tone = tone_magnitude_after_warmup(&denoised, freq);
            assert!(
                denoised_tone < highpass_tone * 0.35,
                "{freq}Hz tone should be attenuated beyond highpass: highpass={highpass_tone}, denoised={denoised_tone}"
            );
        }
    }

    #[test]
    fn denoise_chain_preserves_voice_band_signal() {
        for freq in [250.0, 300.0, 500.0, 1000.0, 2000.0] {
            let input = sine(freq, 1.0, 0.5);
            let input_rms = steady_rms_after_warmup(&input);

            let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);
            let denoised: Vec<f32> = input.iter().map(|sample| chain.process(*sample)).collect();
            let denoised_rms = steady_rms_after_warmup(&denoised);

            assert!(
                denoised_rms > input_rms * 0.9,
                "{freq}Hz voice-band signal should be preserved: input={input_rms}, denoised={denoised_rms}"
            );
        }
    }

    #[test]
    fn denoise_chain_reduces_low_level_broadband_noise_floor() {
        let input = deterministic_wideband_noise(1.0, 0.012);
        let input_rms = steady_rms_after_warmup(&input);

        let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);
        let denoised: Vec<f32> = input.iter().map(|sample| chain.process(*sample)).collect();
        let denoised_rms = steady_rms_after_warmup(&denoised);

        assert!(
            denoised_rms < input_rms * 0.45,
            "low-level broadband noise floor should be reduced: input={input_rms}, denoised={denoised_rms}"
        );
    }

    #[test]
    fn denoise_chain_preserves_quiet_voice_near_noise_gate() {
        for freq in [300.0, 1000.0, 2000.0] {
            let input = sine(freq, 1.0, 0.024);
            let input_rms = steady_rms_after_warmup(&input);

            let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);
            let denoised: Vec<f32> = input.iter().map(|sample| chain.process(*sample)).collect();
            let denoised_rms = steady_rms_after_warmup(&denoised);

            assert!(
                denoised_rms > input_rms * 0.88,
                "{freq}Hz quiet voice should not be treated as noise: input={input_rms}, denoised={denoised_rms}"
            );
        }
    }

    #[test]
    fn denoise_chain_does_not_hold_back_voice_after_quiet_noise() {
        let mut input = deterministic_wideband_noise(0.25, 0.008);
        input.extend(sine(1000.0, 0.75, 0.18));

        let voice_start = (SAMPLE_RATE * 0.25) as usize;
        let onset_len = (SAMPLE_RATE * 0.020) as usize;
        let voice_input_onset_rms = rms(&input[voice_start..voice_start + onset_len]);

        let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);
        let denoised: Vec<f32> = input.iter().map(|sample| chain.process(*sample)).collect();
        let voice_denoised_onset_rms = rms(&denoised[voice_start..voice_start + onset_len]);

        assert!(
            voice_denoised_onset_rms > voice_input_onset_rms * 0.78,
            "voice onset should recover quickly after quiet noise: input={voice_input_onset_rms}, denoised={voice_denoised_onset_rms}"
        );
    }

    #[test]
    fn denoise_chain_preserves_quiet_voice_tail_after_normal_speech() {
        let mut input = sine(1000.0, 0.25, 0.18);
        input.extend(sine(1000.0, 0.35, 0.024));

        let tail_start = (SAMPLE_RATE * 0.25) as usize;
        let tail_warmup = (SAMPLE_RATE * 0.12) as usize;
        let tail_input_rms = rms(&input[tail_start + tail_warmup..]);

        let mut chain = MicrophoneDenoiseChain::new(SAMPLE_RATE);
        let denoised: Vec<f32> = input.iter().map(|sample| chain.process(*sample)).collect();
        let tail_denoised_rms = rms(&denoised[tail_start + tail_warmup..]);

        assert!(
            tail_denoised_rms > tail_input_rms * 0.88,
            "quiet voice tail should not be cut as noise: input={tail_input_rms}, denoised={tail_denoised_rms}"
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
