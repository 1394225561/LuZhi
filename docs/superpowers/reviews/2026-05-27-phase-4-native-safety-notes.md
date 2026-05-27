# Phase 4 Native Safety Notes

## Reviewed Files

- `src-tauri/src/platform/macos/cursor_source.rs`
- `src-tauri/src/platform/macos/screen_capture_kit.rs`

## Checks

- [x] `CGEventCreate` result is null-checked.
- [x] `CGEventCreate` retained event is released with `CFRelease`.
- [x] Cursor polling reads state only and does not synthesize input.
- [x] Cursor polling runs in `CursorMetadataRuntime`, separate from ScreenCaptureKit callbacks.
- [x] `SCStreamConfiguration::setShowsCursor` is controlled through `CaptureConfig.show_system_cursor`.

## Code Walkthrough

### cursor_source.rs

- `CGEventCreate(std::ptr::null())` creates a new NULL event which can be queried for cursor location. Returns retained object.
- null check on the returned `CGEventRef` before use, returning `CursorProcessingFailed` error if null.
- `CGEventGetLocation(event)` reads the cursor position from the event.
- `CFRelease(event as CFTypeRef)` releases the retained event object — confirmed pairing.
- `CGEventSourceButtonState(kCGEventSourceStateCombinedSessionState, button)` reads current button state for left/right/middle.
- `CGEventSourceButtonState` only reads state, does NOT post or synthesize events.

### screen_capture_kit.rs

- `stream_config.setShowsCursor(config.show_system_cursor)` — controlled by `CaptureConfig`.
- `CaptureConfig.show_system_cursor` defaults to `true` for backward compatibility.
- When cursor beautify is enabled (`cursor_magnification || cursor_smoothing`), `show_system_cursor` is set to `false` to avoid double cursor.

## Remaining Manual Checks

- [ ] Verify app permission prompts on a clean macOS user account.
- [ ] Verify no double cursor appears when cursor beautify is enabled (requires `npm run tauri dev` manual test).
