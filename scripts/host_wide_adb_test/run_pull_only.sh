#!/usr/bin/env bash
# Run inside one lt_run_cmd HyperExecute job. Exercises the ADB pull path with
# one long-lived, pull-only collector invocation per device.
# Configure lt_run_cmd to upload fleetbench-artifacts/**.

set -euo pipefail

: "${DEVICE_SERIAL:?lt_run_cmd did not export DEVICE_SERIAL}"

# This launcher requires a release containing `fleetbench adb --direction pull`.
: "${FLEETBENCH_VERSION:?set FLEETBENCH_VERSION to a release containing --direction pull}"
PULL_ITERATIONS="${FLEETBENCH_PULL_ITERATIONS:-5000}"
TRANSFER_SIZE="${FLEETBENCH_TRANSFER_SIZE:-25B}"
ASSET="fleetbench-${FLEETBENCH_VERSION}-linux-x86_64"
BASE_URL="https://github.com/mozilla-platform-ops/fleetbench/releases/download/${FLEETBENCH_VERSION}"
ARTIFACT_DIR="${FLEETBENCH_ARTIFACT_DIR:-fleetbench-artifacts}"

case "$PULL_ITERATIONS" in
  ''|*[!0-9]*) echo "FLEETBENCH_PULL_ITERATIONS must be a positive integer, got $PULL_ITERATIONS" >&2; exit 2 ;;
esac
if [ "$PULL_ITERATIONS" -lt 1 ]; then
  echo "FLEETBENCH_PULL_ITERATIONS must be at least 1" >&2
  exit 2
fi
case "$TRANSFER_SIZE" in
  25B|50K) ;;
  *) echo "FLEETBENCH_TRANSFER_SIZE must be 25B or 50K, got $TRANSFER_SIZE" >&2; exit 2 ;;
esac

mkdir -p "$ARTIFACT_DIR"
curl --fail --location --retry 3 --output "$ASSET" "$BASE_URL/$ASSET"
curl --fail --location --retry 3 --output SHA256SUMS "$BASE_URL/SHA256SUMS"
awk -v asset="$ASSET" '$2 == asset { found = 1; print } END { exit !found }' SHA256SUMS \
  | sha256sum --check --status -
chmod +x "$ASSET"

{
  echo "phase=pull-only-overlap"
  echo "device_serial=$DEVICE_SERIAL"
  echo "fleetbench_version=$FLEETBENCH_VERSION"
  echo "direction=pull"
  echo "remote_path=/sdcard/Download"
  echo "transfer_size=$TRANSFER_SIZE"
  echo "iterations=$PULL_ITERATIONS"
  echo "collector_invocations=1"
  date -u +"started_at_utc=%Y-%m-%dT%H:%M:%SZ"
} > "$ARTIFACT_DIR/manifest.txt"

json="$ARTIFACT_DIR/fleetbench-adb-pull-only-${TRANSFER_SIZE}.json"
log="$ARTIFACT_DIR/fleetbench-adb-pull-only-${TRANSFER_SIZE}.log"
echo "Pull-only run: serial=$DEVICE_SERIAL size=$TRANSFER_SIZE iterations=$PULL_ITERATIONS"
if ! "./$ASSET" adb \
  --serial "$DEVICE_SERIAL" \
  --direction pull \
  --remote-path /sdcard/Download \
  --sizes "$TRANSFER_SIZE" \
  --iterations "$TRANSFER_SIZE=$PULL_ITERATIONS" \
  --json >"$json" 2>"$log"; then
  echo "Pull-only run failed; preserving $json and $log" >&2
  exit 1
fi

date -u +"finished_at_utc=%Y-%m-%dT%H:%M:%SZ" >> "$ARTIFACT_DIR/manifest.txt"
find "$ARTIFACT_DIR" -maxdepth 1 -type f -printf '%f\n' | sort
