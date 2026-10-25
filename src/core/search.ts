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

/** Where the bodies of a streamed search came from (RFC-006 §5). */
export type SearchSource = 'snapshot' | 'disk' | 'mixed'

/** Snapshot lifecycle as reported by the Rust warm-up (RFC-006 §4.4). */
export type SnapshotState = 'warming' | 'ready' | 'degraded'

export interface SnapshotStatus {
  state: SnapshotState
  total: number
  cached: number
  bytes: number
  budgetBytes: number
  degradedReason: string | null
}

/** Summary carried by the single `Done` event of a streamed search. */
export interface SearchSummary {
  total: number
  truncated: boolean
  tookMs: number
  source: SearchSource
}

/**
 * Adaptive debounce (RFC-006 §4.8): a warm snapshot can afford a shorter wait
 * than a warming one, where the query would still scan from disk.
 */
export function searchDebounceMs(
  noteCount: number,
  snapshotState: SnapshotState | null,
): number {
  if (snapshotState !== 'ready') return 200
  return noteCount > 2_000 ? 120 : 80
}

function hitKey(hit: SearchHit): string {
  return `${hit.path}:${hit.line}`
}

/**
 * Merges one streamed batch into the hits already on screen. Results arrive in
 * path order already, but batches are sorted and de-duplicated here so a late
 * or repeated delivery can never reorder or duplicate the list.
 */
export function mergeHits(current: SearchHit[], batch: SearchHit[]): SearchHit[] {
  if (batch.length === 0) return current

  const seen = new Set(current.map(hitKey))
  const merged = [...current]
  for (const hit of batch) {
    const key = hitKey(hit)
    if (seen.has(key)) continue
    seen.add(key)
    merged.push(hit)
  }

  merged.sort((a, b) =>
    a.path === b.path ? a.line - b.line : a.path < b.path ? -1 : 1,
  )
  return merged
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
