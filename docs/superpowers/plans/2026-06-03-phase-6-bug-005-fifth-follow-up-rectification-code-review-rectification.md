# Phase 6 BUG-005 Fifth Follow-up Code Review Rectification Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the three remaining findings from the fifth follow-up code review — prevent `recording-state-changed: failed` event from overwriting specific `finalizationErrors`, rewrite the failed-response test as a real UI behavior test, and remove the unused `RecordingResult` import.

**Architecture:** Surgical changes to `src/App.tsx` (failed event handler), `src/App.test.tsx` (two new tests replacing one weak test), and `src-tauri/src/lib.rs` (unused import removal). No new modules, no new dependencies.

**Tech Stack:** React, TypeScript, Vitest, Rust

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/App.tsx:91-97` | Modify | Change `setErrorMessage(...)` to preserve existing error detail |
| `src/App.test.tsx:622-642` | Modify | Replace weak mock-shape test with two real UI behavior tests |
| `src-tauri/src/lib.rs:31` | Modify | Remove unused `RecordingResult` import |

---

### Task 1: Fix failed event handler to preserve existing error detail

**Files:**
- Modify: `src/App.tsx:91-97`

- [ ] **Step 1: Read the current failed event handler**

Current code at `src/App.tsx:91-97`:

```ts
else if (status.state === 'failed') {
  setAppState('failed')
  setErrorMessage('录制过程中发生错误')
  setMicVolume(0)
  isStartingRef.current = false
  isStoppingRef.current = false
}
```

- [ ] **Step 2: Apply the fix — use functional updater to preserve existing error**

Replace line 93:

```ts
// Before:
setErrorMessage('录制过程中发生错误')

// After:
setErrorMessage((current) => current || '录制过程中发生错误')
```

The full block becomes:

```ts
else if (status.state === 'failed') {
  setAppState('failed')
  setErrorMessage((current) => current || '录制过程中发生错误')
  setMicVolume(0)
  isStartingRef.current = false
  isStoppingRef.current = false
}
```

This ensures:
- If event arrives **before** `StopRecordingResponse`: `current` is empty string, so `'录制过程中发生错误'` is used. The subsequent `response.failed` branch will overwrite with specific `finalizationErrors`.
- If event arrives **after** `StopRecordingResponse`: `current` already holds the specific error detail (e.g. `'消费线程超时'`), so the `||` short-circuits and preserves it.

- [ ] **Step 3: Commit**

```bash
git add src/App.tsx
git commit -m "fix(ui): failed event 不覆盖已有 finalizationErrors 具体错误详情"
```

---

### Task 2: Rewrite failed-response test as real UI behavior test

**Files:**
- Modify: `src/App.test.tsx:622-642`

- [ ] **Step 1: Delete the existing weak test**

Delete the test at lines 622-642 (`enters failed state when stop response has failed=true`).

- [ ] **Step 2: Write two new real UI behavior tests**

Insert the following two tests in place of the deleted one. Follow the pattern used by `displays recording result after stop with camelCase fields` (lines ~440-529):

```ts
it('stop failed response displays finalization error detail', async () => {
  const { listen } = await import('@tauri-apps/api/event')
  const stateCallbacks: Array<(status: { state: string }) => void> = []

  vi.mocked(listen).mockImplementation(
    (event: string, callback: (event: { event: string; id: number; payload: unknown }) => void) => {
      if (event === 'recording-state-changed') {
        stateCallbacks.push((status) => callback({ event, id: 0, payload: status }))
      }
      return Promise.resolve(() => {})
    },
  )

  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'set_capture_mode') return Promise.resolve()
    if (command === 'set_audio_config') return Promise.resolve()
    if (command === 'start_recording') return Promise.resolve()
    if (command === 'stop_recording') return Promise.resolve({
      result: {
        durationSecs: 1,
        frameCount: 30,
        mixedAudioChunkCount: 10,
        outputPath: null,
        cursorMetadataPath: null,
        effectTimelinePath: null,
        trimMetadataPath: null,
        cutTimelinePath: null,
        writerDiagnostics: { audioChunksReceived: 0, audioChunksAppended: 0, audioChunksDiscardedFullOverlap: 0, audioChunksTrimmedPartialOverlap: 0, audioRealFramesAppended: 0, audioSilenceFramesPadded: 0, audioRealRmsMaxBeforeEncode: 0, aacFramesEncoded: 0, silentAacFramesEncoded: 0, generatedSilentTrack: false, videoQueueFullCount: 0, audioQueueFullCount: 0, systemChunksReceivedByWriter: 0, micChunksReceivedByWriter: 0 },
        diagnostics: { requestedSystemAudio: true, requestedMicrophone: false, microphoneDevice: null, systemChunksReceived: 0, micChunksReceived: 0, systemChunksDropped: 0, micChunksDropped: 0, mixedChunksQueued: 0, writerPushAudioFailures: 0, systemRmsMax: 0, micRmsMax: 0, mixedRmsMax: 0, generatedSilentTrack: false, pairedWindowCount: 0, systemOnlyWindowCount: 0, micOnlyWindowCount: 0, sourceTimeoutWindowCount: 0, systemRmsMaxBeforeWriter: 0, micRmsMaxBeforeWriter: 0, systemWindowsBeforeWriter: 0, micWindowsBeforeWriter: 0, systemFramesBeforeWriter: 0, micFramesBeforeWriter: 0, micStopDiagnostics: null },
        finalizationErrors: ['消费线程超时'],
      },
      failed: true,
    })
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)

  // 1. Start recording
  const startButtons = await screen.findAllByText('开始录制')
  fireEvent.click(startButtons[0])

  await vi.waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
  })

  // 2. Enter recording state
  act(() => {
    for (const cb of stateCallbacks) {
      cb({ state: 'recording' })
    }
  })

  // 3. Click stop button
  await vi.waitFor(() => {
    const buttons = screen.getAllByRole('button')
    expect(buttons.length).toBeGreaterThan(0)
  })
  const buttons = screen.getAllByRole('button')
  const stopButton = buttons[buttons.length - 1]
  await act(async () => {
    fireEvent.click(stopButton)
  })

  // 4. Assert stop_recording was called
  await vi.waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('stop_recording', undefined)
  })

  // 5. Assert error page shows specific finalization error
  await screen.findByText('消费线程超时')
  expect(screen.getByText('消费线程超时')).toBeTruthy()
  // 6. Assert does NOT enter preview
  expect(screen.queryByText('预览与美化')).toBeNull()
})

it('late failed state event does not overwrite finalization error detail', async () => {
  const { listen } = await import('@tauri-apps/api/event')
  const stateCallbacks: Array<(status: { state: string }) => void> = []

  vi.mocked(listen).mockImplementation(
    (event: string, callback: (event: { event: string; id: number; payload: unknown }) => void) => {
      if (event === 'recording-state-changed') {
        stateCallbacks.push((status) => callback({ event, id: 0, payload: status }))
      }
      return Promise.resolve(() => {})
    },
  )

  invokeMock.mockImplementation((command: string) => {
    if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
    if (command === 'set_capture_mode') return Promise.resolve()
    if (command === 'set_audio_config') return Promise.resolve()
    if (command === 'start_recording') return Promise.resolve()
    if (command === 'stop_recording') return Promise.resolve({
      result: {
        durationSecs: 1,
        frameCount: 30,
        mixedAudioChunkCount: 10,
        outputPath: null,
        cursorMetadataPath: null,
        effectTimelinePath: null,
        trimMetadataPath: null,
        cutTimelinePath: null,
        writerDiagnostics: { audioChunksReceived: 0, audioChunksAppended: 0, audioChunksDiscardedFullOverlap: 0, audioChunksTrimmedPartialOverlap: 0, audioRealFramesAppended: 0, audioSilenceFramesPadded: 0, audioRealRmsMaxBeforeEncode: 0, aacFramesEncoded: 0, silentAacFramesEncoded: 0, generatedSilentTrack: false, videoQueueFullCount: 0, audioQueueFullCount: 0, systemChunksReceivedByWriter: 0, micChunksReceivedByWriter: 0 },
        diagnostics: { requestedSystemAudio: true, requestedMicrophone: false, microphoneDevice: null, systemChunksReceived: 0, micChunksReceived: 0, systemChunksDropped: 0, micChunksDropped: 0, mixedChunksQueued: 0, writerPushAudioFailures: 0, systemRmsMax: 0, micRmsMax: 0, mixedRmsMax: 0, generatedSilentTrack: false, pairedWindowCount: 0, systemOnlyWindowCount: 0, micOnlyWindowCount: 0, sourceTimeoutWindowCount: 0, systemRmsMaxBeforeWriter: 0, micRmsMaxBeforeWriter: 0, systemWindowsBeforeWriter: 0, micWindowsBeforeWriter: 0, systemFramesBeforeWriter: 0, micFramesBeforeWriter: 0, micStopDiagnostics: null },
        finalizationErrors: ['消费线程超时'],
      },
      failed: true,
    })
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)

  // 1. Start recording
  const startButtons = await screen.findAllByText('开始录制')
  fireEvent.click(startButtons[0])

  await vi.waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
  })

  // 2. Enter recording state
  act(() => {
    for (const cb of stateCallbacks) {
      cb({ state: 'recording' })
    }
  })

  // 3. Click stop button
  await vi.waitFor(() => {
    const buttons = screen.getAllByRole('button')
    expect(buttons.length).toBeGreaterThan(0)
  })
  const buttons = screen.getAllByRole('button')
  const stopButton = buttons[buttons.length - 1]
  await act(async () => {
    fireEvent.click(stopButton)
  })

  // 4. Wait for specific error to appear
  await screen.findByText('消费线程超时')
  expect(screen.getByText('消费线程超时')).toBeTruthy()

  // 5. Simulate late failed state event AFTER response was already processed
  act(() => {
    for (const cb of stateCallbacks) {
      cb({ state: 'failed' })
    }
  })

  // 6. Specific error must still be shown, not overwritten by generic message
  expect(screen.getByText('消费线程超时')).toBeTruthy()
  expect(screen.queryByText('录制过程中发生错误')).toBeNull()
})
```

- [ ] **Step 3: Run frontend tests to verify**

Run: `npm test -- --run`
Expected: All tests pass, including the two new tests.

- [ ] **Step 4: Commit**

```bash
git add src/App.test.tsx
git commit -m "test(ui): failed response 真实 UI 行为测试 — 锁定 finalizationErrors 不被覆盖"
```

---

### Task 3: Remove unused `RecordingResult` import

**Files:**
- Modify: `src-tauri/src/lib.rs:31`

- [ ] **Step 1: Verify `RecordingResult` is unused in the file**

Read `src-tauri/src/lib.rs` and confirm `RecordingResult` only appears on line 31 (the import). The `stop_recording` return type uses `StopRecordingResponse`, not `RecordingResult`.

- [ ] **Step 2: Remove the unused import**

```rust
// Before:
use media::recording_writer::{RecordingResult, StopRecordingResponse};

// After:
use media::recording_writer::StopRecordingResponse;
```

- [ ] **Step 3: Run Rust tests to verify no warnings**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 253 tests pass, no unused import warning for `RecordingResult`.

Run: `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`
Expected: 311 unit + 10 integration tests pass, no unused import warning.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "chore(core): 移除 lib.rs 未使用的 RecordingResult import"
```

---

## Verification Checklist

After all tasks are complete, run the full gate:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
npm run build
```

Expected results:
- `cargo fmt` passes
- `cargo test` — 253 tests pass, **zero** unused import warnings for `RecordingResult`
- `cargo test --features ffmpeg` — 311 unit + 10 integration tests pass
- `npm test` — 54 tests pass (53 existing + 1 new, since we replace 1 weak test with 2 strong tests)
- `npm run build` passes

Key regression checks:
- Deleting the `if (response.failed)` branch in `handleStopRecording()` → `stop failed response displays finalization error detail` test MUST fail
- Reverting `setErrorMessage` to unconditional `'录制过程中发生错误'` → `late failed state event does not overwrite finalization error detail` test MUST fail
