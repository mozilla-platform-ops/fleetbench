#!/usr/bin/env bash
# Run inside one lt_run_cmd HyperExecute job. Runs one long-lived, push-only
# collector invocation per device for sustained-contention analysis.
# Configure lt_run_cmd to upload fleetbench-artifacts/**.

set -euo pipefail

: "${DEVICE_SERIAL:?lt_run_cmd did not export DEVICE_SERIAL}"

# This launcher requires a release containing `fleetbench adb --direction push`.
: "${FLEETBENCH_VERSION:?set FLEETBENCH_VERSION to a release containing --direction push}"
TRANSFER_SIZE="${FLEETBENCH_TRANSFER_SIZE:-25B}"
PUSH_MODE="${FLEETBENCH_PUSH_MODE:-direct}"
REQUIRE_MOZDEVICE_SU="${FLEETBENCH_REQUIRE_MOZDEVICE_SU:-0}"
PUSH_MODE_ARGS=()
ASSET="fleetbench-${FLEETBENCH_VERSION}-linux-x86_64"
BASE_URL="https://github.com/mozilla-platform-ops/fleetbench/releases/download/${FLEETBENCH_VERSION}"
ARTIFACT_DIR="${FLEETBENCH_ARTIFACT_DIR:-fleetbench-artifacts}"

case "$TRANSFER_SIZE" in
  25B|50K)
    DEFAULT_PUSH_ITERATIONS=5000
    REMOTE_PATH=/sdcard/Download
    ;;
  100M)
    # Match the established bulk workload while keeping each device's local
    # pre-generated payload set to 2 GiB and the full-contention phase bounded.
    DEFAULT_PUSH_ITERATIONS=20
    REMOTE_PATH=/data/local/tmp/
    ;;
  *) echo "FLEETBENCH_TRANSFER_SIZE must be 25B, 50K, or 100M, got $TRANSFER_SIZE" >&2; exit 2 ;;
esac
PUSH_ITERATIONS="${FLEETBENCH_PUSH_ITERATIONS:-$DEFAULT_PUSH_ITERATIONS}"
case "$PUSH_ITERATIONS" in
  ''|*[!0-9]*) echo "FLEETBENCH_PUSH_ITERATIONS must be a positive integer, got $PUSH_ITERATIONS" >&2; exit 2 ;;
esac
if [ "$PUSH_ITERATIONS" -lt 1 ]; then
  echo "FLEETBENCH_PUSH_ITERATIONS must be at least 1" >&2
  exit 2
fi
if [ "${FLEETBENCH_PUSH_MODE+x}" = x ]; then
  case "$PUSH_MODE" in
    direct|mozdevice) ;;
    *) echo "FLEETBENCH_PUSH_MODE must be direct or mozdevice, got $PUSH_MODE" >&2; exit 2 ;;
  esac
  PUSH_MODE_ARGS=(--push-mode "$PUSH_MODE")
fi
case "$REQUIRE_MOZDEVICE_SU" in
  0) ;;
  1)
    if [ "$PUSH_MODE" != "mozdevice" ]; then
      echo "FLEETBENCH_REQUIRE_MOZDEVICE_SU=1 requires FLEETBENCH_PUSH_MODE=mozdevice" >&2
      exit 2
    fi
    PUSH_MODE_ARGS+=(--require-mozdevice-su)
    ;;
  *) echo "FLEETBENCH_REQUIRE_MOZDEVICE_SU must be 0 or 1, got $REQUIRE_MOZDEVICE_SU" >&2; exit 2 ;;
esac

mkdir -p "$ARTIFACT_DIR"
curl --fail --location --retry 3 --output "$ASSET" "$BASE_URL/$ASSET"
curl --fail --location --retry 3 --output SHA256SUMS "$BASE_URL/SHA256SUMS"
awk -v asset="$ASSET" '$2 == asset { found = 1; print } END { exit !found }' SHA256SUMS \
  | sha256sum --check --status -
chmod +x "$ASSET"

{
  echo "phase=push-only-overlap"
  echo "device_serial=$DEVICE_SERIAL"
  echo "fleetbench_version=$FLEETBENCH_VERSION"
  echo "direction=push"
  echo "remote_path=$REMOTE_PATH"
  echo "transfer_size=$TRANSFER_SIZE"
  echo "push_mode=$PUSH_MODE"
  echo "require_mozdevice_su=$REQUIRE_MOZDEVICE_SU"
  echo "iterations=$PUSH_ITERATIONS"
  echo "collector_invocations=1"
  date -u +"started_at_utc=%Y-%m-%dT%H:%M:%SZ"
} > "$ARTIFACT_DIR/manifest.txt"

json="$ARTIFACT_DIR/fleetbench-adb-push-only-${PUSH_MODE}-${TRANSFER_SIZE}.json"
log="$ARTIFACT_DIR/fleetbench-adb-push-only-${PUSH_MODE}-${TRANSFER_SIZE}.log"
echo "Push-only run: serial=$DEVICE_SERIAL mode=$PUSH_MODE strict_su=$REQUIRE_MOZDEVICE_SU size=$TRANSFER_SIZE iterations=$PUSH_ITERATIONS"
if ! "./$ASSET" adb \
  --serial "$DEVICE_SERIAL" \
  --direction push \
  "${PUSH_MODE_ARGS[@]}" \
  --remote-path "$REMOTE_PATH" \
  --sizes "$TRANSFER_SIZE" \
  --iterations "$TRANSFER_SIZE=$PUSH_ITERATIONS" \
  --json >"$json" 2>"$log"; then
  echo "Push-only run failed; preserving $json and $log" >&2
  exit 1
fi

date -u +"finished_at_utc=%Y-%m-%dT%H:%M:%SZ" >> "$ARTIFACT_DIR/manifest.txt"
find "$ARTIFACT_DIR" -maxdepth 1 -type f -printf '%f\n' | sort
