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
        await openFromLibrary(/Moby Dick/)
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
        await openFromLibrary(/Moby Dick/)
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
    id: 'L16-long-chapter',
    description:
      'A 1 MB+ chapter opens within the open-book budget (< 500 ms, click → first location)',
    run: async () => {
      await backToLibrary()
      const row = await waitFor('row', () =>
        Array.from(document.querySelectorAll<HTMLButtonElement>('.row')).find((b) =>
          /one file/i.test(b.textContent ?? ''),
        ),
      )
      const t0 = performance.now()
      row.click()
      for (;;) {
        if (loc()?.cfi) break
        if (performance.now() - t0 > 10_000) return 'did not open within 10 s'
        await new Promise((r) => requestAnimationFrame(r))
      }
      const ms = Math.round(performance.now() - t0)
      await settled(800)
      const l = loc()!
      return ms < 500 ? 'ok' : `opened in ${ms} ms (${l.pages} pages in the first section)`
    },
  })

  checks.push({
    id: 'X3-announcements',
    description: 'A page turn is announced briefly to screen readers (“Page N”)',
    run: async () => {
      await backToLibrary()
      await openFromLibrary(/Moby Dick/)
      key('ArrowRight')
      await settled(600)
      const text = document.querySelector('[role="status"][aria-live="polite"]')?.textContent ?? ''
      return /^Page \d+$/.test(text) ? 'ok' : `live region said “${text}”`
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
      await openFromLibrary(/Moby Dick/)
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
  for (const c of checks) {
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
