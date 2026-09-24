import { describe, expect, it } from 'vitest'
import { computeLayout, showLocationLine, widthClass } from './layout'

const base = { fontPx: 19, spacing: 'default' as const }

describe('Screen 02 geometry at 1280 × 800 (L1–L3)', () => {
  const l = computeLayout({ width: 1280, height: 800, ...base })

  it('puts a 640 px column at x = 320', () => {
    expect(l.columnWidth).toBe(640)
    expect(l.left).toBe(320)
    expect(Math.round(l.measureCh)).toBe(66)
  })

  it('puts the page at y = 88 with 21 whole lines of 29.45 px', () => {
    expect(l.top).toBe(88)
    expect(l.lineHeightPx).toBeCloseTo(29.45)
    expect(l.lines).toBe(21)
    expect(l.pageHeight).toBeCloseTo(618.45, 1)
    expect(l.pageHeight / l.lineHeightPx).toBeCloseTo(21) // never a half line (L3)
  })

  it('sizes the foliate view so its text column is exactly the measure', () => {
    // viewWidth × (1 − 2 × gap) = column (paginator geometry)
    expect(l.viewWidth * 0.92).toBeCloseTo(640, 0)
    expect(l.viewLeft + (l.viewWidth - 640) / 2).toBeCloseTo(320, 0)
  })
})

describe('L2 margins absorb width; columns never exceed the measure', () => {
  it('on a wide window the column stays 66 ch', () => {
    expect(computeLayout({ width: 2400, height: 1200, ...base }).columnWidth).toBe(640)
  })

  it('on a narrow window margins shrink to 24 px before the column does', () => {
    const l = computeLayout({ width: 600, height: 800, ...base })
    expect(l.left).toBe(24)
    expect(l.columnWidth).toBe(600 - 48)
  })

  it('at large sizes the column fills to the minimum margin', () => {
    const l = computeLayout({ width: 1280, height: 800, fontPx: 40, spacing: 'default' })
    expect(l.left).toBe(24)
    expect(l.columnWidth).toBe(1280 - 48)
  })

  it('a docked Navigator narrows the reading area', () => {
    const l = computeLayout({ width: 1280, height: 800, ...base, navigatorWidth: 320 })
    expect(l.left).toBe(Math.round((960 - 640) / 2))
  })
})

describe('L5 line height', () => {
  it('Compact and Loose', () => {
    expect(
      computeLayout({ width: 1280, height: 800, fontPx: 19, spacing: 'compact' }).lineHeight,
    ).toBe(1.4)
    expect(
      computeLayout({ width: 1280, height: 800, fontPx: 19, spacing: 'loose' }).lineHeight,
    ).toBe(1.75)
  })

  it('+0.05 above 70 characters per line', () => {
    // A font with narrow letters sets more than 70 characters in the same 66 ch.
    const l = computeLayout({ width: 1280, height: 800, ...base, avgCharEm: 0.44 })
    expect(l.charsPerLine).toBeGreaterThan(70)
    expect(l.lineHeight).toBeCloseTo(1.6)
    expect(computeLayout({ width: 1280, height: 800, ...base }).lineHeight).toBe(1.55)
  })
})

describe('L8 breakpoints and the two-page spread', () => {
  it('classifies widths', () => {
    expect(widthClass(700)).toBe('narrow')
    expect(widthClass(900)).toBe('medium')
    expect(widthClass(1200)).toBe('wide')
    expect(widthClass(1500)).toBe('spread')
  })

  it('shows two pages from 1480 px of reading width, only when allowed', () => {
    expect(computeLayout({ width: 1600, height: 900, ...base, allowSpread: true }).columns).toBe(2)
    expect(computeLayout({ width: 1600, height: 900, ...base }).columns).toBe(1)
    // B12: the width left after a docked Navigator decides.
    expect(
      computeLayout({ width: 1600, height: 900, ...base, allowSpread: true, navigatorWidth: 320 })
        .columns,
    ).toBe(1)
  })
})

describe('L9', () => {
  it('hides the location line below 480 px of height', () => {
    expect(showLocationLine(479)).toBe(false)
    expect(showLocationLine(480)).toBe(true)
  })
})
