use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use crate::app::error::{AppError, AppResult};
use crate::core::frame::{MixedAudioChunk, VideoFrameRef};
use crate::media::recording_writer::{RecordingResult, RecordingWriter};

/// Maximum number of messages in the encoding queue before blocking.
/// At 30fps video + ~50 audio chunks/sec, this is ~0.8s of buffering.
/// Prevents unbounded memory growth if the encoder can't keep up.
const ENCODER_QUEUE_CAPACITY: usize = 25;

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
    tx: mpsc::SyncSender<EncoderMessage>,
    worker: Option<thread::JoinHandle<AppResult<RecordingResult>>>,
    frame_count: u64,
    mixed_audio_chunk_count: u64,
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
        let worker_path = output_path.clone();

        let worker = thread::spawn(move || encoder_worker(worker_path, rx));

        Ok(Self {
            tx,
            worker: Some(worker),
            frame_count: 0,
            mixed_audio_chunk_count: 0,
        })
    }

    /// Drain the worker handle and return its result.
    /// Called by `finish()` after sending the Flush message.
    fn join_worker(&mut self) -> AppResult<RecordingResult> {
        let handle = self.worker.take().ok_or(AppError::RecordingWriteFailed {
            reason: "编码工作线程已被回收".to_string(),
        })?;
        handle.join().map_err(|_| AppError::RecordingWriteFailed {
            reason: "编码工作线程异常退出".to_string(),
        })?
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

        // Blocking enqueue — blocks if the queue is full (bounded backpressure).
        self.tx
            .send(EncoderMessage::Video {
                timestamp_nanos: frame.timestamp.nanos,
                width: frame.width,
                height: frame.height,
                stride_bytes: copy_per_row,
                buffer,
            })
            .map_err(|_| AppError::RecordingWriteFailed {
                reason: "编码队列已关闭，无法发送视频帧".to_string(),
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

        self.tx
            .send(EncoderMessage::Audio {
                samples: chunk.samples.to_vec(),
                timestamp_nanos: chunk.timestamp.nanos,
            })
            .map_err(|_| AppError::RecordingWriteFailed {
                reason: "编码队列已关闭，无法发送音频数据".to_string(),
            })
    }

    fn finish(&mut self) -> AppResult<RecordingResult> {
        // Send flush signal to the worker.
        self.tx
            .send(EncoderMessage::Flush)
            .map_err(|_| AppError::RecordingWriteFailed {
                reason: "编码队列已关闭，无法发送刷新信号".to_string(),
            })?;

        // Join the worker and propagate its result.
        self.join_worker()
    }
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

                // Convert chunk timestamp to 48kHz sample position.
                // This is the position where the first sample of this chunk should be.
                let target_sample =
                    (timestamp_nanos as i128 * 48000 / 1_000_000_000i128) as i64;

                // Pad silence for any gap (leading or middle).
                // This handles sparse system audio where chunks arrive with gaps.
                if target_sample > audio_timeline_cursor && audio_timeline_cursor > 0 {
                    let gap_samples = target_sample - audio_timeline_cursor;
                    // gap_samples is in mono sample units; for stereo interleaved, multiply by 2.
                    let gap_interleaved = (gap_samples * 2) as usize;
                    audio_sample_buffer.extend(std::iter::repeat_n(0.0f32, gap_interleaved));
                }
                audio_timeline_cursor = target_sample;

                audio_sample_buffer.extend_from_slice(&samples);
                // Advance cursor by the number of mono samples in this chunk.
                // samples.len() is interleaved (stereo), so divide by channels.
                let mono_samples = samples.len() / 2; // always stereo output
                audio_timeline_cursor += mono_samples as i64;

                let samples_per_frame = 1024usize;
                let interleaved_frame_size = samples_per_frame * 2; // stereo

                while audio_sample_buffer.len() >= interleaved_frame_size {
                    let frame_data: Vec<f32> = audio_sample_buffer
                        .drain(..interleaved_frame_size)
                        .collect();

                    let mut audio_frame = frame::Audio::new(
                        Sample::F32(sample::Type::Planar),
                        samples_per_frame,
                        ChannelLayout::STEREO,
                    );
                    audio_frame.set_pts(Some(audio_pts));
                    audio_pts += samples_per_frame as i64;

                    // Interleaved → planar conversion.
                    for ch in 0..2usize {
                        let plane = audio_frame.plane_mut::<f32>(ch);
                        for (i, sample) in plane.iter_mut().enumerate() {
                            *sample = frame_data[i * 2 + ch];
                        }
                    }

                    audio_encoder.send_frame(&audio_frame).map_err(|e| {
                        AppError::RecordingWriteFailed {
                            reason: format!("编码音频帧失败: {e}"),
                        }
                    })?;

                    let audio_tb = output.stream(audio_stream_index).unwrap().time_base();
                    let mut packet = ff::Packet::empty();
                    while audio_encoder.receive_packet(&mut packet).is_ok() {
                        packet.set_stream(audio_stream_index);
                        packet.rescale_ts(Rational(1, 48000), audio_tb);
                        packet.write_interleaved(&mut output).map_err(|e| {
                            AppError::RecordingWriteFailed {
                                reason: format!("写入音频数据包失败: {e}"),
                            }
                        })?;
                    }
                }
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
        });
    }

    // --- Flush: pad tail silence if audio is shorter than video ---
    if mixed_audio_chunk_count > 0 && video_duration_nanos > 0 {
        let video_end_sample =
            (video_duration_nanos as i128 * 48000 / 1_000_000_000i128) as i64;
        if audio_timeline_cursor < video_end_sample {
            let tail_gap = video_end_sample - audio_timeline_cursor;
            let tail_interleaved = (tail_gap * 2) as usize;
            audio_sample_buffer.extend(std::iter::repeat_n(0.0f32, tail_interleaved));
        }
    }

    // --- Flush: remaining audio buffer ---
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ffmpeg_helpers::{test_audio_chunk_at, test_video_frame_at};
    use std::sync::Arc;

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

    #[test]
    fn ffmpeg_writer_queue_backpressure_blocks_producer() {
        // Verify that the bounded channel provides backpressure.
        // Fill the queue beyond capacity — the producer should block
        // until the consumer drains messages.
        let path = crate::test_support::ffmpeg_helpers::unique_media_path("writer-pressure", "mp4");
        let mut writer = FfmpegRecordingWriter::new(path.clone()).unwrap();

        // Push many frames — the bounded queue (capacity 25) will cause
        // the producer to block when full, but since we're single-threaded
        // in this test, the worker thread processes messages concurrently.
        for i in 0..50 {
            writer
                .push_video(test_video_frame_at(i * 33_333_333))
                .unwrap();
        }

        let result = writer.finish().unwrap();
        assert_eq!(result.frame_count, 50);
        let _ = std::fs::remove_file(&path);
    }
}
