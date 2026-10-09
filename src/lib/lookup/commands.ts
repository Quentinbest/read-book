// Reading Lens LK1: “Look Up with …” in ⌘K. App defines one command per extension
// lookup (extensions come and go with the library); the reader being read answers
// them while it is active (S14: a warm book takes no commands).

export const lookupCommands: {
  run: ((key: string) => void) | null
  enabled: ((key: string) => boolean) | null
} = { run: null, enabled: null }
