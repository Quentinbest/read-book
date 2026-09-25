// Transforms applied to book content before foliate-js turns it into documents
// (plan §7.1, L13, L14; decisions D-E1, D-X1).
//
// Book XML with entity declarations never gets here: the importer refuses it.

/** Per-document CSP (D-E1), in addition to the app CSP that WebKit applies to blob documents. */
export const BOOK_DOCUMENT_CSP =
  "default-src 'none'; img-src blob: data: linen-book:; style-src blob: 'unsafe-inline'; font-src blob: data:; media-src blob: linen-book:"

export function injectDocumentCsp(html: string): string {
  const meta = `<meta http-equiv="Content-Security-Policy" content="${BOOK_DOCUMENT_CSP}"/>`
  if (/<head[\s>]/i.test(html)) return html.replace(/<head(\s[^>]*)?>/i, (m) => m + meta)
  if (/<html[\s>]/i.test(html))
    return html.replace(/<html(\s[^>]*)?>/i, (m) => `${m}<head>${meta}</head>`)
  return meta + html
}

const PX_PER_REM = 16

/**
 * L14 and L13 on a CSS string:
 * - `position: fixed` and `sticky` become `static`, so a book cannot overlay the page or fake UI;
 * - remote `url(...)` and `@import` of remote sheets are removed (the CSP blocks them anyway);
 * - absolute font sizes (px, pt) become rem, so the reader's size setting scales everything.
 */
export function sanitizeCss(css: string): string {
  return css
    .replace(/@import\s+(?:url\()?\s*["']?\s*(?:https?:)?\/\/[^;]*;?/gi, '')
    .replace(/url\(\s*(["']?)\s*(?:https?:)?\/\/[^)]*\)/gi, 'none')
    .replace(/position\s*:\s*(fixed|sticky)\b/gi, 'position: static')
    .replace(/font-size\s*:\s*(\d*\.?\d+)\s*(px|pt)\b/gi, (_m, n: string, unit: string) => {
      const px = unit.toLowerCase() === 'pt' ? (parseFloat(n) * 4) / 3 : parseFloat(n)
      return `font-size: ${+(px / PX_PER_REM).toFixed(4)}rem`
    })
}

/** Apply `sanitizeCss` to <style> elements and style attributes of an (X)HTML document string. */
export function sanitizeDocumentStyles(html: string): string {
  return html
    .replace(
      /(<style\b[^>]*>)([\s\S]*?)(<\/style>)/gi,
      (_m, open: string, css: string, close: string) => open + sanitizeCss(css) + close,
    )
    .replace(
      /(\sstyle\s*=\s*)(["'])([\s\S]*?)\2/gi,
      (_m, pre: string, q: string, css: string) => `${pre}${q}${sanitizeCss(css)}${q}`,
    )
}

/** foliate-js `transformTarget` hook: everything the WebView will parse passes through here. */
export function transformContent(data: unknown, type: string): unknown {
  if (typeof data !== 'string') return data
  if (/css/.test(type)) return sanitizeCss(data)
  if (/(x?html|svg)/.test(type)) return injectDocumentCsp(sanitizeDocumentStyles(data))
  return data
}
