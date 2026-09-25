<script lang="ts">
  // A book's cover (E6; Screen 01): the image extracted at import, or a generated
  // cover in the book's tint with its title and author set on it.
  import type { Book } from './ipc'
  import { coverUrl } from '../reader/loader'
  import { generatedTint } from '../lib/library/tint'

  let {
    book,
    width,
    height,
    lettering = true,
  }: { book: Book; width?: number; height: number; lettering?: boolean } = $props()

  let failed = $state(false)
  const src = $derived(book.cover_path && !failed ? coverUrl(book.id, book.content_hash) : null)
  const author = $derived(book.authors.join(' · '))
</script>

<div
  class="cover"
  class:image={!!src}
  style:width={width ? `${width}px` : undefined}
  style:height="{height}px"
  style:background={src ? undefined : (book.generated_cover_tint ?? generatedTint(book.title))}
  aria-hidden="true"
>
  {#if src}
    <img {src} alt="" loading="lazy" decoding="async" onerror={() => (failed = true)} />
  {:else if lettering}
    <span class="ct" class:long={book.title.length > 32}>{book.title}</span>
    <span class="ca">{author}</span>
  {/if}
</div>

<style>
  /* Screen 01: a book's edge on the left, a soft shadow below. */
  .cover {
    box-sizing: border-box;
    border-radius: 3px 6px 6px 3px;
    padding: 16px 14px;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    overflow: hidden;
    box-shadow:
      0 1px 2px rgba(40, 30, 20, 0.14),
      0 6px 16px rgba(40, 30, 20, 0.06);
    /* Generated covers are light in every theme; their lettering keeps Paper's ink. */
    color: #22201c;
    flex-shrink: 0;
  }
  .cover.image {
    padding: 0;
    background: var(--hairline);
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .ct {
    font: 500 17px/1.15 var(--font-reading, Literata, Georgia, serif);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 5;
    line-clamp: 5;
    -webkit-box-orient: vertical;
  }
  .ct.long {
    font-size: 14px;
  }
  .ca {
    font: 500 10px var(--font-ui);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: #3f3a33;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
</style>
