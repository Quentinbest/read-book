<script lang="ts">
  import { listen } from '@tauri-apps/api/event'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { open as openDialog } from '@tauri-apps/plugin-dialog'
  import { onMount } from 'svelte'
  import LiveRegion from './components/LiveRegion.svelte'
  import MessageBar from './components/MessageBar.svelte'
  import Library from './app/Library.svelte'
  import { importMessages } from './app/importFeedback'
  import { ipc, type Book, type ImportResult } from './app/ipc'
  import { installMenuBar } from './app/menubar'
  import { applyTheme, type ThemeChoice } from './app/theme'
  import { WriteQueue } from './app/writes'
  import { isTextField, type KeyContext } from './lib/commands/keys'
  import { CommandRegistry } from './lib/commands/registry'
  import { MessageQueue } from './lib/reader/messages'

  let books: Book[] = $state([])
  let dropActive = $state(false)
  let liveRegion: LiveRegion | undefined = $state()
  let singleKeysEnabled = true

  const messages = new MessageQueue({
    announcer: { announce: (text, politeness) => liveRegion?.announce(text, politeness) },
  })
  const writes = new WriteQueue(messages)
  const registry = new CommandRegistry()

  async function refresh() {
    books = await ipc.libraryList()
  }

  async function importPaths(paths: string[]) {
    const epubs = paths.filter((p) => p.toLowerCase().endsWith('.epub'))
    if (!epubs.length) return
    showResults(await ipc.libraryImport(epubs))
  }

  function showResults(results: ImportResult[]) {
    for (const m of importMessages(results)) messages.push(m)
    void refresh()
  }

  async function openBookDialog() {
    const picked = await openDialog({
      multiple: true,
      directory: false,
      filters: [{ name: 'EPUB', extensions: ['epub'] }],
    })
    if (picked) await importPaths(Array.isArray(picked) ? picked : [picked])
  }

  function keyContext(): KeyContext {
    return {
      // The reader page arrives in Phase 2; in the library no page has focus.
      pageFocused: false,
      textFieldActive: isTextField(document.activeElement),
      singleKeysEnabled,
      screenReaderRunning: false, // T6: native detection is wired in keys.ts once available
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
      cleanups.push(registry.handle('book.open', { run: () => void openBookDialog() }))
      cleanups.push(registry.handle('library.show', { run: () => void refresh() }))
      await installMenuBar(registry)
      cleanups.push(
        await listen<ImportResult[]>('os-opened', () => {
          void ipc.openedTake().then(showResults)
        }),
      )
      showResults(await ipc.openedTake())
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

  // Exposed for later phases (progress persistence, notes).
  export { writes }
</script>

<svelte:window {onkeydown} />

<Library {books} {dropActive} onopen={() => void openBookDialog()} />
<MessageBar queue={messages} />
<LiveRegion bind:this={liveRegion} />
