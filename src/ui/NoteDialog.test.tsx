// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { NoteDialog } from './NoteDialog'

afterEach(() => {
  cleanup()
})

const base = {
  fileName: 'hello.md',
  initialTitle: 'hello',
  busy: false,
  error: null,
  onCancel: vi.fn(),
  onRename: vi.fn(),
  onDelete: vi.fn(),
}

describe('NoteDialog', () => {
  it('submits a trimmed rename', async () => {
    const user = userEvent.setup()
    const onRename = vi.fn()
    render(<NoteDialog {...base} mode="rename" onRename={onRename} />)

    const input = screen.getByLabelText('新名称')
    await user.clear(input)
    await user.type(input, '  renamed  ')
    await user.click(screen.getByRole('button', { name: '重命名' }))

    expect(onRename).toHaveBeenCalledWith('renamed')
  })

  it('requires explicit confirmation before deleting', async () => {
    const user = userEvent.setup()
    const onDelete = vi.fn()
    render(<NoteDialog {...base} mode="delete" onDelete={onDelete} />)

    expect(screen.getByText(/移入系统回收站/)).toBeTruthy()
    expect(onDelete).not.toHaveBeenCalled()

    await user.click(screen.getByRole('button', { name: '删除' }))
    expect(onDelete).toHaveBeenCalledTimes(1)
  })

  it('cancels on Escape', async () => {
    const user = userEvent.setup()
    const onCancel = vi.fn()
    render(<NoteDialog {...base} mode="delete" onCancel={onCancel} />)

    await user.keyboard('{Escape}')
    expect(onCancel).toHaveBeenCalledTimes(1)
  })
})
