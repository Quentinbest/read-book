// L17: a foliate-js book loader that reads entries from the core on demand, so a
// large book never crosses into the WebView whole (the §6.4 memory budget).

import { invoke } from '@tauri-apps/api/core'

export interface EntryLoader {
  loadText(name: string): Promise<string | null>
  loadBlob(name: string, type?: string): Promise<Blob | null>
  getSize(name: string): number
}

export async function libraryLoader(bookId: string): Promise<EntryLoader> {
  const entries = new Map(await invoke<[string, number][]>('book_entries', { bookId }))
  const bytes = (name: string) => invoke<ArrayBuffer>('book_entry', { bookId, name })
  return {
    async loadText(name) {
      if (!entries.has(name)) return null
      return new TextDecoder().decode(await bytes(name))
    },
    async loadBlob(name, type) {
      if (!entries.has(name)) return null
      return new Blob([await bytes(name)], type ? { type } : undefined)
    },
    getSize: (name) => entries.get(name) ?? 0,
  }
}
