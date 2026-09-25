/* global linen */
// Phase 7 Done-when: a hostile extension reaches no undeclared host, no Tauri
// command, no book text without the permission, no other extension's storage,
// and no more than 10 MB of storage. Each probe reports what happened.
'use strict'
const attempt = async (fn) => {
  try {
    const r = await fn()
    return `reached: ${JSON.stringify(r).slice(0, 80)}`
  } catch (e) {
    return `refused: ${e.message}`
  }
}
linen.commands.register('probe', async () => {
  const r = {}
  r.declaredHost = await attempt(() => linen.net.fetch('http://127.0.0.1:8765/p7-declared'))
  r.undeclaredHost = await attempt(() => linen.net.fetch('http://localhost:8765/p7-undeclared'))
  r.undeclaredPort = await attempt(() => linen.net.fetch('http://127.0.0.1:8766/p7-port'))
  r.redirectTrick = await attempt(() => linen.net.fetch('http://127.0.0.1:8765@evil.example/p7'))
  r.directFetch = await attempt(() => fetch('http://127.0.0.1:8765/p7-direct'))
  r.bookText = await attempt(() => linen.book.text({ chapter: 0 }))
  r.bookSelection = await attempt(() => linen.book.selection())
  r.annotations = await attempt(() => linen.annotations.list())
  r.library = await attempt(() => linen.library.list())
  r.tauri = typeof self.__TAURI_INTERNALS__ === 'undefined' ? 'refused: absent' : 'reached: present'
  r.unknownCall = await attempt(
    () =>
      new Promise((res, rej) => {
        // A forged call the API does not offer: the host must refuse it.
        const id = 999999
        const prev = self.onmessage
        self.onmessage = (e) => {
          if (e.data && e.data.reply === id) {
            self.onmessage = prev
            if (e.data.error) rej(new Error(e.data.error))
            else res(e.data.result)
          } else prev(e)
        }
        postMessage({ rpc: id, method: 'library.remove', params: { id: 'x' } })
      }),
  )
  r.otherStorage = await attempt(async () => {
    // Storage calls carry no extension id: the host always uses this extension's own.
    await linen.storage.set('mine', 'x')
    const theirs = await linen.storage.get('secret-of-another-extension')
    return theirs === null ? 'nothing (its own storage only)' : theirs
  })
  r.storageQuota = await attempt(async () => {
    const chunk = 'x'.repeat(3 << 20)
    for (let i = 0; i < 4; i++) await linen.storage.set(`big${i}`, chunk)
    return 'stored 12 MB'
  })
  for (let i = 0; i < 4; i++) await linen.storage.delete(`big${i}`).catch(() => {})
  return r
})
