import { Channel, invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { NoteRepository } from '../../application/ports'
import type { DeletedNote, NoteDocument, NoteEntry, WorkspaceDelta } from '../../core/notes'
import type {
  SearchHit,
  SearchSource,
  SearchSummary,
  SnapshotStatus,
} from '../../core/search'

/** Mirrors `search::SearchEvent` in Rust (ADR-008). */
type SearchEvent =
  | { kind: 'batch'; hits: SearchHit[] }
  | {
      kind: 'done'
      total: number
      truncated: boolean
      tookMs: number
      source: SearchSource
    }

/** Mirrors `watcher::WORKSPACE_EVENT` in Rust (ADR-009). */
const WORKSPACE_EVENT = 'workspace:changed'

const EMPTY_SUMMARY: SearchSummary = {
  total: 0,
  truncated: false,
  tookMs: 0,
  source: 'disk',
}

/**
 * The only place in the frontend that knows about Tauri command names.
 * Rust validates every path again, so this adapter is not a security boundary.
 */
export class TauriNoteRepository implements NoteRepository {
  listNotes(root: string): Promise<NoteEntry[]> {
    return invoke<NoteEntry[]>('list_notes', { root })
  }

  readNote(root: string, path: string): Promise<NoteDocument> {
    return invoke<NoteDocument>('read_note', { root, path })
  }

  writeNote(
    root: string,
    path: string,
    content: string,
    expectedRevision: string,
  ): Promise<string> {
    return invoke<string>('write_note', { root, path, content, expectedRevision })
  }

  createNote(root: string, title: string): Promise<NoteEntry> {
    return invoke<NoteEntry>('create_note', { root, title })
  }

  deleteNote(root: string, path: string): Promise<DeletedNote> {
    return invoke<DeletedNote>('delete_note', { root, path })
  }

  restoreNote(root: string, path: string, content: string): Promise<NoteEntry> {
    return invoke<NoteEntry>('restore_note', { root, path, content })
  }

  renameNote(root: string, path: string, newTitle: string): Promise<NoteEntry> {
    return invoke<NoteEntry>('rename_note', { root, path, newTitle })
  }

  searchNotes(
    root: string,
    query: string,
    requestId: number,
    onBatch: (hits: SearchHit[]) => void,
  ): Promise<SearchSummary> {
    const channel = new Channel<SearchEvent>()
    let summary = EMPTY_SUMMARY

    channel.onmessage = (event) => {
      if (event.kind === 'batch') {
        onBatch(event.hits)
        return
      }
      summary = {
        total: event.total,
        truncated: event.truncated,
        tookMs: event.tookMs,
        source: event.source,
      }
    }

    // The command returns after `Done` has been sent, so every event has been
    // delivered to `onmessage` by the time this resolves.
    return invoke<void>('search_notes', { root, query, requestId, onEvent: channel }).then(
      () => summary,
    )
  }

  cancelSearch(requestId: number): Promise<void> {
    return invoke<void>('search_cancel', { requestId })
  }

  snapshotStatus(): Promise<SnapshotStatus | null> {
    return invoke<SnapshotStatus | null>('snapshot_status')
  }

  onWorkspaceChanged(handler: (delta: WorkspaceDelta) => void): Promise<() => void> {
    return listen<WorkspaceDelta>(WORKSPACE_EVENT, (event) => handler(event.payload))
  }
}
