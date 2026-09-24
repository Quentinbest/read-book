// Spike F search worker: builds the per-chapter index and scans it off the main thread (F8).

import {
  indexChapter,
  parseQuery,
  searchBook,
  type ChapterText,
  type IndexedChapter,
} from '../lib/search/search'

let index: IndexedChapter[] = []

export type Request =
  { type: 'index'; cache: string } | { type: 'search'; query: string; current: number }

self.onmessage = (e: MessageEvent<Request>) => {
  const msg = e.data
  if (msg.type === 'index') {
    // `cache` is the on-disk form: the extracted plain text of every chapter.
    const t0 = performance.now()
    const chapters = JSON.parse(msg.cache) as ChapterText[]
    const t1 = performance.now()
    index = chapters.map(indexChapter)
    const t2 = performance.now()
    postMessage({ type: 'indexed', parseMs: t1 - t0, normalizeMs: t2 - t1, chapters: index.length })
    return
  }
  const t0 = performance.now()
  const query = parseQuery(msg.query)
  let results = 0
  let chapters = 0
  const firstChapters: number[] = []
  const samples: string[] = []
  if (query) {
    for (const r of searchBook(index, query, msg.current)) {
      chapters++
      results += r.matches.length
      if (firstChapters.length < 3) firstChapters.push(r.index)
      for (const m of r.matches) {
        if (samples.length < 3)
          samples.push(m.snippet.before + '[' + m.snippet.match + ']' + m.snippet.after)
      }
    }
  }
  postMessage({
    type: 'searched',
    ms: performance.now() - t0,
    results,
    chapters,
    firstChapters,
    samples,
    query,
  })
}
