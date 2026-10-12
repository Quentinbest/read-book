<script lang="ts">
  // The selection bar (A1–A3, A5; Screens 06 and 14). It sits 12 px above the
  // first selected line and flips below when there is less than 56 px above; it
  // never covers the selection. Ink-coloured, inverted to light at Night (V2).
  // F6 moves focus in, arrows move between actions, Esc returns to the text.
  // Extension actions (P10; G7) sit behind “⋯”, after the core actions, which
  // render at once; a stuck one is marked “Not responding” with Restart (Screen 12).
  // Reading Lens: lookups (LK1) lead that menu, for a new selection and for a clicked
  // highlight alike (LK14), where Look Up follows Delete, which keeps its place.
  // SB3 (docs/selection-bar-plan.md): Note, Copy, Search or Delete, and Look Up show their
  // icon only. SB2: every button that shows an icon has a tooltip, drawn here rather than
  // by `title`, on the side of the bar away from the selection. SB4: a lookup that names an
  // icon has a button after Look Up; “⋯” shows only with something in it.
  import { untrack } from 'svelte'
  import { fade } from 'svelte/transition'
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings'
  import { MOTION } from '../lib/theme/tokens'
  import { COLORS } from './annotations.svelte'
  import { tooltipTiming } from './barTooltip'
  import type { HighlightColor } from '../lib/annotations/model'
  import type { LookupIcon } from '../lib/extensions/types'

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
    shortcut,
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
    lookups?: {
      key: string
      title: string
      extId: string
      name: string
      status: string
      /** SB4: the icon of its button on the bar, when it names one. */
      icon?: LookupIcon | null
    }[]
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
    /** SB2: a command's current shortcut, for the tooltips (a remapped one included). */
    shortcut?: (command: string) => { label: string; aria: string } | null
  } = $props()

  let menuOpen = $state(false)
  const stuck = (status: string) => status === 'not-responding' || status === 'suspended'
  /** Extension actions are for a new selection only. */
  const menuActions = $derived(mode === 'new' ? extensionActions : [])
  /** SB4: the first lookup that names an icon and isn't stuck has a button after Look Up. */
  const buttonLookup = $derived(lookups.find((l) => l.icon && !stuck(l.status)) ?? null)
  /** The other lookups stay in “⋯”, a stuck one with Not responding and Restart. */
  const menuLookups = $derived(lookups.filter((l) => l !== buttonLookup))
  /** One Restart per stuck extension, after its last item (a lookup and an action may share one). */
  const restartAfter = $derived.by(() => {
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- local to the derivation
    const last = new Map<string, string>()
    for (const l of menuLookups) if (stuck(l.status)) last.set(l.extId, `l:${l.key}`)
    for (const a of menuActions) if (stuck(a.status)) last.set(a.extId, `a:${a.extId}/${a.command}`)
    return new Set(last.values())
  })

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

  // SB2: the tooltip shows for `tip`, the button the pointer rests on or the keyboard reached.
  let tip: HTMLButtonElement | null = $state(null)
  const tips = tooltipTiming<HTMLButtonElement>((b) => (tip = b))
  let hovered: HTMLButtonElement | null = null
  let tipWidth = $state(0)
  let tipHeight = $state(0)
  const TIP_GAP = 6
  const TIP_EDGE = 8
  const tipTarget = (n: EventTarget | null) =>
    n instanceof Element ? n.closest<HTMLButtonElement>('button[data-tip]') : null
  /** On the side of the bar away from the selection, unless only the other side has room. */
  const tipAt = $derived.by(() => {
    if (!tip || !bar) return null
    void [left, top] // follows the bar
    const b = tip.getBoundingClientRect()
    const r = bar.getBoundingClientRect()
    const above = r.top - TIP_GAP - tipHeight
    const under = r.bottom + TIP_GAP
    const fitsAbove = above >= TIP_EDGE
    const fitsUnder = under + tipHeight <= window.innerHeight - TIP_EDGE
    const y = below
      ? fitsUnder || !fitsAbove
        ? under
        : above
      : fitsAbove || !fitsUnder
        ? above
        : under
    const x = b.left + b.width / 2 - tipWidth / 2
    return { x: Math.max(TIP_EDGE, Math.min(window.innerWidth - tipWidth - TIP_EDGE, x)), y }
  })
  /** A tooltip's words: the action's name, then its shortcut when its command has one. */
  const tipText = (name: string, command?: string) => {
    const k = command ? shortcut?.(command) : null
    return k ? `${name} (${k.label})` : name
  }
  const ariaKeys = (command: string) => shortcut?.(command)?.aria
  /** Keyboard focus shows the focused button's tooltip at once. */
  function focusWithTip(b: HTMLButtonElement | undefined) {
    b?.focus()
    if (b?.dataset.tip) tips.focus(b)
    else tips.hide()
  }
  // The bar moved, changed or closed: its tooltip goes with it.
  $effect(() => {
    void [first, last, mode]
    untrack(() => tips.hide())
  })
  $effect(() => () => tips.hide())

  /** F6 (A2). */
  export function focusFirst() {
    focusWithTip(buttons()[0])
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
      tips.hide()
      onescape()
      return
    }
    if (next < 0) return
    e.preventDefault()
    e.stopPropagation()
    focusWithTip(list[next])
  }
</script>

<div
  bind:this={bar}
  bind:clientWidth={width}
  bind:clientHeight={height}
  class="bar"
  data-reader-layer
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
  onpointerover={(e) => {
    const b = tipTarget(e.target)
    if (b && b !== hovered) {
      hovered = b
      tips.enter(b)
    }
  }}
  onpointerout={(e) => {
    const b = tipTarget(e.target)
    if (b && tipTarget(e.relatedTarget) !== b) {
      hovered = null
      tips.leave()
    }
  }}
  onfocusout={(e) => {
    if (!bar?.contains(e.relatedTarget as Node | null)) tips.hide()
  }}
  onpointerdown={(e) => {
    tips.hide()
    e.preventDefault()
    // Nested (S4): a click in the bar but outside its open menu closes the menu only.
    const t = e.target as Element
    if (menuOpen && !t.closest('.ext-menu, .action.more')) menuOpen = false
  }}
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
      {@const name =
        mode === 'new'
          ? t.annotations.highlightIn(t.colorsInText[c], c === color)
          : t.annotations.colorOption(t.colors[c], c === color)}
      <button
        type="button"
        class="action dot"
        class:current={c === color}
        tabindex={i === 0 ? 0 : -1}
        aria-label={name}
        data-tip={name}
        aria-pressed={mode === 'existing' ? c === color : undefined}
        onclick={() => onhighlight(c)}
      >
        <span style:background="var(--hl-{c}-swatch)"></span>
      </button>
    {/each}
    <span class="divider" aria-hidden="true"></span>
    <!-- SB3: icons only; the name is the tooltip's words without the shortcut. -->
    <button
      type="button"
      class="action icon"
      tabindex="-1"
      aria-label={t.annotations.note}
      data-tip={tipText(t.annotations.note, mode === 'new' ? 'selection.note' : undefined)}
      aria-keyshortcuts={mode === 'new' ? ariaKeys('selection.note') : undefined}
      onclick={onnote}
    >
      <Icon name="note" size={18} />
    </button>
    <button
      type="button"
      class="action icon"
      tabindex="-1"
      aria-label={t.annotations.copy}
      data-tip={t.annotations.copy}
      onclick={oncopy}
    >
      <Icon name="copy" size={18} />
    </button>
    {#if mode === 'new'}
      <button
        type="button"
        class="action icon"
        tabindex="-1"
        aria-label={t.lookUp.searchBook}
        data-tip={t.lookUp.searchBook}
        onclick={onsearch}
      >
        <Icon name="search" size={18} />
      </button>
    {:else}
      <button
        type="button"
        class="action icon"
        tabindex="-1"
        aria-label={t.annotations.deleteHighlight}
        data-tip={t.annotations.deleteHighlight}
        onclick={ondelete}
      >
        <Icon name="trash" size={18} />
      </button>
    {/if}
    {#if onlookup || buttonLookup}
      <span class="divider" aria-hidden="true"></span>
    {/if}
    {#if onlookup}
      <button
        type="button"
        class="action icon"
        tabindex="-1"
        aria-label={t.lookUp.action}
        data-tip={tipText(t.lookUp.action, mode === 'new' ? 'selection.lookUp' : undefined)}
        aria-keyshortcuts={mode === 'new' ? ariaKeys('selection.lookUp') : undefined}
        onclick={onlookup}
      >
        <Icon name="dictionary" size={18} />
      </button>
    {/if}
    {#if buttonLookup?.icon}
      {@const l = buttonLookup}
      {@const name = t.lens.inContext(l.title)}
      <button
        type="button"
        class="action icon"
        tabindex="-1"
        data-lookup-item={l.key}
        aria-label={name}
        data-tip={name}
        onclick={() => onlookupwith?.(l.key)}
      >
        <Icon name={buttonLookup.icon} size={18} />
      </button>
    {/if}
    {#if menuLookups.length || menuActions.length}
      <span class="divider" aria-hidden="true"></span>
      <button
        type="button"
        class="action more"
        tabindex="-1"
        aria-label={t.extensions.more}
        data-tip={t.extensions.more}
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
          {#each menuLookups as l (l.key)}
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
            {#if restartAfter.has(`l:${l.key}`)}
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
          {#if menuLookups.length && menuActions.length}
            <span class="sep" aria-hidden="true"></span>
          {/if}
          {#each menuActions as a (a.extId + a.command)}
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
            {#if restartAfter.has(`a:${a.extId}/${a.command}`)}
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
  {#if tip}
    <div
      class="tip"
      aria-hidden="true"
      bind:clientWidth={tipWidth}
      bind:clientHeight={tipHeight}
      style:left="{tipAt?.x ?? 0}px"
      style:top="{tipAt?.y ?? 0}px"
      in:fade={{ duration: MOTION.selectionBar }}
    >
      {tip.dataset.tip}
    </div>
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
  /* Item 73 (O7, approved 2026-10-09): the accent ring was 2.24:1 on Paper's bar and 1.90:1 at
     Night, under 3:1; the bar's own ink holds 7:1 or more in every theme. */
  .action:focus-visible {
    outline: 2px solid var(--selbar-ink);
    outline-offset: 1px;
  }
  .strong {
    font-weight: 600;
  }
  .dot {
    padding: 0;
    width: 30px;
  }
  /* SB3: an icon alone on the 32 px face. */
  .icon {
    padding: 0;
    width: 32px;
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
    outline: 2px solid var(--selbar-ink);
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
  /* SB2: the tooltip colours, 7:1 or more in every theme (dark on Paper and Sepia, light at
     Night, like the bar). */
  .tip {
    position: fixed;
    z-index: 2;
    max-width: 320px;
    padding: 4px 8px;
    border-radius: 6px;
    background: var(--tooltip);
    color: var(--tooltip-ink);
    box-shadow: 0 4px 12px rgba(40, 30, 20, 0.2);
    font: 400 12px/1.3 var(--font-ui);
    /* Short names stay on one line; a long lookup title wraps rather than overflows. */
    white-space: normal;
    pointer-events: none;
  }
  @media (forced-colors: active) {
    .bar,
    .tip {
      border: 1px solid CanvasText;
    }
    .dot.current {
      outline: 2px solid CanvasText;
    }
  }
</style>
