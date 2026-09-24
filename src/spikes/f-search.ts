// Spike F, search half: index and search Moby-Dick and non-English samples in WKWebView.

import { makeBook } from 'foliate-js/view.js'
import { extractText } from '../lib/search/extract'
import type { ChapterText } from '../lib/search/search'
import { log, readCorpus, stats, type Criterion, type SpikeResult } from './common'

type Reply = Record<string, unknown> & { type: string }

function worker() {
  const w = new Worker(new URL('./search.worker.ts', import.meta.url), { type: 'module' })
  const ask = (msg: unknown) =>
    new Promise<Reply>((resolve, reject) => {
      w.onmessage = (e) => resolve(e.data as Reply)
      w.onerror = (e) => reject(new Error(e.message))
      w.postMessage(msg)
    })
  return { ask, terminate: () => w.terminate() }
}

async function extract(name: string): Promise<{ chapters: ChapterText[]; extractMs: number }> {
  const t0 = performance.now()
  const book = await makeBook(await readCorpus(name))
  const chapters: ChapterText[] = []
  for (const [index, section] of book.sections.entries()) {
    const doc = await section.createDocument()
    chapters.push({ index, text: doc.body ? extractText(doc.body).text : '' })
  }
  return { chapters, extractMs: performance.now() - t0 }
}

export async function spikeF(): Promise<SpikeResult> {
  const criteria: Criterion[] = []
  const raw: Record<string, unknown> = {}

  // 1. Moby-Dick: cold search from the cache (parse + normalise + scan) and warm search.
  const moby = await extract('standardebooks-moby-dick.epub')
  const cache = JSON.stringify(moby.chapters)
  raw.moby = { sections: moby.chapters.length, textChars: cache.length, extractMs: moby.extractMs }
  log('F: Moby-Dick sections', moby.chapters.length, 'extract ms', Math.round(moby.extractMs))

  const queries = [
    'whale',
    'Queequeg',
    'the',
    'harpoon',
    '"Call me Ishmael"',
    'call me ishmael',
    'xyzzy',
  ]
  const cold: number[] = []
  const warm: number[] = []
  const perQuery: Record<string, unknown> = {}
  for (let run = 0; run < 20; run++) {
    const w = worker()
    const t0 = performance.now()
    await w.ask({ type: 'index', cache })
    const q = queries[run % queries.length]
    const first = await w.ask({ type: 'search', query: q, current: 30 })
    cold.push(performance.now() - t0)
    for (const query of queries) {
      const r = await w.ask({ type: 'search', query, current: 30 })
      warm.push(r.ms as number)
      if (run === 0) perQuery[query] = r
    }
    if (run === 0) raw.firstColdSearch = first
    w.terminate()
  }
  raw.coldSearchMs = stats(cold)
  raw.warmSearchMs = stats(warm)
  raw.perQuery = perQuery
  const coldP95 = (raw.coldSearchMs as { p95: number }).p95
  criteria.push({
    id: 'F-search-time',
    description: 'Searching all 135 chapters of Moby-Dick from the cache takes < 300 ms',
    verdict: coldP95 < 300 ? 'pass' : 'fail',
    evidence: `cold (parse cache + normalise + scan, fresh worker) p95 ${coldP95} ms over 20 runs; warm scan p95 ${(raw.warmSearchMs as { p95: number }).p95} ms`,
  })

  const quoted = perQuery['"Call me Ishmael"'] as { results: number }
  const folded = perQuery['call me ishmael'] as { results: number }
  criteria.push({
    id: 'F-quoted-phrase',
    description: 'Quoted phrases match exactly (F2)',
    verdict: quoted.results >= 1 && folded.results >= quoted.results ? 'pass' : 'fail',
    evidence: `"Call me Ishmael" → ${quoted.results} exact; call me ishmael → ${folded.results} folded`,
  })

  // 2. Diacritic folding on a French sample, CJK on a Japanese sample.
  const samples: Record<string, unknown> = {}
  const check = async (name: string, query: string, expectNonAscii: boolean) => {
    const { chapters } = await extract(name)
    const w = worker()
    await w.ask({ type: 'index', cache: JSON.stringify(chapters) })
    const r = await w.ask({ type: 'search', query, current: 0 })
    w.terminate()
    samples[`${name} :: ${query}`] = r
    const hit = (r.samples as string[]).join(' ')
    return (
      (r.results as number) > 0 && (!expectNonAscii || /\[[^\]]*[^\p{ASCII}][^\]]*\]/u.test(hit))
    )
  }
  const diacritics = await check('idpf-sous-le-vent.epub', 'etait', true)
  const cjk = await check('idpf-kusamakura-japanese-vertical-writing.epub', '智に働けば', false)
  const cjkSingle = await check('idpf-kusamakura-japanese-vertical-writing.epub', '山', false)
  raw.samples = samples
  criteria.push({
    id: 'F-diacritics',
    description: 'Diacritic folding (é → e) works on a corpus sample',
    verdict: diacritics ? 'pass' : 'fail',
    evidence:
      'query "etait" in idpf-sous-le-vent.epub matched text containing accented characters (see raw.samples)',
  })
  criteria.push({
    id: 'F-cjk',
    description: 'CJK matching works on a corpus sample, from 1 character',
    verdict: cjk && cjkSingle ? 'pass' : 'fail',
    evidence:
      'queries "智に働けば" and "山" in idpf-kusamakura-japanese-vertical-writing.epub (see raw.samples)',
  })
  return { spike: 'f-search', criteria, raw }
}
