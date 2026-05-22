# Fleetbench

A small cross-platform CPU benchmark collector for performance-testing fleets
(pools of Taskcluster worker hosts that run Firefox perf tests), plus a
Python runner that wraps it for use on those hosts.

Fleetbench produces raw per-iteration timings and host metadata as versioned
JSON. It does not score hosts, compare across hardware classes, or maintain
fleet-wide state — that work belongs to a downstream analysis layer fed from
the collected envelope files.

## Repo Layout

- [`collector/`](collector/) — Rust binary (`fleetbench`). Single-host-aware,
  emits one JSON object per invocation on stdout. No filesystem opinions.
- [`runner/`](runner/) — Python package (`fleetbench-run`). Wraps the
  collector, self-throttles, writes envelope files to disk.
- [`docs/`](docs/)
  - [`fleetbench_design_v2.md`](docs/fleetbench_design_v2.md) — design doc.
    Start here.
  - [`analysis_notes.md`](docs/analysis_notes.md) — guidance for the
    downstream analysis layer (use median, drop iter 0, etc.).

## Status

| Component | Linux | Windows | macOS | Android |
|---|---|---|---|---|
| Collector | shipped | binary cross-compiles, env sampling fields are null pending implementation | shipped (env block intentionally null — no `/proc` on Darwin) | shipped (env block populated; same `/proc/stat` + `/proc/loadavg` path as Linux) |
| Runner    | shipped | deferred pending CPython availability question | works (dev) | not applicable — Android deploy model is different |

### Choosing a mode

`normal` (pi(10⁸), 5 iterations) targets ~150 ms per iteration on slow-x86 fleet
hosts (Xeon E3-class), which is where signal quality matters most. On much
faster hardware — M-class Macs, modern workstations — per-iteration timing
drops to ~90 ms, which is below the ~100 ms noise floor for tight outlier
detection. Use `--mode long` (pi(10⁹), 3 iterations) on hardware that fast
to keep iterations comfortably above the noise floor. Slow phones and old
fleet hardware are well-served by `normal`.

Verified end-to-end:
- **Linux**: smoke-tested on real fleet hosts (Xeon E3-1585L v5).
- **macOS**: dev box (Apple Silicon M4 Pro); pi(10⁹) 1t in ~840 ms, mt in ~118 ms across 14 cores.
- **Android**: Pixel 10 Pro via `adb push`. See [`docs/analysis_notes.md`](docs/analysis_notes.md)
  for Android-specific behavior the analysis layer needs to know about
  (governor ramp, big.LITTLE + thermal throttling, non-zero idle load averages).

Caveats:
- `cpu.frequency_mhz` is `null` on macOS — Apple Silicon doesn't expose a single meaningful peak frequency and sysinfo's value is unreliable, so we deliberately drop it rather than emit a misleading number.
- `cpu.brand` is null on Android (sysinfo doesn't parse the SoC name from `/proc/cpuinfo` on ARM); workaround if needed: parse it directly.

## Build

### Collector (Rust)

```bash
cd collector
cargo build --release                  # native build for dev
./build                                # build all four (linux + windows + mac + android)
./build --platform linux               # just the linux musl binary
./build --platform windows             # just the windows .exe
./build --platform mac                 # just the mac host-arch binary
./build --platform android             # aarch64 Android (requires NDK)
```

`./build` produces:
- `target/x86_64-unknown-linux-musl/release/fleetbench` (~1.1 MB, static, runs
  on any modern Linux including Ubuntu 18.04)
- `target/x86_64-pc-windows-gnu/release/fleetbench.exe` (~1.0 MB)
- `target/<host-arch>-apple-darwin/release/fleetbench` (~1.1 MB)
- `target/aarch64-linux-android/release/fleetbench`

Linux and Windows builds cross-compile via `cargo-zigbuild`; the Mac build
uses the native Apple toolchain; the Android build uses `cargo-ndk`.

Tooling: `brew install zig`, `cargo install cargo-zigbuild cargo-ndk`,
and the rustup targets:

```bash
rustup target add x86_64-unknown-linux-musl x86_64-pc-windows-gnu \
                  aarch64-apple-darwin aarch64-linux-android
```

Android additionally needs the NDK. With Homebrew:

```bash
brew install --cask android-ndk
export ANDROID_NDK_HOME="$(brew --prefix)/share/android-ndk"
```

Add the `export` to your shell rc so it persists. Android Studio's SDK
Manager also works; in that case `ANDROID_NDK_HOME` points at the SDK's
`ndk/<version>/` directory instead.

### Runner (Python)

```bash
cd runner
uv sync                          # creates .venv, installs deps including pytest
uv run pytest -q                 # 98 tests
uv run fleetbench-run --help
```

## Smoke Test

`collector/smoke` builds the binary, scps it to a target host, runs a
sequence of validation checks, and prints a per-run timing table plus
aggregate iter-0/iter-1+ distributions.

```bash
cd collector
./smoke <linux-host> --runs 5 --mode normal
./smoke <windows-host> --platform windows --runs 3 --mode normal
```

The smoke does:

1. `cargo zigbuild` for the target platform.
2. `scp` the binary to the host's home dir.
3. `gwhc --json` activity check (Linux only; skipped silently elsewhere).
4. `inspect` for host/CPU metadata.
5. N runs of `cpu --json` with full schema validation per envelope.
6. Negative test: `--threads 0 --json` must produce a failure envelope and
   exit 1.

If `gwhc` reports a non-IDLE state, smoke exits 0 with a summary rather than
running benchmarks against a contaminated baseline.

### Android (manual; adb-based)

`./smoke` does not yet wire Android. Use `adb` directly:

```bash
cd collector
./build --platform android
adb push target/aarch64-linux-android/release/fleetbench /data/local/tmp/fleetbench
adb shell chmod 755 /data/local/tmp/fleetbench
adb shell /data/local/tmp/fleetbench inspect
adb shell /data/local/tmp/fleetbench cpu --mode quick --json
```

`/data/local/tmp/` is the standard "anyone can push and execute" path on
Android. The collector emits the same v3 envelope as on Linux, with
`host.os_family = "android"` and a populated `environment` block from the
same `/proc/stat` + `/proc/loadavg` reads. `adb shell` exit codes are
historically unreliable; trust the JSON's `status` field, not `$?`.

## Operational Model (Runner)

Invoked by the worker-startup wrapper *before* the Taskcluster worker boots.
Self-throttles based on the newest envelope timestamp in the results
directory (`--min-interval`, default 24h). Pre-flights the host via `gwhc`
on Linux and skips runs against non-IDLE hosts. Writes one envelope file per
run, success or failure, via `.partial` + atomic rename. See
[the design doc](docs/fleetbench_design_v2.md) for the full contract.

```bash
fleetbench-run \
  --results-dir /var/lib/fleetbench \
  --mode normal \
  --collector-binary /usr/local/bin/fleetbench \
  --min-interval 24h
```

### Alternative: Taskcluster jobs (not yet built)

A possible companion model is to run the collector inside dedicated Taskcluster
jobs targeted at specific worker pools, with a small controller tool that
enqueues the jobs, records their IDs, polls for completion, and pulls the
envelope artifacts back. Useful for targeted sweeps ("benchmark every
gecko_t_linux_talos host now, before/after this kernel change") rather than
continuous drift detection.

Tradeoffs noted but not yet committed work:

- **Queue contention.** Benchmark jobs compete with real test traffic for
  worker time; on a busy queue, hourly or even daily fleet sweeps could end
  up waiting behind production work. The boot-throttle model sidesteps this
  by slipping into a window where the worker is *not* taking tasks.
- **Per-job overhead.** TC task scheduling, image pull, and log shipping for
  what's a ~5 second benchmark is wasteful compared to direct invocation.
- **Visibility cost.** Every benchmark becomes a TC entity that shows up in
  task dashboards.

A TC-driven invocation does not require a new runner — the existing
`fleetbench-run` would just need a `taskcluster` value added to its
`--trigger` enum and invocation from inside the task. Filing as a real
beads task is deferred until someone needs the controlled-sweep capability.

## Issue Tracking

Tasks live in `.beads/` via [beads_rust](https://github.com/Dicklesworthstone/beads_rust);
see [`AGENTS.md`](AGENTS.md) for workflow conventions.
