// The extension host frame (§7.2): one per extension, on the extension's own
// origin (linen-ext://<id>/), so the extension's Worker is same-origin with it.
// It only relays messages; the extension's code runs in the Worker, whose CSP
// (sent with the script) forbids any network, other scripts and nested workers.
'use strict'
;(() => {
  const main = new URLSearchParams(location.search).get('main') || 'main.js'
  if (!/^[A-Za-z0-9_-][A-Za-z0-9_./-]*\.js$/.test(main) || main.includes('..')) return
  const worker = new Worker(main)
  worker.onmessage = (e) => parent.postMessage({ linenExt: true, data: e.data }, '*')
  worker.onerror = (e) =>
    parent.postMessage({ linenExt: true, error: String(e.message || 'worker error') }, '*')
  addEventListener('message', (e) => {
    if (e.source === parent) worker.postMessage(e.data)
  })
})()
