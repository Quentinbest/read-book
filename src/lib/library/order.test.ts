import { describe, expect, it } from 'vitest'
import { authorKey, matches, searchAndSort, sortBooks, titleKey, type Sortable } from './order'

const book = (
  id: string,
  title: string,
  authors: string[],
  added: number,
  opened: number | null = null,
): Sortable => ({
  id,
  title,
  authors,
  added_at: added,
  opened_at: opened,
})

const shelf = [
  book('1', 'The Odyssey', ['Homer'], 10),
  book('2', 'Moby-Dick; or, The Whale', ['Herman Melville'], 20, 500),
  book('3', 'Middlemarch', ['George Eliot'], 30, 400),
  book('4', 'Anna Karenina', ['Leo Tolstoy'], 40),
  book('5', 'Frankenstein', ['Mary Shelley'], 50, 600),
  book('6', 'Les Misérables', ['Victor Hugo'], 60),
  book('7', 'untitled', [], 70),
  book('8', 'A Tale of Two Cities', ['Charles Dickens'], 80),
]

describe('E6 library order', () => {
  it('Recent: last opened first, then the newest added', () => {
    expect(sortBooks(shelf, 'recent').map((b) => b.id)).toEqual([
      '5',
      '2',
      '3',
      '8',
      '7',
      '6',
      '4',
      '1',
    ])
  })
  it('Title: ignores leading articles, case and accents', () => {
    expect(sortBooks(shelf, 'title').map((b) => b.title)).toEqual([
      'Anna Karenina',
      'Frankenstein',
      'Les Misérables',
      'Middlemarch',
      'Moby-Dick; or, The Whale',
      'The Odyssey',
      'A Tale of Two Cities',
      'untitled',
    ])
  })
  it('Author: by surname; no author last', () => {
    expect(sortBooks(shelf, 'author').map((b) => b.id)).toEqual([
      '8',
      '3',
      '1',
      '6',
      '2',
      '5',
      '4',
      '7',
    ])
    expect(authorKey(['Herman Melville'])).toBe('melville herman')
    expect(titleKey('The Odyssey')).toBe('odyssey')
  })
  it('Search: every word starts a word of the title or an author, accents folded', () => {
    expect(matches(shelf[5], 'miserables')).toBe(true)
    expect(matches(shelf[1], 'mel moby')).toBe(true)
    expect(matches(shelf[1], 'elville')).toBe(false)
    expect(searchAndSort(shelf, 'the', 'title').map((b) => b.id)).toEqual(['2', '1'])
  })

  it('holds on a 500-book fixture: stable, complete and in order, fast', () => {
    const names = ['Austen', 'Brontë', 'Dickens', 'Eliot', 'Hardy', 'Melville', 'Tolstoy', 'Woolf']
    const books = Array.from({ length: 500 }, (_, i) =>
      book(
        `b${String(i).padStart(3, '0')}`,
        `${i % 3 === 0 ? 'The ' : ''}Volume ${(i * 37) % 500}`,
        [`Author ${names[i % names.length]}`],
        i,
        i % 5 ? null : 10_000 + ((i * 13) % 500),
      ),
    )
    const t0 = performance.now()
    const byTitle = sortBooks(books, 'title')
    const byAuthor = sortBooks(books, 'author')
    const recent = sortBooks(books, 'recent')
    expect(performance.now() - t0).toBeLessThan(100)
    for (const list of [byTitle, byAuthor, recent])
      expect(new Set(list.map((b) => b.id)).size).toBe(500)
    const numbers = byTitle.map((b) => Number(b.title.replace(/\D/g, '')))
    expect(numbers).toEqual([...numbers].sort((a, b) => a - b)) // numeric collation: 2 before 10
    expect(byAuthor[0].authors[0]).toBe('Author Austen')
    expect(recent.slice(0, 100).every((b) => b.opened_at !== null)).toBe(true)
    expect(searchAndSort(books, 'melville', 'title')).toHaveLength(62) // i % 8 === 5
  })
})
