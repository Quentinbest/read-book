// Typed wrappers over the Rust commands (src-tauri/src/commands.rs).

import { invoke } from '@tauri-apps/api/core'

export interface Book {
  id: string
  content_hash: string
  package_identifier: string | null
  file_path: string
  title: string
  title_source: 'package' | 'heading' | 'filename'
  authors: string[]
  language: string | null
  page_direction: string
  layout: 'reflowable' | 'fixed'
  has_page_list: boolean
  cover_path: string | null
  generated_cover_tint: string | null
  added_at: number
  opened_at: number | null
  finished_at: number | null
  replaced_at: number | null
  damaged_items: number
  fraction: number | null
}

export type ImportOutcome =
  | { kind: 'imported'; book_id: string; title: string; damaged: number }
  | { kind: 'alreadyInLibrary'; book_id: string }
  | { kind: 'replaced'; book_id: string; title: string; damaged: number }
  | { kind: 'rejected'; reason: string; hostile: boolean }

export interface ImportResult {
  path: string
  outcome: ImportOutcome
}

export type CommandError =
  { kind: 'saveFailed'; message: string } | { kind: 'failed'; message: string }

export function isCommandError(e: unknown): e is CommandError {
  return typeof e === 'object' && e !== null && 'kind' in e && 'message' in e
}

export const ipc = {
  libraryList: () => invoke<Book[]>('library_list'),
  libraryImport: (paths: string[]) => invoke<ImportResult[]>('library_import', { paths }),
  positionSave: (bookId: string, cfi: string, fraction: number) =>
    invoke<void>('position_save', { bookId, cfi, fraction }),
  positionGet: (bookId: string) => invoke<[string, number] | null>('position_get', { bookId }),
  settingGet: (key: string) => invoke<string | null>('setting_get', { key }),
  settingSet: (key: string, value: string) => invoke<void>('setting_set', { key, value }),
  /** Results of files opened from the OS before the UI was listening. */
  openedTake: () => invoke<ImportResult[]>('opened_take'),
}
