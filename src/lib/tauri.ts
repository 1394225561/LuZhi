import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

// ─── 类型定义 ───

export type RecordingState = 'idle' | 'recording' | 'paused' | 'processing' | 'completed' | 'failed'

export type RecordingStatus = {
  state: RecordingState
  canStart: boolean
}

export type RecordingPermissions = {
  screenRecording: 'granted' | 'denied' | 'notDetermined' | 'unknown'
  microphone: 'granted' | 'denied' | 'notDetermined' | 'unknown'
  /** Accessibility permission retained for diagnostics/future AX probes. */
  accessibility: 'granted' | 'denied' | 'notDetermined' | 'unknown'
}

export type CaptureMode = 'fullscreen' | 'window' | 'area'

export type DenoiseMode = 'none' | 'highpass'

export type AudioConfig = {
  captureSystemAudio: boolean
  captureMicrophone: boolean
  microphoneDevice: string | null
  sampleRate: number
  channels: number
  denoiseMode?: DenoiseMode
}

export type CaptureConfig = {
  mode: CaptureMode
  width?: number
  height?: number
  fps?: number
}

export type BeautifyConfig = {
  cursorMagnification: boolean
  magnificationFactor: number
  cursorSmoothing: boolean
  autoTrimSilences: boolean
  trimSensitivity: 'low' | 'medium' | 'high'
}

export type MicLevelPayload = {
  level: number
}

export type MicrophoneDeviceInfo = {
  name: string
  isBluetooth: boolean
}

export type ExportPreset = 'bilibili' | 'douyin' | 'xiaohongshu'

export type CursorEffectSummary = {
  frameCount: number
  clickEffectCount: number
  effectTimelinePath: string | null
}

export type CursorKind = 'arrow' | 'hand' | 'iBeam'

export type CursorFrame = {
  timestamp: { nanos: number }
  x: number
  y: number
  scale: number
  opacity: number
  kind: CursorKind
}

export type CursorClickEffect = {
  start: { nanos: number }
  end: { nanos: number }
  x: number
  y: number
  maxScale: number
  peakOpacity: number
}

export type EffectTimeline = {
  fps: number
  durationNanos: number
  frames: CursorFrame[]
  clickEffects: CursorClickEffect[]
  rawSystemCursorVisible: boolean
  renderCursorOverlay: boolean
  sourcePtsOriginNanos: number
}

export type CutTimelineSummary = {
  cutCount: number
  totalCutNanos: number
  cutTimelinePath: string
}

export type ExportSummary = {
  frameCount: number
  clickEffectCount: number
  effectTimelinePath: string | null
  cutCount: number
  totalCutNanos: number
  cutTimelinePath: string | null
  outputPath: string | null
}

export type ExportProgressPayload = {
  preset: ExportPreset
  progress: number
  cancellable: boolean
  outputPath: string | null
  error?: string
}

export type LicenseStatus = {
  kind: 'trial' | 'expired' | 'activated'
  trialDaysRemaining: number
  isExpired: boolean
  activated: boolean
}

/** Lightweight summary of a recording in the library. */
export type LibraryEntrySummary = {
  id: string
  createdAt: number
  durationSecs: number
}

/** Full context for re-entering the beautify workflow. */
export type RecordingContextPayload = {
  videoPath: string
  cursorMetadataPath: string
  effectTimelinePath: string
  trimMetadataPath: string
  cutTimelinePath: string
  metadataJson: string
  effectTimelineJson: string
  cutTimelineJson: string
}

export type RecordingResult = {
  durationSecs: number
  frameCount: number
  mixedAudioChunkCount: number
  outputPath: string | null
  cursorMetadataPath: string | null
  effectTimelinePath: string | null
  trimMetadataPath: string | null
  cutTimelinePath: string | null
  writerDiagnostics: WriterDiagnostics
  diagnostics: RecordingDiagnostics
  /** Non-empty when recording completed with issues. */
  finalizationErrors: string[]
}

/** Structured response from stop_recording command. */
export type StopRecordingResponse = {
  result: RecordingResult
  failed: boolean
}

export type WriterDiagnostics = {
  audioChunksReceived: number
  audioChunksAppended: number
  audioChunksDiscardedFullOverlap: number
  audioChunksTrimmedPartialOverlap: number
  audioRealFramesAppended: number
  audioSilenceFramesPadded: number
  audioRealRmsMaxBeforeEncode: number
  aacFramesEncoded: number
  silentAacFramesEncoded: number
  generatedSilentTrack: boolean
  videoQueueFullCount: number
  audioQueueFullCount: number
  systemChunksReceivedByWriter: number
  micChunksReceivedByWriter: number
}

export type RecordingDiagnostics = {
  requestedSystemAudio: boolean
  requestedMicrophone: boolean
  microphoneDevice: string | null
  systemChunksReceived: number
  micChunksReceived: number
  systemChunksDropped: number
  micChunksDropped: number
  mixedChunksQueued: number
  writerPushAudioFailures: number
  systemRmsMax: number
  micRmsMax: number
  mixedRmsMax: number
  generatedSilentTrack: boolean
  pairedWindowCount: number
  systemOnlyWindowCount: number
  micOnlyWindowCount: number
  sourceTimeoutWindowCount: number
  systemRmsMaxBeforeWriter: number
  micRmsMaxBeforeWriter: number
  systemWindowsBeforeWriter: number
  micWindowsBeforeWriter: number
  systemFramesBeforeWriter: number
  micFramesBeforeWriter: number
  micStopDiagnostics: CpalMicrophoneStopDiagnostics | null
}

export type CpalMicrophoneStopDiagnostics = {
  stopRequested: boolean
  streamExisted: boolean
  pauseAttempted: boolean
  pauseOk: boolean
  pauseError: string | null
  streamDropped: boolean
  callbacksAfterStop: number
  stopWaitMs: number
}


// ─── Tauri Commands ───

export async function fetchRecordingStatus(): Promise<RecordingStatus> {
  return invoke<RecordingStatus>('recording_status')
}

export async function fetchRecordingPermissions(): Promise<RecordingPermissions> {
  return invoke<RecordingPermissions>('recording_permissions')
}

export async function startRecording(): Promise<void> {
  return invoke('start_recording')
}

export async function pauseRecording(): Promise<void> {
  return invoke('pause_recording')
}

export async function resumeRecording(): Promise<void> {
  return invoke('resume_recording')
}

export async function stopRecording(): Promise<StopRecordingResponse> {
  return invoke<StopRecordingResponse>('stop_recording')
}

export async function setCaptureMode(config: CaptureConfig): Promise<void> {
  return invoke('set_capture_mode', { payload: config })
}

export async function setAudioConfig(config: AudioConfig): Promise<void> {
  return invoke('set_audio_config', { payload: config })
}

export async function listMicrophoneDevices(): Promise<MicrophoneDeviceInfo[]> {
  return invoke('list_microphone_devices')
}

export async function setBeautifyConfig(config: BeautifyConfig): Promise<number> {
  return invoke<number>('set_beautify_config', { config })
}

export async function getBeautifyConfig(): Promise<BeautifyConfig> {
  return invoke<BeautifyConfig>('get_beautify_config')
}

export async function buildCursorEffectTimeline(recordingId?: string): Promise<CursorEffectSummary> {
  return invoke<CursorEffectSummary>('build_cursor_effect_timeline', { recordingId: recordingId ?? null })
}

export async function getCursorEffectTimeline(): Promise<EffectTimeline> {
  return invoke<EffectTimeline>('get_cursor_effect_timeline')
}

export async function buildCutTimeline(recordingId?: string): Promise<CutTimelineSummary> {
  return invoke<CutTimelineSummary>('build_cut_timeline', { recordingId: recordingId ?? null })
}

export async function exportVideo(preset: ExportPreset, recordingId?: string): Promise<ExportSummary> {
  return invoke<ExportSummary>('export_video', { preset, recordingId: recordingId ?? null })
}

export async function cancelExport(): Promise<void> {
  return invoke('cancel_export')
}

export async function fetchLicenseStatus(): Promise<LicenseStatus> {
  return invoke<LicenseStatus>('license_status')
}

export async function fetchActivationStatus(): Promise<LicenseStatus> {
  return invoke<LicenseStatus>('activation_status')
}

export async function activateLicense(code: string): Promise<void> {
  return invoke('activate_license', { code })
}

export async function listRecordings(): Promise<LibraryEntrySummary[]> {
  return invoke<LibraryEntrySummary[]>('list_recordings')
}

export async function getRecordingContext(id: string): Promise<RecordingContextPayload> {
  return invoke<RecordingContextPayload>('get_recording_context', { id })
}

export async function importRecording(path: string): Promise<LibraryEntrySummary> {
  return invoke<LibraryEntrySummary>('import_recording', { path })
}

export async function deleteRecording(id: string): Promise<void> {
  return invoke('delete_recording', { id })
}

// ─── Tauri Events ───

export function onRecordingTick(callback: (elapsed: number) => void): Promise<UnlistenFn> {
  return listen<{ elapsed: number }>('recording-tick', (event) => {
    callback(event.payload.elapsed)
  })
}

export function onMicLevel(callback: (level: number) => void): Promise<UnlistenFn> {
  return listen<{ level: number }>('mic-level', (event) => {
    callback(event.payload.level)
  })
}

export function onRecordingStateChanged(callback: (status: RecordingStatus) => void): Promise<UnlistenFn> {
  return listen<RecordingStatus>('recording-state-changed', (event) => {
    callback(event.payload)
  })
}

export function onExportProgress(callback: (payload: ExportProgressPayload) => void): Promise<UnlistenFn> {
  return listen<ExportProgressPayload>('export-progress', (event) => {
    callback(event.payload)
  })
}
