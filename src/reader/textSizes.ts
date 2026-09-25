// L4: text size. 19 px by default; twelve steps from 14 to 32 px in the Aa
// slider (Screen 09), and on up to 48 px with ⌘+.

export const TEXT_SIZES = [14, 15, 16, 17, 18, 19, 20, 22, 24, 26, 28, 32] as const
export const EXTRA_SIZES = [36, 40, 44, 48] as const
export const DEFAULT_TEXT_SIZE = 19
const ALL = [...TEXT_SIZES, ...EXTRA_SIZES]

/** ⌘+ / ⌘−: the next size up or down from `px` (which may be off the steps). */
export function stepTextSize(px: number, dir: 1 | -1): number {
  if (dir > 0) return ALL.find((s) => s > px) ?? ALL[ALL.length - 1]
  return [...ALL].reverse().find((s) => s < px) ?? ALL[0]
}

/** A stored size, kept within 14–48 px. */
export function clampTextSize(px: number): number {
  return Number.isFinite(px)
    ? Math.min(ALL[ALL.length - 1], Math.max(ALL[0], px))
    : DEFAULT_TEXT_SIZE
}
