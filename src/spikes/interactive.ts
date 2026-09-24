// Interactive spikes B (input) and C (VoiceOver): a person follows on-screen steps.

import type { View } from 'foliate-js/view.js'
import { NativeTurns, type NativeScroll } from '../lib/input/native'
import { WheelTurns, type WheelSample } from '../lib/input/wheel'
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
  const { listen } = await import('@tauri-apps/api/event')
  const { view } = await openView('standardebooks-moby-dick.epub', { width: 1000 })
  await view.goTo(20)
  const native = new NativeTurns()
  const dom = new WheelTurns()
  let phase = 'idle'
  const nativeSamples: (NativeScroll & { step: string })[] = []
  const domSamples: (WheelSample & { phase: string })[] = []
  const turns: Record<string, { next: number; prev: number }> = {}
  const domTurns: Record<string, number> = {}
  const count = (id: string) => (turns[id] ??= { next: 0, prev: 0 })

  // The DOM wheel event is only prevented; page turns come from the native stream.
  onWheel(view, (e) => {
    e.preventDefault()
    const s: WheelSample = {
      dx: e.deltaX,
      dy: e.deltaY,
      t: e.timeStamp,
      wheelDeltaY: (e as unknown as { wheelDeltaY?: number }).wheelDeltaY,
    }
    domSamples.push({ ...s, phase })
    if (dom.feed(s) && phase !== 'idle') domTurns[phase] = (domTurns[phase] ?? 0) + 1
  })
  const unlistenScroll = await listen<NativeScroll>('native-scroll', ({ payload }) => {
    nativeSamples.push({ ...payload, step: phase })
    const turn = native.feed(payload)
    if (turn && phase !== 'idle') {
      count(phase)[turn]++
      void (turn === 'next' ? view.next() : view.prev())
    }
  })
  let lastFocus = -Infinity
  const onFocus = () => (lastFocus = performance.now())
  window.addEventListener('focus', onFocus)
  const clicks: {
    appActive: boolean
    keyWindow: boolean
    msSinceFocus: number
    hadFocus: boolean
  }[] = []
  let pendingDown: { hadFocus: boolean; t: number } | null = null
  const onDown = () => (pendingDown = { hadFocus: document.hasFocus(), t: performance.now() })
  window.addEventListener('pointerdown', onDown, true)
  const unlistenDown = await listen<{ app_active: boolean; key_window: boolean }>(
    'native-mouse-down',
    ({ payload }) => {
      if (phase !== 'click') return
      setTimeout(() => {
        clicks.push({
          appActive: payload.app_active,
          keyWindow: payload.key_window,
          msSinceFocus: Math.round((pendingDown?.t ?? performance.now()) - lastFocus),
          hadFocus: pendingDown?.hadFocus ?? false,
        })
      }, 50)
    },
  )

  const status = (id: string) => () => {
    const c = turns[id] ?? { next: 0, prev: 0 }
    return `Turns detected: ${c.next} forward, ${c.prev} back`
  }
  const steps = [
    [
      'rollsDown',
      'Wheel: 25 rolls down',
      'Roll the wheel <b>down (towards you) 25 times</b>, each a short separate roll with a pause in between. Then press Done.',
      25,
    ],
    [
      'rollsUp',
      'Wheel: 15 rolls up',
      'Roll the wheel <b>up 15 times</b>, each a short separate roll. Pages should go back. Then press Done.',
      15,
    ],
    [
      'continuous',
      'Wheel: continuous roll',
      'Roll the wheel <b>down continuously for about 2 seconds</b>, once. Then press Done.',
      0,
    ],
    [
      'trackpad',
      'Trackpad (optional)',
      'If a trackpad is available, make <b>25 separate two-finger swipes up</b>. Otherwise press Skip.',
      25,
    ],
  ] as const
  const skipped: string[] = []
  for (const [id, title, body] of steps) {
    phase = id
    const answer = await step(
      title,
      body,
      id === 'trackpad' ? ['Done', 'Skip'] : ['Done'],
      status(id),
    )
    if (answer === 'Skip') skipped.push(id)
    phase = 'idle'
  }
  phase = 'click'
  await step(
    'Activating click (I11)',
    'Switch to another app (⌘Tab) and back <b>by clicking once in the page’s right margin</b>. Do this <b>3 times</b>. Then press Done.',
    ['Done'],
  )
  phase = 'idle'
  unlistenScroll()
  unlistenDown()
  window.removeEventListener('pointerdown', onDown, true)
  window.removeEventListener('focus', onFocus)

  const deviceOf = (id: string) => {
    const xs = nativeSamples.filter((n) => n.step === id)
    return xs.length
      ? xs.every((n) => !n.precise)
        ? 'wheel'
        : xs.every((n) => n.precise)
          ? 'precise'
          : 'mixed'
      : 'none'
  }
  const criteria: Criterion[] = []
  for (const [id, , , want] of steps) {
    const c = turns[id] ?? { next: 0, prev: 0 }
    const events = nativeSamples.filter((n) => n.step === id).length
    const evidence = `${c.next} forward, ${c.prev} back from ${events} native events (device: ${deviceOf(id)}); the WebView-only detector gave ${domTurns[id] ?? 0}`
    if (id === 'continuous') {
      criteria.push({
        id: 'B-wheel-continuous',
        description: 'A 2 s continuous roll turns at most one page per 250 ms',
        verdict: 'manual',
        evidence,
      })
    } else if (id === 'trackpad') {
      criteria.push({
        id: 'B-trackpad',
        description: '25 trackpad swipes give exactly 25 turns, momentum ignored (I2, I5)',
        verdict: skipped.includes(id)
          ? 'manual'
          : c.next === want && c.prev === 0
            ? 'pass'
            : 'fail',
        evidence: skipped.includes(id) ? 'skipped: no trackpad available' : evidence,
      })
    } else {
      const dir = id === 'rollsDown' ? 'next' : 'prev'
      const other = dir === 'next' ? 'prev' : 'next'
      criteria.push({
        id: `B-wheel-${id}`,
        description: `${want} separate wheel rolls give exactly ${want} turns in the right direction (I1)`,
        verdict: c[dir] === want && c[other] === 0 ? 'pass' : 'fail',
        evidence,
      })
    }
  }
  const activating = clicks.filter((c) => !c.appActive)
  criteria.push({
    id: 'B-activating-click',
    description: 'The window-activating click is distinguishable (I11)',
    verdict: clicks.length === 0 ? 'manual' : activating.length === clicks.length ? 'pass' : 'fail',
    evidence: `${clicks.length} clicks recorded; native “app was inactive” on ${activating.length}; ms since window focus: ${clicks.map((c) => c.msSinceFocus).join(', ')}; document.hasFocus(): ${clicks.map((c) => c.hadFocus).join(', ')}`,
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
  return {
    spike: 'b-input',
    criteria,
    raw: { turns, domTurns, skipped, clicks, nativeSamples, domSamples },
  }
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
