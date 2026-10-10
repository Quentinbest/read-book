"""Checks for g0.py: python3 -I scripts/reading-lens/test_g0.py"""

import csv
import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
G0 = HERE / 'g0.py'


def annotation(i, color, quote, note=None):
    return {
        'id': f'urn:uuid:{i}', 'type': 'Annotation', 'created': f'2026-10-1{i % 9}T10:00:00.000Z',
        'body': [{'type': 'TextualBody', 'value': note, 'format': 'text/plain', 'purpose': 'commenting'}] if note else [],
        'target': {'source': 'urn:x', 'selector': [
            {'type': 'FragmentSelector', 'value': 'epubcfi(/6/4!/4/2,/1:0,/1:5)'},
            {'type': 'TextQuoteSelector', 'exact': quote, 'prefix': 'before ', 'suffix': ' after'}]},
        'linen:color': color,
    }


def run(*args):
    return subprocess.run([sys.executable, '-I', str(G0), *args], capture_output=True, text=True)


def main():
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        export = {'label': 'Designing Data', 'first': {'items': [
            annotation(1, 'rose', 'invalidates', 'looked it up in Eudic; wrong sense'),
            annotation(2, 'yellow', 'not a stuck point'),
            annotation(3, 'rose', 'fencing tokens', 'searched the book; worked'),
        ]}}
        (tmp / 'book.json').write_text(json.dumps(export))
        r = run('sheet', 'ann', str(tmp), '-o', str(tmp / 'ann.csv'))
        assert r.returncode == 0, r.stderr
        rows = list(csv.DictReader(open(tmp / 'ann.csv')))
        assert [x['quote'] for x in rows] == ['invalidates', 'fencing tokens'], rows
        assert rows[0]['note'] == 'looked it up in Eudic; wrong sense'

        # Uncoded rows stop the gate.
        assert run('gate', str(tmp / 'ann.csv')).returncode == 2

        def coded(path, reader, codes):
            with open(path, 'w', newline='') as f:
                w = csv.DictWriter(f, fieldnames=['reader', 'book', 'id', 'created', 'quote', 'before', 'after', 'note',
                                                  'type', 'noticed', 'tool', 'failed', 'dict_right_sense', 'dict_first'])
                w.writeheader()
                for i, (t, failed) in enumerate(codes):
                    w.writerow({'reader': reader, 'book': 'b', 'id': str(i), 'created': '', 'quote': 'q', 'before': '',
                                'after': '', 'note': '', 'type': t, 'noticed': 'y', 'tool': 'Eudic', 'failed': failed,
                                'dict_right_sense': 'na', 'dict_first': 'na'})

        # Three readers, 12 local each (4 failed), 6 global each: 67% local, 33% failed: PASS.
        sheets = []
        for reader in ['a', 'b', 'c']:
            p = tmp / f'{reader}.csv'
            coded(p, reader, [('L2', 'y')] * 4 + [('S1', 'n')] * 8 + [('G', 'n')] * 6)
            sheets.append(str(p))
        r = run('gate', *sheets)
        assert r.returncode == 0 and 'Gate 0: PASS' in r.stdout, r.stdout
        # Two readers only: the floor fails it.
        r = run('gate', *sheets[:2])
        assert r.returncode == 1 and 'Gate 0: FAIL' in r.stdout, r.stdout
        # Mostly global stuck points: FAIL on the share.
        p = tmp / 'global.csv'
        coded(p, 'd', [('G', 'n')] * 60)
        r = run('gate', *sheets, str(p))
        assert r.returncode == 1 and 'needs ≥ 40%' in r.stdout, r.stdout
    print('g0.py: ok')


if __name__ == '__main__':
    main()
