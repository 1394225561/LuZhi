pub mod app;
pub mod core;
pub mod media;
pub mod platform;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use app::events::{
    CursorEffectSummaryPayload, CutTimelineSummaryPayload, ExportProgressPayload,
    ExportSummaryPayload, LicenseStatusPayload, MicLevelPayload, PermissionPayload,
    PostProcessProgressPayload, RecordingStatusPayload,
};
use app::mic_level_runtime::MicLevelRuntime;
#[cfg(not(target_os = "macos"))]
use app::permission_service::{PermissionStatus, RecordingPermissions};
use app::recording_runtime::TickRuntime;
use app::state_machine::RecordingState;
use core::capture::AudioConfig;
use core::config::CaptureConfig;
use core::cut::{TrimConfig, TrimSensitivity};
use core::processor::{CursorProcessor, SilenceDetector};
use core::timeline::{BeautifyConfigSnapshot, EffectTimeline};
use media::cursor_engine::{ClickAnimationConfig, CursorEffectEngine};
use media::recording_metadata::{RecordingMetadata, RecordingMetadataWriter};
use media::recording_writer::RecordingResult;
use media::silence_detector::SilenceDetectorEngine;
use media::trim_metadata::TrimMetadataWriter;
#[cfg(target_os = "macos")]
use platform::macos_service::MacRecordingService;
#[cfg(not(target_os = "macos"))]
compile_error!("LuZhi recording service currently supports macOS builds only; Windows app wiring requires a WindowsRecordingService.");
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// Shared recording state managed by Tauri.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BeautifyConfigPayload {
    cursor_magnification: bool,
    magnification_factor: f32,
    cursor_smoothing: bool,
    auto_trim_silences: bool,
    trim_sensitivity: String,
}

impl Default for BeautifyConfigPayload {
    fn default() -> Self {
        Self {
            cursor_magnification: true,
            magnification_factor: 2.0,
            cursor_smoothing: true,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
        }
    }
}

struct AppState {
    service: Arc<Mutex<MacRecordingService>>,
    capture_config: Arc<Mutex<CaptureConfig>>,
    audio_config: Arc<Mutex<AudioConfig>>,
    tick_runtime: Arc<Mutex<Option<TickRuntime>>>,
    mic_level_runtime: Arc<Mutex<Option<MicLevelRuntime>>>,
    beautify_config: Arc<Mutex<BeautifyConfigPayload>>,
    beautify_revision: Arc<AtomicU64>,
    export_cancel_token: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>>,
    export_sequence: Arc<AtomicU64>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            service: Arc::new(Mutex::new(MacRecordingService::new())),
            capture_config: Arc::new(Mutex::new(CaptureConfig::full_screen_1080p_30fps())),
            audio_config: Arc::new(Mutex::new(AudioConfig {
                capture_system_audio: true,
                capture_microphone: true,
                microphone_device: None,
                sample_rate: 48000,
                channels: 2,
            })),
            tick_runtime: Arc::new(Mutex::new(None)),
            mic_level_runtime: Arc::new(Mutex::new(None)),
            beautify_config: Arc::new(Mutex::new(BeautifyConfigPayload::default())),
            beautify_revision: Arc::new(AtomicU64::new(0)),
            export_cancel_token: Arc::new(Mutex::new(None)),
            export_sequence: Arc::new(AtomicU64::new(0)),
        }
    }
}

fn emit_state_changed(app: &AppHandle, state: RecordingState) {
    let payload = RecordingStatusPayload::from(state);
    let _ = app.emit("recording-state-changed", payload);
}

#[tauri::command]
fn recording_status(state: tauri::State<'_, AppState>) -> Result<RecordingStatusPayload, String> {
    let service = state
        .service
        .lock()
        .map_err(|_| "录制服务锁已损坏".to_string())?;
    Ok(RecordingStatusPayload::from(service.state()))
}

#[tauri::command]
fn recording_permissions() -> PermissionPayload {
    #[cfg(target_os = "macos")]
    let permissions = {
        let service = app::permission_service::PermissionService::new(
            platform::macos::permissions::MacPermissionProbe,
        );
        service.recording_permissions()
    };

    #[cfg(not(target_os = "macos"))]
    let permissions = RecordingPermissions {
        screen_recording: PermissionStatus::Unknown,
        microphone: PermissionStatus::Unknown,
    };

    PermissionPayload::from(permissions)
}

#[tauri::command]
async fn start_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let config = *state
        .capture_config
        .lock()
        .map_err(|_| "捕获配置锁已损坏".to_string())?;

    // 窗口/区域录制模式尚未实现
    if config.mode != core::config::CaptureMode::FullScreen {
        return Err("窗口/区域录制模式正在开发中，当前仅支持全屏录制".to_string());
    }

    let audio_config = state
        .audio_config
        .lock()
        .map_err(|_| "音频配置锁已损坏".to_string())?
        .clone();
    let mic_enabled = audio_config.capture_microphone;
    let service = state.service.clone();

    let beautify_config = state
        .beautify_config
        .lock()
        .map_err(|_| "美化配置锁已损坏".to_string())?
        .clone();

    // Recompute show_system_cursor from current beautify config to ensure the
    // raw cursor fact always matches the latest intent, regardless of whether
    // set_capture_mode was called before or after a config change.
    let expected_show_system_cursor =
        !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);
    let mut config = config;
    config.show_system_cursor = expected_show_system_cursor;

    let beautify_snapshot = BeautifyConfigSnapshot {
        cursor_magnification: beautify_config.cursor_magnification,
        magnification_factor: beautify_config.magnification_factor,
        cursor_smoothing: beautify_config.cursor_smoothing,
        auto_trim_silences: beautify_config.auto_trim_silences,
        trim_sensitivity: beautify_config.trim_sensitivity.clone(),
        raw_system_cursor_visible: config.show_system_cursor,
    };

    let new_state = tauri::async_runtime::spawn_blocking(move || {
        let mut service = service.lock().map_err(|_| "录制服务锁已损坏".to_string())?;
        service
            .start(config, audio_config, beautify_snapshot)
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(service.state())
    })
    .await
    .map_err(|e| format!("启动录制任务失败: {e}"))??;

    emit_state_changed(&app, new_state);

    // Start recording-tick runtime (250ms interval).
    let tick_app = app.clone();
    let mut tick_runtime = state
        .tick_runtime
        .lock()
        .map_err(|_| "计时器锁已损坏".to_string())?;
    if let Some(mut existing) = tick_runtime.take() {
        existing.stop();
    }
    *tick_runtime = Some(TickRuntime::spawn(move |elapsed| {
        let _ = tick_app.emit("recording-tick", serde_json::json!({ "elapsed": elapsed }));
    }));

    // Clean up any previous mic-level runtime before starting a new session.
    {
        let mut runtime_guard = state
            .mic_level_runtime
            .lock()
            .map_err(|_| "麦克风电平锁已损坏".to_string())?;
        if let Some(mut existing) = runtime_guard.take() {
            existing.stop();
        }
    }

    // Start mic-level runtime only when microphone capture is enabled.
    if mic_enabled {
        let mic_level = {
            let service = state
                .service
                .lock()
                .map_err(|_| "录制服务锁已损坏".to_string())?;
            service.mic_level_ref()
        };
        let mic_app = app.clone();
        let mic_runtime = MicLevelRuntime::spawn(move || {
            let level = mic_level.lock().map(|g| *g).unwrap_or(0.0);
            let _ = mic_app.emit("mic-level", MicLevelPayload { level });
        });

        *state
            .mic_level_runtime
            .lock()
            .map_err(|_| "麦克风电平锁已损坏".to_string())? = Some(mic_runtime);
    } else {
        // Emit a single zero-level event so the frontend knows mic is off.
        let _ = app.emit("mic-level", MicLevelPayload { level: 0.0 });
    }

    Ok(())
}

#[tauri::command]
async fn stop_recording(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RecordingResult, String> {
    // Stop the tick runtime first.
    if let Some(mut tick) = state
        .tick_runtime
        .lock()
        .map_err(|_| "计时器锁已损坏".to_string())?
        .take()
    {
        tick.stop();
    }

    let service = state.service.clone();

    let (new_state, result) = tauri::async_runtime::spawn_blocking(move || {
        let mut service = service.lock().map_err(|_| "录制服务锁已损坏".to_string())?;
        let result = service.stop();
        let new_state = service.state();
        Ok::<_, String>((new_state, result))
    })
    .await
    .map_err(|e| format!("停止录制任务失败: {e}"))??;

    // Emit final zero mic-level before stopping the runtime so the frontend
    // receives the zero value and clears its indicator.
    let _ = app.emit("mic-level", MicLevelPayload { level: 0.0 });

    // Stop the mic-level runtime.
    if let Some(mut mic_runtime) = state
        .mic_level_runtime
        .lock()
        .map_err(|_| "麦克风电平锁已损坏".to_string())?
        .take()
    {
        mic_runtime.stop();
    }

    emit_state_changed(&app, new_state);

    result.map_err(|e| e.to_string())
}

#[tauri::command]
fn pause_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut service = state
        .service
        .lock()
        .map_err(|_| "录制服务锁已损坏".to_string())?;
    service.pause().map_err(|e| e.to_string())?;
    let new_state = service.state();
    drop(service);
    emit_state_changed(&app, new_state);
    Ok(())
}

#[tauri::command]
fn resume_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut service = state
        .service
        .lock()
        .map_err(|_| "录制服务锁已损坏".to_string())?;
    service.resume().map_err(|e| e.to_string())?;
    let new_state = service.state();
    drop(service);
    emit_state_changed(&app, new_state);
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetCaptureModePayload {
    mode: String,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u32>,
}

#[tauri::command]
fn set_capture_mode(
    state: tauri::State<'_, AppState>,
    payload: SetCaptureModePayload,
) -> Result<(), String> {
    let mode = core::config::CaptureMode::mode_from_str(&payload.mode)?;
    let beautify_config = state
        .beautify_config
        .lock()
        .map_err(|_| "美化配置锁已损坏".to_string())?
        .clone();
    let show_system_cursor =
        !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);
    let mut config = state
        .capture_config
        .lock()
        .map_err(|_| "捕获配置锁已损坏".to_string())?;
    *config = CaptureConfig {
        mode,
        width: payload.width.unwrap_or(1920),
        height: payload.height.unwrap_or(1080),
        fps: payload.fps.unwrap_or(30),
        show_system_cursor,
    };
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetAudioConfigPayload {
    capture_system_audio: bool,
    capture_microphone: bool,
    microphone_device: Option<String>,
    sample_rate: u32,
    channels: u16,
}

#[tauri::command]
fn set_audio_config(
    state: tauri::State<'_, AppState>,
    payload: SetAudioConfigPayload,
) -> Result<(), String> {
    let mut config = state
        .audio_config
        .lock()
        .map_err(|_| "音频配置锁已损坏".to_string())?;
    *config = AudioConfig {
        capture_system_audio: payload.capture_system_audio,
        capture_microphone: payload.capture_microphone,
        microphone_device: payload.microphone_device,
        sample_rate: payload.sample_rate,
        channels: payload.channels,
    };
    Ok(())
}

#[tauri::command]
fn set_beautify_config(
    state: tauri::State<'_, AppState>,
    config: BeautifyConfigPayload,
) -> Result<u64, String> {
    let mut guard = state
        .beautify_config
        .lock()
        .map_err(|_| "美化配置锁已损坏".to_string())?;
    *guard = config;
    let rev = state.beautify_revision.fetch_add(1, Ordering::SeqCst) + 1;
    Ok(rev)
}

#[tauri::command]
fn get_beautify_config(state: tauri::State<'_, AppState>) -> Result<BeautifyConfigPayload, String> {
    state
        .beautify_config
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "美化配置锁已损坏".to_string())
}

/// Pure helper that applies the 方案 B safety contract:
/// - `metadata.beautify_config.raw_system_cursor_visible` is the immutable fact.
/// - `config` is the current Preview export intent.
fn build_effect_timeline_from_metadata(
    metadata: &RecordingMetadata,
    config: &BeautifyConfigPayload,
) -> Result<EffectTimeline, String> {
    let raw_visible = metadata.beautify_config.raw_system_cursor_visible;
    let want_overlay = config.cursor_magnification || config.cursor_smoothing;

    if raw_visible && want_overlay {
        return Err(
            "本次素材已录入系统光标，无法叠加美化光标效果。请先关闭光标美化后重新录制。"
                .to_string(),
        );
    }

    let render_cursor_overlay = !raw_visible;

    if !raw_visible && metadata.cursor_samples.is_empty() {
        return Err(
            "本次素材未录入系统光标，且光标元数据为空，无法生成光标时间线。请关闭光标美化后重新录制，或重新录制以恢复光标元数据。"
                .to_string(),
        );
    }

    let mut timeline = if want_overlay || !raw_visible {
        let engine = CursorEffectEngine::with_smoothing(
            ClickAnimationConfig {
                max_scale: if config.cursor_magnification {
                    config.magnification_factor.clamp(1.0, 3.0)
                } else {
                    1.0
                },
                peak_opacity: if config.cursor_magnification {
                    0.35
                } else {
                    0.0
                },
            },
            config.cursor_smoothing,
        );

        let clicks = if config.cursor_magnification {
            metadata.cursor_clicks.as_slice()
        } else {
            &[]
        };

        engine
            .build_timeline(
                &metadata.cursor_samples,
                clicks,
                metadata.fps,
                metadata.duration_nanos,
            )
            .map_err(|error| error.to_string())?
    } else {
        EffectTimeline {
            fps: metadata.fps,
            duration_nanos: metadata.duration_nanos,
            frames: Vec::new(),
            click_effects: Vec::new(),
            raw_system_cursor_visible: true,
            render_cursor_overlay: false,
        }
    };
    timeline.raw_system_cursor_visible = raw_visible;
    timeline.render_cursor_overlay = render_cursor_overlay;

    Ok(timeline)
}

#[tauri::command]
async fn build_cursor_effect_timeline(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<CursorEffectSummaryPayload, String> {
    // Reject during active recording — timeline must be built post-recording.
    {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        if matches!(
            service.state(),
            RecordingState::Recording | RecordingState::Paused | RecordingState::Processing
        ) {
            return Err("录制进行中，无法构建光标效果时间线。请先停止录制。".to_string());
        }
    }

    // Capture session id, metadata path, and beautify revision atomically so we
    // can verify after the async build that no newer config or session has
    // started (which would make our result stale).
    let (session_id, metadata_path, build_revision) = {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        let sid = service.current_session_id();
        let path = service
            .last_cursor_metadata_path()
            .ok_or_else(|| "没有可用的光标元数据，请先完成一次录制".to_string())?;
        let rev = state.beautify_revision.load(Ordering::SeqCst);
        (sid, path, rev)
    };

    let _ = app.emit(
        "post-process-progress",
        PostProcessProgressPayload {
            stage: "cursor",
            progress: 0,
            error: None,
        },
    );

    let config = state
        .beautify_config
        .lock()
        .map_err(|_| "美化配置锁已损坏".to_string())?
        .clone();

    let app_for_blocking = app.clone();
    let metadata_path_for_blocking = metadata_path.clone();
    let join_result = tauri::async_runtime::spawn_blocking(move || {
        let metadata = RecordingMetadataWriter::read_metadata(
            PathBuf::from(&metadata_path_for_blocking).as_path(),
        )
        .map_err(|error| error.to_string())?;

        let timeline = build_effect_timeline_from_metadata(&metadata, &config)?;

        let path = effect_timeline_path();
        RecordingMetadataWriter::write_effect_timeline(&path, &timeline)
            .map_err(|error| error.to_string())?;

        Ok::<_, String>((path, timeline))
    })
    .await;

    let (path, timeline) = match join_result {
        Ok(Ok(inner)) => inner,
        Ok(Err(e)) => {
            let _ = app_for_blocking.emit(
                "post-process-progress",
                PostProcessProgressPayload {
                    stage: "cursor",
                    progress: 0,
                    error: Some(format!("光标效果构建失败: {e}")),
                },
            );
            return Err(e);
        }
        Err(join_error) => {
            let msg = format!("光标效果时间线构建任务失败: {join_error}");
            let _ = app_for_blocking.emit(
                "post-process-progress",
                PostProcessProgressPayload {
                    stage: "cursor",
                    progress: 0,
                    error: Some(msg.clone()),
                },
            );
            return Err(msg);
        }
    };

    // Only write back the effect path if the session, metadata path, and
    // beautify revision have not changed. This prevents stale builds from
    // overwriting results belonging to a newer config or a different session.
    {
        let mut service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        let current_revision = state.beautify_revision.load(Ordering::SeqCst);
        if service.current_session_id() == session_id
            && service.last_cursor_metadata_path().as_deref() == Some(metadata_path.as_str())
            && current_revision == build_revision
        {
            service.set_last_effect_timeline_path(Some(path.to_string_lossy().to_string()));
        } else {
            // Clean up the temp timeline file since this build is stale and
            // the file will never be referenced by any session.
            let _ = std::fs::remove_file(&path);
            let msg = if current_revision != build_revision {
                "光标美化配置已变更，构建已取消".to_string()
            } else {
                "录制会话已变更，光标效果构建已取消".to_string()
            };
            let _ = app_for_blocking.emit(
                "post-process-progress",
                PostProcessProgressPayload {
                    stage: "cursor",
                    progress: 0,
                    error: Some(msg.clone()),
                },
            );
            return Err(msg);
        }
    }

    let _ = app_for_blocking.emit(
        "post-process-progress",
        PostProcessProgressPayload {
            stage: "cursor",
            progress: 100,
            error: None,
        },
    );

    Ok(CursorEffectSummaryPayload {
        frame_count: timeline.frames.len(),
        click_effect_count: timeline.click_effects.len(),
        effect_timeline_path: path.to_string_lossy().to_string(),
    })
}

fn license_state_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("读取应用数据目录失败: {error}"))
        .map(|dir| dir.join("license-state.json"))
}

#[tauri::command]
fn license_status(app: AppHandle) -> Result<LicenseStatusPayload, String> {
    use app::license_service::{
        FileTrialStore, LicenseService, NoopActivationCredentialStore, SystemLicenseClock,
    };
    let path = license_state_path(&app)?;
    let mut trial_store = FileTrialStore::new(path);
    let mut activation_store = NoopActivationCredentialStore;
    let service = LicenseService::new(&mut trial_store, &mut activation_store, SystemLicenseClock);
    service
        .status()
        .map(LicenseStatusPayload::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn activation_status(app: AppHandle) -> Result<LicenseStatusPayload, String> {
    license_status(app)
}

#[tauri::command]
fn activate_license(_app: AppHandle, code: String) -> Result<(), String> {
    if code.trim().is_empty() {
        return Err("激活码不能为空".to_string());
    }
    Err("服务端激活协议未接入，当前版本仅提供本地试用与激活状态接口".to_string())
}

#[tauri::command]
fn cancel_export(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let guard = state
        .export_cancel_token
        .lock()
        .map_err(|_| "导出取消状态锁已损坏".to_string())?;
    if let Some(token) = guard.as_ref() {
        token.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
async fn export_video(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    preset: String,
) -> Result<ExportSummaryPayload, String> {
    use media::export_presets::ExportPreset;
    let export_preset: ExportPreset = preset.parse()?;
    let preset_id = export_preset.spec().id;

    // Set up cancel token for this export.
    let cancel_token = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut guard = state
            .export_cancel_token
            .lock()
            .map_err(|_| "导出取消状态锁已损坏".to_string())?;
        *guard = Some(cancel_token.clone());
    }

    // Emit initial progress.
    let _ = app.emit(
        "export-progress",
        ExportProgressPayload {
            preset: preset_id,
            progress: 0,
            cancellable: true,
            output_path: None,
            error: None,
        },
    );

    let cursor = build_cursor_effect_timeline(app.clone(), state.clone()).await?;

    let config = state
        .beautify_config
        .lock()
        .map_err(|_| "美化配置锁已损坏".to_string())?
        .clone();

    let cut = if config.auto_trim_silences {
        Some(build_cut_timeline(app.clone(), state.clone()).await?)
    } else {
        None
    };

    // Build cut timeline for the export request.
    let cut_timeline = if let Some(ref summary) = cut {
        let path = PathBuf::from(&summary.cut_timeline_path);
        TrimMetadataWriter::read_cut_timeline(&path).map_err(|error| error.to_string())?
    } else {
        // Auto-trim off: use full duration from trim metadata if available.
        let trim_metadata_path = {
            let service = state
                .service
                .lock()
                .map_err(|_| "录制服务锁已损坏".to_string())?;
            service.last_trim_metadata_path()
        };
        let duration_nanos = if let Some(path) = trim_metadata_path {
            TrimMetadataWriter::read_metadata(PathBuf::from(path).as_path())
                .map_err(|error| error.to_string())?
                .duration_nanos
        } else {
            0
        };
        core::cut::CutTimeline::empty(duration_nanos)
    };

    // Get source artifact path and effect timeline path in a single lock acquisition.
    let (source_path, effect_timeline_path) = {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        let source = service
            .last_recording_output_path()
            .ok_or_else(|| "没有可用的原始录制文件，请先完成一次可播放录制".to_string())?;
        let effect = service.last_effect_timeline_path().map(PathBuf::from);
        (source, effect)
    };

    // Until production FFmpeg exporter is enabled, return output_path: None.
    // The structured export request is validated here but the actual FFmpeg
    // transcoding is gated behind Task 6.
    let _source = PathBuf::from(&source_path);
    let _sequence = state
        .export_sequence
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;

    // Emit final progress and clear cancel token.
    let _ = app.emit(
        "export-progress",
        ExportProgressPayload {
            preset: preset_id,
            progress: 100,
            cancellable: false,
            output_path: None,
            error: None,
        },
    );
    {
        let mut guard = state
            .export_cancel_token
            .lock()
            .map_err(|_| "导出取消状态锁已损坏".to_string())?;
        *guard = None;
    }

    Ok(ExportSummaryPayload {
        frame_count: cursor.frame_count,
        click_effect_count: cursor.click_effect_count,
        effect_timeline_path: cursor.effect_timeline_path,
        cut_count: cut.as_ref().map(|summary| summary.cut_count).unwrap_or(0),
        total_cut_nanos: cut
            .as_ref()
            .map(|summary| summary.total_cut_nanos)
            .unwrap_or(0),
        cut_timeline_path: cut.map(|summary| summary.cut_timeline_path),
        output_path: None,
    })
}

fn effect_timeline_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("cursor-effects-{millis}-{seq}.json"))
}

fn cut_timeline_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    std::env::temp_dir()
        .join("luzhi-recordings")
        .join(format!("cut-timeline-{millis}-{seq}.json"))
}

#[tauri::command]
async fn build_cut_timeline(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<CutTimelineSummaryPayload, String> {
    {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        if matches!(
            service.state(),
            RecordingState::Recording | RecordingState::Paused | RecordingState::Processing
        ) {
            return Err("录制进行中，无法构建裁剪时间线。请先停止录制。".to_string());
        }
    }

    // Capture session id, trim metadata path, and beautify revision atomically
    // so we can verify after the async build that no newer config or session has
    // started (which would make our result stale).
    let (session_id, trim_metadata_path, build_revision) = {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        let sid = service.current_session_id();
        let path = service
            .last_trim_metadata_path()
            .ok_or_else(|| "没有可用的裁剪元数据，请先完成一次录制".to_string())?;
        let rev = state.beautify_revision.load(Ordering::SeqCst);
        (sid, path, rev)
    };

    let config = state
        .beautify_config
        .lock()
        .map_err(|_| "美化配置锁已损坏".to_string())?
        .clone();
    let sensitivity: TrimSensitivity = config.trim_sensitivity.parse()?;
    let trim_config = TrimConfig::from_sensitivity(sensitivity);

    let _ = app.emit(
        "post-process-progress",
        PostProcessProgressPayload {
            stage: "trim",
            progress: 0,
            error: None,
        },
    );

    let app_for_blocking = app.clone();
    let trim_metadata_path_for_blocking = trim_metadata_path.clone();
    let join_result = tauri::async_runtime::spawn_blocking(move || {
        let metadata = TrimMetadataWriter::read_metadata(
            PathBuf::from(&trim_metadata_path_for_blocking).as_path(),
        )
        .map_err(|error| error.to_string())?;
        let audio_activity = metadata.derive_audio_activity(trim_config);
        let detector = SilenceDetectorEngine::new(trim_config);
        let timeline = detector
            .analyze(
                &audio_activity,
                &metadata.visual_activity,
                metadata.duration_nanos,
            )
            .map_err(|error| error.to_string())?;
        let path = cut_timeline_path();
        TrimMetadataWriter::write_cut_timeline(&path, &timeline)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((path, timeline))
    })
    .await;

    let (path, timeline) = match join_result {
        Ok(Ok(inner)) => inner,
        Ok(Err(error)) => {
            let _ = app_for_blocking.emit(
                "post-process-progress",
                PostProcessProgressPayload {
                    stage: "trim",
                    progress: 0,
                    error: Some(format!("裁剪时间线构建失败: {error}")),
                },
            );
            return Err(error);
        }
        Err(join_error) => {
            let msg = format!("裁剪时间线构建任务失败: {join_error}");
            let _ = app_for_blocking.emit(
                "post-process-progress",
                PostProcessProgressPayload {
                    stage: "trim",
                    progress: 0,
                    error: Some(msg.clone()),
                },
            );
            return Err(msg);
        }
    };

    {
        let mut service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        let current_revision = state.beautify_revision.load(Ordering::SeqCst);
        if service.current_session_id() == session_id
            && service.last_trim_metadata_path().as_deref() == Some(trim_metadata_path.as_str())
            && current_revision == build_revision
        {
            service.set_last_cut_timeline_path(Some(path.to_string_lossy().to_string()));
        } else {
            // Clean up the temp timeline file since this build is stale.
            let _ = std::fs::remove_file(&path);
            let msg = if current_revision != build_revision {
                "裁剪配置已变更，构建已取消".to_string()
            } else {
                "录制会话已变更，裁剪时间线构建已取消".to_string()
            };
            let _ = app_for_blocking.emit(
                "post-process-progress",
                PostProcessProgressPayload {
                    stage: "trim",
                    progress: 0,
                    error: Some(msg.clone()),
                },
            );
            return Err(msg);
        }
    }

    let _ = app_for_blocking.emit(
        "post-process-progress",
        PostProcessProgressPayload {
            stage: "trim",
            progress: 100,
            error: None,
        },
    );

    Ok(CutTimelineSummaryPayload {
        cut_count: timeline.cuts.len(),
        total_cut_nanos: timeline.total_cut_nanos,
        cut_timeline_path: path.to_string_lossy().to_string(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            recording_status,
            recording_permissions,
            start_recording,
            stop_recording,
            pause_recording,
            resume_recording,
            set_capture_mode,
            set_audio_config,
            set_beautify_config,
            get_beautify_config,
            build_cursor_effect_timeline,
            build_cut_timeline,
            export_video,
            cancel_export,
            license_status,
            activation_status,
            activate_license
        ])
        .run(tauri::generate_context!())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::{ClickPhase, CursorClick, CursorSample, MouseButton};

    fn sample(nanos: u64, x: f32, y: f32) -> CursorSample {
        CursorSample {
            timestamp: MediaTimestamp::from_nanos(nanos),
            x,
            y,
        }
    }

    fn click(nanos: u64, phase: ClickPhase, x: f32, y: f32) -> CursorClick {
        CursorClick {
            timestamp: MediaTimestamp::from_nanos(nanos),
            button: MouseButton::Left,
            phase,
            x,
            y,
        }
    }

    fn metadata_with_raw_visible(raw_visible: bool) -> RecordingMetadata {
        RecordingMetadata {
            fps: 60,
            duration_nanos: 33_333_333,
            cursor_samples: vec![sample(0, 100.0, 100.0), sample(16_666_667, 200.0, 200.0)],
            cursor_clicks: vec![click(8_000_000, ClickPhase::Down, 100.0, 100.0)],
            beautify_config: BeautifyConfigSnapshot {
                cursor_magnification: false,
                magnification_factor: 1.0,
                cursor_smoothing: false,
                auto_trim_silences: false,
                trim_sensitivity: "medium".to_string(),
                raw_system_cursor_visible: raw_visible,
            },
            cursor_snapshot_success_count: 2,
            cursor_snapshot_error_count: 0,
        }
    }

    #[test]
    fn raw_visible_and_overlay_requested_returns_error() {
        let metadata = metadata_with_raw_visible(true);
        let config = BeautifyConfigPayload {
            cursor_magnification: false,
            magnification_factor: 1.0,
            cursor_smoothing: true,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
        };
        let result = build_effect_timeline_from_metadata(&metadata, &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("已录入系统光标"));
    }

    #[test]
    fn raw_visible_no_overlay_generates_empty_timeline() {
        let metadata = metadata_with_raw_visible(true);
        let config = BeautifyConfigPayload {
            cursor_magnification: false,
            magnification_factor: 1.0,
            cursor_smoothing: false,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
        };
        let timeline = build_effect_timeline_from_metadata(&metadata, &config).unwrap();
        assert!(timeline.frames.is_empty());
        assert!(timeline.click_effects.is_empty());
        assert!(timeline.raw_system_cursor_visible);
        assert!(!timeline.render_cursor_overlay);
    }

    #[test]
    fn raw_hidden_no_overlay_generates_baseline_frames() {
        let metadata = metadata_with_raw_visible(false);
        let config = BeautifyConfigPayload {
            cursor_magnification: false,
            magnification_factor: 1.0,
            cursor_smoothing: false,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
        };
        let timeline = build_effect_timeline_from_metadata(&metadata, &config).unwrap();
        assert!(!timeline.raw_system_cursor_visible);
        assert!(timeline.render_cursor_overlay);
        assert!(!timeline.frames.is_empty());
        for frame in &timeline.frames {
            assert_eq!(frame.scale, 1.0);
            assert_eq!(frame.opacity, 1.0);
        }
    }

    #[test]
    fn raw_hidden_overlay_requested_generates_overlay_with_clicks() {
        let metadata = metadata_with_raw_visible(false);
        let config = BeautifyConfigPayload {
            cursor_magnification: true,
            magnification_factor: 2.0,
            cursor_smoothing: true,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
        };
        let timeline = build_effect_timeline_from_metadata(&metadata, &config).unwrap();
        assert!(!timeline.raw_system_cursor_visible);
        assert!(timeline.render_cursor_overlay);
        assert!(!timeline.frames.is_empty());
        assert!(!timeline.click_effects.is_empty());
    }

    #[test]
    fn raw_hidden_empty_cursor_samples_returns_error() {
        let mut metadata = metadata_with_raw_visible(false);
        metadata.cursor_samples = vec![];
        metadata.cursor_snapshot_success_count = 0;
        metadata.cursor_snapshot_error_count = 100;
        let config = BeautifyConfigPayload {
            cursor_magnification: true,
            magnification_factor: 2.0,
            cursor_smoothing: true,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
        };
        let result = build_effect_timeline_from_metadata(&metadata, &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("光标元数据为空"));
    }

    #[test]
    fn raw_visible_empty_cursor_samples_generates_empty_noop_timeline() {
        let mut metadata = metadata_with_raw_visible(true);
        metadata.cursor_samples = vec![];
        let config = BeautifyConfigPayload {
            cursor_magnification: false,
            magnification_factor: 1.0,
            cursor_smoothing: false,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
        };
        let timeline = build_effect_timeline_from_metadata(&metadata, &config).unwrap();
        assert!(timeline.raw_system_cursor_visible);
        assert!(!timeline.render_cursor_overlay);
        assert!(timeline.frames.is_empty());
        assert!(timeline.click_effects.is_empty());
    }
}
