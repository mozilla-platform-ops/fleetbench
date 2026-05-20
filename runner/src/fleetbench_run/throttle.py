"""Throttle decision: should we run, given the results-dir state?

Operational model: the runner is invoked frequently (e.g. on every boot)
and self-throttles by inspecting the newest envelope in the results dir.
Any envelope counts as a run, success or failure — that's important so a
degraded host doesn't burn through every reboot retrying.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Optional

from fleetbench_run.filenames import is_final_envelope, parse_started_utc


@dataclass(frozen=True)
class ThrottleDecision:
    should_run: bool
    reason: str
    last_run_utc: Optional[datetime]


def find_latest_envelope_time(results_dir: Path) -> Optional[datetime]:
    """Return the newest envelope's started_utc, or None if no envelopes exist.

    Reads filenames only; does not open any files. Skips ``*.partial`` and
    any filename that does not match the envelope shape (defensive against
    unrelated files in the dir).
    """
    if not results_dir.is_dir():
        return None
    latest: Optional[datetime] = None
    try:
        entries = os.listdir(results_dir)
    except OSError:
        return None
    for name in entries:
        if not is_final_envelope(name):
            continue
        ts = parse_started_utc(name)
        if ts is None:
            continue
        if latest is None or ts > latest:
            latest = ts
    return latest


def decide(
    now_utc: datetime,
    results_dir: Path,
    min_interval: timedelta,
) -> ThrottleDecision:
    """Decide whether to run, given the current time and a min_interval."""
    if now_utc.tzinfo is None:
        raise ValueError("now_utc must be timezone-aware")
    last = find_latest_envelope_time(results_dir)
    if last is None:
        return ThrottleDecision(
            should_run=True,
            reason="no prior envelope in results dir",
            last_run_utc=None,
        )
    elapsed = now_utc - last
    if elapsed >= min_interval:
        return ThrottleDecision(
            should_run=True,
            reason=(
                f"last run {_fmt(elapsed)} ago, exceeds min_interval "
                f"{_fmt(min_interval)}"
            ),
            last_run_utc=last,
        )
    next_in = min_interval - elapsed
    return ThrottleDecision(
        should_run=False,
        reason=(
            f"last run {_fmt(elapsed)} ago, less than min_interval "
            f"{_fmt(min_interval)}; next run in {_fmt(next_in)}"
        ),
        last_run_utc=last,
    )


def _fmt(d: timedelta) -> str:
    """Render a timedelta as a short human-friendly duration."""
    total = int(d.total_seconds())
    sign = "-" if total < 0 else ""
    total = abs(total)
    days, rem = divmod(total, 86400)
    hours, rem = divmod(rem, 3600)
    minutes, seconds = divmod(rem, 60)
    parts = []
    if days:
        parts.append(f"{days}d")
    if hours:
        parts.append(f"{hours}h")
    if minutes:
        parts.append(f"{minutes}m")
    if seconds or not parts:
        parts.append(f"{seconds}s")
    return sign + "".join(parts)
