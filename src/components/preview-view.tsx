import { useState } from 'react'
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
  exportVideo,
  setBeautifyConfig,
  type BeautifyConfig,
  type ExportPreset,
  type RecordingResult,
} from '@/lib/tauri'

interface PreviewViewProps {
  onBack: () => void
  recordingResult?: RecordingResult | null
}

export function PreviewView({ onBack, recordingResult }: PreviewViewProps) {
  const [isPlaying, setIsPlaying] = useState(false)
  const [currentTime, setCurrentTime] = useState(45)
  const [duration] = useState(180)
  const [volume, setVolume] = useState(80)

  // AI Beautification settings
  const [cursorMagnification, setCursorMagnification] = useState(true)
  const [magnificationFactor, setMagnificationFactor] = useState([2])
  const [cursorSmoothing, setCursorSmoothing] = useState(true)
  const [autoTrimSilences, setAutoTrimSilences] = useState(false)
  const [trimSensitivity, setTrimSensitivity] = useState<'low' | 'medium' | 'high'>('medium')

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

  const handleBeautifyChange = (config: Partial<BeautifyConfig>) => {
    void setBeautifyConfig(currentBeautifyConfig(config))
      .then(() => buildCursorEffectTimeline())
      .catch((error) => {
        console.error('光标效果处理失败', error)
      })
  }

  const handleExport = (preset: ExportPreset) => {
    void exportVideo(preset).catch((error) => {
      console.error('导出失败', error)
    })
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
      className="flex h-screen bg-background" data-tauri-drag-region="deep"
    >
      {/* Main Preview Area */}
      <div className="flex-1 flex flex-col p-6">
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <button
            onClick={onBack}
            className="text-muted-foreground hover:text-foreground transition-colors text-sm flex items-center gap-2"
          >
            <SkipBack className="w-4 h-4" />
            返回录制
          </button>
          <h1 className="text-lg font-semibold text-foreground">预览与美化</h1>
          <div className="w-20" />
        </div>

        {/* Video Preview */}
        <div className="flex-1 bg-card rounded-2xl border border-border/50 overflow-hidden flex flex-col">
          {/* Video Area */}
          <div className="flex-1 bg-black/50 flex items-center justify-center relative">
            {recordingResult?.outputPath ? (
              <video
                src={`asset://localhost/${recordingResult.outputPath}`}
                controls
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
                <p className="text-xs opacity-40 mt-2">视频编码尚未实现（FFmpeg 集成待完成）</p>
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
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full">
                  <SkipBack className="w-4 h-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-10 w-10 rounded-full bg-surface hover:bg-surface-hover text-foreground"
                  onClick={() => setIsPlaying(!isPlaying)}
                >
                  {isPlaying ? (
                    <Pause className="w-5 h-5" />
                  ) : (
                    <Play className="w-5 h-5 ml-0.5" />
                  )}
                </Button>
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full">
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
      <div className="w-[340px] border-l border-border/50 bg-card/30 p-5 overflow-y-auto">
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
