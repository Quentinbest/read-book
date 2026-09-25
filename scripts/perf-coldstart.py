#!/usr/bin/env python3
"""§6.4 cold-start budgets, measured as the plan says: N launches (default 20) of
the release spikes build against the 500-book fixture; median and p95 recorded.

  library: process launch → library first paint     (budget 1 s)
  book:    process launch → first page of the last book, reopened at launch (1.5 s)

Build first: LINEN_SPIKES=1 pnpm tauri build --no-bundle --features spikes \\
             --config src-tauri/tauri.spikes.conf.json
Usage: scripts/perf-coldstart.py [runs]
Results: docs/spikes/raw/budget-coldstart.json
"""

import json
import os
import sqlite3
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / "src-tauri/target/release/linen"
BUDGETS = {"library": 1000, "book": 1500}


def p95(xs):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, round(0.95 * (len(xs) - 1)))]


def launch(data: Path, until: str) -> float:
    log = data / "startup.log"
    log.unlink(missing_ok=True)
    env = dict(os.environ, LINEN_DATA_DIR=str(data), LINEN_STARTUP_LOG=str(log),
               LINEN_EXIT_AFTER_STARTUP=until)
    env.pop("LINEN_SPIKE", None)
    t0 = time.time() * 1000
    subprocess.run([str(APP)], env=env, timeout=60, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for line in log.read_text().splitlines():
        what, at = line.split()
        if what == until:
            return float(at) - t0
    raise RuntimeError(f"no {until} mark in {log}")


def main() -> int:
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    if not APP.exists():
        print(f"No spikes build at {APP}; see the usage above.", file=sys.stderr)
        return 1
    data = Path(tempfile.mkdtemp(prefix="linen-coldstart-"))
    seed = dict(os.environ, LINEN_DATA_DIR=str(data), LINEN_SPIKE="seed500", LINEN_SPIKE_TIMEOUT="600",
                LINEN_REPO=str(ROOT))
    subprocess.run([str(APP)], env=seed, timeout=900, check=True, stdout=subprocess.DEVNULL)
    db = sqlite3.connect(data / "linen.db")
    count = db.execute("SELECT COUNT(*) FROM books").fetchone()[0]
    if count != 500:
        print(f"seeded {count} books, not 500", file=sys.stderr)
        return 1
    # The “last book”: one opened earlier, reopened at launch (Settings › General).
    db.execute("UPDATE books SET opened_at = ? WHERE id = (SELECT id FROM books ORDER BY title LIMIT 1)",
               (int(time.time() * 1000),))
    db.commit()

    results = {}
    for what in ("library", "book"):
        db.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('openAtLaunch', ?)",
                   ("book" if what == "book" else "library",))
        db.commit()
        launch(data, what)  # the first launch warms the WebView's caches, as any later one finds them
        times = [launch(data, what) for _ in range(runs)]
        results[what] = {
            "runs": runs,
            "median_ms": round(statistics.median(times)),
            "p95_ms": round(p95(times)),
            "budget_ms": BUDGETS[what],
            "pass": p95(times) < BUDGETS[what],
            "all_ms": [round(t) for t in times],
        }
        print(f"{what}: median {results[what]['median_ms']} ms, p95 {results[what]['p95_ms']} ms "
              f"(budget {BUDGETS[what]} ms) → {'PASS' if results[what]['pass'] else 'FAIL'}")
    db.close()
    out = ROOT / "docs/spikes/raw/budget-coldstart.json"
    out.write_text(json.dumps({"books": 500, **results}, indent=2) + "\n")
    return 0 if all(r["pass"] for r in results.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
