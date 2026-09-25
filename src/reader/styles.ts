// Reader styles injected into every book document (plan L4–L7, L11–L13, V2).
//
// The publisher decides structure (headings, emphasis, small caps, poetry
// indents, images, tables); the reader decides comfort: size, body line height,
// text and background colours. Everything here is either a comfort override or a
// guard that keeps publisher layout inside the page.

import type { Theme } from '../lib/theme/tokens'

export interface ReaderStyleOptions {
  fontPx: number
  lineHeight: number
  theme: Theme
  /** L7: justified text only when hyphenation is on. */
  hyphenate: boolean
  /** Page height, so images fit a page (L11). */
  pageHeight: number
}

export function readerStyles(o: ReaderStyleOptions): string {
  const { theme } = o
  const night = theme.scheme === 'dark'
  return `
    /* N9: the note reference keeps a focus ring while its peek is open. */
    .linen-peek-marker {
      outline: 2px solid ${theme.accent} !important;
      outline-offset: 2px;
      border-radius: 3px;
    }
    /* N11: images open the image view. */
    img:not(a img), svg image:not(a image) {
      cursor: zoom-in;
    }
    html {
      font-size: ${o.fontPx}px !important;
      color: ${theme.ink} !important;
      background: ${theme.ground} !important;
      color-scheme: ${theme.scheme};
      -webkit-text-size-adjust: none;
    }
    body {
      font-size: 1rem !important;
      line-height: ${o.lineHeight} !important;
      font-family: Literata, Georgia, serif;
      background: transparent !important;
      ${o.hyphenate ? 'hyphens: auto; -webkit-hyphens: auto;' : 'hyphens: manual; -webkit-hyphens: manual;'}
    }
    /* Comfort: the body line height and colours belong to the reader. */
    p, li, dd, dt, blockquote, td, th, figcaption { line-height: inherit !important; }
    body *:not(a) { color: inherit !important; background-color: transparent !important; }
    a { color: ${theme.accent} !important; }
    ${o.hyphenate ? '' : 'p, li, dd, blockquote { text-align: start !important; }'}
    /* L11: images fit the column and the page, never split, dimmed at Night (never inverted). */
    img, svg, video, picture, object {
      max-width: 100% !important;
      max-height: ${Math.floor(o.pageHeight)}px !important;
      height: auto;
      object-fit: contain;
      break-inside: avoid;
      ${night ? 'filter: brightness(0.9);' : ''}
    }
    figure { break-inside: avoid; }
    /* L12: wide tables and code scroll inside their own box; code keeps its whitespace. */
    .linen-scroll {
      overflow-x: auto;
      max-width: 100%;
      -webkit-mask-image: linear-gradient(to right, #000 calc(100% - 24px), transparent);
    }
    .linen-scroll.linen-at-end { -webkit-mask-image: none; }
    tr { break-inside: avoid; }
    pre, code, kbd, samp {
      hyphens: manual !important;
      -webkit-hyphens: manual !important;
      text-align: start !important;
    }
    pre { white-space: pre !important; }
  `
}

/** L6: when a book's paragraphs have neither an indent nor spacing, add 0.8 em spacing. */
export const PARAGRAPH_SPACING_CSS = 'p { margin-block: 0 0.8em !important; }'
