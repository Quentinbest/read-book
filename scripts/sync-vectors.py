#!/usr/bin/env python3
"""SP4's test vectors (docs/sync-protocol.md), from implementations other than
Linen's: PyNaCl (libsodium) for Ed25519, X25519 and XChaCha20-Poly1305,
argon2-cffi (the reference Argon2) for the recovery code, and Python's own
hashlib and hmac for SHA-256, HMAC and HKDF.

Writes src-tauri/tests/fixtures/sync-vectors.json, which the Rust tests in
src-tauri/src/sync/crypto.rs and envelope.rs check the code against. Run it
only when a derivation changes on purpose:

    python3 -m venv /tmp/vec && /tmp/vec/bin/pip install pynacl argon2-cffi
    /tmp/vec/bin/python scripts/sync-vectors.py
    pnpm exec prettier --write src-tauri/tests/fixtures/sync-vectors.json

SP4's exact inputs: fixed-length fields (IDs 16 bytes; hashes, keys and root 32;
nonces as stated; sequences 8 bytes big-endian) are joined as they are; a domain
string or any other variable-length field is preceded by its length as 4 bytes
big-endian. HKDF's info is the domain string's bytes, as given.

Where the protocol leaves the bytes open, this script follows the same reading as
the Rust, approved by the owner 2026-10-21 (docs/decisions.md, "SP4's exact
inputs and the envelope"): the signature input's
length-prefixed domain and type with literal 0x00 bytes (1), the version list as
one length-prefixed field (2), and the recovery code's 26 ASCII characters as a
fixed field (3). The vectors check that the Rust implements that reading.
"""

import hashlib
import hmac
import json
import pathlib

import nacl.bindings as sodium
import nacl.signing
from argon2.low_level import Type, hash_secret_raw

CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"


def h(b: bytes) -> bytes:
    return hashlib.sha256(b).digest()


def var(b: bytes) -> bytes:
    return len(b).to_bytes(4, "big") + b


def dom(s: str) -> bytes:
    return var(s.encode())


def hkdf(salt: bytes, ikm: bytes, info: str) -> bytes:
    # RFC 5869, SHA-256, 32 bytes: one block of Expand.
    prk = hmac.new(salt, ikm, hashlib.sha256).digest()
    return hmac.new(prk, info.encode() + b"\x01", hashlib.sha256).digest()


def crockford(data: bytes, nbits: int) -> str:
    n = int.from_bytes(data, "big") >> (len(data) * 8 - nbits)
    return "".join(CROCKFORD[(n >> (nbits - 5 * (i + 1))) & 31] for i in range(nbits // 5))


def b64url(b: bytes) -> str:
    import base64

    return base64.urlsafe_b64encode(b).rstrip(b"=").decode()


def pattern(n: int, start: int) -> bytes:
    return bytes((start + i) % 256 for i in range(n))


library = pattern(16, 0x10)
seed = pattern(32, 0x40)
sk = nacl.signing.SigningKey(seed)
public = bytes(sk.verify_key)
device = h(dom("linen-sync/device-id/v1") + public)[:16]

# SP5, SP6: a change file's signature.
payload = json.dumps(
    {"type": "change", "library": library.hex(), "device": device.hex(), "seq": 7},
    separators=(",", ":"),
).encode()
payload_hash = h(payload)
prev = pattern(32, 0x80)
entries = [
    h("note:7c1e".encode())[:16] + pattern(32, 0xA0),
    h("place:b00c".encode())[:16] + pattern(32, 0xC0),
]
vlist = b"".join(entries)
# decisions.md, (2): the list as one length-prefixed field.
vroot = h(dom("linen-sync/vlist/v1") + var(vlist))
seq = 7
# decisions.md, (1): domain and type length-prefixed, 0x00 literal.
message = (
    dom("linen-sync/sig/v1")
    + b"\x00"
    + var(b"change")
    + b"\x00"
    + library
    + device
    + seq.to_bytes(8, "big")
    + prev
    + vroot
    + payload_hash
)
signature = sk.sign(message).signature

# A code key's signature: the signer is zero, and a type with no seq or prev.
code_message = (
    dom("linen-sync/sig/v1")
    + b"\x00"
    + var(b"genesis")
    + b"\x00"
    + library
    + bytes(16)
    + (0).to_bytes(8, "big")
    + bytes(32)
    + bytes(32)
    + payload_hash
)

# SP9: keyed names.
naming_key = pattern(32, 0x20)
body = b'{"text":"Call me Ishmael."}'
blob_name = hmac.new(naming_key, dom("linen-sync/blob/v1") + h(body), hashlib.sha256).hexdigest()
book_file = b"PK\x03\x04 an EPUB"
book_name = hmac.new(naming_key, dom("linen-sync/book/v1") + h(book_file), hashlib.sha256).hexdigest()

# SP4: the fingerprint shown to the reader.
fpr = crockford(h(dom("linen-sync/fpr/v1") + public)[:10], 80)

# SP22: the recovery code.
code = "7K2M9QXT4HV8B1N6CZRDWJ3PAE"
assert len(code) == 26 and all(c in CROCKFORD for c in code)
# decisions.md, (3): the 26 characters as a fixed ASCII field.
check = crockford(h(dom("linen-sync/code-check/v1") + code.encode())[:2], 10)
code_nonce = pattern(16, 0x60)
salt = h(dom("linen-sync/code-salt/v1") + library + code_nonce)
root = hash_secret_raw(
    code.encode(), salt, time_cost=3, memory_cost=65536, parallelism=1, hash_len=32, type=Type.ID, version=19
)
code_seed = hkdf(b"", root, "linen-sync/code-sign/v1")
code_key = bytes(nacl.signing.SigningKey(code_seed).verify_key)
escrow_key = hkdf(b"", root, "linen-sync/escrow/v1")
head = pattern(32, 0xE0)
escrow_nonce = pattern(24, 0x30)
escrow_ad = dom("linen-sync/escrow/v1") + library + head
escrow_ct = sodium.crypto_aead_xchacha20poly1305_ietf_encrypt(naming_key, escrow_ad, escrow_nonce, escrow_key)

# SP24: enrollment by a short code.
applicant_seed = pattern(32, 0x50)
applicant_key = bytes(nacl.signing.SigningKey(applicant_seed).verify_key)
applicant = h(dom("linen-sync/device-id/v1") + applicant_key)[:16]
kx_secret = pattern(32, 0x70)
kx = sodium.crypto_scalarmult_base(kx_secret)
na = pattern(32, 0x90)
ne = pattern(32, 0xB0)
commit = h(dom("linen-sync/commit/v1") + library + applicant + applicant_key + kx + na)
request_hash = pattern(32, 0x01)
answer_hash = pattern(32, 0x02)
t = h(
    dom("linen-sync/sas/v1")
    + library
    + request_hash
    + answer_hash
    + applicant
    + applicant_key
    + kx
    + na
    + device
    + public
    + ne
)
sas = int.from_bytes(t[:8], "big") % 1_000_000
esk = pattern(32, 0xD0)
epk = sodium.crypto_scalarmult_base(esk)
shared = sodium.crypto_scalarmult(esk, kx)
seal_key = hkdf(t, shared, "linen-sync/enroll-seal/v1")
seal_nonce = pattern(24, 0x05)
seal_ad = dom("linen-sync/enroll-seal/v1") + t
seal_ct = sodium.crypto_aead_xchacha20poly1305_ietf_encrypt(naming_key, seal_ad, seal_nonce, seal_key)

vectors = {
    "note": (
        "Made by scripts/sync-vectors.py from PyNaCl, argon2-cffi and hashlib; do not edit by hand. "
        "The encodings follow the reading the owner approved 2026-10-21 "
        "(docs/decisions.md, SP4's exact inputs and the envelope)."
    ),
    "library": library.hex(),
    "seed": seed.hex(),
    "public": public.hex(),
    "device": device.hex(),
    "fingerprint": fpr,
    "signature": {
        "payload": payload.decode(),
        "payload_hash": payload_hash.hex(),
        "seq": seq,
        "prev": prev.hex(),
        "keys": ["note:7c1e", "place:b00c"],
        "version_hashes": [pattern(32, 0xA0).hex(), pattern(32, 0xC0).hex()],
        "vroot": vroot.hex(),
        "message": message.hex(),
        "sig": b64url(signature),
        "code_message": code_message.hex(),
    },
    "names": {
        "naming_key": naming_key.hex(),
        "body": body.decode(),
        "blob": blob_name,
        "book_file": book_file.hex(),
        "book": book_name,
    },
    "code": {
        "chars": code,
        "check": check,
        "nonce": code_nonce.hex(),
        "salt": salt.hex(),
        "root": root.hex(),
        "sign_seed": code_seed.hex(),
        "code_key": code_key.hex(),
        "escrow_key": escrow_key.hex(),
        "head": head.hex(),
        "escrow_nonce": escrow_nonce.hex(),
        "escrow_ct": escrow_ct.hex(),
    },
    "enroll": {
        "applicant_seed": applicant_seed.hex(),
        "applicant_key": applicant_key.hex(),
        "applicant": applicant.hex(),
        "kx_secret": kx_secret.hex(),
        "kx": kx.hex(),
        "na": na.hex(),
        "ne": ne.hex(),
        "commit": commit.hex(),
        "request_hash": request_hash.hex(),
        "answer_hash": answer_hash.hex(),
        "t": t.hex(),
        "sas": sas,
        "esk": esk.hex(),
        "epk": epk.hex(),
        "shared": shared.hex(),
        "seal_key": seal_key.hex(),
        "seal_nonce": seal_nonce.hex(),
        "seal_ct": seal_ct.hex(),
    },
}

out = pathlib.Path(__file__).resolve().parent.parent / "src-tauri/tests/fixtures/sync-vectors.json"
out.write_text(json.dumps(vectors, indent=2) + "\n")
print(f"wrote {out}")
