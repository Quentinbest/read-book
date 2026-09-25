// Fuzzy matching for ⌘K (Screen 16): the query's characters must appear in order.
// Word starts and runs score higher, so “gtc” finds “Go to chapter…” before
// anything that merely contains g, t and c.

export interface FuzzyMatch {
  score: number
  /** Indices of the matched characters, for highlighting. */
  indices: number[]
}

const fold = (s: string) =>
  s
    .normalize('NFD')
    .replace(/\p{M}+/gu, '')
    .toLowerCase()

export function fuzzy(query: string, text: string): FuzzyMatch | null {
  const q = fold(query.trim())
  if (!q) return { score: 0, indices: [] }
  const t = fold(text)
  // A plain substring always wins, earlier and at a word start better.
  const at = t.indexOf(q)
  if (at >= 0) {
    const wordStart = at === 0 || /[\s\-–—:·(]/.test(t[at - 1])
    return {
      score: 1000 - at + (wordStart ? 200 : 0) - (t.length - q.length) * 0.1,
      indices: Array.from({ length: q.length }, (_, i) => at + i),
    }
  }
  const indices: number[] = []
  let score = 0
  let from = 0
  let prev = -2
  for (const ch of q) {
    if (ch === ' ') continue
    const i = t.indexOf(ch, from)
    if (i < 0) return null
    const wordStart = i === 0 || /[\s\-–—:·(]/.test(t[i - 1])
    score += wordStart ? 30 : 0
    score += i === prev + 1 ? 15 : 0
    score -= Math.min(10, i - from)
    indices.push(i)
    prev = i
    from = i + 1
  }
  return { score, indices }
}

/** Items that match, best first; ties keep their original order. */
export function rank<T>(query: string, items: T[], text: (item: T) => string): T[] {
  return items
    .map((item, i) => ({ item, i, m: fuzzy(query, text(item)) }))
    .filter((x): x is { item: T; i: number; m: FuzzyMatch } => x.m !== null)
    .sort((a, b) => b.m.score - a.m.score || a.i - b.i)
    .map((x) => x.item)
}
