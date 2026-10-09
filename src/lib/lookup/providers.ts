// LK11, LK15, EQ1 (Reading Lens): the providers a lookup can use, the menu above
// the answer that switches between them, and the label the core puts on an
// answer. The core owns every label; providers only fill the LK2 fields.

import { t } from '../strings'
import type { LookupResult } from './result'

export interface LookupProvider {
  /** Stable within a peek: `mac`, or `<extension>/<lookup>@<language>`. */
  key: string
  kind: 'mac' | 'extension'
  /** The menu's words: “Explain”, “This Mac’s dictionaries”. */
  title: string
  extId?: string
  lookupId?: string
  /** The extension's name (for its failure messages). */
  name?: string
  /** BCP 47: the language the extension answers in (LK11: each language the reader uses). */
  language?: string
}

/** A language's own name: 简体中文, English, Español. */
export function languageName(tag: string, inLanguage = tag): string {
  try {
    const name = new Intl.DisplayNames([inLanguage], { type: 'language' }).of(tag) ?? tag
    return name.charAt(0).toLocaleUpperCase(inLanguage) + name.slice(1)
  } catch {
    return tag
  }
}

/** LK11: the languages the reader uses, for extension lookups: the UI's, then English. */
export function readerLanguages(uiLocale: string): string[] {
  return uiLocale === 'en' || uiLocale === 'en-XA' ? ['en'] : [uiLocale, 'en']
}

/** The extension lookups, one per language the reader uses, then this Mac's dictionaries. */
export function lookupProviders(
  lookups: { extId: string; id: string; title: string; name: string }[],
  options: { languages: string[]; mac: boolean },
): LookupProvider[] {
  const out: LookupProvider[] = []
  for (const l of lookups)
    for (const language of options.languages)
      out.push({
        key: `${l.extId}/${l.id}@${language}`,
        kind: 'extension',
        title: l.title,
        extId: l.extId,
        lookupId: l.id,
        name: l.name,
        language,
      })
  if (options.mac) out.push({ key: 'mac', kind: 'mac', title: t.lens.macDictionaries })
  return out
}

/** The menu's words for a provider: “Explain · 简体中文”, or its title alone when one language applies. */
export function providerLabel(p: LookupProvider, all: LookupProvider[]): string {
  if (p.kind !== 'extension' || !p.language) return p.title
  const languages = all.filter((x) => x.extId === p.extId && x.lookupId === p.lookupId)
  return languages.length > 1 ? `${p.title} · ${languageName(p.language)}` : p.title
}

/** LK11, LK15: a menu when there is a choice; plain text when one provider applies. */
export function providerMenu(providers: LookupProvider[], current: string) {
  const items = providers.map((p) => ({
    key: p.key,
    label: providerLabel(p, providers),
    checked: p.key === current,
    group: p.kind === 'extension' ? 'extension' : 'dictionary',
  }))
  return { plain: providers.length <= 1, items }
}

/**
 * EQ1: the label above an answer, from a closed set. AI answers name their model;
 * dictionaries say where they are. Never “verified”, “source” or a provider's own words.
 */
export function sourceLabel(result: Pick<LookupResult, 'source'> | null, provider: LookupProvider) {
  if (provider.kind === 'mac') return t.lens.labelMacDictionary
  if (!result) return ''
  const s = result.source
  return s.kind === 'ai' ? t.lens.labelAi(s.model ?? s.name) : t.lens.labelDictionary(s.name)
}
