import { describe, expect, it } from 'vitest'
import { fuzzy, rank } from './fuzzy'

describe('⌘K fuzzy matching (Screen 16)', () => {
  const titles = ['Go to location…', 'Go to chapter…', 'Next chapter', 'Library', 'Scroll Mode']

  it('matches characters in order, and nothing else', () => {
    expect(fuzzy('gtc', 'Go to chapter…')).not.toBeNull()
    expect(fuzzy('ctg', 'Go to chapter…')).toBeNull()
  })

  it('ranks word starts and substrings first', () => {
    expect(rank('gtc', titles, (t) => t)[0]).toBe('Go to chapter…')
    expect(rank('chap', titles, (t) => t)).toEqual(['Next chapter', 'Go to chapter…'])
    expect(rank('lib', titles, (t) => t)).toEqual(['Library'])
  })

  it('ignores case and accents, and keeps order for an empty query', () => {
    expect(fuzzy('ETE', 'Été')).not.toBeNull()
    expect(rank('', titles, (t) => t)).toEqual(titles)
  })

  it('reports matched positions for highlighting', () => {
    expect(fuzzy('scroll', 'Scroll Mode')!.indices).toEqual([0, 1, 2, 3, 4, 5])
  })
})
