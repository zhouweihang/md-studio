import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SearchHit } from '../../core/search'
import { TauriNoteRepository } from './noteRepository'

const mocks = vi.hoisted(() => {
  /** Minimal stand-in for `@tauri-apps/api/core`'s `Channel`. */
  class Channel {
    static instances: Channel[] = []
    onmessage: ((message: unknown) => void) | null = null

    constructor() {
      Channel.instances.push(this)
    }
  }

  return { invoke: vi.fn(), Channel, listen: vi.fn() }
})

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke, Channel: mocks.Channel }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))

const first: SearchHit = {
  path: 'a.md',
  title: 'a',
  line: 1,
  preview: 'alpha needle',
}
const second: SearchHit = {
  path: 'b.md',
  title: 'b',
  line: 4,
  preview: 'beta needle',
}

beforeEach(() => {
  mocks.invoke.mockReset()
  mocks.listen.mockReset()
  mocks.Channel.instances.length = 0
})

describe('TauriNoteRepository', () => {
  it('maps streamed batches to the callback and resolves with the Done summary', async () => {
    mocks.invoke.mockImplementation(async (command: string, payload: { onEvent: MockChannel }) => {
      expect(command).toBe('search_notes')
      payload.onEvent.onmessage?.({ kind: 'batch', hits: [first] })
      payload.onEvent.onmessage?.({ kind: 'batch', hits: [second] })
      payload.onEvent.onmessage?.({
        kind: 'done',
        total: 2,
        truncated: false,
        tookMs: 12,
        source: 'snapshot',
      })
    })

    const batches: SearchHit[][] = []
    const repository = new TauriNoteRepository()
    const summary = await repository.searchNotes('C:\\notes', 'needle', 41, (hits) =>
      batches.push(hits),
    )

    expect(batches).toEqual([[first], [second]])
    expect(summary).toEqual({ total: 2, truncated: false, tookMs: 12, source: 'snapshot' })
    expect(mocks.invoke).toHaveBeenCalledWith('search_notes', {
      root: 'C:\\notes',
      query: 'needle',
      requestId: 41,
      onEvent: mocks.Channel.instances[0],
    })
  })

  it('resolves an empty summary when the command returns without Done', async () => {
    mocks.invoke.mockImplementation(async () => undefined)

    const repository = new TauriNoteRepository()
    const summary = await repository.searchNotes('C:\\notes', 'needle', 1, () => {})

    expect(summary).toEqual({ total: 0, truncated: false, tookMs: 0, source: 'disk' })
  })

  it('sends the request id when cancelling', async () => {
    mocks.invoke.mockImplementation(async () => undefined)

    const repository = new TauriNoteRepository()
    await repository.cancelSearch(41)

    expect(mocks.invoke).toHaveBeenCalledWith('search_cancel', { requestId: 41 })
  })

  it('subscribes to workspace deltas and hands back the unlisten function', async () => {
    const handlers: Array<(event: { payload: unknown }) => void> = []
    const stop = vi.fn()
    mocks.listen.mockImplementation(
      async (event: string, next: (event: { payload: unknown }) => void) => {
        expect(event).toBe('workspace:changed')
        handlers.push(next)
        return stop
      },
    )

    const seen: unknown[] = []
    const repository = new TauriNoteRepository()
    const unlisten = await repository.onWorkspaceChanged((delta) => seen.push(delta))

    const delta = {
      added: [{ path: 'new.md', name: 'new.md', title: 'new', modifiedMs: 7 }],
      removed: ['old.md'],
    }
    handlers[0]({ payload: delta })

    expect(seen).toEqual([delta])
    expect(unlisten).toBe(stop)
  })

  it('passes the snapshot status through, including the empty case', async () => {
    const status = {
      state: 'warming',
      total: 10,
      cached: 4,
      bytes: 1024,
      budgetBytes: 2048,
      degradedReason: null,
    }
    mocks.invoke.mockImplementation(async () => status)

    const repository = new TauriNoteRepository()
    expect(await repository.snapshotStatus()).toEqual(status)

    mocks.invoke.mockImplementation(async () => null)
    expect(await repository.snapshotStatus()).toBeNull()
    expect(mocks.invoke).toHaveBeenCalledWith('snapshot_status')
  })
})

/** The channel shape the adapter drives; `MockChannel` keeps the test readable. */
interface MockChannel {
  onmessage: ((message: unknown) => void) | null
}
