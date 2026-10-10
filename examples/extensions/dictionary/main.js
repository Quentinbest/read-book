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

// API 1.1 (Reading Lens): the same look-up as an answer in the lookup peek, in the
// fixed fields Linen renders (LK2). Nothing is sent until the reader chooses it.
linen.lookups.register('define', async (request, { signal }) => {
  const word = request.text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\s'-]/gu, '')
  const source = { kind: 'dictionary', name: 'Free Dictionary' }
  if (!word) return { status: 'error', headword: request.text, error: 'unavailable', source }
  let r
  try {
    r = await linen.net.fetch(
      `https://api.dictionaryapi.dev/api/v2/entries/en/${encodeURIComponent(word)}`,
    )
  } catch {
    return { status: 'error', headword: word, error: 'offline', source }
  }
  if (signal.aborted) return null
  if (r.status === 429) return { status: 'error', headword: word, error: 'rate_limited', source }
  if (r.status !== 200) return { status: 'error', headword: word, error: 'unavailable', source }
  const entries = JSON.parse(r.body)
  const senses = entries.flatMap((e) =>
    e.meanings.flatMap((m) =>
      m.definitions.slice(0, 2).map((d) => `${m.partOfSpeech}: ${d.definition}`),
    ),
  )
  return {
    status: 'ok',
    headword: word,
    meaning: senses[0] || '',
    details: senses
      .slice(1, 6)
      .map((text, i) => ({ label: `Sense ${i + 2}`, text: text.slice(0, 2000) })),
    source,
  }
})
