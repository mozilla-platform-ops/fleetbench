from datetime import timedelta

import pytest

from fleetbench_run.duration import DurationParseError, parse_duration


@pytest.mark.parametrize("text,expected", [
    ("60s", timedelta(seconds=60)),
    ("30m", timedelta(minutes=30)),
    ("1h", timedelta(hours=1)),
    ("24h", timedelta(hours=24)),
    ("7d", timedelta(days=7)),
    ("0h", timedelta(0)),  # zero allowed: "run every invocation"
])
def test_valid_durations(text, expected):
    assert parse_duration(text) == expected


def test_leading_and_trailing_whitespace_stripped():
    assert parse_duration("  1h  ") == timedelta(hours=1)


@pytest.mark.parametrize("text", [
    "",
    "h",
    "1",
    "1.5h",
    "1h30m",   # compound not supported
    "1y",      # unsupported unit
    "1w",      # unsupported unit
    "-1h",     # negative not allowed
    "abc",
    " ",
])
def test_invalid_durations_raise(text):
    with pytest.raises(DurationParseError):
        parse_duration(text)


def test_non_string_input_raises():
    with pytest.raises(DurationParseError):
        parse_duration(60)  # type: ignore[arg-type]
