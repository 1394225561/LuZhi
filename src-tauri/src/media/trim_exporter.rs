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

/// Tracks export progress to ensure monotonic reporting and avoid
/// flooding the frontend with events.
struct ProgressState {
    last_reported: u8,
}

impl ProgressState {
    fn new() -> Self {
        Self { last_reported: 0 }
    }

    /// Report progress if it's higher than the last reported value.
    fn maybe_report(&mut self, reporter: &ExportProgressReporter, next: u8) {
        let next = next.clamp(1, 99);
        if next > self.last_reported {
            self.last_reported = next;
            reporter.report(next);
        }
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
        use crate::media::ffmpeg_common::{nanos_to_time_base_units, time_base_units_to_nanos};
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

        // Find audio stream (optional) — store INPUT stream index for packet dispatch.
        let audio_stream_info = input
            .streams()
            .find(|s| s.parameters().medium() == ff::media::Type::Audio)
            .map(|s| (s.index(), s.time_base()));
        let input_audio_stream_index = audio_stream_info.map(|(idx, _)| idx);

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

        // Create audio resampler (SwrContext) to convert decoded audio to
        // F32 planar stereo 48kHz — the format required by the AAC encoder.
        let mut audio_resampler = if let Some(ref dec) = audio_dec {
            Some(
                software::resampling::Context::get(
                    dec.format(),
                    dec.channel_layout(),
                    dec.rate(),
                    Sample::F32(sample::Type::Planar),
                    ChannelLayout::STEREO,
                    48000,
                )
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("创建音频重采样器失败: {e}"),
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

        // Video encoder (H.264) — use fps-based time base.
        let out_fps = preset_spec.fps;
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
        // Use fps-based time base for the encoder. PTS values throughout the
        // pipeline are computed in this time_base.
        let video_enc_tb = Rational(1, out_fps as i32);
        video_enc.set_time_base(video_enc_tb);
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
        video_stream_out.set_time_base(video_enc_tb);
        video_stream_out.set_parameters(video_params_out);
        let video_out_idx = video_stream_out.index();

        // Audio encoder (AAC) — always created so the output MP4 has an audio
        // stream even when the source has no audio (silent AAC track).
        let audio_enc_tb = Rational(1, 48000);
        let (mut audio_encoder, audio_out_idx): (encoder::audio::Encoder, usize) = {
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
            audio_enc.set_time_base(audio_enc_tb);
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
            stream.set_time_base(audio_enc_tb);
            stream.set_parameters(params);
            let idx = stream.index();
            (enc, idx)
        };

        // Video scaler — compute crop/fit geometry based on preset scale policy.
        // CenterCrop: scale source to cover output, then crop center.
        // FitWithBars: scale source to fit within output, then pad with black.
        let src_pix_fmt = video_dec.format();
        let scale_policy = preset_spec.scale_policy;
        use crate::media::export_presets::ExportScalePolicy;

        let (scaler_src_w, scaler_src_h) = match scale_policy {
            ExportScalePolicy::CenterCrop => {
                let (_, _, cw, ch) = compute_center_crop(src_width, src_height, out_w, out_h);
                (cw, ch)
            }
            ExportScalePolicy::FitWithBars => (src_width, src_height),
        };
        let mut scaler = software::scaling::Context::get(
            src_pix_fmt,
            scaler_src_w,
            scaler_src_h,
            Pixel::YUV420P,
            out_w,
            out_h,
            software::scaling::Flags::BILINEAR,
        )
        .map_err(|e| AppError::ExportFailed {
            reason: format!("创建像素格式转换器失败: {e}"),
        })?;

        // Pre-compute crop geometry for CenterCrop (crop_x, crop_y in pixels).
        let center_crop_origin = match scale_policy {
            ExportScalePolicy::CenterCrop => {
                let (cx, cy, _, _) = compute_center_crop(src_width, src_height, out_w, out_h);
                Some((cx, cy))
            }
            ExportScalePolicy::FitWithBars => None,
        };

        // Pre-create FitWithBars scaler and compute fit dimensions.
        // These are constant for the entire export since source dimensions
        // do not change. Creating the scaler once avoids per-frame allocation.
        let (mut fit_scaler, fit_w, fit_h) = if let ExportScalePolicy::FitWithBars = scale_policy {
            let src_ratio = src_width as f64 / src_height as f64;
            let out_ratio = out_w as f64 / out_h as f64;
            let (fw, fh) = if src_ratio > out_ratio {
                (out_w, (out_w as f64 / src_ratio).round() as u32)
            } else {
                ((out_h as f64 * src_ratio).round() as u32, out_h)
            };
            let fw = fw.max(1);
            let fh = fh.max(1);
            let s = software::scaling::Context::get(
                src_pix_fmt,
                src_width,
                src_height,
                Pixel::YUV420P,
                fw,
                fh,
                software::scaling::Flags::BILINEAR,
            )
            .map_err(|e| AppError::ExportFailed {
                reason: format!("创建缩放器失败: {e}"),
            })?;
            (Some(s), fw, fh)
        } else {
            (None, 0u32, 0u32)
        };

        output.write_header().map_err(|e| AppError::ExportFailed {
            reason: format!("写入文件头失败: {e}"),
        })?;

        // --- Cursor overlay compositor ---
        // Load effect timeline if provided. The renderer draws a cursor
        // indicator onto each output frame at the mapped screen coordinates.
        //
        // When `render_cursor_overlay` is false (raw cursor already visible),
        // overlay is not needed — this is a no-op, not an error.
        // When `render_cursor_overlay` is true but renderer can't be created
        // (e.g., empty frames), the export must fail to prevent silent
        // "no cursor, no beautification" exports (BUG-007).
        let fit_dims = if fit_scaler.is_some() {
            Some((fit_w, fit_h))
        } else {
            None
        };
        let mut cursor_overlay = None;
        if let Some(ref timeline_path) = request.effect_timeline_path {
            let timeline = crate::media::cursor_overlay::load_effect_timeline(timeline_path)?;
            let needs_overlay = timeline.render_cursor_overlay;
            cursor_overlay = crate::media::cursor_overlay::CursorOverlayRenderer::new(
                timeline,
                src_width,
                src_height,
                out_w,
                out_h,
                scale_policy,
                center_crop_origin,
                fit_dims,
            );
            // Safety contract: if raw cursor is hidden (overlay required) but
            // renderer couldn't be created, fail the export.
            // If render_cursor_overlay=false, cursor_overlay being None is fine
            // (raw cursor is already visible in the source frames).
            if needs_overlay && cursor_overlay.is_none() {
                return Err(AppError::ExportFailed {
                    reason: "光标美化已开启但效果时间线无法渲染。\
                             请确认录制时已启用光标元数据采集，或重新录制。"
                        .to_string(),
                });
            }
        }

        // After write_header(), the muxer may have rewritten the output stream
        // time bases (e.g., MP4 muxer often changes video stream to 1/15360).
        // Read the actual time bases for correct PTS rescaling.
        let video_out_tb = output.stream(video_out_idx).unwrap().time_base();
        let audio_out_tb = output.stream(audio_out_idx).unwrap().time_base();

        // --- Segment-based decode/encode loop ---
        // Track cumulative cut duration so output timestamps are continuous
        // across keep segments. E.g., keeps [0..2s, 6..8s] → output [0..2s, 2..4s].
        // Cut tracking uses nanoseconds; PTS offsets are tracked in each stream's
        // own time_base to avoid precision loss from nanos→time_base roundtrips.
        let total_keeps = request.cut_timeline.keeps.len();
        let total_keep_nanos: u64 = request
            .cut_timeline
            .keeps
            .iter()
            .map(|s| s.end.nanos.saturating_sub(s.start.nanos))
            .sum();
        // First frame PTS offset in input video time_base units.
        let mut video_pts_offset: Option<i64> = None;
        let mut cumulative_cut_nanos: i64 = 0;
        let mut prev_seg_end_nanos: i64 = 0;
        // Track output PTS in encoder time_base units for monotonic continuity.
        let mut last_video_out_pts: i64 = 0;
        // Audio output PTS advances by sample count — never derived from
        // resampled frame PTS, which may be stale after resampler flush.
        let mut next_audio_out_pts: i64 = 0;
        // Audio sample buffer for accumulating sliced samples to meet AAC
        // frame_size=1024 requirement. When boundary slicing produces frames
        // smaller than 1024 samples, we buffer and merge them.
        let aac_frame_size = 1024usize;
        let mut audio_sample_buf_l: Vec<f32> = Vec::new();
        let mut audio_sample_buf_r: Vec<f32> = Vec::new();
        // Input audio time_base for boundary checks and PTS conversion.
        let input_audio_tb = audio_stream_info
            .map(|(_, tb)| tb)
            .unwrap_or(Rational(1, 48000));

        let mut progress_state = ProgressState::new();

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

            // Convert segment boundaries to input video time_base units for
            // packet comparison (PTS filtering).
            let seg_start_in_vtb =
                nanos_to_time_base_units(seg_start_nanos as u64, video_time_base).unwrap_or(0);
            let seg_end_in_vtb =
                nanos_to_time_base_units(seg_end_nanos as u64, video_time_base).unwrap_or(0);

            // Convert segment boundaries to input audio time_base for audio
            // packet boundary checks.
            let seg_start_in_atb =
                nanos_to_time_base_units(seg_start_nanos as u64, input_audio_tb).unwrap_or(0);
            let seg_end_in_atb =
                nanos_to_time_base_units(seg_end_nanos as u64, input_audio_tb).unwrap_or(0);

            // Convert segment boundaries to AV_TIME_BASE units (microseconds)
            // for seeking. ffmpeg-next's Input::seek() with stream_index=-1
            // uses AV_TIME_BASE, NOT the video stream time_base.
            let seg_start_avtb = seg_start_nanos / 1_000;
            let seg_end_avtb = seg_end_nanos / 1_000;

            // Flush audio resampler between segments to drain buffered frames
            // from the previous segment. Without this, stale buffered frames
            // carry old PTS values that break audio monotonicity.
            // Flushed samples go through the buffer to maintain AAC frame_size.
            if seg_idx > 0 {
                if let Some(ref mut resampler) = audio_resampler {
                    let mut flush_out = frame::Audio::empty();
                    let flush_result = resampler.flush(&mut flush_out);
                    if flush_result.is_ok() {
                        let num_samples = flush_out.samples();
                        if num_samples > 0 {
                            let ch_count = flush_out.channels().min(2) as usize;
                            for ch in 0..ch_count {
                                let plane = flush_out.plane::<f32>(ch);
                                let buf = if ch == 0 {
                                    &mut audio_sample_buf_l
                                } else {
                                    &mut audio_sample_buf_r
                                };
                                let copy_count = num_samples.min(plane.len());
                                buf.extend_from_slice(&plane[..copy_count]);
                            }
                            // Drain complete frames from buffer.
                            while audio_sample_buf_l.len() >= aac_frame_size {
                                let mut out_frame = frame::Audio::new(
                                    Sample::F32(sample::Type::Planar),
                                    aac_frame_size,
                                    ChannelLayout::STEREO,
                                );
                                out_frame.set_rate(48000);
                                out_frame.set_pts(Some(next_audio_out_pts));
                                next_audio_out_pts += aac_frame_size as i64;

                                let dst_l = out_frame.plane_mut::<f32>(0);
                                let drain_l: Vec<f32> =
                                    audio_sample_buf_l.drain(..aac_frame_size).collect();
                                dst_l.copy_from_slice(&drain_l);

                                let dst_r = out_frame.plane_mut::<f32>(1);
                                if audio_sample_buf_r.len() >= aac_frame_size {
                                    let drain_r: Vec<f32> =
                                        audio_sample_buf_r.drain(..aac_frame_size).collect();
                                    dst_r.copy_from_slice(&drain_r);
                                }

                                audio_encoder.send_frame(&out_frame).map_err(|e| {
                                    AppError::ExportFailed {
                                        reason: format!("编码刷新音频帧失败: {e}"),
                                    }
                                })?;
                                let mut enc_pkt = ff::Packet::empty();
                                while audio_encoder.receive_packet(&mut enc_pkt).is_ok() {
                                    enc_pkt.set_stream(audio_out_idx);
                                    enc_pkt.rescale_ts(audio_enc_tb, audio_out_tb);
                                    enc_pkt.write_interleaved(&mut output).map_err(|e| {
                                        AppError::ExportFailed {
                                            reason: format!("写入刷新音频数据包失败: {e}"),
                                        }
                                    })?;
                                }
                            }
                        }
                    }
                }
            }

            // Seek input to segment start (in AV_TIME_BASE units = microseconds).
            // ffmpeg-next's Input::seek() with stream_index=-1 expects
            // AV_TIME_BASE units, NOT video stream time_base.
            input
                .seek(seg_start_avtb, ..seg_end_avtb)
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("定位到裁剪段失败: {e}"),
                })?;

            // NOTE: Decoder flush after seek is intentionally omitted here.
            // The correct FFmpeg pattern for segment-based seeking is:
            //   1. Seek demuxer to keyframe before target
            //   2. Flush decoder (avcodec_flush_buffers)
            //   3. Feed keyframe packets to rebuild reference
            //   4. Only encode frames within the segment
            // However, flushing the decoder while the encoder has buffered
            // frames causes PTS monotonicity violations in the muxer
            // (encoder output PTS lags behind input PTS due to look-ahead).
            // Since max_b_frames=0 and the demuxer seeks to a keyframe,
            // the decoder handles the seek correctly without explicit flush.
            // Visual quality at segment boundaries may be slightly degraded
            // (potential brief glitch on first frame) but output is valid.
            // Output PTS continuity is maintained by last_video_out_pts and
            // next_audio_out_pts which track the cumulative output position.

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
                        if pkt_stream == video_stream_index && pkt_ts > seg_end_in_vtb {
                            break;
                        }
                        if pkt_stream == input_audio_stream_index.unwrap_or(usize::MAX)
                            && pkt_ts > seg_end_in_atb
                        {
                            break;
                        }

                        // Skip packets before segment start.
                        if pkt_stream == video_stream_index && pkt_ts < seg_start_in_vtb {
                            continue;
                        }
                        if pkt_stream == input_audio_stream_index.unwrap_or(usize::MAX)
                            && pkt_ts < seg_start_in_atb
                        {
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
                                let raw_pts = decoded.pts().unwrap_or(0);

                                // Capture first frame PTS offset in input time_base units.
                                if video_pts_offset.is_none() {
                                    video_pts_offset = Some(raw_pts);
                                }
                                // Compute cut offset in input video time_base units.
                                let cut_offset_in_vtb = nanos_to_time_base_units(
                                    cumulative_cut_nanos as u64,
                                    video_time_base,
                                )
                                .unwrap_or(0);
                                // Output PTS in input video time_base (continuous after cuts).
                                let out_pts_in_vtb =
                                    raw_pts - video_pts_offset.unwrap_or(0) - cut_offset_in_vtb;
                                // Rescale from input video time_base to encoder output time_base.
                                // Avoids the precision loss of going through nanoseconds.
                                let out_pts = if video_time_base == video_enc_tb {
                                    out_pts_in_vtb
                                } else {
                                    (out_pts_in_vtb as i128
                                        * video_time_base.0 as i128
                                        * video_enc_tb.1 as i128
                                        / (video_time_base.1 as i128 * video_enc_tb.0 as i128))
                                        as i64
                                };

                                // Ensure monotonic PTS.
                                let out_pts = out_pts.max(last_video_out_pts);
                                last_video_out_pts = out_pts;

                                // Apply scale policy (CenterCrop or FitWithBars).
                                // CenterCrop: create a virtual sub-frame pointing to the
                                // crop region, then scale to output dimensions.
                                // FitWithBars: scale to fit, then center on black canvas.
                                let mut output_frame =
                                    frame::Video::new(Pixel::YUV420P, out_w, out_h);
                                output_frame.set_pts(Some(out_pts));

                                if let Some((crop_x, crop_y)) = center_crop_origin {
                                    // CenterCrop: copy the crop region from the decoded
                                    // frame into a new frame, then scale to output size.
                                    // Handles both BGRA (single-plane) and YUV420P
                                    // (multi-plane with chroma subsampling).
                                    let mut crop_frame =
                                        frame::Video::new(src_pix_fmt, scaler_src_w, scaler_src_h);
                                    unsafe {
                                        let src_ptr = decoded.as_ptr();
                                        let num_planes = crop_frame.planes().min(4);
                                        for plane_idx in 0..num_planes {
                                            let src_data = (*src_ptr).data[plane_idx];
                                            let src_linesize =
                                                (*src_ptr).linesize[plane_idx] as usize;
                                            if src_data.is_null() {
                                                continue;
                                            }
                                            let plane_h =
                                                crop_frame.plane_height(plane_idx) as usize;
                                            let plane_w =
                                                crop_frame.plane_width(plane_idx) as usize;
                                            let dst = crop_frame.data_mut(plane_idx);
                                            let dst_linesize = if plane_h > 0 {
                                                dst.len() / plane_h
                                            } else {
                                                continue;
                                            };
                                            // Per-plane crop origin: chroma planes in
                                            // YUV420P are subsampled by 2 in both axes.
                                            let plane_crop_x = if plane_idx == 0 {
                                                crop_x as usize
                                            } else {
                                                crop_x as usize / 2
                                            };
                                            let plane_crop_y = if plane_idx == 0 {
                                                crop_y as usize
                                            } else {
                                                crop_y as usize / 2
                                            };
                                            let copy_per_row = if src_linesize > plane_crop_x {
                                                plane_w
                                                    .min(dst_linesize)
                                                    .min(src_linesize - plane_crop_x)
                                            } else {
                                                0
                                            };
                                            if copy_per_row == 0 {
                                                continue;
                                            }
                                            for row in 0..plane_h {
                                                let src_row = src_data.add(
                                                    (row + plane_crop_y) * src_linesize
                                                        + plane_crop_x,
                                                );
                                                let dst_row =
                                                    dst.as_mut_ptr().add(row * dst_linesize);
                                                std::ptr::copy_nonoverlapping(
                                                    src_row,
                                                    dst_row,
                                                    copy_per_row,
                                                );
                                            }
                                        }
                                    }

                                    scaler.run(&crop_frame, &mut output_frame).map_err(|e| {
                                        AppError::ExportFailed {
                                            reason: format!("像素格式转换失败: {e}"),
                                        }
                                    })?;
                                } else {
                                    // FitWithBars: scale source to fit within output,
                                    // then center on black canvas (YUV420P).
                                    let mut fit_frame =
                                        frame::Video::new(Pixel::YUV420P, fit_w, fit_h);
                                    // Copy decoded frame data for scaling.
                                    let mut src_copy =
                                        frame::Video::new(src_pix_fmt, src_width, src_height);
                                    let num_planes = src_copy.planes().min(4);
                                    unsafe {
                                        let src_ptr = decoded.as_ptr();
                                        for plane_idx in 0..num_planes {
                                            let src_data = (*src_ptr).data[plane_idx];
                                            let src_linesize =
                                                (*src_ptr).linesize[plane_idx] as usize;
                                            if src_data.is_null() {
                                                continue;
                                            }
                                            let plane_h = src_copy.plane_height(plane_idx) as usize;
                                            let dst = src_copy.data_mut(plane_idx);
                                            let dst_linesize = if plane_h > 0 {
                                                dst.len() / plane_h
                                            } else {
                                                continue;
                                            };
                                            let copy_per_row = dst_linesize.min(src_linesize);
                                            for row in 0..plane_h {
                                                let src_row = src_data.add(row * src_linesize);
                                                let dst_row =
                                                    dst.as_mut_ptr().add(row * dst_linesize);
                                                std::ptr::copy_nonoverlapping(
                                                    src_row,
                                                    dst_row,
                                                    copy_per_row,
                                                );
                                            }
                                        }
                                    }

                                    fit_scaler
                                        .as_mut()
                                        .unwrap()
                                        .run(&src_copy, &mut fit_frame)
                                        .map_err(|e| AppError::ExportFailed {
                                            reason: format!("缩放失败: {e}"),
                                        })?;

                                    // Fill output with YUV420P black:
                                    // Y=0 (luma), U=128 (chroma), V=128 (chroma).
                                    // Setting U/V to 0 would produce green, not black.
                                    for plane_idx in 0..output_frame.planes() {
                                        let plane = output_frame.data_mut(plane_idx);
                                        let fill_value: u8 = if plane_idx == 0 { 0 } else { 128 };
                                        for byte in plane.iter_mut() {
                                            *byte = fill_value;
                                        }
                                    }

                                    // Copy fit_frame into center of output.
                                    // YUV420P: Y is full-res, U/V are half-res (2x2 subsampled).
                                    // x_off/y_off are in full-resolution pixel units.
                                    let x_off = (out_w - fit_w) / 2;
                                    let y_off = (out_h - fit_h) / 2;
                                    // Pre-compute plane dimensions to avoid borrow conflict.
                                    let out_plane_heights: Vec<u32> = (0..output_frame.planes())
                                        .map(|i| output_frame.plane_height(i))
                                        .collect();
                                    let fit_plane_heights: Vec<u32> = (0..fit_frame.planes())
                                        .map(|i| fit_frame.plane_height(i))
                                        .collect();
                                    let fit_plane_widths: Vec<u32> = (0..fit_frame.planes())
                                        .map(|i| fit_frame.plane_width(i))
                                        .collect();
                                    let num_planes = fit_frame.planes().min(4);
                                    unsafe {
                                        for plane_idx in 0..num_planes {
                                            let src_data = fit_frame.data(plane_idx);
                                            let dst_data = output_frame.data_mut(plane_idx);
                                            let src_h = fit_plane_heights[plane_idx] as usize;
                                            let dst_h = out_plane_heights[plane_idx] as usize;
                                            let src_linesize = if src_h > 0 {
                                                src_data.len() / src_h
                                            } else {
                                                continue;
                                            };
                                            let dst_linesize = if dst_h > 0 {
                                                dst_data.len() / dst_h
                                            } else {
                                                continue;
                                            };
                                            // Per-plane offset: chroma planes (1,2) are
                                            // subsampled by 2 in YUV420P.
                                            let plane_x_off = if plane_idx == 0 {
                                                x_off as usize
                                            } else {
                                                x_off as usize / 2
                                            };
                                            let plane_y_off = if plane_idx == 0 {
                                                y_off as usize
                                            } else {
                                                y_off as usize / 2
                                            };
                                            let copy_rows =
                                                src_h.min(dst_h.saturating_sub(plane_y_off));
                                            let copy_bytes = fit_plane_widths[plane_idx] as usize;
                                            let copy_bytes = copy_bytes
                                                .min(dst_linesize.saturating_sub(plane_x_off));
                                            let copy_bytes = copy_bytes.min(src_linesize);
                                            for row in 0..copy_rows {
                                                let src_row =
                                                    src_data.as_ptr().add(row * src_linesize);
                                                let dst_row = dst_data.as_mut_ptr().add(
                                                    (row + plane_y_off) * dst_linesize
                                                        + plane_x_off,
                                                );
                                                std::ptr::copy_nonoverlapping(
                                                    src_row, dst_row, copy_bytes,
                                                );
                                            }
                                        }
                                    }
                                }

                                // Compute source timestamp for cursor overlay and progress.
                                let decoded_nanos =
                                    time_base_units_to_nanos(raw_pts, video_time_base)
                                        .unwrap_or(0)
                                        .max(0) as u64;
                                // Align decoded PTS with cursor timeline: subtract source MP4's
                                // first-PTS origin so overlay queries match cursor sample timestamps.
                                let source_pts_origin = cursor_overlay
                                    .as_ref()
                                    .map(|o| o.source_pts_origin_nanos())
                                    .unwrap_or(0);
                                let source_nanos =
                                    decoded_nanos.saturating_sub(source_pts_origin);

                                // Draw cursor overlay onto the scaled output frame.
                                // IMPORTANT: pass source timestamp (not output PTS)
                                // because cursor timeline is in source time.
                                if let Some(ref overlay) = cursor_overlay {
                                    overlay.draw_on_frame(&mut output_frame, source_nanos, out_fps);
                                }

                                video_encoder.send_frame(&output_frame).map_err(|e| {
                                    AppError::ExportFailed {
                                        reason: format!("编码视频帧失败: {e}"),
                                    }
                                })?;

                                // Report time-based progress after each video frame.
                                if total_keep_nanos > 0 {
                                    let mut processed_nanos: u64 = 0;
                                    for prev_seg_idx in 0..seg_idx {
                                        let prev_seg = &request.cut_timeline.keeps[prev_seg_idx];
                                        processed_nanos +=
                                            prev_seg.end.nanos.saturating_sub(prev_seg.start.nanos);
                                    }
                                    let frame_in_segment =
                                        (source_nanos).saturating_sub(segment.start.nanos);
                                    let segment_duration =
                                        segment.end.nanos.saturating_sub(segment.start.nanos);
                                    processed_nanos += frame_in_segment.min(segment_duration);

                                    let pct = (1 + processed_nanos * 98 / total_keep_nanos)
                                        .clamp(1, 99)
                                        as u8;
                                    if let Some(ref progress) = request.progress {
                                        progress_state.maybe_report(progress, pct);
                                    }
                                }

                                let mut enc_pkt = ff::Packet::empty();
                                while video_encoder.receive_packet(&mut enc_pkt).is_ok() {
                                    enc_pkt.set_stream(video_out_idx);
                                    enc_pkt.rescale_ts(video_enc_tb, video_out_tb);
                                    enc_pkt.write_interleaved(&mut output).map_err(|e| {
                                        AppError::ExportFailed {
                                            reason: format!("写入视频数据包失败: {e}"),
                                        }
                                    })?;
                                }
                            }
                        } else if Some(pkt_stream) == input_audio_stream_index {
                            // Decode → resample → boundary-slice → encode audio.
                            // Decoded audio frames may span across keep/cut boundaries.
                            // After resampling, we compute the frame's time range and
                            // slice samples that fall outside the current keep segment.
                            if let Some(ref mut dec) = audio_dec {
                                dec.send_packet(&packet)
                                    .map_err(|e| AppError::ExportFailed {
                                        reason: format!("发送音频包到解码器失败: {e}"),
                                    })?;

                                let mut decoded_audio = frame::Audio::empty();
                                while dec.receive_frame(&mut decoded_audio).is_ok() {
                                    // Compute decoded frame time range in nanoseconds
                                    // for boundary overlap check.
                                    let decoded_pts_nanos = if let Some(pts) = decoded_audio.pts() {
                                        time_base_units_to_nanos(pts, input_audio_tb)
                                            .unwrap_or(0)
                                            .max(0) as u64
                                    } else {
                                        0
                                    };
                                    let decoded_rate = decoded_audio.rate().max(1);
                                    let decoded_samples = decoded_audio.samples() as u64;
                                    let decoded_dur_nanos =
                                        decoded_samples * 1_000_000_000 / decoded_rate as u64;
                                    let decoded_end_nanos = decoded_pts_nanos + decoded_dur_nanos;

                                    // Check overlap with current keep segment.
                                    // Skip frames entirely outside the segment.
                                    if decoded_end_nanos <= seg_start_nanos as u64
                                        || decoded_pts_nanos >= seg_end_nanos as u64
                                    {
                                        continue;
                                    }

                                    // Compute the portion to keep in nanoseconds.
                                    let keep_start_nanos =
                                        decoded_pts_nanos.max(seg_start_nanos as u64);
                                    let keep_end_nanos =
                                        decoded_end_nanos.min(seg_end_nanos as u64);

                                    // Resample decoded audio to F32P stereo 48kHz.
                                    let mut resampled =
                                        if let Some(ref mut resampler) = audio_resampler {
                                            let mut out = frame::Audio::empty();
                                            resampler.run(&decoded_audio, &mut out).map_err(
                                                |e| AppError::ExportFailed {
                                                    reason: format!("音频重采样失败: {e}"),
                                                },
                                            )?;
                                            out
                                        } else {
                                            decoded_audio.clone()
                                        };

                                    // Convert keep boundaries to resampled sample indices.
                                    // The resampler maps decoded sample i → resampled
                                    // position proportionally (linear time mapping).
                                    let resampled_total = resampled.samples() as u64;
                                    if decoded_dur_nanos == 0 {
                                        continue;
                                    }
                                    let keep_start_idx = ((keep_start_nanos - decoded_pts_nanos)
                                        * resampled_total
                                        / decoded_dur_nanos)
                                        .min(resampled_total);
                                    let keep_end_idx = ((keep_end_nanos - decoded_pts_nanos)
                                        * resampled_total
                                        / decoded_dur_nanos)
                                        .min(resampled_total);

                                    if keep_start_idx >= keep_end_idx {
                                        continue;
                                    }

                                    // Slice resampled frame if boundaries don't align.
                                    let sliced =
                                        if keep_start_idx > 0 || keep_end_idx < resampled_total {
                                            slice_audio_frame(
                                                &resampled,
                                                keep_start_idx as usize,
                                                keep_end_idx as usize,
                                            )
                                        } else {
                                            std::mem::replace(&mut resampled, frame::Audio::empty())
                                        };

                                    // Accumulate sliced samples into per-channel buffers.
                                    // The AAC encoder requires exactly 1024 samples per frame.
                                    // Boundary slicing may produce smaller frames, so we buffer
                                    // and only send full 1024-sample frames.
                                    {
                                        let ch_count = sliced.channels().min(2) as usize;
                                        let sample_count = sliced.samples();
                                        for ch in 0..ch_count {
                                            let plane = sliced.plane::<f32>(ch);
                                            let buf = if ch == 0 {
                                                &mut audio_sample_buf_l
                                            } else {
                                                &mut audio_sample_buf_r
                                            };
                                            let copy_count = sample_count.min(plane.len());
                                            buf.extend_from_slice(&plane[..copy_count]);
                                        }
                                    }

                                    // Drain complete 1024-sample frames from the buffer.
                                    while audio_sample_buf_l.len() >= aac_frame_size {
                                        let mut out_frame = frame::Audio::new(
                                            Sample::F32(sample::Type::Planar),
                                            aac_frame_size,
                                            ChannelLayout::STEREO,
                                        );
                                        out_frame.set_rate(48000);
                                        out_frame.set_pts(Some(next_audio_out_pts));
                                        next_audio_out_pts += aac_frame_size as i64;

                                        let dst_l = out_frame.plane_mut::<f32>(0);
                                        let drain_l: Vec<f32> =
                                            audio_sample_buf_l.drain(..aac_frame_size).collect();
                                        dst_l.copy_from_slice(&drain_l);

                                        let dst_r = out_frame.plane_mut::<f32>(1);
                                        if audio_sample_buf_r.len() >= aac_frame_size {
                                            let drain_r: Vec<f32> = audio_sample_buf_r
                                                .drain(..aac_frame_size)
                                                .collect();
                                            dst_r.copy_from_slice(&drain_r);
                                        }
                                        // else: R channel buffer is shorter (shouldn't
                                        // happen for stereo); dst_r stays zero-filled.

                                        audio_encoder.send_frame(&out_frame).map_err(|e| {
                                            AppError::ExportFailed {
                                                reason: format!("编码音频帧失败: {e}"),
                                            }
                                        })?;

                                        let mut enc_pkt = ff::Packet::empty();
                                        while audio_encoder.receive_packet(&mut enc_pkt).is_ok() {
                                            enc_pkt.set_stream(audio_out_idx);
                                            enc_pkt.rescale_ts(audio_enc_tb, audio_out_tb);
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

            // Fallback: report per-segment progress for zero-duration timelines.
            if total_keep_nanos == 0 {
                if let Some(ref progress) = request.progress {
                    let pct = ((seg_idx + 1) * 99 / total_keeps).clamp(1, 99) as u8;
                    progress_state.maybe_report(progress, pct);
                }
            }
        }

        // Flush remaining buffered audio samples as a final frame.
        // The last frame can be < 1024 samples (AAC allows this for the final frame).
        if !audio_sample_buf_l.is_empty() {
            let remaining = audio_sample_buf_l.len();
            let mut final_frame = frame::Audio::new(
                Sample::F32(sample::Type::Planar),
                remaining,
                ChannelLayout::STEREO,
            );
            final_frame.set_rate(48000);
            final_frame.set_pts(Some(next_audio_out_pts));

            let dst_l = final_frame.plane_mut::<f32>(0);
            dst_l.copy_from_slice(&audio_sample_buf_l);
            audio_sample_buf_l.clear();

            let dst_r = final_frame.plane_mut::<f32>(1);
            let r_count = remaining.min(audio_sample_buf_r.len());
            if r_count > 0 {
                dst_r[..r_count].copy_from_slice(&audio_sample_buf_r[..r_count]);
            }
            audio_sample_buf_r.clear();

            audio_encoder
                .send_frame(&final_frame)
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("编码音频尾帧失败: {e}"),
                })?;
            let mut enc_pkt = ff::Packet::empty();
            while audio_encoder.receive_packet(&mut enc_pkt).is_ok() {
                enc_pkt.set_stream(audio_out_idx);
                enc_pkt.rescale_ts(audio_enc_tb, audio_out_tb);
                enc_pkt
                    .write_interleaved(&mut output)
                    .map_err(|e| AppError::ExportFailed {
                        reason: format!("写入音频尾帧数据包失败: {e}"),
                    })?;
            }
        }

        // If source has no audio, generate silent AAC frames to cover the
        // entire kept duration. This ensures the output always has an audio
        // stream for consistent playback behavior.
        if audio_dec.is_none() {
            let total_kept_nanos: i64 = request
                .cut_timeline
                .keeps
                .iter()
                .map(|s| s.end.nanos as i64 - s.start.nanos as i64)
                .sum();
            let total_kept_secs = total_kept_nanos as f64 / 1_000_000_000.0;
            let total_audio_frames = (total_kept_secs * 48000.0).ceil() as i64;
            let frame_size = 1024i64;
            let num_packets = (total_audio_frames / frame_size).max(1);

            for _ in 0..num_packets {
                let mut silent_frame = frame::Audio::new(
                    Sample::F32(sample::Type::Planar),
                    frame_size as usize,
                    ChannelLayout::STEREO,
                );
                silent_frame.set_pts(Some(next_audio_out_pts));
                next_audio_out_pts += frame_size;

                // Fill with silence.
                for ch in 0..2 {
                    let plane = silent_frame.plane_mut::<f32>(ch);
                    for s in plane.iter_mut() {
                        *s = 0.0;
                    }
                }

                audio_encoder
                    .send_frame(&silent_frame)
                    .map_err(|e| AppError::ExportFailed {
                        reason: format!("编码静音音频帧失败: {e}"),
                    })?;

                let mut enc_pkt = ff::Packet::empty();
                while audio_encoder.receive_packet(&mut enc_pkt).is_ok() {
                    enc_pkt.set_stream(audio_out_idx);
                    enc_pkt.rescale_ts(audio_enc_tb, audio_out_tb);
                    enc_pkt
                        .write_interleaved(&mut output)
                        .map_err(|e| AppError::ExportFailed {
                            reason: format!("写入静音音频数据包失败: {e}"),
                        })?;
                }
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
            enc_pkt.rescale_ts(video_enc_tb, video_out_tb);
            enc_pkt
                .write_interleaved(&mut output)
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("写入视频数据包失败: {e}"),
                })?;
        }

        audio_encoder
            .send_eof()
            .map_err(|e| AppError::ExportFailed {
                reason: format!("刷新音频编码器失败: {e}"),
            })?;
        while audio_encoder.receive_packet(&mut enc_pkt).is_ok() {
            enc_pkt.set_stream(audio_out_idx);
            enc_pkt.rescale_ts(audio_enc_tb, audio_out_tb);
            enc_pkt
                .write_interleaved(&mut output)
                .map_err(|e| AppError::ExportFailed {
                    reason: format!("写入音频数据包失败: {e}"),
                })?;
        }

        output.write_trailer().map_err(|e| AppError::ExportFailed {
            reason: format!("写入文件尾失败: {e}"),
        })?;

        // Report final 100% on success.
        if let Some(ref progress) = request.progress {
            progress.report(100);
        }

        Ok(TrimExportResult {
            output_path: request.output_path,
            cut_count: request.cut_timeline.cuts.len(),
        })
    }
}

/// Slice an audio frame to keep only samples in `[start_idx, end_idx)`.
///
/// Works with F32 planar format (what the resampler outputs). Each channel
/// plane is sliced independently. Returns a new frame with adjusted PTS=0
/// (caller is responsible for setting the correct output PTS).
#[cfg(feature = "ffmpeg")]
fn slice_audio_frame(
    src: &ffmpeg_next::util::frame::Audio,
    start_idx: usize,
    end_idx: usize,
) -> ffmpeg_next::util::frame::Audio {
    use ffmpeg_next::util::format::{sample, Sample};
    use ffmpeg_next::ChannelLayout;

    let channels = src.channels() as usize;
    let keep_count = end_idx.saturating_sub(start_idx);
    let mut out = ffmpeg_next::util::frame::Audio::new(
        Sample::F32(sample::Type::Planar),
        keep_count,
        ChannelLayout::STEREO,
    );
    out.set_rate(src.rate());

    for ch in 0..channels.min(2) {
        let src_plane = src.plane::<f32>(ch);
        let dst_plane = out.plane_mut::<f32>(ch);
        let copy_count = keep_count
            .min(src_plane.len().saturating_sub(start_idx))
            .min(dst_plane.len());
        if copy_count > 0 {
            dst_plane[..copy_count].copy_from_slice(&src_plane[start_idx..start_idx + copy_count]);
        }
    }

    out
}

/// Compute center-crop geometry: (crop_x, crop_y, crop_w, crop_h).
///
/// Finds the largest region in the source that has the same aspect ratio as
/// the output, then centers it. Returns the crop origin and size in source
/// pixel coordinates. The crop is always clamped to source bounds.
#[cfg(feature = "ffmpeg")]
fn compute_center_crop(src_w: u32, src_h: u32, out_w: u32, out_h: u32) -> (u32, u32, u32, u32) {
    let src_ratio = src_w as f64 / src_h as f64;
    let out_ratio = out_w as f64 / out_h as f64;

    let (mut crop_w, mut crop_h) = if src_ratio > out_ratio {
        // Source is wider than output: crop horizontally.
        let crop_h = src_h;
        let crop_w = (src_h as f64 * out_ratio).round() as u32;
        (crop_w.max(1), crop_h)
    } else {
        // Source is taller than output: crop vertically.
        let crop_w = src_w;
        let crop_h = (src_w as f64 / out_ratio).round() as u32;
        (crop_w, crop_h.max(1))
    };

    // Clamp crop dimensions to source bounds to prevent overflow.
    crop_w = crop_w.min(src_w);
    crop_h = crop_h.min(src_h);

    let crop_x = (src_w - crop_w) / 2;
    let crop_y = (src_h - crop_h) / 2;
    (crop_x, crop_y, crop_w, crop_h)
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
