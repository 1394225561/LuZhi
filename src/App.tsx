import { useCallback, useEffect, useState } from 'react'
import { AnimatePresence } from 'framer-motion'
import { RecordingPanel } from '@/components/recording-panel'
import { RecordingStatusBar } from '@/components/recording-status-bar'
import { PreviewView } from '@/components/preview-view'
import { ProcessingView } from '@/components/processing-view'
import { ErrorView } from '@/components/error-view'
import {
  fetchRecordingStatus,
  fetchRecordingPermissions,
  startRecording,
  stopRecording,
  pauseRecording,
  resumeRecording,
  setCaptureMode,
  setAudioConfig,
  onRecordingTick,
  onRecordingStateChanged,
  type RecordingPermissions,
  type RecordingStatus,
} from '@/lib/tauri'

type AppState = 'idle' | 'recording' | 'preview' | 'processing' | 'failed'

export type RecordingResult = {
  durationSecs: number
  frameCount: number
  mixedAudioChunkCount: number
  outputPath: string | null
}

export default function App() {
  const [appState, setAppState] = useState<AppState>('idle')
  const [recordingMode, setRecordingMode] = useState<'fullscreen' | 'window' | 'area'>('fullscreen')
  const [systemAudioEnabled, setSystemAudioEnabled] = useState(true)
  const [micEnabled, setMicEnabled] = useState(false)
  const [micVolume, setMicVolume] = useState(60)
  const [elapsedTime, setElapsedTime] = useState(0)
  const [isPaused, setIsPaused] = useState(false)
  const [permissions, setPermissions] = useState<RecordingPermissions>({
    screenRecording: 'unknown',
    microphone: 'unknown',
  })
  const [errorMessage, setErrorMessage] = useState('')
  const [recordingResult, setRecordingResult] = useState<RecordingResult | null>(null)

  // 从 Tauri 后端获取初始状态
  useEffect(() => {
    void fetchRecordingStatus().then((status: RecordingStatus) => {
      if (status.state === 'recording') setAppState('recording')
      else if (status.state === 'processing') setAppState('processing')
      else if (status.state === 'completed') setAppState('preview')
      else if (status.state === 'failed') {
        setAppState('failed')
        setErrorMessage('录制过程中发生错误')
      }
    })
    void fetchRecordingPermissions().then(setPermissions)
  }, [])

  // 监听录制状态变化事件
  useEffect(() => {
    let unlisten: (() => void) | undefined
    void onRecordingStateChanged((status) => {
      if (status.state === 'idle') setAppState('idle')
      else if (status.state === 'recording') {
        setAppState('recording')
        setIsPaused(false)
      }
      else if (status.state === 'paused') setIsPaused(true)
      else if (status.state === 'processing') setAppState('processing')
      else if (status.state === 'completed') setAppState('preview')
      else if (status.state === 'failed') {
        setAppState('failed')
        setErrorMessage('录制过程中发生错误')
      }
    }).then((fn) => { unlisten = fn })
    return () => { unlisten?.() }
  }, [])

  // 监听录制计时事件
  useEffect(() => {
    let unlisten: (() => void) | undefined
    void onRecordingTick((elapsed) => {
      setElapsedTime(elapsed)
    }).then((fn) => { unlisten = fn })
    return () => { unlisten?.() }
  }, [])

  // 模拟麦克风音量变化（后续替换为 Tauri event 'mic-level'）
  useEffect(() => {
    if (!micEnabled) return
    const interval = setInterval(() => {
      setMicVolume(Math.floor(Math.random() * 60) + 20)
    }, 200)
    return () => clearInterval(interval)
  }, [micEnabled])

  const handleStartRecording = useCallback(async () => {
    try {
      setElapsedTime(0)
      setIsPaused(false)
      await setCaptureMode({ mode: recordingMode, width: 1920, height: 1080, fps: 30 })
      await setAudioConfig({
        captureSystemAudio: systemAudioEnabled,
        captureMicrophone: micEnabled,
        microphoneDevice: null,
        sampleRate: 48000,
        channels: 2,
      })
      await startRecording()
    } catch (e) {
      setAppState('failed')
      setErrorMessage(String(e))
    }
  }, [recordingMode, systemAudioEnabled, micEnabled])

  const handlePauseRecording = useCallback(async () => {
    try {
      if (isPaused) {
        await resumeRecording()
      } else {
        await pauseRecording()
      }
    } catch (e) {
      setErrorMessage(String(e))
    }
  }, [isPaused])

  const handleStopRecording = useCallback(async () => {
    try {
      const result = await stopRecording()
      setRecordingResult({
        durationSecs: result.duration_secs,
        frameCount: result.frame_count,
        mixedAudioChunkCount: result.mixed_audio_chunk_count,
        outputPath: result.output_path ?? null,
      })
      setAppState('preview')
    } catch (e) {
      setAppState('failed')
      setErrorMessage(String(e))
    }
  }, [])

  const handleBackToIdle = useCallback(() => {
    setAppState('idle')
    setElapsedTime(0)
    setIsPaused(false)
    setErrorMessage('')
    setRecordingResult(null)
  }, [])

  const handleRetry = useCallback(() => {
    setAppState('idle')
    setErrorMessage('')
  }, [])

  // 禁用桌面端右键菜单和文本选中
  useEffect(() => {
    const handler = (e: Event) => e.preventDefault()
    document.addEventListener('contextmenu', handler)
    document.addEventListener('selectstart', handler)
    return () => {
      document.removeEventListener('contextmenu', handler)
      document.removeEventListener('selectstart', handler)
    }
  }, [])

  // 程序化窗口拖拽（绕过 Tauri drag.js 的 macOS 焦点窗口限制）
  useEffect(() => {
    const handleMouseDown = (e: MouseEvent) => {
      if (e.button !== 0) return

      const target = e.target as HTMLElement

      // 跳过交互元素
      if (target.closest('button, input, select, textarea, a, [role="button"], [role="link"], [role="menuitem"], [role="tab"], [role="checkbox"], [role="radio"], [role="slider"], [role="switch"], [contenteditable]:not([contenteditable="false"]), [tabindex]:not([tabindex="-1"])')) {
        return
      }

      // 确认在拖拽区域内
      const dragRegion = target.closest('[data-tauri-drag-region]')
      if (!dragRegion) return

      import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
        getCurrentWindow().startDragging()
      }).catch(() => {})
    }

    document.addEventListener('mousedown', handleMouseDown)
    return () => document.removeEventListener('mousedown', handleMouseDown)
  }, [])

  // Idle state
  if (appState === 'idle') {
    return (
      <div className="min-h-screen flex items-center justify-center p-8" data-tauri-drag-region="deep">
        <div className="relative">
          <RecordingPanel
            recordingMode={recordingMode}
            setRecordingMode={setRecordingMode}
            systemAudioEnabled={systemAudioEnabled}
            setSystemAudioEnabled={setSystemAudioEnabled}
            micEnabled={micEnabled}
            setMicEnabled={setMicEnabled}
            micVolume={micVolume}
            onStartRecording={handleStartRecording}
          />
          {/* 权限提示 */}
          {(permissions.screenRecording === 'denied' || permissions.microphone === 'denied') && (
            <div className="mt-4 p-3 rounded-xl bg-destructive/10 border border-destructive/20 text-sm text-destructive">
              {permissions.screenRecording === 'denied' && <p>屏幕录制权限未授权，请在系统设置中开启</p>}
              {permissions.microphone === 'denied' && <p>麦克风权限未授权，请在系统设置中开启</p>}
            </div>
          )}
        </div>
      </div>
    )
  }

  // Recording state
  if (appState === 'recording') {
    return (
      <div className="min-h-screen flex flex-col items-center justify-between p-8" data-tauri-drag-region="deep">
        <div className="pt-4">
          <AnimatePresence>
            <RecordingStatusBar
              elapsedTime={elapsedTime}
              isPaused={isPaused}
              onPause={handlePauseRecording}
              onStop={handleStopRecording}
            />
          </AnimatePresence>
        </div>
        <div className="flex-1 w-full max-w-4xl mx-auto my-8 rounded-2xl border-2 border-dashed border-border/30 flex items-center justify-center">
          <div className="text-center text-muted-foreground">
            <p className="text-sm mb-1">
              正在录制 {recordingMode === 'fullscreen' ? '全屏' : recordingMode === 'window' ? '窗口' : '区域'}
            </p>
            <p className="text-xs opacity-60">此区域表示被录制的屏幕内容</p>
          </div>
        </div>
        <div className="h-12" />
      </div>
    )
  }

  // Processing state
  if (appState === 'processing') {
    return <ProcessingView />
  }

  // Failed state
  if (appState === 'failed') {
    return (
      <ErrorView
        message={errorMessage}
        onRetry={handleRetry}
        onBack={handleBackToIdle}
      />
    )
  }

  // Preview state
  return <PreviewView onBack={handleBackToIdle} recordingResult={recordingResult} />
}
