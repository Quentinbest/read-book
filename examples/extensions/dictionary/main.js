/* global linen */
// A sample extension (Phase 7): “Define” on a selection of up to three words,
// looked up at dictionaryapi.dev through the host (network permission), shown in
// the extension's Navigator tab. The Worker and the tab talk over a channel on
// the extension's own origin.
'use strict'

const channel = new BroadcastChannel('linen')
let last = null

function show(result) {
  last = result
  channel.postMessage({ result })
  return linen.storage.set('last', JSON.stringify(result))
}

channel.onmessage = async (e) => {
  if (e.data && e.data.hello) {
    if (!last) {
      const saved = await linen.storage.get('last')
      last = saved ? JSON.parse(saved) : null
    }
    channel.postMessage({ result: last })
  }
}

linen.commands.register('define', async () => {
  const { text } = await linen.book.selection()
  const word = text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\s'-]/gu, '')
  if (!word) return show({ word: text, error: 'Nothing to look up.' })
  await show({ word, loading: true })
  try {
    const r = await linen.net.fetch(
      `https://api.dictionaryapi.dev/api/v2/entries/en/${encodeURIComponent(word)}`,
    )
    if (r.status === 404) return show({ word, error: 'No definition found.' })
    if (r.status !== 200) return show({ word, error: `The dictionary answered ${r.status}.` })
    const entries = JSON.parse(r.body)
    const meanings = entries.flatMap((e) =>
      e.meanings.map((m) => ({
        part: m.partOfSpeech,
        definitions: m.definitions.slice(0, 3).map((d) => d.definition),
      })),
    )
    return show({ word, phonetic: entries[0].phonetic || '', meanings })
  } catch (err) {
    return show({ word, error: `Couldn’t reach the dictionary (${err.message}).` })
  }
})
