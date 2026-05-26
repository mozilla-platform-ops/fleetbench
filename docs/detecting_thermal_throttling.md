# Detecting Thermal Throttling

How to use `fleetbench` to determine whether a host (typically a Windows NUC)
is thermally throttling under sustained CPU load. Companion runbook to
`--duration` torture mode and the `frequency_series` envelope field.

## TL;DR

```powershell
# Windows
curl.exe -L -o fleetbench.exe `
  https://github.com/mozilla-platform-ops/fleetbench/releases/latest/download/fleetbench-vX.Y.Z-windows-x86_64.exe
.\fleetbench.exe cpu --duration 60s --no-warmup --mode quick --json > result.json
```

Then either send `result.json` back, or run the at-a-glance script in
[Reading the output](#reading-the-output) below.

Linux equivalent: same flags, swap the binary name. macOS data is not
diagnostic — see [Platform support](#platform-support).

## What this measures

A `--duration` run loops the MT prime sieve until the wall-clock duration
elapses. While that runs, a background sampler reads per-core CPU frequency
at ~1Hz into `frequency_series`. Throttling is visible as:

- Per-core frequency that starts near the chip's boost clock and decays
  toward base clock over the run, OR
- Per-core frequency that drops below base clock (true throttle), OR
- Per-core variation where some cores throttle while others don't
  (uneven cooling, degraded thermal interface on one part of the die).

The integer sieve workload is a portable baseline — it pegs all cores to
100% utilization but uses no AVX/FMA, so it produces *less* package power
than prime95's torture mode. That means: this test will catch hosts with
gross thermal problems (degraded paste, dead fan, blocked vent) reliably,
but may miss the marginal AVX-offset throttling that only shows up under
floating-point-heavy workloads.

If a 5-10 minute fleetbench torture run looks clean but prime95 still
flags throttling on the same host, that's evidence we need the AVX/FMA
workload (tracked as `fleetbench-jfc`).

## Choosing the duration

| Duration | What it surfaces |
|---|---|
| `--duration 60s` | Gross thermal failures (dead fan, blocked vent, badly degraded paste). Healthy chips finish at boost. |
| `--duration 5m` | Marginal cooling issues. Most NUCs that are going to throttle will start by 2-3 min. |
| `--duration 15m` | Steady-state thermal envelope. Use when 5m runs look clean but the host is suspected of intermittent throttling. |

`--no-warmup` is recommended: it makes the freq series start on a cold chip
so you see the full boost → steady-state trajectory, not just the post-warmup
tail.

`--mode quick` is the recommended pairing with `--duration` — short
per-iteration size (~tens of ms on a modern NUC) gives a dense
iteration-timing series alongside the 1Hz freq samples. See
[the README](../README.md#choosing-a-mode) for why `--mode long` is the
wrong default for torture runs.

## Reading the output

The interesting field is `frequency_series`: an array of samples, each
containing `t_offset_seconds`, `per_core_mhz` (a vector indexed by logical
CPU), and `mean_mhz`.

### At-a-glance PowerShell summary

This collapses the per-core array into P-core and E-core min/max per
second, which is enough to spot throttling visually on hybrid Intel chips:

```powershell
$o = Get-Content result.json | ConvertFrom-Json
"build: $($o.collector_version)+$($o.collector_git_sha)"
"host:  $($o.host.hostname)  $($o.cpu.brand)  ($($o.host.logical_cpus) logical cores)"
"iters: $($o.results.prime_sieve_mt.iterations.Count) in $($o.config.duration_seconds)s"
""
"t(s)   mean   p_min  p_max   e_min  e_max"
$o.frequency_series | ForEach-Object {
  # i5-1340P layout: 0-7 are P-core threads, 8-15 are E-cores.
  # Adjust the slice indices if your chip has a different layout.
  $p = $_.per_core_mhz[0..7]
  $e = $_.per_core_mhz[8..15]
  "{0,5:F1}  {1,5}  {2,5} {3,5}   {4,5} {5,5}" -f `
    $_.t_offset_seconds, $_.mean_mhz, `
    ($p | Measure-Object -Minimum).Minimum, ($p | Measure-Object -Maximum).Maximum, `
    ($e | Measure-Object -Minimum).Minimum, ($e | Measure-Object -Maximum).Maximum
}
```

### Interpreting the table

| Pattern | What it means |
|---|---|
| `mean` starts 2500-3500, drops to 1500-1900 by t=30-60s | **Throttling.** Healthy chips boost early, decay as package heats. The size of the drop indicates severity. |
| `mean` stays high (≥2500) for the whole run | **No throttling detected.** Cooling is fine on this host. |
| `mean` is low (500-1500) throughout | Either severe thermal throttle from t=0, OR the workload isn't pegging cores. Cross-check `iters` — should be in the thousands on a modern NUC. |
| `p_max` near boost clock early, drops to ~base sustained | Classic boost → base thermal envelope. Mild thermal pressure. |
| `p_min` drops well below base clock mid-run | Aggressive per-core throttling. Almost always thermal. |
| Wide `p_min`/`p_max` spread within a single sample | Per-core variation — uneven cooling (one corner of the die hotter than others) or normal OS scheduling artifact. Compare across runs. |

The first 1-2 samples of any run will show artificially low values
because the workload's rayon thread pool takes a brief moment to ramp up.
Ignore samples before the iteration count starts climbing meaningfully.

## What to send back

Easiest: attach `result.json`. The whole file is typically 50-200 KB for a
1-5 minute run, fits anywhere.

If the file is awkward to transfer, paste the output of the at-a-glance
script above — that's enough to tell the story without the per-core detail.

## Platform support

| Platform | Frequency signal | Notes |
|---|---|---|
| Windows | **Reliable.** PDH counter `\Processor Information(*)\% Processor Performance` multiplied by per-core base. | The primary fleetbench thermal-debugging target. |
| Linux | **Reliable.** sysinfo reads `/proc/cpuinfo` per-core. | Works correctly on Xeon E3 fleet hosts. |
| macOS (Apple Silicon) | **Not diagnostic.** sysinfo does not expose per-core frequency. | `frequency_series` will populate but values are placeholders. Don't use macOS results for thermal analysis. |
| Android | Not tested for thermal debugging. SoCs have their own governor behavior that needs separate treatment. |

If the Windows binary cannot initialize PDH (rare; usually permission or
counter-registry issues), the sampler falls back to the broken sysinfo
behavior and reports the base frequency every sample. That's visible in
the output: every `per_core_mhz` value will be identical across all
samples. If you see that, PDH didn't start — check that the binary is
v0.3.0 or later and that the user account has permission to read
performance counters.

## Comparison to prime95 / StressCPU.ps1

`fleetbench` is the *baseline* tool — cross-platform, reproducible per-host,
fleet-deployable. It surfaces gross thermal failures.

prime95 torture mode (as wrapped by `worker-images/.../stress_test/StressCPU.ps1`)
is the *forensics* tool — Windows-only, AVX/FMA-heavy, produces the
absolute thermal worst-case load. Use prime95 when fleetbench shows clean
but a host is still suspected of marginal throttling under real workloads.

These tools are complementary, not redundant. A useful workflow:

1. Use fleetbench across the whole fleet to find hosts that throttle.
2. For ambiguous cases, run prime95 locally on the suspect NUC.
3. If a host throttles under prime95 but not fleetbench, file an issue
   noting the gap — that's evidence to prioritize `fleetbench-jfc`
   (AVX/FMA torture workload).
