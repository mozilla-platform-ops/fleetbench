# ADB Testing Methodology

Background and design rationale for the `fleetbench adb` subcommand — what it
measures, why it's shaped the way it is, and the original developer test it was
built to reproduce. For usage, see the
[ADB I/O benchmark section in the README](../README.md#adb-io-benchmark-adb).

## Why this exists

`fleetbench adb` times `adb push` / `adb pull` against an attached Android
device, from the Linux Docker **host** where `adb` runs (not from the phone).
The goal is to characterize the USB/adb path that raptor sees when it stages
APKs and test assets, and to debug "why is provisioning slow today?" style
problems across device-lab vendors (bitbar vs LambdaTest).

It complements — does not replace — any in-tree raptor-side instrumentation.
Raptor's collector lives in-tree and runs per-job; `fleetbench adb` is a
standalone, controlled reproduction you can run ad hoc on a host.

## The reference test (origin)

The design reference was an experiment run by Gregory Mierzwinski ("Sparky")
on **2026-05-22**: [Try revision
`7757fbcccc8eb83105af2b9518517f47dcca9eff](https://hg-edge.mozilla.org/try/rev/7757fbcccc8eb83105af2b9518517f47dcca9eff#l1.53).
It was retriggered 100 times for an identical Android Browsertime/Speedometer 3
configuration across vendors (a55 hardware, Chrome-m, no-fission), but the
patched job did **not** run Speedometer itself.

Instead, after normal Android ADB setup, the patch ran an ad-hoc latency probe
and returned success before the test workload. Each job:

- creates one temporary text file containing `"adb-latency test payload\n"`
  (**25 bytes**);
- pushes that same file **200 times** with `self.device.push()` to a distinct
  filename under `/sdcard/Download`;
- brackets only each `push()` call with Python `time.perf_counter()`;
- emits the 200 raw timings as Perfherder `adb-push-latency` replicates; and
- removes the pushed files only after the timed loop.

Therefore the historical comparison was a **25-byte ADB push loop**, not timing
Speedometer asset staging or an ordinary end-to-end Speedometer job. The
headline result was LambdaTest push latency spanning **280 → 1600 ms**, while
BitBar stayed within **280 → 470 ms**.

`fleetbench adb --direction push --push-mode mozdevice --sizes 25B
--remote-path /sdcard/Download` matches the successful external-storage command
sequence of that probe: sync, remote `is_dir` check, push, first-call external
storage discovery, and sync. It deliberately adds raw per-iteration output and
deferred checksum verification; use
`scripts/host_wide_adb_test/run_sparky_mozdevice_exact.sh` to validate against
the literal Python client. The default `--push-mode direct` measures only one
`adb push` and is not interchangeable with either historical mode.

For a local comparison of the literal Python client and Fleetbench, use
`scripts/host_wide_adb_test/run_local_mozdevice_interleave.sh`. It runs an
even number of alternating Python/Fleetbench blocks (four 50-sample blocks per
implementation by default), preserves each raw block, and writes an aggregate
`summary.json`. This controls run order better than one complete Python run
followed by one complete Fleetbench run; it is still a single-device experiment,
not a shared-host contention measurement. Each block also records
`state-before.txt` and `state-after.txt` (ADB state, battery/thermal service,
and available host power state). The literal Python block writes opt-in
`mozdevice-phase-timings.json`; Fleetbench's matching phase timings appear in
each `adb_results.iterations[].mozdevice_phase_timings`. In both cases,
`elapsed_ms` remains the complete `ADBDevice.push()`-compatible timing; phase
timings are diagnostic only.

The exact upstream implementation used for this comparison is vendored at
[`scripts/host_wide_adb_test/reference/mozdevice-adb-7757fbcccc8eb83105af2b9518517f47dcca9eff.py`](../scripts/host_wide_adb_test/reference/mozdevice-adb-7757fbcccc8eb83105af2b9518517f47dcca9eff.py).

## The signal: distribution, not mean

The headline finding from the reference run:

- **LambdaTest** push latency spanned **280 → 1600 ms**.
- **BitBar** held a tight **280 → 470 ms**.
- The **means were misleading** — LT's mean was actually *lower* than BitBar's.
  The **distribution width was the whole story**. Both vendors share a ~280 ms
  floor; it's p95/p99/max that separates them.

This is the single most important design constraint: **the collector emits raw
per-iteration timings and never summarizes.** Median/IQR/p95 are computed in the
analysis layer. A collector that reported a median would have hidden the entire
finding. (This matches the philosophy in
[`analysis_notes.md`](analysis_notes.md).)

## Design decisions

These were resolved during design (beads ticket `fleetbench-adb-io-bench-gjb`)
and are the load-bearing parts of the methodology:

- **New top-level subcommand `fleetbench adb`.** I/O is not crammed into the
  `cpu` subcommand; the verbs stay honest and the JSON envelope shape is
  unchanged so the analysis pipeline doesn't care.
- **Measures a chain, not a host.** This times host + hub + cable + device, not
  host capability. The envelope captures device model, serial, and hub topology
  so analysis can hold the device variable constant when comparing hosts —
  otherwise a "slow host" finding is unfalsifiable.
- **One invocation, one device.** There is no in-collector `--parallel` mode.
  In production, Taskcluster schedules N independent tasks against N devices on
  a host; contention emerges from independent concurrent processes, not from one
  process fanning out. A collector-side `--parallel` would measure a scenario
  that never happens in production. Contention is recovered at analysis time by
  joining envelopes that share a host and overlap in wall-clock time. This is
  also exactly the shape of Sparky's 100-retrigger run.
- **Target selection.** One device attached → no flag needed. Multiple attached
  → must pass `--serial`, otherwise the run fails with `multiple_devices`.
- **Remote path defaults to `/data/local/tmp/`.** Avoids the FUSE layer on
  `/sdcard` for a cleaner USB/adb signal. Use `--remote-path /sdcard/Download`
  to reproduce raptor's path exactly.
- **Payload behavior.** Direct mode pre-generates N *distinct* random files
  (one per iteration, xorshift64 fill) before timing to defeat page-cache reuse.
  Mozdevice mode reuses one local payload because Sparky's probe did so.
- **Round-trip verification on separate paths.** Push → `device:/data/local/tmp/X`,
  pull → `orig.pulled`, SHA256 compare; never overwrite the source. Push is
  verified with `adb shell sha256sum` **after its complete timed push loop**,
  and pull by hashing locally **after its complete timed pull loop**.
  Deferring verification preserves back-to-back timing compatibility with
  Sparky's 25-byte probe and removes local per-sample pacing from pull
  measurements. A failed hash sets `sha256_ok = false` and exits non-zero
  (`exit 2`, correctness failure).
- **External dep capture.** `adb --version` is recorded in `adb_env`; accept
  `--adb-path` rather than assuming `PATH`.
- **USB topology.** On Linux, `lsusb -t` is captured per run for hub-path
  correlation across concurrent invocations. macOS/Windows emit no topology
  (`lsusb_topology` is null off Linux). Windows topology (WMI/`pnputil`) was
  punted.

## Sizes & iterations

Defaults emphasize the tiny payload (where vendor variance shows up), then
progressively larger transfers:

| size | default iterations | what it measures |
|---|---|---|
| 25B  | 200 | adb command/setup latency (no real bytes on wire) |
| 1M   | 100 | small-transfer steady state |
| 10M  | 30  | mid-transfer steady state |
| 100M | 10  | bulk-transfer USB throughput ceiling |

A full default run is ~720 timed transfers and takes **10–30 minutes** on a real
device (longer on slow hubs). Override per size with
`--iterations 25B=50,1M=20,...`.

### Why 25 bytes matters most

The 25-byte point matches raptor's in-tree **adb-latency probe** (a ~25-byte
temp file pushed to `/sdcard/Download`). At that size the transfer is dominated
by adb command/setup overhead — there are essentially no bytes on the wire — so
it isolates **adb-layer / USB-arbitration cost** from bandwidth. That is exactly
where the LT-vs-BitBar variance gap appeared, which makes it the most diagnostic
single data point.

## Transport finding: USB vs TCP at LambdaTest

A follow-up investigation surfaced a major confounder. When run against LT, the
collector failed with:

```
error: multiple_devices — R5CX23RT8WW, 10.146.6.230:5555
```

LT exposes **two adb transports for the device: one USB and one over TCP**
(`tcp:5555`). "Transport" is adb's own term for how it reaches `adbd`: a
USB-attached device shows up as a bare serial, a network device as `ip:port`.

**TCP transport ≠ WiFi.** A TCP endpoint only means adb is talking to `adbd`
over an IP socket instead of the USB bus. The far end could be the phone's own
WiFi radio (`adb tcpip 5555` + `adb connect`), *or* — more likely in a device
lab like LT — a network tunnel to a different rack host that has the phone
USB-attached locally. adb cannot tell you which; it only knows the connection is
over IP. Capturing wired-vs-wireless would require out-of-band lab config, not
adb itself.

Either way, network adb adds a full IP path (latency, retransmits, congestion)
that the bytes must traverse. Measured head-to-head at LT, **TCP is one-to-two
orders of magnitude slower than USB on every dimension**, and is itself wildly
heterogeneous host-to-host:

| metric | LT USB baseline | TCP run 1 | TCP run 2 | TCP run 3 | TCP run 4 |
|---|---|---|---|---|---|
| 25B push (ms) | 9.9 | 37.8 | 149.8 | 315.2 | 501.5 |
| 25B pull (ms) | 6.3 | 74.6 | 185.8 | 239.6 | 278.8 |
| 1M push (ms)  | 44.4 | 1040 | 5145 | 5646 | 7229 |
| 10M push (ms) | 319 | 9752 | 14624 | 33605 | 41386 |

Effective throughput: TCP ≈ **150 KB/s – 1 MB/s** vs USB ≈ **25 MB/s**.

Implications:

- **USB-at-LT is the right comparison target** against BitBar. The clean USB
  numbers (LT in isolation: ~10 ms push, ~25–35 MB/s ceiling) are comparable to
  BitBar's. Always disambiguate with `--serial` and confirm you're measuring USB
  on both sides — comparing LT-over-TCP to BitBar-over-USB is apples to oranges.
- **The TCP endpoint looks like an unmaintained/best-effort backup**, not a
  production path. Worth confirming with LT whether anything actually uses it.
- **Follow-up gap:** the collector should capture `transport` (`usb` vs `tcp`)
  per device in `adb_env` so this asymmetry is visible in the data rather than
  inferred from the serial string — note this can only report `usb`/`tcp`, not
  the physical link (WiFi vs tunnel) behind a TCP endpoint. Tracked in
  `fleetbench-adb-transport-capture-filter-cf1`.

This run also validated `fleetbench adb` itself: same binary, same workload,
cleanly surfaced a ~100× performance difference with only n=2–5 per size.

## Per-iteration output

Each timed transfer emits: device serial, device model, hub path (lsusb), file
size, direction (push/pull), transfer start/end timestamps in UTC with
microsecond precision, bytes/sec, `elapsed_ms`, `sha256_ok`. Direct-mode
timestamps bracket one `adb push`/`pull` subprocess; mozdevice-mode timestamps
bracket its complete compatibility sequence. These land in
`adb_results.iterations` in a schema-compatible envelope
(`schema_version`, `host`, `env`, `config`, `results` siblings:
`adb_config` / `adb_env` / `adb_results`).

## Validation status

- **macOS + real phone (validated end-to-end):** Apple Silicon M4 Pro dev box
  with a Pixel 10 Pro over USB. 21/21 iterations passed SHA256 across
  25B/1M/10M/100M. 25B ran ~25–46 ms (pure command/setup overhead); 100M hit
  ~34 MB/s push and ~39 MB/s pull (pull consistently faster — known adb
  asymmetry). Even at n=5 the 25B push tail was ~40% wider than the median —
  the distribution shape the design called out.
- **Linux + real phone (not yet exercised on the real target):** bitbar/LT-style
  Docker host validation is environmental, not code work. The Linux-only env
  capture (`/proc/stat`, `/proc/loadavg`, `lsusb -t`) is the same code that
  ships in `cpu` and is exercised by that command's Linux fleet runs, but the
  full `fleetbench adb` path has not been run on a production bitbar/LT host.

## Open follow-ups

- Capture `transport` (`usb`/`tcp`) per device in `adb_env`, plus a
  `--transport` filter (`fleetbench-adb-transport-capture-filter-cf1`).
- Validate the full `adb` path on a real bitbar/LT Linux Docker host.
- Windows USB topology capture (WMI / `pnputil`).

## References

- [Proposed Android USB/ADB service-level requirements](ANDROID_USB_SLA.md).
- Beads ticket: `fleetbench-adb-io-bench-gjb` (design decisions, closed
  2026-05-27).
- [README — ADB I/O benchmark](../README.md#adb-io-benchmark-adb) (usage).
- [`analysis_notes.md`](analysis_notes.md) (distribution-over-mean analysis
  philosophy, Android-specific behavior).
