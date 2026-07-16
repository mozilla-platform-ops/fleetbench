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
| 25-byte push and pull | p95 <= 50 ms; p99 <= 100 ms | ADB command/setup and arbitration latency |
| 1 MiB push and pull | p95 <= 100 ms | Small-transfer performance |
| 10 MiB push and pull | p95 <= 500 ms | Mid-sized asset transfer |
| 100 MiB push and pull | median throughput >= 25 MiB/s and p95 elapsed <= 4.5 s | Bulk-transfer ceiling |

Preferred bulk throughput is **30-40 MiB/s per active device**. Results must not
contain a recurring multi-hundred-millisecond latency tail for the 25-byte
operation, even if the mean remains within target.

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
`/data/local/tmp/`, which avoids `/sdcard` filesystem overhead and gives a clean
ADB/USB measurement.

Run an additional production-path validation when test jobs stage files through
shared storage:

```bash
fleetbench adb --serial <usb-serial> \
  --remote-path /sdcard/Download --json
```

Acceptance is evaluated per device and per direction using the raw distribution,
including p50, p95, p99, and maximum. A fleet-wide mean must not be used to hide
slow devices, overloaded hubs, or long-tail outliers.

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
- Raw per-iteration Fleetbench JSON, not only aggregate averages.
- Per-device and per-direction p50, p95, p99, maximum, and bulk throughput.
- Per-device and aggregate throughput at each tested concurrency level.
- All disconnects, retries, checksum failures, and excluded samples.

## Rationale and current baseline

Clean, isolated direct-USB measurements at LambdaTest produced approximately
25-35 MiB/s with about 10 ms of 25-byte push latency, broadly comparable to the
available BitBar smoke-test result. That BitBar run reached approximately
32 MiB/s for a 100 MiB transfer, with 25-byte operations below 41 ms. A directly
attached Pixel validation reached about 34 MiB/s push and 39 MiB/s pull.

These measurements establish that both vendors' isolated USB paths could meet
the proposed numerical objectives. They do **not** establish performance at
peak multi-device transfer concurrency: the available BitBar artifact used only
one Fleetbench process on a lightly loaded host, and the LambdaTest comparison
was also an isolated USB measurement.

In contrast, measured LambdaTest TCP ADB endpoints ranged from roughly
150 KiB/s to 1 MiB/s and had much higher, more variable latency.

The distinction matters operationally: a device can be physically attached to
USB somewhere in a vendor rack while the test host still reaches it through a
network tunnel. That architecture adds network latency, congestion,
retransmissions, and additional queues to the path being measured.

For the benchmark methodology and supporting measurements, see
[`ADB_TESTING.md`](ADB_TESTING.md).
