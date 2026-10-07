import { describe, expect, it } from 'vitest'
import { languageKey, negotiate, resolveLocale } from './negotiate'
import type { Locale } from './types'

const ALL: Locale[] = ['en', 'zh-Hans', 'zh-Hant', 'ja', 'es']

describe('L-2, L-3 language keys', () => {
  it.each([
    ['zh-Hans-CN', 'zh-Hans'],
    ['zh-Hans', 'zh-Hans'],
    ['zh-CN', 'zh-Hans'],
    ['zh-SG', 'zh-Hans'],
    ['zh', 'zh-Hans'],
    ['zh-Hant-TW', 'zh-Hant'],
    ['zh-Hant-HK', 'zh-Hant'],
    ['zh-TW', 'zh-Hant'],
    ['zh-HK', 'zh-Hant'],
    ['zh_MO', 'zh-Hant'],
    ['zh-Hans-HK', 'zh-Hans'],
    ['es-419', 'es'],
    ['es-ES', 'es'],
    ['ja-JP', 'ja'],
    ['EN-us', 'en'],
  ])('%s → %s', (tag, key) => expect(languageKey(tag)).toBe(key))
})

describe('L-4 choosing the UI language', () => {
  it('takes the first preferred language Linen has', () => {
    expect(negotiate(['fr-FR', 'zh-Hant-TW', 'en-US'], ALL)).toBe('zh-Hant')
    expect(negotiate(['es-MX'], ALL)).toBe('es')
    expect(negotiate(['de-DE', 'fr'], ALL)).toBe('en')
    expect(negotiate([], ALL)).toBe('en')
  })
  it('falls back to English when a preferred language is not in this build', () => {
    expect(negotiate(['ja-JP'], ['en'])).toBe('en')
  })
  it('matches the pseudo-locale only exactly', () => {
    expect(negotiate(['en-US'], ['en', 'en-XA'])).toBe('en')
    expect(negotiate(['en-XA'], ['en', 'en-XA'])).toBe('en-XA')
  })
  it('the setting wins; system or an unknown value follows macOS', () => {
    expect(resolveLocale('ja', ['es-ES'], ALL)).toBe('ja')
    expect(resolveLocale('system', ['es-ES'], ALL)).toBe('es')
    expect(resolveLocale(null, ['es-ES'], ALL)).toBe('es')
    // A language a later build dropped (L-9 rollback).
    expect(resolveLocale('ja', ['es-ES'], ['en', 'es'])).toBe('es')
    expect(resolveLocale('klingon', ['fr'], ALL)).toBe('en')
  })
})
