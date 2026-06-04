# BUG-0010/0011 第四轮整改实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复第三轮遗留的 4 个 Critical、4 个 Important、1 个 Minor 问题，使 BUG-0010 光标定位可在 raw positioning mode 下通过验证，BUG-0011 在 Accessibility 授权后能识别 Hand/IBeam 并有完整诊断闭环。

**Architecture:** 保持现有三层管线（MacCursorSource → CursorMetadataRecorder → CursorEffectEngine → CursorOverlayRenderer）。核心修复：(1) 合并 AX 全局计数器到 metadata 诊断落盘；(2) 接入 Accessibility 权限检查和录制前门禁；(3) CursorKindProvider 注入生产路径；(4) 新增 MediaTimelineDiagnostics 对齐视频帧和光标时间轴；(5) trim_exporter overlay timestamp 显式 origin mapping；(6) raw positioning 验收模式；(7) Arrow glyph 添加尾部把柄。

**Tech Stack:** Rust (CoreGraphics FFI, Accessibility API), React + TypeScript, Tauri 2.0

---

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `src-tauri/src/platform/macos/permissions.rs` | 新增 Accessibility 权限检查 `AXIsProcessTrusted()` |
| Modify | `src-tauri/src/app/permission_service.rs` | `RecordingPermissions` 新增 `accessibility` 字段 |
| Modify | `src-tauri/src/platform/macos/cursor_kind.rs` | 合并 AX 全局计数器到 diagnostics + 删除 `AXUIElementCreateApplication` FFI + AX 采样日志 |
| Modify | `src-tauri/src/platform/macos/cursor_source.rs` | 注入 `CursorKindProvider` trait，删除重复 TTL |
| Modify | `src-tauri/src/app/cursor_metadata_runtime.rs` | finish() 合并真实 AX counters + MediaTimelineDiagnostics + CursorTimingDiagnostics |
| Modify | `src-tauri/src/media/recording_metadata.rs` | 新增 `MediaTimelineDiagnostics`、`CursorTimingDiagnostics`、`accessibility_permission` 字段 |
| Modify | `src-tauri/src/platform/macos/screen_capture_kit.rs` | 首帧 PTS origin 写入 metadata（非仅 stderr） |
| Modify | `src-tauri/src/media/ffmpeg_writer.rs` | source MP4 第一帧 PTS 记录到 metadata |
| Modify | `src-tauri/src/media/trim_exporter.rs` | overlay timestamp 显式减去 source video PTS origin |
| Modify | `src-tauri/src/media/cursor_engine.rs` | raw positioning mode 测试支持 |
| Modify | `src-tauri/src/media/cursor_overlay.rs` | Arrow glyph 添加尾部把柄 + 像素级测试 |
| Modify | `src-tauri/src/lib.rs` | build_effect_timeline 支持 raw positioning + metadata 传递 source_pts_origin |
| Modify | `src-tauri/src/platform/macos_service.rs` | 录制开始时记录 accessibility 权限状态 + MediaTimelineDiagnostics 收集 |
| Modify | `src/components/recording-panel.tsx` | Accessibility 未授权时显示提示 |
| Modify | `src/lib/tauri.ts` | 前端类型同步 |
| Modify | `src/App.test.tsx` | 前端测试 |
| Modify | `BUG.md` | 更新第四轮整改状态和新增预防规则 |
| Modify | `HANDOFF.md` | 工作记录 |

---

### Task 1: 合并 AX 全局计数器到 metadata（BUG-0011 Critical 4）

**Files:**
- Modify: `src-tauri/src/platform/macos/cursor_kind.rs`
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs`
- Modify: `src-tauri/src/media/recording_metadata.rs`

- [ ] **Step 1: cursor_kind.rs 新增 `cursor_kind_diagnostics_snapshot_with_recorder_counts()` 函数**

在 `cursor_kind.rs` 中新增一个函数，将全局 AX 计数器和 recorder 自身的 kind 计数合并：

```rust
/// Merge global AX diagnostic counters with recorder-local kind counts.
/// Call this at recording stop to get a complete picture.
pub fn cursor_kind_diagnostics_merged(
    recorder_arrow: u64,
    recorder_hand: u64,
    recorder_ibeam: u64,
) -> CursorKindDiagnostics {
    let global = cursor_kind_diagnostics_snapshot();
    CursorKindDiagnostics {
        ax_query_failure_count: global.ax_query_failure_count,
        ax_fallback_arrow_count: global.ax_fallback_arrow_count,
        arrow_count: recorder_arrow,
        hand_count: recorder_hand,
        ibeam_count: recorder_ibeam,
    }
}
```

- [ ] **Step 2: 新增 AX 全局计数器重置函数**

在 `cursor_kind.rs` 测试模块附近新增，用于测试隔离：

```rust
/// Reset all global diagnostic counters. Only for test isolation.
#[cfg(test)]
pub fn reset_global_counters() {
    AX_QUERY_FAILURE_COUNT.store(0, Ordering::Relaxed);
    AX_FALLBACK_ARROW_COUNT.store(0, Ordering::Relaxed);
    KIND_ARROW_COUNT.store(0, Ordering::Relaxed);
    KIND_HAND_COUNT.store(0, Ordering::Relaxed);
    KIND_IBEAM_COUNT.store(0, Ordering::Relaxed);
}
```

- [ ] **Step 3: 修改 `CursorMetadataRecorder.finish()` 使用合并函数**

将 `cursor_metadata_runtime.rs:254-259` 的：

```rust
cursor_kind_diagnostics: Some(CursorKindDiagnostics {
    arrow_count: self.arrow_count,
    hand_count: self.hand_count,
    ibeam_count: self.ibeam_count,
    ..Default::default()
})
```

改为：

```rust
cursor_kind_diagnostics: Some(cursor_kind_diagnostics_merged(
    self.arrow_count,
    self.hand_count,
    self.ibeam_count,
))
```

需要在文件顶部添加 `use crate::platform::macos::cursor_kind::cursor_kind_diagnostics_merged;`。

- [ ] **Step 4: 新增测试验证 AX failure 写入 metadata**

在 `cursor_metadata_runtime.rs` 测试模块中：

```rust
#[test]
fn finish_merges_ax_failure_counts_into_metadata() {
    // Reset global counters to ensure clean state.
    crate::platform::macos::cursor_kind::reset_global_counters();

    // Simulate AX failures by incrementing the global counter directly.
    crate::platform::macos::cursor_kind::AX_QUERY_FAILURE_COUNT
        .fetch_add(5, std::sync::atomic::Ordering::Relaxed);
    crate::platform::macos::cursor_kind::AX_FALLBACK_ARROW_COUNT
        .fetch_add(3, std::sync::atomic::Ordering::Relaxed);

    let mut recorder = CursorMetadataRecorder::new(
        30,
        120_000,
        10_000,
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

    // Record some Arrow samples (simulating fallback).
    for _ in 0..10 {
        recorder.record_snapshot(&CursorSnapshot {
            x: 100.0,
            y: 100.0,
            left_down: false,
            right_down: false,
            middle_down: false,
            kind: CursorKind::Arrow,
        }, 0);
    }

    let metadata = recorder.finish(None, 1_000_000_000);
    let diag = metadata.cursor_kind_diagnostics.unwrap();

    assert_eq!(diag.ax_query_failure_count, 5);
    assert_eq!(diag.ax_fallback_arrow_count, 3);
    assert_eq!(diag.arrow_count, 10);
    assert_eq!(diag.hand_count, 0);
    assert_eq!(diag.ibeam_count, 0);

    // Clean up global state.
    crate::platform::macos::cursor_kind::reset_global_counters();
}
```

- [ ] **Step 5: 更新 stop() 日志逻辑**

在 `cursor_metadata_runtime.rs:328-331` 的警告条件现在可以正确触发了（因为 `ax_query_failure_count` 不再永远为 0）。无需修改代码，只需确认逻辑正确。

- [ ] **Step 6: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata_runtime
cargo test --manifest-path src-tauri/Cargo.toml cursor_kind
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/platform/macos/cursor_kind.rs src-tauri/src/app/cursor_metadata_runtime.rs
git commit -m "fix(cursor): 合并 AX 全局计数器到 metadata 诊断落盘"
```

---

### Task 2: 新增 MediaTimelineDiagnostics 和 CursorTimingDiagnostics（BUG-0010 Critical 1 前置）

**Files:**
- Modify: `src-tauri/src/media/recording_metadata.rs`

- [ ] **Step 1: 新增 `MediaTimelineDiagnostics` 结构体**

在 `recording_metadata.rs` 中添加：

```rust
/// Diagnostics for aligning video frame PTS, cursor timestamps, and export overlay timestamps.
/// Allows post-hoc analysis of whether cursor overlay is using the correct timebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaTimelineDiagnostics {
    /// First valid video PTS from CMSampleBuffer (raw, before normalization).
    pub first_video_pts_nanos_raw: u64,
    /// Session clock value when first video frame callback was entered.
    pub first_video_session_entry_nanos: u64,
    /// First normalized video timestamp (after pts_origin mapping).
    pub first_video_normalized_nanos: u64,
    /// Last normalized video timestamp written to source MP4.
    pub last_video_normalized_nanos: u64,
    /// Total video frames written.
    pub video_frame_count: u64,
    /// First cursor sample timestamp (from SessionClock).
    pub first_cursor_sample_nanos: u64,
    /// Last cursor sample timestamp.
    pub last_cursor_sample_nanos: u64,
    /// Total cursor samples recorded.
    pub cursor_sample_count: u64,
    /// Actual CVPixelBuffer size on first frame (width, height).
    pub first_frame_actual_size: Option<(u32, u32)>,
}

impl Default for MediaTimelineDiagnostics {
    fn default() -> Self {
        Self {
            first_video_pts_nanos_raw: 0,
            first_video_session_entry_nanos: 0,
            first_video_normalized_nanos: 0,
            last_video_normalized_nanos: 0,
            video_frame_count: 0,
            first_cursor_sample_nanos: 0,
            last_cursor_sample_nanos: 0,
            cursor_sample_count: 0,
            first_frame_actual_size: None,
        }
    }
}
```

- [ ] **Step 2: 新增 `CursorTimingDiagnostics` 结构体**

```rust
/// Diagnostics for cursor sampling rate and coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorTimingDiagnostics {
    /// Minimum interval between consecutive cursor samples (nanos).
    pub sample_interval_min_nanos: u64,
    /// Maximum interval between consecutive cursor samples (nanos).
    pub sample_interval_max_nanos: u64,
    /// Average interval between consecutive cursor samples (nanos).
    pub sample_interval_avg_nanos: u64,
    /// Number of cursor samples that were outside the capture geometry.
    pub samples_outside_geometry: u64,
    /// Accessibility permission status at recording start.
    pub accessibility_permission_at_start: String,
}

impl Default for CursorTimingDiagnostics {
    fn default() -> Self {
        Self {
            sample_interval_min_nanos: 0,
            sample_interval_max_nanos: 0,
            sample_interval_avg_nanos: 0,
            samples_outside_geometry: 0,
            accessibility_permission_at_start: "unknown".to_string(),
        }
    }
}
```

- [ ] **Step 3: 扩展 `RecordingMetadata` 结构体**

在 `RecordingMetadata` 中添加新字段（使用 `#[serde(default)]` 保证向后兼容）：

```rust
pub struct RecordingMetadata {
    // ... existing fields ...
    pub cursor_kind_diagnostics: Option<CursorKindDiagnostics>,

    // New fields for fourth round rectification.
    /// Media timeline alignment diagnostics.
    #[serde(default)]
    pub media_timeline_diagnostics: Option<MediaTimelineDiagnostics>,
    /// Cursor sampling rate and coverage diagnostics.
    #[serde(default)]
    pub cursor_timing_diagnostics: Option<CursorTimingDiagnostics>,
}
```

- [ ] **Step 4: 新增序列化/反序列化测试**

```rust
#[test]
fn recording_metadata_serializes_media_timeline_diagnostics() {
    let metadata = RecordingMetadata {
        fps: 30,
        duration_nanos: 5_000_000_000,
        cursor_samples: vec![],
        cursor_clicks: vec![],
        beautify_config: BeautifyConfigSnapshot {
            cursor_magnification: false,
            magnification_factor: 1.0,
            cursor_smoothing: false,
            auto_trim_silences: false,
            trim_sensitivity: "medium".to_string(),
            raw_system_cursor_visible: false,
        },
        cursor_snapshot_success_count: 0,
        cursor_snapshot_error_count: 0,
        capture_geometry: None,
        cursor_kind_diagnostics: None,
        media_timeline_diagnostics: Some(MediaTimelineDiagnostics {
            first_video_pts_nanos_raw: 100_000_000,
            first_video_session_entry_nanos: 50_000_000,
            first_video_normalized_nanos: 0,
            last_video_normalized_nanos: 5_000_000_000,
            video_frame_count: 150,
            first_cursor_sample_nanos: 10_000_000,
            last_cursor_sample_nanos: 5_010_000_000,
            cursor_sample_count: 500,
            first_frame_actual_size: Some((1920, 1080)),
        }),
        cursor_timing_diagnostics: Some(CursorTimingDiagnostics {
            sample_interval_min_nanos: 8_000_000,
            sample_interval_max_nanos: 12_000_000,
            sample_interval_avg_nanos: 10_000_000,
            samples_outside_geometry: 0,
            accessibility_permission_at_start: "granted".to_string(),
        }),
    };

    let json = serde_json::to_string(&metadata).unwrap();
    let deserialized: RecordingMetadata = serde_json::from_str(&json).unwrap();

    let mtd = deserialized.media_timeline_diagnostics.unwrap();
    assert_eq!(mtd.first_video_pts_nanos_raw, 100_000_000);
    assert_eq!(mtd.video_frame_count, 150);
    assert_eq!(mtd.first_frame_actual_size, Some((1920, 1080)));

    let ctd = deserialized.cursor_timing_diagnostics.unwrap();
    assert_eq!(ctd.sample_interval_avg_nanos, 10_000_000);
    assert_eq!(ctd.accessibility_permission_at_start, "granted");
}
```

- [ ] **Step 5: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml recording_metadata
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/recording_metadata.rs
git commit -m "feat(metadata): 新增 MediaTimelineDiagnostics 和 CursorTimingDiagnostics"
```

---

### Task 3: Accessibility 权限检查（BUG-0011 Critical 3）

**Files:**
- Modify: `src-tauri/src/platform/macos/permissions.rs`
- Modify: `src-tauri/src/app/permission_service.rs`
- Modify: `src-tauri/src/lib.rs` (recording_permissions command)

- [ ] **Step 1: permissions.rs 新增 `AXIsProcessTrusted` FFI 声明**

在 `permissions.rs` 的 extern block 中添加：

```rust
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}
```

- [ ] **Step 2: 新增 Accessibility 权限探测函数**

```rust
/// Check if the process has Accessibility (target-aware cursor) permission.
/// AXIsProcessTrusted() returns true if the app is in System Preferences > Privacy > Accessibility.
pub(crate) fn probe_accessibility() -> PermissionStatus {
    // SAFETY: AXIsProcessTrusted() is a read-only query with no side effects.
    // It returns a boolean indicating whether the process is trusted for accessibility.
    let trusted = unsafe { AXIsProcessTrusted() };
    if trusted {
        PermissionStatus::Granted
    } else {
        // Cannot distinguish NotDetermined from Denied via this API alone.
        // For UX purposes, treat as NotDetermined (user may need to grant in System Preferences).
        PermissionStatus::NotDetermined
    }
}
```

- [ ] **Step 3: `MacPermissionProbe` 返回 accessibility 状态**

修改 `MacPermissionProbe::probe()` 方法：

```rust
fn probe(&self) -> RecordingPermissions {
    RecordingPermissions {
        screen_recording: probe_screen_capture(),
        microphone: probe_microphone(),
        accessibility: probe_accessibility(),
    }
}
```

- [ ] **Step 4: `RecordingPermissions` 增加 `accessibility` 字段**

在 `permission_service.rs` 中修改结构体：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingPermissions {
    pub screen_recording: PermissionStatus,
    pub microphone: PermissionStatus,
    /// Accessibility permission — required for target-aware cursor kind (Hand/IBeam).
    /// When Denied/NotDetermined, cursor kind will always fallback to Arrow.
    #[serde(default)]
    pub accessibility: PermissionStatus,
}
```

- [ ] **Step 5: 更新前端 TypeScript 类型**

在 `src/lib/tauri.ts` 的 `RecordingPermissions` 接口中添加：

```typescript
export interface RecordingPermissions {
  screenRecording: PermissionStatus;
  microphone: PermissionStatus;
  /** Accessibility permission — required for target-aware cursor kind (Hand/IBeam). */
  accessibility: PermissionStatus;
}
```

- [ ] **Step 6: 前端 Accessibility 未授权提示**

在 `src/components/recording-panel.tsx` 中，当 `permissions.accessibility !== 'granted'` 且 cursor beautification 启用时，显示提示：

```tsx
{permissions.accessibility !== 'granted' && cursorBeautificationEnabled && (
  <div className="text-xs text-yellow-400/80 mt-1">
    需要辅助功能权限才能识别手形/文本光标。当前将显示标准箭头。
  </div>
)}
```

- [ ] **Step 7: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml permissions
npm test -- --run
```

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/platform/macos/permissions.rs src-tauri/src/app/permission_service.rs src/lib/tauri.ts src/components/recording-panel.tsx
git commit -m "feat(permissions): 新增 Accessibility 权限检查和前端提示"
```

---

### Task 4: CursorKindProvider 注入生产路径（BUG-0011 Important 1）

**Files:**
- Modify: `src-tauri/src/platform/macos/cursor_source.rs`
- Modify: `src-tauri/src/platform/macos/cursor_kind.rs`

- [ ] **Step 1: 重构 `MacCursorSource` 持有 `Box<dyn CursorKindProvider>`**

将 `cursor_source.rs` 中的 `MacCursorSource` 改为：

```rust
pub struct MacCursorSource {
    session_clock: Arc<SessionClock>,
    kind_provider: Box<dyn CursorKindProvider>,
}

impl MacCursorSource {
    pub fn new(session_clock: Arc<SessionClock>) -> Self {
        Self {
            session_clock,
            kind_provider: Box::new(MacCursorKindProvider::new()),
        }
    }

    /// Create with a custom kind provider (for testing).
    #[cfg(test)]
    pub fn with_provider(session_clock: Arc<SessionClock>, provider: Box<dyn CursorKindProvider>) -> Self {
        Self {
            session_clock,
            kind_provider: provider,
        }
    }
}
```

- [ ] **Step 2: 修改 `snapshot()` 使用 provider**

将 `cursor_source.rs:89-92` 的直接调用：

```rust
if self.last_kind_query_at.elapsed() >= self.kind_cache_ttl {
    self.cached_kind = cursor_kind::query_cursor_kind(point.x as f32, point.y as f32);
    self.last_kind_query_at = Instant::now();
}
```

改为使用 provider：

```rust
let kind = self.kind_provider.query(point.x as f32, point.y as f32);
```

同时删除 `MacCursorSource` 中的 `kind_cache_ttl`、`cached_kind`、`last_kind_query_at` 字段（TTL 缓存已内置在 `MacCursorKindProvider` 中）。

- [ ] **Step 3: 删除 `MacCursorSource` 中重复的 TTL 逻辑**

删除 `cursor_source.rs` 中 `kind_cache_ttl`、`cached_kind`、`last_kind_query_at` 字段及其在 `new()` 中的初始化。这些逻辑已由 `MacCursorKindProvider` 内部实现。

- [ ] **Step 4: 新增 mock provider 测试**

在 `cursor_source.rs` 测试模块中添加：

```rust
/// Mock provider that always returns a fixed kind.
struct FixedKindProvider {
    kind: CursorKind,
}

impl CursorKindProvider for FixedKindProvider {
    fn query(&mut self, _global_x: f32, _global_y: f32) -> CursorKind {
        self.kind
    }
}

/// Mock provider that always fails (simulates no Accessibility permission).
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
        if global_x > 960.0 { CursorKind::Hand } else { CursorKind::Arrow }
    }
}
```

- [ ] **Step 5: 新增 `mac_cursor_source_uses_injected_kind_provider` 测试**

```rust
#[test]
fn mac_cursor_source_uses_injected_kind_provider() {
    let clock = Arc::new(SessionClock::new());
    let provider = Box::new(FixedKindProvider { kind: CursorKind::Hand });
    let mut source = MacCursorSource::with_provider(clock, provider);

    let snapshot = source.snapshot().unwrap();
    assert_eq!(snapshot.kind, CursorKind::Hand);
}
```

- [ ] **Step 6: 新增 `mac_cursor_source_preserves_position_timestamp_with_slow_provider` 测试**

```rust
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
fn mac_cursor_source_preserves_position_timestamp_with_slow_provider() {
    let clock = Arc::new(SessionClock::new());
    // Sleep to let some time pass so captured_at_nanos > 0.
    std::thread::sleep(Duration::from_millis(20));

    let provider = Box::new(SlowKindProvider { delay: Duration::from_millis(50) });
    let mut source = MacCursorSource::with_provider(clock.clone(), provider);

    let snapshot = source.snapshot().unwrap();

    // captured_at_nanos should be close to current clock, not delayed by 50ms.
    let clock_now = clock.elapsed_nanos();
    let drift = clock_now.saturating_sub(snapshot.captured_at_nanos);

    // Drift should be small (< 50ms), meaning captured_at was recorded BEFORE kind query.
    assert!(
        drift < 50_000_000,
        "captured_at_nanos drifted too much: {drift}ns (expected < 50ms)"
    );
}
```

- [ ] **Step 7: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_source
cargo test --manifest-path src-tauri/Cargo.toml cursor_kind
```

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/platform/macos/cursor_source.rs src-tauri/src/platform/macos/cursor_kind.rs
git commit -m "refactor(cursor): CursorKindProvider 注入生产路径，删除重复 TTL"
```

---

### Task 5: AX 分类采样日志（BUG-0011 Important 2）

**Files:**
- Modify: `src-tauri/src/platform/macos/cursor_kind.rs`

- [ ] **Step 1: 新增 `AxClassificationLog` 结构体**

在 `cursor_kind.rs` 中：

```rust
/// Per-query classification log entry for debugging why kind was assigned.
#[derive(Debug, Clone)]
struct AxClassificationLog {
    result_code: i32,
    role_chain: Vec<String>,
    has_ax_press: bool,
    classified_kind: CursorKind,
    query_duration_nanos: u64,
}
```

- [ ] **Step 2: 修改 `query_cursor_kind()` 返回日志信息**

将 `query_cursor_kind()` 改为内部函数 `_query_cursor_kind_with_log()` 返回 `(CursorKind, AxClassificationLog)`，然后 `query_cursor_kind()` 调用它并丢弃日志（保持公共 API 不变）。新增限频日志输出：

```rust
/// Rate-limited sampling log: print at most once per second.
static LAST_LOG_TIME: Mutex<Option<Instant>> = Mutex::new(None);
const LOG_INTERVAL: Duration = Duration::from_secs(1);

fn maybe_log_classification(log: &AxClassificationLog, point: (f32, f32)) {
    let mut last = LAST_LOG_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if let Some(last_time) = *last {
        if now.duration_since(last_time) < LOG_INTERVAL {
            return;
        }
    }
    *last = Some(now);
    eprintln!(
        "[cursor-kind-classify] point=({:.0},{:.0}) kind={:?} role_chain={:?} ax_press={} rc={} dur={}µs",
        point.0, point.1,
        log.classified_kind,
        log.role_chain,
        log.has_ax_press,
        log.result_code,
        log.query_duration_nanos / 1000,
    );
}
```

- [ ] **Step 3: 在 `element_role_to_cursor_kind()` 中收集 role chain**

修改函数签名，让它返回 `(CursorKind, Vec<String>, bool)` 而不只是 `CursorKind`：

```rust
fn element_role_to_cursor_kind(
    element: *const std::ffi::c_void,
) -> (CursorKind, Vec<String>, bool) {
    let mut role_chain = Vec::new();
    let mut has_ax_press = false;
    // ... existing logic, push roles to role_chain ...
    // ... check AXPress and set has_ax_press ...
    (classified_kind, role_chain, has_ax_press)
}
```

- [ ] **Step 4: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_kind
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/platform/macos/cursor_kind.rs
git commit -m "feat(cursor): AX 分类限频采样日志，支持 role chain 审计"
```

---

### Task 6: 收集 MediaTimelineDiagnostics 数据（BUG-0010 Critical 1）

**Files:**
- Modify: `src-tauri/src/platform/macos/screen_capture_kit.rs`
- Modify: `src-tauri/src/media/ffmpeg_writer.rs`
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs`
- Modify: `src-tauri/src/platform/macos_service.rs`

- [ ] **Step 1: screen_capture_kit.rs 新增首帧 PTS origin getter**

在 `StreamOutputIvars` 或公开接口上添加方法，允许外部读取首帧 PTS 和 session entry：

```rust
/// Get the first video frame's raw PTS nanos and session entry nanos.
/// Returns (first_pts_nanos, session_entry_nanos) if a frame has been received.
pub fn first_frame_timing(&self) -> Option<(u64, u64)> {
    self.pts_origin.lock().ok().and_then(|guard| *guard)
}
```

同时新增获取实际 buffer size 的方法（在视频回调中已打印，需改为可读取）：

在 `StreamOutputIvars` 中新增字段：
```rust
first_frame_actual_size: Mutex<Option<(u32, u32)>>,
```

在视频回调中（line ~207 附近），首帧时写入：
```rust
{
    let mut size_guard = ivars.first_frame_actual_size.lock().unwrap_or_else(|e| e.into_inner());
    if size_guard.is_none() {
        *size_guard = Some((width as u32, height as u32));
    }
}
```

- [ ] **Step 2: ffmpeg_writer.rs 记录 source MP4 第一帧 PTS**

在 `FfmpegRecordingWriter` 中新增字段：

```rust
first_video_pts_written: Option<i64>,
```

在 `push_video()` 中，首次写入时记录：

```rust
if self.first_video_pts_written.is_none() {
    self.first_video_pts_written = Some(pts);
}
```

新增 getter：

```rust
/// Get the first video PTS written to the source MP4, in encoder time_base units.
pub fn first_video_pts_written(&self) -> Option<i64> {
    self.first_video_pts_written
}
```

- [ ] **Step 3: cursor_metadata_runtime.rs 收集 CursorTimingDiagnostics**

在 `CursorMetadataRecorder` 中新增字段：

```rust
first_sample_nanos: Option<u64>,
last_sample_nanos: Option<u64>,
prev_sample_nanos: Option<u64>,
sample_interval_min_nanos: u64,
sample_interval_max_nanos: u64,
sample_interval_sum_nanos: u64,
sample_count: u64,
samples_outside_geometry: u64,
```

在 `record_snapshot()` 中更新这些字段。在 `finish()` 中构建 `CursorTimingDiagnostics`。

- [ ] **Step 4: macos_service.rs 组装 MediaTimelineDiagnostics**

在录制停止时，从各个组件收集数据组装 `MediaTimelineDiagnostics`：

```rust
let media_timeline_diag = MediaTimelineDiagnostics {
    first_video_pts_nanos_raw: screen_capture.first_frame_timing().map(|t| t.0).unwrap_or(0),
    first_video_session_entry_nanos: screen_capture.first_frame_timing().map(|t| t.1).unwrap_or(0),
    first_video_normalized_nanos: /* from writer or metadata */,
    last_video_normalized_nanos: /* from writer duration */,
    video_frame_count: /* from writer diagnostics */,
    first_cursor_sample_nanos: cursor_timing.first_sample_nanos.unwrap_or(0),
    last_cursor_sample_nanos: cursor_timing.last_sample_nanos.unwrap_or(0),
    cursor_sample_count: cursor_timing.sample_count,
    first_frame_actual_size: screen_capture.first_frame_actual_size(),
};
```

传递给 `recorder.finish()` 并写入 `RecordingMetadata`。

- [ ] **Step 5: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata_runtime
cargo test --manifest-path src-tauri/Cargo.toml recording_metadata
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/platform/macos/screen_capture_kit.rs src-tauri/src/media/ffmpeg_writer.rs src-tauri/src/app/cursor_metadata_runtime.rs src-tauri/src/platform/macos_service.rs
git commit -m "feat(metadata): 收集 MediaTimelineDiagnostics 视频帧/光标时间轴对齐数据"
```

---

### Task 7: trim_exporter overlay timestamp 显式 origin mapping（BUG-0010 Critical 1）

**Files:**
- Modify: `src-tauri/src/media/trim_exporter.rs`
- Modify: `src-tauri/src/media/recording_metadata.rs` (EffectTimeline 扩展)
- Modify: `src-tauri/src/lib.rs` (传递 source_pts_origin)

- [ ] **Step 1: EffectTimeline 新增 `source_video_pts_origin_nanos` 字段**

在 `cursor_engine.rs` 或 `timeline.rs` 的 `EffectTimeline` 结构体中添加：

```rust
/// The PTS origin of the source video in nanos.
/// When decoding source MP4, the first frame's PTS may not be 0.
/// Overlay timestamps must subtract this origin to align with cursor timeline.
#[serde(default)]
pub source_video_pts_origin_nanos: u64,
```

- [ ] **Step 2: trim_exporter 读取 source MP4 第一帧 PTS**

在 `trim_exporter.rs` 的导出循环开始前，解码第一帧获取实际 PTS：

```rust
// Read the actual first frame PTS from the source MP4.
let source_first_pts = {
    let mut first_pts: Option<i64> = None;
    // Re-seek to start and read first packet.
    // ... existing demuxer logic ...
    first_pts.unwrap_or(0)
};
let source_first_pts_nanos = time_base_units_to_nanos(source_first_pts, video_time_base)
    .unwrap_or(0)
    .max(0) as u64;
```

- [ ] **Step 3: 修改 overlay timestamp 计算**

将 `trim_exporter.rs:913-923` 的：

```rust
let source_nanos = time_base_units_to_nanos(raw_pts, video_time_base).unwrap_or(0).max(0) as u64;
```

改为：

```rust
let decoded_nanos = time_base_units_to_nanos(raw_pts, video_time_base).unwrap_or(0).max(0) as u64;
// Align decoded PTS with cursor timeline: subtract source MP4's first-PTS origin,
// then add cursor timeline's origin (which is 0 if cursor started at session start).
let source_nanos = decoded_nanos.saturating_sub(source_first_pts_nanos);
```

这样当 source MP4 第一帧 PTS 不为 0 时，overlay 时间轴仍与 cursor timeline 对齐。

- [ ] **Step 4: 新增测试 `overlay_uses_source_pts_origin_when_input_pts_is_non_zero`**

```rust
#[test]
fn overlay_uses_source_pts_origin_when_input_pts_is_non_zero() {
    // Create a cursor timeline starting at t=0.
    let samples = vec![
        CursorSample { timestamp: 0, x: 100.0, y: 100.0, kind: CursorKind::Arrow, ..Default::default() },
        CursorSample { timestamp: 1_000_000_000, x: 200.0, y: 200.0, kind: CursorKind::Arrow, ..Default::default() },
    ];
    // ... build effect timeline from samples ...

    // Simulate source MP4 where first frame PTS = 200ms in time_base units.
    // Decoded frame at 200ms should query cursor at t=0 (200ms - 200ms origin = 0).
    // Decoded frame at 1200ms should query cursor at t=1000ms.

    // Verify overlay selects correct cursor frame after origin subtraction.
    // (Detailed implementation depends on test helper infrastructure.)
}
```

- [ ] **Step 5: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml trim_exporter
cargo test --manifest-path src-tauri/Cargo.toml cursor_engine
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/trim_exporter.rs src-tauri/src/media/recording_metadata.rs src-tauri/src/lib.rs
git commit -m "fix(export): overlay timestamp 显式减去 source video PTS origin"
```

---

### Task 8: raw positioning 验收模式（BUG-0010 Critical 2）

**Files:**
- Modify: `src-tauri/src/media/cursor_engine.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: `CursorEffectEngine` 新增 raw positioning 模式**

在 `CursorEffectEngine` 中添加配置：

```rust
/// When true, disable smoothing and magnification for position verification.
/// Only renders glyph at raw cursor position with no Bezier interpolation.
pub raw_positioning_mode: bool,
```

- [ ] **Step 2: `build_timeline()` 在 raw mode 下跳过 smoothing 和 Bezier**

修改 `cursor_engine.rs:421-441`：

```rust
fn build_timeline(&self, samples: &[CursorSample], clicks: &[CursorClick], fps: u32) -> EffectTimeline {
    let processed_samples = if self.raw_positioning_mode {
        // Raw mode: no smoothing, no Bezier. Use samples directly.
        samples.to_vec()
    } else if self.smoothing_enabled {
        CursorSmoother::new(SmoothingConfig::for_fps(fps)).smooth(samples)
    } else {
        samples.to_vec()
    };

    let frames = if self.raw_positioning_mode {
        // Raw mode: linear interpolation between samples, no Bezier curve.
        Self::linear_interpolate_frames(&processed_samples, fps)
    } else {
        BezierInterpolator::sample_frames(&processed_samples, fps)
    };

    // ... rest of timeline building (clicks still apply if enabled) ...
}
```

新增 `linear_interpolate_frames()` 辅助函数，按视频帧时间戳线性插值 cursor 位置。

- [ ] **Step 3: `build_effect_timeline_from_metadata()` 支持 raw mode**

在 `lib.rs` 中，当配置指定 raw positioning mode 时，强制：

```rust
let engine = if raw_positioning_mode {
    CursorEffectEngine::with_smoothing(ClickAnimationConfig {
        max_scale: 1.0,  // no magnification
        peak_opacity: 0.0, // no click effect
    })
    .with_raw_positioning_mode(true)
} else {
    // existing engine config
};
```

- [ ] **Step 4: 新增测试 `effect_timeline_raw_positioning_disables_smoothing`**

```rust
#[test]
fn effect_timeline_raw_positioning_disables_smoothing() {
    // Create samples with a sharp direction change.
    let samples = vec![
        CursorSample { timestamp: 0, x: 100.0, y: 100.0, ..Default::default() },
        CursorSample { timestamp: 33_333_333, x: 200.0, y: 100.0, ..Default::default() },
        CursorSample { timestamp: 66_666_666, x: 100.0, y: 100.0, ..Default::default() }, // sharp reversal
    ];

    // With raw mode: frame at t=33ms should be exactly at (200, 100).
    let engine = CursorEffectEngine::raw_positioning();
    let timeline = engine.build_timeline(&samples, &[], 30);

    let frame_at_reversal = timeline.frames.iter()
        .find(|f| f.timestamp >= 33_333_333 && f.timestamp < 40_000_000)
        .expect("should have frame near reversal");

    // In raw mode, position should be very close to sample position (no smoothing lag).
    assert!((frame_at_reversal.x - 200.0).abs() < 5.0,
        "raw mode x should be near 200, got {}", frame_at_reversal.x);
}
```

- [ ] **Step 5: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_engine
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/cursor_engine.rs src-tauri/src/lib.rs
git commit -m "feat(cursor): 新增 raw positioning 验收模式，关闭 smoothing/Bezier"
```

---

### Task 9: 录制时记录 Accessibility 权限状态到 metadata

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/app/cursor_metadata_runtime.rs`

- [ ] **Step 1: `CursorMetadataRuntime::spawn()` 接收 accessibility 权限状态**

修改 `spawn()` 签名，新增参数：

```rust
pub fn spawn(
    source: impl CursorSnapshotSource + 'static,
    fps: u32,
    session_clock: Arc<SessionClock>,
    beautify_config: BeautifyConfigSnapshot,
    coordinate_mapper: Option<CursorCoordinateMapper>,
    accessibility_status: PermissionStatus, // NEW
) -> Self
```

在后台线程中保存该状态，传递给 `CursorTimingDiagnostics`。

- [ ] **Step 2: macos_service.rs 传递权限状态**

在录制开始时获取 accessibility 状态并传递给 runtime：

```rust
let accessibility_status = permission_probe.probe().accessibility;
let cursor_runtime = CursorMetadataRuntime::spawn(
    cursor_source,
    capture_config.fps,
    session_clock.clone(),
    beautify_snapshot,
    coordinate_mapper,
    accessibility_status,
);
```

- [ ] **Step 3: `finish()` 将 accessibility 状态写入 `CursorTimingDiagnostics`**

```rust
cursor_timing_diagnostics: Some(CursorTimingDiagnostics {
    sample_interval_min_nanos: self.sample_interval_min_nanos,
    sample_interval_max_nanos: self.sample_interval_max_nanos,
    sample_interval_avg_nanos: if self.sample_count > 0 { self.sample_interval_sum_nanos / self.sample_count } else { 0 },
    samples_outside_geometry: self.samples_outside_geometry,
    accessibility_permission_at_start: format!("{:?}", self.accessibility_status),
}),
```

- [ ] **Step 4: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_metadata_runtime
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/platform/macos_service.rs src-tauri/src/app/cursor_metadata_runtime.rs
git commit -m "feat(metadata): 录制时记录 Accessibility 权限状态到 CursorTimingDiagnostics"
```

---

### Task 10: 删除未使用 FFI 声明（Minor）

**Files:**
- Modify: `src-tauri/src/platform/macos/cursor_kind.rs`

- [ ] **Step 1: 删除 `AXUIElementCreateApplication` FFI 声明**

在 `cursor_kind.rs` 的 extern block 中删除：

```rust
// DELETE THIS LINE:
fn AXUIElementCreateApplication(pid: i32) -> *const std::ffi::c_void;
```

该函数已在第三轮中被 `AXUIElementCreateSystemWide()` 替代，不应再使用。

- [ ] **Step 2: 运行测试确认无引用**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_kind
cargo build --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/platform/macos/cursor_kind.rs
git commit -m "chore(cursor): 删除已废弃的 AXUIElementCreateApplication FFI 声明"
```

---

### Task 11: Arrow glyph 添加尾部把柄（BUG-0011 Important 4）

**Files:**
- Modify: `src-tauri/src/media/cursor_overlay.rs`

- [ ] **Step 1: 更新 ARROW_GLYPH bitmask**

当前 Arrow 的 24x24 bitmask 在 row 19 有一个水平 "shelf"（列 3-18），但缺少 macOS 风格的把柄。更新 bitmask，在箭头尾部添加一个短的垂直把柄：

在 `cursor_overlay.rs:91-149` 中修改 `ARROW_GLYPH` 数据，在 row 20-23 的把柄区域增加黑色主体和白色边框，使箭头尾部有一个短矩形把柄（约 3-4 像素高，3-4 像素宽），与 macOS 标准箭头光标一致。

新 bitmask 设计（24x24，W=White, B=Black, T=Transparent）：

```
Row 0:  W T T T ... (尖端)
Row 1:  W B T T ...
Row 2:  W B B T ...
...
Row 17: W B B B B B B B B B B B B B T T T T T T T T T T
Row 18: W B B B B B B B B B B B B B B T T T T T T T T T
Row 19: W W B B B B B B B B B B B B B B T T T T T T T T  (shelf)
Row 20: T T W B B B B B T T T T T T T T T T T T T T T T  (把柄开始)
Row 21: T T T W B B B T T T T T T T T T T T T T T T T T
Row 22: T T T T W B T T T T T T T T T T T T T T T T T T
Row 23: T T T T T W T T T T T T T T T T T T T T T T T T  (把柄结束)
```

精确像素数据需要根据 macOS 原生箭头光标比例调整。hotspot 保持在 (0, 0)。

- [ ] **Step 2: 新增 Arrow 把柄区域像素测试**

```rust
#[test]
fn arrow_glyph_has_tail_handle_and_hotspot_still_at_tip() {
    let glyph = &ARROW_GLYPH;

    // Hotspot must be at (0, 0) — the arrow tip.
    assert_eq!(glyph.hotspot_x, 0);
    assert_eq!(glyph.hotspot_y, 0);

    // Check that the tail handle region (rows 20-23, cols ~2-5) has black pixels.
    let has_handle_black = (20..24).any(|row| {
        (2..6).any(|col| {
            glyph.get_pixel(row, col) == PixelKind::Black
        })
    });
    assert!(has_handle_black, "arrow glyph should have a tail handle with black pixels");

    // Check that the handle is surrounded by white outline.
    let has_handle_white = (20..24).any(|row| {
        (1..7).any(|col| {
            glyph.get_pixel(row, col) == PixelKind::White
        })
    });
    assert!(has_handle_white, "arrow glyph tail handle should have white outline");
}
```

- [ ] **Step 3: 运行测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml cursor_overlay
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/media/cursor_overlay.rs
git commit -m "feat(cursor): Arrow glyph 添加尾部把柄形状，匹配 macOS 原生光标"
```

---

### Task 12: 更新 BUG.md 和 HANDOFF.md

**Files:**
- Modify: `BUG.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: 更新 BUG.md BUG-0010 第四轮整改状态**

在 BUG-0010_5 之后新增：

```markdown
#### BUG-0010_6: 第 4 轮整改状态

✅ 已修复-第四轮待人工验证

**本轮根因**：视频帧 PTS、cursor sample timestamp、effect timeline timestamp、导出 overlay source timestamp 缺少显式对齐契约；smoothing/Bezier 插值可能污染定位验收；source MP4 第一帧 PTS 不为 0 时 overlay 时间轴错位。

**本轮整改内容**：

1. 新增 MediaTimelineDiagnostics — 记录视频首帧 PTS origin、首帧 normalized timestamp、首末 cursor timestamp、帧数/样本数对比
2. 新增 CursorTimingDiagnostics — 记录采样间隔 min/max/avg、几何外样本数、Accessibility 权限状态
3. trim_exporter overlay timestamp 显式减去 source MP4 第一帧 PTS origin — 对齐 cursor timeline
4. 新增 raw positioning 验收模式 — 关闭 smoothing/Bezier/magnification，仅渲染 glyph
5. CursorTimingDiagnostics 写入 RecordingMetadata — stop 时完整结构化摘要
6. 首帧 actual CVPixelBuffer size 写入 metadata（非仅 stderr）

**新增预防规则**：

11. video frame PTS、cursor sample timestamp、effect timeline timestamp、export overlay source timestamp 必须共享同一 source media timebase；任何 offset 都必须写入 metadata 并由测试验证。
12. source artifact 第一帧 PTS 和 metadata 首帧/末帧时间必须有诊断对比；导出 overlay 不得隐式假设 raw_pts 从 0 开始。
13. 光标定位验收必须先在 raw positioning mode 下进行，禁止 smoothing/magnification 影响坐标正确性判断。
14. cursor diagnostics 必须落盘到 sidecar，不能只打印到 stderr。
```

- [ ] **Step 2: 更新 BUG.md BUG-0011 第四轮整改状态**

在 BUG-0011_5 之后新增：

```markdown
#### BUG-0011_6: 第 4 轮整改状态

✅ 已修复-第四轮待人工验证

**本轮根因**：(1) AX 查询依赖 Accessibility 权限，但权限模型未检查该权限；(2) AX 全局失败/回退计数未合并到 metadata，diagnostics 显示 0 掩盖了全部 fallback 事实；(3) CursorKindProvider trait 未接入生产路径，测试与生产脱节；(4) Arrow glyph 缺少尾部把柄。

**本轮整改内容**：

1. 新增 Accessibility 权限检查 — `AXIsProcessTrusted()`，前端未授权时显示提示
2. 合并 AX 全局计数器到 metadata — `cursor_kind_diagnostics_merged()` 确保 failure/fallback 真实落盘
3. CursorKindProvider 注入 MacCursorSource — 删除重复 TTL，统一 provider 路径
4. AX 分类限频采样日志 — role chain、result code、query duration，每秒最多 2 条
5. Arrow glyph 添加尾部把柄 — 匹配 macOS 原生箭头光标外形
6. 删除废弃的 AXUIElementCreateApplication FFI 声明

**新增预防规则**：

15. target-aware cursor 依赖 Accessibility 时，Accessibility 权限必须进入录制前门禁和 metadata；未授权不能静默表现为全 Arrow。
16. AX failure/fallback/kind distribution 必须来自实际 provider 并写入 RecordingMetadata，不能只统计 recorder 最终 kind。
17. CursorKindProvider 必须可注入并覆盖生产路径，避免 mock 测试与真实采样脱节。
18. 每次 target-aware 整改必须提供 role/action/classification 采样日志或等价诊断，证明 Hand/IBeam 的来源。
19. glyph 形状变更必须配套 hotspot、颜色、关键形状区域的像素级测试。
```

- [ ] **Step 3: 更新 HANDOFF.md 工作任务记录**

在 HANDOFF.md 的"工作任务记录"部分（时间倒序，保留最近 7 条），添加本轮记录。如果已有 7 条，删除最早的记录。

- [ ] **Step 4: 运行完整回归**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
npm run build
```

- [ ] **Step 5: Commit**

```bash
git add BUG.md HANDOFF.md
git commit -m "docs: 更新 BUG.md 第四轮整改状态和新增预防规则"
```

---

## 自动测试清单

本轮必须通过的测试：

1. `finish_merges_ax_failure_counts_into_metadata` — AX 全局计数合并到 metadata
2. `recording_metadata_serializes_media_timeline_diagnostics` — MediaTimelineDiagnostics 序列化
3. `mac_cursor_source_uses_injected_kind_provider` — provider 注入生效
4. `mac_cursor_source_preserves_position_timestamp_with_slow_provider` — 慢 provider 不污染 timestamp
5. `effect_timeline_raw_positioning_disables_smoothing` — raw mode 关闭 smoothing
6. `overlay_uses_source_pts_origin_when_input_pts_is_non_zero` — PTS origin mapping
7. `arrow_glyph_has_tail_handle_and_hotspot_still_at_tip` — Arrow 把柄形状
8. 既有测试全部不回归

验证命令：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
npm run build
```

## 人工验证清单

### BUG-0010 raw positioning

录制条件：
- cursor smoothing off
- cursor magnification off 或 scale=1
- raw system cursor hidden，overlay glyph on
- Accessibility 状态记录在 metadata 中

场景：
1. 静止桌面四角和中心
2. 水平从左到右匀速移动
3. 水平从右到左匀速移动
4. 多段移动：左到右、停顿、右到左、停顿、再左到右
5. Retina 显示器

通过标准：
- Y 轴保持准确
- X 轴不随时间累计向右或向左漂
- metadata 中 video/cursor first/last timestamp 差异在预期范围内

### BUG-0011 target-aware kind

前置条件：
- Accessibility granted
- metadata 中 `accessibility_permission_at_start=granted`
- AX failure/fallback/kind distribution 可见

场景：
1. 普通桌面：Arrow
2. Tauri 按钮：Hand
3. 浏览器链接：Hand
4. 原生文本框：IBeam
5. Tauri / Electron input：IBeam

通过标准：
- metadata 中 Hand/IBeam count > 0
- 如果某场景失败，metadata 或日志能看到 role/action/classification 依据
- 导出视频 glyph 与 metadata kind 分布一致

### BUG-0011 glyph 外形

场景：
1. Arrow 截帧
2. Hand 截帧
3. IBeam 截帧

通过标准：
- Arrow 黑底白边，并有尾部把柄
- Hand 白底黑边，外形短而清晰
- IBeam 黑底白边，hotspot 居中且不遮挡文本定位
