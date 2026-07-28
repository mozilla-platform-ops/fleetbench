# Host-wide Android USB/ADB test plan

Status: execution plan for the LambdaTest HyperExecute saturation experiment.

## Goal

Measure end-to-end ADB push/pull behavior when every selected `a55-perf` phone
on one physical Docker host is active at once. The primary signal is per-device
throughput and tail latency during intervals where transfers on the other phones
actually overlap.

This is not a DevicePool migration. Launch temporary HyperExecute jobs directly
with `lt_run_cmd`, one long-lived job for each serial on a single host. The
phones return to normal Taskcluster scheduling after the jobs end.

## Safety rules

- Target devices **only** with repeated `--device <serial>` arguments. Do not
  use `--group` or `--all` for this experiment.
- Target only serials assigned to `a55-perf` in `lambdatest.yml`. Do not borrow
  devices from `p9-perf`, `a55-alpha`, or `stab` to fill a host.
- Exception: the explicitly labeled `10.146.2.47` hub experiment includes
  `RZCY10Y548K` from `stab` with seven `a55-perf` devices on the same host.
  Keep its results separate from the standard `a55-perf` batches.
- The `test-1` host `10.146.2.55` is eligible for this experiment because its
  devices are currently idle. It remains a `test-1` host; target only the
  eight serials listed below and do not use `--group` to select it.
- Run one host batch at a time. Do not combine serials from different hosts in a
  single launch; that would measure fleet-wide concurrency, not shared-host USB
  contention.
- Use `FLEETBENCH_VERSION=v0.4.2` or later. This is the first release with
  contiguous timed ADB push/pull loops and per-transfer timestamps needed to
  prove overlap.
- Use `--retries 0`. A retried job changes the concurrency pattern and must be
  treated as a separate, failed attempt rather than silently folded into a run.

## Device batches

The source is `~/Desktop/PowerMeter-Container-mapping - Container_name.csv`,
cross-checked against `~/git/mozilla-bitbar-devicepool/config/lambdatest.yml`
on 2026-07-16. Each batch has eight phones attached to one Docker host.

| Docker host | `a55-perf` target serials |
|---|---|
| `10.146.2.54` | `R5CXC1PW94F`, `RZCXC187YCR`, `RZCXC16WHTA`, `RZCXC16W6KT`, `R5CXC1PW7CR`, `R5CXC1HZ4KD`, `R5CXC1ASH4E`, `R5CXC1AHZBW` |
| `10.146.2.53` | `R5CXC1ASHNJ`, `R5CXC1AHXYD`, `R5CXC1HZ5PZ`, `R5CXC1AHWWZ`, `RZCXC15YZVZ`, `R5CXC1AJ07K`, `RZCXC189JSJ`, `RZCXC19G1CT` |
| `10.146.2.48` | `R5CY21T22NH`, `RZCX23RT6WR`, `R5CXC1AMNFY`, `RZCX31FDGJE`, `RZCX71ZVF6J`, `R5CX23RTKSK`, `RZCY204AAZD`, `RZCX50TW03H` |
| `10.146.2.55` (`test-1`) | `R5CXC1HZA6V`, `R5CXC1ARZDN`, `R5CXC1HZ43J`, `R5CXC1HZ85W`, `R5CXC1SXMVR`, `RZCXC19G1DM`, `RZCXC1BK67D`, `RZCY107MCLV` |
| `10.146.2.47` (hub experiment) | `RZCY10Y4TJX`, `RZCY10Y4TBY`, `RZCY10Y4TAV`, `RZCY10Y4QVX`, `RZCY10Y4HWD`, `RZCY10LGB6W`, `RZCX821GXDJ` (`a55-perf`); `RZCY10Y548K` (`stab`) |

The four eligible eight-device hosts are `10.146.2.54`, `10.146.2.53`,
`10.146.2.48`, and `10.146.2.55` (`test-1`). All other hosts have fewer than
eight eligible phones and are omitted; do not combine them to manufacture an
eight-device batch.

`10.146.2.47` is a separate eight-device mixed-group experiment. The vendor
added a USB hub to this Docker host on 2026-07-22. Its results may be compared
with the `10.146.2.55` saturation result as an external reference, but not as
a controlled before/after measurement because there is no pre-change
`10.146.2.47` baseline.

## Required runner artifact behavior

`lt_run_cmd --script` uploads `output.txt` by default. Every launch must also
pass `--artifact-path 'fleetbench-artifacts/**'` and require the JSON, log, and
manifest paths below. This uploads each Fleetbench JSON envelope and log in
addition to `output.txt` and fails a device if an expected artifact is missing.
A one-device smoke task on `R5CXC1ARZDN` completed this verification on
2026-07-20.

```text
fleetbench-artifacts/fleetbench-adb-bulk-1.json
fleetbench-artifacts/fleetbench-adb-bulk-1.log
fleetbench-artifacts/manifest.txt
```

The host-side test script must download the `v0.4.2` Linux binary and
`SHA256SUMS`, verify the binary, use the exported `$DEVICE_SERIAL`, and retain
each JSON/log pair under `fleetbench-artifacts/`.

Include these options in every `lt_run_cmd` invocation:

```bash
--artifact-path 'fleetbench-artifacts/**' \
--require-artifact-glob 'fleetbench-artifacts/**/*.json' \
--require-artifact-glob 'fleetbench-artifacts/**/*.log' \
--require-artifact-glob 'fleetbench-artifacts/manifest.txt'
```

## Workloads

Run the clean USB/bulk workload first. In the host-side script, use three full
collector invocations per device:

```bash
RUNS=3
for run in $(seq -w 1 "$RUNS"); do
  ./fleetbench-v0.4.2-linux-x86_64 adb \
    --serial "$DEVICE_SERIAL" \
    --remote-path /data/local/tmp/ \
    --sizes 25B,1M,10M,100M \
    --iterations 25B=200,1M=100,10M=30,100M=20 \
    --json \
    > "fleetbench-artifacts/fleetbench-adb-bulk-${run}.json" \
    2> "fleetbench-artifacts/fleetbench-adb-bulk-${run}.log"
done
```

The 100 MiB count is 20 per invocation so each device contributes 60 bulk
samples across three loops. The loops make overlap likely despite HyperExecute
start jitter; timestamp analysis, not an assumed barrier, decides which samples
are contended.

Run the production-path latency probe as a separate batch after the bulk run:

```bash
RUNS=3
for run in $(seq -w 1 "$RUNS"); do
  ./fleetbench-v0.4.2-linux-x86_64 adb \
    --serial "$DEVICE_SERIAL" \
    --remote-path /sdcard/Download \
    --sizes 25B,50K \
    --iterations 25B=200,50K=200 \
    --json \
    > "fleetbench-artifacts/fleetbench-adb-latency-${run}.json" \
    2> "fleetbench-artifacts/fleetbench-adb-latency-${run}.log"
done
```

## Launch procedure

1. Confirm each serial in the chosen batch is currently available and still
   maps to the stated host. If any phone is unavailable, defer that host; do
   not run a partial batch or silently substitute a phone from another host.
2. Run a one-device smoke task and verify the release version, USB device
   selection, checksum success, and artifact download.
3. Launch the host's complete `a55-perf` batch with no submission delay. This
   is the intended saturation step.
4. Download artifacts before beginning the next host.
5. Repeat the same two workloads for each approved host.

Example bulk launch for `10.146.2.54`:

```bash
cd ~/git/mozilla-bitbar-devicepool
source lt_env.sh

lt_run_cmd --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-host-10.146.2.54 \
  --device R5CXC1PW94F --device RZCXC187YCR \
  --device RZCXC16WHTA --device RZCXC16W6KT \
  --device R5CXC1PW7CR --device R5CXC1HZ4KD \
  --device R5CXC1ASH4E --device R5CXC1AHZBW
```

`lt_run_cmd` targets each serial through HyperExecute’s fixed-device selection;
the CSV grouping is what makes these eight jobs share a Docker host. The task
script discovers and exports `DEVICE_SERIAL`; the Fleetbench command must pass
it explicitly with `--serial`.

## Copy/paste HyperExecute commands

Run these from `~/git/mozilla-bitbar-devicepool` after `source lt_env.sh`.
The commands use direct serial targeting. `10.146.2.55` remains labeled
`test-1`, but is an approved host for this experiment. Run the bulk command for
an approved host first, download and inspect its artifacts, then run that host's
latency command.

### Bulk phase

```bash
lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-bulk-10.146.2.54 \
  --device R5CXC1PW94F --device RZCXC187YCR \
  --device RZCXC16WHTA --device RZCXC16W6KT \
  --device R5CXC1PW7CR --device R5CXC1HZ4KD \
  --device R5CXC1ASH4E --device R5CXC1AHZBW

lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-bulk-10.146.2.53 \
  --device R5CXC1ASHNJ --device R5CXC1AHXYD \
  --device R5CXC1HZ5PZ --device R5CXC1AHWWZ \
  --device RZCXC15YZVZ --device R5CXC1AJ07K \
  --device RZCXC189JSJ --device RZCXC19G1CT

lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-bulk-10.146.2.48 \
  --device R5CY21T22NH --device RZCX23RT6WR \
  --device R5CXC1AMNFY --device RZCX31FDGJE \
  --device RZCX71ZVF6J --device R5CX23RTKSK \
  --device RZCY204AAZD --device RZCX50TW03H

lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-bulk-10.146.2.55 \
  --device R5CXC1HZA6V --device R5CXC1ARZDN \
  --device R5CXC1HZ43J --device R5CXC1HZ85W \
  --device R5CXC1SXMVR --device RZCXC19G1DM \
  --device RZCXC1BK67D --device RZCY107MCLV

```

### Production-path latency phase

```bash
lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-latency-10.146.2.54 \
  --device R5CXC1PW94F --device RZCXC187YCR \
  --device RZCXC16WHTA --device RZCXC16W6KT \
  --device R5CXC1PW7CR --device R5CXC1HZ4KD \
  --device R5CXC1ASH4E --device R5CXC1AHZBW

lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-latency-10.146.2.53 \
  --device R5CXC1ASHNJ --device R5CXC1AHXYD \
  --device R5CXC1HZ5PZ --device R5CXC1AHWWZ \
  --device RZCXC15YZVZ --device R5CXC1AJ07K \
  --device RZCXC189JSJ --device RZCXC19G1CT

lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-latency-10.146.2.48 \
  --device R5CY21T22NH --device RZCX23RT6WR \
  --device R5CXC1AMNFY --device RZCX31FDGJE \
  --device RZCX71ZVF6J --device R5CX23RTKSK \
  --device RZCY204AAZD --device RZCX50TW03H

lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-latency-10.146.2.55 \
  --device R5CXC1HZA6V --device R5CXC1ARZDN \
  --device R5CXC1HZ43J --device R5CXC1HZ85W \
  --device R5CXC1SXMVR --device RZCXC19G1DM \
  --device RZCXC1BK67D --device RZCY107MCLV

```

## `10.146.2.47` USB-hub experiment

Run this test from `~/git/mozilla-bitbar-devicepool` after `source lt_env.sh`.
It deliberately includes the `stab` device `RZCY10Y548K`; the labels make that
exception and the hub intervention explicit. Use the direct JSON/log globs
shown below: `lt_run_cmd` searches recursively below each device's downloaded
artifact directory, while the scripts write these files directly under
`fleetbench-artifacts/`.

First run the `stab` device alone. This verifies that it is usable after the
hub change and records a standalone baseline; it is not a saturation result.

```bash
lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 1 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-hub-smoke-10.146.2.47 \
  --device RZCY10Y548K
```

If the smoke completes with all artifacts, run the complete host bulk batch;
download and inspect its artifacts before beginning the latency batch.

```bash
lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-hub-bulk-10.146.2.47 \
  --device RZCY10Y548K \
  --device RZCY10Y4TJX \
  --device RZCY10Y4TBY \
  --device RZCY10Y4TAV \
  --device RZCY10Y4QVX \
  --device RZCY10Y4HWD \
  --device RZCY10LGB6W \
  --device RZCX821GXDJ
```

```bash
lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-hub-latency-10.146.2.47 \
  --device RZCY10Y548K \
  --device RZCY10Y4TJX \
  --device RZCY10Y4TBY \
  --device RZCY10Y4TAV \
  --device RZCY10Y4QVX \
  --device RZCY10Y4HWD \
  --device RZCY10LGB6W \
  --device RZCX821GXDJ
```

## Recorded results

### 2026-07-27 — corrected-timing rerun rounds (`v0.4.2`)

The original `.55` and `.47` saturation batches used `v0.4.1`. They remain
useful historical measurements, but must not be pooled with the reruns below:
that release performed checksum verification between timed ADB transfers.
`v0.4.2` defers remote push verification and local pull verification until the
end of each complete timed loop, preserving checksum validation while keeping
the timed samples contiguous. The fix removes host-side/device-side verification
work that could otherwise pace the next transfer and change the observed USB
contention pattern.

Run the corrected rounds independently, beginning with the standard eight-device
`.55` (`test-1`) batch. Use `FLEETBENCH_VERSION=v0.4.2` and distinct labels so
the artifacts and analysis cannot be mistaken for the 2026-07-21/22 `v0.4.1`
rounds:

| Host and phase | Corrected-timing label |
|---|---|
| `.55` bulk | `fleetbench-usb-bulk-v0.4.2-rerun-10.146.2.55` |
| `.55` latency | `fleetbench-usb-latency-v0.4.2-rerun-10.146.2.55` |
| `.47` hub smoke | `fleetbench-usb-hub-smoke-v0.4.2-rerun-10.146.2.47` |
| `.47` hub bulk | `fleetbench-usb-hub-bulk-v0.4.2-rerun-10.146.2.47` |
| `.47` hub latency | `fleetbench-usb-hub-latency-v0.4.2-rerun-10.146.2.47` |

For each host, complete and inspect the bulk artifacts before launching its
latency phase. Keep `.47` as the mixed-group hub experiment, including its
`stab` device, and compare it only as an external reference to `.55`; the
timing correction does not make those hosts a controlled before/after pair.

#### Work log

| Date | Host | Phase | Version | Label | Status | Notes |
|---|---|---|---|---|---|---|
| 2026-07-27 | `.55` (`test-1`) | Bulk | `v0.4.2` | `fleetbench-usb-bulk-v0.4.2-rerun-10.146.2.55` | **Invalid — stop condition** | [Report and artifacts](~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260727_153911_180690/) contain 24 successful envelopes / 16,800 timestamped, checksum-valid transfers, but the job targeted as `RZCXC19G1DM` used `10.146.6.13:5555` for all three loops (TCP ADB). Preserve the artifacts; do not analyze as a USB result or launch latency. Fix device selection and rerun the entire `.55` host batch. |
| 2026-07-27 | `.55` (`test-1`) | One-device latency smoke (`RZCXC19G1DM`) | `v0.4.2` | `fleetbench-usb-latency-smoke-v0.4.2-10.146.2.55` | **Pass — USB selected** | [Report and artifacts](~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260727_164924_600509/) contain three successful envelopes / 2,400 timestamped, checksum-valid transfers. All loops selected bare serial `RZCXC19G1DM`, not `IP:port`; this confirms the device is currently reachable over USB. |
| 2026-07-27 | `.55` (`test-1`) | Bulk rerun | `v0.4.2` | `fleetbench-usb-bulk-v0.4.2-rerun-10.146.2.55` | **Complete — performance fail** | [Report and artifacts](~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260727_165839_878618/) contain 24 successful envelopes / 16,800 timestamped, checksum-valid transfers. All eight expected bare serials were selected, and transfer windows reached seven peers. The valid full-overlap cohort remains below the 20 MiB/s floor and above the approximately 5 s p95 target. |
| 2026-07-27 | `.55` (`test-1`) | Latency rerun | `v0.4.2` | `fleetbench-usb-latency-v0.4.2-rerun-10.146.2.55` | **Complete — pass** | [Report and artifacts](~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260727_173231_344102/) contain 24 successful envelopes / 19,200 timestamped, checksum-valid transfers. All eight expected bare serials were selected; 25 B and 50 KiB all-sample latency pass their targets. Short transfer windows reached at most four peer devices, so this is not a full-contention tail-latency result. |

#### 2026-07-27 — `.55` corrected-timing bulk rerun result

The valid `v0.4.2` rerun completed with all eight expected USB serials; no
device selected a TCP endpoint. It produced 24 successful JSON envelopes, all
16,800 timestamps and SHA-256 checks passed, and 55 push plus 59 pull 100 MiB
samples overlapped all seven peer devices.

| Cohort | Direction | Samples | Median throughput | p95 elapsed time | Status |
|---|---|---:|---:|---:|---|
| All overlap levels | Push | 480 | 13.89 MiB/s | 11.92 s | Fail |
| All overlap levels | Pull | 480 | 8.37 MiB/s | 12.91 s | Fail |
| Seven peers (full eight-device overlap) | Push | 55 | 13.98 MiB/s | 7.30 s | Fail |
| Seven peers (full eight-device overlap) | Pull | 59 | 13.47 MiB/s | 7.70 s | Fail |

The corrected-timing full-overlap result is materially better than the
2026-07-21 `v0.4.1` result (push: 8.86 MiB/s / 11.75 s; pull: 9.09 MiB/s /
12.87 s), but it still misses the 20 MiB/s throughput floor and approximately
5 s p95 target in both directions. Treat the version-to-version improvement
as observational: this is a fresh host run, not a controlled experiment.

#### 2026-07-27 — `.55` corrected-timing latency rerun result

The valid `v0.4.2` latency rerun completed with all eight expected USB serials.
It produced 24 successful JSON envelopes and 19,200 timestamped,
checksum-valid transfers. Both all-sample distributions pass their respective
targets:

| Size | Samples | Mean | Median | Standard deviation | CV | IQR | MAD | p95 | p99 | Maximum | Status |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 25 B | 9,600 | 16.28 ms | 12.96 ms | 10.58 ms | 64.96% | 15.81 ms | 6.76 ms | 32.19 ms | 52.22 ms | 123.30 ms | Pass |
| 50 KiB | 9,600 | 17.37 ms | 13.75 ms | 8.90 ms | 51.24% | 16.21 ms | 6.23 ms | 31.02 ms | 36.63 ms | 61.43 ms | Pass |

The 25-byte result passes the 375 ms mean, 500 ms p95, and 750 ms p99
criteria. The 50 KiB result passes both the 1 s p95 floor and the 500 ms
preferred target. Full eight-device overlap did not occur: 25-byte transfers
reached at most four peers (56 samples), and 50 KiB transfers reached at most
four peers (3 samples). Treat the all-sample distributions as the reliable
result; these short-transfer runs do not support a full-contention tail claim.

This round **did not reproduce Sparky's high-latency tail**. That is not a
contradiction of the historical result: the 25-byte windows here were only
about 10–30 ms and never overlapped more than four peers, whereas Sparky's
100-retrigger push-only probe produced much longer 280–1,600 ms windows. The
late-starting `.55` jobs and short, unsynchronized transfer windows prevented
the sustained high-concurrency condition needed for a like-for-like
reproduction. The planned push-only, long-running overlap launcher is the
follow-up experiment.

## Sparky-style long-running push-only overlap reproduction

Use `scripts/host_wide_adb_test/run_sparky_push_only.sh` to make each selected
device run **one** long-lived collector invocation. It uses `/sdcard/Download`,
25-byte payloads, and `--direction push`; remote SHA-256 verification and
cleanup happen only after the complete timed push loop. The default
`FLEETBENCH_PUSH_ITERATIONS=5000` is deliberately long enough to outlast normal
HyperExecute launch skew, while remaining configurable for a host's timeout
budget. The script requires `FLEETBENCH_VERSION` so operators explicitly select
a release that contains `--direction push`.

Do not set `FLEETBENCH_RUNS` for this experiment: repeating whole collector
invocations creates phase gaps and weakens overlap. Analyze the raw
`transfer_started_at_utc`/`transfer_finished_at_utc` windows across the JSON
artifacts to identify samples that actually overlapped; do not infer overlap
from submission time.

Example (substitute a release that includes push-only mode and the devices from
one host):

```bash
FLEETBENCH_VERSION=vX.Y.Z FLEETBENCH_PUSH_ITERATIONS=5000 lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_sparky_push_only.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-sparky-push-only-overlap \
  --device <serial-1> --device <serial-2> --device <serial-3>
```

For independent pull-path stress, use
`scripts/host_wide_adb_test/run_pull_only.sh` with the same HyperExecute
arguments. It stages the unique remote source files before the timed section,
then records only pull windows. Set `FLEETBENCH_PULL_ITERATIONS` (default
`5000`) to control the length; it also requires an explicit release containing
`--direction pull`. As with the push-only runner, do not use
`FLEETBENCH_RUNS`; analyze overlap from the transfer timestamps in the artifacts.

## Next test phase: `.55`, then `.47`

Run the next saturation phase on one host at a time, in this order:

1. `10.146.2.55` (`test-1`)
2. `10.146.2.47` (hub group)

For each host, run these three independent eight-device batches in order:

| phase | launcher | purpose |
|---|---|---|
| bulk | `run_bulk.sh` | Existing mixed push/pull baseline across all sizes. |
| latency push | `run_sparky_push_only.sh` | Long, contiguous 25-byte push contention window. |
| latency pull | `run_pull_only.sh` | Long, contiguous 25-byte pull contention window. |

Use a released version that contains both `--direction push` and `--direction
pull`; set it explicitly for every command. Do not run phases from different
hosts concurrently. After each batch, download and validate the JSON, log, and
manifest artifacts before launching the next batch; use each result's transfer
timestamps, rather than submission time, to determine actual overlap.

The `.55` device set is:

```text
R5CXC1HZA6V R5CXC1ARZDN R5CXC1HZ43J R5CXC1HZ85W
R5CXC1SXMVR RZCXC19G1DM RZCXC1BK67D RZCY107MCLV
```

The `.47` device set is:

```text
RZCY10Y548K RZCY10Y4TJX RZCY10Y4TBY RZCY10Y4TAV
RZCY10Y4QVX RZCY10Y4HWD RZCY10LGB6W RZCX821GXDJ
```

For either device set, substitute the appropriate label and launcher in this
command. Run it once for `run_bulk.sh`, once for `run_sparky_push_only.sh`, and
once for `run_pull_only.sh`; do not set `FLEETBENCH_RUNS`.

```bash
FLEETBENCH_VERSION=vX.Y.Z lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/<launcher> \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-<host>-<phase> \
  --device <the-eight-serials-for-this-host>
```

The commands below are the historical corrected-timing rerun record. Run them
from `~/git/mozilla-bitbar-devicepool` after `source lt_env.sh`; they retain
their explicit `v0.4.2` provenance and do not include the new pull-only phase.

#### `.55` (`test-1`) corrected-timing rerun

Run bulk first, inspect its report and artifacts, then run latency.

```bash
FLEETBENCH_VERSION=v0.4.2 lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-bulk-v0.4.2-rerun-10.146.2.55 \
  --device R5CXC1HZA6V --device R5CXC1ARZDN \
  --device R5CXC1HZ43J --device R5CXC1HZ85W \
  --device R5CXC1SXMVR --device RZCXC19G1DM \
  --device RZCXC1BK67D --device RZCY107MCLV
```

```bash
FLEETBENCH_VERSION=v0.4.2 lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/**/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-latency-v0.4.2-rerun-10.146.2.55 \
  --device R5CXC1HZA6V --device R5CXC1ARZDN \
  --device R5CXC1HZ43J --device R5CXC1HZ85W \
  --device R5CXC1SXMVR --device RZCXC19G1DM \
  --device RZCXC1BK67D --device RZCY107MCLV
```

#### `.47` hub corrected-timing rerun

Run the `stab` smoke first. Only if it finishes with all required artifacts,
run bulk, inspect its report and artifacts, and then run latency.

```bash
FLEETBENCH_VERSION=v0.4.2 lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 1 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-hub-smoke-v0.4.2-rerun-10.146.2.47 \
  --device RZCY10Y548K
```

```bash
FLEETBENCH_VERSION=v0.4.2 lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-hub-bulk-v0.4.2-rerun-10.146.2.47 \
  --device RZCY10Y548K \
  --device RZCY10Y4TJX \
  --device RZCY10Y4TBY \
  --device RZCY10Y4TAV \
  --device RZCY10Y4QVX \
  --device RZCY10Y4HWD \
  --device RZCY10LGB6W \
  --device RZCX821GXDJ
```

```bash
FLEETBENCH_VERSION=v0.4.2 lt_run_cmd \
  --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh \
  --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 \
  --artifact-path 'fleetbench-artifacts/**' \
  --require-artifact-glob 'fleetbench-artifacts/*.json' \
  --require-artifact-glob 'fleetbench-artifacts/*.log' \
  --require-artifact-glob 'fleetbench-artifacts/manifest.txt' \
  --label fleetbench-usb-hub-latency-v0.4.2-rerun-10.146.2.47 \
  --device RZCY10Y548K \
  --device RZCY10Y4TJX \
  --device RZCY10Y4TBY \
  --device RZCY10Y4TAV \
  --device RZCY10Y4QVX \
  --device RZCY10Y4HWD \
  --device RZCY10LGB6W \
  --device RZCX821GXDJ
```

### 2026-07-22 — `10.146.2.47` hub-experiment smoke

The standalone `stab` device `RZCY10Y548K` completed the required three-loop
bulk smoke with status `[OK]`. The report and downloaded artifacts are at
`~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260722_175316_311282/`.
All required JSON, log, and manifest artifacts were present; all 2,100 transfer
records had valid timestamps and successful checksums.

This is the post-hub standalone baseline for the mixed-group host, not a
saturation result. Its 100 MiB metrics are healthy:

| Direction | Samples | Median throughput | p95 elapsed time |
|---|---:|---:|---:|
| Push | 60 | 33.68 MiB/s | 3.08 s |
| Pull | 60 | 35.44 MiB/s | 2.89 s |

### 2026-07-22 — `10.146.2.47` hub-experiment bulk phase

The eight-device mixed-group batch labeled `fleetbench-usb-hub-bulk-10.146.2.47`
completed successfully. The report and downloaded artifacts are at
`~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260722_190618_573565/`.
All eight jobs completed `[OK]`; all 16,800 transfer records had valid
timestamps and successful checksums. Transfer windows reached eight simultaneous
transfers (seven peer devices), and 2,510 records overlapped all seven peers.

| Cohort | Direction | Samples | Median throughput | p95 elapsed time |
|---|---|---:|---:|---:|
| All overlap levels | Push | 480 | 24.29 MiB/s | 5.24 s |
| All overlap levels | Pull | 480 | 20.57 MiB/s | 5.67 s |
| Seven peers (full eight-device overlap) | Push | 231 | 21.17 MiB/s | 5.68 s |
| Seven peers (full eight-device overlap) | Pull | 234 | 19.93 MiB/s | 5.83 s |

At full overlap, push clears the 20 MiB/s throughput floor; pull narrowly
misses it by 0.07 MiB/s. Both directions are slightly above the approximately
5 s p95 elapsed-time target.

Compared with the post-hub standalone smoke above, full-overlap contention
reduced median push throughput by approximately 37% and pull throughput by
approximately 44%; p95 elapsed time increased by approximately 1.8× for push
and 2.0× for pull.

| Direction | `.47` single-device median / p95 | `.47` eight-device median / p95 | Contention change |
|---|---:|---:|---|
| Push | 33.68 MiB/s / 3.08 s | 21.17 MiB/s / 5.68 s | 37% lower throughput; 1.8× p95 elapsed |
| Pull | 35.44 MiB/s / 2.89 s | 19.93 MiB/s / 5.83 s | 44% lower throughput; 2.0× p95 elapsed |

The `.55` full-overlap result is an external reference: `.47` is about 2.4×
faster for push and 2.2× faster for pull, with roughly half the p95 elapsed
time. This is strong evidence that the hub configuration helps, but it is not
causal proof because the two hosts have different device sets and configurations.

### 2026-07-22 — `10.146.2.47` hub-experiment latency phase

The eight-device mixed-group batch labeled
`fleetbench-usb-hub-latency-10.146.2.47` completed successfully. The report and
downloaded artifacts are at
`~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260722_194725_615609/`.
All eight jobs completed `[OK]`; all 19,200 transfer records had valid
timestamps and successful checksums. Transfer windows reached eight simultaneous
transfers (seven peer devices) at `2026-07-23T02:48:16.709454Z`.

| Size | Samples | Mean | Median | Standard deviation | CV | IQR | MAD | p95 | p99 | Maximum |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 25 B | 9,600 | 18.01 ms | 17.33 ms | 10.69 ms | 59.36% | 18.04 ms | 9.03 ms | 32.82 ms | 52.87 ms | 74.16 ms |
| 50 KiB | 9,600 | 19.36 ms | 16.10 ms | 9.77 ms | 50.45% | 18.25 ms | 7.90 ms | 33.17 ms | 38.70 ms | 76.81 ms |

The 25-byte result passes the 375 ms mean, 500 ms p95, and 750 ms p99
criteria. The 50 KiB result passes the 1 s p95 floor and the 500 ms preferred
target. These all-sample figures are effectively the same as the `.55`
reference; the hub's observed benefit is in the saturated 100 MiB workload,
not these short transfers.

Full eight-device overlap was sparse: only eight 25-byte transfers overlapped
all seven peers, and no 50 KiB transfer did. The 50 KiB transfers reached at
most six peers. This proves eight-way overlap occurred but is insufficient for
a strong full-contention tail-latency claim.

### 2026-07-21 — `10.146.2.55` (`test-1`) bulk phase

The eight-device bulk batch labeled `fleetbench-usb-bulk-10.146.2.55`
completed successfully. The report and downloaded artifacts are at
`~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260721_120357_866372/`.

- All eight jobs completed `[OK]`, with three JSON envelopes, three logs, and
  one manifest per device.
- All 16,800 transfer records had valid timestamps and successful checksums.
- Transfer windows reached eight simultaneous transfers (seven peer devices);
  5,307 records overlapped all seven peers. The first observed eight-way
  transfer began at `2026-07-21T19:05:04.776837Z`.

100 MiB results show substantial shared-host contention:

| Cohort | Direction | Samples | Median throughput | p95 elapsed time |
|---|---|---:|---:|---:|
| All overlap levels | Push | 480 | 8.86 MiB/s | 11.93 s |
| All overlap levels | Pull | 480 | 8.41 MiB/s | 12.94 s |
| Seven peers (full eight-device overlap) | Push | 371 | 8.86 MiB/s | 11.75 s |
| Seven peers (full eight-device overlap) | Pull | 253 | 9.09 MiB/s | 12.87 s |

These results are below the 20 MiB/s throughput floor and above the
approximately 5 s p95 elapsed-time limit.

#### Single-device baseline comparison

The earlier artifact smoke on the same host and device (`R5CXC1ARZDN`) provides
a useful no-concurrent-job baseline for the 100 MiB workload. It is not a
controlled before/after experiment: it ran on 2026-07-20, before the
eight-device batch, and its primary purpose was artifact validation. The raw
report and artifacts are at
`~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260720_182130_187691/`.

The smoke completed three bulk loops with 2,100 timestamped transfers, no
checksum failures, and 60 100 MiB samples in each direction:

| Direction | Single-device samples | Median throughput | p95 elapsed time |
|---|---:|---:|---:|
| Push | 60 | 34.23 MiB/s | 2.97 s |
| Pull | 60 | 31.32 MiB/s | 3.25 s |

Compared with the full eight-device-overlap cohort above, contention reduced
median push throughput by approximately 74% and pull throughput by
approximately 71%; p95 elapsed time was approximately 4.0× higher in both
directions.

| Direction | Single-device median / p95 | Eight-device median / p95 | Contention change |
|---|---:|---:|---:|
| Push | 34.23 MiB/s / 2.97 s | 8.86 MiB/s / 11.75 s | 74% lower throughput; 4.0× p95 elapsed |
| Pull | 31.32 MiB/s / 3.25 s | 9.09 MiB/s / 12.87 s | 71% lower throughput; 4.0× p95 elapsed |

### 2026-07-21 — `10.146.2.55` (`test-1`) latency phase

The eight-device latency batch labeled `fleetbench-usb-latency-10.146.2.55`
completed successfully. The report and downloaded artifacts are at
`~/git/mozilla-bitbar-devicepool/lt_run_cmd_output/20260721_143927_043369/`.
All eight jobs completed `[OK]`; all 19,200 transfer records had valid
timestamps and successful checksums. Transfer windows reached eight simultaneous
transfers (seven peer devices) at `2026-07-21T21:40:49.228412Z`.

| Size | Samples | Mean | Median | Standard deviation | CV | IQR | MAD | p95 | p99 | Maximum |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 25 B | 9,600 | 17.56 ms | 17.06 ms | 10.98 ms | 62.55% | 17.94 ms | 9.07 ms | 32.16 ms | 51.46 ms | 229.71 ms |
| 50 KiB | 9,600 | 19.98 ms | 18.41 ms | 15.34 ms | 76.81% | 18.32 ms | 8.91 ms | 33.71 ms | 49.80 ms | 461.58 ms |

The 25-byte result passes the 375 ms mean, 500 ms p95, and 750 ms p99
criteria. The 50 KiB result passes the 1 s p95 floor and the 500 ms preferred
target.

Full eight-device overlap was necessarily sparse for these short operations:
only nine 50 KiB transfers overlapped all seven peers (p95 29.70 ms), and no
25-byte transfer did. The 25-byte transfers reached at most six peers. Treat
the all-sample latency statistics above as the reliable result; the nine-sample
full-overlap 50 KiB cohort is not sufficient for a strong tail-latency claim.

| Workload | Desired metric | Result | Status | Caveat |
|---|---|---|---|---|
| 100 MiB bulk (`10.146.2.55`) | Median ≥20 MiB/s; p95 elapsed approximately ≤5 s | Push median 8.86 MiB/s (p95 11.75 s); pull median 9.09 MiB/s (p95 12.87 s) | **Fail** | Full eight-device overlap; below the throughput floor and above the p95 target |
| 100 MiB bulk (`10.146.2.47` hub) | Median ≥20 MiB/s; p95 elapsed approximately ≤5 s | Push median 21.17 MiB/s (p95 5.68 s); pull median 19.93 MiB/s (p95 5.83 s) | **Mixed** | Mixed `a55-perf`/`stab` host; push passes throughput, while pull narrowly misses and p95 exceeds target |
| 25 B latency (`10.146.2.55`) | Mean ≤375 ms; p95 ≤500 ms; p99 ≤750 ms | Mean 17.56 ms; p95 32.16 ms; p99 51.46 ms | **Pass** | No transfer overlapped all seven peers; maximum observed overlap was six peers |
| 50 KiB latency (`10.146.2.55`) | p95 ≤1 s (≤500 ms preferred) | Mean 19.98 ms; p95 33.71 ms; p99 49.80 ms | **Pass** | Only 9 transfers overlapped all seven peers, too few for a strong full-contention tail claim |
| 25 B latency (`10.146.2.47` hub) | Mean ≤375 ms; p95 ≤500 ms; p99 ≤750 ms | Mean 18.01 ms; p95 32.82 ms; p99 52.87 ms | **Pass** | Only 8 transfers overlapped all seven peers |
| 50 KiB latency (`10.146.2.47` hub) | p95 ≤1 s (≤500 ms preferred) | Mean 19.36 ms; p95 33.17 ms; p99 38.70 ms | **Pass** | No transfer overlapped all seven peers; maximum observed overlap was six peers |

## Analysis and acceptance

For each `adb_results.iterations[]` record, use
`transfer_started_at_utc` and `transfer_finished_at_utc` to build a transfer
window. A transfer is contended only when its window overlaps another device’s
window. Report at least:

- Per-device push/pull throughput and elapsed time for all samples.
- Per-device results at each observed overlap level: 0, 1, 2, through 7 peer
  transfers.
- The overlap cohort with the greatest simultaneous-transfer count and its
  aggregate throughput.
- 25-byte and 50 KiB production-path latency mean, median, standard deviation,
  CV, IQR, MAD, p95, p99, and maximum; compare 25-byte results with the 375 ms
  mean, 500 ms p95, and 750 ms p99 criteria, and 50 KiB results with the 1 s
  p95 floor (500 ms preferred target).
- 100 MiB push/pull median throughput and p95 elapsed time; compare with the
  20 MiB/s minimum floor, 25-32 MiB/s preferred range, and approximately 5 s
  p95 elapsed-time limit.
- Every checksum failure, disconnect, retry, missing artifact, or deferred
  host due to an unavailable device.

Do not use a task’s start time as proof of contention. A valid saturated result
requires overlap in the recorded transfer windows.

## Stop conditions

Stop the current host and preserve its artifacts if any job selects TCP ADB,
reports a serial other than its `--device` target, fails SHA-256 verification,
loses a device, or omits transfer timestamps. Do not merge that host’s results
with a clean batch. Investigate and rerun the entire host after the cause is
known.
