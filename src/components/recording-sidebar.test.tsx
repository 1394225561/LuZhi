import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { RecordingSidebar } from './recording-sidebar'
import {
  deleteRecording,
  listRecordings,
  type LibraryEntrySummary,
} from '@/lib/tauri'

vi.mock('@/lib/tauri', () => ({
  listRecordings: vi.fn(),
  deleteRecording: vi.fn(),
  importRecording: vi.fn(),
}))

const listRecordingsMock = vi.mocked(listRecordings)
const deleteRecordingMock = vi.mocked(deleteRecording)

const recordings: LibraryEntrySummary[] = [
  { id: 'recent-recording', createdAt: 1780827600000, durationSecs: 75 },
  { id: 'older-recording', createdAt: 1780741200000, durationSecs: 180 },
]

describe('RecordingSidebar', () => {
  beforeEach(() => {
    listRecordingsMock.mockReset()
    deleteRecordingMock.mockReset()
    listRecordingsMock.mockResolvedValue([...recordings])
    deleteRecordingMock.mockResolvedValue()
  })

  afterEach(() => {
    cleanup()
  })

  it('renders compact history count and recording durations', async () => {
    render(
      <RecordingSidebar
        isOpen={true}
        onToggle={vi.fn()}
        onSelectRecording={vi.fn()}
      />,
    )

    expect(await screen.findByText('共 2 条录制')).toBeInTheDocument()
    expect(screen.getByText('时长：1:15')).toBeInTheDocument()
    expect(screen.getByText('时长：3:00')).toBeInTheDocument()
  })

  it('keeps the empty state clear when no recordings exist', async () => {
    listRecordingsMock.mockResolvedValueOnce([])

    render(
      <RecordingSidebar
        isOpen={true}
        onToggle={vi.fn()}
        onSelectRecording={vi.fn()}
      />,
    )

    expect(await screen.findByText('暂无历史录制')).toBeInTheDocument()
    expect(screen.getByText('录制完成后会出现在这里')).toBeInTheDocument()
  })

  it('selects a recording when a history row is clicked', async () => {
    const onSelectRecording = vi.fn()

    render(
      <RecordingSidebar
        isOpen={true}
        onToggle={vi.fn()}
        onSelectRecording={onSelectRecording}
      />,
    )

    const rowText = await screen.findByText('时长：1:15')
    fireEvent.click(rowText)

    expect(onSelectRecording).toHaveBeenCalledWith('recent-recording')
  })

  it('deletes a recording without selecting it', async () => {
    const onSelectRecording = vi.fn()

    render(
      <RecordingSidebar
        isOpen={true}
        onToggle={vi.fn()}
        onSelectRecording={onSelectRecording}
      />,
    )

    await screen.findByText('共 2 条录制')
    fireEvent.click(screen.getAllByTitle('删除')[0])

    await waitFor(() => {
      expect(deleteRecordingMock).toHaveBeenCalledWith('recent-recording')
    })
    expect(onSelectRecording).not.toHaveBeenCalled()
  })

  it('keeps the collapsed panel from leaving a right-edge border line', () => {
    const { container } = render(
      <RecordingSidebar
        isOpen={false}
        onToggle={vi.fn()}
        onSelectRecording={vi.fn()}
      />,
    )

    const panel = container.querySelector('div.fixed.right-0.top-0')

    expect(panel).toBeInTheDocument()
    expect(panel).toHaveClass('w-0')
    expect(panel).toHaveClass('border-l-0')
    expect(panel).toHaveClass('shadow-none')
    expect(panel).toHaveClass('pointer-events-none')
  })

  it('uses a borderless toggle positioned near the recording panel when collapsed', () => {
    render(
      <RecordingSidebar
        isOpen={false}
        onToggle={vi.fn()}
        onSelectRecording={vi.fn()}
      />,
    )

    const toggle = screen.getByTitle('历史录制')

    expect(toggle).toHaveClass('right-[max(12px,calc(50%_-_208px))]')
    expect(toggle).not.toHaveClass('right-0')
    expect(toggle).not.toHaveClass('border')
    expect(toggle).not.toHaveClass('shadow-lg')
  })
})
