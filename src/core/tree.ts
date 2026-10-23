import type { NoteEntry } from './notes'

export type TreeNode =
  | { type: 'dir'; name: string; path: string; children: TreeNode[] }
  | { type: 'file'; name: string; path: string; note: NoteEntry }

interface DirBuilder {
  dirs: Map<string, DirBuilder>
  files: NoteEntry[]
}

function compareNames(left: string, right: string): number {
  return left.localeCompare(right, 'zh-CN', { numeric: true, sensitivity: 'base' })
}

export function buildNoteTree(notes: readonly NoteEntry[]): TreeNode[] {
  const root: DirBuilder = { dirs: new Map(), files: [] }

  for (const note of notes) {
    const segments = note.path.split('/').filter(Boolean)
    if (segments.length === 0) continue

    let current = root
    for (const segment of segments.slice(0, -1)) {
      let child = current.dirs.get(segment)
      if (!child) {
        child = { dirs: new Map(), files: [] }
        current.dirs.set(segment, child)
      }
      current = child
    }
    current.files.push(note)
  }

  const convert = (builder: DirBuilder, prefix: string): TreeNode[] => {
    const directories: TreeNode[] = [...builder.dirs.entries()]
      .sort(([left], [right]) => compareNames(left, right))
      .map(([name, child]) => {
        const path = prefix ? `${prefix}/${name}` : name
        return { type: 'dir', name, path, children: convert(child, path) }
      })

    const files: TreeNode[] = [...builder.files]
      .sort((left, right) => compareNames(left.name, right.name))
      .map((note) => ({ type: 'file', name: note.name, path: note.path, note }))

    return [...directories, ...files]
  }

  return convert(root, '')
}
