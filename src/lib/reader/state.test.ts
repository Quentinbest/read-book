import { describe, expect, it } from 'vitest'
import {
  initialState,
  navigatorTab,
  readerInputSuspended,
  reduce,
  type ReaderEvent,
  type ReaderState,
} from './state'

const WIDE = 1280
const NARROW = 900

function run(state: ReaderState, ...events: ReaderEvent[]) {
  let s = state
  const effects: string[] = []
  for (const e of events) {
    const t = reduce(s, e)
    s = t.state
    effects.push(...t.effects.map((x) => x.type))
  }
  return { s, effects }
}

describe('S1 three lanes, at most one state each', () => {
  it('starts immersive with nothing open', () => {
    const s = initialState(WIDE)
    expect(s).toMatchObject({ chrome: 'immersive', docked: null, floating: null })
  })

  it('a docked Navigator and a floating layer coexist on a wide window', () => {
    const { s } = run(
      initialState(WIDE),
      { type: 'openNavigator', tab: 'contents' },
      { type: 'openFloating', kind: 'aa' },
    )
    expect(s.docked).toBe('contents')
    expect(s.floating).toEqual({ kind: 'aa' })
  })
})

describe('S2 one floating layer; narrow windows float the Navigator', () => {
  it('opening a floating layer closes the current one', () => {
    const { s } = run(
      initialState(WIDE),
      { type: 'openFloating', kind: 'aa' },
      { type: 'openFloating', kind: 'goto' },
    )
    expect(s.floating).toEqual({ kind: 'goto' })
  })

  it('replacing a note card saves it first', () => {
    const { effects } = run(
      initialState(WIDE),
      { type: 'openFloating', kind: 'note' },
      { type: 'openFloating', kind: 'palette' },
    )
    expect(effects).toContain('saveNote')
  })

  it('below 1100 px the Navigator opens in the floating lane and replaces other layers', () => {
    const { s } = run(
      initialState(NARROW),
      { type: 'openFloating', kind: 'aa' },
      { type: 'openNavigator', tab: 'search' },
    )
    expect(s.docked).toBeNull()
    expect(s.floating).toEqual({ kind: 'navigator', tab: 'search' })
  })

  it('a floating layer opened over a floating Navigator closes the Navigator', () => {
    const { s } = run(
      initialState(NARROW),
      { type: 'openNavigator', tab: 'notes' },
      { type: 'openFloating', kind: 'goto' },
    )
    expect(navigatorTab(s)).toBeNull()
    expect(s.floating).toEqual({ kind: 'goto' })
  })

  it('resizing across 1100 px moves the Navigator between lanes', () => {
    const narrowed = run(
      initialState(WIDE),
      { type: 'openNavigator', tab: 'contents' },
      { type: 'openFloating', kind: 'peek' },
      { type: 'resize', width: NARROW },
    )
    expect(narrowed.s.floating).toEqual({ kind: 'navigator', tab: 'contents' })
    const widened = run(narrowed.s, { type: 'resize', width: WIDE })
    expect(widened.s.docked).toBe('contents')
    expect(widened.s.floating).toBeNull()
  })

  it('a modal keeps its place when the window narrows; the Navigator closes', () => {
    const { s } = run(
      initialState(WIDE),
      { type: 'openNavigator', tab: 'contents' },
      { type: 'openFloating', kind: 'palette' },
      { type: 'resize', width: NARROW },
    )
    expect(s.floating).toEqual({ kind: 'palette' })
    expect(navigatorTab(s)).toBeNull()
  })

  it('starts with a remembered docked Navigator in the right lane (S13)', () => {
    expect(initialState(WIDE, 'notes').docked).toBe('notes')
    expect(initialState(NARROW, 'notes').floating).toEqual({ kind: 'navigator', tab: 'notes' })
  })
})

describe('S3 chrome and Navigator alternate', () => {
  it('opening the Navigator hides the bars; closing restores them (B11 default)', () => {
    const opened = run(
      initialState(WIDE),
      { type: 'showChrome' },
      { type: 'openNavigator', tab: 'contents' },
    )
    expect(opened.s.chrome).toBe('immersive')
    const closed = run(opened.s, { type: 'closeNavigator' })
    expect(closed.s.chrome).toBe('controls')
  })

  it('the chrome cannot be revealed while the Navigator is open', () => {
    const { s } = run(
      initialState(WIDE),
      { type: 'openNavigator', tab: 'contents' },
      { type: 'showChrome' },
    )
    expect(s.chrome).toBe('immersive')
  })

  it('opening the Navigator while open switches its tab', () => {
    const { s } = run(
      initialState(WIDE),
      { type: 'openNavigator', tab: 'contents' },
      { type: 'openNavigator', tab: 'search' },
    )
    expect(s.docked).toBe('search')
  })
})

describe('S4 Esc pops exactly one level and never leaves the book', () => {
  it('floating layer → Navigator → chrome → nothing', () => {
    let s = initialState(WIDE)
    s = run(s, { type: 'showChrome' }).s
    s = run(s, { type: 'openNavigator', tab: 'contents' }).s // hides chrome, remembers it
    s = run(s, { type: 'openFloating', kind: 'aa' }).s
    const steps: ReaderState[] = []
    for (let i = 0; i < 4; i++) {
      s = run(s, { type: 'escape' }).s
      steps.push(s)
    }
    expect(steps[0]).toMatchObject({ floating: null, docked: 'contents' })
    expect(steps[1]).toMatchObject({ docked: null, chrome: 'controls' })
    expect(steps[2]).toMatchObject({ chrome: 'immersive' })
    expect(steps[3]).toEqual(steps[2])
  })

  it('Esc closes a floating Navigator in one step on narrow windows', () => {
    const { s } = run(
      initialState(NARROW),
      { type: 'openNavigator', tab: 'search' },
      { type: 'escape' },
    )
    expect(navigatorTab(s)).toBeNull()
  })
})

describe('S5 closing any layer returns focus to the text', () => {
  const cases: [string, ReaderEvent[]][] = [
    ['Esc on a floating layer', [{ type: 'openFloating', kind: 'peek' }, { type: 'escape' }]],
    ['closeFloating', [{ type: 'openFloating', kind: 'image' }, { type: 'closeFloating' }]],
    ['closeNavigator', [{ type: 'openNavigator', tab: 'notes' }, { type: 'closeNavigator' }]],
    ['Esc on the chrome', [{ type: 'showChrome' }, { type: 'escape' }]],
  ]
  for (const [name, events] of cases) {
    it(name, () => expect(run(initialState(WIDE), ...events).effects).toContain('focusText'))
  }
})

describe('S6 a page turn', () => {
  it('closes the floating layer, saving a note card first, and hides the chrome', () => {
    const { s, effects } = run(
      initialState(WIDE),
      { type: 'showChrome' },
      { type: 'openFloating', kind: 'note' },
      { type: 'pageTurn' },
    )
    expect(s).toMatchObject({ floating: null, chrome: 'immersive' })
    expect(effects).toEqual(['saveNote'])
  })

  it('leaves the Navigator open, docked or floating', () => {
    expect(
      run(initialState(WIDE), { type: 'openNavigator', tab: 'search' }, { type: 'pageTurn' }).s
        .docked,
    ).toBe('search')
    expect(
      navigatorTab(
        run(initialState(NARROW), { type: 'openNavigator', tab: 'search' }, { type: 'pageTurn' }).s,
      ),
    ).toBe('search')
  })
})

describe('S7 starting a selection dismisses popovers and peeks', () => {
  for (const kind of ['aa', 'goto', 'peek', 'image'] as const) {
    it(`dismisses ${kind}`, () => {
      expect(
        run(initialState(WIDE), { type: 'openFloating', kind }, { type: 'selectionStart' }).s
          .floating,
      ).toBeNull()
    })
  }
  it('does not dismiss the palette or a dialog', () => {
    expect(
      run(initialState(WIDE), { type: 'openFloating', kind: 'palette' }, { type: 'selectionStart' })
        .s.floating,
    ).toEqual({ kind: 'palette' })
  })
})

describe('S8 only modals suspend reader input', () => {
  it('palette and dialogs suspend; other layers do not', () => {
    for (const kind of ['palette', 'dialog'] as const) {
      const s = run(initialState(WIDE), { type: 'openFloating', kind }).s
      expect(readerInputSuspended(s)).toBe(true)
      expect(run(s, { type: 'pageTurn' }).s.floating).toEqual({ kind })
    }
    for (const kind of ['aa', 'selection', 'note', 'peek'] as const) {
      expect(readerInputSuspended(run(initialState(WIDE), { type: 'openFloating', kind }).s)).toBe(
        false,
      )
    }
    expect(
      readerInputSuspended(run(initialState(NARROW), { type: 'openNavigator', tab: 'contents' }).s),
    ).toBe(false)
  })
})

describe('S10 chrome hiding', () => {
  it('never hides while a chrome control has focus or a chrome popover is open', () => {
    const focused = run(
      initialState(WIDE),
      { type: 'showChrome' },
      { type: 'chromeFocus', focused: true },
      { type: 'hideChrome' },
    )
    expect(focused.s.chrome).toBe('controls')
    const popover = run(
      initialState(WIDE),
      { type: 'showChrome' },
      { type: 'openFloating', kind: 'aa' },
      { type: 'hideChrome' },
    )
    expect(popover.s.chrome).toBe('controls')
    const free = run(initialState(WIDE), { type: 'showChrome' }, { type: 'hideChrome' })
    expect(free.s.chrome).toBe('immersive')
  })
})
