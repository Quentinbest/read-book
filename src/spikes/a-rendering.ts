// Spike A: foliate-js pagination with the L1–L3 canvas. Runs on each engine
// (WKWebView, WebView2, WebKitGTK) and reports as a-rendering-<platform>;
// scripts/compare-spike-a.mjs compares the page counts (next-steps plan, Phase 10).
// Literata is loaded from the bundle, so every engine lays out the same font.

import type { View } from 'foliate-js/view.js'
import {
  log,
  measurePx,
  openView,
  painted,
  platform,
  readerCss,
  type Criterion,
  type SpikeResult,
} from './common'
import { literataFaces } from '../reader/fonts'

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
  const faces = await literataFaces()
  let literata = true
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
      view.renderer.setStyles(faces + readerCss(fontPx))
      const { doc } = view.renderer.getContents()[0]
      await doc.fonts.ready
      literata &&= doc.fonts.check(`${fontPx}px Literata`)
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
      id: 'A-literata',
      description: 'The bundled Literata is the font laid out (a fair cross-engine comparison)',
      verdict: literata ? 'pass' : 'fail',
      evidence: literata
        ? 'Literata loaded at every size'
        : 'Literata did not load; a fallback font was measured',
    },
    {
      id: 'A-parity',
      description:
        'Page count per chapter differs by ≤ 2% between WKWebView, WebView2 and WebKitGTK',
      verdict: 'deferred',
      evidence: 'computed across runs by scripts/compare-spike-a.mjs',
    },
  ]
  return {
    spike: `a-rendering-${platform()}`,
    criteria,
    raw: { engine: navigator.userAgent, checks },
  }
}

interface Variant {
  id: string
  book: string
  width?: number
  height?: number
  css?: string
}

/**
 * Spike A on content Moby-Dick at one column doesn't exercise (next-steps plan,
 * Phase 10): RTL, vertical writing, CJK, hyphenation, fixed layout, the two-page
 * spread, and 200% zoom (a 1280 × 800 window at 200% is 640 × 400 CSS px).
 * Reported as ax-content-<platform>, compared like Spike A.
 */
const VARIANTS: Variant[] = [
  { id: 'rtl-arabic', book: 'idpf-regime-anticancer-arabic.epub' },
  { id: 'vertical-japanese', book: 'idpf-kusamakura-japanese-vertical-writing.epub' },
  { id: 'cjk-horizontal', book: 'idpf-jlreq-in-japanese.epub' },
  { id: 'fixed-layout', book: 'idpf-sous-le-vent.epub' },
  {
    id: 'hyphenation',
    book: 'standardebooks-moby-dick.epub',
    css: 'p { hyphens: auto; -webkit-hyphens: auto; text-align: justify; }',
  },
  { id: 'spread-1600', book: 'standardebooks-moby-dick.epub', width: 1600, height: 900 },
  { id: 'zoom-200', book: 'standardebooks-moby-dick.epub', width: 640, height: 400 },
]

export async function spikeAx(): Promise<SpikeResult> {
  const faces = await literataFaces()
  const checks: (PageCheck & { variant: string })[] = []
  const failures: string[] = []
  const fontPx = 19
  for (const v of VARIANTS) {
    try {
      const { view } = await openView(v.book, { fontPx, width: v.width, height: v.height })
      const sections = view.book.sections
        .map((s, i) => ({ s, i }))
        .filter(({ s }) => s.linear !== 'no' && s.size > 2000)
        .map(({ i }) => i)
      const picked = [
        ...new Set(
          Array.from({ length: 8 }, (_, k) => sections[Math.floor((k * sections.length) / 8)]),
        ),
      ]
      for (const chapter of picked.filter((c) => c !== undefined)) {
        await view.goTo(chapter)
        view.renderer.setStyles?.(faces + readerCss(fontPx) + (v.css ?? ''))
        await view.renderer.getContents?.()[0]?.doc.fonts.ready
        await painted()
        await painted()
        const { splitLines, measureCh } = inspect(view)
        // Fixed-layout books have no paginator page count; count the section once.
        const pages = typeof view.renderer.pages === 'number' ? view.renderer.pages - 2 : 1
        checks.push({ variant: v.id, chapter, fontPx, pages, splitLines, measureCh })
      }
      view.close()
      log(`AX: ${v.id} done`)
    } catch (e) {
      failures.push(`${v.id}: ${e instanceof Error ? e.message : String(e)}`)
    }
  }
  const split = checks
    .filter((c) => c.variant !== 'fixed-layout')
    .reduce((a, c) => a + c.splitLines, 0)
  return {
    spike: `ax-content-${platform()}`,
    criteria: [
      {
        id: 'AX-opens',
        description: 'Every variant opens and lays out',
        verdict: failures.length ? 'fail' : 'pass',
        evidence: failures.length
          ? failures.join('; ')
          : `${VARIANTS.length} variants, ${checks.length} sections`,
      },
      {
        id: 'AX-no-split-lines',
        description: 'No clipped or split lines at page boundaries (reflowable variants)',
        verdict: split === 0 ? 'pass' : 'fail',
        evidence: `${split} line boxes crossing a page edge`,
      },
    ],
    raw: { engine: navigator.userAgent, checks },
  }
}
