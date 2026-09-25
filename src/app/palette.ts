// Extra ⌘K sources that live outside the command registry. The reader offers the
// open book's chapters (“Type a command, chapter or setting”, Screen 16).

export interface PaletteChapter {
  label: string
  run: () => void
}

let chapters: () => PaletteChapter[] = () => []

export function setPaletteChapters(source: (() => PaletteChapter[]) | null) {
  chapters = source ?? (() => [])
}

export const paletteChapters = () => chapters()
