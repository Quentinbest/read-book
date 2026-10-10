// Phase 0 spike harness entry. Loaded only when the app runs with LINEN_SPIKE set
// (see src-tauri/src/spikes.rs). Runs the requested spikes, writes raw results to
// docs/spikes/raw/, and exits.

import { invoke } from '@tauri-apps/api/core'
import { spikeA } from './a-rendering'
import { log, report, type SpikeResult } from './common'
import { spikeD, spikeDTurns } from './d-fidelity'
import { spikeE } from './e-isolation'
import {
  spikeE2E,
  spikeMemory,
  spikeMemoryDictionaries,
  spikeMemoryTrace,
  spikeSeed500,
  spikeVisual,
  spikeX3,
} from './e2e'
import { spikeF } from './f-search'
import { spikeG } from './g-extension-network'
import { spikeLang } from './i18n'
import { spikeB, spikeBTrackpad, spikeC } from './interactive'

const SPIKES: Record<string, () => Promise<SpikeResult>> = {
  a: spikeA,
  d: spikeD,
  dt: spikeDTurns,
  e: spikeE,
  f: spikeF,
  g: spikeG,
  r: spikeE2E,
  m: () => spikeMemory(),
  md: spikeMemoryDictionaries,
  mt: spikeMemoryTrace,
  v: spikeVisual,
  b: spikeB,
  bt: spikeBTrackpad,
  c: spikeC,
  x3: spikeX3,
  seed500: spikeSeed500,
  lang: spikeLang,
}

async function main() {
  const run = (new URLSearchParams(location.search).get('run') ?? '').split(',').filter(Boolean)
  let failed = 0
  for (const name of run) {
    const spike = SPIKES[name]
    if (!spike) {
      log(`unknown spike: ${name}`)
      failed++
      continue
    }
    try {
      log(`== spike ${name}`)
      const result = await spike()
      for (const c of result.criteria)
        log(`${c.verdict.toUpperCase().padEnd(8)} ${c.id}: ${c.evidence}`)
      await report(result)
    } catch (e) {
      failed++
      log(`spike ${name} crashed: ${e instanceof Error ? e.stack : String(e)}`)
      await invoke('spike_report', {
        name: `${name}-crash`,
        json: JSON.stringify({ error: String(e) }),
      })
    }
  }
  await invoke('spike_exit', { code: failed ? 1 : 0 })
}

main()
