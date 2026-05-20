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

- `cpu_percent`: short-interval CPU utilization, populated on both platforms. On Linux, computed by reading `/proc/stat` twice ~100 ms apart and differencing the idle/total jiffies. On Windows, computed by calling `GetSystemTimes()` twice ~100 ms apart and differencing the idle/kernel/user tick counts. This is the load signal that exists on both platforms; analysis should prefer it for cross-platform comparisons.
- `load_1`, `load_5`, `load_15`: Linux-only, from `getloadavg(3)` or `/proc/loadavg`. `null` on Windows.
- `processor_queue_length`: Windows-only, optional. `null` on Linux, and also `null` on Windows if not cheaply obtainable. Not required for v2.

Sampling is cheap (~100 ms per sample point), requires no privileges, and adds negligible runtime overhead.

### JSON additions

A new `environment` block appears alongside `host`, `cpu`, `config`, and `results`:

```json
{
  "environment": {
    "load_pre_warmup": {
      "cpu_percent": 3.1,
      "load_1": 0.42, "load_5": 0.55, "load_15": 0.61,
      "processor_queue_length": null
    },
    "load_pre_timed": {
      "cpu_percent": 4.0,
      "load_1": 0.48, "load_5": 0.56, "load_15": 0.61,
      "processor_queue_length": null
    },
    "load_post_timed": {
      "cpu_percent": 99.6,
      "load_1": 8.91, "load_5": 2.10, "load_15": 0.95,
      "processor_queue_length": null
    }
  }
}
```

Fields that cannot be obtained on a given platform are emitted as `null` rather than omitted. This keeps the schema uniform across Linux and Windows.

### Schema version bump

Adding `environment` is a backward-compatible addition. The schema version remains `1` for v0 results that lack the field; new emissions set `schema_version = 2` and always include `environment`, even if all fields inside it are `null`.

## Python Runner

The runner is a small Python program that schedules and persists collector output. It is deliberately thin: it shells out to the Rust binary, writes the result to disk, and exits.

### Responsibilities

- Invoke the collector with a configured mode (default `normal`).
- Capture stdout and exit code.
- Write the result to the configured results directory using an atomic rename.
- Add a thin envelope around the collector output recording wrapper-level facts the collector cannot know.
- Provide a single command suitable for cron, systemd timers, or Windows Task Scheduler.

### Non-responsibilities

- No benchmark logic.
- No parsing or transformation of the collector's `results` block.
- No analysis, scoring, or comparison.
- No upload or remote sync. A separate collection tool handles that.

### Envelope format

The runner wraps the collector JSON rather than mutating it. This keeps the collector output bit-for-bit recoverable from any stored file.

```json
{
  "envelope_version": 1,
  "runner_version": "0.1.0",
  "run_id": "01HXYZ...",
  "trigger": "scheduled",
  "scheduled_for_utc": "2026-05-20T03:00:00Z",
  "started_utc": "2026-05-20T03:00:01Z",
  "finished_utc": "2026-05-20T03:02:14Z",
  "collector_exit_code": 0,
  "collector_output": { /* unmodified collector JSON */ }
}
```

`trigger` is `scheduled` or `manual`. `run_id` is a ULID generated by the runner.

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

`collector_stdout_raw` and `collector_stderr` are truncated to 16 KB each. `collector_exit_code` is the raw integer the OS reports — negative signal-derived values on Linux, the unmodified process exit code (or NTSTATUS-derived value) on Windows. The runner does not normalize these into signal names; analysis can interpret per-platform. This guarantees every scheduled run produces exactly one file, and broken collectors are visible downstream.

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

### Scheduling

The runner does not schedule itself. It is a one-shot command. Scheduling is the OS's job:

- Linux: systemd timer (preferred) or cron.
- Windows: Task Scheduler.

A weekly cadence is the initial target. The runner accepts `--mode` to override the default benchmark mode for the run.

### Retention

The runner does not delete old files. Retention is a separate concern handled either by the collection tool (after successful ingestion) or by a simple periodic cleanup task. Keeping retention out of the runner avoids accidental data loss when the collection pipeline is broken.

### CLI

```bash
fleetbench-run --results-dir /var/lib/fleetbench --mode normal
fleetbench-run --results-dir ./out --mode quick --trigger manual
fleetbench-run --results-dir ./out --collector-binary ./target/release/fleetbench
```

Options:

```text
--results-dir <path>          required
--mode quick|normal|long      default: normal
--trigger scheduled|manual    default: scheduled
--collector-binary <path>     default: fleetbench on PATH
```

The runner exits 0 if the file was written successfully, even if the collector itself reported a benchmark failure. The collector's status is preserved inside the envelope. This lets schedulers treat "runner ran" and "benchmark passed" as separate signals.

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

- Should the runner also capture a load sample of its own immediately before invoking the collector, as a sanity check against the collector's own sample? Probably not, but worth deciding.
- Should the envelope record the scheduling source (systemd unit name, cron line, task name) when discoverable? Useful for debugging missed runs.
- On Windows, is `processor_queue_length` worth the implementation cost in v2, or should both load fields simply be `null` until there is a concrete analysis use for them?
- Should the results directory include a `latest.json` symlink/copy for easy local inspection? Convenient but adds a write-ordering concern.
- Is CPython already installed on the Windows performance hosts? If not, the runner needs to ship as a frozen executable (PyInstaller) or be ported to Rust. Porting the runner to Rust is acceptable if needed — the component split stands regardless of language.

## Milestones

### M1: Collector v0 with environment block

- Implement v1 milestones 1 and 2.
- Add `environment.load_before` and `environment.load_after`.
- Bump `schema_version` to 2.

### M2: Python runner

- Implement envelope, atomic write, filename scheme.
- Document systemd timer and Task Scheduler setup.
- Confirm end-to-end on one Linux and one Windows host.

### M3: Collector v1 workloads

- Per v1 milestone 3.

### M4: Collection and analysis handoff

- Separate tool consumes the results directory.
- Analysis layer computes the relative health score.
