import { invoke } from '@tauri-apps/api/core'
import type { NoteRepository } from '../../application/ports'
import type { NoteEntry } from '../../core/notes'

/**
 * The only place in the frontend that knows about Tauri command names.
 * Rust validates every path again, so this adapter is not a security boundary.
 */
export class TauriNoteRepository implements NoteRepository {
  listNotes(root: string): Promise<NoteEntry[]> {
    return invoke<NoteEntry[]>('list_notes', { root })
  }

  readNote(root: string, path: string): Promise<string> {
    return invoke<string>('read_note', { root, path })
  }

  writeNote(root: string, path: string, content: string): Promise<void> {
    return invoke<void>('write_note', { root, path, content })
  }

  createNote(root: string, title: string): Promise<NoteEntry> {
    return invoke<NoteEntry>('create_note', { root, title })
  }
}
