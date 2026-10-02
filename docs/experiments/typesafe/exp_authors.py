"""B: author sort. Baseline = order.ts authorKey (last word is the surname).
Jev = a Choice over code-generated catalogue forms (select, don't generate).
Metric: the primary sort key (text before the first comma, folded) matches."""
import json, re, unicodedata
from concurrent.futures import ThreadPoolExecutor
import ts

SUFFIX = {'jr', 'jr.', 'sr', 'sr.', 'ii', 'iii', 'iv'}


def fold(s):
    s = ''.join(c for c in unicodedata.normalize('NFKD', s) if not unicodedata.combining(c)).lower()
    return re.sub(r'[^\w]+', ' ', s).strip()


def primary(form):
    return fold(form.split(',')[0])


def baseline(name):
    parts = fold(name).split(' ')
    return parts[-1]


def candidates(name):
    toks = name.replace(',', ' ').split()
    suf = ''
    if len(toks) > 2 and toks[-1].lower() in SUFFIX:
        suf = ', ' + toks[-1]
        toks = toks[:-1]
    out = [f"{' '.join(toks[k:])}, {' '.join(toks[:k])}{suf}" for k in range(1, len(toks))]
    return out + [name]


# Hand-labelled hard names: acceptable primary keys.
HARD = {
    'Ursula K. Le Guin': ['le guin'], 'Gabriel García Márquez': ['garcia marquez'],
    'Martin Luther King Jr.': ['king'], 'Murasaki Shikibu': ['murasaki shikibu', 'murasaki'],
    'Sun Tzu': ['sun tzu', 'sun'], 'Melville, Herman': ['melville'], 'Mao Zedong': ['mao', 'mao zedong'],
    'Haruki Murakami': ['murakami'], 'Simone de Beauvoir': ['beauvoir', 'de beauvoir'],
    'Daphne du Maurier': ['du maurier'], 'Plato': ['plato'], 'Lao Tzu': ['lao tzu', 'lao'],
    'Arthur Conan Doyle': ['doyle'], 'Oliver Wendell Holmes Jr.': ['holmes'],
    'Johann Wolfgang von Goethe': ['goethe'], 'Lu Xun': ['lu', 'lu xun'], 'Chinua Achebe': ['achebe'],
    'bell hooks': ['hooks', 'bell hooks'], 'Anonymous': ['anonymous'], 'Various': ['various'],
    'W. E. B. Du Bois': ['du bois'], 'Martin Van Buren': ['van buren'], 'J. R. R. Tolkien': ['tolkien'],
    'Federico García Lorca': ['garcia lorca'], 'José Ortega y Gasset': ['ortega y gasset'],
    'Mary Wollstonecraft Shelley': ['shelley'], 'Ngũgĩ wa Thiong\'o': ['ngugi wa thiong o', 'ngugi'],
    'Project Gutenberg': ['project gutenberg'], 'Tolstoy, Leo': ['tolstoy'], 'Kenzaburō Ōe': ['oe'],
}


def ask(name):
    cands = candidates(name)
    q = {'form': {'type': 'choice',
                  'instructions': {'question': 'How is `name` written in a library catalogue, family name first, '
                                               'so that the books sort by the family name?',
                                   'notes': 'A name that is already family name first, a single name, a name '
                                            'traditionally given family name first (as in Chinese), or an organisation '
                                            'stays as written.'},
                  'criteria': {c: None for c in cands}}}
    a = ts.ask({'name': name}, q)['answers']['form']
    return a['choice'], a['confidence']


def main():
    books = json.load(open('books.json'))
    seen = {}
    for b in books:
        for c in b['creators']:
            if c['name'] and c['file_as'] and c['name'] not in seen and not re.search(r'[぀-鿿]', c['name']):
                seen[c['name']] = [primary(c['file_as'])]
    data = [(n, ok, 'file-as') for n, ok in seen.items()] + [(n, ok, 'hard') for n, ok in HARD.items()]
    with ThreadPoolExecutor(8) as ex:
        res = list(ex.map(lambda d: ask(d[0]), data))
    rows = []
    for (name, ok, src), (form, conf) in zip(data, res):
        rows.append({'name': name, 'ok': ok, 'src': src, 'base': baseline(name), 'jev': form, 'conf': conf,
                     'base_ok': baseline(name) in ok, 'jev_ok': primary(form) in ok})
    json.dump(rows, open('out_authors.json', 'w'), indent=1, ensure_ascii=False)
    for src in ('file-as', 'hard'):
        rs = [r for r in rows if r['src'] == src]
        print(f"{src}: n={len(rs)} lastword={sum(r['base_ok'] for r in rs)} jev={sum(r['jev_ok'] for r in rs)}")
    for r in rows:
        if not (r['base_ok'] and r['jev_ok']):
            print(f"{r['src']:7} {r['name'][:30]:30} want={r['ok'][0]!r:20} last={'✓' if r['base_ok'] else '✗'} jev={'✓' if r['jev_ok'] else '✗'} {r['jev']!r} c={r['conf']:.2f}")
    print(ts.report())


main()
