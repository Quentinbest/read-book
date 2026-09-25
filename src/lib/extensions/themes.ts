// Theme packs (P9): declarative token JSON from an extension, listed with the
// built-in themes. A pack gives the page, ink, secondary ink, accent and hairline
// (and may give the panel and popover colours); the rest is derived the way G5
// derived Sepia and Night. A pack is listed only if it keeps X1's contrast.

import { contrast, over, parseColor, THEMES, type Theme } from '../theme/tokens'
import type { ExtensionManifest } from './types'

type Pack = ExtensionManifest['contributes']['themes'][number]

/** `amount` of `b` mixed into `a`. */
function mix(a: string, b: string, amount: number): string {
  const [r, g, bl] = parseColor(b)
  return over(`rgba(${r},${g},${bl},${amount})`, a)
}

export function themeFromPack(extId: string, pack: Pack): Theme {
  const t = pack.tokens
  const base = pack.scheme === 'dark' ? THEMES.night : THEMES.paper
  const ground = t.ground
  const ink = t.ink
  const light = pack.scheme === 'light'
  return {
    name: `ext:${extId}/${pack.id}`,
    ground,
    ink,
    inkSecondary: t.inkSecondary,
    accent: t.accent,
    hairline: t.hairline,
    chromeHairline: t.chromeHairline ?? t.hairline,
    panel: t.panel ?? mix(ground, ink, 0.04),
    controlTrack: t.controlTrack ?? mix(ground, ink, 0.08),
    raised: t.raised ?? (light ? mix(ground, '#FFFFFF', 0.5) : mix(ground, ink, 0.14)),
    segmentRing: t.segmentRing ?? t.inkSecondary,
    trackInk: t.trackInk ?? t.inkSecondary,
    popover: t.popover ?? (light ? mix(ground, '#FFFFFF', 0.4) : mix(ground, ink, 0.05)),
    popoverBorder: t.popoverBorder ?? mix(ground, ink, 0.18),
    tooltip: t.tooltip ?? ink,
    tooltipInk: t.tooltipInk ?? ground,
    // The selection bar: ink-coloured on light pages, inverted on dark ones (V2).
    selectionBar: {
      ground: light ? ink : mix(ink, ground, 0.08),
      ink: light ? ground : ink === '#FFFFFF' ? '#000000' : mix(ground, '#000000', 0.2),
      divider: mix(light ? ink : ink, ground, 0.3),
      ring: light ? 'rgba(255,255,255,.55)' : ground,
      hover: light ? 'rgba(255,255,255,.1)' : 'rgba(0,0,0,.08)',
    },
    // Highlights and search keep the built-in colours of the same scheme.
    highlight: base.highlight,
    search: base.search,
    scheme: pack.scheme,
  }
}

/** Why a pack cannot be offered (empty when it can): X1, V7 and the highlights. */
export function packProblems(theme: Theme): string[] {
  const problems: string[] = []
  const check = (what: string, a: string, b: string, min: number) => {
    const r = contrast(a, b)
    if (r < min) problems.push(`${what} is ${r.toFixed(1)}:1 (needs ${min}:1)`)
  }
  check('text', theme.ink, theme.ground, 7)
  check('secondary text', theme.inkSecondary, theme.ground, 4.5)
  check('the accent', theme.accent, theme.ground, 3)
  check('panel text', theme.ink, theme.panel, 7)
  for (const [c, h] of Object.entries(theme.highlight)) {
    const bg = over(h.tint, theme.ground)
    check(`highlighted text (${c})`, theme.ink, bg, 7)
  }
  return problems
}
