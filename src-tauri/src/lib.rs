pub mod app;
pub mod core;
pub mod media;
pub mod platform;

use std::sync::{Arc, Mutex};

use app::events::{MicLevelPayload, PermissionPayload, RecordingStatusPayload};
use app::mic_level_runtime::MicLevelRuntime;
#[cfg(not(target_os = "macos"))]
use app::permission_service::{PermissionStatus, RecordingPermissions};
use app::recording_runtime::TickRuntime;
use app::state_machine::RecordingState;
use core::capture::AudioConfig;
use core::config::CaptureConfig;
use media::recording_writer::RecordingResult;
#[cfg(target_os = "macos")]
use platform::macos_service::MacRecordingService;
#[cfg(not(target_os = "macos"))]
compile_error!("LuZhi recording service currently supports macOS builds only; Windows app wiring requires a WindowsRecordingService.");
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

/// Shared recording state managed by Tauri.
struct AppState {
    service: Arc<Mutex<MacRecordingService>>,
    capture_config: Arc<Mutex<CaptureConfig>>,
    audio_config: Arc<Mutex<AudioConfig>>,
    tick_runtime: Arc<Mutex<Option<TickRuntime>>>,
    mic_level_runtime: Arc<Mutex<Option<MicLevelRuntime>>>,
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

    let new_state = tauri::async_runtime::spawn_blocking(move || {
        let mut service = service.lock().map_err(|_| "录制服务锁已损坏".to_string())?;
        service
            .start(config, audio_config)
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

        let mut runtime_guard = state
            .mic_level_runtime
            .lock()
            .map_err(|_| "麦克风电平锁已损坏".to_string())?;
        // Stop any previous runtime before replacing.
        if let Some(mut existing) = runtime_guard.take() {
            existing.stop();
        }
        *runtime_guard = Some(mic_runtime);
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

    // Stop the mic-level runtime.
    if let Some(mut mic_runtime) = state
        .mic_level_runtime
        .lock()
        .map_err(|_| "麦克风电平锁已损坏".to_string())?
        .take()
    {
        mic_runtime.stop();
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
    let mut config = state.capture_config.lock().unwrap();
    *config = CaptureConfig {
        mode,
        width: payload.width.unwrap_or(1920),
        height: payload.height.unwrap_or(1080),
        fps: payload.fps.unwrap_or(30),
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
            set_audio_config
        ])
        .run(tauri::generate_context!())?;

    Ok(())
}
