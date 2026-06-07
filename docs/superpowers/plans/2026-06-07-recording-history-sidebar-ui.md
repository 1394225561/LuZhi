# Recording History Sidebar UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rework the history recording sidebar into a compact Raycast-style tool list while preserving existing load, import, delete, and record-selection logic.

**Architecture:** This is a UI-only change centered on `RecordingSidebar`. Existing Tauri API wrappers and parent `App` state transitions stay unchanged; tests lock the existing click/delete behavior while asserting the new visible UI contract.

**Tech Stack:** React 19, TypeScript, Tailwind CSS 4, Vitest, React Testing Library, lucide-react, shadcn `Button`.

---

## File Structure

- Modify: `src/components/recording-sidebar.tsx`
  - Responsibility: render the sidebar, loading/error/empty states, import button, record list, delete affordance, and record click handler.
- Create: `src/components/recording-sidebar.test.tsx`
  - Responsibility: component-level regression tests for the new compact sidebar UI and preserved interactions.
- Create: `tests/2026-06-07-recording-history-sidebar-ui-checklist.md`
  - Responsibility: manual self-test checklist required by project phase rules.

No backend, Tauri command wrapper, parent `App`, global CSS, dependency, or route changes are part of this plan.

## Phase 1: Lock Existing Behavior With Focused Tests

### Task 1: Add RecordingSidebar Tests

**Files:**
- Create: `src/components/recording-sidebar.test.tsx`

- [ ] **Step 1: Create the component test file**

Add this file:

```tsx
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { RecordingSidebar } from './recording-sidebar'
import {
  deleteRecording,
  listRecordings,
  type LibraryEntrySummary,
} from '@/lib/tauri'

vi.mock('@/lib/tauri', () => ({
  listRecordings: vi.fn(),
  deleteRecording: vi.fn(),
  importRecording: vi.fn(),
}))

const listRecordingsMock = vi.mocked(listRecordings)
const deleteRecordingMock = vi.mocked(deleteRecording)

const recordings: LibraryEntrySummary[] = [
  { id: 'recent-recording', createdAt: 1780827600000, durationSecs: 75 },
  { id: 'older-recording', createdAt: 1780741200000, durationSecs: 180 },
]

describe('RecordingSidebar', () => {
  beforeEach(() => {
    listRecordingsMock.mockReset()
    deleteRecordingMock.mockReset()
    listRecordingsMock.mockResolvedValue([...recordings])
    deleteRecordingMock.mockResolvedValue()
  })

  afterEach(() => {
    cleanup()
  })

  it('renders compact history count and recording durations', async () => {
    render(
      <RecordingSidebar
        isOpen={true}
        onToggle={vi.fn()}
        onSelectRecording={vi.fn()}
      />,
    )

    expect(await screen.findByText('共 2 条录制')).toBeInTheDocument()
    expect(screen.getByText('时长：1:15')).toBeInTheDocument()
    expect(screen.getByText('时长：3:00')).toBeInTheDocument()
  })

  it('keeps the empty state clear when no recordings exist', async () => {
    listRecordingsMock.mockResolvedValueOnce([])

    render(
      <RecordingSidebar
        isOpen={true}
        onToggle={vi.fn()}
        onSelectRecording={vi.fn()}
      />,
    )

    expect(await screen.findByText('暂无历史录制')).toBeInTheDocument()
    expect(screen.getByText('录制完成后会出现在这里')).toBeInTheDocument()
  })

  it('selects a recording when a history row is clicked', async () => {
    const onSelectRecording = vi.fn()

    render(
      <RecordingSidebar
        isOpen={true}
        onToggle={vi.fn()}
        onSelectRecording={onSelectRecording}
      />,
    )

    const rowText = await screen.findByText('时长：1:15')
    fireEvent.click(rowText)

    expect(onSelectRecording).toHaveBeenCalledWith('recent-recording')
  })

  it('deletes a recording without selecting it', async () => {
    const onSelectRecording = vi.fn()

    render(
      <RecordingSidebar
        isOpen={true}
        onToggle={vi.fn()}
        onSelectRecording={onSelectRecording}
      />,
    )

    await screen.findByText('共 2 条录制')
    fireEvent.click(screen.getAllByTitle('删除')[0])

    await waitFor(() => {
      expect(deleteRecordingMock).toHaveBeenCalledWith('recent-recording')
    })
    expect(onSelectRecording).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: Run the new tests and verify they fail for the new UI contract**

Run:

```bash
npm test -- src/components/recording-sidebar.test.tsx
```

Expected: at least the count assertion (`共 2 条录制`) and empty subtitle assertion (`录制完成后会出现在这里`) fail before the UI change.

- [ ] **Step 3: Keep the failing tests uncommitted for the red phase**

Do not commit while the focused test is red. Leave `src/components/recording-sidebar.test.tsx` in the working tree and continue to Phase 2.

## Phase 2: Rework Sidebar UI Without Logic Changes

### Task 2: Apply the Compact Sidebar Layout

**Files:**
- Modify: `src/components/recording-sidebar.tsx`

- [ ] **Step 1: Update the toggle button width offset and panel width**

Change only class strings in the existing toggle button and sidebar panel:

```tsx
className={cn(
  'fixed top-1/2 -translate-y-1/2 z-40 bg-card/95 border border-border/60 rounded-l-lg p-2 shadow-lg shadow-black/20',
  'hover:bg-surface-hover hover:border-border transition-all duration-200',
  isOpen ? 'right-[360px]' : 'right-0',
)}
```

```tsx
className={cn(
  'fixed right-0 top-0 h-full bg-card/98 border-l border-border/60 shadow-2xl shadow-black/30',
  'transition-all duration-300 ease-in-out z-30 flex flex-col',
  isOpen ? 'w-[360px]' : 'w-0 overflow-hidden',
)}
```

- [ ] **Step 2: Replace the header JSX**

Replace the current header block with:

```tsx
<div className="shrink-0 border-b border-border/60 px-4 py-3">
  <div className="flex items-center justify-between">
    <div className="flex items-center gap-2">
      <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-surface text-foreground">
        <History className="h-4 w-4" />
      </div>
      <h3 className="text-sm font-semibold text-foreground">历史录制</h3>
    </div>
    <Button variant="ghost" size="sm" onClick={onToggle} title="关闭历史录制">
      <X className="h-4 w-4" />
    </Button>
  </div>
  <div className="mt-3 flex items-center justify-between">
    <p className="text-xs text-muted-foreground">共 {recordings.length} 条录制</p>
    <Button variant="ghost" size="sm" onClick={handleImport} title="导入录制">
      <Upload className="h-4 w-4" />
    </Button>
  </div>
</div>
```

Keep the existing `handleImport` function unchanged.

- [ ] **Step 3: Restyle the import error banner**

Replace the current import error JSX with:

```tsx
{importError && (
  <div className="mx-4 mt-3 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs leading-relaxed text-destructive">
    {importError}
  </div>
)}
```

- [ ] **Step 4: Replace the list state JSX**

Replace the current list container contents with this JSX. Keep the existing conditional order and the existing `recordings.map((rec) => ...)` data source:

```tsx
<div className="flex-1 overflow-y-auto px-3 py-3">
  {isLoading ? (
    <div className="flex h-full min-h-64 flex-col items-center justify-center gap-3 text-muted-foreground">
      <Loader2 className="h-5 w-5 animate-spin" />
      <p className="text-xs">加载历史录制...</p>
    </div>
  ) : loadError ? (
    <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-3 text-xs leading-relaxed text-destructive">
      {loadError}
    </div>
  ) : recordings.length === 0 ? (
    <div className="flex h-full min-h-64 flex-col items-center justify-center text-center text-muted-foreground">
      <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-lg bg-surface">
        <Film className="h-5 w-5 opacity-60" />
      </div>
      <p className="text-sm font-medium text-foreground">暂无历史录制</p>
      <p className="mt-1 text-xs">录制完成后会出现在这里</p>
    </div>
  ) : (
    <div className="space-y-1.5">
      {recordings.map((rec) => (
        <div
          key={rec.id}
          className={cn(
            'group relative flex min-h-16 cursor-pointer items-center gap-3 rounded-lg border border-transparent px-3 py-2.5',
            'bg-secondary/30 transition-all duration-150 hover:-translate-y-px hover:border-border/70 hover:bg-surface',
            'focus-within:border-border/70 focus-within:bg-surface',
          )}
          onClick={() => onSelectRecording(rec.id)}
        >
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-card text-muted-foreground group-hover:text-foreground">
            <Film className="h-4 w-4" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium text-foreground">
              {formatDate(rec.createdAt)}
            </p>
            <p className="mt-0.5 text-xs text-muted-foreground">
              时长：{formatDuration(rec.durationSecs)}
            </p>
          </div>
          <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground transition-opacity group-hover:opacity-0 group-focus-within:opacity-0" />
          <button
            onClick={(e) => {
              e.stopPropagation()
              void handleDelete(rec.id)
            }}
            disabled={deletingId === rec.id}
            className={cn(
              'absolute right-2 top-1/2 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-lg',
              'text-muted-foreground opacity-0 transition-all hover:bg-destructive/10 hover:text-destructive',
              'group-hover:opacity-100 group-focus-within:opacity-100 disabled:pointer-events-none',
            )}
            title="删除"
          >
            {deletingId === rec.id ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Trash2 className="h-3.5 w-3.5" />
            )}
          </button>
        </div>
      ))}
    </div>
  )}
</div>
```

- [ ] **Step 5: Confirm no logic code was changed**

Review the diff and verify these function bodies are unchanged except for JSX/class usage around them:

```tsx
const loadRecordings = useCallback(async () => {
  setIsLoading(true)
  setLoadError(null)
  try {
    const list = await listRecordings()
    setRecordings(list.sort((a, b) => b.createdAt - a.createdAt))
  } catch (e) {
    console.error('加载历史录制失败:', e)
    setLoadError(String(e))
  } finally {
    setIsLoading(false)
  }
}, [])
```

```tsx
const handleImport = async () => {
  setImportError(null)
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      multiple: false,
      filters: [{ name: '视频文件', extensions: ['mp4'] }],
    })
    if (!selected) return

    await importRecording(selected)
    await loadRecordings()
  } catch (e) {
    const msg = String(e)
    setImportError(msg)
    setTimeout(() => setImportError(null), 5000)
  }
}
```

```tsx
const handleDelete = async (id: string) => {
  setDeletingId(id)
  try {
    await deleteRecording(id)
    setRecordings((prev) => prev.filter((r) => r.id !== id))
  } catch (e) {
    console.error('删除录制失败:', e)
  } finally {
    setDeletingId(null)
  }
}
```

- [ ] **Step 6: Run the focused tests**

Run:

```bash
npm test -- src/components/recording-sidebar.test.tsx
```

Expected: all `RecordingSidebar` tests pass.

- [ ] **Step 7: Commit the passing test and UI change together**

```bash
git add src/components/recording-sidebar.tsx src/components/recording-sidebar.test.tsx
git commit -m "refactor(ui): 重构历史录制侧栏界面"
```

## Phase 3: Checklist and Full Verification

### Task 3: Add Manual Self-Test Checklist

**Files:**
- Create: `tests/2026-06-07-recording-history-sidebar-ui-checklist.md`

- [ ] **Step 1: Create the manual checklist**

Add this file:

```markdown
# 历史录制侧栏 UI 重构自测清单

日期：2026-06-07

## 自动验证

- [ ] `npm test -- src/components/recording-sidebar.test.tsx` 通过
- [ ] `npm test -- --run` 通过
- [ ] `npm run build` 通过
- [ ] `git diff --check` 通过

## 手动验证

- [ ] 空闲页点击历史录制按钮后，右侧侧栏以 360px 宽度展开
- [ ] 侧栏顶部显示“历史录制”和“共 N 条录制”
- [ ] 点击导入图标仍打开 mp4 文件选择器
- [ ] 导入错误时，顶部错误提示展示并在既有 5 秒逻辑后消失
- [ ] 有历史记录时，列表项显示日期、时长和进入箭头
- [ ] 点击记录日期或时长区域后，进入对应历史录制的预览美化界面
- [ ] 悬停记录时，右侧显示删除按钮
- [ ] 点击删除按钮时，不进入预览美化界面
- [ ] 删除中显示 loading 图标，删除完成后该条记录从列表移除
- [ ] 无历史记录时，显示“暂无历史录制”和“录制完成后会出现在这里”
- [ ] 加载历史记录失败时，侧栏显示错误提示
```

- [ ] **Step 2: Commit the checklist**

```bash
git add tests/2026-06-07-recording-history-sidebar-ui-checklist.md
git commit -m "docs(tests): 增加历史录制侧栏UI自测清单"
```

### Task 4: Run Full Verification

**Files:**
- Verify: `src/components/recording-sidebar.tsx`
- Verify: `src/components/recording-sidebar.test.tsx`
- Verify: `tests/2026-06-07-recording-history-sidebar-ui-checklist.md`

- [ ] **Step 1: Run all frontend tests**

```bash
npm test -- --run
```

Expected: all frontend tests pass.

- [ ] **Step 2: Run the frontend build**

```bash
npm run build
```

Expected: TypeScript and Vite build complete successfully.

- [ ] **Step 3: Check whitespace and patch hygiene**

```bash
git diff --check
```

Expected: no whitespace errors.

- [ ] **Step 4: Inspect the final diff for scope**

```bash
git diff --stat HEAD~2..HEAD
```

Expected: only these task files changed across the implementation commits:

```text
src/components/recording-sidebar.tsx
src/components/recording-sidebar.test.tsx
tests/2026-06-07-recording-history-sidebar-ui-checklist.md
```

- [ ] **Step 5: Report verification results**

Final response must include:

```text
已完成历史录制侧栏 UI 重构。
验证：
- npm test -- src/components/recording-sidebar.test.tsx
- npm test -- --run
- npm run build
- git diff --check
```

If any verification command fails, report the failing command, the important error lines, and the exact file that needs the next fix.
