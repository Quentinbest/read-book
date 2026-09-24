// Text normalisation for in-book search (plan F2, F8).
//
// Search runs over normalised text and reports matches as offsets into the
// original chapter text, so every normalised character keeps a pointer back to
// the original character it came from.

export interface NormalizedText {
  /** Normalised text. */
  text: string
  /** map[i] = offset in the original text of normalised character i. map[text.length] = original length. */
  map: Uint32Array
}

const COMBINING = /\p{M}/u
const CJK =
  /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}\u3099-\u309c\uff9e\uff9f]/u
const COMBINING_ALL = /\p{M}/gu
const WHITESPACE = /\s/u
const SOFT_HYPHEN = '­'
const SINGLE_QUOTES = /[‘’‚‛′]/g
const DOUBLE_QUOTES = /[“”„‟″]/g

export type Folding = 'folded' | 'literal'

/**
 * Normalise text for matching.
 *
 * Both modes collapse whitespace runs to one space, drop soft hyphens and unify
 * typographic quotes with the ASCII ones a reader types. `folded` also ignores
 * case and diacritics (NFKD, combining marks removed, lower-cased), so “Café”
 * matches “cafe” and fullwidth forms match ASCII. `literal` keeps case and
 * diacritics, for quoted exact-phrase queries.
 */
export function normalize(original: string, folding: Folding = 'folded'): NormalizedText {
  const out: string[] = []
  const map: number[] = []
  const emit = (piece: string, at: number) => {
    for (let k = 0; k < piece.length; k++) {
      out.push(piece[k])
      map.push(at)
    }
  }
  let lastWasSpace = true // also trims leading whitespace
  const n = original.length
  let i = 0
  while (i < n) {
    const at = i
    const code = original.charCodeAt(i)
    // A cluster is one base character plus the combining marks after it, so
    // decomposed input (e + ◌́, は + ◌゙) normalises like precomposed input.
    let end = i + (code >= 0xd800 && code <= 0xdbff ? 2 : 1)
    while (end < n && original.charCodeAt(end) >= 0x300) {
      const next = String.fromCodePoint(original.codePointAt(end)!)
      if (!COMBINING.test(next)) break
      end += next.length
    }
    i = end
    if (code < 0x80 && end === at + 1) {
      // ASCII fast path: most book text never needs Unicode normalisation.
      if (code === 0x20 || (code >= 0x09 && code <= 0x0d)) {
        if (!lastWasSpace) emit(' ', at)
        lastWasSpace = true
      } else {
        emit(
          folding === 'folded' && code >= 0x41 && code <= 0x5a
            ? String.fromCharCode(code + 32)
            : original[at],
          at,
        )
        lastWasSpace = false
      }
      continue
    }
    const cluster = original.slice(at, end)
    if (cluster === SOFT_HYPHEN) continue
    if (WHITESPACE.test(cluster)) {
      if (!lastWasSpace) emit(' ', at)
      lastWasSpace = true
      continue
    }
    lastWasSpace = false
    const piece = cluster.replace(SINGLE_QUOTES, "'").replace(DOUBLE_QUOTES, '"')
    if (CJK.test(piece)) {
      // Voicing marks (ば = は + ◌゙) change the letter, so CJK text keeps its
      // marks; NFKC still unifies half- and full-width forms.
      emit(piece.normalize('NFKC'), at)
    } else if (folding === 'folded') {
      emit(piece.normalize('NFKD').replace(COMBINING_ALL, '').toLowerCase(), at)
    } else {
      emit(piece.normalize('NFC'), at)
    }
  }
  map.push(original.length)
  return { text: out.join(''), map: Uint32Array.from(map) }
}

/** True when the text contains Chinese, Japanese or Korean characters. */
export function hasCJK(text: string): boolean {
  return CJK.test(text)
}
