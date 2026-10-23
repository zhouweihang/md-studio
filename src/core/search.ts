export interface SearchHit {
  /** Path relative to the workspace root. */
  path: string
  title: string
  /** One-based line number in the note. */
  line: number
  /** Context around the match. */
  preview: string
}

export interface HighlightPart {
  text: string
  match: boolean
}

/**
 * Splits a preview into plain and highlighted segments. Case-insensitive,
 * preserves the original casing, and never changes the joined text.
 */
export function highlightParts(text: string, query: string): HighlightPart[] {
  const needle = query.trim()
  if (!needle) return [{ text, match: false }]

  const haystack = text.toLocaleLowerCase()
  const needleLower = needle.toLocaleLowerCase()
  const parts: HighlightPart[] = []
  let cursor = 0

  while (cursor < text.length) {
    const found = haystack.indexOf(needleLower, cursor)
    if (found === -1) break

    if (found > cursor) parts.push({ text: text.slice(cursor, found), match: false })

    const end = found + needle.length
    parts.push({ text: text.slice(found, end), match: true })
    cursor = end
  }

  if (cursor < text.length) parts.push({ text: text.slice(cursor), match: false })
  return parts.length > 0 ? parts : [{ text, match: false }]
}
