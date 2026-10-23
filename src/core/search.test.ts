import { describe, expect, it } from 'vitest'
import { highlightParts } from './search'

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
