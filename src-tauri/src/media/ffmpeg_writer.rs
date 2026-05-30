use std::path::PathBuf;

use crate::app::error::{AppError, AppResult};
use crate::core::frame::{MixedAudioChunk, VideoFrameRef};
use crate::media::recording_writer::{RecordingResult, RecordingWriter};

use ff::codec;
use ff::codec::encoder;
use ff::format;
use ff::software;
use ff::util::format::{sample, Pixel, Sample};
use ff::util::frame;
use ff::{ChannelLayout, Rational};
use ffmpeg_next as ff;

/// Wrapper to make `software::scaling::Context` Send-safe.
struct SendScaler(software::scaling::Context);
unsafe impl Send for SendScaler {}

/// Encoded packet output via FFmpeg — muxes H.264 video + AAC audio into MP4.
///
/// # Threading
/// All public methods mutate `&mut self` and are called from a single consumer
/// thread, so no internal locking is required.
pub struct FfmpegRecordingWriter {
    output: format::context::Output,
    video_encoder: encoder::video::Encoder,
    audio_encoder: encoder::audio::Encoder,
    scaler: Option<SendScaler>,
    video_stream_index: usize,
    audio_stream_index: usize,
    frame_count: u64,
    mixed_audio_chunk_count: u64,
    video_pts: i64,
    audio_pts: i64,
    output_path: PathBuf,
}

impl FfmpegRecordingWriter {
    pub fn new(output_path: PathBuf) -> AppResult<Self> {
        ff::init().map_err(|e| AppError::RecordingWriteFailed {
            reason: format!("初始化 FFmpeg 失败: {e}"),
        })?;

        // Ensure parent directory exists.
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("创建输出目录失败: {e}"),
            })?;
        }

        let mut output =
            format::output(&output_path).map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("创建输出文件失败: {e}"),
            })?;

        // --- Video encoder (H.264) ---
        let video_codec =
            encoder::find(ff::codec::Id::H264).ok_or(AppError::RecordingWriteFailed {
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
        video_enc.set_time_base(Rational(1, 1_000_000_000));
        video_enc.set_format(Pixel::YUV420P);
        video_enc.set_max_b_frames(0);
        let video_opts = ff::Dictionary::from_iter([("preset", "ultrafast")]);
        let video_encoder = video_enc
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
        video_stream.set_time_base(Rational(1, 1_000_000_000));
        video_stream.set_parameters(video_params);
        let video_stream_index = video_stream.index();

        // --- Audio encoder (AAC) ---
        let audio_codec =
            encoder::find(ff::codec::Id::AAC).ok_or(AppError::RecordingWriteFailed {
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
        let audio_encoder =
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

        // Scaler is created lazily on first frame to match actual input dimensions.
        output
            .write_header()
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("写入文件头失败: {e}"),
            })?;

        Ok(Self {
            output,
            video_encoder,
            audio_encoder,
            scaler: None,
            video_stream_index,
            audio_stream_index,
            frame_count: 0,
            mixed_audio_chunk_count: 0,
            video_pts: 0,
            audio_pts: 0,
            output_path,
        })
    }

    fn encode_and_write_video(&mut self) -> AppResult<()> {
        let mut packet = ff::Packet::empty();
        while self.video_encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(self.video_stream_index);
            packet.rescale_ts(
                Rational(1, 1_000_000_000),
                self.output
                    .stream(self.video_stream_index)
                    .unwrap()
                    .time_base(),
            );
            packet.write_interleaved(&mut self.output).map_err(|e| {
                AppError::RecordingWriteFailed {
                    reason: format!("写入视频数据包失败: {e}"),
                }
            })?;
        }
        Ok(())
    }

    fn encode_and_write_audio(&mut self) -> AppResult<()> {
        let mut packet = ff::Packet::empty();
        while self.audio_encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(self.audio_stream_index);
            packet.rescale_ts(
                Rational(1, 48000),
                self.output
                    .stream(self.audio_stream_index)
                    .unwrap()
                    .time_base(),
            );
            packet.write_interleaved(&mut self.output).map_err(|e| {
                AppError::RecordingWriteFailed {
                    reason: format!("写入音频数据包失败: {e}"),
                }
            })?;
        }
        Ok(())
    }

    fn flush_video(&mut self) -> AppResult<()> {
        self.video_encoder
            .send_eof()
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("刷新视频编码器失败: {e}"),
            })?;
        self.encode_and_write_video()
    }

    fn flush_audio(&mut self) -> AppResult<()> {
        self.audio_encoder
            .send_eof()
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("刷新音频编码器失败: {e}"),
            })?;
        self.encode_and_write_audio()
    }
}

impl RecordingWriter for FfmpegRecordingWriter {
    fn push_video(&mut self, frame: VideoFrameRef) -> AppResult<()> {
        self.frame_count += 1;

        let mut input_frame = frame::Video::new(Pixel::BGRA, frame.width, frame.height);
        let src_data = match &frame.buffer {
            crate::core::frame::FrameBuffer::Owned(arc) => arc.as_ref(),
        };
        let dst = input_frame.data_mut(0);
        dst[..src_data.len()].copy_from_slice(src_data);

        let mut output_frame = frame::Video::new(Pixel::YUV420P, 1920, 1080);
        self.video_pts = frame.timestamp.nanos as i64;
        output_frame.set_pts(Some(self.video_pts));

        // Recreate scaler if input dimensions changed.
        let needs_new = match &self.scaler {
            Some(s) => {
                let ctx = &s.0;
                ctx.input().width != frame.width || ctx.input().height != frame.height
            }
            None => true,
        };
        if needs_new {
            self.scaler = Some(SendScaler(
                software::scaling::Context::get(
                    Pixel::BGRA,
                    frame.width,
                    frame.height,
                    Pixel::YUV420P,
                    1920,
                    1080,
                    software::scaling::Flags::BILINEAR,
                )
                .map_err(|e| AppError::RecordingWriteFailed {
                    reason: format!("创建像素格式转换器失败: {e}"),
                })?,
            ));
        }

        self.scaler
            .as_mut()
            .unwrap()
            .0
            .run(&input_frame, &mut output_frame)
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("像素格式转换失败: {e}"),
            })?;

        self.video_encoder.send_frame(&output_frame).map_err(|e| {
            AppError::RecordingWriteFailed {
                reason: format!("编码视频帧失败: {e}"),
            }
        })?;

        self.encode_and_write_video()
    }

    fn push_audio(&mut self, chunk: MixedAudioChunk) -> AppResult<()> {
        self.mixed_audio_chunk_count += 1;

        let num_frames = chunk.samples.len() / chunk.channels.max(1) as usize;
        let mut frame = frame::Audio::new(
            Sample::F32(sample::Type::Planar),
            num_frames,
            ChannelLayout::STEREO,
        );
        frame.set_pts(Some(self.audio_pts));
        self.audio_pts += num_frames as i64;

        // Convert interleaved f32 → planar f32.
        let channels = chunk.channels.max(1) as usize;
        for ch in 0..channels {
            let plane = frame.plane_mut::<f32>(ch);
            for (i, sample) in plane.iter_mut().enumerate() {
                *sample = chunk.samples[i * channels + ch];
            }
        }

        self.audio_encoder
            .send_frame(&frame)
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("编码音频帧失败: {e}"),
            })?;

        self.encode_and_write_audio()
    }

    fn finish(&mut self) -> AppResult<RecordingResult> {
        self.flush_video()?;
        self.flush_audio()?;

        self.output
            .write_trailer()
            .map_err(|e| AppError::RecordingWriteFailed {
                reason: format!("写入文件尾失败: {e}"),
            })?;

        let duration_secs = if self.frame_count > 0 {
            (self.video_pts as u64 / 1_000_000_000).max(1)
        } else {
            0
        };

        Ok(RecordingResult {
            duration_secs,
            frame_count: self.frame_count,
            mixed_audio_chunk_count: self.mixed_audio_chunk_count,
            output_path: Some(self.output_path.to_string_lossy().to_string()),
            cursor_metadata_path: None,
            effect_timeline_path: None,
            trim_metadata_path: None,
            cut_timeline_path: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ffmpeg_helpers::{test_audio_chunk_at, test_video_frame_at};

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

        let result = writer.finish().unwrap();
        assert_eq!(result.output_path, Some(path.to_string_lossy().to_string()));
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
}
