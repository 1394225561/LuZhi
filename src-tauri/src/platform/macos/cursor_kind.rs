//! macOS CursorKind provider using Accessibility hit-test.
//!
//! Queries the Accessibility API (`AXUIElementCopyElementAtPosition`) to determine
//! the type of UI element under the cursor. Falls back to `Arrow` on failure.
//!
//! # Safety
//!
//! `AXUIElementCopyElementAtPosition` requires the app to have Accessibility
//! permission (System Preferences → Privacy & Security → Accessibility).
//! If not granted, the call returns an error and we fall back to `Arrow`.
//!
//! This module is polled from `CursorMetadataRuntime`, NOT from
//! ScreenCaptureKit callbacks — it must not block the capture main loop.
//!
//! # Diagnostics
//!
//! `AtomicU64` counters track query failures, fallback-to-Arrow counts, and
//! per-kind distribution. These are exposed via `cursor_kind_diagnostics_snapshot()`
//! for diagnostic logging.
//!
//! # System-Wide Root
//!
//! Uses `AXUIElementCreateSystemWide()` (not `AXUIElementCreateApplication(0)`)
//! as the root for cross-app hit-testing, per Apple SDK documentation.

use crate::core::timeline::CursorKind;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Diagnostic counters
// ---------------------------------------------------------------------------

/// Diagnostic counter for accessibility query failures.
static AX_QUERY_FAILURE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Diagnostic counter for successful queries that returned Arrow (fallback).
static AX_FALLBACK_ARROW_COUNT: AtomicU64 = AtomicU64::new(0);

/// Kind distribution counters.
static KIND_ARROW_COUNT: AtomicU64 = AtomicU64::new(0);
static KIND_HAND_COUNT: AtomicU64 = AtomicU64::new(0);
static KIND_IBEAM_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// FFI types and bindings
// ---------------------------------------------------------------------------

type CFTypeRef = *const std::ffi::c_void;
type AXUIElementRef = *const std::ffi::c_void;
type CFStringRef = *const std::ffi::c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout_in_seconds: f32) -> i32;
    fn AXUIElementCopyElementAtPosition(
        application: AXUIElementRef,
        x: f64,
        y: f64,
        element: *mut AXUIElementRef,
    ) -> i32;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut CFTypeRef) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFStringCreateWithCString(
        allocator: *const std::ffi::c_void,
        c_str: *const std::ffi::c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFGetTypeID(cf: CFTypeRef) -> u64;
    fn CFStringGetTypeID() -> u64;
    fn CFStringGetLength(the_string: CFStringRef) -> i64;
    fn CFStringGetCString(
        the_string: CFStringRef,
        buffer: *mut std::ffi::c_char,
        buffer_size: i64,
        encoding: u32,
    ) -> bool;
    fn CFArrayGetCount(the_array: *const std::ffi::c_void) -> i64;
    fn CFArrayGetValueAtIndex(the_array: *const std::ffi::c_void, idx: i64) -> *const std::ffi::c_void;
}

const K_CFStringEncoding_UTF8: u32 = 0x08000100;
const K_AX_ERROR_SUCCESS: i32 = 0;

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Diagnostics for cursor kind queries.
#[derive(Clone, Debug, Default)]
pub struct CursorKindDiagnostics {
    pub ax_query_failure_count: u64,
    pub ax_fallback_arrow_count: u64,
    pub arrow_count: u64,
    pub hand_count: u64,
    pub ibeam_count: u64,
}

/// Snapshot of cursor kind distribution for diagnostics logging.
pub fn cursor_kind_diagnostics_snapshot() -> CursorKindDiagnostics {
    CursorKindDiagnostics {
        ax_query_failure_count: AX_QUERY_FAILURE_COUNT.load(Ordering::Relaxed),
        ax_fallback_arrow_count: AX_FALLBACK_ARROW_COUNT.load(Ordering::Relaxed),
        arrow_count: KIND_ARROW_COUNT.load(Ordering::Relaxed),
        hand_count: KIND_HAND_COUNT.load(Ordering::Relaxed),
        ibeam_count: KIND_IBEAM_COUNT.load(Ordering::Relaxed),
    }
}

/// Get the current accessibility query failure count (for diagnostics).
pub fn ax_query_failure_count() -> u64 {
    AX_QUERY_FAILURE_COUNT.load(Ordering::Relaxed)
}

/// Get the fallback arrow count (for diagnostics).
pub fn ax_fallback_arrow_count() -> u64 {
    AX_FALLBACK_ARROW_COUNT.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// CursorKindProvider trait
// ---------------------------------------------------------------------------

/// Trait abstracting cursor kind queries for testability.
pub trait CursorKindProvider: Send + 'static {
    fn query(&mut self, global_x: f32, global_y: f32) -> CursorKind;
}

/// Production macOS cursor kind provider using Accessibility API.
///
/// Uses `AXUIElementCreateSystemWide()` for cross-app hit-testing.
/// Rate-limited to 10Hz (100ms cache TTL).
pub struct MacCursorKindProvider {
    kind_cache_ttl: Duration,
    cached_kind: CursorKind,
    last_query_at: Instant,
}

impl MacCursorKindProvider {
    pub fn new() -> Self {
        Self {
            kind_cache_ttl: Duration::from_millis(100),
            cached_kind: CursorKind::Arrow,
            last_query_at: Instant::now() - Duration::from_secs(1),
        }
    }
}

impl Default for MacCursorKindProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorKindProvider for MacCursorKindProvider {
    fn query(&mut self, global_x: f32, global_y: f32) -> CursorKind {
        let now = Instant::now();
        if now.duration_since(self.last_query_at) < self.kind_cache_ttl {
            return self.cached_kind;
        }
        self.cached_kind = query_cursor_kind(global_x, global_y);
        self.last_query_at = now;
        self.cached_kind
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Query the cursor kind at the given global screen position.
///
/// Uses `AXUIElementCreateSystemWide()` to create a system-wide accessibility
/// object, then `AXUIElementCopyElementAtPosition()` to find the UI element
/// under the cursor, then reads its role to determine the cursor kind.
///
/// Falls back to `Arrow` on any failure (no accessibility permission, timeout,
/// unexpected role, etc.).
pub fn query_cursor_kind(global_x: f32, global_y: f32) -> CursorKind {
    unsafe {
        // Use the proper system-wide accessibility object for cross-app hit-testing.
        // AXUIElementCreateApplication(0) is NOT a system-wide object per Apple SDK.
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            AX_QUERY_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return CursorKind::Arrow;
        }

        // Set a short timeout to avoid blocking the cursor runtime thread.
        AXUIElementSetMessagingTimeout(system, 0.05); // 50ms

        let mut element: AXUIElementRef = std::ptr::null();
        let result = AXUIElementCopyElementAtPosition(
            system,
            global_x as f64,
            global_y as f64,
            &mut element,
        );
        CFRelease(system);

        if result != K_AX_ERROR_SUCCESS || element.is_null() {
            AX_QUERY_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return CursorKind::Arrow;
        }

        let kind = element_role_to_cursor_kind(element);
        CFRelease(element);

        // Update kind distribution counters.
        match kind {
            CursorKind::Arrow => {
                AX_FALLBACK_ARROW_COUNT.fetch_add(1, Ordering::Relaxed);
                KIND_ARROW_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            CursorKind::Hand => {
                KIND_HAND_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            CursorKind::IBeam => {
                KIND_IBEAM_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        kind
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Map an AXUIElement's role to a CursorKind, with parent chain fallback.
///
/// Classification strategy:
/// - IBeam: AXTextField, AXTextArea, or editable text
/// - Hand: AXButton, AXLink, AXMenuItem, or element/parent supports AXPress
/// - Arrow: everything else
///
/// For elements that hit child text/group/image (common in browsers, Electron, Tauri),
/// we walk up to 3 parent levels looking for a clickable or editable container.
unsafe fn element_role_to_cursor_kind(element: AXUIElementRef) -> CursorKind {
    let mut current = element;
    let mut retained_refs: Vec<AXUIElementRef> = Vec::new();

    for depth in 0..4 {
        // Max 4 levels: element + 3 parents.
        let role = read_role(current);

        match role.as_deref() {
            // IBeam: text input elements (not plain static text).
            Some("AXTextField") | Some("AXTextArea") => {
                cleanup_refs(&retained_refs);
                return CursorKind::IBeam;
            }
            // Hand: clickable elements.
            Some("AXButton")
            | Some("AXLink")
            | Some("AXMenuItem")
            | Some("AXMenuBarItem")
            | Some("AXCheckBox")
            | Some("AXRadioButton")
            | Some("AXPopUpButton")
            | Some("AXComboBox") => {
                cleanup_refs(&retained_refs);
                return CursorKind::Hand;
            }
            _ => {}
        }

        // Check if element supports AXPress action (clickable but role not in list).
        if element_supports_press(current) {
            cleanup_refs(&retained_refs);
            return CursorKind::Hand;
        }

        // Walk up to parent (except on last iteration).
        if depth < 3 {
            if let Some(parent) = element_parent(current) {
                retained_refs.push(parent);
                current = parent;
            } else {
                break;
            }
        }
    }

    cleanup_refs(&retained_refs);
    CursorKind::Arrow
}

/// Clean up retained parent refs.
unsafe fn cleanup_refs(refs: &[AXUIElementRef]) {
    for parent_ref in refs {
        CFRelease(*parent_ref as CFTypeRef);
    }
}

/// Read the AXRole from an element, returning it as an Option<String>.
unsafe fn read_role(element: AXUIElementRef) -> Option<String> {
    let role_attr = CFStringCreateWithCString(
        std::ptr::null(),
        b"AXRole\0".as_ptr() as *const std::ffi::c_char,
        K_CFStringEncoding_UTF8,
    );
    if role_attr.is_null() {
        return None;
    }

    let mut role_value: CFTypeRef = std::ptr::null();
    let result = AXUIElementCopyAttributeValue(element, role_attr, &mut role_value);
    CFRelease(role_attr);

    if result != K_AX_ERROR_SUCCESS || role_value.is_null() {
        return None;
    }

    if CFGetTypeID(role_value) != CFStringGetTypeID() {
        CFRelease(role_value);
        return None;
    }

    let role_str = cfstring_to_rust_string(role_value);
    CFRelease(role_value);
    role_str
}

/// Check if an AXUIElement supports AXPress action.
unsafe fn element_supports_press(element: AXUIElementRef) -> bool {
    let mut names: CFTypeRef = std::ptr::null();
    let result = AXUIElementCopyActionNames(element, &mut names);
    if result != K_AX_ERROR_SUCCESS || names.is_null() {
        return false;
    }

    // names is a CFArray of CFStrings.
    let count = CFArrayGetCount(names as *const std::ffi::c_void);
    for i in 0..count {
        let item = CFArrayGetValueAtIndex(names as *const std::ffi::c_void, i);
        if !item.is_null() {
            if let Some(s) = cfstring_to_rust_string(item) {
                if s == "AXPress" {
                    CFRelease(names);
                    return true;
                }
            }
        }
    }
    CFRelease(names);
    false
}

/// Get the parent AXUIElement of an element.
unsafe fn element_parent(element: AXUIElementRef) -> Option<AXUIElementRef> {
    let parent_attr = CFStringCreateWithCString(
        std::ptr::null(),
        b"AXParent\0".as_ptr() as *const std::ffi::c_char,
        K_CFStringEncoding_UTF8,
    );
    if parent_attr.is_null() {
        return None;
    }

    let mut parent_value: CFTypeRef = std::ptr::null();
    let result = AXUIElementCopyAttributeValue(element, parent_attr, &mut parent_value);
    CFRelease(parent_attr);

    if result != K_AX_ERROR_SUCCESS || parent_value.is_null() {
        return None;
    }

    Some(parent_value as AXUIElementRef)
}

/// Convert a `CFString` to a Rust `String`.
unsafe fn cfstring_to_rust_string(cf_str: CFTypeRef) -> Option<String> {
    let length = CFStringGetLength(cf_str);
    if length == 0 {
        return Some(String::new());
    }

    // Allocate buffer for UTF-8 (max 4 bytes per code point + null terminator).
    let buf_size = (length * 4 + 1) as usize;
    let mut buf = vec![0u8; buf_size];

    if CFStringGetCString(
        cf_str,
        buf.as_mut_ptr() as *mut std::ffi::c_char,
        buf_size as i64,
        K_CFStringEncoding_UTF8,
    ) {
        if let Some(null_pos) = buf.iter().position(|&b| b == 0) {
            return String::from_utf8(buf[..null_pos].to_vec()).ok();
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock provider for testing.
    struct MockCursorKindProvider {
        kind: CursorKind,
    }

    impl CursorKindProvider for MockCursorKindProvider {
        fn query(&mut self, _global_x: f32, _global_y: f32) -> CursorKind {
            self.kind
        }
    }

    #[test]
    fn mock_provider_returns_configured_kind() {
        let mut provider = MockCursorKindProvider {
            kind: CursorKind::Hand,
        };
        assert_eq!(provider.query(100.0, 200.0), CursorKind::Hand);
    }

    #[test]
    fn mock_provider_returns_ibeam() {
        let mut provider = MockCursorKindProvider {
            kind: CursorKind::IBeam,
        };
        assert_eq!(provider.query(50.0, 50.0), CursorKind::IBeam);
    }

    #[test]
    fn diagnostics_snapshot_captures_counts() {
        // Reset counters for test isolation.
        KIND_ARROW_COUNT.store(0, Ordering::Relaxed);
        KIND_HAND_COUNT.store(0, Ordering::Relaxed);
        KIND_IBEAM_COUNT.store(0, Ordering::Relaxed);

        // Simulate some queries.
        KIND_ARROW_COUNT.fetch_add(5, Ordering::Relaxed);
        KIND_HAND_COUNT.fetch_add(3, Ordering::Relaxed);

        let diag = cursor_kind_diagnostics_snapshot();
        assert_eq!(diag.arrow_count, 5);
        assert_eq!(diag.hand_count, 3);
        assert_eq!(diag.ibeam_count, 0);

        // Clean up.
        KIND_ARROW_COUNT.store(0, Ordering::Relaxed);
        KIND_HAND_COUNT.store(0, Ordering::Relaxed);
    }
}
