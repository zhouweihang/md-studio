import { describe, expect, it } from 'vitest'
import { countLines, countWords, filterNotes, type NoteEntry } from './notes'

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
