#!/usr/bin/env python3
"""Taskcluster entry-point: fetch fleetbench, run the adb subcommand, stage artifact.

Designed to run as the `command` in a TC task on a bitbar/LambdaTest worker where
adb + a phone are already wired up. Locally it also works — set MOZ_UPLOAD_DIR to
a writable directory, or leave it unset (defaults to ./out).

Stdlib only, no third-party deps — runs on whatever Python the TC worker has.
"""

from __future__ import annotations

import hashlib
import os
import re
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

# -- knobs --------------------------------------------------------------------
FLEETBENCH_VERSION = os.environ.get("FLEETBENCH_VERSION", "v0.4.6")
FLEETBENCH_PLATFORM = os.environ.get("FLEETBENCH_PLATFORM", "linux-x86_64")
FLEETBENCH_REPO = os.environ.get("FLEETBENCH_REPO", "mozilla-platform-ops/fleetbench")
FLEETBENCH_ARGS = os.environ.get(
    "FLEETBENCH_ARGS",
    "adb --iterations 25B=5,1M=2,10M=2,100M=1 --json",
)
# Number of complete collector invocations to run within this one TC task.
# Values above one are useful for saturation experiments: launch one long-lived
# task per reserved device and let the per-device loops overlap naturally.
FLEETBENCH_RUNS = os.environ.get("FLEETBENCH_RUNS", "1")
# On hosts where 'adb devices' returns both a USB serial and a TCP endpoint
# (LambdaTest), auto-pick one transport and inject --serial. Default is 'usb'
# to match what raptor's Speedometer 3 jobs use (apples-to-apples with the
# existing perfherder series); set 'tcp' to deliberately measure the network
# path instead, or 'off' to disable auto-pick. Skipped entirely if
# FLEETBENCH_ARGS already specifies --serial. Workaround for beads ticket
# fleetbench-adb-transport-capture-filter-cf1.
AUTO_PICK = os.environ.get("FLEETBENCH_AUTO_PICK", "usb").lower()
TCP_SERIAL_RE = re.compile(r"^\d+\.\d+\.\d+\.\d+:\d+$")


def log(msg: str) -> None:
    print(f"[tc-run-adb] {msg}", file=sys.stderr, flush=True)


def fetch(url: str, dest: Path) -> None:
    log(f"fetching {url}")
    with urllib.request.urlopen(url) as resp, dest.open("wb") as f:
        shutil.copyfileobj(resp, f)


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def pick_serial(transport: str) -> str | None:
    """Return the single adb device serial matching the requested transport.

    transport: 'usb' or 'tcp'. Parses `adb devices` and returns the matching
    serial if exactly one is present. Returns None if none match. Exits if
    multiple match (ambiguous — caller should pass --serial explicitly).
    """
    try:
        out = subprocess.run(
            ["adb", "devices"], capture_output=True, text=True, check=True
        ).stdout
    except (FileNotFoundError, subprocess.CalledProcessError) as e:
        log(f"adb devices failed; skipping auto-pick: {e}")
        return None
    matches = []
    for line in out.splitlines()[1:]:  # skip 'List of devices attached'
        parts = line.split()
        if len(parts) < 2 or parts[1] != "device":
            continue
        is_tcp = bool(TCP_SERIAL_RE.match(parts[0]))
        if (transport == "tcp" and is_tcp) or (transport == "usb" and not is_tcp):
            matches.append(parts[0])
    if not matches:
        return None
    if len(matches) > 1:
        sys.exit(f"multiple {transport} adb devices: {matches}; pass --serial explicitly")
    return matches[0]


def verify_sha256(binary: Path, sums_file: Path) -> None:
    want = None
    for line in sums_file.read_text().splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1].lstrip("*") == binary.name:
            want = parts[0]
            break
    if want is None:
        sys.exit(f"no SHA256 entry found for {binary.name} in {sums_file}")
    got = sha256_of(binary)
    if got != want:
        sys.exit(f"sha256 mismatch for {binary.name}: got {got}, want {want}")
    log(f"sha256 ok: {got[:16]}…")


def main() -> int:
    upload_dir = Path(os.environ.get("MOZ_UPLOAD_DIR", Path.cwd() / "out")).resolve()
    work_dir = Path(os.environ.get("TASK_WORKDIR", tempfile.mkdtemp())).resolve()
    try:
        runs = int(FLEETBENCH_RUNS)
    except ValueError:
        sys.exit(f"FLEETBENCH_RUNS={FLEETBENCH_RUNS!r} is not a positive integer")
    if runs < 1:
        sys.exit(f"FLEETBENCH_RUNS={FLEETBENCH_RUNS!r} must be at least 1")

    upload_dir.mkdir(parents=True, exist_ok=True)
    work_dir.mkdir(parents=True, exist_ok=True)

    asset = f"fleetbench-{FLEETBENCH_VERSION}-{FLEETBENCH_PLATFORM}"
    base_url = (
        f"https://github.com/{FLEETBENCH_REPO}/releases/download/{FLEETBENCH_VERSION}"
    )
    binary = work_dir / asset
    sums = work_dir / "SHA256SUMS"

    fetch(f"{base_url}/{asset}", binary)
    fetch(f"{base_url}/SHA256SUMS", sums)
    verify_sha256(binary, sums)

    binary.chmod(binary.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    version_out = subprocess.run(
        [str(binary), "--version"], capture_output=True, text=True, check=True
    ).stdout.strip()
    log(f"binary: {version_out}")

    args = shlex.split(FLEETBENCH_ARGS)
    if AUTO_PICK in ("usb", "tcp") and "--serial" not in args:
        serial = pick_serial(AUTO_PICK)
        if serial:
            log(f"auto-picked {AUTO_PICK} adb device: {serial}")
            # Insert after the subcommand (e.g. 'adb') so flag placement is sane.
            args = [args[0], "--serial", serial, *args[1:]]
        else:
            # AUTO_PICK was explicit but no matching device — fail loudly rather
            # than silently fall back to whatever fleetbench picks on its own
            # (e.g. running USB when the operator asked for TCP).
            sys.exit(
                f"FLEETBENCH_AUTO_PICK={AUTO_PICK!r} but no {AUTO_PICK} adb device "
                f"found; set FLEETBENCH_AUTO_PICK=off to disable this check"
            )
    elif AUTO_PICK not in ("usb", "tcp", "off", ""):
        log(f"FLEETBENCH_AUTO_PICK={AUTO_PICK!r} not in (usb, tcp, off); skipping")

    cmd = [str(binary), *args]

    for run_index in range(1, runs + 1):
        suffix = "" if runs == 1 else f"-{run_index:03d}"
        output = upload_dir / f"fleetbench-adb{suffix}.json"
        err_log = upload_dir / f"fleetbench-adb{suffix}.log"

        log(f"run {run_index}/{runs}: {' '.join(shlex.quote(c) for c in cmd)}")
        log(f"stdout -> {output}")
        log(f"stderr -> {err_log}")

        with output.open("wb") as out_f, err_log.open("wb") as err_f:
            rc = subprocess.call(cmd, stdout=out_f, stderr=err_f)

        log(f"run {run_index}/{runs}: exit {rc}, artifact size: {output.stat().st_size} bytes")
        if rc:
            return rc

    return 0


if __name__ == "__main__":
    sys.exit(main())
