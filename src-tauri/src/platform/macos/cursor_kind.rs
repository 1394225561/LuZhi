//! macOS CursorKind provider using AppKit's current system cursor.
//!
//! Queries `NSCursor.currentSystemCursor` to determine the cursor macOS is
//! actually showing. This is more reliable than inferring cursor kind from
//! Accessibility roles: WebView content may be opaque to AX hit-testing, and
//! system UI such as Mission Control can expose clickable AX elements while the
//! visible cursor remains the standard arrow.
//!
//! # Safety
//!
//! AppKit calls may create autoreleased Objective-C objects and are dispatched
//! onto the application main thread. The production reader wraps each cursor
//! shape read in an autorelease pool.
//!
//! This module is polled from `CursorMetadataRuntime`, NOT from
//! ScreenCaptureKit callbacks — it must not block the capture main loop.
//!
//! # Diagnostics
//!
//! `AtomicU64` counters track legacy AX query failures, fallback-to-Arrow
//! counts, and per-kind distribution. The kind distribution is updated by the
//! NSCursor-backed provider; AX counters remain for diagnostic compatibility.
//!
//! # Legacy AX Reference
//!
//! The AX hit-test helpers are retained for diagnostics/reference, but the
//! production provider no longer lets AX role inference emit cursor kind.

use crate::core::timeline::CursorKind;
use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
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

type CFTypeRef = *const c_void;
type AXUIElementRef = *const c_void;
type CFStringRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout_in_seconds: f32) -> i32;
    fn AXUIElementCopyElementAtPosition(
        application: AXUIElementRef,
        x: f64,
        y: f64,
        element: *mut AXUIElementRef,
    ) -> i32;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut CFTypeRef) -> i32;
}

type ObjcId = *mut c_void;
type Sel = *const c_void;
type ObjcMethod = *const c_void;

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> ObjcId;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn class_getClassMethod(cls: ObjcId, name: Sel) -> ObjcMethod;
    fn class_getInstanceMethod(cls: ObjcId, name: Sel) -> ObjcMethod;
    fn object_getClass(obj: ObjcId) -> ObjcId;
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(context: *mut c_void);
    // objc_msgSend is variadic — we transmute to typed fn pointers at call sites.
    fn objc_msgSend();
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        c_str: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFGetTypeID(cf: CFTypeRef) -> u64;
    fn CFStringGetTypeID() -> u64;
    fn CFStringGetLength(the_string: CFStringRef) -> i64;
    fn CFStringGetCString(
        the_string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: i64,
        encoding: u32,
    ) -> bool;
    fn CFArrayGetCount(the_array: *const c_void) -> i64;
    fn CFArrayGetValueAtIndex(the_array: *const c_void, idx: i64) -> *const c_void;
}

const K_CFSTRING_ENCODING_UTF8: u32 = 0x08000100;
const K_AX_ERROR_SUCCESS: i32 = 0;

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Diagnostics for cursor kind queries.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// Merge global AX diagnostic counters with recorder-local kind counts.
/// Call this at recording stop to get a complete picture.
///
/// The global `ax_query_failure_count` and `ax_fallback_arrow_count` come from
/// the `query_cursor_kind()` function's atomic counters, while `arrow_count`,
/// `hand_count`, and `ibeam_count` come from the `CursorMetadataRecorder`'s
/// own per-sample tracking.
pub fn cursor_kind_diagnostics_merged(
    recorder_arrow: u64,
    recorder_hand: u64,
    recorder_ibeam: u64,
) -> CursorKindDiagnostics {
    CursorKindDiagnostics {
        ax_query_failure_count: AX_QUERY_FAILURE_COUNT.load(Ordering::Relaxed),
        ax_fallback_arrow_count: AX_FALLBACK_ARROW_COUNT.load(Ordering::Relaxed),
        arrow_count: recorder_arrow,
        hand_count: recorder_hand,
        ibeam_count: recorder_ibeam,
    }
}

/// Reset all global diagnostic counters. Only for test isolation.
///
/// Tests that manipulate global counters MUST call this before and after
/// to avoid cross-test contamination.
///
/// # Safety
///
/// This function is NOT thread-safe — it must only be called from single-threaded
/// test contexts. The atomics themselves are safe, but concurrent test execution
/// could cause cross-test contamination.
pub fn reset_global_counters() {
    AX_QUERY_FAILURE_COUNT.store(0, Ordering::Relaxed);
    AX_FALLBACK_ARROW_COUNT.store(0, Ordering::Relaxed);
    KIND_ARROW_COUNT.store(0, Ordering::Relaxed);
    KIND_HAND_COUNT.store(0, Ordering::Relaxed);
    KIND_IBEAM_COUNT.store(0, Ordering::Relaxed);
}

/// Add to the global AX query failure counter. Only for test setup.
pub fn test_add_ax_query_failures(count: u64) {
    AX_QUERY_FAILURE_COUNT.fetch_add(count, Ordering::Relaxed);
}

/// Add to the global AX fallback arrow counter. Only for test setup.
pub fn test_add_ax_fallback_arrows(count: u64) {
    AX_FALLBACK_ARROW_COUNT.fetch_add(count, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// CursorKindProvider trait
// ---------------------------------------------------------------------------

/// Trait abstracting cursor kind queries for testability.
pub trait CursorKindProvider: Send + 'static {
    fn query(&mut self, global_x: f32, global_y: f32) -> CursorKind;
}

// CursorMainThreadDispatcher is now defined in app::recording_service_boundary
// and re-exported here for backward compatibility within the macos module.
pub use crate::app::recording_service_boundary::CursorMainThreadDispatcher;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemCursorShape {
    Arrow,
    PointingHand,
    IBeam,
    Other,
}

trait SystemCursorReader: Send + 'static {
    fn read_current_cursor_shape(&mut self) -> Option<SystemCursorShape>;
}

fn system_cursor_shape_to_kind(shape: SystemCursorShape) -> Option<CursorKind> {
    match shape {
        SystemCursorShape::Arrow => Some(CursorKind::Arrow),
        SystemCursorShape::PointingHand => Some(CursorKind::Hand),
        SystemCursorShape::IBeam => Some(CursorKind::IBeam),
        SystemCursorShape::Other => None,
    }
}

/// Production macOS cursor kind provider using AppKit's current system cursor.
///
/// Uses `NSCursor.currentSystemCursor` so the exported overlay follows the
/// cursor shape macOS is actually displaying.
/// Rate-limited to 10Hz (100ms cache TTL).
pub struct MacCursorKindProvider {
    kind_cache_ttl: Duration,
    cached_kind: CursorKind,
    last_query_at: Instant,
    system_cursor_reader: Box<dyn SystemCursorReader>,
}

impl MacCursorKindProvider {
    pub fn new(main_thread_dispatcher: Box<dyn CursorMainThreadDispatcher>) -> Self {
        Self {
            kind_cache_ttl: Duration::from_millis(100),
            cached_kind: CursorKind::Arrow,
            last_query_at: Instant::now() - Duration::from_secs(1),
            system_cursor_reader: Box::new(MainThreadSystemCursorReader::new(
                main_thread_dispatcher,
            )),
        }
    }

    #[cfg(test)]
    fn with_system_reader_for_test(system_cursor_reader: Box<dyn SystemCursorReader>) -> Self {
        Self {
            kind_cache_ttl: Duration::from_millis(100),
            cached_kind: CursorKind::Arrow,
            last_query_at: Instant::now() - Duration::from_secs(1),
            system_cursor_reader,
        }
    }
}

impl CursorKindProvider for MacCursorKindProvider {
    fn query(&mut self, _global_x: f32, _global_y: f32) -> CursorKind {
        let now = Instant::now();
        if now.duration_since(self.last_query_at) < self.kind_cache_ttl {
            return self.cached_kind;
        }

        let kind = self
            .system_cursor_reader
            .read_current_cursor_shape()
            .and_then(system_cursor_shape_to_kind)
            .unwrap_or(CursorKind::Arrow);
        record_kind_count(kind);
        self.cached_kind = kind;
        self.last_query_at = now;
        self.cached_kind
    }
}

fn record_kind_count(kind: CursorKind) {
    match kind {
        CursorKind::Arrow => {
            KIND_ARROW_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        CursorKind::Hand => {
            KIND_HAND_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        CursorKind::IBeam => {
            KIND_IBEAM_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }
}

type SystemCursorReadFn = Arc<dyn Fn() -> Option<SystemCursorShape> + Send + Sync>;

const MAIN_THREAD_CURSOR_READ_TIMEOUT: Duration = Duration::from_millis(50);

struct MainThreadSystemCursorReader {
    dispatcher: Box<dyn CursorMainThreadDispatcher>,
    read_current_shape: SystemCursorReadFn,
    read_timeout: Duration,
}

impl MainThreadSystemCursorReader {
    fn new(dispatcher: Box<dyn CursorMainThreadDispatcher>) -> Self {
        Self {
            dispatcher,
            read_current_shape: Arc::new(|| unsafe { read_current_system_cursor_shape() }),
            read_timeout: MAIN_THREAD_CURSOR_READ_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_read_fn_for_test(
        dispatcher: Box<dyn CursorMainThreadDispatcher>,
        read_current_shape: SystemCursorReadFn,
    ) -> Self {
        Self {
            dispatcher,
            read_current_shape,
            read_timeout: MAIN_THREAD_CURSOR_READ_TIMEOUT,
        }
    }
}

impl SystemCursorReader for MainThreadSystemCursorReader {
    fn read_current_cursor_shape(&mut self) -> Option<SystemCursorShape> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let read_current_shape = self.read_current_shape.clone();
        let dispatch_result = self.dispatcher.run_on_main_thread(Box::new(move || {
            let _ = sender.send(read_current_shape());
        }));
        if dispatch_result.is_err() {
            return None;
        }

        receiver.recv_timeout(self.read_timeout).ok().flatten()
    }
}

struct AutoreleasePool {
    context: *mut c_void,
}

impl AutoreleasePool {
    unsafe fn new() -> Self {
        Self {
            context: objc_autoreleasePoolPush(),
        }
    }
}

impl Drop for AutoreleasePool {
    fn drop(&mut self) {
        unsafe {
            objc_autoreleasePoolPop(self.context);
        }
    }
}

unsafe fn read_current_system_cursor_shape() -> Option<SystemCursorShape> {
    let _pool = AutoreleasePool::new();

    let cursor_class = objc_class(c"NSCursor")?;

    let current = objc_send_class_id(cursor_class, c"currentSystemCursor")?;

    if ns_cursor_matches_class_cursor(current, cursor_class, c"arrowCursor") {
        return Some(SystemCursorShape::Arrow);
    }
    if ns_cursor_matches_class_cursor(current, cursor_class, c"pointingHandCursor") {
        return Some(SystemCursorShape::PointingHand);
    }
    if ns_cursor_matches_class_cursor(current, cursor_class, c"IBeamCursor") {
        return Some(SystemCursorShape::IBeam);
    }

    Some(SystemCursorShape::Other)
}

unsafe fn ns_cursor_matches_class_cursor(
    current: ObjcId,
    cursor_class: ObjcId,
    selector_name: &'static CStr,
) -> bool {
    let Some(expected) = objc_send_class_id(cursor_class, selector_name) else {
        return false;
    };

    current == expected
        || objc_send_bool_id(current, c"isEqual:", expected).unwrap_or(false)
        || ns_cursor_image_and_hotspot_match(current, expected)
}

unsafe fn ns_cursor_image_and_hotspot_match(current: ObjcId, expected: ObjcId) -> bool {
    if !ns_cursor_hotspot_matches(current, expected) {
        return false;
    }

    let Some(current_image) = objc_send_id(current, c"image") else {
        return false;
    };
    let Some(expected_image) = objc_send_id(expected, c"image") else {
        return false;
    };

    let Some(current_data) = objc_send_id(current_image, c"TIFFRepresentation") else {
        return false;
    };
    let Some(expected_data) = objc_send_id(expected_image, c"TIFFRepresentation") else {
        return false;
    };

    objc_send_bool_id(current_data, c"isEqualToData:", expected_data).unwrap_or(false)
}

unsafe fn ns_cursor_hotspot_matches(current: ObjcId, expected: ObjcId) -> bool {
    let Some(current_hotspot) = objc_send_ns_point(current, c"hotSpot") else {
        return false;
    };
    let Some(expected_hotspot) = objc_send_ns_point(expected, c"hotSpot") else {
        return false;
    };
    (current_hotspot.x - expected_hotspot.x).abs() < 0.01
        && (current_hotspot.y - expected_hotspot.y).abs() < 0.01
}

unsafe fn objc_class(class_name: &'static CStr) -> Option<ObjcId> {
    let class = objc_getClass(class_name.as_ptr());
    if class.is_null() {
        None
    } else {
        Some(class)
    }
}

unsafe fn register_selector(selector_name: &'static CStr) -> Option<Sel> {
    let sel = sel_registerName(selector_name.as_ptr());
    if sel.is_null() {
        None
    } else {
        Some(sel)
    }
}

unsafe fn class_method_selector(class: ObjcId, selector_name: &'static CStr) -> Option<Sel> {
    let sel = register_selector(selector_name)?;
    if class.is_null() || class_getClassMethod(class, sel).is_null() {
        return None;
    }
    Some(sel)
}

unsafe fn instance_method_selector(receiver: ObjcId, selector_name: &'static CStr) -> Option<Sel> {
    if receiver.is_null() {
        return None;
    }
    let sel = register_selector(selector_name)?;
    let class = object_getClass(receiver);
    if class.is_null() || class_getInstanceMethod(class, sel).is_null() {
        return None;
    }
    Some(sel)
}

unsafe fn objc_send_class_id(receiver: ObjcId, selector_name: &'static CStr) -> Option<ObjcId> {
    let sel = class_method_selector(receiver, selector_name)?;
    type MsgSendId = extern "C" fn(ObjcId, Sel) -> ObjcId;
    let msg_send_id: MsgSendId = std::mem::transmute(objc_msgSend as *const ());
    let result = msg_send_id(receiver, sel);
    if result.is_null() {
        None
    } else {
        Some(result)
    }
}

unsafe fn objc_send_id(receiver: ObjcId, selector_name: &'static CStr) -> Option<ObjcId> {
    let sel = instance_method_selector(receiver, selector_name)?;
    type MsgSendId = extern "C" fn(ObjcId, Sel) -> ObjcId;
    let msg_send_id: MsgSendId = std::mem::transmute(objc_msgSend as *const ());
    let result = msg_send_id(receiver, sel);
    if result.is_null() {
        None
    } else {
        Some(result)
    }
}

unsafe fn objc_send_bool_id(
    receiver: ObjcId,
    selector_name: &'static CStr,
    arg: ObjcId,
) -> Option<bool> {
    let sel = instance_method_selector(receiver, selector_name)?;
    type MsgSendBoolId = extern "C" fn(ObjcId, Sel, ObjcId) -> i8;
    let msg_send_bool_id: MsgSendBoolId = std::mem::transmute(objc_msgSend as *const ());
    Some(msg_send_bool_id(receiver, sel, arg) != 0)
}

unsafe fn objc_send_ns_point(receiver: ObjcId, selector_name: &'static CStr) -> Option<NSPoint> {
    let sel = instance_method_selector(receiver, selector_name)?;
    type MsgSendPoint = extern "C" fn(ObjcId, Sel) -> NSPoint;
    let msg_send_point: MsgSendPoint = std::mem::transmute(objc_msgSend as *const ());
    Some(msg_send_point(receiver, sel))
}

unsafe fn objc_send_i32(receiver: ObjcId, selector_name: &'static CStr) -> Option<i32> {
    let sel = instance_method_selector(receiver, selector_name)?;
    type MsgSendI32 = extern "C" fn(ObjcId, Sel) -> i32;
    let msg_send_i32: MsgSendI32 = std::mem::transmute(objc_msgSend as *const ());
    Some(msg_send_i32(receiver, sel))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Per-query classification log entry for debugging why kind was assigned.
#[derive(Debug, Clone)]
struct AxClassificationLog {
    result_code: i32,
    role_chain: Vec<String>,
    has_ax_press: bool,
    classified_kind: CursorKind,
    query_duration_nanos: u64,
    /// Whether this was a frontmost-app query or system-wide fallback.
    used_frontmost_app: bool,
}

/// Rate-limited sampling log: print at most once per second.
static LAST_LOG_TIME: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
const LOG_INTERVAL: Duration = Duration::from_secs(1);

fn maybe_log_classification(log: &AxClassificationLog, point: (f32, f32)) {
    let Ok(mut last) = LAST_LOG_TIME.lock() else {
        return;
    };
    let now = Instant::now();
    if let Some(last_time) = *last {
        if now.duration_since(last_time) < LOG_INTERVAL {
            return;
        }
    }
    *last = Some(now);
    let source = if log.used_frontmost_app {
        "front"
    } else {
        "system"
    };
    eprintln!(
        "[cursor-kind-classify] point=({:.0},{:.0}) kind={:?} role_chain={:?} ax_press={} rc={} dur={}µs src={}",
        point.0, point.1,
        log.classified_kind,
        log.role_chain,
        log.has_ax_press,
        log.result_code,
        log.query_duration_nanos / 1000,
        source,
    );
}

/// Get the PID of the frontmost (active) application.
///
/// Uses `[NSWorkspace sharedWorkspace].frontmostApplication.processIdentifier`.
/// Returns 0 on failure.
unsafe fn get_frontmost_app_pid() -> i32 {
    let Some(workspace_class) = objc_class(c"NSWorkspace") else {
        return 0;
    };
    let Some(workspace) = objc_send_class_id(workspace_class, c"sharedWorkspace") else {
        return 0;
    };
    let Some(app) = objc_send_id(workspace, c"frontmostApplication") else {
        return 0;
    };
    objc_send_i32(app, c"processIdentifier").unwrap_or(0)
}

/// Query the cursor kind at the given position.
///
/// **Strategy**: Try the frontmost application's AX element first (better for
/// app-specific content like Tauri/WebView), then fall back to system-wide.
///
/// **Coordinate space**: The caller is responsible for providing coordinates
/// in the AX coordinate space (Y already flipped if needed).
pub fn query_cursor_kind(global_x: f32, ax_y: f64) -> CursorKind {
    let query_start = Instant::now();
    unsafe {
        // Try frontmost app first — better for Tauri/WebView content.
        let frontmost_pid = get_frontmost_app_pid();
        if frontmost_pid > 0 {
            let app_element = AXUIElementCreateApplication(frontmost_pid);
            if !app_element.is_null() {
                AXUIElementSetMessagingTimeout(app_element, 0.1);

                let mut element: AXUIElementRef = std::ptr::null();
                let result = AXUIElementCopyElementAtPosition(
                    app_element,
                    global_x as f64,
                    ax_y,
                    &mut element,
                );
                CFRelease(app_element);

                if result == K_AX_ERROR_SUCCESS && !element.is_null() {
                    let (kind, role_chain, has_ax_press) = element_role_to_cursor_kind(element);
                    CFRelease(element);

                    let query_duration = query_start.elapsed().as_nanos() as u64;
                    maybe_log_classification(
                        &AxClassificationLog {
                            result_code: result,
                            role_chain,
                            has_ax_press,
                            classified_kind: kind,
                            query_duration_nanos: query_duration,
                            used_frontmost_app: true,
                        },
                        (global_x, ax_y as f32),
                    );

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
                    return kind;
                }
            }
        }

        // Fallback: system-wide element.
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            AX_QUERY_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return CursorKind::Arrow;
        }

        AXUIElementSetMessagingTimeout(system, 0.1);

        let mut element: AXUIElementRef = std::ptr::null();
        let result = AXUIElementCopyElementAtPosition(system, global_x as f64, ax_y, &mut element);
        CFRelease(system);

        if result != K_AX_ERROR_SUCCESS || element.is_null() {
            AX_QUERY_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return CursorKind::Arrow;
        }

        let (kind, role_chain, has_ax_press) = element_role_to_cursor_kind(element);
        CFRelease(element);

        let query_duration = query_start.elapsed().as_nanos() as u64;
        maybe_log_classification(
            &AxClassificationLog {
                result_code: result,
                role_chain,
                has_ax_press,
                classified_kind: kind,
                query_duration_nanos: query_duration,
                used_frontmost_app: false,
            },
            (global_x, ax_y as f32),
        );

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
/// Returns `(kind, role_chain, has_ax_press)` for classification auditing.
///
/// Classification strategy:
/// - IBeam: AXTextField, AXTextArea, or editable text
/// - Hand: AXButton, AXLink, AXMenuItem, or element/parent supports AXPress
/// - Arrow: everything else
///
/// For elements that hit child text/group/image (common in browsers, Electron, Tauri),
/// we walk up to 3 parent levels looking for a clickable or editable container.
unsafe fn element_role_to_cursor_kind(element: AXUIElementRef) -> (CursorKind, Vec<String>, bool) {
    let mut current = element;
    let mut retained_refs: Vec<AXUIElementRef> = Vec::new();
    let mut role_chain: Vec<String> = Vec::new();
    let mut has_ax_press = false;

    for depth in 0..4 {
        // Max 4 levels: element + 3 parents.
        let role = read_role(current);

        // Record role chain for auditing.
        if let Some(ref r) = role {
            role_chain.push(r.clone());
        } else {
            role_chain.push(format!("(unknown@depth{depth})"));
        }

        match role.as_deref() {
            // IBeam: text input elements (not plain static text).
            Some("AXTextField") | Some("AXTextArea") => {
                cleanup_refs(&retained_refs);
                return (CursorKind::IBeam, role_chain, has_ax_press);
            }
            // Hand: clickable elements.
            Some("AXButton")
            | Some("AXLink")
            | Some("AXMenuItem")
            | Some("AXMenuBarItem")
            | Some("AXCheckBox")
            | Some("AXRadioButton")
            | Some("AXPopUpButton")
            | Some("AXComboBox")
            | Some("AXTab")
            | Some("AXDisclosureTriangle")
            | Some("AXSlider") => {
                cleanup_refs(&retained_refs);
                return (CursorKind::Hand, role_chain, has_ax_press);
            }
            // Hand: interactive containers — list items, table rows, rows.
            // These are clickable in many apps (Finder, browsers, Tauri apps).
            Some("AXList") | Some("AXRow") | Some("AXTable") => {
                cleanup_refs(&retained_refs);
                return (CursorKind::Hand, role_chain, has_ax_press);
            }
            _ => {}
        }

        // Check if element supports AXPress action (clickable but role not in list).
        if element_supports_press(current) {
            has_ax_press = true;
            cleanup_refs(&retained_refs);
            return (CursorKind::Hand, role_chain, has_ax_press);
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
    (CursorKind::Arrow, role_chain, has_ax_press)
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
        c"AXRole".as_ptr(),
        K_CFSTRING_ENCODING_UTF8,
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
    let count = CFArrayGetCount(names);
    for i in 0..count {
        let item = CFArrayGetValueAtIndex(names, i);
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
        c"AXParent".as_ptr(),
        K_CFSTRING_ENCODING_UTF8,
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
        buf.as_mut_ptr() as *mut c_char,
        buf_size as i64,
        K_CFSTRING_ENCODING_UTF8,
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
    use std::sync::atomic::AtomicBool;

    struct FixedSystemCursorReader {
        shape: Option<SystemCursorShape>,
    }

    impl SystemCursorReader for FixedSystemCursorReader {
        fn read_current_cursor_shape(&mut self) -> Option<SystemCursorShape> {
            self.shape
        }
    }

    struct InlineMainThreadDispatcher {
        did_run: std::sync::Arc<AtomicBool>,
    }

    impl CursorMainThreadDispatcher for InlineMainThreadDispatcher {
        fn run_on_main_thread(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            self.did_run.store(true, Ordering::Relaxed);
            task();
            Ok(())
        }
    }

    struct FailingMainThreadDispatcher;

    impl CursorMainThreadDispatcher for FailingMainThreadDispatcher {
        fn run_on_main_thread(&self, _task: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            Err("main thread unavailable".to_string())
        }
    }

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
    fn system_cursor_shape_mapping_covers_supported_kinds() {
        assert_eq!(
            system_cursor_shape_to_kind(SystemCursorShape::Arrow),
            Some(CursorKind::Arrow)
        );
        assert_eq!(
            system_cursor_shape_to_kind(SystemCursorShape::PointingHand),
            Some(CursorKind::Hand)
        );
        assert_eq!(
            system_cursor_shape_to_kind(SystemCursorShape::IBeam),
            Some(CursorKind::IBeam)
        );
    }

    #[test]
    fn system_cursor_unknown_does_not_become_hand() {
        assert_eq!(system_cursor_shape_to_kind(SystemCursorShape::Other), None);

        let mut provider =
            MacCursorKindProvider::with_system_reader_for_test(Box::new(FixedSystemCursorReader {
                shape: Some(SystemCursorShape::Other),
            }));

        assert_eq!(provider.query(320.0, 240.0), CursorKind::Arrow);
    }

    #[test]
    fn system_cursor_arrow_wins_for_mission_control_false_positive_case() {
        let mut provider =
            MacCursorKindProvider::with_system_reader_for_test(Box::new(FixedSystemCursorReader {
                shape: Some(SystemCursorShape::Arrow),
            }));

        assert_eq!(provider.query(960.0, 540.0), CursorKind::Arrow);
    }

    #[test]
    fn system_cursor_reader_can_emit_hand_and_ibeam() {
        let mut provider =
            MacCursorKindProvider::with_system_reader_for_test(Box::new(FixedSystemCursorReader {
                shape: Some(SystemCursorShape::PointingHand),
            }));
        assert_eq!(provider.query(960.0, 540.0), CursorKind::Hand);

        let mut provider =
            MacCursorKindProvider::with_system_reader_for_test(Box::new(FixedSystemCursorReader {
                shape: Some(SystemCursorShape::IBeam),
            }));
        assert_eq!(provider.query(960.0, 540.0), CursorKind::IBeam);
    }

    #[test]
    fn main_thread_reader_dispatches_system_cursor_read() {
        let did_run = std::sync::Arc::new(AtomicBool::new(false));
        let read_called = std::sync::Arc::new(AtomicBool::new(false));
        let read_called_for_closure = read_called.clone();
        let mut reader = MainThreadSystemCursorReader::with_read_fn_for_test(
            Box::new(InlineMainThreadDispatcher {
                did_run: did_run.clone(),
            }),
            std::sync::Arc::new(move || {
                read_called_for_closure.store(true, Ordering::Relaxed);
                Some(SystemCursorShape::IBeam)
            }),
        );

        assert_eq!(
            reader.read_current_cursor_shape(),
            Some(SystemCursorShape::IBeam)
        );
        assert!(did_run.load(Ordering::Relaxed));
        assert!(read_called.load(Ordering::Relaxed));
    }

    #[test]
    fn main_thread_reader_returns_none_when_dispatch_fails() {
        let read_called = std::sync::Arc::new(AtomicBool::new(false));
        let read_called_for_closure = read_called.clone();
        let mut reader = MainThreadSystemCursorReader::with_read_fn_for_test(
            Box::new(FailingMainThreadDispatcher),
            std::sync::Arc::new(move || {
                read_called_for_closure.store(true, Ordering::Relaxed);
                Some(SystemCursorShape::PointingHand)
            }),
        );

        assert_eq!(reader.read_current_cursor_shape(), None);
        assert!(!read_called.load(Ordering::Relaxed));
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
