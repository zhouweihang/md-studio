import type { DeletedNote, NoteDocument, NoteEntry } from '../core/notes'
import type { SearchHit } from '../core/search'

export interface NoteRepository {
  listNotes(root: string): Promise<NoteEntry[]>
  readNote(root: string, path: string): Promise<NoteDocument>
  writeNote(root: string, path: string, content: string, expectedRevision: string): Promise<string>
  createNote(root: string, title: string): Promise<NoteEntry>
  deleteNote(root: string, path: string): Promise<DeletedNote>
  restoreNote(root: string, path: string, content: string): Promise<NoteEntry>
  renameNote(root: string, path: string, newTitle: string): Promise<NoteEntry>
  searchNotes(root: string, query: string): Promise<SearchHit[]>
}

export interface WorkspacePicker {
  pick(): Promise<string | null>
}
