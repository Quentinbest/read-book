import { describe, expect, it } from 'vitest'
import { locate, quoteAt } from './anchor'

const chapter = [
  'Call me Ishmael. Some years ago—never mind how long precisely—having little or no money in my purse,',
  'and nothing particular to interest me on shore, I thought I would sail about a little and see the watery part of the world.',
  'It is a way I have of driving off the spleen and regulating the circulation.',
  'Whenever I find myself growing grim about the mouth; whenever it is a damp, drizzly November in my soul;',
  'then, I account it high time to get to sea as soon as I can.',
].join('\n')

const at = (text: string, part: string) => {
  const start = text.indexOf(part)
  return quoteAt(text, start, start + part.length)
}
const placed = (text: string, p: ReturnType<typeof locate>) =>
  p ? text.slice(p.start, p.end) : null

describe('A9 anchoring by text quote', () => {
  const q = at(chapter, 'damp, drizzly November in my soul')

  it('finds the quote in the unchanged text', () => {
    const p = locate(chapter, q)
    expect(p?.how).toBe('exact')
    expect(placed(chapter, p)).toBe('damp, drizzly November in my soul')
  })

  it('survives an inserted paragraph before it', () => {
    const edited = chapter.replace(
      'It is a way',
      'A new paragraph the publisher added.\nIt is a way',
    )
    expect(placed(edited, locate(edited, q))).toBe('damp, drizzly November in my soul')
  })

  it('survives changed punctuation inside the quote', () => {
    const edited = chapter.replace('damp, drizzly November', 'damp; drizzly November')
    const p = locate(edited, q)
    expect(p?.how).toBe('approximate')
    expect(placed(edited, p)).toBe('damp; drizzly November in my soul')
  })

  it('survives typographic changes (quotes, dashes, spacing, case)', () => {
    const long = at(chapter, 'Some years ago—never mind how long precisely—having')
    // Only what normalising covers: dash style, case and spacing.
    const edited = chapter.replace(
      'ago—never mind how long precisely—having',
      'ago–Never mind how  long precisely–having',
    )
    const p = locate(edited, long)
    expect(p?.how).toBe('normalized')
    expect(placed(edited, p)).toBe('Some years ago–Never mind how  long precisely–having')
  })

  it('reports a deleted passage as unplaced, never a guess', () => {
    const edited = chapter.replace(/Whenever I find myself.*soul;\n/, '')
    expect(locate(edited, q)).toBeNull()
  })

  it('uses the context to choose between repeated text, and does not guess without it', () => {
    const repeated = 'I would sail. The sea. I would sail. The land.'
    const at2 = repeated.indexOf('I would sail', 1) // the second one
    const second = quoteAt(repeated, at2, at2 + 12)
    expect(locate(repeated, second)?.start).toBe(at2)
    const bare = { exact: 'I would sail', prefix: '', suffix: '' }
    expect(locate(repeated, bare)).toBeNull()
    expect(locate(repeated, bare, 20)?.start).toBe(at2) // an old position breaks the tie
  })

  it('keeps context at the chapter edges', () => {
    const q0 = quoteAt(chapter, 0, 4)
    expect(q0.prefix).toBe('')
    expect(q0.suffix.length).toBe(32)
  })
})
