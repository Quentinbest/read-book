# Writing a Linen extension

Linen extensions add to fixed places in the app: commands in ⌘K, actions for a text selection, one Navigator tab, themes, and exporters in Notes. They never reach the page or the chrome (P8). This is the reference for Host API v1. The types are in [`linen.d.ts`](linen.d.ts), and the samples are in [`examples/extensions`](../../examples/extensions).

> **Status:** Phase 7. The package format (D4), the install sheet (G1) and the extension surfaces (G7) are provisional until the owner approves them.

## A package

An extension is a zip file named `*.linenext`, with `manifest.json` at its root:

```json
{
  "id": "org.example.dictionary",
  "version": "0.4.2",
  "name": "Dictionary",
  "description": "Look up the selected word without leaving the page.",
  "engines": { "linen": "^1.0" },
  "main": "main.js",
  "activation": ["onCommand:define", "onNavigatorTab:definitions"],
  "contributes": {
    "commands": [{ "id": "define", "title": "Define" }],
    "selectionActions": [{ "command": "define", "when": "selection.words <= 3" }],
    "navigatorTabs": [{ "id": "definitions", "title": "Definitions", "page": "tab.html" }]
  },
  "permissions": ["book.selection", "network:api.dictionaryapi.dev"]
}
```

**What a package may contain.** Files of these types only: `.js`, `.mjs`, `.html`, `.css`, `.json`, `.png`, `.svg` and `.woff2`. The package may be up to 20 MB; the manifest, up to 64 KB.

**What Linen checks at install.** Everything in the manifest; the install sheet lists every problem. It refuses an `id` starting `app.linen.` (built-ins), and a package built for an API major Linen does not support.

| Field | Meaning |
|---|---|
| `id` | Lowercase letters, digits, dots and dashes; reverse-DNS style. |
| `version` | A semantic version. |
| `engines.linen` | The Host API versions it works with, as a semver range. Linen supports the current major and the previous one (P5); an extension outside them is turned off, with the reason. |
| `main` | The Worker script. Theme packs have none. |
| `activation` | When it starts: `onCommand:<id>`, `onNavigatorTab:<id>`, `onExport:<id>`, `onLookup:<id>` (API 1.1), `onAnnotations`, `onReadingSessions` (1.1). It starts lazily and is unloaded after a minute unused. |
| `contributes` | `commands`, `selectionActions` (with an optional `when`), `navigatorTabs`, `themes`, `exporters`, and `lookups` (API 1.1, experimental; see below). Up to 32 in all. |
| `permissions` | See below. Anything not declared is refused. |

## Permissions (P3)

| Permission | Grants | Consent |
|---|---|---|
| (none) | Commands, themes, UI slots, 10 MB of private storage | None |
| `book.metadata` | The open book's title, authors, language and identifier | At install |
| `book.selection` | The selected text and its place, only while the reader uses the extension | At install |
| `book.text` | The open book's full text, chapter by chapter | At install, highlighted |
| `annotations.read` | The open book's highlights and notes, and their events | At install |
| `annotations.write` | Reserved for a later version | At install, highlighted |
| `library.read` | The title, authors and progress of every book | At install |
| `reading.sessions` | Each reading session as it ends: when, how long, how far (1.1). The book's title only with `book.metadata` | At install |
| `network:<host>[:port]` | `net.fetch` to that host only; `*.example.org` covers its subdomains (strongly warned) | At install |
| `files.export`, `files.import` | Only files the reader picks in the system dialog | Each use |
| `background` | Reserved for 1.1 (C3) | At install, strongly warned |

## How it runs (§7.2)

- **The Worker.** An extension's code runs in a Worker on its own origin, `linen-ext://<id>/`. It has no network, and it can load scripts only from its own package: `importScripts('lib.js')` works, while another host, another extension or `data:` does not. It cannot create workers. Everything else goes through `linen`, whose calls are checked against the granted permissions.
- **Tab pages.** A Navigator tab's page runs in a frame on the same origin. It loads Linen's style kit with `<link rel="stylesheet" href="_kit.css">` and `<script src="_ui.js">`, which keeps it in the reader's colours. It talks to the Worker through `linenUi.post` and `linenUi.onMessage`: the Worker listens on `new BroadcastChannel('linen')`.
- **The watchdog (P6).**
  - A command has 10 s to finish; a UI contribution has 2 s.
  - A Worker busy in a loop between calls stops answering its heartbeat and is stopped.
  - Three failures in ten minutes suspend the extension. Its slots then show “… stopped responding · Restart”, and reading is never affected.
- **Safe mode (P7).** Holding ⇧ while Linen opens, or Settings › Extensions › Restart without extensions, starts Linen with every extension off.

## Lookups (Host API 1.1, experimental)

A lookup answers in the lookup peek, under the selection (Reading Lens, `docs/reading-lens-plan.md`, LK1–LK15). It is experimental until Host API 1.2 and may change.

- **Declare it** in `contributes.lookups`: `{ "id": "explain", "title": "Explain", "when": "…" }`, with `onLookup:<id>` in `activation`. A lookup needs `book.selection`; `engines.linen` must allow 1.1 (`"^1.1"`).
- **Where it shows.** In the selection's “⋯” menu, for a new selection and for a clicked highlight; in ⌘K as “Look Up with …”; and in the peek's menu above the answer, once in each language the reader uses. Not in fixed-layout books, and not in safe mode.
- **Nothing is sent before the reader chooses it.** Selecting, highlighting, copying and searching never call a lookup.
- **Answer it** with `linen.lookups.register(id, handler)`. The handler gets the request (`text`, `context`, `bookLang`, `language`) and `{ signal }`; it returns the fields below. `book.selection()` gives the same selection, with its context, while the lookup runs.
- **The context** is `{ before, sentence, after, paragraph, chapter }`: whole sentences around the selection in its own chapter, 1,200 characters at most in all. It never includes other chapters, front matter, highlights or notes; footnote markers and soft hyphens are taken out.
- **The answer** is plain text in fixed fields, which Linen renders:
  `{ status: "ok" | "needs_context" | "error", headword, term?, meaning, qualifier?, details?: [{ label, text }], source: { kind: "ai" | "dictionary", name, model? }, sent?, missing?, error? }`.
  - `qualifier` is shown with the meaning, never only under More; `missing` (for `needs_context`) says what the passage lacks; `error` is `offline`, `unauthorized`, `rate_limited` or `unavailable`.
  - Linen writes the label above the answer itself: “AI explanation · ‹model›” or “Dictionary · ‹name›”.
  - An answer with an unknown field, a label of its own, markup, or a percentage or confidence score is refused, and the peek says the answer can't be shown.
- **Closing the peek cancels the request.** `signal` aborts; whatever the handler returns afterwards is dropped. The 10 s work budget and the watchdog apply as for commands.

## Selection `when` clauses

A small closed language:
- **Variables:** `selection.words`, `selection.chars`, `selection.sentences` (1.1), `selection.language`, `book.language`, `book.lang` (1.1: the primary subtag, lowercase, `""` when the book has none or `und`), `book.fixedLayout`.
- **Values and operators:** numbers, `"strings"`, `true`/`false`, `== != < <= > >=`, `&& || !` and parentheses.

For example: `selection.words <= 3 && book.language == "en"`.

## Theme packs (P9)

A theme pack is data only: no code and no permissions. Each theme gives:
- `scheme` (`light` or `dark`);
- five colours: `ground`, `ink`, `inkSecondary`, `accent`, `hairline`;
- optionally the panel, popover and tooltip colours.

The rest is derived. A theme is listed only if it keeps Linen's contrast rules:
- text at 7:1 or more, secondary text at 4.5:1, the accent at 3:1;
- highlighted text at 7:1 or more, in every colour.

## Building a package

`scripts/corpus/generate.py` builds each folder in `examples/extensions` into a `.linenext` package (zip). Settings › Extensions › Install from file… installs it, after the install sheet.
