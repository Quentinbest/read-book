// Phase 2 end-to-end tests, run inside the real app (macOS has no WebDriver for
// Tauri; plan §6.1 “App end-to-end tests”). Mounts the product App against a
// throwaway data folder (LINEN_DATA_DIR) and drives it like a reader would.

import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { mount } from 'svelte'
import type { TestHooks } from '../app/testHooks'
import { log, sleep, type Criterion, type SpikeResult } from './common'
import { step } from './interactive'

// Cold opens unless a check asks for the warm book (S14).
const hooks: TestHooks = { noWarm: true }
;(globalThis as { __LINEN_E2E__?: TestHooks }).__LINEN_E2E__ = hooks

async function waitFor<T>(
  what: string,
  get: () => T | null | undefined | false,
  ms = 10_000,
): Promise<T> {
  const until = performance.now() + ms
  for (;;) {
    const v = get()
    if (v) return v
    if (performance.now() > until) throw new Error(`timed out waiting for ${what}`)
    await sleep(50)
  }
}

const reader = () => hooks.reader
const loc = () => hooks.reader?.location() ?? null
/** A stable “where am I” for comparisons: section and page. */
const where = () => {
  const l = loc()
  return l ? `${l.sectionIndex}:${l.page ?? '?'}` : 'none'
}

async function settled(ms = 450) {
  await sleep(ms)
}

/** Send a key where a real one arrives: the book document while reading (focus is in the text). */
function key(k: string, opts: KeyboardEventInit = {}) {
  // A fixed-layout spread can hold an empty frame; a reader's focus is in the one with content.
  const docs = hooks.reader?.engine.view.renderer.getContents().map((c) => c.doc) ?? []
  const doc =
    docs.find((d) => d?.body?.textContent?.trim() || d?.body?.querySelector('img, svg')) ?? docs[0]
  const target = opts.metaKey || !doc ? document.body : doc.body
  target.dispatchEvent(
    new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...opts }),
  )
}

/** The same key with focus on the app window (the page body), e.g. after closing a layer. */
function keyOnApp(k: string, opts: KeyboardEventInit = {}) {
  document.body.dispatchEvent(
    new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...opts }),
  )
}

function click(selector: string) {
  const el = document.querySelector<HTMLElement>(selector)
  if (!el) throw new Error(`no element ${selector}`)
  el.click()
}

/** A book's tile in the library grid (E6), found by its title. */
function libraryTile(title: RegExp): HTMLButtonElement | undefined {
  return (
    Array.from(document.querySelectorAll<HTMLElement>('.library .tile'))
      .find((t) => title.test(t.querySelector('.tt')?.textContent ?? ''))
      ?.querySelector<HTMLButtonElement>('.open') ?? undefined
  )
}

async function openFromLibrary(title: RegExp) {
  const row = await waitFor('library tile', () => libraryTile(title))
  row.click()
  // E3: a damaged book shows its card first; the checks read it anyway.
  const card = await waitFor(
    'reader or damaged card',
    () => loc()?.cfi || document.querySelector<HTMLElement>('dialog[open] .card'),
  )
  if (card instanceof HTMLElement) card.querySelector<HTMLButtonElement>('.primary')!.click()
  await waitFor('reader location', () => loc()?.cfi)
  await settled(800)
}

async function backToLibrary() {
  key('l', { code: 'KeyL', metaKey: true })
  await waitFor('library', () => document.querySelector('.library .tile, .library .empty'))
  await settled(300)
}

/** Reveal the full controls (Screen 03): an edge reveal names the book only (S9-edge-reveal). */
async function showControls() {
  reader()!.showControls()
  await sleep(600) // 160 ms fade in
}

async function hideControls() {
  key('Escape', { code: 'Escape' })
  await sleep(400)
}

// ---------------------------------------------------------------- Phase 5 helpers (A1–A9)

const annotationsOf = () => reader()!.annotations
/** The book document showing the current page. */
const pageDoc = () => {
  const l = loc()
  const c = reader()!
    .engine.view.renderer.getContents()
    .find((x) => x.index === l?.sectionIndex && x.doc?.body?.textContent?.trim())
  if (!c?.doc)
    throw new Error(
      `no page document: at ${l?.sectionIndex}, loaded ${reader()!
        .engine.view.renderer.getContents()
        .map((x) => `${x.index}:${x.doc?.body?.textContent?.trim().length ?? 'none'}`)
        .join(',')}`,
    )
  return c as { doc: Document; index: number; overlayer?: { element: SVGElement } }
}
/** Chapter-text offsets of a phrase in the current chapter (the same extractor as the reader). */
const offsetsOf = async (phrase: string) => {
  const { extractText } = await import('../lib/search/extract')
  const { doc, index } = pageDoc()
  const text = extractText(doc.body).text
  const start = text.indexOf(phrase)
  if (start < 0) throw new Error(`“${phrase}” is not in this chapter`)
  return { index, start, end: start + phrase.length }
}
/** Select a phrase as a reader does: show it, select it in the page, then mouse-up. */
const selectPhrase = async (phrase: string, opts: { from?: number } = {}) => {
  const { index, start, end } = await offsetsOf(phrase)
  await reader()!.engine.goToText(index, start + (opts.from ?? 0), end)
  await settled(500)
  const { doc } = pageDoc()
  const range = reader()!.engine.textRange(doc, start + (opts.from ?? 0), end)!
  const sel = doc.getSelection()!
  sel.removeAllRanges()
  sel.addRange(range)
  doc.body.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }))
  await settled(250)
  return range
}
const selBar = () => document.querySelector<HTMLElement>('.bar[role="toolbar"]')
const barButton = (label: RegExp) =>
  Array.from(selBar()?.querySelectorAll<HTMLButtonElement>('button') ?? []).find((b) =>
    label.test(b.getAttribute('aria-label') ?? b.textContent ?? ''),
  )
const pressBar = async (label: RegExp) => {
  const b = barButton(label)
  if (!b) throw new Error(`no ${label} in the selection bar`)
  b.click()
  await settled(300)
}
const stored = async () => {
  await hooks.writes!.idle()
  return invoke<{ id: string; color: string; note: string | null; cfi_range: string }[]>(
    'annotations_list',
    { bookId: reader()!.bookId },
  )
}
/**
 * How a highlight is drawn: its tint behind the text (a CSS custom highlight holding
 * exactly its range, or overlay rects on older WebKit) and its 2 px underline rects.
 */
const drawn = (id: string) => {
  const a = annotationsOf().get(id)
  const { doc } = pageDoc()
  const layer = pageDoc().overlayer?.element
  const vars = getComputedStyle(document.querySelector('.reader')!)
  // WebKit normalises custom properties (“.11” becomes “0.11”), so compare canonical forms.
  const canon = (c: string | null) =>
    (c ?? '')
      .replace(/\s/g, '')
      .replace(/([,(])0\./g, '$1.')
      .toLowerCase()
  const line = a ? canon(vars.getPropertyValue(`--hl-${a.color}-underline`)) : '-'
  const rects = Array.from(layer?.querySelectorAll<SVGRectElement>('rect') ?? [])
  const registry = (doc.defaultView as unknown as { CSS: { highlights?: Map<string, Set<Range>> } })
    .CSS.highlights
  const ranges = a ? [...(registry?.get(`linen-hl-${a.color}`) ?? [])] : []
  return {
    behindText: ranges.some((r) => r.toString() === a?.quote.exact),
    // On light themes the overlay tint multiplies with the page; Night's is translucent.
    overlayTints: rects.filter((r) => {
      const g = r.parentNode as SVGElement | null
      const light = !vars.getPropertyValue('--hl-yellow-tint').trim().startsWith('rgba')
      return (
        !!g?.hasAttribute('data-linen-tints') && (g.style.mixBlendMode === 'multiply') === light
      )
    }),
    lines: rects.filter(
      (r) => canon(r.getAttribute('fill')) === line && r.getAttribute('height') === '2',
    ),
  }
}
/** The first dozen characters on the page shown (its first line). */
const firstLineRange = () => {
  const { doc } = pageDoc()
  const visible = reader()!.engine.view.lastLocation!.range
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT)
  for (let n = walker.nextNode() as Text | null; n; n = walker.nextNode() as Text | null) {
    if (visible.comparePoint(n, 0) < 0 && !visible.intersectsNode(n)) continue
    const from = n === visible.startContainer ? visible.startOffset : 0
    const text = n.data.slice(from)
    const lead = text.length - text.trimStart().length
    if (text.trim().length < 12) continue
    const r = doc.createRange()
    r.setStart(n, from + lead)
    r.setEnd(n, from + lead + 12)
    return r
  }
  throw new Error('no text on the page')
}
/** A click at the middle of a range's first line, where foliate hit-tests annotations (A5). */
const clickOn = (range: Range) => {
  const r = Array.from(range.getClientRects()).find((x) => x.width > 4)!
  const doc = range.startContainer.ownerDocument!
  doc.getSelection()?.removeAllRanges()
  const at = { clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, bubbles: true }
  doc.body.dispatchEvent(new PointerEvent('pointerdown', at))
  doc.body.dispatchEvent(new PointerEvent('pointerup', at))
  doc.body.dispatchEvent(new MouseEvent('click', at))
}
const NOVEMBER = 'damp, drizzly November in my soul'
const MANHATTOES = 'There now is your insular city of the Manhattoes'
const noteCard = () => document.querySelector<HTMLElement>('.note[role="dialog"]')
const typeNote = (text: string) => {
  const field = noteCard()!.querySelector('textarea')!
  field.value = text
  field.dispatchEvent(new Event('input', { bubbles: true }))
}

type Check = { id: string; description: string; run: () => Promise<string> }

export async function spikeE2E(): Promise<SpikeResult> {
  // Import the test books before the app lists the library.
  const books = [
    'standardebooks-moby-dick.epub',
    'idpf-regime-anticancer-arabic.epub',
    'hostile-content.epub',
    'idpf-page-blanche.epub',
    'long-chapter.epub',
    'broken-bad-css-and-font.epub',
    // N6: NCX-only (EPUB 2), no navigation at all, and damaged chapters.
    'gutenberg-2701-moby-dick-epub2.epub',
    'broken-no-toc.epub',
    'broken-missing-items.epub',
    // N8: a print page list.
    'idpf-childrens-literature.epub',
    // N9–N11: footnote asides, a long note, an image with a caption, an external link.
    'notes-and-images.epub',
    // F2: diacritics and CJK (golden results from Spike F).
    'idpf-sous-le-vent.epub',
    'idpf-kusamakura-japanese-vertical-writing.epub',
    // L18: over 30% code and tables.
    'code-heavy.epub',
  ]
  const paths = await Promise.all(
    books.map((name) => invoke<string>('spike_corpus_path', { name })),
  )
  const imported = await invoke<{ path: string; outcome: { kind: string; reason?: string } }[]>(
    'library_import',
    { paths },
  )
  for (const r of imported)
    log(
      `import ${r.path.split('/').pop()}: ${r.outcome.kind}${r.outcome.reason ? ` (${r.outcome.reason})` : ''}`,
    )

  const { installThemeCss } = await import('../app/theme')
  await import('../app/base.css')
  installThemeCss()
  // As src/main.ts: uncaught errors reach the crash log (D1).
  ;(await import('../app/log')).installErrorLogging()
  document.getElementById('log')!.style.display = 'none'
  document.getElementById('chrome-top')!.style.display = 'none'
  const { default: App } = await import('../App.svelte')
  mount(App, { target: document.getElementById('reader')! })

  const turnBy = async (act: () => void, expect: 'next' | 'prev' | 'none') => {
    const before = loc()!
    act()
    await settled()
    const after = loc()!
    const moved = after.cfi !== before.cfi
    const forward = after.fraction > before.fraction || after.sectionIndex > before.sectionIndex
    if (expect === 'none') return moved ? `moved (${before.page} → ${after.page})` : 'ok'
    if (!moved) return `did not move`
    return (expect === 'next') === forward
      ? 'ok'
      : `moved the wrong way (${before.fraction.toFixed(4)} → ${after.fraction.toFixed(4)})`
  }

  const checks: Check[] = [
    {
      id: 'N3-bodymatter',
      description: 'A new book opens at the bodymatter landmark, not the cover',
      run: async () => {
        await openFromLibrary(/Moby Dick(?!;)/)
        const l = loc()!
        return /loomings/i.test(l.chapterLabel) || l.sectionIndex > 2
          ? 'ok'
          : `opened at “${l.chapterLabel}” (section ${l.sectionIndex})`
      },
    },
    {
      id: 'I8-keys-next',
      description: '→ turns to the next page',
      run: () => turnBy(() => key('ArrowRight'), 'next'),
    },
    {
      id: 'I8-keys-prev',
      description: '← turns to the previous page',
      run: () => turnBy(() => key('ArrowLeft'), 'prev'),
    },
    {
      id: 'I8-keys-app-focus',
      description: 'Page keys also work with focus on the app window',
      run: () => turnBy(() => keyOnApp('ArrowRight'), 'next'),
    },
    {
      id: 'I8-space',
      description: 'Space turns forward, ⇧Space back',
      run: async () => {
        const a = await turnBy(() => key(' '), 'next')
        const b = await turnBy(() => key(' ', { shiftKey: true }), 'prev')
        return a === 'ok' && b === 'ok' ? 'ok' : `${a}; ${b}`
      },
    },
    {
      id: 'I8-page-keys',
      description: 'PgDn / ↓ forward, PgUp / ↑ back',
      run: async () => {
        const r = [
          await turnBy(() => key('PageDown'), 'next'),
          await turnBy(() => key('ArrowDown'), 'next'),
          await turnBy(() => key('PageUp'), 'prev'),
          await turnBy(() => key('ArrowUp'), 'prev'),
        ]
        return r.every((x) => x === 'ok') ? 'ok' : r.join('; ')
      },
    },
    {
      id: 'I11-margins',
      description: 'Margin clicks turn pages (right = next, left = previous)',
      run: async () => {
        const a = await turnBy(() => click('.margin.right'), 'next')
        const b = await turnBy(() => click('.margin.left'), 'prev')
        return a === 'ok' && b === 'ok' ? 'ok' : `${a}; ${b}`
      },
    },
    {
      id: 'I11-activating-click',
      description: 'The click that activates the window never turns a page',
      run: async () => {
        await emit('native-mouse-down', { app_active: false, key_window: false, t: 0 })
        await sleep(20)
        return turnBy(() => click('.margin.right'), 'none')
      },
    },
    {
      id: 'I11-column-click',
      description: 'A click inside the text column never turns a page',
      run: async () => {
        await sleep(400)
        const doc = reader()!.engine.view.renderer.getContents()[0].doc
        return turnBy(
          () =>
            (doc.querySelector('p') ?? doc.body).dispatchEvent(
              new MouseEvent('click', { bubbles: true }),
            ),
          'none',
        )
      },
    },
    {
      id: 'I1-wheel-roll',
      description: 'One wheel roll (native, not precise) turns one page',
      run: async () => {
        const before = loc()!
        const t0 = performance.now()
        for (const [i, dy] of [-0.1, -0.9, -3.2].entries())
          await emit('native-scroll', {
            precise: false,
            phase: 0,
            momentum: 0,
            dx: 0,
            dy,
            x: 640,
            y: 400,
            t: t0 + i * 20,
          })
        await settled(600)
        const after = loc()!
        return after.cfi !== before.cfi && after.fraction > before.fraction
          ? 'ok'
          : 'did not turn exactly forward'
      },
    },
    {
      id: 'I2-I5-trackpad',
      description: 'One trackpad gesture turns one page; its momentum does not',
      run: async () => {
        const before = loc()!
        const t0 = performance.now() + 1000
        const ev = (p: Record<string, number | boolean>) =>
          emit('native-scroll', {
            precise: true,
            phase: 0,
            momentum: 0,
            dx: 0,
            dy: 0,
            x: 640,
            y: 400,
            ...p,
          })
        await ev({ phase: 1, dy: -2, t: t0 })
        for (const [i, dy] of [-10, -25, -30, -20].entries())
          await ev({ phase: 4, dy, t: t0 + 8 * (i + 1) })
        await ev({ phase: 8, t: t0 + 60 })
        for (let i = 0; i < 20; i++)
          await ev({ momentum: i === 0 ? 1 : 4, dy: -30 * 0.9 ** i, t: t0 + 80 + i * 16 })
        await ev({ momentum: 8, t: t0 + 500 })
        await settled(700)
        const after = loc()!
        const pages = (after.page ?? 0) - (before.page ?? 0)
        return after.sectionIndex === before.sectionIndex && pages === 1
          ? 'ok'
          : `moved ${pages} pages (section ${before.sectionIndex} → ${after.sectionIndex})`
      },
    },
    {
      id: 'I7-rapid-chip',
      description: 'More than 3 pages in under 1 s offers “Back to page N”',
      run: async () => {
        for (let i = 0; i < 5; i++) {
          key('ArrowRight')
          await sleep(130)
        }
        await settled(600)
        const text = hooks.messages?.current?.text ?? ''
        return /^Back to page ≈?\d+/.test(text) ? 'ok' : `message was “${text}”`
      },
    },
    {
      id: 'N1-back',
      description: 'Back (⌘[) returns to where the rapid turning started',
      run: async () => {
        const entry = reader()!.history.peek()
        if (!entry) return 'no history entry'
        key('[', { code: 'BracketLeft', metaKey: true })
        await settled(800)
        return loc()!.cfi === entry.cfi || reader()!.engine.view.lastLocation?.cfi === entry.cfi
          ? 'ok'
          : `at ${loc()!.cfi}, expected ${entry.cfi}`
      },
    },
    {
      id: 'progress-saved',
      description: 'The position is saved within the 1 s debounce',
      run: async () => {
        key('ArrowRight')
        await settled(1500)
        const l = loc()!
        const saved = await invoke<[string, number] | null>('position_get', {
          bookId: reader()!.bookId,
        })
        return saved?.[0] === l.cfi ? 'ok' : `saved ${saved?.[0]} vs current ${l.cfi}`
      },
    },
    {
      id: 'N5-quit-save',
      description: 'Quitting saves the position at once, inside the 1 s debounce',
      run: async () => {
        key('ArrowRight')
        await sleep(250)
        const l = loc()!
        await emit('app-quitting')
        await waitFor('quit handled', () => hooks.quitRequested, 3000)
        const saved = await invoke<[string, number] | null>('position_get', {
          bookId: reader()!.bookId,
        })
        return saved?.[0] === l.cfi ? 'ok' : `saved ${saved?.[0]} vs current ${l.cfi}`
      },
    },
    {
      id: 'N5-restore',
      description: 'Leaving for the library and reopening restores the same place',
      run: async () => {
        const before = where()
        const cfi = loc()!.cfi
        await backToLibrary()
        await openFromLibrary(/Moby Dick(?!;)/)
        return where() === before
          ? 'ok'
          : `was ${before} (${cfi}), reopened at ${where()} (${loc()!.cfi})`
      },
    },
    {
      id: 'L10-resize',
      description: 'Resizing the window keeps the reading position on screen',
      run: async () => {
        const w = getCurrentWindow()
        const size = await w.innerSize()
        const factor = await w.scaleFactor()
        const engine = reader()!.engine
        const l0 = engine.view.lastLocation
        if (!l0) return 'no location'
        const anchor = l0.range.cloneRange()
        anchor.collapse(true)
        const visible = () => {
          const r = engine.view.lastLocation?.range
          return (
            !!r &&
            r.startContainer.ownerDocument === anchor.startContainer.ownerDocument &&
            r.compareBoundaryPoints(Range.START_TO_START, anchor) <= 0 &&
            r.compareBoundaryPoints(Range.START_TO_END, anchor) >= 0
          )
        }
        await w.setSize(new LogicalSize(900, 700))
        await settled(1200)
        const narrow = visible()
        await w.setSize(new LogicalSize(size.width / factor, size.height / factor))
        await settled(1200)
        const wide = visible()
        return narrow && wide
          ? 'ok'
          : `anchor visible after narrowing: ${narrow}, after restoring: ${wide}`
      },
    },
    {
      id: 'I15-rtl-keys',
      description: 'In a right-to-left book, ← turns to the next page',
      run: async () => {
        await backToLibrary()
        await openFromLibrary(/[؀-ۿ]|anticancer|Régime|regime/i)
        if (!reader()!.engine.rtl) return 'book not reported as right-to-left'
        return turnBy(() => key('ArrowLeft'), 'next')
      },
    },
    {
      id: 'I15-rtl-margin',
      description: 'In a right-to-left book, the left margin moves forward',
      run: () => turnBy(() => click('.margin.left'), 'next'),
    },
  ]

  checks.push({
    id: 'I15-rtl-progress',
    description:
      'In a right-to-left book the progress bar fills from the right and its labels swap sides (G8)',
    run: async () => {
      await showControls()
      try {
        const track = document.querySelector('.track')!.getBoundingClientRect()
        const fill = document.querySelector('.fill')!.getBoundingClientRect()
        const [chapter, progress] = Array.from(document.querySelectorAll('.labels span')).map((e) =>
          e.getBoundingClientRect(),
        )
        if (Math.abs(fill.right - track.right) > 1)
          return `fill ${Math.round(fill.left)}–${Math.round(fill.right)} in track ${Math.round(track.left)}–${Math.round(track.right)}`
        return chapter.left > progress.left ? 'ok' : 'the chapter label is not on the right'
      } finally {
        await hideControls()
      }
    },
  })

  checks.push({
    id: 'V8-chrome-out',
    description: 'The controls leave with a 220 ms fade, not at once (V8)',
    run: async () => {
      await showControls()
      key('Escape', { code: 'Escape' })
      await sleep(80)
      const top = document.querySelector<HTMLElement>('.chrome.top')
      const midway = top ? Number(getComputedStyle(top).opacity) : 1
      await sleep(300)
      const gone = !document.querySelector('.chrome.top')
      if (!top || midway >= 0.95)
        return `80 ms after Esc the top bar was ${top ? `at opacity ${midway}` : 'already gone'}`
      return gone ? 'ok' : 'the top bar was still there after 380 ms'
    },
  })

  checks.push({
    id: 'S9-edge-reveal',
    description:
      'Immersive reading shows no location line. Either edge reveals both bars, the top one naming the book only; they stay while the pointer is in an edge zone or on a bar and go as soon as it leaves (S9)',
    run: async () => {
      await hideControls()
      if (document.querySelector('.location-line'))
        return `a location line showed: “${document.querySelector('.location-line')!.textContent}”`
      const area = document.querySelector('.reader')!
      const move = (y: number) =>
        area.dispatchEvent(
          new PointerEvent('pointermove', { clientX: 640, clientY: y, bubbles: true }),
        )
      const bars = () => [
        !!document.querySelector('.chrome.top'),
        !!document.querySelector('.chrome.bottom'),
      ]
      const bottom = window.innerHeight - 30 // inside the zone, above any Dock strip
      move(20)
      await sleep(600) // 150 ms dwell + 160 ms fade in
      if (bars().join() !== 'true,true') return `the top edge showed bars ${bars()}`
      const top = document.querySelector('.chrome.top')!
      const title = top.querySelector('.title')?.textContent?.trim()
      const book = top.querySelector('.title .book')?.textContent?.trim()
      if (title !== book) return `the top bar said “${title}”, not just “${book}”`
      for (const y of [30, 50, 60, 10, bottom]) move(y)
      await sleep(400)
      if (bars().join() !== 'true,true') return `bars ${bars()} while the pointer was on them`
      move(300)
      await sleep(400) // the 220 ms fade out, well short of the 3 s full-controls delay
      if (document.querySelector('.chrome')) return 'the bars stayed after the pointer left'
      move(bottom)
      await sleep(600)
      if (bars().join() !== 'true,true') return `the bottom edge showed bars ${bars()}`
      move(300)
      await sleep(400)
      return document.querySelector('.chrome') ? 'the bars stayed after leaving the bottom' : 'ok'
    },
  })

  checks.push({
    id: 'G8-opening-line',
    description: 'An open over 500 ms shows one “Opening …” line, which the page then replaces',
    run: async () => {
      await backToLibrary()
      hooks.openDelayMs = 900
      try {
        const row = await waitFor('row', () => libraryTile(/Moby Dick(?!;)/))
        row.click()
        await sleep(300)
        const early = document.querySelector('.location-line')?.textContent ?? ''
        await sleep(450)
        const late = document.querySelector('.location-line')?.textContent ?? ''
        await waitFor('reader location', () => loc()?.pages)
        await settled(300)
        const after = document.querySelector('.location-line')?.textContent ?? ''
        if (early) return `a line showed within 300 ms: “${early}”`
        if (!/^Opening “Moby Dick”…$/.test(late)) return `at 750 ms the line said “${late}”`
        if (/^Opening/.test(after)) return 'the opening line stayed after the page appeared'
        return 'ok'
      } finally {
        hooks.openDelayMs = 0
      }
    },
  })

  checks.push({
    id: 'L8-spread',
    description:
      'From 1480 px the page is a two-page spread (G8): two 640 px pages, a 48 px gutter, one turn per spread',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      const w = getCurrentWindow()
      const size = await w.innerSize()
      const factor = await w.scaleFactor()
      const engine = reader()!.engine
      await engine.goToTextStart() // Chapter 1: plain prose on both pages
      await w.setSize(new LogicalSize(1680, 1050))
      await settled(1500)
      try {
        // Where the pages sit on screen: each paragraph fragment is one column's box.
        const range = engine.view.lastLocation?.range
        if (!range) return 'no visible range'
        const doc = range.startContainer.ownerDocument!
        const dx = doc.defaultView!.frameElement!.getBoundingClientRect().left
        const view = engine.view.getBoundingClientRect()
        const boxes = new Map<number, number>()
        for (const p of doc.querySelectorAll('p'))
          for (const r of p.getClientRects()) {
            const left = Math.round(r.left + dx)
            // Fragments in this spread only (the frame holds the whole chapter).
            if (left < view.left || left >= view.right) continue
            // Each page's leftmost fragment is its column edge (indented blocks sit further in).
            const page = left < view.left + view.width / 2 ? 0 : 1
            const known = [...boxes.keys()].find(
              (x) => (x < view.left + view.width / 2 ? 0 : 1) === page,
            )
            if (known === undefined || left < known) {
              if (known !== undefined) boxes.delete(known)
              boxes.set(left, Math.round(r.width))
            }
          }
        const lefts = [...boxes.keys()].sort((a, b) => a - b)
        const geometry = lefts.map((x) => `${x}+${boxes.get(x)}`).join(', ')
        log(`L8 spread: pages at ${geometry}`)
        if (
          lefts.length !== 2 ||
          Math.abs(lefts[0] - 176) > 1 ||
          Math.abs(lefts[1] - (176 + 640 + 48)) > 1 ||
          lefts.some((x) => Math.abs(boxes.get(x)! - 640) > 1)
        )
          return `expected pages at 176+640 and 864+640; got ${geometry}`
        const before = range.cloneRange()
        await engine.turn('next')
        await settled(400)
        const after = engine.view.lastLocation!.range
        // The next spread starts after everything the previous one showed.
        // END_TO_START compares this range's start with the source range's end.
        if (after.compareBoundaryPoints(Range.END_TO_START, before) < 0)
          return 'the turn did not move past the previous spread'
        return 'ok'
      } finally {
        await w.setSize(new LogicalSize(size.width / factor, size.height / factor))
        await settled(1200)
      }
    },
  })

  checks.push({
    id: 'E2-fixed-layout',
    description: 'A fixed-layout book opens and turns pages (one view, scaled to fit)',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/blanche/i)
      const engine = reader()!.engine
      if (!engine.fixedLayout) return 'not detected as fixed layout'
      const views = document.querySelectorAll('foliate-view').length
      const before = loc()!.fraction
      key('ArrowRight')
      await settled(900)
      const after = loc()!.fraction
      if (after > before) return 'ok'
      // Try the engine directly, to tell key routing from page turning.
      const s0 = JSON.stringify(loc())
      await engine.turn('next')
      await settled(900)
      return `key did not advance (${before} → ${after}); ${views} views; engine.turn: ${s0.slice(0, 160)} → ${JSON.stringify(loc()).slice(0, 160)}; trail ${engine.debug().trail.slice(-4).join(' | ')}`
    },
  })

  checks.push({
    id: 'I17-fixed-zoom',
    description:
      'Fixed layout (G8): ⌘+ zooms (chip “150%”), the wheel pans instead of turning, → turns and returns to fit, ⌘0 fits; the line shows real pages',
    run: async () => {
      const engine = reader()!.engine
      const r = engine.view.renderer
      const zoom = () => engine.zoom
      // G12: the real pages are in the bottom bar (immersive reading has no location line).
      await showControls()
      const line = document.querySelector('.goto-label .rest')?.textContent?.trim() ?? ''
      await hideControls()
      if (!/^\d+% · Pages? \d+(–\d+)? of \d+$/.test(line)) return `progress label “${line}”`
      const cmd = (code: string, k: string) => keyOnApp(k, { code, metaKey: true })
      cmd('Equal', '=')
      await settled(300)
      const chip = document.querySelector('.zoom-chip span')?.textContent
      if (zoom() !== 1.5 || chip !== '150%') return `after ⌘+: zoom ${zoom()}, chip “${chip}”`
      cmd('Equal', '=')
      await settled(300)
      if (zoom() !== 2) return `after two ⌘+: zoom ${zoom()}`
      if (r.scrollWidth <= r.clientWidth && r.scrollHeight <= r.clientHeight)
        return 'the zoomed page does not overflow the view'
      // A wheel roll while zoomed pans (native scrolling), and must not turn the page.
      const before = loc()!.fixedPages?.join()
      const t0 = performance.now()
      for (const [i, dy] of [-0.1, -0.9, -3.2].entries())
        await emit('native-scroll', {
          precise: false,
          phase: 0,
          momentum: 0,
          dx: 0,
          dy,
          x: 640,
          y: 400,
          t: t0 + i * 20,
        })
      await settled(500)
      if (loc()!.fixedPages?.join() !== before) return 'a wheel roll turned the zoomed page'
      const x0 = r.scrollLeft
      const y0 = r.scrollTop
      engine.pan(60, 40)
      if (r.scrollLeft === x0 && r.scrollTop === y0) return 'panning did not move the page'
      key('ArrowRight')
      await settled(900)
      if (loc()!.fixedPages?.join() === before) return '→ did not turn the zoomed page'
      if (zoom() !== 1) return `after the turn zoom is ${zoom()}, not fit`
      cmd('Equal', '=')
      await settled(300)
      cmd('Digit0', '0')
      await settled(300)
      return zoom() === 1 ? 'ok' : `⌘0 left zoom at ${zoom()}`
    },
  })

  checks.push({
    id: 'L16-long-chapter',
    description:
      'A 1 MB+ chapter opens within the open-book budget (< 500 ms, click → first location) and shows “≈” pages',
    run: async () => {
      await backToLibrary()
      const row = await waitFor('row', () => libraryTile(/one file/i))
      const previous = reader()?.engine
      const t0 = performance.now()
      row.click()
      for (;;) {
        // The first laid-out page of the new book (not the previous reader, not a placeholder).
        if (reader() && reader()!.engine !== previous && loc()?.pages) break
        if (performance.now() - t0 > 10_000) return 'did not open within 10 s'
        await new Promise((r) => requestAnimationFrame(r))
      }
      const ms = Math.round(performance.now() - t0)
      const first = loc()!
      log(
        `L16 open: ${ms} ms; first location page ${first.page}/${first.pages}, approximate ${first.approximate}; views ${JSON.stringify(reader()!.engine.debug())}`,
      )
      await settled(800)
      if (ms >= 500) return `opened in ${ms} ms`
      if (!loc()!.approximate) return 'location not marked approximate'
      const before = loc()!
      key('ArrowRight')
      await settled(600)
      const said = document.querySelector('[role="status"][aria-live="polite"]')?.textContent ?? ''
      const after = loc()!
      return /^About page \d+$/.test(said)
        ? 'ok'
        : `turn announced as “${said}” (${before.page}/${before.pages} ${before.reason} → ${after.page}/${after.pages} ${after.reason}; message: ${hooks.messages?.current?.text})`
    },
  })

  checks.push({
    id: 'L16-chunk-traversal',
    description:
      'Paging through a chunked chapter crosses chunk boundaries forward and back with no skipped or repeated page',
    run: async () => {
      const { compare } = await import('foliate-js/epubcfi.js')
      const engine = reader()!.engine
      const chunk = () => engine.debug().chunks[0]
      const start = chunk()
      let crossings = 0
      let prev = loc()!
      for (let i = 0; i < 200 && crossings < 3; i++) {
        const before = chunk()
        await engine.turn('next')
        await settled(60)
        const l = loc()!
        if (compare(l.cfi, prev.cfi) <= 0)
          return `turn ${i}: location did not advance (${prev.cfi} → ${l.cfi})`
        if (l.fraction < prev.fraction)
          return `turn ${i}: fraction went back (${prev.fraction} → ${l.fraction})`
        if (chunk() !== before) {
          crossings++
          const [k, n] = before.split('/').map(Number)
          if (chunk() !== `${k + 1}/${n}`) return `crossed from ${before} to ${chunk()}`
          if (l.page !== 1) return `crossed into ${chunk()} at page ${l.page}, not 1`
          // Back across the boundary lands on the previous chunk's last page, then forward again.
          await engine.turn('prev')
          await settled(60)
          const b = loc()!
          if (chunk() !== before || b.page !== b.pages || compare(b.cfi, prev.cfi) !== 0)
            return `back across ${chunk()}: page ${b.page}/${b.pages}, ${b.cfi} vs ${prev.cfi}`
          await engine.turn('next')
          await settled(60)
          if (compare(loc()!.cfi, l.cfi) !== 0) return `forward again: ${loc()!.cfi} vs ${l.cfi}`
        }
        prev = loc()!
      }
      log(
        `L16 traversal: ${start} → ${chunk()}, ${crossings} crossings; ${engine.debug().trail.slice(-3).join(' | ')}`,
      )
      return crossings === 3 ? 'ok' : `only ${crossings} chunk crossings in 200 turns`
    },
  })

  checks.push({
    id: 'L16-deep-jump-restore',
    description:
      'A jump to a CFI deep in a chunked chapter shows it, and reopening the book restores it',
    run: async () => {
      const { compare } = await import('foliate-js/epubcfi.js')
      const engine = reader()!.engine
      const { doc, index } = engine.view.renderer.getContents()[0]
      const ps = doc.querySelectorAll('p')
      const target = ps[Math.floor(ps.length * 0.8)]
      const range = doc.createRange()
      range.selectNodeContents(target)
      range.collapse(true)
      const cfi = engine.view.getCFI(index, range)
      await engine.goTo(cfi)
      await settled(600)
      const shown = engine.view.lastLocation?.range
      const d = shown?.startContainer.ownerDocument
      if (d) {
        const resolved = engine.view.resolveNavigation(cfi) as {
          anchor: (doc: Document) => Range
        }
        const r = resolved.anchor(d)
        const onPage =
          shown!.compareBoundaryPoints(Range.START_TO_START, r) <= 0 &&
          shown!.compareBoundaryPoints(Range.START_TO_END, r) >= 0
        if (!onPage)
          return `jumped to ${loc()!.cfi}, target ${cfi} not on the page (${engine.debug().chunks[0]})`
      }
      const l = loc()!
      if (l.fraction < 0.6 || l.fraction > 0.95) return `fraction ${l.fraction} for a place 80% in`
      const saved = l.cfi
      await backToLibrary()
      await openFromLibrary(/one file/i)
      const again = loc()!
      if (compare(again.cfi, saved) === 0) return 'ok'
      const stored = await invoke<[string, number] | null>('position_get', {
        bookId: reader()!.bookId,
      })
      return `saved ${saved}, stored ${stored?.[0]}, reopened at ${again.cfi} (${reader()!.engine.debug().chunks[0]}); trail ${reader()!.engine.debug().trail.slice(-8).join(' | ')}`
    },
  })

  checks.push({
    id: 'X3-announcements',
    description: 'A page turn is announced briefly to screen readers (“Page N”)',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      key('ArrowRight')
      await settled(600)
      const text = document.querySelector('[role="status"][aria-live="polite"]')?.textContent ?? ''
      // “About page N” until the pages are counted (B1).
      return /^(About page|Page) \d+$/.test(text) ? 'ok' : `live region said “${text}”`
    },
  })

  checks.push({
    id: 'B1-pages-settle',
    description:
      'Page numbers are counted in idle time: “≈” until then, exact “Page N” after, consecutive across a turn',
    run: async () => {
      const t0 = performance.now()
      await waitFor('pages counted', () => reader()!.pagesExact(), 90_000)
      const took = Math.round(performance.now() - t0)
      const say = () =>
        document.querySelector('[role="status"][aria-live="polite"]')?.textContent ?? ''
      key('ArrowRight')
      await settled(600)
      const a = say()
      key('ArrowRight')
      await settled(600)
      const b = say()
      log(`B1: counted in ${took} ms after the check started; then “${a}”, “${b}”`)
      const [na, nb] = [a, b].map((x) => Number(/^Page (\d+)$/.exec(x)?.[1]))
      return na && nb === na + 1 ? 'ok' : `announced “${a}” then “${b}”`
    },
  })

  const scrollModeCheck = async (): Promise<string> => {
    const { compare } = await import('foliate-js/epubcfi.js')
    const engine = reader()!.engine
    await engine.goToTextStart()
    await settled(600)
    const before = loc()!
    if (!hooks.run?.('layout.scroll')) return 'the Scroll Mode command did not run'
    await settled(1500)
    if (engine.mode !== 'scroll') return `mode is ${engine.mode}`
    const host = engine.view.parentElement!
    const at = loc()!
    if (at.sectionIndex !== before.sectionIndex || Math.abs(at.fraction - before.fraction) > 0.005)
      return `switching moved the place: ${before.sectionIndex}/${before.fraction} → ${at.sectionIndex}/${at.fraction}`
    if (engine.debug().slots.length < 2) return `stack: ${JSON.stringify(engine.debug().slots)}`
    // Down through three chapter joins; the place only ever moves forward.
    let prev = loc()!
    let crossings = 0
    for (let i = 0; i < 600 && crossings < 3; i++) {
      host.scrollBy(0, 300)
      await sleep(40)
      const l = loc()!
      if (compare(l.cfi, prev.cfi) < 0) return `scrolling down went back: ${prev.cfi} → ${l.cfi}`
      if (l.sectionIndex !== prev.sectionIndex) {
        crossings++
        if (!document.querySelector('.linen-join')) return 'no join between chapters'
      }
      prev = l
    }
    if (crossings < 3) return `only ${crossings} chapter crossings`
    const views = engine.debug().slots.length
    if (views > 3) return `${views} stacked views`
    // I9: Space moves a screen less the fades and two lines; ↓ moves three lines.
    await settled(300)
    const lineHeight = 19 * 1.55
    let y0 = host.scrollTop
    key(' ')
    await settled(300)
    const screen = host.scrollTop - y0
    const wantScreen = host.clientHeight - 40 - 88 - 2 * lineHeight
    if (Math.abs(screen - wantScreen) > 2)
      return `Space scrolled ${screen} px, expected ${Math.round(wantScreen)}`
    y0 = host.scrollTop
    key('ArrowDown')
    await settled(300)
    const lines = host.scrollTop - y0
    if (Math.abs(lines - 3 * lineHeight) > 2)
      return `↓ scrolled ${lines} px, expected ${Math.round(3 * lineHeight)}`
    // Back up across a join; the place only moves backward.
    prev = loc()!
    const upFrom = prev.sectionIndex
    for (let i = 0; i < 400 && loc()!.sectionIndex === upFrom; i++) {
      host.scrollBy(0, -300)
      await sleep(40)
      const l = loc()!
      if (compare(l.cfi, prev.cfi) > 0) return `scrolling up went forward: ${prev.cfi} → ${l.cfi}`
      prev = l
    }
    if (loc()!.sectionIndex >= upFrom) return 'scrolling up did not reach the previous chapter'
    // Remembered per book (S13), and the place survives reopening.
    await settled(1200)
    const saved = loc()!
    await backToLibrary()
    await openFromLibrary(/Moby Dick(?!;)/)
    await settled(800)
    const reopened = reader()!.engine
    if (reopened.mode !== 'scroll') return 'reopened in Pages; the mode was not remembered'
    if (
      loc()!.sectionIndex !== saved.sectionIndex ||
      Math.abs(loc()!.fraction - saved.fraction) > 0.003
    )
      return `reopened at ${loc()!.sectionIndex}/${loc()!.fraction}, saved ${saved.sectionIndex}/${saved.fraction}`
    // The restore survives relayouts that land while the book is still opening
    // (Phase 7 found it 2% early after one extra await in the opening sequence).
    for (const ms of [0, 40, 120, 300, 700]) {
      await backToLibrary()
      hooks.relayoutDuringOpenMs = [ms, ms + 60]
      try {
        await openFromLibrary(/Moby Dick(?!;)/)
      } finally {
        hooks.relayoutDuringOpenMs = undefined
      }
      await settled(1200)
      if (
        loc()!.sectionIndex !== saved.sectionIndex ||
        Math.abs(loc()!.fraction - saved.fraction) > 0.003
      )
        return `with a relayout at ${ms} ms, reopened at ${loc()!.sectionIndex}/${loc()!.fraction}, saved ${saved.sectionIndex}/${saved.fraction}`
    }
    // And back to Pages at the same place.
    const s0 = loc()!
    hooks.run?.('layout.pages')
    await settled(1500)
    const mode = () => reader()!.engine.mode // read again: the command changed it
    if (mode() !== 'pages') return 'did not return to Pages'
    if (loc()!.sectionIndex !== s0.sectionIndex || Math.abs(loc()!.fraction - s0.fraction) > 0.01)
      return `Pages moved the place: ${s0.sectionIndex}/${s0.fraction} → ${loc()!.sectionIndex}/${loc()!.fraction}`
    return 'ok'
  }

  checks.push({
    id: 'B8-scroll-mode',
    description:
      'Scroll mode (B8, G8): keeps the place, scrolls continuously across chapters (with the join), Space = a screen, ↓ = 3 lines, remembered per book',
    run: async () => {
      try {
        return await scrollModeCheck()
      } finally {
        // Never leave the book in Scroll mode for the checks after this one.
        if (reader()?.engine.mode === 'scroll') {
          hooks.run?.('layout.pages')
          await settled(1200)
        }
      }
    },
  })

  checks.push({
    id: 'real-wheel',
    description:
      'Real macOS scroll events (posted to this app): a wheel notch turns one page in Pages; wheel and trackpad scroll the text in Scroll mode',
    run: async () => {
      const engine = reader()!.engine
      const wheel = (delta: number, count: number, pixels: boolean) =>
        invoke('spike_scroll_wheel', { x: 640, y: 400, delta, count, pixels })
      await settled(1500) // past any wheel cooldown
      const seen: string[] = []
      const { listen } = await import('@tauri-apps/api/event')
      const unlisten = await listen<{ x: number; y: number; dy: number }>('native-scroll', (e) =>
        seen.push(`${Math.round(e.payload.x)},${Math.round(e.payload.y)} dy ${e.payload.dy}`),
      )
      const before = where()
      await wheel(-1, 1, false)
      await settled(700)
      unlisten()
      const pageTurned = where() !== before
      if (!pageTurned)
        return `a wheel notch in Pages did not turn (${before}); AppKit delivered ${seen.length ? seen.join(' | ') : 'no scroll event'}`
      hooks.run?.('layout.scroll')
      await settled(1500)
      const host = engine.view.parentElement!
      let y0 = host.scrollTop
      await wheel(-3, 6, false)
      await settled(800)
      const byWheel = host.scrollTop - y0
      y0 = host.scrollTop
      await wheel(-12, 20, true)
      await settled(800)
      const byTrackpad = host.scrollTop - y0
      hooks.run?.('layout.pages')
      await settled(1200)
      log(
        `real wheel: Scroll mode moved ${Math.round(byWheel)} px by wheel, ${Math.round(byTrackpad)} px by trackpad`,
      )
      return byWheel > 0 && byTrackpad > 0
        ? 'ok'
        : `Scroll mode did not scroll: wheel ${byWheel} px, trackpad ${byTrackpad} px`
    },
  })

  checks.push({
    id: 'L15-font-fallback',
    description: 'A book font that fails to load falls back to Literata without a prompt',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Bad CSS and fonts/)
      await settled(2200) // the 1.5 s font timeout
      const doc = reader()!.engine.view.renderer.getContents()[0].doc
      const faces: string[] = []
      doc.fonts.forEach((f) => faces.push(`${f.family}:${f.status}`))
      const replaced = faces.some((f) => /Broken.*loaded/.test(f))
      return replaced ? 'ok' : `faces: ${faces.join(', ')}`
    },
  })

  /** The Navigator's rows, once Contents has loaded. */
  const navRows = () =>
    waitFor('contents', () => {
      const rows = Array.from(document.querySelectorAll<HTMLButtonElement>('.navigator .row'))
      return rows.length ? rows : null
    })
  const stageLeft = () => parseFloat(getComputedStyle(document.querySelector('.stage')!).left)

  checks.push({
    id: 'N6-contents-nav',
    description:
      'Contents (⌘T) docks from 1100 px, marks “You are here” on the current chapter with focus there, jumps with Back, and the column recentres',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      await reader()!.engine.goTo('chapter-3.xhtml')
      await settled(800)
      const here = loc()!
      if (!hooks.run?.('navigator.contents')) return 'the Contents command did not run'
      const rows = await navRows()
      await settled(500)
      if (document.querySelector('.navigator.floating')) return 'floated at a wide window'
      if (rows.length < 100) return `${rows.length} rows`
      const current = document.querySelector<HTMLButtonElement>('.navigator .row.current')
      if (!current || !/You are here/.test(current.textContent ?? ''))
        return 'no “You are here” row'
      if (!current.textContent!.includes(here.chapterLabel))
        return `“You are here” on “${current.textContent}”, reading “${here.chapterLabel}”`
      if (document.activeElement !== current) return 'focus is not on the current chapter'
      if (stageLeft() !== 320) return `reading area starts at ${stageLeft()} px`
      // The column is centred in what is left (Screen 04: x = 480 at 1280 px).
      // The leftmost line on the page shown (first lines are indented).
      const shown = reader()!.engine.view.lastLocation!.range
      const frame =
        shown.startContainer.ownerDocument!.defaultView!.frameElement!.getBoundingClientRect()
      const lefts = Array.from(shown.getClientRects())
        .filter((r) => r.width > 40)
        .map((r) => r.left)
      const textLeft = Math.round(frame.left + Math.min(...lefts))
      const want = Math.round(320 + (innerWidth - 320 - 640) / 2)
      const target = rows[rows.indexOf(current) + 3]
      target.click()
      await settled(900)
      if (loc()!.sectionIndex <= here.sectionIndex) return 'the jump did not move forward'
      if (!reader()!.history.canGoBack) return 'the jump is not in Back history (N1)'
      return Math.abs(textLeft - want) <= 1 ? 'ok' : `text at x = ${textLeft}, expected ${want}`
    },
  })

  checks.push({
    id: 'S2-esc-navigator',
    description:
      'Esc closes the docked Navigator, the reading area returns to full width, focus to the text',
    run: async () => {
      key('Escape', { code: 'Escape' })
      await settled(600)
      if (document.querySelector('.navigator')) return 'still open'
      return stageLeft() === 0 ? 'ok' : `reading area at ${stageLeft()} px`
    },
  })

  checks.push({
    id: 'S13-dock-remembered',
    description: 'A docked Navigator is remembered per book and reopens with it',
    run: async () => {
      hooks.run?.('navigator.contents')
      await navRows()
      await settled(600)
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      await settled(600)
      const open = !!document.querySelector('.navigator:not(.floating)')
      key('Escape', { code: 'Escape' })
      await settled(600)
      return open ? 'ok' : 'reopened without the Navigator'
    },
  })

  checks.push({
    id: 'S2-navigator-floats',
    description: 'Below 1100 px the Navigator floats over the text and leaves the layout alone',
    run: async () => {
      const w = getCurrentWindow()
      const size = await w.innerSize()
      const factor = await w.scaleFactor()
      await w.setSize(new LogicalSize(1000, 760))
      await settled(1200)
      try {
        hooks.run?.('navigator.contents')
        await navRows()
        await settled(400)
        const floating = !!document.querySelector('.navigator.floating')
        const left = stageLeft()
        key('Escape', { code: 'Escape' })
        await settled(400)
        if (!floating) return 'docked below 1100 px'
        if (left !== 0) return `reading area moved to ${left} px`
        return document.querySelector('.navigator') ? 'Esc did not close it' : 'ok'
      } finally {
        await w.setSize(new LogicalSize(size.width / factor, size.height / factor))
        await settled(1200)
      }
    },
  })

  const contentsOf = async (title: RegExp) => {
    await backToLibrary()
    await openFromLibrary(title)
    hooks.run?.('navigator.contents')
    const rows = await navRows()
    const note = document.querySelector('.navigator .note')?.textContent ?? ''
    const damaged = document.querySelectorAll('.navigator .damaged').length
    key('Escape', { code: 'Escape' })
    await settled(500)
    return { rows: rows.length, note, damaged }
  }

  checks.push({
    id: 'N6-contents-ncx',
    description: 'An EPUB 2 book with only an NCX lists its contents',
    run: async () => {
      const c = await contentsOf(/Moby Dick; Or/)
      return c.rows > 100 && !c.note ? 'ok' : `${c.rows} rows, note “${c.note}”`
    },
  })

  checks.push({
    id: 'N6-contents-headings',
    description:
      'A book without navigation gets Contents from its headings, labelled “Generated from headings”',
    run: async () => {
      const c = await contentsOf(/No table of contents/)
      return c.rows > 0 && c.note === 'Generated from headings'
        ? 'ok'
        : `${c.rows} rows, note “${c.note}”`
    },
  })

  checks.push({
    id: 'N6-contents-damaged',
    description: 'Damaged chapters are marked in Contents (E3)',
    run: async () => {
      const c = await contentsOf(/Missing items/)
      return c.damaged === 2 ? 'ok' : `${c.damaged} rows marked damaged of ${c.rows}`
    },
  })

  /** Type into a field the way a person does, so Svelte's bindings see it. */
  const typeInto = (el: HTMLInputElement | HTMLSelectElement, value: string) => {
    el.value = value
    el.dispatchEvent(
      new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }),
    )
  }
  const openGoTo = async () => {
    keyOnApp('j', { code: 'KeyJ', metaKey: true })
    return waitFor('Go to', () => document.querySelector<HTMLElement>('.goto'))
  }
  const submitGoTo = async () => {
    document.querySelector<HTMLFormElement>('.goto form')!.requestSubmit()
    await settled(900)
  }
  const back = async () => {
    keyOnApp('[', { code: 'BracketLeft', metaKey: true })
    await settled(900)
  }

  checks.push({
    id: 'N8-goto-percent',
    description:
      'Go to (⌘J) by percent: the preview names the chapter, Go lands there, and Back (⌘[) returns',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      await settled(600)
      const start = loc()!
      const pop = await openGoTo()
      await settled(200)
      const input = pop.querySelector<HTMLInputElement>('input')!
      if (document.activeElement !== input) return 'focus is not in the field'
      typeInto(input, '31')
      await settled(100)
      const preview = pop.querySelector('.preview')?.textContent ?? ''
      if (!/^31% is in .+\. Your current place stays in Back history/.test(preview))
        return `preview “${preview}”`
      await submitGoTo()
      if (document.querySelector('.goto')) return 'Go to stayed open'
      const at = loc()!
      if (Math.abs(at.fraction - 0.31) > 0.01) return `landed at ${Math.round(at.fraction * 100)}%`
      if (!preview.includes(at.chapterLabel))
        return `preview named another chapter than “${at.chapterLabel}”`
      await back()
      return loc()!.cfi === start.cfi ? 'ok' : 'Back did not return to the start'
    },
  })

  checks.push({
    id: 'N8-goto-chapter',
    description: 'Go to by chapter lands at the start of that chapter',
    run: async () => {
      const pop = await openGoTo()
      pop.querySelectorAll<HTMLButtonElement>('.segmented button')[1].click()
      await settled(200)
      const select = pop.querySelector<HTMLSelectElement>('select')!
      const option = select.options[20]
      typeInto(select, option.value)
      await submitGoTo()
      return loc()!.chapterLabel === option.textContent!.trim()
        ? 'ok'
        : `landed in “${loc()!.chapterLabel}”, chose “${option.textContent}”`
    },
  })

  checks.push({
    id: 'S2-esc-order',
    description:
      'With the Navigator docked and Go to open, Esc closes Go to first, then the Navigator (S2)',
    run: async () => {
      hooks.run?.('navigator.contents')
      await navRows()
      await settled(400)
      await openGoTo()
      key('Escape', { code: 'Escape' })
      await settled(300)
      const first = !document.querySelector('.goto') && !!document.querySelector('.navigator')
      key('Escape', { code: 'Escape' })
      await settled(500)
      const second = !document.querySelector('.navigator')
      return first && second
        ? 'ok'
        : `after one Esc: Go to closed ${first}; after two: Navigator closed ${second}`
    },
  })

  checks.push({
    id: 'N7-scrubber',
    description:
      'Dragging the progress track previews chapter and %, and releasing jumps there (Back returns)',
    run: async () => {
      await showControls()
      const start = loc()!
      const track = document.querySelector<HTMLElement>('.track')!
      const r = track.getBoundingClientRect()
      const at = (f: number) => ({
        clientX: r.left + r.width * f,
        clientY: r.top + 2,
        bubbles: true,
        pointerId: 1,
      })
      track.dispatchEvent(new PointerEvent('pointerdown', at(0.3)))
      track.dispatchEvent(new PointerEvent('pointermove', at(0.5)))
      await settled(100)
      const tip = document.querySelector('.scrub-tip')?.textContent?.trim() ?? ''
      track.dispatchEvent(new PointerEvent('pointerup', at(0.5)))
      await settled(900)
      const f = loc()!.fraction
      await hideControls()
      if (!/ · 50%$/.test(tip)) return `tooltip “${tip}”`
      if (Math.abs(f - 0.5) > 0.01) return `landed at ${Math.round(f * 100)}%`
      await back()
      return loc()!.cfi === start.cfi ? 'ok' : 'Back did not return'
    },
  })

  checks.push({
    id: 'N8-goto-page',
    description: 'Go to by print page (only in books with a page list) lands on that page',
    run: async () => {
      let pop = await openGoTo()
      const withoutList = pop.querySelectorAll('.segmented button').length
      key('Escape', { code: 'Escape' })
      await settled(300)
      if (withoutList !== 2) return `Moby Dick offered ${withoutList} modes`
      await backToLibrary()
      await openFromLibrary(/Children's Literature/)
      pop = await openGoTo()
      const modes = pop.querySelectorAll<HTMLButtonElement>('.segmented button')
      if (modes.length !== 3) return `${modes.length} modes with a page list`
      modes[2].click()
      await settled(200)
      const input = pop.querySelector<HTMLInputElement>('input')!
      typeInto(input, '9999')
      await settled(100)
      const none = pop.querySelector('.preview')?.textContent ?? ''
      typeInto(input, '175')
      await settled(100)
      const preview = pop.querySelector('.preview')?.textContent ?? ''
      await submitGoTo()
      // The page's anchor is on the page now shown.
      const target = reader()!.engine.pageList.find((p) => p.label === '175')!
      const shown = reader()!.engine.view.lastLocation?.range
      const { index, anchor } = reader()!.engine.view.resolveNavigation(target.href) as {
        index: number
        anchor: (d: Document) => Range | Element
      }
      const doc = shown?.startContainer.ownerDocument
      const a = doc ? anchor(doc) : null
      const node = a && 'startContainer' in a ? a.startContainer : a
      const range = doc?.createRange()
      if (node && range) range.selectNode(node as Node)
      const onPage =
        !!shown &&
        !!range &&
        loc()!.sectionIndex === index &&
        shown.compareBoundaryPoints(Range.START_TO_END, range) >= 0 &&
        shown.compareBoundaryPoints(Range.END_TO_START, range) <= 0
      if (!/There is no page 9999/.test(none)) return `unknown page preview “${none}”`
      if (!/^Page 175 is in /.test(preview)) return `preview “${preview}”`
      return onPage
        ? 'ok'
        : `page 175 not on the page shown (section ${loc()!.sectionIndex}, target ${index})`
    },
  })

  /** Click something in the book, as the reader does. */
  const clickInBook = (el: Element) =>
    el.dispatchEvent(
      new MouseEvent('click', {
        bubbles: true,
        cancelable: true,
        view: el.ownerDocument.defaultView,
      }),
    )
  const liveText = () =>
    document.querySelector('[role="status"][aria-live="polite"]')?.textContent ?? ''

  checks.push({
    id: 'N9-footnote-peek',
    description:
      'Footnote peek: never navigates; below the marker or above near the page foot, never over its line; scrolls up to 50%; marker keeps focus styling; Tab in; Esc returns focus; asides leave the flow',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Notes and images/)
      await reader()!.engine.goTo(0)
      await settled(800)
      const doc = reader()!.engine.view.renderer.getContents()[0].doc
      const asides = Array.from(doc.querySelectorAll('aside'))
      if (!asides.length || asides.some((a) => getComputedStyle(a).display !== 'none'))
        return 'referenced footnote asides are still in the flow'
      const shown = reader()!.engine.view.lastLocation!.range
      const markers = Array.from(doc.querySelectorAll('a[role="doc-noteref"]')).filter((a) =>
        shown.intersectsNode(a),
      ) as HTMLElement[]
      if (markers.length < 3) return `${markers.length} markers on the first page`
      let above = 0
      for (const m of markers) {
        const before = loc()!.cfi
        clickInBook(m)
        const peek = await waitFor('peek', () => document.querySelector<HTMLElement>('.peek'))
        await settled(250)
        const frame = doc.defaultView!.frameElement!.getBoundingClientRect()
        const mr = m.getBoundingClientRect()
        const line = { top: frame.top + mr.top, bottom: frame.top + mr.bottom }
        const pr = peek.getBoundingClientRect()
        if (!(pr.top >= line.bottom || pr.bottom <= line.top))
          return `peek for ${m.textContent} covers the marker's line`
        if (pr.bottom <= line.top) above++
        if (loc()!.cfi !== before)
          return `the peek navigated: ${before} → ${loc()!.cfi} (${loc()!.reason}, page ${loc()!.page}); scroll ${doc.scrollingElement?.scrollLeft}/${doc.scrollingElement?.scrollTop}`
        if (!m.classList.contains('linen-peek-marker')) return 'the marker lost its focus styling'
        const body = peek.querySelector<HTMLElement>('.body')!
        if (m.textContent === '2') {
          if (body.scrollHeight <= body.clientHeight) return 'the long note does not scroll'
          if (body.clientHeight > innerHeight * 0.5 + 1) return 'the long note is taller than 50%'
        } else if (!body.textContent!.includes(`Footnote ${m.textContent}:`))
          return `peek ${m.textContent} shows “${body.textContent}”`
        key('Escape', { code: 'Escape' })
        await settled(250)
        if (document.querySelector('.peek')) return 'Esc did not close the peek'
        if (doc.activeElement !== m || m.classList.contains('linen-peek-marker'))
          return 'focus did not return to the marker'
      }
      if (!above) return 'no marker near the page foot put its peek above'
      // Tab from the marker moves into the peek.
      clickInBook(markers[0])
      await waitFor('peek', () => document.querySelector('.peek'))
      key('Tab', { code: 'Tab' })
      await settled(100)
      const inPeek = !!document.activeElement?.closest('.peek')
      key('Escape', { code: 'Escape' })
      await settled(250)
      return inPeek ? 'ok' : 'Tab did not move into the peek'
    },
  })

  checks.push({
    id: 'N9-peek-actions',
    description:
      'Copy copies the note; Open note in place goes to the note (shown) and Back returns',
    run: async () => {
      const doc = reader()!.engine.view.renderer.getContents()[0].doc
      const m = doc.querySelector<HTMLElement>('#ref-1')!
      const start = loc()!.cfi
      // Copy writes the real pasteboard: keep what was on it and put it back.
      const kept = await invoke<string>('spike_read_pasteboard')
      clickInBook(m)
      const peek = await waitFor('peek', () => document.querySelector<HTMLElement>('.peek'))
      peek.querySelector<HTMLButtonElement>('.copy')!.click()
      await settled(300)
      if (liveText() !== 'Note copied') return `announced “${liveText()}”`
      const pasted = await invoke<string>('spike_read_pasteboard')
      if (kept) await invoke('copy_text', { text: kept })
      if (pasted !== 'Footnote 1: a short note about paragraph 1.') return `copied “${pasted}”`
      clickInBook(m)
      const again = await waitFor('peek', () => document.querySelector<HTMLElement>('.peek'))
      again.querySelector<HTMLButtonElement>('.open')!.click()
      await settled(900)
      const note = reader()!.engine.view.renderer.getContents()[0].doc.getElementById('fn-1')!
      if (getComputedStyle(note).display === 'none') return 'the note is still hidden'
      const range = reader()!.engine.view.lastLocation!.range
      if (!range.intersectsNode(note)) return 'the note is not on the page shown'
      await back()
      return loc()!.cfi === start ? 'ok' : 'Back did not return to the reference'
    },
  })

  checks.push({
    id: 'N9-endnote-peek',
    description: 'A reference to an endnote in another chapter peeks at it without navigating',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      const sections = reader()!.engine.book!.sections
      await reader()!.engine.goTo(sections.findIndex((x) => x.id.endsWith('/chapter-109.xhtml')))
      await settled(900)
      const doc = reader()!.engine.view.renderer.getContents()[0].doc
      const m = doc.querySelector<HTMLElement>('#noteref-21')
      if (!m)
        return `no #noteref-21 in section ${loc()!.sectionIndex} (${doc.querySelectorAll('[role="doc-noteref"]').length} refs)`
      const before = loc()!
      clickInBook(m)
      const peek = await waitFor('peek', () => document.querySelector<HTMLElement>('.peek'))
      await settled(300)
      const text = peek.querySelector('.body')?.textContent?.trim() ?? ''
      key('Escape', { code: 'Escape' })
      await settled(250)
      if (loc()!.sectionIndex !== before.sectionIndex) return 'the peek navigated'
      return text.length > 20 ? 'ok' : `peek text “${text}”`
    },
  })

  checks.push({
    id: 'N11-image-view',
    description:
      'Clicking an image opens the image view with its caption; ⌘+ zooms; Esc closes and focus returns to the image',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Notes and images/)
      await reader()!.engine.goTo(1)
      await settled(900)
      const doc = reader()!.engine.view.renderer.getContents()[0].doc
      const img = doc.querySelector('img')!
      clickInBook(img)
      const view = await waitFor('image view', () =>
        document.querySelector<HTMLElement>('.image-view'),
      )
      await settled(300)
      const caption = view.querySelector('.caption')?.textContent ?? ''
      const shown = view.querySelector<HTMLImageElement>('img')!
      const loaded = shown.complete && shown.naturalWidth === 320
      keyOnApp('=', { code: 'Equal', metaKey: true })
      await settled(200)
      const zoomLabel = view.querySelector('.fit')?.textContent ?? ''
      key('Escape', { code: 'Escape' })
      await settled(300)
      if (!loaded) return 'the image did not load in the view'
      if (caption !== 'Plate 1. A test image with its caption.') return `caption “${caption}”`
      if (zoomLabel !== '150%') return `zoom label “${zoomLabel}”`
      if (document.querySelector('.image-view')) return 'Esc did not close it'
      return doc.activeElement === img ? 'ok' : 'focus did not return to the image'
    },
  })

  checks.push({
    id: 'N10-external-links',
    description: 'External links show their URL on hover; only http(s) and mailto can be opened',
    run: async () => {
      const doc = reader()!.engine.view.renderer.getContents()[0].doc
      const a = doc.querySelector('a[href^="https:"]')!
      a.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))
      await settled(100)
      const shown = document.querySelector('.link-url')?.textContent ?? ''
      a.dispatchEvent(new MouseEvent('mouseout', { bubbles: true }))
      await settled(100)
      if (shown !== 'https://example.org/linen-test') return `hover showed “${shown}”`
      if (document.querySelector('.link-url')) return 'the URL stayed after the pointer left'
      const refused: string[] = []
      for (const url of ['file:///etc/hosts', 'javascript:alert(1)', 'x-apple-reminder://x'])
        await invoke('open_external', { url }).then(
          () => refused.push(`OPENED ${url}`),
          () => refused.push('refused'),
        )
      return refused.every((r) => r === 'refused') ? 'ok' : refused.join(', ')
    },
  })

  const palette = () => document.querySelector<HTMLElement>('dialog[open] .palette')
  /** Esc as a real key arrives: at the focused element, inside the modal. */
  const escModal = async () => {
    ;(document.activeElement ?? document.body).dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'Escape',
        code: 'Escape',
        bubbles: true,
        cancelable: true,
      }),
    )
    await settled(300)
  }
  const typePalette = async (q: string) => {
    typeInto(palette()!.querySelector('input')!, q)
    await settled(150)
  }
  const pressInPalette = (k: string) =>
    palette()!
      .querySelector('input')!
      .dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }))

  checks.push({
    id: 'K9-palette',
    description:
      '⌘K: fuzzy match, ↑ ↓ ↵ run, chapters by name, Recently closed, and it suspends reader input (S8)',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      await reader()!.engine.goToTextStart()
      await settled(600)
      keyOnApp('k', { code: 'KeyK', metaKey: true })
      await waitFor('palette', palette)
      await settled(200)
      if (document.activeElement !== palette()!.querySelector('input'))
        return 'focus is not in the field'
      // S8: the wheel does not turn pages behind a modal.
      const before = where()
      const t0 = performance.now()
      for (const [i, dy] of [-0.1, -0.9, -3.2].entries())
        await emit('native-scroll', {
          precise: false,
          phase: 0,
          momentum: 0,
          dx: 0,
          dy,
          x: 640,
          y: 400,
          t: t0 + i * 20,
        })
      await settled(500)
      if (where() !== before) return 'the page turned behind ⌘K'
      await typePalette('gtc')
      const first = palette()!.querySelector('.row.selected')?.textContent?.trim()
      if (!first?.startsWith('Go to chapter')) return `“gtc” selected “${first}”`
      pressInPalette('ArrowDown')
      pressInPalette('ArrowUp')
      pressInPalette('Enter')
      await settled(700)
      if (palette()) return 'Enter did not close the palette'
      if (!document.querySelector('.navigator')) return 'Enter did not run Go to chapter…'
      key('Escape', { code: 'Escape' })
      await settled(500)
      // A chapter by name.
      keyOnApp('k', { code: 'KeyK', metaKey: true })
      await waitFor('palette', palette)
      await typePalette('carpet bag')
      const row = palette()!.querySelector('.row.selected')?.textContent?.trim() ?? ''
      pressInPalette('Enter')
      await settled(900)
      if (!/Carpetbag/.test(loc()!.chapterLabel))
        return `went to “${loc()!.chapterLabel}” from “${row}”`
      // Recently closed: the Back chip from that jump, dismissed, is still reachable.
      const m = hooks.messages!.current
      if (!m || !/Back/.test(m.text)) return `no Back chip after the jump (“${m?.text}”)`
      hooks.messages!.dismiss(m.id)
      keyOnApp('k', { code: 'KeyK', metaKey: true })
      await waitFor('palette', palette)
      const sections = Array.from(palette()!.querySelectorAll('.section')).map((x) => x.textContent)
      const closed = palette()!.querySelector('.row')?.textContent ?? ''
      pressInPalette('Enter')
      await settled(900)
      if (sections[0] !== 'Recently closed') return `sections ${sections.join(', ')}`
      if (!/Back to page/.test(closed)) return `first row “${closed}”`
      return /Loomings/.test(loc()!.chapterLabel) ? 'ok' : `Back went to “${loc()!.chapterLabel}”`
    },
  })

  checks.push({
    id: 'K9-cheat-sheet',
    description: '? (and ? in ⌘K) opens the cheat sheet, which lists every working shortcut',
    run: async () => {
      key('?', { code: 'Slash', shiftKey: true })
      const sheet = await waitFor('cheat sheet', () =>
        document.querySelector<HTMLElement>('dialog[open] .sheet'),
      )
      await settled(200)
      const shown = new Set(Array.from(sheet.querySelectorAll('kbd')).map((k) => k.textContent))
      const { chordLabel } = await import('../lib/commands/keys')
      const missing = hooks
        .registry!.available()
        .flatMap((c) =>
          [
            c.chord,
            ...(c.altChords ?? []).filter((a) => a.code !== c.chord?.code),
            c.singleKey,
          ].filter((k) => !!k),
        )
        .map((k) => chordLabel(k!))
        .filter((l) => !shown.has(l))
      await escModal()
      if (document.querySelector('dialog[open]')) return 'Esc did not close the cheat sheet'
      if (missing.length) return `not listed: ${missing.join(' ')}`
      keyOnApp('k', { code: 'KeyK', metaKey: true })
      await waitFor('palette', palette)
      pressInPalette('?')
      const again = await waitFor('cheat sheet from ⌘K', () =>
        document.querySelector('dialog[open] .sheet'),
      )
      await escModal()
      return again ? 'ok' : 'no cheat sheet from ⌘K'
    },
  })

  checks.push({
    id: 'K-commands-reachable',
    description:
      'Every available command is in ⌘K, the macOS menu bar and the ⋯ menu, with its shortcut',
    run: async () => {
      const { menuModel } = await import('../lib/commands/menu')
      const { chordLabel } = await import('../lib/commands/keys')
      const commands = hooks.registry!.available().filter((c) => c.palette)
      const inMenuBar = new Map(
        menuModel(hooks.registry!.available()).flatMap((m) => m.items.map((i) => [i.id, i])),
      )
      keyOnApp('k', { code: 'KeyK', metaKey: true })
      await waitFor('palette', palette)
      const inPalette = new Map(
        Array.from(palette()!.querySelectorAll('.row')).map((r) => [
          r.querySelector('.label')?.textContent,
          r.querySelector('kbd')?.textContent ?? '',
        ]),
      )
      await escModal()
      await showControls()
      document.querySelector<HTMLButtonElement>('.more-button')!.click()
      const more = await waitFor('⋯ menu', () => document.querySelector<HTMLElement>('.more'))
      const inMore = new Map(
        Array.from(more.querySelectorAll('[role="menuitem"]')).map((r) => [
          r.querySelector('span')?.textContent,
          r.querySelector('.key')?.textContent ?? '',
        ]),
      )
      key('Escape', { code: 'Escape' })
      await settled(300)
      await hideControls()
      const problems: string[] = []
      for (const c of commands) {
        const k = c.chord ?? c.singleKey
        const label = k ? chordLabel(k) : ''
        const enabled = c.enabled?.() ?? true
        if (enabled && inPalette.get(c.title) !== label)
          problems.push(`⌘K ${c.id} “${inPalette.get(c.title)}”`)
        // Extension commands are in ⌘K; in ⋯ only when pinned (P8), never in the menu bar.
        if (c.extensionId) continue
        if (!inMore.has(c.title) || inMore.get(c.title) !== label) problems.push(`⋯ ${c.id}`)
        if (c.menu && !inMenuBar.has(c.id)) problems.push(`menu bar ${c.id}`)
        // Settings… lives in the app menu, macOS's place for it (menubar.ts).
        if (!c.menu && c.id !== 'app.settings') problems.push(`no menu bar place for ${c.id}`)
      }
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'K1-chapter-keys',
    description: '⌥↓ ⌥↑ and ] [ move by chapter',
    run: async () => {
      const s0 = loc()!.sectionIndex
      key('ArrowDown', { code: 'ArrowDown', altKey: true })
      await settled(700)
      const s1 = loc()!.sectionIndex
      key(']', { code: 'BracketRight' })
      await settled(700)
      const s2 = loc()!.sectionIndex
      key('[', { code: 'BracketLeft' })
      await settled(700)
      key('ArrowUp', { code: 'ArrowUp', altKey: true })
      await settled(700)
      const s3 = loc()!.sectionIndex
      return s1 > s0 && s2 > s1 && s3 === s0
        ? 'ok'
        : `sections ${s0} → ${s1} → ${s2} → back to ${s3}`
    },
  })

  const searchField = () => document.querySelector<HTMLInputElement>('.navigator .search input')
  /** Search as a reader does: ⌘F, type, and wait until every chapter is searched. */
  const searchFor = async (query: string) => {
    keyOnApp('f', { code: 'KeyF', metaKey: true })
    const field = await waitFor('search field', searchField)
    typeInto(field, query)
    const search = reader()!.search
    // Finished for this query: past the typing pause and every chapter searched.
    await waitFor(
      'search finished',
      () => (search.query === query && search.settled && !search.running) || null,
      60_000,
    )
    await settled(150)
    return { results: search.count, chapters: search.groups.length }
  }
  const closeSearch = async () => {
    key('Escape', { code: 'Escape' })
    await settled(600)
  }
  const fold = (x: string) =>
    x
      .normalize('NFD')
      .replace(/\p{M}+/gu, '')
      .toLowerCase()
      .replace(/\s+/g, ' ')

  checks.push({
    id: 'F-golden-results',
    description:
      'Golden results for the corpus query sets (Spike F): English, quoted phrase, diacritics, CJK from one character',
    run: async () => {
      const golden: [RegExp, string, number, number][] = [
        [/Moby Dick(?!;)/, 'whale', 1688, 120],
        [/Moby Dick(?!;)/, 'Queequeg', 252, 42],
        [/Moby Dick(?!;)/, 'harpoon', 256, 66],
        [/Moby Dick(?!;)/, '"Call me Ishmael"', 1, 1],
        [/Moby Dick(?!;)/, 'call me ishmael', 1, 1],
        [/Moby Dick(?!;)/, 'xyzzy', 0, 0],
        [/Sous le vent/, 'etait', 15, 3],
        [/草枕/, '智に働けば', 1, 1],
        [/草枕/, '山', 103, 13],
      ]
      const wrong: string[] = []
      let open: RegExp | null = null
      for (const [book, query, results, chapters] of golden) {
        if (open !== book) {
          await backToLibrary()
          await openFromLibrary(book)
          open = book
        }
        const got = await searchFor(query)
        log(`F golden: ${query} → ${got.results} in ${got.chapters}`)
        if (got.results !== results || got.chapters !== chapters)
          wrong.push(
            `${query}: ${got.results} in ${got.chapters}, expected ${results} in ${chapters}`,
          )
        await closeSearch()
      }
      return wrong.length ? wrong.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'F5-results-land',
    description:
      'Every result lands on its match on the page shown, marked as F5 says (tint + 1 px outline; active 2 px accent); marks go when Search closes',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      await reader()!.engine.goToTextStart()
      await settled(600)
      const got = await searchFor('water')
      if (got.results < 50) return `${got.results} results for “water”`
      const engine = reader()!.engine
      const search = reader()!.search
      // The colours of the theme the reader is in (Auto follows the system).
      const css = getComputedStyle(document.querySelector('.reader')!)
      // Computed values write “.26” as “0.26”; compare numbers, not spellings.
      const norm = (c: string) => c.replace(/\s/g, '').replace(/(^|[^\d])\.(\d)/g, '$10.$2')
      const colors = {
        tint: norm(css.getPropertyValue('--search-tint')),
        outline: norm(css.getPropertyValue('--search-outline')),
        activeTint: norm(css.getPropertyValue('--search-active-tint')),
        activeOutline: norm(css.getPropertyValue('--search-active-outline')),
      }
      const field = searchField()!
      const problems: string[] = []
      for (let i = 0; i < got.results && problems.length < 4; i++) {
        field.dispatchEvent(
          new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
        )
        await waitFor('result', () => search.position === i + 1 || null)
        await settled(i < 40 ? 250 : 120)
        const a = search.active!
        const hit = search.groups.find((g) => g.index === a.index)!.matches[a.n]
        const shown = engine.view.lastLocation?.range
        const doc = shown?.startContainer.ownerDocument
        const range = doc ? engine.textRange(doc, hit.start, hit.end) : null
        if (loc()!.sectionIndex !== a.index)
          problems.push(`result ${i + 1}: in section ${loc()!.sectionIndex}, not ${a.index}`)
        else if (!range || fold(range.toString()) !== 'water')
          problems.push(`result ${i + 1}: range “${range?.toString()}”`)
        else if (
          shown!.compareBoundaryPoints(Range.START_TO_START, range) > 0 ||
          shown!.compareBoundaryPoints(Range.END_TO_END, range) < 0
        )
          problems.push(`result ${i + 1}: not on the page shown`)
        const raw = engine.debug().marks
        const style = (x: typeof raw.active) =>
          x && { ...x, fill: norm(x.fill), stroke: norm(x.stroke) }
        const m = { ...raw, active: style(raw.active), soft: style(raw.soft) }
        if (
          !m.active ||
          m.active.width !== 2 ||
          m.active.stroke !== colors.activeOutline ||
          m.active.fill !== colors.activeTint
        )
          problems.push(
            `result ${i + 1}: active mark ${JSON.stringify(m.active)}, theme ${JSON.stringify(colors)}`,
          )
        if (
          m.soft &&
          (m.soft.width !== 1 || m.soft.stroke !== colors.outline || m.soft.fill !== colors.tint)
        )
          problems.push(`result ${i + 1}: mark ${JSON.stringify(m.soft)}`)
      }
      await closeSearch()
      if (engine.debug().marks.drawn) problems.push('marks stayed after Search closed')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'F7-esc-return',
    description:
      'Esc returns to the original page after only browsing; after choosing a result you stay, and Back returns',
    run: async () => {
      await reader()!.engine.goToTextStart()
      await settled(600)
      const origin = loc()!.cfi
      await searchFor('harpoon')
      const field = searchField()!
      for (let i = 0; i < 3; i++) {
        field.dispatchEvent(
          new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
        )
        await settled(300)
      }
      if (loc()!.cfi === origin) return 'browsing did not move the page'
      await closeSearch()
      if (loc()!.cfi !== origin) return `Esc after browsing stayed at ${loc()!.cfi}`
      // Choose a result: stay, and Back returns.
      await searchFor('harpoon')
      document.querySelectorAll<HTMLButtonElement>('.navigator .search .hit')[1].click()
      await settled(600)
      const chosen = loc()!.cfi
      await closeSearch()
      if (loc()!.cfi !== chosen) return 'Esc after choosing did not stay'
      await back()
      return loc()!.cfi === origin ? 'ok' : 'Back did not return to where Search began'
    },
  })

  checks.push({
    id: 'F6-keys',
    description: '⇧↵ goes back a result; ⌘G / ⇧⌘G move through results anywhere, reopening Search',
    run: async () => {
      await searchFor('Queequeg')
      const field = searchField()!
      const enter = (shiftKey = false) =>
        field.dispatchEvent(
          new KeyboardEvent('keydown', { key: 'Enter', shiftKey, bubbles: true, cancelable: true }),
        )
      enter()
      await settled(250)
      enter()
      await settled(250)
      enter(true)
      await settled(250)
      const search = reader()!.search
      const position = () => search.position // read afresh: it changes with each key
      if (position() !== 1) return `after ↵ ↵ ⇧↵ at result ${position()}`
      await closeSearch()
      keyOnApp('g', { code: 'KeyG', metaKey: true })
      await settled(400)
      if (!document.querySelector('.navigator .search')) return '⌘G did not reopen Search'
      // The next result after the active one (result 1).
      if (position() !== 2) return `⌘G moved to result ${position()}`
      keyOnApp('g', { code: 'KeyG', metaKey: true })
      await settled(300)
      keyOnApp('g', { code: 'KeyG', metaKey: true, shiftKey: true })
      await settled(300)
      const at = search.position
      await closeSearch()
      return at === 2 ? 'ok' : `⌘G ⇧⌘G ended at result ${at}`
    },
  })

  checks.push({
    id: 'F3-F8-streaming-persisted',
    description:
      'The first search streams (“Searching N of M”, current chapter first); the index is saved, and a search after reopening meets the Spike F budget (< 300 ms)',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick; Or/)
      await reader()!.engine.goTo(20)
      await settled(700)
      const current = loc()!.sectionIndex
      keyOnApp('f', { code: 'KeyF', metaKey: true })
      const field = await waitFor('search field', searchField)
      // “Searching N of M chapters” can be brief: watch the panel for it.
      let sawProgress = false
      const watch = new MutationObserver(() => {
        const status = document.querySelector('.navigator .search .status')?.textContent ?? ''
        if (/Searching \d+ of \d+ chapters/.test(status)) sawProgress = true
      })
      watch.observe(document.querySelector('.navigator')!, {
        subtree: true,
        childList: true,
        characterData: true,
      })
      typeInto(field, 'the')
      const search = reader()!.search
      await waitFor(
        'search finished',
        () => (search.query === 'the' && search.settled && !search.running) || null,
        60_000,
      )
      watch.disconnect()
      const first = search.groups[0]?.index
      await closeSearch()
      if (!sawProgress) return 'no “Searching N of M chapters” while indexing'
      if (first !== current) return `first results from section ${first}, reading ${current}`
      const saved = await invoke<[number, string][]>('search_text_get', {
        bookId: reader()!.bookId,
      })
      if (saved.length < search.total) return `${saved.length} of ${search.total} chapters saved`
      await backToLibrary()
      await openFromLibrary(/Moby Dick; Or/)
      await settled(500)
      keyOnApp('f', { code: 'KeyF', metaKey: true })
      const again = await waitFor('search field', searchField)
      typeInto(again, 'the')
      reader()!.search.run() // skip the 150 ms typing debounce for the timing
      await waitFor('search finished', () => reader()!.search.settled || null, 10_000)
      await settled(300)
      // From starting the search to the results painted (Spike F: < 300 ms from the cache).
      const timings = reader()!.search.timings!
      const ms = timings.painted
      await closeSearch()
      log(`F8: search from the saved index painted in ${ms} ms (${JSON.stringify(timings)})`)
      return ms < 300 ? 'ok' : `search from the saved index took ${ms} ms`
    },
  })

  checks.push({
    id: 'F2-minimum-length',
    description: 'Search starts from 2 characters, or 1 for CJK',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      const one = await searchFor('w')
      await closeSearch()
      return one.results === 0 && !document.querySelector('.navigator .search .hit')
        ? 'ok'
        : `“w” gave ${one.results} results`
    },
  })

  // ---------------------------------------------------------------- Phase 5: selection and annotation

  checks.push({
    id: 'A1-selection-bar',
    description:
      'A selection shows the bar 12 px above its first line (below when < 56 px above), never over the selection; F6 moves focus in, arrows move, Esc returns to the text',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      const problems: string[] = []
      const range = await selectPhrase(MANHATTOES)
      const bar = await waitFor('selection bar', selBar)
      await settled(150)
      const frame =
        range.startContainer.ownerDocument!.defaultView!.frameElement!.getBoundingClientRect()
      const lines = Array.from(range.getClientRects()).map(
        (r) => new DOMRect(r.left + frame.left, r.top + frame.top, r.width, r.height),
      )
      const b = bar.getBoundingClientRect()
      if (Math.abs(lines[0].top - 12 - b.bottom) > 1)
        problems.push(`bar bottom ${b.bottom.toFixed(1)}, first line ${lines[0].top.toFixed(1)}`)
      if (
        lines.some(
          (l) => l.top < b.bottom && l.bottom > b.top && l.left < b.right && l.right > b.left,
        )
      )
        problems.push('the bar covers the selection')
      const ground = getComputedStyle(document.querySelector('.reader')!)
        .getPropertyValue('--selbar-ground')
        .trim()
      const probe = document.createElement('div')
      probe.style.color = ground
      document.body.append(probe)
      const expected = getComputedStyle(probe).color
      probe.remove()
      if (getComputedStyle(bar).backgroundColor !== expected)
        problems.push(`bar colour ${getComputedStyle(bar).backgroundColor}, theme ${expected}`)
      if (barButton(/more actions/i)) problems.push('the empty extension area shows')
      // A2: F6, arrows, Esc.
      key('F6', { code: 'F6' })
      await settled(100)
      if (document.activeElement !== bar.querySelector('button'))
        problems.push('F6 did not focus the bar')
      document.activeElement!.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }),
      )
      if (document.activeElement !== bar.querySelectorAll('button')[1])
        problems.push('→ did not move')
      document.activeElement!.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
      )
      await settled(150)
      if (bar.contains(document.activeElement)) problems.push('Esc left focus in the bar')
      if (!pageDoc().doc.getSelection()?.toString()) problems.push('Esc lost the selection')
      // Flip: with the controls shown, a selection on the page's first line has < 56 px above.
      key('Escape', { code: 'Escape' })
      pageDoc().doc.getSelection()?.removeAllRanges()
      await settled(200)
      const top = firstLineRange()
      const doc = pageDoc().doc
      await showControls()
      doc.getSelection()!.removeAllRanges()
      doc.getSelection()!.addRange(top)
      doc.body.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }))
      const flipped = await waitFor('bar', selBar)
      await settled(150)
      const t = Array.from(top.getClientRects())[0]
      const f2 = doc.defaultView!.frameElement!.getBoundingClientRect()
      const lastBottom = t.bottom + f2.top
      if (t.top + f2.top - 52 < 56) {
        if (!flipped.classList.contains('below')) problems.push('did not flip below near the top')
        else if (Math.abs(flipped.getBoundingClientRect().top - (lastBottom + 12)) > 1)
          problems.push('flipped bar is not 12 px below the selection')
      } else problems.push(`test setup: first line at ${(t.top + f2.top).toFixed(0)} px`)
      doc.getSelection()?.removeAllRanges()
      doc.body.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }))
      await settled(300)
      if (selBar()) problems.push('the bar stayed after the selection went')
      await hideControls()
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  let november = ''
  checks.push({
    id: 'A4-highlight',
    description:
      'A colour saves the highlight at once (no confirmation), drawn as the theme tint plus a 2 px underline; the selection and bar go',
    run: async () => {
      const before = annotationsOf().items.length
      await selectPhrase(NOVEMBER)
      await waitFor('bar', selBar)
      await pressBar(/highlight yellow/i)
      const problems: string[] = []
      const a = annotationsOf().items.at(-1)
      if (annotationsOf().items.length !== before + 1 || !a) return 'no highlight made'
      november = a.id
      if (a.color !== 'yellow' || a.quote.exact !== NOVEMBER)
        problems.push(`${a.color} “${a.quote.exact}”`)
      if ((await stored()).every((r) => r.id !== a.id)) problems.push('not saved')
      if (selBar()) problems.push('bar stayed')
      if (pageDoc().doc.getSelection()?.toString()) problems.push('selection stayed')
      const d = drawn(a.id)
      if (!d.behindText) problems.push('no tint behind the text')
      if (!d.lines.length) problems.push('no underline')
      // Older WebKit (no custom highlights): the tint is an overlay multiplied with the page.
      const engine = reader()!.engine
      engine.useCustomHighlights = false
      engine.redrawHighlights()
      const fallback = drawn(a.id)
      if (fallback.behindText) problems.push('the fallback left the custom highlight')
      if (!fallback.overlayTints.length || fallback.overlayTints.length !== fallback.lines.length)
        problems.push(
          `fallback: ${fallback.overlayTints.length} tints, ${fallback.lines.length} underlines`,
        )
      engine.useCustomHighlights = true
      engine.redrawHighlights()
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'B4-overlaps',
    description:
      'Selecting a highlight’s exact range changes its colour; a partial overlap makes a second highlight; Copy copies plain text',
    run: async () => {
      const problems: string[] = []
      const n = annotationsOf().items.length
      await selectPhrase(NOVEMBER)
      await pressBar(/highlight blue/i)
      if (annotationsOf().items.length !== n) problems.push('same range made a new highlight')
      if (annotationsOf().get(november)?.color !== 'blue') problems.push('colour did not change')
      await selectPhrase('drizzly November in my soul; whenever')
      await pressBar(/highlight green/i)
      if (annotationsOf().items.length !== n + 1)
        problems.push('partial overlap did not make a second')
      // Copy: plain text only, and the reader's clipboard comes back afterwards.
      const saved = await invoke<string | null>('spike_read_pasteboard')
      try {
        await selectPhrase(MANHATTOES)
        await pressBar(/^copy$/i)
        await settled(200)
        const copied = await invoke<string | null>('spike_read_pasteboard')
        if (copied !== MANHATTOES) problems.push(`copied “${copied}”`)
      } finally {
        if (saved !== null) await invoke('copy_text', { text: saved })
      }
      // Tidy: the partial one goes (not part of later checks).
      const partial = annotationsOf().items.at(-1)!
      annotationsOf().remove(partial.id)
      pageDoc().doc.getSelection()?.removeAllRanges()
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A5-click-highlight',
    description:
      'A plain click on a highlight reopens the bar with its colour, Note, Copy and Delete; a drag starting on one begins a new selection',
    run: async () => {
      const problems: string[] = []
      const range = await selectPhrase(NOVEMBER)
      pageDoc().doc.getSelection()?.removeAllRanges()
      clickOn(range)
      const bar = await waitFor('bar for the highlight', selBar)
      await settled(150)
      if (!barButton(/^delete$/i)) problems.push('no Delete')
      if (barButton(/^search$/i)) problems.push('Search shown for a highlight')
      const current = bar.querySelector('button[aria-pressed="true"]')?.getAttribute('aria-label')
      if (!/blue/i.test(current ?? '')) problems.push(`current colour “${current}”`)
      key('Escape', { code: 'Escape' })
      await settled(300)
      // A drag that starts on the highlight: a new selection, with the new-selection bar.
      await selectPhrase('drizzly November in my soul; whenever I find')
      if (!barButton(/^search$/i))
        problems.push('a drag from a highlight did not start a new selection')
      pageDoc().doc.getSelection()?.removeAllRanges()
      key('Escape', { code: 'Escape' })
      await settled(300)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A7-delete-undo',
    description:
      'Delete is immediate with an Undo message; ⌘Z restores the exact annotation; after the message times out, ⌘K › Recently closed still does',
    run: async () => {
      const problems: string[] = []
      const exact = annotationsOf().get(november)
      if (!exact) return 'no highlight to delete'
      const del = async () => {
        const range = await selectPhrase(NOVEMBER)
        pageDoc().doc.getSelection()?.removeAllRanges()
        clickOn(range)
        await waitFor('bar', selBar)
        await pressBar(/^delete$/i)
      }
      await del()
      if (annotationsOf().get(november)) problems.push('not deleted')
      if (document.querySelector('dialog[open]')) problems.push('asked for confirmation')
      if ((await stored()).some((r) => r.id === november)) problems.push('still stored')
      const message = hooks.messages!.current
      if (message?.text !== 'Highlight deleted' || message.action?.label !== 'Undo')
        problems.push(`message “${message?.text}”`)
      keyOnApp('z', { code: 'KeyZ', metaKey: true })
      await settled(300)
      if (JSON.stringify(annotationsOf().get(november)) !== JSON.stringify(exact))
        problems.push('⌘Z did not restore the exact annotation')
      if ((await stored()).every((r) => r.id !== november)) problems.push('restore not stored')
      if (hooks.messages!.current?.id === message?.id) problems.push('the Undo message stayed')
      // Again, and let the message time out.
      await del()
      const second = hooks.messages!.current!
      await waitFor('message timed out', () => hooks.messages!.current?.id !== second.id, 15_000)
      keyOnApp('k', { code: 'KeyK', metaKey: true })
      await waitFor('palette', palette)
      await settled(200)
      const row = Array.from(palette()!.querySelectorAll<HTMLElement>('.row')).find((r) =>
        /Restore highlight “damp, drizzly November/.test(r.textContent ?? ''),
      )
      if (!row) problems.push('not in Recently closed')
      else {
        row.click()
        await settled(400)
      }
      if (palette()) await escModal()
      if (JSON.stringify(annotationsOf().get(november)) !== JSON.stringify(exact))
        problems.push('Recently closed did not restore it exactly')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A11-caret-browsing',
    description:
      'F7 shows a caret; the arrows move it without turning the page; ⇧ + arrows select, and the bar appears; F7 again hides it',
    run: async () => {
      const problems: string[] = []
      await reader()!.engine.goToTextStart()
      await settled(600)
      const page = where()
      const caret = () => pageDoc().overlayer?.element.querySelector('[data-linen-caret]')
      key('F7', { code: 'F7' })
      await settled(200)
      if (!caret()) problems.push('no caret')
      const sel = () => pageDoc().doc.getSelection()!
      const before = sel().focusOffset
      key('ArrowRight', { code: 'ArrowRight' })
      await settled(150)
      if (sel().focusOffset !== before + 1)
        problems.push(`→ moved ${before} → ${sel().focusOffset}`)
      if (where() !== page) problems.push('→ turned the page')
      for (let i = 0; i < 12; i++) key('ArrowRight', { code: 'ArrowRight', shiftKey: true })
      pageDoc().doc.body.dispatchEvent(
        new KeyboardEvent('keyup', { key: 'Shift', shiftKey: false, bubbles: true }),
      )
      await settled(300)
      if (sel().toString().length !== 12) problems.push(`selected “${sel().toString()}”`)
      if (!selBar()) problems.push('no selection bar for a keyboard selection')
      key('ArrowLeft', { code: 'ArrowLeft' })
      key('F7', { code: 'F7' })
      await settled(200)
      if (caret()) problems.push('F7 did not hide the caret')
      pageDoc().doc.getSelection()?.removeAllRanges()
      if (selBar()) key('Escape', { code: 'Escape' })
      await settled(200)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A10-context-menu',
    description:
      'Right-click on a selection or a highlight gives the same actions (native menu); a colour change is undone by ⌘Z (B5)',
    run: async () => {
      const problems: string[] = []
      const rightClick = (range: Range) => {
        const r = Array.from(range.getClientRects()).find((x) => x.width > 4)!
        const ev = new MouseEvent('contextmenu', {
          clientX: r.left + r.width / 2,
          clientY: r.top + r.height / 2,
          bubbles: true,
          cancelable: true,
        })
        range.startContainer.parentElement!.dispatchEvent(ev)
        return ev.defaultPrevented
      }
      const recorded = () => hooks.contextMenu
      hooks.contextMenu = undefined
      const range = await selectPhrase(MANHATTOES)
      if (!rightClick(range)) problems.push('the WebKit menu was not replaced')
      await settled(100)
      const labels = recorded()?.labels.join(', ')
      if (
        labels !==
        'Highlight Yellow, Highlight Green, Highlight Blue, Highlight Rose, Note, Copy, Search'
      )
        problems.push(`selection menu: ${labels}`)
      pageDoc().doc.getSelection()?.removeAllRanges()
      pageDoc().doc.body.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }))
      await settled(300)
      hooks.contextMenu = undefined
      const nov = await selectPhrase(NOVEMBER)
      pageDoc().doc.getSelection()?.removeAllRanges()
      pageDoc().doc.body.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }))
      await settled(300)
      rightClick(nov)
      await settled(100)
      const menu = recorded()
      if (!menu?.labels.includes('Delete'))
        problems.push(`highlight menu: ${menu?.labels.join(', ')}`)
      const before = annotationsOf().get(november)?.color
      menu?.run('Highlight Rose')
      await settled(200)
      if (annotationsOf().get(november)?.color !== 'rose')
        problems.push('the menu did not recolour')
      keyOnApp('z', { code: 'KeyZ', metaKey: true })
      await settled(200)
      if (annotationsOf().get(november)?.color !== before)
        problems.push('⌘Z did not undo the colour')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  let manhattoes = ''
  checks.push({
    id: 'A3-keys',
    description: 'H and ⇧⌘H highlight in the last-used colour; N adds a note (focus in the note)',
    run: async () => {
      const problems: string[] = []
      annotationsOf().lastColor = 'green'
      await selectPhrase('Circumambulate the city of a dreamy Sabbath afternoon')
      key('h', { code: 'KeyH' })
      await settled(300)
      const h = annotationsOf().items.at(-1)!
      if (
        h.quote.exact !== 'Circumambulate the city of a dreamy Sabbath afternoon' ||
        h.color !== 'green'
      )
        problems.push(`H made ${h.color} “${h.quote.exact}”`)
      await selectPhrase('Go from Corlears Hook to Coenties Slip')
      key('H', { code: 'KeyH', metaKey: true, shiftKey: true })
      await settled(300)
      if (annotationsOf().items.at(-1)!.quote.exact !== 'Go from Corlears Hook to Coenties Slip')
        problems.push('⇧⌘H did not highlight')
      annotationsOf().remove(h.id)
      annotationsOf().remove(annotationsOf().items.at(-1)!.id)
      await selectPhrase(MANHATTOES)
      key('n', { code: 'KeyN' })
      const card = await waitFor('note card', noteCard)
      await settled(200)
      manhattoes = annotationsOf().items.at(-1)!.id
      if (annotationsOf().get(manhattoes)?.quote.exact !== MANHATTOES)
        problems.push('N did not highlight')
      if (document.activeElement !== card.querySelector('textarea'))
        problems.push('focus is not in the note')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A6-margin-note',
    description:
      'From 1240 px the note is a card in the margin joined to its dot; it saves as you type and says so; Esc closes it and focus returns to the text',
    run: async () => {
      const problems: string[] = []
      const card = noteCard() ?? (await waitFor('note card', noteCard))
      if (!card.classList.contains('margin'))
        return `a ${card.className} at ${window.innerWidth} px`
      typeNote('Ishmael frames the voyage as a cure for his own gloom.')
      await settled(50)
      const status = () => card.querySelector('[role="status"]')?.textContent?.trim()
      if (status() === 'Saved') problems.push('said Saved before saving')
      await waitFor('Saved', () => status() === 'Saved', 3000).catch(() =>
        problems.push(`status “${status()}”`),
      )
      const row = (await stored()).find((r) => r.id === manhattoes)
      if (row?.note !== 'Ishmael frames the voyage as a cure for his own gloom.')
        problems.push(`stored ${row?.note}`)
      const text = reader()!.engine.passageBox(annotationsOf().get(manhattoes)!.cfi)!
      const c = card.getBoundingClientRect()
      if (c.left < text.edge + 20) problems.push(`card at ${c.left}, column edge ${text.edge}`)
      await settled(100)
      if (!pageDoc().overlayer?.element.querySelector('[data-linen-note-dot]'))
        problems.push('no margin dot')
      card
        .querySelector('textarea')!
        .dispatchEvent(
          new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
        )
      await settled(300)
      if (noteCard()) problems.push('Esc did not close it')
      if (
        document.activeElement?.tagName !== 'IFRAME' &&
        !document.activeElement?.closest('.reader')
      )
        problems.push(`focus on ${document.activeElement?.tagName}`)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A6-empty-note',
    description:
      'An emptied note is discarded and its highlight kept (with Undo); a click elsewhere closes the card',
    run: async () => {
      const problems: string[] = []
      const range = await selectPhrase(MANHATTOES)
      pageDoc().doc.getSelection()?.removeAllRanges()
      clickOn(range)
      await waitFor('bar', selBar)
      await pressBar(/^note$/i)
      await waitFor('note card', noteCard)
      typeNote('   ')
      // A click in the text closes it.
      pageDoc().doc.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
      await settled(400)
      if (noteCard()) problems.push('a click elsewhere did not close it')
      const a = annotationsOf().get(manhattoes)
      if (!a) problems.push('the highlight went')
      else if (a.note !== null) problems.push(`note kept as “${a.note}”`)
      if ((await stored()).find((r) => r.id === manhattoes)?.note !== null)
        problems.push('stored note kept')
      if (hooks.messages!.current?.text !== 'Note deleted')
        problems.push(`message “${hooks.messages!.current?.text}”`)
      keyOnApp('z', { code: 'KeyZ', metaKey: true })
      await settled(300)
      if (
        annotationsOf().get(manhattoes)?.note !==
        'Ishmael frames the voyage as a cure for his own gloom.'
      )
        problems.push('Undo did not bring the note back')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A6-note-sheet',
    description: 'Below 1240 px the note is a bottom sheet with Delete and Done',
    run: async () => {
      const w = getCurrentWindow()
      const size = await w.innerSize()
      const factor = await w.scaleFactor()
      await w.setSize(new LogicalSize(1000, 800))
      await settled(1200)
      try {
        const range = await selectPhrase(MANHATTOES)
        pageDoc().doc.getSelection()?.removeAllRanges()
        clickOn(range)
        await waitFor('bar', selBar)
        await pressBar(/^note$/i)
        const card = await waitFor('note', noteCard)
        const problems: string[] = []
        if (!card.classList.contains('sheet')) problems.push(`a ${card.className}`)
        const r = card.getBoundingClientRect()
        if (Math.abs(r.bottom - window.innerHeight) > 1 || r.left !== 0)
          problems.push('not at the bottom')
        const labels = Array.from(card.querySelectorAll('button')).map((b) => b.textContent?.trim())
        if (!labels.includes('Delete') || !labels.includes('Done'))
          problems.push(`buttons ${labels.join(',')}`)
        if (!card.textContent?.includes('“There now is your insular city'))
          problems.push('no quote')
        Array.from(card.querySelectorAll('button'))
          .find((b) => b.textContent?.trim() === 'Done')!
          .click()
        await settled(300)
        if (noteCard()) problems.push('Done did not close it')
        return problems.length ? problems.join('; ') : 'ok'
      } finally {
        await w.setSize(new LogicalSize(size.width / factor, size.height / factor))
        await settled(1200)
      }
    },
  })

  checks.push({
    id: 'A6-note-quit',
    description: 'A note typed and then followed at once by a quit is saved',
    run: async () => {
      const range = await selectPhrase(MANHATTOES)
      pageDoc().doc.getSelection()?.removeAllRanges()
      clickOn(range)
      await waitFor('bar', selBar)
      await pressBar(/^note$/i)
      await waitFor('note', noteCard)
      const text = 'Typed just before quitting.'
      typeNote(text) // no pause: the autosave has not run yet
      hooks.quitRequested = false
      await emit('app-quitting')
      await waitFor('quit handled', () => hooks.quitRequested, 3000)
      const row = (await stored()).find((r) => r.id === manhattoes)
      noteCard()
        ?.querySelector('textarea')
        ?.dispatchEvent(
          new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
        )
      await settled(300)
      await annotationsOf().setNote(
        manhattoes,
        'Ishmael frames the voyage as a cure for his own gloom.',
      )
      return row?.note === text ? 'ok' : `stored “${row?.note}”`
    },
  })

  checks.push({
    id: 'A8-notes-tab',
    description:
      'Notes tab: by chapter in reading order, “N highlights · M notes”, filter by colour and text; choosing one jumps, pulses it for 1.2 s and offers Back',
    run: async () => {
      const problems: string[] = []
      await reader()!.engine.goToTextStart()
      await settled(600)
      const origin = loc()!.cfi
      keyOnApp('a', { code: 'KeyA', metaKey: true, shiftKey: true })
      const panel = await waitFor('notes tab', () =>
        document.querySelector<HTMLElement>('.navigator .notes'),
      )
      await settled(300)
      const counts = panel.querySelector('.counts')?.textContent
      if (counts !== '2 highlights · 1 note') problems.push(`counts “${counts}”`)
      const items = () => Array.from(panel.querySelectorAll<HTMLElement>('[data-annotation]'))
      const order = items().map((i) => i.querySelector('.quote')?.textContent?.trim())
      if (order[0] !== NOVEMBER || order[1] !== MANHATTOES)
        problems.push(`order ${order.join(' | ')}`)
      const group = panel.querySelector('.group')?.textContent ?? ''
      if (!/Loomings/.test(group)) problems.push(`group “${group}”`)
      // Filters.
      panel.querySelector<HTMLButtonElement>('[aria-label="Only blue"]')!.click()
      await settled(100)
      if (items().length !== 1) problems.push(`blue filter shows ${items().length}`)
      panel.querySelector<HTMLButtonElement>('.pill')!.click()
      const field = panel.querySelector<HTMLInputElement>('input')!
      field.value = 'gloom'
      field.dispatchEvent(new Event('input', { bubbles: true }))
      await settled(100)
      if (items().length !== 1 || !/Manhattoes/.test(items()[0].textContent ?? ''))
        problems.push('text filter did not match the note')
      field.value = ''
      field.dispatchEvent(new Event('input', { bubbles: true }))
      await settled(100)
      // Choose: jump, pulse, Back.
      items()[1].click()
      await settled(700)
      const a = annotationsOf().get(manhattoes)!
      const shown = reader()!.engine.passageBox(a.cfi)
      if (!shown || shown.first.left < 0 || shown.first.left > window.innerWidth)
        problems.push('did not land on it')
      const pulse = () => !!pageDoc().overlayer?.element.querySelector('rect[stroke-width="3"]')
      if (!pulse()) problems.push('no pulse')
      await settled(1400)
      if (pulse()) problems.push('pulse did not end')
      if (!/^Back/.test(hooks.messages!.current?.text ?? ''))
        problems.push(`message “${hooks.messages!.current?.text}”`)
      await back()
      if (loc()!.cfi !== origin) problems.push('Back did not return')
      if (document.querySelector('.navigator')) key('Escape', { code: 'Escape' })
      await settled(400)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A9-reflow',
    description: 'Highlights stay on their text through reflow (window size, Scroll mode and back)',
    run: async () => {
      const problems: string[] = []
      const verify = async (when: string) => {
        await settled(900)
        const a = annotationsOf().get(november)!
        await reader()!.engine.goTo(a.cfi)
        await settled(700)
        const d = drawn(november)
        const box = reader()!.engine.passageBox(a.cfi)
        // The underline (overlay) sits under the text's first line; the tint (a custom
        // highlight) holds the live range, so it moves with the text by construction.
        const line = d.lines[0]?.getBBox()
        const frame = pageDoc().doc.defaultView!.frameElement!.getBoundingClientRect()
        if (!box || !line || !d.behindText) return problems.push(`${when}: not drawn`)
        if (
          Math.abs(line.x + frame.left - box.first.left) > 1 ||
          Math.abs(line.y + 2 + frame.top - box.first.bottom) > 1
        )
          problems.push(
            `${when}: underline at ${line.x + frame.left},${line.y + frame.top}, text at ${box.first.left},${box.first.bottom}`,
          )
      }
      const w = getCurrentWindow()
      const size = await w.innerSize()
      const factor = await w.scaleFactor()
      await verify('before')
      try {
        await w.setSize(new LogicalSize(1600, 900))
        await verify('wider')
      } finally {
        await w.setSize(new LogicalSize(size.width / factor, size.height / factor))
      }
      await verify('restored')
      hooks.run?.('layout.scroll')
      await verify('Scroll mode')
      hooks.run?.('layout.pages')
      await verify('Pages mode')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A9-reimport',
    description:
      'Re-importing an edited edition re-anchors: an inserted paragraph and changed punctuation are survived; a deleted passage goes to “Couldn’t place”, never a wrong place; Re-attach (G11) places it again',
    run: async () => {
      const problems: string[] = []
      const path = (name: string) => invoke<string>('spike_corpus_path', { name })
      await invoke('library_import', { paths: [await path('anchoring-first.epub')] })
      await backToLibrary()
      await openFromLibrary(/Anchoring test/)
      const make = async (phrase: string, color: RegExp) => {
        await selectPhrase(phrase)
        await pressBar(color)
        return annotationsOf().items.at(-1)!.id
      }
      const pistol = 'This is my substitute for pistol and ball.'
      const ids = {
        manhattoes: await make(MANHATTOES, /highlight yellow/i),
        november: await make(NOVEMBER, /highlight green/i),
        pistol: await make(pistol, /highlight rose/i),
      }
      await hooks.writes!.idle()
      await backToLibrary()
      const [r] = await invoke<{ outcome: { kind: string } }[]>('library_import', {
        paths: [await path('anchoring-revised.epub')],
      })
      if (r.outcome.kind !== 'replaced') return `import: ${r.outcome.kind}`
      hooks.run?.('library.show') // the library lists the new edition, as after any import
      await settled(500)
      await openFromLibrary(/Anchoring test/)
      await waitFor(
        're-anchored',
        () => !annotationsOf().stale.length && annotationsOf().items.length === 3,
        10_000,
      )
      await settled(300)
      const textAt = (id: string) => {
        const a = annotationsOf().get(id)!
        const { doc } = pageDoc()
        const range = reader()!.engine.view.resolveCFI(a.cfi).anchor(doc) as Range
        return range.toString()
      }
      await reader()!.engine.goTo(annotationsOf().get(ids.manhattoes)!.cfi)
      await settled(600)
      if (
        annotationsOf().get(ids.manhattoes)?.status !== 'anchored' ||
        textAt(ids.manhattoes) !== MANHATTOES
      )
        problems.push('inserted paragraph: not placed on its text')
      await reader()!.engine.goTo(annotationsOf().get(ids.november)!.cfi)
      await settled(600)
      if (textAt(ids.november) !== 'damp; drizzly November in my soul')
        problems.push(`changed punctuation: placed on “${textAt(ids.november)}”`)
      const lost = annotationsOf().get(ids.pistol)!
      if (lost.status !== 'unplaced') problems.push(`deleted passage is ${lost.status}`)
      // Stored as such.
      const rows = await stored()
      if (rows.length !== 3) problems.push(`${rows.length} stored`)
      // The Notes tab lists it under Couldn't place, with Re-attach.
      keyOnApp('a', { code: 'KeyA', metaKey: true, shiftKey: true })
      const panel = await waitFor('notes', () =>
        document.querySelector<HTMLElement>('.navigator .notes'),
      )
      await settled(300)
      const lostRow = panel.querySelector<HTMLElement>(`.unplaced[data-annotation="${ids.pistol}"]`)
      if (!lostRow || !/Couldn’t place/.test(panel.textContent ?? ''))
        problems.push('not under Couldn’t place')
      lostRow?.querySelector<HTMLButtonElement>('.reattach')?.click()
      await settled(300)
      if (!document.querySelector('.reattach[role="status"]')) problems.push('no Re-attach prompt')
      if (document.querySelector('.navigator') && !document.querySelector('.navigator.floating'))
        key('Escape', { code: 'Escape' })
      await settled(300)
      await selectPhrase('With a philosophical flourish Cato throws himself upon his sword')
      if (!barButton(/attach here/i)) {
        problems.push(`no Attach here (bar: ${selBar()?.textContent?.trim()})`)
        return problems.join('; ')
      }
      await pressBar(/attach here/i)
      const re = annotationsOf().get(ids.pistol)!
      if (
        re.status !== 'anchored' ||
        re.quote.exact !== 'With a philosophical flourish Cato throws himself upon his sword'
      )
        problems.push(`re-attached as ${re.status} “${re.quote.exact}”`)
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'A9-w3c-roundtrip',
    description: 'Every stored annotation round-trips through W3C Web Annotation JSON without loss',
    run: async () => {
      const { toW3C, fromW3C } = await import('../lib/annotations/model')
      const list = annotationsOf().items
      if (!list.length) return 'nothing to round-trip'
      const bad = list.filter(
        (a) =>
          JSON.stringify(fromW3C(JSON.parse(JSON.stringify(toW3C(a, 'urn:x'))), a.bookId)) !==
          JSON.stringify(a),
      )
      return bad.length ? `${bad.length} changed` : 'ok'
    },
  })

  // ---------------------------------------------------------------- Phase 6: reading settings (Aa)
  const aa = () => document.querySelector<HTMLElement>('.aa[role="dialog"]')
  const openAa = async () => {
    if (!aa()) hooks.run?.('reader.settings')
    return waitFor('Aa', aa)
  }
  const aaRadio = (group: string, label: string) =>
    Array.from(
      aa()!.querySelectorAll<HTMLButtonElement>(`[aria-labelledby="aa-${group}"] [role="radio"]`),
    ).find((b) => b.textContent?.trim().endsWith(label))!
  const bodyFontPx = () => parseFloat(getComputedStyle(pageDoc().doc.body).fontSize)
  const bodyLineHeight = () => {
    const cs = getComputedStyle(pageDoc().doc.body.querySelector('p') ?? pageDoc().doc.body)
    return parseFloat(cs.lineHeight) / parseFloat(cs.fontSize)
  }
  const resetReading = async () => {
    for (const [k, v] of [
      ['fontPx', '19'],
      ['lineSpacing', 'default'],
      ['theme', 'auto'],
    ])
      await invoke('setting_set', { key: k, value: v })
  }

  checks.push({
    id: 'L4-aa-scope',
    description:
      'Aa: size, theme and line spacing change all books, layout this book only; each row names its scope; the choices persist',
    run: async () => {
      const problems: string[] = []
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      const pop = await openAa()
      const labels = Array.from(pop.querySelectorAll('.lbl')).map((l) => l.textContent?.trim())
      if (
        labels.join(' | ') !==
        'Text size19 px · All books | ThemeAll books | Line spacingAll books | LayoutThis book'
      )
        problems.push(`rows: ${labels.join(' | ')}`)
      pop.querySelector<HTMLButtonElement>('.step.large')!.click()
      await settled(500)
      if (bodyFontPx() !== 20) problems.push(`text at ${bodyFontPx()} px after A+`)
      aaRadio('theme', 'Sepia').click()
      await settled(500)
      if (
        getComputedStyle(document.querySelector('.reader')!)
          .getPropertyValue('--ground')
          .trim()
          .toLowerCase() !== '#f1e6d2'
      )
        problems.push('the theme did not change to Sepia')
      aaRadio('spacing', 'Loose').click()
      await settled(500)
      if (Math.abs(bodyLineHeight() - 1.75) > 0.051)
        problems.push(`line height ${bodyLineHeight().toFixed(2)} after Loose`)
      aaRadio('layout', 'Scroll').click()
      await settled(1200)
      if (reader()!.engine.mode !== 'scroll') problems.push('layout did not switch to Scroll')
      key('Escape', { code: 'Escape' })
      await settled(300)
      // Another book: the same size, theme and spacing; its own layout.
      await backToLibrary()
      await openFromLibrary(/Notes and images/)
      if (bodyFontPx() !== 20) problems.push(`other book at ${bodyFontPx()} px`)
      if (Math.abs(bodyLineHeight() - 1.75) > 0.051) problems.push('other book lost Loose')
      if (reader()!.engine.mode !== 'pages') problems.push('layout leaked to another book')
      if (document.documentElement.dataset.theme !== 'sepia')
        problems.push(`app theme ${document.documentElement.dataset.theme}`)
      // Reopening (the settings come from the store, as after a restart).
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      if (reader()!.engine.mode !== 'scroll') problems.push('this book forgot Scroll')
      const again = await openAa()
      if (aaRadio('theme', 'Sepia').getAttribute('aria-checked') !== 'true')
        problems.push('Sepia not shown chosen')
      aaRadio('layout', 'Pages').click()
      await settled(1000)
      key('Escape', { code: 'Escape' })
      void again
      await resetReading()
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'K8-text-size',
    description:
      '⌘+ ⌘− ⌘0 change the text size anywhere: 12 steps to 32 px, then on to 48 px; ⌘0 resets to 19 px',
    run: async () => {
      const problems: string[] = []
      const press = async (code: string, n = 1) => {
        for (let i = 0; i < n; i++)
          keyOnApp(code === 'Equal' ? '=' : code === 'Minus' ? '-' : '0', { code, metaKey: true })
        await settled(600)
      }
      await press('Digit0')
      await press('Equal')
      if (bodyFontPx() !== 20) problems.push(`⌘+ gave ${bodyFontPx()}`)
      await press('Minus', 2)
      if (bodyFontPx() !== 18) problems.push(`⌘− ⌘− gave ${bodyFontPx()}`)
      await press('Equal', 20)
      if (bodyFontPx() !== 48) problems.push(`the top is ${bodyFontPx()}`)
      await press('Digit0')
      if (bodyFontPx() !== 19) problems.push(`⌘0 gave ${bodyFontPx()}`)
      await resetReading()
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'P22-font-change-place',
    description:
      'P§22: after a text-size or line-spacing change the reading position stays on screen (anchor-based reflow)',
    run: async () => {
      const problems: string[] = []
      const engine = reader()!.engine
      await engine.goTo(engine.chapterCount > 6 ? 6 : 1)
      await settled(700)
      for (let i = 0; i < 3; i++) await turnBy(() => key('ArrowRight'), 'next')
      const start = engine.view.lastLocation?.range.cloneRange()
      if (!start) return 'no location'
      start.collapse(true)
      const onScreen = () => {
        const r = engine.view.lastLocation?.range
        return (
          !!r &&
          r.startContainer.ownerDocument === start.startContainer.ownerDocument &&
          r.compareBoundaryPoints(Range.START_TO_START, start) <= 0 &&
          r.compareBoundaryPoints(Range.START_TO_END, start) >= 0
        )
      }
      const step = async (what: string, change: () => void | Promise<unknown>) => {
        await change()
        await settled(900)
        if (!onScreen()) problems.push(`${what}: the place moved to ${loc()?.fraction.toFixed(4)}`)
      }
      try {
        for (let i = 0; i < 4; i++)
          await step(`⌘+ ×${i + 1}`, () => keyOnApp('=', { code: 'Equal', metaKey: true }))
        await step('⌘0', () => keyOnApp('0', { code: 'Digit0', metaKey: true }))
        await step('⌘− ×2', () => {
          keyOnApp('-', { code: 'Minus', metaKey: true })
          keyOnApp('-', { code: 'Minus', metaKey: true })
        })
        // As from the Settings window (G2): another source, so the reader applies it.
        const { changeSetting } = await import('../app/settingsSync')
        await step('line spacing', () => changeSetting('lineSpacing', 'loose', crypto.randomUUID()))
        await changeSetting('lineSpacing', 'default', crypto.randomUUID())
      } finally {
        await resetReading()
        await settled(900)
      }
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'L18-E2-aa-hints',
    description:
      'Aa hints: a book over 30% code or tables suggests Scroll (L18); a fixed-layout book keeps only theme, with one line of explanation (E2)',
    run: async () => {
      const problems: string[] = []
      await backToLibrary()
      await openFromLibrary(/Code and tables/)
      // The share is measured once, in idle time, and kept.
      for (let i = 0; i < 100; i++) {
        const known = await invoke<string | null>('setting_get', {
          key: `codeShare:${reader()!.bookId}`,
        })
        if (known !== null) break
        await sleep(100)
      }
      let pop = await openAa()
      if (!pop.querySelector('.hint')?.textContent?.includes('Scroll may read better'))
        problems.push('no Scroll hint')
      key('Escape', { code: 'Escape' })
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      pop = await openAa()
      if (pop.querySelector('.hint')) problems.push('a hint on Moby-Dick')
      key('Escape', { code: 'Escape' })
      await backToLibrary()
      await openFromLibrary(/blanche/i)
      pop = await openAa()
      if (pop.querySelector('#aa-size') || pop.querySelector('[aria-labelledby="aa-spacing"]'))
        problems.push('typography controls on a fixed-layout book')
      if (!pop.querySelector('.hint')?.textContent?.includes('fixed pages'))
        problems.push('no fixed-layout line')
      key('Escape', { code: 'Escape' })
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  // ---------------------------------------------------------------- Phase 6: library (E6–E10)
  const tiles = () =>
    Array.from(document.querySelectorAll<HTMLElement>('.library .tile')).map(
      (t) => t.querySelector('.tt')?.textContent?.trim() ?? '',
    )
  const typeSearch = async (q: string) => {
    const field = document.querySelector<HTMLInputElement>('#lib-search')!
    field.value = q
    field.dispatchEvent(new Event('input', { bubbles: true }))
    await settled(200)
  }
  const menuNow = () => hooks.contextMenu
  const tileMenu = async (title: RegExp) => {
    hooks.contextMenu = undefined
    const tile = libraryTile(title)!.closest('.tile')!
    tile.querySelector<HTMLButtonElement>('.more')!.click()
    await settled(100)
    return hooks.contextMenu!
  }

  checks.push({
    id: 'E6-library',
    description:
      'Library: Continue reading shows the last book with its chapter and Resume reading; All books has every book with its count; search and sort order the grid',
    run: async () => {
      const problems: string[] = []
      await backToLibrary()
      const listed = await invoke<{ title: string }[]>('library_list')
      if (tiles().length !== listed.length)
        problems.push(`${tiles().length} tiles for ${listed.length} books`)
      const count = document.querySelector('.library .all-head .count')?.textContent
      if (count !== String(listed.length)) problems.push(`count ${count}`)
      const cr = document.querySelector<HTMLElement>('.library .continue')
      if (!cr) problems.push('no Continue reading')
      else {
        if (!/Moby Dick(?!;)/.test(cr.querySelector('.big-title')?.textContent ?? ''))
          problems.push(`current book “${cr.querySelector('.big-title')?.textContent}”`)
        if (!cr.querySelector('.resume')) problems.push('no Resume reading')
        if (!/Loomings|·/.test(cr.querySelector('.where')?.textContent ?? ''))
          problems.push(`where “${cr.querySelector('.where')?.textContent}”`)
        if (!/^Opened /.test(cr.querySelector('.opened')?.textContent ?? ''))
          problems.push('no opened time')
      }
      await typeSearch('sous le')
      if (tiles().length !== 1 || !/Sous le vent/.test(tiles()[0]))
        problems.push(`search: ${tiles().join(' | ')}`)
      if (document.querySelector('.library .continue'))
        problems.push('Continue reading shown while searching')
      await typeSearch('xyzzy')
      if (!document.querySelector('.library .no-matches')) problems.push('no “No books match”')
      await typeSearch('')
      hooks.contextMenu = undefined
      document.querySelector<HTMLButtonElement>('.library .sort')!.click()
      await settled(100)
      menuNow()?.run('Title')
      await settled(300)
      const { titleKey } = await import('../lib/library/order')
      const keys = tiles().map(titleKey)
      const sorted = [...keys].sort(
        new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' }).compare,
      )
      if (keys.join('|') !== sorted.join('|'))
        problems.push(`title order: ${tiles().slice(0, 5).join(' | ')}…`)
      if ((await invoke<string | null>('setting_get', { key: 'librarySort' })) !== 'title')
        problems.push('sort not remembered')
      hooks.contextMenu = undefined
      document.querySelector<HTMLButtonElement>('.library .sort')!.click()
      await settled(100)
      menuNow()?.run('Recent')
      await settled(300)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'E8-return-resumes',
    description: 'Return in the library opens the current book at its saved position',
    run: async () => {
      await backToLibrary()
      document.body.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
      )
      await waitFor('reader', () => loc()?.cfi)
      await settled(600)
      const books = await invoke<{ id: string; title: string }[]>('library_list')
      const title = books.find((b) => b.id === reader()?.bookId)?.title ?? ''
      return /Moby Dick(?!;)/.test(title) ? 'ok' : `Return opened “${title}”`
    },
  })

  checks.push({
    id: 'E7-remove-undo',
    description:
      'The item menu has Book info, Show in Finder and Remove; Remove is immediate with Undo (⌘Z and the message)',
    run: async () => {
      const problems: string[] = []
      await backToLibrary()
      const menu = await tileMenu(/Notes and images/)
      if (menu.labels.join(', ') !== 'Book info, Show in Finder, Remove')
        problems.push(`menu: ${menu.labels.join(', ')}`)
      menu.run('Remove')
      await settled(500)
      if (libraryTile(/Notes and images/)) problems.push('not removed')
      if (document.querySelector('dialog[open]')) problems.push('asked to confirm')
      if (hooks.messages!.current?.text !== 'Removed “Notes and images”')
        problems.push(`message “${hooks.messages!.current?.text}”`)
      keyOnApp('z', { code: 'KeyZ', metaKey: true })
      await settled(600)
      if (!libraryTile(/Notes and images/)) problems.push('⌘Z did not bring it back')
      // Again, with the message's Undo.
      ;(await tileMenu(/Notes and images/)).run('Remove')
      await settled(500)
      hooks.messages!.act(hooks.messages!.current!.id)
      await settled(600)
      if (!libraryTile(/Notes and images/)) problems.push('Undo did not bring it back')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'E10-book-info',
    description:
      'Book info: a read-only modal sheet with the metadata and accessibility metadata; Esc closes it',
    run: async () => {
      const problems: string[] = []
      await backToLibrary()
      ;(await tileMenu(/Moby Dick(?!;)/)).run('Book info')
      const sheet = await waitFor('info', () =>
        document.querySelector<HTMLElement>('dialog[open] .info'),
      )
      await waitFor(
        'details',
        () => sheet.querySelector('dd') && /File size/.test(sheet.textContent ?? ''),
      )
      const text = sheet.textContent ?? ''
      for (const want of ['Moby Dick', 'Herman Melville', 'Language', 'English', 'Accessibility'])
        if (!text.includes(want)) problems.push(`no “${want}”`)
      if (sheet.querySelector('input, textarea, [contenteditable]'))
        problems.push('editable fields')
      ;(document.activeElement ?? document.body).dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'Escape',
          code: 'Escape',
          bubbles: true,
          cancelable: true,
        }),
      )
      await settled(300)
      if (document.querySelector('dialog[open] .info')) problems.push('Esc did not close it')
      await openFromLibrary(/Moby Dick(?!;)/)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'E3-damaged-card',
    description:
      'Opening a book with damaged chapters shows the card (how many of how many, Read anyway, Show file, Remove) until Read anyway',
    run: async () => {
      const problems: string[] = []
      await backToLibrary()
      const id = (await invoke<{ id: string; title: string }[]>('library_list')).find((b) =>
        /missing/i.test(b.title),
      )?.id
      if (!id) return 'no damaged book in the library'
      // Forget any earlier Read anyway, as a fresh library has none.
      await invoke('setting_set', { key: `damageAck:${id}`, value: '' })
      const tile = Array.from(document.querySelectorAll<HTMLElement>('.library .tile')).find(
        (t) => t.dataset.book === id,
      )!
      tile.querySelector<HTMLButtonElement>('.open')!.click()
      const card = await waitFor('card', () =>
        document.querySelector<HTMLElement>('dialog[open] .card'),
      )
      await waitFor(
        'counts',
        () => /of \d+ chapters? (is|are) damaged/.test(card.textContent ?? ''),
        3000,
      ).catch(() => problems.push(`text “${card.querySelector('.body')?.textContent}”`))
      const labels = Array.from(card.querySelectorAll('button')).map((b) => b.textContent?.trim())
      if (labels.join(', ') !== 'Read anyway, Show file, Remove')
        problems.push(`buttons ${labels.join(', ')}`)
      card.querySelector<HTMLButtonElement>('.primary')!.click()
      await waitFor('reader', () => loc()?.cfi)
      await backToLibrary()
      Array.from(document.querySelectorAll<HTMLElement>('.library .tile'))
        .find((t) => t.dataset.book === id)!
        .querySelector<HTMLButtonElement>('.open')!
        .click()
      await settled(800)
      if (document.querySelector('dialog[open] .card'))
        problems.push('the card came back after Read anyway')
      await waitFor('reader', () => loc()?.cfi)
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'G2-settings',
    description:
      'Settings… (⌘,) asks for the Settings window; a change there (text size, announcements) reaches the open book at once and is saved',
    run: async () => {
      const problems: string[] = []
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      const before = hooks.settingsOpened ?? 0
      keyOnApp(',', { code: 'Comma', metaKey: true })
      await settled(200)
      if ((hooks.settingsOpened ?? 0) !== before + 1) problems.push('⌘, did not ask for the window')
      // The window's page, mounted here (the harness drives one WebView).
      const { default: Preferences } = await import('../prefs/Preferences.svelte')
      const { unmount } = await import('svelte')
      const host = document.createElement('div')
      host.style.cssText = 'position:fixed;inset:0;z-index:100;background:var(--ground)'
      document.body.append(host)
      const prefs = mount(Preferences, { target: host })
      try {
        await settled(500)
        host.querySelector<HTMLButtonElement>('#prefs-reading')!.click()
        await settled(200)
        const size = host.querySelector<HTMLSelectElement>('#prefs-size')!
        size.value = '22'
        size.dispatchEvent(new Event('change', { bubbles: true }))
        await settled(800)
        if (bodyFontPx() !== 22) problems.push(`the book is at ${bodyFontPx()} px`)
        const announce = Array.from(host.querySelectorAll<HTMLButtonElement>('[role="switch"]'))[1]
        announce.click()
        await settled(300)
        if (
          (await invoke<string | null>('setting_get', { key: 'pageTurnAnnouncements' })) !== 'off'
        )
          problems.push('announcements not saved')
        announce.click()
        size.value = '19'
        size.dispatchEvent(new Event('change', { bubbles: true }))
        await settled(600)
      } finally {
        void unmount(prefs)
        host.remove()
      }
      if (bodyFontPx() !== 19) problems.push(`not reset: ${bodyFontPx()} px`)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  // ---------------------------------------------------------------- Phase 7: extensions
  const pkg = (name: string) => invoke<string>('spike_corpus_path', { name: `${name}.linenext` })
  const ext = () => hooks.extensions!
  /** Chapter 1, where the phrases the checks select are. */
  const toLoomings = async () => {
    const sections = reader()!.engine.book!.sections
    await reader()!.engine.goTo(sections.findIndex((x) => x.id.endsWith('/chapter-1.xhtml')))
    await settled(600)
  }
  /** The Settings page, mounted here (the harness drives one WebView), on Extensions. */
  const withSettings = async <T>(fn: (root: HTMLElement) => Promise<T>): Promise<T> => {
    const { default: Preferences } = await import('../prefs/Preferences.svelte')
    const { unmount } = await import('svelte')
    const root = document.createElement('div')
    root.style.cssText = 'position:fixed;inset:0;z-index:100;background:var(--ground);overflow:auto'
    document.body.append(root)
    const prefs = mount(Preferences, { target: root })
    try {
      await settled(400)
      root.querySelector<HTMLButtonElement>('#prefs-extensions')!.click()
      await settled(400)
      return await fn(root)
    } finally {
      void unmount(prefs)
      root.remove()
    }
  }
  /** Install through the pane and the consent sheet (G1), as a reader does. */
  const installViaSettings = async (root: HTMLElement, name: string) => {
    const path = await pkg(name)
    hooks.pickExtensionFile = async () => path
    Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
      .find((b) => b.textContent?.includes('Install from file'))!
      .click()
    const sheet = await waitFor('sheet', () =>
      document.querySelector<HTMLElement>(
        'dialog[open] [data-install-sheet], dialog[open] [data-install-error]',
      ),
    )
    await settled(200)
    const text = sheet.textContent ?? ''
    if (sheet.hasAttribute('data-install-sheet'))
      Array.from(sheet.querySelectorAll<HTMLButtonElement>('button')).at(-1)!.click()
    else sheet.querySelector<HTMLButtonElement>('button')!.click()
    await settled(800)
    return text
  }

  checks.push({
    id: 'P5-install-consent',
    description:
      'Install from file shows what an extension can access and adds (G1), then installs it; an extension for an unsupported API major is refused with a reason',
    run: async () => {
      const problems: string[] = []
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      await withSettings(async (root) => {
        const consent = await installViaSettings(root, 'dictionary')
        for (const want of [
          'Install “Dictionary”?',
          'Not verified by Linen',
          'Read the text you select',
          'Connect to api.dictionaryapi.dev',
          'Navigator tab',
        ])
          if (!consent.includes(want)) problems.push(`sheet lacks “${want}”`)
        for (const name of ['night-owl', 'hostile', 'hang', 'crash'])
          await installViaSettings(root, name)
        const future = await installViaSettings(root, 'future')
        if (!/can’t be installed/.test(future) || !/\^2\.0/.test(future) || !/1\.x/.test(future))
          problems.push(`future: “${future}”`)
        await settled(300)
        const listed = Array.from(root.querySelectorAll('[data-extension]')).map((e) =>
          e.getAttribute('data-extension'),
        )
        for (const id of [
          'app.linen.markdown-export',
          'org.example.dictionary',
          'org.example.night-owl',
          'test.hostile',
        ])
          if (!listed.includes(id)) problems.push(`${id} not listed`)
        if (listed.includes('test.future')) problems.push('the future extension was installed')
        const builtin =
          root.querySelector('[data-extension="app.linen.markdown-export"]')?.textContent ?? ''
        if (!builtin.includes('Built-in') || builtin.includes('Remove'))
          problems.push('Markdown Export not shown as built-in')
      })
      await waitFor('host reloaded', () => ext().get('org.example.dictionary'), 5000).catch(() =>
        problems.push('the reader did not pick it up'),
      )
      if (
        !hooks.registry!.available().some((c) => c.id === 'extension:org.example.dictionary:define')
      )
        problems.push('Define is not in ⌘K')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'P5-incompatible-at-load',
    description:
      'An installed extension built for an unsupported major is turned off at load, with the reason shown',
    run: async () => {
      await invoke('spike_install_unchecked', { path: await pkg('future') })
      await ext().load()
      const x = ext().get('test.future')
      const problems: string[] = []
      if (!x?.incompatible) problems.push('not marked incompatible')
      if (ext().active.some((a) => a.manifest.id === 'test.future')) problems.push('it is active')
      await withSettings(async (root) => {
        const row = root.querySelector('[data-extension="test.future"]')?.textContent ?? ''
        if (!/Turned off: it was built for Linen API \^2\.0/.test(row))
          problems.push(`row: “${row}”`)
        root
          .querySelector<HTMLElement>('[data-extension="test.future"]')
          ?.querySelector<HTMLButtonElement>('.link')
          ?.click()
        await settled(300)
        document
          .querySelector<HTMLButtonElement>('dialog[open] [data-remove-sheet] .primary')
          ?.click()
        await settled(600)
      })
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'P10-selection-actions',
    description:
      'Extension actions sit behind “⋯” in the selection bar (their when-clauses decide); Define runs, with the selection only through its permission, and its Navigator tab shows',
    run: async () => {
      const problems: string[] = []
      await toLoomings()
      await selectPhrase('drizzly')
      await waitFor('bar', selBar)
      const more = barButton(/more actions from extensions/i)
      if (!more) return 'no ⋯ in the bar'
      more.click()
      await settled(200)
      const items = Array.from(selBar()!.querySelectorAll('.ext-menu [role="menuitem"]')).map((b) =>
        b.textContent?.trim(),
      )
      if (!items.includes('Define') || !items.includes('Manage extensions…'))
        problems.push(`menu: ${items.join(', ')}`)
      Array.from(selBar()!.querySelectorAll<HTMLButtonElement>('.ext-menu [role="menuitem"]'))
        .find((b) => b.textContent?.trim() === 'Define')!
        .click()
      await waitFor(
        'tab',
        () =>
          document.querySelector('.navigator iframe[src^="linen-ext://org.example.dictionary/"]'),
        5000,
      ).catch(() => problems.push('the Definitions tab did not open'))
      let saved: string | null = null
      for (let i = 0; i < 60 && !(saved && !JSON.parse(saved).loading); i++) {
        saved = await invoke<string | null>('extension_storage_get', {
          id: 'org.example.dictionary',
          key: 'last',
        })
        await sleep(250)
      }
      const r = saved ? JSON.parse(saved) : null
      if (r?.word !== 'drizzly') problems.push(`looked up ${JSON.stringify(r)?.slice(0, 80)}`)
      log(`P10: dictionary result ${JSON.stringify(r)?.slice(0, 160)}`)
      key('Escape', { code: 'Escape' })
      await settled(300)
      // More than three words: Define's when-clause hides it.
      await selectPhrase('damp, drizzly November in my soul')
      barButton(/more actions from extensions/i)?.click()
      await settled(200)
      const long = Array.from(selBar()?.querySelectorAll('.ext-menu [role="menuitem"]') ?? []).map(
        (b) => b.textContent?.trim(),
      )
      if (long.includes('Define')) problems.push('Define offered for six words')
      pageDoc().doc.getSelection()?.removeAllRanges()
      key('Escape', { code: 'Escape' })
      await settled(300)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'P2-hostile-extension',
    description:
      'A hostile extension reaches no undeclared host, no Tauri command, no book text or selection without permission, no other extension’s storage, and no more than 10 MB',
    run: async () => {
      await invoke('spike_canary_clear')
      await invoke('extension_storage_set', {
        id: 'org.example.dictionary',
        key: 'secret-of-another-extension',
        value: 'mine',
      })
      const r = (await ext().invoke('test.hostile', 'probe')) as Record<string, string>
      await sleep(500)
      const canary = await invoke<{ http: string[]; ipc: string[] }>('spike_canary_log')
      log(`P2 hostile: ${JSON.stringify(r)}; canary ${JSON.stringify(canary)}`)
      const problems: string[] = []
      if (!/^reached/.test(r.declaredHost)) problems.push(`declared host: ${r.declaredHost}`)
      for (const k of [
        'undeclaredHost',
        'undeclaredPort',
        'redirectTrick',
        'directFetch',
        'bookText',
        'bookSelection',
        'annotations',
        'library',
        'tauri',
        'unknownCall',
        'storageQuota',
      ])
        if (!/^refused/.test(r[k])) problems.push(`${k}: ${r[k]}`)
      if (!/nothing/.test(r.otherStorage)) problems.push(`otherStorage: ${r.otherStorage}`)
      if (!/10 MB/.test(r.storageQuota)) problems.push(`quota: ${r.storageQuota}`)
      const hits = canary.http.filter((h) => h.includes('/p7-'))
      if (hits.length !== 1 || !hits[0].includes('/p7-declared'))
        problems.push(`canary: ${hits.join(', ')}`)
      if (canary.ipc.length) problems.push(`IPC: ${canary.ipc.join(', ')}`)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'P6-watchdog',
    description:
      'A stuck extension times out (10 s), is marked “Not responding” with Restart in the selection menu, is suspended after three failures, and reading carries on',
    run: async () => {
      const problems: string[] = []
      await toLoomings()
      const stall = () =>
        ext()
          .invoke('test.hang', 'stall')
          .then(
            () => 'answered',
            (e: Error) => e.message,
          )
      const t0 = performance.now()
      const pending = stall()
      // Reading carries on while it spins.
      const before = where()
      await settled(500)
      key('ArrowRight', { code: 'ArrowRight' })
      await settled(700)
      if (where() === before) problems.push('a page turn waited for the extension')
      const first = await pending
      const took = performance.now() - t0
      if (!/did not answer within 10 s/.test(first) || took < 9500 || took > 12500)
        problems.push(`first: ${first} after ${Math.round(took)} ms`)
      if (ext().status['test.hang'] !== 'not-responding')
        problems.push(`status ${ext().status['test.hang']}`)
      await selectPhrase('drizzly')
      barButton(/more actions from extensions/i)?.click()
      await settled(200)
      const menu = selBar()?.querySelector('.ext-menu')?.textContent ?? ''
      if (!/Stall\s*Not responding/.test(menu) || !menu.includes('Restart Hang'))
        problems.push(`menu: “${menu}”`)
      pageDoc().doc.getSelection()?.removeAllRanges()
      key('Escape', { code: 'Escape' })
      await stall()
      await stall()
      await settled(500)
      const x = ext().get('test.hang')
      if (!x?.suspended || ext().status['test.hang'] !== 'suspended')
        problems.push(`after three: suspended ${x?.suspended}, ${ext().status['test.hang']}`)
      // A crash at load counts too.
      const boom = await ext()
        .invoke('test.crash', 'boom')
        .then(
          () => 'ran',
          (e: Error) => e.message,
        )
      if (boom === 'ran') problems.push('the crashing extension ran')
      await ext().restart('test.hang')
      if (ext().status['test.hang'] !== 'idle' || ext().get('test.hang')?.suspended)
        problems.push('Restart did not clear it')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'P9-theme-pack',
    description:
      'A theme pack is listed with the built-in themes in Aa and applies to the book and the app',
    run: async () => {
      const problems: string[] = []
      const pop = await openAa()
      const night = aaRadio('theme', 'Night Owl')
      if (!night) return 'Night Owl is not in Aa'
      night.click()
      await settled(700)
      const ground = getComputedStyle(document.querySelector('.reader')!)
        .getPropertyValue('--ground')
        .trim()
        .toLowerCase()
      if (ground !== '#1a1712') problems.push(`reader ground ${ground}`)
      if (document.documentElement.dataset.theme !== 'extension')
        problems.push(`app theme ${document.documentElement.dataset.theme}`)
      void pop
      aaRadio('theme', 'Auto').click()
      await settled(500)
      key('Escape', { code: 'Escape' })
      await settled(300)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'P8-pinned-and-export',
    description:
      'A pinned extension’s commands lead the ⋯ menu (no top-bar slot); Notes › Export as Markdown saves the book’s highlights through the built-in extension',
    run: async () => {
      const problems: string[] = []
      await withSettings(async (root) => {
        root
          .querySelector<HTMLInputElement>('[data-extension="org.example.dictionary"] .pin input')!
          .click()
        await settled(500)
      })
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      await showControls()
      document.querySelector<HTMLButtonElement>('.more-button')!.click()
      const more = await waitFor('⋯ menu', () => document.querySelector<HTMLElement>('.more'))
      const firstGroup = more.querySelector('.group')?.textContent ?? ''
      if (!/Dictionary/.test(firstGroup ?? '') || !/Define/.test(more.textContent ?? ''))
        problems.push(`⋯ menu starts “${firstGroup}”`)
      key('Escape', { code: 'Escape' })
      await hideControls()
      // Export: a highlight, then Notes › Export as Markdown.
      await toLoomings()
      await selectPhrase(NOVEMBER)
      await pressBar(/highlight yellow/i)
      const out = `/tmp/linen-export-${Date.now()}.md`
      hooks.pickSavePath = async () => out
      keyOnApp('a', { code: 'KeyA', metaKey: true, shiftKey: true })
      const panel = await waitFor('notes', () =>
        document.querySelector<HTMLElement>('.navigator .notes'),
      )
      const button = await waitFor('export button', () =>
        panel.querySelector<HTMLButtonElement>('.export-button'),
      )
      if (!/via Markdown Export extension/.test(panel.querySelector('.export')?.textContent ?? ''))
        problems.push('no “via Markdown Export extension”')
      button.click()
      await waitFor('exported', () => hooks.messages!.current?.text === 'Exported', 12_000).catch(
        () => problems.push(`message “${hooks.messages!.current?.text}”`),
      )
      const written = await invoke<string>('spike_read_file', { path: out }).catch(
        (e) => `unreadable: ${e}`,
      )
      if (
        !written.startsWith('# Moby Dick') ||
        !written.includes('> damp, drizzly November in my soul')
      )
        problems.push(`file: ${written.slice(0, 120)}`)
      key('Escape', { code: 'Escape' })
      await settled(300)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'P7-safe-mode',
    description:
      'Safe mode (⇧ at launch or Restart without extensions) starts with every extension off; the next launch has them back',
    run: async () => {
      const safe = await invoke<boolean>('app_safe_mode')
      if (!safe) {
        // A normal run: the way back into safe mode is offered.
        const offered = await withSettings(
          async (root) =>
            Array.from(root.querySelectorAll('button')).some((b) =>
              b.textContent?.includes('Restart without extensions'),
            ) && (root.textContent ?? '').includes('holding ⇧'),
        )
        log('P7: normal launch; safe mode itself is checked by a LINEN_SAFE_MODE=1 run')
        return offered ? 'ok' : 'no Restart without extensions'
      }
      await ext().load()
      const problems: string[] = []
      if (ext().active.length) problems.push(`${ext().active.length} extensions active`)
      if (hooks.registry!.available().some((c) => c.extensionId))
        problems.push('extension commands in ⌘K')
      const banner = await withSettings(
        async (root) => root.querySelector('.banner')?.textContent ?? '',
      )
      if (!/without extensions/.test(banner)) problems.push(`banner “${banner}”`)
      log('P7: checked in a safe-mode launch')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  // ---------------------------------------------------------------- Phase 8: accessibility (D5)
  /** axe on the app's own UI (not the book, which is the publisher's), WCAG 2.2 A and AA. */
  const axeRun = async (where: string) => {
    const axe = (await import('axe-core')).default
    const r = await axe.run(document, {
      runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'] },
      iframes: false,
      resultTypes: ['violations'],
    })
    return r.violations
      .filter((v) => v.impact === 'serious' || v.impact === 'critical')
      .map((v) => {
        log(
          `axe ${where} ${v.id}: ${v.nodes
            .slice(0, 3)
            .map(
              (n) =>
                `${n.target.join(' ')} — ${(n.failureSummary ?? '').replace(/\s+/g, ' ').slice(0, 220)}`,
            )
            .join(' | ')}`,
        )
        return `${where}: ${v.id} (${v.nodes.length}) ${v.nodes[0]?.target.join(' ')}`
      })
  }

  checks.push({
    id: 'X-axe-states',
    description:
      'D5 (WCAG 2.2 AA): axe finds no serious or critical violation in the library, the reader and its layers, and Settings, in Paper and Night',
    run: async () => {
      const found: string[] = []
      // As the Settings window would: save it, and tell the app (a source of its own).
      const setTheme = async (value: string) => {
        await invoke('setting_set', { key: 'theme', value })
        await emit('settings-changed', { key: 'theme', value, source: 'harness' })
      }
      for (const theme of ['paper', 'night']) {
        await setTheme(theme)
        await backToLibrary()
        hooks.run?.('library.show')
        await settled(600)
        found.push(...(await axeRun(`${theme} library`)))
        await openFromLibrary(/Moby Dick(?!;)/)
        await hideControls()
        found.push(...(await axeRun(`${theme} reader`)))
        await showControls()
        found.push(...(await axeRun(`${theme} controls`)))
        for (const tab of ['contents', 'search', 'notes']) {
          hooks.run?.(
            tab === 'contents'
              ? 'navigator.contents'
              : tab === 'search'
                ? 'search.open'
                : 'navigator.notes',
          )
          await settled(700)
          found.push(...(await axeRun(`${theme} navigator ${tab}`)))
          key('Escape', { code: 'Escape' })
          await settled(400)
        }
        await openAa()
        await settled(300) // past its 140 ms fade-in (mid-fade text is half-transparent)
        found.push(...(await axeRun(`${theme} Aa`)))
        key('Escape', { code: 'Escape' })
        await settled(300)
        await selectPhrase('Call me Ishmael').catch(async () => {
          await reader()!.engine.goToTextStart()
          await settled(500)
          return selectPhrase('Call me Ishmael')
        })
        await waitFor('bar', selBar)
        found.push(...(await axeRun(`${theme} selection bar`)))
        await pressBar(/^note$/i)
        await waitFor('note', noteCard)
        found.push(...(await axeRun(`${theme} note card`)))
        noteCard()!
          .querySelector('textarea')!
          .dispatchEvent(
            new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
          )
        await settled(300)
        const made = annotationsOf().items.at(-1)
        if (made) annotationsOf().remove(made.id)
        keyOnApp('k', { code: 'KeyK', metaKey: true })
        await waitFor('palette', palette)
        found.push(...(await axeRun(`${theme} palette`)))
        await escModal()
        hooks.run?.('shortcuts.show')
        await settled(400)
        found.push(...(await axeRun(`${theme} cheat sheet`)))
        await escModal()
        found.push(
          ...(await withSettings(async (root) => {
            const out: string[] = []
            for (const section of [
              'general',
              'reading',
              'library',
              'extensions',
              'shortcuts',
              'about',
            ]) {
              root.querySelector<HTMLButtonElement>(`#prefs-${section}`)!.click()
              await settled(250)
              out.push(...(await axeRun(`${theme} settings ${section}`)))
            }
            return out
          })),
        )
      }
      await setTheme('auto')
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      return found.length ? found.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'X6-zoom',
    description:
      'X6: at 200% and 400% zoom the app takes its narrow form: one column, the Navigator floats, nothing scrolls sideways',
    run: async () => {
      const { getCurrentWebview } = await import('@tauri-apps/api/webview')
      const problems: string[] = []
      const sideways = (where: string) => {
        const el = document.scrollingElement ?? document.documentElement
        if (el.scrollWidth > el.clientWidth + 1)
          problems.push(`${where}: ${el.scrollWidth} > ${el.clientWidth}`)
      }
      try {
        for (const zoom of [2, 4]) {
          await getCurrentWebview().setZoom(zoom)
          await settled(1500)
          await backToLibrary()
          sideways(`${zoom * 100}% library`)
          await openFromLibrary(/Moby Dick(?!;)/)
          await settled(800)
          sideways(`${zoom * 100}% reader`)
          const frames = pageDoc().doc.defaultView!
          const columns = getComputedStyle(frames.document.documentElement).columnCount
          if (columns !== 'auto' && Number(columns) > 1)
            problems.push(`${zoom * 100}%: ${columns} columns`)
          hooks.run?.('navigator.contents')
          await settled(700)
          if (!document.querySelector('.navigator.floating'))
            problems.push(`${zoom * 100}%: the Navigator docked`)
          sideways(`${zoom * 100}% navigator`)
          key('Escape', { code: 'Escape' })
          await settled(400)
        }
      } finally {
        await getCurrentWebview().setZoom(1)
        await settled(1500)
      }
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'X5-text-spacing',
    description: 'X5 (WCAG 1.4.12): user text spacing re-paginates the chapter; no text is clipped',
    run: async () => {
      const engine = reader()!.engine
      await toLoomings()
      const before = engine.view.renderer.pages
      engine.setExtraStyles(
        '* { line-height: 1.5 !important; letter-spacing: 0.12em !important; word-spacing: 0.16em !important; } p { margin-bottom: 2em !important; }',
      )
      await settled(1500)
      const after = engine.view.renderer.pages
      const doc = pageDoc().doc
      const clipped = Array.from(
        doc.querySelectorAll<HTMLElement>('p, h1, h2, h3, li, blockquote'),
      ).filter((el) => el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1)
      engine.setExtraStyles('')
      await settled(1200)
      const problems: string[] = []
      if (!(after > before)) problems.push(`pages ${before} → ${after}`)
      if (clipped.length)
        problems.push(`${clipped.length} clipped, e.g. ${clipped[0].textContent?.slice(0, 40)}`)
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  // ---------------------------------------------------------------- Phase 8: EPUB edge cases
  checks.push({
    id: 'I16-vertical',
    description:
      'I16: vertical writing (vertical-rl) paginates right to left with ruby kept; ← turns forward; Scroll runs sideways',
    run: async () => {
      const problems: string[] = []
      await backToLibrary()
      await openFromLibrary(/草枕/)
      const engine = reader()!.engine
      await engine.goTo(3)
      await settled(900)
      const doc = pageDoc().doc
      const mode = getComputedStyle(doc.documentElement).writingMode
      if (!mode.startsWith('vertical')) problems.push(`writing mode ${mode}`)
      if (doc.querySelector('ruby') && !doc.querySelector('rt')) problems.push('ruby text lost')
      const before = loc()!.fraction
      key('ArrowLeft', { code: 'ArrowLeft' })
      await settled(700)
      const afterLeft = loc()!.fraction
      if (!(afterLeft > before))
        problems.push(`← went ${before.toFixed(4)} → ${afterLeft.toFixed(4)}`)
      key('ArrowRight', { code: 'ArrowRight' })
      await settled(700)
      if (!(loc()!.fraction < afterLeft)) problems.push('→ did not go back')
      hooks.run?.('layout.scroll')
      await settled(1500)
      // Sideways Scroll is foliate's scrolled flow: the vertical text scrolls horizontally.
      const flow = engine.view.renderer.getAttribute('flow')
      const start = loc()!.fraction
      for (let i = 0; i < 6; i++) {
        engine.scrollPixels(300)
        await sleep(120)
      }
      await settled(700)
      const scrolled = loc()!.fraction
      log(
        `I16 scroll: mode ${engine.mode}, flow ${flow}, ${start.toFixed(4)} → ${scrolled.toFixed(4)}`,
      )
      if (engine.mode !== 'scroll') problems.push(`Scroll mode is ${engine.mode}`)
      else if (flow !== 'scrolled') problems.push(`flow ${flow}`)
      else if (!(scrolled > start)) problems.push('Scroll does not move through the text')
      else {
        // Past the end of the chapter, the next one follows.
        const section = loc()!.sectionIndex
        for (let i = 0; i < 80 && loc()!.sectionIndex === section; i++) {
          engine.scrollPixels(1200)
          await sleep(350)
        }
        await settled(600)
        if (loc()!.sectionIndex <= section)
          problems.push(`Scroll stayed in section ${section} (${loc()!.fraction.toFixed(4)})`)
        else if (engine.view.renderer.getAttribute('flow') !== 'scrolled')
          problems.push('the next chapter is not in the sideways flow')
      }
      hooks.run?.('layout.pages')
      await settled(1200)
      if (engine.mode !== 'pages') problems.push('Pages mode did not come back')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'D7-webkit-message',
    description:
      'D7-WebKit: on WebKit older than Safari 16.4, opening a book shows “Books need a newer Safari” instead of a blank reader',
    run: async () => {
      await backToLibrary()
      hooks.webkitTooOld = true
      try {
        const tile = await waitFor('library tile', () => libraryTile(/Moby Dick(?!;)/))
        tile.click()
        const card = await waitFor('the Safari card', () =>
          document.querySelector<HTMLElement>('dialog[open] [data-webkit-too-old]'),
        )
        await settled(300)
        const problems: string[] = []
        if (reader()) problems.push('the reader opened')
        if (!/Safari 16\.4/.test(card.textContent ?? '')) problems.push('no version named')
        if (!card.querySelector('button.primary')) problems.push('no Software Update button')
        // Not now closes it (the primary button would open System Settings).
        card.querySelector<HTMLButtonElement>('button.plain')!.click()
        await settled(400)
        if (document.querySelector('dialog[open] [data-webkit-too-old]'))
          problems.push('Not now left the card open')
        return problems.length ? problems.join('; ') : 'ok'
      } finally {
        hooks.webkitTooOld = false
      }
    },
  })

  checks.push({
    id: 'D1-crash-log',
    description:
      'D1: an uncaught error in the page is written to the crash log on this Mac (nothing is sent); Settings › About can reveal it',
    run: async () => {
      const marker = `e2e crash-log probe ${Date.now()}`
      setTimeout(() => {
        throw new Error(marker)
      })
      void Promise.reject(new Error(`${marker} (rejection)`))
      await settled(800)
      const text = await invoke<string>('spike_crash_log')
      const problems: string[] = []
      if (!text.includes(marker))
        problems.push(
          `the uncaught error is not in the log (${text.split('\n--- ').length - 1} entries; last: ${text.slice(-300)})`,
        )
      if (!text.includes(`${marker} (rejection)`)) problems.push('the rejection is not in the log')
      if (!/Linen \d+\.\d+\.\d+ · macOS \d+/.test(text)) problems.push('no version line')
      if (!(await invoke<boolean>('crash_log_exists'))) problems.push('crash_log_exists says no')
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'budget-open',
    description:
      'Open a book to its first page in < 500 ms (§6.4; click → first location, median and p95 of 5)',
    run: async () => {
      const times: number[] = []
      for (let i = 0; i < 5; i++) {
        await backToLibrary()
        const row = await waitFor('row', () => libraryTile(/Moby Dick(?!;)/))
        const previous = reader()?.engine
        const t0 = performance.now()
        row.click()
        while (!(reader() && reader()!.engine !== previous && loc()?.pages))
          await new Promise((r) => requestAnimationFrame(r))
        times.push(performance.now() - t0)
        await settled(500)
      }
      times.sort((a, b) => a - b)
      const [median, p95] = [times[2], times[4]].map(Math.round)
      log(`budget-open: median ${median} ms, p95 ${p95} ms`)
      return p95 < 500 ? 'ok' : `median ${median} ms, p95 ${p95} ms`
    },
  })

  checks.push({
    id: 'budget-reflow',
    description:
      'Reflow after a resize: < 150 ms from the end of the 120 ms debounce to the page (§6.4)',
    run: async () => {
      const w = getCurrentWindow()
      const size = await w.innerSize()
      const factor = await w.scaleFactor()
      const engine = reader()!.engine
      const times: number[] = []
      for (const width of [1000, 1280, 900, 1200]) {
        let relocated = 0
        const off = engine.onRelocate(() => (relocated = performance.now()))
        const t0 = performance.now()
        await w.setSize(new LogicalSize(width, size.height / factor))
        await sleep(900)
        off()
        if (relocated) times.push(relocated - t0 - 120)
      }
      await w.setSize(new LogicalSize(size.width / factor, size.height / factor))
      await settled(900)
      const worst = Math.round(Math.max(...times))
      return times.length === 4 && worst < 150
        ? 'ok'
        : `reflow after debounce: ${times.map(Math.round).join(', ')} ms`
    },
  })

  // The memory budget runs as its own fresh session: spike 'm' (spikeMemory below).

  checks.push({
    id: 'S14-warm-book',
    description:
      'S14, V8: after the library the book stays warm: returning is instant, at the same place, with the cover grow; in the library it takes no keys, wheel or commands; opening another book lets it go',
    run: async () => {
      const problems: string[] = []
      hooks.noWarm = false
      try {
        await backToLibrary()
        await openFromLibrary(/Moby Dick(?!;)/)
        const engine = reader()!.engine
        if (!hooks.extensions?.bridge) problems.push('extensions do not see the open book')
        const turned = await turnBy(() => key('ArrowRight'), 'next')
        if (turned !== 'ok') problems.push(`before: ${turned}`)
        const at = loc()!.cfi
        await backToLibrary()
        if (reader()) problems.push('the reader hook is still set in the library')
        if (!engine.view.isConnected) problems.push('the book was unloaded')
        if (!document.querySelector('.reader-layer.warm[inert]'))
          problems.push('the warm book is not inert')
        if (hooks.extensions?.bridge) problems.push('extensions still see the book in the library')
        if (hooks.registry!.get('chapter.next'))
          problems.push('Next Chapter is on offer in the library')
        // Keys (at the window and inside the book's own document) and the wheel reach nothing.
        keyOnApp('ArrowRight', { code: 'ArrowRight' })
        keyOnApp(' ', { code: 'Space' })
        const doc = engine.view.renderer.getContents()[0]?.doc
        doc?.body.dispatchEvent(
          new KeyboardEvent('keydown', { key: 'ArrowRight', code: 'ArrowRight', bubbles: true }),
        )
        await invoke('spike_scroll_wheel', { x: 640, y: 400, delta: -3, count: 3, pixels: false })
        await settled(900)
        const grows = hooks.coverGrows ?? 0
        const t0 = performance.now()
        libraryTile(/Moby Dick(?!;)/)!.click()
        await waitFor('reader', () => reader())
        const ms = Math.round(performance.now() - t0)
        log(`S14: back to the warm book in ${ms} ms`)
        if (reader()!.engine !== engine) problems.push('the book was opened again, not kept warm')
        else if (ms > 100) problems.push(`returning took ${ms} ms`)
        await settled(400)
        if (loc()!.cfi !== at)
          problems.push(`the place moved while in the library: ${at} → ${loc()!.cfi}`)
        if ((hooks.coverGrows ?? 0) <= grows) problems.push('no cover grow')
        if (!hooks.registry!.get('chapter.next'))
          problems.push('the reader commands did not come back')
        if (!hooks.extensions?.bridge) problems.push('extensions did not get the book back')
        const again = await turnBy(() => key('ArrowRight'), 'next')
        if (again !== 'ok') problems.push(`after: ${again}`)
        // Another book takes its place; the warm one is closed, and its last save is its own.
        const warmId = hooks.reader!.bookId
        const warmAt = loc()!.cfi
        await backToLibrary()
        await openFromLibrary(/one file/i)
        if (engine.view.isConnected) problems.push('the previous book is still loaded')
        await settled(600)
        const otherId = hooks.reader!.bookId
        const [mine, other] = await Promise.all([
          invoke<[string, number] | null>('position_get', { bookId: warmId }),
          invoke<[string, number] | null>('position_get', { bookId: otherId }),
        ])
        if (mine?.[0] !== warmAt) problems.push(`the warm book's place was not kept: ${mine?.[0]}`)
        if (other?.[0] === warmAt) problems.push('the warm book saved its place over the next book')
        if (document.querySelectorAll('.reader-layer').length !== 1)
          problems.push(`${document.querySelectorAll('.reader-layer').length} reader layers`)
      } finally {
        hooks.noWarm = true
      }
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  checks.push({
    id: 'reader-cleanup',
    description:
      'Leaving the reader closes the engine and removes its views, even for a short book',
    run: async () => {
      await backToLibrary()
      const leftovers = document.querySelectorAll('foliate-view').length
      return !reader() && leftovers === 0
        ? 'ok'
        : `reader hook ${reader() ? 'still set' : 'cleared'}, ${leftovers} views left`
    },
  })

  checks.push({
    id: 'I6-turn-budget',
    description: 'Page turns render within a frame: < 16 ms p95, within and across chapters (§6.4)',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick(?!;)/)
      const engine = reader()!.engine
      await engine.goTo(20)
      await sleep(600)
      const within: number[] = []
      const across: number[] = []
      const wrong: string[] = []
      for (let i = 0; i < 120; i++) {
        const prevLoc = loc()!
        const before = prevLoc.sectionIndex
        let last = 0
        const off = engine.onRelocate(() => (last = performance.now()))
        const t0 = performance.now()
        await engine.turn('next')
        off()
        const now = loc()!
        const after = now.sectionIndex
        if (last) (after === before ? within : across).push(last - t0)
        // Every turn must move forward: the next page, or the first page of the next section.
        const ok =
          (after === before && (now.page ?? 0) === (prevLoc.page ?? 0) + 1) ||
          (after === before + 1 && now.page === 1)
        if (!ok) wrong.push(`${before}:${prevLoc.page} → ${after}:${now.page}`)
      }
      const p95 = (xs: number[]) => {
        const s = [...xs].sort((a, b) => a - b)
        return s.length
          ? Math.round(s[Math.min(s.length - 1, Math.round((s.length - 1) * 0.95))] * 10) / 10
          : NaN
      }
      // Backward: the previous page, or the last page of the previous section.
      for (let i = 0; i < 60; i++) {
        const prevLoc = loc()!
        let last = 0
        const off = engine.onRelocate(() => (last = performance.now()))
        const t0 = performance.now()
        await engine.turn('prev')
        off()
        const now = loc()!
        if (last) (now.sectionIndex === prevLoc.sectionIndex ? within : across).push(last - t0)
        const ok =
          (now.sectionIndex === prevLoc.sectionIndex &&
            (now.page ?? 0) === (prevLoc.page ?? 0) - 1) ||
          (now.sectionIndex === prevLoc.sectionIndex - 1 && now.page === now.pages)
        if (!ok)
          wrong.push(
            `back ${prevLoc.sectionIndex}:${prevLoc.page} → ${now.sectionIndex}:${now.page}/${now.pages}`,
          )
      }
      const w = p95(within)
      const a = p95(across)
      const evidence = `within a chapter p95 ${w} ms (n=${within.length}); across chapters p95 ${a} ms (n=${across.length}); 120 forward + 60 back`
      log(`I6 budget: ${evidence}`)
      if (wrong.length)
        return `${wrong.length} turns landed wrongly: ${wrong.slice(0, 5).join(', ')}; ${evidence}`
      return w < 16 && a < 16 ? 'ok' : evidence
    },
  })

  checks.push({
    id: 'E-hostile-in-product',
    description: 'Hostile book content stays inert in the product reader (Spike E, L14)',
    run: async () => {
      await invoke('spike_canary_clear')
      await backToLibrary()
      await openFromLibrary(/Hostile/)
      const engine = reader()!.engine
      const marks: string[] = []
      let overlay = 'not found'
      for (let i = 0; i < (engine.book?.sections.length ?? 0); i++) {
        if (engine.book!.sections[i].linear === 'no') continue
        await engine.goTo(i)
        await sleep(i === 1 ? 3500 : 1200) // the network section has a 3 s meta refresh
        const doc = engine.view.renderer.getContents()[0]?.doc
        if (!doc) continue
        for (const id of ['p-jslink', 'n-link', 'f-blank', 'p-form-button'])
          (doc.getElementById(id) as HTMLElement | null)?.click()
        await sleep(300)
        const mark = doc.documentElement.getAttribute('data-pwned')
        if (mark) marks.push(mark.trim())
        const fixed = doc.getElementById('o-fixed')
        if (fixed) overlay = doc.defaultView!.getComputedStyle(fixed).position
      }
      await sleep(500)
      const canary = await invoke<{ ipc: string[]; http: string[] }>('spike_canary_log')
      const problems = [
        ...marks.map((m) => `script ran: ${m}`),
        ...canary.ipc.map((c) => `IPC: ${c}`),
        ...canary.http.map((c) => `network: ${c}`),
        ...(overlay === 'static' ? [] : [`overlay position: ${overlay}`]),
      ]
      return problems.length ? problems.join('; ') : 'ok'
    },
  })

  // Last: the run itself raised no uncaught errors (the crash log holds only D1's probe).
  checks.push({
    id: 'D1-no-uncaught-errors',
    description:
      'No uncaught error or unhandled rejection in the whole run (read from the crash log)',
    run: async () => {
      const entries = (await invoke<string>('spike_crash_log'))
        .split(/^--- /m)
        .slice(1)
        .filter((e) => !e.includes('e2e crash-log probe'))
      return entries.length
        ? `${entries.length} in the log, first: ${entries[0].split('\n').slice(1, 4).join(' < ')}`
        : 'ok'
    },
  })

  const criteria: Criterion[] = []
  const only = (await invoke<{ only?: string | null }>('spike_info')).only
  for (const c of checks) {
    if (only && !new RegExp(only).test(c.id)) continue
    let evidence: string
    try {
      evidence = await c.run()
      const w = hooks.writes
      if (evidence !== 'ok' && w?.failed)
        evidence += ` [write queue failed: ${JSON.stringify(w.lastError)}; pending ${w.pendingKeys.join(', ')}]`
    } catch (e) {
      evidence = `error: ${e instanceof Error ? `${e.message} @ ${(e.stack ?? '').split('\n').slice(0, 6).join(' < ')}` : String(e)}`
    }
    log(`${evidence === 'ok' ? 'PASS' : 'FAIL'} ${c.id}: ${evidence}`)
    criteria.push({
      id: c.id,
      description: c.description,
      verdict: evidence === 'ok' ? 'pass' : 'fail',
      evidence,
    })
  }
  return { spike: 'e2e-reader', criteria, raw: {} }
}

/**
 * §6.4 memory budget, as specified: a fresh session opens the 100 MB book and
 * reads 50 pages; resident memory of the app and its WebKit processes < 400 MB.
 */
export async function spikeMemory(): Promise<SpikeResult> {
  const path = await invoke<string>('spike_corpus_path', { name: 'large-100mb.epub' })
  await invoke('library_import', { paths: [path] })
  const { installThemeCss } = await import('../app/theme')
  await import('../app/base.css')
  installThemeCss()
  document.getElementById('log')!.style.display = 'none'
  document.getElementById('chrome-top')!.style.display = 'none'
  const { default: App } = await import('../App.svelte')
  mount(App, { target: document.getElementById('reader')! })
  await openFromLibrary(/100 MB/)
  type Memory = {
    total_mb: number
    processes: string[]
    footprint_mb: number
    footprints: string[]
  }
  const before = await invoke<Memory>('spike_memory')
  for (let i = 0; i < 50; i++) await reader()!.engine.turn('next')
  await settled(2000)
  const after = await invoke<Memory>('spike_memory')
  // §6.4 names resident memory (RSS); under memory pressure macOS compresses pages
  // out of RSS, so the physical footprint (compressed pages included) is recorded too.
  const evidence = `after opening ${before.total_mb} MB; after 50 pages RSS ${after.total_mb} MB = ${after.processes.join(' + ')}; footprint ${after.footprint_mb} MB = ${after.footprints.join(' + ')}`
  log(`memory: ${evidence}`)
  return {
    spike: 'budget-memory',
    criteria: [
      {
        id: 'budget-memory',
        description:
          'A 100 MB book after reading 50 pages uses < 400 MB (RSS and footprint) across the app and its WebKit processes (§6.4, M1)',
        // Decision M1 (docs/decisions.md): both RSS and physical footprint must be under 400 MB.
        verdict: after.total_mb < 400 && after.footprint_mb < 400 ? 'pass' : 'fail',
        evidence,
      },
    ],
    raw: { before, after },
  }
}

/**
 * Memory diagnostics (not a budget check): where the 100 MB book's memory goes.
 * Tracks live blob URLs (count and bytes) and samples each process's resident
 * memory while reading 50 pages and then while idle.
 */
export async function spikeMemoryTrace(): Promise<SpikeResult> {
  const live = new Map<string, number>()
  const create = URL.createObjectURL.bind(URL)
  const revoke = URL.revokeObjectURL.bind(URL)
  URL.createObjectURL = (o: Blob | MediaSource) => {
    const url = create(o)
    live.set(url, o instanceof Blob ? o.size : 0)
    return url
  }
  URL.revokeObjectURL = (url: string) => {
    live.delete(url)
    revoke(url)
  }
  const blobs = () => {
    const bytes = [...live.values()].reduce((a, b) => a + b, 0)
    return `${live.size} blob URLs ${Math.round(bytes / 1e6)} MB`
  }
  const path = await invoke<string>('spike_corpus_path', { name: 'large-100mb.epub' })
  await invoke('library_import', { paths: [path] })
  const { installThemeCss } = await import('../app/theme')
  await import('../app/base.css')
  installThemeCss()
  document.getElementById('log')!.style.display = 'none'
  document.getElementById('chrome-top')!.style.display = 'none'
  const { default: App } = await import('../App.svelte')
  mount(App, { target: document.getElementById('reader')! })
  const samples: string[] = []
  const sample = async (label: string) => {
    const m = await invoke<{
      total_mb: number
      processes: string[]
      footprint_mb: number
      footprints: string[]
    }>('spike_memory')
    const line = `${label}: RSS ${m.total_mb} MB, footprint ${m.footprint_mb} MB = ${m.footprints.join(' + ')}; ${blobs()}; section ${loc()?.sectionIndex}`
    samples.push(line)
    log(line)
  }
  await sample('before open')
  await openFromLibrary(/100 MB/)
  await sample('after open')
  for (let i = 1; i <= 50; i++) {
    await reader()!.engine.turn('next')
    if (i % 10 === 0) await sample(`after ${i} pages`)
  }
  for (const wait of [2000, 5000, 10000]) {
    await settled(wait)
    await sample(`idle +${wait} ms`)
  }
  return {
    spike: 'memory-trace',
    criteria: [
      {
        id: 'memory-trace',
        description: 'Memory trace',
        verdict: 'manual',
        evidence: samples.at(-1) ?? '',
      },
    ],
    raw: { samples },
  }
}

/**
 * Visual baseline candidates (plan §6.1): the product in the states of Screens
 * 02, 03, 10 and 14 — Moby-Dick chapter 1 at 1280 × 800 in the mock's theme —
 * captured from this app's own window only, for the owner's side-by-side review.
 */
export async function spikeVisual(): Promise<SpikeResult> {
  const names = [
    'standardebooks-moby-dick.epub',
    'idpf-regime-anticancer-arabic.epub',
    'idpf-page-blanche.epub',
  ]
  const paths = await Promise.all(
    names.map((name) => invoke<string>('spike_corpus_path', { name })),
  )
  await invoke('library_import', { paths })
  const { installThemeCss } = await import('../app/theme')
  await import('../app/base.css')
  installThemeCss()
  document.getElementById('log')!.style.display = 'none'
  document.getElementById('chrome-top')!.style.display = 'none'
  await getCurrentWindow().setSize(new LogicalSize(1280, 800))
  const { default: App } = await import('../App.svelte')
  mount(App, { target: document.getElementById('reader')! })

  const shots: Record<string, string> = {}
  const capture = async (name: string) => {
    await settled(700)
    shots[name] = await invoke<string>('spike_capture', { name })
    log(`captured ${name}`)
  }
  const openIn = async (theme: string) => {
    await invoke('setting_set', { key: 'theme', value: theme })
    if (reader()) await backToLibrary()
    await openFromLibrary(/Moby Dick(?!;)/)
    // Chapter 1, first page (Screen 02's state).
    await reader()!.engine.goTo('chapter-1.xhtml')
    await settled(900)
    const m = hooks.messages?.current
    if (m) hooks.messages!.dismiss(m.id)
  }

  await openIn('paper')
  await hideControls()
  await capture('02-reader-immersive-paper')
  await showControls()
  await capture('03-reader-controls-paper')
  await hideControls()
  for (const theme of ['sepia', 'night'] as const) {
    await openIn(theme)
    await hideControls()
    await capture(`10-reader-${theme}`)
  }
  await showControls()
  await capture('14-night-controls')
  await hideControls()

  // G8 (approved designs, docs/design/g8): the same states in the product.
  await openIn('paper')
  const w = getCurrentWindow()
  await w.setSize(new LogicalSize(1680, 1050))
  await settled(1500)
  await hideControls()
  await capture('g8-spread-immersive')
  await showControls()
  await capture('g8-spread-controls')
  await hideControls()
  await w.setSize(new LogicalSize(1280, 800))
  await settled(1500)
  hooks.run?.('layout.scroll')
  await settled(1500)
  const host = reader()!.engine.view.parentElement!
  host.scrollBy(0, 700)
  await capture('g8-scroll-reading')
  for (let i = 0; i < 200 && !document.querySelector('.linen-join'); i++) {
    host.scrollBy(0, 400)
    await sleep(50)
  }
  const join = document.querySelector<HTMLElement>('.linen-join')
  if (join) host.scrollTop = join.offsetTop - 330
  await capture('g8-scroll-join')
  hooks.run?.('layout.pages')
  await settled(1200)
  await backToLibrary()
  await openFromLibrary(/[؀-ۿ]|anticancer|Régime|regime/i)
  await showControls()
  await capture('g8-rtl-controls')
  await hideControls()
  await backToLibrary()
  await openFromLibrary(/blanche/i)
  await capture('g8-fxl-fit')
  keyOnApp('=', { code: 'Equal', metaKey: true })
  keyOnApp('=', { code: 'Equal', metaKey: true })
  await capture('g8-fxl-zoom')

  // Phase 3 (Screens 04, 16, 17; G10 and the ⋯ menu, provisional).
  await openIn('paper')
  await hideControls()
  hooks.run?.('navigator.contents')
  await waitFor('contents', () => document.querySelector('.navigator .row.current'))
  await settled(900)
  await capture('04-navigator-contents')
  key('Escape', { code: 'Escape' })
  await settled(600)
  // A jump, so ⌘K has a Back entry under Recently closed (as Screen 16 shows).
  await reader()!.engine.goTo('chapter-3.xhtml')
  await settled(600)
  await reader()!.engine.goTo('chapter-1.xhtml')
  hooks.run?.('history.back')
  await settled(600)
  const m = hooks.messages?.current
  if (m) hooks.messages!.dismiss(m.id)
  keyOnApp('k', { code: 'KeyK', metaKey: true })
  await waitFor('palette', () => document.querySelector('dialog[open] .palette'))
  await capture('16-command-palette')
  ;(document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true }),
  )
  await settled(400)
  keyOnApp('?', { code: 'Slash', shiftKey: true })
  hooks.run?.('shortcuts.show')
  await waitFor('cheat sheet', () => document.querySelector('dialog[open] .sheet'))
  await capture('g10-cheat-sheet')
  ;(document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true }),
  )
  await settled(400)
  await reader()!.engine.goTo('chapter-1.xhtml')
  await settled(600)
  await showControls()
  document.querySelector<HTMLButtonElement>('.more-button')!.click()
  await waitFor('⋯ menu', () => document.querySelector('.more'))
  await capture('03-more-menu')
  key('Escape', { code: 'Escape' })
  await settled(300)
  keyOnApp('j', { code: 'KeyJ', metaKey: true })
  const pop = await waitFor('Go to', () => document.querySelector<HTMLElement>('.goto'))
  const input = pop.querySelector<HTMLInputElement>('input')!
  input.value = '31'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await capture('17-goto')
  key('Escape', { code: 'Escape' })
  await settled(300)
  await hideControls()
  const sections = reader()!.engine.book!.sections
  await reader()!.engine.goTo(sections.findIndex((x) => x.id.endsWith('/chapter-109.xhtml')))
  await settled(900)
  const doc = reader()!.engine.view.renderer.getContents()[0].doc
  doc
    .querySelector('#noteref-21')!
    .dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
  await waitFor('peek', () => document.querySelector('.peek'))
  await capture('17-footnote-peek')
  key('Escape', { code: 'Escape' })
  await settled(300)

  // Phase 4 (Screen 05): “water” in Chapter 1, the second result on the page.
  const chapter1 = reader()!.engine.book!.sections.findIndex((x) =>
    x.id.endsWith('/chapter-1.xhtml'),
  )
  await reader()!.engine.goTo(chapter1)
  await settled(600)
  keyOnApp('f', { code: 'KeyF', metaKey: true })
  const field = await waitFor('search field', () =>
    document.querySelector<HTMLInputElement>('.navigator .search input'),
  )
  field.value = 'water'
  field.dispatchEvent(new Event('input', { bubbles: true }))
  await waitFor('search finished', () => reader()!.search.settled || null, 30_000)
  for (let i = 0; i < 2; i++)
    field.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
    )
  await settled(900)
  await capture('05-navigator-search')
  key('Escape', { code: 'Escape' })
  await settled(600)

  // Phase 5 (Screens 06, 07, 08, 14, 15): the mocks' passages in Chapter 1.
  const NOTE =
    'Ishmael frames the voyage as a cure for his own gloom — the sea as medicine. Compare with Ahab’s reasons later.'
  const clearMessage = () => {
    const current = hooks.messages?.current
    if (current) hooks.messages!.dismiss(current.id)
  }
  await selectPhrase(NOVEMBER)
  await pressBar(/highlight yellow/i)
  await selectPhrase(MANHATTOES)
  await waitFor('selection bar', selBar)
  clearMessage()
  await capture('06-selection-bar')
  await pressBar(/highlight green/i)
  const manhattoesId = reader()!.annotations.items.at(-1)!.id
  const openNoteOn = async () => {
    const range = await selectPhrase(MANHATTOES)
    pageDoc().doc.getSelection()?.removeAllRanges()
    clickOn(range)
    await waitFor('bar', selBar)
    await pressBar(/^note$/i)
    return waitFor('note', noteCard)
  }
  const card = await openNoteOn()
  typeNote(NOTE)
  await waitFor('saved', () =>
    card.querySelector('[role="status"]')?.textContent?.includes('Saved'),
  )
  // The mock shows the card at rest, focus not in the field.
  card.querySelector('textarea')!.blur()
  clearMessage()
  await capture('07-margin-note')
  key('Escape', { code: 'Escape' })
  await settled(400)
  await reader()!.engine.goTo(chapter1)
  await settled(600)
  keyOnApp('a', { code: 'KeyA', metaKey: true, shiftKey: true })
  const notes = await waitFor('notes', () =>
    document.querySelector<HTMLElement>('.navigator .notes'),
  )
  await settled(400)
  notes.querySelector<HTMLElement>(`[data-annotation="${manhattoesId}"]`)!.click()
  await sleep(350) // mid-pulse, with the Back chip (Screen 08)
  shots['08-navigator-notes'] = await invoke<string>('spike_capture', {
    name: '08-navigator-notes',
  })
  log('captured 08-navigator-notes')
  await settled(1200)
  key('Escape', { code: 'Escape' })
  await settled(400)
  clearMessage()
  await openIn('night')
  await reader()!.engine.goTo(chapter1)
  await settled(600)
  // Screen 14 shows a plain selection beside a highlight: the green one is taken off for it.
  await waitFor('highlights', () => reader()!.annotations.get(manhattoesId))
  const kept = reader()!.annotations.remove(manhattoesId)!
  await selectPhrase(MANHATTOES)
  await showControls()
  await waitFor('selection bar', selBar)
  clearMessage()
  await capture('14-night-selection')
  await hideControls()
  pageDoc().doc.getSelection()?.removeAllRanges()
  reader()!.annotations.restore(kept)
  await hooks.writes!.idle()
  await openIn('paper')
  await getCurrentWindow().setSize(new LogicalSize(760, 1000))
  await settled(1500)
  await reader()!.engine.goTo(chapter1)
  await settled(600)
  await openNoteOn()
  await showControls()
  noteCard()!.querySelector('textarea')!.blur()
  clearMessage()
  await capture('15-note-sheet')
  key('Escape', { code: 'Escape' })
  await getCurrentWindow().setSize(new LogicalSize(1280, 800))
  await settled(1200)

  // Phase 6 (Screens 09 and 01): Aa over Chapter 1, then the library.
  await reader()!.engine.goTo(chapter1)
  await settled(600)
  await showControls()
  hooks.run?.('reader.settings')
  await waitFor('Aa', () => document.querySelector('.aa[role="dialog"]'))
  ;(document.activeElement as HTMLElement | null)?.blur()
  clearMessage()
  await capture('09-reading-settings')
  key('Escape', { code: 'Escape' })
  await settled(300)
  await backToLibrary()
  clearMessage()
  await capture('01-library')

  // Screen 12: the damaged-book card, then the empty library.
  const [damaged] = await invoke<{ outcome: { book_id?: string } }[]>('library_import', {
    paths: [await invoke<string>('spike_corpus_path', { name: 'broken-missing-items.epub' })],
  })
  hooks.run?.('library.show')
  await settled(600)
  Array.from(document.querySelectorAll<HTMLElement>('.library .tile'))
    .find((t) => t.dataset.book === damaged.outcome.book_id)
    ?.querySelector<HTMLButtonElement>('.open')
    ?.click()
  await waitFor('damaged card', () => document.querySelector('dialog[open] .card'))
  await settled(400)
  await capture('12-damaged-book')
  ;(document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true }),
  )
  await settled(300)
  const all = await invoke<{ id: string }[]>('library_list')
  for (const b of all) await invoke('library_remove', { bookId: b.id })
  hooks.run?.('library.show')
  await waitFor('empty library', () => document.querySelector('.library .empty'))
  clearMessage()
  await capture('12-empty-library')
  for (const b of all) await invoke('library_restore', { bookId: b.id })

  // Screen 11: Settings › Extensions with the samples installed, as the mock lists
  // them: Markdown Export (built in), Dictionary (with a failure box), Night Owl, and
  // one turned off.
  for (const name of ['dictionary', 'night-owl', 'hang'])
    await invoke('spike_install_unchecked', {
      path: await invoke<string>('spike_corpus_path', { name: `${name}.linenext` }),
    })
  await invoke('extension_crashed', { id: 'org.example.dictionary' })
  await invoke('extension_crashed', { id: 'org.example.dictionary' })
  await invoke('extension_set_enabled', { id: 'test.hang', enabled: false })
  await hooks.extensions!.load()
  const { default: Preferences } = await import('../prefs/Preferences.svelte')
  const prefsHost = document.createElement('div')
  prefsHost.style.cssText = 'position:fixed;inset:0;z-index:100;background:var(--ground)'
  document.body.append(prefsHost)
  mount(Preferences, { target: prefsHost })
  await settled(600)
  prefsHost.querySelector<HTMLButtonElement>('#prefs-extensions')!.click()
  ;(document.activeElement as HTMLElement | null)?.blur()
  await settled(600)
  await capture('11-settings-extensions')
  prefsHost.remove()

  // Screen 12: an extension failure, contained: the selection's “⋯” marks Define
  // “Not responding” with Restart, and the core actions are unaffected.
  hooks.run?.('library.show')
  await settled(600)
  await openFromLibrary(/Moby Dick(?!;)/)
  await reader()!.engine.goTo(chapter1)
  await settled(600)
  hooks.extensions!.status['org.example.dictionary'] = 'not-responding'
  clearMessage()
  await selectPhrase('spleen')
  await waitFor('bar', selBar)
  barButton(/more actions from extensions/i)?.click()
  ;(document.activeElement as HTMLElement | null)?.blur()
  await settled(300)
  await capture('12-extension-failure')
  pageDoc().doc.getSelection()?.removeAllRanges()
  key('Escape', { code: 'Escape' })
  await invoke('setting_set', { key: 'theme', value: 'auto' })
  return {
    spike: 'visual-candidates',
    criteria: [
      {
        id: 'visual-captured',
        description: 'Baseline candidates captured',
        verdict: 'manual',
        evidence: Object.keys(shots).join(', '),
      },
    ],
    raw: { shots },
  }
}

/**
 * X3 re-check with VoiceOver in the product (Spike C found the mechanism; this
 * checks the Reader's handling of it). A person turns VoiceOver on and lets it
 * read across two page ends and one chapter end; the harness records whether the
 * location follows, the page snaps to whole pages, progress is saved, and the
 * app stays quiet about page turns VoiceOver itself makes.
 */
export async function spikeX3(): Promise<SpikeResult> {
  const { compare } = await import('foliate-js/epubcfi.js')
  const { ipc } = await import('../app/ipc')
  const path = await invoke<string>('spike_corpus_path', { name: 'standardebooks-moby-dick.epub' })
  await invoke('library_import', { paths: [path] })
  const { installThemeCss } = await import('../app/theme')
  await import('../app/base.css')
  installThemeCss()
  document.getElementById('log')!.style.display = 'none'
  document.getElementById('chrome-top')!.style.display = 'none'
  const { default: App } = await import('../App.svelte')
  mount(App, { target: document.getElementById('reader')! })
  await openFromLibrary(/Moby Dick(?!;)/)
  const engine = reader()!.engine
  const bookId = reader()!.bookId

  // Three pages before the end of a chapter: two page ends, then a chapter end.
  await engine.goTo(20)
  await settled(800)
  for (let i = 0; i < 200 && loc()!.page! < loc()!.pages! - 2; i++) {
    await engine.turn('next')
    await settled(40)
  }
  await settled(600)
  const startLoc = loc()!
  log(`x3 start: section ${startLoc.sectionIndex}, page ${startLoc.page}/${startLoc.pages}`)

  const region = () =>
    document.querySelector('[role="status"][aria-live="polite"]')?.textContent?.trim() ?? ''
  const relocations: { t: number; reason: string; cfi: string; section: number; page?: number }[] =
    []
  engine.onRelocate((l) =>
    relocations.push({
      t: Math.round(performance.now()),
      reason: l.reason,
      cfi: l.cfi,
      section: l.sectionIndex,
      page: l.page,
    }),
  )
  const samples: {
    t: number
    page: number
    offset: number
    size: number
    section: number
    locPage?: number
    region: string
  }[] = []
  const sample = () => {
    const r = engine.view.renderer
    samples.push({
      t: Math.round(performance.now()),
      page: r.page,
      offset: Math.round(r.start % r.size),
      size: Math.round(r.size),
      section: loc()?.sectionIndex ?? -1,
      locPage: loc()?.page,
      region: region(),
    })
  }

  const start = await step(
    'VoiceOver in the reader (X3), about 5 minutes',
    'Turn on VoiceOver (System Settings › Accessibility › VoiceOver, or ⌘F5). Wait 3 seconds, then click once in the book text and press <b>Control + Option + A</b> to read all. When it has started reading, press <b>Reading started</b> with the mouse.<br><br>The page is three pages before the end of a chapter.',
    ['Reading started', 'Skip'],
  )
  if (start === 'Skip')
    return {
      spike: 'x3-voiceover',
      criteria: [
        {
          id: 'X3-product',
          description: 'VoiceOver re-check',
          verdict: 'manual',
          evidence: 'skipped',
        },
      ],
      raw: {},
    }
  const readingFrom = performance.now()
  const regionBefore = region()
  const detected = await ipc.screenReaderRunning()
  sample()
  const timer = setInterval(sample, 250)
  await step(
    'Keep listening',
    'Let VoiceOver read until it has passed the end of this chapter (about 3–4 minutes), or until it stops. Then stop it (press Control) and press <b>Done</b>.',
    ['Done'],
  )
  clearInterval(timer)
  sample()
  const endLoc = loc()!
  await sleep(1600) // the 1 s save debounce
  const saved = await ipc.positionGet(bookId)
  const continuous = await step(
    'Within the chapter',
    'Did VoiceOver keep reading past the end of each page, and did the visible page follow it?',
    ['Yes', 'No', 'Not sure'],
  )
  const sliver = await step(
    'Page edges',
    'After the page moved, did you ever see part of the neighbouring page at the left or right edge?',
    ['No', 'Yes', 'Not sure'],
  )
  const chapterEnd = await step('Chapter end', 'At the end of the chapter, what happened?', [
    'Read into the next chapter',
    'Stopped at the end',
    'Read other things (menus, labels)',
    'Did not get there',
  ])

  const during = relocations.filter((r) => r.t >= readingFrom)
  const external = during.filter((r) => r.reason === 'external')
  const ordered = external.every((r, i) => i === 0 || compare(r.cfi, external[i - 1].cfi) > 0)
  // A page is snapped when its offset is a whole page; allow the 250 ms after a move to settle.
  const settledSamples = samples.filter((x, i) => i > 1 && x.page === samples[i - 1].page)
  const unsnapped = settledSamples.filter((x) => x.offset > 1 && x.size - x.offset > 1)
  const selfAnnounced = [...new Set(samples.map((x) => x.region))].filter(
    (text) => text !== regionBefore && /page \d+/i.test(text),
  )
  const pagesSeen = new Set(samples.map((x) => `${x.section}:${x.page}`)).size

  const criteria: Criterion[] = [
    {
      id: 'X3-detected',
      description: 'The app detects VoiceOver (T6), so the reader follows external scrolls',
      verdict: detected ? 'pass' : 'fail',
      evidence: `screen_reader_running = ${detected}`,
    },
    {
      id: 'X3-location-follows',
      description:
        'The location follows VoiceOver: the reader reports each page it moves to, in reading order',
      verdict: external.length >= 2 && ordered ? 'pass' : 'fail',
      evidence: `${external.length} external relocations (${during.length} in all) while reading, ${ordered ? 'in order' : 'OUT OF ORDER'}; pages seen ${pagesSeen}; from section ${startLoc.sectionIndex} p${startLoc.page} to section ${endLoc.sectionIndex} p${endLoc.page}`,
    },
    {
      id: 'X3-snap',
      description:
        'After VoiceOver moves the page, it rests on a whole page (no sliver of the next)',
      verdict: unsnapped.length === 0 && sliver === 'No' ? 'pass' : 'fail',
      evidence: `${unsnapped.length} of ${settledSamples.length} settled samples off a page boundary; observer saw a sliver: ${sliver}`,
    },
    {
      id: 'X3-progress-saved',
      description: 'Progress is saved where VoiceOver stopped (N4)',
      verdict: saved?.[0] === endLoc.cfi && compare(endLoc.cfi, startLoc.cfi) > 0 ? 'pass' : 'fail',
      evidence: `saved ${saved?.[0]}; location ${endLoc.cfi}; start ${startLoc.cfi}`,
    },
    {
      id: 'X3-quiet',
      description: 'The app does not announce page turns that VoiceOver itself makes',
      verdict: selfAnnounced.length === 0 ? 'pass' : 'fail',
      evidence: selfAnnounced.length
        ? `announced: ${selfAnnounced.join(' | ')}`
        : 'no page announcements while reading',
    },
    {
      id: 'X3-continuous',
      description: 'VoiceOver reads continuously across page ends and the page follows (observer)',
      verdict: continuous === 'Yes' ? 'pass' : 'fail',
      evidence: `observer: ${continuous}`,
    },
    {
      id: 'X3-chapter-end',
      description: 'What VoiceOver does at the end of a chapter (observer; informs the design)',
      verdict: chapterEnd === 'Read into the next chapter' ? 'pass' : 'manual',
      evidence: `observer: ${chapterEnd}; reader ended in section ${endLoc.sectionIndex} (started in ${startLoc.sectionIndex})`,
    },
  ]
  return {
    spike: 'x3-voiceover',
    criteria,
    raw: { readingFrom: Math.round(readingFrom), startLoc, endLoc, relocations, samples },
  }
}

/** §6.4 cold start: import the 500-book fixture into LINEN_DATA_DIR, then exit. */
export async function spikeSeed500(): Promise<SpikeResult> {
  const paths = await invoke<string[]>('spike_corpus_dir', { name: 'library-500' })
  const t0 = performance.now()
  const results = await invoke<{ outcome: { kind: string } }[]>('library_import', { paths })
  const imported = results.filter((r) => r.outcome.kind === 'imported').length
  return {
    spike: 'seed-500',
    criteria: [
      {
        id: 'seeded',
        description: 'The 500-book fixture is in the library',
        verdict: imported === 500 ? 'pass' : 'fail',
        evidence: `${imported} of ${paths.length} imported in ${Math.round(performance.now() - t0)} ms`,
      },
    ],
    raw: {},
  }
}
