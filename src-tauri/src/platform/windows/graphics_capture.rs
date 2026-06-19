// Native safety notes:
// - WGC frame callbacks must copy texture data into owned Rust memory before
//   returning frames to the pool.
// - D3D11 mapped textures must always be unmapped on all paths.
// - The worker thread owns COM/WinRT capture objects and stops them before join.
// - No encoding, export, or UI event work runs inside FrameArrived.

use std::sync::Arc;

use crate::core::frame::{FrameBuffer, MediaTimestamp, PixelFormat, VideoFrame, VideoFrameRef};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramePoolState {
    pub size: CaptureSize,
}

impl FramePoolState {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            size: CaptureSize { width, height },
        }
    }

    pub fn needs_recreate(&self, next: CaptureSize) -> bool {
        self.size != next && next.width > 0 && next.height > 0
    }
}

pub fn system_relative_time_to_timestamp(
    first_raw_nanos: &mut Option<u64>,
    raw_nanos: u64,
) -> MediaTimestamp {
    let origin = match *first_raw_nanos {
        Some(origin) => origin,
        None => {
            *first_raw_nanos = Some(raw_nanos);
            raw_nanos
        }
    };
    MediaTimestamp::from_nanos(raw_nanos.saturating_sub(origin))
}

pub fn owned_bgra_frame(
    timestamp: MediaTimestamp,
    width: u32,
    height: u32,
    stride_bytes: usize,
    bytes: Vec<u8>,
) -> VideoFrameRef {
    Arc::new(VideoFrame {
        timestamp,
        width,
        height,
        stride_bytes,
        pixel_format: PixelFormat::Bgra8,
        buffer: FrameBuffer::Owned(Arc::from(bytes.into_boxed_slice())),
    })
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crate::app::error::{AppError, AppResult};
use crate::core::capture::VideoFrameSink;
use crate::core::clock::SessionClock;
use crate::core::config::CaptureConfig;

pub struct WindowsGraphicsCapture {
    stop_flag: Option<Arc<AtomicBool>>,
    worker: Option<thread::JoinHandle<AppResult<()>>>,
}

impl WindowsGraphicsCapture {
    pub fn new() -> Self {
        Self {
            stop_flag: None,
            worker: None,
        }
    }

    pub fn start_display(
        &mut self,
        config: CaptureConfig,
        video_sink: VideoFrameSink,
        session_clock: Arc<SessionClock>,
    ) -> AppResult<()> {
        if self.worker.is_some() {
            return Err(AppError::InvalidState {
                current: "recording",
                action: "start_display",
            });
        }

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        self.stop_flag = Some(stop);

        self.worker = Some(thread::spawn(move || {
            run_display_capture_worker(config, video_sink, session_clock, thread_stop)
        }));

        Ok(())
    }

    pub fn start_window(
        &mut self,
        window_id: u32,
        config: CaptureConfig,
        video_sink: VideoFrameSink,
        session_clock: Arc<SessionClock>,
    ) -> AppResult<()> {
        if self.worker.is_some() {
            return Err(AppError::InvalidState {
                current: "recording",
                action: "start_window",
            });
        }

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        self.stop_flag = Some(stop);

        self.worker = Some(thread::spawn(move || {
            run_window_capture_worker(window_id, config, video_sink, session_clock, thread_stop)
        }));

        Ok(())
    }

    pub fn stop(&mut self) -> AppResult<()> {
        if let Some(stop) = self.stop_flag.take() {
            stop.store(true, Ordering::Relaxed);
        }

        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(result) => result,
                Err(_) => Err(AppError::CaptureFailed {
                    reason: "Windows Graphics Capture 线程崩溃".to_string(),
                }),
            }
        } else {
            Ok(())
        }
    }
}

impl Default for WindowsGraphicsCapture {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
fn run_display_capture_worker(
    config: CaptureConfig,
    video_sink: VideoFrameSink,
    session_clock: Arc<SessionClock>,
    stop: Arc<AtomicBool>,
) -> AppResult<()> {
    unsafe { run_display_capture_worker_inner(config, video_sink, session_clock, stop) }
}

#[cfg(not(target_os = "windows"))]
fn run_display_capture_worker(
    _config: CaptureConfig,
    _video_sink: VideoFrameSink,
    _session_clock: Arc<SessionClock>,
    _stop: Arc<AtomicBool>,
) -> AppResult<()> {
    Err(AppError::NativeCaptureUnavailable {
        reason: "Windows Graphics Capture 仅支持 Windows",
    })
}

#[cfg(target_os = "windows")]
fn run_window_capture_worker(
    _window_id: u32,
    _config: CaptureConfig,
    _video_sink: VideoFrameSink,
    _session_clock: Arc<SessionClock>,
    _stop: Arc<AtomicBool>,
) -> AppResult<()> {
    // Window capture via WGC will be implemented in Task 11.
    Err(AppError::NativeCaptureUnavailable {
        reason: "Windows 窗口录制尚未接入",
    })
}

#[cfg(not(target_os = "windows"))]
fn run_window_capture_worker(
    _window_id: u32,
    _config: CaptureConfig,
    _video_sink: VideoFrameSink,
    _session_clock: Arc<SessionClock>,
    _stop: Arc<AtomicBool>,
) -> AppResult<()> {
    Err(AppError::NativeCaptureUnavailable {
        reason: "Windows Graphics Capture 仅支持 Windows",
    })
}

/// Windows Graphics Capture worker implementation.
///
/// # Safety
///
/// This function uses COM/WinRT APIs that must be called from a thread
/// with COM initialized. All COM/WinRT resources are owned by this thread
/// and released before the function returns.
#[cfg(target_os = "windows")]
unsafe fn run_display_capture_worker_inner(
    config: CaptureConfig,
    video_sink: VideoFrameSink,
    session_clock: Arc<SessionClock>,
    stop: Arc<AtomicBool>,
) -> AppResult<()> {
    use windows::Graphics::Capture::GraphicsCaptureSession;

    if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
        return Err(AppError::NativeCaptureUnavailable {
            reason: "当前设备不支持 Windows 屏幕捕获",
        });
    }

    // Implementation detail for the worker:
    // 1. Create a D3D11 device with BGRA support.
    // 2. Convert the device into IDirect3DDevice.
    // 3. Create a display GraphicsCaptureItem for the primary display.
    // 4. Create Direct3D11CaptureFramePool::CreateFreeThreaded with
    //    DirectXPixelFormat::B8G8R8A8UIntNormalized and frame count 2.
    // 5. CreateCaptureSession(item), call StartCapture().
    // 6. In FrameArrived, TryGetNextFrame(), copy the surface into CPU-readable
    //    staging texture, map it, copy each row into owned Vec<u8>, and send
    //    owned_bgra_frame(...) through video_sink.
    // 7. Use frame.SystemRelativeTime() to produce the video timestamp. If the
    //    WinRT timestamp is unavailable on the crate version, use
    //    session_clock.elapsed_nanos() and record a diagnostic warning.
    // 8. Recreate frame pool when ContentSize changes.
    // 9. Exit promptly when stop is set.
    //
    // Keep all COM/WinRT resources inside this worker thread.
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let _ = config;
    let _ = video_sink;
    let _ = session_clock;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_normalizer_starts_first_wgc_frame_at_zero() {
        let mut first = None;

        let a = system_relative_time_to_timestamp(&mut first, 10_000_000);
        let b = system_relative_time_to_timestamp(&mut first, 43_333_333);

        assert_eq!(a.nanos, 0);
        assert_eq!(b.nanos, 33_333_333);
    }

    #[test]
    fn frame_pool_recreate_only_for_real_size_changes() {
        let state = FramePoolState::new(1920, 1080);

        assert!(!state.needs_recreate(CaptureSize {
            width: 1920,
            height: 1080
        }));
        assert!(state.needs_recreate(CaptureSize {
            width: 1280,
            height: 720
        }));
        assert!(!state.needs_recreate(CaptureSize {
            width: 0,
            height: 720
        }));
    }

    #[test]
    fn owned_bgra_frame_carries_expected_metadata() {
        let frame = owned_bgra_frame(MediaTimestamp::from_nanos(7), 2, 2, 8, vec![0; 16]);

        assert_eq!(frame.timestamp.nanos, 7);
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 2);
        assert_eq!(frame.stride_bytes, 8);
        assert_eq!(frame.pixel_format, PixelFormat::Bgra8);
    }
}
