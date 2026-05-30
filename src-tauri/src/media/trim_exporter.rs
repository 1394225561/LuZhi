use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(feature = "ffmpeg")]
use crate::app::error::AppError;
use crate::app::error::AppResult;
use crate::core::cut::CutTimeline;
use crate::media::export_presets::ExportPreset;

#[derive(Clone)]
pub struct ExportProgressReporter {
    callback: Arc<dyn Fn(u8) + Send + Sync>,
}

impl std::fmt::Debug for ExportProgressReporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportProgressReporter")
            .field("callback", &"<closure>")
            .finish()
    }
}

impl ExportProgressReporter {
    pub fn new(callback: Arc<dyn Fn(u8) + Send + Sync>) -> Self {
        Self { callback }
    }

    pub fn report(&self, progress: u8) {
        (self.callback)(progress.min(100));
    }
}

// Intentionally always-equal: progress reporters are excluded from
// TrimExportRequest equality so that request comparison focuses on
// input/output paths, preset, and timeline — not callback identity.
impl PartialEq for ExportProgressReporter {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// Structured request for a future FFmpeg binding implementation.
#[derive(Clone, Debug)]
pub struct TrimExportRequest {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub preset: ExportPreset,
    pub cut_timeline: CutTimeline,
    pub effect_timeline_path: Option<PathBuf>,
    pub cancel_token: Arc<AtomicBool>,
    pub progress: Option<ExportProgressReporter>,
}

impl PartialEq for TrimExportRequest {
    fn eq(&self, other: &Self) -> bool {
        self.input_path == other.input_path
            && self.output_path == other.output_path
            && self.preset == other.preset
            && self.cut_timeline == other.cut_timeline
            && self.effect_timeline_path == other.effect_timeline_path
    }
}

/// Result of a structured trim export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrimExportResult {
    pub output_path: PathBuf,
    pub cut_count: usize,
}

/// Exporter boundary that consumes CutTimeline without shelling out to FFmpeg CLI.
pub trait TrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult>;
}

/// Test exporter that records requests without touching source files.
#[derive(Default)]
pub struct MockTrimExporter {
    requests: Vec<TrimExportRequest>,
}

impl MockTrimExporter {
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
        }
    }

    pub fn requests(&self) -> &[TrimExportRequest] {
        &self.requests
    }
}

impl TrimExporter for MockTrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
        if request.cancel_token.load(Ordering::Relaxed) {
            return Err(crate::app::error::AppError::ExportCancelled);
        }
        let result = TrimExportResult {
            output_path: request.output_path.clone(),
            cut_count: request.cut_timeline.cuts.len(),
        };
        if let Some(progress) = &request.progress {
            progress.report(100);
        }
        self.requests.push(request);
        Ok(result)
    }
}

/// Feature-gated production boundary. It intentionally accepts structured data
/// rather than a shell command string, preserving the no-CLI security rule.
#[cfg(feature = "ffmpeg")]
pub struct FfmpegTrimExporter;

#[cfg(feature = "ffmpeg")]
impl TrimExporter for FfmpegTrimExporter {
    fn export(&mut self, request: TrimExportRequest) -> AppResult<TrimExportResult> {
        use ff::codec;
        use ff::codec::encoder;
        use ff::format;
        use ff::software;
        use ff::util::format::{sample, Pixel, Sample};
        use ff::util::frame;
        use ff::{ChannelLayout, Rational};
        use ffmpeg_next as ff;

        if request.cancel_token.load(Ordering::Relaxed) {
            return Err(AppError::ExportCancelled);
        }
        if !request.input_path.exists() {
            return Err(AppError::ExportFailed {
                reason: format!("源文件不存在: {}", request.input_path.to_string_lossy()),
            });
        }
        if request.input_path == request.output_path {
            return Err(AppError::ExportFailed {
                reason: "导出文件不能覆盖原始录制文件".to_string(),
            });
        }

        ff::init().map_err(|e| AppError::ExportFailed {
            reason: format!("初始化 FFmpeg 失败: {e}"),
        })?;

        // --- Input ---
        let mut input = format::input(&request.input_path).map_err(|e| AppError::ExportFailed {
            reason: format!("打开源文件失败: {e}"),
        })?;

        // Find video stream.
        let video_stream_index = input
            .streams()
            .find(|s| s.parameters().medium() == ff::media::Type::Video)
            .map(|s| s.index())
            .ok_or(AppError::ExportFailed {
                reason: "源文件中未找到视频流".to_string(),
            })?;
        let video_time_base = input.stream(video_stream_index).unwrap().time_base();

        // Find audio stream (optional).
        let audio_stream_info = input
            .streams()
            .find(|s| s.parameters().medium() == ff::media::Type::Audio)
            .map(|s| (s.index(), s.time_base()));

        // Create video decoder.
        let video_stream = input.stream(video_stream_index).unwrap();
        let video_params = video_stream.parameters();
        let video_codec_id = video_params.id();
        let video_codec = codec::decoder::find(video_codec_id).ok_or(AppError::ExportFailed {
            reason: "未找到视频解码器".to_string(),
        })?;
        let video_ctx = ff::codec::Context::from_parameters(video_params).map_err(|e| {
            AppError::ExportFailed {
                reason: format!("创建视频解码上下文失败: {e}"),
            }
        })?;
        let mut video_dec = video_ctx
            .decoder()
            .open_as(video_codec)
            .map_err(|e| AppError::ExportFailed {
                reason: format!("打开视频解码器失败: {e}"),
            })?
            .video()
            .map_err(|e| AppError::ExportFailed {
                reason: format!("创建视频解码器失败: {e}"),
            })?;

        // Create audio decoder (if audio stream exists).
        let mut audio_dec = if let Some((idx, _)) = audio_stream_info {
            let audio_stream = input.stream(idx).unwrap();
            let audio_params = audio_stream.parameters();
            let audio_codec_id = audio_params.id();
            let audio_codec =
                codec::decoder::find(audio_codec_id).ok_or(AppError::ExportFailed {
                    reason: "未找到音频解码器".to_string(),
                })?;
            let audio_ctx = ff::codec::Context::from_parameters(audio_params).map_err(|e| {
                AppError::ExportFailed {
                    reason: format!("创建音频解码上下文失败: {e}"),
                }
            })?;
            Some(
                audio_ctx
                    .decoder()
                    .open_as(audio_codec)
                    .map_err(|e| AppError::ExportFailed {
                        reason: format!("打开音频解码器失败: {e}"),
                    })?
                    .audio()
                    .map_err(|e| AppError::ExportFailed {
                        reason: format!("创建音频解码器失败: {e}"),
                    })?,
            )
        } else {
            None
        };

        // --- Output ---
        // Ensure parent directory exists.
        if let Some(parent) = request.output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::ExportFailed {
                reason: format!("创建输出目录失败: {e}"),
            })?;
        }
        let mut output =
            format::output(&request.output_path).map_err(|e| AppError::ExportFailed {
                reason: format!("创建输出文件失败: {e}"),
            })?;

        let src_width = video_dec.width();
        let src_height = video_dec.height();
        let preset_spec = request.preset.spec();
        let out_w = preset_spec.width;
        let out_h = preset_spec.height;

        // Video encoder (H.264).
        let enc_video_codec = encoder::find(ff::codec::Id::H264).ok_or(AppError::ExportFailed {
            reason: "未找到 H.264 编码器".to_string(),
        })?;
        let video_enc_ctx = ff::codec::Context::new_with_codec(enc_video_codec);
        let mut video_enc =
            video_enc_ctx
                .encoder()
                .video()
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("创建视频编码器失败: {e}"),
                })?;
        video_enc.set_width(out_w);
        video_enc.set_height(out_h);
        video_enc.set_bit_rate(preset_spec.video_bitrate_kbps as usize * 1000);
        video_enc.set_time_base(video_time_base);
        video_enc.set_format(Pixel::YUV420P);
        video_enc.set_max_b_frames(0);
        let video_opts = ff::Dictionary::from_iter([("preset", "ultrafast")]);
        let mut video_encoder = video_enc
            .open_as_with(enc_video_codec, video_opts)
            .map_err(|e| AppError::ExportFailed {
                reason: format!("打开视频编码器失败: {e}"),
            })?;
        let video_params_out = codec::Parameters::from(&video_encoder);
        let mut video_stream_out =
            output
                .add_stream(enc_video_codec)
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("添加视频输出流失败: {e}"),
                })?;
        video_stream_out.set_time_base(video_time_base);
        video_stream_out.set_parameters(video_params_out);
        let video_out_idx = video_stream_out.index();

        // Audio encoder (AAC).
        let (mut audio_encoder, audio_out_idx, audio_enc_tb) = if audio_dec.is_some() {
            let enc_audio_codec =
                encoder::find(ff::codec::Id::AAC).ok_or(AppError::ExportFailed {
                    reason: "未找到 AAC 编码器".to_string(),
                })?;
            let audio_enc_ctx = ff::codec::Context::new_with_codec(enc_audio_codec);
            let mut audio_enc =
                audio_enc_ctx
                    .encoder()
                    .audio()
                    .map_err(|e| AppError::ExportFailed {
                        reason: format!("创建音频编码器失败: {e}"),
                    })?;
            audio_enc.set_rate(48000);
            audio_enc.set_channel_layout(ChannelLayout::STEREO);
            audio_enc.set_format(Sample::F32(sample::Type::Planar));
            let tb = Rational(1, 48000);
            audio_enc.set_time_base(tb);
            let enc = audio_enc
                .open_as(enc_audio_codec)
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("打开音频编码器失败: {e}"),
                })?;
            let params = codec::Parameters::from(&enc);
            let mut stream =
                output
                    .add_stream(enc_audio_codec)
                    .map_err(|e| AppError::ExportFailed {
                        reason: format!("添加音频输出流失败: {e}"),
                    })?;
            stream.set_time_base(tb);
            stream.set_parameters(params);
            let idx = stream.index();
            (Some(enc), Some(idx), Some(tb))
        } else {
            (None, None, None)
        };

        // Video scaler — source pixel format from decoder output.
        let src_pix_fmt = video_dec.format();
        let mut scaler = software::scaling::Context::get(
            src_pix_fmt,
            src_width,
            src_height,
            Pixel::YUV420P,
            out_w,
            out_h,
            software::scaling::Flags::BILINEAR,
        )
        .map_err(|e| AppError::ExportFailed {
            reason: format!("创建像素格式转换器失败: {e}"),
        })?;

        output.write_header().map_err(|e| AppError::ExportFailed {
            reason: format!("写入文件头失败: {e}"),
        })?;

        // --- Segment-based decode/encode loop ---
        // Track cumulative cut duration so output timestamps are continuous
        // across keep segments. E.g., keeps [0..2s, 6..8s] → output [0..2s, 2..4s].
        let total_keeps = request.cut_timeline.keeps.len();
        let mut video_pts_offset: Option<i64> = None;
        let mut audio_pts_offset: Option<i64> = None;
        let mut cumulative_cut_nanos: i64 = 0;
        let mut prev_seg_end_nanos: i64 = 0;

        for (seg_idx, segment) in request.cut_timeline.keeps.iter().enumerate() {
            if request.cancel_token.load(Ordering::Relaxed) {
                let _ = std::fs::remove_file(&request.output_path);
                return Err(AppError::ExportCancelled);
            }

            let seg_start_nanos = segment.start.nanos as i64;
            let seg_end_nanos = segment.end.nanos as i64;

            // Accumulate cut duration between consecutive keep segments.
            if seg_idx > 0 {
                cumulative_cut_nanos += seg_start_nanos - prev_seg_end_nanos;
            }
            prev_seg_end_nanos = seg_end_nanos;

            // Seek input to segment start (in stream time_base).
            input
                .seek(seg_start_nanos, ..seg_end_nanos)
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("定位到裁剪段失败: {e}"),
                })?;

            // Read packets until we pass the segment end.
            let mut packet = ff::Packet::empty();
            loop {
                if request.cancel_token.load(Ordering::Relaxed) {
                    let _ = std::fs::remove_file(&request.output_path);
                    return Err(AppError::ExportCancelled);
                }

                match packet.read(&mut input) {
                    Ok(()) => {
                        let pkt_stream = packet.stream();
                        let pkt_ts = packet.pts().unwrap_or(0);

                        // Stop if we've passed the segment end.
                        let pkt_nanos =
                            pkt_ts * video_time_base.0 as i64 / video_time_base.1 as i64;
                        if pkt_nanos > seg_end_nanos && pkt_stream == video_stream_index {
                            break;
                        }

                        // Skip packets before segment start.
                        if pkt_nanos < seg_start_nanos && pkt_stream == video_stream_index {
                            continue;
                        }

                        if pkt_stream == video_stream_index {
                            // Decode → scale → encode video.
                            video_dec
                                .send_packet(&packet)
                                .map_err(|e| AppError::ExportFailed {
                                    reason: format!("发送视频包到解码器失败: {e}"),
                                })?;

                            let mut decoded = unsafe { frame::Frame::empty() };
                            while video_dec.receive_frame(&mut decoded).is_ok() {
                                if video_pts_offset.is_none() {
                                    video_pts_offset = Some(decoded.pts().unwrap_or(0));
                                }
                                // Remap PTS: subtract first segment start and cumulative
                                // cut duration to produce continuous output timestamps.
                                let raw_pts = decoded.pts().unwrap_or(0);
                                let cut_offset_in_tb = cumulative_cut_nanos
                                    * video_time_base.1 as i64
                                    / video_time_base.0 as i64;
                                let out_pts =
                                    raw_pts - video_pts_offset.unwrap() - cut_offset_in_tb;

                                // Copy decoded frame data row-by-row, respecting source
                                // linesize (stride) padding that differs from destination.
                                let mut input_frame =
                                    frame::Video::new(video_dec.format(), src_width, src_height);
                                input_frame.set_pts(decoded.pts());
                                // Pre-compute plane heights before mutable borrow of data_mut.
                                let num_planes = input_frame.planes().min(4);
                                let plane_heights: Vec<u32> = (0..num_planes)
                                    .map(|i| input_frame.plane_height(i))
                                    .collect();
                                unsafe {
                                    let src_ptr = decoded.as_ptr();
                                    for plane_idx in 0..num_planes {
                                        let dst = input_frame.data_mut(plane_idx);
                                        let src_data = (*src_ptr).data[plane_idx];
                                        let src_linesize = (*src_ptr).linesize[plane_idx] as usize;
                                        if src_data.is_null() || dst.is_empty() {
                                            continue;
                                        }
                                        let plane_h = plane_heights[plane_idx] as usize;
                                        let dst_linesize = if plane_h > 0 {
                                            dst.len() / plane_h
                                        } else {
                                            continue;
                                        };
                                        let copy_per_row = dst_linesize.min(src_linesize);
                                        for row in 0..plane_h {
                                            let src_row = src_data.add(row * src_linesize);
                                            let dst_row = dst.as_mut_ptr().add(row * dst_linesize);
                                            std::ptr::copy_nonoverlapping(
                                                src_row,
                                                dst_row,
                                                copy_per_row,
                                            );
                                        }
                                    }
                                }

                                let mut output_frame =
                                    frame::Video::new(Pixel::YUV420P, out_w, out_h);
                                output_frame.set_pts(Some(out_pts));

                                scaler.run(&input_frame, &mut output_frame).map_err(|e| {
                                    AppError::ExportFailed {
                                        reason: format!("像素格式转换失败: {e}"),
                                    }
                                })?;

                                video_encoder.send_frame(&output_frame).map_err(|e| {
                                    AppError::ExportFailed {
                                        reason: format!("编码视频帧失败: {e}"),
                                    }
                                })?;

                                let mut enc_pkt = ff::Packet::empty();
                                while video_encoder.receive_packet(&mut enc_pkt).is_ok() {
                                    enc_pkt.set_stream(video_out_idx);
                                    enc_pkt.rescale_ts(video_time_base, video_time_base);
                                    enc_pkt.write_interleaved(&mut output).map_err(|e| {
                                        AppError::ExportFailed {
                                            reason: format!("写入视频数据包失败: {e}"),
                                        }
                                    })?;
                                }
                            }
                        } else if Some(pkt_stream) == audio_out_idx {
                            // Decode → encode audio.
                            if let Some(ref mut dec) = audio_dec {
                                dec.send_packet(&packet)
                                    .map_err(|e| AppError::ExportFailed {
                                        reason: format!("发送音频包到解码器失败: {e}"),
                                    })?;

                                let mut decoded = unsafe { frame::Frame::empty() };
                                while dec.receive_frame(&mut decoded).is_ok() {
                                    let audio_tb = audio_enc_tb.unwrap_or(Rational(1, 48000));
                                    if audio_pts_offset.is_none() {
                                        audio_pts_offset = Some(decoded.pts().unwrap_or(0));
                                    }
                                    let raw_audio_pts = decoded.pts().unwrap_or(0);
                                    let audio_cut_offset = cumulative_cut_nanos * audio_tb.1 as i64
                                        / audio_tb.0 as i64;
                                    let out_pts = raw_audio_pts
                                        - audio_pts_offset.unwrap()
                                        - audio_cut_offset;
                                    decoded.set_pts(Some(out_pts));

                                    if let Some(ref mut enc) = audio_encoder {
                                        enc.send_frame(&decoded).map_err(|e| {
                                            AppError::ExportFailed {
                                                reason: format!("编码音频帧失败: {e}"),
                                            }
                                        })?;

                                        let mut enc_pkt = ff::Packet::empty();
                                        while enc.receive_packet(&mut enc_pkt).is_ok() {
                                            if let Some(idx) = audio_out_idx {
                                                enc_pkt.set_stream(idx);
                                            }
                                            enc_pkt.rescale_ts(audio_tb, audio_tb);
                                            enc_pkt.write_interleaved(&mut output).map_err(
                                                |e| AppError::ExportFailed {
                                                    reason: format!("写入音频数据包失败: {e}"),
                                                },
                                            )?;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(ff::Error::Eof) => break,
                    Err(e) => {
                        // Log non-EOF read errors but continue — a single corrupt
                        // packet should not abort the entire export.
                        eprintln!("警告：读取数据包时出错（跳过）: {e}");
                        continue;
                    }
                }
            }

            // Report progress per segment.
            if let Some(ref progress) = request.progress {
                let pct = ((seg_idx + 1) * 100 / total_keeps).min(100) as u8;
                progress.report(pct);
            }
        }

        // Flush encoders.
        video_encoder
            .send_eof()
            .map_err(|e| AppError::ExportFailed {
                reason: format!("刷新视频编码器失败: {e}"),
            })?;
        let mut enc_pkt = ff::Packet::empty();
        while video_encoder.receive_packet(&mut enc_pkt).is_ok() {
            enc_pkt.set_stream(video_out_idx);
            enc_pkt.rescale_ts(video_time_base, video_time_base);
            enc_pkt
                .write_interleaved(&mut output)
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("写入视频数据包失败: {e}"),
                })?;
        }

        if let Some(ref mut enc) = audio_encoder {
            enc.send_eof().map_err(|e| AppError::ExportFailed {
                reason: format!("刷新音频编码器失败: {e}"),
            })?;
            while enc.receive_packet(&mut enc_pkt).is_ok() {
                if let Some(idx) = audio_out_idx {
                    enc_pkt.set_stream(idx);
                }
                enc_pkt
                    .write_interleaved(&mut output)
                    .map_err(|e| AppError::ExportFailed {
                        reason: format!("写入音频数据包失败: {e}"),
                    })?;
            }
        }

        output.write_trailer().map_err(|e| AppError::ExportFailed {
            reason: format!("写入文件尾失败: {e}"),
        })?;

        Ok(TrimExportResult {
            output_path: request.output_path,
            cut_count: request.cut_timeline.cuts.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::cut::CutTimeline;
    use crate::media::export_presets::ExportPreset;
    use std::sync::{atomic::AtomicBool, Arc};

    #[test]
    fn mock_exporter_consumes_cut_timeline_without_deleting_original() {
        let mut exporter = MockTrimExporter::new();
        let request = TrimExportRequest {
            input_path: PathBuf::from("/tmp/raw.mov"),
            output_path: PathBuf::from("/tmp/export.mp4"),
            preset: ExportPreset::Bilibili,
            cut_timeline: CutTimeline::empty(10_000_000_000),
            effect_timeline_path: None,
            cancel_token: Arc::new(AtomicBool::new(false)),
            progress: None,
        };

        let result = exporter.export(request).unwrap();

        assert_eq!(result.output_path, PathBuf::from("/tmp/export.mp4"));
        assert_eq!(result.cut_count, 0);
        assert_eq!(exporter.requests().len(), 1);
        assert_eq!(
            exporter.requests()[0].input_path,
            PathBuf::from("/tmp/raw.mov")
        );
    }

    #[test]
    fn export_preset_rejects_unknown_value() {
        assert_eq!(
            "bilibili".parse::<ExportPreset>().unwrap(),
            ExportPreset::Bilibili
        );
        assert!("unknown".parse::<ExportPreset>().is_err());
    }

    #[cfg(feature = "ffmpeg")]
    mod ffmpeg_tests {
        use super::*;
        use crate::core::cut::{CutReason, CutSegment, KeepSegment};
        use crate::core::frame::MediaTimestamp;
        use crate::test_support::ffmpeg_helpers;

        /// Create a synthetic source artifact and return its path.
        fn create_source(prefix: &str, duration_nanos: u64) -> PathBuf {
            let path = ffmpeg_helpers::unique_media_path(prefix, "mp4");
            ffmpeg_helpers::create_synthetic_source_artifact(&path, 1920, 1080, duration_nanos)
                .unwrap();
            path
        }

        #[test]
        fn ffmpeg_exporter_reproduces_source_when_no_cuts() {
            let source = create_source("export-no-cut", 500_000_000);
            let output = ffmpeg_helpers::unique_media_path("export-no-cut-out", "mp4");

            let mut exporter = FfmpegTrimExporter;
            let result = exporter
                .export(TrimExportRequest {
                    input_path: source.clone(),
                    output_path: output.clone(),
                    preset: ExportPreset::Bilibili,
                    cut_timeline: CutTimeline::empty(500_000_000),
                    effect_timeline_path: None,
                    cancel_token: Arc::new(AtomicBool::new(false)),
                    progress: None,
                })
                .unwrap();

            assert_eq!(result.output_path, output);
            assert_eq!(result.cut_count, 0);
            assert!(output.exists());
            assert!(std::fs::metadata(&output).unwrap().len() > 0);

            let inspection = ffmpeg_helpers::inspect_media_artifact(&output).unwrap();
            assert!(inspection.has_video_stream);
            assert!(inspection.has_audio_stream);

            let _ = std::fs::remove_file(&source);
            let _ = std::fs::remove_file(&output);
        }

        #[test]
        fn ffmpeg_exporter_rejects_missing_source() {
            let mut exporter = FfmpegTrimExporter;
            let result = exporter.export(TrimExportRequest {
                input_path: PathBuf::from("/tmp/nonexistent-source.mp4"),
                output_path: PathBuf::from("/tmp/out.mp4"),
                preset: ExportPreset::Bilibili,
                cut_timeline: CutTimeline::empty(1_000_000_000),
                effect_timeline_path: None,
                cancel_token: Arc::new(AtomicBool::new(false)),
                progress: None,
            });

            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("源文件"));
        }

        #[test]
        fn ffmpeg_exporter_rejects_overwrite() {
            let source = create_source("export-overwrite", 500_000_000);

            let mut exporter = FfmpegTrimExporter;
            let result = exporter.export(TrimExportRequest {
                input_path: source.clone(),
                output_path: source.clone(),
                preset: ExportPreset::Bilibili,
                cut_timeline: CutTimeline::empty(500_000_000),
                effect_timeline_path: None,
                cancel_token: Arc::new(AtomicBool::new(false)),
                progress: None,
            });

            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("覆盖"));

            let _ = std::fs::remove_file(&source);
        }

        #[test]
        fn ffmpeg_exporter_respects_cancel_token() {
            let source = create_source("export-cancel", 500_000_000);
            let output = ffmpeg_helpers::unique_media_path("export-cancel-out", "mp4");

            let cancel = Arc::new(AtomicBool::new(true)); // Already cancelled.
            let mut exporter = FfmpegTrimExporter;
            let result = exporter.export(TrimExportRequest {
                input_path: source.clone(),
                output_path: output.clone(),
                preset: ExportPreset::Bilibili,
                cut_timeline: CutTimeline::empty(500_000_000),
                effect_timeline_path: None,
                cancel_token: cancel,
                progress: None,
            });

            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("取消"));

            let _ = std::fs::remove_file(&source);
            let _ = std::fs::remove_file(&output);
        }

        #[test]
        fn ffmpeg_exporter_removes_output_on_cancel() {
            let source = create_source("export-cancel-cleanup", 500_000_000);
            let output = ffmpeg_helpers::unique_media_path("export-cancel-cleanup-out", "mp4");

            let cancel = Arc::new(AtomicBool::new(true));
            let mut exporter = FfmpegTrimExporter;
            let _ = exporter.export(TrimExportRequest {
                input_path: source.clone(),
                output_path: output.clone(),
                preset: ExportPreset::Bilibili,
                cut_timeline: CutTimeline::empty(500_000_000),
                effect_timeline_path: None,
                cancel_token: cancel,
                progress: None,
            });

            assert!(!output.exists(), "cancelled export should remove output");

            let _ = std::fs::remove_file(&source);
        }
    }
}
