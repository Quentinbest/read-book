import { describe, expect, it } from 'vitest'
import { when, type WhenContext } from './when'

const ctx: WhenContext = {
  'selection.words': 2,
  'selection.chars': 11,
  'selection.language': 'en',
  'book.language': 'en',
  'book.fixedLayout': false,
}

describe('P1 when clauses', () => {
  it('evaluates comparisons, && || ! and parentheses', () => {
    expect(when('selection.words <= 3', ctx)).toBe(true)
    expect(when('selection.words <= 3', { ...ctx, 'selection.words': 4 })).toBe(false)
    expect(when('selection.language == "en" && !book.fixedLayout', ctx)).toBe(true)
    expect(
      when('selection.chars > 100 || (selection.words == 2 && book.language != "fr")', ctx),
    ).toBe(true)
    expect(when(null, ctx)).toBe(true)
  })
  it('is false for anything outside the language', () => {
    expect(when('window.location', ctx)).toBe(false)
    expect(when('selection.words = 2', ctx)).toBe(false)
    expect(when('selection.words <', ctx)).toBe(false)
  })
})
