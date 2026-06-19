import { useCallback, useEffect, useRef, useState } from 'react'
import { AnimatePresence } from 'framer-motion'
import { Toaster, toast } from 'sonner'
import { RecordingPanel } from '@/components/recording-panel'
import { RecordingStatusBar } from '@/components/recording-status-bar'
import { PreviewView } from '@/components/preview-view'
import { ProcessingView } from '@/components/processing-view'
import { ErrorView } from '@/components/error-view'
import { LicenseStatus } from '@/components/license-status'
import { RecordingSidebar } from '@/components/recording-sidebar'
import {
  fetchRecordingStatus,
  fetchRecordingPermissions,
  fetchLicenseStatus,
  startRecording,
  stopRecording,
  pauseRecording,
  resumeRecording,
  setCaptureMode,
  setAudioConfig,
  getRecordingContext,
  onRecordingTick,
  onRecordingStateChanged,
  onMicLevel,
  listen,
  type LicenseStatus as LicenseStatusPayload,
  type RecordingPermissions,
  type RecordingResult,
  type RecordingStatus,
  type WindowInfo,
} from '@/lib/tauri'

type AppState = 'idle' | 'recording' | 'preview' | 'processing' | 'failed'

const DEFAULT_RESOLUTION = {
  width: 1920,
  height: 1080,
  label: '1080p (1920×1080)',
}
const DEFAULT_FPS = 30
const NON_DRAG_TARGET_SELECTOR = [
  'button',
  'input',
  'select',
  'textarea',
  'a',
  'video',
  'audio',
  '[controls]',
  '[role="button"]',
  '[role="link"]',
  '[role="menuitem"]',
  '[role="tab"]',
  '[role="checkbox"]',
  '[role="radio"]',
  '[role="slider"]',
  '[role="switch"]',
  '[contenteditable]:not([contenteditable="false"])',
  '[tabindex]:not([tabindex="-1"])',
].join(', ')
const TEXT_DRAG_TARGET_SELECTOR =
  'p, h1, h2, h3, h4, h5, h6, span, code, pre, kbd, label, small, strong, em'

/**
 * Creates a RecordingResult with zeroed diagnostics for re-opening a past recording.
 * If RecordingResult/WriterDiagnostics/RecordingDiagnostics gain new fields,
 * the TypeScript compiler will flag missing fields here.
 */
function createEmptyRecordingResult(
  overrides: Partial<RecordingResult> = {},
): RecordingResult {
  return {
    durationSecs: 0,
    frameCount: 0,
    mixedAudioChunkCount: 0,
    outputPath: null,
    cursorMetadataPath: null,
    effectTimelinePath: null,
    trimMetadataPath: null,
    cutTimelinePath: null,
    writerDiagnostics: {
      audioChunksReceived: 0,
      audioChunksAppended: 0,
      audioChunksDiscardedFullOverlap: 0,
      audioChunksTrimmedPartialOverlap: 0,
      audioRealFramesAppended: 0,
      audioSilenceFramesPadded: 0,
      audioRealRmsMaxBeforeEncode: 0,
      aacFramesEncoded: 0,
      silentAacFramesEncoded: 0,
      generatedSilentTrack: false,
      videoQueueFullCount: 0,
      audioQueueFullCount: 0,
      systemChunksReceivedByWriter: 0,
      micChunksReceivedByWriter: 0,
    },
    diagnostics: {
      requestedSystemAudio: false,
      requestedMicrophone: false,
      microphoneDevice: null,
      systemChunksReceived: 0,
      micChunksReceived: 0,
      systemChunksDropped: 0,
      micChunksDropped: 0,
      mixedChunksQueued: 0,
      writerPushAudioFailures: 0,
      systemRmsMax: 0,
      micRmsMax: 0,
      mixedRmsMax: 0,
      generatedSilentTrack: false,
      pairedWindowCount: 0,
      systemOnlyWindowCount: 0,
      micOnlyWindowCount: 0,
      sourceTimeoutWindowCount: 0,
      systemRmsMaxBeforeWriter: 0,
      micRmsMaxBeforeWriter: 0,
      systemWindowsBeforeWriter: 0,
      micWindowsBeforeWriter: 0,
      systemFramesBeforeWriter: 0,
      micFramesBeforeWriter: 0,
      micStopDiagnostics: null,
    },
    finalizationErrors: [],
    ...overrides,
  }
}

export default function App() {
  const [appState, setAppState] = useState<AppState>('idle')
  const [recordingMode, setRecordingMode] = useState<
    'fullscreen' | 'window' | 'area'
  >('fullscreen')
  const [systemAudioEnabled, setSystemAudioEnabled] = useState(true)
  const [micEnabled, setMicEnabled] = useState(false)
  const [micDevice, setMicDevice] = useState<string | null>(null)
  const [micVolume, setMicVolume] = useState(0)
  const [denoiseEnabled, setDenoiseEnabled] = useState(false)
  const [elapsedTime, setElapsedTime] = useState(0)
  const [isPaused, setIsPaused] = useState(false)
  const [permissions, setPermissions] = useState<RecordingPermissions>({
    screenRecording: 'unknown',
    microphone: 'unknown',
    accessibility: 'unknown',
  })
  const [errorMessage, setErrorMessage] = useState('')
  const [recordingResult, setRecordingResult] =
    useState<RecordingResult | null>(null)
  const [selectedRecordingId, setSelectedRecordingId] = useState<string | null>(
    null,
  )
  const [selectedWindowId, setSelectedWindowId] = useState<number | null>(null)
  const [resolution, setResolution] = useState(DEFAULT_RESOLUTION)
  const [fps, setFps] = useState(DEFAULT_FPS)
  const [licenseStatus, setLicenseStatus] =
    useState<LicenseStatusPayload | null>(null)
  const [sidebarOpen, setSidebarOpen] = useState(false)
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
    void fetchLicenseStatus()
      .then(setLicenseStatus)
      .catch(() => setLicenseStatus(null))
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
      } else if (status.state === 'recording') {
        setAppState('recording')
        setIsPaused(false)
        isStartingRef.current = false
      } else if (status.state === 'paused') setIsPaused(true)
      else if (status.state === 'processing') setAppState('processing')
      else if (status.state === 'completed') {
        if (status.result) setRecordingResult(status.result)
        setAppState('preview')
        setMicVolume(0)
        isStoppingRef.current = false
      } else if (status.state === 'failed') {
        setAppState('failed')
        setErrorMessage((current) => current || '录制过程中发生错误')
        setMicVolume(0)
        isStartingRef.current = false
        isStoppingRef.current = false
      }
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
    }
  }, [])

  // 监听录制计时事件
  useEffect(() => {
    let unlisten: (() => void) | undefined
    void onRecordingTick((elapsed) => {
      setElapsedTime(elapsed)
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
    }
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
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [appState])

  // 监听窗口状态变化事件
  useEffect(() => {
    let unlisten: (() => void) | undefined
    void listen<{ state: 'minimized' | 'closed'; windowTitle: string }>(
      'window-state-changed',
      (event) => {
        const { state, windowTitle } = event.payload

        switch (state) {
          case 'minimized':
            toast.warning('录制暂停', {
              description: `窗口"${windowTitle}"已最小化，恢复窗口后继续录制`,
            })
            break
          case 'closed':
            toast.error('录制停止', {
              description: `窗口"${windowTitle}"已关闭`,
            })
            break
        }
      },
    ).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
    }
  }, [])

  // 麦克风关闭时清空残留电平
  useEffect(() => {
    if (!micEnabled) setMicVolume(0)
  }, [micEnabled])

  const handleStartRecording = useCallback(async () => {
    if (appState !== 'idle' || isStartingRef.current) return

    isStartingRef.current = true
    setElapsedTime(0)
    setIsPaused(false)

    try {
      await setCaptureMode({
        mode: recordingMode,
        width: resolution.width,
        height: resolution.height,
        fps,
        windowId:
          recordingMode === 'window'
            ? (selectedWindowId ?? undefined)
            : undefined,
      })
      await setAudioConfig({
        captureSystemAudio: systemAudioEnabled,
        captureMicrophone: micEnabled,
        microphoneDevice: micDevice,
        sampleRate: 48000,
        channels: 2,
        denoiseMode: denoiseEnabled ? 'highpass' : 'none',
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
  }, [
    appState,
    recordingMode,
    selectedWindowId,
    systemAudioEnabled,
    micEnabled,
    micDevice,
    denoiseEnabled,
    resolution,
    fps,
  ])

  const handleSelectedWindowChange = useCallback(
    (window: WindowInfo | null) => {
      setSelectedWindowId(window?.windowId ?? null)
    },
    [],
  )

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
      const payload = await stopRecording()
      setSelectedRecordingId(null)
      setPermissions(payload.permissions)
      setLicenseStatus(payload.licenseStatus)
      const recording = payload.recording
      if (!recording) {
        setAppState('failed')
        setErrorMessage('录制停止失败')
        return
      }
      const result = recording.result
      setRecordingResult({
        durationSecs: result.durationSecs,
        frameCount: result.frameCount,
        mixedAudioChunkCount: result.mixedAudioChunkCount,
        outputPath: result.outputPath ?? null,
        cursorMetadataPath: result.cursorMetadataPath ?? null,
        effectTimelinePath: result.effectTimelinePath ?? null,
        trimMetadataPath: result.trimMetadataPath ?? null,
        cutTimelinePath: result.cutTimelinePath ?? null,
        writerDiagnostics: result.writerDiagnostics,
        diagnostics: result.diagnostics,
        finalizationErrors: result.finalizationErrors ?? [],
      })
      if (recording.failed) {
        const errorDetail =
          result.finalizationErrors?.join('; ') || '录制完成但存在错误'
        setAppState('failed')
        setErrorMessage(errorDetail)
        return
      }
      if (result.finalizationErrors && result.finalizationErrors.length > 0) {
        console.warn('录制完成但有警告:', result.finalizationErrors)
      }
      if (payload.state === 'completed') {
        setAppState('preview')
      }
    } catch (e) {
      setAppState('failed')
      setErrorMessage(String(e))
    } finally {
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
    setSelectedRecordingId(null)
    // 重新检测权限（用户可能在系统设置中修改了权限）
    void fetchRecordingPermissions().then(setPermissions)
  }, [])

  const handleRetry = useCallback(() => {
    handleBackToIdle()
  }, [handleBackToIdle])

  const handleSelectRecording = useCallback(async (id: string) => {
    try {
      const ctx = await getRecordingContext(id)
      const result = createEmptyRecordingResult({
        outputPath: ctx.videoPath,
        cursorMetadataPath: ctx.cursorMetadataPath,
        effectTimelinePath: ctx.effectTimelinePath,
        trimMetadataPath: ctx.trimMetadataPath,
        cutTimelinePath: ctx.cutTimelinePath,
      })
      setSelectedRecordingId(id)
      setRecordingResult(result)
      setAppState('preview')
    } catch (e) {
      setErrorMessage(String(e))
      setAppState('failed')
    }
  }, [])

  // 禁用桌面端右键菜单，保留文本选择以便复制录制/导出信息。
  useEffect(() => {
    const handler = (e: Event) => e.preventDefault()
    document.addEventListener('contextmenu', handler)
    return () => {
      document.removeEventListener('contextmenu', handler)
    }
  }, [])

  // 程序化窗口拖拽（绕过 Tauri drag.js 的 macOS 焦点窗口限制）
  useEffect(() => {
    const handleMouseDown = (e: MouseEvent) => {
      if (e.button !== 0) return

      const target = e.target
      if (!(target instanceof Element)) return

      const dragRegion = target.closest('[data-luzhi-drag-region]')
      if (!dragRegion) return

      // 跳过交互元素和可复制文本。
      if (
        target.closest(NON_DRAG_TARGET_SELECTOR) ||
        target.closest(TEXT_DRAG_TARGET_SELECTOR)
      ) {
        return
      }

      import('@tauri-apps/api/window')
        .then(({ getCurrentWindow }) => {
          getCurrentWindow().startDragging()
        })
        .catch(() => {})
    }

    document.addEventListener('mousedown', handleMouseDown)
    return () => document.removeEventListener('mousedown', handleMouseDown)
  }, [])

  // Idle state
  if (appState === 'idle') {
    return (
      <>
        <Toaster position="top-right" />
        <div
          className="min-h-screen flex"
          data-luzhi-drag-region="surface"
        >
          {/* Main content area */}
          <div className="flex-1 flex items-center justify-center p-8">
            <div className="relative">
              <RecordingPanel
                recordingMode={recordingMode}
                setRecordingMode={setRecordingMode}
                systemAudioEnabled={systemAudioEnabled}
                setSystemAudioEnabled={setSystemAudioEnabled}
                micEnabled={micEnabled}
                setMicEnabled={setMicEnabled}
                micDevice={micDevice}
                setMicDevice={setMicDevice}
                micVolume={micVolume}
                denoiseEnabled={denoiseEnabled}
                onDenoiseChange={setDenoiseEnabled}
                onStartRecording={handleStartRecording}
                resolution={resolution}
                setResolution={setResolution}
                fps={fps}
                setFps={setFps}
                onSelectedWindowChange={handleSelectedWindowChange}
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
              <div className="flex w-full items-start justify-end mt-4">
                <LicenseStatus status={licenseStatus} />
              </div>
              {/* History sidebar */}
              <RecordingSidebar
                isOpen={sidebarOpen}
                onToggle={() => setSidebarOpen(!sidebarOpen)}
                onSelectRecording={(id) => void handleSelectRecording(id)}
              />
            </div>
          </div>
        </div>
      </>
    )
  }

  // Recording state
  if (appState === 'recording') {
    return (
      <>
        <Toaster position="top-right" />
        <div
          className="min-h-screen flex flex-col items-center justify-between p-8"
          data-luzhi-drag-region="surface"
        >
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
              <p className="text-sm mb-3">
                正在录制{' '}
                {recordingMode === 'fullscreen'
                  ? '全屏'
                  : recordingMode === 'window'
                    ? '窗口'
                    : '区域'}
              </p>
              <div className="flex gap-8 justify-center">
                <div className="text-left">
                  <p className="text-[16px] uppercase tracking-wide mb-1.5">
                    画面
                  </p>
                  <p className="text-xs">
                    {resolution.width} × {resolution.height}
                  </p>
                  <p className="text-xs">{fps} fps</p>
                </div>
                <div className="text-left">
                  <p className="text-[16px] uppercase tracking-wide mb-1.5">
                    音频
                  </p>
                  <p className="text-xs">
                    系统音频{' '}
                    <span
                      className={
                        systemAudioEnabled
                          ? 'text-green-400'
                          : 'text-muted-foreground'
                      }
                    >
                      {systemAudioEnabled ? '✓' : '✗'}
                    </span>
                  </p>
                  <p className="text-xs">
                    麦克风{' '}
                    <span
                      className={
                        micEnabled ? 'text-green-400' : 'text-muted-foreground'
                      }
                    >
                      {micEnabled ? '✓' : '✗'}
                    </span>
                  </p>
                </div>
              </div>
            </div>
          </div>
          <div className="h-12" />
        </div>
      </>
    )
  }

  // Processing state
  if (appState === 'processing') {
    return (
      <>
        <Toaster position="top-right" />
        <ProcessingView />
      </>
    )
  }

  // Failed state
  if (appState === 'failed') {
    return (
      <>
        <Toaster position="top-right" />
        <ErrorView
          message={errorMessage}
          onRetry={handleRetry}
          onBack={handleBackToIdle}
        />
      </>
    )
  }

  // Preview state
  return (
    <>
      <Toaster position="top-right" />
      <PreviewView
        onBack={handleBackToIdle}
        recordingResult={recordingResult}
        licenseStatus={licenseStatus}
        recordingId={selectedRecordingId}
      />
    </>
  )
}
