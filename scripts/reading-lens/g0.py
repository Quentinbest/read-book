#!/usr/bin/env python3
"""Reading Lens Stage 1: from diary exports to the Gate 0 verdict (docs/reading-lens-stage1.md).

    python3 scripts/reading-lens/g0.py sheet READER EXPORT... -o SHEET.csv
        Each rose highlight in the readers' exports (Settings › Library › Export all
        highlights and notes: one W3C JSON file per book) becomes a row to code.
    python3 scripts/reading-lens/g0.py gate SHEET.csv...
        Checks the coding and applies Gate 0 as decided on 2026-10-10 (docs/decisions.md).

The sheets hold book text and readers' notes: keep them out of the repository.
"""

import argparse
import csv
import json
import math
import sys
from pathlib import Path

# Report 1's types (A §2.10). Local: what a lookup at the word could help with.
TYPES = {
    'L1': 'unknown general word',
    'L2': 'technical sense of a known word',
    'L3': 'term mapping (known in Chinese, not the English term)',
    'L4': 'multiword expression or idiom',
    'S1': 'compressed noun phrase',
    'S2': 'clause, negation or reference',
    'K1': 'concept knowledge',
    'X': 'missing context',
    'G': 'global throughput and affect',
}
LOCAL = {'L2', 'L3', 'L4', 'S1', 'S2'}

# Gate 0, decided 2026-10-10 (item 59).
LOCAL_SHARE = 0.40
FAILED_SHARE = 0.30
MIN_LOCAL = 30
MIN_READERS = 3

CODED = ['type', 'noticed', 'tool', 'failed', 'dict_right_sense', 'dict_first']
COLUMNS = ['reader', 'book', 'id', 'created', 'quote', 'before', 'after', 'note'] + CODED


def rows_from_export(reader: str, path: Path):
    doc = json.loads(path.read_text(encoding='utf-8'))
    book = doc.get('label') or path.stem
    for a in (doc.get('first') or {}).get('items') or []:
        if a.get('linen:color') != 'rose':
            continue
        quote = next((s for s in a['target']['selector'] if s.get('type') == 'TextQuoteSelector'), {})
        note = (a.get('body') or [{}])[0].get('value', '') if a.get('body') else ''
        yield {
            'reader': reader,
            'book': book,
            'id': a.get('id', ''),
            'created': a.get('created', ''),
            'quote': quote.get('exact', ''),
            'before': quote.get('prefix', ''),
            'after': quote.get('suffix', ''),
            'note': note,
            **{c: '' for c in CODED},
        }


def sheet(args):
    rows = []
    for p in args.exports:
        path = Path(p)
        files = sorted(path.glob('*.json')) if path.is_dir() else [path]
        for f in files:
            rows.extend(rows_from_export(args.reader, f))
    rows.sort(key=lambda r: (r['book'], r['created']))
    with open(args.output, 'w', newline='', encoding='utf-8') as out:
        w = csv.DictWriter(out, fieldnames=COLUMNS)
        w.writeheader()
        w.writerows(rows)
    print(f'{len(rows)} stuck points from {args.reader} → {args.output}')


def problems_in(row, n):
    out = []
    t = row['type'].strip().upper()
    if t not in TYPES:
        out.append(f'row {n}: type “{row["type"]}” is not one of {", ".join(TYPES)}')
    for c in ('noticed', 'failed'):
        if row[c].strip().lower() not in ('y', 'n'):
            out.append(f'row {n}: {c} must be y or n')
    for c in ('dict_right_sense', 'dict_first'):
        if row[c].strip().lower() not in ('y', 'n', 'na', ''):
            out.append(f'row {n}: {c} must be y, n or na')
    return out


def gate(args):
    rows, problems = [], []
    for p in args.sheets:
        with open(p, newline='', encoding='utf-8') as f:
            for n, row in enumerate(csv.DictReader(f), start=2):
                problems += [f'{p} {m}' for m in problems_in(row, n)]
                rows.append(row)
    if problems:
        print('The coding is incomplete; Gate 0 is not computed:')
        for m in problems[:40]:
            print('  ' + m)
        return 2
    local = [r for r in rows if r['type'].strip().upper() in LOCAL]
    failed = [r for r in local if r['failed'].strip().lower() == 'y']
    readers = {r['reader'] for r in local}
    share = len(local) / len(rows) if rows else 0
    fail_share = len(failed) / len(local) if local else 0
    by_type = {t: sum(r['type'].strip().upper() == t for r in rows) for t in TYPES}
    unnoticed = sum(r['noticed'].strip().lower() == 'n' for r in rows)
    print(f'Stuck points: {len(rows)} from {len({r["reader"] for r in rows})} readers ({unnoticed} unnoticed at first)')
    print('By type: ' + ', '.join(f'{t} {n}' for t, n in by_type.items() if n))
    print(f'Local (L2, L3, L4, S1, S2): {len(local)} = {share:.0%} (needs ≥ {LOCAL_SHARE:.0%})')
    print(f'The current tool failed on: {len(failed)} of them = {fail_share:.0%} (needs ≥ {FAILED_SHARE:.0%})')
    print(f'Floors: {len(local)} local stuck points (needs ≥ {MIN_LOCAL}), from {len(readers)} readers (needs ≥ {MIN_READERS})')
    # The dictionary-ceiling check (Report 1): how often the reader's MDX had the right sense first.
    coded = [r for r in local if r['dict_right_sense'].strip().lower() in ('y', 'n')]
    if coded:
        right = sum(r['dict_right_sense'].strip().lower() == 'y' for r in coded)
        first = sum(r['dict_first'].strip().lower() == 'y' for r in coded)
        print(f'Dictionary: right sense for {right} of {len(coded)} local items; listed first for {first}')
    ok = share >= LOCAL_SHARE and fail_share >= FAILED_SHARE and len(local) >= MIN_LOCAL and len(readers) >= MIN_READERS
    print('Gate 0: PASS' if ok else 'Gate 0: FAIL')
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    s = sub.add_parser('sheet', help='rows to code from one reader’s exports')
    s.add_argument('reader')
    s.add_argument('exports', nargs='+', help='export files or folders')
    s.add_argument('-o', '--output', required=True)
    g = sub.add_parser('gate', help='Gate 0 from coded sheets')
    g.add_argument('sheets', nargs='+')
    args = ap.parse_args()
    if args.cmd == 'sheet':
        sheet(args)
        return 0
    return gate(args)


if __name__ == '__main__':
    sys.exit(main())
