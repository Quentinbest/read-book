<script lang="ts">
  // Go to location (N8; Screen 17): Percent, Chapter, or Print page when the book
  // has a page list. A preview line names the target; the current place stays in
  // Back history. Anchored above the progress label.
  import { untrack } from 'svelte'
  import SegmentedControl from '../components/SegmentedControl.svelte'
  import { t } from '../lib/strings'
  import type { ContentsItem } from './contents'

  export type GoToTarget =
    | { kind: 'percent'; fraction: number }
    | { kind: 'chapter'; href: string }
    | { kind: 'page'; href: string }

  type Mode = 'percent' | 'chapter' | 'page'

  let {
    fraction,
    chapters,
    currentChapter,
    pages,
    chapterAt,
    chapterStart,
    anchor,
    ongo,
  }: {
    fraction: number
    chapters: ContentsItem[]
    currentChapter: number
    pages: { label: string; href: string }[]
    /** The Contents label for a book fraction, or for an href. */
    chapterAt: (target: number | string) => string
    /** Where a Contents entry starts, as a book fraction. */
    chapterStart: (item: ContentsItem) => number
    /** The label that opened it; Go to sits above it, or below when it is near the top. */
    anchor: DOMRect
    ongo: (target: GoToTarget) => void
  } = $props()

  const below = $derived(anchor.top < innerHeight / 2)
  let mode = $state<Mode>('percent')
  // The fields start at the current place.
  let percent = $state(untrack(() => String(Math.round(fraction * 100))))
  let chapter = $state(untrack(() => Math.max(0, currentChapter)))
  let page = $state('')
  let input: HTMLInputElement | HTMLSelectElement | undefined = $state()

  const options = $derived<{ value: Mode; label: string }[]>([
    { value: 'percent', label: t.goto.percent },
    { value: 'chapter', label: t.goto.chapter },
    ...(pages.length ? [{ value: 'page' as const, label: t.goto.page }] : []),
  ])

  const pct = $derived(Math.min(100, Math.max(0, Math.round(Number(percent)))))
  const pageTarget = $derived(pages.find((p) => p.label === page.trim()))

  /** The preview line: [before, bold, after]. */
  const preview = $derived.by((): [string, string, string] | null => {
    if (mode === 'percent') {
      if (percent.trim() === '' || Number.isNaN(Number(percent))) return null
      return [t.goto.percentIn(pct), chapterAt(pct / 100), '. ']
    }
    if (mode === 'chapter') {
      const item = chapters[chapter]
      if (!item) return null
      return ['', item.label, t.goto.chapterAt(Math.round(chapterStart(item) * 100)) + ' ']
    }
    if (!page.trim()) return null
    if (!pageTarget) return [t.goto.noPage(page.trim()), '', '']
    return [t.goto.pageIn(pageTarget.label), chapterAt(pageTarget.href), '. ']
  })

  const ready = $derived(
    mode === 'percent'
      ? percent.trim() !== '' && !Number.isNaN(Number(percent))
      : mode === 'chapter'
        ? !!chapters[chapter]
        : !!pageTarget,
  )

  function go() {
    if (!ready) return
    if (mode === 'percent') ongo({ kind: 'percent', fraction: pct / 100 })
    else if (mode === 'chapter') ongo({ kind: 'chapter', href: chapters[chapter].href })
    else if (pageTarget) ongo({ kind: 'page', href: pageTarget.href })
  }

  $effect(() => {
    void mode
    requestAnimationFrame(() => {
      input?.focus()
      if (input instanceof HTMLInputElement) input.select()
    })
  })
</script>

<div
  class="goto"
  role="dialog"
  aria-label={t.goto.label}
  style:right={below ? undefined : `${Math.max(12, innerWidth - anchor.right - 20)}px`}
  style:bottom={below ? undefined : `${innerHeight - anchor.top + 12}px`}
  style:left={below ? `${Math.max(12, anchor.left - 20)}px` : undefined}
  style:top={below ? `${anchor.bottom + 10}px` : undefined}
>
  <SegmentedControl label={t.goto.label} {options} bind:value={mode} />
  <form
    onsubmit={(e) => {
      e.preventDefault()
      go()
    }}
  >
    {#if mode === 'percent'}
      <label for="goto-input">{t.goto.percentLabel}</label>
      <div class="field">
        <input
          id="goto-input"
          bind:this={input}
          bind:value={percent}
          inputmode="numeric"
          autocomplete="off"
        />
        <button type="submit" class="go" disabled={!ready}>{t.goto.go}</button>
      </div>
    {:else if mode === 'chapter'}
      <label for="goto-input">{t.goto.chapterLabel}</label>
      <div class="field">
        <select id="goto-input" bind:this={input} bind:value={chapter}>
          {#each chapters as item, i (i)}
            <option value={i}>{' '.repeat(item.depth)}{item.label}</option>
          {/each}
        </select>
        <button type="submit" class="go" disabled={!ready}>{t.goto.go}</button>
      </div>
    {:else}
      <label for="goto-input">{t.goto.pageLabel}</label>
      <div class="field">
        <input id="goto-input" bind:this={input} bind:value={page} autocomplete="off" />
        <button type="submit" class="go" disabled={!ready}>{t.goto.go}</button>
      </div>
    {/if}
  </form>
  <p class="preview" aria-live="polite">
    {#if preview}
      {preview[0]}{#if preview[1]}<b>{preview[1]}</b>{/if}{preview[2]}{#if ready}{t.goto.stays}{/if}
    {/if}
  </p>
  {#if !pages.length}
    <p class="note">{t.goto.pageListOnly}</p>
  {/if}
</div>

<style>
  /* Screen 17: 360 px on the popover surface, anchored above the progress label. */
  .goto {
    position: fixed;
    z-index: 30;
    width: 360px;
    box-sizing: border-box;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    border-radius: 12px;
    background: var(--popover);
    border: 1px solid var(--popover-border);
    box-shadow: 0 12px 32px rgb(40 30 20 / 14%);
    color: var(--ink);
    font-family: var(--font-ui);
    animation: pop-in var(--motion-popover, 140ms) ease-out;
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
  }
  .goto :global(.segmented) {
    display: grid;
    background: var(--control-track);
  }
  .goto :global(.segmented button) {
    font-size: 13px;
    color: var(--track-ink);
  }
  .goto :global(.segmented button[aria-checked='true']) {
    background: var(--raised);
    border-color: var(--segment-ring);
    color: var(--ink);
    font-weight: 600;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  label {
    font-size: 12px;
    font-weight: 600;
    color: var(--ink);
  }
  .field {
    display: flex;
    gap: 10px;
  }
  input,
  select {
    flex: 1;
    min-width: 0;
    height: 36px;
    box-sizing: border-box;
    padding: 0 10px;
    border: 1px solid var(--segment-ring);
    border-radius: 8px;
    background: var(--raised);
    color: var(--ink);
    font: 400 14px var(--font-ui);
  }
  .go {
    height: 36px;
    padding: 0 14px;
    border: 0;
    border-radius: 8px;
    background: var(--ink);
    color: var(--ground);
    font: 600 13px var(--font-ui);
    cursor: default;
  }
  .go:disabled {
    opacity: 0.4;
  }
  .preview {
    margin: 0;
    min-height: 38px;
    font-size: 13px;
    line-height: 1.45;
    color: var(--ink);
  }
  .note {
    margin: 0;
    font-size: 12px;
    color: var(--ink-secondary);
  }
</style>
