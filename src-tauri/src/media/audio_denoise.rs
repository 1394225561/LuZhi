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
