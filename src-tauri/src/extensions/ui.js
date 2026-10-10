// Linen's style kit for extension pages (P4): the reader's colours and type, sent
// by Linen when the page loads and whenever the theme changes, and a channel to
// the extension's own Worker (same origin). Pages load it with
// <script src="_ui.js"></script> and <link rel="stylesheet" href="_kit.css">.
'use strict'
;(() => {
  const channel = new BroadcastChannel('linen')
  let next = 1
  const pending = new Map()
  addEventListener('message', (e) => {
    if (e.source !== parent) return
    const m = e.data || {}
    if (m.linenReply) {
      const p = pending.get(m.linenReply)
      pending.delete(m.linenReply)
      if (p && m.error) p.reject(new Error(m.error))
      else if (p) p.resolve(m.result)
      return
    }
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
    /**
     * 1.1, experimental (LK8): on an options page, Linen does these for the page:
     * storage.get/set/delete/keys, permissions.request/has, secrets.request/has.
     */
    call(method, params) {
      return new Promise((resolve, reject) => {
        const id = next++
        pending.set(id, { resolve, reject })
        parent.postMessage({ linenCall: id, method, params: params || {} }, '*')
      })
    },
  })
})()
