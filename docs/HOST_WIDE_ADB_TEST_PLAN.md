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
- Do not target any `test-1` device. The entire host `10.146.2.55` is reserved
  for unrelated testing and is excluded below.
- Run one host batch at a time. Do not combine serials from different hosts in a
  single launch; that would measure fleet-wide concurrency, not shared-host USB
  contention.
- Use `FLEETBENCH_VERSION=v0.4.1` or later. This is the first release with
  per-transfer timestamps needed to prove overlap.
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

Excluded `test-1` host, `10.146.2.55`: `R5CXC1HZA6V`, `R5CXC1ARZDN`,
`R5CXC1HZ43J`, `R5CXC1HZ85W`, `R5CXC1SXMVR`, `RZCXC19G1DM`,
`RZCXC1BK67D`, and `RZCY107MCLV`.

The three eligible eight-device `a55-perf` hosts are `10.146.2.54`,
`10.146.2.53`, and `10.146.2.48`. All other hosts have fewer than eight
eligible `a55-perf` phones and are omitted; do not combine them to manufacture
an eight-device batch.

## Required runner artifact behavior

`lt_run_cmd --script` currently uploads `output.txt` only. That is insufficient:
the experiment needs every Fleetbench JSON envelope and log. Before the first
real batch, make the run-command HyperExecute configuration upload
`fleetbench-artifacts/**` in addition to `output.txt`, then verify a one-device
smoke task downloads:

```text
fleetbench-artifacts/fleetbench-adb-001.json
fleetbench-artifacts/fleetbench-adb-001.log
```

The host-side test script must download the `v0.4.1` Linux binary and
`SHA256SUMS`, verify the binary, use the exported `$DEVICE_SERIAL`, and retain
each JSON/log pair under `fleetbench-artifacts/`.

## Workloads

Run the clean USB/bulk workload first. In the host-side script, use three full
collector invocations per device:

```bash
RUNS=3
for run in $(seq -w 1 "$RUNS"); do
  ./fleetbench-v0.4.1-linux-x86_64 adb \
    --serial "$DEVICE_SERIAL" \
    --remote-path /data/local/tmp/ \
    --sizes 25B,1M,10M,100M \
    --iterations 25B=200,1M=100,10M=30,100M=20 \
    --json \
    > "fleetbench-artifacts/fleetbench-adb-${run}.json" \
    2> "fleetbench-artifacts/fleetbench-adb-${run}.log"
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
  ./fleetbench-v0.4.1-linux-x86_64 adb \
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
The commands use direct serial targeting and intentionally contain no
`10.146.2.55` / `test-1` device. Run the bulk command for an approved host
first, download and inspect its artifacts, then run that host's latency command.

### Bulk phase

```bash
lt_run_cmd --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 --label fleetbench-usb-bulk-10.146.2.54 --device R5CXC1PW94F --device RZCXC187YCR --device RZCXC16WHTA --device RZCXC16W6KT --device R5CXC1PW7CR --device R5CXC1HZ4KD --device R5CXC1ASH4E --device R5CXC1AHZBW

lt_run_cmd --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 --label fleetbench-usb-bulk-10.146.2.53 --device R5CXC1ASHNJ --device R5CXC1AHXYD --device R5CXC1HZ5PZ --device R5CXC1AHWWZ --device RZCXC15YZVZ --device R5CXC1AJ07K --device RZCXC189JSJ --device RZCXC19G1CT

lt_run_cmd --script ~/git/fleetbench/scripts/host_wide_adb_test/run_bulk.sh --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 --label fleetbench-usb-bulk-10.146.2.48 --device R5CY21T22NH --device RZCX23RT6WR --device R5CXC1AMNFY --device RZCX31FDGJE --device RZCX71ZVF6J --device R5CX23RTKSK --device RZCY204AAZD --device RZCX50TW03H

```

### Production-path latency phase

```bash
lt_run_cmd --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 --label fleetbench-usb-latency-10.146.2.54 --device R5CXC1PW94F --device RZCXC187YCR --device RZCXC16WHTA --device RZCXC16W6KT --device R5CXC1PW7CR --device R5CXC1HZ4KD --device R5CXC1ASH4E --device R5CXC1AHZBW

lt_run_cmd --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 --label fleetbench-usb-latency-10.146.2.53 --device R5CXC1ASHNJ --device R5CXC1AHXYD --device R5CXC1HZ5PZ --device R5CXC1AHWWZ --device RZCXC15YZVZ --device R5CXC1AJ07K --device RZCXC189JSJ --device RZCXC19G1CT

lt_run_cmd --script ~/git/fleetbench/scripts/host_wide_adb_test/run_latency.sh --parallel 8 --start-delay 0 --timeout 2700 --queue-timeout 900 --retries 0 --label fleetbench-usb-latency-10.146.2.48 --device R5CY21T22NH --device RZCX23RT6WR --device R5CXC1AMNFY --device RZCX31FDGJE --device RZCX71ZVF6J --device R5CX23RTKSK --device RZCY204AAZD --device RZCX50TW03H

```

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
