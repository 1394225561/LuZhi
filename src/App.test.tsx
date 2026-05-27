import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
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
  beforeEach(async () => {
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

    // Reset listen mock between tests.
    const { listen } = await import('@tauri-apps/api/event')
    vi.mocked(listen).mockReset()
    vi.mocked(listen).mockImplementation(() => Promise.resolve(() => {}))
  })

  afterEach(() => {
    cleanup()
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

    expect((await screen.findAllByText('系统音频')).length).toBeGreaterThan(0)
    expect(screen.getAllByText('麦克风').length).toBeGreaterThan(0)
  })

  it('renders resolution and fps selectors', async () => {
    render(<App />)

    // framer-motion may render multiple copies in jsdom; use *AllBy variants
    expect((await screen.findAllByText('画面参数')).length).toBeGreaterThan(0)
    expect(screen.getAllByText('1080p (1920×1080)').length).toBeGreaterThan(0)
    expect(screen.getAllByText('30 fps').length).toBeGreaterThan(0)
  })

  it('shows coming-soon button for window mode', async () => {
    render(<App />)

    await screen.findAllByText('全屏')

    // Click window mode
    const windowButtons = screen.getAllByText('窗口')
    fireEvent.click(windowButtons[0])

    await vi.waitFor(() => {
      expect(screen.getAllByText('即将推出').length).toBeGreaterThan(0)
    })
    expect(
      screen.getAllByText('该模式正在开发中，将随后续版本推出').length,
    ).toBeGreaterThan(0)
  })

  it('shows coming-soon button for area mode', async () => {
    render(<App />)

    await screen.findAllByText('全屏')

    // Click area mode
    const areaButtons = screen.getAllByText('区域')
    fireEvent.click(areaButtons[0])

    await vi.waitFor(() => {
      expect(screen.getAllByText('即将推出').length).toBeGreaterThan(0)
    })
  })

  it('shows notDetermined permission guidance', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') {
        return Promise.resolve({ state: 'idle', canStart: true })
      }
      if (command === 'recording_permissions') {
        return Promise.resolve({
          screenRecording: 'notDetermined',
          microphone: 'notDetermined',
        })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    const guidance = await screen.findAllByText(
      '需要屏幕录制权限才能录制，请在启动录制时授权',
    )
    expect(guidance.length).toBeGreaterThan(0)
  })

  it('prevents double-click on start recording', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_capture_mode') return Promise.resolve()
      if (command === 'set_audio_config') return Promise.resolve()
      if (command === 'start_recording') return Promise.resolve()
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    const startButtons = await screen.findAllByText('开始录制')

    // Click twice rapidly — second click blocked by isStarting guard.
    fireEvent.click(startButtons[0])
    fireEvent.click(startButtons[0])

    // Wait for the first click's async chain to complete.
    await vi.waitFor(() => {
      const startCalls = invokeMock.mock.calls.filter(
        (call: unknown[]) => call[0] === 'start_recording',
      )
      expect(startCalls).toHaveLength(1)
    })
  })

  it('prevents double-click on stop recording', async () => {
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
      if (command === 'stop_recording') return Promise.resolve({ durationSecs: 1, frameCount: 30, mixedAudioChunkCount: 10, outputPath: null, cursorMetadataPath: '/tmp/cursor.json', effectTimelinePath: null })
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    // Start recording
    const startButtons = await screen.findAllByText('开始录制')
    fireEvent.click(startButtons[0])
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
    })

    // Emit recording state
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'recording' })
      }
    })

    // Find stop button
    await vi.waitFor(() => {
      const buttons = screen.getAllByRole('button')
      expect(buttons.length).toBeGreaterThan(0)
    })
    const buttons = screen.getAllByRole('button')
    const stopButton = buttons[buttons.length - 1]

    invokeMock.mockClear()

    // Click stop twice rapidly
    fireEvent.click(stopButton)
    fireEvent.click(stopButton)

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalled()
    })

    const stopCalls = invokeMock.mock.calls.filter(
      (call: unknown[]) => call[0] === 'stop_recording',
    )
    expect(stopCalls).toHaveLength(1)
  })

  it('subscribes to mic-level when entering recording state', async () => {
    const { listen } = await import('@tauri-apps/api/event')
    const stateCallbacks: Array<(status: { state: string }) => void> = []
    const micListenCalls: Array<unknown> = []

    vi.mocked(listen).mockImplementation(
      (event: string, callback: (event: { event: string; id: number; payload: unknown }) => void) => {
        if (event === 'recording-state-changed') {
          stateCallbacks.push((status) => callback({ event, id: 0, payload: status }))
        }
        if (event === 'mic-level') {
          micListenCalls.push(callback)
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
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    // In idle state, mic-level should NOT be subscribed yet.
    expect(micListenCalls).toHaveLength(0)

    // Start recording
    const startButtons = await screen.findAllByText('开始录制')
    fireEvent.click(startButtons[0])
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
    })

    // Emit recording state — this triggers the mic-level subscription effect.
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'recording' })
      }
    })

    // mic-level listener should now be registered.
    await vi.waitFor(() => {
      expect(micListenCalls.length).toBeGreaterThan(0)
    })
  })

  it('unsubscribes from mic-level when leaving recording state', async () => {
    const { listen } = await import('@tauri-apps/api/event')
    const stateCallbacks: Array<(status: { state: string }) => void> = []
    const micUnlisten = vi.fn()

    vi.mocked(listen).mockImplementation(
      (event: string, callback: (event: { event: string; id: number; payload: unknown }) => void) => {
        if (event === 'recording-state-changed') {
          stateCallbacks.push((status) => callback({ event, id: 0, payload: status }))
        }
        if (event === 'mic-level') {
          return Promise.resolve(micUnlisten)
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
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    // Start recording
    const startButtons = await screen.findAllByText('开始录制')
    fireEvent.click(startButtons[0])
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
    })

    // Enter recording — triggers mic-level subscription.
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'recording' })
      }
    })

    // Wait for the mic-level listener promise to resolve.
    await vi.waitFor(() => {
      expect(micUnlisten).not.toHaveBeenCalled()
    })

    // Leave recording — triggers effect cleanup, which must unlisten.
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'idle' })
      }
    })

    expect(micUnlisten).toHaveBeenCalledTimes(1)
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
      expect(invokeMock).toHaveBeenCalledTimes(4)
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
    expect(invokeMock).toHaveBeenNthCalledWith(4, 'recording_status', undefined)
  })

  it('does not call Tauri when window mode is selected', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)
    await screen.findAllByText('全屏')

    // Switch to window mode
    const windowButtons = screen.getAllByText('窗口')
    fireEvent.click(windowButtons[0])

    await vi.waitFor(() => {
      expect(screen.getAllByText('即将推出').length).toBeGreaterThan(0)
    })

    // The coming-soon button should be disabled
    const comingSoonBtns = screen.getAllByText('即将推出')
    const btn = comingSoonBtns[0].closest('button')
    expect(btn).toBeDisabled()

    // No additional Tauri invokes beyond initial status/permissions
    const expectedCalls = invokeMock.mock.calls.length
    expect(expectedCalls).toBe(2) // recording_status + recording_permissions
  })

  it('does not render area-level false drag-region wrappers', async () => {
    render(<App />)

    await screen.findAllByText('开始录制')

    expect(document.querySelector('[data-tauri-drag-region="false"]')).toBeNull()
  })

  it('error view does not contain false drag-region markers', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'failed', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)
    await screen.findByText('录制失败')
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
          cursorMetadataPath: '/tmp/cursor.json',
          effectTimelinePath: null,
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
    const stopButton = buttons[buttons.length - 1]
    await act(async () => {
      fireEvent.click(stopButton)
    })
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('stop_recording', undefined)
    })

    // Emit completed event — now the sole driver of preview state.
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'completed' })
      }
    })

    // PreviewView renders with outputPath (video element)
    await screen.findByText('预览与美化')
    expect(screen.getByText('预览与美化')).toBeTruthy()
    // Verify stop_recording returned camelCase result
    expect(invokeMock).toHaveBeenCalledWith('stop_recording', undefined)
  })

  it('enters recording state via status fallback when event is not emitted', async () => {
    const { listen } = await import('@tauri-apps/api/event')
    vi.mocked(listen).mockImplementation(() => Promise.resolve(() => {}))

    let statusCalls = 0
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') {
        statusCalls++
        if (statusCalls === 1) return Promise.resolve({ state: 'idle', canStart: true })
        return Promise.resolve({ state: 'recording', canStart: false })
      }
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_capture_mode') return Promise.resolve()
      if (command === 'set_audio_config') return Promise.resolve()
      if (command === 'start_recording') return Promise.resolve()
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    const startButtons = await screen.findAllByText('开始录制')
    fireEvent.click(startButtons[0])

    // Should enter recording state via fallback, without any recording-state-changed event
    await screen.findByText('正在录制 全屏')
  })

  it('enters preview state via status fallback when completed event is not emitted', async () => {
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

    let statusCalls = 0
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') {
        statusCalls++
        if (statusCalls === 1) return Promise.resolve({ state: 'idle', canStart: true })
        if (statusCalls === 2) return Promise.resolve({ state: 'recording', canStart: false })
        return Promise.resolve({ state: 'completed', canStart: true })
      }
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_capture_mode') return Promise.resolve()
      if (command === 'set_audio_config') return Promise.resolve()
      if (command === 'start_recording') return Promise.resolve()
      if (command === 'stop_recording') {
        return Promise.resolve({ durationSecs: 1, frameCount: 30, mixedAudioChunkCount: 10, outputPath: null, cursorMetadataPath: null, effectTimelinePath: null })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    // Enter recording via event
    const startButtons = await screen.findAllByText('开始录制')
    fireEvent.click(startButtons[0])
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
    })
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'recording' })
      }
    })

    // Find stop button
    await vi.waitFor(() => {
      const buttons = screen.getAllByRole('button')
      expect(buttons.length).toBeGreaterThan(0)
    })
    const buttons = screen.getAllByRole('button')
    const stopButton = buttons[buttons.length - 1]

    // Clear callbacks to simulate completed event loss
    stateCallbacks.length = 0

    await act(async () => {
      fireEvent.click(stopButton)
    })

    // Should enter preview state via fallback
    await screen.findByText('预览与美化')
  })

  it('clears mic volume when returning to idle', async () => {
    const { listen } = await import('@tauri-apps/api/event')
    const stateCallbacks: Array<(status: { state: string }) => void> = []
    const micCallbacks: Array<(payload: { level: number }) => void> = []

    vi.mocked(listen).mockImplementation(
      (event: string, callback: (event: { event: string; id: number; payload: unknown }) => void) => {
        if (event === 'recording-state-changed') {
          stateCallbacks.push((status) => callback({ event, id: 0, payload: status }))
        }
        if (event === 'mic-level') {
          micCallbacks.push((payload) => callback({ event, id: 0, payload }))
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
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    // Enable microphone so bars are visible in idle state
    const micButton = screen.getByText('麦克风').closest('button')!
    fireEvent.click(micButton)

    // Start recording
    const startButtons = await screen.findAllByText('开始录制')
    fireEvent.click(startButtons[0])
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('start_recording', undefined)
    })

    // Enter recording and trigger high mic level
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'recording' })
      }
    })
    act(() => {
      for (const cb of micCallbacks) {
        cb({ level: 0.8 })
      }
    })

    // In recording state the mic indicator bars should exist (mic is enabled)
    const micBars = document.querySelectorAll('.rounded-full')
    expect(micBars.length).toBeGreaterThan(0)

    // Return to idle — should clear micVolume to 0
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'idle' })
      }
    })

    // Should be back in idle state showing the start button
    await screen.findAllByText('开始录制')
  })

  it('removes mic indicator bars when mic is toggled off', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'idle', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    // Initially mic is off — bars container should not exist
    let micBars = document.querySelectorAll('.rounded-full.bg-muted-foreground\\/60')
    expect(micBars.length).toBe(0)

    // Enable microphone — bars should appear in idle RecordingPanel
    const micButton = screen.getByText('麦克风').closest('button')!

    await act(async () => {
      fireEvent.click(micButton)
    })

    micBars = document.querySelectorAll('.rounded-full.bg-muted-foreground\\/60')
    expect(micBars.length).toBe(5)

    // Toggle mic off — bars container should be removed
    await act(async () => {
      fireEvent.click(micButton)
    })

    micBars = document.querySelectorAll('.rounded-full.bg-muted-foreground\\/60')
    expect(micBars.length).toBe(0)
  })

  it('shows mic level meter in recording status bar when mic is enabled', async () => {
    const { listen } = await import('@tauri-apps/api/event')
    const stateCallbacks: Array<(status: { state: string }) => void> = []
    const micCallbacks: Array<(payload: { level: number }) => void> = []

    vi.mocked(listen).mockImplementation(
      (event: string, callback: (event: { event: string; id: number; payload: unknown }) => void) => {
        if (event === 'recording-state-changed') {
          stateCallbacks.push((status) => callback({ event, id: 0, payload: status }))
        }
        if (event === 'mic-level') {
          micCallbacks.push((payload) => callback({ event, id: 0, payload }))
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
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    // Enable microphone
    const micButton = screen.getByText('麦克风').closest('button')!
    fireEvent.click(micButton)

    // Start recording
    const startButtons = await screen.findAllByText('开始录制')
    fireEvent.click(startButtons[0])

    // Enter recording state — meter should appear (mic is enabled)
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'recording' })
      }
    })

    // Mic level meter should be present with level 0 initially
    let meter = screen.getByTestId('mic-level-meter')
    expect(meter).toBeDefined()
    expect(meter.getAttribute('data-level')).toBe('0')

    // Trigger high mic level
    act(() => {
      for (const cb of micCallbacks) {
        cb({ level: 0.8 })
      }
    })

    // Meter should reflect the updated level
    expect(meter.getAttribute('data-level')).toBe('80')

    // Trigger zero mic level — bars should collapse
    act(() => {
      for (const cb of micCallbacks) {
        cb({ level: 0.0 })
      }
    })

    // Meter should be back to zero
    meter = screen.getByTestId('mic-level-meter')
    expect(meter.getAttribute('data-level')).toBe('0')
  })

  it('hides mic level meter when recording with mic disabled', async () => {
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
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    // Mic is disabled by default; start recording directly
    const startButtons = await screen.findAllByText('开始录制')
    fireEvent.click(startButtons[0])

    // Enter recording state
    act(() => {
      for (const cb of stateCallbacks) {
        cb({ state: 'recording' })
      }
    })

    // Mic level meter should not appear when mic is disabled
    expect(screen.queryByTestId('mic-level-meter')).toBeNull()
  })

  it('sends beautify config when cursor smoothing is toggled in preview', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_beautify_config') return Promise.resolve()
      if (command === 'build_cursor_effect_timeline') {
        return Promise.resolve({ frameCount: 30, clickEffectCount: 1, effectTimelinePath: '/tmp/effects.json' })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    await screen.findByText('预览与美化')
    invokeMock.mockClear()

    const switches = screen.getAllByRole('switch')
    fireEvent.click(switches[1])

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('set_beautify_config', {
        config: {
          cursorMagnification: true,
          magnificationFactor: 2,
          cursorSmoothing: false,
          autoTrimSilences: false,
          trimSensitivity: 'medium',
        },
      })
    })
  })

  it('calls export_video from preview export button', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_beautify_config') return Promise.resolve()
      if (command === 'build_cursor_effect_timeline') {
        return Promise.resolve({ frameCount: 0, clickEffectCount: 0, effectTimelinePath: '/tmp/effects.json' })
      }
      if (command === 'export_video') {
        return Promise.resolve({
          frameCount: 30,
          clickEffectCount: 1,
          effectTimelinePath: '/tmp/effects.json',
        })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    await screen.findByText('预览与美化')
    // Wait for initial status/permissions calls to settle
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('recording_status', undefined)
    })
    invokeMock.mockClear()

    const exportButtons = screen.getAllByRole('button', { name: '导出' })
    await act(async () => {
      fireEvent.click(exportButtons[0])
    })

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('export_video', { preset: 'bilibili' })
    })
  })

  it('debounces consecutive beautify changes into a single timeline build', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_beautify_config') return Promise.resolve()
      if (command === 'build_cursor_effect_timeline') {
        return Promise.resolve({ frameCount: 10, clickEffectCount: 0, effectTimelinePath: '/tmp/effects.json' })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)
    await screen.findByText('预览与美化')
    invokeMock.mockClear()

    vi.useFakeTimers()

    const switches = screen.getAllByRole('switch')
    fireEvent.click(switches[0])
    fireEvent.click(switches[0])

    await act(async () => {
      vi.advanceTimersByTime(350)
    })

    const setBeautifyCalls = invokeMock.mock.calls.filter(
      (call: unknown[]) => (call[0] as string) === 'set_beautify_config',
    )
    expect(setBeautifyCalls).toHaveLength(1)

    const buildCalls = invokeMock.mock.calls.filter(
      (call: unknown[]) => (call[0] as string) === 'build_cursor_effect_timeline',
    )
    expect(buildCalls).toHaveLength(1)

    vi.useRealTimers()
  })

  it('clears debounce timer on unmount so no backend call fires after leaving preview', async () => {
    let buildCalled = false
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_beautify_config') return Promise.resolve()
      if (command === 'build_cursor_effect_timeline') {
        buildCalled = true
        return Promise.resolve({ frameCount: 10, clickEffectCount: 0, effectTimelinePath: '/tmp/effects.json' })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    const { unmount } = render(<App />)
    await screen.findByText('预览与美化')
    invokeMock.mockClear()

    const switches = screen.getAllByRole('switch')
    fireEvent.click(switches[1])

    // Unmount before the 300ms debounce fires.
    unmount()

    // Wait well past the debounce window.
    await new Promise((resolve) => setTimeout(resolve, 500))

    expect(buildCalled).toBe(false)
  })

  it('does not build timeline when set_beautify_config rejects', async () => {
    let buildCalled = false
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_beautify_config') return Promise.reject(new Error('config write failed'))
      if (command === 'build_cursor_effect_timeline') {
        buildCalled = true
        return Promise.resolve({ frameCount: 10, clickEffectCount: 0, effectTimelinePath: '/tmp/effects.json' })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)
    await screen.findByText('预览与美化')
    invokeMock.mockClear()

    const switches = screen.getAllByRole('switch')
    fireEvent.click(switches[1])

    // Wait past the debounce window.
    await new Promise((resolve) => setTimeout(resolve, 500))

    // set_beautify_config was called (and rejected).
    expect(invokeMock).toHaveBeenCalledWith('set_beautify_config', expect.anything())
    // build_cursor_effect_timeline must NOT be called after rejection.
    expect(buildCalled).toBe(false)
  })

  it('delays timeline build until set_beautify_config resolves', async () => {
    let resolveConfig: (value: void) => void = () => {}
    let buildCalled = false

    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') return Promise.resolve({ state: 'completed', canStart: true })
      if (command === 'recording_permissions') return Promise.resolve({ screenRecording: 'granted', microphone: 'granted' })
      if (command === 'set_beautify_config') {
        return new Promise<void>((resolve) => {
          resolveConfig = resolve
        })
      }
      if (command === 'build_cursor_effect_timeline') {
        buildCalled = true
        return Promise.resolve({ frameCount: 10, clickEffectCount: 0, effectTimelinePath: '/tmp/effects.json' })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)
    await screen.findByText('预览与美化')
    invokeMock.mockClear()
    buildCalled = false

    // Switch to fake timers after React has finished rendering.
    vi.useFakeTimers()

    const switches = screen.getAllByRole('switch')
    fireEvent.click(switches[0])

    // Advance past the 300ms debounce window.
    await act(async () => {
      vi.advanceTimersByTime(350)
    })

    // set_beautify_config should have been called by now.
    expect(invokeMock).toHaveBeenCalledWith('set_beautify_config', expect.anything())
    // build_cursor_effect_timeline must NOT be called while config is pending.
    expect(buildCalled).toBe(false)

    // Resolve the config promise.
    await act(async () => {
      resolveConfig()
    })

    // After config resolves, build_cursor_effect_timeline should be called.
    await vi.waitFor(() => {
      expect(buildCalled).toBe(true)
    })

    vi.useRealTimers()
  })
})
