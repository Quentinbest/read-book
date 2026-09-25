// The `?` cheat sheet (K9, G10): every shortcut, grouped. Built from the command
// registry so it cannot drift from the keys that work; page keys (I8, I9) are
// handled by the reader rather than the registry, so they are listed here.

import { chordLabel, type Chord } from './keys'
import type { Command, CommandSection } from './registry'

export interface CheatRow {
  label: string
  /** Alternatives, each already a key-cap label such as “⌘T” or “]”. */
  keys: string[]
}

export interface CheatSection {
  title: string
  rows: CheatRow[]
}

export const SECTION_TITLES: Record<CommandSection, string> = {
  navigation: 'Navigation',
  reading: 'Reading',
  search: 'Search',
  annotation: 'Highlights and notes',
  view: 'View',
  app: 'App',
}

/** Page keys (I8 Pages, I9 Scroll): the reader's own, not registry commands. */
export const PAGE_KEYS: CheatRow[] = [
  { label: 'Next page', keys: ['→', 'Space', 'PgDn', '↓'] },
  { label: 'Previous page', keys: ['←', '⇧Space', 'PgUp', '↑'] },
  { label: 'Scroll by a screen (Scroll mode)', keys: ['Space', '⇧Space'] },
  { label: 'Scroll by lines (Scroll mode)', keys: ['↓', '↑'] },
]

const ORDER: CommandSection[] = ['navigation', 'reading', 'search', 'annotation', 'view', 'app']

export function cheatSheet(
  commands: Pick<Command, 'title' | 'section' | 'chord' | 'altChords' | 'singleKey'>[],
  layout?: ReadonlyMap<string, string>,
): CheatSection[] {
  const label = (c: Chord) => chordLabel(c, layout)
  const bySection = new Map<CommandSection, CheatRow[]>()
  for (const c of commands) {
    // An alternative on the same key (⌘+ typed with or without ⇧) is a typing
    // variant of the shortcut, not another one: show the shortcut once.
    const alts = (c.altChords ?? []).filter((a) => a.code !== c.chord?.code)
    const keys = [c.chord, ...alts, c.singleKey].filter((k): k is Chord => !!k).map(label)
    const unique = Array.from(new Set(keys))
    if (!unique.length) continue
    const rows = bySection.get(c.section) ?? []
    rows.push({ label: c.title, keys: unique })
    bySection.set(c.section, rows)
  }
  return [
    { title: 'Pages', rows: PAGE_KEYS },
    ...ORDER.filter((s) => bySection.has(s)).map((s) => ({
      title: SECTION_TITLES[s],
      rows: bySection.get(s)!,
    })),
  ]
}
