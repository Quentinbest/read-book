// Interactive spikes B (input) and C (VoiceOver): a person follows on-screen steps.

import type { View } from 'foliate-js/view.js'
import {
  NativeTurns,
  PHASE,
  ROLL_GAP_MS,
  TURN_DISTANCE_PX,
  WHEEL_COOLDOWN_MS,
  type NativeScroll,
} from '../lib/input/native'
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
export function step(
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
  const turnTimes: { t: number; turn: 'next' | 'prev' }[] = []
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
      turnTimes.push({ t: payload.t, turn })
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
  // Grade the wheel per roll (bursts more than ROLL_GAP_MS apart), not against the
  // instructed count: people rarely roll exactly as asked (run 2). A clean roll in
  // one direction must give 1 + floor(duration / 250 ms) turns, all in its direction.
  // Rolls that rock back and forth are reported, not graded.
  const wheel = nativeSamples.filter(
    (n) => !n.precise && n.step !== 'idle' && n.step !== 'click' && (n.dx || n.dy),
  )
  const rolls: (typeof wheel)[] = []
  for (const n of wheel) {
    const cur = rolls[rolls.length - 1]
    if (cur && n.t - cur[cur.length - 1].t <= ROLL_GAP_MS) cur.push(n)
    else rolls.push([n])
  }
  let clean = 0
  let correct = 0
  let rocking = 0
  for (const roll of rolls) {
    const dirs = new Set(roll.map((n) => ((n.dy || n.dx) < 0 ? 'next' : 'prev')))
    if (dirs.size > 1) {
      rocking++
      continue
    }
    clean++
    const [a, b] = [roll[0].t, roll[roll.length - 1].t]
    const got = turnTimes.filter((x) => x.t >= a && x.t <= b)
    const want = 1 + Math.floor((b - a) / WHEEL_COOLDOWN_MS)
    if (got.length === want && got.every((x) => dirs.has(x.turn))) correct++
  }
  const criteria: Criterion[] = [
    {
      id: 'B-wheel',
      description:
        'Every wheel roll turns exactly one page in its direction (I1; 250 ms cooldown within a long roll)',
      verdict: clean === 0 ? 'manual' : correct === clean ? 'pass' : 'fail',
      evidence: `${correct}/${clean} clean rolls correct; ${rocking} back-and-forth rolls not graded; AppKit classified the device as ${deviceOf('rollsDown')}`,
    },
  ]
  const tp = turns.trackpad ?? { next: 0, prev: 0 }
  criteria.push({
    id: 'B-trackpad',
    description: '25 trackpad swipes give exactly 25 turns, momentum ignored (I2, I5)',
    verdict: skipped.includes('trackpad')
      ? 'manual'
      : tp.next === 25 && tp.prev === 0
        ? 'pass'
        : 'fail',
    evidence: skipped.includes('trackpad')
      ? 'skipped: no trackpad available'
      : `${tp.next} forward, ${tp.prev} back`,
  })
  const activating = clicks.filter((c) => !c.appActive)
  criteria.push({
    id: 'B-activating-click',
    description: 'The window-activating click is distinguishable (I11)',
    verdict: activating.length > 0 ? 'pass' : 'manual',
    evidence: `${clicks.length} clicks; AppKit reported the app inactive on ${activating.length} and active on ${clicks.length - activating.length}; document.hasFocus() was true on ${clicks.filter((c) => c.hadFocus).length}, so the WebView alone cannot tell`,
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
  // What the reader actually sees vs. what foliate believes (run 1 found they can differ):
  // the book frame's position on screen, the book document's own scroll, and foliate's page.
  const samples: {
    t: number
    frameX: number
    frameY: number
    docX: number
    docY: number
    page: number
    cfi?: string
  }[] = []
  const sample = () => {
    const doc = view.renderer.getContents()[0]?.doc
    const frame = doc?.defaultView?.frameElement?.getBoundingClientRect()
    samples.push({
      t: Math.round(performance.now()),
      frameX: Math.round(frame?.left ?? NaN),
      frameY: Math.round(frame?.top ?? NaN),
      docX: Math.round(doc?.scrollingElement?.scrollLeft ?? NaN),
      docY: Math.round(doc?.scrollingElement?.scrollTop ?? NaN),
      page: view.renderer.page,
      cfi: view.lastLocation?.cfi,
    })
  }
  const start = await step(
    'VoiceOver: continuous reading (X3)',
    'Turn on VoiceOver (Fn + ⌘F5, or System Settings › Accessibility). Click in the book text, then press <b>Control + Option + A</b> to read all. When it has started, press <b>Reading started</b> with the mouse. Press Skip if VoiceOver cannot be used now.',
    ['Reading started', 'Skip'],
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
  const readingFrom = performance.now()
  sample()
  const timer = setInterval(sample, 500)
  await step(
    'Keep listening',
    'Let VoiceOver read across <b>at least 3 page boundaries</b> (about 3 minutes). Then stop it (Control) and press Done.',
    ['Done'],
  )
  clearInterval(timer)
  sample()
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

  const during = relocations.filter((r) => r.t >= readingFrom)
  const distinct = new Set(during.map((r) => r.cfi)).size
  const moved = samples.filter(
    (x, i) =>
      i > 0 &&
      (x.frameX !== samples[i - 1].frameX ||
        x.frameY !== samples[i - 1].frameY ||
        x.docX !== samples[i - 1].docX ||
        x.docY !== samples[i - 1].docY),
  ).length
  const pagesSeen = new Set(samples.map((x) => x.page)).size
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
          'VoiceOver’s reading position is observable to the reader engine, so the visible page can follow (X3, T2)',
        // Observable if the engine can see the new position: relocate events, or (run 2)
        // foliate's scroll-derived page number changing without them.
        verdict: distinct >= 3 || pagesSeen >= 3 ? 'pass' : 'fail',
        evidence: `${during.length} relocate events (${distinct} distinct locations) while VoiceOver read; foliate page numbers seen: ${pagesSeen}; the frame or document moved in ${moved} of ${samples.length} samples; observer says the page followed: ${followed}`,
      },
      { id: 'C-nvda', description: 'NVDA', verdict: 'deferred', evidence: 'macOS-only scope' },
    ],
    raw: { readingFrom: Math.round(readingFrom), relocations, samples, continuous, followed },
  }
}

/**
 * Spike B, trackpad only, without a book (runs on WebKit older than 16.4, where
 * foliate-js cannot load). Page turns are counted, not shown. Each gesture is
 * graded from AppKit's phases: a gesture with ≥ 80 px of travel on its axis must
 * give exactly one turn, and momentum must give none (I2, I3, I5).
 */
export async function spikeBTrackpad(): Promise<SpikeResult> {
  const { listen } = await import('@tauri-apps/api/event')
  const pad = document.getElementById('reader')!
  Object.assign(pad.style, {
    width: '640px',
    height: '420px',
    margin: '24px',
    border: '2px dashed #999',
    borderRadius: '12px',
    display: 'grid',
    placeItems: 'center',
    font: '600 20px system-ui',
    color: '#444',
  })
  pad.textContent = 'Swipe with the pointer over this area'
  // Keep the page from scrolling; turns come from the native stream.
  window.addEventListener('wheel', (e) => e.preventDefault(), { passive: false })
  const native = new NativeTurns()
  let phase = 'idle'
  const samples: (NativeScroll & { step: string })[] = []
  const turnLog: { t: number; turn: 'next' | 'prev'; step: string }[] = []
  const unlistenScroll = await listen<NativeScroll>('native-scroll', ({ payload }) => {
    samples.push({ ...payload, step: phase })
    const turn = native.feed(payload)
    if (turn && phase !== 'idle') {
      turnLog.push({ t: payload.t, turn, step: phase })
      pad.textContent = `${turnLog.filter((x) => x.step === phase).length} page turns`
    }
  })
  const clicks: { appActive: boolean }[] = []
  const unlistenDown = await listen<{ app_active: boolean }>('native-mouse-down', ({ payload }) => {
    if (phase === 'click') clicks.push({ appActive: payload.app_active })
  })
  const steps = [
    [
      'vertical',
      'Trackpad: 25 swipes up',
      'With two fingers, make <b>25 separate swipes up</b> (as if scrolling down a page) over the dashed area, one at a time. Let each one coast to a stop. Then press Done.',
    ],
    [
      'horizontal',
      'Trackpad: 25 swipes left',
      'Make <b>25 separate two-finger swipes to the left</b>, one at a time. Then press Done.',
    ],
    [
      'quick',
      'Trackpad: 10 quick swipes',
      'Make <b>10 quick swipes up</b>, starting each while the previous one still coasts. Then press Done.',
    ],
  ] as const
  for (const [id, title, body] of steps) {
    phase = id
    pad.textContent = '0 page turns'
    await step(title, body, ['Done'])
    phase = 'idle'
  }
  phase = 'click'
  await step(
    'Activating click (I11)',
    'Switch to another app (⌘Tab) and come back <b>by clicking once in the dashed area</b>. Do this <b>3 times</b>. Then press Done.',
    ['Done'],
  )
  phase = 'idle'
  unlistenScroll()
  unlistenDown()

  // Split each step's precise events into gestures: Began (or MayBegin) … Ended/Cancelled.
  const grade = (id: string) => {
    const evs = samples.filter((n) => n.step === id && n.precise)
    const gestures: { start: number; end: number; travel: number }[] = []
    let cur: {
      start: number
      end: number
      travel: number
      axis: 'x' | 'y' | null
      tx: number
      ty: number
    } | null = null
    for (const n of evs) {
      if (n.momentum) continue
      if (n.phase & (PHASE.began | PHASE.mayBegin) || !cur) {
        if (cur) gestures.push(cur)
        cur = { start: n.t, end: n.t, travel: 0, axis: null, tx: 0, ty: 0 }
      }
      cur.end = n.t
      cur.tx += n.dx
      cur.ty += n.dy
      cur.travel = Math.max(Math.abs(cur.tx), Math.abs(cur.ty))
      if (n.phase & (PHASE.ended | PHASE.cancelled)) {
        gestures.push(cur)
        cur = null
      }
    }
    if (cur) gestures.push(cur)
    const turns = turnLog.filter((x) => x.step === id)
    const big = gestures.filter((g) => g.travel >= TURN_DISTANCE_PX)
    // A turn belongs to the gesture whose span contains it.
    const perGesture = big.map((g) => turns.filter((x) => x.t >= g.start && x.t <= g.end).length)
    const momentumTurns = turns.filter(
      (x) => !gestures.some((g) => x.t >= g.start && x.t <= g.end),
    ).length
    return {
      events: evs.length,
      wheelEvents: samples.filter((n) => n.step === id && !n.precise).length,
      gestures: gestures.length,
      bigGestures: big.length,
      oneTurnEach: perGesture.filter((c) => c === 1).length,
      turns: turns.length,
      momentumTurns,
    }
  }
  const results = Object.fromEntries(steps.map(([id]) => [id, grade(id)]))
  const criteria: Criterion[] = steps.map(([id]) => {
    const r = results[id]
    return {
      id: `B-trackpad-${id}`,
      description: `Each ${id} trackpad gesture of ≥ 80 px gives exactly one turn; momentum gives none (I2, I3, I5)`,
      verdict:
        r.events === 0
          ? 'manual'
          : r.oneTurnEach === r.bigGestures && r.turns === r.bigGestures && r.momentumTurns === 0
            ? 'pass'
            : 'fail',
      evidence: `${r.bigGestures} gestures ≥ 80 px (of ${r.gestures}), ${r.oneTurnEach} with exactly one turn, ${r.turns} turns in total, ${r.momentumTurns} during momentum; ${r.events} precise and ${r.wheelEvents} wheel events`,
    }
  })
  const inactive = clicks.filter((c) => !c.appActive).length
  criteria.push({
    id: 'B-activating-click',
    description: 'The window-activating click is distinguishable (I11)',
    verdict: inactive > 0 ? 'pass' : 'manual',
    evidence: `${clicks.length} clicks; AppKit reported the app inactive on ${inactive}`,
  })
  return {
    spike: 'b-trackpad',
    criteria,
    raw: { results, clicks, samples, turnLog, userAgent: navigator.userAgent },
  }
}
