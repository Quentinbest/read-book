import { describe, expect, it } from 'vitest'
import { cheatSheet } from './cheatsheet'
import { chordLabel } from './keys'
import { CORE_COMMANDS as COMMANDS } from './registry'

describe('K9 cheat sheet', () => {
  const sheet = cheatSheet(COMMANDS)
  const listed = new Set(sheet.flatMap((s) => s.rows.flatMap((r) => r.keys)))

  it('lists every chord, alternative key and single key in the registry', () => {
    for (const c of COMMANDS) {
      const alts = (c.altChords ?? []).filter((a) => a.code !== c.chord?.code)
      for (const k of [c.chord, ...alts, c.singleKey])
        if (k) expect(listed.has(chordLabel(k)), `${c.id} ${chordLabel(k)}`).toBe(true)
    }
  })

  it('shows a typing variant of the same key once (⌘+, not also ⇧⌘+)', () => {
    const larger = sheet.flatMap((s) => s.rows).find((r) => r.label === 'Larger text')!
    expect(larger.keys).toEqual(['⌘+'])
  })

  it('starts with the page keys and names every section', () => {
    expect(sheet[0].title).toBe('Pages')
    expect(sheet.every((s) => s.title && s.rows.length)).toBe(true)
  })
})
