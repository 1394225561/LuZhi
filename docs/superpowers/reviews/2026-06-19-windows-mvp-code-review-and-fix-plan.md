# Windows MVP Code Review And Fix Plan

> Review range: `e71cf51d95602f019f6cfe8629f168fd5944dda2..637cdc899bc74a5476a90e0214d917504b70a9f8`
>
> Review date: 2026-06-19
>
> Scope: Windows MVP implementation, shared recording consumer refactor, and macOS regression risk.

## Summary

This round is **not ready to merge**.

The Windows path currently opens the build path and service boundary, but core WGC/WASAPI workers still have false-success behavior: `start()` can report success while no frames or system audio are produced. There is also a macOS compile-risk introduced by moving CPAL microphone capture to a shared module while leaving diagnostics typed against the old macOS module.

Priority is:

1. Restore macOS compile/runtime safety.
2. Remove Windows false-success capture paths.
3. Fix Windows stop/finalize resource safety.
4. Replace native-touching tests with mockable boundaries.
5. Re-run cross-platform verification before marking checklist rows as PASS.

## Subagent Coverage

- Windows native reviewer: checked WGC/WASAPI/window enumeration/Windows service state and native safety.
- macOS regression reviewer: checked `MacRecordingService`, cursor metadata, shared consumer compatibility, and existing macOS behavior.
- Shared pipeline reviewer: checked recording consumer, writer diagnostics/source-aware contracts, test coverage, and BUG.md prevention rules.

## Critical Findings

### 1. Windows WGC reports success without capture

**Location:** `src-tauri/src/platform/windows/graphics_capture.rs:104`, `src-tauri/src/platform/windows/graphics_capture.rs:129`, `src-tauri/src/platform/windows/graphics_capture.rs:259`

**Problem:** `start_display()` and `start_window()` return `Ok(())` immediately after spawning a worker, but the display worker only sleeps until stop. The window worker returns `NativeCaptureUnavailable` inside the spawned thread. The UI can enter `Recording` even though no WGC session exists and no video frames can arrive.

**Impact:** Windows full-screen/window recording can look started while producing no video. Failure appears late at stop/finalize, which violates fail-visible behavior and MVP completeness.

**Fix direction:**

- Implement WGC startup synchronously enough to prove capture is ready before returning `Ok(())`.
- Add a startup handshake channel from worker to `start_display()` / `start_window()`.
- Return `NativeCaptureUnavailable` or `CaptureFailed` before changing the user-visible flow to Recording when WGC is not implemented or cannot start.
- For window capture, do not expose it as started until `GraphicsCaptureItem` and frame pool/session creation succeed.

**Verification:**

- Add a unit/integration-style test using a mock Windows graphics capture starter that can report startup failure before `WindowsRecordingService::start()` returns.
- Manual Windows test must prove at least one video frame reaches the consumer before the session is considered started.

### 2. WASAPI loopback reports support without producing audio

**Location:** `src-tauri/src/platform/windows/wasapi_loopback.rs:53`, `src-tauri/src/platform/windows/wasapi_loopback.rs:95`, `src-tauri/src/platform/windows/wasapi_loopback.rs:159`

**Problem:** `WasapiLoopback::start()` returns success and `capabilities()` advertises system audio support, but the worker never initializes WASAPI and never sends audio chunks.

**Impact:** Default Windows recording requests system audio, enters a false started state, then fails source-aware/audio artifact validation because no system chunks arrived.

**Fix direction:**

- Implement actual WASAPI shared loopback capture before advertising support.
- Until implemented, return `NativeCaptureUnavailable` from `start()` and report `supports_system_audio=false`.
- Ensure unavailable errors tell the user they can disable system audio.

**Verification:**

- Test that unsupported/unimplemented loopback fails at start, not at stop.
- On Windows hardware, record system audio and verify `system_chunks_received > 0`, `system_windows_before_writer > 0`, and writer source counters are non-zero.

### 3. macOS CPAL diagnostics type mismatch after shared module move

**Location:** `src-tauri/src/media/recording_writer.rs:108`, `src-tauri/src/platform/macos_service.rs:524`

**Problem:** `MacRecordingService` now uses `crate::platform::cpal_microphone::CpalMicrophoneCapture`, but `RecordingDiagnostics::mic_stop_diagnostics` and `RecordingFinalizeGuard::mic_stop_diag` still use `crate::platform::macos::cpal_microphone::CpalMicrophoneStopDiagnostics`.

**Impact:** macOS build is at risk of compile-time type mismatch when `stop_with_diagnostics()` returns the shared module type. This directly threatens existing macOS recording stop/finalize.

**Fix direction:**

- Change diagnostics types to `crate::platform::cpal_microphone::CpalMicrophoneStopDiagnostics`.
- Remove the duplicate macOS CPAL module or re-export the shared type from the old path to avoid two distinct diagnostics types.
- Update tests that import the old path.

**Verification:**

- Run macOS-targeted `cargo check --manifest-path src-tauri/Cargo.toml`.
- Run macOS recording consumer/finalize tests, especially tests involving `mic_stop_diagnostics`.

### 4. Windows consumer timeout is defeated by unbounded join

**Location:** `src-tauri/src/platform/windows_service.rs:409`, `src-tauri/src/platform/windows_service.rs:422`

**Problem:** `stop()` uses `recv_timeout(CONSUMER_RESULT_TIMEOUT)`, but then unconditionally calls `handle.join()`. If the consumer is stuck in `writer.finish()`, artifact validation, or any blocking path, stop can still hang forever.

**Impact:** Violates BUG.md prevention rule for consumer timeout. User can get stuck stopping a Windows recording.

**Fix direction:**

- Reuse or extract the macOS `receive_consumer_output_with_timeout` pattern.
- On timeout, detach the consumer handle and return a visible failed response with `finalization_errors`.
- Do not call unbounded `join()` after timeout.

**Verification:**

- Add a Windows service/helper test with a parked consumer thread and a short timeout. The test must return quickly and set failed/error state.

### 5. Windows writer creation can leak started native resources

**Location:** `src-tauri/src/platform/windows_service.rs:161`, `src-tauri/src/platform/windows_service.rs:208`, `src-tauri/src/platform/windows_service.rs:333`

**Problem:** Windows starts WGC/WASAPI/mic/cursor runtime before constructing the writer. If `FfmpegRecordingWriter::new()` or `with_fps()` fails, `?` returns without stopping already-started resources and without driving the state machine to `Failed`.

**Impact:** Capture threads/runtimes can leak, and the service can remain in `Recording` after startup failure.

**Fix direction:**

- Prefer creating the writer before starting native capture resources, matching the safer macOS window path.
- If writer must be created later, wrap startup in a rollback guard that stops cursor, graphics, system audio, and mic, then calls `state_machine.fail()`.

**Verification:**

- Add a test writer factory that fails and assert all started resources are stopped and service state becomes `Failed`.

## Important Findings

### 6. Windows pause/resume does not update state machine

**Location:** `src-tauri/src/platform/windows_service.rs:511`

**Problem:** Windows `pause()` and `resume()` only toggle `pause_flag`; they do not call `state_machine.pause()` / `state_machine.resume()`.

**Impact:** UI/event consumers can continue seeing `Recording` while the consumer is dropping media as paused. State guards and telemetry become inconsistent.

**Fix direction:** Mirror macOS: transition the state machine first, then update the pause flag.

**Verification:** Add tests for Windows pause/resume state transitions and invalid-state errors.

### 7. Windows HWND is truncated to u32

**Location:** `src-tauri/src/platform/windows/window_capture.rs:112`

**Problem:** Raw `HWND` is stored as `isize`, then serialized into `WindowInfo.window_id: u32`.

**Impact:** 64-bit window handles can be corrupted. A selected window may not be recapturable or may refer to the wrong handle.

**Fix direction:**

- Make window IDs platform-safe: `u64`, `isize`, or opaque string.
- Preserve the full `HWND` value through frontend selection and back into native capture.
- Audit macOS call sites if changing the shared `WindowInfo` type.

**Verification:** Add a Windows unit test for a high-bit handle value and round-trip through `WindowInfo`.

### 8. Windows set_window_id skips validation

**Location:** `src-tauri/src/lib.rs:1121`

**Problem:** `set_window_id` validates existence/minimized state only on macOS. Windows accepts stale, minimized, elevated, protected, or inaccessible windows.

**Impact:** Violates the Windows security rule requiring clear prompts for restricted/UAC windows. Users can select invalid windows and only discover failure later.

**Fix direction:**

- On Windows, validate `IsWindow`, visibility, minimized state, and likely access constraints.
- Return `WindowNotFound`, `WindowMinimized`, or `WindowAccessDenied` with clear Chinese guidance.

**Verification:** Add unit coverage for mapping validation outcomes to user-facing errors. Manually test minimized and elevated target windows.

### 9. Shared CPAL stream Send safety is not justified on Windows

**Location:** `src-tauri/src/platform/cpal_microphone.rs:31`

**Problem:** `unsafe impl Send for SendStream` moved to a shared module, but the safety comment only justifies CoreAudio. Windows now uses the same type with CPAL WASAPI streams.

**Impact:** CPAL marks `Stream` as non-Send. Moving/dropping a WASAPI-backed stream across Tauri blocking threads may be unsound without a platform-specific lifecycle guarantee.

**Fix direction:**

- Either add a Windows-specific safety audit and comment, or avoid cross-thread ownership by running CPAL stream lifecycle on a dedicated owner thread.
- Consider cfg-gating platform implementations if CoreAudio and WASAPI require different ownership models.

**Verification:** Manual/native review required. This should not be accepted on tests alone.

### 10. CPAL running flag is set after stream.play()

**Location:** `src-tauri/src/platform/cpal_microphone.rs:253`

**Problem:** `stream.play()` is called before `running.store(true)`. A callback can arrive before `play()` returns and be treated as stopped.

**Impact:** First microphone buffer can be dropped, shifting lazy timestamp offset and creating avoidable leading gaps/padding.

**Fix direction:** Set `running=true` before `play()`, and roll it back to `false` if `play()` fails.

**Verification:** Add a unit-level test around the start-state ordering if the stream builder is made injectable, or cover with a mock stream lifecycle.

### 11. Failed recordings can still be registered as normal history entries

**Location:** `src-tauri/src/lib.rs:179`, `src-tauri/src/lib.rs:247`

**Problem:** `register_recording_response()` registers any response with `output_path`, even when `response.failed == true`.

**Impact:** Source-aware/audio contract failures can pollute the normal recording library and later be exported as if they were valid recordings.

**Fix direction:** Skip registration when `failed == true`, or register as an explicit failed entry that the UI cannot export as normal media.

**Verification:** Add a test for `StopRecordingResponse { failed: true, output_path: Some(...) }` that proves no normal library entry is created.

### 12. Cursor metadata has no Windows capture geometry

**Location:** `src-tauri/src/platform/windows_service.rs:199`, `src-tauri/src/platform/windows_service.rs:324`

**Problem:** Windows passes `None` for `capture_geometry`, so cursor samples remain global screen coordinates. The overlay renderer expects source video pixel coordinates.

**Impact:** Cursor overlay can be offset on non-primary displays, scaled displays, and window capture. Window capture is especially likely to be wrong.

**Fix direction:**

- Populate Windows `CaptureGeometry` from the captured display/window bounds and output frame size.
- For window capture, use the selected window content bounds and WGC output size.

**Verification:** Add coordinate mapping tests for a non-zero-origin display/window.

### 13. Windows native test touches real device APIs and fails

**Location:** `src-tauri/src/platform/windows_service.rs:586`

**Problem:** `windows_service_fails_visibly_before_native_capture_exists` can start real graphics/mic paths. On the review machine it failed with CPAL/WASAPI panic: `HRESULT(0x80040154) "没有注册类"`.

**Impact:** Violates project testing rule: tests must mock native recording APIs, not call real system capture devices.

**Fix direction:**

- Extract injectable capture/audio/writer factories for `WindowsRecordingService` tests.
- Replace native-touching unit tests with mock startup success/failure cases.

**Verification:** `cargo test --manifest-path src-tauri/Cargo.toml --lib windows` should pass without needing real capture/audio device access.

## Minor Findings

### 14. Windows window capture ignores show_system_cursor and config

**Location:** `src-tauri/src/platform/windows_service.rs:263`, `src-tauri/src/platform/windows_service.rs:284`

**Problem:** `_show_system_cursor` is ignored, and window capture uses `CaptureConfig::full_screen_1080p_30fps()` instead of the selected config.

**Impact:** Future window capture can record raw cursor while metadata says cursor was hidden, creating double cursor overlays.

**Fix direction:** Pass actual config and cursor visibility into the Windows WGC window path.

### 15. Checklist marks unverified items as PASS

**Location:** `tests/2026-06-19-windows-mvp-checklist.md:54`

**Problem:** The status table marks platform boundary/shared consumer/native safety as PASS while upper checklist items remain unchecked and native safety still has blocking findings.

**Impact:** Creates false confidence during handoff.

**Fix direction:** Only mark PASS when supported by command output or manual verification evidence. Otherwise use `待验证` or `失败`.

## Recommended Fix Order

1. Fix macOS CPAL diagnostics type mismatch first.
2. Replace Windows false-success WGC/WASAPI behavior with fail-fast startup or real implementations.
3. Fix Windows stop timeout and writer-start rollback.
4. Fix Windows state transitions, window ID representation, and Windows `set_window_id` validation.
5. Audit shared CPAL Send safety before relying on Windows microphone recording.
6. Add Windows capture geometry before claiming cursor overlay correctness.
7. Replace native-touching Windows tests with mockable service tests.
8. Update checklist/docs only after verification passes.

## Verification Commands

Run on Windows:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib windows
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm test -- --run
npm run build
```

Run on macOS before merge:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib consume_frames -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib mic_stop_diagnostics
npm test -- --run
npm run build
```

Manual Windows gates:

- Start full-screen recording and confirm video frames reach writer.
- Record system audio and confirm audible output plus non-zero source-aware diagnostics.
- Record microphone and confirm no device/API panic.
- Select minimized/elevated/protected windows and confirm clear Chinese errors.
- Stop while writer/consumer is slow or blocked and confirm UI receives a failed response rather than hanging.

Manual macOS gates:

- Full-screen recording with system audio and microphone still starts/stops.
- Window recording still handles minimize/restore/close.
- Cursor metadata/effect timeline still aligns after export.
- Existing low-volume/source-aware audio contracts still behave according to BUG.md rules.

## Review-Time Verification

Executed during review on Windows:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Result: passed with warnings.

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib windows
```

Result: failed. `platform::windows_service::tests::windows_service_fails_visibly_before_native_capture_exists` touched real CPAL/WASAPI path and panicked with `HRESULT(0x80040154) "没有注册类"`.

macOS verification was not executed on this Windows review machine; the CPAL diagnostics issue was confirmed by static path/type inspection.
