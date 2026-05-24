use std::sync::Arc;

/// Monotonic timestamp shared by video frames, audio chunks, and UI events.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_orders_by_nanoseconds() {
        let early = MediaTimestamp::from_nanos(10);
        let late = MediaTimestamp::from_nanos(20);

        assert!(early < late);
    }
}
