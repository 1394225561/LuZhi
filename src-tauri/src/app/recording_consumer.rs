use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::core::capture::DenoiseMode;
use crate::core::frame::{AudioChunk, VideoFrameRef};
use crate::core::media_channel::MediaReceiver;
use crate::media::audio_mixer::SimpleAudioMixer;
use crate::media::recording_writer::{
    RecordingDiagnostics, RecordingResult, RecordingWriter, WriterDiagnostics,
};
use crate::media::trim_metadata::{TrimMetadata, TRIM_METADATA_SCHEMA_VERSION};

/// Return type produced by the recording consumer thread.
///
/// This struct intentionally mirrors the previous macOS-private contract.
/// Keep field semantics unchanged so macOS finalize behavior is preserved.
pub struct RecordingConsumerOutput {
    pub result: RecordingResult,
    pub trim_metadata: TrimMetadata,
    pub diagnostics: RecordingDiagnostics,
    pub errors: Vec<String>,
}

pub struct RecordingConsumerInput {
    pub stop_flag: Arc<AtomicBool>,
    pub pause_flag: Arc<AtomicBool>,
    pub video_rx: MediaReceiver<VideoFrameRef>,
    pub system_audio_rx: MediaReceiver<AudioChunk>,
    pub mic_rx: Option<MediaReceiver<AudioChunk>>,
    pub frame_count: Arc<AtomicU64>,
    pub writer: Box<dyn RecordingWriter>,
    pub mic_level: Arc<Mutex<f64>>,
    pub trim_sensitivity: String,
    pub requested_system_audio: bool,
    pub requested_microphone: bool,
    pub microphone_device: Option<String>,
    pub denoise_mode: DenoiseMode,
}

/// Pushes a base audio sample into the bounded Vec, dropping it if at capacity.
/// Returns `true` if the sample was pushed, `false` if dropped.
fn push_bounded_base_audio_sample(
    samples: &mut Vec<crate::media::trim_audio_activity::BaseAudioActivitySample>,
    sample: crate::media::trim_audio_activity::BaseAudioActivitySample,
    max: usize,
) -> bool {
    if samples.len() < max {
        samples.push(sample);
        true
    } else {
        false
    }
}

/// Pushes a visual sample into the bounded Vec, dropping it if at capacity.
/// Returns `true` if the sample was pushed, `false` if dropped.
fn push_bounded_visual_sample(
    samples: &mut Vec<crate::core::cut::FrameDiffSample>,
    sample: crate::core::cut::FrameDiffSample,
    max: usize,
) -> bool {
    if samples.len() < max {
        samples.push(sample);
        true
    } else {
        false
    }
}

/// Chooses the effective trim metadata duration: prefer writer duration when
/// available, otherwise fall back to the latest observed media timestamp.
fn choose_duration_nanos(writer_duration_nanos: u64, latest_observed_media_nanos: u64) -> u64 {
    if writer_duration_nanos > 0 {
        writer_duration_nanos
    } else {
        latest_observed_media_nanos
    }
}

/// Computes the RMS (root mean square) of a slice of f32 audio samples.
///
/// Returns 0.0 for empty slices.
fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

/// Source artifact audio contract for FFmpeg validation.
#[cfg(feature = "ffmpeg")]
fn source_artifact_audio_contract(
    requested_system_audio: bool,
    requested_microphone: bool,
    source_aware_verified: bool,
) -> crate::media::ffmpeg_common::RequestedAudioContract {
    crate::media::ffmpeg_common::RequestedAudioContract {
        requested_system_audio,
        requested_microphone,
        allow_silent_if_system_only: requested_system_audio && !requested_microphone,
        allow_quiet_when_source_verified: source_aware_verified,
        ..Default::default()
    }
}

pub fn consume_frames(input: RecordingConsumerInput) -> RecordingConsumerOutput {
    let RecordingConsumerInput {
        stop_flag,
        pause_flag,
        video_rx,
        system_audio_rx,
        mic_rx,
        frame_count,
        mut writer,
        mic_level,
        trim_sensitivity: _trim_sensitivity_str,
        requested_system_audio,
        requested_microphone,
        microphone_device,
        denoise_mode,
    } = input;

    let mut synchronizer = crate::media::audio_synchronizer::AudioSynchronizer::new(
        SimpleAudioMixer::new(denoise_mode),
        crate::media::audio_synchronizer::AudioSynchronizerConfig {
            requested_system_audio,
            requested_microphone,
            ..Default::default()
        },
    );
    let mut mic_detector = crate::media::mic_level::MicLevelDetector::new(4096);

    let mut diagnostics = RecordingDiagnostics {
        requested_system_audio,
        requested_microphone,
        microphone_device,
        ..Default::default()
    };

    let mut base_audio_analyzer =
        crate::media::trim_audio_activity::BaseAudioActivityAnalyzer::default();
    let frame_diff_analyzer = crate::media::silence_detector::FrameDiffAnalyzer::new(64, 36);
    const VISUAL_SAMPLE_INTERVAL_NANOS: u64 = 250_000_000;
    const MAX_VISUAL_SAMPLES: usize = 144_000;
    const MAX_AUDIO_SAMPLES: usize = 72_000;
    let mut base_audio_activity = Vec::new();
    let mut previous_sampled_frame: Option<crate::core::frame::VideoFrame> = None;
    let mut last_visual_sample_nanos: u64 = 0;
    let mut visual_activity = Vec::new();
    let mut visual_dropped: u64 = 0;
    let mut audio_dropped: u64 = 0;
    let mut latest_observed_media_nanos: u64 = 0;
    let mut errors: Vec<String> = Vec::new();

    loop {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        let paused = pause_flag.load(Ordering::Relaxed);

        const MAX_VIDEO_BATCH_PER_ITERATION: usize = 10;
        let mut video_batch_count = 0;
        while video_batch_count < MAX_VIDEO_BATCH_PER_ITERATION {
            match video_rx.try_recv() {
                Ok(frame) => {
                    video_batch_count += 1;
                    if paused {
                        continue;
                    }
                    frame_count.fetch_add(1, Ordering::Relaxed);

                    let frame_nanos = frame.timestamp.nanos;
                    latest_observed_media_nanos = latest_observed_media_nanos.max(frame_nanos);
                    if frame_nanos.saturating_sub(last_visual_sample_nanos)
                        >= VISUAL_SAMPLE_INTERVAL_NANOS
                    {
                        if let Some(ref previous) = previous_sampled_frame {
                            if let Ok(diff) = frame_diff_analyzer.diff_pair(previous, &frame) {
                                if !push_bounded_visual_sample(
                                    &mut visual_activity,
                                    diff,
                                    MAX_VISUAL_SAMPLES,
                                ) {
                                    visual_dropped += 1;
                                }
                            }
                        }
                        previous_sampled_frame = Some((*frame).clone());
                        last_visual_sample_nanos = frame_nanos;
                    }

                    if let Err(e) = writer.push_video(frame) {
                        let msg = format!("写入视频帧失败: {e}");
                        eprintln!("{msg}");
                        errors.push(msg);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }

        while let Ok(chunk) = system_audio_rx.try_recv() {
            if paused {
                continue;
            }
            diagnostics.system_chunks_received += 1;
            let rms = compute_rms(&chunk.samples);
            if rms > diagnostics.system_rms_max {
                diagnostics.system_rms_max = rms;
            }
            synchronizer.push_system(chunk);
        }

        if let Some(ref mic_rx) = mic_rx {
            while let Ok(chunk) = mic_rx.try_recv() {
                if paused {
                    if let Ok(mut guard) = mic_level.lock() {
                        *guard = 0.0;
                    }
                    continue;
                }
                diagnostics.mic_chunks_received += 1;
                let rms = compute_rms(&chunk.samples);
                if rms > diagnostics.mic_rms_max {
                    diagnostics.mic_rms_max = rms;
                }
                let level = mic_detector.push_samples(&chunk.samples);
                if let Ok(mut guard) = mic_level.lock() {
                    *guard = level;
                }
                synchronizer.push_mic(chunk);
            }
        }

        if !paused {
            for synced_result in synchronizer.drain_mixed() {
                match synced_result {
                    Ok(synced) => {
                        if synced.has_system {
                            diagnostics.system_windows_before_writer += 1;
                            diagnostics.system_frames_before_writer += synced.system_frames;
                            if synced.system_rms > diagnostics.system_rms_max_before_writer {
                                diagnostics.system_rms_max_before_writer = synced.system_rms;
                            }
                        }
                        if synced.has_mic {
                            diagnostics.mic_windows_before_writer += 1;
                            diagnostics.mic_frames_before_writer += synced.mic_frames;
                            if synced.mic_rms > diagnostics.mic_rms_max_before_writer {
                                diagnostics.mic_rms_max_before_writer = synced.mic_rms;
                            }
                        }
                        if synced.emitted_due_to_timeout {
                            diagnostics.source_timeout_window_count += 1;
                        }

                        let chunk_frames =
                            synced.mixed.samples.len() as u64 / synced.mixed.channels.max(1) as u64;
                        let chunk_nanos = chunk_frames.saturating_mul(1_000_000_000)
                            / synced.mixed.sample_rate.max(1) as u64;
                        latest_observed_media_nanos = latest_observed_media_nanos
                            .max(synced.mixed.timestamp.nanos.saturating_add(chunk_nanos));
                        let mixed_rms = compute_rms(&synced.mixed.samples);
                        if mixed_rms > diagnostics.mixed_rms_max {
                            diagnostics.mixed_rms_max = mixed_rms;
                        }
                        for sample in base_audio_analyzer.push_chunk(&synced.mixed) {
                            if !push_bounded_base_audio_sample(
                                &mut base_audio_activity,
                                sample,
                                MAX_AUDIO_SAMPLES,
                            ) {
                                audio_dropped += 1;
                            }
                        }
                        let has_system = synced.has_system;
                        let has_mic = synced.has_mic;
                        let system_frames = synced.system_frames;
                        let mic_frames = synced.mic_frames;

                        if let Err(e) = writer.push_audio(synced.mixed) {
                            diagnostics.writer_push_audio_failures += 1;
                            let msg = format!("写入混音音频失败: {e}");
                            eprintln!("{msg}");
                            errors.push(msg);
                        } else {
                            writer.record_source_contribution(
                                has_system,
                                has_mic,
                                system_frames,
                                mic_frames,
                            );
                            diagnostics.mixed_chunks_queued += 1;
                        }
                    }
                    Err(e) => eprintln!("音频混合失败: {e}"),
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let stopped_while_paused = pause_flag.load(Ordering::Relaxed);

    if stopped_while_paused {
        while video_rx.try_recv().is_ok() {}
        while system_audio_rx.try_recv().is_ok() {}
        if let Some(ref mic_rx) = mic_rx {
            while mic_rx.try_recv().is_ok() {}
        }
        if let Ok(mut guard) = mic_level.lock() {
            *guard = 0.0;
        }
    } else {
        while let Ok(frame) = video_rx.try_recv() {
            frame_count.fetch_add(1, Ordering::Relaxed);
            let frame_nanos = frame.timestamp.nanos;
            latest_observed_media_nanos = latest_observed_media_nanos.max(frame_nanos);
            if frame_nanos.saturating_sub(last_visual_sample_nanos) >= VISUAL_SAMPLE_INTERVAL_NANOS
            {
                if let Some(ref previous) = previous_sampled_frame {
                    if let Ok(diff) = frame_diff_analyzer.diff_pair(previous, &frame) {
                        if !push_bounded_visual_sample(
                            &mut visual_activity,
                            diff,
                            MAX_VISUAL_SAMPLES,
                        ) {
                            visual_dropped += 1;
                        }
                    }
                }
                previous_sampled_frame = Some((*frame).clone());
                last_visual_sample_nanos = frame_nanos;
            }
            if let Err(e) = writer.push_video(frame) {
                let msg = format!("写入视频帧失败: {e}");
                eprintln!("{msg}");
                errors.push(msg);
            }
        }

        while let Ok(chunk) = system_audio_rx.try_recv() {
            diagnostics.system_chunks_received += 1;
            let rms = compute_rms(&chunk.samples);
            if rms > diagnostics.system_rms_max {
                diagnostics.system_rms_max = rms;
            }
            synchronizer.push_system(chunk);
        }

        if let Some(ref mic_rx) = mic_rx {
            while let Ok(chunk) = mic_rx.try_recv() {
                diagnostics.mic_chunks_received += 1;
                let rms = compute_rms(&chunk.samples);
                if rms > diagnostics.mic_rms_max {
                    diagnostics.mic_rms_max = rms;
                }
                synchronizer.push_mic(chunk);
            }
        }
    }

    if let Some(sample) = base_audio_analyzer.flush() {
        if !push_bounded_base_audio_sample(&mut base_audio_activity, sample, MAX_AUDIO_SAMPLES) {
            audio_dropped += 1;
        }
    }

    let drain_results = synchronizer.drain_final();
    let (paired, sys_only, mic_only, timeout_windows) = synchronizer.diagnostics();
    diagnostics.paired_window_count = paired;
    diagnostics.system_only_window_count = sys_only;
    diagnostics.mic_only_window_count = mic_only;
    diagnostics.source_timeout_window_count = timeout_windows;
    for (synchronized, was_unpaired) in drain_results {
        if was_unpaired {
            if synchronized.has_system && !synchronized.has_mic {
                eprintln!("警告: 最终排空发现未配对的系统音频块");
            } else if synchronized.has_mic && !synchronized.has_system {
                eprintln!("警告: 最终排空发现未配对的麦克风音频块");
            }
        }

        if synchronized.has_system {
            diagnostics.system_windows_before_writer += 1;
            diagnostics.system_frames_before_writer += synchronized.system_frames;
            if synchronized.system_rms > diagnostics.system_rms_max_before_writer {
                diagnostics.system_rms_max_before_writer = synchronized.system_rms;
            }
        }
        if synchronized.has_mic {
            diagnostics.mic_windows_before_writer += 1;
            diagnostics.mic_frames_before_writer += synchronized.mic_frames;
            if synchronized.mic_rms > diagnostics.mic_rms_max_before_writer {
                diagnostics.mic_rms_max_before_writer = synchronized.mic_rms;
            }
        }
        if synchronized.emitted_due_to_timeout {
            diagnostics.source_timeout_window_count += 1;
        }

        let chunk_frames =
            synchronized.mixed.samples.len() as u64 / synchronized.mixed.channels.max(1) as u64;
        let chunk_nanos = chunk_frames.saturating_mul(1_000_000_000)
            / synchronized.mixed.sample_rate.max(1) as u64;
        latest_observed_media_nanos = latest_observed_media_nanos.max(
            synchronized
                .mixed
                .timestamp
                .nanos
                .saturating_add(chunk_nanos),
        );
        let mixed_rms = compute_rms(&synchronized.mixed.samples);
        if mixed_rms > diagnostics.mixed_rms_max {
            diagnostics.mixed_rms_max = mixed_rms;
        }
        for sample in base_audio_analyzer.push_chunk(&synchronized.mixed) {
            if !push_bounded_base_audio_sample(&mut base_audio_activity, sample, MAX_AUDIO_SAMPLES)
            {
                audio_dropped += 1;
            }
        }
        let has_system = synchronized.has_system;
        let has_mic = synchronized.has_mic;
        let system_frames = synchronized.system_frames;
        let mic_frames = synchronized.mic_frames;

        if let Err(e) = writer.push_audio(synchronized.mixed) {
            diagnostics.writer_push_audio_failures += 1;
            let msg = format!("写入混音音频失败: {e}");
            eprintln!("{msg}");
            errors.push(msg);
        } else {
            writer.record_source_contribution(has_system, has_mic, system_frames, mic_frames);
            diagnostics.mixed_chunks_queued += 1;
        }
    }

    let result = match writer.finish() {
        Ok(result) => result,
        Err(e) => {
            let msg = format!("录制写入器完成失败: {e}");
            eprintln!("{msg}");
            errors.push(msg);
            RecordingResult {
                duration_secs: 0,
                frame_count: 0,
                mixed_audio_chunk_count: 0,
                output_path: None,
                cursor_metadata_path: None,
                effect_timeline_path: None,
                trim_metadata_path: None,
                cut_timeline_path: None,
                writer_diagnostics: WriterDiagnostics::default(),
                diagnostics: RecordingDiagnostics::default(),
                finalization_errors: Vec::new(),
            }
        }
    };

    diagnostics.generated_silent_track = result.writer_diagnostics.generated_silent_track;
    diagnostics.system_chunks_dropped = system_audio_rx.dropped_count();
    if let Some(ref mic_rx) = mic_rx {
        diagnostics.mic_chunks_dropped = mic_rx.dropped_count();
    }

    eprintln!("录制音频诊断: {:?}", diagnostics);

    if requested_system_audio && diagnostics.system_chunks_received == 0 {
        let msg = "警告: 请求了系统音频但未收到任何音频块".to_string();
        eprintln!("{msg}");
        errors.push(msg);
    }
    if requested_microphone && diagnostics.mic_chunks_received == 0 {
        let msg = "警告: 请求了麦克风但未收到任何音频块".to_string();
        eprintln!("{msg}");
        errors.push(msg);
    }

    const AUDIO_DROP_RATIO_HARD_FAIL: f64 = 0.10;
    if requested_system_audio && diagnostics.system_chunks_dropped > 0 {
        let attempted = diagnostics
            .system_chunks_received
            .saturating_add(diagnostics.system_chunks_dropped)
            .max(1) as f64;
        let ratio = diagnostics.system_chunks_dropped as f64 / attempted;
        eprintln!(
            "警告: 系统音频通道丢弃了 {} 个音频块 (总计 {}，丢弃率 {:.1}%)",
            diagnostics.system_chunks_dropped,
            attempted as u64,
            ratio * 100.0,
        );
        if ratio > AUDIO_DROP_RATIO_HARD_FAIL {
            errors.push(format!(
                "系统音频丢弃率过高 ({:.1}% > {:.1}%)",
                ratio * 100.0,
                AUDIO_DROP_RATIO_HARD_FAIL * 100.0
            ));
        }
    }
    if requested_microphone && diagnostics.mic_chunks_dropped > 0 {
        let attempted = diagnostics
            .mic_chunks_received
            .saturating_add(diagnostics.mic_chunks_dropped)
            .max(1) as f64;
        let ratio = diagnostics.mic_chunks_dropped as f64 / attempted;
        eprintln!(
            "警告: 麦克风通道丢弃了 {} 个音频块 (总计 {}，丢弃率 {:.1}%)",
            diagnostics.mic_chunks_dropped,
            attempted as u64,
            ratio * 100.0,
        );
        if ratio > AUDIO_DROP_RATIO_HARD_FAIL {
            errors.push(format!(
                "麦克风音频丢弃率过高 ({:.1}% > {:.1}%)",
                ratio * 100.0,
                AUDIO_DROP_RATIO_HARD_FAIL * 100.0
            ));
        }
    }

    #[cfg(feature = "ffmpeg")]
    if let Some(ref output_path) = result.output_path {
        if requested_system_audio || requested_microphone {
            let source_aware_verified =
                match crate::media::recording_writer::validate_source_aware_audio_contract(
                    &diagnostics,
                    &result.writer_diagnostics,
                ) {
                    Ok(()) => true,
                    Err(e) => {
                        let msg = format!("source-aware audio contract 失败: {e}");
                        eprintln!("{msg}");
                        errors.push(msg);
                        false
                    }
                };

            let contract = source_artifact_audio_contract(
                requested_system_audio,
                requested_microphone,
                source_aware_verified,
            );
            match crate::media::ffmpeg_common::validate_source_artifact_with_audio_contract(
                std::path::Path::new(output_path),
                &contract,
            ) {
                Ok(inspection) => {
                    eprintln!(
                        "录制音频 contract 验证通过: RMS={:.6}, peak={:.6}, samples={}",
                        inspection.audio_rms.unwrap_or(0.0),
                        inspection.audio_peak.unwrap_or(0.0),
                        inspection.audio_sample_count.unwrap_or(0),
                    );
                }
                Err(e) => {
                    let msg = format!("录制音频 contract 验证失败: {e}");
                    eprintln!("{msg}");
                    errors.push(msg);
                }
            }
        }
    }

    let writer_duration_nanos = result.duration_secs.saturating_mul(1_000_000_000);
    let duration_nanos = choose_duration_nanos(writer_duration_nanos, latest_observed_media_nanos);

    let activity_truncated = audio_dropped > 0 || visual_dropped > 0;

    RecordingConsumerOutput {
        result,
        trim_metadata: TrimMetadata {
            schema_version: TRIM_METADATA_SCHEMA_VERSION,
            duration_nanos,
            base_audio_activity,
            audio_activity: Vec::new(),
            visual_activity,
            audio_activity_dropped_count: audio_dropped,
            visual_activity_dropped_count: visual_dropped,
            activity_truncated,
        },
        diagnostics,
        errors,
    }
}
