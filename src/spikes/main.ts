// Phase 0 spike harness entry. Loaded only when the app runs with LINEN_SPIKE set
// (see src-tauri/src/spikes.rs). Runs the requested spikes, writes raw results to
// docs/spikes/raw/, and exits.

import { invoke } from '@tauri-apps/api/core'
import { spikeA } from './a-rendering'
import { log, report, type SpikeResult } from './common'
import { spikeD, spikeDTurns } from './d-fidelity'
import { spikeE } from './e-isolation'
import { spikeE2E } from './e2e'
import { spikeF } from './f-search'
import { spikeB, spikeBTrackpad, spikeC } from './interactive'

const SPIKES: Record<string, () => Promise<SpikeResult>> = {
  a: spikeA,
  d: spikeD,
  dt: spikeDTurns,
  e: spikeE,
  f: spikeF,
  r: spikeE2E,
  b: spikeB,
  bt: spikeBTrackpad,
  c: spikeC,
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
