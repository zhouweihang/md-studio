import type { DeletedNote, NoteDocument, NoteEntry } from '../core/notes'
import type { SearchHit, SearchSummary, SnapshotStatus } from '../core/search'

export interface NoteRepository {
  listNotes(root: string): Promise<NoteEntry[]>
  readNote(root: string, path: string): Promise<NoteDocument>
  writeNote(root: string, path: string, content: string, expectedRevision: string): Promise<string>
  createNote(root: string, title: string): Promise<NoteEntry>
  deleteNote(root: string, path: string): Promise<DeletedNote>
  restoreNote(root: string, path: string, content: string): Promise<NoteEntry>
  renameNote(root: string, path: string, newTitle: string): Promise<NoteEntry>
  /**
   * Streams hits for `query` in batches (RFC-006 §4.5 / ADR-008) and resolves
   * with the summary carried by the single `Done` event.
   */
  searchNotes(
    root: string,
    query: string,
    requestId: number,
    onBatch: (hits: SearchHit[]) => void,
  ): Promise<SearchSummary>
  /** Asks the backend to stop a streamed search; late batches are dropped. */
  cancelSearch(requestId: number): Promise<void>
  snapshotStatus(): Promise<SnapshotStatus | null>
}

export interface WorkspacePicker {
  pick(): Promise<string | null>
}
