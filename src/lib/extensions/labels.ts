// Plain words for permissions and contributions (P3; Screen 11, G1): what the
// Settings list and the install sheet show.

import { t } from '../strings'
import type { ExtensionManifest } from './types'

export interface PermissionLabel {
  label: string
  /** P3: highlighted in the install sheet (book.text, annotations.write, wildcards, background). */
  warn: boolean
  /** A network permission: shown with its globe (Screen 11). */
  network?: boolean
}

/** P3: the permissions the install sheet highlights. */
const WARN = new Set(['book.text', 'annotations.write', 'background'])

export function permissionLabel(p: string): PermissionLabel {
  const words = t.extSettings.permissions as Record<string, string>
  if (p in words) return { label: words[p], warn: WARN.has(p) }
  const host = p.replace(/^network:/, '')
  return host.startsWith('*.')
    ? { label: t.extSettings.connectAny(host.slice(1)), warn: true, network: true }
    : { label: t.extSettings.connect(host), warn: false, network: true }
}

/** What an extension adds (Screen 11: “Adds”). */
export function contributionLabels(m: ExtensionManifest): string[] {
  const c = m.contributes
  const out: string[] = []
  if (c.commands.length) out.push(t.extSettings.commandsAdded(c.commands.length))
  for (const a of c.selectionActions) {
    const title = c.commands.find((x) => x.id === a.command)?.title ?? a.command
    out.push(t.extSettings.inSelectionMenu(title))
  }
  if (c.navigatorTabs.length) out.push(t.extSettings.navigatorTab)
  if (c.themes.length) out.push(t.extSettings.themesAdded(c.themes.length))
  if (c.exporters.length) out.push(t.extSettings.exportInNotes)
  return out
}
