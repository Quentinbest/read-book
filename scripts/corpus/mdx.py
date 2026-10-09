"""Write MDX and MDD dictionaries for Linen's tests (Reading Lens Stage 2b, DX3, DX12).

New code from the published format notes; no licensed dictionary and no GPL reader is
used. It writes what Linen must read (MDX/MDD 1.2 and 2.0, `Encrypted` 0 and 2, zlib or
uncompressed blocks, UTF-8, UTF-16LE, GBK, GB18030 and Big5) and what it must refuse
(registration-protected files, LZO blocks, truncated files, caps exceeded).

    python3 scripts/corpus/mdx.py --fixtures src-tauri/tests/fixtures/mdx

writes the small set the Rust tests use; generate.py writes the large ones into the corpus.
"""

import struct
import sys
import zlib
from pathlib import Path

# ---------------------------------------------------------------- RIPEMD-128

_R = (list(range(16))
      + [7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9, 5, 2, 14, 11, 8]
      + [3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12]
      + [1, 9, 11, 10, 0, 8, 12, 4, 13, 3, 7, 15, 14, 5, 6, 2])
_R2 = ([5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12]
       + [6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8, 12, 4, 9, 1, 2]
       + [15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13]
       + [8, 6, 4, 1, 3, 11, 15, 0, 5, 12, 2, 13, 9, 7, 10, 14])
_S = ([11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8]
      + [7, 6, 8, 13, 11, 9, 7, 15, 7, 12, 15, 9, 11, 7, 13, 12]
      + [11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5]
      + [11, 12, 14, 15, 14, 15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12])
_S2 = ([8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6]
       + [9, 13, 15, 7, 12, 8, 9, 11, 7, 7, 12, 7, 6, 15, 13, 11]
       + [9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5]
       + [15, 5, 8, 11, 14, 14, 6, 14, 6, 9, 12, 9, 12, 5, 15, 8])
_K = [0x00000000, 0x5A827999, 0x6ED9EBA1, 0x8F1BBCDC]
_K2 = [0x50A28BE6, 0x5C4DD124, 0x6D703EF3, 0x00000000]
_M = 0xFFFFFFFF


def _f(j, x, y, z):
    if j < 16:
        return x ^ y ^ z
    if j < 32:
        return (x & y) | (~x & z)
    if j < 48:
        return (x | ~y) ^ z
    return (x & z) | (y & ~z)


def _rol(x, n):
    x &= _M
    return ((x << n) | (x >> (32 - n))) & _M


def ripemd128(data: bytes) -> bytes:
    h = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476]
    msg = data + b'\x80' + b'\x00' * ((55 - len(data)) % 64) + struct.pack('<Q', len(data) * 8)
    for off in range(0, len(msg), 64):
        x = struct.unpack('<16I', msg[off:off + 64])
        a, b, c, d = h
        a2, b2, c2, d2 = h
        for j in range(64):
            t = _rol(a + (_f(j, b, c, d) & _M) + x[_R[j]] + _K[j // 16], _S[j])
            a, d, c, b = d, c, b, t
            t = _rol(a2 + (_f(63 - j, b2, c2, d2) & _M) + x[_R2[j]] + _K2[j // 16], _S2[j])
            a2, d2, c2, b2 = d2, c2, b2, t
        t = (h[1] + c + d2) & _M
        h[1] = (h[2] + d + a2) & _M
        h[2] = (h[3] + a + b2) & _M
        h[3] = (h[0] + b + c2) & _M
        h[0] = t
    return struct.pack('<4I', *h)


assert ripemd128(b'').hex() == 'cdf26213a150dc3ecb610f18f6b38b46'
assert ripemd128(b'abc').hex() == 'c14a12199c66e4ba84636b0f69144c77'

# ---------------------------------------------------------------- blocks


def _adler(b: bytes) -> int:
    return zlib.adler32(b) & 0xFFFFFFFF


def block(data: bytes, kind: int = 2) -> bytes:
    """A compressed block: type (LE u32), adler32 of the plain data (BE u32), payload."""
    if kind == 0:
        payload = data
    elif kind == 2:
        payload = zlib.compress(data, 9)
    else:  # 1: LZO, which Linen refuses; the payload is never read
        payload = data
    return struct.pack('<I', kind) + struct.pack('>I', _adler(data)) + payload


def encrypt_key_index(comp: bytes) -> bytes:
    """`Encrypted=2`: the key index's payload, keyed by its own checksum."""
    key = ripemd128(comp[4:8] + struct.pack('<I', 0x3695))
    out = bytearray(comp[8:])
    previous = 0x36
    for i, p in enumerate(comp[8:]):
        t = p ^ previous ^ (i & 0xFF) ^ key[i % len(key)]
        c = ((t >> 4) | (t << 4)) & 0xFF
        out[i] = c
        previous = c
    return comp[:8] + bytes(out)


# ---------------------------------------------------------------- writer

CODECS = {'UTF-8': 'utf-8', 'UTF-16': 'utf-16-le', 'GBK': 'gbk', 'GB18030': 'gb18030', 'BIG5': 'big5'}


def write(path, entries, *, version='2.0', encrypted=0, encoding='UTF-8', kind=2,
          mdd=False, keys_per_block=4, records_per_block=4, title='Test', extra_header='',
          header_attrs=None):
    """entries: [(key, value)], value str for MDX (encoded with `encoding`) or bytes for MDD."""
    v2 = version.startswith('2')
    num = '>Q' if v2 else '>I'
    codec = 'utf-16-le' if mdd else CODECS[encoding]
    wide = codec == 'utf-16-le'
    term = b'\x00\x00' if wide else b'\x00'
    entries = sorted(entries, key=lambda e: e[0].lower())

    # Records, with each key's offset in the concatenated plain record data.
    records, offsets, at = [], [], 0
    for _, value in entries:
        data = value if isinstance(value, bytes) else value.encode(CODECS[encoding]) + term
        offsets.append(at)
        records.append(data)
        at += len(data)

    attrs = {
        'GeneratedByEngineVersion': version, 'RequiredEngineVersion': version,
        'Encrypted': str(encrypted), 'Encoding': '' if mdd else encoding,
        'Format': 'Html', 'Stripkey': 'Yes', 'KeyCaseSensitive': 'No',
        'Title': title, 'Description': 'Generated for Linen tests',
        'CreationDate': '2026-10-09', 'Compact': 'Yes', 'Compat': 'Yes', 'Left2Right': 'Yes',
        'DataSourceFormat': '106', 'StyleSheet': '', 'RegisterBy': '',
    }
    attrs.update(header_attrs or {})
    xml = '<Dictionary ' + ' '.join(f'{k}="{v}"' for k, v in attrs.items()) + extra_header + '/>\r\n'
    header = xml.encode('utf-16-le') + b'\x00\x00'
    out = bytearray(struct.pack('>I', len(header)) + header + struct.pack('<I', _adler(header)))

    # Key blocks and their index.
    key_blocks, index = [], bytearray()
    for start in range(0, len(entries), keys_per_block):
        chunk = list(zip(entries[start:start + keys_per_block], offsets[start:start + keys_per_block]))
        plain = b''.join(struct.pack(num, off) + k.encode(codec) + term for (k, _), off in chunk)
        comp = block(plain, kind)
        key_blocks.append(comp)
        first, last = chunk[0][0][0].encode(codec), chunk[-1][0][0].encode(codec)
        size = (lambda b: len(b) // 2) if wide else len
        index += struct.pack(num, len(chunk))
        for k in (first, last):
            if v2:
                index += struct.pack('>H', size(k)) + k + term
            else:
                index += struct.pack('>B', size(k)) + k
        index += struct.pack(num, len(comp)) + struct.pack(num, len(plain))
    key_blob = b''.join(key_blocks)
    if v2:
        index_block = block(bytes(index), 2 if kind != 0 else 0)
        if encrypted & 2:
            index_block = encrypt_key_index(index_block)
        head = struct.pack('>5Q', len(key_blocks), len(entries), len(index), len(index_block), len(key_blob))
        out += head + struct.pack('>I', _adler(head)) + index_block
    else:
        index_block = bytes(index)
        head = struct.pack('>4I', len(key_blocks), len(entries), len(index_block), len(key_blob))
        out += head + index_block
    out += key_blob

    # Record blocks and their index.
    rec_blocks, rec_index = [], bytearray()
    for start in range(0, len(records), records_per_block):
        plain = b''.join(records[start:start + records_per_block])
        comp = block(plain, kind)
        rec_blocks.append(comp)
        rec_index += struct.pack(num, len(comp)) + struct.pack(num, len(plain))
    rec_blob = b''.join(rec_blocks)
    n = struct.pack('>4Q' if v2 else '>4I', len(rec_blocks), len(entries), len(rec_index), len(rec_blob))
    out += n + rec_index + rec_blob
    Path(path).write_bytes(bytes(out))
    return bytes(out)


# ---------------------------------------------------------------- the fixtures

PNG_1x1 = bytes.fromhex(
    '89504e470d0a1a0a0000000d4948445200000001000000010806000000'
    '1f15c4890000000d4944415478da63f8cfc0f01f0005050201a5b2a8c60000000049454e44ae426082')
CSS = b'.hw { font-weight: bold; font-size: 26px; color: #1a1a6e } .pos { font-style: italic }'


def png(w, h, rgb):
    """A solid PNG: big enough for a capture to find (Spike I)."""
    row = b'\x00' + bytes(rgb) * w
    raw = zlib.compress(row * h, 9)

    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xFFFFFFFF)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', raw) + chunk(b'IEND', b''))


RED = png(24, 24, (220, 30, 30))

ENTRIES = [
    ('invalidate', '<link rel="stylesheet" href="test.css"><div class="hw">in·val·i·date</div>'
                   '<span class="pos">verb</span><ol><li>to show that an argument or claim is wrong '
                   '<span lang="zh">证明……不成立</span></li></ol><img src="dot.png" alt=""><img src="red.png" alt="">'),
    ('Invalidate', '<div class="hw">Invalidate</div><p>A second entry with the same folded headword.</p>'),
    ('cache', '<p style="margin:0"><a href="entry://invalidate" style="display:block;height:44px;line-height:44px">'
              'see invalidate</a></p><div class="hw">cache</div><p>a store of data kept close at hand</p>'),
    ('caches', '@@@LINK=cache'),
    ('cycle-a', '@@@LINK=cycle-b'),
    ('cycle-b', '@@@LINK=cycle-a'),
    ('spleen', '<link rel="stylesheet" href="test.css"><p style="margin:0"><a href="entry://cache" style="display:block;height:44px;line-height:44px">'
               'see cache</a></p><div class="hw">spleen</div><p>bad temper or spite</p>'
               '<img src="red.png" alt="" style="width:48px;height:48px">'),
    ('news', '<div class="hw">news</div><p>new information</p>'),
    ('data', '<div class="hw">data</div><p>facts and statistics</p>'),
    ('it’s', '<div class="hw">it’s</div><p>it is</p>'),
    ('café', '<div class="hw">café</div><p>a small restaurant</p>'),
    ('缓存', '<div class="hw">缓存</div><p>高速缓冲存储器</p>'),
    ('pronounce', '<div class="hw">pronounce</div><a href="sound://pronounce.mp3">▶</a>'),
]
MDD_ENTRIES = [('\\test.css', CSS), ('\\dot.png', PNG_1x1), ('\\red.png', RED)]

HOSTILE = [
    ('script', '<p>before</p><script>fetch("http://127.0.0.1:8765/canary/script")</script><p>after</p>'),
    ('handler', '<p onclick="alert(1)" onmouseover="x()">click</p><img src=x onerror="fetch(\'http://127.0.0.1:8765/canary/onerror\')">'),
    ('javascript', '<a href="javascript:alert(1)">js</a><a href="http://127.0.0.1:8765/canary/web">web</a>'),
    ('base', '<base href="http://127.0.0.1:8765/canary/base/"><img src="a.png">'),
    ('refresh', '<meta http-equiv="refresh" content="0;url=http://127.0.0.1:8765/canary/refresh">'),
    ('prefetch', '<link rel="prefetch" href="http://127.0.0.1:8765/canary/prefetch"><link rel="preload" href="http://127.0.0.1:8765/canary/preload">'),
    ('remote-img', '<img src="http://127.0.0.1:8765/canary/img"><img srcset="http://127.0.0.1:8765/canary/srcset 2x">'),
    ('remote-css', '<p style="background:url(http://127.0.0.1:8765/canary/style)">s</p>'
                   '<style>@import "http://127.0.0.1:8765/canary/import"; '
                   'p { background: u\\72 l(http://127.0.0.1:8765/canary/escaped) } '
                   'div { background-image: image-set("http://127.0.0.1:8765/canary/imageset" 1x) } '
                   '@font-face { font-family: x; src: url(http://127.0.0.1:8765/canary/font) }</style>'),
    ('frames', '<iframe src="http://127.0.0.1:8765/canary/frame"></iframe><object data="x"></object><form action="http://127.0.0.1:8765/canary/form"><input></form>'),
    ('traversal', '<img src="../../../etc/passwd"><img src="%2e%2e/%2e%2e/secret.png"><link rel="stylesheet" href="/abs.css">'),
    ('svg', '<svg><script>fetch("http://127.0.0.1:8765/canary/svg")</script><circle r="2"/></svg>'),
]


def fixtures(out: Path, prefix: str = ''):
    out.mkdir(parents=True, exist_ok=True)
    pre = prefix
    # DX1: an MDX 2.0, Encrypted=2, with its MDD (split: .mdd and .1.mdd).
    write(out / f'{pre}basic.mdx', ENTRIES, encrypted=2, title='Basic Test Dictionary')
    write(out / f'{pre}basic.mdd', MDD_ENTRIES[:1], mdd=True, encrypted=2)
    write(out / f'{pre}basic.1.mdd', MDD_ENTRIES[1:], mdd=True, encrypted=2)
    # DX3: MDX 1.2, uncompressed blocks, and each encoding.
    write(out / f'{pre}v1.mdx', ENTRIES, version='1.2', title='Version 1.2')
    write(out / f'{pre}stored.mdx', ENTRIES, kind=0, title='Uncompressed')
    for enc in ['UTF-16', 'GBK', 'GB18030', 'BIG5']:
        # Big5 has no 缓存 (simplified): the CJK entry is traditional there.
        entries = ENTRIES if enc != 'BIG5' else [
            (('緩存' if k == '缓存' else k),
             v.replace('缓存', '緩存').replace('高速缓冲存储器', '高速緩衝儲存器')
              .replace('证明……不成立', '證明……不成立'))
            for k, v in ENTRIES]
        # Keys an encoding cannot hold (é in Big5) are left out of its file.
        codec = CODECS[enc]

        def fits(text, codec=codec):
            try:
                text.encode(codec)
                return True
            except UnicodeEncodeError:
                return False
        entries = [(k, v) for k, v in entries if fits(k) and fits(v)]
        write(out / f'{pre}enc-{enc.lower().replace("-", "")}.mdx', entries, encoding=enc, title=f'Encoding {enc}')
    # Refused before activation (DX3).
    write(out / f'{pre}registered.mdx', ENTRIES[:2], encrypted=1, title='Registered')
    write(out / f'{pre}lzo.mdx', ENTRIES[:2], kind=1, title='LZO')
    write(out / f'{pre}unknown-encoding.mdx', ENTRIES[:2], header_attrs={'Encoding': 'KOI8-R'}, title='KOI8')
    data = write(out / f'{pre}whole.mdx', ENTRIES)
    (out / f'{pre}truncated.mdx').write_bytes(data[: len(data) * 2 // 3])
    (out / f'{pre}whole.mdx').unlink()
    # DX13: a block that decompresses past 16 MB (a bomb), and an entry over 4 MB.
    write(out / f'{pre}bomb.mdx', [('bomb', 'a' * (17 * 1024 * 1024))], records_per_block=1, title='Bomb')
    write(out / f'{pre}big-entry.mdx', [('big', 'b' * (4 * 1024 * 1024 + 10)), ('small', 'ok')],
          records_per_block=1, title='Big entry')
    # DX13: a key count past what the file holds.
    data = bytearray(write(out / f'{pre}counts.mdx', ENTRIES[:2], title='Counts'))
    hlen = struct.unpack('>I', data[:4])[0]
    at = 4 + hlen + 4
    data[at + 8:at + 16] = struct.pack('>Q', 10 ** 12)  # num_entries
    head = bytes(data[at:at + 40])
    data[at + 40:at + 44] = struct.pack('>I', _adler(head))
    (out / f'{pre}counts.mdx').write_bytes(bytes(data))
    # DX6: hostile entries (the sanitiser's set); every remote URL is the canary.
    write(out / f'{pre}hostile.mdx', HOSTILE, title='Hostile')



def corpus(out: Path):
    """The corpus set for the in-app checks: the fixtures (dict-*), a newer version of
    the basic dictionary under the same name (DX2), and a large one (DX10)."""
    fixtures(out, 'dict-')
    newer = [(k, v.replace('to show that an argument or claim is wrong', 'NEWER: to show a claim is wrong'))
             for k, v in ENTRIES]
    (out / 'dict-v2').mkdir(exist_ok=True)
    write(out / 'dict-v2' / 'dict-basic.mdx', newer, encrypted=2, title='Basic Test Dictionary')
    write(out / 'dict-v2' / 'dict-basic.mdd', MDD_ENTRIES[:1], mdd=True, encrypted=2)
    write(out / 'dict-v2' / 'dict-basic.1.mdd', MDD_ENTRIES[1:], mdd=True, encrypted=2)
    words = [f'term{i:06d}' for i in range(60_000)]
    write(out / 'dict-large.mdx', [(w, f'<div class="hw">{w}</div><p>Entry {w} of a large test dictionary.</p>')
                                   for w in words], keys_per_block=512, records_per_block=512, title='Large')


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--fixtures':
        fixtures(Path(sys.argv[2]))
        for p in sorted(Path(sys.argv[2]).iterdir()):
            print(f'{p.stat().st_size:>10,}  {p.name}')
    else:
        print(__doc__)
