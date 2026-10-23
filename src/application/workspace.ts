import type { NoteEntry } from '../core/notes'
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

  read(root: string, path: string): Promise<string> {
    return this.repository.readNote(root, path)
  }

  save(root: string, path: string, content: string): Promise<void> {
    return this.repository.writeNote(root, path, content)
  }

  create(root: string, title: string): Promise<NoteEntry> {
    return this.repository.createNote(root, title)
  }
}
