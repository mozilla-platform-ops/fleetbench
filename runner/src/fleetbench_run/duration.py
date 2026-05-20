"""Human-friendly duration parsing for CLI args."""

from __future__ import annotations

import re
from datetime import timedelta

_PATTERN = re.compile(r"^(\d+)([smhd])$")

_UNIT_SECONDS = {
    "s": 1,
    "m": 60,
    "h": 60 * 60,
    "d": 60 * 60 * 24,
}


class DurationParseError(ValueError):
    pass


def parse_duration(s: str) -> timedelta:
    """Parse a duration like "30m", "1h", "24h", "7d" into a timedelta.

    Accepts a non-negative integer followed by a single unit suffix:
        s -> seconds
        m -> minutes
        h -> hours
        d -> days

    Compound forms ("1h30m"), fractional values ("1.5h"), bare numbers
    ("60"), and unsupported units ("1y", "1w") are rejected. Zero is
    allowed (equivalent to "run every invocation").
    """
    if not isinstance(s, str):
        raise DurationParseError(f"duration must be a string, got {type(s).__name__}")
    s = s.strip()
    m = _PATTERN.match(s)
    if not m:
        raise DurationParseError(
            f"invalid duration {s!r}: expected <integer><s|m|h|d>, e.g. '30m', '1h', '7d'"
        )
    value = int(m.group(1))
    unit = m.group(2)
    return timedelta(seconds=value * _UNIT_SECONDS[unit])
