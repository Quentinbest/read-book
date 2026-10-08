# i18n spike: where the UI language comes from (Stage 0, L-4)

**Verdict: use the core's `NSLocale.preferredLanguages`, not `navigator.languages`; declare the languages in `Info.plist` only when their translations ship.** One question is left for a person at the Mac: the System Settings listing and the look of the system panels (below).

- **Date:** 2026-10-07
- **Machine:** macOS 14.6.1 (see `reference-machines.md`)
- **Raw results:** `raw/i18n-lang.json` (seven runs)
- **Code:** `src/spikes/i18n.ts` (`LINEN_SPIKE=lang`), `preferred_languages` in `src-tauri/src/native.rs`, `spike_bundle_languages` in `src-tauri/src/spikes.rs`, `docs/i18n-plan.md` (the plan)

## Method

The spike page reports `navigator.language(s)`, the default `Intl` locale, the core's `NSLocale.preferredLanguages`, and the main bundle's `localizations` and `preferredLocalizations` (the language AppKit uses for its own text). A per-app language from System Settings is stored as `AppleLanguages` in the app's defaults domain. Because the test builds share `app.linen.reader` with the installed Linen, the spike wrote no defaults. Instead it passed `-AppleLanguages '(…)'` on the command line, which sets the same key in the argument domain (higher precedence, nothing persisted).

Two builds were run: the raw test binary (`--no-bundle`, as the e2e suite uses it) and a `Linen.app` bundle with a temporary `src-tauri/Info.plist` declaring `CFBundleDevelopmentRegion` `en` and `CFBundleLocalizations` `en zh-Hans zh-Hant ja es`. Tauri merged it into the bundle's `Info.plist` with no configuration. The plist and the bundle were removed afterwards.

## Results

| Run | `navigator.languages` | `NSLocale.preferredLanguages` | AppKit's choice for the bundle | Linen's choice (from NSLocale) |
|---|---|---|---|---|
| Raw binary, this Mac's settings | `en-US` | `en-US`, `zh-Hans-US` | `en` (no localisations, no bundle id) | en |
| Raw binary, `(ja-JP)` | `ja` | `ja-JP` | `en` | ja |
| Raw binary, `(zh-Hant-TW, en-US)` | `zh-TW` | `zh-Hant-TW`, `en-US` | `en` | zh-Hant |
| App with plist, this Mac's settings | `en-US` | `en-US`, `zh-Hans-US` | `en` | en |
| App with plist, `(ja-JP)` | `ja` | `ja-JP` | `ja` | ja |
| App with plist, `(fr-FR, ja-JP)` | **`fr-FR` only** | `fr-FR`, `ja-JP` | `ja` | ja |
| App with plist, `(es-419)` | `es-419` | `es-419` | `es` | es |

## Findings

1. **`navigator.languages` holds one language.** WKWebView reports only the first preferred language, so a reader who prefers French, then Japanese, would get English from it, while AppKit picks Japanese. `NSLocale.preferredLanguages` has the whole list and agrees with AppKit in every run. Linen asks the core (`preferred_languages`), and falls back to `navigator.languages` only if that call fails.
2. **The per-app language reaches the core.** `AppleLanguages` (here through the argument domain, which overrides the app's domain as a per-app setting would) changes both `NSLocale` and WebKit.
3. **Without `CFBundleLocalizations`, AppKit stays in English** (`preferredLocalizations` = `en`), whatever the preferred languages are. With them, AppKit follows (`ja`, `es`), and so will the text AppKit draws (open and save panels, standard menu items that AppKit itself names).
4. **Declare only languages whose translations ship (Stage 3).** If the plist listed Japanese before the Japanese catalogue existed, a Japanese reader would get Japanese system panels around an English UI. Stage 1 therefore adds no `Info.plist`.
5. **muda names the predefined menu items itself, in English.** `muda` 0.19.3 (`src/platform_impl/macos/mod.rs`, `src/items/predefined.rs`) builds “About/Hide/Quit Linen”, “Services”, “Hide Others”, “Show All”, “Cut”, “Copy”, “Paste”, “Select All” and “Minimize” from fixed English strings unless a `text` is given. Declaring localisations does not change them, so Stage 3 passes `text` from the catalogue for each one.
6. **Raw test binaries have no bundle identifier.** A per-app language set in System Settings cannot reach the e2e binary, which suits the harness: it stays in English (plan §6 pins it to `en`).

## Left for a person at the Mac (Stage 3)

With a bundle that declares the shipped languages: (a) System Settings › General › Language & Region › Applications offers Linen, and choosing Japanese there makes Linen start in Japanese; (b) an open panel (Settings › Extensions › Install from file…) appears in Japanese. Findings 2 and 3 predict both. They were not driven here, because the spike would have had to write the installed app's defaults and the harness cannot drive system panels.
