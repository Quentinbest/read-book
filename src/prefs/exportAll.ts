// D2 (provisional): every book's highlights and notes as W3C Web Annotation JSON
// (A9), one file per book, into a folder the reader chose.

import { invoke } from '@tauri-apps/api/core'
import { ipc } from '../app/ipc'
import { fromRow, toW3C } from '../lib/annotations/model'

/** A file name from a title: no folders, nothing hidden. */
export function exportFileName(title: string): string {
  const safe = title
    .replace(/[/\\:\0]/g, '-')
    .replace(/^\.+/, '')
    .trim()
    .slice(0, 120)
  return `${safe || 'Untitled'} — highlights.json`
}

export async function exportAllAnnotations(dir: string): Promise<number> {
  let n = 0
  for (const book of await ipc.libraryList()) {
    const rows = await ipc.annotationsList(book.id)
    if (!rows.length) continue
    const source = book.package_identifier ?? `urn:linen:book:${book.id}`
    const doc = {
      '@context': 'http://www.w3.org/ns/anno.jsonld',
      type: 'AnnotationCollection',
      label: book.title,
      total: rows.length,
      first: { type: 'AnnotationPage', items: rows.map((r) => toW3C(fromRow(r), source)) },
    }
    await invoke('export_write', {
      dir,
      name: exportFileName(book.title),
      contents: JSON.stringify(doc, null, 2),
    })
    n++
  }
  return n
}
