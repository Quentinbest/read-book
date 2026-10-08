// Each page chooses its UI language before it imports the UI (L-4, L-5): the
// `language` setting, else macOS's preferred languages, else English. The menu bar
// is built once, so the choice holds until Linen restarts.

import { AVAILABLE, loadLocale, setLocale, type Locale } from '../lib/strings'
import { LANGUAGE_SETTING, resolveLocale } from '../lib/strings/negotiate'
import { ipc } from './ipc'

export async function initLocale(): Promise<Locale> {
  // A build with English only needs no lookups; otherwise the two run together
  // (§6.4: little before first paint).
  const l =
    AVAILABLE.length > 1
      ? resolveLocale(
          ...(await Promise.all([
            ipc.settingGet(LANGUAGE_SETTING).catch(() => null),
            ipc.preferredLanguages().catch(() => [...navigator.languages]),
          ])),
          AVAILABLE,
        )
      : 'en'
  await loadLocale(l)
  setLocale(l)
  document.documentElement.lang = l
  return l
}
