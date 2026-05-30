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
}

export type CaptureMode = 'fullscreen' | 'window' | 'area'

export type AudioConfig = {
  captureSystemAudio: boolean
  captureMicrophone: boolean
  microphoneDevice: string | null
  sampleRate: number
  channels: number
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

export type ExportPreset = 'bilibili' | 'douyin' | 'xiaohongshu'

export type CursorEffectSummary = {
  frameCount: number
  clickEffectCount: number
  effectTimelinePath: string
}

export type CutTimelineSummary = {
  cutCount: number
  totalCutNanos: number
  cutTimelinePath: string
}

export type ExportSummary = {
  frameCount: number
  clickEffectCount: number
  effectTimelinePath: string
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

export type RecordingResult = {
  durationSecs: number
  frameCount: number
  mixedAudioChunkCount: number
  outputPath: string | null
  cursorMetadataPath: string | null
  effectTimelinePath: string | null
  trimMetadataPath: string | null
  cutTimelinePath: string | null
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

export async function stopRecording(): Promise<RecordingResult> {
  return invoke<RecordingResult>('stop_recording')
}

export async function setCaptureMode(config: CaptureConfig): Promise<void> {
  return invoke('set_capture_mode', { payload: config })
}

export async function setAudioConfig(config: AudioConfig): Promise<void> {
  return invoke('set_audio_config', { payload: config })
}

export async function setBeautifyConfig(config: BeautifyConfig): Promise<number> {
  return invoke<number>('set_beautify_config', { config })
}

export async function getBeautifyConfig(): Promise<BeautifyConfig> {
  return invoke<BeautifyConfig>('get_beautify_config')
}

export async function buildCursorEffectTimeline(): Promise<CursorEffectSummary> {
  return invoke<CursorEffectSummary>('build_cursor_effect_timeline')
}

export async function buildCutTimeline(): Promise<CutTimelineSummary> {
  return invoke<CutTimelineSummary>('build_cut_timeline')
}

export async function exportVideo(preset: ExportPreset): Promise<ExportSummary> {
  return invoke<ExportSummary>('export_video', { preset })
}

export async function cancelExport(): Promise<void> {
  return invoke('cancel_export')
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
