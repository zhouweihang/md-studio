import { describe, expect, it } from 'vitest'
import {
  applyWorkspaceDelta,
  countLines,
  countWords,
  filterNotes,
  type NoteEntry,
} from './notes'

const notes: NoteEntry[] = [
  { path: 'daily/2026-10-23.md', name: '2026-10-23.md', title: '2026-10-23', modifiedMs: null },
  { path: 'ideas/架构.md', name: '架构.md', title: '架构', modifiedMs: 1 },
  { path: 'README.md', name: 'README.md', title: 'README', modifiedMs: 2 },
]

describe('filterNotes', () => {
  it('returns all notes for an empty query', () => {
    expect(filterNotes(notes, '   ')).toHaveLength(3)
  })

  it('matches path and title case-insensitively', () => {
    expect(filterNotes(notes, 'IDEAS').map((note) => note.path)).toEqual(['ideas/架构.md'])
    expect(filterNotes(notes, 'readme').map((note) => note.path)).toEqual(['README.md'])
  })

  it('matches Chinese titles', () => {
    expect(filterNotes(notes, '架构').map((note) => note.path)).toEqual(['ideas/架构.md'])
  })

  it('returns an empty list when nothing matches', () => {
    expect(filterNotes(notes, 'missing')).toEqual([])
  })
})

describe('applyWorkspaceDelta', () => {
  const added = (path: string): NoteEntry => ({
    path,
    name: path.split('/').at(-1) ?? path,
    title: 'new',
    modifiedMs: 3,
  })

  it('inserts created notes and drops removed ones', () => {
    const next = applyWorkspaceDelta(notes, {
      added: [added('drafts/idea.md')],
      removed: ['README.md'],
    })

    expect(next.map((note) => note.path)).toEqual([
      'daily/2026-10-23.md',
      'ideas/架构.md',
      'drafts/idea.md',
    ])
  })

  it('replaces an existing entry instead of duplicating it', () => {
    const refreshed = { ...added('README.md'), title: 'renamed' }
    const next = applyWorkspaceDelta(notes, { added: [refreshed], removed: [] })

    expect(next.filter((note) => note.path === 'README.md')).toHaveLength(1)
    expect(next.find((note) => note.path === 'README.md')?.title).toBe('renamed')
  })

  it('keeps a path that is both removed and added, as an atomic replace does', () => {
    const next = applyWorkspaceDelta(notes, {
      added: [added('README.md')],
      removed: ['README.md'],
    })

    expect(next.some((note) => note.path === 'README.md')).toBe(true)
  })

  it('ignores removals of paths it never had', () => {
    expect(applyWorkspaceDelta(notes, { added: [], removed: ['ghost.md'] })).toEqual(notes)
  })
})

describe('countWords', () => {
  it('counts CJK characters individually and latin words once', () => {
    expect(countWords('Hello 世界 2026')).toBe(4)
  })

  it('handles punctuation and empty text', () => {
    expect(countWords('你好，world!')).toBe(3)
    expect(countWords('')).toBe(0)
  })
})

describe('countLines', () => {
  it('counts logical lines across newline styles', () => {
    expect(countLines('a\nb\r\nc')).toBe(3)
    expect(countLines('')).toBe(0)
    expect(countLines('single')).toBe(1)
  })
})
