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

const hooks: TestHooks = {}
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

async function openFromLibrary(title: RegExp) {
  const row = await waitFor('library row', () =>
    Array.from(document.querySelectorAll<HTMLButtonElement>('.row')).find((b) =>
      title.test(b.textContent ?? ''),
    ),
  )
  row.click()
  await waitFor('reader location', () => loc()?.cfi)
  await settled(800)
}

async function backToLibrary() {
  key('l', { code: 'KeyL', metaKey: true })
  await waitFor('library', () => document.querySelector('.library .row, .library .empty'))
  await settled(300)
}

/** Reveal the controls the way a reader does: dwell at the top edge (S9). */
async function showControls() {
  const area = document.querySelector('.reader')!
  area.dispatchEvent(new PointerEvent('pointermove', { clientX: 640, clientY: 20, bubbles: true }))
  await sleep(600) // 150 ms dwell + 160 ms fade in
}

async function hideControls() {
  key('Escape', { code: 'Escape' })
  await sleep(400)
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
    // N9: endnotes in another chapter.
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
    id: 'G8-opening-line',
    description: 'An open over 500 ms shows one “Opening …” line, which the page then replaces',
    run: async () => {
      await backToLibrary()
      hooks.openDelayMs = 900
      try {
        const row = await waitFor('row', () =>
          Array.from(document.querySelectorAll<HTMLButtonElement>('.row')).find((b) =>
            /Moby Dick(?!;)/.test(b.textContent ?? ''),
          ),
        )
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
      const line = document.querySelector('.location-line')?.textContent ?? ''
      if (!/^Pages? \d+(–\d+)? of \d+$/.test(line)) return `location line “${line}”`
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
      const row = await waitFor('row', () =>
        Array.from(document.querySelectorAll<HTMLButtonElement>('.row')).find((b) =>
          /one file/i.test(b.textContent ?? ''),
        ),
      )
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

  checks.push({
    id: 'B8-scroll-mode',
    description:
      'Scroll mode (B8, G8): keeps the place, scrolls continuously across chapters (with the join), Space = a screen, ↓ = 3 lines, remembered per book',
    run: async () => {
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
      if (
        at.sectionIndex !== before.sectionIndex ||
        Math.abs(at.fraction - before.fraction) > 0.005
      )
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
      // And back to Pages at the same place.
      const s0 = loc()!
      hooks.run?.('layout.pages')
      await settled(1500)
      const mode = () => reopened.mode // read again: the command changed it
      if (mode() !== 'pages') return 'did not return to Pages'
      if (loc()!.sectionIndex !== s0.sectionIndex || Math.abs(loc()!.fraction - s0.fraction) > 0.01)
        return `Pages moved the place: ${s0.sectionIndex}/${s0.fraction} → ${loc()!.sectionIndex}/${loc()!.fraction}`
      return 'ok'
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
      const before = where()
      await wheel(-1, 1, false)
      await settled(700)
      const pageTurned = where() !== before
      if (!pageTurned) return `a wheel notch in Pages did not turn (${before})`
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
        if (!inMore.has(c.title) || inMore.get(c.title) !== label) problems.push(`⋯ ${c.id}`)
        if (c.menu && !inMenuBar.has(c.id)) problems.push(`menu bar ${c.id}`)
        if (!c.menu) problems.push(`no menu bar place for ${c.id}`)
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

  checks.push({
    id: 'budget-open',
    description:
      'Open a book to its first page in < 500 ms (§6.4; click → first location, median and p95 of 5)',
    run: async () => {
      const times: number[] = []
      for (let i = 0; i < 5; i++) {
        await backToLibrary()
        const row = await waitFor('row', () =>
          Array.from(document.querySelectorAll<HTMLButtonElement>('.row')).find((b) =>
            /Moby Dick(?!;)/.test(b.textContent ?? ''),
          ),
        )
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

  const criteria: Criterion[] = []
  const only = (await invoke<{ only?: string | null }>('spike_info')).only
  for (const c of checks) {
    if (only && !new RegExp(only).test(c.id)) continue
    let evidence: string
    try {
      evidence = await c.run()
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
