// ⚠️ 人工审查检查点：
// - CGEventCreate returns a retained event that must be released with CFRelease.
// - CGEventSourceButtonState only reads button state; it must not post or synthesize input.
// - This source is polled from CursorMetadataRuntime, not from ScreenCaptureKit callbacks.
// - Polling failures return a structured Rust error and must not panic.

use crate::app::cursor_metadata_runtime::{CursorSnapshot, CursorSnapshotSource};
use crate::app::error::{AppError, AppResult};

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[allow(non_camel_case_types)]
type CGEventRef = *const std::ffi::c_void;

#[allow(non_camel_case_types)]
type CFTypeRef = *const std::ffi::c_void;

const K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION_STATE: i32 = 0;
const K_CG_MOUSE_BUTTON_LEFT: u32 = 0;
const K_CG_MOUSE_BUTTON_RIGHT: u32 = 1;
const K_CG_MOUSE_BUTTON_CENTER: u32 = 2;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *const std::ffi::c_void) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
}

/// macOS cursor snapshot source backed by CoreGraphics.
pub struct MacCursorSource;

impl MacCursorSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacCursorSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorSnapshotSource for MacCursorSource {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot> {
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return Err(AppError::CursorProcessingFailed {
                    reason: "读取鼠标位置失败".to_string(),
                });
            }

            let point = CGEventGetLocation(event);
            CFRelease(event as CFTypeRef);

            Ok(CursorSnapshot {
                x: point.x as f32,
                y: point.y as f32,
                left_down: CGEventSourceButtonState(
                    K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION_STATE,
                    K_CG_MOUSE_BUTTON_LEFT,
                ),
                right_down: CGEventSourceButtonState(
                    K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION_STATE,
                    K_CG_MOUSE_BUTTON_RIGHT,
                ),
                middle_down: CGEventSourceButtonState(
                    K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION_STATE,
                    K_CG_MOUSE_BUTTON_CENTER,
                ),
            })
        }
    }
}
