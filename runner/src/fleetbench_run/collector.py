"""Collector subprocess invocation.

This module owns the mechanics of calling the fleetbench collector binary and
capturing its output. It does not parse the JSON or wrap the result in an
envelope.

Timeout behavior on POSIX: the collector is launched in its own session
(``os.setsid``) so that it and any descendants share a process group. On
timeout the runner sends SIGKILL to that group, guaranteeing descendants die
along with the direct child. On Windows (where ``os.setsid`` is unavailable)
we fall back to a single-process kill via ``Popen.kill()``; the Windows Job
Object treatment lives in a separate runner task and is gated on the CPython
availability question.
"""

from __future__ import annotations

import os
import signal
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

    On POSIX, the child is placed in its own process group via ``os.setsid``;
    on timeout we kill the whole group with SIGKILL so descendants don't
    orphan. Any output produced before the kill is preserved in the result.

    Raises FileNotFoundError if the binary is missing. Orchestration is
    responsible for turning that into a failure envelope.
    """
    cmd = [binary, "cpu", "--mode", mode, "--json"]
    if extra_args:
        cmd.extend(extra_args)

    popen_kwargs = {
        "stdout": subprocess.PIPE,
        "stderr": subprocess.PIPE,
        "text": True,
    }
    if _can_setsid():
        popen_kwargs["preexec_fn"] = os.setsid

    started = _now_utc()
    proc = subprocess.Popen(cmd, **popen_kwargs)

    try:
        stdout, stderr = proc.communicate(timeout=timeout.total_seconds())
        finished = _now_utc()
        return CollectorResult(
            stdout=stdout or "",
            stderr=stderr or "",
            exit_code=proc.returncode,
            killed_by_timeout=False,
            started_utc=started,
            finished_utc=finished,
        )
    except subprocess.TimeoutExpired:
        _kill_process_group(proc)
        # Drain any remaining buffered output now that the group is dead.
        try:
            stdout, stderr = proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            stdout, stderr = "", ""
        finished = _now_utc()
        return CollectorResult(
            stdout=stdout or "",
            stderr=stderr or "",
            exit_code=None,
            killed_by_timeout=True,
            started_utc=started,
            finished_utc=finished,
        )


def _can_setsid() -> bool:
    return hasattr(os, "setsid")


def _kill_process_group(proc: subprocess.Popen) -> None:
    """SIGKILL the child's process group on POSIX; fall back to direct kill."""
    if _can_setsid() and hasattr(os, "killpg"):
        try:
            os.killpg(proc.pid, signal.SIGKILL)
            return
        except ProcessLookupError:
            return
    try:
        proc.kill()
    except ProcessLookupError:
        pass
