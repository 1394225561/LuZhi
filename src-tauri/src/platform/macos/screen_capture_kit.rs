// ⚠️ 人工审查检查点：
// - buffer 所有权：CMSampleBuffer → Arc<[u8]> 的内存拷贝路径是否安全
// - 回调线程：SCStream 回调在哪个线程？channel 发送是否线程安全？
// - stop 路径：所有 Retained 是否在 drop 时正确释放？是否存在 use-after-free？
// - 错误处理：回调中的错误如何传播？是否会导致 panic？
// - 帧率控制：回调频率是否受控？是否存在帧率失控导致内存暴涨？
// - 音频 buffer 生命周期：CMSampleBuffer 音频数据的引用是否安全
// - 采样格式转换：Int16 → f32 转换是否有精度损失或溢出

use std::sync::{Arc, Mutex};

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{AnyThread, DefinedClass};
use objc2_core_media::{CMSampleBuffer, CMTimeFlags};
use objc2_foundation::NSError;
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamDelegate, SCStreamOutput, SCStreamOutputType,
};

use crate::app::error::{AppError, AppResult};
use crate::core::capture::{
    AudioCapabilities, AudioCapture, AudioChunkSink, AudioConfig, AudioDevice, CaptureCapabilities,
    ScreenCapture, VideoFrameSink, WindowCapture,
};
use crate::core::config::CaptureConfig;
use crate::core::frame::{AudioChunk, FrameBuffer, PixelFormat, VideoFrame};
use crate::core::timeline::CaptureGeometry;
use crate::core::window::{WindowInfo, WindowRecordingState};

use super::window_list;

// ---------------------------------------------------------------------------
// Send wrapper for SCStream
// ---------------------------------------------------------------------------
// SCStream (NSObject subclass) is !Send in objc2 because it contains UnsafeCell.
// Retained<SCStream> uses atomic reference counting and is safe to move between
// threads — the Objective-C runtime ensures the underlying object outlives all
// strong references. We assert Send for our use case: the stream is created,
// stored, and dropped in MacScreenCapture; callbacks arrive on dispatch queues.

struct SendSCStream(Retained<SCStream>);

// SAFETY: Retained<SCStream> uses atomic reference counting. The stream is only
// started/stopped through the Objective-C runtime's thread-safe API.
unsafe impl Send for SendSCStream {}

// ---------------------------------------------------------------------------
// Stream output delegate
// ---------------------------------------------------------------------------
// The delegate receives CMSampleBuffer callbacks from SCStream on an internal
// dispatch queue. It forwards video frames and audio chunks to Rust channel
// sinks shared via Arc<Mutex<Option<Sender>>>.

struct StreamOutputIvars {
    video_sink: Mutex<Option<VideoFrameSink>>,
    audio_sink: Mutex<Option<AudioChunkSink>>,
    session_clock: Arc<crate::core::clock::SessionClock>,
    /// (first_cmsamplebuffer_pts_nanos, session_clock_elapsed_at_first_pts)
    pts_origin: Mutex<Option<(u64, u64)>>,
    /// Actual CVPixelBuffer size on first frame (width, height).
    first_frame_actual_size: Mutex<Option<(u32, u32)>>,
}

// Only protocol conformance goes inside define_class! — all helper functions
// are regular Rust functions defined outside the macro.
objc2::define_class!(
    #[unsafe(super(NSObject))]
    #[name = "LuZhiStreamOutput"]
    #[ivars = StreamOutputIvars]
    struct StreamOutput;

    // NSObjectProtocol conformance.
    unsafe impl NSObjectProtocol for StreamOutput {}

    // SCStreamOutput: receive CMSampleBuffer callbacks from SCStream.
    unsafe impl SCStreamOutput for StreamOutput {
        #[allow(non_snake_case)]
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn stream_didOutputSampleBuffer_ofType(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            r#type: SCStreamOutputType,
        ) {
            match r#type {
                SCStreamOutputType::Screen => {
                    handle_video_frame(self, sample_buffer);
                }
                SCStreamOutputType::Audio => {
                    handle_audio_chunk(self, sample_buffer);
                }
                _ => {}
            }
        }
    }

    // SCStreamDelegate: handle stream lifecycle events (all optional).
    unsafe impl SCStreamDelegate for StreamOutput {}
);

impl StreamOutput {
    fn new(
        video_sink: VideoFrameSink,
        audio_sink: AudioChunkSink,
        session_clock: Arc<crate::core::clock::SessionClock>,
    ) -> Retained<Self> {
        let this = Self::alloc().set_ivars(StreamOutputIvars {
            video_sink: Mutex::new(Some(video_sink)),
            audio_sink: Mutex::new(Some(audio_sink)),
            session_clock,
            pts_origin: Mutex::new(None),
            first_frame_actual_size: Mutex::new(None),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    fn clear_sinks(&self) {
        let mut video = self.ivars().video_sink.lock().unwrap();
        *video = None;
        let mut audio = self.ivars().audio_sink.lock().unwrap();
        *audio = None;
    }
}

// ---------------------------------------------------------------------------
// Frame/audio extraction helpers (outside define_class! to avoid method macro)
// ---------------------------------------------------------------------------

/// Core PTS normalization, factored for testability. The first valid PTS
/// establishes the origin; subsequent PTS values are shifted so they share the
/// same time domain as cursor samples.
///
/// Uses i128 arithmetic for the PTS delta so that values beyond i64::MAX
/// (theoretical for ScreenCaptureKit host-time PTS but possible) do not wrap.
fn compute_normalized_pts(
    origin: &mut Option<(u64, u64)>,
    pts_nanos: u64,
    session_entry_nanos: u64,
) -> u64 {
    match *origin {
        Some((first_pts, session_at_first)) => {
            let delta = pts_nanos as i128 - first_pts as i128;
            let result = delta + session_at_first as i128;
            if result < 0 {
                0
            } else if result > u64::MAX as i128 {
                u64::MAX
            } else {
                result as u64
            }
        }
        None => {
            *origin = Some((pts_nanos, session_entry_nanos));
            session_entry_nanos
        }
    }
}

/// Maps a CMSampleBuffer PTS (host-time nanoseconds) to a session-relative
/// timestamp anchored to the shared SessionClock. The first PTS seen by any
/// stream (video or system audio) establishes the mapping; subsequent PTS
/// values are shifted so they share the same time domain as cursor samples
/// (which use SessionClock directly via the cursor metadata runtime).
///
/// `session_entry_nanos` must be captured at callback entry, before any pixel
/// copy or audio conversion, so the origin does not include callback-internal
/// processing delay.
fn normalize_pts(delegate: &StreamOutput, pts_nanos: u64, session_entry_nanos: u64) -> u64 {
    let mut origin = delegate.ivars().pts_origin.lock().unwrap();
    compute_normalized_pts(&mut origin, pts_nanos, session_entry_nanos)
}

/// Extracts BGRA pixel data from CMSampleBuffer and sends as VideoFrameRef.
///
/// # Safety
///
/// Reads pixel data from the CMSampleBuffer's CVPixelBuffer.
/// Data is copied into Arc<[u8]> before the sample buffer is released.
unsafe fn handle_video_frame(delegate: &StreamOutput, sample_buffer: &CMSampleBuffer) {
    // Read CMSampleBuffer PTS at callback entry. If PTS is invalid, discard the
    // frame rather than using a fallback that would poison the pts_origin.
    let Some(pts_nanos) = extract_timestamp_nanos(sample_buffer) else {
        return;
    };
    // Capture session time at callback entry so origin establishment does not
    // include pixel-copy delay.
    let session_entry_nanos = delegate.ivars().session_clock.elapsed_nanos();

    let Some(image_buffer) = cmsamplebuffer_get_image_buffer(sample_buffer) else {
        return;
    };

    let status = cvpixelbuffer_lock_base_address(image_buffer, 0);
    if status != 0 {
        return;
    }

    let width = cvpixelbuffer_get_width(image_buffer);
    let height = cvpixelbuffer_get_height(image_buffer);
    let base_address = cvpixelbuffer_get_base_address(image_buffer);
    let bytes_per_row = cvpixelbuffer_get_bytes_per_row(image_buffer);

    // First-frame diagnostics: store actual CVPixelBuffer size and log for verification.
    {
        let mut size_guard = delegate.ivars().first_frame_actual_size.lock().unwrap();
        if size_guard.is_none() {
            *size_guard = Some((width as u32, height as u32));
            eprintln!(
                "[sck-first-frame] actual_buffer={}×{} bytes_per_row={}",
                width, height, bytes_per_row
            );
        }
    }

    if base_address.is_null() {
        cvpixelbuffer_unlock_base_address(image_buffer, 0);
        return;
    }

    let total_bytes = bytes_per_row * height;

    // ⚠️ 人工审查：此拷贝是必要的——CVPixelBuffer 的 base address 在
    // unlock 后不可访问。Arc<[u8]> 保证帧数据在 channel 传递中不被释放。
    let pixel_data = std::slice::from_raw_parts(base_address as *const u8, total_bytes).to_vec();
    let buffer = Arc::from(pixel_data.into_boxed_slice());

    cvpixelbuffer_unlock_base_address(image_buffer, 0);

    // Map PTS to session-relative time only after all validations pass so an
    // invalid sample never establishes the global pts_origin.
    let timestamp_nanos = normalize_pts(delegate, pts_nanos, session_entry_nanos);
    let timestamp = crate::core::frame::MediaTimestamp::from_nanos(timestamp_nanos);

    let frame = VideoFrame {
        timestamp,
        width: width as u32,
        height: height as u32,
        stride_bytes: bytes_per_row,
        pixel_format: PixelFormat::Bgra8,
        buffer: FrameBuffer::Owned(buffer),
    };

    let guard = delegate.ivars().video_sink.lock().unwrap();
    if let Some(sink) = guard.as_ref() {
        let _sent = sink.try_send_drop_newest(Arc::new(frame));
    }
}

/// Extracts PCM audio samples from CMSampleBuffer and sends as AudioChunk.
///
/// # Safety
///
/// Reads audio data from the CMSampleBuffer's audio buffer list.
unsafe fn handle_audio_chunk(delegate: &StreamOutput, sample_buffer: &CMSampleBuffer) {
    // Read CMSampleBuffer PTS at callback entry. If PTS is invalid, discard the
    // chunk rather than using a fallback that would poison the pts_origin.
    let Some(pts_nanos) = extract_timestamp_nanos(sample_buffer) else {
        return;
    };
    // Capture session time at callback entry so origin establishment does not
    // include audio-conversion delay.
    let session_entry_nanos = delegate.ivars().session_clock.elapsed_nanos();

    let Some(format_desc) = cmsamplebuffer_get_format_description(sample_buffer) else {
        return;
    };

    let stream_basic = cmformat_description_get_stream_basic_description(format_desc);
    if stream_basic.is_null() {
        return;
    }

    let basic = &*stream_basic;
    let sample_rate = basic.mSampleRate as u32;
    let channels = basic.mChannelsPerFrame as u16;

    if sample_rate == 0 || channels == 0 {
        return;
    }

    let pcm_format = match classify_pcm_format(basic) {
        Ok(fmt) => fmt,
        Err(_) => return,
    };

    if validate_asbd(basic).is_err() {
        return;
    }

    // Two-call size query for AudioBufferList.
    let mut needed_size = 0usize;
    let mut block_buffer: *mut std::ffi::c_void = std::ptr::null_mut();

    let size_status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        sample_buffer as *const _,
        &mut needed_size as *mut _,
        std::ptr::null_mut(),
        0,
        std::ptr::null(),
        std::ptr::null(),
        0,
        // Pass null for block_buffer_out on the size-query call —
        // we only need needed_size here and there is nothing to retain.
        std::ptr::null_mut(),
    );

    if size_status != 0 || needed_size == 0 {
        if !block_buffer.is_null() {
            cf_release(block_buffer as *const _);
        }
        return;
    }

    let mut storage = vec![0u8; needed_size];
    let buffer_list = storage.as_mut_ptr() as *mut AudioBufferList;

    let status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        sample_buffer as *const _,
        std::ptr::null_mut(),
        buffer_list,
        needed_size as isize,
        std::ptr::null(),
        std::ptr::null(),
        0,
        &mut block_buffer as *mut _,
    );

    if status != 0 {
        if !block_buffer.is_null() {
            cf_release(block_buffer as *const _);
        }
        return;
    }

    // Validate bounds before pointer arithmetic.
    let list_ref = &*buffer_list;
    let num_buffers = list_ref.mNumberBuffers as usize;
    if num_buffers == 0 {
        if !block_buffer.is_null() {
            cf_release(block_buffer as *const _);
        }
        return;
    }
    // Compute minimum size accounting for C struct layout:
    // AudioBufferList has mNumberBuffers (u32) + mBuffers ([AudioBuffer; 1]).
    // Additional buffers beyond the first are at offset size_of::<AudioBufferList>()
    // plus (n-1)*size_of::<AudioBuffer>(). This correctly accounts for padding
    // between mNumberBuffers and the mBuffers array on 64-bit targets.
    let minimum_size = std::mem::size_of::<AudioBufferList>()
        + (num_buffers - 1) * std::mem::size_of::<AudioBuffer>();
    if needed_size < minimum_size {
        if !block_buffer.is_null() {
            cf_release(block_buffer as *const _);
        }
        return;
    }

    let first_buffer = list_ref.mBuffers.as_ptr();
    let mut buffers: Vec<AudioBuffer> = Vec::with_capacity(num_buffers);
    for i in 0..num_buffers {
        let buffer = *first_buffer.add(i);
        if !buffer.mData.is_null() && buffer.mDataByteSize > 0 {
            buffers.push(buffer);
        }
    }

    let samples_f32 = if is_non_interleaved(basic) && buffers.len() > 1 {
        deinterleave_buffers(&buffers, channels as usize, pcm_format)
    } else {
        let mut samples = Vec::new();
        for buffer in &buffers {
            let bytes = unsafe {
                std::slice::from_raw_parts(buffer.mData as *const u8, buffer.mDataByteSize as usize)
            };
            samples.extend(convert_pcm_bytes_to_f32(pcm_format, bytes));
        }
        samples
    };

    if !block_buffer.is_null() {
        cf_release(block_buffer as *const _);
    }

    if samples_f32.is_empty() {
        return;
    }

    // Map PTS to session-relative time only after all validations pass so an
    // invalid sample never establishes the global pts_origin.
    let timestamp_nanos = normalize_pts(delegate, pts_nanos, session_entry_nanos);
    let timestamp = crate::core::frame::MediaTimestamp::from_nanos(timestamp_nanos);

    let chunk = AudioChunk {
        timestamp,
        sample_rate,
        channels,
        samples: Arc::from(samples_f32.into_boxed_slice()),
    };

    let guard = delegate.ivars().audio_sink.lock().unwrap();
    if let Some(sink) = guard.as_ref() {
        let _sent = sink.try_send_drop_newest(chunk);
    }
}

// ---------------------------------------------------------------------------
// FFI helpers for CMSampleBuffer / CVPixelBuffer / CoreMedia
// ---------------------------------------------------------------------------
#[allow(non_snake_case, non_camel_case_types, dead_code)]
#[repr(C)]
struct __CVPixelBuffer {
    _opaque: [u8; 0],
}
type CVPixelBufferRef = *const __CVPixelBuffer;

#[repr(C)]
struct AudioBufferList {
    mNumberBuffers: u32,
    mBuffers: [AudioBuffer; 1],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AudioBuffer {
    mNumberChannels: u32,
    mDataByteSize: u32,
    mData: *mut std::ffi::c_void,
}

#[repr(C)]
struct __CMFormatDescription {
    _opaque: [u8; 0],
}
type CMFormatDescriptionRef = *const __CMFormatDescription;

#[repr(C)]
struct AudioStreamBasicDescription {
    mSampleRate: f64,
    mFormatID: u32,
    mFormatFlags: u32,
    mBytesPerPacket: u32,
    mFramesPerPacket: u32,
    mBytesPerFrame: u32,
    mChannelsPerFrame: u32,
    mBitsPerChannel: u32,
    mReserved: u32,
}

#[repr(C)]
struct CMSampleBufferTimingInfo {
    duration: objc2_core_media::CMTime,
    presentationTimeStamp: objc2_core_media::CMTime,
    decodeTimeStamp: objc2_core_media::CMTime,
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetImageBuffer(sbuf: *const CMSampleBuffer) -> CVPixelBufferRef;
    fn CVPixelBufferLockBaseAddress(pixel_buffer: CVPixelBufferRef, lock_flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pixel_buffer: CVPixelBufferRef, lock_flags: u64) -> i32;
    fn CVPixelBufferGetWidth(pixel_buffer: CVPixelBufferRef) -> usize;
    fn CVPixelBufferGetHeight(pixel_buffer: CVPixelBufferRef) -> usize;
    fn CVPixelBufferGetBaseAddress(pixel_buffer: CVPixelBufferRef) -> *mut std::ffi::c_void;
    fn CVPixelBufferGetBytesPerRow(pixel_buffer: CVPixelBufferRef) -> usize;

    fn CMSampleBufferGetSampleTimingInfo(
        sbuf: *const CMSampleBuffer,
        index: isize,
        timing_info_out: *mut CMSampleBufferTimingInfo,
    ) -> i32;

    fn CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        sbuf: *const CMSampleBuffer,
        buffer_list_size_needed_out: *mut usize,
        buffer_list: *mut AudioBufferList,
        buffer_list_size: isize,
        block_buffer_structure_allocator: *const std::ffi::c_void,
        block_buffer_block_allocator: *const std::ffi::c_void,
        flags: u32,
        block_buffer_out: *mut *mut std::ffi::c_void,
    ) -> i32;

    fn CMSampleBufferGetNumSamples(sbuf: *const CMSampleBuffer) -> isize;
    fn CMSampleBufferGetFormatDescription(sbuf: *const CMSampleBuffer) -> CMFormatDescriptionRef;
    fn CMAudioFormatDescriptionGetStreamBasicDescription(
        desc: CMFormatDescriptionRef,
    ) -> *const AudioStreamBasicDescription;
    fn CMFormatDescriptionGetMediaSubType(desc: CMFormatDescriptionRef) -> u32;

    fn CFRelease(cf: *const std::ffi::c_void);
}

unsafe fn cmsamplebuffer_get_image_buffer(sbuf: &CMSampleBuffer) -> Option<CVPixelBufferRef> {
    let ptr = CMSampleBufferGetImageBuffer(sbuf as *const _);
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

unsafe fn cvpixelbuffer_lock_base_address(buf: CVPixelBufferRef, flags: u64) -> i32 {
    CVPixelBufferLockBaseAddress(buf, flags)
}

unsafe fn cvpixelbuffer_unlock_base_address(buf: CVPixelBufferRef, flags: u64) -> i32 {
    CVPixelBufferUnlockBaseAddress(buf, flags)
}

unsafe fn cvpixelbuffer_get_width(buf: CVPixelBufferRef) -> usize {
    CVPixelBufferGetWidth(buf)
}

unsafe fn cvpixelbuffer_get_height(buf: CVPixelBufferRef) -> usize {
    CVPixelBufferGetHeight(buf)
}

unsafe fn cvpixelbuffer_get_base_address(buf: CVPixelBufferRef) -> *mut std::ffi::c_void {
    CVPixelBufferGetBaseAddress(buf)
}

unsafe fn cvpixelbuffer_get_bytes_per_row(buf: CVPixelBufferRef) -> usize {
    CVPixelBufferGetBytesPerRow(buf)
}

unsafe fn cmsamplebuffer_get_format_description(
    sbuf: &CMSampleBuffer,
) -> Option<CMFormatDescriptionRef> {
    let ptr = CMSampleBufferGetFormatDescription(sbuf as *const _);
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

unsafe fn cmformat_description_get_stream_basic_description(
    desc: CMFormatDescriptionRef,
) -> *const AudioStreamBasicDescription {
    CMAudioFormatDescriptionGetStreamBasicDescription(desc)
}

unsafe fn cmsamplebuffer_get_num_samples(sbuf: &CMSampleBuffer) -> isize {
    CMSampleBufferGetNumSamples(sbuf as *const _)
}

unsafe fn cf_release(ptr: *const std::ffi::c_void) {
    CFRelease(ptr);
}

/// Validate a CMTime and convert to nanoseconds.
/// Factored for testability: the flag/value checks can be tested without FFI.
fn cmtime_to_nanos(pts: &objc2_core_media::CMTime) -> Option<u64> {
    // kCMTimeFlags_Valid must be set per Apple's CMTime specification.
    if !pts.flags.contains(CMTimeFlags::Valid) {
        return None;
    }
    // Reject special sentinel values: infinity and indefinite.
    if pts.flags.intersects(
        CMTimeFlags::PositiveInfinity | CMTimeFlags::NegativeInfinity | CMTimeFlags::Indefinite,
    ) {
        return None;
    }
    // Negative PTS would wrap when cast to u64.
    if pts.value < 0 {
        return None;
    }
    if pts.timescale > 0 {
        let value = pts.value as u128;
        let scale = pts.timescale as u128;
        let nanos = value.checked_mul(1_000_000_000)?.checked_div(scale)?;
        return u64::try_from(nanos).ok();
    }
    None
}

/// Extract display timestamp from CMSampleBuffer as nanoseconds.
/// Returns `None` when the sample buffer has no valid presentation timestamp.
unsafe fn extract_timestamp_nanos(sample_buffer: &CMSampleBuffer) -> Option<u64> {
    let mut timing_info = std::mem::MaybeUninit::<CMSampleBufferTimingInfo>::uninit();
    let status =
        CMSampleBufferGetSampleTimingInfo(sample_buffer as *const _, 0, timing_info.as_mut_ptr());

    if status == 0 {
        let info = timing_info.assume_init();
        cmtime_to_nanos(&info.presentationTimeStamp)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// MacScreenCapture — implements ScreenCapture + AudioCapture
// ---------------------------------------------------------------------------

/// macOS ScreenCaptureKit adapter that captures both video and system audio
/// through a single SCStream.
///
/// System audio requires macOS 13+ and Screen Recording permission.
/// Audio and video timestamps are naturally aligned (same clock base).
pub struct MacScreenCapture {
    stream: Option<SendSCStream>,
    delegate: Option<Retained<StreamOutput>>,
    running: bool,
    /// True when a previous stop timed out. The stream/delegate handles are
    /// kept alive (SCKit callbacks may still reference them), but `running`
    /// is false so the caller can attempt a fresh start.
    needs_reset: bool,
    audio_sink: Option<AudioChunkSink>,
    last_capture_geometry: Option<CaptureGeometry>,
}

impl MacScreenCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
            delegate: None,
            running: false,
            needs_reset: false,
            audio_sink: None,
            last_capture_geometry: None,
        }
    }

    /// Read display geometry from the selected SCDisplay for cursor
    /// coordinate normalization.
    ///
    /// # Safety
    /// Caller must ensure `display` is a valid SCDisplay reference.
    unsafe fn read_display_geometry(
        display: &SCDisplay,
        stream_width: u32,
        stream_height: u32,
    ) -> CaptureGeometry {
        let display_id = display.displayID();
        let frame = display.frame();
        // pointPixelScale is not on SCDisplay; derive from stream vs display point size.
        let point_pixel_scale = if frame.size.width > 0.0 {
            stream_width as f32 / frame.size.width as f32
        } else {
            1.0
        };

        // 诊断日志：仅打印一次，用于人工确认坐标来源。
        eprintln!(
            "[cursor-geometry] display_id={} frame_origin=({:.1}, {:.1}) frame_size=({:.1}×{:.1}) \
             stream={}×{} point_pixel_scale={:.2}",
            display_id,
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            stream_width,
            stream_height,
            point_pixel_scale
        );

        CaptureGeometry {
            display_id,
            content_origin_x: frame.origin.x as f32,
            content_origin_y: frame.origin.y as f32,
            content_width: frame.size.width as f32,
            content_height: frame.size.height as f32,
            point_pixel_scale,
            stream_width,
            stream_height,
        }
    }

    fn window_geometry_from_content_rect(
        origin_x: f64,
        origin_y: f64,
        width_points: f64,
        height_points: f64,
        point_pixel_scale: f32,
    ) -> CaptureGeometry {
        let scale = if point_pixel_scale.is_finite() && point_pixel_scale > 0.0 {
            point_pixel_scale
        } else {
            1.0
        };
        let stream_width = ((width_points * scale as f64).round().max(1.0)) as u32;
        let stream_height = ((height_points * scale as f64).round().max(1.0)) as u32;

        CaptureGeometry {
            display_id: 0,
            content_origin_x: origin_x as f32,
            content_origin_y: origin_y as f32,
            content_width: width_points as f32,
            content_height: height_points as f32,
            point_pixel_scale: scale,
            stream_width,
            stream_height,
        }
    }

    /// Returns the capture geometry from the last `start_stream()` call.
    pub fn last_capture_geometry(&self) -> Option<CaptureGeometry> {
        self.last_capture_geometry
    }

    /// Get the first video frame's raw PTS nanos and session entry nanos.
    /// Returns (first_pts_nanos, session_entry_nanos) if a frame has been received.
    pub fn first_frame_timing(&self) -> Option<(u64, u64)> {
        self.delegate
            .as_ref()
            .and_then(|d| d.ivars().pts_origin.lock().ok())
            .and_then(|guard| *guard)
    }

    /// Get the actual CVPixelBuffer size of the first video frame (width, height).
    pub fn first_frame_actual_size(&self) -> Option<(u32, u32)> {
        self.delegate
            .as_ref()
            .and_then(|d| d.ivars().first_frame_actual_size.lock().ok())
            .and_then(|guard| *guard)
    }

    /// Creates and starts an SCStream with the given configuration.
    #[allow(unused_unsafe)]
    fn start_stream(
        &mut self,
        config: &CaptureConfig,
        capture_system_audio: bool,
        video_sink: VideoFrameSink,
        audio_sink: AudioChunkSink,
        session_clock: Arc<crate::core::clock::SessionClock>,
    ) -> AppResult<()> {
        use objc2_foundation::NSArray;

        // Conservative policy: if a previous stop timed out, the native
        // stream lifecycle is uncertain — SCKit may still invoke callbacks
        // on the old delegate/stream. Dropping them could cause use-after-free.
        // Refuse to start until the app is restarted.
        if self.needs_reset {
            return Err(AppError::NativeCaptureUnavailable {
                reason: "上次停止录制超时，请重启应用后再试",
            });
        }

        // ⚠️ 人工审查：此调用阻塞当前线程等待异步回调。
        // 不应在 Tauri 主线程调用，否则可能死锁。
        let content = Self::get_shareable_content_sync()?;
        let displays = unsafe { content.displays() };

        if displays.count() == 0 {
            return Err(AppError::CaptureFailed {
                reason: "未找到可用显示器".to_string(),
            });
        }

        let display = unsafe { displays.objectAtIndex(0) };

        // Read display geometry for cursor coordinate normalization.
        let capture_geometry =
            unsafe { Self::read_display_geometry(&display, config.width, config.height) };
        self.last_capture_geometry = Some(capture_geometry);

        // Create content filter: capture the entire display.
        let empty_windows: Retained<NSArray<objc2_screen_capture_kit::SCWindow>> =
            unsafe { NSArray::new() };
        let filter = unsafe {
            SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &empty_windows,
            )
        };

        // Configure the stream.
        let stream_config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            stream_config.setWidth(config.width as usize);
            stream_config.setHeight(config.height as usize);
            stream_config.setCapturesAudio(capture_system_audio);
            stream_config.setSampleRate(48000);
            stream_config.setChannelCount(2);
            stream_config.setShowsCursor(config.show_system_cursor);
            stream_config.setQueueDepth(8);

            // BGRA pixel format (0x42475241 = 'BGRA')
            stream_config.setPixelFormat(0x42475241);

            // Frame interval: CMTime(value=1, timescale=fps) for target FPS.
            // kCMTimeValid = 1 << 0 = 1
            let frame_interval = objc2_core_media::CMTime {
                value: 1,
                timescale: config.fps as i32,
                flags: objc2_core_media::CMTimeFlags(1),
                epoch: 0,
            };
            stream_config.setMinimumFrameInterval(frame_interval);
        }

        // Create the stream delegate.
        let delegate = StreamOutput::new(video_sink, audio_sink, session_clock);

        // Create the SCStream.
        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &stream_config,
                Some(objc2::runtime::ProtocolObject::from_ref(&*delegate)),
            )
        };

        // Add stream output for screen (video) frames.
        unsafe {
            if let Err(e) = stream.addStreamOutput_type_sampleHandlerQueue_error(
                objc2::runtime::ProtocolObject::from_ref(&*delegate),
                SCStreamOutputType::Screen,
                None,
            ) {
                return Err(AppError::CaptureFailed {
                    reason: format!("添加视频输出失败: {}", e),
                });
            }
        }

        // Add stream output for system audio (only if enabled).
        if capture_system_audio {
            unsafe {
                if let Err(e) = stream.addStreamOutput_type_sampleHandlerQueue_error(
                    objc2::runtime::ProtocolObject::from_ref(&*delegate),
                    SCStreamOutputType::Audio,
                    None,
                ) {
                    return Err(AppError::CaptureFailed {
                        reason: format!("添加音频输出失败: {}", e),
                    });
                }
            }
        }

        // Start capture and wait for completion.
        let (tx, rx) = std::sync::mpsc::channel();

        unsafe {
            stream.startCaptureWithCompletionHandler(Some(&block2::RcBlock::new(
                move |error: *mut NSError| {
                    let result = if error.is_null() {
                        Ok(())
                    } else {
                        let error_ref = unsafe { &*error };
                        Err(AppError::CaptureFailed {
                            reason: format!("启动捕获失败: {}", error_ref),
                        })
                    };
                    let _ = tx.send(result);
                },
            )));
        }

        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(result) => result?,
            Err(_) => {
                return Err(AppError::CaptureFailed {
                    reason: "启动捕获超时".to_string(),
                });
            }
        }

        self.stream = Some(SendSCStream(stream));
        self.delegate = Some(delegate);
        self.running = true;

        Ok(())
    }

    /// Creates and starts an SCStream for window capture.
    #[allow(unused_unsafe)]
    pub fn start_window_stream(
        &mut self,
        window_id: u32,
        capture_system_audio: bool,
        show_system_cursor: bool,
        video_sink: VideoFrameSink,
        audio_sink: AudioChunkSink,
        session_clock: Arc<crate::core::clock::SessionClock>,
    ) -> AppResult<()> {
        // Conservative policy: if a previous stop timed out, refuse to start.
        if self.needs_reset {
            return Err(AppError::NativeCaptureUnavailable {
                reason: "上次停止录制超时，请重启应用后再试",
            });
        }

        // Get SCShareableContent
        let content = Self::get_shareable_content_sync()?;
        let windows = unsafe { content.windows() };

        // Find target window
        let target_window = (0..windows.count())
            .map(|i| unsafe { windows.objectAtIndex(i) })
            .find(|w| unsafe { w.windowID() } == window_id)
            .ok_or(AppError::WindowNotFound { window_id })?;

        // Verify window is on screen
        if !unsafe { target_window.isOnScreen() } {
            return Err(AppError::WindowMinimized { window_id });
        }

        // Create content filter: capture only the target window
        let filter = unsafe {
            SCContentFilter::initWithDesktopIndependentWindow(
                SCContentFilter::alloc(),
                &target_window,
            )
        };

        let content_rect = unsafe { filter.contentRect() };
        let point_pixel_scale = unsafe { filter.pointPixelScale() };
        let capture_geometry = Self::window_geometry_from_content_rect(
            content_rect.origin.x,
            content_rect.origin.y,
            content_rect.size.width,
            content_rect.size.height,
            point_pixel_scale,
        );
        let stream_width = capture_geometry.stream_width;
        let stream_height = capture_geometry.stream_height;
        self.last_capture_geometry = Some(capture_geometry);

        // Configure stream
        let stream_config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            stream_config.setWidth(stream_width as usize);
            stream_config.setHeight(stream_height as usize);
            stream_config.setCapturesAudio(capture_system_audio);
            stream_config.setSampleRate(48000);
            stream_config.setChannelCount(2);
            stream_config.setShowsCursor(show_system_cursor);
            stream_config.setQueueDepth(8);
            stream_config.setPixelFormat(0x42475241); // BGRA

            let frame_interval = objc2_core_media::CMTime {
                value: 1,
                timescale: 30, // Default 30fps for window capture
                flags: objc2_core_media::CMTimeFlags(1),
                epoch: 0,
            };
            stream_config.setMinimumFrameInterval(frame_interval);
        }

        // Create stream delegate
        let delegate = StreamOutput::new(video_sink, audio_sink, session_clock);

        // Create SCStream
        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &stream_config,
                Some(objc2::runtime::ProtocolObject::from_ref(&*delegate)),
            )
        };

        // Add video output
        unsafe {
            if let Err(e) = stream.addStreamOutput_type_sampleHandlerQueue_error(
                objc2::runtime::ProtocolObject::from_ref(&*delegate),
                SCStreamOutputType::Screen,
                None,
            ) {
                return Err(AppError::CaptureFailed {
                    reason: format!("添加视频输出失败: {}", e),
                });
            }
        }

        // Add audio output (if enabled)
        if capture_system_audio {
            unsafe {
                if let Err(e) = stream.addStreamOutput_type_sampleHandlerQueue_error(
                    objc2::runtime::ProtocolObject::from_ref(&*delegate),
                    SCStreamOutputType::Audio,
                    None,
                ) {
                    return Err(AppError::CaptureFailed {
                        reason: format!("添加音频输出失败: {}", e),
                    });
                }
            }
        }

        // Start capture and wait for completion
        let (tx, rx) = std::sync::mpsc::channel();

        unsafe {
            stream.startCaptureWithCompletionHandler(Some(&block2::RcBlock::new(
                move |error: *mut NSError| {
                    let result = if error.is_null() {
                        Ok(())
                    } else {
                        let error_ref = unsafe { &*error };
                        Err(AppError::CaptureFailed {
                            reason: format!("启动捕获失败: {}", error_ref),
                        })
                    };
                    let _ = tx.send(result);
                },
            )));
        }

        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(result) => result?,
            Err(_) => {
                return Err(AppError::CaptureFailed {
                    reason: "启动捕获超时".to_string(),
                });
            }
        }

        self.stream = Some(SendSCStream(stream));
        self.delegate = Some(delegate);
        self.running = true;

        Ok(())
    }

    /// Synchronously fetches SCShareableContent.
    pub fn get_shareable_content_sync() -> AppResult<Retained<SCShareableContent>> {
        let (tx, rx) = std::sync::mpsc::channel();

        unsafe {
            SCShareableContent::getShareableContentWithCompletionHandler(&block2::RcBlock::new(
                move |content: *mut SCShareableContent, error: *mut NSError| {
                    if error.is_null() && !content.is_null() {
                        // SAFETY: content is non-null and we verified no error.
                        if let Some(retained) = Retained::retain(content) {
                            let _ = tx.send(Some(retained));
                        } else {
                            let _ = tx.send(None);
                        }
                    } else {
                        let _ = tx.send(None);
                    }
                },
            ));
        }

        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(opt) => match opt {
                Some(content) => Ok(content),
                None => Err(AppError::CaptureFailed {
                    reason: "获取可共享内容失败".to_string(),
                }),
            },
            Err(_) => Err(AppError::CaptureFailed {
                reason: "获取可共享内容超时".to_string(),
            }),
        }
    }
}

impl Default for MacScreenCapture {
    fn default() -> Self {
        Self::new()
    }
}

// --- ScreenCapture Trait ---

impl ScreenCapture for MacScreenCapture {
    fn start(&mut self, config: CaptureConfig, sink: VideoFrameSink) -> AppResult<()> {
        if self.running {
            return Err(AppError::InvalidState {
                current: "recording",
                action: "start",
            });
        }

        // Audio sink must be injected before start via AudioCapture::start().
        // If not set, create a dummy bounded channel that drops all audio.
        let audio_sink = self.audio_sink.take().unwrap_or_else(|| {
            let (tx, _rx) = crate::core::media_channel::bounded_media_channel(1, "audio_fallback");
            tx
        });

        let session_clock = Arc::new(crate::core::clock::SessionClock::new());
        self.start_stream(&config, true, sink, audio_sink, session_clock)
    }

    fn stop(&mut self) -> AppResult<()> {
        if !self.running {
            return Ok(());
        }

        // Clear sinks first to prevent any more frames/chunks from being sent.
        if let Some(delegate) = &self.delegate {
            delegate.clear_sinks();
        }

        // Stop the stream asynchronously and wait for completion.
        // ⚠️ 人工审查：stopCaptureWithCompletionHandler 是异步的。
        // 我们等待完成后再清理资源，避免 use-after-free。
        if let Some(SendSCStream(stream)) = &self.stream {
            let (tx, rx) = std::sync::mpsc::channel();

            unsafe {
                stream.stopCaptureWithCompletionHandler(Some(&block2::RcBlock::new(
                    move |_error: *mut NSError| {
                        let _ = tx.send(());
                    },
                )));
            }

            match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(()) => {
                    // Normal stop: clean up all handles.
                    self.stream = None;
                    self.delegate = None;
                    self.running = false;
                    self.needs_reset = false;
                }
                Err(_) => {
                    // Timeout: keep native handles alive (SCKit callbacks
                    // may still reference them), but mark running=false so
                    // the caller can retry. Stale handles are dropped in
                    // start_stream() via needs_reset.
                    self.running = false;
                    self.needs_reset = true;
                    return Err(AppError::CaptureStopTimeout {
                        reason: "ScreenCaptureKit stopCaptureWithCompletionHandler 未在 5 秒内回调"
                            .to_string(),
                    });
                }
            }
        } else {
            self.running = false;
            self.needs_reset = false;
        }

        Ok(())
    }

    fn capabilities(&self) -> CaptureCapabilities {
        CaptureCapabilities {
            supports_full_screen: true,
            supports_window: true,
            supports_region: false,
            supports_4k: false,
        }
    }
}

impl WindowCapture for MacScreenCapture {
    fn list_windows(&self) -> AppResult<Vec<WindowInfo>> {
        window_list::list_windows()
    }

    fn get_thumbnail(&self, window_id: u32) -> AppResult<Option<String>> {
        window_list::get_window_thumbnail(window_id)
    }

    fn start_window_stream(
        &mut self,
        window_id: u32,
        capture_system_audio: bool,
        show_system_cursor: bool,
        video_sink: VideoFrameSink,
        audio_sink: AudioChunkSink,
        session_clock: Arc<crate::core::clock::SessionClock>,
    ) -> AppResult<()> {
        MacScreenCapture::start_window_stream(
            self,
            window_id,
            capture_system_audio,
            show_system_cursor,
            video_sink,
            audio_sink,
            session_clock,
        )
    }

    fn stop_window_stream(&mut self) -> AppResult<()> {
        MacScreenCapture::stop_window_stream(self)
    }

    fn window_state(&self, window_id: u32) -> AppResult<WindowRecordingState> {
        let windows = window_list::list_windows()?;
        if let Some(window) = windows.iter().find(|window| window.window_id == window_id) {
            if window.is_on_screen {
                Ok(WindowRecordingState::Recording)
            } else {
                Ok(WindowRecordingState::Minimized)
            }
        } else {
            Ok(WindowRecordingState::Closed)
        }
    }
}

// --- Combined start for MacRecordingService ---

impl MacScreenCapture {
    /// Starts both video and system audio capture in a single SCStream.
    ///
    /// This is the preferred entry point for `MacRecordingService` because the
    /// SCStream is a unified pipeline — video and system audio come from the same
    /// stream. Calling `AudioCapture::start()` separately to inject the audio sink
    /// is fragile; this method sets both sinks atomically.
    pub fn start_combined(
        &mut self,
        config: CaptureConfig,
        capture_system_audio: bool,
        video_sink: VideoFrameSink,
        audio_sink: AudioChunkSink,
        session_clock: Arc<crate::core::clock::SessionClock>,
    ) -> AppResult<()> {
        if self.running {
            return Err(AppError::InvalidState {
                current: "recording",
                action: "start",
            });
        }
        self.start_stream(
            &config,
            capture_system_audio,
            video_sink,
            audio_sink,
            session_clock,
        )
    }

    /// Stops window capture stream.
    pub fn stop_window_stream(&mut self) -> AppResult<()> {
        if !self.running {
            return Ok(());
        }

        // Clear sinks first to prevent any more frames/chunks from being sent.
        if let Some(delegate) = &self.delegate {
            delegate.clear_sinks();
        }

        // Stop the stream asynchronously and wait for completion.
        if let Some(SendSCStream(stream)) = &self.stream {
            let (tx, rx) = std::sync::mpsc::channel();

            unsafe {
                stream.stopCaptureWithCompletionHandler(Some(&block2::RcBlock::new(
                    move |_error: *mut NSError| {
                        let _ = tx.send(());
                    },
                )));
            }

            match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(()) => {
                    // Normal stop: clean up all handles.
                    self.stream = None;
                    self.delegate = None;
                    self.running = false;
                    self.needs_reset = false;
                }
                Err(_) => {
                    // Timeout: keep native handles alive but mark running=false.
                    self.running = false;
                    self.needs_reset = true;
                    return Err(AppError::CaptureStopTimeout {
                        reason: "ScreenCaptureKit stopCaptureWithCompletionHandler 未在 5 秒内回调"
                            .to_string(),
                    });
                }
            }
        } else {
            self.running = false;
            self.needs_reset = false;
        }

        Ok(())
    }
}

// --- AudioCapture Trait ---
//
// MacScreenCapture implements AudioCapture for system audio via the same
// SCStream. The audio sink is stored here and consumed when ScreenCapture::start()
// creates the stream.

impl AudioCapture for MacScreenCapture {
    fn start(&mut self, _config: AudioConfig, sink: AudioChunkSink) -> AppResult<()> {
        self.audio_sink = Some(sink);
        Ok(())
    }

    fn stop(&mut self) -> AppResult<()> {
        self.audio_sink = None;
        Ok(())
    }

    fn device_list(&self) -> AppResult<Vec<AudioDevice>> {
        Ok(vec![AudioDevice {
            id: "system_audio".to_string(),
            name: "系统音频".to_string(),
            is_default: true,
        }])
    }

    fn capabilities(&self) -> AudioCapabilities {
        AudioCapabilities {
            supports_system_audio: true,
            supports_microphone: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Audio format classification and sample conversion helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PcmSampleFormat {
    Float32,
    SignedInt16,
}

const K_AUDIO_FORMAT_LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
const K_AUDIO_FORMAT_FLAG_IS_FLOAT: u32 = 1 << 0;
const K_AUDIO_FORMAT_FLAG_IS_SIGNED_INTEGER: u32 = 1 << 2;
const K_AUDIO_FORMAT_FLAG_IS_NON_INTERLEAVED: u32 = 1 << 5;

fn classify_pcm_format(basic: &AudioStreamBasicDescription) -> AppResult<PcmSampleFormat> {
    if basic.mFormatID != K_AUDIO_FORMAT_LINEAR_PCM {
        return Err(AppError::AudioCaptureFailed {
            reason: format!("不支持的音频格式 ID: {}", basic.mFormatID),
        });
    }

    if basic.mBitsPerChannel == 32 && (basic.mFormatFlags & K_AUDIO_FORMAT_FLAG_IS_FLOAT) != 0 {
        return Ok(PcmSampleFormat::Float32);
    }

    if basic.mBitsPerChannel == 16
        && (basic.mFormatFlags & K_AUDIO_FORMAT_FLAG_IS_SIGNED_INTEGER) != 0
    {
        return Ok(PcmSampleFormat::SignedInt16);
    }

    Err(AppError::AudioCaptureFailed {
        reason: format!(
            "不支持的 PCM 位深或标志: bits={}, flags={}",
            basic.mBitsPerChannel, basic.mFormatFlags
        ),
    })
}

/// Validates critical ASBD fields to catch malformed audio buffers early.
fn validate_asbd(basic: &AudioStreamBasicDescription) -> AppResult<()> {
    if basic.mFramesPerPacket == 0 {
        return Err(AppError::AudioCaptureFailed {
            reason: "mFramesPerPacket 为 0".to_string(),
        });
    }
    if basic.mBytesPerFrame == 0 {
        return Err(AppError::AudioCaptureFailed {
            reason: "mBytesPerFrame 为 0".to_string(),
        });
    }
    if basic.mChannelsPerFrame == 0 {
        return Err(AppError::AudioCaptureFailed {
            reason: "mChannelsPerFrame 为 0".to_string(),
        });
    }
    Ok(())
}

/// Returns `true` when the ASBD indicates non-interleaved channel layout.
fn is_non_interleaved(basic: &AudioStreamBasicDescription) -> bool {
    (basic.mFormatFlags & K_AUDIO_FORMAT_FLAG_IS_NON_INTERLEAVED) != 0
}

/// Deinterleaves per-channel buffers into a single interleaved L,R,L,R,... sequence.
///
/// Each buffer in `buffers` contains one channel of `frames` samples.
/// `channels` is the total number of channels expected in the output.
fn deinterleave_buffers(
    buffers: &[AudioBuffer],
    channels: usize,
    format: PcmSampleFormat,
) -> Vec<f32> {
    if buffers.is_empty() || channels == 0 {
        return Vec::new();
    }

    let bytes_per_sample = match format {
        PcmSampleFormat::Float32 => 4,
        PcmSampleFormat::SignedInt16 => 2,
    };

    let frames = if buffers[0].mDataByteSize > 0 {
        buffers[0].mDataByteSize as usize / bytes_per_sample
    } else {
        return Vec::new();
    };

    // Convert each channel buffer to f32 independently.
    let channel_samples: Vec<Vec<f32>> = buffers
        .iter()
        .take(channels)
        .filter(|b| !b.mData.is_null() && b.mDataByteSize > 0)
        .map(|b| {
            let bytes = unsafe {
                std::slice::from_raw_parts(b.mData as *const u8, b.mDataByteSize as usize)
            };
            convert_pcm_bytes_to_f32(format, bytes)
        })
        .collect();

    if channel_samples.is_empty() || channel_samples[0].len() != frames {
        return Vec::new();
    }

    // Interleave: L0,R0,L1,R1,...
    let mut interleaved = Vec::with_capacity(frames * channels);
    for frame_idx in 0..frames {
        for channel in &channel_samples {
            interleaved.push(channel[frame_idx]);
        }
    }
    interleaved
}

fn convert_pcm_bytes_to_f32(format: PcmSampleFormat, bytes: &[u8]) -> Vec<f32> {
    match format {
        PcmSampleFormat::Float32 => bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect(),
        PcmSampleFormat::SignedInt16 => bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_ne_bytes([chunk[0], chunk[1]]) as f32 / i16::MAX as f32)
            .collect(),
    }
}

#[cfg(test)]
mod audio_conversion_tests {
    use super::*;

    #[test]
    fn converts_float32_pcm_bytes() {
        let bytes = [0.5f32.to_ne_bytes(), (-0.25f32).to_ne_bytes()].concat();

        let samples = convert_pcm_bytes_to_f32(PcmSampleFormat::Float32, &bytes);

        assert!((samples[0] - 0.5).abs() < 1e-6);
        assert!((samples[1] - (-0.25)).abs() < 1e-6);
    }

    #[test]
    fn converts_signed_int16_pcm_bytes() {
        let bytes = [i16::MAX.to_ne_bytes(), 0i16.to_ne_bytes()].concat();

        let samples = convert_pcm_bytes_to_f32(PcmSampleFormat::SignedInt16, &bytes);

        assert!((samples[0] - 1.0).abs() < 1e-6);
        assert_eq!(samples[1], 0.0);
    }

    #[test]
    fn deinterleaves_float32_stereo() {
        let left = [0.5f32, 0.3f32];
        let right = [0.25f32, 0.1f32];
        let left_bytes: Vec<u8> = left.iter().flat_map(|f| f.to_ne_bytes()).collect();
        let right_bytes: Vec<u8> = right.iter().flat_map(|f| f.to_ne_bytes()).collect();

        let buffers = [
            AudioBuffer {
                mNumberChannels: 1,
                mDataByteSize: left_bytes.len() as u32,
                mData: left_bytes.as_ptr() as *mut _,
            },
            AudioBuffer {
                mNumberChannels: 1,
                mDataByteSize: right_bytes.len() as u32,
                mData: right_bytes.as_ptr() as *mut _,
            },
        ];

        let result = deinterleave_buffers(&buffers, 2, PcmSampleFormat::Float32);

        assert_eq!(result.len(), 4);
        assert!((result[0] - 0.5).abs() < 1e-6);
        assert!((result[1] - 0.25).abs() < 1e-6);
        assert!((result[2] - 0.3).abs() < 1e-6);
        assert!((result[3] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn validate_asbd_rejects_zero_frames_per_packet() {
        let asbd = AudioStreamBasicDescription {
            mSampleRate: 48000.0,
            mFormatID: K_AUDIO_FORMAT_LINEAR_PCM,
            mFormatFlags: K_AUDIO_FORMAT_FLAG_IS_FLOAT,
            mBytesPerPacket: 8,
            mFramesPerPacket: 0,
            mBytesPerFrame: 8,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        assert!(validate_asbd(&asbd).is_err());
    }

    #[test]
    fn validate_asbd_accepts_valid_description() {
        let asbd = AudioStreamBasicDescription {
            mSampleRate: 48000.0,
            mFormatID: K_AUDIO_FORMAT_LINEAR_PCM,
            mFormatFlags: K_AUDIO_FORMAT_FLAG_IS_FLOAT,
            mBytesPerPacket: 8,
            mFramesPerPacket: 1,
            mBytesPerFrame: 8,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        assert!(validate_asbd(&asbd).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_capture_exposes_phase_one_capabilities() {
        let capture = MacScreenCapture::new();
        let capabilities = ScreenCapture::capabilities(&capture);

        assert!(capabilities.supports_full_screen);
        assert!(capabilities.supports_window);
        assert!(!capabilities.supports_region);
    }

    #[test]
    fn window_geometry_uses_content_rect_scale_for_stream_pixels() {
        let geometry =
            MacScreenCapture::window_geometry_from_content_rect(10.0, 20.0, 720.0, 450.0, 2.0_f32);

        assert_eq!(geometry.content_origin_x, 10.0);
        assert_eq!(geometry.content_origin_y, 20.0);
        assert_eq!(geometry.content_width, 720.0);
        assert_eq!(geometry.content_height, 450.0);
        assert_eq!(geometry.point_pixel_scale, 2.0);
        assert_eq!(geometry.stream_width, 1440);
        assert_eq!(geometry.stream_height, 900);
    }

    #[test]
    fn mac_audio_capture_supports_system_audio() {
        let capture = MacScreenCapture::new();
        let caps = AudioCapture::capabilities(&capture);

        assert!(caps.supports_system_audio);
        assert!(!caps.supports_microphone);
    }

    #[test]
    fn mac_audio_device_list_returns_system_audio() {
        let capture = MacScreenCapture::new();
        let devices = capture.device_list().unwrap();

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "系统音频");
        assert!(devices[0].is_default);
    }

    #[test]
    fn mac_capture_stop_when_not_running_succeeds() {
        let mut capture = MacScreenCapture::new();
        assert!(ScreenCapture::stop(&mut capture).is_ok());
    }

    #[test]
    fn mac_audio_stop_when_not_running_succeeds() {
        let mut capture = MacScreenCapture::new();
        assert!(AudioCapture::stop(&mut capture).is_ok());
    }

    #[test]
    fn pts_normalize_first_call_establishes_origin() {
        let mut origin: Option<(u64, u64)> = None;
        let result = compute_normalized_pts(&mut origin, 100_000_000, 5_000_000);
        // First PTS maps to session elapsed time at that point.
        assert_eq!(result, 5_000_000);
        assert_eq!(origin, Some((100_000_000, 5_000_000)));
    }

    #[test]
    fn pts_normalize_second_call_shifts_by_first_pts() {
        let mut origin = Some((100_000_000, 5_000_000));
        let result = compute_normalized_pts(&mut origin, 200_000_000, 105_000_000);
        // 200M - 100M + 5M = 105M
        assert_eq!(result, 105_000_000);
        // Origin unchanged.
        assert_eq!(origin, Some((100_000_000, 5_000_000)));
    }

    #[test]
    fn pts_normalize_later_valid_pts_not_compressed_by_invalid_first() {
        // If an invalid PTS were to establish origin (e.g. 0), a later valid PTS
        // would produce a huge shift. This test simulates that the first call
        // that actually establishes origin uses the real PTS.
        let mut origin: Option<(u64, u64)> = None;
        // Valid first call with real PTS.
        let first = compute_normalized_pts(&mut origin, 1_000_000_000, 10_000_000);
        assert_eq!(first, 10_000_000);
        // Later PTS: (2B - 1B + 10M) = 1.01B
        let second = compute_normalized_pts(&mut origin, 2_000_000_000, 1_010_000_000);
        assert_eq!(second, 1_010_000_000);
    }

    #[test]
    fn earlier_valid_pts_gets_lower_session_time() {
        // When an audio callback arrives first and establishes origin,
        // a subsequent video callback with an earlier PTS should map
        // to a proportionally lower session-relative time, not be
        // silently flattened to session_at_first.
        let mut origin = Some((200_000_000, 10_000_000));
        let result = compute_normalized_pts(&mut origin, 100_000_000, 0);
        // delta = 100M - 200M = -100M, result = -100M + 10M = -90M → max(0)
        assert_eq!(result, 0);
        // But a slightly earlier PTS preserves the ordering:
        let mut origin2 = Some((200_000_000, 10_000_000));
        let result2 = compute_normalized_pts(&mut origin2, 195_000_000, 0);
        // delta = 195M - 200M = -5M, result = -5M + 10M = 5M
        assert_eq!(result2, 5_000_000);
    }

    // --- cmtime_to_nanos validation tests ---

    fn make_cmtime(value: i64, timescale: i32, flags: CMTimeFlags) -> objc2_core_media::CMTime {
        objc2_core_media::CMTime {
            value,
            timescale,
            flags,
            epoch: 0,
        }
    }

    #[test]
    fn cmtime_valid_converts_to_nanos() {
        let pts = make_cmtime(1000, 600, CMTimeFlags::Valid);
        // 1000 / 600 ≈ 1.666... seconds → 1_666_666_666 ns
        let result = cmtime_to_nanos(&pts);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 1_666_666_666);
    }

    #[test]
    fn cmtime_missing_valid_flag_returns_none() {
        let pts = make_cmtime(1000, 600, CMTimeFlags::empty());
        assert_eq!(cmtime_to_nanos(&pts), None);
    }

    #[test]
    fn cmtime_positive_infinity_returns_none() {
        let pts = make_cmtime(0, 600, CMTimeFlags::Valid | CMTimeFlags::PositiveInfinity);
        assert_eq!(cmtime_to_nanos(&pts), None);
    }

    #[test]
    fn cmtime_indefinite_returns_none() {
        let pts = make_cmtime(0, 600, CMTimeFlags::Valid | CMTimeFlags::Indefinite);
        assert_eq!(cmtime_to_nanos(&pts), None);
    }

    #[test]
    fn cmtime_negative_value_returns_none() {
        let pts = make_cmtime(-1000, 600, CMTimeFlags::Valid);
        assert_eq!(cmtime_to_nanos(&pts), None);
    }

    #[test]
    fn cmtime_zero_timescale_returns_none() {
        let pts = make_cmtime(1000, 0, CMTimeFlags::Valid);
        assert_eq!(cmtime_to_nanos(&pts), None);
    }

    // --- i128 PTS normalization boundary tests ---

    #[test]
    fn pts_normalize_large_pts_beyond_i64_max_no_wrap() {
        let large_pts = (i64::MAX as u64) + 1;
        let mut origin = Some((0, 0));
        let result = compute_normalized_pts(&mut origin, large_pts, 0);
        // delta = large_pts - 0 = large_pts (fits in i128)
        assert_eq!(result, large_pts);
    }

    #[test]
    fn pts_normalize_large_first_pts_beyond_i64_max_no_wrap() {
        let large_first = (i64::MAX as u64) + 1;
        let mut origin = Some((large_first, 0));
        let result = compute_normalized_pts(&mut origin, large_first + 1000, 0);
        // delta = 1000
        assert_eq!(result, 1000);
    }

    #[test]
    fn pts_normalize_result_clamped_to_u64_max() {
        let mut origin = Some((0, u64::MAX));
        let result = compute_normalized_pts(&mut origin, 1, 0);
        // delta = 1, result = 1 + u64::MAX → clamped to u64::MAX
        assert_eq!(result, u64::MAX);
    }

    #[test]
    fn pts_normalize_extreme_negative_delta_clamped_to_zero() {
        let mut origin = Some((u64::MAX, 0));
        let result = compute_normalized_pts(&mut origin, 0, 0);
        // delta = 0 - u64::MAX → huge negative, result < 0 → clamp to 0
        assert_eq!(result, 0);
    }
}
