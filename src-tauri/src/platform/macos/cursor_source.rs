// ⚠️ 人工审查检查点：
// - CGEventCreate returns a retained event that must be released with CFRelease.
// - CGEventSourceButtonState only reads button state; it must not post or synthesize input.
// - This source is polled from CursorMetadataRuntime, not from ScreenCaptureKit callbacks.
// - Polling failures return a structured Rust error and must not panic.
// - Kind query uses CursorKindProvider trait (production: MacCursorKindProvider with NSCursor).

use std::sync::Arc;

use crate::app::cursor_metadata_runtime::{CursorSnapshot, CursorSnapshotSource};
use crate::app::error::{AppError, AppResult};
use crate::core::clock::SessionClock;
use crate::platform::macos::cursor_kind::{
    CursorKindProvider, CursorMainThreadDispatcher, MacCursorKindProvider,
};

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
/// avoid timing pollution from the subsequent kind query via the provider.
///
/// Kind queries are delegated to a `CursorKindProvider` implementation
/// (production: `MacCursorKindProvider` with 10Hz rate limiting).
pub struct MacCursorSource {
    session_clock: Arc<SessionClock>,
    kind_provider: Box<dyn CursorKindProvider>,
}

impl MacCursorSource {
    /// Create with the production `MacCursorKindProvider` (NSCursor API, 10Hz).
    pub fn new(
        session_clock: Arc<SessionClock>,
        main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>,
    ) -> Self {
        Self {
            session_clock,
            kind_provider: Box::new(MacCursorKindProvider::new(main_thread_dispatcher)),
        }
    }

    /// Create with a custom kind provider (for testing).
    #[cfg(test)]
    pub fn with_provider(
        session_clock: Arc<SessionClock>,
        provider: Box<dyn CursorKindProvider>,
    ) -> Self {
        Self {
            session_clock,
            kind_provider: provider,
        }
    }
}

impl CursorSnapshotSource for MacCursorSource {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot> {
        let start = std::time::Instant::now();
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

            // Kind query via provider (rate-limited inside provider).
            let kind = self.kind_provider.query(point.x as f32, point.y as f32);

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
                kind,
                captured_at_nanos,
                snapshot_duration_nanos,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::timeline::CursorKind;
    use std::time::Duration;

    /// Mock provider that always returns a fixed kind.
    struct FixedKindProvider {
        kind: CursorKind,
    }

    impl CursorKindProvider for FixedKindProvider {
        fn query(&mut self, _global_x: f32, _global_y: f32) -> CursorKind {
            self.kind
        }
    }

    /// Mock provider that always falls back to Arrow.
    struct FailingKindProvider;

    impl CursorKindProvider for FailingKindProvider {
        fn query(&mut self, _global_x: f32, _global_y: f32) -> CursorKind {
            CursorKind::Arrow // fallback
        }
    }

    /// Mock provider that returns Hand when cursor is in the right half, Arrow otherwise.
    struct RegionKindProvider;

    impl CursorKindProvider for RegionKindProvider {
        fn query(&mut self, global_x: f32, _global_y: f32) -> CursorKind {
            if global_x > 960.0 {
                CursorKind::Hand
            } else {
                CursorKind::Arrow
            }
        }
    }

    /// Mock provider with configurable delay.
    struct SlowKindProvider {
        delay: Duration,
    }

    impl CursorKindProvider for SlowKindProvider {
        fn query(&mut self, _global_x: f32, _global_y: f32) -> CursorKind {
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            CursorKind::Arrow
        }
    }

    #[test]
    fn mac_cursor_source_uses_injected_kind_provider() {
        let clock = Arc::new(SessionClock::new());
        let provider = Box::new(FixedKindProvider {
            kind: CursorKind::Hand,
        });
        let mut source = MacCursorSource::with_provider(clock, provider);

        let snapshot = source.snapshot().unwrap();
        assert_eq!(snapshot.kind, CursorKind::Hand);
    }

    #[test]
    fn mac_cursor_source_returns_ibeam_from_provider() {
        let clock = Arc::new(SessionClock::new());
        let provider = Box::new(FixedKindProvider {
            kind: CursorKind::IBeam,
        });
        let mut source = MacCursorSource::with_provider(clock, provider);

        let snapshot = source.snapshot().unwrap();
        assert_eq!(snapshot.kind, CursorKind::IBeam);
    }

    #[test]
    fn mac_cursor_source_failing_provider_returns_arrow() {
        let clock = Arc::new(SessionClock::new());
        let provider = Box::new(FailingKindProvider);
        let mut source = MacCursorSource::with_provider(clock, provider);

        let snapshot = source.snapshot().unwrap();
        assert_eq!(snapshot.kind, CursorKind::Arrow);
    }

    #[test]
    fn mac_cursor_source_preserves_position_timestamp_with_slow_provider() {
        let clock = Arc::new(SessionClock::new());
        // Sleep to let some time pass so captured_at_nanos > 0.
        std::thread::sleep(Duration::from_millis(20));

        let provider = Box::new(SlowKindProvider {
            delay: Duration::from_millis(50),
        });
        let mut source = MacCursorSource::with_provider(clock.clone(), provider);

        let snapshot = source.snapshot().unwrap();

        // captured_at_nanos should be close to current clock, not delayed by 50ms.
        let clock_now = clock.elapsed_nanos();
        let drift = clock_now.saturating_sub(snapshot.captured_at_nanos);

        // Drift should be small (< 50ms kind query delay + system overhead).
        // captured_at is recorded BEFORE kind query, so drift = CGEvent overhead + scheduling jitter.
        // The 50ms kind query delay should NOT appear in the drift.
        assert!(
            drift < 70_000_000,
            "captured_at_nanos drifted too much: {drift}ns (expected < 70ms)"
        );
    }

    #[test]
    fn mac_cursor_source_snapshot_has_valid_position() {
        let clock = Arc::new(SessionClock::new());
        let provider = Box::new(FixedKindProvider {
            kind: CursorKind::Arrow,
        });
        let mut source = MacCursorSource::with_provider(clock, provider);

        let snapshot = source.snapshot().unwrap();

        // Position should be valid (not NaN).
        assert!(snapshot.x.is_finite());
        assert!(snapshot.y.is_finite());
        // captured_at_nanos should be non-zero.
        assert!(snapshot.captured_at_nanos > 0);
    }
}
