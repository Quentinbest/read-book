import { describe, expect, it } from 'vitest'
import { packProblems, themeFromPack } from './themes'

const nightOwl = {
  id: 'night-owl',
  title: 'Night Owl',
  scheme: 'dark' as const,
  tokens: {
    ground: '#0B1622',
    ink: '#C9D4E0',
    inkSecondary: '#8A9BB0',
    accent: '#7FB0E0',
    hairline: '#1E2C3C',
  },
}

describe('P9 theme packs', () => {
  it('derives a whole theme from the five core colours', () => {
    const t = themeFromPack('org.example.night-owl', nightOwl)
    expect(t.name).toBe('ext:org.example.night-owl/night-owl')
    expect(t.scheme).toBe('dark')
    expect(t.panel).toMatch(/^#[0-9a-f]{6}$/)
    expect(packProblems(t)).toEqual([])
  })
  it('refuses a pack that would break X1', () => {
    const t = themeFromPack('x', { ...nightOwl, tokens: { ...nightOwl.tokens, ink: '#3A4A5A' } })
    expect(packProblems(t)[0]).toMatch(/^text is/)
  })
})

describe('the Night Owl sample (examples/extensions/night-owl)', () => {
  it('passes the pack checks', async () => {
    const m = (await import('../../../examples/extensions/night-owl/manifest.json')).default
    const pack = m.contributes.themes[0] as Parameters<typeof themeFromPack>[1]
    expect(packProblems(themeFromPack(m.id, pack))).toEqual([])
  })
})
