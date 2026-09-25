// Design tokens (plan V1–V9, design S4, Screens 10 and 14).
//
// Components read these through CSS custom properties and contain no colour
// values of their own. Values marked PROVISIONAL are not in the approved design
// (plan §1.2) and need owner sign-off.

export type ThemeName = 'paper' | 'sepia' | 'night'
export type HighlightColor = 'yellow' | 'green' | 'blue' | 'rose'
export const HIGHLIGHT_COLORS: HighlightColor[] = ['yellow', 'green', 'blue', 'rose']

export interface Theme {
  name: ThemeName
  ground: string
  ink: string
  inkSecondary: string
  accent: string
  hairline: string
  /** Night chrome keeps the page colour with its own hairline (V2). */
  chromeHairline: string
  /** Side panels (Navigator, the library sidebar): Screen 04 and 01. */
  panel: string
  /** The track behind segmented controls and tabs on a panel. */
  controlTrack: string
  /** A raised surface on the track: the selected segment or tab. */
  raised: string
  /** The ring around the selected segment (V7: ≥ 3:1 against the track). */
  segmentRing: string
  /** Unselected segment and tab labels: secondary ink, darkened to hold 4.5:1 on the track. */
  trackInk: string
  /** Popovers and peeks (Go to, footnote peek; Screen 17): surface and border. */
  popover: string
  popoverBorder: string
  /** Dark tooltips (the scrubber preview, Screen 03): ground and text. */
  tooltip: string
  tooltipInk: string
  highlight: Record<HighlightColor, { tint: string; underline: string }>
  search: { tint: string; outline: string; activeTint: string; activeOutline: string }
  /** Colour scheme for native controls and scrollbars. */
  scheme: 'light' | 'dark'
}

const LIGHT_HIGHLIGHTS: Theme['highlight'] = {
  yellow: { tint: '#F1DA8A', underline: '#9A7410' },
  green: { tint: '#CFE0B0', underline: '#4E7F2A' },
  blue: { tint: '#C8DAEC', underline: '#3F6FA8' },
  rose: { tint: '#EFCACA', underline: '#B0474A' },
}

// Approved 2026-09-24 (C7): S4 draws highlights on Paper only. Reusing the Paper tints on
// Sepia leaves rose text at 8.6:1, under X1's 9:1, so Sepia uses the Paper tints
// at 75% over the Sepia ground; underlines are unchanged.
const SEPIA_HIGHLIGHTS: Theme['highlight'] = {
  yellow: { tint: '#F1DD9C', underline: '#9A7410' },
  green: { tint: '#D8E2B8', underline: '#4E7F2A' },
  blue: { tint: '#D2DDE6', underline: '#3F6FA8' },
  rose: { tint: '#F0D1CC', underline: '#B0474A' },
}

// Approved 2026-09-24 (C7): the Paper outline #A0705A is 2.9:1 on Sepia, under 3:1; this
// darker outline gives 3.9:1, close to the 3.8:1 that F5 states.
const SEPIA_SEARCH: Theme['search'] = {
  tint: 'rgba(140,74,47,.12)',
  outline: '#8C5A45',
  activeTint: 'rgba(140,74,47,.28)',
  activeOutline: '#8C4A2F',
}

const LIGHT_SEARCH: Theme['search'] = {
  tint: 'rgba(140,74,47,.12)',
  outline: '#A0705A',
  activeTint: 'rgba(140,74,47,.28)',
  activeOutline: '#8C4A2F',
}

export const THEMES: Record<ThemeName, Theme> = {
  paper: {
    name: 'paper',
    ground: '#F7F3EC',
    ink: '#22201C',
    inkSecondary: '#6B655C',
    accent: '#8C4A2F',
    hairline: '#E4DDD1',
    chromeHairline: '#E4DDD1',
    // As drawn in Screens 01 and 04.
    panel: '#F1ECE3',
    controlTrack: '#E6DFD3',
    raised: '#FBF8F3',
    // PROVISIONAL: Screen 04 draws the ring #8F877B, 2.7:1 on the track, under V7's
    // 3:1; secondary ink (as the Phase 1 segmented control) gives 4.4:1.
    segmentRing: '#6B655C',
    trackInk: '#5E584F',
    popover: '#FCFAF6',
    popoverBorder: '#D9D1C3',
    tooltip: '#2A2622',
    tooltipInk: '#F3EEE6',
    highlight: LIGHT_HIGHLIGHTS,
    search: LIGHT_SEARCH,
    scheme: 'light',
  },
  sepia: {
    name: 'sepia',
    ground: '#F1E6D2',
    ink: '#3B2F22',
    inkSecondary: '#6E5E4A',
    accent: '#8C4A2F',
    hairline: '#E2D4BC',
    chromeHairline: '#E2D4BC',
    // PROVISIONAL (G5: Sepia panels are derived): Paper's steps from its ground, applied to Sepia's.
    panel: '#EBDFC9',
    controlTrack: '#E0D2BD',
    raised: '#F5EBD9',
    segmentRing: '#6E5E4A',
    trackInk: '#615139',
    // PROVISIONAL (G5): Paper's steps from its ground.
    popover: '#F6EDDC',
    popoverBorder: '#D3C4A9',
    tooltip: '#2E261D',
    tooltipInk: '#F5EBDA',
    highlight: SEPIA_HIGHLIGHTS,
    search: SEPIA_SEARCH,
    scheme: 'light',
  },
  night: {
    name: 'night',
    ground: '#1B1A18',
    ink: '#D9D3C7',
    inkSecondary: '#9A9387',
    accent: '#D39A73',
    hairline: '#2E2C29',
    chromeHairline: '#3A3733',
    // PROVISIONAL (G5: Night panels are derived): a panel sits a step above the ground.
    panel: '#22211E',
    controlTrack: '#2E2C29',
    raised: '#3A3733',
    segmentRing: '#8F877B',
    trackInk: '#A7A094',
    // PROVISIONAL (G5): popovers a step above the Night panel; tooltips invert to light.
    popover: '#26241F',
    popoverBorder: '#3A3733',
    tooltip: '#D9D3C7',
    tooltipInk: '#1B1A18',
    // Approved 2026-09-24 (C7, docs/decisions.md): S4 draws Night tints at 16%
    // alpha, which leaves highlighted text at 8.2–8.8:1, under X1's 9:1. At 11%
    // every colour reaches ≥ 9.2:1. Underline colours are as approved.
    highlight: {
      yellow: { tint: 'rgba(227,195,90,.11)', underline: '#E3C35A' },
      green: { tint: 'rgba(160,196,112,.11)', underline: '#A0C470' },
      blue: { tint: 'rgba(128,170,214,.11)', underline: '#80AAD6' },
      rose: { tint: 'rgba(226,142,142,.11)', underline: '#E28E8E' },
    },
    // Approved 2026-09-24 (C7): the design shows search marks on light themes
    // only; Night's are derived from the Night accent with the same structure.
    search: {
      tint: 'rgba(211,154,115,.14)',
      outline: '#B98664',
      activeTint: 'rgba(211,154,115,.26)',
      activeOutline: '#D39A73',
    },
    scheme: 'dark',
  },
}

/** Type (V3): UI sizes 12 / 14 / 17 only, weights 400 / 500 / 600; reading 19 / 1.55 in Literata. */
export const TYPE = {
  uiFamily: 'system-ui, -apple-system, "SF Pro Text", "Segoe UI Variable", sans-serif',
  readingFamily: 'Literata, Georgia, serif',
  caption: 12,
  body: 14,
  title: 17,
  weights: [400, 500, 600] as const,
  readingSize: 19,
  readingLineHeight: 1.55,
}

/** Spacing (V4). */
export const SPACE = [4, 8, 12, 16, 24, 32, 48, 64] as const
/** Radii (V4): controls, popovers and cards, sheets. Dots and chips are round. */
export const RADIUS = { control: 6, popover: 10, sheet: 14 } as const

/** Motion in ms (V8). Nothing in the reader exceeds 250 ms (V9). */
export const MOTION = {
  chromeIn: 160,
  chromeOut: 220,
  navigator: 220,
  popover: 140,
  selectionBar: 100,
  pageTurn: 0,
  pageTurnCrossfade: 120,
  swipeSettle: 180,
  jumpPulse: 1200,
  theme: 200,
  libraryToBook: 240,
  /** Under reduced motion everything becomes a fade of at most this. */
  reducedMax: 100,
} as const

// ---------------------------------------------------------------- contrast

type RGBA = [number, number, number, number]

export function parseColor(c: string): RGBA {
  const hex = /^#([0-9a-f]{6})$/i.exec(c)
  if (hex) {
    const n = parseInt(hex[1], 16)
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1]
  }
  const rgba = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(c)
  if (rgba) return [+rgba[1], +rgba[2], +rgba[3], rgba[4] === undefined ? 1 : +rgba[4]]
  throw new Error(`unsupported colour ${c}`)
}

/** Composite a possibly translucent colour over an opaque background. */
export function over(top: string, bottom: string): string {
  const [r, g, b, a] = parseColor(top)
  const [R, G, B] = parseColor(bottom)
  const mix = (x: number, y: number) => Math.round(x * a + y * (1 - a))
  return (
    '#' + [mix(r, R), mix(g, G), mix(b, B)].map((v) => v.toString(16).padStart(2, '0')).join('')
  )
}

function luminance(c: string): number {
  const [r, g, b] = parseColor(c).map((v, i) => {
    if (i === 3) return v
    const s = v / 255
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** WCAG 2 contrast ratio between two opaque colours. */
export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p)
  return (x + 0.05) / (y + 0.05)
}

// ---------------------------------------------------------------- CSS

const kebab = (s: string) => s.replace(/[A-Z]/g, (m) => '-' + m.toLowerCase())

/** CSS custom properties for a theme, e.g. --ground, --hl-yellow-tint. */
export function themeVariables(t: Theme): Record<string, string> {
  const vars: Record<string, string> = {}
  for (const key of [
    'ground',
    'ink',
    'inkSecondary',
    'accent',
    'hairline',
    'chromeHairline',
    'panel',
    'controlTrack',
    'raised',
    'segmentRing',
    'trackInk',
    'popover',
    'popoverBorder',
    'tooltip',
    'tooltipInk',
  ] as const) {
    vars[`--${kebab(key)}`] = t[key]
  }
  for (const c of HIGHLIGHT_COLORS) {
    vars[`--hl-${c}-tint`] = t.highlight[c].tint
    vars[`--hl-${c}-underline`] = t.highlight[c].underline
  }
  for (const [k, v] of Object.entries(t.search)) vars[`--search-${kebab(k)}`] = v
  vars['--hover-wash'] = over(`rgba(${parseColor(t.ink).slice(0, 3).join(',')},.06)`, t.ground)
  return vars
}

export function themeCss(): string {
  const block = (t: Theme) =>
    Object.entries(themeVariables(t))
      .map(([k, v]) => `  ${k}: ${v};`)
      .join('\n') + `\n  color-scheme: ${t.scheme};`
  return [
    `:root, [data-theme='paper'] {\n${block(THEMES.paper)}\n}`,
    `[data-theme='sepia'] {\n${block(THEMES.sepia)}\n}`,
    `[data-theme='night'] {\n${block(THEMES.night)}\n}`,
    // Auto follows the system between Paper and Night (V1).
    `@media (prefers-color-scheme: dark) {\n  [data-theme='auto'] {\n${block(THEMES.night).replace(/^/gm, '  ')}\n  }\n}`,
  ].join('\n\n')
}
