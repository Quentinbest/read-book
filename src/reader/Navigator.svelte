<script lang="ts">
  // The Navigator (S2, S3, N6; Screen 04): Library, the book and its progress in
  // the header, then Contents, Search (F1–F7) and Notes (A8). It docks from
  // 1100 px and floats below.
  import type { Snippet } from 'svelte'
  import Icon from '../components/Icon.svelte'
  import Tabs from '../components/Tabs.svelte'
  import { t } from '../lib/strings/en'
  import type { Contents, ContentsItem } from './contents'

  let {
    title,
    author,
    coverUrl,
    coverTint,
    fraction,
    contents,
    current,
    floating,
    onselect,
    onclose,
    onlibrary,
    ongoto,
    tab,
    ontab,
    search,
    notes,
    extensionTabs = [],
    extension,
  }: {
    title: string
    author: string
    coverUrl: string | null
    coverTint: string | null
    fraction: number
    contents: Contents | null
    current: number
    floating: boolean
    onselect: (item: ContentsItem) => void
    onclose: () => void
    onlibrary: () => void
    /** N8: the progress label opens Go to (S3: the header stands in for the chrome). */
    ongoto: (anchor: DOMRect) => void
    /** The open tab. */
    tab: string
    ontab: (tab: string) => void
    /** The Search tab's panel (F1–F7). */
    search: Snippet
    /** The Notes tab's panel (A8). */
    notes: Snippet
    /** G7: extensions' tabs (`extension:<id>/<tab>`), after Notes. */
    extensionTabs?: { value: string; label: string }[]
    /** The panel of the extension tab that shows. */
    extension?: Snippet
  } = $props()

  let rows: HTMLButtonElement[] = $state([])
  const MORE = 'extension:more'
  let coverFailed = $state(false)

  /** K3, C2: Contents opens focused on the current chapter. */
  export function focusCurrent() {
    const row = rows[Math.max(0, current)]
    row?.scrollIntoView({ block: 'center' })
    row?.focus()
  }

  /** ↑ ↓ Home End move between rows; the list is one tab stop (the current row). */
  function onkeydown(e: KeyboardEvent, i: number) {
    const last = (contents?.items.length ?? 1) - 1
    const next =
      e.key === 'ArrowDown'
        ? Math.min(last, i + 1)
        : e.key === 'ArrowUp'
          ? Math.max(0, i - 1)
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? last
              : -1
    if (next < 0) return
    e.preventDefault()
    e.stopPropagation()
    rows[next]?.focus()
  }
</script>

<aside class="navigator" class:floating aria-label={t.navigator.label}>
  <header class="head" data-tauri-drag-region>
    <button type="button" class="library" onclick={onlibrary}>
      <Icon name="library" size={18} />{t.reader.library}
    </button>
    <button type="button" class="close" aria-label={t.navigator.close} onclick={onclose}>
      <Icon name="close" size={16} />
    </button>
  </header>

  {#if tab === 'contents'}
    <div class="book">
      <div class="cover" style:background={coverTint ?? 'var(--hairline)'} aria-hidden="true">
        {#if coverUrl && !coverFailed}
          <img src={coverUrl} alt="" onerror={() => (coverFailed = true)} />
        {/if}
      </div>
      <div class="meta">
        <div class="title">{title}</div>
        {#if author}<div class="author">{author}</div>{/if}
        <div class="progress">
          <div class="track" aria-hidden="true">
            <div class="fill" style:width="{fraction * 100}%"></div>
          </div>
          <button
            type="button"
            class="nav-goto"
            aria-haspopup="dialog"
            aria-label="{Math.round(fraction * 100)}% · {t.goto.open}"
            onclick={(e) => ongoto(e.currentTarget.getBoundingClientRect())}
            >{Math.round(fraction * 100)}%</button
          >
        </div>
      </div>
    </div>
  {/if}
  <div class="tabs">
    <Tabs
      label={t.navigator.tabs}
      idPrefix="navigator"
      tabs={[
        { value: 'contents', label: t.navigator.contents },
        { value: 'search', label: t.navigator.search },
        { value: 'notes', label: t.navigator.notes },
        // G7: one extension tab shows by name; more collapse into “More”.
        ...(extensionTabs.length === 1
          ? extensionTabs
          : extensionTabs.length
            ? [{ value: MORE, label: t.navigator.more }]
            : []),
      ]}
      bind:selected={
        () => (extensionTabs.length > 1 && tab.startsWith('extension:') ? MORE : tab),
        (v) => ontab(v)
      }
    />
  </div>
  {#if tab === 'search'}
    <div
      class="panel"
      id="navigator-panel-search"
      role="tabpanel"
      aria-labelledby="navigator-tab-search"
    >
      {@render search()}
    </div>
  {:else if tab === MORE}
    <div
      class="panel"
      id="navigator-panel-{MORE}"
      role="tabpanel"
      aria-labelledby="navigator-tab-{MORE}"
    >
      <ul class="more-tabs">
        {#each extensionTabs as x (x.value)}
          <li>
            <button type="button" class="row" onclick={() => ontab(x.value)}>{x.label}</button>
          </li>
        {/each}
      </ul>
    </div>
  {:else if tab.startsWith('extension:')}
    <div
      class="panel"
      id="navigator-panel-{extensionTabs.length > 1 ? MORE : tab}"
      role="tabpanel"
      aria-labelledby="navigator-tab-{extensionTabs.length > 1 ? MORE : tab}"
    >
      {@render extension?.()}
    </div>
  {:else if tab === 'notes'}
    <div
      class="panel"
      id="navigator-panel-notes"
      role="tabpanel"
      aria-labelledby="navigator-tab-notes"
    >
      {@render notes()}
    </div>
  {:else}
    <div
      class="panel"
      id="navigator-panel-contents"
      role="tabpanel"
      aria-labelledby="navigator-tab-contents"
    >
      <nav class="list" aria-label={t.navigator.contents}>
        {#if !contents}
          <p class="note">{t.navigator.loading}</p>
        {:else}
          {#if contents.source === 'headings'}
            <p class="note">{t.navigator.generated}</p>
          {/if}
          <ol>
            {#each contents.items as item, i (i)}
              <li>
                <button
                  type="button"
                  bind:this={rows[i]}
                  class="row"
                  class:current={i === current}
                  style:padding-left="{12 + item.depth * 16}px"
                  tabindex={i === Math.max(0, current) ? 0 : -1}
                  aria-current={i === current ? 'location' : undefined}
                  onclick={() => onselect(item)}
                  onkeydown={(e) => onkeydown(e, i)}
                >
                  <span class="label">{item.label}</span>
                  {#if item.damaged}
                    <span class="damaged"
                      ><Icon name="warning" size={14} />{t.navigator.damaged}</span
                    >
                  {:else if i === current}
                    <span class="here">{t.navigator.youAreHere}</span>
                  {/if}
                </button>
              </li>
            {/each}
          </ol>
        {/if}
      </nav>
    </div>
  {/if}
</aside>

<style>
  /* Screen 04: a 320 px panel on the panel ground, with a hairline to the text. */
  .navigator {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    z-index: 20;
    width: 320px;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    color: var(--ink);
    border-right: 1px solid var(--hairline);
    font-family: var(--font-ui);
    animation: slide-in var(--motion-navigator, 220ms) ease-out;
  }
  /* Below 1100 px it floats over the text, in the floating lane (S2). */
  .floating {
    box-shadow: var(--shadow-popover);
  }
  @keyframes slide-in {
    from {
      transform: translateX(-100%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .navigator {
      animation: none;
    }
  }
  .head {
    height: 52px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    /* The window buttons sit at x = 20 (Screen 04). */
    padding: 0 10px 0 82px;
  }
  .library,
  .close {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 36px;
    border: 0;
    border-radius: 8px;
    background: none;
    color: var(--ink-secondary);
    font: 500 13px var(--font-ui);
    cursor: default;
  }
  .library {
    padding: 0 10px 0 4px;
  }
  .close {
    width: 36px;
    justify-content: center;
  }
  .library:hover,
  .close:hover {
    background: var(--hover-wash);
    color: var(--ink);
  }
  .book {
    flex: none;
    display: flex;
    gap: 14px;
    padding: 4px 20px 16px;
  }
  .cover {
    width: 40px;
    height: 60px;
    flex: none;
    overflow: hidden;
    border-radius: 2px 4px 4px 2px;
    box-shadow:
      0 1px 2px rgb(40 30 20 / 14%),
      0 6px 16px rgb(40 30 20 / 6%);
  }
  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .meta {
    min-width: 0;
    flex: 1;
    padding-top: 3px;
  }
  .title {
    font-size: 14px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .author {
    font-size: 12.5px;
    color: var(--ink-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .track {
    flex: 1;
    height: 3px;
    border-radius: 2px;
    background: var(--control-track);
  }
  .fill {
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
  }
  .nav-goto {
    padding: 2px 6px;
    margin: -2px -6px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    font: inherit;
    cursor: default;
  }
  .nav-goto:hover,
  .nav-goto:focus-visible {
    color: var(--accent);
    font-weight: 600;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .tabs {
    flex: none;
    padding: 0 16px 8px;
  }
  /* Screen 04/05: the tab track on the panel. */
  .tabs :global(.tabs) {
    background: var(--control-track);
  }
  .tabs :global(button) {
    font: 500 13px var(--font-ui);
    color: var(--track-ink);
  }
  .tabs :global(button[aria-selected='true']) {
    background: var(--raised);
    border-color: var(--segment-ring);
    color: var(--ink);
    font-weight: 600;
  }
  .more-tabs {
    list-style: none;
    margin: 0;
    padding: 4px 8px;
  }
  .panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 2px 8px 16px;
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .note {
    margin: 4px 12px 8px;
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .row {
    width: 100%;
    min-height: 34px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px;
    border: 0;
    border-radius: 7px;
    background: none;
    color: var(--ink);
    font: 400 13.5px var(--font-ui);
    text-align: left;
    cursor: default;
  }
  .row:hover {
    background: var(--hover-wash);
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* “You are here”: accent text on a 9% accent tint (Screen 04). */
  .current,
  .current:hover {
    color: var(--accent);
    font-weight: 600;
    background: color-mix(in srgb, var(--accent) 9%, transparent);
  }
  .here {
    flex: none;
    font-size: 12px;
    font-weight: 500;
  }
  /* E3: damaged chapters stay listed and say so (Screen 12). */
  .damaged {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    font-weight: 500;
    color: var(--accent);
  }
</style>
