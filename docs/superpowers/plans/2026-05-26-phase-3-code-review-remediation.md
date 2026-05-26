# Phase 3 Code Review Remediation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 Phase 3 首版代码审查发现的并发状态、线程生命周期、权限测试隔离和测试覆盖缺口，确保录制 UI 与前后端联调达到可合并标准。

**Architecture:** 前端继续只负责命令下发和展示状态，但需要增加“启动中”防重入边界，避免 UI 与 Rust 录制状态机脱节。Rust 端继续由 `MacRecordingService` 维护录制主链路，新增的麦克风电平事件推送必须拥有可停止、可 join 的生命周期管理。权限探测保留真实 macOS API，但普通单测必须改为纯函数映射测试，避免调用系统权限 API。

**Tech Stack:** Tauri 2, React + TypeScript, Rust, ScreenCaptureKit, AVFoundation/CoreGraphics FFI, Vitest, React Testing Library, Cargo test/clippy/fmt.

---

## 0. 审查结论

Phase 3 首版主体功能已落地，但当前状态不建议合并。

已完成的主要交付：

- `CaptureMode` 已扩展 `FullScreen` / `Window` / `Area`，后端能解析三种模式。
- `set_capture_mode` 已接受窗口/区域参数，`start_recording` 对非全屏返回中文错误。
- `MacPermissionProbe` 已从 stub 改为真实 macOS 权限探测。
- `MicLevelDetector` 已实现 RMS 计算。
- 消费线程已从麦克风 `AudioChunk` 更新共享电平值。
- 前端已移除随机麦克风音量模拟，改监听 `mic-level` 事件。
- `RecordingPanel` 已加入分辨率/FPS 选择、窗口/区域“即将推出”提示和按钮禁用。
- 自动化验证已通过：Rust 69 tests、前端 14 tests。

阻塞合并的问题：

1. 前端快速重复点击“开始录制”仍可能触发多组启动命令，导致前端进入失败态但后端仍可能录制。
2. `mic-level` 推送线程没有保存 `JoinHandle`，停止时只设置 flag，不 join，资源释放路径不可证明。
3. 未启用麦克风时仍启动 `mic-level` 推送线程，且新录制未重置上一次电平值。
4. 权限测试直接调用真实 macOS 权限 API，违反测试规则。
5. Phase 3 计划要求的若干测试存在缺口或假覆盖。

---

## 1. 审查输入与验证记录

审查输入：

- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/2026-05-26-phase-3-plan.md`
- `tests/phase-3-w5-w6-checklist.md`
- `HANDOFF.md`
- `BUG.md`
- `.codex/rules/0-global.md`
- `.codex/rules/1-coding-style.md`
- `.codex/rules/2-testing.md`
- `.codex/rules/4-security.md`

审查范围：

- 当前分支：`feat/architecture-planning`
- 审查基线：`HEAD = db3ced097417eef6d6d6b73408a5aef287ae68ba`
- 审查对象：Phase 3 工作区未提交改动，包含 untracked 文件：
  - `docs/superpowers/plans/2026-05-26-phase-3-plan.md`
  - `src-tauri/src/media/mic_level.rs`

已执行验证：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

验证结果：

- `cargo fmt --check`：通过。
- `cargo test`：69 passed。
- `cargo clippy --all-targets`：无 error，仍有 21 个既有 FFI naming / unused warning。
- `cargo build`：通过。
- `npm run build`：通过。
- `npm test -- --run`：14 passed。

注意：自动化通过不代表 Phase 3 可合并。本次审查发现的问题主要集中在并发状态边界、后台线程生命周期和测试真实性上，现有测试没有覆盖这些风险。

---

## 2. Critical Findings

### Critical 1: 快速重复点击开始录制仍可能破坏 UI/Rust 状态一致性

**文件：**

- `src/App.tsx:101`
- `src/App.test.tsx:119`

**问题：**

`handleStartRecording` 当前只检查：

```ts
if (appState !== 'idle') return
```

但点击开始后没有立即设置本地 pending 状态，也没有 `isStarting` 防重入标志。在 Rust `recording-state-changed` 事件回来前，React 的 `appState` 仍可能保持 `idle`，第二次点击会继续执行：

```ts
await setCaptureMode(...)
await setAudioConfig(...)
await startRecording()
```

如果第一次 `start_recording` 已经让 Rust 状态机进入 `Recording`，第二次 `start_recording` 可能返回 InvalidState。前端 catch 后会执行：

```ts
setAppState('failed')
setErrorMessage(String(e))
```

此时可能出现“后端仍在录制，前端却进入失败页”的脱节状态，用户看不到停止按钮，属于录制主链路可用性风险。

**为什么重要：**

- 这是用户很容易触发的真实交互路径。
- 可能导致捕获线程仍在运行，但 UI 失去停止入口。
- 违背 Phase 3 “快速点击开始/停止不会导致状态错乱”的稳定性验收项。

**当前测试缺口：**

`src/App.test.tsx:119` 的测试名为 `prevents double-click on start recording`，但测试实际只 click 了一次，因此是假的防重入覆盖。

**整改建议：**

增加本地启动锁，例如：

```ts
const [isStarting, setIsStarting] = useState(false)

const handleStartRecording = useCallback(async () => {
  if (appState !== 'idle' || isStarting) return
  if (recordingMode !== 'fullscreen') return

  setIsStarting(true)
  setAppState('processing')

  try {
    await setCaptureMode({
      mode: recordingMode,
      width: resolution.width,
      height: resolution.height,
      fps,
    })
    await setAudioConfig({
      captureSystemAudio: systemAudioEnabled,
      captureMicrophone: micEnabled,
      microphoneDevice: null,
      sampleRate: 48000,
      channels: 2,
    })
    await startRecording()
  } catch (e) {
    setAppState('failed')
    setErrorMessage(String(e))
    setIsStarting(false)
  }
}, [appState, isStarting, recordingMode, resolution, fps, systemAudioEnabled, micEnabled])
```

在 `recording-state-changed` 收到 `recording` / `failed` / `idle` 时清理 `isStarting`。如果使用 `processing` 作为本地 pending 状态，需确认 `ProcessingView` 是可接受的启动中视图。

---

## 3. Important Findings

### Important 1: `mic-level` 推送线程没有可 join 的生命周期管理

**文件：**

- `src-tauri/src/lib.rs:31`
- `src-tauri/src/lib.rs:137`
- `src-tauri/src/lib.rs:172`

**问题：**

当前 `AppState` 只保存：

```rust
mic_level_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
```

启动时直接：

```rust
std::thread::spawn(move || {
    while !mic_thread_stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
        ...
    }
});
```

停止时只执行：

```rust
mic_stop.store(true, Ordering::Relaxed);
```

没有保存 `JoinHandle`，因此不能确认线程已退出。

**为什么重要：**

- 不符合 Phase 3 计划“线程句柄存储到 AppState 中，在 stop_recording 时销毁该线程”的要求。
- 录制快速 stop/start 时，旧线程可能短时间继续 emit，和新录制的 mic-level 事件交错。
- 应用退出或异常路径下，资源释放路径不可证明。

**整改建议：**

复用 `TickRuntime` 的模式新增 `MicLevelRuntime`：

```rust
pub struct MicLevelRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MicLevelRuntime {
    pub fn spawn<F>(mut read_and_emit: F) -> Self
    where
        F: FnMut() + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                read_and_emit();
            }
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MicLevelRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}
```

然后将 `AppState` 改为：

```rust
mic_level_runtime: Arc<Mutex<Option<MicLevelRuntime>>>,
```

停止录制时和 tick runtime 一样 `take()` 后 `stop()`。

---

### Important 2: 未启用麦克风时仍发 `mic-level`，且录制开始未重置电平

**文件：**

- `src-tauri/src/lib.rs:126`
- `src-tauri/src/platform/macos_service.rs:51`
- `src-tauri/src/platform/macos_service.rs:123`

**问题：**

`start_recording` 总是启动 mic-level emission thread，即使 `audio_config.capture_microphone == false`。同时 `MacRecordingService::start()` 没有将 `mic_level` 重置为 `0.0`。

这会导致：

- 未启用麦克风时仍有 `mic-level` 事件。
- 下一次录制开始时可能继续使用上一次录制残留的电平值。
- UI 麦克风指示器可能显示过期数据。

**为什么重要：**

Phase 3 的核心目标之一是“真实麦克风电平反馈”。残留电平或无麦仍推送会让 UI 反馈不可信，也会影响手动验收。

**整改建议：**

在 `MacRecordingService::start()` 开头或消费线程启动前重置：

```rust
if let Ok(mut guard) = self.mic_level.lock() {
    *guard = 0.0;
}
```

在 `stop()` 结束时也重置：

```rust
if let Ok(mut guard) = self.mic_level.lock() {
    *guard = 0.0;
}
```

在 `start_recording` 中根据 `audio_config.capture_microphone` 决定是否启动 `MicLevelRuntime`。如果前端希望禁用麦克风时固定归零，也可以只发一次 `MicLevelPayload { level: 0.0 }`，但不应保留常驻线程。

---

### Important 3: 权限测试调用真实 macOS API，违反测试隔离规则

**文件：**

- `src-tauri/src/platform/macos/permissions.rs:80`

**问题：**

当前测试：

```rust
#[test]
fn permission_probe_returns_permissions() {
    let probe = MacPermissionProbe;
    let perms = probe.recording_permissions();
    ...
}
```

会直接触发：

- `CGPreflightScreenCaptureAccess()`
- `AVCaptureDevice.authorizationStatusForMediaType:`

**为什么重要：**

`.codex/rules/2-testing.md` 明确要求测试环境使用 Mock，不调用真实系统录屏 API。真实权限 API 会受本机 TCC 状态影响，CI 和开发机表现可能不同。

**整改建议：**

将权限状态映射拆成纯函数：

```rust
fn map_av_authorization_status(status: isize) -> PermissionStatus {
    match status {
        3 => PermissionStatus::Granted,
        2 => PermissionStatus::Denied,
        1 => PermissionStatus::Denied,
        0 => PermissionStatus::NotDetermined,
        _ => PermissionStatus::Unknown,
    }
}

fn map_screen_preflight(allowed: bool) -> PermissionStatus {
    if allowed {
        PermissionStatus::Granted
    } else {
        PermissionStatus::NotDetermined
    }
}
```

普通单测只测纯函数：

```rust
#[test]
fn maps_av_authorized_to_granted() {
    assert_eq!(map_av_authorization_status(3), PermissionStatus::Granted);
}

#[test]
fn maps_av_denied_to_denied() {
    assert_eq!(map_av_authorization_status(2), PermissionStatus::Denied);
}

#[test]
fn maps_av_restricted_to_denied() {
    assert_eq!(map_av_authorization_status(1), PermissionStatus::Denied);
}

#[test]
fn maps_av_not_determined() {
    assert_eq!(map_av_authorization_status(0), PermissionStatus::NotDetermined);
}

#[test]
fn maps_unknown_av_status() {
    assert_eq!(map_av_authorization_status(-1), PermissionStatus::Unknown);
}
```

真实 FFI 调用保留在生产路径，不放入普通单测。

---

### Important 4: Phase 3 测试覆盖存在缺口和假覆盖

**文件：**

- `src-tauri/src/app/events.rs:34`
- `src/lib/tauri.ts:106`
- `src/App.test.tsx:119`
- `src-tauri/src/platform/macos/permissions.rs:76`

**问题：**

Phase 3 计划明确要求：

- `MicLevelPayload` 序列化格式测试。
- 权限状态映射测试。
- `mic-level` 事件更新前端 micVolume。
- idle 状态下多次点击开始仅触发一次启动命令。
- recording 状态下多次点击停止仅触发一次停止命令。

当前状态：

- `MicLevelPayload` 没有序列化测试。
- 权限测试调用真实 API，而不是映射测试。
- `onMicLevel` / `App.tsx` 的 mic-level 事件没有有效前端测试。
- `prevents double-click on start recording` 没有双击。
- 停止按钮防重入没有测试。

**整改建议：**

补齐以下测试：

1. Rust：`MicLevelPayload` camelCase 序列化。
2. Rust：权限映射纯函数。
3. 前端：mock `listen('mic-level')`，手动触发 payload，断言麦克风条更新。
4. 前端：连续两次 click 开始按钮，断言 `start_recording` 只调用一次。
5. 前端：连续两次 click 停止按钮，断言 `stop_recording` 只调用一次。

---

## 4. Minor Findings

### Minor 1: `MicLevelDetector` 测试命名和断言不一致

**文件：**

- `src-tauri/src/media/mic_level.rs:100`

**问题：**

测试名：

```rust
fn half_amplitude_returns_around_half()
```

实际断言是半幅信号因 `REFERENCE_LEVEL = 0.3` 被 clamp 到 `1.0`。测试注释里还保留了自我修正过程，不符合代码风格里“无临时解释”的口径。

**整改建议：**

改名为：

```rust
fn half_amplitude_clamps_to_one_with_reference_level()
```

并精简注释，只保留必要说明。

---

## 5. Phase 3 完成度判定

按当前代码状态，Phase 3 首版是“主体完成，但未达到可合并完成”。

已满足：

- UI 中文主流程已完成。
- 前后端 invoke/event 主链路已完成。
- 音视频帧未进入 JS 层。
- Rust 仍是录制状态机唯一来源。
- 分辨率/FPS 参数已进入后端配置。
- 窗口/区域模式已预留，当前不可启动。
- 自动化验证全部通过。

未满足：

- 快速点击开始/停止的稳定性验收未被真实覆盖。
- `mic-level` 线程生命周期未按计划实现为可销毁 runtime。
- 未启用麦克风时的 `mic-level` 事件行为不正确。
- 权限测试违反“测试环境不调用真实系统 API”的规则。
- `tests/phase-3-w5-w6-checklist.md` 中仍有手动验证项未执行。
- Native Safety Gate 仍需人工逐行审查 `permissions.rs` 中 FFI。

---

## 6. 整改 Phase 拆分

### Phase A: 前端启动/停止防重入修复

**目标：** 防止 UI 与 Rust 录制状态机脱节。

**建议改动文件：**

- `src/App.tsx`
- `src/App.test.tsx`

**验收标准：**

- 连续点击开始 2 次，只触发 1 次 `start_recording`。
- 启动 pending 期间 UI 不再接受第二次启动。
- 启动失败后可以返回 idle 并再次启动。
- 连续点击停止 2 次，只触发 1 次 `stop_recording`。

### Phase B: `MicLevelRuntime` 生命周期修复

**目标：** 让 mic-level 推送线程可停止、可 join、可 Drop。

**建议改动文件：**

- `src-tauri/src/app/recording_runtime.rs` 或新建 `src-tauri/src/app/mic_level_runtime.rs`
- `src-tauri/src/app/mod.rs`
- `src-tauri/src/lib.rs`

**验收标准：**

- `AppState` 保存 runtime handle，而不是只保存 stop flag。
- `stop_recording` 中明确停止并 join runtime。
- 重复 start/stop 不产生旧线程继续 emit。

### Phase C: 麦克风关闭与电平重置修复

**目标：** 确保 mic-level 事件只代表当前录制的真实麦克风输入。

**建议改动文件：**

- `src-tauri/src/lib.rs`
- `src-tauri/src/platform/macos_service.rs`

**验收标准：**

- 每次 start 前 `mic_level` 重置为 `0.0`。
- stop 后 `mic_level` 重置为 `0.0`。
- `capture_microphone == false` 时不启动常驻 mic-level 推送线程。

### Phase D: 权限测试隔离与映射测试

**目标：** 保留真实权限 API，同时让普通单测不依赖系统权限状态。

**建议改动文件：**

- `src-tauri/src/platform/macos/permissions.rs`

**验收标准：**

- 普通单测不调用 `MacPermissionProbe::recording_permissions()`。
- 新增 AVFoundation 状态映射纯函数测试。
- 新增 screen preflight 映射纯函数测试。
- `cargo test` 不依赖当前机器权限状态。

### Phase E: 测试补齐与手动验证

**目标：** 让 Phase 3 checklist 中自动化项真实可信，并完成剩余手动验收。

**建议改动文件：**

- `src-tauri/src/app/events.rs`
- `src/App.test.tsx`
- `tests/phase-3-code-review-remediation-checklist.md`
- `tests/phase-3-w5-w6-checklist.md`（只在实际手动验证后更新）

**验收标准：**

- `MicLevelPayload` 序列化测试存在并通过。
- `mic-level` 前端事件测试存在并通过。
- 快速开始/停止测试真实覆盖双击或重复触发。
- 手动验证清单剩余项完成后再勾选原 Phase 3 checklist。

---

## 7. 必跑验证矩阵

每个整改 Phase 完成后至少运行对应最小验证。全部整改完成后运行完整矩阵：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

手动验证仍需执行：

```bash
npm run tauri dev
```

手动重点：

- 快速点击开始/停止 10 次，确认 UI 不进入错误状态且后端无悬挂录制。
- 麦克风关闭时 UI 电平保持 0 或不显示动态电平。
- 麦克风开启时对着麦克风说话，电平明显上升；停止后归零。
- 断开麦克风后开始录制，不崩溃并显示可理解错误。
- 权限关闭/开启后返回首页能重新检测。

---

## 8. 与 BUG.md 预防规则对照

本次审查未发现新增违反 `BUG.md` 已记录预防规则的改动：

- 未新增区域级 `data-tauri-drag-region="false"`。
- `ErrorView` 未恢复 false drag-region 标记。
- 未使用 `motion.div` 包裹主开始录制 `Button` 造成点击被吞。

整改时仍需继续遵守：

- 不要用 `{false}` 做区域级拖拽排除。
- 可交互元素的按压反馈优先使用 CSS `active:` 或直接作用于按钮本身。
- 每个状态视图都要保留可拖拽区域，同时排除按钮、输入框、select 等交互元素。

---

## 9. 合并门槛

整改后满足以下条件，才建议进入合并或下一阶段：

- Critical 1 已修复并有真实双击测试。
- Important 1-4 已修复或有明确人工接受记录。
- 完整验证矩阵全部通过。
- `npm run tauri dev` 手动验证完成并更新 `tests/phase-3-w5-w6-checklist.md`。
- `permissions.rs` FFI 已完成人工 Native Safety Gate 审查。
- 没有新增 `allow-all`、生产配置修改、FFmpeg CLI 拼接、音视频帧进入 JS 层。

---

## 10. Round 2 整改后复审结果（2026-05-26）

### 10.1 复审结论

第一轮整改已经解决了首版审查中的一批核心问题：

- 前端已新增 `isStartingRef` / `isStoppingRef` 防重入守卫。
- Rust 已新增 `MicLevelRuntime`，mic-level 推送线程具备 stop / join / Drop 生命周期。
- `capture_microphone == false` 时不再启动常驻 mic-level runtime。
- `MacRecordingService::start()` / `stop()` 已重置共享 `mic_level`。
- 权限测试已改为纯函数映射测试，不再在普通单测中调用真实系统权限 API。
- 自动化验证全部通过。

但当前状态仍不建议合并。复审发现 1 个 Critical、3 个 Important，主要集中在：

1. 前端防重入锁仍完全依赖 `recording-state-changed` 事件清理，事件丢失时可能再次造成 UI / Rust 状态脱节。
2. 麦克风电平虽然在 Rust 侧归零，但前端没有可靠收到或主动清空，仍可能显示上一轮残留值。
3. 麦克风权限探测使用 `AVCaptureDevice`，但当前代码没有显式链接 `AVFoundation` framework，真实权限检测可能长期返回 `Unknown`。
4. Phase 3 整改测试清单仍未真实覆盖 `mic-level` payload 更新 UI 和麦克风关闭/停止后的归零行为。

### 10.2 复审输入

复审依据：

- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/2026-05-26-phase-3-plan.md`
- `docs/superpowers/plans/2026-05-26-phase-3-code-review-remediation.md`
- `tests/phase-3-code-review-remediation-checklist.md`
- `tests/phase-3-w5-w6-checklist.md`
- `HANDOFF.md`
- `BUG.md`
- `.codex/rules/0-global.md`
- `.codex/rules/1-coding-style.md`
- `.codex/rules/2-testing.md`
- `.codex/rules/4-security.md`
- `.codex/rules/5-docs.md`

复审范围：

- 当前 `HEAD`: `db3ced097417eef6d6d6b73408a5aef287ae68ba`
- 审查对象：Phase 3 相关工作区未提交改动与 untracked 文件。
- 并行子审查：已按 `superpowers:requesting-code-review` 尝试派发只读审查子任务，但该子任务因 `502 Bad Gateway` 失败；以下结论来自本地逐文件审查和验证。

### 10.3 已执行验证

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml`：76 passed。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`：无 error，仍有 21 个既有 warning。
- `cargo build --manifest-path src-tauri/Cargo.toml`：通过。
- `npm run build`：通过。
- `npm test -- --run`：17 passed。

注意：自动化全绿不代表 Phase 3 已可合并。本轮问题主要是事件时序、前端残留状态、Native framework 链接和测试真实性缺口。

### 10.4 Critical Findings

#### Critical 1: 防重入锁完全依赖事件清理，事件丢失时仍可能造成 UI / Rust 状态脱节

**文件：**

- `src/App.tsx:64`
- `src/App.tsx:117`
- `src/App.tsx:164`

**问题：**

当前前端用 `isStartingRef` / `isStoppingRef` 阻止重复点击：

```ts
if (appState !== 'idle' || isStartingRef.current) return
...
isStartingRef.current = true
```

但 `isStartingRef.current = false` 主要依赖 `recording-state-changed` 事件：

```ts
else if (status.state === 'recording') {
  setAppState('recording')
  setIsPaused(false)
  isStartingRef.current = false
}
```

如果出现以下任一情况：

- Tauri `listen('recording-state-changed')` 的 Promise 尚未 resolve，用户已经点击开始。
- 后端事件先于前端监听注册完成。
- Tauri event 投递失败或被前端 missed。
- 后续重构导致某些成功路径没有发出状态事件。

则可能出现：

1. `startRecording()` 已成功，Rust 侧已进入 `Recording`。
2. 前端没有收到 `recording-state-changed: recording`。
3. `appState` 仍是 `idle`，但 `isStartingRef.current` 永远保持 `true`。
4. 用户看到的仍是 idle 录制面板，但再次点击“开始录制”被隐藏锁拦截。
5. UI 没有进入 recording 状态，也就没有停止按钮，后端可能仍在录制。

停止流程也有类似风险：`isStoppingRef` 主要依赖 completed / failed / idle 事件清理。若 `stopRecording()` 已成功但 completed 事件丢失，UI 可能卡在 recording，且停止按钮被 `isStoppingRef.current` 拦截。

**为什么重要：**

- 这与首轮 Critical 的本质相同：UI 与 Rust 状态机存在脱节路径。
- 风险不在双击本身，而在“命令成功但状态事件未被前端观察到”。
- 一旦发生，用户可能失去停止录制入口，属于录制主链路可用性风险。

**整改建议：**

保留事件驱动，但为 invoke 成功路径增加 Rust 状态回读兜底：

```ts
await startRecording()
const status = await fetchRecordingStatus()
if (status.state === 'recording') {
  setAppState('recording')
  setIsPaused(false)
  isStartingRef.current = false
}
```

停止流程同理：

```ts
const result = await stopRecording()
setRecordingResult(normalizeResult(result))
const status = await fetchRecordingStatus()
if (status.state === 'completed') {
  setAppState('preview')
  isStoppingRef.current = false
}
```

更稳妥的做法：

- 事件仍作为主路径，`fetchRecordingStatus()` 作为兜底确认。
- 开始按钮只在事件监听注册完成后可用，避免应用刚挂载时的监听竞态。
- 失败路径和超时路径都必须清理 `isStartingRef` / `isStoppingRef`。

### 10.5 Important Findings

#### Important 1: 麦克风电平停止后 Rust 归零，但前端仍可能显示上一轮残留值

**文件：**

- `src-tauri/src/lib.rs:173`
- `src-tauri/src/platform/macos_service.rs:176`
- `src/App.tsx:67`
- `src/App.tsx:182`
- `src/components/recording-panel.tsx:182`

**问题：**

整改后后端已有归零逻辑：

```rust
// MacRecordingService::stop()
if let Ok(mut guard) = self.mic_level.lock() {
    *guard = 0.0;
}
```

但 `stop_recording` 当前先停止 mic-level runtime：

```rust
if let Some(mut mic_runtime) = state.mic_level_runtime.lock()?.take() {
    mic_runtime.stop();
}
```

然后才调用 `service.stop()`，由 `service.stop()` 重置 `mic_level = 0.0`。由于 runtime 已停止，前端不会收到“最终 level = 0.0”的事件。

前端也没有在以下路径主动清理 `micVolume`：

- `recording-state-changed: completed`
- `recording-state-changed: idle`
- `recording-state-changed: failed`
- `handleBackToIdle()`
- 用户关闭麦克风开关时

因此用户回到 idle 后，如果 `micEnabled === true`，`RecordingPanel` 仍会根据旧的 `micVolume` 渲染条形指示器。

**为什么重要：**

- Phase D 明确要求“麦克风关闭时前端不会显示上一轮录制残留电平”和“停止录制后麦克风电平归零”。
- 当前实现只保证 Rust 共享值归零，不保证 UI 归零。
- 用户会看到与真实麦克风输入不一致的反馈。

**整改建议：**

后端：

- 在 `service.stop()` 完成并重置 mic_level 后、发 completed 状态事件前，emit 一次 `MicLevelPayload { level: 0.0 }`。
- 或调整顺序：先 stop capture / reset mic level / emit zero，再 stop mic-level runtime。

前端：

```ts
if (status.state === 'idle' || status.state === 'completed' || status.state === 'failed') {
  setMicVolume(0)
}
```

并在 `handleBackToIdle()` 中补充：

```ts
setMicVolume(0)
```

麦克风开关关闭时也应清空：

```ts
setMicEnabled(false)
setMicVolume(0)
```

#### Important 2: `AVCaptureDevice` 权限探测未显式链接 AVFoundation，真实麦克风权限可能长期返回 Unknown

**文件：**

- `src-tauri/src/platform/macos/permissions.rs:58`
- `src-tauri/src/platform/macos/permissions.rs:67`
- `src-tauri/Cargo.toml:27`

**问题：**

当前麦克风权限检测使用：

```rust
let cls = match AnyClass::get(c"AVCaptureDevice") {
    Some(c) => c,
    None => return -1,
};
```

但当前代码只显式链接了 CoreGraphics：

```rust
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}
```

没有显式链接 `AVFoundation` framework，也没有使用 `objc2-av-foundation` typed binding。若运行进程中 AVFoundation 未加载，`AnyClass::get(c"AVCaptureDevice")` 可能返回 `None`，进而映射为 `PermissionStatus::Unknown`。

**为什么重要：**

- Phase 3 的核心交付之一是“真实 macOS 权限检测”。
- 麦克风权限如果长期 Unknown，UI 无法可靠显示授权/拒绝/未决定状态。
- 这是 Native Safety Gate 范畴，必须人工审查真实运行路径。

**整改建议：**

在不新增依赖的情况下，可显式链接 framework：

```rust
#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}
```

或按 Dependency Gate 经人工批准后引入 `objc2-av-foundation`，用 typed API 替代字符串类名和 selector。

整改后需要手动验证：

- 麦克风权限未请求时返回 `notDetermined`。
- 麦克风权限拒绝时返回 `denied`。
- 麦克风权限授权后返回 `granted`。

#### Important 3: Phase 3 整改测试仍未覆盖 `mic-level` payload 更新 UI 与残留归零

**文件：**

- `tests/phase-3-code-review-remediation-checklist.md:60`
- `src/App.test.tsx:218`
- `tests/phase-3-w5-w6-checklist.md:37`

**问题：**

`src/App.test.tsx` 当前已有：

- 开始双击防重入测试。
- 停止双击防重入测试。
- `mic-level` 订阅测试。
- `mic-level` 退订测试。

但缺少以下计划要求的真实覆盖：

- mock `listen('mic-level')` 后保存 callback。
- 手动触发 `mic-level` payload。
- 断言 UI 麦克风电平发生可观察变化。
- 停止录制或回到 idle 后，断言麦克风电平归零。
- 麦克风关闭时，断言不会展示上一轮残留动态电平。

`tests/phase-3-code-review-remediation-checklist.md` 仍然全是未勾选项；`tests/phase-3-w5-w6-checklist.md` 中也存在“14 tests”等与当前 17 tests 不一致的记录。

**为什么重要：**

- 当前自动化无法抓住 Important 1 的残留电平问题。
- Phase 3 checklist 对“快速点击开始/停止不会导致状态错乱”的说明仍停留在旧实现描述。
- 文档声称整改完成，但可验收证据没有同步闭合。

**整改建议：**

补充前端测试：

1. 进入 recording 后触发 `mic-level { level: 0.8 }`，断言 UI 中麦克风条从静默变为动态状态。
2. 触发 completed / idle 后，断言麦克风条恢复 0 或不显示动态高度。
3. 麦克风关闭时开始录制，断言不会显示上一轮残留动态电平。

更新清单：

- `tests/phase-3-code-review-remediation-checklist.md`：按实际完成情况勾选自动化项，手动项保持未勾选。
- `tests/phase-3-w5-w6-checklist.md`：修正测试数量和快速点击验证说明；手动验证未执行的项不要勾选。

### 10.6 Minor Findings

#### Minor 1: `MicLevelRuntime` 替换顺序与清单描述不完全一致

**文件：**

- `src-tauri/src/lib.rs:136`
- `src-tauri/src/lib.rs:141`

**问题：**

当前 `start_recording` 中先 spawn 新的 `MicLevelRuntime`，再 lock `mic_level_runtime` 并停止旧 runtime：

```rust
let mic_runtime = MicLevelRuntime::spawn(move || {
    let level = mic_level.lock().map(|g| *g).unwrap_or(0.0);
    let _ = mic_app.emit("mic-level", MicLevelPayload { level });
});

let mut runtime_guard = state.mic_level_runtime.lock()?;
if let Some(mut existing) = runtime_guard.take() {
    existing.stop();
}
*runtime_guard = Some(mic_runtime);
```

清单要求是“启动新 mic-level runtime 前会停止并清理旧 runtime”。当前实际重叠窗口很短，且正常 start/stop 路径下旧 runtime 应已被 stop，但实现顺序与验收描述不完全一致。

**整改建议：**

先取出并停止旧 runtime，再 spawn 新 runtime：

```rust
{
    let mut runtime_guard = state.mic_level_runtime.lock()?;
    if let Some(mut existing) = runtime_guard.take() {
        existing.stop();
    }
}

let mic_runtime = MicLevelRuntime::spawn(...);
*state.mic_level_runtime.lock()? = Some(mic_runtime);
```

这能让资源释放路径更容易审计。

### 10.7 Phase 3 完成度更新

当前 Phase 3 状态应记录为：

- 主体功能：基本完成。
- 第一轮整改：部分完成。
- 自动化验证：通过。
- 手动验证：仍待执行。
- 合并状态：不建议合并，需继续整改 Round 2 复审发现的问题。

已满足：

- 前端未接收视频帧或音频流。
- Rust 仍持有录制状态机与主链路。
- 捕获回调未新增阻塞等待。
- 音视频队列仍为有界非阻塞发送。
- mic-level runtime 已具备 stop / join / Drop 生命周期。
- 权限单测已从真实 API 调用改为纯函数映射测试。

未满足：

- 事件丢失情况下的 UI / Rust 状态一致性兜底。
- UI 麦克风电平停止/关闭后的可靠归零。
- AVFoundation 权限探测的 framework 链接与 Native Safety Gate。
- `mic-level` payload 驱动 UI 的真实测试覆盖。
- Phase 3 / remediation checklist 的状态同步。
- `npm run tauri dev` 手动验证。

### 10.8 Round 2 整改建议拆分

#### Phase R2-A: 状态事件丢失兜底

**目标：** 保证 Tauri 状态事件丢失时，invoke 成功路径仍能通过 Rust 状态回读恢复 UI。

**建议改动文件：**

- `src/App.tsx`
- `src/App.test.tsx`

**验收标准：**

- `startRecording()` resolve 后，即使没有触发 `recording-state-changed` callback，也会通过 `fetchRecordingStatus()` 进入 recording 或失败态。
- `stopRecording()` resolve 后，即使没有 completed event，也会通过 `fetchRecordingStatus()` 进入 preview 或失败态。
- `isStartingRef` / `isStoppingRef` 在成功、失败、兜底超时路径都能释放。
- 有测试模拟“不触发 recording-state-changed 事件”的 start / stop 成功路径。

#### Phase R2-B: 麦克风电平 UI 归零闭环

**目标：** 停止录制、返回 idle、关闭麦克风时，UI 不显示上一轮残留电平。

**建议改动文件：**

- `src-tauri/src/lib.rs`
- `src/App.tsx`
- `src/App.test.tsx`

**验收标准：**

- stop 后前端能收到或主动设置 `micVolume = 0`。
- completed / idle / failed 状态下 `micVolume = 0`。
- 关闭麦克风开关时 `micVolume = 0`。
- 有测试覆盖 payload 更新 UI 和停止/关闭后的归零。

#### Phase R2-C: AVFoundation 链接与 Native Safety Gate

**目标：** 确保 `AVCaptureDevice` 权限探测在真实 macOS app 中可用。

**建议改动文件：**

- `src-tauri/src/platform/macos/permissions.rs`
- 如经人工批准，可修改 `src-tauri/Cargo.toml` 增加 typed binding。

**验收标准：**

- 明确链接 `AVFoundation` framework，或使用 `objc2-av-foundation` typed API。
- `AnyClass::get(c"AVCaptureDevice")` 不再依赖偶然加载。
- 人工完成 Native Safety Gate 审查。
- 手动验证麦克风权限 Granted / Denied / NotDetermined 三种状态。

#### Phase R2-D: Checklist 与测试证据同步

**目标：** 让整改状态、自动化证据和手动验证状态一致。

**建议改动文件：**

- `tests/phase-3-code-review-remediation-checklist.md`
- `tests/phase-3-w5-w6-checklist.md`
- `HANDOFF.md`

**验收标准：**

- 自动化已覆盖项勾选并标注测试名。
- 手动验证未执行项保持未勾选。
- 测试数量与实际输出一致。
- HANDOFF 不再写“整改完成”而忽略 Round 2 复审阻塞项。

### 10.9 Round 2 合并门槛

继续整改后，必须满足以下条件才建议合并：

- Critical 1 已修复，并有“事件未触发但 invoke 成功”的兜底测试。
- Important 1 已修复，并有 mic-level UI 更新与归零测试。
- Important 2 已修复或有人工 Native Safety Gate 接受记录。
- Important 3 已修复，checklist 与实际测试证据一致。
- 完整自动化矩阵全部通过。
- `npm run tauri dev` 手动验证完成。
- `permissions.rs` FFI 完成人工逐行审查。
