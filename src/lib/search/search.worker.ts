// The search worker (F8): indexes chapters off the main thread and streams
// results back as each chapter is scanned (see session.ts).

import type { ChapterText } from './search'
import { SearchSession, type SearchEvent } from './session'

export type SearchRequest =
  | { type: 'add'; chapters: ChapterText[] }
  | { type: 'search'; id: number; query: string; order: number[] }
  | { type: 'cancel' }

const session = new SearchSession()
/** One message per batch: the reader applies a batch in one update. */
const send = (events: SearchEvent[]) => {
  if (events.length) postMessage(events)
}

self.onmessage = (e: MessageEvent<SearchRequest>) => {
  const msg = e.data
  if (msg.type === 'add') send(session.add(msg.chapters))
  else if (msg.type === 'search') send(session.search(msg.id, msg.query, msg.order))
  else session.cancel()
}
