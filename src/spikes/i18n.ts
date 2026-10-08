// i18n spike (docs/spikes/i18n-spike.md; L-4): what WebKit and macOS each report
// as the reader's languages, and what the bundle offers AppKit. Run with
// `LINEN_SPIKE=lang`, adding `-AppleLanguages '(ja-JP)'` to stand in for the
// per-app language System Settings writes.

import { invoke } from '@tauri-apps/api/core'
import { adoptMessages, t, type Locale, type Messages } from '../lib/strings'
import { en } from '../lib/strings/en'
import { negotiate } from '../lib/strings/negotiate'
import { pseudoLocalize } from '../lib/strings/pseudo'
import type { SpikeResult } from './common'

const ALL: Locale[] = ['en', 'zh-Hans', 'zh-Hant', 'ja', 'es']

export async function spikeLang(): Promise<SpikeResult> {
  const preferred = await invoke<string[]>('preferred_languages')
  const bundle = await invoke<Record<string, unknown>>('spike_bundle_languages')
  const raw = {
    args: await invoke<Record<string, unknown>>('spike_info'),
    navigatorLanguage: navigator.language,
    navigatorLanguages: [...navigator.languages],
    intlDefault: new Intl.DateTimeFormat().resolvedOptions().locale,
    preferred,
    bundle,
    chosenFromPreferred: negotiate(preferred, ALL),
    chosenFromNavigator: negotiate([...navigator.languages], ALL),
  }
  const agree = raw.chosenFromPreferred === raw.chosenFromNavigator
  return {
    spike: 'lang',
    criteria: [
      {
        id: 'I18N-preferred',
        description: 'NSLocale.preferredLanguages reaches the page (L-4)',
        verdict: preferred.length > 0 ? 'pass' : 'fail',
        evidence: `preferred ${JSON.stringify(preferred)} → ${raw.chosenFromPreferred}`,
      },
      {
        id: 'I18N-navigator',
        description: 'navigator.languages agrees with NSLocale',
        verdict: 'manual',
        evidence: `navigator ${JSON.stringify(raw.navigatorLanguages)} → ${raw.chosenFromNavigator} (${agree ? 'agrees' : 'differs'}); Intl default ${raw.intlDefault}`,
      },
      {
        id: 'I18N-bundle',
        description: 'What the bundle offers AppKit (CFBundleLocalizations)',
        verdict: 'manual',
        evidence: JSON.stringify(bundle),
      },
    ],
    raw,
  }
}

// Stage 4 (docs/i18n-plan.md): the visual run in any language. `LINEN_LOCALE=ja
// scripts/e2e.sh v` reaches the page as `?locale=ja`; captures and the report go to
// i18n-out/visual/<tag>/ (spikes.rs), and each capture lists the text that does
// not fit its box.

const DRAFTS = import.meta.glob<Record<string, Messages>>(
  '../lib/strings/{zh-Hans,zh-Hant,ja,es}.ts',
  { eager: true },
)

/** Use the run's language before the app mounts: a draft, the pseudo-locale or English. */
export function applyRunLocale(): Locale {
  const l = (new URLSearchParams(location.search).get('locale') ?? 'en') as Locale
  if (l === 'en') return l
  const m =
    l === 'en-XA' ? pseudoLocalize(en) : Object.values(DRAFTS[`../lib/strings/${l}.ts`] ?? {})[0]
  if (!m) throw new Error(`no catalogue for ${l}`)
  adoptMessages(l, m)
  document.documentElement.lang = l
  return l
}

/** UI text wider than its box (or taller, where clipped); book text is left out. */
export function overflowing(root: Document = document): string[] {
  const out: string[] = []
  for (const el of Array.from(root.body.querySelectorAll<HTMLElement>('*'))) {
    if (el instanceof HTMLIFrameElement || el.closest('[aria-hidden="true"]')) continue
    // Book text carries its own lang and is cut short on purpose (Contents, titles).
    if (el.closest('body [lang]')) continue
    const text = Array.from(el.childNodes)
      .filter((n) => n.nodeType === Node.TEXT_NODE)
      .map((n) => n.textContent ?? '')
      .join('')
      .trim()
    // Screen-reader text sits in a 1 px box by design (.visually-hidden).
    if (!text || el.clientWidth <= 1) continue
    const style = getComputedStyle(el)
    if (/auto|scroll/.test(style.overflowX + style.overflowY)) continue
    const clipped = /hidden|clip/.test(style.overflowX + style.overflowY)
    const wide = el.scrollWidth > el.clientWidth + 1
    const tall = clipped && el.scrollHeight > el.clientHeight + 1
    if (!wide && !tall) continue
    const name = `${el.localName}${el.className && typeof el.className === 'string' ? '.' + el.className.trim().split(/\s+/).join('.') : ''}`
    out.push(
      `${name}: “${text.slice(0, 60)}” (${el.scrollWidth}×${el.scrollHeight} in ${el.clientWidth}×${el.clientHeight})`,
    )
  }
  return out
}

/** Plain texts of a catalogue, by key (messages, which take values, are left out). */
function plainTexts(m: unknown, path = ''): Map<string, string> {
  const out = new Map<string, string>()
  for (const [k, v] of Object.entries(m as Record<string, unknown>)) {
    const key = path ? `${path}.${k}` : k
    if (typeof v === 'string') out.set(key, v)
    else if (v && typeof v === 'object') for (const e of plainTexts(v, key)) out.set(...e)
  }
  return out
}

/**
 * Stage 6 (plan §6): UI text still in English in a translated UI, which means it
 * bypassed `t`. Visible text and labels (aria-label, title, placeholder) equal to
 * an English catalogue text that this language translates differently; book text,
 * which carries its own lang, is left out.
 */
export function englishLeft(root: Document = document): string[] {
  const ours = plainTexts(t)
  const english = new Set(
    [...plainTexts(en)]
      .filter(([k, v]) => /\p{L}{4}/u.test(v) && ours.get(k) !== v)
      .map(([, v]) => v),
  )
  const out = new Set<string>()
  for (const el of Array.from(root.body.querySelectorAll<HTMLElement>('*'))) {
    // Book text (its own lang) and extension text (translate="no"; L-10) are not ours.
    if (el instanceof HTMLIFrameElement || el.closest('body [lang], [translate="no"]')) continue
    for (const n of Array.from(el.childNodes))
      if (n.nodeType === Node.TEXT_NODE && english.has((n.textContent ?? '').trim()))
        out.add(`${el.localName}: “${(n.textContent ?? '').trim()}”`)
    for (const a of ['aria-label', 'title', 'placeholder']) {
      const v = el.getAttribute(a)
      if (v && english.has(v.trim())) out.add(`${el.localName}[${a}]: “${v.trim()}”`)
    }
  }
  return [...out]
}
