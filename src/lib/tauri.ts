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
  systemAudio: boolean
  microphone: boolean
}

export type BeautifyConfig = {
  cursorMagnification: boolean
  magnificationFactor: number
  cursorSmoothing: boolean
  autoTrimSilences: boolean
  trimSensitivity: 'low' | 'medium' | 'high'
}

export type ExportPreset = 'bilibili' | 'douyin' | 'xiaohongshu'

export type RecordingResult = {
  duration_secs: number
  frame_count: number
  output_path: string | null
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

export async function setCaptureMode(mode: CaptureMode): Promise<void> {
  return invoke('set_capture_mode', { mode })
}

export async function setAudioConfig(config: AudioConfig): Promise<void> {
  return invoke('set_audio_config', { config })
}

export async function setBeautifyConfig(config: BeautifyConfig): Promise<void> {
  return invoke('set_beautify_config', { config })
}

export async function exportVideo(preset: ExportPreset): Promise<void> {
  return invoke('export_video', { preset })
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
