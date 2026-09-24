<script lang="ts">
  // The reader (plan Phase 2; Screens 02, 03, 14). Features that arrive in later
  // phases (Contents, Search, Notes, Aa, the ⋯ menu, Go to) are hidden until then.
  import { listen } from '@tauri-apps/api/event'
  import { onMount } from 'svelte'
  import Icon from '../components/Icon.svelte'
  import { ipc, type Book } from '../app/ipc'
  import { testHooks } from '../app/testHooks'
  import type { WriteQueue } from '../app/writes'
  import { NativeTurns, type NativeScroll } from '../lib/input/native'
  import type { KeyContext } from '../lib/commands/keys'
  import type { CommandRegistry } from '../lib/commands/registry'
  import { LocationHistory } from '../lib/reader/history'
  import type { MessageQueue } from '../lib/reader/messages'
  import { initialState, reduce, type ReaderEvent, type ReaderState } from '../lib/reader/state'
  import { t } from '../lib/strings/en'
  import { THEMES, type Theme } from '../lib/theme/tokens'
  import { ReaderEngine, type ReaderLocation, type Turn } from './engine'
  import { fallBackFailedFonts, literataFaces } from './fonts'
  import { libraryLoader } from './loader'
  import { computeLayout, showLocationLine, type Layout } from './layout'
  import { ReadingPace } from './pace'
  import { PageCounter } from './pages'
  import { readerStyles } from './styles'

  let {
    book,
    messages,
    writes,
    registry,
    screenReader,
    keyContext,
    onBeforeQuit,
    announce,
    onexit,
  }: {
    book: Book
    messages: MessageQueue
    writes: WriteQueue
    registry: CommandRegistry
    screenReader: () => boolean
    keyContext: () => KeyContext
    onBeforeQuit: (fn: () => void) => () => void
    /** Screen-reader announcement (polite), for page turns (X3). */
    announce: (text: string) => void
    onexit: () => void
  } = $props()

  // ---- constants from the design (S3, S9–S12, N2, I7)
  const REVEAL_ZONE = 64
  const REVEAL_DWELL_MS = 150
  const HIDE_AFTER_MS = 3000
  const SAVE_DEBOUNCE_MS = 1000
  const RAPID_TURNS = 3
  const RAPID_WINDOW_MS = 1000

  let area: HTMLElement
  let host: HTMLElement
  let engine: ReaderEngine | null = null
  let lanes: ReaderState = $state(initialState(window.innerWidth))
  let layout: Layout = $state(
    computeLayout({
      width: window.innerWidth,
      height: window.innerHeight,
      fontPx: 19,
      spacing: 'default',
    }),
  )
  let location = $state<ReaderLocation | null>(null)
  /** I15, G8: a right-to-left book fills progress from the right. */
  let rtlBook = $state(false)
  /** G8: an open taking over 500 ms shows one “Opening …” line, never a spinner. */
  let openingShown = $state(false)
  const OPENING_LINE_AFTER_MS = 500
  let theme: Theme = $state(THEMES.paper)
  let chromeVisible = $derived(lanes.chrome === 'controls')
  // Screens 02/03: the window buttons live in the top bar and hide with it.
  $effect(() => {
    void ipc.setWindowControls(chromeVisible).catch(() => {})
  })
  let height = $state(window.innerHeight)
  let fontPx = 19
  let edges = { topStart: 0, bottomOff: false }
  let announceTurns = true
  let fontFaces = ''
  const history = new LocationHistory()
  let pace = new ReadingPace()
  let pages: PageCounter | null = null
  let pageShownAt = performance.now()
  const recentTurns: number[] = []
  let rapidStart: ReaderLocation | null = null

  /** V8: the chrome leaves over --motion-chrome-out (220 ms; shorter with reduced motion). */
  function chromeOut(_node: Element, { from }: { from: number }) {
    // WebKit reports the token normalised to seconds (“0.22s”), so read the unit.
    const token = getComputedStyle(document.documentElement)
      .getPropertyValue('--motion-chrome-out')
      .trim()
    const ms = parseFloat(token) * (token.endsWith('ms') ? 1 : token.endsWith('s') ? 1000 : 1)
    return {
      duration: ms || 220,
      css: (k: number) => `opacity: ${k}; transform: translateY(${(1 - k) * from}px)`,
    }
  }

  function dispatch(e: ReaderEvent) {
    const { state, effects } = reduce(lanes, e)
    lanes = state
    for (const fx of effects) if (fx.type === 'focusText') engine?.focusPage()
  }

  // ---- theme (V1, V2): Auto follows the system between Paper and Night
  async function resolveTheme(): Promise<Theme> {
    const choice = (await ipc.settingGet('theme')) ?? 'auto'
    if (choice === 'sepia' || choice === 'night' || choice === 'paper') return THEMES[choice]
    return matchMedia('(prefers-color-scheme: dark)').matches ? THEMES.night : THEMES.paper
  }

  function relayout() {
    height = window.innerHeight
    layout = computeLayout({
      width: window.innerWidth,
      height: window.innerHeight,
      fontPx,
      spacing: 'default',
      // L8, G8: the two-page spread is a Pages-mode layout.
      allowSpread: true,
    })
    engine?.applyLayout(
      layout,
      fontFaces +
        readerStyles({
          fontPx,
          lineHeight: layout.lineHeight,
          theme,
          hyphenate: true,
          pageHeight: layout.pageHeight,
        }),
    )
    pages?.reset()
  }

  // ---- progress (N5, 1 s debounce, blur, quit)
  let saveTimer = 0
  function saveNow() {
    clearTimeout(saveTimer)
    const l = location
    if (!l?.cfi) return
    void writes.write(`position:${book.id}`, () => ipc.positionSave(book.id, l.cfi, l.fraction))
  }
  function saveSoon() {
    clearTimeout(saveTimer)
    saveTimer = window.setTimeout(saveNow, SAVE_DEBOUNCE_MS)
  }

  function onRelocate(l: ReaderLocation) {
    const prev = location
    location = l
    // A chunked chapter's page count covers one chunk (L16): it is not the section's.
    if (l.pages && !l.approximate) pages?.count(l.sectionIndex, l.pages)
    // X3: brief page-turn announcements, off when VoiceOver itself moved the page.
    if (announceTurns && l.reason === 'page' && prev && prev.cfi !== l.cfi) {
      const n = pageNumber(l)
      if (n)
        announce(
          n.approximate ? t.reader.pageAnnouncementApprox(n.n) : t.reader.pageAnnouncement(n.n),
        )
    }
    // B2: a page's dwell time feeds the reading pace.
    const now = performance.now()
    if (prev && prev.cfi !== l.cfi) {
      pace.record(
        now - pageShownAt,
        (l.sectionCharsLeft - prev.sectionCharsLeft) * -1 || 0,
        Date.now(),
      )
      pageShownAt = now
    }
    saveSoon()
  }

  // ---- page turns (I1–I10, S6, I7)
  function turn(dir: Turn) {
    if (!engine || lanes.floating?.kind === 'palette' || lanes.floating?.kind === 'dialog') return
    dispatch({ type: 'pageTurn' })
    const now = performance.now()
    // I7: more than 3 pages in under 1 s offers a way back.
    if (!recentTurns.length || now - recentTurns[recentTurns.length - 1] > RAPID_WINDOW_MS)
      rapidStart = location
    recentTurns.push(now)
    while (recentTurns.length && now - recentTurns[0] > RAPID_WINDOW_MS) recentTurns.shift()
    void engine.turn(dir).then(() => {
      if (recentTurns.length > RAPID_TURNS && rapidStart) {
        pushJump(rapidStart, 'pages')
        rapidStart = null
        recentTurns.length = 0
      }
    })
  }

  /** I8, I15: arrow keys stay spatial, so in a right-to-left book ← is next. */
  function pageKey(e: KeyboardEvent): Turn | null {
    if (e.metaKey || e.ctrlKey || e.altKey) return null
    const rtl = engine?.rtl ?? false
    switch (e.key) {
      case 'ArrowRight':
        return rtl ? 'prev' : 'next'
      case 'ArrowLeft':
        return rtl ? 'next' : 'prev'
      case 'ArrowDown':
      case 'PageDown':
        return 'next'
      case 'ArrowUp':
      case 'PageUp':
        return 'prev'
      case ' ':
        return e.shiftKey ? 'prev' : 'next'
    }
    return null
  }

  function onPageKey(e: KeyboardEvent, fromBook: boolean) {
    // Keys inside the book document never reach the app's command handler: route them here,
    // with the page counting as focused (single-key shortcuts, §2.8).
    if (fromBook) {
      const command = registry.commandForKey(e, {
        ...keyContext(),
        pageFocused: true,
        textFieldActive: false,
      })
      if (command && registry.run(command.id)) {
        e.preventDefault()
        return
      }
    }
    // I10: Space on a focused button presses the button.
    const target = e.target
    if (
      !fromBook &&
      target instanceof Element &&
      target !== document.body &&
      target.closest('button, input, textarea, select, [contenteditable]')
    )
      return
    if (e.key === 'Tab' && !fromBook && !chromeVisible) {
      // S9: Tab reveals the chrome and focuses its first control.
      dispatch({ type: 'showChrome' })
      e.preventDefault()
      requestAnimationFrame(() => area.querySelector<HTMLElement>('.chrome button')?.focus())
      return
    }
    const dir = pageKey(e)
    if (dir) {
      e.preventDefault()
      turn(dir)
    }
  }

  // ---- history (N1, N2)
  /** Back chips from this session: withdrawn when the book closes (their action needs this reader). */
  const jumpMessages: number[] = []
  function pushJump(
    from: ReaderLocation,
    source: Parameters<LocationHistory['push']>[0]['source'],
  ) {
    history.push({ cfi: from.cfi, source })
    const page = pageNumber(from)
    const message = messages.push({
      text: page
        ? t.reader.backToPage(page.approximate ? t.reader.approxPage(page.n) : String(page.n))
        : t.reader.back,
      action: { label: t.reader.back, shortcut: '⌘[', run: () => goBack() },
    })
    jumpMessages.push(message.id)
  }

  function goBack() {
    const entry = history.back()
    if (entry) void engine?.goTo(entry.cfi)
  }

  /** Book-wide page number (B1); estimated inside a chunked chapter (L16). */
  function pageNumber(l: ReaderLocation | null): { n: number; approximate: boolean } | null {
    if (!l || !pages || !l.page) return null
    if (l.approximate && l.sectionFraction !== undefined) {
      const inSection = pages.pagesIn(l.sectionIndex)
      const page = Math.min(inSection, Math.floor(l.sectionFraction * inSection) + 1)
      return { n: pages.pageNumber(l.sectionIndex, page), approximate: true }
    }
    return { n: pages.pageNumber(l.sectionIndex, l.page), approximate: false }
  }

  // ---- chrome reveal (S9–S11)
  let dwellTimer = 0
  let hideTimer = 0
  /** S11: in full screen the top zone starts below the menu bar; the bottom zone yields to an auto-hiding Dock. */
  async function refreshEdges() {
    try {
      const e = await ipc.screenEdges()
      edges = {
        topStart: e.fullscreen ? e.menu_bar_height : 0,
        bottomOff: e.dock_edge === 'bottom' && (e.dock_autohide || e.fullscreen),
      }
    } catch {
      // keep the defaults
    }
  }

  function onPointerMove(e: PointerEvent) {
    const nearTop = e.clientY >= edges.topStart && e.clientY <= edges.topStart + REVEAL_ZONE
    const nearBottom = !edges.bottomOff && e.clientY >= window.innerHeight - REVEAL_ZONE
    clearTimeout(hideTimer)
    if (nearTop || nearBottom) {
      if (!chromeVisible && !dwellTimer)
        dwellTimer = window.setTimeout(() => {
          dwellTimer = 0
          dispatch({ type: 'showChrome' })
        }, REVEAL_DWELL_MS)
    } else {
      clearTimeout(dwellTimer)
      dwellTimer = 0
      if (chromeVisible)
        hideTimer = window.setTimeout(() => dispatch({ type: 'hideChrome' }), HIDE_AFTER_MS)
    }
  }

  // ---- margins (I11): click targets for previous / next; the activating click never turns
  let windowJustActivated = false
  /** I11, I15: the left margin goes back, or forward in a right-to-left book. */
  function onMarginClick(side: 'left' | 'right') {
    if (windowJustActivated) return
    const rtl = engine?.rtl ?? false
    turn((side === 'right') !== rtl ? 'next' : 'prev')
  }

  // ---- location line (L9, B2)
  let locationText = $derived.by(() => {
    if (!location) return ''
    const minutes = pace.minutesLeft(location.sectionCharsLeft)
    const chapter = location.chapterLabel
    return [chapter, minutes ? t.reader.minutesLeft(minutes) : null].filter(Boolean).join(' · ')
  })
  let progressText = $derived(location ? `${Math.round(location.fraction * 100)}%` : '')

  onMount(() => {
    const cleanups: (() => void)[] = []
    void (async () => {
      theme = await resolveTheme()
      fontPx = Number((await ipc.settingGet('fontPx')) ?? 19) || 19
      announceTurns = (await ipc.settingGet('pageTurnAnnouncements')) !== 'off'
      await refreshEdges()
      const savedPace = await ipc.settingGet('readingPace')
      if (savedPace) pace = new ReadingPace(JSON.parse(savedPace))
      fontFaces = await literataFaces()
      engine = new ReaderEngine(host)
      if (testHooks) {
        const e = engine
        testHooks.reader = { engine: e, location: () => location, history, bookId: book.id }
      }
      relayout()
      cleanups.push(engine.onRelocate(onRelocate))
      cleanups.push(engine.onKey((e) => onPageKey(e, true)))
      // L15: a book font that fails or takes over 1.5 s falls back to Literata, without a prompt.
      cleanups.push(engine.onDocument((doc) => void fallBackFailedFonts(doc)))
      cleanups.push(
        engine.onLink((link) => {
          if (link.external) {
            // N10: only http(s), in the system browser. Opening waits for the opener plugin.
            if (/^https?:\/\//i.test(link.href)) console.info('external link', link.href)
            return
          }
          if (location?.cfi) pushJump(location, 'link')
        }),
      )
      // N5: the last session's save (made as it closed) may still be in flight.
      await writes.idle()
      const saved = await ipc.positionGet(book.id)
      // L17: entries are read from the zip on demand; the book never crosses whole.
      const openingTimer = window.setTimeout(
        () => (openingShown = !location),
        OPENING_LINE_AFTER_MS,
      )
      cleanups.push(() => clearTimeout(openingTimer))
      if (testHooks?.openDelayMs) await new Promise((r) => setTimeout(r, testHooks!.openDelayMs))
      const opened = await engine.open(
        await libraryLoader(book.id),
        saved ? { cfi: saved[0] } : undefined,
      )
      clearTimeout(openingTimer)
      openingShown = false
      rtlBook = engine.rtl
      pages = new PageCounter(opened.sections.map((s) => (s.linear === 'no' ? 0 : s.size)))
      relayout()
      // N4: resuming deep in a book says where, with a way back to the beginning.
      if (saved && saved[1] > 0.02) {
        const label = engine.location?.chapterLabel
        messages.push({
          text: label ? t.reader.resumedIn(label) : t.reader.resumed,
          action: { label: t.reader.goToBeginning, run: () => void engine?.goToTextStart() },
        })
      }
      engine.focusPage()

      // Wheel and trackpad come from AppKit (Spike B): one turn per roll or gesture.
      const turns = new NativeTurns()
      cleanups.push(
        await listen<NativeScroll>('native-scroll', ({ payload }) => {
          const panel = document
            .elementFromPoint(payload.x, payload.y)
            ?.closest('.chrome, dialog, .popover')
          if (panel) return
          const dir = turns.feed(payload)
          if (dir) turn(dir)
        }),
      )
      // I11: AppKit tells whether a click is the one that activated the window.
      cleanups.push(
        await listen<{ app_active: boolean }>('native-mouse-down', ({ payload }) => {
          windowJustActivated = !payload.app_active
          if (windowJustActivated) setTimeout(() => (windowJustActivated = false), 300)
        }),
      )
      // X3: follow VoiceOver's scrolling while it runs.
      const srTimer = window.setInterval(() => engine?.watchExternalScroll(screenReader()), 2000)
      engine.watchExternalScroll(screenReader())
      cleanups.push(() => clearInterval(srTimer))

      cleanups.push(
        registry.handle('history.back', { run: goBack, enabled: () => history.canGoBack }),
      )
      cleanups.push(registry.handle('chapter.next', { run: () => void engine?.nextSection() }))
      cleanups.push(
        registry.handle('chapter.previous', {
          run: () => void engine?.prevSection(),
        }),
      )
      cleanups.push(registry.handle('layer.close', { run: () => dispatch({ type: 'escape' }) }))
    })()

    const onResize = () => {
      dispatch({ type: 'resize', width: window.innerWidth })
      relayout()
      void refreshEdges() // entering or leaving full screen resizes the window
    }
    let resizeTimer = 0
    const debouncedResize = () => {
      clearTimeout(resizeTimer)
      resizeTimer = window.setTimeout(onResize, 120) // L10
    }
    const onBlur = () => saveNow()
    const onFocusEdges = () => void refreshEdges()
    window.addEventListener('focus', onFocusEdges)
    const offQuit = onBeforeQuit(() => {
      saveNow()
      void ipc.settingSet('readingPace', JSON.stringify(pace.toJSON()))
    })
    const onKeydown = (e: KeyboardEvent) => onPageKey(e, false)
    window.addEventListener('resize', debouncedResize)
    window.addEventListener('blur', onBlur)
    window.addEventListener('keydown', onKeydown)
    return () => {
      window.removeEventListener('resize', debouncedResize)
      window.removeEventListener('blur', onBlur)
      window.removeEventListener('keydown', onKeydown)
      window.removeEventListener('focus', onFocusEdges)
      offQuit()
      cleanups.forEach((c) => c())
      saveNow()
      void ipc.settingSet('readingPace', JSON.stringify(pace.toJSON()))
      engine?.close()
      for (const id of jumpMessages) messages.withdraw(id)
      if (testHooks) testHooks.reader = undefined
      void ipc.setWindowControls(true).catch(() => {})
    }
  })

  function leave() {
    saveNow() // N5
    onexit()
  }
</script>

<div
  class="reader"
  bind:this={area}
  role="presentation"
  onpointermove={onPointerMove}
  style:--ground={theme.ground}
  style:--ink={theme.ink}
  style:--ink-secondary={theme.inkSecondary}
  style:--accent={theme.accent}
  style:--chrome-hairline={theme.chromeHairline}
>
  <div class="host" bind:this={host}></div>

  <!-- Margins: click targets for the previous and next page (I11, S3). -->
  <button
    class="margin left"
    class:shown={chromeVisible}
    style:width="{layout.marginWidth}px"
    aria-label={t.reader.previousPage}
    tabindex="-1"
    onclick={() => onMarginClick('left')}
  >
    <svg
      class="chevron"
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"><path d="M15 5l-7 7 7 7" /></svg
    >
  </button>
  <button
    class="margin right"
    class:shown={chromeVisible}
    style:width="{layout.marginWidth}px"
    aria-label={t.reader.nextPage}
    tabindex="-1"
    onclick={() => onMarginClick('right')}
  >
    <svg
      class="chevron"
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"><path d="M9 5l7 7-7 7" /></svg
    >
  </button>

  {#if chromeVisible}
    <header class="chrome top" data-tauri-drag-region out:chromeOut={{ from: -4 }}>
      <button type="button" class="library" onclick={leave}>
        <Icon name="library" size={18} />{t.reader.library}
      </button>
      <div class="title" aria-live="off">
        <span class="book">{book.title}</span>{#if location?.chapterLabel}&nbsp;· {location.chapterLabel}{/if}
      </div>
    </header>
    <footer class="chrome bottom" out:chromeOut={{ from: 4 }}>
      <div class="progress" class:rtl={rtlBook} style:width="{layout.textWidth}px">
        <div class="track" aria-hidden="true">
          <div class="fill" style:width="{(location?.fraction ?? 0) * 100}%"></div>
        </div>
        <div class="labels">
          <span>{location?.chapterLabel ?? ''}</span>
          <span>{[progressText, locationText.split(' · ')[1]].filter(Boolean).join(' · ')}</span>
        </div>
      </div>
    </footer>
  {:else if openingShown && !location}
    <div class="location-line">{t.reader.opening(book.title)}</div>
  {:else if showLocationLine(height) && locationText}
    <div class="location-line" aria-hidden="true">{locationText}</div>
  {/if}
</div>

<style>
  .reader {
    position: fixed;
    inset: 0;
    background: var(--ground);
    color: var(--ink);
    overflow: hidden;
  }
  .host {
    position: absolute;
    inset: 0;
  }
  .margin {
    position: absolute;
    top: 0;
    bottom: 0;
    padding: 0;
    border: 0;
    background: none;
    cursor: default;
    color: var(--ink-secondary);
  }
  .margin.left {
    left: 0;
  }
  .margin.right {
    right: 0;
  }
  /* A faint chevron on hover (I11), and with the controls (Screen 03). */
  .chevron {
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    color: #8f877b;
    opacity: 0;
    transition: opacity 160ms;
  }
  .left .chevron {
    left: 33px;
  }
  .right .chevron {
    right: 33px;
  }
  .margin:hover .chevron,
  .margin.shown .chevron {
    opacity: 1;
  }
  .chrome {
    position: absolute;
    left: 0;
    right: 0;
    z-index: 10;
    background: var(--ground);
    font-family: var(--font-ui);
    animation: chrome-in var(--motion-chrome-in) ease-out;
  }
  @keyframes chrome-in {
    from {
      opacity: 0;
      transform: translateY(var(--from, -4px));
    }
  }
  .top {
    top: 0;
    height: 52px;
    display: flex;
    align-items: center;
    /* The window buttons sit at x = 20 (native), then an 18 px gap (Screen 03). */
    padding: 0 12px 0 90px;
    border-bottom: 1px solid var(--chrome-hairline);
  }
  .bottom {
    --from: 4px;
    bottom: 0;
    height: 64px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-top: 1px solid var(--chrome-hairline);
  }
  .library {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 36px;
    padding: 0 10px 0 4px;
    border: 0;
    border-radius: 8px;
    background: none;
    color: var(--ink-secondary);
    font: 500 13px var(--font-ui);
    position: relative;
    z-index: 1;
  }
  .library:hover {
    background: var(--hover-wash);
    color: var(--ink);
  }
  .title {
    position: absolute;
    left: 0;
    right: 0;
    text-align: center;
    font-size: 13px;
    color: var(--ink-secondary);
    pointer-events: none;
  }
  .book {
    color: var(--ink);
    font-weight: 600;
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .track {
    position: relative;
    height: 4px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--ink-secondary) 45%, var(--ground));
  }
  .rtl .track {
    display: flex;
    justify-content: flex-end;
  }
  .rtl .labels {
    flex-direction: row-reverse;
  }
  .fill {
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
  }
  .labels {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .location-line {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 30px;
    text-align: center;
    font: 12px var(--font-ui);
    color: var(--ink-secondary);
  }
</style>
