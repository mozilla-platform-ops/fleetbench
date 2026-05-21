# Analysis Notes for Fleetbench Data

Notes for whoever builds the downstream analysis layer that ingests envelope
files. Captures lessons from real-world smoke runs on identical-hardware fleet
hosts. Not a spec for the analysis tool, just guidance to save the next
person time.

## Use median, not mean

Across iterations within a single run, and across runs for a single host, use
**median**. Mean is sensitive to single-iteration spikes that are not
host-level signal; median is not.

Concrete example from two identical Xeon E3-1585L v5 hosts, 5 runs of `normal`
mode each:

| Host    | 1t iter 1+ median (n=20) | 1t iter 0 median (n=5) | 1t iter 0 mean (n=5) | 1t iter 0 max |
|---------|--------------------------|------------------------|----------------------|---------------|
| ms-005  | 182.6 ms                 | 183.3 ms               | 183.4 ms             | 183.6 ms      |
| ms-011  | 182.5 ms                 | 184.6 ms               | 186.3 ms             | 194.9 ms      |

The two hosts are physically distinct copies of the same hardware. Their
steady-state medians match to 0.1 ms, which is the desired signal. But ms-011
had a single iter-0 reading of 194.9 ms — likely a brief external interrupt
storm or scheduler jitter, not anything wrong with the host. Mean iter 0 on
ms-011 sees that spike (186.3 ms vs 183.4 ms on ms-005); median does not. Use
median.

## Drop iteration 0 defensively, even when it looks clean

Warmup brings the CPU governor most of the way up before timed iterations
start, but iteration 0 is still the iteration most likely to absorb:

- residual governor ramp (sub-millisecond on healthy hosts, larger on
  power-managed cloud VMs)
- transient external load that started just before our run
- TLB and branch predictor priming

Even when the iter-0 / iter-1+ ratio is 1.00× on aggregate, individual runs
can have a contaminated iter 0. Dropping it costs little (4 of 5 iterations
in `normal` mode still gives a tight median) and removes a known noise source.

Recommendation: skip iter 0 by default. Surface it separately in detailed
views so noise can still be characterized.

## Cross-validate suspicious runs against cpu_counters

Every envelope contains three `cpu_counters` snapshots in
`environment.load_pre_warmup`, `load_pre_timed`, and `load_post_timed`. These
are raw `/proc/stat` jiffy counts (or `GetSystemTimes` 100-ns intervals on
Windows). Differencing two snapshots over any window yields total system CPU
utilization in that window:

```
busy_d = (total_units[end] - total_units[start])
       - (idle_units[end] - idle_units[start])
       - (iowait_units[end] - iowait_units[start])   # optional, kernel-dep
cpu_percent_window = busy_d / (total_units[end] - total_units[start]) * 100
```

The collector's own work dominates the timed window. What matters for
contamination detection is the **excess** above what our run alone would
explain:

```
expected_our_jiffies = sum(iteration_seconds_1t) * HZ
                     + sum(iteration_seconds_mt * threads) * HZ
excess = busy_d_timed_window - expected_our_jiffies
```

If `excess` is materially positive, some other process was running during the
timed window — flag the run as low-confidence even if its numbers look
reasonable. Conversely, a slow run with `excess ~= 0` and `cpu_percent` near
100% across cores is more likely a real host-level problem.

`HZ` on Linux is typically 100 but can vary. Read it from `getconf CLK_TCK` or
hard-code per-platform expectations.

## Compare within hardware class

The collector tags every result with `host.cpu.brand`, `host.cpu.vendor`, and
`host.logical_cpus`. Cross-class comparisons (Xeon E3 vs M4 Pro) are
meaningless and should be refused by the analysis layer with a clear error,
not silently produced. Peer groups for percentile/z-score comparisons must be
homogeneous within a hardware class **and** within a `cpu_suite_version`.

## Health score is a relative metric

The collector deliberately does not emit a single composite score (see design
v2 doc). The analysis layer may compute one, but it must be:

- Relative to the host's peer group, not absolute.
- Tagged with the `cpu_suite_version` it was computed against, since the
  workload mix changes the meaning.
- Recomputable from stored raw timings. If the formula changes, the analysis
  layer reprocesses historical envelopes; the fleet is not re-run.

Median ratio against peer group median is a reasonable starting point:
`score = host_median / peer_median`. Values significantly above 1.0 indicate a
slow host within its class.

## Filename ordering is reliable; timestamps in JSON are authoritative

Envelope filenames embed an ISO-8601-ish UTC timestamp at the start
(`<ts>_<host>_<suite>_<run_id>.json`) so `ls` produces chronological order. The
filename's timestamp is the runner's `started_utc`. Inside the envelope,
prefer `runner.started_utc` and `collector.timestamp_utc` for any analysis
that depends on precise time; filenames may have lower resolution and use a
filename-safe encoding (`:` replaced with `-`).

## Partial files are real

The runner writes envelopes via temp-file-and-rename: `*.json.partial` exists
during writes and is renamed to `*.json` atomically. Collection tooling and
analysis must skip `*.partial` files. A `*.partial` file present for more than
a few seconds indicates a stuck or crashed runner on that host; worth
surfacing as an operational warning, not an analysis input.

## Android phones behave differently — don't reuse desktop heuristics

The "drop iter 0, take median of iters 1+" pattern works on desktops because
their thermal headroom and frequency governors stabilize within milliseconds.
Phones are different. From a single Pixel 10 Pro smoke run (Tensor G5,
8 logical cores), `--mode quick`:

```
1t: [53.6, 27.9, 20.7] ms     # iter 0 slowest; still ramping
mt: [27.0, 87.9, 95.1] ms     # iter 0 fastest; thermal throttling kicks in
```

Two phenomena are stacked here:

1. **Governor ramp is slower on mobile.** The 10⁸ warmup that fully wakes a
   Xeon E3 does not fully wake a Tensor G5; the 1t curve is still trending
   faster at iter 2. Treating iter 0 as one-shot ramp is wrong on phones;
   the ramp can span the entire timed window.

2. **Mobile big.LITTLE + thermal headroom inverts the mt pattern.** Iter 0
   of multi-thread lands on cool big cores and finishes fast. By iter 1
   the SoC is hot and the scheduler is migrating work to LITTLE efficiency
   cores; subsequent iters are 3–4× slower. This is real device behavior,
   not measurement noise — the phone *is* slower under sustained load.

Implications for analysis on Android envelopes:

- **Do not blindly drop iter 0 on mt.** On phones it is often the cleanest
  reading. Take median across all iterations as a first cut, but also
  consider min for "best-case performance" and the slope (last / first) as
  a **thermal-headroom signal**. Two phones with the same median may still
  be worth differentiating if one degrades 2× across iterations and another
  stays flat.

- **Load averages will rarely be ≈0** on a real phone. Pixel 10 Pro idle
  showed `load_1=1.37, load_5=3.25, load_15=4.64` — the phone is not idle,
  Android has a constant trickle of background work. Don't apply a
  desktop-style "non-zero load average means contaminated run" filter; phones
  need a different threshold or a delta-based rule (load went *up* during
  our window, not just "load was non-zero").

- **Run-to-run variance will be larger than on desktops.** A single sample
  is much less reliable. Either require more iterations per run, or more
  runs per host, or both, before trusting comparisons.

- **`host.os_family == "android"`** is the discriminator. The collector
  reports this distinct from `"linux"` even though Android is a Linux
  kernel; analysis should branch on it for any phone-specific logic.

- **Heterogeneous cores are opaque to the schema.** The collector reports
  `logical_cpus = 8` for an 8-core Pixel even though the cores are not
  equivalent. Without per-core data, "mt scales sublinearly" cannot be
  distinguished from "scheduler chose LITTLE cores." Future schema work
  could add per-core frequency capability ranges; until then, treat the
  Android mt timing as a coarser signal than 1t.

## Failure envelopes are envelopes

When the collector fails (correctness check, bad args, runtime error), the
runner still writes an envelope with `collector_output.status = "failed"` and
a populated `error.kind`. Analysis should:

- Treat the file as a real data point (host attempted a run at time X).
- Bucket by `error.kind` for fleet-health monitoring (`correctness_check_failed`
  is much more alarming than `invalid_arguments`).
- Exclude failure envelopes from steady-state timing aggregates.

The runner may also emit envelopes where `collector_output` is `null` because
the collector crashed hard. Those carry `collector_stderr` for diagnostics.
Treat them like other failure envelopes for fleet-health purposes.
