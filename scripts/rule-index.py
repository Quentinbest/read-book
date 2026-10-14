#!/usr/bin/env python3
"""Rule index: where each plan rule ID is defined and which owner decisions change it.

  python3 scripts/rule-index.py            # write docs/rule-index.md
  python3 scripts/rule-index.py --check    # exit 1 if docs/rule-index.md is out of date
  python3 scripts/rule-index.py S9 L14     # look up rules: plan text, decisions, code and e2e checks

The plan (docs/implementation-plan.md) is read-only; owner decisions that change a rule
live in docs/decisions.md. The index joins the two so a rule is never read without its
overrides. Code citations change with every commit, so they are only in the lookup.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLAN = ROOT / "docs/implementation-plan.md"
DECISIONS = ROOT / "docs/decisions.md"
OUT = ROOT / "docs/rule-index.md"

# Not D-D1 (a spike finding); B3-guard still counts as B3.
RULE_ID = re.compile(r"(?<![\w-])([A-Z]{1,2}\d{1,2})(?!\w)")
TABLE_ROW = re.compile(r"^\|\s*([A-Z]{1,2}\d{1,2})\s*\|\s*(.*?)\s*\|")
BULLET = re.compile(r"^-\s+\*\*([A-Z]{1,2}\d{1,2})\*\*\s+(.*)")


def plain(text: str, limit: int = 110) -> str:
    text = re.sub(r"\*\*|`", "", text).replace("|", "/").strip()
    return text if len(text) <= limit else text[: limit - 1].rstrip() + "…"


# Plan table columns that carry a rule's substance beyond its second cell, read from
# each table's header: a question's or conflict's decision goes first (it is the rule),
# a shortcut's macOS keys after the action. Source, Phase, Owner and dates are left out.
DECISION_COLUMNS = ("Decision", "Decision (accepted default)", "Resolved by")
EXTRA_COLUMNS = ("macOS",)


def row_text(header: list[str], cells: list[str]) -> str:
    text = cells[1]
    named = dict(zip(header, cells))
    for col in DECISION_COLUMNS:
        if named.get(col):
            text = f"Decision: {named[col]} (on: {text})"
    for col in EXTRA_COLUMNS:
        if named.get(col):
            text = f"{text}; {col}: {named[col]}"
    return text


def plan_rules() -> dict[str, dict]:
    rules: dict[str, dict] = {}
    section = ""
    header: list[str] = []
    for n, line in enumerate(PLAN.read_text().splitlines(), 1):
        if line.startswith("#"):
            section = line.lstrip("#").strip()
            continue
        if line.startswith("|"):
            cells = [c.strip() for c in line.strip("|").split("|")]
            if line.startswith("|---"):
                continue
            if not TABLE_ROW.match(line):
                header = cells
                continue
            rid = cells[0]
            if rid not in rules and len(cells) >= 2:
                rules[rid] = {"line": n, "section": section, "text": row_text(header, cells)}
            continue
        header = []
        m = BULLET.match(line)
        if m and m.group(1) not in rules:
            rules[m.group(1)] = {"line": n, "section": section, "text": m.group(2)}
    return rules


# Decision sections whose IDs belong to other work and only look like plan rules:
# (words in the section heading, the IDs that are not the plan's there).
OTHER_IDS = [
    ("Reading Lens gate thresholds", re.compile(r"G\d")),  # Reading Lens gates G0–G3
    ("reader panels and the lookup budget", re.compile(r"Q\d")),  # that review's questions
]
# An amendment named in a row's text or a section's prose: "amends A1", "an exception to X4".
AMENDS = re.compile(
    r"\b(?:amends?|amended|exception to|replaces|supersedes|overrides)\s+"
    r"((?:[A-Z]{1,2}\d{1,2})(?:(?:,\s*|\s+and\s+)[A-Z]{1,2}\d{1,2})*)"
)


def decisions(known: set[str]) -> dict[str, list[dict]]:
    """Decisions that change a plan rule: table rows whose first cell names it, and rows
    or prose that say they amend it."""
    found: dict[str, list[dict]] = {}
    section = "Decision record"

    def add(rid: str, n: int, label: str, text: str) -> None:
        if rid not in known or any(
            words in section and pattern.fullmatch(rid) for words, pattern in OTHER_IDS
        ):
            return
        if not any(d["line"] == n for d in found.get(rid, [])):
            found.setdefault(rid, []).append(
                {"line": n, "section": section, "label": label, "text": text}
            )

    for n, line in enumerate(DECISIONS.read_text().splitlines(), 1):
        if line.startswith("## "):
            section = line[3:].strip()
            continue
        if line.startswith("|---") or line.startswith("| #"):
            continue
        if line.startswith("|"):
            cells = [c.strip() for c in line.strip("|").split("|")]
            if len(cells) < 2:
                continue
            for rid in dict.fromkeys(RULE_ID.findall(cells[0])):
                add(rid, n, cells[0], cells[1])
            rest = " | ".join(cells[1:])
        else:
            rest = line
        for m in AMENDS.finditer(rest):
            for rid in RULE_ID.findall(m.group(1)):
                add(rid, n, m.group(0), rest)
    return found


def sort_key(rid: str):
    m = re.match(r"([A-Z]+)(\d+)", rid)
    return (m.group(1), int(m.group(2)))


def render() -> str:
    rules = plan_rules()
    changes = decisions(set(rules))
    out = [
        "<!-- Generated by scripts/rule-index.py from docs/implementation-plan.md and docs/decisions.md. Do not edit. -->",
        "",
        "# Rule index",
        "",
        "Every rule ID in the plan, with the owner decisions that change it. **When a rule has decisions, they win over the plan**, newest last.",
        "Look up one rule with its code and e2e checks: `python3 scripts/rule-index.py S9`.",
        "",
        f"{len(rules)} rules; {len(changes)} changed or settled by a decision.",
        "",
        "| Rule | Plan | Rule text (abridged) | Decisions (docs/decisions.md) |",
        "|---|---|---|---|",
    ]
    for rid in sorted(rules, key=sort_key):
        r = rules[rid]
        # Sections, not line numbers: any edit above a row would move its line and make
        # every open PR's index stale. The lookup prints the lines.
        sections = dict.fromkeys(d["section"] for d in changes.get(rid, []))
        dec = "<br>".join(sections)
        out.append(f"| {rid} | L{r['line']} | {plain(r['text'])} | {dec} |")
    return "\n".join(out) + "\n"


def lookup(rid: str) -> None:
    rules = plan_rules()
    if rid not in rules:
        print(f"{rid}: not a rule ID in {PLAN.relative_to(ROOT)}")
        return
    r = rules[rid]
    print(f"== {rid}  (plan L{r['line']}, {r['section']})\n{r['text']}\n")
    for d in decisions({rid}).get(rid, []):
        print(f"-- decisions.md L{d['line']}, {d['section']} [{d['label']}]\n{d['text']}\n")
    code = subprocess.run(
        ["git", "grep", "-c", "-w", rid, "--", "src", "src-tauri/src", ":!src/spikes"],
        cwd=ROOT, capture_output=True, text=True,
    ).stdout.split()
    print("code:", ", ".join(code) if code else "(none)")
    checks = subprocess.run(
        ["git", "grep", "-n", "-o", "-E", rf"id: '{rid}[-'][^']*", "--", "src/spikes/e2e.ts"],
        cwd=ROOT, capture_output=True, text=True,
    ).stdout.split("\n")
    # "src/spikes/e2e.ts:1234:id: 'S9-edge-reveal" → "S9-edge-reveal (e2e.ts:1234)"
    names = [f"{c.split(chr(39))[1]} (e2e.ts:{c.split(':')[1]})" for c in checks if "'" in c]
    print("e2e checks:", ", ".join(names) if names else "(none)")
    print()


def main(args: list[str]) -> int:
    if args and args[0] != "--check":
        for rid in args:
            lookup(rid.upper())
        return 0
    text = render()
    if args == ["--check"]:
        if not OUT.exists() or OUT.read_text() != text:
            print(f"{OUT.relative_to(ROOT)} is out of date: run `pnpm rules:index`", file=sys.stderr)
            return 1
        return 0
    OUT.write_text(text)
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
