// Interactive spikes B (input) and C (VoiceOver): a person follows on-screen steps.

import type { View } from 'foliate-js/view.js'
import { WheelTurns, isNotchedWheel, type WheelSample } from '../lib/input/wheel'
import { log, openView, type Criterion, type SpikeResult } from './common'

function panel(): HTMLElement {
  let el = document.getElementById('spike-panel')
  if (!el) {
    el = document.createElement('aside')
    el.id = 'spike-panel'
    Object.assign(el.style, {
      position: 'fixed',
      top: '12px',
      right: '12px',
      width: '360px',
      padding: '16px',
      zIndex: '100',
      background: '#fff',
      color: '#222',
      border: '1px solid #ccc',
      borderRadius: '10px',
      font: '14px/1.45 system-ui',
      boxShadow: '0 6px 24px rgba(0,0,0,.18)',
    })
    document.body.append(el)
  }
  return el
}

/** Show a step and wait for the person to press one of the buttons. */
function step(
  title: string,
  body: string,
  buttons: string[],
  status?: () => string,
): Promise<string> {
  const el = panel()
  el.innerHTML = `<h3 style="margin:0 0 8px;font-size:15px">${title}</h3><p style="margin:0 0 12px">${body}</p><p id="spike-status" style="margin:0 0 12px;font-weight:600"></p>`
  const statusEl = el.querySelector('#spike-status') as HTMLElement
  const timer = status ? setInterval(() => (statusEl.textContent = status()), 100) : 0
  return new Promise((resolve) => {
    for (const label of buttons) {
      const b = document.createElement('button')
      b.textContent = label
      Object.assign(b.style, { marginRight: '8px', padding: '6px 12px', font: 'inherit' })
      b.onclick = () => {
        clearInterval(timer)
        resolve(label)
      }
      el.append(b)
    }
  })
}

/** Wheel events land in the book's iframe; listen in every section document. */
function onWheel(view: View, handler: (e: WheelEvent) => void) {
  const attach = (doc: Document) => doc.addEventListener('wheel', handler, { passive: false })
  view.addEventListener('load', (e) => attach((e as CustomEvent<{ doc: Document }>).detail.doc))
  for (const { doc } of view.renderer.getContents()) attach(doc)
  document.getElementById('reader')!.addEventListener('wheel', handler, { passive: false })
}

export async function spikeB(): Promise<SpikeResult> {
  const { view } = await openView('standardebooks-moby-dick.epub', { width: 1000 })
  await view.goTo(20)
  const detector = new WheelTurns()
  let phase = 'idle'
  const samples: (WheelSample & { phase: string; notched: boolean })[] = []
  const turns: Record<string, number> = {}
  onWheel(view, (e) => {
    e.preventDefault()
    const s: WheelSample = {
      dx: e.deltaX,
      dy: e.deltaY,
      t: e.timeStamp,
      wheelDeltaY: (e as unknown as { wheelDeltaY?: number }).wheelDeltaY,
    }
    samples.push({ ...s, phase, notched: isNotchedWheel(s) })
    const turn = detector.feed(s)
    if (turn && phase !== 'idle') {
      turns[phase] = (turns[phase] ?? 0) + 1
      void (turn === 'next' ? view.next() : view.prev())
    }
  })

  const phases = [
    [
      'vertical',
      'Trackpad, vertical',
      'With two fingers, make <b>25 separate swipes up</b> (as if scrolling down), one at a time, letting each one coast to a stop. Then press Done.',
    ],
    [
      'horizontal',
      'Trackpad, horizontal',
      'Make <b>25 separate swipes to the left</b>, one at a time. Then press Done.',
    ],
    [
      'quick',
      'Quick swipes',
      'Make <b>10 quick vertical swipes</b>, starting each while the previous one still coasts. Then press Done.',
    ],
    [
      'wheel',
      'Mouse wheel (optional)',
      'If a notched mouse is attached, roll <b>20 single notches</b> slowly. Otherwise press Skip.',
    ],
  ] as const
  const expected: Record<string, number> = { vertical: 25, horizontal: 25, quick: 10, wheel: 20 }
  const skipped: string[] = []
  for (const [id, title, body] of phases) {
    phase = id
    const answer = await step(
      title,
      body,
      id === 'wheel' ? ['Done', 'Skip'] : ['Done'],
      () => `Page turns detected: ${turns[id] ?? 0}`,
    )
    if (answer === 'Skip') skipped.push(id)
    phase = 'idle'
  }

  // I11: the click that activates an inactive window.
  let activation = null as { hadFocus: boolean; t: number } | null
  const onDown = () => (activation ??= { hadFocus: document.hasFocus(), t: performance.now() })
  window.addEventListener('pointerdown', onDown, true)
  await step(
    'Activating click (I11)',
    'Switch to another app (⌘Tab), then click once in the <b>right margin of the page</b> to come back. Then press Done.',
    ['Done'],
  )
  window.removeEventListener('pointerdown', onDown, true)

  const criteria: Criterion[] = (['vertical', 'horizontal', 'quick', 'wheel'] as const).map(
    (id) => {
      const got = turns[id] ?? 0
      const want = expected[id]
      const skip = skipped.includes(id)
      return {
        id: `B-${id}`,
        description: `${want} ${id} gestures give exactly ${want} page turns`,
        verdict: skip ? 'manual' : got === want ? 'pass' : 'fail',
        evidence: skip
          ? 'skipped (no notched mouse)'
          : `${got} turns for ${want} gestures (${samples.filter((s) => s.phase === id).length} wheel events recorded)`,
      }
    },
  )
  criteria.push({
    id: 'B-activating-click',
    description: 'The window-activating click is distinguishable (I11)',
    verdict: activation ? (activation.hadFocus ? 'fail' : 'pass') : 'manual',
    evidence: activation
      ? `document.hasFocus() at pointerdown: ${activation.hadFocus}`
      : 'no click recorded',
  })
  criteria.push({
    id: 'B-scripted',
    description: '50 scripted gestures',
    verdict: 'manual',
    evidence:
      'not run: posting CGEvent gestures needs Accessibility permission for the test process',
  })
  log('B: done')
  view.close()
  return { spike: 'b-input', criteria, raw: { turns, expected, skipped, activation, samples } }
}

export async function spikeC(): Promise<SpikeResult> {
  const { view } = await openView('standardebooks-moby-dick.epub', { width: 1000 })
  await view.goTo(20)
  const relocations: { t: number; cfi: string; reason?: string }[] = []
  view.addEventListener('relocate', (e) => {
    const d = (e as CustomEvent<{ cfi: string; reason?: string }>).detail
    relocations.push({ t: Math.round(performance.now()), cfi: d.cfi, reason: d.reason })
  })
  const start = await step(
    'VoiceOver: continuous reading (X3)',
    'Turn on VoiceOver (⌘F5). Move the VoiceOver cursor into the book text, then start reading all (VO + A). Let it read across <b>at least 3 page boundaries</b> (about 3 minutes), then stop it (Control) and press Continue. Press Skip if VoiceOver cannot be used now.',
    ['Continue', 'Skip'],
  )
  if (start === 'Skip') {
    view.close()
    return {
      spike: 'c-accessibility',
      criteria: [
        {
          id: 'C-continuous',
          description: 'VoiceOver reads continuously across 3 page boundaries',
          verdict: 'manual',
          evidence: 'skipped',
        },
      ],
      raw: {},
    }
  }
  const continuous = await step(
    'Did it read continuously?',
    'Did VoiceOver keep reading past the end of each page without stopping or jumping?',
    ['Yes', 'No'],
  )
  const followed = await step(
    'Did the page follow?',
    'While VoiceOver read past a page end, did the visible page turn to show what it was reading?',
    ['Yes', 'No', 'Not sure'],
  )
  view.close()
  return {
    spike: 'c-accessibility',
    criteria: [
      {
        id: 'C-continuous',
        description: 'VoiceOver reads continuously across 3 page boundaries',
        verdict: continuous === 'Yes' ? 'pass' : 'fail',
        evidence: `observer answer: ${continuous}`,
      },
      {
        id: 'C-position-observable',
        description:
          'VoiceOver’s reading position is observable, so the visible page can follow (X3, T2)',
        verdict: relocations.length > 1 ? 'pass' : 'fail',
        evidence: `${relocations.length} relocate events while reading; observer says the page followed: ${followed}`,
      },
      { id: 'C-nvda', description: 'NVDA', verdict: 'deferred', evidence: 'macOS-only scope' },
    ],
    raw: { relocations, continuous, followed },
  }
}
