# Linen — next steps plan (draft)

- **Date:** 2026-10-02
- **State:** draft for the owner. Nothing here is built. The decisions it needs are in §4; once approved they become items in `docs/pending-approvals.md` and `docs/decisions.md`, and each phase gets a row in `docs/PROGRESS.md` and its own status document.
- **Follows:** `docs/implementation-plan.md` (phases 0–8 and release 1.1 are done). Same conventions: sizes S / M / L, a “Before starting” and a “Done when” list per phase, rule IDs from the rule register.

## 1. Where things stand

- **Released:** 0.1.0 (2026-09-26), 0.1.1, 0.1.2, 0.2.0 (2026-10-02, release 1.1). macOS only, by the scope decision of 2026-09-24.
- **Builds are ad-hoc signed** (item 28), so macOS warns before the first open.
- **Left from the design's roadmap** (P§18, plan §1.3):
  - 1.1: translation peek providers, metadata providers. The dictionary peek is built (item 35); extension peek providers wait for a Host API addition.
  - 1.2: TTS (a speech provider with sentence sync and a playback bar) and sync providers (a storage adapter for progress and annotations).
  - Tablet: Q2 named it the next release after desktop. Screen 13 (touch model) and the iPad column of P§16 exist; there are no full iPad screens.
  - Windows and Linux: deferred. Spike A parity, Spike E on WebView2 and WebKitGTK, NVDA and Orca were never run.
  - Phones: “Not planned” (plan §1.3).
- **No usage or demand data.** Linen has no telemetry (D1), and nothing on record says readers want another platform.

## 2. Direction

Refine and distribute the macOS app first. Find out cheaply, in CI, whether Windows and Linux are feasible. Make the iPad the next platform. Then sync, then TTS. Windows and Linux wait behind a gate; Android and phones are not planned.

Why, in short:
- **Each platform multiplies the cost of every later feature.** The features are cheapest to settle while there is one codebase to change.
- **The iPad carries the least risk.** It uses the same WebKit as macOS, so the Spike A parity risk mostly goes away, and the owner has already named it next (Q2).
- **Sync matters once there is a second device.** It is designed together with the iPad, so it comes right after.
- **Windows and Linux carry the most risk:** WebView2 and WebKitGTK are untested, and there are no test machines.

The TypeSafe check of this direction (2026-10-02) corrected three earlier reasons, and the plan uses the corrected versions:
- **The macOS-only native code isn't a blocker.** Every macOS block in `native.rs`, `native_input.rs` and `dictionary.rs` has a non-macOS fallback, so the core is already set up to compile elsewhere.
- **The Spike A fallback depends on which engine fails** (plan §5, Spike A row). If only WebKitGTK fails, ship Linux later; Readium or Electron come in only if every engine fails.
- **Windows and Linux end-to-end testing is already designed** (plan §6.1, `tauri-driver`).

| Phase | What | Size | Needs from the owner | Ships in |
|---|---|---|---|---|
| 9 | Signed, notarised macOS builds; demand signals | S | Apple Developer Program membership (§4, O1) | 0.3.0 |
| 10 | Cross-engine checks in CI (runs alongside 9) | M | Nothing | No release |
| 11 | Finish 1.1: lookup and metadata providers | M | Host API additions (O2) | 0.3.0 |
| Gate W | Windows and Linux: go or no-go | — | The decision (O7) | — |
| 12 | iPad | L+ | Channel, extensions, a test iPad (O3–O5) | 0.4.0 as a TestFlight beta |
| 13 | Sync providers (1.2) | L | The approach (O6) | 0.5.0 |
| 14 | Text-to-speech (1.2) | M–L | Approval of the playback bar | 0.6.0 |

With D6's cadence of a minor release about every six weeks, that is about six to eight months of releases. The sizes, not the dates, are the estimate.

## 3. Phases

### Checks that apply to every phase

Each phase's edge cases are listed with the phase. These apply to all of them:
- **Every new surface:**
  - It is reachable and dismissable from the keyboard and read by VoiceOver.
  - It is captured in the light and night themes and at 200% zoom (X6).
  - It follows reduced motion (V8).
- **Every new command** goes through the command registry. Its default shortcut is checked against readers' existing remaps (C4), with the reader's choice winning; the unit test is introduced in Phase 11.
- **Every schema change** gets a migration fixture (plan §6.6). An older database opens in the new release, and the new release's database is refused clearly by an older one, never damaged.
- **Every Host API addition** is checked against the manifest per call, with an `engines.linen` range test on both sides of the new version (P5).
- **Every phase** reruns the hostile corpus (Spike E) and the full in-app suite on macOS. The Mac stays the reference while other platforms are added.

### Phase 9 — Signed distribution and demand signals (S)

**Before starting:** the owner has joined the Apple Developer Program (O1) and created a Developer ID Application certificate and an App Store Connect API key for notarisation. The same membership later covers TestFlight for the iPad (Phase 12).

- **Signing in the release workflow.** Item 28 says Developer ID signing and notarisation switch on when their secrets are added.
  - Add Tauri's macOS signing variables as repository secrets: the certificate as base64 p12, its password, the signing identity, and the API key, issuer and key file.
  - Confirm that `release.yml` and `scripts/release-macos.sh` take the signed path, with the hardened runtime on.
  - Staple the notarisation ticket to the DMGs.
- **Entitlements.** Start with none beyond the hardened runtime. Add an entitlement only with a check that needs it.
  - The in-app suite runs only in the `spikes` build, never in the product build. So sign the `spikes` build with the same identity, hardened runtime and entitlements, and run the suite on that.
  - Then run a written smoke list on the notarised product DMG itself: open a book, Look Up (Dictionary Services), Open in Dictionary (`NSWorkspace`), install an extension and run its worker, export highlights, Show in Finder, the crash log.
- **Moving from ad-hoc to Developer ID.**
  - Update an installed 0.2.x through the updater to the first signed build. Updater archives are signed with the Linen update key, which doesn't change.
  - Confirm the result opens without a Gatekeeper prompt and that the library folder is untouched.
- **Edge cases the release checks must cover:**
  - **The first open with no network.** Gatekeeper reads the stapled ticket instead of asking Apple, so turn Wi-Fi off and open a freshly downloaded DMG.
  - **The Intel DMG.** There is no Intel Mac on macOS 13 or later (`reference-machines.md`), so run the x86_64 build under Rosetta on the M2: launch, open a book, Look Up.
  - **An app run straight from the disk image, or from App Translocation** (a quarantined app moved by macOS to a read-only location). It can't replace itself, so it must show item 32's “Linen x.y.z is available · Download” line and not fail silently.
  - **An admin-installed copy in `/Applications` on a standard account:** the same, item 32's path.
  - **An update that skips versions** (0.1.0 to the signed build): the updater must still accept it.
  - **A tampered updater archive:** one whose signature doesn't match is refused, so the D6 check still holds on the new release path.
- **README.** Drop “macOS asks before the first open”; describe the signed builds.
- **Demand signals, with no telemetry (D1):**
  - `scripts/release-stats.sh` reads the public GitHub download count of each release asset and appends it to `docs/release-stats.md` at every release.
    - Count DMGs (new installs) apart from `.app.tar.gz` updater archives (existing readers updating). Otherwise every daily update check that downloads inflates the demand number.
    - `latest.json` is fetched by every install every day, so it measures nothing and is left out.
    - Counts are cumulative, so record the change since the last run, not the total.
  - A “Platform request” issue template, and one line in the README inviting readers to use it.
  - Gate W and the iPad's priority read these numbers.
- **Carried over:** the oldest supported macOS (13) is still missing from `docs/spikes/reference-machines.md`. A macOS 13 virtual machine for a smoke run of the signed build is enough here; budgets on it stay open.

**Done when:**
- A pushed tag produces Developer ID-signed, notarised and stapled DMGs for Apple silicon and Intel.
- `spctl --assess --type open --context context:primary-signature` accepts them on a clean user account.
- The updater moves 0.2.0 and 0.1.0 to the signed build; a tampered archive is refused.
- The in-app suite and the visual comparison pass on the `spikes` build signed like the product, and the smoke list passes on the notarised product DMG (Apple silicon, and Intel under Rosetta).
- The edge cases above are recorded with their results in `docs/phase9-status.md`.
- The README is updated, and `release-stats.sh` has recorded 0.2.0 and the new release.

### Phase 10 — Cross-engine checks in CI (M)

This answers whether a Windows or Linux port is feasible without starting one. Nothing here ships, and no Windows or Linux UI work is done.

- **Build matrix.** Add `windows-latest` and `ubuntu-22.04` to the `build` job; `ci.yml` already names them in a comment.
  - Fix what fails to compile, lint or test, keeping each fix behind `cfg` where needed.
  - Once green, make both blocking for build and unit tests, so macOS-only code can't quietly break portability again.
  - The repository is public, so the runner minutes are free.
- **Cross-engine workflow** (`.github/workflows/cross-engine.yml`, manual and weekly):
  - Build the `spikes` binary on each OS and fetch the corpus per `corpus/manifest.json`.
  - Run the harness directly. `scripts/e2e.sh` uses `caffeinate`, which is macOS only, so it gets a per-OS branch. Linux runs under `xvfb-run` at a fixed size and DPI; the Windows runner has a desktop session.
  - Upload the raw JSON.
- **Which spikes run, and why:**
  - **A, rendering:** page count per chapter within 2% of `docs/spikes/raw/a-rendering-macos.json`, and no split lines. Literata is bundled (`public/fonts/`), so the harness must force it for a fair comparison. A new `scripts/compare-spike-a.mjs` computes the verdict.
  - **E, content isolation:** on WebView2 the custom URI schemes become `http://<scheme>.localhost` origins, so the CSP-based isolation (D-E1) must be proven there, not assumed.
  - **D, engine fidelity:** CFI and highlight anchoring must agree across engines before annotations sync between them (Phase 13).
    - Test across engines, not only within one: anchors created in WKWebView are resolved in WebView2 and WebKitGTK, and the other way round, using Spike D's fixtures.
    - They must land on the same text after whitespace collapsing, soft hyphens and Unicode normalisation (NFC and NFD forms of the same word).
- **Edge cases the comparison must cover:**
  - **A moving baseline.** The macOS counts in `raw/a-rendering-macos.json` date from 2026-09-24 and WebKit has changed since. Re-measure macOS on `macos-latest` in the same workflow run and compare against that, keeping the stored file only as history.
  - **Engine versions.** WebView2 is evergreen and runner images update. Record the exact engine version per run (plan §5, Phase 0), and report a verdict change together with the version change that came with it.
  - **Flakiness.** Run each engine twice. A verdict counts only when both runs agree, and a disagreement is itself a finding.
  - **Headless WebKitGTK.** Without a GPU, WebKitGTK can fall back to other rendering paths, or fail to paint under `xvfb`. A blank or zero-page result is a harness failure, not a parity failure, and is reported as such.
  - **Content Moby-Dick doesn't exercise:**
    - Vertical writing (I16).
    - An RTL book (I15).
    - CJK text without spaces.
    - `hyphens: auto` (the engines ship different hyphenation dictionaries).
    - A book's embedded publisher fonts, in each font format the corpus has.
    - OpenDyslexic, and Publisher styles set to Off (C5).
    - A fixed-layout book (E2).
    - The two-page spread from 1480 px (L8).
  - **What “no split lines” checks.** It must also run at 200% zoom (X6), where most lines move.
  - **Not run:** B (trackpad) and C (NVDA, Orca) need hardware and a person, so they stay deferred.
- **Report:** `docs/spikes/cross-engine.md`, with a verdict per spike and engine, and which branch of the Spike A fallback applies.

**Done when:**
- CI builds and runs unit tests green on all three OSes.
- Spikes A, D and E have recorded verdicts and raw JSON for WebView2 and WebKitGTK.
- The Gate W inputs below are filled in.

### Gate W — Windows and Linux: go or no-go (decision)

- **Inputs:**
  - Phase 10's verdicts.
  - Phase 9's download counts and platform requests.
  - Whether one mid-range Windows machine and one Linux machine can be had as reference machines (plan §5, Phase 0).
- **If go:** plan the port as its own L+ phase, picking up what the scope decision deferred:
  - Window chrome and the ⋯ app menu (G6, P§16).
  - NVDA and Orca runs.
  - Windows high contrast (X7).
  - The Windows Jump List and Linux MIME registration.
  - `tauri-driver` end-to-end tests, and visual baselines per engine.
  - Windows code signing (a paid certificate or signing service).
- **If no-go:** keep the Phase 10 workflow running weekly, so the answer stays current at little cost.

### Phase 11 — Finish 1.1: lookup and metadata providers (M)

**Before starting:** the owner approves the Host API additions (O2).

- **Host API 1.1** (a semver minor, P2). Each new call is checked against the manifest like the rest.
  - `lookup.registerProvider({ id, title, kind: 'definition' | 'translation' }, handler)`. The handler gets the selected text, up to 80 characters as in item 35, and the book's language. It returns structured fields: headword, senses or translation, attribution, link.
  - Linen renders those fields in the existing peek. Extensions don’t draw in the chrome (P§18), and the peek keeps one look.
  - Permissions: `book.selection` (only when invoked), plus `network:<host>` with consent at install.
- **Translation peek.** A “Translate” item joins Look Up in the selection bar and the context menu, and shows only when a translation provider is installed. The target language follows the system, with an override in Settings › Reading.
  - Linen ships no built-in translation, because it sends nothing anywhere.
  - An example extension in `examples/` shows the pattern.
- **Metadata providers.** `metadata.registerProvider` gets the book's metadata (needs `book.metadata`) and returns extra information: description, series, subjects.
  - Book info shows it under the provider's name, apart from the book's own metadata, which stays read-only (Q4).
  - Results are cached per book and provider, with a fetched-at time and a Refresh action.
- **Documentation:** `docs/extensions/README.md` and `linen.d.ts`.
- **Checks:**
  - In-app: a lookup provider and a translation provider each render in the peek; a provider that fails or times out shows the peek's error line; metadata appears in Book info and survives a restart.
  - Visual captures for approval: the peek with a provider, and Book info with provider metadata, in the light and night themes.
- **Edge cases the checks must cover:**
  - **A stale answer.** The reader selects another word, or closes the peek, before a slow provider answers. The late answer must be dropped and never shown for the new selection; a request carries an ID and only the latest one renders.
  - **Hostile or broken provider output.** Markup in a field shows as text, never HTML. A `javascript:` or `file:` link is refused (only `https:`). A huge answer is cut to a set size, and a missing required field gives the error line.
  - **The provider stops.** It is suspended by the heartbeat mid-lookup, disabled or uninstalled while its peek is open, or offline. Each gives the peek's error line or closes the peek, and nothing is left waiting.
  - **More than one provider of a kind:** one is used by default and the others are reachable, in an order that stays the same across restarts. With none of a kind, its item is absent, not greyed out.
  - **Languages:**
    - A book with no `dc:language`.
    - A selection in another language than the book (`xml:lang`).
    - The target language the same as the source.
    - An RTL answer shown in an LTR book (direction set per field).
  - **The selection:** one that crosses a footnote marker, a line break or an element boundary; one over 80 characters (cut, as for Look Up).
  - **Metadata:**
    - Fetched only when Book info opens, never in bulk at import, so a provider learns about a book only when the reader asks.
    - A re-imported book with a new `content_hash` keeps or refetches its cache by a stated rule.
    - Uninstalling a provider deletes its cached results.
    - A book with no identifier still gets a request, by title and author.
  - **Host API versions:**
    - An extension with `engines.linen: "^1.1"` on a 1.0 host (an older install) is turned off with its reason (P5).
    - An extension declaring `^1.0` that calls a 1.1 method is refused by the per-call manifest check.
  - **Shortcuts.** If Translate gets a default chord, a reader's existing remap (C4, `remap.ts`) may already use it. At startup a new default that collides with an override yields to the reader's choice, and Settings › Shortcuts says so. A unit test covers this; it applies to every later phase that adds commands.
  - **Accessibility:** VoiceOver reads the provider's answer and its attribution, and the peek is reachable and dismissable from the keyboard.

**Done when:**
- The new checks, including the edge cases above, pass with the full in-app suite, and the unit tests pass.
- Both example extensions install, ask for consent and work.
- The captures are approved, and the Host API version and documentation are updated.

### Phase 12 — iPad (L+)

**Before starting:**
- Phase 9's Apple membership.
- The owner's answers on the distribution channel (O3), extensions on iPad (O4) and a test iPad (O5).
- Xcode with the iOS SDK on the development Mac.

**12a — Spike (S–M).** `pnpm tauri ios init`, then builds for the simulator and a device. Record a pass or fail for each of these:
- The book and extension URI schemes (WKURLSchemeHandler), IPC, and the search and extension workers.
- `rusqlite` and `ureq` on iOS.
- Import through the Files picker, with security-scoped access. The importer already copies books into the library.
- Lifecycle: progress and annotations are saved when the app goes to the background, and nothing is lost if iOS ends it there. This replaces the quit path of item 31 on iOS.
  - Today progress is saved on a page turn (with a 1 s debounce), on `blur` and at quit (`Reader.svelte`). iOS may not send `blur` when the app goes to the background, and it freezes the debounce timer.
  - Add a flush on `visibilitychange` and `pagehide`, and on the native background event. Test it by turning a page and backgrounding within that 1 s, then ending the app with `simctl terminate`.
- Spike A page counts against the macOS baseline (expected to match: same WebKit family), and Spike E's hostile corpus.
  - Compare at the same pixel size; the tablet default is 20 px, the desktop 19 px.
  - Note the iOS and macOS WebKit versions, because they don't move in step.
- The memory budget. iOS ends apps at lower limits than macOS, so try the large books in the corpus.
- The verdict goes in `docs/spikes/ipad.md`, with a go or no-go.

**12b — Platform layer (M).** `cfg(target_os = "ios")` beside the existing macOS and fallback branches.
- **Look Up:** `UIReferenceLibraryViewController` in place of Dictionary Services.
- **Screen reader state (T6):** `UIAccessibility.isVoiceOverRunning` and its change notification, in place of the `NSWorkspace` query in `native.rs`. Single-key shortcuts and motion follow it, as on the Mac.
- **Selection (T4):** the design puts Linen's actions in the system edit menu on iPad (P§8, touch row), which needs native code on the WKWebView. If that proves impractical, a PROVISIONAL fallback for sign-off: Linen's selection bar below the selection, clear of the handles, as on desktop touch.
- **Hardware keyboard:** shortcuts come from the command registry. Holding ⌘ shows the system shortcut overlay instead of a menu bar (P§16).
- **Not on iOS:**
  - The updater is compiled out, because TestFlight and the App Store update the app.
  - The menu bar and window chrome are gone; the app is always full screen (P§16).
- **Library folder:** stays in the app container and is visible in the Files app, so the README's backup, restore and export story still holds. The crash log moves into the container, and About reveals it through the share sheet.
- **Opening books:** from the Files app and the share sheet, through document types.

**12c — Tablet layout (M–L).** Built to the design's tablet values:
- Reading size 20 px; side margins at least 32 px (S3); touch targets 44 × 44 pt (P§15).
- Preferences as a sheet (P§3); notes as a bottom sheet (Screen 15).
- Safe areas.
- One column in portrait; a spread in landscape only when two full measures fit.
- Rotation, Split View and Stage Manager resizes keep the reading position (L rules).
- The touch model I12–I17 is built for desktop touch already; this is where it gets real-finger checks.
- **Design gap:** apart from the Screen 13 overlay and the P§16 table there are no iPad screens. Build to a written recommendation marked PROVISIONAL and submit the captures for approval, as in Phases 5–7.

**12d — Verification:**
- **Simulator:** the in-app suite runs there. Set the harness variables with `SIMCTL_CHILD_*` on `xcrun simctl launch`, and read the report from the app container.
- **Visual baselines** for the iPad, captured with `simctl io screenshot`.
- **On the test iPad:** cold start and memory budgets, and a VoiceOver run (X rules).
- **TestFlight builds** from CI on a macOS runner, with the App Store Connect API key from Phase 9.
- **Simulator or device:** visual baselines come from one or the other, never both. The simulator renders on the Mac's GPU, so its pixels differ from a device's.

**Edge cases the checks must cover:**
- **Background and resume:**
  - **The extension heartbeat** is a `window.setInterval` in the main window (`host.svelte.ts`), and iOS freezes it in the background. On resume, an extension must not be stopped as hung for a gap the app caused itself: the heartbeat restarts on `visibilitychange`.
  - **Reading sessions (item 37):** time spent in the background is not reading. A session that spans a background period ends at the last movement before it, as the 5-minute rule already says.
  - **An import or a write when iOS ends the app:** the B3 and E5 promise (nothing changes until the store points at the new files) is checked on iOS, and a library never opens half-imported.
  - **A memory warning while reading a large book:** the app keeps the position and recovers when reopened.
- **Getting books in:**
  - A Files file still in iCloud and not downloaded yet: the import waits, or says so.
  - A file from a third-party file provider whose access ends during the import.
  - Cancelling the picker.
  - A full device: the import fails with E-rule wording, not a crash.
  - The same book shared twice.
- **Sizes:**
  - **Split View at its narrowest (about 320 pt).** That is below the tablet margins, so it takes the phone minimum of 20 px (S3) and one column.
  - **Slide Over, and Stage Manager windows resized continuously:** the position holds throughout.
  - **Rotation in the middle of a page-turn animation.**
- **Input:**
  - **A hardware keyboard attached or removed mid-session**, the mixed-input reader of P§2.
  - **A trackpad or mouse on iPad:** pointer events follow the per-event rule for touch laptops (P§16), not the touch zones.
  - **Apple Pencil Scribble and hover:** they must not select text or turn pages unexpectedly.
  - **VoiceOver on:** the touch zones must not swallow VoiceOver's gestures.
- **System settings:**
  - **Dynamic Type:** the OS text size is the base (P§15).
  - **Low Power Mode,** which lowers the animation frame rate: page turns stay correct, and budgets are measured with it off and noted with it on.
  - **ProMotion (120 Hz).**
  - **A switch between light and dark appearance while reading.**
- **Not applicable on iOS.** “Not applicable” isn't a free pass. Each such check names its iOS replacement where the behaviour exists in another form: quit becomes background save; the menu bar becomes the ⌘ overlay; the updater becomes item 32's Download line never showing.

**Done when:**
- The spike verdict is go.
- The in-app suite passes on the simulator, including the edge cases above. Checks not applicable on iOS (menu bar, updater, trackpad) each have a reason and, where one exists, the iOS check that replaces them.
- The iPad captures are approved, and the budgets and a VoiceOver run are recorded on the test iPad.
- A TestFlight build installs and reads the corpus.
- Backup through the Files app restores a library.

**Risks:**
- **App Review and extensions.** Extensions are JavaScript the reader installs; App Store guideline 2.5.2 restricts downloaded code that changes an app's features. Mitigation: TestFlight first, and O4 decides before any App Store submission.
- **Tauri's iOS support is younger than its desktop support.** 12a exists to find out early.

### Phase 13 — Sync providers (1.2, L)

**Before starting:** the owner chooses the approach (O6). Linen has no server and no accounts (plan §1.3; item 42's decision), and the design defines sync as a storage adapter for progress and annotations (P§18).

- **Recommended shape:**
  1. A Host API `sync.registerAdapter` with a small interface: list, get and put opaque, versioned change records. It is for backends like WebDAV, through extensions. A new permission, `sync.storage`, is consented at install and strongly warned, because the adapter sees every annotation.
  2. One built-in adapter: a sync folder the reader chooses, normally iCloud Drive, used through the app's iCloud container on iPad. Mac and iPad then sync with no account and no server, and the data stays in the reader's own storage.
- **What syncs:** reading positions, highlights and notes. Settings don't sync, and neither do the books themselves. A record for a book this device doesn't have waits until the book is imported, matched by `package_identifier` and `content_hash`; `anchored_content_hash` covers a different edition of the file.
- **Data model:** UUIDs and timestamps were in the schema from the start “so sync can come later” (plan §4). This phase adds:
  - Tombstones for deletions.
  - A device ID.
  - A hybrid logical clock, so a wrong system clock can't reorder changes.
- **Merge rules (PROVISIONAL, for approval):**
  - **Position:** the newest one wins. When another device is well ahead, a quiet line offers “Continue from page N (iPad)”.
  - **Highlights and notes:** the newest change to each record wins. If two devices edit the same note at once, both texts are kept, the second as a copy. Nothing is dropped, in keeping with A8.
- **Checks:**
  - Property-based unit tests of the merge.
  - The data-safety tests of §6.3 with two devices.
  - In-app checks that run two data folders against one sync folder, including offline edits and a deletion racing an edit.
- **Edge cases the checks must cover:**
  - **A cloned device ID.** The README's restore story (“put the folder back, on this Mac or another”) copies the database, device ID and clock included, to a second device. Two devices with one ID overwrite each other's changes in silence. Treat the device ID as belonging to the machine, not the library: it is created on first launch and stored outside the library folder. A restored library gets a new one, and a check covers it.
  - **A clock far in the future.** A device whose clock reads 2030 would win every merge for years. The hybrid clock refuses remote times more than a set margin ahead of the local clock, and keeps those records aside with a quiet line, never applying them silently.
  - **The first sync of two libraries that both have content.** Not an empty one joining a full one. The same book was imported on both with different book UUIDs, so they are joined by `package_identifier` and `content_hash` and their annotations merged, with no duplicates.
  - **Different schema versions.** The Mac updates before the iPad, so a device meets records from a newer schema. It keeps them as they are and passes them on unchanged. It never drops them, and never rewrites them in its older form.
  - **Ping-pong.** The same book is open on both devices. Applying a remote position must not count as a local change, or the two devices bounce the position between them forever.
  - **Deleting and undoing:**
    - Undo after a delete is a newer change that beats the tombstone, so the highlight comes back on every device.
    - Removing a book from one library doesn't delete its annotations on the other devices; the rule is stated and tested.
  - **Reading backwards on purpose:** the newest position wins even when it is earlier in the book, and the “Continue from page N” offer covers the other case.
  - **The sync folder itself:**
    - **iCloud Drive files not downloaded yet:** a sync waits and doesn't treat them as missing.
    - **iCloud's own conflict copies** (“records 2.json”): read and merged, then removed.
    - **A file seen half-written:** writes go to a temporary name and are then renamed, and readers ignore temporary names.
    - **The folder moved, deleted or out of reach** (a stale security-scoped bookmark), or the iCloud quota full: a quiet line says so, and nothing local is lost.
    - **Switching the folder, and turning sync off and on again:** no duplicates and no lost records.
  - **An adapter that misbehaves.** Records failing validation, oversized or corrupt, are refused. A record missing on the remote side never deletes the local copy; only a tombstone does.
  - **A different edition on each device:** highlights that can't be placed go to “Couldn't place” (A8), never dropped.
- **Soak test.** A scripted simulation of three devices runs thousands of random operations, with offline periods, clock skew, reordered and repeated deliveries, and restarts. After each run every device must hold the same data, and no note text may be lost. It runs in CI with a fixed seed, and with fresh seeds weekly.

**Done when:**
- The checks, the edge cases and the soak test pass.
- A Mac and the test iPad stay in step over a week of real reading, recorded in the status document. This confirms the soak test in real use and doesn't replace it.
- The adapter API is documented with a WebDAV example extension.

### Phase 14 — Text-to-speech (1.2, M–L)

- **Spike first:** `speechSynthesis` in WKWebView against a native `AVSpeechSynthesizer` bridge on macOS and iOS. The question is which gives reliable word or sentence boundary events and enough voices; that decides the built-in provider.
- **Following along:**
  - Sentences come from `Intl.Segmenter` and are anchored by CFI, as highlights are.
  - The spoken sentence gets a follow-along mark, and pages turn by themselves.
  - Selection and Look Up pause speech.
- **Playback bar:** the design names it (P§18), but no screen shows it. Build it PROVISIONAL and submit it for approval. It needs play and pause, previous and next sentence, speed, voice, and a sleep timer. On iPad it also needs lock-screen controls (Now Playing) and background audio.
- **Host API:** `speech.registerProvider` for cloud voices, using the network permission and its consent.
- **Screen readers:** while VoiceOver is running, TTS defers to it (T6). Screen readers remain the accessibility route (P§20).
- **Edge cases the checks must cover:**
  - **Text that splits badly into sentences:**
    - Abbreviations (“Mr.”, “e.g.”), ellipses, and quotation marks around full stops.
    - CJK and Thai, which have no spaces.
    - Sentences that cross a page or chapter boundary, or inline elements (italics, links).
  - **What is spoken or skipped:**
    - Footnote markers and note references.
    - Images: their alt text, or nothing when there is none.
    - Tables, poetry line breaks, ruby (the base text only), MathML, and hidden content (`aria-hidden`, `display: none`).
  - **Voices:**
    - The voice follows `xml:lang` changes mid-text.
    - A language with no installed voice says so and doesn't read with the wrong voice.
    - Voices that send no word boundary events fall back to sentence-level marks.
    - A speed change mid-sentence.
  - **The reader acting during playback:**
    - A manual page turn, a Navigator jump or a search result: playback follows the reader from the newly shown page, and the page never jumps back.
    - Selecting text, or opening Look Up.
    - VoiceOver turning on mid-playback.
  - **Audio interruptions:** a call, another app's audio, or headphones unplugged all pause playback. Bluetooth latency must not throw the follow-along mark out of step.
  - **The sleep timer** across a background period on iPad.
  - **Reading sessions (item 37):** whether listening counts as reading is stated in the API documentation and tested.
  - **Reduced motion:** automatic page turns stay instant, with no follow-along animation.

**Done when:**
- A chapter is read aloud on the Mac and the iPad with the follow-along mark in step (within one sentence), and position and pages follow. The edge cases above pass in the in-app suite or, where audio hardware is needed, in a recorded manual run.
- The playback bar is approved, and the in-app checks and an example provider extension pass.

## 4. Decisions for the owner

| # | Decision | Recommendation |
|---|---|---|
| O1 | Join the Apple Developer Program (99 USD a year). An individual membership is the quickest; an organisation needs a D-U-N-S number. | Join as an individual now. It unblocks Phases 9 and 12. |
| O2 | Host API 1.1: `lookup.registerProvider` and `metadata.registerProvider`, rendered by Linen from structured results. | Approve as described in Phase 11. |
| O3 | iPad distribution channel. | TestFlight beta first; the App Store after O4 is settled. |
| O4 | Extensions on iPad, given App Review guideline 2.5.2. | Ship the iPad with extensions, through TestFlight. Before the App Store, decide whether they stay or the iPad keeps built-in extensions only. |
| O5 | A test iPad for budgets and VoiceOver. | Any iPad that runs the iOS version Tauri supports, recorded in `reference-machines.md`. |
| O6 | Sync approach. | An adapter API plus a built-in sync-folder adapter (iCloud Drive), as in Phase 13. |
| O7 | Gate W, after Phase 10. | Decide on Phase 10's verdicts and the demand signals; no-go by default if nobody is asking. |
| O8 | Order of iPad and sync. | iPad first, as here. Sync is designed with a second device in hand. |

## 5. Not in this plan

- Android and phone layouts (“Not planned”, plan §1.3).
- Windows and Linux port work beyond Phase 10, until Gate W says go.
- TypeSafe judgments inside Linen (item 42 (b), (c)): there is still no safe way to ship an API key without a server.
- Accounts, AI features, a store, DRM (plan §1.3).
