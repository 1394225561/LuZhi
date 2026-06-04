// ⚠️ 人工审查检查点：
// - CGEventCreate returns a retained event that must be released with CFRelease.
// - CGEventSourceButtonState only reads button state; it must not post or synthesize input.
// - This source is polled from CursorMetadataRuntime, not from ScreenCaptureKit callbacks.
// - Polling failures return a structured Rust error and must not panic.
// - Kind query uses AXUIElementCreateSystemWide (not AXUIElementCreateApplication(0)).

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::cursor_metadata_runtime::{CursorSnapshot, CursorSnapshotSource};
use crate::app::error::{AppError, AppResult};
use crate::core::clock::SessionClock;
use crate::platform::macos::cursor_kind;

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
///
/// Records `captured_at_nanos` immediately after `CGEventGetLocation()` to
/// avoid timing pollution from the subsequent AX kind query.
///
/// Kind queries are rate-limited to 10Hz (100ms TTL) to avoid blocking the
/// cursor runtime thread on every snapshot.
pub struct MacCursorSource {
    session_clock: Arc<SessionClock>,
    /// Kind is cached and only re-queried at most once per this interval.
    kind_cache_ttl: Duration,
    cached_kind: crate::core::timeline::CursorKind,
    last_kind_query_at: Instant,
}

impl MacCursorSource {
    pub fn new(session_clock: Arc<SessionClock>) -> Self {
        Self {
            session_clock,
            kind_cache_ttl: Duration::from_millis(100), // 10Hz kind query
            cached_kind: crate::core::timeline::CursorKind::Arrow,
            last_kind_query_at: Instant::now() - Duration::from_secs(1),
        }
    }
}

impl CursorSnapshotSource for MacCursorSource {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot> {
        let start = Instant::now();
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return Err(AppError::CursorProcessingFailed {
                    reason: "读取鼠标位置失败".to_string(),
                });
            }

            let point = CGEventGetLocation(event);
            CFRelease(event as CFTypeRef);

            // Record position sampling time immediately after CGEventGetLocation.
            let captured_at_nanos = self.session_clock.elapsed_nanos();

            // Kind query: cached, low-frequency (10Hz max).
            if start.duration_since(self.last_kind_query_at) >= self.kind_cache_ttl {
                self.cached_kind = cursor_kind::query_cursor_kind(point.x as f32, point.y as f32);
                self.last_kind_query_at = start;
            }

            let snapshot_duration_nanos = start.elapsed().as_nanos() as u64;

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
                kind: self.cached_kind,
                captured_at_nanos,
                snapshot_duration_nanos,
            })
        }
    }
}
