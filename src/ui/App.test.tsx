// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { App } from './App'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
  openUrl: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: mocks.openUrl }))

const notes = [
  { path: 'hello.md', name: 'hello.md', title: 'hello', modifiedMs: null },
  {
    path: 'ideas/architecture.md',
    name: 'architecture.md',
    title: 'architecture',
    modifiedMs: null,
  },
]

afterEach(() => {
  cleanup()
})

beforeEach(() => {
  localStorage.clear()
  mocks.invoke.mockReset()
  mocks.open.mockReset()
  mocks.openUrl.mockReset()

  mocks.open.mockResolvedValue('C:\\notes')
  mocks.invoke.mockImplementation((command: string) => {
    if (command === 'list_notes') return Promise.resolve(notes)
    if (command === 'read_note') return Promise.resolve('# Hello\n\nworld')
    if (command === 'write_note') return Promise.resolve()
    if (command === 'create_note') return Promise.resolve(notes[0])
    return Promise.reject(new Error(`unexpected command: ${command}`))
  })
})

describe('App', () => {
  it('renders the empty state before a workspace is chosen', () => {
    render(<App />)
    expect(screen.getByRole('heading', { name: '把文件夹变成写作工作区' })).toBeTruthy()
  })

  it('opens a workspace, edits a note, previews it, and filters the tree', async () => {
    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole('button', { name: '打开 Markdown 文件夹' }))
    expect(await screen.findByText('hello.md')).toBeTruthy()
    expect(screen.getByText('2 篇')).toBeTruthy()
    expect(mocks.invoke).toHaveBeenCalledWith('list_notes', { root: 'C:\\notes' })

    await user.click(screen.getByRole('button', { name: /hello\.md/ }))

    const editor = (await screen.findByLabelText('Markdown 编辑器')) as HTMLTextAreaElement
    expect(editor.value).toContain('# Hello')
    expect(await screen.findByRole('heading', { level: 1, name: 'Hello' })).toBeTruthy()

    await user.type(screen.getByPlaceholderText('按文件名或路径搜索'), 'architecture')
    expect(screen.queryByRole('button', { name: /hello\.md/ })).toBeNull()
    expect(screen.getByRole('button', { name: /architecture\.md/ })).toBeTruthy()
  })
})
