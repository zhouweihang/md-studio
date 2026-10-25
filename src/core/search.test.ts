import { describe, expect, it } from 'vitest'
import { highlightParts, mergeHits, searchDebounceMs, type SearchHit } from './search'

const matches = (text: string, query: string) =>
  highlightParts(text, query)
    .filter((part) => part.match)
    .map((part) => part.text)

describe('highlightParts', () => {
  it('highlights every occurrence case-insensitively but keeps original casing', () => {
    const text = 'Autosave and AUTOSAVE'
    const parts = highlightParts(text, 'autosave')

    expect(matches(text, 'autosave')).toEqual(['Autosave', 'AUTOSAVE'])
    expect(parts.map((part) => part.text).join('')).toBe(text)
  })

  it('highlights Chinese needles and leaves the surrounding text intact', () => {
    const text = '正文里的关键字，以及第二个关键字。'
    const parts = highlightParts(text, '关键字')

    expect(matches(text, '关键字')).toEqual(['关键字', '关键字'])
    expect(parts.map((part) => part.text).join('')).toBe(text)
  })

  it('returns the whole text unhighlighted when the query is blank', () => {
    expect(highlightParts('anything', '   ')).toEqual([{ text: 'anything', match: false }])
  })

  it('returns the whole text unhighlighted when there is no match', () => {
    expect(highlightParts('anything', 'missing')).toEqual([
      { text: 'anything', match: false },
    ])
  })

  it('trims surrounding whitespace from the query', () => {
    expect(matches('needle in a haystack', '  needle  ')).toEqual(['needle'])
  })
})

describe('searchDebounceMs', () => {
  it('uses the three RFC-006 tiers', () => {
    expect(searchDebounceMs(10, 'ready')).toBe(80)
    expect(searchDebounceMs(2_001, 'ready')).toBe(120)
    expect(searchDebounceMs(10, 'warming')).toBe(200)
    expect(searchDebounceMs(10, 'degraded')).toBe(200)
    expect(searchDebounceMs(10, null)).toBe(200)
  })
})

describe('mergeHits', () => {
  const hit = (path: string, line: number): SearchHit => ({
    path,
    title: path,
    line,
    preview: 'needle',
  })

  it('keeps batches in path order', () => {
    const merged = mergeHits([hit('b.md', 1)], [hit('a.md', 3)])
    expect(merged.map((item) => `${item.path}:${item.line}`)).toEqual(['a.md:3', 'b.md:1'])
  })

  it('sorts lines inside one note and drops duplicate deliveries', () => {
    const merged = mergeHits([hit('a.md', 5)], [hit('a.md', 2), hit('a.md', 5)])
    expect(merged.map((item) => item.line)).toEqual([2, 5])
  })

  it('returns the same array when the batch is empty', () => {
    const current = [hit('a.md', 1)]
    expect(mergeHits(current, [])).toBe(current)
  })
})
