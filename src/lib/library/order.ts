// Library search and sort (E6): search by title and author; sort by Recent,
// Title or Author. Pure, so the order is tested on a 500-book fixture.

export type SortKey = 'recent' | 'title' | 'author'

export interface Sortable {
  id: string
  title: string
  authors: string[]
  /** The publisher's sort form of the first author (“Melville, Herman”), when the book gives one. */
  author_sort?: string | null
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

const SUFFIX = new Set(['jr', 'sr', 'ii', 'iii', 'iv'])
const TITLE = /^(lord|lady|sir|dame|baron|baroness|count|countess) \S/

/**
 * Authors sort by the first author's surname, then given names (“Melville, Herman”):
 * the publisher's sort form when the book gives one, else the last word of the name.
 * A name already written surname first keeps its order; Jr., Sr. and numerals are not
 * surnames (“King, Martin Luther, Jr.”). “Alfred, Lord Tennyson” is not inverted.
 */
export function authorKey(authors: string[], sort?: string | null): string {
  if (sort?.trim()) return fold(sort)
  let name = authors[0] ?? ''
  if (!fold(name)) return '￿' // books with no author go last
  let suffix = ''
  const comma = name.indexOf(',')
  if (comma >= 0) {
    const tail = fold(name.slice(comma + 1))
    if (SUFFIX.has(tail)) {
      name = name.slice(0, comma)
      suffix = tail
    } else if (!TITLE.test(tail)) return fold(name)
  }
  const parts = fold(name).split(' ')
  if (!suffix && parts.length > 2 && SUFFIX.has(parts.at(-1)!)) suffix = parts.pop()!
  return [parts.at(-1), ...parts.slice(0, -1), suffix].filter(Boolean).join(' ')
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
      collator.compare(authorKey(a.authors, a.author_sort), authorKey(b.authors, b.author_sort)) ||
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
