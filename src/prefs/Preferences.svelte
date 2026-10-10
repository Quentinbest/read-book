<script lang="ts">
  // Settings (G2, provisional; Screen 11 draws the Extensions pane): a sidebar of
  // sections and one pane. Changes save at once and reach the reader (settingsSync).
  import { onMount } from 'svelte'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { getVersion } from '@tauri-apps/api/app'
  import { emit } from '@tauri-apps/api/event'
  import { open } from '@tauri-apps/plugin-dialog'
  import Switch from '../components/Switch.svelte'
  import { AVAILABLE, LANGUAGE_NAMES, locale, t, type Locale } from '../lib/strings'
  import { LANGUAGE_SETTING, SYSTEM, negotiate } from '../lib/strings/negotiate'
  import { ipc } from '../app/ipc'
  import { applyTheme, type ThemeChoice } from '../app/theme'
  import { changeSetting as change, onSettingChanged } from '../app/settingsSync'
  import { TEXT_SIZES, DEFAULT_TEXT_SIZE } from '../reader/textSizes'
  import {
    SETTING as TYPE_SETTING,
    parseFont,
    parsePageWidth,
    parsePublisherStyles,
    type FontChoice,
    type PageWidth,
    type PublisherStyles,
  } from '../reader/typography'
  import { exportAllAnnotations } from './exportAll'
  import ExtensionsPane from './ExtensionsPane.svelte'
  import DictionariesPane from './DictionariesPane.svelte'
  import ShortcutsPane from './ShortcutsPane.svelte'

  /** This page's own changes are not applied back to it. */
  const source = crypto.randomUUID()
  const changeSetting = (key: string, value: string) => change(key, value, source)

  type Section = keyof typeof t.prefs.sections
  const SECTIONS = Object.keys(t.prefs.sections) as Section[]

  let section = $state<Section>('general')
  let theme = $state<ThemeChoice>('auto')
  // Reading Lens LK8: the peek's “Open options” opens Settings on an extension's options.
  let extensionsPane: ReturnType<typeof ExtensionsPane> | undefined = $state()
  async function showOptions(id: string) {
    section = 'extensions'
    for (let i = 0; i < 40 && !extensionsPane; i++) await new Promise((r) => setTimeout(r, 50))
    await new Promise((r) => setTimeout(r, 200))
    extensionsPane?.openOptions(id)
  }
  let atLaunch = $state<'library' | 'book'>('library')
  /** L-4: the `language` setting (the page's own language until it loads), and the
   * language “System” stands for. */
  let language = $state<string>(locale)
  let systemLocale = $state<Locale>('en')
  /** L-5: the choice applies at restart; until then this page keeps its language. */
  const languagePending = $derived((language === SYSTEM ? systemLocale : language) !== locale)
  let fontPx = $state(DEFAULT_TEXT_SIZE)
  let spacing = $state('default')
  let font = $state<FontChoice>('book')
  let width = $state<PageWidth>('normal')
  let publisher = $state<PublisherStyles>('balanced')
  let crossfade = $state(false)
  let announcements = $state(true)
  let singleKeys = $state(true)
  let folder = $state('')
  let version = $state('')
  /** D1: a crash log on this Mac, if anything has been written to it. */
  let crashLog = $state(false)
  let status = $state('')
  let loaded = false

  const choice = <T extends string>(label: string, options: [T, string][]) => ({ label, options })
  const themes = choice<ThemeChoice>(t.prefs.theme, [
    ['paper', t.aa.paper],
    ['sepia', t.aa.sepia],
    ['night', t.aa.night],
    ['auto', t.aa.auto],
  ])
  const spacings = choice(t.prefs.lineSpacing, [
    ['compact', t.aa.compact],
    ['default', t.aa.normal],
    ['loose', t.aa.loose],
  ])
  // 1.1 (approved 2026-10-01)
  const fonts = choice<FontChoice>(t.prefs.font, [
    ['book', t.aa.fontBook],
    ['literata', t.aa.fontLiterata],
    ['sans', t.aa.fontSans],
    ['dyslexic', t.aa.fontDyslexicLong],
  ])
  const widths = choice<PageWidth>(t.prefs.pageWidth, [
    ['narrow', t.aa.narrow],
    ['normal', t.aa.normal],
    ['wide', t.aa.wide],
  ])
  const publishers = choice<PublisherStyles>(t.prefs.publisherStyles, [
    ['full', t.prefs.publisherFull],
    ['balanced', t.prefs.publisherBalanced],
    ['off', t.prefs.publisherOff],
  ])
  const launches = choice<'library' | 'book'>(t.prefs.atLaunch, [
    ['library', t.prefs.atLaunchLibrary],
    ['book', t.prefs.atLaunchBook],
  ])

  // Switches save when they change (after the first load).
  $effect(() => {
    const v = crossfade
    if (loaded) void changeSetting('pageTurnCrossfade', v ? 'on' : 'off')
  })
  $effect(() => {
    const v = announcements
    if (loaded) void changeSetting('pageTurnAnnouncements', v ? 'on' : 'off')
  })
  $effect(() => {
    const v = singleKeys
    if (loaded) void changeSetting('singleKeyShortcuts', v ? 'on' : 'off')
  })

  async function setTheme(v: ThemeChoice) {
    theme = v
    applyTheme(v)
    await changeSetting('theme', v)
  }

  async function exportAll() {
    const dir = await open({ directory: true, title: t.prefs.exportAll })
    if (typeof dir !== 'string') return
    const n = await exportAllAnnotations(dir)
    status = n ? t.prefs.exported(n) : t.prefs.nothingToExport
  }

  function onkeydown(e: KeyboardEvent, i: number) {
    const d = e.key === 'ArrowDown' ? 1 : e.key === 'ArrowUp' ? -1 : 0
    if (!d) return
    e.preventDefault()
    const next = SECTIONS[(i + d + SECTIONS.length) % SECTIONS.length]
    section = next
    requestAnimationFrame(() => document.getElementById(`prefs-${next}`)?.focus())
  }

  onMount(() => {
    void (async () => {
      const get = (k: string) => ipc.settingGet(k)
      theme = ((await get('theme')) as ThemeChoice | null) ?? 'auto'
      applyTheme(theme)
      atLaunch = (await get('openAtLaunch')) === 'book' ? 'book' : 'library'
      if (AVAILABLE.length > 1) {
        const stored = await get(LANGUAGE_SETTING)
        language = AVAILABLE.find((l) => l === stored) ?? SYSTEM
        systemLocale = negotiate(await ipc.preferredLanguages().catch(() => []), AVAILABLE)
      }
      fontPx = Number((await get('fontPx')) ?? DEFAULT_TEXT_SIZE) || DEFAULT_TEXT_SIZE
      spacing = (await get('lineSpacing')) ?? 'default'
      font = parseFont(await get(TYPE_SETTING.font))
      width = parsePageWidth(await get(TYPE_SETTING.width))
      publisher = parsePublisherStyles(await get(TYPE_SETTING.publisher))
      crossfade = (await get('pageTurnCrossfade')) === 'on'
      announcements = (await get('pageTurnAnnouncements')) !== 'off'
      singleKeys = (await get('singleKeyShortcuts')) !== 'off'
      folder = await ipc.libraryFolder().catch(() => '')
      version = await getVersion().catch(() => '')
      crashLog = await ipc.crashLogExists().catch(() => false)
      loaded = true
      const options = /^#options=(.+)$/.exec(location.hash)?.[1]
      if (options) void showOptions(decodeURIComponent(options))
    })()
    const offOptions = getCurrentWebviewWindow().listen<string>(
      'open-extension-options',
      (e) => void showOptions(e.payload),
    )
    // The reader's Aa popover may change the same settings.
    const off = onSettingChanged(({ key, value }) => {
      if (key === 'theme') theme = value as ThemeChoice
      if (key === 'fontPx') fontPx = Number(value)
      if (key === 'lineSpacing') spacing = value
      if (key === TYPE_SETTING.font) font = parseFont(value)
      if (key === TYPE_SETTING.width) width = parsePageWidth(value)
    }, source)
    return () => {
      void off.then((f) => f())
      void offOptions.then((f) => f())
    }
  })
</script>

<div class="titlebar" data-tauri-drag-region></div>
<div class="prefs">
  <nav aria-label={t.prefs.title} data-tauri-drag-region>
    <ul role="tablist" aria-orientation="vertical">
      {#each SECTIONS as s, i (s)}
        <li role="presentation">
          <button
            type="button"
            role="tab"
            id="prefs-{s}"
            aria-selected={section === s}
            aria-controls="prefs-pane"
            tabindex={section === s ? 0 : -1}
            onclick={() => (section = s)}
            onkeydown={(e) => onkeydown(e, i)}>{t.prefs.sections[s]}</button
          >
        </li>
      {/each}
    </ul>
  </nav>
  <div class="pane" id="prefs-pane" role="tabpanel" aria-labelledby="prefs-{section}">
    <h1>{t.prefs.sections[section]}</h1>
    {#if section === 'general'}
      {@render radios(themes, theme, (v) => void setTheme(v))}
      {@render radios(launches, atLaunch, (v) => {
        atLaunch = v
        void changeSetting('openAtLaunch', v)
      })}
      {#if AVAILABLE.length > 1}
        <div class="field">
          <label for="prefs-language">{t.prefs.language}</label>
          <select
            id="prefs-language"
            value={language}
            onchange={(e) => {
              language = e.currentTarget.value
              void changeSetting(LANGUAGE_SETTING, language)
            }}
          >
            <option value={SYSTEM}>{t.prefs.languageSystem(LANGUAGE_NAMES[systemLocale])}</option>
            {#each AVAILABLE as l (l)}<option value={l} lang={l}>{LANGUAGE_NAMES[l]}</option>{/each}
          </select>
          {#if languagePending}
            <p class="help" role="status">{t.prefs.languageRestart}</p>
            <div>
              <button type="button" class="btn" onclick={() => void ipc.appRestart()}
                >{t.prefs.restartNow}</button
              >
            </div>
          {/if}
        </div>
      {/if}
    {:else if section === 'reading'}
      <div class="field">
        <label for="prefs-size">{t.prefs.textSize}</label>
        <select
          id="prefs-size"
          value={String(fontPx)}
          onchange={(e) => {
            fontPx = Number(e.currentTarget.value)
            void changeSetting('fontPx', e.currentTarget.value)
          }}
        >
          {#each TEXT_SIZES as px (px)}<option value={String(px)}>{t.aa.px(px)}</option>{/each}
          {#if !TEXT_SIZES.includes(fontPx as never)}<option value={String(fontPx)}
              >{t.aa.px(fontPx)}</option
            >{/if}
        </select>
      </div>
      {@render radios(spacings, spacing, (v) => {
        spacing = v
        void changeSetting('lineSpacing', v)
      })}
      {@render radios(fonts, font, (v) => {
        font = v
        void changeSetting(TYPE_SETTING.font, v)
      })}
      {@render radios(widths, width, (v) => {
        width = v
        void changeSetting(TYPE_SETTING.width, v)
      })}
      {@render radios(publishers, publisher, (v) => {
        publisher = v
        void changeSetting(TYPE_SETTING.publisher, v)
      })}
      <p class="help">{t.prefs.publisherHelp}</p>
      <Switch
        label={t.prefs.crossfade}
        description={t.prefs.crossfadeHelp}
        bind:checked={crossfade}
      />
      <Switch
        label={t.prefs.announcements}
        description={t.prefs.announcementsHelp}
        bind:checked={announcements}
      />
    {:else if section === 'library'}
      <div class="field">
        <span class="label">{t.prefs.folder}</span>
        <code class="path">{folder}</code>
        <p class="help">{t.prefs.folderHelp}</p>
        <div>
          <button type="button" class="btn" onclick={() => void ipc.libraryFolderShow()}
            >{t.prefs.showFolder}</button
          >
        </div>
      </div>
      <div class="field">
        <div>
          <button type="button" class="btn" onclick={() => void exportAll()}
            >{t.prefs.exportAll}</button
          >
        </div>
        <p class="help">{t.prefs.exportHelp}</p>
        {#if status}<p class="status" role="status">{status}</p>{/if}
      </div>
      <p class="help">{t.prefs.uninstall}</p>
    {:else if section === 'dictionaries'}
      <DictionariesPane />
    {:else if section === 'extensions'}
      <ExtensionsPane bind:this={extensionsPane} />
    {:else if section === 'shortcuts'}
      <Switch
        label={t.prefs.singleKeys}
        description={t.prefs.singleKeysHelp}
        bind:checked={singleKeys}
      />
      <div>
        <button type="button" class="btn" onclick={() => void emit('show-shortcuts')}
          >{t.prefs.showShortcuts}</button
        >
      </div>
      <ShortcutsPane {source} />
    {:else}
      <picture class="about-logo">
        {#if theme === 'auto' || theme.startsWith('ext:')}
          <source
            media="(prefers-color-scheme: dark)"
            srcset="/brand/linen-a-v002/horizontal-white.svg"
          />
        {/if}
        <img
          src={theme === 'night'
            ? '/brand/linen-a-v002/horizontal-white.svg'
            : '/brand/linen-a-v002/horizontal-primary.svg'}
          width="204"
          height="80"
          alt="Linen"
        />
      </picture>
      {#if version}<p class="help">{t.prefs.version(version)}</p>{/if}
      <p>{t.prefs.aboutLine}</p>
      <p class="help">{t.prefs.fonts}</p>
      <p class="help">{t.prefs.privacy}</p>
      {#if crashLog}
        <div>
          <button type="button" class="btn" onclick={() => void ipc.crashLogShow().catch(() => {})}
            >{t.prefs.showCrashLog}</button
          >
        </div>
      {/if}
    {/if}
  </div>
</div>

{#snippet radios<T extends string>(
  group: { label: string; options: [T, string][] },
  value: T | string,
  set: (v: T) => void,
)}
  <div class="field" role="radiogroup" aria-label={group.label}>
    <span class="label">{group.label}</span>
    <div class="seg">
      {#each group.options as [v, label] (v)}
        <button type="button" role="radio" aria-checked={value === v} onclick={() => set(v)}
          >{label}</button
        >
      {/each}
    </div>
  </div>
{/snippet}

<style>
  :global(body) {
    margin: 0;
    background: var(--ground);
  }
  .titlebar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 28px;
  }
  /* Screen 11: a 220 px sidebar on the panel ground, the pane on the page. */
  .prefs {
    display: grid;
    grid-template-columns: 220px 1fr;
    min-height: 100vh;
    font-family: var(--font-ui);
    color: var(--ink);
  }
  nav {
    background: var(--panel);
    border-right: 1px solid var(--hairline);
    padding: 64px 12px 16px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  [role='tab'] {
    width: 100%;
    height: 32px;
    padding: 0 12px;
    border: 0;
    border-radius: 8px;
    background: none;
    text-align: left;
    font: 500 14px var(--font-ui);
    color: var(--ink);
  }
  [role='tab'][aria-selected='true'] {
    background: var(--control-track);
    color: var(--accent);
    font-weight: 600;
  }
  .pane {
    padding: 40px 64px 48px;
    display: flex;
    flex-direction: column;
    gap: 20px;
    max-width: 720px;
  }
  h1 {
    margin: 0;
    font-size: 24px;
    font-weight: 600;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .label,
  label {
    font-size: 13px;
    font-weight: 600;
  }
  .seg {
    display: inline-flex;
    align-self: flex-start;
    gap: 2px;
    padding: 3px;
    background: var(--control-track);
    border-radius: 9px;
  }
  .seg button {
    height: 30px;
    padding: 0 14px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    font: 500 13px var(--font-ui);
    color: var(--track-ink);
  }
  .seg button[aria-checked='true'] {
    background: var(--raised);
    color: var(--ink);
    font-weight: 600;
    box-shadow: 0 0 0 1px var(--segment-ring);
  }
  select {
    align-self: flex-start;
    height: 32px;
    padding: 0 8px;
    border: 1px solid var(--popover-border);
    border-radius: 8px;
    background: var(--raised);
    color: var(--ink);
    font: 13px var(--font-ui);
  }
  .btn {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--popover-border);
    border-radius: 8px;
    background: var(--raised);
    color: var(--ink);
    font: 500 13px var(--font-ui);
  }
  .path {
    font:
      12px ui-monospace,
      monospace;
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  .help,
  p {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
  }
  .help {
    color: var(--ink-secondary);
  }
  .status {
    color: var(--ink);
  }
  .about-logo {
    width: 204px;
    max-width: 100%;
  }
  .about-logo img {
    display: block;
    width: 100%;
    height: auto;
  }
  /* X6: narrow (zoomed) Settings: the sections run across the top. */
  @media (max-width: 560px) {
    .prefs {
      grid-template-columns: 1fr;
    }
    nav {
      padding: 48px 12px 8px;
      border-right: 0;
      border-bottom: 1px solid var(--hairline);
    }
    ul {
      flex-direction: row;
      flex-wrap: wrap;
    }
    [role='tab'] {
      width: auto;
    }
    .pane {
      padding: 24px 16px 32px;
    }
  }
  [role='tab']:focus-visible,
  .seg button:focus-visible,
  .btn:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
