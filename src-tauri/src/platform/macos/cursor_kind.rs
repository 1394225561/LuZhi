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
//! Two `AtomicU64` counters track query failures and fallback-to-Arrow counts.
//! These are exposed via `ax_query_failure_count()` and `ax_fallback_arrow_count()`
//! for diagnostic logging.

use crate::core::timeline::CursorKind;
use std::sync::atomic::{AtomicU64, Ordering};

/// Diagnostic counter for accessibility query failures.
static AX_QUERY_FAILURE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Diagnostic counter for successful queries that returned Arrow (fallback).
static AX_FALLBACK_ARROW_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// FFI types and bindings
// ---------------------------------------------------------------------------

type CFTypeRef = *const std::ffi::c_void;
type AXUIElementRef = *const std::ffi::c_void;
type CFStringRef = *const std::ffi::c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
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
}

const K_CFStringEncoding_UTF8: u32 = 0x08000100;
const K_AX_ERROR_SUCCESS: i32 = 0;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Get the current accessibility query failure count (for diagnostics).
pub fn ax_query_failure_count() -> u64 {
    AX_QUERY_FAILURE_COUNT.load(Ordering::Relaxed)
}

/// Get the fallback arrow count (for diagnostics).
pub fn ax_fallback_arrow_count() -> u64 {
    AX_FALLBACK_ARROW_COUNT.load(Ordering::Relaxed)
}

/// Query the cursor kind at the given global screen position.
///
/// Uses `AXUIElementCopyElementAtPosition` to find the UI element under the
/// cursor, then reads its role to determine the cursor kind.
///
/// Falls back to `Arrow` on any failure (no accessibility permission, timeout,
/// unexpected role, etc.).
pub fn query_cursor_kind(global_x: f32, global_y: f32) -> CursorKind {
    unsafe {
        // pid=0 creates a system-wide accessibility element.
        let app_element = AXUIElementCreateApplication(0);
        if app_element.is_null() {
            AX_QUERY_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return CursorKind::Arrow;
        }

        let mut element: AXUIElementRef = std::ptr::null();
        let result = AXUIElementCopyElementAtPosition(
            app_element,
            global_x as f64,
            global_y as f64,
            &mut element,
        );
        CFRelease(app_element);

        if result != K_AX_ERROR_SUCCESS || element.is_null() {
            AX_QUERY_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return CursorKind::Arrow;
        }

        let kind = element_role_to_cursor_kind(element);
        CFRelease(element);

        if kind == CursorKind::Arrow {
            AX_FALLBACK_ARROW_COUNT.fetch_add(1, Ordering::Relaxed);
        }

        kind
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Map an `AXUIElement`'s role to a `CursorKind`.
unsafe fn element_role_to_cursor_kind(element: AXUIElementRef) -> CursorKind {
    let role_attr = CFStringCreateWithCString(
        std::ptr::null(),
        b"AXRole\0".as_ptr() as *const std::ffi::c_char,
        K_CFStringEncoding_UTF8,
    );
    if role_attr.is_null() {
        return CursorKind::Arrow;
    }

    let mut role_value: CFTypeRef = std::ptr::null();
    let result = AXUIElementCopyAttributeValue(element, role_attr, &mut role_value);
    CFRelease(role_attr);

    if result != K_AX_ERROR_SUCCESS || role_value.is_null() {
        return CursorKind::Arrow;
    }

    let kind = classify_by_role(role_value);
    CFRelease(role_value);
    kind
}

/// Classify a `CFString` role value into a `CursorKind`.
unsafe fn classify_by_role(role_value: CFTypeRef) -> CursorKind {
    // Verify it's a CFString.
    if CFGetTypeID(role_value) != CFStringGetTypeID() {
        return CursorKind::Arrow;
    }

    let role_str = cfstring_to_rust_string(role_value);

    match role_str.as_deref() {
        // IBeam: text input elements
        Some("AXTextField") | Some("AXTextArea") | Some("AXStaticText") => CursorKind::IBeam,
        // Hand: clickable elements
        Some("AXButton")
        | Some("AXLink")
        | Some("AXMenuItem")
        | Some("AXMenuBarItem")
        | Some("AXCheckBox")
        | Some("AXRadioButton")
        | Some("AXPopUpButton")
        | Some("AXComboBox") => CursorKind::Hand,
        // Everything else → Arrow
        _ => CursorKind::Arrow,
    }
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
