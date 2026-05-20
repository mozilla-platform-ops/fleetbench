"""Envelope filename construction and parsing.

Filename shape: ``<ts>_<host>_<suite>_<run_id>.json``

- ``<ts>``      ISO-8601 UTC with ``:`` replaced by ``-`` for Windows
                compatibility, e.g. ``2026-05-20T19-15-02Z``. Sortable
                lexicographically across hosts and time.
- ``<host>``    hostname, sanitized to ``[A-Za-z0-9-]``. Dots in FQDNs
                collapse to hyphens.
- ``<suite>``   collector ``cpu_suite_version`` (e.g. ``cpu-v0``), sanitized.
- ``<run_id>``  uuid4 hex (32 chars). Uniqueness is sufficient; sort order
                comes from the leading timestamp.

In-progress writes use the same name plus ``.partial``. Collection tooling
and analysis must skip ``*.partial``.
"""

from __future__ import annotations

import os
import re
import uuid
from datetime import datetime, timezone
from typing import Optional

_FINAL_SUFFIX = ".json"
_PARTIAL_SUFFIX = ".json.partial"
_SAFE_CHARS = re.compile(r"[^A-Za-z0-9-]")


def make_envelope_filename(
    started_utc: datetime,
    hostname: str,
    suite: str,
    run_id: str,
) -> str:
    if started_utc.tzinfo is None:
        raise ValueError("started_utc must be timezone-aware")
    if started_utc.utcoffset() != timezone.utc.utcoffset(None):
        # Normalize to UTC for the filename; we only emit "Z".
        started_utc = started_utc.astimezone(timezone.utc)
    ts = started_utc.strftime("%Y-%m-%dT%H-%M-%SZ")
    host = _sanitize(hostname)
    suite_clean = _sanitize(suite)
    return f"{ts}_{host}_{suite_clean}_{run_id}{_FINAL_SUFFIX}"


def partial_path_for(final_path: str) -> str:
    """Append the .partial suffix to a final envelope path."""
    if not final_path.endswith(_FINAL_SUFFIX):
        raise ValueError(f"expected a {_FINAL_SUFFIX} path, got {final_path!r}")
    return final_path + ".partial"


def is_partial(name: str) -> bool:
    return name.endswith(_PARTIAL_SUFFIX)


def is_final_envelope(name: str) -> bool:
    """True for completed envelope files (excludes .partial)."""
    return name.endswith(_FINAL_SUFFIX) and not is_partial(name)


def parse_started_utc(filename: str) -> Optional[datetime]:
    """Extract the leading timestamp from an envelope filename.

    Returns a timezone-aware UTC datetime, or None if the filename does not
    match the expected shape. Accepts both bare basenames and full paths.
    """
    base = os.path.basename(filename)
    if not is_final_envelope(base):
        return None
    head = base.split("_", 1)[0]
    # Expect "YYYY-MM-DDTHH-MM-SSZ"
    if len(head) != 20 or head[-1] != "Z" or head[10] != "T":
        return None
    date_part = head[:10]
    time_core = head[11:-1]
    parts = time_core.split("-")
    if len(parts) != 3 or not all(p.isdigit() and len(p) == 2 for p in parts):
        return None
    hh, mm, ss = parts
    try:
        return datetime.fromisoformat(f"{date_part}T{hh}:{mm}:{ss}+00:00")
    except ValueError:
        return None


def generate_run_id() -> str:
    """Return a 32-char hex uuid4 suitable for the filename's run_id slot."""
    return uuid.uuid4().hex


def _sanitize(s: str) -> str:
    cleaned = _SAFE_CHARS.sub("-", s).strip("-")
    return cleaned or "unknown"
