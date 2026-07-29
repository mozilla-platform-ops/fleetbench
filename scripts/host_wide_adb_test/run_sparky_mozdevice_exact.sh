#!/usr/bin/env bash
# Run inside one lt_run_cmd HyperExecute job. This is the literal Python
# mozdevice probe from Sparky's 2026-05-22 Try revision, isolated from the
# rest of Raptor so it can be scheduled directly against a selected device.
# Configure lt_run_cmd to upload fleetbench-artifacts/**.

set -euo pipefail

: "${DEVICE_SERIAL:?lt_run_cmd did not export DEVICE_SERIAL}"
case "$DEVICE_SERIAL" in
  *:*) echo "DEVICE_SERIAL must be a bare USB serial, got TCP transport $DEVICE_SERIAL" >&2; exit 2 ;;
esac

ITERATIONS="${FLEETBENCH_ADB_LATENCY_ITERATIONS:-200}"
REMOTE_DIR="${FLEETBENCH_ADB_LATENCY_REMOTE_DIR:-/sdcard/Download}"
TRY_REVISION="${FLEETBENCH_MOZDEVICE_TRY_REVISION:-7757fbcccc8eb83105af2b9518517f47dcca9eff}"
ARTIFACT_DIR="${FLEETBENCH_ARTIFACT_DIR:-fleetbench-artifacts}"
WORK_DIR="$(mktemp -d)"
PYTHON="${PYTHON:-python3}"

case "$ITERATIONS" in
  ''|*[!0-9]*) echo "FLEETBENCH_ADB_LATENCY_ITERATIONS must be a positive integer, got $ITERATIONS" >&2; exit 2 ;;
esac
if [ "$ITERATIONS" -lt 1 ]; then
  echo "FLEETBENCH_ADB_LATENCY_ITERATIONS must be at least 1" >&2
  exit 2
fi
case "$REMOTE_DIR" in
  /sdcard/Download|/sdcard/Download/) ;;
  *) echo "FLEETBENCH_ADB_LATENCY_REMOTE_DIR must be /sdcard/Download for the exact Sparky probe" >&2; exit 2 ;;
esac

cleanup() {
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

mkdir -p "$ARTIFACT_DIR" "$WORK_DIR/mozdevice/mozdevice"

# Fetch the exact mozdevice source Raptor used in Sparky's Try revision. The
# only third-party runtime dependency is mozlog; it does not implement ADB.
MOZDEVICE_URL="https://hg-edge.mozilla.org/try/raw-file/${TRY_REVISION}/testing/mozbase/mozdevice/mozdevice"
for module in __init__.py adb.py adb_android.py remote_process_monitor.py version_codes.py; do
  curl --fail --location --retry 3 --output "$WORK_DIR/mozdevice/mozdevice/$module" \
    "$MOZDEVICE_URL/$module"
done
"$PYTHON" -m venv "$WORK_DIR/venv"
"$WORK_DIR/venv/bin/python" -m pip install --disable-pip-version-check --no-cache-dir 'mozlog>=6'

{
  echo "phase=sparky-mozdevice-exact"
  echo "device_serial=$DEVICE_SERIAL"
  echo "try_revision=$TRY_REVISION"
  echo "mozdevice_source=testing/mozbase/mozdevice"
  echo "remote_path=/sdcard/Download"
  echo "payload=adb-latency test payload\\n"
  echo "payload_bytes=25"
  echo "iterations=$ITERATIONS"
  echo "timing=time.perf_counter around ADBDevice.push"
  echo "phase_timings=${FLEETBENCH_MOZDEVICE_PHASE_TIMINGS_PATH:+enabled}"
  echo "cleanup=deferred until after timed loop"
  date -u +"started_at_utc=%Y-%m-%dT%H:%M:%SZ"
} > "$ARTIFACT_DIR/manifest.txt"

cat > "$WORK_DIR/run_probe.py" <<'PYTHON'
import json
import os
import sys
import tempfile
import time

from mozdevice import ADBDeviceFactory


def main():
    iterations = int(os.environ["FLEETBENCH_ADB_LATENCY_ITERATIONS"])
    remote_dir = os.environ["FLEETBENCH_ADB_LATENCY_REMOTE_DIR"].rstrip("/")
    output_path = sys.argv[1]
    phase_timings_path = os.environ.get("FLEETBENCH_MOZDEVICE_PHASE_TIMINGS_PATH")

    # This is the same factory call Raptor's _initialize_device() made before
    # Sparky's run_adb_latency_test(), with ANDROID_SERIAL selecting the
    # lt_run_cmd-assigned device.
    device = ADBDeviceFactory(verbose=True)
    active_phase_timings = None
    diagnostic_iterations = []

    def record_phase(method, command):
        if method == "command_output":
            if command == ["shell", "sync"]:
                return (
                    "pre_push_sync"
                    if not any(p["phase"] == "pre_push_sync" for p in active_phase_timings)
                    else "post_push_sync"
                )
            if command and command[0] == "push":
                return "push"
        elif method == "shell_bool" and command.startswith("test -d "):
            return "remote_directory_check"
        elif method == "shell_output":
            if command == "sync":
                return (
                    "pre_push_sync"
                    if not any(p["phase"] == "pre_push_sync" for p in active_phase_timings)
                    else "post_push_sync"
                )
            if command == "set":
                return "external_storage_discovery"
        return None

    def wrap_adb_method(method_name):
        original = getattr(device, method_name)

        def wrapped(*args, **kwargs):
            command = args[0] if args else kwargs.get("cmds", kwargs.get("cmd", ""))
            started = time.perf_counter()
            try:
                return original(*args, **kwargs)
            finally:
                if active_phase_timings is not None:
                    phase = record_phase(method_name, command)
                    if phase:
                        active_phase_timings.append(
                            {"phase": phase, "elapsed_ms": (time.perf_counter() - started) * 1000.0}
                        )

        setattr(device, method_name, wrapped)

    if phase_timings_path:
        for method_name in ("command_output", "shell_bool", "shell_output"):
            wrap_adb_method(method_name)
    local_file = None
    pushed_remote_paths = []
    replicates = []
    try:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False) as f:
            f.write("adb-latency test payload\n")
            local_file = f.name

        run_id = int(time.time())
        for i in range(iterations):
            remote_path = "%s/adb-latency-%d-%d.txt" % (remote_dir, run_id, i)
            if phase_timings_path:
                active_phase_timings = []
            start = time.perf_counter()
            device.push(local_file, remote_path)
            elapsed_ms = (time.perf_counter() - start) * 1000.0
            if phase_timings_path:
                diagnostic_iterations.append(
                    {"iteration": i, "elapsed_ms": elapsed_ms, "phases": active_phase_timings}
                )
                active_phase_timings = None
            replicates.append(elapsed_ms)
            pushed_remote_paths.append(remote_path)
    finally:
        if local_file and os.path.exists(local_file):
            try:
                os.remove(local_file)
            except OSError:
                pass
        for remote_path in pushed_remote_paths:
            try:
                if device.exists(remote_path):
                    device.rm(remote_path, force=True)
            except Exception as error:
                print("failed to remove remote file %s: %s" % (remote_path, error), file=sys.stderr)

    if not replicates:
        raise RuntimeError("adb-latency probe produced no replicates")

    avg_ms = sum(replicates) / len(replicates)
    perfherder_data = {
        "framework": {"name": "browsertime"},
        "suites": [
            {
                "name": "adb-latency",
                "type": "adhoc",
                "unit": "ms",
                "lowerIsBetter": True,
                "alertThreshold": 2.0,
                "value": avg_ms,
                "subtests": [
                    {
                        "name": "adb-push-latency",
                        "unit": "ms",
                        "lowerIsBetter": True,
                        "value": avg_ms,
                        "replicates": replicates,
                    }
                ],
            }
        ],
    }
    print("PERFHERDER_DATA: %s" % json.dumps(perfherder_data))
    with open(output_path, "w") as output:
        json.dump(perfherder_data, output, indent=2, sort_keys=True)
    if phase_timings_path:
        with open(phase_timings_path, "w") as output:
            json.dump(
                {
                    "timing": "time.perf_counter around each mozdevice subprocess",
                    "iterations": diagnostic_iterations,
                },
                output,
                indent=2,
                sort_keys=True,
            )


if __name__ == "__main__":
    main()
PYTHON

json="$ARTIFACT_DIR/sparky-adb-latency-perfherder.json"
log="$ARTIFACT_DIR/sparky-adb-latency.log"
phase_timings="${FLEETBENCH_MOZDEVICE_PHASE_TIMINGS_PATH:-}"
echo "Exact Sparky mozdevice run: serial=$DEVICE_SERIAL iterations=$ITERATIONS"
if ! ANDROID_SERIAL="$DEVICE_SERIAL" \
  FLEETBENCH_ADB_LATENCY_ITERATIONS="$ITERATIONS" \
  FLEETBENCH_ADB_LATENCY_REMOTE_DIR="$REMOTE_DIR" \
  FLEETBENCH_MOZDEVICE_PHASE_TIMINGS_PATH="$phase_timings" \
  PYTHONPATH="$WORK_DIR/mozdevice" \
  "$WORK_DIR/venv/bin/python" "$WORK_DIR/run_probe.py" "$json" >"$log" 2>&1; then
  echo "Exact Sparky mozdevice run failed; preserving $log" >&2
  exit 1
fi

date -u +"finished_at_utc=%Y-%m-%dT%H:%M:%SZ" >> "$ARTIFACT_DIR/manifest.txt"
# BSD find (the macOS default) does not support GNU find's -printf.
find "$ARTIFACT_DIR" -maxdepth 1 -type f -exec basename {} \; | sort
