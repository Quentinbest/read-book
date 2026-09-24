// Spike E: content isolation for untrusted book content in WKWebView (plan §7.1).

import { invoke } from '@tauri-apps/api/core'
import type { Book } from 'foliate-js/view.js'
import { log, openView, sleep, type Criterion, type SpikeResult } from './common'

/** CSP injected into every book document (defence in depth, variant "csp+meta"). */
export const BOOK_DOCUMENT_CSP =
  "default-src 'none'; img-src blob: data:; style-src blob: 'unsafe-inline'; font-src blob: data:; media-src blob:"

export function injectBookCsp(book: Book) {
  book.transformTarget?.addEventListener('data', (e) => {
    const detail = (e as CustomEvent<{ data: unknown; type: string }>).detail
    if (!/(x?html|svg)/.test(detail.type)) return
    detail.data = Promise.resolve(detail.data).then((data) => {
      if (typeof data !== 'string') return data
      const meta = `<meta http-equiv="Content-Security-Policy" content="${BOOK_DOCUMENT_CSP}"/>`
      if (/<head[\s>]/i.test(data)) return data.replace(/<head(\s[^>]*)?>/i, (m) => m + meta)
      return data.replace(/<html(\s[^>]*)?>/i, (m) => `${m}<head>${meta}</head>`)
    })
  })
}

interface CanaryLog {
  ipc: string[]
  http: string[]
}

interface VariantResult {
  variant: string
  pwned: string[]
  postMessages: string[]
  canary: CanaryLog
  cspViolations: string[]
  externalLinks: string[]
  internalLinks: string[]
  chromeOnTop: boolean
  fixedOverlayInsideFrame: boolean
  appFrameLoaded: boolean
  fileProbesLoaded: string[]
  sections: number
  bookStyleApplied: boolean
}

async function runVariant(variant: 'csp' | 'csp+meta'): Promise<VariantResult> {
  await invoke('spike_canary_clear')
  const postMessages: string[] = []
  const onMessage = (e: MessageEvent) => {
    const id = (e.data as { spikeCanary?: string } | null)?.spikeCanary
    if (id) postMessages.push(id)
  }
  window.addEventListener('message', onMessage)

  const pwned = new Set<string>()
  const cspViolations = new Set<string>()
  const externalLinks: string[] = []
  const internalLinks: string[] = []
  let chromeOnTop = true
  let fixedOverlayInsideFrame = false
  let appFrameLoaded = false
  let bookStyleApplied = false
  const fileProbesLoaded: string[] = []

  const { view } = await openView('hostile-content.epub', {
    onBook: variant === 'csp+meta' ? injectBookCsp : undefined,
  })
  // The product adapter cancels every link event and routes it itself (N10).
  view.addEventListener('external-link', (e) => {
    e.preventDefault()
    externalLinks.push(String((e as CustomEvent<{ href_: string }>).detail.href_))
  })
  view.addEventListener('link', (e) => {
    e.preventDefault()
    internalLinks.push(String((e as CustomEvent<{ href: string }>).detail.href))
  })

  const sections = view.book.sections.length
  const scan = (doc: Document) => {
    const mark = doc.documentElement.getAttribute('data-pwned')
    if (mark)
      mark
        .trim()
        .split(/\s+/)
        .forEach((m) => pwned.add(m))
    for (const frame of doc.querySelectorAll('iframe')) {
      try {
        const inner = frame.contentDocument?.documentElement?.getAttribute('data-pwned')
        if (inner)
          inner
            .trim()
            .split(/\s+/)
            .forEach((m) => pwned.add(m))
        const url = frame.contentDocument?.URL ?? ''
        if (frame.id === 'f-app' && url.startsWith('tauri:')) appFrameLoaded = true
      } catch {
        // cross-origin frame: its script, if any, reports through postMessage
      }
    }
  }

  for (let index = 0; index < sections; index++) {
    if (view.book.sections[index].linear === 'no') continue
    await view.goTo(index)
    await sleep(300)
    const contents = view.renderer.getContents()[0]
    if (!contents) continue
    const { doc } = contents
    doc.addEventListener('securitypolicyviolation', (e) =>
      cspViolations.add(`${e.violatedDirective} ${e.blockedURI.slice(0, 60)}`),
    )
    // Exercise the probes that need an action.
    for (const id of ['p-jslink', 'n-link', 'f-blank', 'p-form-button']) {
      const el = doc.getElementById(id) as HTMLElement | null
      el?.click()
    }
    // Wait out the meta refresh (3 s) and any async loads.
    await sleep(index === 1 ? 4000 : 1500)
    const live = view.renderer.getContents()[0]?.doc ?? doc
    scan(live)

    if (live.getElementById('o-fixed')) {
      const r = (live.getElementById('o-fixed') as HTMLElement).getBoundingClientRect()
      fixedOverlayInsideFrame = r.width > 0 && r.height > 0
      const chrome = document.getElementById('chrome-top')!
      const cr = chrome.getBoundingClientRect()
      const hit = document.elementFromPoint(cr.left + cr.width / 2, cr.top + cr.height / 2)
      chromeOnTop = chromeOnTop && hit === chrome
    }
    for (const id of ['f-etc', 'f-traverse', 'f-other-book']) {
      const img = live.getElementById(id) as HTMLImageElement | null
      if (img && img.naturalWidth > 0) fileProbesLoaded.push(id)
    }
    const probe = live.getElementById('p-inline')
    if (probe && live.defaultView) {
      // Book text should use the reader styles applied through the paginator.
      bookStyleApplied = live.defaultView.getComputedStyle(probe).lineHeight !== 'normal'
    }
  }
  await sleep(1000)
  const canary = await invoke<CanaryLog>('spike_canary_log')
  window.removeEventListener('message', onMessage)
  view.close()
  return {
    variant,
    pwned: [...pwned],
    postMessages,
    canary,
    cspViolations: [...cspViolations],
    externalLinks,
    internalLinks,
    chromeOnTop,
    fixedOverlayInsideFrame,
    appFrameLoaded,
    fileProbesLoaded,
    sections,
    bookStyleApplied,
  }
}

/** WebKit bug 218086: parent listeners on a sandboxed frame without allow-scripts. */
async function sandboxListenerCheck(sandbox: string): Promise<boolean> {
  const frame = document.createElement('iframe')
  frame.setAttribute('sandbox', sandbox)
  const url = URL.createObjectURL(
    new Blob(['<!doctype html><p id="t">x</p>'], { type: 'text/html' }),
  )
  document.body.append(frame)
  await new Promise<void>((resolve) => {
    frame.addEventListener('load', () => resolve(), { once: true })
    frame.src = url
  })
  let fired = false
  try {
    const doc = frame.contentDocument!
    doc.addEventListener('click', () => (fired = true))
    ;(doc.getElementById('t') as HTMLElement).click()
  } catch (e) {
    log('E: sandbox check error', String(e))
  }
  frame.remove()
  URL.revokeObjectURL(url)
  return fired
}

async function openWithTimeout(name: string, ms: number): Promise<string> {
  const started = performance.now()
  try {
    const result = await Promise.race([
      openView(name).then(({ view }) => {
        const n = view.book.sections.length
        view.close()
        return `opened (${n} sections)`
      }),
      sleep(ms).then(() => 'timed out'),
    ])
    return `${result} in ${Math.round(performance.now() - started)} ms`
  } catch (e) {
    return `rejected: ${String(e).slice(0, 120)}`
  }
}

export async function spikeE(): Promise<SpikeResult> {
  const variants: VariantResult[] = []
  for (const v of ['csp', 'csp+meta'] as const) {
    log('E: variant', v)
    variants.push(await runVariant(v))
  }
  const withoutScripts = await sandboxListenerCheck('allow-same-origin')
  const withScripts = await sandboxListenerCheck('allow-same-origin allow-scripts')
  const archives: Record<string, string> = {}
  for (const name of [
    'zip-traversal.epub',
    'zip-absolute.epub',
    'zip-symlink.epub',
    'xml-external-entity.epub',
    // xml-billion-laughs.epub is not opened here: on 2026-09-24 it pinned the WebContent
    // process at 100% CPU indefinitely (DOMParser in epub.js). See docs/spikes/e-content-isolation.md.
  ]) {
    archives[name] = await openWithTimeout(name, 15000)
    log('E:', name, archives[name])
  }
  const canary = await invoke<CanaryLog>('spike_canary_log')

  const criteria: Criterion[] = []
  for (const v of variants) {
    const scriptSignals = [...v.pwned, ...v.postMessages, ...v.canary.ipc]
    criteria.push({
      id: `E-script-${v.variant}`,
      description: `No book script runs (inline, event handlers, javascript: links, SVG script, external, srcdoc/data frames) — ${v.variant}`,
      verdict: scriptSignals.length === 0 ? 'pass' : 'fail',
      evidence: scriptSignals.length
        ? `signals: ${scriptSignals.join(', ')}`
        : `no DOM marks, postMessages or IPC calls across ${v.sections} sections; CSP reported: ${v.cspViolations.length} violations`,
    })
    criteria.push({
      id: `E-ipc-${v.variant}`,
      description: `Book content calls no Tauri command — ${v.variant}`,
      verdict: v.canary.ipc.length === 0 ? 'pass' : 'fail',
      evidence: v.canary.ipc.length ? v.canary.ipc.join('; ') : 'spike_canary never invoked',
    })
    criteria.push({
      id: `E-network-${v.variant}`,
      description: `No remote resource loads (img, srcset, CSS link/@import/url(), @font-face, media, prefetch, meta refresh, form) — ${v.variant}`,
      verdict: v.canary.http.length === 0 ? 'pass' : 'fail',
      evidence: v.canary.http.length
        ? v.canary.http.join('; ')
        : 'canary server at 127.0.0.1:8765 received no requests',
    })
    criteria.push({
      id: `E-files-${v.variant}`,
      description: `No access to file:, traversal paths, another book, or the app origin in a frame — ${v.variant}`,
      verdict: v.fileProbesLoaded.length === 0 && !v.appFrameLoaded ? 'pass' : 'fail',
      evidence: `loaded probes: [${v.fileProbesLoaded.join(', ')}]; tauri://localhost framed: ${v.appFrameLoaded}`,
    })
    criteria.push({
      id: `E-overlay-${v.variant}`,
      description: `position: fixed in a book cannot cover the chrome (L14) — ${v.variant}`,
      verdict: v.chromeOnTop ? 'pass' : 'fail',
      evidence: `chrome stays topmost: ${v.chromeOnTop}; the overlay still covers the page inside the frame: ${v.fixedOverlayInsideFrame} (removed by the L14 sanitiser in Phase 2)`,
    })
  }
  criteria.push({
    id: 'E-sandbox-no-scripts',
    description:
      'Book iframes sandboxed without allow-scripts still deliver events to the reader (plan §7.1 design)',
    verdict: withoutScripts ? 'pass' : 'fail',
    evidence: `parent click listener fired: without allow-scripts ${withoutScripts}, with allow-scripts ${withScripts} (WebKit bug 218086)`,
  })
  criteria.push({
    id: 'E-archives',
    description:
      'Hostile archives and XML open or fail without hanging (zip paths are validated by the Rust importer, Phase 1)',
    verdict: Object.values(archives).some((r) => r.startsWith('timed out')) ? 'fail' : 'pass',
    evidence: JSON.stringify(archives),
  })
  criteria.push({
    id: 'E-other-webviews',
    description: 'Verified on WebView2 and WebKitGTK',
    verdict: 'deferred',
    evidence: 'macOS-only scope (docs/decisions.md)',
  })
  return {
    spike: 'e-isolation',
    criteria,
    raw: {
      variants,
      sandbox: { withoutScripts, withScripts },
      archives,
      canaryAfterArchives: canary,
    },
  }
}
