<script lang="ts">
  // The Navigator's Notes tab (A8; Screen 08): highlights by chapter in reading
  // order, filtered by colour and text, with “N highlights · M notes”. Choosing
  // one jumps to it (the reader pulses the passage and offers Back). Highlights
  // that could not be placed in this edition are listed under “Couldn't place”
  // with their quote, the reason and Re-attach; they are never dropped.
  // Exporters from extensions (Markdown Export is built in) sit in the footer,
  // each naming the extension that does the work (Screen 08).
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings'
  import { bookLang } from './bookLang'
  const lang = bookLang()
  import type { Annotation, HighlightColor } from '../lib/annotations/model'
  import { COLORS } from './annotations.svelte'
  import { formatDate } from '../lib/strings/format'

  let {
    placed,
    unplaced,
    chapterOf,
    chapterLabel,
    order,
    current,
    onchoose,
    onreattach,
    exporters = [],
    onexport,
    now = Date.now(),
  }: {
    placed: Annotation[]
    unplaced: Annotation[]
    /** The chapter a highlight is in (by its CFI), or -1. */
    chapterOf: (a: Annotation) => number
    chapterLabel: (index: number) => string
    /** Reading order of two CFIs. */
    order: (a: string, b: string) => number
    /** The highlight last jumped to. */
    current: string | null
    onchoose: (a: Annotation) => void
    onreattach: (a: Annotation) => void
    /** P§18: exporters contributed by extensions. */
    exporters?: { extId: string; id: string; title: string; name: string; command: string }[]
    onexport?: (e: { extId: string; command: string; name: string }) => void
    now?: number
  } = $props()

  let query = $state('')
  let only = $state<HighlightColor | null>(null)
  let input: HTMLInputElement | undefined = $state()
  let list: HTMLElement | undefined = $state()

  export function focusField() {
    input?.focus()
  }

  const matches = (a: Annotation) => {
    if (only && a.color !== only) return false
    const q = query.trim().toLowerCase()
    if (!q) return true
    return a.quote.exact.toLowerCase().includes(q) || !!a.note?.toLowerCase().includes(q)
  }

  const groups = $derived.by(() => {
    const by: Record<number, Annotation[]> = {}
    for (const a of placed) if (matches(a)) (by[chapterOf(a)] ??= []).push(a)
    return Object.entries(by)
      .map(([i, items]) => [Number(i), items] as const)
      .sort(([a], [b]) => a - b)
      .map(([index, items]) => ({ index, items: items.sort((x, y) => order(x.cfi, y.cfi)) }))
  })
  const lost = $derived(unplaced.filter(matches))
  const shown = $derived(groups.reduce((n, g) => n + g.items.length, 0) + lost.length)
  const counts = $derived({
    highlights: placed.length + unplaced.length,
    notes: [...placed, ...unplaced].filter((a) => a.note).length,
  })

  function when(ms: number): string {
    const day = (x: number) => new Date(x).toDateString()
    if (day(ms) === day(now)) return t.annotations.today
    if (day(ms) === day(now - 86_400_000)) return t.annotations.yesterday
    const d = new Date(ms)
    return formatDate(ms, {
      day: 'numeric',
      month: 'short',
      year: d.getFullYear() === new Date(now).getFullYear() ? undefined : 'numeric',
    })
  }
  const meta = (a: Annotation) =>
    [t.colors[a.color], when(a.createdAt), a.note ? t.annotations.hasNote : null]
      .filter(Boolean)
      .join(' · ')

  /** ↑ ↓ move between rows (one tab stop per list). */
  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
    const rows = Array.from(list?.querySelectorAll<HTMLElement>('[data-row]') ?? [])
    const i = rows.indexOf(document.activeElement as HTMLElement)
    const next = rows[e.key === 'ArrowDown' ? i + 1 : i - 1]
    if (!next) return
    e.preventDefault()
    e.stopPropagation()
    next.focus()
  }
</script>

<div class="notes">
  <div class="field">
    <Icon name="search" size={15} />
    <label class="visually-hidden" for="notes-filter">{t.annotations.filter}</label>
    <input
      id="notes-filter"
      bind:this={input}
      type="search"
      bind:value={query}
      placeholder={t.annotations.filter}
      onkeydown={(e) => e.stopPropagation()}
    />
  </div>
  <div class="chips">
    <button
      type="button"
      class="pill"
      class:on={only === null}
      aria-pressed={only === null}
      onclick={() => (only = null)}>{t.annotations.all}</button
    >
    {#each COLORS as c (c)}
      <button
        type="button"
        class="chip"
        class:on={only === c}
        aria-pressed={only === c}
        aria-label={t.annotations.only(t.colorsInText[c])}
        onclick={() => (only = only === c ? null : c)}
      >
        <span style:background="var(--hl-{c}-swatch)"></span>
      </button>
    {/each}
    <span class="counts">{t.annotations.counts(counts.highlights, counts.notes)}</span>
  </div>
  <div class="list" bind:this={list}>
    {#if !counts.highlights}
      <p class="empty">{t.annotations.none}</p>
    {:else if !shown}
      <p class="empty">{t.annotations.noMatches}</p>
    {/if}
    {#each groups as g (g.index)}
      <div class="group" role="presentation">
        {g.index >= 0 ? chapterLabel(g.index) : ''}<span class="count">{g.items.length}</span>
      </div>
      {#each g.items as a (a.id)}
        <button
          type="button"
          class="item"
          class:on={a.id === current}
          data-row
          {onkeydown}
          data-annotation={a.id}
          aria-current={a.id === current ? 'true' : undefined}
          onclick={() => onchoose(a)}
        >
          <span class="quote" {lang}
            ><span class="hl hl-{a.color}">{a.quote.exact.trim()}</span></span
          >
          {#if a.note}<span class="note">{a.note}</span>{/if}
          <span class="meta">{meta(a)}</span>
        </button>
      {/each}
    {/each}
    {#if lost.length}
      <div class="group lost" role="presentation">
        <Icon name="warning" size={14} />{t.annotations.couldntPlace}<span class="count"
          >{lost.length}</span
        >
      </div>
      {#each lost as a (a.id)}
        <div class="item unplaced" data-annotation={a.id}>
          <span class="quote dashed" {lang}>{a.quote.exact.trim()}</span>
          {#if a.note}<span class="note">{a.note}</span>{/if}
          <span class="meta"
            >{t.annotations.changedReason}
            <button
              type="button"
              class="reattach"
              data-row
              {onkeydown}
              onclick={() => onreattach(a)}>{t.annotations.reattach}</button
            ></span
          >
        </div>
      {/each}
    {/if}
  </div>
  {#if exporters.length && counts.highlights}
    <footer class="export">
      {#each exporters as e (e.extId + e.id)}
        <button type="button" class="export-button" onclick={() => onexport?.(e)}>{e.title}</button>
        <span class="via">{t.extensions.via(e.name)}</span>
      {/each}
    </footer>
  {/if}
</div>

<style>
  .notes {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  .field {
    margin: 0 16px;
    position: relative;
    display: flex;
    align-items: center;
    color: var(--ink-secondary);
  }
  .field :global(svg) {
    position: absolute;
    left: 10px;
  }
  input {
    width: 100%;
    height: 32px;
    box-sizing: border-box;
    padding: 0 12px 0 32px;
    border: 0;
    border-radius: 8px;
    background: var(--control-track);
    font: 13px var(--font-ui);
    color: var(--ink);
  }
  input::placeholder {
    color: var(--ink-secondary);
    opacity: 1;
  }
  input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .chips {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 10px 16px 6px;
  }
  .pill {
    height: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: 13px;
    font: 600 12px var(--font-ui);
    background: transparent;
    color: var(--ink-secondary);
    cursor: pointer;
  }
  .pill.on {
    background: var(--ink);
    color: var(--ground);
  }
  .chip {
    width: 26px;
    height: 26px;
    border: 0;
    padding: 0;
    border-radius: 50%;
    background: transparent;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }
  .chip span {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    display: block;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.14);
  }
  .chip.on {
    box-shadow: inset 0 0 0 1.5px var(--ink);
  }
  .pill:focus-visible,
  .chip:focus-visible,
  .item:focus-visible,
  .reattach:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .counts {
    margin-left: auto;
    font-size: 12px;
    color: var(--ink-secondary);
  }
  /* Screen 08: the export footer, on the panel's hairline. */
  .export {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-top: 1px solid var(--hairline);
  }
  .export-button {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--popover-border);
    border-radius: 8px;
    background: var(--raised);
    color: var(--ink);
    font: 500 13px var(--font-ui);
  }
  .export-button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .via {
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .list {
    flex: 1;
    overflow: auto;
    padding: 0 8px 16px;
  }
  .empty {
    margin: 12px;
    font-size: 13px;
    color: var(--ink-secondary);
  }
  .group {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--ink);
  }
  .group .count {
    margin-left: auto;
    font-weight: 500;
    color: var(--ink-secondary);
  }
  .lost {
    margin-top: 6px;
  }
  .lost :global(svg) {
    color: var(--accent);
  }
  .item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 10px 12px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--ink);
    font: inherit;
    cursor: pointer;
  }
  .item:hover {
    background: var(--hover-wash);
  }
  .item.on {
    background: var(--raised);
    box-shadow: 0 1px 2px rgba(40, 30, 20, 0.1);
  }
  .item.unplaced {
    cursor: default;
  }
  .item.unplaced:hover {
    background: transparent;
  }
  .quote {
    display: block;
    font: 13.5px/1.55 var(--font-reading, Literata, Georgia, serif);
    color: var(--ink);
  }
  .hl {
    -webkit-box-decoration-break: clone;
    box-decoration-break: clone;
    padding: 1px 2px;
  }
  .hl-yellow {
    background: var(--hl-yellow-tint);
    box-shadow: inset 0 -2px 0 var(--hl-yellow-underline);
  }
  .hl-green {
    background: var(--hl-green-tint);
    box-shadow: inset 0 -2px 0 var(--hl-green-underline);
  }
  .hl-blue {
    background: var(--hl-blue-tint);
    box-shadow: inset 0 -2px 0 var(--hl-blue-underline);
  }
  .hl-rose {
    background: var(--hl-rose-tint);
    box-shadow: inset 0 -2px 0 var(--hl-rose-underline);
  }
  .dashed {
    text-decoration: underline dashed var(--popover-border);
    text-underline-offset: 3px;
  }
  .note {
    display: block;
    font-size: 13px;
    line-height: 1.45;
    color: var(--ink);
    margin-top: 6px;
  }
  .meta {
    display: block;
    font-size: 12px;
    color: var(--ink-secondary);
    margin-top: 5px;
  }
  .reattach {
    display: block;
    border: 0;
    background: transparent;
    padding: 0;
    margin-top: 2px;
    font: 600 12px var(--font-ui);
    color: var(--accent);
    cursor: pointer;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  @media (forced-colors: active) {
    .hl {
      text-decoration: underline 2px;
    }
    .pill.on,
    .chip.on {
      outline: 2px solid CanvasText;
    }
  }
</style>
