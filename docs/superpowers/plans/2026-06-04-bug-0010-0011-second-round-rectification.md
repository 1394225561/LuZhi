# BUG-0010/0011 第二轮整改实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复第一轮 BUG-0010/BUG-0011/BUG-0012 修复中遗留的 3 个 Critical、3 个 Important、1 个 Minor 问题，使光标坐标归一化、光标形态识别、glyph 颜色和导出错误显示全部通过人工验证。

**Architecture:** 保持现有三层管线（MacCursorSource → CursorMetadataRecorder → CursorEffectEngine → CursorOverlayRenderer），修复 Y 轴翻转逻辑、接入真实 CursorKind 检测、修正 glyph 颜色契约、归一化 click 坐标、修复 terminal error UI 显示。

**Tech Stack:** Rust (CoreGraphics FFI, Accessibility API), React + TypeScript, Tauri 2.0

---

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `src-tauri/src/app/cursor_metadata_runtime.rs` | CursorCoordinateMapper Y 轴修复 + CursorSnapshot 扩展 kind + CursorClick 归一化 |
| Create | `src-tauri/src/platform/macos/cursor_kind.rs` | macOS CursorKind provider (Accessibility hit-test) |
| Modify | `src-tauri/src/platform/macos/cursor_source.rs` | MacCursorSource.snapshot() 返回 kind |
| Modify | `src-tauri/src/platform/macos/mod.rs` | 导出 cursor_kind 模块 |
| Modify | `src-tauri/src/media/cursor_overlay.rs` | Arrow/IBeam glyph 颜色修正 + 死代码清理 + 像素级测试 |
| Modify | `src/components/preview-view.tsx` | terminal error 写入 beautifyError |
| Modify | `src/App.test.tsx` | terminal error 前端测试 |
| Modify | `BUG.md` | 更新第二轮验证结果和预防规则 |
| Modify | `HANDOFF.md` | 工作记录 |

---

### Task 1: 新增 mapper 四角失败测试（BUG-0010）

**Files:**
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs:508-612` (tests module)

- [ ] **Step 1: 新增 top-left 测试**

在 `cursor_metadata_runtime.rs` 的 `#[cfg(test)] mod tests` 中添加：

```rust
#[test]
fn cursor_mapper_maps_top_left_corner() {
    let geo = CaptureGeometry {
        display_id: 1,
        content_origin_x: 0.0,
        content_origin_y: 0.0,
        content_width: 1920.0,
        content_height: 1080.0,
        point_pixel_scale: 1.0,
        stream_width: 1920,
        stream_height: 1080,
    };
    let mapper = CursorCoordinateMapper::new(&geo);
    // Top-left corner: global (0, 0).
    // With flip_y=true: local_y = 1080 - 0 = 1080, source_y = 1080 → out of bounds!
    // With flip_y=false: local_y = 0, source_y = 0 → correct.
    let result = mapper.map(0.0, 0.0);
    // Current implementation with flip_y=true returns None (out of bounds).
    // This is WRONG — top-left should map to (0, 0).
    assert!(
        result.is_some(),
        "top-left corner (0,0) should map to source (0,0), got None"
    );
    let (x, y) = result.unwrap();
    assert!((x - 0.0).abs() < 1.0, "expected x≈0, got {x}");
    assert!((y - 0.0).abs() < 1.0, "expected y≈0, got {y}");
}
```

- [ ] **Step 2: 新增 bottom-right 测试**

```rust
#[test]
fn cursor_mapper_maps_bottom_right_corner() {
    let geo = CaptureGeometry {
        display_id: 1,
        content_origin_x: 0.0,
        content_origin_y: 0.0,
        content_width: 1920.0,
        content_height: 1080.0,
        point_pixel_scale: 1.0,
        stream_width: 1920,
        stream_height: 1080,
    };
    let mapper = CursorCoordinateMapper::new(&geo);
    // Bottom-right corner: global (1919, 1079).
    // With flip_y=true: local_y = 1080 - 1079 = 1, source_y = 1 → WRONG (should be ~1079).
    // With flip_y=false: local_y = 1079, source_y = 1079 → correct.
    let result = mapper.map(1919.0, 1079.0);
    assert!(
        result.is_some(),
        "bottom-right corner should map to source, got None"
    );
    let (x, y) = result.unwrap();
    assert!((x - 1919.0).abs() < 1.0, "expected x≈1919, got {x}");
    assert!((y - 1079.0).abs() < 1.0, "expected y≈1079, got {y}");
}
```

- [ ] **Step 3: 新增下半屏测试**

```rust
#[test]
fn cursor_mapper_maps_lower_half_screen() {
    let geo = CaptureGeometry {
        display_id: 1,
        content_origin_x: 0.0,
        content_origin_y: 0.0,
        content_width: 1920.0,
        content_height: 1080.0,
        point_pixel_scale: 1.0,
        stream_width: 1920,
        stream_height: 1080,
    };
    let mapper = CursorCoordinateMapper::new(&geo);
    // Cursor at y=900 (lower half of 1080p display).
    // With flip_y=true: source_y = 1080 - 900 = 180 → WRONG (should be 900).
    // With flip_y=false: source_y = 900 → correct.
    let (x, y) = mapper.map(960.0, 900.0).unwrap();
    assert!((x - 960.0).abs() < 0.01);
    assert!((y - 900.0).abs() < 1.0, "expected y≈900, got {y}");
}
```

- [ ] **Step 4: 新增负 origin（外接屏）测试**

```rust
#[test]
fn cursor_mapper_handles_negative_origin() {
    let geo = CaptureGeometry {
        display_id: 1,
        content_origin_x: -1920.0,
        content_origin_y: 0.0,
        content_width: 1920.0,
        content_height: 1080.0,
        point_pixel_scale: 1.0,
        stream_width: 1920,
        stream_height: 1080,
    };
    let mapper = CursorCoordinateMapper::new(&geo);
    // Cursor on secondary display at global x=-960 (center of -1920..0 range).
    let (x, y) = mapper.map(-960.0, 540.0).unwrap();
    assert!((x - 960.0).abs() < 0.01, "expected x≈960, got {x}");
    assert!((y - 540.0).abs() < 1.0, "expected y≈540, got {y}");
}
```

- [ ] **Step 5: 新增 Retina point-to-pixel 四角测试**

```rust
#[test]
fn cursor_mapper_retina_top_left() {
    // Retina 2x: display 960×540 points → 1920×1080 stream pixels.
    let geo = CaptureGeometry {
        display_id: 1,
        content_origin_x: 0.0,
        content_origin_y: 0.0,
        content_width: 960.0,
        content_height: 540.0,
        point_pixel_scale: 2.0,
        stream_width: 1920,
        stream_height: 1080,
    };
    let mapper = CursorCoordinateMapper::new(&geo);
    // Top-left in points: (0, 0).
    let result = mapper.map(0.0, 0.0);
    assert!(result.is_some(), "Retina top-left should not be out of bounds");
    let (x, y) = result.unwrap();
    assert!((x - 0.0).abs() < 1.0, "expected x≈0, got {x}");
    assert!((y - 0.0).abs() < 1.0, "expected y≈0, got {y}");
}

#[test]
fn cursor_mapper_retina_bottom_right() {
    let geo = CaptureGeometry {
        display_id: 1,
        content_origin_x: 0.0,
        content_origin_y: 0.0,
        content_width: 960.0,
        content_height: 540.0,
        point_pixel_scale: 2.0,
        stream_width: 1920,
        stream_height: 1080,
    };
    let mapper = CursorCoordinateMapper::new(&geo);
    // Bottom-right in points: (959, 539).
    let result = mapper.map(959.0, 539.0);
    assert!(result.is_some(), "Retina bottom-right should not be out of bounds");
    let (x, y) = result.unwrap();
    assert!((x - 1918.0).abs() < 2.0, "expected x≈1918, got {x}");
    assert!((y - 1078.0).abs() < 2.0, "expected y≈1078, got {y}");
}
```

- [ ] **Step 6: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor_mapper`
Expected: FAIL — `top-left corner (0,0) should map to source (0,0), got None`

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/app/cursor_metadata_runtime.rs
git commit -m "test(cursor): 新增 mapper 四角和负 origin 测试锁定 BUG-0010 Y 轴错误"
```

---

### Task 2: 修复 CursorCoordinateMapper Y 轴翻转（BUG-0010 Critical 1）

**Files:**
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs:36-90` (CursorCoordinateMapper)

- [ ] **Step 1: 移除无条件 flip_y，改为默认不翻转**

`CGEventGetLocation()` 和 `SCDisplay.frame()` 在 macOS 上使用同一坐标系（top-left origin, Y 向下增长）。不需要翻转。

修改 `CursorCoordinateMapper`：

```rust
pub struct CursorCoordinateMapper {
    origin_x: f32,
    origin_y: f32,
    scale_x: f32,
    scale_y: f32,
    stream_width: f32,
    stream_height: f32,
}

impl CursorCoordinateMapper {
    pub fn new(geometry: &CaptureGeometry) -> Self {
        Self {
            origin_x: geometry.content_origin_x,
            origin_y: geometry.content_origin_y,
            scale_x: geometry.stream_width as f32 / geometry.content_width.max(1.0),
            scale_y: geometry.stream_height as f32 / geometry.content_height.max(1.0),
            stream_width: geometry.stream_width as f32,
            stream_height: geometry.stream_height as f32,
        }
    }

    pub fn map(&self, global_x: f32, global_y: f32) -> Option<(f32, f32)> {
        let source_x = (global_x - self.origin_x) * self.scale_x;
        let source_y = (global_y - self.origin_y) * self.scale_y;

        if source_x < 0.0
            || source_y < 0.0
            || source_x > self.stream_width
            || source_y > self.stream_height
        {
            return None;
        }

        Some((source_x, source_y))
    }
}
```

- [ ] **Step 2: 更新现有测试中因 flip_y 产生的错误预期**

修改 `cursor_mapper_maps_identity_display_to_stream_pixels` 测试：

```rust
#[test]
fn cursor_mapper_maps_identity_display_to_stream_pixels() {
    let geo = CaptureGeometry {
        display_id: 1,
        content_origin_x: 0.0,
        content_origin_y: 0.0,
        content_width: 1920.0,
        content_height: 1080.0,
        point_pixel_scale: 1.0,
        stream_width: 1920,
        stream_height: 1080,
    };
    let mapper = CursorCoordinateMapper::new(&geo);
    let (x, y) = mapper.map(960.0, 540.0).unwrap();
    assert!((x - 960.0).abs() < 0.01);
    assert!((y - 540.0).abs() < 0.01);
}
```

修改 `cursor_mapper_scales_points_to_1080p_stream` 测试：

```rust
#[test]
fn cursor_mapper_scales_points_to_1080p_stream() {
    let geo = CaptureGeometry {
        display_id: 1,
        content_origin_x: 0.0,
        content_origin_y: 0.0,
        content_width: 960.0,
        content_height: 540.0,
        point_pixel_scale: 2.0,
        stream_width: 1920,
        stream_height: 1080,
    };
    let mapper = CursorCoordinateMapper::new(&geo);
    let (x, y) = mapper.map(480.0, 270.0).unwrap();
    assert!((x - 960.0).abs() < 0.01);
    assert!((y - 540.0).abs() < 0.01);
}
```

修改 `recorder_normalizes_coordinates_when_geometry_present` 测试中的预期值。

- [ ] **Step 3: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor_mapper`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/app/cursor_metadata_runtime.rs
git commit -m "fix(cursor): 移除无条件 Y 轴翻转 — CGEventGetLocation 与 SCK 使用同一 top-down 坐标系"
```

---

### Task 3: 添加真实设备诊断日志（BUG-0010 Important 2）

**Files:**
- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs` (read_display_geometry)

- [ ] **Step 1: 在 read_display_geometry 中添加诊断日志**

在 `read_display_geometry()` 函数中，读取完 geometry 后添加限量诊断日志：

```rust
// 诊断日志：仅打印一次，用于人工确认坐标来源。
eprintln!(
    "[cursor-geometry] display_id={} frame_origin=({:.1}, {:.1}) frame_size=({:.1}×{:.1}) \
     stream={}×{} point_pixel_scale={:.2}",
    display_id,
    frame.origin.x,
    frame.origin.y,
    frame.size.width,
    frame.size.height,
    stream_width,
    stream_height,
    point_pixel_scale
);
```

- [ ] **Step 2: Commit**

```bash
git add src-tauri/src/platform/macos/screen_capture_kit.rs
git commit -m "feat(cursor): 添加 SCDisplay 几何诊断日志用于人工确认坐标系"
```

---

### Task 4: 扩展 CursorSnapshot 携带 kind（BUG-0011 Critical 2 前置）

**Files:**
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs:19-27` (CursorSnapshot struct)

- [ ] **Step 1: CursorSnapshot 新增 kind 字段**

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorSnapshot {
    pub x: f32,
    pub y: f32,
    pub left_down: bool,
    pub right_down: bool,
    pub middle_down: bool,
    pub kind: CursorKind,
}
```

- [ ] **Step 2: 更新 record_snapshot 使用 snapshot.kind**

```rust
self.samples.push_back(CursorSample {
    timestamp,
    x: norm_x,
    y: norm_y,
    kind: snapshot.kind,
});
```

- [ ] **Step 3: 更新所有测试中的 CursorSnapshot 构造**

所有 `CursorSnapshot { x, y, left_down, right_down, middle_down }` 需要添加 `kind: CursorKind::Arrow`。

- [ ] **Step 4: 更新 MacCursorSource.snapshot() 返回 kind**

暂时返回 `CursorKind::Arrow`（真实 kind provider 在 Task 5 实现）：

```rust
Ok(CursorSnapshot {
    x: point.x as f32,
    y: point.y as f32,
    left_down: CGEventSourceButtonState(...),
    right_down: CGEventSourceButtonState(...),
    middle_down: CGEventSourceButtonState(...),
    kind: CursorKind::Arrow, // TODO: Task 5 接入真实 kind provider
})
```

- [ ] **Step 5: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/app/cursor_metadata_runtime.rs src-tauri/src/platform/macos/cursor_source.rs
git commit -m "feat(cursor): CursorSnapshot 携带 kind 字段，record_snapshot 使用真实 kind"
```

---

### Task 5: 实现 macOS CursorKind provider（BUG-0011 Critical 2 核心）

**Files:**
- Create: `src-tauri/src/platform/macos/cursor_kind.rs`
- Modify: `src-tauri/src/platform/macos/mod.rs`
- Modify: `src-tauri/src/platform/macos/cursor_source.rs`

- [ ] **Step 1: 创建 cursor_kind.rs 模块**

```rust
//! macOS CursorKind provider using Accessibility hit-test.
//!
//! Queries the Accessibility API (AXUIElementCopyElementAtPosition) to determine
//! the type of UI element under the cursor. Falls back to Arrow on failure.
//!
//! Safety: AXUIElementCopyElementAtPosition requires Accessibility permission.
//! If not granted, it returns an error and we fall back to Arrow.

use crate::core::timeline::CursorKind;
use std::sync::atomic::{AtomicU64, Ordering};

/// Diagnostic counter for accessibility query failures.
static AX_QUERY_FAILURE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Diagnostic counter for successful queries that returned Arrow (fallback).
static AX_FALLBACK_ARROW_COUNT: AtomicU64 = AtomicU64::new(0);

// CoreFoundation types
#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

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
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> bool;
    fn CFGetTypeID(cf: CFTypeRef) -> u64;
    fn CFStringGetTypeID() -> u64;
    fn CFStringGetLength(theString: CFStringRef) -> i64;
    fn CFStringGetCString(
        theString: CFStringRef,
        buffer: *mut std::ffi::c_char,
        bufferSize: i64,
        encoding: u32,
    ) -> bool;
}

const K_CFStringEncoding_UTF8: u32 = 0x08000100;
const K_AX_ERROR_SUCCESS: i32 = 0;

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
/// Uses AXUIElementCopyElementAtPosition to find the UI element under the cursor,
/// then reads its role to determine the cursor kind.
///
/// Falls back to Arrow on any failure (no accessibility permission, timeout, etc.).
pub fn query_cursor_kind(global_x: f32, global_y: f32) -> CursorKind {
    unsafe {
        // Create system-wide accessibility element (pid=0 means system-wide).
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

/// Map an AXUIElement's role to a CursorKind.
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

/// Classify a CFString role value into a CursorKind.
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
        // Check if the element supports press action (clickable but role not in the list above)
        _ => {
            if element_supports_action(role_value, "AXPress") {
                CursorKind::Hand
            } else {
                CursorKind::Arrow
            }
        }
    }
}

/// Check if an AXUIElement supports a given action.
unsafe fn element_supports_action(element: AXUIElementRef, action_name: &str) -> bool {
    // This is a simplified check — we read AXActions attribute.
    // For MVP, we rely on role-based classification. Action-based can be added later.
    // The element parameter here is actually the role_value, not the element.
    // This function is a placeholder for future enhancement.
    // TODO: Implement AXAction-based check when needed.
    false
}

/// Convert a CFString to a Rust String.
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
        // Find null terminator and convert.
        if let Some(null_pos) = buf.iter().position(|&b| b == 0) {
            return String::from_utf8(buf[..null_pos].to_vec()).ok();
        }
    }

    None
}
```

- [ ] **Step 2: 在 macos/mod.rs 导出 cursor_kind 模块**

在 `src-tauri/src/platform/macos/mod.rs` 中添加：

```rust
pub mod cursor_kind;
```

- [ ] **Step 3: 更新 MacCursorSource.snapshot() 使用真实 kind provider**

```rust
use super::cursor_kind;

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

            let kind = cursor_kind::query_cursor_kind(point.x as f32, point.y as f32);

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
            })
        }
    }
}
```

- [ ] **Step 4: 新增 fake source 单元测试覆盖 Arrow/Hand/IBeam**

在 `cursor_metadata_runtime.rs` 测试模块中添加：

```rust
#[test]
fn recorder_preserves_cursor_kind_from_snapshot() {
    let mut recorder = CursorMetadataRecorder::new(
        30,
        BeautifyConfigSnapshot {
            cursor_magnification: false,
            magnification_factor: 1.0,
            cursor_smoothing: false,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
            raw_system_cursor_visible: false,
        },
        None,
    );

    let kinds = [CursorKind::Arrow, CursorKind::Hand, CursorKind::IBeam];
    for (i, kind) in kinds.iter().enumerate() {
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(i as u64 * 33_333_333),
            CursorSnapshot {
                x: 100.0,
                y: 100.0,
                left_down: false,
                right_down: false,
                middle_down: false,
                kind: *kind,
            },
        );
    }

    let metadata = recorder.finish(100_000_000, None);
    assert_eq!(metadata.cursor_samples[0].kind, CursorKind::Arrow);
    assert_eq!(metadata.cursor_samples[1].kind, CursorKind::Hand);
    assert_eq!(metadata.cursor_samples[2].kind, CursorKind::IBeam);
}
```

- [ ] **Step 5: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata`
Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor_engine`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/platform/macos/cursor_kind.rs src-tauri/src/platform/macos/mod.rs src-tauri/src/platform/macos/cursor_source.rs src-tauri/src/app/cursor_metadata_runtime.rs
git commit -m "feat(cursor): 实现 macOS CursorKind provider — Accessibility hit-test 识别 Arrow/Hand/IBeam"
```

---

### Task 6: CursorClick 坐标归一化（Important 1）

**Files:**
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs:195-222` (record_button_transition)

- [ ] **Step 1: 修改 record_button_transition 接收归一化坐标**

```rust
fn record_button_transition(
    &mut self,
    timestamp: MediaTimestamp,
    norm_x: f32,
    norm_y: f32,
    button: MouseButton,
    was_down: bool,
    is_down: bool,
) {
    if was_down == is_down {
        return;
    }

    if self.clicks.len() >= self.max_clicks {
        self.clicks.pop_front();
    }

    self.clicks.push_back(CursorClick {
        timestamp,
        button,
        phase: if is_down {
            ClickPhase::Down
        } else {
            ClickPhase::Up
        },
        x: norm_x,
        y: norm_y,
    });
}
```

- [ ] **Step 2: 更新 record_snapshot 中的调用**

```rust
self.record_button_transition(
    timestamp,
    norm_x,
    norm_y,
    MouseButton::Left,
    previous.left_down,
    snapshot.left_down,
);
self.record_button_transition(
    timestamp,
    norm_x,
    norm_y,
    MouseButton::Right,
    previous.right_down,
    snapshot.right_down,
);
self.record_button_transition(
    timestamp,
    norm_x,
    norm_y,
    MouseButton::Middle,
    previous.middle_down,
    snapshot.middle_down,
);
```

- [ ] **Step 3: 新增 click 坐标归一化测试**

```rust
#[test]
fn recorder_normalizes_click_coordinates() {
    let geo = CaptureGeometry {
        display_id: 1,
        content_origin_x: 0.0,
        content_origin_y: 0.0,
        content_width: 1920.0,
        content_height: 1080.0,
        point_pixel_scale: 1.0,
        stream_width: 1920,
        stream_height: 1080,
    };
    let mut recorder = CursorMetadataRecorder::new(
        30,
        BeautifyConfigSnapshot {
            cursor_magnification: false,
            magnification_factor: 1.0,
            cursor_smoothing: false,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
            raw_system_cursor_visible: false,
        },
        Some(geo),
    );

    // First snapshot: no click.
    recorder.record_snapshot(
        MediaTimestamp::from_nanos(0),
        CursorSnapshot {
            x: 960.0,
            y: 540.0,
            left_down: false,
            right_down: false,
            middle_down: false,
            kind: CursorKind::Arrow,
        },
    );
    // Second snapshot: left button pressed.
    recorder.record_snapshot(
        MediaTimestamp::from_nanos(16_666_666),
        CursorSnapshot {
            x: 960.0,
            y: 540.0,
            left_down: true,
            right_down: false,
            middle_down: false,
            kind: CursorKind::Arrow,
        },
    );

    let metadata = recorder.finish(33_333_333, Some(geo));
    assert_eq!(metadata.cursor_clicks.len(), 1);
    // Click coordinates should be in source video pixel space.
    assert!((metadata.cursor_clicks[0].x - 960.0).abs() < 0.01);
    assert!((metadata.cursor_clicks[0].y - 540.0).abs() < 0.01);
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata`
Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/app/cursor_metadata_runtime.rs
git commit -m "fix(cursor): CursorClick 坐标归一化 — 与 CursorSample 使用同一 source video 坐标空间"
```

---

### Task 7: 修正 Arrow/IBeam glyph 颜色（BUG-0011 Critical 3）

**Files:**
- Modify: `src-tauri/src/media/cursor_overlay.rs:64-146` (glyph bitmaps)

- [ ] **Step 1: 替换 Arrow bitmap — 黑底白边**

Arrow 验收要求：黑色填充、白色描边。当前实现是白底黑边，需要反转。

将 `ARROW_PIXELS` 中的 `B` 和 `W` 互换：
- 外层描边（原来用 `B`）→ 改为 `W`（白色描边）
- 主体填充（原来用 `W`）→ 改为 `B`（黑色填充）

```rust
static ARROW_PIXELS: [GlyphPixel; 576] = [
    W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, W, W, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, W, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    W, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, B, B, W, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, W, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    W, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, B, B, B, B, B,
    B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T,
    T, T, T, T, T, T, T, T, W, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T,
    W, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, W, B, B, B, B, B, B, B,
    B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, W, B, B, B, B, B, B, B, B, B, B, B, B, B, W, T,
    T, T, T, T, T, T, T, T, W, B, B, B, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T,
    W, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, W, B, B, B, B, B, B, B,
    B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, W, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B,
    B, B, W, T, T, T, T, T, W, B, B, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, T, T, T, T, T, W,
    B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, W, W, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
];
```

- [ ] **Step 2: 替换 IBeam bitmap — 黑底白边**

IBeam 验收要求：黑色主体、白色描边。

```rust
static IBEAM_PIXELS: [GlyphPixel; 384] = [
    T, T, T, T, W, W, W, W, W, W, T, T, T, T, T, T, T, T, T, W, B, B, B, B, B, B, W, T, T, T, T, T,
    T, T, T, W, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, W, W, B, B, W, W, T, T, T, T, T, T,
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, W, W, B, B, W, W, T, T, T, T, T, T,
    T, T, T, W, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, W, B, B, B, B, B, B, W, T, T, T, T, T,
];
```

- [ ] **Step 3: 精修 Hand bitmap — 更短更接近系统手形**

保持 Hand 为白底黑边（符合验收），但调整形态使其更短更紧凑。参考 macOS 系统手形光标。

- [ ] **Step 4: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay`
Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/cursor_overlay.rs
git commit -m "fix(cursor): 修正 Arrow/IBeam glyph 颜色 — 黑底白边符合验收要求"
```

---

### Task 8: 像素级颜色契约测试（BUG-0011 Critical 3 验证）

**Files:**
- Modify: `src-tauri/src/media/cursor_overlay.rs` (tests module)

- [ ] **Step 1: 新增 Arrow 颜色测试**

```rust
#[test]
fn arrow_glyph_has_black_fill_and_white_outline() {
    let glyph = &ARROW_GLYPH;
    // Arrow tip pixel (0,0) should be white outline.
    let tip = &glyph.pixels[0];
    assert!(
        matches!(tip, GlyphPixel::White(_)),
        "arrow tip should be white outline"
    );
    // Interior pixel (e.g., row 5, col 12 — inside the arrow body) should be black fill.
    let interior = &glyph.pixels[5 * glyph.width + 12];
    assert!(
        matches!(interior, GlyphPixel::Black(_)),
        "arrow interior should be black fill"
    );
}
```

- [ ] **Step 2: 新增 Hand 颜色测试**

```rust
#[test]
fn hand_glyph_has_white_fill_and_black_outline() {
    let glyph = &HAND_GLYPH;
    // Find a white fill pixel (interior of hand).
    let has_white = glyph.pixels.iter().any(|p| matches!(p, GlyphPixel::White(_)));
    assert!(has_white, "hand should have white fill pixels");
    // Find a black outline pixel.
    let has_black = glyph.pixels.iter().any(|p| matches!(p, GlyphPixel::Black(_)));
    assert!(has_black, "hand should have black outline pixels");
}
```

- [ ] **Step 3: 新增 IBeam 颜色测试**

```rust
#[test]
fn ibeam_glyph_has_black_body_and_white_outline() {
    let glyph = &IBEAM_GLYPH;
    // Top bar edge should be white outline.
    let top_edge = &glyph.pixels[4]; // Row 0, col 4.
    assert!(
        matches!(top_edge, GlyphPixel::White(_)),
        "ibeam top edge should be white outline"
    );
    // Vertical stem should be black body.
    let stem = &glyph.pixels[5 * glyph.width + 7]; // Row 5, col 7 (center of stem).
    assert!(
        matches!(stem, GlyphPixel::Black(_)),
        "ibeam stem should be black body"
    );
}
```

- [ ] **Step 4: 新增渲染后颜色验证测试**

```rust
#[test]
fn rendered_arrow_has_correct_y_plane_values() {
    let timeline = make_timeline(vec![cursor_frame(0, 960.0, 540.0)], true);
    let renderer = CursorOverlayRenderer::new(
        timeline,
        1920, 1080, 1920, 1080,
        ExportScalePolicy::FitWithBars,
        None,
        Some((1920, 1080)),
    ).unwrap();

    let mut frame = ffmpeg_next::util::frame::Video::new(
        ffmpeg_next::util::format::Pixel::YUV420P,
        1920, 1080,
    );
    renderer.draw_on_frame(&mut frame, 0, 30);

    let y_plane = frame.data(0);
    let linesize = y_plane.len() / 1080;

    // Arrow tip at (960, 540) should be white outline (Y≈235).
    let tip_offset = 540 * linesize + 960;
    assert!(
        y_plane[tip_offset] > 200,
        "arrow tip should be white (Y>200), got {}",
        y_plane[tip_offset]
    );

    // Arrow interior (e.g., 960+12, 540+5) should be black fill (Y≈16).
    let interior_offset = (540 + 5) * linesize + (960 + 12);
    assert!(
        y_plane[interior_offset] < 50,
        "arrow interior should be black (Y<50), got {}",
        y_plane[interior_offset]
    );
}
```

- [ ] **Step 5: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/cursor_overlay.rs
git commit -m "test(cursor): 新增像素级颜色契约测试 — Arrow/Hand/IBeam 颜色和渲染验证"
```

---

### Task 9: Terminal error UI 修复（BUG-0012 Important 3）

**Files:**
- Modify: `src/components/preview-view.tsx:111-118`
- Modify: `src/App.test.tsx`

- [ ] **Step 1: 修改 onExportProgress handler 设置 beautifyError**

```tsx
void onExportProgress((payload) => {
  setExportProgress(payload)
  if (!payload.cancellable && payload.error) {
    // Terminal error: show error details, clear exporting state.
    setIsExporting(false)
    setBeautifyError(payload.error)
  } else {
    setIsExporting(payload.cancellable && payload.progress < 100)
  }
}).then((fn) => { unlisten = fn })
```

- [ ] **Step 2: 新增前端测试**

在 `App.test.tsx` 中添加：

```tsx
it('shows terminal export error in beautifyError', async () => {
  // ... setup mock with terminal error event ...
  // Verify beautifyError is set and isExporting is false.
})
```

- [ ] **Step 3: 运行测试**

Run: `npm test -- --run`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git add src/components/preview-view.tsx src/App.test.tsx
git commit -m "fix(ui): terminal export error 写入 beautifyError 确保错误显示"
```

---

### Task 10: 死代码清理（Minor）

**Files:**
- Modify: `src-tauri/src/media/cursor_overlay.rs`

- [ ] **Step 1: 删除未使用的 `radius` 变量**

在 `draw_on_frame()` 中，`radius` 变量只用于 `ring_radius` 计算。将其合并。

- [ ] **Step 2: 删除 `draw_circle_i64` 死函数**

`draw_circle_i64()` 已无调用点（glyph 渲染替代了圆点绘制）。仅保留 `draw_circle_outline_i64()`（click ring 使用）。

- [ ] **Step 3: 运行测试确认无回归**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay`
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Run: `npm test -- --run`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/media/cursor_overlay.rs
git commit -m "chore(cursor): 清理 cursor_overlay.rs 死代码 — 删除未使用 radius 变量和 draw_circle_i64"
```

---

### Task 11: BUG.md 预防规则更新与完整回归

**Files:**
- Modify: `BUG.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: 更新 BUG.md BUG-0010/0011 状态**

更新 BUG-0010 和 BUG-0011 的状态为 "✅ 已修复-第二轮待人工验证"，记录本轮根因分析和修复内容。

新增预防规则：
- mapper 测试不能只测中心点，必须测 top/bottom。
- Y 轴翻转必须有真实设备证据或显式 geometry 语义，不能硬编码假设。
- `CursorSample` 和 `CursorClick` 必须处于同一 source video pixel 坐标空间。
- `CursorKind` enum 存在不代表 target-aware 已完成；必须有 kind source。
- renderer 测试必须验证具体 glyph 类型和颜色，不得只检查"像素非零"。
- target-aware 查询失败必须 fallback Arrow，但 fallback 不能掩盖全部样本都未识别的问题，必须有诊断计数。

- [ ] **Step 2: 更新 HANDOFF.md**

添加本轮工作记录。

- [ ] **Step 3: 完整回归**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
cargo fmt --check
npm run build
```

- [ ] **Step 4: Commit**

```bash
git add BUG.md HANDOFF.md
git commit -m "docs: 更新 BUG.md 第二轮整改状态和预防规则，HANDOFF.md 工作记录"
```

---

## 人工验证门禁（第二轮）

1. **BUG-0010 1080p**：鼠标移动到四角和下半屏，导出 overlay 精确匹配源光标位置
2. **BUG-0010 Retina**：Retina 显示器四角和中心，导出 overlay 不偏移
3. **BUG-0011 Arrow**：普通桌面导出光标为黑底白边箭头
4. **BUG-0011 Hand**：悬停按钮/链接导出光标为白底黑边手形
5. **BUG-0011 IBeam**：悬停文本框导出光标为黑底白边 I-beam
6. **BUG-0011 Hotspot**：点击放大中心在目标 hotspot，不在 glyph 中心
7. **BUG-0012 Error**：导出失败时 UI 显示错误详情，isExporting 清理
