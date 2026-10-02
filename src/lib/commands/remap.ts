// Shortcut remapping (C4; 1.1, approved 2026-10-01). The reader may give any command a
// shortcut, extension commands included, or take one away. Rules (§2.8, P§12):
//
// - A shortcut needs ⌘, ⌃ or ⌥, except the function keys; Esc and Tab stay Linen's.
// - The system's own chords (⌘Q ⌘W ⌘H ⌘M, ⌘, for Settings) can't be taken.
// - Extensions can't claim core shortcuts: an extension command can't take a chord
//   that a core command uses. A core command can take a chord from any command,
//   which then loses it (the reader is told).
// - Single-key shortcuts (H, N, [ ]) are not remapped; one setting turns them off.

import { type Chord } from './keys'
import type { CommandDef } from './registry'

/** A command's shortcut set by the reader; null takes the default away. */
export type Overrides = Record<string, Chord | null>

export const OVERRIDES_SETTING = 'shortcuts'

const RESERVED: Chord[] = [
  { code: 'KeyQ', meta: true },
  { code: 'KeyW', meta: true },
  { code: 'KeyH', meta: true },
  { code: 'KeyH', meta: true, alt: true },
  { code: 'KeyM', meta: true },
  { code: 'Comma', meta: true },
  { code: 'Tab', meta: true },
  { code: 'Space', meta: true },
  // Edit menu: Cut, Copy, Paste, Select All (text fields need them).
  { code: 'KeyX', meta: true },
  { code: 'KeyC', meta: true },
  { code: 'KeyV', meta: true },
  { code: 'KeyA', meta: true },
]

const MODIFIER_CODES = /^(Meta|Control|Alt|Shift)(Left|Right)$|^(OS|CapsLock|Fn)/

export const sameChord = (a: Chord, b: Chord) =>
  a.code === b.code &&
  !!a.meta === !!b.meta &&
  !!a.ctrl === !!b.ctrl &&
  !!a.alt === !!b.alt &&
  !!a.shift === !!b.shift

/** The chord a key event makes, or null while only modifiers are down. */
export function chordFromEvent(
  e: Pick<KeyboardEvent, 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>,
): Chord | null {
  if (!e.code || MODIFIER_CODES.test(e.code)) return null
  const c: Chord = { code: e.code }
  if (e.metaKey) c.meta = true
  if (e.ctrlKey) c.ctrl = true
  if (e.altKey) c.alt = true
  if (e.shiftKey) c.shift = true
  return c
}

export type ChordProblem = 'needsModifier' | 'reserved'

/** Why a chord can't be a shortcut, or null when it can. */
export function chordProblem(c: Chord): ChordProblem | null {
  if (c.code === 'Escape' || c.code === 'Tab') return 'reserved'
  if (RESERVED.some((r) => sameChord(r, c))) return 'reserved'
  const fKey = /^F([1-9]|1[0-9])$/.test(c.code)
  if (!fKey && !c.meta && !c.ctrl && !c.alt) return 'needsModifier'
  return null
}

/** Commands on a reserved chord (Esc closes layers, ⌘, opens Settings) keep it. */
export const remappable = (def: CommandDef) => !def.chord || chordProblem(def.chord) !== 'reserved'

/** The chords that run a command: the reader's, or else its defaults. */
export function effectiveChords(def: CommandDef, overrides: Overrides): Chord[] {
  if (def.id in overrides) {
    const o = overrides[def.id]
    return o ? [o] : []
  }
  return [def.chord, ...(def.altChords ?? [])].filter((c): c is Chord => !!c)
}

export type AssignResult =
  | { ok: true; overrides: Overrides; displaced: CommandDef | null }
  | { ok: false; problem: ChordProblem | 'core'; holder?: CommandDef }

const isCore = (d: CommandDef) => !d.extensionId

/** Give `id` the chord `chord`, under the rules above. */
export function assign(
  defs: CommandDef[],
  overrides: Overrides,
  id: string,
  chord: Chord,
): AssignResult {
  const problem = chordProblem(chord)
  if (problem) return { ok: false, problem }
  const target = defs.find((d) => d.id === id)
  if (!target || !remappable(target)) return { ok: false, problem: 'reserved' }
  const holder = defs.find(
    (d) => d.id !== id && effectiveChords(d, overrides).some((c) => sameChord(c, chord)),
  )
  if (holder && isCore(holder) && !isCore(target)) return { ok: false, problem: 'core', holder }
  const next: Overrides = { ...overrides, [id]: chord }
  if (holder) {
    // The holder keeps its other chords (⌘= goes, ⌘+ stays); with none left it has none.
    const rest = effectiveChords(holder, overrides).filter((c) => !sameChord(c, chord))
    next[holder.id] = rest[0] ?? null
  }
  // Back to the default: no override needed.
  if (sameChord(chord, target.chord ?? { code: '' }) && !target.altChords?.length) delete next[id]
  return { ok: true, overrides: next, displaced: holder ?? null }
}

/** Take a command's shortcut away. */
export const unassign = (overrides: Overrides, id: string): Overrides => ({
  ...overrides,
  [id]: null,
})

/** Back to the default shortcut. */
export function reset(overrides: Overrides, id: string): Overrides {
  const next = { ...overrides }
  delete next[id]
  return next
}

/** Overrides from the stored setting; anything malformed is dropped. */
export function parseOverrides(json: string | null | undefined): Overrides {
  if (!json) return {}
  try {
    const raw = JSON.parse(json) as Record<string, unknown>
    const out: Overrides = {}
    for (const [id, v] of Object.entries(raw)) {
      if (v === null) out[id] = null
      else if (v && typeof v === 'object' && typeof (v as Chord).code === 'string') {
        const c = v as Chord
        if (!chordProblem(c)) out[id] = c
      }
    }
    return out
  } catch {
    return {}
  }
}
