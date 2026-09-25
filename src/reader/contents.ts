// Contents (N6, E1, E3; Screen 04).
//
// The list comes from the first source that has entries: the book's navigation
// (foliate-js reads the EPUB 3 nav document, then the NCX), else a scan of the
// chapters' headings (labelled “Generated from headings”), else the spine.
// Entries in damaged chapters are marked so nothing looks silently missing.

export type ContentsSource = 'nav' | 'headings' | 'spine'

export interface ContentsItem {
  label: string
  href: string
  /** Nesting depth, 0 for top-level entries. */
  depth: number
  /** Spine index the entry points into, or -1 when it cannot be resolved. */
  section: number
  damaged: boolean
}

export interface Contents {
  source: ContentsSource
  items: ContentsItem[]
}

export interface TocEntry {
  label?: string
  href?: string
  subitems?: TocEntry[] | null
}

export interface ContentsBook {
  toc?: TocEntry[] | null
  sections: { id: string; linear?: string; createDocument?: () => Promise<Document> }[]
  resolveHref?: (href: string) => { index: number } | null | undefined
}

/** Deeper entries are folded into their parents' rows (Screen 04 lists two levels at most). */
export const MAX_DEPTH = 2

const clean = (s: string | undefined) => (s ?? '').replace(/\s+/g, ' ').trim()

function sectionOf(book: ContentsBook, href: string): number {
  try {
    return book.resolveHref?.(href)?.index ?? -1
  } catch {
    return -1
  }
}

/** Flatten the navigation tree into rows. */
export function fromToc(book: ContentsBook, damaged: Set<string>): ContentsItem[] {
  const rows: ContentsItem[] = []
  const walk = (entries: TocEntry[] | null | undefined, depth: number) => {
    for (const e of entries ?? []) {
      const label = clean(e.label)
      const href = e.href ?? ''
      if (label && href && depth < MAX_DEPTH) {
        const section = sectionOf(book, href)
        rows.push({
          label,
          href,
          depth,
          section,
          damaged: section >= 0 && damaged.has(book.sections[section]?.id),
        })
      }
      walk(e.subitems, depth + 1)
    }
  }
  walk(book.toc, 0)
  return rows
}

/** The first heading (h1–h3) of each chapter, for books without navigation. */
export async function fromHeadings(
  book: ContentsBook,
  damaged: Set<string>,
): Promise<ContentsItem[]> {
  const rows: ContentsItem[] = []
  for (const [index, section] of book.sections.entries()) {
    if (section.linear === 'no') continue
    if (damaged.has(section.id)) {
      rows.push({
        label: fileLabel(section.id),
        href: section.id,
        depth: 0,
        section: index,
        damaged: true,
      })
      continue
    }
    try {
      const doc = await section.createDocument?.()
      const h = doc?.querySelector('h1, h2, h3')
      const label = clean(h?.textContent ?? '')
      if (!label) continue
      const href = h?.id ? `${section.id}#${h.id}` : section.id
      rows.push({ label, href, depth: 0, section: index, damaged: false })
    } catch {
      rows.push({
        label: fileLabel(section.id),
        href: section.id,
        depth: 0,
        section: index,
        damaged: true,
      })
    }
  }
  return rows
}

/** The last resort: one row per linear chapter, named from its file. */
export function fromSpine(book: ContentsBook, damaged: Set<string>): ContentsItem[] {
  return book.sections.flatMap((s, index) =>
    s.linear === 'no'
      ? []
      : [
          {
            label: fileLabel(s.id),
            href: s.id,
            depth: 0,
            section: index,
            damaged: damaged.has(s.id),
          },
        ],
  )
}

/** “chapter-03_intro.xhtml” → “Chapter 03 intro”. */
export function fileLabel(path: string): string {
  const base = (path.split('/').pop() ?? path).replace(/\.[^.]+$/, '').replace(/[-_]+/g, ' ')
  return clean(base.charAt(0).toUpperCase() + base.slice(1)) || path
}

export async function buildContents(book: ContentsBook, damagedIds: string[]): Promise<Contents> {
  const damaged = new Set(damagedIds)
  const nav = fromToc(book, damaged)
  if (nav.length) return { source: 'nav', items: nav }
  const headings = await fromHeadings(book, damaged)
  if (headings.length) return { source: 'headings', items: headings }
  return { source: 'spine', items: fromSpine(book, damaged) }
}

/**
 * The row for the reader's place (“You are here”): the entry foliate reports for
 * the location, else the last entry that starts at or before the current chapter.
 */
export function currentIndex(
  items: ContentsItem[],
  place: { tocHref?: string; sectionIndex: number },
): number {
  if (place.tocHref) {
    const exact = items.findIndex((i) => i.href === place.tocHref)
    if (exact >= 0) return exact
  }
  let best = -1
  for (const [i, item] of items.entries())
    if (item.section >= 0 && item.section <= place.sectionIndex) best = i
  return best
}
