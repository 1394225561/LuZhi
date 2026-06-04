# BUG-0010/0011 第三轮整改实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复第二轮 BUG-0010/BUG-0011 修复中遗留的 3 个 Critical、3 个 Important、2 个 Minor 问题，使光标坐标时间戳正确、AX kind provider 使用正确的 system-wide root、诊断信息可见化，全部通过人工验证。

**Architecture:** 保持现有三层管线（MacCursorSource → CursorMetadataRecorder → CursorEffectEngine → CursorOverlayRenderer）。核心修复：(1) 将坐标采样时间戳从 snapshot 返回后移到 snapshot 开始前，避免 AX 查询耗时污染；(2) AX provider 改用 `AXUIElementCreateSystemWide()`；(3) 增加 CursorKindDiagnostics 和 cursor timing diagnostics 可见化。

**Tech Stack:** Rust (CoreGraphics FFI, Accessibility API), React + TypeScript, Tauri 2.0

---

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `src-tauri/src/app/cursor_metadata_runtime.rs` | timestamp 生成位置修复 + CursorSnapshot 扩展 captured_at + snapshot duration diagnostics |
| Modify | `src-tauri/src/platform/macos/cursor_kind.rs` | AXUIElementCreateSystemWide + AXUIElementSetMessagingTimeout + error code + kind distribution + parent chain |
| Modify | `src-tauri/src/platform/macos/cursor_source.rs` | MacCursorSource 持有 SessionClock，snapshot() 内记录 captured_at |
| Modify | `src-tauri/src/platform/macos/screen_capture_kit.rs` | 首帧 actual CVPixelBuffer 尺寸诊断 |
| Modify | `src-tauri/src/media/cursor_overlay.rs` | Hand/IBeam rendered Y plane 测试 + 清理陈旧注释 |
| Modify | `src/components/preview-view.tsx` | 无变更（第二轮已完成） |
| Modify | `src/App.test.tsx` | terminal error UI 测试 |
| Modify | `BUG.md` | 更新第三轮验证结果和预防规则 |
| Modify | `HANDOFF.md` | 工作记录 |

---

### Task 1: 新增慢 snapshot 失败测试（BUG-0010 Critical 1 前置）

**Files:**
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs` (tests module)

- [ ] **Step 1: 新增 MockSource 支持可配置延迟**

在 `cursor_metadata_runtime.rs` 测试模块中添加：

```rust
/// Mock source that returns a fixed position and kind, with configurable delay.
struct SlowMockSource {
    snapshot_count: u32,
    delay: Duration,
    kind: CursorKind,
}

impl SlowMockSource {
    fn new(delay: Duration, kind: CursorKind) -> Self {
        Self { snapshot_count: 0, delay, kind }
    }
}

impl CursorSnapshotSource for SlowMockSource {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot> {
        self.snapshot_count += 1;
        if !self.delay.is_zero() {
            thread::sleep(self.delay);
        }
        Ok(CursorSnapshot {
            x: 960.0,
            y: 540.0,
            left_down: false,
            right_down: false,
            middle_down: false,
            kind: self.kind,
        })
    }
}
```

- [ ] **Step 2: 新增慢 snapshot timestamp 不漂移测试**

```rust
#[test]
fn runtime_timestamps_snapshot_at_poll_start_not_after_slow_source() {
    let clock = Arc::new(SessionClock::new());
    let source = SlowMockSource::new(Duration::from_millis(50), CursorKind::Arrow);
    let stop = Arc::new(AtomicBool::new(false));

    let mut runtime = CursorMetadataRuntime::spawn(
        source,
        30,
        clock.clone(),
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

    // Wait for a few samples.
    thread::sleep(Duration::from_millis(200));
    let metadata = runtime.stop().unwrap();

    // The first sample's timestamp should be close to 0, not delayed by 50ms.
    // If timestamp is recorded AFTER snapshot(), the first sample would be ~50ms.
    assert!(
        metadata.cursor_samples[0].timestamp.as_nanos() < 30_000_000,
        "first sample timestamp should be near 0, got {}ms",
        metadata.cursor_samples[0].timestamp.as_nanos() / 1_000_000
    );
}
```

- [ ] **Step 3: 运行测试确认当前实现失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml runtime_timestamps_snapshot_at_poll_start_not_after_slow_source`
Expected: FAIL — first sample timestamp > 30ms (因为 timestamp 在 snapshot() 返回后生成)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/app/cursor_metadata_runtime.rs
git commit -m "test(cursor): 新增慢 snapshot 测试锁定 BUG-0010 timestamp 错位"
```

---

### Task 2: 修复 timestamp 生成位置（BUG-0010 Critical 1 核心）

**Files:**
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs:259-275` (CursorMetadataRuntime::spawn loop)

- [ ] **Step 1: 将 timestamp 移到 snapshot() 调用前**

修改 `CursorMetadataRuntime::spawn()` 中的循环：

```rust
let handle = thread::spawn(move || {
    let mut recorder =
        CursorMetadataRecorder::new(fps.max(1), beautify_snapshot, capture_geometry);
    while !thread_stop.load(Ordering::Relaxed) {
        // Record timestamp BEFORE snapshot() to avoid AX query delay pollution.
        let sample_timestamp = MediaTimestamp::from_nanos(session_clock.elapsed_nanos());
        match source.snapshot() {
            Ok(snapshot) => {
                recorder.record_snapshot(sample_timestamp, snapshot);
            }
            Err(_) => {
                recorder.record_snapshot_failure();
            }
        }
        thread::sleep(interval);
    }

    recorder.finish(session_clock.elapsed_nanos(), capture_geometry)
});
```

- [ ] **Step 2: 运行 Task 1 的慢 snapshot 测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml runtime_timestamps_snapshot_at_poll_start_not_after_slow_source`
Expected: PASS

- [ ] **Step 3: 运行全部 cursor 测试确认无回归**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/app/cursor_metadata_runtime.rs
git commit -m "fix(cursor): timestamp 移到 snapshot() 调用前 — 避免 AX 查询耗时污染坐标时间戳"
```

---

### Task 3: AX 查询限频与 snapshot duration 记录（Important 3）

**Files:**
- Modify: `src-tauri/src/platform/macos/cursor_source.rs`
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs`

- [ ] **Step 1: CursorSnapshot 新增 captured_at 和 snapshot_duration_nanos**

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorSnapshot {
    pub x: f32,
    pub y: f32,
    pub left_down: bool,
    pub right_down: bool,
    pub middle_down: bool,
    pub kind: CursorKind,
    /// Monotonic timestamp when the cursor position was sampled (before AX query).
    pub captured_at_nanos: u64,
    /// Duration of the full snapshot() call in nanoseconds.
    pub snapshot_duration_nanos: u64,
}
```

- [ ] **Step 2: MacCursorSource 记录 captured_at 和 duration**

`MacCursorSource` 需要持有 `SessionClock` 引用来记录 `captured_at`：

```rust
use crate::core::clock::SessionClock;
use std::sync::Arc;

pub struct MacCursorSource {
    session_clock: Arc<SessionClock>,
    /// Kind is cached and only re-queried at most once per this interval.
    kind_cache_ttl: Duration,
    cached_kind: CursorKind,
    last_kind_query_at: Instant,
}

impl MacCursorSource {
    pub fn new(session_clock: Arc<SessionClock>) -> Self {
        Self {
            session_clock,
            kind_cache_ttl: Duration::from_millis(100), // 10Hz kind query
            cached_kind: CursorKind::Arrow,
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
```

- [ ] **Step 3: 更新 CursorMetadataRuntime 使用 captured_at**

由于 Task 2 已将 timestamp 移到 snapshot() 前，这里改为优先使用 `snapshot.captured_at_nanos`（如果非零），否则 fallback 到 poll-start timestamp：

```rust
while !thread_stop.load(Ordering::Relaxed) {
    let poll_start = MediaTimestamp::from_nanos(session_clock.elapsed_nanos());
    match source.snapshot() {
        Ok(snapshot) => {
            // Use snapshot's captured_at if available (more precise), else poll start.
            let ts = if snapshot.captured_at_nanos > 0 {
                MediaTimestamp::from_nanos(snapshot.captured_at_nanos)
            } else {
                poll_start
            };
            recorder.record_snapshot(ts, snapshot);
        }
        Err(_) => {
            recorder.record_snapshot_failure();
        }
    }
    thread::sleep(interval);
}
```

- [ ] **Step 4: 更新所有测试中的 CursorSnapshot 构造**

所有 `CursorSnapshot` 构造需要添加 `captured_at_nanos: 0, snapshot_duration_nanos: 0`。

- [ ] **Step 5: 更新 MacCursorSource::new() 调用点**

所有 `MacCursorSource::new()` 调用需要传入 `session_clock`。搜索 `MacCursorSource::new()` 调用点并更新。

- [ ] **Step 6: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor`
Expected: ALL PASS

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/app/cursor_metadata_runtime.rs src-tauri/src/platform/macos/cursor_source.rs
git commit -m "feat(cursor): AX 查询限频 10Hz + snapshot duration 记录 + captured_at 时间戳"
```

---

### Task 4: 新增 AX provider 测试锁定 system-wide root（BUG-0011 Critical 2 前置）

**Files:**
- Modify: `src-tauri/src/platform/macos/cursor_kind.rs` (tests module)

- [ ] **Step 1: 新增 CursorKindProvider trait 用于可测试性**

在 `cursor_kind.rs` 中新增 trait：

```rust
/// Trait abstracting cursor kind queries for testability.
pub trait CursorKindProvider: Send + 'static {
    fn query(&mut self, global_x: f32, global_y: f32) -> CursorKind;
}
```

- [ ] **Step 2: 实现 MacCursorKindProvider**

```rust
/// Production macOS cursor kind provider using Accessibility API.
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

impl Default for MacCursorKindProvider {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: 新增 AX diagnostics 结构体**

```rust
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
        ax_query_failure_count: ax_query_failure_count(),
        ax_fallback_arrow_count: ax_fallback_arrow_count(),
        arrow_count: KIND_ARROW_COUNT.load(Ordering::Relaxed),
        hand_count: KIND_HAND_COUNT.load(Ordering::Relaxed),
        ibeam_count: KIND_IBEAM_COUNT.load(Ordering::Relaxed),
    }
}
```

- [ ] **Step 4: 新增 kind distribution 计数器**

在 `cursor_kind.rs` 顶部新增：

```rust
static KIND_ARROW_COUNT: AtomicU64 = AtomicU64::new(0);
static KIND_HAND_COUNT: AtomicU64 = AtomicU64::new(0);
static KIND_IBEAM_COUNT: AtomicU64 = AtomicU64::new(0);
```

在 `query_cursor_kind()` 返回前递增对应计数器。

- [ ] **Step 5: 新增 MockCursorKindProvider 测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_provider_returns_configured_kind() {
        let mut provider = MockCursorKindProvider { kind: CursorKind::Hand };
        assert_eq!(provider.query(100.0, 200.0), CursorKind::Hand);
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
    }
}

/// Mock provider for testing.
#[cfg(test)]
struct MockCursorKindProvider {
    kind: CursorKind,
}

#[cfg(test)]
impl CursorKindProvider for MockCursorKindProvider {
    fn query(&mut self, _global_x: f32, _global_y: f32) -> CursorKind {
        self.kind
    }
}
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/platform/macos/cursor_kind.rs
git commit -m "test(cursor): 新增 CursorKindProvider trait + diagnostics + mock 测试"
```

---

### Task 5: 修复 AX provider 使用 system-wide root（BUG-0011 Critical 2 核心）

**Files:**
- Modify: `src-tauri/src/platform/macos/cursor_kind.rs` (FFI + query_cursor_kind)

- [ ] **Step 1: FFI 新增 AXUIElementCreateSystemWide 和 AXUIElementSetMessagingTimeout**

在 `cursor_kind.rs` 的 FFI 绑定中添加：

```rust
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout_in_seconds: f32) -> i32;
    // ... existing bindings ...
}
```

- [ ] **Step 2: 替换 query_cursor_kind 使用 system-wide root**

```rust
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
            CursorKind::Hand => KIND_HAND_COUNT.fetch_add(1, Ordering::Relaxed),
            CursorKind::IBeam => KIND_IBEAM_COUNT.fetch_add(1, Ordering::Relaxed),
        }

        kind
    }
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/platform/macos/cursor_kind.rs
git commit -m "fix(cursor): AX provider 改用 AXUIElementCreateSystemWide — 修复跨应用 hit-test"
```

---

### Task 6: 扩展 CursorKind 分类 — parent chain / action / editable（Important 1）

**Files:**
- Modify: `src-tauri/src/platform/macos/cursor_kind.rs` (element_role_to_cursor_kind)

- [ ] **Step 1: FFI 新增 AXUIElementCopyActionNames**

```rust
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    // ... existing bindings ...
    fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut CFTypeRef) -> i32;
}
```

- [ ] **Step 2: 新增 action 检查 helper**

```rust
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
```

新增 FFI：

```rust
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    // ... existing bindings ...
    fn CFArrayGetCount(the_array: *const std::ffi::c_void) -> i64;
    fn CFArrayGetValueAtIndex(the_array: *const std::ffi::c_void, idx: i64) -> *const std::ffi::c_void;
}
```

- [ ] **Step 3: 新增 parent 回溯 helper**

```rust
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
```

- [ ] **Step 4: 重写 element_role_to_cursor_kind 支持 parent 回溯**

```rust
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
            Some("AXTextField") | Some("AXTextArea") => return CursorKind::IBeam,
            // Hand: clickable elements.
            Some("AXButton")
            | Some("AXLink")
            | Some("AXMenuItem")
            | Some("AXMenuBarItem")
            | Some("AXCheckBox")
            | Some("AXRadioButton")
            | Some("AXPopUpButton")
            | Some("AXComboBox") => return CursorKind::Hand,
            _ => {}
        }

        // Check if element supports AXPress action (clickable but role not in list).
        if element_supports_press(current) {
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

    // Clean up retained parent refs.
    for parent_ref in retained_refs {
        CFRelease(parent_ref as CFTypeRef);
    }

    CursorKind::Arrow
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
```

- [ ] **Step 5: 移除旧的 classify_by_role 和 element_supports_action**

删除不再需要的 `classify_by_role()` 和旧的 `element_supports_action()` 函数。

- [ ] **Step 6: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor`
Expected: ALL PASS

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/platform/macos/cursor_kind.rs
git commit -m "feat(cursor): 扩展 CursorKind 分类 — parent chain 回溯 + AXPress action 检查"
```

---

### Task 7: CursorKindDiagnostics 写入 RecordingMetadata（Important 2）

**Files:**
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs`
- Modify: `src-tauri/src/media/recording_metadata.rs`

- [ ] **Step 1: RecordingMetadata 新增 cursor_kind_diagnostics 字段**

在 `recording_metadata.rs` 的 `RecordingMetadata` 结构体中添加：

```rust
/// Cursor kind distribution and AX query diagnostics.
#[serde(skip_serializing_if = "Option::is_none")]
pub cursor_kind_diagnostics: Option<CursorKindDiagnostics>,
```

- [ ] **Step 2: CursorMetadataRecorder 收集 kind 分布**

在 `CursorMetadataRecorder` 中新增计数：

```rust
struct CursorMetadataRecorder {
    // ... existing fields ...
    arrow_count: u64,
    hand_count: u64,
    ibeam_count: u64,
}
```

在 `record_snapshot()` 中按 kind 递增。

- [ ] **Step 3: finish() 写入 diagnostics**

```rust
pub fn finish(self, duration_nanos: u64, capture_geometry: Option<CaptureGeometry>) -> RecordingMetadata {
    RecordingMetadata {
        // ... existing fields ...
        cursor_kind_diagnostics: Some(CursorKindDiagnostics {
            arrow_count: self.arrow_count,
            hand_count: self.hand_count,
            ibeam_count: self.ibeam_count,
            ..Default::default()
        }),
    }
}
```

- [ ] **Step 4: stop 时打印结构化诊断日志**

在 `CursorMetadataRuntime::stop()` 中：

```rust
pub fn stop(&mut self) -> Option<RecordingMetadata> {
    self.stop.store(true, Ordering::Relaxed);
    let result = self.handle.take().and_then(|handle| handle.join().ok());
    if let Some(ref metadata) = result {
        if let Some(ref diag) = metadata.cursor_kind_diagnostics {
            eprintln!(
                "[cursor-kind-diagnostics] arrow={} hand={} ibeam={} ax_fail={} ax_fallback_arrow={}",
                diag.arrow_count, diag.hand_count, diag.ibeam_count,
                diag.ax_query_failure_count, diag.ax_fallback_arrow_count
            );
            if diag.hand_count == 0 && diag.ibeam_count == 0 && diag.ax_query_failure_count > 0 {
                eprintln!("[cursor-kind-diagnostics] ⚠️ 未检测到 Hand/IBeam，AX 查询存在失败");
            }
        }
    }
    result
}
```

- [ ] **Step 5: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/app/cursor_metadata_runtime.rs src-tauri/src/media/recording_metadata.rs
git commit -m "feat(cursor): CursorKindDiagnostics 写入 RecordingMetadata + stop 时结构化日志"
```

---

### Task 8: 首帧 actual CVPixelBuffer 尺寸诊断（Critical 3）

**Files:**
- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs`

- [ ] **Step 1: 在视频帧回调中记录首帧 actual 尺寸**

在 `stream_output` 回调的视频帧处理路径中，添加首帧诊断：

```rust
// In the video frame callback, after getting the CVPixelBuffer:
static FIRST_FRAME_DIAGNOSED: AtomicBool = AtomicBool::new(false);

if !FIRST_FRAME_DIAGNOSED.swap(true, Ordering::Relaxed) {
    let actual_width = CVPixelBufferGetWidth(pixel_buffer);
    let actual_height = CVPixelBufferGetHeight(pixel_buffer);
    let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
    eprintln!(
        "[sck-first-frame] actual_buffer={}×{} bytes_per_row={} config_stream={}×{}",
        actual_width, actual_height, bytes_per_row,
        config_width, config_height
    );
    if actual_width != config_width as usize || actual_height != config_height as usize {
        eprintln!(
            "[sck-first-frame] ⚠️ actual buffer size ≠ configured stream size! \
             cursor coordinate mapping may be incorrect."
        );
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add src-tauri/src/platform/macos/screen_capture_kit.rs
git commit -m "feat(cursor): 首帧 CVPixelBuffer 实际尺寸诊断日志"
```

---

### Task 9: 补 renderer Hand/IBeam rendered Y plane 测试（Phase 4）

**Files:**
- Modify: `src-tauri/src/media/cursor_overlay.rs` (tests module)

- [ ] **Step 1: 新增 Hand rendered Y plane 测试**

```rust
#[test]
fn rendered_hand_has_correct_y_plane_values() {
    let timeline = make_timeline(vec![cursor_frame(0, 960.0, 540.0, CursorKind::Hand)], true);
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

    // Hand interior should be white fill (Y≈235).
    // Find any white pixel in the hand glyph area.
    let hand_center_offset = 540 * linesize + 960;
    // The hand glyph center should contain white pixels.
    let has_white = y_plane[hand_center_offset..hand_center_offset + 24]
        .iter()
        .any(|&y| y > 200);
    assert!(has_white, "hand glyph should have white fill pixels near center");
}
```

- [ ] **Step 2: 新增 IBeam rendered Y plane 测试**

```rust
#[test]
fn rendered_ibeam_has_correct_y_plane_values() {
    let timeline = make_timeline(vec![cursor_frame(0, 960.0, 540.0, CursorKind::IBeam)], true);
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

    // IBeam body should be black (Y≈16).
    let ibeam_center_offset = (540 + 10) * linesize + (960 + 7);
    assert!(
        y_plane[ibeam_center_offset] < 50,
        "ibeam body should be black (Y<50), got {}",
        y_plane[ibeam_center_offset]
    );
}
```

- [ ] **Step 3: 更新现有 renderer 测试的陈旧注释**

清理 `cursor_overlay.rs` 测试中关于 "arrow tip should be black" 的陈旧注释，改为 "arrow tip should be white outline"。

- [ ] **Step 4: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay`
Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/cursor_overlay.rs
git commit -m "test(cursor): 补 Hand/IBeam rendered Y plane 测试 + 清理陈旧注释"
```

---

### Task 10: 补 terminal error UI 测试（Minor / Test Gap）

**Files:**
- Modify: `src/App.test.tsx`

- [ ] **Step 1: 新增 terminal export error 测试**

在 `App.test.tsx` 中添加：

```tsx
it('shows terminal export error in beautifyError', async () => {
  // Setup: mock export-progress event with terminal error
  const mockUnlisten = vi.fn();
  const listeners: Record<string, Function> = {};
  (listen as Mock).mockImplementation((event: string, cb: Function) => {
    listeners[event] = cb;
    return Promise.resolve(mockUnlisten);
  });

  render(<App />);

  // Simulate being in preview state with export in progress.
  // ... setup code to reach preview state with isExporting=true ...

  // Fire terminal error event.
  if (listeners['export-progress']) {
    listeners['export-progress']({
      payload: { progress: 50, cancellable: false, error: 'FFmpeg 编码失败' },
    });
  }

  // Verify: isExporting should be false and error should be displayed.
  // ... assertions based on actual UI structure ...
});
```

- [ ] **Step 2: 运行测试**

Run: `npm test -- --run`
Expected: ALL PASS

- [ ] **Step 3: Commit**

```bash
git add src/App.test.tsx
git commit -m "test(ui): 补 terminal export error UI 测试"
```

---

### Task 11: 更新 BUG.md 预防规则

**Files:**
- Modify: `BUG.md`

- [ ] **Step 1: 更新 BUG-0010 状态和根因**

更新 BUG-0010_3 为 "✅ 已修复-第三轮待人工验证"，记录第三轮根因：
- 主根因：第二轮新增同步 AX kind 查询后，坐标采样时刻和记录 timestamp 不一致
- 修复：timestamp 移到 snapshot() 调用前；AX 查询限频 10Hz

新增预防规则：
- 8. `snapshot()` 内任何可能阻塞的操作（如 AX 查询）不得污染坐标采样 timestamp。
- 9. cursor position 和 cursor kind 对时间精度要求不同；position 必须贴近视频帧时间，kind 可以低频缓存。
- 10. 首帧视频到达时必须记录 actual CVPixelBuffer 尺寸并与 CaptureGeometry 对比。

- [ ] **Step 2: 更新 BUG-0011 状态和根因**

更新 BUG-0011_3 为 "✅ 已修复-第三轮待人工验证"，记录第三轮根因：
- 主根因：AX provider 使用 `AXUIElementCreateApplication(0)` 而非 `AXUIElementCreateSystemWide()`
- 修复：改用 system-wide root + messaging timeout + parent chain 回溯

新增预防规则：
- 11. AX hit-test 必须使用 `AXUIElementCreateSystemWide()`，不能用 `AXUIElementCreateApplication(0)`。
- 12. AX 查询必须设置 messaging timeout（建议 50ms），避免阻塞 cursor runtime。
- 13. CursorKind 分类必须支持 parent chain 回溯（至少 3 层），覆盖浏览器/Electron/Tauri 场景。
- 14. `AXStaticText` 不等于可编辑文本；只有 `AXTextField`/`AXTextArea` 或明确 editable 才判 IBeam。

- [ ] **Step 3: Commit**

```bash
git add BUG.md
git commit -m "docs: 更新 BUG.md 第三轮整改状态和预防规则"
```

---

### Task 12: 更新 HANDOFF.md 与完整回归

**Files:**
- Modify: `HANDOFF.md`

- [ ] **Step 1: 更新 HANDOFF.md**

添加第三轮工作记录。

- [ ] **Step 2: 完整回归**

```bash
cargo fmt --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
npm run build
```

- [ ] **Step 3: Commit**

```bash
git add HANDOFF.md
git commit -m "docs: 更新 HANDOFF.md 第三轮工作记录与回归验证"
```

---

## 人工验证门禁（第三轮）

BUG-0010：

1. 静止 cursor 放四角，导出 overlay 与目标点一致
2. 快速水平移动 cursor，导出 overlay 不再随移动方向左/右漂
3. Retina 显示器四角和中心
4. 外接显示器负 origin
5. 关闭 cursor smoothing 做定位验收

BUG-0011：

1. 普通桌面：Arrow
2. 按钮 / 链接：Hand
3. 文本框：IBeam
4. 关闭 Accessibility 权限：fallback Arrow，diagnostics 显示 failure count
5. 导出 effect timeline 中必须出现非 Arrow kind 样本
