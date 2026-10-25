export interface NoteEntry {
  /** Path relative to the workspace root, using forward slashes. */
  path: string
  name: string
  title: string
  modifiedMs: number | null
}

export interface NoteDocument {
  content: string
  /** Opaque change-detection token produced by the Rust read command. */
  revision: string
}

/** Incremental note-list change pushed by the Rust watcher (ADR-009). */
export interface WorkspaceDelta {
  /** Notes that appeared (created or renamed into the workspace). */
  added: NoteEntry[]
  /** Relative paths that no longer exist. */
  removed: string[]
}

/**
 * Applies a watcher delta to the note list. A path that is both removed and
 * added in one batch ends up present: the watcher emits both sides of an atomic
 * replace, and the file exists afterwards.
 */
export function applyWorkspaceDelta(
  notes: readonly NoteEntry[],
  delta: WorkspaceDelta,
): NoteEntry[] {
  if (delta.added.length === 0 && delta.removed.length === 0) return [...notes]

  const removed = new Set(delta.removed)
  const pending = new Map(delta.added.map((note) => [note.path, note]))

  const next: NoteEntry[] = []
  for (const note of notes) {
    if (removed.has(note.path)) continue

    const replacement = pending.get(note.path)
    if (replacement) {
      pending.delete(note.path)
      next.push(replacement)
      continue
    }
    next.push(note)
  }

  next.push(...pending.values())
  return next
}

export interface DeletedNote {
  /** Relative path the note can be restored to. */
  path: string
  /** Text snapshot kept in memory for undo; null when too large or not UTF-8. */
  content: string | null
}

export function filterNotes(notes: readonly NoteEntry[], query: string): NoteEntry[] {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return [...notes]

  return notes.filter((note) => {
    const path = note.path.toLocaleLowerCase()
    const title = note.title.toLocaleLowerCase()
    return path.includes(needle) || title.includes(needle)
  })
}

/**
 * Counts CJK characters individually and latin/digit sequences as words.
 * This matches how Chinese writing tools usually report length.
 */
export function countWords(text: string): number {
  const cjk = text.match(/[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]/g)?.length ?? 0
  const latin = text.match(/[A-Za-z0-9]+(?:['’-][A-Za-z0-9]+)*/g)?.length ?? 0
  return cjk + latin
}

export function countLines(text: string): number {
  if (text.length === 0) return 0
  return text.split(/\r\n|\r|\n/).length
}
