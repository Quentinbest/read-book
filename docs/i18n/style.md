# Style guide

How Linen sounds and looks in each language. The owner's decisions are L-2, L-3 and L-11 (`docs/decisions.md`); the rest are recommendations for the translator, which the reviewer may change (L-6). The checks in `src/lib/strings/catalogues.test.ts` enforce the parts marked **checked**.

## Every language

- **Tone.** Linen's English is quiet, plain and short: “Your library is empty”, “Couldn’t save notes to disk”. No exclamation marks, no “please”, no “successfully”. Keep it so: say what happened and what the reader can do.
- **Values.** Keep every `${…}` in a message; move it wherever the sentence needs it. **Checked.**
- **Names.** Linen, EPUB, Finder, Safari, Time Machine and VoiceOver are not translated (see `glossary.md` for Finder in Simplified Chinese). Key symbols (⌘ ⇧ ⌥ ⌃ ⌫ ↵ and arrows) and the letters in shortcuts (“H or ⇧⌘H”) stay as they are.
- **Ellipsis.** A menu item or button that asks for more before acting ends with “…” (one character), as in English: “Open…”, “Go to…”.
- **Room.** Some strings sit in tight places; `scripts/i18n/limits.json` and the export's “room” column say roughly how many characters fit. Prefer a shorter word to an abbreviation.
- **Plurals.** Use `plural()` where English does. Chinese and Japanese need only `other`, with a counter word where natural (本, 冊, 件, 章, 個).
- **Screen-reader text.** Labels such as “Previous page” or “Highlight yellow, last used” are spoken by VoiceOver. Translate them as natural speech, not as abbreviations.

- **Line breaks in Chinese and Japanese.** The UI breaks lines between phrases, not between any two characters: at spaces, and after full-width punctuation (、。，：！？」 and the like), where Linen adds an invisible break (`src/lib/strings/breaks.ts`, `src/app/base.css`). Long sentences need their commas; a long unbroken run still wraps, but anywhere.

## Simplified Chinese (`zh-Hans`)

- Mainland usage. Address the reader as 你 (recommended; matches Linen's informal English).
- Full-width punctuation after Chinese text: ，。：；？！ **Checked** (no ASCII , . : ; ? ! right after a Chinese character).
- Quotation marks “…” and ‘…’; not 「」. **Checked.**
- A space between Chinese and Latin letters or digits, as macOS does: “在 Finder 中显示”, “共 3 本书” (recommended). No space before % or after ⌘.
- Book titles in messages already come with Linen's quotation marks; use 《》 only if the reviewer prefers it for titles, then consistently.

## Traditional Chinese (`zh-Hant`)

- Taiwan usage (L-2): 檔案, 資料夾, 設定, 視窗, 螢幕, 搜尋. Readers in Hong Kong and Macau get the same text.
- Address the reader as 你 (recommended).
- Full-width punctuation: ，。：；？！ **Checked.**
- Quotation marks 「…」, nested 『…』 (L-11). **Checked** (no “ ”).
- A space between Chinese and Latin letters or digits, as macOS does: “在 Finder 中顯示” (recommended).

## Japanese (`ja`)

- Polite です/ます form in sentences (L-11); buttons and menu items as short verbs or nouns, as macOS writes them: 開く…, コピー, 削除, 設定….
- Full-width punctuation: 、。：？！ **Checked** (no ASCII punctuation right after Japanese text).
- Quotation marks 「…」, nested 『…』 (L-11). **Checked.**
- Spacing between Japanese and Latin words (“Linen について”, “Finder に表示”): follow macOS's own Japanese menus, which the reviewer checks; the glossary's rows use one space, to be corrected with them if macOS differs. Use the same rule everywhere in Linen.
- Katakana as Apple writes it: ライブラリ, ブック, メモ, フルスクリーン (no long-vowel mark where Apple omits it).

## Spanish (`es`)

- One neutral Spanish for every region (L-3): prefer words understood everywhere (“archivo”, not regional words for computer or file); no *vosotros*.
- Address the reader as *tú* (L-3): “Abre un libro”, “Tu biblioteca está vacía”.
- Menu items and buttons in the infinitive, as macOS does: Abrir…, Copiar, Eliminar, Buscar en el libro.
- Sentence case: only the first word and names take a capital (“Ajustes de lectura”, not “Ajustes De Lectura”).
- Opening ¿ and ¡ always.
- Quotation marks “…”, as macOS's Spanish uses (recommended; « » only if the reviewer prefers it, then everywhere).
- Numbers: Linen formats them (decimal comma, “45 %”); don't write numbers into the text.
- Avoid gendered forms where a neutral one reads naturally (“Se eliminó el resaltado” rather than forms that assume the reader's gender).
