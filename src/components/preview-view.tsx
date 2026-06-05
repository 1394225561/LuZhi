import { useEffect, useRef, useState } from 'react'
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Volume2,
  Maximize2,
  MousePointer2,
  Sparkles,
  Scissors,
  Download,
} from 'lucide-react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { Slider } from '@/components/ui/slider'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import { motion } from 'framer-motion'
import {
  buildCursorEffectTimeline,
  buildCutTimeline,
  cancelExport,
  exportVideo,
  getBeautifyConfig,
  onExportProgress,
  setBeautifyConfig,
  type BeautifyConfig,
  type ExportPreset,
  type ExportProgressPayload,
  type ExportSummary,
  type LicenseStatus as LicenseStatusPayload,
  type RecordingResult,
} from '@/lib/tauri'
import { LicenseStatus } from '@/components/license-status'

interface PreviewViewProps {
  onBack: () => void
  recordingResult?: RecordingResult | null
  licenseStatus?: LicenseStatusPayload | null
}

function messageForBeautifyError(error: unknown, fallback: string): string {
  const msg = String(error)
  if (
    msg.includes('配置已变更，构建已取消') ||
    msg.includes('录制会话已变更，光标效果构建已取消') ||
    msg.includes('裁剪配置已变更，构建已取消') ||
    msg.includes('录制会话已变更，裁剪时间线构建已取消')
  ) {
    return ''
  }
  return msg.includes('已录入系统光标') ||
    msg.includes('光标元数据为空') ||
    msg.includes('裁剪元数据') ||
    msg.includes('裁剪时间线')
    ? msg
    : fallback
}

export function PreviewView({ onBack, recordingResult, licenseStatus }: PreviewViewProps) {
  const videoRef = useRef<HTMLVideoElement>(null)
  const videoContainerRef = useRef<HTMLDivElement>(null)
  const isSeekingRef = useRef(false)
  const [isPlaying, setIsPlaying] = useState(false)
  const [currentTime, setCurrentTime] = useState(0)
  const [duration, setDuration] = useState(0)
  const [volume, setVolume] = useState(80)

  // AI Beautification settings
  const [cursorMagnification, setCursorMagnification] = useState(true)
  const [magnificationFactor, setMagnificationFactor] = useState([2])
  const [cursorSmoothing, setCursorSmoothing] = useState(true)
  const [autoTrimSilences, setAutoTrimSilences] = useState(false)
  const [trimSensitivity, setTrimSensitivity] = useState<'low' | 'medium' | 'high'>('medium')
  const [exportSummary, setExportSummary] = useState<ExportSummary | null>(null)
  const [exportProgress, setExportProgress] = useState<ExportProgressPayload | null>(null)
  const [isExporting, setIsExporting] = useState(false)

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60)
    const secs = Math.floor(seconds % 60)
    return `${mins}:${secs.toString().padStart(2, '0')}`
  }

  const currentBeautifyConfig = (patch: Partial<BeautifyConfig>): BeautifyConfig => ({
    cursorMagnification,
    magnificationFactor: magnificationFactor[0],
    cursorSmoothing,
    autoTrimSilences,
    trimSensitivity,
    ...patch,
  })

  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const pendingConfigRef = useRef<BeautifyConfig | null>(null)
  const writeChainRef = useRef<Promise<void>>(Promise.resolve())
  const buildSeqRef = useRef(0)
  const exportRevisionRef = useRef(0)
  const [beautifyError, setBeautifyError] = useState<string | null>(null)

  // Initialize beautify state from backend on mount so the UI reflects the
  // persistent config rather than hardcoded defaults that may have drifted
  // from what was set in a previous preview session.
  useEffect(() => {
    let unlisten: (() => void) | undefined
    void onExportProgress((payload) => {
      setExportProgress(payload)
      if (!payload.cancellable && payload.error) {
        // Terminal error: show error details, clear exporting state.
        setIsExporting(false)
        setBeautifyError(payload.error)
      } else {
        setIsExporting(payload.cancellable && payload.progress < 100)
      }
    }).then((fn) => { unlisten = fn })
    return () => { unlisten?.() }
  }, [])

  useEffect(() => {
    let cancelled = false
    getBeautifyConfig()
      .then((cfg) => {
        if (cancelled) return
        setCursorMagnification(cfg.cursorMagnification)
        setMagnificationFactor([cfg.magnificationFactor])
        setCursorSmoothing(cfg.cursorSmoothing)
        setAutoTrimSilences(cfg.autoTrimSilences)
        setTrimSensitivity(cfg.trimSensitivity)
      })
      .catch((err) => {
        console.error('读取美化配置失败，使用默认值', err)
      })
    return () => {
      cancelled = true
    }
  }, [])

  // Sync video element events with React state
  useEffect(() => {
    const video = videoRef.current
    if (!video) return

    const onLoadedMetadata = () => setDuration(video.duration)
    const onTimeUpdate = () => {
      if (!isSeekingRef.current) {
        setCurrentTime(video.currentTime)
      }
    }
    const onPlay = () => setIsPlaying(true)
    const onPause = () => setIsPlaying(false)
    const onEnded = () => setIsPlaying(false)

    video.addEventListener('loadedmetadata', onLoadedMetadata)
    video.addEventListener('timeupdate', onTimeUpdate)
    video.addEventListener('play', onPlay)
    video.addEventListener('pause', onPause)
    video.addEventListener('ended', onEnded)

    return () => {
      video.removeEventListener('loadedmetadata', onLoadedMetadata)
      video.removeEventListener('timeupdate', onTimeUpdate)
      video.removeEventListener('play', onPlay)
      video.removeEventListener('pause', onPause)
      video.removeEventListener('ended', onEnded)
    }
  }, [])

  // Flush any pending beautify config on unmount so user's last changes
  // are not silently discarded when navigating away from Preview.
  useEffect(() => {
    return () => {
      if (debounceRef.current) {
        clearTimeout(debounceRef.current)
        debounceRef.current = null
      }
      if (pendingConfigRef.current) {
        const config = pendingConfigRef.current
        pendingConfigRef.current = null
        void enqueueConfigWrite(config).catch((err) => {
          console.error('退出预览时配置保存失败', err)
        })
      }
    }
  }, [])

  // Serialize all setBeautifyConfig calls through a promise chain so that
  // writes always land in user-intention order, even when a debounce-triggered
  // write is still in-flight when a flush/export/back occurs.
  const enqueueConfigWrite = (config: BeautifyConfig): Promise<void> => {
    const write = writeChainRef.current
      .catch((err) => { if (err) console.error('配置写入链中前序写入失败', err) })
      .then(() => setBeautifyConfig(config).then(() => {}))
    writeChainRef.current = write
    return write
  }

  // Immediately flush the latest pending beautify config to backend.
  // Enqueues the pending write onto the chain and then awaits all
  // previously-enqueued writes to complete in order.
  // Does NOT swallow errors: callers must handle rejection to avoid
  // proceeding with stale backend config.
  const flushPendingConfig = (): Promise<void> => {
    if (debounceRef.current) {
      clearTimeout(debounceRef.current)
      debounceRef.current = null
    }
    if (pendingConfigRef.current) {
      enqueueConfigWrite(pendingConfigRef.current)
      pendingConfigRef.current = null
    }
    return writeChainRef.current
  }

  const handleBeautifyChange = (config: Partial<BeautifyConfig>) => {
    exportRevisionRef.current += 1
    setExportSummary(null)
    const nextConfig = currentBeautifyConfig(config)
    pendingConfigRef.current = nextConfig

    if (debounceRef.current) {
      clearTimeout(debounceRef.current)
    }
    debounceRef.current = setTimeout(() => {
      debounceRef.current = null
      pendingConfigRef.current = null
      const seq = ++buildSeqRef.current
      void enqueueConfigWrite(nextConfig)
        .then(() => {
          setBeautifyError(null)
          return buildCursorEffectTimeline().then(() => {
            if (nextConfig.autoTrimSilences) {
              return buildCutTimeline()
            }
          })
        })
        .then(() => {
          if (seq === buildSeqRef.current) {
            setBeautifyError(null)
          }
        })
        .catch((error) => {
          const msg = messageForBeautifyError(error, '光标效果处理失败，请重试或重新录制。')
          if (seq !== buildSeqRef.current || !msg) {
            return
          }
          console.error('光标效果处理失败', error)
          setBeautifyError(msg)
        })
    }, 300)
  }

  const handleExport = (preset: ExportPreset) => {
    const revision = exportRevisionRef.current
    setIsExporting(true)
    setExportProgress({ preset, progress: 0, cancellable: true, outputPath: null })
    void flushPendingConfig()
      .then(() => exportVideo(preset))
      .then((summary) => {
        if (revision !== exportRevisionRef.current) return
        setBeautifyError(null)
        setExportSummary(summary)
      })
      .catch((error) => {
        const msg = messageForBeautifyError(error, '导出失败，请重试或检查录制素材。')
        if (!msg) return
        console.error('导出失败', error)
        setBeautifyError(msg)
      })
      .finally(() => {
        setIsExporting(false)
      })
  }

  const handleCancelExport = () => {
    void cancelExport().catch((error) => {
      console.error('取消导出失败', error)
    })
  }

  const handleBack = () => {
    void flushPendingConfig()
      .then(() => onBack())
      .catch((error) => {
        const msg = messageForBeautifyError(error, '美化配置保存失败，请重试或恢复设置后再返回录制。')
        if (!msg) return
        console.error('保存美化配置失败', error)
        setBeautifyError(msg)
      })
  }

  const handlePlayPause = () => {
    const video = videoRef.current
    if (!video) return
    if (video.paused) {
      void video.play()
    } else {
      video.pause()
    }
  }

  const handleSkipBack = () => {
    const video = videoRef.current
    if (!video) return
    video.currentTime = Math.max(0, video.currentTime - 10)
  }

  const handleSkipForward = () => {
    const video = videoRef.current
    if (!video) return
    video.currentTime = Math.min(video.duration, video.currentTime + 10)
  }

  const exportPresets: Array<{
    id: ExportPreset
    name: string
    specs: string
  }> = [
    { id: 'bilibili', name: 'Bilibili', specs: '16:9 · 1080p' },
    { id: 'douyin', name: '抖音', specs: '9:16 · 竖屏' },
    { id: 'xiaohongshu', name: '小红书', specs: '1:1 · 方形' },
  ]

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 }}
      className="flex h-screen bg-background"
    >
      {/* Main Preview Area */}
      <div className="flex-1 flex flex-col p-6" data-luzhi-drag-region="preview-main">
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <button
            onClick={handleBack}
            className="text-muted-foreground hover:text-foreground transition-colors text-sm flex items-center gap-2"
          >
            <SkipBack className="w-4 h-4" />
            返回录制
          </button>
          <h1 className="text-lg font-semibold text-foreground">预览与美化</h1>
          <div className="shrink-0">
            <LicenseStatus status={licenseStatus ?? null} />
          </div>
        </div>

        {/* Video Preview */}
        <div className="flex-1 bg-card rounded-2xl border border-border/50 overflow-hidden flex flex-col">
          {/* Video Area */}
          <div className="flex-1 bg-black/50 flex items-center justify-center relative" ref={videoContainerRef}>
            {recordingResult?.outputPath ? (
              <video
                ref={videoRef}
                src={convertFileSrc(recordingResult.outputPath)}
                playsInline
                className="w-full h-full object-contain"
              />
            ) : (
              <div className="text-center text-muted-foreground">
                <p className="text-sm mb-1">录制完成</p>
                {recordingResult && (
                  <p className="text-xs opacity-60">
                    已捕获 {recordingResult.frameCount} 帧
                    {recordingResult.durationSecs > 0 && ` · ${recordingResult.durationSecs}s`}
                  </p>
                )}
                <p className="text-xs opacity-40 mt-2">
                  当前构建未启用 FFmpeg，无法生成可播放文件。请使用 npm run tauri:dev:ffmpeg
                </p>
              </div>
            )}
            {cursorMagnification && (
              <motion.div
                initial={{ scale: 1 }}
                animate={{ scale: 1.1 }}
                transition={{ duration: 0.5, repeat: Infinity, repeatType: 'reverse' }}
                className="absolute bottom-1/3 right-1/3 w-6 h-6 rounded-full border-2 border-muted-foreground/50 bg-muted-foreground/10"
              />
            )}
          </div>

          {/* Video Controls */}
          <div className="p-4 border-t border-border/50 bg-card/50 backdrop-blur-sm">
            {/* Progress Bar */}
            <div className="mb-3">
              <Slider
                value={[currentTime]}
                max={duration}
                step={1}
                onValueChange={(value) => setCurrentTime(value[0])}
                className="w-full"
              />
              <div className="flex justify-between text-xs text-muted-foreground mt-1 font-mono">
                <span>{formatTime(currentTime)}</span>
                <span>{formatTime(duration)}</span>
              </div>
            </div>

            {/* Playback Controls */}
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full" onClick={handleSkipBack}>
                  <SkipBack className="w-4 h-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-10 w-10 rounded-full bg-surface hover:bg-surface-hover text-foreground"
                  onClick={handlePlayPause}
                >
                  {isPlaying ? (
                    <Pause className="w-5 h-5" />
                  ) : (
                    <Play className="w-5 h-5 ml-0.5" />
                  )}
                </Button>
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full" onClick={handleSkipForward}>
                  <SkipForward className="w-4 h-4" />
                </Button>
              </div>

              <div className="flex items-center gap-3">
                <div className="flex items-center gap-2">
                  <Volume2 className="w-4 h-4 text-muted-foreground" />
                  <Slider
                    value={[volume]}
                    max={100}
                    step={1}
                    onValueChange={(value) => setVolume(value[0])}
                    className="w-24"
                  />
                </div>
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full">
                  <Maximize2 className="w-4 h-4" />
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Right Sidebar */}
      <div className="w-[340px] border-l border-border/50 bg-card/30 p-5 overflow-y-auto" data-luzhi-drag-region="preview-sidebar">
        {/* AI Beautification Section */}
        <div className="mb-8">
          <div className="flex items-center gap-2 mb-4">
            <Sparkles className="w-4 h-4 text-foreground" />
            <h2 className="text-sm font-semibold text-foreground uppercase tracking-wide">
              AI 美化
            </h2>
          </div>

          <div className="space-y-5">
            {/* Cursor Magnification */}
            <div className="bg-secondary/30 rounded-xl p-4">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <MousePointer2 className="w-4 h-4 text-muted-foreground" />
                  <span className="text-sm text-foreground">光标放大</span>
                </div>
                <Switch
                  checked={cursorMagnification}
                  onCheckedChange={(checked) => {
                    setCursorMagnification(checked)
                    handleBeautifyChange({ cursorMagnification: checked })
                  }}
                />
              </div>
              {cursorMagnification && (
                <motion.div
                  initial={{ opacity: 0, height: 0 }}
                  animate={{ opacity: 1, height: 'auto' }}
                  exit={{ opacity: 0, height: 0 }}
                  className="pt-2 border-t border-border/50"
                >
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-xs text-muted-foreground">放大倍数</span>
                    <span className="text-xs font-mono text-foreground">
                      {magnificationFactor[0]}x
                    </span>
                  </div>
                  <Slider
                    value={magnificationFactor}
                    min={1}
                    max={3}
                    step={0.5}
                    onValueChange={(value) => {
                      setMagnificationFactor(value)
                      handleBeautifyChange({ magnificationFactor: value[0] })
                    }}
                    className="w-full"
                  />
                </motion.div>
              )}
            </div>

            {/* Cursor Smoothing */}
            <div className="bg-secondary/30 rounded-xl p-4">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <MousePointer2 className="w-4 h-4 text-muted-foreground" />
                  <span className="text-sm text-foreground">光标平滑</span>
                </div>
                <Switch
                  checked={cursorSmoothing}
                  onCheckedChange={(checked) => {
                    setCursorSmoothing(checked)
                    handleBeautifyChange({ cursorSmoothing: checked })
                  }}
                />
              </div>
            </div>

            {/* Auto-trim Silences */}
            <div className="bg-secondary/30 rounded-xl p-4">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <Scissors className="w-4 h-4 text-muted-foreground" />
                  <span className="text-sm text-foreground">自动剪除静音</span>
                </div>
                <Switch
                  checked={autoTrimSilences}
                  onCheckedChange={(checked) => {
                    setAutoTrimSilences(checked)
                    handleBeautifyChange({ autoTrimSilences: checked })
                  }}
                />
              </div>
              {autoTrimSilences && (
                <motion.div
                  initial={{ opacity: 0, height: 0 }}
                  animate={{ opacity: 1, height: 'auto' }}
                  exit={{ opacity: 0, height: 0 }}
                  className="pt-2 border-t border-border/50"
                >
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-muted-foreground">灵敏度</span>
                    <Select
                      value={trimSensitivity}
                      onValueChange={(value: 'low' | 'medium' | 'high') => {
                        setTrimSensitivity(value)
                        handleBeautifyChange({ trimSensitivity: value })
                      }}
                    >
                      <SelectTrigger className="w-24 h-8 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="low">低</SelectItem>
                        <SelectItem value="medium">中</SelectItem>
                        <SelectItem value="high">高</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </motion.div>
              )}
            </div>
          </div>
        </div>

        {/* Export Section */}
        <div>
          {beautifyError && (
            <div className="mb-3 rounded-lg border border-destructive/50 bg-destructive/10 p-3 text-xs text-destructive">
              {beautifyError}
            </div>
          )}
          {exportProgress && isExporting && (
            <div className="mb-3 rounded-lg border border-border/50 bg-secondary/30 p-3 text-xs text-muted-foreground">
              <div className="mb-2 flex items-center justify-between">
                <span>正在导出 {exportProgress.progress}%</span>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-7 px-2 text-xs"
                  onClick={handleCancelExport}
                >
                  取消导出
                </Button>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full bg-foreground transition-all"
                  style={{ width: `${Math.max(0, Math.min(exportProgress.progress, 100))}%` }}
                />
              </div>
            </div>
          )}
          {exportSummary && (
            <div className="mb-3 rounded-lg border border-border/50 bg-secondary/30 p-3 text-xs text-muted-foreground space-y-1">
              {exportSummary.cutCount > 0 ? (
                <p>
                  已生成裁剪时间线：{exportSummary.cutCount} 段，预计剪除{' '}
                  {Math.round(exportSummary.totalCutNanos / 1_000_000_000)} 秒
                </p>
              ) : (
                <p>未检测到可裁剪空白段</p>
              )}
              {exportSummary.outputPath ? (
                <div>
                  <p className="opacity-80 mb-1">✅ 已生成可播放导出文件</p>
                  <p className="opacity-60 break-all font-mono text-[10px]">
                    {exportSummary.outputPath}
                  </p>
                </div>
              ) : (
                <p className="opacity-60">FFmpeg 编码器接入后将生成可播放文件</p>
              )}
            </div>
          )}
          <div className="flex items-center gap-2 mb-4">
            <Download className="w-4 h-4 text-foreground" />
            <h2 className="text-sm font-semibold text-foreground uppercase tracking-wide">
              导出
            </h2>
          </div>

          <div className="space-y-3">
            {exportPresets.map((preset) => (
              <div
                key={preset.id}
                className={cn(
                  'relative overflow-hidden rounded-xl border border-border/50 p-4 cursor-pointer transition-all duration-200',
                  'bg-secondary/30 hover:border-border hover:scale-[1.02] active:scale-[0.98]',
                )}
              >
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-sm font-medium text-foreground">{preset.name}</p>
                    <p className="text-xs text-muted-foreground">{preset.specs}</p>
                  </div>
                  <Button
                    size="sm"
                    disabled={isExporting}
                    className="h-8 px-4 rounded-lg bg-surface hover:bg-surface-hover text-foreground border border-border/50"
                    onClick={() => handleExport(preset.id)}
                  >
                    导出
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </motion.div>
  )
}
