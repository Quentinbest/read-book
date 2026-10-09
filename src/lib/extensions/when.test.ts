import { describe, expect, it } from 'vitest'
import { primaryLang, when, type WhenContext } from './when'

const ctx: WhenContext = {
  'selection.words': 2,
  'selection.chars': 11,
  'selection.language': 'en',
  'book.language': 'en',
  'book.fixedLayout': false,
  'book.lang': 'en',
  'selection.sentences': 1,
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

describe('LK9 book.lang and selection.sentences', () => {
  it('reports the primary subtag, lowercase, and "" for none or und', () => {
    expect(primaryLang('en')).toBe('en')
    expect(primaryLang('en-GB')).toBe('en')
    expect(primaryLang('EN-us')).toBe('en')
    expect(primaryLang('und')).toBe('')
    expect(primaryLang('')).toBe('')
    expect(primaryLang(null)).toBe('')
    expect(primaryLang(undefined)).toBe('')
  })
  it('lets a lookup condition accept "" as well as en (O12)', () => {
    const explain = '(book.lang == "en" || book.lang == "") && selection.words <= 40'
    expect(when(explain, ctx)).toBe(true)
    expect(when(explain, { ...ctx, 'book.lang': '' })).toBe(true)
    expect(when(explain, { ...ctx, 'book.lang': 'fr' })).toBe(false)
  })
  it('compares at the boundaries (40 against 41 words; one sentence against two)', () => {
    expect(when('selection.words <= 40', { ...ctx, 'selection.words': 40 })).toBe(true)
    expect(when('selection.words <= 40', { ...ctx, 'selection.words': 41 })).toBe(false)
    expect(when('selection.sentences <= 1', ctx)).toBe(true)
    expect(when('selection.sentences <= 1', { ...ctx, 'selection.sentences': 2 })).toBe(false)
  })
})
