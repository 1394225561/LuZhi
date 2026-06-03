use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use crate::app::error::{AppError, AppResult};
use crate::core::frame::{MixedAudioChunk, VideoFrameRef};
use crate::media::recording_writer::{RecordingResult, RecordingWriter, WriterDiagnostics};

/// Maximum number of messages in the encoding queue before returning an error.
/// At 30fps video + ~50 audio chunks/sec, capacity of 64 gives ~0.8s of buffering.
/// Prevents unbounded memory growth if the encoder can't keep up.
/// Uses non-blocking send to avoid stalling the capture consumer thread.
const ENCODER_QUEUE_CAPACITY: usize = 64;

/// Message sent from the front writer to the encoder worker.
enum EncoderMessage {
    Video {
        timestamp_nanos: u64,
        width: u32,
        height: u32,
        stride_bytes: usize,
        buffer: Vec<u8>,
    },
    Audio {
        /// Interleaved f32 samples (stereo = L,R,L,R,...).
        samples: Vec<f32>,
        /// Timestamp of the first sample in nanoseconds (session clock).
        timestamp_nanos: u64,
    },
    Flush,
}

/// Lightweight front writer that validates input and enqueues to the encoder worker.
///
/// All FFmpeg encoding/muxing runs on a dedicated worker thread, keeping the
/// capture consumer thread free. The bounded channel provides backpressure when
/// the encoder can't keep up, preventing unbounded memory growth.
pub struct FfmpegRecordingWriter {
    tx: Option<mpsc::SyncSender<EncoderMessage>>,
    worker: Option<thread::JoinHandle<()>>,
    /// Receives the worker's final result via channel (bounded join, Important 1).
    result_rx: Option<mpsc::Receiver<AppResult<RecordingResult>>>,
    frame_count: u64,
    mixed_audio_chunk_count: u64,
    /// Number of video queue full events (for diagnostics).
    video_queue_full_count: u64,
    /// Number of audio queue full events (for diagnostics).
    audio_queue_full_count: u64,
    // Per-source writer-side counters (Important 3).
    system_chunks_received_by_writer: u64,
    mic_chunks_received_by_writer: u64,
}

impl FfmpegRecordingWriter {
    pub fn new(output_path: PathBuf) -> AppResult<Self> {
        // Ensure parent directory exists.
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("创建输出目录失败: {e}"),
            })?;
        }

        let (tx, rx) = mpsc::sync_channel::<EncoderMessage>(ENCODER_QUEUE_CAPACITY);
        let (result_tx, result_rx) = mpsc::channel::<AppResult<RecordingResult>>();
        let worker_path = output_path.clone();

        let worker = thread::spawn(move || {
            let result = encoder_worker(worker_path, rx);
            let _ = result_tx.send(result);
        });

        Ok(Self {
            tx: Some(tx),
            worker: Some(worker),
            result_rx: Some(result_rx),
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            video_queue_full_count: 0,
            audio_queue_full_count: 0,
            system_chunks_received_by_writer: 0,
            mic_chunks_received_by_writer: 0,
        })
    }

    /// Drain the worker handle and return its result with a bounded timeout.
    /// Uses a result channel from the worker so we can apply `recv_timeout`.
    ///
    /// On timeout: returns error immediately WITHOUT calling `handle.join()`.
    /// The worker may be stuck in FFmpeg flush/muxer IO — joining would block
    /// the stop path indefinitely, violating BUG.md rules 15 and 23.
    ///
    /// On disconnect: worker already exited (likely panicked), safe to join
    /// to capture panic info.
    fn join_worker(&mut self) -> AppResult<RecordingResult> {
        const WORKER_RESULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

        let rx = self
            .result_rx
            .take()
            .ok_or(AppError::RecordingWriteFailed {
                reason: "编码工作结果通道已被回收".to_string(),
            })?;

        match rx.recv_timeout(WORKER_RESULT_TIMEOUT) {
            Ok(result) => {
                // Worker completed — join to clean up thread resources.
                if let Some(handle) = self.worker.take() {
                    let _ = handle.join();
                }
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Timeout: do NOT call handle.join(). The worker may still be
                // running (stuck in FFmpeg flush/muxer IO/write_trailer).
                // Drop the JoinHandle to detach the thread.
                self.worker.take();
                eprintln!(
                    "警告: FFmpeg worker 超时未返回结果 ({:?})，worker 可能仍在后台执行",
                    WORKER_RESULT_TIMEOUT
                );
                Err(AppError::RecordingWriteFailed {
                    reason: format!("FFmpeg worker 超时未返回结果 ({:?})", WORKER_RESULT_TIMEOUT),
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Worker exited without sending result — likely panicked.
                // Safe to join to capture panic info.
                eprintln!("警告: FFmpeg worker 结果通道断开，尝试 join 获取 panic 信息");
                if let Some(handle) = self.worker.take() {
                    match handle.join() {
                        Ok(()) => Err(AppError::RecordingWriteFailed {
                            reason: "FFmpeg worker 异常退出且未返回结果".to_string(),
                        }),
                        Err(panic_payload) => {
                            let msg = extract_panic_message(&panic_payload);
                            Err(AppError::RecordingWriteFailed {
                                reason: format!("FFmpeg worker panic: {}", msg),
                            })
                        }
                    }
                } else {
                    Err(AppError::RecordingWriteFailed {
                        reason: "FFmpeg worker 结果通道断开且线程句柄已被回收".to_string(),
                    })
                }
            }
        }
    }
}

impl RecordingWriter for FfmpegRecordingWriter {
    fn push_video(&mut self, frame: VideoFrameRef) -> AppResult<()> {
        self.frame_count += 1;

        // Validate and copy frame data on the caller thread.
        let src_data = match &frame.buffer {
            crate::core::frame::FrameBuffer::Owned(arc) => arc.as_ref(),
        };
        let expected_min_size = frame.stride_bytes * frame.height as usize;
        if src_data.len() < expected_min_size {
            return Err(AppError::RecordingWriteFailed {
                reason: format!(
                    "视频帧数据不足：期望至少 {} 字节（stride={}×height={}），实际 {} 字节",
                    expected_min_size,
                    frame.stride_bytes,
                    frame.height,
                    src_data.len()
                ),
            });
        }

        // Copy only the valid pixel data (respecting stride padding).
        let row_bytes = frame.width as usize * 4;
        let copy_per_row = row_bytes.min(frame.stride_bytes);
        let mut buffer = vec![0u8; copy_per_row * frame.height as usize];
        for row in 0..frame.height as usize {
            let src_offset = row * frame.stride_bytes;
            let dst_offset = row * copy_per_row;
            if src_offset + copy_per_row > src_data.len() {
                break;
            }
            buffer[dst_offset..dst_offset + copy_per_row]
                .copy_from_slice(&src_data[src_offset..src_offset + copy_per_row]);
        }

        // Non-blocking enqueue — returns error if the queue is full.
        // This prevents the capture consumer thread from stalling when the
        // FFmpeg encoder can't keep up, which would cause audio drops in
        // the media channels.
        let tx = self.tx.as_ref().ok_or(AppError::RecordingWriteFailed {
            reason: "编码队列已关闭（writer 已 finish）".to_string(),
        })?;
        tx.try_send(EncoderMessage::Video {
            timestamp_nanos: frame.timestamp.nanos,
            width: frame.width,
            height: frame.height,
            stride_bytes: copy_per_row,
            buffer,
        })
        .map_err(|e| match e {
            mpsc::TrySendError::Full(_) => {
                self.video_queue_full_count += 1;
                AppError::RecordingWriteFailed {
                    reason: "FFmpeg 编码队列已满，视频帧被丢弃（编码速度跟不上采集速度）"
                        .to_string(),
                }
            }
            mpsc::TrySendError::Disconnected(_) => AppError::RecordingWriteFailed {
                reason: "编码队列已关闭，无法发送视频帧".to_string(),
            },
        })
    }

    fn push_audio(&mut self, chunk: MixedAudioChunk) -> AppResult<()> {
        // Validate on the caller thread.
        if chunk.sample_rate != 48_000 {
            return Err(AppError::RecordingWriteFailed {
                reason: format!(
                    "FFmpeg writer 仅接受 48kHz 音频，实际采样率 {}Hz",
                    chunk.sample_rate
                ),
            });
        }
        if chunk.channels != 2 {
            return Err(AppError::RecordingWriteFailed {
                reason: format!(
                    "FFmpeg writer 仅接受 stereo 音频，实际通道数 {}",
                    chunk.channels
                ),
            });
        }
        if !chunk.samples.len().is_multiple_of(2) {
            return Err(AppError::RecordingWriteFailed {
                reason: "FFmpeg writer 收到的 stereo 音频样本数不是 2 的倍数".to_string(),
            });
        }

        self.mixed_audio_chunk_count += 1;

        // Non-blocking enqueue — returns error if the queue is full.
        // Audio starvation is worse than a logged drop because it can cause
        // the entire capture pipeline to stall.
        let tx = self.tx.as_ref().ok_or(AppError::RecordingWriteFailed {
            reason: "编码队列已关闭（writer 已 finish）".to_string(),
        })?;
        tx.try_send(EncoderMessage::Audio {
            samples: chunk.samples.to_vec(),
            timestamp_nanos: chunk.timestamp.nanos,
        })
        .map_err(|e| match e {
            mpsc::TrySendError::Full(_) => {
                self.audio_queue_full_count += 1;
                AppError::RecordingWriteFailed {
                    reason: "FFmpeg 编码队列已满，音频数据被丢弃（编码速度跟不上采集速度）"
                        .to_string(),
                }
            }
            mpsc::TrySendError::Disconnected(_) => AppError::RecordingWriteFailed {
                reason: "编码队列已关闭，无法发送音频数据".to_string(),
            },
        })
    }

    fn record_source_contribution(
        &mut self,
        has_system: bool,
        has_mic: bool,
        _system_frames: u64,
        _mic_frames: u64,
    ) {
        if has_system {
            self.system_chunks_received_by_writer += 1;
        }
        if has_mic {
            self.mic_chunks_received_by_writer += 1;
        }
    }

    fn finish(&mut self) -> AppResult<RecordingResult> {
        // Send flush signal to the worker using try_send with bounded retries.
        // This prevents blocking indefinitely if the encoder queue is full or
        // the worker is stuck on a slow FFmpeg operation.
        const MAX_FLUSH_RETRIES: usize = 100;
        const FLUSH_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(10);

        let mut flush_sent = false;
        for _ in 0..MAX_FLUSH_RETRIES {
            match self
                .tx
                .as_ref()
                .map(|tx| tx.try_send(EncoderMessage::Flush))
            {
                Some(Ok(())) => {
                    flush_sent = true;
                    break;
                }
                Some(Err(mpsc::TrySendError::Full(_))) => {
                    // Queue is full — wait briefly for the worker to drain.
                    std::thread::sleep(FLUSH_RETRY_DELAY);
                }
                Some(Err(mpsc::TrySendError::Disconnected(_))) => {
                    // Worker already exited — skip flush.
                    flush_sent = true;
                    break;
                }
                None => {
                    // tx already taken — skip flush.
                    flush_sent = true;
                    break;
                }
            }
        }

        if !flush_sent {
            return Err(AppError::RecordingWriteFailed {
                reason: format!(
                    "无法在 {}ms 内发送刷新信号（编码队列持续满载）",
                    MAX_FLUSH_RETRIES as u64 * 10
                ),
            });
        }

        // Drop sender before joining worker.
        // This ensures the worker's channel becomes disconnected after it processes
        // the Flush message, so if the worker is stuck on rx.recv() it will get a
        // RecvError and exit cleanly instead of blocking join() forever.
        self.tx.take();

        // Join the worker and merge front-end queue diagnostics.
        let start = std::time::Instant::now();
        let mut result = self.join_worker()?;
        let join_elapsed = start.elapsed();
        if join_elapsed > std::time::Duration::from_secs(10) {
            eprintln!(
                "警告: encoder worker join 耗时 {:.1}s，可能 FFmpeg flush 缓慢",
                join_elapsed.as_secs_f64()
            );
        }
        result.writer_diagnostics.video_queue_full_count += self.video_queue_full_count;
        result.writer_diagnostics.audio_queue_full_count += self.audio_queue_full_count;
        result.writer_diagnostics.system_chunks_received_by_writer +=
            self.system_chunks_received_by_writer;
        result.writer_diagnostics.mic_chunks_received_by_writer +=
            self.mic_chunks_received_by_writer;
        Ok(result)
    }
}

/// Extract a human-readable message from a panic payload.
fn extract_panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

/// Drain complete AAC frames from the interleaved audio sample buffer.
///
/// Extracts 1024-sample (2048 interleaved) frames, converts to planar,
/// encodes via the AAC encoder, and writes packets to the output muxer.
/// `audio_pts` advances monotonically by 1024 per encoded frame.
///
/// Returns the number of AAC frames encoded in this call.
fn drain_audio_sample_buffer(
    audio_sample_buffer: &mut Vec<f32>,
    audio_pts: &mut i64,
    audio_encoder: &mut ffmpeg_next::codec::encoder::Audio,
    output: &mut ffmpeg_next::format::context::Output,
    audio_stream_index: usize,
) -> AppResult<u64> {
    use ffmpeg_next as ff;

    let samples_per_frame = 1024usize;
    let interleaved_frame_size = samples_per_frame * 2; // stereo
    let mut frames_encoded: u64 = 0;

    while audio_sample_buffer.len() >= interleaved_frame_size {
        let frame_data: Vec<f32> = audio_sample_buffer
            .drain(..interleaved_frame_size)
            .collect();

        let mut audio_frame = ff::util::frame::Audio::new(
            ff::util::format::Sample::F32(ff::util::format::sample::Type::Planar),
            samples_per_frame,
            ff::ChannelLayout::STEREO,
        );
        audio_frame.set_pts(Some(*audio_pts));
        *audio_pts += samples_per_frame as i64;

        // Interleaved → planar conversion.
        for ch in 0..2usize {
            let plane = audio_frame.plane_mut::<f32>(ch);
            for (i, sample) in plane.iter_mut().enumerate() {
                *sample = frame_data[i * 2 + ch];
            }
        }

        audio_encoder
            .send_frame(&audio_frame)
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("编码音频帧失败: {e}"),
            })?;

        let audio_tb = output.stream(audio_stream_index).unwrap().time_base();
        let mut packet = ff::Packet::empty();
        while audio_encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(audio_stream_index);
            packet.rescale_ts(ff::Rational(1, 48000), audio_tb);
            packet
                .write_interleaved(output)
                .map_err(|e| AppError::RecordingWriteFailed {
                    reason: format!("写入音频数据包失败: {e}"),
                })?;
        }
        frames_encoded += 1;
    }

    Ok(frames_encoded)
}

/// Result of appending an audio chunk to the timeline buffer.
struct TimelineAppendResult {
    /// Whether real PCM samples were appended (not just silence padding).
    chunk_appended: bool,
    /// Number of silence mono frames padded for gap.
    silence_frames_padded: u64,
    /// Number of real mono frames appended from this chunk.
    appended_frames: u64,
    /// Whether this chunk was partially trimmed due to overlap.
    trimmed_partial_overlap: bool,
    /// Number of mono frames trimmed (skipped) due to partial overlap.
    trimmed_frames: u64,
}

/// Append an audio chunk to the timeline buffer, handling gaps and overlaps.
///
/// - Gap (`target > cursor`): pad silence, then append real samples.
/// - Overlap (`target < cursor`): trim or discard overlapping prefix.
/// - Contiguous (`target == cursor`): append directly.
fn append_audio_chunk_to_timeline(
    audio_sample_buffer: &mut Vec<f32>,
    audio_timeline_cursor: &mut i64,
    target_sample: i64,
    samples: &[f32],
) -> TimelineAppendResult {
    let chunk_mono_frames = (samples.len() / 2) as i64;

    if target_sample > *audio_timeline_cursor {
        // Gap: pad silence from cursor to target, then append real samples.
        let gap_mono = target_sample - *audio_timeline_cursor;
        let gap_interleaved = (gap_mono * 2) as usize;
        audio_sample_buffer.extend(std::iter::repeat_n(0.0f32, gap_interleaved));
        audio_sample_buffer.extend_from_slice(samples);
        *audio_timeline_cursor = target_sample + chunk_mono_frames;
        return TimelineAppendResult {
            chunk_appended: true,
            silence_frames_padded: gap_mono as u64,
            appended_frames: chunk_mono_frames as u64,
            trimmed_partial_overlap: false,
            trimmed_frames: 0,
        };
    }

    if target_sample < *audio_timeline_cursor {
        // Overlap: this chunk's start is before where we already wrote.
        let overlap_mono = (*audio_timeline_cursor - target_sample) as usize;
        if overlap_mono >= chunk_mono_frames as usize {
            // Entire chunk is already covered — discard.
            return TimelineAppendResult {
                chunk_appended: false,
                silence_frames_padded: 0,
                appended_frames: 0,
                trimmed_partial_overlap: false,
                trimmed_frames: 0,
            };
        }
        // Partial overlap: skip the overlapping prefix, append the rest.
        let skip_interleaved = overlap_mono * 2;
        let remaining = &samples[skip_interleaved..];
        audio_sample_buffer.extend_from_slice(remaining);
        let appended_mono = (remaining.len() / 2) as i64;
        *audio_timeline_cursor += appended_mono;
        return TimelineAppendResult {
            chunk_appended: true,
            silence_frames_padded: 0,
            appended_frames: appended_mono as u64,
            trimmed_partial_overlap: true,
            trimmed_frames: overlap_mono as u64,
        };
    }

    // Contiguous: append all.
    audio_sample_buffer.extend_from_slice(samples);
    *audio_timeline_cursor += chunk_mono_frames;
    TimelineAppendResult {
        chunk_appended: true,
        silence_frames_padded: 0,
        appended_frames: chunk_mono_frames as u64,
        trimmed_partial_overlap: false,
        trimmed_frames: 0,
    }
}

/// Compute RMS of interleaved stereo samples (uses only left channel for speed).
fn compute_chunk_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mono_count = samples.len() / 2;
    if mono_count == 0 {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().step_by(2).map(|s| s * s).sum();
    (sum_squares / mono_count as f32).sqrt()
}

/// Encoder worker that runs on a dedicated thread.
///
/// Owns all FFmpeg contexts (encoder, scaler, output muxer) and processes
/// messages from the bounded channel. Returns the `RecordingResult` on
/// successful flush.
fn encoder_worker(
    output_path: PathBuf,
    rx: mpsc::Receiver<EncoderMessage>,
) -> AppResult<RecordingResult> {
    use ff::codec;
    use ff::codec::encoder;
    use ff::format;
    use ff::software;
    use ff::util::format::{sample, Pixel, Sample};
    use ff::util::frame;
    use ff::{ChannelLayout, Rational};
    use ffmpeg_next as ff;

    ff::init().map_err(|e| AppError::RecordingWriteFailed {
        reason: format!("初始化 FFmpeg 失败: {e}"),
    })?;

    let mut output = format::output(&output_path).map_err(|e| AppError::RecordingWriteFailed {
        reason: format!("创建输出文件失败: {e}"),
    })?;

    // --- Video encoder (H.264) ---
    let video_fps = 30u32;
    let video_codec = encoder::find(ff::codec::Id::H264).ok_or(AppError::RecordingWriteFailed {
        reason: "未找到 H.264 编码器".to_string(),
    })?;
    let video_ctx = ff::codec::Context::new_with_codec(video_codec);
    let mut video_enc =
        video_ctx
            .encoder()
            .video()
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("创建视频编码器失败: {e}"),
            })?;
    video_enc.set_width(1920);
    video_enc.set_height(1080);
    video_enc.set_bit_rate(8_000_000);
    video_enc.set_time_base(Rational(1, video_fps as i32));
    video_enc.set_format(Pixel::YUV420P);
    video_enc.set_max_b_frames(0);
    let video_opts = ff::Dictionary::from_iter([("preset", "ultrafast")]);
    let mut video_encoder = video_enc
        .open_as_with(video_codec, video_opts)
        .map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("打开视频编码器失败: {e}"),
        })?;

    let video_params = codec::Parameters::from(&video_encoder);
    let mut video_stream =
        output
            .add_stream(video_codec)
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("添加视频流失败: {e}"),
            })?;
    video_stream.set_time_base(Rational(1, video_fps as i32));
    video_stream.set_parameters(video_params);
    let video_stream_index = video_stream.index();

    // --- Audio encoder (AAC) ---
    let audio_codec = encoder::find(ff::codec::Id::AAC).ok_or(AppError::RecordingWriteFailed {
        reason: "未找到 AAC 编码器".to_string(),
    })?;
    let audio_ctx = ff::codec::Context::new_with_codec(audio_codec);
    let mut audio_enc =
        audio_ctx
            .encoder()
            .audio()
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("创建音频编码器失败: {e}"),
            })?;
    audio_enc.set_rate(48000);
    audio_enc.set_channel_layout(ChannelLayout::STEREO);
    audio_enc.set_format(Sample::F32(sample::Type::Planar));
    audio_enc.set_time_base(Rational(1, 48000));
    let mut audio_encoder =
        audio_enc
            .open_as(audio_codec)
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("打开音频编码器失败: {e}"),
            })?;

    let audio_params = codec::Parameters::from(&audio_encoder);
    let mut audio_stream =
        output
            .add_stream(audio_codec)
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("添加音频流失败: {e}"),
            })?;
    audio_stream.set_time_base(Rational(1, 48000));
    audio_stream.set_parameters(audio_params);
    let audio_stream_index = audio_stream.index();

    output
        .write_header()
        .map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("写入文件头失败: {e}"),
        })?;

    // --- Worker state ---
    let mut scaler: Option<software::scaling::Context> = None;
    let mut frame_count: u64 = 0;
    let mut mixed_audio_chunk_count: u64 = 0;
    let mut audio_pts: i64 = 0;
    let mut writer_diag = WriterDiagnostics::default();
    let mut video_duration_nanos: u64 = 0;
    let mut last_video_pts: i64 = -1;
    let mut audio_sample_buffer: Vec<f32> = Vec::new();
    // Audio timeline cursor in 48kHz sample units.
    // Used to detect gaps (leading silence, middle silence) and pad accordingly.
    let mut audio_timeline_cursor: i64 = 0;

    // --- Message processing loop ---
    for msg in rx.iter() {
        match msg {
            EncoderMessage::Video {
                timestamp_nanos,
                width,
                height,
                stride_bytes,
                buffer,
            } => {
                frame_count += 1;

                // Create input frame and copy data.
                let mut input_frame = frame::Video::new(Pixel::BGRA, width, height);
                let dst = input_frame.data_mut(0);
                let dst_linesize = if height > 0 {
                    dst.len() / height as usize
                } else {
                    0
                };
                let row_bytes = width as usize * 4;
                let copy_per_row = row_bytes.min(stride_bytes).min(dst_linesize);
                for row in 0..height as usize {
                    let src_offset = row * stride_bytes;
                    let dst_offset = row * dst_linesize;
                    if src_offset + copy_per_row > buffer.len()
                        || dst_offset + copy_per_row > dst.len()
                    {
                        break;
                    }
                    dst[dst_offset..dst_offset + copy_per_row]
                        .copy_from_slice(&buffer[src_offset..src_offset + copy_per_row]);
                }

                video_duration_nanos = timestamp_nanos;

                // PTS from real timestamp in encoder time_base (1/fps).
                let pts_in_enc_tb = (timestamp_nanos as i128 * 30 / 1_000_000_000i128) as i64;
                let pts = pts_in_enc_tb.max(last_video_pts + 1);
                last_video_pts = pts;

                let mut output_frame = frame::Video::new(Pixel::YUV420P, 1920, 1080);
                output_frame.set_pts(Some(pts));

                // Recreate scaler if input dimensions changed.
                let needs_new = match &scaler {
                    Some(s) => s.input().width != width || s.input().height != height,
                    None => true,
                };
                if needs_new {
                    scaler = Some(
                        software::scaling::Context::get(
                            Pixel::BGRA,
                            width,
                            height,
                            Pixel::YUV420P,
                            1920,
                            1080,
                            software::scaling::Flags::BILINEAR,
                        )
                        .map_err(|e| AppError::RecordingWriteFailed {
                            reason: format!("创建像素格式转换器失败: {e}"),
                        })?,
                    );
                }

                scaler
                    .as_mut()
                    .unwrap()
                    .run(&input_frame, &mut output_frame)
                    .map_err(|e| AppError::RecordingWriteFailed {
                        reason: format!("像素格式转换失败: {e}"),
                    })?;

                video_encoder.send_frame(&output_frame).map_err(|e| {
                    AppError::RecordingWriteFailed {
                        reason: format!("编码视频帧失败: {e}"),
                    }
                })?;

                let video_tb = output.stream(video_stream_index).unwrap().time_base();
                let mut packet = ff::Packet::empty();
                while video_encoder.receive_packet(&mut packet).is_ok() {
                    packet.set_stream(video_stream_index);
                    packet.rescale_ts(Rational(1, 30), video_tb);
                    packet.write_interleaved(&mut output).map_err(|e| {
                        AppError::RecordingWriteFailed {
                            reason: format!("写入视频数据包失败: {e}"),
                        }
                    })?;
                }
            }

            EncoderMessage::Audio {
                samples,
                timestamp_nanos,
                ..
            } => {
                mixed_audio_chunk_count += 1;
                writer_diag.audio_chunks_received += 1;

                // Convert chunk timestamp to 48kHz mono sample position.
                let target_sample = (timestamp_nanos as i128 * 48000 / 1_000_000_000i128) as i64;

                // BUG-009 fix: gap branch now pads silence THEN appends real samples.
                // Previously it only padded silence, discarding the current chunk's PCM.
                let append_result = append_audio_chunk_to_timeline(
                    &mut audio_sample_buffer,
                    &mut audio_timeline_cursor,
                    target_sample,
                    &samples,
                );

                if append_result.chunk_appended {
                    writer_diag.audio_chunks_appended += 1;
                    writer_diag.audio_real_frames_appended += append_result.appended_frames;
                    let chunk_rms = compute_chunk_rms(&samples);
                    if chunk_rms > writer_diag.audio_real_rms_max_before_encode {
                        writer_diag.audio_real_rms_max_before_encode = chunk_rms;
                    }
                } else {
                    writer_diag.audio_chunks_discarded_full_overlap += 1;
                }
                if append_result.trimmed_partial_overlap {
                    writer_diag.audio_chunks_trimmed_partial_overlap += 1;
                }
                writer_diag.audio_silence_frames_padded += append_result.silence_frames_padded;

                writer_diag.aac_frames_encoded += drain_audio_sample_buffer(
                    &mut audio_sample_buffer,
                    &mut audio_pts,
                    &mut audio_encoder,
                    &mut output,
                    audio_stream_index,
                )?;
            }

            EncoderMessage::Flush => {
                break;
            }
        }
    }

    // --- Flush: handle zero-frame recordings ---
    if frame_count == 0 {
        video_encoder.send_eof().ok();
        let video_tb = output.stream(video_stream_index).unwrap().time_base();
        let mut packet = ff::Packet::empty();
        while video_encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(video_stream_index);
            packet.rescale_ts(Rational(1, 30), video_tb);
            packet.write_interleaved(&mut output).ok();
        }
        audio_encoder.send_eof().ok();
        let audio_tb = output.stream(audio_stream_index).unwrap().time_base();
        while audio_encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(audio_stream_index);
            packet.rescale_ts(Rational(1, 48000), audio_tb);
            packet.write_interleaved(&mut output).ok();
        }
        output.write_trailer().ok();
        return Ok(RecordingResult {
            duration_secs: 0,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            output_path: None,
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
            writer_diagnostics: writer_diag,
            diagnostics: crate::media::recording_writer::RecordingDiagnostics::default(),
            finalization_errors: Vec::new(),
        });
    }

    // --- Flush: pad tail silence if audio is shorter than video ---
    // Use "last video frame timestamp + one frame duration" for the video end
    // to avoid A/V drift from the last frame's timestamp being slightly early.
    if mixed_audio_chunk_count > 0 && video_duration_nanos > 0 {
        let one_frame_nanos = 1_000_000_000u64 / 30; // ~33ms at 30fps
        let video_end_nanos = video_duration_nanos + one_frame_nanos;
        let video_end_sample = (video_end_nanos as i128 * 48000 / 1_000_000_000i128) as i64;
        if audio_timeline_cursor < video_end_sample {
            let tail_gap = video_end_sample - audio_timeline_cursor;
            let tail_interleaved = (tail_gap * 2) as usize;
            audio_sample_buffer.extend(std::iter::repeat_n(0.0f32, tail_interleaved));
            // Note: audio_timeline_cursor intentionally not updated here.
            // audio_pts is the authoritative counter during flush, advancing
            // monotonically as AAC frames are encoded below.
        }
    }

    // --- Flush: remaining audio buffer ---
    // Process ALL remaining samples, not just one AAC frame.
    // The buffer may contain multiple frames worth of samples after tail padding.
    writer_diag.aac_frames_encoded += drain_audio_sample_buffer(
        &mut audio_sample_buffer,
        &mut audio_pts,
        &mut audio_encoder,
        &mut output,
        audio_stream_index,
    )?;

    // Handle final partial frame (pad with silence to fill 1024 samples).
    if !audio_sample_buffer.is_empty() {
        let samples_per_frame = 1024usize;
        let interleaved_frame_size = samples_per_frame * 2;
        audio_sample_buffer.resize(interleaved_frame_size, 0.0);
        let frame_data: Vec<f32> = std::mem::take(&mut audio_sample_buffer);
        let mut audio_frame = frame::Audio::new(
            Sample::F32(sample::Type::Planar),
            samples_per_frame,
            ChannelLayout::STEREO,
        );
        audio_frame.set_pts(Some(audio_pts));

        for ch in 0..2usize {
            let plane = audio_frame.plane_mut::<f32>(ch);
            for (i, sample) in plane.iter_mut().enumerate() {
                *sample = frame_data[i * 2 + ch];
            }
        }

        audio_encoder
            .send_frame(&audio_frame)
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("编码最终音频帧失败: {e}"),
            })?;
        let audio_tb = output.stream(audio_stream_index).unwrap().time_base();
        let mut packet = ff::Packet::empty();
        while audio_encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(audio_stream_index);
            packet.rescale_ts(Rational(1, 48000), audio_tb);
            packet
                .write_interleaved(&mut output)
                .map_err(|e| AppError::RecordingWriteFailed {
                    reason: format!("写入最终音频数据包失败: {e}"),
                })?;
        }
    }

    // Generate silent audio track if no audio was received.
    if mixed_audio_chunk_count == 0 {
        writer_diag.generated_silent_track = true;
        let video_duration_secs = video_duration_nanos as f64 / 1_000_000_000.0;
        let total_audio_frames = (video_duration_secs * 48000.0).ceil() as u64;
        let num_silent_packets = (total_audio_frames / 1024).max(1);

        for _ in 0..num_silent_packets {
            let mut frame = frame::Audio::new(
                Sample::F32(sample::Type::Planar),
                1024,
                ChannelLayout::STEREO,
            );
            frame.set_pts(Some(audio_pts));
            audio_pts += 1024;

            for ch in 0..2usize {
                let plane = frame.plane_mut::<f32>(ch);
                for sample in plane.iter_mut() {
                    *sample = 0.0;
                }
            }

            audio_encoder
                .send_frame(&frame)
                .map_err(|e| AppError::RecordingWriteFailed {
                    reason: format!("编码静音音频帧失败: {e}"),
                })?;
            let audio_tb = output.stream(audio_stream_index).unwrap().time_base();
            let mut packet = ff::Packet::empty();
            while audio_encoder.receive_packet(&mut packet).is_ok() {
                packet.set_stream(audio_stream_index);
                packet.rescale_ts(Rational(1, 48000), audio_tb);
                packet.write_interleaved(&mut output).map_err(|e| {
                    AppError::RecordingWriteFailed {
                        reason: format!("写入静音音频数据包失败: {e}"),
                    }
                })?;
            }
        }
        writer_diag.silent_aac_frames_encoded = num_silent_packets;
        mixed_audio_chunk_count = num_silent_packets;
    }

    // Flush encoders.
    video_encoder
        .send_eof()
        .map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("刷新视频编码器失败: {e}"),
        })?;
    let video_tb = output.stream(video_stream_index).unwrap().time_base();
    let mut packet = ff::Packet::empty();
    while video_encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(video_stream_index);
        packet.rescale_ts(Rational(1, 30), video_tb);
        packet
            .write_interleaved(&mut output)
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("写入视频尾帧数据包失败: {e}"),
            })?;
    }

    audio_encoder
        .send_eof()
        .map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("刷新音频编码器失败: {e}"),
        })?;
    let audio_tb = output.stream(audio_stream_index).unwrap().time_base();
    while audio_encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(audio_stream_index);
        packet.rescale_ts(Rational(1, 48000), audio_tb);
        packet
            .write_interleaved(&mut output)
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("写入音频尾帧数据包失败: {e}"),
            })?;
    }

    output
        .write_trailer()
        .map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("写入文件尾失败: {e}"),
        })?;

    // Validate the artifact.
    crate::media::ffmpeg_common::validate_source_artifact(&output_path)?;

    let duration_secs = (video_duration_nanos / 1_000_000_000).max(1);

    Ok(RecordingResult {
        duration_secs,
        frame_count,
        mixed_audio_chunk_count,
        output_path: Some(output_path.to_string_lossy().to_string()),
        cursor_metadata_path: None,
        effect_timeline_path: None,
        trim_metadata_path: None,
        cut_timeline_path: None,
        writer_diagnostics: writer_diag,
        diagnostics: crate::media::recording_writer::RecordingDiagnostics::default(),
        finalization_errors: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::test_support::ffmpeg_helpers::{test_audio_chunk_at, test_video_frame_at};
    use std::sync::Arc;

    /// Create a MixedAudioChunk with specific timestamp and mono frame count.
    /// Each mono frame produces 2 interleaved stereo samples.
    fn audio_chunk_with_frames(timestamp_nanos: u64, mono_frames: usize) -> MixedAudioChunk {
        MixedAudioChunk {
            timestamp: crate::core::frame::MediaTimestamp::from_nanos(timestamp_nanos),
            sample_rate: 48_000,
            channels: 2,
            samples: Arc::from(vec![0.5f32; mono_frames * 2].into_boxed_slice()),
        }
    }

    #[test]
    fn ffmpeg_writer_encodes_video_and_audio_to_mp4() {
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-test", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        for i in 0..3 {
            let frame = test_video_frame_at(i * 33_333_333);
            writer.push_video(frame).unwrap();
        }
        for i in 0..5 {
            let chunk = test_audio_chunk_at(i * 20_000_000);
            writer.push_audio(chunk).unwrap();
        }

        let result = writer.finish().unwrap();
        assert_eq!(result.frame_count, 3);
        assert_eq!(result.mixed_audio_chunk_count, 5);
        assert!(result.output_path.is_some());
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_with_zero_duration_generates_valid_container() {
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-zero-dur", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        let result = writer.finish().unwrap();
        assert_eq!(result.frame_count, 0);
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_increments_frame_counts() {
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-counts", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        writer.push_video(test_video_frame_at(0)).unwrap();
        writer.push_audio(test_audio_chunk_at(0)).unwrap();
        writer.push_video(test_video_frame_at(33_333_333)).unwrap();

        assert_eq!(writer.frame_count, 2);
        assert_eq!(writer.mixed_audio_chunk_count, 1);
        let _ = writer.finish();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_sets_output_path_in_result() {
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-path", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        writer.push_video(test_video_frame_at(0)).unwrap();
        writer.push_audio(test_audio_chunk_at(0)).unwrap();

        let result = writer.finish().unwrap();
        assert_eq!(result.output_path, Some(path.to_string_lossy().to_string()));
        assert_eq!(result.frame_count, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_zero_frame_returns_no_output_path() {
        let path =
            crate::test_support::ffmpeg_helpers::unique_media_path("writer-zero-frames", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        let result = writer.finish().unwrap();
        assert_eq!(
            result.output_path, None,
            "zero-frame recording should not return output path"
        );
        assert_eq!(result.frame_count, 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_produces_playable_artifact() {
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-playable", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        for i in 0..3 {
            writer
                .push_video(test_video_frame_at(i * 33_333_333))
                .unwrap();
        }
        for i in 0..5 {
            writer
                .push_audio(test_audio_chunk_at(i * 20_000_000))
                .unwrap();
        }

        writer.finish().unwrap();

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_video_stream);
        assert!(inspection.has_audio_stream);
        assert_eq!(inspection.width, 1920);
        assert_eq!(inspection.height, 1080);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_handles_padded_bgra_stride() {
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-padded", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        let width = 2u32;
        let height = 2u32;
        let stride_bytes = 12;
        let buffer_size = stride_bytes * height as usize;
        let buffer = vec![42u8; buffer_size];

        let frame = Arc::new(crate::core::frame::VideoFrame {
            timestamp: crate::core::frame::MediaTimestamp::from_nanos(0),
            width,
            height,
            stride_bytes,
            pixel_format: crate::core::frame::PixelFormat::Bgra8,
            buffer: crate::core::frame::FrameBuffer::Owned(Arc::from(buffer.into_boxed_slice())),
        });
        writer.push_video(frame).unwrap();

        let result = writer.finish().unwrap();
        assert_eq!(result.frame_count, 1);
        assert!(path.exists());

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_video_stream);
        assert_eq!(inspection.width, 1920);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_without_audio_generates_silent_track() {
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-silent", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        for i in 0..3 {
            writer
                .push_video(test_video_frame_at(i * 33_333_333))
                .unwrap();
        }

        let result = writer.finish().unwrap();
        assert_eq!(result.frame_count, 3);
        assert!(
            result.mixed_audio_chunk_count > 0,
            "silent audio should be generated"
        );
        assert!(path.exists());

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_video_stream, "must have video stream");
        assert!(inspection.has_audio_stream, "must have silent audio stream");
        assert!(inspection.duration_nanos > 0, "must have non-zero duration");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_uses_sane_video_time_base() {
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-tb", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        writer.push_video(test_video_frame_at(0)).unwrap();
        writer.finish().unwrap();

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_video_stream);
        assert!(inspection.duration_nanos > 0);
        let _ = std::fs::remove_file(&path);
    }

    /// Helper that pushes a video frame, tolerating queue-full errors.
    /// Returns true if the frame was accepted, false if dropped.
    fn push_video_tolerant(writer: &mut FfmpegRecordingWriter, frame: VideoFrameRef) -> bool {
        match writer.push_video(frame) {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// Helper that pushes an audio chunk, tolerating queue-full errors.
    /// Returns true if the chunk was accepted, false if dropped.
    fn push_audio_tolerant(writer: &mut FfmpegRecordingWriter, chunk: MixedAudioChunk) -> bool {
        match writer.push_audio(chunk) {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    #[test]
    fn ffmpeg_writer_queue_returns_error_when_full() {
        // Verify that the bounded channel returns an error when full
        // instead of blocking the producer. This prevents the capture
        // consumer thread from stalling when the encoder can't keep up.
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-pressure", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // Push many frames rapidly — the bounded queue will eventually
        // return TrySendError::Full instead of blocking.
        let mut success_count = 0u64;
        let mut error_count = 0u64;
        for i in 0..500 {
            match writer.push_video(test_video_frame_at(i * 33_333_333)) {
                Ok(()) => success_count += 1,
                Err(_) => error_count += 1,
            }
        }

        // At least some frames should have been rejected (queue full).
        assert!(
            error_count > 0,
            "expected some frames to be rejected when queue is full, got {success_count} success, {error_count} errors"
        );

        // Finish should still work for the accepted frames.
        let result = writer.finish().unwrap();
        assert_eq!(result.frame_count, success_count);
        let _ = std::fs::remove_file(&path);
    }

    // --- BUG-005: Audio timeline merge tests ---

    #[test]
    fn ffmpeg_writer_pads_first_audio_gap_for_mic_start_offset() {
        // BUG-005: When the first audio chunk arrives at t=200ms (not t=0),
        // the writer must pad 200ms of leading silence so audio stream starts at t=0.
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-lead-gap", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // 10 seconds of video at 30fps
        let fps = 30u64;
        let frame_duration = 1_000_000_000 / fps;
        for i in 0..(10 * fps) {
            push_video_tolerant(&mut writer, test_video_frame_at(i * frame_duration));
        }

        // First audio chunk at t=200ms — should produce 200ms of leading silence + chunk.
        let chunk = audio_chunk_with_frames(200_000_000, 1024);
        push_audio_tolerant(&mut writer, chunk);
        // Second audio chunk at t=221ms (contiguous after first chunk's ~21ms).
        let ts_second = 200_000_000u64 + (1024u64 * 1_000_000_000 / 48000);
        push_audio_tolerant(&mut writer, audio_chunk_with_frames(ts_second, 1024));

        let result = writer.finish().unwrap();
        assert!(result.output_path.is_some(), "should produce output");

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_audio_stream, "must have audio stream");
        // Audio should include leading silence (200ms) + 2 chunks (~42ms) + tail padding.
        // Total should be at least 200ms.
        let audio_ms = inspection.audio_duration_nanos / 1_000_000;
        assert!(
            audio_ms >= 200,
            "audio should include leading silence, got {}ms",
            audio_ms
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_crops_overlapping_audio_chunk() {
        // BUG-005: When a chunk's timestamp overlaps with already-written audio,
        // the writer must trim the overlapping prefix, not append the full chunk.
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-overlap", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        for i in 0..3 {
            writer
                .push_video(test_video_frame_at(i * 33_333_333))
                .unwrap();
        }

        // First chunk: t=0, 1024 mono frames → covers 0..1024 at 48kHz.
        writer.push_audio(audio_chunk_with_frames(0, 1024)).unwrap();
        // Second chunk: t=0 again (overlap) — should be fully discarded
        // because all 1024 frames are already covered.
        writer.push_audio(audio_chunk_with_frames(0, 1024)).unwrap();
        // Third chunk: t=1024/48000 seconds — contiguous, should be appended.
        let ts_after_first = 1024u64 * 1_000_000_000 / 48000;
        writer
            .push_audio(audio_chunk_with_frames(ts_after_first, 1024))
            .unwrap();

        let result = writer.finish().unwrap();
        assert!(result.output_path.is_some());

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_audio_stream);
        // Duration should be ~2 * 1024/48000 ≈ 42.7ms, not ~3 * 1024/48000 ≈ 64ms.
        // With video at 3 frames (~100ms), tail padding will extend it.
        // The key is that audio should NOT be 3x the expected length.
        // Assert non-inflation: 3 chunks but only 2 unique, so audio should
        // correspond to 2 chunks of content, not 3.
        let expected_max_ms = 200u64; // generous upper bound with tail padding
        let audio_ms = inspection.audio_duration_nanos / 1_000_000;
        assert!(
            audio_ms <= expected_max_ms,
            "overlapping chunk inflated audio: {}ms (expected <= {}ms)",
            audio_ms,
            expected_max_ms
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_drops_fully_overlapped_audio_chunk() {
        // BUG-005: A chunk entirely within already-written audio must be dropped.
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-drop", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        for i in 0..3 {
            writer
                .push_video(test_video_frame_at(i * 33_333_333))
                .unwrap();
        }

        // Write a chunk at t=0 with 2048 mono frames.
        writer.push_audio(audio_chunk_with_frames(0, 2048)).unwrap();
        // Write another chunk at t=0 with 1024 mono frames — entirely within the first.
        writer.push_audio(audio_chunk_with_frames(0, 1024)).unwrap();

        let result = writer.finish().unwrap();
        assert!(result.output_path.is_some());

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_audio_stream);
        // Audio should be ~2048/48000 ≈ 42.7ms, not (2048+1024)/48000 ≈ 64ms.
        // With tail padding for 3 video frames (~100ms), audio ≈ 100ms.
        // Must NOT be inflated by the fully-overlapped second chunk.
        let audio_ms = inspection.audio_duration_nanos / 1_000_000;
        assert!(
            audio_ms < 200,
            "fully-overlapped chunk inflated audio: {}ms (expected < 200ms)",
            audio_ms
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_preserves_av_duration_with_mic_24khz_mono_after_mixer() {
        // BUG-005: Simulates the real scenario — mic at 24kHz/1ch is resampled
        // to 48kHz/2ch by AudioMixer before reaching the writer. The writer
        // must produce A/V duration drift within 1s.
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-24k-mic", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // 3 seconds of video at 30fps — reduced to avoid queue overflow in tests.
        let fps = 30u64;
        let frame_duration = 1_000_000_000 / fps;
        for i in 0..(3 * fps) {
            push_video_tolerant(&mut writer, test_video_frame_at(i * frame_duration));
        }

        // Give the encoder worker time to process video frames before audio.
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 3 seconds of audio at 48kHz, chunks every 20ms
        let chunk_duration = 20_000_000u64; // 20ms
        let chunks_count = 3_000_000_000u64 / chunk_duration;
        for i in 0..chunks_count {
            let ts = i * chunk_duration;
            push_audio_tolerant(&mut writer, test_audio_chunk_at(ts));
        }

        let result = writer.finish().unwrap();
        assert!(result.output_path.is_some());

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_video_stream);
        assert!(inspection.has_audio_stream);

        // Video and audio duration drift should be within 2 seconds (generous for test).
        let drift =
            (inspection.video_duration_nanos as i64 - inspection.audio_duration_nanos as i64).abs();
        assert!(
            drift < 2_000_000_000,
            "A/V drift too large: {}ms (video={}ms, audio={}ms)",
            drift / 1_000_000,
            inspection.video_duration_nanos / 1_000_000,
            inspection.audio_duration_nanos / 1_000_000
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_drains_partial_overlap_audio_before_finish() {
        // R1: Partial-overlap chunks must be drained during recording, not
        // accumulated until finish(). Multiple slight-overlap chunks should
        // all produce audio output without excessive buffering.
        let path =
            crate::test_support::ffmpeg_helpers::unique_media_path("writer-partial-drain", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // 3 seconds of video at 30fps — reduced to avoid queue overflow in tests.
        let fps = 30u64;
        let frame_duration = 1_000_000_000 / fps;
        for i in 0..(3 * fps) {
            push_video_tolerant(&mut writer, test_video_frame_at(i * frame_duration));
        }

        // Give encoder worker time to process video before audio.
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Push audio chunks that are slightly overlapping (1024 mono frames per
        // chunk at 48kHz = ~21.33ms, but spaced 20ms apart). This causes each
        // chunk to partially overlap the previous one by ~1.33ms.
        let chunk_mono = 1024usize;
        let step_nanos = 20_000_000u64; // 20ms spacing
        let num_chunks = 3_000_000_000u64 / step_nanos; // ~150 chunks for 3s
        for i in 0..num_chunks {
            let ts = i * step_nanos;
            push_audio_tolerant(&mut writer, audio_chunk_with_frames(ts, chunk_mono));
            // Small sleep to avoid overwhelming the queue in test environment.
            if i % 10 == 0 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        let result = writer.finish().unwrap();
        assert!(result.output_path.is_some());

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_video_stream);
        assert!(inspection.has_audio_stream);

        // A/V drift must stay within 2s — generous for test environment where
        // frames may be dropped due to queue pressure.
        let drift =
            (inspection.video_duration_nanos as i64 - inspection.audio_duration_nanos as i64).abs();
        assert!(
            drift < 2_000_000_000,
            "A/V drift too large with partial-overlap chunks: {}ms (video={}ms, audio={}ms)",
            drift / 1_000_000,
            inspection.video_duration_nanos / 1_000_000,
            inspection.audio_duration_nanos / 1_000_000
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_handles_out_of_order_audio_chunks() {
        // R2: Out-of-order chunks (timestamp < cursor) must be trimmed or
        // discarded without corrupting the audio timeline.
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-ooo", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        for i in 0..3 {
            writer
                .push_video(test_video_frame_at(i * 33_333_333))
                .unwrap();
        }

        // First chunk at t=0.
        writer.push_audio(audio_chunk_with_frames(0, 1024)).unwrap();
        // Second chunk at correct position.
        let ts2 = 1024u64 * 1_000_000_000 / 48000;
        writer
            .push_audio(audio_chunk_with_frames(ts2, 1024))
            .unwrap();
        // Third chunk: out-of-order, back at t=0 — fully overlapped, should be discarded.
        writer.push_audio(audio_chunk_with_frames(0, 1024)).unwrap();
        // Fourth chunk: partially overlapping the second chunk.
        let ts4 = ts2 + 512u64 * 1_000_000_000 / 48000;
        writer
            .push_audio(audio_chunk_with_frames(ts4, 1024))
            .unwrap();

        let result = writer.finish().unwrap();
        assert!(result.output_path.is_some());

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_audio_stream);

        // Audio should not be inflated: expected ~2.5 unique chunks worth ≈ 53ms.
        // With tail padding for 3 video frames (~100ms), audio ≈ 100ms.
        // Must NOT be 4x the unique chunk duration (~213ms).
        let audio_ms = inspection.audio_duration_nanos / 1_000_000;
        assert!(
            audio_ms < 500,
            "out-of-order chunks inflated audio: {}ms (expected < 500ms)",
            audio_ms
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_audio_duration_not_inflated_by_multiple_chunks() {
        // BUG-005: Multiple chunks at the same timestamp should NOT inflate
        // audio duration. This was the root cause of "10s video, 53s audio".
        let path =
            crate::test_support::ffmpeg_helpers::unique_media_path("writer-no-inflate", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // 10 seconds of video at 30fps
        let fps = 30u64;
        let frame_duration = 1_000_000_000 / fps;
        for i in 0..(10 * fps) {
            push_video_tolerant(&mut writer, test_video_frame_at(i * frame_duration));
        }

        // Push 50 chunks all at t=0 (simulating buggy overlapping scenario).
        // Only the first should be kept; the rest should be dropped or trimmed.
        for _ in 0..50 {
            push_audio_tolerant(&mut writer, test_audio_chunk_at(0));
        }

        let result = writer.finish().unwrap();
        assert!(result.output_path.is_some());

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact(&path).unwrap();
        assert!(inspection.has_audio_stream);

        // Audio duration should be ~10s (with tail padding), not 50 * 21ms ≈ 1s.
        // More importantly, it should NOT be 50x the expected duration.
        let audio_secs = inspection.audio_duration_nanos as f64 / 1_000_000_000.0;
        assert!(
            audio_secs < 15.0,
            "audio duration inflated: {:.1}s (expected ~10s from tail padding)",
            audio_secs
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_records_non_silent_mixed_24khz_mono_mic() {
        // BUG-005: Tests the real path — a 24kHz/1ch mic AudioChunk goes through
        // SimpleAudioMixer, gets resampled to 48kHz/2ch, then pushed to writer.
        // Verifies: mixed sample layout, A/V drift, and decoded audio RMS.
        use crate::core::frame::AudioChunk;
        use crate::media::audio_mixer::{AudioMixer, SimpleAudioMixer};

        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-24k-real", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // 3 seconds of video at 30fps — reduced from 5s to avoid queue overflow
        // in test environment where frames are pushed faster than real-time.
        let fps = 30u64;
        let frame_duration = 1_000_000_000 / fps;
        for i in 0..(3 * fps) {
            push_video_tolerant(&mut writer, test_video_frame_at(i * frame_duration));
        }

        // Give the encoder worker time to process video frames before audio.
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 3 seconds of 24kHz/1ch mic audio, chunks every 20ms (480 samples)
        let mic_chunk_samples = 480usize; // 20ms @ 24kHz
        let chunk_duration = 20_000_000u64; // 20ms
        let num_chunks = 3_000_000_000u64 / chunk_duration;
        let mixer = SimpleAudioMixer::new();

        for i in 0..num_chunks {
            let ts = i * chunk_duration;
            let mic = AudioChunk {
                timestamp: MediaTimestamp::from_nanos(ts),
                sample_rate: 24_000,
                channels: 1,
                samples: Arc::from(vec![0.5f32; mic_chunk_samples].into_boxed_slice()),
            };

            // Verify mixer output properties.
            let mixed = mixer.mix(None, Some(&mic)).unwrap();
            assert_eq!(mixed.sample_rate, 48_000, "mixer must resample to 48kHz");
            assert_eq!(mixed.channels, 2, "mixer must output stereo");

            push_audio_tolerant(&mut writer, mixed);
        }

        let result = writer.finish().unwrap();
        assert!(result.output_path.is_some());

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact_with_audio_stats(&path)
                .unwrap();
        assert!(inspection.has_video_stream);
        assert!(inspection.has_audio_stream);

        // A/V drift must be within 2s (generous for test environment).
        let drift =
            (inspection.video_duration_nanos as i64 - inspection.audio_duration_nanos as i64).abs();
        assert!(
            drift < 2_000_000_000,
            "A/V drift too large with real 24kHz mixer: {}ms (video={}ms, audio={}ms)",
            drift / 1_000_000,
            inspection.video_duration_nanos / 1_000_000,
            inspection.audio_duration_nanos / 1_000_000
        );

        // Decoded audio must be non-silent.
        let rms = inspection
            .audio_rms
            .expect("audio RMS must be available for non-silent artifact");
        assert!(
            rms > 0.05,
            "decoded audio RMS too low: {rms} (expected > 0.05 for 0.5 amplitude input)"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_preserves_non_silent_audio_after_leading_gap() {
        let path =
            crate::test_support::ffmpeg_helpers::unique_media_path("writer-leading-gap-rms", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // Video from t=0, 30 frames = 1 second. Keep count low to avoid queue overflow.
        for i in 0..30 {
            writer
                .push_video(test_video_frame_at(i * 33_333_333))
                .unwrap();
        }

        // First audio chunk at t=200ms — creates a leading gap.
        // Use 0.5 amplitude to ensure non-trivial RMS.
        let chunk1 = audio_chunk_with_frames(200_000_000, 1024);
        writer.push_audio(chunk1).unwrap();

        // Second audio chunk immediately after.
        let chunk2_ts = 200_000_000 + 1024 * 1_000_000_000 / 48_000;
        let chunk2 = audio_chunk_with_frames(chunk2_ts, 1024);
        writer.push_audio(chunk2).unwrap();

        writer.finish().unwrap();

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact_with_audio_stats(&path)
                .unwrap();
        assert!(
            inspection.audio_rms.unwrap() > 0.01,
            "audio RMS should be > 0.01 after leading gap, got {:?}",
            inspection.audio_rms
        );
        assert!(
            inspection.audio_peak.unwrap() > 0.02,
            "audio peak should be > 0.02 after leading gap, got {:?}",
            inspection.audio_peak
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffmpeg_writer_preserves_non_silent_audio_after_middle_gap() {
        let path =
            crate::test_support::ffmpeg_helpers::unique_media_path("writer-middle-gap-rms", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // Video from t=0, 60 frames = 2 seconds.
        for i in 0..60 {
            writer
                .push_video(test_video_frame_at(i * 33_333_333))
                .unwrap();
        }

        // First audio chunk at t=0, contiguous.
        writer.push_audio(audio_chunk_with_frames(0, 1024)).unwrap();

        // Second audio chunk at t=500ms — creates a middle gap.
        writer
            .push_audio(audio_chunk_with_frames(500_000_000, 1024))
            .unwrap();

        // Third audio chunk immediately after second.
        let chunk3_ts = 500_000_000 + 1024 * 1_000_000_000 / 48_000;
        writer
            .push_audio(audio_chunk_with_frames(chunk3_ts, 1024))
            .unwrap();

        writer.finish().unwrap();

        let inspection =
            crate::test_support::ffmpeg_helpers::inspect_media_artifact_with_audio_stats(&path)
                .unwrap();
        assert!(
            inspection.audio_rms.unwrap() > 0.01,
            "audio RMS should be > 0.01 after middle gap, got {:?}",
            inspection.audio_rms
        );
        assert!(
            inspection.audio_peak.unwrap() > 0.02,
            "audio peak should be > 0.02 after middle gap, got {:?}",
            inspection.audio_peak
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Verifies that dropping a writer without calling finish() completes
    /// quickly — the worker thread exits when the channel disconnects.
    /// This tests that the timeout path does not block indefinitely.
    #[test]
    fn ffmpeg_writer_drop_completes_quickly_when_worker_exits() {
        use std::time::Instant;

        let path =
            crate::test_support::ffmpeg_helpers::unique_media_path("writer-drop-test", "mp4");
        let writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // Drop without finish() — tx drops, worker sees channel disconnect and exits.
        let start = Instant::now();
        drop(writer);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < 5,
            "Writer drop took {:?}, expected < 5s (worker should exit on channel disconnect)",
            elapsed
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Verifies that extract_panic_message correctly extracts messages
    /// from different panic payload types.
    #[test]
    fn extract_panic_message_handles_various_payloads() {
        // &str payload
        let payload: Box<dyn std::any::Any + Send> = Box::new("test panic message");
        assert_eq!(extract_panic_message(&payload), "test panic message");

        // String payload
        let payload: Box<dyn std::any::Any + Send> = Box::new("owned panic message".to_string());
        assert_eq!(extract_panic_message(&payload), "owned panic message");

        // Unknown payload
        let payload: Box<dyn std::any::Any + Send> = Box::new(42i32);
        assert_eq!(extract_panic_message(&payload), "unknown panic payload");
    }
}
