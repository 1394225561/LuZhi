/// 滑动窗口 RMS 电平检测器。
///
/// 从 PCM 采样流中计算归一化 RMS 电平值 (0.0 ~ 1.0)。
/// 设计为单线程使用，消费线程持有唯一实例。
///
/// 算法：在滑动窗口内累积采样平方和，窗口填满后计算 RMS 并重置。
/// 参考电平 REFERENCE_LEVEL 用于将 RMS 归一化到 0.0~1.0 区间。
pub struct MicLevelDetector {
    /// 当前窗口采样平方累加和
    squared_sum: f64,
    /// 当前窗口已收集的采样数
    sample_count: usize,
    /// 窗口容量（采样数）
    window_size: usize,
    /// 最近一次计算的归一化电平值
    current_level: f64,
}

impl MicLevelDetector {
    /// 参考电平：满幅 1.0 的典型语音 RMS 约 0.1~0.3。
    /// 0.3 作为归一化基准，使得正常语音电平映射到 0.33~1.0 范围。
    const REFERENCE_LEVEL: f64 = 0.3;

    /// 创建检测器。
    ///
    /// `window_size` 建议 1024~4096（在 48kHz 下对应 21~85ms 窗口）。
    pub fn new(window_size: usize) -> Self {
        assert!(window_size > 0, "window_size 必须大于 0");
        Self {
            squared_sum: 0.0,
            sample_count: 0,
            window_size,
            current_level: 0.0,
        }
    }

    /// 推入新的 PCM 采样块，返回当前归一化电平值 (0.0 ~ 1.0)。
    ///
    /// `samples` 为 f32 格式的 PCM 采样（单声道或已混合的音频数据）。
    /// 空切片时返回当前缓存值不变。
    pub fn push_samples(&mut self, samples: &[f32]) -> f64 {
        for &s in samples {
            let s = s as f64;
            self.squared_sum += s * s;
            self.sample_count += 1;

            if self.sample_count >= self.window_size {
                let rms = (self.squared_sum / self.sample_count as f64).sqrt();
                self.current_level = (rms / Self::REFERENCE_LEVEL).min(1.0);
                self.squared_sum = 0.0;
                self.sample_count = 0;
            }
        }
        self.current_level
    }

    /// 获取最近一次计算的电平值（不推入新数据）。
    pub fn current_level(&self) -> f64 {
        self.current_level
    }

    /// 重置检测器状态。
    pub fn reset(&mut self) {
        self.squared_sum = 0.0;
        self.sample_count = 0;
        self.current_level = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_samples_returns_zero_initially() {
        let mut detector = MicLevelDetector::new(1024);
        assert_eq!(detector.current_level(), 0.0);
        let level = detector.push_samples(&[]);
        assert_eq!(level, 0.0);
    }

    #[test]
    fn silence_returns_zero() {
        let mut detector = MicLevelDetector::new(100);
        let zeros = vec![0.0f32; 200];
        let level = detector.push_samples(&zeros);
        assert_eq!(level, 0.0);
    }

    #[test]
    fn full_amplitude_returns_near_one() {
        let mut detector = MicLevelDetector::new(100);
        let full = vec![1.0f32; 200];
        let level = detector.push_samples(&full);
        // RMS of 1.0 signal = 1.0, normalized = 1.0/0.3 ≈ 3.33 → clamp to 1.0
        assert!((level - 1.0).abs() < 0.01, "expected ~1.0, got {level}");
    }

    #[test]
    fn half_amplitude_clamps_to_one_with_reference_level() {
        let mut detector = MicLevelDetector::new(100);
        let half = vec![0.5f32; 200];
        let level = detector.push_samples(&half);
        // RMS 0.5 / REFERENCE_LEVEL 0.3 ≈ 1.667 → clamped to 1.0
        assert!(level > 0.99, "expected ~1.0 (clamped), got {level}");
    }

    #[test]
    fn low_amplitude_returns_small_value() {
        let mut detector = MicLevelDetector::new(100);
        let low = vec![0.05f32; 200];
        let level = detector.push_samples(&low);
        // RMS of 0.05 signal = 0.05, normalized = 0.05/0.3 ≈ 0.167
        assert!(level > 0.1 && level < 0.25, "expected ~0.167, got {level}");
    }

    #[test]
    fn reset_clears_level() {
        let mut detector = MicLevelDetector::new(100);
        let full = vec![1.0f32; 200];
        detector.push_samples(&full);
        assert!(detector.current_level() > 0.0);
        detector.reset();
        assert_eq!(detector.current_level(), 0.0);
    }

    #[test]
    fn single_sample_window() {
        let mut detector = MicLevelDetector::new(1);
        let level = detector.push_samples(&[0.6]);
        // RMS of single 0.6 sample = 0.6, normalized = 0.6/0.3 = 2.0 → clamp to 1.0
        assert!((level - 1.0).abs() < 0.01, "expected 1.0, got {level}");
    }

    #[test]
    fn large_window_size() {
        let mut detector = MicLevelDetector::new(4096);
        let samples = vec![0.2f32; 8192];
        let level = detector.push_samples(&samples);
        // RMS = 0.2, normalized = 0.2/0.3 ≈ 0.667
        assert!(level > 0.5 && level < 0.8, "expected ~0.667, got {level}");
    }

    #[test]
    fn increasing_signal_gives_increasing_level() {
        let mut detector = MicLevelDetector::new(50);
        let quiet = vec![0.03f32; 100];
        let loud = vec![0.3f32; 100];

        let quiet_level = detector.push_samples(&quiet);
        let loud_level = detector.push_samples(&loud);

        assert!(
            loud_level > quiet_level,
            "loud {loud_level} should exceed quiet {quiet_level}"
        );
    }
}
