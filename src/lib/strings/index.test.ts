import { afterEach, describe, expect, it } from 'vitest'
import { en } from './en'
import * as strings from './index'
import type { Locale } from './types'
import { pseudo, pseudoLocalize } from './pseudo'
import plist from '../../../src-tauri/Info.plist?raw'

describe('L-1 the page language', () => {
  afterEach(() => strings.setLocale('en'))

  it('starts in English', () => {
    expect(strings.locale).toBe('en')
    expect(strings.t).toBe(en)
  })
  it('a language this build lacks means English', () => {
    strings.setLocale('xx' as Locale)
    expect(strings.locale).toBe('en')
    expect(strings.t.library.title).toBe('Library')
  })
  it('switches `t` for importers (development builds offer the pseudo-locale)', () => {
    // Development builds also offer drafts in the folder (docs/i18n/README.md).
    expect(strings.AVAILABLE[0]).toBe('en')
    expect(strings.AVAILABLE).toContain('en-XA')
    expect(strings.SHIPPED).toEqual(['en'])
    strings.setLocale('en-XA')
    expect(strings.locale).toBe('en-XA')
    expect(strings.t.library.title).toBe(pseudo('Library'))
    expect(strings.missingKeys).toEqual([])
  })
  it('the bundle declares exactly the languages that ship (spike finding 4)', () => {
    const list = plist.slice(plist.indexOf('<key>CFBundleLocalizations</key>'))
    const declared = [...list.slice(0, list.indexOf('</array>')).matchAll(/<string>([^<]+)</g)].map(
      (m) => m[1],
    )
    expect(declared).toEqual(strings.SHIPPED)
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

describe('Stage 4 phrase breaks for Chinese and Japanese', () => {
  it('a break opportunity follows full-width punctuation inside a text, not at its end', async () => {
    const { phraseBreaks } = await import('./breaks')
    expect(phraseBreaks('ドロップするか、開いてください。')).toBe(
      'ドロップするか、​開いてください。',
    )
    expect(phraseBreaks('「本」を開く')).toBe('「本」​を開く')
    expect(phraseBreaks('Open a book…')).toBe('Open a book…')
  })
  it('applies to messages and their output, and only to Chinese and Japanese', async () => {
    const { withPhraseBreaks } = await import('./breaks')
    const m = withPhraseBreaks({
      ...en,
      library: { ...en.library, removed: (x: string) => `「${x}」を削除しました` },
    } as never) as typeof en
    expect(m.library.removed('A')).toBe('「A」​を削除しました')
    strings.setLocale('en-XA')
    expect(strings.t.library.title).not.toContain('​')
  })
})
