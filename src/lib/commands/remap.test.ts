import { describe, expect, it } from 'vitest'
import { CORE_COMMANDS, CommandRegistry, type CommandDef } from './registry'
import {
  assign,
  chordFromEvent,
  chordProblem,
  effectiveChords,
  parseOverrides,
  reset,
  unassign,
} from './remap'

const ext: CommandDef = {
  id: 'extension:org.example.dictionary:define',
  rule: 'P1',
  title: 'Define',
  section: 'extension',
  extensionId: 'org.example.dictionary',
}
const defs = [...CORE_COMMANDS, ext]
const byId = (id: string) => defs.find((d) => d.id === id)!
const key = (
  code: string,
  mods: Partial<Record<'meta' | 'ctrl' | 'alt' | 'shift', boolean>> = {},
) =>
  ({
    code,
    metaKey: !!mods.meta,
    ctrlKey: !!mods.ctrl,
    altKey: !!mods.alt,
    shiftKey: !!mods.shift,
  }) as KeyboardEvent

describe('C4 chords a reader can assign', () => {
  it('reads a chord from a key, not from modifiers alone', () => {
    expect(chordFromEvent(key('MetaLeft', { meta: true }))).toBeNull()
    expect(chordFromEvent(key('KeyE', { meta: true, shift: true }))).toEqual({
      code: 'KeyE',
      meta: true,
      shift: true,
    })
  })

  it('needs a modifier, except the function keys; the system’s chords stay', () => {
    expect(chordProblem({ code: 'KeyE' })).toBe('needsModifier')
    expect(chordProblem({ code: 'KeyE', shift: true })).toBe('needsModifier')
    expect(chordProblem({ code: 'F8' })).toBeNull()
    expect(chordProblem({ code: 'KeyE', ctrl: true })).toBeNull()
    for (const code of ['KeyQ', 'KeyW', 'KeyH', 'KeyM', 'Comma', 'KeyC', 'KeyV'])
      expect(chordProblem({ code, meta: true }), code).toBe('reserved')
    expect(chordProblem({ code: 'Escape', meta: true })).toBe('reserved')
  })
})

describe('C4 assigning', () => {
  it('gives an extension command a free chord', () => {
    const r = assign(defs, {}, ext.id, { code: 'KeyE', meta: true, ctrl: true })
    expect(r.ok && r.overrides[ext.id]).toEqual({ code: 'KeyE', meta: true, ctrl: true })
    expect(r.ok && r.displaced).toBeNull()
  })

  it('never lets an extension take a core shortcut', () => {
    const r = assign(defs, {}, ext.id, { code: 'KeyF', meta: true })
    expect(r).toMatchObject({ ok: false, problem: 'core', holder: { id: 'search.open' } })
  })

  it('a core command takes a chord from another, which loses it', () => {
    const r = assign(defs, {}, 'reader.settings', { code: 'KeyJ', meta: true })
    expect(r.ok).toBe(true)
    if (!r.ok) return
    expect(r.displaced?.id).toBe('goto.open')
    expect(effectiveChords(byId('goto.open'), r.overrides)).toEqual([])
  })

  it('a command with two chords keeps the other one', () => {
    const larger = byId('text.larger')
    expect(larger.altChords?.length).toBeGreaterThan(0)
    const alt = larger.altChords![0]
    const r = assign(defs, {}, 'reader.settings', alt)
    expect(r.ok).toBe(true)
    if (!r.ok) return
    expect(effectiveChords(larger, r.overrides)).toEqual([larger.chord])
  })

  it('a core command can take an extension’s chord', () => {
    const first = assign(defs, {}, ext.id, { code: 'KeyE', meta: true, ctrl: true })
    if (!first.ok) throw new Error('setup')
    const r = assign(defs, first.overrides, 'reader.settings', {
      code: 'KeyE',
      meta: true,
      ctrl: true,
    })
    expect(r.ok && r.displaced?.id).toBe(ext.id)
    expect(r.ok && r.overrides[ext.id]).toBeNull()
  })

  it('taking away, resetting, and the stored form', () => {
    const o = unassign({}, 'search.open')
    expect(effectiveChords(byId('search.open'), o)).toEqual([])
    expect(effectiveChords(byId('search.open'), reset(o, 'search.open'))).toEqual([
      byId('search.open').chord,
    ])
    expect(parseOverrides(JSON.stringify(o))).toEqual(o)
    expect(parseOverrides('{"a":{"code":"KeyQ","meta":true},"b":7}')).toEqual({})
    expect(parseOverrides('not json')).toEqual({})
  })
})

describe('C4 in the registry', () => {
  it('menus, ⌘K and keys follow the reader’s shortcuts', () => {
    const r = new CommandRegistry(defs)
    let ran = ''
    r.handle(ext.id, { run: () => (ran = 'define') })
    r.handle('search.open', { run: () => (ran = 'search') })
    const ctx = {
      pageFocused: true,
      textFieldActive: false,
      singleKeysEnabled: true,
      screenReaderRunning: false,
      modalOpen: false,
    }
    r.setOverrides({ [ext.id]: { code: 'KeyE', meta: true, ctrl: true }, 'search.open': null })
    r.commandForKey(key('KeyE', { meta: true, ctrl: true }), ctx)?.run()
    expect(ran).toBe('define')
    expect(r.commandForKey(key('KeyF', { meta: true }), ctx)).toBeNull()
    expect(r.get('search.open')?.chord).toBeUndefined()
    r.setOverrides({})
    expect(r.commandForKey(key('KeyF', { meta: true }), ctx)?.id).toBe('search.open')
  })
})
