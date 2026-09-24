// Literata, the bundled reading font (T5, OFL; plan L15).
//
// Book documents only accept fonts from blob: and data: URLs (per-document CSP,
// D-E1), so the font is fetched once and handed to them as a blob URL.

let faces: Promise<string> | null = null

async function blobUrl(path: string): Promise<string> {
  const res = await fetch(path)
  return URL.createObjectURL(await res.blob())
}

/** @font-face rules for Literata, safe to inject into book documents. */
export function literataFaces(): Promise<string> {
  faces ??= Promise.all([blobUrl('/fonts/Literata.ttf'), blobUrl('/fonts/Literata-Italic.ttf')])
    .then(
      ([roman, italic]) => `
        @font-face { font-family: Literata; src: url("${roman}") format("truetype"); font-weight: 200 900; font-style: normal; font-display: block; }
        @font-face { font-family: Literata; src: url("${italic}") format("truetype"); font-weight: 200 900; font-style: italic; font-display: block; }
      `,
    )
    .catch(() => '') // Georgia remains the fallback
  return faces
}
