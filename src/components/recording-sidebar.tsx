import { useState, useEffect, useCallback } from 'react'
import { History, X, Upload, Trash2, ChevronRight, Loader2, Film } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import {
  listRecordings,
  deleteRecording,
  importRecording,
  type LibraryEntrySummary,
} from '@/lib/tauri'

interface RecordingSidebarProps {
  isOpen: boolean
  onToggle: () => void
  onSelectRecording: (id: string) => void
}

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60)
  const secs = Math.floor(seconds % 60)
  return `${mins}:${secs.toString().padStart(2, '0')}`
}

function formatDate(timestamp: number): string {
  const date = new Date(timestamp)
  return date.toLocaleDateString('zh-CN', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function RecordingSidebar({
  isOpen,
  onToggle,
  onSelectRecording,
}: RecordingSidebarProps) {
  const [recordings, setRecordings] = useState<LibraryEntrySummary[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [importError, setImportError] = useState<string | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)

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

  useEffect(() => {
    if (isOpen) {
      void loadRecordings()
    }
  }, [isOpen, loadRecordings])

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

  return (
    <>
      {/* Toggle button — always visible */}
      <button
        onClick={onToggle}
        className={cn(
          'absolute -top-8 z-40 rounded-t-lg rounded-b-none bg-surface/95 p-2 text-muted-foreground outline-none',
          'hover:bg-surface-hover hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/40 transition-[right] duration-200',
          isOpen ? 'right-[calc(360px+0.5rem)] delay-200' : 'right-2 delay-0',
        )}
        title="历史录制"
      >
        <History className="w-4 h-4" />
      </button>

      {/* Sidebar panel */}
      <div
        className={cn(
          'fixed right-0 top-0 h-full bg-card/98',
          'transition-all duration-300 ease-in-out z-30 flex flex-col',
          isOpen
            ? 'w-[360px] border-l-0 shadow-none'
            : 'w-0 overflow-hidden border-l-0 shadow-none pointer-events-none',
        )}
      >
        {/* Header */}
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

        {/* Error banner */}
        {importError && (
          <div className="mx-4 mt-3 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs leading-relaxed text-destructive">
            {importError}
          </div>
        )}

        {/* List */}
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
      </div>
    </>
  )
}
