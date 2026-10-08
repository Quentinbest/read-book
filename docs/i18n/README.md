# Translating Linen

How a language goes from nothing to shipped. The plan is `docs/i18n-plan.md`; the owner's decisions are L-1 to L-11 in `docs/decisions.md`.

## What gets translated

Everything in `src/lib/strings/en.ts`: about 480 entries and 1,500 words, Linen's whole UI (menus, buttons, messages, screen-reader text). Not translated: book content, user notes, text that extensions supply, the extension diagnostics (item 56), documentation, and the names Linen, EPUB, Finder, Safari, Time Machine and VoiceOver.

The languages and their tags: `zh-Hans` Simplified Chinese, `zh-Hant` Traditional Chinese in Taiwan usage (L-2), `ja` Japanese, `es` Spanish, neutral, addressing the reader as *tú* (L-3).

## The files

| File | What it is |
|---|---|
| `src/lib/strings/en.ts` | The English source. Comments name the rule or screen each string belongs to (“E3; Screen 12”); look them up in `docs/implementation-plan.md` or the design. |
| `src/lib/strings/<tag>.ts` | A language's catalogue: the same keys, translated. |
| `docs/i18n/glossary.md` | Terms that must be translated the same way everywhere. |
| `docs/i18n/style.md` | Tone, punctuation and typography, per language. |
| `scripts/i18n/limits.json` | Strings with little room on screen: how much, in Latin characters (`latin`) and in Chinese or Japanese ones (`cjk`). The catalogue check holds every language to it. |

## Steps

1. **Start the catalogue.** `node scripts/i18n/new-locale.mjs ja` copies `en.ts` to `src/lib/strings/ja.ts`, keys, comments and all. The copy is a draft: development builds (`pnpm tauri dev`) offer it in Settings › General › Language, release builds do not.
2. **Draft (L-6).** Replace each English text in place, following the glossary and the style guide. A plain string is translated as it is. A message is a small function: translate the text in its quotes and keep every `${…}` value; the order may change. `plural(n, { one: …, other: … })` picks a form by count: Chinese and Japanese have only `other`, so give `other` alone. Keep the shape of the file; the type check (`pnpm check`) fails on a missing key or a changed function.
3. **Check.** `pnpm vitest run src/lib/strings` runs `catalogues.test.ts` on every catalogue in the folder: each message must keep every value it is given; Chinese and Japanese follow the punctuation rules (full-width punctuation, their quotation marks; L-11); a draft is told how many entries are still English. `VITE_I18N_STRICT=1 pnpm vitest run src/lib/strings` holds drafts to the shipping rule (nothing in English except the entries `catalogues.test.ts` lists as the same in that language).
4. **Export for review.** `node scripts/i18n/export.mjs ja` writes `i18n-out/ja.csv` (not committed): key, values, context, English, the translation, the room it has (`limits.json`) and its status. Reviewers note changes in the last column.
5. **Review in context (L-6).** To see every screen in the language at once, `LINEN_LOCALE=ja scripts/e2e.sh v` (after the test build in `CLAUDE.md`) captures the 32 screens of the visual pass to `i18n-out/visual/ja/` and lists any text that does not fit its box.
    A native speaker reads the CSV and uses Linen in the language: in a development run (`pnpm tauri dev`), choose it in Settings › General › Language and restart; drafts are offered there. Every screen in `docs/design/` is worth a visit; long strings show their room there. Corrections go into `<tag>.ts`; the reviewer's sign-off is recorded in `docs/decisions.md`.
6. **Smoke check.** `scripts/i18n/check-languages.sh ja` (after the test build in `CLAUDE.md`, about 4 minutes a language) runs the visual pass in English and in the language, then fails if a screen was not captured, UI text overflows where English does not, UI text is left in English (it bypassed `t`), or the catalogue lacks a key. Run it before shipping and after layout changes.
7. **Ship.** Register the catalogue in `SHIPPED_LOADERS` in `src/lib/strings/index.ts` (each loads as its own chunk when a page uses it) and add the tag to `CFBundleLocalizations` in `src-tauri/Info.plist` (a test keeps the two equal). From then on nothing in it may be left in English. With the first language shipped, check by hand that System Settings › General › Language & Region › Applications offers Linen, and that an open panel follows the language chosen there (`docs/spikes/i18n-spike.md`).

## When English changes

A new or changed English string makes every catalogue fail the type check (new key) or leaves the old translation in place (changed text). Change the catalogues in the same commit, or mark the entry for translators with a `// TODO(i18n)` comment and export again.
