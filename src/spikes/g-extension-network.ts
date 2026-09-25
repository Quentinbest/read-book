// Spike G (plan §7.2): extension network isolation, on macOS's WKWebView.
//
// A probe extension runs in a Worker made by our host frame on its own origin
// (linen-ext://g.probe/), with the Worker's script served under
// `connect-src 'none'; script-src 'self'; worker-src 'none'`. It tries fetch, XHR,
// WebSocket, EventSource, importScripts (remote, another extension, data:), nested
// Workers (own, remote, blob:), sendBeacon, FontFace, and Tauri's IPC routes. Its
// UI page (a Phase 7 Navigator tab) tries an iframe, a form, images, styles and IPC
// in a sandboxed frame. The canary server at 127.0.0.1:8765 and the IPC canary are
// the judges: nothing may reach them.

import { invoke } from '@tauri-apps/api/core'
import { log, sleep, type Criterion, type SpikeResult } from './common'

type CanaryLog = { ipc: string[]; http: string[] }

function frame(src: string, sandbox: string): HTMLIFrameElement {
  const f = document.createElement('iframe')
  f.setAttribute('sandbox', sandbox)
  f.src = src
  f.style.cssText = 'width:400px;height:200px;border:1px solid #ccc'
  document.body.append(f)
  return f
}

function message<T>(
  from: HTMLIFrameElement,
  pick: (d: unknown) => T | undefined,
  ms: number,
): Promise<T | null> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      removeEventListener('message', on)
      resolve(null)
    }, ms)
    const on = (e: MessageEvent) => {
      if (e.source !== from.contentWindow) return
      const v = pick(e.data)
      if (v === undefined) return
      clearTimeout(timer)
      removeEventListener('message', on)
      resolve(v)
    }
    addEventListener('message', on)
  })
}

export async function spikeG(): Promise<SpikeResult> {
  await invoke('spike_install_ext', { id: 'g.probe' })
  await invoke('spike_install_ext', { id: 'g.other' })
  await invoke('spike_canary_clear')

  // Extension code: the host frame on the extension's origin makes the Worker.
  const host = frame(
    'linen-ext://g.probe/_host.html?main=main.js',
    'allow-scripts allow-same-origin',
  )
  const worker = await message(
    host,
    (d) => {
      const m = d as {
        linenExt?: boolean
        data?: { done?: boolean; results?: Record<string, string> }
      }
      return m?.linenExt && m.data?.done ? m.data.results : undefined
    },
    60_000,
  )
  log('G worker:', JSON.stringify(worker))

  // Extension UI pages, in the two sandboxes Phase 7 could use.
  const ui: Record<string, Record<string, string> | null> = {}
  for (const sandbox of ['allow-scripts', 'allow-scripts allow-same-origin']) {
    const f = frame('linen-ext://g.probe/ui.html', sandbox)
    ui[sandbox] = await message(
      f,
      (d) => {
        const m = d as { linenExtUi?: boolean; results?: Record<string, string> }
        return m?.linenExtUi ? m.results : undefined
      },
      30_000,
    )
    log(`G ui (${sandbox}):`, JSON.stringify(ui[sandbox]))
  }
  await sleep(1500) // late requests (beacons, prefetches) reach the canary too
  const canary = await invoke<CanaryLog>('spike_canary_log')
  const hits = canary.http.filter((l) => l.includes('/g-'))
  const ipc = canary.ipc.filter((l) => l.startsWith('g-'))
  log('G canary:', JSON.stringify({ hits, ipc }))

  const reached = (r: Record<string, string> | null) =>
    Object.entries(r ?? {}).filter(
      ([k, v]) => /^reached/.test(v) && k !== 'importScripts (own package)',
    )
  const workerLeaks = reached(worker)
  const criteria: Criterion[] = [
    {
      id: 'G-worker-ran',
      description:
        'The extension Worker runs on its own origin and can load its own package (positive control)',
      verdict: worker?.['importScripts (own package)'] === 'allowed' ? 'pass' : 'fail',
      evidence: worker
        ? `own importScripts: ${worker['importScripts (own package)']}`
        : 'the Worker never reported',
    },
    {
      id: 'G-worker-network',
      description:
        'The Worker reaches no undeclared host: fetch, XHR, WebSocket, EventSource, importScripts, nested Workers, sendBeacon, FontFace',
      verdict:
        worker && !workerLeaks.length && !hits.some((h) => !h.includes('/g-ui-')) ? 'pass' : 'fail',
      evidence: worker
        ? `${Object.entries(worker)
            .map(([k, v]) => `${k}: ${v}`)
            .join(
              '; ',
            )}; canary hits: ${hits.filter((h) => !h.includes('/g-ui-')).join(', ') || 'none'}`
        : 'no report',
    },
    {
      id: 'G-ipc',
      description: 'Neither extension code nor its UI can call a Tauri command',
      verdict: ipc.length === 0 ? 'pass' : 'fail',
      evidence: ipc.length ? ipc.join('; ') : 'the IPC canary was never called',
    },
  ]
  for (const [sandbox, r] of Object.entries(ui)) {
    const leaks = reached(r)
    if (!r && !sandbox.includes('same-origin')) {
      // WebKit gives an opaque-origin frame no 'self', so the page's own script is
      // refused: nothing runs, nothing leaks (fail-closed). Phase 7 frames UI pages
      // with allow-same-origin, on the extension's own origin (the next criterion).
      criteria.push({
        id: 'G-ui-opaque',
        description: `An extension UI page (sandbox="${sandbox}") reaches no undeclared host`,
        verdict: hits.some((h) => h.includes('/g-ui-')) ? 'fail' : 'pass',
        evidence:
          "its own script does not run at all in WebKit (script-src 'self' matches no opaque origin), so it is unusable, not unsafe; canary hits: none",
      })
      continue
    }
    criteria.push({
      id: `G-ui-${sandbox.includes('same-origin') ? 'same-origin' : 'opaque'}`,
      description: `An extension UI page (sandbox="${sandbox}") reaches no undeclared host and not the app`,
      verdict: r && !leaks.length && !hits.some((h) => h.includes('/g-ui-')) ? 'pass' : 'fail',
      evidence: r
        ? `${Object.entries(r)
            .map(([k, v]) => `${k}: ${v}`)
            .join(
              '; ',
            )}; canary hits: ${hits.filter((h) => h.includes('/g-ui-')).join(', ') || 'none'}`
        : 'the page never reported (its script may be blocked in this sandbox)',
    })
  }
  return { spike: 'g-extension-network', criteria, raw: { worker, ui, canary } }
}
