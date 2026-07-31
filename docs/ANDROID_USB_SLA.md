# Android USB/ADB Service-Level Requirements

Status: **Proposed vendor requirements**

## Purpose

These requirements define the Android Debug Bridge (ADB) performance expected
from a hosted Android device lab. They apply to the complete path used by a test
job:

```text
test host/container -> ADB server -> USB controller/hubs/cables -> Android device
```

The nominal USB version alone is not an acceptance criterion. Host load, shared
hubs, cables, device storage, and ADB overhead all affect the performance seen
by test jobs, so acceptance is based on end-to-end `adb push` and `adb pull`
measurements.

## Transport requirements

1. Devices must be available to the test host through direct USB ADB using USB
   2.0 High-Speed (480 Mb/s) or better.
2. A network-backed ADB endpoint, such as `IP:port`, must not be presented as
   equivalent to direct USB merely because the device is USB-attached to a
   remote rack host.
3. TCP-tunneled ADB may be offered as a separate transport, but the vendor must
   disclose it and report its performance separately. It is equivalent to
   direct USB only if it independently meets every latency, throughput,
   reliability, and concurrency requirement in this document.
4. If both USB and TCP endpoints are exposed for one device, the vendor must
   document how jobs select the intended transport and prevent accidental
   selection of the other endpoint.
5. The vendor must disclose both the maximum installed device density and the
   maximum supported number of simultaneous ADB transfers per host, together
   with the USB controller/hub topology serving those devices.

An ADB serial that looks like `192.0.2.10:5555` identifies a TCP transport. A
hardware-looking serial normally identifies USB, but naming alone is not
sufficient proof: transport architecture and topology must also be disclosed.

## Capacity and concurrency definitions

- **Installed device density** is the total number of devices connected to a
  host, whether or not they are transferring data at the same moment.
- **Transfer concurrency** is the number of devices simultaneously running an
  ADB push or pull.
- **Agreed peak concurrent-transfer load** is the highest transfer concurrency
  the vendor and customer expect the production workload to reach. It must be
  stated as a concrete number of devices per host.

Installing many devices does not imply that they can all transfer at full speed
simultaneously. Conversely, a single-device demonstration does not establish
performance at production concurrency. The vendor must characterize the
relationship between these two conditions.

## Performance objectives

The following objectives apply **per active device with the host populated at
full production device density and operating at the agreed peak
concurrent-transfer load**, not only in a single-device test. Transfer sizes are
binary (1 MiB = 1,048,576 bytes).

| Operation | Required objective | Purpose |
|---|---:|---|
| Production latency probe: 25-byte push to `/sdcard/Download` | mean near 375 ms; p95 <= 500 ms; p99 <= 750 ms; standard deviation approximately <= 125 ms | End-to-end latency under production conditions |
| Mozdevice-compatible setup probe: 25-byte push to `/sdcard/Download` | mean near 375 ms; p95 <= 500 ms; p99 <= 750 ms; standard deviation approximately <= 125 ms; rooted fleets require `mozdevice_root_mode == "su_c"` | End-to-end `mozdevice.ADBDevice.push()` setup path used by test jobs |
| 100 MiB push and pull to `/data/local/tmp/` | median throughput >= 20 MiB/s; preferred range 25-32 MiB/s per device; p95 elapsed approximately <= 5.0 s | Bulk-transfer floor and target |
| Small-transfer probe: 50 KiB push and pull | p95 elapsed <= 1.0 s; preferred p95 <= 500 ms | Detects congestion hidden by bulk averages |

### Basis for the formal values

The performance team supplied this expectation as the starting point:

> For latency, we should ask for a maximum high-load latency of around ~500 ms
> with a mean near ~375 ms and no more than ~25-30% variation around the mean.
> For bandwidth, asking for 25-32 MiB/s seems reasonable. Some outliers for
> latency/bandwidth would be fine, but overall we need to see a distribution of
> the latency around a low mean and for there not to be a long tail.

This SLA turns that guidance into statistics that can be reproduced across
runs. The approximately 125 ms variation corresponds to the requested
25-30%-of-mean spread (125 ms / 375 ms is approximately 33%); it is represented
by standard deviation rather than by an informal interval called “variance.”

The p95 and p99 limits are the operational tail requirements. An isolated
maximum above those limits should be reported and investigated, but a single
outlier does not by itself characterize the service. A recurring tail or a
failure of the p95/p99 limits is non-compliant.

### Mozdevice-compatible setup latency

The mozdevice-compatible 25-byte probe is an additional workload requirement,
not a replacement for the direct production-latency probe or the bulk-transfer
objectives. It measures the successful `mozdevice.ADBDevice.push()` path:
device-shell synchronization, remote-directory check, push, first-call
external-storage discovery, and post-push synchronization. For tiny payloads,
this setup work dominates the elapsed time and represents test-job behavior
that a single direct `adb push` does not capture.

Run it with `fleetbench adb --direction push --push-mode mozdevice`, targeting
`/sdcard/Download` with a 25-byte payload. Every result must report
`adb_config.mozdevice_root_mode`. On fleets where the production mozdevice
client has root access, acceptance requires `su_c`; `direct_fallback` is a
valid unrooted-client measurement but must be reported separately and cannot
be treated as equivalent to the rooted workload.

This probe has the same statistical objectives as the production 25-byte
latency probe: mean near 375 ms, p95 at most 500 ms, p99 at most 750 ms, and
standard deviation approximately at most 125 ms. The requirements apply to
the complete outer operation; phase timings are diagnostic evidence for a
regression, not separate pass/fail metrics.

### Additional throughput and small-transfer guidance

The follow-up guidance was:

> 20 Mb/s would be fine with me. I’m not sure if MB/s is the right thing to
> target though. An average of 20 Mb/s can still hide a small 50 KiB transfer
> taking one second. High-load congestion is the issue, so the acceptance test
> must measure both sustained throughput and small-transfer latency. The vendor
> should also confirm whether additional PCIe USB controllers are available to
> remove shared-controller contention.

This SLA uses **MiB/s** (mebibytes per second) consistently; vendor reports must
not mix bits and bytes. The formal minimum sustained-throughput floor is a
per-device median of 20 MiB/s for 100 MiB transfers. The preferred operating
range remains 25-32 MiB/s. At the 20 MiB/s floor, a 100 MiB transfer completes
in approximately five seconds, which is why the corresponding p95 elapsed-time
limit is approximately 5.0 seconds.

Bulk throughput cannot waive small-operation latency. A 50 KiB probe must have
p95 elapsed time no greater than one second, with 500 ms as the preferred
target. This probe is evaluated independently for push and pull and under the
same high-load concurrency as the bulk test. The vendor must document available
PCIe USB controllers and state whether adding controllers, changing hub
topology, or reducing devices per controller is required to meet these values.

Expected bulk throughput is **25-32 MiB/s per active device**, with 20 MiB/s as
the minimum acceptable floor. Higher throughput is welcome but is not required.

Fleetbench's 25-byte `/data/local/tmp/` operation and its 1 MiB and 10 MiB
transfers remain useful diagnostic measurements. They isolate command overhead
and show how quickly the connection reaches steady state, but they are not
separate vendor pass/fail thresholds in this SLA.

These objectives describe the end-to-end ADB path, not raw USB signaling speed.
USB 3.x is welcome but is not required when USB 2.0 High-Speed meets the
measured objectives.

## Reliability requirements

- Every completed push/pull round trip must pass SHA-256 verification.
- The run must have no unexpected device disconnects, transport changes, ADB
  server restarts, or retries hidden from the reported results.
- Performance must remain within the objectives during sustained concurrent
  operation at the agreed peak concurrent-transfer load while the host remains
  populated at full production device density.
- Scheduled maintenance or an explicitly declared degraded transport must be
  reported separately from compliant USB service.

## Acceptance test

Use the Fleetbench ADB workload from the Linux host or container where
production jobs run. Populate the host at full production device density, then
execute one Fleetbench process per actively transferring device. Repeat at
increasing concurrency through the agreed peak concurrent-transfer load:

```bash
fleetbench adb --serial <usb-serial> --json
```

The default workload measures 25-byte, 1 MiB, 10 MiB, and 100 MiB payloads and
emits raw per-iteration push/pull timings. It uses distinct pre-generated files
and verifies transfers with SHA-256. The default remote path is
`/data/local/tmp/`, which avoids `/sdcard` filesystem overhead and provides the
bulk-throughput acceptance measurement plus diagnostic latency data.

Add a 50 KiB payload to the acceptance run (or run it as a separate probe) so
that small-transfer latency is measured directly; the default payload set does
not substitute for this probe.

Run the latency acceptance test through the production shared-storage path:

```bash
fleetbench adb --serial <usb-serial> \
  --remote-path /sdcard/Download --json
```

Acceptance is evaluated per device and per direction using the raw distribution.
For latency, report mean, median, standard deviation, coefficient of variation,
IQR, MAD, p95, p99, and maximum. For bandwidth, report mean, median, p05, p10,
p25, p75, p95, IQR, and coefficient of variation. Include bootstrap 95%
confidence intervals for the mean and the key tail statistics. A fleet-wide
mean must not be used to hide slow devices, overloaded hubs, or long-tail
outliers.

The latency target is met when the per-device mean is near 375 ms, p95 is at
most 500 ms, p99 is at most 750 ms, and standard deviation is approximately at
most 125 ms (coefficient of variation approximately at most 33%). The bandwidth
target floor is met when the per-device median is at least 20 MiB/s; the
preferred operating range is 25-32 MiB/s. Report the pooled lower tail (p10)
separately to expose devices that a median could conceal. The 50 KiB latency
floor is met when p95 is at most one second. These criteria apply to push and
pull independently.

The shape of the latency distribution is diagnostic rather than a requirement
to pass a formal normality test. A skewed or multimodal distribution is
acceptable only if it still satisfies the quantile and dispersion limits and
does not show a recurring long tail.

The acceptance report must include a concurrency curve showing per-device and
aggregate performance with 1, 2, 4, and successively more simultaneous
transfers through the agreed peak load. If those exact steps do not fit the host
size, use equivalent increasing steps and document them. The agreed peak—not
the total number of installed devices—is the concurrency level at which every
per-device performance objective must be met.

## Required test report

The vendor's acceptance report must include:

- Device manufacturer, model, Android version, and ADB serial.
- Whether each endpoint is direct USB or TCP/network-backed ADB.
- ADB client/server versions and the host/container configuration.
- USB link speed, controller/hub topology, and devices sharing each upstream
  link.
- Total devices installed on the host, the agreed peak concurrent-transfer
  load, and the number actively transferring in each test step.
- Raw per-iteration Fleetbench JSON, including transfer start/end timestamps,
  not only aggregate averages.
- Per-device and per-direction latency mean, median, standard deviation, CV, IQR,
  MAD, p95, p99, maximum, and bootstrap 95% confidence intervals.
- Per-device and per-direction bandwidth mean, median, p05, p10, p25, p75, p95,
  IQR, CV, and bootstrap 95% confidence intervals.
- 50 KiB push/pull p95 latency, separately from the bulk-transfer statistics.
- Per-device and aggregate throughput at each tested concurrency level.
- All disconnects, retries, checksum failures, and excluded samples.

## Rationale and current baseline

Clean, isolated direct-USB measurements at LambdaTest produced approximately
25-35 MiB/s with about 10 ms of diagnostic 25-byte push latency, broadly
comparable to the available BitBar smoke-test result. That BitBar run reached
approximately 32 MiB/s for a 100 MiB transfer, with diagnostic 25-byte
operations below 41 ms. A directly attached Pixel validation reached about
34 MiB/s push and 39 MiB/s pull.

These measurements establish that both vendors' isolated USB paths could meet
the bulk-throughput objective. They do **not** establish performance at peak
multi-device transfer concurrency: the available BitBar artifact used only one
Fleetbench process on a lightly loaded host, and the LambdaTest comparison was
also an isolated USB measurement.

In the older production-path comparison, BitBar push latency stayed within
approximately 280-470 ms, consistent with the proposed high-load latency
objective. LambdaTest results ranged from approximately 280-1,600 ms and would
not meet it. That historical data did not capture transport identity, so it
cannot determine whether LambdaTest's long tail came from direct USB, TCP ADB,
or a mixture of the two paths.

In contrast, measured LambdaTest TCP ADB endpoints ranged from roughly
150 KiB/s to 1 MiB/s and had much higher, more variable latency.

The distinction matters operationally: a device can be physically attached to
USB somewhere in a vendor rack while the test host still reaches it through a
network tunnel. That architecture adds network latency, congestion,
retransmissions, and additional queues to the path being measured.

For the benchmark methodology and supporting measurements, see
[`ADB_TESTING.md`](ADB_TESTING.md).
