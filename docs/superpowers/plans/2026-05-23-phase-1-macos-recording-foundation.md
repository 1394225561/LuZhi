# Phase 1 macOS Recording Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the W1-W2 foundation: Tauri 2 + React scaffold, Rust recording boundaries, state machine, permission checks, lightweight UI commands, and a macOS ScreenCaptureKit integration gate for producing the first 1080p recording artifact.

**Architecture:** The React UI remains a command/status layer. Rust owns recording state, platform capture abstractions, permission detection, and event payloads. Native macOS capture is isolated under `src-tauri/src/platform/macos`, with a required human review gate before any ScreenCaptureKit callback or unsafe/FFI code is accepted.

**Tech Stack:** Tauri 2, React, TypeScript, Tailwind, Rust, ScreenCaptureKit on macOS, Vitest, Rust unit tests.

---

## Scope

This plan covers only `Phase 1 / W1-W2：脚手架与 macOS 录制闭环`.

It does not implement Windows capture, audio mixing, cursor effects, blank-segment trimming, export presets, licensing, telemetry, or platform publishing APIs.

## Reference Inputs

- `docs/architecture/project-architecture-and-overall-planning.md`
- `tests/phase-1-w1-w2-checklist.md`
- `.codex/rules/0-global.md`
- `.codex/rules/1-coding-style.md`
- `.codex/rules/2-testing.md`
- `.codex/rules/3-git-commit.md`
- `.codex/rules/4-security.md`
- `.codex/rules/5-docs.md`
- Official Tauri create-project guide: `https://v2.tauri.app/start/create-project/`

## Manual Gates

1. Dependency gate: AI must not directly change core dependency versions in `src-tauri/Cargo.toml`. Use the official scaffold output, then stop for human review before adding or changing Rust dependency versions.
2. Native capture gate: ScreenCaptureKit callback, buffer ownership, thread handoff, and any unsafe/FFI code must be reviewed line by line by a human before it is treated as accepted implementation.
3. Push/merge gate: AI must not run `git push` or merge branches.

## File Map

- Create: `package.json` - npm scripts for dev, build, test, lint.
- Create: `index.html` - Vite entry.
- Create: `vite.config.ts` - React + Vitest configuration.
- Create: `tsconfig.json`, `tsconfig.node.json` - TypeScript configuration.
- Create: `src/styles.css` - Tailwind entry stylesheet.
- Create: `src/main.tsx` - React root mount.
- Create: `src/App.tsx` - Chinese recording control screen.
- Create: `src/lib/tauri.ts` - typed Tauri command wrapper.
- Create: `src/App.test.tsx` - UI behavior tests with mocked Tauri.
- Create: `src/test/setup.ts` - Vitest DOM setup.
- Create: `src-tauri/tauri.conf.json` - minimum Tauri configuration.
- Create: `src-tauri/src/lib.rs` - Tauri command registration.
- Create: `src-tauri/src/main.rs` - Tauri app entry.
- Create: `src-tauri/src/app/error.rs` - app error model.
- Create: `src-tauri/src/app/events.rs` - frontend event payloads.
- Create: `src-tauri/src/app/state_machine.rs` - recording state machine.
- Create: `src-tauri/src/app/recording_service.rs` - service orchestration over `ScreenCapture`.
- Create: `src-tauri/src/app/permission_service.rs` - permission status API.
- Create: `src-tauri/src/app/mod.rs` - app module exports.
- Create: `src-tauri/src/core/capture.rs` - capture trait and sinks.
- Create: `src-tauri/src/core/config.rs` - recording configuration.
- Create: `src-tauri/src/core/frame.rs` - timestamp and frame types.
- Create: `src-tauri/src/core/mod.rs` - core module exports.
- Create: `src-tauri/src/platform/macos/screen_capture_kit.rs` - macOS capture boundary.
- Create: `src-tauri/src/platform/macos/permissions.rs` - macOS permission boundary.
- Create: `src-tauri/src/platform/macos/mod.rs` - macOS module exports.
- Create: `src-tauri/src/platform/mod.rs` - platform module exports.
- Modify: `.gitignore` - ignore env files and build output.
- Modify: `HANDOFF.md` - record Phase 1 plan path after this plan is written.

---

### Task 1: Scaffold Tauri 2 + React TypeScript Shell

**Files:**
- Create: `package.json`
- Create: `index.html`
- Create: `vite.config.ts`
- Create: `tsconfig.json`
- Create: `tsconfig.node.json`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Modify: `.gitignore`

- [ ] **Step 1: Generate the scaffold from the official Tauri command**

Run:

```bash
npm create tauri-app@latest luzhi
```

Interactive choices:

```text
Package manager: npm
UI template: React
UI flavor: TypeScript
Identifier: com.luzhi.app
```

Expected:

```text
Project created
```

- [ ] **Step 2: Copy scaffold files into the repository root**

Run:

```bash
cp -R luzhi/src ./src
cp -R luzhi/src-tauri ./src-tauri
cp luzhi/package.json ./package.json
cp luzhi/index.html ./index.html
cp luzhi/vite.config.ts ./vite.config.ts
cp luzhi/tsconfig.json ./tsconfig.json
```

Expected:

```text
No output; files exist in repository root
```

- [ ] **Step 3: Install Tailwind for Vite**

Run:

```bash
npm install tailwindcss @tailwindcss/vite
```

Expected:

```text
Packages installed and package-lock.json updated
```

- [ ] **Step 4: Configure Tailwind in `vite.config.ts`**

Use this content:

```ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
})
```

- [ ] **Step 5: Create `src/styles.css`**

```css
@import "tailwindcss";
```

- [ ] **Step 6: Stop for dependency review**

Open:

```bash
sed -n '1,220p' src-tauri/Cargo.toml
sed -n '1,220p' package.json
```

Expected:

```text
Human reviewer confirms generated Rust dependency versions before any Cargo.toml edits
```

- [ ] **Step 7: Replace `.gitignore` with project-safe ignores**

Use this content:

```gitignore
.DS_Store
node_modules/
dist/
src-tauri/target/
*.log
.env
.env.*
!.env.example
```

- [ ] **Step 8: Run scaffold validation**

Run:

```bash
npm install
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected:

```text
npm run build exits 0
cargo test exits 0
```

- [ ] **Step 9: Commit scaffold**

Run:

```bash
git add package.json package-lock.json index.html vite.config.ts tsconfig.json src src-tauri .gitignore
git commit -m "chore(core): 搭建Tauri录制应用脚手架"
```

Expected:

```text
Commit created
```

---

### Task 2: Add Rust Core Media Types and Capture Trait

**Files:**
- Create: `src-tauri/src/core/frame.rs`
- Create: `src-tauri/src/core/config.rs`
- Create: `src-tauri/src/core/capture.rs`
- Create: `src-tauri/src/core/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create `src-tauri/src/core/frame.rs`**

```rust
use std::sync::Arc;

/// Monotonic timestamp shared by video frames, audio chunks, and UI events.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct MediaTimestamp {
    pub nanos: u64,
}

impl MediaTimestamp {
    pub fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    Bgra8,
}

#[derive(Clone, Debug)]
pub enum FrameBuffer {
    Owned(Arc<[u8]>),
}

#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub timestamp: MediaTimestamp,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub buffer: FrameBuffer,
}

pub type VideoFrameRef = Arc<VideoFrame>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_orders_by_nanoseconds() {
        let early = MediaTimestamp::from_nanos(10);
        let late = MediaTimestamp::from_nanos(20);

        assert!(early < late);
    }
}
```

- [ ] **Step 2: Create `src-tauri/src/core/config.rs`**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode {
    FullScreen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureConfig {
    pub mode: CaptureMode,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl CaptureConfig {
    pub fn full_screen_1080p_30fps() -> Self {
        Self {
            mode: CaptureMode::FullScreen,
            width: 1920,
            height: 1080,
            fps: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_phase_one_config_targets_1080p() {
        let config = CaptureConfig::full_screen_1080p_30fps();

        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
        assert_eq!(config.fps, 30);
    }
}
```

- [ ] **Step 3: Create `src-tauri/src/core/capture.rs`**

```rust
use std::sync::mpsc::Sender;

use crate::app::error::AppResult;
use crate::core::config::CaptureConfig;
use crate::core::frame::VideoFrameRef;

pub type VideoFrameSink = Sender<VideoFrameRef>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureCapabilities {
    pub supports_full_screen: bool,
    pub supports_window: bool,
    pub supports_region: bool,
    pub supports_4k: bool,
}

impl CaptureCapabilities {
    pub fn phase_one_macos() -> Self {
        Self {
            supports_full_screen: true,
            supports_window: false,
            supports_region: false,
            supports_4k: false,
        }
    }
}

pub trait ScreenCapture: Send {
    fn start(&mut self, config: CaptureConfig, sink: VideoFrameSink) -> AppResult<()>;
    fn stop(&mut self) -> AppResult<()>;
    fn capabilities(&self) -> CaptureCapabilities;
}
```

- [ ] **Step 4: Create `src-tauri/src/core/mod.rs`**

```rust
pub mod capture;
pub mod config;
pub mod frame;
```

- [ ] **Step 5: Modify `src-tauri/src/lib.rs` and `src-tauri/src/main.rs` to expose modules**

Use this baseline:

```rust
pub mod app;
pub mod core;
pub mod platform;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .run(tauri::generate_context!())?;

    Ok(())
}
```

Use this `src-tauri/src/main.rs`:

```rust
fn main() {
    if let Err(error) = luzhi_lib::run() {
        eprintln!("录智启动失败：{error}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 6: Run Rust unit tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml core::
```

Expected:

```text
2 passed
```

- [ ] **Step 7: Commit core types**

Run:

```bash
git add src-tauri/src/core src-tauri/src/lib.rs src-tauri/src/main.rs
git commit -m "feat(core): 定义录制核心数据结构"
```

Expected:

```text
Commit created
```

---

### Task 3: Add Application Error Model

**Files:**
- Create: `src-tauri/src/app/error.rs`
- Create: `src-tauri/src/app/mod.rs`

- [ ] **Step 1: Create `src-tauri/src/app/error.rs`**

```rust
use std::fmt::{Display, Formatter};

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AppError {
    InvalidState {
        current: &'static str,
        action: &'static str,
    },
    PermissionDenied {
        permission: &'static str,
    },
    NativeCaptureUnavailable {
        reason: &'static str,
    },
    CaptureFailed {
        reason: String,
    },
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::InvalidState { current, action } => {
                write!(formatter, "当前状态 {current} 不允许执行 {action}")
            }
            AppError::PermissionDenied { permission } => {
                write!(formatter, "缺少系统权限：{permission}")
            }
            AppError::NativeCaptureUnavailable { reason } => {
                write!(formatter, "当前录制能力不可用：{reason}")
            }
            AppError::CaptureFailed { reason } => {
                write!(formatter, "录制失败：{reason}")
            }
        }
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_error_uses_chinese_message() {
        let error = AppError::PermissionDenied {
            permission: "屏幕录制",
        };

        assert_eq!(error.to_string(), "缺少系统权限：屏幕录制");
    }
}
```

- [ ] **Step 2: Create `src-tauri/src/app/mod.rs`**

```rust
pub mod error;
```

- [ ] **Step 3: Run error tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::error
```

Expected:

```text
1 passed
```

- [ ] **Step 4: Commit error model**

Run:

```bash
git add src-tauri/src/app
git commit -m "feat(core): 增加应用错误模型"
```

Expected:

```text
Commit created
```

---

### Task 4: Add Recording State Machine with TDD

**Files:**
- Create: `src-tauri/src/app/state_machine.rs`
- Modify: `src-tauri/src/app/mod.rs`

- [ ] **Step 1: Create failing state machine tests**

Create `src-tauri/src/app/state_machine.rs` with tests first:

```rust
use crate::app::error::{AppError, AppResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingState {
    Idle,
    Recording,
    Processing,
    Completed,
    Failed,
}

impl RecordingState {
    pub fn as_str(self) -> &'static str {
        match self {
            RecordingState::Idle => "idle",
            RecordingState::Recording => "recording",
            RecordingState::Processing => "processing",
            RecordingState::Completed => "completed",
            RecordingState::Failed => "failed",
        }
    }
}

#[derive(Debug)]
pub struct RecordingStateMachine {
    state: RecordingState,
}

impl RecordingStateMachine {
    pub fn new() -> Self {
        Self {
            state: RecordingState::Idle,
        }
    }

    pub fn state(&self) -> RecordingState {
        self.state
    }

    pub fn start(&mut self) -> AppResult<()> {
        match self.state {
            RecordingState::Idle | RecordingState::Completed | RecordingState::Failed => {
                self.state = RecordingState::Recording;
                Ok(())
            }
            _ => Err(AppError::InvalidState {
                current: self.state.as_str(),
                action: "start",
            }),
        }
    }

    pub fn stop(&mut self) -> AppResult<()> {
        match self.state {
            RecordingState::Recording => {
                self.state = RecordingState::Processing;
                Ok(())
            }
            _ => Err(AppError::InvalidState {
                current: self.state.as_str(),
                action: "stop",
            }),
        }
    }

    pub fn complete(&mut self) -> AppResult<()> {
        match self.state {
            RecordingState::Processing => {
                self.state = RecordingState::Completed;
                Ok(())
            }
            _ => Err(AppError::InvalidState {
                current: self.state.as_str(),
                action: "complete",
            }),
        }
    }

    pub fn fail(&mut self) {
        self.state = RecordingState::Failed;
    }
}

impl Default for RecordingStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_machine_starts_idle() {
        let machine = RecordingStateMachine::new();

        assert_eq!(machine.state(), RecordingState::Idle);
    }

    #[test]
    fn can_start_stop_and_complete_recording() {
        let mut machine = RecordingStateMachine::new();

        machine.start().unwrap();
        assert_eq!(machine.state(), RecordingState::Recording);

        machine.stop().unwrap();
        assert_eq!(machine.state(), RecordingState::Processing);

        machine.complete().unwrap();
        assert_eq!(machine.state(), RecordingState::Completed);
    }

    #[test]
    fn cannot_stop_when_idle() {
        let mut machine = RecordingStateMachine::new();

        let error = machine.stop().unwrap_err();

        assert_eq!(
            error,
            AppError::InvalidState {
                current: "idle",
                action: "stop"
            }
        );
    }

    #[test]
    fn failed_state_can_start_again() {
        let mut machine = RecordingStateMachine::new();
        machine.fail();

        machine.start().unwrap();

        assert_eq!(machine.state(), RecordingState::Recording);
    }
}
```

- [ ] **Step 2: Export state machine module**

Modify `src-tauri/src/app/mod.rs`:

```rust
pub mod error;
pub mod state_machine;
```

- [ ] **Step 3: Run state machine tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::state_machine
```

Expected:

```text
4 passed
```

- [ ] **Step 4: Commit state machine**

Run:

```bash
git add src-tauri/src/app/state_machine.rs src-tauri/src/app/mod.rs
git commit -m "feat(record): 增加录制状态机"
```

Expected:

```text
Commit created
```

---

### Task 5: Add Recording Service over Mockable ScreenCapture

**Files:**
- Create: `src-tauri/src/app/recording_service.rs`
- Modify: `src-tauri/src/app/mod.rs`

- [ ] **Step 1: Create `src-tauri/src/app/recording_service.rs`**

```rust
use std::sync::mpsc::{channel, Receiver};

use crate::app::error::AppResult;
use crate::app::state_machine::{RecordingState, RecordingStateMachine};
use crate::core::capture::ScreenCapture;
use crate::core::config::CaptureConfig;
use crate::core::frame::VideoFrameRef;

pub struct RecordingService<C: ScreenCapture> {
    capture: C,
    state_machine: RecordingStateMachine,
    frame_receiver: Option<Receiver<VideoFrameRef>>,
}

impl<C: ScreenCapture> RecordingService<C> {
    pub fn new(capture: C) -> Self {
        Self {
            capture,
            state_machine: RecordingStateMachine::new(),
            frame_receiver: None,
        }
    }

    pub fn state(&self) -> RecordingState {
        self.state_machine.state()
    }

    pub fn start(&mut self, config: CaptureConfig) -> AppResult<()> {
        self.state_machine.start()?;
        let (sender, receiver) = channel();
        self.capture.start(config, sender)?;
        self.frame_receiver = Some(receiver);
        Ok(())
    }

    pub fn stop(&mut self) -> AppResult<()> {
        self.capture.stop()?;
        self.state_machine.stop()?;
        self.state_machine.complete()?;
        self.frame_receiver = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Sender;

    use super::*;
    use crate::app::error::AppError;
    use crate::core::capture::{CaptureCapabilities, VideoFrameSink};

    #[derive(Default)]
    struct MockScreenCapture {
        started: bool,
        stopped: bool,
        fail_on_start: bool,
        sink_seen: Option<Sender<VideoFrameRef>>,
    }

    impl ScreenCapture for MockScreenCapture {
        fn start(&mut self, _config: CaptureConfig, sink: VideoFrameSink) -> AppResult<()> {
            if self.fail_on_start {
                return Err(AppError::CaptureFailed {
                    reason: "mock start failure".to_string(),
                });
            }
            self.started = true;
            self.sink_seen = Some(sink);
            Ok(())
        }

        fn stop(&mut self) -> AppResult<()> {
            self.stopped = true;
            Ok(())
        }

        fn capabilities(&self) -> CaptureCapabilities {
            CaptureCapabilities::phase_one_macos()
        }
    }

    #[test]
    fn start_moves_service_to_recording() {
        let capture = MockScreenCapture::default();
        let mut service = RecordingService::new(capture);

        service.start(CaptureConfig::full_screen_1080p_30fps()).unwrap();

        assert_eq!(service.state(), RecordingState::Recording);
        assert!(service.frame_receiver.is_some());
    }

    #[test]
    fn stop_moves_service_to_completed() {
        let capture = MockScreenCapture::default();
        let mut service = RecordingService::new(capture);

        service.start(CaptureConfig::full_screen_1080p_30fps()).unwrap();
        service.stop().unwrap();

        assert_eq!(service.state(), RecordingState::Completed);
        assert!(service.frame_receiver.is_none());
    }
}
```

- [ ] **Step 2: Export recording service module**

Modify `src-tauri/src/app/mod.rs`:

```rust
pub mod error;
pub mod recording_service;
pub mod state_machine;
```

- [ ] **Step 3: Run recording service tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::recording_service
```

Expected:

```text
2 passed
```

- [ ] **Step 4: Commit recording service**

Run:

```bash
git add src-tauri/src/app/recording_service.rs src-tauri/src/app/mod.rs
git commit -m "feat(record): 增加录制服务编排"
```

Expected:

```text
Commit created
```

---

### Task 6: Add Permission Service Boundary

**Files:**
- Create: `src-tauri/src/app/permission_service.rs`
- Create: `src-tauri/src/platform/mod.rs`
- Create: `src-tauri/src/platform/macos/mod.rs`
- Create: `src-tauri/src/platform/macos/permissions.rs`
- Modify: `src-tauri/src/app/mod.rs`

- [ ] **Step 1: Create `src-tauri/src/app/permission_service.rs`**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingPermissions {
    pub screen_recording: PermissionStatus,
    pub microphone: PermissionStatus,
}

pub trait PermissionProbe: Send + Sync {
    fn recording_permissions(&self) -> RecordingPermissions;
}

pub struct PermissionService<P: PermissionProbe> {
    probe: P,
}

impl<P: PermissionProbe> PermissionService<P> {
    pub fn new(probe: P) -> Self {
        Self { probe }
    }

    pub fn recording_permissions(&self) -> RecordingPermissions {
        self.probe.recording_permissions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct GrantedProbe;

    impl PermissionProbe for GrantedProbe {
        fn recording_permissions(&self) -> RecordingPermissions {
            RecordingPermissions {
                screen_recording: PermissionStatus::Granted,
                microphone: PermissionStatus::Granted,
            }
        }
    }

    #[test]
    fn service_returns_probe_permissions() {
        let service = PermissionService::new(GrantedProbe);

        assert_eq!(
            service.recording_permissions(),
            RecordingPermissions {
                screen_recording: PermissionStatus::Granted,
                microphone: PermissionStatus::Granted,
            }
        );
    }
}
```

- [ ] **Step 2: Create `src-tauri/src/platform/macos/permissions.rs`**

```rust
use crate::app::permission_service::{
    PermissionProbe, PermissionStatus, RecordingPermissions,
};

pub struct MacPermissionProbe;

impl PermissionProbe for MacPermissionProbe {
    fn recording_permissions(&self) -> RecordingPermissions {
        RecordingPermissions {
            screen_recording: PermissionStatus::Unknown,
            microphone: PermissionStatus::Unknown,
        }
    }
}
```

- [ ] **Step 3: Create platform module exports**

Create `src-tauri/src/platform/mod.rs`:

```rust
#[cfg(target_os = "macos")]
pub mod macos;
```

Create `src-tauri/src/platform/macos/mod.rs`:

```rust
pub mod permissions;
pub mod screen_capture_kit;
```

- [ ] **Step 4: Export permission service**

Modify `src-tauri/src/app/mod.rs`:

```rust
pub mod error;
pub mod permission_service;
pub mod recording_service;
pub mod state_machine;
```

- [ ] **Step 5: Run permission tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::permission_service
```

Expected:

```text
1 passed
```

- [ ] **Step 6: Commit permission boundary**

Run:

```bash
git add src-tauri/src/app/permission_service.rs src-tauri/src/app/mod.rs src-tauri/src/platform
git commit -m "feat(record): 增加录制权限检测边界"
```

Expected:

```text
Commit created
```

---

### Task 7: Add macOS ScreenCaptureKit Boundary

**Files:**
- Create: `src-tauri/src/platform/macos/screen_capture_kit.rs`

- [ ] **Step 1: Create `src-tauri/src/platform/macos/screen_capture_kit.rs`**

```rust
use crate::app::error::{AppError, AppResult};
use crate::core::capture::{CaptureCapabilities, ScreenCapture, VideoFrameSink};
use crate::core::config::CaptureConfig;

pub struct MacScreenCapture {
    running: bool,
}

impl MacScreenCapture {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for MacScreenCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenCapture for MacScreenCapture {
    fn start(&mut self, _config: CaptureConfig, _sink: VideoFrameSink) -> AppResult<()> {
        self.running = true;
        Err(AppError::NativeCaptureUnavailable {
            reason: "ScreenCaptureKit native callback must pass human review before activation",
        })
    }

    fn stop(&mut self) -> AppResult<()> {
        self.running = false;
        Ok(())
    }

    fn capabilities(&self) -> CaptureCapabilities {
        CaptureCapabilities::phase_one_macos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn mac_capture_exposes_phase_one_capabilities() {
        let capture = MacScreenCapture::new();
        let capabilities = capture.capabilities();

        assert!(capabilities.supports_full_screen);
        assert!(!capabilities.supports_window);
        assert!(!capabilities.supports_region);
    }

    #[test]
    fn mac_capture_requires_human_reviewed_native_activation() {
        let mut capture = MacScreenCapture::new();
        let (sender, _receiver) = channel();

        let error = capture
            .start(CaptureConfig::full_screen_1080p_30fps(), sender)
            .unwrap_err();

        assert_eq!(
            error,
            AppError::NativeCaptureUnavailable {
                reason: "ScreenCaptureKit native callback must pass human review before activation",
            }
        );
    }
}
```

- [ ] **Step 2: Run macOS boundary tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml platform::macos::screen_capture_kit
```

Expected:

```text
2 passed
```

- [ ] **Step 3: Human native capture checkpoint**

Before replacing the guarded `start()` body with ScreenCaptureKit code, human reviewer must confirm:

```text
1. Buffer ownership is documented.
2. Callback thread handoff is documented.
3. Stop path releases stream/session resources.
4. No callback can send frames after stop completes.
5. Every unsafe/FFI call has a local safety explanation.
6. Tests still use MockScreenCapture and never call real system recording APIs.
```

Expected:

```text
Human reviewer signs off before native ScreenCaptureKit activation
```

- [ ] **Step 4: Commit macOS capture boundary**

Run:

```bash
git add src-tauri/src/platform/macos/screen_capture_kit.rs
git commit -m "feat(record): 增加macOS录制适配边界"
```

Expected:

```text
Commit created
```

---

### Task 8: Add Tauri Commands for Recording Status

**Files:**
- Create: `src-tauri/src/app/events.rs`
- Modify: `src-tauri/src/app/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create `src-tauri/src/app/events.rs`**

```rust
use serde::Serialize;

use crate::app::permission_service::{PermissionStatus, RecordingPermissions};
use crate::app::state_machine::RecordingState;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatusPayload {
    pub state: &'static str,
    pub can_start: bool,
}

impl From<RecordingState> for RecordingStatusPayload {
    fn from(state: RecordingState) -> Self {
        Self {
            state: state.as_str(),
            can_start: matches!(
                state,
                RecordingState::Idle | RecordingState::Completed | RecordingState::Failed
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPayload {
    pub screen_recording: PermissionStatusPayload,
    pub microphone: PermissionStatusPayload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionStatusPayload {
    Granted,
    Denied,
    NotDetermined,
    Unknown,
}

impl From<PermissionStatus> for PermissionStatusPayload {
    fn from(status: PermissionStatus) -> Self {
        match status {
            PermissionStatus::Granted => PermissionStatusPayload::Granted,
            PermissionStatus::Denied => PermissionStatusPayload::Denied,
            PermissionStatus::NotDetermined => PermissionStatusPayload::NotDetermined,
            PermissionStatus::Unknown => PermissionStatusPayload::Unknown,
        }
    }
}

impl From<RecordingPermissions> for PermissionPayload {
    fn from(permissions: RecordingPermissions) -> Self {
        Self {
            screen_recording: permissions.screen_recording.into(),
            microphone: permissions.microphone.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_status_can_start() {
        let payload = RecordingStatusPayload::from(RecordingState::Idle);

        assert_eq!(payload.state, "idle");
        assert!(payload.can_start);
    }

    #[test]
    fn recording_status_cannot_start() {
        let payload = RecordingStatusPayload::from(RecordingState::Recording);

        assert_eq!(payload.state, "recording");
        assert!(!payload.can_start);
    }
}
```

- [ ] **Step 2: Export events module**

Modify `src-tauri/src/app/mod.rs`:

```rust
pub mod error;
pub mod events;
pub mod permission_service;
pub mod recording_service;
pub mod state_machine;
```

- [ ] **Step 3: Modify `src-tauri/src/lib.rs`**

```rust
pub mod app;
pub mod core;
pub mod platform;

use app::events::{PermissionPayload, RecordingStatusPayload};
use app::permission_service::{PermissionService, RecordingPermissions};
use app::state_machine::RecordingState;

#[tauri::command]
fn recording_status() -> RecordingStatusPayload {
    RecordingStatusPayload::from(RecordingState::Idle)
}

#[tauri::command]
fn recording_permissions() -> PermissionPayload {
    let permissions = RecordingPermissions {
        screen_recording: app::permission_service::PermissionStatus::Unknown,
        microphone: app::permission_service::PermissionStatus::Unknown,
    };
    PermissionPayload::from(permissions)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            recording_status,
            recording_permissions
        ])
        .run(tauri::generate_context!())?;

    Ok(())
}
```

- [ ] **Step 4: Run command payload tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml app::events
```

Expected:

```text
2 passed
```

- [ ] **Step 5: Commit Tauri commands**

Run:

```bash
git add src-tauri/src/app/events.rs src-tauri/src/app/mod.rs src-tauri/src/lib.rs
git commit -m "feat(record): 增加录制状态命令"
```

Expected:

```text
Commit created
```

---

### Task 9: Add Minimal Chinese Recording UI

**Files:**
- Create: `src/lib/tauri.ts`
- Modify: `src/App.tsx`
- Modify: `src/main.tsx`

- [ ] **Step 1: Create `src/lib/tauri.ts`**

```ts
import { invoke } from '@tauri-apps/api/core'

export type RecordingStatus = {
  state: 'idle' | 'recording' | 'processing' | 'completed' | 'failed'
  canStart: boolean
}

export type RecordingPermissions = {
  screenRecording: 'granted' | 'denied' | 'notDetermined' | 'unknown'
  microphone: 'granted' | 'denied' | 'notDetermined' | 'unknown'
}

export async function fetchRecordingStatus(): Promise<RecordingStatus> {
  return invoke<RecordingStatus>('recording_status')
}

export async function fetchRecordingPermissions(): Promise<RecordingPermissions> {
  return invoke<RecordingPermissions>('recording_permissions')
}
```

- [ ] **Step 2: Replace `src/App.tsx`**

```tsx
import { useEffect, useState } from 'react'
import {
  fetchRecordingPermissions,
  fetchRecordingStatus,
  type RecordingPermissions,
  type RecordingStatus,
} from './lib/tauri'

const STATUS_LABELS: Record<RecordingStatus['state'], string> = {
  idle: '待录制',
  recording: '录制中',
  processing: '处理中',
  completed: '已完成',
  failed: '录制失败',
}

function permissionLabel(value: RecordingPermissions[keyof RecordingPermissions]) {
  if (value === 'granted') return '已授权'
  if (value === 'denied') return '未授权'
  if (value === 'notDetermined') return '待确认'
  return '未知'
}

export default function App() {
  const [status, setStatus] = useState<RecordingStatus>({
    state: 'idle',
    canStart: true,
  })
  const [permissions, setPermissions] = useState<RecordingPermissions>({
    screenRecording: 'unknown',
    microphone: 'unknown',
  })

  useEffect(() => {
    void fetchRecordingStatus().then(setStatus)
    void fetchRecordingPermissions().then(setPermissions)
  }, [])

  return (
    <main className="min-h-screen bg-neutral-950 text-neutral-50">
      <section className="mx-auto flex min-h-screen w-full max-w-5xl flex-col gap-6 px-6 py-8">
        <header className="flex items-center justify-between border-b border-neutral-800 pb-4">
          <div>
            <h1 className="text-2xl font-semibold">录智</h1>
            <p className="mt-1 text-sm text-neutral-400">录屏、美化、导出</p>
          </div>
          <span className="rounded bg-neutral-800 px-3 py-1 text-sm">
            {STATUS_LABELS[status.state]}
          </span>
        </header>

        <div className="grid gap-4 md:grid-cols-3">
          <button
            className="rounded border border-neutral-700 bg-neutral-900 px-4 py-3 text-left hover:border-neutral-500"
            type="button"
          >
            全屏录制
          </button>
          <button
            className="rounded border border-neutral-800 bg-neutral-900/60 px-4 py-3 text-left text-neutral-500"
            type="button"
            disabled
          >
            窗口录制
          </button>
          <button
            className="rounded border border-neutral-800 bg-neutral-900/60 px-4 py-3 text-left text-neutral-500"
            type="button"
            disabled
          >
            区域录制
          </button>
        </div>

        <section className="grid gap-3 text-sm text-neutral-300 md:grid-cols-2">
          <div className="rounded border border-neutral-800 p-4">
            屏幕录制权限：{permissionLabel(permissions.screenRecording)}
          </div>
          <div className="rounded border border-neutral-800 p-4">
            麦克风权限：{permissionLabel(permissions.microphone)}
          </div>
        </section>

        <button
          className="w-fit rounded bg-emerald-500 px-5 py-2 font-medium text-neutral-950 disabled:bg-neutral-700 disabled:text-neutral-400"
          type="button"
          disabled={!status.canStart}
        >
          开始录制
        </button>
      </section>
    </main>
  )
}
```

- [ ] **Step 3: Replace `src/main.tsx`**

```tsx
import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './styles.css'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
```

- [ ] **Step 4: Run UI build**

Run:

```bash
npm run build
```

Expected:

```text
Build exits 0
```

- [ ] **Step 5: Commit UI**

Run:

```bash
git add src
git commit -m "feat(ui): 增加中文录制入口"
```

Expected:

```text
Commit created
```

---

### Task 10: Add Frontend Tests for Recording Screen

**Files:**
- Create: `src/test/setup.ts`
- Create: `src/App.test.tsx`
- Modify: `vite.config.ts`

- [ ] **Step 1: Install frontend test dependencies**

Run:

```bash
npm install -D vitest @testing-library/react @testing-library/jest-dom @testing-library/user-event jsdom
```

Expected:

```text
Packages installed and package-lock.json updated
```

- [ ] **Step 2: Modify `vite.config.ts`**

```ts
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  test: {
    environment: 'jsdom',
    setupFiles: './src/test/setup.ts',
  },
})
```

- [ ] **Step 3: Create `src/test/setup.ts`**

```ts
import '@testing-library/jest-dom/vitest'
```

- [ ] **Step 4: Create `src/App.test.tsx`**

```tsx
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'

const invokeMock = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string) => invokeMock(command),
}))

describe('App', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') {
        return Promise.resolve({ state: 'idle', canStart: true })
      }
      if (command === 'recording_permissions') {
        return Promise.resolve({
          screenRecording: 'unknown',
          microphone: 'unknown',
        })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })
  })

  it('renders Chinese recording controls', async () => {
    render(<App />)

    expect(await screen.findByText('录智')).toBeInTheDocument()
    expect(screen.getByText('全屏录制')).toBeInTheDocument()
    expect(screen.getByText('开始录制')).toBeInTheDocument()
  })

  it('requests status and permissions from Tauri', async () => {
    render(<App />)

    await screen.findByText('待录制')

    expect(invokeMock).toHaveBeenCalledWith('recording_status')
    expect(invokeMock).toHaveBeenCalledWith('recording_permissions')
  })
})
```

- [ ] **Step 5: Add test script to `package.json`**

Ensure scripts include:

```json
{
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "test": "vitest run"
  }
}
```

- [ ] **Step 6: Run frontend tests**

Run:

```bash
npm test
```

Expected:

```text
2 passed
```

- [ ] **Step 7: Commit frontend tests**

Run:

```bash
git add package.json package-lock.json vite.config.ts src/App.test.tsx src/test/setup.ts
git commit -m "test(ui): 覆盖录制入口界面"
```

Expected:

```text
Commit created
```

---

### Task 11: Wire Phase 1 Verification and Handoff

**Files:**
- Modify: `HANDOFF.md`
- Read: `tests/phase-1-w1-w2-checklist.md`

- [ ] **Step 1: Run Rust verification**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Expected:

```text
All commands exit 0
```

- [ ] **Step 2: Run frontend verification**

Run:

```bash
npm test
npm run build
```

Expected:

```text
Both commands exit 0
```

- [ ] **Step 3: Update `HANDOFF.md`**

Append this entry under `## 工作任务记录`:

```markdown
### 2026-05-23：Phase 1 实施计划

已生成实施计划：

- `docs/superpowers/plans/2026-05-23-phase-1-macos-recording-foundation.md`

执行要求：

1. 按任务顺序执行，每个任务完成后提交一次。
2. 执行前确认 `src-tauri/Cargo.toml` 依赖版本经过人工审查。
3. ScreenCaptureKit 原生回调和 FFI 代码必须人工逐行审查。
4. Phase 1 完成后逐项执行 `tests/phase-1-w1-w2-checklist.md`。
```

- [ ] **Step 4: Run final documentation checks**

Run:

```bash
rg -n "T[O]DO|T[B]D|F[I]XME|待[定]|占[位]|\\(填[写]|后续需[要]" docs/superpowers/plans/2026-05-23-phase-1-macos-recording-foundation.md HANDOFF.md
git diff --check
```

Expected:

```text
rg returns no matches
git diff --check exits 0
```

- [ ] **Step 5: Commit plan handoff**

Run:

```bash
git add HANDOFF.md docs/superpowers/plans/2026-05-23-phase-1-macos-recording-foundation.md
git commit -m "docs(core): 编写Phase1实施计划"
```

Expected:

```text
Commit created
```

---

## Self-Review Checklist

- Spec coverage: Phase 1 scaffold, Rust boundaries, state machine, permission service, mockable capture service, Tauri commands, Chinese UI, tests, and handoff are covered.
- Scope boundary: Windows, audio mixing, cursor effects, blank trimming, export presets, licensing, telemetry, and publishing APIs are excluded.
- Type consistency: `RecordingState`, `RecordingStatusPayload`, `CaptureConfig`, `ScreenCapture`, `VideoFrameRef`, and `PermissionStatus` names are used consistently.
- Safety: `Cargo.toml` dependency changes and ScreenCaptureKit native callback activation both have explicit human gates.
- Testing: Rust unit tests, frontend Vitest tests, build checks, clippy, formatting, and Phase 1 manual checklist are specified.
