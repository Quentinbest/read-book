// Linen Host API v1 (P2): what an extension's Worker sees as `linen`.
// Every call is asynchronous and checked against the permissions the reader
// granted at install; a call without its permission is refused.

export interface Selection {
  /** The selected text. */
  text: string
  /** Where it is: an EPUB CFI. */
  cfi: string
  /** 1.1, during a lookup: the text around the selection (see LookupRequest). */
  context?: SelectionContext
}

/** 1.1, experimental: whole sentences around a selection, in its own chapter, 1,200 characters at most. */
export interface SelectionContext {
  before: string
  sentence: string
  after: string
  paragraph: string
  /** The chapter's label in the book's Contents. */
  chapter: string
  /** Where the selection sits in `sentence`. */
  selection: { start: number; end: number }
}

/** 1.1, experimental: what a lookup is asked. */
export interface LookupRequest {
  text: string
  context: SelectionContext
  /** The book's primary language subtag, or "". */
  bookLang: string
  /** The language to answer in (BCP 47). */
  language: string
}

/** 1.1, experimental: plain text in fixed fields; Linen renders it and writes the label. */
export interface LookupAnswer {
  status: 'ok' | 'needs_context' | 'error'
  headword: string
  term?: string
  meaning: string
  /** A qualifier that changes the meaning: shown with it, never only under More. */
  qualifier?: string
  /** Shown under More. */
  details?: { label: string; text: string }[]
  source: { kind: 'ai' | 'dictionary'; name: string; model?: string }
  /** What was sent, as sent. */
  sent?: string
  /** needs_context: what the passage lacks. */
  missing?: string
  error?: 'offline' | 'unauthorized' | 'rate_limited' | 'unavailable'
}

/** A W3C Web Annotation (A9), with Linen's colour and the chapter it is in. */
export interface Annotation {
  id: string
  type: 'Annotation'
  motivation: 'highlighting' | 'commenting'
  created: string
  modified: string
  body: { type: 'TextualBody'; value: string; format: 'text/plain'; purpose: 'commenting' }[]
  target: {
    source: string
    selector: (
      | { type: 'FragmentSelector'; conformsTo: string; value: string }
      | { type: 'TextQuoteSelector'; exact: string; prefix: string; suffix: string }
    )[]
  }
  'linen:color': 'yellow' | 'green' | 'blue' | 'rose'
  'linen:chapter'?: string
}

/** 1.1: a reading session, from the first move of the place to the last. */
export interface ReadingSession {
  startedAt: string
  endedAt: string
  /** Without pauses longer than five minutes. */
  activeSeconds: number
  startFraction: number
  endFraction: number
  pagesTurned: number
  book?: { title: string; identifier: string | null }
}

export interface Linen {
  apiVersion: '1.1.0'
  /** 1.1, experimental: answer a lookup declared in `contributes.lookups`. Needs `book.selection`. */
  lookups: {
    register(
      id: string,
      handler: (request: LookupRequest, options: { signal: AbortSignal }) => LookupAnswer | null | Promise<LookupAnswer | null>,
    ): Promise<true>
  }
  commands: {
    /** Handle a command declared in the manifest. The context says how it was invoked. */
    register(id: string, handler: (context: { source?: 'selection'; export?: { book: string } }) => unknown): Promise<true>
  }
  book: {
    /** Needs `book.metadata`. */
    metadata(): Promise<{ title: string; authors: string[]; language: string | null; identifier: string | null }>
    /** Needs `book.selection`; only while the reader is using the extension (its command or action). */
    selection(): Promise<Selection>
    /** Needs `book.text`. */
    chapters(): Promise<{ index: number; label: string }[]>
    /** Needs `book.text`: the reading text of one chapter. */
    text(range: { chapter: number }): Promise<string>
  }
  annotations: {
    /** Needs `annotations.read`: the open book's highlights and notes, in reading order. */
    list(): Promise<{ label: string; total: number; first: { items: Annotation[] } }>
    /** Needs `annotations.read`, and `onAnnotations` in `activation`. Read-only events. */
    on(event: 'created' | 'changed' | 'deleted', handler: (a: Annotation | { id: string }) => void): Promise<true>
  }
  /** 1.1. */
  reading: {
    /**
     * Needs `reading.sessions`, and `onReadingSessions` in `activation`. Each reading
     * session as it ends; `book` only with `book.metadata`. Linen keeps no statistics.
     */
    on(event: 'sessionEnded', handler: (s: ReadingSession) => void): Promise<true>
  }
  library: {
    /** Needs `library.read`. */
    list(): Promise<{ title: string; authors: string[]; language: string | null; progress: number | null }[]>
  }
  net: {
    /** Needs `network:<host>` for the URL's host (and port). Redirects are not followed. */
    fetch(url: string, init?: { method?: 'GET' | 'POST'; body?: string }): Promise<{ status: number; content_type: string | null; body: string }>
  }
  files: {
    /** Needs `files.export`. The reader picks the place in the system's save dialog, each time. */
    save(options: { suggestedName: string; content: string; type?: string }): Promise<{ saved: boolean }>
  }
  /** Private to the extension, up to 10 MB. No permission needed. */
  storage: {
    get(key: string): Promise<string | null>
    set(key: string, value: string): Promise<void>
    delete(key: string): Promise<void>
    keys(): Promise<string[]>
  }
}

declare global {
  const linen: Linen
  /** In extension pages (Navigator tabs), after <script src="_ui.js">. */
  const linenUi: {
    onMessage(handler: (data: unknown) => void): void
    post(data: unknown): void
  }
}
