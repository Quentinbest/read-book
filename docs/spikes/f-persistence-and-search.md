# Spike F: persistence and search

**Verdict: pass on macOS.** Every criterion is met, most with a wide margin. Two search bugs the spike exposed are fixed in the product code.

- **Date:** 2026-09-24
- **Machine:** see `reference-machines.md`
- **Raw results:** `raw/f-search.json`; persistence numbers below
- **Code:** `spikes/f-persistence/` (Rust benchmark), `src/lib/search/` (product search code), `src/spikes/f-search.ts` (in-app harness)

## Persistence (Rust, rusqlite 0.40 bundled SQLite, WAL)

`spikes/f-persistence` upserts a `positions` row, one transaction per write, 1,000 writes per run, 3 runs.

| Measure | Budget | Result | Verdict |
|---|---|---|---|
| Progress write, `synchronous=NORMAL` | < 5 ms p95 | p95 0.028–0.064 ms (max 19.8–21.8 ms at checkpoints) | pass |
| Progress write, `synchronous=FULL` | (for comparison) | p95 1.8–2.8 ms | — |
| SIGKILL during writes | lose at most the last uncommitted write; the database opens cleanly | 200/200 runs: `integrity_check` ok, every write reported as committed was present | pass |

The store uses `synchronous=NORMAL`: WAL with NORMAL is durable across process crashes, which is what the criterion tests. It is not durable across power loss for the last transactions. On macOS, `fsync` does not flush the drive cache anyway (full durability needs `PRAGMA fullfsync`), so for crash safety FULL would add latency without adding protection.

## Search (in WKWebView, Web Worker)

The harness extracts the text of every section of the Standard Ebooks Moby-Dick (145 sections, 1.27 M characters, 147 ms). The extracted text is the on-disk cache form. Each timed run starts a fresh worker, parses the cache, normalises it and scans for a query; 20 runs.

| Criterion | Result | Verdict |
|---|---|---|
| Search all 135 chapters from the cache in < 300 ms | Cold (parse + normalise + scan): p50 94 ms, p95 114 ms. Warm scan: p95 22 ms (worst query, “the”, 19,959 results) | pass |
| Diacritic folding (é → e) | `etait` finds 15 matches of “était” in `idpf-sous-le-vent.epub` | pass |
| CJK matching, from 1 character | `智に働けば` → 1 match, `山` → 103 matches in `idpf-kusamakura-japanese-vertical-writing.epub` | pass |
| Quoted phrases match exactly | `"Call me Ishmael"` → 1 (the opening line) | pass |

### Bugs found and fixed

The first run failed the CJK criterion. The sample output showed two defects in the search code, both now fixed and covered by unit tests (`src/lib/search/*.test.ts`):

1. **Kana voicing marks were folded away.** Unicode NFKD splits ば into は plus a combining mark, and diacritic folding then dropped the mark, which turns one letter into another. CJK text is now normalised with NFKC and keeps its marks. Normalisation also works on whole clusters (a base character plus its marks), so decomposed input matches precomposed input.
2. **Ruby readings and block boundaries were in the text.** `textContent` gave “智ちに働はたらけば” and ran paragraphs together (“…se ficherait.Eh bien…”). A new extractor (`src/lib/search/extract.ts`) skips `<rt>` and `<rp>`, separates block elements, and keeps a map back to DOM text nodes, which Phase 4 needs to place marks (F5).

### Notes for later phases

- **B6 (result caps).** “the” returns 19,959 results in 143 chapters in 24 ms. Scanning is not the bottleneck; rendering that many results is. B6 should cap results per chapter in the UI rather than in the scan.
- **Quote semantics (assumption, for B6).** Quoted queries are matched literally, with case and diacritics, after whitespace and quote-mark normalisation; unquoted queries ignore both (F2). P§7 says only that “quotes match exact phrases”.
