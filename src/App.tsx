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
