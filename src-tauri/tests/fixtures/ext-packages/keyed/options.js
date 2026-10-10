/* global linenUi */
// LK8: an options page asks Linen, through its frame, whether a key is saved, and
// records what it saw so the harness can check the bridge.
'use strict'
;(async () => {
  const has = await linenUi.call('secrets.has', { name: 'token' })
  document.getElementById('state').textContent = has ? 'A key is saved.' : 'No key yet.'
  await linenUi.call('storage.set', { key: 'options-saw', value: String(has) })
})()
document.getElementById('set').addEventListener('click', async () => {
  if (
    await linenUi.call('permissions.request', { host: '127.0.0.1:8765', purpose: 'To test keys.' })
  )
    await linenUi.call('secrets.request', {
      name: 'token',
      host: '127.0.0.1:8765',
      label: 'Test token',
    })
})
