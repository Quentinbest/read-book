# Phase 7 status: extensions (macOS)

- **Last updated:** 2026-09-25
- **Before starting:**
  - Spike G passed (`docs/spikes/g-extension-network.md`).
  - D4 was not decided, and the G1 and G7 designs did not exist. Each is built to a written recommendation, marked provisional, and set aside for the owner (`docs/pending-approvals.md`, items 21–23).
- **Evidence:**
  - The in-app suite, run on Desktop 2 (`scripts/e2e.sh`; checks `P*`). The safe-mode run is `LINEN_SAFE_MODE=1 scripts/e2e.sh r '^(N3-bodymatter|P7)'`.
  - Unit tests: Rust `extensions::*`; TypeScript `src/lib/extensions/*` and `src/extensions/markdownExport.test.ts`.
  - The visual spike (Screens 11 and 12).

## Done-when (plan §5, Phase 7)

| Item | Status | Evidence |
|---|---|---|
| A hostile test extension fails to reach any undeclared host, any Tauri command, book text without the permission, another extension's storage, or more than 10 MB of storage. Verified on all three WebViews | Done on macOS (WKWebView). WebView2 and WebKitGTK are deferred with the platforms (scope decision). | e2e P2-hostile-extension, with `test.hostile`. **Network:** its declared host (127.0.0.1:8765) is reached through `net.fetch`. An undeclared host, an undeclared port, a user-info trick and a direct `fetch` are refused; the canary server saw only the declared request. **Tauri:** `__TAURI_INTERNALS__` is absent, and a forged host call is refused; the IPC canary is never called. **The book:** `book.text`, `book.selection`, `annotations.list` and `library.list` without their permissions are refused. **Storage:** it sees only its own (another extension's key reads as nothing), and 12 MB is refused at 10 MB. Unit tests (Rust) cover the core's own checks: `net.fetch` refuses undeclared hosts before connecting, and storage is per extension and capped. |
| The watchdog restarts and suspends a hanging or crashing extension as P6 specifies; the selection bar marks it “Not responding”; reading continues (Screen 12) | Done | e2e P6-watchdog, with `test.hang` and `test.crash`. **Hang:** a busy loop fails its command after 10 s, while a page turn goes through. The selection “⋯” shows “Stall · Not responding” and “Restart Hang”. After three failures it is suspended, and Restart clears it. **Crash:** an extension that throws as it loads fails its call and counts as a crash. Rust unit test: three failures in ten minutes suspend. |
| Safe mode starts with every extension off | Done | e2e P7-safe-mode in a `LINEN_SAFE_MODE=1` launch: no extension is active, there are no extension commands in ⌘K, and Settings shows the banner. ⇧ held at launch and “Restart without extensions” set the same flag (`ext_commands::launch_in_safe_mode`). In a normal launch the check sees the button and the ⇧ hint. |
| An extension built for an unsupported major version is disabled at load with a reason | Done | e2e P5-incompatible-at-load: installed under the check, it is not run, and Settings says “Turned off: it was built for Linen API ^2.0; this version of Linen supports API 1.x”. e2e P5-install-consent: installing it is refused with the same reason. |
| Markdown Export output matches a golden file | Done | `src/extensions/markdownExport.test.ts` runs the built-in's own script against `src/extensions/golden/`. e2e P8-pinned-and-export exports a real book's highlight through Notes › Export as Markdown. |
| Visual baselines for Screen 11 and the Screen 12 extension-failure state are approved | **Waiting for the owner** | `docs/visual/phase7-review.html`; `docs/pending-approvals.md` item 24 |

## Work items

| Item | Status |
|---|---|
| Manifest schema and validation (P1); install from file; consent (G1, P3); enable and disable; update re-prompts; uninstall asking about data (P5) | Done. **Checks and install:** strict validation that lists every problem, from `extensions/manifest.rs`. Install is from a `.linenext` file (D4, provisional), after the install sheet. **Updates:** an update shows only its new permissions, and the old version keeps running until the reader accepts. **Remove:** asks “Keep its data / Delete its data”; kept data returns on reinstall. **Built-ins:** they can be turned off, not removed. |
| Extension host (P2, P4): one Worker per extension with the Spike G isolation; sandboxed frames for Navigator tabs, styled by the style kit; lazy activation and idle unload; a permission check on every brokered call; semver compatibility | Done (`src/extensions/host.svelte.ts`, `src-tauri/src/extensions/`). **Frames:** tab frames run on the extension's own origin, as Spike G found necessary on WebKit. **Isolation:** they have no same-origin access to the app. |
| Watchdog (P6); safe mode and “Restart without extensions” (P7) | Done. **Timeouts:** 2 s for UI, 10 s for work. **Hangs:** a heartbeat catches a Worker spinning between calls. **CPU and memory:** the CPU budget is enforced through the heartbeat. WebKit gives no way to measure a Worker's memory (`measureUserAgentSpecificMemory` is Chromium-only), so the memory budget is not enforced; it is the one part of P6 not met. |
| Slots | Done: <ul><li>⌘K commands.</li><li>Selection “⋯” actions with `when` clauses.</li><li>A Navigator tab: one shows by name; more collapse under “More”.</li><li>Theme packs, listed with the built-in themes when they pass X1; derived from five colours.</li><li>Notes › Export.</li><li>Read-only annotation events.</li><li>Pinning in the ⋯ menu.</li></ul> |
| Markdown Export ported to the extension API and listed as “Built-in” | Done |
| Samples: Dictionary and a Night Owl-style theme pack | Done, in `examples/extensions`. Dictionary looks words up at dictionaryapi.dev and shows them in its Definitions tab. |
| Extension API reference in `docs/extensions/` | Done: `README.md` and `linen.d.ts` |

## Also done in this phase

- **End-to-end runs on Desktop 2** (the owner's instruction). The harness window moves to another desktop before it shows (`LINEN_SPACE`, `scripts/e2e.sh`), so the screen in use never changes.

## Found and fixed

- **The selection text was about to reach extensions without their permission.** The reader passed it in the command's context. It now comes only through `linen.book.selection()`, which needs `book.selection` and works only while the reader is using the extension.
- **The heartbeat stopped a busy command at 7 s,** before the 10 s it is allowed. The heartbeat now skips a Worker that has a call running; the call's own timeout governs.
- **Core errors reached extensions as “[object Object]”.** The host now passes on their message.
- **Prettier rewrote the golden Markdown** (`*Yellow*` → `_Yellow_`). Golden files are now excluded from formatting.
- **Scroll-mode position restore (B8) is fragile.** One extra `await` in the reader's opening sequence (loading the pinned extensions) made the reopened position land 2% early, every time. The load no longer waits in that sequence and B8 passes again, but a restore this sensitive to timing should be made robust in Phase 8.
