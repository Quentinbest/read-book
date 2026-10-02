"""C: E4 title fallback. dc:title is hidden and used as truth. Baseline = first heading of the
first spine document, else the file name. Jev = a Choice over code-gathered candidates."""
import json, re, unicodedata
from concurrent.futures import ThreadPoolExecutor
import ts


def fold(s):
    s = ''.join(c for c in unicodedata.normalize('NFKD', s) if not unicodedata.combining(c)).lower()
    return re.sub(r'[\W_]+', ' ', s).strip()


def same(pred, title):
    p, t = fold(pred), fold(title)
    return bool(p) and (p == t or p == fold(re.split(r'[:;]', title)[0]))


def filename(b):
    f = b['file']
    if f.startswith('se__'):
        return f[4:]  # Standard Ebooks serves author_title.epub
    m = re.match(r'pg_(\d+)', f)
    return f'pg{m.group(1)}.epub' if m else f


def gather(b):
    cands, seen = [], set()
    lin = [s for s in b['spine'] if s['linear'] != 'no'][:4]
    for k, s in enumerate(lin):
        for h in s['headings']:
            if len(h) <= 120 and fold(h) not in seen:
                seen.add(fold(h)); cands.append((h, f'a heading in section {k + 1} of the book'))
    fn = filename(b)
    return cands


def run(b):
    cands = gather(b)
    if not cands:
        return None, 1.0, []
    crit = {f'candidate {i + 1}': f'“{t}”, {where}' for i, (t, where) in enumerate(cands)}
    q = {'title': {'type': 'choice', 'instructions': 'An e-book has no title in its metadata. Which of these strings, found in the book, '
                   'is most likely the title of the book itself (not a chapter, a section heading, the publisher, or the author)?', 'criteria': crit}}
    a = ts.ask({'file_name': filename(b)}, q)['answers']['title']
    ch = a['choice']
    pick = None if ch == 'none' else cands[int(ch.split()[1]) - 1][0]
    return pick, a['confidence'], cands


def main():
    books = [b for b in json.load(open('books.json')) if b['title']]
    with ThreadPoolExecutor(8) as ex:
        res = list(ex.map(run, books))
    rows = []
    for b, (pick, conf, cands) in zip(books, res):
        first = b['spine'][0]['heading'] if b['spine'] else None
        base = first or filename(b)
        rows.append({'file': b['file'], 'title': b['title'], 'base': base, 'jev': pick, 'conf': conf,
                     'covered': any(same(c, b['title']) for c, _ in cands),
                     'base_ok': same(base, b['title']), 'jev_ok': same(pick or filename(b), b['title'])})
    json.dump(rows, open('out_title3.json', 'w'), indent=1, ensure_ascii=False)
    n = len(rows)
    print(f"n={n}  answer among candidates={sum(r['covered'] for r in rows)}  "
          f"first-heading={sum(r['base_ok'] for r in rows)}  jev={sum(r['jev_ok'] for r in rows)}")
    for r in rows:
        if not (r['base_ok'] and r['jev_ok']):
            print(f"{r['file'][:34]:34} title={r['title'][:28]!r:30} base={'✓' if r['base_ok'] else '✗'}{r['base'][:22]!r:24} "
                  f"jev={'✓' if r['jev_ok'] else '✗'}{str(r['jev'])[:22]!r} c={r['conf']:.2f} cov={r['covered']}")
    print(ts.report())


main()
