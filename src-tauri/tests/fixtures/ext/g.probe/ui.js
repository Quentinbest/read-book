// Spike G probe: an extension UI page (a Navigator tab in Phase 7) in its frame.
const C = 'http://127.0.0.1:8765/g-ui-'
const results = {}
const timeout = (ms) => new Promise((_, rej) => setTimeout(() => rej(new Error('timeout')), ms))
async function t(name, fn) {
  try {
    results[name] = (await Promise.race([Promise.resolve().then(fn), timeout(2500)])) ?? 'reached'
  } catch (e) {
    results[name] = 'blocked: ' + ((e && (e.name || e.message)) || String(e))
  }
}
;(async () => {
  await t('fetch', () => fetch(C + 'fetch').then(() => 'reached'))
  await t('iframe', () => {
    const f = document.createElement('iframe')
    f.src = C + 'iframe'
    document.body.append(f)
    return new Promise(
      (res) => (f.onload = () => res('loaded (the canary tells whether it was fetched)')),
    )
  })
  await t('form submit', () => {
    document.getElementById('f').submit()
    return 'submitted (the canary tells)'
  })
  await t('Worker', () => (new Worker('main.js'), 'created'))
  await t('Tauri internals', () =>
    window.__TAURI_INTERNALS__
      ? window.__TAURI_INTERNALS__
          .invoke('spike_canary', { id: 'g-ui-internals' })
          .then(() => 'reached')
      : 'absent',
  )
  await t('WebKit message handler', () => {
    const h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ipc
    if (!h) return 'absent'
    h.postMessage('{"cmd":"spike_canary","callback":0,"error":0,"payload":{"id":"g-ui-webkit"}}')
    return 'posted (the canary tells)'
  })
  await t('Tauri IPC (ipc://)', () =>
    fetch('ipc://localhost/spike_canary', { method: 'POST', body: '{"id":"g-ui-ipc"}' }).then(
      () => 'reached',
    ),
  )
  await t('parent DOM', () => (window.parent.document.title, 'reached'))
  parent.postMessage({ linenExtUi: true, results }, '*')
})()
