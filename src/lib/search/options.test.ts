import { describe, expect, it } from 'vitest'
import { INVALID_PATTERN, indexChapter, parseQuery, searchChapter } from './search'
import { SearchSession } from './session'

const chapter = indexChapter({
  index: 3,
  text: 'The whale, the whaleboat and the Whale-ship. Café society; cafés. 鯨は大きい。',
})
const find = (raw: string, o: { wholeWord?: boolean; regex?: boolean }) => {
  const q = parseQuery(raw, o)
  if (!q || q === INVALID_PATTERN) throw new Error(`no query: ${String(q)}`)
  return searchChapter(chapter, q).matches.map((m) => chapter.original.slice(m.start, m.end))
}

describe('B6 whole words (1.1)', () => {
  it('matches only whole words, still ignoring case and accents', () => {
    expect(find('whale', {})).toEqual(['whale', 'whale', 'Whale'])
    expect(find('whale', { wholeWord: true })).toEqual(['whale', 'Whale'])
    expect(find('cafe', { wholeWord: true })).toEqual(['Café'])
  })

  it('CJK has no word spaces: whole words changes nothing', () => {
    expect(find('鯨', { wholeWord: true })).toEqual(['鯨'])
  })
})

describe('B6 regular expressions (1.1)', () => {
  it('matches a pattern, ignoring case', () => {
    expect(find('whale\\w*', { regex: true })).toEqual(['whale', 'whaleboat', 'Whale'])
    expect(find('caf[eé]s?', { regex: true })).toEqual(['Café', 'cafés'])
  })

  it('combines with whole words', () => {
    expect(find('whale\\w*', { regex: true, wholeWord: true })).toEqual([
      'whale',
      'whaleboat',
      'Whale',
    ])
    expect(find('whal', { regex: true, wholeWord: true })).toEqual([])
  })

  it('skips empty matches instead of looping', () => {
    expect(find('x*', { regex: true })).toEqual([])
  })

  it('says when a pattern does not parse', () => {
    expect(parseQuery('(whale', { regex: true })).toBe(INVALID_PATTERN)
    const s = new SearchSession()
    s.add([{ index: 3, text: chapter.original }])
    expect(s.search(1, '(whale', [3], { regex: true })).toEqual([
      { type: 'done', id: 1, searched: 0, total: 0, invalid: true },
    ])
  })
})
