# Fleetbench Minimal Collector Design

## Summary

Fleetbench is a small cross-platform benchmark collector for Linux and Windows performance-testing hosts. Its purpose is to collect deterministic CPU benchmark timings and host metadata so downstream systems can identify extreme outliers and quantify fleet performance within comparable machine classes.

Fleetbench does **not** attempt to compare unlike hardware classes, generate authoritative public benchmark scores, or perform analysis itself. The collector emits raw, versioned JSON suitable for later ingestion, aggregation, and outlier detection.

## Goals

- Provide a simple CPU-focused collector that runs on Linux and Windows.
- Emit stable, machine-readable JSON output.
- Collect raw per-iteration benchmark timings rather than only summary statistics.
- Use deterministic workloads with correctness checks where practical.
- Support phased expansion from a minimal v0 to a broader CPU v1 suite.
- Keep runtime short enough for regular fleet collection.
- Avoid external services or network dependencies during benchmark execution.
- Avoid requiring administrator/root privileges.

## Non-goals

- No cross-machine-class scoring in the collector.
- No fleet-wide analysis, dashboards, or outlier detection in the collector.
- No attempt to produce a public benchmark comparable to Geekbench, PassMark, Cinebench, etc.
- No browser, graphics, disk, or network benchmarks in the initial collector.
- No thread pinning or platform-specific scheduler control in v0.
- No tuning for maximum theoretical CPU performance.

## Design Principles

1. **Boring and repeatable over comprehensive**

   The collector should prefer deterministic, easy-to-debug benchmarks over complex synthetic workloads.

2. **Raw data first**

   The collector should emit per-iteration timings. Aggregation and scoring can change later without requiring fleet reruns.

3. **Version everything**

   Output schema version, benchmark binary version, benchmark suite version, and workload configuration should all be explicit.

4. **Correctness matters**

   Benchmarks that produce verifiable outputs should validate them and fail loudly on mismatches.

5. **Analysis stays elsewhere**

   The collector should not decide whether a host is bad. It should only collect high-quality data.

## Implementation Language

Fleetbench should be implemented in Rust.

Rust provides:

- Native performance.
- Good Linux and Windows support.
- Easy static or near-static distribution.
- Strong type safety.
- Good CLI and JSON ecosystem.
- Low runtime dependency burden.

Suggested crates:

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
hostname = "0.4"
num_cpus = "1"
sysinfo = "0.30"
rayon = "1"
```

Additional crates may be added in later versions for specific workloads:

```toml
blake3 = "1"
zstd = "0.13"
```

## Command Line Interface

Initial commands:

```bash
fleetbench inspect --json
fleetbench cpu --mode quick --json
fleetbench cpu --mode normal --json
fleetbench cpu --limit 100000000 --iterations 5 --threads auto --json
```

### `inspect`

Collects host and CPU metadata without running a benchmark.

Example:

```bash
fleetbench inspect --json
```

### `cpu`

Runs CPU benchmark workloads and emits JSON.

Example:

```bash
fleetbench cpu --mode normal --json
```

Options:

```text
--mode quick|normal|long
--limit <N>
--iterations <N>
--threads <N|auto>
--json
```

`--limit`, `--iterations`, and `--threads` override mode defaults.

## Benchmark Modes

Modes provide stable preset configurations. The exact values should be versioned and should not change silently within a benchmark suite version.

### Quick

Purpose: short health check suitable for frequent collection.

Suggested configuration:

```text
prime_limit = 10,000,000
iterations = 3
threads = logical CPU count
```

### Normal

Purpose: default fleet collection mode.

Suggested configuration:

```text
prime_limit = 100,000,000
iterations = 5
threads = logical CPU count
```

### Long

Purpose: deeper diagnosis or less frequent collection.

Suggested configuration:

```text
prime_limit = 1,000,000,000
iterations = 3
threads = logical CPU count
```

## Benchmark Suite Versions

Fleetbench should evolve in explicit benchmark suite versions. The collector binary version and benchmark suite version are related but distinct.

Example:

```json
{
  "collector_version": "0.1.0",
  "cpu_suite_version": "cpu-v0"
}
```

## CPU Suite v0

CPU v0 is the minimal useful collector.

### Included workloads

```text
prime_sieve_1t
prime_sieve_mt
```

### Purpose

CPU v0 is intended to catch obvious CPU outliers with a small amount of implementation complexity.

It measures:

- Single-thread integer/cache performance.
- Multi-thread scaling.
- Basic scheduler behavior.
- Obvious throttling or bad CPU governor behavior.
- Incorrect logical CPU exposure.

### Workload: `prime_sieve_1t`

A single-threaded sieve of Eratosthenes or segmented sieve over a fixed range.

Measures:

- Single-core integer performance.
- Cache behavior.
- CPU frequency behavior.

Correctness:

Known limits should be validated against known prime counts.

Useful known values:

```text
pi(10,000,000) = 664,579
pi(100,000,000) = 5,761,455
pi(1,000,000,000) = 50,847,534
```

### Workload: `prime_sieve_mt`

A multi-threaded segmented sieve.

Suggested approach:

1. Compute base primes up to `sqrt(limit)`.
2. Split the target range into chunks.
3. Sieve chunks in parallel using a fixed worker count.
4. Sum prime counts.
5. Validate total prime count for known limits.

Measures:

- All-core CPU throughput.
- Parallel scaling.
- Scheduler behavior.
- Thermal behavior under sustained CPU load.

### v0 JSON result fields

```json
{
  "results": {
    "prime_sieve_1t": {
      "iterations": [
        { "seconds": 4.21, "prime_count": 5761455 },
        { "seconds": 4.19, "prime_count": 5761455 }
      ]
    },
    "prime_sieve_mt": {
      "threads": 32,
      "iterations": [
        { "seconds": 0.41, "prime_count": 5761455 },
        { "seconds": 0.40, "prime_count": 5761455 }
      ]
    }
  }
}
```

## CPU Suite v1

CPU v1 expands beyond primes to provide a more balanced CPU signal while remaining compact.

### Included workloads

```text
prime_sieve_1t
prime_sieve_mt
blake3_1t
blake3_mt
zstd_1t
fp_vector_1t
stability_loop
```

### Purpose

CPU v1 is intended to capture a broader range of CPU behavior without turning Fleetbench into a large benchmark suite.

It adds coverage for:

- Hash/integer/vector throughput.
- Compression-style real-world CPU work.
- Floating-point and SIMD-heavy execution.
- Repeated-run stability and noise.

### Workload: `blake3_1t`

Hash deterministic generated data using BLAKE3 in a single thread.

Measures:

- Integer throughput.
- SIMD/vectorized hashing paths.
- Tight-loop CPU behavior.

The input should be generated deterministically and should avoid disk I/O.

### Workload: `blake3_mt`

Hash deterministic generated data using multiple threads.

Measures:

- Parallel CPU scaling.
- Hash throughput across available logical CPUs.

### Workload: `zstd_1t`

Compress deterministic generated data using zstd in a single thread.

Measures:

- Real-world-ish CPU behavior.
- Branch-heavy integer work.
- Cache and memory access patterns.

The input should be generated in memory. No file I/O should be included in the timed section.

### Workload: `fp_vector_1t`

Run a deterministic floating-point/vector math loop, such as a dot product or small matrix multiply.

Measures:

- Floating-point throughput.
- SIMD/FMA behavior where available.
- Compiler target differences.

Correctness should be checked using a tolerance rather than exact floating-point equality.

### Workload: `stability_loop`

Run a short benchmark repeatedly and emit all timings.

The stability loop may reuse one of the existing workloads, for example a smaller `prime_sieve_1t` or hash workload.

Measures:

- Performance variance.
- Background load.
- Thermal ramp-down.
- Power-management instability.
- Noisy-neighbor behavior in virtualized environments.

The collector should not calculate an outlier status, but it may emit basic summary values such as median and coefficient of variation if raw timings are also included.

## Future CPU Suite v2

Potential additions:

```text
memory_bandwidth
memory_latency
build-like workload
browser-specific workload
JavaScript engine workload
disk read/write microbenchmarks
```

These should remain separate from CPU v0/v1 to avoid changing the meaning of existing results.

## JSON Output

The collector should emit a single JSON object per run.

Example:

```json
{
  "schema_version": 1,
  "collector_version": "0.1.0",
  "cpu_suite_version": "cpu-v0",
  "timestamp_utc": "2026-05-20T00:00:00Z",
  "status": "ok",
  "host": {
    "hostname": "linux-perf-123",
    "os_family": "linux",
    "os_version": "Ubuntu 24.04",
    "kernel_version": "6.8.0-xx",
    "arch": "x86_64",
    "logical_cpus": 32,
    "physical_cpus": 16
  },
  "cpu": {
    "brand": "AMD EPYC ...",
    "vendor": "AuthenticAMD",
    "frequency_mhz": 3200
  },
  "config": {
    "command": "cpu",
    "mode": "normal",
    "prime_limit": 100000000,
    "iterations": 5,
    "threads": 32
  },
  "results": {
    "prime_sieve_1t": {
      "iterations": [
        { "seconds": 4.21, "prime_count": 5761455 },
        { "seconds": 4.19, "prime_count": 5761455 },
        { "seconds": 4.24, "prime_count": 5761455 },
        { "seconds": 4.20, "prime_count": 5761455 },
        { "seconds": 4.22, "prime_count": 5761455 }
      ]
    },
    "prime_sieve_mt": {
      "threads": 32,
      "iterations": [
        { "seconds": 0.41, "prime_count": 5761455 },
        { "seconds": 0.40, "prime_count": 5761455 },
        { "seconds": 0.42, "prime_count": 5761455 },
        { "seconds": 0.41, "prime_count": 5761455 },
        { "seconds": 0.40, "prime_count": 5761455 }
      ]
    }
  }
}
```

## Failure Output

On failure, Fleetbench should still emit JSON when `--json` is used.

Example:

```json
{
  "schema_version": 1,
  "collector_version": "0.1.0",
  "cpu_suite_version": "cpu-v0",
  "timestamp_utc": "2026-05-20T00:00:00Z",
  "status": "failed",
  "error": {
    "kind": "correctness_check_failed",
    "message": "prime count mismatch for limit 100000000: expected 5761455, got 5761454"
  }
}
```

Exit codes:

```text
0: success
1: invalid arguments or runtime error
2: benchmark correctness check failed
```

## Timed Section Rules

Timed benchmark sections should exclude:

- JSON serialization.
- Host metadata collection.
- Argument parsing.
- Disk I/O.
- Network I/O.
- Test data generation when practical, unless data generation is explicitly part of the workload definition.

Timed sections may include:

- CPU computation.
- Memory allocation that is inherent to the algorithm.
- Thread scheduling and coordination for multi-threaded workloads.

## Warm-up

Each benchmark command should perform a short warm-up before recording timings.

The warm-up should:

- Run a smaller deterministic workload.
- Not be included in results.
- Help reduce first-run effects from CPU frequency ramp-up, page faults, and cold code paths.

The warm-up should be recorded in config metadata, for example:

```json
{
  "config": {
    "warmup_enabled": true,
    "warmup_prime_limit": 1000000
  }
}
```

## Threading

Default behavior:

```text
threads = logical CPU count
```

The user may override the thread count:

```bash
fleetbench cpu --threads 16 --json
```

Thread pinning is intentionally excluded from v0. It may be considered later if needed, but the initial collector should measure the host as the OS schedules normal workloads.

## Host Metadata

The collector should gather enough metadata to allow downstream grouping and debugging.

Minimum host metadata:

```text
hostname
OS family
OS version
kernel version or Windows build
architecture
logical CPU count
physical CPU count when available
CPU brand/model
CPU vendor
reported CPU frequency when available
collector version
benchmark suite version
```

Optional future metadata:

```text
memory size
virtualization detected yes/no
BIOS/firmware version
CPU governor on Linux
power plan on Windows
microcode version
NUMA node count
```

## Reproducibility

- Workloads must use deterministic input data.
- Random data, if used, must be generated from a fixed seed.
- Benchmark presets must be stable within a suite version.
- Changing workload behavior should require a new suite version.

## Security and Safety

- The collector should not require elevated privileges.
- The collector should not make network requests.
- The collector should not execute arbitrary commands.
- The collector should not write files unless explicitly requested by the user.
- JSON output should be safe to redirect to a file or ingestion tool.

## Open Questions

- Should v0 use a simple sieve or segmented sieve for both single-thread and multi-thread tests?
- Should `normal` mode target a fixed runtime instead of a fixed prime limit?
- Should CPU v1 be implemented immediately or after validating v0 collection in the fleet?
- Should the collector include optional summary statistics in addition to raw timings?
- Should Windows-specific metadata include active power plan in v1?
- Should Linux-specific metadata include CPU governor in v1?

## Recommended Initial Milestones

### Milestone 1: CPU v0 prototype

- Implement CLI.
- Implement metadata collection.
- Implement `inspect --json`.
- Implement `prime_sieve_1t`.
- Implement `prime_sieve_mt`.
- Emit raw per-iteration JSON.
- Validate known prime counts.

### Milestone 2: Cross-platform packaging

- Build Linux binary.
- Build Windows binary.
- Run on representative hosts.
- Confirm JSON schema stability.
- Confirm runtime of quick and normal modes.

### Milestone 3: CPU v1 expansion

- Add BLAKE3 single-thread and multi-thread tests.
- Add zstd single-thread compression test.
- Add floating-point/vector test.
- Add stability loop.
- Preserve CPU v0 suite behavior.

### Milestone 4: Collection integration

- Run via existing fleet orchestration.
- Store JSON artifacts.
- Hand off raw output to downstream analysis.

## Recommendation

Start with CPU v0 to validate the collector, schema, packaging, and fleet execution path. Then add CPU v1 workloads once the collection pipeline is reliable.

The minimal useful path is:

```text
cpu-v0:
  prime_sieve_1t
  prime_sieve_mt

cpu-v1:
  prime_sieve_1t
  prime_sieve_mt
  blake3_1t
  blake3_mt
  zstd_1t
  fp_vector_1t
  stability_loop
```

This keeps the first implementation small while preserving a clear path to a more representative CPU signal.
