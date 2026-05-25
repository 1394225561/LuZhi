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
use objc2_core_media::CMSampleBuffer;
use objc2_foundation::NSError;
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCStreamOutput, SCStreamOutputType,
};

use crate::app::error::{AppError, AppResult};
use crate::core::capture::{
    AudioCapabilities, AudioCapture, AudioChunkSink, AudioConfig, AudioDevice, CaptureCapabilities,
    ScreenCapture, VideoFrameSink,
};
use crate::core::config::CaptureConfig;
use crate::core::frame::{AudioChunk, FrameBuffer, PixelFormat, VideoFrame};

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
    timestamp_normalizer: crate::core::clock::TimestampNormalizer,
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
    fn new(video_sink: VideoFrameSink, audio_sink: AudioChunkSink) -> Retained<Self> {
        let this = Self::alloc().set_ivars(StreamOutputIvars {
            video_sink: Mutex::new(Some(video_sink)),
            audio_sink: Mutex::new(Some(audio_sink)),
            timestamp_normalizer: crate::core::clock::TimestampNormalizer::default(),
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

/// Extracts BGRA pixel data from CMSampleBuffer and sends as VideoFrameRef.
///
/// # Safety
///
/// Reads pixel data from the CMSampleBuffer's CVPixelBuffer.
/// Data is copied into Arc<[u8]> before the sample buffer is released.
unsafe fn handle_video_frame(delegate: &StreamOutput, sample_buffer: &CMSampleBuffer) {
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

    let timestamp_nanos = extract_timestamp_nanos(sample_buffer);
    let timestamp = delegate
        .ivars()
        .timestamp_normalizer
        .normalize(timestamp_nanos);

    let frame = VideoFrame {
        timestamp,
        width: width as u32,
        height: height as u32,
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
        &mut block_buffer as *mut _,
    );

    if size_status != 0 || needed_size == 0 {
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
    let minimum_size =
        std::mem::size_of::<u32>() + num_buffers * std::mem::size_of::<AudioBuffer>();
    if needed_size < minimum_size {
        if !block_buffer.is_null() {
            cf_release(block_buffer as *const _);
        }
        return;
    }

    let mut samples_f32: Vec<f32> = Vec::new();
    let first_buffer = list_ref.mBuffers.as_ptr();
    for i in 0..num_buffers {
        let buffer = *first_buffer.add(i);
        if buffer.mData.is_null() || buffer.mDataByteSize == 0 {
            continue;
        }
        let bytes =
            std::slice::from_raw_parts(buffer.mData as *const u8, buffer.mDataByteSize as usize);
        samples_f32.extend(convert_pcm_bytes_to_f32(pcm_format, bytes));
    }

    if !block_buffer.is_null() {
        cf_release(block_buffer as *const _);
    }

    if samples_f32.is_empty() {
        return;
    }

    let timestamp_nanos = extract_timestamp_nanos(sample_buffer);
    let timestamp = delegate
        .ivars()
        .timestamp_normalizer
        .normalize(timestamp_nanos);

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

/// Extract display timestamp from CMSampleBuffer as nanoseconds.
unsafe fn extract_timestamp_nanos(sample_buffer: &CMSampleBuffer) -> u64 {
    let mut timing_info = std::mem::MaybeUninit::<CMSampleBufferTimingInfo>::uninit();
    let status =
        CMSampleBufferGetSampleTimingInfo(sample_buffer as *const _, 0, timing_info.as_mut_ptr());

    if status == 0 {
        let info = timing_info.assume_init();
        let pts = info.presentationTimeStamp;
        if pts.timescale > 0 {
            let seconds = pts.value as f64 / pts.timescale as f64;
            return (seconds * 1_000_000_000.0) as u64;
        }
    }
    0
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
    audio_sink: Option<AudioChunkSink>,
}

impl MacScreenCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
            delegate: None,
            running: false,
            audio_sink: None,
        }
    }

    /// Creates and starts an SCStream with the given configuration.
    #[allow(unused_unsafe)]
    fn start_stream(
        &mut self,
        config: &CaptureConfig,
        capture_system_audio: bool,
        video_sink: VideoFrameSink,
        audio_sink: AudioChunkSink,
    ) -> AppResult<()> {
        use objc2_foundation::NSArray;

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
            stream_config.setShowsCursor(true);
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
        let delegate = StreamOutput::new(video_sink, audio_sink);

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

    /// Synchronously fetches SCShareableContent.
    fn get_shareable_content_sync() -> AppResult<Retained<SCShareableContent>> {
        let (tx, rx) = std::sync::mpsc::channel();

        unsafe {
            SCShareableContent::getShareableContentWithCompletionHandler(&block2::RcBlock::new(
                move |content: *mut SCShareableContent, error: *mut NSError| {
                    if error.is_null() && !content.is_null() {
                        // SAFETY: content is non-null and we verified no error.
                        if let Some(retained) = unsafe { Retained::retain(content) } {
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
            let (tx, _rx) = crate::core::media_channel::bounded_media_channel(1);
            tx
        });

        self.start_stream(&config, true, sink, audio_sink)
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

            rx.recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|_| AppError::CaptureStopTimeout {
                    reason: "ScreenCaptureKit stopCaptureWithCompletionHandler 未在 5 秒内回调"
                        .to_string(),
                })?;
        }

        self.stream = None;
        self.delegate = None;
        self.running = false;

        Ok(())
    }

    fn capabilities(&self) -> CaptureCapabilities {
        CaptureCapabilities {
            supports_full_screen: true,
            supports_window: false,
            supports_region: false,
            supports_4k: false,
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
    ) -> AppResult<()> {
        if self.running {
            return Err(AppError::InvalidState {
                current: "recording",
                action: "start",
            });
        }
        self.start_stream(&config, capture_system_audio, video_sink, audio_sink)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_capture_exposes_phase_one_capabilities() {
        let capture = MacScreenCapture::new();
        let capabilities = ScreenCapture::capabilities(&capture);

        assert!(capabilities.supports_full_screen);
        assert!(!capabilities.supports_window);
        assert!(!capabilities.supports_region);
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
}
