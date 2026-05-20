#!/usr/bin/env bash
# Smoke test for the fleetbench collector.
#
# Two modes:
#   ./smoke.sh                       Build from source (cargo build --release).
#   ./smoke.sh --binary <path>       Use a pre-built binary (e.g. scp'd musl static).

set -euo pipefail
cd "$(dirname "$0")"

BIN=""
if [[ "${1:-}" == "--binary" ]]; then
    BIN="${2:?path to binary required after --binary}"
else
    echo "=== build (release) ==="
    cargo build --release
    BIN=./target/release/fleetbench
fi

echo
echo "=== inspect (human) ==="
"$BIN" inspect

echo
echo "=== inspect --json ==="
"$BIN" inspect --json

echo
echo "=== cpu --mode quick (human, timed) ==="
time "$BIN" cpu --mode quick

echo
echo "=== cpu --mode quick --json (validating shape) ==="
"$BIN" cpu --mode quick --json > /tmp/fleetbench_smoke.json
python3 - <<'PY'
import json
with open("/tmp/fleetbench_smoke.json") as f:
    d = json.load(f)
required_top = ["schema_version", "collector_version", "cpu_suite_version",
                "timestamp_utc", "status", "host", "cpu", "config",
                "environment", "results"]
missing = [k for k in required_top if k not in d]
assert not missing, f"missing top-level keys: {missing}"
assert d["schema_version"] == 2, f"schema_version={d['schema_version']}"
assert d["cpu_suite_version"] == "cpu-v0"
assert d["status"] == "ok"
env = d["environment"]
for slot in ("load_pre_warmup", "load_pre_timed", "load_post_timed"):
    assert slot in env, f"missing env slot: {slot}"
    s = env[slot]
    assert s["cpu_percent"] is not None, f"cpu_percent null in {slot}"
    assert s["load_1"] is not None, f"load_1 null in {slot}"
results = d["results"]
assert "prime_sieve_1t" in results and "prime_sieve_mt" in results
for name in ("prime_sieve_1t", "prime_sieve_mt"):
    r = results[name]
    assert len(r["iterations"]) == d["config"]["iterations"], f"{name} iteration count mismatch"
    counts = {it["prime_count"] for it in r["iterations"]}
    assert counts == {664579}, f"{name} prime_count: {counts}"
print(f"schema OK: {len(results['prime_sieve_1t']['iterations'])} iterations, "
      f"prime_count={list(counts)[0]}, threads={results['prime_sieve_mt']['threads']}")
PY

echo
echo "=== cpu --mode normal (timed; expect ~10s on slow x86) ==="
time "$BIN" cpu --mode normal > /tmp/fleetbench_normal.json

echo
echo "=== failure path: bad --threads with --json ==="
"$BIN" cpu --threads 0 --json && rc=0 || rc=$?
echo "exit=$rc"
[[ "$rc" == "1" ]] || { echo "expected exit 1, got $rc"; exit 1; }

echo
echo "=== ALL SMOKE CHECKS PASSED ==="
