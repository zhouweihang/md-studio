import { describe, expect, it } from 'vitest'
import type { NoteEntry } from './notes'
import { buildNoteTree, type TreeNode } from './tree'

function note(path: string): NoteEntry {
  const name = path.split('/').at(-1) ?? path
  return { path, name, title: name.replace(/\.[^.]+$/, ''), modifiedMs: null }
}

function flatten(nodes: TreeNode[]): string[] {
  return nodes.flatMap((node) =>
    node.type === 'dir' ? [`${node.path}/`, ...flatten(node.children)] : [node.path],
  )
}

describe('buildNoteTree', () => {
  it('puts directories before files and sorts both by name', () => {
    const tree = buildNoteTree([
      note('root.md'),
      note('z/zeta.md'),
      note('a/beta.md'),
      note('a/alpha.md'),
      note('b/other.md'),
    ])

    expect(flatten(tree)).toEqual([
      'a/',
      'a/alpha.md',
      'a/beta.md',
      'b/',
      'b/other.md',
      'z/',
      'z/zeta.md',
      'root.md',
    ])
  })

  it('does not create directories for empty paths', () => {
    expect(buildNoteTree([note('only.md')])).toEqual([
      {
        type: 'file',
        name: 'only.md',
        path: 'only.md',
        note: note('only.md'),
      },
    ])
  })

  it('preserves deep nesting and file metadata', () => {
    const source = note('one/two/three.md')
    const tree = buildNoteTree([source])
    const one = tree[0]

    expect(one.type).toBe('dir')
    if (one.type !== 'dir') throw new Error('expected directory')
    const two = one.children[0]
    expect(two.type).toBe('dir')
    if (two.type !== 'dir') throw new Error('expected directory')
    expect(two.children[0]).toMatchObject({ type: 'file', path: 'one/two/three.md', note: source })
  })
})
