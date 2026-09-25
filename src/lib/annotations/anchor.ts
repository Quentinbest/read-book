// Anchoring (A9, B3): every highlight keeps a text quote (exact text with some
// context) besides its CFI. When a book file is replaced, highlights are placed
// again from the quote: exactly, or after normalising, or approximately, within
// a strict error bound. Anything that cannot be placed that way is reported
// unplaced (A8: “Couldn't place”); it is never put at a guessed position.

/** Characters of context kept on each side of the quote. */
export const CONTEXT = 32
/** Approximate matches may differ from the quote by at most this share of its length. */
export const MAX_ERROR_RATE = 0.15

export interface TextQuote {
  exact: string
  prefix: string
  suffix: string
}

export interface Placement {
  start: number
  end: number
  /** How it was found: the quote as is, after normalising, or approximately. */
  how: 'exact' | 'normalized' | 'approximate'
}

/** The quote for [start, end) in a chapter's extracted text. */
export function quoteAt(text: string, start: number, end: number): TextQuote {
  return {
    exact: text.slice(start, end),
    prefix: text.slice(Math.max(0, start - CONTEXT), start),
    suffix: text.slice(end, end + CONTEXT),
  }
}

/** Characters shared at the end of `a` and `b`. */
function commonSuffix(a: string, b: string): number {
  let n = 0
  while (n < a.length && n < b.length && a[a.length - 1 - n] === b[b.length - 1 - n]) n++
  return n
}

function commonPrefix(a: string, b: string): number {
  let n = 0
  while (n < a.length && n < b.length && a[n] === b[n]) n++
  return n
}

/** How well the text around [start, end) agrees with the quote's context. */
function contextScore(text: string, start: number, end: number, q: TextQuote): number {
  const before = text.slice(Math.max(0, start - CONTEXT), start)
  const after = text.slice(end, end + CONTEXT)
  return commonSuffix(before, q.prefix) + commonPrefix(after, q.suffix)
}

function allIndexes(hay: string, needle: string): number[] {
  const out: number[] = []
  if (!needle) return out
  for (let at = hay.indexOf(needle); at >= 0; at = hay.indexOf(needle, at + 1)) out.push(at)
  return out
}

/** Pick among candidates by context, then by closeness to a hint; ties with no signal fail. */
function best(
  text: string,
  q: TextQuote,
  candidates: { start: number; end: number; cost: number }[],
  hint?: number,
): { start: number; end: number } | null {
  if (!candidates.length) return null
  if (candidates.length === 1) return candidates[0]
  const scored = candidates.map((c) => ({
    ...c,
    score: contextScore(text, c.start, c.end, q) - c.cost,
    distance: hint === undefined ? 0 : Math.abs(c.start - hint),
  }))
  scored.sort((a, b) => b.score - a.score || a.distance - b.distance)
  const [first, second] = scored
  // Two places equally good with nothing to tell them apart: do not guess.
  if (hint === undefined && first.score === second.score) return null
  return first
}

/** Normalise for comparison: case, whitespace, quote marks, dashes and ellipses. */
export function normalizeForMatch(s: string): { text: string; map: number[] } {
  let text = ''
  const map: number[] = []
  let lastSpace = true
  for (let i = 0; i < s.length; i++) {
    let ch = s[i]
    if (/\s/.test(ch)) {
      if (lastSpace) continue
      ch = ' '
      lastSpace = true
    } else lastSpace = false
    ch = ch
      .replace(/[‘’‚‛′]/, "'")
      .replace(/[“”„‟″]/, '"')
      .replace(/[‐‑‒–—―−]/, '-')
      .replace(/…/, '.')
      .toLowerCase()
    text += ch
    map.push(i)
  }
  map.push(s.length)
  return { text, map }
}

/**
 * Approximate substring match (Sellers): the end positions in `text` where some
 * substring is within `k` edits of `pattern`, with the cost.
 */
function approximateEnds(
  text: string,
  pattern: string,
  k: number,
): { end: number; cost: number }[] {
  const m = pattern.length
  let prev = new Array<number>(m + 1)
  for (let i = 0; i <= m; i++) prev[i] = i
  const ends: { end: number; cost: number }[] = []
  for (let j = 1; j <= text.length; j++) {
    const cur = new Array<number>(m + 1)
    cur[0] = 0
    const c = text[j - 1]
    for (let i = 1; i <= m; i++)
      cur[i] = Math.min(prev[i - 1] + (pattern[i - 1] === c ? 0 : 1), prev[i] + 1, cur[i - 1] + 1)
    if (cur[m] <= k) ends.push({ end: j, cost: cur[m] })
    prev = cur
  }
  return ends
}

/** The start of the best alignment of `pattern` ending at `end` (reverse pass). */
function startFor(text: string, pattern: string, end: number, k: number): number {
  const window = text.slice(Math.max(0, end - pattern.length - k), end)
  const rev = [...window].reverse().join('')
  const pat = [...pattern].reverse().join('')
  const hits = approximateEnds(rev, pat, k)
  if (!hits.length) return end - pattern.length
  const bestHit = hits.reduce((a, b) =>
    b.cost < a.cost || (b.cost === a.cost && b.end < a.end) ? b : a,
  )
  return end - bestHit.end
}

/**
 * Place a quote in a chapter's text. `hint` is where the old CFI pointed, if it
 * still resolves; it only breaks ties.
 */
export function locate(text: string, q: TextQuote, hint?: number): Placement | null {
  if (!q.exact.trim()) return null
  // 1. The exact text.
  const exact = allIndexes(text, q.exact).map((start) => ({
    start,
    end: start + q.exact.length,
    cost: 0,
  }))
  const e = best(text, q, exact, hint)
  if (e) return { ...e, how: 'exact' }
  if (exact.length > 1) return null
  // 2. After normalising whitespace, case, quote marks and dashes.
  const nt = normalizeForMatch(text)
  const nq = normalizeForMatch(q.exact).text
  const norm = allIndexes(nt.text, nq).map((s) => ({
    start: nt.map[s],
    end: nt.map[s + nq.length - 1] + 1,
    cost: 0,
  }))
  const n = best(text, q, norm, hint)
  if (n) return { ...n, how: 'normalized' }
  if (norm.length > 1) return null
  // 3. Approximately, near seeds taken from the quote, within MAX_ERROR_RATE.
  const k = Math.floor(nq.length * MAX_ERROR_RATE)
  if (k < 1) return null
  const seedLength = Math.max(4, Math.min(16, Math.floor(nq.length / 4)))
  const seeds = [0, Math.floor((nq.length - seedLength) / 2), nq.length - seedLength].map((at) => ({
    at,
    seed: nq.slice(at, at + seedLength),
  }))
  const windows = new Map<number, number>()
  for (const { at, seed } of seeds)
    for (const pos of allIndexes(nt.text, seed)) {
      const from = Math.max(0, pos - at - k)
      windows.set(from, Math.min(nt.text.length, from + nq.length + 2 * k))
    }
  const found: { start: number; end: number; cost: number }[] = []
  for (const [from, to] of windows) {
    const slice = nt.text.slice(from, to)
    const ends = approximateEnds(slice, nq, k)
    if (!ends.length) continue
    const top = ends.reduce((a, b) => (b.cost < a.cost ? b : a))
    const end = from + top.end
    const start = from + startFor(slice, nq, top.end, k)
    if (found.some((f) => Math.abs(f.start - start) < nq.length / 2)) continue
    found.push({ start: nt.map[start], end: nt.map[end - 1] + 1, cost: top.cost })
  }
  if (!found.length) return null
  const lowest = Math.min(...found.map((f) => f.cost))
  const a = best(
    text,
    q,
    found.filter((f) => f.cost === lowest),
    hint,
  )
  return a ? { ...a, how: 'approximate' } : null
}
