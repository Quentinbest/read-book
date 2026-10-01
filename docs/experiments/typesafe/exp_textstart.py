"""A: N3, where a new book opens. Ground truth = the publisher's bodymatter landmark
(hidden from the model) or a hand label. Baseline = first linear section (engine.ts #textStart
fallback). Jev = a Choice over the first WINDOW linear sections, and per-section Nouls."""
import json, sys
from concurrent.futures import ThreadPoolExecutor
import ts

WINDOW = 12
books = json.load(open('books.json'))
labels = json.load(open('labels_textstart.json'))  # file -> spine index, for books without landmarks

INSTR = ('These are the first sections of an e-book, in reading order. A reader opening the book for '
         'the first time should land on the first page of the main text. Which section is that?')
FRONT = ('cover, title page, half-title page, copyright or imprint page, dedication, epigraph, table of '
         'contents, list of illustrations, preface, foreword, introduction by an editor, publisher notes, '
         'licence or transcriber notes')
MAIN = 'the first chapter, the first part, a prologue, or the opening of the main work (first poem, first story)'


def sections(b):
    lin = [i for i, s in enumerate(b['spine']) if s['linear'] != 'no']
    return lin[:WINDOW]


def describe(s):
    h = s['heading'] or '(no heading)'
    o = s['snippet'][:220] or '(no text; an image page)'
    return f'Heading: {h}. Opens with: {o}'


def run(b):
    idx = sections(b)
    state = {'sections': [{'position': k + 1, 'heading': b['spine'][i]['heading'] or '(no heading)',
                           'opening_text': b['spine'][i]['snippet'][:220] or '(no text; an image page)'}
                          for k, i in enumerate(idx)]}
    crit = {f'section {k + 1}': describe(b['spine'][i]) for k, i in enumerate(idx)}
    qs = {'start': {'type': 'choice',
                    'instructions': {'question': INSTR, 'front_matter_includes': FRONT, 'main_text_begins_with': MAIN},
                    'criteria': crit}}
    for k in range(len(idx)):
        qs[f'front_{k}'] = {'type': 'noul',
                            'instructions': {'question': f'Is `sections[{k}]` front matter rather than the main text of the book?',
                                             'front_matter_includes': FRONT, 'main_text_includes': MAIN}}
    a = ts.ask(state, qs)['answers']
    choice = idx[int(a['start']['choice'].split()[1]) - 1]
    fronts = [a[f'front_{k}']['noul'] for k in range(len(idx))]
    noul_pick = next((idx[k] for k, p in enumerate(fronts) if p < 0.5), idx[0])
    return {'choice': choice, 'conf': a['start']['confidence'], 'noul': noul_pick, 'fronts': fronts}


def main():
    cases = []
    for b in books:
        truth = labels.get(b['file'], b['body_index'])
        idx = sections(b)
        if truth is None or len(idx) < 2:
            continue
        if truth not in idx:
            print('truth outside window, skipped:', b['file'], truth)
            continue
        cases.append((b, truth))
    with ThreadPoolExecutor(6) as ex:
        results = list(ex.map(lambda c: run(c[0]), cases))
    rows = []
    for (b, truth), r in zip(cases, results):
        base = sections(b)[0]
        rows.append({'file': b['file'], 'truth': truth, 'base': base, **r})
    json.dump(rows, open('out_textstart.json', 'w'), indent=1)
    n = len(rows)
    acc = lambda k: sum(r[k] == r['truth'] for r in rows)
    # Gate: trust Jev only when confident, otherwise keep today's fallback.
    for t in (0.0, 0.5, 0.7, 0.9):
        g = sum((r['choice'] if r['conf'] >= t else r['base']) == r['truth'] for r in rows)
        print(f'gate conf>={t}: {g}/{n}  (jev used on {sum(r["conf"] >= t for r in rows)})')
    print(f'n={n}  first-linear={acc("base")}  jev_choice={acc("choice")}  jev_nouls={acc("noul")}')
    import re
    DIV = re.compile(r'^(part|book|volume|first part|section)\b', re.I)
    kinds = {'exact': 0, 'divider': 0, 'early': 0, 'late': 0}
    for r, (b, _) in zip(rows, cases):
        lin = sections(b)
        c, t = lin.index(r['choice']), lin.index(r['truth'])
        if c == t: kinds['exact'] += 1
        elif c == t + 1 and DIV.match(b['spine'][r['truth']]['heading'] or ''): kinds['divider'] += 1
        elif c < t: kinds['early'] += 1
        else: kinds['late'] += 1
        r['kind'] = [k for k in kinds][[c == t, c == t + 1 and bool(DIV.match(b['spine'][r['truth']]['heading'] or '')), c < t, True].index(True)]
    print('jev choice outcomes:', kinds)
    json.dump(rows, open('out_textstart.json', 'w'), indent=1)
    for r, (b, _) in zip(rows, cases):
        if r['choice'] != r['truth'] or r['noul'] != r['truth']:
            hd = lambda i: (b['spine'][i]['heading'] or '-')[:28]
            print(f"✗ {r['file'][:42]:42} truth={r['truth']}:{hd(r['truth'])!r} choice={r['choice']}:{hd(r['choice'])!r} c={r['conf']:.2f} noul={r['noul']}")
    print(ts.report())


main()
