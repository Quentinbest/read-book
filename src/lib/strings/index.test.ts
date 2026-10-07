import { afterEach, describe, expect, it } from 'vitest'
import { en } from './en'
import * as strings from './index'
import { pseudo, pseudoLocalize } from './pseudo'

describe('L-1 the page language', () => {
  afterEach(() => strings.setLocale('en'))

  it('starts in English', () => {
    expect(strings.locale).toBe('en')
    expect(strings.t).toBe(en)
  })
  it('a language this build lacks means English', () => {
    strings.setLocale('ja')
    expect(strings.locale).toBe('en')
    expect(strings.t.library.title).toBe('Library')
  })
  it('switches `t` for importers (development builds offer the pseudo-locale)', () => {
    expect(strings.AVAILABLE).toEqual(['en', 'en-XA'])
    strings.setLocale('en-XA')
    expect(strings.locale).toBe('en-XA')
    expect(strings.t.library.title).toBe(pseudo('Library'))
    expect(strings.missingKeys).toEqual([])
  })
  it('every language has a name in its own script', () => {
    for (const l of strings.AVAILABLE) expect(strings.LANGUAGE_NAMES[l]).toBeTruthy()
  })
})

describe('en-XA pseudo-locale', () => {
  it('accents, pads and brackets the words', () => {
    expect(pseudo('Open a book')).toBe('⟦Öpéñ á böök~~~~⟧')
  })
  it('leaves the values passed in alone', () => {
    const m = pseudoLocalize(en)
    expect(m.library.removed('Moby-Dick')).toContain('Moby-Dick')
    expect(m.library.count(3)).toContain('3')
    expect(m.library.count(3)).toContain('böökš')
  })
  it('covers every message', () => {
    const walk = (a: object, b: object, path = ''): string[] =>
      Object.entries(a).flatMap(([k, v]) =>
        typeof v === 'object'
          ? walk(v, (b as Record<string, object>)[k], `${path}${k}.`)
          : typeof (b as Record<string, unknown>)[k] === typeof v
            ? []
            : [`${path}${k}`],
      )
    expect(walk(en, pseudoLocalize(en))).toEqual([])
  })
})
