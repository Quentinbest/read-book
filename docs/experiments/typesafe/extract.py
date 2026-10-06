"""Pull what the experiments need out of each EPUB: metadata (with file-as),
the spine with headings and opening text, and the publisher's bodymatter landmark."""
import json, re, sys, zipfile, posixpath, glob, os
from urllib.parse import unquote
import xml.etree.ElementTree as ET

NS = {
    'opf': 'http://www.idpf.org/2007/opf',
    'dc': 'http://purl.org/dc/elements/1.1/',
    'c': 'urn:oasis:names:tc:opendocument:xmlns:container',
    'x': 'http://www.w3.org/1999/xhtml',
    'ops': 'http://www.idpf.org/2007/ops',
}
EPUB_TYPE = '{http://www.idpf.org/2007/ops}type'
OPF_FILE_AS = '{http://www.idpf.org/2007/opf}file-as'


def parse(z, name):
    data = z.read(name)
    data = re.sub(rb'<!DOCTYPE[^>]*>', b'', data, count=1)
    return ET.fromstring(data)


def text_of(el):
    return re.sub(r'\s+', ' ', ''.join(el.itertext())).strip()


def doc_info(z, name):
    try:
        root = parse(z, name)
    except Exception:
        return {'heading': None, 'title_el': None, 'snippet': '', 'headings': []}
    body = root.find('.//x:body', NS)
    headings = []
    for el in root.iter():
        tag = el.tag.split('}')[-1]
        if tag in ('h1', 'h2', 'h3', 'h4'):
            t = text_of(el)
            if t:
                headings.append(t)
    title_el = root.find('.//x:head/x:title', NS)
    snippet = text_of(body)[:400] if body is not None else ''
    return {
        'heading': headings[0] if headings else None,
        'headings': headings[:4],
        'title_el': text_of(title_el) if title_el is not None else None,
        'snippet': snippet,
    }


def extract(path):
    z = zipfile.ZipFile(path)
    rootfile = parse(z, 'META-INF/container.xml').find('.//c:rootfile', NS).get('full-path')
    base = posixpath.dirname(rootfile)
    opf = parse(z, rootfile)
    md = opf.find('opf:metadata', NS)
    refines = {}
    for m in md.findall('opf:meta', NS):
        if m.get('property') == 'file-as' and m.get('refines'):
            refines[m.get('refines').lstrip('#')] = (m.text or '').strip()
    creators = []
    for c in md.findall('dc:creator', NS):
        name = (c.text or '').strip()
        fa = c.get(OPF_FILE_AS) or refines.get(c.get('id') or '')
        role = c.get('{http://www.idpf.org/2007/opf}role')
        creators.append({'name': name, 'file_as': fa, 'role': role})
    title = next((t.text.strip() for t in md.findall('dc:title', NS) if t.text and t.text.strip()), None)
    lang = next((t.text.strip() for t in md.findall('dc:language', NS) if t.text), None)

    manifest = {}
    nav_href = None
    for it in opf.find('opf:manifest', NS):
        href = posixpath.normpath(posixpath.join(base, unquote(it.get('href'))))
        manifest[it.get('id')] = href
        if 'nav' in (it.get('properties') or '').split():
            nav_href = href
    spine = []
    for ir in opf.find('opf:spine', NS):
        href = manifest.get(ir.get('idref'))
        if not href:
            continue
        spine.append({'href': href, 'linear': ir.get('linear', 'yes'), **doc_info(z, href)})

    # The publisher's bodymatter: EPUB 3 landmarks, else the EPUB 2 guide.
    body_href = None
    if nav_href:
        try:
            nav = parse(z, nav_href)
            for n in nav.iter('{http://www.w3.org/1999/xhtml}nav'):
                if 'landmarks' in (n.get(EPUB_TYPE) or ''):
                    for a in n.iter('{http://www.w3.org/1999/xhtml}a'):
                        if 'bodymatter' in (a.get(EPUB_TYPE) or '').split():
                            h = unquote(a.get('href').split('#')[0])
                            body_href = posixpath.normpath(posixpath.join(posixpath.dirname(nav_href), h))
        except Exception:
            pass
    if not body_href:
        guide = opf.find('opf:guide', NS)
        if guide is not None:
            for r in guide:
                if r.get('type') in ('text', 'bodymatter'):
                    body_href = posixpath.normpath(posixpath.join(base, unquote(r.get('href').split('#')[0])))
                    break
    body_index = next((i for i, s in enumerate(spine) if s['href'] == body_href), None)
    return {
        'file': os.path.basename(path),
        'title': title,
        'language': lang,
        'creators': creators,
        'spine': spine,
        'body_index': body_index,
    }


if __name__ == '__main__':
    out = []
    for p in sorted(sys.argv[1:]):
        try:
            out.append(extract(p))
        except Exception as e:
            print('skip', p, e, file=sys.stderr)
    json.dump(out, sys.stdout, ensure_ascii=False)
