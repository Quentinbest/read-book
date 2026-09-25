import { describe, expect, it } from 'vitest'
import { contrast } from '../theme/tokens'
import { COVER_TINTS, generatedTint } from './tint'

describe('E6 generated covers', () => {
  it('lettering keeps ≥ 7:1 on every tint (title in ink, author in #3F3A33 ≥ 4.5:1)', () => {
    for (const tint of COVER_TINTS) {
      expect(contrast('#22201C', tint), tint).toBeGreaterThanOrEqual(7)
      expect(contrast('#3F3A33', tint), tint).toBeGreaterThanOrEqual(4.5)
    }
  })
  it('the same title always gets the same tint', () => {
    expect(generatedTint('Walden')).toBe(generatedTint('Walden'))
  })
})
