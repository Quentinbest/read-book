<script lang="ts">
  // The selection bar (A1–A3, A5; Screens 06 and 14). It sits 12 px above the
  // first selected line and flips below when there is less than 56 px above; it
  // never covers the selection. Ink-coloured, inverted to light at Night (V2).
  // F6 moves focus in, arrows move between actions, Esc returns to the text.
  // Extension actions (P10; G7) sit behind “⋯”, after the core actions, which
  // render at once; a stuck one is marked “Not responding” with Restart (Screen 12).
  // Reading Lens: lookups (LK1) lead that menu, for a new selection and for a clicked
  // highlight alike (LK14), where Look Up follows Delete, which keeps its place.
  import { fade } from 'svelte/transition'
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings'
  import { MOTION } from '../lib/theme/tokens'
  import { COLORS } from './annotations.svelte'
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
    onlookup,
    lookups = [],
    onlookupwith,
    ondelete,
    onescape,
    onattach,
    oncancel,
    extensionActions = [],
    onextension,
    onrestart,
    onmanage,
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
    /** 1.1: the dictionary peek. */
    onlookup?: () => void
    /** LK1: extension lookups whose `when` holds. */
    lookups?: { key: string; title: string; extId: string; name: string; status: string }[]
    onlookupwith?: (key: string) => void
    ondelete: () => void
    onescape: () => void
    onattach?: () => void
    oncancel?: () => void
    extensionActions?: {
      extId: string
      command: string
      title: string
      name: string
      status: string
    }[]
    onextension?: (a: { extId: string; command: string; name: string }) => void
    onrestart?: (extId: string) => void
    onmanage?: () => void
  } = $props()

  let menuOpen = $state(false)
  const stuck = (status: string) => status === 'not-responding' || status === 'suspended'

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
          ? t.annotations.highlightIn(t.colorsInText[c], c === color)
          : t.annotations.colorOption(t.colors[c], c === color)}
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
    {#if onlookup}
      <button type="button" class="action" tabindex="-1" onclick={onlookup}>
        <Icon name="dictionary" size={16} />{t.lookUp.action}
      </button>
    {/if}
    {@const actions = mode === 'new' ? extensionActions : []}
    {#if lookups.length || actions.length}
      <span class="divider" aria-hidden="true"></span>
      <button
        type="button"
        class="action more"
        tabindex="-1"
        aria-label={t.extensions.more}
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        onclick={() => {
          menuOpen = !menuOpen
          if (menuOpen)
            requestAnimationFrame(() =>
              bar?.querySelector<HTMLButtonElement>('.ext-menu button:not(:disabled)')?.focus(),
            )
        }}><Icon name="more" size={18} /></button
      >
      {#if menuOpen}
        <div class="ext-menu" class:up={below} role="menu" aria-label={t.extensions.more}>
          {#each lookups as l (l.key)}
            <button
              type="button"
              role="menuitem"
              class="item"
              data-lookup-item={l.key}
              disabled={stuck(l.status)}
              onclick={() => {
                menuOpen = false
                onlookupwith?.(l.key)
              }}
              ><span>{l.title}</span>{#if stuck(l.status)}<span class="stuck"
                  >{t.extensions.notResponding}</span
                >{/if}</button
            >
            {#if stuck(l.status)}
              <button
                type="button"
                role="menuitem"
                class="item restart"
                onclick={() => {
                  menuOpen = false
                  onrestart?.(l.extId)
                }}>{t.extensions.restartNamed(l.name)}</button
              >
            {/if}
          {/each}
          {#if lookups.length && actions.length}
            <span class="sep" aria-hidden="true"></span>
          {/if}
          {#each actions as a (a.extId + a.command)}
            <button
              type="button"
              role="menuitem"
              class="item"
              disabled={stuck(a.status)}
              onclick={() => {
                menuOpen = false
                onextension?.(a)
              }}
              ><span>{a.title}</span>{#if stuck(a.status)}<span class="stuck"
                  >{t.extensions.notResponding}</span
                >{/if}</button
            >
            {#if stuck(a.status)}
              <button
                type="button"
                role="menuitem"
                class="item restart"
                onclick={() => {
                  menuOpen = false
                  onrestart?.(a.extId)
                }}>{t.extensions.restartNamed(a.name)}</button
              >
            {/if}
          {/each}
          <span class="sep" aria-hidden="true"></span>
          <button
            type="button"
            role="menuitem"
            class="item"
            onclick={() => {
              menuOpen = false
              onmanage?.()
            }}>{t.extensions.manage}</button
          >
        </div>
      {/if}
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
  /* Screen 12: extension actions in a menu on the bar's own surface. */
  .ext-menu {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    min-width: 290px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    background: var(--selbar-ground);
    color: var(--selbar-ink);
    border-radius: 10px;
    box-shadow: 0 10px 28px rgba(40, 30, 20, 0.24);
  }
  .ext-menu.up {
    top: auto;
    bottom: calc(100% + 8px);
  }
  .item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    min-height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: inherit;
    font: 500 13px var(--font-ui);
    text-align: left;
  }
  .item:hover:not(:disabled) {
    background: var(--selbar-hover);
  }
  .item:disabled {
    opacity: 0.7;
  }
  .item:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .stuck {
    font-size: 12px;
    font-weight: 400;
  }
  .restart {
    font-size: 12px;
    font-weight: 600;
    color: color-mix(in srgb, var(--selbar-ink) 55%, var(--accent));
  }
  .sep {
    height: 1px;
    margin: 6px 4px;
    background: var(--selbar-divider);
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
