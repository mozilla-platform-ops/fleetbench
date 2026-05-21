# Fleetbench Design v2

## Changes from v1

- Split into two components: a Rust **collector** binary and a Python **runner** wrapper.
- Added run-environment capture, starting with system load.
- Clarified that a single composite "health score" lives in the downstream analysis layer, not in the collector.
- Defined an on-disk results layout suitable for later collection/ingestion.

## Repo Layout

Both components live in the same repository under separate top-level directories:

```
fleetbench/
  collector/    Rust crate (cargo project)
  runner/       Python package
  docs/
```

They are versioned and released independently. Neither imports from the other; the runner invokes the collector as a subprocess.

## Component Overview

```
+-------------------+       +--------------------+       +------------------+
|  fleetbench-run   | ----> |   fleetbench       | ----> | results dir      |
|  (Python wrapper) |       |   (Rust collector) |       | (JSON per run)   |
+-------------------+       +--------------------+       +------------------+
                                                                  |
                                                                  v
                                                       +----------------------+
                                                       | collection / ingest  |
                                                       | (separate tool)      |
                                                       +----------------------+
                                                                  |
                                                                  v
                                                       +----------------------+
                                                       | analysis / scoring   |
                                                       | (separate tool)      |
                                                       +----------------------+
```

The collector and the runner are independently versioned and independently testable. The collector knows nothing about scheduling, file layout, or fleet identity. The runner knows nothing about benchmark internals.

## Rust Collector

Scope is unchanged from v1 with one addition: the collector captures lightweight run-environment metadata immediately before and after the timed section.

### Responsibilities

- Parse CLI arguments.
- Collect host metadata (`inspect`).
- Run CPU benchmark workloads.
- Capture run-environment samples (see below).
- Emit a single JSON object on stdout.
- Exit non-zero on correctness or runtime failure.

### Non-responsibilities

- No scheduling.
- No persistence to disk (stdout only).
- No directory layout opinions.
- No retention or rotation.
- No upload, sync, or network I/O.
- No fleet identity beyond the OS hostname.

### Run-environment capture

The collector samples system load at three points: before warmup, after warmup but before the first timed iteration, and immediately after the last timed iteration. All three samples are reported verbatim. The collector does not decide whether the load was "too high" — that judgement belongs to analysis.

The pre-warmup sample captures the host's idle state. The pre-timed sample captures conditions immediately before measurement begins. The delta between them is itself a useful signal (for example, warmup ramped the CPU and pulled in unrelated work).

Each sample contains:

- `cpu_counters`: a single-point-in-time snapshot of cumulative CPU time counters. Differencing two snapshots over a window yields CPU utilization for that window. On Linux, read from `/proc/stat` (kind `"linux_proc_stat"`, jiffies). On Windows, read from `GetSystemTimes` (kind `"windows_get_system_times"`, 100-ns intervals). `null` if unavailable on the running platform. Fields: `kind`, `idle_units`, `iowait_units` (Linux-only, `null` on Windows), `total_units`.
- `load_1`, `load_5`, `load_15`: Linux-only, from `getloadavg(3)` or `/proc/loadavg`. `null` on Windows.
- `processor_queue_length`: Windows-only, optional. `null` on Linux, and also `null` on Windows if not cheaply obtainable.

Sampling is cheap (single file read or syscall, no sleep), requires no privileges, and adds no measurable runtime overhead. An earlier iteration computed `cpu_percent` directly by sleeping ~100 ms between two `/proc/stat` reads inside the collector; that approach was dropped because the idle sleeps between warmup and timed iterations let the CPU governor down-clock, producing a consistent first-iteration penalty in the timed phase. Raw counter snapshots avoid this side effect and follow the "raw data first" design principle: any rate-over-window calculation is a pure function of the stored snapshots and can be recomputed downstream without re-running the fleet.

### JSON additions

A new `environment` block appears alongside `host`, `cpu`, `config`, and `results`:

```json
{
  "environment": {
    "load_pre_warmup": {
      "cpu_counters": {
        "kind": "linux_proc_stat",
        "idle_units": 1827451, "iowait_units": 9382, "total_units": 2249341
      },
      "load_1": 0.42, "load_5": 0.55, "load_15": 0.61,
      "processor_queue_length": null
    },
    "load_pre_timed": {
      "cpu_counters": {
        "kind": "linux_proc_stat",
        "idle_units": 1827612, "iowait_units": 9384, "total_units": 2249720
      },
      "load_1": 0.48, "load_5": 0.56, "load_15": 0.61,
      "processor_queue_length": null
    },
    "load_post_timed": {
      "cpu_counters": {
        "kind": "linux_proc_stat",
        "idle_units": 1827648, "iowait_units": 9384, "total_units": 2258140
      },
      "load_1": 8.91, "load_5": 2.10, "load_15": 0.95,
      "processor_queue_length": null
    }
  }
}
```

Fields that cannot be obtained on a given platform are emitted as `null` rather than omitted. This keeps the schema uniform across Linux and Windows.

### Schema version bump

Adding `environment` is a backward-compatible addition. The schema version remains `1` for v0 results that lack the field; emissions with the original `cpu_percent`-style environment block use `schema_version = 2`. The current shape uses `schema_version = 3`, which replaces `cpu_percent` with the raw `cpu_counters` snapshot per the rationale above.

## Python Runner

The runner is a small Python program that wraps the collector and persists results. It is deliberately thin: it decides whether to run, shells out to the Rust binary, writes the result to disk, and exits.

### Operational Model

The runner is invoked by the worker-startup wrapper on each host boot, *before* the Taskcluster worker starts. This is the moment when:

- the host is guaranteed idle (the worker hasn't begun a task yet),
- the timing is naturally serialized with worker lifecycle,
- no OS-level scheduling infrastructure (systemd timers, cron, Task Scheduler) is required — the wrapper IS the schedule.

The runner self-throttles by inspecting the newest envelope file in the results directory. If less than `--min-interval` has elapsed since that envelope's timestamp, the runner logs "throttled" and exits 0 without invoking the collector. Otherwise it proceeds.

Because reboots happen frequently in a test fleet (a reboot follows every test), the runner gets many opportunities to fire per day. `--min-interval` is therefore a **soft lower bound**: "at least this much time between runs." If reboots stop, runs stop. `--min-interval=24h` does not mean "every Sunday" — it means "no more than once per 24 hours, gated on the wrapper invoking us."

Any envelope file counts as a previous run, success or failure. This prevents a degraded host that fails the benchmark from burning through every reboot retrying.

### Responsibilities

- Decide whether to run, based on results-dir state and `--min-interval`.
- Pre-flight the host via gwhc on Linux; skip the run if the host is not IDLE.
- Invoke the collector with a configured mode (default `normal`).
- Capture stdout, stderr, and exit code.
- Write the result to the configured results directory using an atomic rename.
- Wrap the collector output in a thin envelope recording wrapper-level facts the collector cannot know.

### Non-responsibilities

- No benchmark logic.
- No parsing or transformation of the collector's `results` block.
- No analysis, scoring, or comparison.
- No upload or remote sync. A separate collection tool handles that.
- No scheduling infrastructure. The wrapper invokes the runner; the runner does not register timers or daemonize.

### Envelope format

The runner wraps the collector JSON rather than mutating it. This keeps the collector output bit-for-bit recoverable from any stored file.

```json
{
  "envelope_version": 1,
  "runner_version": "0.1.0",
  "run_id": "0c48d593a5194c8d8372493908526880",
  "trigger": "boot",
  "started_utc": "2026-05-20T03:00:01Z",
  "finished_utc": "2026-05-20T03:02:14Z",
  "collector_exit_code": 0,
  "collector_killed_by_runner": false,
  "collector_output": { /* unmodified collector JSON */ }
}
```

`trigger` is `boot` (invoked by the worker-startup wrapper) or `manual` (ad-hoc operator invocation). `run_id` is a 32-character hex uuid4; sort order across runs comes from the filename's leading timestamp, not the run id, so uniqueness alone is sufficient.

If the collector exits non-zero, the envelope is still written, with `collector_output` set to whatever the collector emitted (which, per v1, should still be valid JSON describing the failure).

If the collector crashes hard — non-JSON stdout, panic, segfault, killed by signal — the runner records the failure in the envelope rather than dropping the run:

```json
{
  "collector_exit_code": -11,
  "collector_output": null,
  "collector_stdout_raw": "...",
  "collector_stderr": "thread 'main' panicked at ...",
  "collector_output_parse_error": "expected value at line 1 column 1",
  "collector_killed_by_runner": false
}
```

`collector_stdout_raw` and `collector_stderr` are truncated to 16 KB each (UTF-8 boundary-aware). `collector_exit_code` is the raw integer the OS reports — negative signal-derived values on Linux, the unmodified process exit code (or NTSTATUS-derived value) on Windows. The runner does not normalize these into signal names; analysis can interpret per-platform. This guarantees every invocation that decides to run produces exactly one file, and broken collectors are visible downstream.

On success the three diagnostic fields (`collector_stdout_raw`, `collector_stderr`, `collector_output_parse_error`) are omitted from the JSON so success envelopes stay clean. On failure they are emitted even when empty so consumers can distinguish "parsed but empty stderr" from "this field was never relevant."

The runner enforces a hard timeout (default 10 minutes) as a backstop against a hung collector. On timeout the runner writes an envelope with `collector_killed_by_runner: true` and whatever exit code the OS returned after the kill.

To ensure the kill actually terminates the collector and any descendants:

- On Linux, the collector is launched in its own process group (`os.setsid`) and the runner sends `SIGKILL` to the group.
- On Windows, the collector is launched inside a Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Killing the job terminates the collector and any child processes atomically. This requires `pywin32` or a small ctypes wrapper; v0 of the collector spawns no children, but the Job Object is set up from day one so later workloads cannot orphan processes.

### Results directory layout

Flat directory, one file per run:

```
<results_dir>/
  2026-05-20T03-00-01Z_linux-perf-123_cpu-v0_01HXYZABC...json
  2026-05-20T03-00-01Z_linux-perf-123_cpu-v0_01HXYZABC....json.partial   (during write)
```

Filename components, joined by `_`:

- ISO-8601 UTC timestamp with `:` replaced by `-` for Windows compatibility.
- Hostname.
- CPU suite version.
- Run ID.

Properties:

- Sortable by filename.
- Globbing by hostname or suite version is trivial.
- A downstream collector can `rsync`/`scp`/`robocopy` the directory and treat each file as independent.
- No appended files. No locks. Concurrent runs (which should be rare) write distinct filenames.

Writes use the standard temp-file-plus-rename pattern: write to `*.partial`, `fsync`, then rename into final name. A collection tool can safely skip files ending in `.partial`.

### Throttle Decision

On startup the runner lists the results directory, identifies the newest envelope filename (skipping `*.partial` and any file that does not match the envelope shape), and parses the timestamp embedded in the filename. The decision is:

- If no prior envelope exists: run.
- If `now - newest_envelope_started_utc >= min_interval`: run.
- Otherwise: log "throttled: last run X ago, less than min_interval Y; next run in Z" and exit 0 without invoking the collector.

The boundary is inclusive: `elapsed == min_interval` runs. `--min-interval=0s` always runs.

No sidecar state file is maintained. The results directory itself is the single source of truth for "when did this host last run?" — if a host is wiped or the results dir is cleared, the next invocation runs unconditionally.

### Activity Pre-flight

On Linux, before invoking the collector the runner runs `gwhc --json` (Mozilla releng's generic worker host check) with a 10-second timeout. If the reported `state` is anything other than `"IDLE"`, the runner logs the gwhc summary (including any non-passing checks) and exits 0 without running the collector. The operational model already guards against running mid-test by invoking the runner before the worker boots, but an ad-hoc operator invocation mid-test would otherwise produce noisy data; this is defense in depth.

The check is deliberately advisory: hosts without gwhc installed (the entire non-Linux fleet, plus any Linux host that does not ship it) proceed silently, and a broken gwhc that returns non-JSON or non-object output is treated as advisory ("proceed"). The one exception is valid JSON missing a `state` field — that blocks, on the principle that recording potentially-noisy data is worse than skipping a run.

`--skip-activity-check` bypasses the check entirely for development and debugging.

### Retention

The runner does not delete old files. Retention is a separate concern handled either by the collection tool (after successful ingestion) or by a simple periodic cleanup task. Keeping retention out of the runner avoids accidental data loss when the collection pipeline is broken.

### CLI

```bash
fleetbench-run --results-dir /var/lib/fleetbench
fleetbench-run --results-dir /var/lib/fleetbench --mode quick --min-interval 1h
fleetbench-run --results-dir ./out --mode normal --trigger manual --skip-activity-check
fleetbench-run --results-dir ./out --collector-binary ./fleetbench
```

Options:

```text
--results-dir <path>          required: directory to write envelope files into
--mode quick|normal|long      collector cpu mode (default: normal)
--collector-binary <path>     path to the collector (default: fleetbench on PATH)
--min-interval <duration>     soft lower bound on cadence (default: 24h)
--skip-activity-check         skip the gwhc pre-flight
--trigger boot|manual         value recorded in the envelope (default: boot)
--timeout <duration>          hard timeout for the collector subprocess (default: 10m)
```

`--min-interval` and `--timeout` accept durations like `30m`, `1h`, `24h`, `7d`. Compound forms (`1h30m`), fractional values, and unsupported units (`1w`, `1y`) are rejected at argument-parse time.

The runner exits 0 in every case that writes an envelope file (success, hard collector failure, timeout-killed collector), and also exits 0 when throttled or blocked by the activity check. Non-zero exits indicate the runner itself failed (bad arguments, disk write error, etc.). This lets the worker-startup wrapper treat "runner exited" as success and rely on the envelope file's presence and contents for actual results.

## Health Score

The v1 doc explicitly avoids a single composite score. v2 keeps that position for the collector and runner.

A single-number "is this host way out of spec" indicator is useful, but it belongs in the analysis layer for two reasons:

1. It must be relative to a peer group (same hardware class, same suite version). The collector has no peer group.
2. It must be recomputable when the formula changes. Storing it in the collector output would freeze a definition that should evolve.

The analysis layer can compute a per-suite-version relative score (for example, a z-score or median ratio against same-class hosts) directly from the stored raw timings. This gives the desired "one number per host" view without contaminating the collected data.

## Resolved Decisions

These were open in v1 and are settled for v0/v1 implementation:

- **Sieve implementation: segmented**, for both `prime_sieve_1t` and `prime_sieve_mt`. A naive sieve at `long` mode (N=10⁹) would allocate a ~125 MB bitmap and measure DRAM bandwidth rather than CPU/cache behavior. Segmented sieves process L1/L2-sized chunks (32 KB initial target) and keep only base primes up to √N. The segment size is part of the suite version and must not change silently.
- **`normal` mode stays fixed-limit**, not fixed-runtime. Fixed-limit makes seconds-to-complete directly comparable across runs and hosts. The runner's hard timeout (default 10 minutes) bounds total wall time for degraded hosts. Limits are sized so a healthy host completes `normal` in well under that backstop.
- **No collector-side summary statistics** (median, p95, cov, etc.) in v0. Raw per-iteration timings only. Summaries are trivially recomputable and including them in the collector output encourages downstream consumers to read the summary and ignore the raw data — exactly the failure mode the "raw data first" principle is meant to prevent. Whether `stability_loop` in v1 is the one exception (since variance is its entire point) will be decided when v1 lands.

## Migration from v1

- v1 collector output (schema_version 1, no `environment` block) remains valid input to downstream tooling.
- v2 collector output (schema_version 2) adds `environment` and is otherwise identical.
- The runner can wrap either; the envelope is independent of the collector schema.

## Open Questions

- On Windows, is `processor_queue_length` worth the implementation cost, or should both load fields simply be `null` until there is a concrete analysis use for them?
- Should the results directory include a `latest.json` symlink/copy for easy local inspection? Convenient but adds a write-ordering concern.
- Is CPython already installed on the Windows performance hosts? If not, the runner needs to ship as a frozen executable (PyInstaller) or be ported to Rust. Porting the runner to Rust is acceptable if needed — the component split stands regardless of language.
- Per-mode cadence: should a single host eventually run multiple modes at different intervals (e.g. quick hourly, normal daily, long weekly)? Deferred — current design assumes one cadence per host. Adding it requires the throttle check to maintain per-mode last-run state.

## Milestones

### M1: Collector v0 with environment block

- Implement v1 milestones 1 and 2.
- Add `environment.load_before` and `environment.load_after`.
- Bump `schema_version` to 2.

### M2: Python runner

- Implement envelope, atomic write, filename scheme.
- Implement throttle decision from results-dir state.
- Implement gwhc activity pre-flight on Linux.
- Implement timeout backstop with Linux process-group SIGKILL.
- Document wrapper integration; Windows runner deferred until the CPython availability question is answered.
- Confirm end-to-end on a representative Linux host.

### M3: Collector v1 workloads

- Per v1 milestone 3.

### M4: Collection and analysis handoff

- Separate tool consumes the results directory.
- Analysis layer computes the relative health score.
