<script lang="ts">
  // The selection bar (A1–A3, A5; Screens 06 and 14). It sits 12 px above the
  // first selected line and flips below when there is less than 56 px above; it
  // never covers the selection. Ink-coloured, inverted to light at Night (V2).
  // F6 moves focus in, arrows move between actions, Esc returns to the text.
  // The extension-actions area (P10) stays hidden until extensions arrive.
  import { fade } from 'svelte/transition'
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings/en'
  import { MOTION } from '../lib/theme/tokens'
  import { COLOR_NAMES, COLORS } from './annotations.svelte'
  import type { HighlightColor } from '../lib/annotations/model'

  let {
    first,
    last,
    top: minTop = 0,
    mode,
    color,
    onhighlight,
    onnote,
    oncopy,
    onsearch,
    ondelete,
    onescape,
    onattach,
    oncancel,
  }: {
    first: DOMRect
    last: DOMRect
    /** What covers the top of the window (the chrome when shown). */
    top?: number
    /** A new selection, a highlight that was clicked (A5), or re-attaching (G11). */
    mode: 'new' | 'existing' | 'attach'
    /** The last-used colour (new) or the highlight's colour (existing). */
    color: HighlightColor
    onhighlight: (c: HighlightColor) => void
    onnote: () => void
    oncopy: () => void
    onsearch: () => void
    ondelete: () => void
    onescape: () => void
    onattach?: () => void
    oncancel?: () => void
  } = $props()

  const GAP = 12
  const FLIP_BELOW = 56
  const TAIL_X = 49 // Screen 06: the tail sits 49 px in, 15 px after the selection starts

  let bar: HTMLElement | undefined = $state()
  let width = $state(0)
  let height = $state(42)

  const below = $derived(first.top - minTop < FLIP_BELOW)
  const anchorX = $derived(below ? last.left : first.left)
  const left = $derived(Math.max(8, Math.min(window.innerWidth - width - 8, anchorX + 15 - TAIL_X)))
  const tail = $derived(Math.max(14, Math.min(width - 14, anchorX + 15 - left)))
  const top = $derived(below ? last.bottom + GAP : first.top - GAP - height)

  const buttons = () => Array.from(bar?.querySelectorAll<HTMLButtonElement>('button') ?? [])

  /** F6 (A2). */
  export function focusFirst() {
    buttons()[0]?.focus()
  }

  export function contains(node: Node | null): boolean {
    return !!node && !!bar?.contains(node)
  }

  function onkeydown(e: KeyboardEvent) {
    const list = buttons()
    const i = list.indexOf(document.activeElement as HTMLButtonElement)
    let next = -1
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (i + 1) % list.length
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp')
      next = (i - 1 + list.length) % list.length
    else if (e.key === 'Home') next = 0
    else if (e.key === 'End') next = list.length - 1
    else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      onescape()
      return
    }
    if (next < 0) return
    e.preventDefault()
    e.stopPropagation()
    list[next]?.focus()
  }
</script>

<div
  bind:this={bar}
  bind:clientWidth={width}
  bind:clientHeight={height}
  class="bar"
  class:below
  role="toolbar"
  aria-label={t.annotations.bar}
  aria-orientation="horizontal"
  tabindex="-1"
  style:left="{left}px"
  style:top="{top}px"
  style:--tail="{tail}px"
  transition:fade={{ duration: MOTION.selectionBar }}
  {onkeydown}
  onpointerdown={(e) => e.preventDefault()}
>
  {#if mode === 'attach'}
    <button type="button" class="action strong" onclick={onattach}
      >{t.annotations.attachHere}</button
    >
    <button type="button" class="action" tabindex="-1" onclick={oncancel}
      >{t.annotations.cancel}</button
    >
  {:else}
    {#each COLORS as c, i (c)}
      <button
        type="button"
        class="action dot"
        class:current={c === color}
        tabindex={i === 0 ? 0 : -1}
        aria-label={mode === 'new'
          ? t.annotations.highlightIn(COLOR_NAMES[c], c === color)
          : t.annotations.colorOption(COLOR_NAMES[c], c === color)}
        aria-pressed={mode === 'existing' ? c === color : undefined}
        onclick={() => onhighlight(c)}
      >
        <span style:background="var(--hl-{c}-swatch)"></span>
      </button>
    {/each}
    <span class="divider" aria-hidden="true"></span>
    <button type="button" class="action" tabindex="-1" onclick={onnote}>
      <Icon name="note" size={16} />{t.annotations.note}
    </button>
    <button type="button" class="action" tabindex="-1" onclick={oncopy}>
      <Icon name="copy" size={16} />{t.annotations.copy}
    </button>
    {#if mode === 'new'}
      <button type="button" class="action" tabindex="-1" onclick={onsearch}>
        <Icon name="search" size={16} />{t.annotations.search}
      </button>
    {:else}
      <button type="button" class="action" tabindex="-1" onclick={ondelete}>
        <Icon name="trash" size={16} />{t.annotations.delete}
      </button>
    {/if}
  {/if}
</div>

<style>
  .bar {
    position: fixed;
    z-index: 40;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 5px;
    background: var(--selbar-ground);
    color: var(--selbar-ink);
    border-radius: 10px;
    box-shadow: 0 10px 28px rgba(40, 30, 20, 0.24);
    white-space: nowrap;
    font: 500 13px var(--font-ui);
    line-height: 1;
  }
  :global([data-theme='night']) .bar {
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5);
  }
  .bar::after {
    content: '';
    position: absolute;
    left: calc(var(--tail) - 5px);
    bottom: -5px;
    width: 10px;
    height: 10px;
    background: var(--selbar-ground);
    transform: rotate(45deg);
  }
  .bar.below::after {
    bottom: auto;
    top: -5px;
  }
  .action {
    height: 32px;
    min-width: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 0 9px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
    position: relative;
    z-index: 1;
  }
  .action:hover {
    background: var(--selbar-hover);
  }
  .action:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .strong {
    font-weight: 600;
  }
  .dot {
    padding: 0;
    width: 30px;
  }
  .dot span {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    display: block;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.14);
  }
  .dot.current {
    box-shadow: inset 0 0 0 1.5px var(--selbar-ring);
  }
  .divider {
    width: 1px;
    height: 20px;
    background: var(--selbar-divider);
    margin: 0 4px;
  }
  @media (forced-colors: active) {
    .bar {
      border: 1px solid CanvasText;
    }
    .dot.current {
      outline: 2px solid CanvasText;
    }
  }
</style>
