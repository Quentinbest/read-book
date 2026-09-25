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
  import { THEMES, themeVariables, type Theme } from '../lib/theme/tokens'
  import { navigatorTab } from '../lib/reader/state'
  import { setPaletteChapters } from '../app/palette'
  import Navigator from './Navigator.svelte'
  import GoTo, { type GoToTarget } from './GoTo.svelte'
  import FootnotePeek from './FootnotePeek.svelte'
  import MoreMenu from './MoreMenu.svelte'
  import ImageView from './ImageView.svelte'
  import { noteText } from './notes'
  import type { ImageEvent, NoteEvent } from './engine'
  import { buildContents, currentIndex, type Contents, type ContentsItem } from './contents'
  import { bookMediaUrl } from './loader'
  import Kbd from '../components/Kbd.svelte'
  import {
    ReaderEngine,
    SCROLL_LINES,
    ZOOM_LEVELS,
    type ReaderLocation,
    type ReadingMode,
    type Turn,
  } from './engine'
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
  /** Scroll mode: one wheel line, as WebKit scrolls it. */
  const WHEEL_LINE_PX = 40

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
  /** B8, S13: Pages or Scroll, remembered per book. */
  let readingMode = $state<ReadingMode>('pages')
  let navigatorDocked: string | null = null
  /** I15, G8: a right-to-left book fills progress from the right. */
  let rtlBook = $state(false)
  /** G8: an open taking over 500 ms shows one “Opening …” line, never a spinner. */
  let openingShown = $state(false)
  const OPENING_LINE_AFTER_MS = 500
  let theme: Theme = $state(THEMES.paper)
  let chromeVisible = $derived(lanes.chrome === 'controls')
  /** Every theme token, for panels and controls that the inline colours below do not cover. */
  let themeStyle = $derived(
    Object.entries(themeVariables(theme))
      .map(([k, v]) => `${k}: ${v}`)
      .join('; '),
  )
  // ---- Navigator (S2, S3, N6; Screen 04)
  const NAVIGATOR_WIDTH = 320
  const MOTION_NAVIGATOR_MS = 220
  let navigatorOpen = $derived(navigatorTab(lanes) !== null)
  /** The width the docked Navigator takes from the reading area (applied after its slide, V8). */
  let dockedWidth = $state(0)
  let stageShift = $state(0)
  let contents = $state<Contents | null>(null)
  let currentRow = $derived(contents && location ? currentIndex(contents.items, location) : -1)
  let navPanel: ReturnType<typeof Navigator> | undefined = $state()
  // Screens 02/03/04: the window buttons live in the top bar or the Navigator header.
  $effect(() => {
    void ipc.setWindowControls(chromeVisible || navigatorOpen).catch(() => {})
  })
  // V8: the Navigator slides in 220 ms while the column moves by transform; the
  // page reflows at the new width only after the slide, so the text never jumps.
  $effect(() => {
    const target = lanes.docked ? NAVIGATOR_WIDTH : 0
    if (target === dockedWidth) return
    const settle = () => {
      stageShift = 0
      dockedWidth = target
      relayout()
    }
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return settle()
    // The column is centred in the area; docking moves the area's centre by half the width.
    stageShift = (target - dockedWidth) / 2
    const timer = window.setTimeout(settle, MOTION_NAVIGATOR_MS)
    return () => clearTimeout(timer)
  })
  // S13: whether the Navigator is docked is remembered per book.
  let restoredDock = false
  $effect(() => {
    const docked = lanes.docked
    if (!restoredDock) return
    navigatorDocked = docked
    void ipc.bookSettingsSet(book.id, readingMode, docked).catch(() => {})
  })
  /** K3: open Contents (or, when it is open, focus it) on the current chapter (C2). */
  function openContents() {
    if (navigatorTab(lanes) !== 'contents') dispatch({ type: 'openNavigator', tab: 'contents' })
    requestAnimationFrame(() => navPanel?.focusCurrent())
  }
  // ---- Go to (N8) and the scrubber (N7)
  let gotoOpen = $derived(lanes.floating?.kind === 'goto')
  let pageList = $state<{ label: string; href: string }[]>([])
  let gotoLabel: HTMLButtonElement | undefined = $state()
  let gotoAnchor = $state<DOMRect | null>(null)
  /**
   * ⌘J or a progress label: open Go to from the progress label, which is the bottom
   * bar's, or the Navigator header's while the Navigator is open (S3).
   */
  function openGoTo(anchor?: DOMRect) {
    if (!engine) return
    dispatch({ type: 'showChrome' })
    dispatch({ type: 'openFloating', kind: 'goto' })
    if (anchor) gotoAnchor = anchor
    else
      requestAnimationFrame(() => {
        const label =
          document.querySelector<HTMLElement>('.navigator .nav-goto') ?? gotoLabel ?? null
        gotoAnchor = label?.getBoundingClientRect() ?? null
      })
  }
  /** The Contents label for a book fraction or an href. */
  function chapterLabelAt(target: number | string): string {
    if (!engine || !contents) return ''
    const index =
      typeof target === 'number'
        ? engine.sectionAt(target)
        : (engine.book?.resolveHref?.(target)?.index ?? -1)
    return contents.items[currentIndex(contents.items, { sectionIndex: index })]?.label ?? ''
  }
  function onGo(target: GoToTarget) {
    if (location) pushJump(location, target.kind === 'percent' ? 'percent' : 'goto')
    dispatch({ type: 'closeFloating' })
    if (target.kind === 'percent') void engine?.goToFraction(target.fraction)
    else void engine?.goTo(target.href)
  }
  /** N7: dragging the progress track previews the chapter and %, and jumps on release. */
  let scrub = $state<{ fraction: number; x: number } | null>(null)
  let track: HTMLElement | undefined = $state()
  let scrubbedByKey = false
  function scrubAt(e: PointerEvent) {
    if (!track) return
    const r = track.getBoundingClientRect()
    const x = Math.min(r.width, Math.max(0, e.clientX - r.left))
    scrub = { fraction: rtlBook ? 1 - x / r.width : x / r.width, x }
  }
  function scrubEnd() {
    if (!scrub) return
    if (location) pushJump(location, 'percent')
    void engine?.goToFraction(scrub.fraction)
    scrub = null
  }
  function onScrubKey(e: KeyboardEvent) {
    const step =
      {
        ArrowRight: 0.01,
        ArrowUp: 0.01,
        ArrowLeft: -0.01,
        ArrowDown: -0.01,
        PageUp: 0.1,
        PageDown: -0.1,
      }[e.key] ?? 0
    if (!step || !location) return
    e.preventDefault()
    e.stopPropagation()
    // One Back entry for a run of key presses, not one per percent.
    if (!scrubbedByKey) pushJump(location, 'percent')
    scrubbedByKey = true
    void engine?.goToFraction(
      Math.min(1, Math.max(0, location.fraction + (rtlBook ? -step : step))),
    )
  }
  /** Floating popovers close on a click outside them (S2). */
  function onReaderPointerDown(e: PointerEvent) {
    const kind = lanes.floating?.kind
    if (kind !== 'goto' && kind !== 'peek' && kind !== 'more') return
    const inside = (e.target as Element | null)?.closest?.(
      '.goto, .goto-label, .nav-goto, .peek, .more, .more-button',
    )
    if (!inside) dispatch({ type: 'closeFloating' })
  }

  // ---- The ⋯ menu (Screen 03): every command, as in the menu bar and ⌘K
  let moreOpen = $derived(lanes.floating?.kind === 'more')
  let moreAnchor = $state<DOMRect | null>(null)
  function openMore(anchor: DOMRect) {
    moreAnchor = anchor
    if (moreOpen) dispatch({ type: 'closeFloating' })
    else dispatch({ type: 'openFloating', kind: 'more' })
  }

  // ---- Footnote peek (N9), image view (N11), links (N10)
  let note = $state<NoteEvent | null>(null)
  let peek: ReturnType<typeof FootnotePeek> | undefined = $state()
  let peekOpen = $derived(lanes.floating?.kind === 'peek')
  let image = $state<ImageEvent | null>(null)
  let imageZoom = $state(1)
  let imageView: ReturnType<typeof ImageView> | undefined = $state()
  let imageOpen = $derived(lanes.floating?.kind === 'image')
  let hoverUrl = $state<string | null>(null)
  function onNote(n: NoteEvent) {
    note?.marker.classList.remove('linen-peek-marker')
    // The marker keeps focus styling while the peek is open (N9).
    n.marker.classList.add('linen-peek-marker')
    note = n
    dispatch({ type: 'openFloating', kind: 'peek' })
  }
  // Esc (or anything) closing the peek returns focus to the marker (N9).
  $effect(() => {
    if (peekOpen || !note) return
    const marker = note.marker as HTMLElement
    note = null
    marker.classList.remove('linen-peek-marker')
    marker.focus?.({ preventScroll: true })
  })
  function openNoteInPlace() {
    if (!note) return
    const n = note
    if (location) pushJump(location, 'link')
    dispatch({ type: 'closeFloating' })
    void engine?.openNoteInPlace(n)
  }
  function copyNote() {
    if (!note?.note) return
    void ipc.copyText(noteText(note.note)).then(() => announce(t.peek.copied))
  }
  function onImage(i: ImageEvent) {
    image = i
    imageZoom = 1
    dispatch({ type: 'openFloating', kind: 'image' })
  }
  // Closing the image view returns focus to the image in the text (G8).
  $effect(() => {
    if (imageOpen || !image) return
    const el = image.element as HTMLElement
    image = null
    if (!el.hasAttribute('tabindex')) el.setAttribute('tabindex', '-1')
    el.focus?.({ preventScroll: true })
  })

  function closeNavigator() {
    dispatch({ type: 'closeNavigator' })
  }
  function onContentsSelect(item: ContentsItem) {
    if (location) pushJump(location, 'contents')
    void engine?.goTo(item.href)
    // A floating Navigator gives way to the text; a docked one stays (S2).
    if (!lanes.docked) closeNavigator()
  }
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

  // ---- fixed-layout zoom (I17, G8): ⌘+ ⌘− ⌘0, pan by scrolling or dragging
  let zoom = $state(1)
  let zoomChipShown = $state(false)
  let zoomChipTimer = 0
  const ZOOM_CHIP_MS = 2000
  function setZoom(next: number) {
    if (!engine?.fixedLayout) return
    engine.setZoom(next)
    zoom = engine.zoom
    zoomChipShown = true
    clearTimeout(zoomChipTimer)
    zoomChipTimer = window.setTimeout(() => (zoomChipShown = false), ZOOM_CHIP_MS)
  }
  function zoomStep(dir: 1 | -1) {
    const i = ZOOM_LEVELS.findIndex((z) => z >= zoom - 1e-6)
    setZoom(ZOOM_LEVELS[Math.max(0, Math.min(ZOOM_LEVELS.length - 1, i + dir))])
  }
  /** Drag pans a zoomed page; the pointer is inside the page's frame, so use screen coordinates. */
  function panByDrag(doc: Document) {
    let last: { x: number; y: number } | null = null
    doc.addEventListener('pointerdown', (e) => {
      if (zoom > 1 && e.button === 0) last = { x: e.screenX, y: e.screenY }
    })
    doc.addEventListener('pointermove', (e) => {
      if (!last || !(e.buttons & 1)) return (last = null)
      engine?.pan(last.x - e.screenX, last.y - e.screenY)
      last = { x: e.screenX, y: e.screenY }
    })
    doc.addEventListener('pointerup', () => (last = null))
  }

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
      navigatorWidth: dockedWidth,
      // L8, G8: the two-page spread is a Pages-mode layout.
      allowSpread: readingMode === 'pages',
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
    // B1: count pages at the new layout in idle time; until then numbers show “≈”.
    stopCounting()
    const counter = pages
    if (engine && counter)
      stopCounting = engine.countPages(
        (i) => !counter.has(i),
        (i, n) => counter.count(i, n),
      )
  }
  let stopCounting = () => {}

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
    if (engine) zoom = engine.zoom
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
    // S8: modals (⌘K, dialogs) suspend reader input, the wheel included.
    if (!engine || keyContext().modalOpen) return
    dispatch({ type: 'pageTurn' })
    if (readingMode === 'scroll') {
      void engine.turn(dir) // I9: a screen, not a page; no rapid-turn chip
      return
    }
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
    // N9: Tab from the marker moves into the peek.
    if (fromBook && peekOpen && e.key === 'Tab' && !e.shiftKey) {
      e.preventDefault()
      peek?.focusFirst()
      return
    }
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
    // I9: in Scroll mode ↓ ↑ scroll by lines.
    if (
      readingMode === 'scroll' &&
      (e.key === 'ArrowDown' || e.key === 'ArrowUp') &&
      !(e.metaKey || e.ctrlKey || e.altKey || e.shiftKey)
    ) {
      e.preventDefault()
      dispatch({ type: 'pageTurn' })
      engine?.scrollLines(e.key === 'ArrowDown' ? SCROLL_LINES : -SCROLL_LINES)
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
    // B1: numbers are estimates until every section's pages are counted.
    if (l.approximate && l.sectionFraction !== undefined) {
      const inSection = pages.pagesIn(l.sectionIndex)
      const page = Math.min(inSection, Math.floor(l.sectionFraction * inSection) + 1)
      return { n: pages.pageNumber(l.sectionIndex, page), approximate: true }
    }
    return { n: pages.pageNumber(l.sectionIndex, l.page), approximate: !pages.exact }
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
    // G8: in Scroll mode there are no pages to turn.
    if (windowJustActivated || readingMode === 'scroll') return
    const rtl = engine?.rtl ?? false
    turn((side === 'right') !== rtl ? 'next' : 'prev')
  }

  // ---- location line (L9, B2)
  let locationText = $derived.by(() => {
    if (!location) return ''
    if (location.fixedPages?.length)
      return t.reader.fixedPages(location.fixedPages, location.sectionCount)
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
        testHooks.reader = {
          engine: e,
          location: () => location,
          history,
          bookId: book.id,
          pagesExact: () => pages?.exact ?? false,
        }
      }
      relayout()
      cleanups.push(engine.onRelocate(onRelocate))
      cleanups.push(engine.onKey((e) => onPageKey(e, true)))
      // L15: a book font that fails or takes over 1.5 s falls back to Literata, without a prompt.
      cleanups.push(engine.onDocument((doc) => void fallBackFailedFonts(doc)))
      cleanups.push(engine.onDocument(panByDrag))
      cleanups.push(engine.onNote(onNote))
      cleanups.push(engine.onImage(onImage))
      cleanups.push(engine.onLinkHover((href) => (hoverUrl = href)))
      // A click in the text closes the peek (it is not a modal, S8).
      cleanups.push(
        engine.onDocument((doc) =>
          doc.addEventListener('pointerdown', (e) => {
            if (lanes.floating?.kind !== 'peek') return
            if ((e.target as Element | null)?.closest?.('.linen-peek-marker')) return
            dispatch({ type: 'closeFloating' })
          }),
        ),
      )
      cleanups.push(
        engine.onLink((link) => {
          if (link.external) {
            // N10: in the system browser; the core accepts only http(s) and mailto.
            if (testHooks)
              testHooks.externalOpened = [...(testHooks.externalOpened ?? []), link.href]
            else
              void ipc
                .openExternal(link.href)
                .catch((e) => console.warn('external link refused', e))
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
      const settings = await ipc.bookSettingsGet(book.id).catch(() => null)
      if (settings?.[0] === 'scroll') readingMode = 'scroll'
      navigatorDocked = settings?.[1] ?? null
      // S13: reopen the Navigator docked if it was, when the window is wide enough.
      if (navigatorDocked === 'contents' && window.innerWidth >= 1100)
        dispatch({ type: 'openNavigator', tab: 'contents' })
      restoredDock = true
      relayout()
      const opened = await engine.open(await libraryLoader(book.id), {
        cfi: saved?.[0],
        mode: readingMode,
      })
      readingMode = engine.mode
      pageList = engine.pageList
      // N6: Contents from the navigation, headings or spine, with damaged chapters marked.
      void ipc
        .bookDamage(book.id)
        .catch(() => [] as string[])
        .then((damaged) => buildContents(opened, damaged))
        .then((c) => (contents = c))
      // ⌘K finds chapters too (Screen 16: “Type a command, chapter or setting”).
      setPaletteChapters(() =>
        (contents?.items ?? []).map((item) => ({
          label: item.label,
          run: () => onContentsSelect(item),
        })),
      )
      cleanups.push(() => setPaletteChapters(null))
      clearTimeout(openingTimer)
      openingShown = false
      rtlBook = engine.rtl
      zoom = engine.zoom
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
          // I17: a zoomed page pans with the wheel and two fingers; no page turns.
          if (engine?.zoom && engine.zoom > 1) return
          // B8: Scroll mode follows the wheel and trackpad, momentum included.
          if (readingMode === 'scroll') {
            engine?.scrollPixels(-(payload.precise ? payload.dy : payload.dy * WHEEL_LINE_PX))
            return
          }
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
      cleanups.push(registry.handle('navigator.contents', { run: openContents }))
      cleanups.push(registry.handle('goto.open', { run: () => openGoTo() }))
      // B8: Pages and Scroll (the Aa popover's control arrives in Phase 6).
      const switchMode = async (mode: ReadingMode) => {
        if (!engine || !(await engine.setMode(mode))) return
        readingMode = mode
        relayout()
        void ipc.bookSettingsSet(book.id, mode, navigatorDocked).catch(() => {})
      }
      for (const mode of ['pages', 'scroll'] as const)
        cleanups.push(
          registry.handle(`layout.${mode}`, {
            run: () => void switchMode(mode),
            enabled: () => readingMode !== mode && engine?.fixedLayout !== true,
          }),
        )
      // K8 on a fixed-layout book zooms the page (I17); text size itself arrives with Aa.
      // In the image view (N11) they zoom the image.
      const zoomable = () => engine?.fixedLayout === true || imageOpen
      cleanups.push(
        registry.handle('text.larger', {
          run: () => (imageOpen ? imageView?.step(1) : zoomStep(1)),
          enabled: zoomable,
        }),
      )
      cleanups.push(
        registry.handle('text.smaller', {
          run: () => (imageOpen ? imageView?.step(-1) : zoomStep(-1)),
          enabled: zoomable,
        }),
      )
      cleanups.push(
        registry.handle('text.reset', {
          run: () => (imageOpen ? (imageZoom = 1) : setZoom(1)),
          enabled: zoomable,
        }),
      )
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
      stopCounting()
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
  onpointerdowncapture={onReaderPointerDown}
  style:--ground={theme.ground}
  style:--ink={theme.ink}
  style:--ink-secondary={theme.inkSecondary}
  style:--accent={theme.accent}
  style:--chrome-hairline={theme.chromeHairline}
  style={themeStyle}
>
  {#if navigatorOpen}
    <Navigator
      bind:this={navPanel}
      title={book.title}
      author={book.authors.join(', ')}
      coverUrl={book.cover_path ? bookMediaUrl(book.id, book.cover_path) : null}
      coverTint={book.generated_cover_tint}
      fraction={location?.fraction ?? 0}
      {contents}
      current={currentRow}
      floating={!lanes.docked}
      onselect={onContentsSelect}
      onclose={closeNavigator}
      onlibrary={leave}
      ongoto={openGoTo}
    />
  {/if}
  <!-- The reading area: everything right of a docked Navigator. -->
  <div
    class="stage"
    style:left="{dockedWidth}px"
    style:transform={stageShift ? `translateX(${stageShift}px)` : undefined}
    style:transition={stageShift ? `transform ${MOTION_NAVIGATOR_MS}ms ease-out` : undefined}
  >
    <div class="host" bind:this={host}></div>

    <!-- Margins: click targets for the previous and next page (I11, S3). -->
    <button
      class="margin left"
      class:scroll={readingMode === 'scroll'}
      class:shown={chromeVisible}
      class:zoomed={zoom > 1}
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
      class:scroll={readingMode === 'scroll'}
      class:shown={chromeVisible}
      class:zoomed={zoom > 1}
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

    {#if readingMode === 'scroll'}
      <!-- G8: the text runs under the window edges; the location line sits on the lower fade. -->
      <div class="fade top-fade" aria-hidden="true"></div>
      <div class="fade bottom-fade" aria-hidden="true"></div>
    {/if}
    {#if moreOpen && moreAnchor}
      <MoreMenu
        {registry}
        anchor={moreAnchor}
        onclose={() => dispatch({ type: 'closeFloating' })}
      />
    {/if}
    {#if peekOpen && note}
      <FootnotePeek bind:this={peek} {note} onopen={openNoteInPlace} oncopy={copyNote} />
    {/if}
    {#if imageOpen && image}
      <ImageView
        bind:this={imageView}
        {image}
        bind:zoom={imageZoom}
        dim={theme.scheme === 'dark'}
        onclose={() => dispatch({ type: 'closeFloating' })}
      />
    {/if}
    {#if hoverUrl}
      <!-- N10: where an external link goes. -->
      <div class="link-url" role="status">{hoverUrl}</div>
    {/if}
    {#if gotoOpen && contents && gotoAnchor}
      <GoTo
        fraction={location?.fraction ?? 0}
        chapters={contents.items}
        currentChapter={currentRow}
        pages={pageList}
        chapterAt={chapterLabelAt}
        chapterStart={(item) => engine?.sectionStart(item.section) ?? 0}
        anchor={gotoAnchor}
        ongo={onGo}
      />
    {/if}
    {#if zoomChipShown}
      <div class="zoom-chip" role="status">
        <span>{t.reader.zoomLevel(zoom)}</span>
        <button type="button" onclick={() => setZoom(1)}>{t.reader.zoomFit}<Kbd keys="⌘0" /></button
        >
      </div>
    {/if}
    {#if chromeVisible}
      <header
        class="chrome top"
        class:docked={dockedWidth > 0}
        data-tauri-drag-region
        out:chromeOut={{ from: -4 }}
      >
        <button type="button" class="library" onclick={leave}>
          <Icon name="library" size={18} />{t.reader.library}
        </button>
        <div class="title" aria-live="off">
          <span class="book">{book.title}</span>{#if location?.chapterLabel}&nbsp;· {location.chapterLabel}{/if}
        </div>
        <!-- Screen 03: Contents · Search · Notes · Aa · ⋯ (Search, Notes and Aa arrive later). -->
        <div class="tools">
          <button type="button" class="tool" aria-label={t.reader.contents} onclick={openContents}>
            <Icon name="contents" size={18} />
          </button>
          <button
            type="button"
            class="tool more-button"
            aria-label={t.reader.more}
            aria-haspopup="menu"
            aria-expanded={moreOpen}
            onclick={(e) => openMore(e.currentTarget.getBoundingClientRect())}
          >
            <Icon name="more" size={18} />
          </button>
        </div>
      </header>
      <footer class="chrome bottom" out:chromeOut={{ from: 4 }}>
        <div class="progress" class:rtl={rtlBook} style:width="{layout.textWidth}px">
          <div
            class="track"
            class:scrubbing={!!scrub}
            bind:this={track}
            role="slider"
            tabindex="0"
            aria-label={t.goto.scrubber}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round((location?.fraction ?? 0) * 100)}
            aria-valuetext="{progressText} · {location?.chapterLabel ?? ''}"
            onpointerdown={(e) => {
              try {
                track?.setPointerCapture(e.pointerId)
              } catch {
                // not a live pointer (e.g. a synthetic event); dragging still works over the bar
              }
              scrubAt(e)
            }}
            onpointermove={(e) => scrub && scrubAt(e)}
            onpointerup={scrubEnd}
            onpointercancel={() => (scrub = null)}
            onkeydown={onScrubKey}
            onblur={() => (scrubbedByKey = false)}
          >
            <div
              class="fill"
              style:width="{(scrub?.fraction ?? location?.fraction ?? 0) * 100}%"
            ></div>
            <div
              class="thumb"
              style:left="{(rtlBook
                ? 1 - (scrub?.fraction ?? location?.fraction ?? 0)
                : (scrub?.fraction ?? location?.fraction ?? 0)) * 100}%"
            ></div>
            {#if scrub}
              <div class="scrub-tip" style:left="{scrub.x}px">
                {[chapterLabelAt(scrub.fraction), `${Math.round(scrub.fraction * 100)}%`]
                  .filter(Boolean)
                  .join(' · ')}
              </div>
            {/if}
          </div>
          <div class="labels">
            <span>{location?.chapterLabel ?? ''}</span>
            <button
              type="button"
              class="goto-label"
              class:open={gotoOpen}
              bind:this={gotoLabel}
              aria-haspopup="dialog"
              aria-expanded={gotoOpen}
              onclick={(e) => openGoTo(e.currentTarget.getBoundingClientRect())}
            >
              <span class="rest"
                >{[progressText, locationText.split(' · ')[1]].filter(Boolean).join(' · ')}</span
              >
              <span class="hint">{progressText} · {t.goto.open} ⌘J</span>
            </button>
          </div>
        </div>
      </footer>
    {:else if openingShown && !location}
      <div class="location-line">{t.reader.opening(book.title)}</div>
    {:else if showLocationLine(height) && locationText}
      <div class="location-line" aria-hidden="true">{locationText}</div>
    {/if}
  </div>
</div>

<style>
  .reader {
    position: fixed;
    inset: 0;
    background: var(--ground);
    color: var(--ink);
    overflow: hidden;
  }
  .stage {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
  }
  .host {
    position: absolute;
    inset: 0;
  }
  /* With the Navigator docked, its header holds the window buttons (Screen 04). */
  .top.docked {
    padding-left: 12px;
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
  /* G8 Scroll mode: margins do nothing, and fades cover the window edges. */
  .margin.scroll {
    pointer-events: none;
  }
  .margin.scroll .chevron {
    display: none;
  }
  .fade {
    position: absolute;
    left: 0;
    right: 0;
    z-index: 5;
    pointer-events: none;
  }
  .top-fade {
    top: 0;
    height: 40px;
    background: linear-gradient(var(--ground), transparent);
  }
  .bottom-fade {
    bottom: 0;
    height: 88px;
    background: linear-gradient(transparent, var(--ground) 55%);
  }
  .tools {
    margin-left: auto;
    display: flex;
    gap: 4px;
    position: relative;
    z-index: 1;
  }
  .tool {
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 8px;
    background: none;
    color: var(--ink-secondary);
    cursor: default;
  }
  .tool:hover,
  .tool[aria-expanded='true'] {
    background: var(--hover-wash);
    color: var(--ink);
  }
  .link-url {
    position: absolute;
    left: 12px;
    bottom: 12px;
    z-index: 25;
    max-width: min(480px, 60%);
    padding: 4px 8px;
    border-radius: 6px;
    background: var(--tooltip);
    color: var(--tooltip-ink);
    font: 400 12px var(--font-ui);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }
  /* I17: a zoomed page takes the whole window for panning; keys still turn. */
  .margin.zoomed {
    pointer-events: none;
  }
  .zoom-chip {
    position: absolute;
    left: 50%;
    bottom: 24px;
    z-index: 30;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 8px 4px 16px;
    border-radius: 22px;
    /* The message style (M1): ink-coloured, like the selection bar. */
    background: var(--ink);
    color: var(--ground);
    box-shadow: var(--shadow-popover);
    font: 500 var(--text-body) var(--font-ui);
  }
  .zoom-chip button {
    min-height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: 18px;
    background: none;
    color: inherit;
    font-weight: 600;
    cursor: default;
  }
  .zoom-chip button:hover {
    background: color-mix(in srgb, var(--ground) 14%, transparent);
  }
  .zoom-chip :global(kbd) {
    color: inherit;
    opacity: 0.75;
    margin-left: 4px;
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
    cursor: default;
    touch-action: none;
  }
  /* A taller hit area than the 4 px bar (X4). */
  .track::before {
    content: '';
    position: absolute;
    inset: -10px 0;
  }
  .thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    margin: -6px 0 0 -6px;
    border-radius: 50%;
    background: var(--accent);
    opacity: 0;
    transition: opacity 100ms;
    pointer-events: none;
  }
  .track:hover .thumb,
  .track:focus-visible .thumb,
  .track.scrubbing .thumb {
    opacity: 1;
  }
  /* N7, Screen 03: the dark preview above the track while dragging. */
  .scrub-tip {
    position: absolute;
    bottom: 16px;
    transform: translateX(-50%);
    padding: 6px 10px;
    border-radius: 6px;
    background: var(--tooltip);
    color: var(--tooltip-ink);
    font: 400 12px var(--font-ui);
    white-space: nowrap;
    pointer-events: none;
  }
  /* N8, Screen 17: the progress label opens Go to; it shows the hint when pointed at. */
  .goto-label {
    padding: 2px 6px;
    margin: -2px -6px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    font: inherit;
    cursor: default;
  }
  .goto-label .hint {
    display: none;
  }
  .goto-label:hover,
  .goto-label:focus-visible,
  .goto-label.open {
    color: var(--accent);
    font-weight: 600;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .goto-label:hover .rest,
  .goto-label:focus-visible .rest,
  .goto-label.open .rest {
    display: none;
  }
  .goto-label:hover .hint,
  .goto-label:focus-visible .hint,
  .goto-label.open .hint {
    display: inline;
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
    /* Above the Scroll-mode fades (G8). */
    z-index: 6;
    left: 0;
    right: 0;
    bottom: 30px;
    text-align: center;
    font: 12px var(--font-ui);
    color: var(--ink-secondary);
  }
</style>
