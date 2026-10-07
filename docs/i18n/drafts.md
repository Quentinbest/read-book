# The machine drafts: notes for reviewers

On 2026-10-08 all four catalogues were machine-drafted, as L-6 allows: `src/lib/strings/zh-Hans.ts`, `zh-Hant.ts`, `ja.ts` and `es.ts`. They are drafts. Development builds offer them in Settings; release builds do not (checked: no draft text in a release bundle). Each waits for a native speaker's review in context (`README.md`, steps 4–5).

They pass every automatic check, the shipping rule included (`VITE_I18N_STRICT=1`). Every entry is translated except numbers, symbols, font names, key caps and the cognates `catalogues.test.ts` lists. Every message keeps its values, and Chinese and Japanese follow the punctuation rules. A first look at the Japanese library in the app was right, except for one line break (Stage 4, below).

What a reviewer should look at first: the drafter's choices that a native speaker may well make differently.

## Every language

- **The glossary's “proposed” rows** (`glossary.md`) are used throughout: Library, Navigator, Highlight, Pages / Scroll, Paper / Sepia / Night. Changing a term means changing it everywhere in the file.
- **The macOS rows** were written from memory of macOS, not read off a Mac in that language. Compare the menu bar with TextEdit's.
- **Tight places** (`scripts/i18n/limits.json`): the Aa popover's choices, the Navigator and Go to tabs. Stage 4 measures them; until then, prefer the shorter word.

## Simplified Chinese

- Addresses the reader as 你, not 您.
- Copy is 拷贝 and Paste 粘贴, as macOS has them; Finder is 访达.
- Highlight is 高亮 (noun and verb: “用黄色高亮”). Navigator is 导航栏.
- “Sepia” is 棕褐. The reviewer may prefer a name readers know from other reading apps.

## Traditional Chinese (Taiwan)

- Highlight is 螢光標示, which is long. It appears in buttons and counts (“3 個螢光標示”); a shorter word may read better.
- Extensions are 延伸功能 and shortcuts 快速鍵, as macOS has them in Taiwan; Finder stays Finder.
- “App” is left in Latin letters in the cheat sheet, as Apple writes it in Taiwan.

## Japanese

- Sentences use です/ます (L-11); buttons and menu items are short (開く…, コピー).
- A book is ブック (Apple Books' word), not 本; counts use 冊 (“3冊”).
- Digits sit right next to Japanese (“3冊”, “残り5分”). Latin words get one space (“Finder に表示”, “Linen について”), as the glossary has it; check this against macOS.
- “Sepia / Night / Paper” are katakana (セピア / ナイト / ペーパー).

## Spanish

- One neutral Spanish with *tú* (L-3), following macOS's `es` where the regions differ: “pulsa”, “Gestionar extensiones”, “Funciones rápidas de teclado”, “Ajustes”. A reviewer from Latin America may prefer “presiona”, “Administrar” or “Atajos de teclado”; L-3 allows a split later if needed.
- “Mac” is used without a gender noun (“esta Mac”), as Apple's Spanish does.
- Scroll is “Desplazar” in the Aa popover and “Desplazamiento” in “modo Desplazamiento”.
- The page keys are “RePág / AvPág”, as Spanish keyboards label them.

## Found while drafting

- **Go to sentence.** The sheet added an English “. ” after the chapter's name. It is now `goto.sentenceEnd`, so Japanese can end with “にあります。” and Chinese with “。”. English is unchanged.
- **Japanese line breaks (Stage 4).** In the empty library, “開いてください。” broke between く and だ. Japanese text needs phrase-aware line breaking (`word-break: auto-phrase` where WebKit supports it, otherwise `line-break: strict` and no forced breaks); this belongs to Stage 4's `:lang()` rules.
