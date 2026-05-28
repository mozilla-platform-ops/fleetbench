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
FLEETBENCH_VERSION = os.environ.get("FLEETBENCH_VERSION", "v0.4.0")
FLEETBENCH_PLATFORM = os.environ.get("FLEETBENCH_PLATFORM", "linux-x86_64")
FLEETBENCH_REPO = os.environ.get("FLEETBENCH_REPO", "mozilla-platform-ops/fleetbench")
FLEETBENCH_ARGS = os.environ.get(
    "FLEETBENCH_ARGS",
    "adb --iterations 25B=5,1M=2,10M=2,100M=1 --json",
)
# When set (default), auto-pick the TCP-attached adb device and inject --serial
# into FLEETBENCH_ARGS if it doesn't already specify one. Workaround for LT
# hosts having both a USB and a TCP adb device — see fleetbench beads ticket
# fleetbench-adb-transport-capture-filter-cf1 for the durable in-collector fix.
AUTO_PICK_TCP = os.environ.get("FLEETBENCH_AUTO_PICK_TCP", "1") == "1"
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


def pick_tcp_serial() -> str | None:
    """Return the single TCP-attached adb device serial, or None.

    Parses `adb devices` looking for entries shaped like `<ip>:<port>` in the
    'device' state. Returns None if none found, exits if multiple TCP devices
    are present (ambiguous — caller should pass --serial explicitly).
    """
    try:
        out = subprocess.run(
            ["adb", "devices"], capture_output=True, text=True, check=True
        ).stdout
    except (FileNotFoundError, subprocess.CalledProcessError) as e:
        log(f"adb devices failed; skipping auto-pick: {e}")
        return None
    tcp = []
    for line in out.splitlines()[1:]:  # skip 'List of devices attached'
        parts = line.split()
        if len(parts) >= 2 and parts[1] == "device" and TCP_SERIAL_RE.match(parts[0]):
            tcp.append(parts[0])
    if not tcp:
        return None
    if len(tcp) > 1:
        sys.exit(f"multiple TCP adb devices: {tcp}; pass --serial explicitly")
    return tcp[0]


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
    if AUTO_PICK_TCP and "--serial" not in args:
        tcp_serial = pick_tcp_serial()
        if tcp_serial:
            log(f"auto-picked TCP adb device: {tcp_serial}")
            # Insert after the subcommand (e.g. 'adb') so flag placement is sane.
            args = [args[0], "--serial", tcp_serial, *args[1:]]

    output = upload_dir / "fleetbench-adb.json"
    err_log = upload_dir / "fleetbench-adb.log"
    cmd = [str(binary), *args]

    log(f"running: {' '.join(shlex.quote(c) for c in cmd)}")
    log(f"stdout -> {output}")
    log(f"stderr -> {err_log}")

    with output.open("wb") as out_f, err_log.open("wb") as err_f:
        rc = subprocess.call(cmd, stdout=out_f, stderr=err_f)

    log(f"exit {rc}, artifact size: {output.stat().st_size} bytes")
    return rc


if __name__ == "__main__":
    sys.exit(main())
