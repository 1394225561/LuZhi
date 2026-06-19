# Windows MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Windows MVP recording path so Windows can run the Tauri app and support the same primary workflow as current macOS: fullscreen/window recording, system audio, microphone, cursor metadata, preview, history, and export.

**Architecture:** Introduce a platform recording-service boundary so `src-tauri/src/lib.rs` no longer hard-codes `MacRecordingService`. Implement Windows through `WindowsRecordingService`, using Windows Graphics Capture for fullscreen/window video, WASAPI loopback for system audio, existing `cpal` microphone capture, and the existing media writer/mixer/export stack.

**Tech Stack:** Tauri 2, Rust 2021, React/Vite, Windows Graphics Capture via windows-rs, WASAPI via windows-rs/cpal, existing FFmpeg feature path, Vitest, Rust unit/integration tests.

---

## Scope And Sequencing

This plan implements the approved design in [2026-06-19-windows-mvp-design.md](../specs/2026-06-19-windows-mvp-design.md).

Implementation is intentionally staged:

1. Open the Windows build path safely.
2. Add a Windows service that fails visibly before native capture exists.
3. Add Windows Graphics Capture video.
4. Add WASAPI loopback system audio.
5. Reuse cpal microphone and existing mixer/writer/export.
6. Add Windows window capture and cursor metadata.
7. Add Windows manual gates and docs.

Do not add area recording, activation-code protocol, D3D/FFmpeg zero-copy, or 4K/60fps stability work in this plan.

---

## File Structure

### New Files

| File | Responsibility |
|------|----------------|
| `src-tauri/src/app/recording_service_boundary.rs` | Platform-neutral trait used by `AppState` and Tauri commands. |
| `src-tauri/src/platform/windows_service.rs` | Windows recording orchestrator. Starts/stops WGC/WASAPI/cpal/cursor and reuses writer/mixer flow. |
| `src-tauri/src/platform/windows/graphics_capture.rs` | Windows Graphics Capture display/window video implementation. |
| `src-tauri/src/platform/windows/cursor_source.rs` | Windows cursor snapshot source for coordinates and click states. |
| `src-tauri/src/platform/windows/audio_device.rs` | WASAPI device-format helpers used by `wasapi_loopback.rs`. |
| `tests/2026-06-19-windows-mvp-checklist.md` | Manual Windows MVP acceptance checklist. |

### Modified Files

| File | Responsibility |
|------|----------------|
| `src-tauri/src/app/mod.rs` | Export the new recording service boundary. |
| `src-tauri/src/app/cursor_metadata_runtime.rs` | Remove macOS-only diagnostic dependency from common runtime. |
| `src-tauri/src/platform/macos_service.rs` | Implement the platform recording-service trait for `MacRecordingService`; keep behavior unchanged. |
| `src-tauri/src/platform/macos/cursor_kind.rs` | Keep AppKit cursor-kind diagnostics behind macOS module. |
| `src-tauri/src/platform/macos/cursor_source.rs` | Keep macOS cursor source unchanged except for imported trait paths if needed. |
| `src-tauri/src/platform/mod.rs` | Export `windows_service` behind `cfg(target_os = "windows")`. |
| `src-tauri/src/platform/windows/mod.rs` | Export `graphics_capture`, `cursor_source`, and `audio_device`. |
| `src-tauri/src/platform/windows/wasapi_loopback.rs` | Replace stub with WASAPI loopback capture. |
| `src-tauri/src/platform/windows/window_capture.rs` | Replace stub with window enumeration/status and WGC target helpers. |
| `src-tauri/src/lib.rs` | Replace macOS-only `AppState.service`, remove `compile_error!`, add platform service factory and cursor dispatcher shims. |
| `src-tauri/Cargo.toml` | Add Windows-only windows-rs dependency/features. |
| `docs/platform-diff/windows-development-environment.md` | Add WGC/WASAPI validation notes after implementation. |

---

## Task 1: Add Platform Recording Service Boundary

**Files:**
- Create: `src-tauri/src/app/recording_service_boundary.rs`
- Modify: `src-tauri/src/app/mod.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`

- [ ] **Step 1: Write the boundary trait**

Create `src-tauri/src/app/recording_service_boundary.rs`:

```rust
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Receiver;

use crate::app::error::AppResult;
use crate::app::state_machine::RecordingState;
use crate::core::capture::AudioConfig;
use crate::core::config::CaptureConfig;
use crate::core::timeline::BeautifyConfigSnapshot;
use crate::core::window::WindowRecordingState;
use crate::media::recording_writer::StopRecordingResponse;

/// Runs platform-specific main-thread work needed by cursor metadata capture.
///
/// macOS uses this to read AppKit cursor state. Windows implementations can
/// execute the task inline because the planned cursor source does not need an
/// AppKit-style main-thread hop.
pub trait CursorMainThreadDispatcher: Send + Sync + 'static {
    fn run_on_main_thread(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String>;
}

/// Platform service API consumed by Tauri command handlers.
///
/// This is intentionally shaped around the existing `MacRecordingService`
/// surface so the first refactor can preserve macOS behavior.
pub trait PlatformRecordingService: Send {
    fn state(&self) -> RecordingState;

    fn start(
        &mut self,
        config: CaptureConfig,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()>;

    fn start_window(
        &mut self,
        window_id: u32,
        show_system_cursor: bool,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()>;

    fn stop(&mut self) -> AppResult<StopRecordingResponse>;
    fn pause(&mut self) -> AppResult<()>;
    fn resume(&mut self) -> AppResult<()>;

    fn mic_level_ref(&self) -> Arc<Mutex<f64>>;
    fn take_window_state_receiver(&mut self) -> Option<Receiver<WindowRecordingState>>;

    fn current_session_id(&self) -> u64;
    fn last_cursor_metadata_path(&self) -> Option<String>;
    fn last_effect_timeline_path(&self) -> Option<String>;
    fn set_last_effect_timeline_path(&mut self, path: Option<String>);
    fn last_trim_metadata_path(&self) -> Option<String>;
    fn last_cut_timeline_path(&self) -> Option<String>;
    fn set_last_cut_timeline_path(&mut self, path: Option<String>);
    fn last_recording_output_path(&self) -> Option<String>;
    fn last_requested_system_audio(&self) -> bool;
    fn last_requested_microphone(&self) -> bool;
}
```

- [ ] **Step 2: Export the module**

Modify `src-tauri/src/app/mod.rs`:

```rust
pub mod cursor_metadata_runtime;
pub mod error;
pub mod events;
pub mod export_service;
pub mod license_service;
pub mod mic_level_runtime;
pub mod permission_service;
pub mod recording_library;
pub mod recording_runtime;
pub mod recording_service;
pub mod recording_service_boundary;
pub mod state_machine;
```

- [ ] **Step 3: Move the dispatcher import for macOS service**

In `src-tauri/src/platform/macos_service.rs`, replace:

```rust
use super::macos::cursor_kind::CursorMainThreadDispatcher;
```

with:

```rust
use crate::app::recording_service_boundary::{
    CursorMainThreadDispatcher, PlatformRecordingService,
};
```

- [ ] **Step 4: Implement the trait for `MacRecordingService`**

Append this implementation near the existing `impl Default for MacRecordingService`:

```rust
impl PlatformRecordingService for MacRecordingService {
    fn state(&self) -> RecordingState {
        MacRecordingService::state(self)
    }

    fn start(
        &mut self,
        config: CaptureConfig,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        MacRecordingService::start(
            self,
            config,
            audio_config,
            beautify_snapshot,
            cursor_main_thread_dispatcher,
        )
    }

    fn start_window(
        &mut self,
        window_id: u32,
        show_system_cursor: bool,
        audio_config: AudioConfig,
        beautify_snapshot: BeautifyConfigSnapshot,
        cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        MacRecordingService::start_window(
            self,
            window_id,
            show_system_cursor,
            audio_config,
            beautify_snapshot,
            cursor_main_thread_dispatcher,
        )
    }

    fn stop(&mut self) -> AppResult<StopRecordingResponse> {
        MacRecordingService::stop(self)
    }

    fn pause(&mut self) -> AppResult<()> {
        MacRecordingService::pause(self)
    }

    fn resume(&mut self) -> AppResult<()> {
        MacRecordingService::resume(self)
    }

    fn mic_level_ref(&self) -> Arc<Mutex<f64>> {
        MacRecordingService::mic_level_ref(self)
    }

    fn take_window_state_receiver(&mut self) -> Option<Receiver<WindowRecordingState>> {
        MacRecordingService::take_window_state_receiver(self)
    }

    fn current_session_id(&self) -> u64 {
        MacRecordingService::current_session_id(self)
    }

    fn last_cursor_metadata_path(&self) -> Option<String> {
        MacRecordingService::last_cursor_metadata_path(self)
    }

    fn last_effect_timeline_path(&self) -> Option<String> {
        MacRecordingService::last_effect_timeline_path(self)
    }

    fn set_last_effect_timeline_path(&mut self, path: Option<String>) {
        MacRecordingService::set_last_effect_timeline_path(self, path);
    }

    fn last_trim_metadata_path(&self) -> Option<String> {
        MacRecordingService::last_trim_metadata_path(self)
    }

    fn last_cut_timeline_path(&self) -> Option<String> {
        self.last_cut_timeline_path.clone()
    }

    fn set_last_cut_timeline_path(&mut self, path: Option<String>) {
        MacRecordingService::set_last_cut_timeline_path(self, path);
    }

    fn last_recording_output_path(&self) -> Option<String> {
        MacRecordingService::last_recording_output_path(self)
    }

    fn last_requested_system_audio(&self) -> bool {
        MacRecordingService::last_requested_system_audio(self)
    }

    fn last_requested_microphone(&self) -> bool {
        MacRecordingService::last_requested_microphone(self)
    }
}
```

If `last_cut_timeline_path` is private without a getter, add this inherent getter beside the other `last_*` methods:

```rust
pub fn last_cut_timeline_path(&self) -> Option<String> {
    self.last_cut_timeline_path.clone()
}
```

Then use `MacRecordingService::last_cut_timeline_path(self)` in the trait implementation.

- [ ] **Step 5: Run targeted tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib window_state_actions_pause_resume_and_stop_recording
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/app/recording_service_boundary.rs src-tauri/src/app/mod.rs src-tauri/src/platform/macos_service.rs
git commit -m "refactor(app): add platform recording service boundary"
```

---

## Task 2: Make Cursor Diagnostics Platform-Neutral

**Files:**
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs`
- Modify: `src-tauri/src/platform/macos/cursor_kind.rs`
- Modify: `src-tauri/src/platform/macos/cursor_source.rs`

- [ ] **Step 1: Move `CursorKindDiagnostics` to common runtime**

In `src-tauri/src/app/cursor_metadata_runtime.rs`, remove:

```rust
use crate::platform::macos::cursor_kind::cursor_kind_diagnostics_merged;
```

Add this common diagnostic type and helper near the constants:

```rust
/// Diagnostics for cursor kind classification.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorKindDiagnostics {
    pub ax_query_failure_count: u64,
    pub ax_fallback_arrow_count: u64,
    pub arrow_count: u64,
    pub hand_count: u64,
    pub ibeam_count: u64,
}

fn cursor_kind_diagnostics_from_counts(
    arrow_count: u64,
    hand_count: u64,
    ibeam_count: u64,
) -> CursorKindDiagnostics {
    CursorKindDiagnostics {
        ax_query_failure_count: 0,
        ax_fallback_arrow_count: 0,
        arrow_count,
        hand_count,
        ibeam_count,
    }
}
```

Change `finish()` to use the common helper:

```rust
cursor_kind_diagnostics: Some(cursor_kind_diagnostics_from_counts(
    self.arrow_count,
    self.hand_count,
    self.ibeam_count,
)),
```

- [ ] **Step 2: Update recording metadata import**

In `src-tauri/src/media/recording_metadata.rs`, replace any import of `platform::macos::cursor_kind::CursorKindDiagnostics` with:

```rust
use crate::app::cursor_metadata_runtime::CursorKindDiagnostics;
```

- [ ] **Step 3: Preserve macOS AX diagnostic merge**

In `src-tauri/src/platform/macos/cursor_kind.rs`, remove the local `CursorKindDiagnostics` struct and import the common one:

```rust
use crate::app::cursor_metadata_runtime::CursorKindDiagnostics;
```

Keep `cursor_kind_diagnostics_merged()` returning the common struct.

In `src-tauri/src/platform/macos_service.rs`, after stopping cursor runtime and before writing metadata, merge macOS AX counters into the metadata:

```rust
if let Some(ref mut metadata) = cursor_metadata {
    if let Some(ref diag) = metadata.cursor_kind_diagnostics {
        metadata.cursor_kind_diagnostics = Some(
            crate::platform::macos::cursor_kind::cursor_kind_diagnostics_merged(
                diag.arrow_count,
                diag.hand_count,
                diag.ibeam_count,
            ),
        );
    }
}
```

Place this immediately after `cursor_runtime.stop()` returns metadata in the existing stop/finalize flow.

- [ ] **Step 4: Run cursor metadata tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib cursor_metadata_runtime
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/app/cursor_metadata_runtime.rs src-tauri/src/media/recording_metadata.rs src-tauri/src/platform/macos/cursor_kind.rs src-tauri/src/platform/macos_service.rs
git commit -m "refactor(cursor): make cursor diagnostics platform-neutral"
```

---

## Task 3: Add WindowsRecordingService Stub And Open Windows Build Path

**Files:**
- Create: `src-tauri/src/platform/windows_service.rs`
- Modify: `src-tauri/src/platform/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create Windows service stub**

Create `src-tauri/src/platform/windows_service.rs`:

```rust
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Receiver;

use crate::app::error::{AppError, AppResult};
use crate::app::recording_service_boundary::{
    CursorMainThreadDispatcher, PlatformRecordingService,
};
use crate::app::state_machine::{RecordingState, RecordingStateMachine};
use crate::core::capture::AudioConfig;
use crate::core::config::CaptureConfig;
use crate::core::timeline::BeautifyConfigSnapshot;
use crate::core::window::WindowRecordingState;
use crate::media::recording_writer::{
    RecordingDiagnostics, RecordingResult, StopRecordingResponse, WriterDiagnostics,
};

pub struct WindowsRecordingService {
    state_machine: RecordingStateMachine,
    mic_level: Arc<Mutex<f64>>,
    session_id: u64,
    last_cursor_metadata_path: Option<String>,
    last_effect_timeline_path: Option<String>,
    last_trim_metadata_path: Option<String>,
    last_cut_timeline_path: Option<String>,
    last_recording_output_path: Option<String>,
    last_requested_system_audio: bool,
    last_requested_microphone: bool,
}

impl WindowsRecordingService {
    pub fn new() -> Self {
        Self {
            state_machine: RecordingStateMachine::new(),
            mic_level: Arc::new(Mutex::new(0.0)),
            session_id: 0,
            last_cursor_metadata_path: None,
            last_effect_timeline_path: None,
            last_trim_metadata_path: None,
            last_cut_timeline_path: None,
            last_recording_output_path: None,
            last_requested_system_audio: false,
            last_requested_microphone: false,
        }
    }

    fn unavailable(reason: &'static str) -> AppError {
        AppError::NativeCaptureUnavailable { reason }
    }
}

impl Default for WindowsRecordingService {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformRecordingService for WindowsRecordingService {
    fn state(&self) -> RecordingState {
        self.state_machine.state()
    }

    fn start(
        &mut self,
        _config: CaptureConfig,
        audio_config: AudioConfig,
        _beautify_snapshot: BeautifyConfigSnapshot,
        _cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        self.last_requested_system_audio = audio_config.capture_system_audio;
        self.last_requested_microphone = audio_config.capture_microphone;
        self.session_id = self.session_id.wrapping_add(1);
        self.state_machine.fail();
        Err(Self::unavailable("Windows Graphics Capture 尚未接入"))
    }

    fn start_window(
        &mut self,
        _window_id: u32,
        _show_system_cursor: bool,
        audio_config: AudioConfig,
        _beautify_snapshot: BeautifyConfigSnapshot,
        _cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> AppResult<()> {
        self.last_requested_system_audio = audio_config.capture_system_audio;
        self.last_requested_microphone = audio_config.capture_microphone;
        self.session_id = self.session_id.wrapping_add(1);
        self.state_machine.fail();
        Err(Self::unavailable("Windows 窗口录制尚未接入"))
    }

    fn stop(&mut self) -> AppResult<StopRecordingResponse> {
        if let Ok(mut level) = self.mic_level.lock() {
            *level = 0.0;
        }
        Ok(StopRecordingResponse {
            result: RecordingResult {
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
                finalization_errors: vec!["Windows 录制尚未启动".to_string()],
            },
            failed: true,
        })
    }

    fn pause(&mut self) -> AppResult<()> {
        Err(Self::unavailable("Windows 录制尚未启动"))
    }

    fn resume(&mut self) -> AppResult<()> {
        Err(Self::unavailable("Windows 录制尚未启动"))
    }

    fn mic_level_ref(&self) -> Arc<Mutex<f64>> {
        self.mic_level.clone()
    }

    fn take_window_state_receiver(&mut self) -> Option<Receiver<WindowRecordingState>> {
        None
    }

    fn current_session_id(&self) -> u64 {
        self.session_id
    }

    fn last_cursor_metadata_path(&self) -> Option<String> {
        self.last_cursor_metadata_path.clone()
    }

    fn last_effect_timeline_path(&self) -> Option<String> {
        self.last_effect_timeline_path.clone()
    }

    fn set_last_effect_timeline_path(&mut self, path: Option<String>) {
        self.last_effect_timeline_path = path;
    }

    fn last_trim_metadata_path(&self) -> Option<String> {
        self.last_trim_metadata_path.clone()
    }

    fn last_cut_timeline_path(&self) -> Option<String> {
        self.last_cut_timeline_path.clone()
    }

    fn set_last_cut_timeline_path(&mut self, path: Option<String>) {
        self.last_cut_timeline_path = path;
    }

    fn last_recording_output_path(&self) -> Option<String> {
        self.last_recording_output_path.clone()
    }

    fn last_requested_system_audio(&self) -> bool {
        self.last_requested_system_audio
    }

    fn last_requested_microphone(&self) -> bool {
        self.last_requested_microphone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::capture::DenoiseMode;
    use crate::core::config::CaptureConfig;

    #[test]
    fn windows_service_fails_visibly_before_native_capture_exists() {
        let mut service = WindowsRecordingService::new();
        let err = service
            .start(
                CaptureConfig::full_screen_1080p_30fps(),
                AudioConfig {
                    capture_system_audio: true,
                    capture_microphone: true,
                    microphone_device: None,
                    sample_rate: 48000,
                    channels: 2,
                    denoise_mode: DenoiseMode::default(),
                },
                BeautifyConfigSnapshot {
                    cursor_magnification: true,
                    magnification_factor: 2.0,
                    cursor_smoothing: true,
                    auto_trim_silences: false,
                    trim_sensitivity: "medium".to_string(),
                    raw_system_cursor_visible: false,
                },
                Box::new(InlineDispatcher),
            )
            .unwrap_err();

        assert!(err.to_string().contains("Windows Graphics Capture"));
        assert_eq!(service.state(), RecordingState::Failed);
        assert!(service.last_recording_output_path().is_none());
    }

    struct InlineDispatcher;

    impl CursorMainThreadDispatcher for InlineDispatcher {
        fn run_on_main_thread(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            task();
            Ok(())
        }
    }
}
```

- [ ] **Step 2: Export Windows service**

Modify `src-tauri/src/platform/mod.rs`:

```rust
#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub mod macos_service;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "windows")]
pub mod windows_service;
```

- [ ] **Step 3: Replace the service type in `lib.rs`**

In `src-tauri/src/lib.rs`, replace the macOS-only imports and compile error:

```rust
use app::recording_service_boundary::{
    CursorMainThreadDispatcher, PlatformRecordingService,
};
#[cfg(target_os = "macos")]
use platform::macos_service::MacRecordingService;
#[cfg(target_os = "windows")]
use platform::windows_service::WindowsRecordingService;
```

Delete:

```rust
#[cfg(not(target_os = "macos"))]
compile_error!("LuZhi recording service currently supports macOS builds only; Windows app wiring requires a WindowsRecordingService.");
```

Change `AppState.service`:

```rust
service: Arc<Mutex<Box<dyn PlatformRecordingService>>>,
```

Add a service factory near `AppState`:

```rust
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
```

Change `AppState::default()`:

```rust
service: Arc::new(Mutex::new(create_platform_recording_service())),
```

- [ ] **Step 4: Keep `TauriCursorMainThreadDispatcher` platform-neutral**

In `src-tauri/src/lib.rs`, remove `#[cfg(target_os = "macos")]` from `TauriCursorMainThreadDispatcher` and its trait impl. The implementation can continue to call `app.run_on_main_thread(task)` on all Tauri desktop targets:

```rust
#[derive(Clone)]
struct TauriCursorMainThreadDispatcher {
    app: AppHandle,
}

impl CursorMainThreadDispatcher for TauriCursorMainThreadDispatcher {
    fn run_on_main_thread(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        self.app.run_on_main_thread(task).map_err(|e| e.to_string())
    }
}
```

- [ ] **Step 5: Run non-Windows regression**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib windows_service_fails_visibly_before_native_capture_exists
cargo test --manifest-path src-tauri/Cargo.toml --lib window_state_actions_pause_resume_and_stop_recording
```

Expected: both PASS on the current platform or Windows-service test is not compiled on non-Windows if the module is Windows-gated. If the test is Windows-gated, add a non-Windows compile test around `create_platform_recording_service()` instead.

- [ ] **Step 6: Run Windows build check on Windows**

Run on a Windows machine:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: no `compile_error!`; any remaining errors identify concrete macOS-only imports that must be gated.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/platform/windows_service.rs src-tauri/src/platform/mod.rs src-tauri/src/lib.rs
git commit -m "feat(windows): add stub recording service and open build path"
```

---

## Task 4: Add Windows API Dependency Behind Windows Target

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`

- [ ] **Step 1: Add windows-rs dependency**

Run:

```powershell
cargo add windows --target 'cfg(windows)' --manifest-path src-tauri/Cargo.toml --features Win32_Foundation,Win32_Graphics_Direct3D,Win32_Graphics_Direct3D11,Win32_Graphics_Dxgi,Win32_Graphics_Dxgi_Common,Win32_Graphics_Gdi,Win32_Media_Audio,Win32_System_Com,Win32_UI_WindowsAndMessaging,Graphics_Capture,Graphics_DirectX,Graphics_DirectX_Direct3D11,Foundation
```

Expected: `src-tauri/Cargo.toml` gets a target-specific dependency similar to:

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "...", features = [
    "Foundation",
    "Graphics_Capture",
    "Graphics_DirectX",
    "Graphics_DirectX_Direct3D11",
    "Win32_Foundation",
    "Win32_Graphics_Direct3D",
    "Win32_Graphics_Direct3D11",
    "Win32_Graphics_Dxgi",
    "Win32_Graphics_Dxgi_Common",
    "Win32_Graphics_Gdi",
    "Win32_Media_Audio",
    "Win32_System_Com",
    "Win32_UI_WindowsAndMessaging",
] }
```

- [ ] **Step 2: Verify non-Windows dependency isolation**

Run:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS on macOS/Linux without trying to compile Windows-only modules.

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore(windows): add windows-rs capture dependencies"
```

---

## Task 5: Implement Windows Graphics Capture Frame Helpers

**Files:**
- Create: `src-tauri/src/platform/windows/graphics_capture.rs`
- Modify: `src-tauri/src/platform/windows/mod.rs`

- [ ] **Step 1: Create pure helper types and tests first**

Create `src-tauri/src/platform/windows/graphics_capture.rs`:

```rust
use std::sync::Arc;

use crate::core::frame::{
    FrameBuffer, MediaTimestamp, PixelFormat, VideoFrame, VideoFrameRef,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramePoolState {
    pub size: CaptureSize,
}

impl FramePoolState {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            size: CaptureSize { width, height },
        }
    }

    pub fn needs_recreate(&self, next: CaptureSize) -> bool {
        self.size != next && next.width > 0 && next.height > 0
    }
}

pub fn system_relative_time_to_timestamp(
    first_raw_nanos: &mut Option<u64>,
    raw_nanos: u64,
) -> MediaTimestamp {
    let origin = match *first_raw_nanos {
        Some(origin) => origin,
        None => {
            *first_raw_nanos = Some(raw_nanos);
            raw_nanos
        }
    };
    MediaTimestamp::from_nanos(raw_nanos.saturating_sub(origin))
}

pub fn owned_bgra_frame(
    timestamp: MediaTimestamp,
    width: u32,
    height: u32,
    stride_bytes: usize,
    bytes: Vec<u8>,
) -> VideoFrameRef {
    Arc::new(VideoFrame {
        timestamp,
        width,
        height,
        stride_bytes,
        pixel_format: PixelFormat::Bgra8,
        buffer: FrameBuffer::Owned(Arc::from(bytes.into_boxed_slice())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_normalizer_starts_first_wgc_frame_at_zero() {
        let mut first = None;

        let a = system_relative_time_to_timestamp(&mut first, 10_000_000);
        let b = system_relative_time_to_timestamp(&mut first, 43_333_333);

        assert_eq!(a.nanos, 0);
        assert_eq!(b.nanos, 33_333_333);
    }

    #[test]
    fn frame_pool_recreate_only_for_real_size_changes() {
        let state = FramePoolState::new(1920, 1080);

        assert!(!state.needs_recreate(CaptureSize { width: 1920, height: 1080 }));
        assert!(state.needs_recreate(CaptureSize { width: 1280, height: 720 }));
        assert!(!state.needs_recreate(CaptureSize { width: 0, height: 720 }));
    }

    #[test]
    fn owned_bgra_frame_carries_expected_metadata() {
        let frame = owned_bgra_frame(
            MediaTimestamp::from_nanos(7),
            2,
            2,
            8,
            vec![0; 16],
        );

        assert_eq!(frame.timestamp.nanos, 7);
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 2);
        assert_eq!(frame.stride_bytes, 8);
        assert_eq!(frame.pixel_format, PixelFormat::Bgra8);
    }
}
```

- [ ] **Step 2: Export the module**

Modify `src-tauri/src/platform/windows/mod.rs`:

```rust
pub mod dxgi_capture;
pub mod graphics_capture;
pub mod wasapi_loopback;
pub mod window_capture;
```

- [ ] **Step 3: Run helper tests**

Run on Windows:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib graphics_capture
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/platform/windows/graphics_capture.rs src-tauri/src/platform/windows/mod.rs
git commit -m "feat(windows): add graphics capture frame helpers"
```

---

## Task 6: Implement Windows Graphics Capture Fullscreen Adapter

**Files:**
- Modify: `src-tauri/src/platform/windows/graphics_capture.rs`
- Modify: `src-tauri/src/platform/windows_service.rs`

- [ ] **Step 1: Add adapter skeleton with explicit start/stop lifecycle**

Append to `graphics_capture.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crate::app::error::{AppError, AppResult};
use crate::core::capture::VideoFrameSink;
use crate::core::clock::SessionClock;
use crate::core::config::CaptureConfig;

pub struct WindowsGraphicsCapture {
    stop_flag: Option<Arc<AtomicBool>>,
    worker: Option<thread::JoinHandle<AppResult<()>>>,
}

impl WindowsGraphicsCapture {
    pub fn new() -> Self {
        Self {
            stop_flag: None,
            worker: None,
        }
    }

    pub fn start_display(
        &mut self,
        config: CaptureConfig,
        video_sink: VideoFrameSink,
        session_clock: Arc<SessionClock>,
    ) -> AppResult<()> {
        if self.worker.is_some() {
            return Err(AppError::InvalidState {
                current: "recording",
                action: "start_display",
            });
        }

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        self.stop_flag = Some(stop);

        self.worker = Some(thread::spawn(move || {
            run_display_capture_worker(config, video_sink, session_clock, thread_stop)
        }));

        Ok(())
    }

    pub fn stop(&mut self) -> AppResult<()> {
        if let Some(stop) = self.stop_flag.take() {
            stop.store(true, Ordering::Relaxed);
        }

        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(result) => result,
                Err(_) => Err(AppError::CaptureFailed {
                    reason: "Windows Graphics Capture 线程崩溃".to_string(),
                }),
            }
        } else {
            Ok(())
        }
    }
}

impl Default for WindowsGraphicsCapture {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2: Implement worker with Windows-only API**

Add this Windows-only worker in the same file:

```rust
#[cfg(target_os = "windows")]
fn run_display_capture_worker(
    config: CaptureConfig,
    video_sink: VideoFrameSink,
    session_clock: Arc<SessionClock>,
    stop: Arc<AtomicBool>,
) -> AppResult<()> {
    use std::time::Duration;

    let result = unsafe { run_display_capture_worker_inner(config, video_sink, session_clock, stop) };
    if let Err(ref error) = result {
        eprintln!("[windows-graphics-capture] {error}");
    }
    std::thread::sleep(Duration::from_millis(1));
    result
}

#[cfg(not(target_os = "windows"))]
fn run_display_capture_worker(
    _config: CaptureConfig,
    _video_sink: VideoFrameSink,
    _session_clock: Arc<SessionClock>,
    _stop: Arc<AtomicBool>,
) -> AppResult<()> {
    Err(AppError::NativeCaptureUnavailable {
        reason: "Windows Graphics Capture 仅支持 Windows",
    })
}
```

Then implement `run_display_capture_worker_inner()` using the Microsoft `Windows.Graphics.Capture` pattern:

```rust
#[cfg(target_os = "windows")]
unsafe fn run_display_capture_worker_inner(
    config: CaptureConfig,
    video_sink: VideoFrameSink,
    session_clock: Arc<SessionClock>,
    stop: Arc<AtomicBool>,
) -> AppResult<()> {
    use std::time::Duration;
    use windows::Graphics::Capture::GraphicsCaptureSession;

    if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
        return Err(AppError::NativeCaptureUnavailable {
            reason: "当前设备不支持 Windows 屏幕捕获",
        });
    }

    // Implementation detail for the worker:
    // 1. Create a D3D11 device with BGRA support.
    // 2. Convert the device into IDirect3DDevice.
    // 3. Create a display GraphicsCaptureItem for the primary display.
    // 4. Create Direct3D11CaptureFramePool::CreateFreeThreaded with
    //    DirectXPixelFormat::B8G8R8A8UIntNormalized and frame count 2.
    // 5. CreateCaptureSession(item), call StartCapture().
    // 6. In FrameArrived, TryGetNextFrame(), copy the surface into CPU-readable
    //    staging texture, map it, copy each row into owned Vec<u8>, and send
    //    owned_bgra_frame(...) through video_sink.
    // 7. Use frame.SystemRelativeTime() to produce the video timestamp. If the
    //    WinRT timestamp is unavailable on the crate version, use
    //    session_clock.elapsed_nanos() and record a diagnostic warning.
    // 8. Recreate frame pool when ContentSize changes.
    // 9. Exit promptly when stop is set.
    //
    // Keep all COM/WinRT resources inside this worker thread.
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
    }

    let _ = config;
    let _ = video_sink;
    let _ = session_clock;
    Ok(())
}
```

- [ ] **Step 3: Implement real frame capture before committing**

Fill `run_display_capture_worker_inner()` with actual WGC code following the numbered comments. This task is not complete until the worker sends real `VideoFrameRef` values to the service consumer. The worker is accepted only when this manual smoke test passes:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri:dev
```

Manual action: start a fullscreen recording with system audio and microphone disabled. Expected: at least one `VideoFrameRef` reaches the Windows service consumer in Task 9. If Task 9 is not yet implemented, add a Windows-only test harness in this file that starts the adapter with a test sink and asserts a frame arrives within 3 seconds.

- [ ] **Step 4: Add native safety notes**

At the top of `graphics_capture.rs`, add:

```rust
// Native safety notes:
// - WGC frame callbacks must copy texture data into owned Rust memory before
//   returning frames to the pool.
// - D3D11 mapped textures must always be unmapped on all paths.
// - The worker thread owns COM/WinRT capture objects and stops them before join.
// - No encoding, export, or UI event work runs inside FrameArrived.
```

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/platform/windows/graphics_capture.rs src-tauri/src/platform/windows_service.rs
git commit -m "feat(windows): implement graphics capture fullscreen adapter"
```

---

## Task 7: Implement WASAPI Loopback Audio Capture

**Files:**
- Create: `src-tauri/src/platform/windows/audio_device.rs`
- Modify: `src-tauri/src/platform/windows/wasapi_loopback.rs`
- Modify: `src-tauri/src/platform/windows/mod.rs`

- [ ] **Step 1: Add audio conversion helpers and tests**

Create `src-tauri/src/platform/windows/audio_device.rs`:

```rust
use std::sync::Arc;

use crate::core::clock::AudioSampleClock;
use crate::core::frame::{AudioChunk, MediaTimestamp};

pub fn interleaved_i16_to_f32(input: &[i16]) -> Vec<f32> {
    input
        .iter()
        .map(|sample| (*sample as f32 / i16::MAX as f32).clamp(-1.0, 1.0))
        .collect()
}

pub fn interleaved_f32_to_chunk(
    timestamp: MediaTimestamp,
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
) -> AudioChunk {
    AudioChunk {
        timestamp,
        sample_rate,
        channels,
        samples: Arc::from(samples.into_boxed_slice()),
    }
}

pub fn timestamp_for_packet(clock: &AudioSampleClock, sample_count: usize) -> MediaTimestamp {
    clock.timestamp_for_interleaved_sample_count(sample_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::clock::AudioSampleClock;

    #[test]
    fn i16_conversion_clamps_to_unit_range() {
        let samples = interleaved_i16_to_f32(&[i16::MIN, 0, i16::MAX]);

        assert!(samples[0] <= -1.0);
        assert_eq!(samples[1], 0.0);
        assert!(samples[2] <= 1.0);
    }

    #[test]
    fn chunk_preserves_wasapi_format_metadata() {
        let chunk = interleaved_f32_to_chunk(
            MediaTimestamp::from_nanos(5),
            44100,
            2,
            vec![0.0; 882],
        );

        assert_eq!(chunk.timestamp.nanos, 5);
        assert_eq!(chunk.sample_rate, 44100);
        assert_eq!(chunk.channels, 2);
        assert_eq!(chunk.samples.len(), 882);
    }

    #[test]
    fn packet_timestamp_advances_by_sample_frames() {
        let clock = AudioSampleClock::new(48_000, 2);
        let first = timestamp_for_packet(&clock, 960);
        let second = timestamp_for_packet(&clock, 960);

        assert_eq!(first.nanos, 0);
        assert_eq!(second.nanos, 10_000_000);
    }
}
```

- [ ] **Step 2: Export helper module**

Modify `src-tauri/src/platform/windows/mod.rs`:

```rust
pub mod audio_device;
pub mod dxgi_capture;
pub mod graphics_capture;
pub mod wasapi_loopback;
pub mod window_capture;
```

- [ ] **Step 3: Replace WASAPI stub with lifecycle implementation**

Replace `src-tauri/src/platform/windows/wasapi_loopback.rs` with:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crate::app::error::{AppError, AppResult};
use crate::core::capture::{
    AudioCapabilities, AudioCapture, AudioChunkSink, AudioConfig, AudioDevice,
};
use crate::core::clock::{AudioSampleClock, SessionClock};

pub struct WasapiLoopback {
    session_clock: Option<Arc<SessionClock>>,
    stop_flag: Option<Arc<AtomicBool>>,
    worker: Option<thread::JoinHandle<AppResult<()>>>,
}

impl WasapiLoopback {
    pub fn new() -> Self {
        Self {
            session_clock: None,
            stop_flag: None,
            worker: None,
        }
    }

    pub fn set_session_clock(&mut self, session_clock: Arc<SessionClock>) {
        self.session_clock = Some(session_clock);
    }
}

impl Default for WasapiLoopback {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCapture for WasapiLoopback {
    fn start(&mut self, config: AudioConfig, sink: AudioChunkSink) -> AppResult<()> {
        if self.worker.is_some() {
            return Err(AppError::InvalidState {
                current: "recording",
                action: "start_wasapi_loopback",
            });
        }

        let session_clock = self
            .session_clock
            .clone()
            .unwrap_or_else(|| Arc::new(SessionClock::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        self.stop_flag = Some(stop);
        self.worker = Some(thread::spawn(move || {
            run_loopback_worker(config, sink, session_clock, thread_stop)
        }));
        Ok(())
    }

    fn stop(&mut self) -> AppResult<()> {
        if let Some(stop) = self.stop_flag.take() {
            stop.store(true, Ordering::Relaxed);
        }

        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(result) => result,
                Err(_) => Err(AppError::AudioCaptureFailed {
                    reason: "WASAPI loopback 线程崩溃".to_string(),
                }),
            }
        } else {
            Ok(())
        }
    }

    fn device_list(&self) -> AppResult<Vec<AudioDevice>> {
        Ok(vec![AudioDevice {
            id: "default-output-loopback".to_string(),
            name: "默认系统输出".to_string(),
            is_default: true,
        }])
    }

    fn capabilities(&self) -> AudioCapabilities {
        AudioCapabilities {
            supports_system_audio: true,
            supports_microphone: false,
        }
    }
}
```

- [ ] **Step 4: Implement the Windows worker**

Add to the same file:

```rust
#[cfg(target_os = "windows")]
fn run_loopback_worker(
    config: AudioConfig,
    sink: AudioChunkSink,
    session_clock: Arc<SessionClock>,
    stop: Arc<AtomicBool>,
) -> AppResult<()> {
    unsafe { run_loopback_worker_inner(config, sink, session_clock, stop) }
}

#[cfg(not(target_os = "windows"))]
fn run_loopback_worker(
    _config: AudioConfig,
    _sink: AudioChunkSink,
    _session_clock: Arc<SessionClock>,
    _stop: Arc<AtomicBool>,
) -> AppResult<()> {
    Err(AppError::NativeCaptureUnavailable {
        reason: "WASAPI loopback 仅支持 Windows",
    })
}
```

Implement `run_loopback_worker_inner()` with WASAPI:

```rust
#[cfg(target_os = "windows")]
unsafe fn run_loopback_worker_inner(
    _config: AudioConfig,
    sink: AudioChunkSink,
    session_clock: Arc<SessionClock>,
    stop: Arc<AtomicBool>,
) -> AppResult<()> {
    // Implementation detail:
    // 1. CoInitializeEx for the worker thread.
    // 2. IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole).
    // 3. Activate IAudioClient.
    // 4. GetMixFormat and derive sample_rate/channels/sample type.
    // 5. Initialize shared loopback stream with AUDCLNT_STREAMFLAGS_LOOPBACK.
    // 6. Get IAudioCaptureClient.
    // 7. Start audio client.
    // 8. Poll GetNextPacketSize, then GetBuffer/ReleaseBuffer.
    // 9. Convert PCM float or i16 to interleaved f32.
    // 10. Timestamp with AudioSampleClock lazy offset.
    // 11. Send AudioChunk through sink; record dropped chunks via MediaSender counters.
    // 12. Stop client and CoUninitialize before return.
    let _ = sink;
    let _ = session_clock;
    let _ = stop;
    run_real_wasapi_loopback_capture(_config, sink, session_clock, stop)
}
```

Add `run_real_wasapi_loopback_capture(...)` in the same Windows-only section. This task is not complete until the function reads real packets from `IAudioCaptureClient`, converts them to `AudioChunk`, and sends them through `sink`.

- [ ] **Step 5: Run tests and manual loopback smoke**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib audio_device
cargo check --manifest-path src-tauri/Cargo.toml
```

Manual smoke on Windows:

1. Play system audio.
2. Start fullscreen recording with system audio on and mic off.
3. Stop after 5 seconds.

Expected after Task 9 service integration: recording diagnostics show `requested_system_audio=true` and nonzero system chunks/windows before writer.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/platform/windows/audio_device.rs src-tauri/src/platform/windows/wasapi_loopback.rs src-tauri/src/platform/windows/mod.rs
git commit -m "feat(windows): implement wasapi loopback capture"
```

---

## Task 8: Add Windows Cursor Source

**Files:**
- Create: `src-tauri/src/platform/windows/cursor_source.rs`
- Modify: `src-tauri/src/platform/windows/mod.rs`

- [ ] **Step 1: Create cursor source**

Create `src-tauri/src/platform/windows/cursor_source.rs`:

```rust
use std::sync::Arc;

use crate::app::cursor_metadata_runtime::{
    CursorSnapshot, CursorSnapshotSource,
};
use crate::app::error::{AppError, AppResult};
use crate::core::clock::SessionClock;
use crate::core::timeline::CursorKind;

pub struct WindowsCursorSource {
    session_clock: Arc<SessionClock>,
}

impl WindowsCursorSource {
    pub fn new(session_clock: Arc<SessionClock>) -> Self {
        Self { session_clock }
    }
}

impl CursorSnapshotSource for WindowsCursorSource {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot> {
        #[cfg(target_os = "windows")]
        {
            windows_cursor_snapshot(&self.session_clock)
        }

        #[cfg(not(target_os = "windows"))]
        {
            Err(AppError::CursorProcessingFailed {
                reason: "Windows 光标采集仅支持 Windows".to_string(),
            })
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_cursor_snapshot(session_clock: &SessionClock) -> AppResult<CursorSnapshot> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAsyncKeyState, GetCursorPos, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
    };

    let start = std::time::Instant::now();
    let mut point = POINT::default();
    let ok = unsafe { GetCursorPos(&mut point).as_bool() };
    if !ok {
        return Err(AppError::CursorProcessingFailed {
            reason: "读取 Windows 鼠标位置失败".to_string(),
        });
    }

    let captured_at_nanos = session_clock.elapsed_nanos();
    let left_down = unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 };
    let right_down = unsafe { (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0 };
    let middle_down = unsafe { (GetAsyncKeyState(VK_MBUTTON.0 as i32) as u16 & 0x8000) != 0 };

    Ok(CursorSnapshot {
        x: point.x as f32,
        y: point.y as f32,
        left_down,
        right_down,
        middle_down,
        kind: CursorKind::Arrow,
        captured_at_nanos,
        snapshot_duration_nanos: start.elapsed().as_nanos() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_cursor_source_constructs_with_session_clock() {
        let source = WindowsCursorSource::new(Arc::new(SessionClock::new()));
        let _ = source;
    }
}
```

- [ ] **Step 2: Export module**

Modify `src-tauri/src/platform/windows/mod.rs`:

```rust
pub mod audio_device;
pub mod cursor_source;
pub mod dxgi_capture;
pub mod graphics_capture;
pub mod wasapi_loopback;
pub mod window_capture;
```

- [ ] **Step 3: Run test**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib windows_cursor_source_constructs_with_session_clock
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/platform/windows/cursor_source.rs src-tauri/src/platform/windows/mod.rs
git commit -m "feat(windows): add cursor metadata source"
```

---

## Task 9: Wire WindowsRecordingService To Existing Writer/Mixer Flow

**Files:**
- Modify: `src-tauri/src/platform/windows_service.rs`
- Modify: `src-tauri/src/platform/windows/graphics_capture.rs`
- Modify: `src-tauri/src/platform/windows/wasapi_loopback.rs`

- [ ] **Step 1: Add service fields**

Extend `WindowsRecordingService`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crate::app::cursor_metadata_runtime::CursorMetadataRuntime;
use crate::core::clock::SessionClock;
use crate::core::frame::{AudioChunk, VideoFrameRef};
use crate::core::media_channel::{bounded_media_channel, MediaReceiver};
use crate::media::audio_mixer::SimpleAudioMixer;
use crate::media::recording_writer::RecordingWriter;
use crate::platform::windows::cursor_source::WindowsCursorSource;
use crate::platform::windows::graphics_capture::WindowsGraphicsCapture;
use crate::platform::cpal_microphone::CpalMicrophoneCapture;
use crate::platform::windows::wasapi_loopback::WasapiLoopback;
```

If `CpalMicrophoneCapture` is still under `platform::macos`, first move `src-tauri/src/platform/macos/cpal_microphone.rs` to `src-tauri/src/platform/cpal_microphone.rs`, export it from `src-tauri/src/platform/mod.rs`, and update macOS imports. Make that a tiny commit before continuing:

```powershell
git add src-tauri/src/platform/mod.rs src-tauri/src/platform/macos/mod.rs src-tauri/src/platform/cpal_microphone.rs src-tauri/src/platform/macos_service.rs
git commit -m "refactor(audio): move cpal microphone capture to shared platform module"
```

Add fields:

```rust
graphics_capture: WindowsGraphicsCapture,
system_audio_capture: WasapiLoopback,
mic_capture: CpalMicrophoneCapture,
video_receiver: Option<MediaReceiver<VideoFrameRef>>,
system_audio_receiver: Option<MediaReceiver<AudioChunk>>,
mic_receiver: Option<MediaReceiver<AudioChunk>>,
stop_flag: Option<Arc<AtomicBool>>,
pause_flag: Arc<AtomicBool>,
consumer_handle: Option<thread::JoinHandle<()>>,
consumer_result_rx: Option<std::sync::mpsc::Receiver<RecordingConsumerOutput>>,
cursor_runtime: Option<CursorMetadataRuntime>,
```

- [ ] **Step 2: Reuse Mac consumer code without duplicating long logic**

Extract the shared consumer pieces from `macos_service.rs` into a new file `src-tauri/src/app/recording_consumer.rs`:

```rust
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
```

Move `RecordingConsumerOutput`, `consume_frames`, and its direct helper types into the shared module. Keep behavior byte-for-byte where possible. Update `MacRecordingService` to call the shared consumer. Then use the same consumer from `WindowsRecordingService`.

Verification before continuing:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib consume_frames
```

Expected: existing consume-frame tests still pass.

- [ ] **Step 3: Implement `WindowsRecordingService::start`**

`start()` should:

1. Call `state_machine.start()`.
2. Clear stale paths.
3. Increment `session_id`.
4. Reset mic level.
5. Create `SessionClock`.
6. Create video/system/mic bounded channels.
7. Start `graphics_capture.start_display(...)`.
8. Start `system_audio_capture` if `capture_system_audio`.
9. Start `mic_capture` if `capture_microphone`.
10. Start `CursorMetadataRuntime::spawn(WindowsCursorSource::new(...))`.
11. Create writer via the same feature-gated writer factory used by macOS.
12. Spawn shared consumer.

Code shape:

```rust
fn start(
    &mut self,
    config: CaptureConfig,
    audio_config: AudioConfig,
    beautify_snapshot: BeautifyConfigSnapshot,
    _cursor_main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
) -> AppResult<()> {
    self.state_machine.start()?;
    self.clear_session_paths();
    self.session_id = self.session_id.wrapping_add(1);
    self.last_requested_system_audio = audio_config.capture_system_audio;
    self.last_requested_microphone = audio_config.capture_microphone;
    if let Ok(mut level) = self.mic_level.lock() {
        *level = 0.0;
    }

    let session_clock = Arc::new(SessionClock::new());
    let (video_sender, video_receiver) = bounded_media_channel(90, "video");
    let (system_sender, system_receiver) = bounded_media_channel(256, "system");

    if let Err(error) = self.graphics_capture.start_display(
        config,
        video_sender,
        session_clock.clone(),
    ) {
        self.state_machine.fail();
        return Err(error);
    }

    if audio_config.capture_system_audio {
        self.system_audio_capture.set_session_clock(session_clock.clone());
        if let Err(error) = self.system_audio_capture.start(audio_config.clone(), system_sender) {
            let _ = self.graphics_capture.stop();
            self.state_machine.fail();
            return Err(error);
        }
        self.system_audio_receiver = Some(system_receiver);
    } else {
        self.system_audio_receiver = Some(system_receiver);
    }

    // Continue with mic/cursor/writer/shared consumer.
    Ok(())
}
```

Complete the function; do not commit the partial snippet.

- [ ] **Step 4: Implement stop/pause/resume**

`stop()` should mirror macOS cleanup order:

1. Stop cursor runtime and write cursor metadata sidecar.
2. Stop graphics capture.
3. Stop WASAPI if started.
4. Stop mic if started.
5. Signal consumer stop flag.
6. Receive consumer output with bounded timeout.
7. Write trim metadata sidecar.
8. Reset mic level.
9. Drive state machine to Completed or Failed.
10. Return `StopRecordingResponse`.

`pause()` and `resume()` should set the same `pause_flag` semantics as macOS.

- [ ] **Step 5: Run service tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib consume_frames
cargo test --manifest-path src-tauri/Cargo.toml --lib windows_service
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/app/recording_consumer.rs src-tauri/src/app/mod.rs src-tauri/src/platform/macos_service.rs src-tauri/src/platform/windows_service.rs src-tauri/src/platform/windows/graphics_capture.rs src-tauri/src/platform/windows/wasapi_loopback.rs
git commit -m "feat(windows): wire recording service to shared media pipeline"
```

---

## Task 10: Implement Windows Window Enumeration And WGC Window Capture

**Files:**
- Modify: `src-tauri/src/platform/windows/window_capture.rs`
- Modify: `src-tauri/src/platform/windows/graphics_capture.rs`
- Modify: `src-tauri/src/platform/windows_service.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Implement window enumeration**

Replace `window_capture.rs` stub with EnumWindows-based enumeration:

```rust
use crate::app::error::{AppError, AppResult};
use crate::core::window::{WindowInfo, WindowRecordingState};

pub fn list_windows() -> AppResult<Vec<WindowInfo>> {
    #[cfg(target_os = "windows")]
    {
        windows_list_windows()
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口枚举仅支持 Windows",
        })
    }
}
```

Implement `windows_list_windows()` using:

- `EnumWindows`
- `IsWindowVisible`
- `IsIconic`
- `GetWindowTextLengthW`
- `GetWindowTextW`
- `GetWindowRect`
- `GetWindowThreadProcessId`

Filter out:

- invisible windows
- empty title windows
- zero-size windows
- the LuZhi process window if it is identifiable

- [ ] **Step 2: Add pure tests for filtering**

Add a pure helper:

```rust
#[derive(Clone, Debug)]
struct RawWindowInfo {
    hwnd: isize,
    title: String,
    app_name: String,
    visible: bool,
    minimized: bool,
    width: f64,
    height: f64,
}

fn should_include_window(raw: &RawWindowInfo) -> bool {
    raw.visible
        && !raw.title.trim().is_empty()
        && raw.width >= 1.0
        && raw.height >= 1.0
        && raw.app_name != "LuZhi"
        && raw.app_name != "录智"
}
```

Tests:

```rust
#[test]
fn filters_empty_and_hidden_windows() {
    assert!(!should_include_window(&RawWindowInfo {
        hwnd: 1,
        title: "".to_string(),
        app_name: "App".to_string(),
        visible: true,
        minimized: false,
        width: 100.0,
        height: 100.0,
    }));

    assert!(!should_include_window(&RawWindowInfo {
        hwnd: 1,
        title: "Hidden".to_string(),
        app_name: "App".to_string(),
        visible: false,
        minimized: false,
        width: 100.0,
        height: 100.0,
    }));
}
```

- [ ] **Step 3: Implement window capture start**

Add to `WindowsGraphicsCapture`:

```rust
pub fn start_window(
    &mut self,
    window_id: u32,
    config: CaptureConfig,
    video_sink: VideoFrameSink,
    session_clock: Arc<SessionClock>,
) -> AppResult<()> {
    if self.worker.is_some() {
        return Err(AppError::InvalidState {
            current: "recording",
            action: "start_window",
        });
    }
    // Spawn worker that creates a GraphicsCaptureItem from the selected HWND.
    // Use Windows Graphics Capture, not desktop-frame cropping.
    Ok(())
}
```

Complete it with WGC window item creation before commit.

- [ ] **Step 4: Wire `list_windows` command for Windows**

In `src-tauri/src/lib.rs`, change `list_windows()`:

```rust
#[cfg(target_os = "windows")]
{
    platform::windows::window_capture::list_windows().map_err(|e| e.to_string())
}
```

Change `set_window_id()` validation on Windows:

```rust
#[cfg(target_os = "windows")]
{
    let windows = platform::windows::window_capture::list_windows()
        .map_err(|e| e.to_string())?;
    let window = windows
        .iter()
        .find(|window| window.window_id == window_id)
        .ok_or(format!("窗口未找到：{window_id}"))?;
    if !window.is_on_screen {
        return Err(format!("窗口已最小化，请恢复窗口后重试：{}", window.title));
    }
}
```

- [ ] **Step 5: Wire `WindowsRecordingService::start_window`**

Use `graphics_capture.start_window(...)` instead of `start_display(...)`, then continue through the same system audio, mic, cursor, writer, and consumer setup as fullscreen.

- [ ] **Step 6: Run tests and manual window smoke**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib window_capture
npm test -- src/components/window-selector.test.tsx
```

Manual smoke:

1. Open Notepad or Terminal.
2. Switch LuZhi to window mode.
3. Confirm the window appears in the selector.
4. Select it and start recording.
5. Stop after 5 seconds.

Expected: recording enters preview and contains the selected window content. No desktop-frame crop fallback is used.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/platform/windows/window_capture.rs src-tauri/src/platform/windows/graphics_capture.rs src-tauri/src/platform/windows_service.rs src-tauri/src/lib.rs
git commit -m "feat(windows): implement window enumeration and WGC window capture"
```

---

## Task 11: Update Windows Docs And Manual Checklist

**Files:**
- Create: `tests/2026-06-19-windows-mvp-checklist.md`
- Modify: `docs/platform-diff/windows-development-environment.md`
- Modify: `README.md`

- [ ] **Step 1: Add manual checklist**

Create `tests/2026-06-19-windows-mvp-checklist.md`:

```markdown
# Windows MVP Manual Checklist

## Environment

- [ ] Windows 10/11 test machine.
- [ ] WebView2 Runtime installed.
- [ ] Rust stable-msvc active.
- [ ] Visual Studio Build Tools C++ workload installed.
- [ ] FFmpeg dev libraries configured if testing `--features ffmpeg`.

## Build

- [ ] `npm test -- --run` passes.
- [ ] `npm run build` passes.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib` passes.
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` passes.
- [ ] `npm run tauri:dev` launches the app.

## Fullscreen Recording

- [ ] 1080p/30fps fullscreen recording starts.
- [ ] Stop enters preview.
- [ ] Recording appears in history.
- [ ] Recording with system audio contains audible system audio.
- [ ] Recording with microphone contains audible microphone audio.
- [ ] Recording with system audio + microphone has no obvious A/V drift.

## Window Recording

- [ ] Window selector lists Notepad or Terminal.
- [ ] Minimized windows are disabled or return a Chinese error.
- [ ] Selected window records via Windows Graphics Capture.
- [ ] Closing target window auto-stops or fails visibly.
- [ ] Protected/unavailable windows do not generate fake successful recordings.

## Cursor And Export

- [ ] Cursor position aligns with video in preview/export.
- [ ] Click effect appears in exported video when enabled.
- [ ] Bilibili / YouTube 16:9 export opens and plays.
- [ ] Douyin 9:16 export opens and plays.
- [ ] Xiaohongshu 1:1 export opens and plays.

## Failure Modes

- [ ] WASAPI unavailable path explains that system audio can be disabled.
- [ ] Requested microphone unavailable path fails visibly.
- [ ] Failed recording does not create a fake history item.
```

- [ ] **Step 2: Update Windows development docs**

In `docs/platform-diff/windows-development-environment.md`, replace statements saying Windows Tauri is blocked by `compile_error!` with:

```markdown
Windows 原生应用已进入 MVP 实现路径：`npm run tauri:dev` 应能启动应用壳；录制能力按 Windows Graphics Capture、WASAPI、cpal 麦克风和 FFmpeg feature 的接入状态逐步验证。
```

Add a Windows Graphics Capture note:

```markdown
Windows 窗口录制采用 Windows Graphics Capture，不以 DXGI 桌面帧裁剪作为正式窗口录制方案。若窗口最小化、关闭、受保护或 API 不支持，应用应给出中文错误，不生成假成功录制。
```

- [ ] **Step 3: Update README Windows status**

In `README.md`, update the Windows status table after implementation:

```markdown
| Windows | MVP 开发中 | 应用可启动；录制主链路按 Windows Graphics Capture / WASAPI / cpal / FFmpeg 逐步验收 |
```

- [ ] **Step 4: Commit**

```powershell
git add tests/2026-06-19-windows-mvp-checklist.md docs/platform-diff/windows-development-environment.md README.md
git commit -m "docs(windows): add MVP validation checklist"
```

---

## Task 12: Final Verification And Native Safety Review

**Files:**
- Modify only if verification finds targeted fixes.

- [ ] **Step 1: Run frontend checks**

Run:

```powershell
npm test -- --run
npm run build
```

Expected: PASS.

- [ ] **Step 2: Run Rust checks**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 3: Run FFmpeg checks when environment is ready**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

Expected: PASS, or document missing FFmpeg dev libraries in the checklist.

- [ ] **Step 4: Run manual checklist**

Run every item in:

```powershell
tests/2026-06-19-windows-mvp-checklist.md
```

Expected: all applicable MVP items checked. If an item is blocked by local environment, record the exact blocker in the checklist.

- [ ] **Step 5: Native safety review**

Review these files line by line:

```text
src-tauri/src/platform/windows/graphics_capture.rs
src-tauri/src/platform/windows/wasapi_loopback.rs
src-tauri/src/platform/windows/window_capture.rs
src-tauri/src/platform/windows/cursor_source.rs
```

Checklist:

- [ ] All mapped D3D textures are unmapped on every path.
- [ ] WGC frame surfaces are copied before frame pool ownership returns.
- [ ] COM/WinRT objects are owned by the worker thread that uses them.
- [ ] WASAPI buffers are released exactly once.
- [ ] Audio packet flags for silence/discontinuity are handled visibly.
- [ ] Capture callbacks do not encode, export, or emit heavy UI events.
- [ ] Stop paths terminate worker threads without unbounded joins.

- [ ] **Step 6: Final commit if checklist/docs changed**

```powershell
git add tests/2026-06-19-windows-mvp-checklist.md docs/platform-diff/windows-development-environment.md README.md
git commit -m "test(windows): record MVP verification results"
```

---

## Self-Review

### Spec Coverage

- Windows app build/start: Tasks 1-4.
- Platform service boundary: Tasks 1 and 3.
- Windows Graphics Capture fullscreen: Tasks 5-6.
- WASAPI loopback: Task 7.
- cpal microphone reuse: Task 9, with explicit move if current module remains macOS-owned.
- Existing mixer/writer/export reuse: Task 9.
- Windows window recording via WGC, not crop: Task 10.
- Cursor metadata: Task 8 and Task 9.
- Manual validation/docs: Tasks 11-12.

### Placeholder Scan

Search terms checked: `TBD`, `TODO`, `尚未完成`, and `placeholder`. The plan keeps native API details behind explicit implementation and smoke-test gates instead of allowing half-complete worker bodies to pass as done.

### Type Consistency

- Service trait uses `StopRecordingResponse`, matching current `MacRecordingService::stop()`.
- Cursor dispatcher is moved to `app::recording_service_boundary` so `lib.rs` is no longer macOS-only.
- Video frames remain `VideoFrameRef` with `PixelFormat::Bgra8`.
- Audio remains `AudioChunk` and existing mixer/writer contracts stay unchanged.
