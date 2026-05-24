import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'

const invokeMock = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string) => invokeMock(command),
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

  it('renders Chinese recording controls', async () => {
    render(<App />)

    expect(await screen.findByText('录智')).toBeInTheDocument()
    expect(screen.getByText('全屏录制')).toBeInTheDocument()
    expect(screen.getByText('开始录制')).toBeInTheDocument()
  })

  it('requests status and permissions from Tauri', async () => {
    render(<App />)

    await screen.findAllByText('待录制')

    expect(invokeMock).toHaveBeenCalledWith('recording_status')
    expect(invokeMock).toHaveBeenCalledWith('recording_permissions')
  })
})
