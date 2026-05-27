use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Monotonic timestamp shared by video frames, audio chunks, and UI events.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct MediaTimestamp {
    pub nanos: u64,
}

impl MediaTimestamp {
    /// Creates a monotonic media timestamp from nanoseconds.
    pub fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }
}

/// Pixel layout for a captured video frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    Bgra8,
}

/// Reference-counted frame bytes owned by the Rust recording pipeline.
#[derive(Clone, Debug)]
pub enum FrameBuffer {
    Owned(Arc<[u8]>),
}

/// Video frame metadata and pixel buffer.
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub timestamp: MediaTimestamp,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub buffer: FrameBuffer,
}

/// Shared video frame reference passed between capture and processing stages.
pub type VideoFrameRef = Arc<VideoFrame>;

/// Audio samples captured from system audio or microphone.
///
/// Samples are stored as interleaved f32 in [-1.0, 1.0] range.
/// Timestamps must share the same monotonic clock as `VideoFrame`.
#[derive(Clone, Debug)]
pub struct AudioChunk {
    pub timestamp: MediaTimestamp,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Arc<[f32]>,
}

/// Mixed audio output combining system audio and microphone.
///
/// Produced by `AudioMixer` after resampling, alignment, and clipping protection.
#[derive(Clone, Debug)]
pub struct MixedAudioChunk {
    pub timestamp: MediaTimestamp,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Arc<[f32]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_orders_by_nanoseconds() {
        let early = MediaTimestamp::from_nanos(10);
        let late = MediaTimestamp::from_nanos(20);

        assert!(early < late);
    }

    #[test]
    fn audio_chunk_clone_shares_samples() {
        let chunk = AudioChunk {
            timestamp: MediaTimestamp::from_nanos(0),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.5f32, -0.5].into_boxed_slice()),
        };

        let cloned = chunk.clone();

        // Arc clone — both point to the same allocation
        assert!(Arc::ptr_eq(&chunk.samples, &cloned.samples));
        assert_eq!(chunk.timestamp, cloned.timestamp);
        assert_eq!(chunk.sample_rate, cloned.sample_rate);
    }

    #[test]
    fn mixed_audio_chunk_metadata() {
        let mixed = MixedAudioChunk {
            timestamp: MediaTimestamp::from_nanos(1_000_000),
            sample_rate: 48000,
            channels: 2,
            samples: Arc::from(vec![0.0f32; 960].into_boxed_slice()),
        };

        assert_eq!(mixed.sample_rate, 48000);
        assert_eq!(mixed.channels, 2);
        // 960 samples / 2 channels = 480 frames = 10ms at 48kHz
        assert_eq!(mixed.samples.len(), 960);
    }

    #[test]
    fn timestamp_equality() {
        let a = MediaTimestamp::from_nanos(12345);
        let b = MediaTimestamp::from_nanos(12345);
        let c = MediaTimestamp::from_nanos(99999);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
