#!/usr/bin/env python3
"""Export the design bundles in docs/design/ as plain Markdown, one file per board.

The design files are self-extracting bundles: each board is a gzip-compressed,
base64-encoded HTML page inside the root document's manifest. This script unpacks
them without a browser and writes docs/design/text/<bundle>/<board>.md so rule
citations can be reviewed and design revisions show up as text diffs
(implementation plan §5, Phase 0).

Usage:
  scripts/export-design-text.py           write the export
  scripts/export-design-text.py --check   exit 1 if the committed export is stale
"""

import base64
import gzip
import html
import json
import re
import sys
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DESIGN = ROOT / "docs" / "design"
OUT = DESIGN / "text"
BUNDLE_NAME = re.compile(r"^Quiet EPUB Reader \((?P<name>[^)]+)\)\.html$")


def island(doc: str, kind: str):
    m = re.search(
        r'<script type="__bundler/%s">\s*(.*?)\s*</script>' % re.escape(kind), doc, re.S
    )
    return json.loads(m.group(1)) if m else None


def decode(entry: dict) -> str:
    data = base64.b64decode(entry["data"])
    if entry.get("compressed"):
        data = gzip.decompress(data)
    return data.decode("utf-8")


def boards(bundle: str):
    """Yield (title, page_html) for each board, in page order."""
    manifest = island(bundle, "manifest") or {}
    template = island(bundle, "template") or ""
    order = island(bundle, "page_order") or []
    titles = dict(
        (uuid, html.unescape(title))
        for uuid, title in re.findall(
            r'<iframe src="about:blank#([0-9a-f-]{36})[^"]*" title="([^"]*)"', template
        )
    )
    for uuid in order:
        page = decode(manifest[uuid])
        # Each board is itself a bundle; its readable markup is the template.
        yield titles.get(uuid, uuid), island(page, "template") or page


BLOCK = {
    "address", "article", "aside", "blockquote", "body", "dd", "details", "dialog",
    "div", "dl", "dt", "fieldset", "figcaption", "figure", "footer", "form", "header",
    "hr", "html", "legend", "li", "main", "nav", "ol", "p", "pre", "section",
    "summary", "table", "tbody", "thead", "tfoot", "ul", "x-dc",
}
HEADINGS = {"h1": "# ", "h2": "## ", "h3": "### ", "h4": "#### ", "h5": "##### ", "h6": "###### "}
SKIP = {"script", "style", "helmet", "head", "template", "noscript", "svg"}
# The design tool renders tables as <sc-raw-table>, <sc-raw-tr>, <sc-raw-td> ...
RAW_PREFIX = re.compile(r"^sc-raw-")
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"}


class TextExtractor(HTMLParser):
    """Turn board markup into readable Markdown: block structure, headings, lists,
    tables and bold text. Styling and layout are dropped."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.lines: list[str] = []
        self.line = ""
        self.skip = 0
        self.pre = 0
        self.list_depth = 0
        self.row: list[str] | None = None
        self.cell: str | None = None
        self.table_rows: list[list[str]] = []
        self.boundary = False

    # -- output helpers
    def _write(self, text: str):
        if self.cell is not None:
            self.cell += text
        else:
            self.line += text

    def _break(self):
        if self.cell is not None:
            self.cell += " "
            return
        line = self.line if self.pre else re.sub(r"[ \t]+", " ", self.line).strip()
        if line:
            self.lines.append(line)
        self.line = ""

    def _blank(self):
        self._break()
        if self.lines and self.lines[-1] != "":
            self.lines.append("")

    # -- parser callbacks
    def handle_starttag(self, tag, attrs):
        tag = RAW_PREFIX.sub("", tag)
        if tag in SKIP:
            if tag not in VOID:
                self.skip += 1
            return
        if self.skip:
            return
        attrs = dict(attrs)
        if tag in HEADINGS:
            self._blank()
            self._write(HEADINGS[tag])
        elif tag == "table":
            self._blank()
            self.table_rows = []
        elif tag == "tr":
            self.row = []
        elif tag in ("td", "th"):
            self.cell = ""
        elif tag == "li":
            self._break()
            self._write("  " * max(self.list_depth - 1, 0) + "- ")
        elif tag in ("ul", "ol"):
            self._break()
            self.list_depth += 1
        elif tag == "pre":
            self._blank()
            self.pre += 1
            self.lines.append("```")
        elif tag == "br":
            self._break()
        elif tag == "hr":
            self._blank()
        elif tag == "p":
            self._blank()
        elif tag in BLOCK:
            self._break()
        else:
            # Boards lay chips, key caps and labels out with flex, so adjacent
            # inline runs are separate words on screen; keep them apart in text.
            self.boundary = True
            if tag in ("b", "strong"):
                self._separate("*")
                self._write("**")
                self.boundary = False
            elif tag == "img" and attrs.get("alt"):
                self._separate("[")
                self._write("[image: %s]" % attrs["alt"])
            elif tag == "input" and (attrs.get("value") or attrs.get("placeholder")):
                text = attrs.get("value") or attrs.get("placeholder")
                self._separate("[")
                self._write("[%s]" % text)

    def handle_endtag(self, tag):
        tag = RAW_PREFIX.sub("", tag)
        if tag in SKIP:
            if tag not in VOID:
                self.skip = max(self.skip - 1, 0)
            # An icon between two labels separates them on screen.
            self.boundary = not self.skip
            return
        if self.skip:
            return
        if tag in HEADINGS:
            self._blank()
        elif tag in ("td", "th"):
            if self.row is not None and self.cell is not None:
                self.row.append(re.sub(r"\s+", " ", self.cell).strip().replace("|", "\\|"))
            self.cell = None
        elif tag == "tr":
            if self.row:
                self.table_rows.append(self.row)
            self.row = None
        elif tag == "table":
            self._emit_table()
        elif tag in ("ul", "ol"):
            self.list_depth = max(self.list_depth - 1, 0)
            self._break()
        elif tag == "pre":
            self._break()
            self.lines.append("```")
            self.lines.append("")
            self.pre = max(self.pre - 1, 0)
        elif tag == "p":
            self._blank()
        elif tag in BLOCK:
            self._break()
        else:
            if tag in ("b", "strong"):
                # Keep whitespace outside the closing marker: "**open** 1 click".
                buf = self.cell if self.cell is not None else self.line
                trimmed = buf.rstrip()
                tail = buf[len(trimmed):]
                if trimmed.endswith("**"):
                    trimmed = trimmed[:-2]  # empty bold run
                    tail = ""
                else:
                    trimmed += "**"
                if self.cell is not None:
                    self.cell = trimmed + tail
                else:
                    self.line = trimmed + tail
            self.boundary = True

    def handle_data(self, data):
        if self.skip:
            return
        if not self.pre:
            data = re.sub(r"\s+", " ", data)
        if data:
            self._separate(data[0])
            self._write(data)

    def _separate(self, next_char: str):
        if not self.boundary:
            return
        self.boundary = False
        buf = self.cell if self.cell is not None else self.line
        prev = buf[-1:] if buf else ""
        if not prev or prev.isspace() or prev in "([{‘“" or buf.endswith("- "):
            return
        if next_char.isspace() or next_char in ".,;:!?)]}’”%":
            return
        self._write(" ")

    def _emit_table(self):
        rows = [r for r in self.table_rows if any(c for c in r)]
        self.table_rows = []
        if not rows:
            return
        width = max(len(r) for r in rows)
        rows = [r + [""] * (width - len(r)) for r in rows]
        self.lines.append("| " + " | ".join(rows[0]) + " |")
        self.lines.append("|" + "---|" * width)
        for r in rows[1:]:
            self.lines.append("| " + " | ".join(r) + " |")
        self.lines.append("")

    def markdown(self) -> str:
        self._break()
        text = "\n".join(self.lines)
        text = re.sub(r"\n{3,}", "\n\n", text)
        return text.strip() + "\n"


def board_markdown(bundle_name: str, title: str, page: str) -> str:
    body = page.split("<body", 1)[-1]
    parser = TextExtractor()
    parser.feed("<body" + body)
    parser.close()
    header = (
        f"<!-- Generated by scripts/export-design-text.py from "
        f"docs/design/Quiet EPUB Reader ({bundle_name}).html. Do not edit. -->\n\n"
    )
    return header + f"# {title}\n\n" + parser.markdown()


def safe_name(title: str) -> str:
    return re.sub(r'[/\\:*?"<>|]', "-", title).strip() + ".md"


def build() -> dict[Path, str]:
    files: dict[Path, str] = {}
    for path in sorted(DESIGN.glob("*.html")):
        m = BUNDLE_NAME.match(path.name)
        if not m:
            continue
        name = m.group("name")
        bundle = path.read_text(encoding="utf-8")
        index = [f"# {name.capitalize()} boards\n", f"Source: `docs/design/{path.name}`\n"]
        for title, page in boards(bundle):
            fname = safe_name(title)
            files[OUT / name / fname] = board_markdown(name, title, page)
            index.append(f"- [{title}](<{fname}>)")
        files[OUT / name / "README.md"] = "\n".join(index) + "\n"
    return files


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = build()
    existing = set(OUT.rglob("*.md")) if OUT.exists() else set()
    stale = sorted(
        [p for p, text in files.items() if not p.exists() or p.read_text(encoding="utf-8") != text]
        + [p for p in existing - set(files)]
    )
    if check:
        for p in stale:
            print(f"stale: {p.relative_to(ROOT)}")
        if stale:
            print("Design text export is out of date. Run scripts/export-design-text.py.")
            return 1
        print(f"Design text export is up to date ({len(files)} files).")
        return 0
    for p in existing - set(files):
        p.unlink()
    for p, text in files.items():
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")
    print(f"Wrote {len(files)} files to {OUT.relative_to(ROOT)} ({len(stale)} changed).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
