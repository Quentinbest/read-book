// Library search and sort (E6): search by title and author; sort by Recent,
// Title or Author. Pure, so the order is tested on a 500-book fixture.

export type SortKey = 'recent' | 'title' | 'author'

export interface Sortable {
  id: string
  title: string
  authors: string[]
  added_at: number
  opened_at: number | null
}

/** Case, accents and punctuation do not matter to search or sort. */
export function fold(s: string): string {
  return s
    .normalize('NFKD')
    .replace(/\p{M}+/gu, '')
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, ' ')
    .trim()
}

/** Titles sort without a leading article (“The Odyssey” under O). */
export function titleKey(title: string): string {
  return fold(title).replace(/^(the|a|an) /, '')
}

/** Authors sort by the first author's surname, then given names (“Melville, Herman”). */
export function authorKey(authors: string[]): string {
  const first = fold(authors[0] ?? '')
  if (!first) return '￿' // books with no author go last
  const parts = first.split(' ')
  return [parts.at(-1), ...parts.slice(0, -1)].join(' ')
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })

/** Every word typed must start a word of the title or an author (in any order). */
export function matches(book: Sortable, query: string): boolean {
  const words = fold(query).split(' ').filter(Boolean)
  if (!words.length) return true
  const hay = ` ${fold(book.title)} ${book.authors.map(fold).join(' ')}`
  return words.every((w) => hay.includes(` ${w}`))
}

export function sortBooks<T extends Sortable>(books: T[], key: SortKey): T[] {
  const recent = (b: T) => b.opened_at ?? b.added_at
  const byId = (a: T, b: T) => collator.compare(a.id, b.id)
  return [...books].sort((a, b) => {
    if (key === 'recent') return recent(b) - recent(a) || byId(a, b)
    if (key === 'title') return collator.compare(titleKey(a.title), titleKey(b.title)) || byId(a, b)
    return (
      collator.compare(authorKey(a.authors), authorKey(b.authors)) ||
      collator.compare(titleKey(a.title), titleKey(b.title)) ||
      byId(a, b)
    )
  })
}

export function searchAndSort<T extends Sortable>(books: T[], query: string, key: SortKey): T[] {
  return sortBooks(
    books.filter((b) => matches(b, query)),
    key,
  )
}
