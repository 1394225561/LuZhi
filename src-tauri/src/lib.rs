pub mod app;
pub mod core;
pub mod media;
pub mod platform;
pub mod test_support;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use app::error::AppResult;
use app::events::{
    CursorEffectSummaryPayload, CutTimelineSummaryPayload, ExportProgressPayload,
    ExportSummaryPayload, LicenseStatusPayload, MicLevelPayload, PermissionPayload,
    PostProcessProgressPayload, RecordingStatusPayload,
};
use app::mic_level_runtime::MicLevelRuntime;
#[cfg(not(target_os = "macos"))]
use app::permission_service::{PermissionStatus, RecordingPermissions};
use app::recording_library::{LibraryEntrySummary, RecordingContextPayload, RecordingLibrary};
use app::recording_runtime::TickRuntime;
use app::recording_service_boundary::{CursorMainThreadDispatcher, PlatformRecordingService};
use app::state_machine::RecordingState;
use core::capture::{AudioConfig, DenoiseMode};
use core::config::{CaptureConfig, CaptureMode};
use core::cut::{TrimConfig, TrimSensitivity};
use core::processor::{CursorProcessor, SilenceDetector};
use core::timeline::{BeautifyConfigSnapshot, EffectTimeline};
use media::cursor_engine::{ClickAnimationConfig, CursorEffectEngine};
use media::export_paths::export_output_path;
use media::recording_metadata::{RecordingMetadata, RecordingMetadataWriter};
use media::recording_writer::StopRecordingResponse;
use media::silence_detector::SilenceDetectorEngine;
use media::trim_exporter::ExportProgressReporter;
use media::trim_metadata::TrimMetadataWriter;
#[cfg(target_os = "macos")]
use platform::macos_service::MacRecordingService;
#[cfg(target_os = "windows")]
use platform::windows_service::WindowsRecordingService;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone)]
struct TauriCursorMainThreadDispatcher {
    app: AppHandle,
}

impl CursorMainThreadDispatcher for TauriCursorMainThreadDispatcher {
    fn run_on_main_thread(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        self.app.run_on_main_thread(task).map_err(|e| e.to_string())
    }
}

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

fn create_platform_recording_service() -> Box<dyn PlatformRecordingService> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacRecordingService::new())
    }

    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsRecordingService::new())
    }
}

#[derive(Clone)]
struct AppState {
    service: Arc<Mutex<Box<dyn PlatformRecordingService>>>,
    capture_config: Arc<Mutex<CaptureConfig>>,
    audio_config: Arc<Mutex<AudioConfig>>,
    tick_runtime: Arc<Mutex<Option<TickRuntime>>>,
    mic_level_runtime: Arc<Mutex<Option<MicLevelRuntime>>>,
    beautify_config: Arc<Mutex<BeautifyConfigPayload>>,
    beautify_revision: Arc<AtomicU64>,
    export_cancel_token: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>>,
    export_sequence: Arc<AtomicU64>,
    library: Arc<Mutex<RecordingLibrary>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            service: Arc::new(Mutex::new(create_platform_recording_service())),
            capture_config: Arc::new(Mutex::new(CaptureConfig::full_screen_1080p_30fps())),
            audio_config: Arc::new(Mutex::new(AudioConfig {
                capture_system_audio: true,
                capture_microphone: true,
                microphone_device: None,
                sample_rate: 48000,
                channels: 2,
                denoise_mode: DenoiseMode::default(),
            })),
            tick_runtime: Arc::new(Mutex::new(None)),
            mic_level_runtime: Arc::new(Mutex::new(None)),
            beautify_config: Arc::new(Mutex::new(BeautifyConfigPayload::default())),
            beautify_revision: Arc::new(AtomicU64::new(0)),
            export_cancel_token: Arc::new(Mutex::new(None)),
            export_sequence: Arc::new(AtomicU64::new(0)),
            library: Arc::new(Mutex::new(RecordingLibrary::new(
                &std::env::temp_dir().join("luzhi-recordings"),
            ))),
        }
    }
}

impl AppState {
    fn init_library(&self, app_data_dir: &Path) {
        let mut lib = self.library.lock().expect("library lock poisoned");
        *lib = RecordingLibrary::new(app_data_dir);
        lib.repair_on_startup();
    }
}

fn emit_state_changed(app: &AppHandle, state: RecordingState) {
    let payload = RecordingStatusPayload::from(state);
    let _ = app.emit("recording-state-changed", payload);
}

fn emit_state_changed_with_result(
    app: &AppHandle,
    state: RecordingState,
    result: &crate::media::recording_writer::RecordingResult,
) {
    let status = RecordingStatusPayload::from(state);
    let _ = app.emit(
        "recording-state-changed",
        serde_json::json!({
            "state": status.state,
            "canStart": status.can_start,
            "result": result,
        }),
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowStateAction {
    Pause,
    Resume,
    Stop,
}

fn action_for_window_state(
    window_state: crate::core::window::WindowRecordingState,
) -> WindowStateAction {
    use crate::core::window::WindowRecordingState;
    match window_state {
        WindowRecordingState::Recording => WindowStateAction::Resume,
        WindowRecordingState::Minimized => WindowStateAction::Pause,
        WindowRecordingState::Closed => WindowStateAction::Stop,
    }
}

fn register_recording_response(
    library: &Arc<Mutex<RecordingLibrary>>,
    response: &StopRecordingResponse,
) -> Result<(), String> {
    // Do not register failed recordings — they would pollute the normal library
    // and could be exported as if they were valid recordings.
    if response.failed {
        return Ok(());
    }

    if let Some(ref video_path) = response.result.output_path {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let cursor_path = response.result.cursor_metadata_path.as_deref();
        let effect_path = response.result.effect_timeline_path.as_deref();
        let trim_path = response.result.trim_metadata_path.as_deref();
        let cut_path = response.result.cut_timeline_path.as_deref();

        let mut library = library.lock().map_err(|_| "录制库锁已损坏".to_string())?;
        let register_result = library.register(
            &format!("rec-{now_ms}-{}", response.result.frame_count),
            now_ms,
            response.result.duration_secs as f64,
            video_path,
            cursor_path,
            effect_path,
            trim_path,
            cut_path,
        );
        if let Err(e) = register_result {
            log::warn!("自动注册录制到历史库失败：{e}");
        }
    }

    Ok(())
}

fn stop_recording_blocking(
    app: &AppHandle,
    state: &AppState,
) -> Result<(RecordingState, AppResult<StopRecordingResponse>), String> {
    if let Some(mut tick) = state
        .tick_runtime
        .lock()
        .map_err(|_| "计时器锁已损坏".to_string())?
        .take()
    {
        tick.stop();
    }

    let response = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        let response = service.stop();
        let new_state = service.state();
        (new_state, response)
    };

    let _ = app.emit("mic-level", MicLevelPayload { level: 0.0 });

    if let Some(mut mic_runtime) = state
        .mic_level_runtime
        .lock()
        .map_err(|_| "麦克风电平锁已损坏".to_string())?
        .take()
    {
        mic_runtime.stop();
    }

    if let Ok(ref resp) = response.1 {
        register_recording_response(&state.library, resp)?;
    }

    Ok(response)
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
        accessibility: PermissionStatus::Unknown,
    };

    PermissionPayload::from(permissions)
}

#[tauri::command]
async fn start_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let config = *state
        .capture_config
        .lock()
        .map_err(|_| "捕获配置锁已损坏".to_string())?;

    // 区域录制模式尚未实现
    if config.mode == core::config::CaptureMode::Area {
        return Err("区域录制模式正在开发中".to_string());
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
    let cursor_main_thread_dispatcher =
        Box::new(TauriCursorMainThreadDispatcher { app: app.clone() });

    let new_state = tauri::async_runtime::spawn_blocking(move || {
        let mut service = service.lock().map_err(|_| "录制服务锁已损坏".to_string())?;
        match config.mode {
            core::config::CaptureMode::Window => {
                let window_id = config.window_id.ok_or("未选择录制窗口".to_string())?;
                service
                    .start_window(
                        window_id,
                        config.clone(),
                        config.show_system_cursor,
                        audio_config,
                        beautify_snapshot,
                        cursor_main_thread_dispatcher,
                    )
                    .map_err(|e| e.to_string())?;
            }
            _ => {
                service
                    .start(
                        config,
                        audio_config,
                        beautify_snapshot,
                        cursor_main_thread_dispatcher,
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
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

    // Start window-state-monitor listener after runtimes are installed so an
    // immediate close event cannot leave a fresh tick/mic runtime running.
    {
        let mut service_guard = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        if let Some(receiver) = service_guard.take_window_state_receiver() {
            let window_app = app.clone();
            let window_state = state.inner().clone();
            let window_title = "录制窗口".to_string();

            std::thread::spawn(move || {
                while let Ok(window_state_event) = receiver.recv() {
                    match action_for_window_state(window_state_event) {
                        WindowStateAction::Pause => {
                            let _ = window_app.emit(
                                "window-state-changed",
                                serde_json::json!({
                                    "state": "minimized",
                                    "windowTitle": window_title
                                }),
                            );
                            let new_state = window_state
                                .service
                                .lock()
                                .map_err(|_| "录制服务锁已损坏".to_string())
                                .and_then(|mut service| {
                                    service.pause().map_err(|e| e.to_string())?;
                                    Ok(service.state())
                                });
                            if let Ok(new_state) = new_state {
                                emit_state_changed(&window_app, new_state);
                            }
                        }
                        WindowStateAction::Resume => {
                            let new_state = window_state
                                .service
                                .lock()
                                .map_err(|_| "录制服务锁已损坏".to_string())
                                .and_then(|mut service| {
                                    service.resume().map_err(|e| e.to_string())?;
                                    Ok(service.state())
                                });
                            if let Ok(new_state) = new_state {
                                emit_state_changed(&window_app, new_state);
                            }
                        }
                        WindowStateAction::Stop => {
                            let _ = window_app.emit(
                                "window-state-changed",
                                serde_json::json!({
                                    "state": "closed",
                                    "windowTitle": window_title
                                }),
                            );
                            match stop_recording_blocking(&window_app, &window_state) {
                                Ok((new_state, response)) => match response {
                                    Ok(resp) => {
                                        emit_state_changed_with_result(
                                            &window_app,
                                            new_state,
                                            &resp.result,
                                        );
                                        if resp.failed {
                                            log::warn!("窗口关闭自动停止录制存在完成错误");
                                        }
                                    }
                                    Err(e) => {
                                        emit_state_changed(&window_app, new_state);
                                        log::warn!("窗口关闭自动停止录制失败：{e}");
                                    }
                                },
                                Err(e) => log::warn!("窗口关闭自动停止录制失败：{e}"),
                            }
                            break;
                        }
                    }
                }
            });
        }
    }

    Ok(())
}

/// Combined payload returned by `stop_recording`, bundling the recording result
/// with the final state, permissions, and license status so the frontend can
/// transition in a single round-trip.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordStatePayload {
    state: &'static str,
    permissions: app::events::PermissionPayload,
    license_status: app::events::LicenseStatusPayload,
    recording: Option<StopRecordingResponse>,
}

#[tauri::command]
async fn stop_recording(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RecordStatePayload, String> {
    let app_state = state.inner().clone();
    let stop_app = app.clone();
    let (new_state, response) = tauri::async_runtime::spawn_blocking(move || {
        stop_recording_blocking(&stop_app, &app_state)
    })
    .await
    .map_err(|e| format!("停止录制任务失败: {e}"))??;

    // Convert AppResult to Option — on error, recording is None.
    let recording = response.ok();

    // Gather permissions.
    #[cfg(target_os = "macos")]
    let permissions = {
        let perm_service = app::permission_service::PermissionService::new(
            platform::macos::permissions::MacPermissionProbe,
        );
        perm_service.recording_permissions()
    };
    #[cfg(not(target_os = "macos"))]
    let permissions = RecordingPermissions {
        screen_recording: app::permission_service::PermissionStatus::Unknown,
        microphone: app::permission_service::PermissionStatus::Unknown,
        accessibility: app::permission_service::PermissionStatus::Unknown,
    };

    // Gather license status (non-fatal: recording result must not be lost
    // if the license lookup fails due to filesystem or corruption issues).
    let license_status = (|| -> Result<app::events::LicenseStatusPayload, String> {
        use app::license_service::{
            FileTrialStore, LicenseService, NoopActivationCredentialStore, SystemLicenseClock,
        };
        let path = license_state_path(&app)?;
        let mut trial_store = FileTrialStore::new(path);
        let mut activation_store = NoopActivationCredentialStore;
        let svc = LicenseService::new(&mut trial_store, &mut activation_store, SystemLicenseClock);
        svc.status()
            .map(app::events::LicenseStatusPayload::from)
            .map_err(|e| e.to_string())
    })()
    .unwrap_or(app::events::LicenseStatusPayload {
        kind: "unknown",
        trial_days_remaining: 0,
        is_expired: false,
        activated: false,
    });

    emit_state_changed(&app, new_state);

    Ok(RecordStatePayload {
        state: new_state.as_str(),
        permissions: app::events::PermissionPayload::from(permissions),
        license_status: app::events::LicenseStatusPayload::from(license_status),
        recording,
    })
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
    window_id: Option<u32>,
}

fn build_capture_config_update(
    previous: CaptureConfig,
    mode: CaptureMode,
    width: Option<u32>,
    height: Option<u32>,
    fps: u32,
    show_system_cursor: bool,
) -> CaptureConfig {
    CaptureConfig {
        mode,
        width: width.unwrap_or(1920),
        height: height.unwrap_or(1080),
        fps,
        show_system_cursor,
        window_id: if mode == CaptureMode::Window {
            previous.window_id
        } else {
            None
        },
    }
}

fn validated_capture_fps(fps: Option<u32>) -> Result<u32, String> {
    let fps = fps.unwrap_or(30);
    match fps {
        30 | 60 => Ok(fps),
        other => Err(format!("不支持的 fps: {other}，仅支持 30 或 60")),
    }
}

fn apply_capture_config_payload(
    previous: CaptureConfig,
    payload: SetCaptureModePayload,
    show_system_cursor: bool,
) -> Result<CaptureConfig, String> {
    let mode = CaptureMode::mode_from_str(&payload.mode)?;
    let fps = validated_capture_fps(payload.fps)?;
    let mut config = build_capture_config_update(
        previous,
        mode,
        payload.width,
        payload.height,
        fps,
        show_system_cursor,
    );
    if mode == CaptureMode::Window && payload.window_id.is_some() {
        config.window_id = payload.window_id;
    }
    Ok(config)
}

#[tauri::command]
fn set_capture_mode(
    state: tauri::State<'_, AppState>,
    payload: SetCaptureModePayload,
) -> Result<(), String> {
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
    *config = apply_capture_config_payload(*config, payload, show_system_cursor)?;
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
    #[serde(default)]
    denoise_mode: DenoiseMode,
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
        denoise_mode: payload.denoise_mode,
    };
    Ok(())
}

/// Information about a microphone device available on the system.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MicrophoneDeviceInfo {
    /// Device name (used as identifier in `set_audio_config`).
    name: String,
    /// Whether this is likely a Bluetooth device based on name heuristics.
    is_bluetooth: bool,
}

/// Lists available microphone input devices.
///
/// Returns device names and Bluetooth detection hints.
/// The frontend uses this to populate a device selector and show
/// Bluetooth HFP compatibility warnings.
#[tauri::command]
fn list_microphone_devices() -> Result<Vec<MicrophoneDeviceInfo>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| format!("枚举麦克风设备失败: {e}"))?;

    let bluetooth_keywords = [
        "bluetooth",
        "airpods",
        "headset",
        "hands-free",
        "hfp",
        "a2dp",
        "wireless",
        "bt ",
        "bt-",
    ];

    let mut result = Vec::new();
    for device in devices {
        if let Ok(name) = device.name() {
            let name_lower = name.to_lowercase();
            let is_bluetooth = bluetooth_keywords.iter().any(|kw| name_lower.contains(kw));
            result.push(MicrophoneDeviceInfo { name, is_bluetooth });
        }
    }

    Ok(result)
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
            source_pts_origin_nanos: 0,
        }
    };
    timeline.raw_system_cursor_visible = raw_visible;
    timeline.render_cursor_overlay = render_cursor_overlay;
    // Set source PTS origin from metadata for overlay timestamp alignment.
    if let Some(ref mtd) = metadata.media_timeline_diagnostics {
        timeline.source_pts_origin_nanos = mtd.first_video_pts_nanos_raw;
    }

    Ok(timeline)
}

#[tauri::command]
async fn build_cursor_effect_timeline(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    recording_id: Option<String>,
) -> Result<CursorEffectSummaryPayload, String> {
    let is_history_export = recording_id.is_some();

    // Reject during active recording — timeline must be built post-recording.
    // Skip this check for history export (no active recording session).
    if !is_history_export {
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
    // For history export, resolve from the library and skip session tracking.
    let (session_id, metadata_path, build_revision) = if is_history_export {
        let lib = state
            .library
            .lock()
            .map_err(|_| "录制库锁已损坏".to_string())?;
        let ctx = lib
            .get_recording(recording_id.as_ref().unwrap())
            .map_err(|e| e.to_string())?;
        let path = ctx
            .entry
            .cursor_metadata_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .ok_or_else(|| "历史录制缺少光标元数据路径".to_string())?;
        (0u64, path, 0u64)
    } else {
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
    // For history export, skip this validation entirely.
    if !is_history_export {
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
        effect_timeline_path: Some(path.to_string_lossy().to_string()),
    })
}

/// Load the full cursor effect timeline from disk so the frontend can render
/// a preview overlay. Returns the complete `EffectTimeline` including per-frame
/// cursor positions, scales, and click effects.
#[tauri::command]
async fn get_cursor_effect_timeline(
    state: tauri::State<'_, AppState>,
) -> Result<EffectTimeline, String> {
    let path = {
        let service = state
            .service
            .lock()
            .map_err(|_| "录制服务锁已损坏".to_string())?;
        service
            .last_effect_timeline_path()
            .ok_or_else(|| "没有可用的光标效果时间线，请先构建光标效果".to_string())?
    };

    let json =
        std::fs::read_to_string(&path).map_err(|e| format!("读取光标效果时间线失败: {e}"))?;
    serde_json::from_str(&json).map_err(|e| format!("解析光标效果时间线失败: {e}"))
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
fn list_recordings(state: tauri::State<'_, AppState>) -> Result<Vec<LibraryEntrySummary>, String> {
    let library = state
        .library
        .lock()
        .map_err(|_| "录制库锁已损坏：lock poisoned".to_string())?;
    Ok(library.list())
}

#[tauri::command]
fn get_recording_context(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<RecordingContextPayload, String> {
    let library = state
        .library
        .lock()
        .map_err(|_| "录制库锁已损坏：lock poisoned".to_string())?;
    let ctx = library.get_recording(&id).map_err(|e| e.to_string())?;
    Ok(RecordingContextPayload {
        video_path: ctx.entry.video_path.to_string_lossy().to_string(),
        cursor_metadata_path: ctx
            .entry
            .cursor_metadata_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        effect_timeline_path: ctx
            .entry
            .effect_timeline_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        trim_metadata_path: ctx
            .entry
            .trim_metadata_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        cut_timeline_path: ctx
            .entry
            .cut_timeline_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        metadata_json: ctx.metadata_json,
        effect_timeline_json: ctx.effect_timeline_json,
        cut_timeline_json: ctx.cut_timeline_json,
    })
}

#[tauri::command]
fn import_recording(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<LibraryEntrySummary, String> {
    let mut library = state
        .library
        .lock()
        .map_err(|_| "录制库锁已损坏：lock poisoned".to_string())?;
    let entry = library
        .import(Path::new(&path))
        .map_err(|e| e.to_string())?;
    Ok(entry)
}

#[tauri::command]
fn delete_recording(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let mut library = state
        .library
        .lock()
        .map_err(|_| "录制库锁已损坏：lock poisoned".to_string())?;
    library.delete(&id, true).map_err(|e| e.to_string())?;
    Ok(())
}

/// 获取当前可见窗口列表
#[tauri::command]
async fn list_windows() -> Result<Vec<core::window::WindowInfo>, String> {
    #[cfg(target_os = "macos")]
    {
        platform::macos::window_list::list_windows().map_err(|e| e.to_string())
    }

    #[cfg(target_os = "windows")]
    {
        platform::windows::window_capture::list_windows().map_err(|e| e.to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("窗口录制尚未支持当前平台".to_string())
    }
}

/// 设置录制目标窗口 ID
#[tauri::command]
async fn set_window_id(state: tauri::State<'_, AppState>, window_id: u32) -> Result<(), String> {
    // 验证窗口存在
    #[cfg(target_os = "macos")]
    {
        let windows = platform::macos::window_list::list_windows().map_err(|e| e.to_string())?;

        let window = windows
            .iter()
            .find(|w| w.window_id == window_id)
            .ok_or(format!("窗口未找到：{window_id}"))?;

        if !window.is_on_screen {
            return Err(format!("窗口已最小化，请恢复窗口后重试：{}", window.title));
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindow, IsWindowVisible};

        let hwnd = windows::Win32::Foundation::HWND(window_id as *mut std::ffi::c_void);
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return Err(format!("窗口未找到：{window_id}"));
            }
            if !IsWindowVisible(hwnd).as_bool() {
                return Err(format!("窗口不可见，请恢复窗口后重试：{window_id}"));
            }
            if IsIconic(hwnd).as_bool() {
                return Err(format!("窗口已最小化，请恢复窗口后重试：{window_id}"));
            }
        }
    }

    // 更新配置
    let mut config = state
        .capture_config
        .lock()
        .map_err(|_| "捕获配置锁已损坏".to_string())?;

    config.mode = core::config::CaptureMode::Window;
    config.window_id = Some(window_id);

    Ok(())
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExportedFileLocationPlatform {
    Macos,
    Windows,
    Unsupported,
}

impl ExportedFileLocationPlatform {
    fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::Macos
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Self::Unsupported
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExportedFileLocationCommand {
    program: &'static str,
    args: Vec<String>,
}

fn build_exported_file_location_command(
    path: &Path,
    platform: ExportedFileLocationPlatform,
) -> Result<ExportedFileLocationCommand, String> {
    let file_path = path.to_string_lossy().to_string();
    if file_path.trim().is_empty() {
        return Err("导出文件路径为空，无法打开所在目录".to_string());
    }

    match platform {
        ExportedFileLocationPlatform::Macos => Ok(ExportedFileLocationCommand {
            program: "open",
            args: vec!["-R".to_string(), file_path],
        }),
        ExportedFileLocationPlatform::Windows => Ok(ExportedFileLocationCommand {
            program: "explorer.exe",
            args: vec![format!("/select,{file_path}")],
        }),
        ExportedFileLocationPlatform::Unsupported => {
            Err("当前平台暂不支持打开导出文件所在目录".to_string())
        }
    }
}

#[tauri::command]
fn open_exported_file_location(path: String) -> Result<(), String> {
    let command = build_exported_file_location_command(
        Path::new(&path),
        ExportedFileLocationPlatform::current(),
    )?;

    Command::new(command.program)
        .args(command.args)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开导出文件所在目录失败: {error}"))
}

#[tauri::command]
async fn export_video(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    preset: String,
    recording_id: Option<String>,
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

    // Inner function that does the actual export work.
    // Any error from this function will be handled by the outer code which
    // always clears the cancel token and emits terminal progress.
    let do_export = async {
        let config = state
            .beautify_config
            .lock()
            .map_err(|_| "美化配置锁已损坏".to_string())?
            .clone();

        // Cursor timeline: when beautification is enabled (cursor hidden),
        // the effect timeline is REQUIRED. A failed build must block export
        // to prevent silent "no cursor, no beautification" exports (BUG-005).
        let cursor_needs_overlay = config.cursor_magnification || config.cursor_smoothing;
        let cursor =
            build_cursor_effect_timeline(app.clone(), state.clone(), recording_id.clone()).await;

        // Progress: cursor timeline preparation (0% → 5%).
        let _ = app.emit(
            "export-progress",
            ExportProgressPayload {
                preset: preset_id,
                progress: 5,
                cancellable: true,
                output_path: None,
                error: None,
            },
        );
        let cursor = if cursor_needs_overlay {
            cursor.map_err(|e| format!("光标美化已开启但效果时间线构建失败：{e}"))?
        } else {
            cursor.unwrap_or(CursorEffectSummaryPayload {
                frame_count: 0,
                click_effect_count: 0,
                effect_timeline_path: None,
            })
        };

        // Progress: cut timeline preparation (5% → 10%).
        let _ = app.emit(
            "export-progress",
            ExportProgressPayload {
                preset: preset_id,
                progress: 10,
                cancellable: true,
                output_path: None,
                error: None,
            },
        );

        let cut = if config.auto_trim_silences {
            Some(build_cut_timeline(app.clone(), state.clone(), recording_id.clone()).await?)
        } else {
            None
        };

        // Read source artifact path. When `recording_id` is provided (history
        // re-export), look up from the library; otherwise use the service's
        // last recording path (fresh recording workflow).
        let (trim_metadata_path, source_path) = if let Some(ref id) = recording_id {
            let lib = state
                .library
                .lock()
                .map_err(|_| "录制库锁已损坏".to_string())?;
            let ctx = lib.get_recording(id).map_err(|e| e.to_string())?;
            let source = Some(ctx.entry.video_path.to_string_lossy().to_string());
            let trim = ctx
                .entry
                .trim_metadata_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string());
            (trim, source)
        } else {
            let service = state
                .service
                .lock()
                .map_err(|_| "录制服务锁已损坏".to_string())?;
            let trim = service.last_trim_metadata_path();
            let source = service.last_recording_output_path();
            (trim, source)
        };
        let effect_timeline = cursor.effect_timeline_path.as_ref().map(PathBuf::from);

        // In non-FFmpeg builds, source artifact is not available.
        // Return a clear FFmpeg Gate error instead of requiring a source artifact.
        #[cfg(not(feature = "ffmpeg"))]
        if source_path.is_none() {
            // Terminal progress is emitted by the outer handler on Ok with output_path: None.
            let cut_count = cut.as_ref().map(|s| s.cut_count).unwrap_or(0);
            let total_cut_nanos = cut.as_ref().map(|s| s.total_cut_nanos).unwrap_or(0);
            let cut_timeline_path = cut.map(|s| s.cut_timeline_path);
            return Ok(ExportSummaryPayload {
                frame_count: cursor.frame_count,
                click_effect_count: cursor.click_effect_count,
                effect_timeline_path: cursor.effect_timeline_path,
                cut_count,
                total_cut_nanos,
                cut_timeline_path,
                output_path: None,
            });
        }

        // In FFmpeg builds, source artifact is required.
        let source_path = source_path
            .ok_or_else(|| "没有可用的原始录制文件，请先完成一次可播放录制".to_string())
            .map(PathBuf::from)?;

        // Build cut timeline for the export request.
        let cut_timeline = if let Some(ref summary) = cut {
            let path = PathBuf::from(&summary.cut_timeline_path);
            TrimMetadataWriter::read_cut_timeline(&path).map_err(|error| error.to_string())?
        } else {
            // Auto-trim off: use full duration from trim metadata if available.
            let duration_nanos = if let Some(path) = trim_metadata_path {
                TrimMetadataWriter::read_metadata(PathBuf::from(path).as_path())
                    .map_err(|error| error.to_string())?
                    .duration_nanos
            } else {
                0
            };
            core::cut::CutTimeline::empty(duration_nanos)
        };

        // Generate output path for the export.
        let sequence = state
            .export_sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let output_path = export_output_path(&source_path, export_preset, sequence)
            .map_err(|error| error.to_string())?;

        // Create progress reporter that emits events to the frontend.
        // Map exporter 1-99 to overall 10-99 (preparation phase uses 0-10).
        let app_for_progress = app.clone();
        let progress_reporter = ExportProgressReporter::new(Arc::new(move |progress| {
            let mapped = 10 + (progress as u32) * 89 / 99;
            let _ = app_for_progress.emit(
                "export-progress",
                ExportProgressPayload {
                    preset: preset_id,
                    progress: mapped.clamp(10, 99) as u8,
                    cancellable: true,
                    output_path: None,
                    error: None,
                },
            );
        }));

        // FFmpeg decode/encode is CPU+IO intensive. Run in a blocking worker
        // to avoid blocking the Tauri async runtime.
        #[cfg(feature = "ffmpeg")]
        let export_result = {
            let cancel = cancel_token.clone();
            // Resolve cursor assets directory relative to the crate manifest.
            // At runtime, assets are bundled alongside the binary.
            let cursor_assets_dir = Some(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join("cursors"),
            );
            tauri::async_runtime::spawn_blocking(move || {
                let mut exporter = media::trim_exporter::FfmpegTrimExporter;
                app::export_service::export_recording_with_timeline(
                    &mut exporter,
                    source_path,
                    Some(output_path),
                    export_preset,
                    cut_timeline,
                    effect_timeline,
                    cursor_assets_dir,
                    cancel,
                    Some(progress_reporter),
                    sequence,
                )
            })
            .await
            .map_err(|e| format!("导出工作线程异常终止: {e}"))?
        };

        // No-FFmpeg build: return explicit gate result, no output file.
        #[cfg(not(feature = "ffmpeg"))]
        let export_result: Result<
            media::trim_exporter::TrimExportResult,
            crate::app::error::AppError,
        > = {
            let _ = (
                &source_path,
                &output_path,
                &export_preset,
                &cut_timeline,
                &effect_timeline,
                &progress_reporter,
                &sequence,
            );
            Err(crate::app::error::AppError::ExportFailed {
                reason: "当前构建未启用 FFmpeg，无法导出视频。请使用 `--features ffmpeg` 构建。"
                    .to_string(),
            })
        };

        // Validate artifact is playable (streams, dimensions, duration, audio contract).
        #[cfg(feature = "ffmpeg")]
        if let Ok(ref result) = export_result {
            let spec = export_preset.spec();
            let contract = {
                let svc = state
                    .service
                    .lock()
                    .map_err(|_| "录制服务锁已损坏".to_string())?;
                let requested_system_audio = svc.last_requested_system_audio();
                let requested_microphone = svc.last_requested_microphone();
                media::ffmpeg_common::RequestedAudioContract {
                    requested_system_audio,
                    requested_microphone,
                    // BUG-0013: Allow silent audio when only system audio was requested.
                    // This handles the legitimate case where the user captures system audio
                    // but the system has no audio output during the recording session.
                    allow_silent_if_system_only: requested_system_audio && !requested_microphone,
                    ..Default::default()
                }
            };
            if let Err(error) = media::ffmpeg_common::validate_export_artifact_with_audio_contract(
                &result.output_path,
                spec.width,
                spec.height,
                &contract,
            ) {
                let _ = std::fs::remove_file(&result.output_path);
                return Err(error.to_string());
            }
        }

        let result = export_result.map_err(|error| error.to_string())?;

        // Emit final progress with output path.
        let _ = app.emit(
            "export-progress",
            ExportProgressPayload {
                preset: preset_id,
                progress: 100,
                cancellable: false,
                output_path: Some(result.output_path.to_string_lossy().to_string()),
                error: None,
            },
        );

        let cut_count = cut.as_ref().map(|s| s.cut_count).unwrap_or(0);
        let total_cut_nanos = cut.as_ref().map(|s| s.total_cut_nanos).unwrap_or(0);
        let cut_timeline_path = cut.map(|s| s.cut_timeline_path);
        Ok(ExportSummaryPayload {
            frame_count: cursor.frame_count,
            click_effect_count: cursor.click_effect_count,
            effect_timeline_path: cursor.effect_timeline_path,
            cut_count,
            total_cut_nanos,
            cut_timeline_path,
            output_path: Some(result.output_path.to_string_lossy().to_string()),
        })
    };

    // Execute the inner export logic and handle cleanup on all paths.
    let result: Result<ExportSummaryPayload, String> = do_export.await;

    // Always clear the cancel token — this handles ALL early return paths.
    {
        let mut guard = state
            .export_cancel_token
            .lock()
            .map_err(|_| "导出取消状态锁已损坏".to_string())?;
        *guard = None;
    }

    // Emit terminal progress on failure/cancel so UI can clean up.
    // Also emit when Ok has output_path: None (no-FFmpeg gate) to ensure
    // UI clears the exporting/cancellable state.
    match &result {
        Err(error_msg) => {
            let error_str = error_msg.to_string();
            let is_cancel = error_str.contains("ExportCancelled") || error_str.contains("已取消");
            let _ = app.emit(
                "export-progress",
                ExportProgressPayload {
                    preset: preset_id,
                    progress: 0,
                    cancellable: false,
                    output_path: None,
                    error: Some(if is_cancel {
                        "导出已取消".to_string()
                    } else {
                        error_str
                    }),
                },
            );
        }
        Ok(ref payload) if payload.output_path.is_none() => {
            // No-FFmpeg gate or other gate condition: emit terminal progress
            // so UI clears the exporting/cancellable state.
            let _ = app.emit(
                "export-progress",
                ExportProgressPayload {
                    preset: preset_id,
                    progress: 0,
                    cancellable: false,
                    output_path: None,
                    error: Some("当前构建未启用 FFmpeg，无法生成可播放文件".to_string()),
                },
            );
        }
        _ => {}
    }

    result
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
    recording_id: Option<String>,
) -> Result<CutTimelineSummaryPayload, String> {
    let is_history_export = recording_id.is_some();

    // Skip active recording check for history export.
    if !is_history_export {
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
    // For history export, resolve from the library and skip session tracking.
    let (session_id, trim_metadata_path, build_revision) = if is_history_export {
        let lib = state
            .library
            .lock()
            .map_err(|_| "录制库锁已损坏".to_string())?;
        let ctx = lib
            .get_recording(recording_id.as_ref().unwrap())
            .map_err(|e| e.to_string())?;
        let path = ctx
            .entry
            .trim_metadata_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .ok_or_else(|| "历史录制缺少裁剪元数据路径".to_string())?;
        (0u64, path, 0u64)
    } else {
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

    // For history export, skip session validation and service write-back.
    if !is_history_export {
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
        .plugin(tauri_plugin_dialog::init())
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
            list_microphone_devices,
            set_beautify_config,
            get_beautify_config,
            build_cursor_effect_timeline,
            get_cursor_effect_timeline,
            build_cut_timeline,
            export_video,
            cancel_export,
            license_status,
            activation_status,
            activate_license,
            list_recordings,
            get_recording_context,
            import_recording,
            delete_recording,
            list_windows,
            set_window_id,
            open_exported_file_location
        ])
        .setup(|app| {
            // Initialize recording library with proper app data dir.
            if let Ok(data_dir) = app.path().app_data_dir() {
                let state = app.state::<AppState>();
                state.init_library(&data_dir);
            }
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{CaptureConfig, CaptureMode};
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::{ClickPhase, CursorClick, CursorKind, CursorSample, MouseButton};

    fn sample(nanos: u64, x: f32, y: f32) -> CursorSample {
        CursorSample {
            timestamp: MediaTimestamp::from_nanos(nanos),
            x,
            y,
            kind: CursorKind::default(),
        }
    }

    #[test]
    fn capture_mode_update_preserves_selected_window_for_window_mode() {
        let previous = CaptureConfig {
            mode: CaptureMode::Window,
            width: 1920,
            height: 1080,
            fps: 30,
            show_system_cursor: true,
            window_id: Some(42),
        };

        let next = build_capture_config_update(
            previous,
            CaptureMode::Window,
            Some(1280),
            Some(720),
            60,
            false,
        );

        assert_eq!(next.window_id, Some(42));
    }

    #[test]
    fn capture_mode_update_rejects_unsupported_fps() {
        let previous = CaptureConfig::full_screen_1080p_30fps();
        let payload = SetCaptureModePayload {
            mode: "fullscreen".to_string(),
            width: Some(1920),
            height: Some(1080),
            fps: Some(120),
            window_id: None,
        };

        let result = apply_capture_config_payload(previous, payload, true);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("fps"));
    }

    #[test]
    fn window_state_actions_pause_resume_and_stop_recording() {
        use crate::core::window::WindowRecordingState;

        assert_eq!(
            action_for_window_state(WindowRecordingState::Minimized),
            WindowStateAction::Pause
        );
        assert_eq!(
            action_for_window_state(WindowRecordingState::Recording),
            WindowStateAction::Resume
        );
        assert_eq!(
            action_for_window_state(WindowRecordingState::Closed),
            WindowStateAction::Stop
        );
    }

    #[test]
    fn macos_export_location_command_reveals_file_in_finder() {
        let command = build_exported_file_location_command(
            Path::new("/tmp/luzhi/exported video.mp4"),
            ExportedFileLocationPlatform::Macos,
        )
        .unwrap();

        assert_eq!(command.program, "open");
        assert_eq!(command.args, vec!["-R", "/tmp/luzhi/exported video.mp4"]);
    }

    #[test]
    fn windows_export_location_command_selects_file_in_explorer() {
        let command = build_exported_file_location_command(
            Path::new(r"C:\Users\demo\Videos\exported video.mp4"),
            ExportedFileLocationPlatform::Windows,
        )
        .unwrap();

        assert_eq!(command.program, "explorer.exe");
        assert_eq!(
            command.args,
            vec![r"/select,C:\Users\demo\Videos\exported video.mp4"]
        );
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
            capture_geometry: None,
            cursor_kind_diagnostics: None,
            media_timeline_diagnostics: None,
            cursor_timing_diagnostics: None,
            source_pts_origin_nanos: 0,
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
