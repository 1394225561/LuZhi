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

  const loadRecordings = useCallback(async () => {
    setIsLoading(true)
    try {
      const list = await listRecordings()
      setRecordings(list.sort((a, b) => b.createdAt - a.createdAt))
    } catch (e) {
      console.error('加载历史录制失败:', e)
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
          'fixed top-1/2 -translate-y-1/2 z-40 bg-card border border-border/50 rounded-l-lg p-2',
          'hover:bg-secondary transition-all duration-200',
          isOpen ? 'right-80' : 'right-0',
        )}
        title="历史录制"
      >
        <History className="w-4 h-4" />
      </button>

      {/* Sidebar panel */}
      <div
        className={cn(
          'fixed right-0 top-0 h-full bg-card border-l border-border/50',
          'transition-all duration-300 ease-in-out z-30 flex flex-col',
          isOpen ? 'w-80' : 'w-0 overflow-hidden',
        )}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-border/50 shrink-0">
          <h3 className="font-semibold text-sm">历史录制</h3>
          <div className="flex items-center gap-1">
            <Button variant="ghost" size="sm" onClick={handleImport} title="导入录制">
              <Upload className="w-4 h-4" />
            </Button>
            <Button variant="ghost" size="sm" onClick={onToggle}>
              <X className="w-4 h-4" />
            </Button>
          </div>
        </div>

        {/* Error banner */}
        {importError && (
          <div className="mx-3 mt-2 p-2 rounded-lg bg-destructive/10 border border-destructive/20 text-xs text-destructive">
            {importError}
          </div>
        )}

        {/* List */}
        <div className="flex-1 overflow-y-auto p-2">
          {isLoading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
            </div>
          ) : recordings.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
              <Film className="w-8 h-8 mb-2 opacity-40" />
              <p className="text-xs">暂无历史录制</p>
            </div>
          ) : (
            recordings.map((rec) => (
              <div
                key={rec.id}
                className="group relative bg-secondary/50 rounded-lg p-3 mb-2 hover:bg-secondary cursor-pointer"
                onClick={() => onSelectRecording(rec.id)}
              >
                <div className="flex justify-between items-start">
                  <div>
                    <p className="text-sm font-medium">{formatDate(rec.createdAt)}</p>
                    <p className="text-xs text-muted-foreground">
                      时长: {formatDuration(rec.durationSecs)}
                    </p>
                  </div>
                  <ChevronRight className="w-4 h-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
                </div>

                {/* Delete button */}
                <button
                  onClick={(e) => {
                    e.stopPropagation()
                    void handleDelete(rec.id)
                  }}
                  disabled={deletingId === rec.id}
                  className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive transition-all p-1"
                  title="删除"
                >
                  {deletingId === rec.id ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <Trash2 className="w-3 h-3" />
                  )}
                </button>
              </div>
            ))
          )}
        </div>
      </div>
    </>
  )
}
