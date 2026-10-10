/* global linen */
// Reading Lens test provider for LK6–LK8 (docs/reading-lens-plan.md §6.2). Each lookup
// does what the harness's script names and writes what happened to storage
// ('report'), so a check can read it. It never sees a key: it only names one.
//
// script: { step: 'grant' | 'key' | 'send' | 'probe' | 'slow' }
'use strict'

const HOST = '127.0.0.1:8765'
const report = (r) => linen.storage.set('report', JSON.stringify(r))
const attempt = (p) =>
  p.then(
    (r) => ({ ok: true, status: r.status }),
    (e) => ({ ok: false, error: String((e && e.message) || e) }),
  )

linen.lookups.register('keyed', async (request, { signal }) => {
  const script = JSON.parse((await linen.storage.get('script')) || '{}')
  const headword = request.text
  const source = { kind: 'ai', name: 'Keyed', model: 'test-model' }
  switch (script.step) {
    case 'grant':
      await report({ granted: await linen.permissions.request(HOST, { purpose: 'To test keys.' }) })
      break
    case 'key':
      await report({
        saved: await linen.secrets.request('token', { host: HOST, label: 'Test token' }),
        has: await linen.secrets.has('token'),
      })
      break
    case 'send': {
      const auth = { secret: 'token', scheme: 'bearer' }
      await report({
        // The key's own host: sent.
        match: await attempt(linen.net.fetch(`http://${HOST}/canary/keyed`, { auth })),
        // A redirect elsewhere: not followed.
        redirect: await attempt(linen.net.fetch(`http://${HOST}/canary/redirect`, { auth })),
        // Another allowed host: the key is refused there.
        other: await attempt(linen.net.fetch('http://localhost:8765/canary/other', { auth })),
        // Headers Linen keeps for itself.
        authorization: await attempt(
          linen.net.fetch(`http://${HOST}/canary/own`, { headers: { Authorization: 'Bearer x' } }),
        ),
        cookie: await attempt(
          linen.net.fetch(`http://${HOST}/canary/own`, { headers: { Cookie: 'a=b' } }),
        ),
        host: await attempt(
          linen.net.fetch(`http://${HOST}/canary/own`, { headers: { Host: 'example.com' } }),
        ),
        // Nothing hands the key back.
        readable: Object.keys(linen.secrets).sort(),
      })
      break
    }
    case 'slow':
      // LK6: a request still running when the reader removes its host.
      await new Promise((resolve) => {
        const timer = setTimeout(resolve, 4000)
        signal.addEventListener('abort', () => {
          clearTimeout(timer)
          resolve()
        })
      })
      if (signal.aborted) return null
      await linen.net.fetch(`http://${HOST}/canary/slow`)
      break
    case 'probe':
      // The peek's refused-key state, with Open options (LK8).
      if (!(await linen.secrets.has('token')))
        return { status: 'error', headword, error: 'unauthorized', source }
      break
  }
  return { status: 'ok', headword, meaning: `Keyed: ${script.step || 'none'}.`, source }
})
