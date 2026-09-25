// Generated cover tints (E6; Screen 01's palette). The core picks one at import
// for books without a cover; this is the same idea for a cover that fails to load.

export const COVER_TINTS = [
  '#B9C6CC',
  '#D8C8A8',
  '#C9BED6',
  '#BCC9A8',
  '#E0C4BE',
  '#A9B4A4',
  '#D6CFC0',
  '#CDB592',
  '#D9B8A0',
  '#C8D3B6',
  '#C9A9A6',
  '#B8B3A6',
  '#D8C3CF',
  '#CFC39A',
] as const

export function generatedTint(title: string): string {
  let h = 0
  for (const ch of title) h = (h * 31 + ch.codePointAt(0)!) >>> 0
  return COVER_TINTS[h % COVER_TINTS.length]
}
