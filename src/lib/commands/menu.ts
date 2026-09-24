// The macOS menu bar, generated from the command registry (plan §3 “Commands”,
// P§16). Only commands with a handler appear; later phases add theirs.

import type { Chord } from './keys'
import type { Command } from './registry'

export type MenuName = 'File' | 'Edit' | 'View' | 'Go' | 'Window'
export const MENU_ORDER: MenuName[] = ['File', 'Edit', 'View', 'Go', 'Window']

export interface MenuModelItem {
  id: string
  title: string
  accelerator?: string
}

export interface MenuModel {
  title: MenuName
  items: MenuModelItem[]
}

const KEY_NAMES: Record<string, string> = {
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  BracketLeft: '[',
  BracketRight: ']',
  Equal: '=',
  Minus: '-',
  Slash: '/',
  Escape: 'Escape',
}

/** A Tauri accelerator string for a chord, e.g. { code: 'KeyG', meta, shift } → "Shift+Cmd+G". */
export function accelerator(c: Chord): string {
  const key = KEY_NAMES[c.code] ?? c.code.replace(/^(Key|Digit)/, '')
  return [c.ctrl && 'Ctrl', c.alt && 'Alt', c.shift && 'Shift', c.meta && 'Cmd', key]
    .filter(Boolean)
    .join('+')
}

export function menuModel(commands: Command[]): MenuModel[] {
  return MENU_ORDER.map((title) => ({
    title,
    items: commands
      .filter((c) => c.menu === title)
      .map((c) => ({
        id: c.id,
        title: c.title,
        // Only modified chords become accelerators; single keys are shown in ⌘K and the cheat sheet.
        accelerator:
          c.chord && (c.chord.meta || c.chord.ctrl || c.chord.alt)
            ? accelerator(c.chord)
            : undefined,
      })),
  })).filter((m) => m.items.length > 0)
}
