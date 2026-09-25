#!/bin/sh
# §6.4 memory budget, measured as the plan says (decision M1): N fresh launches
# (default 20) of the memory check, each with a throwaway data folder; the
# median and p95 of RSS and physical footprint must be under 400 MB.
# Usage: scripts/perf-memory.sh [runs]   (build first: LINEN_SPIKES=1 pnpm tauri build
#        --no-bundle --features spikes --config src-tauri/tauri.spikes.conf.json)
# Results: docs/spikes/raw/budget-memory-runs.json
set -e
cd "$(dirname "$0")/.."
RUNS="${1:-20}"
APP=./src-tauri/target/release/linen
OUT=$(mktemp -d)
i=1
while [ "$i" -le "$RUNS" ]; do
  DATA=$(mktemp -d)
  LINEN_DATA_DIR="$DATA" LINEN_SPIKE=m LINEN_SPIKE_TIMEOUT=900 "$APP" >"$OUT/run-$i.log" 2>&1 || true
  cp docs/spikes/raw/budget-memory.json "$OUT/run-$i.json"
  rm -rf "$DATA"
  echo "run $i/$RUNS: $(python3 -c "import json;a=json.load(open('$OUT/run-$i.json'))['raw']['after'];print(a['total_mb'],'MB RSS,',a.get('footprint_mb'),'MB footprint')")"
  i=$((i + 1))
done
python3 - "$OUT" "$RUNS" <<'PY'
import json, math, sys, statistics, datetime
out, runs = sys.argv[1], int(sys.argv[2])
rows = [json.load(open(f"{out}/run-{i}.json"))["raw"]["after"] for i in range(1, runs + 1)]
def stats(key):
    v = sorted(r[key] for r in rows)
    p95 = v[max(0, math.ceil(0.95 * len(v)) - 1)]  # nearest rank
    return {"median": statistics.median(v), "p95": p95, "min": v[0], "max": v[-1], "values": v}
rss, fp = stats("total_mb"), stats("footprint_mb")
ok = rss["p95"] < 400 and fp["p95"] < 400
result = {
    "spike": "budget-memory-runs",
    "date": datetime.date.today().isoformat(),
    "runs": runs,
    "budget_mb": 400,
    "rss_mb": rss,
    "footprint_mb": fp,
    "verdict": "pass" if ok else "fail",
}
json.dump(result, open("docs/spikes/raw/budget-memory-runs.json", "w"), indent=2)
print(f"RSS median {rss['median']} MB, p95 {rss['p95']} MB; footprint median {fp['median']} MB, p95 {fp['p95']} MB: {result['verdict'].upper()}")
PY
