// The English message catalogue (B10): every UI string lives here, so the UI can
// be translated later. No translations or RTL UI mirroring in the MVP.

const fileName = (path: string) => path.split('/').pop() ?? path

export const en = {
  library: {
    emptyTitle: 'Your library is empty',
    emptyBody: 'Drop EPUB files here, or open one from your computer.',
    openBook: 'Open a book…',
    open: 'Open…',
    note: 'Books are copied into your library and stay on this device.',
    title: 'Library',
    count: (n: number) => `${n} ${n === 1 ? 'book' : 'books'}`,
    unknownAuthor: 'Unknown author',
    damaged: (n: number) => `${n} damaged`,
    new: 'New',
    finished: 'Finished',
    percent: (fraction: number) => `${Math.round(fraction * 100)}%`,
  },
  // Import outcomes: wording PROVISIONAL until the G4 designs exist.
  import: {
    addedDamaged: (title: string, n: number) =>
      `“${title}” was added · ${n} damaged ${n === 1 ? 'chapter' : 'chapters'}`,
    alreadyInLibrary: (path: string) => `“${fileName(path)}” is already in your library`,
    replaced: (title: string) => `“${title}” was updated from a newer file`,
    drm: (path: string) => `“${fileName(path)}” is protected by DRM and can’t be opened in Linen`,
    hostile: (path: string) => `“${fileName(path)}” couldn’t be opened safely and wasn’t added`,
    rejected: (path: string, reason: string) =>
      `“${fileName(path)}” couldn’t be opened · ${reason}`,
    fileFilter: 'EPUB',
  },
  reader: {
    library: 'Library',
    previousPage: 'Previous page',
    nextPage: 'Next page',
    minutesLeft: (n: number) => `${n} min left in chapter`,
    resumedIn: (chapter: string) => `Resumed in ${chapter}`,
    resumed: 'Resumed where you left off',
    goToBeginning: 'Go to beginning',
    backToPage: (page: string) => `Back to page ${page}`,
    back: 'Back',
    pageAnnouncement: (page: number) => `Page ${page}`,
    pageAnnouncementApprox: (page: number) => `About page ${page}`,
    /** An estimated page number (L16), e.g. in “Back to page ≈312”. */
    approxPage: (page: number) => `≈${page}`,
    /** G8: fixed-layout books show their real pages, exactly. */
    fixedPages: (pages: number[], total: number) =>
      pages.length > 1
        ? `Pages ${pages[0]}–${pages[pages.length - 1]} of ${total}`
        : `Page ${pages[0]} of ${total}`,
    zoomLevel: (zoom: number) => `${Math.round(zoom * 100)}%`,
    zoomFit: 'Fit',
    /** G8: shown only when opening takes over 500 ms. */
    opening: (title: string) => `Opening “${title}”…`,
  },
  messages: {
    region: 'Message',
    dismiss: 'Dismiss',
    saveFailed: 'Couldn’t save notes to disk',
    retry: 'Retry',
  },
  commands: {
    'chapter.next': 'Next chapter',
    'chapter.previous': 'Previous chapter',
    'search.open': 'Search in book',
    'search.next': 'Next result',
    'search.previous': 'Previous result',
    'navigator.contents': 'Go to chapter…',
    'goto.open': 'Go to location…',
    'navigator.notes': 'Highlights and notes',
    'selection.highlight': 'Highlight selection',
    'selection.note': 'Add note to selection',
    'selection.focusBar': 'Move to selection actions',
    'reader.caretBrowsing': 'Caret browsing',
    'text.larger': 'Larger text',
    'text.smaller': 'Smaller text',
    'text.reset': 'Default text size',
    'layout.pages': 'Pages Mode',
    'layout.scroll': 'Scroll Mode',
    'palette.open': 'Command palette',
    'shortcuts.show': 'Keyboard shortcuts',
    'history.back': 'Back',
    'library.show': 'Library',
    'book.open': 'Open…',
    'window.fullScreen': 'Full screen',
    'edit.undo': 'Undo',
    'layer.close': 'Close layer',
  },
} as const

export const t = en
