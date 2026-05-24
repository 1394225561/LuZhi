use crate::app::error::{AppError, AppResult};
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

/// Real-time audio mixer that combines system audio and microphone input.
///
/// Performs:
/// 1. Resampling to a unified 48 kHz sample rate (linear interpolation)
/// 2. Channel layout normalization to stereo
/// 3. Timestamp-based alignment with silence padding for missing segments
/// 4. Weighted sum mixing with hard clipping protection
pub struct SimpleAudioMixer;

impl SimpleAudioMixer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SimpleAudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioMixer for SimpleAudioMixer {
    fn mix(
        &self,
        system: Option<&AudioChunk>,
        mic: Option<&AudioChunk>,
    ) -> AppResult<MixedAudioChunk> {
        match (system, mic) {
            (Some(sys), Some(mic)) => mix_two(sys, mic),
            (Some(sys), None) => passthrough(sys),
            (None, Some(mic)) => passthrough(mic),
            (None, None) => Err(AppError::AudioMixFailed {
                reason: "系统音频和麦克风均无数据".to_string(),
            }),
        }
    }
}

/// Single-source passthrough: resample + convert to stereo if needed.
fn passthrough(chunk: &AudioChunk) -> AppResult<MixedAudioChunk> {
    let resampled = resample(chunk, MIXED_SAMPLE_RATE);
    let stereo = to_stereo(&resampled, chunk.channels);
    let clamped = clamp_samples(&stereo);

    Ok(MixedAudioChunk {
        timestamp: chunk.timestamp,
        sample_rate: MIXED_SAMPLE_RATE,
        channels: MIXED_CHANNELS,
        samples: clamped.into(),
    })
}

/// Mix two audio sources with timestamp alignment.
///
/// Algorithm:
/// 1. Resample both to 48 kHz stereo
/// 2. Align by timestamp — the earlier chunk is padded with silence at the front
/// 3. Overlap region: equal-weight sum (0.5 * system + 0.5 * mic)
/// 4. Non-overlapping tail: passthrough from the longer source
/// 5. Hard clamp to [-1.0, 1.0] to prevent clipping
fn mix_two(system: &AudioChunk, mic: &AudioChunk) -> AppResult<MixedAudioChunk> {
    let sys_resampled = resample(system, MIXED_SAMPLE_RATE);
    let mic_resampled = resample(mic, MIXED_SAMPLE_RATE);

    let sys_stereo = to_stereo(&sys_resampled, system.channels);
    let mic_stereo = to_stereo(&mic_resampled, mic.channels);

    // Determine the overlap window based on timestamps.
    // The output starts at the earlier timestamp and ends at the later one.
    let ts_sys = system.timestamp.nanos;
    let ts_mic = mic.timestamp.nanos;

    let sys_offset_samples = if ts_sys > ts_mic {
        // System audio starts later — offset into mic's timeline
        ((ts_sys - ts_mic) as f64 / 1_000_000_000.0
            * MIXED_SAMPLE_RATE as f64
            * MIXED_CHANNELS as f64) as usize
    } else {
        0
    };

    let mic_offset_samples = if ts_mic > ts_sys {
        ((ts_mic - ts_sys) as f64 / 1_000_000_000.0
            * MIXED_SAMPLE_RATE as f64
            * MIXED_CHANNELS as f64) as usize
    } else {
        0
    };

    // Output length = max(sys_offset + sys_len, mic_offset + mic_len)
    let sys_total = sys_offset_samples + sys_stereo.len();
    let mic_total = mic_offset_samples + mic_stereo.len();
    let output_len = sys_total.max(mic_total);

    let mut output = vec![0.0f32; output_len];

    // Add system audio into the output buffer
    for (i, &sample) in sys_stereo.iter().enumerate() {
        let idx = sys_offset_samples + i;
        if idx < output_len {
            output[idx] += sample * 0.5;
        }
    }

    // Add microphone into the output buffer
    for (i, &sample) in mic_stereo.iter().enumerate() {
        let idx = mic_offset_samples + i;
        if idx < output_len {
            output[idx] += sample * 0.5;
        }
    }

    // Hard clamp to prevent clipping
    let clamped = clamp_samples(&output);

    let earliest_ts = MediaTimestamp::from_nanos(ts_sys.min(ts_mic));

    Ok(MixedAudioChunk {
        timestamp: earliest_ts,
        sample_rate: MIXED_SAMPLE_RATE,
        channels: MIXED_CHANNELS,
        samples: clamped.into(),
    })
}

/// Resamples audio from its native sample rate to `target_rate` using linear interpolation.
///
/// Linear interpolation is sufficient for real-time mixing preview quality.
/// For final export, a higher-quality resampler (e.g., FFmpeg swresample) should be used.
fn resample(chunk: &AudioChunk, target_rate: u32) -> Vec<f32> {
    if chunk.sample_rate == target_rate {
        return chunk.samples.to_vec();
    }

    let src_rate = chunk.sample_rate as f64;
    let dst_rate = target_rate as f64;
    let ratio = src_rate / dst_rate;

    // For mono input, each "frame" is 1 sample; for stereo, 2 samples.
    // We resample per-channel independently.
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
            // else: leave as 0.0 (silence padding)
        }
    }

    output
}

/// Converts mono audio to stereo by duplicating each sample.
/// If already stereo, passthrough unchanged.
fn to_stereo(samples: &[f32], src_channels: u16) -> Vec<f32> {
    if src_channels >= 2 {
        // Already stereo or more — just take first 2 channels
        // For interleaved stereo: [L, R, L, R, ...] — passthrough
        return samples.to_vec();
    }

    // Mono: duplicate each sample to L+R
    let mut output = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        output.push(s);
        output.push(s);
    }
    output
}

/// Hard-clamps all samples to [-1.0, 1.0] to prevent clipping.
fn clamp_samples(samples: &[f32]) -> Vec<f32> {
    samples.iter().map(|s| s.clamp(-1.0, 1.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(ts_nanos: u64, sample_rate: u32, channels: u16, samples: Vec<f32>) -> AudioChunk {
        AudioChunk {
            timestamp: MediaTimestamp::from_nanos(ts_nanos),
            sample_rate,
            channels,
            samples: samples.into(),
        }
    }

    #[test]
    fn passthrough_single_source() {
        let mixer = SimpleAudioMixer::new();
        let chunk = make_chunk(0, 48000, 2, vec![0.5, -0.5, 0.3, -0.3]);

        let result = mixer.mix(Some(&chunk), None).unwrap();

        assert_eq!(result.sample_rate, 48000);
        assert_eq!(result.channels, 2);
        assert_eq!(result.samples.len(), 4);
        assert!((result.samples[0] - 0.5).abs() < 1e-6);
        assert!((result.samples[1] - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn mix_two_aligned_sources() {
        let mixer = SimpleAudioMixer::new();
        let sys = make_chunk(0, 48000, 2, vec![0.6, 0.6, 0.6, 0.6]);
        let mic = make_chunk(0, 48000, 2, vec![0.4, 0.4, 0.4, 0.4]);

        let result = mixer.mix(Some(&sys), Some(&mic)).unwrap();

        // Equal-weight mix: 0.5 * 0.6 + 0.5 * 0.4 = 0.5
        for sample in result.samples.iter() {
            assert!((sample - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn clipping_protection() {
        let mixer = SimpleAudioMixer::new();
        // Both sources at full amplitude
        let sys = make_chunk(0, 48000, 2, vec![1.0, 1.0]);
        let mic = make_chunk(0, 48000, 2, vec![1.0, 1.0]);

        let result = mixer.mix(Some(&sys), Some(&mic)).unwrap();

        // 0.5 * 1.0 + 0.5 * 1.0 = 1.0 — at the boundary, should not exceed
        for sample in result.samples.iter() {
            assert!(*sample <= 1.0);
            assert!(*sample >= -1.0);
        }
    }

    #[test]
    fn resample_44100_to_48000() {
        let chunk = make_chunk(0, 44100, 2, vec![0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
        let resampled = resample(&chunk, 48000);

        // Output should have more frames proportionally
        let src_frames = 3; // 6 samples / 2 channels
        let expected_frames = (src_frames as f64 * 48000.0 / 44100.0) as usize;
        assert_eq!(resampled.len(), expected_frames * 2);
    }

    #[test]
    fn resample_same_rate_passthrough() {
        let samples = vec![0.1, 0.2, 0.3, 0.4];
        let chunk = make_chunk(0, 48000, 2, samples.clone());
        let resampled = resample(&chunk, 48000);

        assert_eq!(resampled, samples);
    }

    #[test]
    fn mix_both_none_returns_error() {
        let mixer = SimpleAudioMixer::new();
        let result = mixer.mix(None, None);

        assert!(result.is_err());
    }

    #[test]
    fn mixed_output_stereo_48k() {
        let mixer = SimpleAudioMixer::new();
        let sys = make_chunk(0, 44100, 1, vec![0.5, 0.5, 0.5]);

        let result = mixer.mix(Some(&sys), None).unwrap();

        assert_eq!(result.sample_rate, 48000);
        assert_eq!(result.channels, 2);
    }

    #[test]
    fn timestamp_alignment_with_offset() {
        let mixer = SimpleAudioMixer::new();
        // System audio starts at 0ms, mic starts at 100ms
        let sys = make_chunk(0, 48000, 2, vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        let mic = make_chunk(100_000_000, 48000, 2, vec![0.5, 0.5, 0.5, 0.5]);

        let result = mixer.mix(Some(&sys), Some(&mic)).unwrap();

        // Mic starts 100ms late = 48000 * 0.1 * 2 = 9600 samples offset
        // First 9600 samples should be system only (0.5 * 1.0 = 0.5)
        assert_eq!(result.timestamp.nanos, 0);
        // The output should be longer than either input alone
        assert!(result.samples.len() > 6);
    }
}
