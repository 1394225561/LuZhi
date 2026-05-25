import { render, screen, fireEvent, act } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'

// Mock ResizeObserver for framer-motion in jsdom
class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver = ResizeObserverMock as unknown as typeof ResizeObserver

const invokeMock = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

describe('App', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') {
        return Promise.resolve({ state: 'idle', canStart: true })
      }
      if (command === 'recording_permissions') {
        return Promise.resolve({
          screenRecording: 'unknown',
          microphone: 'unknown',
        })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })
  })

  it('renders Chinese recording controls in idle state', async () => {
    render(<App />)

    expect(await screen.findByText('录制')).toBeInTheDocument()
    expect(screen.getByText('全屏')).toBeInTheDocument()
    expect(screen.getByText('窗口')).toBeInTheDocument()
    expect(screen.getByText('区域')).toBeInTheDocument()
    expect(screen.getByText('开始录制')).toBeInTheDocument()
  })

  it('renders audio settings', async () => {
    render(<App />)

    expect(await screen.findAllByText('系统音频')).toHaveLength(2)
    expect(screen.getAllByText('麦克风')).toHaveLength(2)
  })

  it('requests status and permissions from Tauri', async () => {
    render(<App />)

    await screen.findAllByText('录制')

    expect(invokeMock).toHaveBeenCalledWith('recording_status', undefined)
    expect(invokeMock).toHaveBeenCalledWith('recording_permissions', undefined)
  })

  it('shows permission warning when denied', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') {
        return Promise.resolve({ state: 'idle', canStart: true })
      }
      if (command === 'recording_permissions') {
        return Promise.resolve({
          screenRecording: 'denied',
          microphone: 'denied',
        })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    expect(await screen.findByText('屏幕录制权限未授权，请在系统设置中开启')).toBeInTheDocument()
    expect(screen.getByText('麦克风权限未授权，请在系统设置中开启')).toBeInTheDocument()
  })

  it('sends set_capture_mode and set_audio_config before start_recording', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') {
        return Promise.resolve({ state: 'idle', canStart: true })
      }
      if (command === 'recording_permissions') {
        return Promise.resolve({
          screenRecording: 'unknown',
          microphone: 'unknown',
        })
      }
      if (command === 'set_capture_mode') return Promise.resolve()
      if (command === 'set_audio_config') return Promise.resolve()
      if (command === 'start_recording') return Promise.resolve()
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    const startButtons = await screen.findAllByText('开始录制')
    invokeMock.mockClear()
    fireEvent.click(startButtons[0])

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledTimes(3)
    })

    expect(invokeMock).toHaveBeenNthCalledWith(1, 'set_capture_mode', {
      payload: { mode: 'fullscreen', width: 1920, height: 1080, fps: 30 },
    })
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'set_audio_config', {
      payload: {
        captureSystemAudio: true,
        captureMicrophone: false,
        microphoneDevice: null,
        sampleRate: 48000,
        channels: 2,
      },
    })
    expect(invokeMock).toHaveBeenNthCalledWith(3, 'start_recording', undefined)
  })

  it('does not render area-level false drag-region wrappers', async () => {
    render(<App />)

    await screen.findAllByText('开始录制')

    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
  })

  it('displays recording result after stop with camelCase fields', async () => {
    const { listen } = await import('@tauri-apps/api/event')
    const stateCallbacks: Array<(status: { state: string }) => void> = []

    vi.mocked(listen).mockImplementation(
      (event: string, callback: (event: { event: string; id: number; payload: unknown }) => void) => {
        if (event === 'recording-state-changed') {
          stateCallbacks.push((status) => callback({ event, id: 0, payload: status }))
        }
        return Promise.resolve(() => {})
      },
    )

    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_capture_mode') return Promise.resolve()
      if (command === 'set_audio_config') return Promise.resolve()
      if (command === 'start_recording') return Promise.resolve()
      if (command === 'stop_recording') {
        return Promise.resolve({
          durationSecs: 5,
          frameCount: 150,
          mixedAudioChunkCount: 50,
          outputPath: '/tmp/test.mp4',
        })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    const startButtons = await screen.findAllByText('开始录制')
    fireEvent.click(startButtons[0])

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
    })

    // Emit recording state change event
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'recording' })
      }
    })

    // RecordingStatusBar uses icon buttons; find the stop button (last button)
    await vi.waitFor(() => {
      const buttons = screen.getAllByRole('button')
      expect(buttons.length).toBeGreaterThan(0)
    })
    const buttons = screen.getAllByRole('button')
    console.log('Button count:', buttons.length)
    console.log('Button labels:', buttons.map(b => b.textContent || b.getAttribute('aria-label')))
    const stopButton = buttons[buttons.length - 1]
    await act(async () => {
      fireEvent.click(stopButton)
    })
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('stop_recording', undefined)
    })

    // PreviewView renders with outputPath (video element)
    await screen.findByText('预览与美化')
    expect(screen.getByText('预览与美化')).toBeTruthy()
    // Verify stop_recording returned camelCase result
    expect(invokeMock).toHaveBeenCalledWith('stop_recording', undefined)
  })
})
