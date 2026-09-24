#!/usr/bin/env python3
"""Check rule-register values (docs/implementation-plan.md §2) against the design text export.

For each rule row, extract concrete values (numbers with units, colours, ratios,
quoted UI strings, shortcut glyphs) and look for each one in the export files the
row cites. Prints a report of rows whose values were not found, for a human
reviewer to resolve (spec gate §1.2 item 3). Exit status is always 0: a miss is
a prompt to review, not proof of an error.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLAN = ROOT / "docs" / "implementation-plan.md"
TEXT = ROOT / "docs" / "design" / "text"

PROPOSAL = [(1, 5, "Proposal 1"), (6, 10, "Proposal 2"), (11, 16, "Proposal 3"), (17, 20, "Proposal 4"), (21, 25, "Proposal 5")]


def norm(s: str) -> str:
    s = s.replace(" ", " ").replace(" ", " ").replace(" ", " ")
    s = s.replace("−", "-").replace("–", "-").replace("×", "x")
    s = s.replace("’", "'").replace("‘", "'").replace("“", '"').replace("”", '"')
    return re.sub(r"\s+", " ", s).lower()


def files_for(source: str) -> list[Path]:
    out: list[Path] = []
    for n in re.findall(r"P§(\d+)", source):
        n = int(n)
        for lo, hi, name in PROPOSAL:
            if lo <= n <= hi:
                out += list((TEXT / "proposal").glob(name + " *.md"))
    for s in re.findall(r"\bS([1-5])\b", source):
        out += list((TEXT / "system").glob(f"S{s} *.md"))
    for group in re.findall(r"Screens? ([\d, and]+)", source):
        for n in re.findall(r"\d+", group):
            out += list((TEXT / "screens").glob(f"{int(n):02d} *.md"))
    return sorted(set(out))


VALUE = re.compile(
    r"#[0-9A-Fa-f]{6}"                                   # colours
    r"|\d+(?:\.\d+)?\s?:\s?1"                            # contrast ratios
    r"|\d+(?:[.,]\d+)?(?:\s?[–-]\s?\d+(?:\.\d+)?)?\s?(?:px|ms|ch|MB|GB|vh|%|s|em)\b"  # measures
    r"|“[^”]{3,}”"                                        # quoted UI strings
    r"|[⌘⇧⌥⌃]+[^\s·,/()]*"                              # macOS shortcuts
)


def values(rule: str) -> list[str]:
    rule = re.sub(r"\(prov\.\)", "", rule)
    return [v.rstrip(";,.") for v in VALUE.findall(rule)]


def found(value: str, text: str) -> bool:
    v = norm(value).strip('"')
    if v in text:
        return True
    # Tolerate spacing differences such as "12 px" vs "12px", "3 s" vs "3s".
    return v.replace(" ", "") in text.replace(" ", "")


def main() -> int:
    plan = PLAN.read_text(encoding="utf-8")
    section = plan.split("## 2. Rule register", 1)[1].split("## 3.", 1)[0]
    rows = 0
    flagged = 0
    for line in section.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 3 or not re.fullmatch(r"[A-Z]\d+", cells[0]):
            continue
        rows += 1
        rid = cells[0]
        if rid.startswith("K"):
            rule, source = " ".join(cells[1:4]), cells[4]
        else:
            rule, source = cells[1], cells[2]
        files = files_for(source)
        text = norm(" ".join(p.read_text(encoding="utf-8") for p in files))
        missing = [v for v in values(rule) if not found(v, text)]
        if not files or missing:
            flagged += 1
            names = ", ".join(p.stem for p in files) or "NO FILES RESOLVED"
            print(f"{rid}: missing {missing}  [{names}]")
    print(f"\n{rows} rows checked, {flagged} flagged for review.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
