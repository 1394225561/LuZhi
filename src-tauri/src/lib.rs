pub mod app;
pub mod core;
pub mod platform;

use app::events::{PermissionPayload, RecordingStatusPayload};
use app::permission_service::{PermissionStatus, RecordingPermissions};
use app::state_machine::RecordingState;

#[tauri::command]
fn recording_status() -> RecordingStatusPayload {
    RecordingStatusPayload::from(RecordingState::Idle)
}

#[tauri::command]
fn recording_permissions() -> PermissionPayload {
    let permissions = RecordingPermissions {
        screen_recording: PermissionStatus::Unknown,
        microphone: PermissionStatus::Unknown,
    };
    PermissionPayload::from(permissions)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            recording_status,
            recording_permissions
        ])
        .run(tauri::generate_context!())?;

    Ok(())
}
