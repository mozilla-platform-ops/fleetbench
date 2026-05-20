"""Collector subprocess invocation.

This module owns the mechanics of calling the fleetbench collector binary and
capturing its output. It does not parse the JSON or wrap the result in an
envelope — that lives in the orchestration layer.

The current implementation uses subprocess.run with a wall-clock timeout. The
process-group SIGKILL machinery promised by the design (runner task .7) lands
on top of this; until then the timeout kills the direct child only.
"""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from typing import Optional


@dataclass(frozen=True)
class CollectorResult:
    stdout: str
    stderr: str
    exit_code: Optional[int]
    killed_by_timeout: bool
    started_utc: str
    finished_utc: str


def _now_utc() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def run_collector(
    binary: str,
    mode: str,
    timeout: timedelta,
    extra_args: Optional[list[str]] = None,
) -> CollectorResult:
    """Invoke the collector and capture stdout, stderr, and exit code.

    The collector is always invoked with `cpu --mode <mode> --json`. Additional
    flags can be passed via ``extra_args`` (used by tests).

    Returns a CollectorResult populated regardless of how the process exited.
    On timeout, the child is killed and ``killed_by_timeout`` is set; any
    output produced before the kill is preserved.

    Raises FileNotFoundError if the binary cannot be located. Upstream
    orchestration is responsible for turning that into a failure envelope.
    """
    cmd = [binary, "cpu", "--mode", mode, "--json"]
    if extra_args:
        cmd.extend(extra_args)

    started = _now_utc()
    try:
        cp = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout.total_seconds(),
        )
        finished = _now_utc()
        return CollectorResult(
            stdout=cp.stdout,
            stderr=cp.stderr,
            exit_code=cp.returncode,
            killed_by_timeout=False,
            started_utc=started,
            finished_utc=finished,
        )
    except subprocess.TimeoutExpired as e:
        finished = _now_utc()
        return CollectorResult(
            stdout=_to_text(e.stdout),
            stderr=_to_text(e.stderr),
            exit_code=None,
            killed_by_timeout=True,
            started_utc=started,
            finished_utc=finished,
        )


def _to_text(b) -> str:
    """TimeoutExpired carries bytes or str depending on the call shape."""
    if b is None:
        return ""
    if isinstance(b, bytes):
        return b.decode("utf-8", errors="replace")
    return b
