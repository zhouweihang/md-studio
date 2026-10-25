import type { DeletedNote, NoteDocument, NoteEntry } from '../core/notes'
import type { SearchHit, SearchSummary, SnapshotStatus } from '../core/search'
import type { NoteRepository } from './ports'

/**
 * Application service: coordinates note use cases and depends only on ports.
 * UI and platform details stay outside this class.
 */
export class WorkspaceService {
  constructor(private readonly repository: NoteRepository) {}

  list(root: string): Promise<NoteEntry[]> {
    return this.repository.listNotes(root)
  }

  read(root: string, path: string): Promise<NoteDocument> {
    return this.repository.readNote(root, path)
  }

  save(root: string, path: string, content: string, expectedRevision: string): Promise<string> {
    return this.repository.writeNote(root, path, content, expectedRevision)
  }

  create(root: string, title: string): Promise<NoteEntry> {
    return this.repository.createNote(root, title)
  }

  delete(root: string, path: string): Promise<DeletedNote> {
    return this.repository.deleteNote(root, path)
  }

  restore(root: string, path: string, content: string): Promise<NoteEntry> {
    return this.repository.restoreNote(root, path, content)
  }

  rename(root: string, path: string, newTitle: string): Promise<NoteEntry> {
    return this.repository.renameNote(root, path, newTitle)
  }

  search(
    root: string,
    query: string,
    requestId: number,
    onBatch: (hits: SearchHit[]) => void,
  ): Promise<SearchSummary> {
    return this.repository.searchNotes(root, query, requestId, onBatch)
  }

  cancelSearch(requestId: number): Promise<void> {
    return this.repository.cancelSearch(requestId)
  }

  snapshotStatus(): Promise<SnapshotStatus | null> {
    return this.repository.snapshotStatus()
  }
}
