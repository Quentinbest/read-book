import { describe, expect, it } from 'vitest'
import { normalize } from './normalize'
import {
  indexChapter,
  parseQuery,
  searchBook,
  searchChapter,
  searchOrder,
  snippet,
  SNIPPET_LENGTH,
} from './search'

const find = (text: string, raw: string) => {
  const query = parseQuery(raw)
  if (!query) throw new Error(`query rejected: ${raw}`)
  return searchChapter(indexChapter({ index: 0, text }), query).matches.map((m) =>
    text.slice(m.start, m.end),
  )
}

describe('normalize', () => {
  it('maps every normalised character back to the original text', () => {
    const original = '  Café  Noël\n\tﬁne “quote” é'
    const { text, map } = normalize(original)
    expect(text).toBe('cafe noel fine "quote" e')
    for (let i = 0; i < text.length; i++) expect(map[i]).toBeLessThan(original.length)
    expect(original[map[text.indexOf('noel')]]).toBe('N')
    expect(map[text.length]).toBe(original.length)
  })

  it('drops soft hyphens and folds typographic apostrophes', () => {
    expect(normalize('whale­bone Ahab’s').text).toBe("whalebone ahab's")
  })

  it('literal mode keeps case and diacritics', () => {
    expect(normalize('Café  Noël').text).toBe('cafe noel')
    expect(normalize('Café  Noël', 'literal').text).toBe('Café Noël')
  })
})

describe('parseQuery (F2)', () => {
  it('needs 2 characters, or 1 for CJK', () => {
    expect(parseQuery('a')).toBeNull()
    expect(parseQuery(' a ')).toBeNull()
    expect(parseQuery('ab')).toEqual({ needle: 'ab', exact: false })
    expect(parseQuery('鯨')).toEqual({ needle: '鯨', exact: false })
  })

  it('treats straight or curly quotes as an exact phrase', () => {
    expect(parseQuery('"Call me Ishmael"')).toEqual({ needle: 'Call me Ishmael', exact: true })
    expect(parseQuery('“Call me Ishmael”')).toEqual({ needle: 'Call me Ishmael', exact: true })
  })
})

describe('searchChapter', () => {
  it('ignores case and diacritics (F2)', () => {
    expect(find('Le café et le CAFE, puis Café.', 'cafe')).toEqual(['café', 'CAFE', 'Café'])
    expect(find('Le café et le CAFE.', 'CAFÉ')).toEqual(['café', 'CAFE'])
  })

  it('matches across whitespace and line breaks', () => {
    expect(find('Call me\n   Ishmael. Some years ago', 'me ishmael')).toEqual(['me\n   Ishmael'])
  })

  it('quoted phrases match exactly', () => {
    const text = 'Call me Ishmael. call me ishmael. Call  me Ishmael.'
    expect(find(text, '"Call me Ishmael"')).toEqual(['Call me Ishmael', 'Call  me Ishmael'])
    expect(find(text, 'call me ishmael')).toHaveLength(3)
  })

  it('finds CJK text with a single character', () => {
    expect(find('吾輩は猫である。名前はまだ無い。猫', '猫')).toEqual(['猫', '猫'])
    expect(find('吾輩は猫である。', '猫で')).toEqual(['猫で'])
  })

  it('reports astral characters whole', () => {
    expect(find('a 𠮷野家 b', '𠮷野')).toEqual(['𠮷野'])
  })
})

describe('searchOrder and searchBook (F3)', () => {
  const chapters = [3, 0, 2, 1].map((index) =>
    indexChapter({ index, text: `whale in chapter ${index}` }),
  )

  it('starts at the current chapter and wraps', () => {
    expect(searchOrder(chapters, 2).map((c) => c.index)).toEqual([2, 3, 0, 1])
    expect(searchOrder(chapters, 0).map((c) => c.index)).toEqual([0, 1, 2, 3])
    expect(searchOrder(chapters, 9).map((c) => c.index)).toEqual([0, 1, 2, 3])
  })

  it('streams results per chapter in that order and skips empty chapters', () => {
    const extra = [...chapters, indexChapter({ index: 4, text: 'no match' })]
    const got = [...searchBook(extra, parseQuery('whale')!, 3)].map((r) => r.index)
    expect(got).toEqual([3, 0, 1, 2])
  })
})

describe('snippet (F4)', () => {
  const text =
    'It was the Bottle Conjuror! Upon my soul, this is the most extraordinary sight; and the whale went down into the sea, and the ship followed after it for a very long while.'

  it('is about 90 characters, cut at word boundaries, with the match separate', () => {
    const start = text.indexOf('whale')
    const s = snippet(text, start, start + 5)
    expect(s.match).toBe('whale')
    const total = (s.before + s.match + s.after).replace(/…/g, '')
    expect(total.length).toBeLessThanOrEqual(SNIPPET_LENGTH)
    expect(total.length).toBeGreaterThan(SNIPPET_LENGTH - 20)
    expect(s.before.startsWith('…')).toBe(true)
    expect(s.after.endsWith('…')).toBe(true)
    expect(text).toContain(s.before.slice(1) + s.match + s.after.slice(0, -1))
    expect(s.before[1]).not.toBe(' ')
  })

  it('does not add ellipses at the ends of the text', () => {
    const s = snippet('the whale', 4, 9)
    expect(s).toEqual({ before: 'the ', match: 'whale', after: '' })
  })
})

describe('CJK normalisation', () => {
  it('keeps voicing marks: ば is not は', () => {
    expect(normalize('ば').text).toBe('ば')
    expect(find('はじめ', 'ば')).toEqual([])
  })

  it('matches decomposed and precomposed kana alike', () => {
    expect(find('智に働けば角が立つ', '智に働けば')).toEqual(['智に働けば'])
  })

  it('unifies half-width katakana with full-width', () => {
    expect(find('ｶﾀｶﾅ', 'カタカナ')).toEqual(['ｶﾀｶﾅ'])
  })

  it('folds decomposed Latin diacritics', () => {
    expect(find('Café noir', 'café')).toEqual(['Café'])
    expect(find('Café noir', '"Café"')).toEqual(['Café'])
  })
})
