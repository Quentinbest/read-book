// Phase 2 end-to-end tests, run inside the real app (macOS has no WebDriver for
// Tauri; plan §6.1 “App end-to-end tests”). Mounts the product App against a
// throwaway data folder (LINEN_DATA_DIR) and drives it like a reader would.

import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { mount } from 'svelte'
import type { TestHooks } from '../app/testHooks'
import { log, sleep, type Criterion, type SpikeResult } from './common'

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
  const doc = hooks.reader?.engine.view.renderer.getContents()[0]?.doc
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

type Check = { id: string; description: string; run: () => Promise<string> }

export async function spikeE2E(): Promise<SpikeResult> {
  // Import the test books before the app lists the library.
  const books = ['standardebooks-moby-dick.epub', 'idpf-regime-anticancer-arabic.epub']
  const paths = await Promise.all(
    books.map((name) => invoke<string>('spike_corpus_path', { name })),
  )
  await invoke('library_import', { paths })

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
        await openFromLibrary(/Moby/)
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
        return /^Back to page \d+/.test(text) ? 'ok' : `message was “${text}”`
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
      id: 'N5-restore',
      description: 'Leaving for the library and reopening restores the same place',
      run: async () => {
        const before = where()
        const cfi = loc()!.cfi
        await backToLibrary()
        await openFromLibrary(/Moby/)
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

  const criteria: Criterion[] = []
  for (const c of checks) {
    let evidence: string
    try {
      evidence = await c.run()
    } catch (e) {
      evidence = `error: ${e instanceof Error ? e.message : String(e)}`
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
