<script lang="ts" module>
  /** Only the session's first book counts for the cold-start mark. */
  let startupMarked = false
</script>

<script lang="ts">
  // The reader (plan Phase 2; Screens 02, 03, 14). Features that arrive in later
  // phases (Contents, Search, Notes, Aa, the ⋯ menu, Go to) are hidden until then.
  import { listen } from '@tauri-apps/api/event'
  import { onMount, untrack } from 'svelte'
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
  import LookUpPeek from './LookUpPeek.svelte'
  import { ReadingSessions } from './sessions'
  import MoreMenu from './MoreMenu.svelte'
  import SearchPanel from './SearchPanel.svelte'
  import { SearchState, type Hit } from './search.svelte'
  import { BookSearch } from './bookSearch'
  import ImageView from './ImageView.svelte'
  import { noteText } from './notes'
  import type { ImageEvent, NoteEvent, SelectionEvent } from './engine'
  import { cfiOrder } from './engine'
  import SelectionBar from './SelectionBar.svelte'
  import NoteCard from './NoteCard.svelte'
  import NotesPanel from './NotesPanel.svelte'
  import { Annotations, COLOR_NAMES, COLORS, UndoStack } from './annotations.svelte'
  import type { Annotation, HighlightColor } from '../lib/annotations/model'
  import { popUpMenu, type MenuEntry } from '../app/nativeMenu'
  import { MOTION, multipliedTint, parseColor } from '../lib/theme/tokens'
  import AaPopover from './AaPopover.svelte'
  import type { ExtensionHost, ReaderBridge } from '../extensions/host.svelte'
  import ExtensionTab from './ExtensionTab.svelte'
  import type { NavigatorTab } from '../lib/reader/state'
  import { toW3C } from '../lib/annotations/model'
  import { clampTextSize, DEFAULT_TEXT_SIZE, stepTextSize } from './textSizes'
  import { applyTheme, type ThemeChoice } from '../app/theme'
  import { changeSetting, onSettingChanged } from '../app/settingsSync'
  import { buildContents, currentIndex, type Contents, type ContentsItem } from './contents'
  import { coverUrl as bookCoverUrl } from './loader'
  import Kbd from '../components/Kbd.svelte'
  import {
    ReaderEngine,
    SCROLL_LINES,
    ZOOM_LEVELS,
    type ReaderLocation,
    type ReadingMode,
    type Turn,
  } from './engine'
  import { dyslexicFaces, fallBackFailedFonts, literataFaces } from './fonts'
  import {
    PAGE_WIDTH_CH,
    SETTING as TYPE_SETTING,
    effectivePublisherStyles,
    parseFont,
    parsePageWidth,
    parsePublisherStyles,
    simplifyKey,
    type FontChoice,
    type PageWidth,
    type PublisherStyles,
  } from './typography'
  import { libraryLoader } from './loader'
  import { computeLayout, showLocationLine, type Layout, type Spacing } from './layout'
  import { ReadingPace } from './pace'
  import { PageCounter } from './pages'
  import { readerStyles } from './styles'

  let {
    book: bookProp,
    messages,
    writes,
    registry,
    screenReader,
    keyContext,
    onBeforeQuit,
    announce,
    onexit,
    extensions,
    active = true,
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
    /** Phase 7: the extension host (slots, and the book as extensions may see it). */
    extensions: ExtensionHost
    /**
     * S14: false while the library shows. The book stays warm (open, laid out, in
     * memory) but takes no keys, wheel, commands or window buttons, and extensions
     * do not see it.
     */
    active?: boolean
  } = $props()
  /**
   * The book, taken once: a reader is keyed to one book (App's {#key}). Read live, the
   * prop would already name the next book while this reader is torn down (S14: a warm
   * book replaced by another), and its last save would land on the wrong book.
   */
  const book = untrack(() => bookProp)

  // S14: the reader's commands are attached only while it is active; a warm book in
  // the library behind keeps them, detached. Not reactive state: nothing renders from it.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const attached = new Map<
    string,
    { h: Parameters<CommandRegistry['handle']>[1]; off: (() => void) | null }
  >()
  function handle(id: string, h: Parameters<CommandRegistry['handle']>[1]): () => void {
    const entry = { h, off: active ? registry.handle(id, h) : null }
    attached.set(id, entry)
    return () => {
      entry.off?.()
      if (attached.get(id) === entry) attached.delete(id)
    }
  }
  $effect(() => {
    if (active) {
      for (const [id, e] of attached) e.off ??= registry.handle(id, e.h)
      return
    }
    for (const e of attached.values()) {
      e.off?.()
      e.off = null
    }
  })

  /** The book as the tests see it (`testHooks.reader`), while active. */
  let hookReader: NonNullable<typeof testHooks>['reader']
  let bridgeReady = $state(false)
  // Extensions see the book only while it is open and the one being read.
  $effect(() => {
    const see = active && bridgeReady
    untrack(() => {
      if (see) extensions.bridge = bridge
      else if (extensions.bridge === bridge) extensions.bridge = null
    })
  })
  $effect(() => {
    const on = active
    untrack(() => {
      if (testHooks && hookReader) testHooks.reader = on ? hookReader : undefined
      if (on) {
        // B2: time in the library is not time on this page.
        pageShownAt = performance.now()
        // Back from the library: the page has focus, as when the book opened.
        requestAnimationFrame(() => engine?.focusPage())
        return
      }
      // Its lines (Back to p. 12, highlights that couldn't be placed) leave with it.
      for (const id of jumpMessages) messages.withdraw(id)
      for (const id of annotationMessages) messages.withdraw(id)
      saveNow()
      sessions.end()
    })
  })

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
  /** The Navigator's cover: straight from the zip, or (SVG covers) through foliate-js. */
  let coverUrl = $state<string | null>(null)
  let currentRow = $derived(contents && location ? currentIndex(contents.items, location) : -1)
  let navPanel: ReturnType<typeof Navigator> | undefined = $state()
  // Screens 02/03/04: the window buttons live in the top bar or the Navigator header.
  $effect(() => {
    // S14: the library, over a warm book, always has its window buttons.
    void ipc.setWindowControls(!active || chromeVisible || navigatorOpen).catch(() => {})
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
    if (kind !== 'goto' && kind !== 'peek' && kind !== 'more' && kind !== 'aa' && kind !== 'note')
      return
    const inside = (e.target as Element | null)?.closest?.(
      '.goto, .goto-label, .nav-goto, .peek, .more, .more-button, .aa, .aa-button, .note',
    )
    if (!inside) dispatch({ type: 'closeFloating' })
  }

  // ---- Search (F1–F8; Screen 05)
  let bookSearch: BookSearch | null = null
  let searchPanel: ReturnType<typeof SearchPanel> | undefined = $state()
  const searchState = new SearchState(
    () => (bookSearch ??= engine?.book ? new BookSearch(book.id, engine.book) : null),
    () => location?.sectionIndex ?? 0,
    () => updateMarks(),
  )
  let searchOpen = $derived(navigatorTab(lanes) === 'search')
  /** F7: where the reader was when Search opened, and what they did since. */
  let searchOrigin: ReaderLocation | null = null
  let searchBrowsed = false
  let searchChosen = false
  function chapterLabelFor(index: number): string {
    const items = contents?.items ?? []
    return items[currentIndex(items, { sectionIndex: index })]?.label ?? ''
  }
  /** F5: marks in the page, only while Search is open; redrawn at most once a frame. */
  let marksFrame = 0
  function updateMarks() {
    if (marksFrame) return
    marksFrame = requestAnimationFrame(() => {
      marksFrame = 0
      drawMarks()
    })
  }
  function drawMarks() {
    if (!engine) return
    if (!searchOpen) return engine.setSearchMarks(null)
    const a = searchState.active
    const hit = a ? searchState.groups.find((g) => g.index === a.index)?.matches[a.n] : null
    engine.setSearchMarks({
      bySection: new Map(searchState.groups.map((g) => [g.index, g.matches])),
      active: a && hit ? { index: a.index, start: hit.start, end: hit.end } : null,
      colors: theme.search,
    })
  }
  /** F1: ⌘F, the search button or ⌘K; a selection pre-fills the field. */
  function openSearch() {
    if (!searchOpen) {
      searchOrigin = location
      searchBrowsed = false
      searchChosen = false
      const selected = engine?.selectionText ?? ''
      if (selected) {
        searchState.setQuery(selected)
        searchState.run()
      }
      dispatch({ type: 'openNavigator', tab: 'search' })
    }
    requestAnimationFrame(() => {
      searchPanel?.focusField()
      updateMarks()
    })
  }
  async function goToHit(hit: Hit) {
    searchState.active = { index: hit.index, n: hit.n }
    updateMarks()
    await engine?.goToText(hit.index, hit.match.start, hit.match.end)
  }
  /** F6: ↵ ⇧↵ ↑ ↓ ⌘G: browse; the page follows, and Esc can still go back (F7). */
  function browseHit(hit: Hit) {
    searchBrowsed = true
    void goToHit(hit)
  }
  /** F7: choosing a result stays there; Back returns to where Search began. */
  function chooseHit(hit: Hit) {
    if (!searchChosen && searchOrigin) pushJump(searchOrigin, 'search')
    searchChosen = true
    void goToHit(hit)
  }
  function stepResult(dir: 1 | -1) {
    if (!searchState.count) return
    if (!searchOpen) openSearch()
    const hit = searchState.step(dir)
    if (hit) browseHit(hit)
  }
  // Leaving Search removes the marks; closing it after only browsing returns to the
  // original page (F7). Switching to another tab stays where you are.
  let wasSearchOpen = false
  $effect(() => {
    const open = searchOpen
    const navigatorClosed = navigatorTab(lanes) === null
    if (wasSearchOpen && !open) {
      engine?.setSearchMarks(null)
      if (navigatorClosed && searchBrowsed && !searchChosen && searchOrigin)
        void engine?.goTo(searchOrigin.cfi)
      searchBrowsed = false
      searchChosen = false
      searchOrigin = null
    }
    wasSearchOpen = open
  })

  // ---- Selection and annotation (A1–A11; Screens 06, 07, 08, 14, 15)
  // The reader is recreated for each book ({#key}), so the book it starts with is its book.
  // svelte-ignore state_referenced_locally
  const annotations = new Annotations({
    bookId: book.id,
    contentHash: book.content_hash,
    writes,
    api: { save: ipc.annotationSave, delete: ipc.annotationDelete },
  })
  const undo = new UndoStack()
  /** Undo messages from this session; they go when the book closes (their action needs it). */
  const annotationMessages: number[] = []
  let annotationsReady = $state(false)
  let selection = $state.raw<SelectionEvent | null>(null)
  let bar = $state.raw<{
    mode: 'new' | 'existing' | 'attach'
    id?: string
    first: DOMRect
    last: DOMRect
  } | null>(null)
  let selectionBar: ReturnType<typeof SelectionBar> | undefined = $state()
  let barOpen = $derived(lanes.floating?.kind === 'selection' && bar !== null)
  let noteFor = $state<string | null>(null)
  let noteBefore: string | null = null
  let noteCard: ReturnType<typeof NoteCard> | undefined = $state()
  let noteOpen = $derived(lanes.floating?.kind === 'note' && noteFor !== null)
  let notePlace = $state.raw<{ left: number; top: number; dotX: number; dotY: number } | null>(null)
  let noteMode = $state<'margin' | 'sheet'>('margin')
  let notesPanel: ReturnType<typeof NotesPanel> | undefined = $state()
  let notesOpen = $derived(navigatorTab(lanes) === 'notes')
  /** The highlight last jumped to from the Notes tab. */
  let jumpedTo = $state<string | null>(null)
  /** G11 (provisional): the highlight being re-attached to a new selection. */
  let reattaching = $state.raw<Annotation | null>(null)
  const NOTE_MARGIN_MIN_WIDTH = 1240
  const NOTE_CARD_WIDTH = 256
  const shortQuote = (q: string) => {
    const s = q.replace(/\s+/g, ' ').trim()
    return s.length > 40 ? s.slice(0, 40).replace(/\s+\S*$/, '') + '…' : s
  }
  const accentAt = (alpha: number) =>
    `rgba(${parseColor(theme.accent).slice(0, 3).join(',')},${alpha})`

  // A4: highlights are drawn from their CFIs in every loaded chapter, and again on reflow.
  $effect(() => {
    if (!annotationsReady || !engine) return
    engine.setHighlights(
      annotations.placed.map((a) => ({ id: a.id, cfi: a.cfi, color: a.color, note: !!a.note })),
      {
        colors: theme.highlight,
        multiply: theme.scheme === 'light',
        overlayTints: Object.fromEntries(
          COLORS.map((c) => [
            c,
            theme.scheme === 'light' ? multipliedTint(theme, c) : theme.highlight[c].tint,
          ]),
        ) as Record<HighlightColor, string>,
      },
    )
  })
  // The bar and the card go with their floating layer (a page turn, Esc, another layer).
  $effect(() => {
    if (lanes.floating?.kind !== 'selection' && bar) bar = null
  })
  $effect(() => {
    if (lanes.floating?.kind !== 'note' && noteFor) noteClosed()
  })

  /** Something changed that ⌘Z, the message's Undo and ⌘K › Recently closed can take back (B5). */
  function offerUndo(text: string, closedLabel: string, restore: () => void) {
    let id = 0
    const run = undo.push(() => {
      restore()
      messages.withdraw(id)
    })
    id = messages.push({
      text,
      closedLabel,
      action: { label: t.annotations.undo, shortcut: '⌘Z', run },
    }).id
    annotationMessages.push(id)
  }

  function onSelection(e: SelectionEvent | null) {
    selection = e
    if (!e) {
      if (bar?.mode !== 'existing' && lanes.floating?.kind === 'selection')
        dispatch({ type: 'closeFloating' })
      return
    }
    // S7: a new selection dismisses popovers and peeks; the bar takes the floating lane.
    dispatch({ type: 'selectionStart' })
    bar = { mode: reattaching ? 'attach' : 'new', first: e.first, last: e.last }
    dispatch({ type: 'openFloating', kind: 'selection' })
    announce(t.annotations.barAnnounce)
  }

  function onHighlightClick(e: { id: string; first: DOMRect; last: DOMRect }) {
    if (!annotations.get(e.id)) return
    dispatch({ type: 'selectionStart' })
    bar = { mode: 'existing', id: e.id, first: e.first, last: e.last }
    dispatch({ type: 'openFloating', kind: 'selection' })
    announce(t.annotations.barAnnounce)
  }

  function closeBar(clear: boolean) {
    if (clear) {
      engine?.clearSelection()
      selection = null
    }
    if (lanes.floating?.kind === 'selection') dispatch({ type: 'closeFloating' })
    bar = null
  }

  /** A4: a colour saves the highlight at once; the same range again changes its colour (B4). */
  function highlightSelection(color: HighlightColor): Annotation | null {
    const sel = selection
    if (!sel) return null
    const { annotation, previousColor } = annotations.highlight(
      { cfi: sel.cfi, quote: sel.quote },
      color,
    )
    if (previousColor && previousColor !== color) offerRecolorUndo(annotation, previousColor)
    else announce(t.annotations.highlighted(COLOR_NAMES[color]))
    closeBar(true)
    return annotation
  }

  function offerRecolorUndo(a: Annotation, previous: HighlightColor) {
    offerUndo(
      t.annotations.recolored(COLOR_NAMES[a.color]),
      t.annotations.restoreColor(shortQuote(a.quote.exact)),
      () => annotations.setColor(a.id, previous),
    )
  }

  function recolor(id: string, color: HighlightColor) {
    const a = annotations.get(id)
    const previous = annotations.setColor(id, color)
    if (a && previous) offerRecolorUndo({ ...a, color }, previous)
  }

  /** A7: delete at once, with Undo; never a confirmation. */
  function deleteAnnotation(id: string) {
    const gone = annotations.remove(id)
    if (!gone) return
    if (noteFor === id) {
      noteFor = null
      if (lanes.floating?.kind === 'note') dispatch({ type: 'closeFloating' })
    }
    closeBar(false)
    offerUndo(t.annotations.deleted, t.annotations.restore(shortQuote(gone.quote.exact)), () =>
      annotations.restore(gone),
    )
    engine?.focusPage()
  }

  /** B4: Copy copies the plain text only. */
  function copyText(text: string) {
    void ipc.copyText(text).then(() => announce(t.annotations.copied))
  }

  function searchFor(text: string) {
    closeBar(false)
    searchState.setQuery(text.replace(/\s+/g, ' ').trim())
    searchState.run()
    openSearch()
  }

  /** A6: a note belongs to a highlight; N on a selection highlights it first. */
  function noteOnSelection() {
    const sel = selection
    if (!sel) return
    const existing = annotations.placed.find((a) => a.cfi === sel.cfi)
    const a = existing ?? highlightSelection(annotations.lastColor)
    closeBar(true)
    if (a) openNote(a.id)
  }

  function openNote(id: string) {
    const a = annotations.get(id)
    if (!a) return
    const box = engine?.passageBox(a.cfi)
    noteMode = 'sheet'
    notePlace = null
    if (box && window.innerWidth >= NOTE_MARGIN_MIN_WIDTH) {
      const dotX = box.edge + 9.5
      const dotY = box.first.top + box.first.height / 2
      const left = dotX + 36
      if (left + NOTE_CARD_WIDTH + 16 <= window.innerWidth) {
        noteMode = 'margin'
        notePlace = { left, top: Math.max(8, Math.min(dotY - 23, height - 280)), dotX, dotY }
      }
    }
    noteFor = id
    noteBefore = a.note
    dispatch({ type: 'openFloating', kind: 'note' })
  }

  /** The card closed: an emptied note can be undone (B5). */
  function noteClosed() {
    const id = noteFor
    noteFor = null
    const before = noteBefore
    noteBefore = null
    if (!id) return
    const a = annotations.get(id)
    if (a && before && !a.note)
      offerUndo(
        t.annotations.noteDeleted,
        t.annotations.restoreNote(shortQuote(a.quote.exact)),
        () => void annotations.setNote(id, before),
      )
  }

  function closeNote() {
    noteCard?.flush()
    if (lanes.floating?.kind === 'note') dispatch({ type: 'closeFloating' })
    engine?.focusPage()
  }

  // A8: the Notes tab. Choosing a highlight jumps, pulses it for 1.2 s and offers Back.
  function openNotes() {
    if (!notesOpen) dispatch({ type: 'openNavigator', tab: 'notes' })
    requestAnimationFrame(() => notesPanel?.focusField())
  }
  async function chooseAnnotation(a: Annotation) {
    if (location) pushJump(location, 'annotation')
    jumpedTo = a.id
    if (!lanes.docked) closeNavigator()
    await engine?.goTo(a.cfi)
    // After the jump has laid out (not rAF: WebKit defers frames for a window no one sees).
    setTimeout(() => engine?.pulse(a.cfi, accentAt(0.4)), 16)
  }
  function chapterOf(a: Annotation): number {
    return engine?.cfiIndex(a.cfi) ?? -1
  }

  // G11 (provisional): Re-attach asks for the passage to be selected again.
  function startReattach(a: Annotation) {
    reattaching = a
    if (!lanes.docked) closeNavigator()
    engine?.focusPage()
  }
  function attachSelection() {
    const a = reattaching
    const sel = selection
    if (!a || !sel) return
    annotations.reanchor(a.id, { cfi: sel.cfi, quote: sel.quote }, true)
    reattaching = null
    closeBar(true)
    messages.push({ text: t.annotations.reattached })
  }
  function cancelReattach() {
    reattaching = null
    closeBar(true)
  }

  /** B3: highlights made against another edition of the file are placed again. */
  async function reanchorStale() {
    const stale = annotations.stale
    if (!stale.length || !engine) return
    const placed = await engine.placeQuotes(stale.map((a) => ({ quote: a.quote, cfi: a.cfi })))
    let lost = 0
    stale.forEach((a, i) => {
      const p = placed[i]
      annotations.reanchor(a.id, p ? { cfi: p.cfi, quote: p.quote } : null)
      if (!p) lost++
    })
    if (lost)
      annotationMessages.push(
        messages.push({
          text: t.annotations.unplacedAfterUpdate(lost),
          action: { label: t.annotations.show, run: openNotes },
        }).id,
      )
  }

  /** A10: right-click on a selection or a highlight gives the same actions, natively. */
  async function contextMenu(e: MouseEvent) {
    const sel = selection
    const hit = sel ? null : engine?.highlightAt(e)
    const existing = hit ? annotations.get(hit) : null
    if (!sel && !existing) return
    e.preventDefault()
    const entries: MenuEntry[] = [
      ...(['yellow', 'green', 'blue', 'rose'] as const).map((c) => ({
        label: `Highlight ${COLOR_NAMES[c]}`,
        run: () => void (existing ? recolor(existing.id, c) : highlightSelection(c)),
      })),
      null,
      {
        label: t.annotations.note,
        run: () => (existing ? openNote(existing.id) : noteOnSelection()),
      },
      {
        label: t.annotations.copy,
        run: () => copyText(existing ? existing.quote.exact : (sel?.text ?? '')),
      },
      existing
        ? { label: t.annotations.delete, run: () => deleteAnnotation(existing.id) }
        : { label: t.annotations.search, run: () => searchFor(sel?.text ?? '') },
      ...(sel && !existing
        ? [{ label: t.lookUp.contextMenu(sel.text.trim()), run: () => lookUp(sel.text, sel.last) }]
        : []),
    ]
    await popUpMenu(entries)
  }

  // ---- Extensions (Phase 7): slots, and the book as extensions may see it (P2)
  /** E2: a fixed-layout book (Aa keeps theme and zoom only). */
  let fixedBook = $state(false)
  /** P8: extensions pinned in the ⋯ menu (Settings › Extensions). */
  let pinnedExtensions = $state<string[]>([])
  const bridge: ReaderBridge = {
    metadata: () => ({
      title: book.title,
      authors: book.authors,
      language: book.language,
      identifier: book.package_identifier,
    }),
    chapters: async () =>
      Array.from({ length: engine?.chapterCount ?? 0 }, (_, index) => ({
        index,
        label: chapterLabelFor(index),
      })),
    text: async (chapter) => {
      if (!engine) throw new Error('no book is open')
      return engine.chapterText(chapter)
    },
    annotations: () => {
      const source = book.package_identifier ?? `urn:linen:book:${book.id}`
      const items = [...annotations.placed]
        .sort((a, b) => cfiOrder(a.cfi, b.cfi))
        .map((a) => ({
          ...toW3C(a, source),
          'linen:chapter': chapterLabelFor(engine?.cfiIndex(a.cfi) ?? -1),
        }))
      return {
        '@context': 'http://www.w3.org/ns/anno.jsonld',
        type: 'AnnotationCollection',
        label: book.title,
        total: items.length,
        first: { type: 'AnnotationPage', items },
      }
    },
  }
  // Read-only annotation events (P§18): created, changed, deleted.
  let seenAnnotations: Map<string, number> | null = null
  $effect(() => {
    const items = annotations.items
    if (!annotationsReady) return
    const now = new Map(items.map((a) => [a.id, a.updatedAt]))
    const before = seenAnnotations
    seenAnnotations = now
    if (!before) return
    const source = book.package_identifier ?? `urn:linen:book:${book.id}`
    for (const a of items) {
      if (!before.has(a.id)) extensions.emitAnnotationEvent('created', toW3C(a, source))
      else if (before.get(a.id) !== a.updatedAt)
        extensions.emitAnnotationEvent('changed', toW3C(a, source))
    }
    for (const id of before.keys())
      if (!now.has(id)) extensions.emitAnnotationEvent('deleted', { id: `urn:uuid:${id}` })
  })

  /** P10: extension actions for the selection, whose `when` holds. */
  const selectionExtensionActions = $derived(
    bar?.mode === 'new' && selection
      ? extensions.selectionActions({
          'selection.words': selection.text.trim().split(/\s+/).filter(Boolean).length,
          'selection.chars': selection.text.length,
          'selection.language': book.language ?? '',
          'book.language': book.language ?? '',
          'book.fixedLayout': fixedBook,
        })
      : [],
  )
  /** Run an extension's selection action; its Navigator tab (if any) shows the result. */
  async function runSelectionAction(a: { extId: string; command: string; name: string }) {
    const sel = selection
    if (!sel) return
    closeBar(false)
    const tab = extensions.navigatorTabs().find((x) => x.extId === a.extId)
    if (tab) dispatch({ type: 'openNavigator', tab: `extension:${a.extId}/${tab.id}` })
    try {
      await extensions.invoke(
        a.extId,
        a.command,
        // The text itself only through linen.book.selection(), which needs its permission.
        { source: 'selection' },
        { selection: { text: sel.text, cfi: sel.cfi } },
      )
    } catch {
      messages.push({
        text: t.extensions.stopped(a.name),
        action: { label: t.extensions.restart, run: () => void extensions.restart(a.extId) },
      })
    }
  }
  /** Notes › Export (P§18): an exporter extension, e.g. Markdown Export. */
  async function runExporter(e: { extId: string; command: string; name: string }) {
    try {
      const r = (await extensions.invoke(e.extId, e.command, { export: { book: book.title } })) as {
        saved?: boolean
      } | null
      if (r?.saved) messages.push({ text: t.extensions.exported })
    } catch {
      messages.push({
        text: t.extensions.exportFailed(e.name),
        action: { label: t.extensions.restart, run: () => void extensions.restart(e.extId) },
      })
    }
  }
  /** The Navigator's extension tab showing now, if any (G7). */
  const extensionTab = $derived.by(() => {
    const tab = navigatorTab(lanes)
    if (!tab?.startsWith('extension:')) return null
    return extensions.navigatorTabs().find((x) => tab === `extension:${x.extId}/${x.id}`) ?? null
  })

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
  // ---- Dictionary peek (1.1, PROVISIONAL): this Mac's dictionaries, nothing sent anywhere.
  let lookup = $state<{ word: string; rect: DOMRect; definition?: string | null } | null>(null)
  let lookupOpen = $derived(lanes.floating?.kind === 'lookup' && lookup !== null)
  /** The longest text looked up: a phrase, not a passage. */
  const LOOKUP_MAX = 80
  function lookUp(text: string, rect: DOMRect) {
    const word = text.replace(/\s+/g, ' ').trim().slice(0, LOOKUP_MAX)
    if (!word) return
    const request = { word, rect }
    lookup = request
    dispatch({ type: 'openFloating', kind: 'lookup' })
    void ipc
      .lookUp(word)
      .catch(() => null)
      .then((definition) => {
        if (lookup?.word === word && lookup.rect === rect) lookup = { ...request, definition }
      })
  }
  function lookUpSelection() {
    if (selection) lookUp(selection.text, selection.last)
  }
  function openInDictionary() {
    const word = lookup?.word
    dispatch({ type: 'closeFloating' })
    if (!word) return
    // Tests never open other apps (N10).
    if (testHooks) testHooks.externalOpened = [...(testHooks.externalOpened ?? []), `dict:${word}`]
    else void ipc.openDictionary(word).catch(() => {})
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
  // ---- Reading settings, the Aa popover (L4, L5, V1, B8; Screen 09)
  let fontPx = $state(DEFAULT_TEXT_SIZE)
  let spacing = $state<Spacing>('default')
  let themeChoice = $state<ThemeChoice>('auto')
  // 1.1 (PROVISIONAL): font family and page width (all books), Publisher styles (C5).
  let fontChoice = $state<FontChoice>('book')
  let pageWidth = $state<PageWidth>('normal')
  let publisherStyles = $state<PublisherStyles>('balanced')
  let simplified = $state(false)
  let extraFaces = ''
  let aaOpen = $derived(lanes.floating?.kind === 'aa')
  let aaButton: HTMLButtonElement | undefined = $state()
  let aaAnchor = $state<DOMRect | null>(null)
  /** L18: over 30% of the book is code or tables (measured once per book, in idle time). */
  let codeHeavy = $state(false)
  const CODE_HEAVY_SHARE = 0.3
  const CODE_SHARE_DELAY_MS = 5000
  let unmounted = false
  function openAa() {
    if (aaOpen) return dispatch({ type: 'closeFloating' })
    if (!chromeVisible) dispatch({ type: 'showChrome' })
    requestAnimationFrame(() => {
      aaAnchor = aaButton?.getBoundingClientRect() ?? new DOMRect(window.innerWidth - 90, 8, 36, 36)
      dispatch({ type: 'openFloating', kind: 'aa' })
    })
  }
  /** L4: all books; ⌘+ ⌘− ⌘0 and the slider. */
  function setFont(px: number) {
    const next = clampTextSize(px)
    if (next === fontPx) return
    fontPx = next
    void changeSetting('fontPx', String(next))
    relayout()
  }
  /** L5: all books. */
  function setSpacing(next: Spacing) {
    if (next === spacing) return
    spacing = next
    void changeSetting('lineSpacing', next)
    relayout()
  }
  /** 1.1: all books. The dyslexia-friendly face is loaded the first time it is chosen. */
  async function setFontChoice(next: FontChoice) {
    if (next === fontChoice) return
    fontChoice = next
    void changeSetting(TYPE_SETTING.font, next)
    extraFaces = next === 'dyslexic' ? await dyslexicFaces() : ''
    if (fontChoice === next) relayout()
  }
  /** 1.1: all books. */
  function setPageWidth(next: PageWidth) {
    if (next === pageWidth) return
    pageWidth = next
    void changeSetting(TYPE_SETTING.width, next)
    relayout()
  }
  /** C5: all books (set in Settings). */
  function setPublisherStyles(next: PublisherStyles) {
    if (next === publisherStyles) return
    publisherStyles = next
    relayout()
  }
  /** C5: Simplify styles, this book only; a message offers Undo. */
  function toggleSimplified() {
    simplified = !simplified
    void ipc.settingSet(simplifyKey(book.id), simplified ? 'on' : 'off').catch(() => {})
    relayout()
    messages.push({
      text: simplified ? t.reader.simplified : t.reader.unsimplified,
      action: { label: t.reader.undo, run: toggleSimplified },
    })
  }
  /** B8: this book only. */
  async function switchMode(mode: ReadingMode) {
    if (!engine || !(await engine.setMode(mode))) return
    readingMode = mode
    relayout()
    void ipc.bookSettingsSet(book.id, mode, navigatorDocked).catch(() => {})
  }
  /** V1: all books, and the app around them; Auto follows the system. */
  async function setTheme(choice: ThemeChoice) {
    themeChoice = choice
    applyTheme(choice, themePacks.find((p) => p.value === choice)?.theme)
    await changeSetting('theme', choice)
    theme = await resolveTheme()
    relayout()
  }
  let edges = { topStart: 0, dockBottom: false }
  let announceTurns = true
  /** V8, G2: the page-turn crossfade preference (off by default: turns are instant). */
  let crossfadeTurns = false
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
    for (const fx of effects) if (fx.type === 'saveNote') noteCard?.flush()
    lanes = state
    for (const fx of effects) if (fx.type === 'focusText') engine?.focusPage()
  }

  // ---- theme (V1, V2): Auto follows the system between Paper and Night
  async function resolveTheme(): Promise<Theme> {
    const choice = (await ipc.settingGet('theme')) ?? 'auto'
    if (choice === 'sepia' || choice === 'night' || choice === 'paper') return THEMES[choice]
    // P9: an extension's theme pack, while it is installed, on and passes its checks.
    const pack = extensions
      .themes()
      .find((x) => `ext:${x.extId}/${x.id}` === choice && !x.problems.length)
    if (pack) return pack.theme
    return matchMedia('(prefers-color-scheme: dark)').matches ? THEMES.night : THEMES.paper
  }
  /** P9: theme packs for the Aa popover. */
  const themePacks = $derived(
    extensions
      .themes()
      .filter((x) => !x.problems.length)
      .map((x) => ({
        value: `ext:${x.extId}/${x.id}` as ThemeChoice,
        label: x.title,
        theme: x.theme,
      })),
  )

  function relayout() {
    height = window.innerHeight
    layout = computeLayout({
      width: window.innerWidth,
      height: window.innerHeight,
      fontPx,
      spacing,
      measureCh: PAGE_WIDTH_CH[pageWidth],
      navigatorWidth: dockedWidth,
      // L8, G8: the two-page spread is a Pages-mode layout.
      allowSpread: readingMode === 'pages',
    })
    const publisher = effectivePublisherStyles(publisherStyles, simplified)
    engine?.setPublisherStyles(publisher)
    engine?.applyLayout(
      layout,
      fontFaces +
        extraFaces +
        readerStyles({
          fontPx,
          lineHeight: layout.lineHeight,
          theme,
          hyphenate: true,
          pageHeight: layout.pageHeight,
          font: fontChoice,
          publisher,
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
    void writes.write(`position:${book.id}`, () =>
      ipc.positionSave(book.id, l.cfi, l.fraction, l.chapterLabel || null),
    )
  }
  function saveSoon() {
    clearTimeout(saveTimer)
    saveTimer = window.setTimeout(saveNow, SAVE_DEBOUNCE_MS)
  }

  // 1.1: reading sessions, heard by extensions with reading.sessions (Linen keeps none).
  const sessions = new ReadingSessions((s) =>
    extensions.emitReadingSession(s, {
      title: book.title,
      identifier: book.package_identifier ?? null,
    }),
  )

  function onRelocate(l: ReaderLocation) {
    const prev = location
    location = l
    if (active) sessions.moved(l.fraction, l.reason === 'page')
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
      // The new page fades in over 120 ms (a cross-fade needs a picture of the old page,
      // which a WebView cannot take cheaply); never with reduced motion (V9).
      if (crossfadeTurns && !matchMedia('(prefers-reduced-motion: reduce)').matches)
        host.animate([{ opacity: 0.25 }, { opacity: 1 }], {
          duration: MOTION.pageTurnCrossfade,
          easing: 'ease-out',
        })
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
    if (!active) return // S14: a warm book behind the library
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
    // A11: while caret browsing, the arrows move the caret (⇧ extends the selection).
    if (engine?.caretKey(e)) {
      e.preventDefault()
      return
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
    if (e.key === 'Tab' && !fromBook && (!chromeVisible || lanes.chromePeek)) {
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
  let topBar: HTMLElement | undefined = $state()
  let bottomBar: HTMLElement | undefined = $state()
  /** S11: the strip at the window's foot where an auto-hiding Dock slides in. */
  const DOCK_STRIP = 6
  /**
   * S11: in full screen the top zone starts below the menu bar. With the Dock hiding at the
   * bottom, the bottom zone leaves out the Dock's strip (owner decision, 2026-10-01).
   */
  async function refreshEdges() {
    try {
      const e = await ipc.screenEdges()
      edges = {
        topStart: e.fullscreen ? e.menu_bar_height : 0,
        dockBottom: e.dock_edge === 'bottom' && (e.dock_autohide || e.fullscreen),
      }
    } catch {
      // keep the defaults
    }
  }

  /** `y` is in the reader's coordinates. */
  function onPointerAt(y: number) {
    const nearTop = y >= edges.topStart && y <= edges.topStart + REVEAL_ZONE
    const bottomEnd = window.innerHeight - (edges.dockBottom ? DOCK_STRIP : 0)
    const nearBottom = y >= window.innerHeight - REVEAL_ZONE && y <= bottomEnd
    const edge = nearTop ? 'top' : nearBottom ? 'bottom' : null
    clearTimeout(hideTimer)
    if (lanes.chromePeek) {
      // S9: the bars the pointer revealed stay while the pointer is on one of them or in an
      // edge zone, and go the moment it leaves (no delay, so reading resumes at once).
      const on = (bar?: HTMLElement) => {
        const r = bar?.getBoundingClientRect()
        return !!r && y >= r.top && y <= r.bottom
      }
      if (edge || on(topBar) || on(bottomBar)) {
        clearTimeout(dwellTimer)
        dwellTimer = 0
        return
      }
      dispatch({ type: 'hideChrome' })
    }
    if (edge) {
      if (!chromeVisible && !dwellTimer)
        dwellTimer = window.setTimeout(() => {
          dwellTimer = 0
          dispatch({ type: 'showChrome', edge })
        }, REVEAL_DWELL_MS)
    } else {
      clearTimeout(dwellTimer)
      dwellTimer = 0
      // S10: the full controls (Tab, ⌘J, Aa) still wait 3 s after the pointer leaves an edge.
      if (chromeVisible && !lanes.chromePeek)
        hideTimer = window.setTimeout(() => dispatch({ type: 'hideChrome' }), HIDE_AFTER_MS)
    }
  }
  const onPointerMove = (e: PointerEvent) => onPointerAt(e.clientY)
  /** The book's frames swallow pointer moves; without these a bar would stay up over the text. */
  function onBookPointerMove(doc: Document) {
    doc.addEventListener('pointermove', (e) => {
      if (!lanes.chromePeek && !dwellTimer) return
      const frame = doc.defaultView?.frameElement?.getBoundingClientRect()
      if (!frame) return
      // Fixed layout scales the frame (I17); map the pointer through it.
      const scale = frame.height / (doc.defaultView?.innerHeight || frame.height || 1)
      onPointerAt(frame.top + e.clientY * scale)
    })
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
    // Screen 05: while Search is open, the line says which result is on the page.
    if (searchOpen && searchState.position && searchState.active)
      return t.search.result(
        searchState.position,
        searchState.count,
        chapterLabelFor(searchState.active.index),
      )
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
      fontPx = clampTextSize(Number((await ipc.settingGet('fontPx')) ?? DEFAULT_TEXT_SIZE))
      const savedSpacing = await ipc.settingGet('lineSpacing')
      if (savedSpacing === 'compact' || savedSpacing === 'loose') spacing = savedSpacing
      themeChoice = ((await ipc.settingGet('theme')) as ThemeChoice | null) ?? 'auto'
      fontChoice = parseFont(await ipc.settingGet(TYPE_SETTING.font))
      pageWidth = parsePageWidth(await ipc.settingGet(TYPE_SETTING.width))
      publisherStyles = parsePublisherStyles(await ipc.settingGet(TYPE_SETTING.publisher))
      simplified = (await ipc.settingGet(simplifyKey(book.id))) === 'on'
      if (fontChoice === 'dyslexic') extraFaces = await dyslexicFaces()
      announceTurns = (await ipc.settingGet('pageTurnAnnouncements')) !== 'off'
      crossfadeTurns = (await ipc.settingGet('pageTurnCrossfade')) === 'on'
      await refreshEdges()
      const savedPace = await ipc.settingGet('readingPace')
      if (savedPace) pace = new ReadingPace(JSON.parse(savedPace))
      fontFaces = await literataFaces()
      engine = new ReaderEngine(host)
      if (testHooks) {
        const e = engine
        testHooks.reader = hookReader = {
          engine: e,
          location: () => location,
          history,
          bookId: book.id,
          pagesExact: () => pages?.exact ?? false,
          search: searchState,
          annotations,
          undo,
          showControls: () => dispatch({ type: 'showChrome' }),
        }
      }
      relayout()
      cleanups.push(engine.onRelocate(onRelocate))
      cleanups.push(engine.onKey((e) => onPageKey(e, true)))
      // L15: a book font that fails or takes over 1.5 s falls back to Literata, without a prompt.
      cleanups.push(engine.onDocument((doc) => void fallBackFailedFonts(doc)))
      cleanups.push(engine.onDocument(panByDrag))
      cleanups.push(engine.onDocument(onBookPointerMove))
      cleanups.push(engine.onNote(onNote))
      cleanups.push(engine.onImage(onImage))
      cleanups.push(engine.onLinkHover((href) => (hoverUrl = href)))
      cleanups.push(engine.onSelection(onSelection))
      cleanups.push(engine.onHighlightClick(onHighlightClick))
      // A click in the text closes the peek, a clicked highlight's bar and the note
      // card (none is a modal, S8; A6: a click elsewhere closes the note).
      cleanups.push(
        engine.onDocument((doc) => {
          doc.addEventListener('pointerdown', (e) => {
            const kind = lanes.floating?.kind
            if (kind === 'peek' && (e.target as Element | null)?.closest?.('.linen-peek-marker'))
              return
            if (
              kind === 'peek' ||
              kind === 'note' ||
              (kind === 'selection' && bar?.mode === 'existing')
            )
              dispatch({ type: 'closeFloating' })
          })
          doc.addEventListener('contextmenu', (e) => void contextMenu(e))
        }),
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
      const opening = engine.open(await libraryLoader(book.id), {
        cfi: saved?.[0],
        mode: readingMode,
      })
      // B8 tests: relayouts that land while the book is still opening.
      for (const ms of testHooks?.relayoutDuringOpenMs ?? []) setTimeout(relayout, ms)
      const opened = await opening
      readingMode = engine.mode
      pageList = engine.pageList
      // Phase 7: the book, for extensions that were allowed to see it.
      bridgeReady = true
      cleanups.push(() => {
        bridgeReady = false
        if (extensions.bridge === bridge) extensions.bridge = null
      })
      // Not awaited: nothing in the opening sequence may wait on it (B8's restore is timing-sensitive).
      void ipc
        .settingGet('pinnedExtensions')
        .then((v) => (pinnedExtensions = JSON.parse(v ?? '[]')))
        .catch(() => {})
      // A4, A9: the book's highlights, placed again if the file changed (B3).
      void ipc
        .annotationsList(book.id)
        .then((rows) => {
          annotations.load(rows)
          annotationsReady = true
          return reanchorStale()
        })
        .catch((e) => console.error('annotations', e))
      // The cover extracted at import, served as an image (scripts cannot run in <img>).
      if (book.cover_path) coverUrl = bookCoverUrl(book.id, book.content_hash)
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
      fixedBook = engine.fixedLayout
      zoom = engine.zoom
      pages = new PageCounter(opened.sections.map((s) => (s.linear === 'no' ? 0 : s.size)))
      relayout()
      // N4: resuming deep in a book says where, with a way back to the beginning.
      if (saved && saved[1] > 0.02) {
        const label = engine.location?.chapterLabel
        const resumed = messages.push({
          text: label ? t.reader.resumedIn(label) : t.reader.resumed,
          action: { label: t.reader.goToBeginning, run: () => void engine?.goToTextStart() },
        })
        // Its action needs this reader: it goes when the book closes.
        jumpMessages.push(resumed.id)
      }
      engine.focusPage()
      // §6.4: the first page of the first book this session (cold start to last book).
      if (!startupMarked) {
        startupMarked = true
        requestAnimationFrame(() =>
          requestAnimationFrame(() => void ipc.startupMark('book').catch(() => {})),
        )
      }

      // Wheel and trackpad come from AppKit (Spike B): one turn per roll or gesture.
      const turns = new NativeTurns()
      cleanups.push(
        await listen<NativeScroll>('native-scroll', ({ payload }) => {
          if (!active) return // S14: the library is on top
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
      const srTimer = window.setInterval(
        () => active && engine?.watchExternalScroll(screenReader()),
        2000,
      )
      engine.watchExternalScroll(screenReader())
      cleanups.push(() => clearInterval(srTimer))

      cleanups.push(handle('history.back', { run: goBack, enabled: () => history.canGoBack }))
      cleanups.push(handle('chapter.next', { run: () => void engine?.nextSection() }))
      cleanups.push(
        handle('chapter.previous', {
          run: () => void engine?.prevSection(),
        }),
      )
      cleanups.push(handle('layer.close', { run: () => dispatch({ type: 'escape' }) }))
      cleanups.push(handle('navigator.contents', { run: openContents }))
      cleanups.push(handle('goto.open', { run: () => openGoTo() }))
      cleanups.push(handle('search.open', { run: openSearch }))
      // A3, K6: H and N (and ⇧⌘H, ⇧⌘N) on the selection or a clicked highlight.
      const onSelectionOrHighlight = () => !!selection || bar?.mode === 'existing'
      cleanups.push(
        handle('selection.highlight', {
          run: () => {
            if (selection) highlightSelection(annotations.lastColor)
            else if (bar?.id) recolor(bar.id, annotations.lastColor)
          },
          enabled: onSelectionOrHighlight,
        }),
      )
      cleanups.push(
        handle('selection.note', {
          run: () => (selection ? noteOnSelection() : bar?.id && openNote(bar.id)),
          enabled: onSelectionOrHighlight,
        }),
      )
      // A2, K7: F6 moves focus into the bar.
      cleanups.push(
        handle('selection.focusBar', {
          run: () => selectionBar?.focusFirst(),
          enabled: () => barOpen,
        }),
      )
      cleanups.push(handle('navigator.notes', { run: openNotes }))
      // A11, K7: F7 caret browsing (T3: our own caret; WebKit has none).
      cleanups.push(
        handle('reader.caretBrowsing', {
          run: () => {
            const on = !engine?.caretBrowsing
            engine?.setCaretBrowsing(on ? theme.ink : null)
            announce(on ? t.reader.caretOn : t.reader.caretOff)
          },
        }),
      )
      // K13, B5: ⌘Z undoes a deletion or a colour change; in a text field it is the field's own undo.
      const inTextField = () => {
        const el = document.activeElement
        return el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement
      }
      cleanups.push(
        handle('edit.undo', {
          run: () => (inTextField() ? document.execCommand('undo') : undo.undo()),
          enabled: () => inTextField() || undo.canUndo,
        }),
      )
      const hasResults = () => searchState.count > 0
      cleanups.push(handle('search.next', { run: () => stepResult(1), enabled: hasResults }))
      cleanups.push(handle('search.previous', { run: () => stepResult(-1), enabled: hasResults }))
      // B8: Pages and Scroll, from the commands and the Aa popover (this book only).
      for (const mode of ['pages', 'scroll'] as const)
        cleanups.push(
          handle(`layout.${mode}`, {
            run: () => void switchMode(mode),
            enabled: () => readingMode !== mode && engine?.fixedLayout !== true,
          }),
        )
      // K8: ⌘+ ⌘− ⌘0 change the text size (all books). A fixed-layout book zooms its
      // page instead (I17, E2), and the image view zooms the image (N11).
      const zooms = () => engine?.fixedLayout === true || imageOpen
      cleanups.push(
        handle('text.larger', {
          run: () =>
            imageOpen
              ? imageView?.step(1)
              : zooms()
                ? zoomStep(1)
                : setFont(stepTextSize(fontPx, 1)),
        }),
      )
      cleanups.push(
        handle('text.smaller', {
          run: () =>
            imageOpen
              ? imageView?.step(-1)
              : zooms()
                ? zoomStep(-1)
                : setFont(stepTextSize(fontPx, -1)),
        }),
      )
      cleanups.push(
        handle('text.reset', {
          run: () =>
            imageOpen ? (imageZoom = 1) : zooms() ? setZoom(1) : setFont(DEFAULT_TEXT_SIZE),
        }),
      )
      cleanups.push(handle('reader.settings', { run: openAa }))
      cleanups.push(
        handle('selection.lookUp', { run: lookUpSelection, enabled: () => selection !== null }),
      )
      cleanups.push(
        handle('reader.simplifyStyles', { run: toggleSimplified, enabled: () => !fixedBook }),
      )
      // G2: changes made in the Settings window apply here at once.
      cleanups.push(
        await onSettingChanged(({ key, value }) => {
          if (key === 'fontPx') setFont(Number(value))
          else if (
            key === 'lineSpacing' &&
            (value === 'compact' || value === 'default' || value === 'loose')
          )
            setSpacing(value)
          else if (key === 'theme' && value !== themeChoice) void setTheme(value as ThemeChoice)
          else if (key === 'pageTurnAnnouncements') announceTurns = value !== 'off'
          else if (key === TYPE_SETTING.font) void setFontChoice(parseFont(value))
          else if (key === TYPE_SETTING.width) setPageWidth(parsePageWidth(value))
          else if (key === TYPE_SETTING.publisher) setPublisherStyles(parsePublisherStyles(value))
          else if (key === 'pageTurnCrossfade') crossfadeTurns = value === 'on'
        }),
      )
      // L18: how much of the book is code or tables, measured once in idle time.
      const shareKey = `codeShare:${book.id}`
      const known = await ipc.settingGet(shareKey)
      if (known !== null) codeHeavy = Number(known) > CODE_HEAVY_SHARE
      else if (!engine.fixedLayout)
        // After the book has settled: measuring parses every chapter.
        void new Promise((r) => setTimeout(r, CODE_SHARE_DELAY_MS))
          .then(() => (unmounted || !engine ? null : engine.codeShare()))
          .then((share) => {
            // A book closed meanwhile is measured next time it opens.
            if (share === null || Number.isNaN(share) || unmounted) return
            codeHeavy = share > CODE_HEAVY_SHARE
            void ipc.settingSet(shareKey, share.toFixed(3))
          })
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
    // V1: Auto follows the system between Paper and Night.
    const scheme = matchMedia('(prefers-color-scheme: dark)')
    const onScheme = () => {
      if (themeChoice !== 'auto') return
      void resolveTheme().then((th) => {
        theme = th
        relayout()
      })
    }
    scheme.addEventListener('change', onScheme)
    const onBlur = () => saveNow()
    const onFocusEdges = () => void refreshEdges()
    window.addEventListener('focus', onFocusEdges)
    const offQuit = onBeforeQuit(() => {
      noteCard?.flush() // N5, A6: a note typed just before quitting is saved
      saveNow()
      sessions.end()
      void ipc.settingSet('readingPace', JSON.stringify(pace.toJSON()))
    })
    const onKeydown = (e: KeyboardEvent) => onPageKey(e, false)
    window.addEventListener('resize', debouncedResize)
    window.addEventListener('blur', onBlur)
    window.addEventListener('keydown', onKeydown)
    return () => {
      unmounted = true
      window.removeEventListener('resize', debouncedResize)
      window.removeEventListener('blur', onBlur)
      window.removeEventListener('keydown', onKeydown)
      window.removeEventListener('focus', onFocusEdges)
      scheme.removeEventListener('change', onScheme)
      offQuit()
      cleanups.forEach((c) => c())
      saveNow()
      sessions.end()
      void ipc.settingSet('readingPace', JSON.stringify(pace.toJSON()))
      stopCounting()
      searchState.stop()
      bookSearch?.close()
      engine?.close()
      for (const id of jumpMessages) messages.withdraw(id)
      for (const id of annotationMessages) messages.withdraw(id)
      if (testHooks && testHooks.reader === hookReader) testHooks.reader = undefined
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
      {coverUrl}
      coverTint={book.generated_cover_tint}
      fraction={location?.fraction ?? 0}
      {contents}
      current={currentRow}
      floating={!lanes.docked}
      onselect={onContentsSelect}
      onclose={closeNavigator}
      onlibrary={leave}
      ongoto={openGoTo}
      tab={searchOpen ? 'search' : notesOpen ? 'notes' : (navigatorTab(lanes) ?? 'contents')}
      extensionTabs={extensions
        .navigatorTabs()
        .map((x) => ({ value: `extension:${x.extId}/${x.id}`, label: x.title }))}
      ontab={(tab) =>
        tab === 'search'
          ? openSearch()
          : tab === 'notes'
            ? openNotes()
            : dispatch({ type: 'openNavigator', tab: tab as NavigatorTab })}
    >
      {#snippet extension()}
        {#if extensionTab}
          {#key extensionTab.extId + extensionTab.id}
            <ExtensionTab
              host={extensions}
              extId={extensionTab.extId}
              name={extensionTab.name}
              title={extensionTab.title}
              page={extensionTab.page}
              {theme}
            />
          {/key}
        {/if}
      {/snippet}
      {#snippet notes()}
        <NotesPanel
          bind:this={notesPanel}
          placed={annotations.placed}
          unplaced={annotations.unplaced}
          {chapterOf}
          chapterLabel={chapterLabelFor}
          order={cfiOrder}
          current={jumpedTo}
          onchoose={(a) => void chooseAnnotation(a)}
          onreattach={startReattach}
          exporters={extensions.exporters()}
          onexport={(e) => void runExporter(e)}
        />
      {/snippet}
      {#snippet search()}
        <SearchPanel
          bind:this={searchPanel}
          search={searchState}
          chapterLabel={chapterLabelFor}
          currentSection={location?.sectionIndex ?? -1}
          onbrowse={browseHit}
          onchoose={chooseHit}
        />
      {/snippet}
    </Navigator>
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
        pinned={pinnedExtensions}
        anchor={moreAnchor}
        onclose={() => dispatch({ type: 'closeFloating' })}
      />
    {/if}
    {#if peekOpen && note}
      <FootnotePeek bind:this={peek} {note} onopen={openNoteInPlace} oncopy={copyNote} />
    {/if}
    {#if lookupOpen && lookup}
      <LookUpPeek
        word={lookup.word}
        definition={lookup.definition}
        rect={lookup.rect}
        onopen={openInDictionary}
        onsearch={() => {
          const word = lookup?.word ?? ''
          dispatch({ type: 'closeFloating' })
          searchFor(word)
        }}
      />
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
    {#if aaOpen && aaAnchor}
      <AaPopover
        packs={themePacks}
        anchor={aaAnchor}
        {fontPx}
        theme={themeChoice}
        {spacing}
        layout={readingMode}
        fixedLayout={fixedBook}
        {codeHeavy}
        onfont={setFont}
        ontheme={(c) => void setTheme(c)}
        onspacing={setSpacing}
        font={fontChoice}
        width={pageWidth}
        onfontchoice={(f) => void setFontChoice(f)}
        onwidth={setPageWidth}
        onlayout={(m) => void switchMode(m)}
        onsettings={() => {
          dispatch({ type: 'closeFloating' })
          registry.run('app.settings')
        }}
      />
    {/if}
    {#if barOpen && bar}
      {@const existing = bar.id ? annotations.get(bar.id) : undefined}
      <SelectionBar
        bind:this={selectionBar}
        first={bar.first}
        last={bar.last}
        top={chromeVisible ? 52 : 0}
        mode={bar.mode}
        color={existing?.color ?? annotations.lastColor}
        onhighlight={(c) => (bar?.id ? recolor(bar.id, c) : highlightSelection(c))}
        onnote={() => (bar?.id ? openNote(bar.id) : noteOnSelection())}
        oncopy={() => {
          copyText(existing ? existing.quote.exact : (selection?.text ?? ''))
          closeBar(false)
        }}
        onsearch={() => searchFor(selection?.text ?? '')}
        onlookup={lookUpSelection}
        ondelete={() => bar?.id && deleteAnnotation(bar.id)}
        onescape={() => engine?.focusPage()}
        onattach={attachSelection}
        oncancel={cancelReattach}
        extensionActions={selectionExtensionActions.map((a) => ({
          ...a,
          status: extensions.status[a.extId] ?? 'idle',
        }))}
        onextension={(a) => void runSelectionAction(a)}
        onrestart={(id) => void extensions.restart(id)}
        onmanage={() => {
          closeBar(false)
          registry.run('app.settings')
        }}
      />
    {/if}
    {#if noteOpen && noteFor}
      {@const a = annotations.get(noteFor)}
      {#if a}
        <NoteCard
          bind:this={noteCard}
          annotation={a}
          mode={noteMode}
          place={notePlace}
          onsave={(text) => annotations.setNote(a.id, text)}
          oncolor={(c) => recolor(a.id, c)}
          oncopy={() => copyText(a.note ? `${a.quote.exact}\n\n${a.note}` : a.quote.exact)}
          ondelete={() => deleteAnnotation(a.id)}
          onclose={closeNote}
        />
      {/if}
    {/if}
    {#if reattaching}
      <!-- G11 (provisional): what Re-attach is waiting for. -->
      <div class="reattach" role="status">
        <span>{t.annotations.reattachPrompt(shortQuote(reattaching.quote.exact))}</span>
        <button type="button" onclick={cancelReattach}>{t.annotations.cancel}</button>
      </div>
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
        bind:this={topBar}
        data-tauri-drag-region
        out:chromeOut={{ from: -4 }}
      >
        <button type="button" class="library" onclick={leave}>
          <Icon name="library" size={18} />{t.reader.library}
        </button>
        <button type="button" class="tool" aria-label={t.reader.contents} onclick={openContents}>
          <Icon name="contents" size={18} />
        </button>
        <div class="title" aria-live="off">
          <!-- S9: bars the pointer revealed name the book only. -->
          <span class="book">{book.title}</span
          >{#if location?.chapterLabel && !lanes.chromePeek}&nbsp;·
            {location.chapterLabel}{/if}
        </div>
        <!-- Screen 03: Search · Notes · Aa · ⋯ -->
        <div class="tools">
          <button type="button" class="tool" aria-label={t.search.label} onclick={openSearch}>
            <Icon name="search" size={18} />
          </button>
          <button
            type="button"
            class="tool"
            aria-label={t.commands['navigator.notes']}
            onclick={openNotes}
          >
            <Icon name="highlights" size={18} />
          </button>
          <button
            type="button"
            class="tool aa-button"
            class:open={aaOpen}
            bind:this={aaButton}
            aria-label={t.aa.button}
            aria-haspopup="dialog"
            aria-expanded={aaOpen}
            onclick={openAa}>Aa</button
          >
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
      <footer class="chrome bottom" bind:this={bottomBar} out:chromeOut={{ from: 4 }}>
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
                >{[
                  progressText,
                  // G12: a fixed-layout book's real pages, here now that the location line is gone.
                  location?.fixedPages?.length ? locationText : locationText.split(' · ')[1],
                ]
                  .filter(Boolean)
                  .join(' · ')}</span
              >
              <span class="hint">{progressText} · {t.goto.open} ⌘J</span>
            </button>
          </div>
        </div>
      </footer>
    {:else if openingShown && !location}
      <!-- G8: the slow-open notice. Immersive reading shows no location line (owner
           decision, 2026-09-29): the chapter, time left and pages are in the bottom bar. -->
      <div class="location-line">{t.reader.opening(book.title)}</div>
    {:else if searchOpen && searchState.position && searchState.active && showLocationLine(height)}
      <!-- Screen 05: while Search is open, the line says which result is on the page. -->
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
  /* Screen 09: “Aa” in the reading face; accent on a 10% accent wash while open. */
  .aa-button {
    font: 500 16px var(--font-reading, Literata, Georgia, serif);
  }
  .aa-button.open {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    color: var(--accent);
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
  .reattach {
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
    background: var(--ink);
    color: var(--ground);
    box-shadow: var(--shadow-popover);
    font: 500 var(--text-body) var(--font-ui);
    white-space: nowrap;
  }
  .reattach button {
    min-height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: 18px;
    background: none;
    color: inherit;
    font-weight: 600;
  }
  .reattach button:focus-visible {
    outline: 2px solid var(--accent);
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
