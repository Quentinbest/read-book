// L-1, docs/i18n-plan.md §6: UI text lives in the catalogue. This scan finds string
// literals and Svelte template text that read like UI words (a capital, then more
// words) anywhere else in the product code, so new English cannot slip past
// translation. Developer-facing text (logs, errors thrown to extension code) is not
// UI and is skipped by its line.

import { describe, expect, it } from 'vitest'

/** The product sources, by path from the repository root (“/src/…”). */
const SOURCES = import.meta.glob<string>('/src/**/*.{ts,svelte}', {
  query: '?raw',
  import: 'default',
  eager: true,
})
const SKIP =
  /(\.test\.ts$|\.d\.ts$|^\/src\/spikes\/|^\/src\/gallery\/|^\/src\/lib\/strings\/|^\/src\/extensions\/golden\/)/
/** Lines that hold developer text, not UI text. */
const DEV_LINE = /^\s*(\/\/|\*|\/\*)|console\.|log(Error|Warn)?\(|throw |Error\(|^\s*import /

/** Words that are not to be translated, each with why. */
const ALLOWED = new Map([
  ['Helvetica Neue', 'a font family'],
  ['Aa', 'the reading-settings glyph, drawn as type (Screen 09)'],
  ['Safari 16.4', 'a product name and version (D7-WebKit)'],
])

const LITERAL = /(['"`])((?:(?!\1)[^\\\n]|\\.)*)\1/g
const UI_WORDS = /^\s*[A-Z][a-z’']+(?: [A-Za-z’'…]+)+/
/** Template text made of words only (not code or symbols). */
const WORDS_ONLY = /^[A-Za-z][A-Za-z0-9’' .,!?…-]*$/
const TEMPLATE_TEXT = /(?<![=-])>([^<>{}]*[A-Za-z]{2,}[^<>{}]*)(?=<|\{)/g

/** Allowed words met by the scan: each must still exist, or it leaves the list. */
const seen = new Set<string>()

function findings(): string[] {
  const out: string[] = []
  for (const [file, text] of Object.entries(SOURCES)) {
    if (SKIP.test(file)) continue
    // In a component, template text is read outside <script> and <style> only.
    let markup = !file.endsWith('.svelte')
    text.split('\n').forEach((line, i) => {
      if (file.endsWith('.svelte')) {
        if (/^<(script|style)\b/.test(line)) markup = false
        else if (/^<\/(script|style)>/.test(line)) return void (markup = true)
        else if (!markup && i === 0) markup = true
      }
      if (DEV_LINE.test(line)) return
      const hits = [...line.matchAll(LITERAL)]
        .map((m) => m[2].replace(/\$\{[^}]*\}/g, ' '))
        .filter((v) => UI_WORDS.test(v))
      if (file.endsWith('.svelte') && markup)
        hits.push(
          ...[...line.matchAll(TEMPLATE_TEXT)]
            .map((m) => m[1].trim())
            .filter((v) => WORDS_ONLY.test(v)),
        )
      for (const h of hits.map((x) => x.trim()))
        if (ALLOWED.has(h)) seen.add(h)
        else if (h) out.push(`${file}:${i + 1}: ${h}`)
    })
  }
  return out
}

describe('L-1 UI text comes from the catalogue', () => {
  it('no UI words are written into the product code', () => {
    expect(findings()).toEqual([])
    // The allowed words are found where they are, so the scan reads templates too.
    expect([...seen].sort()).toEqual([...ALLOWED.keys()].sort())
  })
  it('the scan finds what it should', () => {
    expect(UI_WORDS.test('Next page')).toBe(true)
    expect(UI_WORDS.test('btn small')).toBe(false)
    expect([...'<kbd>Esc</kbd>'.matchAll(TEMPLATE_TEXT)].map((m) => m[1])).toEqual(['Esc'])
    expect([...'onclick={() => go()}>{t.x}<'.matchAll(TEMPLATE_TEXT)]).toEqual([])
  })
})
