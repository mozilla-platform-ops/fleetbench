# Host-wide Android USB/ADB work log

This is the chronological record for the LambdaTest HyperExecute shared-host
USB/ADB experiments. Use [HOST_WIDE_ADB_RUNBOOK.md](HOST_WIDE_ADB_RUNBOOK.md)
for the current operator procedure; this file intentionally contains no
copy/paste-ready launch command.

## Latest status

| Date | Host | Phase | Version | Outcome | Notes |
|---|---|---|---|---|---|
| 2026-07-28 | `.55` (`test-1`) | Literal Sparky `mozdevice` probe | Try `7757fb…` | Baseline reproduced | Eight bare USB serials / 1,600 raw replicates: 292.01 ms median, 332.39 ms p95, 393.68 ms maximum. This recovered Sparky's ~280 ms baseline, but not the historical LambdaTest tail. |
| 2026-07-28 | Local Pixel 10 Pro | Python vs Fleetbench `mozdevice` | `b7f629b` | Tail parity | Sequential 200-sample probes; Fleetbench p95/p99 matched the literal Python client within 2%/3%, while its median was 16% lower. |
| 2026-07-27 | `.55` (`test-1`) | Corrected bulk rerun | `v0.4.2` | Performance fail | All eight USB serials were selected and full overlap occurred, but 100 MiB push/pull missed the throughput and p95 targets. |
| 2026-07-27 | `.55` (`test-1`) | Corrected latency rerun | `v0.4.2` | Pass, limited contention claim | All-sample latency passed; short transfers reached at most four peer devices. |
| Next | `.55`, then `.47` | Bulk, long push, long pull | `v0.4.3` | Planned | Follow the runbook; long-running directional phases are intended to overcome launch skew. |

The `.47` hub host is a mixed `a55-perf`/`stab` experiment. Compare it with
`.55` only as an external reference, not as a controlled before/after result.

## Results

### 2026-07-28 — literal `mozdevice` validation

The earlier `mozdevice`-mode Fleetbench result was too low because it omitted
parts of `ADBDevice.push()`. A dedicated runner fetched the `mozdevice` source
from Sparky's Try revision `7757fbcccc8eb83105af2b9518517f47dcca9eff` and ran
the original 25-byte, 200-iteration Python loop against every selected `.55`
phone. All eight jobs used bare USB serials and emitted the expected Perfherder
replicate artifacts.

| Scope | Samples | Mean | Median | p95 | p99 | Maximum |
|---|---:|---:|---:|---:|---:|---:|
| `.55` literal Python `mozdevice` | 1,600 | — | 292.01 ms | 332.39 ms | 347.42 ms | 393.68 ms |

This reproduces Sparky's approximate 280 ms baseline and tight BitBar-like
distribution; it does not reproduce the historical LambdaTest tail to 1,600
ms. The literal Perfherder output has no per-transfer timestamps, so it cannot
establish actual eight-way transfer overlap after each job's independent Python
dependency setup.

#### Local Pixel 10 Pro comparison

After Fleetbench was updated to time the same external-storage command path
(`sync → test -d → push → first-call storage discovery → sync`), a local,
sequential 200-sample comparison was run on the attached Pixel 10 Pro. The
Python run was completed first, including its deferred cleanup, then Fleetbench
ran with `--direction push --push-mode mozdevice` against the same 25-byte
`/sdcard/Download` workload.

| Implementation | Samples | Mean | Median | p95 | p99 | Maximum |
|---|---:|---:|---:|---:|---:|---:|
| Literal Python `mozdevice` | 200 | 474.06 ms | 497.56 ms | 592.11 ms | 606.51 ms | 943.24 ms |
| Fleetbench Rust `mozdevice` (`b7f629b`) | 200 | 408.60 ms | 415.68 ms | 580.66 ms | 621.53 ms | 658.20 ms |

Fleetbench was 16% lower at the median, but p95 and p99 were within 2% and 3%
of the literal client. Treat this as command-path fidelity evidence, not a
controlled performance comparison: the probes were sequential and the Python
client's verbose logging and local runtime behavior remain inside its timing
boundary.

The remaining central-tendency difference is expected. Fleetbench reproduces
the ADB command sequence, but the literal client also times Python's
`subprocess`/polling implementation, temporary-file path handling, and verbose
`mozlog` work after each ADB command. Fleetbench uses Rust process handling and
its own random payload and remote filenames instead. In addition, the Python
probe ran first, so device/host state could have changed before Fleetbench ran.
Neither result isolates those host-side costs; a deliberately alternating,
repeated comparison would be needed to attribute the 16% median gap.

### 2026-07-27 — `.55` corrected-timing reruns (`v0.4.2`)

`v0.4.1` performed checksum verification between timed transfers. `v0.4.2`
deferred remote push and local pull verification until the end of each timed
loop, retaining validation without pacing the next transfer. Do not pool the
older `v0.4.1` measurements with this corrected-timing round.

| Phase | Label | Outcome | Evidence |
|---|---|---|---|
| Bulk, first attempt | `fleetbench-usb-bulk-v0.4.2-rerun-10.146.2.55` | Invalid — stop condition | 24 successful envelopes and 16,800 valid transfers, but the target `RZCXC19G1DM` selected `10.146.6.13:5555` (TCP ADB). Preserve only as diagnostic evidence. |
| One-device latency smoke | `fleetbench-usb-latency-smoke-v0.4.2-10.146.2.55` | Pass | Three envelopes / 2,400 valid transfers; `RZCXC19G1DM` selected its bare USB serial. |
| Bulk rerun | `fleetbench-usb-bulk-v0.4.2-rerun-10.146.2.55` | Performance fail | 24 envelopes / 16,800 valid transfers; all eight expected bare serials were selected. |
| Latency rerun | `fleetbench-usb-latency-v0.4.2-rerun-10.146.2.55` | Pass, limited contention claim | 24 envelopes / 19,200 valid transfers; all eight expected bare serials were selected. |

#### Bulk rerun

The valid rerun produced 55 push and 59 pull 100 MiB samples with all seven
peer devices overlapping. It improved over the original run, but still failed
both directional acceptance targets.

| Cohort | Direction | Samples | Median throughput | p95 elapsed | Status |
|---|---|---:|---:|---:|---|
| All overlap levels | Push | 480 | 13.89 MiB/s | 11.92 s | Fail |
| All overlap levels | Pull | 480 | 8.37 MiB/s | 12.91 s | Fail |
| Seven peers | Push | 55 | 13.98 MiB/s | 7.30 s | Fail |
| Seven peers | Pull | 59 | 13.47 MiB/s | 7.70 s | Fail |

The version-to-version improvement is observational, not causal proof: it was
a fresh host run, not a controlled experiment.

#### Latency rerun

| Size | Samples | Mean | Median | p95 | p99 | Maximum | Status |
|---|---:|---:|---:|---:|---:|---:|---|
| 25 B | 9,600 | 16.28 ms | 12.96 ms | 32.19 ms | 52.22 ms | 123.30 ms | Pass |
| 50 KiB | 9,600 | 17.37 ms | 13.75 ms | 31.02 ms | 36.63 ms | 61.43 ms | Pass |

The 25-byte result passes the 375 ms mean, 500 ms p95, and 750 ms p99
criteria. The 50 KiB result passes the 1 s p95 floor and 500 ms preferred
target. However, 25-byte and 50 KiB transfers reached at most four peers, so
these distributions do not support a full-eight-device tail-latency claim.

This did not reproduce Sparky's high-latency tail: the short transfer windows
were about 10–30 ms, while Sparky's 100-retrigger push-only probe had
280–1,600 ms windows. The long-running directional phases in the current
runbook are the follow-up experiment.

### 2026-07-22 — `.47` USB-hub experiment (`v0.4.1`)

The vendor added a USB hub to `10.146.2.47` on 2026-07-22. This host contains
seven `a55-perf` devices and `RZCY10Y548K` from `stab`; its outcome is therefore
an external reference only.

#### Standalone smoke

The `stab` device completed three bulk loops with 2,100 valid, checksum-verified
transfers. This was a post-hub standalone baseline, not a saturation result.

| Direction | Samples | Median throughput | p95 elapsed |
|---|---:|---:|---:|
| Push | 60 | 33.68 MiB/s | 3.08 s |
| Pull | 60 | 35.44 MiB/s | 2.89 s |

#### Eight-device bulk

All eight jobs completed successfully with 16,800 valid, checksum-verified
transfers. 2,510 records overlapped all seven peers.

| Cohort | Direction | Samples | Median throughput | p95 elapsed | Status |
|---|---|---:|---:|---:|---|
| All overlap levels | Push | 480 | 24.29 MiB/s | 5.24 s | Mixed |
| All overlap levels | Pull | 480 | 20.57 MiB/s | 5.67 s | Mixed |
| Seven peers | Push | 231 | 21.17 MiB/s | 5.68 s | Mixed |
| Seven peers | Pull | 234 | 19.93 MiB/s | 5.83 s | Mixed |

At full overlap, push cleared the 20 MiB/s floor while pull missed it by
0.07 MiB/s; both directions exceeded the approximately 5 s p95 target.
Relative to the standalone smoke, median throughput fell 37% for push and 44%
for pull. This is evidence consistent with the hub helping, not causal proof.

#### Eight-device latency

All eight jobs completed successfully with 19,200 valid, checksum-verified
transfers. Eight 25-byte transfers overlapped all seven peers; no 50 KiB
transfer did, so the full-contention tail evidence is sparse.

| Size | Samples | Mean | Median | p95 | p99 | Maximum | Status |
|---|---:|---:|---:|---:|---:|---:|---|
| 25 B | 9,600 | 18.01 ms | 17.33 ms | 32.82 ms | 52.87 ms | 74.16 ms | Pass |
| 50 KiB | 9,600 | 19.36 ms | 16.10 ms | 33.17 ms | 38.70 ms | 76.81 ms | Pass |

### 2026-07-21 — `.55` original saturation run (`v0.4.1`)

All eight jobs completed for both phases, with valid timestamps and checksums.
These figures predate the timing correction and are retained for comparison,
not pooled with the `v0.4.2` reruns.

#### Bulk

5,307 records overlapped all seven peers. The one-device smoke baseline on
`R5CXC1ARZDN` recorded 34.23 MiB/s push and 31.32 MiB/s pull medians, with p95
of 2.97 s and 3.25 s respectively. It was an artifact-validation smoke, not a
controlled baseline.

| Cohort | Direction | Samples | Median throughput | p95 elapsed | Status |
|---|---|---:|---:|---:|---|
| All overlap levels | Push | 480 | 8.86 MiB/s | 11.93 s | Fail |
| All overlap levels | Pull | 480 | 8.41 MiB/s | 12.94 s | Fail |
| Seven peers | Push | 371 | 8.86 MiB/s | 11.75 s | Fail |
| Seven peers | Pull | 253 | 9.09 MiB/s | 12.87 s | Fail |

#### Production-path latency

| Size | Samples | Mean | Median | p95 | p99 | Maximum | Status |
|---|---:|---:|---:|---:|---:|---:|---|
| 25 B | 9,600 | 17.56 ms | 17.06 ms | 32.16 ms | 51.46 ms | 229.71 ms | Pass |
| 50 KiB | 9,600 | 19.98 ms | 18.41 ms | 33.71 ms | 49.80 ms | 461.58 ms | Pass |

No 25-byte transfer and only nine 50 KiB transfers overlapped all seven peers.
Treat the all-sample latency figures as reliable distributions, not as a
full-contention tail result.

## Historical planning and command archive

The following is provenance, not current instruction. The complete original
copy/paste command record is preserved in the parent of the documentation
split: `git show dafa65c:docs/HOST_WIDE_ADB_TEST_PLAN.md`. Do not reuse those
commands; use the current runbook instead.

### Original experiment scope

The original plan measured end-to-end ADB push/pull with every selected phone
on a Docker host active simultaneously. It required direct repeated
`--device <serial>` selection, one host per batch, `--retries 0`, verified
release artifacts, and timestamp-based overlap analysis. It was explicitly not
a DevicePool migration.

The 2026-07-16 host mapping snapshot was:

| Docker host | Target serials |
|---|---|
| `10.146.2.54` | `R5CXC1PW94F`, `RZCXC187YCR`, `RZCXC16WHTA`, `RZCXC16W6KT`, `R5CXC1PW7CR`, `R5CXC1HZ4KD`, `R5CXC1ASH4E`, `R5CXC1AHZBW` |
| `10.146.2.53` | `R5CXC1ASHNJ`, `R5CXC1AHXYD`, `R5CXC1HZ5PZ`, `R5CXC1AHWWZ`, `RZCXC15YZVZ`, `R5CXC1AJ07K`, `RZCXC189JSJ`, `RZCXC19G1CT` |
| `10.146.2.48` | `R5CY21T22NH`, `RZCX23RT6WR`, `R5CXC1AMNFY`, `RZCX31FDGJE`, `RZCX71ZVF6J`, `R5CX23RTKSK`, `RZCY204AAZD`, `RZCX50TW03H` |
| `10.146.2.55` (`test-1`) | `R5CXC1HZA6V`, `R5CXC1ARZDN`, `R5CXC1HZ43J`, `R5CXC1HZ85W`, `R5CXC1SXMVR`, `RZCXC19G1DM`, `RZCXC1BK67D`, `RZCY107MCLV` |
| `10.146.2.47` (hub) | `RZCY10Y4TJX`, `RZCY10Y4TBY`, `RZCY10Y4TAV`, `RZCY10Y4QVX`, `RZCY10Y4HWD`, `RZCY10LGB6W`, `RZCX821GXDJ`, plus `RZCY10Y548K` from `stab` |

### Historical command sets

| Period | Launchers | Version | Interpretation |
|---|---|---|---|
| Original `.55` / `.47` saturation | `run_bulk.sh`, `run_latency.sh` | `v0.4.1` | Historical only; verification occurred between timed transfers. |
| Corrected `.55` reruns | `run_bulk.sh`, `run_latency.sh` | `v0.4.2` | Historical only; results are recorded above. |
| Planned directional reproduction | `run_push_only.sh` (formerly `run_sparky_push_only.sh`), `run_pull_only.sh` | Explicit direction-capable release | Superseded by the current `v0.4.3` runbook. |

Historical artifacts remain in the corresponding
`~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/<timestamp>/` directories.

## Reference

### Analysis and acceptance criteria

Build a transfer window from each iteration's `transfer_started_at_utc` and
`transfer_finished_at_utc`. A sample is contended only when it overlaps a
transfer on another device; task start time is not evidence of contention.

Report per-device and aggregate results by observed peer-overlap level,
including the largest cohort, every checksum failure/disconnect/retry/missing
artifact, and these acceptance references:

| Workload | Acceptance reference |
|---|---|
| 100 MiB push/pull | Median throughput at least 20 MiB/s; preferred 25–32 MiB/s; p95 elapsed about 5 s or less. |
| 25 B latency | Mean at most 375 ms; p95 at most 500 ms; p99 at most 750 ms. |
| 50 KiB latency | p95 at most 1 s; 500 ms preferred. |

### Stop conditions

Preserve the artifacts and invalidate the current host batch if any job selects
TCP ADB, reports a serial other than its `--device` target, fails SHA-256
verification, loses a device, or omits transfer timestamps. Investigate first,
then rerun the entire host; never merge it with a clean batch.
