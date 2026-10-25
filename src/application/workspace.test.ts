import { describe, expect, it, vi } from 'vitest'
import type { NoteEntry } from '../core/notes'
import type { NoteRepository } from './ports'
import { WorkspaceService } from './workspace'

const note: NoteEntry = {
  path: 'hello.md',
  name: 'hello.md',
  title: 'hello',
  modifiedMs: 1,
}

function repository(): NoteRepository {
  return {
    listNotes: vi.fn(async () => [note]),
    readNote: vi.fn(async () => ({ content: '# hello', revision: 'rev-1' })),
    writeNote: vi.fn(async () => 'rev-2'),
    createNote: vi.fn(async () => note),
    deleteNote: vi.fn(async () => ({ path: note.path, content: '# hello' })),
    restoreNote: vi.fn(async () => note),
    renameNote: vi.fn(async () => note),
    searchNotes: vi.fn(async (_root, _query, _requestId, onBatch) => {
      onBatch([{ path: 'hello.md', title: 'hello', line: 1, preview: '# hello' }])
      return { total: 1, truncated: false, tookMs: 3, source: 'snapshot' as const }
    }),
    cancelSearch: vi.fn(async () => {}),
    snapshotStatus: vi.fn(async () => null),
    onWorkspaceChanged: vi.fn(async () => () => {}),
  }
}

describe('WorkspaceService', () => {
  it('delegates each use case to the repository', async () => {
    const port = repository()
    const service = new WorkspaceService(port)

    expect(await service.list('/notes')).toEqual([note])
    expect(await service.read('/notes', 'hello.md')).toEqual({
      content: '# hello',
      revision: 'rev-1',
    })
    expect(await service.save('/notes', 'hello.md', '# updated', 'rev-1')).toBe('rev-2')
    expect(await service.create('/notes', 'hello')).toEqual(note)
    expect(await service.delete('/notes', 'hello.md')).toEqual({
      path: 'hello.md',
      content: '# hello',
    })
    expect(await service.restore('/notes', 'hello.md', '# hello')).toEqual(note)
    expect(await service.rename('/notes', 'hello.md', 'renamed')).toEqual(note)

    const batch = vi.fn()
    expect(await service.search('/notes', 'hello', 7, batch)).toEqual({
      total: 1,
      truncated: false,
      tookMs: 3,
      source: 'snapshot',
    })
    expect(batch).toHaveBeenCalledWith([
      { path: 'hello.md', title: 'hello', line: 1, preview: '# hello' },
    ])
    expect(await service.snapshotStatus()).toBeNull()

    expect(port.listNotes).toHaveBeenCalledWith('/notes')
    expect(port.readNote).toHaveBeenCalledWith('/notes', 'hello.md')
    expect(port.writeNote).toHaveBeenCalledWith('/notes', 'hello.md', '# updated', 'rev-1')
    expect(port.createNote).toHaveBeenCalledWith('/notes', 'hello')
    expect(port.deleteNote).toHaveBeenCalledWith('/notes', 'hello.md')
    expect(port.restoreNote).toHaveBeenCalledWith('/notes', 'hello.md', '# hello')
    expect(port.renameNote).toHaveBeenCalledWith('/notes', 'hello.md', 'renamed')
    expect(port.searchNotes).toHaveBeenCalledWith('/notes', 'hello', 7, batch)

    const onDelta = vi.fn()
    const stop = await service.onWorkspaceChanged(onDelta)
    expect(port.onWorkspaceChanged).toHaveBeenCalledWith(onDelta)
    stop()
  })

  it('propagates repository errors unchanged', async () => {
    const port = repository()
    port.readNote = vi.fn(async () => {
      throw new Error('note path escapes the workspace')
    })
    const service = new WorkspaceService(port)

    await expect(service.read('/notes', '../secret.md')).rejects.toThrow('escapes')
  })
})
