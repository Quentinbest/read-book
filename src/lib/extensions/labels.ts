// Plain words for permissions and contributions (P3; Screen 11, G1): what the
// Settings list and the install sheet show.

import type { ExtensionManifest } from './types'

export interface PermissionLabel {
  label: string
  /** P3: highlighted in the install sheet (book.text, annotations.write, wildcards, background). */
  warn: boolean
  /** A network permission: shown with its globe (Screen 11). */
  network?: boolean
}

export function permissionLabel(p: string): PermissionLabel {
  switch (p) {
    case 'book.metadata':
      return { label: 'See the book’s title and author', warn: false }
    case 'book.selection':
      return { label: 'Read the text you select', warn: false }
    case 'book.text':
      return { label: 'Read the whole book', warn: true }
    case 'annotations.read':
      return { label: 'Read highlights and notes', warn: false }
    case 'annotations.write':
      return { label: 'Change highlights and notes', warn: true }
    case 'library.read':
      return { label: 'See every book in your library', warn: false }
    case 'reading.sessions':
      return { label: 'Know when and how long you read', warn: false }
    case 'files.import':
      return { label: 'Open files you choose', warn: false }
    case 'files.export':
      return { label: 'Save files you choose', warn: false }
    case 'background':
      return { label: 'Run in the background', warn: true }
  }
  const host = p.replace(/^network:/, '')
  return host.startsWith('*.')
    ? { label: `Connect to any address ending ${host.slice(1)}`, warn: true, network: true }
    : { label: `Connect to ${host}`, warn: false, network: true }
}

/** What an extension adds (Screen 11: “Adds”). */
export function contributionLabels(m: ExtensionManifest): string[] {
  const c = m.contributes
  const out: string[] = []
  if (c.commands.length)
    out.push(c.commands.length === 1 ? 'Command' : `${c.commands.length} commands`)
  for (const a of c.selectionActions) {
    const title = c.commands.find((x) => x.id === a.command)?.title ?? a.command
    out.push(`“${title}” in selection menu`)
  }
  if (c.navigatorTabs.length) out.push('Navigator tab')
  if (c.themes.length) out.push(c.themes.length === 1 ? 'Theme' : `${c.themes.length} themes`)
  if (c.exporters.length) out.push('Export in Notes')
  return out
}
