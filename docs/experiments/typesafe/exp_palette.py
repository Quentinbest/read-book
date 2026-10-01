"""D: ⌘K. Baseline = a port of src/lib/commands/fuzzy.ts; Jev = one Choice over the commands."""
import json, re, unicodedata, sys
import ts

CMDS = {
    'chapter.next': 'Next chapter', 'chapter.previous': 'Previous chapter',
    'search.open': 'Search in book', 'navigator.contents': 'Go to chapter…',
    'goto.open': 'Go to location…', 'navigator.notes': 'Highlights and notes',
    'reader.caretBrowsing': 'Caret browsing', 'layout.pages': 'Pages Mode',
    'layout.scroll': 'Scroll Mode', 'text.larger': 'Larger text', 'text.smaller': 'Smaller text',
    'text.reset': 'Default text size', 'reader.settings': 'Reading settings…',
    'app.settings': 'Settings…', 'shortcuts.show': 'Keyboard shortcuts', 'history.back': 'Back',
    'library.show': 'Library', 'book.open': 'Open…', 'window.fullScreen': 'Full screen',
}
# What each command does, from the plan/strings — what a description field in the registry would hold.
DESC = {
    'chapter.next': 'Jump to the start of the next chapter', 'chapter.previous': 'Jump to the start of the previous chapter',
    'search.open': 'Find words or phrases in the open book', 'navigator.contents': 'Show the table of contents to pick a chapter',
    'goto.open': 'Jump to a percentage, chapter or printed page number', 'navigator.notes': "List the book's highlights and notes",
    'reader.caretBrowsing': 'Move a text cursor through the book with the keyboard',
    'layout.pages': 'Read in pages that turn', 'layout.scroll': 'Read as one continuous vertical scroll',
    'text.larger': 'Increase the font size', 'text.smaller': 'Decrease the font size', 'text.reset': 'Reset the font size to the default',
    'reader.settings': 'Theme (light, dark, sepia), font, line spacing, margins for reading',
    'app.settings': 'App preferences: library folder, extensions, updates',
    'shortcuts.show': 'Cheat sheet of keyboard shortcuts', 'history.back': 'Return to where you were before the last jump',
    'library.show': 'Go back to the bookshelf of all books', 'book.open': 'Open or import an EPUB file',
    'window.fullScreen': 'Make the window fill the screen',
}

CASES = [  # (query, expected id or None)
    # typed fragments: the baseline's job
    ('next', 'chapter.next'), ('prev ch', 'chapter.previous'), ('search', 'search.open'), ('larger', 'text.larger'),
    ('gtc', 'navigator.contents'), ('scroll', 'layout.scroll'), ('full', 'window.fullScreen'), ('lib', 'library.show'),
    ('settings', 'app.settings'), ('keyboard', 'shortcuts.show'), ('high', 'navigator.notes'), ('caret', 'reader.caretBrowsing'),
    # synonyms and natural phrasing
    ('bigger text', 'text.larger'), ('make the font bigger', 'text.larger'), ('zoom in', 'text.larger'),
    ('text too big', 'text.smaller'), ('shrink font', 'text.smaller'), ('normal font size', 'text.reset'),
    ('dark mode', 'reader.settings'), ('sepia', 'reader.settings'), ('change font', 'reader.settings'),
    ('line spacing', 'reader.settings'), ('margins', 'reader.settings'),
    ('table of contents', 'navigator.contents'), ('toc', 'navigator.contents'), ('chapters', 'navigator.contents'),
    ('find', 'search.open'), ('look for a word', 'search.open'), ('where does it mention whales', 'search.open'),
    ('my notes', 'navigator.notes'), ('annotations', 'navigator.notes'), ('bookmarks', 'navigator.notes'),
    ('jump to page 120', 'goto.open'), ('go to 50%', 'goto.open'), ('skip ahead', 'chapter.next'),
    ('go back', 'history.back'), ('undo jump', 'history.back'), ('bookshelf', 'library.show'), ('my books', 'library.show'),
    ('import epub', 'book.open'), ('add a book', 'book.open'), ('hotkeys', 'shortcuts.show'),
    ('continuous scrolling', 'layout.scroll'), ('page turning', 'layout.pages'), ('preferences', 'app.settings'),
    ('hide everything', 'window.fullScreen'),
    # nothing fits
    ('delete this book', None), ('print', None), ('read aloud', None), ('translate', None),
]


def fold(s):
    return ''.join(c for c in unicodedata.normalize('NFD', s) if not unicodedata.combining(c)).lower()


WS = re.compile(r'[\s\-–—:·(]')


def fuzzy(query, text):
    q = fold(query.strip()); t = fold(text)
    if not q:
        return 0
    at = t.find(q)
    if at >= 0:
        ws = at == 0 or bool(WS.match(t[at - 1]))
        return 1000 - at + (200 if ws else 0) - (len(t) - len(q)) * 0.1
    score = 0; frm = 0; prev = -2
    for ch in q:
        if ch == ' ':
            continue
        i = t.find(ch, frm)
        if i < 0:
            return None
        ws = i == 0 or bool(WS.match(t[i - 1]))
        score += 30 if ws else 0
        score += 15 if i == prev + 1 else 0
        score -= min(10, i - frm)
        prev = i; frm = i + 1
    return score


def baseline(q):
    scored = [(fuzzy(q, title), i, cid) for i, (cid, title) in enumerate(CMDS.items())]
    scored = [s for s in scored if s[0] is not None]
    scored.sort(key=lambda s: (-s[0], s[1]))
    return (scored[0][2], scored[0][0]) if scored else (None, None)


def question(with_desc):
    crit = {cid: (f'{CMDS[cid]}: {DESC[cid]}' if with_desc else CMDS[cid]) for cid in CMDS}
    crit['none'] = 'None of these commands does what was asked'
    return {'type': 'choice',
            'instructions': 'A reader typed `query` into the command palette of an e-book reading app. '
                            'Which command does the reader want to run?',
            'criteria': crit}


def main():
    rows = []
    for q, want in CASES:
        b, bscore = baseline(q)
        res = ts.ask({'query': q}, {'plain': question(False), 'desc': question(True)})['answers']
        rows.append({'q': q, 'want': want, 'base': b, 'base_score': bscore,
                     **{k: (None if res[k]['choice'] == 'none' else res[k]['choice']) for k in ('plain', 'desc')},
                     'conf': res['desc']['confidence']})
    json.dump(rows, open('out_palette.json', 'w'), indent=1)
    n = len(rows)
    acc = lambda key: sum(r[key] == r['want'] for r in rows)
    # Hybrid: keep fuzzy when it found a plain substring (score >= 500), else ask Jev.
    for r in rows:
        r['hybrid'] = r['base'] if (r['base_score'] or 0) >= 500 else r['desc']
    print(f'n={n}  fuzzy={acc("base")}  jev_titles={acc("plain")}  jev_titles+desc={acc("desc")}  hybrid={acc("hybrid")}')
    for r in rows:
        mark = lambda k: '✓' if r[k] == r['want'] else '✗'
        print(f"{r['q'][:30]:30} want={str(r['want']):22} fuzzy{mark('base')} {str(r['base']):22} jev{mark('desc')} {str(r['desc']):22} c={r['conf']:.2f} hyb{mark('hybrid')}")
    print(ts.report())


main()
