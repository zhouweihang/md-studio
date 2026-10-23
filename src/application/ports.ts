import type { NoteEntry } from '../core/notes'

export interface NoteRepository {
  listNotes(root: string): Promise<NoteEntry[]>
  readNote(root: string, path: string): Promise<string>
  writeNote(root: string, path: string, content: string): Promise<void>
  createNote(root: string, title: string): Promise<NoteEntry>
}

export interface WorkspacePicker {
  pick(): Promise<string | null>
}
