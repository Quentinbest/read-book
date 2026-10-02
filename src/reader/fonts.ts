// Literata, the bundled reading font (T5, OFL; plan L15).
//
// Book documents only accept fonts from blob: and data: URLs (per-document CSP,
// D-E1), so the font is fetched once and handed to them as a blob URL.

let faces: Promise<string> | null = null
let romanUrl: Promise<string> | null = null

async function blobUrl(path: string): Promise<string> {
  const res = await fetch(path)
  return URL.createObjectURL(await res.blob())
}

/** @font-face rules for Literata, safe to inject into book documents. */
export function literataFaces(): Promise<string> {
  romanUrl ??= blobUrl('/fonts/Literata.ttf')
  faces ??= Promise.all([romanUrl, blobUrl('/fonts/Literata-Italic.ttf')])
    .then(
      ([roman, italic]) => `
        @font-face { font-family: Literata; src: url("${roman}") format("truetype"); font-weight: 200 900; font-style: normal; font-display: block; }
        @font-face { font-family: Literata; src: url("${italic}") format("truetype"); font-weight: 200 900; font-style: italic; font-display: block; }
      `,
    )
    .catch(() => '') // Georgia remains the fallback
  return faces
}

let dyslexic: Promise<string> | null = null

/** 1.1: @font-face rules for OpenDyslexic (OFL), loaded only when that font is chosen. */
export function dyslexicFaces(): Promise<string> {
  const face = (file: string, weight: number, style: string) =>
    blobUrl(`/fonts/OpenDyslexic-${file}.woff2`).then(
      (url) =>
        `@font-face { font-family: OpenDyslexic; src: url("${url}") format("woff2"); font-weight: ${weight}; font-style: ${style}; font-display: block; }`,
    )
  dyslexic ??= Promise.all([
    face('Regular', 400, 'normal'),
    face('Italic', 400, 'italic'),
    face('Bold', 700, 'normal'),
    face('Bold-Italic', 700, 'italic'),
  ])
    .then((rules) => rules.join('\n'))
    .catch(() => '') // the system sans remains the fallback
  return dyslexic
}

/** L15: a book font that fails to load, or is still loading after this long, falls back to Literata. */
export const FONT_TIMEOUT_MS = 1500

/**
 * Replace each font family of `doc` that failed (or timed out) with Literata,
 * without a prompt. A later @font-face with the same family name wins, so the
 * book's other fonts are untouched. Returns the families replaced.
 */
export async function fallBackFailedFonts(
  doc: Document,
  timeoutMs = FONT_TIMEOUT_MS,
): Promise<string[]> {
  const fonts = doc.fonts
  if (!fonts || fonts.size === 0) return []
  await Promise.race([fonts.ready, new Promise((r) => setTimeout(r, timeoutMs))])
  const failed = new Set<string>()
  fonts.forEach((face) => {
    if (face.status === 'error' || face.status === 'loading')
      failed.add(face.family.replace(/^["']|["']$/g, ''))
  })
  // The reader's own faces never count as failed book fonts.
  failed.delete('Literata')
  failed.delete('OpenDyslexic')
  if (!failed.size) return []
  const url = await (romanUrl ?? blobUrl('/fonts/Literata.ttf'))
  const style = doc.createElement('style')
  style.textContent = [...failed]
    .map(
      (family) =>
        `@font-face { font-family: "${family.replace(/"/g, '')}"; src: url("${url}") format("truetype"); font-weight: 100 900; font-style: normal italic; }`,
    )
    .join('\n')
  doc.head?.append(style)
  return [...failed]
}
