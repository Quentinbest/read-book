// Shared helpers for the Phase 0 spike harness (docs/spikes/). Not product code.

import { invoke } from '@tauri-apps/api/core'
import 'foliate-js/view.js'
import type { Book, View } from 'foliate-js/view.js'

export interface SpikeResult {
  spike: string
  criteria: Criterion[]
  raw: Record<string, unknown>
}

export interface Criterion {
  id: string
  description: string
  /** pass | fail | deferred (not testable on macOS only) | manual (needs a person) */
  verdict: 'pass' | 'fail' | 'deferred' | 'manual'
  evidence: string
}

const logEl = () => document.getElementById('log')!

export function log(...parts: unknown[]) {
  const line = parts.map((p) => (typeof p === 'string' ? p : JSON.stringify(p))).join(' ')
  logEl().textContent += line + '\n'
  console.log(line)
  void invoke('spike_log', { line }).catch(() => {})
}

export async function readCorpus(name: string): Promise<File> {
  const bytes = await invoke<ArrayBuffer>('spike_read_corpus', { name })
  return new File([bytes], name, { type: 'application/epub+zip' })
}

export const nextFrame = () => new Promise<number>((r) => requestAnimationFrame(r))
/** Resolves after the frame following the current one has been painted. */
export const painted = async () => {
  await nextFrame()
  await nextFrame()
  return performance.now()
}
export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

export function stats(values: number[]) {
  const s = [...values].sort((a, b) => a - b)
  const q = (p: number) => s[Math.min(s.length - 1, Math.round((s.length - 1) * p))]
  const r = (x: number) => Math.round(x * 100) / 100
  return { n: s.length, min: r(s[0]), p50: r(q(0.5)), p95: r(q(0.95)), max: r(s[s.length - 1]) }
}

/** Seeded PRNG so every run samples the same positions. */
export function rng(seed: number) {
  let x = seed >>> 0 || 1
  return () => {
    x ^= x << 13
    x ^= x >>> 17
    x ^= x << 5
    return (x >>> 0) / 0x100000000
  }
}

/** Reading styles for the L1–L5 canvas: 66 ch measure, 1.55 line height. */
export function readerCss(fontPx: number) {
  return `
    html { font-size: ${fontPx}px; line-height: 1.55; }
    body { font-family: Literata, Georgia, serif; }
    p { orphans: 1; widows: 1; }
  `
}

/** L1: 66 ch, about 640 px at 19 px. */
export const measurePx = (fontPx: number) => Math.round((fontPx * 640) / 19)

export interface OpenedView {
  view: View
  openMs: number
  firstPageMs: number
}

/** Create a foliate-view inside #reader at the given size and open a corpus book. */
export async function openView(
  name: string,
  opts: { width?: number; height?: number; fontPx?: number; onBook?: (book: Book) => void } = {},
): Promise<OpenedView> {
  const host = document.getElementById('reader')!
  host.replaceChildren()
  host.style.width = `${opts.width ?? 1280}px`
  host.style.height = `${opts.height ?? 800}px`
  const t0 = performance.now()
  const file = await readCorpus(name)
  const view = document.createElement('foliate-view') as View
  host.append(view)
  view.style.display = 'block'
  view.style.width = '100%'
  view.style.height = '100%'
  const { makeBook } = await import('foliate-js/view.js')
  const book = await makeBook(file)
  opts.onBook?.(book)
  await view.open(book)
  const t1 = performance.now()
  view.renderer.setAttribute('max-inline-size', `${measurePx(opts.fontPx ?? 19)}px`)
  view.renderer.setAttribute('gap', '6%')
  // L8: one column below 1480 px; the two-page spread starts there.
  view.renderer.setAttribute('max-column-count', (opts.width ?? 1280) >= 1480 ? '2' : '1')
  view.renderer.setStyles(readerCss(opts.fontPx ?? 19))
  await view.init({ showTextStart: true })
  const t2 = await painted()
  return { view, openMs: t1 - t0, firstPageMs: t2 - t0 }
}

export async function report(result: SpikeResult) {
  const info = await invoke('spike_info')
  const json = JSON.stringify({ ...result, info, userAgent: navigator.userAgent }, null, 2)
  const path = await invoke<string>('spike_report', { name: result.spike, json })
  log(`wrote ${path}`)
}
