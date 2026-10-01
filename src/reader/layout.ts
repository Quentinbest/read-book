// Reading-canvas geometry (plan L1–L5, L8; design S3 and Screen 02).
//
// Pure: given the window and the font, where the text column and the page go.
// At 1280 × 800 and 19 px it reproduces Screen 02: a 640 px column at x = 320,
// the page at y = 88, 21 lines of 29.45 px.

export const MEASURE_CH = 66
export const MEASURE_MIN_CH = 56
export const MEASURE_MAX_CH = 74
/** L1: 66 ch ≈ 640 px at 19 px, so one ch of the reading font ≈ 0.51 em. */
export const DEFAULT_CH_EM = 640 / 19 / MEASURE_CH
/** Spike A: about 69 characters of real text per 640 px line at 19 px. */
export const DEFAULT_AVG_CHAR_EM = 640 / 19 / 69
export const MIN_SIDE_MARGIN = 24
/** L3: the vertical margin is max(48 px, 7vh) at each edge… */
export const MIN_VERTICAL_MARGIN = 48
export const VERTICAL_MARGIN_VH = 0.07
/**
 * …plus room for the chrome's reveal zone above and the location line below.
 * Derived from Screen 02 at 800 px: top 56 + 32 = 88, bottom 56 + 37 = 93.
 */
export const TOP_EXTRA = 32
export const BOTTOM_EXTRA = 37
export const LINE_HEIGHTS = { compact: 1.4, default: 1.55, loose: 1.75 } as const
/** L5: +0.05 line height above 70 characters per line. */
export const LONG_LINE_CH = 70
export const LONG_LINE_BONUS = 0.05
/** L8 breakpoints. */
export const BREAKPOINTS = { narrow: 760, dock: 1100, spread: 1480 } as const
/** foliate-js column gap as a fraction of the view (see paginator #beforeRender). */
export const PAGINATOR_GAP = 0.04

export type Spacing = keyof typeof LINE_HEIGHTS

export interface LayoutInput {
  /** Window size in CSS px. */
  width: number
  height: number
  fontPx: number
  spacing: Spacing
  /** Width taken by a docked Navigator (0 when none). */
  navigatorWidth?: number
  /** Width of one ch of the reading font in em, when measured (defaults to L1's ratio). */
  chEm?: number
  /** Average character width of real text in em, when measured (L5). */
  avgCharEm?: number
  /** Allow a two-page spread (Pages mode, L8). */
  allowSpread?: boolean
  /** 1.1 page width: the measure in ch, within L1's clamp (default 66). */
  measureCh?: number
}

export interface Layout {
  /** Text column width (one page) in px. */
  columnWidth: number
  /** Width of the text: one page, or two pages and the gutter (L8). */
  textWidth: number
  /** Measure in ch (L1). */
  measureCh: number
  /** Average characters of text per line (L5). */
  charsPerLine: number
  columns: 1 | 2
  lineHeight: number
  lineHeightPx: number
  lines: number
  /** The page box, relative to the reading area. */
  top: number
  pageHeight: number
  /** Where the text starts horizontally and the side margins (click targets, L2, I11). */
  left: number
  marginWidth: number
  /** The paginator's gap setting, as a fraction of the view (see PAGINATOR_GAP). */
  gap: number
  /** The foliate-view element box that yields exactly `columnWidth` (see PAGINATOR_GAP). */
  viewLeft: number
  viewWidth: number
}

export function computeLayout(input: LayoutInput): Layout {
  const { width, height, fontPx, spacing } = input
  const areaWidth = width - (input.navigatorWidth ?? 0)
  const chPx = (input.chEm ?? DEFAULT_CH_EM) * fontPx

  // L2: the column is min(measure, W − 2 × min margin); extra width becomes margin.
  // At large sizes margins shrink to the minimum before characters per line fall.
  const measureTarget = Math.min(
    MEASURE_MAX_CH,
    Math.max(MEASURE_MIN_CH, input.measureCh ?? MEASURE_CH),
  )
  const measurePx = measureTarget * chPx
  const columns: 1 | 2 =
    input.allowSpread &&
    areaWidth >= BREAKPOINTS.spread &&
    areaWidth - 3 * MIN_SIDE_MARGIN >= 2 * MEASURE_MIN_CH * chPx
      ? 2
      : 1
  const gutter = columns === 2 ? 2 * MIN_SIDE_MARGIN : 0
  const available = (areaWidth - 2 * MIN_SIDE_MARGIN - gutter) / columns
  const columnWidth = Math.floor(Math.min(measurePx, available) + 1e-6)
  const measureCh = columnWidth / chPx
  // L5 counts actual characters of text, which are narrower than the “0” of a ch.
  const charsPerLine = columnWidth / ((input.avgCharEm ?? DEFAULT_AVG_CHAR_EM) * fontPx)

  // L5: body line height, +0.05 above 70 characters per line.
  const lineHeight = LINE_HEIGHTS[spacing] + (charsPerLine > LONG_LINE_CH ? LONG_LINE_BONUS : 0)
  const lineHeightPx = fontPx * lineHeight

  // L3: vertical margins, then snap the page to whole lines.
  const edge = Math.max(MIN_VERTICAL_MARGIN, VERTICAL_MARGIN_VH * height)
  const top = Math.round(edge + TOP_EXTRA)
  const room = height - top - (edge + BOTTOM_EXTRA)
  const lines = Math.max(1, Math.floor(room / lineHeightPx + 1e-6))
  const pageHeight = Math.round(lines * lineHeightPx * 100) / 100

  const textWidth = columns * columnWidth + gutter
  const left = Math.round((areaWidth - textWidth) / 2)
  // foliate-js places text from its grid and gap (paginator.js #beforeRender):
  // with a view of width V and gap g, the text starts g·V in; one column is
  // V(1 − 2g) wide; two columns are V(1 − 3g)/2 each with a g·V gutter.
  // So for one column keep g and size V; for a spread, pick V and g so the
  // gutter is `gutter` and each page is `columnWidth`.
  const viewWidth = columns === 2 ? textWidth + 2 * gutter : textWidth / (1 - 2 * PAGINATOR_GAP)
  const gap = columns === 2 ? gutter / viewWidth : PAGINATOR_GAP
  return {
    columnWidth,
    textWidth,
    measureCh,
    charsPerLine,
    columns,
    lineHeight,
    lineHeightPx,
    lines,
    top,
    pageHeight,
    left,
    marginWidth: left,
    gap,
    viewLeft: Math.round(left - (viewWidth - textWidth) / 2),
    viewWidth: Math.round(viewWidth),
  }
}

/** L8: how the Navigator and panels behave at this width. */
export function widthClass(width: number): 'narrow' | 'medium' | 'wide' | 'spread' {
  if (width < BREAKPOINTS.narrow) return 'narrow'
  if (width < BREAKPOINTS.dock) return 'medium'
  if (width < BREAKPOINTS.spread) return 'wide'
  return 'spread'
}

/** L9: below 480 px of height the location line is hidden. */
export const showLocationLine = (height: number) => height >= 480
