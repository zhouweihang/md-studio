// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { App } from './App'

const mocks = vi.hoisted(() => {
  const closeListeners: Array<(event: { preventDefault: () => void }) => void> = []
  return {
    invoke: vi.fn(),
    open: vi.fn(),
    openUrl: vi.fn(),
    closeListeners,
    close: vi.fn(async () => {}),
    onCloseRequested: vi.fn(
      async (handler: (event: { preventDefault: () => void }) => void) => {
        closeListeners.push(handler)
        return () => {
          const index = closeListeners.indexOf(handler)
          if (index >= 0) closeListeners.splice(index, 1)
        }
      },
    ),
  }
})

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: mocks.openUrl }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    close: mocks.close,
    onCloseRequested: mocks.onCloseRequested,
  }),
}))

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
  mocks.close.mockClear()
  mocks.onCloseRequested.mockClear()
  mocks.closeListeners.length = 0

  mocks.open.mockResolvedValue('C:\\notes')
  mocks.invoke.mockImplementation((command: string) => {
    if (command === 'list_notes') return Promise.resolve(notes)
    if (command === 'read_note') {
      return Promise.resolve({ content: '# Hello\n\nworld', revision: 'rev-1' })
    }
    if (command === 'write_note') return Promise.resolve('rev-2')
    if (command === 'create_note') return Promise.resolve(notes[0])
    if (command === 'delete_note') {
      return Promise.resolve({ path: 'hello.md', content: '# Hello\n\nworld' })
    }
    if (command === 'restore_note') return Promise.resolve(notes[0])
    return Promise.reject(new Error(`unexpected command: ${command}`))
  })
})

async function openFirstNote() {
  const user = userEvent.setup()
  render(<App />)

  await user.click(screen.getByRole('button', { name: '打开 Markdown 文件夹' }))
  await screen.findByText('hello.md')
  await user.click(screen.getByRole('button', { name: /hello\.md/ }))

  const editor = (await screen.findByLabelText('Markdown 编辑器')) as HTMLTextAreaElement
  await waitFor(() => expect(mocks.closeListeners).toHaveLength(1))

  return { user, editor }
}

async function openConflict() {
  let reads = 0
  mocks.invoke.mockImplementation((command: string, payload?: { expectedRevision?: string }) => {
    if (command === 'list_notes') return Promise.resolve(notes)
    if (command === 'read_note') {
      reads += 1
      return reads === 1
        ? Promise.resolve({ content: '# Hello\n\nworld', revision: 'rev-1' })
        : Promise.resolve({ content: '# External\n\nedit', revision: 'rev-2' })
    }
    if (command === 'write_note') {
      return payload?.expectedRevision === 'rev-2'
        ? Promise.resolve('rev-3')
        : Promise.reject('conflict: note was modified outside MD Studio')
    }
    return Promise.reject(new Error(`unexpected command: ${command}`))
  })

  return openFirstNote()
}

async function requestClose() {
  const event = { preventDefault: vi.fn() }
  const handler = mocks.closeListeners[0]
  await handler(event)
  return event
}

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
    expect(mocks.invoke).toHaveBeenCalledWith('read_note', {
      root: 'C:\\notes',
      path: 'hello.md',
    })
    expect(await screen.findByRole('heading', { level: 1, name: 'Hello' })).toBeTruthy()

    await user.type(screen.getByPlaceholderText('搜索文件名或正文'), 'architecture')
    expect(screen.queryByRole('button', { name: /hello\.md/ })).toBeNull()
    expect(screen.getByRole('button', { name: /architecture\.md/ })).toBeTruthy()
  })

  it('creates a note from the sidebar and opens it', async () => {
    const created = {
      path: 'idea.md',
      name: 'idea.md',
      title: 'idea',
      modifiedMs: null,
    }
    let listings = 0
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'list_notes') {
        listings += 1
        return Promise.resolve(listings === 1 ? notes : [...notes, created])
      }
      if (command === 'create_note') return Promise.resolve(created)
      if (command === 'read_note') {
        return Promise.resolve({ content: '', revision: 'rev-new' })
      }
      return Promise.reject(new Error(`unexpected command: ${command}`))
    })

    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole('button', { name: '打开 Markdown 文件夹' }))
    await screen.findByText('hello.md')

    await user.type(screen.getByLabelText('新笔记名称'), 'idea')
    await user.click(screen.getByRole('button', { name: '新建' }))

    expect(mocks.invoke).toHaveBeenCalledWith('create_note', {
      root: 'C:\\notes',
      title: 'idea',
    })
    expect(await screen.findByRole('button', { name: /idea\.md/ })).toBeTruthy()

    const editor = (await screen.findByLabelText('Markdown 编辑器')) as HTMLTextAreaElement
    expect(editor.value).toBe('')
    expect((screen.getByLabelText('新笔记名称') as HTMLInputElement).value).toBe('')
  })

  it('flushes pending edits before the window closes', async () => {
    const { user, editor } = await openFirstNote()

    await user.type(editor, '!')

    const event = await requestClose()

    expect(event.preventDefault).toHaveBeenCalled()
    await waitFor(() => expect(mocks.close).toHaveBeenCalledTimes(1))
    expect(mocks.invoke).toHaveBeenCalledWith('write_note', {
      root: 'C:\\notes',
      path: 'hello.md',
      content: '# Hello\n\nworld!',
      expectedRevision: 'rev-1',
    })
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('keeps the window open and shows the conflict banner when the file changed on disk', async () => {
    const { user, editor } = await openConflict()

    await user.type(editor, '!')
    const event = await requestClose()

    expect(event.preventDefault).toHaveBeenCalled()
    expect(mocks.close).not.toHaveBeenCalled()
    expect(await screen.findByText('磁盘文件已被外部修改。')).toBeTruthy()
    expect(screen.getByText('磁盘已变更')).toBeTruthy()
  })

  it('overwrites the disk file with the local text after a conflict', async () => {
    const { user, editor } = await openConflict()

    await user.type(editor, '!')
    await requestClose()
    await screen.findByText('磁盘文件已被外部修改。')

    await user.click(screen.getByRole('button', { name: '覆盖保存' }))

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('write_note', {
        root: 'C:\\notes',
        path: 'hello.md',
        content: '# Hello\n\nworld!',
        expectedRevision: 'rev-2',
      }),
    )
    await waitFor(() => expect(screen.queryByText('磁盘文件已被外部修改。')).toBeNull())
    expect(screen.getByText('已保存')).toBeTruthy()
  })

  it('moves a deleted note to the system trash and restores it from the undo toast', async () => {
    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole('button', { name: '打开 Markdown 文件夹' }))
    await screen.findByText('hello.md')
    await user.click(screen.getByRole('button', { name: /hello\.md/ }))
    await screen.findByLabelText('Markdown 编辑器')

    await user.click(screen.getByRole('button', { name: '删除' }))
    const dialog = await screen.findByRole('dialog')
    await user.click(within(dialog).getByRole('button', { name: '删除' }))

    expect(await screen.findByText('已移入系统回收站')).toBeTruthy()
    expect(mocks.invoke).toHaveBeenCalledWith('delete_note', {
      root: 'C:\\notes',
      path: 'hello.md',
    })

    await user.click(screen.getByRole('button', { name: '撤销' }))

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('restore_note', {
        root: 'C:\\notes',
        path: 'hello.md',
        content: '# Hello\n\nworld',
      }),
    )
    const editor = (await screen.findByLabelText('Markdown 编辑器')) as HTMLTextAreaElement
    expect(editor.value).toBe('# Hello\n\nworld')
    await waitFor(() => expect(screen.queryByText('已移入系统回收站')).toBeNull())
  })

  it('hides the undo action when no content snapshot is available', async () => {
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'list_notes') return Promise.resolve(notes)
      if (command === 'read_note') {
        return Promise.resolve({ content: '# Hello\n\nworld', revision: 'rev-1' })
      }
      if (command === 'delete_note') return Promise.resolve({ path: 'hello.md', content: null })
      return Promise.reject(new Error(`unexpected command: ${command}`))
    })

    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole('button', { name: '打开 Markdown 文件夹' }))
    await screen.findByText('hello.md')
    await user.click(screen.getByRole('button', { name: /hello\.md/ }))
    await screen.findByLabelText('Markdown 编辑器')

    await user.click(screen.getByRole('button', { name: '删除' }))
    const dialog = await screen.findByRole('dialog')
    await user.click(within(dialog).getByRole('button', { name: '删除' }))

    expect(await screen.findByText('已移入系统回收站')).toBeTruthy()
    expect(
      screen.getByText('文件超过 5 MiB 或非 UTF-8，无法在应用内撤销。'),
    ).toBeTruthy()
    expect(screen.queryByRole('button', { name: '撤销' })).toBeNull()
  })

  it('highlights the query inside content search hits', async () => {
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'list_notes') return Promise.resolve(notes)
      if (command === 'read_note') {
        return Promise.resolve({ content: '# Hello\n\nworld', revision: 'rev-1' })
      }
      if (command === 'search_notes') {
        return Promise.resolve([
          {
            path: 'ideas/architecture.md',
            title: 'architecture',
            line: 3,
            preview: 'The autosave loop is subtle.',
          },
        ])
      }
      return Promise.reject(new Error(`unexpected command: ${command}`))
    })

    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole('button', { name: '打开 Markdown 文件夹' }))
    await screen.findByText('hello.md')
    await user.type(screen.getByPlaceholderText('搜索文件名或正文'), 'AutoSave')

    const mark = await screen.findByText('autosave', { selector: 'mark' })
    expect(mark.textContent).toBe('autosave')
    expect(mocks.invoke).toHaveBeenCalledWith('search_notes', {
      root: 'C:\\notes',
      query: 'AutoSave',
    })
  })

  it('reloads the disk version and discards the local text after a conflict', async () => {
    const { user, editor } = await openConflict()

    await user.type(editor, '!')
    await requestClose()
    await screen.findByText('磁盘文件已被外部修改。')

    await user.click(screen.getByRole('button', { name: '重新载入' }))

    await waitFor(() => expect(editor.value).toBe('# External\n\nedit'))
    await waitFor(() => expect(screen.queryByText('磁盘文件已被外部修改。')).toBeNull())
  })
})
