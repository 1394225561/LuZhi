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
  type RecordingPermissions,
  type RecordingStatus,
} from '@/lib/tauri'

type AppState = 'idle' | 'recording' | 'preview' | 'processing' | 'failed'

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

  // 模拟麦克风音量变化（后续替换为 Tauri event 'mic-level'）
  useEffect(() => {
    if (!micEnabled) return
    const interval = setInterval(() => {
      setMicVolume(Math.floor(Math.random() * 60) + 20)
    }, 200)
    return () => clearInterval(interval)
  }, [micEnabled])

  // 录制计时器（后续替换为 Tauri event 'recording-tick'）
  useEffect(() => {
    if (appState !== 'recording' || isPaused) return
    const interval = setInterval(() => {
      setElapsedTime((prev) => prev + 1)
    }, 1000)
    return () => clearInterval(interval)
  }, [appState, isPaused])

  const handleStartRecording = useCallback(() => {
    console.log('start_recording', { recordingMode, systemAudioEnabled, micEnabled })
    // TODO: await invoke('start_recording')
    setAppState('recording')
    setElapsedTime(0)
    setIsPaused(false)
  }, [recordingMode, systemAudioEnabled, micEnabled])

  const handlePauseRecording = useCallback(() => {
    console.log(isPaused ? 'resume_recording' : 'pause_recording')
    // TODO: await invoke(isPaused ? 'resume_recording' : 'pause_recording')
    setIsPaused((prev) => !prev)
  }, [isPaused])

  const handleStopRecording = useCallback(() => {
    console.log('stop_recording')
    // TODO: await invoke('stop_recording')
    setAppState('preview')
  }, [])

  const handleBackToIdle = useCallback(() => {
    setAppState('idle')
    setElapsedTime(0)
    setIsPaused(false)
    setErrorMessage('')
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
        <div className="relative" data-tauri-drag-region={false}>
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
        <div className="pt-4" data-tauri-drag-region={false}>
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
  return <PreviewView onBack={handleBackToIdle} />
}
