import { describe, expect, it } from 'vitest'
import { cheatSheet } from './cheatsheet'
import { chordLabel } from './keys'
import { CORE_COMMANDS as COMMANDS } from './registry'

describe('K9 cheat sheet', () => {
  const sheet = cheatSheet(COMMANDS)
  const listed = new Set(sheet.flatMap((s) => s.rows.flatMap((r) => r.keys)))

  it('lists every chord, alternative and single key in the registry', () => {
    for (const c of COMMANDS)
      for (const k of [c.chord, ...(c.altChords ?? []), c.singleKey])
        if (k) expect(listed.has(chordLabel(k)), `${c.id} ${chordLabel(k)}`).toBe(true)
  })

  it('starts with the page keys and names every section', () => {
    expect(sheet[0].title).toBe('Pages')
    expect(sheet.every((s) => s.title && s.rows.length)).toBe(true)
  })
})
