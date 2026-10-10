/* global importScripts */
// The extension's Worker, bootstrapped by Linen (§7.2, P2). It defines `linen`,
// the Host API v1: asynchronous, structured messages to the host, which checks
// each call against the extension's granted permissions. Then it loads the
// extension's own main script (same origin; the CSP allows nothing else).
'use strict'
;(() => {
  const main = new URLSearchParams(location.search).get('main') || 'main.js'
  if (!/^[A-Za-z0-9_-][A-Za-z0-9_./-]*\.js$/.test(main) || main.includes('..')) return
  let next = 1
  const pending = new Map()
  const commands = new Map()
  // API 1.1 (Reading Lens LK1, LK4): lookups, and the requests in flight, which Linen may cancel.
  const lookups = new Map()
  const inFlight = new Map()
  const listeners = new Map()
  const call = (method, params) =>
    new Promise((resolve, reject) => {
      const id = next++
      pending.set(id, { resolve, reject })
      postMessage({ rpc: id, method, params: params === undefined ? null : params })
    })
  self.onmessage = async (e) => {
    const m = e.data || {}
    if (m.reply) {
      const p = pending.get(m.reply)
      pending.delete(m.reply)
      if (p && m.error) p.reject(new Error(m.error))
      else if (p) p.resolve(m.result)
    } else if (m.invoke) {
      try {
        const fn = commands.get(m.command)
        if (!fn) throw new Error(`no handler for command ${m.command}`)
        const result = await fn(m.context || {})
        postMessage({ done: m.invoke, result: result === undefined ? null : result })
      } catch (err) {
        postMessage({ done: m.invoke, error: String((err && err.message) || err) })
      }
    } else if (m.lookup) {
      const controller = new AbortController()
      inFlight.set(m.lookup, controller)
      try {
        const fn = lookups.get(m.id)
        if (!fn) throw new Error(`no handler for lookup ${m.id}`)
        const result = await fn(m.request || {}, { signal: controller.signal })
        if (!controller.signal.aborted)
          postMessage({ done: m.lookup, result: result === undefined ? null : result })
      } catch (err) {
        if (!controller.signal.aborted)
          postMessage({ done: m.lookup, error: String((err && err.message) || err) })
      } finally {
        inFlight.delete(m.lookup)
      }
    } else if (m.cancel) {
      const controller = inFlight.get(m.cancel)
      if (controller) controller.abort()
    } else if (m.event) {
      for (const fn of listeners.get(m.event) || []) {
        try {
          fn(m.payload)
        } catch (err) {
          console.error(err)
        }
      }
    } else if (m.ping) {
      postMessage({ pong: m.ping })
    }
  }
  self.linen = Object.freeze({
    apiVersion: '1.1.0',
    commands: Object.freeze({
      register(id, handler) {
        commands.set(id, handler)
        return call('commands.register', { id })
      },
    }),
    // 1.1, experimental: answer a lookup in the peek with the LK2 fields.
    lookups: Object.freeze({
      register(id, handler) {
        lookups.set(id, handler)
        return call('lookups.register', { id })
      },
    }),
    book: Object.freeze({
      metadata: () => call('book.metadata'),
      selection: () => call('book.selection'),
      chapters: () => call('book.chapters'),
      text: (range) => call('book.text', range || {}),
    }),
    annotations: Object.freeze({
      list: () => call('annotations.list'),
      on(event, handler) {
        if (!listeners.has(event)) listeners.set(event, [])
        listeners.get(event).push(handler)
        return call('annotations.on', { event })
      },
    }),
    // 1.1: reading sessions, as each one ends.
    reading: Object.freeze({
      on(event, handler) {
        if (!listeners.has(event)) listeners.set(event, [])
        listeners.get(event).push(handler)
        return call('reading.on', { event })
      },
    }),
    library: Object.freeze({ list: () => call('library.list') }),
    net: Object.freeze({
      // 1.1 (LK7): headers of the extension's own, and a saved key by name
      // (`auth: { secret, scheme }`), which Linen adds only for the key's host.
      fetch: (url, init) =>
        call('net.fetch', {
          url: String(url),
          method: (init && init.method) || 'GET',
          body: (init && init.body) || null,
          headers: (init && init.headers) || null,
          auth: (init && init.auth) || null,
        }),
    }),
    // 1.1, experimental (LK6): a host from optionalPermissions, asked for in Linen's sheet.
    permissions: Object.freeze({
      request: (host, options) => call('permissions.request', { host, ...(options || {}) }),
      has: (host) => call('permissions.has', { host }),
    }),
    // 1.1, experimental (LK7): keys in the Keychain, typed into Linen's own dialog.
    // The extension can ask for one and ask whether one exists; it never reads one.
    secrets: Object.freeze({
      request: (name, options) => call('secrets.request', { name, ...(options || {}) }),
      has: (name) => call('secrets.has', { name }),
    }),
    files: Object.freeze({ save: (options) => call('files.save', options) }),
    storage: Object.freeze({
      get: (key) => call('storage.get', { key }),
      set: (key, value) => call('storage.set', { key, value }),
      delete: (key) => call('storage.delete', { key }),
      keys: () => call('storage.keys'),
    }),
  })
  importScripts(main)
})()
