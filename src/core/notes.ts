export interface NoteEntry {
  /** Path relative to the workspace root, using forward slashes. */
  path: string
  name: string
  title: string
  modifiedMs: number | null
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
