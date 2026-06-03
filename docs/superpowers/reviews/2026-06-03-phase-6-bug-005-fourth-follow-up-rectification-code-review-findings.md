# Phase 6 BUG-005 Fourth Follow-up Rectification Code Review Findings

> 日期：2026-06-03
> 审查结论：**Ready to merge? With fixes**

## 2. 总体结论

本轮整改已经关闭上一轮 review 的核心后端问题：

1. 后端 `stop_recording` 已返回结构化 `StopRecordingResponse { result, failed }`。
2. `drive_state_machine()` 在 finalize hard failure 时保留完整 `RecordingResult`、`diagnostics`、`writerDiagnostics` 和 `finalizationErrors`，并通过 `failed=true` 显式暴露失败语义。
3. TypeScript `stopRecording()` 返回类型已改为 `StopRecordingResponse`。
4. `RecordingResult.finalizationErrors` 已从 Rust serde 层保证始终输出数组，关闭 Rust/TS 契约不诚实问题。
5. writer / consumer timeout 测试已经调用生产 timeout helper，并用 parked thread + never-send channel 打到 timeout 分支。
6. 本轮未发现 `Cargo.toml` 核心依赖版本修改，未发现媒体帧/音频流穿越到前端 JS 层。

但当前仍建议 **With fixes** 后再合并，原因集中在前端失败路径的用户可见语义和测试强度：

1. `recording-state-changed: failed` 事件处理器可能覆盖 `StopRecordingResponse.failed=true` 分支写入的具体 `finalizationErrors` 文案。
2. 新增的前端 failed-response 测试没有真正驱动 UI stop 流程，只验证了 mock response shape，无法防止 `handleStopRecording()` 失败分支回退。
3. Rust 存在一个本轮引入的 unused import warning。

## 3. Findings

### Critical Findings

未发现 Critical 级别问题。

本轮没有发现会直接导致构建失败、测试失败、无界 timeout join 回退、数据流红线违规、或明显破坏录制/导出主路径的问题。

### Important 1: failed state event 可能覆盖 `finalizationErrors` 的具体错误详情

位置：

- `src/App.tsx:91`
- `src/App.tsx:93`
- `src/App.tsx:204`
- `src/App.tsx:206`
- `src/App.tsx:207`
- `src/App.tsx:208`

当前实现：

```ts
else if (status.state === 'failed') {
  setAppState('failed')
  setErrorMessage('录制过程中发生错误')
  setMicVolume(0)
  isStartingRef.current = false
  isStoppingRef.current = false
}
```

同时，`handleStopRecording()` 在收到结构化 response 后会处理具体错误：

```ts
if (response.failed) {
  const errorDetail =
    result.finalizationErrors?.join('; ') || '录制完成但存在错误'
  setAppState('failed')
  setErrorMessage(errorDetail)
  isStoppingRef.current = false
  return
}
```

问题：

- 后端 `stop_recording()` 在返回 response 前会 `emit_state_changed(&app, new_state)`。
- Tauri event 与 invoke response 都经过前端异步队列，UI 不应依赖二者固定排序。
- 如果 `failed` state event 在 invoke response 之前到达，后续 `response.failed` 分支会把通用错误覆盖为具体错误，结果可接受。
- 如果 `failed` state event 在 invoke response 之后到达，事件处理器会把已经展示的具体 `finalizationErrors` 覆盖成通用文案“录制过程中发生错误”。

为什么重要：

1. 上一轮 review 的核心要求是 hard finalize failure 必须对用户可见，不能只在 console 或泛化错误中丢失诊断。
2. `BUG.md` 规则 35 要求 `failed=true` 时前端进入 failed UI，并且不能把 hard failure 当成 preview 或 warning。
3. BUG-005 后续定位依赖具体 `finalizationErrors`，例如 consumer timeout、writer finish failure、artifact contract failure、source-aware contract failure。
4. 当前代码可能在真实 IPC 排序下展示通用错误，削弱本轮结构化 stop response 的主要价值。

建议修复：

方案 A：failed event 不覆盖已有错误详情。

```ts
else if (status.state === 'failed') {
  setAppState('failed')
  setErrorMessage((current) => current || '录制过程中发生错误')
  setMicVolume(0)
  isStartingRef.current = false
  isStoppingRef.current = false
}
```

该方案保证：

- event 先到时，先展示通用错误，后续 `response.failed` 仍能覆盖成具体错误。
- response 先到时，event 后到不会覆盖具体错误。

方案 B：后端 failed state event 携带结构化错误详情，前端统一从事件 payload 或 response payload 中读取。

推荐先采用方案 A，因为改动最小，符合本轮整改的 surgical 目标。

建议新增回归测试：

1. `stop failed response displays finalization error detail`
2. `late failed state event does not overwrite finalization error detail`

测试应覆盖：

- mock `stop_recording` 返回 `{ failed: true, result.finalizationErrors: ['消费线程超时'] }`
- 点击 stop 后错误页展示 `消费线程超时`
- 在 response 分支之后再模拟 `recording-state-changed: failed`
- 仍然展示 `消费线程超时`，而不是“录制过程中发生错误”

严重级别：Important。

理由：这不是构建 blocker，但它影响本轮整改最核心的“失败可见性”和 BUG-005 诊断闭环。

### Important 2: 新增前端 failed-response 测试没有真正覆盖 UI 失败路径

位置：

- `src/App.test.tsx:622`
- `src/App.test.tsx:636`
- `src/App.test.tsx:638`
- `src/App.test.tsx:639`
- `src/App.test.tsx:640`
- `src/App.test.tsx:641`

当前测试：

```ts
it('enters failed state when stop response has failed=true', async () => {
  invokeMock.mockImplementation((command: string) => {
    // ...
    if (command === 'stop_recording') return Promise.resolve({
      result: { /* ... */, finalizationErrors: ['消费线程超时'] },
      failed: true,
    })
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)

  // Verify the mock returns the correct StopRecordingResponse shape.
  const stopResponse = await invokeMock('stop_recording')
  expect(stopResponse.failed).toBe(true)
  expect(stopResponse.result.finalizationErrors).toContain('消费线程超时')
})
```

问题：

- 测试没有点击“开始录制”。
- 测试没有模拟 `recording-state-changed: recording`。
- 测试没有点击 stop 按钮。
- 测试没有等待 `handleStopRecording()` 调用 `stopRecording()`。
- 测试没有断言 `ErrorView` 出现。
- 测试没有断言 `finalizationErrors` 被展示给用户。
- 测试没有断言不会进入 `PreviewView` / `预览与美化`。
- 测试只是直接调用 mock 并验证 mock 自己返回了什么。

为什么重要：

1. `handleStopRecording()` 中 `response.failed` 分支如果被删除、条件写反、或继续进入 preview，该测试仍会通过。
2. 这没有锁住 `BUG.md` 规则 35 的前端要求：`failed=true` 必须进入 failed UI，不能进入 preview。
3. 这也无法发现 Important 1 中的 failed event 覆盖具体错误详情问题。
4. 测试标题与实际覆盖范围不一致，后续维护者可能误以为 failed response UI 已被保护。

建议修复：

参考已有测试 `displays recording result after stop with camelCase fields` 和 `enters preview state via status fallback when completed event is lost` 的写法，改成真实 UI 行为测试：

1. mock 初始 `recording_status` 为 idle。
2. mock permissions / set_capture_mode / set_audio_config / start_recording。
3. mock `stop_recording` 返回：

```ts
{
  result: {
    durationSecs: 1,
    frameCount: 30,
    mixedAudioChunkCount: 10,
    outputPath: null,
    cursorMetadataPath: null,
    effectTimelinePath: null,
    trimMetadataPath: null,
    cutTimelinePath: null,
    writerDiagnostics: { /* complete shape */ },
    diagnostics: { /* complete shape */ },
    finalizationErrors: ['消费线程超时'],
  },
  failed: true,
}
```

4. `render(<App />)`。
5. 点击“开始录制”。
6. 模拟 `recording-state-changed: recording`，让 UI 进入录制中。
7. 点击 stop 按钮。
8. 断言 `stop_recording` 被调用。
9. 断言错误页出现：

```ts
expect(await screen.findByText('录制失败')).toBeTruthy()
expect(screen.getByText('消费线程超时')).toBeTruthy()
expect(screen.queryByText('预览与美化')).toBeNull()
```

10. 额外模拟 late failed event 后，继续断言 `消费线程超时` 仍存在。

严重级别：Important。

理由：实现代码当前看起来基本正确，但测试没有保护用户最关心的失败路径，回归风险仍然存在。

### Minor 1: `src-tauri/src/lib.rs` 存在本轮引入的 unused import warning

位置：

- `src-tauri/src/lib.rs:31`

当前代码：

```rust
use media::recording_writer::{RecordingResult, StopRecordingResponse};
```

问题：

- `stop_recording()` 返回类型已改为 `StopRecordingResponse`。
- `RecordingResult` 在 `src-tauri/src/lib.rs` 中已未使用。
- `cargo test --manifest-path src-tauri/Cargo.toml` 和 `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` 均会提示 unused import warning。

为什么重要：

1. 这是小问题，不影响运行。
2. 但本轮是 code review 整改收口，新增 warning 会降低门禁信噪比。
3. 后续如果开启更严格 lint，这类 warning 可能变成 blocker。

建议修复：

```rust
use media::recording_writer::StopRecordingResponse;
```

严重级别：Minor。

理由：不影响功能，但应随整改清理掉。

## 4. 已验证通过的内容

### 4.1 后端结构化 stop response 主体正确

位置：

- `src-tauri/src/media/recording_writer.rs:297`
- `src-tauri/src/platform/macos_service.rs:543`
- `src-tauri/src/platform/macos_service.rs:564`
- `src-tauri/src/platform/macos_service.rs:576`
- `src-tauri/src/platform/macos_service.rs:583`
- `src-tauri/src/lib.rs:242`
- `src-tauri/src/lib.rs:280`

结论：

- `StopRecordingResponse` 使用 `#[serde(rename_all = "camelCase")]`。
- response 包含完整 `result` 和 `failed`。
- `self.errors.is_empty()` 时 `failed=false`，状态机进入 completed。
- `self.errors` 非空时写入 `result.finalization_errors`，状态机进入 failed，response 返回 `failed=true`。
- `stop_recording` command 返回 `Result<StopRecordingResponse, String>`。

这关闭了上一轮 review 中“hard finalize failure 被包装成 `Ok(RecordingResult)` 且前端当 warning 处理”的主要后端语义问题。

### 4.2 `finalizationErrors` 类型契约已对齐

位置：

- `src-tauri/src/media/recording_writer.rs:282`
- `src-tauri/src/media/recording_writer.rs:285`
- `src-tauri/src/media/recording_writer.rs:860`
- `src/lib/tauri.ts:103`

结论：

- Rust 已移除 `skip_serializing_if = "Vec::is_empty"`。
- `RecordingResult.finalization_errors` 空数组也会序列化为 `finalizationErrors: []`。
- TypeScript 保持 `finalizationErrors: string[]` 必填是诚实的。
- 测试 `recording_result_always_includes_finalization_errors` 通过。

### 4.3 timeout helper 和测试已打到生产分支

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:98`
- `src-tauri/src/media/ffmpeg_writer.rs:111`
- `src-tauri/src/media/ffmpeg_writer.rs:127`
- `src-tauri/src/media/ffmpeg_writer.rs:1816`
- `src-tauri/src/platform/macos_service.rs:405`
- `src-tauri/src/platform/macos_service.rs:420`
- `src-tauri/src/platform/macos_service.rs:436`
- `src-tauri/src/platform/macos_service.rs:2325`
- `src-tauri/src/platform/macos_service.rs:2366`

结论：

- `join_worker_with_timeout()` timeout 分支 `worker.take()` 后直接返回 `RecordingWriteFailed`，没有调用无界 `join()`。
- `receive_consumer_output_with_timeout()` timeout 分支 `handle.take()` 后返回 empty output，并记录 timeout error，没有调用无界 `join()`。
- 新测试使用 parked thread + never-send channel 触发 timeout 分支。
- `join_consumer_timeout_detaches_parked_consumer` 和 `join_consumer_timeout_records_error_and_preserves_empty_output` 在默认 `cargo test` 中运行通过。
- `join_worker_timeout_returns_without_joining_parked_worker` 在 `--features ffmpeg` 下运行通过，因为 `ffmpeg_writer` 模块 feature-gated。

这符合 `BUG.md` 规则 28 和规则 36。

### 4.4 没有发现本轮新增的数据流红线问题

结论：

- `StopRecordingResponse` 只携带轻量结构化结果、路径、诊断和错误字符串。
- 没有把视频帧或音频流传给前端 JS。
- React 仍只负责展示状态、错误、诊断路径和结果摘要。

### 4.5 没有发现本轮修改核心依赖版本

结论：

- 审查范围内未发现 `Cargo.toml` 核心依赖版本修改。
- 符合项目约束中“AI 不能直接修改 Cargo.toml 核心依赖版本”的红线。

## 5. 验证命令与结果

本轮审查执行过以下命令：

```bash
cargo test --manifest-path src-tauri/Cargo.toml join_worker_timeout_returns_without_joining_parked_worker
```

结果：

- 通过，但匹配到 0 个测试。
- 原因：`ffmpeg_writer` 模块受 `ffmpeg` feature gate 控制，该测试不在默认 feature 下编译运行。
- 该结果不是失败，但说明 writer timeout 测试必须纳入 `--features ffmpeg` 门禁。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg join_worker_timeout_returns_without_joining_parked_worker
```

结果：

- 1 个测试通过。

```bash
cargo test --manifest-path src-tauri/Cargo.toml join_consumer_timeout
```

结果：

- 2 个测试通过。

```bash
cargo test --manifest-path src-tauri/Cargo.toml recording_result_always_includes_finalization_errors
```

结果：

- 1 个测试通过。

```bash
npm test -- --run
```

结果：

- 1 个 test file 通过。
- 53 个测试通过。
- jsdom 输出若干 `Not implemented: Window's scrollTo() method`，未导致失败。

```bash
npm run build
```

结果：

- TypeScript 和 Vite build 通过。

```bash
cargo fmt --check
```

结果：

- 从仓库根目录执行失败，原因是根目录没有 `Cargo.toml`。
- 这是命令路径问题，不是代码格式问题。

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

结果：

- 通过。

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

结果：

- 253 个测试通过。
- 有 warnings，其中本轮相关 warning 是 `src/lib.rs:31` unused import `RecordingResult`。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

结果：

- 311 个 unit tests 通过。
- 10 个 integration tests 通过。
- 有 warnings，其中本轮相关 warning 是 `src/lib.rs:31` unused import `RecordingResult`。

## 6. 建议整改顺序

### Phase A: 修复 failed event 覆盖具体错误详情

目标：

- `StopRecordingResponse.failed=true` 写入的具体 `finalizationErrors` 不会被 late failed event 覆盖。

建议改动：

- 修改 `src/App.tsx` failed event handler：

```ts
setErrorMessage((current) => current || '录制过程中发生错误')
```

验收：

- late failed event 后仍显示具体 finalization error。

### Phase B: 改造前端 failed-response 测试为真实 UI 行为测试

目标：

- 测试真正覆盖 `handleStopRecording()` 的 `response.failed` 分支。

建议改动：

- 重写 `src/App.test.tsx` 中 `enters failed state when stop response has failed=true`。
- 走完整流程：render -> start -> emit recording -> click stop -> mock failed response -> assert error page。
- 增加 late failed event 不覆盖错误详情的断言。

验收：

- 删除 `handleStopRecording()` 中 `if (response.failed)` 分支时，测试必须失败。
- 将 failed event handler 改回无条件通用错误时，late event 测试必须失败。

### Phase C: 清理 unused import

目标：

- 去除本轮新增 Rust warning。

建议改动：

- `src-tauri/src/lib.rs` 删除 unused `RecordingResult` import。

验收：

- `cargo test --manifest-path src-tauri/Cargo.toml` 不再出现该 unused import warning。
- 既有 ScreenCaptureKit / FFI 命名 warnings 可另行处理，不属于本轮整改范围。

## 7. 推荐门禁

整改完成后建议至少运行：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
npm run build
```

若只做最小快速验证，至少运行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml join_consumer_timeout
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg join_worker_timeout_returns_without_joining_parked_worker
cargo test --manifest-path src-tauri/Cargo.toml recording_result_always_includes_finalization_errors
npm test -- --run
npm run build
```

## 8. 最终评估

**Ready to merge? With fixes**

理由：

- 后端结构化 stop response、timeout helper、serde 契约整改方向正确，且自动化验证通过。
- 仍需修复前端 failed event 覆盖具体错误详情的风险。
- 仍需把前端 failed-response 测试从 mock shape 断言升级为真实 UI 行为测试。
- unused import 属于小清理项，建议与上述整改一起处理。
