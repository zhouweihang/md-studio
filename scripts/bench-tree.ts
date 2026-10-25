/**
 * Builds the sidebar tree for a synthetic workspace and reports the cost
 * (RFC-006 §4.7). Run with `bun scripts/bench-tree.ts 50000`.
 */
import type { NoteEntry } from '../src/core/notes'
import { buildNoteTree, flattenTree } from '../src/core/tree'

const count = Number(process.argv[2] ?? 50_000)
const notes: NoteEntry[] = Array.from({ length: count }, (_, index) => {
  const name = `note-${String(index).padStart(5, '0')}.md`
  return {
    path: `folder-${index % 200}/${name}`,
    name,
    title: `note ${index}`,
    modifiedMs: null,
  }
})

const buildStarted = performance.now()
const tree = buildNoteTree(notes)
const buildMs = performance.now() - buildStarted

const flattenStarted = performance.now()
const collapsed = flattenTree(tree, new Set())
const flattenMs = performance.now() - flattenStarted

const expanded = new Set(collapsed.map((row) => row.node.path).filter((path) => !path.endsWith('.md')))
const expandedStarted = performance.now()
const allRows = flattenTree(tree, expanded)
const expandedMs = performance.now() - expandedStarted

console.log(
  JSON.stringify(
    {
      notes: count,
      buildMs: Number(buildMs.toFixed(2)),
      collapsedRows: collapsed.length,
      flattenMs: Number(flattenMs.toFixed(2)),
      expandedRows: allRows.length,
      expandedFlattenMs: Number(expandedMs.toFixed(2)),
    },
    null,
    2,
  ),
)
