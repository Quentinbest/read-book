// Spike D: engine fidelity of foliate-js in WKWebView (open time, CFI round-trips,
// highlight stability through reflow, page-turn latency).

import { Overlayer } from 'foliate-js/overlayer.js'
import type { View } from 'foliate-js/view.js'
import {
  log,
  openView,
  painted,
  readCorpus,
  readerCss,
  rng,
  sleep,
  stats,
  type Criterion,
  type SpikeResult,
} from './common'

const MOBY = 'standardebooks-moby-dick.epub'

/** Wait for the next relocate event (or a timeout), then for a painted frame. */
function relocated(view: View, timeout = 1500) {
  return new Promise<void>((resolve) => {
    const done = () => {
      clearTimeout(timer)
      view.removeEventListener('relocate', done)
      painted().then(() => resolve())
    }
    const timer = setTimeout(done, timeout)
    view.addEventListener('relocate', done)
  })
}

async function openTimes() {
  const moby: number[] = []
  const mobyRead: number[] = []
  for (let i = 0; i < 10; i++) {
    const t0 = performance.now()
    await readCorpus(MOBY)
    mobyRead.push(performance.now() - t0)
    const { view, firstPageMs } = await openView(MOBY)
    moby.push(firstPageMs)
    view.close()
  }
  const large: number[] = []
  const largeRead: number[] = []
  for (let i = 0; i < 5; i++) {
    const t0 = performance.now()
    await readCorpus('large-100mb.epub')
    largeRead.push(performance.now() - t0)
    const { view, firstPageMs } = await openView('large-100mb.epub')
    large.push(firstPageMs)
    view.close()
  }
  return {
    moby: stats(moby),
    mobyReadOnly: stats(mobyRead),
    large: stats(large),
    largeReadOnly: stats(largeRead),
  }
}

function randomTextRange(doc: Document, random: () => number): Range | null {
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) =>
      (n.textContent ?? '').trim().length > 40 ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP,
  })
  const nodes: Text[] = []
  for (let n = walker.nextNode(); n; n = walker.nextNode()) nodes.push(n as Text)
  if (!nodes.length) return null
  const node = nodes[Math.floor(random() * nodes.length)]
  const len = node.length
  const start = Math.floor(random() * (len - 20))
  const range = doc.createRange()
  range.setStart(node, start)
  range.setEnd(node, start + 20)
  return range
}

const visible = (view: View, target: Range) => {
  const page = view.lastLocation?.range
  if (!page || page.startContainer.ownerDocument !== target.startContainer.ownerDocument)
    return false
  return (
    page.compareBoundaryPoints(Range.START_TO_START, target) <= 0 &&
    // START_TO_END compares this range's end with the source range's start.
    page.compareBoundaryPoints(Range.START_TO_END, target) >= 0
  )
}

async function setFont(view: View, px: number) {
  view.renderer.setStyles(readerCss(px))
  await relocated(view)
}

async function setWidth(view: View, px: number) {
  document.getElementById('reader')!.style.width = `${px}px`
  await relocated(view)
}

/** Font 16 → 24 → 16 px, then width 1280 → 800 → 1280 px; after each step run check(). */
async function reflowSteps(view: View, check: (step: string) => void) {
  await setFont(view, 24)
  check('font 24')
  await setFont(view, 16)
  check('font 16')
  await setWidth(view, 800)
  check('width 800')
  await setWidth(view, 1280)
  check('width 1280')
}

async function cfiRoundTrips(samples: number) {
  const random = rng(20260924)
  const { view } = await openView(MOBY, { fontPx: 16 })
  const chapters = view.book.sections
    .map((s, i) => ({ s, i }))
    .filter(({ s }) => s.linear !== 'no' && s.size > 4000)
  let textOk = 0
  let visibleOk = 0
  const failures: string[] = []
  for (let n = 0; n < samples; n++) {
    const { i } = chapters[Math.floor(random() * chapters.length)]
    await view.goTo(i)
    await painted()
    const doc = view.renderer.getContents()[0].doc
    const range = randomTextRange(doc, random)
    if (!range) continue
    const text = range.toString()
    const cfi = view.getCFI(i, range)
    await view.goTo(cfi)
    await relocated(view)
    let seen = visible(view, range) ? 1 : 0
    const steps: string[] = []
    await reflowSteps(view, (step) => {
      if (visible(view, range)) seen++
      else steps.push(step)
    })
    const resolved = view.resolveCFI(cfi)
    const again = resolved.anchor(view.renderer.getContents()[0].doc) as Range
    const sameText = resolved.index === i && again.toString() === text
    if (sameText) textOk++
    if (seen === 5) visibleOk++
    else failures.push(`${cfi} not visible after: ${steps.join(', ') || 'initial goTo'}`)
    if (n % 20 === 0) log(`D: CFI ${n}/${samples} text ${textOk} visible ${visibleOk}`)
  }
  view.close()
  return { samples, textOk, visibleOk, failures: failures.slice(0, 20) }
}

async function highlightStability(count: number) {
  const random = rng(7)
  const { view } = await openView(MOBY, { fontPx: 16 })
  const chapter = view.book.sections.findIndex(
    (s, i) => i > 10 && s.linear !== 'no' && s.size > 30000,
  )
  await view.goTo(chapter)
  await painted()
  const doc = view.renderer.getContents()[0].doc
  const drawn = new Map<string, DOMRect[]>()
  view.addEventListener('draw-annotation', (e) => {
    const { draw, annotation } = (e as CustomEvent).detail
    draw((rects: DOMRectList) => {
      drawn.set(annotation.value, Array.from(rects))
      return Overlayer.highlight(rects, { color: 'yellow' })
    })
  })
  const items: { cfi: string; text: string }[] = []
  while (items.length < count) {
    const range = randomTextRange(doc, random)
    if (!range) break
    items.push({ cfi: view.getCFI(chapter, range), text: range.toString() })
  }
  for (const { cfi } of items) await view.addAnnotation({ value: cfi })
  const mismatches: string[] = []
  const compare = (step: string) => {
    const liveDoc = view.renderer.getContents()[0].doc
    for (const { cfi, text } of items) {
      const range = view.resolveCFI(cfi).anchor(liveDoc) as Range
      const live = Array.from(range.getClientRects())
      const last = drawn.get(cfi) ?? []
      const same =
        range.toString() === text &&
        live.length === last.length &&
        live.every(
          (r, k) => Math.abs(r.left - last[k].left) < 0.5 && Math.abs(r.top - last[k].top) < 0.5,
        )
      if (!same) mismatches.push(`${step}: ${cfi}`)
    }
  }
  compare('initial')
  await reflowSteps(view, compare)
  view.close()
  return {
    highlights: items.length,
    chapter,
    mismatches: mismatches.slice(0, 20),
    mismatchCount: mismatches.length,
  }
}

async function pageTurns(turns: number) {
  const { view } = await openView(MOBY)
  const within: number[] = []
  const across: number[] = []
  for (let n = 0; n < turns; n++) {
    await painted() // start each turn at a quiet frame boundary
    const before = view.renderer.getContents()[0]?.index
    const t0 = performance.now()
    // A turn can emit several relocate events (the old section, then the new
    // one); the turn is done at the last relocate before foliate-js settles.
    let last = 0
    const onRelocate = () => (last = performance.now())
    view.addEventListener('relocate', onRelocate)
    const turn = view.next()
    await turn // resolves after the new page is in place plus foliate-js's 100 ms lock
    view.removeEventListener('relocate', onRelocate)
    const after = view.renderer.getContents()[0]?.index
    if (last) {
      ;(before === after ? within : across).push(last - t0)
    }
  }
  view.close()
  return {
    workWithinChapter: stats(within),
    workAcrossChapter: across.length ? stats(across) : null,
    work: stats([...within, ...across]),
  }
}

/** Page turns only (to re-measure without the CFI and highlight runs). */
export async function spikeDTurns(): Promise<SpikeResult> {
  const turns = await pageTurns(200)
  return {
    spike: 'd-page-turns',
    criteria: [
      {
        id: 'D-page-turn',
        description: 'Page turn < 16 ms at p95',
        verdict: turns.work.p95 < 16 ? 'pass' : 'fail',
        evidence: `input → last relocate: all p95 ${turns.work.p95} ms; within a chapter p95 ${turns.workWithinChapter.p95} ms (n=${turns.workWithinChapter.n}); crossing a chapter ${turns.workAcrossChapter ? `p95 ${turns.workAcrossChapter.p95} ms, max ${turns.workAcrossChapter.max} ms (n=${turns.workAcrossChapter.n})` : 'none'}`,
      },
    ],
    raw: { turns },
  }
}

export async function spikeD(): Promise<SpikeResult> {
  log('D: open times')
  const open = await openTimes()
  log('D: page turns')
  const turns = await pageTurns(200)
  log('D: CFI round-trips')
  const cfi = await cfiRoundTrips(200)
  log('D: highlights')
  const hl = await highlightStability(100)
  await sleep(100)

  const criteria: Criterion[] = [
    {
      id: 'D-open-moby',
      description: 'Moby-Dick opens to the first page in < 500 ms',
      verdict: open.moby.p95 < 500 ? 'pass' : 'fail',
      evidence: `file read via IPC + parse + first page painted: p50 ${open.moby.p50} ms, p95 ${open.moby.p95} ms (read alone p50 ${open.mobyReadOnly.p50} ms), 10 runs`,
    },
    {
      id: 'D-open-100mb',
      description: 'A 100 MB corpus book opens in < 1.5 s',
      verdict: open.large.p95 < 1500 ? 'pass' : 'fail',
      evidence: `p50 ${open.large.p50} ms, p95 ${open.large.p95} ms including reading the whole file over IPC (read alone p50 ${open.largeReadOnly.p50} ms), 5 runs`,
    },
    {
      id: 'D-cfi',
      description:
        '200 random CFIs round-trip to the same text after font 16 → 24 → 16 px and resize 1280 → 800 → 1280 px',
      verdict: cfi.textOk === cfi.samples && cfi.visibleOk === cfi.samples ? 'pass' : 'fail',
      evidence: `same text ${cfi.textOk}/${cfi.samples}; target on the visible page after every step ${cfi.visibleOk}/${cfi.samples}`,
    },
    {
      id: 'D-highlights',
      description: '100 highlights stay on their exact text after the same reflows',
      verdict: hl.mismatchCount === 0 && hl.highlights === 100 ? 'pass' : 'fail',
      evidence: `${hl.highlights} highlights, ${hl.mismatchCount} rect/text mismatches over 5 checks`,
    },
    {
      id: 'D-page-turn',
      description: 'Page turn < 16 ms at p95',
      verdict: turns.work.p95 < 16 ? 'pass' : 'fail',
      evidence: `turn work (input → new page laid out): all p95 ${turns.work.p95} ms; within a chapter p95 ${turns.workWithinChapter.p95} ms (n=${turns.workWithinChapter.n}); crossing a chapter ${turns.workAcrossChapter ? `p95 ${turns.workAcrossChapter.p95} ms (n=${turns.workAcrossChapter.n})` : 'none'}`,
    },
  ]
  return { spike: 'd-fidelity', criteria, raw: { open, turns, cfi, highlights: hl } }
}
