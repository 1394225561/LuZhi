import { useCallback, useEffect, useRef, useState } from 'react'
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
  onMicLevel,
  type RecordingPermissions,
  type RecordingResult,
  type RecordingStatus,
} from '@/lib/tauri'

type AppState = 'idle' | 'recording' | 'preview' | 'processing' | 'failed'

const DEFAULT_RESOLUTION = { width: 1920, height: 1080, label: '1080p (1920×1080)' }
const DEFAULT_FPS = 30

export default function App() {
  const [appState, setAppState] = useState<AppState>('idle')
  const [recordingMode, setRecordingMode] = useState<'fullscreen' | 'window' | 'area'>('fullscreen')
  const [systemAudioEnabled, setSystemAudioEnabled] = useState(true)
  const [micEnabled, setMicEnabled] = useState(false)
  const [micVolume, setMicVolume] = useState(0)
  const [elapsedTime, setElapsedTime] = useState(0)
  const [isPaused, setIsPaused] = useState(false)
  const [permissions, setPermissions] = useState<RecordingPermissions>({
    screenRecording: 'unknown',
    microphone: 'unknown',
  })
  const [errorMessage, setErrorMessage] = useState('')
  const [recordingResult, setRecordingResult] = useState<RecordingResult | null>(null)
  const [resolution, setResolution] = useState(DEFAULT_RESOLUTION)
  const [fps, setFps] = useState(DEFAULT_FPS)
  const isStartingRef = useRef(false)
  const isStoppingRef = useRef(false)

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
      if (status.state === 'idle') {
        setAppState('idle')
        setMicVolume(0)
        isStartingRef.current = false
        isStoppingRef.current = false
      }
      else if (status.state === 'recording') {
        setAppState('recording')
        setIsPaused(false)
        isStartingRef.current = false
      }
      else if (status.state === 'paused') setIsPaused(true)
      else if (status.state === 'processing') setAppState('processing')
      else if (status.state === 'completed') {
        setAppState('preview')
        setMicVolume(0)
        isStoppingRef.current = false
      }
      else if (status.state === 'failed') {
        setAppState('failed')
        setErrorMessage('录制过程中发生错误')
        setMicVolume(0)
        isStartingRef.current = false
        isStoppingRef.current = false
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

  // 监听真实麦克风电平事件
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined
    if (appState === 'recording') {
      void onMicLevel((level) => {
        if (!cancelled) setMicVolume(Math.round(level * 100))
      }).then((fn) => {
        if (cancelled) fn()
        else unlisten = fn
      })
    }
    return () => { cancelled = true; unlisten?.() }
  }, [appState])

  // 麦克风关闭时清空残留电平
  useEffect(() => {
    if (!micEnabled) setMicVolume(0)
  }, [micEnabled])

  const handleStartRecording = useCallback(async () => {
    if (appState !== 'idle' || isStartingRef.current) return

    if (recordingMode !== 'fullscreen') {
      setErrorMessage('窗口/区域录制模式正在开发中，当前仅支持全屏录制')
      return
    }

    isStartingRef.current = true
    setElapsedTime(0)
    setIsPaused(false)

    try {
      await setCaptureMode({
        mode: recordingMode,
        width: resolution.width,
        height: resolution.height,
        fps,
      })
      await setAudioConfig({
        captureSystemAudio: systemAudioEnabled,
        captureMicrophone: micEnabled,
        microphoneDevice: null,
        sampleRate: 48000,
        channels: 2,
      })
      await startRecording()
      // Fallback: sync state via backend query in case recording-state-changed event is lost.
      const startStatus = await fetchRecordingStatus()
      if (startStatus.state === 'recording') {
        setAppState('recording')
        setIsPaused(false)
        isStartingRef.current = false
      }
    } catch (e) {
      setAppState('failed')
      setErrorMessage(String(e))
      isStartingRef.current = false
    }
  }, [appState, recordingMode, systemAudioEnabled, micEnabled, resolution, fps])

  const handlePauseRecording = useCallback(async () => {
    if (appState !== 'recording') return
    try {
      if (isPaused) {
        await resumeRecording()
      } else {
        await pauseRecording()
      }
    } catch (e) {
      setErrorMessage(String(e))
    }
  }, [appState, isPaused])

  const handleStopRecording = useCallback(async () => {
    if (appState !== 'recording' || isStoppingRef.current) return
    isStoppingRef.current = true
    try {
      const result = await stopRecording()
      setRecordingResult({
        durationSecs: result.durationSecs,
        frameCount: result.frameCount,
        mixedAudioChunkCount: result.mixedAudioChunkCount,
        outputPath: result.outputPath ?? null,
      })
      // Fallback: sync state via backend query in case recording-state-changed event is lost.
      const stopStatus = await fetchRecordingStatus()
      if (stopStatus.state === 'completed') {
        setAppState('preview')
        isStoppingRef.current = false
      }
    } catch (e) {
      setAppState('failed')
      setErrorMessage(String(e))
      isStoppingRef.current = false
    }
  }, [appState])

  const handleBackToIdle = useCallback(() => {
    setAppState('idle')
    setElapsedTime(0)
    setIsPaused(false)
    setMicVolume(0)
    setErrorMessage('')
    setRecordingResult(null)
    // 重新检测权限（用户可能在系统设置中修改了权限）
    void fetchRecordingPermissions().then(setPermissions)
  }, [])

  const handleRetry = useCallback(() => {
    handleBackToIdle()
  }, [handleBackToIdle])

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
            resolution={resolution}
            setResolution={setResolution}
            fps={fps}
            setFps={setFps}
          />
          {/* 权限提示 */}
          {permissions.screenRecording === 'denied' && (
            <div className="mt-4 p-3 rounded-xl bg-destructive/10 border border-destructive/20 text-sm text-destructive">
              <p>屏幕录制权限未授权，请在系统设置中开启</p>
            </div>
          )}
          {permissions.microphone === 'denied' && (
            <div className="mt-2 p-3 rounded-xl bg-destructive/10 border border-destructive/20 text-sm text-destructive">
              <p>麦克风权限未授权，请在系统设置中开启</p>
            </div>
          )}
          {permissions.screenRecording === 'notDetermined' && (
            <div className="mt-4 p-3 rounded-xl bg-amber-500/10 border border-amber-500/20 text-sm text-amber-400">
              <p>需要屏幕录制权限才能录制，请在启动录制时授权</p>
            </div>
          )}
          {permissions.microphone === 'notDetermined' && micEnabled && (
            <div className="mt-2 p-3 rounded-xl bg-amber-500/10 border border-amber-500/20 text-sm text-amber-400">
              <p>需要麦克风权限才能录制音频，请在启动录制时授权</p>
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
              micEnabled={micEnabled}
              micVolume={micVolume}
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
