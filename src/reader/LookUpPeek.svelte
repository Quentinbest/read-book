<script lang="ts" module>
  import type { LookupResult } from '../lib/lookup/result'

  /** What the peek shows (Canvas 2–11). */
  export type PeekState =
    | { kind: 'pending' }
    /** 1.1's Mac dictionaries: undefined while looking up, null when none has the word. */
    | { kind: 'mac'; definition: string | null | undefined; noEntry?: boolean }
    /** Stage 2b: an entry from one of the reader's dictionaries, in its own frame (DX6). */
    | { kind: 'dict'; url: string; title: string }
    | { kind: 'answer'; result: LookupResult }
    | {
        kind: 'error'
        /** LK10: a provider's report, or what the core saw (timeout, a stopped or suspended one, an answer it refused). */
        error:
          | 'offline'
          | 'unauthorized'
          | 'rate_limited'
          | 'unavailable'
          | 'timeout'
          | 'stopped'
          | 'bad'
          /** LK7: a saved key Linen couldn't read (a locked Keychain, denied access). */
          | 'keychain'
      }
</script>

<script lang="ts">
  // The lookup peek (Reading Lens LK1–LK3, LK10–LK15, EA1–EA3; Canvas 2–11). One
  // layer at a time (S2) on the popover surface, solid (Decision 10-09). It opens
  // below the selection's last line with a pointer at it, or above its first line
  // near the page foot, and never covers the selected lines (EQ5). Its height is at
  // most half the window; when neither side has room it takes the side with more,
  // minus 16 px, and the answer scrolls (LK13).
  import { t } from '../lib/strings'
  import type { SelectionContext } from '../lib/lookup/context'
  import { providerMenu, sourceLabel, type LookupProvider } from '../lib/lookup/providers'

  let {
    word,
    wordLang,
    sourceLanguage,
    context,
    first,
    last,
    providers,
    current,
    shown,
    onprovider,
    oncopy,
    onsearch,
    onopen,
    onretry,
    onrestart,
    oncontinue,
    onnotnow,
    onexplain,
    explainTitle,
    onoptions,
  }: {
    word: string
    /** The book's language, for the word and the quoted sentence (EA2). */
    wordLang: string | undefined
    /** The book's language, named in the UI's (“English”). */
    sourceLanguage: string
    context: SelectionContext | null
    first: DOMRect
    last: DOMRect
    providers: LookupProvider[]
    current: string
    shown: PeekState
    onprovider: (key: string) => void
    oncopy: (text: string) => void
    onsearch: () => void
    /** The Mac's dictionaries: Open in Dictionary. */
    onopen: () => void
    onretry: () => void
    onrestart: () => void
    /** Item 76: the first-request notice's buttons. */
    oncontinue: () => void
    onnotnow: () => void
    /** DX14, Canvas 11: “Explain in context”, when an extension lookup applies. */
    onexplain?: () => void
    /** The lookup “… in context” opens (its own title, so a dictionary lookup isn't called Explain). */
    explainTitle?: string
    /** LK8: a key the provider refused; the extension's options page, when it has one. */
    onoptions?: () => void
  } = $props()

  const GAP = 12
  const EDGE = 16
  const POINTER = 7

  let peek: HTMLElement | undefined = $state()
  let answer: HTMLElement | undefined = $state()
  let natural = $state(0)
  let view = $state<'main' | 'more' | 'sent'>('main')
  let menuOpen = $state(false)
  /** DX5: entry links navigate the frame; Back returns to the entry the peek opened on. */
  let frameLoads = $state(0)
  let frameKey = $state(0)
  /** Where the menu hangs, under its button (it sits on the peek, outside the scrolling answer). */
  let menuTop = $state(36)
  let winW = $state(innerWidth)
  let winH = $state(innerHeight)

  const provider = $derived(providers.find((p) => p.key === current) ?? providers[0])
  const menu = $derived(providerMenu(providers, current))
  const currentLabel = $derived(menu.items.find((i) => i.checked)?.label ?? provider?.title ?? '')
  const name = $derived(provider?.name ?? provider?.title ?? '')
  const result = $derived(shown.kind === 'answer' ? shown.result : null)
  const label = $derived(provider ? sourceLabel(result, provider) : '')
  /** EA2: an explanation carries the language it is written in. */
  const answerLang = $derived(provider?.kind === 'extension' ? provider.language : wordLang)

  // ---- Placement (LK1, LK13)
  const width = $derived(Math.min(440, winW - 2 * EDGE))
  const cap = $derived(Math.round(winH * 0.5))
  const roomBelow = $derived(winH - (last.bottom + GAP + POINTER) - EDGE)
  const roomAbove = $derived(first.top - GAP - POINTER - EDGE)
  const want = $derived(Math.min(natural || 160, cap))
  const below = $derived(roomBelow >= want || (roomAbove < want && roomBelow >= roomAbove))
  const maxHeight = $derived(
    Math.max(
      80,
      Math.min(
        cap,
        (below ? roomBelow : roomAbove) - (roomBelow >= want || roomAbove >= want ? 0 : 16),
      ),
    ),
  )
  const anchorX = $derived(below ? last.left : first.left)
  const left = $derived(Math.max(EDGE, Math.min(winW - width - EDGE, anchorX - 20)))
  const pointerX = $derived(Math.max(14, Math.min(width - 26, anchorX - left + 4)))

  /** The peek's height with its answer unscrolled, measured after each change. */
  function measure() {
    if (!peek || !answer) return
    natural = peek.offsetHeight - answer.clientHeight + answer.scrollHeight
  }
  $effect(() => {
    void shown
    void view
    void width
    requestAnimationFrame(measure)
  })
  $effect(() => {
    // A new answer starts on its first layer.
    void current
    view = 'main'
    menuOpen = false
  })

  /** EA1: Tab from the selection enters the peek. */
  export function focusFirst() {
    peek?.querySelector<HTMLElement>('button, [tabindex="0"]')?.focus()
  }

  export function contains(node: Node | null): boolean {
    return !!node && !!peek?.contains(node)
  }

  // ---- Text
  /** The system's plain text runs senses together; numbered senses start their own line. */
  const paragraphs = $derived(
    shown.kind === 'mac'
      ? (shown.definition ?? '')
          .replace(/\s+(?=\d{1,2} (?=[a-z(]))/g, '\n')
          .split('\n')
          .map((p) => p.trim())
          .filter(Boolean)
      : [],
  )
  const copyText = $derived(
    result
      ? [
          result.term,
          result.meaning,
          result.qualifier,
          ...(view === 'more' ? (result.details ?? []).map((d) => `${d.label}: ${d.text}`) : []),
        ]
          .filter(Boolean)
          .join('\n')
      : '',
  )
  const errorText = $derived.by((): [string, string] => {
    if (shown.kind !== 'error') return ['', '']
    switch (shown.error) {
      case 'offline':
        return [t.lens.offline, t.lens.offlineDetail(name)]
      case 'timeout':
        return [t.lens.timeout(name), '']
      case 'unauthorized':
        return [t.lens.unauthorized(name), t.lens.unauthorizedDetail]
      case 'keychain':
        return [t.extAccess.keyUnavailable(name), t.extAccess.keyUnavailableDetail]
      case 'rate_limited':
        return [t.lens.rateLimited(name), t.lens.rateLimitedDetail]
      case 'unavailable':
        return [t.lens.unavailable(name), '']
      case 'bad':
        return [t.lens.badAnswer(name), '']
      case 'stopped':
        return [t.lens.stopped(name), t.lens.stoppedDetail]
    }
  })
  const dialogLabel = $derived(
    provider?.kind === 'extension' && result?.source.kind !== 'dictionary'
      ? t.lens.explanationOf(word)
      : t.lens.lookUpWord(word),
  )

  /** The notice's text with its host set apart (Canvas 8). */
  function noticeParts(text: string, host: string): string[] {
    return text.split(new RegExp(`(${host.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`))
  }

  function menuKey(e: KeyboardEvent) {
    const items = Array.from(
      peek?.querySelectorAll<HTMLElement>('.menu [role="menuitemradio"]') ?? [],
    )
    const i = items.indexOf(document.activeElement as HTMLElement)
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const n = items.length
      items[(i + (e.key === 'ArrowDown' ? 1 : n - 1) + n) % n]?.focus()
    } else if (e.key === 'Escape') {
      // Esc closes the menu first, then the peek (S4: one level at a time).
      e.preventDefault()
      e.stopPropagation()
      menuOpen = false
      peek?.querySelector<HTMLElement>('.provider')?.focus()
    }
  }
</script>

<svelte:window bind:innerWidth={winW} bind:innerHeight={winH} />

<div
  class="peek"
  class:above={!below}
  bind:this={peek}
  role="dialog"
  aria-label={dialogLabel}
  data-lookup
  data-provider={current}
  data-state={shown.kind}
  style:width="{width}px"
  style:left="{left}px"
  style:max-height="{maxHeight}px"
  style:top={below ? `${last.bottom + GAP + POINTER}px` : undefined}
  style:bottom={below ? undefined : `${winH - first.top + GAP + POINTER}px`}
  style:--pointer="{pointerX}px"
>
  <div class="source">
    <span class="meta">{sourceLanguage}</span>
    <span class="word" lang={wordLang}>{word}</span>
  </div>
  <div class="hairline" aria-hidden="true"></div>

  <div class="answer" bind:this={answer} tabindex="-1">
    <div class="row">
      {#if menu.plain}
        <span class="provider plain" data-provider-label>{currentLabel}</span>
      {:else}
        <button
          type="button"
          class="provider"
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          data-provider-label
          onclick={(e) => {
            const b = (e.currentTarget as HTMLElement).getBoundingClientRect()
            menuTop = b.bottom - (peek?.getBoundingClientRect().top ?? 0) + 4
            menuOpen = !menuOpen
            if (menuOpen)
              requestAnimationFrame(() =>
                peek?.querySelector<HTMLElement>('.menu [aria-checked="true"]')?.focus(),
              )
          }}
          >{currentLabel}<svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M6 9l6 6 6-6" /></svg
          ></button
        >
      {/if}
      {#if label && shown.kind !== 'pending' && shown.kind !== 'error' && result?.status !== 'notice'}
        <span class="meta label" data-source-label>{label}</span>
      {/if}
    </div>

    <div class="body" aria-live="polite">
      {#if shown.kind === 'pending'}
        {#if context}
          <p class="quote" lang={wordLang}>
            {context.sentence.slice(0, context.selection.start)}<span class="selected"
              >{context.sentence.slice(context.selection.start, context.selection.end)}</span
            >{context.sentence.slice(context.selection.end)}
          </p>
        {/if}
        <p class="asking"><span class="progress" aria-hidden="true"></span>{t.lens.asking(name)}</p>
      {:else if shown.kind === 'dict'}
        <!-- DX6, DX7: sanitised, no scripts, on the light card; text is selectable. -->
        {#key `${shown.url}#${frameKey}`}
          <iframe
            class="entry"
            src={shown.url}
            title={t.lens.dictionaryFrame(shown.title)}
            sandbox=""
            referrerpolicy="no-referrer"
            data-dict-frame
            onload={() => (frameLoads += 1)}
          ></iframe>
        {/key}
      {:else if shown.kind === 'mac'}
        {#if shown.definition === undefined}
          <p class="missing">{t.lookUp.looking}</p>
        {:else if shown.definition === null}
          <p class="missing" data-no-entry>
            {shown.noEntry ? t.lens.noEntry(word) : t.lookUp.none(word)}
          </p>
        {:else}
          <div class="definition" lang={wordLang}>
            {#each paragraphs as p, i (i)}<p>{p}</p>{/each}
          </div>
        {/if}
      {:else if shown.kind === 'error'}
        <p class="error"><strong>{errorText[0]}</strong> {errorText[1]}</p>
      {:else if result && view === 'sent'}
        <p class="meta">{t.lens.sentBy(name)}</p>
        <pre class="sent" lang={wordLang}>{result.sent}</pre>
      {:else if result?.status === 'notice' && result.notice}
        <!-- Item 76 (EX8): the provider's notice; only Continue sends anything. -->
        <div class="notice" data-notice>
          <p class="notice-title">{result.notice.title}</p>
          <p class="notice-text">
            {#each noticeParts(result.notice.text, result.notice.host) as part, i (i)}{#if part === result.notice.host}<strong
                  >{part}</strong
                >{:else}{part}{/if}{/each}
          </p>
        </div>
      {:else if result}
        <div lang={answerLang}>
          {#if result.status === 'needs_context'}
            {#if result.term}<p class="term">{result.term}</p>{/if}
            <p class="qualifier" data-missing>{result.missing}</p>
            {#if result.meaning}<p class="meaning">{result.meaning}</p>{/if}
          {:else}
            {#if result.term}<p class="term">{result.term}</p>{/if}
            <p class="meaning">{result.meaning}</p>
            {#if result.qualifier}<p class="qualifier" data-qualifier>{result.qualifier}</p>{/if}
            {#if view === 'more' && result.details?.length}
              <dl class="details">
                {#each result.details as d, i (i)}
                  <dt>{d.label}</dt>
                  <dd>{d.text}</dd>
                {/each}
              </dl>
            {/if}
          {/if}
        </div>
      {/if}
    </div>
  </div>

  {#if shown.kind !== 'pending'}
    <div class="footer">
      {#if shown.kind === 'dict'}
        {#if frameLoads > 1}
          <button
            type="button"
            onclick={() => {
              frameLoads = 0
              frameKey += 1
            }}>{t.lens.back}</button
          >
        {/if}
        {#if onexplain}
          <button type="button" onclick={onexplain}>{t.lens.inContext(explainTitle ?? '')}</button>
        {/if}
        <button type="button" class="link" onclick={onsearch}>{t.lookUp.searchBook}</button>
      {:else if shown.kind === 'mac'}
        {#if shown.definition !== null}
          <button type="button" onclick={onopen}>{t.lookUp.openDictionary}</button>
        {/if}
        {#if onexplain}
          <button type="button" onclick={onexplain}>{t.lens.inContext(explainTitle ?? '')}</button>
        {/if}
        <button type="button" class="link" onclick={onsearch}>{t.lookUp.searchBook}</button>
      {:else if shown.kind === 'error'}
        {#if shown.error === 'stopped'}
          <button type="button" onclick={onrestart}>{t.lens.restart(name)}</button>
        {:else if shown.error !== 'unauthorized' && shown.error !== 'keychain'}
          <button type="button" onclick={onretry}>{t.lens.tryAgain}</button>
        {:else if onoptions}
          <button type="button" data-open-options onclick={onoptions}
            >{t.extAccess.openOptions}</button
          >
        {/if}
      {:else if result && view === 'sent'}
        <button type="button" onclick={() => (view = 'main')}>{t.lens.back}</button>
      {:else if result?.status === 'notice'}
        <button type="button" class="primary" onclick={oncontinue}>{t.lens.continue}</button>
        <button type="button" onclick={onnotnow}>{t.lens.notNow}</button>
      {:else if result?.status === 'needs_context'}
        <button type="button" onclick={onsearch}>{t.lens.searchInBook}</button>
      {:else if result}
        <button type="button" onclick={() => oncopy(copyText)}>{t.lens.copyExplanation}</button>
        {#if result.details?.length}
          <button
            type="button"
            aria-expanded={view === 'more'}
            onclick={() => (view = view === 'more' ? 'main' : 'more')}
            >{view === 'more' ? t.lens.less : t.lens.more}</button
          >
        {/if}
        {#if result.sent}
          <button type="button" class="link sent-link" onclick={() => (view = 'sent')}
            >{t.lens.whatWasSent}</button
          >
        {/if}
      {/if}
    </div>
  {/if}
  {#if menuOpen}
    <div
      class="menu"
      style:top="{menuTop}px"
      role="menu"
      aria-label={t.lens.lookUpWith}
      tabindex="-1"
      onkeydown={menuKey}
    >
      {#each menu.items as item, i (item.key)}
        {#if i > 0 && menu.items[i - 1].group !== item.group}
          <span class="sep" aria-hidden="true"></span>
        {/if}
        <button
          type="button"
          role="menuitemradio"
          aria-checked={item.checked}
          tabindex="-1"
          onclick={() => {
            menuOpen = false
            if (!item.checked) onprovider(item.key)
          }}
        >
          <svg class="check" width="16" height="16" viewBox="0 0 24 24" aria-hidden="true"
            >{#if item.checked}<path d="M5 12.5l4.5 4.5L19 7.5" />{/if}</svg
          >{item.label}
        </button>
      {/each}
    </div>
  {/if}
  <span class="pointer" aria-hidden="true"></span>
</div>

<style>
  /* Canvas 2–11 in Linen's tokens: solid popover surface, no blur (Decision 10-09). */
  .peek {
    position: fixed;
    z-index: 30;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    border-radius: 10px;
    background: var(--popover);
    border: 1px solid var(--popover-border);
    box-shadow: 0 12px 32px rgb(40 30 20 / 14%);
    color: var(--ink);
    font: 400 13px/1.45 var(--font-ui);
    animation: pop-in var(--motion-popover, 140ms) ease-out;
  }
  :global([data-theme='night']) .peek {
    box-shadow: 0 12px 32px rgb(0 0 0 / 50%);
  }
  @keyframes pop-in {
    from {
      opacity: 0;
    }
  }
  .pointer {
    position: absolute;
    left: var(--pointer);
    top: -7px;
    width: 12px;
    height: 12px;
    background: var(--popover);
    border-left: 1px solid var(--popover-border);
    border-top: 1px solid var(--popover-border);
    transform: rotate(45deg);
  }
  .above .pointer {
    top: auto;
    bottom: -7px;
    transform: rotate(225deg);
  }
  .source {
    display: flex;
    flex-direction: column;
    padding: 12px 16px;
    flex: none;
  }
  .meta {
    font-size: 12px;
    font-weight: 500;
    color: var(--ink-secondary);
  }
  .word {
    margin-top: 2px;
    font: 600 19px/1.35 var(--font-reading, Literata, Georgia, serif);
    overflow-wrap: anywhere;
  }
  .hairline {
    flex: none;
    height: 1px;
    margin: 0 16px;
    background: var(--hairline);
  }
  .answer {
    position: relative;
    min-height: 0;
    overflow-y: auto;
    padding: 10px 16px 14px;
    outline: none;
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }
  .provider {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 6px;
    margin-left: -6px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--ink-secondary);
    font: 500 12px var(--font-ui);
  }
  .provider.plain {
    padding: 0;
    margin: 0;
  }
  button.provider:hover,
  button.provider[aria-expanded='true'] {
    background: var(--hover-wash);
    color: var(--ink);
  }
  .provider svg,
  .check {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .label {
    text-align: right;
  }
  .menu {
    position: absolute;
    left: 10px;
    z-index: 3;
    display: flex;
    flex-direction: column;
    min-width: 250px;
    padding: 5px;
    border-radius: 10px;
    background: var(--popover);
    border: 1px solid var(--popover-border);
    box-shadow:
      0 8px 28px rgb(0 0 0 / 14%),
      0 1px 3px rgb(0 0 0 / 10%);
    font: 500 13px var(--font-ui);
  }
  .menu button {
    min-height: 30px;
    padding: 0 10px 0 6px;
    display: flex;
    align-items: center;
    gap: 6px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--ink);
    font: inherit;
    text-align: left;
  }
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--hover-wash);
    outline: none;
  }
  .sep {
    height: 1px;
    margin: 5px 6px;
    background: var(--hairline);
  }
  .body p {
    margin: 0;
  }
  .quote {
    margin-top: 6px !important;
    font: 400 16px/1.55 var(--font-reading, Literata, Georgia, serif);
    color: var(--ink-secondary);
  }
  .selected {
    color: var(--ink);
    text-decoration: underline;
    text-decoration-color: var(--accent);
    text-decoration-thickness: 2px;
    text-underline-offset: 3px;
  }
  .asking {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 12px !important;
    color: var(--ink-secondary);
  }
  .progress {
    position: relative;
    overflow: hidden;
    width: 64px;
    height: 2px;
    border-radius: 1px;
    background: var(--control-track);
  }
  .progress::after {
    content: '';
    position: absolute;
    inset: 0 60% 0 0;
    background: var(--accent);
    animation: travel 1.2s ease-in-out infinite alternate;
  }
  @keyframes travel {
    to {
      transform: translateX(150%);
    }
  }
  .term {
    margin-top: 4px !important;
    font: 600 19px/1.35 var(--font-reading, Literata, Georgia, serif);
  }
  .meaning,
  .definition {
    margin-top: 4px !important;
    font: 400 16px/1.6 var(--font-reading, Literata, Georgia, serif);
  }
  .definition p {
    margin: 0 0 0.4em;
  }
  .definition p:first-child {
    font-weight: 600;
  }
  .qualifier {
    margin-top: 8px !important;
    padding-left: 10px;
    border-left: 2px solid var(--accent);
    font: 400 15px/1.55 var(--font-reading, Literata, Georgia, serif);
  }
  .details {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 12px 0 0;
    padding-top: 11px;
    border-top: 1px solid var(--hairline);
  }
  .details dt {
    margin-top: 7px;
    font: 600 11px var(--font-ui);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-secondary);
  }
  .details dt:first-child {
    margin-top: 0;
  }
  .details dd {
    margin: 0;
    font: 400 15px/1.55 var(--font-reading, Literata, Georgia, serif);
  }
  .entry {
    display: block;
    width: 100%;
    height: min(240px, 30vh);
    margin-top: 8px;
    border: 1px solid var(--hairline);
    border-radius: 8px;
    background: #fffdf9;
  }
  .missing,
  .error {
    margin-top: 6px !important;
    font-size: 14px;
  }
  .missing {
    color: var(--ink-secondary);
  }
  .error strong {
    font-weight: 600;
  }
  .sent {
    margin: 8px 0 0;
    padding: 10px 12px;
    border-radius: 6px;
    background: var(--control-track);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font:
      400 12.5px/1.55 ui-monospace,
      'SF Mono',
      Menlo,
      monospace;
  }
  .footer {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-top: 1px solid var(--hairline);
  }
  .footer button {
    height: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: 6px;
    background: var(--control-track);
    color: var(--ink);
    font: 500 12px var(--font-ui);
  }
  .footer button:hover {
    background: var(--hover-wash);
  }
  .footer .primary {
    background: var(--accent);
    color: var(--popover);
  }
  .notice-title {
    margin-top: 6px !important;
    font-weight: 600;
    font-size: 14px;
  }
  .notice-text {
    margin-top: 4px !important;
    font-size: 13px;
    line-height: 1.5;
  }
  .footer .link {
    background: none;
    color: var(--accent);
    font-weight: 600;
  }
  .footer .sent-link {
    margin-left: auto;
    color: var(--ink-secondary);
    font-weight: 500;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  /* EA3: under reduced motion the pending line stands still and nothing fades in. */
  @media (prefers-reduced-motion: reduce) {
    .peek {
      animation: none;
    }
    .progress::after {
      animation: none;
    }
  }
  /* X7: Increase Contrast and forced colours keep the border, pointer and rings. */
  @media (prefers-contrast: more) {
    .peek,
    .pointer {
      border-color: var(--ink);
    }
  }
  @media (forced-colors: active) {
    .peek {
      border: 1px solid CanvasText;
    }
    .pointer {
      border-color: CanvasText;
    }
    .selected {
      text-decoration-color: Highlight;
    }
  }
</style>
