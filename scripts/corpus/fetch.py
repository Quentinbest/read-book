#!/usr/bin/env python3
"""Download the public test corpus (plan §6.2) into corpus/cache/ and verify hashes.

corpus/manifest.json lists every file with its source URL and SHA-256. Downloads
are skipped when a verified copy exists. Use --pin to record hashes for entries
that have none yet (first fetch of a new entry).
"""

import hashlib
import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "corpus" / "manifest.json"
CACHE = ROOT / "corpus" / "cache"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    pin = "--pin" in sys.argv[1:]
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    CACHE.mkdir(parents=True, exist_ok=True)
    failed = 0
    for entry in manifest["files"]:
        dest = CACHE / entry["name"]
        want = entry.get("sha256")
        if dest.exists() and want and sha256(dest) == want:
            continue
        req = urllib.request.Request(entry["url"], headers={"User-Agent": "linen-corpus-fetch"})
        with urllib.request.urlopen(req, timeout=120) as r:
            dest.write_bytes(r.read())
        got = sha256(dest)
        if not want and pin:
            entry["sha256"] = got
            print(f"pinned  {entry['name']} {got}")
        elif got != want:
            print(f"MISMATCH {entry['name']}: expected {want}, got {got}")
            dest.unlink()
            failed += 1
        else:
            print(f"fetched {entry['name']}")
    if pin:
        MANIFEST.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{len(manifest['files'])} files in corpus/cache, {failed} failed.")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
