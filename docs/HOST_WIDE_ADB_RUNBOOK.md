# Host-wide Android USB/ADB runbook

Status: current operator procedure for the next LambdaTest HyperExecute
saturation rounds. Historical commands, results, and superseded plans live in
[HOST_WIDE_ADB_WORK_LOG.md](HOST_WIDE_ADB_WORK_LOG.md).

## Purpose

Measure ADB behavior while all eight selected phones attached to one Docker
host are active. This is a shared-host USB-contention experiment, not a
DevicePool migration: launch temporary fixed-device HyperExecute jobs and let
normal Taskcluster scheduling resume when they finish.

Run one host at a time. Determine contention from the recorded transfer
windows, never from job submission or start time.

## Non-negotiable safeguards

- Use repeated `--device <serial>` arguments only. Never use `--group` or
  `--all`.
- Do not substitute devices or combine hosts. If one of the eight is
  unavailable, defer the entire host.
- Use `--parallel 8`, `--start-delay 0`, and `--retries 0` for saturation
  batches.
- Set `FLEETBENCH_VERSION` explicitly for every launch. The long-running
  push/pull phases require a release supporting `adb --direction push` and
  `adb --direction pull` (currently `v0.4.3`).
- Stop and preserve artifacts if any job selects TCP ADB, selects a serial
  other than its target, loses a device, fails SHA-256 verification, or omits
  transfer timestamps. Fix the cause and rerun the full host; do not pool a
  tainted run with a clean one.

## Approved device sets and order

Run the `.55` standard host first, then the `.47` hub experiment. Keep `.47`
separate: it deliberately mixes seven `a55-perf` phones with one `stab` phone,
so it is an external comparison rather than a controlled before/after result.

| Host | Devices |
|---|---|
| `10.146.2.55` (`test-1`) | `R5CXC1HZA6V`, `R5CXC1ARZDN`, `R5CXC1HZ43J`, `R5CXC1HZ85W`, `R5CXC1SXMVR`, `RZCXC19G1DM`, `RZCXC1BK67D`, `RZCY107MCLV` |
| `10.146.2.47` (hub) | `RZCY10Y548K` (`stab`), `RZCY10Y4TJX`, `RZCY10Y4TBY`, `RZCY10Y4TAV`, `RZCY10Y4QVX`, `RZCY10Y4HWD`, `RZCY10LGB6W`, `RZCX821GXDJ` |

Before a saturation batch, confirm that every serial is available, still maps
to the stated host, and is reachable through bare USB serial selection. A
one-device smoke is appropriate when that needs verification; it is not a
saturation result.

## Run each host in three phases

Download and validate the JSON, log, and manifest artifacts after every phase
before launching the next one.

| Phase | Launcher | Purpose |
|---|---|---|
| bulk | `run_bulk.sh` | Mixed push/pull baseline over 25 B, 1 MiB, 10 MiB, and 100 MiB. |
| latency push | `run_sparky_push_only.sh` | One long, contiguous 25-byte push loop per device. |
| latency pull | `run_pull_only.sh` | One long, contiguous 25-byte pull loop per device. |

Do not set `FLEETBENCH_RUNS` for these runs. In particular, repeating a
long-running push/pull collector invocation introduces gaps and weakens
overlap. The push and pull launchers default to 5,000 iterations; adjust only
`FLEETBENCH_PUSH_ITERATIONS` or `FLEETBENCH_PULL_ITERATIONS` if the host timeout
budget requires it.

## Launch template

From `~/git/mozilla-bitbar-devicepool`:

```bash
source lt_env.sh

FLEETBENCH_VERSION=v0.4.3 lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/<launcher> \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-<host>-<phase>-v0.4.3 \
  --device <serial-1> --device <serial-2> --device <serial-3> \
  --device <serial-4> --device <serial-5> --device <serial-6> \
  --device <serial-7> --device <serial-8>
```

For the push phase, prefix the command with
`FLEETBENCH_PUSH_ITERATIONS=5000`; for pull, use
`FLEETBENCH_PULL_ITERATIONS=5000`. Substitute exactly one approved eight-device
set and use a label that names its host and phase. Never launch commands for
the two hosts concurrently.

## Validate and analyze

Each device must upload a manifest, JSON envelope, and log. Confirm the
manifested serial is the expected bare serial, checksums succeed, and every
transfer has `transfer_started_at_utc` and `transfer_finished_at_utc`.

For every `adb_results.iterations[]` record, build a transfer window from
those timestamps. A sample is contended only when its window overlaps a
transfer on another device. Report per-device and aggregate results for every
observed peer-overlap level, including the largest cohort.

Acceptance references:

- 100 MiB: median throughput at least 20 MiB/s; preferred range 25–32 MiB/s;
  p95 elapsed time about 5 seconds or less.
- 25 B: mean at most 375 ms, p95 at most 500 ms, p99 at most 750 ms.
- 50 KiB: p95 at most 1 second; 500 ms is preferred.

Short-transfer phases may not generate enough full-overlap samples for a
full-contention tail claim. Report that limitation explicitly rather than
using all-sample statistics as proof of eight-way contention.
