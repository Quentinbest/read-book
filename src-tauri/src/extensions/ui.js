// Linen's style kit for extension pages (P4): the reader's colours and type, sent
// by Linen when the page loads and whenever the theme changes, and a channel to
// the extension's own Worker (same origin). Pages load it with
// <script src="_ui.js"></script> and <link rel="stylesheet" href="_kit.css">.
'use strict'
;(() => {
  const channel = new BroadcastChannel('linen')
  addEventListener('message', (e) => {
    if (e.source !== parent) return
    const m = e.data || {}
    if (m.theme) {
      for (const [k, v] of Object.entries(m.theme)) document.documentElement.style.setProperty(k, v)
      document.documentElement.style.colorScheme = m.scheme || 'light'
    }
  })
  self.linenUi = Object.freeze({
    /** Messages from the extension's Worker (it posts on BroadcastChannel('linen')). */
    onMessage(handler) {
      channel.addEventListener('message', (e) => handler(e.data))
    },
    /** A message to the extension's Worker. */
    post(data) {
      channel.postMessage(data)
    },
  })
})()
