// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest'
import { chordLabel, isTextField, type KeyContext } from './keys'
import { CORE_COMMANDS, CommandRegistry } from './registry'

const page: KeyContext = {
  pageFocused: true,
  textFieldActive: false,
  singleKeysEnabled: true,
  screenReaderRunning: false,
  modalOpen: false,
}

const key = (
  code: string,
  mods: Partial<Record<'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey', boolean>> = {},
) => ({
  code,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
})

function registryWithAll() {
  const r = new CommandRegistry()
  const runs: string[] = []
  for (const c of CORE_COMMANDS) r.handle(c.id, { run: () => runs.push(c.id) })
  return { r, runs }
}

describe('K1–K14 bindings', () => {
  const cases: [string, ReturnType<typeof key>][] = [
    ['chapter.next', key('ArrowDown', { altKey: true })],
    ['chapter.previous', key('BracketLeft')],
    ['search.open', key('KeyF', { metaKey: true })],
    ['search.previous', key('KeyG', { metaKey: true, shiftKey: true })],
    ['navigator.contents', key('KeyT', { metaKey: true })],
    ['goto.open', key('KeyJ', { metaKey: true })],
    ['navigator.notes', key('KeyA', { metaKey: true, shiftKey: true })],
    ['selection.highlight', key('KeyH')],
    ['selection.note', key('KeyN', { metaKey: true, shiftKey: true })],
    ['selection.focusBar', key('F6')],
    ['reader.caretBrowsing', key('F7')],
    ['text.larger', key('Equal', { metaKey: true, shiftKey: true })],
    ['text.smaller', key('Minus', { metaKey: true })],
    ['text.reset', key('Digit0', { metaKey: true })],
    ['palette.open', key('KeyK', { metaKey: true })],
    ['shortcuts.show', key('Slash', { shiftKey: true })],
    ['history.back', key('BracketLeft', { metaKey: true })],
    ['library.show', key('KeyL', { metaKey: true })],
    ['book.open', key('KeyO', { metaKey: true })],
    ['window.fullScreen', key('KeyF', { metaKey: true, ctrlKey: true })],
    ['edit.undo', key('KeyZ', { metaKey: true })],
    ['layer.close', key('Escape')],
  ]
  for (const [id, event] of cases) {
    it(`${id}`, () => expect(registryWithAll().r.commandForKey(event, page)?.id).toBe(id))
  }

  it('binds physical positions: modifiers must match exactly', () => {
    const { r } = registryWithAll()
    expect(r.commandForKey(key('KeyF', { metaKey: true, shiftKey: true }), page)).toBeNull()
  })

  it('no two commands share a chord', () => {
    const seen = new Set<string>()
    for (const c of CORE_COMMANDS) {
      for (const ch of [c.chord, c.singleKey, ...(c.altChords ?? [])]) {
        if (!ch) continue
        const k = JSON.stringify({
          ...{ meta: false, ctrl: false, alt: false, shift: false },
          ...ch,
        })
        expect(seen.has(k), `${c.id} reuses ${k}`).toBe(false)
        seen.add(k)
      }
    }
  })
})

describe('single-key gating (§2.8)', () => {
  const off: [string, Partial<KeyContext>][] = [
    ['in a text field', { textFieldActive: true }],
    ['when the page does not have focus', { pageFocused: false }],
    ['when switched off', { singleKeysEnabled: false }],
    ['while a screen reader runs (T6)', { screenReaderRunning: true }],
    ['while a modal is open', { modalOpen: true }],
  ]
  for (const [name, ctx] of off) {
    it(`single keys are inert ${name}; modified chords still work`, () => {
      const { r } = registryWithAll()
      const c = { ...page, ...ctx }
      expect(r.commandForKey(key('KeyH'), c)).toBeNull()
      expect(r.commandForKey(key('BracketRight'), c)).toBeNull()
      expect(r.commandForKey(key('Slash', { shiftKey: true }), c)).toBeNull()
      expect(r.commandForKey(key('KeyH', { metaKey: true, shiftKey: true }), c)?.id).toBe(
        'selection.highlight',
      )
    })
  }

  it('recognises text fields', () => {
    const input = document.createElement('input')
    const checkbox = Object.assign(document.createElement('input'), { type: 'checkbox' })
    const div = document.createElement('div')
    div.contentEditable = 'true'
    expect(isTextField(input)).toBe(true)
    expect(isTextField(checkbox)).toBe(false)
    expect(isTextField(document.createElement('textarea'))).toBe(true)
    expect(isTextField(document.createElement('button'))).toBe(false)
  })
})

describe('registry', () => {
  it('shows only commands with a handler, and runs them', () => {
    const r = new CommandRegistry()
    expect(r.available()).toHaveLength(0)
    const run = vi.fn()
    const off = r.handle('goto.open', { run })
    expect(r.available().map((c) => c.id)).toEqual(['goto.open'])
    expect(r.run('goto.open')).toBe(true)
    expect(run).toHaveBeenCalledOnce()
    off()
    expect(r.run('goto.open')).toBe(false)
  })

  it('does not run disabled commands', () => {
    const r = new CommandRegistry()
    const run = vi.fn()
    r.handle('edit.undo', { run, enabled: () => false })
    expect(r.run('edit.undo')).toBe(false)
    expect(run).not.toHaveBeenCalled()
  })

  it('extension commands get no shortcut and cannot claim a core id (C4, P8)', () => {
    const r = new CommandRegistry()
    const id = r.defineExtension('dict', { id: 'lookup', title: 'Look up' })
    r.handle(id, { run() {} })
    const c = r.get(id)!
    expect(c.chord).toBeUndefined()
    expect(c.singleKey).toBeUndefined()
    expect(() => r.define({ id: 'goto.open', rule: 'P8', title: 'x', section: 'app' })).toThrow()
  })
})

describe('labels', () => {
  it('uses macOS modifier order and glyphs', () => {
    expect(chordLabel({ code: 'KeyG', meta: true, shift: true })).toBe('⇧⌘G')
    expect(chordLabel({ code: 'KeyF', meta: true, ctrl: true })).toBe('⌃⌘F')
    expect(chordLabel({ code: 'ArrowDown', alt: true })).toBe('⌥↓')
    expect(chordLabel({ code: 'BracketLeft', meta: true })).toBe('⌘[')
    expect(chordLabel({ code: 'Slash', shift: true })).toBe('?')
  })

  it('prefers native layout labels for the same physical key (T7)', () => {
    const azerty = new Map([['KeyQ', 'a']])
    expect(chordLabel({ code: 'KeyQ', meta: true }, azerty)).toBe('⌘A')
  })
})

describe('menu bar model (generated from the registry)', () => {
  it('lists only commands with handlers, grouped by menu, with accelerators', async () => {
    const { accelerator, menuModel } = await import('./menu')
    const r = new CommandRegistry()
    r.handle('book.open', { run() {} })
    r.handle('search.previous', { run() {} })
    r.handle('selection.highlight', { run() {} })
    r.handle('chapter.next', { run() {} })
    const model = menuModel(r.available())
    expect(model.map((m) => m.title)).toEqual(['File', 'Edit', 'Go'])
    expect(model[0].items).toEqual([{ id: 'book.open', title: 'Open…', accelerator: 'Cmd+O' }])
    expect(model[1].items.find((i) => i.id === 'search.previous')?.accelerator).toBe('Shift+Cmd+G')
    expect(model[2].items[0].accelerator).toBe('Alt+Down')
    expect(accelerator({ code: 'KeyF', meta: true, ctrl: true })).toBe('Ctrl+Cmd+F')
    expect(accelerator({ code: 'BracketLeft', meta: true })).toBe('Cmd+[')
  })
})
