pub mod app;
pub mod core;
pub mod media;
pub mod platform;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use app::events::{
    CursorEffectSummaryPayload, MicLevelPayload, PermissionPayload, PostProcessProgressPayload,
    RecordingStatusPayload,
};
use app::mic_level_runtime::MicLevelRuntime;
#[cfg(not(target_os = "macos"))]
use app::permission_service::{PermissionStatus, RecordingPermissions};
use app::recording_runtime::TickRuntime;
use app::state_machine::RecordingState;
use core::capture::AudioConfig;
use core::config::CaptureConfig;
use core::processor::CursorProcessor;
use core::timeline::{BeautifyConfigSnapshot, EffectTimeline};
use media::cursor_engine::{ClickAnimationConfig, CursorEffectEngine};
use media::recording_metadata::RecordingMetadataWriter;
use media::recording_writer::RecordingResult;
#[cfg(target_os = "macos")]
use platform::macos_service::MacRecordingService;
#[cfg(not(target_os = "macos"))]
compile_error!("LuZhi recording service currently supports macOS builds only; Windows app wiring requires a WindowsRecordingService.");
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

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
        }
    }
}

fn emit_state_changed(app: &AppHandle, state: RecordingState) {
    let payload = RecordingStatusPayload::from(state);
    let _ = app.emit("recording-state-changed", payload);
}

#[tauri::command]
fn recording_status(state: tauri::State<'_, AppState>) -> RecordingStatusPayload {
    let service = state.service.lock().unwrap();
    RecordingStatusPayload::from(service.state())
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
    let mut service = state.service.lock().unwrap();
    service.pause().map_err(|e| e.to_string())?;
    let new_state = service.state();
    drop(service);
    emit_state_changed(&app, new_state);
    Ok(())
}

#[tauri::command]
fn resume_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut service = state.service.lock().unwrap();
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
    let mut config = state.capture_config.lock().unwrap();
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
    let mut config = state.audio_config.lock().unwrap();
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

        let raw_visible = metadata.beautify_config.raw_system_cursor_visible;
        let want_overlay = config.cursor_magnification || config.cursor_smoothing;

        if raw_visible && want_overlay {
            return Err(
                "本次素材已录入系统光标，无法叠加美化光标效果。请先关闭光标美化后重新录制。"
                    .to_string(),
            );
        }

        let render_cursor_overlay = !raw_visible;

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

#[tauri::command]
async fn export_video(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    preset: String,
) -> Result<CursorEffectSummaryPayload, String> {
    if !matches!(preset.as_str(), "bilibili" | "douyin" | "xiaohongshu") {
        return Err(format!("未知导出预设：{preset}"));
    }

    build_cursor_effect_timeline(app, state).await
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
            export_video
        ])
        .run(tauri::generate_context!())?;

    Ok(())
}
