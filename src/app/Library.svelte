<script lang="ts">
  // The library (E6–E9; Screen 01): Continue reading (the current book large, the
  // next two small), then All books as a cover grid with its count. Search by
  // title and author; sort by Recent, Title or Author. Each book has an item menu
  // (Book info, Show in Finder, Remove; E7); Remove is immediate with Undo (G4,
  // provisional). Return opens the current book (E8). Empty: E9, Screen 12.
  // 1.1 (approved 2026-10-01): All books as covers or as a list, remembered (P§10).
  import { onMount } from 'svelte'
  import Button from '../components/Button.svelte'
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings/en'
  import { ipc, type Book } from './ipc'
  import BookCover from './BookCover.svelte'
  import BookInfo from './BookInfo.svelte'
  import DamagedBook from './DamagedBook.svelte'
  import { popUpMenu } from './nativeMenu'
  import { searchAndSort, type SortKey } from '../lib/library/order'
  import { openedAgo, openedLine } from '../lib/library/when'
  import type { MessageQueue } from '../lib/reader/messages'
  import type { CommandRegistry } from '../lib/commands/registry'
  import { UndoStack } from '../reader/annotations.svelte'

  let {
    books,
    dropActive,
    messages,
    registry,
    onopen,
    onopenbook,
    onchanged,
  }: {
    books: Book[]
    dropActive: boolean
    messages: MessageQueue
    registry: CommandRegistry
    onopen: () => void
    /** `cover`: the cover the book was opened from, for the cover-grow transition (V8). */
    onopenbook: (book: Book, cover?: HTMLElement | null) => void
    /** The library changed in the core (a book removed or restored). */
    onchanged: () => Promise<void>
  } = $props()

  let query = $state('')
  let sort = $state<SortKey>('recent')
  let view = $state<'grid' | 'list'>('grid')
  function setView(next: 'grid' | 'list') {
    view = next
    void ipc.settingSet('libraryView', next)
  }
  let infoFor = $state<Book | null>(null)
  let damagedFor = $state<Book | null>(null)

  /** E3: a book with damaged chapters shows the card first, until Read anyway. */
  async function open(book: Book, from?: EventTarget | null) {
    if (book.damaged_items > 0 && (await ipc.settingGet(`damageAck:${book.id}`)) !== '1') {
      damagedFor = book
      return
    }
    onopenbook(book, coverOf(book, from))
  }
  /** The cover clicked, or else the book's first cover on screen. */
  function coverOf(book: Book, from?: EventTarget | null): HTMLElement | null {
    const inTile = from instanceof HTMLElement ? from.querySelector<HTMLElement>('.cover') : null
    return inTile ?? document.querySelector<HTMLElement>(`.library [data-cover="${book.id}"]`)
  }
  function readAnyway(book: Book) {
    damagedFor = null
    void ipc.settingSet(`damageAck:${book.id}`, '1')
    onopenbook(book, coverOf(book))
  }
  let now = $state(Date.now())
  const undo = new UndoStack()

  const shown = $derived(searchAndSort(books, query, sort))
  /** E6: books opened before, most recent first; the first is the current book. */
  const started = $derived(
    books
      .filter((b) => b.opened_at !== null && !b.finished_at)
      .sort((a, b) => (b.opened_at ?? 0) - (a.opened_at ?? 0)),
  )
  const current = $derived(started[0] ?? null)
  const next = $derived(started.slice(1, 3))

  const authorOf = (b: Book) => b.authors.join(', ') || t.library.unknownAuthor
  const percent = (b: Book) => Math.round((b.fraction ?? 0) * 100)
  const smallLine = (b: Book) =>
    [authorOf(b), `${percent(b)}%`, b.opened_at ? openedAgo(b.opened_at, now) : null]
      .filter(Boolean)
      .join(' · ')

  async function remove(book: Book) {
    await ipc.libraryRemove(book.id)
    await onchanged()
    let id = 0
    const run = undo.push(() => {
      void ipc
        .libraryRestore(book.id)
        .then(onchanged)
        .then(() => messages.withdraw(id))
    })
    id = messages.push({
      text: t.library.removed(book.title),
      closedLabel: t.library.restore(book.title),
      action: { label: t.annotations.undo, shortcut: '⌘Z', run },
    }).id
  }

  function itemMenu(book: Book) {
    void popUpMenu([
      { label: t.library.info, run: () => (infoFor = book) },
      { label: t.library.showFile, run: () => void ipc.bookShowFile(book.id) },
      null,
      { label: t.library.remove, run: () => void remove(book) },
    ])
  }

  function sortMenu() {
    void popUpMenu(
      (['recent', 'title', 'author'] as const).map((k) => ({
        label: t.library.sorts[k],
        checked: sort === k,
        run: () => {
          sort = k
          void ipc.settingSet('librarySort', k)
        },
      })),
    )
  }

  onMount(() => {
    void ipc.settingGet('librarySort').then((s) => {
      if (s === 'title' || s === 'author' || s === 'recent') sort = s
    })
    void ipc.settingGet('libraryView').then((v) => {
      if (v === 'list') view = 'list'
    })
    const clock = window.setInterval(() => (now = Date.now()), 60_000)
    // E8: Return opens the current book (“Resume reading” is the default button).
    const onkeydown = (e: KeyboardEvent) => {
      if (e.key !== 'Enter' || e.metaKey || e.altKey || e.ctrlKey || e.shiftKey) return
      const target = e.target as Element | null
      if (target && target !== document.body && target.closest('button, input, a, [tabindex]'))
        return
      if (!current || infoFor || damagedFor) return
      e.preventDefault()
      void open(current)
    }
    window.addEventListener('keydown', onkeydown)
    const offUndo = registry.handle('edit.undo', {
      run: () => {
        const el = document.activeElement
        if (el instanceof HTMLInputElement) document.execCommand('undo')
        else undo.undo()
      },
      enabled: () => undo.canUndo || document.activeElement instanceof HTMLInputElement,
    })
    return () => {
      clearInterval(clock)
      window.removeEventListener('keydown', onkeydown)
      offUndo()
    }
  })
</script>

<div class="titlebar" data-tauri-drag-region></div>
<main class="library" class:drop={dropActive}>
  {#if books.length === 0}
    <!-- E9, Screen 12 -->
    <section class="empty" aria-labelledby="empty-title">
      <h1 id="empty-title">{t.library.emptyTitle}</h1>
      <p>{t.library.emptyBody}</p>
      <Button variant="primary" shortcut="⌘O" onclick={onopen}>{t.library.openBook}</Button>
      <p class="note">{t.library.note}</p>
    </section>
  {:else}
    <header data-tauri-drag-region>
      <h1>{t.library.title}</h1>
      <div class="grow" data-tauri-drag-region></div>
      <div class="search">
        <Icon name="search" size={16} />
        <label class="visually-hidden" for="lib-search">{t.library.search}</label>
        <input
          id="lib-search"
          type="search"
          placeholder={t.library.searchPlaceholder}
          bind:value={query}
          onkeydown={(e) => {
            e.stopPropagation()
            if (e.key === 'Escape') query = ''
          }}
        />
      </div>
      <button
        type="button"
        class="btn sort"
        aria-label={t.library.sortBy(t.library.sorts[sort])}
        aria-haspopup="menu"
        onclick={sortMenu}>{t.library.sorts[sort]}<Icon name="chevron-down" size={14} /></button
      >
      <div class="views" role="radiogroup" aria-label={t.library.view}>
        {#each [['grid', t.library.viewGrid], ['list', t.library.viewList]] as const as [v, label] (v)}
          <button
            type="button"
            role="radio"
            class="view-btn"
            aria-checked={view === v}
            aria-label={label}
            title={label}
            onclick={() => setView(v)}><Icon name={v} size={16} /></button
          >
        {/each}
      </div>
      <button type="button" class="btn" onclick={onopen}
        ><Icon name="plus" size={16} />{t.library.open}</button
      >
    </header>

    {#if current && !query}
      <section class="continue" aria-labelledby="cr">
        <h2 id="cr">{t.library.continueReading}</h2>
        <div class="cr">
          <div class="current">
            <button
              type="button"
              class="cover-button"
              tabindex="-1"
              aria-hidden="true"
              onclick={(e) => void open(current, e.currentTarget)}
            >
              <BookCover book={current} width={112} height={168} />
            </button>
            <div class="current-meta">
              <div class="big-title">{current.title}</div>
              <div class="author">{authorOf(current)}</div>
              <div class="where">
                <div class="bar" style:width="200px">
                  <i style:width="{percent(current)}%"></i>
                </div>
                <span
                  >{[current.chapter_label, `${percent(current)}%`]
                    .filter(Boolean)
                    .join(' · ')}</span
                >
              </div>
              <div class="resume-row">
                <button
                  type="button"
                  class="resume"
                  onclick={(e) => void open(current, e.currentTarget)}>{t.library.resume}</button
                >
                {#if current.opened_at}<span class="opened"
                    >{openedLine(current.opened_at, now)}</span
                  >{/if}
              </div>
            </div>
          </div>
          {#if next.length}
            <div class="next">
              {#each next as b (b.id)}
                <button
                  type="button"
                  class="small"
                  onclick={(e) => void open(b, e.currentTarget)}
                  oncontextmenu={(e) => {
                    e.preventDefault()
                    itemMenu(b)
                  }}
                >
                  <BookCover book={b} width={44} height={66} lettering={false} />
                  <span>
                    <span class="small-title">{b.title}</span>
                    <span class="small-line">{smallLine(b)}</span>
                  </span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </section>
    {/if}

    <section aria-labelledby="ab">
      <div class="all-head">
        <h2 id="ab">{t.library.allBooks}</h2>
        <span class="count">{shown.length}</span>
      </div>
      {#if !shown.length}
        <p class="no-matches">
          {t.library.noMatches(query)}
          <button type="button" class="btn" onclick={() => (query = '')}>{t.library.clear}</button>
        </p>
      {:else}
        <ul class="grid" class:list={view === 'list'}>
          {#each shown as b (b.id)}
            <li class="tile" data-book={b.id}>
              <button
                type="button"
                class="open"
                onclick={(e) => void open(b, e.currentTarget)}
                oncontextmenu={(e) => {
                  e.preventDefault()
                  itemMenu(b)
                }}
              >
                {#if view === 'list'}
                  <BookCover book={b} width={36} height={54} lettering={false} />
                {:else}
                  <BookCover book={b} height={198} />
                {/if}
                <span class="tt">{b.title}</span>
                <span class="ta">{authorOf(b)}</span>
                <span class="tp">
                  {#if b.finished_at}
                    <span>{t.library.finished}</span>
                  {:else if b.fraction === null}
                    <span class="new">{t.library.new}</span>
                  {:else}
                    <span class="bar"><i style:width="{percent(b)}%"></i></span>
                    <span>{percent(b)}%</span>
                  {/if}
                  {#if b.damaged_items > 0}
                    <span class="damaged">{t.library.damaged(b.damaged_items)}</span>
                  {/if}
                </span>
                {#if view === 'list'}
                  <span class="when"
                    >{b.opened_at ? openedAgo(b.opened_at, now) : t.library.neverOpened}</span
                  >
                {/if}
              </button>
              <button
                type="button"
                class="more"
                aria-label={t.library.itemMenu(b.title)}
                aria-haspopup="menu"
                onclick={() => itemMenu(b)}><Icon name="more" size={18} /></button
              >
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</main>
<BookInfo book={infoFor} onclose={() => (infoFor = null)} />
<DamagedBook
  book={damagedFor}
  onread={readAnyway}
  onremove={(b) => {
    damagedFor = null
    void remove(b)
  }}
  onclose={() => (damagedFor = null)}
/>

<style>
  /* The window's title bar is an overlay; this strip keeps it draggable. */
  .titlebar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 28px;
    z-index: 5;
  }
  .library {
    min-height: 100vh;
    box-sizing: border-box;
    /* The empty state's top margin stays inside (no scrollbar on an empty library). */
    display: flow-root;
    padding: 0 48px 48px;
    font-family: var(--font-ui);
    color: var(--ink);
  }
  .drop {
    outline: 2px dashed var(--accent);
    outline-offset: -12px;
  }
  .empty {
    display: grid;
    justify-items: center;
    gap: 8px;
    max-width: 420px;
    margin: 18vh auto 0;
    text-align: center;
  }
  .empty h1 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
  }
  .empty p {
    margin: 0;
  }
  .note {
    margin-top: 8px;
    font-size: var(--text-caption);
    color: var(--ink-secondary);
  }
  /* Screen 01: a 64 px header; the title clears the window buttons. */
  header {
    height: 64px;
    margin: 0 -16px 12px 56px;
    display: flex;
    align-items: center;
    gap: 12px;
  }
  header h1 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }
  .grow {
    flex-grow: 1;
    align-self: stretch;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    color: var(--ink-secondary);
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
  }
  .search input {
    width: 260px;
    height: 32px;
    box-sizing: border-box;
    padding: 0 12px 0 34px;
    border: 0;
    border-radius: 8px;
    background: var(--control-track);
    font: 13px var(--font-ui);
    color: var(--ink);
  }
  .search input::placeholder {
    color: var(--ink-secondary);
    opacity: 1;
  }
  .btn {
    height: 32px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 12px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--ink);
    font: 500 13px var(--font-ui);
  }
  .btn:hover {
    background: var(--hover-wash);
  }
  h2 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--ink-secondary);
  }
  .continue {
    margin-bottom: 40px;
  }
  .continue h2 {
    margin-bottom: 16px;
  }
  .cr {
    display: flex;
    gap: 40px;
    align-items: stretch;
  }
  .current {
    display: flex;
    gap: 28px;
    width: 660px;
    max-width: 60%;
    flex-shrink: 0;
  }
  .cover-button {
    border: 0;
    padding: 0;
    background: none;
  }
  .current-meta {
    display: flex;
    flex-direction: column;
    gap: 6px;
    justify-content: center;
    min-width: 0;
  }
  .big-title {
    font: 500 26px/1.2 var(--font-reading, Literata, Georgia, serif);
  }
  .author {
    font-size: 14px;
    color: var(--ink-secondary);
  }
  .where {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 10px;
    font-size: 13px;
    color: var(--ink);
  }
  .resume-row {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 14px;
  }
  .resume {
    height: 36px;
    padding: 0 18px;
    border: 0;
    border-radius: 8px;
    background: var(--accent);
    color: var(--on-accent);
    font: 600 14px var(--font-ui);
  }
  .opened {
    font-size: 12.5px;
    color: var(--ink-secondary);
  }
  .next {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    justify-content: center;
    padding-left: 40px;
    border-left: 1px solid var(--hairline);
  }
  .small {
    display: flex;
    gap: 14px;
    align-items: center;
    border: 0;
    padding: 0;
    background: none;
    text-align: left;
    color: var(--ink);
    border-radius: 6px;
    min-width: 0;
  }
  .small > span {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .small-title {
    font-size: 14px;
    font-weight: 500;
  }
  .small-line {
    font-size: 12.5px;
    color: var(--ink-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .all-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 16px;
  }
  .count {
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .no-matches {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--ink-secondary);
  }
  .grid {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(142px, 1fr));
    gap: 36px 32px;
  }
  .tile {
    position: relative;
    min-width: 0;
    /* §6.4 cold start: tiles below the fold are laid out and painted only when near. */
    content-visibility: auto;
    contain-intrinsic-size: auto 290px;
  }
  .open {
    display: flex;
    flex-direction: column;
    gap: 3px;
    width: 100%;
    border: 0;
    padding: 0;
    background: none;
    text-align: left;
    color: var(--ink);
    border-radius: 6px;
  }
  .tt,
  .ta {
    width: 100%;
  }
  .tt {
    font-size: 13.5px;
    font-weight: 500;
    line-height: 1.3;
    margin-top: 10px;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .ta {
    font-size: 12.5px;
    color: var(--ink-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tp {
    /* WebKit does not stretch a button's flex children; the bar needs the full width. */
    width: 100%;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--ink-secondary);
    margin-top: 4px;
    min-height: 14px;
  }
  .bar {
    display: block;
    flex-grow: 1;
    height: 3px;
    background: var(--hairline);
    border-radius: 2px;
    overflow: hidden;
  }
  .where .bar {
    flex-grow: 0;
  }
  .bar i {
    display: block;
    height: 3px;
    background: var(--accent);
  }
  .new {
    color: var(--accent);
    font-weight: 600;
  }
  .damaged {
    color: var(--accent);
  }
  /* 1.1 (approved 2026-10-01): the view switch, and All books as a list. */
  .views {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: 9px;
    background: var(--control-track);
  }
  .view-btn {
    width: 30px;
    height: 28px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--track-ink);
  }
  .view-btn[aria-checked='true'] {
    background: var(--raised);
    color: var(--ink);
    box-shadow: 0 0 0 1px var(--segment-ring);
  }
  .view-btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .grid.list {
    display: block;
  }
  .list .tile {
    border-bottom: 1px solid var(--hairline);
    contain-intrinsic-size: auto 71px;
  }
  .list .open {
    display: grid;
    grid-template-columns: 36px minmax(0, 1fr) 180px 120px;
    grid-template-rows: auto auto;
    column-gap: 16px;
    row-gap: 2px;
    align-items: center;
    padding: 8px 48px 8px 8px;
    box-sizing: border-box;
  }
  .list .open :global(.cover) {
    grid-row: 1 / 3;
  }
  .list .tt {
    grid-column: 2;
    margin-top: 0;
    -webkit-line-clamp: 1;
    line-clamp: 1;
    align-self: end;
  }
  .list .ta {
    grid-column: 2;
    align-self: start;
  }
  .list .tp {
    grid-column: 3;
    grid-row: 1 / 3;
    margin-top: 0;
  }
  .when {
    grid-column: 4;
    grid-row: 1 / 3;
    font-size: 12px;
    color: var(--ink-secondary);
    text-align: right;
  }
  .list .open:hover {
    background: var(--hover-wash);
  }
  .list .more {
    top: 50%;
    transform: translateY(-50%);
    box-shadow: none;
    background: transparent;
  }
  /* E7: the item menu's button shows on hover and focus. */
  .more {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 8px;
    background: var(--popover);
    color: var(--ink);
    box-shadow: 0 1px 3px rgba(40, 30, 20, 0.2);
    opacity: 0;
  }
  .tile:hover .more,
  .tile:focus-within .more {
    opacity: 1;
  }
  .btn:focus-visible,
  .open:focus-visible,
  .small:focus-visible,
  .resume:focus-visible,
  .more:focus-visible,
  .search input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  /* X6: at 200–400% zoom (or a narrow window) the library takes its narrow form:
     the header wraps, Continue reading stacks, and nothing scrolls sideways. */
  @media (max-width: 720px) {
    .library {
      padding: 0 16px 32px;
    }
    header {
      height: auto;
      min-height: 64px;
      flex-wrap: wrap;
      margin: 0 0 12px 56px;
      padding: 12px 0;
    }
    .grow {
      display: none;
    }
    .search {
      flex: 1 1 100%;
      order: 3;
    }
    .search input {
      width: 100%;
    }
    .cr,
    .current {
      flex-direction: column;
      gap: 16px;
    }
    .current {
      width: auto;
      max-width: 100%;
    }
    .next {
      padding-left: 0;
      border-left: 0;
    }
    .where .bar {
      width: 120px !important;
    }
    .grid {
      grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
      gap: 24px 16px;
    }
    .list .open {
      grid-template-columns: 36px minmax(0, 1fr) 96px;
    }
    .when {
      display: none;
    }
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
