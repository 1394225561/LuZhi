import { Monitor, AppWindow, Square, Volume2, Mic, Circle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import { motion } from 'framer-motion'

interface ResolutionOption {
  width: number
  height: number
  label: string
}

const RESOLUTION_OPTIONS: ResolutionOption[] = [
  { width: 1920, height: 1080, label: '1080p (1920×1080)' },
  { width: 1280, height: 720, label: '720p (1280×720)' },
  { width: 3840, height: 2160, label: '4K (3840×2160) 实验性' },
]

const FPS_OPTIONS = [30, 60]

interface RecordingPanelProps {
  recordingMode: 'fullscreen' | 'window' | 'area'
  setRecordingMode: (mode: 'fullscreen' | 'window' | 'area') => void
  systemAudioEnabled: boolean
  setSystemAudioEnabled: (enabled: boolean) => void
  micEnabled: boolean
  setMicEnabled: (enabled: boolean) => void
  micVolume: number
  onStartRecording: () => void
  resolution: ResolutionOption
  setResolution: (res: ResolutionOption) => void
  fps: number
  setFps: (fps: number) => void
}

export function RecordingPanel({
  recordingMode,
  setRecordingMode,
  systemAudioEnabled,
  setSystemAudioEnabled,
  micEnabled,
  setMicEnabled,
  micVolume,
  onStartRecording,
  resolution,
  setResolution,
  fps,
  setFps,
}: RecordingPanelProps) {
  const modes = [
    { id: 'fullscreen' as const, icon: Monitor, label: '全屏' },
    { id: 'window' as const, icon: AppWindow, label: '窗口' },
    { id: 'area' as const, icon: Square, label: '区域' },
  ]

  const isNonFullscreen = recordingMode !== 'fullscreen'

  return (
    <motion.div
      initial={{ opacity: 0, y: 20, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="bg-card/95 border border-border/50 rounded-2xl p-5 w-[320px]"
    >
      {/* Header */}
      <div className="flex items-center justify-between mb-5">
        <div className="flex items-center gap-2">
          <div className="w-8 h-8 rounded-lg bg-surface flex items-center justify-center">
            <Circle className="w-4 h-4 text-foreground" />
          </div>
          <span className="font-semibold text-foreground">录制</span>
        </div>
      </div>

      {/* Mode Selection */}
      <div className="mb-4">
        <p className="text-xs text-muted-foreground mb-2.5 uppercase tracking-wide">录制模式</p>
        <div className="flex gap-2">
          {modes.map((mode) => (
            <motion.button
              key={mode.id}
              whileHover={{ scale: 1.02 }}
              whileTap={{ scale: 0.98 }}
              onClick={() => setRecordingMode(mode.id)}
              className={cn(
                'flex-1 flex flex-col items-center gap-1.5 py-3 px-2 rounded-xl transition-all duration-200',
                recordingMode === mode.id
                  ? 'bg-surface border border-border text-foreground'
                  : 'bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground',
              )}
            >
              <mode.icon className="w-5 h-5" />
              <span className="text-xs font-medium">{mode.label}</span>
            </motion.button>
          ))}
        </div>
        {isNonFullscreen && (
          <p className="text-xs text-amber-400 mt-2.5 text-center">
            该模式正在开发中，将随后续版本推出
          </p>
        )}
      </div>

      {/* Resolution & FPS */}
      <div className="mb-4">
        <p className="text-xs text-muted-foreground mb-2.5 uppercase tracking-wide">画面参数</p>
        <div className="flex gap-2">
          <Select
            value={`${resolution.width}x${resolution.height}`}
            onValueChange={(val) => {
              const [w, h] = val.split('x').map(Number)
              const match = RESOLUTION_OPTIONS.find((r) => r.width === w && r.height === h)
              if (match) setResolution(match)
            }}
          >
            <SelectTrigger className="flex-1 h-9 text-xs">
              <SelectValue placeholder="分辨率" />
            </SelectTrigger>
            <SelectContent>
              {RESOLUTION_OPTIONS.map((r) => (
                <SelectItem key={`${r.width}x${r.height}`} value={`${r.width}x${r.height}`}>
                  {r.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select value={String(fps)} onValueChange={(val) => setFps(Number(val))}>
            <SelectTrigger className="w-20 h-9 text-xs">
              <SelectValue placeholder="FPS" />
            </SelectTrigger>
            <SelectContent>
              {FPS_OPTIONS.map((f) => (
                <SelectItem key={f} value={String(f)}>
                  {f} fps
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Audio Controls */}
      <div className="mb-6">
        <p className="text-xs text-muted-foreground mb-2.5 uppercase tracking-wide">音频设置</p>
        <div className="flex gap-2">
          {/* System Audio */}
          <motion.button
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={() => setSystemAudioEnabled(!systemAudioEnabled)}
            className={cn(
              'flex-1 flex items-center justify-center gap-2 py-3 px-3 rounded-xl transition-all duration-200',
              systemAudioEnabled
                ? 'bg-surface border border-border text-foreground'
                : 'bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground',
            )}
          >
            <Volume2 className="w-4 h-4" />
            <span className="text-xs font-medium">系统音频</span>
          </motion.button>

          {/* Microphone with volume indicator */}
          <motion.button
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={() => setMicEnabled(!micEnabled)}
            className={cn(
              'flex-1 flex items-center justify-center gap-2 py-3 px-3 rounded-xl transition-all duration-200 relative overflow-hidden',
              micEnabled
                ? 'bg-surface border border-border text-foreground'
                : 'bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground',
            )}
          >
            <Mic className="w-4 h-4" />
            <span className="text-xs font-medium">麦克风</span>
            {micEnabled && (
              <div className="absolute bottom-1 left-1/2 -translate-x-1/2 flex gap-0.5">
                {[...Array(5)].map((_, i) => (
                  <motion.div
                    key={i}
                    initial={{ height: 2 }}
                    animate={{ height: i < Math.ceil(micVolume / 20) ? 6 + i * 2 : 2 }}
                    transition={{ duration: 0.1 }}
                    className="w-1 rounded-full bg-muted-foreground/60"
                  />
                ))}
              </div>
            )}
          </motion.button>
        </div>
      </div>

      {/* Start Recording Button */}
      <Button
        onClick={isNonFullscreen ? undefined : onStartRecording}
        disabled={isNonFullscreen}
        className="w-full h-12 rounded-xl bg-primary hover:bg-primary/90 text-primary-foreground font-semibold text-base transition-all duration-200 active:scale-[0.99] disabled:opacity-50 disabled:cursor-not-allowed"
      >
        <Circle className="w-4 h-4 mr-2 fill-current" />
        {isNonFullscreen ? '即将推出' : '开始录制'}
      </Button>
    </motion.div>
  )
}
