import { invoke } from '@tauri-apps/api/core'
import type { NoteRepository } from '../../application/ports'
import type { DeletedNote, NoteDocument, NoteEntry } from '../../core/notes'
import type { SearchHit } from '../../core/search'

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

  searchNotes(root: string, query: string): Promise<SearchHit[]> {
    return invoke<SearchHit[]>('search_notes', { root, query })
  }
}
