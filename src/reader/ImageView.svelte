<script lang="ts">
  // Image view (N11; G8, approved 2026-09-24). The page ground at 97% covers the
  // book; the image fits inside 48 px with its caption. ⌘+ ⌘− and the buttons zoom;
  // drag pans once the image is larger than the window. Esc or a click outside
  // closes, and focus returns to the image in the text. Night dims it 10%.
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings'
  import type { ImageEvent } from './engine'

  export const IMAGE_ZOOM = [1, 1.5, 2, 3, 4]

  let {
    image,
    dim,
    zoom = $bindable(1),
    onclose,
  }: { image: ImageEvent; dim: boolean; zoom?: number; onclose: () => void } = $props()

  let frame: HTMLElement | undefined = $state()
  let img: HTMLImageElement | undefined = $state()
  let fit = $state({ w: 0, h: 0 })
  let drag: { x: number; y: number } | null = null

  /** The image's size at fit: inside the window less 48 px margins and the caption. */
  function measure() {
    if (!img?.naturalWidth || !frame) return
    const maxW = frame.clientWidth - 96
    const maxH = frame.clientHeight - 96
    const s = Math.min(1, maxW / img.naturalWidth, maxH / img.naturalHeight)
    fit = { w: img.naturalWidth * s, h: img.naturalHeight * s }
  }

  export function step(dir: 1 | -1) {
    const i = IMAGE_ZOOM.findIndex((z) => z >= zoom - 1e-6)
    zoom = IMAGE_ZOOM[Math.min(IMAGE_ZOOM.length - 1, Math.max(0, i + dir))]
  }

  $effect(() => {
    const r = new ResizeObserver(measure)
    if (frame) r.observe(frame)
    return () => r.disconnect()
  })
</script>

<svelte:window onresize={measure} />

<div
  class="image-view"
  role="dialog"
  tabindex="-1"
  aria-label={image.caption || t.image.label}
  onpointerdown={(e) => {
    // A click outside the image closes the view.
    if (!(e.target as Element).closest('img, button')) onclose()
  }}
>
  <div
    class="frame"
    class:zoomed={zoom > 1}
    bind:this={frame}
    role="presentation"
    onpointerdown={(e) => {
      if (zoom > 1 && (e.target as Element).closest('img')) {
        drag = { x: e.clientX, y: e.clientY }
        e.preventDefault()
      }
    }}
    onpointermove={(e) => {
      if (!drag || !frame || !(e.buttons & 1)) return (drag = null)
      frame.scrollBy(drag.x - e.clientX, drag.y - e.clientY)
      drag = { x: e.clientX, y: e.clientY }
    }}
    onpointerup={() => (drag = null)}
  >
    <img
      bind:this={img}
      src={image.src}
      alt={image.alt}
      class:dim
      onload={measure}
      style:width={fit.w ? `${fit.w * zoom}px` : undefined}
      style:height={fit.h ? `${fit.h * zoom}px` : undefined}
    />
  </div>
  {#if image.caption && zoom === 1}
    <p class="caption">{image.caption}</p>
  {/if}
  <button type="button" class="icon close" aria-label={t.image.close} onclick={onclose}>
    <Icon name="close" size={16} />
  </button>
  <div class="zoom">
    <button type="button" class="icon" aria-label={t.image.zoomOut} onclick={() => step(-1)}
      >−</button
    >
    <button type="button" class="fit" onclick={() => (zoom = 1)}
      >{zoom === 1 ? t.image.fit : t.reader.zoomLevel(zoom)}</button
    >
    <button type="button" class="icon" aria-label={t.image.zoomIn} onclick={() => step(1)}>+</button
    >
  </div>
</div>

<style>
  .image-view {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    background: color-mix(in srgb, var(--ground) 97%, transparent);
    font-family: var(--font-ui);
    animation: fade-in var(--motion-popover, 140ms) ease-out;
  }
  @keyframes fade-in {
    from {
      opacity: 0;
    }
  }
  .frame {
    max-width: 100%;
    max-height: calc(100% - 80px);
    display: grid;
    place-items: center;
  }
  .frame.zoomed {
    position: absolute;
    inset: 0;
    max-height: none;
    overflow: auto;
    scrollbar-width: none;
    place-items: safe center;
  }
  .frame.zoomed img {
    cursor: grab;
  }
  img {
    display: block;
    max-width: none;
  }
  /* V1: images at Night are dimmed about 10%, never inverted. */
  img.dim {
    filter: brightness(0.9);
  }
  .caption {
    max-width: 640px;
    margin: 0;
    padding: 0 24px;
    text-align: center;
    font-size: 14px;
    line-height: 1.4;
    color: var(--ink-secondary);
  }
  button {
    border: 0;
    background: none;
    color: var(--ink-secondary);
    border-radius: 8px;
    font: 500 12px var(--font-ui);
    cursor: default;
  }
  button:hover {
    background: var(--hover-wash);
    color: var(--ink);
  }
  .icon {
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    font-size: 16px;
  }
  .close {
    position: absolute;
    top: 20px;
    right: 20px;
  }
  .zoom {
    position: absolute;
    right: 20px;
    bottom: 20px;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .fit {
    min-width: 44px;
    height: 36px;
  }
</style>
