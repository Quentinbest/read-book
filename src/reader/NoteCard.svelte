<script lang="ts">
  // A note on a highlight (A6; Screens 07 and 15). From 1240 px it is a card in
  // the margin beside the passage, joined to its dot; below that it is a bottom
  // sheet with Delete and Done. It saves as you type and says so; Esc or a click
  // elsewhere closes it (the reader handles those); an empty note is discarded
  // and the highlight kept (the store does that).
  import { onDestroy, onMount } from 'svelte'
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings'
  import type { Annotation, HighlightColor } from '../lib/annotations/model'
  import { COLORS } from './annotations.svelte'

  let {
    annotation,
    mode,
    place,
    onsave,
    oncolor,
    oncopy,
    ondelete,
    onclose,
  }: {
    annotation: Annotation
    mode: 'margin' | 'sheet'
    /** Margin placement in window coordinates: the card's corner and the dot it joins. */
    place: { left: number; top: number; dotX: number; dotY: number } | null
    onsave: (text: string) => Promise<void>
    oncolor: (c: HighlightColor) => void
    oncopy: () => void
    ondelete: () => void
    onclose: () => void
  } = $props()

  const SAVE_AFTER_MS = 400
  // The text starts as the saved note; from then on the field owns it.
  // svelte-ignore state_referenced_locally
  let text = $state(annotation.note ?? '')
  let status = $state<'saved' | 'saving' | 'unsaved'>('saved')
  let dirty = false
  let timer = 0
  let field: HTMLTextAreaElement | undefined = $state()
  let card: HTMLElement | undefined = $state()

  /** Save now what was typed (on close, page turn and quit: S6, N5). */
  export function flush() {
    clearTimeout(timer)
    if (!dirty) return
    dirty = false
    status = 'saving'
    void onsave(text).then(
      () => (status = dirty ? 'unsaved' : 'saved'),
      () => (status = 'unsaved'),
    )
  }

  export function focus() {
    field?.focus()
  }

  export function contains(node: Node | null): boolean {
    return !!node && !!card?.contains(node)
  }

  function oninput() {
    dirty = true
    status = 'unsaved'
    clearTimeout(timer)
    timer = window.setTimeout(flush, SAVE_AFTER_MS)
  }

  function onkeydown(e: KeyboardEvent) {
    // Keys typed here are the note's, never reader shortcuts.
    e.stopPropagation()
    if (e.key === 'Escape') {
      e.preventDefault()
      onclose()
    }
  }

  const quote = $derived.by(() => {
    const q = annotation.quote.exact.replace(/\s+/g, ' ').trim()
    return q.length > 72 ? q.slice(0, 72).replace(/\s+\S*$/, '') + '…' : q
  })
  const statusText = $derived(
    status === 'saved'
      ? t.annotations.saved
      : status === 'saving'
        ? t.annotations.saving
        : t.annotations.notSaved,
  )

  onMount(() => field?.focus())
  onDestroy(flush)
</script>

<div
  bind:this={card}
  class="note {mode}"
  role="dialog"
  tabindex="-1"
  aria-label={t.annotations.noteLabel}
  style:left={mode === 'margin' && place ? `${place.left}px` : undefined}
  style:top={mode === 'margin' && place ? `${place.top}px` : undefined}
  {onkeydown}
>
  {#if mode === 'margin' && place}
    <span
      class="connector"
      aria-hidden="true"
      style:left="{place.dotX + 4 - place.left}px"
      style:width="{Math.max(0, place.left - place.dotX - 4)}px"
      style:top="{place.dotY - place.top}px"
    ></span>
  {/if}
  {#if mode === 'sheet'}<div class="grip" aria-hidden="true"></div>{/if}
  <header>
    {#if mode === 'margin'}
      <span class="swatch" style:background="var(--hl-{annotation.color}-swatch)"></span>
    {/if}
    <span class="title">{t.annotations.noteTitle(t.colors[annotation.color])}</span>
    <span class="status" role="status">
      {#if status === 'saved'}<Icon name="check" size={14} />{/if}{statusText}
    </span>
  </header>
  {#if mode === 'sheet'}
    <p class="quote">“{quote}”</p>
  {/if}
  <label class="visually-hidden" for="note-{annotation.id}">{t.annotations.noteText}</label>
  <textarea
    id="note-{annotation.id}"
    bind:this={field}
    bind:value={text}
    rows={mode === 'margin' ? 5 : 4}
    {oninput}></textarea>
  <div class="actions">
    {#each COLORS as c (c)}
      <button
        type="button"
        class="color"
        class:current={c === annotation.color}
        aria-label={t.annotations.colorOption(t.colors[c], c === annotation.color)}
        aria-pressed={c === annotation.color}
        onclick={() => oncolor(c)}
      >
        <span style:background="var(--hl-{c}-swatch)"></span>
      </button>
    {/each}
    <span class="grow"></span>
    {#if mode === 'margin'}
      <button type="button" class="icon" aria-label={t.annotations.copyNote} onclick={oncopy}>
        <Icon name="copy" size={15} />
      </button>
      <button type="button" class="icon" aria-label={t.annotations.deleteNote} onclick={ondelete}>
        <Icon name="trash" size={15} />
      </button>
    {:else}
      <button type="button" class="button" onclick={ondelete}>{t.annotations.delete}</button>
      <button type="button" class="button primary" onclick={onclose}>{t.annotations.done}</button>
    {/if}
  </div>
  {#if mode === 'margin'}<p class="hint">{t.annotations.noteHint}</p>{/if}
</div>

<style>
  .note {
    position: fixed;
    z-index: 35;
    display: flex;
    flex-direction: column;
    background: var(--popover);
    color: var(--ink-secondary);
    font: 400 12px/1.4 var(--font-ui);
  }
  .margin {
    width: 256px;
    padding: 12px 14px 10px;
    gap: 8px;
    border: 1px solid var(--hairline);
    border-radius: 10px;
    box-shadow: 0 10px 28px rgba(40, 30, 20, 0.12);
  }
  .sheet {
    left: 0;
    right: 0;
    bottom: 0;
    padding: 10px 28px 20px;
    gap: 12px;
    border-top: 1px solid var(--hairline);
    border-radius: 14px 14px 0 0;
    box-shadow: 0 -10px 30px rgba(40, 30, 20, 0.12);
  }
  .connector {
    position: absolute;
    height: 1px;
    background: var(--popover-border);
  }
  .grip {
    align-self: center;
    width: 40px;
    height: 4px;
    border-radius: 2px;
    background: var(--popover-border);
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.14);
  }
  .title {
    font-weight: 600;
    color: var(--ink);
    flex-grow: 1;
  }
  .sheet .title {
    font-size: 15px;
  }
  .sheet header {
    font-size: 13px;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .quote {
    margin: 0;
    font: italic 14px/1.5 var(--font-reading, Literata, Georgia, serif);
    color: var(--ink);
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: none;
    color: var(--ink);
    font: 15px/1.5 var(--font-reading, Literata, Georgia, serif);
  }
  .margin textarea {
    border: 0;
    padding: 0;
    background: transparent;
    outline: none;
  }
  .sheet textarea {
    border: 1px solid var(--popover-border);
    border-radius: 10px;
    padding: 12px 14px;
    background: var(--raised);
    font-size: 16px;
  }
  .sheet textarea:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .margin .actions {
    border-top: 1px solid var(--hairline);
    padding-top: 6px;
  }
  .sheet .actions {
    gap: 4px;
  }
  .grow {
    flex-grow: 1;
  }
  .color {
    width: 24px;
    height: 24px;
    border: 0;
    padding: 0;
    border-radius: 50%;
    background: transparent;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }
  .color span {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    display: block;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.14);
  }
  .sheet .color {
    width: 32px;
    height: 32px;
  }
  .sheet .color span {
    width: 18px;
    height: 18px;
  }
  .color.current {
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }
  .sheet .color.current {
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  .icon {
    min-width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    background: transparent;
    border-radius: 7px;
    color: var(--ink-secondary);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover-wash);
    color: var(--ink);
  }
  .button {
    height: 36px;
    padding: 0 14px;
    border: 1px solid var(--popover-border);
    border-radius: 8px;
    background: var(--raised);
    font: 500 13px var(--font-ui);
    color: var(--ink);
    cursor: pointer;
  }
  .button.primary {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--ground);
    font-weight: 600;
  }
  .color:focus-visible,
  .icon:focus-visible,
  .button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .hint {
    margin: 0;
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
