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

export interface TreeRow {
  node: TreeNode
  /** 0 for a top-level entry; directories add one per level. */
  depth: number
}

/**
 * Flattens the tree into the rows the sidebar actually paints. A directory is
 * always a row; its children only appear when it is expanded, which is how
 * collapsed folders keep 50k workspaces cheap.
 */
export function flattenTree(
  nodes: readonly TreeNode[],
  expanded: ReadonlySet<string>,
): TreeRow[] {
  const rows: TreeRow[] = []

  const walk = (level: readonly TreeNode[], depth: number) => {
    for (const node of level) {
      rows.push({ node, depth })
      if (node.type === 'dir' && expanded.has(node.path)) {
        walk(node.children, depth + 1)
      }
    }
  }

  walk(nodes, 0)
  return rows
}

/** Every directory path in the tree, used to reveal search matches. */
export function directoryPaths(nodes: readonly TreeNode[]): string[] {
  const paths: string[] = []

  const walk = (level: readonly TreeNode[]) => {
    for (const node of level) {
      if (node.type !== 'dir') continue
      paths.push(node.path)
      walk(node.children)
    }
  }

  walk(nodes)
  return paths
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
