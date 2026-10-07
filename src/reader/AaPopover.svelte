<script lang="ts">
  // Reading settings, the Aa popover (Screen 09). Each row names its scope: text
  // size, theme and line spacing apply to all books; layout to this book (B8).
  // 1.1 (approved 2026-10-01): font and page width rows, all books.
  // Fixed-layout books keep only theme and zoom (E2); books that are mostly code
  // or tables get a one-line hint to try Scroll (L18). ⌘+ ⌘− ⌘0 work anywhere.
  import { onMount } from 'svelte'
  import { t } from '../lib/strings'
  import type { ThemeChoice } from '../app/theme'
  import type { Spacing } from './layout'
  import { TEXT_SIZES } from './textSizes'
  import type { FontChoice, PageWidth } from './typography'
  import { THEMES, type Theme } from '../lib/theme/tokens'

  /** Each swatch shows its theme's own page and ink (Screen 09); Auto shows Paper and Night. */
  const swatch = (choice: ThemeChoice) => {
    const { paper, night } = THEMES
    if (choice === 'auto')
      return `background: linear-gradient(135deg, ${paper.ground} 50%, ${night.ground} 50%); color: ${paper.accent}`
    const th = choice.startsWith('ext:')
      ? packs.find((p) => p.value === choice)?.theme
      : THEMES[choice as keyof typeof THEMES]
    return th ? `background: ${th.ground}; color: ${th.ink}` : ''
  }

  let {
    anchor,
    fontPx,
    theme,
    spacing,
    layout,
    fixedLayout,
    codeHeavy,
    onfont,
    ontheme,
    onspacing,
    onlayout,
    onsettings,
    font,
    width,
    onfontchoice,
    onwidth,
    packs = [],
  }: {
    anchor: DOMRect
    fontPx: number
    theme: ThemeChoice
    spacing: Spacing
    layout: 'pages' | 'scroll'
    fixedLayout: boolean
    codeHeavy: boolean
    onfont: (px: number) => void
    ontheme: (t: ThemeChoice) => void
    onspacing: (s: Spacing) => void
    onlayout: (l: 'pages' | 'scroll') => void
    onsettings: () => void
    font: FontChoice
    width: PageWidth
    onfontchoice: (f: FontChoice) => void
    onwidth: (w: PageWidth) => void
    /** P9: theme packs from extensions, listed after the built-in themes. */
    packs?: { value: ThemeChoice; label: string; theme: Theme }[]
  } = $props()

  const WIDTH = 300
  let panel: HTMLElement | undefined = $state()
  const right = $derived(Math.max(12, window.innerWidth - anchor.right - 16))
  const left = $derived(window.innerWidth - right - WIDTH)
  /** Down to 12 px above the window's foot. */
  const maxHeight = $derived(Math.max(200, window.innerHeight - anchor.bottom - 8 - 12))
  const arrow = $derived(
    Math.max(16, Math.min(WIDTH - 26, anchor.left + anchor.width / 2 - left - 5)),
  )

  // The slider moves through the L4 steps; sizes set beyond them with ⌘+ show at the end.
  const step = $derived.by(() => {
    const i = TEXT_SIZES.findIndex((s) => s >= fontPx)
    return i < 0 ? TEXT_SIZES.length - 1 : i
  })

  const themes: { value: ThemeChoice; label: string }[] = $derived([
    { value: 'paper', label: t.aa.paper },
    { value: 'sepia', label: t.aa.sepia },
    { value: 'night', label: t.aa.night },
    { value: 'auto', label: t.aa.auto },
    ...packs.map((p) => ({ value: p.value, label: p.label })),
  ])
  const spacings: { value: Spacing; label: string }[] = [
    { value: 'compact', label: t.aa.compact },
    { value: 'default', label: t.aa.normal },
    { value: 'loose', label: t.aa.loose },
  ]
  const fonts: { value: FontChoice; label: string; title?: string }[] = [
    { value: 'book', label: t.aa.fontBook },
    { value: 'literata', label: t.aa.fontLiterata },
    { value: 'sans', label: t.aa.fontSans },
    { value: 'dyslexic', label: t.aa.fontDyslexic, title: t.aa.fontDyslexicLong },
  ]
  const widths: { value: PageWidth; label: string }[] = [
    { value: 'narrow', label: t.aa.narrow },
    { value: 'normal', label: t.aa.normal },
    { value: 'wide', label: t.aa.wide },
  ]
  const layouts: { value: 'pages' | 'scroll'; label: string }[] = [
    { value: 'pages', label: t.aa.pages },
    { value: 'scroll', label: t.aa.scroll },
  ]

  /** Radio groups: arrows move the choice, one tab stop per group. */
  function radioKeys<T>(e: KeyboardEvent, list: { value: T }[], current: T, set: (v: T) => void) {
    const d =
      e.key === 'ArrowRight' || e.key === 'ArrowDown'
        ? 1
        : e.key === 'ArrowLeft' || e.key === 'ArrowUp'
          ? -1
          : 0
    if (!d) return
    e.preventDefault()
    e.stopPropagation()
    const i = list.findIndex((x) => x.value === current)
    const next = list[(i + d + list.length) % list.length]
    set(next.value)
    requestAnimationFrame(() =>
      (e.currentTarget as HTMLElement | null)
        ?.querySelector<HTMLElement>('[aria-checked="true"]')
        ?.focus(),
    )
  }

  export function contains(node: Node | null) {
    return !!node && !!panel?.contains(node)
  }

  onMount(() => panel?.querySelector<HTMLElement>('input, [aria-checked="true"]')?.focus())
</script>

<div
  bind:this={panel}
  class="aa"
  role="dialog"
  aria-label={t.aa.label}
  style:right="{right}px"
  style:top="{anchor.bottom + 8}px"
  style:--arrow="{arrow}px"
  style:max-height="{maxHeight}px"
>
  <!-- In a short window the rows scroll; the arrow and the foot stay put. -->
  <div class="rows">
    {#if !fixedLayout}
      <div class="row">
        <div class="lbl">
          <label for="aa-size">{t.aa.textSize}</label><span>{t.aa.sizeScope(fontPx)}</span>
        </div>
        <div class="size">
          <button
            type="button"
            class="step small"
            aria-label={t.commands['text.smaller']}
            disabled={step === 0 && fontPx <= TEXT_SIZES[0]}
            onclick={() => onfont(TEXT_SIZES[Math.max(0, step - 1)])}>A</button
          >
          <input
            id="aa-size"
            type="range"
            min="0"
            max={TEXT_SIZES.length - 1}
            step="1"
            value={step}
            aria-valuetext={t.aa.px(fontPx)}
            oninput={(e) => onfont(TEXT_SIZES[Number(e.currentTarget.value)])}
            onkeydown={(e) => e.stopPropagation()}
          />
          <button
            type="button"
            class="step large"
            aria-label={t.commands['text.larger']}
            disabled={step === TEXT_SIZES.length - 1}
            onclick={() => onfont(TEXT_SIZES[Math.min(TEXT_SIZES.length - 1, step + 1)])}>A</button
          >
        </div>
      </div>
    {/if}
    <div class="row">
      <div class="lbl" id="aa-theme">{t.aa.theme}<span>{t.aa.allBooks}</span></div>
      <div
        class="swatches"
        role="radiogroup"
        aria-labelledby="aa-theme"
        tabindex="-1"
        onkeydown={(e) => radioKeys(e, themes, theme, ontheme)}
      >
        {#each themes as th (th.value)}
          <button
            type="button"
            role="radio"
            class="sw {th.value}"
            aria-checked={theme === th.value}
            tabindex={theme === th.value ? 0 : -1}
            onclick={() => ontheme(th.value)}
          >
            <i aria-hidden="true" style={swatch(th.value)}>Aa</i>{th.label}
          </button>
        {/each}
      </div>
    </div>
    {#if !fixedLayout}
      <div class="row">
        <div class="lbl" id="aa-spacing">{t.aa.lineSpacing}<span>{t.aa.allBooks}</span></div>
        <div
          class="seg three"
          role="radiogroup"
          aria-labelledby="aa-spacing"
          tabindex="-1"
          onkeydown={(e) => radioKeys(e, spacings, spacing, onspacing)}
        >
          {#each spacings as s (s.value)}
            <button
              type="button"
              role="radio"
              aria-checked={spacing === s.value}
              tabindex={spacing === s.value ? 0 : -1}
              onclick={() => onspacing(s.value)}>{s.label}</button
            >
          {/each}
        </div>
      </div>
      <div class="row">
        <div class="lbl" id="aa-font">{t.aa.font}<span>{t.aa.allBooks}</span></div>
        <div
          class="seg four"
          role="radiogroup"
          aria-labelledby="aa-font"
          tabindex="-1"
          onkeydown={(e) => radioKeys(e, fonts, font, onfontchoice)}
        >
          {#each fonts as f (f.value)}
            <button
              type="button"
              role="radio"
              class="font-{f.value}"
              title={f.title}
              aria-checked={font === f.value}
              tabindex={font === f.value ? 0 : -1}
              onclick={() => onfontchoice(f.value)}>{f.label}</button
            >
          {/each}
        </div>
      </div>
      <div class="row">
        <div class="lbl" id="aa-width">{t.aa.width}<span>{t.aa.allBooks}</span></div>
        <div
          class="seg three"
          role="radiogroup"
          aria-labelledby="aa-width"
          tabindex="-1"
          onkeydown={(e) => radioKeys(e, widths, width, onwidth)}
        >
          {#each widths as w (w.value)}
            <button
              type="button"
              role="radio"
              aria-checked={width === w.value}
              tabindex={width === w.value ? 0 : -1}
              onclick={() => onwidth(w.value)}>{w.label}</button
            >
          {/each}
        </div>
      </div>
      <div class="row">
        <div class="lbl" id="aa-layout">{t.aa.layout}<span>{t.aa.thisBook}</span></div>
        <div
          class="seg two"
          role="radiogroup"
          aria-labelledby="aa-layout"
          tabindex="-1"
          onkeydown={(e) => radioKeys(e, layouts, layout, onlayout)}
        >
          {#each layouts as l (l.value)}
            <button
              type="button"
              role="radio"
              aria-checked={layout === l.value}
              tabindex={layout === l.value ? 0 : -1}
              onclick={() => onlayout(l.value)}>{l.label}</button
            >
          {/each}
        </div>
        {#if codeHeavy && layout === 'pages'}
          <!-- L18 (G12, provisional) -->
          <p class="hint">{t.aa.codeHint}</p>
        {/if}
      </div>
    {:else}
      <!-- E2 (G12, provisional) -->
      <p class="hint">{t.aa.fixedLayout}</p>
    {/if}
  </div>
  <div class="foot">
    <button type="button" class="link" onclick={onsettings}>{t.aa.moreSettings}</button>
    <span>{t.aa.anywhere}</span>
  </div>
</div>

<style>
  /* Screen 09: 300 px on the popover surface, under the Aa button. */
  .aa {
    position: fixed;
    z-index: 30;
    width: 300px;
    box-sizing: border-box;
    padding: 18px 18px 14px;
    display: flex;
    flex-direction: column;
    gap: 18px;
    background: var(--popover);
    border: 1px solid var(--hairline);
    border-radius: 12px;
    box-shadow: 0 12px 32px rgba(40, 30, 20, 0.14);
    color: var(--ink);
    font-family: var(--font-ui);
    animation: pop-in var(--motion-popover) ease-out;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
    overflow-y: auto;
    /* Room for the focus rings inside the scrolling box. */
    margin: -4px;
    padding: 4px;
  }
  .aa::before {
    content: '';
    position: absolute;
    top: -6px;
    left: var(--arrow);
    width: 10px;
    height: 10px;
    background: var(--popover);
    border-left: 1px solid var(--hairline);
    border-top: 1px solid var(--hairline);
    transform: rotate(45deg);
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
  }
  .lbl {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    font-size: 12px;
    font-weight: 600;
    color: var(--ink);
    margin-bottom: 8px;
  }
  .lbl span {
    font-weight: 500;
    color: var(--ink-secondary);
  }
  .size {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .size input {
    flex-grow: 1;
    accent-color: var(--accent);
    margin: 0;
  }
  .step {
    min-width: 32px;
    height: 32px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--ink);
    font-family: var(--font-reading, Literata, Georgia, serif);
    cursor: pointer;
  }
  .step.small {
    font-size: 13px;
  }
  .step.large {
    font-size: 20px;
  }
  .step:hover:not(:disabled) {
    background: var(--hover-wash);
  }
  .step:disabled {
    color: var(--ink-secondary);
    cursor: default;
  }
  .swatches {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }
  .sw {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    border: 0;
    background: transparent;
    padding: 0;
    cursor: pointer;
    font: 500 12px var(--font-ui);
    color: var(--ink-secondary);
    border-radius: 10px;
  }
  .sw i {
    width: 58px;
    height: 40px;
    border-radius: 9px;
    display: flex;
    align-items: center;
    justify-content: center;
    font: 500 16px var(--font-reading, Literata, Georgia, serif);
    font-style: normal;
    box-shadow: inset 0 0 0 1px rgba(34, 32, 28, 0.12);
  }
  .sw[aria-checked='true'] i {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .seg {
    display: grid;
    gap: 2px;
    padding: 3px;
    background: var(--control-track);
    border-radius: 9px;
  }
  .seg.three {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
  .seg.four {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
  .seg .font-literata {
    font-family: var(--font-reading, Literata, Georgia, serif);
  }
  .seg .font-sans {
    font-family: -apple-system, 'Helvetica Neue', sans-serif;
  }
  .seg.two {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .seg button {
    height: 30px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    font: 500 13px var(--font-ui);
    color: var(--track-ink);
    cursor: pointer;
  }
  .seg button[aria-checked='true'] {
    background: var(--raised);
    color: var(--ink);
    font-weight: 600;
    box-shadow: 0 0 0 1px var(--segment-ring);
  }
  .sw:focus-visible,
  .seg button:focus-visible,
  .step:focus-visible,
  .link:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .hint {
    margin: 8px 0 0;
    font-size: 12px;
    line-height: 1.4;
    color: var(--ink-secondary);
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-top: 1px solid var(--hairline);
    padding-top: 12px;
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent);
    font: 600 12px var(--font-ui);
    cursor: pointer;
  }
  @media (forced-colors: active) {
    .seg button[aria-checked='true'],
    .sw[aria-checked='true'] i {
      outline: 2px solid CanvasText;
    }
  }
</style>
