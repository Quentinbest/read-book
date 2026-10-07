// i18n spike (docs/spikes/i18n-spike.md; L-4): what WebKit and macOS each report
// as the reader's languages, and what the bundle offers AppKit. Run with
// `LINEN_SPIKE=lang`, adding `-AppleLanguages '(ja-JP)'` to stand in for the
// per-app language System Settings writes.

import { invoke } from '@tauri-apps/api/core'
import { negotiate } from '../lib/strings/negotiate'
import type { Locale } from '../lib/strings'
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
