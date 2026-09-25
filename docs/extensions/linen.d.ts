// Linen Host API v1 (P2): what an extension's Worker sees as `linen`.
// Every call is asynchronous and checked against the permissions the reader
// granted at install; a call without its permission is refused.

export interface Selection {
  /** The selected text. */
  text: string
  /** Where it is: an EPUB CFI. */
  cfi: string
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

export interface Linen {
  apiVersion: '1.0.0'
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
