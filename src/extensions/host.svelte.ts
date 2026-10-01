// The extension host (P2, P4, P6; §7.2). Each extension runs in its own Worker,
// made by a hidden host frame on the extension's own origin (Spike G). Every call
// the Worker makes arrives here as a structured message and is checked against
// the permissions the reader granted, before anything is done for it.
//
// Extensions start lazily (on their activation events) and are unloaded after a
// minute unused. The watchdog gives UI contributions 2 s and work 10 s; a Worker
// that stops answering its heartbeat is stopped; each failure is reported to the
// core, which suspends an extension after three in ten minutes. The reader never
// waits for an extension beyond those limits.

import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { ipc } from '../app/ipc'
import { testHooks } from '../app/testHooks'
import { contributionLabels } from '../lib/extensions/labels'
import { packProblems, themeFromPack } from '../lib/extensions/themes'
import type { InstalledExtension } from '../lib/extensions/types'
import { when, type WhenContext } from '../lib/extensions/when'
import type { Theme } from '../lib/theme/tokens'
import { sessionForExtensions } from '../reader/sessions'

export const UI_TIMEOUT_MS = 2000
export const WORK_TIMEOUT_MS = 10_000
export const IDLE_UNLOAD_MS = 60_000
const HEARTBEAT_MS = 5000
const HEARTBEAT_GRACE_MS = 2000
const START_TIMEOUT_MS = 5000

export type ExtensionStatus = 'idle' | 'starting' | 'running' | 'not-responding' | 'suspended'

/** What the open book gives extensions, each part behind its permission. */
export interface ReaderBridge {
  metadata(): {
    title: string
    authors: string[]
    language: string | null
    identifier: string | null
  }
  chapters(): Promise<{ index: number; label: string }[]>
  text(chapter: number): Promise<string>
  /** W3C Web Annotations (A9) of this book, each with `linen:chapter`, as a collection. */
  annotations(): { label: string; total: number; first: { items: unknown[] } }
}

interface Running {
  /** The package it runs: an update (a new version) must not keep the old code running. */
  version: string
  frame: HTMLIFrameElement
  started: Promise<void>
  calls: Map<number, { resolve(v: unknown): void; reject(e: Error): void; timer: number }>
  lastUsed: number
  idle: number
  heartbeat: number
  awaitingPong: number | null
  ready(): void
  fail(e: Error): void
}

export class ExtensionError extends Error {}

export class ExtensionHost {
  /** Installed extensions as the core lists them. */
  extensions = $state.raw<InstalledExtension[]>([])
  /** Per extension: idle, starting, running, not responding (a call timed out), suspended. */
  status = $state<Record<string, ExtensionStatus>>({})
  /** P7: this launch runs without extensions. */
  safeMode = $state(false)
  bridge: ReaderBridge | null = null
  /** Extensions whose Navigator tab is showing: kept loaded while it shows. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping, nothing renders from it
  visibleTabs = new Set<string>()

  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping, nothing renders from it
  #running = new Map<string, Running>()
  #nextCall = 1
  /** book.selection is readable only while the reader is using the extension (P3). */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping, nothing renders from it
  #selection = new Map<string, { text: string; cfi: string }>()
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping, nothing renders from it
  #subscribed = new Set<string>()
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping, nothing renders from it
  #frames = new Map<Window, string>()

  constructor() {
    addEventListener('message', (e) => this.#onMessage(e))
  }

  /** Extensions that may run now. */
  get active(): InstalledExtension[] {
    if (this.safeMode) return []
    return this.extensions.filter((x) => x.enabled && !x.incompatible && !x.suspended)
  }

  async load() {
    this.safeMode = await invoke<boolean>('app_safe_mode').catch(() => false)
    this.extensions = await invoke<InstalledExtension[]>('extensions_list').catch(() => [])
    for (const x of this.extensions)
      this.status[x.manifest.id] = x.suspended
        ? 'suspended'
        : (this.status[x.manifest.id] ?? 'idle')
    // Anything no longer active stops, and anything updated stops to run its new
    // package. A tab showing starts again at once (its contributions are the new ones).
    for (const [id, run] of [...this.#running]) {
      const x = this.active.find((e) => e.manifest.id === id)
      if (x && x.manifest.version === run.version) continue
      this.unload(id)
      if (x && this.visibleTabs.has(id)) void this.activate(id).catch(() => {})
    }
  }

  /** P6: slots keep a suspended extension, marked, with Restart (it is not run). */
  get slotted(): InstalledExtension[] {
    if (this.safeMode) return []
    return this.extensions.filter((x) => x.enabled && !x.incompatible)
  }

  get(id: string): InstalledExtension | undefined {
    return this.extensions.find((x) => x.manifest.id === id)
  }

  // ------------------------------------------------------------ slots

  commands() {
    return this.active.flatMap((x) =>
      x.manifest.contributes.commands.map((c) => ({
        extId: x.manifest.id,
        name: x.manifest.name,
        ...c,
      })),
    )
  }

  /** P10: the actions whose `when` holds, in install order. */
  selectionActions(ctx: WhenContext) {
    return this.slotted.flatMap((x) =>
      x.manifest.contributes.selectionActions
        .filter((a) => when(a.when, ctx))
        .map((a) => ({
          extId: x.manifest.id,
          name: x.manifest.name,
          command: a.command,
          title:
            x.manifest.contributes.commands.find((c) => c.id === a.command)?.title ?? a.command,
        })),
    )
  }

  navigatorTabs() {
    return this.slotted.flatMap((x) =>
      x.manifest.contributes.navigatorTabs.map((t) => ({
        extId: x.manifest.id,
        name: x.manifest.name,
        ...t,
      })),
    )
  }

  exporters() {
    return this.active.flatMap((x) =>
      x.manifest.contributes.exporters.map((e) => ({
        extId: x.manifest.id,
        name: x.manifest.name,
        ...e,
      })),
    )
  }

  /** P9: theme packs, with any reason one cannot be offered. Theme packs run no code. */
  themes(): { extId: string; id: string; title: string; theme: Theme; problems: string[] }[] {
    return this.active.flatMap((x) =>
      x.manifest.contributes.themes.map((p) => {
        const theme = themeFromPack(x.manifest.id, p)
        return {
          extId: x.manifest.id,
          id: p.id,
          title: p.title,
          theme,
          problems: packProblems(theme),
        }
      }),
    )
  }

  adds(x: InstalledExtension): string[] {
    return contributionLabels(x.manifest)
  }

  // ------------------------------------------------------------ running

  /**
   * Run one of an extension's commands. `selection` makes book.selection readable
   * for this call only. Rejects on a timeout or a crash; the slot shows the status.
   */
  async invoke(
    extId: string,
    command: string,
    context: Record<string, unknown> = {},
    options: { selection?: { text: string; cfi: string }; kind?: 'ui' | 'work' } = {},
  ): Promise<unknown> {
    const x = this.active.find((e) => e.manifest.id === extId)
    if (!x) throw new ExtensionError(`${extId} is not running`)
    if (!x.manifest.contributes.commands.some((c) => c.id === command))
      throw new ExtensionError(`${extId} has no command ${command}`)
    const run = await this.activate(extId)
    if (options.selection) this.#selection.set(extId, options.selection)
    const limit = options.kind === 'ui' ? UI_TIMEOUT_MS : WORK_TIMEOUT_MS
    try {
      return await this.#call(extId, run, { invoke: this.#nextCall++, command, context }, limit)
    } finally {
      this.#selection.delete(extId)
    }
  }

  /** Start an extension if it is not running (lazy activation, P4). */
  activate(extId: string): Promise<Running> {
    const existing = this.#running.get(extId)
    if (existing) return existing.started.then(() => existing)
    const x = this.active.find((e) => e.manifest.id === extId)
    if (!x?.manifest.main) return Promise.reject(new ExtensionError(`${extId} has no code to run`))
    const frame = document.createElement('iframe')
    frame.setAttribute('sandbox', 'allow-scripts allow-same-origin')
    frame.setAttribute('aria-hidden', 'true')
    frame.tabIndex = -1
    frame.style.cssText = 'position:fixed;width:0;height:0;border:0;left:-10px;top:-10px'
    frame.src = `linen-ext://${extId}/_host.html?main=${encodeURIComponent(x.manifest.main)}`
    let started!: () => void
    let failed!: (e: Error) => void
    const run: Running = {
      version: x.manifest.version,
      frame,
      started: new Promise<void>((res, rej) => ((started = res), (failed = rej))),
      // eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping, nothing renders from it
      calls: new Map(),
      lastUsed: performance.now(),
      idle: 0,
      heartbeat: 0,
      awaitingPong: null,
      ready: () => started(),
      fail: (e) => failed(e),
    }
    this.#running.set(extId, run)
    this.status[extId] = 'starting'
    document.body.append(frame)
    this.#frames.set(frame.contentWindow!, extId)
    const timer = window.setTimeout(
      () => failed(new ExtensionError(`${extId} did not start`)),
      START_TIMEOUT_MS,
    )
    return run.started.then(
      () => {
        clearTimeout(timer)
        this.status[extId] = 'running'
        run.heartbeat = window.setInterval(() => this.#beat(extId, run), HEARTBEAT_MS)
        this.#touch(extId, run)
        return run
      },
      (e) => {
        clearTimeout(timer)
        void this.#failure(extId, String(e.message ?? e))
        throw e
      },
    )
  }

  unload(extId: string) {
    const run = this.#running.get(extId)
    if (!run) return
    this.#running.delete(extId)
    clearTimeout(run.idle)
    clearInterval(run.heartbeat)
    for (const c of run.calls.values()) {
      clearTimeout(c.timer)
      c.reject(new ExtensionError(`${extId} stopped`))
    }
    if (run.frame.contentWindow) this.#frames.delete(run.frame.contentWindow)
    run.frame.remove()
    this.#subscribed.delete(extId)
    if (this.status[extId] === 'running' || this.status[extId] === 'starting')
      this.status[extId] = 'idle'
  }

  /** “Restart” (Screens 11, 12): clear the failures and let it start again. */
  async restart(extId: string) {
    this.unload(extId)
    await invoke('extension_restart', { id: extId })
    this.status[extId] = 'idle'
    await this.load()
    // A showing tab mounted while stopped never activated its worker: start it now,
    // or the tab would show with nothing behind it.
    if (this.visibleTabs.has(extId) && this.active.some((x) => x.manifest.id === extId))
      await this.activate(extId).catch(() => {})
  }

  /** A8 events to extensions that listen (annotations.read; read-only, P§18). */
  emitAnnotationEvent(event: 'created' | 'changed' | 'deleted', annotation: unknown) {
    for (const x of this.active) {
      if (
        !x.granted.includes('annotations.read') ||
        !x.manifest.activation.includes('onAnnotations')
      )
        continue
      void this.activate(x.manifest.id)
        .then((run) => {
          if (this.#subscribed.has(x.manifest.id))
            run.frame.contentWindow?.postMessage({ event, payload: annotation }, '*')
        })
        .catch(() => {})
    }
  }

  /**
   * 1.1 (PROVISIONAL): a reading session ended. Extensions with `reading.sessions` and
   * `onReadingSessions` hear it; the book's title and identifier only with `book.metadata`.
   */
  emitReadingSession(
    session: import('../reader/sessions').ReadingSession,
    book: { title: string; identifier: string | null },
  ) {
    for (const x of this.active) {
      if (
        !x.granted.includes('reading.sessions') ||
        !x.manifest.activation.includes('onReadingSessions')
      )
        continue
      const payload = {
        ...sessionForExtensions(session),
        ...(x.granted.includes('book.metadata') ? { book } : {}),
      }
      // A worker started for this event subscribes as its main script runs: wait for that.
      const id = x.manifest.id
      void this.activate(id)
        .then(async (run) => {
          for (let i = 0; i < 40 && !this.#subscribed.has(id); i++)
            await new Promise((r) => setTimeout(r, 50))
          if (this.#subscribed.has(id))
            run.frame.contentWindow?.postMessage({ event: 'sessionEnded', payload }, '*')
        })
        .catch(() => {})
    }
  }

  /** Post a message to a running extension's frame (theme to a tab page, etc.). */
  isRunning(extId: string) {
    return this.#running.has(extId)
  }

  // ------------------------------------------------------------ internals

  #touch(extId: string, run: Running) {
    run.lastUsed = performance.now()
    clearTimeout(run.idle)
    run.idle = window.setTimeout(() => {
      if (run.calls.size || this.visibleTabs.has(extId) || this.#subscribed.has(extId))
        return this.#touch(extId, run)
      this.unload(extId)
    }, IDLE_UNLOAD_MS)
  }

  #call(
    extId: string,
    run: Running,
    message: { invoke: number; command: string; context: unknown },
    limit: number,
  ) {
    return new Promise<unknown>((resolve, reject) => {
      const timer = window.setTimeout(() => {
        run.calls.delete(message.invoke)
        this.status[extId] = 'not-responding'
        reject(new ExtensionError(`${extId} did not answer within ${limit / 1000} s`))
        void this.#failure(extId, 'timeout')
      }, limit)
      run.calls.set(message.invoke, { resolve, reject, timer })
      this.#touch(extId, run)
      run.frame.contentWindow?.postMessage(message, '*')
    })
  }

  /** CPU budget: a Worker busy in a loop cannot answer its heartbeat. */
  #beat(extId: string, run: Running) {
    // During a call its own timeout (2 s or 10 s) governs; the heartbeat is for idle spinning.
    if (run.awaitingPong !== null || run.calls.size) return
    const id = this.#nextCall++
    run.awaitingPong = id
    run.frame.contentWindow?.postMessage({ ping: id }, '*')
    window.setTimeout(() => {
      if (run.awaitingPong === id && this.#running.get(extId) === run) {
        this.status[extId] = 'not-responding'
        void this.#failure(extId, 'hung')
      }
    }, HEARTBEAT_GRACE_MS)
  }

  /** A crash, a timeout or a hang: stop it, tell the core, suspend after three (P6). */
  async #failure(extId: string, why: string) {
    console.warn(`extension ${extId}: ${why}`)
    this.unload(extId)
    const suspended = await invoke<boolean>('extension_crashed', { id: extId }).catch(() => false)
    this.status[extId] = suspended ? 'suspended' : 'not-responding'
    if (suspended) await this.load()
  }

  #onMessage(e: MessageEvent) {
    const extId = e.source ? this.#frames.get(e.source as Window) : undefined
    if (!extId) return
    const run = this.#running.get(extId)
    const d = e.data as {
      linenExt?: boolean
      ready?: boolean
      error?: string
      data?: Record<string, unknown>
    }
    if (!run || !d?.linenExt) return
    if (d.ready) return run.ready()
    if (d.error) {
      run.fail(new ExtensionError(d.error))
      return void this.#failure(extId, d.error)
    }
    const m = d.data ?? {}
    if (typeof m.pong === 'number') {
      if (run.awaitingPong === m.pong) run.awaitingPong = null
      return
    }
    if (typeof m.done === 'number') {
      const c = run.calls.get(m.done)
      if (!c) return
      run.calls.delete(m.done)
      clearTimeout(c.timer)
      if (this.status[extId] === 'not-responding') this.status[extId] = 'running'
      return m.error ? c.reject(new ExtensionError(String(m.error))) : c.resolve(m.result)
    }
    if (typeof m.rpc === 'number') {
      const reply = (result: unknown, error?: string) =>
        run.frame.contentWindow?.postMessage(
          error ? { reply: m.rpc, error } : { reply: m.rpc, result },
          '*',
        )
      this.#touch(extId, run)
      this.#rpc(extId, String(m.method), (m.params ?? {}) as Record<string, unknown>).then(
        (r) => reply(r ?? null),
        (err: unknown) =>
          reply(
            null,
            err instanceof Error
              ? err.message
              : typeof err === 'object' && err && 'message' in err
                ? String((err as { message: unknown }).message)
                : String(err),
          ),
      )
    }
  }

  /** P2: every call, checked against the manifest and the granted permissions. */
  async #rpc(extId: string, method: string, params: Record<string, unknown>): Promise<unknown> {
    const x = this.active.find((e) => e.manifest.id === extId)
    if (!x) throw new ExtensionError('not running')
    const need = (permission: string) => {
      if (!x.granted.includes(permission))
        throw new ExtensionError(`needs the ${permission} permission`)
    }
    const book = () => {
      if (!this.bridge) throw new ExtensionError('no book is open')
      return this.bridge
    }
    switch (method) {
      case 'commands.register': {
        if (!x.manifest.contributes.commands.some((c) => c.id === params.id))
          throw new ExtensionError(`command ${String(params.id)} is not in the manifest`)
        return true
      }
      case 'book.metadata':
        need('book.metadata')
        return book().metadata()
      case 'book.selection': {
        need('book.selection')
        const s = this.#selection.get(extId)
        if (!s)
          throw new ExtensionError('the selection is readable only while you use the extension')
        return s
      }
      case 'book.chapters':
        need('book.text')
        return book().chapters()
      case 'book.text':
        need('book.text')
        return book().text(Number(params.chapter ?? 0))
      case 'annotations.list':
        need('annotations.read')
        return book().annotations()
      case 'annotations.on':
        need('annotations.read')
        this.#subscribed.add(extId)
        return true
      case 'reading.on':
        need('reading.sessions')
        if (params.event !== 'sessionEnded')
          throw new ExtensionError(`no reading event “${String(params.event)}”`)
        this.#subscribed.add(extId)
        return true
      case 'library.list':
        need('library.read')
        return (await ipc.libraryList()).map((b) => ({
          title: b.title,
          authors: b.authors,
          language: b.language,
          progress: b.fraction,
        }))
      case 'net.fetch': {
        const url = String(params.url ?? '')
        // The core checks the host against the granted permissions again.
        return invoke('extension_net_fetch', {
          id: extId,
          url,
          method: String(params.method ?? 'GET'),
          body: params.body == null ? null : String(params.body),
        })
      }
      case 'files.save': {
        need('files.export')
        // Each use goes through the OS dialog (P3).
        const path = testHooks?.pickSavePath
          ? await testHooks.pickSavePath(String(params.suggestedName ?? 'export.txt'))
          : await save({ defaultPath: String(params.suggestedName ?? 'export.txt') })
        if (!path) return { saved: false }
        await invoke('export_save_file', { path, contents: String(params.content ?? '') })
        return { saved: true }
      }
      case 'storage.get':
        return invoke('extension_storage_get', { id: extId, key: String(params.key) })
      case 'storage.set':
        return invoke('extension_storage_set', {
          id: extId,
          key: String(params.key),
          value: String(params.value),
        })
      case 'storage.delete':
        return invoke('extension_storage_delete', { id: extId, key: String(params.key) })
      case 'storage.keys':
        return invoke('extension_storage_keys', { id: extId })
    }
    throw new ExtensionError(`unknown call ${method}`)
  }
}
