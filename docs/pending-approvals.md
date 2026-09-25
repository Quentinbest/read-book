# Waiting for the owner

Items set aside while work continued (owner's instruction, 2026-09-25). Each has a recommendation; approving it means “as recommended”. Newest phase last.

Items 1–8 and 11–20 were approved as recommended on 2026-09-25; the record is in `docs/decisions.md`. New items are added below as work continues.

## Phase 7

The plan wanted D4 decided and the G1 and G7 designs before Phase 7. None existed; each is built as recommended below and marked provisional.

21. **D4 — extension packages.** *Recommendation:*
    - **Package:** a zip file named `*.linenext`, with `manifest.json` at its root (P1), plus the extension's scripts, UI pages and icons. A theme pack is the same, with only a manifest and token JSON.
    - **Signing:** none in the MVP. Install is from a file only, and the consent dialog says “Not verified by Linen”. A signed registry follows the MVP (P§19).
    - **API reference:** Markdown in `docs/extensions/`, with a TypeScript declaration file (`linen.d.ts`) that extension authors can use.
    - **Localisation:** English only in the MVP. A manifest `locales` field is reserved and ignored.

22. **G1 — install consent.** *Recommendation, built:* a modal sheet in the Settings window. The main install sheet shows:
    - the extension's name, version and description;
    - “Not verified by Linen”;
    - **Can access**: the permissions in Screen 11's plain words (“Read the text you select”, “Connect to api.dictionaryapi.dev”). Those marked in P3 (`book.text`, `annotations.write`, wildcards, `background`) are highlighted in accent, as Screen 11 draws `network:` and `background`;
    - **Adds**: what it contributes (“Command”, “‘Define’ in selection menu”, “Navigator tab”, “Theme”);
    - Cancel and Install.

    Two other sheets follow the same pattern:
    - **Updates** that ask for more show only the new permissions, with “Keep current version” and “Update”; until then the old version keeps running.
    - **Remove** asks “Also delete its saved data?”, with Keep data and Delete data (P5).

23. **G7 — extension surfaces.** *Recommendation, built:*
    - **Navigator tab:** an extension's tab follows Notes in the Navigator's tabs. More than one extension tab collapses into a “More” menu tab.
    - **Selection “⋯”:** opens a small menu of extension actions under the bar, as Screen 12 draws. A stuck action shows “Not responding” with “Restart <name>”; “Manage extensions…” is at the end.
    - **Pinned commands:** they appear at the top of the reader's ⋯ menu, under the extension's name. There is no top-bar slot (P8).
    - **Built-in extensions** (Markdown Export) are listed with “Built-in” and can be turned off, not removed.
    - **Suspended extensions** show Screen 11's warning box (“Stopped responding … Reading wasn't affected. Restart · Disable”).
    - **Frames:** extension UI frames run on the extension's own origin (Spike G: WebKit runs no script in an opaque-origin frame). P§19's “no same-origin access” holds for the app: the frame is never on the app's origin.

24. **Phase 7 visual baselines (Screen 11 with extensions installed; Screen 12's extension failure).** Review `docs/visual/phase7-review.html`.
    *Recommendation:* approve.

25. **Not met in Phase 7: the P6 memory budget.** WebKit gives a page no way to measure a Worker's memory; `measureUserAgentSpecificMemory` is Chromium-only. CPU is budgeted through the heartbeat; memory is not.
    *Recommendation:* accept for the MVP. Revisit if a native per-WebContent-process measure (like Phase 2's `proc_pid_rusage`) proves attributable to one extension.

## Phase 8

The plan wants D1 and D6 decided before Phase 8 (D3 and D5 are). Phase 8 goes ahead on everything that does not depend on them.

26. **D1 — telemetry and crash reporting.** *Recommendation:* none. Nothing leaves the Mac. Screen 12 promises that books “stay on this device”, and a reading app does not need usage data. Crashes are written to a local log (`~/Library/Logs/Linen`), and Settings › About offers “Show crash log” for attaching to an email by choice.

27. **D6 — auto-update and release cadence.** *Recommendation:*
    - Tauri's updater, checking once a day, with updates signed by a Linen update key and served from GitHub Releases.
    - It downloads in the background and installs on the next launch, after a quiet “Update ready · Restart” message. It never interrupts reading.
    - A minor release about every six weeks, and fixes as needed.

    **Needs from the owner:** the update signing key (kept out of the repo) and the release host. The updater is wired up, but it is off until both exist.

28. **Code signing and notarisation (D3: a signed, notarised DMG).** *Needs from the owner:*
    - a Developer ID Application certificate in the keychain;
    - an App Store Connect API key (or an Apple ID app password) for `notarytool`.

    `scripts/release-macos.sh` is ready to build, sign, notarise, staple and verify once they are there. Until then, releases are unsigned development builds.

## Carried over (need a person, not a decision)

9. **VoiceOver re-check of the reader (X3).** About 5 minutes: `scripts/run-spikes.sh x3`, turn VoiceOver on, Control + Option + A, follow the panel.

10. **Oldest supported macOS reference machine (macOS 13 with Safari 16.4+).** Needed before the §6.4 budgets are signed off for release (`docs/spikes/reference-machines.md`).
