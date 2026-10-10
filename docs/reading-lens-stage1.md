# Reading Lens Stage 1: the stuck-point diary

Stage 1 of `docs/reading-lens-plan.md`. It needs no code: it measures how often readers of English technical books get stuck on something a lookup at the word could help with. Its result is Gate 0, set on 2026-10-10 (`docs/decisions.md`), which decides whether Stage 2d and the Explain work go ahead. Stages 2a and 2b do not depend on it.

## Who and how long

The owner and 3–5 other readers, each reading their own English technical EPUBs in Linen for a week. Gate 0 needs local stuck points from at least 3 readers.

## For the readers

1. Read as you normally do, in Linen. Use your usual tools when you get stuck: Look Up, a dictionary app, a translator, a search, a chat window.
2. Each time you get stuck, highlight the words in **rose**. Use rose only for this; other colours are yours.
3. Add a note to the highlight, for example: *“Eudic: wrong sense; asked ChatGPT; it worked”* or *“Couldn’t tell what ‘it’ refers to; gave up”*. Say:
   - what you did,
   - whether it settled it, or you gave up,
   - if you used a dictionary: whether its entry had the right sense, and whether that sense came first.
4. If you realise later that you misread something, highlight it in rose too and say so in the note.
5. At the end of the week: Settings › Library › Export all highlights and notes…, choose a folder, and send the folder to the owner. It holds one file per book with your highlights, their surrounding words and your notes. Nothing else leaves your Mac.

## For the owner: coding and the gate

The exports contain book text and readers' notes: keep them and the sheets outside this repository (it is public).

1. **A sheet per reader:**
   `python3 scripts/reading-lens/g0.py sheet READER EXPORT-FOLDER -o READER.csv`
   Each rose highlight becomes a row with its quote, the words around it and the note.
2. **Code every row before looking at any totals:**

   | Column | Value |
   |---|---|
   | `type` | One of Report 1's types: `L1` unknown general word, `L2` technical sense of a known word, `L3` term mapping (known in Chinese, not the English term), `L4` multiword expression or idiom, `S1` compressed noun phrase, `S2` clause, negation or reference, `K1` concept knowledge, `X` missing context, `G` global throughput and affect |
   | `noticed` | `y` if the reader noticed at the time, `n` if they found the misreading later |
   | `tool` | What they used (free text) |
   | `failed` | `y` if the note says the tool didn't settle it, or they gave up; else `n` |
   | `dict_right_sense`, `dict_first` | For dictionary look-ups: `y` or `n`; otherwise `na` |

   Local types, the ones a lookup at the word can serve, are L2, L3, L4, S1 and S2.
3. **The gate:**
   `python3 scripts/reading-lens/g0.py gate *.csv`
   It refuses incomplete coding, then reports the counts by type, the share of local stuck points and the share of those the current tool failed on, with the dictionary-ceiling counts. Gate 0 passes when **local stuck points are at least 40%** of all, **the tool failed on at least 30%** of them, and there are **at least 30** local stuck points from **at least 3** readers.
4. Record the result in `docs/decisions.md`, with the counts, before any Stage 2d work.

If Gate 0 fails, Stages 2d–4 stop and the existing workflow is written up instead (plan §5); the lookup peek and local dictionaries stay.

The script's own checks: `python3 -I scripts/reading-lens/test_g0.py`.
