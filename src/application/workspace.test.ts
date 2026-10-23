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
    readNote: vi.fn(async () => '# hello'),
    writeNote: vi.fn(async () => {}),
    createNote: vi.fn(async () => note),
  }
}

describe('WorkspaceService', () => {
  it('delegates each use case to the repository', async () => {
    const port = repository()
    const service = new WorkspaceService(port)

    expect(await service.list('/notes')).toEqual([note])
    expect(await service.read('/notes', 'hello.md')).toBe('# hello')
    await service.save('/notes', 'hello.md', '# updated')
    expect(await service.create('/notes', 'hello')).toEqual(note)

    expect(port.listNotes).toHaveBeenCalledWith('/notes')
    expect(port.readNote).toHaveBeenCalledWith('/notes', 'hello.md')
    expect(port.writeNote).toHaveBeenCalledWith('/notes', 'hello.md', '# updated')
    expect(port.createNote).toHaveBeenCalledWith('/notes', 'hello')
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
