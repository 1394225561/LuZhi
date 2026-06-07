import { describe, it, expect, vi, beforeEach } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { WindowSelector } from './window-selector'
import { listWindows, type WindowInfo } from '@/lib/tauri'

vi.mock('@/lib/tauri', () => ({
  listWindows: vi.fn(),
}))

const mockWindows: WindowInfo[] = [
  {
    windowId: 1,
    title: 'Safari',
    appName: 'Safari',
    bundleId: 'com.apple.Safari',
    isOnScreen: true,
    width: 1920,
    height: 1080,
    thumbnail: null,
  },
  {
    windowId: 2,
    title: 'Terminal',
    appName: 'Terminal',
    bundleId: 'com.apple.Terminal',
    isOnScreen: false,
    width: 800,
    height: 600,
    thumbnail: null,
  },
]

describe('WindowSelector', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(listWindows).mockResolvedValue(mockWindows)
  })

  it('renders window list when open', async () => {
    render(<WindowSelector isOpen={true} onSelect={vi.fn()} onClose={vi.fn()} />)

    await waitFor(() => {
      expect(screen.getByText('选择录制窗口')).toBeInTheDocument()
    })
  })

  it('does not render when closed', () => {
    const { container } = render(<WindowSelector isOpen={false} onSelect={vi.fn()} onClose={vi.fn()} />)

    expect(container.innerHTML).toBe('')
  })

  it('shows window titles', async () => {
    render(<WindowSelector isOpen={true} onSelect={vi.fn()} onClose={vi.fn()} />)

    await waitFor(() => {
      const titles = screen.getAllByText(/Safari|Terminal/)
      expect(titles.length).toBeGreaterThan(0)
    })
  })

  it('shows loading state', () => {
    vi.mocked(listWindows).mockReturnValue(new Promise(() => {}))

    render(<WindowSelector isOpen={true} onSelect={vi.fn()} onClose={vi.fn()} />)

    // Loading spinner should be visible
    expect(document.querySelector('.animate-spin')).toBeInTheDocument()
  })

  it('disables minimized windows', async () => {
    render(<WindowSelector isOpen={true} onSelect={vi.fn()} onClose={vi.fn()} />)

    await waitFor(() => {
      const terminalButtons = screen.getAllByText('Terminal')
      const terminalCard = terminalButtons[0].closest('button')
      expect(terminalCard).toBeDisabled()
    })
  })

  it('shows list_windows errors and allows retry', async () => {
    vi.mocked(listWindows)
      .mockRejectedValueOnce(new Error('permission denied'))
      .mockResolvedValueOnce(mockWindows)

    render(<WindowSelector isOpen={true} onSelect={vi.fn()} onClose={vi.fn()} />)

    await waitFor(() => {
      expect(screen.getByText(/窗口列表加载失败/)).toBeInTheDocument()
    })

    fireEvent.click(screen.getByRole('button', { name: /重试/ }))

    await waitFor(() => {
      expect(screen.getAllByText('Safari').length).toBeGreaterThan(0)
    })
    expect(listWindows).toHaveBeenCalledTimes(2)
  })
})
