// L17: a foliate-js book loader that reads entries from the core on demand, so a
// large book never crosses into the WebView whole (the §6.4 memory budget).

import { invoke } from '@tauri-apps/api/core'

export interface EntryLoader {
  loadText(name: string): Promise<string | null>
  loadBlob(name: string, type?: string): Promise<Blob | null>
  getSize(name: string): number
  /** §6.4: a URL the app serves a media entry from directly (no blob), or null. */
  urlFor?(name: string, type: string): string | null
}

/** Must match `commands::media_type` in the core: the types served on the book scheme. */
const DIRECT_MEDIA = /^(image\/(png|jpeg|gif|webp|avif|bmp)|audio\/|video\/)/

/** The URL of a media entry on the book scheme (`commands::BOOK_SCHEME`). */
export function bookMediaUrl(bookId: string, name: string): string {
  const path = name.split('/').map(encodeURIComponent).join('/')
  return `linen-book://localhost/${encodeURIComponent(bookId)}/${path}`
}

/** The core serves by extension, so both the manifest type and the extension must be media. */
const hasMediaExtension = (name: string) =>
  /\.(png|jpe?g|gif|webp|avif|bmp|mp3|m4a|aac|ogg|oga|opus|wav|flac|mp4|m4v|webm|ogv|mov)$/i.test(
    name,
  )

/** The book-scheme URL for a media entry, or null when it must load as a blob. */
export function directMediaUrl(bookId: string, name: string, type: string): string | null {
  return DIRECT_MEDIA.test(type) && hasMediaExtension(name) ? bookMediaUrl(bookId, name) : null
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
    // Images, audio and video load straight from the zip: as blobs, WebKit kept
    // several copies of each in its networking process until a garbage collection.
    urlFor: (name, type) => (entries.has(name) ? directMediaUrl(bookId, name, type) : null),
  }
}
