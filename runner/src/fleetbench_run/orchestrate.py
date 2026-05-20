"""Orchestrate a single runner invocation.

Pulls together collector subprocess, JSON parse, envelope build, and atomic
disk write. Hard failures — non-JSON stdout, panic, signal kill, timeout,
missing binary — flow through the same path as a clean run and produce a
failure envelope that's still written to disk. The runner exits 0 as long
as an envelope was written; the collector's status lives inside it.
"""

from __future__ import annotations

import json
import socket
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Optional, Tuple

from fleetbench_run.collector import CollectorResult, run_collector
from fleetbench_run.disk import write_envelope
from fleetbench_run.envelope import Envelope, build_envelope
from fleetbench_run.filenames import generate_run_id, make_envelope_filename

_UNKNOWN_SUITE = "unknown-suite"
_TS_FMT = "%Y-%m-%dT%H:%M:%SZ"


def parse_collector_output(stdout: str) -> Tuple[Optional[dict], Optional[str]]:
    """Try to parse the collector's stdout as a JSON object.

    Returns ``(dict, None)`` on success, ``(None, reason)`` on failure.
    The collector is contracted to emit a single JSON object per run with
    ``--json`` — anything else (empty stdout, JSON array, JSON scalar,
    syntax error) is a hard failure.
    """
    if stdout is None or not stdout.strip():
        return None, "collector produced no stdout"
    try:
        obj = json.loads(stdout)
    except json.JSONDecodeError as e:
        return None, f"stdout is not valid JSON: {e}"
    if not isinstance(obj, dict):
        return None, f"collector output was not a JSON object: got {type(obj).__name__}"
    return obj, None


def perform_run(
    *,
    results_dir: Path,
    mode: str,
    collector_binary: str,
    trigger: str,
    timeout: timedelta,
    hostname: Optional[str] = None,
) -> Tuple[Envelope, Path]:
    """Run the collector once, build an envelope, write atomically.

    Returns the (envelope, final_path) pair. Always writes an envelope file
    even on hard collector failure.
    """
    try:
        cr = run_collector(collector_binary, mode, timeout)
        parsed, parse_error = parse_collector_output(cr.stdout)
    except FileNotFoundError:
        cr = _synthetic_missing_binary_result(collector_binary)
        parsed, parse_error = None, f"collector binary not found: {collector_binary}"

    run_id = generate_run_id()
    envelope = build_envelope(
        run_id=run_id,
        trigger=trigger,
        collector_result=cr,
        parsed_output=parsed,
        parse_error=parse_error,
    )
    suite = (parsed.get("cpu_suite_version") if parsed else None) or _UNKNOWN_SUITE
    host = hostname or socket.gethostname()
    started_dt = _parse_ts(cr.started_utc)
    filename = make_envelope_filename(started_dt, host, suite, run_id)
    final_path = write_envelope(envelope, results_dir, filename)
    return envelope, final_path


def _synthetic_missing_binary_result(binary: str) -> CollectorResult:
    now = datetime.now(timezone.utc).strftime(_TS_FMT)
    return CollectorResult(
        stdout="",
        stderr=f"collector binary not found: {binary}",
        exit_code=None,
        killed_by_timeout=False,
        started_utc=now,
        finished_utc=now,
    )


def _parse_ts(s: str) -> datetime:
    return datetime.strptime(s, _TS_FMT).replace(tzinfo=timezone.utc)
