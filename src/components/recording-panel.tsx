import { useState, useEffect } from 'react'
import { Monitor, AppWindow, Square, Volume2, Mic, Circle, AlertTriangle, AudioWaveform } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'
import { motion } from 'framer-motion'
import { listMicrophoneDevices, setWindowId, type MicrophoneDeviceInfo, type WindowInfo } from '@/lib/tauri'
import { WindowSelector } from './window-selector'

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
  micDevice: string | null
  setMicDevice: (device: string | null) => void
  micVolume: number
  denoiseEnabled: boolean
  onDenoiseChange: (enabled: boolean) => void
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
  micDevice,
  setMicDevice,
  micVolume,
  denoiseEnabled,
  onDenoiseChange,
  onStartRecording,
  resolution,
  setResolution,
  fps,
  setFps,
}: RecordingPanelProps) {
  const [micDevices, setMicDevices] = useState<MicrophoneDeviceInfo[]>([])
  const [selectedWindow, setSelectedWindow] = useState<WindowInfo | null>(null)
  const [showWindowSelector, setShowWindowSelector] = useState(false)

  // Load microphone devices when mic is enabled.
  useEffect(() => {
    if (micEnabled) {
      listMicrophoneDevices()
        .then(setMicDevices)
        .catch(() => setMicDevices([]))
    }
  }, [micEnabled])

  // Find the selected device to check if it's Bluetooth.
  const selectedDevice = micDevices.find((d) => d.name === micDevice)
  const isBluetoothMic = selectedDevice?.isBluetooth ?? false
  const hasBluetoothDevice = micDevices.some((d) => d.isBluetooth)

  const handleModeChange = (mode: 'fullscreen' | 'window' | 'area') => {
    setRecordingMode(mode)

    if (mode === 'window') {
      setShowWindowSelector(true)
    }
  }

  const handleWindowSelect = async (window: WindowInfo) => {
    setSelectedWindow(window)
    setShowWindowSelector(false)

    try {
      await setWindowId(window.windowId)
    } catch (error) {
      console.error('设置窗口失败:', error)
    }
  }

  const canStartRecording = recordingMode === 'window' ? selectedWindow !== null : recordingMode !== 'area'

  const modes = [
    { id: 'fullscreen' as const, icon: Monitor, label: '全屏' },
    { id: 'window' as const, icon: AppWindow, label: '窗口' },
    { id: 'area' as const, icon: Square, label: '区域' },
  ]

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
              onClick={() => handleModeChange(mode.id)}
              disabled={mode.id === 'area'}
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
        {recordingMode === 'window' && selectedWindow && (
          <div className="mt-3 p-3 rounded-xl bg-surface border">
            <div className="flex items-center gap-2">
              {selectedWindow.thumbnail ? (
                <img
                  src={`data:image/png;base64,${selectedWindow.thumbnail}`}
                  className="w-16 h-12 object-cover rounded"
                  alt={selectedWindow.title}
                />
              ) : (
                <div className="w-16 h-12 bg-secondary rounded flex items-center justify-center">
                  <Monitor className="w-6 h-6 text-muted-foreground" />
                </div>
              )}
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium truncate">{selectedWindow.title}</p>
                <p className="text-xs text-muted-foreground">{selectedWindow.appName}</p>
              </div>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setShowWindowSelector(true)}
              >
                更换
              </Button>
            </div>
          </div>
        )}
      </div>

      {/* Resolution & FPS - Hidden in window mode */}
      {recordingMode !== 'window' && (
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
      )}

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

        {/* Microphone device selector — shown when mic is enabled */}
        {micEnabled && micDevices.length > 0 && (
          <div className="mt-3 space-y-2">
            <Select
              value={micDevice ?? 'default'}
              onValueChange={(value) => setMicDevice(value === 'default' ? null : value)}
            >
              <SelectTrigger className="w-full h-9 text-xs">
                <SelectValue placeholder="选择麦克风设备" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">系统默认麦克风</SelectItem>
                {micDevices.map((device) => (
                  <SelectItem key={device.name} value={device.name}>
                    {device.name}
                    {device.isBluetooth ? ' 🔵' : ''}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {/* Bluetooth HFP warning */}
            {isBluetoothMic && (
              <motion.div
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                className="flex items-start gap-2 p-2 rounded-lg bg-amber-500/10 border border-amber-500/20 text-xs text-amber-600 dark:text-amber-400"
              >
                <AlertTriangle className="w-3.5 h-3.5 mt-0.5 flex-shrink-0" />
                <span>
                  蓝牙耳机麦克风可能降低音频输出质量。建议使用内置麦克风录制。
                </span>
              </motion.div>
            )}

            {/* General Bluetooth device hint */}
            {!isBluetoothMic && hasBluetoothDevice && (
              <p className="text-[10px] text-muted-foreground">
                检测到蓝牙音频设备 — 使用内置麦克风可避免音质下降
              </p>
            )}
          </div>
        )}

        {/* Denoise toggle — shown when mic is enabled */}
        {micEnabled && (
          <div className="mt-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <AudioWaveform className="w-4 h-4 text-muted-foreground" />
              <Label htmlFor="denoise" className="text-xs text-muted-foreground cursor-pointer">
                降噪（去除电流声）
              </Label>
            </div>
            <Switch
              id="denoise"
              checked={denoiseEnabled}
              onCheckedChange={onDenoiseChange}
            />
          </div>
        )}
      </div>

      {/* Start Recording Button */}
      <Button
        onClick={canStartRecording ? onStartRecording : undefined}
        disabled={!canStartRecording || recordingMode === 'area'}
        className="w-full h-12 rounded-xl bg-primary hover:bg-primary/90 text-primary-foreground font-semibold text-base transition-all duration-200 active:scale-[0.99] disabled:opacity-50 disabled:cursor-not-allowed"
      >
        <Circle className="w-4 h-4 mr-2 fill-current" />
        {recordingMode === 'area'
          ? '即将推出'
          : recordingMode === 'window' && !selectedWindow
            ? '请选择窗口'
            : '开始录制'}
      </Button>

      {/* Window Selector Dialog */}
      <WindowSelector
        isOpen={showWindowSelector}
        onSelect={handleWindowSelect}
        onClose={() => setShowWindowSelector(false)}
      />
    </motion.div>
  )
}
