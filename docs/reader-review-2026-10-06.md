# Reader review: jumps, Contents wheel, minimum size, Scroll-mode reveal (2026-10-06)

- **Asked for:** the owner, 2026-10-06, from manual testing: review and fix (1) Contents and search jumps that stutter or miss, (2) the wheel over Contents also moving the book, (3) a minimum reader size (Apple Books as the reference; dimensions undecided), (4) the top edge not revealing the bars in Scroll mode. Reproduce first; report causes, changes and verification as Pass / Fail / Not verified / Pending human acceptance; no invented thresholds or dimensions.
- **Branch:** `worktree-reader-nav-fixes`, from `main` at `bcb174c` (0.2.0).
- **Evidence:** `docs/spikes/raw/reader-review-2026-10-06/` (run logs before and after), `docs/visual/survey/` (size captures).

## How it was reproduced

The in-app suite gained checks that drive the real app with **real input**, because two of the four problems live where synthetic DOM events never go. The wheel over the Navigator reaches the reader through AppKit's scroll monitor (`native-scroll`), not the DOM. A pointer over the text in Scroll mode lands in a book's frame, not in the reader's page.

- `spike_scroll_wheel { hid: true }` and `spike_mouse` post CGEvents through the HID tap, as a real mouse's arrive. The system cursor moves during these checks and is put back afterwards (`spike_cursor`). Events posted to the process alone reach AppKit's monitor but not WebKit: WebKit scrolls and hovers by the real cursor. `input-paths` checks the harness itself: a posted wheel scrolls the Contents list, and a move lands in the page at the top margin and in a frame over the text.
- **Filming jumps.** Every animation frame from the click until nothing has changed for 1 s, the checks record which section and which words show at the top of the text, read from the frame under that point. A good jump shows the old place, then the target, then nothing else. A *stray* is another section on the way; a *move after arriving* means the text moved again once the target showed. A *blank* means no page under the point while the view loads; blanks are reported, not failed.

| Item | Check | What it does |
|---|---|---|
| 1 | `N6-contents-jumps` | Moby-Dick (Standard Ebooks). Contents rows 40, 5, 6, 6 (again), 90, 12, then two choices 120 ms apart. Pages and Scroll; docked (1400 px) and floating (1000 × 760). |
| 1 | `F6-result-jumps` | Search “Queequeg” (252 results). Results 1, 127, 4, 252, 5, 5 (again), clicked in the panel (chapters unfolded as a reader would); Pages and Scroll. The match must also be on screen. |
| 2 | `S2-navigator-wheel` | Real wheel (lines) and trackpad (pixels) over Contents: down, at the bottom, at the top, up at the top. The book must not move. Pages and Scroll; docked and floating. |
| 4 | `S9-edge-reveal-modes` | Real pointer: over the text, then the top edge, the lower part of the top zone, the bottom edge; then away. Pages and Scroll. Then real clicks on the bottom bar's progress label (Go to) and the top bar's Contents. |
| 3 | `L8-minimum-size` | After the decision: a real drag of the window's corner toward 200 × 150 must stop at 760 × 480. At that size, the top bar's items must not overlap, the bottom bar, Aa and Go to must fit, and a real click on Contents must open it. |
| 3 | `size-survey` (opt-in) | Steps the window from 1280 × 800 down to 420 × 340; measures the text column, the bars, Aa, Go to, the Navigator and the library; captures each step. |

To repeat (spike build, see CLAUDE.md):

```sh
scripts/e2e.sh r '^(input-paths|N6-contents-jumps|F6-result-jumps|S2-navigator-wheel|S9-edge-reveal-modes)$'
scripts/e2e.sh r '^size-survey$'
```

## Findings before the fix (unchanged code, real input)

**1 · Jumps.** Pages mode: every Contents and search jump went straight to its target in one change, about 30–45 ms after the click; a few showed one blank frame while foliate-js loaded the chapter into the visible view. Scroll mode:

- **Failed jumps.** In the docked run, jumps 3–6 never moved the screen: it stayed on the “Moby Dick” title page while Contents asked for I, LXXXV and VII, and two quick choices ended there too. The engine's state after the failure: views `[6, 5, 6, counter]` for slots showing 5, 6 and 7. One view was listed twice and another lost.
- **Stutter.** A jump to another chapter first drew the new chapter at the old scroll position (“…thead” of XXXV, at 39 ms), then jumped to its start (113 ms). Every cross-chapter search result in Scroll mode did the same: drawn at the wrong place, then moved 65–150 ms later.

**2 · Wheel over Contents.** Wheel and trackpad over Contents turned pages (Pages) or scrolled the text by up to 960 px (Scroll), docked and floating. This also happened at the list's top and bottom.

**4 · Scroll-mode reveal.** Pages: the top edge revealed both bars. Scroll: the top edge, the lower top zone and the bottom edge revealed nothing; under the pointer was `foliate-view` (a book's frame).

## Causes

1. **Scroll jumps (`engine.ts`).** Scroll mode keeps three views (`#current`, `#next.view`, `#prev.view`) and stacks them as slots. `#onScroll` made whichever slot was under the middle of the window `#current`, without giving the neighbours the other two views. Once the reader had scrolled into a neighbour, `#current` and `#next.view` (or `#prev.view`) were the same view, and `#views()` listed it twice and lost one. The next jump's rebuild (`#scrollToTarget`) took “the other two views” from that list, got one, and stopped partway: the target was loaded into a view, but the stack was never placed. That depends on where the reader last scrolled, which matches “sometimes”.
   **Stutter:** a jump loaded the new chapter into the view on screen at its old position, then reloaded the two neighbours in place (also on screen), and only then placed the stack and scrolled. Each step was visible.
2. **Wheel (`Reader.svelte`).** Scroll mode and page turns follow AppKit's scroll stream (`native-scroll`), which is not a DOM event and does not stop at the panel. The listener skipped only `.chrome, dialog, .popover`; the Navigator is an `aside.navigator`, so the book took every wheel event over it. WebKit scrolled the list as well. At the list's ends, only the book moved.
3. **Minimum size.** The main window has none (`tauri.conf.json`); see below.
4. **Reveal (`Reader.svelte`).** The edge zones are watched through the reader's `pointermove`. In Scroll mode the text runs under the window edges (G8), so the edges are over a book's frame. `onBookPointerMove` passed moves on only once a reveal was under way, so the dwell never started.

## Changes

- **`engine.ts` (B8).**
  - `#makeCurrent` keeps the three views distinct: the view the reader scrolls into swaps roles with the neighbour that held it.
  - A jump to a chapter already in the stack scrolls to it (`#scrollViewFor`) without loading it again.
  - A jump to another chapter hides the stack while that chapter loads, then shows it alone at its final place, and loads its neighbours out of sight. They join above and below without moving it. Screen sequence: old place, a blank, the target, still.
  - When the stack already holds the target and its neighbours (e.g. a relayout), the stack is re-measured and placed again, then aligned.
  - `goToText` (search results) takes the same path and supersedes a stack still being built.
- **`Reader.svelte`.**
  - The wheel stream skips `.navigator` too, docked or floating, at any scroll position.
  - Frame pointer moves that fall in an edge zone start the reveal (`edgeAt`), as moves over the page do. S9 and S10 are unchanged: both bars, hidden on leaving; the 3 s delay applies only to the full controls.
- **Harness (spike builds only):** `spike_mouse`, `spike_cursor`, `spike_scroll_wheel { hid }`; the checks above; opt-in checks (`optIn`), which run only when selected.

- **Item 3, after the owner chose option A (2026-10-06, `docs/decisions.md`):** the main window's `minWidth`/`minHeight` are 760 × 480, in `tauri.conf.json` and the spike config. The harness gained `spike_mouse_drag`, which drags as a person does: AppKit's minimum limits a person's resize, not `setSize`.

## Minimum size (item 3)

**Window or reading area.** It belongs to the **application window**. Linen has one main window, which holds the library and the reader. The reading area is that window minus a docked Navigator, and the Navigator docks only from 1100 px (L8). Below that it floats over the text. At any size proposed here, the reading area is the whole window, so there is no embedded area to constrain. The mechanism is the one the Settings window already uses (`minWidth`/`minHeight`; Settings is 720 × 480): set them on `main` in `src-tauri/tauri.conf.json` (and the spike config).

**What the existing layout needs.** Measured by `size-survey`, from the captures in `docs/visual/survey/`:

| Window | Text at 19 px | Reader top bar | Library header |
|---|---|---|---|
| 760 × 560 | 640 px = 66 ch, 13 lines | clear | clear (wide layout) |
| 700 × 520 | 66 ch, 12 lines | clear | narrow layout: “Library” touches the window buttons |
| 640 × 480 | 592 px = 61 ch, 10 lines | clear | as at 700 |
| 600 × 460 | 552 px = 57 ch, 10 lines | the title runs over the Contents button | as at 700 |
| 560 × 420 | 53 ch (below L1's 56), 8 lines | title over Contents and Library | as at 700 |
| 480 × 380 | 45 ch, 7 lines | title over the tools | as at 700 |

- Aa and Go to fit at every size measured (Aa scrolls in short windows, `11-aa-short-window`).
- The floating Navigator (320 px) takes 42% of the width at 760 px, 50% at 640 and 67% at 480.
- L1's minimum measure (56 ch at the default 19 px) needs a window of at least 591 px.
- The top bar's breaking point depends on the title's length, because the title does not truncate (see Side findings). The figures above are for “Moby Dick · LXXII: The Monkey-Rope”.
- **Apple Books (measured here, macOS 14):** its library window stops at **1001 × 530**. Its book window was **not measured**: that needs a book open in the owner's Books library, which changes its reading state. Not verified.

The proposal, with options and a recommendation, is item 53 in `docs/pending-approvals.md`. The owner chose **A, 760 × 480**, on 2026-10-06.

## Verification

All runs were on this Mac (macOS 14, current desktop), with the spike build of this branch.

| Owner's item | Result | Evidence |
|---|---|---|
| 1 · Contents jumps, both modes, docked and floating, repeated, across chapters | **Pass.** Before: Scroll-mode jumps failed (docked: jumps 3–6 and the double choice) and stuttered. After: every jump shows the old place, at most one blank, then the target, and nothing moves after it arrives. The target shows 20–110 ms after the click in Scroll and 30–95 ms in Pages. | `N6-contents-jumps`; logs 2 and 4 |
| 1 · Search-result jumps, both modes, back and forth, the same one twice | **Pass.** Before: every cross-chapter result in Scroll mode was drawn at the wrong place, then moved. After: one change, and the match is on screen. | `F6-result-jumps`; logs 1 and 4 |
| 2 · Wheel over Contents, at the top and bottom too | **Pass**, Pages and Scroll, docked and floating. The book does not move; the list scrolls. | `S2-navigator-wheel`; logs 2 and 4 |
| 3 · Minimum size: which window, and the dimensions | The application window; the owner chose 760 × 480 (item 53). | `size-survey`; `docs/visual/survey/` |
| 3 · Resizing stops at 760 × 480; the reader and its controls work there | **Pass.** A real corner drag toward 200 × 150 stopped at 760 × 480; the top bar is clear, the bottom bar, Aa and Go to fit, and a real click on Contents opens it. | `L8-minimum-size`; log 5 |
| 3 · Apple Books book-window minimum | **Not verified** (needs a book opened in the owner's Books library). Its library window: 1001 × 530. | — |
| 4 · Top (and bottom) edge in Scroll as in Pages; both bars work | **Pass.** Top edge, lower top zone and bottom edge reveal both bars in both modes, and hide when the pointer leaves. Real clicks open Go to (bottom bar) and Contents (top bar). | `S9-edge-reveal-modes`; logs 2 and 4 |
| 1, 2, 4 by hand (real trackpad momentum, the owner's books) | **Pending human acceptance** (item 54). | — |

Regression checks:
- **Full in-app suite (log 4):** 121 of 122 pass. That includes B8 Scroll mode, real-wheel, the N6, F5–F7, S2, S9 and S13 checks, the reflow and open budgets, and D1 (no uncaught error in the whole run).
  - The one failure is `P2-hostile-extension`, **not verified**. Its canary server could not bind 127.0.0.1:8765, which an unrelated `python -m http.server 8765` (started 20:46 that day) was holding, so it saw no requests. It does not touch the code changed here.
- **Full in-app suite with the 760 × 480 minimum (log 5):** 119 of 123 pass, including `L8-minimum-size`, every check added here and D1. The four failures:
  - `P2-hostile-extension`: as above, the port was still taken.
  - `F5-results-land`: one result's active mark was missing. It passes in both reruns (log 6). It flaked the same way in the 1.1 runs, and the Pages-mode search path is unchanged here.
  - `budget-reflow`: one of four resizes took 158 ms (budget 150; the others 124, 67 and 45 ms). It passed in log 4 and in both reruns.
  - `S9-edge-reveal-modes`: the reveal passed; a real click on a revealed bar did not register. Before the change below, that happened in 3 of the 7 runs that included this check. The click was spent activating the window (I11). The real-input checks now make the window active first, and the click step gets three attempts, each logged. The reveal itself stays strict. Since then: 3 runs of S2, S9 and L8, with no retry needed.
  - One of those 3 runs had a stray `S2-navigator-wheel` failure: the book jumped from section 6 to 38, as a click on a Contents row would. The check never clicks, and the failure did not recur. These checks move the real cursor into the window, so another click on this Mac at that moment would do exactly this. **Not explained.**
- **Visual (`scripts/e2e.sh v`, `node tests/visual/compare.mjs`):**
  - 24 of 25 baselines match. `17-goto` differs by 0.141%, only in the field's focus ring and the window buttons: the active-window difference already noted for that capture in `docs/release-1.1-status.md`.
  - The seven `g8-*` captures, including Scroll reading and the chapter join, are pixel-identical to the committed ones.
- **Local CI (`scripts/ci-local.sh`):** green. 299 TypeScript tests, 64 + 2 Rust tests, clippy `-D warnings` (also with `--features spikes`), rustfmt, Prettier, ESLint, svelte-check, design export.

Logs, in `docs/spikes/raw/reader-review-2026-10-06/`:
1. `1-before-programmatic-clicks.txt`, the first harness: clicks in the page, and wheel and moves posted to the process. Its wheel and pointer results are void: WebKit never saw those events (see `input-paths`).
2. `2-before-real-input.txt`: unchanged code, real input.
3. `3-after-fix.txt`: the fixed code, the size survey. Its one F6 failure was the check's own mistake (a result in the chapter already showing), corrected before log 4.
4. `4-full-suite.txt`: the whole suite on the fixed code.
5. `5-full-suite-min-size.txt`: the whole suite with the 760 × 480 minimum (`docs/spikes/raw/e2e-reader.json` holds this run).
6. `6-real-input-reruns.txt`: reruns of the real-input checks and of log 5's failures.

The real-input checks (`input-paths`, `S2-navigator-wheel`, `S9-edge-reveal-modes`, `L8-minimum-size`) move the system cursor, activate Linen's window and click in it. Run them with nobody using the Mac.

## Side findings (not changed; outside the four items)

- **The reader's top-bar title does not truncate.** Below about 600 px, or at wider windows with a long title and chapter, it runs over the buttons.
- **The library's narrow header (below about 720 px) puts “Library” against the window buttons.**
- **Message lines** (“Resumed in …”, “Back to p. …”) sit at the middle of the window's foot. While one shows, a pointer on it does not reveal the bars: the message is outside the reader. This is the same in both modes; the reveal check moves the pointer off-centre.
- **Pages-mode jumps show at most one blank frame** while foliate-js loads the chapter into the visible view. Scroll-mode jumps now do the same instead of drawing the chapter at the wrong place.
