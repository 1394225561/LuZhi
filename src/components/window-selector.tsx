import { useState, useEffect } from 'react'
import { Monitor, Loader2, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import { listWindows, type WindowInfo } from '@/lib/tauri'

interface WindowSelectorProps {
  isOpen: boolean
  onSelect: (window: WindowInfo) => void
  onClose: () => void
}

export function WindowSelector({ isOpen, onSelect, onClose }: WindowSelectorProps) {
  const [windows, setWindows] = useState<WindowInfo[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (isOpen) {
      setLoading(true)
      listWindows()
        .then(setWindows)
        .catch(() => setWindows([]))
        .finally(() => setLoading(false))
    }
  }, [isOpen])

  if (!isOpen) return null

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="bg-background rounded-2xl shadow-xl max-w-2xl w-full mx-4 max-h-[80vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b">
          <h2 className="text-lg font-semibold">选择录制窗口</h2>
          <button
            onClick={onClose}
            className="p-1 rounded-lg hover:bg-secondary transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
            </div>
          ) : windows.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
              <Monitor className="w-12 h-12 mb-4" />
              <p>未找到可用窗口</p>
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-3">
              {windows.map((window) => (
                <WindowCard
                  key={window.windowId}
                  window={window}
                  onSelect={() => onSelect(window)}
                />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

function WindowCard({
  window,
  onSelect,
}: {
  window: WindowInfo
  onSelect: () => void
}) {
  return (
    <button
      onClick={onSelect}
      disabled={!window.isOnScreen}
      className={cn(
        'flex flex-col gap-2 p-3 rounded-xl border transition-all duration-200 text-left',
        'hover:bg-surface-hover hover:border-border',
        'disabled:opacity-50 disabled:cursor-not-allowed'
      )}
    >
      {/* 缩略图 */}
      {window.thumbnail ? (
        <img
          src={`data:image/png;base64,${window.thumbnail}`}
          className="w-full h-24 object-cover rounded-lg"
          alt={window.title}
        />
      ) : (
        <div className="w-full h-24 bg-secondary rounded-lg flex items-center justify-center">
          <Monitor className="w-8 h-8 text-muted-foreground" />
        </div>
      )}

      {/* 应用信息 */}
      <div className="flex items-center gap-2">
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium truncate">{window.title}</p>
          <p className="text-xs text-muted-foreground truncate">{window.appName}</p>
        </div>
      </div>

      {/* 状态指示 */}
      {!window.isOnScreen && (
        <span className="text-xs text-amber-500">窗口已最小化</span>
      )}
    </button>
  )
}
