#!/usr/bin/env bash
# Compare the literal Sparky mozdevice probe with Fleetbench's Rust
# --push-mode mozdevice implementation on one locally attached phone.
# Produces per-block artifacts plus summary.json; it does not use lt_run_cmd.

set -euo pipefail

: "${DEVICE_SERIAL:?set DEVICE_SERIAL to the locally attached phone bare USB serial}"

ROUNDS="${FLEETBENCH_INTERLEAVE_ROUNDS:-4}"
ITERATIONS="${FLEETBENCH_INTERLEAVE_ITERATIONS:-50}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SPARKY_RUNNER="$SCRIPT_DIR/run_sparky_mozdevice_exact.sh"
COLLECTOR_DIR="$REPO_ROOT/collector"
ARTIFACT_DIR="${FLEETBENCH_INTERLEAVE_ARTIFACT_DIR:-$PWD/fleetbench-mozdevice-interleave-$(date -u +%Y%m%dT%H%M%SZ)}"

for value in "$ROUNDS" "$ITERATIONS"; do
  case "$value" in
    ''|*[!0-9]*) echo "rounds and iterations must be positive integers" >&2; exit 2 ;;
  esac
done
if [ "$ROUNDS" -lt 2 ] || [ $((ROUNDS % 2)) -ne 0 ]; then
  echo "FLEETBENCH_INTERLEAVE_ROUNDS must be an even integer of at least 2" >&2
  exit 2
fi
if [ "$ITERATIONS" -lt 1 ]; then
  echo "FLEETBENCH_INTERLEAVE_ITERATIONS must be at least 1" >&2
  exit 2
fi
case "$DEVICE_SERIAL" in
  *:*) echo "DEVICE_SERIAL must be a bare USB serial, got $DEVICE_SERIAL" >&2; exit 2 ;;
esac
if [ -e "$ARTIFACT_DIR" ]; then
  echo "refusing to overwrite existing artifact directory: $ARTIFACT_DIR" >&2
  exit 2
fi

for command in adb cargo jq; do
  command -v "$command" >/dev/null || {
    echo "required command not found: $command" >&2
    exit 2
  }
done
adb -s "$DEVICE_SERIAL" get-state | grep -qx device || {
  echo "ADB device is not ready: $DEVICE_SERIAL" >&2
  exit 2
}

mkdir -p "$ARTIFACT_DIR"
{
  echo "phase=local-mozdevice-interleave"
  echo "device_serial=$DEVICE_SERIAL"
  echo "rounds=$ROUNDS"
  echo "iterations_per_block=$ITERATIONS"
  echo "order=odd:python-then-fleetbench;even:fleetbench-then-python"
  echo "fleetbench_git_sha=$(git -C "$REPO_ROOT" rev-parse HEAD)"
  date -u +"started_at_utc=%Y-%m-%dT%H:%M:%SZ"
} > "$ARTIFACT_DIR/manifest.txt"

capture_command() {
  local label="$1"
  shift
  printf '\n## %s\n' "$label"
  "$@" 2>&1 || printf '[unavailable: exit %s]\n' "$?"
}

capture_block_state() {
  local round="$1"
  local implementation="$2"
  local point="$3"
  local output_dir="$4"
  {
    date -u +"captured_at_utc=%Y-%m-%dT%H:%M:%SZ"
    echo "round=$round"
    echo "implementation=$implementation"
    echo "point=$point"
    capture_command "adb get-state" adb -s "$DEVICE_SERIAL" get-state
    capture_command "device properties" adb -s "$DEVICE_SERIAL" shell getprop ro.build.fingerprint
    capture_command "device battery" adb -s "$DEVICE_SERIAL" shell dumpsys battery
    capture_command "device thermal service" adb -s "$DEVICE_SERIAL" shell dumpsys thermalservice
    capture_command "host uptime" uptime
    if command -v pmset >/dev/null; then
      capture_command "host power" pmset -g batt
      capture_command "host thermal settings" pmset -g therm
    fi
  } > "$output_dir/state-${point}.txt"
}

run_python() {
  local round="$1"
  local output_dir="$ARTIFACT_DIR/round-${round}-python"
  mkdir -p "$output_dir"
  echo "round $round: literal Python mozdevice ($ITERATIONS iterations)"
  capture_block_state "$round" python before "$output_dir"
  DEVICE_SERIAL="$DEVICE_SERIAL" \
    FLEETBENCH_ADB_LATENCY_ITERATIONS="$ITERATIONS" \
    FLEETBENCH_ADB_LATENCY_REMOTE_DIR=/sdcard/Download \
    FLEETBENCH_ARTIFACT_DIR="$output_dir" \
    FLEETBENCH_MOZDEVICE_PHASE_TIMINGS_PATH="$output_dir/mozdevice-phase-timings.json" \
    "$SPARKY_RUNNER" >"$output_dir/launcher.stdout" 2>"$output_dir/launcher.stderr"
  capture_block_state "$round" python after "$output_dir"
}

run_fleetbench() {
  local round="$1"
  local output_dir="$ARTIFACT_DIR/round-${round}-fleetbench"
  mkdir -p "$output_dir"
  echo "round $round: Fleetbench Rust mozdevice ($ITERATIONS iterations)"
  capture_block_state "$round" fleetbench before "$output_dir"
  (
    cd "$COLLECTOR_DIR"
    cargo run --quiet -- adb \
      --serial "$DEVICE_SERIAL" \
      --direction push \
      --push-mode mozdevice \
      --remote-path /sdcard/Download \
      --sizes 25B \
      --iterations "25B=$ITERATIONS" \
      --json
  ) >"$output_dir/fleetbench.json" 2>"$output_dir/fleetbench.log"
  capture_block_state "$round" fleetbench after "$output_dir"
}

for ((round = 1; round <= ROUNDS; round++)); do
  if [ $((round % 2)) -eq 1 ]; then
    run_python "$round"
    run_fleetbench "$round"
  else
    run_fleetbench "$round"
    run_python "$round"
  fi
done

shopt -s nullglob
python_jsons=("$ARTIFACT_DIR"/round-*-python/sparky-adb-latency-perfherder.json)
fleetbench_jsons=("$ARTIFACT_DIR"/round-*-fleetbench/fleetbench.json)
if [ "${#python_jsons[@]}" -ne "$ROUNDS" ] || [ "${#fleetbench_jsons[@]}" -ne "$ROUNDS" ]; then
  echo "expected $ROUNDS completed artifact pairs, found ${#python_jsons[@]} Python and ${#fleetbench_jsons[@]} Fleetbench" >&2
  exit 1
fi

python_samples=$(jq -s --arg name adb-push-latency '[.[] | .suites[] | .subtests[] | select(.name == $name) | .replicates[]]' "${python_jsons[@]}")
fleetbench_samples=$(jq -s '[.[] | .adb_results.iterations[] | .elapsed_ms]' "${fleetbench_jsons[@]}")
jq -n \
  --arg device_serial "$DEVICE_SERIAL" \
  --argjson rounds "$ROUNDS" \
  --argjson iterations_per_block "$ITERATIONS" \
  --argjson python_samples "$python_samples" \
  --argjson fleetbench_samples "$fleetbench_samples" '
  def stats($samples):
    ($samples | sort) as $sorted
    | ($sorted | length) as $n
    | {samples: $n, mean_ms: ($sorted | add / $n), min_ms: $sorted[0], p50_ms: $sorted[($n * .50 | floor)], p95_ms: $sorted[($n * .95 | floor)], p99_ms: $sorted[($n * .99 | floor)], max_ms: $sorted[-1]};
  stats($python_samples) as $python
  | stats($fleetbench_samples) as $fleetbench
  | {
      device_serial: $device_serial,
      rounds: $rounds,
      iterations_per_block: $iterations_per_block,
      order: "odd: python then fleetbench; even: fleetbench then python",
      python_mozdevice: $python,
      fleetbench_mozdevice: $fleetbench,
      mean_delta_ms: ($fleetbench.mean_ms - $python.mean_ms),
      mean_ratio_fleetbench_to_python: ($fleetbench.mean_ms / $python.mean_ms)
    }
' > "$ARTIFACT_DIR/summary.json"

date -u +"finished_at_utc=%Y-%m-%dT%H:%M:%SZ" >> "$ARTIFACT_DIR/manifest.txt"
echo "comparison complete: $ARTIFACT_DIR"
cat "$ARTIFACT_DIR/summary.json"
