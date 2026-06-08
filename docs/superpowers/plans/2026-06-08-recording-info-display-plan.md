# 录制参数摘要显示 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将录制中虚线框内的占位文字替换为分组网格样式的录制参数摘要（分辨率、帧率、系统音频、麦克风）。

**Architecture:** 仅修改 `src/App.tsx` 的录制状态视图中的虚线框内容区域，将第二行占位文案替换为"画面"和"音频"两个分组的参数展示。所有数据来自 App 组件已有 state，无需新增状态或后端调用。

**Tech Stack:** React, Tailwind CSS, Vitest

---

### Task 1: 修改虚线框内容为分组网格

**Files:**
- Modify: `src/App.tsx:504-511`

- [ ] **Step 1: 替换虚线框内的占位文字**

将 `src/App.tsx` 第 504-511 行的：

```tsx
<div className="flex-1 w-full max-w-4xl mx-auto my-8 rounded-2xl border-2 border-dashed border-border/30 flex items-center justify-center">
  <div className="text-center text-muted-foreground">
    <p className="text-sm mb-1">
      正在录制 {recordingMode === 'fullscreen' ? '全屏' : recordingMode === 'window' ? '窗口' : '区域'}
    </p>
    <p className="text-xs opacity-60">此区域表示被录制的屏幕内容</p>
  </div>
</div>
```

替换为：

```tsx
<div className="flex-1 w-full max-w-4xl mx-auto my-8 rounded-2xl border-2 border-dashed border-border/30 flex items-center justify-center">
  <div className="text-center text-muted-foreground">
    <p className="text-sm mb-3">
      正在录制 {recordingMode === 'fullscreen' ? '全屏' : recordingMode === 'window' ? '窗口' : '区域'}
    </p>
    <div className="flex gap-8 justify-center">
      <div className="text-left">
        <p className="text-[10px] uppercase tracking-wide mb-1.5">画面</p>
        <p className="text-xs">{resolution.width}×{resolution.height}</p>
        <p className="text-xs">{fps} fps</p>
      </div>
      <div className="text-left">
        <p className="text-[10px] uppercase tracking-wide mb-1.5">音频</p>
        <p className="text-xs">
          系统音频{' '}
          <span className={systemAudioEnabled ? 'text-green-400' : 'text-muted-foreground'}>
            {systemAudioEnabled ? '✓' : '✗'}
          </span>
        </p>
        <p className="text-xs">
          麦克风{' '}
          <span className={micEnabled ? 'text-green-400' : 'text-muted-foreground'}>
            {micEnabled ? '✓' : '✗'}
          </span>
        </p>
      </div>
    </div>
  </div>
</div>
```

- [ ] **Step 2: 验证编译通过**

Run: `npm run build`
Expected: 构建成功，无 TypeScript 错误

- [ ] **Step 3: Commit**

```bash
git add src/App.tsx
git commit -m "feat(ui): 录制中虚线框显示参数摘要替代占位文字"
```

---

### Task 2: 添加前端测试

**Files:**
- Modify: `src/App.test.tsx`

- [ ] **Step 1: 编写失败测试 — 录制中显示参数摘要**

在 `src/App.test.tsx` 中添加测试，验证进入录制状态后虚线框内显示分辨率、帧率和音频状态：

```tsx
it('shows recording parameter summary during recording', async () => {
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
    if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted', accessibility: 'granted' })
    if (command === 'set_capture_mode') return Promise.resolve()
    if (command === 'set_audio_config') return Promise.resolve()
    if (command === 'start_recording') return Promise.resolve()
    return Promise.reject(new Error(`unexpected command ${command}`))
  })

  render(<App />)

  // Start recording
  const startButtons = await screen.findAllByText('开始录制')
  fireEvent.click(startButtons[0])
  await vi.waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
  })

  // Enter recording state
  act(() => {
    for (const cb of stateCallbacks) {
      cb({ state: 'recording' })
    }
  })

  // Verify grouped grid appears with recording parameters
  await vi.waitFor(() => {
    expect(screen.getByText('画面')).toBeInTheDocument()
    expect(screen.getByText('音频')).toBeInTheDocument()
    expect(screen.getByText('1920×1080')).toBeInTheDocument()
    expect(screen.getByText('30 fps')).toBeInTheDocument()
    expect(screen.getByText(/系统音频/)).toBeInTheDocument()
    expect(screen.getByText(/麦克风/)).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: 运行测试验证失败**

Run: `npm test -- src/App.test.tsx -t "shows recording parameter summary" --run`
Expected: FAIL — 测试找不到 "画面"、"音频" 等文本（因为还没实现）

- [ ] **Step 3: 运行 Task 1 的实现后重新运行测试**

Run: `npm test -- src/App.test.tsx -t "shows recording parameter summary" --run`
Expected: PASS

- [ ] **Step 4: 运行全部测试确认无回归**

Run: `npm test -- --run`
Expected: 全部通过，无失败

- [ ] **Step 5: Commit**

```bash
git add src/App.test.tsx
git commit -m "test(ui): 添加录制参数摘要显示的测试"
```
