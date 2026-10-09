/* global linen */
// Reading Lens test provider (docs/reading-lens-plan.md §6.2). It imitates Explain
// without a model: the harness writes a script into its storage, and each lookup
// follows it. It echoes what it was sent in `sent`, so checks can compare the context
// (LK5, EP3), and counts the cancellations it heard (LK4).
//
// script: { delayMs, answer: 'ok' | 'needs_context' | 'offline' | 'unauthorized' |
//           'rate_limited' | 'bad' | 'crash' | 'fetch', details: boolean }
'use strict'

const sleep = (ms, signal) =>
  new Promise((resolve) => {
    const timer = setTimeout(resolve, ms)
    signal.addEventListener('abort', () => {
      clearTimeout(timer)
      resolve()
    })
  })

async function bump(key) {
  const n = Number((await linen.storage.get(key)) || 0)
  await linen.storage.set(key, String(n + 1))
}

linen.lookups.register('explain', async (request, { signal }) => {
  const script = JSON.parse((await linen.storage.get('script')) || '{}')
  await bump('asked')
  if (script.delayMs) await sleep(script.delayMs, signal)
  if (signal.aborted) {
    await bump('cancelled')
    return null
  }
  // The selection as book.selection gives it, with its context, while the lookup runs.
  const selection = await linen.book.selection()
  const sent = JSON.stringify({ request, selection }, null, 1)
  const source = { kind: 'ai', name: 'Test', model: 'test-model' }
  const headword = request.text
  switch (script.answer) {
    case 'needs_context':
      return {
        status: 'needs_context',
        headword,
        meaning: 'A guess without the definition.',
        missing: 'This paragraph does not define it.',
        source,
        sent,
      }
    case 'offline':
    case 'unauthorized':
    case 'rate_limited':
      return { status: 'error', headword, error: script.answer, source }
    case 'bad':
      return { status: 'ok', headword, meaning: 'Sure (97% confident).', source }
    case 'crash':
      throw new Error('scripted crash')
    case 'fetch':
      // EP1: a request goes out only when the reader chose this lookup.
      await linen.net.fetch('http://127.0.0.1:8765/canary/lookup')
      break
  }
  return {
    status: 'ok',
    headword,
    term: `${headword} (test)`,
    meaning: `A scripted meaning of “${headword}”.`,
    qualifier: 'Only in this sentence.',
    details: script.details
      ? [
          { label: 'Sentence', text: request.context.sentence },
          { label: 'Why this reading', text: 'The test provider always says so.' },
        ]
      : undefined,
    source,
    sent,
  }
})
