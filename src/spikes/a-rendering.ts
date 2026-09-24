// Spike A (macOS part): foliate-js pagination in WKWebView with the L1–L3 canvas.
// Cross-engine parity is deferred (macOS-only scope); the page counts recorded
// here are the WKWebView baseline for that comparison.

import type { View } from 'foliate-js/view.js'
import {
  log,
  measurePx,
  openView,
  painted,
  readerCss,
  type Criterion,
  type SpikeResult,
} from './common'

const SIZES = [16, 19, 24]

interface PageCheck {
  chapter: number
  fontPx: number
  pages: number
  splitLines: number
  measureCh: number
}

/** Count line boxes that straddle a page (column) edge, and the measure in ch. */
function inspect(view: View): { splitLines: number; measureCh: number } {
  const { doc } = view.renderer.getContents()[0]
  const win = doc.defaultView!
  const pageHeight = win.innerHeight
  let split = 0
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT)
  const range = doc.createRange()
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (!(n.textContent ?? '').trim()) continue
    range.selectNodeContents(n)
    for (const r of range.getClientRects()) {
      if (r.height === 0) continue
      // In paginated (column) layout every line box must sit wholly inside one page.
      if (r.top < -0.5 || r.bottom > pageHeight + 0.5) split++
    }
  }
  // Measure as characters per line of real text (the unit of L1 “66 ch ≈ 640 px
  // at 19 px” and L5 “above 70 characters per line”): paragraphs of 4+ lines,
  // characters / (lines − ½) to discount the short last line.
  const columnWidth =
    parseFloat(win.getComputedStyle(doc.documentElement).columnWidth) || win.innerWidth
  const perLine: number[] = []
  for (const p of doc.querySelectorAll('p')) {
    const text = (p.textContent ?? '').replace(/\s+/g, ' ').trim()
    if (text.length < 300) continue
    range.selectNodeContents(p)
    const tops = new Set<number>()
    for (const r of range.getClientRects()) {
      // One line box = one top position within one column.
      if (r.width > 0) tops.add(Math.round(r.top) * 1000 + Math.floor((r.left + 1) / columnWidth))
    }
    if (tops.size >= 4) perLine.push(text.length / (tops.size - 0.5))
    if (perLine.length >= 12) break
  }
  const measureCh = perLine.length
    ? Math.round(perLine.reduce((a, b) => a + b, 0) / perLine.length)
    : 0
  return { splitLines: split, measureCh }
}

export async function spikeA(): Promise<SpikeResult> {
  const checks: PageCheck[] = []
  for (const fontPx of SIZES) {
    const { view } = await openView('standardebooks-moby-dick.epub', { fontPx })
    const chapters = view.book.sections
      .map((s, i) => ({ s, i }))
      .filter(({ s }) => s.linear !== 'no' && s.size > 8000)
      .map(({ i }) => i)
    const picked = Array.from(
      { length: 20 },
      (_, k) => chapters[Math.floor((k * chapters.length) / 20)],
    )
    for (const chapter of picked) {
      await view.goTo(chapter)
      view.renderer.setStyles(readerCss(fontPx))
      await painted()
      await painted()
      const { splitLines, measureCh } = inspect(view)
      checks.push({ chapter, fontPx, pages: view.renderer.pages - 2, splitLines, measureCh })
    }
    view.close()
    log(`A: ${fontPx}px done`)
  }
  const split = checks.reduce((a, c) => a + c.splitLines, 0)
  const measures = checks.map((c) => c.measureCh).filter((m) => m > 0)
  const inRange = measures.filter((m) => m >= 56 && m <= 74).length
  const criteria: Criterion[] = [
    {
      id: 'A-no-split-lines',
      description: 'No clipped or split lines at page boundaries (20 chapters × 3 sizes)',
      verdict: split === 0 ? 'pass' : 'fail',
      evidence: `${split} line boxes crossing a page edge across ${checks.length} chapter layouts`,
    },
    {
      id: 'A-measure',
      description: `L1 measure 56–74 characters per line (column max-inline-size ${SIZES.map(measurePx).join('/')} px)`,
      verdict: inRange === measures.length ? 'pass' : 'fail',
      evidence: `${inRange}/${measures.length} layouts within 56–74 characters per line; range ${Math.min(...measures)}–${Math.max(...measures)}`,
    },
    {
      id: 'A-parity',
      description:
        'Page count per chapter differs by ≤ 2% between WKWebView, WebView2 and WebKitGTK',
      verdict: 'deferred',
      evidence: 'macOS-only scope; WKWebView page counts recorded in raw.checks as the baseline',
    },
    {
      id: 'A-screenshots',
      description: 'Screenshots differ only in font rasterisation across engines',
      verdict: 'deferred',
      evidence: 'macOS-only scope',
    },
  ]
  return { spike: 'a-rendering-macos', criteria, raw: { checks } }
}
