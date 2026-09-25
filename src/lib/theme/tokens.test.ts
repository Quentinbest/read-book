import { describe, expect, it } from 'vitest'
import {
  contrast,
  HIGHLIGHT_COLORS,
  over,
  multipliedTint,
  parseColor,
  THEMES,
  themeCss,
  type ThemeName,
} from './tokens'

const names = Object.keys(THEMES) as ThemeName[]
const r1 = (x: number) => Math.round(x * 10) / 10

describe('V1 theme tokens match the approved values', () => {
  // Ratios as printed in S4 and Screen 10 (ink, secondary, accent against the ground).
  const printed: Record<ThemeName, [number, number, number]> = {
    paper: [14.7, 5.2, 6.0],
    sepia: [10.5, 5.1, 5.4],
    night: [11.7, 5.7, 7.1],
  }
  for (const n of names) {
    it(`${n}: ink, secondary and accent ratios`, () => {
      const t = THEMES[n]
      const got = [
        contrast(t.ink, t.ground),
        contrast(t.inkSecondary, t.ground),
        contrast(t.accent, t.ground),
      ]
      got.forEach((v, i) =>
        expect(Math.abs(v - printed[n][i]), `${n}[${i}] = ${r1(v)}`).toBeLessThanOrEqual(0.1),
      )
    })
  }
})

describe('X1 contrast in every theme', () => {
  for (const n of names) {
    const t = THEMES[n]
    it(`${n}: body text ≥ 7:1, secondary UI text ≥ 4.5:1`, () => {
      expect(contrast(t.ink, t.ground)).toBeGreaterThanOrEqual(7)
      expect(contrast(t.inkSecondary, t.ground)).toBeGreaterThanOrEqual(4.5)
    })

    it(`${n}: highlighted text ≥ 9:1 on every tint`, () => {
      for (const c of HIGHLIGHT_COLORS) {
        const bg = over(t.highlight[c].tint, t.ground)
        expect(contrast(t.ink, bg), `${c} ${r1(contrast(t.ink, bg))}`).toBeGreaterThanOrEqual(9)
      }
    })

    it(`${n}: highlighted text ≥ 9:1 on WebKit without custom highlights (the tint drawn over the text)`, () => {
      const multiply = (a: string, b: string) => {
        const [x, y] = [parseColor(a), parseColor(b)]
        return (
          '#' +
          [0, 1, 2]
            .map((i) =>
              Math.round((x[i] * y[i]) / 255)
                .toString(16)
                .padStart(2, '0'),
            )
            .join('')
        )
      }
      for (const c of HIGHLIGHT_COLORS) {
        const tint = t.highlight[c].tint
        const [text, bg] =
          t.scheme === 'light'
            ? [multiply(t.ink, multipliedTint(t, c)), multiply(t.ground, multipliedTint(t, c))]
            : [over(tint, t.ink), over(tint, t.ground)]
        expect(contrast(text, bg), `${c} ${r1(contrast(text, bg))}`).toBeGreaterThanOrEqual(9)
      }
    })

    it(`${n}: highlight underlines ≥ 3:1 against their tint and the page (A4)`, () => {
      for (const c of HIGHLIGHT_COLORS) {
        const u = t.highlight[c].underline
        const bg = over(t.highlight[c].tint, t.ground)
        expect(contrast(u, bg), `${c} vs tint`).toBeGreaterThanOrEqual(3)
        expect(contrast(u, t.ground), `${c} vs page`).toBeGreaterThanOrEqual(3)
      }
    })

    it(`${n}: search outlines ≥ 3:1 (F5 states 3.8:1 on light themes)`, () => {
      const soft = over(t.search.tint, t.ground)
      const active = over(t.search.activeTint, t.ground)
      expect(contrast(t.search.outline, soft)).toBeGreaterThanOrEqual(3)
      expect(contrast(t.search.activeOutline, active)).toBeGreaterThanOrEqual(3)
      expect(contrast(t.ink, active)).toBeGreaterThanOrEqual(7)
    })

    it(`${n}: the focus ring (accent) is ≥ 3:1 against the page (V7)`, () => {
      expect(contrast(t.accent, t.ground)).toBeGreaterThanOrEqual(3)
    })
  }

  it('light-theme underlines fall in the stated 3.5–4.9:1 band against the page (A4)', () => {
    for (const n of ['paper', 'sepia'] as const) {
      for (const c of HIGHLIGHT_COLORS) {
        const ratio = contrast(THEMES[n].highlight[c].underline, THEMES[n].ground)
        expect(ratio, `${n} ${c} ${r1(ratio)}`).toBeGreaterThanOrEqual(3.45) // the design rounds to one decimal
        expect(ratio, `${n} ${c} ${r1(ratio)}`).toBeLessThanOrEqual(4.95)
      }
    }
  })
})

describe('themeCss', () => {
  it('emits every theme and an Auto block that follows the system', () => {
    const css = themeCss()
    expect(css).toContain("[data-theme='sepia']")
    expect(css).toContain('@media (prefers-color-scheme: dark)')
    expect(css).toContain('--hl-rose-underline: #B0474A')
    expect(css).toContain('--chrome-hairline: #3A3733')
  })
})

describe('Panels (Screens 01, 04; G5 derived for Sepia and Night)', () => {
  for (const n of names) {
    const t = THEMES[n]
    it(`${n}: text on the panel keeps X1 contrast`, () => {
      expect(contrast(t.ink, t.panel)).toBeGreaterThanOrEqual(7)
      expect(contrast(t.inkSecondary, t.panel)).toBeGreaterThanOrEqual(4.5)
      expect(contrast(t.accent, t.panel)).toBeGreaterThanOrEqual(4.5)
    })
    it(`${n}: the current Contents row (accent on a 9% accent tint) stays readable`, () => {
      const [r, g, b] = [1, 3, 5].map((i) => parseInt(t.accent.slice(i, i + 2), 16))
      const row = over(`rgba(${r},${g},${b},.09)`, t.panel)
      expect(contrast(t.accent, row)).toBeGreaterThanOrEqual(4.5)
    })
    it(`${n}: segments: text on the track and the raised segment, and its ring ≥ 3:1 (V7)`, () => {
      expect(contrast(t.trackInk, t.controlTrack)).toBeGreaterThanOrEqual(4.5)
      expect(contrast(t.ink, t.raised)).toBeGreaterThanOrEqual(7)
      expect(contrast(t.segmentRing, t.controlTrack)).toBeGreaterThanOrEqual(3)
    })
  }
})

describe('Popovers and tooltips (Screens 03, 17; G5 derived)', () => {
  for (const n of names) {
    const t = THEMES[n]
    it(`${n}: text on popovers and tooltips keeps X1 contrast`, () => {
      expect(contrast(t.ink, t.popover)).toBeGreaterThanOrEqual(7)
      expect(contrast(t.inkSecondary, t.popover)).toBeGreaterThanOrEqual(4.5)
      expect(contrast(t.accent, t.popover)).toBeGreaterThanOrEqual(4.5)
      expect(contrast(t.tooltipInk, t.tooltip)).toBeGreaterThanOrEqual(7)
    })
  }
})

describe('Selection bar (A1, V2; Screens 06 and 14)', () => {
  for (const [n, th] of Object.entries(THEMES)) {
    it(`${n}: labels on the bar are ≥ 7:1 and the bar stands out from the page`, () => {
      expect(contrast(th.selectionBar.ink, th.selectionBar.ground)).toBeGreaterThanOrEqual(7)
      expect(contrast(th.selectionBar.ground, th.ground)).toBeGreaterThanOrEqual(3)
      expect(
        contrast(over(th.selectionBar.ring, th.selectionBar.ground), th.selectionBar.ground),
      ).toBeGreaterThanOrEqual(3)
    })
  }
  it('Night inverts to a light surface, 13.6:1 against the page (Screen 14)', () => {
    const n = THEMES.night
    expect(contrast(n.selectionBar.ground, n.ground)).toBeCloseTo(13.6, 1)
  })
})

describe('D5 (Phase 8 axe pass): key caps and text on the accent', () => {
  for (const [n, t] of Object.entries(THEMES)) {
    it(`${n}: key caps ≥ 4.5:1 on the raised surface, accent buttons ≥ 4.5:1`, () => {
      expect(contrast(t.keyInk, t.raised)).toBeGreaterThanOrEqual(4.5)
      expect(contrast(t.onAccent, t.accent)).toBeGreaterThanOrEqual(4.5)
    })
  }
})
