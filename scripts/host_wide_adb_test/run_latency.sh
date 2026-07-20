#!/usr/bin/env bash
# Run inside one lt_run_cmd HyperExecute job. This is the production-path
# 25-byte and 50 KiB latency phase. Configure lt_run_cmd to upload
# fleetbench-artifacts/**.

set -euo pipefail

: "${DEVICE_SERIAL:?lt_run_cmd did not export DEVICE_SERIAL}"

VERSION="${FLEETBENCH_VERSION:-v0.4.1}"
RUNS="${FLEETBENCH_RUNS:-3}"
ASSET="fleetbench-${VERSION}-linux-x86_64"
BASE_URL="https://github.com/mozilla-platform-ops/fleetbench/releases/download/${VERSION}"
ARTIFACT_DIR="${FLEETBENCH_ARTIFACT_DIR:-fleetbench-artifacts}"

case "$RUNS" in
  ''|*[!0-9]*) echo "FLEETBENCH_RUNS must be a positive integer, got $RUNS" >&2; exit 2 ;;
esac
if [ "$RUNS" -lt 1 ]; then
  echo "FLEETBENCH_RUNS must be at least 1" >&2
  exit 2
fi

mkdir -p "$ARTIFACT_DIR"
curl --fail --location --retry 3 --output "$ASSET" "$BASE_URL/$ASSET"
curl --fail --location --retry 3 --output SHA256SUMS "$BASE_URL/SHA256SUMS"
awk -v asset="$ASSET" '$2 == asset { found = 1; print } END { exit !found }' SHA256SUMS \
  | sha256sum --check --status -
chmod +x "$ASSET"

{
  echo "phase=production-latency"
  echo "device_serial=$DEVICE_SERIAL"
  echo "fleetbench_version=$VERSION"
  echo "runs=$RUNS"
  date -u +"started_at_utc=%Y-%m-%dT%H:%M:%SZ"
} > "$ARTIFACT_DIR/manifest.txt"

for run in $(seq -w 1 "$RUNS"); do
  json="$ARTIFACT_DIR/fleetbench-adb-latency-${run}.json"
  log="$ARTIFACT_DIR/fleetbench-adb-latency-${run}.log"
  echo "latency run $run/$RUNS: serial=$DEVICE_SERIAL"
  if ! "./$ASSET" adb \
    --serial "$DEVICE_SERIAL" \
    --remote-path /sdcard/Download \
    --sizes 25B,50K \
    --iterations 25B=200,50K=200 \
    --json >"$json" 2>"$log"; then
    echo "latency run $run failed; preserving $json and $log" >&2
    exit 1
  fi
done

date -u +"finished_at_utc=%Y-%m-%dT%H:%M:%SZ" >> "$ARTIFACT_DIR/manifest.txt"
find "$ARTIFACT_DIR" -maxdepth 1 -type f -printf '%f\n' | sort
