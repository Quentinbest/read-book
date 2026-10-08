// @vitest-environment jsdom
// The Stage 6 scan for UI text left in English (src/spikes/i18n.ts).

import { afterEach, describe, expect, it } from 'vitest'
import { adoptMessages, setLocale } from '../lib/strings'
import { en } from '../lib/strings/en'
import { pseudoLocalize } from '../lib/strings/pseudo'
import { englishLeft } from './i18n'

describe('Stage 6 English left in a translated UI', () => {
  afterEach(() => {
    document.body.innerHTML = ''
    setLocale('en')
  })

  it('finds English text and labels the language translates', () => {
    // A language that keeps the font's name, as real ones do.
    const xa = pseudoLocalize(en)
    adoptMessages('en-XA', { ...xa, aa: { ...xa.aa, fontLiterata: 'Literata' } })
    document.body.innerHTML = `
      <h1>Your library is empty</h1>
      <button aria-label="Close image"></button>
      <input placeholder="Search library">
      <p>Literata</p>`
    expect(englishLeft().sort()).toEqual([
      'button[aria-label]: “Close image”',
      'h1: “Your library is empty”',
      'input[placeholder]: “Search library”',
    ])
  })

  it('leaves book text, which carries its own lang, and translated text alone', () => {
    adoptMessages('en-XA', pseudoLocalize(en))
    document.body.innerHTML = `
      <span lang="en">Library</span>
      <span translate="no">Dictionary</span>
      <h1>${pseudoLocalize(en).library.emptyTitle}</h1>`
    expect(englishLeft()).toEqual([])
  })
})
