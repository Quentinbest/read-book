#!/usr/bin/env python3
"""Generate the synthetic part of the test corpus (plan §6.2) into corpus/generated/.

Output is deterministic. Requires the downloaded corpus (scripts/corpus/fetch.py)
for realistic text: chapters are taken from the Standard Ebooks Moby-Dick.

Files:
  hostile-*.epub   script, network, IPC, overlay and file-access probes (Spike E, §7.1)
  zip-*.epub       hostile archives: traversal, absolute, symlink, bomb, many entries
  xml-*.epub       XML entity attacks in the OPF
  broken-*.epub    truncated, bad OPF, missing items, bad CSS/fonts, no metadata, no TOC
  large-100mb.epub a ~100 MB book (L17, §6.4 budgets)
  long-chapter.epub one chapter over 1 MB (L16)
  code-heavy.epub  more than 30% code and tables (L18)

Probe convention for hostile books: any script that runs marks the document with
data-pwned and tries to reach the canary at http://127.0.0.1:8765/ and the
Tauri IPC. The Spike E harness fails the probe on any of those signals.
"""

import html
import uuid
import random
import re
import stat
import struct
import sys
import zipfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "corpus" / "generated"
MOBY = ROOT / "corpus" / "cache" / "standardebooks-moby-dick.epub"
CANARY = "http://127.0.0.1:8765"
FIXED_TIME = (2026, 1, 1, 0, 0, 0)


# ---------------------------------------------------------------- EPUB builder

def xhtml(title: str, body: str, head: str = "") -> str:
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n<!DOCTYPE html>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en">\n'
        f"<head><meta charset=\"utf-8\"/><title>{html.escape(title)}</title>{head}</head>\n"
        f"<body>\n{body}\n</body>\n</html>\n"
    )


def opf(title: str, items: list[tuple[str, str, str, str]], spine: list[str], *, meta: bool = True,
        nav: bool = True, ident: str = "urn:uuid:00000000-0000-4000-8000-000000000000",
        doctype: str = "") -> str:
    """items: (id, href, media-type, properties)"""
    md = (
        f"<dc:identifier id=\"uid\">{ident}</dc:identifier>"
        f"<dc:title>{html.escape(title)}</dc:title><dc:language>en</dc:language>"
        "<meta property=\"dcterms:modified\">2026-01-01T00:00:00Z</meta>"
        if meta else
        f"<dc:identifier id=\"uid\">{ident}</dc:identifier><meta property=\"dcterms:modified\">2026-01-01T00:00:00Z</meta>"
    )
    manifest = "".join(
        f'<item id="{i}" href="{h}" media-type="{m}"' + (f' properties="{p}"' if p else "") + "/>"
        for i, h, m, p in items
    )
    if nav:
        manifest += '<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
    itemrefs = "".join(f'<itemref idref="{s}"/>' for s in spine)
    return (
        f'<?xml version="1.0" encoding="utf-8"?>\n{doctype}'
        '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid" '
        'xmlns:dc="http://purl.org/dc/elements/1.1/">'
        f"<metadata>{md}</metadata><manifest>{manifest}</manifest><spine>{itemrefs}</spine></package>\n"
    )


def nav_doc(entries: list[tuple[str, str]]) -> str:
    lis = "".join(f'<li><a href="{h}">{html.escape(t)}</a></li>' for h, t in entries)
    return xhtml("Contents", f'<nav epub:type="toc" id="toc"><h1>Contents</h1><ol>{lis}</ol></nav>')


CONTAINER = (
    '<?xml version="1.0"?>\n<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
    '<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>\n'
)


def zinfo(name: str, compress: int = zipfile.ZIP_DEFLATED) -> zipfile.ZipInfo:
    zi = zipfile.ZipInfo(name, FIXED_TIME)
    zi.compress_type = compress
    zi.external_attr = 0o644 << 16
    return zi


def write_epub(path: Path, files: dict[str, bytes | str], extra=None, compress_large=True):
    """files maps paths relative to OEBPS/ to content; extra(zf) may add raw entries."""
    with zipfile.ZipFile(path, "w") as zf:
        zf.writestr(zinfo("mimetype", zipfile.ZIP_STORED), "application/epub+zip")
        zf.writestr(zinfo("META-INF/container.xml"), CONTAINER)
        for name, data in files.items():
            if isinstance(data, str):
                data = data.encode("utf-8")
            method = zipfile.ZIP_STORED if (not compress_large and len(data) > 1 << 20) else zipfile.ZIP_DEFLATED
            zf.writestr(zinfo("OEBPS/" + name, method), data)
        if extra:
            extra(zf)


def simple_book(path: Path, title: str, chapters: list[tuple[str, str]], *, head: str = "",
                resources: dict[str, tuple[bytes | str, str]] | None = None, **opf_kw):
    # A distinct, stable identifier per file unless the caller sets one.
    opf_kw.setdefault("ident", "urn:uuid:" + str(uuid.uuid5(uuid.NAMESPACE_URL, "linen-corpus:" + path.name)))
    files: dict[str, bytes | str] = {}
    items = []
    spine = []
    for n, (ctitle, body) in enumerate(chapters, 1):
        href = f"chapter-{n}.xhtml"
        files[href] = xhtml(ctitle, body, head)
        items.append((f"c{n}", href, "application/xhtml+xml", opf_kw.pop(f"props{n}", "")))
        spine.append(f"c{n}")
    for n, (href, (data, mtype)) in enumerate((resources or {}).items()):
        files[href] = data
        props = "svg" if mtype == "image/svg+xml" and href.endswith(".xhtml") else ""
        items.append((f"r{n}", href, mtype, props))
    if opf_kw.get("nav", True):
        files["nav.xhtml"] = nav_doc([(f"chapter-{i}.xhtml", t) for i, (t, _) in enumerate(chapters, 1)])
    files["content.opf"] = opf(title, items, spine, **opf_kw)
    write_epub(path, files)


# ---------------------------------------------------------------- source text

def moby_chapters() -> list[tuple[str, str]]:
    if not MOBY.exists():
        sys.exit("corpus/cache is missing; run scripts/corpus/fetch.py first")
    out = []
    with zipfile.ZipFile(MOBY) as zf:
        names = sorted(
            (n for n in zf.namelist() if re.search(r"chapter-\d+\.xhtml$", n)),
            key=lambda n: int(re.search(r"(\d+)\.xhtml$", n).group(1)),
        )
        for n in names:
            doc = zf.read(n).decode("utf-8")
            title = re.search(r"<title>(.*?)</title>", doc, re.S).group(1)
            body = re.search(r"<body[^>]*>(.*)</body>", doc, re.S).group(1)
            out.append((html.unescape(title), body))
    return out


# ---------------------------------------------------------------- hostile books

def probe_js(pid: str) -> str:
    """Script body run by a probe. Every channel it uses is a failure signal."""
    return (
        "(function(){var id=%r;"
        "try{document.documentElement.setAttribute('data-pwned',(document.documentElement.getAttribute('data-pwned')||'')+' '+id)}catch(e){}"
        "try{new Image().src=%r+'/script?id='+id}catch(e){}"
        "try{fetch(%r+'/fetch?id='+id)}catch(e){}"
        "var ws=[window,window.parent,window.top];"
        "for(var i=0;i<ws.length;i++){try{var t=ws[i].__TAURI_INTERNALS__;if(t&&t.invoke){t.invoke('spike_canary',{id:id,channel:'internals'+i})}}catch(e){}}"
        "try{window.webkit.messageHandlers.ipc.postMessage(JSON.stringify({cmd:'spike_canary',id:id}))}catch(e){}"
        "try{fetch('ipc://localhost/spike_canary',{method:'POST',body:JSON.stringify({id:id,channel:'ipc-scheme'})})}catch(e){}"
        "try{window.top.postMessage({spikeCanary:id},'*')}catch(e){}"
        "})();"
    ) % (pid, CANARY, CANARY)


def hostile_books():
    js = probe_js
    script_body = f"""
<h1>Script probes</h1>
<p id="p-inline">Inline script follows.</p>
<script>{js('inline-script')}</script>
<script src="evil.js"></script>
<script type="module">{js('module-script')}</script>
<img id="p-onerror" src="does-not-exist.png" onerror="{html.escape(js('img-onerror'))}" alt="x"/>
<p><a id="p-jslink" href="javascript:{html.escape(js('javascript-link'))}">javascript: link</a></p>
<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" onload="{html.escape(js('svg-onload'))}"><script>{js('inline-svg-script')}</script></svg>
<iframe id="p-iframe-js" src="javascript:parent.document&amp;&amp;0" title="js"></iframe>
<iframe id="p-srcdoc" srcdoc="&lt;script&gt;{html.escape(js('srcdoc-script'))}&lt;/script&gt;" title="srcdoc"></iframe>
<iframe id="p-data-frame" src="data:text/html,%3Cscript%3E{js('data-iframe').replace('%', '%25').replace('#', '%23')}%3C/script%3E" title="data"></iframe>
<object id="p-object" data="evil.svg" type="image/svg+xml">object</object>
<embed id="p-embed" src="evil.svg" type="image/svg+xml"/>
<form id="p-form" action="{CANARY}/form" method="get"><input name="id" value="form-submit"/><button id="p-form-button">Submit</button></form>
<details open="open" ontoggle="{html.escape(js('details-ontoggle'))}"><summary>details</summary>d</details>
<p id="p-style-expr" style="background:url(&quot;javascript:{html.escape(js('css-url-js'))}&quot;)">css url javascript:</p>
"""
    network_body = f"""
<h1>Network probes</h1>
<img id="n-img" src="{CANARY}/img" alt="remote image"/>
<img id="n-srcset" srcset="{CANARY}/srcset 2x" src="does-not-exist.png" alt="srcset"/>
<p id="n-bg" style="background-image:url({CANARY}/css-inline-bg)">inline style background</p>
<p class="remote-bg">stylesheet background</p>
<p class="remote-font">remote font</p>
<video id="n-video" src="{CANARY}/video" preload="auto"></video>
<audio id="n-audio" src="{CANARY}/audio" preload="auto"></audio>
<link rel="prefetch" href="{CANARY}/prefetch"/>
<a id="n-link" href="https://example.com/">external link (must open in the system browser, never inside the app)</a>
"""
    network_head = (
        f'<link rel="stylesheet" href="{CANARY}/css-link"/>'
        f"<style>@import url({CANARY}/css-import);"
        f".remote-bg{{background:url({CANARY}/css-bg)}}"
        f"@font-face{{font-family:Evil;src:url({CANARY}/font)}}.remote-font{{font-family:Evil}}</style>"
        f'<meta http-equiv="refresh" content="3;url={CANARY}/meta-refresh"/>'
    )
    overlay_body = """
<h1>Overlay probe</h1>
<div id="o-fixed" style="position:fixed;top:0;left:0;right:0;bottom:0;z-index:2147483647;background:#fff;color:#000">
<p>Linen needs you to sign in again.</p><p><input type="password" placeholder="Password"/></p></div>
<div id="o-sticky" style="position:sticky;top:0">sticky</div>
"""
    file_body = f"""
<h1>File and app-origin probes</h1>
<img id="f-etc" src="file:///etc/hosts" alt="file url"/>
<img id="f-traverse" src="../../../../../../etc/hosts" alt="traversal"/>
<img id="f-other-book" src="../../other-book/OEBPS/cover.jpg" alt="other book"/>
<iframe id="f-app" src="tauri://localhost/" title="app origin"></iframe>
<iframe id="f-book-scheme" src="book://localhost/" title="book scheme"></iframe>
<a id="f-top-nav" href="tauri://localhost/" target="_top">navigate top</a>
<a id="f-blank" href="{CANARY}/window-open" target="_blank">new window</a>
"""
    svg_doc = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        f'<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" onload="{html.escape(js("svg-file-onload"))}">'
        f"<script>{js('svg-file-script')}</script><rect width=\"100\" height=\"100\" fill=\"red\"/></svg>\n"
    )
    svg_page = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>SVG page</title></head><body>'
        f'<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><script>{js("svg-spine-script")}</script>'
        '<rect width="100" height="100" fill="red"/></svg></body></html>\n'
    )
    simple_book(
        OUT / "hostile-content.epub", "Hostile content probes",
        [("Script probes", script_body), ("Network probes", network_body), ("Overlay probe", overlay_body),
         ("File probes", file_body), ("SVG page", svg_page_body(svg_page))],
        head=network_head,
        resources={"evil.js": (js("external-script"), "application/javascript"),
                   "evil.svg": (svg_doc, "image/svg+xml")},
        props1="scripted", props5="scripted svg",
        ident="urn:uuid:5a1e0000-0000-4000-8000-00000000e001",
    )


def svg_page_body(page: str) -> str:
    return re.search(r"<body>(.*)</body>", page, re.S).group(1)


# ---------------------------------------------------------------- hostile archives

def zip_attacks(chapters):
    base_title, base_body = chapters[0]

    def base(path: Path, extra):
        files = {
            "chapter-1.xhtml": xhtml(base_title, base_body),
            "nav.xhtml": nav_doc([("chapter-1.xhtml", base_title)]),
            "content.opf": opf("Zip probe", [("c1", "chapter-1.xhtml", "application/xhtml+xml", "")], ["c1"]),
        }
        write_epub(path, files, extra)

    base(OUT / "zip-traversal.epub", lambda zf: zf.writestr(zinfo("../../escaped.txt"), "escaped"))
    base(OUT / "zip-absolute.epub", lambda zf: zf.writestr(zinfo("/tmp/linen-absolute.txt"), "absolute"))

    def symlink(zf):
        zi = zinfo("OEBPS/link-to-etc")
        zi.external_attr = (stat.S_IFLNK | 0o777) << 16
        zi.create_system = 3
        zf.writestr(zi, "/etc/hosts")
    base(OUT / "zip-symlink.epub", symlink)

    def bomb(zf):
        # 1 GiB of zeros deflates to about 1 MB: ratio ~1000:1, over the 100:1 limit.
        zi = zinfo("OEBPS/bomb.bin")
        with zf.open(zi, "w", force_zip64=True) as f:
            block = b"\0" * (1 << 20)
            for _ in range(1024):
                f.write(block)
    base(OUT / "zip-bomb.epub", bomb)

    def many(zf):
        for i in range(100_000):
            zf.writestr(zinfo(f"OEBPS/many/{i}.txt", zipfile.ZIP_STORED), b"")
    base(OUT / "zip-100k-entries.epub", many)


def xml_attacks(chapters):
    title, body = chapters[0]
    items = [("c1", "chapter-1.xhtml", "application/xhtml+xml", "")]
    lol = (
        '<!DOCTYPE package [<!ENTITY a "aaaaaaaaaa">'
        + "".join(f'<!ENTITY {chr(98 + i)} "{("&" + chr(97 + i) + ";") * 10}">' for i in range(9))
        + "]>\n"
    )
    xxe = '<!DOCTYPE package [<!ENTITY xxe SYSTEM "file:///etc/hosts">]>\n'
    for name, doctype, t in (("xml-billion-laughs.epub", lol, "&j;"), ("xml-external-entity.epub", xxe, "&xxe;")):
        doc = opf("TITLE", items, ["c1"], doctype=doctype).replace("TITLE", t)
        write_epub(OUT / name, {"chapter-1.xhtml": xhtml(title, body), "nav.xhtml": nav_doc([("chapter-1.xhtml", title)]),
                                "content.opf": doc})


# ---------------------------------------------------------------- broken books

def broken_books(chapters):
    few = chapters[:3]
    simple_book(OUT / "broken-ok-reference.epub", "Reference (valid)", few)
    data = (OUT / "broken-ok-reference.epub").read_bytes()
    (OUT / "broken-truncated.epub").write_bytes(data[: len(data) // 2])
    (OUT / "broken-ok-reference.epub").unlink()

    write_epub(OUT / "broken-bad-opf.epub", {"content.opf": "<package><metadata><dc:title>Unclosed", "chapter-1.xhtml": xhtml(*few[0])})

    items = [(f"c{i}", f"chapter-{i}.xhtml", "application/xhtml+xml", "") for i in (1, 2, 3)]
    write_epub(OUT / "broken-missing-items.epub", {
        "chapter-1.xhtml": xhtml(*few[0]),
        "nav.xhtml": nav_doc([(f"chapter-{i}.xhtml", few[i - 1][0]) for i in (1, 2, 3)]),
        "content.opf": opf("Missing items (2 of 3 chapters absent)", items, ["c1", "c2", "c3"]),
    })

    bad_css = "body{color:red;;;{{{ font-size: 99999px } @media (((( } p{position:fixed!important;top:-9999px}"
    simple_book(OUT / "broken-bad-css-and-font.epub", "Bad CSS and fonts", few,
                head='<link rel="stylesheet" href="bad.css"/><style>@font-face{font-family:Broken;src:url(bad.woff2)}body{font-family:Broken}</style>',
                resources={"bad.css": (bad_css, "text/css"), "bad.woff2": (b"wOF2" + bytes(range(256)) * 4, "font/woff2")})

    simple_book(OUT / "broken-no-metadata.epub", "", few, meta=False)
    simple_book(OUT / "broken-no-toc.epub", "No table of contents", few, nav=False)


# ---------------------------------------------------------------- large books

def png(width: int, height: int, rng: random.Random) -> bytes:
    raw = b"".join(b"\0" + rng.randbytes(width * 3) for _ in range(height))

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 1)) + chunk(b"IEND", b""))


def note_books():
    """N9, N10, N11: footnote asides referenced from every paragraph (so some markers sit at
    the page foot), one very long note, a captioned image and an external link."""
    words = ("Call me Ishmael. Some years ago never mind how long precisely having little or no "
             "money in my purse, and nothing particular to interest me on shore, I thought I would "
             "sail about a little and see the watery part of the world. ").split()
    paras, notes = [], []
    for i in range(1, 25):
        text = " ".join(words[(i * 7) % len(words):] + words[:(i * 7) % len(words)])
        paras.append(f'<p>{text} <a epub:type="noteref" role="doc-noteref" id="ref-{i}" '
                     f'href="#fn-{i}">{i}</a> {text}</p>')
        body = f"Footnote {i}: a short note about paragraph {i}."
        if i == 2:
            body = " ".join(f"Long note sentence {k} that keeps going so the peek must scroll." for k in range(80))
        notes.append(f'<aside epub:type="footnote" role="doc-footnote" id="fn-{i}"><p>{body} '
                     f'<a epub:type="backlink" role="doc-backlink" href="#ref-{i}">↩</a></p></aside>')
    image = ('<figure><img src="plate.png" alt="A grey plate"/>'
             '<figcaption>Plate 1. A test image with its caption.</figcaption></figure>')
    link = '<p>See <a href="https://example.org/linen-test">example.org</a> for more.</p>'
    rng = random.Random(7)
    simple_book(OUT / "notes-and-images.epub", "Notes and images", [
        ("Notes", "<h1>Notes</h1>" + "".join(paras) + "".join(notes)),
        ("Image and link", "<h1>Image and link</h1>" + image + link),
    ], resources={"plate.png": (png(320, 200, rng), "image/png")})


def edited_books(chapters):
    """A9, B3: one book in two editions (same identifier), the second edited in known
    ways: a paragraph inserted before a passage, punctuation changed inside one, a
    passage deleted; a passage in another chapter is untouched."""
    ident = "urn:uuid:5a1e0000-0000-4000-8000-0000000a9001"
    first = [chapters[0], chapters[1], chapters[2]]
    loomings_title, loomings = chapters[0]
    marker = "<p>Call me Ishmael"
    assert marker in loomings
    revised_loomings = (
        loomings.replace(marker, "<p>A paragraph the publisher added to this edition, before everything else.</p>\n" + marker, 1)
        .replace("damp, drizzly November in my soul", "damp; drizzly November in my soul", 1)
        .replace("This is my substitute for pistol and ball. ", "", 1)
    )
    assert revised_loomings.count("pistol and ball") == 0
    revised = [(loomings_title, revised_loomings), chapters[1], chapters[2]]
    simple_book(OUT / "anchoring-first.epub", "Anchoring test", first, ident=ident)
    simple_book(OUT / "anchoring-revised.epub", "Anchoring test", revised, ident=ident)


def library_500(chapters):
    """§6.4 cold start: a 500-book library (small books; one in three with a cover)."""
    out = OUT / "library-500"
    out.mkdir(exist_ok=True)
    rng = random.Random(500)
    first = ["Anna", "Brontë", "Charles", "Dora", "Emil", "Fatima", "George", "Hana", "Ivan", "Jun"]
    last = ["Austen", "Brontë", "Dickens", "Eliot", "Hardy", "Melville", "Tolstoy", "Woolf", "Sōseki", "Hugo"]
    words = ["River", "Winter", "Lantern", "Harbour", "Orchard", "Glass", "Northern", "Silent", "Paper", "Evening"]
    for i in range(500):
        title = f"{'The ' if i % 4 == 0 else ''}{words[i % 10]} {words[(i * 7) % 10].lower()} {i + 1}"
        author = f"{first[i % 10]} {last[(i * 3) % 10]}"
        body = "".join(f"<p>{chapters[(i + k) % len(chapters)][1][:400]}</p>" for k in range(2))
        res = {"cover.png": (png(120, 180, rng), "image/png")} if i % 3 == 0 else None
        # The package names the author (simple_book does not).
        files = {"chapter-1.xhtml": xhtml(title, f"<h1>{html.escape(title)}</h1>{body}")}
        items = [("c1", "chapter-1.xhtml", "application/xhtml+xml", "")]
        if res:
            files["cover.png"] = res["cover.png"][0]
            items.append(("cover", "cover.png", "image/png", "cover-image"))
        files["nav.xhtml"] = nav_doc([("chapter-1.xhtml", title)])
        ident = "urn:uuid:" + str(uuid.uuid5(uuid.NAMESPACE_URL, f"linen-corpus:library-500:{i}"))
        package = opf(title, items, ["c1"], ident=ident)
        package = package.replace("<dc:language>", f"<dc:creator>{html.escape(author)}</dc:creator><dc:language>", 1)
        files["content.opf"] = package
        write_epub(out / f"book-{i:03d}.epub", files)


def large_books(chapters):
    rng = random.Random(20260924)
    images = {f"plate-{i}.png": (png(1800, 1800, rng), "image/png") for i in range(10)}  # ~97 MB, incompressible
    body_with_plates = [
        (t, b + (f'<p><img src="plate-{i}.png" alt="Plate {i}"/></p>' if i < 10 else ""))
        for i, (t, b) in enumerate(chapters)
    ]
    files: dict[str, bytes | str] = {}
    items, spine = [], []
    for n, (t, b) in enumerate(body_with_plates, 1):
        files[f"chapter-{n}.xhtml"] = xhtml(t, b)
        items.append((f"c{n}", f"chapter-{n}.xhtml", "application/xhtml+xml", ""))
        spine.append(f"c{n}")
    for n, (href, (data, mtype)) in enumerate(images.items()):
        files[href] = data
        items.append((f"i{n}", href, mtype, ""))
    files["nav.xhtml"] = nav_doc([(f"chapter-{i}.xhtml", t) for i, (t, _) in enumerate(chapters, 1)])
    files["content.opf"] = opf("Moby-Dick (100 MB plates)", items, spine, ident="urn:uuid:5a1e0000-0000-4000-8000-00000000b100")
    write_epub(OUT / "large-100mb.epub", files, compress_large=False)

    long_body = "".join(f"<section><h2>{html.escape(t)}</h2>{b}</section>" for t, b in chapters)
    simple_book(OUT / "long-chapter.epub", "Moby-Dick in one file", [("All chapters", long_body)],
                ident="urn:uuid:5a1e0000-0000-4000-8000-00000000c001")

    code = "\n".join(f"fn step_{i}(x: u64) -> u64 {{ x.wrapping_mul({i}).rotate_left({i % 63}) ^ 0x{i:08x} }}" for i in range(80))
    table = "<table><thead><tr><th>Key</th><th>macOS</th><th>Windows</th><th>Notes</th></tr></thead><tbody>" + "".join(
        f"<tr><td>Command {i}</td><td>⌘{chr(65 + i % 26)}</td><td>Ctrl+{chr(65 + i % 26)}</td><td>{'x' * (i % 40)}</td></tr>" for i in range(60)
    ) + "</tbody></table>"
    heavy = [(f"Section {n}", f"<p>{chapters[n][1][:600]}</p><pre><code>{html.escape(code)}</code></pre>{table}") for n in range(1, 9)]
    simple_book(OUT / "code-heavy.epub", "Code and tables (over 30%)", heavy)


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    chapters = moby_chapters()
    hostile_books()
    zip_attacks(chapters)
    xml_attacks(chapters)
    broken_books(chapters)
    large_books(chapters)
    note_books()
    edited_books(chapters)
    library_500(chapters)
    for p in sorted(OUT.iterdir()):
        if p.is_file():
            print(f"{p.stat().st_size:>12,}  {p.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
