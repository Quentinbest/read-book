// L-1, L-6, L-11 (docs/i18n/README.md): checks every catalogue in this folder, a
// draft too. Each message must keep every value it is given; a shipped language
// may not leave text in English (a draft only reports it); Chinese and Japanese
// follow their punctuation rules (docs/i18n/style.md).

import ts from 'typescript'
import { describe, expect, it } from 'vitest'
import { en } from './en'
import enSource from './en.ts?raw'
import { SHIPPED, type Locale } from './index'
import { pseudoLocalize } from './pseudo'
import type { Messages } from './types'

const FILES = import.meta.glob<Record<string, unknown>>('./{zh-Hans,zh-Hant,ja,es}.ts', {
  eager: true,
})
const CATALOGUES: [Locale, Messages][] = [
  ['en', en],
  ['en-XA', pseudoLocalize(en)],
  ...Object.entries(FILES).map(([path, mod]): [Locale, Messages] => [
    path.slice(2, -3) as Locale,
    Object.values(mod)[0] as Messages,
  ]),
]

/** Text that is the same in every language. */
const SAME_EVERYWHERE = new Set(['EPUB', '—', 'Literata', 'OpenDyslexic', 'Esc', 'PgUp', 'PgDn'])
/** Entries every language writes as English does (units). */
const SAME_KEYS = ['info.sizeMB', 'info.sizeKB']
/** Entries a language writes as English does, on purpose (cognates, “App”). */
const SAME_AS_ENGLISH: Partial<Record<Locale, string[]>> = {
  'zh-Hant': ['cheatSheet.sections.app'],
  es: [
    'prefs.sections.general',
    'aa.sepia',
    'aa.auto',
    'aa.normal',
    'aa.fontSans',
    'cheatSheet.sections.app',
    'goto.sentenceEnd',
  ],
}
/** Values a language may leave out of a message, by key (none yet). */
const MAY_DROP: Partial<Record<Locale, Record<string, string[]>>> = {}

/** Each message's parameter types, read from en.ts (`n: number`, `title: string`). */
function paramTypes(): Map<string, string[]> {
  const file = ts.createSourceFile('en.ts', enSource, ts.ScriptTarget.Latest, true)
  const out = new Map<string, string[]>()
  const walk = (o: ts.ObjectLiteralExpression, path: string[]) => {
    for (const p of o.properties) {
      if (!ts.isPropertyAssignment(p)) continue
      const key = [...path, p.name.getText(file).replace(/^['"]|['"]$/g, '')]
      const v = p.initializer
      if (ts.isObjectLiteralExpression(v)) walk(v, key)
      else if (ts.isArrowFunction(v))
        out.set(
          key.join('.'),
          v.parameters.map((x) => x.type?.getText(file) ?? 'string'),
        )
    }
  }
  file.forEachChild((n) => {
    if (!ts.isVariableStatement(n)) return
    for (const d of n.declarationList.declarations) {
      const init = d.initializer
      const obj = init && ts.isAsExpression(init) ? init.expression : init
      if (obj && ts.isObjectLiteralExpression(obj)) walk(obj, [])
    }
  })
  return out
}
const PARAMS = paramTypes()

function entries(m: unknown, path: string[] = []): [string, unknown][] {
  return Object.entries(m as Record<string, unknown>).flatMap(([k, v]) =>
    v && typeof v === 'object' ? entries(v, [...path, k]) : [[[...path, k].join('.'), v]],
  )
}

/** A message called with recognisable values, and the values to look for. */
function call(key: string, fn: (...a: unknown[]) => string): [string, string[]] {
  let digit = 7
  const args: unknown[] = []
  const expected: string[] = []
  ;(PARAMS.get(key) ?? []).forEach((type, i) => {
    if (type === 'boolean') return void args.push(true)
    if (type === 'number') {
      const n = digit++
      expected.push(String(n))
      return void args.push(n)
    }
    if (type === 'number[]') {
      const ns = [digit++, digit++]
      expected.push(...ns.map(String))
      return void args.push(ns)
    }
    const s = `⟨v${i}⟩`
    expected.push(s)
    args.push(s)
  })
  return [fn(...args), expected]
}

/** Every text a catalogue shows, messages called as above. */
function texts(m: Messages): [string, string][] {
  return entries(m).map(([k, v]) => [
    k,
    typeof v === 'function' ? call(k, v as never)[0] : String(v),
  ])
}

describe.each(CATALOGUES)('catalogue %s', (locale, m) => {
  it('every message keeps every value it is given', () => {
    const lost: string[] = []
    for (const [key, v] of entries(m)) {
      if (typeof v !== 'function') continue
      const [out, expected] = call(key, v as (...a: unknown[]) => string)
      const allowed = MAY_DROP[locale]?.[key] ?? []
      for (const e of expected)
        if (!out.includes(e) && !allowed.includes(e)) lost.push(`${key}: ${e} in “${out}”`)
    }
    expect(lost).toEqual([])
  })

  it.skipIf(locale === 'en' || locale === 'en-XA')('nothing is left in English', () => {
    const english = new Map(texts(en))
    const same = texts(m)
      .filter(
        ([k, v]) =>
          v === english.get(k) &&
          /\p{L}/u.test(v.replace(/⟨v\d+⟩/g, '')) &&
          !SAME_EVERYWHERE.has(v) &&
          !SAME_KEYS.includes(k) &&
          !SAME_AS_ENGLISH[locale]?.includes(k),
      )
      .map(([k]) => k)
    // VITE_I18N_STRICT=1 holds a draft to the shipping rule (docs/i18n/README.md).
    if (SHIPPED.includes(locale) || import.meta.env.VITE_I18N_STRICT) expect(same).toEqual([])
    else if (same.length)
      console.info(`[strings] draft ${locale}: ${same.length} entries still in English`)
  })

  it.skipIf(!['zh-Hans', 'zh-Hant', 'ja'].includes(locale))('L-11 punctuation', () => {
    const bad: string[] = []
    const quotes = locale === 'zh-Hans' ? /[「」]/ : /[“”]/
    const cjk = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u
    for (const [k, v] of texts(m)) {
      // Rules for translated text: a draft still holds English.
      if (!cjk.test(v)) continue
      // Full-width punctuation after Chinese or Japanese text.
      if (/[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}][,.!?:;]/u.test(v))
        bad.push(`${k}: ${v}`)
      if (quotes.test(v)) bad.push(`${k}: quotation marks in “${v}”`)
    }
    expect(bad).toEqual([])
  })
})

describe('the checks themselves', () => {
  it('know every message’s parameters', () => {
    const missing = entries(en)
      .filter(([k, v]) => typeof v === 'function' && !PARAMS.has(k))
      .map(([k]) => k)
    expect(missing).toEqual([])
    expect(PARAMS.get('library.count')).toEqual(['number'])
    expect(PARAMS.get('reader.fixedPages')).toEqual(['number[]', 'number'])
  })
  it('notice a dropped value', () => {
    const [out, expected] = call('library.removed', () => 'Removed')
    expect(expected.every((e) => out.includes(e))).toBe(false)
  })
})
