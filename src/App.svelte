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
  import { applyTheme, type ThemeChoice } from './app/theme'
  import { WriteQueue } from './app/writes'
  import { testHooks } from './app/testHooks'
  import { isTextField, type KeyContext } from './lib/commands/keys'
  import { CommandRegistry } from './lib/commands/registry'
  import { MessageQueue } from './lib/reader/messages'
  import { t } from './lib/strings/en'

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
      applyTheme(((await ipc.settingGet('theme')) as ThemeChoice | null) ?? 'auto')
      singleKeysEnabled = (await ipc.settingGet('singleKeyShortcuts')) !== 'off'
      await refresh()
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
      onexit={closeReader}
    />
  {/key}
{:else}
  <Library {books} {dropActive} onopen={() => void openBookDialog()} onopenbook={openBook} />
{/if}
<MessageBar queue={messages} />
<LiveRegion bind:this={liveRegion} />
