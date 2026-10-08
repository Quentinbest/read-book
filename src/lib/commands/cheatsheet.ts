// The `?` cheat sheet (K9, G10): every shortcut, grouped. Built from the command
// registry so it cannot drift from the keys that work; page keys (I8, I9) are
// handled by the reader rather than the registry, so they are listed here.

import { t } from '../strings'
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

/** Section titles, in the UI language. */
export const sectionTitle = (s: CommandSection) => t.cheatSheet.sections[s]

/** Page keys (I8 Pages, I9 Scroll): the reader's own, not registry commands. */
export function pageKeys(): CheatRow[] {
  const { space, pageUp, pageDown } = t.keys
  return [
    { label: t.cheatSheet.nextPage, keys: ['→', space, pageDown, '↓'] },
    { label: t.cheatSheet.previousPage, keys: ['←', `⇧${space}`, pageUp, '↑'] },
    { label: t.cheatSheet.scrollScreen, keys: [space, `⇧${space}`] },
    { label: t.cheatSheet.scrollLines, keys: ['↓', '↑'] },
  ]
}

const ORDER: CommandSection[] = [
  'navigation',
  'reading',
  'search',
  'annotation',
  'view',
  'app',
  'extension',
]

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
    { title: t.cheatSheet.pages, rows: pageKeys() },
    ...ORDER.filter((s) => bySection.has(s)).map((s) => ({
      title: sectionTitle(s),
      rows: bySection.get(s)!,
    })),
  ]
}
