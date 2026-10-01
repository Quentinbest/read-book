# TypeSafe experiments (2026-10-01)

Can a TypeSafe System One judgment (model `jev-1.13.0`) stand in for four pieces of
heuristic code? Each experiment compares today's code with one Jev request on public
books and hand-labelled cases. Nothing here is wired into Linen; see pending approval 33.

## Results

| | Today's code | Jev | Sample |
| --- | --- | --- | --- |
| **D. ⌘K natural phrasing** (`lib/commands/fuzzy.ts`) | 17/50 | 45/50 (hybrid 46/50) | 50 queries, hand-labelled |
| **B. Author sort, hard names** (`lib/library/order.ts` `authorKey`) | 12/30 | 28/30 | 30 names, hand-labelled |
| **B. Author sort, publisher `file-as`** | 117/132 | 127/132 | 132 creators with `file-as` |
| **A. Where a new book opens without a landmark** (`engine.ts` `#textStart`) | 0/123 | 104/123 exact, 113/123 acceptable | 123 books, landmark hidden |
| **C. Title fallback** (`epub.rs` `first_heading_text`, E4) | 117/178 | 139/178 | 178 books, `dc:title` hidden |

Latency: p50 about 1.2 s, p90 about 2.1 s per request. Each request is one book or one query.

### D. ⌘K: the strongest result
- **Today:** fuzzy matching handles typed fragments ("gtc", "lib", "prev ch"). It finds nothing for "bigger text", "dark mode", "table of contents", "my notes", "jump to page 120", "import epub" or "hotkeys".
- **Jev:** one Choice over the 19 palette commands, each with a one-line description, plus a `none` option. Descriptions raised the score from 42 to 45.
- **The `none` option works:** all four "no such command" queries ("delete this book", "print", "read aloud", "translate") correctly got `none`.
- **Where Jev fails:** abbreviations ("gtc"), and "high" (it chose `text.larger`).
- **Hybrid:** keep fuzzy when it finds a plain substring (score ≥ 500), otherwise ask Jev. That scored 46/50, keeping every fuzzy success.
- **Caveat:** the queries were written by the experimenter, so the score is optimistic. Validate on real palette queries before relying on it.

### B. Author sort
- **Today:** the last word is taken as the surname. That fails on "Le Guin", "García Márquez", "King Jr.", "Du Bois", "Van Buren", "Ortega y Gasset", "Sun Tzu" and "Mao Zedong". It also fails on names already written surname-first: "Melville, Herman" sorts under *H*.
- **Jev:** code generates every split into "Family, Given", keeping Jr./Sr. suffixes at the end, plus "as written". Jev selects one, so it never writes a name.
- **Remaining misses:**
  - Murasaki Shikibu: confidence 0.06.
  - Ngũgĩ wa Thiong'o: confidence 0.52.
  - "Bagieu Pénélope": already family name first, with no comma.
  - Particle conventions where the sources disagree: Standard Ebooks files "de la Mare" under *Mare* and "van Dyke" under *Dyke*, while the Library of Congress keeps the particle.
- **A deterministic gap comes first.** `epub.rs` never reads `file-as`, whether as the `opf:file-as` attribute or a `<meta refines property="file-as">`. Every creator in the publisher-`file-as` sample carries one, which is how that sample was chosen. That needs no model at all.

### A. Where a new book opens
- **Today:** with no `bodymatter` landmark or EPUB 2 guide entry, `#textStart` opens the first linear section. That section was never the start of the text in 123 books; it is always the cover or title page.
- **Jev:** a Choice over the first 12 linear sections, each given as heading plus 220 characters of opening text. It got 104 exact.
  - 9 land on chapter 1, just after the "Part I" divider the landmark points to.
  - 2 land early; 8 land late.
  - Half the late ones are prefaces that Standard Ebooks counts as main text. That's a policy choice for the prompt, not a model error.
- **Nouls did worse:** asking per section "is this front matter?" and taking the first "no" scored 76/123.
- **Limited reach:** foliate already uses landmarks and the EPUB 2 guide, so this fallback only runs for books with neither. In the sample, that is all 21 Gutenberg books and 27 of the 45 IDPF test books. Gutenberg puts the licence header, front matter and chapter 1 in one file, so section-level candidates cannot express the right answer there. A follow-up must offer **headings inside documents** as candidates before this is worth building.

### C. Title fallback: only with the right candidates
Three variants ran on the same books:

| Candidates | Jev | First-heading rule |
| --- | --- | --- |
| Headings + file name + a `none` option | 70/112 | 76/112 |
| Headings + file name | 61/112 | 76/112 |
| Headings only; the file name stays the fallback in code | 139/178 | 117/178 |

- The first two variants (pilot set): Jev overused `none`, then the file name.
- **Headings only, on the full set:** whenever the title was among the candidates (141 books), Jev picked it 139 times. The one book where today's rule beat it was a collection called "Poetry", where Jev chose its first work.
- **Caveat:** the prompt was tuned on these books.
- **Small payoff:** real books without `dc:title` are rare.

## What was sent
- **D:** the typed query.
- **B:** the author name.
- **C:** headings from the first sections.
- **A:** a heading and 220 characters of opening text for each of the first 12 sections, which is book content.

The books were Standard Ebooks (`books-standard-ebooks.txt`), Project Gutenberg (`https://www.gutenberg.org/ebooks/<id>.epub.noimages` for IDs 1342, 84, 11, 1661, 2600, 98, 74, 345, 1260, 158, 5200, 4300, 2554, 1952, 16328, 3207, 1080, 76, 46 and 35) and `corpus/cache`. The downloaded books are not committed.

## Reproduce
```sh
# books/ holds the EPUBs; TYPESAFE_API_KEY must be set
python3 extract.py books/*.epub ../../../corpus/cache/*.epub > books.json
echo '{}' > labels_textstart.json
python3 exp_palette.py; python3 exp_authors.py; python3 exp_textstart.py; python3 exp_title3.py
```
`exp_title.py` and `exp_title2.py` are the two rejected variants of C. `results/` holds the per-case outputs of the final runs.
