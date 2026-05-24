pub mod app;
pub mod core;
pub mod media;
pub mod platform;

use std::sync::Mutex;

use app::events::{PermissionPayload, RecordingStatusPayload};
use app::permission_service::{PermissionStatus, RecordingPermissions};
use app::state_machine::RecordingState;
use core::capture::AudioConfig;
use core::config::CaptureConfig;
use platform::macos_service::MacRecordingService;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// Shared recording state managed by Tauri.
struct AppState {
    service: Mutex<MacRecordingService>,
    capture_config: Mutex<CaptureConfig>,
    audio_config: Mutex<AudioConfig>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            service: Mutex::new(MacRecordingService::new()),
            capture_config: Mutex::new(CaptureConfig::full_screen_1080p_30fps()),
            audio_config: Mutex::new(AudioConfig {
                capture_system_audio: true,
                capture_microphone: true,
                microphone_device: None,
                sample_rate: 48000,
                channels: 2,
            }),
        }
    }
}

fn emit_state_changed(app: &AppHandle, state: RecordingState) {
    let payload = RecordingStatusPayload::from(state);
    let _ = app.emit("recording-state-changed", payload);
}

/// Result returned after stopping a recording session.
#[derive(Serialize)]
struct RecordingResult {
    duration_secs: u64,
    frame_count: u64,
    output_path: Option<String>,
}

#[tauri::command]
fn recording_status(state: tauri::State<'_, AppState>) -> RecordingStatusPayload {
    let service = state.service.lock().unwrap();
    RecordingStatusPayload::from(service.state())
}

#[tauri::command]
fn recording_permissions() -> PermissionPayload {
    let permissions = RecordingPermissions {
        screen_recording: PermissionStatus::Unknown,
        microphone: PermissionStatus::Unknown,
    };
    PermissionPayload::from(permissions)
}

#[tauri::command]
fn start_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let config = state.capture_config.lock().unwrap().clone();
    let audio_config = state.audio_config.lock().unwrap().clone();

    let mut service = state.service.lock().unwrap();
    service
        .start(config, audio_config)
        .map_err(|e| e.to_string())?;
    let new_state = service.state();
    drop(service);

    emit_state_changed(&app, new_state);

    // Spawn recording-tick emitter (every second).
    std::thread::spawn(move || {
        let mut elapsed: u64 = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            elapsed += 1;
            let _ = app.emit("recording-tick", serde_json::json!({ "elapsed": elapsed }));
        }
    });

    Ok(())
}

#[tauri::command]
fn stop_recording(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RecordingResult, String> {
    let mut service = state.service.lock().unwrap();
    let frame_count = service.frame_count();
    service.stop().map_err(|e| e.to_string())?;
    let new_state = service.state();
    drop(service);

    emit_state_changed(&app, new_state);

    Ok(RecordingResult {
        duration_secs: 0, // TODO: track elapsed time from recording-tick
        frame_count,
        output_path: None, // FFmpeg encoding not yet implemented
    })
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
    width: u32,
    height: u32,
    fps: u32,
}

#[tauri::command]
fn set_capture_mode(
    state: tauri::State<'_, AppState>,
    payload: SetCaptureModePayload,
) -> Result<(), String> {
    let mode = match payload.mode.as_str() {
        "fullscreen" => core::config::CaptureMode::FullScreen,
        _ => return Err(format!("未知捕获模式: {}", payload.mode)),
    };
    let mut config = state.capture_config.lock().unwrap();
    *config = CaptureConfig {
        mode,
        width: payload.width,
        height: payload.height,
        fps: payload.fps,
    };
    Ok(())
}

#[tauri::command]
fn set_audio_config(
    state: tauri::State<'_, AppState>,
    capture_system_audio: bool,
    capture_microphone: bool,
    microphone_device: Option<String>,
    sample_rate: u32,
    channels: u16,
) -> Result<(), String> {
    let mut config = state.audio_config.lock().unwrap();
    *config = AudioConfig {
        capture_system_audio,
        capture_microphone,
        microphone_device,
        sample_rate,
        channels,
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
