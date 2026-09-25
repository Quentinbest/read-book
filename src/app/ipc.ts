// Typed wrappers over the Rust commands (src-tauri/src/commands.rs).

import { invoke } from '@tauri-apps/api/core'
import type { AnnotationRow } from '../lib/annotations/model'

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
  /** E6: the chapter at the saved position (Continue reading). */
  chapter_label: string | null
}

/** E10, G3: the book info sheet's extra fields. */
export interface BookInfo {
  a11y: [string, string][]
  file_size: number
  added_at: number
  publisher: string | null
  published: string | null
  identifier: string | null
  description: string | null
}

export type ImportOutcome =
  | { kind: 'imported'; book_id: string; title: string; damaged: number }
  | { kind: 'alreadyInLibrary'; book_id: string }
  | { kind: 'replaced'; book_id: string; title: string; damaged: number }
  | { kind: 'rejected'; reason: string; hostile: boolean; drm: boolean }

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
  /** The book file for the reader (library files only). */
  bookBytes: (bookId: string) => invoke<ArrayBuffer>('book_bytes', { bookId }),
  bookDamage: (bookId: string) => invoke<string[]>('book_damage', { bookId }),
  searchTextGet: (bookId: string) => invoke<[number, string][]>('search_text_get', { bookId }),
  searchTextPut: (bookId: string, chapters: [number, string][]) =>
    invoke<void>('search_text_put', { bookId, chapters }),
  annotationsList: (bookId: string) => invoke<AnnotationRow[]>('annotations_list', { bookId }),
  annotationSave: (annotation: AnnotationRow) => invoke<void>('annotation_save', { annotation }),
  annotationDelete: (id: string) => invoke<void>('annotation_delete', { id }),
  annotationRestore: (id: string) => invoke<void>('annotation_restore', { id }),
  openExternal: (url: string) => invoke<void>('open_external', { url }),
  copyText: (text: string) => invoke<void>('copy_text', { text }),
  bookSettingsGet: (bookId: string) =>
    invoke<[string, string | null] | null>('book_settings_get', { bookId }),
  bookSettingsSet: (bookId: string, layoutMode: string, navigatorDocked: string | null) =>
    invoke<void>('book_settings_set', { bookId, layoutMode, navigatorDocked }),
  positionSave: (bookId: string, cfi: string, fraction: number, chapterLabel: string | null) =>
    invoke<void>('position_save', { bookId, cfi, fraction, chapterLabel }),
  libraryRemove: (bookId: string) => invoke<void>('library_remove', { bookId }),
  libraryRestore: (bookId: string) => invoke<void>('library_restore', { bookId }),
  bookShowFile: (bookId: string) => invoke<void>('book_show_file', { bookId }),
  bookInfo: (bookId: string) => invoke<BookInfo>('book_info', { bookId }),
  positionGet: (bookId: string) => invoke<[string, number] | null>('position_get', { bookId }),
  settingGet: (key: string) => invoke<string | null>('setting_get', { key }),
  settingSet: (key: string, value: string) => invoke<void>('setting_set', { key, value }),
  /** Results of files opened from the OS before the UI was listening. */
  openedTake: () => invoke<ImportResult[]>('opened_take'),
  /** T8, S11: Dock auto-hide and full screen, for the chrome's reveal zones. */
  screenEdges: () =>
    invoke<{
      dock_autohide: boolean
      dock_edge: string
      fullscreen: boolean
      menu_bar_height: number
    }>('screen_edges'),
  /** Screens 02/03: the window buttons show with the reader's controls. */
  setWindowControls: (visible: boolean) => invoke<void>('set_window_controls', { visible }),
  /** N5: saving is done; the app may quit. */
  quitReady: () => invoke<void>('quit_ready'),
  /** T6: VoiceOver running (single-key shortcuts and page-turn motion switch off). */
  screenReaderRunning: () => invoke<boolean>('screen_reader_running'),
  /** T7: what each physical key prints in the current layout, keyed by KeyboardEvent.code. */
  keyboardLayoutLabels: () => invoke<Record<string, string>>('keyboard_layout_labels'),
}
