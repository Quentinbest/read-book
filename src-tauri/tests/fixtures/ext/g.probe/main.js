/* global importScripts */
// Spike G probe: an extension trying to reach the network without declaring it.
// Every attempt below must fail; the canary server (127.0.0.1:8765) is the judge.
const C = 'http://127.0.0.1:8765/g-'
const results = {}
const timeout = (ms) => new Promise((_, rej) => setTimeout(() => rej(new Error('timeout')), ms))
async function t(name, fn) {
  try {
    results[name] = (await Promise.race([Promise.resolve().then(fn), timeout(2500)])) ?? 'reached'
  } catch (e) {
    results[name] = 'blocked: ' + ((e && (e.name || e.message)) || String(e))
  }
}
const opens = (make) =>
  new Promise((res, rej) => {
    const s = make()
    s.onopen = () => res('reached')
    s.onerror = () => rej(new Error('error'))
  })
const runs = (url) =>
  new Promise((res, rej) => {
    const w = new Worker(url)
    w.onmessage = () => res('reached')
    w.onerror = (e) => {
      e.preventDefault?.()
      rej(new Error('error'))
    }
  })

;(async () => {
  await t('fetch', () => fetch(C + 'fetch').then(() => 'reached'))
  await t('fetch no-cors', () =>
    fetch(C + 'fetch-nocors', { mode: 'no-cors' }).then(() => 'reached'),
  )
  await t(
    'XMLHttpRequest',
    () =>
      new Promise((res, rej) => {
        const x = new XMLHttpRequest()
        x.open('GET', C + 'xhr')
        x.onload = () => res('reached')
        x.onerror = () => rej(new Error('error'))
        x.send()
      }),
  )
  await t('WebSocket', () => opens(() => new WebSocket('ws://127.0.0.1:8765/g-ws')))
  await t('EventSource', () => opens(() => new EventSource(C + 'eventsource')))
  await t('importScripts (remote)', () => {
    importScripts(C + 'import.js')
    return 'reached'
  })
  await t('importScripts (another extension)', () => {
    importScripts('linen-ext://g.other/x.js')
    return self.otherLoaded ? 'reached' : 'no effect'
  })
  await t('importScripts (data:)', () => {
    importScripts('data:text/javascript,self.dataRan=1')
    return self.dataRan ? 'reached' : 'no effect'
  })
  await t('nested Worker (own package)', () => runs('nested.js'))
  await t('nested Worker (remote)', () => runs(C + 'worker.js'))
  await t('nested Worker (blob:)', () =>
    runs(URL.createObjectURL(new Blob([`fetch('${C}blob-worker'); postMessage(1)`]))),
  )
  await t('sendBeacon', () => (navigator.sendBeacon?.(C + 'beacon') ? 'queued' : 'refused'))
  await t('FontFace', () => new FontFace('x', `url(${C}font)`).load().then(() => 'reached'))
  await t('Tauri IPC (ipc://)', () =>
    fetch('ipc://localhost/spike_canary', { method: 'POST', body: '{"id":"g-worker-ipc"}' }).then(
      () => 'reached',
    ),
  )
  await t('Tauri IPC (http://ipc.localhost)', () =>
    fetch('http://ipc.localhost/spike_canary', {
      method: 'POST',
      body: '{"id":"g-worker-ipc2"}',
    }).then(() => 'reached'),
  )
  await t('app origin', () => fetch('tauri://localhost/index.html').then(() => 'reached'))
  await t('book media', () => fetch('linen-book://localhost/x/y.png').then(() => 'reached'))
  // Positive control: the extension's own scripts do load.
  await t('importScripts (own package)', () => {
    importScripts('lib.js')
    return self.libLoaded ? 'allowed' : 'failed'
  })
  postMessage({ done: true, results })
})()
