<script lang="ts">
  // The Navigator's Search tab (F1–F4, F6, F7; Screen 05).
  import { SvelteSet } from 'svelte/reactivity'
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings/en'
  import { RESULTS_PER_CHAPTER, type Hit, type SearchState } from './search.svelte'

  let {
    search,
    chapterLabel,
    currentSection,
    onbrowse,
    onchoose,
  }: {
    search: SearchState
    chapterLabel: (index: number) => string
    currentSection: number
    /** ↵ ⇧↵ ↑ ↓: move through results; the page follows (F6). */
    onbrowse: (hit: Hit) => void
    /** A click on a result: stay there, and Back returns (F7). */
    onchoose: (hit: Hit) => void
  } = $props()

  let input: HTMLInputElement | undefined = $state()
  /** Chapters toggled from their default (the current chapter open, others closed). */
  const collapsed = new SvelteSet<number>()
  const showAll = new SvelteSet<number>()

  export function focusField() {
    input?.focus()
    input?.select()
  }

  /** Open by default: the current chapter, or the first with results when it has none. */
  const openByDefault = $derived(
    search.groups.some((g) => g.index === currentSection)
      ? currentSection
      : (search.groups[0]?.index ?? -1),
  )
  const expanded = (index: number) =>
    index === openByDefault ? !collapsed.has(index) : collapsed.has(index)
  function toggle(index: number) {
    if (collapsed.has(index)) collapsed.delete(index)
    else collapsed.add(index)
  }

  function onfieldkey(e: KeyboardEvent) {
    if (e.key !== 'Enter') return
    e.preventDefault()
    if (!search.settled && !search.groups.length) search.run()
    const hit = search.step(e.shiftKey ? -1 : 1)
    if (hit) onbrowse(hit)
  }

  /** ↑ ↓ in the list move through results, and the page follows. */
  function onlistkey(e: KeyboardEvent) {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
    e.preventDefault()
    const hit = search.step(e.key === 'ArrowDown' ? 1 : -1)
    if (!hit) return
    // The result's chapter opens so the row is there to focus.
    if (!expanded(hit.index)) toggle(hit.index)
    onbrowse(hit)
    requestAnimationFrame(() =>
      document.querySelector<HTMLElement>(`[data-hit="${hit.index}:${hit.n}"]`)?.focus(),
    )
  }

  const chapters = $derived(search.groups.length)
</script>

<div class="search">
  <div class="field">
    <Icon name="search" size={16} />
    <input
      bind:this={input}
      value={search.query}
      oninput={(e) => search.setQuery(e.currentTarget.value)}
      onkeydown={onfieldkey}
      placeholder={t.search.placeholder}
      aria-label={t.search.label}
      autocomplete="off"
      spellcheck="false"
    />
    {#if search.query}
      <button
        type="button"
        class="clear"
        aria-label={t.search.clear}
        onclick={() => {
          search.setQuery('')
          search.run()
          input?.focus()
        }}><Icon name="close" size={14} /></button
      >
    {/if}
  </div>

  <!-- B6 (1.1, approved 2026-10-01): whole words and regular expressions. -->
  <div class="options" role="group" aria-label={t.search.options}>
    <button
      type="button"
      class="opt"
      aria-pressed={search.wholeWord}
      onclick={() => search.setOptions({ wholeWord: !search.wholeWord })}
      >{t.search.wholeWords}</button
    >
    <button
      type="button"
      class="opt mono"
      aria-pressed={search.regex}
      title={t.search.regex}
      onclick={() => search.setOptions({ regex: !search.regex })}
      >.*<span class="visually-hidden">{t.search.regex}</span></button
    >
  </div>

  {#if search.query.trim()}
    <div class="status" aria-live="polite">
      <div class="count">
        <span class="total">{t.search.count(search.count, chapters)}</span>
        {#if search.running}<span class="sofar">{t.search.soFar}</span>{/if}
      </div>
      {#if search.running}
        <div class="progress">
          <div class="bar" aria-hidden="true">
            <div style:width="{(search.searched / Math.max(1, search.total)) * 100}%"></div>
          </div>
          <span>{t.search.searching(search.searched, search.total)}</span>
        </div>
      {/if}
    </div>
  {/if}

  <div class="results" role="presentation" onkeydown={onlistkey}>
    {#each search.groups as group (group.index)}
      {@const open = expanded(group.index)}
      <section class="group">
        <button type="button" class="head" aria-expanded={open} onclick={() => toggle(group.index)}>
          <span class="chevron" class:open><Icon name="chevron" size={12} /></span>
          <span class="label">{chapterLabel(group.index)}</span>
          {#if group.index === currentSection}<span class="here">{t.navigator.youAreHere}</span
            >{/if}
          <span class="n">{group.matches.length}</span>
        </button>
        {#if open}
          {@const limit = showAll.has(group.index) ? Infinity : RESULTS_PER_CHAPTER}
          <ul>
            {#each group.matches.slice(0, limit) as match, n (n)}
              {@const active = search.active?.index === group.index && search.active.n === n}
              <li>
                <button
                  type="button"
                  class="hit"
                  class:active
                  data-hit="{group.index}:{n}"
                  aria-current={active ? 'true' : undefined}
                  onclick={() => onchoose({ index: group.index, n, match })}
                >
                  {match.snippet.before}<mark>{match.snippet.match}</mark>{match.snippet.after}
                </button>
              </li>
            {/each}
          </ul>
          {#if group.matches.length > limit}
            <button type="button" class="more" onclick={() => showAll.add(group.index)}
              >{t.search.showAll(group.matches.length)}</button
            >
          {/if}
        {/if}
      </section>
    {:else}
      {#if search.query.trim() && search.settled}
        <p class="empty">
          {search.invalid ? t.search.invalid : t.search.none(search.query.trim())}
        </p>
      {/if}
    {/each}
  </div>

  <div class="foot" aria-hidden="true">
    <span><kbd>↵</kbd>{t.search.next}</span>
    <span><kbd>⇧↵</kbd>{t.search.previous}</span>
    <span><kbd>Esc</kbd>{t.search.back}</span>
  </div>
</div>

<style>
  .search {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  /* Screen 05: the field with an accent ring. */
  .field {
    flex: none;
    margin: 12px 16px 8px;
    height: 38px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 0 12px;
    border: 1px solid var(--popover-border);
    border-radius: 9px;
    background: var(--raised);
    color: var(--ink-secondary);
  }
  .field:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }
  input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: none;
    color: var(--ink);
    font: 400 14px var(--font-ui);
  }
  .clear {
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--ink-secondary);
    cursor: default;
  }
  .options {
    flex: none;
    display: flex;
    gap: 6px;
    padding: 8px 20px 0;
  }
  .opt {
    height: 24px;
    padding: 0 9px;
    border: 1px solid var(--popover-border);
    border-radius: 6px;
    background: none;
    color: var(--ink-secondary);
    font: 500 12px var(--font-ui);
    cursor: default;
  }
  .opt.mono {
    font-family: ui-monospace, monospace;
  }
  .opt[aria-pressed='true'] {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }
  .opt:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .status {
    flex: none;
    padding: 4px 20px 8px;
  }
  .count {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }
  .total {
    font-size: 13px;
    font-weight: 600;
  }
  .sofar,
  .progress span {
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }
  .bar {
    flex: 1;
    height: 2px;
    border-radius: 1px;
    background: var(--control-track);
  }
  .bar div {
    height: 100%;
    border-radius: 1px;
    background: var(--accent);
  }
  .results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px 12px;
  }
  .head {
    width: 100%;
    min-height: 32px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 12px 0 8px;
    border: 0;
    border-radius: 7px;
    background: none;
    color: var(--ink);
    font: 600 13px var(--font-ui);
    text-align: left;
    cursor: default;
  }
  .head:hover {
    background: var(--hover-wash);
  }
  .chevron {
    display: inline-grid;
    color: var(--ink-secondary);
    transition: transform 100ms;
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .here {
    flex: none;
    color: var(--accent);
    font-size: 12px;
    font-weight: 500;
  }
  .n {
    margin-left: auto;
    color: var(--ink-secondary);
    font-weight: 500;
  }
  ul {
    list-style: none;
    margin: 0 0 6px;
    padding: 0;
  }
  .hit {
    width: 100%;
    padding: 7px 12px 7px 30px;
    border: 0;
    border-radius: 8px;
    background: none;
    color: var(--ink);
    font: 400 13px/1.45 var(--font-ui);
    text-align: left;
    cursor: default;
  }
  .hit:hover {
    background: var(--hover-wash);
  }
  /* The result on the page: a raised row (Screen 05). */
  .hit.active {
    background: var(--raised);
    box-shadow: 0 1px 2px rgb(40 30 20 / 8%);
  }
  /* F5 in the list: the match as in the page. */
  mark {
    color: inherit;
    background: var(--search-tint);
    outline: 1px solid var(--search-outline);
    border-radius: 2px;
  }
  .hit.active mark {
    background: var(--search-active-tint);
    outline: 1px solid var(--search-active-outline);
  }
  .more {
    margin: 0 0 8px 30px;
    padding: 4px 8px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--accent);
    font: 600 12px var(--font-ui);
    cursor: default;
  }
  .empty {
    margin: 12px;
    font-size: 13px;
    color: var(--ink-secondary);
  }
  .foot {
    flex: none;
    display: flex;
    gap: 14px;
    padding: 10px 16px 12px;
    border-top: 1px solid var(--hairline);
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .foot span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  kbd {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--raised);
    border: 1px solid var(--popover-border);
    color: var(--key-ink);
    font: 500 11px var(--font-ui);
  }
</style>
