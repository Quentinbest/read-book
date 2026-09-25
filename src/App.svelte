<script lang="ts">
  import { listen } from '@tauri-apps/api/event'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { open as openDialog } from '@tauri-apps/plugin-dialog'
  import { onMount } from 'svelte'
  import LiveRegion from './components/LiveRegion.svelte'
  import MessageBar from './components/MessageBar.svelte'
  import Library from './app/Library.svelte'
  import Reader from './reader/Reader.svelte'
  import { importMessages } from './app/importFeedback'
  import { ipc, type Book, type ImportResult } from './app/ipc'
  import { installMenuBar } from './app/menubar'
  import { refreshLayoutLabels } from './app/keyLabels'
  import CommandPalette from './app/CommandPalette.svelte'
  import CheatSheet from './app/CheatSheet.svelte'
  import { applyTheme, type ThemeChoice } from './app/theme'
  import { onSettingChanged } from './app/settingsSync'
  import { openSettingsWindow } from './app/settingsWindow'
  import { WriteQueue } from './app/writes'
  import { testHooks } from './app/testHooks'
  import { isTextField, type KeyContext } from './lib/commands/keys'
  import { CommandRegistry } from './lib/commands/registry'
  import { MessageQueue } from './lib/reader/messages'
  import { t } from './lib/strings/en'
  import { ExtensionHost } from './extensions/host.svelte'

  let books: Book[] = $state([])
  let reading: Book | null = $state(null)
  let dropActive = $state(false)
  let liveRegion: LiveRegion | undefined = $state()
  let singleKeysEnabled = true
  let screenReaderRunning = false

  const messages = new MessageQueue({
    announcer: { announce: (text, politeness) => liveRegion?.announce(text, politeness) },
  })
  const writes = new WriteQueue(messages)
  if (testHooks) testHooks.messages = messages
  const registry = new CommandRegistry()
  // Phase 7: extensions, run by the host in this window (P2, P4).
  const extensions = new ExtensionHost()
  if (testHooks) {
    testHooks.run = (id) => registry.run(id)
    testHooks.registry = registry
    testHooks.writes = writes
    testHooks.extensions = extensions
  }

  /** P9: the theme pack a choice names, when it is installed, on and passes its checks. */
  function packTheme(choice: string) {
    return extensions
      .themes()
      .find((x) => `ext:${x.extId}/${x.id}` === choice && !x.problems.length)?.theme
  }
  let themeChoice: ThemeChoice = 'auto'
  function applyChoice(choice: ThemeChoice) {
    themeChoice = choice
    applyTheme(choice, packTheme(choice))
  }

  // P1: extension commands in ⌘K, while their extension is on (never with a shortcut, C4).
  let extensionCommands: (() => void)[] = []
  function syncExtensionCommands() {
    for (const off of extensionCommands) off()
    extensionCommands = extensions.commands().map((c) => {
      const id = registry.defineExtension(c.extId, {
        id: c.id,
        title: c.title,
        extensionName: c.name,
      })
      registry.handle(id, { run: () => void runExtensionCommand(c.extId, c.id, c.name) })
      return () => registry.undefine(id)
    })
  }
  /** P6: a failure shows one quiet line with Restart; reading carries on. */
  async function runExtensionCommand(extId: string, command: string, name: string) {
    try {
      await extensions.invoke(extId, command)
    } catch {
      messages.push({
        text: t.extensions.stopped(name),
        action: { label: t.extensions.restart, run: () => void extensions.restart(extId) },
      })
    }
  }
  async function reloadExtensions() {
    await extensions.load()
    syncExtensionCommands()
    applyChoice(themeChoice)
  }
  // N5: work to finish before the app quits (the reader saves its position).
  // Not reactive state: nothing renders from it.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const beforeQuit = new Set<() => void>()
  const onBeforeQuit = (fn: () => void) => {
    beforeQuit.add(fn)
    return () => beforeQuit.delete(fn)
  }

  async function refresh() {
    books = await ipc.libraryList()
  }

  async function importPaths(paths: string[]) {
    const epubs = paths.filter((p) => p.toLowerCase().endsWith('.epub'))
    if (!epubs.length) return
    showResults(await ipc.libraryImport(epubs))
  }

  function showResults(results: ImportResult[], openSingle = false) {
    // Opening a book that is already in the library needs no message: it just opens (E8).
    const shown =
      openSingle && results.length === 1
        ? results.filter((r) => r.outcome.kind !== 'alreadyInLibrary')
        : results
    for (const m of importMessages(shown)) messages.push(m)
    void refresh().then(() => {
      // E8: a single book opened from the OS goes straight to the reader.
      if (!openSingle || results.length !== 1) return
      const o = results[0].outcome
      const id = o.kind === 'rejected' ? null : o.book_id
      const book = books.find((b) => b.id === id)
      if (book) openBook(book)
    })
  }

  function openBook(book: Book) {
    reading = book
  }

  function closeReader() {
    reading = null
    void refresh()
  }

  async function openBookDialog() {
    const picked = await openDialog({
      multiple: true,
      directory: false,
      filters: [{ name: t.import.fileFilter, extensions: ['epub'] }],
    })
    if (picked) await importPaths(Array.isArray(picked) ? picked : [picked])
  }

  // K9, S8: the command palette, a modal over the reader or the library.
  let paletteOpen = $state(false)
  let cheatSheetOpen = $state(false)

  function keyContext(): KeyContext {
    return {
      // In the library no page has focus; the reader routes its own page keys.
      pageFocused: false,
      textFieldActive: isTextField(document.activeElement),
      singleKeysEnabled,
      screenReaderRunning,
      modalOpen: document.querySelector('dialog[open]') !== null,
    }
  }

  function onkeydown(e: KeyboardEvent) {
    const command = registry.commandForKey(e, keyContext())
    if (command && registry.run(command.id)) e.preventDefault()
  }

  onMount(() => {
    const cleanups: (() => void)[] = []
    void (async () => {
      // §6.4 cold start: the first round trips go together.
      const [themeSetting, singleKeys, atLaunch] = await Promise.all([
        ipc.settingGet('theme'),
        ipc.settingGet('singleKeyShortcuts'),
        ipc.settingGet('openAtLaunch'),
        refresh(),
      ])
      await reloadExtensions()
      applyChoice((themeSetting as ThemeChoice | null) ?? 'auto')
      singleKeysEnabled = singleKeys !== 'off'
      // G2 (provisional): “When Linen opens: Reopen the last book”.
      if (atLaunch === 'book' && !reading) {
        const last = books
          .filter((b) => b.opened_at !== null && !b.finished_at)
          .sort((a, b) => (b.opened_at ?? 0) - (a.opened_at ?? 0))[0]
        if (last) reading = last
      }
      // §6.4: the library's first paint (the reader marks its first page itself).
      if (!reading)
        requestAnimationFrame(() =>
          requestAnimationFrame(() => void ipc.startupMark('library').catch(() => {})),
        )
      // T6: VoiceOver has no web-visible signal; ask the core, and keep asking.
      const pollScreenReader = async () => {
        screenReaderRunning = await ipc.screenReaderRunning()
      }
      await pollScreenReader()
      await refreshLayoutLabels()
      const onFocus = () => void refreshLayoutLabels()
      window.addEventListener('focus', onFocus)
      cleanups.push(() => window.removeEventListener('focus', onFocus))
      const srTimer = setInterval(() => void pollScreenReader(), 2000)
      cleanups.push(() => clearInterval(srTimer))
      cleanups.push(registry.handle('book.open', { run: () => void openBookDialog() }))
      cleanups.push(registry.handle('palette.open', { run: () => (paletteOpen = !paletteOpen) }))
      cleanups.push(registry.handle('shortcuts.show', { run: () => (cheatSheetOpen = true) }))
      // G2: Settings… (⌘,) opens the Settings window; its changes reach this window.
      cleanups.push(
        registry.handle('app.settings', {
          run: () => {
            if (testHooks) testHooks.settingsOpened = (testHooks.settingsOpened ?? 0) + 1
            else void openSettingsWindow()
          },
        }),
      )
      cleanups.push(
        await onSettingChanged(({ key, value }) => {
          if (key === 'theme') applyChoice(value as ThemeChoice)
          if (key === 'singleKeyShortcuts') singleKeysEnabled = value !== 'off'
        }),
      )
      cleanups.push(await listen('show-shortcuts', () => (cheatSheetOpen = true)))
      // Settings installed, turned on or off, or removed an extension.
      cleanups.push(await listen('extensions-changed', () => void reloadExtensions()))
      cleanups.push(
        registry.handle('library.show', {
          run: () => (reading ? closeReader() : void refresh()),
        }),
      )
      await installMenuBar(registry)
      // Later commands (the reader's) join the menu bar as they are registered.
      let menuTimer = 0
      cleanups.push(
        registry.subscribe(() => {
          clearTimeout(menuTimer)
          menuTimer = window.setTimeout(() => void installMenuBar(registry), 50)
        }),
      )
      cleanups.push(
        await listen<ImportResult[]>('os-opened', () => {
          void ipc.openedTake().then((r) => showResults(r, true))
        }),
      )
      showResults(await ipc.openedTake(), true)
      cleanups.push(
        await listen('app-quitting', async () => {
          beforeQuit.forEach((fn) => fn())
          await writes.idle()
          if (testHooks) testHooks.quitRequested = true
          else await ipc.quitReady()
        }),
      )
      cleanups.push(
        await getCurrentWebview().onDragDropEvent((e) => {
          if (e.payload.type === 'over' || e.payload.type === 'enter') dropActive = true
          else if (e.payload.type === 'leave') dropActive = false
          else if (e.payload.type === 'drop') {
            dropActive = false
            void importPaths(e.payload.paths)
          }
        }),
      )
    })()
    return () => cleanups.forEach((c) => c())
  })
</script>

<svelte:window {onkeydown} />

{#if reading}
  {#key reading.id}
    <Reader
      book={reading}
      {messages}
      {writes}
      {registry}
      screenReader={() => screenReaderRunning}
      {keyContext}
      {onBeforeQuit}
      announce={(text) => liveRegion?.announce(text, 'polite')}
      onexit={closeReader}
      {extensions}
    />
  {/key}
{:else}
  <Library
    {books}
    {dropActive}
    {messages}
    {registry}
    onopen={() => void openBookDialog()}
    onopenbook={openBook}
    onchanged={refresh}
  />
{/if}
<CommandPalette open={paletteOpen} {registry} {messages} onclose={() => (paletteOpen = false)} />
<CheatSheet open={cheatSheetOpen} {registry} onclose={() => (cheatSheetOpen = false)} />
<MessageBar queue={messages} />
<LiveRegion bind:this={liveRegion} />
