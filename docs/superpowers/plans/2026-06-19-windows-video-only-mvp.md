# Windows Video-Only MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the narrowed Windows Video-Only MVP: fullscreen WGC video recording only, no Windows audio, raw system cursor visible, clear fail-fast behavior for unsupported Windows paths, and no macOS regression.

**Architecture:** Keep the existing macOS pipeline unchanged and constrain Windows at both backend and frontend capability boundaries. Windows recording creates the writer before native capture, starts only fullscreen WGC after a startup handshake, feeds the shared consumer with no requested audio sources, and rejects window/area/audio/cursor-overlay requests before native devices are touched.

**Tech Stack:** Tauri 2, Rust 2021, React/Vite, TypeScript, Windows Graphics Capture via `windows` crate 0.62, existing FFmpeg/Counting writer boundary, Vitest, Rust unit tests, manual Windows/macOS gates.

---

## Scope

This plan implements [2026-06-19-windows-video-only-mvp-design.md](../specs/2026-06-19-windows-video-only-mvp-design.md). It supersedes the broader [2026-06-19-windows-mvp.md](2026-06-19-windows-mvp.md) for this round only.

The implementation must close review findings `#1`, `#2`, `#5`, and the Windows-facing parts of `#12` by scope reduction:

- `#1`: no WGC false success; fullscreen WGC must handshake before `start_recording` succeeds.
- `#2`: Windows WASAPI is disabled/fail-fast; it must not advertise support or return fake success.
- `#5`: writer must be created before WGC; writer failure must not leak native capture or leave `Recording`.
- `#12`: Windows cursor overlay is disabled; raw cursor remains visible, so capture geometry is not required in this round.

Deferred by explicit product decision:

- Windows window recording and HWND type migration (`#7`).
- Windows microphone and CPAL `Send` safety (`#9`).
- Windows WASAPI audio implementation.
- Windows cursor overlay/capture geometry.

---

## File Structure

### Modified Rust Files

| File | Responsibility |
|------|----------------|
| `src-tauri/src/lib.rs` | Add platform capabilities command, Windows defaults, Windows start-time policy clamp, Windows beautify fail-fast, and command registration. |
| `src-tauri/src/platform/windows_service.rs` | Enforce video-only policy, introduce testable writer/capture seams, create writer before WGC, disable cursor metadata runtime for Windows MVP, and pass no requested audio sources to the consumer. |
| `src-tauri/src/platform/windows/graphics_capture.rs` | Add WGC startup handshake, replace sleep-only worker with real fullscreen WGC capture, and add non-native handshake tests. |
| `src-tauri/src/platform/windows/wasapi_loopback.rs` | Stop advertising system-audio support and return clear unavailable errors from `start()`. |
| `src-tauri/Cargo.toml` | Add only the Windows API features required for WGC interop if the compiler proves the existing feature list is insufficient; do not change crate versions. |

### Modified Frontend Files

| File | Responsibility |
|------|----------------|
| `src/lib/tauri.ts` | Add `PlatformCapabilities` type and `fetchPlatformCapabilities()` wrapper. |
| `src/App.tsx` | Fetch capabilities, clamp Windows UI state to fullscreen/no-audio/no-cursor-beautify, send safe start payloads, and pass capabilities to child views. |
| `src/components/recording-panel.tsx` | Disable Windows unsupported mode/audio controls and prevent their handlers from mutating state. |
| `src/components/preview-view.tsx` | Disable cursor beautify controls when unsupported so Windows recordings do not attempt overlay timeline builds. |
| `src/App.test.tsx` | Add capability mock defaults, Windows capability tests, and adjust existing expectations without weakening macOS/default behavior. |

### Modified Docs

| File | Responsibility |
|------|----------------|
| `tests/2026-06-19-windows-mvp-checklist.md` | Update verification rows to video-only scope and mark unsupported audio/window/cursor paths as disabled/fail-fast, not PASS. |

---

## Guardrails

- Do not edit `src-tauri/src/platform/macos_service.rs` unless a compile error proves a shared signature changed. macOS behavior is protected.
- Do not remove or weaken existing macOS/default frontend tests; add capability-aware branches instead.
- Do not enable Windows system audio, microphone, window recording, area recording, or cursor overlay in this plan.
- Do not change dependency versions. If WGC interop needs additional `windows` crate features, add features only.
- Keep all native-touching tests ignored or manual. Unit tests must use injected fakes or pure helpers.

---

## Task 1: Backend Platform Capabilities And Windows Defaults

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Test: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add Rust capability payload and helper**

In `src-tauri/src/lib.rs`, near the existing payload structs, add:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformCapabilitiesPayload {
    platform: &'static str,
    supports_fullscreen: bool,
    supports_window: bool,
    supports_area: bool,
    supports_system_audio: bool,
    supports_microphone: bool,
    supports_cursor_beautify: bool,
}

fn current_platform_capabilities() -> PlatformCapabilitiesPayload {
    #[cfg(target_os = "windows")]
    {
        PlatformCapabilitiesPayload {
            platform: "windows",
            supports_fullscreen: true,
            supports_window: false,
            supports_area: false,
            supports_system_audio: false,
            supports_microphone: false,
            supports_cursor_beautify: false,
        }
    }

    #[cfg(target_os = "macos")]
    {
        PlatformCapabilitiesPayload {
            platform: "macos",
            supports_fullscreen: true,
            supports_window: true,
            supports_area: false,
            supports_system_audio: true,
            supports_microphone: true,
            supports_cursor_beautify: true,
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        PlatformCapabilitiesPayload {
            platform: "unknown",
            supports_fullscreen: true,
            supports_window: false,
            supports_area: false,
            supports_system_audio: false,
            supports_microphone: false,
            supports_cursor_beautify: false,
        }
    }
}

#[tauri::command]
fn platform_capabilities() -> PlatformCapabilitiesPayload {
    current_platform_capabilities()
}
```

- [ ] **Step 2: Register the Tauri command**

In `src-tauri/src/lib.rs`, add `platform_capabilities` to `tauri::generate_handler![...]` immediately after `recording_permissions`:

```rust
recording_status,
recording_permissions,
platform_capabilities,
start_recording,
```

- [ ] **Step 3: Make `AppState` defaults platform-aware**

In `impl Default for AppState`, replace the inline `AudioConfig` and `BeautifyConfigPayload::default()` with helper functions:

```rust
fn default_audio_config_for_platform() -> AudioConfig {
    #[cfg(target_os = "windows")]
    {
        AudioConfig {
            capture_system_audio: false,
            capture_microphone: false,
            microphone_device: None,
            sample_rate: 48000,
            channels: 2,
            denoise_mode: DenoiseMode::default(),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        AudioConfig {
            capture_system_audio: true,
            capture_microphone: true,
            microphone_device: None,
            sample_rate: 48000,
            channels: 2,
            denoise_mode: DenoiseMode::default(),
        }
    }
}

fn default_beautify_config_for_platform() -> BeautifyConfigPayload {
    #[cfg(target_os = "windows")]
    {
        BeautifyConfigPayload {
            cursor_magnification: false,
            magnification_factor: 2.0,
            cursor_smoothing: false,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        BeautifyConfigPayload::default()
    }
}
```

Then use:

```rust
audio_config: Arc::new(Mutex::new(default_audio_config_for_platform())),
beautify_config: Arc::new(Mutex::new(default_beautify_config_for_platform())),
```

- [ ] **Step 4: Add backend capability tests**

In `src-tauri/src/lib.rs` tests, add:

```rust
#[test]
fn platform_capabilities_match_current_target() {
    let caps = current_platform_capabilities();

    #[cfg(target_os = "windows")]
    {
        assert_eq!(caps.platform, "windows");
        assert!(caps.supports_fullscreen);
        assert!(!caps.supports_window);
        assert!(!caps.supports_area);
        assert!(!caps.supports_system_audio);
        assert!(!caps.supports_microphone);
        assert!(!caps.supports_cursor_beautify);
    }

    #[cfg(target_os = "macos")]
    {
        assert_eq!(caps.platform, "macos");
        assert!(caps.supports_fullscreen);
        assert!(caps.supports_window);
        assert!(!caps.supports_area);
        assert!(caps.supports_system_audio);
        assert!(caps.supports_microphone);
        assert!(caps.supports_cursor_beautify);
    }
}

#[test]
fn default_windows_audio_and_beautify_are_video_only() {
    #[cfg(target_os = "windows")]
    {
        let audio = default_audio_config_for_platform();
        let beautify = default_beautify_config_for_platform();

        assert!(!audio.capture_system_audio);
        assert!(!audio.capture_microphone);
        assert!(!beautify.cursor_magnification);
        assert!(!beautify.cursor_smoothing);
    }
}
```

- [ ] **Step 5: Run the focused Rust test**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib platform_capabilities_match_current_target
```

Expected: test passes on the current platform.

- [ ] **Step 6: Add frontend Tauri wrapper**

In `src/lib/tauri.ts`, add:

```ts
export type PlatformCapabilities = {
  platform: 'windows' | 'macos' | 'unknown'
  supportsFullscreen: boolean
  supportsWindow: boolean
  supportsArea: boolean
  supportsSystemAudio: boolean
  supportsMicrophone: boolean
  supportsCursorBeautify: boolean
}

export async function fetchPlatformCapabilities(): Promise<PlatformCapabilities> {
  return invoke<PlatformCapabilities>('platform_capabilities')
}
```

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/lib.rs src/lib/tauri.ts
git commit -m "feat(windows): expose video-only platform capabilities"
```

---

## Task 2: Windows Backend Video-Only Fail-Fast Policy

**Files:**
- Modify: `src-tauri/src/platform/windows_service.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/platform/windows_service.rs`

- [ ] **Step 1: Add policy helper**

In `src-tauri/src/platform/windows_service.rs`, add near the constants:

```rust
const WINDOWS_MVP_FULLSCREEN_ONLY: &str = "Windows MVP 暂仅支持全屏录制";
const WINDOWS_MVP_NO_SYSTEM_AUDIO: &str =
    "Windows MVP 暂不支持系统音频录制，请关闭系统音频后重试";
const WINDOWS_MVP_NO_MICROPHONE: &str =
    "Windows MVP 暂不支持麦克风录制，请关闭麦克风后重试";
const WINDOWS_MVP_NO_CURSOR_BEAUTIFY: &str =
    "Windows MVP 暂不支持光标美化，请保留系统光标";

fn validate_windows_video_only_request(
    config: &CaptureConfig,
    audio_config: &AudioConfig,
    beautify_snapshot: &BeautifyConfigSnapshot,
) -> AppResult<()> {
    if config.mode != crate::core::config::CaptureMode::FullScreen {
        return Err(AppError::NativeCaptureUnavailable {
            reason: WINDOWS_MVP_FULLSCREEN_ONLY,
        });
    }
    if audio_config.capture_system_audio {
        return Err(AppError::NativeCaptureUnavailable {
            reason: WINDOWS_MVP_NO_SYSTEM_AUDIO,
        });
    }
    if audio_config.capture_microphone {
        return Err(AppError::NativeCaptureUnavailable {
            reason: WINDOWS_MVP_NO_MICROPHONE,
        });
    }
    if !config.show_system_cursor || !beautify_snapshot.raw_system_cursor_visible {
        return Err(AppError::NativeCaptureUnavailable {
            reason: WINDOWS_MVP_NO_CURSOR_BEAUTIFY,
        });
    }
    if beautify_snapshot.cursor_magnification || beautify_snapshot.cursor_smoothing {
        return Err(AppError::NativeCaptureUnavailable {
            reason: WINDOWS_MVP_NO_CURSOR_BEAUTIFY,
        });
    }
    Ok(())
}
```

- [ ] **Step 2: Call the policy before channels/native startup**

In `WindowsRecordingService::start()`, immediately after `self.state_machine.start()?;`, add:

```rust
if let Err(error) = validate_windows_video_only_request(&config, &audio_config, &beautify_snapshot) {
    self.state_machine.fail();
    return Err(error);
}
```

Keep `clear_session_paths()`, `session_id`, channel creation, WGC startup, and writer startup after this guard.

- [ ] **Step 3: Make `start_window()` fail-fast**

Replace the body of `WindowsRecordingService::start_window()` with:

```rust
let _ = (
    window_id,
    config,
    _show_system_cursor,
    audio_config,
    beautify_snapshot,
    cursor_main_thread_dispatcher,
);
self.state_machine.start()?;
self.state_machine.fail();
Err(AppError::NativeCaptureUnavailable {
    reason: WINDOWS_MVP_FULLSCREEN_ONLY,
})
```

Do not start WGC, WASAPI, microphone, cursor runtime, writer, or consumer in `start_window()`.

- [ ] **Step 4: Make Windows `set_beautify_config` reject cursor overlay**

In `src-tauri/src/lib.rs`, add before writing the beautify config:

```rust
#[cfg(target_os = "windows")]
if config.cursor_magnification || config.cursor_smoothing {
    return Err("Windows MVP 暂不支持光标美化，请保留系统光标".to_string());
}
```

This keeps old frontend state or direct invoke calls from enabling hidden-cursor recordings on Windows.

- [ ] **Step 5: Force raw cursor facts in Windows `start_recording`**

In `start_recording`, replace the current `expected_show_system_cursor` assignment with platform-aware logic:

```rust
#[cfg(target_os = "windows")]
let expected_show_system_cursor = true;

#[cfg(not(target_os = "windows"))]
let expected_show_system_cursor =
    !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);
```

When constructing `BeautifyConfigSnapshot`, use platform-aware cursor effect booleans:

```rust
#[cfg(target_os = "windows")]
let cursor_magnification = false;
#[cfg(not(target_os = "windows"))]
let cursor_magnification = beautify_config.cursor_magnification;

#[cfg(target_os = "windows")]
let cursor_smoothing = false;
#[cfg(not(target_os = "windows"))]
let cursor_smoothing = beautify_config.cursor_smoothing;
```

Then set:

```rust
cursor_magnification,
cursor_smoothing,
raw_system_cursor_visible: config.show_system_cursor,
```

- [ ] **Step 6: Add fail-fast tests**

In `src-tauri/src/platform/windows_service.rs` tests, add helper constructors:

```rust
fn video_only_audio_config() -> AudioConfig {
    AudioConfig {
        capture_system_audio: false,
        capture_microphone: false,
        microphone_device: None,
        sample_rate: 48000,
        channels: 2,
        denoise_mode: DenoiseMode::default(),
    }
}

fn video_only_beautify_snapshot() -> BeautifyConfigSnapshot {
    BeautifyConfigSnapshot {
        cursor_magnification: false,
        magnification_factor: 2.0,
        cursor_smoothing: false,
        auto_trim_silences: false,
        trim_sensitivity: "medium".to_string(),
        raw_system_cursor_visible: true,
    }
}
```

Add tests:

```rust
#[test]
fn rejects_system_audio_before_native_start() {
    let mut config = CaptureConfig::full_screen_1080p_30fps();
    config.show_system_cursor = true;
    let mut audio = video_only_audio_config();
    audio.capture_system_audio = true;

    let result = validate_windows_video_only_request(
        &config,
        &audio,
        &video_only_beautify_snapshot(),
    );

    assert!(result.unwrap_err().to_string().contains("系统音频"));
}

#[test]
fn rejects_microphone_before_native_start() {
    let config = CaptureConfig::full_screen_1080p_30fps();
    let mut audio = video_only_audio_config();
    audio.capture_microphone = true;

    let result = validate_windows_video_only_request(
        &config,
        &audio,
        &video_only_beautify_snapshot(),
    );

    assert!(result.unwrap_err().to_string().contains("麦克风"));
}

#[test]
fn rejects_cursor_beautify_before_native_start() {
    let mut config = CaptureConfig::full_screen_1080p_30fps();
    config.show_system_cursor = false;

    let result = validate_windows_video_only_request(
        &config,
        &video_only_audio_config(),
        &BeautifyConfigSnapshot {
            raw_system_cursor_visible: false,
            cursor_magnification: true,
            ..video_only_beautify_snapshot()
        },
    );

    assert!(result.unwrap_err().to_string().contains("光标美化"));
}

#[test]
fn rejects_window_recording_for_video_only_mvp() {
    let mut service = WindowsRecordingService::new();

    let result = service.start_window(
        42,
        CaptureConfig {
            mode: crate::core::config::CaptureMode::Window,
            window_id: Some(42),
            ..CaptureConfig::full_screen_1080p_30fps()
        },
        true,
        video_only_audio_config(),
        video_only_beautify_snapshot(),
        Box::new(InlineDispatcher),
    );

    assert!(result.unwrap_err().to_string().contains("全屏录制"));
    assert_eq!(service.state(), RecordingState::Failed);
}
```

- [ ] **Step 7: Run focused tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib validate_windows_video_only_request
cargo test --manifest-path src-tauri/Cargo.toml --lib rejects_window_recording_for_video_only_mvp
```

Expected: tests pass without touching real WGC, WASAPI, or CPAL devices.

- [ ] **Step 8: Commit**

```powershell
git add src-tauri/src/platform/windows_service.rs src-tauri/src/lib.rs
git commit -m "fix(windows): fail fast for unsupported video-only MVP paths"
```

---

## Task 3: Testable Windows Writer And Capture Seams

**Files:**
- Modify: `src-tauri/src/platform/windows_service.rs`
- Test: `src-tauri/src/platform/windows_service.rs`

- [ ] **Step 1: Add private capture trait and writer build type**

In `src-tauri/src/platform/windows_service.rs`, add:

```rust
use crate::core::capture::VideoFrameSink;
use crate::media::recording_writer::RecordingWriter;

trait WindowsVideoCapture: Send {
    fn start_display(
        &mut self,
        config: CaptureConfig,
        video_sink: VideoFrameSink,
        session_clock: Arc<SessionClock>,
    ) -> AppResult<()>;

    fn stop(&mut self) -> AppResult<()>;
}

impl WindowsVideoCapture for WindowsGraphicsCapture {
    fn start_display(
        &mut self,
        config: CaptureConfig,
        video_sink: VideoFrameSink,
        session_clock: Arc<SessionClock>,
    ) -> AppResult<()> {
        WindowsGraphicsCapture::start_display(self, config, video_sink, session_clock)
    }

    fn stop(&mut self) -> AppResult<()> {
        WindowsGraphicsCapture::stop(self)
    }
}

struct WindowsWriterBuild {
    writer: Box<dyn RecordingWriter>,
    output_path: Option<String>,
}

type WindowsWriterFactory = Arc<dyn Fn(CaptureConfig) -> AppResult<WindowsWriterBuild> + Send + Sync>;
```

- [ ] **Step 2: Update service fields and constructors**

Change the `graphics_capture` field to:

```rust
graphics_capture: Box<dyn WindowsVideoCapture>,
writer_factory: WindowsWriterFactory,
```

In `WindowsRecordingService::new()`, initialize:

```rust
graphics_capture: Box::new(WindowsGraphicsCapture::new()),
writer_factory: Arc::new(default_windows_writer_factory),
```

Add a test-only constructor:

```rust
#[cfg(test)]
fn with_dependencies(
    graphics_capture: Box<dyn WindowsVideoCapture>,
    writer_factory: WindowsWriterFactory,
) -> Self {
    Self {
        graphics_capture,
        writer_factory,
        ..Self::new()
    }
}
```

If Rust rejects struct update syntax because `Self::new()` already initializes fields being overridden, use:

```rust
let mut service = Self::new();
service.graphics_capture = graphics_capture;
service.writer_factory = writer_factory;
service
```

- [ ] **Step 3: Move writer creation into a default factory**

Add:

```rust
fn default_windows_writer_factory(config: CaptureConfig) -> AppResult<WindowsWriterBuild> {
    #[cfg(feature = "ffmpeg")]
    {
        let output_path = crate::media::export_paths::recording_output_path();
        let writer = crate::media::ffmpeg_writer::FfmpegRecordingWriter::with_fps(
            output_path.clone(),
            config.fps,
        )?;
        Ok(WindowsWriterBuild {
            writer: Box::new(writer),
            output_path: Some(output_path.to_string_lossy().to_string()),
        })
    }

    #[cfg(not(feature = "ffmpeg"))]
    {
        let _ = config;
        Ok(WindowsWriterBuild {
            writer: Box::new(crate::media::recording_writer::CountingRecordingWriter::new(None)),
            output_path: None,
        })
    }
}
```

- [ ] **Step 4: Add mock capture for tests**

In the test module:

```rust
struct MockVideoCapture {
    started: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    start_result: AppResult<()>,
}

impl WindowsVideoCapture for MockVideoCapture {
    fn start_display(
        &mut self,
        _config: CaptureConfig,
        _video_sink: VideoFrameSink,
        _session_clock: Arc<SessionClock>,
    ) -> AppResult<()> {
        self.started.store(true, Ordering::SeqCst);
        self.start_result.clone()
    }

    fn stop(&mut self) -> AppResult<()> {
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}
```

- [ ] **Step 5: Add writer-failure test before changing order**

Add:

```rust
#[test]
fn writer_failure_does_not_start_wgc() {
    let started = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let capture = MockVideoCapture {
        started: started.clone(),
        stopped,
        start_result: Ok(()),
    };
    let writer_factory: WindowsWriterFactory = Arc::new(|_config| {
        Err(AppError::RecordingWriteFailed {
            reason: "writer boom".to_string(),
        })
    });
    let mut service = WindowsRecordingService::with_dependencies(Box::new(capture), writer_factory);

    let result = service.start(
        CaptureConfig::full_screen_1080p_30fps(),
        video_only_audio_config(),
        video_only_beautify_snapshot(),
        Box::new(InlineDispatcher),
    );

    assert!(result.unwrap_err().to_string().contains("writer boom"));
    assert!(!started.load(Ordering::SeqCst));
    assert_eq!(service.state(), RecordingState::Failed);
}
```

Expected before Step 6: this test fails if the current code starts WGC before writer creation.

- [ ] **Step 6: Run the failing test**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib writer_failure_does_not_start_wgc
```

Expected: FAIL before reordering, proving the test covers finding `#5`.

- [ ] **Step 7: Create writer before WGC**

In `WindowsRecordingService::start()`, after policy validation and session bookkeeping, create the writer before channels and WGC:

```rust
let writer_build = match (self.writer_factory)(config) {
    Ok(build) => build,
    Err(error) => {
        self.state_machine.fail();
        return Err(error);
    }
};
self.last_recording_output_path = writer_build.output_path.clone();
let writer = writer_build.writer;
```

Remove the later inline `#[cfg(feature = "ffmpeg")]` / `#[cfg(not(feature = "ffmpeg"))]` writer creation block from `start()`.

- [ ] **Step 8: Keep WGC cleanup on WGC startup failure**

When `self.graphics_capture.start_display(...)` returns `Err(error)`, ensure the code does:

```rust
let _ = self.graphics_capture.stop();
self.state_machine.fail();
return Err(error);
```

No consumer thread should be spawned before WGC startup succeeds.

- [ ] **Step 9: Run writer and WGC-failure tests**

Add:

```rust
#[test]
fn wgc_start_failure_does_not_start_consumer() {
    let started = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let capture = MockVideoCapture {
        started: started.clone(),
        stopped: stopped.clone(),
        start_result: Err(AppError::CaptureFailed {
            reason: "wgc boom".to_string(),
        }),
    };
    let writer_factory: WindowsWriterFactory = Arc::new(|_config| {
        Ok(WindowsWriterBuild {
            writer: Box::new(crate::media::recording_writer::CountingRecordingWriter::new(None)),
            output_path: None,
        })
    });
    let mut service = WindowsRecordingService::with_dependencies(Box::new(capture), writer_factory);

    let result = service.start(
        CaptureConfig::full_screen_1080p_30fps(),
        video_only_audio_config(),
        video_only_beautify_snapshot(),
        Box::new(InlineDispatcher),
    );

    assert!(result.unwrap_err().to_string().contains("wgc boom"));
    assert!(started.load(Ordering::SeqCst));
    assert!(stopped.load(Ordering::SeqCst));
    assert!(service.consumer_handle.is_none());
    assert_eq!(service.state(), RecordingState::Failed);
}
```

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib writer_failure_does_not_start_wgc
cargo test --manifest-path src-tauri/Cargo.toml --lib wgc_start_failure_does_not_start_consumer
```

Expected: both pass.

- [ ] **Step 10: Commit**

```powershell
git add src-tauri/src/platform/windows_service.rs
git commit -m "fix(windows): create writer before WGC startup"
```

---

## Task 4: Windows Service Video-Only Runtime Path

**Files:**
- Modify: `src-tauri/src/platform/windows_service.rs`
- Test: `src-tauri/src/platform/windows_service.rs`

- [ ] **Step 1: Remove Windows audio startup from `start()`**

In `WindowsRecordingService::start()`, remove these runtime branches from the Windows MVP path:

```rust
if audio_config.capture_system_audio { ... }
if audio_config.capture_microphone { ... }
```

After policy validation, `audio_config.capture_system_audio` and `audio_config.capture_microphone` are guaranteed false. Keep an empty `system_receiver` for `consume_frames`, and set:

```rust
let mic_rx = None;
```

- [ ] **Step 2: Do not start cursor metadata runtime**

Remove the Windows `CursorMetadataRuntime::spawn(...)` block from `start()` for this round and set:

```rust
self.cursor_runtime = None;
```

The raw system cursor remains visible in the captured video, so Windows does not need cursor metadata for this MVP.

- [ ] **Step 3: Pass false requested audio flags to consumer**

Set the consumer flags explicitly:

```rust
let requested_system_audio = false;
let requested_microphone = false;
let microphone_device = None;
let denoise_mode = DenoiseMode::default();
```

This prevents source-aware audio validation from expecting unavailable Windows audio.

- [ ] **Step 4: Add successful mock-start test**

Add:

```rust
#[test]
fn video_only_success_path_starts_consumer_without_audio_or_cursor_runtime() {
    let started = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let capture = MockVideoCapture {
        started: started.clone(),
        stopped,
        start_result: Ok(()),
    };
    let writer_factory: WindowsWriterFactory = Arc::new(|_config| {
        Ok(WindowsWriterBuild {
            writer: Box::new(crate::media::recording_writer::CountingRecordingWriter::new(None)),
            output_path: None,
        })
    });
    let mut service = WindowsRecordingService::with_dependencies(Box::new(capture), writer_factory);

    service
        .start(
            CaptureConfig::full_screen_1080p_30fps(),
            video_only_audio_config(),
            video_only_beautify_snapshot(),
            Box::new(InlineDispatcher),
        )
        .unwrap();

    assert!(started.load(Ordering::SeqCst));
    assert!(service.consumer_handle.is_some());
    assert!(service.cursor_runtime.is_none());
    assert!(!service.last_requested_system_audio());
    assert!(!service.last_requested_microphone());

    let _ = service.stop();
}
```

- [ ] **Step 5: Run focused tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib video_only_success_path_starts_consumer_without_audio_or_cursor_runtime
cargo test --manifest-path src-tauri/Cargo.toml --lib windows_service
```

Expected: all non-ignored Windows service tests pass without native devices.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/platform/windows_service.rs
git commit -m "fix(windows): run service as fullscreen video-only"
```

---

## Task 5: WASAPI Loopback Disabled Without False Support

**Files:**
- Modify: `src-tauri/src/platform/windows/wasapi_loopback.rs`
- Test: `src-tauri/src/platform/windows/wasapi_loopback.rs`

- [ ] **Step 1: Make `start()` fail-fast**

Replace `WasapiLoopback::start()` body with:

```rust
fn start(&mut self, _config: AudioConfig, _sink: AudioChunkSink) -> AppResult<()> {
    if self.worker.is_some() {
        return Err(AppError::InvalidState {
            current: "recording",
            action: "start_wasapi_loopback",
        });
    }

    Err(AppError::NativeCaptureUnavailable {
        reason: "Windows MVP 暂不支持系统音频录制，请关闭系统音频后重试",
    })
}
```

Leave `stop()` tolerant: if no worker exists, it returns `Ok(())`.

- [ ] **Step 2: Stop advertising support**

Change `capabilities()` to:

```rust
fn capabilities(&self) -> AudioCapabilities {
    AudioCapabilities {
        supports_system_audio: false,
        supports_microphone: false,
    }
}
```

- [ ] **Step 3: Make `device_list()` non-misleading**

For this round, return no system-audio devices:

```rust
fn device_list(&self) -> AppResult<Vec<AudioDevice>> {
    Ok(Vec::new())
}
```

- [ ] **Step 4: Add tests**

Add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::media_channel::bounded_media_channel;

    #[test]
    fn wasapi_reports_no_system_audio_support_for_video_only_mvp() {
        let loopback = WasapiLoopback::new();

        assert!(!loopback.capabilities().supports_system_audio);
        assert!(!loopback.capabilities().supports_microphone);
    }

    #[test]
    fn wasapi_start_fails_fast_for_video_only_mvp() {
        let mut loopback = WasapiLoopback::new();
        let (sink, _rx) = bounded_media_channel(1, "wasapi-test");

        let result = loopback.start(
            AudioConfig {
                capture_system_audio: true,
                capture_microphone: false,
                microphone_device: None,
                sample_rate: 48000,
                channels: 2,
                denoise_mode: crate::core::capture::DenoiseMode::default(),
            },
            sink,
        );

        assert!(result.unwrap_err().to_string().contains("系统音频"));
        assert!(loopback.stop().is_ok());
    }
}
```

- [ ] **Step 5: Run tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib wasapi_reports_no_system_audio_support_for_video_only_mvp
cargo test --manifest-path src-tauri/Cargo.toml --lib wasapi_start_fails_fast_for_video_only_mvp
```

Expected: both pass.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/platform/windows/wasapi_loopback.rs
git commit -m "fix(windows): disable WASAPI for video-only MVP"
```

---

## Task 6: WGC Startup Handshake

**Files:**
- Modify: `src-tauri/src/platform/windows/graphics_capture.rs`
- Test: `src-tauri/src/platform/windows/graphics_capture.rs`

- [ ] **Step 1: Add startup result enum and timeout**

In `src-tauri/src/platform/windows/graphics_capture.rs`, add:

```rust
const WGC_STARTUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug)]
enum GraphicsCaptureStartup {
    Started,
    Failed(AppError),
}
```

- [ ] **Step 2: Thread startup sender through workers**

Change worker function signatures to include `startup_tx`:

```rust
fn run_display_capture_worker(
    config: CaptureConfig,
    video_sink: VideoFrameSink,
    session_clock: Arc<SessionClock>,
    stop: Arc<AtomicBool>,
    startup_tx: std::sync::mpsc::Sender<GraphicsCaptureStartup>,
) -> AppResult<()>
```

For non-Windows and unimplemented paths, send `Failed(error.clone())` before returning the same error.

- [ ] **Step 3: Add a reusable start helper**

Add a private helper:

```rust
fn start_worker_with_handshake<F>(
    worker_slot: &mut Option<thread::JoinHandle<AppResult<()>>>,
    stop_slot: &mut Option<Arc<AtomicBool>>,
    action: &'static str,
    worker: F,
) -> AppResult<()>
where
    F: FnOnce(Arc<AtomicBool>, std::sync::mpsc::Sender<GraphicsCaptureStartup>) -> AppResult<()>
        + Send
        + 'static,
{
    if worker_slot.is_some() {
        return Err(AppError::InvalidState {
            current: "recording",
            action,
        });
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let (startup_tx, startup_rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || worker(thread_stop, startup_tx));

    match startup_rx.recv_timeout(WGC_STARTUP_TIMEOUT) {
        Ok(GraphicsCaptureStartup::Started) => {
            *stop_slot = Some(stop);
            *worker_slot = Some(handle);
            Ok(())
        }
        Ok(GraphicsCaptureStartup::Failed(error)) => {
            stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
            Err(error)
        }
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            drop(handle);
            Err(AppError::CaptureFailed {
                reason: "Windows Graphics Capture 启动超时".to_string(),
            })
        }
    }
}
```

- [ ] **Step 4: Use helper in `start_display()`**

Replace the direct `thread::spawn` in `start_display()` with:

```rust
start_worker_with_handshake(
    &mut self.worker,
    &mut self.stop_flag,
    "start_display",
    move |thread_stop, startup_tx| {
        run_display_capture_worker(config, video_sink, session_clock, thread_stop, startup_tx)
    },
)
```

- [ ] **Step 5: Make the current placeholder fail visibly until real WGC lands**

Inside `run_display_capture_worker_inner`, before the existing sleep loop, send a failure instead of `Started` until Task 7 replaces the placeholder:

```rust
let error = AppError::NativeCaptureUnavailable {
    reason: "Windows 屏幕录制启动失败：WGC 全屏捕获尚未完成接入",
};
let _ = startup_tx.send(GraphicsCaptureStartup::Failed(error.clone()));
return Err(error);
```

This temporary state is allowed only inside Task 6. Task 7 must replace it with real WGC and `Started`.

- [ ] **Step 6: Add non-native handshake tests**

Add tests using `start_worker_with_handshake()` directly:

```rust
#[test]
fn handshake_failure_returns_error_and_clears_worker() {
    let mut worker = None;
    let mut stop = None;

    let result = start_worker_with_handshake(&mut worker, &mut stop, "test", |_stop, startup| {
        let error = AppError::CaptureFailed {
            reason: "startup failed".to_string(),
        };
        let _ = startup.send(GraphicsCaptureStartup::Failed(error.clone()));
        Err(error)
    });

    assert!(result.unwrap_err().to_string().contains("startup failed"));
    assert!(worker.is_none());
    assert!(stop.is_none());
}

#[test]
fn handshake_success_stores_worker_and_stop_flag() {
    let mut worker = None;
    let mut stop = None;

    start_worker_with_handshake(&mut worker, &mut stop, "test", |stop, startup| {
        let _ = startup.send(GraphicsCaptureStartup::Started);
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    })
    .unwrap();

    assert!(worker.is_some());
    assert!(stop.is_some());
    stop.unwrap().store(true, Ordering::Relaxed);
    let _ = worker.unwrap().join();
}
```

- [ ] **Step 7: Run handshake tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib handshake_failure_returns_error_and_clears_worker
cargo test --manifest-path src-tauri/Cargo.toml --lib handshake_success_stores_worker_and_stop_flag
```

Expected: both pass. Manual Windows recording still cannot pass until Task 7.

- [ ] **Step 8: Commit**

```powershell
git add src-tauri/src/platform/windows/graphics_capture.rs
git commit -m "fix(windows): require WGC startup handshake"
```

---

## Task 7: Real Fullscreen WGC Worker

**Files:**
- Modify: `src-tauri/Cargo.toml` only if compiler requires more Windows features.
- Modify: `src-tauri/src/platform/windows/graphics_capture.rs`
- Test: `src-tauri/src/platform/windows/graphics_capture.rs`

- [ ] **Step 1: Confirm Windows API bindings with a compile-only native spike**

In `src-tauri/src/platform/windows/graphics_capture.rs`, under `#[cfg(target_os = "windows")]`, add imports inside `run_display_capture_worker_inner` rather than file-level imports. Start with:

```rust
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::{
    IDirect3DDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{BOOL, HWND};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
```

Run:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

If imports fail because the current `windows` feature list lacks WinRT interop namespaces, add these features to `src-tauri/Cargo.toml` under the existing `windows = { version = "0.62", features = [...] }` list:

```toml
"Win32_System_WinRT",
"Win32_System_WinRT_Direct3D11",
"Win32_System_WinRT_Graphics_Capture",
```

Run `cargo check` again. Do not change `windows = "0.62"`.

- [ ] **Step 2: Add COM guard**

Add:

```rust
#[cfg(target_os = "windows")]
struct ComGuard;

#[cfg(target_os = "windows")]
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
```

Inside the worker:

```rust
unsafe {
    CoInitializeEx(None, COINIT_MULTITHREADED).map_err(|e| AppError::CaptureFailed {
        reason: format!("Windows 屏幕录制启动失败：COM 初始化失败：{e}"),
    })?;
}
let _com_guard = ComGuard;
```

- [ ] **Step 3: Create D3D11 and Direct3D devices**

Add helper:

```rust
#[cfg(target_os = "windows")]
unsafe fn create_direct3d_device() -> AppResult<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice)> {
    let mut d3d_device = None;
    let mut d3d_context = None;
    let feature_levels = [D3D_FEATURE_LEVEL_11_0];
    let mut selected_level = D3D_FEATURE_LEVEL::default();

    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        None,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        Some(&feature_levels),
        D3D11_SDK_VERSION,
        Some(&mut d3d_device),
        Some(&mut selected_level),
        Some(&mut d3d_context),
    )
    .map_err(|e| AppError::CaptureFailed {
        reason: format!("Windows 屏幕录制启动失败：D3D11 设备创建失败：{e}"),
    })?;

    let d3d_device = d3d_device.ok_or_else(|| AppError::CaptureFailed {
        reason: "Windows 屏幕录制启动失败：D3D11 设备为空".to_string(),
    })?;
    let d3d_context = d3d_context.ok_or_else(|| AppError::CaptureFailed {
        reason: "Windows 屏幕录制启动失败：D3D11 上下文为空".to_string(),
    })?;
    let dxgi_device: IDXGIDevice = d3d_device.cast().map_err(|e| AppError::CaptureFailed {
        reason: format!("Windows 屏幕录制启动失败：DXGI 设备转换失败：{e}"),
    })?;
    let direct3d_device = CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)
        .map_err(|e| AppError::CaptureFailed {
            reason: format!("Windows 屏幕录制启动失败：Direct3D 设备创建失败：{e}"),
        })?;

    Ok((d3d_device, d3d_context, direct3d_device))
}
```

If `CreateDirect3D11DeviceFromDXGIDevice` has a different generated signature in `windows 0.62`, adjust only this helper and keep its return contract unchanged.

- [ ] **Step 4: Create primary monitor capture item**

Add helper:

```rust
#[cfg(target_os = "windows")]
unsafe fn create_primary_monitor_item() -> AppResult<GraphicsCaptureItem> {
    let monitor = MonitorFromWindow(HWND(0), MONITOR_DEFAULTTOPRIMARY);
    if monitor.0 == 0 {
        return Err(AppError::CaptureFailed {
            reason: "Windows 屏幕录制启动失败：未找到主显示器".to_string(),
        });
    }

    let interop: IGraphicsCaptureItemInterop =
        windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(|e| AppError::CaptureFailed {
                reason: format!("Windows 屏幕录制启动失败：WGC interop 获取失败：{e}"),
            })?;

    interop.CreateForMonitor(monitor).map_err(|e| AppError::CaptureFailed {
        reason: format!("Windows 屏幕录制启动失败：显示器捕获项创建失败：{e}"),
    })
}
```

- [ ] **Step 5: Create frame pool/session and send `Started` only after `StartCapture()`**

Inside `run_display_capture_worker_inner`, replace the temporary Task 6 failure with:

```rust
if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
    let error = AppError::NativeCaptureUnavailable {
        reason: "Windows 屏幕录制启动失败：当前设备不支持 Windows Graphics Capture",
    };
    let _ = startup_tx.send(GraphicsCaptureStartup::Failed(error.clone()));
    return Err(error);
}

let (_d3d_device, d3d_context, direct3d_device) = match unsafe { create_direct3d_device() } {
    Ok(devices) => devices,
    Err(error) => {
        let _ = startup_tx.send(GraphicsCaptureStartup::Failed(error.clone()));
        return Err(error);
    }
};
let item = match unsafe { create_primary_monitor_item() } {
    Ok(item) => item,
    Err(error) => {
        let _ = startup_tx.send(GraphicsCaptureStartup::Failed(error.clone()));
        return Err(error);
    }
};
let size = item.Size().map_err(|e| AppError::CaptureFailed {
    reason: format!("Windows 屏幕录制启动失败：读取显示器尺寸失败：{e}"),
})?;
let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
    &direct3d_device,
    DirectXPixelFormat::B8G8R8A8UIntNormalized,
    2,
    size,
)
.map_err(|e| AppError::CaptureFailed {
    reason: format!("Windows 屏幕录制启动失败：帧池创建失败：{e}"),
})?;
let session = frame_pool.CreateCaptureSession(&item).map_err(|e| AppError::CaptureFailed {
    reason: format!("Windows 屏幕录制启动失败：捕获会话创建失败：{e}"),
})?;
session.StartCapture().map_err(|e| AppError::CaptureFailed {
    reason: format!("Windows 屏幕录制启动失败：StartCapture 失败：{e}"),
})?;
let _ = startup_tx.send(GraphicsCaptureStartup::Started);
```

- [ ] **Step 6: Copy each frame into owned BGRA bytes**

Add helper:

```rust
#[cfg(target_os = "windows")]
unsafe fn copy_frame_to_owned_bgra(
    frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
    d3d_context: &ID3D11DeviceContext,
    first_raw_nanos: &mut Option<u64>,
) -> AppResult<VideoFrameRef> {
    let surface = frame.Surface().map_err(|e| AppError::CaptureFailed {
        reason: format!("Windows WGC 读取帧 surface 失败：{e}"),
    })?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| AppError::CaptureFailed {
        reason: format!("Windows WGC surface 转换失败：{e}"),
    })?;
    let texture: ID3D11Texture2D = access.GetInterface().map_err(|e| AppError::CaptureFailed {
        reason: format!("Windows WGC texture 获取失败：{e}"),
    })?;

    let mut desc = D3D11_TEXTURE2D_DESC::default();
    texture.GetDesc(&mut desc);
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    desc.MiscFlags = 0;

    let device = {
        let mut device = None;
        texture.GetDevice(Some(&mut device));
        device.ok_or_else(|| AppError::CaptureFailed {
            reason: "Windows WGC texture device 为空".to_string(),
        })?
    };
    let staging = device.CreateTexture2D(&desc, None).map_err(|e| AppError::CaptureFailed {
        reason: format!("Windows WGC staging texture 创建失败：{e}"),
    })?;
    d3d_context.CopyResource(&staging, &texture);

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    d3d_context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        .map_err(|e| AppError::CaptureFailed {
            reason: format!("Windows WGC staging texture 映射失败：{e}"),
        })?;

    let width = desc.Width;
    let height = desc.Height;
    let stride_bytes = (width as usize) * 4;
    let mut bytes = vec![0u8; stride_bytes * height as usize];
    for row in 0..height as usize {
        let src = (mapped.pData as *const u8).add(row * mapped.RowPitch as usize);
        let dst = bytes.as_mut_ptr().add(row * stride_bytes);
        std::ptr::copy_nonoverlapping(src, dst, stride_bytes);
    }
    d3d_context.Unmap(&staging, 0);

    let raw_nanos = frame
        .SystemRelativeTime()
        .map(|t| t.Duration as u64 * 100)
        .unwrap_or_else(|_| 0);
    let timestamp = if raw_nanos == 0 {
        MediaTimestamp::from_nanos(0)
    } else {
        system_relative_time_to_timestamp(first_raw_nanos, raw_nanos)
    };

    Ok(owned_bgra_frame(timestamp, width, height, stride_bytes, bytes))
}
```

If `SystemRelativeTime()` or `Duration` differs in generated bindings, adjust only the timestamp extraction and keep `system_relative_time_to_timestamp()` as the normalizer.

- [ ] **Step 7: Poll frames and send to the media channel**

Use a simple MVP loop:

```rust
let mut first_raw_nanos = None;
let frame_interval_ms = (1000 / config.fps.max(1)).max(1) as u64;
while !stop.load(Ordering::Relaxed) {
    match frame_pool.TryGetNextFrame() {
        Ok(frame) => {
            let video_frame = unsafe {
                copy_frame_to_owned_bgra(&frame, &d3d_context, &mut first_raw_nanos)
            }?;
            let _ = video_sink.try_send_drop_newest(video_frame);
        }
        Err(_) => {
            std::thread::sleep(std::time::Duration::from_millis(frame_interval_ms));
        }
    }
}
drop(session);
drop(frame_pool);
Ok(())
```

If polling does not yield frames reliably, switch to `FrameArrived` with `TypedEventHandler`, but keep the same ownership rule: copy texture bytes into owned Rust memory before sending.

- [ ] **Step 8: Compile and run focused tests**

Run:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib graphics_capture
```

Expected: compile succeeds; non-native helper tests pass.

- [ ] **Step 9: Manual Windows smoke**

Run the app on Windows with FFmpeg enabled if local preview requires it:

```powershell
npm run tauri:dev:ffmpeg
```

Manual gate:

- Start fullscreen recording.
- Record 5-10 seconds.
- Stop.
- Confirm preview opens.
- Confirm `frame_count > 0`.
- Confirm `duration_secs > 0`.
- Confirm `output_path != null` when `ffmpeg` feature is enabled.
- Confirm raw system cursor is visible in the recording.

- [ ] **Step 10: Commit**

```powershell
git add src-tauri/src/platform/windows/graphics_capture.rs src-tauri/Cargo.toml
git commit -m "feat(windows): implement fullscreen WGC video capture"
```

If `Cargo.toml` did not change, omit it from `git add`.

---

## Task 8: Frontend Capability Enforcement

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/components/recording-panel.tsx`
- Modify: `src/components/preview-view.tsx`
- Modify: `src/App.test.tsx`

- [ ] **Step 1: Add capability default in `App.tsx`**

Import:

```ts
fetchPlatformCapabilities,
type PlatformCapabilities,
```

Add:

```ts
const DEFAULT_PLATFORM_CAPABILITIES: PlatformCapabilities = {
  platform: 'unknown',
  supportsFullscreen: true,
  supportsWindow: false,
  supportsArea: false,
  supportsSystemAudio: false,
  supportsMicrophone: false,
  supportsCursorBeautify: false,
}
```

Add state:

```ts
const [platformCapabilities, setPlatformCapabilities] =
  useState<PlatformCapabilities>(DEFAULT_PLATFORM_CAPABILITIES)
```

In the initial load effect:

```ts
void fetchPlatformCapabilities()
  .then((caps) => {
    setPlatformCapabilities(caps)
    if (caps.platform === 'windows') {
      setRecordingMode('fullscreen')
      setSystemAudioEnabled(false)
      setMicEnabled(false)
      setMicDevice(null)
      setDenoiseEnabled(false)
    }
  })
  .catch(() => setPlatformCapabilities(DEFAULT_PLATFORM_CAPABILITIES))
```

- [ ] **Step 2: Clamp start payload**

In `handleStartRecording`, compute effective values:

```ts
const effectiveRecordingMode =
  platformCapabilities.supportsWindow || recordingMode !== 'window'
    ? recordingMode
    : 'fullscreen'
const effectiveSystemAudio =
  platformCapabilities.supportsSystemAudio && systemAudioEnabled
const effectiveMic =
  platformCapabilities.supportsMicrophone && micEnabled
```

Use these in `setCaptureMode` and `setAudioConfig`:

```ts
await setCaptureMode({
  mode: effectiveRecordingMode,
  width: resolution.width,
  height: resolution.height,
  fps,
  windowId:
    effectiveRecordingMode === 'window'
      ? (selectedWindowId ?? undefined)
      : undefined,
})
await setAudioConfig({
  captureSystemAudio: effectiveSystemAudio,
  captureMicrophone: effectiveMic,
  microphoneDevice: effectiveMic ? micDevice : null,
  sampleRate: 48000,
  channels: 2,
  denoiseMode: effectiveMic && denoiseEnabled ? 'highpass' : 'none',
})
```

Add `platformCapabilities` to the hook dependency list.

- [ ] **Step 3: Pass capabilities to `RecordingPanel` and `PreviewView`**

In idle render:

```tsx
<RecordingPanel
  platformCapabilities={platformCapabilities}
  ...
/>
```

In preview render:

```tsx
<PreviewView
  platformCapabilities={platformCapabilities}
  ...
/>
```

- [ ] **Step 4: Update `RecordingPanel` props and disabled behavior**

In `src/components/recording-panel.tsx`, import `type PlatformCapabilities` and add prop:

```ts
platformCapabilities: PlatformCapabilities
```

Use:

```ts
const modeDisabled = (mode: 'fullscreen' | 'window' | 'area') =>
  (mode === 'window' && !platformCapabilities.supportsWindow) ||
  (mode === 'area' && !platformCapabilities.supportsArea)

const systemAudioDisabled = !platformCapabilities.supportsSystemAudio
const micDisabled = !platformCapabilities.supportsMicrophone
```

In mode buttons:

```tsx
disabled={modeDisabled(mode.id)}
onClick={() => {
  if (modeDisabled(mode.id)) return
  handleModeChange(mode.id)
}}
```

In audio buttons:

```tsx
disabled={systemAudioDisabled}
onClick={() => {
  if (systemAudioDisabled) return
  setSystemAudioEnabled(!systemAudioEnabled)
}}
```

and:

```tsx
disabled={micDisabled}
onClick={() => {
  if (micDisabled) return
  setMicEnabled(!micEnabled)
}}
```

Include disabled styling by extending existing `className` branches with:

```ts
(systemAudioDisabled || micDisabled) && 'opacity-50 cursor-not-allowed'
```

Apply per button, not as one shared condition.

- [ ] **Step 5: Update `PreviewView` props and cursor controls**

In `src/components/preview-view.tsx`, import `type PlatformCapabilities` and add prop:

```ts
platformCapabilities?: PlatformCapabilities
```

Inside component:

```ts
const supportsCursorBeautify =
  platformCapabilities?.supportsCursorBeautify ?? true
```

In `getBeautifyConfig().then(...)`, if unsupported, force UI state off:

```ts
if (!supportsCursorBeautify) {
  setCursorMagnification(false)
  setCursorSmoothing(false)
  return
}
```

At the top of `handleBeautifyChange`, guard cursor-only changes:

```ts
if (!supportsCursorBeautify && ('cursorMagnification' in config || 'cursorSmoothing' in config)) {
  setCursorMagnification(false)
  setCursorSmoothing(false)
  return
}
```

Disable the two switches and the magnification slider:

```tsx
<Switch
  disabled={!supportsCursorBeautify}
  checked={supportsCursorBeautify && cursorMagnification}
  ...
/>
```

```tsx
<Slider
  disabled={!supportsCursorBeautify}
  ...
/>
```

Leave auto-trim controls unchanged.

- [ ] **Step 6: Update default test mocks**

In `src/App.test.tsx`, add shared payloads near `invokeMock`:

```ts
const macosCapabilities = {
  platform: 'macos',
  supportsFullscreen: true,
  supportsWindow: true,
  supportsArea: false,
  supportsSystemAudio: true,
  supportsMicrophone: true,
  supportsCursorBeautify: true,
}

const trialLicenseStatus = {
  kind: 'trial',
  trialDaysRemaining: 14,
  isExpired: false,
  activated: false,
}
```

In `beforeEach`, add:

```ts
if (command === 'platform_capabilities') {
  return Promise.resolve(macosCapabilities)
}
if (command === 'license_status') {
  return Promise.resolve(trialLicenseStatus)
}
```

Then update every test-local `invokeMock.mockImplementation((command: string) => { ... })` that renders `<App />` to include the same two branches unless that test intentionally exercises a failure:

```ts
if (command === 'platform_capabilities') return Promise.resolve(macosCapabilities)
if (command === 'license_status') return Promise.resolve(trialLicenseStatus)
```

Use this search to find them:

```powershell
Select-String -Path src/App.test.tsx -Pattern 'mockImplementation\(\(command: string\)'
```

For exact call-count tests, avoid counting initial bootstrap calls. Clear `invokeMock` after the UI has loaded:

```ts
render(<App />)
const startButtons = await screen.findAllByText('开始录制')
await vi.waitFor(() => {
  expect(invokeMock).toHaveBeenCalledWith('platform_capabilities', undefined)
})
invokeMock.mockClear()
fireEvent.click(startButtons[0])
```

Keep the existing start-sequence assertions after the clear:

```ts
expect(invokeMock).toHaveBeenNthCalledWith(1, 'set_capture_mode', {
  payload: { mode: 'fullscreen', width: 1920, height: 1080, fps: 30 },
})
expect(invokeMock).toHaveBeenNthCalledWith(2, 'set_audio_config', {
  payload: {
    captureSystemAudio: true,
    captureMicrophone: false,
    microphoneDevice: null,
    sampleRate: 48000,
    channels: 2,
    denoiseMode: 'none',
  },
})
expect(invokeMock).toHaveBeenNthCalledWith(3, 'start_recording', undefined)
expect(invokeMock).toHaveBeenNthCalledWith(4, 'recording_status', undefined)
```

- [ ] **Step 7: Add Windows capability tests**

Add:

```ts
it('disables unsupported Windows mode and audio controls', async () => {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'unknown', microphone: 'unknown', accessibility: 'unknown' })
    if (command === 'license_status') return Promise.resolve({ kind: 'trial', trialDaysRemaining: 14, isExpired: false, activated: false })
    if (command === 'platform_capabilities') {
      return Promise.resolve({
        platform: 'windows',
        supportsFullscreen: true,
        supportsWindow: false,
        supportsArea: false,
        supportsSystemAudio: false,
        supportsMicrophone: false,
        supportsCursorBeautify: false,
      })
    }
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)
  await screen.findByText('开始录制')

  const windowButton = screen.getAllByText('窗口')[0].closest('button')
  const systemAudioButton = screen.getAllByText('系统音频')[0].closest('button')
  const micButton = screen.getAllByText('麦克风')[0].closest('button')

  await vi.waitFor(() => {
    expect(windowButton).toBeDisabled()
    expect(systemAudioButton).toBeDisabled()
    expect(micButton).toBeDisabled()
  })
})

it('sends fullscreen no-audio payload on Windows even if defaults drift', async () => {
  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'unknown', microphone: 'unknown', accessibility: 'unknown' })
    if (command === 'license_status') return Promise.resolve({ kind: 'trial', trialDaysRemaining: 14, isExpired: false, activated: false })
    if (command === 'platform_capabilities') {
      return Promise.resolve({
        platform: 'windows',
        supportsFullscreen: true,
        supportsWindow: false,
        supportsArea: false,
        supportsSystemAudio: false,
        supportsMicrophone: false,
        supportsCursorBeautify: false,
      })
    }
    if (command === 'set_capture_mode') return Promise.resolve()
    if (command === 'set_audio_config') return Promise.resolve()
    if (command === 'start_recording') return Promise.resolve()
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)
  const startButtons = await screen.findAllByText('开始录制')
  await vi.waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('platform_capabilities', undefined)
  })

  invokeMock.mockClear()
  fireEvent.click(startButtons[0])

  await vi.waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
  })
  expect(invokeMock).toHaveBeenCalledWith('set_capture_mode', {
    payload: { mode: 'fullscreen', width: 1920, height: 1080, fps: 30 },
  })
  expect(invokeMock).toHaveBeenCalledWith('set_audio_config', {
    payload: {
      captureSystemAudio: false,
      captureMicrophone: false,
      microphoneDevice: null,
      sampleRate: 48000,
      channels: 2,
      denoiseMode: 'none',
    },
  })
})
```

- [ ] **Step 8: Run frontend tests**

Run:

```powershell
npm test -- --run src/App.test.tsx
```

Expected: existing tests still pass after adjusting initial command mocks, and new Windows capability tests pass.

- [ ] **Step 9: Commit**

```powershell
git add src/App.tsx src/components/recording-panel.tsx src/components/preview-view.tsx src/App.test.tsx
git commit -m "feat(windows): disable unsupported MVP controls"
```

---

## Task 9: Checklist And Verification Notes

**Files:**
- Modify: `tests/2026-06-19-windows-mvp-checklist.md`

- [ ] **Step 1: Update checklist scope**

At the top of `tests/2026-06-19-windows-mvp-checklist.md`, add:

```markdown
> 2026-06-19 更新：本轮验收范围已收缩为 Windows Video-Only MVP。
> Windows 系统音频、麦克风、窗口录制、区域录制、光标美化不再作为本轮 PASS 项；
> 它们的本轮验收口径是前端禁用 + 后端 fail-fast。
```

- [ ] **Step 2: Replace misleading PASS rows**

For any Windows row that claims PASS for WASAPI, microphone, window recording, cursor overlay, capture geometry, HWND safety, or CPAL Send safety, change status to:

```markdown
不在本轮范围；要求禁用/fail-fast，后续独立设计
```

For fullscreen WGC, use:

```markdown
待验证：需真实 Windows 录制 5-10 秒并确认 frame_count > 0、duration_secs > 0、output_path != None
```

- [ ] **Step 3: Add manual Windows video-only gate**

Add:

```markdown
## Windows Video-Only MVP Manual Gate

- [ ] Windows 应用可启动。
- [ ] 前端只允许全屏录制。
- [ ] 系统音频开关禁用。
- [ ] 麦克风开关禁用。
- [ ] 光标美化入口禁用。
- [ ] 后端直接请求窗口录制返回 `Windows MVP 暂仅支持全屏录制`。
- [ ] 后端直接请求系统音频返回 `Windows MVP 暂不支持系统音频录制，请关闭系统音频后重试`。
- [ ] 后端直接请求麦克风返回 `Windows MVP 暂不支持麦克风录制，请关闭麦克风后重试`。
- [ ] 后端直接请求光标美化返回 `Windows MVP 暂不支持光标美化，请保留系统光标`。
- [ ] 全屏录制 5-10 秒后停止。
- [ ] 停止结果 `failed == false`。
- [ ] 停止结果 `frame_count > 0`。
- [ ] 停止结果 `duration_secs > 0`。
- [ ] 停止结果 `output_path != null`（FFmpeg feature 构建）。
- [ ] 预览可打开。
- [ ] 成功录制进入历史库。
- [ ] 失败录制不进入正常历史库。
- [ ] 录制画面中保留系统原始光标。
```

- [ ] **Step 4: Commit**

```powershell
git add tests/2026-06-19-windows-mvp-checklist.md
git commit -m "docs(windows): update MVP checklist for video-only scope"
```

---

## Task 10: Final Automated And Manual Verification

**Files:**
- No source edits unless verification finds a bug.

- [ ] **Step 1: Run Rust checks**

Run:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib windows
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected:

- `cargo check` passes.
- Windows unit tests pass without real device access.
- Ignored native/manual tests remain ignored unless explicitly run.

- [ ] **Step 2: Run frontend checks**

Run:

```powershell
npm test -- --run
npm run build
```

Expected: Vitest and production build pass.

- [ ] **Step 3: Run Windows manual gate**

Run:

```powershell
npm run tauri:dev:ffmpeg
```

Record the results in `tests/2026-06-19-windows-mvp-checklist.md` under the manual gate added in Task 9.

- [ ] **Step 4: Run macOS regression gate before merge**

On macOS, run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib consume_frames -- --test-threads=1
npm test -- --run
npm run build
```

Manual macOS smoke:

- Fullscreen recording still works.
- Window recording still works.
- System audio still works.
- Microphone still works.
- Cursor beautify/export still works.

- [ ] **Step 5: Commit verification notes if checklist changed**

```powershell
git add tests/2026-06-19-windows-mvp-checklist.md
git commit -m "test(windows): record video-only MVP verification"
```

---

## Self-Review Checklist

- [ ] Every unsupported Windows feature is disabled in frontend and rejected in backend.
- [ ] Windows service validates policy before WGC/WASAPI/CPAL/cursor runtime startup.
- [ ] Writer factory runs before WGC startup.
- [ ] WGC `start_display()` waits for startup handshake.
- [ ] WGC real worker sends `Started` only after `StartCapture()` succeeds.
- [ ] WASAPI does not advertise support and cannot return fake success.
- [ ] Windows consumer receives `requested_system_audio = false` and `requested_microphone = false`.
- [ ] Windows raw cursor remains visible; cursor overlay controls are disabled.
- [ ] macOS default capabilities keep existing controls enabled.
- [ ] No production test touches real native capture devices.
- [ ] `Cargo.toml` changes, if any, add features only and do not change crate versions.
